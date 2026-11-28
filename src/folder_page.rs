//! A folder's page: the one-row header (`topbar`, folder_header.rs), then one level of the
//! tree as cards or a list, in the order 显示 picks. Dressed as 所有文档: a hairline list with quiet
//! hover fills, white cards and pills, native menus.

use crate::app::{RowMenu, Workspace};
#[path = "thumb_style.rs"]
mod bs;
#[path = "thumb_text.rs"]
mod text_preview;
#[path = "folder_header.rs"]
pub(crate) mod header;
use crate::settings::display_title;
use crate::model::BlockKind;
use crate::search::doc_icon;
use crate::store::Entry;
use crate::editor::image::picture;
use crate::theme::{self, Theme};
use crate::trash::{empty_state, list_header_text, row_hover, row_line};
use crate::design::Popover;
use crate::ui::{ROW_RADIUS, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::RefCell;
use std::collections::{HashMap};
use std::rc::Rc;

/// A card's title steps this far right in 批量管理, past its checkbox (`geo.checkbox` and a gap).
const CHECK_ROOM: f32 = 24.;

/// The optional list columns (标题 is always shown).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Col {
    Modified,
    Created,
    Visited,
}

impl Col {
    fn label(self) -> &'static str {
        match self {
            Col::Modified => "修改时间",
            Col::Created => "创建时间",
            Col::Visited => "最近访问",
        }
    }
    /// The order this column sorts by.
    fn sort(self) -> Option<PageSort> {
        match self {
            Col::Modified => Some(PageSort::Modified),
            Col::Created => Some(PageSort::Created),
            Col::Visited => Some(PageSort::Visited),
        }
    }
}

/// The list's columns beside 标题.
const COLS: [Col; 3] = [Col::Modified, Col::Created, Col::Visited];

/// Bounded document trees retain formatting without retaining a whole long document.
type EquationNumbers = HashMap<crate::model::BlockId, u32>;
/// The 数据库 blocks' grids in miniature, by the preview block's id.
type Grids = HashMap<crate::model::BlockId, Rc<crate::table::still::Thumb>>;
/// `tables`: the 数据库 files the page shows (their rows drawn in the card), saved apart from the page.
/// `listed`: for a preview with a child-page list, the entries' generation and the card's own
/// stamp it was built against; the same pair needs no new look at the page's siblings.
struct ThumbContent { blocks: Vec<crate::model::Block>, eqnos: EquationNumbers, grids: Grids, has_subpages: bool, tables: Vec<String>, listed: (u64, u64) }
type Thumb = Rc<ThumbContent>;

/// The files of the 数据库 blocks in `blocks`, at any depth.
fn table_srcs(blocks: &[crate::model::Block], out: &mut Vec<String>) {
    for b in blocks {
        match &b.kind {
            BlockKind::DataTable(t) => out.push(t.src.clone()),
            BlockKind::Table(t) => for cell in t.rows.iter().flatten() { table_srcs(&cell.blocks, out) },
            _ => {}
        }
        table_srcs(&b.children, out);
    }
}

/// When the files in `srcs` (relative to the library) last changed, as one number; and
/// which of their card grids have come in (`card_grid`), so a preview drawn before its
/// grid was read is built again once it is.
fn tables_time(srcs: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    let root = crate::store::Store::active_dir();
    for s in srcs { modified(&root.join(s)).hash(&mut h); }
    if !srcs.is_empty() { GRIDS.with(|g| g.borrow().arrived).hash(&mut h); }
    h.finish()
}

/// The 数据库 shown on cards, their grids' top-left (`Still`) read off the main thread: reading a
/// table of 30,000 records took 100-400 ms, in the frame that first drew its card. By file, table,
/// view and whether it sits in a page; kept while the file's time stays, with its laid-out marks.
type GridKey = (std::path::PathBuf, Option<String>, Option<String>, bool);
#[derive(Default)]
struct CardGrids {
    /// `None`: the file could not be read (asked again once its time changes); and when a card last
    /// asked for it: past `KEPT` grids the longest unasked goes.
    done: HashMap<GridKey, (Option<std::time::SystemTime>, Option<Rc<crate::table::still::Thumb>>, web_time::Instant)>,
    /// Asked for this frame, not yet being read.
    asked: Vec<(GridKey, fdl_core::table::TableRef, Option<std::time::SystemTime>)>,
    reading: std::collections::HashSet<GridKey>,
    /// Grids come in so far, for `tables_time`.
    arrived: u64,
}
thread_local! { static GRIDS: std::cell::RefCell<CardGrids> = Default::default(); }
const KEPT: usize = 128;

/// A card's grid of the 数据库 `r` (`inline`: a block in a page), or none while it is read
/// (`fetch_card_grids`); a grid of an older file time stands in meanwhile.
fn card_grid(root: &std::path::Path, r: &fdl_core::table::TableRef, inline: bool) -> Option<Rc<crate::table::still::Thumb>> {
    let path = root.join(&r.src);
    let time = modified(&path);
    let key = (path, r.table.clone(), r.view.clone(), inline);
    GRIDS.with(|g| {
        let mut g = g.borrow_mut();
        let have = g.done.get_mut(&key).map(|(t, grid, used)| { *used = web_time::Instant::now(); (*t == time, grid.clone()) });
        if have.as_ref().is_none_or(|(fresh, _)| !fresh) && !g.reading.contains(&key) && !g.asked.iter().any(|(k, _, _)| *k == key) {
            g.asked.push((key, r.clone(), time));
        }
        have.and_then(|(_, grid)| grid)
    })
}

/// Reads the grids `card_grid` was asked for this frame on the background executor (the file, its
/// view run, cut to what a card shows), and draws the window again as each comes in; the main
/// thread only lays the cut copy out.
fn fetch_card_grids(cx: &mut Context<Workspace>) {
    let asked = GRIDS.with(|g| { let mut g = g.borrow_mut(); let asked = std::mem::take(&mut g.asked); for (k, _, _) in &asked { g.reading.insert(k.clone()); } asked });
    let (rows, width) = crate::table::still::Thumb::room();
    for (key, r, time) in asked {
        let root = crate::store::Store::active_dir();
        let read = cx.background_executor().spawn({ let r = r.clone(); async move { let _s = crate::devtools::span("card-grid-read"); fdl_core::table::block_still(&root, &r, rows, width) } });
        cx.spawn(async move |this, cx| {
            let still = read.await;
            GRIDS.with(|g| {
                let mut g = g.borrow_mut();
                g.reading.remove(&key);
                let thumb = still.map(|s| crate::table::still::Thumb::new(s, &r, key.3));
                if g.done.len() >= KEPT && !g.done.contains_key(&key) && let Some(old) = g.done.iter().min_by_key(|(_, (_, _, used))| *used).map(|(k, _)| k.clone()) { g.done.remove(&old); }
                g.done.insert(key, (time, thumb, web_time::Instant::now()));
                g.arrived += 1;
            });
            let _ = this.update(cx, |_, cx| cx.notify());
        }).detach();
    }
}

/// When `path` last changed, asked of the disk at most every 2 s: every card is drawn each
/// frame (a folder card's hover, a scroll), and a frame waiting on the disk drops.
fn modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
    thread_local! { static SEEN: std::cell::RefCell<HashMap<std::path::PathBuf, (web_time::Instant, Option<std::time::SystemTime>)>> = Default::default(); }
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        let now = web_time::Instant::now();
        if let Some((at, t)) = seen.get(path) && now.duration_since(*at) < std::time::Duration::from_secs(2) { return *t }
        if seen.len() > 4096 { seen.retain(|_, (at, _)| now.duration_since(*at) < std::time::Duration::from_secs(2)); }
        let t = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        seen.insert(path.to_path_buf(), (now, t));
        t
    })
}

fn has_subpage_list(blocks: &[crate::model::Block]) -> bool {
    blocks.iter().any(|b| match &b.kind {
        BlockKind::SubPageList => true,
        BlockKind::Table(t) => t.rows.iter().any(|row| row.iter().any(|cell| has_subpage_list(&cell.blocks))),
        _ => has_subpage_list(&b.children),
    })
}

fn thumb_blocks(blocks: &[crate::model::Block], budget: &mut usize, depth: usize, numbers: &EquationNumbers, eqnos: &mut EquationNumbers, grids: &mut Grids) -> Vec<crate::model::Block> {
    if depth > 8 { return vec![]; }
    let mut out = vec![];
    let mut sequence = 0;
    for b in blocks {
        if *budget == 0 { break; }
        *budget -= 1;
        let end = b.text.text.char_indices().nth(600).map_or(b.text.text.len(), |(i, _)| i);
        // A table owns its cell trees inside the kind, so bound those before cloning.
        let kind = match &b.kind {
            BlockKind::Table(t) => {
                let nr = t.rows.len().min(6); let nc = t.col_widths.len().min(6);
                let rows = t.rows.iter().take(nr).enumerate().map(|(r, row)| row.iter().take(nc).enumerate().map(|(c, cell)| {
                    crate::model::TableCell { blocks: thumb_blocks(&cell.blocks, budget, depth + 1, numbers, eqnos, grids), bg: cell.bg.clone(), valign: cell.valign.clone(),
                        colspan: cell.colspan.min((nc - c) as u32), rowspan: cell.rowspan.min((nr - r) as u32) }
                }).collect()).collect();
                BlockKind::Table(crate::model::Table { col_widths: t.col_widths.iter().take(nc).copied().collect(), header_rows: t.header_rows.min(nr), header_col: t.header_col, rows })
            }
            BlockKind::Image { src, width, height, .. } => {
                let mut kind = b.kind.clone();
                // The file and missing dimensions found once per cached thumbnail, not each frame:
                // finding the file asks the disk (`moved_asset`), which a frame must not wait on.
                let path = crate::store::image_path(src);
                let dimensions = path.as_ref().and_then(|p| image::ImageReader::open(p).ok())
                    .and_then(|r| r.with_guessed_format().ok()).and_then(|r| r.into_dimensions().ok());
                let (iw, ih) = dimensions.map(|(w, h)| (w.max(1) as f32, h.max(1) as f32)).unwrap_or((690., 460.));
                let w = width.unwrap_or_else(|| height.map(|h| h * iw / ih).unwrap_or(iw)).max(1.);
                let h = height.unwrap_or(w * ih / iw).max(1.);
                // No wider than the page: a 1536 px picture would otherwise reserve its full height below a narrower drawing.
                let (w, h) = if w > 690. { (690., h * 690. / w) } else { (w, h) };
                if let BlockKind::Image { src, width, height, .. } = &mut kind {
                    *src = path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(); *width = Some(w); *height = Some(h);
                }
                kind
            }
            BlockKind::Raw { tag, .. } => BlockKind::Raw { tag: tag.clone(), xml: String::new() },
            kind => kind.clone(),
        };
        let text = b.text.slice(0..end);
        let mut preview = crate::model::RichText::default();
        for (range, marks) in text.spans() {
            let mut marks = marks.clone();
            if marks.inline.as_ref().is_some_and(|i| i.equation().is_some()) {
                preview.push(&text.text[range], marks);
                continue;
            }
            let label = marks.inline.as_ref().map(|inline| match &inline.kind {
                crate::model::InlineKind::Equation(_) => unreachable!(),
                crate::model::InlineKind::File(file) => file.name.clone(),
                crate::model::InlineKind::Button(button) => button.text.clone(),
                crate::model::InlineKind::Reminder(reminder) => crate::editor::reminder::reminder_label(reminder, " "),
                crate::model::InlineKind::DocLink(doc) => display_title(&doc.title).to_string(),
                crate::model::InlineKind::Other { tag } => tag.clone(),
            }).map(|label| label.chars().take(160).collect::<String>());
            if let Some(inline) = &marks.inline { match &inline.kind {
                crate::model::InlineKind::Equation(_) => {},
                crate::model::InlineKind::Button(_) => marks.bold = true,
                crate::model::InlineKind::DocLink(_) | crate::model::InlineKind::File(_) => marks.link = Some(String::new()),
                _ => {},
            } }
            marks.inline = None;
            preview.push(&text.text[range].replace('\u{fffc}', label.as_deref().unwrap_or("")), marks);
        }
        let mut copy = crate::model::Block::new(kind, preview);
        if let Some(n) = numbers.get(&b.id) { eqnos.insert(copy.id, *n); }
        // a 数据库 in a page: the top-left of its grid, once read (the card is built again when it comes in or its file changes)
        if let BlockKind::DataTable(r) = &b.kind && let Some(g) = card_grid(&crate::store::Store::active_dir(), r, true) { grids.insert(copy.id, g); }
        if let BlockKind::Ordered { seq } = &mut copy.kind { sequence = seq.unwrap_or(sequence + 1); *seq = Some(sequence); } else { sequence = 0; }
        copy.bg = b.bg.clone(); copy.align = b.align;
        copy.children = thumb_blocks(&b.children, budget, depth + 1, numbers, eqnos, grids);
        out.push(copy);
    }
    out
}

/// A 数据库 in a page's card is drawn no taller than the tallest card (`render_page_grid`).
const GRID_ROOM: f32 = 420.;

/// Folder page cards decode pictures at this share of the size they are drawn:
/// a card is small and glanced at, so a softer picture costs nothing.
const THUMB: f32 = 0.75;

/// Long edge drawn for a picture `w` wide with this width / height `ratio`.
fn thumb_long(w: f32, ratio: f32) -> f32 { (if ratio >= 1. { w } else { w / ratio }) * THUMB }

fn thumb_block(b: &crate::model::Block, depth: usize, inherited: Hsla, quote: bool, th: &ThumbContent) -> AnyElement {
    thumb_block_in(b, depth, inherited, quote, false, 0, th)
}

fn thumb_block_in(b: &crate::model::Block, depth: usize, inherited: Hsla, quote: bool, cell: bool, list_depth: usize, th: &ThumbContent) -> AnyElement {
    let s = bs::SCALE;
    let metrics = bs::typography(&b.kind);
    let (font_size, line_height) = if cell && !matches!(b.kind, BlockKind::Heading { .. } | BlockKind::Code { .. }) { (14., 22.) } else { (metrics.size, metrics.line_height) };
    let done = matches!(b.kind, BlockKind::Todo { done: true });
    let color = if done { Theme::todo_done() } else { inherited };
    let mut node = div().w_full().min_w_0().flex_none().flex().flex_col()
        .text_size(px(font_size * s)).line_height(px(line_height * s)).text_color(color)
        .when(b.kind.has_text(), |d| d.min_h(px(line_height * s)));
    if let Some(bg) = b.bg.as_ref().and_then(|c| theme::block_bg(&c.0)) { node = node.rounded(px(6. * s)).bg(bs::display_fill(bg)); }
    match &b.kind {
        BlockKind::Image { src, width, height, caption, .. } => {
            // `thumb_blocks` put the file found in `src` ("" when there is none).
            let path = (!src.is_empty()).then(|| std::path::PathBuf::from(src.to_string()));
            let ratio = width.unwrap_or(690.).max(1.) / height.unwrap_or(460.).max(1.);
            node = match b.align { crate::model::Align::Left => node.items_start(), crate::model::Align::Center => node.items_center(), crate::model::Align::Right => node.items_end() };
            // Reserve the proportional height at the available width before loading.
            let media = div().relative().flex_none().w_full().max_w_full().overflow_hidden().when_some(*width, |d, w| d.w(px(w * s)))
                .map(|mut d| { d.style().aspect_ratio = Some(ratio); d });
            node = node.child(if let Some(path) = path {
                // The web demo fetches pictures on first use: an empty frame until then.
                if fdl_core::vfs::pending(&path) { media.into_any_element() } else {
                media.child(picture(&path, thumb_long(width.unwrap_or(690.).min(690.), ratio) * s).absolute().inset_0().size_full().min_w_0().min_h_0().rounded(px(bs::RADIUS * s)).object_fit(ObjectFit::Contain)).into_any_element() }
            } else {
                media.rounded(px(bs::RADIUS * s)).bg(bs::code_surface()).flex().items_center().justify_center().child(icon("image-broken", 24. * s, Theme::text_disabled())).into_any_element()
            });
            return node.when_some(caption.as_ref().filter(|c| !c.is_empty()), |d, caption| d.child(div().w_full().max_w_full().when_some(*width, |d, w| d.w(px(w * s))).mt(px(6. * s)).text_size(px(13. * s)).line_height(px(20. * s)).text_color(Theme::text_caption()).text_center().child(caption.clone()))).into_any_element();
        }
        BlockKind::Divider => return node.h(px(25. * s)).justify_center().child(div().h(px(s)).w_full().bg(Theme::line_divider())).into_any_element(),
        BlockKind::Grid => node = node.flex_row().gap(px(20. * s)),
        BlockKind::Column { .. } => node = node.flex_1(),
        BlockKind::Quote => node = node.relative().py(px(bs::QUOTE_PAD * s)).pl(px(bs::QUOTE_INSET * s))
            .child(div().absolute().left_0().top_0().bottom_0().w(px(bs::QUOTE_BAR * s)).rounded_full().bg(color)),
        BlockKind::Callout { emoji, bg, border, text_color } => {
            let tc = text_color.as_ref().and_then(|c| theme::text_color(&c.0)).map(bs::display_ink).unwrap_or(color);
            let mut body = div().flex_1().min_w_0().flex().flex_col();
            if depth < 8 { for (i, child) in b.children.iter().enumerate() {
                let gap = if i == 0 { 0. } else { bs::gap(&b.children[i - 1].kind).1.max(bs::gap(&child.kind).0) };
                body = body.child(div().mt(px(gap * s)).child(thumb_block_in(child, depth + 1, tc, quote, cell, list_depth, th)));
            } }
            let first_line = b.children.first().map(|b| bs::typography(&b.kind).line_height).unwrap_or(theme::BODY_LINE);
            return node.relative().p(px(bs::INSET * s)).rounded(px(bs::CALLOUT_RADIUS * s)).border(px(s)).border_color(bs::border(border.as_ref().map(|c| c.0.as_str()))).bg(bs::fill(bg.as_ref().map(|c| c.0.as_str())))
                .child(div().flex().gap(px(bs::ICON_GAP * s)).child(div().size(px(bs::ICON * s)).mt(px((first_line - bs::ICON) / 2. * s)).text_size(px(20. * s)).line_height(px(22. * s)).flex_none().child(emoji.clone())).child(body)).into_any_element();
        }
        BlockKind::Code { lang, caption, wrap, .. } => {
            node = node.rounded(px(bs::RADIUS * s)).border(px(s)).border_color(Theme::line_border()).bg(bs::code_surface()).font_family(theme::MONO_FONT)
                .child(div().h(px(bs::CODE_HEADER * s)).pl(px(28. * s)).pr(px(8. * s)).rounded_t(px((bs::RADIUS - 1.) * s)).bg(bs::code_header()).flex().items_center().font_family(theme::ui_family()).text_size(px(13. * s)).text_color(Theme::text_caption()).child(caption.clone().unwrap_or_else(|| crate::settings::text("代码块").into())));
            let spans = crate::highlight::spans(lang, &b.text.text);
            let mut body = div().pt(px(14. * s)).pb(px(18. * s)).pr(px(20. * s)).flex().flex_col();
            let mut off = 0;
            for (i, line) in b.text.text.split('\n').enumerate() {
                let end = off + line.len();
                let mut cuts = vec![off, end];
                for (r, _, _) in &spans {
                    if r.start > off && r.start < end { cuts.push(r.start); }
                    if r.end > off && r.end < end { cuts.push(r.end); }
                }
                cuts.sort_unstable(); cuts.dedup();
                let runs = cuts.windows(2).map(|r| {
                    let h = spans.iter().find(|(h, _, _)| h.start <= r[0] && r[1] <= h.end);
                    let mut f = font(theme::MONO_FONT); if h.is_some_and(|(_, _, it)| *it) { f.style = FontStyle::Italic; }
                    TextRun { len: r[1] - r[0], font: f, color: h.map_or(Theme::text_code(), |(_, c, _)| *c), background_color: None, underline: None, strikethrough: None }
                }).collect::<Vec<_>>();
                body = body.child(div().flex().items_start()
                    .child(div().w(px(bs::CODE_GUTTER * s)).mr(px(bs::CODE_GAP * s)).flex_none().text_right().text_color(Theme::text_caption()).child((i + 1).to_string()))
                    .child(div().min_w_0().flex_1().overflow_hidden().when(!*wrap, |d| d.whitespace_nowrap()).child(if line.is_empty() { StyledText::new(" ") } else { StyledText::new(line.to_string()).with_runs(runs) })));
                off = end + 1;
            }
            return node.child(body).into_any_element();
        }
        BlockKind::Table(table) => {
            let widths: Vec<f32> = table.col_widths.iter().map(|w| w * s).collect();
            let mut table_el = div().grid().grid_col_widths(widths.clone()).w(px(widths.iter().sum())).max_w_full();
            for (r, c, value) in table.cells() {
                let (rs, cs) = (value.rowspan as usize, value.colspan as usize);
                let header = r < table.header_rows || (table.header_col && c == 0);
                let mut col = div().min_w_0().flex().flex_col().row_start(r as i16 + 1).row_end((r + rs) as i16 + 1).col_start(c as i16 + 1).col_end((c + cs) as i16 + 1)
                    .p(px(8. * s)).border_r(px(s)).border_b(px(s)).when(r == 0, |d| d.border_t(px(s))).when(c == 0, |d| d.border_l(px(s))).border_color(bs::table_line())
                    .when(header, |d| d.bg(Theme::table_header())).when_some(value.bg.as_ref().and_then(|c| theme::block_bg(&c.0)), |d, c| d.bg(bs::display_fill(c)))
                    .map(|d| match value.valign.as_deref() { Some("middle") => d.justify_center(), Some("bottom") => d.justify_end(), _ => d });
                for (i, child) in value.blocks.iter().enumerate() {
                    let gap = if i == 0 { 0. } else { bs::gap(&value.blocks[i - 1].kind).1.min(6.).max(bs::gap(&child.kind).0.min(8.)) };
                    col = col.child(div().mt(px(gap * s)).child(thumb_block_in(child, depth + 1, color, quote, true, 0, th)));
                }
                table_el = table_el.child(col);
            }
            return node.child(table_el).into_any_element();
        }
        BlockKind::File(f) => return node.p(px(12. * s)).rounded(px(bs::RADIUS * s)).border(px(s)).border_color(Theme::line_border()).bg(bs::code_surface()).flex_row().items_center().gap(px(12. * s))
            .child(icon("file-text", 30. * s, Theme::icon())).child(div().flex_1().min_w_0().text_size(px(15. * s)).font_weight(FontWeight::MEDIUM).child(f.name.clone())).into_any_element(),
        BlockKind::Countdown(c) => {
            let left = c.remaining(crate::time::now_ms());
            let color = u32::from_str_radix(c.color.trim_start_matches('#'), 16).map(theme::c).unwrap_or(Theme::primary_500());
            let mut ink = bs::display_ink(color); ink.l = ink.l.min(0.38);
            let mut row = div().flex().justify_center().gap(px(16. * s));
            for (i, value) in [left / 86400, left / 3600 % 24, left / 60 % 60, left % 60].into_iter().enumerate() {
                row = row.child(div().flex().flex_col().items_center().child(div().size(px(52. * s)).rounded(px(8. * s)).bg(bs::highlight(color)).flex().items_center().justify_center().font_family(theme::MONO_FONT).font_weight(FontWeight::MEDIUM).text_size(px(26. * s)).text_color(ink).child(format!("{value:02}")))
                    .child(div().mt(px(4. * s)).text_size(px(14. * s)).line_height(px(20. * s)).text_color(Theme::text_caption()).child([crate::settings::text("天"), crate::settings::text("时"), crate::settings::text("分"), crate::settings::text("秒")][i])));
            }
            return node.py(px(18. * s)).rounded(px(bs::RADIUS * s)).border(px(s)).border_color(Theme::line_border()).bg(bs::code_surface()).child(row).into_any_element();
        }
        BlockKind::SubPageList => {
            node = node.p(px(8. * s)).rounded(px(bs::RADIUS * s)).border(px(s)).border_color(Theme::line_border()).bg(Theme::bg_overlay());
            if b.children.is_empty() { return node.h(px(96. * s)).justify_center().text_center().text_color(Theme::text_caption()).child(crate::settings::text("目录下暂无子页面")).into_any_element(); }
            for child in &b.children { node = node.child(div().py(px(9. * s)).px(px(6. * s)).flex().items_start().gap(px(6. * s)).child(icon("doc", 16. * s, Theme::icon())).child(div().flex_1().min_w_0().font_weight(FontWeight::MEDIUM).child(child.text.text.clone()))); }
            return node.into_any_element();
        }
        // a 画板 block: its board fit into the block's box (painters cached by board version)
        BlockKind::Board(bd) => {
            let board = bd.clone();
            return node.h(px(bd.height * s)).rounded(px(8. * s)).border(px(s)).border_color(Theme::line_border()).bg(crate::board::render::canvas_color(bd)).overflow_hidden()
                .child(canvas(|_, _, _| {}, move |bounds, _, window, cx| crate::board::render::paint_card(&board, bounds, window, cx)).size_full()).into_any_element();
        }
        // a 数据库: its grid as it shows, cut at the tallest card
        BlockKind::DataTable(_) => return node.children(th.grids.get(&b.id).map(|g| g.element(s, GRID_ROOM))).into_any_element(),
        BlockKind::Raw { tag, .. } => return node.p(px(8. * s)).rounded(px(8. * s)).bg(Theme::bg_overlay()).text_color(Theme::text_caption()).child(crate::localized_format!("<{tag}> 已保留的内容", "<{tag}> Preserved content")).into_any_element(),
        _ => {}
    }
    if b.kind.has_text() {
        let runs = b.text.runs.iter().map(|r| {
            let mut f = font(if r.marks.code { theme::MONO_FONT } else { theme::ui_family() });
            f.weight = if r.marks.bold { FontWeight::BOLD } else { bs::weight(&b.kind) };
            if r.marks.italic || quote { f.style = FontStyle::Italic; }
            let mut ink = if r.marks.link.is_some() { Theme::link() } else { r.marks.text_color.as_ref().and_then(|c| theme::text_color(&c.0)).map(bs::display_ink).unwrap_or(color) };
            if done { ink.a = 0.5; }
            let mark = r.marks.bg_color.as_ref().and_then(|c| theme::bg_color(&c.0)).map(bs::highlight);
            TextRun { len: r.len, font: f, color: if mark.is_some() { crate::theme::on_mark(ink) } else { ink },
                background_color: mark.or_else(|| r.marks.code.then(bs::code_surface)),
                underline: (r.marks.underline || r.marks.link.is_some()).then_some(UnderlineStyle { thickness: px(s), color: None, wavy: false }),
                strikethrough: (r.marks.strike || done).then_some(StrikethroughStyle { thickness: px(s), color: None }) }
        }).collect::<Vec<_>>();
        let marker = match b.kind {
            BlockKind::Bullet => Some(div().w(px(24. * s)).h(px(line_height * s)).pl(px(4. * s)).flex_none().flex().items_center().child(match list_depth % 3 {
                0 => div().size(px(5.5 * s)).rounded_full().bg(Theme::text_caption()),
                1 => div().size(px(5.5 * s)).rounded_full().border(px(s)).border_color(Theme::text_caption()),
                _ => div().size(px(5. * s)).bg(Theme::text_caption()),
            }).into_any_element()),
            BlockKind::Ordered { seq } => Some(div().min_w(px(18. * s)).mr(px(6. * s)).flex_none().text_color(Theme::text_caption()).child(crate::editor::core::ordered_marker(seq.unwrap_or(1), list_depth)).into_any_element()),
            BlockKind::Todo { done } => Some(div().w(px(24. * s)).h(px(line_height * s)).flex_none().flex().items_center().child(div().size(px(16. * s)).rounded(px(4. * s)).border(px(s)).border_color(if done { Theme::primary_500() } else { Theme::text_disabled() }).bg(if done { Theme::primary_500() } else { Theme::bg_body() }).flex().items_center().justify_center().when(done, |d| d.child(icon("todo-check", 12. * s, gpui::white())))).into_any_element()),
            _ => None,
        };
        let text = if b.text.runs.iter().any(|r| r.marks.inline.as_ref().is_some_and(|i| i.equation().is_some())) {
            text_preview::PreviewText { text: b.text.clone(), runs, size: font_size * s, line_height: line_height * s, align: b.align, eq_counter: th.eqnos.get(&b.id).copied().unwrap_or(0) }.into_any_element()
        } else { StyledText::new(b.text.text.clone()).with_runs(runs).into_any_element() };
        node = node.child(div().flex().items_start().children(marker).child(div().min_w_0().flex_1().map(|d| match b.align { crate::model::Align::Center => d.text_center(), crate::model::Align::Right => d.text_right(), _ => d }).child(text)));
    }
    if depth < 8 { for (i, child) in b.children.iter().enumerate() {
        let nested = b.kind.nests();
        let gap = if i == 0 { if nested { 8. } else { 0. } } else { bs::gap(&b.children[i - 1].kind).1.max(bs::gap(&child.kind).0) };
        node = node.child(div().min_w_0().when(!matches!(b.kind, BlockKind::Grid), |d| d.mt(px(gap * s)))
            .when(nested, |d| d.pl(px(25. * s)))
            .when(matches!(b.kind, BlockKind::Grid), |d| d.flex().w_0().map(|mut d| { d.style().flex_grow = Some(match child.kind { BlockKind::Column { ratio } => ratio, _ => 1. }); d }))
            .child(thumb_block_in(child, depth + 1, color, quote || matches!(b.kind, BlockKind::Quote), cell, list_depth + usize::from(nested), th)));
    } }
    node.into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SortDir { Asc, Desc }
impl SortDir {
    fn key(self) -> &'static str { match self { Self::Asc => "asc", Self::Desc => "desc" } }
    fn of(key: &str) -> Self { if key == "asc" { Self::Asc } else { Self::Desc } }
    fn flip(self) -> Self { match self { Self::Asc => Self::Desc, Self::Desc => Self::Asc } }
    fn label(self) -> &'static str { crate::settings::tr(if self == Self::Asc { "升序" } else { "降序" }, if self == Self::Asc { "Ascending" } else { "Descending" }) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PageSort { Manual, Visited, Modified, Created, Title, Kind }
impl PageSort {
    fn key(self) -> &'static str { match self { Self::Manual => "manual", Self::Visited => "visited", Self::Modified => "modified", Self::Created => "created", Self::Title => "title", Self::Kind => "kind" } }
    fn of(key: &str) -> Self { match key { "manual" => Self::Manual, "modified" => Self::Modified, "created" => Self::Created, "title" => Self::Title, "kind" => Self::Kind, _ => Self::Visited } }
}
impl PageSort {
    fn label(self) -> &'static str {
        match self {
            Self::Manual => crate::settings::tr("手动排序", "Manual order"),
            Self::Visited => "最近访问时间",
            Self::Modified => "最近修改时间",
            Self::Created => "创建时间",
            Self::Title => "标题",
            Self::Kind => crate::settings::tr("类型", "Kind"),
        }
    }
    /// The direction a column sorts by the first time it is picked.
    fn default_dir(self) -> SortDir { if matches!(self, Self::Title | Self::Kind) { SortDir::Asc } else { SortDir::Desc } }
    /// 手动排序 has no direction; the others do.
    fn directional(self) -> bool { self != Self::Manual }
}

pub struct FolderPage {
    pub grid: bool,
    folder_grid_scroll: UniformListScrollHandle,
    folder_list_scroll: ScrollHandle,
    grid_scrollbar: Rc<crate::design::ScrollBar>,
    list_scrollbar: Rc<crate::design::ScrollBar>,
    /// The 搜索 dialog's results (`search.rs`).
    pub(crate) search_scrollbar: Rc<crate::design::ScrollBar>,
    sort: PageSort,
    sort_dir: SortDir,
    folders_first: bool,
    /// The header's 显示 menu (view and sort) is open.
    pub view_menu: bool,
    thumbs: RefCell<HashMap<String, (u64, Thumb, web_time::Instant)>>,
    /// Last-frame bounds of the controls the popovers hang from.
    anchors: Rc<RefCell<HashMap<&'static str, Bounds<Pixels>>>>,
    /// Folder cards and dragging into them (`folder_card.rs`).
    pub(crate) folders: crate::folder_card::FolderCards,
    /// The level last shown, kept while the entries (`Level::key`: their
    /// `generation`) and the order stay as they were:
    /// copying and sorting a folder of 3,000 notes took most of a frame.
    level: RefCell<Option<Level>>,
    /// The list row under the pointer (its element id), so the hairlines on
    /// both sides of it hide (`ui::row_line`); the trash list shares it.
    pub(crate) list_hover: Option<SharedString>,
}

struct Level { key: u64, parent: Option<String>, order: Order, entries: Rc<Vec<Entry>> }

impl Default for FolderPage {
    fn default() -> Self {
        FolderPage {
            grid: true,
            folder_grid_scroll: Default::default(),
            folder_list_scroll: Default::default(),
            // both run down to the window's bottom corner
            grid_scrollbar: Rc::new(crate::design::ScrollBar::corners(0., crate::design::scroll::WINDOW_R)),
            list_scrollbar: Rc::new(crate::design::ScrollBar::corners(0., crate::design::scroll::WINDOW_R)),
            search_scrollbar: Default::default(),
            // Manual: the sidebar's order (`Meta::place`), so both lists read the same.
            sort: PageSort::Manual,
            sort_dir: SortDir::Desc,
            folders_first: false,
            view_menu: false,
            thumbs: Default::default(),
            anchors: Default::default(),
            folders: Default::default(),
            level: Default::default(),
            list_hover: None,
        }
    }
}

impl FolderPage {
    /// The page list's current order, for anything else that lists pages
    /// (the sidebar's tree).
    pub(crate) fn order(&self) -> Order {
        Order { field: self.sort, dir: self.sort_dir, folders_first: self.folders_first }
    }

    /// Tracks the hovered list row; true when it changed.
    pub(crate) fn hover_row(&mut self, key: &SharedString, on: bool) -> bool {
        let was = self.list_hover.clone();
        if on { self.list_hover = Some(key.clone()) } else if self.list_hover.as_ref() == Some(key) { self.list_hover = None }
        was != self.list_hover
    }

    pub(crate) fn anchor(&self, key: &str) -> Option<Bounds<Pixels>> {
        self.anchors.borrow().get(key).copied()
    }

    /// Records the element's bounds under `key` each frame.
    pub(crate) fn track(&self, key: &'static str) -> impl IntoElement {
        let rec = self.anchors.clone();
        canvas(move |b, _, _| {
            rec.borrow_mut().insert(key, b);
        }, |_, _, _, _| {})
        .absolute()
        .inset_0()
    }

    /// The column drawn as sorted; 标题 and 手动排序 mark their own header.
    fn sort_col(&self) -> Option<Col> {
        match self.sort {
            PageSort::Visited => Some(Col::Visited),
            PageSort::Modified => Some(Col::Modified),
            PageSort::Created => Some(Col::Created),
            PageSort::Title | PageSort::Kind | PageSort::Manual => None,
        }
    }
}

/// The arrow on a sorted column: down for 降序, up for 升序. A chevron turned
/// over, rather than a second icon, so both directions share one glyph.
fn sort_glyph(dir: SortDir, size: f32) -> Svg {
    let g = icon("chevron-down", size, Theme::icon_muted());
    if dir == SortDir::Asc { g.with_transformation(Transformation::rotate(radians(std::f32::consts::PI))) } else { g }
}

/// Sort a folder's page. 手动排序 is the sidebar's order; the
/// automatic ones never write it, and fall back to it (then the id) for equal
/// values, so equal pages keep the order the user arranged by hand.
pub(crate) fn sort_entries(v: &mut [Entry], order: Order) {
    v.sort_by(|a, b| order.cmp(a, b));
}

/// How pages are ordered, as one value: the folder pages' setting, used there and by the
/// sidebar's tree so both lists always agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Order {
    pub field: PageSort,
    pub dir: SortDir,
    pub folders_first: bool,
}

impl Order {
    pub(crate) fn cmp(&self, a: &Entry, b: &Entry) -> std::cmp::Ordering {
        // 多个顶层文件夹: their rows keep folders.json's order; only what is in a top-level folder is sorted.
        let row = |e: &Entry| crate::shared_store::top_of_row(&e.id).is_some();
        if row(a) || row(b) {
            return row(b).cmp(&row(a)).then_with(|| (a.meta.place(), &a.id).cmp(&(b.meta.place(), &b.id)));
        }
        if self.field == PageSort::Manual {
            return (a.meta.place(), &a.id).cmp(&(b.meta.place(), &b.id));
        }
        let asc = |o: std::cmp::Ordering| if self.dir == SortDir::Asc { o } else { o.reverse() };
        let order = match self.field {
            PageSort::Title => asc(natural_cmp(&a.title, &b.title)),
            PageSort::Visited => asc(visited(a).cmp(&visited(b))),
            PageSort::Modified => asc(a.meta.modified.cmp(&b.meta.modified)),
            PageSort::Created => asc(a.meta.created.cmp(&b.meta.created)),
            PageSort::Kind => asc(kind_class(a).cmp(&kind_class(b)).then_with(|| natural_cmp(&a.title, &b.title))),
            PageSort::Manual => std::cmp::Ordering::Equal,
        };
        let group = if self.folders_first { b.meta.folder.is_some().cmp(&a.meta.folder.is_some()) } else { std::cmp::Ordering::Equal };
        group.then(order).then_with(|| a.meta.place().cmp(&b.meta.place())).then_with(|| a.id.cmp(&b.id))
    }
}

/// Titles compare the way people read them: case-blind, and 图2 before 图10.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                // Compare digit runs as numbers without an integer conversion:
                // leading zeroes go, then the longer run is the larger number.
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(|c| c.is_ascii_digit()) { s.push(c); it.next(); }
                    s.trim_start_matches('0').to_string()
                };
                let (da, db) = (run(&mut a), run(&mut b));
                let order = da.len().cmp(&db.len()).then_with(|| da.cmp(&db));
                if order != Ordering::Equal { return order; }
            }
            (Some(x), Some(y)) => {
                a.next();
                b.next();
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal { return order; }
            }
        }
    }
}

/// 按类型: folders, documents, Markdown, 画板, then 数据表. Names order equals kinds.
fn kind_class(e: &Entry) -> u8 {
    use crate::store::DocKind;
    match e.meta.kind {
        _ if e.meta.folder.is_some() => 0,
        DocKind::Doc => if e.meta.markdown { 2 } else { 1 },
        DocKind::Board => 3,
        DocKind::Table => 4,
    }
}

fn visited(e: &Entry) -> u64 {
    if e.meta.opened > 0 { e.meta.opened } else { e.meta.modified }
}

fn cell_text(e: &Entry, c: Col) -> String {
    match c {
        Col::Modified => crate::ui::format_visit(e.meta.modified),
        Col::Created => crate::ui::format_visit(e.meta.created),
        Col::Visited => crate::ui::format_visit(visited(e)),
    }
}

/// The title column grows three times as fast as the others.
fn weight3(mut d: Div) -> Div {
    d.style().flex_grow = Some(3.);
    d
}

/// A document's icon: its emoji, or the monochrome page glyph.
fn entry_icon(e: &Entry) -> AnyElement {
    if e.meta.folder.is_some() && e.note.is_none() { return icon("folder", 22., Theme::icon()).mr(px(13.)).into_any_element(); }
    match &e.icon {
        _ if e.meta.named_by_file() => crate::search::doc_icon_named(crate::search::file_glyph(&e.meta)).mr(px(12.)).into_any_element(),
        Some(em) => div().size(px(24.)).mr(px(12.)).flex_none().flex().items_center().justify_center().text_size(px(18.)).child(em.clone()).into_any_element(),
        None => doc_icon().mr(px(12.)).into_any_element(),
    }
}

impl Workspace {
    /// In 批量管理 a click checks or unchecks; otherwise it opens.
    pub(crate) fn card_click(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.batch.is_none() { return self.open(id, window, cx) }
        if !self.in_batch_scope(id) { return }
        if !self.toggle_check(id) {
            self.show_toast_kind(crate::ui::ToastKind::Error, &crate::localized_format!("一次最多选择 {} 项", "Select up to {} items at a time", crate::sidebar::BATCH_LIMIT), cx);
        }
        cx.notify();
    }

    /// The row's / card's checkbox while 批量管理 is on.
    pub(crate) fn card_check(&self, e: &Entry) -> Option<crate::design::Checkbox> {
        use crate::sidebar::{BATCH_LIMIT, Check, checkbox};
        let sel = self.batch.as_ref().filter(|_| self.in_batch_scope(&e.id))?;
        let on = sel.contains(&e.id);
        Some(checkbox(if on { Check::On } else { Check::Off }, !on && sel.len() >= BATCH_LIMIT))
    }

    /// One level of the tree as a folder's page shows it.
    /// Cards show no body, links or reminders: the entries come without them (`tree::face`).
    pub(crate) fn level_entries(&self, parent: Option<&str>) -> Rc<Vec<Entry>> {
        let (key, order) = (self.entries.generation(), self.folder_page.order());
        let mut level = self.folder_page.level.borrow_mut();
        if let Some(l) = level.as_ref().filter(|l| l.key == key && l.order == order && l.parent.as_deref() == parent) { return l.entries.clone(); }
        // A folder's 正文 is the folder's own card, not one inside it.
        let notes: std::collections::HashSet<&str> = self.entries.iter().filter_map(|e| e.note.as_deref()).collect();
        let entries = Rc::new(self.sorted_entries(self.entries.iter().filter(|e| e.meta.parent.as_deref() == parent && !notes.contains(e.id.as_str())).map(crate::tree::face).collect()));
        *level = Some(Level { key, parent: parent.map(str::to_string), order, entries: entries.clone() });
        entries
    }

    pub(crate) fn prepare_manual_order(&mut self, cx: &mut Context<Self>) -> bool {
        if self.tree_writes_pending() { return false; }
        let previous = self.store.track_file_changes();
        match self.store.initialize_manual_order() {
            Err(e) => { self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx); return false; }
            // Already in order: nothing written, the rows stand.
            Ok(false) => return true,
            Ok(true) => self.entries.set(self.store.list()),
        }
        self.remember_files(previous, crate::settings::tr("初始化手动排序", "Initialize manual order"));
        true
    }

    pub(crate) fn manual_page_sort(&self) -> bool { self.folder_page.sort == PageSort::Manual }

    /// A list header: clicking it sorts by that column; clicking the sorted
    /// one flips the direction. Headers without an order are not clickable.
    fn sort_head(&self, id: SharedString, label: &'static str, sorted: bool, on: Option<PageSort>, cx: &mut Context<Self>) -> Stateful<Div> {
        let dir = self.folder_page.sort_dir;
        div()
            .id(id)
            .relative()
            .px(px(4.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(ROW_RADIUS))
            .text_size(px(13.))
            .line_height(px(24.))
            .text_color(if sorted { Theme::text() } else { list_header_text() })
            .when(sorted, |d| d.font_weight(FontWeight::SEMIBOLD))
            .when(on.is_some(), |d| d.cursor_pointer().hover(|s| s.bg(Theme::fill_hover())))
            .child(crate::settings::label(label))
            .when(sorted, |d| d.child(sort_glyph(dir, 12.)))
            .when_some(on, |d, field| d.on_click(cx.listener(move |w, _, _, cx| w.sort_by(field, cx))))
    }

    /// Pick a sort field. Picking the current one flips 升序 / 降序; picking
    /// 手动排序 freezes the order shown right now, so later edits cannot
    /// reshuffle it.
    fn sort_by(&mut self, field: PageSort, cx: &mut Context<Self>) {
        if field == PageSort::Manual {
            if self.folder_page.sort != PageSort::Manual && !self.prepare_manual_order(cx) { return; }
            self.folder_page.sort = PageSort::Manual;
        } else if self.folder_page.sort == field {
            self.folder_page.sort_dir = self.folder_page.sort_dir.flip();
        } else {
            self.folder_page.sort = field;
            self.folder_page.sort_dir = field.default_dir();
        }
        self.save_page_view();
        self.folder_page.folder_grid_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.folder_page.folder_list_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// Mix folders and documents unless the user explicitly groups folders first.
    fn sorted_entries(&self, mut v: Vec<Entry>) -> Vec<Entry> {
        sort_entries(&mut v, self.folder_page.order());
        v
    }

    fn thumb(&self, e: &Entry) -> Thumb {
        use std::hash::{Hash, Hasher};
        let mut stamp = rustc_hash::FxHasher::default();
        e.meta.modified.hash(&mut stamp);
        crate::settings::feishu().hash(&mut stamp);
        // a 数据库's rows live in its file, saved apart from the index: its time is the card's too
        let file_time = (e.meta.kind == crate::store::DocKind::Table).then(|| self.store.file_path(&e.id).and_then(|p| modified(&p))).flatten();
        file_time.hash(&mut stamp);
        // and a page's embedded tables: a row changed there redraws the card
        let cached_tables = self.folder_page.thumbs.borrow().get(&e.id).map(|(_, t, _)| tables_time(&t.tables)).unwrap_or(0);
        let mut page_stamp = stamp.clone();
        cached_tables.hash(&mut stamp);
        let base_stamp = stamp.finish();
        // If the preview contains no sibling subpages, modified + settings uniquely
        // identifies it without rebuilding the whole tree on every frame.
        let listed = (self.entries.generation(), base_stamp);
        if let Some((m, t, used)) = self.folder_page.thumbs.borrow_mut().get_mut(&e.id)
            && (if t.has_subpages { t.listed == listed } else { *m == base_stamp }) { *used = web_time::Instant::now(); return t.clone(); }
        // A child-page list shows the other pages in the document's folder, as in
        // the open document (`sync_sub_pages`).
        let tree = crate::tree::Tree::new(&self.entries);
        // A document with children (a folder's 正文) lists them.
        let level = if e.note.is_some() { Some(e.id.as_str()) } else { e.meta.parent.as_deref() };
        let children: Vec<&Entry> = tree.children(level).iter().copied().filter(|p| p.id != e.id).collect();
        let mut full_stamp = rustc_hash::FxHasher::default();
        e.meta.modified.hash(&mut full_stamp);
        file_time.hash(&mut full_stamp);
        crate::settings::feishu().hash(&mut full_stamp);
        for child in &children { (&child.id, &child.title, &child.icon).hash(&mut full_stamp); }
        let mut children_stamp = full_stamp.clone();
        cached_tables.hash(&mut full_stamp);
        let full_stamp = full_stamp.finish();
        if let Some((m, t, used)) = self.folder_page.thumbs.borrow_mut().get_mut(&e.id)
            && *m == full_stamp {
            *used = web_time::Instant::now();
            if t.has_subpages && t.listed != listed { *t = Rc::new(ThumbContent { blocks: t.blocks.clone(), eqnos: t.eqnos.clone(), grids: t.grids.clone(), has_subpages: true, tables: t.tables.clone(), listed }); }
            return t.clone();
        }
        let _s = crate::devtools::span("thumb-miss");
        // The derived rows exist only in this preview tree and never enter the editor/store.
        fn subpages(blocks: &mut [crate::model::Block], entries: &[&Entry], budget: &mut usize) {
            for b in blocks {
                if matches!(b.kind, BlockKind::SubPageList) {
                    b.children = entries.iter().take(*budget).map(|e| crate::model::Block::text(&format!("{}{}", e.icon.as_ref().map(|i| format!("{i} ")).unwrap_or_default(), display_title(&e.title)))).collect();
                    *budget -= b.children.len();
                } else {
                    subpages(&mut b.children, entries, budget);
                    if let BlockKind::Table(t) = &mut b.kind { for row in &mut t.rows { for cell in row { subpages(&mut cell.blocks, entries, budget); } } }
                }
            }
        }
        let mut budget = 32;
        let (mut eqnos, mut grids) = (HashMap::new(), HashMap::new());
        let doc = { let _s = crate::devtools::span("thumb-load"); self.store.load(e.note.as_deref().unwrap_or(&e.id)) };
        let loaded = doc.as_ref().map(|doc| {
            // a table file: its grid, the card's whole face (its title is the card's)
            if e.meta.kind == crate::store::DocKind::Table {
                return doc.blocks.iter().filter_map(|b| match &b.kind {
                    BlockKind::DataTable(r) => { let g = card_grid(&crate::store::Store::active_dir(), r, false)?; let copy = b.clone(); grids.insert(copy.id, g); Some(copy) }
                    _ => None,
                }).collect();
            }
            let numbers = crate::editor::equation_numbers(doc);
            thumb_blocks(&doc.blocks, &mut budget, 0, &numbers, &mut eqnos, &mut grids)
        });
        let read = loaded.is_ok();
        let mut blocks = loaded.unwrap_or_default();
        let has_subpages = has_subpage_list(&blocks);
        if has_subpages { subpages(&mut blocks, &children, &mut budget); }
        let mut tables = vec![];
        if let Ok(doc) = &doc { table_srcs(&doc.blocks, &mut tables); }
        // stamped with the tables just found, so the next frame finds it
        let tt = tables_time(&tables);
        let stamp = if has_subpages { tt.hash(&mut children_stamp); children_stamp.finish() } else { tt.hash(&mut page_stamp); page_stamp.finish() };
        let t: Thumb = Rc::new(ThumbContent { blocks, eqnos, grids, has_subpages, tables, listed });
        // A failed read (mid-write by another process, still in iCloud) is not
        // remembered: the card would stay blank until the document changes.
        if !read { return t; }
        let mut thumbs = self.folder_page.thumbs.borrow_mut();
        // Keep recent previews warm without retaining every document ever visited.
        if thumbs.len() >= 128 && !thumbs.contains_key(&e.id) {
            let oldest = thumbs.iter().min_by_key(|(_, (_, _, used))| *used).map(|(id, _)| id.clone());
            if let Some(id) = oldest { thumbs.remove(&id); }
        }
        thumbs.insert(e.id.clone(), (stamp, t.clone(), web_time::Instant::now()));
        t
    }

    fn open_row_menu(&mut self, id: &str, key: &str, cx: &mut Context<Self>) {
        let b = self.more_bounds.borrow().get(key).copied();
        let Some(b) = b else { return };
        let (pos, corner) = crate::design::hang(b, crate::design::Align::End, None);
        self.row_menu = Some(RowMenu { id: id.to_string(), pos, corner, sub: false, batch: false, from: Some(key.to_string()) });
        cx.notify();
    }

    pub(crate) fn card_more(&self, e: &Entry, key: String, cx: &mut Context<Self>) -> crate::design::Button {
        let id = e.id.clone();
        let (rec, k2) = (self.more_bounds.clone(), key.clone());
        let open = self.row_menu.as_ref().is_some_and(|m| m.id == e.id && m.from.as_deref() == Some(key.as_str()));
        crate::design::Button::ghost(SharedString::from(key.clone()))
            .sized(crate::design::ButtonSize::Xs)
            .relative()
            .icon("ellipsis")
            .selected(open)
            .child(canvas(move |b, _, _| {
                rec.borrow_mut().insert(key.clone(), b);
            }, |_, _, _, _| {}).absolute().inset_0())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |w, _, _, cx| {
                cx.stop_propagation();
                w.open_row_menu(&id, &k2, cx);
            }))
    }

    /// A newly opened folder page starts at its top.
    pub(crate) fn reset_folder_scroll(&self) {
        self.folder_page.folder_grid_scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.folder_page.folder_list_scroll.set_offset(point(px(0.), px(0.)));
    }

    /// The folder pages' view and sort order are remembered (`settings.json`).
    /// Whether folder pages list by hand (手动排序) under these settings, as `load_page_view` reads them.
    pub(crate) fn sorts_by_hand(s: &crate::store::Settings) -> bool { s.page_sort.as_deref().map(PageSort::of).unwrap_or(PageSort::Manual) == PageSort::Manual }

    pub(crate) fn load_page_view(&mut self) {
        let s = self.store.settings();
        self.folder_page.grid = !s.page_list;
        self.folder_page.folders_first = s.page_folders_first;
        self.folder_page.sort = s.page_sort.as_deref().map(PageSort::of).unwrap_or(PageSort::Manual);
        // Settings from before directions existed keep the order they used to show.
        self.folder_page.sort_dir = s.page_sort_dir.as_deref().map(SortDir::of).unwrap_or_else(|| self.folder_page.sort.default_dir());
        self.store.set_place_by_hand(self.folder_page.sort == PageSort::Manual);
    }

    fn save_page_view(&mut self) {
        let mut s = self.store.settings();
        s.page_folders_first = self.folder_page.folders_first;
        (s.page_list, s.page_sort) = (!self.folder_page.grid, Some(self.folder_page.sort.key().to_string()));
        s.page_sort_dir = Some(self.folder_page.sort_dir.key().to_string());
        self.store.set_place_by_hand(self.folder_page.sort == PageSort::Manual);
        if let Err(e) = self.store.save_settings(&s) { eprintln!("无法保存文件夹视图：{e}"); }
    }

    /// No folder open (2.8): nothing to list or sort, a way to open one.
    pub(crate) fn render_empty(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let header = crate::trash::page_header_with(self, div(), 18., cx);
        div().flex_1().min_w_0().h_full().flex().flex_col().child(header).child(self.empty_main(cx)).into_any_element()
    }

    pub(crate) fn render_page_list(&self, entries: &[Entry], cx: &mut Context<Self>) -> AnyElement {
        let list_scroll = &self.folder_page.folder_list_scroll;
        let cols = COLS.to_vec();
        let sort = self.folder_page.sort_col();
        // The title is sized at about three times a column.
        let title_cell = || div().flex_grow_1().flex_shrink_1().flex_basis(relative(0.)).min_w(px(200.));
        let col_cell = || div().flex_grow_1().flex_shrink_1().flex_basis(relative(0.)).max_w(px(240.)).min_w(px(120.));
        // The list: 13pt tertiary headers over a hairline; the
        // sorted column in the text colour, semibold, with its arrow.
        // A row is lit while hovered or holding its menu; the lines touching it hide.
        let lit = |i: usize| entries.get(i).is_some_and(|e| {
            self.folder_page.list_hover.as_ref().is_some_and(|h| *h == format!("page-row-{}", e.id))
                || self.row_menu.as_ref().is_some_and(|m| m.id == e.id)
        });
        // its top where the grid's first row starts, 64pt down the window
        let mut header = div()
            .h(px(33.))
            .mt(px(8.))
            .flex_none()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(row_line(lit(0)))
            .child(
                weight3(title_cell()).pl(px(20.)).child(
                    self.sort_head(SharedString::from("page-col-title"), "标题", self.folder_page.sort == PageSort::Title, Some(PageSort::Title), cx),
                ),
            );
        for c in &cols {
            let sorted = sort == Some(*c);
            header = header.child(
                col_cell().pl(px(20.)).child(
                    self.sort_head(SharedString::from(format!("page-col-{}", c.label())), c.label(), sorted, c.sort(), cx),
                ),
            );
        }
        header = header.child(div().w(px(60.)).flex_none());
        let mut rows = div().id("page-rows").on_mouse_move(cx.listener(|w, ev: &MouseMoveEvent, _, cx| w.track_card_press(ev, cx)))
            .flex_1().min_h_0().overflow_y_scroll().track_scroll(list_scroll).px(px(24.)).pb(px(24.));
        if entries.is_empty() {
            rows = rows.child(div().pt(px(72.)).child(empty_state("illus/empty-list.svg", crate::settings::text("当前列表暂无内容"))));
        }
        for (i, e) in entries.iter().enumerate() {
            let (id, id2, arm) = (e.id.clone(), e.id.clone(), e.id.clone());
            let menu_on = self.row_menu.as_ref().is_some_and(|m| m.id == e.id);
            let check = self.card_check(e);
            let key = SharedString::from(format!("page-row-{}", e.id));
            // Carried away: its place an empty row, as a card's is.
            if self.card_dragging(&e.id) {
                let slot = div().id(key).relative().h(px(56.)).child(self.record_card(&e.id))
                    .child(div().absolute().inset_0().rounded(px(ROW_RADIUS)).bg(Theme::fill_hover()));
                rows = rows.child(self.card_motion(&e.id, slot.into_any_element()));
                continue;
            }
            let over = self.card_drop_over(&e.id);
            let mut row = div()
                .id(key.clone())
                .relative().child(self.record_card(&e.id)).children(crate::devtools::anchor_probe(format!("list:{}", display_title(&e.title))))
                .when(check.is_none(), |d| d.on_mouse_down(MouseButton::Left, cx.listener(move |w, ev: &MouseDownEvent, _, cx| w.arm_card_drag(&arm, ev.position, cx))))
                .when(over, |d| d.bg(Theme::sidebar_drop()).rounded(px(ROW_RADIUS)))
                .h(px(56.))
                .flex()
                .items_center()
                .when(i > 0, |d| d.border_t_1().border_color(row_line(lit(i - 1) || lit(i))))
                .cursor_pointer()
                .hover(row_hover)
                .on_hover(cx.listener(move |w, on: &bool, _, cx| if w.folder_page.hover_row(&key, *on) { cx.notify() }))
                .when(menu_on, |d| d.bg(Theme::fill_hover()).rounded(px(ROW_RADIUS)))
                .on_click(cx.listener(move |w, _, window, cx| w.card_click(&id, window, cx)))
                .when(check.is_none(), |d| d.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                        w.row_menu = Some(RowMenu { id: id2.clone(), pos: ev.position, corner: Anchor::TopLeft, sub: false, batch: false, from: None });
                        cx.notify();
                    }),
                ))
                .child(
                    weight3(title_cell())
                        .pl(px(24.))
                        .pr(px(16.))
                        .flex()
                        .items_center()
                        .when_some(check, |d, c| d.child(c.mr(px(12.))))
                        .child(entry_icon(e))
                        .child(div().min_w_0().text_size(px(15.)).line_height(px(22.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(display_title(&e.title))))
                        .when(e.meta.pinned, |d| d.child(icon("pin-filled", 14., Theme::pinned()).ml(px(6.)))),
                );
            for c in &cols {
                row = row.child(col_cell().pl(px(24.)).text_size(px(13.)).text_color(Theme::text_caption()).whitespace_nowrap().overflow_hidden().child(cell_text(e, *c)));
            }
            row = row.child(div().w(px(60.)).flex_none().flex().justify_end().pr(px(12.)).when(self.batch.is_none(), |d| d.child(self.card_more(e, format!("page-more-{}", e.id), cx))));
            rows = rows.child(self.card_motion(&e.id, row.into_any_element()));
        }
        div().flex_1().min_h_0().flex().flex_col().child(header.mx(px(24.)))
            .child(div().relative().flex_1().min_h_0().flex().flex_col().child(rows).child(self.folder_page.list_scrollbar.render(list_scroll))).into_any_element()
    }

    pub(crate) fn render_page_grid(&self, entries: &Rc<Vec<Entry>>, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let grid_scroll = &self.folder_page.folder_grid_scroll;
        let main_w = f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let avail = (main_w - 48.).max(1.);
        let entries = entries.clone();
        let mixed = entries.iter().any(|e| e.meta.folder.is_some() && e.note.is_none());
        // Folders spill past their cell (shadow, tab, the front flaring open), so
        // a level holding them gets more air between cells.
        let gap = if mixed { 24. } else { 16. };
        let wide_cols = (((avail + gap) / (200. + gap)).floor() as usize).max(1);
        let n = if wide_cols >= 4 { (((avail + gap) / (240. + gap)).floor() as usize).max(4) } else { (((avail + gap) / (160. + gap)).floor() as usize).clamp(1, 3) };
        // A card is always 5:6 at its column's width, a folder's too: its tab
        // inside that height, so folders and documents share top and bottom
        // edges (the maintainer, 2026-11-27).
        let col_w = (avail - gap * (n - 1) as f32) / n as f32;
        let card_h = col_w * (6. / 5.);
        let rows = entries.len().div_ceil(n);
        let grid = uniform_list("page-grid-area", rows, cx.processor(move |workspace, range: std::ops::Range<usize>, window, cx| {
            let cards: Vec<AnyElement> = range.map(|row| {
                let start = row * n;
                let end = (start + n).min(entries.len());
                div().w_full().h(px(card_h + gap)).px(px(24.)).pb(px(gap))
                    .grid().grid_cols(n as u16).gap(px(gap))
                    .children(entries[start..end].iter().map(|e| {
                        // A folder with a 正文 is a document with child pages: a document's card (#357).
                        let card = if e.meta.folder.is_some() && e.note.is_none() { workspace.render_folder_card(e, col_w, card_h, window, cx) }
                        else { workspace.render_doc_card(e, card_h, cx) };
                        workspace.card_motion(&e.id, card)
                    }))
                    .into_any_element()
            }).collect();
            fetch_card_grids(cx);
            cards
        }))
        // The last row's own 16 plus 32 more: 48 below the cards at the end,
        // clear of the window's rounded corner. (32: GPUI 0.2's uniform_list
        // counted its padding twice, and the cards end where that put them.)
        // The top padding sits inside the list's clip, so a folder's papers can
        // rise above the first row without being cut off — and that clip line is
        // where a row scrolling out of sight stops. It used to sit 4pt higher,
        // the list lifted into the tab row above it, so a scrolling row drew over
        // the tabs and the buttons beside them (#238). 16 under the header (the
        // maintainer, 2026-11-26): the first row's top 64pt down the window, the list
        // view's header where it starts; rows scroll right up to the header.
        .flex_1().min_h_0().pt(px(16.)).pb(px(32.)).track_scroll(grid_scroll);
        let scroll = grid_scroll.0.borrow().base_handle.clone();
        div().relative().flex_1().min_h_0().flex().flex_col()
            .on_mouse_move(cx.listener(|w, ev: &MouseMoveEvent, _, cx| w.track_card_press(ev, cx)))
            .child(grid).child(self.folder_page.grid_scrollbar.render(&scroll)).into_any_element()
    }

    /// A document's first page in miniature: a card's face, and the papers
    /// in a folder that show one (`folder_card.rs`). `pad`: its margin.
    pub(crate) fn thumb_page(&self, e: &Entry, pad: f32) -> Div {
        let thumb = self.thumb(e);
        self.thumb_page_with(e, &thumb, pad, 0., GRID_ROOM)
    }

    /// `indent`: room before the title for 批量管理's checkbox; `h`: the card's height (a table file's grid is drawn to it).
    fn thumb_page_with(&self, e: &Entry, thumb: &Thumb, pad: f32, indent: f32, h: f32) -> Div {
        if thumb.blocks.is_empty() && e.title.trim().is_empty() {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .child(img(crate::trash::illus_for("illus/blank-doc.svg")).size(px(64.)))
                    .child(div().mt(px(4.)).text_size(px(14.)).line_height(px(22.)).text_color(Theme::text_placeholder()).child(crate::settings::text("空白文档")))
            } else {
                let mut p = div().size_full().p(px(pad)).overflow_hidden().flex().flex_col();
                p = p.child(div().flex_none().flex().items_start().gap(px(6.)).mb(px(8.)).pl(px(indent))
                    .when_some(e.icon.as_ref(), |d, emoji| d.child(div().flex_none().text_size(px(14.)).line_height(px(20.)).child(emoji.clone())))
                    .child(div().min_w_0().text_size(px(14.)).line_height(px(20.)).font_weight(FontWeight::BOLD).child(e.title.clone())));
                // A 画板: its board, drawn to fit the rest of the card.
                if e.meta.kind == crate::store::DocKind::Board {
                    let board = thumb.blocks.iter().find_map(|b| match &b.kind { BlockKind::Board(bd) => Some(bd.clone()), _ => None }).unwrap_or_default();
                    return p.child(canvas(|_, _, _| {}, move |bounds, _, window, cx| crate::board::render::paint_card(&board, bounds, window, cx)).flex_1().w_full());
                }
                // A 数据库: its grid's top-left, the rows the card has room for.
                if e.meta.kind == crate::store::DocKind::Table {
                    return p.children(thumb.blocks.first().and_then(|b| thumb.grids.get(&b.id)).map(|g| g.element(bs::SCALE, h - pad - 28.)));
                }
                for (i, block) in thumb.blocks.iter().enumerate() {
                    let (mt, _) = bs::gap(&block.kind);
                    let prev = if i == 0 { 0. } else { bs::gap(&thumb.blocks[i - 1].kind).1 };
                    p = p.child(div().flex_none().mt(px(mt.max(prev) * bs::SCALE)).child(thumb_block(block, 0, Theme::text(), false, thumb)));
                }
                p
            }
    }

    fn render_doc_card(&self, e: &Entry, card_h: f32, cx: &mut Context<Self>) -> AnyElement {
            const RING: f32 = 3.;
            let id = e.id.clone();
            let id2 = e.id.clone();
            let thumb = self.thumb(e);
            let radius = 32.;
            let menu_on = self.row_menu.as_ref().is_some_and(|m| m.id == e.id);
            let check = self.card_check(e);
            // 批量管理: the title steps aside for the checkbox, which sits on its first line.
            let page = self.thumb_page_with(e, &thumb, 18., if check.is_some() { CHECK_ROOM } else { 0. }, card_h);
            let arm = e.id.clone();
            // Carried away as a paper: its place stays as an empty slot of the
            // card's shape (no page, no cover), so nothing half-shows through.
            if self.card_dragging(&e.id) {
                return div().id(SharedString::from(format!("page-card-{}", e.id))).w_full().min_w_0().h(px(card_h)).relative()
                    .child(self.record_card(&e.id))
                    .child(crate::ui::squircle_surface(radius, Theme::fill_hover()).size_full())
                    .into_any_element();
            }
                div()
                    .id(SharedString::from(format!("page-card-{}", e.id)))
                    .when(check.is_none(), |d| d.on_mouse_down(MouseButton::Left, cx.listener(move |w, ev: &MouseDownEvent, _, cx| w.arm_card_drag(&arm, ev.position, cx))))
                    .child(self.record_card(&e.id)).children(crate::devtools::anchor_probe(format!("card:{}", display_title(&e.title))))
                    .w_full().min_w_0().h(px(card_h)).relative().group("page-card")
                    .flex_none()
                    // A loose sheet on a folder page: squircle corners, a 0.5pt
                    // hairline and the faintest lift; hover darkens the hairline.
                    .cursor_pointer()
                    // Inset as the folder card's shadow box is (6 a side, 10 from the top).
                    .child(div().absolute().left(px(6.)).right(px(6.)).top(px(10.)).bottom_0().rounded(px(radius * 0.6)).shadow(crate::design::shadow::sheet()))
                    .on_click(cx.listener(move |w, _, window, cx| w.card_click(&id, window, cx)))
                    .when(check.is_none(), |d| d.on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                            w.row_menu = Some(RowMenu { id: id2.clone(), pos: ev.position, corner: Anchor::TopLeft, sub: false, batch: false, from: None });
                            cx.notify();
                        }),
                    ))
                    .child(crate::ui::squircle_surface(radius, Theme::bg_body()).size_full().overflow_hidden().child(page.relative()))
                    .child(crate::ui::squircle_outline(radius, 0.5, Theme::line_sheet()))
                    // Hovered, or its menu open: a soft grey ring around the card.
                    .child(div().absolute().left(px(-RING)).right(px(-RING)).top(px(-RING)).bottom(px(-RING))
                        .when(!menu_on, |d| d.opacity(0.).group_hover("page-card", |s| s.opacity(1.)))
                        .child(crate::ui::squircle_outline(radius + RING, RING, Theme::ring_sheet())))
                    .when_some(check, |d, c| d.child(div().absolute().top(px(18.)).left(px(18.)).h(px(20.)).flex().items_center().child(c)))
                    .when(self.batch.is_none(), |d| d.child(
                        div().absolute().top(px(8.)).right(px(8.))
                            .when(!menu_on, |d| d.opacity(0.).group_hover("page-card", |s| s.opacity(1.)))
                            .child(self.card_more(e, format!("page-gmore-{}", e.id), cx).rounded_full().bg(Theme::bg_overlay())),
                    ))
            .into_any_element()
    }

    /// 显示设置 and the sort dropdown: the menu panel; the sort item is a
    /// native menu row.
    pub(crate) fn render_page_menus(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = vec![];
        if !matches!(self.view, crate::app::View::Default | crate::app::View::Folder) {
            return out;
        }
        let place = |pos: Point<Pixels>, corner: Anchor, menu: Popover| {
            deferred(anchored().anchor(corner).position(pos).snap_to_window_with_margin(px(8.)).child(menu.fit(crate::design::room(pos, corner)))).with_priority(2).into_any_element()
        };
        if self.folder_page.view_menu && let Some(b) = self.folder_page.anchor("page-view") {
            let (pos, corner) = crate::design::hang(b, crate::design::Align::End, None);
            out.push(place(pos, corner, self.view_menu(cx)));
        }
        out
    }
}

#[cfg(test)]
mod sort_tests {
    // Not `use super::*`: it would bring in gpui's `test` attribute macro.
    use super::{PageSort, Order, SortDir, sort_entries};
    use crate::store::Entry;

    fn page(id: &str, title: &str, modified: u64, created: u64, order: u64, folder: bool) -> Entry {
        Entry {
            id: id.into(),
            meta: crate::store::Meta {
                modified,
                created,
                order,
                // A page inside a folder: its hand order is `order`, not
                // `top_order` (which is only for the top level).
                parent: Some("p".into()),
                folder: folder.then(|| format!("{id}-dir")),
                ..Default::default()
            },
            title: title.into(),
            icon: None,
            body: "".into(),
            reminders: vec![],
            links: vec![],
            note: None,
            top: String::new(),
        }
    }

    fn ids(v: &[Entry]) -> Vec<&str> { v.iter().map(|e| e.id.as_str()).collect() }

    #[test]
    fn directions_apply_to_every_field() {
        let mut v = vec![
            page("a", "Banana", 20, 200, 3, false),
            page("b", "Apple", 10, 100, 2, false),
            page("c", "Cherry", 30, 300, 1, false),
        ];
        sort_entries(&mut v, Order { field: PageSort::Modified, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["c", "a", "b"]);
        sort_entries(&mut v, Order { field: PageSort::Modified, dir: SortDir::Asc, folders_first: false });
        assert_eq!(ids(&v), ["b", "a", "c"]);
        sort_entries(&mut v, Order { field: PageSort::Created, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["c", "a", "b"]);
        sort_entries(&mut v, Order { field: PageSort::Title, dir: SortDir::Asc, folders_first: false });
        assert_eq!(ids(&v), ["b", "a", "c"]);
        sort_entries(&mut v, Order { field: PageSort::Title, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["c", "a", "b"]);
    }

    #[test]
    fn titles_sort_naturally_and_without_case() {
        let mut v = vec![
            page("ten", "图10", 0, 0, 1, false),
            page("upper", "Report 09", 0, 0, 2, false),
            page("two", "图2", 0, 0, 3, false),
            page("lower", "report 9", 0, 0, 4, false),
        ];
        sort_entries(&mut v, Order { field: PageSort::Title, dir: SortDir::Asc, folders_first: false });
        // Equal folded titles keep their hand order.
        assert_eq!(ids(&v), ["upper", "lower", "two", "ten"]);
        sort_entries(&mut v, Order { field: PageSort::Title, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["ten", "two", "upper", "lower"]);
    }

    #[test]
    fn kinds_group_then_sort_names() {
        use crate::store::DocKind;
        let mut folder = page("folder", "Same", 0, 0, 1, true);
        let doc = page("doc", "Same", 0, 0, 2, false);
        let mut md = page("md", "Same", 0, 0, 3, false);
        let mut board = page("board", "Same", 0, 0, 4, false);
        let mut table = page("table", "Same", 0, 0, 5, false);
        md.meta.markdown = true;
        board.meta.kind = DocKind::Board;
        table.meta.kind = DocKind::Table;
        // A folder's 正文 belongs with its folder, not with Markdown pages.
        folder.meta.markdown = true;
        let mut v = vec![table, board.clone(), md, doc, folder];
        sort_entries(&mut v, Order { field: PageSort::Kind, dir: SortDir::Asc, folders_first: false });
        assert_eq!(ids(&v), ["folder", "doc", "md", "board", "table"]);
        sort_entries(&mut v, Order { field: PageSort::Kind, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["table", "board", "md", "doc", "folder"]);
        // The same kind is alphabetical, not its hand order.
        let other = page("board2", "图2", 0, 0, 0, false);
        board.title = "图10".into();
        let mut v = vec![board, other];
        sort_entries(&mut v, Order { field: PageSort::Kind, dir: SortDir::Asc, folders_first: false });
        assert_eq!(ids(&v), ["board2", "board"]);
        assert_eq!(PageSort::of(PageSort::Kind.key()), PageSort::Kind);
        assert_eq!(PageSort::Kind.default_dir(), SortDir::Asc);
        assert!(PageSort::Kind.directional());
    }

    #[test]
    fn manual_order_is_the_sidebar_order() {
        let mut v = vec![
            page("a", "A", 10, 10, 2, false),
            page("b", "B", 99, 99, 1, false),
            // Never placed by hand: above the placed ones, newest first.
            page("c", "C", 50, 50, 0, false),
        ];
        sort_entries(&mut v, Order { field: PageSort::Manual, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["c", "b", "a"]);
    }

    #[test]
    fn equal_values_keep_the_hand_order() {
        // Same modification time: the hand order decides, in either direction.
        let mut v = vec![page("a", "A", 10, 10, 2, false), page("b", "B", 10, 10, 1, false)];
        sort_entries(&mut v, Order { field: PageSort::Modified, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["b", "a"]);
        sort_entries(&mut v, Order { field: PageSort::Modified, dir: SortDir::Asc, folders_first: false });
        assert_eq!(ids(&v), ["b", "a"]);
        // Title has no eponymous column, but sorts the same way.
        let mut v = vec![page("a", "Same", 10, 10, 2, false), page("b", "Same", 20, 20, 1, false)];
        sort_entries(&mut v, Order { field: PageSort::Title, dir: SortDir::Asc, folders_first: false });
        assert_eq!(ids(&v), ["b", "a"]);
    }

    #[test]
    fn folders_first_groups_only_when_asked() {
        let mut v = vec![page("d", "Doc", 30, 30, 2, false), page("f", "Folder", 20, 20, 3, true)];
        sort_entries(&mut v, Order { field: PageSort::Modified, dir: SortDir::Desc, folders_first: false });
        assert_eq!(ids(&v), ["d", "f"]);
        sort_entries(&mut v, Order { field: PageSort::Modified, dir: SortDir::Desc, folders_first: true });
        assert_eq!(ids(&v), ["f", "d"]);
        // 手动排序 never groups: it is the order the sidebar shows.
        sort_entries(&mut v, Order { field: PageSort::Manual, dir: SortDir::Desc, folders_first: true });
        assert_eq!(ids(&v), ["d", "f"]);
    }

    #[test]
    fn top_rows_keep_their_order() {
        // 多个顶层文件夹: the rows stay in folders.json's order (`top_order`) whatever the sort.
        let row = |key: &str, title: &str, at: u64| {
            let mut e = page(&crate::shared_store::top_row(key), title, 0, 0, 0, true);
            (e.meta.parent, e.meta.top_order) = (None, Some(at));
            e
        };
        let mut v = vec![row("b", "Alpha", 2), row("a", "Zulu", 1), page("x", "Middle", 5, 5, 0, false)];
        for field in [PageSort::Title, PageSort::Kind, PageSort::Modified, PageSort::Manual] {
            for dir in [SortDir::Asc, SortDir::Desc] {
                sort_entries(&mut v, Order { field, dir, folders_first: false });
                assert_eq!(ids(&v), ["top:a", "top:b", "x"]);
            }
        }
    }
}
