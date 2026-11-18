//! Declarative JSON Document Spec compiler & reverse serializer.
//! Compiles `DocSpec` / `BlockSpec` / `RunSpec` into validated `fdl_core::model::{Document, Block, RichText}`
//! (and `Meta` patches), and serializes existing `Document`s back into the exact same JSON shape with `loc`.

use crate::assets;
use crate::board_builder::{self, DiagramSpec};
use crate::locator;
use crate::output::CliFailure;
use fdl_core::docxml;
use fdl_core::editor::core::{MAX_COLUMNS, MIN_COLUMN_RATIO, validate_document};
use fdl_core::lang;
use fdl_core::model::*;
use fdl_core::palette;
use fdl_core::store::{Meta, PageWidth, Store};
use fdl_core::whiteboard::Board;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DocSpec {
    pub title: Option<String>,
    pub title_runs: Option<Vec<RunSpec>>,
    pub icon: Option<String>,
    pub cover: Option<CoverSpec>,
    pub meta: Option<MetaSpec>,
    pub blocks: Vec<BlockSpec>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CoverSpec {
    pub gallery: Option<String>,
    pub file: Option<String>,
    pub src: Option<String>,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct MetaSpec {
    pub width: Option<String>, // "standard" | "wide" | "full"
    pub pinned: Option<bool>,
    pub starred: Option<bool>,
    pub reading: Option<bool>,
    pub catalogue_open: Option<bool>,
    pub refs_hidden: Option<bool>,
    pub info_hidden: Option<bool>,
    pub info_created: Option<bool>,
    pub info_modified: Option<bool>,
    pub order: Option<u64>,
    pub root_order: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RunSpec {
    pub text: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub code: Option<bool>,
    pub link: Option<String>,
    pub text_color: Option<String>,
    pub bg_color: Option<String>,
    pub equation: Option<String>,
    pub button: Option<ButtonSpec>,
    pub reminder: Option<ReminderSpec>,
    pub doc_link: Option<DocLinkSpec>,
    pub file: Option<InlineFileSpec>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ButtonSpec {
    pub text: String,
    pub action: Option<String>, // "OpenLink" | "DuplicatePage" | "FollowPage"
    pub src: Option<String>,
    pub color: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ReminderSpec {
    pub date: Option<String>,     // "YYYY-MM-DD"
    pub time: Option<String>,     // "HH:MM"
    pub expire: Option<i64>,      // unix ms
    pub notify: Option<i64>,      // unix ms
    pub should_notify: Option<bool>,
    pub whole_day: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DocLinkSpec {
    pub target: String, // UUID, rel path, or exact title
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct InlineFileSpec {
    pub src: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct BlockSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loc: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq_auto: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<Vec<RunSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<BlockSpec>>,
    // code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrap: Option<bool>,
    // callout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    // image
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<[f32; 4]>,
    // file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    // grid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnSpec>>,
    // table
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col_widths: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_rows: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_col: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Vec<TableCellSpec>>>,
    // countdown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub countdown: Option<CountdownSpec>,
    // board
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagram: Option<DiagramSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene: Option<serde_json::Value>,
    // raw
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xml: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ColumnSpec {
    pub ratio: Option<f32>,
    pub children: Vec<BlockSpec>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TableCellSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<BlockSpec>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valign: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colspan: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rowspan: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CountdownSpec {
    pub date: Option<String>,
    pub time: Option<String>,
    pub duration: Option<f64>,
    pub start: Option<u64>,
    pub notify: Option<bool>,
    pub color: Option<String>,
    pub by_date: Option<bool>,
    pub dhms: Option<[u32; 4]>,
}

pub fn check_text_color(name: &str) -> Result<ColorName, CliFailure> {
    if palette::FONT_COLORS.iter().filter_map(|(n, _)| *n).any(|n| n == name) {
        Ok(ColorName(name.into()))
    } else {
        Err(CliFailure::new("INVALID_COLOR", format!("Unknown text_color '{name}'."))
            .with_valid(palette::FONT_COLORS.iter().filter_map(|(n, _)| *n)))
    }
}

pub fn check_highlight_color(name: &str) -> Result<ColorName, CliFailure> {
    if palette::BG_COLORS.iter().flatten().any(|n| *n == name) {
        Ok(ColorName(name.into()))
    } else {
        Err(CliFailure::new("INVALID_COLOR", format!("Unknown highlight bg_color '{name}'."))
            .with_valid(palette::BG_COLORS.iter().flatten().copied()))
    }
}

pub fn check_block_bg(name: &str) -> Result<ColorName, CliFailure> {
    if palette::CELL_BG_COLORS.iter().flatten().any(|n| *n == name) {
        Ok(ColorName(name.into()))
    } else {
        Err(CliFailure::new("INVALID_COLOR", format!("Unknown block/cell background '{name}'."))
            .with_valid(palette::CELL_BG_COLORS.iter().flatten().copied()))
    }
}

pub fn check_callout_bg(name: &str) -> Result<ColorName, CliFailure> {
    if palette::CALLOUT_BG_COLORS.iter().flatten().any(|n| *n == name) {
        Ok(ColorName(name.into()))
    } else {
        Err(CliFailure::new("INVALID_COLOR", format!("Unknown callout bg '{name}'."))
            .with_valid(palette::CALLOUT_BG_COLORS.iter().flatten().copied()))
    }
}

pub fn check_callout_border(name: &str) -> Result<ColorName, CliFailure> {
    if palette::callout_border(name).is_some() {
        Ok(ColorName(name.into()))
    } else {
        Err(CliFailure::new("INVALID_COLOR", format!("Unknown callout border '{name}'."))
            .with_valid(palette::HUES))
    }
}

pub fn parse_align(s: Option<&str>) -> Result<Align, CliFailure> {
    match s.unwrap_or("left").to_ascii_lowercase().as_str() {
        "left" => Ok(Align::Left),
        "center" => Ok(Align::Center),
        "right" => Ok(Align::Right),
        other => Err(CliFailure::new("INVALID_ALIGN", format!("Unknown align '{other}'.")).with_valid(["left", "center", "right"])),
    }
}

pub fn parse_width(s: &str) -> Result<PageWidth, CliFailure> {
    match s.trim().to_ascii_lowercase().as_str() {
        "standard" => Ok(PageWidth::Standard),
        "wide" => Ok(PageWidth::Wide),
        "full" => Ok(PageWidth::Full),
        other => Err(CliFailure::new("INVALID_PAGE_WIDTH", format!("Unknown page width '{other}'.")).with_valid(["standard", "wide", "full"])),
    }
}

pub fn parse_date_time_ms(date: &str, time: Option<&str>) -> Result<(i64, bool), CliFailure> {
    let parts: Vec<i64> = date.trim().split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return Err(CliFailure::new("INVALID_DATE", format!("Expected YYYY-MM-DD, got '{date}'.")));
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    if !(1970..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(CliFailure::new("INVALID_DATE", format!("Out-of-range date '{date}'.")));
    }
    let (hh, mm, whole_day) = match time.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            let tp: Vec<i64> = t.split(':').filter_map(|p| p.parse().ok()).collect();
            if tp.len() != 2 || !(0..=23).contains(&tp[0]) || !(0..=59).contains(&tp[1]) {
                return Err(CliFailure::new("INVALID_TIME", format!("Expected HH:MM, got '{t}'.")));
            }
            (tp[0], tp[1], false)
        }
        None => (23, 59, true),
    };
    let days = days_from_civil(y, m as u32, d as u32);
    let local_secs = days * 86_400 + hh * 3600 + mm * 60;
    let offset_secs = local_utc_offset_secs(local_secs);
    Ok(((local_secs - offset_secs) * 1000, whole_day))
}

fn days_from_civil(mut y: i64, m: u32, d: u32) -> i64 {
    y -= (m <= 2) as i64;
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i64;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn local_utc_offset_secs(approx_secs: i64) -> i64 {
    let probe = approx_secs.max(0) as u64;
    let (cy, cm, cd, ch, cmi) = fdl_core::time::civil(probe);
    let civil_days = days_from_civil(cy, cm, cd);
    let civil_secs = civil_days * 86_400 + ch as i64 * 3600 + cmi as i64 * 60;
    civil_secs - (probe as i64 - (probe as i64 % 60))
}

pub fn compile_rich_text(runs: &[RunSpec], store: &mut Store, dry_run: bool) -> Result<RichText, CliFailure> {
    let mut rt = RichText::default();
    for r in runs {
        if let Some(src) = &r.equation {
            let inl = docxml::to_inline(InlineKind::Equation(src.clone()));
            let marks = Marks {
                bold: r.bold.unwrap_or(false),
                italic: r.italic.unwrap_or(false),
                underline: r.underline.unwrap_or(false),
                strike: r.strike.unwrap_or(false),
                text_color: r.text_color.as_deref().map(check_text_color).transpose()?,
                bg_color: r.bg_color.as_deref().map(check_highlight_color).transpose()?,
                inline: Some(inl),
                ..Default::default()
            };
            rt.push("\u{fffc}", marks);
            continue;
        }
        if let Some(btn) = &r.button {
            let action = match btn.action.as_deref().unwrap_or("OpenLink") {
                "OpenLink" => ButtonAction::OpenLink,
                "DuplicatePage" => ButtonAction::DuplicatePage,
                "FollowPage" => ButtonAction::FollowPage,
                other => return Err(CliFailure::new("INVALID_BUTTON_ACTION", format!("Unknown button action '{other}'.")).with_valid(["OpenLink", "DuplicatePage", "FollowPage"])),
            };
            let color = btn.color.as_deref().unwrap_or(BUTTON_DEFAULT_COLOR).to_string();
            let inl = docxml::to_inline(InlineKind::Button(InlineButton {
                text: btn.text.clone(),
                action,
                src: btn.src.clone().unwrap_or_default(),
                color,
            }));
            rt.push("\u{fffc}", Marks { inline: Some(inl), ..Default::default() });
            continue;
        }
        if let Some(rem) = &r.reminder {
            let (expire, notify, whole_day) = if let Some(exp) = rem.expire {
                (exp, rem.notify.unwrap_or(exp), rem.whole_day.unwrap_or(false))
            } else if let Some(date) = &rem.date {
                let (exp, wd) = parse_date_time_ms(date, rem.time.as_deref())?;
                let whole = rem.whole_day.unwrap_or(wd);
                let not = if let Some(n) = rem.notify {
                    n
                } else if whole {
                    parse_date_time_ms(date, Some("09:00"))?.0
                } else {
                    exp
                };
                (exp, not, whole)
            } else {
                return Err(CliFailure::new("INVALID_REMINDER", "Reminder requires either `date` (YYYY-MM-DD) or `expire` (unix ms)."));
            };
            let inl = docxml::to_inline(InlineKind::Reminder(InlineReminder {
                expire,
                notify,
                should_notify: rem.should_notify.unwrap_or(true),
                whole_day,
            }));
            rt.push("\u{fffc}", Marks { inline: Some(inl), ..Default::default() });
            continue;
        }
        if let Some(dl) = &r.doc_link {
            let target = locator::resolve_doc(store, &dl.target)?;
            let inl = docxml::to_inline(InlineKind::DocLink(InlineDocLink {
                id: target.id,
                title: dl.title.clone().unwrap_or(target.title),
            }));
            rt.push("\u{fffc}", Marks { inline: Some(inl), ..Default::default() });
            continue;
        }
        if let Some(f) = &r.file {
            let (src, name) = resolve_asset_src(store, &f.src, f.name.as_deref(), true, dry_run)?;
            let inl = docxml::to_inline(InlineKind::File(FileBlock { src_kind: "path".into(), src, name, view: FileView::Card }));
            rt.push("\u{fffc}", Marks { inline: Some(inl), ..Default::default() });
            continue;
        }
        let text = r.text.as_deref().unwrap_or("");
        let marks = Marks {
            bold: r.bold.unwrap_or(false),
            italic: r.italic.unwrap_or(false),
            underline: r.underline.unwrap_or(false),
            strike: r.strike.unwrap_or(false),
            code: r.code.unwrap_or(false),
            link: r.link.clone(),
            text_color: r.text_color.as_deref().map(check_text_color).transpose()?,
            bg_color: r.bg_color.as_deref().map(check_highlight_color).transpose()?,
            inline: None,
        };
        rt.push(text, marks);
    }
    Ok(rt)
}

fn resolve_asset_src(store: &Store, src: &str, name_override: Option<&str>, is_attachment: bool, dry_run: bool) -> Result<(String, String), CliFailure> {
    let trimmed = src.trim();
    if trimmed.starts_with(".assets/") || trimmed.starts_with("assets/") {
        let fallback_name = name_override.map(str::to_string).unwrap_or_else(|| trimmed.rsplit('/').next().unwrap_or("file").to_string());
        return Ok((trimmed.to_string(), fallback_name));
    }
    let p = Path::new(trimmed);
    if is_attachment {
        let (rel, name) = assets::store_attachment_file(store, p, dry_run)?;
        Ok((rel, name_override.map(str::to_string).unwrap_or(name)))
    } else {
        let (rel, _, _) = assets::store_image_file(store, p, dry_run)?;
        let name = name_override.map(str::to_string).unwrap_or_else(|| p.file_name().and_then(|s| s.to_str()).unwrap_or("image").to_string());
        Ok((rel, name))
    }
}

pub fn compile_block(spec: &BlockSpec, store: &mut Store, dry_run: bool) -> Result<Block, CliFailure> {
    let text = match &spec.runs {
        Some(runs) => compile_rich_text(runs, store, dry_run)?,
        None => RichText::plain(spec.text.as_deref().unwrap_or("")),
    };
    let align = parse_align(spec.align.as_deref())?;
    let bg = spec.bg.as_deref();
    let folded = spec.folded.unwrap_or(false);
    let compile_kids = |list: &Option<Vec<BlockSpec>>, store: &mut Store| -> Result<Vec<Block>, CliFailure> {
        list.as_deref().unwrap_or(&[]).iter().map(|c| compile_block(c, store, dry_run)).collect()
    };

    let k = spec.kind.to_ascii_lowercase();
    let mut block = match k.as_str() {
        "p" | "text" | "paragraph" => {
            let kids = compile_kids(&spec.children, store)?;
            Block::new(BlockKind::Text, text).with_children(kids)
        }
        "heading" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "h7" | "h8" | "h9" => {
            let lvl = if k == "heading" { spec.level.unwrap_or(1) } else { k[1..].parse().unwrap_or(1) };
            if !(1..=9).contains(&lvl) {
                return Err(CliFailure::new("INVALID_HEADING_LEVEL", format!("Heading level {lvl} must be 1..=9.")));
            }
            let kids = compile_kids(&spec.children, store)?;
            Block::new(BlockKind::Heading { level: lvl, seq_auto: spec.seq_auto.unwrap_or(false) }, text).with_children(kids)
        }
        "bullet" | "ul" => {
            let kids = compile_kids(&spec.children, store)?;
            Block::new(BlockKind::Bullet, text).with_children(kids)
        }
        "ordered" | "ol" => {
            let kids = compile_kids(&spec.children, store)?;
            Block::new(BlockKind::Ordered { seq: spec.seq }, text).with_children(kids)
        }
        "todo" | "checkbox" => {
            let kids = compile_kids(&spec.children, store)?;
            Block::new(BlockKind::Todo { done: spec.done.unwrap_or(false) }, text).with_children(kids)
        }
        "code" => {
            let raw_lang = spec.lang.as_deref().unwrap_or("Plain Text");
            let resolved = if raw_lang.trim().is_empty() { lang::PLAIN.to_string() } else { lang::resolve(raw_lang).to_string() };
            let plain = RichText::plain(text.plain_text());
            Block::new(BlockKind::Code { lang: resolved, caption: spec.caption.clone(), wrap: spec.wrap.unwrap_or(false) }, plain)
        }
        "quote" | "blockquote" => {
            let mut kids = compile_kids(&spec.children, store)?;
            if kids.is_empty() {
                kids.push(if !text.is_empty() { Block::new(BlockKind::Text, text) } else { Block::text("") });
            }
            Block::new(BlockKind::Quote, RichText::default()).with_children(kids)
        }
        "callout" => {
            let emoji = match spec.emoji.as_deref() { Some(e) => assets::resolve_emoji(e)?, None => "💡".into() };
            let c_bg = spec.bg.as_deref().map(check_callout_bg).transpose()?;
            let border = spec.border.as_deref().map(check_callout_border).transpose()?;
            let tc = spec.text_color.as_deref().map(check_text_color).transpose()?;
            let mut kids = compile_kids(&spec.children, store)?;
            if kids.is_empty() {
                kids.push(if !text.is_empty() { Block::new(BlockKind::Text, text) } else { Block::text("") });
            }
            Block::new(BlockKind::Callout { emoji, bg: c_bg, border, text_color: tc }, RichText::default()).with_children(kids)
        }
        "divider" | "hr" => Block::new(BlockKind::Divider, RichText::default()),
        "sub_page_list" | "sub-page-list" => Block::new(BlockKind::SubPageList, RichText::default()),
        "image" | "img" => {
            let raw_src = spec.src.as_deref().ok_or_else(|| CliFailure::new("MISSING_IMAGE_SRC", "Image block requires `src`."))?;
            let (src_kind, src) = if raw_src.starts_with("http://") || raw_src.starts_with("https://") {
                ("href".into(), raw_src.to_string())
            } else {
                let (rel, _) = resolve_asset_src(store, raw_src, None, false, dry_run)?;
                ("src".into(), rel)
            };
            if let Some([l, t, r, b]) = spec.crop && !(0. <= l && l < r && r <= 1. && 0. <= t && t < b && b <= 1.) {
                return Err(CliFailure::new("INVALID_CROP", "Image crop must be [left, top, right, bottom] within 0..=1 with left < right and top < bottom."));
            }
            let mut blk = Block::new(
                BlockKind::Image { src_kind, src, width: spec.width, height: spec.height, caption: spec.caption.clone(), crop: spec.crop },
                RichText::default(),
            );
            blk.align = if spec.align.is_some() { align } else { Align::Center };
            return Ok(blk);
        }
        "file" | "attachment" => {
            let raw_src = spec.src.as_deref().ok_or_else(|| CliFailure::new("MISSING_FILE_SRC", "File block requires `src`."))?;
            let (src, name) = resolve_asset_src(store, raw_src, spec.name.as_deref(), true, dry_run)?;
            let view = match spec.view.as_deref().unwrap_or("card").to_ascii_lowercase().as_str() {
                "preview" => FileView::Preview,
                "card" => FileView::Card,
                other => return Err(CliFailure::new("INVALID_FILE_VIEW", format!("Unknown file view '{other}'.")).with_valid(["card", "preview"])),
            };
            Block::new(BlockKind::File(FileBlock { src_kind: "path".into(), src, name, view }), RichText::default())
        }
        "grid" => {
            let cols_spec = spec.columns.as_ref().ok_or_else(|| CliFailure::new("MISSING_COLUMNS", "Grid block requires `columns`."))?;
            if !(2..=MAX_COLUMNS).contains(&cols_spec.len()) {
                return Err(CliFailure::new("INVALID_GRID", format!("Grid must have 2..={MAX_COLUMNS} columns (got {}).", cols_spec.len())));
            }
            let raw_ratios: Vec<f32> = cols_spec.iter().map(|c| c.ratio.unwrap_or(1. / cols_spec.len() as f32)).collect();
            let sum: f32 = raw_ratios.iter().copied().sum();
            if sum <= 0. || raw_ratios.iter().any(|&r| !r.is_finite() || r <= 0.) {
                return Err(CliFailure::new("INVALID_GRID_RATIOS", "Column ratios must be positive finite numbers."));
            }
            let mut cols = vec![];
            for (c_spec, r) in cols_spec.iter().zip(raw_ratios) {
                let ratio = r / sum;
                if ratio < MIN_COLUMN_RATIO - 1e-4 {
                    return Err(CliFailure::new("COLUMN_TOO_NARROW", format!("Normalized column ratio {ratio:.4} is below MIN_COLUMN_RATIO ({MIN_COLUMN_RATIO:.4}).")));
                }
                let mut kids: Vec<Block> = c_spec.children.iter().map(|b| compile_block(b, store, dry_run)).collect::<Result<_, _>>()?;
                if kids.is_empty() { kids.push(Block::text("")); }
                cols.push(Block::new(BlockKind::Column { ratio }, RichText::default()).with_children(kids));
            }
            Block::new(BlockKind::Grid, RichText::default()).with_children(cols)
        }
        "table" => compile_table(spec, store, dry_run)?,
        "countdown" => {
            let cd = compile_countdown(spec.countdown.as_ref())?;
            Block::new(BlockKind::Countdown(cd), RichText::default())
        }
        "board" | "whiteboard" => {
            let height = spec.height.unwrap_or(360.).max(120.);
            let bd = if let Some(diag) = &spec.diagram {
                let mut d = diag.clone();
                if spec.height.is_some() { d.height = height; }
                board_builder::compile_diagram(&d)?
            } else if let Some(scene) = &spec.scene {
                let json = serde_json::to_string(scene).map_err(|e| CliFailure::new("BOARD_JSON_ERROR", e.to_string()))?;
                Board::from_scene(&json, height).ok_or_else(|| CliFailure::new("INVALID_BOARD_SCENE", "Invalid Excalidraw scene JSON."))?
            } else {
                Board { height, ..Default::default() }
            };
            Block::new(BlockKind::Board(std::sync::Arc::new(bd)), RichText::default())
        }
        "raw" => {
            let xml = spec.xml.clone().ok_or_else(|| CliFailure::new("MISSING_RAW_XML", "Raw block requires `xml`."))?;
            let parsed = docxml::parse(&xml).map_err(|e| CliFailure::new("INVALID_RAW_XML", format!("Invalid raw XML: {e}")))?;
            parsed.blocks.into_iter().next().unwrap_or_else(|| Block::text(""))
        }
        other => {
            return Err(CliFailure::new("UNKNOWN_BLOCK_TYPE", format!("Unknown block type '{other}'."))
                .with_valid(["p", "h1..h9", "bullet", "ordered", "todo", "code", "quote", "callout", "divider", "image", "file", "grid", "table", "countdown", "sub_page_list", "board", "raw"]));
        }
    };
    block.align = align;
    block.folded = folded && block.kind.has_text();
    if let Some(bg_name) = bg && block.kind.takes_bg() {
        block.bg = Some(check_block_bg(bg_name)?);
    }
    Ok(block)
}

fn compile_table(spec: &BlockSpec, store: &mut Store, dry_run: bool) -> Result<Block, CliFailure> {
    let rows_spec = spec.rows.as_ref().ok_or_else(|| CliFailure::new("MISSING_TABLE_ROWS", "Table block requires `rows`."))?;
    if rows_spec.is_empty() {
        return Err(CliFailure::new("EMPTY_TABLE", "Table must have at least 1 row."));
    }
    let ncols = rows_spec.iter().map(|r| r.iter().map(|c| c.colspan.unwrap_or(1) as usize).sum::<usize>()).max().unwrap_or(0).max(1);
    let col_widths = match &spec.col_widths {
        Some(w) if !w.is_empty() => {
            let mut v = w.iter().map(|x| x.max(Table::MIN_WIDTH)).collect::<Vec<_>>();
            v.resize(ncols, Table::DEFAULT_WIDTH);
            v
        }
        _ => vec![Table::DEFAULT_WIDTH; ncols],
    };
    let nrows = rows_spec.len();
    let mut rows: Vec<Vec<TableCell>> = (0..nrows).map(|_| (0..ncols).map(|_| TableCell::empty()).collect()).collect();
    let mut occupied = vec![vec![false; ncols]; nrows];
    for (r, r_spec) in rows_spec.iter().enumerate() {
        let mut c = 0usize;
        for c_spec in r_spec {
            while c < ncols && occupied[r][c] { c += 1; }
            if c >= ncols { break; }
            let cs = c_spec.colspan.unwrap_or(1).max(1) as usize;
            let rs = c_spec.rowspan.unwrap_or(1).max(1) as usize;
            if r + rs > nrows || c + cs > ncols {
                return Err(CliFailure::new("INVALID_TABLE_SPAN", format!("Cell ({r},{c}) span {rs}x{cs} exceeds {nrows}x{ncols}.")));
            }
            for rr in r..r + rs {
                for cc in c..c + cs {
                    if occupied[rr][cc] {
                        return Err(CliFailure::new("OVERLAPPING_TABLE_SPAN", format!("Table spans overlap at ({rr},{cc}).")));
                    }
                    occupied[rr][cc] = true;
                    rows[rr][cc] = TableCell::covered();
                }
            }
            let blocks = match &c_spec.blocks {
                Some(bs) if !bs.is_empty() => bs.iter().map(|b| compile_block(b, store, dry_run)).collect::<Result<Vec<_>, _>>()?,
                _ => vec![Block::text(c_spec.text.as_deref().unwrap_or(""))],
            };
            let bg = c_spec.bg.as_deref().map(check_block_bg).transpose()?;
            let valign = match c_spec.valign.as_deref() {
                None | Some("top") => None,
                Some("middle") | Some("center") => Some("middle".into()),
                Some("bottom") => Some("bottom".into()),
                Some(other) => return Err(CliFailure::new("INVALID_VALIGN", format!("Unknown cell valign '{other}'.")).with_valid(["top", "middle", "bottom"])),
            };
            rows[r][c] = TableCell { blocks, bg, valign, colspan: cs as u32, rowspan: rs as u32 };
            c += cs;
        }
    }
    let mut t = Table {
        col_widths,
        header_rows: spec.header_rows.unwrap_or(0).min(nrows),
        header_col: spec.header_col.unwrap_or(false),
        rows,
    };
    t.repair();
    Ok(Block::new(BlockKind::Table(t), RichText::default()))
}

pub fn compile_countdown(spec: Option<&CountdownSpec>) -> Result<Countdown, CliFailure> {
    let mut cd = Countdown::default();
    let Some(s) = spec else { return Ok(cd); };
    if let Some(color) = &s.color {
        let up = color.to_ascii_uppercase();
        if !COUNTDOWN_COLORS.iter().any(|c| *c == up) {
            return Err(CliFailure::new("INVALID_COUNTDOWN_COLOR", format!("Unknown countdown color '{color}'.")).with_valid(COUNTDOWN_COLORS));
        }
        cd.color = up;
    }
    cd.notify = s.notify.unwrap_or(true);
    let now_ms = fdl_core::time::now_ms();
    if let Some(date) = &s.date {
        let t = s.time.as_deref().unwrap_or("09:00");
        let (target_ms, _) = parse_date_time_ms(date, Some(t))?;
        cd.by_date = s.by_date.unwrap_or(true);
        cd.date = date.clone();
        cd.time = t.to_string();
        cd.start = s.start.unwrap_or(now_ms);
        cd.duration = s.duration.unwrap_or_else(|| ((target_ms - cd.start as i64).max(0) as f64 / 1000.).round() + 1.);
    } else if let Some(dhms) = s.dhms {
        cd.by_date = false;
        cd.dhms = dhms;
        cd.start = s.start.unwrap_or(now_ms);
        cd.duration = s.duration.unwrap_or((dhms[0] as f64 * 86400. + dhms[1] as f64 * 3600. + dhms[2] as f64 * 60. + dhms[3] as f64) + 1.);
    } else if let Some(dur) = s.duration {
        cd.duration = dur;
        cd.start = s.start.unwrap_or(now_ms);
        cd.by_date = s.by_date.unwrap_or(false);
    }
    Ok(cd)
}

pub fn compile_doc_spec(spec: &DocSpec, store: &mut Store, dry_run: bool) -> Result<Document, CliFailure> {
    let title = match &spec.title_runs {
        Some(runs) => compile_rich_text(runs, store, dry_run)?,
        None => RichText::plain(spec.title.as_deref().unwrap_or("")),
    };
    let icon = spec.icon.as_deref().filter(|s| !s.trim().is_empty()).map(assets::resolve_emoji).transpose()?;
    let cover = match &spec.cover {
        Some(c) => {
            if let Some(src) = &c.src && (src.starts_with(".assets/") || src.starts_with("assets/")) {
                Some(Cover { src: src.clone(), x: c.x.unwrap_or(0.), y: c.y.unwrap_or(0.) })
            } else {
                Some(assets::resolve_cover(
                    store,
                    c.gallery.as_deref(),
                    c.file.as_deref().map(Path::new),
                    c.x.unwrap_or(0.),
                    c.y.unwrap_or(0.),
                    dry_run,
                )?)
            }
        }
        None => None,
    };
    let mut blocks: Vec<Block> = spec.blocks.iter().map(|b| compile_block(b, store, dry_run)).collect::<Result<_, _>>()?;
    if blocks.is_empty() {
        blocks.push(Block::text(""));
    }
    let doc = Document { title, icon, cover, blocks };
    validate_document(&doc).map_err(|e| CliFailure::new("MODEL_INVARIANT_ERROR", e))?;
    Ok(doc)
}

pub fn apply_meta_spec(store: &mut Store, id: &str, m: &MetaSpec) -> Result<(), CliFailure> {
    if let Some(w) = &m.width {
        store.set_width(id, parse_width(w)?)?;
    }
    if let Some(p) = m.pinned { store.set_pinned(id, p)?; }
    if let Some(s) = m.starred { store.set_starred(id, s)?; }
    if let Some(cat) = m.catalogue_open { store.set_catalogue(id, cat)?; }
    store.update_meta(id, |meta| {
        if let Some(r) = m.reading { meta.reading = r; }
        if let Some(rh) = m.refs_hidden { meta.refs_hidden = rh; }
        if let Some(ih) = m.info_hidden { meta.info.hidden = ih; }
        if let Some(ic) = m.info_created { meta.info.created = ic; }
        if let Some(im) = m.info_modified { meta.info.modified = im; }
        if let Some(ord) = m.order { meta.order = ord; }
        if let Some(rord) = m.root_order { meta.root_order = Some(rord); }
    })?;
    Ok(())
}

// ------------------------------------------------------------------ Reverse serialization (Document -> DocSpec)

fn runs_from_rich(t: &RichText) -> Vec<RunSpec> {
    let mut out = vec![];
    for (r, m) in t.spans() {
        let seg = &t.text[r];
        if let Some(inl) = &m.inline {
            let mut rs = RunSpec {
                bold: m.bold.then_some(true),
                italic: m.italic.then_some(true),
                underline: m.underline.then_some(true),
                strike: m.strike.then_some(true),
                text_color: m.text_color.as_ref().map(|c| c.0.clone()),
                bg_color: m.bg_color.as_ref().map(|c| c.0.clone()),
                ..Default::default()
            };
            match &inl.kind {
                InlineKind::Equation(src) => rs.equation = Some(src.clone()),
                InlineKind::Button(b) => rs.button = Some(ButtonSpec {
                    text: b.text.clone(),
                    action: Some(b.action.name().into()),
                    src: (!b.src.is_empty()).then(|| b.src.clone()),
                    color: Some(b.color.clone()),
                }),
                InlineKind::Reminder(rem) => rs.reminder = Some(ReminderSpec {
                    expire: Some(rem.expire),
                    notify: Some(rem.notify),
                    should_notify: Some(rem.should_notify),
                    whole_day: Some(rem.whole_day),
                    ..Default::default()
                }),
                InlineKind::DocLink(dl) => rs.doc_link = Some(DocLinkSpec { target: dl.id.clone(), title: Some(dl.title.clone()) }),
                InlineKind::File(f) => rs.file = Some(InlineFileSpec { src: f.src.clone(), name: Some(f.name.clone()) }),
                InlineKind::Other { .. } => rs.text = Some(inl.xml.clone()),
            }
            out.push(rs);
            continue;
        }
        out.push(RunSpec {
            text: Some(seg.to_string()),
            bold: m.bold.then_some(true),
            italic: m.italic.then_some(true),
            underline: m.underline.then_some(true),
            strike: m.strike.then_some(true),
            code: m.code.then_some(true),
            link: m.link.clone(),
            text_color: m.text_color.as_ref().map(|c| c.0.clone()),
            bg_color: m.bg_color.as_ref().map(|c| c.0.clone()),
            ..Default::default()
        });
    }
    out
}

fn align_str(a: Align) -> Option<String> {
    match a { Align::Left => None, Align::Center => Some("center".into()), Align::Right => Some("right".into()) }
}

pub fn block_to_spec(b: &Block, loc: &str) -> BlockSpec {
    let has_runs = b.text.runs.len() > 1 || b.text.runs.first().is_some_and(|r| r.marks != Marks::default());
    let text = (!has_runs && b.kind.has_text()).then(|| b.text.text.clone());
    let runs = (has_runs && b.kind.has_text()).then(|| runs_from_rich(&b.text));
    let mut s = BlockSpec {
        loc: Some(loc.to_string()),
        kind: locator::block_kind_name(&b.kind).into(),
        text,
        runs,
        align: align_str(b.align),
        bg: b.bg.as_ref().map(|c| c.0.clone()),
        folded: b.folded.then_some(true),
        ..Default::default()
    };
    if !b.children.is_empty() && !matches!(b.kind, BlockKind::Grid) {
        s.children = Some(b.children.iter().enumerate().map(|(i, c)| block_to_spec(c, &format!("{loc}.{i}"))).collect());
    }
    match &b.kind {
        BlockKind::Heading { level, seq_auto } => {
            s.level = Some(*level);
            if *seq_auto { s.seq_auto = Some(true); }
        }
        BlockKind::Ordered { seq } => s.seq = *seq,
        BlockKind::Todo { done } => s.done = Some(*done),
        BlockKind::Code { lang, caption, wrap } => {
            s.lang = Some(lang.clone());
            s.caption = caption.clone();
            if *wrap { s.wrap = Some(true); }
        }
        BlockKind::Callout { emoji, bg, border, text_color } => {
            s.emoji = Some(emoji.clone());
            s.bg = bg.as_ref().map(|c| c.0.clone());
            s.border = border.as_ref().map(|c| c.0.clone());
            s.text_color = text_color.as_ref().map(|c| c.0.clone());
        }
        BlockKind::Image { src, width, height, caption, crop, .. } => {
            s.src = Some(src.clone());
            s.width = *width;
            s.height = *height;
            s.caption = caption.clone();
            s.crop = *crop;
        }
        BlockKind::File(f) => {
            s.src = Some(f.src.clone());
            s.name = Some(f.name.clone());
            s.view = Some(match f.view { FileView::Preview => "preview".into(), FileView::Card => "card".into() });
        }
        BlockKind::Grid => {
            s.columns = Some(b.children.iter().enumerate().map(|(ci, col)| {
                let ratio = if let BlockKind::Column { ratio } = col.kind { Some(ratio) } else { None };
                let col_loc = format!("{loc}.col{ci}");
                ColumnSpec {
                    ratio,
                    children: col.children.iter().enumerate().map(|(k, c)| block_to_spec(c, &format!("{col_loc}.{k}"))).collect(),
                }
            }).collect());
        }
        BlockKind::Table(t) => {
            s.col_widths = Some(t.col_widths.clone());
            s.header_rows = Some(t.header_rows);
            s.header_col = Some(t.header_col);
            s.rows = Some(t.rows.iter().enumerate().map(|(r, row)| {
                row.iter().enumerate().filter(|(_, c)| !c.is_covered()).map(|(c, cell)| {
                    TableCellSpec {
                        blocks: Some(cell.blocks.iter().enumerate().map(|(k, cb)| block_to_spec(cb, &format!("{loc}[{r},{c}].{k}"))).collect()),
                        bg: cell.bg.as_ref().map(|x| x.0.clone()),
                        valign: cell.valign.clone(),
                        colspan: (cell.colspan > 1).then_some(cell.colspan),
                        rowspan: (cell.rowspan > 1).then_some(cell.rowspan),
                        text: None,
                    }
                }).collect()
            }).collect());
        }
        BlockKind::Countdown(c) => {
            s.countdown = Some(CountdownSpec {
                date: (!c.date.is_empty()).then(|| c.date.clone()),
                time: (!c.time.is_empty()).then(|| c.time.clone()),
                duration: Some(c.duration),
                start: Some(c.start),
                notify: Some(c.notify),
                color: Some(c.color.clone()),
                by_date: Some(c.by_date),
                dhms: (c.dhms != [0; 4]).then_some(c.dhms),
            });
        }
        BlockKind::Board(bd) => {
            s.height = Some(bd.height);
            s.scene = serde_json::from_str(&bd.scene_json()).ok();
        }
        BlockKind::Raw { xml, .. } => s.xml = Some(xml.clone()),
        _ => {}
    }
    s
}

pub fn doc_to_spec(doc: &Document, meta: Option<&Meta>) -> DocSpec {
    let cover = doc.cover.as_ref().map(|c| CoverSpec {
        src: Some(c.src.clone()),
        x: (c.x != 0.).then_some(c.x),
        y: (c.y != 0.).then_some(c.y),
        ..Default::default()
    });
    let meta_spec = meta.map(|m| MetaSpec {
        width: Some(match m.width { PageWidth::Standard => "standard", PageWidth::Wide => "wide", PageWidth::Full => "full" }.into()),
        pinned: Some(m.pinned),
        starred: Some(m.starred > 0),
        reading: Some(m.reading),
        catalogue_open: Some(!m.catalogue_closed),
        refs_hidden: Some(m.refs_hidden),
        info_hidden: Some(m.info.hidden),
        info_created: Some(m.info.created),
        info_modified: Some(m.info.modified),
        order: (m.order > 0).then_some(m.order),
        root_order: m.root_order,
    });
    DocSpec {
        title: Some(doc.title.text.clone()),
        title_runs: (doc.title.runs.len() > 1).then(|| runs_from_rich(&doc.title)),
        icon: doc.icon.clone(),
        cover,
        meta: meta_spec,
        blocks: doc.blocks.iter().enumerate().map(|(i, b)| block_to_spec(b, &format!("{i}"))).collect(),
    }
}
