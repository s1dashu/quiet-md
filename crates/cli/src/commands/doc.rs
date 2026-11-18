//! `qmd doc` — create, apply, get, rename, move, duplicate, delete, restore, purge, versions.

use super::{check_if_unmodified, commit_core, is_markdown_entry, known_doc_ids, load_doc, parse_json_arg, read_arg_or_stdin};
use crate::args::ParsedCmd;
use crate::lint::lint_document;
use crate::locator;
use crate::output::{CliFailure, Diagnostic, GlobalOpts, print_success};
use crate::spec::{self, DocSpec};
use fdl_core::diff::Kind as DiffKind;
use fdl_core::docxml;
use fdl_core::editor::core::{Core, validate_document};
use fdl_core::markdown;
use fdl_core::model::{Block, Document, RichText};
use fdl_core::store::{Meta, Store};
use std::path::Path;

fn check_diags(diags: &[Diagnostic], strict: bool) -> Result<(), CliFailure> {
    if diags.iter().any(|d| d.severity == "error") {
        return Err(CliFailure::new("VALIDATION_FAILED", "Document has structural or schema errors.")
            .with_diagnostics(diags.to_vec())
            .with_hint("Fix the reported diagnostics before saving."));
    }
    if strict && !diags.is_empty() {
        return Err(CliFailure::new("STRICT_VALIDATION_FAILED", format!("--strict enabled and {} diagnostic(s) reported.", diags.len()))
            .with_diagnostics(diags.to_vec()));
    }
    Ok(())
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut store = super::open_store(opts)?;
    let root = store.root().to_path_buf();
    match cmd.command_path.as_str() {
        "doc create" => {
            let sources = [cmd.flag("spec").is_some(), cmd.flag("xml").is_some(), cmd.flag("md").is_some()]
                .into_iter().filter(|x| *x).count();
            if sources > 1 {
                return Err(CliFailure::usage("Pass at most one of --spec, --xml, or --md."));
            }
            let parent = locator::resolve_optional_folder(&mut store, cmd.flag("folder"))?;
            let as_md = cmd.bool_flag("as-markdown")?.unwrap_or(false);
            let strict = cmd.bool_flag("strict")?.unwrap_or(false);
            let mut front = String::new();
            let mut meta_spec = None;

            let doc = if let Some(spec_arg) = cmd.flag("spec") {
                let mut dspec: DocSpec = parse_json_arg(spec_arg)?;
                if let Some(t) = cmd.flag("title") && dspec.title.as_deref().unwrap_or("").is_empty() && dspec.title_runs.is_none() {
                    dspec.title = Some(t.to_string());
                }
                meta_spec = dspec.meta.clone();
                spec::compile_doc_spec(&dspec, &mut store, opts.dry_run)?
            } else if let Some(xml_arg) = cmd.flag("xml") {
                let raw = read_arg_or_stdin(xml_arg)?;
                let mut d = docxml::parse(&raw).map_err(|e| CliFailure::new("INVALID_DOCXML", format!("{e:#}")))?;
                if let Some(t) = cmd.flag("title") {
                    d.title = RichText::plain(t);
                }
                validate_document(&d).map_err(|e| CliFailure::new("MODEL_INVARIANT_ERROR", e))?;
                d
            } else if let Some(md_arg) = cmd.flag("md") {
                let raw = read_arg_or_stdin(md_arg)?;
                let (fm, _) = markdown::front_matter(&raw);
                front = fm.to_string();
                let base = (md_arg != "-").then(|| Path::new(md_arg)).filter(|p| p.exists()).and_then(|p| p.parent());
                let fallback_title = (md_arg != "-")
                    .then(|| Path::new(md_arg).file_stem().and_then(|s| s.to_str()))
                    .flatten()
                    .unwrap_or("Untitled");
                let title = cmd.flag("title").unwrap_or(fallback_title);
                let d = markdown::parse_file(&raw, title, base);
                validate_document(&d).map_err(|e| CliFailure::new("MODEL_INVARIANT_ERROR", e))?;
                d
            } else {
                Document {
                    title: RichText::plain(cmd.flag("title").unwrap_or("")),
                    icon: None,
                    cover: None,
                    blocks: vec![Block::text("")],
                }
            };

            let mut preview_meta = Meta::default();
            if let Some(m) = &meta_spec && let Some(w) = &m.width {
                preview_meta.width = spec::parse_width(w)?;
            }
            let ids = known_doc_ids(&mut store);
            let diags = lint_document(&doc, Some(&preview_meta), &store, &ids);
            check_diags(&diags, strict)?;

            let (id, rel, mtime) = if opts.dry_run {
                ("dry-run-doc-id".to_string(), "(dry-run)".to_string(), None)
            } else {
                let new_id = if as_md {
                    store.create_markdown_in(&doc, &front, parent.as_deref())?
                } else {
                    store.create_from_in(&doc, parent.as_deref())?
                };
                if let Some(m) = &meta_spec {
                    spec::apply_meta_spec(&mut store, &new_id, m)?;
                }
                let rel = store.list().into_iter().find(|e| e.id == new_id).map(|e| e.meta.rel).unwrap_or_default();
                let mtime = store.doc_mtime_secs(&new_id);
                (new_id, rel, mtime)
            };
            let indexed = locator::index_blocks(&doc);
            let locs: Vec<_> = indexed.iter().map(|b| serde_json::json!({
                "loc": b.loc, "kind": b.kind, "preview": b.preview,
            })).collect();
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "id": id,
                    "title": doc.title.text,
                    "rel": rel,
                    "folder": parent,
                    "markdown": as_md,
                    "block_count": doc.blocks.len(),
                    "mtime_secs": mtime,
                    "blocks": locs,
                }),
                &store.warnings, &diags,
                &format!("Created document '{}' ({id}) at {rel}", doc.title.text),
            );
            Ok(())
        }
        "doc apply" => {
            let (entry, old_doc) = load_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            check_if_unmodified(&store, &entry.id, cmd)?;
            let has_spec = cmd.flag("spec").is_some();
            let has_xml = cmd.flag("xml").is_some();
            if has_spec == has_xml {
                return Err(CliFailure::usage("Pass exactly one of --spec or --xml to `doc apply`."));
            }
            let strict = cmd.bool_flag("strict")?.unwrap_or(false);
            let mut meta_spec = None;
            let mut next_doc = if let Some(spec_arg) = cmd.flag("spec") {
                let mut dspec: DocSpec = parse_json_arg(spec_arg)?;
                if dspec.title.is_none() && dspec.title_runs.is_none() {
                    dspec.title = Some(old_doc.title.text.clone());
                }
                meta_spec = dspec.meta.clone();
                spec::compile_doc_spec(&dspec, &mut store, opts.dry_run)?
            } else {
                let raw = read_arg_or_stdin(cmd.flag("xml").unwrap())?;
                let d = docxml::parse(&raw).map_err(|e| CliFailure::new("INVALID_DOCXML", format!("{e:#}")))?;
                validate_document(&d).map_err(|e| CliFailure::new("MODEL_INVARIANT_ERROR", e))?;
                d
            };

            let mut preview_meta = entry.meta.clone();
            if let Some(m) = &meta_spec && let Some(w) = &m.width {
                preview_meta.width = spec::parse_width(w)?;
            }
            let title_next = std::mem::take(&mut next_doc.title);
            let blocks_next = std::mem::take(&mut next_doc.blocks);
            let mut base = old_doc;
            base.icon = next_doc.icon;
            base.cover = next_doc.cover;
            let mut core = Core::new(base);
            core.set_title(title_next);
            core.replace_blocks(blocks_next);

            let ids = known_doc_ids(&mut store);
            let diags = lint_document(&core.doc, Some(&preview_meta), &store, &ids);
            check_diags(&diags, strict)?;

            let mut entry_for_save = entry.clone();
            entry_for_save.meta = preview_meta;
            let _ = commit_core(&mut store, &entry_for_save, &core, opts.dry_run)?;
            if !opts.dry_run && let Some(m) = &meta_spec {
                spec::apply_meta_spec(&mut store, &entry.id, m)?;
            }
            let mtime = if opts.dry_run { Some(entry.meta.modified) } else { store.doc_mtime_secs(&entry.id) };
            let indexed = locator::index_blocks(&core.doc);
            let locs: Vec<_> = indexed.iter().map(|b| serde_json::json!({
                "loc": b.loc, "kind": b.kind, "preview": b.preview,
            })).collect();
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "id": entry.id,
                    "title": core.doc.title.text,
                    "rel": entry.meta.rel,
                    "block_count": core.doc.blocks.len(),
                    "mtime_secs": mtime,
                    "blocks": locs,
                }),
                &store.warnings, &diags,
                &format!("Applied updates to '{}' ({})", core.doc.title.text, entry.id),
            );
            Ok(())
        }
        "doc get" => {
            let (entry, doc) = load_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let fmt = cmd.flag("format").unwrap_or("spec").to_ascii_lowercase();
            let mtime = store.doc_mtime_secs(&entry.id);
            match fmt.as_str() {
                "spec" => {
                    let doc_spec = spec::doc_to_spec(&doc, Some(&entry.meta));
                    let data = serde_json::json!({
                        "id": entry.id,
                        "rel": entry.meta.rel,
                        "markdown": is_markdown_entry(&entry),
                        "mtime_secs": mtime,
                        "spec": doc_spec,
                    });
                    let human = serde_json::to_string_pretty(&data).unwrap_or_default();
                    print_success(opts, &cmd.command_path, Some(&root), data, &store.warnings, &[], &human);
                    Ok(())
                }
                "xml" => {
                    let xml = docxml::serialize(&doc);
                    print_success(opts, &cmd.command_path, Some(&root), serde_json::json!({ "id": entry.id, "title": entry.title, "xml": xml }), &store.warnings, &[], &xml);
                    Ok(())
                }
                "md" => {
                    let md = markdown::export(&doc, &markdown::Options::default()).text;
                    print_success(opts, &cmd.command_path, Some(&root), serde_json::json!({ "id": entry.id, "title": entry.title, "markdown": md }), &store.warnings, &[], &md);
                    Ok(())
                }
                "outline" => {
                    let indexed = locator::index_blocks(&doc);
                    let blocks: Vec<_> = indexed.iter().map(|b| serde_json::json!({
                        "loc": b.loc, "order": b.order, "kind": b.kind, "preview": b.preview,
                    })).collect();
                    let mut human = format!("{} ({}) [{}]\n", doc.title.text, entry.id, entry.meta.rel);
                    for b in &indexed {
                        human.push_str(&format!("  {:12} {:10} {}\n", b.loc, b.kind, b.preview));
                    }
                    print_success(
                        opts, &cmd.command_path, Some(&root),
                        serde_json::json!({ "id": entry.id, "title": entry.title, "rel": entry.meta.rel, "mtime_secs": mtime, "blocks": blocks }),
                        &store.warnings, &[],
                        human.trim_end(),
                    );
                    Ok(())
                }
                other => Err(CliFailure::new("INVALID_FORMAT", format!("Unknown --format '{other}'.")).with_valid(["spec", "xml", "md", "outline"])),
            }
        }
        "doc rename" => {
            let entry = locator::resolve_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let new_title = cmd.pos(1, "<title>")?;
            if !opts.dry_run {
                store.rename(&entry.id, new_title)?;
            }
            let rel = if opts.dry_run { entry.meta.rel.clone() } else { store.list().into_iter().find(|e| e.id == entry.id).map(|e| e.meta.rel).unwrap_or(entry.meta.rel) };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "title": new_title, "rel": rel }),
                &store.warnings, &[],
                &format!("Renamed '{}' -> '{new_title}'", entry.title),
            );
            Ok(())
        }
        "doc move" => {
            let entry = locator::resolve_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let folder = match cmd.flag("folder") {
                Some(f) => locator::resolve_optional_folder(&mut store, Some(f))?,
                None => entry.meta.parent.clone(),
            };
            let before = match cmd.flag("before") {
                Some(b) => {
                    let entries = store.list();
                    let found = entries.iter().find(|e| e.id == b || e.meta.rel == b || e.title == b)
                        .ok_or_else(|| CliFailure::new("SIBLING_NOT_FOUND", format!("Sibling '{b}' not found.")))?;
                    Some(found.id.clone())
                }
                None => None,
            };
            if !opts.dry_run {
                store.move_in_tree(&entry.id, folder.as_deref(), before.as_deref())?;
            }
            let rel = if opts.dry_run { entry.meta.rel.clone() } else { store.list().into_iter().find(|e| e.id == entry.id).map(|e| e.meta.rel).unwrap_or(entry.meta.rel) };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "folder": folder, "before": before, "rel": rel }),
                &store.warnings, &[],
                &format!("Moved '{}' ({rel})", entry.title),
            );
            Ok(())
        }
        "doc duplicate" => {
            let (entry, mut doc) = load_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let folder = match cmd.flag("folder") {
                Some(f) => locator::resolve_optional_folder(&mut store, Some(f))?,
                None => entry.meta.parent.clone(),
            };
            if let Some(t) = cmd.flag("title") {
                doc.title = RichText::plain(t);
            }
            let (id, rel) = if opts.dry_run {
                ("dry-run-copy-id".to_string(), "(dry-run)".to_string())
            } else {
                let new_id = if is_markdown_entry(&entry) {
                    store.create_markdown_in(&doc, "", folder.as_deref())?
                } else {
                    store.create_from_in(&doc, folder.as_deref())?
                };
                store.set_width(&new_id, entry.meta.width)?;
                store.set_catalogue(&new_id, !entry.meta.catalogue_closed)?;
                store.update_meta(&new_id, |m| {
                    m.refs_hidden = entry.meta.refs_hidden;
                    m.info = entry.meta.info;
                })?;
                let rel = store.list().into_iter().find(|e| e.id == new_id).map(|e| e.meta.rel).unwrap_or_default();
                (new_id, rel)
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "source_id": entry.id, "id": id, "title": doc.title.text, "rel": rel, "folder": folder }),
                &store.warnings, &[],
                &format!("Duplicated '{}' -> '{rel}' ({id})", entry.title),
            );
            Ok(())
        }
        "doc delete" => {
            let entry = locator::resolve_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            if !opts.dry_run {
                store.delete(&entry.id, None)?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "trashed": true }),
                &store.warnings, &[],
                &format!("Moved '{}' to Trash", entry.title),
            );
            Ok(())
        }
        "doc restore" => {
            let id = cmd.pos(0, "<id>")?;
            let moved_to_root = if opts.dry_run { false } else { store.restore(id)? };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": id, "restored": true, "moved_to_root": moved_to_root }),
                &store.warnings, &[],
                &format!("Restored '{id}' from Trash"),
            );
            Ok(())
        }
        "doc purge" => {
            let id = cmd.pos(0, "<id>")?;
            if !opts.dry_run {
                store.purge(id)?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": id, "purged": true }),
                &store.warnings, &[],
                &format!("Purged '{id}' permanently"),
            );
            Ok(())
        }
        "doc versions" => run_versions(cmd, opts, &mut store),
        _ => Err(CliFailure::usage(format!("Unhandled doc command {}", cmd.command_path))),
    }
}

fn run_versions(cmd: &ParsedCmd, opts: &GlobalOpts, store: &mut Store) -> Result<(), CliFailure> {
    let root = store.root().to_path_buf();
    let sub = cmd.pos(0, "<list|save|label|diff|restore>")?;
    let entry = locator::resolve_doc(store, cmd.pos(1, "<doc>")?)?;
    match sub {
        "list" => {
            let vs: Vec<_> = store.versions(&entry.id).into_iter().map(|v| serde_json::json!({
                "name": v.name, "ms": v.ms, "size": v.size, "label": v.label,
            })).collect();
            let human = if vs.is_empty() {
                "No saved versions.".into()
            } else {
                vs.iter().map(|v| format!("{}  ({} bytes){}", v["name"].as_str().unwrap_or(""), v["size"], v["label"].as_str().map(|l| format!("  [{l}]")).unwrap_or_default()))
                    .collect::<Vec<_>>().join("\n")
            };
            print_success(opts, &cmd.command_path, Some(&root), serde_json::json!({ "id": entry.id, "versions": vs }), &store.warnings, &[], &human);
            Ok(())
        }
        "save" => {
            let created = if opts.dry_run { true } else { store.save_version_now(&entry.id)? };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "created": created }),
                &store.warnings, &[],
                if created { "Saved new version snapshot." } else { "Document unchanged since latest version." },
            );
            Ok(())
        }
        "label" => {
            let ver = cmd.req_flag("version")?;
            let lbl = cmd.flag("label").filter(|s| !s.is_empty());
            if !opts.dry_run {
                store.set_version_label(&entry.id, ver, lbl)?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "version": ver, "label": lbl }),
                &store.warnings, &[],
                &format!("Updated label on snapshot {ver}"),
            );
            Ok(())
        }
        "diff" => {
            let ver = cmd.req_flag("version")?;
            let lines = store.version_diff(&entry.id, ver, None)?;
            let out: Vec<_> = lines.iter().map(|l| serde_json::json!({
                "kind": match l.kind { DiffKind::Same => "same", DiffKind::Added => "added", DiffKind::Removed => "removed" },
                "text": l.text,
            })).collect();
            let human: String = lines.iter().map(|l| {
                let p = match l.kind { DiffKind::Same => " ", DiffKind::Added => "+", DiffKind::Removed => "-" };
                format!("{p} {}\n", l.text)
            }).collect();
            print_success(opts, &cmd.command_path, Some(&root), serde_json::json!({ "id": entry.id, "version": ver, "lines": out }), &store.warnings, &[], human.trim_end());
            Ok(())
        }
        "restore" => {
            let ver = cmd.req_flag("version")?;
            if !opts.dry_run {
                let restored = store.restore_version(&entry.id, ver)?;
                validate_document(&restored).map_err(|e| CliFailure::new("MODEL_INVARIANT_ERROR", e))?;
            }
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "restored_version": ver }),
                &store.warnings, &[],
                &format!("Restored '{}' to snapshot {ver}", entry.title),
            );
            Ok(())
        }
        other => Err(CliFailure::new("UNKNOWN_VERSIONS_OP", format!("Unknown doc versions op '{other}'.")).with_valid(["list", "save", "label", "diff", "restore"])),
    }
}
