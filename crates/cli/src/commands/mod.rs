//! Command module exports & common library opening/input helpers.

pub mod asset;
pub mod block;
pub mod board;
pub mod doc;
pub mod export;
pub mod folder;
pub mod library;
pub mod meta;
pub mod schema;
pub mod table;
pub mod theme;
pub mod validate;

use crate::args::ParsedCmd;
use crate::lint::lint_document;
use crate::locator;
use crate::output::{CliFailure, Diagnostic, GlobalOpts};
use crate::spec::BlockSpec;
use fdl_core::editor::core::{Core, DropPlace, validate_core};
use fdl_core::model::Document;
use fdl_core::store::{DocKind, Entry, Store};
use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;

pub fn resolve_library_root(opts: &GlobalOpts) -> PathBuf {
    opts.lib.clone().unwrap_or_else(Store::root_dir)
}

pub fn open_store(opts: &GlobalOpts) -> Result<Store, CliFailure> {
    let root = resolve_library_root(opts);
    if let Some(lib) = &opts.lib {
        // Point Store::root_dir() at --lib if needed, without altering global library.json
        // when QUIET_MD_HOME isn't set: we open directly with Store::open_at.
        let _ = lib;
    }
    let store = Store::open_at(root).map_err(|e| CliFailure::new("LIBRARY_OPEN_FAILED", format!("{e:#}")))?;
    Ok(store)
}

pub fn read_arg_or_stdin(spec_arg: &str) -> Result<String, CliFailure> {
    if spec_arg == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map_err(|e| CliFailure::new("STDIN_READ_ERROR", e.to_string()))?;
        return Ok(s);
    }
    if spec_arg.trim_start().starts_with('{') || spec_arg.trim_start().starts_with('[') || spec_arg.trim_start().starts_with('<') {
        return Ok(spec_arg.to_string());
    }
    std::fs::read_to_string(spec_arg).map_err(|e| CliFailure::new("FILE_READ_ERROR", format!("Failed reading '{spec_arg}': {e}")))
}

pub fn parse_json_arg<T: serde::de::DeserializeOwned>(arg: &str) -> Result<T, CliFailure> {
    let raw = read_arg_or_stdin(arg)?;
    serde_json::from_str::<T>(&raw).map_err(|e| {
        CliFailure::new("INVALID_JSON", format!("Failed parsing JSON: {e}"))
            .with_hint("Run `qmd schema doc-spec --json` or `qmd schema board-spec --json` to inspect valid JSON fields.")
    })
}

pub fn check_if_unmodified(store: &Store, doc_id: &str, cmd: &ParsedCmd) -> Result<(), CliFailure> {
    let Some(expected) = cmd.flag("if-unmodified") else { return Ok(()); };
    let expected: u64 = expected.parse().map_err(|_| CliFailure::usage("--if-unmodified requires unix seconds"))?;
    if let Some(actual) = store.doc_mtime_secs(doc_id) && actual != expected {
        return Err(CliFailure::new(
            "MODIFIED_EXTERNALLY",
            format!("Document '{doc_id}' was modified externally (mtime {actual} != expected {expected})."),
        ).with_hint("Re-read the document with `qmd doc get` before applying changes."));
    }
    Ok(())
}

pub fn is_board_entry(e: &Entry) -> bool {
    e.meta.kind == DocKind::Board || e.meta.rel.ends_with(".excalidraw")
}

pub fn is_markdown_entry(e: &Entry) -> bool {
    e.meta.markdown || e.meta.rel.ends_with(".md")
}

pub fn known_doc_ids(store: &mut Store) -> HashSet<String> {
    store.list().into_iter().filter(|e| e.meta.folder.is_none()).map(|e| e.id).collect()
}

pub fn load_doc(store: &mut Store, target: &str) -> Result<(Entry, Document), CliFailure> {
    let entry = locator::resolve_doc(store, target)?;
    if is_board_entry(&entry) {
        return Err(CliFailure::new(
            "IS_WHITEBOARD",
            format!("'{}' is a standalone .excalidraw whiteboard, not a structured document.", entry.title),
        ).with_hint("Use `qmd board inspect` / `qmd board export-svg` / `qmd board render-png` instead."));
    }
    let doc = store.load(&entry.id).map_err(|e| CliFailure::new("DOC_LOAD_FAILED", format!("{e:#}")))?;
    Ok((entry, doc))
}

pub fn parse_place(s: Option<&str>) -> Result<DropPlace, CliFailure> {
    match s.unwrap_or("after").trim().to_ascii_lowercase().as_str() {
        "after" => Ok(DropPlace::After),
        "before" => Ok(DropPlace::Before),
        "first-child" | "first_child" | "child" => Ok(DropPlace::FirstChild),
        other => Err(CliFailure::new("INVALID_PLACE", format!("Unknown --place '{other}'."))
            .with_valid(["after", "before", "first-child"])),
    }
}

pub fn commit_core(store: &mut Store, entry: &Entry, core: &Core, dry_run: bool) -> Result<Vec<Diagnostic>, CliFailure> {
    validate_core(core).map_err(|e| CliFailure::new("CORE_INVARIANT_ERROR", e))?;
    let ids = known_doc_ids(store);
    let diags = lint_document(&core.doc, Some(&entry.meta), store, &ids);
    if !dry_run {
        store.save(&entry.id, &core.doc).map_err(|e| CliFailure::new("DOC_SAVE_FAILED", format!("{e:#}")))?;
    }
    Ok(diags)
}

pub fn opt_clear_flag(val: Option<&str>) -> Option<Option<&str>> {
    val.map(|s| if matches!(s.trim().to_ascii_lowercase().as_str(), "" | "none" | "null" | "clear") { None } else { Some(s.trim()) })
}

pub fn parse_block_specs_arg(arg: &str) -> Result<Vec<BlockSpec>, CliFailure> {
    let v: serde_json::Value = parse_json_arg(arg)?;
    if v.is_array() {
        serde_json::from_value(v).map_err(|e| CliFailure::new("INVALID_SPEC", format!("Invalid BlockSpec array: {e}")))
    } else {
        let one: BlockSpec = serde_json::from_value(v).map_err(|e| CliFailure::new("INVALID_SPEC", format!("Invalid BlockSpec: {e}")))?;
        Ok(vec![one])
    }
}

pub fn write_file_bytes(path: &std::path::Path, bytes: &[u8], dry_run: bool) -> Result<(), CliFailure> {
    if dry_run { return Ok(()); }
    if let Some(p) = path.parent() && !p.as_os_str().is_empty() {
        std::fs::create_dir_all(p).map_err(|e| CliFailure::new("WRITE_FAILED", format!("Failed creating {}: {e}", p.display())))?;
    }
    std::fs::write(path, bytes).map_err(|e| CliFailure::new("WRITE_FAILED", format!("Failed writing {}: {e}", path.display())))
}
