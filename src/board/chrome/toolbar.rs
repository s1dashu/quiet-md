//! The tool bar, after Excalidraw `Toolbar.tsx`, `Tools.tsx`, `LockButton.tsx`
//! @ 3a317b5: the tool lock, a separator, hand, selection, the shapes, text,
//! sticky note and eraser, a separator and the "more tools" dropdown (image,
//! frame, lasso: the ones we have); no hint line under it. Each tool shows
//! its letter, and its title "Rectangle — R or 2", as upstream's. Looks: a
//! `design::Toolbar` of `Toggle`s (the active tool pressed) with Nucleo's
//! icons, `ButtonGroupSeparator`s, the dropdown a `Popover` menu hung 4 under
//! the bar. Differs: no pen mode, laser, embed, draw-to-shape, bucket fill or
//! "Generate" items.

use super::*;
use super::menu::{dropdown, dropdown_item};
use super::tip::ExTip;
use crate::board::action::{BoardAction, Tool};
use crate::design::toolbar::{TOOLBAR_H, TOOLBAR_PAD};
use crate::design::{ButtonGroupSeparator, Toggle};
use crate::settings::tr;

/// The main row, in upstream order.
const MAIN: [Tool; 11] = [Tool::Hand, Tool::Selection, Tool::Rectangle, Tool::Diamond, Tool::Ellipse, Tool::Arrow, Tool::Line, Tool::FreeDraw, Tool::Text, Tool::StickyNote, Tool::Eraser];
/// `ExtraToolsDropdown`'s tools we have.
const EXTRA: [Tool; 3] = [Tool::Image, Tool::Frame, Tool::Lasso];

/// The tool's icon (Nucleo UI Outline 18; the diamond drawn to match).
pub(crate) fn tool_icon_name(t: Tool) -> &'static str {
    match t {
        Tool::Selection => "pointer",
        Tool::Lasso => "lasso",
        Tool::Hand => "hand",
        Tool::Rectangle => "shape-square",
        Tool::Diamond => "diamond",
        Tool::Ellipse => "shape-circle",
        Tool::Arrow => "arrow-right",
        Tool::Line => "shape-line",
        Tool::FreeDraw => "pen-line",
        Tool::Text => "type",
        Tool::Image => "image",
        Tool::Eraser => "eraser",
        Tool::Frame => "frame",
        Tool::StickyNote => "sticky-note",
    }
}

/// `toolBar.*`.
pub(crate) fn tool_label(t: Tool) -> &'static str {
    match t {
        Tool::Selection => tr("选择", "Selection"),
        Tool::Lasso => tr("套索选择", "Lasso selection"),
        Tool::Hand => tr("抓手（平移工具）", "Hand (panning tool)"),
        Tool::Rectangle => tr("矩形", "Rectangle"),
        Tool::Diamond => tr("菱形", "Diamond"),
        Tool::Ellipse => tr("椭圆", "Ellipse"),
        Tool::Arrow => tr("箭头", "Arrow"),
        Tool::Line => tr("线条", "Line"),
        Tool::FreeDraw => tr("自由书写", "Draw"),
        Tool::Text => tr("文字", "Text"),
        Tool::Image => tr("插入图像", "Insert image"),
        Tool::Eraser => tr("橡皮", "Eraser"),
        Tool::Frame => tr("画框工具", "Frame tool"),
        Tool::StickyNote => tr("便签", "Sticky note"),
    }
}

/// `getToolLetter`: the badge (the letter, else the number).
fn tool_letter(t: Tool) -> Option<String> {
    let (k, n) = t.shortcut()?;
    if k.chars().all(|c| c.is_ascii_alphabetic()) { Some(k.to_uppercase()) } else { n.map(|n| n.to_string()) }
}

/// `getToolShortcut`: "R or 2".
pub(crate) fn tool_shortcut(t: Tool) -> Option<String> {
    let (k, n) = t.shortcut()?;
    let letter = k.chars().all(|c| c.is_ascii_alphabetic()).then(|| k.to_uppercase());
    Some(match (letter, n) { (Some(l), Some(n)) => format!("{l} {} {n}", tr("或", "or")), (Some(l), None) => l, (None, n) => n.map(|n| n.to_string()).unwrap_or_default() })
}

fn tool_button(me: &BoardView, t: Tool, cx: &mut Context<BoardView>) -> Toggle {
    let on = me.tool() == t;
    Toggle::new(SharedString::from(format!("board-tool-{t:?}"))).icon(tool_icon_name(t)).pressed(on).relative()
        // the hand hides its key (`hideKeyBinding`)
        .when_some(tool_letter(t).filter(|_| t != Tool::Hand), |d, k| d.child(keybinding(k)))
        .ex_tip(tool_label(t), tool_shortcut(t).as_deref())
        .on_click(cx.listener(move |me, _, window, cx| me.set_tool(t, window, cx)))
}

/// `ExtraToolsDropdown`: the trigger shows the chosen extra tool, else the dots;
/// its menu hangs 4 under the bar, right edges lined up.
fn extra_tools(me: &BoardView, cx: &mut Context<BoardView>) -> (Toggle, Option<AnyElement>) {
    let tool = me.tool();
    let selected = EXTRA.contains(&tool);
    let open = pop(cx) == Some(Pop::ExtraTools);
    let trigger = Toggle::new("board-extra-tools").icon(if selected { tool_icon_name(tool) } else { "ellipsis" }).pressed(selected || open)
        .when(!open, |d| d.ex_tip(tr("更多工具", "More tools"), None))
        .on_click(cx.listener(|_, _, _, cx| { let open = pop(cx) == Some(Pop::ExtraTools); set_pop(if open { None } else { Some(Pop::ExtraTools) }, cx) }));
    if !open { return (trigger, None); }
    let mut menu = dropdown("board-extra-tools-menu");
    for t in EXTRA {
        let k = match t { Tool::Image => Some("9".to_string()), Tool::Frame => Some("F".to_string()), _ => None };
        menu = menu.child(dropdown_item(("board-extra-tool", t as usize), Some(tool_icon_name(t)), tool_label(t), k, tool == t)
            .on_click(cx.listener(move |me, _, window, cx| { set_pop(None, cx); me.set_tool(t, window, cx) })));
    }
    let menu = menu.on_mouse_down_out(cx.listener(|_, _, _, cx| if pop(cx) == Some(Pop::ExtraTools) { set_pop(None, cx) }));
    let at = div().absolute().top(px(TOOLBAR_H + crate::design::menu::SIDE_OFFSET)).right(px(TOOLBAR_PAD))
        .child(deferred(anchored().anchor(Anchor::TopRight).snap_to_window_with_margin(px(crate::design::menu::EDGE)).child(menu)).with_priority(3));
    (trigger, Some(at.into_any_element()))
}

pub(crate) fn render(me: &BoardView, cx: &mut Context<BoardView>) -> impl IntoElement {
    let locked = me.tool_locked();
    let lock = Toggle::new("board-tool-lock").icon(if locked { "lock" } else { "lock-open" }).pressed(locked)
        .ex_tip(&format!("{} — Q", tr("绘制后保持所选的工具栏状态", "Keep selected tool active after drawing")), None)
        .on_click(cx.listener(|me, _, window, cx| me.run(BoardAction::ToggleToolLock, window, cx)));
    let (more, menu) = extra_tools(me, cx);
    let mut tb = bar("board-toolbar").child(lock).child(ButtonGroupSeparator::new());
    for t in MAIN { tb = tb.child(tool_button(me, t, cx)); }
    let tb = tb.child(ButtonGroupSeparator::new()).child(more).overlays(menu);
    div().absolute().top(px(style::TOP_PADDING)).left_0().right_0().flex().justify_center().child(tb)
}
