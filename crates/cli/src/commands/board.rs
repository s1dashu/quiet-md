//! `qmd board` — build, create, insert-into-doc, inspect, export-svg, render-png.

use super::{commit_core, is_board_entry, load_doc, parse_place, read_arg_or_stdin, write_file_bytes};
use crate::args::ParsedCmd;
use crate::board_builder::{self, DiagramSpec};
use crate::locator;
use crate::output::{CliFailure, GlobalOpts, print_success};
use fdl_core::board::bounds::get_common_bounds;
use fdl_core::board::scene::Scene;
use fdl_core::board::types::{Element, Kind};
use fdl_core::editor::core::Core;
use fdl_core::model::*;
use fdl_core::store::Store;
use fdl_core::whiteboard::Board;
use std::path::Path;

fn compile_or_load_board(spec_arg: &str, height_override: Option<f32>) -> Result<Board, CliFailure> {
    let raw = read_arg_or_stdin(spec_arg)?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| CliFailure::new("INVALID_JSON", format!("Failed parsing board JSON: {e}")))?;
    let is_raw_excalidraw = v.get("type").and_then(|t| t.as_str()) == Some("excalidraw") || (v.get("elements").is_some() && v.get("nodes").is_none());
    let mut bd = if is_raw_excalidraw {
        Board::from_file(&raw).map_err(|e| CliFailure::new("BOARD_PARSE_ERROR", format!("Invalid .excalidraw JSON: {e}")))?
    } else {
        let mut ds: DiagramSpec = serde_json::from_value(v).map_err(|e| CliFailure::new("INVALID_DIAGRAM_SPEC", format!("Invalid DiagramSpec: {e}")))?;
        if let Some(h) = height_override { ds.height = h; }
        board_builder::compile_diagram(&ds)?
    };
    if let Some(h) = height_override {
        bd.height = h.max(120.);
    }
    Ok(bd)
}

fn load_target_board(store: &mut Store, doc_arg: &str, at: Option<&str>) -> Result<(String, Board), CliFailure> {
    let p = Path::new(doc_arg);
    if at.is_none() && doc_arg.ends_with(".excalidraw") && p.is_file() {
        let bd = board_builder::load_board_file(p)?;
        return Ok((doc_arg.to_string(), bd));
    }
    let entry = locator::resolve_doc(store, doc_arg)?;
    if is_board_entry(&entry) {
        let abs = store.abs_path(&entry.meta.rel);
        let bd = board_builder::load_board_file(&abs)?;
        return Ok((entry.id, bd));
    }
    let doc = store.load(&entry.id).map_err(|e| CliFailure::new("DOC_LOAD_FAILED", format!("{e:#}")))?;
    let bid = if let Some(sel) = at {
        locator::resolve_block(&doc, sel)?
    } else {
        let boards: Vec<BlockId> = locator::index_blocks(&doc).into_iter().filter(|b| b.kind == "board").map(|b| b.id).collect();
        match boards.len() {
            1 => boards[0],
            0 => return Err(CliFailure::new("NO_BOARD_BLOCK", format!("Document '{}' contains no whiteboard blocks.", entry.title))),
            n => return Err(CliFailure::new("AMBIGUOUS_BOARD", format!("Document '{}' contains {n} whiteboard blocks; pass --at <loc>.", entry.title))
                .with_hint("Run `qmd doc get <doc> --format outline --json` to see each board's `loc`.")),
        }
    };
    let Some(BlockKind::Board(bd)) = locator::find_block(&doc, bid).map(|b| &b.kind) else {
        return Err(CliFailure::new("NOT_A_BOARD", "Target block is not a whiteboard block."));
    };
    Ok((format!("{}:{}", entry.id, locator::loc_of(&doc, bid).unwrap_or_default()), bd.as_ref().clone()))
}

fn element_kind_label(e: &Element) -> &'static str {
    if e.is_text() { "text" }
    else if e.is_arrow() { "arrow" }
    else if e.is_linear() { "line" }
    else if e.is_frame() { "frame" }
    else if e.is_image() { "image" }
    else if matches!(e.kind, Kind::Diamond) { "diamond" }
    else if matches!(e.kind, Kind::Ellipse) { "ellipse" }
    else if matches!(e.kind, Kind::StickyNote { .. }) { "sticky" }
    else { "rectangle" }
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    match cmd.command_path.as_str() {
        "board build" => {
            let bd = compile_or_load_board(cmd.req_flag("spec")?, None)?;
            let out_path = cmd.flag("out").map(Path::new);
            let fmt = cmd.flag("format").map(str::to_ascii_lowercase).unwrap_or_else(|| {
                match out_path.and_then(|p| p.extension()).and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                    Some("svg") => "svg".into(),
                    Some("png") => "png".into(),
                    _ => "excalidraw".into(),
                }
            });
            let scale: f32 = cmd.flag("scale").map(|s| s.parse().map_err(|_| CliFailure::usage("Invalid f32 for --scale"))).transpose()?.unwrap_or(2.0);
            let lib_root = super::resolve_library_root(opts);
            let svg = bd.svg_in(&lib_root);

            match fmt.as_str() {
                "excalidraw" => {
                    let json = bd.file_json();
                    if let Some(p) = out_path {
                        write_file_bytes(p, json.as_bytes(), opts.dry_run)?;
                    }
                    let human = if let Some(p) = out_path {
                        format!("Built board ({} elements) -> {}", bd.elements.len(), p.display())
                    } else {
                        json.clone()
                    };
                    print_success(
                        opts, &cmd.command_path, None,
                        serde_json::json!({
                            "format": "excalidraw",
                            "elements": bd.elements.len(),
                            "out": out_path.map(|p| p.display().to_string()),
                            "excalidraw": if out_path.is_none() { serde_json::from_str::<serde_json::Value>(&json).ok() } else { None },
                        }),
                        &[], &[], &human,
                    );
                    Ok(())
                }
                "svg" => {
                    if let Some(p) = out_path {
                        write_file_bytes(p, svg.as_bytes(), opts.dry_run)?;
                    }
                    let human = if let Some(p) = out_path {
                        format!("Rendered SVG ({} bytes) -> {}", svg.len(), p.display())
                    } else {
                        svg.clone()
                    };
                    print_success(
                        opts, &cmd.command_path, None,
                        serde_json::json!({ "format": "svg", "bytes": svg.len(), "out": out_path.map(|p| p.display().to_string()), "svg": if out_path.is_none() { Some(svg) } else { None } }),
                        &[], &[], &human,
                    );
                    Ok(())
                }
                "png" => {
                    let out = out_path.ok_or_else(|| CliFailure::usage("--out <path.png> required when outputting PNG"))?;
                    let png = board_builder::render_svg_to_png(&svg, scale)?;
                    write_file_bytes(out, &png, opts.dry_run)?;
                    print_success(
                        opts, &cmd.command_path, None,
                        serde_json::json!({ "format": "png", "bytes": png.len(), "scale": scale, "out": out.display().to_string() }),
                        &[], &[],
                        &format!("Rendered PNG ({} bytes, scale {scale}x) -> {}", png.len(), out.display()),
                    );
                    Ok(())
                }
                other => Err(CliFailure::new("INVALID_FORMAT", format!("Unknown --format '{other}'.")).with_valid(["excalidraw", "svg", "png"])),
            }
        }
        "board create" => {
            let mut store = super::open_store(opts)?;
            let root = store.root().to_path_buf();
            let title = cmd.pos(0, "<title>")?;
            let bd = compile_or_load_board(cmd.req_flag("spec")?, None)?;
            let parent = locator::resolve_optional_folder(&mut store, cmd.flag("folder"))?;
            let (id, rel) = if opts.dry_run {
                ("dry-run-board-id".into(), format!("{title}.excalidraw"))
            } else {
                let id = store.create_board_in(title, &bd, parent.as_deref())?;
                let rel = store.list().into_iter().find(|e| e.id == id).map(|e| e.meta.rel).unwrap_or_default();
                (id, rel)
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": id, "title": title, "rel": rel, "elements": bd.elements.len() }),
                &store.warnings, &[],
                &format!("Created whiteboard '{title}' ({id}) at {rel}"),
            );
            Ok(())
        }
        "board insert-into-doc" => {
            let mut store = super::open_store(opts)?;
            let root = store.root().to_path_buf();
            let (entry, doc) = load_doc(&mut store, cmd.pos(0, "<doc>")?)?;
            let h = cmd.flag("height").map(|v| v.parse::<f32>().map_err(|_| CliFailure::usage("Invalid f32 for --height"))).transpose()?;
            let bd = compile_or_load_board(cmd.req_flag("spec")?, h)?;
            let replace = cmd.bool_flag("replace")?.unwrap_or(false);
            let mut core = Core::new(doc);
            let bid = if replace {
                let sel = cmd.req_flag("at")?;
                let target = locator::resolve_block(&core.doc, sel)?;
                if !core.set_board(target, bd, false) {
                    return Err(CliFailure::new("NOT_A_BOARD", format!("Block at '{sel}' is not a whiteboard block.")));
                }
                target
            } else {
                let anchor = locator::resolve_block(&core.doc, cmd.flag("at").unwrap_or("last"))?;
                let place = parse_place(cmd.flag("place"))?;
                let blk = Block::new(BlockKind::Board(bd.into()), RichText::default());
                let new_id = blk.id;
                core.insert_blocks_at(vec![blk], anchor, place);
                new_id
            };
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, bid);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc, "replaced": replace }),
                &store.warnings, &diags,
                &format!("{} whiteboard at {} in '{}'", if replace { "Replaced" } else { "Inserted" }, loc.as_deref().unwrap_or("?"), entry.title),
            );
            Ok(())
        }
        "board inspect" => {
            let mut store = super::open_store(opts)?;
            let root = store.root().to_path_buf();
            let (target_label, bd) = load_target_board(&mut store, cmd.pos(0, "<doc>")?, cmd.flag("at"))?;
            let scene = Scene::new(bd.elements.clone());
            let bounds = get_common_bounds(scene.non_deleted_elements(), &scene);
            let elements: Vec<_> = bd.elements.iter().filter(|e| !e.is_deleted).map(|e| serde_json::json!({
                "id": e.id,
                "kind": element_kind_label(e),
                "x": e.x, "y": e.y, "width": e.width, "height": e.height,
                "text": e.text().map(|t| t.text.as_str()),
                "frame_id": e.frame_id,
            })).collect();
            let data = serde_json::json!({
                "target": target_label,
                "height": bd.height,
                "element_count": elements.len(),
                "bounds": { "min_x": bounds[0], "min_y": bounds[1], "max_x": bounds[2], "max_y": bounds[3] },
                "plain_text": bd.plain_text(),
                "elements": elements,
            });
            print_success(opts, &cmd.command_path, Some(&root), data.clone(), &store.warnings, &[], &serde_json::to_string_pretty(&data).unwrap_or_default());
            Ok(())
        }
        "board export-svg" => {
            let mut store = super::open_store(opts)?;
            let root = store.root().to_path_buf();
            let (target_label, bd) = load_target_board(&mut store, cmd.pos(0, "<doc>")?, cmd.flag("at"))?;
            let svg = bd.svg_in(&root);
            let out = cmd.flag("out").map(Path::new);
            if let Some(p) = out {
                write_file_bytes(p, svg.as_bytes(), opts.dry_run)?;
            }
            let human = if let Some(p) = out {
                format!("Exported SVG ({} bytes) -> {}", svg.len(), p.display())
            } else {
                svg.clone()
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "target": target_label, "bytes": svg.len(), "out": out.map(|p| p.display().to_string()), "svg": if out.is_none() { Some(svg) } else { None } }),
                &store.warnings, &[], &human,
            );
            Ok(())
        }
        "board render-png" => {
            let mut store = super::open_store(opts)?;
            let root = store.root().to_path_buf();
            let (target_label, bd) = load_target_board(&mut store, cmd.pos(0, "<doc>")?, cmd.flag("at"))?;
            let out = Path::new(cmd.req_flag("out")?);
            let scale: f32 = cmd.flag("scale").map(|s| s.parse().map_err(|_| CliFailure::usage("Invalid f32 for --scale"))).transpose()?.unwrap_or(2.0);
            let svg = bd.svg_in(&root);
            let png = board_builder::render_svg_to_png(&svg, scale)?;
            write_file_bytes(out, &png, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "target": target_label, "bytes": png.len(), "scale": scale, "out": out.display().to_string() }),
                &store.warnings, &[],
                &format!("Rendered PNG ({} bytes, scale {scale}x) -> {}", png.len(), out.display()),
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled board command {}", cmd.command_path))),
    }
}
