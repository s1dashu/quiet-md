//! `qmd folder` — create, rename, move, delete.

use super::open_store;
use crate::args::ParsedCmd;
use crate::locator;
use crate::output::{CliFailure, GlobalOpts, print_success};

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut store = open_store(opts)?;
    let root = store.root().to_path_buf();
    match cmd.command_path.as_str() {
        "folder create" => {
            let title = cmd.pos(0, "<title>")?;
            let parent = locator::resolve_optional_folder(&mut store, cmd.flag("parent"))?;
            let id = if opts.dry_run {
                "dry-run-folder-id".to_string()
            } else {
                store.create_folder(title, parent.as_deref())?
            };
            let rel = if opts.dry_run {
                title.to_string()
            } else {
                store.list().into_iter().find(|e| e.id == id).map(|e| e.meta.rel).unwrap_or_default()
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": id, "title": title, "rel": rel, "parent": parent }),
                &store.warnings, &[],
                &format!("Created folder '{title}' ({id})"),
            );
            Ok(())
        }
        "folder rename" => {
            let folder = locator::resolve_folder(&mut store, cmd.pos(0, "<folder>")?)?;
            let title = cmd.pos(1, "<title>")?;
            if !opts.dry_run {
                store.rename(&folder.id, title)?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": folder.id, "title": title }),
                &store.warnings, &[],
                &format!("Renamed folder '{}' -> '{title}'", folder.title),
            );
            Ok(())
        }
        "folder move" => {
            let folder = locator::resolve_folder(&mut store, cmd.pos(0, "<folder>")?)?;
            let parent = match cmd.flag("parent") {
                Some(p) => locator::resolve_optional_folder(&mut store, Some(p))?,
                None => folder.meta.parent.clone(),
            };
            let before = match cmd.flag("before") {
                Some(b) => {
                    let entries = store.list();
                    let found = entries.iter().find(|e| e.id == b || e.meta.rel.trim_end_matches('/') == b.trim_matches('/') || e.title == b)
                        .ok_or_else(|| CliFailure::new("SIBLING_NOT_FOUND", format!("Sibling '{b}' not found.")))?;
                    Some(found.id.clone())
                }
                None => None,
            };
            if !opts.dry_run {
                store.move_in_tree(&folder.id, parent.as_deref(), before.as_deref())?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": folder.id, "parent": parent, "before": before }),
                &store.warnings, &[],
                &format!("Moved folder '{}'", folder.title),
            );
            Ok(())
        }
        "folder delete" => {
            let folder = locator::resolve_folder(&mut store, cmd.pos(0, "<folder>")?)?;
            if !opts.dry_run {
                store.delete(&folder.id, None)?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": folder.id, "trashed": true }),
                &store.warnings, &[],
                &format!("Moved folder '{}' to Trash", folder.title),
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled folder command {}", cmd.command_path))),
    }
}
