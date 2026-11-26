//! The help dialog and the clear-canvas confirmation, after Excalidraw
//! `HelpDialog.tsx`, `ConfirmDialog.tsx`, `Dialog.tsx` / `Modal.tsx` @
//! 3a317b5: "Help" over "Keyboard shortcuts" in three groups (tools, view,
//! editor; two columns), each row a label and its keys with "or" between.
//! Looks: a `design::Dialog` (closable, 960 wide, its body scrolling) over the
//! board's `scrim()`, each group a `Card` of rows parted by `Separator`s, the
//! keys `design::Kbd`s; the confirmation `Dialog::alert` with outline Cancel
//! and destructive Confirm.
//! Differs: no header links (docs, blog, GitHub, YouTube); only the
//! shortcuts the board has, plus 退出画板 (Esc).

use super::*;
use crate::board::action::{BoardAction, Tool};
use crate::design::{Button, Card, Dialog, Kbd, Separator};
use crate::settings::tr;

/// A row: the label and its alternatives, each a `+`-joined key list
/// (`getShortcutKey` already applied); `and` joins them without "or".
struct Row(&'static str, Vec<String>, bool);

fn r(label: &'static str, keys: &[&str]) -> Row { Row(label, keys.iter().map(|k| shortcut(k)).collect(), false) }

fn tools() -> Vec<Row> {
    use super::toolbar::tool_label;
    let click = tr("单击", "click");
    let mut v: Vec<Row> = [(Tool::Hand, "H", None), (Tool::Selection, "V", Some("1")), (Tool::Rectangle, "R", Some("2")), (Tool::Diamond, "D", Some("3")), (Tool::Ellipse, "O", Some("4")),
        (Tool::Arrow, "A", Some("5")), (Tool::Line, "L", Some("6")), (Tool::FreeDraw, "P", Some("7")), (Tool::Text, "T", Some("8")), (Tool::StickyNote, "N", None), (Tool::Image, "9", None),
        (Tool::Eraser, "E", Some("0")), (Tool::Frame, "F", None)]
        .into_iter().map(|(t, k, n)| Row(tool_label(t), std::iter::once(k.to_string()).chain(n.map(str::to_string)).collect(), false)).collect();
    v.extend([
        r(tr("编辑线条或箭头的点", "Edit line/arrow points"), &["CtrlOrCmd+Enter"]),
        r(tr("添加或编辑文本", "Edit text / add label"), &["Enter"]),
        r(tr("添加新行(文本编辑器)", "Add new line (text editor)"), &["Enter", "Shift+Enter"]),
        r(tr("完成编辑 (文本编辑器)", "Finish editing (text editor)"), &["Esc", "CtrlOrCmd+Enter"]),
        Row(tr("曲线箭头", "Curved arrow"), vec!["A".into(), click.into(), click.into(), click.into()], true),
        Row(tr("曲线", "Curved line"), vec!["L".into(), click.into(), click.into(), click.into()], true),
        Row(tr("裁剪图片", "Crop image"), vec![tr("双击", "double-click").into(), shortcut("Enter")], false),
        r(tr("完成图片裁剪", "Finish image cropping"), &["Enter", "Esc"]),
        r(tr("绘制后保持所选的工具栏状态", "Keep selected tool active after drawing"), &["Q"]),
        r(tr("禁用箭头吸附", "Prevent arrow binding"), &["CtrlOrCmd"]),
    ]);
    v
}

fn view() -> Vec<Row> {
    vec![
        r(tr("放大", "Zoom in"), &["CtrlOrCmd++"]),
        r(tr("缩小", "Zoom out"), &["CtrlOrCmd+-"]),
        r(tr("重置缩放", "Reset zoom"), &["CtrlOrCmd+0"]),
        r(tr("缩放以适应所有元素", "Zoom to fit all elements"), &["Shift+1"]),
        r(tr("缩放到选区", "Zoom to selection"), &["Shift+2"]),
        r(tr("吸附至对象", "Snap to objects"), &["Alt+S"]),
        r(tr("切换网格显示", "Toggle grid"), &["CtrlOrCmd+'"]),
        r(tr("退出画板", "Exit board"), &["Esc"]),
    ]
}

fn editor() -> Vec<Row> {
    let (click, drag) = (tr("单击", "click"), tr("拖动", "drag"));
    let with = |m: &str, s: &str| shortcut(&format!("{m}+{s}"));
    let mac = cfg!(target_os = "macos");
    vec![
        Row(tr("从一个基本元素创建流程图", "Create a flowchart from a generic element"), vec![shortcut("CtrlOrCmd+Arrow Key")], false),
        Row(tr("在流程图元素间移动", "Navigate a flowchart"), vec![shortcut("Alt+Arrow Key")], false),
        Row(tr("移动画布", "Move canvas"), vec![with("Space", drag), with("Wheel", drag)], false),
        r(tr("重置画布", "Reset the canvas"), &["CtrlOrCmd+Delete"]),
        r(tr("删除", "Delete"), &["Delete"]),
        r(tr("剪切", "Cut"), &["CtrlOrCmd+X"]),
        r(tr("拷贝", "Copy"), &["CtrlOrCmd+C"]),
        r(tr("粘贴", "Paste"), &["CtrlOrCmd+V"]),
        r(tr("粘贴为纯文本", "Paste as plaintext"), &["CtrlOrCmd+Shift+V"]),
        r(tr("全部选中", "Select all"), &["CtrlOrCmd+A"]),
        Row(tr("添加元素到选区", "Add element to selection"), vec![with("Shift", click)], false),
        Row(tr("深度选择", "Deep select"), vec![with("CtrlOrCmd", click)], false),
        Row(tr("在方框内深度选择并避免拖拽", "Deep select within box, and prevent dragging"), vec![with("CtrlOrCmd", drag)], false),
        r(tr("拷贝样式", "Copy styles"), &["CtrlOrCmd+Alt+C"]),
        r(tr("粘贴样式", "Paste styles"), &["CtrlOrCmd+Alt+V"]),
        r(tr("置于底层", "Send to back"), &[if mac { "CtrlOrCmd+Alt+[" } else { "CtrlOrCmd+Shift+[" }]),
        r(tr("置于顶层", "Bring to front"), &[if mac { "CtrlOrCmd+Alt+]" } else { "CtrlOrCmd+Shift+]" }]),
        r(tr("下移一层", "Send backward"), &["CtrlOrCmd+["]),
        r(tr("上移一层", "Bring forward"), &["CtrlOrCmd+]"]),
        r(tr("顶部对齐", "Align top"), &["CtrlOrCmd+Shift+Up"]),
        r(tr("底端对齐", "Align bottom"), &["CtrlOrCmd+Shift+Down"]),
        r(tr("左对齐", "Align left"), &["CtrlOrCmd+Shift+Left"]),
        r(tr("右对齐", "Align right"), &["CtrlOrCmd+Shift+Right"]),
        Row(tr("创建副本", "Duplicate"), vec![shortcut("CtrlOrCmd+D"), with("Alt", drag)], false),
        r(tr("锁定/解锁", "Lock/unlock selection"), &["CtrlOrCmd+Shift+L"]),
        r(tr("撤销", "Undo"), &["CtrlOrCmd+Z"]),
        r(tr("重做", "Redo"), if mac { &["CtrlOrCmd+Shift+Z"] } else { &["CtrlOrCmd+Y", "CtrlOrCmd+Shift+Z"] }),
        r(tr("编组", "Group selection"), &["CtrlOrCmd+G"]),
        r(tr("解除编组", "Ungroup selection"), &["CtrlOrCmd+Shift+G"]),
        r(tr("水平翻转", "Flip horizontal"), &["Shift+H"]),
        r(tr("垂直翻转", "Flip vertical"), &["Shift+V"]),
        r(tr("缩小字体大小", "Decrease font size"), &["CtrlOrCmd+Shift+<"]),
        r(tr("放大字体大小", "Increase font size"), &["CtrlOrCmd+Shift+>"]),
    ]
}

/// A key cap: one letter upper case.
fn key(k: &str) -> Kbd { Kbd::new(if k.chars().count() == 1 { k.to_uppercase() } else { k.to_string() }) }

/// `ShortcutIsland`: the caption over a card of rows.
fn island_rows(caption: &'static str, rows: Vec<Row>) -> Div {
    let g = design::geo();
    let mut list = Card::new(caption);
    let n = rows.len();
    for (i, Row(label, alts, and)) in rows.into_iter().enumerate() {
        let mut keys = div().flex_none().flex().items_center().gap(px(4.));
        for (j, alt) in alts.iter().enumerate() {
            if j > 0 && !and { keys = keys.child(div().text_size(px(g.text_xs)).text_color(design::color::muted_foreground()).child(tr("或", "or"))); }
            // `split("+")`, keeping a trailing "+" key ("Cmd++")
            let parts: Vec<String> = if let Some(s) = alt.strip_suffix("++") { s.split('+').map(str::to_string).chain(std::iter::once("+".to_string())).collect() } else { alt.split('+').map(str::to_string).collect() };
            for p in parts { keys = keys.child(key(&p)); }
        }
        list = list.child(div().min_h(px(36.)).flex().items_center().justify_between().gap(px(8.)).px(px(12.)).py(px(6.)).child(div().child(label)).child(keys));
        if i + 1 < n { list = list.child(Separator::new()); }
    }
    div().flex().flex_col().gap(px(g.label_gap)).child(div().text_color(design::color::muted_foreground()).child(caption)).child(list)
}

/// The scrim over the board and the dialog in it; a press outside closes it.
fn modal(id: &'static str, dialog: Dialog, on_close: impl Fn(&mut BoardView, &mut Context<BoardView>) + 'static, cx: &mut Context<BoardView>) -> impl IntoElement {
    deferred(design::scrim().id((id, 1usize)).occlude().p(px(40.))
        .on_mouse_down(MouseButton::Left, cx.listener(move |me, _, _, cx| on_close(me, cx)))
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .child(dialog.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).on_scroll(|_, _, cx| cx.stop_propagation()))).with_priority(5)
}

pub(crate) fn render(window: &mut Window, cx: &mut Context<BoardView>) -> impl IntoElement {
    let wide = window.viewport_size().width >= px(1024.);
    let islands = if wide {
        div().flex().items_start().gap(px(24.))
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(24.)).child(island_rows(tr("工具", "Tools"), tools())).child(island_rows(tr("视图", "View"), view())))
            .child(div().flex_1().min_w_0().child(island_rows(tr("编辑器", "Editor"), editor())))
    } else {
        div().flex().flex_col().gap(px(24.)).child(island_rows(tr("工具", "Tools"), tools())).child(island_rows(tr("视图", "View"), view())).child(island_rows(tr("编辑器", "Editor"), editor()))
    };
    let handle = design::scroll::handle("board-help");
    // as tall as it is, up to what the window leaves; the rest scrolls
    let body = design::scroll::make_room(div().id("board-help-body").flex_initial().min_h_0().overflow_y_scroll().track_scroll(&handle).child(islands)
        .child(design::scroll::bar("board-help", &handle, 0.)), "board-help", 0.);
    let dialog = Dialog::new("board-help-dialog").title(tr("帮助", "Help")).description(tr("快捷键列表", "Keyboard shortcuts"))
        .closable(cx.listener(|me, _, _, cx| me.set_help_open(false, cx))).w(px(960.)).max_w_full().max_h_full().child(body);
    modal("board-help-dialog", dialog, |me, cx| me.set_help_open(false, cx), cx)
}

/// `ConfirmDialog` for `actionClearCanvas`: the question, then Cancel and
/// Confirm (destructive).
pub(crate) fn clear_canvas(_window: &mut Window, cx: &mut Context<BoardView>) -> impl IntoElement {
    let footer = div().flex().justify_end().gap(px(8.))
        .child(Button::outline("board-clear-cancel").label(tr("取消", "Cancel")).on_click(cx.listener(|_, _, _, cx| set_pop(None, cx))))
        .child(Button::destructive("board-clear-confirm").label(tr("确定", "Confirm")).on_click(cx.listener(|me, _, window, cx| { set_pop(None, cx); me.run(BoardAction::ClearCanvas, window, cx) })));
    let dialog = Dialog::alert("board-clear-dialog").title(tr("清除画布", "Clear canvas")).description(tr("这将会清除整个画布。您是否要继续?", "This will clear the whole canvas. Are you sure?")).footer(footer);
    modal("board-clear-dialog", dialog, |_, cx| set_pop(None, cx), cx)
}
