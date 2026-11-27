//! What the CLI asks of the app rather than of the library: which folders the
//! sidebar lists (`quiet folders`, the same `folders.json` and the same
//! `TopFolders` code as 添加文件夹 / 从侧栏移除; a running app follows the file), and
//! showing a document or folder there (`quiet open`, a `quietmd://` link the
//! system hands to the app, launching it if need be).

use std::path::{Path, PathBuf};

use fdl_core::app_url::Target;
use fdl_core::top_folders::{AddError, TopFolders, TopFolder};
use fdl_core::shown::Survey;
use fdl_core::store::INTERNAL;
use serde_json::{Value, json};

use crate::fail::{R, fail};
use crate::session::{Session, arg};
use crate::{Args, blocks};

fn canonical(p: &Path) -> PathBuf { fdl_core::top_folders::plain(p.canonicalize().unwrap_or_else(|_| p.to_path_buf())) }

/// The top-level folders as the app keeps them, opened as the CLI opens them (nothing
/// migrated, a missing folder listed as such); fails for a run with a fixed list.
fn top_folders(writing: bool) -> R<TopFolders> {
    let (entries, fixed) = TopFolders::entries();
    if fixed && writing {
        return Err(fail("fixed", "this run's folder list is fixed (QUIET_MD_HOME is set)").hint("unset QUIET_MD_HOME to change the folders Quiet.md lists on this computer"));
    }
    Ok(TopFolders::open_with(entries, fixed, crate::session::open_top))
}

fn folder_json(r: &TopFolder, default: bool) -> Value {
    json!({ "name": r.name(), "path": r.path, "key": r.key, "default": default, "available": r.error.is_none(), "error": r.error })
}

/// `quiet folders [add PATH | remove NAME]`.
pub fn folders(a: &Args, out: &mut String) -> R<()> {
    let json_out = a.has("json");
    let mut pos = a.pos.iter().map(String::as_str);
    match pos.next() {
        None | Some("ls" | "list") => {
            let tops = top_folders(false)?;
            let first = tops.first().map(|r| r.key.clone());
            let list: Vec<Value> = tops.folders.iter().map(|r| folder_json(r, first.as_deref() == Some(&r.key))).collect();
            if json_out { *out += &format!("{}\n", json!({ "folders": list })); return Ok(()); }
            if list.is_empty() { *out += "no folders yet: `quiet folders add PATH` adds one\n"; }
            for r in &tops.folders {
                let mut line = format!("{}\t{}\tkey {}", r.name(), r.path.display(), r.key);
                if first.as_deref() == Some(&r.key) { line += "\tdefault"; }
                if let Some(e) = &r.error { line += &format!("\tunavailable: {e}"); }
                *out += &format!("{line}\n");
            }
            Ok(())
        }
        Some("add") => {
            let raw = pos.next().ok_or_else(|| fail("usage", "folders add needs a folder: quiet folders add PATH"))?;
            let dir = std::path::absolute(raw).map_err(|e| fail("bad_args", format!("{raw}: {e}")))?;
            if !dir.is_dir() { return Err(fail("not_found", format!("{} is not a folder", dir.display())).hint("make it first (mkdir), or check the path")); }
            let mut tops = top_folders(true)?;
            let name = |tops: &TopFolders, k: &str| tops.folder(k).map(|r| format!("{} ({})", r.name(), r.path.display())).unwrap_or_default();
            // Inside an open top-level folder or holding one is fine: a view of its own of the same files.
            match tops.check(&dir) {
                Ok(_) => {}
                Err(AddError::Open(k)) => return Err(fail("already_open", format!("{} is already open", name(&tops, &k))).hint("`quiet folders` lists what is open")),
                Err(AddError::TooBroad) => return Err(fail("too_broad", format!("{} is a disk's top or the home folder", dir.display())).hint("add Documents, Desktop and other folders one at a time")),
                Err(AddError::Other(e)) => return Err(fail("io", e)),
            }
            if fdl_core::top_folders::too_broad(&dir) {
                return Err(fail("too_broad", format!("{} is a system folder", dir.display())).hint("add the folder your notes are in, e.g. ~/Documents/Notes"));
            }
            // As 添加文件夹 asks first about a code project or a very big folder: here --force says yes.
            if !dir.join(INTERNAL).exists() && !a.has("force") {
                let why = match fdl_core::shown::survey(&dir) { Survey::Plain => None, Survey::Project => Some("looks like a code project"), Survey::Big => Some("holds a great many files") };
                if let Some(why) = why {
                    return Err(fail("confirm", format!("{} {why}; Quiet.md shows only its Markdown and Quiet.md documents, skips dependencies, build output and what .gitignore ignores, and adds a hidden .quiet folder", dir.display())).hint("add --force to add it anyway"));
                }
            }
            if a.has("dry-run") {
                *out += &if json_out { format!("{}\n", json!({ "path": dir, "added": false, "dry_run": true })) } else { format!("would add {} (dry run: nothing written)\n", dir.display()) };
                return Ok(());
            }
            let key = tops.add(&dir).map_err(|e| fail("io", e.to_string()))?;
            let r = tops.folder(&key).unwrap();
            let first = tops.first().is_some_and(|f| f.key == key);
            *out += &if json_out { format!("{}\n", json!({ "added": folder_json(r, first) })) } else { format!("added {}\t{}\tkey {key}\n(used where it is, not copied; Quiet.md shows it in the sidebar)\n", r.name(), r.path.display()) };
            Ok(())
        }
        Some("remove" | "rm") => {
            let spec = pos.next().ok_or_else(|| fail("usage", "folders remove needs a folder: its name, path or key (`quiet folders`)"))?;
            let mut tops = top_folders(true)?;
            let path = std::path::absolute(spec).ok().filter(|p| p.exists()).map(|p| canonical(&p));
            let mut hits: Vec<&TopFolder> = tops.folders.iter().filter(|r| r.key == spec || path.as_ref().is_some_and(|p| *p == canonical(&r.path))).collect();
            if hits.is_empty() { hits = tops.folders.iter().filter(|r| r.name().to_lowercase() == spec.trim_end_matches(['/', '\\']).to_lowercase()).collect(); }
            let r = match hits.as_slice() {
                [r] => (r.key.clone(), r.name(), r.path.clone()),
                [] => return Err(fail("not_found", format!("no top-level folder {spec:?}")).hint("`quiet folders` lists them with their paths and keys")),
                many => return Err(fail("ambiguous", format!("{} folders are called {spec:?}: {}", many.len(), many.iter().map(|r| format!("{} (key {})", r.path.display(), r.key)).collect::<Vec<_>>().join(", "))).hint("pass its path or key instead")),
            };
            if a.has("dry-run") {
                *out += &if json_out { format!("{}\n", json!({ "key": r.0, "removed": false, "dry_run": true })) } else { format!("would remove {} from the sidebar (dry run: nothing written)\n", r.1) };
                return Ok(());
            }
            // The folder's view state stays as the app left it: only the list changes.
            tops.remove(&r.0).map_err(|e| fail("io", format!("{e:#}")))?;
            *out += &if json_out { format!("{}\n", json!({ "removed": { "name": r.1, "path": r.2, "key": r.0 } })) } else { format!("removed {} from the sidebar; its files stay in {}\n", r.1, r.2.display()) };
            Ok(())
        }
        Some(x) => Err(fail("usage", format!("unknown folders command {x:?}")).hint("quiet folders [add PATH | remove NAME|PATH|KEY]")),
    }
}

/// Top-level folder `key` of session `s` as the app knows it: with `--library DIR`, the
/// key the app lists that folder under.
fn app_key(s: &Session, key: &str) -> R<String> {
    if !s.alone { return Ok(key.to_string()); }
    let dir = canonical(&s.top_dir(key));
    TopFolders::entries().0.into_iter().find(|e| canonical(&e.path) == dir).map(|e| e.key)
        .ok_or_else(|| fail("not_open", format!("{} is not open in Quiet.md", dir.display())).hint(format!("`quiet folders add {}` adds it to the sidebar", dir.display())))
}

/// `quiet open DOC[#REF]` / `quiet open --folder FOLDER`: the app shows it.
pub fn open(s: &mut Session, a: &Args, out: &mut String) -> R<()> {
    let (target, shown) = match (a.get("folder"), a.pos.first()) {
        _ if a.has("trash") => { if a.get("folder").is_some() || !a.pos.is_empty() { return Err(fail("usage", "quiet open --trash takes nothing else")); } (Target::Trash, "the trash".to_string()) }
        (Some(f), None) => folder_target(s, f)?,
        (None, Some(spec)) => match doc_target(s, spec) {
            // Not a document: a folder of that name or path, when one is.
            Err(e) if matches!(e.code.as_ref(), "not_found" | "not_a_document") && !spec.contains('#') => folder_target(s, spec).map_err(|_| e)?,
            r => r?,
        },
        (Some(_), Some(_)) => return Err(fail("usage", "quiet open DOC[#REF], or quiet open --folder FOLDER, not both")),
        (None, None) => return Err(fail("usage", "missing a document: quiet open DOC[#REF], --folder FOLDER or --trash")),
    };
    let url = target.to_url();
    let launched = !a.has("dry-run");
    if launched { launch(&url)?; }
    *out += &if a.has("json") { format!("{}\n", json!({ "open": shown, "url": url, "launched": launched })) }
        else { format!("{} {shown} in Quiet.md\n{url}\n", if launched { "opening" } else { "would open" }) };
    Ok(())
}

fn doc_target(s: &mut Session, spec: &str) -> R<(Target, String)> {
    let a = s.split_doc(&json!({ "doc": spec }))?;
    let i = s.doc_arg(&a)?;
    let d = &s.docs[i];
    let id = d.id.clone().ok_or_else(|| fail("not_found", "not saved yet"))?;
    let block = match arg(&a, "at") {
        Some(at) => Some(blocks::pin_of(&d.doc, blocks::resolve(&d.doc, at)?).ok_or_else(|| fail("bad_ref", format!("no block {at}")))?),
        None => None,
    };
    let shown = match &block { Some(b) => format!("{}{b}", d.path), None => d.path.clone() };
    Ok((Target::Doc { top: Some(app_key(s, &d.top.clone())?), doc: id, block }, shown))
}

fn folder_target(s: &Session, spec: &str) -> R<(Target, String)> {
    let (key, folder) = s.locate(Some(spec))?.ok_or_else(|| fail("usage", "主页 itself has no page to open: name a folder"))?;
    let shown = match &folder { Some(id) => s.entry_by_id(id).map(|e| s.label(e)).unwrap_or_default(), None => s.top_name(&key) };
    Ok((Target::Folder { top: app_key(s, &key)?, folder }, shown))
}

/// Hand `url` to the system, which passes it to Quiet.md (launching it when it isn't running).
fn launch(url: &str) -> R<()> {
    // Tests: the link is written down in that file instead of handed over.
    if let Some(p) = std::env::var_os("QUIET_MD_OPEN_LOG") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| fail("launch", e.to_string()))?;
        return writeln!(f, "{url}").map_err(|e| fail("launch", e.to_string()));
    }
    let mut cmd = if cfg!(target_os = "macos") { std::process::Command::new("open") }
        // `start` is cmd's, and cmd reads `&` in the link as a second command: the URL handler instead.
        else if cfg!(windows) { let mut c = std::process::Command::new("rundll32"); c.arg("url.dll,FileProtocolHandler"); c }
        else { std::process::Command::new("xdg-open") };
    let o = cmd.arg(url).output().map_err(|e| fail("launch", format!("could not hand the link to the system: {e}")))?;
    if !o.status.success() {
        return Err(fail("launch", format!("the system could not open the link: {}", String::from_utf8_lossy(&o.stderr).trim())).hint("is Quiet.md installed? Its installer registers quietmd:// links"));
    }
    Ok(())
}
