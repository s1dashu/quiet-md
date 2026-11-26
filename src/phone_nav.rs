//! A phone's navigation, as `UINavigationController` keeps it: 主页 at the
//! root, a folder's page and a document pushed on top. A pushed page has
//! 「‹ 上一级」 at its top left, named after the page it goes back to, and a
//! swipe from the screen's leading edge takes it back, the page following
//! the finger and the gesture cancellable halfway (the platform decides,
//! `BackGestureEvent`). The pages under it are the ones visited; a page
//! reached some other way (launch, the drawer, search) goes back to the
//! level above it: its folder, else 主页.

use crate::app::{View, Workspace};
use crate::settings::{display_title, tr};
use crate::theme::Theme;
use gpui::*;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Place { Home, Folder(String), Doc(String) }

/// The pages visited under the one shown, and a back swipe under way.
#[derive(Default)]
pub struct PhoneNav {
    stack: Vec<Place>,
    seen: Option<Place>,
    /// The page shown next is the one gone back to, not a new one.
    popping: bool,
    /// The finger's progress across the screen, 0 to 1.
    swipe: Option<f32>,
    /// After the finger lifts: the page settling, back (`true`) or in place.
    settle: Option<(Instant, f32, bool)>,
    /// The page drawn under a swipe (`render_back_layer`): its own 「‹」 is its own way back.
    under: Option<Place>,
}

/// UINavigationController's transition: about a third of a second, easing out.
const SETTLE: Duration = Duration::from_millis(300);
/// How far the page underneath starts to the left, as a share of the width.
const PARALLAX: f32 = 0.3;

impl Workspace {
    fn place(&self) -> Option<Place> {
        match self.view {
            View::Home => Some(Place::Home),
            View::Folder => self.folder_current.clone().map(Place::Folder),
            View::Doc => self.current.clone().map(Place::Doc),
            _ => None,
        }
    }

    fn alive(&self, p: &Place) -> bool {
        match p { Place::Home => true, Place::Folder(id) | Place::Doc(id) => self.entries.iter().any(|e| &e.id == id) }
    }

    /// The level above `p`: its folder, else 主页.
    fn up_of(&self, p: &Place) -> Option<Place> {
        let row = match p { Place::Home => return None, Place::Folder(id) => id.clone(), Place::Doc(id) => self.row_of(id) };
        let parent = self.entries.iter().find(|e| e.id == row).and_then(|e| e.meta.parent.clone());
        Some(parent.filter(|id| self.entries.iter().any(|e| &e.id == id && e.meta.folder.is_some())).map_or(Place::Home, Place::Folder))
    }

    /// Where 「‹」 and the edge swipe go: the page visited before, else the level above.
    pub(crate) fn back_target(&self) -> Option<Place> {
        let nav = &self.phone_nav;
        let (now, stack) = match &nav.under {
            Some(p) if nav.stack.last() == Some(p) => (p.clone(), &nav.stack[..nav.stack.len() - 1]),
            Some(p) => (p.clone(), &nav.stack[..]),
            None => (self.place()?, &nav.stack[..]),
        };
        stack.last().filter(|p| **p != now && self.alive(p)).cloned().or_else(|| self.up_of(&now))
    }

    fn place_name(&self, p: &Place) -> String {
        match p {
            Place::Home => tr("主页", "Home").to_string(),
            Place::Folder(id) | Place::Doc(id) => self.entries.iter().find(|e| &e.id == id).map(|e| display_title(&e.title)).unwrap_or_default(),
        }
    }

    /// Each frame on a phone: a page shown since the last one is pushed on
    /// the one before it (or, already under it, unwinds to it); 主页 is the root.
    pub(crate) fn track_place(&mut self) {
        let Some(now) = self.place() else { return };
        let nav = &mut self.phone_nav;
        if nav.seen.as_ref() == Some(&now) { return }
        if std::mem::take(&mut nav.popping) {
        } else if now == Place::Home {
            nav.stack.clear();
        } else if let Some(i) = nav.stack.iter().rposition(|p| *p == now) {
            nav.stack.truncate(i);
        } else if let Some(prev) = nav.seen.take() {
            nav.stack.push(prev);
            if nav.stack.len() > 64 { nav.stack.remove(0); }
        }
        nav.seen = Some(now);
    }

    pub(crate) fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.back_target() else { return };
        if self.phone_nav.stack.last() == Some(&t) { self.phone_nav.stack.pop(); }
        self.phone_nav.popping = true;
        if let Some(e) = self.editor.clone() { e.update(cx, |e, cx| e.step_aside(window, cx)); }
        match t {
            Place::Home => self.show_view(View::Home, cx),
            Place::Folder(id) => { self.open_folder(&id, window, cx); }
            Place::Doc(id) => self.open(&id, window, cx),
        }
        cx.notify();
    }

    /// 「‹ 上一级」: the chevron on the drawer's icon column, the page it goes back to beside it.
    pub(crate) fn back_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let name = self.place_name(&self.back_target()?);
        // The bar starts at TOUCH_BUTTON_X: the 22pt chevron centred where a 44pt button's glyph is.
        Some(div().id("phone-back").flex_none().h(px(44.)).max_w(px(200.)).pl(px(crate::design::T.tool_h / 2. - 11.)).pr(px(8.))
            .flex().items_center().gap(px(2.)).cursor_pointer().active(|s| s.opacity(0.5))
            .text_size(px(17.)).text_color(crate::design::color::foreground())
            .child(crate::design::icon("chevron-left", 22., crate::design::color::foreground()).flex_none())
            .child(div().min_w_0().truncate().child(crate::editor_ui::ui_text(name)))
            .on_click(cx.listener(|w, _, window, cx| w.go_back(window, cx)))
            .into_any_element())
    }

    pub(crate) fn back_gesture(&mut self, ev: &BackGestureEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.phase {
            BackGesturePhase::Started => {
                if self.back_target().is_none() { return }
                self.phone_nav.swipe = Some(0.);
                self.phone_nav.settle = None;
                if let Some(e) = self.editor.clone() { e.update(cx, |e, cx| e.step_aside(window, cx)); }
            }
            BackGesturePhase::Progressed => if self.phone_nav.swipe.is_some() { self.phone_nav.swipe = Some(ev.progress) },
            BackGesturePhase::Committed | BackGesturePhase::Cancelled => if let Some(p) = self.phone_nav.swipe.take() {
                self.phone_nav.settle = Some((Instant::now(), p, ev.phase == BackGesturePhase::Committed));
            },
        }
        cx.notify();
    }

    /// How far the page is pushed aside this frame (None: in place). A settled
    /// swipe that went back goes back once the page is off the screen.
    pub(crate) fn swipe_progress(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<f32> {
        if let Some((t0, from, commit)) = self.phone_nav.settle {
            // Reduce Motion: no slide.
            let t = if crate::phone::system().reduce_motion { 1. } else { (t0.elapsed().as_secs_f32() / SETTLE.as_secs_f32()).min(1.) };
            let to = if commit { 1. } else { 0. };
            if t >= 1. {
                self.phone_nav.settle = None;
                if commit { cx.defer_in(window, |w, window, cx| w.go_back(window, cx)); return Some(1.) }
                return None;
            }
            window.request_animation_frame();
            return Some(from + (to - from) * (1. - (1. - t).powi(3)));
        }
        self.phone_nav.swipe
    }

    /// The page gone back to, under the one sliding away: it comes in from
    /// the left, dimmed until it is uncovered.
    pub(crate) fn render_back_layer(&mut self, progress: f32, top: f32, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let target = self.back_target()?;
        let vw = f32::from(window.viewport_size().width);
        self.phone_nav.under = Some(target.clone());
        let page = match &target {
            Place::Home => self.render_home(window, cx),
            Place::Folder(id) => {
                let shown = self.folder_current.replace(id.clone());
                let el = if self.phone_pages(window) { self.render_phone_list(true, window, cx) } else { self.render_folder(window, cx) };
                self.folder_current = shown;
                el
            }
            // Another document can't be drawn without opening it: its title on a blank page.
            Place::Doc(_) => div().size_full().px(px(24.)).pt(px(64.)).text_size(px(28.)).font_weight(FontWeight::BOLD).text_color(Theme::text())
                .child(crate::editor_ui::ui_text(self.place_name(&target))).into_any_element(),
        };
        self.phone_nav.under = None;
        let dim = 0.12 * (1. - progress);
        Some(div().absolute().top_0().bottom_0().w(px(vw)).left(px(-(1. - progress) * vw * PARALLAX)).pt(px(top))
            .flex().bg(Theme::bg_body()).overflow_hidden().child(page)
            .child(div().absolute().inset_0().bg(black().opacity(dim)))
            .into_any_element())
    }
}
