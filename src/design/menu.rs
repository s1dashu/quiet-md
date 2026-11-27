//! Menus and popovers (shadcn's `DropdownMenu` / `Popover`): the floating surface, its rows, labels
//! and separators.
//!
//! ```ignore
//! Popover::new("row-menu").menu().min_w(px(180.))
//!     .child(MenuItem::new("rename").icon("pen-line").label("重命名").on_click(…))
//!     .child(MenuSeparator::new())
//!     .child(MenuItem::new("delete").icon("bin").label("删除").destructive(true))
//! ```
//!
//! A menu's rows sit `PAD` in from every side; code that places a submenu beside a row adds up
//! `PAD`, `ROW_H` and `SEP_H` rather than measuring.

use super::base::{element, icon, label, under};
use super::{G, color, geo, shadow};
use gpui::prelude::FluentBuilder;
use gpui::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// A menu's padding (p-1), all round.
/// shadcn's `sideOffset`: a button's menu hangs this far below the button.
pub const SIDE_OFFSET: f32 = 4.;
/// The window margin a menu keeps (`snap_to_window_with_margin`), GPUI Kit's `WINDOW_MARGIN`.
pub const EDGE: f32 = 8.;

/// How a button's menu lines up with it: its left edge on the button's, or its right edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align { Start, End }

/// Where the menu of the button measured at `b` goes, as GPUI Kit's `Popup` and shadcn's DropdownMenu
/// place it: `SIDE_OFFSET` under the button, lined up by `align`; the point, and the corner of the menu
/// `anchored` puts on it. Only the button's bounds count, never the pointer, so a click anywhere on the
/// button opens the menu in one place and the menu never covers it. Below unless it doesn't fit there:
/// a menu `menu_h` tall goes above when it fits there, else to the side with more room; without its
/// height, above only when below is short (under 160) and above is longer. Whichever side, `room` caps
/// the menu to what is there and `Popover::fit` scrolls the rest.
pub fn hang(b: Bounds<Pixels>, align: Align, menu_h: Option<f32>) -> (Point<Pixels>, Anchor) {
    let vh = viewport();
    let x = if align == Align::Start { b.left() } else { b.right() };
    let below = f32::from(vh - b.bottom()) - SIDE_OFFSET - EDGE;
    let above = f32::from(b.top()) - SIDE_OFFSET - EDGE;
    let up = match menu_h { Some(h) => h > below && (h <= above || above > below), None => below < 160. && above > below };
    let corner = match (up, align) { (false, Align::Start) => Anchor::TopLeft, (false, Align::End) => Anchor::TopRight, (true, Align::Start) => Anchor::BottomLeft, (true, Align::End) => Anchor::BottomRight };
    (point(x, if up { b.top() - px(SIDE_OFFSET) } else { b.bottom() + px(SIDE_OFFSET) }), corner)
}

/// The most a menu `anchored` at `pos` by `corner` may be tall in the window, `EDGE` short of
/// the window's edge (for `Popover::fit`).
pub fn room(pos: Point<Pixels>, corner: Anchor) -> f32 {
    let vh = viewport();
    match corner {
        Anchor::BottomLeft | Anchor::BottomRight | Anchor::BottomCenter => f32::from(pos.y) - EDGE,
        _ => f32::from(vh - pos.y) - EDGE,
    }.max(ROW_H_MIN)
}
thread_local! {
    /// The window's height, as last laid out (`set_viewport`): `hang` and `room` are asked where no
    /// window is at hand (a click handler, a state change).
    static VIEWPORT: Cell<f32> = const { Cell::new(f32::MAX) };
}
/// Notes the window's height for `hang` and `room`: the window's root calls it as it renders.
pub fn set_viewport(window: &Window) { VIEWPORT.with(|v| v.set(f32::from(window.viewport_size().height))); }
fn viewport() -> Pixels { px(VIEWPORT.with(|v| v.get())) }
/// A capped menu still shows a row or two.
const ROW_H_MIN: f32 = 64.;

pub const PAD: f32 = G.menu_pad;
/// A row's height.
pub const ROW_H: f32 = G.menu_row_h;
/// A separator with its margin (my-1).
pub const SEP_H: f32 = SEP_W + 2. * SEP_MY;
const SEP_MY: f32 = 4.;
/// The rule's thickness: a 0.5pt hairline (maintainer 2026-11-24; shadcn's is 1px).
const SEP_W: f32 = 0.5;
/// A label row's height (py-1.5 over text-xs).
pub const LABEL_H: f32 = 28.;
/// A menu's least width (`min-w-32`).
pub const MIN_W: f32 = 128.;

/// The floating surface: `popover` on affectUI's edge (`shadow::popover`), `rounded-md`, 14pt text.
/// It has an opaque ground, which the shadow-drawn edge needs. `menu()` makes it a column of rows
/// (`DropdownMenuContent`: p-1, at least 128 wide).
#[derive(IntoElement)]
pub struct Popover { id: ElementId, base: Stateful<Div>, menu: bool, padded: bool, fit: Option<f32>, handle: Option<ScrollHandle>, inset: Option<f32>, sub: bool }
element!(Popover, parent);

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self { let id = id.into(); Popover { base: div().id(id.clone()), id, menu: false, padded: false, fit: None, handle: None, inset: None, sub: false } }
    /// A column of menu rows, `PAD` above and below them (rows keep `PAD` from the sides), `min-w-32`.
    pub fn menu(mut self) -> Self { self.menu = true; self }
    /// A popover holding a form: padded 16 (p-4), a column with 16 between its fields (gap-4).
    pub fn padded(mut self) -> Self { self.padded = true; self }
    /// At most `max_h` tall (`room`), the rest scrolling under the shared scrollbar (`scroll::ScrollBar`).
    pub fn fit(mut self, max_h: f32) -> Self { self.fit = Some(max_h); self }
    /// `fit` with the caller's handle, for a caller that needs how far it scrolled (a submenu levelled
    /// with its row).
    pub fn scroll(mut self, handle: &ScrollHandle) -> Self { self.handle = Some(handle.clone()); self }
    /// How far in from the right edge a `fit` popover's content keeps of itself, for the room it
    /// makes the scrollbar (`scroll::ScrollBar::room`): a padded one's padding, else a row's `PAD`.
    pub fn inset(mut self, inset: f32) -> Self { self.inset = Some(inset); self }
    /// The open submenu of a row (`DropdownMenuSubContent`): where it is is the far end of the
    /// pointer's grace area (`submenu_area`).
    pub fn submenu(mut self) -> Self { self.sub = true; self }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (g, menu, padded) = (geo(), self.menu, self.padded);
        set_viewport(window);
        let fit = self.fit.map(|h| {
            let id = self.id.clone();
            let st = window.use_keyed_state((self.id.clone(), "fit"), cx, move |_, _| (ScrollHandle::new(), Rc::new(super::scroll::ScrollBar::rounded(g.menu_r).remembered(&id))));
            let (own, bar) = st.read(cx).clone();
            (h, (self.handle.clone().unwrap_or(own), bar))
        });
        let inset = self.inset.unwrap_or(if padded { g.popover_pad } else { PAD });
        // the scrollbar's lane goes on the outside: a width the caller set grows by it, the content keeps its own
        let room = fit.as_ref().map_or(0., |(_, (_, bar))| bar.room(inset));
        let mut base = self.base;
        let own_right = super::scroll::widen(base.style(), room);
        let d = under(base, |d| {
            d.occlude().bg(color::popover()).rounded(px(g.menu_r)).shadow(shadow::popover()).font_family(crate::theme::ui_family())
                .text_size(px(g.text)).line_height(px(super::text::SM_LINE)).text_color(color::foreground())
                .when(menu, |d| d.min_w(px(MIN_W)).py(px(PAD)).flex().flex_col())
                .when(padded, |d| d.p(px(g.popover_pad)).flex().flex_col().gap(px(g.popover_gap)))
                .when(own_right, |d| d.pr(px(if padded { g.popover_pad } else { 0. } + room)))
                .when_some(fit, |d, (h, (handle, bar))| d.max_h(px(h)).overflow_y_scroll().track_scroll(&handle).child(bar.render(&handle)))
                .when(self.sub, |d| d.child(submenu_area()))
        });
        MenuScope { id: self.id, child: d.into_any_element() }
    }
}

// ---- one lit row per menu ----
//
// Radix's rule: a menu highlights at most one row, the pointer's or the keyboard's, whichever moved
// last; when the pointer leaves the rows, none (the keyboard's row comes back only when the keyboard
// moves). A row whose submenu is open (`MenuItem::open`, shadcn's `data-open:bg-accent`) is lit
// whatever the pointer does; Radix closes the submenu when the pointer reaches a sibling, and the
// callers do too. A popover is one menu (`MenuScope`): its rows (those not inside a nearer popover)
// check in while it prepaints, it picks the lit one before it paints, and each row paints its fill
// only if it is that one. A lit row's text is `accent-foreground`: the rows read the last pick while
// they lay out, and a changed pick draws one more frame. A submenu is a popover of its own. A row
// outside any popover (a list in a dialog) keeps the plain `hover` fill.

/// A menu's rows this frame and what was lit, kept across frames under the popover's id.
#[derive(Default)]
struct Rows {
    /// (id, hitbox, `selected`) for each row, in order.
    now: Vec<(ElementId, Hitbox, bool)>,
    lit: Option<ElementId>,
    /// The pointer has the say (else the keyboard: the `selected` row).
    pointer: bool,
    /// The pick has changed since the rows laid out (their text colour is a frame behind).
    stale: bool,
    /// The pointer moved over a row while the keyboard had the say.
    moved: bool,
    last_hover: Option<ElementId>,
    last_sel: Option<ElementId>,
}
type Shared = Rc<RefCell<Rows>>;

thread_local! {
    /// Popovers being laid out around the current element (rows inside one leave `hover` to it).
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// The menu whose rows are prepainting or painting.
    static SCOPE: RefCell<Option<Shared>> = const { RefCell::new(None) };
    /// The menu whose rows are laying out.
    static LAYOUT: RefCell<Option<Shared>> = const { RefCell::new(None) };
}

fn in_scope<R>(rows: Shared, f: impl FnOnce() -> R) -> R {
    let prev = SCOPE.with(|s| s.replace(Some(rows)));
    let r = f();
    SCOPE.with(|s| *s.borrow_mut() = prev);
    r
}
fn scope() -> Option<Shared> { SCOPE.with(|s| s.borrow().clone()) }

struct MenuScope { id: ElementId, child: AnyElement }
impl Element for MenuScope {
    type RequestLayoutState = ();
    type PrepaintState = bool;
    fn id(&self) -> Option<ElementId> { Some(self.id.clone()) }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> { None }
    fn request_layout(&mut self, id: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let rows = window.with_element_state(id.unwrap(), |s: Option<Shared>, _| { let s = s.unwrap_or_default(); (s.clone(), s) });
        rows.borrow_mut().stale = false;
        DEPTH.with(|d| d.set(d.get() + 1));
        let prev = LAYOUT.with(|s| s.replace(Some(rows)));
        let l = super::base::in_raised(|| self.child.request_layout(window, cx));
        LAYOUT.with(|s| *s.borrow_mut() = prev);
        DEPTH.with(|d| d.set(d.get() - 1));
        (l, ())
    }
    /// Whether a scrollbar inside found its room a frame behind (`scroll::unsettled`): this frame isn't painted.
    fn prepaint(&mut self, id: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> bool {
        let rows = window.with_element_state(id.unwrap(), |s: Option<Shared>, _| { let s = s.unwrap_or_default(); (s.clone(), s) });
        rows.borrow_mut().now.clear();
        super::scroll::unsettled(|| { in_scope(rows, || self.child.prepaint(window, cx)); })
    }
    fn paint(&mut self, id: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), unsettled: &mut bool, window: &mut Window, cx: &mut App) {
        if *unsettled { return; }
        let rows = window.with_element_state(id.unwrap(), |s: Option<Shared>, _| { let s = s.unwrap_or_default(); (s.clone(), s) });
        {
            let r = &mut *rows.borrow_mut();
            // on the way to an open submenu the rows it crosses don't light (Radix's pointer grace)
            let hover = if holding() { None } else { r.now.iter().find(|x| x.1.is_hovered(window)).map(|x| x.0.clone()) };
            let sel = r.now.iter().find(|x| x.2).map(|x| x.0.clone());
            // the pointer took a row, left them all, or moved on one: it has the say
            if hover != r.last_hover || r.moved { r.pointer = true; }
            // the keyboard moved the selection: it has
            if sel != r.last_sel && sel.is_some() { r.pointer = false; }
            let lit = if r.pointer { hover.clone() } else { sel.clone() };
            // the rows' text took the old pick: once more
            if lit != r.lit && !r.stale { r.stale = true; window.request_animation_frame(); }
            r.lit = lit;
            (r.last_hover, r.last_sel, r.moved) = (hover, sel, false);
        }
        in_scope(rows, || self.child.paint(window, cx));
    }
}

// ---- the pointer's grace on the way to a submenu ----
//
// Radix's `DropdownMenu.Sub` (@radix-ui/react-menu: `MenuSubTrigger`'s `onPointerLeave`,
// `isPointerMovingToSubmenu`, `isPointerInGraceArea`): when the pointer leaves the row of an open
// submenu, the point it left by (5 back towards the row) and the submenu's four corners make a
// polygon. While the pointer moves inside it, heading towards the submenu's side, the rows it
// crosses neither light nor take over: the submenu stays. Leaving the polygon, turning back, or
// `GRACE_MS` after it left the row, the row it is on has its way. A click or a key ends it at once,
// so the keyboard never waits. The rows' handlers that open or shut a submenu go through
// `sub_hover` / `sub_move`; the submenu marks where it is with `Popover::submenu` (or
// `submenu_area`); `MenuItem`'s `submenu` and `open` mark its row.

/// Radix's `pointerGraceTimer`.
pub const GRACE_MS: u64 = 300;
/// How far back towards the row the polygon's tip sits (Radix's bleed).
const BLEED: f32 = 5.;

struct Grace {
    area: [Point<Pixels>; 5],
    /// The submenu is on the right.
    right: bool,
    turn: u64,
    /// What the row the pointer reached would have done, kept for when the grace ends.
    pending: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

thread_local! {
    static GRACE: RefCell<Option<Grace>> = const { RefCell::new(None) };
    static GEN: Cell<u64> = const { Cell::new(0) };
    /// The open submenu as last painted.
    static SUB: Cell<Option<Bounds<Pixels>>> = const { Cell::new(None) };
    /// The pointer's last point, and whether it last moved right (Radix's `pointerDirRef`).
    static LAST: Cell<Option<Point<Pixels>>> = const { Cell::new(None) };
    static DIR_RIGHT: Cell<Option<bool>> = const { Cell::new(None) };
}

/// The polygon the pointer may cross on its way to `sub` from `exit`, where it left the row:
/// Radix's area (the tip `BLEED` back towards the row, then the submenu's near and far corners).
pub fn grace_area(exit: Point<Pixels>, sub: Bounds<Pixels>, right: bool) -> [Point<Pixels>; 5] {
    let bleed = if right { -BLEED } else { BLEED };
    let (near, far) = if right { (sub.left(), sub.right()) } else { (sub.right(), sub.left()) };
    [point(exit.x + px(bleed), exit.y), point(near, sub.top()), point(far, sub.top()), point(far, sub.bottom()), point(near, sub.bottom())]
}

/// Radix's `isPointInPolygon` (ray casting).
pub fn in_polygon(p: Point<Pixels>, poly: &[Point<Pixels>]) -> bool {
    let (x, y) = (f32::from(p.x), f32::from(p.y));
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi, xj, yj) = (f32::from(poly[i].x), f32::from(poly[i].y), f32::from(poly[j].x), f32::from(poly[j].y));
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi { inside = !inside; }
        j = i;
    }
    inside
}

/// The pointer is on its way to a submenu: the rows it crosses wait.
fn holding() -> bool { GRACE.with(|g| g.borrow().is_some()) }

/// Ends the grace; with `run`, the row the pointer reached has its way now.
fn end_grace(run: bool, window: &mut Window, cx: &mut App) {
    let Some(g) = GRACE.with(|g| g.borrow_mut().take()) else { return };
    if let Some(f) = g.pending.filter(|_| run) { f(window, cx); }
    window.refresh();
}

/// Marks where the open submenu is, for its row's grace area (`Popover::submenu` adds it).
pub fn submenu_area() -> impl IntoElement {
    canvas(|b, _, _| SUB.with(|s| s.set(Some(b))), |_, _, _, _| {}).absolute().inset_0()
}

/// Follows the pointer for the open submenu's row painted at `row`.
fn watch_grace(row: Bounds<Pixels>, window: &mut Window) {
    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture { return }
        let (p, last) = (ev.position, LAST.with(|l| l.replace(Some(ev.position))));
        if let Some(l) = last.filter(|l| l.x != p.x) { DIR_RIGHT.with(|d| d.set(Some(p.x > l.x))); }
        let sub = SUB.with(|s| s.get());
        // back on the row, or there: no grace
        if row.contains(&p) || sub.is_some_and(|s| s.contains(&p)) { end_grace(false, window, cx); return }
        let Some(sub) = sub else { return };
        let heading = |right: bool| DIR_RIGHT.with(|d| d.get()) == Some(right);
        let held = GRACE.with(|g| g.borrow().as_ref().map(|g| in_polygon(p, &g.area) && heading(g.right)));
        match held {
            Some(true) => {}
            Some(false) => end_grace(true, window, cx),
            // just off the row: a grace while it heads for the submenu
            None if last.is_some_and(|l| row.contains(&l)) => {
                let right = sub.center().x > row.center().x;
                let area = grace_area(p, sub, right);
                if !(in_polygon(p, &area) && heading(right)) { return }
                let turn = GEN.with(|g| { g.set(g.get() + 1); g.get() });
                GRACE.with(|g| *g.borrow_mut() = Some(Grace { area, right, turn, pending: None }));
                let w = window.window_handle();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(GRACE_MS)).await;
                    let _ = w.update(cx, |_, window, cx| if GRACE.with(|g| g.borrow().as_ref().is_some_and(|g| g.turn == turn)) { end_grace(true, window, cx) });
                }).detach();
            }
            None => {}
        }
    });
    // a click or a key: no waiting
    window.on_mouse_event(|_: &MouseDownEvent, phase, _, _| if phase == DispatchPhase::Capture { GRACE.with(|g| g.borrow_mut().take()); });
    window.on_key_event(|_: &KeyDownEvent, phase, _, _| if phase == DispatchPhase::Capture { GRACE.with(|g| g.borrow_mut().take()); });
}

/// Keeps `f` for when the grace ends if the pointer is on its way to a submenu; false if it isn't.
fn hold(f: Rc<dyn Fn(&mut Window, &mut App)>) -> bool {
    GRACE.with(|g| g.borrow_mut().as_mut().map(|g| g.pending = Some(f)).is_some())
}

/// An `on_hover` handler of a menu's row that opens or shuts a submenu: on the way to an open
/// submenu it waits for the grace to end (Radix's pointer grace).
pub fn sub_hover(f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    let f = Rc::new(f);
    move |on, window, cx| {
        let g = f.clone();
        if *on && hold(Rc::new(move |window, cx| g(&true, window, cx))) { return }
        f(on, window, cx)
    }
}

/// `sub_hover` for an `on_mouse_move` handler.
pub fn sub_move(f: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static) -> impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static {
    let f = Rc::new(f);
    move |ev, window, cx| {
        let (g, e) = (f.clone(), ev.clone());
        if hold(Rc::new(move |window, cx| g(&e, window, cx))) { return }
        f(ev, window, cx)
    }
}

/// A row's fill under a menu's rule: checks in while prepainting (a disabled row too: the pointer on it
/// lights none), paints `fill` if lit, and redraws
/// when the pointer comes or goes (or moves on it while the keyboard has the say).
fn lit_fill(id: ElementId, selected: bool, open: bool, sub: bool, off: bool, fill: Hsla, r: f32) -> impl IntoElement {
    canvas(move |bounds, window, _| {
        // the open submenu's row: its submenu records itself afresh after it (`submenu_area`)
        if open && sub { SUB.with(|s| s.set(None)); }
        let rows = scope()?;
        let hit = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        rows.borrow_mut().now.push((id.clone(), hit.clone(), selected));
        Some((rows, hit, id))
    }, move |bounds, state, window, _| {
        let Some((rows, hit, id)) = state else { return };
        if !off && (open || rows.borrow().lit.as_ref() == Some(&id)) { window.paint_quad(gpui::fill(bounds, fill).corner_radii(px(r))); }
        if open && sub { watch_grace(bounds, window); }
        let was = hit.is_hovered(window);
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, _| {
            if phase != DispatchPhase::Capture { return }
            let now = hit.is_hovered(window);
            if now != was { window.refresh(); }
            else if now && !rows.borrow().pointer { rows.borrow_mut().moved = true; window.refresh(); }
        });
    }).absolute().inset_0()
}

/// A menu row (`DropdownMenuItem`: px-2 py-1.5, gap-2, `rounded-sm`; lit, `accent` under
/// `accent-foreground`): an optional glyph in the text's colour, the label, then whatever the caller
/// adds, then the trailing mark (a shortcut, a check, a submenu's chevron). Disabled, the whole row
/// at 50%.
#[derive(IntoElement)]
pub struct MenuItem {
    base: Stateful<Div>,
    icon: Option<SharedString>,
    editor_icon: bool,
    leading: Option<AnyElement>,
    label: Option<SharedString>,
    description: Option<SharedString>,
    extra: Vec<AnyElement>,
    shortcut: Option<SharedString>,
    checked: Option<bool>,
    switch: Option<bool>,
    submenu: bool,
    inset: bool,
    selected: bool,
    open: bool,
    destructive: bool,
    disabled: bool,
}
element!(MenuItem);
impl ParentElement for MenuItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) { self.extra.extend(elements) }
}

impl MenuItem {
    pub fn new(id: impl Into<ElementId>) -> Self {
        MenuItem { base: div().id(id), icon: None, editor_icon: false, leading: None, label: None, description: None, extra: vec![], shortcut: None, checked: None, switch: None,
            submenu: false, inset: false, selected: false, open: false, destructive: false, disabled: false }
    }
    pub fn label(mut self, s: impl Into<SharedString>) -> Self { self.label = Some(s.into()); self }
    /// A 16pt glyph before the label, in the text's colour.
    pub fn icon(mut self, name: impl Into<SharedString>) -> Self { let n = name.into(); if !n.is_empty() { self.icon = Some(n) } self }
    /// The editor's glyph of that name (its set draws the same name otherwise), in the text's colour.
    pub fn editor_icon(mut self, name: impl Into<SharedString>) -> Self { self.editor_icon = true; self.icon(name) }
    /// Anything else before the label (an illustration, a swatch).
    pub fn leading(mut self, el: impl IntoElement) -> Self { self.leading = Some(el.into_any_element()); self }
    /// A second, smaller line under the label (`muted-foreground`, text-xs); the row grows to fit.
    pub fn description(mut self, s: impl Into<SharedString>) -> Self { self.description = Some(s.into()); self }
    /// A key hint at the end (`DropdownMenuShortcut`).
    pub fn shortcut(mut self, s: impl Into<SharedString>) -> Self { self.shortcut = Some(s.into()); self }
    /// One of a set of choices: a check at the end when on; when off, its room kept.
    pub fn checked(mut self, on: bool) -> Self { self.checked = Some(on); self }
    /// A setting the row turns on and off: a `Switch` at the end, sliding when it flips. A switch row
    /// keeps its menu open (Radix's CheckboxItem with `onSelect` prevented): its click toggles and
    /// nothing else, so callers must not close the menu from it; Esc and a click outside still do.
    pub fn switch(mut self, on: bool) -> Self { self.switch = Some(on); self }
    /// Opens a submenu: a chevron at the end.
    pub fn submenu(mut self, on: bool) -> Self { self.submenu = on; self }
    /// Keeps a glyph's room before the label though there is none (`data-inset`).
    pub fn inset(mut self) -> Self { self.inset = true; self }
    /// The keyboard's row: lit until the pointer moves onto the rows or leaves them.
    pub fn selected(mut self, on: bool) -> Self { self.selected = on; self }
    /// Its submenu is open (`DropdownMenuSubTrigger`'s `data-open`): lit whatever the pointer does.
    pub fn open(mut self, on: bool) -> Self { self.open = on; self }
    pub fn destructive(mut self, on: bool) -> Self { self.destructive = on; self }
    pub fn disabled(mut self, on: bool) -> Self { self.disabled = on; self }
}

impl RenderOnce for MenuItem {
    fn render(mut self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let g = geo();
        let (danger, off, sel, open) = (self.destructive, self.disabled, self.selected, self.open);
        // inside a popover the menu decides which one row is lit; elsewhere the pointer and `selected`
        let scoped = DEPTH.with(|d| d.get()) > 0;
        let id = self.base.interactivity().element_id.clone();
        let lit = !off && (open || scoped && id.as_ref().is_some_and(|id| LAYOUT.with(|s| s.borrow().as_ref().is_some_and(|r| r.borrow().lit.as_ref() == Some(id)))));
        // `focus:**:text-accent-foreground`: the label, its glyph, the shortcut, the caption
        let fg = if danger { color::destructive() } else if lit { color::accent_foreground() } else { color::foreground() };
        let caption = if lit && !danger { fg } else { color::muted_foreground() };
        let fill = if danger { color::destructive_soft() } else { color::accent() };
        let two_lines = self.description.is_some();
        let MenuItem { base, icon: ic, editor_icon, leading, label: lab, description, extra, shortcut, checked, switch, submenu, inset, .. } = self;
        let key = id.clone();
        let probe = lab.clone().and_then(crate::devtools::row_probe);
        under(base, |d| {
            d.relative().children(probe).flex_none().when(!two_lines, |d| d.h(px(g.menu_row_h))).when(two_lines, |d| d.min_h(px(g.menu_row_h)).py(px(6.)))
                .mx(px(g.menu_pad)).px(px(g.menu_row_px)).flex().items_center().gap(px(g.menu_gap)).rounded(px(g.menu_row_r))
                .text_size(px(g.text)).line_height(px(super::text::SM_LINE)).text_color(fg)
                .when(off, |d| d.opacity(0.5)).when(!off, |d| d.cursor_pointer())
                .when_some(id.filter(|_| scoped), |d, id| d.child(lit_fill(id, sel, open, submenu, off, fill, g.menu_row_r)))
                .when(!scoped && !off, |d| d.hover(move |s| s.bg(fill)).when(sel || open, |d| d.bg(fill)))
                .when_some(ic, |d, n| d.child(if editor_icon { crate::editor_ui::icon(&n, 16., fg) } else { icon(&n, 16., fg) }))
                .when_some(leading, |d, el| d.child(el))
                .when(inset, |d| d.child(div().flex_none().w(px(16.))))
                .when_some(lab, |d, s| {
                    let text = div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label(s));
                    match description {
                        Some(desc) => d.child(div().flex_1().min_w_0().flex().flex_col().child(text)
                            .child(div().text_size(px(g.text_xs)).line_height(px(super::text::XS_LINE)).text_color(caption).line_clamp(2).child(desc))),
                        None => d.child(text),
                    }
                })
                .children(extra)
                .when_some(shortcut, |d, s| d.child(shortcut_text(s, caption)))
                .when_some(checked, |d, on| d.child(div().flex_none().w(px(16.)).flex().justify_center().when(on, |d| d.child(icon("check", 16., fg)))))
                .when(submenu, |d| d.child(icon("chevron-right", 16., fg)))
                .when_some(switch, |d, on| d.child(match key { Some(k) => super::Switch::new(on).keyed(k), None => super::Switch::new(on) }))
        })
    }
}

/// `DropdownMenuShortcut`: text-xs, `tracking-widest` (0.1em after each glyph; GPUI has no letter
/// spacing, so the glyphs sit apart by it).
fn shortcut_text(s: SharedString, c: Hsla) -> Div {
    let xs = geo().text_xs;
    div().flex_none().ml_auto().flex().gap(px(xs * 0.1)).pr(px(xs * 0.1)).text_size(px(xs)).line_height(px(super::text::XS_LINE)).text_color(c)
        .children(s.chars().map(|ch| div().child(SharedString::from(ch.to_string()))))
}

/// A small heading over a group of rows (`DropdownMenuLabel`: px-2 py-1.5, text-xs, `muted-foreground`;
/// font-medium wants a 500 weight, see DESIGN_SYSTEM.md).
#[derive(IntoElement)]
pub struct MenuLabel { base: Stateful<Div>, text: SharedString }
element!(MenuLabel);
impl MenuLabel {
    pub fn new(text: impl Into<SharedString>) -> Self { let text = text.into(); MenuLabel { base: div().id(ElementId::Name(text.clone())), text } }
}
impl RenderOnce for MenuLabel {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let g = geo();
        let t = self.text;
        under(self.base, |d| d.flex_none().h(px(LABEL_H)).px(px(g.menu_pad + g.menu_row_px)).flex().items_center()
            .text_size(px(g.text_xs)).text_color(color::muted_foreground()).whitespace_nowrap().child(label(t)))
    }
}

/// A rule between groups of rows (`DropdownMenuSeparator`'s `border`, my-1). Maintainer's change
/// 2026-11-24: a 0.5pt hairline held in to the rows' content (their icon's and label's edge)
/// rather than shadcn's 1px edge to edge.
#[derive(IntoElement, Default)]
pub struct MenuSeparator;
impl MenuSeparator { pub fn new() -> Self { MenuSeparator } }
impl RenderOnce for MenuSeparator {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().flex_none().h(px(SEP_W)).my(px(SEP_MY)).mx(px(geo().rule_inset)).bg(color::border())
    }
}

#[cfg(test)]
mod tests {
    use super::{grace_area, in_polygon};
    use gpui::{Bounds, Pixels, point, px, size};

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> { Bounds::new(point(px(x), px(y)), size(px(w), px(h))) }
    fn inside(exit: (f32, f32), sub: Bounds<Pixels>, right: bool, p: (f32, f32)) -> bool {
        in_polygon(point(px(p.0), px(p.1)), &grace_area(point(px(exit.0), px(exit.1)), sub, right))
    }

    #[test]
    fn grace_area_on_the_right() {
        // a row 200..400 wide at y 100..132, left by its right edge at y 116; the submenu at 404..604, 96..296
        let sub = b(404., 96., 200., 200.);
        assert!(inside((400., 116.), sub, true, (402., 116.)));
        assert!(inside((400., 116.), sub, true, (402., 130.)));
        assert!(inside((400., 116.), sub, true, (403., 270.)), "down towards the submenu's bottom corner");
        assert!(inside((400., 116.), sub, true, (396., 116.)), "the bleed behind the exit");
        assert!(!inside((400., 116.), sub, true, (390., 116.)), "back over the row");
        assert!(!inside((400., 116.), sub, true, (380., 160.)), "back under the row");
        assert!(!inside((400., 116.), sub, true, (402., 60.)), "above the submenu");
    }

    #[test]
    fn grace_area_on_the_left() {
        // the 页宽 case: a row 600..800 at y 50..82, left by its top-left going up-left; the submenu at 396..596, 46..170
        let sub = b(396., 46., 200., 124.);
        assert!(inside((610., 52.), sub, false, (598., 50.)));
        assert!(inside((610., 52.), sub, false, (600., 80.)), "down-left towards the submenu's bottom corner");
        assert!(inside((610., 52.), sub, false, (613., 52.)), "the bleed behind the exit");
        assert!(!inside((610., 52.), sub, false, (620., 52.)), "back over the row");
        assert!(!inside((700., 82.), sub, false, (700., 110.)), "straight down to 阅读模式");
        assert!(!inside((610., 52.), sub, false, (598., 20.)), "above the submenu");
        assert!(!inside((610., 52.), sub, false, (598., 180.)), "below the submenu");
    }
}
