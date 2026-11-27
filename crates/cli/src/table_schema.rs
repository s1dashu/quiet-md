//! A 数据表's shape from the CLI: fields (add, change, delete), views (list,
//! add, set, rename, delete), the tables of a file (new, rename, remove), and
//! rows imported by their column names. Each goes through the engine's own
//! edit (`ops::Doc`), so it does what the app does: a type change converts
//! the cells, a 双向关联's mirror follows, a removed table's links turn to text.
//! What the engine would quietly adjust (a taken name, the primary field's
//! type, the last view) is refused here instead, with the reason.

use fdl_core::table::{BaseOp, DateFmt, Field, FieldId, FieldOpts, FieldType, Filter, NumberStyle, RecId, Rollup, SelectOption, Table, TableId, Value as Cell, ViewKind, csv, field, formula, query};
use serde_json::{Value, json};

use crate::fail::{R, fail};
use crate::session::{Session, arg, flag, need};
use crate::tables::{cell_text, cond_arg, filter_text, field_names, field_of, field_type, items, key_of, loose, names, names_arg, sort_arg, sort_text, stat_named, view_kind, view_of};

/// `,`, tab or `;`: whichever the CSV's first line has most of (as `csv::import` reads it).
fn separator(text: &str) -> char {
    let line = text.lines().next().unwrap_or("");
    let n = |c: char| line.matches(c).count();
    if n('\t') > n(',').max(n(';')) { '\t' } else if n(';') > n(',') { ';' } else { ',' }
}

/// What the engine's `change_field` keeps a primary field to (it must read as a title).
fn titles(ty: FieldType) -> bool { ty.textual() || matches!(ty, FieldType::Number | FieldType::Date | FieldType::Formula | FieldType::AutoNumber | FieldType::Currency | FieldType::Phone | FieldType::Email) }

/// A name a new field, view or table can take: the file can carry it, and nothing has it.
fn free(what: &str, name: &str, taken: bool) -> R<String> {
    let n = name.trim();
    if n.is_empty() { return Err(fail("bad_args", format!("a {what} needs a name"))); }
    if what != "view" && !fdl_core::table::sqlite_name_ok(n) { return Err(fail("bad_args", format!("{n:?} cannot name a {what}: not empty, not starting with _ or sqlite_, not id"))); }
    if taken { return Err(fail("exists", format!("there is a {what} called {n:?} already"))); }
    Ok(n.to_string())
}

/// `--format`: settings joined with commas, each one of: `plain`, `thousands`,
/// `percent`; a digit (decimal places); `currency=$` (or the symbol alone on
/// a 货币 field); `stars=N`; `prefix=X`; a date format (`dash`, `cn`… or its
/// sample, `2026-01-30`); `time` / `notime`.
fn set_format(f: &mut Field, s: &str) -> R<()> {
    let o = &mut f.opts;
    for p in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let low = p.to_lowercase();
        let bad = || fail("bad_args", format!("format {p:?}: use plain, thousands, percent, a digit (decimal places), currency=SYMBOL, stars=N, prefix=TEXT, time, notime or a date format ({})", DateFmt::ALL.iter().map(|d| key_of(*d)).collect::<Vec<_>>().join(", ")));
        if let Some(n) = low.strip_prefix("stars=").or(low.strip_prefix("max=")) { o.max = n.parse::<u8>().ok().filter(|n| (1..=10).contains(n)).ok_or_else(bad)?; }
        else if let Some(x) = p.strip_prefix("prefix=") { o.prefix = x.to_string(); }
        else if let Some(x) = p.strip_prefix("currency=") { o.currency = x.to_string(); }
        else if let Ok(n) = low.trim_start_matches('.').parse::<u8>() && n <= 10 { o.precision = Some(n); }
        else if let Some(st) = [NumberStyle::Plain, NumberStyle::Thousands, NumberStyle::Percent].into_iter().find(|x| key_of(*x) == low || (low == "千分位" && *x == NumberStyle::Thousands) || (low == "百分比" && *x == NumberStyle::Percent)) { o.style = st; }
        else if low == "time" { o.time = true; }
        else if low == "notime" { o.time = false; }
        else if let Some(d) = DateFmt::ALL.iter().copied().find(|d| loose(&key_of(*d)) == loose(p) || d.sample() == p) { o.date = d; }
        else if f.ty == FieldType::Currency && p.chars().count() <= 3 && !p.chars().any(char::is_alphanumeric) { o.currency = p.to_string(); }
        else { return Err(bad()); }
    }
    Ok(())
}

fn rollup_named(s: &str) -> R<Rollup> {
    Rollup::ALL.iter().copied().find(|r| loose(&key_of(*r)) == loose(s) || loose(r.label()) == loose(s))
        .ok_or_else(|| fail("bad_args", format!("rollup {s:?}: use {}", Rollup::ALL.iter().map(|r| key_of(*r)).collect::<Vec<_>>().join(", "))))
}

/// A formula checked against the table: where it is wrong, by character.
fn check_formula(t: &Table, src: &str) -> R<()> {
    let Some((start, end, msg)) = formula::check(src, t) else { return Ok(()) };
    let (from, to) = (src[..start].chars().count(), src[..end.max(start)].chars().count());
    let bit: String = src[start..end.max(start)].to_string();
    Err(fail("bad_formula", format!("{msg} at {}{}{}", from + 1, if to > from + 1 { format!("–{to}") } else { String::new() }, if bit.is_empty() { String::new() } else { format!(" ({bit:?})") }))
        .hint(format!("fields are {{name}} or the bare name; fields: {}", field_names(t))))
}

impl Session {
    /// A field's settings from `a`: options, formula, link, lookup, format, rollup.
    fn field_opts(&mut self, k: usize, tid: TableId, f: &mut Field, a: &Value, new: bool) -> R<()> {
        let base = &self.tables[k].doc.base;
        let t = base.table(tid).unwrap();
        let want = |key: &str| a.get(key).is_some_and(|v| !v.is_null());
        let only = |key: &str, ok: bool, what: &str| if want(key) && !ok { Err(fail("bad_args", format!("--{key} is for {what} fields; {} is a {}", f.name, f.ty.key()))) } else { Ok(()) };
        only("options", f.ty.select(), "single_select, multi_select and person")?;
        only("formula", f.ty == FieldType::Formula, "formula")?;
        only("link", f.ty == FieldType::Link, "link")?;
        only("lookup", f.ty == FieldType::Lookup, "lookup")?;
        only("rollup", f.ty == FieldType::Lookup, "lookup")?;
        if let Some(src) = arg(a, "formula") { check_formula(t, src)?; f.opts.formula = src.to_string(); }
        else if new && f.ty == FieldType::Formula { return Err(fail("bad_args", "a formula field needs --formula EXPR")); }
        if f.ty == FieldType::Link {
            if let Some(n) = arg(a, "link") { f.opts.link_table = Some(base.table_named(n).ok_or_else(|| fail("not_found", format!("no table {n:?}; tables: {}", names(base))))?.id); }
            else if new { f.opts.link_table = Some(tid); }
            if flag(a, "single") { f.opts.single = true; }
            if new && flag(a, "two_way") { if f.opts.link_table == Some(tid) { return Err(fail("bad_args", "a two-way link goes to another table")); } f.opts.back = Some(0); }
        }
        if f.ty == FieldType::Lookup {
            if let Some(spec) = arg(a, "lookup") {
                // LINKFIELD.FIELD: either name may hold a dot, so each split is tried
                let found = spec.match_indices('.').find_map(|(i, _)| {
                    let via = t.field_named(spec[..i].trim()).filter(|v| v.ty == FieldType::Link)?;
                    let other = base.table(via.opts.link_table?)?;
                    Some((via.id, other.field_named(spec[i + 1..].trim())?.id))
                });
                let (via, target) = found.ok_or_else(|| {
                    let links: Vec<&str> = t.fields.iter().filter(|f| f.ty == FieldType::Link).map(|f| f.name.as_str()).collect();
                    fail("bad_args", format!("--lookup {spec:?}: write LINKFIELD.FIELD, a link field of {} and a field of the table it links to", t.name)).hint(if links.is_empty() { "this table has no link field: add one first (--type link --link TABLE)".to_string() } else { format!("link fields: {}", links.join(", ")) })
                })?;
                (f.opts.via, f.opts.target) = (Some(via), Some(target));
            } else if new { return Err(fail("bad_args", "a lookup field needs --lookup LINKFIELD.FIELD")); }
            if let Some(r) = arg(a, "rollup") { f.opts.rollup = rollup_named(r)?; }
        }
        let ids = names_arg(a, "options");
        if want("options") {
            // the list as given: options kept by name, new ones added, the rest dropped
            let mut list = vec![];
            for (i, n) in ids.iter().enumerate() {
                match f.option_named(n) { Some(o) => list.push(o.clone()), None => list.push(SelectOption { id: self.tables[k].doc.base.new_id(), name: n.clone(), color: (i % 10) as u8 }) }
            }
            f.opts.options = list;
        }
        if let Some(fmt) = arg(a, "format") { set_format(f, fmt)?; }
        Ok(())
    }

    pub(crate) fn table_field_add(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let t = self.tab(k, tid);
        let n = need(a, "name")?;
        let name = free("field", n, t.fields.iter().any(|f| f.name.eq_ignore_ascii_case(n.trim())))?;
        let ty = field_type(arg(a, "type").unwrap_or("text"))?;
        let at = match arg(a, "after") { Some(n) => { let id = field_of(t, n)?.id; t.fields.iter().position(|f| f.id == id).map(|i| i + 1) } None => None };
        let mut f = Field { id: 0, name: name.clone(), ty, opts: field::default_opts(ty), desc: arg(a, "description").unwrap_or("").to_string() };
        self.field_opts(k, tid, &mut f, a, true)?;
        let doc = &mut self.tables[k].doc;
        let id = doc.add_field(tid, at, &f.name, ty, f.opts.clone()).ok_or_else(|| fail("io", "the field could not be added"))?;
        if !f.desc.is_empty() { let mut x = doc.base.table(tid).unwrap().field(id).unwrap().clone(); x.desc = f.desc.clone(); doc.change_field(tid, x); }
        let t = self.tab(k, tid);
        let made = t.field(id).unwrap();
        let mut out = json!({ "src": self.tables[k].src, "table": t.name, "added": made.name, "type": ty.key() });
        if let Some(b) = made.opts.back.and_then(|b| made.opts.link_table.and_then(|o| self.tables[k].doc.base.table(o)).and_then(|o| o.field(b).map(|x| format!("{}.{}", o.name, x.name)))) { out["mirror"] = json!(b); }
        let tname = t.name.clone();
        self.tables[k].log.push(format!("+ field {name} ({}) in {tname}", ty.key()));
        Ok(out)
    }

    /// `name`, `type`, `options`, `add_options`, `remove_options`,
    /// `rename_options` ({old: new} or ["old=new"]), `formula`, `link`,
    /// `lookup`, `rollup`, `format`. Reports cells a type change could not carry over.
    pub(crate) fn table_field_change(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let t = self.tab(k, tid);
        let old = field_of(t, need(a, "field")?)?.clone();
        let primary = t.fields[0].id == old.id;
        let mut f = old.clone();
        if let Some(n) = arg(a, "name").filter(|n| n.trim() != old.name) {
            f.name = free("field", n, t.fields.iter().any(|x| x.id != old.id && x.name.eq_ignore_ascii_case(n.trim())))?;
        }
        if let Some(ty) = arg(a, "type") {
            f.ty = field_type(ty)?;
            if primary && !titles(f.ty) { return Err(fail("bad_args", format!("{} is the primary field (the first column): it cannot be a {}", old.name, f.ty.key())).hint("it can be text, url, phone, email, location, barcode, number, currency, date, formula or auto_number")); }
            if f.ty != old.ty { let keep = std::mem::take(&mut f.opts); f.opts = FieldOpts { options: keep.options, ..field::default_opts(f.ty) }; }
        }
        let retyped = f.ty != old.ty;
        self.field_opts(k, tid, &mut f, a, retyped)?;
        // options by name: added, removed (the cells holding them say so), renamed
        let (adds, removes) = (names_arg(a, "add_options"), names_arg(a, "remove_options"));
        let renames: Vec<(String, String)> = match a.get("rename_options") {
            Some(Value::Object(o)) => o.iter().map(|(x, y)| (x.clone(), cell_text(y))).collect(),
            _ => names_arg(a, "rename_options").iter().map(|p| p.split_once('=').map(|(x, y)| (x.trim().to_string(), y.trim().to_string())).ok_or_else(|| fail("bad_args", format!("rename option {p:?}: write OLD=NEW")))).collect::<R<_>>()?,
        };
        if (!adds.is_empty() || !removes.is_empty() || !renames.is_empty()) && !f.ty.select() { return Err(fail("bad_args", format!("{} has no options: it is a {}", f.name, f.ty.key()))); }
        let opts = |f: &Field| f.opts.options.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join(", ");
        for (x, y) in &renames {
            if f.option_named(y).is_some() { return Err(fail("exists", format!("{} has an option {y:?} already", f.name))); }
            let at = f.opts.options.iter().position(|o| &o.name == x).ok_or_else(|| fail("not_found", format!("{} has no option {x:?}", f.name)).hint(format!("options: {}", opts(&f))))?;
            f.opts.options[at].name = y.clone();
        }
        let mut gone = vec![];
        for x in &removes {
            let at = f.opts.options.iter().position(|o| &o.name == x).ok_or_else(|| fail("not_found", format!("{} has no option {x:?}", f.name)).hint(format!("options: {}", opts(&f))))?;
            gone.push(f.opts.options.remove(at).id);
        }
        for x in &adds {
            if f.option_named(x).is_some() { continue; }
            let color = (f.opts.options.len() % 10) as u8;
            f.opts.options.push(SelectOption { id: self.tables[k].doc.base.new_id(), name: x.clone(), color });
        }
        if let Some(d) = arg(a, "description") { f.desc = d.to_string(); }
        if f == old { return Ok(json!({ "src": self.tables[k].src, "field": old.name, "changed": false })); }
        // what the cells were, to tell which the change empties
        let base = &self.tables[k].doc.base;
        let t = base.table(tid).unwrap();
        let before: Vec<(RecId, String)> = t.records.iter().filter(|r| !r.get(old.id).is_empty()).map(|r| (r.id, field::display_in(base, &old, r.get(old.id)))).collect();
        let dropped_opts: Vec<(RecId, String)> = if f.ty.select() { t.records.iter().filter(|r| matches!(r.get(old.id), Cell::Select(ids) if ids.iter().any(|i| !f.opts.options.iter().any(|o| o.id == *i)))).map(|r| (r.id, field::display_in(base, &old, r.get(old.id)))).collect() } else { vec![] };
        let doc = &mut self.tables[k].doc;
        doc.change_field(tid, f.clone());
        let t = self.tables[k].doc.base.table(tid).unwrap();
        let now = t.field(old.id).unwrap().clone();
        let lost: Vec<Value> = if now.ty != old.ty && !now.ty.computed() {
            before.iter().filter(|(id, _)| t.record(*id).is_some_and(|r| r.get(old.id).is_empty())).map(|(id, s)| json!({ "row": format!("r{id}"), "was": s })).collect()
        } else { vec![] };
        let mut out = json!({ "src": self.tables[k].src, "table": t.name, "field": now.name, "type": now.ty.key(), "changed": true });
        if now.ty != old.ty { out["converted"] = json!(before.len() - lost.len()); out["lost"] = json!(lost.len()); if !lost.is_empty() { out["lost_rows"] = json!(lost.iter().take(20).collect::<Vec<_>>()); } }
        if now.ty.computed() && !old.ty.computed() && !before.is_empty() { out["replaced"] = json!(before.len()); }
        if !gone.is_empty() || !dropped_opts.is_empty() { out["cells_cleared"] = json!(dropped_opts.len()); }
        let mut what = vec![];
        if now.name != old.name { what.push(format!("renamed to {}", now.name)); }
        if now.ty != old.ty { what.push(format!("{} → {}{}", old.ty.key(), now.ty.key(), if lost.is_empty() { String::new() } else { format!(", {} value(s) lost", lost.len()) })); }
        if !renames.is_empty() { what.push(format!("options renamed: {}", renames.iter().map(|(x, y)| format!("{x} → {y}")).collect::<Vec<_>>().join(", "))); }
        if !removes.is_empty() { what.push(format!("options removed: {} ({} cell(s) changed)", removes.join(", "), dropped_opts.len())); }
        if !adds.is_empty() { what.push(format!("options added: {}", adds.join(", "))); }
        if what.is_empty() { what.push("settings".into()); }
        self.tables[k].log.push(format!("~ field {}: {}", old.name, what.join("; ")));
        Ok(out)
    }

    pub(crate) fn table_field_delete(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let t = self.tab(k, tid);
        let list = names_arg(a, "fields");
        if list.is_empty() { return Err(fail("bad_args", "fields: the names of the fields to delete")); }
        let mut ids: Vec<FieldId> = vec![];
        for n in &list {
            let f = field_of(t, n)?;
            if t.fields[0].id == f.id { return Err(fail("bad_args", format!("{} is the primary field (the first column): every table keeps one", f.name)).hint("rename it or change its type instead (`quiet table field change`)")); }
            ids.push(f.id);
        }
        // what reads them: formulas and lookups that will show #引用失效
        let mut uses = vec![];
        for f in t.fields.iter().filter(|f| !ids.contains(&f.id)) {
            let reads = match f.ty {
                FieldType::Formula => formula::compile(&f.opts.formula, t).map(|e| formula::deps(&e).iter().any(|d| ids.contains(d))).unwrap_or(false),
                FieldType::Lookup => f.opts.via.is_some_and(|v| ids.contains(&v)),
                _ => false,
            };
            if reads { uses.push(f.name.clone()); }
        }
        let base = &self.tables[k].doc.base;
        let mirrors: Vec<String> = ids.iter().filter_map(|id| t.field(*id)).filter_map(|f| { let o = base.table(f.opts.link_table?)?; o.field(f.opts.back?).map(|b| format!("{}.{}", o.name, b.name)) }).collect();
        let tname = t.name.clone();
        self.tables[k].doc.remove_fields(tid, &ids);
        let mut out = json!({ "src": self.tables[k].src, "table": tname, "deleted": list });
        if !mirrors.is_empty() { out["mirrors_deleted"] = json!(mirrors); }
        if !uses.is_empty() { out["broken"] = json!(uses); }
        self.tables[k].log.push(format!("- field {} in {tname}{}", list.join(", "), if mirrors.is_empty() { String::new() } else { format!(" (and its mirror {})", mirrors.join(", ")) }));
        Ok(out)
    }

    /// Each view with its settings, conditions as they read.
    pub(crate) fn table_views(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let base = &self.tables[k].doc.base;
        let t = base.table(tid).unwrap();
        let name = |id: FieldId| t.field(id).map_or("?".to_string(), |f| f.name.clone());
        let views: Vec<Value> = t.views.iter().map(|w| {
            let o = &w.opts;
            let mut v = json!({ "name": w.name, "kind": key_of(w.kind) });
            if !o.filter.conds.is_empty() { v["where"] = json!(filter_text(t, None, &o.filter)); }
            if !o.sorts.is_empty() { v["sort"] = json!(o.sorts.iter().map(|s| sort_text(t, s)).collect::<Vec<_>>()); }
            if !o.groups.is_empty() { v["group"] = json!(o.groups.iter().map(|s| sort_text(t, s)).collect::<Vec<_>>()); }
            if !o.hidden.is_empty() { v["hidden"] = json!(t.ordered_fields(w).iter().filter(|f| o.hidden.contains(&f.id)).map(|f| f.name.as_str()).collect::<Vec<_>>()); }
            if !o.order.is_empty() { v["order"] = json!(t.ordered_fields(w).iter().map(|f| f.name.as_str()).collect::<Vec<_>>()); }
            for (key, f) in [("stack", o.stack), ("date", o.date), ("end", o.end)] { if let Some(f) = f { v[key] = json!(name(f)); } }
            if !o.stats.is_empty() { v["stats"] = json!(o.stats.iter().map(|(f, s)| (name(*f), json!(key_of(*s)))).collect::<serde_json::Map<_, _>>()); }
            v["rows"] = json!(query::run(base, t, w).records.len());
            v
        }).collect();
        Ok(json!({ "src": self.tables[k].src, "table": t.name, "views": views }))
    }

    pub(crate) fn table_view_add(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let t = self.tab(k, tid);
        let n = need(a, "name")?;
        let name = free("view", n, t.views.iter().any(|v| v.name == n.trim()))?;
        let kind = arg(a, "kind").map(view_kind).transpose()?.unwrap_or(ViewKind::Grid);
        let tname = t.name.clone();
        self.tables[k].doc.add_view(tid, kind, &name).ok_or_else(|| fail("io", "the view could not be added"))?;
        self.tables[k].log.push(format!("+ view {name} ({}) in {tname}", key_of(kind)));
        // what it shows, so a following `view set` can be written knowingly
        let shown = self.table_views(a)?["views"].as_array().and_then(|l| l.iter().find(|v| v["name"] == json!(name)).cloned()).unwrap_or_default();
        Ok(json!({ "src": self.tables[k].src, "table": tname, "added": shown }))
    }

    pub(crate) fn table_view_rename(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let t = self.tab(k, tid);
        let v = view_of(t, need(a, "view")?)?;
        let (vid, old) = (v.id, v.name.clone());
        let n = need(a, "name")?;
        let name = free("view", n, t.views.iter().any(|x| x.id != vid && x.name == n.trim()))?;
        self.tables[k].doc.rename_view(tid, vid, &name);
        self.tables[k].log.push(format!("~ view {old} → {name}"));
        Ok(json!({ "src": self.tables[k].src, "view": old, "renamed": name }))
    }

    pub(crate) fn table_view_delete(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let t = self.tab(k, tid);
        let v = view_of(t, need(a, "view")?)?;
        if t.views.len() <= 1 { return Err(fail("bad_args", format!("{} is the only view of {}: a table keeps one", v.name, t.name)).hint("add another view first, or change this one with `quiet table view set`")); }
        let (vid, name) = (v.id, v.name.clone());
        self.tables[k].doc.run_in(tid, vec![fdl_core::table::Op::RemoveView(vid)]);
        self.tables[k].log.push(format!("- view {name}"));
        Ok(json!({ "src": self.tables[k].src, "deleted": name }))
    }

    /// A view's settings: `clear` first (filter, sort, group, stats, hidden),
    /// then `where` (+ `any`) replaces its 筛选, `sort` and `group` theirs,
    /// `hide` / `show` / `order` the fields, `stack` / `date` / `end` the
    /// fields a 看板 / 日历 / 甘特 uses, `stats` {field: stat}.
    pub(crate) fn table_view_set(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let t = self.tab(k, tid);
        let mut v = view_of(t, need(a, "view")?)?.clone();
        let before = v.clone();
        for c in names_arg(a, "clear") {
            match loose(&c).as_str() {
                "filter" | "where" => v.opts.filter = Filter::default(),
                "sort" | "sorts" => v.opts.sorts.clear(),
                "group" | "groups" => v.opts.groups.clear(),
                "stats" => v.opts.stats.clear(),
                "hidden" | "hide" => v.opts.hidden.clear(),
                "order" => v.opts.order.clear(),
                x => return Err(fail("bad_args", format!("--clear {x}: use filter, sort, group, stats, hidden or order"))),
            }
        }
        let conds = items(a, "where").iter().map(|w| cond_arg(t, w)).collect::<R<Vec<_>>>()?;
        if !conds.is_empty() { v.opts.filter = Filter { all: !flag(a, "any"), conds }; } else if flag(a, "any") { v.opts.filter.all = false; }
        if a.get("sort").is_some() { v.opts.sorts = items(a, "sort").iter().map(|s| sort_arg(t, s)).collect::<R<_>>()?; }
        if a.get("group").is_some() {
            v.opts.groups = items(a, "group").iter().map(|s| sort_arg(t, s)).collect::<R<_>>()?;
            if v.opts.groups.len() > 3 { return Err(fail("bad_args", "at most 3 group levels")); }
        }
        let ids = |key: &str| names_arg(a, key).iter().map(|n| field_of(t, n).map(|f| f.id)).collect::<R<Vec<FieldId>>>();
        for id in ids("hide")? {
            if t.fields[0].id == id { return Err(fail("bad_args", format!("{} is the primary field: it always shows", t.fields[0].name))); }
            v.opts.hidden.insert(id);
        }
        for id in ids("show")? { v.opts.hidden.remove(&id); }
        let order = ids("order")?;
        if !order.is_empty() { v.opts.order = order; }
        for (key, ok, what) in [("stack", (|f: &Field| f.ty.select()) as fn(&Field) -> bool, "a single_select, multi_select or person"), ("date", |f: &Field| f.ty.is_date(), "a date"), ("end", |f: &Field| f.ty.is_date(), "a date")] {
            let Some(n) = arg(a, key) else { continue };
            let f = field_of(t, n)?;
            if !ok(f) { return Err(fail("bad_args", format!("--{key} takes {what} field; {} is a {}", f.name, f.ty.key()))); }
            let slot = match key { "stack" => &mut v.opts.stack, "date" => &mut v.opts.date, _ => &mut v.opts.end };
            *slot = Some(f.id);
        }
        let stats: Vec<(String, String)> = match a.get("stats") {
            Some(Value::Object(o)) => o.iter().map(|(f, s)| (f.clone(), cell_text(s))).collect(),
            _ => names_arg(a, "stats").iter().map(|p| p.split_once('=').map(|(x, y)| (x.trim().to_string(), y.trim().to_string())).ok_or_else(|| fail("bad_args", format!("--stats {p:?}: write FIELD=STAT (a view keeps one 统计 per field)")))).collect::<R<_>>()?,
        };
        for (n, s) in stats {
            let f = field_of(t, &n)?;
            if matches!(loose(&s).as_str(), "" | "none") { v.opts.stats.remove(&f.id); continue; }
            let st = stat_named(&s).ok_or_else(|| fail("bad_args", format!("stat {s:?}: use one of {}", query::stats_for(f.ty).iter().map(|s| key_of(*s)).collect::<Vec<_>>().join(", "))))?;
            if !query::stats_for(f.ty).contains(&st) { return Err(fail("bad_args", format!("{} is not a 统计 of {} ({}); it takes: {}", key_of(st), f.name, f.ty.key(), query::stats_for(f.ty).iter().map(|s| key_of(*s)).collect::<Vec<_>>().join(", ")))); }
            v.opts.stats.insert(f.id, st);
        }
        let name = v.name.clone();
        let changed = v != before;
        if changed { self.tables[k].doc.set_view(tid, v); self.tables[k].log.push(format!("~ view {name}")); }
        let shown = self.table_views(a)?["views"].as_array().and_then(|l| l.iter().find(|x| x["name"] == json!(name)).cloned()).unwrap_or_default();
        Ok(json!({ "src": self.tables[k].src, "view": shown, "changed": changed }))
    }

    /// Another table in the file: blank as the app makes one, or with the
    /// columns `fields` names, or from `csv` (its first row names them).
    pub(crate) fn table_new(&mut self, a: &Value) -> R<Value> {
        let (k, _) = self.open_table(&json!({ "doc": a["doc"], "at": a.get("at"), "rev": a.get("rev") }))?;
        let base = &self.tables[k].doc.base;
        let n = need(a, "name")?;
        let name = free("table", n, base.tables.iter().any(|t| t.name.eq_ignore_ascii_case(n.trim())))?;
        let csv_text = match (arg(a, "csv"), names_arg(a, "fields")) {
            (Some(c), _) => Some(c.to_string()),
            (None, f) if !f.is_empty() => Some(csv::write(&[f], ',')),
            _ => None,
        };
        let doc = &mut self.tables[k].doc;
        let t = match csv_text { Some(c) => csv::import(&mut doc.base, &name, &c), None => doc.base.blank_table(&name, field::now()) };
        let (rows, cols) = (t.records.len(), t.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
        let at = doc.base.tables.len();
        doc.run(vec![BaseOp::InsertTable { at, table: Box::new(t) }]);
        self.tables[k].log.push(format!("+ table {name} ({} field(s), {rows} row(s))", cols.len()));
        Ok(json!({ "src": self.tables[k].src, "added": name, "fields": cols, "rows": rows }))
    }

    pub(crate) fn table_rename(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let base = &self.tables[k].doc.base;
        let old = base.table(tid).unwrap().name.clone();
        let n = need(a, "name")?;
        let name = free("table", n, base.tables.iter().any(|t| t.id != tid && t.name.eq_ignore_ascii_case(n.trim())))?;
        self.tables[k].doc.rename_table(tid, &name);
        self.tables[k].log.push(format!("~ table {old} → {name}"));
        Ok(json!({ "src": self.tables[k].src, "table": old, "renamed": name }))
    }

    /// A table out of the file; not the last one, and not one other tables
    /// link to unless `force` (their links then turn to the text they showed).
    pub(crate) fn table_remove(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let base = &self.tables[k].doc.base;
        let t = base.table(tid).unwrap();
        if base.tables.len() <= 1 { return Err(fail("bad_args", format!("{} is the only table in {}: a file keeps one", t.name, self.tables[k].src)).hint("`quiet rm DOC` moves the whole file to the trash")); }
        let links: Vec<String> = base.tables.iter().filter(|o| o.id != tid).flat_map(|o| o.fields.iter().filter(|f| f.ty == FieldType::Link && f.opts.link_table == Some(tid)).map(move |f| format!("{}.{}", o.name, f.name))).collect();
        if !links.is_empty() && !flag(a, "force") {
            return Err(fail("linked", format!("{} is linked from {}", t.name, links.join(", "))).hint("pass --force to remove it anyway: those link cells turn into the text they showed"));
        }
        let name = t.name.clone();
        self.tables[k].doc.remove_table(tid);
        self.tables[k].log.push(format!("- table {name}{}", if links.is_empty() { String::new() } else { format!(" (links from {} turned to text)", links.join(", ")) }));
        Ok(json!({ "src": self.tables[k].src, "removed": name, "unlinked": links }))
    }

    /// Rows appended by their column names: `csv` (header first) or `rows`
    /// ([{field: value}]). A column no field has is an error, or with
    /// `create_fields` a new field of the type its values suggest.
    pub(crate) fn table_import(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let (header, rows): (Vec<String>, Vec<Vec<Value>>) = match (arg(a, "csv"), a.get("rows")) {
            (Some(text), _) => {
                let mut grid = csv::parse(text, separator(text));
                grid.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
                if grid.is_empty() { return Err(fail("bad_args", "the CSV is empty: its first row names the columns")); }
                let header = grid.remove(0).into_iter().map(|h| h.trim().to_string()).collect();
                (header, grid.into_iter().map(|r| r.into_iter().map(Value::String).collect()).collect())
            }
            (None, Some(_)) => {
                let objs = Self::row_objects(a)?;
                let mut header: Vec<String> = vec![];
                for o in &objs { for key in o.keys() { if key != "_id" && !header.contains(key) { header.push(key.clone()); } } }
                (header.clone(), objs.iter().map(|o| header.iter().map(|h| o.get(h).cloned().unwrap_or(Value::Null)).collect()).collect())
            }
            _ => return Err(fail("bad_args", "import takes csv (text, header first) or rows ([{field: value}])")),
        };
        // `_id` (as `quiet table --as csv` has none, but a JSON read does) is not a column to write
        let cols: Vec<usize> = (0..header.len()).filter(|i| header[*i] != "_id" && !header[*i].is_empty()).collect();
        let t = self.tab(k, tid);
        let missing: Vec<&str> = cols.iter().map(|i| header[*i].as_str()).filter(|h| t.field_named(h).is_none()).collect();
        let mut made = vec![];
        if !missing.is_empty() {
            if !flag(a, "create_fields") {
                return Err(fail("not_found", format!("{} has no field {}", t.name, missing.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join(", "))).hint(format!("pass --create-fields to add them (types guessed from the values); fields: {}", field_names(t))));
            }
            for h in missing.iter().map(|s| s.to_string()).collect::<Vec<_>>() {
                let i = header.iter().position(|x| *x == h).unwrap();
                let texts: Vec<String> = rows.iter().map(|r| r.get(i).map(cell_text).unwrap_or_default()).collect();
                free("field", &h, false)?;
                let ty = field::infer_type(texts.iter().map(String::as_str));
                let mut opts = field::default_opts(ty);
                if ty == FieldType::Date { opts.time = texts.iter().filter_map(|s| field::parse_date(s)).any(|ms| ms.rem_euclid(86_400_000) != 0); }
                self.tables[k].doc.add_field(tid, None, &h, ty, opts);
                made.push(json!({ "name": h, "type": ty.key() }));
            }
        }
        let t = self.tab(k, tid);
        let fields: Vec<Field> = cols.iter().map(|i| t.field_named(&header[*i]).unwrap().clone()).collect();
        if let Some(f) = fields.iter().find(|f| f.ty.computed() || f.ty == FieldType::Note) { return Err(fail("read_only", format!("{:?} is a {} field: drop that column", f.name, f.ty.key()))); }
        let mut lines = vec![];
        for r in &rows {
            let line = cols.iter().zip(&fields).map(|(i, f)| self.write_text(k, f, r.get(*i).unwrap_or(&Value::Null))).collect::<R<Vec<_>>>()?;
            lines.push(line);
        }
        let fids: Vec<FieldId> = fields.iter().map(|f| f.id).collect();
        let added = self.append_rows(k, tid, &fids, lines);
        let tname = self.tab(k, tid).name.clone();
        let t = &mut self.tables[k];
        t.log.push(format!("+ {} row(s) imported into {tname}{}", added.len(), if made.is_empty() { String::new() } else { format!(", new field(s): {}", made.iter().map(|m| m["name"].as_str().unwrap_or("")).collect::<Vec<_>>().join(", ")) }));
        Ok(json!({ "src": t.src, "table": tname, "added": added.iter().map(|id| format!("r{id}")).collect::<Vec<_>>(), "created_fields": made }))
    }
}
