//! Toasts (Sonner as shadcn ships it; Radix Toast's parts): a mark by tone, a title, an optional
//! description under it in `muted-foreground`, an optional action (「撤销」) and close button. Each
//! window keeps its own stack, the newest in front: three show, the ones behind peeking out under
//! the front one until the pointer comes onto the stack, which spreads them out and stops every
//! clock. A toast shows 4s (6s with an action, to leave time to reach it) and fades over its last
//! .3s. Esc closes the front one only while the keyboard is on the stack (Radix: "When focus is on a
//! Toast, Escape closes it"; Sonner's Toaster): a click on it takes the focus, and it goes back to
//! whoever had it once no toast is left. Esc elsewhere stays with the editor, the menus and so on.
//!
//! ```ignore
//! toast::show(window, Toast::new(ToastTone::Success, "已删除").action("撤销", |w, cx| …), cx);
//! toast::toaster(window, cx)   // in a window's root, over everything
//! ```

use super::base::icon;
use super::{Button, ButtonSize, color, geo, shadow};
use gpui::prelude::FluentBuilder;
use gpui::*;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use web_time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastTone { Success, Error, Info }

/// Sonner's `width`, `gap` and `visibleToasts`; how far each toast behind peeks out and how much
/// narrower it is (Sonner scales it by 5% a step).
const WIDTH: f32 = 356.;
const GAP: f32 = 14.;
const SHOWN: usize = 3;
const PEEK: f32 = 10.;
const NARROW: f32 = 9.;
/// The title and the description wrap, each to this many lines, then end in an ellipsis.
const LINES: usize = 3;
const LIFE: f32 = 4.;
const LIFE_ACTION: f32 = 6.;
const FADE: f32 = 0.3;
/// Below the window's top edge.
const TOP: f32 = 32.;

type Run = Rc<dyn Fn(&mut Window, &mut App)>;

/// What a toast says and offers.
#[derive(Clone, IntoElement)]
pub struct Toast { tone: ToastTone, title: SharedString, description: Option<SharedString>, action: Option<(SharedString, Run)>, close: bool }

impl Toast {
    pub fn new(tone: ToastTone, title: impl Into<SharedString>) -> Self { Toast { tone, title: title.into(), description: None, action: None, close: false } }
    /// A second line in `muted-foreground`: what the title leaves out.
    pub fn description(mut self, s: impl Into<SharedString>) -> Self { self.description = Some(s.into()); self }
    /// A button on the right (Sonner's `action`); clicking it also closes the toast.
    pub fn action(mut self, label: impl Into<SharedString>, run: impl Fn(&mut Window, &mut App) + 'static) -> Self { self.action = Some((label.into(), Rc::new(run))); self }
    /// A ✕ on the top-left corner (Sonner's `closeButton`).
    pub fn close_button(mut self) -> Self { self.close = true; self }
    fn same(&self, o: &Toast) -> bool { self.tone == o.tone && self.title == o.title && self.description == o.description && self.action.as_ref().map(|a| &a.0) == o.action.as_ref().map(|a| &a.0) }
    fn life(&self) -> f32 { if self.action.is_some() { LIFE_ACTION } else { LIFE } }

    /// The card. `close` closes it (the ✕, and after the action); `id` keeps its buttons apart.
    fn card(&self, id: u64, close: Option<Run>, window: &Window) -> Stateful<Div> {
        let g = geo();
        let mark = match self.tone { ToastTone::Success => "circle-check", ToastTone::Error => "octagon-x", ToastTone::Info => "info" };
        // the text's column: the card less its padding, the mark and the action's button
        let action_w = self.action.as_ref().map_or(0., |(name, _)| line_w(&crate::settings::label(name), g.text_xs, window) + 16. + 6.);
        let width = WIDTH - 2. * g.toast_px - 16. - 6. - action_w;
        let text = |s: &SharedString| div().min_w_0().child(clamped(&crate::settings::label(s), width, window));
        div().id(("toast", id as usize)).relative().w(px(WIDTH)).flex().items_center().gap(px(6.)).px(px(g.toast_px)).py(px(g.toast_py)).rounded(px(g.toast_r))
            .bg(color::popover()).shadow(shadow::toast())
            .font_family(crate::theme::ui_family()).text_size(px(13.)).line_height(px(super::text::SM_LINE)).text_color(color::foreground())
            .child(icon(mark, 16., color::foreground()).flex_none())
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.))
                .child(text(&self.title))
                .when_some(self.description.as_ref(), |d, s| d.child(text(s).text_color(color::muted_foreground()))))
            .when_some(self.action.clone(), |d, (name, run)| {
                let close = close.clone();
                d.child(Button::primary(("toast-action", id as usize)).sized(ButtonSize::Xs).label(crate::settings::label(&name))
                    .on_click(move |_, window, cx| { cx.stop_propagation(); run(window, cx); if let Some(c) = &close { c(window, cx) } }))
            })
            .when(self.close, |d| {
                let close = close.clone();
                d.child(div().id(("toast-close", id as usize)).absolute().top(px(-6.)).left(px(-6.)).size(px(20.)).rounded_full().flex().items_center().justify_center()
                    .bg(color::popover()).border_1().border_color(color::border()).cursor_pointer().hover(|d| d.bg(color::muted()))
                    .child(icon("x", 12., color::foreground()))
                    .on_click(move |_, window, cx| { cx.stop_propagation(); if let Some(c) = &close { c(window, cx) } }))
            })
    }
}

/// Drawn where it is put (the 组件库's samples): its action runs, its ✕ does nothing.
impl RenderOnce for Toast {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement { self.card(0, None, window) }
}

fn style(size: f32, window: &Window) -> TextStyle {
    let mut style = window.text_style();
    style.font_family = crate::theme::ui_family().into();
    style.font_size = px(size).into();
    style
}

fn line_w(s: &str, size: f32, window: &Window) -> f32 {
    let run = style(size, window).to_run(s.len());
    f32::from(window.text_system().shape_line(s.to_string().into(), px(size), &[run], None).width)
}

/// `s` as at most `LINES` lines of the toast's text `width` wide, ending in "…" when cut (GPUI's
/// `line_clamp` cuts at `LINES` widths, not at where the lines wrap).
fn clamped(s: &str, width: f32, window: &Window) -> SharedString {
    let lines = |s: &str| {
        let run = style(13., window).to_run(s.len());
        window.text_system().shape_text(s.to_string().into(), px(13.), &[run], Some(px(width)), None)
            .map(|ls| ls.iter().map(|l| l.wrap_boundaries().len() + 1).sum::<usize>()).unwrap_or(1)
    };
    if lines(s) <= LINES { return s.to_string().into(); }
    let ends: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
    let (mut lo, mut hi) = (0, ends.len());
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if lines(&format!("{}…", &s[..ends[mid]])) <= LINES { lo = mid } else { hi = mid }
    }
    format!("{}…", s[..ends[lo]].trim_end()).into()
}

/// A toast's clock: what has run, and since when it runs again (None: stopped).
struct Clock { life: f32, ran: Duration, since: Option<Instant>, closing: bool }

impl Clock {
    fn elapsed(&self) -> f32 { (self.ran + self.since.map_or(Duration::ZERO, |t| t.elapsed())).as_secs_f32() }
    fn over(&self) -> bool { self.elapsed() >= self.life }
    fn opacity(&self) -> f32 { ((self.life - self.elapsed()) / FADE).clamp(0., 1.) }
    fn fading(&self) -> bool { self.since.is_some() && self.elapsed() > self.life - FADE }
    /// How long until the fade starts; None while stopped.
    fn until_fade(&self) -> Option<Duration> { self.since.map(|_| Duration::from_secs_f32((self.life - FADE - self.elapsed()).max(0.))) }
    fn stop(&mut self) { if let Some(t) = self.since.take() { self.ran += t.elapsed(); } }
    fn go(&mut self) { if self.since.is_none() { self.since = Some(Instant::now()); } }
    /// Fade out now, whatever the pointer does.
    fn close(&mut self) { self.ran = Duration::from_secs_f32((self.life - FADE).max(self.elapsed().min(self.life))); self.since = Some(Instant::now()); self.closing = true; }
}

struct Live { id: u64, toast: Toast, clock: Clock }

/// A window's toasts, the newest first; whether the pointer is over them; the stack's focus and
/// whoever had the focus before a click took it.
#[derive(Default)]
struct Stack { live: Vec<Live>, over: bool, focus: Option<FocusHandle>, prev: Option<FocusHandle> }

impl Stack {
    /// Once no toast is left to take Esc and the keyboard is still here: where it goes back to
    /// (`give_back`; None inside: nowhere).
    fn release(&mut self, window: &Window) -> Option<Option<FocusHandle>> {
        if self.live.iter().any(|l| !l.clock.closing && !l.clock.over()) { return None }
        if !self.focus.as_ref()?.is_focused(window) { return None }
        Some(self.prev.take())
    }
}

fn give_back(to: Option<Option<FocusHandle>>, window: &mut Window, cx: &mut App) {
    match to { Some(Some(p)) => window.focus(&p, cx), Some(None) => window.blur(cx), None => {} }
}

#[derive(Default)]
struct Toaster { stacks: HashMap<WindowId, Stack>, next: u64 }
impl Global for Toaster {}

/// Up to this many wait behind the three shown (each on its own clock) before the oldest goes.
const KEPT: usize = 10;

/// Puts `t` in front of `window`'s stack.
pub fn show(window: AnyWindowHandle, t: Toast, cx: &mut App) {
    let toaster = cx.default_global::<Toaster>();
    toaster.next += 1;
    let id = toaster.next;
    // a window closed with its toasts up is never drawn again: let its stack go once they are over
    toaster.stacks.retain(|_, s| s.live.iter().any(|l| !l.clock.over()));
    let s = toaster.stacks.entry(window.window_id()).or_default();
    let mut clock = Clock { life: t.life(), ran: Duration::ZERO, since: Some(Instant::now()), closing: false };
    if s.over { clock.stop(); }
    let stack = &mut s.live;
    // The same note again (a key typed in 阅读模式) starts over in front rather than piling up.
    let id = match stack.iter().position(|l| !l.clock.closing && l.toast.same(&t)) { Some(i) => stack.remove(i).id, None => id };
    stack.insert(0, Live { id, toast: t, clock });
    stack.truncate(KEPT);
    wake(window, cx);
}

/// Esc on the stack: closes the front toast of `window`; false when it has none (Esc goes on).
fn dismiss_front(window: &mut Window, cx: &mut App) -> bool {
    let handle = window.window_handle();
    let Some(s) = cx.default_global::<Toaster>().stacks.get_mut(&handle.window_id()) else { return false };
    let Some(l) = s.live.iter_mut().find(|l| !l.clock.closing && !l.clock.over()) else { return false };
    l.clock.close();
    let to = s.release(window);
    give_back(to, window, cx);
    window.refresh();
    true
}

/// A click on the stack: the keyboard comes to it (Esc then closes the front toast).
fn take_focus(window: &mut Window, cx: &mut App) {
    let Some(s) = cx.default_global::<Toaster>().stacks.get_mut(&window.window_handle().window_id()) else { return };
    let Some(f) = s.focus.clone() else { return };
    if f.is_focused(window) { return }
    let prev = window.focused(cx);
    if let Some(s) = cx.default_global::<Toaster>().stacks.get_mut(&window.window_handle().window_id()) { s.prev = prev; }
    window.focus(&f, cx);
}

/// Closes every toast of `window` that `pick` picks (a progress note once the work is done).
pub fn dismiss_where(window: AnyWindowHandle, pick: impl Fn(ToastTone, &str) -> bool, cx: &mut App) {
    let Some(s) = cx.default_global::<Toaster>().stacks.get_mut(&window.window_id()) else { return };
    for l in s.live.iter_mut().filter(|l| pick(l.toast.tone, &l.toast.title)) { l.clock.close(); }
    wake(window, cx);
}

/// The ✕, or the action once it has run.
fn close(window: &mut Window, id: u64, cx: &mut App) {
    let handle = window.window_handle();
    if let Some(s) = cx.default_global::<Toaster>().stacks.get_mut(&handle.window_id()) {
        if let Some(l) = s.live.iter_mut().find(|l| l.id == id) { l.clock.close(); }
        let to = s.release(window);
        give_back(to, window, cx);
    }
    wake(handle, cx);
}

/// The pointer came onto the stack or left it: spread it out and stop every clock, or go on.
fn hover(window: AnyWindowHandle, on: bool, cx: &mut App) {
    let Some(s) = cx.default_global::<Toaster>().stacks.get_mut(&window.window_id()) else { return };
    s.over = on;
    for l in s.live.iter_mut().filter(|l| !l.clock.closing) { if on { l.clock.stop() } else { l.clock.go() } }
    wake(window, cx);
}

/// Draws `window` again now and when the next fade starts (the fade's own frames carry it on).
fn wake(window: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| { let _ = window.update(cx, |_, w, _| w.refresh()); });
    let next = cx.try_global::<Toaster>().and_then(|t| t.stacks.get(&window.window_id())).and_then(|s| s.live.iter().filter_map(|l| l.clock.until_fade()).min());
    if let Some(wait) = next {
        cx.spawn(async move |cx| {
            cx.background_executor().timer(wait).await;
            let _ = window.update(cx, |_, w, _| w.refresh());
        }).detach();
    }
}

/// This window's stack, over everything 32pt below the top; None when it is empty.
pub fn toaster(window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let handle = window.window_handle();
    let id = handle.window_id();
    let s = cx.default_global::<Toaster>().stacks.get_mut(&id)?;
    s.live.retain(|l| !l.clock.over());
    if s.live.is_empty() {
        // the last one ran out with the keyboard on it
        let mut s = cx.default_global::<Toaster>().stacks.remove(&id)?;
        let to = s.release(window);
        window.defer(cx, move |window, cx| give_back(to, window, cx));
        return None;
    }
    if s.focus.is_none() { let f = cx.focus_handle(); cx.default_global::<Toaster>().stacks.get_mut(&id)?.focus = Some(f); }
    let s = cx.default_global::<Toaster>().stacks.get_mut(&id)?;
    let (over, focus) = (s.over, s.focus.clone()?);
    if s.live.iter().any(|l| l.clock.fading()) { window.request_animation_frame(); }
    let shown: Vec<_> = s.live.iter().take(SHOWN).map(|l| (l.id, l.toast.clone(), l.clock.opacity())).collect();
    let closer = |id: u64| -> Run { Rc::new(move |window, cx| close(window, id, cx)) };
    let stack_el = if over {
        // spread out: each whole, the newest on top
        div().flex().flex_col().gap(px(GAP)).children(shown.into_iter().map(|(id, t, op)| t.card(id, Some(closer(id)), window).opacity(op)))
    } else {
        // the ones behind as bare cards peeking out under the front one, drawn first so it covers them
        let mut it = shown.into_iter();
        let (fid, front, fop) = it.next()?;
        let behind: Vec<_> = it.enumerate().map(|(i, (id, t, op))| (i + 1, id, t, op)).collect();
        div().relative()
            .children(behind.into_iter().rev().map(|(i, id, _, op)| {
                let (dy, dx) = (PEEK * i as f32, NARROW * i as f32);
                div().id(("toast-behind", id as usize)).absolute().top(px(dy)).bottom(px(-dy)).left(px(dx)).right(px(dx)).rounded(px(geo().toast_r))
                    .bg(color::popover()).shadow(shadow::toast()).opacity(op)
            }))
            .child(front.card(fid, Some(closer(fid)), window).opacity(fop))
    };
    let vw = window.viewport_size().width;
    let area = div().id("toaster").track_focus(&focus).occlude().on_hover(move |on, _, cx| hover(handle, *on, cx))
        .capture_any_mouse_down(|_, window, cx| take_focus(window, cx))
        .on_key_down(|e, window, cx| if e.keystroke.key == "escape" && dismiss_front(window, cx) { cx.stop_propagation() })
        .child(stack_el);
    Some(deferred(anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.), px(TOP)))
        .child(div().w(vw).flex().justify_center().child(area))).with_priority(20).into_any_element())
}
