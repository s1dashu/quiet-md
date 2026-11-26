//! A folder page's header: one row — the path on the left, four glyphs on the right
//! ("+" · 显示 · 搜索 · …): the two used most at the ends, "+" first and "…" last (the
//! maintainer, 2026-11-26). Counted from the right "…" and 搜索 sit where a document's do.

use super::{PageSort, SortDir};
use crate::app::Workspace;
use crate::design::{Align, Button, ButtonSize, MenuItem, MenuLabel, MenuSeparator, Popover, hang};
use crate::settings::tr;
use crate::theme::Theme;
use crate::ui::{TipExt, TitleBarExt};
use gpui::{prelude::FluentBuilder, *};

/// The `RowMenu::from` of a folder page's header "…".
pub(crate) const TB_MORE: &str = "tb-more";

impl Workspace {
    fn main_w(&self, window: &Window) -> f32 {
        f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. }
    }

    /// Folder `folder`'s header; in 批量管理 the whole row is its toolbar. Its "…" and "+" are its
    /// sidebar row's: what acts on the folder, and 新建 with 导入 at its end.
    pub(crate) fn topbar(&self, folder: &str, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let main = self.main_w(window);
        if let Some(sel) = &self.batch { return self.batch_bar(folder, sel.len(), main, cx) }
        let left = div().relative().min_w_0().overflow_hidden().flex().items_center().child(self.folder_crumbs(folder, main < 640., cx));
        let icon_btn = crate::ui::header_icon;
        // Menu state at render: a popover's outside press clears it on mouse-down, before a click would fire.
        let view_open = self.folder_page.view_menu;
        let search = icon_btn("tb-search", "search")
            .tip(tr("搜索文档", "Search documents"), Some(crate::shortcuts::hint("workspace::FocusSearch", "⌘ + J")))
            .on_click(cx.listener(|w, _, window, cx| w.open_search(window, cx)));
        let display = icon_btn("page-view", if self.folder_page.grid { "view-grid" } else { "view-list" }).relative().selected(view_open).child(self.folder_page.track("page-view"))
            .when(!view_open, |d| d.tip(tr("显示", "View"), None))
            .on_mouse_down(MouseButton::Left, cx.listener(move |w, _, _, cx| { cx.stop_propagation(); w.folder_page.view_menu = !view_open; cx.notify(); }));
        // the folder's "…": its sidebar row's menu (`folder_row_menu`), 批量管理 first
        let more = {
            let (id, open) = (folder.to_string(), self.row_menu.as_ref().is_some_and(|m| m.from.as_deref() == Some(TB_MORE)));
            icon_btn("tb-more", "ellipsis").relative().selected(open).child(self.folder_page.track("tb-more"))
                .when(!open, |d| d.tip(tr("更多", "More"), None))
                .on_mouse_down(MouseButton::Left, cx.listener(move |w, _, _, cx| {
                    cx.stop_propagation();
                    let Some(b) = w.folder_page.anchor("tb-more") else { return };
                    let (pos, corner) = hang(b, Align::End, None);
                    w.row_menu = (!open).then(|| crate::app::RowMenu { id: id.clone(), pos, corner, sub: false, batch: false, from: Some(TB_MORE.into()) });
                    cx.notify();
                }))
        };
        // its "+", last and in sight: the 新建 menu every "+" opens, 导入 at its end (`render_create_menu`)
        let primary = {
            let parent = Some(folder.to_string());
            let open = self.create_menu.as_ref().is_some_and(|(_, pos)| self.folder_page.anchor("create-folder").is_some_and(|b| hang(b, Align::End, None).0 == *pos));
            icon_btn("folder-new", "plus").relative().selected(open).child(self.folder_page.track("create-folder"))
                .when(!open, |d| d.tip(tr("新建", "New"), Some(crate::shortcuts::hint("workspace::NewDoc", "⌘ + N"))))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |w, _: &ClickEvent, _, cx| { let Some(b) = w.folder_page.anchor("create-folder") else { return }; let pos = hang(b, Align::End, None).0; w.create_menu = if w.create_menu.is_some() { None } else { Some((parent.clone(), pos)) }; cx.notify(); }))
        };
        let right = div().relative().flex_none().flex().items_center().gap(px(crate::app::HEADER_ACTIONS_GAP))
            .child(primary).child(display).child(search).child(more).child(self.folder_page.track("tb-right"));
        crate::trash::page_header_with(self, left, 18., cx).child(div().w(px(16.)).flex_none()).child(right).into_any_element()
    }

    /// 批量管理's row: 全选 and the count where the path was, the actions where the controls were.
    fn batch_bar(&self, folder: &str, n: usize, main: f32, cx: &mut Context<Self>) -> AnyElement {
        use crate::sidebar::{BATCH_LIMIT, Check, checkbox};
        let ids: Vec<String> = self.level_entries(Some(folder)).iter().take(BATCH_LIMIT).map(|e| e.id.clone()).collect();
        let all = n > 0 && ids.iter().all(|id| self.batch.as_ref().is_some_and(|s| s.contains(id)));
        let compact = main < 640.;
        let left = div().flex().items_center().min_w_0()
            .child(div().id("page-batch-all").no_drag().flex_none().ml(px(4.)).size(px(16.)).flex().items_center().justify_center().cursor_pointer().group("page-batch-all")
                .child(checkbox(if all { Check::On } else if n > 0 { Check::Partial } else { Check::Off }, false))
                .tip(crate::settings::text("全选"), None)
                .on_click(cx.listener(move |w, _, _, cx| { w.batch = Some(if all { vec![] } else { ids.clone() }); cx.notify(); })))
            .child(div().ml(px(8.)).text_size(px(15.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).whitespace_nowrap()
                .child(crate::localized_format!("已选 {n} 项", "Selected: {n}")));
        let act = |id: &'static str, ic: &'static str, label: &'static str| Button::ghost(id).icon(ic).no_drag().disabled(n == 0).relative().children(crate::devtools::anchor_probe(id))
            .when(!compact, |d| d.label(crate::settings::label(label))).when(compact, |d| d.size(px(crate::app::HEADER_ACTIONS_H)).tip(label, None));
        let right = div().flex_none().flex().items_center().gap(px(2.))
            .child(act("page-batch-copy", "copy", crate::settings::text("创建副本")).when(n > 0, |d| d.on_click(cx.listener(|w, _, window, cx| w.batch_copy(window, cx)))))
            .child(act("page-batch-move", "folder-input", crate::settings::text("移动到")).when(n > 0, |d| d.on_click(cx.listener(|w, _, window, cx| w.batch_move(window, cx)))))
            .child(act("page-batch-delete", "bin", crate::settings::text("删除")).when(n > 0, |d| d.on_click(cx.listener(|w, _, _, cx| w.ask_batch_delete(cx)))))
            .child(Button::secondary("page-batch-exit").sized(ButtonSize::Sm).no_drag().ml(px(6.)).label(tr("完成", "Done")).relative().child(self.folder_page.track("page-batch-exit"))
                .tip(crate::settings::text("退出批量操作"), None)
                .on_click(cx.listener(|w, _, _, cx| { w.batch = None; w.row_menu = None; cx.notify(); })));
        crate::trash::page_header_with(self, left, 18., cx).child(right).into_any_element()
    }

    /// The sort rows, shared by the sort dropdown and 显示: the orders, then 升序 / 降序 for
    /// the automatic ones, then 文件夹优先.
    pub(crate) fn sort_rows(&self, mut menu: Popover, cx: &mut Context<Self>) -> Popover {
        for (index, order) in [PageSort::Manual, PageSort::Visited, PageSort::Modified, PageSort::Created, PageSort::Title].into_iter().enumerate() {
            let g = SharedString::from(format!("page-order-{index}"));
            menu = menu.child(MenuItem::new(g).label(crate::settings::label(order.label())).checked(self.folder_page.sort == order)
                .on_click(cx.listener(move |w, _, _, cx| { w.folder_page.view_menu = false; w.sort_by(order, cx) })));
        }
        if self.folder_page.sort.directional() {
            menu = menu.child(MenuSeparator::new());
            for (index, dir) in [SortDir::Asc, SortDir::Desc].into_iter().enumerate() {
                menu = menu.child(MenuItem::new(SharedString::from(format!("page-dir-{index}"))).label(dir.label()).checked(self.folder_page.sort_dir == dir)
                    .on_click(cx.listener(move |w, _, _, cx| {
                        w.folder_page.sort_dir = dir;
                        w.save_page_view();
                       
                        w.folder_page.view_menu = false;
                        w.folder_page.folder_grid_scroll.scroll_to_item(0, ScrollStrategy::Top);
                        w.folder_page.folder_list_scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    })));
            }
        }
        menu.child(MenuSeparator::new()).child(MenuItem::new("page-folders-first").label(tr("文件夹优先", "Folders first")).checked(self.folder_page.folders_first && self.folder_page.sort != PageSort::Manual)
            .when(self.folder_page.sort == PageSort::Manual, |d| d.opacity(0.4))
            .on_click(cx.listener(|w, _, _, cx| { if w.folder_page.sort == PageSort::Manual { return; } w.folder_page.folders_first = !w.folder_page.folders_first; w.save_page_view(); w.folder_page.view_menu = false; cx.notify(); })))
    }

    /// 显示 (Finder's and Notion's view options): 视图 and 排序 in one menu.
    pub(crate) fn view_menu(&self, cx: &mut Context<Self>) -> Popover {
        let view = |id: &'static str, ic: &'static str, label: &'static str, grid: bool| MenuItem::new(id).icon(ic).label(label).checked(self.folder_page.grid == grid)
            .on_click(cx.listener(move |w, _, _, cx| { w.folder_page.grid = grid; w.save_page_view(); w.folder_page.view_menu = false; cx.notify(); }));
        let menu = Popover::new("view-menu").menu().w(px(crate::design::menu_w::SM))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.folder_page.view_menu = false; cx.notify(); }))
            .child(MenuLabel::new(tr("视图", "View")))
            .child(view("tb-grid", "view-grid", tr("宫格", "Grid"), true))
            .child(view("tb-list", "view-list", tr("列表", "List"), false))
            .child(MenuSeparator::new())
            .child(MenuLabel::new(tr("排序", "Sort by")));
        self.sort_rows(menu, cx)
    }
}
