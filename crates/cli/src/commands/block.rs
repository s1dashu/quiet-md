//! `qmd block` — fine-grained block mutations through `editor::Core`.

use super::{commit_core, load_doc, opt_clear_flag, parse_block_specs_arg, parse_place};
use crate::args::ParsedCmd;
use crate::assets;
use crate::locator;
use crate::output::{CliFailure, GlobalOpts, print_success};
use crate::spec::{self, BlockSpec, ButtonSpec, DocLinkSpec, ReminderSpec};
use fdl_core::editor::core::{Core, DropPlace, Pos, Selection};
use fdl_core::lang;
use fdl_core::model::*;

fn ensure_not_inside_fold(doc: &Document, target: BlockId) -> Result<(), CliFailure> {
    fn walk(list: &[Block], target: BlockId, folded_anc: Option<BlockId>) -> Result<bool, BlockId> {
        for b in list {
            if b.id == target {
                return match folded_anc {
                    Some(anc) => Err(anc),
                    None => Ok(true),
                };
            }
            let next_fold = folded_anc.or_else(|| b.folded.then_some(b.id));
            if let BlockKind::Table(t) = &b.kind {
                for cell in t.rows.iter().flatten() {
                    if walk(&cell.blocks, target, next_fold)? { return Ok(true); }
                }
            } else if walk(&b.children, target, next_fold)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    if let Err(anc_id) = walk(&doc.blocks, target, None) {
        let anc_loc = locator::loc_of(doc, anc_id).unwrap_or_else(|| "?".into());
        return Err(CliFailure::new("BLOCK_HIDDEN_IN_FOLD", format!("Target block is hidden inside folded ancestor '{anc_loc}'."))
            .with_hint(format!("Run `qmd block fold <doc> --at {anc_loc} --folded false` first.")));
    }
    Ok(())
}

fn parse_crop(s: &str) -> Result<[f32; 4], CliFailure> {
    let nums: Vec<f32> = s.split(',').map(|p| p.trim().parse::<f32>()).collect::<Result<_, _>>()
        .map_err(|_| CliFailure::usage("--crop must be 4 comma-separated floats: l,t,r,b"))?;
    if nums.len() != 4 || !(0.0..1.0).contains(&nums[0]) || !(nums[0] < nums[2] && nums[2] <= 1.0)
        || !(0.0..1.0).contains(&nums[1]) || !(nums[1] < nums[3] && nums[3] <= 1.0) {
        return Err(CliFailure::new("INVALID_CROP", "Crop requires 0 <= l < r <= 1 and 0 <= t < b <= 1."));
    }
    Ok([nums[0], nums[1], nums[2], nums[3]])
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut store = super::open_store(opts)?;
    let root = store.root().to_path_buf();
    let (entry, doc) = load_doc(&mut store, cmd.pos(0, "<doc>")?)?;

    match cmd.command_path.as_str() {
        "block insert" => {
            let anchor = locator::resolve_block(&doc, cmd.flag("at").unwrap_or("last"))?;
            let place = parse_place(cmd.flag("place"))?;
            let specs = if let Some(sp) = cmd.flag("spec") {
                parse_block_specs_arg(sp)?
            } else {
                let kind = cmd.flag("type").unwrap_or("p").to_string();
                let view = cmd.flag("view").map(str::to_string);
                vec![BlockSpec {
                    kind,
                    text: cmd.flag("text").map(str::to_string),
                    lang: cmd.flag("lang").map(str::to_string),
                    caption: cmd.flag("caption").map(str::to_string),
                    wrap: cmd.bool_flag("wrap")?,
                    emoji: cmd.flag("emoji").map(str::to_string),
                    bg: cmd.flag("bg").map(str::to_string),
                    border: cmd.flag("border").map(str::to_string),
                    text_color: cmd.flag("text-color").map(str::to_string),
                    align: cmd.flag("align").map(str::to_string),
                    seq_auto: cmd.bool_flag("seq-auto")?,
                    done: cmd.bool_flag("done")?,
                    src: cmd.flag("src").map(str::to_string),
                    width: cmd.flag("width").map(|w| w.parse::<f32>().map_err(|_| CliFailure::usage("Invalid f32 for --width"))).transpose()?,
                    view,
                    ..Default::default()
                }]
            };
            if specs.is_empty() {
                return Err(CliFailure::usage("No blocks specified to insert."));
            }
            let blocks: Vec<Block> = specs.iter().map(|s| spec::compile_block(s, &mut store, opts.dry_run)).collect::<Result<_, _>>()?;
            let inserted_ids: Vec<BlockId> = blocks.iter().map(|b| b.id).collect();
            let mut core = Core::new(doc);
            core.insert_blocks_at(blocks, anchor, place);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let inserted_locs: Vec<_> = inserted_ids.iter().filter_map(|id| locator::loc_of(&core.doc, *id)).collect();
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "inserted_locs": inserted_locs, "block_count": core.doc.blocks.len() }),
                &store.warnings, &diags,
                &format!("Inserted {} block(s) into '{}' at {:?}", inserted_locs.len(), entry.title, inserted_locs),
            );
            Ok(())
        }
        "block update" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            let existing = locator::find_block(&doc, target).cloned().ok_or_else(|| CliFailure::new("BLOCK_NOT_FOUND", "Target block not found."))?;
            let mut core = Core::new(doc);
            let mut resulting_id = target;

            if let Some(sp) = cmd.flag("spec") {
                let mut specs = parse_block_specs_arg(sp)?;
                if specs.len() != 1 {
                    return Err(CliFailure::usage("`block update --spec` expects a single BlockSpec JSON object."));
                }
                let replacement = spec::compile_block(&specs.remove(0), &mut store, opts.dry_run)?;
                resulting_id = replacement.id;
                core.insert_blocks_at(vec![replacement], target, DropPlace::After);
                core.delete_block(target);
            } else {
                if let Some(txt) = cmd.flag("text") {
                    if !existing.kind.has_text() {
                        return Err(CliFailure::new("UNSUPPORTED_FIELD", format!("Block kind '{}' does not have text.", locator::block_kind_name(&existing.kind))));
                    }
                    core.set_block_text(target, RichText::plain(txt));
                }
                if let BlockKind::Callout { emoji, bg, border, text_color } = &existing.kind {
                    let has_co = cmd.has_flag("emoji") || cmd.has_flag("bg") || cmd.has_flag("border") || cmd.has_flag("text-color");
                    if has_co {
                        let new_emoji = cmd.flag("emoji").map(assets::resolve_emoji).transpose()?.unwrap_or_else(|| emoji.clone());
                        let new_bg = match opt_clear_flag(cmd.flag("bg")) {
                            Some(None) => None,
                            Some(Some(c)) => Some(spec::check_callout_bg(c)?),
                            None => bg.clone(),
                        };
                        let new_border = match opt_clear_flag(cmd.flag("border")) {
                            Some(None) => None,
                            Some(Some(c)) => Some(spec::check_callout_border(c)?),
                            None => border.clone(),
                        };
                        let new_tc = match opt_clear_flag(cmd.flag("text-color")) {
                            Some(None) => None,
                            Some(Some(c)) => Some(spec::check_text_color(c)?),
                            None => text_color.clone(),
                        };
                        core.set_callout(target, new_emoji, new_bg, new_border, new_tc);
                    }
                } else if let Some(bg_opt) = opt_clear_flag(cmd.flag("bg")) {
                    let new_bg = bg_opt.map(spec::check_block_bg).transpose()?;
                    if new_bg.is_some() && !existing.kind.takes_bg() {
                        return Err(CliFailure::new("UNSUPPORTED_FIELD", format!("Block kind '{}' ignores background color.", locator::block_kind_name(&existing.kind))));
                    }
                    core.set_block_bg(&[target], new_bg);
                } else if cmd.has_flag("emoji") || cmd.has_flag("border") || cmd.has_flag("text-color") {
                    return Err(CliFailure::new("UNSUPPORTED_FIELD", "Flags --emoji/--border/--text-color only apply to callout blocks (use `block style` for inline text color)."));
                }

                if let BlockKind::Code { lang: old_l, caption: old_c, wrap: old_w } = &existing.kind {
                    if cmd.has_flag("lang") || cmd.has_flag("caption") || cmd.has_flag("wrap") {
                        let new_l = match cmd.flag("lang") {
                            Some(l) => lang::resolve(l).to_string(),
                            None => old_l.clone(),
                        };
                        let new_c = match opt_clear_flag(cmd.flag("caption")) {
                            Some(None) => None,
                            Some(Some(c)) => Some(c.to_string()),
                            None => old_c.clone(),
                        };
                        let new_w = cmd.bool_flag("wrap")?.unwrap_or(*old_w);
                        core.set_code_props(target, new_l, new_c, new_w);
                    }
                } else if let BlockKind::Image { width, height, caption, crop, .. } = &existing.kind {
                    if cmd.has_flag("width") || cmd.has_flag("height") || cmd.has_flag("caption") || cmd.has_flag("crop") {
                        let w = match opt_clear_flag(cmd.flag("width")) {
                            Some(None) => None,
                            Some(Some(v)) => Some(v.parse::<f32>().map_err(|_| CliFailure::usage("Invalid f32 for --width"))?),
                            None => *width,
                        };
                        let h = match opt_clear_flag(cmd.flag("height")) {
                            Some(None) => None,
                            Some(Some(v)) => Some(v.parse::<f32>().map_err(|_| CliFailure::usage("Invalid f32 for --height"))?),
                            None => *height,
                        };
                        let cap = match opt_clear_flag(cmd.flag("caption")) {
                            Some(None) => None,
                            Some(Some(c)) => Some(c.to_string()),
                            None => caption.clone(),
                        };
                        let cr = match opt_clear_flag(cmd.flag("crop")) {
                            Some(None) => None,
                            Some(Some(c)) => Some(parse_crop(c)?),
                            None => *crop,
                        };
                        core.set_image_props(target, w, h, cap, cr);
                    }
                } else if matches!(existing.kind, BlockKind::File(_))
                    && let Some(v) = cmd.flag("view")
                {
                    let view = match v.to_ascii_lowercase().as_str() {
                        "card" => FileView::Card,
                        "preview" => FileView::Preview,
                        other => return Err(CliFailure::new("INVALID_FILE_VIEW", format!("Unknown --view '{other}'.")).with_valid(["card", "preview"])),
                    };
                    core.set_file_view(target, view);
                }

                if let Some(sa) = cmd.bool_flag("seq-auto")?
                    && !core.set_heading_seq_auto(target, sa)
                {
                    return Err(CliFailure::new("UNSUPPORTED_FIELD", "--seq-auto only applies to heading blocks."));
                }
                if let Some(seq_opt) = opt_clear_flag(cmd.flag("seq")) {
                    let seq_val = seq_opt.map(|s| s.parse::<u32>().map_err(|_| CliFailure::usage("Invalid u32 for --seq"))).transpose()?;
                    if !core.set_ordered_seq(target, seq_val) {
                        return Err(CliFailure::new("UNSUPPORTED_FIELD", "--seq only applies to ordered list blocks."));
                    }
                }
                if let Some(done) = cmd.bool_flag("done")?
                    && !core.set_todo_done(target, done)
                {
                    return Err(CliFailure::new("UNSUPPORTED_FIELD", "--done only applies to todo blocks."));
                }
                if let Some(al) = cmd.flag("align") {
                    if !(existing.kind.takes_bg() || matches!(existing.kind, BlockKind::Image { .. } | BlockKind::File(_))) {
                        return Err(CliFailure::new("UNSUPPORTED_FIELD", format!("Block kind '{}' ignores alignment.", locator::block_kind_name(&existing.kind))));
                    }
                    core.set_align(&[target], spec::parse_align(Some(al))?);
                }
            }

            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, resulting_id);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc }),
                &store.warnings, &diags,
                &format!("Updated block {} in '{}'", loc.as_deref().unwrap_or("?"), entry.title),
            );
            Ok(())
        }
        "block style" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            ensure_not_inside_fold(&doc, target)?;
            let b = locator::find_block(&doc, target).ok_or_else(|| CliFailure::new("BLOCK_NOT_FOUND", "Target block not found."))?;
            if !b.kind.has_text() || matches!(b.kind, BlockKind::Code { .. }) {
                return Err(CliFailure::new("UNSUPPORTED_FIELD", "Target block does not support inline formatting marks."));
            }
            let txt_str = b.text.text.clone();
            let bold = cmd.bool_flag("bold")?;
            let italic = cmd.bool_flag("italic")?;
            let underline = cmd.bool_flag("underline")?;
            let strike = cmd.bool_flag("strike")?;
            let code = cmd.bool_flag("code")?;
            let link = cmd.flag("link").map(|s| if s.is_empty() { None } else { Some(s.to_string()) });
            let text_color = match opt_clear_flag(cmd.flag("text-color")) {
                Some(None) => Some(None),
                Some(Some(c)) => Some(Some(spec::check_text_color(c)?)),
                None => None,
            };
            let bg_color = match opt_clear_flag(cmd.flag("bg-color")) {
                Some(None) => Some(None),
                Some(Some(c)) => Some(Some(spec::check_highlight_color(c)?)),
                None => None,
            };

            let ranges: Vec<(usize, usize)> = if let Some(r_str) = cmd.flag("range") {
                let (a, z) = r_str.split_once("..").ok_or_else(|| CliFailure::usage("--range expects start..end"))?;
                let s: usize = a.trim().parse().map_err(|_| CliFailure::usage("Invalid range start"))?;
                let e: usize = z.trim().parse().map_err(|_| CliFailure::usage("Invalid range end"))?;
                if s >= e || e > txt_str.len() || !txt_str.is_char_boundary(s) || !txt_str.is_char_boundary(e) {
                    return Err(CliFailure::new("INVALID_RANGE", format!("Range {s}..{e} out of bounds or not UTF-8 char boundaries for block length {}.", txt_str.len())));
                }
                vec![(s, e)]
            } else if let Some(pat) = cmd.flag("match") {
                if pat.is_empty() {
                    return Err(CliFailure::usage("--match substring cannot be empty."));
                }
                let found: Vec<(usize, usize)> = txt_str.match_indices(pat).map(|(i, m)| (i, i + m.len())).collect();
                if found.is_empty() {
                    return Err(CliFailure::new("MATCH_NOT_FOUND", format!("Substring '{pat}' not found in block.")));
                }
                found
            } else {
                vec![(0, txt_str.len())]
            };

            let mut core = Core::new(doc);
            let mut applied_count = 0usize;
            for (s, e) in &ranges {
                if *s < *e {
                    core.sel = Selection { anchor: Pos::new(target, *s), head: Pos::new(target, *e) };
                    core.apply_marks(|m| {
                        if let Some(v) = bold { m.bold = v; }
                        if let Some(v) = italic { m.italic = v; }
                        if let Some(v) = underline { m.underline = v; }
                        if let Some(v) = strike { m.strike = v; }
                        if let Some(v) = code { m.code = v; }
                        if let Some(l) = &link { m.link = l.clone(); }
                        if let Some(tc) = &text_color { m.text_color = tc.clone(); }
                        if let Some(hc) = &bg_color { m.bg_color = hc.clone(); }
                    });
                    applied_count += 1;
                }
            }
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, target);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc, "spans_styled": applied_count }),
                &store.warnings, &diags,
                &format!("Styled {applied_count} range(s) in block {}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "block inline" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            ensure_not_inside_fold(&doc, target)?;
            let b = locator::find_block(&doc, target).ok_or_else(|| CliFailure::new("BLOCK_NOT_FOUND", "Target block not found."))?;
            if !b.kind.has_text() || matches!(b.kind, BlockKind::Code { .. }) {
                return Err(CliFailure::new("UNSUPPORTED_FIELD", "Target block does not support inline objects."));
            }
            let txt_len = b.text.text.len();
            let offset = match cmd.flag("offset") {
                Some(o) => {
                    let v: usize = o.parse().map_err(|_| CliFailure::usage("Invalid byte offset for --offset"))?;
                    if v > txt_len || !b.text.text.is_char_boundary(v) {
                        return Err(CliFailure::new("INVALID_OFFSET", format!("Offset {v} invalid for block length {txt_len}.")));
                    }
                    v
                }
                None => txt_len,
            };
            let kind = cmd.req_flag("kind")?.to_ascii_lowercase();
            let mut core = Core::new(doc);
            core.sel = Selection::caret(Pos::new(target, offset));
            match kind.as_str() {
                "equation" => {
                    let latex = cmd.flag("latex").ok_or_else(|| CliFailure::usage("--latex required for --kind equation"))?;
                    core.insert_equation(latex);
                }
                "button" => {
                    let rs = ButtonSpec {
                        text: cmd.flag("text").unwrap_or("Button").to_string(),
                        action: cmd.flag("action").map(str::to_string),
                        src: cmd.flag("src").map(str::to_string),
                        color: cmd.flag("color").map(str::to_string),
                    };
                    let action = match rs.action.as_deref().unwrap_or("OpenLink") {
                        "OpenLink" => ButtonAction::OpenLink,
                        "DuplicatePage" => ButtonAction::DuplicatePage,
                        "FollowPage" => ButtonAction::FollowPage,
                        other => return Err(CliFailure::new("INVALID_BUTTON_ACTION", format!("Unknown button action '{other}'."))),
                    };
                    core.insert_button(&InlineButton {
                        text: rs.text,
                        action,
                        src: rs.src.unwrap_or_default(),
                        color: rs.color.unwrap_or_else(|| BUTTON_DEFAULT_COLOR.into()),
                    });
                }
                "reminder" => {
                    let rem = ReminderSpec { date: cmd.flag("date").map(str::to_string), time: cmd.flag("time").map(str::to_string), ..Default::default() };
                    let rt = spec::compile_rich_text(&[crate::spec::RunSpec { reminder: Some(rem), ..Default::default() }], &mut store, opts.dry_run)?;
                    if let Some(inl) = rt.runs.first().and_then(|r| r.marks.inline.as_ref()).and_then(Inline::reminder) {
                        core.insert_reminder(inl);
                    }
                }
                "doc-link" | "doc_link" => {
                    let dl = DocLinkSpec { target: cmd.flag("target").ok_or_else(|| CliFailure::usage("--target required for --kind doc-link"))?.to_string(), title: cmd.flag("text").map(str::to_string) };
                    let target_entry = locator::resolve_doc(&mut store, &dl.target)?;
                    core.insert_doc_link(&InlineDocLink { id: target_entry.id, title: dl.title.unwrap_or(target_entry.title) });
                }
                other => return Err(CliFailure::new("INVALID_INLINE_KIND", format!("Unknown inline kind '{other}'.")).with_valid(["equation", "button", "reminder", "doc-link"])),
            }
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, target);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc, "inline_kind": kind }),
                &store.warnings, &diags,
                &format!("Inserted {kind} in block {}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "block turn-into" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            let to = cmd.req_flag("to")?.to_ascii_lowercase();
            let code_lang = cmd.flag("lang").unwrap_or("").to_string();
            let new_kind = match to.as_str() {
                "p" | "text" | "paragraph" => BlockKind::Text,
                "h1" => BlockKind::Heading { level: 1, seq_auto: false },
                "h2" => BlockKind::Heading { level: 2, seq_auto: false },
                "h3" => BlockKind::Heading { level: 3, seq_auto: false },
                "h4" => BlockKind::Heading { level: 4, seq_auto: false },
                "h5" => BlockKind::Heading { level: 5, seq_auto: false },
                "h6" => BlockKind::Heading { level: 6, seq_auto: false },
                "h7" => BlockKind::Heading { level: 7, seq_auto: false },
                "h8" => BlockKind::Heading { level: 8, seq_auto: false },
                "h9" => BlockKind::Heading { level: 9, seq_auto: false },
                "bullet" | "ul" => BlockKind::Bullet,
                "ordered" | "ol" => BlockKind::Ordered { seq: None },
                "todo" => BlockKind::Todo { done: false },
                "code" => BlockKind::Code { lang: code_lang, caption: None, wrap: false },
                "quote" => BlockKind::Quote,
                "callout" => BlockKind::Callout { emoji: "💡".into(), bg: None, border: None, text_color: None },
                other => return Err(CliFailure::new("INVALID_BLOCK_TYPE", format!("Cannot turn_into '{other}'."))
                    .with_valid(["p", "h1", "h2", "h3", "h4", "h5", "h6", "h7", "h8", "h9", "bullet", "ordered", "todo", "code", "quote", "callout"])),
            };
            let mut core = Core::new(doc);
            core.turn_into(&[target], new_kind);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, target);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc, "kind": to }),
                &store.warnings, &diags,
                &format!("Converted block {} to {to}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "block move" => {
            let src_id = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            let dst_id = locator::resolve_block(&doc, cmd.req_flag("to")?)?;
            let place = parse_place(cmd.flag("place"))?;
            let mut core = Core::new(doc);
            if !core.move_blocks(&[src_id], dst_id, place) {
                return Err(CliFailure::new("MOVE_REJECTED", "Cannot move block to target location (self-nesting or structural rule)."));
            }
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, src_id);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc }),
                &store.warnings, &diags,
                &format!("Moved block to {}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "block indent" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            ensure_not_inside_fold(&doc, target)?;
            let mut core = Core::new(doc);
            if !core.can_indent(target) {
                return Err(CliFailure::new("CANNOT_INDENT", "Block cannot be indented under its previous sibling (no nestable previous sibling or max depth reached)."));
            }
            core.sel = Selection::caret(Pos::new(target, 0));
            core.indent();
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, target);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc }),
                &store.warnings, &diags,
                &format!("Indented block -> {}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "block outdent" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            ensure_not_inside_fold(&doc, target)?;
            let mut core = Core::new(doc);
            if !core.can_outdent(target) {
                return Err(CliFailure::new("CANNOT_OUTDENT", "Block is already at top level or inside a table cell and cannot be outdented."));
            }
            core.sel = Selection::caret(Pos::new(target, 0));
            core.outdent();
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let loc = locator::loc_of(&core.doc, target);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": loc }),
                &store.warnings, &diags,
                &format!("Outdented block -> {}", loc.as_deref().unwrap_or("?")),
            );
            Ok(())
        }
        "block fold" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            let want = cmd.bool_flag("folded")?;
            let mut core = Core::new(doc);
            if !core.can_fold(target) {
                return Err(CliFailure::new("CANNOT_FOLD", "Block cannot be folded (must be foldable heading or block with children)."));
            }
            let cur = core.block(target).is_some_and(|b| b.folded);
            if want.is_none() || want == Some(!cur) {
                core.toggle_fold(target);
            }
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let now_folded = core.block(target).is_some_and(|b| b.folded);
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": locator::loc_of(&core.doc, target), "folded": now_folded }),
                &store.warnings, &diags,
                &format!("Set block folded={now_folded}"),
            );
            Ok(())
        }
        "block toggle-todo" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            let mut core = Core::new(doc);
            core.toggle_todo(target);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            let done = core.block(target).and_then(|b| if let BlockKind::Todo { done } = b.kind { Some(done) } else { None });
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "loc": locator::loc_of(&core.doc, target), "done": done }),
                &store.warnings, &diags,
                &format!("Todo done={}", done.unwrap_or(false)),
            );
            Ok(())
        }
        "block delete" => {
            let target = locator::resolve_block(&doc, cmd.req_flag("at")?)?;
            let mut core = Core::new(doc);
            core.delete_block(target);
            let diags = commit_core(&mut store, &entry, &core, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "deleted": true, "block_count": core.doc.blocks.len() }),
                &store.warnings, &diags,
                &format!("Deleted block from '{}'", entry.title),
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled block command {}", cmd.command_path))),
    }
}
