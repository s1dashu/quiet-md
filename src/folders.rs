use crate::app::{View, Workspace};
use crate::settings::{display_title, tr};
use crate::theme::Theme;
use crate::ui::{TipExt, TitleBarExt, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};

impl Workspace {
    /// 新建文档/Markdown under `parent` (None = top level), opened and focused.
    /// `markdown` picks the on-disk format; the choice is remembered for ⌘N.
    pub(crate) fn create_doc(&mut self, markdown: bool, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.create_menu = None;
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
                self.home.sort_menu = false;
                self.entries = self.store.list();
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
                self.home.sort_menu = false;
                self.entries = self.store.list();
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

    /// 新建 › 数据表: a standalone `.sqlite` file in `parent`, opened at once.
    pub(crate) fn create_table(&mut self, parent: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.create_menu = None;
        let p = parent.clone();
        if !self.save_before(cx, move |w, window, cx| w.create_table(p.clone(), window, cx)) { return; }
        let file_before = self.store.track_file_changes();
        match self.store.create_table_in(crate::settings::text("未命名数据表"), &crate::table::blank_base(), parent.as_deref()) {
            Ok(id) => {
                self.search_open = false;
                self.doc_info = None;
                self.home.sort_menu = false;
                self.entries = self.store.list();
                self.sort();
                if let Some(p) = &parent { self.expanded.insert(p.clone()); }
                self.search.update(cx, |s, cx| s.set_text("", cx));
                self.open(&id, window, cx);
            }
            Err(e) => self.status = crate::app::SaveStatus::Failed(crate::localized_format!("新建失败：{}", "Could not create document: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        self.remember_files(file_before, tr("新建数据表", "New data table"));
        cx.notify();
    }

    pub(crate) fn copy_folder(&mut self, id: &str, cx: &mut Context<Self>) -> Option<String> {
        let ids = crate::tree::Tree::new(&self.entries).subtree(id);
        let mut copied = std::collections::HashMap::<String, String>::new();
        for key in ids {
            let Some(entry) = self.entries.iter().find(|e| e.id == key).cloned() else { continue };
            let parent = entry.meta.parent.as_ref().map(|p| copied.get(p).cloned().unwrap_or_else(|| p.clone()));
            let result = if entry.meta.folder.is_some() {
                let title = if key == id { crate::localized_format!("{} 副本", "{} copy", entry.title) } else { entry.title };
                self.store.create_folder(&title, parent.as_deref())
            } else {
                self.store.load(&key).and_then(|doc| self.store.create_from(&doc)).and_then(|new| {
                    self.store.set_parent(&new, parent.as_deref())?;
                    self.store.set_width(&new, entry.meta.width)?;
                    Ok(new)
                })
            };
            match result {
                Ok(new) => { copied.insert(key, new); }
                Err(e) => { self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx); return None; }
            }
        }
        copied.remove(id)
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
        if let Some(meta) = self.store.touch(id) && let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) { entry.meta = meta; }
        self.schedule_view_state(cx);
        if self.manual_home_sort() { self.prepare_manual_order(cx); }
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
        let file_before = self.store.track_file_changes();
        self.create_menu = None;
        match self.store.create_folder(tr("未命名文件夹", "Untitled folder"), parent.as_deref()) {
            Ok(id) => {
                self.entries = self.store.list(); self.sort();
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
        let folder_parent = parent.clone();
        // each kind its own solid, coloured mark (assets/illus/kinds), so a glance tells them apart
        let row = |id: &'static str, kind: &'static str, label: &'static str| crate::design::MenuItem::new(id).leading(img(format!("illus/kinds/{kind}.svg")).flex_none().size(px(18.))).label(label);
        let (doc_parent, md_parent, board_parent, table_parent) = (parent.clone(), parent.clone(), parent.clone(), parent.clone());
        // A top-level folder's own "+" (its row, or the sidebar heading when it is the only top-level folder) also brings
        // things into it; 主页 and a folder's page have 导入 beside their "+".
        let sidebar_heading = parent.is_none() && self.store.top_folders().len() == 1 && self.home.anchor("create-sidebar").is_some_and(|b| point(b.left(), b.bottom() + px(4.)) == pos);
        let import = (sidebar_heading || parent.as_deref().is_some_and(|p| crate::shared_store::top_of_row(p).is_some())).then(|| self.import_items(parent.clone(), cx));
        // A phone's 新建 sits at the bottom right: its menu rises from it.
        let corner = if self.home.anchor("phone-new").is_some_and(|b| point(b.right(), b.top() - px(8.)) == pos) { Anchor::BottomRight } else { Anchor::TopLeft };
        Some(deferred(anchored().anchor(corner).position(pos).snap_to_window_with_margin(px(8.)).child(
            crate::design::Popover::new("create-items").menu().min_w(px(180.))
                .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.create_menu = None; cx.notify(); }))
                .child(row("create-document", "doc", tr("文档", "Document")).on_click(cx.listener(move |w, _, window, cx| w.create_doc(false, doc_parent.clone(), window, cx))))
                .child(row("create-markdown", "markdown", "Markdown").on_click(cx.listener(move |w, _, window, cx| w.create_doc(true, md_parent.clone(), window, cx))))
                .child(row("create-board", "board", crate::settings::text("画板")).on_click(cx.listener(move |w, _, window, cx| w.create_board(board_parent.clone(), window, cx))))
                .when(crate::DATATABLE, |d| d.child(row("create-table", "datatable", crate::settings::text("数据表")).on_click(cx.listener(move |w, _, window, cx| w.create_table(table_parent.clone(), window, cx)))))
                .child(row("create-folder", "folder", tr("文件夹", "Folder")).on_click(cx.listener(move |w, _, window, cx| w.new_folder(folder_parent.clone(), window, cx))))
                .when_some(import, |d, rows| d.child(crate::design::MenuSeparator::new()).children(rows))
        )).with_priority(3).into_any_element())
    }

    /// A folder's page: one header row (the path on the left, the actions on
    /// the right), then its contents in the view 主页 shows.
    pub(crate) fn render_folder(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let id = self.folder_current.clone().unwrap_or_default();
        let title = self.entries.iter().find(|e| e.id == id).map(|e| display_title(&e.title)).unwrap_or_else(|| tr("文件夹", "Folder").to_string());
        // 我的文档 › … › parent › this folder; a long path folds its middle into "…".
        let sep = || div().mx(px(4.)).flex_none().child(icon("chevron-right", 12., Theme::icon()));
        let link = |key: String, label: String, target: Option<String>| {
            div().id(SharedString::from(format!("folder-crumb-{key}"))).no_drag().flex_none().max_w(px(220.)).px(px(4.)).rounded(px(6.))
                .overflow_hidden().whitespace_nowrap().text_ellipsis().cursor_pointer()
                .text_color(Theme::text_caption()).hover(|s| s.text_color(Theme::text()))
                .child(ui_text(label))
                .on_click(cx.listener(move |w, _, window, cx| match &target { Some(t) => w.open(t, window, cx), None => w.show_view(View::Home, cx) }))
        };
        // A top-level folder's own row (two or more top-level folders) is the first crumb, by name.
        let path: Vec<_> = self.ancestors(&id).into_iter().filter(|e| crate::shared_store::top_of_row(&e.id).is_none()).collect();
        // The path starts at its top-level folder, by that folder's name (W5).
        let root = self.entries.iter().find(|e| e.id == id).map(|e| e.top.clone()).unwrap_or_default();
        let first = link("home".into(), self.folder_name(&root), None).into_any_element();
        // A top-level folder's own page is the top-level folder: its name is the page's title, not a crumb before it.
        // A phone: the folder's name only (no path), as iOS titles a pushed page.
        let phone = crate::theme::compact(f32::from(window.viewport_size().width));
        let own = crate::shared_store::top_of_row(&id).is_some() || phone;
        let mut crumbs = div().min_w_0().flex().items_center().text_size(px(17.)).line_height(px(24.)).when(!own, |d| d.child(first));
        for (i, e) in path.iter().enumerate().filter(|_| !phone) {
            if path.len() > 2 && i > 0 && i < path.len() - 1 {
                if i == 1 {
                    let hidden: Vec<String> = path[1..path.len() - 1].iter().map(|e| display_title(&e.title)).collect();
                    crumbs = crumbs.child(sep()).child(div().id("folder-crumb-more").flex_none().px(px(4.)).text_color(Theme::text_caption()).tip(&hidden.join(" / "), None).child("…"));
                }
                continue;
            }
            crumbs = crumbs.child(sep()).child(link(e.id.clone(), display_title(&e.title), Some(e.id.clone())));
        }
        crumbs = crumbs.when(!own, |d| d.child(sep())).child(div().min_w_0().px(px(4.)).overflow_hidden().whitespace_nowrap().text_ellipsis()
            .font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(ui_text(title)));
        let doc = id.clone();
        let header = crate::trash::page_header_with(self, crumbs, 26., crate::trash::Nav::Back, cx)
            .child(self.place_button("folder-more", Some(id.clone()), cx))
            // A phone's bar has room for 更多 and + only: the view follows 主页's choice, 导入 is 主页's.
            .when(!phone, |d| d.child(self.import_button("folder-import", Some(id.clone()), cx)))
            .child(crate::ui::header_icon("folder-new", "plus")
                .child(self.home.track("create-folder"))
                .selected(self.create_menu.as_ref().is_some_and(|(_, pos)| self.home.anchor("create-folder").is_some_and(|b| point(b.left(), b.bottom() + px(4.)) == *pos)))
                .tip(tr("新建", "New"), Some(crate::shortcuts::hint("workspace::NewDoc", "⌘ + N")))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |w, ev: &ClickEvent, _, cx| { let pos = w.home.anchor("create-folder").map_or(ev.position(), |b| point(b.left(), b.bottom() + px(4.))); let p = Some(doc.clone()); w.create_menu = if w.create_menu.is_some() { None } else { Some((p, pos)) }; cx.notify(); })))
            .when(!phone, |d| d.child(div().id("folder-view-switch").no_drag().ml(px(12.)).child(self.view_switch(cx))));
        let children = self.level_entries(Some(&id));
        let body = if children.is_empty() {
            crate::trash::empty_state("illus/empty-list.svg", tr("这个文件夹还是空的", "This folder is empty")).into_any_element()
        } else if self.shows_grid(window) {
            self.render_home_grid(&children, true, window, cx)
        } else {
            self.render_home_list(&children, true, cx)
        };
        div().size_full().flex().flex_col().child(header).child(div().h(px(8.)).flex_none()).child(body).into_any_element()
    }

    pub(crate) fn folder_row_menu(&self, m: crate::app::RowMenu, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (doc, folder, rename, mv, del) = (m.id.clone(), m.id.clone(), m.id.clone(), m.id.clone(), m.id.clone());
        let row = |id: &'static str, glyph: &'static str, label: &'static str| crate::design::MenuItem::new(id).icon(glyph).label(label);
        let pos = if m.pos.x < px(0.) { point(-m.pos.x, m.pos.y + px(22.)) } else { m.pos };
        // The row's "…" only manages the folder (新建 is the "+" beside it); a
        // right-click still creates straight from the menu.
        let create = !(m.pos.x < px(0.));
        vec![deferred(anchored().position(pos).snap_to_window_with_margin(px(8.)).child(crate::design::Popover::new("folder-row-menu").menu().min_w(px(190.))
            .on_mouse_down_out(cx.listener(|w, _, _, cx| { w.row_menu = None; cx.notify(); }))
            .when(create, |d| d
                .child(row("fr-new-doc", "doc", tr("新建文档", "New document")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.new_child(&doc, window, cx); })))
                .child(row("fr-new-folder", "folder", tr("新建文件夹", "New folder")).on_click(cx.listener(move |w, _, window, cx| { w.row_menu = None; w.new_folder(Some(folder.clone()), window, cx); }))))
            .child(row("fr-rename", "square-pen", tr("重命名", "Rename")).on_click(cx.listener(move |w, _, window, cx| w.start_rename(&rename, window, cx))))
            .child(row("fr-move", "folder-input", tr("移动到", "Move to")).on_click(cx.listener(move |w, _, window, cx| w.open_move(vec![mv.clone()], window, cx))))
            .child(row("fr-delete", "bin", tr("删除", "Delete")).on_click(cx.listener(move |w, _, _, cx| w.ask_delete(&del, cx))))))
            .with_priority(2).into_any_element()]
    }
}
