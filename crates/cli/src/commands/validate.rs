//! `qmd validate` — run Core invariants + aesthetic layout linter on docs or the full library.

use super::{is_board_entry, known_doc_ids};
use crate::args::ParsedCmd;
use crate::board_builder;
use crate::lint::lint_document;
use crate::locator;
use crate::output::{CliFailure, Diagnostic, GlobalOpts, print_success};
use fdl_core::whiteboard::Board;

fn count_diags(diags: &[Diagnostic]) -> (usize, usize, usize) {
    let errors = diags.iter().filter(|d| d.severity == "error").count();
    let warnings = diags.iter().filter(|d| d.severity == "warning").count();
    let hints = diags.iter().filter(|d| d.severity == "hint").count();
    (errors, warnings, hints)
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut store = super::open_store(opts)?;
    let root = store.root().to_path_buf();
    let strict = cmd.bool_flag("strict")?.unwrap_or(false);
    let ids = known_doc_ids(&mut store);

    match cmd.command_path.as_str() {
        "validate doc" => {
            let entry = locator::resolve_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let diags = if is_board_entry(&entry) {
                match board_builder::load_board_file(&store.abs_path(&entry.meta.rel)) {
                    Ok(bd) => match Board::from_scene(&bd.scene_json(), bd.height) {
                        Some(_) => vec![],
                        None => vec![Diagnostic {
                            severity: "error", rule: "board-roundtrip".into(), loc: None,
                            message: "Whiteboard failed scene round-trip check.".into(), fix_hint: None,
                        }],
                    },
                    Err(f) => vec![Diagnostic {
                        severity: "error", rule: "board-parse".into(), loc: None,
                        message: f.0.error.message, fix_hint: None,
                    }],
                }
            } else {
                let doc = store.load(&entry.id).map_err(|e| CliFailure::new("DOC_LOAD_FAILED", format!("{e:#}")))?;
                lint_document(&doc, Some(&entry.meta), &store, &ids)
            };
            let (errors, warnings, hints) = count_diags(&diags);
            if errors > 0 || (strict && !diags.is_empty()) {
                return Err(CliFailure::new(
                    "VALIDATION_FAILED",
                    format!("Validation failed for '{}': {errors} error(s), {warnings} warning(s), {hints} hint(s).", entry.title),
                ).with_diagnostics(diags));
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "title": entry.title, "errors": errors, "warnings": warnings, "hints": hints }),
                &store.warnings, &diags,
                &format!("Validated '{}': {errors} error(s), {warnings} warning(s), {hints} hint(s)", entry.title),
            );
            Ok(())
        }
        "validate library" => {
            let entries = store.list();
            let mut docs_checked = 0usize;
            let mut boards_checked = 0usize;
            let mut all_diags = vec![];
            let mut results = vec![];

            for entry in &entries {
                if entry.meta.folder.is_some() { continue; }
                let diags = if is_board_entry(entry) {
                    boards_checked += 1;
                    match board_builder::load_board_file(&store.abs_path(&entry.meta.rel)) {
                        Ok(_) => vec![],
                        Err(f) => vec![Diagnostic {
                            severity: "error", rule: "board-parse".into(), loc: Some(entry.meta.rel.clone()),
                            message: format!("{}: {}", entry.title, f.0.error.message), fix_hint: None,
                        }],
                    }
                } else {
                    docs_checked += 1;
                    match store.load(&entry.id) {
                        Ok(doc) => lint_document(&doc, Some(&entry.meta), &store, &ids),
                        Err(e) => vec![Diagnostic {
                            severity: "error", rule: "doc-load".into(), loc: Some(entry.meta.rel.clone()),
                            message: format!("{}: {e:#}", entry.title), fix_hint: None,
                        }],
                    }
                };
                let (e, w, h) = count_diags(&diags);
                results.push(serde_json::json!({
                    "id": entry.id, "title": entry.title, "rel": entry.meta.rel,
                    "errors": e, "warnings": w, "hints": h,
                }));
                for mut d in diags {
                    let prefix = match &d.loc {
                        Some(l) => format!("{}:{}", entry.meta.rel, l),
                        None => entry.meta.rel.clone(),
                    };
                    d.loc = Some(prefix);
                    all_diags.push(d);
                }
            }
            let (errors, warnings, hints) = count_diags(&all_diags);
            if errors > 0 || (strict && !all_diags.is_empty()) {
                return Err(CliFailure::new(
                    "LIBRARY_VALIDATION_FAILED",
                    format!("Library validation failed: {errors} error(s), {warnings} warning(s), {hints} hint(s) across {docs_checked} doc(s) and {boards_checked} board(s)."),
                ).with_diagnostics(all_diags));
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "docs_checked": docs_checked,
                    "boards_checked": boards_checked,
                    "errors": errors,
                    "warnings": warnings,
                    "hints": hints,
                    "results": results,
                }),
                &store.warnings, &all_diags,
                &format!("Validated {docs_checked} doc(s), {boards_checked} board(s): {errors} error(s), {warnings} warning(s), {hints} hint(s)"),
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled validate command {}", cmd.command_path))),
    }
}
