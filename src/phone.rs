//! A phone's folder pages, laid out as an iOS list app lays them out (备忘录,
//! 文件): one 44pt bar (the drawer's button or 「‹」, then 显示 · 搜索 · … as a
//! folder page's header has them) over one scrolling column — a search field
//! that hides under the bar until the list is pulled down, a large title that
//! the bar takes over once it scrolls away, then the cards or rows. A row
//! swipes left for 置顶 / 删除, a long press lifts it over a menu with the
//! page's preview, and 新建 floats at the bottom right, under the thumb.
//!
//! Only `theme::compact` panes come here; every other window draws folder
//! pages as before (`folders.rs`, `folder_page.rs`).

use crate::app::{RowMenu, Workspace};
use crate::design::{MenuItem, MenuSeparator, color};
use crate::settings::{display_title, tr};
use crate::store::Entry;
use crate::theme::{self, Theme};
use crate::ui::{icon, list_line, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The bar's height (UINavigationBar's).
pub(crate) const BAR_H: f32 = 44.;
/// The search field's slot at the top of the column: 36pt and 8 under it.
const SEARCH_H: f32 = 44.;
/// The large title's slot: 34pt bold on a 41pt line, 8 under it.
const TITLE_H: f32 = 49.;
const ROW_H: f32 = 60.;
/// A row at the reader's Dynamic Type size: its two lines grow, its padding does not.
fn row_h() -> f32 { ROW_H - 40. + text(22.) + text(18.) }
/// One swipe action's width: a glyph alone, as iOS Mail's (the row's icon and date stay in sight).
const ACTION_W: f32 = 60.;
/// The same in the drawer, whose rows are shorter.
const DRAWER_ACTION_W: f32 = 52.;
/// A row pulled past this share of its width deletes when let go.
const FULL: f32 = 0.6;
/// A settling swipe, the search field snapping, a menu fading in.
const SETTLE: Duration = Duration::from_millis(220);
const FAB: f32 = 56.;

#[derive(Clone, Copy, PartialEq)]
enum Action { Pin, Delete }

/// A row pulled left over its actions.
struct Swipe {
    id: String,
    /// How far the row is pulled, in points.
    x: f32,
    /// The finger is on it.
    drag: bool,
    /// After the finger lifts: (since, from, to).
    settle: Option<(Instant, f32, f32)>,
    /// The row is the drawer's (it goes with the drawer), and its width there.
    drawer: bool,
    width: f32,
    /// Let go past this, the row is deleted.
    full: f32,
}

/// A long-pressed row, lifted over its menu.
struct Held {
    id: String,
    since: Instant,
    /// A drawer row's or a card's press, the finger still down: moving it carries the row
    /// (`sidebar_drag.rs`) or the card (`folder_card.rs`) instead.
    press: Option<Point<Pixels>>,
    card: bool,
}

#[derive(Default)]
pub struct Phone {
    scroll: ScrollHandle,
    /// The page the column last showed: a new one starts with its search field tucked away.
    shown: Option<String>,
    /// The search field snapping open or shut once the finger lifts: (since, from, to).
    snap: Option<(Instant, f32, f32)>,
    swipe: Option<Swipe>,
    held: Option<Held>,
    /// Each row's bounds, by entry, as last painted.
    rows: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// A phone's page was drawn in the last frame.
    paged: bool,
}

fn ease(t: f32) -> f32 { 1. - (1. - t).powi(3) }

/// `(value now, still moving)` of a `(since, from, to)` animation.
fn settle((t0, from, to): (Instant, f32, f32)) -> (f32, bool) {
    // Reduce Motion: there at once.
    if system().reduce_motion { return (to, false) }
    let t = (t0.elapsed().as_secs_f32() / SETTLE.as_secs_f32()).min(1.);
    (from + (to - from) * ease(t), t < 1.)
}

impl Workspace {
    pub(crate) fn phone(&self, window: &Window) -> bool { theme::compact(f32::from(window.viewport_size().width)) }

    /// The swipe actions a row has, leftmost first. A top-level folder's row and a
    /// file outside the library keep to their own menus.
    fn phone_actions(&self, e: &Entry) -> &'static [Action] {
        if crate::shared_store::top_of_row(&e.id).is_some() || e.meta.local() { &[] }
        else if e.meta.folder.is_some() && e.note.is_none() { &[Action::Delete] }
        else { &[Action::Pin, Action::Delete] }
    }

    fn phone_scroll_handle(&self) -> ScrollHandle { self.phone.scroll.clone() }

    /// A phone's cards scroll in its column: a carried card scrolls that (`folder_card.rs`).
    pub(crate) fn phone_grid_scroll(&self) -> Option<ScrollHandle> { self.phone.paged.then(|| self.phone.scroll.clone()) }

    fn row_at(&self, at: Point<Pixels>) -> Option<String> {
        self.phone.rows.borrow().iter().find(|(_, b)| b.contains(&at)).map(|(id, _)| id.clone())
    }

    fn close_swipe(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.phone.swipe.as_mut() && s.settle.is_none_or(|(_, _, to)| to != 0.) {
            s.drag = false;
            s.settle = Some((Instant::now(), s.x, 0.));
            cx.notify();
        }
    }

    /// A touch pan over the list, before the column scrolls: a sideways one pulls
    /// the row under it over its actions. Returns whether it was the row's.
    /// `row`: the row the pan started on and its width (None: not on a row); `drawer`: the sidebar's rows.
    fn phone_pan(&mut self, ev: &ScrollEvent, row: Option<(String, f32)>, drawer: bool, cx: &mut Context<Self>) -> bool {
        if ev.source != ScrollSource::Touch { return false }
        let d = ev.delta.pixel_delta(px(20.));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        match ev.touch_phase {
            TouchPhase::Started => {
                self.phone.snap = None;
                // A pan anywhere else puts an open row back.
                if self.phone.swipe.as_ref().is_some_and(|s| Some(&s.id) != row.as_ref().map(|r| &r.0)) { self.close_swipe(cx); }
                if dx.abs() <= dy.abs() { return false }
                let Some((id, width)) = row else { return false };
                if self.batch.is_some() || self.rename.is_some() || !self.entries.iter().any(|e| e.id == id && !self.phone_actions(e).is_empty()) { return false }
                let from = self.phone.swipe.as_ref().filter(|s| s.id == id).map_or(0., |s| s.x);
                // Opening only: a row at rest does not follow a finger going right.
                if from == 0. && dx > 0. { return false }
                // Let go past `full` it deletes: most of the way across, and clear of the open actions.
                let n = self.entries.iter().find(|e| e.id == id).map_or(0, |e| self.phone_actions(e).len()) as f32;
                let full = (width * FULL).max(n * if drawer { DRAWER_ACTION_W } else { ACTION_W } + 48.);
                self.phone.swipe = Some(Swipe { id, x: (from - dx).clamp(0., width), drag: true, settle: None, drawer, width, full });
                cx.notify();
                true
            }
            TouchPhase::Moved => {
                let Some(s) = self.phone.swipe.as_mut().filter(|s| s.drag && s.drawer == drawer) else { return self.phone.swipe.as_ref().is_some_and(|s| s.drawer == drawer) && dx.abs() > dy.abs() };
                let was = s.x;
                s.x = (s.x - dx).clamp(0., s.width);
                // Past this the release deletes: a tick as the finger crosses it, either way.
                let full = s.full;
                if (was > full) != (s.x > full) { haptic(Haptic::Light); }
                cx.notify();
                true
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(s) = self.phone.swipe.as_mut().filter(|s| s.drag && s.drawer == drawer) else { return false };
                s.drag = false;
                let (id, full) = (s.id.clone(), s.full);
                let actions = self.entries.iter().find(|e| e.id == id).map(|e| self.phone_actions(e)).unwrap_or(&[]);
                let open = actions.len() as f32 * if drawer { DRAWER_ACTION_W } else { ACTION_W };
                let s = self.phone.swipe.as_mut().unwrap();
                // Pulled most of the way across: the last action (删除), as a full swipe does on iOS.
                if s.x > full && actions.last() == Some(&Action::Delete) {
                    s.settle = Some((Instant::now(), s.x, 0.));
                    self.phone_act(&id, Action::Delete, cx);
                } else {
                    s.settle = Some((Instant::now(), s.x, if s.x > open / 2. { open } else { 0. }));
                }
                cx.notify();
                true
            }
        }
    }

    fn phone_act(&mut self, id: &str, a: Action, cx: &mut Context<Self>) {
        self.close_swipe(cx);
        let doc = self.note_or(id);
        match a {
            Action::Pin => self.toggle_pin(&doc, cx),
            Action::Delete => self.ask_delete(id, cx),
        }
    }

    /// A long press on a row: it lifts over its menu. Returns whether the press was a row's.
    fn phone_hold(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        if self.batch.is_some() { return false }
        let Some(id) = self.row_at(at) else { return false };
        let Some(e) = self.entries.iter().find(|e| e.id == id).cloned() else { return false };
        self.close_swipe(cx);
        haptic(Haptic::Medium);
        // Rows with menus of their own (a top-level folder, a file outside the library): that menu, at the finger.
        if crate::shared_store::top_of_row(&e.id).is_some() || e.meta.local() {
            self.row_menu = Some(RowMenu { id, pos: at, corner: Anchor::TopLeft, sub: false, batch: false, from: None });
        } else {
            self.phone.held = Some(Held { id, since: Instant::now(), press: None, card: false });
        }
        cx.notify();
        true
    }

    /// A long press on a drawer row: its menu, as on a list's row. The finger still
    /// down and moving carries the row instead (`held_moved`).
    pub(crate) fn hold_drawer_row(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.close_swipe(cx);
        haptic(Haptic::Medium);
        self.row_menu = None;
        self.phone.held = Some(Held { id: id.to_string(), since: Instant::now(), press: Some(at), card: false });
        cx.notify();
    }

    /// The same for a document's card on a phone's page.
    pub(crate) fn hold_card(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        haptic(Haptic::Medium);
        self.row_menu = None;
        self.phone.held = Some(Held { id: id.to_string(), since: Instant::now(), press: Some(at), card: true });
        cx.notify();
    }

    /// A phone's page was drawn last frame: its cards' long presses are menus first.
    pub(crate) fn phone_paged(&self) -> bool { self.phone.paged }

    /// The rest of a long press that opened a menu with the finger still down.
    fn held_follow(&mut self, ev: &LongPressEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.phone.held.as_ref().is_some_and(|h| h.card) { return self.on_row_long_press(ev, window, cx) }
        if let Ok(id) = self.held_moved(ev, cx) { self.lift_card(&id, ev.position, cx); }
    }

    /// A folder's card on a phone's page: a long press is its menu.
    fn card_hold(&self, id: &str, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (id, ws) = (id.to_string(), cx.entity());
        canvas(|b, window, _| window.insert_hitbox(b, HitboxBehavior::Normal), move |_, hitbox, window, _| {
            window.on_mouse_event(move |ev: &LongPressEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || ev.phase != TouchPhase::Started || !hitbox.is_hovered(window) { return }
                ws.update(cx, |w, cx| { haptic(Haptic::Medium); w.row_menu = None; w.phone.held = Some(Held { id: id.clone(), since: Instant::now(), press: None, card: true }); cx.notify(); });
                window.prevent_default();
                cx.stop_propagation();
            });
        }).absolute().inset_0()
    }

    /// A phone's cards, two to a row. Only the rows in or beside the window are built; the
    /// rest keep their room.
    fn phone_cards(&mut self, entries: &[Entry], y: f32, view_h: f32, window: &mut Window, cx: &mut Context<Self>) -> (Div, f32) {
        const COLS: usize = 2;
        let vw = f32::from(window.viewport_size().width);
        let mixed = entries.iter().any(|e| e.meta.folder.is_some() && e.note.is_none());
        let gap = if mixed { 20. } else { 14. };
        let col_w = (vw - 32. - gap * (COLS - 1) as f32) / COLS as f32;
        let doc_h = (col_w * 1.2).round();
        // a folder's card is a document's size, its tab inside it (`folder_page.rs`)
        let card_h = doc_h;
        let row_h = card_h + gap;
        let rows = entries.len().div_ceil(COLS);
        let first = (((y - SEARCH_H - TITLE_H - 8.) / row_h).floor() as isize - 1).max(0) as usize;
        let last = (first + (view_h / row_h).ceil() as usize + 2).min(rows);
        self.folder_page.thumb_deferred.set(true);
        let mut grid = div().relative().flex_none().pt(px(8.)).px(px(16.)).child(div().h(px(first as f32 * row_h)));
        for row in first..last {
            let mut line = div().h(px(row_h)).pb(px(gap)).flex().gap(px(gap));
            for e in &entries[row * COLS..((row + 1) * COLS).min(entries.len())] {
                let cell = div().relative().w(px(col_w)).h(px(card_h)).flex_none();
                line = line.child(if e.meta.folder.is_some() && e.note.is_none() {
                    cell.child(self.render_folder_card(e, col_w, card_h, window, cx)).child(self.card_hold(&e.id, cx))
                } else {
                    cell.child(self.render_doc_card(e, doc_h, cx))
                });
            }
            grid = grid.child(line);
        }
        self.folder_page.thumb_deferred.set(false);
        (grid.child(div().h(px((rows - last) as f32 * row_h))), 8. + rows as f32 * row_h)
    }

    /// The rest of that long press. `Some(id)`: the finger has moved off, the menu is gone
    /// and the row is to be picked up; otherwise whether the press is still the menu's.
    pub(crate) fn held_moved(&mut self, ev: &LongPressEvent, cx: &mut Context<Self>) -> Result<String, bool> {
        let Some(h) = self.phone.held.as_mut() else { return Err(false) };
        let Some(at) = h.press else { return Err(false) };
        match ev.phase {
            TouchPhase::Moved => {
                let d = ev.position - at;
                if f32::from(d.x).hypot(f32::from(d.y)) < 10. { return Err(true) }
                let id = self.phone.held.take().unwrap().id;
                cx.notify();
                Ok(id)
            }
            TouchPhase::Started => Err(true),
            // Let go where it was pressed: the menu stays.
            TouchPhase::Ended | TouchPhase::Cancelled => { h.press = None; Err(true) }
        }
    }

    /// A drawer row's sideways pan (the row's own listener: the tree has no one column of rows).
    pub(crate) fn drawer_row_swipe(&self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let (id, ws) = (id.to_string(), cx.entity());
        canvas(|b, window, _| (window.insert_hitbox(b, HitboxBehavior::Normal), b), move |_, (hitbox, b), window, _| {
            window.on_mouse_event(move |ev: &ScrollEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture { return }
                let on = hitbox.is_hovered(window);
                let taken = ws.update(cx, |w, cx| match ev.touch_phase {
                    TouchPhase::Started if on => w.phone_pan(ev, Some((id.clone(), f32::from(b.size.width))), true, cx),
                    TouchPhase::Started => false,
                    _ if w.phone.swipe.as_ref().is_some_and(|s| s.drawer && s.id == id) => w.phone_pan(ev, None, true, cx),
                    _ => false,
                });
                if taken { cx.stop_propagation(); }
            });
        }).absolute().inset_0().into_any_element()
    }

    /// How far a row is pulled over its actions.
    /// `drawer`: the drawer's row of `id`, else the list's (the two show the same page).
    pub(crate) fn swiped(&self, id: &str, drawer: bool) -> f32 { self.phone.swipe.as_ref().filter(|s| s.id == id && s.drawer == drawer).map_or(0., |s| s.x) }

    /// Each frame: a let-go row settles; the drawer's goes with the drawer.
    pub(crate) fn phone_tick(&mut self, window: &mut Window) {
        sheet_tick();
        self.phone.paged = false;
        // The drawer's row goes with the drawer; a list's, when the drawer comes over it.
        if self.phone.swipe.as_ref().is_some_and(|s| s.drawer != self.sidebar_open) { self.phone.swipe = None; }
        let Some(s) = self.phone.swipe.as_mut() else { return };
        if let Some(a) = s.settle {
            let (x, moving) = settle(a);
            s.x = x;
            if moving { window.request_animation_frame(); } else { s.settle = None; if x == 0. { self.phone.swipe = None; } }
        }
    }

    /// The actions behind a pulled row: `x` wide at its right end.
    pub(crate) fn swipe_strip(&self, e: &Entry, x: f32, cx: &mut Context<Self>) -> Div {
        let actions = self.phone_actions(e);
        let full = self.phone.swipe.as_ref().map_or(f32::MAX, |s| s.full);
        let full = x > full && actions.last() == Some(&Action::Delete);
        let mut strip = div().absolute().top_0().bottom_0().right_0().w(px(x)).flex().overflow_hidden();
        for a in actions {
            let (glyph, label, bg) = match a {
                Action::Pin => (if e.meta.pinned { "pin-off" } else { "pin" }, if e.meta.pinned { tr("取消置顶", "Unpin") } else { tr("置顶", "Pin") }, Theme::pinned()),
                Action::Delete => ("bin", tr("删除", "Delete"), Theme::danger()),
            };
            let (id, a) = (e.id.clone(), *a);
            // A full swipe: 删除 alone, across the row.
            if full && a != Action::Delete { continue }
            strip = strip.child(div().id(SharedString::from(format!("phone-act-{label}-{}", e.id))).flex_1().min_w_0().h_full().overflow_hidden().bg(bg).occlude()
                .flex().flex_col().items_center().justify_center().gap(px(3.))
                // glyphs alone (the label names it for scripts)
                .child(icon(glyph, 22., white()).flex_none())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |w, _, _, cx| { cx.stop_propagation(); w.phone_act(&id, a, cx) })));
        }
        strip
    }

    /// The listeners a list hangs on its column: sideways pans and long presses, taken before the rows see them.
    fn phone_gestures(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ws = cx.entity();
        canvas(|b, window, _| window.insert_hitbox(b, HitboxBehavior::Normal), move |_, hitbox, window, _| {
            let (w1, h1) = (ws.clone(), hitbox.clone());
            window.on_mouse_event(move |ev: &ScrollEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture { return }
                if ev.touch_phase == TouchPhase::Started && !h1.is_hovered(window) { return }
                let vw = f32::from(window.viewport_size().width);
                if w1.update(cx, |w, cx| { let row = w.row_at(ev.position).map(|id| (id, vw)); w.phone_pan(ev, row, false, cx) }) { cx.stop_propagation(); }
            });
            window.on_mouse_event(move |ev: &LongPressEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || ev.phase != TouchPhase::Started || !hitbox.is_hovered(window) { return }
                if ws.update(cx, |w, cx| w.phone_hold(ev.position, cx)) { window.prevent_default(); cx.stop_propagation(); }
            });
        }).absolute().inset_0()
    }

    fn phone_row(&self, e: &Entry, last: bool, cx: &mut Context<Self>) -> AnyElement {
        let folder = e.meta.folder.is_some() && e.note.is_none();
        let x = self.swiped(&e.id, false);
        let sub = if folder {
            let n = self.entries.iter().filter(|c| c.meta.parent.as_deref() == Some(e.id.as_str())).count();
            crate::localized_count!(n, "{} 项", "{} item", "{} items", n)
        } else { crate::ui::format_visit(if e.meta.modified > 0 { e.meta.modified } else { e.meta.created }) };
        let (id, rec, key) = (e.id.clone(), self.phone.rows.clone(), e.id.clone());
        let open = self.phone.swipe.is_some();
        // Pulled, the row gives up its right end to the actions: the icon and the date stay where they are.
        let face = div().id(SharedString::from(format!("phone-row-{}", e.id))).relative().mr(px(x)).h(px(row_h())).pl(px(16.)).flex().items_center()
            .bg(Theme::bg_body()).active(|s| s.bg(Theme::fill_hover()))
            .child(crate::folder_page::entry_icon(e))
            .child(div().flex_1().min_w_0().h_full().pr(px(12.)).flex().items_center().when(!last, |d| d.border_b_1().border_color(list_line()))
                .child(div().flex_1().min_w_0()
                    .child(div().text_size(px(text(17.))).line_height(px(text(22.))).text_color(Theme::text()).truncate().child(ui_text(display_title(&e.title))))
                    .child(div().text_size(px(text(13.))).line_height(px(text(18.))).text_color(Theme::text_caption()).truncate().child(sub)))
                .when(e.meta.pinned, |d| d.child(icon("pin-filled", 14., Theme::pinned()).ml(px(8.)).flex_none()))
                .when(folder, |d| d.child(icon("chevron-right", 16., color::muted_foreground()).ml(px(6.)).flex_none())))
            // An open row's tap closes it; otherwise it opens the page.
            .on_click(cx.listener(move |w, _, window, cx| if open { w.close_swipe(cx) } else { w.open(&id, window, cx) }));
        let mut row = div().relative().h(px(row_h())).flex_none().overflow_hidden()
            .child(canvas(move |b, _, _| { rec.borrow_mut().insert(key.clone(), b); }, |_, _, _, _| {}).absolute().inset_0());
        if x > 0. { row = row.child(self.swipe_strip(e, x, cx)); }
        row.child(face).into_any_element()
    }

    /// The bar: `lead` at the left, the title centred once the large one has scrolled
    /// under it (`shown` 0 to 1), `trail` at the right.
    fn phone_bar(&self, lead: AnyElement, title: String, shown: f32, trail: Vec<AnyElement>, window: &Window) -> Div {
        let vw = f32::from(window.viewport_size().width);
        div().relative().h(px(BAR_H)).flex_none().flex().items_center().pl(px(crate::sidebar::TOUCH_BUTTON_X)).pr(px(crate::ui::header_right_inset()))
            .child(div().absolute().top_0().bottom_0().left(px(96.)).w(px(vw - 192.)).flex().items_center().justify_center().opacity(shown)
                .child(div().truncate().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(ui_text(title))))
            .child(lead).child(div().flex_1()).children(trail)
            // The hairline under the bar once the list runs beneath it.
            .child(div().absolute().left_0().right_0().bottom_0().h(px(0.5)).bg(list_line()).opacity(shown))
    }

    /// A folder's page: the bar, then the column (search, large title, cards or rows) and 新建.
    pub(crate) fn render_phone_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let insets = window.insets();
        let (top, bottom) = (f32::from(insets.safe_area.top), f32::from(insets.safe_area.bottom));
        let vh = f32::from(window.viewport_size().height);
        let fid = self.folder_current.clone().unwrap_or_default();
        let title = self.entries.iter().find(|e| e.id == fid).map(|e| display_title(&e.title)).unwrap_or_else(|| tr("文件夹", "Folder").to_string());
        let entries = self.level_entries(Some(&fid));
        let scroll = self.phone_scroll_handle();
        // A page newly shown starts under its search field, as a pushed list does on iOS.
        if self.phone.shown.as_ref() != Some(&fid) {
            self.phone.shown = Some(fid.clone());
            scroll.set_offset(point(px(0.), px(-SEARCH_H)));
            self.phone.snap = None;
            self.phone.swipe = None;
        }
        if let Some(a) = self.phone.snap {
            let (y, moving) = settle(a);
            scroll.set_offset(point(px(0.), px(-y)));
            if moving { window.request_animation_frame(); } else { self.phone.snap = None; }
        }
        let y = -f32::from(scroll.offset().y);
        // The bar takes the title over as the large one's baseline passes under it.
        let shown = ((y - SEARCH_H - 28.) / 10.).clamp(0., 1.);
        self.phone.rows.borrow_mut().clear();
        let snap_scroll = scroll.clone();
        let mut column = div().id("phone-page-rows").relative().flex_1().min_h_0().overflow_y_scroll().track_scroll(&scroll)
            // The bar's title and the field's snap follow the column.
            .on_scroll(cx.listener(move |w, ev: &ScrollEvent, _, cx| {
                if ev.source == ScrollSource::Touch && matches!(ev.touch_phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                    // Left half-open, the search field settles open or shut.
                    let y = -f32::from(snap_scroll.offset().y);
                    if y > 0. && y < SEARCH_H { w.phone.snap = Some((Instant::now(), y, if y < SEARCH_H / 2. { 0. } else { SEARCH_H })); }
                }
                cx.notify();
            }))
            .child(div().h(px(SEARCH_H)).flex_none().px(px(16.))
                .child(div().id("phone-search").h(px(36.)).px(px(8.)).flex().items_center().gap(px(6.)).rounded(px(crate::design::R)).bg(color::muted()).active(|s| s.opacity(0.6))
                    .child(icon("search", 17., color::muted_foreground()).flex_none())
                    .child(div().text_size(px(17.)).text_color(color::muted_foreground()).child(tr("搜索", "Search")))
                    .on_click(cx.listener(|w, _, window, cx| w.open_search(window, cx)))))
            .child(div().h(px(TITLE_H)).flex_none().px(px(16.)).text_size(px(34.)).line_height(px(41.)).font_weight(FontWeight::BOLD).text_color(color::foreground()).truncate()
                .child(ui_text(title.clone())));
        if entries.is_empty() {
            column = column.child(div().h(px(vh - top - BAR_H - SEARCH_H - TITLE_H - 80.)).flex().child(crate::trash::empty_state("illus/empty-list.svg", tr("这个文件夹还是空的", "This folder is empty"))));
        }
        let n = entries.len();
        self.phone.paged = true;
        let (rows, rows_h) = if self.folder_page.phone_list {
            let mut rows = div().relative().flex_none().child(self.phone_gestures(cx));
            for (i, e) in entries.iter().enumerate() { rows = rows.child(self.phone_row(e, i + 1 == n, cx)); }
            (rows, n as f32 * row_h())
        } else { self.phone_cards(&entries, y, vh - top - BAR_H, window, cx) };
        // The previews those cards asked for are built between frames.
        self.pump_thumbs(cx);
        // Tall enough to scroll the search field away however short the list; clear of 新建 and the home indicator.
        let fill = (vh - top - BAR_H - TITLE_H - rows_h).max(FAB + 32. + bottom);
        column = column.child(rows).child(div().h(px(fill)).flex_none());
        // A top-level folder's page is the root: the drawer's button where 「‹」 would be.
        let lead = match self.back_button(cx) {
            Some(back) if !self.at_root(&crate::phone_nav::Place::Folder(fid.clone())) => back,
            _ => crate::design::Button::ghost("open-sidebar").icon(crate::sidebar::toggle_glyph(false))
                .on_click(cx.listener(|w, _, _, cx| { w.sidebar_open = true; cx.notify(); })).into_any_element(),
        };
        let trail = self.phone_bar_buttons(&fid, cx);
        let parent = Some(fid);
        let fab = div().id("phone-new").absolute().right(px(20.)).bottom(px(bottom.max(12.) + 12.)).size(px(FAB)).rounded_full()
            .bg(color::primary()).shadow(crate::design::shadow::popover()).flex().items_center().justify_center().active(|s| s.opacity(0.7))
            .child(icon("plus", 26., color::primary_foreground()))
            .child(self.folder_page.track("phone-new"))
            // One tap: what to make (文档, Markdown, 数据表, 画板, 文件夹, then 导入), as a folder's "+" asks on the desktop.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |w, ev: &ClickEvent, _, cx| {
                haptic(Haptic::Light);
                let pos = w.folder_page.anchor("phone-new").map_or(ev.position(), |b| point(b.right(), b.top() - px(8.)));
                w.create_menu = if w.create_menu.is_some() { None } else { Some((parent.clone(), pos)) };
                cx.notify();
            }));
        div().relative().flex_1().min_w_0().h_full().flex().flex_col()
            .child(self.phone_bar(lead, title, shown, trail, window))
            .child(column)
            .child(fab)
            .into_any_element()
    }

    /// The bar's right end, a folder page header's without its "+" (新建 is under the thumb):
    /// 显示 (view and sort), 搜索, and the folder's "…" (批量管理 first).
    fn phone_bar_buttons(&self, folder: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::folder_page::header::TB_MORE;
        let view_open = self.folder_page.view_menu;
        let display = crate::design::Button::ghost("page-view").icon(if self.folder_page.phone_list { "view-list" } else { "view-grid" }).relative().selected(view_open)
            .child(self.folder_page.track("page-view"))
            .on_mouse_down(MouseButton::Left, cx.listener(move |w, _, _, cx| { cx.stop_propagation(); w.folder_page.view_menu = !view_open; cx.notify(); }));
        let search = crate::design::Button::ghost("tb-search").icon("search")
            .on_click(cx.listener(|w, _, window, cx| w.open_search(window, cx)));
        let (id, open) = (folder.to_string(), self.row_menu.as_ref().is_some_and(|m| m.from.as_deref() == Some(TB_MORE)));
        let more = crate::design::Button::ghost("tb-more").icon("ellipsis").relative().selected(open).child(self.folder_page.track("tb-more"))
            .on_mouse_down(MouseButton::Left, cx.listener(move |w, _, _, cx| {
                cx.stop_propagation();
                let Some(b) = w.folder_page.anchor("tb-more") else { return };
                let (pos, corner) = crate::design::hang(b, crate::design::Align::End, None);
                w.row_menu = (!open).then(|| RowMenu { id: id.clone(), pos, corner, sub: false, batch: false, from: Some(TB_MORE.into()) });
                cx.notify();
            }));
        vec![display.into_any_element(), search.into_any_element(), more.into_any_element()]
    }

    /// The long-pressed row over a dimmed page: the page's first screen in
    /// miniature (a folder: its row), and its menu underneath.
    pub(crate) fn render_phone_held(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let held = self.phone.held.as_ref()?;
        // A drawer row's press, the finger still down: its movement is the row's (`sidebar_drag.rs`).
        let follow = held.press.is_some().then(|| { let ws = cx.entity(); canvas(|_, _, _| {}, move |_, _, window, _| {
            window.on_mouse_event(move |ev: &LongPressEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && ev.phase != TouchPhase::Started { ws.update(cx, |w, cx| w.held_follow(ev, window, cx)); }
            });
        }).absolute().size_0() });
        let Some(e) = self.entries.iter().find(|e| e.id == held.id).cloned() else { self.phone.held = None; return None };
        let t = if system().reduce_motion { 1. } else { (held.since.elapsed().as_secs_f32() / SETTLE.as_secs_f32()).min(1.) };
        if t < 1. { window.request_animation_frame(); }
        let a = ease(t);
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        let insets = window.insets();
        let (top, bottom) = (f32::from(insets.safe_area.top), f32::from(insets.safe_area.bottom));
        let folder = e.meta.folder.is_some() && e.note.is_none();
        let doc = self.note_or(&e.id);
        let pinned = self.entries.iter().find(|x| x.id == doc).is_some_and(|x| x.meta.pinned);
        let item = |id: &'static str, glyph: &'static str, label: &'static str| MenuItem::new(id).icon(glyph).label(label);
        let done = |f: fn(&mut Workspace, &str, &mut Window, &mut Context<Workspace>), id: &str| {
            let id = id.to_string();
            cx.listener(move |w: &mut Workspace, _: &ClickEvent, window, cx| { w.phone.held = None; f(w, &id, window, cx); cx.notify(); })
        };
        let mut rows: Vec<AnyElement> = vec![];
        if !folder {
            rows.push(item("ph-pin", if pinned { "pin-off" } else { "pin" }, if pinned { crate::settings::text("从“置顶”移除") } else { crate::settings::text("添加到“置顶”") })
                .on_click(done(|w, id, _, cx| w.toggle_pin(id, cx), &doc)).into_any_element());
            rows.push(MenuSeparator::new().into_any_element());
            rows.push(item("ph-copy", "copy", crate::settings::text("创建副本")).on_click(done(|w, id, window, cx| w.duplicate_doc(id, window, cx), &doc)).into_any_element());
        }
        rows.push(item("ph-move", "folder-input", crate::settings::text("移动到")).on_click(done(|w, id, window, cx| w.open_move(vec![id.to_string()], window, cx), &e.id)).into_any_element());
        rows.push(item("ph-rename", "pen-line", crate::settings::text("重命名")).on_click(done(|w, id, window, cx| w.start_rename(id, window, cx), &e.id)).into_any_element());
        rows.push(MenuSeparator::new().into_any_element());
        rows.push(item("ph-del", "bin", crate::settings::text("删除")).destructive(true).on_click(done(|w, id, _, cx| w.ask_delete(id, cx), &e.id)).into_any_element());
        let menu_h = rows.len() as f32 * crate::design::T.tool_h + 16.;
        let menu = div().id("phone-held-menu").w(px(250.)).py(px(6.)).px(px(6.)).rounded(px(14.)).bg(color::popover()).shadow(crate::design::shadow::popover()).occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).children(rows);
        // The preview takes what the menu leaves, up to a card 5:6 at most of the width.
        let room = vh - top - bottom - menu_h - 12. - 32.;
        let open = e.id.clone();
        let preview = if folder {
            let n = self.entries.iter().filter(|c| c.meta.parent.as_deref() == Some(e.id.as_str())).count();
            div().id("phone-held-preview").w(px(vw - 32.)).h(px(row_h() + 12.)).px(px(16.)).flex().items_center().rounded(px(14.)).bg(Theme::bg_body()).shadow(crate::design::shadow::popover())
                .child(crate::folder_page::entry_icon(&e))
                .child(div().flex_1().min_w_0()
                    .child(div().text_size(px(text(17.))).line_height(px(text(22.))).text_color(Theme::text()).truncate().child(ui_text(display_title(&e.title))))
                    .child(div().text_size(px(text(13.))).line_height(px(text(18.))).text_color(Theme::text_caption()).child(crate::localized_count!(n, "{} 项", "{} item", "{} items", n))))
        } else {
            let w = (vw - 96.).min(300.);
            let h = (w * 1.2).min(room).max(120.);
            div().id("phone-held-preview").w(px(w)).h(px(h)).rounded(px(18.)).overflow_hidden().bg(Theme::bg_body()).shadow(crate::design::shadow::popover())
                .child(self.thumb_page(&e, 18.))
        }.occlude().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |w, _, window, cx| { w.phone.held = None; w.open(&open, window, cx); }));
        Some(deferred(anchored().position(point(px(0.), px(0.))).child(
            div().id("phone-held").relative().w(px(vw)).h(px(vh)).pt(px(top + 16.)).pb(px(bottom + 16.)).px(px(16.)).flex().flex_col().justify_center().gap(px(12.)).opacity(a)
                .child(div().id("phone-held-scrim").absolute().inset_0().bg(black().opacity(0.28)).occlude()
                    .on_mouse_down(MouseButton::Left, cx.listener(|w, _, _, cx| { cx.stop_propagation(); w.phone.held = None; cx.notify(); })))
                .children(follow).child(preview).child(menu))).with_priority(2).into_any_element())
    }

    pub(crate) fn phone_pages(&self, window: &Window) -> bool {
        self.phone(window) && self.batch.is_none()
    }
}

/// A phone's bottom sheet (`settings::phone_sheet`), as `UISheetPresentationController`
/// keeps one: it slides up, its grabber drags it between half the screen and all of
/// it (a sheet as tall as its rows has the one size), and pulled down it goes away.
/// One sheet shows at a time, so one state, on the UI thread.
#[derive(Default)]
struct Sheet {
    id: &'static str,
    /// A tall sheet at its full height rather than half.
    full: bool,
    /// How far the finger (or the slide in and out) has it below its resting place; negative: above.
    pull: f32,
    drag: bool,
    settle: Option<(Instant, f32, f32)>,
    /// Sliding out: closed once it has.
    leaving: bool,
    /// Drawn in the window's last frame (`sheet_tick`): a sheet closed by one of its rows is forgotten.
    drawn: bool,
}

thread_local! { static SHEET: RefCell<Sheet> = RefCell::new(Sheet::default()); }

/// What `phone_sheet` draws this frame.
pub(crate) struct SheetFrame { pub full: bool, pub pull: f32, pub gone: bool }

/// Advances sheet `id` by a frame. `keyboard`: up, which takes a tall sheet to its full height.
pub(crate) fn sheet_frame(id: &'static str, keyboard: bool, window: &Window) -> SheetFrame {
    SHEET.with_borrow_mut(|s| {
        // Newly shown: it slides up from the bottom edge.
        if s.id != id { *s = Sheet { id, pull: 360., settle: Some((Instant::now(), 360., 0.)), ..Default::default() }; }
        s.drawn = true;
        if keyboard && !s.full { s.full = true; }
        let mut gone = false;
        if let Some(a) = s.settle {
            let (v, moving) = settle(a);
            s.pull = v;
            if moving { window.request_animation_frame(); } else { s.settle = None; gone = s.leaving; }
        }
        if gone { *s = Sheet::default(); }
        SheetFrame { full: s.full, pull: s.pull, gone }
    })
}

/// Each frame of the window, before its sheets are drawn: one not drawn last frame is gone.
pub(crate) fn sheet_tick() { SHEET.with_borrow_mut(|s| if !std::mem::take(&mut s.drawn) && !s.id.is_empty() { *s = Sheet::default(); }) }

/// No sheet shows any more (closed some other way): the next one slides in afresh.
pub(crate) fn sheet_closed(id: &'static str) { SHEET.with_borrow_mut(|s| if s.id == id { *s = Sheet::default(); }) }

/// A finger panning the sheet's grabber and title. `tall`: it has the two sizes;
/// `half`: how far below the full height the half one rests; `height`: the sheet's own.
/// Returns whether to redraw.
pub(crate) fn sheet_pan(id: &'static str, ev: &ScrollEvent, tall: bool, half: f32, height: f32) -> bool {
    if ev.source != ScrollSource::Touch { return false }
    let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
    SHEET.with_borrow_mut(|s| {
        if s.id != id || s.leaving { return false }
        match ev.touch_phase {
            TouchPhase::Started => { s.drag = true; s.settle = None; s.pull += dy; }
            TouchPhase::Moved if s.drag => s.pull += dy,
            TouchPhase::Moved => return false,
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if !std::mem::take(&mut s.drag) { return false }
                let now = Instant::now();
                // Where it rests from here: its other size, away, or back.
                if tall && s.full && s.pull > 60. && s.pull < half + 80. {
                    (s.full, s.pull) = (false, s.pull - half);
                    haptic(Haptic::Selection);
                } else if tall && !s.full && s.pull < -40. {
                    (s.full, s.pull) = (true, s.pull + half);
                    haptic(Haptic::Selection);
                } else if s.pull > 80. {
                    s.leaving = true;
                    s.settle = Some((now, s.pull, height));
                    return true;
                }
                s.settle = Some((now, s.pull, 0.));
            }
        }
        // It stretches a little past its top, no more.
        let floor = if tall && !s.full { -half } else { 0. };
        if s.pull < floor { s.pull = floor - (floor - s.pull).min(24.) * 0.5; }
        true
    })
}

/// The Taptic Engine's vocabulary (`UIFeedbackGenerator`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Haptic {
    /// Something landed or crossed a line: a snap, a threshold, a button.
    Light,
    /// Something was picked up: a long press.
    Medium,
    /// A value ticked over: a switch, a detent.
    Selection,
    /// About to destroy something.
    Warning,
}

/// A tap of the Taptic Engine; nothing off iOS.
pub fn haptic(kind: Haptic) {
    #[cfg(target_os = "ios")]
    gpui_ios::ios::haptic(match kind {
        Haptic::Light => gpui_ios::ios::Haptic::Light,
        Haptic::Medium => gpui_ios::ios::Haptic::Medium,
        Haptic::Selection => gpui_ios::ios::Haptic::Selection,
        Haptic::Warning => gpui_ios::ios::Haptic::Warning,
    });
    #[cfg(not(target_os = "ios"))]
    let _ = kind;
}

/// The system settings an iOS app follows: Dynamic Type, Bold Text, Reduce
/// Motion. Read from UIKit at most once a second (they change while the app
/// is in the background); everywhere else they are at their defaults.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct System { pub text_scale: f32, pub bold: bool, pub reduce_motion: bool }

pub fn system() -> System {
    #[cfg(target_os = "ios")]
    {
        thread_local! { static SEEN: std::cell::Cell<Option<(Instant, System)>> = const { std::cell::Cell::new(None) }; }
        if let Some((at, s)) = SEEN.get() && at.elapsed() < Duration::from_secs(1) { return s }
        // Dynamic Type's accessibility sizes go past 3x; a page of text stays usable up to 2x.
        let s = System { text_scale: gpui_ios::ios::text_scale().clamp(0.8, 2.), bold: gpui_ios::ios::bold_text_enabled(), reduce_motion: gpui_ios::ios::reduce_motion_enabled() };
        if SEEN.replace(Some((Instant::now(), s))).is_some_and(|(_, was)| was != s) { crate::themes::touch(); }
        s
    }
    #[cfg(not(target_os = "ios"))]
    System { text_scale: 1., bold: false, reduce_motion: false }
}

/// A text size at the reader's Dynamic Type size (whole points).
pub fn text(size: f32) -> f32 {
    let k = system().text_scale;
    if k == 1. { size } else { (size * k).round() }
}

/// Set by a shake of the device (and by devtools' `shake`): the note undoes its last edit.
pub static SHAKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by a tap on the status bar (and by devtools' `statustap`): the page shown scrolls to its top.
pub static TO_TOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Workspace {
    /// A tap on the status bar: the list or the document goes back to its top.
    pub(crate) fn scroll_to_top(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.view {
            crate::app::View::Doc => if let Some(e) = self.editor.clone() { e.update(cx, |e, cx| { e.list.scroll_to(ListOffset { item_ix: 0, offset_in_item: px(0.) }); cx.notify(); }) },
            crate::app::View::Folder if self.phone_pages(window) => {
                let scroll = self.phone_scroll_handle();
                let y = -f32::from(scroll.offset().y);
                // To the large title; from there, on to the search field.
                self.phone.snap = Some((Instant::now(), y, if y > SEARCH_H { SEARCH_H } else { 0. }));
            }
            _ => {}
        }
        cx.notify();
    }
}
