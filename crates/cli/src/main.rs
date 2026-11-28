//! `quiet`: the Quiet.md library from the command line. Each command is one
//! `Session` operation and its commit; `quiet run` hands the same operations
//! to a JavaScript script, and `quiet mcp` both to an MCP client. Output is
//! compact text, `--json` for JSON; errors go to stderr with a code and a
//! hint, and the exit status says which kind.

mod app_control;
mod blocks;
mod boards;
mod fail;
mod help;
mod js;
mod library;
mod mcp;
mod output;
mod session;
mod table_schema;
mod tables;
mod usage;

use std::collections::HashMap;
use std::io::{IsTerminal, Read};

use serde_json::{Value, json};

use fail::{R, fail};
use session::Session;

/// The full guide for agents: the skill's reference in the repository
/// (skills/quiet-md, installed with `npx skills add`), the one copy.
pub const GUIDE: &str = include_str!("../../../skills/quiet-md/references/guide.md");

/// `read` prints at most this much unless `--max-chars` says otherwise (0: all).
const READ_CAP: u64 = 40_000;

pub struct Args {
    pos: Vec<String>,
    opt: HashMap<String, Vec<String>>,
    flags: Vec<String>,
}

const FLAGS: &[&str] = &["force", "json", "dry-run", "section", "no-heading", "recursive", "done", "undone", "markdown", "start", "end", "help", "version", "replace", "full", "any", "create-fields", "two-way", "single", "stats", "open", "trash"];

pub fn parse(raw: Vec<String>) -> R<Args> {
    let a = parse_as(raw.clone(), false)?;
    // `quiet table …`: `--end FIELD` (a view's end date) takes a value, and `--stats` may
    if a.pos.first().map(String::as_str) == Some("table") { parse_as(raw, true) } else { Ok(a) }
}

fn parse_as(raw: Vec<String>, table: bool) -> R<Args> {
    let mut a = Args { pos: vec![], opt: HashMap::new(), flags: vec![] };
    let mut it = raw.into_iter().peekable();
    while let Some(s) = it.next() {
        if s == "--" { a.pos.extend(it.by_ref()); break; }
        let name = match s.as_str() { "-r" => Some("recursive".to_string()), "-e" => Some("code".to_string()), "-h" => Some("help".to_string()), _ => s.strip_prefix("--").filter(|n| !n.is_empty()).map(str::to_string) };
        match name {
            // `--name=value` too.
            Some(n) if n.contains('=') => { let (k, v) = n.split_once('=').unwrap(); a.opt.entry(k.to_string()).or_default().push(v.to_string()); }
            // `--stats` alone, or `--stats sum,average` / `--stats 页数=sum` (after DOC, any word: a wrong one is reported)
            Some(n) if table && n == "stats" && it.peek().is_some_and(|v| !v.starts_with("--") && (a.pos.len() >= 2 || v.contains('=') || v.split(',').all(|x| tables::stat_named(x).is_some()))) => { let v = it.next().unwrap(); a.opt.entry(n).or_default().push(v); }
            Some(n) if FLAGS.contains(&n.as_str()) && !(table && matches!(n.as_str(), "start" | "end")) => a.flags.push(n),
            Some(n) => { let v = it.next().ok_or_else(|| fail("usage", format!("--{n} needs a value")))?; a.opt.entry(n).or_default().push(v); }
            None => a.pos.push(s),
        }
    }
    Ok(a)
}

impl Args {
    fn has(&self, f: &str) -> bool { self.flags.iter().any(|x| x == f) }
    fn get(&self, k: &str) -> Option<&str> { self.opt.get(k).and_then(|v| v.last()).map(String::as_str) }
    fn all(&self, k: &str) -> &[String] { self.opt.get(k).map_or(&[], Vec::as_slice) }
    fn nth(&self, i: usize, what: &str) -> R<&str> { self.pos.get(i).map(String::as_str).ok_or_else(|| fail("usage", format!("missing {what}")).hint("`quiet help COMMAND` shows how to call it")) }
}

/// Standard input, or what an MCP client sent in its place.
pub struct Input(Option<String>);

impl Input {
    fn text(&mut self) -> R<String> {
        if let Some(s) = self.0.take() { return Ok(s); }
        if std::io::stdin().is_terminal() { return Err(fail("usage", "`-` reads standard input, and nothing is piped in")); }
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    }

    /// An option's value; `-` reads the input.
    fn value(&mut self, a: &Args, k: &str) -> R<Option<String>> {
        match a.get(k) { Some("-") => Ok(Some(self.text()?)), Some(s) => Ok(Some(s.to_string())), None => Ok(None) }
    }

    /// An option holding JSON (or `-`).
    fn json(&mut self, a: &Args, k: &str) -> R<Option<Value>> {
        match self.value(a, k)? { Some(s) => serde_json::from_str(&s).map(Some).map_err(|e| fail("bad_args", format!("--{k}: not JSON: {e}"))), None => Ok(None) }
    }
}


/// `--done` / `--undone`.
fn done(a: &Args) -> Option<bool> { if a.has("done") { Some(true) } else if a.has("undone") { Some(false) } else { None } }

fn put(v: &mut Value, k: &str, x: Option<impl Into<Value>>) { if let Some(x) = x { v[k] = x.into(); } }

/// `--set Field=Value` (repeated) as one row's cells.
fn cells(a: &Args) -> R<Option<Value>> {
    if a.all("set").is_empty() { return Ok(None); }
    let mut o = serde_json::Map::new();
    for kv in a.all("set") {
        let (k, v) = kv.split_once('=').ok_or_else(|| fail("usage", format!("--set {kv:?}: write Field=Value")))?;
        o.insert(k.trim().to_string(), json!(v));
    }
    Ok(Some(Value::Object(o)))
}

const WRITES: &[&str] = &["insert", "replace", "remove", "move", "set", "create", "mkdir", "rename", "mv", "trash", "pin", "unpin", "duplicate", "restore", "untrash", "import", "table_add", "table_update", "table_delete",
    "table_field_add", "table_field_change", "table_field_delete", "table_view_add", "table_view_set", "table_view_rename", "table_view_delete", "table_new", "table_rename", "table_remove", "table_import", "board_add", "board_remove", "guide_import"];

/// `--where` (repeatable), `--any`, `--view`: which rows of a 数据表.
fn rows_where(v: &mut Value, a: &Args) {
    if !a.all("where").is_empty() { v["where"] = json!(a.all("where")); }
    if a.has("any") { v["any"] = json!(true); }
    put(v, "view", a.get("view"));
}

/// A comma list option as a list (absent: nothing).
fn list(v: &mut Value, a: &Args, opt: &str, key: &str) {
    let all: Vec<String> = a.all(opt).iter().flat_map(|s| s.split(',')).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    if !all.is_empty() { v[key] = json!(all); }
}

/// `--csv FILE|-` (or CSV text itself, holding a line break).
fn csv_arg(a: &Args, input: &mut Input) -> R<Option<String>> {
    match a.get("csv") {
        None => Ok(None),
        Some("-") => input.text().map(Some),
        Some(t) if t.contains('\n') => Ok(Some(t.to_string())),
        Some(f) => std::fs::read_to_string(f).map(Some).map_err(|e| fail("io", format!("{f}: {e}"))),
    }
}

/// The `quiet table field|view …` and `table new|rename|remove|import` calls.
fn table_calls(sub: &str, a: &mut Args, input: &mut Input, v: &mut Value) -> R<&'static str> {
    let two = |a: &Args, what: &str| -> R<String> { a.nth(1, what).map(str::to_string) };
    // `field add DOC …`: the second word picks the operation
    let leaf = if matches!(sub, "field" | "view") {
        match a.pos.first().map(String::as_str) {
            Some(x @ ("add" | "change" | "set" | "rename" | "delete" | "rm" | "remove")) => { let x = x.to_string(); a.pos.remove(0); Some(x) }
            _ if sub == "view" => None,
            _ => return Err(fail("usage", "quiet table field add|change|rename|delete DOC …")),
        }
    } else { None };
    let a = &*a;
    v["doc"] = json!(a.nth(0, "a document")?);
    v["split"] = json!(true);
    put(v, "table", a.get("table"));
    put(v, "rev", a.get("expect-rev"));
    Ok(match (sub, leaf.as_deref()) {
        ("field", Some("add")) => {
            v["name"] = json!(two(a, "the new field's name")?);
            put(v, "type", a.get("type")); list(v, a, "options", "options"); put(v, "formula", a.get("formula")); put(v, "link", a.get("link"));
            put(v, "lookup", a.get("lookup")); put(v, "rollup", a.get("rollup")); put(v, "format", a.get("format")); put(v, "after", a.get("after")); put(v, "description", a.get("description"));
            if a.has("two-way") { v["two_way"] = json!(true); }
            if a.has("single") { v["single"] = json!(true); }
            "table_field_add"
        }
        ("field", Some("change" | "set")) => {
            v["field"] = json!(two(a, "the field's name")?);
            for k in ["name", "type", "formula", "link", "lookup", "rollup", "format", "description"] { put(v, k, a.get(k)); }
            if a.get("options").is_some() { v["options"] = json!(a.all("options").iter().flat_map(|s| s.split(',')).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>()); }
            list(v, a, "add-option", "add_options"); list(v, a, "remove-option", "remove_options"); list(v, a, "rename-option", "rename_options");
            if a.has("single") { v["single"] = json!(true); }
            "table_field_change"
        }
        ("field", Some("rename")) => { v["field"] = json!(two(a, "the field's name")?); v["name"] = json!(a.nth(2, "the new name")?); "table_field_change" }
        ("field", Some(_)) => { if a.pos.len() < 2 { return Err(fail("usage", "table field delete needs field names")); } v["fields"] = json!(a.pos[1..]); "table_field_delete" }
        ("view", None) => "table_views",
        ("view", Some("add")) => { v["name"] = json!(two(a, "the new view's name")?); put(v, "kind", a.get("kind")); "table_view_add" }
        ("view", Some("rename")) => { v["view"] = json!(two(a, "the view's name")?); v["name"] = json!(a.nth(2, "the new name")?); "table_view_rename" }
        ("view", Some("set" | "change")) => {
            v["view"] = json!(two(a, "the view's name")?);
            if !a.all("where").is_empty() { v["where"] = json!(a.all("where")); }
            if a.has("any") { v["any"] = json!(true); }
            if !a.all("sort").is_empty() { v["sort"] = json!(a.all("sort")); }
            if !a.all("group").is_empty() { v["group"] = json!(a.all("group")); }
            for (o, k) in [("hide", "hide"), ("show", "show"), ("order", "order"), ("clear", "clear"), ("stats", "stats")] { list(v, a, o, k); }
            for k in ["stack", "date", "end"] { put(v, k, a.get(k)); }
            "table_view_set"
        }
        ("view", Some(_)) => { v["view"] = json!(two(a, "the view's name")?); "table_view_delete" }
        ("new", _) => { v["name"] = json!(two(a, "the new table's name")?); list(v, a, "fields", "fields"); put(v, "csv", csv_arg(a, input)?); v.as_object_mut().unwrap().remove("table"); "table_new" }
        ("rename", _) => { v["table"] = json!(two(a, "the table's name")?); v["name"] = json!(a.nth(2, "the new name")?); "table_rename" }
        ("remove", _) => { v["table"] = json!(two(a, "the table's name")?); if a.has("force") { v["force"] = json!(true); } "table_remove" }
        _ => {
            put(v, "csv", csv_arg(a, input)?);
            if let Some(r) = input.json(a, "rows")? { v["rows"] = r; }
            if v.get("csv").is_none() && v.get("rows").is_none() { return Err(fail("usage", "table import needs --csv FILE|- or --rows JSON|-")); }
            if a.has("create-fields") { v["create_fields"] = json!(true); }
            "table_import"
        }
    })
}

/// The operations a command line stands for (several for `import A B`), and whether they write.
fn to_calls(cmd: &str, a: &mut Args, input: &mut Input) -> R<(Vec<(String, Value)>, bool)> {
    let mut v = json!({});
    // `table add …`, `board rm …`: the subcommand is part of the command.
    let sub = match (cmd, a.pos.first().map(String::as_str)) {
        ("table", Some(s @ ("add" | "update" | "delete" | "rm" | "field" | "view" | "new" | "rename" | "remove" | "import"))) | ("board", Some(s @ ("add" | "rm" | "remove"))) => { let s = s.to_string(); a.pos.remove(0); Some(s) }
        _ => None,
    };
    if cmd == "table" && let Some(s @ ("field" | "view" | "new" | "rename" | "remove" | "import")) = sub.as_deref() {
        let op = table_calls(s, a, input, &mut v)?;
        return Ok((vec![(op.to_string(), v)], WRITES.contains(&op)));
    }
    let a = &*a;
    let place = |v: &mut Value| { for k in ["after", "before", "into"] { put(v, k, a.get(k)); } put(v, "end_of", a.get("end-of")); put(v, "to", ["start", "end"].into_iter().find(|f| a.has(f))); };
    let with_doc = |v: &mut Value, need_ref: bool| -> R<()> {
        // `DOC#REF`: the session splits it, as a title may hold `#` itself.
        v["doc"] = json!(a.nth(0, "a document")?);
        v["split"] = json!(true);
        if need_ref { v["need_ref"] = json!(true); }
        // `quiet delete 周报 风险` as `quiet delete 周报#风险`; `--at` wins.
        let second = (need_ref || matches!(cmd, "read" | "cat" | "blocks")) && a.pos.len() == 2;
        put(v, "at", a.get("at").map(str::to_string).or(second.then(|| a.pos[1].clone())));
        put(v, "rev", a.get("expect-rev"));
        if a.has("section") { v["section"] = json!(true); }
        if a.has("no-heading") { v["section"] = json!(true); v["heading"] = json!(false); }
        Ok(())
    };
    let num = |k: &str| a.get(k).and_then(|n| n.parse::<u64>().ok());
    let op = match (cmd, sub.as_deref()) {
        ("ls", _) => { put(&mut v, "folder", a.pos.first().cloned()); if a.has("recursive") { v["recursive"] = json!(true); } put(&mut v, "kind", a.get("kind")); "ls" }
        ("find" | "search", _) => { if a.pos.is_empty() { return Err(fail("usage", "find needs a query")); } v["query"] = json!(a.pos.join(" ")); put(&mut v, "folder", a.get("in")); put(&mut v, "limit", num("limit")); "find" }
        ("outline", _) => { with_doc(&mut v, false)?; "outline" }
        ("read" | "cat", _) => {
            with_doc(&mut v, false)?;
            put(&mut v, "as", a.get("as"));
            match num("max-chars") { Some(0) => {} Some(n) => v["max_chars"] = json!(n), None => v["max_chars"] = json!(READ_CAP) }
            "read"
        }
        ("blocks", _) => { with_doc(&mut v, false)?; put(&mut v, "kind", a.get("kind")); put(&mut v, "grep", a.get("grep")); put(&mut v, "done", done(a)); "blocks" }
        ("links", _) => { with_doc(&mut v, false)?; "links" }
        ("check", _) => "check",
        ("history", _) => { with_doc(&mut v, false)?; put(&mut v, "version", a.pos.get(1).cloned()); "history" }
        ("restore", _) => { with_doc(&mut v, false)?; v["version"] = json!(a.nth(1, "a version (from `quiet history DOC`)")?); "restore" }
        ("trash", _) => { if !a.pos.is_empty() { return Err(fail("usage", "`quiet trash` lists the trash; `quiet rm DOC` moves a document there")); } "trash_list" }
        ("untrash", _) => { v["name"] = json!(a.nth(0, "a title or id from `quiet trash`")?); "untrash" }
        // 导入最新的开始指南, as Settings › About does in the app.
        ("guide", _) => { put(&mut v, "folder", a.get("in")); put(&mut v, "lang", a.get("lang")); "guide_import" }
        ("import", _) => {
            if a.pos.is_empty() { return Err(fail("usage", "import needs files")); }
            let calls = a.pos.iter().map(|p| { let mut v = json!({ "path": p }); put(&mut v, "folder", a.get("in")); ("import".to_string(), v) }).collect();
            return Ok((calls, true));
        }
        ("table", None) => {
            with_doc(&mut v, false)?; put(&mut v, "table", a.get("table")); put(&mut v, "limit", num("limit")); put(&mut v, "offset", num("offset"));
            rows_where(&mut v, a);
            if !a.all("sort").is_empty() { v["sort"] = json!(a.all("sort")); }
            if !a.all("group").is_empty() { v["group"] = json!(a.all("group")); }
            list(&mut v, a, "fields", "fields");
            put(&mut v, "as", a.get("as"));
            if a.has("stats") { v["stats"] = json!(true); } else if let Some(s) = a.get("stats") { v["stats"] = json!(s); }
            "table"
        }
        ("table", Some("add")) => {
            with_doc(&mut v, false)?; put(&mut v, "table", a.get("table"));
            v["rows"] = match (input.json(a, "rows")?, cells(a)?) { (Some(r), _) | (None, Some(r)) => r, _ => return Err(fail("usage", "table add needs --set Field=Value (repeatable) or --rows JSON|-")) };
            "table_add"
        }
        ("table", Some("update")) => {
            with_doc(&mut v, false)?; put(&mut v, "table", a.get("table"));
            match (input.json(a, "rows")?, cells(a)?) {
                (Some(r), _) => v["rows"] = r,
                (None, Some(c)) if !a.all("where").is_empty() => { rows_where(&mut v, a); v["set"] = c; }
                (None, Some(c)) => { v["row"] = json!(a.nth(1, "a row id (r12), or --where conditions")?); v["set"] = c; }
                _ => return Err(fail("usage", "table update needs ROW --set Field=Value, --where COND --set Field=Value, or --rows JSON|- with an _id on each")),
            }
            "table_update"
        }
        ("table", Some(_)) => {
            with_doc(&mut v, false)?; put(&mut v, "table", a.get("table"));
            if a.pos.len() >= 2 { v["rows"] = json!(a.pos[1..]); } else if !a.all("where").is_empty() { rows_where(&mut v, a); } else { return Err(fail("usage", "table delete needs row ids (r12 …) or --where conditions")); }
            "table_delete"
        }
        ("board", None) => { with_doc(&mut v, false)?; if a.has("full") { v["full"] = json!(true); } "board" }
        ("board", Some("add")) => {
            with_doc(&mut v, false)?;
            v["elements"] = input.json(a, "elements")?.ok_or_else(|| fail("usage", "board add needs --elements JSON|- (a list of element skeletons)"))?;
            if a.has("replace") { v["replace"] = json!(true); }
            "board_add"
        }
        ("board", Some(_)) => { with_doc(&mut v, false)?; if a.pos.len() < 2 { return Err(fail("usage", "board rm needs element ids")); } v["ids"] = json!(a.pos[1..]); "board_remove" }
        ("new", _) => {
            v["title"] = json!(a.nth(0, "a title")?); put(&mut v, "folder", a.get("in")); put(&mut v, "md", input.value(a, "md")?); put(&mut v, "kind", a.get("kind"));
            put(&mut v, "csv", input.value(a, "csv")?); put(&mut v, "fields", a.get("fields").map(|f| f.split(',').map(str::trim).collect::<Vec<_>>()));
            if a.has("markdown") { v["markdown"] = json!(true); }
            "create"
        }
        ("mkdir", _) => { v["name"] = json!(a.nth(0, "a folder name")?); put(&mut v, "folder", a.get("in")); "mkdir" }
        ("insert" | "add", _) => {
            with_doc(&mut v, false)?;
            v["ref_is_after"] = json!(a.get("after").is_none() && a.get("end-of").is_none());
            place(&mut v);
            v["md"] = json!(input.value(a, "md")?.ok_or_else(|| fail("usage", "insert needs --md TEXT (or --md - to read stdin)"))?);
            "insert"
        }
        ("replace", _) => { with_doc(&mut v, true)?; v["md"] = json!(input.value(a, "md")?.ok_or_else(|| fail("usage", "replace needs --md TEXT (or --md - to read stdin)"))?); "replace" }
        ("delete", _) => { with_doc(&mut v, true)?; "remove" }
        ("move", _) => { with_doc(&mut v, true)?; place(&mut v); "move" }
        ("set", _) => { with_doc(&mut v, true)?; put(&mut v, "text", input.value(a, "text")?); put(&mut v, "kind", a.get("kind")); put(&mut v, "done", done(a)); "set" }
        ("rename", _) => { with_doc(&mut v, false)?; v["title"] = json!(a.nth(1, "the new title")?); "rename" }
        ("mv", _) => { with_doc(&mut v, false)?; v["folder"] = json!(a.nth(1, "a folder (/ for the top)")?); "mv" }
        ("rm", _) => { with_doc(&mut v, false)?; "trash" }
        ("pin" | "unpin", _) => { with_doc(&mut v, false)?; if cmd == "pin" { "pin" } else { "unpin" } }
        ("duplicate" | "cp", _) => { with_doc(&mut v, false)?; put(&mut v, "title", a.pos.get(1).cloned()); "duplicate" }
        ("call", _) => {
            let op = a.nth(0, "an operation")?.to_string();
            let args: Value = match a.pos.get(1).map(String::as_str) { Some("-") => serde_json::from_str(&input.text()?), Some(s) => serde_json::from_str(s), None => Ok(json!({})) }.map_err(|e| fail("bad_args", format!("JSON: {e}")))?;
            let write = WRITES.contains(&op.as_str());
            return Ok((vec![(op, args)], write));
        }
        _ => return Err(fail("usage", format!("unknown command {cmd:?}")).hint("`quiet help` lists the commands")),
    };
    Ok((vec![(op.to_string(), v)], WRITES.contains(&op)))
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let json_out = raw.iter().any(|s| s == "--json");
    if raw.first().map(String::as_str) == Some("__send-usage") { std::process::exit(usage::send()); }
    if raw.first().map(String::as_str) == Some("mcp") { usage::count("mcp", true); std::process::exit(mcp::serve(raw[1..].to_vec())); }
    let name = usage::name(&raw);
    let mut out = String::new();
    let r = execute(raw, Input(None), &mut out);
    usage::count(&name, r.is_ok());
    print!("{out}");
    if let Err(e) = r {
        if json_out { eprintln!("{}", e.json()); } else { eprintln!("{}", e.text()); }
        std::process::exit(e.exit());
    }
}

/// One command line, its output into `out` (what was printed before a failure stays).
pub fn execute(raw: Vec<String>, mut input: Input, out: &mut String) -> R<()> {
    let mut a = parse(raw)?;
    if a.pos.is_empty() {
        *out += &if a.has("version") { format!("quiet {}\n", env!("CARGO_PKG_VERSION")) } else { format!("{}\n", help::HELP) };
        return Ok(());
    }
    let cmd = a.pos.remove(0);
    match cmd.as_str() {
        "help" => { *out += &help::of(a.pos.first().map(String::as_str))?; return Ok(()); }
        // `guide import`: a write like the others, below.
        "guide" | "skill" if a.pos.first().map(String::as_str) != Some("import") => { *out += GUIDE; return Ok(()); }
        "version" => { *out += &format!("quiet {}\n", env!("CARGO_PKG_VERSION")); return Ok(()); }
        "telemetry" => { *out += &usage::command(a.pos.first().map(String::as_str))?; return Ok(()); }
        _ if a.has("help") => { *out += &help::of(Some(&cmd))?; return Ok(()); }
        // The sidebar's list, which may be empty: no library to open first.
        "folders" => return app_control::folders(&a, out),
        _ => {}
    }
    let json_out = a.has("json");
    let dry = a.has("dry-run");
    let mut s = Session::open(a.get("library"))?;
    if cmd == "open" { return app_control::open(&mut s, &a, out); }
    if cmd == "run" {
        let code = match (a.get("code"), a.pos.first().map(String::as_str)) {
            (Some(c), _) => c.to_string(),
            (None, Some("-")) | (None, None) => input.text().map_err(|_| fail("usage", "run needs a script: a file, - (stdin) or -e CODE"))?,
            (None, Some(f)) => std::fs::read_to_string(f).map_err(|e| fail("io", format!("{f}: {e}")))?,
        };
        let timeout = a.get("timeout").and_then(|t| t.parse::<u64>().ok()).unwrap_or(60);
        let (back, r) = js::run(s, &code, timeout, out);
        s = back;
        r?;
        if s.changed() {
            let report = s.commit(dry)?;
            if json_out { *out += &format!("{}\n", json!({ "committed": !dry, "changes": report })); } else { *out += &output::report(&report, dry); }
        }
        return Ok(());
    }
    let (calls, write) = to_calls(&cmd, &mut a, &mut input)?;
    let mut results = vec![];
    for (op, args) in &calls {
        let v = s.call(op, args)?;
        if !json_out { *out += &output::text(op, &v); }
        results.push(v);
    }
    let mut v = if results.len() == 1 { results.pop().unwrap() } else { json!(results) };
    if write {
        let report = s.commit(dry)?;
        if json_out {
            if !v.is_object() { v = json!({ "results": v }); }
            v["changes"] = report;
            v["committed"] = json!(!dry);
        } else { *out += &output::report(&report, dry); }
    }
    if json_out { *out += &format!("{v}\n"); }
    // `new … --open`: written, then shown in Quiet.md, as 新建 there opens what it made.
    if cmd == "new" && a.has("open") && !dry && let Some(path) = v.get("path").and_then(Value::as_str) {
        let mut s = Session::open(a.get("library"))?;
        let mut how = parse(vec![path.to_string()])?;
        how.flags.extend(a.flags.iter().filter(|f| *f == "json").cloned());
        app_control::open(&mut s, &how, out)?;
    }
    Ok(())
}
