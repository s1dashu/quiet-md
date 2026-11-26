//! The footer, after Excalidraw `footer/Footer.tsx`, `Actions.tsx`
//! (`ZoomActions`, `UndoRedoActions`) and `HelpButton.tsx` @ 3a317b5: bottom
//! left, 16 in, the zoom bar (zoom out, the zoom level, zoom in) and the
//! undo / redo bar 8 to its right; bottom right the help button. Looks: each a
//! `design::Toolbar` of ghost sm `Button`s (Nucleo's icons; the level 56 wide),
//! those that can't act now disabled, help `selected` while its dialog is
//! open. Differs: no minimap toggle or zen-mode exit.

use super::*;
use super::tip::ExTip;
use crate::board::action::BoardAction;
use crate::design::{Button, ButtonSize};
use crate::settings::tr;

pub(crate) fn footer(me: &BoardView, cx: &mut Context<BoardView>) -> impl IntoElement {
    let pct = format!("{}%", (me.viewport().zoom * 100.).round() as i64);
    let run = |a: BoardAction| cx.listener(move |me: &mut BoardView, _: &ClickEvent, window, cx| me.run(a.clone(), window, cx));
    let btn = |id: &'static str| Button::ghost(id).sized(ButtonSize::Sm);
    let zoom = bar("board-zoom")
        .child(btn("board-zoom-out").icon("minus").ex_tip_up(tr("缩小", "Zoom out"), Some(&shortcut("CtrlOrCmd+-"))).on_click(run(BoardAction::ZoomOut)))
        .child(btn("board-zoom-reset").label(pct).w(px(56.)).ex_tip_up(tr("重置缩放", "Reset zoom"), None).on_click(run(BoardAction::ResetZoom)))
        .child(btn("board-zoom-in").icon("plus").ex_tip_up(tr("放大", "Zoom in"), Some(&shortcut("CtrlOrCmd++"))).on_click(run(BoardAction::ZoomIn)));
    let (u, r) = (me.can_undo(), me.can_redo());
    let undo = bar("board-history")
        .child(btn("board-undo").icon("arrow-undo").disabled(!u).when(u, |d| d.ex_tip_up(tr("撤销", "Undo"), Some(&shortcut("CtrlOrCmd+Z"))).on_click(run(BoardAction::Undo))))
        .child(btn("board-redo").icon("arrow-redo").disabled(!r).when(r, |d| d.ex_tip_up(tr("重做", "Redo"), Some(&shortcut("CtrlOrCmd+Shift+Z"))).on_click(run(BoardAction::Redo))));
    let open = me.help_open();
    let help = bar("board-help-bar").child(btn("board-help").icon("circle-question").selected(open)
        // no tooltip over the open dialog
        .when(!open, |d| d.ex_tip_up(tr("帮助", "Help"), Some("?")))
        .on_click(cx.listener(|me, _, _, cx| { let open = !me.help_open(); me.set_help_open(open, cx) })));
    div().absolute().left(px(style::PADDING)).right(px(style::PADDING)).bottom(px(style::PADDING)).flex().items_center().gap(px(8.))
        .child(zoom).child(undo).child(div().flex_1()).child(help)
}
