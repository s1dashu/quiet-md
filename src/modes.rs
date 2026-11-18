//! The header's 编辑 / 阅读 mode switch, the 快捷键 drawer (⌘ + /) and the
//! "?" help button at the bottom right of the page that opens it.

use crate::app::Workspace;
use crate::theme::{self, Theme};
use crate::trash::menu_label;
use crate::ui::{ToastKind, icon, menu_glyph, menu_row, popover};
use gpui::{prelude::FluentBuilder, *};

actions!(workspace, [EditMode, ReadMode, ToggleShortcuts]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-alt-e", EditMode, None),
        KeyBinding::new("ctrl-alt-v", ReadMode, None),
        KeyBinding::new("cmd-/", ToggleShortcuts, None),
        KeyBinding::new("cmd-alt-n", crate::app::NewDoc, None),
    ]);
}

/// The mode button: 85×32, 16px left of "…" (whose right edge is
/// `MENU_RIGHT` from the window's).
const DRAWER_W: f32 = 314.;

/// 快捷键 rows: icon, description, keys; `None` separates groups.
type Row = Option<(&'static str, &'static str, &'static str)>;
type VimRow = Option<(&'static str, &'static str, &'static str, &'static str)>;

const VIM_KEYS: &[VimRow] = &[
    Some(("keyboard", "返回普通模式", "Return to Normal mode", "Esc / ⌃[")),
    Some(("pen-line", "光标前 / 行首插入", "Insert before caret / line start", "i / I")),
    Some(("pen-line", "光标后 / 行尾追加", "Append after caret / line end", "a / A")),
    Some(("plus", "下方 / 上方新建行", "Open line below / above", "o / O")),
    Some(("code", "字符 / 整行可视模式", "Visual char / line mode", "v / V")),
    None,
    Some(("arrow-left", "左 / 下 / 上 / 右移动", "Left / down / up / right", "h / j / k / l")),
    Some(("chevron-right", "词首 / 词尾 / 上一词", "Word start / end / back", "w / e / b")),
    Some(("arrow-left", "行首 / 非空首 / 行尾", "Line start / first / end", "0 / ^ / $")),
    Some(("doc", "文档开头 / 结尾", "Document start / end", "gg / G")),
    Some(("heading-1", "上一标题 / 下一标题", "Prev / next heading", "[h / ]h")),
    Some(("list", "上一段 / 下一段", "Prev / next paragraph", "{ / }")),
    Some(("chevron-down", "半页向下 / 向上", "Half page down / up", "⌃D / ⌃U")),
    Some(("text-search", "行内查找字符与重复", "Find char in line & repeat", "f / F / t / T · ; / ,")),
    Some(("text-search", "Flash 屏幕快速跳转", "Flash jump anywhere", "s")),
    Some(("search", "文档内查找与上下匹配", "Find in doc & next/prev", "/ · n / N")),
    None,
    Some(("bin", "删除 / 整行 / 至行尾", "Delete / line / to EOL", "d / dd / D")),
    Some(("square-pen", "修改 / 整行 / 至行尾", "Change / line / to EOL", "c / cc / C")),
    Some(("copy", "复制 / 复制整行 / 粘贴", "Yank / line / paste", "y / yy / p / P")),
    Some(("indent-increase", "增加 / 减少缩进", "Indent / outdent", ">> / <<")),
    Some(("restore", "撤销 / 重做 / 重复", "Undo / redo / repeat", "u / ⌃R / .")),
    Some(("braces", "文本对象（词 / 引号 / 括号）", "Text objects (word/quotes/parens)", "iw / aw / i\" / i(")),
    None,
    Some(("panel-left-open", "切换左栏 / 右栏 / 循环分栏", "Focus left / right / cycle pane", "⌃W h / l / w")),
    Some(("panel-left-open", "快速切换分栏与文档", "Quick pane & doc switch", "⌥H / L / J / K")),
    Some(("restore", "跨视图与文档历史后退 / 前进", "Jump back / forward in history", "⌃O / ⌃I")),
    Some(("copy", "上一篇 / 下一篇文档 (Buffer 环)", "Prev / next doc (buffer ring)", "[b / ]b")),
    Some(("search", "搜索与快速切换文档", "Search & switch documents", "Space b / Space Space")),
    Some(("list", "主页 / 目录大纲 / 历史 / 侧栏", "Home / outline / history / sidebar", "Space h / o / H / e")),
    Some(("keyboard", "打开快捷键速查 / 界面确认与返回", "Cheatsheet / accept & back", "Space ? · Enter / Esc")),
];

fn vim_rows() -> Vec<AnyElement> {
    VIM_KEYS
        .iter()
        .map(|r| match r {
            None => div().h(px(16.)).into_any_element(),
            Some((ic, zh, en, keys)) => div()
                .flex()
                .items_center()
                .py(px(8.))
                .border_b_1()
                .border_color(Theme::line_divider())
                .child(div().flex_none().mr(px(8.)).child(icon(ic, 16., Theme::icon())))
                .child(div().flex_none().mr(px(12.)).text_size(px(13.)).line_height(px(18.)).child(crate::settings::tr(zh, en)))
                .child(div().flex_1().min_w_0().flex().justify_end().text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).child(crate::shortcuts::keys(keys)))
                .into_any_element(),
        })
        .collect()
}

const STANDARD: &[Row] = &[
    Some(("keyboard", "阅读模式切回编辑", "E")), Some(("keyboard", "复制 / 剪切 / 粘贴", "⌘C / ⌘X / ⌘V")),
    Some(("keyboard", "全选", "⌘A")), Some(("keyboard", "逐字扩选", "⇧← / ⇧→")), Some(("keyboard", "逐行扩选", "⇧↑ / ⇧↓")),
    Some(("keyboard", "选到行首 / 行尾", "⌘⇧← / ⌘⇧→")), Some(("keyboard", "选到文档开头 / 结尾", "⌘⇧↑ / ⌘⇧↓")),
];
const MARKDOWN: &[Row] = &[
    Some(("heading-1", "一级标题", "# 空格")),
    Some(("heading-2", "二级标题", "## 空格")),
    Some(("heading-3", "三级标题", "### 空格")),
    Some(("list-ordered", "有序列表", "1. 空格")),
    Some(("list", "无序列表", "- 空格 或 * 空格")),
    Some(("list-todo", "任务列表", "[] 空格")),
    Some(("indent-increase", "增加缩进", "Tab")),
    Some(("indent-decrease", "减少缩进", "Shift + Tab")),
    Some(("quote", "引用", "> 空格")),
    Some(("code", "代码", "`代码`")),
    Some(("braces", "代码块", "``` 空格 或 ``` 代码语言 空格")),
    None,
    Some(("bold", "加粗", "**文字** 空格")),
    Some(("italic", "斜体", "*文字* 空格")),
    Some(("underline", "下划线", "~文字~ 空格")),
    Some(("strikethrough", "删除线", "~~文字~~ 空格")),
    Some(("sigma", "公式", "$$文字$$")),
    Some(("divider", "分隔线", "--- 或 ***")),
];

fn rows(list: &[Row]) -> Vec<AnyElement> {
    list.iter()
        .map(|r| match r {
            None => div().h(px(24.)).into_any_element(),
            Some((ic, desc, keys)) => div()
                .flex()
                .items_center()
                .py(px(10.))
                .border_b_1()
                .border_color(Theme::line_divider())
                .child(div().flex_none().mr(px(8.)).child(icon(ic, 16., Theme::icon())))
                .child(div().flex_none().mr(px(16.)).text_size(px(13.)).line_height(px(18.)).child(crate::settings::text(desc)))
                .child(div().flex_1().min_w_0().flex().justify_end().text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).child(crate::shortcuts::keys(crate::settings::text(keys))))
                .into_any_element(),
        })
        .collect()
}

fn section(title: &'static str) -> Div {
    div().text_size(px(13.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).pb(px(16.)).child(title)
}

impl Workspace {
    /// The open document is in 阅读模式.
    pub fn reading(&self) -> bool {
        self.current.as_ref().and_then(|id| self.entries.iter().find(|e| &e.id == id)).is_some_and(|e| e.meta.reading)
    }

    pub fn set_mode(&mut self, reading: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.mode_menu = false;
        cx.notify();
        let Some(id) = self.current.clone() else { return };
        if self.reading() == reading {
            return;
        }
        // The snapshot reading mode keeps must be what is on disk.
        if !self.save_before(cx, move |w, window, cx| w.set_mode(reading, window, cx)) {
            return;
        }
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.meta.reading = reading;
        }
        let _ = self.store.update_meta(&id, |m| m.reading = reading);
        if let Some(ed) = &self.editor {
            ed.update(cx, |e, cx| e.set_read_only(reading, cx));
            ed.read(cx).focus(window);
        }
        self.show_toast_kind(ToastKind::Info, if reading { crate::settings::text("已切换为阅读模式") } else { crate::settings::text("已切换为编辑模式") }, cx);
    }

    pub fn set_info_row(&mut self, info: crate::store::InfoRow) {
        let Some(id) = self.current.clone() else { return };
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.meta.info = info;
        }
        let _ = self.store.update_meta(&id, |m| m.info = info);
    }

    /// The mode menu: a native menu with two-line rows (name over a
    /// description); hovering paints the accent, the current mode carries a check.
    pub(crate) fn render_mode_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.mode_menu || self.current.is_none() {
            return None;
        }
        let at = self.mode_btn_bounds.get()?;
        let reading = self.reading();
        let item = |id: &'static str, ic: &'static str, title: &'static str, desc: &'static str, selected: bool, to_reading: bool| {
            let g = SharedString::from(id);
            menu_row(id, g.clone())
                .h(px(42.))
                .child(menu_glyph(ic, 16., g.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(menu_label(title, g.clone(), gpui::white()).text_size(px(13.)).line_height(px(18.)))
                        .child(menu_label(desc, g.clone(), theme::rgbaf(255, 255, 255, 0.85)).text_size(px(11.)).line_height(px(15.)).text_color(Theme::text_caption())),
                )
                .when(selected, |d| d.child(menu_glyph("check", 14., g)))
                .on_click(cx.listener(move |w, _, window, cx| w.set_mode(to_reading, window, cx)))
        };
        let btn = self.mode_btn_bounds.clone();
        let menu = popover()
            .id("doc-mode-menu")
            .occlude()
            .w(px(180.))
            .py(px(crate::ui::MENU_PAD_Y))
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                // The button toggles the menu itself.
                if btn.get().is_none_or(|b| !b.contains(&ev.position)) {
                    w.mode_menu = false;
                    cx.notify();
                }
            }))
            .child(item("mode-edit", "pen-line", crate::settings::text("编辑"), crate::settings::text("可编辑文档"), !reading, false))
            .child(item("mode-read", "eye-open", crate::settings::text("阅读"), crate::settings::text("仅查看或评论文档"), reading, true));
        Some(deferred(anchored().position(point(at.left(), at.bottom() + px(4.))).child(menu)).with_priority(2).into_any_element())
    }

    /// The 快捷键 drawer to the right of the page, below the top bar.
    pub(crate) fn render_shortcuts(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.shortcuts {
            return None;
        }
        Some(
            div()
                .id("shortcuts")
                .flex_none()
                .w(px(DRAWER_W))
                .h_full()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(Theme::line_divider())
                .bg(crate::theme::raised(Theme::bg_body()))
                .child(
                    div()
                        .flex_none()
                        .h(px(49.))
                        .px(px(17.))
                        .py(px(12.))
                        .flex()
                        .items_center()
                        .border_b_1()
                        .border_color(Theme::line_divider())
                        .child(div().flex_1().text_size(px(15.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).child(crate::settings::tr("快捷键", "Keyboard shortcuts")))
                        .child(crate::trash::close_button("shortcuts-close").mr(px(5.)).on_click(cx.listener(|w, _, _, cx| {
                            w.shortcuts = false;
                            cx.notify();
                        }))),
                )
                .child(
                    div()
                        .id("shortcuts-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .pt(px(24.))
                        .px(px(16.))
                        .pb(px(24.))
                        .text_color(Theme::text())
                        .child(section(crate::settings::text("键盘快捷键")))
                        .children(crate::shortcuts::rows(cx).into_iter().map(|(_, title, key)| div().flex().items_center().justify_between().gap(px(12.)).py(px(10.)).text_size(px(12.)).child(title).child(crate::shortcuts::pretty(&key)).into_any_element()))
                        .children(rows(STANDARD))
                        .child(section("Markdown").mt(px(32.)))
                        .child(
                            div()
                                .mt(px(-4.))
                                .pb(px(16.))
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(Theme::text_caption())
                                .child(crate::settings::text("Markdown 是一种快速输入带格式文本的方式，让你书写文档时行云流水，一气呵成，例如输入#空格，即可输入一级标题。")),
                        )
                        .children(rows(MARKDOWN))
                        .child(section(crate::settings::tr("Vim 模式", "Vim mode")).mt(px(32.)))
                        .child(
                            div()
                                .mt(px(-4.))
                                .pb(px(12.))
                                .text_size(px(12.))
                                .line_height(px(18.))
                                .text_color(Theme::text_caption())
                                .child(crate::settings::tr(
                                    "在 设置 › 通用 中开启 Vim 编辑模式后，可在正文及侧栏、主页、文件夹、回收站、设置、目录大纲与版本历史中使用全键盘操作。",
                                    "Enable Vim mode in Settings › General for modal editing and full-keyboard navigation across sidebar, Home, folders, Trash, Settings, Outline and History.",
                                )),
                        )
                        .children(vim_rows()),
                )
                .into_any_element(),
        )
    }

}
