use crate::app::{View, Workspace};
use crate::settings::{display_title, tr};
use crate::ui::{TipExt, TitleBarExt, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};

impl Workspace {
    /// 新建文档/Markdown under `parent` (None = top level), opened and focused.
    /// `markdown` picks the on-disk format; the choice is remembered for ⌘N.
    pub(crate) fn create_doc(&mut self, markdown: bool, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.create_menu = None;
        // A 快速笔记 window's own note is written as always; it needs no list.
        if !self.quick_note && self.still_loading(cx) { return }
        let (m, p) = (markdown, parent.clone());
        if !self.save_before(cx, move |w, window, cx| w.create_doc(m, p.clone(), window, cx)) { return; }
        let file_before = self.store.track_file_changes();
        let made = if parent.is_some() {
            // In a folder: written into that folder's top-level folder at once.
            let doc = crate::model::Document::default();
            if markdown { self.store.create_markdown_in(&doc, "", parent.as_deref()) } else { self.store.create_from_in(&doc, parent.as_deref()) }
        } else if markdown {
            self.store.create_markdown(&crate::model::Document::default(), "").and_then(|id| {
                if let Some(p) = &parent { self.store.set_parent(&id, Some(p))?; }
                Ok(id)
            })
        } else {
            self.store.create().and_then(|(id, _)| {
                if let Some(p) = &parent { self.store.set_parent(&id, Some(p))?; }
                Ok(id)
            })
        };
        match made {
            Ok(id) => {
                self.search_open = false;
                self.doc_info = None;
                self.entries.set(self.store.list());
                self.sort();
                if let Some(p) = &parent { self.expanded.insert(p.clone()); }
                self.search.update(cx, |s, cx| s.set_text("", cx));
                self.open(&id, window, cx);
                if let Some(ed) = &self.editor { { let f = ed.read(cx).focus.clone(); window.focus(&f, cx); } }
            }
            Err(e) => self.status = crate::app::SaveStatus::Failed(crate::localized_format!("新建失败：{}", "Could not create document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        self.set_pref(|p| p.last_new = if markdown { "markdown" } else { "doc" }.into(), cx);
        self.remember_files(file_before, crate::settings::tr(if markdown { "新建 Markdown" } else { "新建文档" }, if markdown { "New Markdown" } else { "New document" }));
        cx.notify();
    }

    /// 新建 › 画板: a standalone `.excalidraw` file in `parent`, opened at once.
    pub(crate) fn create_board(&mut self, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.create_menu = None;
        let p = parent.clone();
        if !self.save_before(cx, move |w, window, cx| w.create_board(p.clone(), window, cx)) { return; }
        let file_before = self.store.track_file_changes();
        match self.store.create_board_in(crate::settings::text("未命名画板"), &crate::whiteboard::Board::default(), parent.as_deref()) {
            Ok(id) => {
                self.search_open = false;
                self.doc_info = None;
                self.entries.set(self.store.list());
                self.sort();
                if let Some(p) = &parent { self.expanded.insert(p.clone()); }
                self.search.update(cx, |s, cx| s.set_text("", cx));
                self.open(&id, window, cx);
            }
            Err(e) => self.status = crate::app::SaveStatus::Failed(crate::localized_format!("新建失败：{}", "Could not create document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        self.remember_files(file_before, tr("新建画板", "New board"));
        cx.notify();
    }

    /// 新建 › 数据库: a standalone `.sqlite` file in `parent`, opened at once.
    pub(crate) fn create_table(&mut self, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.create_menu = None;
        let p = parent.clone();
        if !self.save_before(cx, move |w, window, cx| w.create_table(p.clone(), window, cx)) { return; }
        let file_before = self.store.track_file_changes();
        match self.store.create_table_in(crate::settings::text("未命名数据库"), &crate::table::blank_base(), parent.as_deref()) {
            Ok(id) => {
                self.search_open = false;
                self.doc_info = None;
                self.entries.set(self.store.list());
                self.sort();
                if let Some(p) = &parent { self.expanded.insert(p.clone()); }
                self.search.update(cx, |s, cx| s.set_text("", cx));
                self.open(&id, window, cx);
            }
            Err(e) => self.status = crate::app::SaveStatus::Failed(crate::localized_format!("新建失败：{}", "Could not create document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        self.remember_files(file_before, tr("新建数据库", "New database"));
        cx.notify();
    }

    pub(crate) fn creation_parent(&self) -> Option<String> {
        if self.view == View::Folder { self.folder_current.clone() }
        // Beside the open page; beside its folder when it is the folder's 正文 (#357).
        else if self.view == View::Doc { self.current.as_ref().map(|id| self.row_of(id)).and_then(|id| self.entries.iter().find(|e| e.id == id)).and_then(|e| e.meta.parent.clone()) }
        else { None }
    }

    pub(crate) fn open_folder(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // A folder with a 正文 has no page of its own: it is that document (#357).
        if let Some(note) = self.entries.iter().find(|e| e.id == id).and_then(|e| e.note.clone()) { return self.try_open(&note, window, cx); }
        let target = id.to_string();
        if !self.save_before(cx, move |w, window, cx| { w.open_folder(&target, window, cx); }) { return false; }
        self.editor = None;
        self.current = None;
        if self.folder_current.as_deref() != Some(id) { self.reset_folder_scroll(); }
        if let Some(meta) = { let _s = crate::devtools::span("folder-touch"); self.store.touch(id) } && let Some(entry) = self.entries.edit().iter_mut().find(|e| e.id == id) { entry.meta = meta; }
        self.schedule_view_state(cx);
        if self.manual_page_sort() { let _s = crate::devtools::span("manual-order"); self.prepare_manual_order(cx); }
        self.folder_current = Some(id.to_string());
        self.view = View::Folder;
        self.row_menu = None;
        self.create_menu = None;
        self.doc_menu = false;
        self.expanded.insert(id.to_string());
        self.reveal_in_tree(id);
        window.focus(&self.focus, cx);
        cx.notify();
        true
    }

    pub(crate) fn new_folder(&mut self, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.still_loading(cx) { return }
        let file_before = self.store.track_file_changes();
        self.create_menu = None;
        match self.store.create_folder(tr("未命名文件夹", "Untitled folder"), parent.as_deref()) {
            Ok(id) => {
                self.entries.set(self.store.list()); self.sort();
                if let Some(p) = parent { self.expanded.insert(p); }
                // Select and open the new folder (like 新建文档), then focus its name
                // field. open_folder focuses the pane, so start_rename must follow it.
                if self.open_folder(&id, window, cx) {
                    self.start_rename(&id, window, cx);
                }
            }
            Err(e) => self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx),
        }
        self.remember_files(file_before, crate::settings::tr("新建文件夹", "New folder"));
        cx.notify();
    }

    pub(crate) fn render_create_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (parent, pos) = self.create_menu.clone()?;
        // A header "+" sits at the right edge, so its menu hangs by its right edge.
        let header_new = self.folder_page.anchor("create-folder").is_some_and(|b| crate::design::hang(b, crate::design::Align::End, None).0 == pos);
        let corner = if header_new { Anchor::TopRight } else { Anchor::TopLeft };
        // Only a folder's "+" opens this menu: the sidebar's opens a folder straight away.
        let parent = Some(parent?);
        // Every 新建 menu ends with the 导入 rows, into the same place it makes things: the same
        // rows each time it opens, whichever "+" opened it.
        let import = self.import_items(parent.clone(), cx);
        Some(deferred(anchored().anchor(corner).position(pos).snap_to_window_with_margin(px(8.)).child(
            crate::design::Popover::new("create-items").menu().fit(crate::design::room(pos, corner)).w(px(crate::design::menu_w::MD))
                .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.create_menu = None; cx.notify(); }))
                .children(self.create_rows(parent.clone(), cx))
                .child(crate::design::MenuSeparator::new()).children(import)
        )).with_priority(3).into_any_element())
    }

    /// 新建's rows, making things in `parent`: the "+" menus, and the top of a folder page's header "…".
    pub(crate) fn create_rows(&self, parent: Option<String>, cx: &mut Context<Self>) -> Vec<crate::design::MenuItem> {
        // each kind its own solid, coloured mark (assets/illus/kinds), so a glance tells them apart
        let row = |id: &'static str, kind: &'static str, label: &'static str| crate::design::MenuItem::new(id).leading(img(format!("illus/kinds/{kind}.svg")).flex_none().size(px(18.))).label(label);
        let (doc_parent, md_parent, board_parent, table_parent, folder_parent) = (parent.clone(), parent.clone(), parent.clone(), parent.clone(), parent);
        let mut out = vec![
            row("create-document", "doc", tr("文档", "Document")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.create_doc(false, doc_parent.clone(), window, cx) })),
            row("create-markdown", "markdown", "Markdown").on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.create_doc(true, md_parent.clone(), window, cx) })),
        ];
        // 数据库 above 画板 (the maintainer, 2026-11-25: the more finished of the two first).
        out.push(row("create-table", "datatable", crate::settings::text("数据库")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.create_table(table_parent.clone(), window, cx) })));
        out.push(row("create-board", "board", crate::settings::text("画板")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.create_board(board_parent.clone(), window, cx) })));
        out.push(row("create-folder", "folder", tr("文件夹", "Folder")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.new_folder(folder_parent.clone(), window, cx) })));
        out
    }

    /// A folder's path from its top-level folder, as its page's header shows it.
    pub(crate) fn folder_crumbs(&self, id: &str, fold: bool, cx: &mut Context<Self>) -> Div {
        let id = id.to_string();
        let title = self.entries.iter().find(|e| e.id == id).map(|e| display_title(&e.title)).unwrap_or_else(|| tr("文件夹", "Folder").to_string());
        // 我的文档 › … › parent › this folder; a long path folds its middle into "…".
        let sep = || div().mx(px(3.)).flex_none().child(icon("chevron-right", 12., crate::ui::crumb_ink()));
        // A top-level folder's own row (two or more top-level folders) is the first crumb, by name.
        let path: Vec<_> = self.ancestors(&id).into_iter().filter(|e| crate::shared_store::top_of_row(&e.id).is_none()).collect();
        // a long path folds its middle into a "…" that opens the folded crumbs
        // `fold` (a tight header): every crumb between the top-level folder and this one folds.
        let (from, to) = if fold { (0, path.len()) } else { (1, path.len().saturating_sub(1)) };
        let folds = if fold { !path.is_empty() } else { path.len() > 2 };
        let mut more = folds.then(|| {
            let hidden: Vec<Crumb> = path[from..to].iter().map(|e| (display_title(&e.title), Some(e.id.clone()))).collect();
            self.crumb_more("folder-crumb-more", hidden, cx)
        });
        let link = |key: String, label: String, target: Option<String>| {
            div().id(SharedString::from(format!("folder-crumb-{key}"))).no_drag().flex_none().max_w(px(220.)).px(px(4.)).rounded(px(6.))
                .overflow_hidden().whitespace_nowrap().text_ellipsis().cursor_pointer()
                .text_color(crate::ui::crumb_ink()).hover(|s| s.text_color(crate::ui::crumb_page()))
                .child(ui_text(label).optical())
                .on_click(cx.listener(move |w, _, window, cx| match &target { Some(t) => w.open(t, window, cx), None => w.show_view(View::Default, cx) }))
        };
        // The path starts at its top-level folder, by that folder's name (W5).
        let root = self.entries.iter().find(|e| e.id == id).map(|e| e.top.clone()).unwrap_or_default();
        // The path starts at its top-level folder (W5).
        let first = link("top".into(), self.folder_name(&root), self.top_crumb(&root)).into_any_element();
        // A top-level folder's own page is the top-level folder: its name is the page's title, not a crumb before it.
        let own = crate::shared_store::top_of_row(&id).is_some();
        let mut crumbs = div().min_w_0().overflow_hidden().flex().items_center().text_size(px(crate::ui::CRUMB_SIZE)).line_height(px(20.)).when(!own, |d| d.child(first));
        for (i, e) in path.iter().enumerate() {
            if folds && i >= from && i < to {
                if let Some(m) = more.take() { crumbs = crumbs.child(sep()).child(m); }
                continue;
            }
            crumbs = crumbs.child(sep()).child(link(e.id.clone(), display_title(&e.title), Some(e.id.clone())));
        }
        crumbs.when(!own, |d| d.child(sep())).child(div().min_w_0().px(px(4.)).overflow_hidden().whitespace_nowrap().text_ellipsis()
            .text_color(crate::ui::crumb_page()).child(ui_text(title).optical()))
    }

    /// The folder shown when no page is picked (`View::Default`): the documents folder, else the first top-level folder.
    pub(crate) fn default_folder(&self) -> Option<String> {
        let tops = self.store.shown_tops();
        let docs = crate::store::Store::legacy_dir();
        tops.iter().find(|r| r.2 == docs).or(tops.first()).map(|r| crate::shared_store::top_row(&r.0))
    }

    /// A folder's page: one header row (the path on the left, the actions on
    /// the right), then its contents in the view picked.
    pub(crate) fn render_folder(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let id = self.folder_current.clone().unwrap_or_default();
        let header = self.topbar(&id, window, cx);
        let children = self.level_entries(Some(&id));
        // the grid scrolls right up to the header: its 8 above the cards is inside the list (`render_page_grid`),
        // not a band between, where a scrolling row was cut off 8 under the header
        let grid = self.folder_page.grid && !children.is_empty();
        let body = if children.is_empty() {
            crate::trash::empty_state("illus/empty-list.svg", tr("这个文件夹还是空的", "This folder is empty")).into_any_element()
        } else if grid { self.render_page_grid(&children, window, cx) } else { self.render_page_list(&children, cx) };
        div().size_full().flex().flex_col().child(header).when(!grid, |d| d.child(div().h(px(8.)).flex_none())).child(body).into_any_element()
    }

    pub(crate) fn folder_row_menu(&self, m: crate::app::RowMenu, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (doc, folder, rename, dup, mv, del) = (m.id.clone(), m.id.clone(), m.id.clone(), m.id.clone(), m.id.clone(), m.id.clone());
        let row = |id: &'static str, glyph: &'static str, label: &'static str| crate::design::MenuItem::new(id).icon(glyph).label(label);
        let pos = m.pos;
        // The row's "…" only manages the folder (新建 is the "+" beside it); a
        // right-click still creates straight from the menu. The folder page's header "…"
        // is this same menu, its "+" the same 新建 menu.
        let create = m.from.is_none();
        // the page header's "…" leads with 批量管理 over the page (the header has no button of its own), as a
        // top-level folder's "…" does
        let page = m.from.as_deref() == Some(crate::folder_page::header::TB_MORE);
        vec![deferred(anchored().anchor(m.corner).position(pos).snap_to_window_with_margin(px(8.)).child(crate::design::Popover::new("folder-row-menu").menu().fit(crate::design::room(pos, m.corner)).w(px(crate::design::menu_w::SM))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.row_menu = None; cx.notify(); }))
            .when(page, |d| d.child(row("fr-batch", "batch", crate::settings::text("批量管理"))
                .on_click(cx.listener(|w, _, _, cx| { w.row_menu = None; w.batch = Some(vec![]); w.batch_top = None; w.folder_page.view_menu = false; cx.notify(); }))))
            .when(create, |d| d
                .child(row("fr-new-doc", "doc", tr("新建文档", "New document")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.new_child(&doc, window, cx); })))
                .child(row("fr-new-folder", "folder", tr("新建文件夹", "New folder")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.new_folder(Some(folder.clone()), window, cx); }))))
            .child(row("fr-rename", "pen-line", tr("重命名", "Rename")).on_click(cx.listener(move |w, _, window, cx| w.start_rename(&rename, window, cx))))
            // the folder with everything in it, in the background, as a document's 创建副本 is
            .child(row("fr-copy", "copy", tr("创建副本", "Duplicate")).on_click(cx.listener(move |w, _, window, cx| w.duplicate_doc(&dup, window, cx))))
            .child(row("fr-move", "folder-input", tr("移动到", "Move to")).on_click(cx.listener(move |w, _, window, cx| w.open_move(vec![mv.clone()], window, cx))))
            .when_some(self.store.folder_dir(Some(&m.id)), |d, dir| d.child(row("fr-reveal", "folder", crate::local_folders::show_in_files()).on_click(cx.listener(move |w, _, _, cx| { w.row_menu = None; cx.reveal_path(&dir); cx.notify(); }))))
            .child(row("fr-delete", "bin", tr("删除", "Delete")).on_click(cx.listener(move |w, _, _, cx| w.ask_delete(&del, cx))))))
            .with_priority(2).into_any_element()]
    }
}

/// Where a breadcrumb goes: the default page (None), or a page or folder by id.
pub(crate) type Crumb = (String, Option<String>);

impl Workspace {
    /// A breadcrumb's "…" for the crumbs it folds (shadcn's BreadcrumbEllipsis): a click opens them
    /// as a menu hung from it; hovering shows the path.
    pub(crate) fn crumb_more(&self, key: &'static str, hidden: Vec<Crumb>, cx: &mut Context<Self>) -> Stateful<Div> {
        let path = hidden.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join(" / ");
        let open = self.crumb_menu.is_some();
        div().id(key).no_drag().relative().flex_none().size(px(20.)).flex().items_center().justify_center().rounded(px(4.)).cursor_pointer()
            .text_color(crate::ui::crumb_ink()).hover(|s| s.text_color(crate::ui::crumb_page())).when(open, |d| d.text_color(crate::ui::crumb_page()))
            .child(self.folder_page.track(key)).child("…")
            .when(!open, |d| d.tip(&path, None))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |w, _: &ClickEvent, _, cx| {
                // hung from the measured "…", never the pointer
                let Some(b) = w.folder_page.anchor(key) else { return };
                let (pos, corner) = crate::design::hang(b, crate::design::Align::Start, None);
                w.crumb_menu = if w.crumb_menu.is_some() { None } else { Some((hidden.clone(), pos, corner)) };
                cx.notify();
            }))
    }

    /// The menu of a breadcrumb's folded crumbs.
    pub(crate) fn render_crumb_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (items, pos, corner) = self.crumb_menu.clone()?;
        let menu = crate::design::Popover::new("crumb-menu").menu().fit(crate::design::room(pos, corner)).w(px(crate::design::menu_w::SM))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.crumb_menu = None; cx.notify(); }))
            .children(items.into_iter().enumerate().map(|(i, (label, target))| crate::design::MenuItem::new(("crumb-item", i)).label(label)
                .on_click(cx.listener(move |w, _, window, cx| {
                    w.crumb_menu = None;
                    match &target { Some(t) => { w.open(t, window, cx); } None => w.show_view(View::Default, cx) }
                    cx.notify();
                }))));
        Some(deferred(anchored().anchor(corner).position(pos).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(3).into_any_element())
    }

    /// Where a path's top-level folder crumb goes: its own page when it has one, else the default page.
    pub(crate) fn top_crumb(&self, key: &str) -> Option<String> {
        let row = crate::shared_store::top_row(key);
        self.entries.iter().any(|e| e.id == row).then_some(row)
    }
}
