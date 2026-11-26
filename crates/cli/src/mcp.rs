//! `quiet mcp`: the CLI as an MCP server (JSON-RPC 2.0, one message per
//! line on stdin/stdout) for agents with no shell. Typed tools for the
//! everyday steps (`list`, `read`, `search`, `create`, `edit`, `table`), each
//! a few `Session::call`s as the matching command makes; `quiet` runs any
//! command line and `quiet_run` a script, for the rest. Each call opens the
//! library afresh, as a command would.

use std::io::{BufRead, Write};

use serde_json::{Map, Value, json};

use crate::fail::{R, fail};
use crate::session::Session;
use crate::{Input, READ_CAP, apply, execute};

const INSTRUCTIONS: &str = "Quiet.md is the user's notes library: documents, Markdown notes, boards and 数据表 (data tables). Start with `list` (no folder: the top folders on 主页); every path starts at 主页 with a top folder's name, e.g. `Quiet.md/工作/周报` (a title only one document has works too). For a long document, `read` with outline: true first, then read only the sections you need. Reads print the document's rev: pass it as expect_rev when you edit, and on stale_rev / stale_ref / conflict read again and redo the edit, never force it. These are the user's own notes: prefer adding over rewriting, try a large change with dry_run first, and say which paths you changed. `quiet` with args [\"guide\"] prints the full guide (script API, boards, fields and views).";

/// A string property.
fn s(d: &str) -> Value { json!({ "type": "string", "description": d }) }
fn b(d: &str) -> Value { json!({ "type": "boolean", "description": d }) }
fn n(d: &str) -> Value { json!({ "type": "integer", "description": d }) }
fn strs(d: &str) -> Value { json!({ "type": "array", "items": { "type": "string" }, "description": d }) }

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    let mut props = props;
    props["json"] = b("Every field as JSON instead of compact text");
    // Hints for clients that ask before a tool writes: these three only read.
    let read_only = matches!(name, "list" | "read" | "search");
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": props, "required": required },
        "annotations": { "readOnlyHint": read_only, "destructiveHint": !read_only && name != "create", "openWorldHint": false } })
}

const DOC: &str = "A document: its path from 主页 (Quiet.md/工作/周报; extension optional), a title only one document has, or an id";
const REF: &str = "A block: a ref as outline/search print it (#3~k7f2), a position (#2.1) or a heading's text";

fn tools() -> Value {
    json!([
        tool("list", "List a folder's documents and subfolders (path, kind, last modified). No folder: the top folders on 主页, where each is on disk and whether it opens.", json!({
            "folder": s("A folder path from 主页, e.g. Quiet.md/工作; omit for 主页"),
            "recursive": b("Go into subfolders"),
            "kind": { "type": "string", "enum": ["doc", "md", "board", "table", "folder"], "description": "Only this kind" },
        }), &[]),
        tool("read", "Read a document as Markdown (its rev on the first line), or one block or section of it. outline: true gives the headings with refs and sizes instead; read those first in a long document. A 数据表 file gives its rows (as the `table` tool), a 画板 its elements.", json!({
            "doc": s(DOC),
            "at": s(REF),
            "section": b("With at on a heading: the heading and everything under it"),
            "heading": b("With section: false leaves the heading itself out"),
            "outline": b("Headings with refs and section sizes instead of the text"),
            "as": { "type": "string", "enum": ["md", "text", "xml", "json"], "description": "Default md" },
            "max_chars": n("Cut the text here; default 40000, 0 for all"),
            "limit": n("A 数据表: at most this many rows (default 100)"),
        }), &["doc"]),
        tool("search", "Documents holding every word (any case) in their title or text, best first, each with its rev and up to 5 matching blocks (ref, kind, snippet).", json!({
            "query": s("Words, all of which must occur"),
            "in": s("Only this folder (path from 主页)"),
            "limit": n("At most this many documents (default 20)"),
        }), &["query"]),
        tool("create", "Create a document (doc: DocxXML, md: a Markdown file), a 画板 (board) or a 数据表 (table). Content is Markdown: headings, lists, - [ ] todos, tables, code; [[path or title]] links another document. Goes into `in`, else the first top folder; the output says where.", json!({
            "title": s("The title (also the file name)"),
            "md": s("The body as Markdown"),
            "in": s("The folder (path from 主页)"),
            "kind": { "type": "string", "enum": ["doc", "md", "board", "table"], "description": "Default doc" },
            "fields": strs("A table: its column names"),
            "csv": s("A table: CSV text with a header row (types guessed from the values)"),
            "dry_run": b("Show what would be written, write nothing"),
        }), &["title"]),
        tool("edit", "Change a document with Markdown. insert: at start, end (default), after/before/into a block, or end_of a heading's section. replace: the block at (section: true for a heading's whole section; heading: false keeps the heading and replaces what is under it). set: the whole body. Pass the rev you read as expect_rev. Delete, move, a todo's state, rename, history and restore: the `quiet` tool.", json!({
            "doc": s(DOC),
            "action": { "type": "string", "enum": ["insert", "replace", "set"] },
            "md": s("The new content as Markdown"),
            "at": s(&format!("replace: {}", REF.to_lowercase())),
            "section": b("replace: the heading at and its section"),
            "heading": b("replace with section: false keeps the heading"),
            "after": s("insert after this block"),
            "before": s("insert before this block"),
            "into": s("insert as the last child of this block (a list item, quote, callout)"),
            "end_of": s("insert at the end of this heading's section"),
            "start": b("insert at the start"),
            "end": b("insert at the end (the default)"),
            "expect_rev": s("Fail with stale_rev if the document changed since this rev"),
            "dry_run": b("Show the diff, write nothing"),
        }), &["doc", "action", "md"]),
        tool("table", "A 数据表: query rows (default), or add, update or delete them. Rows have ids (r12); values are text as the app shows them and are written as if pasted (numbers, dates, true/false, select options, comma lists). Query: a view as the app shows it, plus where conditions 'FIELD OP VALUE' (OP: = != ~ !~ > >= < <= empty notempty before after; dates also today, this_week, past_7…). Fields, views and tables themselves change with the `quiet` tool: [\"table\", \"field\", \"add\", DOC, NAME, \"--type\", T], [\"table\", \"view\", …], [\"help\", \"table\"].", json!({
            "doc": s("A table file or a document showing a 数据表 (as for read)"),
            "action": { "type": "string", "enum": ["query", "add", "update", "delete"], "description": "Default query" },
            "at": s("A document's datatable block, if not its first"),
            "table": s("Which table of the file, if not its first"),
            "view": s("Read (or pick rows) as this view does"),
            "where": strs("Conditions, all of which must hold: 'FIELD OP VALUE'"),
            "any": b("Any condition suffices"),
            "sort": strs("FIELD or FIELD:desc"),
            "group": strs("Group by these fields"),
            "fields": strs("Only these columns"),
            "stats": { "type": ["boolean", "string"], "description": "true: each column's 统计; or names, e.g. sum,average" },
            "as": { "type": "string", "enum": ["json", "csv", "md"], "description": "The rows as CSV or a Markdown table" },
            "limit": n("At most this many rows"),
            "offset": n("Skip this many rows"),
            "rows": { "type": "array", "description": "add: [{Field: value}]; update: [{_id: \"r12\", Field: value}]; delete: [\"r12\", …]" },
            "row": s("update: one row's id"),
            "set": { "type": "object", "description": "update: {Field: value} for row, or for every row where picks" },
            "expect_rev": s("Fail with stale_rev if the file changed since this rev"),
            "dry_run": b("List what would change, write nothing"),
        }), &["doc"]),
        {
            "name": "quiet",
            "description": "Run one `quiet` command on the Quiet.md library and get its output, for what the other tools do not cover, e.g. [\"delete\", \"周报#3~k7f2\", \"--section\"], [\"set\", \"周报#5~a1b2\", \"--done\"], [\"history\", \"周报\"], [\"table\", \"field\", \"add\", \"书单\", \"评分\", \"--type\", \"rating\"]. [\"help\"] lists commands; [\"guide\"] prints the full guide. An option value of \"-\" reads `stdin`.",
            "inputSchema": { "type": "object", "properties": {
                "args": { "type": "array", "items": { "type": "string" }, "description": "The command line after `quiet`, one argument per item" },
                "stdin": { "type": "string", "description": "Text for an option given as -, e.g. --md -" }
            }, "required": ["args"] }
        },
        {
            "name": "quiet_run",
            "description": "Run a JavaScript script against the library's `lib` API (see the guide's Script API) as one transaction: nothing is written if it throws. Return or print a small result.",
            "inputSchema": { "type": "object", "properties": {
                "code": { "type": "string" },
                "dry_run": { "type": "boolean", "description": "Show what would change, write nothing" },
                "timeout": { "type": "number", "description": "Seconds, default 60" }
            }, "required": ["code"] }
        }
    ])
}

/// `a[from]` into `v[to]` when it is there.
fn take(v: &mut Value, a: &Value, from: &str, to: &str) { if let Some(x) = a.get(from).filter(|x| !x.is_null()) { v[to] = x.clone(); } }

fn flag(a: &Value, k: &str) -> bool { a.get(k).and_then(Value::as_bool) == Some(true) }

/// A typed tool's arguments as operations (what the matching command line
/// would call) and whether they write; `read` asks the session what kind of
/// file the document is.
fn typed(name: &str, a: &Value, s: &mut Session) -> R<(Vec<(String, Value)>, bool)> {
    if !a.is_object() { return Err(fail("bad_args", "arguments: an object")); }
    let need = |k: &str| a.get(k).and_then(Value::as_str).filter(|x| !x.trim().is_empty()).ok_or_else(|| fail("bad_args", format!("{name} needs {k}")));
    let mut v = json!({});
    // `DOC#REF` as on the command line; `at` wins.
    let doc = |v: &mut Value| -> R<()> { v["doc"] = json!(need("doc")?); v["split"] = json!(true); take(v, a, "at", "at"); take(v, a, "expect_rev", "rev"); Ok(()) };
    let section = |v: &mut Value| { if flag(a, "section") { v["section"] = json!(true); if a.get("heading").and_then(Value::as_bool) == Some(false) { v["heading"] = json!(false); } } };
    Ok(match name {
        "list" => { take(&mut v, a, "folder", "folder"); take(&mut v, a, "recursive", "recursive"); take(&mut v, a, "kind", "kind"); (vec![("ls".into(), v)], false) }
        "search" => { v["query"] = json!(need("query")?); take(&mut v, a, "in", "folder"); take(&mut v, a, "limit", "limit"); (vec![("find".into(), v)], false) }
        "read" => {
            doc(&mut v)?;
            let kind = s.call("open", &v)?["kind"].as_str().unwrap_or("doc").to_string();
            if flag(a, "outline") { return Ok((vec![("outline".into(), v)], false)); }
            // As text: a first line with the path, kind and rev to pass back as expect_rev.
            let head = (!flag(a, "json")).then(|| ("open".to_string(), json!({ "doc": v["doc"], "split": true })));
            let (op, v) = match kind.as_str() {
                "table" => { v["limit"] = a.get("limit").cloned().unwrap_or(json!(100)); ("table", v) }
                "board" => ("board", v),
                _ => {
                    section(&mut v); take(&mut v, a, "as", "as");
                    match a.get("max_chars").and_then(Value::as_u64) { Some(0) => {} Some(m) => v["max_chars"] = json!(m), None => v["max_chars"] = json!(READ_CAP) }
                    ("read", v)
                }
            };
            (head.into_iter().chain([(op.to_string(), v)]).collect(), false)
        }
        "create" => {
            v["title"] = json!(need("title")?);
            for (f, t) in [("md", "md"), ("in", "folder"), ("kind", "kind"), ("fields", "fields"), ("csv", "csv")] { take(&mut v, a, f, t); }
            (vec![("create".into(), v)], true)
        }
        "edit" => {
            doc(&mut v)?;
            v["md"] = json!(a.get("md").and_then(Value::as_str).ok_or_else(|| fail("bad_args", "edit needs md: the new content as Markdown"))?);
            let op = match a.get("action").and_then(Value::as_str) {
                Some("insert") => {
                    for k in ["after", "before", "into", "end_of"] { take(&mut v, a, k, k); }
                    if let Some(t) = ["start", "end"].into_iter().find(|k| flag(a, k)) { v["to"] = json!(t); }
                    // `at` alone is where to insert, as `quiet insert DOC#REF`.
                    if ["after", "before", "into", "end_of"].iter().all(|k| v.get(k).is_none()) && let Some(r) = v.as_object_mut().unwrap().remove("at") { v["after"] = r; }
                    "insert"
                }
                Some("replace") => { if a.get("at").is_none() && !need("doc")?.contains('#') { return Err(fail("bad_args", "replace needs at: the block to replace").hint("refs come from read with outline: true, or search; action set replaces the whole body")); } section(&mut v); "replace" }
                Some("set") => { v["all"] = json!(true); v.as_object_mut().unwrap().remove("at"); "replace" }
                x => return Err(fail("bad_args", format!("action {x:?}: use insert, replace or set"))),
            };
            (vec![(op.into(), v)], true)
        }
        "table" => {
            doc(&mut v)?;
            take(&mut v, a, "table", "table");
            let action = a.get("action").and_then(Value::as_str).unwrap_or("query");
            let rows_where = |v: &mut Value| { for k in ["where", "any", "view"] { take(v, a, k, k); } };
            let op = match action {
                "query" => { rows_where(&mut v); for k in ["sort", "group", "fields", "stats", "as", "limit", "offset"] { take(&mut v, a, k, k); } "table" }
                "add" => { v["rows"] = a.get("rows").cloned().ok_or_else(|| fail("bad_args", "add needs rows: [{Field: value}]"))?; "table_add" }
                "update" => {
                    match (a.get("rows"), a.get("set")) {
                        (Some(r), _) => v["rows"] = r.clone(),
                        (None, Some(set)) if a.get("where").is_some() => { rows_where(&mut v); v["set"] = set.clone(); }
                        (None, Some(set)) => { v["row"] = json!(need("row").map_err(|_| fail("bad_args", "update needs row (r12) or where, with set; or rows with an _id on each"))?); v["set"] = set.clone(); }
                        _ => return Err(fail("bad_args", "update needs set {Field: value} with row or where, or rows [{_id, Field: value}]")),
                    }
                    "table_update"
                }
                "delete" => { match a.get("rows") { Some(r) => v["rows"] = r.clone(), None if a.get("where").is_some() => rows_where(&mut v), None => return Err(fail("bad_args", "delete needs rows (ids) or where")) } "table_delete" }
                x => return Err(fail("bad_args", format!("action {x:?}: use query, add, update or delete"))),
            };
            (vec![(op.into(), v)], op != "table")
        }
        _ => return Err(fail("bad_args", format!("no tool {name:?}"))),
    })
}

/// What a typed tool counts as in the usage counts: its command's name.
fn command_of(name: &str, a: &Value) -> String {
    match (name, a.get("action").and_then(Value::as_str)) {
        ("list", _) => "ls".into(),
        ("search", _) => "find".into(),
        ("create", _) => "new".into(),
        ("edit", Some("insert")) => "insert".into(),
        ("edit", _) => "replace".into(),
        ("table", Some(x @ ("add" | "update" | "delete"))) => format!("table {x}"),
        (x, _) => x.into(),
    }
}

/// A tool call; its output and whether it failed (then the error's code, message and hint, as the CLI prints them).
fn call(name: &str, args: &Value, library: Option<&str>) -> (String, bool) {
    let mut raw: Vec<String> = library.map(|l| vec!["--library".to_string(), l.to_string()]).unwrap_or_default();
    let mut input = None;
    let mut out = String::new();
    let (cmd, r) = match name {
        "quiet" | "quiet_run" => {
            if name == "quiet" {
                let Some(list) = args.get("args").and_then(Value::as_array) else { return ("args: a list of strings".into(), true) };
                if list.first().and_then(Value::as_str) == Some("mcp") { return ("already serving".into(), true); }
                raw.extend(list.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)));
                input = args.get("stdin").and_then(Value::as_str).map(str::to_string);
            } else {
                let Some(code) = args.get("code").and_then(Value::as_str) else { return ("code: the script".into(), true) };
                raw.extend(["run".to_string(), "-e".to_string(), code.to_string()]);
                if args.get("dry_run").and_then(Value::as_bool) == Some(true) { raw.push("--dry-run".into()); }
                if let Some(t) = args.get("timeout").and_then(Value::as_u64) { raw.extend(["--timeout".to_string(), t.to_string()]); }
            }
            (crate::usage::name(&raw), execute(raw, Input(Some(input.unwrap_or_default())), &mut out))
        }
        _ => {
            let r = Session::open(library).and_then(|mut s| {
                let (calls, write) = typed(name, args, &mut s)?;
                apply(&mut s, &calls, write, flag(args, "dry_run"), flag(args, "json"), &mut out)
            });
            (command_of(name, args), r)
        }
    };
    crate::usage::count(&cmd, r.is_ok());
    match r {
        Ok(()) => (out, false),
        Err(e) => { if !out.is_empty() && !out.ends_with('\n') { out.push('\n'); } out += &e.text(); (out, true) }
    }
}

fn answer(id: &Value, result: Value) -> Value { json!({ "jsonrpc": "2.0", "id": id, "result": result }) }

fn error(id: &Value, code: i64, message: &str) -> Value { json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }) }

/// One message in, the reply (None for a notification).
fn handle(msg: &Value, library: Option<&str>) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    Some(match method {
        "initialize" => {
            let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18");
            answer(&id, json!({ "protocolVersion": version, "capabilities": { "tools": {} }, "serverInfo": { "name": "quiet", "version": env!("CARGO_PKG_VERSION") }, "instructions": INSTRUCTIONS }))
        }
        "ping" => answer(&id, json!({})),
        "tools/list" => answer(&id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let (text, failed) = call(name, params.get("arguments").unwrap_or(&Value::Object(Map::new())), library);
            answer(&id, json!({ "content": [{ "type": "text", "text": text }], "isError": failed }))
        }
        "resources/list" => answer(&id, json!({ "resources": [] })),
        "prompts/list" => answer(&id, json!({ "prompts": [] })),
        _ => error(&id, -32601, &format!("method not found: {method}")),
    })
}

pub fn serve(args: Vec<String>) -> i32 {
    // `--library DIR` applies to every call.
    let library = args.iter().position(|a| a == "--library").and_then(|i| args.get(i + 1)).cloned();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => { let r: Vec<Value> = batch.iter().filter_map(|m| handle(m, library.as_deref())).collect(); (!r.is_empty()).then(|| json!(r)) }
            Ok(msg) => handle(&msg, library.as_deref()),
            Err(e) => Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = reply && writeln!(stdout, "{r}").and_then(|_| stdout.flush()).is_err() { break; }
    }
    0
}
