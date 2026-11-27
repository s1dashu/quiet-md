//! The top-level folders the windows share: every top-level folder's library behind one
//! handle. A call about a document goes to the top-level folder it is in; a new one goes
//! into its folder's top-level folder, else the top-level folder of the document in front (`active`),
//! else the first top-level folder. Links stay inside one (docs/design/workspaces.md, W6); a
//! page or folder moved into another is handed from one library to the other (`move_across`).

use crate::store::*;
use crate::model::Document;
use anyhow::Result;
use fdl_core::vfs::PathExt as _;
use fdl_core::top_folders::{AddError, RenameError, TopFolder, TopFolders};
use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc, sync::{atomic::{AtomicBool, Ordering}, Arc}};

struct Shared {
    tops: TopFolders,
    active: Option<String>,
    /// Each top-level folder's entries and 回收站 as last listed, so a background writer's
    /// list (one top-level folder's) can stand in for that top-level folder among the others.
    entries: Vec<(String, Vec<Entry>)>,
    trash: Vec<(String, Vec<TrashEntry>)>,
    /// `Store::set_place_by_hand` for every top-level folder, also those opened later.
    place_by_hand: bool,
    /// The first listing is still running off the main thread (`loading`):
    /// listing here reads no document, and whether anything was listed
    /// (after a write) meanwhile.
    loading: bool,
    listed_meanwhile: bool,
    /// `begin_relist` until `end_relist`.
    relisting: bool,
    /// Top-level folders that moved with a folder of another (`follow`), for the windows (`take_moved`).
    moved: Vec<String>,
}

#[derive(Clone)]
pub struct SharedStore(Rc<RefCell<Shared>>, Arc<AtomicBool>);
pub struct FileTaskGuard(Arc<AtomicBool>);

impl Drop for FileTaskGuard {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

/// With two or more top-level folders, each shows as a row at the top of the sidebar's
/// tree, its own top level inside it; this is that row's id. Only the
/// window sees these rows: a library is handed its own top level (`None`).
const TOP_ROW: &str = "top:";
pub fn top_row(key: &str) -> String { format!("{TOP_ROW}{key}") }
/// The top-level folder whose row `id` is.
pub fn top_of_row(id: &str) -> Option<&str> { id.strip_prefix(TOP_ROW) }

/// Said when something needs a top-level folder and none is open (or can be).
pub fn no_folder() -> anyhow::Error { anyhow::anyhow!("{}", crate::settings::tr("还没有打开任何文件夹", "No folder is open yet")) }

impl gpui::Global for SharedStore {}
impl SharedStore {
    pub fn open() -> Self { Self::with_top_folders(TopFolders::load()) }
    pub fn with_top_folders(tops: TopFolders) -> Self {
        let s = Self(Rc::new(RefCell::new(Shared { tops, active: None, entries: vec![], trash: vec![], place_by_hand: true, loading: false, listed_meanwhile: false, relisting: false, moved: vec![] })), Arc::new(AtomicBool::new(false)));
        if let Some(r) = s.0.borrow().tops.first() { Store::set_active_dir(&r.path); }
        s.folders_changed();
        s
    }
    /// After top-level folders came or went: whether there is a top-level folder's look to use.
    fn folders_changed(&self) { crate::settings::set_has_folders(self.0.borrow().tops.first().is_some()); }
    pub fn file_task_busy(&self) -> bool { self.1.load(Ordering::Acquire) }
    pub fn begin_file_task(&self) -> Option<FileTaskGuard> {
        self.1.compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed).is_ok().then(|| FileTaskGuard(self.1.clone()))
    }

    // ---- top-level folders

    /// The top-level folders, in sidebar order: (key, name, path, error).
    pub fn top_folders(&self) -> Vec<(String, String, PathBuf, Option<String>)> {
        self.0.borrow().tops.folders.iter().map(|r: &TopFolder| (r.key.clone(), r.name(), r.path.clone(), r.error.clone())).collect()
    }
    /// Which of `entries` are the same file again through a nested top-level folder (`TopFolders::twins`):
    /// what lists every top-level folder at once shows each file once, `prefer`'s first.
    pub fn twins(&self, entries: &[Entry], prefer: &str) -> Vec<bool> { self.0.borrow().tops.twins(entries, prefer) }
    /// Whether `dir` may be added as a top-level folder (`TopFolders::check`); nothing changes.
    pub fn check_folder(&self, dir: &Path) -> std::result::Result<PathBuf, AddError> { self.0.borrow().tops.check(dir) }
    pub fn add_folder(&self, dir: &Path) -> std::result::Result<String, AddError> { let r = self.0.borrow_mut().tops.add(dir); self.folders_changed(); r }
    pub fn remove_folder(&self, key: &str) -> Result<()> {
        let mut sh = self.0.borrow_mut();
        if let Some(s) = sh.tops.store(key) { let _ = s.flush_view_state(); }
        sh.tops.remove(key)?;
        if sh.active.as_deref() == Some(key) {
            sh.active = None;
            if let Some(r) = sh.tops.first() { Store::set_active_dir(&r.path); }
        }
        drop(sh);
        self.folders_changed();
        Ok(())
    }
    pub fn retry_folder(&self, key: &str) { self.0.borrow_mut().tops.retry(key) }
    /// `folders.json` as it is now against the list here: top-level folders another program
    /// (the `quiet` CLI) added there, and the keys of those it removed (`TopFolders::changed_elsewhere`).
    pub fn changed_elsewhere(&self, now: &[fdl_core::top_folders::TopFolderEntry]) -> (Vec<fdl_core::top_folders::TopFolderEntry>, Vec<String>) { self.0.borrow().tops.changed_elsewhere(now) }
    /// Top-level folders another program added to the list, opened here; their keys.
    pub fn adopt_folders(&self, added: Vec<fdl_core::top_folders::TopFolderEntry>) -> Vec<String> {
        let keys = added.iter().map(|e| e.key.clone()).collect();
        for e in added { self.0.borrow_mut().tops.adopt(e); }
        self.folders_changed();
        keys
    }
    /// The top-level folders the window shows: those open (one gone from the disk is not shown).
    pub fn shown_tops(&self) -> Vec<(String, String, PathBuf, Option<String>)> { self.top_folders().into_iter().filter(|r| r.3.is_none()).collect() }
    /// Each top-level folder against the disk: one whose folder is gone (no `.quiet/format.json`
    /// any more) closes and leaves the window; one gone before that is back opens again.
    /// The keys lost and found.
    pub fn check_presence(&self) -> (Vec<String>, Vec<String>) {
        let mut sh = self.0.borrow_mut();
        let here = |p: &Path| p.join(crate::store::INTERNAL).join("format.json").fs_exists();
        let lost: Vec<String> = sh.tops.folders.iter().filter(|r| r.store.is_some() && !here(&r.path)).map(|r| r.key.clone()).collect();
        let back: Vec<String> = sh.tops.folders.iter().filter(|r| r.store.is_none() && here(&r.path)).map(|r| r.key.clone()).collect();
        for k in &lost { sh.tops.lose(k, crate::settings::tr("文件夹不在原来的位置了", "The folder is no longer where it was").to_string()); sh.entries.retain(|(e, _)| e != k); }
        for k in &back { sh.tops.retry(k); }
        let back = back.into_iter().filter(|k| sh.tops.store(k).is_some()).collect();
        (lost, back)
    }
    /// The window's rows from what was last listed, without reading the disk.
    pub fn listed_rows(&self) -> Vec<Entry> {
        let all: Vec<Entry> = self.0.borrow().entries.iter().flat_map(|(_, v)| v.iter().cloned()).collect();
        self.with_top_rows(all)
    }
    /// 重命名 a top-level folder: its folder on disk (2.11). Ids and the key stay.
    pub fn rename_folder_top(&self, key: &str, name: &str) -> std::result::Result<PathBuf, RenameError> {
        let mut sh = self.0.borrow_mut();
        let r = sh.tops.rename(key, name);
        // Writing `folders.json` points `active_dir` at the first top-level folder; the one in front stays in front.
        let front = sh.active.clone().and_then(|k| sh.tops.folder(&k).filter(|r| r.store.is_some()).map(|r| r.path.clone())).or_else(|| sh.tops.first().map(|r| r.path.clone()));
        if let Some(p) = front { Store::set_active_dir(&p); }
        r
    }
    pub fn can_rename_folder_top(&self, key: &str) -> bool { self.0.borrow().tops.can_rename(key) }
    pub fn folder_top_keeps_name(&self, key: &str) -> bool { self.0.borrow().tops.keeps_name(key) }
    /// Where a 快速笔记 goes: the top-level folder picked in Settings while it is open, else the first open one.
    pub fn quick_note_folder(&self) -> Option<String> { self.0.borrow().tops.quick_note_folder(&crate::settings::prefs().quick_note_folder).map(|r| r.key.clone()) }
    pub fn top_path(&self, key: &str) -> Option<PathBuf> { self.0.borrow().tops.folder(key).map(|r| r.path.clone()) }
    /// The top-level folder `id` is in (a top-level folder's row: that top-level folder).
    pub fn top_of(&self, id: &str) -> Option<String> {
        if let Some(k) = top_of_row(id) { return Some(k.to_string()); }
        self.0.borrow_mut().tops.owner(id)
    }
    /// The top-level folder holding `path`, if one is open.
    pub fn folder_of_path(&self, path: &Path) -> Option<String> { self.0.borrow().tops.folder_of_path(path).map(|r| r.key.clone()) }

    /// The document in front is `id`: new ones without a folder go to its
    /// top-level folder, and images, 数据表 and 画板 resolve against it.
    /// True when that is another top-level folder than before (its look should take over).
    pub fn set_active(&self, id: &str) -> bool {
        let key = self.top_of(id);
        key.is_some_and(|k| self.set_active_top(&k))
    }
    /// `set_active` for top-level folder `key`; true when the top-level folder in front changed.
    pub fn set_active_top(&self, key: &str) -> bool {
        let mut sh = self.0.borrow_mut();
        let Some(path) = sh.tops.folder(key).filter(|r| r.store.is_some()).map(|r| r.path.clone()) else { return false };
        let changed = Store::active_dir() != path;
        Store::set_active_dir(&path);
        sh.active = Some(key.to_string());
        changed
    }
    /// The top-level folder things go to when nothing else says: the active one, else the first.
    pub fn active_top(&self) -> Option<String> {
        let sh = self.0.borrow();
        sh.active.clone().filter(|k| sh.tops.folder(k).is_some_and(|r| r.store.is_some())).or_else(|| sh.tops.first().map(|r| r.key.clone()))
    }

    // ---- routing

    /// `f` on the library `id` is in.
    fn on<R>(&self, id: &str, f: impl FnOnce(&mut Store) -> R) -> Option<R> {
        let r = self.0.borrow_mut().tops.store_of(id).map(f);
        self.follow();
        r
    }
    /// What one of two nested top-level folders moved on disk, carried into the other before it
    /// looks (`TopFolders::follow_moves`); a top-level folder moved along waits for the windows.
    pub fn follow(&self) {
        let mut sh = self.0.borrow_mut();
        let moved = sh.tops.follow_moves();
        if moved.is_empty() { return }
        if let Some(p) = sh.active.clone().filter(|k| moved.contains(k)).and_then(|k| sh.tops.folder(&k).map(|r| r.path.clone())) { Store::set_active_dir(&p); }
        for k in moved { if !sh.moved.contains(&k) { sh.moved.push(k); } }
    }
    /// The top-level folders that moved with a folder of another since last asked (renamed through the one holding them).
    pub fn take_moved(&self) -> Vec<String> { std::mem::take(&mut self.0.borrow_mut().moved) }
    /// Whether one open top-level folder is inside another.
    pub fn nests(&self) -> bool { self.0.borrow().tops.nests() }
    /// Whether `a` and `b` are one file: the same id, or the same file listed through two nested top-level folders.
    pub fn same_file(&self, a: &str, b: &str) -> bool {
        if a == b { return true }
        let mut sh = self.0.borrow_mut();
        if top_of_row(a).is_some() || top_of_row(b).is_some() || !sh.tops.nests() { return false }
        let mut at = |id: &str| sh.tops.store_of(id).and_then(|s| s.file_path(id)).map(|p| fdl_core::top_folders::plain(p.fs_canonicalize().unwrap_or(p)));
        let x = at(a);
        x.is_some() && x == at(b)
    }
    /// The ids of `id`'s file in the other top-level folders nested with its own (none when none nest).
    pub fn twin_ids(&self, id: &str) -> Vec<String> {
        let mut sh = self.0.borrow_mut();
        if top_of_row(id).is_some() || !sh.tops.nests() { return vec![] }
        let Some(path) = sh.tops.store_of(id).and_then(|s| s.file_path(id)) else { return vec![] };
        let path = fdl_core::top_folders::plain(path.fs_canonicalize().unwrap_or(path));
        let mut out = vec![];
        for r in sh.tops.folders.iter() {
            let Some(s) = r.store.as_ref() else { continue };
            let Ok(rel) = path.strip_prefix(s.top_dir()) else { continue };
            let rel = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
            out.extend(s.ids_at(&rel).into_iter().filter(|k| k != id));
        }
        out
    }
    /// `id` was just saved: its twins' editors (`twin_ids`) show the same text, so that file is their baseline now.
    pub fn saved(&self, id: &str) {
        for t in self.twin_ids(id) { let _ = self.on(&t, |s| s.follow_saved(&t)); }
    }
    /// The open top-level folders inside folders `dirs` (deleted through the one holding them),
    /// off the sidebar; what `put_back` takes to bring them back.
    pub fn take_tops_within(&self, dirs: &[PathBuf]) -> Vec<fdl_core::top_folders::TopFolderEntry> {
        let taken = self.0.borrow_mut().tops.take_within(dirs);
        if taken.is_empty() { return taken }
        let mut sh = self.0.borrow_mut();
        sh.entries.retain(|(k, _)| !taken.iter().any(|e| e.key == *k));
        if sh.active.as_ref().is_some_and(|a| taken.iter().any(|e| e.key == *a)) { sh.active = None; if let Some(r) = sh.tops.first() { Store::set_active_dir(&r.path); } }
        drop(sh);
        self.folders_changed();
        taken
    }
    /// `take_tops_within`'s top-level folders back (their folders are back, 撤销).
    pub fn put_back(&self, entries: Vec<fdl_core::top_folders::TopFolderEntry>) {
        self.0.borrow_mut().tops.put_back(entries);
        self.folders_changed();
    }
    /// The top-level folders at or under one of folders `dirs`: (key, name).
    pub fn tops_within(&self, dirs: &[PathBuf]) -> Vec<(String, String)> {
        self.0.borrow().tops.folders.iter().filter(|r| dirs.iter().any(|d| r.path.starts_with(d))).map(|r| (r.key.clone(), r.name())).collect()
    }
    fn on_res<R>(&self, id: &str, f: impl FnOnce(&mut Store) -> Result<R>) -> Result<R> {
        self.on(id, f).unwrap_or_else(|| Err(anyhow::anyhow!("文档或文件夹不存在")))
    }
    /// `f` on the library a new entry in `parent` goes to, with `parent` as
    /// that library sees it (a top-level folder's own row is its top level).
    fn put<R>(&self, parent: Option<&str>, f: impl FnOnce(&mut Store, Option<&str>) -> Result<R>) -> Result<R> {
        let (key, real) = match parent.and_then(top_of_row) { Some(k) => (Some(k.to_string()), None), None => (match parent { Some(p) => self.top_of(p), None => self.active_top() }, parent) };
        let mut sh = self.0.borrow_mut();
        let by_hand = sh.place_by_hand;
        let s = key.and_then(|k| sh.tops.store(&k)).ok_or_else(no_folder)?;
        s.set_place_by_hand(by_hand);
        f(s, real)
    }
    /// Whether new pages get a hand-order number: only under 手动排序.
    pub fn set_place_by_hand(&self, on: bool) { self.0.borrow_mut().place_by_hand = on; }
    /// `f` on the library `id` is in, else (no id) the active top-level folder's.
    pub fn with<R>(&self, id: Option<&str>, f: impl FnOnce(&mut Store) -> Result<R>) -> Result<R> {
        match id { Some(id) => self.on_res(id, f), None => self.put(None, |s, _| f(s)) }
    }
    /// `f` on every open top-level folder's library.
    pub fn each(&self, mut f: impl FnMut(&mut Store)) {
        for r in self.0.borrow_mut().tops.folders.iter_mut() { if let Some(s) = r.store.as_mut() { f(s); } }
    }

    pub fn take_warnings(&self) -> Vec<String> { self.0.borrow_mut().tops.take_warnings() }
    pub fn list(&mut self) -> Vec<Entry> {
        let all = {
            let mut sh = self.0.borrow_mut();
            let parse = !sh.loading;
            sh.listed_meanwhile |= !parse;
            let all = sh.tops.list_with(parse);
            let keys: Vec<String> = sh.tops.folders.iter().map(|r| r.key.clone()).collect();
            sh.entries = keys.into_iter().map(|k| { let v: Vec<Entry> = all.iter().filter(|e| e.top == k).cloned().collect(); (k, v) }).collect();
            all
        };
        self.with_top_rows(all)
    }
    /// The rows as last listed, each top-level folder's under its key, without reading the
    /// disk: for a view that only looks documents up (数据表's 笔记 cells).
    pub fn listed<R>(&self, f: impl FnOnce(&[(String, Vec<Entry>)]) -> R) -> R { f(&self.0.borrow().entries) }
    /// Bring a listed row up to date after a save made outside a window's own list, for `listed`'s readers.
    pub fn update_listed(&self, id: &str, f: impl FnOnce(&mut Entry)) {
        if let Some(e) = self.0.borrow_mut().entries.iter_mut().flat_map(|r| r.1.iter_mut()).find(|e| e.id == id) { f(e); }
    }
    /// The first listing goes off the main thread: until `end_loading`,
    /// `list` reads no document (one opened from the Finder still works).
    pub fn begin_loading(&self) { let mut sh = self.0.borrow_mut(); sh.loading = true; sh.listed_meanwhile = false; }
    /// Takes over each top-level folder's `tree_writer` that listed it; true
    /// when something was written (and listed) meanwhile, so a `list` is due.
    pub fn end_loading(&self, done: Vec<(String, Store)>) -> bool {
        let mut sh = self.0.borrow_mut();
        for (key, w) in done { if let Some(s) = sh.tops.store(&key) { s.adopt(w); } }
        sh.loading = false;
        std::mem::take(&mut sh.listed_meanwhile)
    }
    pub fn loading(&self) -> bool { self.0.borrow().loading }
    /// Writers to list top-level folders `keys` again off the main thread, each with
    /// its library's `Store::writes` now; `None` while another listing runs.
    pub fn begin_relist(&self, keys: &[String]) -> Option<Vec<(String, u64, Store)>> {
        self.follow();
        let mut sh = self.0.borrow_mut();
        if sh.relisting || sh.loading { return None }
        sh.relisting = true;
        Some(keys.iter().filter_map(|k| { let s = sh.tops.store(k)?; Some((k.clone(), s.writes(), s.tree_writer())) }).collect())
    }
    /// What the writers listed: taken over where nothing was written meanwhile, else
    /// that top-level folder is listed here. The window's rows.
    pub fn end_relist(&self, done: Vec<(String, u64, Store, Vec<Entry>)>) -> Vec<Entry> {
        self.0.borrow_mut().relisting = false;
        let mut out = None;
        for (key, n, w, entries) in done {
            let fresh = {
                let mut sh = self.0.borrow_mut();
                match sh.tops.store(&key) { Some(s) if s.writes() == n => { s.adopt(w); Some(entries) } Some(s) => Some(s.list()), None => None }
            };
            if let Some(e) = fresh { out = Some(self.merge_entries(&key, e)); }
        }
        out.unwrap_or_else(|| { let all: Vec<Entry> = self.0.borrow().entries.iter().flat_map(|(_, v)| v.iter().cloned()).collect(); self.with_top_rows(all) })
    }
    /// A writer of top-level folder `key` done writing and listing (an import): its library
    /// takes over what it read and wrote, and every top-level folder's entries with `entries` for `key`.
    pub fn adopt_listed(&self, key: &str, writer: Store, entries: Vec<Entry>) -> Vec<Entry> {
        if let Some(s) = self.0.borrow_mut().tops.store(key) { s.adopt(writer); }
        self.merge_entries(key, entries)
    }
    /// Whether top-level folders show as rows of the tree: always, one or many (
    /// every top-level folder a folder under it, with a page and a "+" of its own; 2026-11-25).
    pub fn top_rows(&self) -> bool { !self.0.borrow().tops.folders.is_empty() }
    /// `entries` as the window shows them: a row for each top-level folder, holding what is at its top level.
    fn with_top_rows(&self, mut entries: Vec<Entry>) -> Vec<Entry> {
        if !self.top_rows() { return entries; }
        for e in entries.iter_mut().filter(|e| e.meta.parent.is_none() && !e.meta.local() && !e.top.is_empty()) {
            e.meta.parent = Some(top_row(&e.top));
            // Its place at the top of its top-level folder, now that it sits in the top-level folder's row.
            e.meta.order = e.meta.top_order.unwrap_or(0);
        }
        // a top-level folder gone from the disk has no row (`check_presence`)
        for (i, (key, name, _, _)) in self.shown_tops().into_iter().enumerate() {
            let meta = Meta { folder: Some(name.clone()), top_order: Some(i as u64 + 1), ..Default::default() };
            entries.push(Entry { id: top_row(&key), meta, title: name, icon: None, body: Default::default(), reminders: vec![], links: vec![], note: None, top: key });
        }
        entries
    }
    /// A `Meta` a library hands back for `id`, as the window's rows hold it
    /// (`with_top_rows`): at a top-level folder's top level, under that top-level folder's row.
    pub fn as_row(&self, id: &str, mut m: Meta) -> Meta {
        if m.parent.is_none() && !m.local() && self.top_rows() && let Some(k) = self.top_of(id) {
            m.parent = Some(top_row(&k));
            m.order = m.top_order.unwrap_or(0);
        }
        m
    }
    /// `parent` as its library sees it: a top-level folder's row is that top-level folder's top level.
    pub fn real_parent<'a>(&self, parent: Option<&'a str>) -> Option<&'a str> { parent.filter(|p| top_of_row(p).is_none()) }
    /// Every top-level folder's entries, top-level folder `key`'s replaced by `fresh` (a background
    /// writer's list of that top-level folder).
    pub fn merge_entries(&self, key: &str, mut fresh: Vec<Entry>) -> Vec<Entry> {
        for e in &mut fresh { e.top = key.to_string(); }
        let mut sh = self.0.borrow_mut();
        let order: Vec<String> = sh.tops.folders.iter().map(|r| r.key.clone()).collect();
        match sh.entries.iter_mut().find(|(k, _)| k == key) { Some((_, v)) => *v = fresh, None => sh.entries.push((key.to_string(), fresh)) }
        let mut out = vec![];
        for k in order { if let Some((_, v)) = sh.entries.iter().find(|(x, _)| *x == k) { out.extend(v.iter().cloned()); } }
        drop(sh);
        self.with_top_rows(out)
    }
    /// `merge_entries` for the 回收站.
    pub fn merge_trash(&self, key: &str, fresh: Vec<TrashEntry>) -> Vec<TrashEntry> {
        let mut sh = self.0.borrow_mut();
        let order: Vec<String> = sh.tops.folders.iter().map(|r| r.key.clone()).collect();
        match sh.trash.iter_mut().find(|(k, _)| k == key) { Some((_, v)) => *v = fresh, None => sh.trash.push((key.to_string(), fresh)) }
        let mut out = vec![];
        for k in order { if let Some((_, v)) = sh.trash.iter().find(|(x, _)| *x == k) { out.extend(v.iter().cloned()); } }
        out
    }
    pub fn load(&self, id: &str) -> Result<Document> { self.on_res(id, |s| s.load(id)) }
    /// The file of `id` while it is still only in iCloud (#406).
    pub fn cloud_only(&self, id: &str) -> Option<std::path::PathBuf> { self.on(id, |s| s.cloud_only(id)).flatten() }
    pub fn load_for_edit(&self, id: &str, shared: bool) -> Result<Document> { self.on_res(id, |s| s.load_for_edit(id, shared)) }
    pub fn forget_loaded(&self, id: &str) { self.on(id, |s| s.forget_loaded(id)); }
    pub fn recover_later(&self, doc: &Document) -> Result<()> { self.put(None, |s, _| s.recover_later(doc)) }
    pub fn save(&mut self, id: &str, doc: &Document) -> Result<Meta> { self.on_res(id, |s| s.save(id, doc)).map(|m| self.as_row(id, m)) }
    pub fn create(&mut self) -> Result<(String, Document)> { self.put(None, |s, _| s.create()) }
    pub fn create_from(&mut self, doc: &Document) -> Result<String> { self.put(None, |s, _| s.create_from(doc)) }
    pub fn size_of(&self, id: &str, doc: &Document) -> u64 { self.on(id, |s| s.size_of(id, doc)).unwrap_or(0) }
    pub fn list_trash(&mut self) -> Vec<TrashEntry> {
        let mut sh = self.0.borrow_mut();
        let mut lists = vec![];
        for r in sh.tops.folders.iter_mut() { if let Some(s) = r.store.as_mut() { lists.push((r.key.clone(), s.list_trash())); } }
        let out = lists.iter().flat_map(|(_, v)| v.iter().cloned()).collect();
        sh.trash = lists;
        out
    }
    pub fn restore(&mut self, id: &str) -> Result<bool> {
        let mut sh = self.0.borrow_mut();
        let s = sh.tops.folders.iter_mut().filter_map(|r| r.store.as_mut()).find_map(|s| s.trashed(id).then_some(s)).ok_or_else(|| anyhow::anyhow!("回收站里没有这篇文档"))?;
        s.restore(id)
    }
    /// The top-level folder whose 回收站 holds `id`.
    pub fn trash_top(&self, id: &str) -> Option<String> {
        self.0.borrow_mut().tops.folders.iter_mut().find_map(|r| r.store.as_mut().is_some_and(|s| s.trashed(id)).then(|| r.key.clone()))
    }
    pub fn set_width(&mut self, id: &str, width: PageWidth) -> Result<()> { self.on_res(id, |s| s.set_width(id, width)) }
    pub fn update_meta(&mut self, id: &str, f: impl FnOnce(&mut Meta)) -> Result<()> { self.on_res(id, |s| s.update_meta(id, f)) }
    pub fn set_catalogue(&mut self, id: &str, pinned: bool) -> Result<()> { self.on_res(id, |s| s.set_catalogue(id, pinned)) }
    /// 应用级 settings: this computer's, the same whatever top-level folder is in front.
    pub fn settings(&self) -> Settings { Store::app_settings() }
    pub fn save_settings(&self, s: &Settings) -> Result<()> { Store::save_app_settings(s) }
    /// 版本历史's snapshots kept per document: the top-level folder in front's (文件夹级, 2.1).
    pub fn history_limit(&self) -> u32 { self.put(None, |s, _| Ok(s.settings().history_limit())).unwrap_or_else(|_| fdl_core::versions::default_history_limit()) }
    pub fn set_history_limit(&self, n: u32) -> Result<()> { self.put(None, |s, _| { let mut v = s.settings(); v.set_history_limit(n); s.save_settings(&v) }) }
    /// 回收站's 自动删除 days: the top-level folder in front's; set on every one, as the 回收站 shows them all.
    pub fn trash_days(&self) -> u64 { self.put(None, |s, _| Ok(s.settings().trash_days())).unwrap_or(fdl_core::store::TRASH_DAYS) }
    pub fn set_trash_days(&self, n: u64) -> Result<()> {
        let mut out = Ok(());
        self.each(|s| { let mut v = s.settings(); v.set_trash_days(n); if let Err(e) = s.save_settings(&v) { out = Err(e) } });
        out
    }
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
    pub fn touch(&mut self, id: &str) -> Option<Meta> { self.on(id, |s| s.touch(id)).flatten().map(|m| self.as_row(id, m)) }
    pub fn create_folder(&mut self, title: &str, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_folder(title, parent)) }
    pub fn accept_file_title(&self, id: &str, title: &str) -> Result<()> { self.on_res(id, |s| s.accept_file_title(id, title)) }
    /// True when some top-level folder numbered something.
    pub fn initialize_manual_order(&mut self) -> Result<bool> {
        let mut r = Ok(false);
        self.each(|s| match s.initialize_manual_order() { Err(e) => r = Err(e), Ok(true) => if let Ok(c) = &mut r { *c = true }, Ok(false) => {} });
        r
    }
    /// One journal of file changes across every top-level folder.
    pub fn track_file_changes(&mut self) -> FileChanges {
        let mut sh = self.0.borrow_mut();
        let mut stores = sh.tops.folders.iter_mut().filter_map(|r| r.store.as_mut());
        let Some(first) = stores.next() else { return FileChanges::none() };
        let changes = first.track_file_changes();
        for s in stores { s.share_file_changes(&changes); }
        changes
    }
    /// A copy of the library `id` is in (else the active top-level folder's) and its
    /// top-level folder's key, for writes off the main thread; what it lists afterwards
    /// goes back through `merge_entries`.
    pub fn tree_writer(&self, id: Option<&str>) -> Option<(String, Store)> {
        let key = match id { Some(id) => self.top_of(id), None => self.active_top() }?;
        let mut sh = self.0.borrow_mut();
        let w = sh.tops.store(&key)?.tree_writer();
        Some((key, w))
    }
    /// `tree_writer` for a write of the tree made ahead on screen (`tree_writes.rs`), with
    /// its library's `Store::writes` now, for `adopt_written`.
    pub fn tree_writer_at(&self, id: &str) -> Option<(String, u64, Store)> {
        self.follow();
        let key = self.top_of(id)?;
        let mut sh = self.0.borrow_mut();
        let s = sh.tops.store(&key)?;
        Some((key, s.writes(), s.tree_writer()))
    }
    /// That writer done and listed: its library takes it over when nothing was written
    /// here meanwhile, else lists itself (reading what the writer left). The window's rows.
    pub fn adopt_written(&self, key: &str, n: u64, writer: Store, entries: Vec<Entry>) -> Vec<Entry> {
        let fresh = match self.0.borrow_mut().tops.store(key) { Some(s) if s.writes() == n => { s.adopt(writer); entries } Some(s) => s.list(), None => entries };
        let out = self.merge_entries(key, fresh);
        self.follow();
        out
    }
    /// The top-level folders a move of `id` into folder `parent` (as the window's rows have
    /// it) goes from and to, when they differ. A top-level folder's own row never moves.
    pub fn crossing(&self, id: &str, parent: Option<&str>) -> Option<(String, String)> {
        if top_of_row(id).is_some() { return None; }
        let (from, to) = (self.top_of(id)?, self.top_of(parent?)?);
        (from != to).then_some((from, to))
    }
    /// `Store::move_across` on the window's thread: `id` from top-level folder `from` into folder
    /// `parent` of `to` (as that library sees it).
    pub fn move_across(&self, from: &str, to: &str, id: &str, parent: Option<&str>, place: Place) -> Result<()> {
        let mut sh = self.0.borrow_mut();
        let (a, b) = sh.tops.pair(from, to).ok_or_else(no_folder)?;
        let r = a.move_across(b, id, parent, place);
        sh.tops.forget_owners();
        r
    }
    /// Pages went from one top-level folder to another (taken over from writers): which holds what is looked up again.
    pub fn moved_across(&self) { self.0.borrow_mut().tops.forget_owners(); }
    /// The open document `id` went from top-level folder `from` to `to`: its editor's baseline
    /// goes along. `fresh`: what its file holds now (an editor with edits not saved yet, whose
    /// save writes over it); else the one it had, so the next look (`refresh_external`) takes
    /// in what the move rewrote (pictures, links).
    pub fn hand_loaded(&self, from: &str, to: &str, id: &str, fresh: bool) {
        let mut sh = self.0.borrow_mut();
        let Some(old) = sh.tops.store(from).and_then(|s| s.take_loaded(id)) else { return };
        if let Some(s) = sh.tops.store(to) { s.give_loaded(id, (!fresh).then_some(old)); }
    }
    /// Writers of top-level folders `keys`, each with its library's `Store::writes` now (`adopt_written`).
    pub fn writers(&self, keys: &[String]) -> Vec<(String, u64, Store)> {
        self.follow();
        let mut sh = self.0.borrow_mut();
        keys.iter().filter_map(|k| { let s = sh.tops.store(k)?; Some((k.clone(), s.writes(), s.tree_writer())) }).collect()
    }
    /// `tree_writer` for top-level folder `key`.
    pub fn top_writer(&self, key: &str) -> Option<Store> { self.follow(); self.0.borrow_mut().tops.store(key).map(|s| s.tree_writer()) }
    pub fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<Meta> {
        self.same_top(id, parent)?;
        let parent = self.real_parent(parent);
        self.on_res(id, |s| s.set_parent(id, parent)).map(|m| self.as_row(id, m))
    }
    /// A drop in the tree, inside one top-level folder.
    pub fn move_in_tree(&mut self, id: &str, parent: Option<&str>, before: Option<&str>) -> Result<()> {
        self.same_top(id, parent)?;
        let (parent, before) = (self.real_parent(parent), self.real_parent(before));
        self.on_res(id, |s| s.move_in_tree(id, parent, before))
    }
    /// A move in one top-level folder (`set_parent`, `move_in_tree`); between two, `move_across`.
    pub fn same_top(&self, id: &str, parent: Option<&str>) -> Result<()> {
        // With top-level folders as rows, the very top is no top-level folder's: nothing goes there.
        let stray = parent.is_none() && self.top_rows();
        if stray || parent.is_some_and(|p| self.top_of(p) != self.top_of(id)) { anyhow::bail!("{}", crate::settings::tr("暂不支持在文件夹之间移动", "Moving between folders isn't supported yet")); }
        Ok(())
    }
    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> Result<()> { self.on_res(id, |s| s.set_pinned(id, pinned)) }
    pub fn is_markdown(&self, id: &str) -> bool { self.on(id, |s| s.is_markdown(id)).unwrap_or(false) }
    pub fn kind(&self, id: &str) -> crate::store::DocKind { self.on(id, |s| s.kind(id)).unwrap_or_default() }
    pub fn create_markdown(&mut self, doc: &Document, front: &str) -> Result<String> { self.put(None, |s, _| s.create_markdown(doc, front)) }
    pub fn create_markdown_in(&mut self, doc: &Document, front: &str, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_markdown_in(doc, front, parent)) }
    pub fn create_from_in(&mut self, doc: &Document, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_from_in(doc, parent)) }
    /// A child page of document `id` (#357): `id` becomes its folder's 正文, in its own top-level folder.
    pub fn create_child(&mut self, id: &str, doc: &Document) -> Result<String> { self.on_res(id, |s| s.create_child(id, doc)) }
    /// A top-level folder's own row is a folder, never a 正文: it takes no child page this way.
    pub fn can_have_children(&self, id: &str) -> bool { top_of_row(id).is_none() && self.on(id, |s| s.can_have_children(id)).unwrap_or(false) }
    pub fn create_board_in(&mut self, title: &str, board: &crate::whiteboard::Board, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_board_in(title, board, parent)) }
    #[cfg(not(target_family = "wasm"))]
    pub fn create_table_in(&mut self, title: &str, base: &crate::table::Base, parent: Option<&str>) -> Result<String> { self.put(parent, |s, parent| s.create_table_in(title, base, parent)) }
    #[cfg(target_family = "wasm")]
    pub fn create_table_in(&mut self, _title: &str, _base: &crate::table::Base, _parent: Option<&str>) -> Result<String> { anyhow::bail!("unsupported") }
    #[cfg(not(target_family = "wasm"))]
    pub fn copy_table_version(&mut self, src: &str, name: &str, title: &str) -> Result<String> { self.on_res(src, |s| s.copy_table_version(src, name, title)) }
    #[cfg(target_family = "wasm")]
    pub fn copy_table_version(&mut self, _src: &str, _name: &str, _title: &str) -> Result<String> { anyhow::bail!("unsupported") }
    #[cfg(not(target_family = "wasm"))]
    pub fn snapshot_table(&self, id: &str, force: bool) -> Result<bool> { self.on_res(id, |s| s.snapshot_table(id, force)) }
    #[cfg(target_family = "wasm")]
    pub fn snapshot_table(&self, _id: &str, _force: bool) -> Result<bool> { Ok(false) }
    /// A Markdown file opened where it is: in the top-level folder holding it, else kept
    /// with the active top-level folder's 本机文件.
    pub fn open_local(&mut self, path: &Path) -> Result<String> {
        let key = self.folder_of_path(path).or_else(|| self.active_top()).ok_or_else(no_folder)?;
        let mut sh = self.0.borrow_mut();
        sh.tops.store(&key).ok_or_else(no_folder)?.open_local(path)
    }
    pub fn local_path(&self, id: &str) -> Option<PathBuf> { self.on(id, |s| s.local_path(id)).flatten() }
    /// A folder on disk; `None` the active top-level folder itself.
    pub fn folder_dir(&self, id: Option<&str>) -> Option<PathBuf> {
        match id { Some(id) => self.on(id, |s| s.folder_dir(Some(id))).flatten(), None => self.active_top().and_then(|k| self.top_path(&k)) }
    }
    pub fn file_path(&self, id: &str) -> Option<PathBuf> { self.on(id, |s| s.file_path(id)).flatten() }
    /// Every open top-level folder by key, for the 2 s look.
    pub fn top_keys_dirs(&self) -> Vec<(String, PathBuf)> { self.0.borrow().tops.folders.iter().filter(|r| r.store.is_some()).map(|r| (r.key.clone(), r.path.clone())).collect() }
    /// Every open top-level folder, for what scans them all (unused images, wiki links).
    pub fn top_dirs(&self) -> Vec<PathBuf> { self.0.borrow().tops.folders.iter().filter(|r| r.store.is_some()).map(|r| r.path.clone()).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, BlockKind, RichText};

    /// Outer holding Inner, both top-level folders, each with a page of its own and one shared (`inner/<name>`).
    fn nested(name: &str, body: &str) -> (SharedStore, PathBuf) {
        let o = std::env::temp_dir().join(format!("quiet-shared-{}", uuid::Uuid::new_v4()));
        fdl_core::vfs::create_dir_all(&o).unwrap();
        let o = o.fs_canonicalize().unwrap();
        fdl_core::vfs::create_dir_all(o.join("inner")).unwrap();
        fdl_core::vfs::write(o.join("inner").join(name), body).unwrap();
        let mut tops = TopFolders::transient();
        tops.add(&o).unwrap(); tops.add(&o.join("inner")).unwrap();
        (SharedStore::with_top_folders(tops), o)
    }

    fn ids(s: &mut SharedStore, rel_end: &str) -> (String, String) {
        let list = s.list();
        let mut v: Vec<&Entry> = list.iter().filter(|e| e.meta.rel.ends_with(rel_end)).collect();
        v.sort_by_key(|e| e.meta.rel.len());
        assert_eq!(v.len(), 2, "{:?}", list.iter().map(|e| &e.meta.rel).collect::<Vec<_>>());
        (v[1].id.clone(), v[0].id.clone())
    }

    /// The same page through both, edited in turn as the windows do (the editors show one
    /// document, `sync_document`; each save then follows through `saved`): every edit on disk.
    fn alternate(name: &str, body: &str) {
        let (mut s, o) = nested(name, body);
        let (outer, inner) = ids(&mut s, name);
        assert!(s.same_file(&outer, &inner) && s.twin_ids(&outer) == [inner.clone()] && s.twin_ids(&inner) == [outer.clone()]);
        let mut d = s.load_for_edit(&outer, false).unwrap();
        s.load_for_edit(&inner, false).unwrap();
        for i in 0..6 {
            let id = if i % 2 == 0 { &outer } else { &inner };
            d.blocks.push(Block::new(BlockKind::Text, RichText::plain(format!("typed{i}"))));
            s.with(Some(id), |st| st.save_editor_naming_later(id, &d)).unwrap_or_else(|e| panic!("save {i}: {e}"));
            s.saved(id);
        }
        let text = fdl_core::vfs::read_to_string(o.join("inner").join(name)).unwrap();
        for i in 0..6 { assert!(text.contains(&format!("typed{i}")), "typed{i} lost: {text}"); }
    }

    #[test]
    fn a_markdown_page_typed_into_through_both_keeps_everything() { alternate("note.md", "# Note\n\nfirst\n"); }

    #[test]
    fn a_page_typed_into_through_both_keeps_everything() { alternate("Note.xml", "<title>Note</title>\n<p>first</p>\n"); }

    /// Without `saved` the second library's save of the same DocxXML text finds a file it did not load.
    #[test]
    fn without_following_the_save_the_other_refuses() {
        let (mut s, _) = nested("Note.xml", "<title>Note</title>\n<p>first</p>\n");
        let (outer, inner) = ids(&mut s, "Note.xml");
        let mut d = s.load_for_edit(&outer, false).unwrap();
        s.load_for_edit(&inner, false).unwrap();
        d.blocks.push(Block::new(BlockKind::Text, RichText::plain("a")));
        s.with(Some(&outer), |st| st.save_editor_naming_later(&outer, &d)).unwrap();
        d.blocks.push(Block::new(BlockKind::Text, RichText::plain("b")));
        assert!(s.with(Some(&inner), |st| st.save_editor_naming_later(&inner, &d)).is_err());
    }
}
