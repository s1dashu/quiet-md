//! 数据表 from the CLI: a table file, or the 数据表 block a document shows.
//! Rows have ids that last (`r12`); values are text, read as the app shows
//! them and written as if pasted (a number column parses numbers, a select
//! gains the options it lacks, a 关联 cell takes the other table's titles).
//! Reading goes through the engine's own query (`query::run`): a view's
//! 筛选 / 排序 / 分组, with conditions given here laid over it. Edits are held
//! until commit and written in one SQLite transaction; a file changed
//! meanwhile stops the commit. Fields, views and tables: `table_schema.rs`.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;

use fdl_core::model::BlockKind;
use fdl_core::table::query::{self, Row};
use fdl_core::table::{Base, Cond, DateRef, Field, FieldId, FieldType, Filter, FilterOp, RecId, SortBy, Stat, Table, TableId, TableRef, Value as Cell, View, ViewKind, csv, field, ops, sqlite};
use serde_json::{Value, json};

use crate::blocks;
use crate::fail::{R, fail};
use crate::session::{Session, arg, flag, num};

pub(crate) struct OpenTable {
    pub(crate) src: String,
    path: PathBuf,
    stamp: Option<u64>,
    pub(crate) doc: ops::Doc,
    pub(crate) log: Vec<String>,
}

impl OpenTable {
    pub(crate) fn dirty(&self) -> bool { !self.doc.dirty.is_clean() }
}

pub(crate) fn row_id(s: &str) -> R<RecId> {
    s.trim().trim_start_matches(['r', '#']).parse::<RecId>().map_err(|_| fail("bad_args", format!("row {s:?}: rows are r1, r2… as `quiet table` lists them")))
}

/// A cell as the text a paste would hold: lists joined with commas.
pub(crate) fn cell_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Array(l) => l.iter().map(cell_text).collect::<Vec<_>>().join(", "),
        x => x.to_string(),
    }
}

/// `a[k]` as a list: an array as it is, anything else as one item.
pub(crate) fn items(a: &Value, k: &str) -> Vec<Value> {
    match a.get(k) { None | Some(Value::Null) => vec![], Some(Value::Array(l)) => l.clone(), Some(x) => vec![x.clone()] }
}

/// `a[k]` as names: an array, or one string of them joined with commas.
pub(crate) fn names_arg(a: &Value, k: &str) -> Vec<String> {
    match a.get(k) {
        Some(Value::Array(l)) => l.iter().map(cell_text).collect(),
        Some(Value::String(s)) => s.split([',', '，']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect(),
        _ => vec![],
    }
}

/// An enum's key as the file and `--json` spell it (`single_select`, `past7`).
pub(crate) fn key_of<T: serde::Serialize>(x: T) -> String { serde_json::to_value(x).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default() }

/// For matching what is typed: no case, spaces, `_` or `-`.
pub(crate) fn loose(s: &str) -> String { s.trim().to_lowercase().replace([' ', '_', '-'], "") }

pub(crate) fn field_names(t: &Table) -> String { t.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ") }

pub(crate) fn field_of<'a>(t: &'a Table, name: &str) -> R<&'a Field> {
    t.field_named(name.trim()).ok_or_else(|| fail("not_found", format!("no field {name:?} in {}", t.name)).hint(format!("fields: {}", field_names(t))))
}

pub(crate) fn view_of<'a>(t: &'a Table, name: &str) -> R<&'a View> {
    t.views.iter().find(|v| v.name == name.trim()).ok_or_else(|| fail("not_found", format!("no view {name:?} in {}", t.name)).hint(format!("views: {}", t.views.iter().map(|v| v.name.as_str()).collect::<Vec<_>>().join(", "))))
}

/// A field type by its key (`single_select`) or the app's name for it (`单选`).
pub(crate) fn field_type(s: &str) -> R<FieldType> {
    FieldType::ALL.iter().copied().find(|t| loose(t.key()) == loose(s) || loose(t.label()) == loose(s))
        .ok_or_else(|| fail("bad_args", format!("type {s:?}: use one of {}", FieldType::ALL.iter().map(|t| format!("{} ({})", t.key(), t.label())).collect::<Vec<_>>().join(", "))))
}

pub(crate) fn view_kind(s: &str) -> R<ViewKind> {
    let alias = match loose(s).as_str() { "table" => "grid", "board" => "kanban", x => return ViewKind::ALL.iter().copied().find(|k| loose(&key_of(*k)) == x || loose(k.label()) == x || loose(k.short()) == x).ok_or_else(|| fail("bad_args", format!("view kind {s:?}: use grid, kanban, calendar, gantt or form (表格, 看板, 日历, 甘特图, 表单)"))) };
    view_kind(alias)
}

/// 筛选 operators as typed: the first name of each is how conditions are written back.
const OPS: &[(FilterOp, &[&str])] = &[
    (FilterOp::Is, &["=", "==", "is", "eq", "等于"]),
    (FilterOp::IsNot, &["!=", "<>", "is_not", "isnot", "ne", "不等于"]),
    (FilterOp::Contains, &["~", "contains", "has", "包含"]),
    (FilterOp::NotContains, &["!~", "not_contains", "notcontains", "不包含"]),
    (FilterOp::Gt, &[">", "gt", "大于"]),
    (FilterOp::Ge, &[">=", "≥", "ge", "大于等于"]),
    (FilterOp::Lt, &["<", "lt", "小于"]),
    (FilterOp::Le, &["<=", "≤", "le", "小于等于"]),
    (FilterOp::Empty, &["empty", "is_empty", "为空"]),
    (FilterOp::NotEmpty, &["notempty", "not_empty", "!empty", "不为空"]),
    (FilterOp::Before, &["before", "早于"]),
    (FilterOp::After, &["after", "晚于"]),
];

pub(crate) fn op_text(op: FilterOp) -> &'static str { OPS.iter().find(|(o, _)| *o == op).map_or("?", |(_, n)| n[0]) }

fn op_named(s: &str) -> Option<FilterOp> { OPS.iter().find(|(_, names)| names.iter().any(|n| n.eq_ignore_ascii_case(s.trim()))).map(|(o, _)| *o) }

const DATE_HINT: &str = "a day (2026-11-26) or one relative to today: today, tomorrow, yesterday, this_week, last_week, next_week, this_month, last_month, next_month, past_7, next_7, past_30, next_30 (or 今天, 本周, 过去 7 天内 …)";

/// `past7` as typed: `past_7`.
pub(crate) fn ref_key(d: DateRef) -> String {
    let k = key_of(d);
    match k.find(|c: char| c.is_ascii_digit()) { Some(i) => format!("{}_{}", &k[..i], &k[i..]), None => k }
}

fn date_ref(s: &str) -> Option<DateRef> { DateRef::ALL.iter().copied().filter(|d| *d != DateRef::Exact).find(|d| loose(&ref_key(*d)) == loose(s) || loose(d.label()) == loose(s)) }

pub(crate) fn stat_named(s: &str) -> Option<Stat> {
    let s = match loose(s).as_str() { "avg" | "mean" => "average".to_string(), x => x.to_string() };
    use Stat::*;
    [Count, Filled, Empty, Unique, PercentFilled, PercentEmpty, Sum, Average, Min, Max, Earliest, Latest, Checked, Unchecked].into_iter().find(|x| loose(&key_of(*x)) == s || loose(x.label()) == s)
}

/// `FIELD OP VALUE` as typed: the first operator after the field's name (the
/// longest there), a word operator only between spaces. The name may be
/// quoted or braced when it holds one: `{评 分} > 3`.
fn split_cond(s: &str) -> Option<(String, FilterOp, String)> {
    let s = s.trim();
    let (lead, from) = match s.chars().next() {
        Some(q @ ('"' | '\'' | '{' | '[')) => { let close = match q { '{' => '}', '[' => ']', c => c }; let end = s[1..].find(close)? + 1; (Some(s[1..end].to_string()), end + 1) }
        _ => (None, 1),
    };
    for (i, _) in s.char_indices().filter(|(i, _)| *i >= from) {
        let here = &s[i..];
        let mut best: Option<(usize, FilterOp)> = None;
        for (op, names) in OPS {
            for n in names.iter() {
                let word = n.chars().any(|c| c.is_ascii_alphabetic());
                let hit = if word {
                    s[..i].ends_with(char::is_whitespace) && here.len() >= n.len() && here.is_char_boundary(n.len()) && here[..n.len()].eq_ignore_ascii_case(n) && here[n.len()..].chars().next().is_none_or(char::is_whitespace)
                } else { here.starts_with(n) };
                if hit && best.is_none_or(|(l, _)| n.len() > l) { best = Some((n.len(), *op)); }
            }
        }
        if let Some((l, op)) = best {
            let name = lead.unwrap_or_else(|| s[..i].trim().to_string());
            if name.is_empty() { return None; }
            let v = s[i + l..].trim();
            let v = [('"', '"'), ('\'', '\'')].iter().find_map(|(a, b)| v.strip_prefix(*a).and_then(|x| x.strip_suffix(*b))).unwrap_or(v);
            return Some((name, op, v.to_string()));
        }
    }
    None
}

fn bool_word(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() { "true" | "yes" | "y" | "1" | "checked" | "✓" | "是" | "已勾选" => Some(true), "false" | "no" | "n" | "0" | "unchecked" | "否" | "未勾选" => Some(false), _ => None }
}

/// One 筛选 condition, checked against the field it names: the operator
/// applies to its type, the value reads as one of it.
pub(crate) fn make_cond(t: &Table, name: &str, op: FilterOp, value: &str) -> R<Cond> {
    let f = field_of(t, name)?;
    let date = f.ty.is_date();
    // `<` / `>` on a date: before / after
    let op = match op { FilterOp::Lt if date => FilterOp::Before, FilterOp::Gt if date => FilterOp::After, o => o };
    let ok = query::ops_for(f.ty);
    if !ok.contains(&op) {
        return Err(fail("bad_args", format!("`{}` does not apply to {} ({}); it takes: {}", op_text(op), f.name, f.ty.key(), ok.iter().map(|o| op_text(*o)).collect::<Vec<_>>().join(" "))));
    }
    let mut c = Cond { field: f.id, op, value: String::new(), date: DateRef::Exact };
    if !op.needs_value() { return Ok(c); }
    let v = value.trim();
    if v.is_empty() { return Err(fail("bad_args", format!("`{} {}` needs a value", f.name, op_text(op)))); }
    if date {
        if let Some(d) = date_ref(v) { c.date = d; return Ok(c); }
        let ms = field::parse_date(v).ok_or_else(|| fail("bad_args", format!("{v:?} is not a date")).hint(DATE_HINT))?;
        c.value = field::iso(ms, false);
        return Ok(c);
    }
    if f.ty == FieldType::Checkbox { c.value = bool_word(v).ok_or_else(|| fail("bad_args", format!("{} is a checkbox: compare it with true or false", f.name)))?.to_string(); return Ok(c); }
    if f.ty.numeric() && field::parse_number(v).is_none() { return Err(fail("bad_args", format!("{} is a {} field and {v:?} is not a number", f.name, f.ty.key()))); }
    if f.ty.select() && let Some(x) = field::split_list(v).into_iter().find(|n| f.option_named(n).is_none()) {
        return Err(fail("bad_args", format!("{} has no option {x:?}", f.name)).hint(format!("options: {}", f.opts.options.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join(", "))));
    }
    c.value = v.to_string();
    Ok(c)
}

/// A condition from a command line (`状态 = 想读`) or a script (`{field, op, value}`).
pub(crate) fn cond_arg(t: &Table, w: &Value) -> R<Cond> {
    match w {
        Value::String(s) => {
            let (name, op, v) = split_cond(s).ok_or_else(|| fail("bad_args", format!("where {s:?}: write FIELD OP VALUE, e.g. '状态 = 想读'")).hint("operators: = != ~ (contains) !~ > >= < <= empty notempty before after, or 等于 不等于 包含 不包含 为空 不为空 早于 晚于"))?;
            make_cond(t, &name, op, &v)
        }
        Value::Object(o) => {
            let name = o.get("field").and_then(Value::as_str).ok_or_else(|| fail("bad_args", "a condition object needs a field"))?;
            let op = match o.get("op").and_then(Value::as_str) { Some(x) => op_named(x).ok_or_else(|| fail("bad_args", format!("operator {x:?}: use = != ~ !~ > >= < <= empty notempty before after")))?, None => FilterOp::Is };
            make_cond(t, name, op, &o.get("value").map(cell_text).unwrap_or_default())
        }
        x => Err(fail("bad_args", format!("a condition is 'FIELD OP VALUE' or {{field, op, value}}, not {x}"))),
    }
}

/// How a condition reads: `状态 = 想读`, `截止 = this_week`.
pub(crate) fn cond_text(t: &Table, c: &Cond) -> String {
    let name = t.field(c.field).map_or("?", |f| f.name.as_str());
    let v = if c.date != DateRef::Exact { ref_key(c.date) } else { c.value.clone() };
    if c.op.needs_value() { format!("{name} {} {v}", op_text(c.op)) } else { format!("{name} {}", op_text(c.op)) }
}

/// A 筛选 as it reads: `状态 = 在读 or 评分 >= 9.5`; `pre` (a view's own, applied first) in brackets before.
pub(crate) fn filter_text(t: &Table, pre: Option<&Filter>, f: &Filter) -> String {
    let one = |f: &Filter| f.conds.iter().map(|c| cond_text(t, c)).collect::<Vec<_>>().join(if f.all { " and " } else { " or " });
    match pre { Some(p) => format!("({}) and {}{}{}", one(p), if f.conds.len() > 1 { "(" } else { "" }, one(f), if f.conds.len() > 1 { ")" } else { "" }), None => one(f) }
}

/// `评分`, `评分:desc`, or `{field, desc}`.
pub(crate) fn sort_arg(t: &Table, v: &Value) -> R<SortBy> {
    let (name, desc) = match v {
        Value::Object(o) => (o.get("field").and_then(Value::as_str).unwrap_or("").to_string(), o.get("desc").and_then(Value::as_bool).unwrap_or(false)),
        Value::String(s) if t.field_named(s.trim()).is_some() => (s.trim().to_string(), false),
        Value::String(s) => match s.rsplit_once(':') {
            Some((n, d)) if matches!(d.trim(), "desc" | "asc" | "降序" | "升序") => (n.trim().to_string(), matches!(d.trim(), "desc" | "降序")),
            _ => (s.trim().to_string(), false),
        },
        x => return Err(fail("bad_args", format!("sort by FIELD or FIELD:desc, not {x}"))),
    };
    Ok(SortBy { field: field_of(t, &name)?.id, desc })
}

pub(crate) fn sort_text(t: &Table, s: &SortBy) -> String { format!("{}{}", t.field(s.field).map_or("?", |f| f.name.as_str()), if s.desc { ":desc" } else { "" }) }

/// What `quiet table` shows: a view (or the whole table) with the
/// conditions, sorts and groups given laid over it. `pre`: a view's own 筛选
/// that can't be joined with the one given (or with and), applied first.
pub(crate) struct Query {
    pub(crate) view: View,
    pre: Option<Filter>,
    pub(crate) named: Option<String>,
}

pub(crate) fn query_of(t: &Table, a: &Value) -> R<Query> {
    let picked = match arg(a, "view") { Some(n) => Some(view_of(t, n)?.clone()), None => None };
    let named = picked.as_ref().map(|v| v.name.clone());
    let mut v = picked.unwrap_or_else(|| View { kind: ViewKind::Grid, ..Default::default() });
    // folding is how the app shows a group, not which rows there are
    v.opts.folded.clear();
    let conds = items(a, "where").iter().map(|w| cond_arg(t, w)).collect::<R<Vec<_>>>()?;
    let any = flag(a, "any");
    if any && conds.is_empty() { return Err(fail("bad_args", "--any joins --where conditions with or: give some")); }
    let mut pre = None;
    if !conds.is_empty() {
        let mine = std::mem::replace(&mut v.opts.filter, Filter { all: !any, conds });
        if !mine.conds.is_empty() {
            if mine.all && !any { v.opts.filter.conds.splice(0..0, mine.conds); } else { pre = Some(mine); }
        }
    }
    // given ones first, then the view's own on other fields
    let lay = |given: Vec<SortBy>, own: &[SortBy]| -> Vec<SortBy> { let mut out = given; for s in own { if !out.iter().any(|x| x.field == s.field) { out.push(*s); } } out };
    let sorts = items(a, "sort").iter().map(|s| sort_arg(t, s)).collect::<R<Vec<_>>>()?;
    v.opts.sorts = lay(sorts, &v.opts.sorts);
    let groups = items(a, "group").iter().map(|s| sort_arg(t, s)).collect::<R<Vec<_>>>()?;
    if groups.len() > 3 { return Err(fail("bad_args", "at most 3 group levels")); }
    v.opts.groups = lay(groups, &v.opts.groups);
    v.opts.groups.truncate(3);
    let only = names_arg(a, "fields");
    if !only.is_empty() {
        let ids = only.iter().map(|n| field_of(t, n).map(|f| f.id)).collect::<R<Vec<FieldId>>>()?;
        v.opts.hidden = t.fields.iter().map(|f| f.id).filter(|id| !ids.contains(id)).collect();
        v.opts.order = ids;
    }
    Ok(Query { view: v, pre, named })
}

/// The records `q` shows (indexes into the table returned: a copy holding
/// only what `pre` lets through, when there is one) and the grid's rows.
pub(crate) fn run<'a>(base: &Base, t: &'a Table, q: &Query) -> (Cow<'a, Table>, query::ViewResult) {
    let t = match &q.pre {
        Some(f) => { let today = field::today(); let mut c = t.clone(); c.records.retain(|r| query::passes(base, t, r, f, today)); Cow::Owned(c) }
        None => Cow::Borrowed(t),
    };
    let r = query::run(base, &t, &q.view);
    (t, r)
}

/// Each field as an agent needs it to read and write the table.
pub(crate) fn schema(base: &Base, t: &Table, view: Option<&View>) -> Vec<Value> {
    t.fields.iter().enumerate().map(|(i, f)| {
        let o = &f.opts;
        let mut v = json!({ "name": f.name, "type": f.ty.key(), "label": f.ty.label() });
        if i == 0 { v["primary"] = json!(true); }
        if f.ty.computed() { v["computed"] = json!(true); }
        if view.is_some_and(|w| w.opts.hidden.contains(&f.id)) { v["hidden"] = json!(true); }
        if !f.desc.is_empty() { v["description"] = json!(f.desc); }
        if f.ty.select() { v["options"] = json!(o.options.iter().map(|o| o.name.as_str()).collect::<Vec<_>>()); }
        if f.ty == FieldType::Formula { v["formula"] = json!(o.formula); }
        if f.ty == FieldType::Link {
            let other = o.link_table.and_then(|id| base.table(id));
            v["link"] = json!(other.map(|x| x.name.as_str()));
            if o.single { v["single"] = json!(true); }
            if let Some(b) = o.back.and_then(|b| other.and_then(|x| x.field(b))) { v["mirror"] = json!(b.name); }
        }
        if f.ty == FieldType::Lookup {
            let via = o.via.and_then(|id| t.field(id));
            let target = via.and_then(|v| v.opts.link_table).and_then(|id| base.table(id)).zip(o.target).and_then(|(x, id)| x.field(id));
            v["lookup"] = json!(format!("{}.{}", via.map_or("?", |f| f.name.as_str()), target.map_or("?", |f| f.name.as_str())));
            v["rollup"] = json!(key_of(o.rollup));
        }
        let fmt = format_of(f);
        if !fmt.is_empty() { v["format"] = Value::Object(fmt); }
        if !o.default.is_empty() { v["default"] = json!(o.default); }
        if o.unique { v["unique"] = json!(true); }
        v
    }).collect()
}

/// A field's display settings that differ from plain.
fn format_of(f: &Field) -> serde_json::Map<String, Value> {
    let (o, mut m) = (&f.opts, serde_json::Map::new());
    if f.ty.numeric() || f.ty == FieldType::Formula {
        if let Some(p) = o.precision { m.insert("precision".into(), json!(p)); }
        if o.style != Default::default() { m.insert("style".into(), json!(key_of(o.style))); }
    }
    if f.ty == FieldType::Currency { m.insert("currency".into(), json!(if o.currency.is_empty() { "¥" } else { &o.currency })); }
    if f.ty == FieldType::Rating { m.insert("stars".into(), json!(f.stars())); }
    if f.ty == FieldType::AutoNumber && !o.prefix.is_empty() { m.insert("prefix".into(), json!(o.prefix)); }
    if f.ty.is_date() {
        if o.date != Default::default() { m.insert("date".into(), json!(key_of(o.date))); }
        if o.time { m.insert("time".into(), json!(true)); }
    }
    m
}

/// The 统计 `--stats` gives a field with none asked for: numbers' sum,
/// average, min, max; dates' earliest and latest; checkboxes' counts; else
/// how many are filled and how many differ.
fn default_stats(t: &Table, f: &Field, records: &[usize]) -> Vec<Stat> {
    let numbers = f.ty.numeric() || (matches!(f.ty, FieldType::Formula | FieldType::Lookup) && records.iter().any(|i| matches!(t.records[*i].get(f.id), Cell::Number(_))));
    if f.ty == FieldType::Checkbox { vec![Stat::Checked, Stat::Unchecked] } else if f.ty.is_date() { vec![Stat::Earliest, Stat::Latest] } else if numbers { vec![Stat::Sum, Stat::Average, Stat::Min, Stat::Max] } else { vec![Stat::Filled, Stat::Unique] }
}

/// The 统计 in the order the footer menu has them (JSON objects sort their keys).
pub(crate) const STATS: &[&str] = &["count", "filled", "empty", "unique", "percent_filled", "percent_empty", "sum", "average", "min", "max", "earliest", "latest", "checked", "unchecked"];

/// `stats: true` (each field's default, or the view's own 统计), or the ones named.
fn stats_of(base: &Base, t: &Table, fields: &[&Field], view: &View, records: &[usize], want: &Value) -> R<Value> {
    let asked: Option<Vec<Stat>> = match want {
        Value::Bool(_) => None,
        _ => Some(names_arg(&json!({ "s": want }), "s").iter().map(|n| stat_named(n).ok_or_else(|| fail("bad_args", format!("stat {n:?}: use count, filled, empty, unique, percent_filled, percent_empty, sum, average, min, max, earliest, latest, checked, unchecked")))).collect::<R<_>>()?),
    };
    let mut out = vec![];
    for f in fields {
        let list = match &asked {
            Some(l) => l.iter().copied().filter(|s| query::stats_for(f.ty).contains(s)).collect(),
            None => view.opts.stats.get(&f.id).map(|s| vec![*s]).unwrap_or_else(|| default_stats(t, f, records)),
        };
        if list.is_empty() { continue; }
        let mut o = json!({ "field": f.name });
        for s in list { o[key_of(s)] = json!(query::stat(base, t, f, records, s)); }
        out.push(o);
    }
    Ok(json!(out))
}

/// The groups of `rows` from `level` down, each with its records that `row` keeps.
fn group_tree(t: &Table, rows: &[Row], at: &mut usize, level: usize, row: &dyn Fn(usize) -> Option<Value>) -> (Vec<Value>, Vec<Value>) {
    let (mut groups, mut recs) = (vec![], vec![]);
    while *at < rows.len() {
        match &rows[*at] {
            Row::Record(i) => { recs.extend(row(*i)); *at += 1; }
            Row::Group(g) if g.level < level => break,
            Row::Group(g) => {
                *at += 1;
                let (gs, rs) = group_tree(t, rows, at, level + 1, row);
                if gs.is_empty() && rs.is_empty() { continue; }
                let mut o = json!({ "field": t.field(g.field).map(|f| f.name.as_str()), "value": g.label, "key": g.key, "count": g.count });
                if gs.is_empty() { o["rows"] = json!(rs); } else { o["groups"] = json!(gs); }
                groups.push(o);
            }
        }
    }
    (groups, recs)
}

impl Session {
    /// The 数据表 a document is or shows: `at` a datatable block, else its first one.
    fn table_ref(&mut self, a: &Value) -> R<TableRef> {
        let i = self.doc_arg(a)?;
        let d = &self.docs[i];
        Ok(match arg(a, "at") {
            Some(x) => { let id = blocks::resolve(&d.doc, x)?; match &blocks::find(&d.doc, id).unwrap().kind { BlockKind::DataTable(t) => t.clone(), k => return Err(fail("bad_kind", format!("{x} is a {} block, not a 数据表", blocks::kind_name(k)))) } }
            None => match d.doc.blocks.iter().find_map(|b| if let BlockKind::DataTable(t) = &b.kind { Some(t.clone()) } else { None }) {
                Some(t) => t,
                None => return Err(fail("bad_kind", format!("{} holds no 数据表", d.path)).hint("pass --at <ref> of a datatable block")),
            },
        })
    }

    /// The table file, read once per run; and which of its tables `a` names.
    pub(crate) fn open_table(&mut self, a: &Value) -> R<(usize, TableId)> {
        let tref = self.table_ref(a)?;
        // Relative to the top-level folder the document is in.
        let i = self.doc_arg(a)?;
        let path = self.top_dir(&self.docs[i].top).join(&tref.src);
        let k = match self.tables.iter().position(|t| t.path == path) {
            Some(k) => k,
            None => {
                let stamp = sqlite::stamp(&path);
                let base = sqlite::load(&path).map_err(|e| fail("io", format!("{}: {e:#}", tref.src)))?;
                self.tables.push(OpenTable { src: tref.src.clone(), path, stamp, doc: ops::Doc::new(base), log: vec![] });
                self.tables.len() - 1
            }
        };
        let base = &self.tables[k].doc.base;
        let name = arg(a, "table").map(str::to_string).or(tref.table.clone());
        let t = match &name {
            Some(n) => base.table_named(n).ok_or_else(|| fail("not_found", format!("no table {n:?}; tables: {}", names(base))).hint("`quiet table DOC` lists the tables; pick one with --table NAME"))?,
            None => base.tables.first().ok_or_else(|| fail("not_found", "the file holds no tables"))?,
        };
        Ok((k, t.id))
    }

    pub(crate) fn tab(&self, k: usize, tid: TableId) -> &Table { self.tables[k].doc.base.table(tid).unwrap() }

    /// Rows as text by field name, with their ids: a view's or the whole
    /// table's, filtered, sorted and grouped as asked; `limit` / `offset`
    /// page through; `stats` adds 统计; `as: csv|md` gives the grid as text.
    pub(crate) fn table(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let base = &self.tables[k].doc.base;
        let t0 = base.table(tid).unwrap();
        let q = query_of(t0, a)?;
        let (t, res) = run(base, t0, &q);
        let total = res.records.len();
        let (offset, limit) = (num(a, "offset").unwrap_or(0), num(a, "limit").unwrap_or(usize::MAX));
        let paged: Vec<usize> = res.records.iter().copied().skip(offset).take(limit).collect();
        let shown = t.visible_fields(&q.view);
        let mut v = json!({
            "src": self.tables[k].src, "table": t.name, "tables": base.tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            "views": t.views.iter().map(|w| json!({ "name": w.name, "kind": key_of(w.kind) })).collect::<Vec<_>>(),
            "view": q.named, "total": total,
        });
        if q.named.is_some() || !items(a, "where").is_empty() { v["records"] = json!(t0.records.len()); }
        if !q.view.opts.filter.conds.is_empty() { v["where"] = json!(filter_text(&t, q.pre.as_ref(), &q.view.opts.filter)); }
        if let Some(fmt) = arg(a, "as").filter(|f| *f != "json") {
            v["as"] = json!(fmt);
            v["text"] = json!(match fmt {
                "csv" => csv::write(&csv::export(base, &t, &q.view, &paged), ','),
                "md" | "markdown" => csv::to_markdown(base, &t, &q.view, &paged),
                x => return Err(fail("bad_args", format!("--as {x}: use json, csv or md"))),
            });
            if let Some(want) = a.get("stats").filter(|w| !matches!(w, Value::Null | Value::Bool(false))) { v["stats"] = stats_of(base, &t, &shown, &q.view, &res.records, want)?; }
            return Ok(v);
        }
        let row = |i: usize| -> Value {
            let r = &t.records[i];
            let mut o = serde_json::Map::new();
            o.insert("_id".into(), json!(format!("r{}", r.id)));
            for f in &shown { o.insert(f.name.clone(), json!(field::display_in(base, f, r.get(f.id)))); }
            Value::Object(o)
        };
        v["fields"] = json!(shown.iter().map(|f| f.name.as_str()).collect::<Vec<_>>());
        v["schema"] = json!(schema(base, &t, q.named.as_ref().map(|_| &q.view)));
        if q.view.opts.groups.is_empty() {
            v["rows"] = json!(paged.iter().map(|i| row(*i)).collect::<Vec<_>>());
        } else {
            let keep: HashSet<usize> = paged.iter().copied().collect();
            let (groups, loose_rows) = group_tree(&t, &res.rows, &mut 0, 0, &|i| keep.contains(&i).then(|| row(i)));
            v["group_by"] = json!(q.view.opts.groups.iter().map(|g| sort_text(&t, g)).collect::<Vec<_>>());
            v["groups"] = json!(groups);
            if !loose_rows.is_empty() { v["rows"] = json!(loose_rows); }
        }
        if offset + limit < total { v["more"] = json!(total - offset - limit); }
        if let Some(want) = a.get("stats").filter(|w| !matches!(w, Value::Null | Value::Bool(false))) { v["stats"] = stats_of(base, &t, &shown, &q.view, &res.records, want)?; }
        Ok(v)
    }

    /// The records `where` (with `view`, `any`) picks. No condition is an
    /// error, never every row.
    fn matching(&self, k: usize, tid: TableId, a: &Value) -> R<Vec<RecId>> {
        if items(a, "where").is_empty() { return Err(fail("bad_args", "where: give at least one condition (FIELD OP VALUE)")); }
        let base = &self.tables[k].doc.base;
        let t = base.table(tid).unwrap();
        let q = query_of(t, a)?;
        let (t, res) = run(base, t, &q);
        Ok(res.records.iter().map(|i| t.records[*i].id).collect())
    }

    /// A cell to write as text a paste takes: a 关联's records by their
    /// titles (the other table's primary field) or ids, made ids.
    pub(crate) fn write_text(&self, k: usize, f: &Field, v: &Value) -> R<String> {
        let text = cell_text(v);
        if f.ty != FieldType::Link || text.trim().is_empty() { return Ok(text); }
        let base = &self.tables[k].doc.base;
        let other = f.opts.link_table.and_then(|id| base.table(id)).ok_or_else(|| fail("not_found", format!("{} links to a table that is gone", f.name)))?;
        let parts = match v { Value::Array(l) => l.iter().map(cell_text).collect(), _ => field::split_list(&text) };
        let ids = parts.iter().map(|p| {
            if let Ok(id) = row_id(p) && other.record(id).is_some() { return Ok(id.to_string()); }
            let hits: Vec<RecId> = other.records.iter().filter(|r| other.title(r).trim() == p.trim()).map(|r| r.id).collect();
            match hits[..] {
                [id] => Ok(id.to_string()),
                [] => Err(fail("not_found", format!("{}: no row {p:?} in {}", f.name, other.name)).hint(format!("a 关联 cell takes rows of {} by their {} or their id (r12)", other.name, other.primary().map_or("title", |f| f.name.as_str())))),
                _ => Err(fail("ambiguous", format!("{}: {} rows of {} are called {p:?}: {}", f.name, hits.len(), other.name, hits.iter().map(|id| format!("r{id}")).collect::<Vec<_>>().join(" "))).hint("write the one you mean by its id")),
            }
        }).collect::<R<Vec<_>>>()?;
        Ok(ids.join(","))
    }

    /// The fields an object of cells names, checked: there, and not computed.
    fn fields_of(&self, k: usize, tid: TableId, cells: &serde_json::Map<String, Value>) -> R<Vec<(FieldId, String)>> {
        let t = self.tab(k, tid);
        cells.iter().filter(|(name, _)| name.as_str() != "_id").map(|(name, v)| {
            let f = field_of(t, name)?;
            if f.ty.computed() { return Err(fail("read_only", format!("{name:?} is a {} field, computed by the table", f.ty.key()))); }
            if f.ty == FieldType::Note { return Err(fail("read_only", format!("{name:?} is a 笔记 field: its notes are made in the app"))); }
            Ok((f.id, self.write_text(k, f, v)?))
        }).collect()
    }

    pub(crate) fn row_objects(a: &Value) -> R<Vec<serde_json::Map<String, Value>>> {
        let rows = match a.get("rows") { Some(Value::Array(l)) => l.clone(), Some(o @ Value::Object(_)) => vec![o.clone()], _ => return Err(fail("bad_args", "rows: a list of {field: value} objects")) };
        rows.into_iter().map(|r| match r { Value::Object(o) => Ok(o), x => Err(fail("bad_args", format!("a row is an object of field: value, not {x}"))) }).collect()
    }

    /// New rows at the end, as pasted: `paste` adds a row per line past the ones given.
    pub(crate) fn append_rows(&mut self, k: usize, tid: TableId, fids: &[FieldId], lines: Vec<Vec<String>>) -> Vec<RecId> {
        let t = &mut self.tables[k];
        let before: HashSet<RecId> = t.doc.base.table(tid).unwrap().records.iter().map(|r| r.id).collect();
        let n = lines.len();
        if fids.is_empty() { t.doc.add_records(tid, None, n, &[]); } else { t.doc.paste(tid, &[], fids, &lines); }
        t.doc.base.table(tid).unwrap().records.iter().map(|r| r.id).filter(|id| !before.contains(id)).collect()
    }

    pub(crate) fn table_add(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let rows = Self::row_objects(a)?;
        let mut made = vec![];
        for r in &rows {
            let cells = self.fields_of(k, tid, r)?;
            let (fids, line): (Vec<FieldId>, Vec<String>) = cells.into_iter().unzip();
            made.extend(self.append_rows(k, tid, &fids, vec![line]));
        }
        let t = &mut self.tables[k];
        t.log.push(format!("+ {} row(s)", made.len()));
        Ok(json!({ "src": t.src, "added": made.iter().map(|id| format!("r{id}")).collect::<Vec<_>>() }))
    }

    /// `r12 三体`: a row as a log line names it.
    fn row_label(&self, k: usize, tid: TableId, id: RecId) -> String {
        let t = self.tab(k, tid);
        match t.record(id).map(|r| t.title(r)).filter(|s| !s.is_empty()) { Some(s) => format!("r{id} {}", blocks::clip(&s, 40)), None => format!("r{id}") }
    }

    pub(crate) fn table_update(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let rows = match (arg(a, "row"), a.get("set")) {
            // `{where, set}`: every row the conditions pick
            (None, Some(Value::Object(set))) if a.get("where").is_some() => {
                self.matching(k, tid, a)?.into_iter().map(|id| { let mut o = set.clone(); o.insert("_id".into(), json!(format!("r{id}"))); o }).collect()
            }
            // `{row, set}` for one row
            (Some(r), Some(Value::Object(set))) => { let mut o = set.clone(); o.insert("_id".into(), json!(r)); vec![o] }
            _ => Self::row_objects(a)?,
        };
        let mut done = vec![];
        let mut log = vec![];
        for r in &rows {
            let id = row_id(r.get("_id").and_then(Value::as_str).ok_or_else(|| fail("bad_args", "each row to update needs its _id (r12)"))?)?;
            if self.tab(k, tid).record(id).is_none() { return Err(fail("not_found", format!("no row r{id}")).hint("`quiet table DOC` lists the rows")); }
            let cells = self.fields_of(k, tid, r)?;
            let (fids, line): (Vec<FieldId>, Vec<String>) = cells.into_iter().unzip();
            if !fids.is_empty() { self.tables[k].doc.paste(tid, &[id], &fids, &[line]); }
            log.push(self.row_label(k, tid, id));
            done.push(format!("r{id}"));
        }
        let t = &mut self.tables[k];
        if !done.is_empty() { t.log.push(format!("~ {}", log.join(", "))); }
        Ok(json!({ "src": t.src, "updated": done }))
    }

    pub(crate) fn table_delete(&mut self, a: &Value) -> R<Value> {
        let (k, tid) = self.open_table(a)?;
        let ids: Vec<RecId> = match a.get("rows") {
            Some(Value::Array(l)) if !l.is_empty() => l.iter().map(|v| row_id(&cell_text(v))).collect::<R<_>>()?,
            Some(v @ Value::String(_)) => vec![row_id(&cell_text(v))?],
            _ if a.get("where").is_some() => self.matching(k, tid, a)?,
            _ => return Err(fail("bad_args", "rows: the ids to delete, or where: conditions picking them")),
        };
        if let Some(x) = ids.iter().find(|id| self.tab(k, tid).record(**id).is_none()) { return Err(fail("not_found", format!("no row r{x}"))); }
        let labels: Vec<String> = ids.iter().map(|id| self.row_label(k, tid, *id)).collect();
        let t = &mut self.tables[k];
        if !ids.is_empty() { t.doc.remove_records(tid, &ids); t.log.push(format!("- {}", labels.join(", "))); }
        Ok(json!({ "src": t.src, "deleted": ids.len(), "rows": labels }))
    }

    pub(crate) fn check_tables(&self) -> R<()> {
        for t in self.tables.iter().filter(|t| t.dirty()) {
            if sqlite::stamp(&t.path) != t.stamp {
                return Err(fail("conflict", format!("{} changed on disk while this ran; nothing was written", t.src)).hint("read it again and redo the edit"));
            }
        }
        Ok(())
    }

    pub(crate) fn describe_tables(&self) -> Vec<Value> {
        self.tables.iter().filter(|t| t.dirty()).map(|t| json!({ "path": t.src, "action": "would save", "diff": t.log })).collect()
    }

    pub(crate) fn save_tables(&mut self) -> R<Vec<Value>> {
        let mut out = vec![];
        for t in self.tables.iter_mut().filter(|t| t.dirty()) {
            sqlite::save(&t.path, &t.doc.base, &t.doc.dirty).map_err(|e| fail("io", format!("{}: {e:#}", t.src)))?;
            t.doc.dirty = Default::default();
            t.stamp = sqlite::stamp(&t.path);
            out.push(json!({ "path": t.src, "action": "saved", "changes": std::mem::take(&mut t.log) }));
        }
        Ok(out)
    }
}

pub(crate) fn names(base: &Base) -> String { base.tables.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ") }
