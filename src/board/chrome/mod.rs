//! 画板的界面外壳, after Excalidraw's `LayerUI.tsx` @ 3a317b5: the main menu
//! (top left, `MainMenu`), the tool bar (top center, `Toolbar.tsx`; no
//! `HintViewer` line under it), the styles panel (left,
//! `SelectedShapeActions`), the footer (zoom, undo / redo, help), the context
//! menu and the help dialog. Upstream decides what each piece contains, when
//! it shows and in what order; the looks are the app's design system
//! (`crate::design`, docs/design/DESIGN_SYSTEM.md): every floating piece is a
//! `Toolbar` or a `Popover` (the popover's face and edge), tools and options
//! `Toggle`s, actions ghost or outline `Button`s, the icons Nucleo's
//! (`assets/icons/`), but for the style options' pictures of what they draw
//! (fill, stroke, sloppiness, arrowheads …), which stay upstream's
//! (`assets/icons/excalidraw/`). The canvas itself keeps upstream's look
//! (`style.rs`). Differs: no collaboration, library, sidebars, AI, socials,
//! stats, zen / view mode, compact or mobile layouts; the main menu adds 退出,
//! and the top left keeps room for the window's traffic lights. Everything here
//! only reads `BoardView` and dispatches `BoardAction`s / `set_tool`.

pub(crate) mod color_picker;
pub mod colors;
mod context_menu;
mod controls;
mod help;
mod menu;
mod properties;
pub mod style;
mod tip;
mod toolbar;

use super::BoardView;
use crate::design;
use gpui::prelude::FluentBuilder;
use gpui::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// The chrome's open popup (upstream `appState.openPopup` / `openMenu`; one
/// at a time).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pop { Stroke, Background, StartArrowhead, EndArrowhead, Font, ExtraTools, MainMenu, CanvasBackground, ClearCanvas }

thread_local! {
    /// Chrome-only state, per board view: which popup is open.
    static POP: RefCell<HashMap<EntityId, Pop>> = RefCell::default();
}

pub(crate) fn pop(cx: &Context<BoardView>) -> Option<Pop> { POP.with(|p| p.borrow().get(&cx.entity_id()).copied()) }

pub(crate) fn set_pop(p: Option<Pop>, cx: &mut Context<BoardView>) {
    let id = cx.entity_id();
    POP.with(|m| match p { Some(p) => { m.borrow_mut().insert(id, p); } None => { m.borrow_mut().remove(&id); } });
    color_picker::popup_changed(p, cx);
    properties::font_popup_changed(p == Some(Pop::Font), cx);
    cx.notify();
}

/// The chrome as its own view, so GPUI keeps its layout and paint from the
/// last frame (`AnyView::cached`) while only the drawing changes: on a
/// pointer move that hovers or drags the selection (`BoardView::chrome_quiet`)
/// with the app state and the selected elements as they were (moves aside:
/// `BoardView::scope_unchanged`), it isn't told to redraw; every other
/// change to the board redraws it.
pub(crate) struct ChromeView {
    board: WeakEntity<BoardView>,
    /// The selection scope and app state it last redrew for.
    seen: Option<(super::ScopeStamp, super::app_state::AppState)>,
    _watch: Subscription,
}

impl ChromeView {
    pub(crate) fn new(board: &Entity<BoardView>, cx: &mut Context<Self>) -> Self {
        let watch = cx.observe(board, |me, board, cx| {
            let b = board.read(cx);
            let quiet = b.chrome_quiet.replace(false);
            let same = me.seen.as_ref().is_some_and(|(s, a)| *a == b.app && b.scope_unchanged(s));
            if quiet && same { return; }
            if !same { me.seen = Some((b.scope_stamp(), b.app.clone())); }
            cx.notify();
        });
        ChromeView { board: board.downgrade(), seen: None, _watch: watch }
    }
}

impl Render for ChromeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(board) = self.board.upgrade() else { return div().into_any_element() };
        board.update(cx, |me, cx| render(me, window, cx).into_any_element())
    }
}

pub fn render(me: &BoardView, window: &mut Window, cx: &mut Context<BoardView>) -> impl IntoElement {
    let panel = properties::render(me, window, cx);
    if panel.is_none() && matches!(pop(cx), Some(Pop::Stroke | Pop::Background | Pop::StartArrowhead | Pop::EndArrowhead | Pop::Font)) { set_pop(None, cx); }
    div().absolute().top_0().left_0().size_full().font_family(crate::theme::ui_family()).text_size(px(design::geo().text)).text_color(design::color::foreground())
        .child(menu::main_menu(me, window, cx))
        .child(toolbar::render(me, cx))
        .when_some(panel, |d, p| d.child(p))
        .child(controls::footer(me, cx))
        .when_some(context_menu::render(me, window, cx), |d, m| d.child(m))
        .when(me.help_open(), |d| d.child(help::render(window, cx)))
        .when(pop(cx) == Some(Pop::ClearCanvas), |d| d.child(help::clear_canvas(window, cx)))
}

// ---------------------------------------------------------------------------
// shared pieces
// ---------------------------------------------------------------------------

/// Presses on the chrome don't reach the canvas.
pub(crate) fn eat<E: InteractiveElement>(e: E) -> E {
    e.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
}

/// A floating bar (the tool bar, the corners' buttons): `design::Toolbar`, the popover's face and
/// edge, 40 tall.
pub(crate) fn bar(id: impl Into<ElementId>) -> design::Toolbar { eat(design::Toolbar::new(id)) }

/// A floating panel (the styles, a picker): `design::Popover`'s face and edge.
pub(crate) fn panel(id: impl Into<ElementId>) -> design::Popover { eat(design::Popover::new(id)) }

/// An upstream picture of what a style option draws (`assets/icons/excalidraw/`).
pub(crate) fn picture(name: &str, size: f32, color: Hsla) -> Svg {
    svg().path(SharedString::from(format!("icons/excalidraw/{name}.svg"))).size(px(size)).flex_none().text_color(color)
}

/// A toolbar item's shortcut letter in its bottom right corner (upstream's
/// `.ToolIcon__keybinding`): 10pt `muted-foreground`.
pub(crate) fn keybinding(k: impl Into<SharedString>) -> Div {
    div().absolute().right(px(3.)).bottom(px(1.)).text_size(px(10.)).line_height(px(10.)).text_color(design::color::muted_foreground()).child(k.into())
}

/// A panel's section title (`text-xs`, `muted-foreground`), `label_gap` above its controls.
pub(crate) fn legend(t: &'static str) -> Div {
    let g = design::geo();
    div().mb(px(g.label_gap)).text_size(px(g.text_xs)).line_height(px(design::text::XS_LINE)).text_color(design::color::muted_foreground()).child(t)
}

/// The key names upstream's `getShortcutKey` writes ("Cmd+Shift+Z").
pub(crate) fn shortcut(s: &str) -> String {
    use crate::settings::tr;
    let mac = cfg!(target_os = "macos");
    s.split('+').map(|k| match k {
        "CtrlOrCmd" => if mac { "Cmd" } else { "Ctrl" },
        "Alt" => if mac { "Option" } else { "Alt" },
        "Enter" => tr("回车键", "Enter"),
        "Space" => tr("空格", "Space"),
        k => k,
    }).collect::<Vec<_>>().join("+")
}
