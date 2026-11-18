//! `qmd table` — fine-grained table creation, cell editing, styling, merges and widths via `editor::Core`.

use super::{commit_core, load_doc, opt_clear_flag, parse_block_specs_arg, parse_place};
use crate::args::ParsedCmd;
use crate::locator;
use crate::output::{CliFailure, GlobalOpts, print_success};
use crate::spec;
use fdl_core::editor::core::Core;
use fdl_core::editor::table_ops::{CellSel, CellSelKind};
use fdl_core::model::*;

fn resolve_table(doc: &Document, sel: &str) -> Result<(BlockId, usize, usize), CliFailure> {
    let id = locator::resolve_block(doc, sel)?;
    let Some(b) = locator::find_block(doc, id) else {
        return Err(CliFailure::new("BLOCK_NOT_FOUND", format!("Table '{sel}' not found.")));
    };
    let BlockKind::Table(t) = &b.kind else {
        return Err(CliFailure::new("NOT_A_TABLE", format!("Block at '{sel}' is '{}', not a table.", locator::block_kind_name(&b.kind))));
    };
    Ok((id, t.rows.len(), t.ncols()))
}

fn parse_widths_csv(s: &str, ncols: usize) -> Result<Vec<f32>, CliFailure> {
    let mut out: Vec<f32> = s.split(',').map(|p| p.trim().parse::<f32>()).collect::<Result<_, _>>()
        .map_err(|_| CliFailure::usage("--widths expects comma-separated pixel numbers (e.g. 160,280,160)"))?;
    if out.is_empty() {
        return Err(CliFailure::usage("--widths cannot be empty."));
    }
    for w in &mut out {
        *w = w.max(Table::MIN_WIDTH);
    }
    out.resize(ncols, Table::DEFAULT_WIDTH);
    Ok(out)
}

fn parse_index_range(s: &str, limit: usize, label: &str) -> Result<(usize, usize), CliFailure> {
    let s = s.trim();
    let (a, b) = if let Some((l, r)) = s.split_once("..") {
        let start: usize = l.trim().parse().map_err(|_| CliFailure::usage(format!("Invalid {label} range '{s}'.")))?;
        let end_excl: usize = r.trim().parse().map_err(|_| CliFailure::usage(format!("Invalid {label} range '{s}'.")))?;
        if end_excl <= start {
            return Err(CliFailure::new("INVALID_RANGE", format!("Empty/reversed {label} range '{s}' (end must exceed start).")));
        }
        (start, end_excl - 1)
    } else {
        let idx: usize = s.parse().map_err(|_| CliFailure::usage(format!("Invalid {label} index '{s}'.")))?;
        (idx, idx)
    };
    if b >= limit {
        return Err(CliFailure::new("INDEX_OUT_OF_BOUNDS", format!("{label} index {b} out of bounds (table has {limit}).")));
    }
    Ok((a, b))
}

fn parse_cell_coord(cmd: &ParsedCmd, nr: usize, nc: usize) -> Result<(usize, usize), CliFailure> {
    let r: usize = cmd.req_flag("row")?.parse().map_err(|_| CliFailure::usage("Invalid usize for --row"))?;
    let c: usize = cmd.req_flag("col")?.parse().map_err(|_| CliFailure::usage("Invalid usize for --col"))?;
    if r >= nr || c >= nc {
        return Err(CliFailure::new("CELL_OUT_OF_BOUNDS", format!("Cell ({r},{c}) out of bounds for {nr}x{nc} table.")));
    }
    Ok((r, c))
}

fn parse_pair(s: &str) -> Result<(usize, usize), CliFailure> {
    let (a, b) = s.trim().split_once(',').ok_or_else(|| CliFailure::usage("Expected row,col pair"))?;
    let r = a.trim().parse::<usize>().map_err(|_| CliFailure::usage("Invalid row index"))?;
    let c = b.trim().parse::<usize>().map_err(|_| CliFailure::usage("Invalid col index"))?;
    Ok((r, c))
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut store = super::open_store(opts)?;
    let root = store.root().to_path_buf();
    let (entry, doc) = load_doc(&mut store, cmd.pos(0, "<doc>")?)?;

    match cmd.command_path.as_str() {
        "table create" => {
            let anchor = locator::resolve_block(&doc, cmd.flag("at").unwrap_or("last"))?;
            let place = parse_place(cmd.flag("place"))?;
            let rows: usize = cmd.flag("rows").unwrap_or("3").parse().map_err(|_| CliFailure::usage("Invalid --rows"))?;
            let cols: usize = cmd.flag("cols").unwrap_or("3").parse().map_err(|_| CliFailure::usage("Invalid --cols"))?;
            if rows == 0 || cols == 0 {
                return Err(CliFailure::new("INVALID_TABLE_SIZE", "Table requires at least 1 row and 1 column."));
            }
            let col_widths = match cmd.flag("widths") {
                Some(w) => parse_widths_csv(w, cols)?,
                None => vec![Table::DEFAULT_WIDTH; cols],
            };
            let header_rows: usize = cmd.flag("header-rows").unwrap_or("1").parse().map_err(|_| CliFailure::usage("Invalid --header-rows"))?;
            let header_col = cmd.bool_flag("header-col")?.unwrap_or(false);
            let mut t = Table {
                col_widths,
                header_rows: header_rows.min(rows),
                header_col,
                rows: (0..rows).map(|_| (0..cols).map(|_| TableCell::empty()).collect()).collect(),
            };
            t.repair();
            let blk = Block::new(BlockKind::Table(t), RichText::default()).fresh_ids();
            let new_id = blk.id;
            let mut core = Core::new(doc);
            core.insert_blocks_at(vec![blk], anchor, place);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, new_id);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": loc, "rows": rows, "cols": cols }),
                &store.warnings, &diags,
                &format!("Created {rows}x{cols} table at {}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "table set-cell" => {
            let tbl_sel = cmd.req_flag("table")?;
            let (tid, nr, nc) = resolve_table(&doc, tbl_sel)?;
            let (r, c) = parse_cell_coord(cmd, nr, nc)?;
            let mut core = Core::new(doc);

            if let Some(sp) = cmd.flag("spec") {
                let specs = parse_block_specs_arg(sp)?;
                let blocks: Vec<Block> = specs.iter().map(|s| spec::compile_block(s, &mut store, opts.dry_run)).collect::<Result<_, _>>()?;
                core.table_set_cell_blocks(tid, r, c, blocks);
            } else if let Some(txt) = cmd.flag("text") {
                core.table_set_cell_blocks(tid, r, c, vec![Block::text(txt)]);
            }

            if let Some(bg_opt) = opt_clear_flag(cmd.flag("bg")) {
                let bg = bg_opt.map(spec::check_block_bg).transpose()?;
                core.select_cells(CellSel { table: tid, anchor: (r, c), head: (r, c), kind: CellSelKind::Range });
                core.table_set_bg(bg);
            }
            if let Some(val_opt) = opt_clear_flag(cmd.flag("valign")) {
                let valign = match val_opt.map(str::to_ascii_lowercase).as_deref() {
                    None | Some("top") => None,
                    Some("middle") | Some("center") => Some("middle"),
                    Some("bottom") => Some("bottom"),
                    Some(other) => return Err(CliFailure::new("INVALID_VALIGN", format!("Unknown --valign '{other}'.")).with_valid(["top", "middle", "bottom", "none"])),
                };
                core.select_cells(CellSel { table: tid, anchor: (r, c), head: (r, c), kind: CellSelKind::Range });
                core.table_set_valign(valign);
            }

            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, tid);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": loc, "row": r, "col": c }),
                &store.warnings, &diags,
                &format!("Updated cell ({r},{c}) in table {}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "table set-widths" => {
            let (tid, _, nc) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let mut core = Core::new(doc);
            if let Some(ws) = cmd.flag("widths") {
                let widths = parse_widths_csv(ws, nc)?;
                for (i, w) in widths.into_iter().enumerate() {
                    core.table_set_width(tid, i, w);
                }
            } else {
                let c: usize = cmd.req_flag("col")?.parse().map_err(|_| CliFailure::usage("Invalid usize for --col"))?;
                let w: f32 = cmd.req_flag("width")?.parse().map_err(|_| CliFailure::usage("Invalid f32 for --width"))?;
                if c >= nc {
                    return Err(CliFailure::new("INDEX_OUT_OF_BOUNDS", format!("Column {c} out of bounds (table has {nc}).")));
                }
                core.table_set_width(tid, c, w.max(Table::MIN_WIDTH));
            }
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let updated_widths = locator::find_block(&core.doc, tid).and_then(|b| if let BlockKind::Table(t) = &b.kind { Some(t.col_widths.clone()) } else { None }).unwrap_or_default();
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "col_widths": updated_widths }),
                &store.warnings, &diags,
                &format!("Updated column widths: {:?}", updated_widths),
            );
            Ok(())
        }
        "table distribute" => {
            let (tid, _, _) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let mut core = Core::new(doc);
            core.table_distribute(tid);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let updated_widths = locator::find_block(&core.doc, tid).and_then(|b| if let BlockKind::Table(t) = &b.kind { Some(t.col_widths.clone()) } else { None }).unwrap_or_default();
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "col_widths": updated_widths }),
                &store.warnings, &diags,
                &format!("Distributed column widths: {:?}", updated_widths),
            );
            Ok(())
        }
        "table merge" => {
            let (tid, nr, nc) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let range = cmd.req_flag("range")?;
            let (lhs, rhs) = range.split_once(':').ok_or_else(|| CliFailure::usage("--range expects r1,c1:r2,c2"))?;
            let (r1, c1) = parse_pair(lhs)?;
            let (r2, c2) = parse_pair(rhs)?;
            if r1 >= nr || r2 >= nr || c1 >= nc || c2 >= nc {
                return Err(CliFailure::new("CELL_OUT_OF_BOUNDS", format!("Merge range {range} exceeds {nr}x{nc} table.")));
            }
            let mut core = Core::new(doc);
            core.select_cells(CellSel { table: tid, anchor: (r1, c1), head: (r2, c2), kind: CellSelKind::Range });
            core.table_merge();
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "merged": range }),
                &store.warnings, &diags,
                &format!("Merged cells {range}"),
            );
            Ok(())
        }
        "table unmerge" => {
            let (tid, nr, nc) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let (r, c) = parse_cell_coord(cmd, nr, nc)?;
            let mut core = Core::new(doc);
            core.select_cells(CellSel { table: tid, anchor: (r, c), head: (r, c), kind: CellSelKind::Range });
            core.table_unmerge();
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "row": r, "col": c }),
                &store.warnings, &diags,
                &format!("Unmerged cell ({r},{c})"),
            );
            Ok(())
        }
        "table insert-row" => {
            let (tid, nr, _) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let at = cmd.flag("at").map(|s| s.parse::<usize>().map_err(|_| CliFailure::usage("Invalid --at"))).transpose()?.unwrap_or(nr).min(nr);
            let mut core = Core::new(doc);
            core.table_insert(tid, false, at);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "inserted_row": at, "rows": nr + 1 }),
                &store.warnings, &diags,
                &format!("Inserted row at index {at}"),
            );
            Ok(())
        }
        "table insert-col" => {
            let (tid, _, nc) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let at = cmd.flag("at").map(|s| s.parse::<usize>().map_err(|_| CliFailure::usage("Invalid --at"))).transpose()?.unwrap_or(nc).min(nc);
            let mut core = Core::new(doc);
            core.table_insert(tid, true, at);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "inserted_col": at, "cols": nc + 1 }),
                &store.warnings, &diags,
                &format!("Inserted column at index {at}"),
            );
            Ok(())
        }
        "table delete-rows" => {
            let (tid, nr, _) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let (a, b) = parse_index_range(cmd.req_flag("row")?, nr, "row")?;
            let mut core = Core::new(doc);
            core.select_cells(CellSel { table: tid, anchor: (a, 0), head: (b, 0), kind: CellSelKind::Rows });
            core.table_delete_selected();
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "deleted_rows": [a, b + 1] }),
                &store.warnings, &diags,
                &format!("Deleted rows {a}..{}", b + 1),
            );
            Ok(())
        }
        "table delete-cols" => {
            let (tid, _, nc) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let (a, b) = parse_index_range(cmd.req_flag("col")?, nc, "col")?;
            let mut core = Core::new(doc);
            core.select_cells(CellSel { table: tid, anchor: (0, a), head: (0, b), kind: CellSelKind::Cols });
            core.table_delete_selected();
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "deleted_cols": [a, b + 1] }),
                &store.warnings, &diags,
                &format!("Deleted columns {a}..{}", b + 1),
            );
            Ok(())
        }
        "table set-header" => {
            let (tid, _, _) = resolve_table(&doc, cmd.req_flag("table")?)?;
            let hr = cmd.bool_flag("header-row")?.map(usize::from);
            let hc = cmd.bool_flag("header-col")?;
            let mut core = Core::new(doc);
            core.table_set_header(tid, hr, hc);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "table_loc": locator::loc_of(&core.doc, tid), "header_rows": hr, "header_col": hc }),
                &store.warnings, &diags,
                "Updated table header flags",
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled table command {}", cmd.command_path))),
    }
}
