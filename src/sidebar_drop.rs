//! Dropping onto the sidebar's 我的文档: files from the Finder are imported
//! into the folder under the pointer, or between the rows there (a document
//! row only opens a slot beside it, #418).
use fdl_core::vfs::PathExt as _;
use crate::app::Workspace;
use crate::settings::tr;
use gpui::*;
use std::path::PathBuf;

/// Where a drop on the sidebar lands: in `parent` (None: the top of 我的文档),
/// before `before` when it opens a slot between rows (index, depth), or last
/// in a folder row it is over.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SidebarDropAt { pub parent: Option<String>, pub slot: Option<(usize, usize)>, pub before: Option<String> }

impl SidebarDropAt {
    /// The top of 我的文档 with no tree shown to open a slot in.
    pub(crate) fn top() -> Self { Self { parent: None, slot: None, before: None } }
}

fn importable(p: &std::path::Path) -> bool {
    p.fs_is_dir() || crate::local_files::is_markdown_path(p) || p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")) || crate::transfer::is_csv_path(p)
}

impl Workspace {
    /// Something dragged over the sidebar at `at`: where it would land
    /// (`Some(None)`: the top of 我的文档).
    pub(crate) fn sidebar_drop_at(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let target = if self.view == crate::app::View::Settings || self.view == crate::app::View::Trash { None } else { self.sidebar_target(at) };
        if target != self.sidebar_drop {
            if let Some(f) = target.as_ref().filter(|t| t.slot.is_none()).and_then(|t| t.parent.as_ref()) { self.folder_drag.kick(f, 3.2); }
            self.sidebar_drop = target;
            cx.notify();
        }
        // Over the sidebar, the page's own drop veil steps back.
        if self.sidebar_drop.is_some() && let Some(d) = self.folder_page.folders.file_drop.as_mut() { d.leave(); cx.notify(); }
        self.sidebar_drop.is_some()
    }

    /// Files from the Finder let go over the sidebar.
    pub(crate) fn sidebar_files_dropped(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(target) = self.sidebar_drop.take() else { return };
        let (dirs, files): (Vec<_>, Vec<_>) = paths.into_iter().filter(|p| importable(p)).partition(|p| p.fs_is_dir());
        if dirs.is_empty() && files.is_empty() {
            self.show_toast_kind(crate::ui::ToastKind::Error, tr("这些文件不能导入：支持 Markdown、DocxXML 文件和文件夹", "These files can't be imported: Markdown, DocxXML files and folders are supported"), cx);
            cx.notify();
            return;
        }
        self.ask_import(crate::app::DropImport::Files(dirs.into_iter().chain(files).collect()), target, cx);
    }

    /// 是否导入: asked before anything from outside comes into the library.
    fn ask_import(&mut self, what: crate::app::DropImport, target: SidebarDropAt, cx: &mut Context<Self>) {
        self.row_menu = None;
        self.confirm = Some(crate::app::Confirm::Import { what, target });
        cx.notify();
    }

    /// The place a drop goes, for the dialog and the toast.
    pub(crate) fn drop_place(&self, target: &SidebarDropAt) -> String {
        target.parent.as_ref().and_then(|t| self.entries.iter().find(|e| &e.id == t)).map(|e| crate::settings::display_title(&e.title))
            .unwrap_or_else(|| self.front_top_name())
    }

    /// 导入 confirmed: bring it in where it was dropped.
    pub(crate) fn import_dropped(&mut self, what: crate::app::DropImport, target: SidebarDropAt, window: &mut Window, cx: &mut Context<Self>) {
        let crate::app::DropImport::Files(paths) = what;
        self.import_dropped_files(paths, target, window, cx)
    }

    fn import_dropped_files(&mut self, paths: Vec<PathBuf>, target: SidebarDropAt, window: &mut Window, cx: &mut Context<Self>) {
        let (dirs, files): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| p.fs_is_dir());
        let known: std::collections::HashSet<String> = self.entries.iter().map(|e| e.id.clone()).collect();
        let parent = target.parent.clone();
        self.landed_in(parent.as_deref());
        if !files.is_empty() { self.import_paths(&files, parent.clone(), false, window, cx); }
        // Into the slot it showed: what came in at this level goes before the row below it.
        let new: Vec<String> = self.entries.iter().filter(|e| !known.contains(&e.id) && e.meta.parent == parent).map(|e| e.id.clone()).collect();
        self.place_dropped(&new, &target);
        // Folders come in off the window's thread; each takes its slot once it is in.
        for dir in &dirs {
            let target = target.clone();
            self.import_folder_then(dir, parent.clone(), false, Some(Box::new(move |w, id| w.place_dropped(&[id.to_string()], &target))), window, cx);
        }
        cx.notify();
    }

    /// Put what a drop brought in into the slot it showed (a folder row: as imported).
    fn place_dropped(&mut self, ids: &[String], target: &SidebarDropAt) {
        let journal = self.store.track_file_changes();
        if target.slot.is_some() {
            for id in ids { let _ = self.store.move_in_tree(id, target.parent.as_deref(), target.before.as_deref()); }
        }
        self.entries.set(self.store.list());
        self.sort();
        // Part of the import: undo checks every place it recorded.
        self.amend_file_record(ids, journal.take());
    }

    /// The folder that took a drop opens and bounces.
    fn landed_in(&mut self, folder: Option<&str>) {
        if let Some(f) = folder { self.expanded.insert(f.to_string()); self.folder_drag.kick(f, 4.5); }
    }

    /// Nothing is being dragged any more: forget the drop target.
    pub(crate) fn settle_sidebar_drop(&mut self, cx: &App) {
        if self.sidebar_drop.is_some() && !cx.has_active_drag() && !crate::devtools::file_drag() { self.sidebar_drop = None; }
    }

    /// The sidebar list's handlers for files dragged in.
    pub(crate) fn sidebar_drop_zone(&self, list: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        list.on_drag_move(cx.listener(|w, ev: &DragMoveEvent<ExternalPaths>, _, cx| {
                let at = ev.event.position;
                if ev.bounds.contains(&at) { w.sidebar_drop_at(at, cx); } else if w.sidebar_drop.take().is_some() { cx.notify(); }
            }))
            // Where it is let go decides, not the last move heard.
            .on_drop(cx.listener(|w, paths: &ExternalPaths, window, cx| { w.sidebar_drop_at(window.mouse_position(), cx); w.sidebar_files_dropped(paths.paths().to_vec(), cx) }))
    }
}
