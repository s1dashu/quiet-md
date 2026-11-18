//! `qmd asset` — import and verify images & file attachments in `.assets/`.

use super::is_board_entry;
use crate::args::ParsedCmd;
use crate::assets;
use crate::output::{CliFailure, GlobalOpts, print_success};
use fdl_core::model::{BlockKind, Inline};
use fdl_core::store;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(String, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return; };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files(base, &p, out);
        } else if p.is_file() {
            let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((rel, bytes));
        }
    }
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut st = super::open_store(opts)?;
    let root = st.root().to_path_buf();
    match cmd.command_path.as_str() {
        "asset add-image" => {
            let file = Path::new(cmd.pos(0, "<file>")?);
            let (rel, w, h) = assets::store_image_file(&st, file, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "src": rel, "width": w, "height": h }),
                &st.warnings, &[],
                &format!("Imported image -> {rel} ({w}x{h})"),
            );
            Ok(())
        }
        "asset add-file" => {
            let file = Path::new(cmd.pos(0, "<file>")?);
            let (rel, name) = assets::store_attachment_file(&st, file, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "src": rel, "name": name }),
                &st.warnings, &[],
                &format!("Imported attachment '{name}' -> {rel}"),
            );
            Ok(())
        }
        "asset list" => {
            let assets_dir = root.join(".assets");
            let mut found = vec![];
            collect_files(&root, &assets_dir, &mut found);
            found.sort_by(|a, b| a.0.cmp(&b.0));
            let items: Vec<_> = found.iter().map(|(rel, bytes)| {
                let kind = if rel.starts_with(".assets/files/") { "file" } else { "image" };
                serde_json::json!({ "src": rel, "bytes": bytes, "kind": kind })
            }).collect();
            let human = if found.is_empty() {
                "No assets in .assets/.".into()
            } else {
                found.iter().map(|(r, b)| format!("{r}  ({b} bytes)")).collect::<Vec<_>>().join("\n")
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "count": items.len(), "assets": items }),
                &st.warnings, &[], &human,
            );
            Ok(())
        }
        "asset verify" => {
            let mut missing = vec![];
            let mut referenced_abs: HashSet<PathBuf> = HashSet::new();
            let mut remote = vec![];

            for entry in st.list() {
                if entry.meta.folder.is_some() || is_board_entry(&entry) { continue; }
                let Ok(doc) = st.load(&entry.id) else { continue; };
                if let Some(cov) = &doc.cover {
                    match store::image_path_in(&root, &cov.src) {
                        Some(p) if p.exists() => { referenced_abs.insert(p); }
                        _ => missing.push(serde_json::json!({ "doc_id": entry.id, "doc_title": entry.title, "kind": "cover", "src": cov.src })),
                    }
                }
                doc.for_each_block(|b| {
                    match &b.kind {
                        BlockKind::Image { src, .. } => {
                            if src.starts_with("http://") || src.starts_with("https://") {
                                remote.push(serde_json::json!({ "doc_id": entry.id, "src": src }));
                            } else {
                                match store::image_path_in(&root, src) {
                                    Some(p) if p.exists() => { referenced_abs.insert(p); }
                                    _ => missing.push(serde_json::json!({ "doc_id": entry.id, "doc_title": entry.title, "kind": "image", "src": src })),
                                }
                            }
                        }
                        BlockKind::File(f) => {
                            let resolved = store::export_path_in(&root, &f.src).or_else(|| store::image_path_in(&root, &f.src));
                            match resolved {
                                Some(p) if p.exists() => { referenced_abs.insert(p); }
                                _ => missing.push(serde_json::json!({ "doc_id": entry.id, "doc_title": entry.title, "kind": "file", "src": f.src })),
                            }
                        }
                        BlockKind::Board(bd) => {
                            for fd in bd.files.values() {
                                if let Some(rel) = &fd.path {
                                    let p = root.join(rel);
                                    if p.exists() {
                                        referenced_abs.insert(p);
                                    } else {
                                        missing.push(serde_json::json!({ "doc_id": entry.id, "doc_title": entry.title, "kind": "board_image", "src": rel }));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    for (_, m) in b.text.spans() {
                        if let Some(f) = m.inline.as_ref().and_then(Inline::file) {
                            let resolved = store::export_path_in(&root, &f.src).or_else(|| store::image_path_in(&root, &f.src));
                            match resolved {
                                Some(p) if p.exists() => { referenced_abs.insert(p); }
                                _ => missing.push(serde_json::json!({ "doc_id": entry.id, "doc_title": entry.title, "kind": "inline_file", "src": f.src })),
                            }
                        }
                    }
                });
            }

            let mut on_disk = vec![];
            collect_files(&root, &root.join(".assets"), &mut on_disk);
            let unreferenced: Vec<_> = on_disk.iter()
                .filter(|(rel, _)| !referenced_abs.contains(&root.join(rel)))
                .map(|(rel, bytes)| serde_json::json!({ "src": rel, "bytes": bytes }))
                .collect();

            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "missing": missing,
                    "missing_count": missing.len(),
                    "unreferenced": unreferenced,
                    "unreferenced_count": unreferenced.len(),
                    "remote_images": remote,
                }),
                &st.warnings, &[],
                &format!("Verified assets: {} missing, {} unreferenced in .assets/", missing.len(), unreferenced.len()),
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled asset command {}", cmd.command_path))),
    }
}
