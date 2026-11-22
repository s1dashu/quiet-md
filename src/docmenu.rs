//! The header "…" menu: page width, find & replace, move, pin, star,
//! 新建子文档, duplicate, 下载为 PDF / Markdown, 复制为 Markdown, 打印, document info,
//! 阅读模式 and 显示引用列表 switches, delete.

use crate::app::Workspace;
use crate::settings::display_title;
use crate::store::PageWidth;
use crate::theme::{self, Theme, craft};
use crate::ui::popover;
use gpui::{prelude::FluentBuilder, *};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DocSub {
    Width,
    Download,
}

const MENU_W: f32 = 200.;
/// Gap between the "…" button and the menu.
const MENU_GAP: f32 = 6.;
/// Rows: 32 tall; dividers 1px with 5px either side; 6px padding.
const ROW_H: f32 = crate::ui::MENU_ROW_H;
const DIV_H: f32 = 11.;
const PAD: f32 = crate::ui::MENU_PAD_Y;

/// Offset of item `k`'s row from the menu's top with `dividers` divider rows above it.
fn item_top(k: usize, dividers: usize) -> f32 {
    1. + PAD + ROW_H * k as f32 + DIV_H * dividers as f32
}

fn menu_item(id: &'static str, ic: &'static str, label: impl Into<SharedString>, sub: bool, open: bool) -> Stateful<Div> {
    let g = SharedString::from(id);
    crate::ui::menu_row(id, g.clone())
        // A row whose submenu is open keeps the hover fill.
        .when(open, |d| d.bg(Theme::fill_hover()))
        .child(crate::ui::menu_glyph(ic, 16., g.clone()))
        .child(div().flex_1().min_w_0().overflow_hidden().child(crate::ui::menu_label(label, g.clone(), gpui::white())))
        .when(sub, |d| d.child(crate::ui::menu_glyph("chevron-right", 10., g)))
}

/// 下载为's text-only items.
pub(crate) fn sub_item(id: &'static str, label: &'static str) -> Stateful<Div> {
    crate::ui::menu_row(id, SharedString::from(id)).mx(px(0.)).child(crate::ui::menu_label(label, SharedString::from(id), gpui::white()))
}

/// A row's on/off switch.
fn switch(on: bool) -> Div {
    div()
        .flex_none()
        .w(px(28.))
        .h(px(16.))
        .rounded_full()
        .p(px(2.))
        .flex()
        .when(on, |d| d.justify_end())
        .bg(if on { Theme::primary_500() } else { Theme::block_placeholder() })
        .child(div().size(px(12.)).rounded_full().bg(crate::theme::raised(Theme::bg_body())))
}

/// Native-menu separator: 1px, 5px either side, inset 13px.
fn divider() -> Div {
    div().flex_none().mx(px(13.)).my(px(5.)).h(px(1.)).bg(Theme::line_border())
}

impl Workspace {
    fn close_doc_menu(&mut self, cx: &mut Context<Self>) {
        self.doc_menu = false;
        self.doc_sub = None;
        cx.notify();
    }

    pub fn set_page_width(&mut self, width: PageWidth, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.meta.width = width;
        }
        let _ = self.store.set_width(&id, width);
        if let Some(ed) = &self.editor {
            ed.update(cx, |e, cx| {
                e.page_width = width;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// The document as it stands: the open editor's copy, or the file on disk.
    pub(crate) fn doc_of(&mut self, id: &str, cx: &mut Context<Self>) -> Option<crate::model::Document> {
        if self.current.as_deref() == Some(id) {
            self.flush(cx);
            return self.editor.as_ref().map(|ed| ed.read(cx).core.doc.clone());
        }
        self.store.load(id).ok()
    }

    /// 创建副本: "<标题> 副本", opened right away.
    pub fn duplicate_doc(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let file_before = self.store.track_file_changes();
        self.close_doc_menu(cx);
        self.row_menu = None;
        if let Some(new) = self.copy_doc(id, cx) {
            self.entries = self.store.list();
            self.sort();
            self.open(&new, window, cx);
        }
        self.remember_files(file_before, crate::settings::tr("创建副本", "Duplicate"));
        cx.notify();
    }

    /// 转换为 Markdown / 转换为 Quiet.md 文档 (#312): asks first, listing it,
    /// when the other format cannot keep something.
    pub fn ask_convert(&mut self, id: &str, markdown: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_doc_menu(cx);
        self.row_menu = None;
        if self.current.as_deref() == Some(id) {
            let retry = id.to_string();
            if !self.save_before(cx, move |w, window, cx| w.ask_convert(&retry, markdown, window, cx)) { return; }
        }
        if !self.document_peers(id, window, cx).is_empty() {
            self.show_toast_kind(crate::ui::ToastKind::Error, crate::settings::tr("请先关闭正在编辑这篇文档的其他窗口，再转换", "Close the other windows editing this document first, then convert it"), cx);
            return;
        }
        let losses = if !markdown {
            if self.store.front_matter_of(id).trim().is_empty() { vec![] } else { vec![crate::markdown::Loss::FrontMatter] }
        } else {
            match self.store.load(id) {
                Ok(doc) => crate::markdown::losses(&doc),
                Err(e) => return self.convert_failed(&e, cx),
            }
        };
        if losses.is_empty() { return self.convert_doc(id, markdown, window, cx); }
        self.confirm = Some(crate::app::Confirm::Convert { id: id.to_string(), markdown, losses });
        cx.notify();
    }

    /// The conversion itself; the open document is reopened in its new format.
    pub(crate) fn convert_doc(&mut self, id: &str, markdown: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.file_task_busy() || self.file_history.busy || self.tree_writes_pending() { return; }
        let open = self.current.as_deref() == Some(id);
        if open {
            let retry = id.to_string();
            if !self.save_before(cx, move |w, window, cx| w.convert_doc(&retry, markdown, window, cx)) { return; }
        }
        if let Err(e) = self.store.convert(id, markdown) { return self.convert_failed(&e, cx); }
        if open {
            self.save_task = None;
            self.unsaved = false;
            self.current = None;
            self.editor = None;
            self.editor_sub = None;
        }
        let (entries, trash) = (self.store.list(), self.store.list_trash());
        self.sync_peers(cx, &entries, Some(&trash));
        self.entries = entries;
        self.trash = trash;
        self.sort();
        if open {
            let view = self.view;
            self.open(id, window, cx);
            self.view = view;
        }
        let warnings = self.store.take_warnings();
        if warnings.is_empty() { self.show_toast(if markdown { crate::settings::tr("已转换为 Markdown", "Converted to Markdown") } else { crate::settings::tr("已转换为 Quiet.md 文档", "Converted to a Quiet.md document") }, cx); }
        else { self.show_toast_kind(crate::ui::ToastKind::Error, &warnings.iter().map(|s| crate::settings::diagnostic(s)).collect::<Vec<_>>().join(crate::settings::tr("；", "; ")), cx); }
        cx.notify();
    }

    fn convert_failed(&mut self, e: &anyhow::Error, cx: &mut Context<Self>) {
        let msg = crate::localized_format!("转换失败：{}", "Could not convert document: {}", crate::settings::diagnostic(&e.to_string()));
        self.show_toast_kind(crate::ui::ToastKind::Error, &msg, cx);
        self.status = crate::app::SaveStatus::Failed(msg);
        cx.notify();
    }

    /// Write "<标题> 副本" of `id` to the library; the caller refreshes the list.
    pub(crate) fn copy_doc(&mut self, id: &str, cx: &mut Context<Self>) -> Option<String> {
        if self.entries.iter().any(|e| e.id == id && e.meta.folder.is_some()) { return self.copy_folder(id, cx); }
        let mut doc = self.doc_of(id, cx)?;
        let title = crate::localized_format!("{} 副本", "{} copy", display_title(&doc.title.text));
        doc.title = crate::model::RichText::plain(title);
        let width = self.entries.iter().find(|e| e.id == id).map(|e| e.meta.width).unwrap_or_default();
        // A Markdown file copies as Markdown, into the library even from a 本机文件.
        // A 画板 copies as a 画板.
        let board = self.entries.iter().find(|e| e.id == id).filter(|e| e.meta.kind == crate::store::DocKind::Board)
            .map(|_| doc.blocks.iter().find_map(|b| match &b.kind { crate::model::BlockKind::Board(bd) => Some(bd.clone()), _ => None }).unwrap_or_default());
        let table = self.entries.iter().any(|e| e.id == id && e.meta.kind == crate::store::DocKind::Table);
        if table { crate::table::flush_all(cx); }
        let made = if table { self.store.copy_table(id, &doc.title.text) }
            else if let Some(board) = board { self.store.create_board_in(&doc.title.text, &board, None) }
            else if self.store.is_markdown(id) { let front = self.store.front_matter_of(id); self.store.create_markdown(&doc, &front) } else { self.store.create_from(&doc) };
        match made {
            Ok(new) => {
                let _ = self.store.set_width(&new, width);
                // A document with children is copied alone, beside its folder.
                let row = self.row_of(id);
                let parent = self.entries.iter().find(|e| e.id == row).filter(|e| !e.meta.local()).and_then(|e| e.meta.parent.clone());
                if let Some(parent) = parent { let _ = self.store.set_parent(&new, Some(&parent)); }
                Some(new)
            }
            Err(e) => {
                self.status = crate::app::SaveStatus::Failed(crate::localized_format!("创建副本失败：{}", "Could not duplicate document: {}", crate::settings::diagnostic(&e.to_string())));
                None
            }
        }
    }

    /// 查找和替换 (also ⌘F / ⌘⇧H).
    pub fn find_in_doc(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_doc_menu(cx);
        // the document's text is hidden under an open board
        if self.editor.as_ref().is_some_and(|e| e.read(cx).board_open()) { return; }
        if let Some(ed) = &self.editor {
            ed.update(cx, |e, cx| e.open_find(replace, window, cx));
        }
    }

    pub(crate) fn render_doc_menu(&self, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.doc_menu {
            return vec![];
        }
        let Some(id) = self.current.clone() else { return vec![] };
        let entry = self.entries.iter().find(|e| e.id == id);
        let pinned = entry.is_some_and(|e| e.meta.pinned);
        let starred = entry.is_some_and(|e| e.meta.starred > 0);
        let width = entry.map(|e| e.meta.width).unwrap_or_default();
        let refs_shown = !entry.is_some_and(|e| e.meta.refs_hidden);
        // An open board isn't the page: no page width, find in its text or
        // editor shortcuts; a standalone board file has no Markdown or
        // references either
        let (board, board_file) = self.editor.as_ref().map_or((false, false), |e| { let e = e.read(cx); (e.board_open(), e.board_file) });
        let reading = self.reading();
        let sub = self.doc_sub;
        let hover_sub = |s: Option<DocSub>| {
            cx.listener(move |w: &mut Workspace, hovered: &bool, _, cx| {
                if *hovered && w.doc_sub != s {
                    w.doc_sub = s;
                    cx.notify();
                }
            })
        };
        let sub_bounds = self.doc_sub_bounds.clone();
        // The menu follows its longest translation (#225); its rendered width
        // (from the previous frame — a submenu only opens after one) places
        // the submenus that open to its left.
        let menu_w_record = self.doc_menu_w.clone();
        let menu_w = move || {
            let record = menu_w_record.clone();
            canvas(move |b, _, _| record.set(Some(f32::from(b.size.width))), |_, _, _, _| {}).absolute().inset_0()
        };
        let (pin_id, star_id, mv_id, del_id, dup_id, print_id) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
        // 新建子文档 (#357): under a document of the library.
        let child = !board_file && self.takes_children(&id);
        let child_id = id.clone();
        let menu = popover()
            .id("doc-menu")
            .occlude()
            .min_w(px(MENU_W))
            .py(px(PAD))
            .flex()
            .flex_col()
            .shadow(theme::shadow_s4_down())
            .child(menu_w())
            .on_mouse_down_out(cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                if sub_bounds.get().is_none_or(|b| !b.contains(&ev.position)) {
                    w.close_doc_menu(cx);
                }
            }))
            .when(!board, |d| d.child(
                menu_item("dm-width", "page-width", crate::settings::text("页宽设置"), true, sub == Some(DocSub::Width))
                    .on_hover(hover_sub(Some(DocSub::Width))),
            )
            .child(divider())
            .child(
                menu_item("dm-find", "text-search", crate::settings::text("查找和替换"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, window, cx| w.find_in_doc(false, window, cx))),
            )
            .child(divider()))
            .child(
                menu_item("dm-move", "folder-input", crate::settings::text("移动到"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| {
                        w.close_doc_menu(cx);
                        w.open_move(vec![mv_id.clone()], window, cx);
                    })),
            )
            .child(
                menu_item(
                    "dm-pin",
                    if pinned { "pin-off" } else { "pin" },
                    if pinned { crate::settings::text("从“置顶”移除") } else { crate::settings::text("添加到“置顶”") },
                    false,
                    false,
                )
                .on_hover(hover_sub(None))
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.toggle_pin(&pin_id, cx);
                    w.close_doc_menu(cx);
                })),
            )
            .child(
                menu_item("dm-star", if starred { "star-off" } else { "star" }, if starred { crate::settings::text("取消收藏") } else { crate::settings::text("添加到收藏") }, false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| w.toggle_star(&star_id, cx))),
            )
            .child(divider())
            .when(child, |d| d.child(
                menu_item("dm-child", "plus", crate::settings::tr("新建子文档", "New child page"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| { w.close_doc_menu(cx); w.new_child(&child_id, window, cx); })),
            ))
            .child(
                menu_item("dm-copy", "copy", crate::settings::text("创建副本"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| w.duplicate_doc(&dup_id, window, cx))),
            )
            .child(
                menu_item("dm-download", "export", crate::settings::text("下载为"), true, sub == Some(DocSub::Download))
                    .on_hover(hover_sub(Some(DocSub::Download))),
            )
            .when(!board_file, |d| d.child(
                menu_item("dm-copy-md", "doc-md", crate::settings::text("复制为 Markdown"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.close_doc_menu(cx);
                        if let Some(ed) = w.editor.clone() {
                            ed.update(cx, |e, cx| e.copy_markdown(cx));
                            w.show_toast(crate::settings::text("已复制为 Markdown"), cx);
                        }
                    })),
            ))
            .child(divider())
            .when(crate::printing::SUPPORTED, |d| d.child(
                menu_item("dm-print", "printer", crate::settings::text("打印"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| w.print_doc(&print_id, window, cx))),
            ).child(divider()))
            .child(
                menu_item("dm-info", "info", crate::settings::text("文档信息"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, window, cx| w.open_doc_info(window, cx))),
            )
            .child(
                menu_item("dm-history", "restore", crate::settings::tr("版本历史", "Version history"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, window, cx| {
                        w.close_doc_menu(cx);
                        if let Some(id) = w.current.clone() { w.open_history(&id, window, cx) }
                    })),
            )
            .child(divider())
            // (a board file has no reading mode or references)
            .when(!board_file, |d| d.child(
                menu_item("dm-read", "eye-open", crate::settings::text("阅读模式"), false, false)
                    .child(switch(reading))
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| {
                        w.close_doc_menu(cx);
                        w.set_mode(!reading, window, cx);
                    })),
            )
            .child(
                menu_item("dm-refs", "list", crate::settings::text("显示引用列表"), false, false)
                    .child(switch(refs_shown))
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        w.close_doc_menu(cx);
                        w.ask_refs(!refs_shown, cx);
                    })),
            )
            .child(divider()))
            .child(
                menu_item("dm-delete", "bin", crate::settings::text("删除"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| w.ask_delete(&del_id, cx))),
            )
            // (a board has its own shortcut help, `?`)
            .when(!board, |d| d.child(divider())
            .child(
                menu_item("dm-shortcuts", "keyboard", crate::settings::text("快捷键"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.close_doc_menu(cx);
                        w.shortcuts = true;
                        cx.notify();
                    })),
            ));
        let vw = window.viewport_size().width;
        let btn = self.more_btn_bounds.get();
        let right = btn.map_or(vw - px(120.), |b| b.right());
        let menu_top = btn.map_or(px(60.), |b| b.bottom() + px(MENU_GAP));
        let mut out = vec![
            deferred(anchored().anchor(Anchor::TopRight).position(point(right, menu_top)).child(menu))
                .with_priority(2)
                .into_any_element(),
        ];
        // Submenus open to the left, 4px from the menu, level with their item.
        let sub_at = point(right - px(self.doc_menu_w.get().unwrap_or(MENU_W) + 4.), px(0.));
        let item_top = |k: usize, d: usize| f32::from(menu_top) + item_top(k, d);
        let record = self.doc_sub_bounds.clone();
        record.set(None);
        let recorder = move || {
            let record = record.clone();
            canvas(move |b, _, _| record.set(Some(b)), |_, _, _, _| {}).absolute().inset_0()
        };
        // the items and dividers above a submenu's item that aren't shown
        let hidden = if board { (2, 2) } else { (0, 0) };
        let sub_el = match sub {
            Some(DocSub::Width) => Some((item_top(0, 0) - 1. - PAD, self.width_panel(width, cx).child(recorder()).into_any_element())),
            // Level the submenu's first item with 下载为.
            Some(DocSub::Download) => Some((
                item_top(6 - hidden.0 + usize::from(child), 3 - hidden.1) - 1. - PAD,
                {
                    let pdf = id.clone();
                    popover()
                        .id("dm-download-menu")
                        .occlude()
                        .min_w(px(130.))
                        .px(px(5.))
                        .py(px(PAD))
                        .child(recorder())
                        .when(crate::printing::SUPPORTED, |d| d.child(sub_item("dm-pdf", "PDF").on_click(cx.listener(move |w, _, window, cx| w.export_pdf(&pdf, window, cx)))))
                        .child(sub_item("dm-md", "Markdown").on_click(cx.listener(move |w, _, _, cx| w.open_export_md(&id, cx))))
                        .into_any_element()
                },
            )),
            None => None,
        };
        if let Some((top, el)) = sub_el {
            out.push(
                deferred(anchored().anchor(Anchor::TopRight).position(point(sub_at.x, px(top))).snap_to_window_with_margin(px(8.)).child(el))
                    .with_priority(2)
                    .into_any_element(),
            );
        }
        out
    }

    /// 为当前窗口选择合适页宽: 默认 / 较宽 / 全宽 cards.
    fn width_panel(&self, current: PageWidth, cx: &mut Context<Self>) -> Stateful<Div> {
        let card = |id: &'static str, label: &'static str, w: PageWidth, line: f32| {
            let on = w == current;
            let line_color = if on { Theme::primary() } else { theme::c(craft::N400) };
            div()
                .id(id)
                .w(px(56.))
                .h(px(66.))
                .p(px(4.))
                .flex()
                .flex_col()
                .items_center()
                .rounded(px(9.))
                .border_1()
                .border_color(if on { Theme::primary() } else { Theme::line_border() })
                .bg(crate::theme::raised(Theme::bg_body()))
                .cursor_pointer()
                .when(!on, |d| d.hover(|s| s.border_color(Theme::primary())))
                .child(
                    div()
                        .w(px(46.))
                        .h(px(28.))
                        .pt(px(7.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(2.))
                        .rounded(px(2.))
                        .bg(if on { theme::c(craft::B100) } else { theme::c(craft::N100) })
                        .children((0..4).map(move |_| div().w(px(line)).h(px(2.)).flex_none().bg(line_color))),
                )
                .child(
                    div()
                        .mt(px(6.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(if on { Theme::primary() } else { Theme::text() })
                        .child(crate::settings::label(label)),
                )
                .on_click(cx.listener(move |ws, _, _, cx| ws.set_page_width(w, cx)))
        };
        popover()
            .id("dm-width-panel")
            .occlude()
            .relative()
            .w(px(238.))
            .h(px(136.))
            .child(
                div()
                    .absolute()
                    .left(px(19.))
                    .top(px(14.))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(Theme::text_caption())
                    .child(crate::settings::text("为当前窗口选择合适页宽")),
            )
            .child(
                div()
                    .absolute()
                    .left(px(19.))
                    .top(px(47.))
                    .flex()
                    .gap(px(16.))
                    .child(card("pw-standard", crate::settings::text("默认"), PageWidth::Standard, 20.))
                    .child(card("pw-wide", crate::settings::text("较宽"), PageWidth::Wide, 28.))
                    .child(card("pw-full", crate::settings::text("全宽"), PageWidth::Full, 38.)),
            )
    }
}

/// What 转换为 Markdown would drop, as the confirmation lists it.
pub(crate) fn loss_label(l: crate::markdown::Loss) -> &'static str {
    use crate::markdown::Loss::*;
    use crate::settings::tr;
    match l {
        IconCover => tr("文档图标和封面", "document icon and cover"),
        Colors => tr("文字颜色和背景色", "text and background colors"),
        Underline => tr("下划线", "underline"),
        LineBreaks => tr("段内换行", "line breaks within a paragraph"),
        Align => tr("对齐方式", "alignment"),
        Indent => tr("缩进", "indentation"),
        HeadingNumbers => tr("标题自动编号", "heading numbering"),
        DeepHeadings => tr("七到九级标题", "headings below level 6"),
        CodeOptions => tr("代码块名称和自动换行", "code block names and wrapping"),
        Callout => tr("高亮块", "callouts"),
        Grid => tr("分栏", "columns"),
        Table => tr("表格的合并单元格、单元格背景和格内排版", "merged cells, cell colors and layout inside table cells"),
        ImageSize => tr("图片尺寸和裁剪", "image sizes and cropping"),
        Files => tr("文件和视频卡片", "file and video cards"),
        Objects => tr("提及、日期提醒和按钮", "mentions, date reminders and buttons"),
        Board => tr("画板", "whiteboards"),
        DataTable => tr("数据表", "data tables"),
        Countdown => tr("倒计时", "countdowns"),
        SubPages => tr("子页面列表", "subpage lists"),
        Unknown => tr("无法识别的内容（如书签、电子表格）", "content Quiet.md doesn't recognize (bookmarks, sheets …)"),
        FrontMatter => tr("文件开头的 YAML 信息（front matter）", "the YAML front matter at the top of the file"),
    }
}
