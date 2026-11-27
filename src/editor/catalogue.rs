//! 文档目录: the heading outline at the top left of the page, and the page
//! margins that make room for it.

use super::core::TITLE;
use super::render::{TOP, flattens, visible_with_gaps};
use super::view::{Editor, EditorEvent};
use crate::model::*;
use crate::feishu_theme::{self as theme, Theme};
use crate::editor_ui::{TipExt, TipSide, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::Cell;
use std::rc::Rc;

/// PAGE_MAIN_PADDING: the page's minimum side margin.
const PAD: f32 = 66.;
/// The side margin on a phone (`theme::compact`).
const COMPACT_PAD: f32 = 20.;
/// CATALOGUE_MAX_UNFOLD_WIDTH.
const UNFOLD: f32 = 236.5;
/// EDITOR_MIN_EXTRUDING_WIDTH: the narrowest text column beside a pinned catalogue.
const EDITOR_MIN: f32 = 520.;
/// RESPONSE_MIN: the narrowest pinned catalogue.
const RESPONSE_MIN: f32 = 144.;
/// RESPONSE_MAX.
const RESPONSE_MAX: f32 = 265.;
/// Catalogue padding-top; the pin row (PIN_HEIGHT 40) sits below it.
const CAT_TOP: f32 = 54.;
const PIN_H: f32 = 40.;
/// Item height: a 22px line plus 8px padding-bottom.
/// 36pt on touch: denser than a 44pt list, a long outline reads at a glance.
const ROW_H: f32 = if crate::design::touch() { 36. } else { 30. };
/// Indent per level.
const INDENT: f32 = 14.;
/// A heading this close below the viewport top already counts as current.
const ACTIVE_LINE: f32 = 90.;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CatMode {
    /// No headings: no catalogue at all.
    #[default]
    None,
    /// The list is shown in an area this wide.
    Pinned(f32),
    /// Only the 展开目录 button.
    Unpinned,
    /// The pane is too narrow to pin: show the manually opened list as an overlay.
    Forced,
}

/// Side margins of the text column and the catalogue state for one pane width.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PageLayout {
    pub ml: f32,
    pub mr: f32,
    /// Width of the text column.
    pub content: f32,
    pub cat: CatMode,
}

pub fn page_layout(o: f32, max: Option<f32>, has_cat: bool, pinned: bool) -> PageLayout {
    // A phone (iOS, narrow pane): the text takes the width, with the system's
    // usual side margin.
    // The outline opens from the top bar's button instead, floating over the text.
    if crate::theme::compact(o) {
        let cat = if has_cat && pinned { CatMode::Forced } else { CatMode::None };
        return PageLayout { ml: COMPACT_PAD, mr: COMPACT_PAD, content: o - 2. * COMPACT_PAD, cat };
    }
    let centered = |cat| {
        let m = max.map_or(PAD, |w| ((o - w) / 2.).max(PAD));
        PageLayout { ml: m, mr: m, content: o - 2. * m, cat }
    };
    if !has_cat {
        return centered(CatMode::None);
    }
    if !pinned {
        return centered(CatMode::Unpinned);
    }
    if o < RESPONSE_MIN + PAD + EDITOR_MIN + PAD {
        return centered(CatMode::Forced);
    }
    let (ml, mr) = match max {
        None => (UNFOLD + PAD, PAD),
        Some(w) if o - PAD >= UNFOLD + PAD + EDITOR_MIN => {
            let a = (o - UNFOLD - PAD).min(w);
            let l = o - a;
            if l / 2. < UNFOLD + PAD {
                let l = l - UNFOLD;
                (UNFOLD + (l / 2.).max(PAD), (l / 2.).max(PAD))
            } else {
                (l / 2., l / 2.)
            }
        }
        Some(_) => (o - EDITOR_MIN - PAD, PAD),
    };
    PageLayout { ml, mr, content: o - ml - mr, cat: CatMode::Pinned(ml - PAD) }
}

/// One catalogue row.
#[derive(Clone, Debug)]
pub struct CatEntry {
    pub id: BlockId,
    pub text: SharedString,
    /// 1 for the title and the top heading level present.
    pub level: usize,
    pub h1: bool,
    /// The page list item showing it (or the one hiding it).
    pub item: usize,
}

/// The title plus every heading outside tables and callouts, in page order.
/// Levels are ranks among the heading levels present, so a document of h2
/// and h4 indents only once.
pub fn entries(doc: &Document) -> Vec<CatEntry> {
    fn walk(list: &[Block], item: &mut usize, shown: bool, raw: &mut Vec<(BlockId, String, u8, usize)>) {
        if list.is_empty() {
            return;
        }
        let mut visible = vec![false; list.len()];
        if shown {
            visible_with_gaps(list, TOP).into_iter().for_each(|(i, _)| visible[i] = true);
        }
        for (i, b) in list.iter().enumerate() {
            let row = visible[i];
            if row {
                *item += 1;
            }
            if let BlockKind::Heading { level, .. } = b.kind {
                raw.push((b.id, b.text.text.clone(), level, *item));
            }
            match b.kind {
                BlockKind::Table(_) | BlockKind::Callout { .. } => {}
                _ => walk(&b.children, item, row && flattens(b), raw),
            }
        }
    }
    let mut raw = vec![];
    walk(&doc.blocks, &mut 0, true, &mut raw);
    if raw.is_empty() {
        return vec![];
    }
    // Numbered headings read "1.2 Title".
    let orders = super::core::heading_orders(&doc.blocks);
    for r in &mut raw {
        if let Some(o) = orders.get(&r.0) {
            r.1 = format!("{o} {}", r.1);
        }
    }
    let mut levels: Vec<u8> = raw.iter().map(|r| r.2).collect();
    levels.sort();
    levels.dedup();
    let title = crate::settings::display_title(&doc.title.text);
    // The 文档图标 leads the title entry.
    let title = match &doc.icon {
        Some(icon) => format!("{icon} {title}"),
        None => title.to_string(),
    };
    let mut out = vec![CatEntry { id: TITLE, text: title.into(), level: 1, h1: false, item: 0 }];
    out.extend(raw.into_iter().map(|(id, text, l, item)| CatEntry {
        id,
        text: text.replace('\n', " ").into(),
        level: levels.iter().position(|x| *x == l).unwrap_or(0) + 1,
        h1: l == 1,
        item,
    }));
    out
}

thread_local! {
    /// Last pane size any editor was laid out at, so a newly opened document
    /// gets its margins right on the first frame.
    static PANE: Cell<Size<Pixels>> = const { Cell::new(size(px(1012.), px(736.))) };
}

impl Editor {
    /// The text column's max width: a theme sets 标准 (max-width); 较宽 / 全宽 stay.
    pub(super) fn column_max(&self) -> Option<f32> {
        match self.page_width {
            crate::store::PageWidth::Standard => Some(crate::editor_theme::style().num("page.width")),
            w => w.max(),
        }
    }

    /// Pane size for this frame's layout (last frame's list viewport).
    pub(super) fn pane_size(&self) -> Size<Pixels> {
        let v = self.list.viewport_bounds().size;
        if v.width > px(0.) {
            PANE.with(|p| p.set(v));
            v
        } else {
            PANE.with(|p| p.get())
        }
    }

    /// Width of the text column.
    pub(super) fn content_width(&self) -> f32 {
        self.layout.content
    }

    /// How far a wide page-level table may reach past the text column on
    /// each side: to the page's minimum margin, as in Feishu, but not over
    /// a pinned catalogue (nor nearer to it than that margin).
    pub(super) fn bleed(&self) -> (f32, f32) {
        let l = match self.layout.cat {
            CatMode::Pinned(u) => self.layout.ml - PAD - u.min(RESPONSE_MAX),
            _ => self.layout.ml - PAD,
        };
        (l.max(0.), (self.layout.mr - PAD).max(0.))
    }

    /// Recomputes the outline and the page margins for this frame.
    pub(super) fn update_catalogue(&mut self) {
        if self.cat_generation != Some(self.core.generation) {
            self.cat_generation = Some(self.core.generation);
            self.cat_entries = Rc::new(entries(&self.core.doc));
        }
        let o = f32::from(self.pane_size().width);
        // 外观 › 文档大纲 off, or 专注模式: as a document without headings, no outline and no button
        let shown = !crate::settings::prefs().hide_outline && !self.focus_mode && !self.cat_entries.is_empty();
        let pinned = if crate::theme::compact(o) { self.cat_float } else { self.cat_pinned };
        self.layout = page_layout(o, self.column_max(), shown, pinned);
    }

    /// Scroll-spy: the last heading at most 90px below the viewport top. A
    /// caret move without scrolling selects the heading above the caret.
    /// Runs at paint time, once this frame's block bounds are known; returns
    /// whether the active row changed.
    pub(super) fn update_active(&mut self) -> bool {
        let before = self.cat_active;
        self.spy();
        self.cat_active != before
    }

    fn spy(&mut self) {
        let top = self.list.logical_scroll_top();
        let scroll = (top.item_ix, f32::from(top.offset_in_item).to_bits());
        let head = self.core.sel.head.block;
        let entries = self.cat_entries.clone();
        if entries.is_empty() {
            self.cat_active = None;
            return;
        }
        let scrolled = self.cat_seen.0 != scroll;
        let moved = self.cat_seen.1 != head;
        self.cat_seen = (scroll, head);
        // A clicked entry stays active through its jump (even when the page
        // can't scroll it up to the top).
        if let Some(id) = self.cat_jump
            && entries.iter().any(|e| e.id == id)
            && !moved
        {
            if self.reveal.is_none() {
                self.cat_jump = None;
            }
            return;
        }
        self.cat_jump = None;
        if moved && !scrolled {
            let at = self.item_of_block(head);
            self.cat_active = entries.iter().rev().find(|e| e.item <= at).map(|e| e.id);
        } else if scrolled || self.cat_active.is_none_or(|id| !entries.iter().any(|e| e.id == id)) {
            let view = self.list.viewport_bounds();
            let bounds = self.block_bounds.borrow();
            self.cat_active = entries
                .iter()
                .rev()
                .find(|e| {
                    e.id == TITLE
                        || e.item < top.item_ix
                        || bounds.get(&e.id).is_some_and(|b| b.top() - view.top() <= px(ACTIVE_LINE))
                })
                .map(|e| e.id);
        }
    }

    fn item_of_block(&self, id: BlockId) -> usize {
        self.item_of(id).unwrap_or(0)
    }

    /// A phone (`theme::compact`): the top bar's outline button, shown when
    /// the document has headings, and whether the outline is open.
    /// It has its own state, never saved: the document's pinned outline is the desktop's.
    pub fn outline(&self) -> Option<bool> { (!self.cat_entries.is_empty()).then_some(self.cat_float) }

    pub fn toggle_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cat_float = !self.cat_float;
        // Opened over the note: its keyboard, edit menu and handles step aside.
        if self.cat_float { self.step_aside(window, cx); }
        cx.notify();
    }

    /// Something opens over the note (a drawer, the outline): the keyboard
    /// goes with the focus, and with it the touch edit menu, the selection's
    /// handles and the format toolbar (all drawn only while focused).
    pub fn step_aside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.touch.menu = false;
        self.touch.handle = None;
        if self.focus.is_focused(window) { window.blur(cx); }
        cx.notify();
    }

    fn set_cat_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
        self.cat_pinned = pinned;
        cx.emit(EditorEvent::Catalogue(pinned));
        cx.notify();
    }

    fn jump_to_heading(&mut self, id: BlockId, cx: &mut Context<Self>) {
        // A phone's outline floats over the text: it closes once used.
        if self.cat_float { self.cat_float = false; cx.notify(); }
        self.cat_active = Some(id);
        self.cat_jump = Some(id);
        if id == TITLE {
            self.list.scroll_to(ListOffset { item_ix: 0, offset_in_item: px(0.) });
        } else {
            self.core.unfold_to(id);
            if self.core.dirty {
                self.core.dirty = false;
                cx.emit(super::view::EditorEvent::Changed);
            }
            self.reveal = Some((super::render::Reveal::Block(id), 0));
        }
        cx.notify();
    }

    pub(super) fn render_catalogue(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mode = self.layout.cat;
        if mode == CatMode::None {
            return None;
        }
        // Touch: a tapped block's handle sits where the folded outline's
        // button does (the first paragraph on screen); the button steps aside
        // while it shows.
        if self.touch.on && mode == CatMode::Unpinned && self.handle_block().is_some() {
            return None;
        }
        let compact = crate::theme::compact(f32::from(self.pane_size().width));
        if compact { return Some(self.render_outline_drawer(window, cx)); }
        let floating = mode == CatMode::Forced;
        let pinned = self.cat_pinned;
        let pin = crate::design::Button::ghost(SharedString::from(format!("cat-pin-{pinned}")))
            .sized(crate::design::ButtonSize::Xs)
            .absolute()
            .left(px(14.))
            .top(px(10.))
            .child(icon(if pinned { "catalogue-collapse" } else { "catalogue-list" }, 16., crate::design::color::muted_foreground()))
            .tip_label(if pinned { crate::settings::text("收起目录") } else { crate::settings::text("展开目录") }.into(), TipSide::Above)
            .on_click(cx.listener(move |ed, _, _, cx| ed.set_cat_pinned(!pinned, cx)));

        let list_w = match mode {
            CatMode::Pinned(u) => Some(u.min(RESPONSE_MAX) - 4.),
            CatMode::Forced if floating => Some(UNFOLD),
            _ => None,
        };
        let pane = self.pane_size();
        // Below the cover while it is in view.
        let top = CAT_TOP + self.core.doc.cover.as_ref().map_or(0., |_| {
            let at = self.list.logical_scroll_top();
            if at.item_ix == 0 { (super::cover::cover_height(window) - f32::from(at.offset_in_item)).max(0.) } else { 0. }
        });
        let body = list_w.map(|w| self.catalogue_list(w, f32::from(pane.height) - top - PIN_H - 40., window, cx));
        Some(
            div()
                .id("catalogue")
                .absolute()
                .left_0()
                .top(px(top))
                .pl(px(4.))
                .pt(px(PIN_H))
                .block_mouse_except_scroll()
                .cursor(CursorStyle::Arrow)
                .when(list_w.is_none(), |d| d.w(px(42.)).h(px(PIN_H)).pt_0())
                .when(floating, |d| d.bg(theme::ta(255, 255, 255, 0.96)).rounded(px(8.)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .when(!compact, |d| d.child(pin))
                .when(compact, |d| d.pt(px(8.)).bg(Theme::bg_body()).shadow(crate::design::shadow::popover()))
                .children(body)
                .into_any_element(),
        )
    }

    /// A phone's outline: a drawer from the right over the whole window, as
    /// the sidebar's is from the left; tapping the scrim or a heading closes it.
    fn render_outline_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let size = window.viewport_size();
        let (vw, vh) = (f32::from(size.width), f32::from(size.height));
        let insets = window.insets();
        let (top, bottom) = (f32::from(insets.safe_area.top), f32::from(insets.safe_area.bottom));
        let w = (vw * 0.8).min(300.);
        let body = self.catalogue_list(w - 16., vh - top - bottom - 24. - 44., window, cx);
        deferred(anchored().position(point(px(0.), px(0.))).child(
            div().id("outline-drawer").relative().w(px(vw)).h(px(vh))
                .child(div().id("outline-scrim").absolute().inset_0().bg(crate::design::color::overlay()).occlude()
                    .on_mouse_down(MouseButton::Left, cx.listener(|ed, _, _, cx| { cx.stop_propagation(); ed.cat_float = false; cx.notify(); })))
                .child(div().id("outline-panel").absolute().top_0().right_0().h_full().w(px(w)).pt(px(top + 12.)).pb(px(bottom)).pl(px(8.))
                    .bg(Theme::bg_body()).shadow(crate::design::shadow::dialog()).occlude().cursor(CursorStyle::Arrow)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    // Named, as a panel is: 目录.
                    .child(div().h(px(44.)).pl(px(24.)).flex().items_center().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text())
                        .child(crate::settings::tr("目录", "Outline")))
                    // Clipped under the header: a scrolled row doesn't show above it.
                    .child(div().overflow_hidden().child(body))),
        )).with_priority(3).into_any_element()
    }

    fn catalogue_list(&mut self, width: f32, max_h: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Catalogue-only folding: hide everything deeper below a folded row.
        let entries = self.cat_entries.clone();
        let mut rows: Vec<(CatEntry, bool)> = vec![];
        let mut hide_below: Option<usize> = None;
        for (i, e) in entries.iter().enumerate() {
            if let Some(l) = hide_below {
                if e.level > l {
                    continue;
                }
                hide_below = None;
            }
            let parent = e.id != TITLE && entries.get(i + 1).is_some_and(|n| n.level > e.level);
            if parent && self.cat_collapsed.contains(&e.id) {
                hide_below = Some(e.level);
            }
            rows.push((e.clone(), parent));
        }
        let active = self.cat_active;
        if active != self.cat_shown_active {
            self.cat_shown_active = active;
            if let Some(ix) = rows.iter().position(|(e, _)| Some(e.id) == active) {
                self.cat_scroll.scroll_to_item(ix, ScrollStrategy::Top);
            }
        }
        let collapsed = self.cat_collapsed.clone();
        let mut style = window.text_style();
        style.font_family = theme::ui_family().into();
        let entity = cx.entity();
        let n = rows.len();
        let rows = Rc::new(rows);
        let h = (n as f32 * ROW_H + 22.).min(max_h.max(ROW_H));
        let scrolls = n as f32 * ROW_H + 22. > h;
        // a list that scrolls runs out to the side, its rows `room_for` clear of the shared scrollbar's lane
        let text_w = if scrolls { width - crate::design::scroll::room_for(0.) } else { width - 11. };
        let list = uniform_list("catalogue-list", n, move |range, window, _| {
            range
                .map(|ix| {
                    let (e, parent) = &rows[ix];
                    let folded = collapsed.contains(&e.id);
                    let title = e.id == TITLE;
                    // Touch: an iOS list's density, body-size text centred in
                    // its 44pt row, the deeper levels a step smaller.
                    let touch = crate::design::touch();
                    let size = match (touch, title || e.level <= 2) { (true, true) => 17., (true, false) => 15., (false, _) => if title { 14. } else { 13. } };
                    let fold_w = if touch { 24. } else { 16. };
                    let indent = if touch { 16. } else { INDENT } * (e.level - 1) as f32;
                    let avail = text_w - indent - fold_w;
                    let runs = crate::editor_ui::ui_runs(&e.text, &TextStyle { font_size: px(size).into(), ..style.clone() }, &[], style.color);
                    let full_w = window.text_system().shape_line(e.text.clone(), px(size), &runs, None).width;
                    // The outline: quiet secondary rows, the current one in full text colour.
                    let on = Some(e.id) == active;
                    let color = if on { Theme::text() } else { Theme::text_caption() };
                    let (id, ent, ent2) = (e.id, entity.clone(), entity.clone());
                    let group = SharedString::from(format!("cat-row-{ix}"));
                    let row = div()
                        .id(("cat-row", ix))
                        .group(group.clone())
                        .w(px(text_w))
                        .h(px(ROW_H))
                        .when(!touch, |d| d.pb(px(8.)))
                        .when(touch, |d| d.items_center())
                        .pl(px(indent))
                        .flex()
                        .cursor_pointer()
                        .child(
                            div()
                                .id(("cat-fold", ix))
                                .flex_none()
                                .w(px(fold_w))
                                .h(px(if touch { ROW_H } else { 22. }))
                                .when(!touch, |d| d.px(px(3.)).py(px(6.)))
                                .when(touch, |d| d.flex().items_center().justify_center())
                                // Touch has no hover: the triangles always show.
                                .when(*parent, |d| {
                                    d.child(
                                        div()
                                            .size(px(if touch { 12. } else { 10. }))
                                            .when(!folded && !touch, |d| d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.)))
                                            .child(
                                                icon(if folded { "triangle-right" } else { "triangle-down" }, if touch { 12. } else { 10. }, Theme::icon())
                                                    .hover(|s| s.text_color(Theme::text())),
                                            ),
                                    )
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        ent2.update(cx, |ed, cx| {
                                            if !ed.cat_collapsed.remove(&id) {
                                                ed.cat_collapsed.insert(id);
                                            }
                                            cx.notify();
                                        });
                                    })
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(22.))
                                .text_size(px(size))
                                .line_height(px(22.))
                                .when(title || e.h1 || on, |d| d.font_weight(FontWeight::SEMIBOLD))
                                .text_color(color)
                                .hover(|s| s.text_color(Theme::text()))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(ui_text(e.text.clone())),
                        )
                        .on_click(move |_, _, cx| ent.update(cx, |ed, cx| ed.jump_to_heading(id, cx)));
                    if !touch && f32::from(full_w) > avail { row.tip_label(e.text.to_string(), TipSide::Right) } else { row }
                })
                .collect()
        })
        .track_scroll(&self.cat_scroll)
        .w(px(if scrolls { width } else { width - 11. }))
        .h(px(h))
        // A list that scrolls ends 44 below its last row: GPUI 0.2's
        // uniform_list counted the bottom padding twice.
        .pb(px(if scrolls { 44. } else { 22. }));
        // A list that scrolls keeps the wheel: the page under it stays put.
        let handle = self.cat_scroll.0.borrow().base_handle.clone();
        div().relative().w(px(width)).when(scrolls, |d| d.on_scroll(|_, _, cx| cx.stop_propagation())).child(list).child(self.cat_bar.render(&handle)).into_any_element()
    }
}

