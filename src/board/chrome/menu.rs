//! The main menu and the dropdown pieces, after Excalidraw
//! `main-menu/MainMenu.tsx`, `main-menu/DefaultItems.tsx` and
//! `dropdownMenu/*` @ 3a317b5: the hamburger (`.main-menu-trigger`, a ghost
//! button on a one-item `design::Toolbar`) opens a menu in the app's look
//! (`design::Popover`, `MenuItem`: icon, text, shortcut) 4 under it. The
//! default items we support: 帮助 and 重置画布 (with its confirm dialog) and
//! the canvas background (`ChangeCanvasBackground`).
//! Differs: no open / save / export / search / socials / theme items (the
//! app's header, above the board, has the way back and the document menu).

use super::*;
use crate::board::action::BoardAction;
use crate::settings::tr;
use crate::design::toolbar::TOOLBAR_H;
use crate::design::{self, Button, ButtonSize, MenuItem, MenuLabel, MenuSeparator, Popover};

/// `DropdownMenuContent`: the app's menu (`design::Popover`).
pub(crate) fn dropdown(id: impl Into<ElementId>) -> Popover {
    eat(Popover::new(id).menu().w(px(crate::design::menu_w::SM)))
}

/// `DropdownMenuItem`: a menu row with its icon and the shortcut at the end;
/// the current choice ticked (`DropdownMenuRadioItem`).
pub(crate) fn dropdown_item(id: impl Into<ElementId>, icon_name: Option<&str>, label: impl Into<SharedString>, shortcut: Option<String>, current: bool) -> MenuItem {
    MenuItem::new(id).when(current, |d| d.checked(true))
        .when_some(icon_name, |d, n| d.icon(n.to_string()))
        .label(label)
        .when_some(shortcut, |d, k| d.shortcut(k))
}

/// `DropdownMenuSeparator`.
pub(crate) fn dropdown_separator() -> MenuSeparator { MenuSeparator::new() }

/// Top left (`.excalidraw-ui-top-left`): the hamburger and its menu.
pub(crate) fn main_menu(me: &BoardView, window: &mut Window, cx: &mut Context<BoardView>) -> impl IntoElement {
    let open = matches!(pop(cx), Some(Pop::MainMenu | Pop::CanvasBackground));
    let trigger = bar("board-main-menu-bar").child(Button::ghost("board-main-menu").sized(ButtonSize::Sm).icon("menu").selected(open)
        .on_click(cx.listener(|_, _, _, cx| { let open = matches!(pop(cx), Some(Pop::MainMenu | Pop::CanvasBackground)); set_pop(if open { None } else { Some(Pop::MainMenu) }, cx) })));
    // the board sits in the note pane, under the app's header (which names it)
    let mut col = div().absolute().top(px(style::TOP_PADDING)).left(px(style::PADDING)).child(trigger);
    if open {
        let run = |a: BoardAction| cx.listener(move |me: &mut BoardView, _: &ClickEvent, window, cx| { set_pop(None, cx); me.run(a.clone(), window, cx) });
        let menu = dropdown("board-main-menu-list").w(px(crate::design::menu_w::MD))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| if pop(cx) == Some(Pop::MainMenu) { set_pop(None, cx) }))
            .child(dropdown_item("board-mm-help", Some("circle-question"), tr("帮助", "Help"), Some("?".into()), false).on_click(run(BoardAction::ShowHelp)))
            // (upstream shows it on an empty canvas too)
            .child(dropdown_item("board-mm-clear", Some("bin"), tr("重置画布", "Reset the canvas"), None, false)
                .on_click(cx.listener(|_, _, _, cx| set_pop(Some(Pop::ClearCanvas), cx))))
            // `ChangeCanvasBackground`: a label over the picker, in line with the rows' text
            .child(dropdown_separator())
            .child(MenuLabel::new(tr("画布背景", "Canvas background")))
            .child(div().px(px(design::geo().menu_row_px + design::menu::PAD)).pb(px(design::menu::PAD)).child(super::color_picker::color_picker("board-canvas-bg", None, Pop::CanvasBackground, Some(me.app_state().view_background_color.clone()),
                super::colors::CANVAS_BACKGROUND_PICKS, BoardAction::ViewBackgroundColor, me.elements(), window, cx)));
        // 4 under the bar, left edges aligned (shadcn's `sideOffset`)
        col = col.child(div().absolute().top(px(TOOLBAR_H + design::menu::SIDE_OFFSET)).left_0().child(deferred(anchored().snap_to_window_with_margin(px(design::menu::EDGE)).child(menu)).with_priority(3)));
    }
    col
}
