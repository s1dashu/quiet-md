//! `TopFolders`: the top-level folders the sidebar lists, all alike. Each is a library
//! of its own (`Store`, its `.quiet/` holding its index, settings, 回收站 and
//! history); which folders are open is this computer's, in `folders.json`.
//! See docs/design/workspaces.md.

use crate::store::{Entry, Store};
use crate::vfs::PathExt as _;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One line of `folders.json`. The key names what this computer keeps about
/// the top-level folder (its caches, its lock); it stays when the folder is renamed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TopFolderEntry {
    pub key: String,
    pub path: PathBuf,
    /// A macOS / iOS bookmark that finds the folder after a move (not yet written).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
    #[serde(default)]
    pub added: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct FolderList {
    v: u32,
    /// `roots` in builds before the rename (`roots.json`).
    #[serde(default, alias = "roots")]
    folders: Vec<TopFolderEntry>,
}

/// An open top-level folder: its library, or why it could not be opened (an unplugged
/// disk, a folder renamed in the Finder, a newer format).
pub struct TopFolder {
    pub key: String,
    pub path: PathBuf,
    pub store: Option<Store>,
    pub error: Option<String>,
}

impl TopFolder {
    /// What the sidebar calls it: the folder's own name.
    pub fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }
}

/// Why a folder could not be added. A folder inside an open top-level folder, or
/// holding one, may be: it is a view of its own of the same files (each keeps its
/// own `.quiet/`, and the outer one never reads the inner one's).
#[derive(Debug, PartialEq)]
pub enum AddError {
    /// Already open as this top-level folder.
    Open(String),
    /// A disk's top or the home folder: everything on it would be listed.
    TooBroad,
    /// A folder the system keeps (its programs, settings and libraries).
    System,
    Other(String),
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { AddError::Open(_) => write!(f, "这个文件夹已经打开"), AddError::TooBroad => write!(f, "请分别添加文稿、桌面等文件夹"), AddError::System => write!(f, "请添加存放笔记的文件夹"), AddError::Other(e) => write!(f, "{e}") }
    }
}

/// Why a top-level folder could not be renamed; the folder keeps its name.
#[derive(Debug, PartialEq)]
pub enum RenameError {
    /// Something else in the same folder has this name.
    Taken(String),
    Empty,
    /// No permission, or a read-only disk.
    Denied,
    /// Not open (an unplugged disk, a folder gone missing).
    Unavailable,
    /// A folder the system names, or a test run's fixed top-level folder.
    Fixed,
    Other(String),
}

#[derive(Default)]
pub struct TopFolders {
    pub folders: Vec<TopFolder>,
    /// Which top-level folder each id is in, as of the last `list`.
    owner: HashMap<String, String>,
    /// `QUIET_MD_HOME`: one top-level folder, nothing remembered.
    fixed: bool,
    /// Each library's paths by id and its `Store::writes` when last looked at (`follow_moves`).
    seen: HashMap<String, (u64, HashMap<String, String>)>,
}

/// A canonical path in its ordinary form: Windows' are `\\?\C:\…`.
pub fn plain(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")).filter(|p| !p.starts_with("UNC\\")) { Some(p) => PathBuf::from(p), None => path }
}

fn canonical(p: &Path) -> PathBuf { plain(p.fs_canonicalize().unwrap_or_else(|_| p.to_path_buf())) }

use crate::time::now;

impl TopFolders {
    /// `folders.json`: beside the other things this computer keeps, or where
    /// `QUIET_MD_FOLDERS` says (test runs with several top-level folders, which must not
    /// touch this computer's list).
    pub fn list_file() -> PathBuf {
        if let Some(f) = std::env::var_os("QUIET_MD_FOLDERS") { return PathBuf::from(f); }
        dirs::data_local_dir().unwrap_or_else(crate::vfs::temp_dir).join("quiet-md").join("folders.json")
    }

    /// Where builds before the rename kept the list (`roots.json`): read once
    /// when `folders.json` is not there yet, and left in place (older builds
    /// read it; it is also the way back). Drop the reading after 0.1.16.
    fn old_list_file() -> Option<PathBuf> {
        if std::env::var_os("QUIET_MD_FOLDERS").is_some() { return None; }
        Some(dirs::data_local_dir().unwrap_or_else(crate::vfs::temp_dir).join("quiet-md").join("roots.json"))
    }

    /// The list in `path`, if it is there and reads.
    fn read_list(path: &Path) -> Option<Vec<TopFolderEntry>> { serde_json::from_slice::<FolderList>(&crate::vfs::read(path).ok()?).ok().map(|f| f.folders) }

    /// The top-level folders as last left; the first run with top-level folders takes the library
    /// builds before it used, and the folders it had opened in place. With
    /// `QUIET_MD_HOME`, that folder alone (test runs touch nothing else).
    pub fn entries() -> (Vec<TopFolderEntry>, bool) {
        // Several top-level folders in a test run: the list in its own file, `QUIET_MD_HOME` first when it is new.
        if std::env::var_os("QUIET_MD_FOLDERS").is_some() {
            if let Some(v) = Self::read_list(&Self::list_file()) { return (v, false); }
            let home = std::env::var_os("QUIET_MD_HOME").map(|h| TopFolderEntry { key: new_key(), path: PathBuf::from(h), bookmark: None, added: now() });
            return (home.into_iter().collect(), false);
        }
        if let Some(home) = std::env::var_os("QUIET_MD_HOME") {
            // Named by its path, so two test runs never share what this computer keeps.
            let path = PathBuf::from(home);
            return (vec![TopFolderEntry { key: crate::store::library_key(&path), path, bookmark: None, added: 0 }], true);
        }
        if cfg!(target_family = "wasm") {
            let path = Store::default_dir();
            return (vec![TopFolderEntry { key: crate::store::library_key(&path), path, bookmark: None, added: 0 }], true);
        }
        if let Some(v) = Self::read_list(&Self::list_file()) { return (v, false); }
        if let Some(v) = Self::old_list_file().and_then(|p| Self::read_list(&p)) { return (v, false); }
        (Self::from_legacy(), false)
    }

    /// The library and opened folders builds before top-level folders kept.
    fn from_legacy() -> Vec<TopFolderEntry> {
        let lib = Store::legacy_dir();
        let mut out = vec![];
        if Store::is_library(&lib) {
            out.push(TopFolderEntry { key: new_key(), path: lib.clone(), bookmark: None, added: now() });
            for dir in Store::legacy_mounts(&lib) {
                if dir.fs_is_dir() && !out.iter().any(|e: &TopFolderEntry| e.path == dir) { out.push(TopFolderEntry { key: new_key(), path: dir, bookmark: None, added: now() }); }
            }
        }
        out
    }

    /// Open every top-level folder in `entries`. The library builds before top-level folders used
    /// still moves to the new layout without asking when it may (`Store::open`).
    pub fn open(entries: Vec<TopFolderEntry>, fixed: bool) -> Self {
        let legacy = Store::legacy_dir();
        Self::open_with(entries, fixed, |e| if e.path == legacy && e.path.fs_is_dir() { Store::open_library_as(e.path.clone(), &e.key) } else { Store::open_top_folder_as(&e.path, &e.key) })
    }

    /// `open`, each top-level folder's library opened by `open`: tools beside the app
    /// (the `quiet` CLI) open them as they are and leave crash copies alone.
    pub fn open_with(entries: Vec<TopFolderEntry>, fixed: bool, mut open: impl FnMut(&TopFolderEntry) -> Result<Store>) -> Self {
        let mut tops = TopFolders { fixed, ..Default::default() };
        for e in entries {
            let store = open(&e);
            tops.push(e.key, e.path, store);
        }
        tops
    }

    /// The top-level folders as last left, opened.
    pub fn load() -> Self {
        let (entries, fixed) = Self::entries();
        let first = !fixed && !Self::list_file().fs_exists();
        let tops = Self::open(entries, fixed);
        // The first run here, or the first after the rename: the list is written under its name now.
        // The old file stays as it was: a way back, and what older builds read.
        if first && (!tops.folders.is_empty() || Self::old_list_file().is_some_and(|p| p.fs_exists())) { let _ = tops.save(); }
        tops
    }

    fn push(&mut self, key: String, path: PathBuf, store: Result<Store>) {
        let (store, error) = match store {
            Ok(mut s) => {
                let open: Vec<&Store> = self.folders.iter().filter_map(|r| r.store.as_ref()).collect();
                // A folder copied whole, `.quiet/` and all: a library of its own from now on.
                let lib = s.library_id();
                if !lib.is_empty() && open.iter().any(|o| o.library_id() == lib) && let Err(e) = s.renew_library_id() { s.warnings.push(format!("{e}")); }
                let taken: HashSet<String> = open.iter().flat_map(|s| s.ids().cloned().collect::<Vec<_>>()).collect();
                if let Err(e) = s.rekey(&taken) { s.warnings.push(format!("{e}")); }
                (Some(s), None)
            }
            Err(e) => (None, Some(e.to_string())),
        };
        self.folders.push(TopFolder { key, path, store, error });
    }

    pub fn save(&self) -> Result<()> {
        if self.fixed { return Ok(()); }
        let f = FolderList { v: 1, folders: self.folders.iter().map(|r| TopFolderEntry { key: r.key.clone(), path: r.path.clone(), bookmark: None, added: 0 }).collect() };
        let path = Self::list_file();
        crate::vfs::create_dir_all(path.parent().unwrap())?;
        crate::store::write_atomic(&path, &serde_json::to_vec_pretty(&f)?)?;
        // Older builds look for one library: the first that opens.
        if let Some(r) = self.folders.iter().find(|r| r.store.is_some()) { let _ = Store::remember_dir(r.path.clone()); }
        Ok(())
    }

    /// Where `dir` stands among the open top-level folders: its canonical path when it may be
    /// added. Inside one or around one it may (`AddError`).
    pub fn check(&self, dir: &Path) -> std::result::Result<PathBuf, AddError> {
        let dir = canonical(dir);
        if disk_or_home(&dir) { return Err(AddError::TooBroad); }
        if system_folder(&dir) { return Err(AddError::System); }
        match self.folders.iter().find(|r| canonical(&r.path) == dir) { Some(r) => Err(AddError::Open(r.key.clone())), None => Ok(dir) }
    }

    /// Open folder `dir` as a new top-level folder at the end; its key.
    pub fn add(&mut self, dir: &Path) -> std::result::Result<String, AddError> {
        let dir = self.check(dir)?;
        let key = new_key();
        let store = Store::open_top_folder_as(&dir, &key).map_err(|e| AddError::Other(e.to_string()))?;
        self.push(key.clone(), dir, Ok(store));
        self.save().map_err(|e| AddError::Other(e.to_string()))?;
        Ok(key)
    }

    /// Make folder `dir` (it must not exist yet, or be empty) and open it.
    pub fn create(&mut self, dir: &Path) -> std::result::Result<String, AddError> {
        crate::vfs::create_dir_all(dir).map_err(|e| AddError::Other(e.to_string()))?;
        self.add(dir)
    }

    /// The list as `folders.json` holds it now (another program, the `quiet` CLI, may have
    /// changed it); None when it is not there or does not read.
    pub fn listed_now() -> Option<Vec<TopFolderEntry>> { Self::read_list(&Self::list_file()) }

    /// What list `now` (`listed_now`) has that this one doesn't, and the other way round:
    /// the top-level folders added elsewhere, and the keys of those removed. Nothing for a
    /// test run's fixed list. By key: a folder renamed elsewhere is not a new one.
    pub fn changed_elsewhere(&self, now: &[TopFolderEntry]) -> (Vec<TopFolderEntry>, Vec<String>) {
        if self.fixed { return (vec![], vec![]); }
        let added = now.iter().filter(|e| self.folder(&e.key).is_none()).cloned().collect();
        let removed = self.folders.iter().filter(|r| !now.iter().any(|e| e.key == r.key)).map(|r| r.key.clone()).collect();
        (added, removed)
    }

    /// Top-level folder `e`, added to the list by another program, opened here under its key
    /// (one that does not open is listed as such, as at launch). The list is not written:
    /// it already says so.
    pub fn adopt(&mut self, e: TopFolderEntry) {
        if self.folder(&e.key).is_some() { return; }
        let store = Store::open_top_folder_as(&e.path, &e.key);
        self.push(e.key, e.path, store);
    }

    /// None open, and nothing remembered (folders.json): a test's or a tool's.
    pub fn transient() -> Self { TopFolders { fixed: true, ..Default::default() } }

    /// Whether one open top-level folder is inside another.
    pub fn nests(&self) -> bool {
        let open: Vec<&Path> = self.folders.iter().filter(|r| r.store.is_some()).map(|r| r.path.as_path()).collect();
        open.iter().any(|a| open.iter().any(|b| a != b && b.starts_with(a)))
    }

    /// What a library moved on disk since this was last asked (a rename or move through the
    /// app, or one it found by inode), carried into the top-level folders nested with it
    /// (workspaces.md 2.6): what they keep about the same files (pins, order …) goes to the new
    /// paths (`Store::follow_paths`), and a top-level folder whose own folder moved (renamed
    /// through the one holding it) follows to its new path. Nothing when none nest. The keys of
    /// the top-level folders that moved.
    pub fn follow_moves(&mut self) -> Vec<String> {
        if !self.nests() { self.seen.clear(); return vec![]; }
        let abs = |top: &Path, rel: &str| rel.split('/').filter(|p| !p.is_empty()).fold(top.to_path_buf(), |p, s| p.join(s));
        let mut moves: Vec<(String, PathBuf, PathBuf)> = vec![];
        for r in &self.folders {
            let Some(s) = &r.store else { continue };
            let Some((n, was)) = self.seen.get(&r.key) else { continue };
            if *n == s.writes() { continue; }
            let now = s.paths();
            for (id, old) in was { if let Some(new) = now.get(id).filter(|n| *n != old) { moves.push((r.key.clone(), abs(s.top_dir(), old), abs(s.top_dir(), new))); } }
        }
        let mut moved = vec![];
        if !moves.is_empty() {
            for r in self.folders.iter_mut() {
                let Some(to) = moves.iter().filter(|(k, _, _)| *k != r.key).find_map(|(_, a, b)| r.path.strip_prefix(a).ok().map(|rest| if rest.as_os_str().is_empty() { b.clone() } else { b.join(rest) })) else { continue };
                if to == r.path { continue; }
                r.path = to.clone();
                if let Some(s) = r.store.as_mut() { s.moved_to(to); }
                moved.push(r.key.clone());
            }
            for k in moved.clone() { self.retry(&k); }
            for r in self.folders.iter_mut() {
                let Some(s) = r.store.as_mut() else { continue };
                let top = s.top_dir().to_path_buf();
                let mine: Vec<(PathBuf, PathBuf)> = moves.iter().filter(|(k, a, b)| *k != r.key && (a.starts_with(&top) || b.starts_with(&top))).map(|(_, a, b)| (a.clone(), b.clone())).collect();
                if let Err(e) = s.follow_paths(&mine) { s.warnings.push(format!("{e}")); }
            }
            if !moved.is_empty() { let _ = self.save(); }
        }
        // Looked at again only where something was written since (this runs before every call that writes).
        for r in &self.folders {
            let Some(s) = &r.store else { continue };
            if self.seen.get(&r.key).is_none_or(|(n, _)| *n != s.writes()) { self.seen.insert(r.key.clone(), (s.writes(), s.paths())); }
        }
        moved
    }

    /// The open top-level folders at or under one of `dirs` (a folder deleted through the one
    /// holding them), off the sidebar: the entries to put them back with (`put_back`).
    pub fn take_within(&mut self, dirs: &[PathBuf]) -> Vec<TopFolderEntry> {
        let within = |p: &Path| dirs.iter().any(|d| p.starts_with(d) || canonical(p).starts_with(canonical(d)));
        let out: Vec<TopFolderEntry> = self.folders.iter().filter(|r| within(&r.path)).map(|r| TopFolderEntry { key: r.key.clone(), path: r.path.clone(), bookmark: None, added: now() }).collect();
        for e in &out { if let Some(s) = self.store(&e.key) { let _ = s.flush_view_state(); } let _ = self.remove(&e.key); }
        out
    }

    /// `take_within`'s top-level folders back on the sidebar (their folders are back: 撤销), under their keys.
    pub fn put_back(&mut self, entries: Vec<TopFolderEntry>) {
        for e in entries { self.adopt(e); }
        let _ = self.save();
    }

    /// Off the sidebar; the folder and its `.quiet/` stay as they are.
    pub fn remove(&mut self, key: &str) -> Result<()> {
        self.folders.retain(|r| r.key != key);
        self.seen.remove(key);
        self.owner.retain(|_, k| k != key);
        self.save()
    }

    /// Top-level folder `key`'s folder is gone from the disk: its library closes, so nothing is
    /// written there again (writing would make the folder anew, empty); it stays on the list,
    /// unopened, until it is back (`retry`) or removed.
    pub fn lose(&mut self, key: &str, why: String) {
        if let Some(r) = self.folders.iter_mut().find(|r| r.key == key) { r.store = None; r.error = Some(why); }
    }

    /// Try a top-level folder that could not be opened again (the disk is back).
    pub fn retry(&mut self, key: &str) {
        let Some(r) = self.folders.iter_mut().find(|r| r.key == key && r.store.is_none()) else { return };
        match Store::open_top_folder_as(&r.path, &r.key) { Ok(s) => { r.store = Some(s); r.error = None; } Err(e) => r.error = Some(e.to_string()) }
    }

    /// 重命名 (2.11): top-level folder `key`'s folder takes `name` on disk, in the same
    /// folder as before. The key stays, so the ids the windows hold and what
    /// this computer keeps about the top-level folder stay valid. The new path.
    pub fn rename(&mut self, key: &str, name: &str) -> std::result::Result<PathBuf, RenameError> {
        if self.folder(key).is_none_or(|r| r.store.is_none()) { return Err(RenameError::Unavailable); }
        if !self.can_rename(key) { return Err(RenameError::Fixed); }
        let r = self.folders.iter_mut().find(|r| r.key == key).ok_or(RenameError::Unavailable)?;
        let store = r.store.as_mut().ok_or(RenameError::Unavailable)?;
        if name.trim().is_empty() { return Err(RenameError::Empty); }
        let name = crate::store::file_name_for(name.trim());
        let to = r.path.with_file_name(&name);
        if to == r.path { return Ok(to); }
        // Taken: anything there but the folder itself (a change of case on a disk that ignores case).
        let same = |a: &Path, b: &Path| a.file_name().map(|n| n.to_string_lossy().to_lowercase()) == b.file_name().map(|n| n.to_string_lossy().to_lowercase()) && canonical(a) == canonical(b);
        if crate::vfs::symlink_metadata(&to).is_ok() && !same(&r.path, &to) { return Err(RenameError::Taken(name)); }
        store.flush_view_state().map_err(|e| RenameError::Other(e.to_string()))?;
        crate::vfs::rename(&r.path, &to).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem => RenameError::Denied,
            std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::DirectoryNotEmpty => RenameError::Taken(name.clone()),
            _ => RenameError::Other(e.to_string()),
        })?;
        store.moved_to(to.clone());
        // The default folder (the library's location): launches and new libraries follow it.
        let was_default = follows_default(&r.path, &Store::legacy_dir());
        r.path = to.clone();
        if was_default && std::env::var_os("QUIET_MD_HOME").is_none() { let _ = Store::remember_dir(to.clone()); }
        let _ = self.save();
        Ok(to)
    }

    /// Whether top-level folder `key` may be renamed from the sidebar: open, not the folder a
    /// test run or preview was started on (`QUIET_MD_HOME`), and not a folder the system names
    /// (home, Desktop, Documents, Downloads …). Folders added in such a run rename as usual.
    pub fn can_rename(&self, key: &str) -> bool {
        // A scripted launch's environment: unit tests are not that run, whatever their shell sets.
        // With `QUIET_MD_FOLDERS` the run keeps its own list, which follows a rename.
        let started_on = if cfg!(test) || std::env::var_os("QUIET_MD_FOLDERS").is_some() { None } else { std::env::var_os("QUIET_MD_HOME").map(PathBuf::from) };
        self.can_rename_beside(key, started_on.as_deref())
    }

    /// `can_rename`, `started_on` the folder the run's launch named, which stays where it put it:
    /// it is where the run's library lives (a preview's is the real one), and renaming it would
    /// move that.
    fn can_rename_beside(&self, key: &str, started_on: Option<&Path>) -> bool {
        if cfg!(target_family = "wasm") { return false; }
        let Some(r) = self.folder(key).filter(|r| r.store.is_some()) else { return false };
        if started_on.is_some_and(|h| canonical(h) == canonical(&r.path)) { return false; }
        let system = [dirs::home_dir(), dirs::desktop_dir(), dirs::document_dir(), dirs::download_dir(), dirs::picture_dir(), dirs::audio_dir(), dirs::video_dir(), dirs::public_dir()];
        r.path.parent().is_some() && !system.into_iter().flatten().any(|d| canonical(&d) == canonical(&r.path))
    }

    /// The top-level folders in the order of `keys` (the sidebar's, dragged by hand), written to
    /// `folders.json`; one not named keeps its place after them.
    pub fn arrange(&mut self, keys: &[String]) -> Result<()> {
        let at = |k: &str| keys.iter().position(|x| x == k).unwrap_or(keys.len());
        self.folders.sort_by_key(|r| at(&r.key));
        self.save()
    }

    pub fn folder(&self, key: &str) -> Option<&TopFolder> { self.folders.iter().find(|r| r.key == key) }

    /// Why top-level folder `key` keeps its name, for the menu: the folder a test run or preview
    /// was started on (`QUIET_MD_HOME`), or a folder the system names. None: it may be renamed (or is not open).
    pub fn keeps_name(&self, key: &str) -> bool { self.folder(key).is_some_and(|r| r.store.is_some()) && !self.can_rename(key) }

    /// The first top-level folder that opened: where things go when nothing says otherwise.
    pub fn first(&self) -> Option<&TopFolder> { self.folders.iter().find(|r| r.store.is_some()) }

    /// Where a 快速笔记 goes (2.8): top-level folder `chosen` (Settings › 文档库) while it
    /// is open, else the first that is; `None` with no top-level folder open.
    pub fn quick_note_folder(&self, chosen: &str) -> Option<&TopFolder> {
        self.folders.iter().find(|r| r.key == chosen && r.store.is_some()).or_else(|| self.first())
    }

    /// The key of the top-level folder `id` is in.
    pub fn owner(&mut self, id: &str) -> Option<String> {
        if let Some(k) = self.owner.get(id) { return Some(k.clone()); }
        let k = self.folders.iter().find(|r| r.store.as_ref().is_some_and(|s| s.has(id)))?.key.clone();
        self.owner.insert(id.to_string(), k.clone());
        Some(k)
    }

    /// The library `id` is in.
    pub fn store_of(&mut self, id: &str) -> Option<&mut Store> {
        let key = self.owner(id)?;
        self.folders.iter_mut().find(|r| r.key == key)?.store.as_mut()
    }

    pub fn store(&mut self, key: &str) -> Option<&mut Store> { self.folders.iter_mut().find(|r| r.key == key)?.store.as_mut() }

    /// Top-level folders `a` and `b` (two different ones) at once, for a move from one to the other.
    pub fn pair(&mut self, a: &str, b: &str) -> Option<(&mut Store, &mut Store)> {
        let (mut x, mut y) = (None, None);
        for r in self.folders.iter_mut() {
            if r.key == a { x = r.store.as_mut(); } else if r.key == b { y = r.store.as_mut(); }
        }
        Some((x?, y?))
    }

    /// Pages moved from one top-level folder to another: who holds what is looked up again.
    pub fn forget_owners(&mut self) { self.owner.clear(); }

    /// The top-level folder holding `path`, if one does; of two nested ones, the inner.
    pub fn folder_of_path(&self, path: &Path) -> Option<&TopFolder> {
        let path = canonical(path);
        self.folders.iter().map(|r| (r, canonical(&r.path))).filter(|(_, p)| path.starts_with(p)).max_by_key(|(_, p)| p.components().count()).map(|(r, _)| r)
    }

    /// Which of `entries` (as `list` gave them) are another's twin: the same file listed again
    /// through a top-level folder nested in another (or holding one). Of each file one stays:
    /// the one in top-level folder `prefer`, else the one in the innermost. All false when no
    /// two top-level folders nest. Folders (and pages) are told apart by their path on disk.
    pub fn twins(&self, entries: &[Entry], prefer: &str) -> Vec<bool> {
        let mut out = vec![false; entries.len()];
        let dirs: HashMap<&str, PathBuf> = self.folders.iter().filter(|r| r.store.is_some()).map(|r| (r.key.as_str(), canonical(&r.path))).collect();
        if !dirs.values().any(|a| dirs.values().any(|b| a != b && b.starts_with(a))) { return out; }
        let depth = |k: &str| dirs.get(k).map_or(0, |p| p.components().count());
        let mut kept: HashMap<PathBuf, usize> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            let Some(dir) = dirs.get(e.top.as_str()) else { continue };
            if e.meta.local() || e.meta.rel.is_empty() { continue; }
            let at = dir.join(&e.meta.rel);
            let better = |j: usize| { let o = &entries[j]; (e.top == prefer, depth(&e.top)) > (o.top == prefer, depth(&o.top)) };
            match kept.get(&at).copied() {
                None => { kept.insert(at, i); }
                Some(j) if better(j) => { out[j] = true; kept.insert(at, i); }
                Some(_) => out[i] = true,
            }
        }
        out
    }

    /// Every top-level folder's entries, each marked with its top-level folder.
    pub fn list(&mut self) -> Vec<Entry> { self.list_with(true) }

    /// `list`; with `parse` off, documents not read yet go by their file's name (`Store::list_with`).
    pub fn list_with(&mut self, parse: bool) -> Vec<Entry> {
        // Moves made through one of two nested top-level folders reach the other before it looks.
        self.follow_moves();
        let mut out = vec![];
        self.owner.clear();
        for r in &mut self.folders {
            let Some(s) = r.store.as_mut() else { continue };
            for mut e in s.list_with(None, parse) {
                self.owner.insert(e.id.clone(), r.key.clone());
                e.top = r.key.clone();
                out.push(e);
            }
        }
        // And what one found moved in the Finder (by inode) reaches the other.
        if !self.follow_moves().is_empty() { return self.list_with(parse); }
        out
    }

    /// Problems the top-level folders met while opening or listing, each once.
    pub fn take_warnings(&mut self) -> Vec<String> {
        self.folders.iter_mut().filter_map(|r| r.store.as_mut()).flat_map(|s| std::mem::take(&mut s.warnings)).collect()
    }
}

fn new_key() -> String { uuid::Uuid::new_v4().to_string() }

/// A disk's top or the home folder (`dir` canonical): never a top-level folder, from the
/// sidebar or the command line.
fn disk_or_home(dir: &Path) -> bool { dir.parent().is_none() || dirs::home_dir().is_some_and(|h| canonical(&h) == dir) }

/// A folder the system keeps (`dir` canonical): its programs, settings and libraries. Never a
/// top-level folder, from the sidebar or the command line.
fn system_folder(dir: &Path) -> bool {
    let system: &[&str] = if cfg!(windows) { &[r"C:\Windows", r"C:\Program Files", r"C:\Program Files (x86)", r"C:\ProgramData"] }
        else { &["/System", "/Library", "/Applications", "/usr", "/bin", "/sbin", "/etc", "/var", "/private", "/opt", "/dev", "/proc", "/sys", "/boot", "/tmp"] };
    system.iter().any(|s| { let s = canonical(Path::new(s)); dir == s })
        || dirs::home_dir().is_some_and(|h| [h.join("Library"), h.join("AppData")].iter().any(|l| dir == canonical(l)))
}

/// A folder too broad to open as a top-level folder: a disk's top, the home folder, or one the
/// system keeps (`TopFolders::check` says which).
pub fn too_broad(dir: &Path) -> bool { let dir = canonical(dir); disk_or_home(&dir) || system_folder(&dir) }

/// Whether folder `path` is the library's location `legacy`, the default folder.
fn follows_default(path: &Path, legacy: &Path) -> bool { path == legacy || canonical(path) == canonical(legacy) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, RichText};

    fn dir(name: &str) -> PathBuf {
        let d = crate::vfs::temp_dir().join(format!("quiet-tops-{name}-{}", uuid::Uuid::new_v4()));
        crate::vfs::create_dir_all(&d).unwrap();
        d
    }

    fn doc(t: &str) -> Document { Document { title: RichText::plain(t), ..Default::default() } }

    #[test]
    fn folders_are_libraries_of_their_own() {
        let (a, b) = (dir("a"), dir("b"));
        crate::vfs::write(b.join("note.md"), "# note\n").unwrap();
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let ka = tops.add(&a).unwrap();
        let kb = tops.add(&b).unwrap();
        assert!(under(&a, ".quiet/format.json").fs_is_file() && under(&b, ".quiet/format.json").fs_is_file(), "every top-level folder has its .quiet/");
        let made = tops.store(&ka).unwrap().create_from(&doc("in a")).unwrap();
        let list = tops.list();
        assert_eq!(list.iter().find(|e| e.id == made).unwrap().top, ka);
        assert_eq!(list.iter().find(|e| e.title == "note").unwrap().top, kb);
        assert_eq!(tops.owner(&made).as_deref(), Some(ka.as_str()));
        // The same folder again: already open.
        assert_eq!(tops.add(&a), Err(AddError::Open(ka.clone())));
        assert_eq!(tops.add(&a.join(".")), Err(AddError::Open(ka.clone())), "however written");
        // Removed: the folder stays, and comes back as it was.
        tops.remove(&ka).unwrap();
        assert!(a.join("in a.xml").fs_is_file() || a.join("in a.md").fs_is_file());
        let back = tops.add(&a).unwrap();
        assert!(tops.list().iter().any(|e| e.id == made && e.top == back), "same id after reopening");
    }

    /// The sidebar's headings dragged into another order (`arrange`): one not named keeps its place after them.
    #[test]
    fn folders_take_the_order_they_are_dragged_into() {
        let (a, b, c) = (dir("ord-a"), dir("ord-b"), dir("ord-c"));
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let [ka, kb, kc] = [&a, &b, &c].map(|d| tops.add(d).unwrap());
        let keys = |t: &TopFolders| t.folders.iter().map(|r| r.key.clone()).collect::<Vec<_>>();
        tops.arrange(&[kc.clone(), ka.clone(), kb.clone()]).unwrap();
        assert_eq!(keys(&tops), [kc.clone(), ka.clone(), kb.clone()]);
        tops.arrange(&[kb.clone(), kc.clone()]).unwrap();
        assert_eq!(keys(&tops), [kb, kc, ka]);
    }

    #[test]
    fn a_folder_gone_from_the_disk_is_never_written_again() {
        let (a, b) = (dir("gone"), dir("stays"));
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let ka = tops.add(&a).unwrap();
        let kb = tops.add(&b).unwrap();
        let made = tops.store(&ka).unwrap().create_from(&doc("in a")).unwrap();
        // Moved away while open (in the Finder): the window finds it gone and closes its library.
        let moved = a.with_file_name(format!("{}-moved", a.file_name().unwrap().to_string_lossy()));
        crate::vfs::rename(&a, &moved).unwrap();
        tops.lose(&ka, "gone".into());
        assert!(tops.store(&ka).is_none(), "no library left to write with");
        // Whatever the app does meanwhile, nothing appears at the old path, not even an empty .quiet/.
        let _ = tops.list();
        tops.save().unwrap();
        tops.store(&kb).unwrap().create_from(&doc("in b")).unwrap();
        tops.retry(&ka);
        assert!(tops.store(&ka).is_none() && !a.fs_exists(), "the folder is not made anew at its old path");
        assert!(!tops.list().iter().any(|e| e.top == ka), "its documents are not listed");
        // Back where it was: it opens again, as it was.
        crate::vfs::rename(&moved, &a).unwrap();
        tops.retry(&ka);
        assert!(tops.store(&ka).is_some());
        assert!(tops.list().iter().any(|e| e.id == made && e.top == ka));
    }

    #[test]
    fn a_copied_folder_gets_ids_of_its_own() {
        let a = dir("orig");
        let mut s = Store::open_top_folder(&a).unwrap();
        s.create_from(&doc("one")).unwrap();
        s.list();
        drop(s);
        let b = dir("copy");
        crate::vfs::copy(a.join("one.xml"), b.join("one.xml")).unwrap();
        let own = format!("meta/{}", crate::store::device().id);
        crate::vfs::create_dir_all(b.join(".quiet").join(&own)).unwrap();
        crate::vfs::copy(under(&a, ".quiet/format.json"), under(&b, ".quiet/format.json")).unwrap();
        for e in crate::vfs::read_dir(a.join(".quiet").join(&own)).unwrap().flatten() { crate::vfs::copy(e.path(), b.join(".quiet").join(&own).join(e.file_name())).unwrap(); }
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        tops.add(&a).unwrap();
        tops.add(&b).unwrap();
        let list = tops.list();
        let ids: HashSet<&String> = list.iter().map(|e| &e.id).collect();
        assert_eq!(ids.len(), list.len(), "no id in two top-level folders");
        let (la, lb) = (tops.folders[0].store.as_ref().unwrap().library_id(), tops.folders[1].store.as_ref().unwrap().library_id());
        assert!(!la.is_empty() && la != lb, "the copy is a library of its own");
    }

    #[test]
    fn quick_notes_go_to_the_chosen_folder_while_it_is_open() {
        let (a, b) = (dir("qa"), dir("qb"));
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let ka = tops.add(&a).unwrap();
        let kb = tops.add(&b).unwrap();
        let at = |tops: &TopFolders, chosen: &str| tops.quick_note_folder(chosen).map(|r| r.key.clone());
        assert_eq!(at(&tops, ""), Some(ka.clone()), "nothing chosen: the first top-level folder");
        assert_eq!(at(&tops, &kb), Some(kb.clone()));
        assert_eq!(at(&tops, "gone"), Some(ka.clone()), "a top-level folder no longer listed: the first");
        // The chosen top-level folder can't be opened (an unplugged disk): the first that can.
        tops.folders[1].store = None;
        assert_eq!(at(&tops, &kb), Some(ka.clone()));
        // The first can't either: the next one that can.
        tops.folders[1].store = Some(Store::open_top_folder(&b).unwrap());
        tops.folders[0].store = None;
        assert_eq!(at(&tops, &ka), Some(kb.clone()));
        tops.folders[1].store = None;
        assert_eq!(at(&tops, &kb), None, "none open: the empty state");
        assert!(TopFolders::default().quick_note_folder("").is_none());
    }

    #[test]
    fn renaming_the_default_folder_moves_the_default() {
        let d = dir("follows-default");
        let lib = d.join("Quiet.md");
        crate::vfs::create_dir_all(&lib).unwrap();
        assert!(follows_default(&lib, &lib) && follows_default(&lib.join("."), &lib), "the same folder, however written");
        assert!(!follows_default(&d.join("Other"), &lib));
    }

    #[test]
    fn a_run_keeps_the_folder_it_was_started_on_and_renames_the_others() {
        let (home, other) = (dir("started-on").join("lib"), dir("added").join("vault"));
        crate::vfs::create_dir_all(&home).unwrap();
        crate::vfs::create_dir_all(&other).unwrap();
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let (kh, ko) = (tops.add(&home).unwrap(), tops.add(&other).unwrap());
        assert!(!tops.can_rename_beside(&kh, Some(&home)) && !tops.can_rename_beside(&kh, Some(&home.join("."))), "the run's own folder, however written");
        assert!(tops.can_rename_beside(&ko, Some(&home)), "a folder added in the run renames");
        assert!(tops.can_rename_beside(&kh, None) && tops.can_rename_beside(&ko, None), "an ordinary launch renames both");
    }

    #[test]
    fn a_renamed_folder_keeps_its_key_ids_and_data() {
        // In a folder of their own: the new names are looked for beside them.
        let (a, b) = (dir("ra").join("a"), dir("rb"));
        crate::vfs::create_dir_all(&a).unwrap();
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let ka = tops.add(&a).unwrap();
        tops.add(&b).unwrap();
        let id = tops.store(&ka).unwrap().create_from(&doc("kept")).unwrap();
        tops.store(&ka).unwrap().touch(&id);
        tops.list();
        let to = tops.rename(&ka, " renamed/root ").unwrap();
        assert_eq!(to.file_name().unwrap().to_str(), Some(crate::store::file_name_for("renamed/root").as_str()), "the same name rules as a document's file");
        assert_ne!(to.file_name().unwrap(), "renamed/root");
        assert!(!a.fs_exists() && under(&to, ".quiet/format.json").fs_is_file(), "the folder itself moved");
        assert_eq!(tops.folder(&ka).unwrap().path, to);
        assert_eq!(tops.store(&ka).unwrap().top_dir(), to.as_path());
        let list = tops.list();
        let e = list.iter().find(|e| e.id == id).expect("same id after the rename");
        assert_eq!((e.top.as_str(), e.title.as_str()), (ka.as_str(), "kept"));
        assert!(e.meta.opened > 0, "this computer's 最近打开 still found by the key");
        assert!(tops.store_of(&id).unwrap().load(&id).is_ok());
        // Taken by a sibling, empty, or not open: the name stays.
        crate::vfs::create_dir_all(to.with_file_name("other")).unwrap();
        assert_eq!(tops.rename(&ka, "other"), Err(RenameError::Taken("other".into())));
        assert_eq!(tops.rename(&ka, "  "), Err(RenameError::Empty));
        assert_eq!(tops.rename("nope", "x"), Err(RenameError::Unavailable));
        assert_eq!(tops.folder(&ka).unwrap().path, to);
        // A change of case only is a rename of the same folder.
        let upper = tops.rename(&ka, &to.file_name().unwrap().to_string_lossy().to_uppercase()).unwrap();
        assert!(upper != to && upper.fs_is_dir() && tops.folder(&ka).unwrap().path == upper);
    }

    #[test]
    fn data_kept_by_path_moves_to_the_key_once() {
        let a = dir("adopt");
        let mut s = Store::open_top_folder(&a).unwrap();
        let id = s.create_from(&doc("one")).unwrap();
        s.touch(&id);
        s.list();
        s.recover_later(&doc("unsaved")).unwrap();
        drop(s);
        let key = new_key();
        let mut tops = TopFolders::open(vec![TopFolderEntry { key: key.clone(), path: a.clone(), bookmark: None, added: 0 }], true);
        let s = tops.store(&key).unwrap();
        assert_eq!(s.key(), key);
        let list = s.list();
        assert!(list.iter().any(|e| e.title == "unsaved (recovered copy)"), "the recovery copy kept by path comes back");
        assert!(list.iter().find(|e| e.id == id).unwrap().meta.opened > 0, "最近打开 kept by path is found");
        drop(tops);
        // Taken over once: the next launch neither restores nor loses anything.
        let mut tops = TopFolders::open(vec![TopFolderEntry { key: key.clone(), path: a.clone(), bookmark: None, added: 0 }], true);
        assert_eq!(tops.list().len(), 2);
        if let Some(d) = dirs::data_local_dir() { let _ = crate::vfs::remove_dir_all(under(&d, "quiet-md/recovery").join(&key)); }
    }

    /// A list written before the rename (`roots.json`, its `roots`) still reads.
    #[test]
    fn the_list_from_before_the_rename_reads() {
        let d = dir("old-list");
        let old = d.join("roots.json");
        crate::vfs::write(&old, r#"{"v":1,"roots":[{"key":"k1","path":"/tmp/a"}]}"#).unwrap();
        let v = TopFolders::read_list(&old).unwrap();
        assert_eq!((v.len(), v[0].key.as_str()), (1, "k1"));
    }

    /// The `quiet` CLI added one top-level folder to `folders.json` and removed another: the app's
    /// list sees both, opens the new one under the key written there, and writes nothing.
    #[test]
    fn a_list_changed_elsewhere_is_taken_in() {
        let (a, b, c) = (dir("ea"), dir("eb"), dir("ec"));
        // Fixed while it is built (nothing written to this computer's list), then following it.
        let mut app = TopFolders { fixed: true, ..Default::default() };
        let ka = app.add(&a).unwrap();
        let kb = app.add(&b).unwrap();
        app.fixed = false;
        let mut now: Vec<TopFolderEntry> = app.folders.iter().filter(|r| r.key != kb).map(|r| TopFolderEntry { key: r.key.clone(), path: r.path.clone(), bookmark: None, added: 0 }).collect();
        now.push(TopFolderEntry { key: "from-cli".into(), path: c.clone(), bookmark: None, added: 0 });
        let (added, removed) = app.changed_elsewhere(&now);
        assert_eq!((added.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), removed), (vec!["from-cli"], vec![kb.clone()]));
        for e in added { app.adopt(e); }
        assert!(app.store("from-cli").is_some() && under(&c, ".quiet/format.json").fs_is_file());
        app.fixed = true;
        app.remove(&kb).unwrap();
        // A test run's fixed list follows nothing.
        assert_eq!(app.changed_elsewhere(&[]).1.len(), 0);
        app.fixed = false;
        let (added, removed) = app.changed_elsewhere(&now);
        assert!(added.is_empty() && removed.is_empty(), "taken in: nothing left to do");
        assert!(app.folder(&ka).is_some());
    }

    #[test]
    fn a_disk_or_the_home_folder_is_never_added() {
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let top = if cfg!(windows) { PathBuf::from(r"C:\") } else { PathBuf::from("/") };
        assert_eq!(tops.add(&top), Err(AddError::TooBroad));
        if let Some(h) = dirs::home_dir() { assert_eq!(tops.check(&h), Err(AddError::TooBroad)); assert_eq!(tops.check(&h.join(".")), Err(AddError::TooBroad)); }
        assert!(tops.folders.is_empty());
        assert!(!AddError::TooBroad.to_string().is_empty());
    }

    /// A inside B, B inside A: both open, each a library of its own; the outer one
    /// lists the inner one's pages and never its `.quiet/`, and what each keeps
    /// about a page (pins, 最近打开) is its own.
    #[test]
    fn nested_folders_open_side_by_side() {
        let outer = dir("outer");
        let inner = outer.join("inner");
        crate::vfs::create_dir_all(inner.join("deep")).unwrap();
        crate::vfs::write(inner.join("note.md"), "# note\n").unwrap();
        crate::vfs::write(under(&inner, "deep/more.md"), "# more\n").unwrap();
        crate::vfs::write(outer.join("top.md"), "# top\n").unwrap();
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let ko = tops.add(&outer).unwrap();
        let ki = tops.add(&inner).unwrap();
        // And the other way round: one around an open one.
        let around = dir("around");
        let within = around.join("within");
        crate::vfs::create_dir_all(&within).unwrap();
        crate::vfs::write(within.join("w.md"), "# w\n").unwrap();
        let kw = tops.add(&within).unwrap();
        let ka = tops.add(&around).unwrap();
        assert!(under(&inner, ".quiet/format.json").fs_is_file() && under(&outer, ".quiet/format.json").fs_is_file());
        let list = tops.list();
        let of = |k: &str| { let mut v: Vec<String> = list.iter().filter(|e| e.top == k).map(|e| e.meta.rel.clone()).collect(); v.sort(); v };
        assert_eq!(of(&ko), ["inner", "inner/deep", "inner/deep/more.md", "inner/note.md", "top.md"], "the inner .quiet/ is not content");
        assert_eq!(of(&ki), ["deep", "deep/more.md", "note.md"]);
        assert_eq!((of(&kw), of(&ka)), (vec!["w.md".to_string()], vec!["within".to_string(), "within/w.md".to_string()]));
        let ids: HashSet<&String> = list.iter().map(|e| &e.id).collect();
        assert_eq!(ids.len(), list.len(), "the same file has an id in each, never the same one");
        // Each keeps its own: a pin in the inner one is not the outer one's.
        let (io, ii) = (list.iter().find(|e| e.top == ko && e.meta.rel == "inner/note.md").unwrap().id.clone(), list.iter().find(|e| e.top == ki && e.meta.rel == "note.md").unwrap().id.clone());
        tops.store(&ki).unwrap().set_pinned(&ii, true).unwrap();
        tops.store(&ko).unwrap().touch(&io);
        let list = tops.list();
        let get = |id: &str| list.iter().find(|e| e.id == id).unwrap().meta.clone();
        assert!(get(&ii).pinned && !get(&io).pinned);
        assert!(get(&io).opened > 0 && get(&ii).opened == 0);
        assert_eq!(tops.owner(&ii).as_deref(), Some(ki.as_str()));
        assert_eq!(tops.folder_of_path(&inner.join("note.md")).map(|r| r.key.clone()), Some(ki.clone()), "the inner one holds it nearest");
        assert_eq!(tops.folder_of_path(&outer.join("top.md")).map(|r| r.key.clone()), Some(ko.clone()));
        // Twins: one row per file in what lists them all, the preferred top-level folder's first.
        let twins = tops.twins(&list, &ko);
        let kept: Vec<(String, String)> = list.iter().zip(&twins).filter(|(_, t)| !**t).map(|(e, _)| (e.top.clone(), e.meta.rel.clone())).collect();
        assert_eq!(kept.len(), list.len() - 4, "note, deep, more and w once each");
        assert!(kept.contains(&(ko.clone(), "inner/note.md".into())) && !kept.contains(&(ki.clone(), "note.md".into())));
        let twins = tops.twins(&list, "");
        assert!(list.iter().zip(&twins).any(|(e, t)| !t && e.top == ki && e.meta.rel == "note.md"), "none preferred: the inner one");
        // Removing the inner one leaves the outer one as it was.
        tops.remove(&ki).unwrap();
        assert!(tops.list().iter().any(|e| e.id == io));
    }

    /// Outer folder O with I inside it, both open, a page in each and one in I's part of O.
    fn nested() -> (TopFolders, PathBuf, String, String) {
        let o = canonical(&dir("o"));
        crate::vfs::create_dir_all(o.join("inner")).unwrap();
        crate::vfs::write(under(&o, "inner/note.md"), "# note\n").unwrap();
        crate::vfs::write(o.join("top.md"), "# top\n").unwrap();
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let ko = tops.add(&o).unwrap();
        let ki = tops.add(&o.join("inner")).unwrap();
        tops.list();
        (tops, o, ko, ki)
    }
    fn id_at(tops: &mut TopFolders, k: &str, rel: &str) -> Option<String> { tops.list().into_iter().find(|e| e.top == k && e.meta.rel == rel).map(|e| e.id) }
    fn rels(tops: &mut TopFolders, k: &str) -> Vec<String> { let mut v: Vec<String> = tops.list().into_iter().filter(|e| e.top == k).map(|e| e.meta.rel).collect(); v.sort(); v }

    /// Moving across nested top-level folders: to where the file already is, nothing
    /// moves (no copy, no " 2", nothing removed); into itself, refused; elsewhere, an
    /// ordinary move that the other side lists again, once.
    #[test]
    fn a_move_between_nested_folders_to_where_it_is_changes_nothing() {
        let (mut tops, o, ko, ki) = nested();
        let (before_o, before_i) = (rels(&mut tops, &ko), rels(&mut tops, &ki));
        let note = id_at(&mut tops, &ko, "inner/note.md").unwrap();
        let (a, b) = tops.pair(&ko, &ki).unwrap();
        let e = a.move_across(b, &note, None, crate::store::Place::Before(None)).unwrap_err();
        assert_eq!(e.to_string(), "已在这个文件夹里");
        assert_eq!(crate::vfs::read_to_string(under(&o, "inner/note.md")).unwrap(), "# note\n");
        assert!(!under(&o, "inner/note 2.md").fs_exists());
        let folder = id_at(&mut tops, &ko, "inner").unwrap();
        let (a, b) = tops.pair(&ko, &ki).unwrap();
        assert_eq!(a.move_across(b, &folder, None, crate::store::Place::Before(None)).unwrap_err().to_string(), "不能移动到自身或后代文件夹中");
        assert!(under(&o, "inner/.quiet/format.json").fs_is_file() && !under(&o, "inner/inner").fs_exists());
        assert_eq!((rels(&mut tops, &ko), rels(&mut tops, &ki)), (before_o, before_i.clone()), "both as they were");
        // The other way: I's page to its own folder in O's view of it.
        let ni = id_at(&mut tops, &ki, "note.md").unwrap();
        let fo = id_at(&mut tops, &ko, "inner").unwrap();
        let (b, a) = tops.pair(&ki, &ko).unwrap();
        assert_eq!(b.move_across(a, &ni, Some(&fo), crate::store::Place::Before(None)).unwrap_err().to_string(), "已在这个文件夹里");
        assert_eq!(rels(&mut tops, &ki), before_i);
        // O's own page into I: on disk now in I's folder; each lists it once.
        let top = id_at(&mut tops, &ko, "top.md").unwrap();
        let (a, b) = tops.pair(&ko, &ki).unwrap();
        a.move_across(b, &top, None, crate::store::Place::Before(None)).unwrap();
        tops.forget_owners();
        assert!(under(&o, "inner/top.md").fs_is_file() && !o.join("top.md").fs_exists());
        assert_eq!(rels(&mut tops, &ki), ["note.md", "top.md"]);
        assert_eq!(rels(&mut tops, &ko), ["inner", "inner/note.md", "inner/top.md"]);
        assert_eq!(tops.owner(&top).as_deref(), Some(ki.as_str()), "the moved id is I's now");
        // And out of I to O's top: I lets it go, O has it once.
        let (b, a) = tops.pair(&ki, &ko).unwrap();
        b.move_across(a, &top, None, crate::store::Place::Before(None)).unwrap();
        tops.forget_owners();
        assert_eq!(rels(&mut tops, &ki), ["note.md"]);
        assert_eq!(rels(&mut tops, &ko), ["inner", "inner/note.md", "top.md"]);
        assert_eq!(tops.list().iter().filter(|e| e.id == top).count(), 1);
    }

    /// `rel` (written with `/`) under folder `dir`, as this system names it.
    fn under(dir: &Path, rel: &str) -> PathBuf { crate::paths::resolve(rel, dir) }

    fn with_picture(title: &str, src: &Path) -> Document {
        let mut d = doc(title);
        d.blocks.push(crate::model::Block::new(crate::model::BlockKind::Image { src_kind: "src".into(), src: src.to_string_lossy().into_owned(), width: None, height: None, caption: None, crop: None }, Default::default()));
        d
    }
    fn pictures(tops: &mut TopFolders, k: &str, id: &str) -> Vec<String> { tops.store(k).unwrap().load(id).unwrap().asset_sources() }

    /// A page in I's part of O names its pictures from its library's top (`.assets/x.png`):
    /// through either top-level folder it shows the same file, whichever one put it there.
    #[test]
    fn a_nested_page_shows_its_pictures_through_both() {
        let (mut tops, o, ko, ki) = nested();
        let png = o.join("p.png");
        crate::vfs::write(&png, b"png").unwrap();
        // Through I: into I's .assets/.
        let s = tops.store(&ki).unwrap();
        let at = crate::paths::resolve(&s.import_attachment(&png).unwrap(), &o.join("inner"));
        let by_i = s.create_from(&with_picture("by i", &at)).unwrap();
        let text = crate::vfs::read_to_string(under(&o, "inner/by i.xml")).unwrap();
        assert!(text.contains(".assets/") && !text.contains(o.to_str().unwrap()), "written from the library's top: {text}");
        let via_o = id_at(&mut tops, &ko, "inner/by i.xml").unwrap();
        assert_eq!(pictures(&mut tops, &ko, &via_o), [at.to_string_lossy().into_owned()], "O finds it in I's .assets/");
        assert_eq!(pictures(&mut tops, &ki, &by_i), [at.to_string_lossy().into_owned()]);
        // Through O: into O's .assets/, still named from a top, found through I too.
        let s = tops.store(&ko).unwrap();
        let at_o = crate::paths::resolve(&s.import_attachment(&png).unwrap(), &o);
        let fo = id_at(&mut tops, &ko, "inner").unwrap();
        let by_o = tops.store(&ko).unwrap().create_from_in(&with_picture("by o", &at_o), Some(&fo)).unwrap();
        let text = crate::vfs::read_to_string(under(&o, "inner/by o.xml")).unwrap();
        assert!(text.contains(".assets/") && !text.contains(o.to_str().unwrap()), "{text}");
        let via_i = id_at(&mut tops, &ki, "by o.xml").unwrap();
        assert_eq!(pictures(&mut tops, &ki, &via_i), [at_o.to_string_lossy().into_owned()], "I finds it in O's .assets/");
        assert_eq!(pictures(&mut tops, &ko, &by_o), [at_o.to_string_lossy().into_owned()]);
        // Saved again through the other one, the file says the same.
        let d = tops.store(&ki).unwrap().load(&via_i).unwrap();
        tops.store(&ki).unwrap().save(&via_i, &d).unwrap();
        assert_eq!(crate::vfs::read_to_string(under(&o, "inner/by o.xml")).unwrap(), text);
        // The sweep: what a page deleted through the other one still names (in that one's 回收站) stays.
        tops.store(&ko).unwrap().delete(&via_o, None).unwrap();
        tops.store(&ki).unwrap().delete(&via_i, None).unwrap();
        tops.list();
        let unused = |root: &Path| Store::unused_assets_at(root.to_path_buf(), vec![], vec![]).unwrap();
        assert!(unused(&o.join("inner")).is_empty(), "named by a page in O's 回收站");
        assert!(unused(&o).is_empty(), "named by a page in I's 回收站");
        // Nothing nests: as before, a picture no page names is offered.
        let lone = canonical(&dir("lone"));
        let mut s = Store::open_top_folder(&lone).unwrap();
        let pic = lone.join(s.import_attachment(&png).unwrap());
        let id = s.create_from(&with_picture("lone", &pic)).unwrap();
        s.delete(&id, None).unwrap();
        crate::vfs::write(Store::assets_in(&lone).join("stray.png"), b"x").unwrap();
        assert_eq!(unused(&lone).into_iter().map(|(n, _)| n).collect::<Vec<_>>(), ["stray.png"]);
    }

    /// The same page open for editing through both: a save through one that finds the
    /// file changed by the other refuses to write over it (DocxXML pages).
    #[test]
    fn a_page_edited_through_both_is_never_written_over() {
        let (mut tops, o, ko, ki) = nested();
        let fo = id_at(&mut tops, &ko, "inner").unwrap();
        let po = tops.store(&ko).unwrap().create_from_in(&doc("shared"), Some(&fo)).unwrap();
        let pi = id_at(&mut tops, &ki, "shared.xml").unwrap();
        let mut a = tops.store(&ko).unwrap().load_for_edit(&po, false).unwrap();
        let mut b = tops.store(&ki).unwrap().load_for_edit(&pi, false).unwrap();
        a.blocks.push(crate::model::Block::new(crate::model::BlockKind::Text, RichText::plain("from o")));
        b.blocks.push(crate::model::Block::new(crate::model::BlockKind::Text, RichText::plain("from i")));
        tops.store(&ko).unwrap().save_editor(&po, &a).unwrap();
        assert!(tops.store(&ki).unwrap().save_editor(&pi, &b).is_err());
        let text = crate::vfs::read_to_string(under(&o, "inner/shared.xml")).unwrap();
        assert!(text.contains("from o") && !text.contains("from i"));
        // A clean editor through I takes in O's save.
        assert!(tops.store(&ki).unwrap().refresh_external(&pi).unwrap().is_some());
    }

    #[test]
    fn a_folder_the_system_keeps_is_never_added() {
        let tops = TopFolders { fixed: true, ..Default::default() };
        if cfg!(unix) {
            for d in ["/usr", "/etc/", "/private", "/Library", "/Applications"] { if Path::new(d).fs_is_dir() { assert_eq!(tops.check(Path::new(d)), Err(AddError::System), "{d}"); } }
            if let Some(h) = dirs::home_dir().map(|h| h.join("Library")).filter(|l| l.fs_is_dir()) { assert_eq!(tops.check(&h), Err(AddError::System)); }
        }
        assert!(tops.check(&dir("notes")).is_ok() && tops.folders.is_empty());
        assert_ne!(AddError::System.to_string(), AddError::TooBroad.to_string());
    }

    /// A page renamed or moved through one of two nested top-level folders keeps, in the other,
    /// its id and what that one keeps about it (a pin); a folder renamed through the outer one
    /// that is the inner top-level folder's own takes it along to its new path.
    #[test]
    fn a_rename_through_one_keeps_what_the_other_has() {
        let (mut tops, o, ko, ki) = nested();
        let (io, ii) = (id_at(&mut tops, &ko, "inner/note.md").unwrap(), id_at(&mut tops, &ki, "note.md").unwrap());
        tops.store(&ki).unwrap().set_pinned(&ii, true).unwrap();
        tops.store(&ko).unwrap().set_pinned(&io, true).unwrap();
        tops.list();
        // Saved through the outer one first: a new file (inode), so only the move itself tells the inner one.
        let mut d = tops.store(&ko).unwrap().load(&io).unwrap();
        d.blocks.push(crate::model::Block::new(crate::model::BlockKind::Text, RichText::plain("more")));
        tops.store(&ko).unwrap().save(&io, &d).unwrap();
        tops.store(&ko).unwrap().rename(&io, "renamed").unwrap();
        let list = tops.list();
        let e = list.iter().find(|e| e.id == ii).expect("the inner one keeps its id");
        assert!(e.meta.rel.starts_with("renamed") && e.meta.pinned, "{:?}", e.meta.rel);
        assert!(list.iter().find(|e| e.id == io).unwrap().meta.pinned);
        assert_eq!(list.iter().filter(|e| e.top == ki).count(), 1, "no ghost of the old name");
        // The inner one's own folder, renamed through the outer one: it follows.
        let fo = id_at(&mut tops, &ko, "inner").unwrap();
        tops.store(&ko).unwrap().rename(&fo, "moved").unwrap();
        let list = tops.list();
        assert_eq!(tops.folder(&ki).unwrap().path, o.join("moved"));
        assert!(tops.store(&ki).is_some_and(|s| s.top_dir() == o.join("moved")));
        assert!(list.iter().find(|e| e.id == ii).is_some_and(|e| e.meta.pinned && e.top == ki), "still listed, still pinned");
        assert!(!o.join("inner").fs_exists(), "nothing written at the old path");
        // And through the inner one: the outer one follows.
        tops.store(&ki).unwrap().rename(&ii, "again").unwrap();
        let list = tops.list();
        assert!(list.iter().find(|e| e.id == io).is_some_and(|e| e.meta.rel.starts_with("moved/again") && e.meta.pinned));
    }

    /// Renamed in the Finder: found by its inode where it turns up, the same id and pin.
    #[test]
    fn a_rename_in_the_finder_keeps_what_it_had() {
        if !cfg!(any(unix, windows)) { return; }
        let (mut tops, o, ko, ki) = nested();
        let ii = id_at(&mut tops, &ki, "note.md").unwrap();
        let top = id_at(&mut tops, &ko, "top.md").unwrap();
        tops.store(&ki).unwrap().set_pinned(&ii, true).unwrap();
        tops.store(&ko).unwrap().set_pinned(&top, true).unwrap();
        tops.list();
        crate::vfs::rename(under(&o, "inner/note.md"), under(&o, "inner/renamed.md")).unwrap();
        crate::vfs::rename(o.join("top.md"), under(&o, "inner/top.md")).unwrap();
        let list = tops.list();
        assert!(list.iter().find(|e| e.id == ii).is_some_and(|e| e.meta.rel == "renamed.md" && e.meta.pinned));
        assert!(list.iter().find(|e| e.id == top).is_some_and(|e| e.meta.rel == "inner/top.md" && e.meta.pinned), "moved into a folder too");
        assert_eq!(list.iter().filter(|e| e.top == ki).count(), 2);
    }

    /// One top-level folder with `a.md` pinned and a folder `sub`, listed, then closed (the app quit).
    fn closed_with_a_pin() -> (PathBuf, String) {
        let o = canonical(&dir("again"));
        crate::vfs::create_dir_all(o.join("sub")).unwrap();
        crate::vfs::write(o.join("a.md"), "# a\n\nfirst\n").unwrap();
        crate::vfs::write(o.join("sub").join("keep.md"), "# keep\n").unwrap();
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let k = tops.add(&o).unwrap();
        let a = id_at(&mut tops, &k, "a.md").unwrap();
        tops.store(&k).unwrap().set_pinned(&a, true).unwrap();
        // Saved by the app just before it quit: a new file (number) the table must know.
        let mut d = tops.store(&k).unwrap().load(&a).unwrap();
        d.blocks.push(crate::model::Block::new(crate::model::BlockKind::Text, RichText::plain("more")));
        tops.store(&k).unwrap().save(&a, &d).unwrap();
        (o, a)
    }
    /// The app started again: what it lists, by path.
    fn reopened(o: &Path) -> Vec<Entry> {
        let mut tops = TopFolders { fixed: true, ..Default::default() };
        let k = tops.add(o).unwrap();
        tops.list().into_iter().filter(|e| e.top == k).collect()
    }
    fn at<'a>(list: &'a [Entry], rel: &str) -> &'a Entry { list.iter().find(|e| e.meta.rel == rel).unwrap_or_else(|| panic!("{rel} not listed")) }
    /// This computer's table of the files' numbers, edited as `f` says (standing in for what a test cannot make happen).
    fn edit_table(o: &Path, a: &str, f: impl FnOnce(&mut crate::file_ids::FileId)) {
        let local = o.join(".quiet").join("local");
        let dev = crate::vfs::read_dir(&local).unwrap().flatten().next().unwrap().path();
        let p = dev.join("files.json");
        let mut t: crate::file_ids::Table = serde_json::from_slice(&crate::vfs::read(&p).unwrap()).unwrap();
        f(&mut t.files.get_mut(a).unwrap().id);
        crate::vfs::write(&p, serde_json::to_vec(&t).unwrap()).unwrap();
    }

    /// Renamed and moved in the Finder while the app was closed: the next start finds it by
    /// the table in `.quiet/local/`, the same id and pin.
    #[test]
    fn a_rename_while_closed_keeps_what_it_had() {
        if !cfg!(any(unix, windows)) { return; }
        let (o, a) = closed_with_a_pin();
        crate::vfs::rename(o.join("a.md"), o.join("sub").join("renamed.md")).unwrap();
        let list = reopened(&o);
        let e = at(&list, "sub/renamed.md");
        assert!(e.id == a && e.meta.pinned, "{e:?}");
        assert!(list.iter().all(|e| e.meta.rel != "a.md"));
    }

    /// The deleted file's number given to a new one (`edit_table` makes the table say so): not
    /// inherited, as it was made at another time. With the time it was made, it would be.
    #[test]
    fn a_number_used_again_is_not_inherited() {
        if !cfg!(any(unix, windows)) { return; }
        let (o, a) = closed_with_a_pin();
        crate::vfs::remove_file(o.join("a.md")).unwrap();
        crate::vfs::write(o.join("new.md"), "# new\n").unwrap();
        let now = crate::file_ids::of(&o.join("new.md"), &crate::vfs::metadata(o.join("new.md")).unwrap()).unwrap();
        edit_table(&o, &a, |f| { f.dev = now.dev; f.ino = now.ino; f.born = now.born.map(|b| b + 1_000_000_000); f.size = now.size; });
        let list = reopened(&o);
        assert!(at(&list, "new.md").id != a && !at(&list, "new.md").meta.pinned);
        assert!(list.iter().all(|e| e.id != a && !e.meta.pinned));
        // The same, made at the time it says: that is the file renamed, and it keeps the pin.
        let (o, a) = closed_with_a_pin();
        crate::vfs::rename(o.join("a.md"), o.join("moved.md")).unwrap();
        let now = crate::file_ids::of(&o.join("moved.md"), &crate::vfs::metadata(o.join("moved.md")).unwrap()).unwrap();
        edit_table(&o, &a, |f| { f.born = now.born; });
        let list = reopened(&o);
        assert!(at(&list, "moved.md").id == a && at(&list, "moved.md").meta.pinned);
    }

    /// Deleted, and a new file of the same name made elsewhere: a new file, nothing inherited.
    #[test]
    fn a_new_file_of_the_same_name_is_not_inherited() {
        let (o, a) = closed_with_a_pin();
        crate::vfs::remove_file(o.join("a.md")).unwrap();
        crate::vfs::write(o.join("sub").join("a.md"), "# a\n\nfirst\n").unwrap();
        let list = reopened(&o);
        assert!(at(&list, "sub/a.md").id != a && !at(&list, "sub/a.md").meta.pinned);
        assert!(list.iter().all(|e| !e.meta.pinned));
    }

    /// Moved to another disk and one with the same number brought in: another disk's file (the
    /// table says the number was on another `dev`), nothing inherited.
    #[test]
    fn a_file_from_another_disk_is_not_taken_for_one_renamed() {
        if !cfg!(any(unix, windows)) { return; }
        let (o, a) = closed_with_a_pin();
        crate::vfs::rename(o.join("a.md"), o.join("b.md")).unwrap();
        edit_table(&o, &a, |f| f.dev = f.dev.wrapping_add(1));
        let list = reopened(&o);
        assert!(at(&list, "b.md").id != a && !at(&list, "b.md").meta.pinned);
    }

    /// A folder deleted through the outer one that is the inner top-level folder: off the
    /// sidebar with it, and back under its key.
    #[test]
    fn a_folder_taken_within_comes_back() {
        let (mut tops, o, ko, ki) = nested();
        let taken = tops.take_within(&[o.join("inner")]);
        assert_eq!(taken.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), [ki.as_str()]);
        assert!(tops.folder(&ki).is_none() && tops.folder(&ko).is_some());
        tops.put_back(taken);
        assert!(tops.store(&ki).is_some());
        assert!(id_at(&mut tops, &ki, "note.md").is_some());
    }

    /// The same page open through both, its editors showing the same text: a save through
    /// one makes that file the other's baseline (`follow_saved`), so its next save is no conflict.
    #[test]
    fn a_save_through_one_is_the_others_baseline() {
        let (mut tops, o, ko, ki) = nested();
        let fo = id_at(&mut tops, &ko, "inner").unwrap();
        let po = tops.store(&ko).unwrap().create_from_in(&doc("shared"), Some(&fo)).unwrap();
        let pi = id_at(&mut tops, &ki, "shared.xml").unwrap();
        let mut d = tops.store(&ko).unwrap().load_for_edit(&po, false).unwrap();
        tops.store(&ki).unwrap().load_for_edit(&pi, false).unwrap();
        d.blocks.push(crate::model::Block::new(crate::model::BlockKind::Text, RichText::plain("one")));
        tops.store(&ko).unwrap().save_editor(&po, &d).unwrap();
        tops.store(&ki).unwrap().follow_saved(&pi);
        d.blocks.push(crate::model::Block::new(crate::model::BlockKind::Text, RichText::plain("two")));
        tops.store(&ki).unwrap().save_editor(&pi, &d).unwrap();
        let text = crate::vfs::read_to_string(under(&o, "inner/shared.xml")).unwrap();
        assert!(text.contains("one") && text.contains("two"));
    }

    #[test]
    fn broad_folders_are_too_broad() {
        assert!(too_broad(Path::new("/")));
        if let Some(h) = dirs::home_dir() { assert!(too_broad(&h) && !too_broad(&h.join("Notes"))); }
        if cfg!(unix) { assert!(too_broad(Path::new("/usr")) && too_broad(Path::new("/etc/")) && !too_broad(&dir("narrow"))); }
    }

    #[test]
    fn a_missing_folder_is_listed_not_fatal() {
        let a = dir("gone");
        let entries = vec![TopFolderEntry { key: "k".into(), path: a.join("not-here"), bookmark: None, added: 0 }];
        let mut tops = TopFolders::open(entries, true);
        assert!(tops.folders[0].store.is_none() && tops.folders[0].error.is_some());
        assert!(!a.join("not-here").fs_exists(), "never made anew");
        assert!(tops.list().is_empty());
    }
}
