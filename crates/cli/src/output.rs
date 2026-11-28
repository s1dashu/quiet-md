//! What commands print: compact text for people and agents alike (`--json`
//! has every field), and the commit report.

use serde_json::Value;

fn s(v: &Value, k: &str) -> String { match v.get(k) { Some(Value::String(x)) => x.clone(), Some(Value::Null) | None => String::new(), Some(x) => x.to_string() } }

fn list<'a>(v: &'a Value, k: &str) -> impl Iterator<Item = &'a Value> { v.get(k).and_then(Value::as_array).into_iter().flatten() }

fn refs(v: &Value) -> String { v.as_array().map(|l| l.iter().map(|r| r.as_str().map_or_else(|| s(r, "ref"), str::to_string)).collect::<Vec<_>>().join(" ")).unwrap_or_default() }

fn one_line(t: &str, n: usize) -> String {
    let t = t.replace('\n', " ⏎ ");
    if t.chars().count() > n { format!("{}…", t.chars().take(n).collect::<String>()) } else { t }
}

fn block_line(b: &Value) -> String {
    let kind = match (s(b, "kind").as_str(), b.get("done").and_then(Value::as_bool)) { ("todo", Some(true)) => "todo[x]".to_string(), ("todo", _) => "todo[ ]".to_string(), (k, _) => k.to_string() };
    let indent = "  ".repeat(b.get("depth").and_then(Value::as_u64).unwrap_or(0) as usize);
    format!("{:<12} {indent}{kind:<8} {}\n", s(b, "ref"), one_line(&s(b, "text"), 160))
}

fn size(n: u64) -> String { if n >= 1000 { format!("{:.1}k", n as f64 / 1000.) } else { n.to_string() } }

fn cell(v: &Value, k: &str) -> String { s(v, k).replace('|', "\\|").replace('\n', " ") }

/// Compact text for people and agents alike; `--json` has every field.
/// Paths start with the top-level folder's name.
pub fn text(op: &str, v: &Value) -> String {
    let mut o = String::new();
    match op {
        "ls" => for e in v.as_array().into_iter().flatten() {
            let mark = if e.get("pinned").is_some() { " pinned" } else { "" };
            if e.get("top").is_some() {
                // A top-level folder: where it is on disk, and whether it opens.
                let state = match e.get("documents") { Some(n) => format!("{n} documents{}", if e.get("default").is_some() { ", new documents go here" } else { "" }), None => format!("unavailable: {}", s(e, "error")) };
                o += &format!("{}/  ({state})  {}\n", s(e, "path"), s(e, "dir"));
            } else if s(e, "kind") == "folder" { o += &format!("{}/\n", s(e, "path")); } else { o += &format!("{}  [{}] {}{mark}\n", s(e, "path"), s(e, "kind"), s(e, "modified")); }
        },
        "find" => {
            let res: Vec<&Value> = list(v, "results").collect();
            let total = v["total"].as_u64().unwrap_or(0) as usize;
            o += &format!("{total} documents match{}\n", if res.len() < total { format!(" (first {} shown; --limit for more)", res.len()) } else { String::new() });
            for r in res {
                o += &format!("\n{}  rev {}\n", s(r, "path"), s(r, "rev"));
                for h in list(r, "hits") { o += &format!("  {:<12} {:<8} {}\n", s(h, "ref"), s(h, "kind"), s(h, "text")); }
                if let Some(n) = r.get("more_hits") { o += &format!("  … {n} more blocks match (quiet blocks DOC --grep WORD)\n"); }
            }
        }
        "outline" | "load" | "open" => {
            o += &format!("{}  [{}] rev {}  {} blocks, {} chars\n", s(v, "path"), s(v, "kind"), s(v, "rev"), s(v, "block_count"), size(v["chars"].as_u64().unwrap_or(0)));
            for h in list(v, "headings") {
                let l = h["level"].as_u64().unwrap_or(1) as usize;
                o += &format!("{:<12} {}{} {}  ({})\n", s(h, "ref"), "  ".repeat(l.saturating_sub(1)), "#".repeat(l), s(h, "text"), size(h["chars"].as_u64().unwrap_or(0)));
            }
            for b in list(v, "top") { o += &format!("{:<12} {:<8} {}\n", s(b, "ref"), s(b, "kind"), s(b, "text")); }
        }
        "read" | "blocks" if v.get("blocks").is_some() => { o += &format!("{}  rev {}\n", s(v, "path"), s(v, "rev")); for b in list(v, "blocks") { o += &block_line(b); } }
        "read" => {
            o += &s(v, "text");
            if !o.ends_with('\n') { o.push('\n'); }
            if v.get("truncated").is_some() { o += "…[truncated: read a section with DOC#REF --section (refs from `quiet outline`), or pass --max-chars N (0: all)]\n"; }
        }
        "links" => {
            o += &format!("{}\nout:\n", s(v, "path"));
            for l in list(v, "out") { o += &format!("  → {}  {}\n", if l["broken"].as_bool() == Some(true) { "(broken)".to_string() } else { s(l, "to") }, s(l, "text")); }
            o += "back:\n";
            for l in list(v, "back") { o += &format!("  ← {}  {}\n", s(l, "from"), l["text"].as_array().map(|t| t.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" / ")).unwrap_or_default()); }
        }
        "table" if v.get("text").is_some() => { o += &s(v, "text"); if !o.ends_with('\n') { o.push('\n'); } o += &stats(v); }
        "table" => {
            let fields: Vec<String> = list(v, "fields").filter_map(Value::as_str).map(str::to_string).collect();
            let types: Vec<String> = list(v, "schema").filter(|f| fields.contains(&s(f, "name"))).map(|f| {
                let mut d = s(f, "type");
                if let Some(x) = f.get("options").and_then(Value::as_array).filter(|l| !l.is_empty()) { d += &format!(" [{}]", x.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("/")); }
                for (k, sign) in [("formula", "="), ("link", "→"), ("lookup", "←")] { if f.get(k).is_some() { d += &format!(" {sign} {}", s(f, k)); } }
                if f.get("computed").is_some() { d += " (computed)"; }
                format!("{}: {d}", s(f, "name"))
            }).collect();
            let view = if v["view"].is_string() { format!(" › {}", s(v, "view")) } else { String::new() };
            let of = v.get("records").map(|n| format!(" of {n}")).unwrap_or_default();
            o += &format!("{} › {}{view}  ({} rows{of})\n", s(v, "src"), s(v, "table"), s(v, "total"));
            let tables: Vec<&str> = list(v, "tables").filter_map(Value::as_str).collect();
            if tables.len() > 1 { o += &format!("tables: {}\n", tables.join(", ")); }
            o += &format!("views: {}\n", list(v, "views").map(|w| format!("{} ({})", s(w, "name"), s(w, "kind"))).collect::<Vec<_>>().join(", "));
            if v.get("where").is_some() { o += &format!("where: {}\n", s(v, "where")); }
            o += &format!("fields: {}\n", types.join(", "));
            let head = format!("| _id | {} |\n| --- |{}\n", fields.join(" | "), " --- |".repeat(fields.len()));
            let line = |r: &Value| format!("| {} | {} |\n", s(r, "_id"), fields.iter().map(|f| cell(r, f)).collect::<Vec<_>>().join(" | "));
            fn groups(o: &mut String, gs: &Value, depth: usize, head: &str, line: &dyn Fn(&Value) -> String) {
                for g in gs.as_array().into_iter().flatten() {
                    let label = if s(g, "value").is_empty() { "(empty)".to_string() } else { s(g, "value") };
                    *o += &format!("\n{} {} = {}  ({} row{})\n", "#".repeat(depth + 2), s(g, "field"), label, s(g, "count"), if g["count"] == 1 { "" } else { "s" });
                    if g.get("groups").is_some() { groups(o, &g["groups"], depth + 1, head, line); } else { *o += head; for r in list(g, "rows") { *o += &line(r); } }
                }
            }
            if v.get("groups").is_some() { groups(&mut o, &v["groups"], 0, &head, &line); } else { o += &head; for r in list(v, "rows") { o += &line(r); } }
            if let Some(n) = v.get("more") { o += &format!("… {n} more rows (--offset / --limit)\n"); }
            o += &stats(v);
        }
        "table_views" => {
            o += &format!("{} › {}\n", s(v, "src"), s(v, "table"));
            for w in list(v, "views") { o += &view_line(w); }
        }
        "table_view_add" => o += &format!("added view {}", view_line(&v["added"])),
        "table_view_set" => o += &format!("{}view {}", if v["changed"] == true { "set " } else { "unchanged " }, view_line(&v["view"])),
        "table_view_rename" => o += &format!("renamed view {} → {}\n", s(v, "view"), s(v, "renamed")),
        "table_view_delete" => o += &format!("deleted view {}\n", s(v, "deleted")),
        "table_field_add" => o += &format!("added field {} ({}){}\n", s(v, "added"), s(v, "type"), v.get("mirror").map(|m| format!(", its mirror {}", m.as_str().unwrap_or(""))).unwrap_or_default()),
        "table_field_change" if v["changed"] != true => o += &format!("field {}: nothing to change\n", s(v, "field")),
        "table_field_change" => {
            o += &format!("changed field {} ({})", s(v, "field"), s(v, "type"));
            if let Some(n) = v.get("lost") { o += &format!(": {} value(s) converted, {n} lost", s(v, "converted")); }
            if let Some(n) = v.get("cells_cleared") { o += &format!(", {n} cell(s) lost a removed option"); }
            o += "\n";
            for l in list(v, "lost_rows") { o += &format!("  {} was {:?}\n", s(l, "row"), s(l, "was")); }
        }
        "table_field_delete" => {
            o += &format!("deleted field(s) {}\n", list(v, "deleted").filter_map(Value::as_str).collect::<Vec<_>>().join(", "));
            if let Some(m) = v.get("mirrors_deleted") { o += &format!("  with their mirror field(s) {}\n", refs(m)); }
            if let Some(b) = v.get("broken") { o += &format!("  these now show #引用失效: {}\n", refs(b)); }
        }
        "table_new" => o += &format!("added table {} ({} fields, {} rows)\n", s(v, "added"), list(v, "fields").count(), s(v, "rows")),
        "table_rename" => o += &format!("renamed table {} → {}\n", s(v, "table"), s(v, "renamed")),
        "table_remove" => o += &format!("removed table {}{}\n", s(v, "removed"), if list(v, "unlinked").count() > 0 { format!(" (links from {} turned to text)", refs(&v["unlinked"])) } else { String::new() }),
        "table_import" => {
            o += &format!("imported {} row(s) into {}\n", list(v, "added").count(), s(v, "table"));
            for f in list(v, "created_fields") { o += &format!("  new field {} ({})\n", s(f, "name"), s(f, "type")); }
        }
        "board" => {
            let labels = list(v, "elements").filter(|e| e.get("in").is_some()).count();
            o += &format!("{}  rev {}  {} elements{}", s(v, "path"), s(v, "rev"), list(v, "elements").count() - labels, if labels > 0 { format!(" (+{labels} labels, shown on their shapes)") } else { String::new() });
            if let Some(b) = v.get("bounds") { o += &format!(", within x {} y {} w {} h {}", s(b, "x"), s(b, "y"), s(b, "w"), s(b, "h")); }
            o += "\n";
            for e in list(v, "elements") {
                if e.get("in").is_some() { continue; } // labels show on their container
                let what = [e.get("label"), e.get("text"), e.get("name")].into_iter().flatten().filter_map(Value::as_str).map(|t| format!("{:?}", one_line(t, 80))).collect::<Vec<_>>().join(" ");
                let ends = match (e.get("from"), e.get("to")) { (None, None) => String::new(), (f, t) => format!(" {} → {}", f.and_then(Value::as_str).unwrap_or("·"), t.and_then(Value::as_str).unwrap_or("·")) };
                o += &format!("{:<22} {:<10} {},{} {}×{} {what}{ends}\n", s(e, "id"), s(e, "type"), s(e, "x"), s(e, "y"), s(e, "w"), s(e, "h"));
            }
        }
        "history" if v.get("diff").is_some() => {
            o += &format!("{}: from version {} ({}) to now\n", s(v, "path"), s(v, "version"), s(v, "time"));
            for l in list(v, "diff") { o += &format!("  {}\n", l.as_str().unwrap_or("")); }
        }
        "history" => {
            let vs: Vec<&Value> = list(v, "versions").collect();
            o += &format!("{}: {} versions, newest first. Each is the text before a save; \"then\" is what that save changed (lines added/removed).\n", s(v, "path"), vs.len());
            for x in vs { o += &format!("{:>3}  {}  then {:<10} {:<28} {}{}\n", s(x, "n"), s(x, "time"), s(x, "then"), s(x, "version"), size(x["size"].as_u64().unwrap_or(0)), x.get("label").and_then(Value::as_str).map(|l| format!("  “{l}”")).unwrap_or_default()); }
        }
        "trash_list" => {
            let items: Vec<&Value> = list(v, "items").collect();
            o += &format!("{} in the trash (kept {} days)\n", items.len(), s(v, "days"));
            for t in items { o += &format!("{}  {}/{}  ({})\n", s(t, "deleted"), s(t, "folder"), s(t, "title"), s(t, "id")); }
        }
        "check" => {
            let issues: Vec<&Value> = list(v, "issues").collect();
            o += &format!("{} documents, {} issues\n", s(v, "documents"), issues.len());
            for i in issues { o += &format!("{:<14} {}  {}\n", s(i, "code"), s(i, "path"), s(i, "message")); }
        }
        "insert" => o += &format!("inserted {}\n", refs(&v["inserted"])),
        "replace" => o += &format!("replaced {} block(s) with {}\n", s(v, "replaced"), refs(&v["inserted"])),
        "remove" => o += &format!("removed {} block(s)\n", s(v, "removed")),
        "move" => o += &format!("moved to {}\n", refs(&v["moved"])),
        "set" => o += &format!("set {}\n", refs(&v["set"])),
        "table_add" => o += &format!("added {}\n", refs(&v["added"])),
        "table_update" if list(v, "updated").count() == 0 => o += "updated no rows: none matched\n",
        "table_update" => o += &format!("updated {}\n", refs(&v["updated"])),
        "table_delete" => o += &format!("deleted {} row(s){}\n", s(v, "deleted"), if list(v, "rows").count() > 0 { format!(": {}", list(v, "rows").filter_map(Value::as_str).collect::<Vec<_>>().join(", ")) } else { String::new() }),
        "board_add" => o += &format!("added {}\n", refs(&v["added"])),
        "board_remove" => o += &format!("removed {} element(s)\n", s(v, "removed")),
        "create" | "mkdir" | "rename" | "mv" | "trash" | "restore" | "untrash" | "import" | "pin" | "unpin" | "duplicate" | "guide_import" => {}
        _ => o += &format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default()),
    }
    o
}

/// A table read's 统计, a line per field.
fn stats(v: &Value) -> String {
    let mut o = String::new();
    if v.get("stats").is_none() { return o; }
    o += "stats:\n";
    for x in list(v, "stats") { o += &format!("  {}: {}\n", s(x, "field"), crate::tables::STATS.iter().filter(|k| x.get(**k).is_some()).map(|k| format!("{k} {}", s(x, k))).collect::<Vec<_>>().join(", ")); }
    o
}

/// A view and its settings on one line.
fn view_line(w: &Value) -> String {
    let mut parts = vec![format!("{} ({}) {} rows", s(w, "name"), s(w, "kind"), s(w, "rows"))];
    let join = |k: &str| list(w, k).filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
    if w.get("where").is_some() { parts.push(format!("where {}", s(w, "where"))); }
    for k in ["sort", "group", "hidden", "order"] { if w.get(k).is_some() { parts.push(format!("{k} {}", join(k))); } }
    for k in ["stack", "date", "end"] { if w.get(k).is_some() { parts.push(format!("{k} {}", s(w, k))); } }
    if let Some(st) = w.get("stats").and_then(Value::as_object) { parts.push(format!("stats {}", st.iter().map(|(f, x)| format!("{f}={}", x.as_str().unwrap_or(""))).collect::<Vec<_>>().join(", "))); }
    format!("{}\n", parts.join(" · "))
}

/// What a commit wrote (or, `dry`, would write), by full paths (the top-level folder first).
pub fn report(r: &Value, dry: bool) -> String {
    let mut o = String::new();
    for c in r.as_array().into_iter().flatten() {
        let rev = match (c.get("rev_before"), c.get("rev")) { (Some(a), Some(b)) => format!("  rev {} → {}", a.as_str().unwrap_or(""), b.as_str().unwrap_or("")), (None, Some(b)) => format!("  rev {}", b.as_str().unwrap_or("")), _ => String::new() };
        let extra = c.get("from").map(|f| format!("  (was {})", f.as_str().unwrap_or(""))).or_else(|| c.get("source").map(|f| format!("  (from {})", f.as_str().unwrap_or("")))).or_else(|| c.get("to").map(|t| format!(" → {}", t.as_str().unwrap_or("")))).unwrap_or_default();
        o += &format!("{} {}{extra}{rev}\n", s(c, "action"), s(c, "path"));
        for l in list(c, "diff").chain(list(c, "changes")) { o += &format!("    {}\n", l.as_str().unwrap_or("")); }
    }
    if dry { o += if r.as_array().is_some_and(|l| !l.is_empty()) { "(dry run: nothing written)\n" } else { "(dry run: nothing would change)\n" }; }
    o
}
