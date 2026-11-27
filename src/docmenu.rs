//! The header "…" menu: how the document is seen (page width, then the 阅读模式 and 打字机模式
//! switches, 专注模式, then 显示引用列表 and 窗口保持在最前), find & replace, move, pin, 新建子文档, 在新窗口中打开, duplicate,
//! 下载为 PDF / Markdown, 复制为 Markdown, 打印, document info, 版本历史, delete, 快捷键.

use crate::app::Workspace;
use crate::settings::display_title;
use crate::store::PageWidth;
use crate::design::{MenuItem, MenuSeparator, Popover, menu};
use crate::ui::TipExt;
use gpui::{prelude::FluentBuilder, *};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DocSub {
    Width,
    Download,
}

const MENU_W: f32 = crate::design::menu_w::MD;
/// The menu's padding (`design::menu`): a submenu's first item sits this far below its top.
const PAD: f32 = menu::PAD;

thread_local! {
    /// Where the rows with a submenu were last painted, so the submenu levels with its row wherever
    /// the row is: no counting rows and dividers above it, and right after the menu scrolls.
    static ROWS: std::cell::RefCell<std::collections::HashMap<&'static str, Bounds<Pixels>>> = Default::default();
}

/// What a page's 「…」 holds, by what the page is: each kind its own list, nothing in it that the
/// page can't do. How the page is seen comes first (页宽, then the 阅读模式, 显示引用列表 and
/// 窗口保持在最前 switches: the user's order, 2026-11-25; 打字机模式 and 专注模式, a document's only, after 阅读模式),
/// then what is done to it.
#[derive(Clone, Copy, PartialEq)]
enum Item { Width, Read, Typewriter, Dim, Focus, Refs, OnTop, Find, Move, Pin, Child, Window, Copy, Download, CopyMd, Print, Info, History, Delete, Shortcuts, Rule }

/// The page open: a document, one with a 画板 opened in its place, a 画板 file, a 数据库 file.
#[derive(Clone, Copy, PartialEq)]
enum Page { Doc, Board, BoardFile, Table }

fn items(page: Page, child: bool) -> Vec<Item> {
    use Item::*;
    let print: &[Item] = if crate::printing::SUPPORTED { &[Print, Rule] } else { &[] };
    let child: &[Item] = if child { &[Child] } else { &[] };
    let top: &[Item] = if crate::on_top::SUPPORTED { &[OnTop] } else { &[] };
    let top_rule: &[Item] = if crate::on_top::SUPPORTED { &[OnTop, Rule] } else { &[] };
    let list: Vec<&[Item]> = match page {
        // 在新窗口中打开 where the sidebar row's menu has it: after 新建子文档, before 创建副本
        Page::Doc => vec![&[Width, Read, Typewriter, Dim, Focus, Refs], top, &[Rule, Find, Rule, Move, Pin, Rule], child, &[Window, Copy, Download, CopyMd, Rule], print, &[Info, History, Rule, Delete, Rule, Shortcuts]],
        // a board isn't the page: no page width, no find in its text, its own shortcut help (`?`)
        Page::Board => vec![&[Read, Refs], top, &[Rule, Move, Pin, Rule], child, &[Window, Copy, Download, CopyMd, Rule], print, &[Info, History, Rule, Delete]],
        // a board file: no Markdown, no reading mode or references either
        Page::BoardFile => vec![top_rule, &[Move, Pin, Rule, Window, Copy, Download, Rule], print, &[Info, History, Rule, Delete]],
        // a 数据库 file: the file's own things; finding, exporting and copying are the table's (its bar)
        Page::Table => vec![top_rule, &[Move, Pin, Rule, Window, Copy, Rule, History, Rule, Delete]],
    };
    list.concat()
}

/// Records where the row `key` is painted (a submenu's row).
fn record_row(key: &'static str) -> impl IntoElement {
    canvas(move |b, _, _| ROWS.with(|m| { m.borrow_mut().insert(key, b); }), |_, _, _, _| {}).absolute().inset_0()
}

/// The top of the row `key` as last painted.
fn row_top(key: &'static str) -> Option<f32> { ROWS.with(|m| m.borrow().get(key).map(|b| f32::from(b.top()))) }

/// A row; one whose submenu is open stays lit (`open`).
fn menu_item(id: &'static str, ic: &'static str, label: impl Into<SharedString>, sub: bool, open: bool) -> MenuItem {
    MenuItem::new(id).icon(ic).label(crate::settings::label(label.into())).submenu(sub).open(open)
}

/// 下载为's text-only items.
pub(crate) fn sub_item(id: &'static str, label: &'static str) -> MenuItem {
    MenuItem::new(id).label(crate::settings::label(label))
}

fn divider() -> MenuSeparator { MenuSeparator::new() }

impl Workspace {
    fn close_doc_menu(&mut self, cx: &mut Context<Self>) {
        self.doc_menu = false;
        self.doc_sub = None;
        cx.notify();
    }

    pub fn set_page_width(&mut self, width: PageWidth, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        if let Some(e) = self.entries.edit().iter_mut().find(|e| e.id == id) {
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

    /// 创建副本: "<标题> 副本", opened once it is written.
    pub fn duplicate_doc(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.close_doc_menu(cx);
        self.row_menu = None;
        self.copy_docs(&[id.to_string()], true, crate::settings::tr("创建副本", "Duplicate"), window, cx);
    }

    /// 创建副本 of each of `ids` ("<标题> 副本", beside it; a folder with
    /// everything in it, `Store::duplicate`), written off the window's thread
    /// as 删除 is: the copies show when they are on disk, from the list read
    /// back, one step of 撤销 (`label`) for them all. 上层优先: what a picked
    /// folder holds is copied once, with it. `open`: the copy is opened then.
    pub(crate) fn copy_docs(&mut self, ids: &[String], open: bool, label: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if self.store.file_task_busy() || self.file_history.busy { return; }
        let later = ids.to_vec();
        if self.after_tree_writes(move |w, window, cx| w.copy_docs(&later, open, label, window, cx)) { return; }
        // What is copied is what is on disk: the open document and the 数据库 are written first.
        crate::table::flush_all(cx);
        let retry = ids.to_vec();
        if !self.save_before(cx, move |w, window, cx| w.copy_docs(&retry, open, label, window, cx)) { return; }
        let ids = crate::tree::Tree::new(&self.entries).outermost(ids);
        let current = self.current.clone().zip(self.editor.as_ref().map(|e| e.read(cx).core.doc.title.text.clone()));
        let mut writers: Vec<(String, u64, crate::store::Store, Vec<(String, String)>)> = vec![];
        for id in ids.iter().filter(|id| crate::shared_store::top_of_row(id).is_none()) {
            // the editor's title for the open document: the list may not have caught up with it
            let title = match &current { Some((c, t)) if c == id => t.clone(), _ => self.entries.iter().find(|e| &e.id == id).map(|e| e.title.clone()).unwrap_or_default() };
            let job = (id.clone(), crate::localized_format!("{} 副本", "{} copy", display_title(&title)));
            let Some(key) = self.store.top_of(id) else { continue };
            match writers.iter_mut().find(|(k, _, _, _)| *k == key) {
                Some((_, _, _, v)) => v.push(job),
                // with the window's count of writes: the window's library takes the writer over afterwards
                None => if let Some((k, n, w)) = self.store.writers(std::slice::from_ref(&key)).pop() { writers.push((k, n, w, vec![job])); },
            }
        }
        if writers.is_empty() { return; }
        let Some(task_guard) = self.store.begin_file_task() else { return };
        let keys = self.sidebar_keys(window);
        let task = cx.background_executor().spawn(async move {
            let (mut made, mut errors, mut changes, mut listed) = (vec![], vec![], vec![], vec![]);
            for (key, n, mut writer, jobs) in writers {
                let journal = writer.track_file_changes();
                for (id, title) in jobs {
                    match writer.duplicate(&id, &title) {
                        Ok(new) => made.push(new),
                        Err(e) => errors.push(crate::localized_format!("创建副本失败：{}", "Could not duplicate document: {}", crate::settings::diagnostic(&e.to_string()))),
                    }
                }
                errors.extend(writer.take_warnings().iter().map(|s| crate::settings::diagnostic(s)));
                let list = writer.list();
                changes.extend(journal.take());
                listed.push((key, n, writer, list));
            }
            (made, errors, changes, listed)
        });
        self.batch = None;
        self.row_menu = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let (made, errors, changes, listed) = task.await;
            let _ = this.update_in(cx, move |w, window, cx| {
                let mut entries = None;
                // the window's library takes the writer over (as a move's or an undo's): the copy opens
                // from its row at once, not only after the library is next read
                for (key, n, writer, list) in listed { entries = Some(w.store.adopt_written(&key, n, writer, list)); }
                if let Some(entries) = entries {
                    w.sync_peers(cx, &entries, None);
                    w.entries.set(entries);
                    w.sort();
                }
                w.remember_file_changes(changes, label);
                w.sync_sub_pages(cx);
                if !errors.is_empty() {
                    let msg = errors.join("；");
                    w.show_toast_kind(crate::ui::ToastKind::Error, &msg, cx);
                    w.status = crate::app::SaveStatus::Failed(msg);
                }
                // the copy is on disk and the window's: the task is over, so opening it (`save_before`) isn't held back
                drop(task_guard);
                if open && let [new] = &made[..] { w.open_beside(new, keys, window, cx); }
                cx.notify();
            });
        }).detach();
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

    pub(crate) fn render_doc_menu(&self, _window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.doc_menu {
            return vec![];
        }
        let Some(id) = self.current.clone() else { return vec![] };
        let entry = self.entries.iter().find(|e| e.id == id);
        let pinned = entry.is_some_and(|e| e.meta.pinned);
        let width = entry.map(|e| e.meta.width).unwrap_or_default();
        let refs_shown = !entry.is_some_and(|e| e.meta.refs_hidden);
        // which list the 「…」 shows (`items`)
        let page = self.editor.as_ref().map_or(Page::Doc, |e| { let e = e.read(cx); if e.table_file { Page::Table } else if e.board_file { Page::BoardFile } else if e.board_open() { Page::Board } else { Page::Doc } });
        let reading = self.reading();
        let typewriter = crate::settings::typewriter();
        let dim = crate::settings::paragraph_focus();
        let on_top = self.on_top;
        let focus = self.focus_mode.switch();
        let sub = self.doc_sub;
        let hover_sub = |s: Option<DocSub>| {
            crate::design::sub_hover(cx.listener(move |w: &mut Workspace, hovered: &bool, _, cx| {
                if *hovered && w.doc_sub != s {
                    w.doc_sub = s;
                    cx.notify();
                }
            }))
        };
        let sub_bounds = self.doc_sub_bounds.clone();
        let (pin_id, mv_id, del_id, dup_id, print_id) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
        // 新建子文档 (#357): under a document of the library.
        let child = matches!(page, Page::Doc | Page::Board) && self.takes_children(&id);
        let list = items(page, child);
        let child_id = id.clone();
        let window_id = id.clone();
        let mut menu = Popover::new("doc-menu")
            .menu()
            .w(px(MENU_W))
            .on_mouse_down_out(cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                if sub_bounds.get().is_none_or(|b| !b.contains(&ev.position)) {
                    w.close_doc_menu(cx);
                }
            }));
        for item in &list {
            let (pin_id, mv_id, del_id, dup_id, print_id, child_id, window_id) = (pin_id.clone(), mv_id.clone(), del_id.clone(), dup_id.clone(), print_id.clone(), child_id.clone(), window_id.clone());
            menu = menu.child(match item {
                Item::Rule => divider().into_any_element(),
                Item::Width => menu_item("dm-width", "page-width", crate::settings::text("页宽设置"), true, sub == Some(DocSub::Width))
                    .child(record_row("dm-width"))
                    .on_hover(hover_sub(Some(DocSub::Width))).into_any_element(),
                // switch rows keep the menu open (`MenuItem::switch`): the click flips, nothing else
                Item::Read => menu_item("dm-read", "eye-open", crate::settings::text("阅读模式"), false, false)
                    .switch(reading)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| w.set_mode(!reading, window, cx))).into_any_element(),
                Item::Typewriter => menu_item("dm-typewriter", crate::typewriter::ICON, crate::settings::tr("打字机模式", "Typewriter Mode"), false, false)
                    .switch(typewriter)
                    .tip_right(crate::settings::tr("正在写的一行停在窗口中间", "Keeps the line you write in the middle"), Some(crate::shortcuts::hint("workspace::ToggleTypewriter", "⌘ + Shift + T")))
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| w.set_typewriter(!typewriter, false, cx))).into_any_element(),
                Item::Dim => menu_item("dm-dim", crate::typewriter::FOCUS_ICON, crate::settings::tr("段落聚焦", "Paragraph Focus"), false, false)
                    .switch(dim)
                    .tip_right(crate::settings::tr("正在写的一段保持清晰，其余变淡", "The paragraph you write stays clear, the rest fade"), Some(crate::shortcuts::hint("workspace::ToggleParagraphFocus", "⌘ + Shift + F")))
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| w.set_paragraph_focus(!dim, false, cx))).into_any_element(),
                Item::Refs => menu_item("dm-refs", "list", crate::settings::text("显示引用列表"), false, false)
                    .switch(refs_shown)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| w.ask_refs(!refs_shown, cx))).into_any_element(),
                Item::OnTop => menu_item("dm-on-top", crate::on_top::ICON, crate::settings::tr("窗口保持在最前", "Keep Window on Top"), false, false)
                    .switch(on_top)
                    .tip_right(crate::settings::tr("窗口会一直显示在其他应用的窗口之上", "Stays above other apps' windows"), None)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| w.set_on_top(!on_top, window, cx))).into_any_element(),
                Item::Focus => menu_item("dm-focus", crate::focus::ICON, crate::settings::tr("专注模式", "Focus Mode"), false, false)
                    .switch(focus)
                    .tip_right(crate::settings::tr("只留正文，鼠标移到顶部可退出", "Just the page; point at the top to exit"), Some(crate::focus::hint()))
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, _, cx| w.focus_switch(cx))).into_any_element(),
                Item::Find => menu_item("dm-find", "text-search", crate::settings::text("查找和替换"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, window, cx| w.find_in_doc(false, window, cx))).into_any_element(),
                Item::Move => menu_item("dm-move", "folder-input", crate::settings::text("移动到"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| {
                        w.close_doc_menu(cx);
                        w.open_move(vec![mv_id.clone()], window, cx);
                    })).into_any_element(),
                Item::Pin => menu_item("dm-pin", if pinned { "pin-off" } else { "pin" }, if pinned { crate::settings::text("从“置顶”移除") } else { crate::settings::text("添加到“置顶”") }, false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| {
                        w.toggle_pin(&pin_id, cx);
                        w.close_doc_menu(cx);
                    })).into_any_element(),
                Item::Child => menu_item("dm-child", "plus", crate::settings::tr("新建子文档", "New child page"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| { w.close_doc_menu(cx); w.new_child(&child_id, window, cx); })).into_any_element(),
                // as the sidebar row's: from a main window a window of its own, sized and with the sidebar as
                // this one; from a document's own window, the window showing it comes forward (`show_document`)
                Item::Window => menu_item("dm-window", "copy", crate::settings::text("在新窗口中打开"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| {
                        w.close_doc_menu(cx);
                        let (id, opener, main) = (window_id.clone(), w.opener(window), w.frame == crate::quick_note::Frame::Main);
                        cx.defer(move |cx| if main { crate::quick_note::open_document(Some(id), opener, cx) } else { crate::quick_note::show_document(id, None, cx) });
                    })).into_any_element(),
                Item::Copy => menu_item("dm-copy", "copy", crate::settings::text("创建副本"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| w.duplicate_doc(&dup_id, window, cx))).into_any_element(),
                Item::Download => menu_item("dm-download", "export", crate::settings::text("下载为"), true, sub == Some(DocSub::Download))
                    .child(record_row("dm-download"))
                    .on_hover(hover_sub(Some(DocSub::Download))).into_any_element(),
                Item::CopyMd => menu_item("dm-copy-md", "doc-md", crate::settings::text("复制为 Markdown"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.close_doc_menu(cx);
                        if let Some(ed) = w.editor.clone() {
                            ed.update(cx, |e, cx| e.copy_markdown(cx));
                            w.show_toast(crate::settings::text("已复制为 Markdown"), cx);
                        }
                    })).into_any_element(),
                Item::Print => menu_item("dm-print", "printer", crate::settings::text("打印"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, window, cx| w.print_doc(&print_id, window, cx))).into_any_element(),
                Item::Info => menu_item("dm-info", "info", crate::settings::text("文档信息"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, window, cx| w.open_doc_info(window, cx))).into_any_element(),
                Item::History => menu_item("dm-history", "restore", crate::settings::tr("版本历史", "Version history"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, window, cx| {
                        w.close_doc_menu(cx);
                        if let Some(id) = w.current.clone() { w.open_history(&id, window, cx) }
                    })).into_any_element(),
                Item::Delete => menu_item("dm-delete", "bin", crate::settings::text("删除"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(move |w, _, _, cx| w.ask_delete(&del_id, cx))).into_any_element(),
                Item::Shortcuts => menu_item("dm-shortcuts", "keyboard", crate::settings::text("快捷键"), false, false)
                    .on_hover(hover_sub(None))
                    .on_click(cx.listener(|w, _, _, cx| {
                        w.close_doc_menu(cx);
                        w.shortcuts = true;
                        cx.notify();
                    })).into_any_element(),
            });
        }
        // hung from the measured button; not yet measured: not drawn yet
        let Some(b) = self.more_btn_bounds.get() else { return vec![] };
        let (at, _) = crate::design::hang(b, crate::design::Align::End, None);
        let (right, menu_top) = (at.x, at.y);
        let mut out = vec![
            deferred(anchored().anchor(Anchor::TopRight).position(point(right, menu_top)).child(menu.fit(crate::design::room(point(right, menu_top), Anchor::TopRight))))
                .with_priority(2)
                .into_any_element(),
        ];
        // Submenus open to the left, 4px from the menu, level with their item.
        let sub_at = point(right - px(MENU_W + 4.), px(0.));
        let record = self.doc_sub_bounds.clone();
        record.set(None);
        let recorder = move || {
            let record = record.clone();
            canvas(move |b, _, _| record.set(Some(b)), |_, _, _, _| {}).absolute().inset_0()
        };
        let sub_el = match sub {
            // each levels its first item with its row, where the row was last painted
            Some(DocSub::Width) => row_top("dm-width").map(|top| (top - PAD, self.width_panel(width, cx).submenu().child(recorder()).into_any_element())),
            // Level the submenu's first item with 下载为.
            Some(DocSub::Download) => row_top("dm-download").map(|top| (
                top - PAD,
                {
                    let pdf = id.clone();
                    Popover::new("dm-download-menu")
                        .menu()
                        .submenu()
                        .w(px(crate::design::menu_w::SM))
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

    /// 为当前窗口选择合适页宽: 默认 / 较宽 / 全宽 cards. The panel takes its width from its
    /// contents (the caption, or three cards each as wide as its longest label) so no
    /// language wraps; the cards share it equally.
    fn width_panel(&self, current: PageWidth, cx: &mut Context<Self>) -> Popover {
        use crate::design::{color, geo};
        // A choice card: the focus ring round the picked one, `accent` under the pointer.
        let card = |id: &'static str, label: &'static str, w: PageWidth, line: f32| {
            let on = w == current;
            let line_color = if on { color::foreground() } else { color::muted_foreground() };
            div()
                .id(id)
                .flex_1()
                .min_w(px(64.))
                .h(px(66.))
                .px(px(8.))
                .pt(px(4.))
                .flex()
                .flex_col()
                .items_center()
                .rounded(px(geo().button_r))
                .border_1()
                // the picked card wears the focus ring (shadcn: a `ring` edge and a 3pt `ring/50`
                // halo), as a field with the keyboard in it does; not a thin near-black line
                .border_color(if on { color::ring() } else { color::border() })
                .when(on, |d| d.shadow(vec![crate::design::shadow::ring(color::focus_ring(), 3.)]))
                .bg(color::popover())
                .cursor_pointer()
                .when(!on, |d| d.hover(|s| s.bg(color::accent())))
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
                        .bg(color::muted())
                        .children((0..4).map(move |_| div().w(px(line)).h(px(2.)).flex_none().bg(line_color))),
                )
                .child(
                    div()
                        .mt(px(6.))
                        .whitespace_nowrap()
                        .text_size(px(geo().text_xs))
                        .line_height(px(18.))
                        .text_color(if on { color::foreground() } else { color::muted_foreground() })
                        .child(crate::settings::label(label)),
                )
                .on_click(cx.listener(move |ws, _, _, cx| ws.set_page_width(w, cx)))
        };
        Popover::new("dm-width-panel")
            .relative()
            .min_w(px(238.))
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .whitespace_nowrap()
                    .text_size(px(geo().text_xs))
                    .line_height(px(18.))
                    .text_color(color::muted_foreground())
                    .child(crate::settings::text("为当前窗口选择合适页宽")),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .gap(px(12.))
                    .child(card("pw-standard", crate::settings::text("默认"), PageWidth::Standard, 20.))
                    .child(card("pw-wide", crate::settings::text("较宽"), PageWidth::Wide, 28.))
                    .child(card("pw-full", crate::settings::text("全宽"), PageWidth::Full, 38.)),
            )
    }
}
