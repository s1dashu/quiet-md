//! Touch editing (docs/design/mobile.md 3.1): handles on both ends of a
//! selection, the edit menu over it, and a tap that shows a block's handle
//! where a mouse would hover. All drawn by GPUI: the system edit menu
//! (UIEditMenuInteraction) was out of reach (#377).
//!
//! A finger on a block's handle drags the block as the mouse does (the same
//! drop line and edge scrolling); let go without moving, it opens the menu.
//!
//! Touch mode starts with a touch (a long press, a touch drag) or on iOS, and
//! ends when the mouse moves. Out of it nothing here draws or changes, so the
//! desktop stays as it is.

use super::core::{self, Pos, Selection};
use super::keymap as k;
use super::overlays::at;
use super::view::{BlockSub, Editor, Menu};
use crate::design::toolbar::TOOLBAR_H;
use crate::design::{Button, ButtonSize, Toolbar};
use crate::feishu_theme::Theme;
use crate::model::BlockId;
use gpui::*;

/// A handle's bar and the dot on its outer end.
const BAR_W: f32 = 2.;
const DOT: f32 = 10.;
/// The handle takes a finger this far around its bar (44pt targets, Apple HIG).
const GRAB: f32 = 22.;
/// The edit menu floats this far above what it points at.
const MENU_GAP: f32 = 8.;
/// A block handle (42 × 26) takes a finger this much further round it.
const GRAB_BLOCK: f32 = 9.;

pub struct Touch {
    pub on: bool,
    /// The end of the selection a finger is dragging, and how far the finger
    /// sits from that end's line (so the text under the dot isn't what counts).
    pub(super) grab: Option<(End, Point<Pixels>)>,
    /// The block whose handle a finger holds (dragging it, or a tap if it never moves).
    pub(super) block: Option<BlockId>,
    /// The edit menu shows: after a long press, a handle let go, a tap on the caret.
    pub menu: bool,
    /// The block a tap showed the handle of.
    pub handle: Option<BlockId>,
    /// On for good, the mouse notwithstanding: scripts (`FDL_TOUCH=1`, devtools
    /// builds) drive touch mode with mouse clicks.
    pub pinned: bool,
    /// What the last frame showed (focus, handle block, text drag, typing), for
    /// the diagnostic log: only changes are written.
    seen: Option<(bool, Option<BlockId>, bool, bool)>,
}

impl Default for Touch {
    fn default() -> Self {
        let pinned = cfg!(feature = "devtools") && std::env::var_os("FDL_TOUCH").is_some();
        Touch { on: cfg!(target_os = "ios") || pinned, grab: None, block: None, menu: false, handle: None, pinned, seen: None }
    }
}

impl Touch {
    /// A touch came in. True when that turned touch mode on.
    pub fn touched(&mut self) -> bool { !std::mem::replace(&mut self.on, true) }
    /// The mouse moved: touch mode ends and takes its handles and menu with it.
    /// True when there was something to take.
    pub fn moused(&mut self) -> bool {
        if !self.on || self.pinned { return false }
        *self = Touch { on: false, grab: None, block: None, menu: false, handle: None, pinned: false, seen: None };
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End { Start, End }

/// The selection while `end` of it (`ordered`: start, end in document order)
/// is dragged: the other end stays put, the dragged one moves, as a
/// shift-click moves the head. Dragged past the other end, it just keeps going.
pub fn grab((s, e): (Pos, Pos), end: End) -> Selection {
    match end { End::Start => Selection { anchor: e, head: s }, End::End => Selection { anchor: s, head: e } }
}

/// What letting go of a held block handle does: drop the blocks where the
/// line shows, or (it never moved) open the handle's menu, as a click would;
/// a cancelled touch does neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release { Drop, Menu, Cancel }

pub fn release(phase: TouchPhase, started: bool) -> Release {
    match (phase, started) {
        (TouchPhase::Ended, true) => Release::Drop,
        (TouchPhase::Ended, false) => Release::Menu,
        _ => Release::Cancel,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item { Cut, Copy, Paste, Select, SelectAll }

/// What the edit menu offers: a caret has nothing to cut or copy, a
/// read-only page nothing to change.
pub fn items(read_only: bool, caret: bool) -> &'static [Item] {
    match (read_only, caret) {
        (true, true) => &[Item::SelectAll],
        (true, false) => &[Item::Copy, Item::SelectAll],
        (false, true) => &[Item::Paste, Item::Select, Item::SelectAll],
        (false, false) => &[Item::Cut, Item::Copy, Item::Paste, Item::SelectAll],
    }
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::Cut => crate::settings::text("剪切"),
            Item::Copy => crate::settings::text("复制"),
            Item::Paste => crate::settings::text("粘贴"),
            Item::Select => crate::settings::tr("选择", "Select"),
            Item::SelectAll => crate::settings::text("全选"),
        }
    }
}

/// A tap on a block menu row that opens its submenu `s`, which a mouse opens
/// by hovering. Out of touch mode it does nothing and lets the press through.
pub(super) fn tap_sub(cx: &Context<Editor>, s: BlockSub) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |e: &mut Editor, _: &MouseDownEvent, _, cx| {
        if !e.touch.on { return }
        cx.stop_propagation();
        if let Some(Menu::Block { sub, .. }) = &mut e.menu { *sub = Some(s); cx.notify() }
    })
}

impl Editor {
    /// A touch came in (any phase of a long press, a touch drag starting).
    pub(super) fn touched(&mut self, cx: &mut Context<Self>) {
        if self.touch.touched() { cx.notify() }
    }

    /// The selection's two handles: (which end, window bounds of its bar).
    fn sel_bars(&self) -> Option<[(End, Bounds<Pixels>); 2]> {
        if !self.touch.on || self.core.sel.is_collapsed() || self.core.cell_sel.is_some() || self.core.in_block_mode() { return None }
        let (s, e) = self.core.ordered();
        let (sp, slh) = self.point_of(s)?;
        let (ep, elh) = self.point_of(e)?;
        Some([(End::Start, Bounds::new(sp, size(px(BAR_W), slh))), (End::End, Bounds::new(ep, size(px(BAR_W), elh)))])
    }

    /// The handle a touch starting at `p` takes, if any.
    fn handle_under(&self, p: Point<Pixels>) -> Option<(End, Point<Pixels>)> {
        self.sel_bars()?.into_iter()
            .filter(|(_, b)| Bounds::from_corners(point(b.left() - px(GRAB), b.top() - px(GRAB)), point(b.right() + px(GRAB), b.bottom() + px(GRAB))).contains(&p))
            .min_by(|(_, a), (_, b)| (a.center() - p).magnitude().total_cmp(&(b.center() - p).magnitude()))
            .map(|(end, b)| (end, b.center() - p))
    }

    /// A finger on a selection handle: its end follows the finger (the page
    /// scrolls at its edges, as a text drag does), and letting go brings the menu.
    pub(super) fn on_touch_drag(&mut self, ev: &TouchDragEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.phase {
            TouchPhase::Started => {
                self.touched(cx);
                let under = self.handle_under(ev.start_position);
                let block = under.is_none().then(|| self.block_handle_under(ev.start_position)).flatten();
                log::info!("touch: drag starts {:?} on handle {:?}, block handle {:?}", ev.start_position, under.map(|(e, _)| e), block);
                if let Some(id) = block {
                    self.press_handle(id, ev.start_position);
                    // Columns and grids have a handle but don't move.
                    if self.block_drag.is_none() { return }
                    self.touch.block = Some(id);
                    self.touch.menu = false;
                    window.prevent_default();
                    cx.stop_propagation();
                    return cx.notify();
                }
                let Some((end, off)) = under else { return };
                self.core.sel = grab(self.core.ordered(), end);
                self.touch.grab = Some((end, off));
                self.touch.menu = false;
                self.dragging = true;
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
            }
            TouchPhase::Moved if self.touch.block.is_some() => self.block_drag_move(ev.position, true, window, cx),
            TouchPhase::Moved => if let Some((_, off)) = self.touch.grab { self.drag_to(ev.position + off, cx) },
            TouchPhase::Ended | TouchPhase::Cancelled if self.touch.block.is_some() => {
                let Some(id) = self.touch.block.take() else { return };
                let started = self.block_drag.as_ref().is_some_and(|d| d.started);
                log::info!("touch: block handle let go {:?}: {:?}", ev.phase, release(ev.phase, started));
                match release(ev.phase, started) {
                    Release::Drop => self.end_block_drag(Some(ev.position), window, cx),
                    Release::Menu => { self.block_drag = None; self.toggle_block_menu(id, false, window, cx) }
                    Release::Cancel => { self.block_drag = None; self.toolbar_hidden = false; cx.notify() }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if let Some((_, off)) = self.touch.grab.take() {
                    if ev.phase == TouchPhase::Ended { self.drag_to(ev.position + off, cx) }
                    self.end_drag(None, cx);
                    self.touch.menu = true;
                    cx.notify();
                }
            }
        }
    }

    /// The shown block handle a finger landing at `p` holds (not the one of an
    /// empty line, which only adds).
    fn block_handle_under(&self, p: Point<Pixels>) -> Option<BlockId> {
        let id = self.handle_block()?;
        if self.is_empty_line(id) { return None }
        self.handle_rect(id, false).is_some_and(|r| r.dilate(px(GRAB_BLOCK)).contains(&p)).then_some(id)
    }

    /// A tap coming down in touch mode, before anything can return early:
    /// the block under it (or right of it, a tap in the margin) shows its
    /// handle, as a pointer arriving there would; the pointer "moved", so a
    /// past edit no longer hides it; and the keyboard is asked for, from
    /// within the gesture as UIKit wants.
    pub(super) fn touch_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.touch.on { return }
        self.typing = false;
        self.touch.handle = self.hover_target(ev.position);
        if !self.read_only { window.request_virtual_keyboard(); }
        log::info!("touch: down {:?} x{} focused={:?} handle={:?} of {} targets", ev.position, ev.click_count, window.focused(cx), self.touch.handle, self.handle_targets.len());
    }

    /// The tap placed the caret or selected: a tap on the caret where it
    /// already was (`was`: the caret before), or a double or triple tap's word
    /// or line, brings the menu.
    pub(super) fn touch_tap(&mut self, was: Option<Pos>, taps: usize, window: &Window, cx: &App) {
        if !self.touch.on { return }
        let caret = self.core.sel.is_collapsed();
        self.touch.menu = if caret { was.is_some() && Some(self.core.sel.head) == was } else { taps >= 2 };
        log::info!("touch: placed {:?} menu={} focused={:?} dragging={}", self.core.sel, self.touch.menu, window.focused(cx), self.dragging);
    }

    /// The tap's release reached the editor.
    pub(super) fn touch_up(&mut self, ev: &MouseUpEvent, window: &Window, cx: &App) {
        if self.touch.on { log::info!("touch: up {:?} focused={:?}", ev.position, window.focused(cx)) }
    }

    /// Diagnostic log (iOS bring-up): each frame whose focus, shown handle,
    /// text drag or typing differs from the last one's, with who has focus.
    pub(super) fn touch_frame(&mut self, focused: bool, window: &Window, cx: &App) {
        if !self.touch.on { return }
        let now = (focused, self.handle_block(), self.dragging, self.typing);
        if self.touch.seen != Some(now) {
            self.touch.seen = Some(now);
            log::info!("touch: frame focused={} (window: {:?}) handle={:?} (tapped {:?}) dragging={} typing={}", now.0, window.focused(cx), now.1, self.touch.handle, now.2, now.3);
        }
    }

    /// 选择: the word at the caret, as a long press would.
    fn select_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let h = self.core.sel.head;
        let text = self.core.text_of(h.block).map(|t| t.text.clone()).unwrap_or_default();
        if text.is_empty() { return }
        let r = core::word_at(&text, h.offset);
        self.set_caret(Pos::new(h.block, r.start), false);
        self.core.sel.head = Pos::new(h.block, r.end);
        self.touch.menu = true;
        self.restart_blink(window, cx);
    }

    /// The handles and the edit menu, over the page (empty out of touch mode).
    pub(super) fn touch_layers(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = vec![];
        if !self.touch.on || !self.focus.is_focused(window) { return out }
        if let Some(bars) = self.sel_bars() {
            for (end, b) in bars {
                // The dot sits on the outer end: above the start, below the end.
                let dot = div().absolute().left(px((BAR_W - DOT) / 2.)).size(px(DOT)).rounded_full().bg(Theme::primary());
                let dot = if end == End::Start { dot.top(px(-DOT + 1.)) } else { dot.bottom(px(-DOT + 1.)) };
                let el = div().relative().w(px(BAR_W)).h(b.size.height).bg(Theme::primary()).child(dot);
                out.push(at(self.rel(b.origin), Anchor::TopLeft, el));
            }
        }
        out.extend(self.loupe(window));
        out.extend(self.touch_menu(window, cx));
        out
    }

    /// The loupe: while a finger drags a selection's end (a handle, or on from a long
    /// press) it covers the text it moves through, so the line shows again above it,
    /// larger, with the caret where the end is. GPUI cannot magnify what is drawn, so
    /// this is the line's plain text around the end rather than a lens over the page.
    fn loupe(&self, window: &Window) -> Option<AnyElement> {
        if self.touch.grab.is_none() && !self.long_press { return None }
        let head = self.core.sel.head;
        let (hp, lh) = self.point_of(head)?;
        let text = &self.core.text_of(head.block)?.text;
        if !text.is_char_boundary(head.offset.min(text.len())) { return None }
        let (before, after) = text.split_at(head.offset.min(text.len()));
        // A dozen characters each side, without the line breaks inside a block.
        let clean = |s: String| s.replace(['\n', '\u{2028}'], " ");
        let b: String = clean(before.chars().rev().take(14).collect::<Vec<_>>().into_iter().rev().collect());
        let a: String = clean(after.chars().take(14).collect());
        let (w, h) = (188., 44.);
        let size = (crate::editor_theme::body_size() * 1.2).round();
        let vw = f32::from(window.viewport_size().width);
        let x = (f32::from(hp.x) - w / 2.).clamp(8., (vw - w - 8.).max(8.));
        // Above the line, clear of the finger; under it when the line is at the top.
        let above = hp.y - px(h + 14.) >= self.list.viewport_bounds().top();
        let y = if above { hp.y - px(h + 14.) } else { hp.y + lh + px(28.) };
        let side = || div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().flex();
        let el = div().w(px(w)).h(px(h)).rounded_full().bg(crate::design::color::popover()).border_1().border_color(crate::design::color::border())
            .shadow(crate::design::shadow::popover()).overflow_hidden().flex().items_center().px(px(14.))
            .text_size(px(size)).text_color(Theme::text())
            .child(side().justify_end().child(crate::editor_ui::ui_text(b)))
            .child(div().flex_none().w(px(2.)).h(px(size + 6.)).mx(px(1.)).rounded_full().bg(Theme::primary()))
            .child(side().child(crate::editor_ui::ui_text(a)));
        Some(at(self.rel(point(px(x), y)), Anchor::TopLeft, el))
    }

    /// Whether the format toolbar shows this frame (`overlays::toolbar`'s own test).
    fn toolbar_shown(&self, window: &Window) -> bool {
        let menu_ok = matches!(self.menu, None | Some(Menu::TurnInto | Menu::Align | Menu::Color | Menu::TbMore));
        menu_ok && !self.read_only && self.core.cell_sel.is_none() && !self.toolbar_hidden && !self.dragging && !self.core.sel.is_collapsed()
            && !self.core.in_block_mode() && self.slash.is_none() && self.focus.is_focused(window) && self.core.ordered().0.block != core::TITLE
    }

    /// Cut / copy / paste / select all over the selection (or the caret),
    /// above the format toolbar when that shows too.
    fn touch_menu(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.touch.menu || self.touch.grab.is_some() || self.menu.is_some() || self.slash.is_some() { return None }
        let caret = self.core.sel.is_collapsed();
        let (s, e) = self.core.ordered();
        let (sp, _) = self.point_of(s)?;
        let (ep, elh) = self.point_of(e)?;
        let (hp, _) = self.point_of(self.core.sel.head)?;
        let view_top = self.list.viewport_bounds().top();
        // The toolbar takes the room above the selection when it fits there, else below it.
        let bar = self.toolbar_shown(window).then(|| sp.y - px(45.) >= view_top);
        let above_top = match bar { Some(true) => sp.y - px(45.), _ => sp.y } - px(MENU_GAP);
        let (y, corner) = if above_top - px(TOOLBAR_H) >= view_top { (above_top, Anchor::BottomCenter) }
            else { (ep.y + elh + px(MENU_GAP) + if bar == Some(false) { px(5. + TOOLBAR_H) } else { px(0.) }, Anchor::TopCenter) };
        let mut tb = Toolbar::new("touch-menu").on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        for &it in items(self.read_only, caret) {
            tb = tb.child(Button::ghost(("touch-menu", it as usize)).sized(ButtonSize::Sm).label(it.label()).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |e: &mut Editor, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    e.touch.menu = false;
                    match it {
                        Item::Cut => window.dispatch_action(Box::new(k::Cut), cx),
                        Item::Copy => window.dispatch_action(Box::new(k::Copy), cx),
                        Item::Paste => window.dispatch_action(Box::new(k::Paste), cx),
                        Item::SelectAll => { window.dispatch_action(Box::new(k::SelectAll), cx); e.touch.menu = true }
                        Item::Select => e.select_word(window, cx),
                    }
                    cx.notify();
                }),
            ));
        }
        // Centred on the selection's moving end; `at` keeps it in the window.
        Some(at(self.rel(point(hp.x, y)), corner, tb))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's `test` would shadow `#[test]`.
    use super::{End, Item, Release, Touch, grab, items, release};
    use gpui::TouchPhase;
    use super::core::{self, Pos, Selection};
    use crate::model::BlockId;

    fn p(b: u64, o: usize) -> Pos { Pos::new(BlockId(b), o) }

    #[test]
    fn touch_mode_starts_with_a_touch_and_ends_with_the_mouse() {
        let mut t = Touch { on: false, pinned: false, ..Touch::default() };
        assert!(!t.moused() && !t.on);
        assert!(t.touched() && t.on);
        assert!(!t.touched());
        t.menu = true;
        t.handle = Some(BlockId(3));
        assert!(t.moused());
        assert!(!t.on && !t.menu && t.handle.is_none() && t.grab.is_none());
        // Pinned on (scripts): the mouse doesn't end it.
        let mut t = Touch { on: true, pinned: true, ..Touch::default() };
        assert!(!t.moused() && t.on);
    }

    #[test]
    fn a_dragged_handle_moves_its_end_and_keeps_the_other() {
        // One block, 2..5: the end handle moves the head, the start stays.
        let s = grab((p(1, 2), p(1, 5)), End::End);
        assert_eq!((s.anchor, s.head), (p(1, 2), p(1, 5)));
        // The start handle: the end is the anchor now.
        let s = grab((p(1, 2), p(1, 5)), End::Start);
        assert_eq!((s.anchor, s.head), (p(1, 5), p(1, 2)));
        // Whichever way the selection was made.
        let mut c = core::Core::new(crate::docxml::parse("<title>t</title>\n<p>甲乙丙丁戊</p>\n<p>己庚辛</p>\n").unwrap());
        let ids: Vec<BlockId> = c.doc.blocks.iter().map(|b| b.id).collect();
        c.sel = Selection { anchor: Pos::new(ids[0], 9), head: Pos::new(ids[0], 3) };
        c.sel = grab(c.ordered(), End::End);
        assert_eq!((c.sel.anchor, c.sel.head), (Pos::new(ids[0], 3), Pos::new(ids[0], 9)));
        // Across blocks, then back past the other end: the anchor never moves.
        c.sel.head = Pos::new(ids[1], 6);
        assert_eq!(c.ordered(), (Pos::new(ids[0], 3), Pos::new(ids[1], 6)));
        c.sel = grab(c.ordered(), End::Start);
        assert_eq!(c.sel.anchor, Pos::new(ids[1], 6));
        c.sel.head = Pos::new(ids[1], 3);
        assert_eq!(c.ordered(), (Pos::new(ids[1], 3), Pos::new(ids[1], 6)));
        // The start handle dragged below the end: now it is the end that moves.
        c.sel = Selection { anchor: Pos::new(ids[0], 3), head: Pos::new(ids[0], 6) };
        c.sel = grab(c.ordered(), End::Start);
        c.sel.head = Pos::new(ids[1], 3);
        assert_eq!(c.ordered(), (Pos::new(ids[0], 6), Pos::new(ids[1], 3)));
    }

    #[test]
    fn the_menu_offers_what_applies() {
        assert_eq!(items(false, false), [Item::Cut, Item::Copy, Item::Paste, Item::SelectAll]);
        assert_eq!(items(false, true), [Item::Paste, Item::Select, Item::SelectAll]);
        assert_eq!(items(true, false), [Item::Copy, Item::SelectAll]);
        assert_eq!(items(true, true), [Item::SelectAll]);
    }

    #[test]
    fn a_held_block_handle_drops_once_moved_and_otherwise_opens_its_menu() {
        assert_eq!(release(TouchPhase::Ended, true), Release::Drop);
        assert_eq!(release(TouchPhase::Ended, false), Release::Menu);
        assert_eq!(release(TouchPhase::Cancelled, true), Release::Cancel);
        assert_eq!(release(TouchPhase::Cancelled, false), Release::Cancel);
    }
}
