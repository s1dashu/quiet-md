//! The `quiet` binary against libraries made in temporary folders.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

use fdl_core::model::{Document, RichText};
use fdl_core::store::Store;

static N: AtomicU32 = AtomicU32::new(0);

/// A fresh library holding `docs` (title, Markdown body).
fn library(docs: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("quiet-cli-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = Store::open_at(dir.clone()).unwrap();
    for (title, md) in docs {
        let blocks = fdl_core::paste::markdown_to_blocks(md, fdl_core::paste::Off { file: true, ..Default::default() });
        store.create_from(&Document { title: RichText::plain(*title), blocks, ..Default::default() }).unwrap();
    }
    dir
}

fn quiet(lib: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiet")).env("QUIET_NO_TELEMETRY", "1").arg("--library").arg(lib).args(args).output().unwrap()
}

fn ok(lib: &Path, args: &[&str]) -> String {
    let o = quiet(lib, args);
    assert!(o.status.success(), "quiet {args:?} failed: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

fn json(lib: &Path, args: &[&str]) -> serde_json::Value {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    serde_json::from_str(&ok(lib, &a)).unwrap()
}

fn script(lib: &Path, code: &str, extra: &[&str]) -> Output {
    let mut a = vec!["run", "-e", code];
    a.extend_from_slice(extra);
    quiet(lib, &a)
}

/// A ref's position, without its fingerprint.
fn at(r: &serde_json::Value) -> String { r.as_str().unwrap().split('~').next().unwrap().to_string() }

const PLAN: &str = "# Goals\nShip the CLI.\n\n## Tasks\n- [ ] write the guide\n- [x] parse arguments\n- [ ] measure agents\n\n# Notes\nNothing yet.";

#[test]
fn reads_outline_blocks_and_search() {
    let lib = library(&[("Plan", PLAN), ("Other", "Unrelated text.")]);
    let o = json(&lib, &["outline", "Plan"]);
    let heads: Vec<(String, String)> = o["headings"].as_array().unwrap().iter().map(|h| (at(&h["ref"]), h["text"].as_str().unwrap().into())).collect();
    assert_eq!(heads, [("#1".into(), "Goals".into()), ("#3".into(), "Tasks".into()), ("#7".into(), "Notes".into())]);
    let open = json(&lib, &["blocks", "Plan", "--kind", "todo", "--undone"]);
    let texts: Vec<&str> = open["blocks"].as_array().unwrap().iter().map(|b| b["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["write the guide", "measure agents"]);
    let section = ok(&lib, &["read", "Plan#Tasks", "--section"]);
    assert!(section.contains("- [ ] write the guide") && !section.contains("Nothing yet"), "{section}");
    let found = json(&lib, &["find", "MEASURE"]);
    assert_eq!(found["total"], 1);
    assert_eq!(at(&found["results"][0]["hits"][0]["ref"]), "#6");
    let missing = quiet(&lib, &["read", "Pla"]);
    assert_eq!(missing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("did you mean: Plan.xml"));
}

#[test]
fn writes_check_the_revision() {
    let lib = library(&[("Plan", PLAN)]);
    let rev = json(&lib, &["outline", "Plan"])["rev"].as_str().unwrap().to_string();
    let w = json(&lib, &["insert", "Plan#6", "--md", "- [ ] ship it", "--expect-rev", &rev]);
    assert_eq!(at(&w["inserted"][0]["ref"]), "#7");
    assert_ne!(w["changes"][0]["rev"].as_str().unwrap(), rev);
    let stale = quiet(&lib, &["delete", "Plan#1", "--expect-rev", &rev]);
    assert_eq!(stale.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("stale_rev"));
    assert!(ok(&lib, &["read", "Plan"]).contains("Goals"), "the stale delete must not have run");
}

#[test]
fn dry_run_writes_nothing() {
    let lib = library(&[("Plan", PLAN)]);
    let before = std::fs::read(lib.join("Plan.xml")).unwrap();
    let out = ok(&lib, &["replace", "Plan#Notes", "--section", "--md", "# Notes\nAll done.", "--dry-run"]);
    assert!(out.contains("+ All done.") && out.contains("- Nothing yet.") && out.contains("dry run"), "{out}");
    assert_eq!(std::fs::read(lib.join("Plan.xml")).unwrap(), before);
}

#[test]
fn a_failing_script_writes_nothing() {
    let lib = library(&[("Plan", PLAN)]);
    let before = std::fs::read(lib.join("Plan.xml")).unwrap();
    let o = script(&lib, "const d = lib.doc('Plan'); d.append('lost'); lib.create('Lost too'); throw new Error('boom');", &[]);
    assert_eq!(o.status.code(), Some(1));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("boom (line 1); nothing was written"), "{err}");
    assert_eq!(std::fs::read(lib.join("Plan.xml")).unwrap(), before);
    assert!(!lib.join("Lost too.xml").exists());
}

#[test]
fn api_errors_keep_their_code() {
    let lib = library(&[("Plan", PLAN)]);
    let o = script(&lib, "lib.doc('Plan').remove('#40')", &[]);
    assert_eq!(o.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error[stale_ref]"));
    let caught = script(&lib, "try { lib.doc('nope') } catch (e) { return e.code }", &[]);
    assert_eq!(String::from_utf8_lossy(&caught.stdout).trim(), "not_found");
}

#[test]
fn ids_survive_edits_within_a_script() {
    let lib = library(&[("Plan", PLAN)]);
    let out = ok(&lib, &["run", "-e", "const d = lib.doc('Plan'); const t = d.blocks({ kind: 'todo', grep: 'measure' })[0]; d.prepend('First line'); d.setDone(t); d.move(t, 'start'); return d.block(t).ref + ' ' + d.block(t).done"]);
    assert!(out.starts_with("#1~") && out.lines().next().unwrap().ends_with(" true"), "{out}");
    assert!(out.contains("saved Plan.xml"));
    let first = json(&lib, &["blocks", "Plan"])["blocks"][0].clone();
    assert_eq!((first["text"].as_str(), first["done"].as_bool()), (Some("measure agents"), Some(true)));
}

#[test]
fn a_script_spans_documents_and_folders() {
    let lib = library(&[("Plan", PLAN), ("Week", "- [ ] call the printer\n- [x] done already")]);
    let code = "lib.mkdir('Inbox'); let md = ''; for (const e of lib.ls()) { const t = lib.doc(e.path).blocks({ kind: 'todo', done: false }); if (t.length) md += `## ${e.title}\\n` + t.map(x => `- [ ] ${x.text}`).join('\\n') + '\\n\\n'; } return lib.create('Todos', { folder: 'Inbox', md }).path";
    let out = ok(&lib, &["run", "-e", code]);
    assert!(out.starts_with("Inbox/Todos.xml") && out.contains("folder created Inbox"), "{out}");
    let todos = json(&lib, &["blocks", "Inbox/Todos", "--kind", "todo"]);
    assert_eq!(todos["blocks"].as_array().unwrap().len(), 3);
}

#[test]
fn moving_a_document_and_trashing_it() {
    let lib = library(&[("Plan", PLAN)]);
    ok(&lib, &["mkdir", "Archive"]);
    let out = ok(&lib, &["mv", "Plan", "Archive"]);
    assert!(out.contains("moved Archive/Plan.xml"), "{out}");
    ok(&lib, &["rename", "Archive/Plan", "Old plan"]);
    assert!(lib.join("Archive/Old plan.xml").exists());
    ok(&lib, &["rm", "Old plan"]);
    assert!(!lib.join("Archive/Old plan.xml").exists());
    assert_eq!(json(&lib, &["ls", "-r", "--kind", "doc"]).as_array().unwrap().len(), 0);
}

#[test]
fn runaway_scripts_stop() {
    let lib = library(&[("Plan", PLAN)]);
    let o = script(&lib, "lib.doc('Plan').append('x'); while (true) {}", &["--timeout", "1"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[timeout]"));
    assert!(!ok(&lib, &["read", "Plan"]).contains("\nx\n"));
}

#[test]
fn the_sandbox_has_no_files_or_network() {
    let lib = library(&[]);
    let out = ok(&lib, &["run", "-e", "return [typeof require, typeof fetch, typeof process, typeof std, typeof os].join(' ')"]);
    assert_eq!(out.trim(), "undefined undefined undefined undefined undefined");
}

#[test]
fn reads_a_data_table() {
    let lib = library(&[]);
    let mut store = Store::open_at(lib.clone()).unwrap();
    store.create_table_in("Books", &fdl_core::table::Base::blank_named(0, "Shelf", "Column"), None).unwrap();
    drop(store);
    let t = json(&lib, &["table", "Books"]);
    assert_eq!(t["table"], "Shelf");
    assert_eq!(t["fields"][0], "Column 1");
    assert_eq!(t["rows"].as_array().unwrap().len(), 5);
    let edit = quiet(&lib, &["insert", "Books", "--md", "x"]);
    assert!(String::from_utf8_lossy(&edit.stderr).starts_with("error[read_only]"));
}

#[test]
fn not_a_library() {
    let dir = std::env::temp_dir().join(format!("quiet-cli-test-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let o = quiet(&dir, &["ls"]);
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error[no_library]"));
    assert!(!dir.join(".quiet").exists(), "the CLI must not turn a folder into a library");
}

#[test]
fn refs_find_their_block_after_others_edit() {
    let lib = library(&[("Plan", PLAN)]);
    let notes = json(&lib, &["outline", "Plan"])["headings"][2]["ref"].as_str().unwrap().to_string();
    assert!(notes.starts_with("#7~"), "{notes}");
    // Someone else adds a block at the top: the old ref still means Notes.
    ok(&lib, &["insert", "Plan", "--start", "--md", "Added by someone else"]);
    ok(&lib, &["set", &format!("Plan{notes}"), "--text", "Notes, renamed"]);
    assert_eq!(json(&lib, &["blocks", "Plan", "--kind", "h1"])["blocks"][1]["text"], "Notes, renamed");
    // The block changed since that ref was read: it is refused, not misapplied.
    let stale = quiet(&lib, &["delete", &format!("Plan{notes}")]);
    assert_eq!(stale.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("no longer the one you read"));
}

#[test]
fn find_wants_every_word_and_ranks_titles_first() {
    let lib = library(&[("Budget notes", "Travel plans."), ("Trip", "The budget for travel is small."), ("Other", "budget only")]);
    let r = json(&lib, &["find", "budget", "travel"]);
    assert_eq!(r["total"], 2);
    let r = json(&lib, &["find", "budget", "notes"]);
    assert_eq!(r["results"][0]["path"], "Budget notes.xml");
    let r = json(&lib, &["find", "budget"]);
    assert_eq!(r["results"][0]["path"], "Budget notes.xml", "a title match first: {r}");
}

#[test]
fn long_reads_are_cut_with_a_hint() {
    let long = "word ".repeat(20_000);
    let lib = library(&[("Long", &long)]);
    let out = ok(&lib, &["read", "Long"]);
    assert!(out.contains("[truncated") && out.chars().count() < 41_000);
    assert!(!ok(&lib, &["read", "Long", "--max-chars", "0"]).contains("[truncated"));
}

#[test]
fn data_table_rows_are_written_in_one_commit() {
    let lib = library(&[]);
    ok(&lib, &["new", "Books", "--kind", "table", "--csv", "Title,Pages,Read\nDune,412,false\n"]);
    let t = json(&lib, &["table", "Books"]);
    assert_eq!(t["schema"][1]["type"], "number");
    let added = json(&lib, &["table", "add", "Books", "--set", "Title=Emma", "--set", "Pages=320"]);
    let id = added["added"][0].as_str().unwrap().to_string();
    ok(&lib, &["table", "update", "Books", &id, "--set", "Read=true"]);
    let rows = json(&lib, &["table", "Books"])["rows"].clone();
    assert_eq!((rows[1]["Title"].as_str(), rows[1]["Pages"].as_str()), (Some("Emma"), Some("320")));
    assert_ne!(rows[1]["Read"], "", "the checkbox is set");
    // A script that fails after adding rows leaves the file alone.
    let o = script(&lib, "const b = lib.doc('Books'); b.addRows([{ Title: 'Lost' }]); throw new Error('no')", &[]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(json(&lib, &["table", "Books"])["total"], 2);
    let bad = quiet(&lib, &["table", "add", "Books", "--set", "Nope=1"]);
    assert!(String::from_utf8_lossy(&bad.stderr).contains("no field \"Nope\""));
    ok(&lib, &["table", "delete", "Books", &id]);
    assert_eq!(json(&lib, &["table", "Books"])["total"], 1);
}

#[test]
fn boards_take_element_skeletons() {
    let lib = library(&[]);
    ok(&lib, &["new", "Flow", "--kind", "board"]);
    let els = r#"[{"type":"rectangle","id":"a","x":0,"y":0,"label":{"text":"Start"}},{"type":"ellipse","id":"b","x":300,"y":0,"label":{"text":"End"}},{"type":"arrow","start":{"id":"a"},"end":{"id":"b"}}]"#;
    let added = json(&lib, &["board", "add", "Flow", "--elements", els]);
    assert_eq!(added["added"].as_array().unwrap().len(), 3, "labels are not named back: {added}");
    let b = json(&lib, &["board", "Flow"]);
    let arrow = b["elements"].as_array().unwrap().iter().find(|e| e["type"] == "arrow").unwrap().clone();
    assert_eq!((arrow["from"].as_str(), arrow["to"].as_str()), (Some("a"), Some("b")));
    assert!(b["elements"].as_array().unwrap().iter().any(|e| e["label"] == "Start"));
    ok(&lib, &["board", "rm", "Flow", "b"]);
    let left = json(&lib, &["board", "Flow"])["elements"].as_array().unwrap().iter().filter(|e| e.get("in").is_none()).count();
    assert_eq!(left, 2);
    assert!(std::fs::read_to_string(lib.join("Flow.excalidraw")).unwrap().contains("\"type\": \"excalidraw\""));
}

#[test]
fn history_undoes_a_save() {
    let lib = library(&[("Plan", PLAN)]);
    ok(&lib, &["set", "Plan#2", "--text", "Ship it all"]);
    let h = json(&lib, &["history", "Plan"]);
    assert!(!h["versions"].as_array().unwrap().is_empty());
    assert!(ok(&lib, &["history", "Plan", "1"]).contains("+ Ship it all"));
    ok(&lib, &["restore", "Plan", "1"]);
    assert!(ok(&lib, &["read", "Plan"]).contains("Ship the CLI."));
    // The restore kept what it replaced: it can be undone too.
    ok(&lib, &["restore", "Plan", "1"]);
    assert!(ok(&lib, &["read", "Plan"]).contains("Ship it all"));
}

#[test]
fn trash_and_back() {
    let lib = library(&[("Plan", PLAN)]);
    ok(&lib, &["rm", "Plan"]);
    assert!(ok(&lib, &["trash"]).contains("Plan"));
    ok(&lib, &["untrash", "Plan"]);
    assert!(lib.join("Plan.xml").exists());
    assert!(!ok(&lib, &["trash"]).contains("Plan"));
}

#[test]
fn imports_files_but_not_from_scripts() {
    let lib = library(&[]);
    let src = lib.with_extension("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("note.md"), "# Note\n\n- [ ] from outside\n").unwrap();
    std::fs::write(src.join("data.csv"), "Name,Count\nA,1\nB,2\n").unwrap();
    ok(&lib, &["mkdir", "In"]);
    let out = ok(&lib, &["import", src.join("note.md").to_str().unwrap(), src.join("data.csv").to_str().unwrap(), "--in", "In"]);
    assert!(out.contains("imported In/note.md") && out.contains("imported In/data.sqlite"), "{out}");
    assert_eq!(json(&lib, &["blocks", "In/note.md", "--kind", "todo"])["blocks"][0]["text"], "from outside");
    assert_eq!(json(&lib, &["table", "In/data.sqlite"])["total"], 2);
    let o = script(&lib, &format!("lib.call('import', {{ path: {:?} }})", src.join("note.md").to_str().unwrap()), &[]);
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error[sandbox]"));
}

#[test]
fn every_command_has_help() {
    let lib = library(&[]);
    for c in ["ls", "find", "outline", "read", "blocks", "links", "check", "new", "mkdir", "insert", "replace", "delete", "move", "set", "rename", "mv", "rm", "table", "board", "history", "restore", "trash", "untrash", "import", "run", "call", "mcp", "guide", "telemetry", "folders", "open", "duplicate", "pin"] {
        let out = ok(&lib, &["help", c]);
        assert!(out.starts_with(&format!("quiet {c}")), "{c}: {out}");
    }
    assert!(ok(&lib, &["insert", "--help"]).contains("--after"));
}

#[test]
fn serves_mcp() {
    use std::io::Write;
    let lib = library(&[("Plan", PLAN)]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_quiet")).env("QUIET_NO_TELEMETRY", "1").args(["mcp", "--library"]).arg(&lib)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let msgs = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"quiet","arguments":{"args":["outline","Plan"]}}}"#,
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"quiet_run","arguments":{"code":"lib.doc('Plan').append('via mcp'); return 1"}}}"#,
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"quiet","arguments":{"args":["read","Nope"]}}}"#,
    ];
    child.stdin.as_mut().unwrap().write_all((msgs.join("\n") + "\n").as_bytes()).unwrap();
    drop(child.stdin.take());
    let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    let replies: Vec<serde_json::Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(replies.len(), 5, "the notification gets no reply: {out}");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "quiet");
    assert_eq!(replies[1]["result"]["tools"].as_array().unwrap().len(), 2);
    assert!(replies[2]["result"]["content"][0]["text"].as_str().unwrap().contains("Goals"));
    assert_eq!(replies[3]["result"]["isError"], false);
    assert!(ok(&lib, &["read", "Plan"]).contains("via mcp"));
    assert_eq!(replies[4]["result"]["isError"], true);
}

#[test]
fn wiki_links_become_mentions_that_follow_moves() {
    let lib = library(&[("Plan", PLAN), ("Notes", "Some notes.")]);
    ok(&lib, &["insert", "Notes", "--md", "See [[Plan]] and [the plan](Plan.xml), not [the web](https://example.com)."]);
    let links = json(&lib, &["links", "Notes"]);
    assert_eq!((links["out"].as_array().unwrap().len(), links["out"][0]["to"].as_str()), (1, Some("Plan.xml")), "both mention Plan, one line: {links}");
    assert!(json(&lib, &["links", "Plan"])["back"][0]["from"] == "Notes.xml");
    // Read out as [[path]], so a copied section keeps its links …
    assert!(ok(&lib, &["read", "Notes"]).contains("See [[Plan]] and [[Plan]], not [the web](https://example.com)."));
    // … which follow the document when it moves.
    ok(&lib, &["mkdir", "Old"]);
    ok(&lib, &["mv", "Plan", "Old"]);
    assert!(ok(&lib, &["read", "Notes"]).contains("See [[Old/Plan]]"));
    let missing = quiet(&lib, &["insert", "Notes", "--md", "[[Nowhere]]"]);
    assert_eq!(missing.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("[[Nowhere]]: no such document"));
    ok(&lib, &["set", "Notes#1", "--text", "Now about [[Old/Plan]]"]);
    assert_eq!(json(&lib, &["links", "Notes"])["out"].as_array().unwrap().len(), 2, "a mention in each of two blocks");
}

#[test]
fn sections_can_be_extended_and_read_without_their_heading() {
    let lib = library(&[("Plan", PLAN)]);
    ok(&lib, &["insert", "Plan", "--end-of", "Tasks", "--md", "- [ ] review"]);
    let tasks = json(&lib, &["read", "Plan#Tasks", "--section", "--as", "json"])["blocks"].clone();
    assert_eq!(tasks.as_array().unwrap().last().unwrap()["text"], "review");
    let body = ok(&lib, &["read", "Plan#Tasks", "--no-heading"]);
    assert!(!body.contains("## Tasks") && body.contains("- [ ] review"), "{body}");
    let out = ok(&lib, &["run", "-e", "return lib.doc('Plan').markdown('Goals', { section: true, heading: false })"]);
    assert!(out.starts_with("Ship the CLI."), "{out}");
}

#[test]
fn history_says_what_each_save_changed() {
    let lib = library(&[("Plan", PLAN)]);
    ok(&lib, &["set", "Plan#2", "--text", "Ship it all"]);
    let v = json(&lib, &["history", "Plan"]);
    assert_eq!(v["versions"][0]["then"], "+1 -1", "{v}");
}

#[test]
fn a_board_made_in_a_script_can_be_drawn_on_at_once() {
    let lib = library(&[]);
    ok(&lib, &["run", "-e", "const b = lib.create('Map', { kind: 'board' }); b.addToBoard([{ type: 'rectangle', id: 'x', x: 0, y: 0, label: { text: 'here' } }])"]);
    assert_eq!(json(&lib, &["board", "Map"])["elements"].as_array().unwrap().iter().filter(|e| e.get("in").is_none()).count(), 1);
}

#[test]
fn pictures_from_outside_are_copied_in_at_commit() {
    let lib = library(&[("Plan", PLAN)]);
    let src = lib.with_extension("pics");
    std::fs::create_dir_all(&src).unwrap();
    let png = src.join("chart.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\nnot really").unwrap();
    let md = format!("![a chart]({})", png.display());
    // A dry run copies nothing; a script may not reach outside the library.
    ok(&lib, &["insert", "Plan", "--md", &md, "--dry-run"]);
    let assets = Store::assets_in(&lib);
    let count = || std::fs::read_dir(&assets).map(|d| d.count()).unwrap_or(0);
    assert_eq!(count(), 0);
    let o = script(&lib, &format!("lib.doc('Plan').append({md:?})"), &[]);
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error[sandbox]"));
    let out = ok(&lib, &["insert", "Plan", "--md", &md]);
    assert!(out.contains("copied picture .assets/"), "{out}");
    assert_eq!(count(), 1);
    let xml = std::fs::read_to_string(lib.join("Plan.xml")).unwrap();
    assert!(!xml.contains(src.to_str().unwrap()), "the document points at the copy, not the original");
    // Imported Markdown brings its pictures along.
    std::fs::write(src.join("note.md"), "# Note\n\n![](chart.png)\n").unwrap();
    ok(&lib, &["import", src.join("note.md").to_str().unwrap()]);
    assert_eq!(count(), 2);
    assert!(!std::fs::read_to_string(lib.join("note.md")).unwrap().contains(src.to_str().unwrap()));
}

#[test]
fn refs_tell_mentions_apart_and_take_a_second_argument() {
    let lib = library(&[("Plan", PLAN), ("A", "a"), ("B", "b")]);
    let ins = json(&lib, &["insert", "Plan", "--md", "- [[A]]\n- [[B]]"]);
    let (r1, r2) = (ins["inserted"][0]["ref"].as_str().unwrap(), ins["inserted"][1]["ref"].as_str().unwrap());
    assert_ne!(r1.split('~').nth(1), r2.split('~').nth(1), "two mentions, two fingerprints");
    ok(&lib, &["delete", "Plan", "Notes", "--section"]);
    assert!(!ok(&lib, &["read", "Plan"]).contains("Nothing yet"));
    // The body only: the title is not a line of it.
    assert!(!ok(&lib, &["read", "A"]).contains("# A"));
}

#[test]
fn markdown_files_keep_wiki_links_and_mentions_show_their_title() {
    let lib = library(&[("Plan", PLAN)]);
    ok(&lib, &["new", "Note", "--kind", "md", "--md", "## [[Plan]]\n\nsee [[Plan]]"]);
    assert!(std::fs::read_to_string(lib.join("Note.md")).unwrap().contains("## [[Plan]]"));
    assert_eq!(json(&lib, &["links", "Note.md"])["out"][0]["to"], "Plan.xml");
    ok(&lib, &["new", "Doc", "--md", "## [[Plan]]\n\nbody"]);
    assert_eq!(json(&lib, &["outline", "Doc"])["headings"][0]["text"], "Plan");
    assert!(ok(&lib, &["read", "Doc#Plan", "--section"]).contains("body"));
}

#[test]
fn titles_may_hold_a_hash() {
    let lib = library(&[("Issue #1", "first\n\n# Notes\nsecond"), ("Ends#", "the end")]);
    assert!(ok(&lib, &["read", "Ends#"]).contains("the end"));
    assert!(ok(&lib, &["read", "Ends##1"]).contains("the end"));
    assert!(ok(&lib, &["read", "Issue #1"]).contains("first"));
    assert!(ok(&lib, &["read", "Issue #1#Notes", "--section"]).contains("second"));
    assert!(ok(&lib, &["read", "Issue #1.xml#1"]).contains("first"));
    ok(&lib, &["insert", "Issue #1#1", "--md", "after the first"]);
    assert_eq!(json(&lib, &["blocks", "Issue #1"])["blocks"][1]["text"], "after the first");
}

#[test]
fn the_guide_is_the_skills_reference() {
    let lib = library(&[]);
    let repo = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/quiet-md/references/guide.md")).unwrap();
    assert_eq!(ok(&lib, &["guide"]).replace("\r\n", "\n"), repo.replace("\r\n", "\n"));
    assert!(!quiet(&lib, &["skill", "install"]).stdout.is_empty(), "quiet skill is the guide too; nothing installs");
    let skill = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/quiet-md/SKILL.md")).unwrap().replace("\r\n", "\n");
    assert!(skill.starts_with("---\nname: quiet-md\ndescription: "), "the frontmatter comes first");
}

/// `quiet` counting into `usage` (a file of the test's own), nothing in the environment turning it off.
fn counted(lib: &Path, usage: &Path, args: &[&str], url: Option<&str>) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_quiet"));
    c.env_remove("QUIET_NO_TELEMETRY").env_remove("DO_NOT_TRACK").env_remove("CI").env("QUIET_MD_HOME", lib).env("QUIET_TELEMETRY_FILE", usage);
    c.env("QUIET_TELEMETRY_URL", url.unwrap_or("http://127.0.0.1:9/never"));
    c.args(args).output().unwrap()
}

#[test]
fn usage_is_counted_by_command_name_only() {
    let lib = library(&[("Secret plan", PLAN)]);
    let usage = lib.with_extension("usage.json");
    let first = counted(&lib, &usage, &["read", "Secret plan"], None);
    assert!(String::from_utf8_lossy(&first.stderr).contains("quiet telemetry off"), "said once that it counts");
    let again = counted(&lib, &usage, &["table", "add", "Secret plan", "--set", "x=1"], None);
    assert!(!String::from_utf8_lossy(&again.stderr).contains("quiet telemetry off"));
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&usage).unwrap()).unwrap();
    let today = v["days"].as_object().unwrap().values().next().unwrap().clone();
    assert_eq!((today["runs"].as_u64(), today["errors"].as_u64()), (Some(2), Some(1)));
    assert_eq!(today["commands"]["table add"], 1);
    let text = std::fs::read_to_string(&usage).unwrap();
    assert!(!text.contains("Secret") && !text.contains("x=1"), "no names, no arguments: {text}");
    // Off: forgotten, and nothing more is counted.
    counted(&lib, &usage, &["telemetry", "off"], None);
    counted(&lib, &usage, &["ls"], None);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&usage).unwrap()).unwrap();
    assert!(v.get("days").is_none() && v.get("id").is_none(), "{v}");
    // The app's 自动检查更新 off: nothing is written at all.
    let other = lib.with_extension("usage2.json");
    std::fs::write(lib.join(".quiet/preferences.json"), r#"{"auto_update": false}"#).unwrap();
    counted(&lib, &other, &["ls"], None);
    assert!(!other.exists());
    // 匿名使用统计, once touched, decides on its own.
    std::fs::write(lib.join(".quiet/preferences.json"), r#"{"auto_update": false, "usage_stats": true}"#).unwrap();
    counted(&lib, &other, &["ls"], None);
    assert!(other.exists());
    let third = lib.with_extension("usage3.json");
    std::fs::write(lib.join(".quiet/preferences.json"), r#"{"auto_update": true, "usage_stats": false}"#).unwrap();
    counted(&lib, &third, &["ls"], None);
    assert!(!third.exists());
}

#[test]
fn finished_days_are_sent_and_forgotten() {
    use std::io::{Read, Write};
    let lib = library(&[]);
    let usage = lib.with_extension("usage.json");
    std::fs::write(&usage, r#"{"id":"0b8e8f2e-3c4a-4b7e-9d55-5f6f7a8b9c0d","noticed":true,"days":{"2020-01-01":{"runs":3,"errors":0,"commands":{"read":3}},"2999-01-01":{"runs":1,"errors":0,"commands":{"ls":1}}}}"#).unwrap();
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/cli", server.local_addr().unwrap());
    let got = std::thread::spawn(move || {
        let (mut s, _) = server.accept().unwrap();
        let mut buf = vec![0u8; 65536];
        let mut req = Vec::new();
        loop {
            let n = s.read(&mut buf).unwrap();
            req.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&req).to_string();
            if let Some(head) = text.find("\r\n\r\n") {
                let len: usize = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                if req.len() >= head + 4 + len { break; }
            }
            if n == 0 { break; }
        }
        s.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").unwrap();
        String::from_utf8_lossy(&req).to_string()
    });
    let o = counted(&lib, &usage, &["__send-usage"], Some(&url));
    assert!(o.status.success());
    let req = got.join().unwrap();
    assert!(req.starts_with("POST /v1/cli"), "{req}");
    let body: serde_json::Value = serde_json::from_str(&req[req.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(body["days"].as_array().unwrap().len(), 1, "today's (here: a future day) is not finished: {body}");
    assert_eq!(body["days"][0]["commands"]["read"], 3);
    let left: serde_json::Value = serde_json::from_slice(&std::fs::read(&usage).unwrap()).unwrap();
    assert!(left["days"].get("2020-01-01").is_none() && left["days"].get("2999-01-01").is_some());
}

/// Two top-level folders, as the app keeps them: folders `Work` and `Personal` and a
/// `folders.json` of their own (`QUIET_MD_FOLDERS`, so this computer's list is untouched).
fn two_folders(work: &[(&str, &str)], personal: &[(&str, &str)]) -> (PathBuf, PathBuf, PathBuf) {
    let (mut dirs, file) = top_folders(&[("Work", work), ("Personal", personal)]);
    (dirs.remove(0), dirs.remove(0), file)
}

/// Top-level folders named and filled as given, in that order, and their `folders.json`.
fn top_folders(list: &[(&str, &[(&str, &str)])]) -> (Vec<PathBuf>, PathBuf) {
    let base = library(&[]).with_extension("folders");
    let _ = std::fs::remove_dir_all(&base);
    let mut dirs = vec![];
    for (name, docs) in list {
        let dir = base.join(name);
        let mut store = Store::open_at(dir.clone()).unwrap();
        for (title, md) in *docs {
            let blocks = fdl_core::paste::markdown_to_blocks(md, fdl_core::paste::Off { file: true, ..Default::default() });
            store.create_from(&Document { title: RichText::plain(*title), blocks, ..Default::default() }).unwrap();
        }
        dirs.push(dir);
    }
    let list = serde_json::json!({ "v": 1, "folders": dirs.iter().enumerate().map(|(i, d)| serde_json::json!({ "key": format!("k{i}"), "path": d })).collect::<Vec<_>>() });
    let file = base.join("folders.json");
    std::fs::write(&file, list.to_string()).unwrap();
    (dirs, file)
}

/// `quiet` on the top-level folders in `file`, as it runs beside the app (no --library).
fn folders_run(file: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_quiet")).env("QUIET_NO_TELEMETRY", "1").env_remove("QUIET_MD_HOME").env("QUIET_MD_FOLDERS", file).args(args).output().unwrap()
}

fn folders_ok(file: &Path, args: &[&str]) -> String {
    let o = folders_run(file, args);
    assert!(o.status.success(), "quiet {args:?} failed: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

fn folders_json(file: &Path, args: &[&str]) -> serde_json::Value {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    serde_json::from_str(&folders_ok(file, &a)).unwrap()
}

#[test]
fn paths_start_at_the_top_folders() {
    let (work, personal, file) = two_folders(&[("Plan", PLAN), ("Budget", "Rent and food.")], &[("Plan", "# Garden\nPlant the tomatoes."), ("Recipes", "Bread: flour, water, salt.")]);
    // `ls` alone: the top-level folders; their paths are where every path starts.
    let tops = folders_json(&file, &["ls"]);
    let names: Vec<&str> = tops.as_array().unwrap().iter().map(|e| e["path"].as_str().unwrap()).collect();
    assert_eq!(names, ["Work", "Personal"]);
    assert!(tops.as_array().unwrap().iter().all(|e| e["top"] == true && e["documents"] == 2));
    let all = folders_json(&file, &["ls", "-r"]);
    let docs: Vec<&str> = all.as_array().unwrap().iter().filter(|e| e["kind"] != "folder").map(|e| e["path"].as_str().unwrap()).collect();
    assert_eq!(docs, ["Personal/Plan.xml", "Personal/Recipes.xml", "Work/Budget.xml", "Work/Plan.xml"]);
    assert!(folders_ok(&file, &["ls", "Work"]).contains("Work/Budget.xml"));
    let found = folders_json(&file, &["find", "flour"]);
    assert_eq!((found["total"].as_u64(), found["results"][0]["path"].as_str()), (Some(1), Some("Personal/Recipes.xml")));
    // The same title in both: ambiguous, each by its full path; the path says which.
    let o = folders_run(&file, &["read", "Plan"]);
    assert_eq!(o.status.code(), Some(3));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("error[ambiguous]") && err.contains("Work/Plan.xml") && err.contains("Personal/Plan.xml") && err.contains("full path"), "{err}");
    assert!(folders_ok(&file, &["read", "personal/Plan"]).contains("Plant the tomatoes"), "the folder's name in any case");
    assert!(folders_ok(&file, &["read", "Work/Plan.xml"]).contains("Ship the CLI"));
    assert!(folders_ok(&file, &["read", work.join("Plan.xml").to_str().unwrap()]).contains("Ship the CLI"), "a path on disk");
    assert!(folders_ok(&file, &["read", "Budget"]).contains("Rent"), "a title only one document has needs no folder");
    // By id, in whichever top-level folder it is; edits land in that folder's file.
    let id = all.as_array().unwrap().iter().find(|e| e["path"] == "Personal/Plan.xml").unwrap()["id"].as_str().unwrap().to_string();
    folders_ok(&file, &["insert", &id, "--md", "Water daily."]);
    assert!(std::fs::read_to_string(personal.join("Plan.xml")).unwrap().contains("Water daily."));
    assert!(!std::fs::read_to_string(work.join("Plan.xml")).unwrap().contains("Water daily."));
    // A top-level folder lists what is in it; a name no folder has is not_found.
    let only = folders_json(&file, &["ls", "Work"]);
    assert!(only.as_array().unwrap().iter().all(|e| e["path"].as_str().unwrap().starts_with("Work/")) && only.as_array().unwrap().len() == 2);
    let o = folders_run(&file, &["ls", "Attic"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[not_found]"));
    // --library: that folder on its own, paths starting in it.
    let lib = ok(&personal, &["ls"]);
    assert!(lib.contains("Recipes.xml") && !lib.contains("Personal/") && !lib.contains("Budget"), "{lib}");
}

#[test]
fn two_folders_of_one_name_are_told_apart_by_key() {
    let base = library(&[]).with_extension("twins");
    let _ = std::fs::remove_dir_all(&base);
    let (a, b) = (base.join("one/笔记"), base.join("two/笔记"));
    for (dir, title) in [(&a, "甲"), (&b, "乙")] {
        let mut store = Store::open_at(dir.clone()).unwrap();
        store.create_from(&Document { title: RichText::plain(title), ..Default::default() }).unwrap();
    }
    let file = base.join("folders.json");
    std::fs::write(&file, serde_json::json!({ "v": 1, "folders": [{ "key": "ka", "path": a }, { "key": "kb", "path": b }] }).to_string()).unwrap();
    let o = folders_run(&file, &["ls", "笔记"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("error[ambiguous]") && err.contains(a.to_str().unwrap()) && err.contains("key kb"), "{err}");
    assert!(folders_ok(&file, &["ls", "kb"]).contains("笔记/乙.xml"));
    assert_eq!(folders_json(&file, &["read", "ka/甲"])["title"], "甲");
}

#[test]
fn new_documents_land_in_one_folder() {
    let (work, personal, file) = two_folders(&[("Plan", PLAN)], &[("Recipes", "Bread.")]);
    // Nothing said: the first top-level folder, and the path says so.
    let out = folders_ok(&file, &["new", "Inbox", "--md", "first"]);
    assert!(out.contains("created Work/Inbox.xml"), "{out}");
    assert!(work.join("Inbox.xml").is_file() && !personal.join("Inbox.xml").exists());
    assert!(folders_ok(&file, &["new", "Shopping", "--in", "Personal"]).contains("created Personal/Shopping.xml"));
    assert!(personal.join("Shopping.xml").is_file());
    // Into a folder: by its full path, or by a name only one folder has.
    folders_ok(&file, &["mkdir", "Kitchen", "--in", "Personal"]);
    let out = folders_ok(&file, &["new", "Soup", "--in", "Personal/Kitchen"]);
    assert!(out.contains("Personal/Kitchen/Soup.xml") && personal.join("Kitchen/Soup.xml").is_file(), "{out}");
    assert!(folders_ok(&file, &["new", "Stew", "--in", "Kitchen"]).contains("Personal/Kitchen/Stew.xml"));
    // A folder that is not there is not_found, whichever top-level folder is named.
    let o = folders_run(&file, &["new", "Pie", "--in", "Work/Kitchen"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[not_found]"));
    assert!(!work.join("Kitchen").exists());
    let made = folders_json(&file, &["new", "Notes", "--dry-run"]);
    assert_eq!(made["path"], "Work/Notes.xml");
    // No moving between top-level folders yet; nor out beside the top-level folders.
    let o = folders_run(&file, &["mv", "Work/Plan", "Personal/Kitchen"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[cross_folder]"), "{}", String::from_utf8_lossy(&o.stderr));
    let o = folders_run(&file, &["mv", "Work/Plan", "/"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[bad_args]"));
    assert!(work.join("Plan.xml").is_file());
    // Each top-level folder's own 回收站: trashed in Personal, back in Personal.
    folders_ok(&file, &["rm", "Personal/Shopping"]);
    let trash = folders_json(&file, &["trash"]);
    assert_eq!(trash["items"][0]["folder"], "Personal");
    folders_ok(&file, &["untrash", "Shopping"]);
    assert!(personal.join("Shopping.xml").is_file());
}

#[test]
fn a_missing_folder_is_listed_and_the_rest_work() {
    let (work, personal, file) = two_folders(&[("Plan", PLAN)], &[("Recipes", "Bread.")]);
    std::fs::remove_dir_all(&work).unwrap();
    let list_json = folders_json(&file, &["ls"]);
    let list = list_json.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0]["path"].as_str(), list[0]["available"].as_bool()), (Some("Work"), Some(false)));
    assert!(list[0]["error"].as_str().unwrap().contains("not there"));
    assert_eq!((list[1]["path"].as_str(), list[1]["documents"].as_u64(), list[1]["default"].as_bool()), (Some("Personal"), Some(1), Some(true)));
    let text = folders_ok(&file, &["ls"]);
    assert!(text.contains("unavailable") && text.contains(&personal.display().to_string()), "{text}");
    assert!(folders_ok(&file, &["read", "Recipes"]).contains("Bread."));
    // New documents go to the first top-level folder that opens.
    folders_ok(&file, &["new", "Later"]);
    assert!(personal.join("Later.xml").is_file() && !work.exists(), "a missing top-level folder is never made anew");
    let o = folders_run(&file, &["ls", "Work"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[unavailable]"));
    // No top-level folders at all.
    std::fs::write(&file, r#"{"v":1,"folders":[]}"#).unwrap();
    let o = folders_run(&file, &["ls"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("error[no_library]"));
}

#[test]
fn wiki_links_look_in_their_own_folder_first() {
    let (dirs, file) = top_folders(&[
        ("Work", &[("Plan", "Ship it."), ("Notes", "Today.")]),
        ("Personal", &[("Plan", "Plant tomatoes."), ("Garden", "Beds."), ("Recipes", "Bread.")]),
        ("Attic", &[("Recipes", "Old ones.")]),
    ]);
    let links = |doc: &str| -> Vec<String> {
        folders_json(&file, &["links", doc])["out"].as_array().unwrap().iter().map(|l| l["to"].as_str().unwrap().to_string()).collect()
    };
    // The same title here and in another top-level folder: this folder's.
    folders_ok(&file, &["insert", "Notes", "--md", "See [[Plan]]."]);
    assert_eq!(links("Notes"), ["Work/Plan.xml"]);
    // Only in another top-level folder: that one.
    folders_ok(&file, &["insert", "Work/Notes", "--md", "And [[Garden]]."]);
    assert_eq!(links("Notes"), ["Work/Plan.xml", "Personal/Garden.xml"]);
    // A full path names exactly one.
    folders_ok(&file, &["insert", "Notes", "--md", "Also [[Attic/Recipes]]."]);
    assert_eq!(links("Notes"), ["Work/Plan.xml", "Personal/Garden.xml", "Attic/Recipes.xml"]);
    // In two other top-level folders: ambiguous, both by path and id, nothing written.
    let before = std::fs::read_to_string(dirs[0].join("Notes.xml")).unwrap();
    let o = folders_run(&file, &["insert", "Notes", "--md", "Bake from [[Recipes]]."]);
    assert_eq!(o.status.code(), Some(3));
    let err = String::from_utf8_lossy(&o.stderr);
    let ids: Vec<String> = folders_json(&file, &["ls", "-r"]).as_array().unwrap().iter().filter(|e| e["path"].as_str().unwrap().ends_with("/Recipes.xml")).map(|e| e["id"].as_str().unwrap().to_string()).collect();
    assert!(err.contains("error[ambiguous]") && err.contains("Personal/Recipes.xml (id ") && err.contains("Attic/Recipes.xml (id ") && ids.iter().all(|id| err.contains(id.as_str())) && err.contains("[["), "{err}");
    assert_eq!(std::fs::read_to_string(dirs[0].join("Notes.xml")).unwrap(), before);
    // An id names a document in any top-level folder.
    let personal_plan = folders_json(&file, &["ls", "Personal"]).as_array().unwrap().iter().find(|e| e["path"] == "Personal/Plan.xml").unwrap()["id"].as_str().unwrap().to_string();
    folders_ok(&file, &["insert", "Notes", "--md", &format!("The other [[{personal_plan}]].")]);
    // Read out and written back, the mentions point where they did: Personal's
    // Plan by id (`[[Plan]]` would be Work's), Garden by its path.
    let md = folders_json(&file, &["read", "Work/Notes"])["text"].as_str().unwrap().to_string();
    assert!(md.contains("[[Plan]]") && md.contains("[[Garden]]") && md.contains(&format!("[[{personal_plan}]]")), "{md}");
    // A new document's links look in the top-level folder it lands in.
    folders_ok(&file, &["new", "Copy", "--md", &md]);
    assert_eq!(links("Copy"), links("Notes"));
    folders_ok(&file, &["new", "Shelf", "--in", "Attic", "--md", "[[Recipes]] and [[Garden]]"]);
    assert_eq!(links("Shelf"), ["Attic/Recipes.xml", "Personal/Garden.xml"]);
    // Several in its own top-level folder: ambiguous as before, even with one elsewhere.
    for f in ["A", "B"] { folders_ok(&file, &["mkdir", f, "--in", "Attic"]); folders_ok(&file, &["new", "Box", "--in", &format!("Attic/{f}")]); }
    folders_ok(&file, &["new", "Box", "--in", "Personal"]);
    let o = folders_run(&file, &["insert", "Shelf", "--md", "[[Box]]"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("error[ambiguous]") && err.contains("Attic/A/Box.xml") && err.contains("Attic/B/Box.xml") && !err.contains("Personal/Box"), "{err}");
    // From Work, Box is only elsewhere, in two top-level folders.
    assert!(String::from_utf8_lossy(&folders_run(&file, &["insert", "Notes", "--md", "[[Box]]"]).stderr).contains("other top-level folders match"));
}

/// `quiet folders`: the sidebar's list in `folders.json`, written by the same code as the app's
/// 添加文件夹 / 从侧栏移除, with its checks.
#[test]
fn folders_add_list_and_remove() {
    let (_, file) = top_folders(&[("Work", &[("Plan", PLAN)])]);
    let base = file.parent().unwrap().to_path_buf();
    let notes = base.join("Notes");
    std::fs::create_dir_all(notes.join("sub")).unwrap();
    std::fs::write(notes.join("a.md"), "# A\n").unwrap();
    let list = folders_json(&file, &["folders"]);
    assert_eq!(list["folders"].as_array().unwrap().len(), 1);
    assert_eq!(list["folders"][0]["default"], true);
    // Dry run writes nothing.
    folders_ok(&file, &["folders", "add", notes.to_str().unwrap(), "--dry-run"]);
    assert!(!notes.join(".quiet").exists());
    let added = folders_json(&file, &["folders", "add", notes.to_str().unwrap()]);
    assert_eq!(added["added"]["name"], "Notes");
    assert!(notes.join(".quiet").is_dir(), "opened where it is");
    let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(written["folders"].as_array().unwrap().len(), 2);
    assert!(folders_ok(&file, &["ls", "Notes"]).contains("a.md"), "its documents are listed at once");
    // The app's checks: open already, not there, a disk's top or the home folder (with --dry-run: nothing is written even if it passed).
    let err = |args: &[&str]| { let o = folders_run(&file, args); assert!(!o.status.success(), "{args:?}"); String::from_utf8_lossy(&o.stderr).into_owned() };
    assert!(err(&["folders", "add", notes.to_str().unwrap()]).starts_with("error[already_open]"));
    assert!(err(&["folders", "add", base.join("nope").to_str().unwrap()]).starts_with("error[not_found]"));
    assert!(err(&["folders", "add", "/", "--dry-run"]).starts_with("error[too_broad]"));
    if let Some(home) = std::env::var_os("HOME") { assert!(err(&["folders", "add", home.to_str().unwrap(), "--dry-run"]).starts_with("error[too_broad]")); }
    if cfg!(target_os = "macos") { assert!(err(&["folders", "add", "/usr", "--dry-run"]).starts_with("error[too_broad]")); }
    // Inside an open one, or holding one: added, a view of its own of the same files.
    let sub = notes.join("sub");
    std::fs::write(sub.join("b.md"), "# B\n").unwrap();
    assert_eq!(folders_json(&file, &["folders", "add", sub.to_str().unwrap()])["added"]["name"], "sub");
    assert!(sub.join(".quiet").is_dir() && folders_ok(&file, &["ls", "sub"]).contains("b.md") && folders_ok(&file, &["ls", "Notes/sub"]).contains("b.md"));
    assert!(folders_ok(&file, &["folders", "add", base.to_str().unwrap(), "--dry-run", "--force"]).starts_with("would add"));
    folders_ok(&file, &["folders", "remove", sub.to_str().unwrap()]);
    // A code project asks first; --force adds it.
    let code = base.join("code");
    std::fs::create_dir_all(&code).unwrap();
    std::fs::write(code.join("Cargo.toml"), "[package]\n").unwrap();
    assert!(err(&["folders", "add", code.to_str().unwrap()]).starts_with("error[confirm]"));
    folders_ok(&file, &["folders", "add", code.to_str().unwrap(), "--force"]);
    // Removed by name, path or key: off the list, every file where it was.
    folders_ok(&file, &["folders", "remove", "notes"]);
    folders_ok(&file, &["folders", "rm", code.to_str().unwrap()]);
    assert!(err(&["folders", "remove", "Notes"]).starts_with("error[not_found]"));
    let left = folders_json(&file, &["folders"]);
    assert_eq!(left["folders"].as_array().unwrap().len(), 1);
    assert_eq!(left["folders"][0]["name"], "Work");
    assert!(notes.join("a.md").is_file() && notes.join(".quiet").is_dir() && code.join("Cargo.toml").is_file());
}

/// `quiet open`: the link handed to the system names the document, its block (pinned), a
/// folder or the trash; `new --open` opens what it made.
#[test]
fn open_hands_over_a_link() {
    let (_, file) = top_folders(&[("Work", &[("Plan", PLAN)])]);
    let caught = file.with_file_name("links.txt");
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_quiet")).env("QUIET_NO_TELEMETRY", "1").env_remove("QUIET_MD_HOME").env("QUIET_MD_FOLDERS", &file).env("QUIET_MD_OPEN_LOG", &caught).args(args).output().unwrap();
    let ok = |args: &[&str]| { let o = run(args); assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr)); serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap() };
    let parse = |v: &serde_json::Value| fdl_core::app_url::Target::parse(v["url"].as_str().unwrap()).unwrap();
    let to_doc = ok(&["--json", "open", "Work/Plan#Tasks", "--dry-run"]);
    assert_eq!(to_doc["launched"], false);
    let fdl_core::app_url::Target::Doc { top, block, .. } = parse(&to_doc) else { panic!("{to_doc}") };
    assert_eq!((top.as_deref(), block.as_deref().map(|b| b.split('~').next().unwrap())), (Some("k0"), Some("#3")));
    assert!(!caught.exists(), "a dry run hands nothing over");
    let launched = ok(&["--json", "open", "Plan#3"]);
    assert_eq!(launched["launched"], true);
    assert_eq!(std::fs::read_to_string(&caught).unwrap().trim(), launched["url"].as_str().unwrap());
    assert_eq!(parse(&ok(&["--json", "open", "Work", "--dry-run"])), fdl_core::app_url::Target::Folder { top: "k0".into(), folder: None });
    assert_eq!(parse(&ok(&["--json", "open", "--trash", "--dry-run"])), fdl_core::app_url::Target::Trash);
    // Clear refusals.
    for (args, code) in [(&["open", "Nope"][..], "not_found"), (&["open", "Plan#no such heading"][..], "bad_ref"), (&["open"][..], "usage")] {
        let o = run(args);
        assert!(String::from_utf8_lossy(&o.stderr).starts_with(&format!("error[{code}]")), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }
    // `new --open`: written, then opened.
    let o = run(&["new", "Fresh", "--in", "Work", "--open"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let links = std::fs::read_to_string(&caught).unwrap();
    assert_eq!(links.lines().count(), 2);
    assert!(matches!(fdl_core::app_url::Target::parse(links.lines().last().unwrap()), Ok(fdl_core::app_url::Target::Doc { .. })));
}

/// `load` is the operation lib.doc uses; `open`, its old name, still works for now and says so.
#[test]
fn load_is_the_operation_and_open_its_old_name() {
    let lib = library(&[("Plan", PLAN)]);
    let run = |args: &[&str]| quiet(&lib, args);
    let loaded = run(&["--json", "call", "load", r#"{"doc": "Plan"}"#]);
    assert!(loaded.status.success() && loaded.stderr.is_empty(), "{}", String::from_utf8_lossy(&loaded.stderr));
    let old = run(&["--json", "call", "open", r#"{"doc": "Plan"}"#]);
    assert!(old.status.success());
    assert_eq!(old.stdout, loaded.stdout);
    assert!(String::from_utf8_lossy(&old.stderr).contains("\"load\""), "{}", String::from_utf8_lossy(&old.stderr));
    let script = run(&["run", "-e", "return lib.doc('Plan').title"]);
    assert!(script.status.success() && script.stderr.is_empty(), "{}", String::from_utf8_lossy(&script.stderr));
    let help = String::from_utf8_lossy(&run(&["help", "call"]).stdout).to_string();
    assert!(help.contains(" load ") && !help.contains(" open "), "{help}");
}

/// 置顶 and 创建副本, as the document's menu does them.
#[test]
fn pins_and_duplicates() {
    let lib = library(&[("Plan", PLAN)]);
    let entry = |lib: &Path, title: &str| json(lib, &["ls"]).as_array().unwrap().iter().find(|e| e["title"] == title).cloned().unwrap_or_default();
    ok(&lib, &["pin", "Plan", "--dry-run"]);
    assert_ne!(entry(&lib, "Plan")["pinned"], true, "a dry run writes nothing");
    ok(&lib, &["pin", "Plan"]);
    assert_eq!(entry(&lib, "Plan")["pinned"], true);
    ok(&lib, &["unpin", "Plan"]);
    assert_ne!(entry(&lib, "Plan")["pinned"], true);
    assert!(ok(&lib, &["duplicate", "Plan"]).contains("duplicated"));
    assert!(ok(&lib, &["read", "Plan 副本"]).contains("Ship the CLI."), "the copy holds the same text");
    ok(&lib, &["duplicate", "Plan", "Template"]);
    assert!(entry(&lib, "Template").is_object());
    let o = script(&lib, "const d = lib.doc('Plan'); d.pin(); d.duplicate('Copy 2');", &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(entry(&lib, "Plan")["pinned"], true);
    assert!(entry(&lib, "Copy 2").is_object());
}
