//! The 工作区 the windows share: every root folder's library behind one
//! handle. A call about a document goes to the root it is in; a new one goes
//! into its folder's root, else the root of the document in front (`active`),
//! else the first root. Nothing here crosses roots: moves and links stay
//! inside one (docs/design/workspaces.md, W6 / W7).

use crate::store::*;
use crate::model::Document;
use anyhow::Result;
use fdl_core::workspace::{AddError, Root, Workspace};
use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc, sync::{atomic::{AtomicBool, Ordering}, Arc}};

struct Shared {
    ws: Workspace,
    active: Option<String>,
    /// Each root's entries and 回收站 as last listed, so a background writer's
    /// list (one root's) can stand in for that root among the others.
    entries: Vec<(String, Vec<Entry>)>,
    trash: Vec<(String, Vec<TrashEntry>)>,
}

#[derive(Clone)]
pub struct SharedStore(Rc<RefCell<Shared>>, Arc<AtomicBool>);
pub struct FileTaskGuard(Arc<AtomicBool>);

impl Drop for FileTaskGuard {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

/// With two or more roots, each shows as a row at the top of the sidebar's
/// tree, its own top level inside it; this is that row's id. Only the
/// window sees these rows: a library is handed its own top level (`None`).
const ROOT_ROW: &str = "root:";
pub fn root_row(key: &str) -> String { format!("{ROOT_ROW}{key}") }
/// The root whose row `id` is.
pub fn root_of_row(id: &str) -> Option<&str> { id.strip_prefix(ROOT_ROW) }

/// Said when something needs a root and none is open (or can be).
pub fn no_root() -> anyhow::Error { anyhow::anyhow!("{}", crate::settings::tr("还没有打开任何文件夹", "No folder is open yet")) }

impl gpui::Global for SharedStore {}
impl SharedStore {
    pub fn open() -> Self { Self::with_workspace(Workspace::load()) }
    pub fn with_workspace(ws: Workspace) -> Self {
        let s = Self(Rc::new(RefCell::new(Shared { ws, active: None, entries: vec![], trash: vec![] })), Arc::new(AtomicBool::new(false)));
        if let Some(r) = s.0.borrow().ws.first() { Store::set_active_root(&r.path); }
        s.roots_changed();
        s
    }
    /// After roots came or went: whether there is a root's look to use.
    fn roots_changed(&self) { crate::settings::set_has_roots(self.0.borrow().ws.first().is_some()); }
    pub fn file_task_busy(&self) -> bool { self.1.load(Ordering::Acquire) }
    pub fn begin_file_task(&self) -> Option<FileTaskGuard> {
        self.1.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok().then(|| FileTaskGuard(self.1.clone()))
    }

    // ---- roots

    /// The roots, in sidebar order: (key, name, path, error).
    pub fn roots(&self) -> Vec<(String, String, PathBuf, Option<String>)> {
        self.0.borrow().ws.roots.iter().map(|r: &Root| (r.key.clone(), r.name(), r.path.clone(), r.error.clone())).collect()
    }
    pub fn add_root(&self, dir: &Path) -> std::result::Result<String, AddError> { let r = self.0.borrow_mut().ws.add(dir); self.roots_changed(); r }
    pub fn create_root(&self, dir: &Path) -> std::result::Result<String, AddError> { let r = self.0.borrow_mut().ws.create(dir); self.roots_changed(); r }
    pub fn remove_root(&self, key: &str) -> Result<()> {
        let mut sh = self.0.borrow_mut();
        if let Some(s) = sh.ws.store(key) { let _ = s.flush_view_state(); }
        sh.ws.remove(key)?;
        if sh.active.as_deref() == Some(key) {
            sh.active = None;
            if let Some(r) = sh.ws.first() { Store::set_active_root(&r.path); }
        }
        drop(sh);
        self.roots_changed();
        Ok(())
    }
    pub fn retry_root(&self, key: &str) { self.0.borrow_mut().ws.retry(key) }
    pub fn root_path(&self, key: &str) -> Option<PathBuf> { self.0.borrow().ws.root(key).map(|r| r.path.clone()) }
    /// The root `id` is in (a root's row: that root).
    pub fn root_of(&self, id: &str) -> Option<String> {
        if let Some(k) = root_of_row(id) { return Some(k.to_string()); }
        self.0.borrow_mut().ws.owner(id)
    }
    /// The root folder holding `path`, if one is open.
    pub fn root_of_path(&self, path: &Path) -> Option<String> { self.0.borrow().ws.root_of_path(path).map(|r| r.key.clone()) }

    /// The document in front is `id`: new ones without a folder go to its
    /// root, and images, 数据表 and 画板 resolve against it.
    /// True when that is another root than before (its look should take over).
    pub fn set_active(&self, id: &str) -> bool {
        let key = self.root_of(id);
        key.is_some_and(|k| self.set_active_root(&k))
    }
    /// `set_active` for root `key`; true when the root in front changed.
    pub fn set_active_root(&self, key: &str) -> bool {
        let mut sh = self.0.borrow_mut();
        let Some(path) = sh.ws.root(key).filter(|r| r.store.is_some()).map(|r| r.path.clone()) else { return false };
        let changed = Store::root_dir() != path;
        Store::set_active_root(&path);
        sh.active = Some(key.to_string());
        changed
    }
    /// The root things go to when nothing else says: the active one, else the first.
    pub fn active_root(&self) -> Option<String> {
        let sh = self.0.borrow();
        sh.active.clone().filter(|k| sh.ws.root(k).is_some_and(|r| r.store.is_some())).or_else(|| sh.ws.first().map(|r| r.key.clone()))
    }

    // ---- routing

    /// `f` on the library `id` is in.
    fn on<R>(&self, id: &str, f: impl FnOnce(&mut Store) -> R) -> Option<R> { self.0.borrow_mut().ws.store_of(id).map(f) }
    fn on_res<R>(&self, id: &str, f: impl FnOnce(&mut Store) -> Result<R>) -> Result<R> {
        self.on(id, f).unwrap_or_else(|| Err(anyhow::anyhow!("文档或文件夹不存在")))
    }
    /// `f` on the library a new entry in `parent` goes to, with `parent` as
    /// that library sees it (a root's own row is its top level).
    fn put<R>(&self, parent: Option<&str>, f: impl FnOnce(&mut Store, Option<&str>) -> Result<R>) -> Result<R> {
        let (key, real) = match parent.and_then(root_of_row) { Some(k) => (Some(k.to_string()), None), None => (match parent { Some(p) => self.root_of(p), None => self.active_root() }, parent) };
        let mut sh = self.0.borrow_mut();
        let s = key.and_then(|k| sh.ws.store(&k)).ok_or_else(no_root)?;
        f(s, real)
    }
    /// `f` on the library `id` is in, else (no id) the active root's.
    pub fn with<R>(&self, id: Option<&str>, f: impl FnOnce(&mut Store) -> Result<R>) -> Result<R> {
        match id { Some(id) => self.on_res(id, f), None => self.put(None, |s, _| f(s)) }
    }
    /// `f` on every open root's library.
    pub fn each(&self, mut f: impl FnMut(&mut Store)) {
        for r in self.0.borrow_mut().ws.roots.iter_mut() { if let Some(s) = r.store.as_mut() { f(s); } }
    }

    pub fn take_warnings(&self) -> Vec<String> { self.0.borrow_mut().ws.take_warnings() }
    pub fn list(&mut self) -> Vec<Entry> {
        let all = {
            let mut sh = self.0.borrow_mut();
            let all = sh.ws.list();
            let keys: Vec<String> = sh.ws.roots.iter().map(|r| r.key.clone()).collect();
            sh.entries = keys.into_iter().map(|k| { let v: Vec<Entry> = all.iter().filter(|e| e.root == k).cloned().collect(); (k, v) }).collect();
            all
        };
        self.with_root_rows(all)
    }
    /// Whether roots show as rows of the tree (two or more of them).
    pub fn root_rows(&self) -> bool { self.0.borrow().ws.roots.len() > 1 }
    /// `entries` as the window shows them: with two or more roots, a row
    /// for each, holding what is at its top level.
    fn with_root_rows(&self, mut entries: Vec<Entry>) -> Vec<Entry> {
        if !self.root_rows() { return entries; }
        for e in entries.iter_mut().filter(|e| e.meta.parent.is_none() && !e.meta.local() && !e.root.is_empty()) {
            e.meta.parent = Some(root_row(&e.root));
            // Its place at the top of its root, now that it sits in the root's row.
            e.meta.order = e.meta.root_order.unwrap_or(0);
        }
        for (i, (key, name, _, _)) in self.roots().into_iter().enumerate() {
            let meta = Meta { folder: Some(name.clone()), root_order: Some(i as u64 + 1), ..Default::default() };
            entries.push(Entry { id: root_row(&key), meta, title: name, icon: None, body: Default::default(), reminders: vec![], links: vec![], note: None, root: key });
        }
        entries
    }
    /// `parent` as its library sees it: a root's row is that root's top level.
    pub fn real_parent<'a>(&self, parent: Option<&'a str>) -> Option<&'a str> { parent.filter(|p| root_of_row(p).is_none()) }
    /// Every root's entries, root `key`'s replaced by `fresh` (a background
    /// writer's list of that root).
    pub fn merge_entries(&self, key: &str, mut fresh: Vec<Entry>) -> Vec<Entry> {
        for e in &mut fresh { e.root = key.to_string(); }
        let mut sh = self.0.borrow_mut();
        let order: Vec<String> = sh.ws.roots.iter().map(|r| r.key.clone()).collect();
        match sh.entries.iter_mut().find(|(k, _)| k == key) { Some((_, v)) => *v = fresh, None => sh.entries.push((key.to_string(), fresh)) }
        let mut out = vec![];
        for k in order { if let Some((_, v)) = sh.entries.iter().find(|(x, _)| *x == k) { out.extend(v.iter().cloned()); } }
        drop(sh);
        self.with_root_rows(out)
    }
    /// `merge_entries` for the 回收站.
    pub fn merge_trash(&self, key: &str, fresh: Vec<TrashEntry>) -> Vec<TrashEntry> {
        let mut sh = self.0.borrow_mut();
        let order: Vec<String> = sh.ws.roots.iter().map(|r| r.key.clone()).collect();
        match sh.trash.iter_mut().find(|(k, _)| k == key) { Some((_, v)) => *v = fresh, None => sh.trash.push((key.to_string(), fresh)) }
        let mut out = vec![];
        for k in order { if let Some((_, v)) = sh.trash.iter().find(|(x, _)| *x == k) { out.extend(v.iter().cloned()); } }
        out
    }
    pub fn load(&self, id: &str) -> Result<Document> { self.on_res(id, |s| s.load(id)) }
    pub fn load_for_edit(&self, id: &str, shared: bool) -> Result<Document> { self.on_res(id, |s| s.load_for_edit(id, shared)) }
    pub fn forget_loaded(&self, id: &str) { self.on(id, |s| s.forget_loaded(id)); }
    pub fn recover_later(&self, doc: &Document) -> Result<()> { self.put(None, |s, _| s.recover_later(doc)) }
    pub fn save(&mut self, id: &str, doc: &Document) -> Result<Meta> { self.on_res(id, |s| s.save(id, doc)) }
    pub fn create(&mut self) -> Result<(String, Document)> { self.put(None, |s, _| s.create()) }
    pub fn create_from(&mut self, doc: &Document) -> Result<String> { self.put(None, |s, _| s.create_from(doc)) }
    pub fn size_of(&self, id: &str, doc: &Document) -> u64 { self.on(id, |s| s.size_of(id, doc)).unwrap_or(0) }
    pub fn list_trash(&mut self) -> Vec<TrashEntry> {
        let mut sh = self.0.borrow_mut();
        let mut lists = vec![];
        for r in sh.ws.roots.iter_mut() { if let Some(s) = r.store.as_mut() { lists.push((r.key.clone(), s.list_trash())); } }
        let out = lists.iter().flat_map(|(_, v)| v.iter().cloned()).collect();
        sh.trash = lists;
        out
    }
    pub fn restore(&mut self, id: &str) -> Result<bool> {
        let mut sh = self.0.borrow_mut();
        let s = sh.ws.roots.iter_mut().filter_map(|r| r.store.as_mut()).find(|s| s.trashed(id)).ok_or_else(|| anyhow::anyhow!("回收站里没有这篇文档"))?;
        s.restore(id)
    }
    /// The root whose 回收站 holds `id`.
    pub fn trash_root(&self, id: &str) -> Option<String> {
        self.0.borrow().ws.roots.iter().find(|r| r.store.as_ref().is_some_and(|s| s.trashed(id))).map(|r| r.key.clone())
    }
    pub fn set_width(&mut self, id: &str, width: PageWidth) -> Result<()> { self.on_res(id, |s| s.set_width(id, width)) }
    pub fn update_meta(&mut self, id: &str, f: impl FnOnce(&mut Meta)) -> Result<()> { self.on_res(id, |s| s.update_meta(id, f)) }
    pub fn set_catalogue(&mut self, id: &str, pinned: bool) -> Result<()> { self.on_res(id, |s| s.set_catalogue(id, pinned)) }
    /// 应用级 settings: this computer's, the same whatever root is in front.
    pub fn settings(&self) -> Settings { Store::app_settings() }
    pub fn save_settings(&self, s: &Settings) -> Result<()> { Store::save_app_settings(s) }
    /// 版本历史's snapshots kept per document: the root in front's (根级, 2.1).
    pub fn history_limit(&self) -> u32 { self.put(None, |s, _| Ok(s.settings().history_limit())).unwrap_or_else(|_| fdl_core::versions::default_history_limit()) }
    pub fn set_history_limit(&self, n: u32) -> Result<()> { self.put(None, |s, _| { let mut v = s.settings(); v.set_history_limit(n); s.save_settings(&v) }) }
    pub fn scroll_top(&self, id: &str) -> Option<(usize, f32)> { self.on(id, |s| s.scroll_top(id)).flatten() }
    pub fn remember_scroll(&self, id: &str, top: Option<(usize, f32)>) { self.on(id, |s| s.remember_scroll(id, top)); }
    pub fn flush_view_state(&self) -> Result<()> {
        let mut r = Ok(());
        self.each(|s| if let Err(e) = s.flush_view_state() { r = Err(e) });
        r
    }
    pub fn versions(&self, id: &str) -> Vec<fdl_core::versions::Version> { self.on(id, |s| s.versions(id)).unwrap_or_default() }
    pub fn version_diff(&self, id: &str, name: &str, baseline: Option<&str>) -> Result<Vec<fdl_core::diff::Line>> { self.on_res(id, |s| s.version_diff(id, name, baseline)) }
    pub fn version_document(&self, id: &str, name: &str) -> Result<Document> { self.on_res(id, |s| s.version_document(id, name)) }
    pub fn set_version_label(&self, id: &str, name: &str, label: Option<&str>) -> Result<()> { self.on_res(id, |s| s.set_version_label(id, name, label)) }
    pub fn delete_version(&self, id: &str, name: &str) -> Result<()> { self.on_res(id, |s| s.delete_version(id, name)) }
    pub fn restore_version(&self, id: &str, name: &str) -> Result<Document> { self.on_res(id, |s| s.restore_version(id, name)) }
    pub fn save_version_now(&self, id: &str) -> Result<bool> { self.on_res(id, |s| s.save_version_now(id)) }
    pub fn rename(&mut self, id: &str, title: &str) -> Result<Meta> { self.on_res(id, |s| s.rename(id, title)) }
    pub fn convert(&mut self, id: &str, markdown: bool) -> Result<()> { self.on_res(id, |s| s.convert(id, markdown)) }
    pub fn touch(&mut self, id: &str) -> Option<Meta> { self.on(id, |s| s.touch(id)).flatten() }
    pub fn create_folder(&mut self, title: &str, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_folder(title, parent)) }
    pub fn accept_file_title(&self, id: &str, title: &str) -> Result<()> { self.on_res(id, |s| s.accept_file_title(id, title)) }
    pub fn initialize_manual_order(&mut self) -> Result<()> {
        let mut r = Ok(());
        self.each(|s| if let Err(e) = s.initialize_manual_order() { r = Err(e) });
        r
    }
    /// One journal of file changes across every root.
    pub fn track_file_changes(&mut self) -> FileChanges {
        let mut sh = self.0.borrow_mut();
        let mut stores = sh.ws.roots.iter_mut().filter_map(|r| r.store.as_mut());
        let Some(first) = stores.next() else { return FileChanges::none() };
        let changes = first.track_file_changes();
        for s in stores { s.share_file_changes(&changes); }
        changes
    }
    /// A copy of the library `id` is in (else the active root's) and its
    /// root's key, for writes off the main thread; what it lists afterwards
    /// goes back through `merge_entries`.
    pub fn tree_writer(&self, id: Option<&str>) -> Option<(String, Store)> {
        let key = match id { Some(id) => self.root_of(id), None => self.active_root() }?;
        let mut sh = self.0.borrow_mut();
        let w = sh.ws.store(&key)?.tree_writer();
        Some((key, w))
    }
    /// `tree_writer` for root `key`.
    pub fn root_writer(&self, key: &str) -> Option<Store> { self.0.borrow_mut().ws.store(key).map(|s| s.tree_writer()) }
    pub fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<Meta> {
        self.same_root(id, parent)?;
        let parent = self.real_parent(parent);
        self.on_res(id, |s| s.set_parent(id, parent))
    }
    /// A drop in the tree, inside one root.
    pub fn move_in_tree(&mut self, id: &str, parent: Option<&str>, before: Option<&str>) -> Result<()> {
        self.same_root(id, parent)?;
        let (parent, before) = (self.real_parent(parent), self.real_parent(before));
        self.on_res(id, |s| s.move_in_tree(id, parent, before))
    }
    /// Moves stay inside a root for now (W7).
    pub fn same_root(&self, id: &str, parent: Option<&str>) -> Result<()> {
        // With roots as rows, the very top is no root's: nothing goes there.
        let stray = parent.is_none() && self.root_rows();
        if stray || parent.is_some_and(|p| self.root_of(p) != self.root_of(id)) { anyhow::bail!("{}", crate::settings::tr("暂不支持在文件夹之间移动", "Moving between folders isn't supported yet")); }
        Ok(())
    }
    pub fn set_starred(&mut self, id: &str, on: bool) -> Result<u64> { self.on_res(id, |s| s.set_starred(id, on)) }
    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> { self.on_res(id, |s| s.set_pinned(id, pinned)) }
    pub fn is_markdown(&self, id: &str) -> bool { self.on(id, |s| s.is_markdown(id)).unwrap_or(false) }
    pub fn create_markdown(&mut self, doc: &Document, front: &str) -> Result<String> { self.put(None, |s, _| s.create_markdown(doc, front)) }
    pub fn create_markdown_in(&mut self, doc: &Document, front: &str, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_markdown_in(doc, front, parent)) }
    pub fn create_from_in(&mut self, doc: &Document, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_from_in(doc, parent)) }
    /// A child page of document `id` (#357): `id` becomes its folder's 正文, in its own root.
    pub fn create_child(&mut self, id: &str, doc: &Document) -> Result<String> { self.on_res(id, |s| s.create_child(id, doc)) }
    /// A root's own row is a folder, never a 正文: it takes no child page this way.
    pub fn can_have_children(&self, id: &str) -> bool { root_of_row(id).is_none() && self.on(id, |s| s.can_have_children(id)).unwrap_or(false) }
    pub fn create_board_in(&mut self, title: &str, board: &crate::whiteboard::Board, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_board_in(title, board, parent)) }
    #[cfg(not(target_family = "wasm"))]
    pub fn create_table_in(&mut self, title: &str, base: &crate::table::Base, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_table_in(title, base, parent)) }
    #[cfg(not(target_family = "wasm"))]
    pub fn copy_table(&mut self, src: &str, title: &str) -> Result<String> { self.on_res(src, |s| s.copy_table(src, title)) }
    #[cfg(target_family = "wasm")]
    pub fn create_table_in(&mut self, _title: &str, _base: &crate::table::Base, _parent: Option<&str>) -> Result<String> { anyhow::bail!("unsupported") }
    #[cfg(target_family = "wasm")]
    pub fn copy_table(&mut self, _src: &str, _title: &str) -> Result<String> { anyhow::bail!("unsupported") }
    #[cfg(not(target_family = "wasm"))]
    pub fn copy_table_version(&mut self, src: &str, name: &str, title: &str) -> Result<String> { self.on_res(src, |s| s.copy_table_version(src, name, title)) }
    #[cfg(target_family = "wasm")]
    pub fn copy_table_version(&mut self, _src: &str, _name: &str, _title: &str) -> Result<String> { anyhow::bail!("unsupported") }
    #[cfg(not(target_family = "wasm"))]
    pub fn snapshot_table(&self, id: &str, force: bool) -> Result<bool> { self.on_res(id, |s| s.snapshot_table(id, force)) }
    #[cfg(target_family = "wasm")]
    pub fn snapshot_table(&self, _id: &str, _force: bool) -> Result<bool> { Ok(false) }
    pub fn front_matter_of(&self, id: &str) -> String { self.on(id, |s| s.front_matter_of(id)).unwrap_or_default() }
    /// A Markdown file opened where it is: in the root holding it, else kept
    /// with the active root's 本机文件.
    pub fn open_local(&mut self, path: &Path) -> Result<String> {
        let key = self.root_of_path(path).or_else(|| self.active_root()).ok_or_else(no_root)?;
        let mut sh = self.0.borrow_mut();
        sh.ws.store(&key).ok_or_else(no_root)?.open_local(path)
    }
    pub fn local_path(&self, id: &str) -> Option<PathBuf> { self.on(id, |s| s.local_path(id)).flatten() }
    /// A folder on disk; `None` the active root itself.
    pub fn folder_dir(&self, id: Option<&str>) -> Option<PathBuf> {
        match id { Some(id) => self.on(id, |s| s.folder_dir(Some(id))).flatten(), None => self.active_root().and_then(|k| self.root_path(&k)) }
    }
    pub fn file_path(&self, id: &str) -> Option<PathBuf> { self.on(id, |s| s.file_path(id)).flatten() }
    /// Every open root folder, for what scans them all (unused images, wiki links).
    pub fn root_dirs(&self) -> Vec<PathBuf> { self.0.borrow().ws.roots.iter().filter(|r| r.store.is_some()).map(|r| r.path.clone()).collect() }
}
