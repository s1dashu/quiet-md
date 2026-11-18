//! Structural invariant & aesthetic layout linter for Quiet.md documents.

use crate::locator;
use crate::output::Diagnostic;
use fdl_core::board::bounds::aabb_for_element;
use fdl_core::board::scene::Scene;
use fdl_core::docxml;
use fdl_core::editor::core::{MAX_COLUMNS, MIN_COLUMN_RATIO, validate_document};
use fdl_core::model::*;
use fdl_core::palette;
use fdl_core::store::{Meta, PageWidth, Store};
use std::collections::HashSet;

pub fn lint_document(doc: &Document, meta: Option<&Meta>, store: &Store, known_doc_ids: &HashSet<String>) -> Vec<Diagnostic> {
    let mut diags = vec![];

    // 1. Structural model invariants (`editor::core::validate_document`)
    if let Err(err) = validate_document(doc) {
        diags.push(Diagnostic {
            severity: "error",
            rule: "core-invariant".into(),
            loc: None,
            message: err,
            fix_hint: Some("Rebuild or repair the invalid block structure via `qmd doc apply`.".into()),
        });
    }

    // 2. DocxXML round-trip check
    let xml = docxml::serialize(doc);
    match docxml::parse(&xml) {
        Ok(back) => {
            if docxml::serialize(&back) != xml {
                diags.push(Diagnostic {
                    severity: "error",
                    rule: "docxml-roundtrip".into(),
                    loc: None,
                    message: "DocxXML serialize -> parse -> serialize changed output.".into(),
                    fix_hint: Some("Inspect raw XML blocks or nested empty list items.".into()),
                });
            }
        }
        Err(e) => {
            diags.push(Diagnostic {
                severity: "error",
                rule: "docxml-parse".into(),
                loc: None,
                message: format!("Serialized DocxXML failed to parse back: {e}"),
                fix_hint: None,
            });
        }
    }

    // 3. Cover asset existence
    if let Some(c) = &doc.cover
        && !c.src.starts_with(".assets/.dry-run-")
        && fdl_core::store::image_path_in(store.root(), &c.src).is_none_or(|p| !p.exists())
    {
        diags.push(Diagnostic {
            severity: "error",
            rule: "missing-cover".into(),
            loc: Some("title".into()),
            message: format!("Cover image '{}' does not exist in library.", c.src),
            fix_hint: Some("Set a built-in gallery cover or local image via `qmd meta cover set`.".into()),
        });
    }

    let indexed = locator::index_blocks(doc);
    let loc_for = |id: BlockId| indexed.iter().find(|b| b.id == id).map(|b| b.loc.clone());

    let page_max_w = meta.map(|m| m.width).unwrap_or(PageWidth::Standard).max();
    let mut prev_heading: Option<u8> = None;
    let mut heading_count = 0usize;
    let mut consecutive_long_p = 0usize;
    let mut used_callout_colors: HashSet<String> = HashSet::new();

    doc.for_each_block(|b| {
        let loc = loc_for(b.id);

        // Palette checks for runs
        for (_, m) in b.text.spans() {
            if let Some(tc) = &m.text_color && palette::text_color(&tc.0).is_none() {
                diags.push(Diagnostic {
                    severity: "error",
                    rule: "invalid-palette-color".into(),
                    loc: loc.clone(),
                    message: format!("Unknown text-color '{}'.", tc.0),
                    fix_hint: Some("Run `qmd schema palette --json` for valid font colors.".into()),
                });
            }
            if let Some(hc) = &m.bg_color && palette::bg_color(&hc.0).is_none() {
                diags.push(Diagnostic {
                    severity: "error",
                    rule: "invalid-palette-color".into(),
                    loc: loc.clone(),
                    message: format!("Unknown highlight background-color '{}'.", hc.0),
                    fix_hint: Some("Run `qmd schema palette --json` for valid highlight colors.".into()),
                });
            }
            if let Some(dl) = m.inline.as_ref().and_then(Inline::doc_link) && !known_doc_ids.contains(&dl.id) {
                diags.push(Diagnostic {
                    severity: "warning",
                    rule: "broken-doc-link".into(),
                    loc: loc.clone(),
                    message: format!("@doc link target '{}' ('{}') was not found in the library.", dl.id, dl.title),
                    fix_hint: Some("Verify target document id/path with `qmd library tree`.".into()),
                });
            }
        }

        // Block background check
        if let Some(bg) = &b.bg {
            if !b.kind.takes_bg() {
                diags.push(Diagnostic {
                    severity: "warning",
                    rule: "unsupported-block-bg".into(),
                    loc: loc.clone(),
                    message: format!("Block type '{}' ignores background-color.", locator::block_kind_name(&b.kind)),
                    fix_hint: Some("Wrap blocks inside a callout or remove `bg`.".into()),
                });
            } else if palette::block_bg(&bg.0).is_none() {
                diags.push(Diagnostic {
                    severity: "error",
                    rule: "invalid-palette-color".into(),
                    loc: loc.clone(),
                    message: format!("Unknown block background '{}'.", bg.0),
                    fix_hint: Some("Run `qmd schema palette --json` for valid block background names.".into()),
                });
            } else {
                used_callout_colors.insert(bg.0.clone());
            }
        }

        match &b.kind {
            BlockKind::Text => {
                let len = b.text.plain_text().chars().count();
                if len > 500 {
                    diags.push(Diagnostic {
                        severity: "warning",
                        rule: "dense-wall-of-text".into(),
                        loc: loc.clone(),
                        message: format!("Paragraph has {len} characters; long blocks hurt readability."),
                        fix_hint: Some("Split into concise paragraphs, bullet points or a callout.".into()),
                    });
                }
                if len > 120 { consecutive_long_p += 1; } else { consecutive_long_p = 0; }
                if consecutive_long_p >= 5 {
                    diags.push(Diagnostic {
                        severity: "hint",
                        rule: "dense-wall-of-text".into(),
                        loc: loc.clone(),
                        message: "5+ consecutive long paragraphs without headings, lists or visual breaks.".into(),
                        fix_hint: Some("Insert a heading, divider, callout or 2-column grid to improve visual rhythm.".into()),
                    });
                }
            }
            BlockKind::Heading { level, .. } => {
                consecutive_long_p = 0;
                heading_count += 1;
                if let Some(prev) = prev_heading && *level > prev + 1 {
                    diags.push(Diagnostic {
                        severity: "warning",
                        rule: "heading-hierarchy".into(),
                        loc: loc.clone(),
                        message: format!("Heading jumps from h{prev} directly to h{level}."),
                        fix_hint: Some(format!("Use h{} instead of h{level} for clean outline hierarchy.", prev + 1)),
                    });
                }
                prev_heading = Some(*level);
            }
            BlockKind::Callout { bg, border, text_color, .. } => {
                consecutive_long_p = 0;
                if let Some(c) = bg {
                    if palette::callout_bg(&c.0).is_none() {
                        diags.push(Diagnostic { severity: "error", rule: "invalid-palette-color".into(), loc: loc.clone(), message: format!("Unknown callout bg '{}'.", c.0), fix_hint: None });
                    } else {
                        used_callout_colors.insert(c.0.clone());
                    }
                }
                if let Some(c) = border && palette::callout_border(&c.0).is_none() {
                    diags.push(Diagnostic { severity: "error", rule: "invalid-palette-color".into(), loc: loc.clone(), message: format!("Unknown callout border '{}'.", c.0), fix_hint: None });
                }
                if let Some(c) = text_color && palette::text_color(&c.0).is_none() {
                    diags.push(Diagnostic { severity: "error", rule: "invalid-palette-color".into(), loc: loc.clone(), message: format!("Unknown callout text_color '{}'.", c.0), fix_hint: None });
                }
            }
            BlockKind::Grid => {
                consecutive_long_p = 0;
                let n = b.children.len();
                if !(2..=MAX_COLUMNS).contains(&n) {
                    diags.push(Diagnostic { severity: "error", rule: "grid-columns".into(), loc: loc.clone(), message: format!("Grid has {n} columns (must be 2..={MAX_COLUMNS})."), fix_hint: None });
                } else if n > 4 {
                    diags.push(Diagnostic { severity: "hint", rule: "grid-balance".into(), loc: loc.clone(), message: format!("Grid has {n} columns; >4 columns often feel cramped on Standard page width."), fix_hint: Some("Use 2-3 columns or set page width to `wide` / `full`.".into()) });
                }
                let mut sum = 0.0f32;
                let mut min_blocks = usize::MAX;
                let mut max_blocks = 0usize;
                let mut empty_col = false;
                let mut non_empty_col = false;
                for col in &b.children {
                    let count = col.children.len();
                    min_blocks = min_blocks.min(count);
                    max_blocks = max_blocks.max(count);
                    let has_content = col.children.iter().any(|c| !c.text.plain_text().trim().is_empty() || !matches!(c.kind, BlockKind::Text));
                    if has_content { non_empty_col = true; } else { empty_col = true; }
                    if let BlockKind::Column { ratio } = col.kind {
                        sum += ratio;
                        if ratio < MIN_COLUMN_RATIO - 1e-4 {
                            diags.push(Diagnostic { severity: "error", rule: "grid-min-ratio".into(), loc: loc.clone(), message: format!("Column ratio {ratio:.4} < MIN_COLUMN_RATIO."), fix_hint: None });
                        }
                    }
                }
                if (sum - 1.0).abs() > 0.02 {
                    diags.push(Diagnostic { severity: "warning", rule: "grid-ratio-sum".into(), loc: loc.clone(), message: format!("Grid column ratios sum to {sum:.3} instead of 1.0."), fix_hint: Some("Normalize column ratios to sum to 1.0.".into()) });
                }
                if n >= 2 && ((empty_col && non_empty_col) || (max_blocks >= 4 && max_blocks >= min_blocks.max(1) * 4)) {
                    diags.push(Diagnostic {
                        severity: "hint",
                        rule: "grid-balance".into(),
                        loc: loc.clone(),
                        message: format!("Grid columns have unbalanced vertical density ({min_blocks} vs {max_blocks} blocks, empty column: {empty_col})."),
                        fix_hint: Some("Balance block counts across sibling columns so no column leaves a large empty void.".into()),
                    });
                }
            }
            BlockKind::Quote => {
                consecutive_long_p = 0;
                let mut bad_color: Option<String> = None;
                for (_, m) in b.text.spans() {
                    if let Some(tc) = &m.text_color && tc.0 != "gray" { bad_color = Some(tc.0.clone()); break; }
                }
                if bad_color.is_none() {
                    for ch in &b.children {
                        for (_, m) in ch.text.spans() {
                            if let Some(tc) = &m.text_color && tc.0 != "gray" { bad_color = Some(tc.0.clone()); break; }
                        }
                        if bad_color.is_some() { break; }
                    }
                }
                if let Some(c) = bad_color {
                    diags.push(Diagnostic {
                        severity: "hint",
                        rule: "quote-color-restraint".into(),
                        loc: loc.clone(),
                        message: format!("Quote uses colored text ('{c}'); quote bars already provide emphasis."),
                        fix_hint: Some("Keep quote text in default or gray color and use bold/italic sparingly.".into()),
                    });
                }
            }
            BlockKind::Table(t) => {
                consecutive_long_p = 0;
                let total_w: f32 = t.col_widths.iter().sum();
                if let Some(max_w) = page_max_w && total_w > max_w + 130. {
                    diags.push(Diagnostic {
                        severity: "warning",
                        rule: "table-overflow".into(),
                        loc: loc.clone(),
                        message: format!("Table width {total_w:.0}px exceeds page width ({max_w:.0}px) and will require horizontal scrolling."),
                        fix_hint: Some("Reduce `col_widths` with `qmd table set-widths` or run `qmd meta set --width wide`.".into()),
                    });
                }
                for (_, _, cell) in t.cells() {
                    if let Some(bg) = &cell.bg && palette::block_bg(&bg.0).is_none() {
                        diags.push(Diagnostic { severity: "error", rule: "invalid-palette-color".into(), loc: loc.clone(), message: format!("Unknown table cell bg '{}'.", bg.0), fix_hint: None });
                    }
                }
            }
            BlockKind::Image { src, .. } => {
                consecutive_long_p = 0;
                if !src.starts_with("http://") && !src.starts_with("https://") && !src.starts_with("data:") && !src.starts_with(".assets/.dry-run-")
                    && fdl_core::store::image_path_in(store.root(), src).is_none_or(|p| !p.exists())
                {
                    diags.push(Diagnostic {
                        severity: "error",
                        rule: "missing-asset".into(),
                        loc: loc.clone(),
                        message: format!("Image asset '{src}' does not exist in library."),
                        fix_hint: Some("Import the image via `qmd asset add-image` or pass a valid local path in spec.".into()),
                    });
                }
            }
            BlockKind::File(f) if f.src_kind == "path" && !f.src.starts_with(".assets/.dry-run-") => {
                consecutive_long_p = 0;
                if fdl_core::store::export_path_in(store.root(), &f.src).is_none() {
                    diags.push(Diagnostic {
                        severity: "error",
                        rule: "missing-asset".into(),
                        loc: loc.clone(),
                        message: format!("Attachment '{}' does not exist in library.", f.src),
                        fix_hint: Some("Import the file via `qmd asset add-file`.".into()),
                    });
                }
            }
            BlockKind::Board(bd) => {
                consecutive_long_p = 0;
                check_board_overlap(&bd.elements, loc.as_deref(), &mut diags);
            }
            _ => { consecutive_long_p = 0; }
        }
    });

    let mut prev_top_callout = false;
    for b in &doc.blocks {
        let is_callout = matches!(b.kind, BlockKind::Callout { .. });
        if prev_top_callout && is_callout {
            diags.push(Diagnostic {
                severity: "hint",
                rule: "consecutive-callouts".into(),
                loc: loc_for(b.id),
                message: "Consecutive callout blocks stacked back-to-back weaken visual hierarchy.".into(),
                fix_hint: Some("Separate callouts with prose, or convert secondary callouts to paragraphs, bullets, or a borderless grid.".into()),
            });
        }
        prev_top_callout = is_callout;
    }

    if used_callout_colors.len() > 4 {
        diags.push(Diagnostic {
            severity: "hint",
            rule: "color-restraint".into(),
            loc: None,
            message: format!("Document uses {} distinct background/callout color names; excessive colors can feel busy.", used_callout_colors.len()),
            fix_hint: Some("Stick to 1 primary accent hue + neutral gray/subtle tints for clean aesthetics.".into()),
        });
    }
    if doc.blocks.len() >= 8 && heading_count == 0 {
        diags.push(Diagnostic {
            severity: "hint",
            rule: "doc-polish".into(),
            loc: None,
            message: "Long document has no headings (cannot generate an Outline).".into(),
            fix_hint: Some("Add h1/h2 section headings to structure the document and enable the floating Outline.".into()),
        });
    }

    diags
}

fn check_board_overlap(elements: &[fdl_core::board::types::Element], loc: Option<&str>, diags: &mut Vec<Diagnostic>) {
    let scene = Scene::new(elements.to_vec());
    let boxes: Vec<(&fdl_core::board::types::Element, [f64; 4])> = elements
        .iter()
        .filter(|e| !e.is_deleted && !e.is_frame() && !e.is_arrow() && !e.is_line() && e.container_id().is_none())
        .map(|e| (e, aabb_for_element(e, &scene, None)))
        .collect();
    for i in 0..boxes.len() {
        for j in (i + 1)..boxes.len() {
            let (ea, a) = boxes[i];
            let (eb, b) = boxes[j];
            let ix = (a[2].min(b[2]) - a[0].max(b[0])).max(0.);
            let iy = (a[3].min(b[3]) - a[1].max(b[1])).max(0.);
            if ix > 4. && iy > 4. {
                diags.push(Diagnostic {
                    severity: "warning",
                    rule: "board-overlap".into(),
                    loc: loc.map(str::to_string),
                    message: format!("Board elements '{}' and '{}' overlap by {:.0}x{:.0}px.", ea.id, eb.id, ix, iy),
                    fix_hint: Some("Increase `gap_x`/`gap_y` or use `qmd board build` automatic layout.".into()),
                });
                break;
            }
        }
    }
    let frames: Vec<(&fdl_core::board::types::Element, [f64; 4])> = elements
        .iter()
        .filter(|e| !e.is_deleted && e.is_frame())
        .map(|e| {
            let mut b = aabb_for_element(e, &scene, None);
            b[1] -= 28.;
            (e, b)
        })
        .collect();
    for i in 0..frames.len() {
        for j in (i + 1)..frames.len() {
            let (fa, a) = frames[i];
            let (fb, b) = frames[j];
            let ix = (a[2].min(b[2]) - a[0].max(b[0])).max(0.);
            let iy = (a[3].min(b[3]) - a[1].max(b[1])).max(0.);
            if ix > 4. && iy > 4. {
                diags.push(Diagnostic {
                    severity: "warning",
                    rule: "board-frame-overlap".into(),
                    loc: loc.map(str::to_string),
                    message: format!("Board frames '{}' and '{}' overlap by {:.0}x{:.0}px.", fa.id, fb.id, ix, iy),
                    fix_hint: Some("Use `layout: \"manual\"` with non-overlapping coordinates for multi-frame boards or split into separate boards.".into()),
                });
                return;
            }
        }
    }
}
