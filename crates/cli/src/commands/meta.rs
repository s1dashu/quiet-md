//! `qmd meta` — page width, pinning, stars, info row, icon & cover control.

use super::{is_board_entry, load_doc};
use crate::args::ParsedCmd;
use crate::assets;
use crate::locator;
use crate::output::{CliFailure, GlobalOpts, print_success};
use crate::spec::{self, MetaSpec};
use fdl_core::store::PageWidth;
use std::path::Path;

fn width_str(w: PageWidth) -> &'static str {
    match w { PageWidth::Standard => "standard", PageWidth::Wide => "wide", PageWidth::Full => "full" }
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut store = super::open_store(opts)?;
    let root = store.root().to_path_buf();
    match cmd.command_path.as_str() {
        "meta get" => {
            let entry = locator::resolve_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let (icon, cover) = if is_board_entry(&entry) {
                (None, None)
            } else {
                let d = store.load(&entry.id)?;
                (d.icon, d.cover.map(|c| serde_json::json!({ "src": c.src, "x": c.x, "y": c.y })))
            };
            let m = &entry.meta;
            let data = serde_json::json!({
                "id": entry.id,
                "title": entry.title,
                "rel": entry.meta.rel,
                "width": width_str(m.width),
                "pinned": m.pinned,
                "starred": m.starred > 0,
                "reading": m.reading,
                "catalogue_open": !m.catalogue_closed,
                "refs_hidden": m.refs_hidden,
                "info": { "hidden": m.info.hidden, "created": m.info.created, "modified": m.info.modified },
                "order": m.order,
                "root_order": m.root_order,
                "created": m.created,
                "modified": m.modified,
                "mtime_secs": store.doc_mtime_secs(&entry.id),
                "icon": icon,
                "cover": cover,
            });
            print_success(opts, &cmd.command_path, Some(&root), data.clone(), &store.warnings, &[], &serde_json::to_string_pretty(&data).unwrap_or_default());
            Ok(())
        }
        "meta set" => {
            let entry = locator::resolve_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let ms = MetaSpec {
                width: cmd.flag("width").map(str::to_string),
                pinned: cmd.bool_flag("pinned")?,
                starred: cmd.bool_flag("starred")?,
                reading: cmd.bool_flag("reading")?,
                catalogue_open: cmd.bool_flag("catalogue-open")?,
                refs_hidden: cmd.bool_flag("refs-hidden")?,
                info_hidden: cmd.bool_flag("info-hidden")?,
                info_created: cmd.bool_flag("info-created")?,
                info_modified: cmd.bool_flag("info-modified")?,
                order: cmd.flag("order").map(|v| v.parse().map_err(|_| CliFailure::usage("Invalid u64 for --order"))).transpose()?,
                root_order: cmd.flag("root-order").map(|v| v.parse().map_err(|_| CliFailure::usage("Invalid u64 for --root-order"))).transpose()?,
            };
            if let Some(w) = &ms.width {
                let _ = spec::parse_width(w)?;
            }
            if !opts.dry_run {
                spec::apply_meta_spec(&mut store, &entry.id, &ms)?;
            }
            let updated = store.list().into_iter().find(|e| e.id == entry.id).map(|e| e.meta).unwrap_or(entry.meta);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "id": entry.id,
                    "width": ms.width.as_deref().unwrap_or(width_str(updated.width)),
                    "pinned": updated.pinned,
                    "starred": updated.starred > 0,
                    "reading": updated.reading,
                    "catalogue_open": !updated.catalogue_closed,
                    "refs_hidden": updated.refs_hidden,
                }),
                &store.warnings, &[],
                &format!("Updated metadata on '{}'", entry.title),
            );
            Ok(())
        }
        "meta cover" => {
            let sub = cmd.pos(0, "<set|remove>")?;
            let (entry, mut doc) = load_doc(&mut store, cmd.pos(1, "<doc>")?)?;
            match sub {
                "set" => {
                    let x: f32 = cmd.flag("x").map(|v| v.parse().map_err(|_| CliFailure::usage("Invalid f32 for --x"))).transpose()?.unwrap_or(0.);
                    let y: f32 = cmd.flag("y").map(|v| v.parse().map_err(|_| CliFailure::usage("Invalid f32 for --y"))).transpose()?.unwrap_or(0.);
                    let cov = assets::resolve_cover(&store, cmd.flag("gallery"), cmd.flag("file").map(Path::new), x, y, opts.dry_run)?;
                    doc.cover = Some(cov.clone());
                    if !opts.dry_run { store.save(&entry.id, &doc)?; }
                    print_success(
                        opts, &cmd.command_path, Some(&root),
                        serde_json::json!({ "id": entry.id, "cover": { "src": cov.src, "x": cov.x, "y": cov.y } }),
                        &store.warnings, &[],
                        &format!("Set cover on '{}' -> {}", entry.title, cov.src),
                    );
                    Ok(())
                }
                "remove" => {
                    doc.cover = None;
                    if !opts.dry_run { store.save(&entry.id, &doc)?; }
                    print_success(
                        opts, &cmd.command_path, Some(&root),
                        serde_json::json!({ "id": entry.id, "cover": null }),
                        &store.warnings, &[],
                        &format!("Removed cover from '{}'", entry.title),
                    );
                    Ok(())
                }
                other => Err(CliFailure::new("UNKNOWN_COVER_OP", format!("Expected 'set' or 'remove', got '{other}'.")).with_valid(["set", "remove"])),
            }
        }
        "meta icon" => {
            let sub = cmd.pos(0, "<set|remove>")?;
            let (entry, mut doc) = load_doc(&mut store, cmd.pos(1, "<doc>")?)?;
            match sub {
                "set" => {
                    let em = assets::resolve_emoji(cmd.req_flag("emoji")?)?;
                    doc.icon = Some(em.clone());
                    if !opts.dry_run { store.save(&entry.id, &doc)?; }
                    print_success(
                        opts, &cmd.command_path, Some(&root),
                        serde_json::json!({ "id": entry.id, "icon": em }),
                        &store.warnings, &[],
                        &format!("Set icon on '{}' -> {em}", entry.title),
                    );
                    Ok(())
                }
                "remove" => {
                    doc.icon = None;
                    if !opts.dry_run { store.save(&entry.id, &doc)?; }
                    print_success(
                        opts, &cmd.command_path, Some(&root),
                        serde_json::json!({ "id": entry.id, "icon": null }),
                        &store.warnings, &[],
                        &format!("Removed icon from '{}'", entry.title),
                    );
                    Ok(())
                }
                other => Err(CliFailure::new("UNKNOWN_ICON_OP", format!("Expected 'set' or 'remove', got '{other}'.")).with_valid(["set", "remove"])),
            }
        }
        _ => Err(CliFailure::usage(format!("Unhandled meta command {}", cmd.command_path))),
    }
}
