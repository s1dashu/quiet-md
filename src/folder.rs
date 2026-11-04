//! An open folder of Markdown / MDX files, shown in the normal window.
//!
//! Each open file is a `Buffer`: its `Source` (the file's text, authoritative)
//! and an ordinary `Editor` over the `Projection` parsed from it. Rendered edits
//! go through `Core`, then `Projection::render` patches the source text; Source
//! mode edits the text directly and re-parses. Parsed documents, Git rows and
//! diagrams are all derived and disposable. `Workspace` owns the window: this
//! module only supplies the sidebar, top bar and pane it draws.
use crate::{app::{Workspace, View}, editor::view::{Editor, EditorEvent}, input::{TextField, TextFieldEvent}, sidebar::{ROW_H, ROW_ICON, ROW_R, SB_X, footer_line, footer_row, footer_slot}, theme::{self, Theme, craft}, ui::{TipExt, TitleBarExt, ToastKind, icon, menu_row, menu_label, menu_glyph, popover}};
use fdl_core::{source_doc::{self, Projection}, workspace_files::{self as files, Entry, Source}, workspace_git::{self as git, FileStatus, Repository, Snapshot}};
use futures::StreamExt;
use gpui::{prelude::FluentBuilder, *};
use std::{collections::{HashMap, HashSet, VecDeque}, path::{Path, PathBuf}, rc::Rc, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};

actions!(folder, [OpenFolder, CloseFolder, Back, Forward, SaveSource, SourceMode, EditingMode, SplitMode, ReadingMode]);
pub fn bind_keys(cx: &mut App) {
    // ⌥← / ⌥→ move by word and ⌘] is the sidebar, so history takes ⌘⌥← / ⌘⌥→.
    let (back, forward) = if cfg!(target_os = "windows") { ("alt-left", "alt-right") } else { ("cmd-alt-left", "cmd-alt-right") };
    cx.bind_keys([
        KeyBinding::new("cmd-o", OpenFolder, None), KeyBinding::new("cmd-s", SaveSource, None), KeyBinding::new(back, Back, None), KeyBinding::new(forward, Forward, None),
        KeyBinding::new("cmd-alt-1", EditingMode, None), KeyBinding::new("cmd-alt-2", ReadingMode, None), KeyBinding::new("cmd-alt-3", SourceMode, None), KeyBinding::new("cmd-alt-4", SplitMode, None),
    ]);
}
/// Narrower than this, a folder's tree comes in over the document.
pub const NARROW: f32 = 720.;
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode { Edit, Read, Source, Split }
/// What a successful open does to the window's history. A failed open does nothing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum How { Record, Travel(bool) }
pub enum FolderEvent { Opened { path: PathBuf, anchor: Option<String>, how: How }, Moved(PathBuf, PathBuf), Toast(ToastKind, String) }
impl EventEmitter<FolderEvent> for Folder {}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Conflict { Changed, Deleted }
enum Disk { Text(String), Missing, Moved(PathBuf) }
struct Buffer {
    source: Source, projection: Arc<Projection>, editor: Entity<Editor>, field: Entity<TextField>, scroll: ScrollHandle,
    _subs: [Subscription; 2], used: u64,
    /// The editor has edits not yet written into `source.text`.
    pending: bool,
    /// `source.text` moved on without the other side: the field needs it, or the projection does.
    field_stale: bool, projection_stale: bool,
    sync_task: Option<Task<()>>, save_task: Option<Task<()>>, parse_task: Option<Task<()>>,
    conflict: Option<Conflict>, error: Option<String>, recovered: bool, confirm_reload: bool,
}
#[derive(Clone)]
enum TreeRow { Entry(Entry, usize), Note(SharedString, usize) }
#[derive(Clone)]
enum GitRow { Group(&'static str, usize), File(FileStatus, bool), More(usize) }
enum GitState { Detecting, None, Failed(Option<Repository>, String), Ready(Repository, Snapshot) }
struct Diff { file: FileStatus, staged: bool, patch: Option<Rc<git::Patch>>, error: Option<String>, list: ListState }
#[derive(Default)]
struct Palette { open: bool, index: Option<Rc<Vec<PathBuf>>>, truncated: bool, files: Vec<PathBuf>, hits: Vec<files::Hit>, searching: bool, cut: bool, selected: usize, task: Option<Task<()>>, cancel: Option<Arc<AtomicBool>> }
/// Sets its flag when the search that owns it is replaced or the palette closes.
impl Palette {
    fn stop(&mut self) { if let Some(c) = self.cancel.take() { c.store(true, Ordering::Relaxed); } self.task = None; self.searching = false; }
    fn rows(&self) -> usize { self.files.len() + self.hits.len() }
}

pub struct Folder {
    pub root: PathBuf, pub current: Option<PathBuf>, buffers: HashMap<PathBuf, Buffer>, directories: HashMap<PathBuf, files::Listing>, expanded: HashSet<PathBuf>,
    show_hidden: bool, tree: Rc<Vec<TreeRow>>, tree_scroll: UniformListScrollHandle, git: GitState, git_rows: Rc<Vec<GitRow>>, git_open: bool, git_task: Option<Task<()>>, git_again: bool, git_scroll: UniformListScrollHandle,
    mode: Mode, diff: Option<Diff>, menu: bool, folders_menu: bool, overlay: bool, recoveries: Vec<PathBuf>, tick: u64, open_request: u64, open_task: Option<Task<()>>, opening: Option<PathBuf>,
    watcher: Option<notify::RecommendedWatcher>, watched_git: Option<PathBuf>, watcher_error: Option<String>, tasks: Vec<Task<()>>, more_bounds: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    palette: Palette, palette_field: Entity<TextField>, _palette_sub: Subscription, scroll_memo: HashMap<PathBuf, ListOffset>,
    window: AnyWindowHandle, recent_files: VecDeque<PathBuf>,
    /// A search hit being opened: select this text once the file is showing.
    pending_find: Option<String>,
}
/// Beside a throwaway `QUIET_MD_HOME`, or in app data: never in the user's library.
fn recovery_dir() -> PathBuf {
    let default = dirs::document_dir().map(|d| d.join("Quiet.md"));
    std::env::var_os("QUIET_MD_HOME").map(PathBuf::from).filter(|p| Some(p) != default.as_ref()).map(|p| p.join("workspace-recovery")).unwrap_or_else(|| dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("quiet.md/workspace-recovery"))
}
fn preferences() -> PathBuf { recovery_dir().with_file_name("workspace-folders.json") }
pub fn recent() -> Vec<PathBuf> { std::fs::read(preferences()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default() }
fn remember(root: &Path) {
    let mut paths = recent(); paths.retain(|p| p != root); paths.insert(0, root.to_path_buf()); paths.truncate(12);
    if let Some(parent) = preferences().parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(bytes) = serde_json::to_vec(&paths) { let _ = std::fs::write(preferences(), bytes); }
}
fn recovery_paths(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(recovery_dir()).ok().into_iter().flatten().take(200).filter_map(Result::ok).filter_map(|entry| {
        if entry.path().extension().is_none_or(|s| s != "json") || entry.metadata().ok()?.len() > 64 * 1024 * 1024 { return None; }
        let source: Source = serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
        (source.path.starts_with(root) && source.dirty()).then_some(source.path)
    }).collect()
}
/// Paths whose changes matter. Our own save temporaries and Git's object churn don't.
fn relevant(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".quiet-save-")) { return false; }
    !path.components().any(|c| c.as_os_str() == ".git") || path.file_name().is_some_and(|n| matches!(n.to_string_lossy().as_ref(), "HEAD" | "index" | "MERGE_HEAD" | "packed-refs")) || path.components().any(|c| c.as_os_str() == "refs")
}

impl Folder {
    pub fn new(root: PathBuf, first: Option<(PathBuf, Option<String>, How)>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        remember(&root);
        let palette_field = cx.new(|cx| TextField::new("按文件名查找…", cx));
        let palette_sub = cx.subscribe_in(&palette_field, window, |f, field, ev: &TextFieldEvent, window, cx| match ev {
            TextFieldEvent::Changed => { let q = field.read(cx).text.clone(); f.filter_palette(&q); f.search_contents(&q, cx); cx.notify(); }
            TextFieldEvent::Submit => f.palette_choose(f.palette.selected, window, cx),
            TextFieldEvent::Cancel => f.close_palette(window, cx),
            TextFieldEvent::Up => { f.palette.selected = f.palette.selected.saturating_sub(1); cx.notify(); }
            TextFieldEvent::Down => { f.palette.selected = (f.palette.selected + 1).min(f.palette.rows().saturating_sub(1)); cx.notify(); }
            _ => {}
        });
        let mut this = Self {
            root: root.clone(), current: None, buffers: HashMap::new(), directories: HashMap::new(), expanded: HashSet::from([root.clone()]), show_hidden: false, tree: Rc::default(), tree_scroll: Default::default(),
            git: GitState::Detecting, git_rows: Rc::default(), git_open: false, git_task: None, git_again: false, git_scroll: Default::default(), mode: Mode::Edit, diff: None, menu: false, folders_menu: false, overlay: false, recoveries: vec![], tick: 0, open_request: 0, open_task: None, opening: None,
            watcher: None, watched_git: None, watcher_error: None, tasks: vec![], more_bounds: Default::default(), palette: Palette::default(), palette_field, _palette_sub: palette_sub, scroll_memo: HashMap::new(), window: window.window_handle(), recent_files: VecDeque::new(), pending_find: None,
        };
        this.start_watching(cx);
        this.refresh_git(cx);
        // The first listing and the first document, off the UI thread.
        let scan = root.clone();
        this.tasks.push(cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move { (files::list(&scan, &scan, false), recovery_paths(&scan)) }).await;
            let _ = this.update_in(cx, |f, window, cx| {
                match result.0 {
                    Ok(listing) => {
                        let pick = listing.entries.iter().find(|e| !e.directory && e.path.file_name().is_some_and(|s| s.eq_ignore_ascii_case("readme.md"))).or_else(|| listing.entries.iter().find(|e| !e.directory)).map(|e| e.path.clone());
                        f.directories.insert(f.root.clone(), listing); f.recoveries = result.1; f.rebuild_tree();
                        if f.watcher_error.is_some() { cx.emit(FolderEvent::Toast(ToastKind::Info, crate::settings::tr("无法实时监听此文件夹，外部改动每 30 秒检查一次", "Can't watch this folder live; outside changes are checked every 30 seconds").to_string())); }
                        match first { Some((path, anchor, how)) => f.open(path, anchor, how, window, cx), None => if let Some(path) = pick { f.open(path, None, How::Record, window, cx); } }
                    }
                    Err(e) => cx.emit(FolderEvent::Toast(ToastKind::Error, e.to_string())),
                }
                cx.notify();
            });
        }));
        // A slow safety net for volumes whose events don't arrive.
        this.tasks.push(cx.spawn(async |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(30)).await;
            if this.update(cx, |f, cx| { f.check_buffers(None, cx); f.refresh_git(cx); }).is_err() { break; }
        }));
        this
    }
    fn start_watching(&mut self, cx: &mut Context<Self>) {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<notify::Result<notify::Event>>();
        let watcher = notify::recommended_watcher(move |ev| { let _ = tx.unbounded_send(ev); }).and_then(|mut w| { use notify::Watcher; w.watch(&self.root, notify::RecursiveMode::Recursive)?; Ok(w) });
        match watcher { Ok(w) => self.watcher = Some(w), Err(e) => self.watcher_error = Some(e.to_string()) }
        self.tasks.push(cx.spawn(async move |this, cx| {
            while let Some(first) = rx.next().await {
                // Coalesce a burst (a checkout, a save) into one pass.
                cx.background_executor().timer(Duration::from_millis(150)).await;
                let (mut paths, mut rescan) = (HashSet::new(), false);
                let mut take = |ev: notify::Result<notify::Event>| match ev {
                    Ok(ev) if matches!(ev.kind, notify::EventKind::Access(_)) => {},
                    Ok(ev) => { rescan |= ev.need_rescan(); paths.extend(ev.paths.into_iter().filter(|p| relevant(p))); }
                    Err(_) => rescan = true,
                };
                take(first);
                while let Ok(ev) = rx.try_recv() { take(ev); }
                if this.update(cx, |f, cx| f.on_fs_events(paths, rescan, cx)).is_err() { break; }
            }
        }));
    }
    fn on_fs_events(&mut self, paths: HashSet<PathBuf>, rescan: bool, cx: &mut Context<Self>) {
        if paths.is_empty() && !rescan { return; }
        let dirs: Vec<PathBuf> = if rescan { self.expanded.iter().cloned().collect() } else { paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).chain(paths.iter().cloned()).filter(|d| self.expanded.contains(d)).collect::<HashSet<_>>().into_iter().collect() };
        self.relist(dirs, cx);
        self.check_buffers(if rescan { None } else { Some(paths) }, cx);
        self.refresh_git(cx);
    }
    fn relist(&mut self, dirs: Vec<PathBuf>, cx: &mut Context<Self>) {
        if dirs.is_empty() { return; }
        let (root, hidden) = (self.root.clone(), self.show_hidden);
        cx.spawn(async move |this, cx| {
            let results = cx.background_executor().spawn(async move { dirs.into_iter().map(|d| { let l = files::list(&root, &d, hidden); (d, l) }).collect::<Vec<_>>() }).await;
            let _ = this.update(cx, |f, cx| {
                let mut changed = false;
                for (dir, listing) in results {
                    match listing {
                        Ok(l) if f.expanded.contains(&dir) && f.directories.get(&dir) != Some(&l) => { f.directories.insert(dir, l); changed = true; }
                        Err(_) if f.directories.remove(&dir).is_some() => { f.expanded.remove(&dir); changed = true; }
                        _ => {}
                    }
                }
                if changed { f.rebuild_tree(); cx.notify(); }
            });
        }).detach();
    }
    fn rebuild_tree(&mut self) {
        fn walk(f: &Folder, dir: &Path, depth: usize, out: &mut Vec<TreeRow>) {
            let Some(listing) = f.directories.get(dir) else { return };
            for entry in &listing.entries {
                if out.len() >= 20_000 { return; }
                out.push(TreeRow::Entry(entry.clone(), depth));
                if entry.directory && !entry.outside && f.expanded.contains(&entry.path) { walk(f, &entry.path, depth + 1, out); }
            }
            if listing.truncated { out.push(TreeRow::Note(crate::settings::tr("文件过多，仅显示前 5000 项", "Too many entries; showing the first 5,000").into(), depth)); }
            if listing.other_files > 0 { out.push(TreeRow::Note(if listing.other_files == 1 { crate::settings::tr("另有 1 个非文档文件", "1 other file not shown").into() } else { crate::localized_format!("另有 {} 个非文档文件", "{} other files not shown", listing.other_files).into() }, depth)); }
        }
        let mut rows = vec![]; walk(self, &self.root, 0, &mut rows);
        self.tree = Rc::new(rows);
    }
    fn toggle_dir(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.expanded.remove(&path) { self.rebuild_tree(); cx.notify(); return; }
        self.expanded.insert(path.clone()); self.relist(vec![path], cx); self.rebuild_tree(); cx.notify();
    }
    /// Open the tree down to `path` so the open file is always visible in it.
    fn reveal(&mut self, path: &Path, cx: &mut Context<Self>) {
        let mut need = vec![];
        for dir in path.ancestors().skip(1).take_while(|d| d.starts_with(&self.root)) { if self.expanded.insert(dir.to_path_buf()) || !self.directories.contains_key(dir) { need.push(dir.to_path_buf()); } }
        self.relist(need, cx); self.rebuild_tree();
        if let Some(i) = self.tree.iter().position(|r| matches!(r, TreeRow::Entry(e, _) if e.path == path)) { self.tree_scroll.scroll_to_item(i, ScrollStrategy::Center); }
    }

    // ---- opening

    pub fn open(&mut self, path: PathBuf, anchor: Option<String>, how: How, window: &mut Window, cx: &mut Context<Self>) {
        self.open_request += 1; self.open_task = None; self.opening = None; self.menu = false;
        if self.buffers.contains_key(&path) { self.show(path, anchor, how, window, cx); return; }
        // A directory link goes to its README.
        let readme = (path.is_dir()).then(|| ["README.md", "readme.md", "index.md", "index.mdx"].iter().map(|n| path.join(n)).find(|p| p.is_file())).flatten();
        if path.is_dir() && readme.is_none() { if !self.expanded.contains(&path) { self.toggle_dir(path, cx); } return; }
        let target = readme.unwrap_or(path);
        if self.buffers.contains_key(&target) { self.show(target, anchor, how, window, cx); return; }
        if !files::supported(&target) {
            cx.emit(FolderEvent::Toast(ToastKind::Info, crate::localized_format!("无法在此打开：{}", "Can't open this here: {}", target.file_name().unwrap_or_default().to_string_lossy())));
            return;
        }
        let (request, root) = (self.open_request, self.root.clone());
        self.opening = Some(target.clone());
        self.open_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let recovered = Source::recovery(&root, &target, &recovery_dir())?;
                let is_recovered = recovered.is_some();
                let source = match recovered { Some(s) => s, None => Source::open(&root, &files::contained(&root, &target)?)? };
                let projection = source_doc::project(&source.text, &source.path, &root);
                anyhow::Ok((source, projection, is_recovered))
            }).await;
            let _ = this.update_in(cx, |f, window, cx| {
                if f.open_request != request { return; }
                f.open_task = None; f.opening = None;
                match result {
                    Ok((source, projection, recovered)) => { let path = source.path.clone(); f.add_buffer(source, projection, recovered, window, cx); f.show(path, anchor, how, window, cx); }
                    Err(e) => cx.emit(FolderEvent::Toast(ToastKind::Error, crate::localized_format!("无法打开文件：{}", "Could not open file: {}", e))),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn add_buffer(&mut self, source: Source, projection: Projection, recovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = source.path.clone();
        if self.buffers.contains_key(&path) { return; }
        let meta = projection.meta.clone();
        let read_only = self.mode == Mode::Read;
        let memo = self.scroll_memo.get(&path).copied();
        let editor = cx.new(|cx| { let mut e = Editor::new(projection.doc.clone(), 0, Default::default(), window, cx); e.source_doc = true; e.source_meta = meta; e.set_read_only(read_only, cx); e.restore_top = memo;
            // A diagram up front stays a diagram until the caret goes into it.
            if e.core.doc.blocks.first().is_some_and(|b| matches!(&b.kind, crate::model::BlockKind::Code { lang, .. } if lang.eq_ignore_ascii_case("mermaid"))) { e.core.sel = crate::editor::core::Selection::caret(crate::editor::core::Pos::new(crate::editor::core::TITLE, 0)); }
            e });
        let field = cx.new(|cx| { let mut f = TextField::new("", cx); f.multiline = true; f.source = true; f.mono = true; f.wrap = true; f.size = 13.; f.line_height = 21.; f.set_text(&source.text, cx); f });
        let p = path.clone();
        let editor_sub = cx.subscribe_in(&editor, window, move |f, editor, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Changed => { if !editor.update(cx, |e, cx| e.reject_read_only_edit(cx)) { f.editor_changed(&p, window, cx); } }
            EditorEvent::Save => { f.flush(cx); }
            EditorEvent::SourceLink(url) => f.follow_link(&p, url, window, cx),
            EditorEvent::SourceUndo(redo) => f.source_undo(&p, *redo, window, cx),
            EditorEvent::Mode(reading) => f.set_mode(if *reading { Mode::Read } else { Mode::Edit }, window, cx),
            EditorEvent::Search(_) => f.open_palette(window, cx),
            _ => {}
        });
        let p = path.clone();
        let field_sub = cx.subscribe_in(&field, window, move |f, field, ev: &TextFieldEvent, window, cx| match ev {
            TextFieldEvent::Changed => { let text = field.read(cx).text.clone(); f.source_edited(&p, text, window, cx); }
            TextFieldEvent::Undo => f.source_undo(&p, false, window, cx),
            TextFieldEvent::Redo => f.source_undo(&p, true, window, cx),
            _ => {}
        });
        self.buffers.insert(path, Buffer { source, projection: Arc::new(projection), editor, field, scroll: ScrollHandle::new(), _subs: [editor_sub, field_sub], used: 0, pending: false, field_stale: false, projection_stale: false, sync_task: None, save_task: None, parse_task: None, conflict: None, error: None, recovered, confirm_reload: false });
    }
    fn show(&mut self, path: PathBuf, anchor: Option<String>, how: How, window: &mut Window, cx: &mut Context<Self>) {
        // Leaving a file: its text is up to date on the way out.
        if let Some(previous) = self.current.clone() && previous != path { self.sync_from_editor(&previous, cx); self.save_buffer(&previous, cx); }
        self.tick += 1;
        let tick = self.tick; let read_only = self.mode == Mode::Read;
        let Some(b) = self.buffers.get_mut(&path) else { return };
        b.used = tick;
        b.editor.update(cx, |e, cx| e.set_read_only(read_only, cx));
        if let Some(id) = anchor.as_ref().and_then(|a| b.projection.anchors.get(a).copied()) { b.editor.update(cx, |e, cx| e.scroll_to_block(id, cx)); }
        if let Some(find) = self.pending_find.take() { Self::select_text(&b.editor, &find, cx); }
        self.current = Some(path.clone()); self.diff = None; self.overlay = false;
        self.recent_files.retain(|p| p != &path); self.recent_files.push_front(path.clone()); self.recent_files.truncate(20);
        self.reveal(&path, cx);
        self.refresh_field(&path, cx);
        self.focus_current(window, cx);
        cx.emit(FolderEvent::Opened { path: path.clone(), anchor, how });
        self.evict(&path, cx);
        // It may have changed while it wasn't being looked at.
        self.check_buffers(Some(HashSet::from([path])), cx);
        cx.notify();
    }
    fn select_text(editor: &Entity<Editor>, text: &str, cx: &mut Context<Self>) {
        let needle = text.to_lowercase();
        editor.update(cx, |e, cx| {
            let mut found = None;
            e.core.doc.for_each_block(|b| { if found.is_none() && let Some(at) = b.text.text.to_lowercase().find(&needle) { found = Some((b.id, at)); } });
            let Some((id, at)) = found else { return };
            let end = (at + text.len()).min(e.core.block(id).map_or(at, |b| b.text.len()));
            if e.core.block(id).is_some_and(|b| b.text.text.is_char_boundary(at) && b.text.text.is_char_boundary(end)) {
                e.core.sel = crate::editor::core::Selection { anchor: crate::editor::core::Pos::new(id, at), head: crate::editor::core::Pos::new(id, end) };
            }
            e.scroll_to_block(id, cx);
        });
    }
    /// Clean files beyond the limit go; their scroll position stays. Dirty ones are never evicted.
    fn evict(&mut self, keep: &Path, cx: &mut Context<Self>) {
        while self.buffers.len() > 24 || self.buffers.values().map(|b| b.source.text.len() * 2).sum::<usize>() > 64 * 1024 * 1024 {
            let oldest = self.buffers.iter().filter(|(p, b)| *p != keep && !b.source.dirty() && !b.pending && b.conflict.is_none()).min_by_key(|(_, b)| b.used).map(|(p, _)| p.clone());
            let Some(oldest) = oldest else { break };
            if let Some(b) = self.buffers.remove(&oldest) { self.scroll_memo.insert(oldest, b.editor.read(cx).list.logical_scroll_top()); }
        }
    }
    fn focus_current(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.current.as_ref().and_then(|p| self.buffers.get(p)) else { return };
        if self.mode == Mode::Source { window.focus(&b.field.read(cx).focus_handle(cx)); } else { b.editor.read(cx).focus(window); }
    }
    fn follow_link(&mut self, from: &Path, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") { cx.open_url(url); return; }
        match files::relative_link(&self.root, from, url) {
            Ok((path, anchor)) => self.open(path, anchor, How::Record, window, cx),
            // Nothing there, or outside the folder: say which, not the OS error.
            Err(e) => {
                let missing = e.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound);
                cx.emit(FolderEvent::Toast(ToastKind::Error, if missing { crate::localized_format!("找不到文件：{}", "File not found: {}", url) } else { crate::localized_format!("无法打开链接：{}", "Could not follow this link: {}", crate::settings::diagnostic(&e.to_string())) }));
            }
        }
    }

    // ---- edits: both directions, debounced and guarded by revision

    fn editor_changed(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.buffers.get_mut(path) else { return };
        b.pending = true;
        let p = path.to_path_buf();
        b.sync_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let _ = this.update(cx, |f, cx| { f.sync_from_editor(&p, cx); f.schedule_save(&p, cx); });
        }));
    }
    /// Write the editor's state into the source text. Cheap: it is a diff against the base.
    fn sync_from_editor(&mut self, path: &Path, cx: &mut Context<Self>) {
        let split = self.mode == Mode::Split;
        let Some(b) = self.buffers.get_mut(path) else { return };
        if !b.pending { return; }
        b.pending = false; b.sync_task = None;
        // The base is out of date until Source mode's edit has been re-parsed.
        if b.projection_stale { return; }
        let text = b.projection.render(&b.editor.read(cx).core.doc);
        if text != b.source.text { b.source.edit(text); b.field_stale = true; }
        Self::write_recovery(b);
        if split { self.refresh_field(path, cx); }
        // Whoever got here first took over the debounce that would have saved this.
        self.schedule_save(path, cx);
        cx.notify();
    }
    fn write_recovery(b: &mut Buffer) {
        if b.source.dirty() { if let Err(e) = b.source.recover(&recovery_dir()) { b.error = Some(crate::localized_format!("无法写入恢复文件：{}", "Could not write the recovery copy: {}", e)); } } else { b.source.clear_recovery(&recovery_dir()); }
    }
    fn source_edited(&mut self, path: &Path, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_from_editor(path, cx);
        let Some(b) = self.buffers.get_mut(path) else { return };
        if text == b.source.text { return; }
        b.source.edit(text); b.projection_stale = true;
        Self::write_recovery(b);
        self.schedule_parse(path, 200, window, cx);
        self.schedule_save(path, cx);
        cx.notify();
    }
    fn source_undo(&mut self, path: &Path, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_from_editor(path, cx);
        let Some(b) = self.buffers.get_mut(path) else { return };
        if !b.source.undo(redo) { return; }
        b.projection_stale = true; b.field_stale = true;
        Self::write_recovery(b);
        self.refresh_field(path, cx);
        self.schedule_parse(path, 0, window, cx);
        self.schedule_save(path, cx);
        cx.notify();
    }
    fn refresh_field(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !matches!(self.mode, Mode::Source | Mode::Split) { return; }
        let Some(b) = self.buffers.get_mut(path) else { return };
        if !b.field_stale { return; }
        b.field_stale = false;
        let text = b.source.text.clone();
        b.field.update(cx, |f, cx| if f.text != text { f.set_text(&text, cx) });
    }
    /// Re-parse off the UI thread; a result for an older revision is dropped.
    fn schedule_parse(&mut self, path: &Path, delay: u64, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.root.clone();
        let Some(b) = self.buffers.get_mut(path) else { return };
        let (text, revision, p) = (b.source.text.clone(), b.source.revision, path.to_path_buf());
        b.parse_task = Some(cx.spawn_in(window, async move |this, cx| {
            if delay > 0 { cx.background_executor().timer(Duration::from_millis(delay)).await; }
            let file = p.clone();
            let projection = cx.background_executor().spawn(async move { source_doc::project(&text, &file, &root) }).await;
            let _ = this.update(cx, |f, cx| f.apply_projection(&p, revision, projection, cx));
        }));
    }
    fn reparse_now(&mut self, path: &Path, cx: &mut Context<Self>) {
        let root = self.root.clone();
        let Some(b) = self.buffers.get_mut(path) else { return };
        let (text, revision, p) = (b.source.text.clone(), b.source.revision, path.to_path_buf());
        b.parse_task = Some(cx.spawn(async move |this, cx| {
            let file = p.clone();
            let projection = cx.background_executor().spawn(async move { source_doc::project(&text, &file, &root) }).await;
            let _ = this.update(cx, |f, cx| f.apply_projection(&p, revision, projection, cx));
        }));
    }
    fn apply_projection(&mut self, path: &Path, revision: u64, projection: Projection, cx: &mut Context<Self>) {
        let Some(b) = self.buffers.get_mut(path).filter(|b| b.source.revision == revision) else { return };
        b.parse_task = None; b.projection_stale = false; b.pending = false; b.sync_task = None;
        let (doc, meta) = (projection.doc.clone(), projection.meta.clone());
        b.projection = Arc::new(projection);
        b.editor.update(cx, |e, cx| e.rebase_source(doc, meta, cx));
        cx.notify();
    }
    fn schedule_save(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(b) = self.buffers.get_mut(path) else { return };
        if !b.source.dirty() { b.save_task = None; return; }
        let p = path.to_path_buf();
        b.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let _ = this.update(cx, |f, cx| { f.sync_from_editor(&p, cx); f.save_buffer(&p, cx); f.refresh_git(cx); });
        }));
    }
    /// False when the file could not be written. Its text stays, and the recovery copy.
    fn save_buffer(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let root = self.root.clone();
        let Some(b) = self.buffers.get_mut(path) else { return true };
        b.save_task = None;
        if !b.source.dirty() { b.source.clear_recovery(&recovery_dir()); b.error = None; return true; }
        if b.conflict.is_some() { return false; }
        Self::write_recovery(b);
        let ok = match b.source.save(&root) {
            Ok(()) => { b.error = None; b.recovered = false; b.source.clear_recovery(&recovery_dir()); self.recoveries.retain(|p| p != path); true }
            Err(e) => {
                let text = e.to_string();
                if text.contains("changed on disk") || text.contains("changed during save") { b.conflict = Some(Conflict::Changed); }
                else if !path.exists() { b.conflict = Some(Conflict::Deleted); }
                else { b.error = Some(crate::settings::diagnostic(&text)); }
                false
            }
        };
        cx.notify();
        ok
    }
    pub fn flush(&mut self, cx: &mut Context<Self>) -> bool {
        let paths: Vec<PathBuf> = self.buffers.keys().cloned().collect();
        let mut ok = true;
        for path in paths { self.sync_from_editor(&path, cx); ok &= self.save_buffer(&path, cx); }
        ok
    }
    pub fn save_error(&self) -> String {
        self.buffers.values().find_map(|b| match b.conflict {
            Some(Conflict::Changed) => Some(crate::localized_format!("{} 已在磁盘上更改", "{} changed on disk", b.source.path.file_name().unwrap_or_default().to_string_lossy())),
            Some(Conflict::Deleted) => Some(crate::localized_format!("{} 已被移动或删除", "{} was moved or deleted", b.source.path.file_name().unwrap_or_default().to_string_lossy())),
            None => b.error.clone(),
        }).unwrap_or_else(|| crate::settings::text("保存失败").to_string())
    }
    pub fn discard(&mut self, cx: &mut Context<Self>) {
        for b in self.buffers.values() { b.source.clear_recovery(&recovery_dir()); }
        self.buffers.clear(); self.current = None; cx.notify();
    }

    // ---- what changed on disk

    fn check_buffers(&mut self, only: Option<HashSet<PathBuf>>, cx: &mut Context<Self>) {
        // A file already known gone stays a candidate: its new name may arrive later.
        let paths: Vec<PathBuf> = self.buffers.iter().filter(|(p, b)| only.as_ref().is_none_or(|s| s.contains(*p)) || b.conflict == Some(Conflict::Deleted)).map(|(p, _)| p.clone()).collect();
        if paths.is_empty() { return; }
        for p in &paths { self.sync_from_editor(p, cx); }
        let jobs: Vec<(PathBuf, u64, String)> = paths.iter().filter_map(|p| self.buffers.get(p).map(|b| (p.clone(), b.source.revision, b.source.baseline.clone()))).collect();
        let arrivals: Vec<PathBuf> = only.into_iter().flatten().filter(|p| !self.buffers.contains_key(p) && files::supported(p)).collect();
        cx.spawn(async move |this, cx| {
            let results = cx.background_executor().spawn(async move { jobs.into_iter().map(|(p, rev, baseline)| {
                if p.exists() { let disk = files::read(&p).map(Disk::Text).map_err(|e| e.to_string()); return (p, rev, disk); }
                // Gone: a file that just appeared with the same bytes is the same file, renamed.
                let moved = arrivals.iter().find(|new| files::read(new).is_ok_and(|t| t == baseline)).cloned();
                (p, rev, Ok(moved.map_or(Disk::Missing, Disk::Moved)))
            }).collect::<Vec<_>>() }).await;
            let _ = this.update(cx, |f, cx| { for (p, rev, disk) in results { f.reconcile(&p, rev, disk, cx); } cx.notify(); });
        }).detach();
    }
    fn reconcile(&mut self, path: &Path, revision: u64, disk: Result<Disk, String>, cx: &mut Context<Self>) {
        let Some(b) = self.buffers.get_mut(path) else { return };
        // The buffer moved while this read was in flight: a late result must not win.
        if b.source.revision != revision || b.pending { self.retry_check(path, cx); return; }
        match disk {
            Err(e) => b.error = Some(crate::settings::diagnostic(&e)),
            Ok(Disk::Missing) => b.conflict = Some(Conflict::Deleted),
            Ok(Disk::Moved(to)) => self.follow_move(path, to, cx),
            Ok(Disk::Text(disk)) if disk == b.source.text => { b.source.baseline = disk; b.conflict = None; b.error = None; b.source.clear_recovery(&recovery_dir()); }
            Ok(Disk::Text(disk)) if disk == b.source.baseline => { if b.conflict == Some(Conflict::Deleted) { b.conflict = None; } }
            Ok(Disk::Text(_)) if b.source.dirty() => b.conflict = Some(Conflict::Changed),
            Ok(Disk::Text(disk)) => {
                // Clean: follow the file, keeping the scroll position.
                b.source.replace(disk); b.conflict = None; b.error = None; b.projection_stale = true; b.field_stale = true;
                self.refresh_field(path, cx);
                self.reparse_now(path, cx);
            }
        }
    }
    /// The open file was renamed: same text, new home. Unsaved edits come along.
    fn follow_move(&mut self, from: &Path, to: PathBuf, cx: &mut Context<Self>) {
        let Some(mut source) = self.buffers.get(from).map(|b| b.source.clone()) else { return };
        let (root, handle, was_current, old) = (self.root.clone(), self.window, self.current.as_deref() == Some(from), from.to_path_buf());
        source.clear_recovery(&recovery_dir());
        source.path = to.canonicalize().unwrap_or(to);
        cx.spawn(async move |this, cx| {
            let text = source.text.clone(); let file = source.path.clone();
            let projection = cx.background_executor().spawn(async move { source_doc::project(&text, &file, &root) }).await;
            let _ = cx.update_window(handle, |_, window, cx| { let _ = this.update(cx, |f, cx| {
                if !f.buffers.contains_key(&old) { return; }
                let new = source.path.clone();
                let dirty = source.dirty();
                f.buffers.remove(&old);
                f.add_buffer(source, projection, false, window, cx);
                if dirty && let Some(b) = f.buffers.get_mut(&new) { Self::write_recovery(b); }
                cx.emit(FolderEvent::Moved(old.clone(), new.clone()));
                cx.emit(FolderEvent::Toast(ToastKind::Info, crate::localized_format!("文件已移动到 {}", "File moved to {}", new.strip_prefix(&f.root).unwrap_or(&new).display())));
                if was_current { f.current = Some(new.clone()); f.reveal(&new, cx); f.focus_current(window, cx); }
                if dirty { f.schedule_save(&new, cx); }
                cx.notify();
            }); });
        }).detach();
    }
    fn retry_check(&mut self, path: &Path, cx: &mut Context<Self>) {
        let p = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            let _ = this.update(cx, |f, cx| f.check_buffers(Some(HashSet::from([p])), cx));
        }).detach();
    }
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current.clone() else { return };
        let Some(b) = self.buffers.get_mut(&path) else { return };
        if b.source.dirty() && !b.confirm_reload { b.confirm_reload = true; cx.notify(); return; }
        match files::read(&path) {
            Ok(disk) => {
                b.source.clear_recovery(&recovery_dir());
                b.source.replace(disk); b.conflict = None; b.error = None; b.confirm_reload = false; b.recovered = false; b.projection_stale = true; b.field_stale = true; b.pending = false;
                self.recoveries.retain(|p| p != &path);
                self.refresh_field(&path, cx); self.reparse_now(&path, cx);
            }
            Err(e) => cx.emit(FolderEvent::Toast(ToastKind::Error, e.to_string())),
        }
        cx.notify();
    }
    /// The edits win: what is on disk now becomes the base for the next save.
    fn keep_mine(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current.clone() else { return };
        let Some(b) = self.buffers.get_mut(&path) else { return };
        b.recovered = false; b.confirm_reload = false;
        if b.conflict == Some(Conflict::Deleted) {
            match b.source.restore() { Ok(()) => { b.conflict = None; b.source.clear_recovery(&recovery_dir()); } Err(e) => b.error = Some(e.to_string()) }
        } else if let Ok(disk) = files::read(&path) {
            b.source.baseline = disk; b.conflict = None;
            self.save_buffer(&path, cx);
        }
        cx.notify();
    }
    fn close_current(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current.take() else { return };
        if let Some(b) = self.buffers.remove(&path) { b.source.clear_recovery(&recovery_dir()); }
        self.recoveries.retain(|p| p != &path);
        cx.notify();
    }
    fn save_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.current.clone() else { return };
        self.sync_from_editor(&path, cx);
        let Some(text) = self.buffers.get(&path).map(|b| b.source.text.clone()) else { return };
        let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
        let prompt = picked.is_none().then(|| cx.prompt_for_new_path(path.parent().unwrap_or(&self.root), path.file_name().and_then(|s| s.to_str())));
        cx.spawn_in(window, async move |this, cx| {
            let target = match prompt { Some(p) => p.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(target) = target else { return };
            let result = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).and_then(|mut f| { use std::io::Write; f.write_all(text.as_bytes()) });
            let _ = this.update(cx, |_, cx| cx.emit(match result {
                Ok(()) => FolderEvent::Toast(ToastKind::Info, crate::localized_format!("副本已保存：{}", "Copy saved: {}", target.display())),
                Err(e) => FolderEvent::Toast(ToastKind::Error, crate::localized_format!("无法保存副本：{}", "Could not save a copy: {}", e)),
            }));
        }).detach();
    }

    // ---- Git: one query at a time, never stale, read-only

    pub fn refresh_git(&mut self, cx: &mut Context<Self>) {
        if self.git_task.is_some() { self.git_again = true; return; }
        let root = self.root.clone();
        self.git_task = Some(cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let probe = cx.background_executor().spawn(async move { git::probe(&root) }).await;
            let took = started.elapsed();
            let _ = this.update(cx, |f, cx| { f.apply_git(probe); cx.notify(); });
            // However slow status is here, it never takes more than a third of the time.
            cx.background_executor().timer((took * 2).max(Duration::from_millis(500))).await;
            let _ = this.update(cx, |f, cx| { f.git_task = None; if std::mem::take(&mut f.git_again) { f.refresh_git(cx); } });
        }));
    }
    fn apply_git(&mut self, probe: git::Probe) {
        self.git = match (probe.repository, probe.snapshot, probe.error) {
            (Some(r), Some(s), None) => GitState::Ready(r, s),
            (r, _, Some(e)) => GitState::Failed(r, e),
            _ => GitState::None,
        };
        let (GitState::Ready(r, _) | GitState::Failed(Some(r), _)) = &self.git else { self.git_rows = Rc::default(); return };
        if self.watched_git.as_ref() != Some(&r.git_dir) && !r.git_dir.starts_with(&self.root) && let Some(w) = self.watcher.as_mut() {
            use notify::Watcher;
            if w.watch(&r.git_dir, notify::RecursiveMode::NonRecursive).is_ok() { self.watched_git = Some(r.git_dir.clone()); }
        }
        let GitState::Ready(_, snapshot) = &self.git else { return };
        const LIMIT: usize = 500;
        let mut rows = vec![];
        let mut group = |label: &'static str, staged: bool, keep: &dyn Fn(&FileStatus) -> bool| {
            let files: Vec<&FileStatus> = snapshot.files.iter().filter(|f| keep(f)).collect();
            if files.is_empty() { return; }
            rows.push(GitRow::Group(label, files.len()));
            rows.extend(files.iter().take(LIMIT).map(|f| GitRow::File((*f).clone(), staged)));
            if files.len() > LIMIT { rows.push(GitRow::More(files.len() - LIMIT)); }
        };
        group("冲突", false, &FileStatus::conflicted);
        group("已暂存", true, &FileStatus::staged);
        group("未暂存", false, &FileStatus::unstaged);
        group("未跟踪", false, &FileStatus::untracked);
        self.git_rows = Rc::new(rows);
    }
    fn show_diff(&mut self, file: FileStatus, staged: bool, cx: &mut Context<Self>) {
        let GitState::Ready(repo, _) = &self.git else { return };
        let root = repo.root.clone();
        self.diff = Some(Diff { file: file.clone(), staged, patch: None, error: None, list: ListState::new(0, ListAlignment::Top, px(400.)) });
        cx.spawn(async move |this, cx| {
            let f = file.clone();
            let result = cx.background_executor().spawn(async move { git::diff(&root, &f, staged).map(|s| git::parse_patch(&s)) }).await;
            let _ = this.update(cx, |f, cx| {
                let Some(d) = f.diff.as_mut().filter(|d| d.file == file && d.staged == staged) else { return };
                match result {
                    Ok(patch) => { d.list = ListState::new(patch.rows.len(), ListAlignment::Top, px(400.)); d.patch = Some(Rc::new(patch)); }
                    Err(e) => d.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    // ---- modes and quick open

    pub fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = false; self.diff = None;
        let previous = std::mem::replace(&mut self.mode, mode);
        if let Some(path) = self.current.clone() {
            self.sync_from_editor(&path, cx);
            self.refresh_field(&path, cx);
            let read_only = mode == Mode::Read;
            if let Some(b) = self.buffers.get(&path) {
                let stale = b.projection_stale;
                b.editor.update(cx, |e, cx| e.set_read_only(read_only, cx));
                if stale && mode != Mode::Source { self.schedule_parse(&path, 0, window, cx); }
            }
        }
        if previous != mode { self.focus_current(window, cx); }
        cx.notify();
    }
    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.open { self.close_palette(window, cx); return; }
        self.palette.open = true; self.palette.selected = 0;
        self.palette_field.update(cx, |f, cx| { f.set_text("", cx); });
        window.focus(&self.palette_field.read(cx).focus_handle(cx));
        let root = self.root.clone();
        cx.spawn(async move |this, cx| {
            let (paths, truncated) = cx.background_executor().spawn(async move { files::index(&root, 100_000) }).await;
            let _ = this.update(cx, |f, cx| { f.palette.index = Some(Rc::new(paths)); f.palette.truncated = truncated; let q = f.palette_field.read(cx).text.clone(); f.filter_palette(&q); cx.notify(); });
        }).detach();
        self.filter_palette("");
        cx.notify();
    }
    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) { self.palette.stop(); self.palette.open = false; self.focus_current(window, cx); cx.notify(); }
    fn filter_palette(&mut self, query: &str) {
        let Some(index) = self.palette.index.clone() else { return };
        self.palette.selected = 0;
        if query.trim().is_empty() {
            // Where you have just been, then the shallowest files.
            let recent: Vec<PathBuf> = self.recent_files.iter().filter(|p| self.current.as_ref() != Some(*p)).cloned().collect();
            let mut rest: Vec<&PathBuf> = index.iter().filter(|p| !recent.contains(p)).collect();
            rest.sort_by_key(|p| p.components().count());
            self.palette.files = recent.iter().cloned().chain(rest.into_iter().cloned()).take(40).collect();
            self.palette.hits.clear();
            return;
        }
        let mut hits: Vec<((u8, usize, usize), &PathBuf)> = index.iter().filter_map(|p| files::rank(&p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy(), query).map(|r| (r, p))).collect();
        hits.sort_by_key(|(r, _)| *r);
        self.palette.files = hits.into_iter().take(20).map(|(_, p)| p.clone()).collect();
    }
    /// Inside the files, after a pause in typing: replaced by the next keystroke and
    /// cancelled for real, because a walk of a big folder must not outlive its query.
    fn search_contents(&mut self, query: &str, cx: &mut Context<Self>) {
        self.palette.stop();
        self.palette.hits.clear(); self.palette.cut = false;
        if query.trim().chars().count() < 2 { return; }
        let cancel = Arc::new(AtomicBool::new(false));
        self.palette.cancel = Some(cancel.clone()); self.palette.searching = true;
        let (root, q) = (self.root.clone(), query.trim().to_string());
        self.palette.task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(200)).await;
            let (hits, cut) = cx.background_executor().spawn(async move { files::search_content(&root, &q, 50, Duration::from_secs(3), &cancel) }).await;
            let _ = this.update(cx, |f, cx| { f.palette.hits = hits; f.palette.cut = cut; f.palette.searching = false; f.palette.task = None; cx.notify(); });
        }));
    }
    fn palette_choose(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.palette_field.read(cx).text.trim().to_string();
        let target = match self.palette.files.get(i) {
            Some(path) => Some((path.clone(), None)),
            None => self.palette.hits.get(i - self.palette.files.len()).map(|h| (h.path.clone(), Some(query))),
        };
        let Some((path, find)) = target else { return };
        self.close_palette(window, cx);
        self.pending_find = find;
        self.open(path, None, How::Record, window, cx);
    }

    // ---- what Workspace draws

    pub fn sidebar_el(&mut self, width: f32, update: Option<AnyElement>, cx: &mut Context<Self>) -> AnyElement {
        let name = self.root.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let header = div().h(px(crate::app::HEADER_H)).flex_none().flex().items_center().pl(px(theme::TRAFFIC_LIGHTS_W)).pr(px(12.)).title_bar()
            .when(!cfg!(target_os = "windows"), |d| d.child(
                div().id("collapse-sidebar").no_drag().flex_none().ml(px(4.)).size(px(28.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer().hover(|s| s.bg(Theme::sidebar_hover()))
                    .tip(crate::settings::tr("收起侧栏", "Hide sidebar"), Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                    .child(icon("panel-left-close", 17., Theme::icon()))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::app::ToggleSidebar), cx))))
            .child(div().flex_1()).children(update);
        let folders_open = self.folders_menu;
        let title = div().flex_none().pt(px(8.)).pb(px(4.)).px(px(SB_X)).child(
            div().id("folder-switch").h(px(ROW_H)).pl(px(6.)).pr(px(8.)).flex().items_center().rounded(px(ROW_R)).cursor_pointer().text_size(px(13.)).line_height(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text())
                .when(folders_open, |d| d.bg(Theme::sidebar_selected())).when(!folders_open, |d| d.hover(|s| s.bg(Theme::sidebar_hover())))
                .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().mr(px(9.)).child(icon("folder", ROW_ICON, Theme::icon())))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                .child(icon("chevron-down", 12., Theme::text_caption()))
                .tip(&self.root.display().to_string(), None)
                .on_click(cx.listener(|f, _, _, cx| { f.folders_menu = !f.folders_menu; cx.notify(); })));
        let tree_len = self.tree.len();
        let tree = uniform_list("folder-tree", tree_len, cx.processor(|f, range: std::ops::Range<usize>, _, cx| range.map(|i| f.tree_row(i, cx)).collect::<Vec<_>>())).track_scroll(self.tree_scroll.clone()).flex_1().min_h_0().px(px(SB_X));
        let retained: Vec<PathBuf> = self.recoveries.iter().filter(|p| !self.buffers.contains_key(*p)).cloned().collect();
        let footer = div().flex_none().flex().flex_col()
            .child(footer_line())
            .child(self.git_panel(cx))
            .child(footer_line())
            .child(footer_slot(footer_row("ft-settings", "settings-gear", crate::settings::tr("设置", "Settings")).on_click(|_, window, cx| window.dispatch_action(Box::new(crate::app::OpenSettings), cx))));
        div().w(px(width)).flex_none().h_full().flex().flex_col().relative().child(header).child(title)
            .when(tree_len == 0, |d| d.child(div().flex_1().min_h_0().px(px(SB_X + 6.)).py(px(8.)).text_size(px(12.)).text_color(Theme::text_caption()).child(crate::settings::tr("没有 Markdown 文件", "No Markdown files here"))))
            .when(tree_len > 0, |d| d.child(tree))
            .when(!retained.is_empty(), |d| d.child(div().flex_none().px(px(SB_X)).pb(px(6.)).children(retained.into_iter().enumerate().map(|(i, p)| {
                let label = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                div().id(("retained", i)).h(px(ROW_H)).pl(px(6.)).flex().items_center().gap(px(9.)).rounded(px(ROW_R)).cursor_pointer().hover(|s| s.bg(Theme::sidebar_hover())).text_size(px(13.)).text_color(Theme::sidebar_text())
                    .child(div().size(px(20.)).flex().items_center().justify_center().child(icon("info", 14., theme::c(craft::R400))))
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(crate::localized_format!("未保存的修改：{}", "Unsaved edits: {}", label)))
                    .on_click(cx.listener(move |f, _, window, cx| f.open(p.clone(), None, How::Record, window, cx)))
            }))))
            .child(footer)
            .children(self.folders_menu_el(cx))
            .into_any_element()
    }
    fn tree_row(&mut self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.tree.get(i).cloned() {
            Some(TreeRow::Note(text, depth)) => div().h(px(ROW_H)).pl(px(6. + 18. * depth as f32 + 29.)).flex().items_center().text_size(px(12.)).text_color(Theme::text_caption()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(text).into_any_element(),
            Some(TreeRow::Entry(entry, depth)) => {
                let path = entry.path.clone();
                let selected = self.current.as_ref() == Some(&path) && self.diff.is_none();
                let open = self.expanded.contains(&path);
                let decoration = if let GitState::Ready(_, s) = &self.git { s.decorations.get(&path).copied() } else { None };
                let dirty = self.buffers.get(&path).is_some_and(|b| b.pending || b.source.dirty());
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let outside = entry.outside;
                let glyph: AnyElement = if outside { icon("info", 14., Theme::text_disabled()).into_any_element() }
                    else if entry.directory { icon("tree-triangle", 10., theme::c(craft::N500)).when(open, |s| s.with_transformation(Transformation::rotate(radians(std::f32::consts::FRAC_PI_2)))).into_any_element() }
                    else { icon("doc", ROW_ICON, if selected { Theme::text() } else { Theme::text_caption() }).into_any_element() };
                let (directory, target) = (entry.directory, path.clone());
                div().id(("tree-row", i)).w_full().h(px(ROW_H)).flex().items_center().rounded(px(ROW_R)).pl(px(6. + 18. * depth as f32)).pr(px(6.))
                    .when(!outside, |d| d.cursor_pointer())
                    .when(selected, |d| d.bg(Theme::sidebar_selected())).when(!selected && !outside, |d| d.hover(|s| s.bg(Theme::sidebar_hover())))
                    .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().child(glyph))
                    .child(div().pl(px(9.)).flex_1().min_w_0().text_size(px(13.)).line_height(px(ROW_H)).overflow_hidden().whitespace_nowrap().text_ellipsis()
                        .text_color(if outside { Theme::text_disabled() } else if selected { Theme::text() } else { Theme::sidebar_text() }).child(name))
                    .when(entry.link, |d| d.tip(if outside { crate::settings::tr("符号链接指向文件夹之外，不会打开", "This symlink points outside the folder and won't open") } else { crate::settings::tr("符号链接", "Symlink") }, None))
                    // Unsaved here is a dot; Git's on-disk state is a separate letter.
                    .when(dirty, |d| d.child(div().flex_none().ml(px(6.)).size(px(6.)).rounded_full().bg(Theme::primary())))
                    .when_some(decoration, |d, g| d.child(git_badge(g, directory)))
                    .on_click(cx.listener(move |f, _, window, cx| { if outside {} else if directory { f.toggle_dir(target.clone(), cx); } else { f.open(target.clone(), None, How::Record, window, cx); } }))
                    .into_any_element()
            }
            None => div().into_any_element(),
        }
    }
    fn git_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use crate::settings::tr;
        let (label, detail, error): (String, Option<String>, Option<String>) = match &self.git {
            GitState::Detecting => (tr("正在检查 Git…", "Checking Git…").into(), None, None),
            GitState::None => (tr("不在 Git 仓库中", "Not in a Git repository").into(), None, None),
            GitState::Failed(None, e) => (tr("无法检查 Git", "Could not check Git").into(), None, Some(e.clone())),
            GitState::Failed(Some(r), e) => (head_label(r), Some(tr("状态不可用", "status unavailable").into()), Some(e.clone())),
            GitState::Ready(r, s) => (head_label(r), Some(if s.files.is_empty() { tr("无更改", "clean").to_string() } else { crate::localized_format!("{} 个更改", "{} changes", s.files.len()) }), None),
        };
        let identity = match &self.git {
            GitState::Ready(r, _) | GitState::Failed(Some(r), _) => Some(r.root.display().to_string()),
            _ => None,
        };
        let openable = !self.git_rows.is_empty() || error.is_some();
        let open = self.git_open && openable;
        let rows = self.git_rows.clone();
        let height = (rows.len() as f32 * 26.).min(280.);
        div().flex_none().flex().flex_col().px(px(SB_X)).py(px(4.))
            .child(div().id("git-summary").h(px(ROW_H)).pl(px(6.)).pr(px(4.)).flex().items_center().rounded(px(ROW_R)).text_size(px(12.)).text_color(Theme::sidebar_text())
                .when(openable, |d| d.cursor_pointer().hover(|s| s.bg(Theme::sidebar_hover())))
                .child(div().size(px(20.)).flex_none().flex().items_center().justify_center().mr(px(9.)).child(icon("git-branch", 16., if error.is_some() { theme::c(craft::R400) } else { Theme::icon() })))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(label))
                .when_some(detail, |d, t| d.child(div().flex_none().pl(px(6.)).text_color(if error.is_some() { theme::c(craft::R400) } else { Theme::text_caption() }).child(t)))
                .when_some(identity, |d, t| d.tip(&t, None))
                .on_click(cx.listener(move |f, _, _, cx| { if openable { f.git_open = !f.git_open; cx.notify(); } })))
            .when(open && error.is_some(), |d| d.child(div().px(px(6.)).pb(px(6.)).text_size(px(11.)).line_height(px(16.)).text_color(Theme::text_caption()).max_h(px(120.)).overflow_hidden().child(error.clone().unwrap_or_default())))
            .when(open && !rows.is_empty(), |d| d.child(uniform_list("git-rows", rows.len(), cx.processor(|f, range: std::ops::Range<usize>, _, cx| range.map(|i| f.git_row(i, cx)).collect::<Vec<_>>())).track_scroll(self.git_scroll.clone()).h(px(height))))
            .into_any_element()
    }
    fn git_row(&mut self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.git_rows.clone();
        match rows.get(i) {
            Some(GitRow::Group(label, n)) => div().h(px(26.)).pl(px(6.)).flex().items_center().text_size(px(11.)).text_color(Theme::text_caption()).font_weight(FontWeight::SEMIBOLD).child(format!("{} {n}", group_label(label))).into_any_element(),
            Some(GitRow::More(n)) => div().h(px(26.)).pl(px(6.)).flex().items_center().text_size(px(11.)).text_color(Theme::text_caption()).child(crate::localized_format!("还有 {} 个未列出", "{} more not listed", n)).into_any_element(),
            Some(GitRow::File(file, staged)) => {
                let (file, staged) = (file.clone(), *staged);
                let on = self.diff.as_ref().is_some_and(|d| d.file == file && d.staged == staged);
                let code = if file.conflicted() { 'U' } else if file.untracked() { '?' } else if staged { file.index } else { file.worktree };
                let name = file.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let dir = file.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                let path = match &file.old_path { Some(old) => format!("{} → {}", old.display(), file.path.display()), None => file.path.display().to_string() };
                div().id(("git-row", i)).w_full().h(px(26.)).pl(px(6.)).pr(px(4.)).flex().items_center().gap(px(6.)).rounded(px(6.)).cursor_pointer().text_size(px(12.)).text_color(Theme::sidebar_text())
                    .when(on, |d| d.bg(Theme::sidebar_selected())).when(!on, |d| d.hover(|s| s.bg(Theme::sidebar_hover())))
                    .child(div().w(px(14.)).flex_none().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(code_color(code)).child(code.to_string()))
                    .child(div().flex_shrink().min_w(px(40.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                    .child(div().flex_1().min_w_0().text_size(px(11.)).text_color(Theme::text_caption()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(dir))
                    .tip(&path, None)
                    .on_click(cx.listener(move |f, _, _, cx| f.show_diff(file.clone(), staged, cx)))
                    .into_any_element()
            }
            None => div().into_any_element(),
        }
    }
    fn folders_menu_el(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::settings::tr;
        if !self.folders_menu { return None; }
        let row = |id: &'static str, glyph: &'static str, label: String| { let g = SharedString::from(id); menu_row(id, g.clone()).text_size(px(13.)).child(menu_glyph(glyph, 16., g.clone())).child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(menu_label(label, g, gpui::white()))) };
        let recents: Vec<PathBuf> = recent().into_iter().filter(|p| *p != self.root).take(6).collect();
        Some(deferred(div().absolute().top(px(crate::app::HEADER_H + 44.)).left(px(SB_X)).right(px(SB_X)).child(popover().occlude().py(px(5.))
            .children(recents.into_iter().enumerate().map(|(i, p)| {
                let g = SharedString::from(format!("recent-{i}"));
                let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let full = p.display().to_string();
                menu_row(("recent-folder", i), g.clone()).text_size(px(13.)).child(menu_glyph("folder", 16., g.clone())).child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(menu_label(name, g, gpui::white()))).tip(&full, None)
                    .on_click(cx.listener(move |f, _, window, cx| { f.folders_menu = false; cx.notify(); let p = p.clone(); window.defer(cx, move |window, cx| { if let Some(h) = window.window_handle().downcast::<Workspace>() { let _ = h.update(cx, |w, window, cx| w.enter_folder(p, None, window, cx)); } }); }))
            }))
            .child(row("folder-open-other", "folder-input", tr("打开其他文件夹…", "Open Another Folder…").into()).on_click(cx.listener(|f, _, window, cx| { f.folders_menu = false; cx.notify(); window.dispatch_action(Box::new(OpenFolder), cx); })))
            .child(row("folder-hidden", "eye", if self.show_hidden { tr("隐藏被忽略和隐藏的文件", "Hide Ignored and Hidden Files") } else { tr("显示被忽略和隐藏的文件", "Show Ignored and Hidden Files") }.into()).on_click(cx.listener(|f, _, _, cx| { f.show_hidden = !f.show_hidden; f.folders_menu = false; let dirs = f.expanded.iter().cloned().collect(); f.relist(dirs, cx); cx.notify(); })))
            .child(row("folder-notebook", "home", tr("返回笔记库", "Return to Notebook").into()).on_click(cx.listener(|f, _, window, cx| { f.folders_menu = false; cx.notify(); window.dispatch_action(Box::new(CloseFolder), cx); })))
        )).with_priority(3).into_any_element())
    }
    pub fn top_bar_el(&mut self, sidebar_open: bool, can_back: bool, can_forward: bool, cx: &mut Context<Self>) -> AnyElement {
        use crate::settings::tr;
        let path = self.current.clone();
        let b = path.as_ref().and_then(|p| self.buffers.get(p));
        let dirty = b.is_some_and(|b| b.pending || b.source.dirty());
        let failed = b.and_then(|b| b.error.clone());
        let rel = path.as_ref().map(|p| p.strip_prefix(&self.root).unwrap_or(p).to_path_buf()).unwrap_or_default();
        let name = self.diff.as_ref().map(|d| d.file.path.display().to_string()).or_else(|| rel.file_name().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| tr("选择文件", "Choose a file").into());
        let crumbs: Vec<String> = if self.diff.is_some() { vec![] } else { rel.parent().into_iter().flat_map(|p| p.components()).map(|c| c.as_os_str().to_string_lossy().into_owned()).collect() };
        let sep = || div().mx(px(3.)).flex_none().child(icon("chevron-right", 12., theme::c(craft::N500))).into_any_element();
        let mut trail: Vec<AnyElement> = vec![];
        let count = crumbs.len();
        for (i, c) in crumbs.into_iter().enumerate() {
            if count > 2 && i < count - 2 {
                if i == 0 { trail.push(div().flex_none().text_color(Theme::text_caption()).child("…").into_any_element()); trail.push(sep()); }
                continue;
            }
            trail.push(div().min_w(px(20.)).max_w(px(160.)).flex_shrink().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(Theme::text_caption()).child(c).into_any_element());
            trail.push(sep());
        }
        let record_more = self.more_bounds.clone();
        let menu_open = self.menu;
        div().id("top-bar").relative().h(px(crate::app::HEADER_H)).flex_none().flex().items_center().pr(px(crate::ui::header_right_inset()))
            .pl(if sidebar_open { px(12.) } else { px(theme::TRAFFIC_LIGHTS_W) }).title_bar()
            .when(!sidebar_open, |d| d.child(
                div().id("open-sidebar").no_drag().flex_none().ml(px(4.)).size(px(28.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer().hover(|s| s.bg(Theme::fill_hover_strong()))
                    .tip(tr("展开侧栏", "Show sidebar"), Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                    .child(icon("panel-left-open", 17., Theme::icon()))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::app::ToggleSidebar), cx))))
            .child(nav_button("nav-back", false, can_back).tip(tr("后退", "Back"), Some("⌘ + ⌥ + ←")).on_click(|_, window, cx| window.dispatch_action(Box::new(Back), cx)))
            .child(nav_button("nav-forward", true, can_forward).tip(tr("前进", "Forward"), Some("⌘ + ⌥ + →")).on_click(|_, window, cx| window.dispatch_action(Box::new(Forward), cx)))
            .child(div().ml(px(8.)).flex_1().min_w_0().h(px(20.)).flex().items_center().text_size(px(13.)).line_height(px(20.)).overflow_hidden()
                .children(trail)
                .child(div().min_w(px(40.)).flex_shrink().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(name))
                .when(dirty, |d| d.child(div().flex_none().ml(px(8.)).text_size(px(12.)).text_color(Theme::text_caption()).child(tr("未保存", "Unsaved"))))
                .when_some(failed, |d, e| d.child(div().flex_shrink().min_w_0().ml(px(8.)).max_w(px(220.)).text_size(px(12.)).text_color(theme::c(craft::R400)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(e))))
            .child(div().pl(px(8.)).gap(px(4.)).h(px(crate::app::HEADER_ACTIONS_H)).flex_none().flex().items_center()
                .child(crate::ui::header_icon("doc-more", "ellipsis").relative().when(menu_open, |d| d.bg(Theme::fill_hover())).when(!menu_open, |d| d.tip(tr("更多", "More"), None))
                    .child(canvas(move |b, _, _| record_more.set(Some(b)), |_, _, _, _| {}).absolute().inset_0())
                    .on_click(cx.listener(|f, _, _, cx| { f.menu = !f.menu; cx.notify(); })))
                .child(crate::ui::header_icon("doc-search", "search").tip(tr("查找文件", "Find a file"), Some(crate::shortcuts::hint("workspace::FocusSearch", "⌘ + J"))).on_click(cx.listener(|f, _, window, cx| f.open_palette(window, cx)))))
            .into_any_element()
    }
    pub fn menu_el(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::settings::tr;
        if !self.menu { return None; }
        let at = self.more_bounds.get()?;
        let mode = self.mode;
        let row = |id: &'static str, glyph: &'static str, label: &'static str, keys: Option<&'static str>, on: bool| {
            let g = SharedString::from(id);
            menu_row(id, g.clone()).text_size(px(13.)).child(menu_glyph(glyph, 16., g.clone())).child(div().flex_1().min_w_0().child(menu_label(label, g.clone(), gpui::white())))
                .when(on, |d| d.child(icon("check", 14., Theme::primary())))
                .when_some(keys.filter(|_| !on), |d, k| d.child(div().flex_none().text_size(px(11.)).text_color(Theme::text_caption()).child(k)))
        };
        let path = self.current.clone();
        let diffable = path.as_ref().and_then(|p| match &self.git { GitState::Ready(r, s) => s.files.iter().find(|f| r.root.join(&f.path) == *p).cloned(), _ => None });
        let vw = f32::from(window.viewport_size().width);
        let width = 248f32.min(vw - 16.);
        let left = (f32::from(at.right()) - width).max(8.);
        Some(deferred(
            div().absolute().inset_0().occlude().on_mouse_down(MouseButton::Left, cx.listener(|f, _, _, cx| { f.menu = false; cx.notify(); })).child(
            div().absolute().top(at.bottom() + px(4.)).left(px(left)).w(px(width)).child(popover().occlude().py(px(5.)).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(row("mode-edit", "doc", tr("编辑", "Editing"), Some("⌘⌥1"), mode == Mode::Edit).on_click(cx.listener(|f, _, window, cx| f.set_mode(Mode::Edit, window, cx))))
                .child(row("mode-read", "eye", tr("阅读", "Reading"), Some("⌘⌥2"), mode == Mode::Read).on_click(cx.listener(|f, _, window, cx| f.set_mode(Mode::Read, window, cx))))
                .child(row("mode-source", "code", tr("源码", "Source"), Some("⌘⌥3"), mode == Mode::Source).on_click(cx.listener(|f, _, window, cx| f.set_mode(Mode::Source, window, cx))))
                .child(row("mode-split", "copy", tr("分屏", "Split"), Some("⌘⌥4"), mode == Mode::Split).on_click(cx.listener(|f, _, window, cx| f.set_mode(Mode::Split, window, cx))))
                .child(div().mx(px(8.)).my(px(5.)).h(px(1.)).bg(Theme::line_border()))
                .when_some(diffable, |d, file| { let staged = file.staged() && !file.unstaged(); d.child(row("show-diff", "git-branch", tr("查看更改", "Show Changes"), None, false).on_click(cx.listener(move |f, _, _, cx| { f.menu = false; f.show_diff(file.clone(), staged, cx); }))) })
                .child(row("save-copy", "copy", tr("保存副本…", "Save a Copy…"), None, false).on_click(cx.listener(|f, _, window, cx| { f.menu = false; f.save_copy(window, cx); cx.notify(); })))
                .when_some(path, |d, p| d.child(row("copy-path", "copy", tr("复制路径", "Copy Path"), None, false).on_click(cx.listener(move |f, _, _, cx| { f.menu = false; cx.write_to_clipboard(ClipboardItem::new_string(p.display().to_string())); cx.notify(); }))))
                .child(row("close-folder", "home", tr("返回笔记库", "Return to Notebook"), None, false).on_click(cx.listener(|f, _, window, cx| { f.menu = false; window.dispatch_action(Box::new(CloseFolder), cx); })))
            ))).with_priority(3).into_any_element())
    }
    fn banner(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::settings::tr;
        let path = self.current.clone()?;
        let b = self.buffers.get(&path)?;
        let button = |id: &'static str, text: &'static str| div().id(id).h(px(26.)).px(px(10.)).flex_none().flex().items_center().rounded(px(6.)).cursor_pointer().text_size(px(12.)).bg(Theme::fill_hover()).hover(|s| s.bg(Theme::fill_hover_strong())).child(text);
        let (text, primary): (String, Option<&'static str>) = match (b.conflict, b.source.dirty()) {
            (Some(Conflict::Changed), _) => (tr("此文件已在磁盘上更改。你的修改已保留，尚未写入。", "This file changed on disk. Your edits are kept and have not been written.").into(), Some(tr("用我的修改覆盖", "Keep My Edits"))),
            (Some(Conflict::Deleted), true) => (tr("此文件已被移动或删除。你的修改已保留。", "This file was moved or deleted. Your edits are kept.").into(), Some(tr("在原位置恢复", "Restore File"))),
            (Some(Conflict::Deleted), false) => (tr("此文件已被移动或删除。", "This file was moved or deleted.").into(), None),
            (None, _) if b.recovered => (tr("已恢复上次未保存的修改。", "Unsaved edits from last time were recovered.").into(), Some(tr("保存", "Save"))),
            _ => return None,
        };
        let deleted = b.conflict == Some(Conflict::Deleted);
        let confirm = b.confirm_reload;
        Some(div().flex_none().w_full().min_w_0().px(px(18.)).py(px(8.)).flex().flex_wrap().items_center().gap(px(8.)).bg(Theme::fill_hover()).text_size(px(12.)).line_height(px(18.))
            .child(div().flex_1().min_w(px(200.)).child(text))
            .when_some(primary, |d, label| d.child(button("banner-keep", label).on_click(cx.listener(|f, _, _, cx| f.keep_mine(cx)))))
            .child(button("banner-copy", tr("保存副本…", "Save a Copy…")).on_click(cx.listener(|f, _, window, cx| f.save_copy(window, cx))))
            .when(!deleted, |d| d.child(button("banner-reload", if confirm { tr("确认放弃修改", "Discard My Edits") } else { tr("从磁盘重新加载", "Reload from Disk") }).on_click(cx.listener(|f, _, _, cx| f.reload(cx)))))
            .when(deleted, |d| d.child(button("banner-close", if confirm { tr("确认放弃修改", "Discard My Edits") } else { tr("关闭", "Close") }).on_click(cx.listener(|f, _, _, cx| {
                let dirty = f.current.as_ref().and_then(|p| f.buffers.get(p)).is_some_and(|b| b.source.dirty());
                if let Some(b) = f.current.clone().and_then(|p| f.buffers.get_mut(&p)) && dirty && !b.confirm_reload { b.confirm_reload = true; cx.notify(); return; }
                f.close_current(cx);
            }))))
            .into_any_element())
    }
    pub fn pane_el(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        use crate::settings::tr;
        let banner = self.banner(cx);
        let path = self.current.clone();
        let b = path.as_ref().and_then(|p| self.buffers.get(p));
        let diagnostic = b.and_then(|b| b.projection.diagnostic.clone()).filter(|_| self.mode != Mode::Source && self.diff.is_none());
        let body: AnyElement = if let Some(d) = &self.diff {
            let title = if d.file.untracked() { tr("未跟踪文件的内容", "Untracked file contents") } else if d.staged { tr("已暂存的更改（索引与 HEAD）", "Staged changes (index vs HEAD)") } else { tr("未暂存的更改（工作区与索引）", "Unstaged changes (working tree vs index)") };
            let patch = d.patch.clone();
            let error = d.error.clone();
            let list_state = d.list.clone();
            let counts = patch.as_ref().filter(|p| p.added + p.removed > 0).map(|p| (p.added, p.removed));
            let target = self.git_root().map(|r| r.join(&d.file.path)).filter(|p| files::supported(p) && p.exists());
            let action = |id: &'static str, label: &'static str| div().id(id).h(px(26.)).px(px(10.)).flex_none().flex().items_center().rounded(px(6.)).cursor_pointer().bg(Theme::fill_hover()).hover(|s| s.bg(Theme::fill_hover_strong())).text_color(Theme::text()).child(label);
            let message = |text: SharedString| div().flex_1().px(px(18.)).text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).child(text).into_any_element();
            div().flex_1().min_h_0().w_full().min_w_0().flex().flex_col()
                .child(div().flex_none().min_h(px(36.)).px(px(18.)).flex().items_center().gap(px(8.)).text_size(px(12.)).text_color(Theme::text_caption())
                    .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(title))
                    .when_some(counts, |d, (a, r)| d.child(div().flex_none().text_color(theme::c(craft::old::G500)).child(format!("+{a}"))).child(div().flex_none().text_color(theme::c(craft::R400)).child(format!("−{r}"))))
                    .when_some(target, |d, p| d.child(action("diff-open", tr("打开文件", "Open File")).on_click(cx.listener(move |f, _, window, cx| { f.diff = None; f.open(p.clone(), None, How::Record, window, cx); }))))
                    .child(action("diff-close", tr("关闭", "Close")).on_click(cx.listener(|f, _, _, cx| { f.diff = None; cx.notify(); }))))
                .child(match (patch, error) {
                    (_, Some(e)) => message(e.into()),
                    (None, None) => message(tr("正在读取差异…", "Reading the diff…").into()),
                    (Some(p), None) if p.rows.is_empty() => message(tr("没有差异", "No differences").into()),
                    (Some(patch), None) => list(list_state, move |i, _, _| patch_row(patch.rows.get(i))).flex_1().min_h_0().w_full().into_any_element(),
                }).into_any_element()
        } else if let Some(b) = b {
            let source = |available: f32| {
                let pad = ((available - 860.) / 2.).max(20.);
                div().id("folder-source").flex_1().min_w_0().min_h_0().overflow_y_scroll().track_scroll(&b.scroll).px(px(pad)).pt(px(16.)).pb(px(48.)).child(div().w_full().min_w_0().child(b.field.clone()))
            };
            let editor = || div().relative().flex_1().min_w_0().min_h_0().h_full().child(b.editor.clone());
            match self.mode {
                Mode::Edit | Mode::Read => editor().into_any_element(),
                Mode::Source => div().flex_1().min_h_0().w_full().min_w_0().flex().child(source(width)).into_any_element(),
                Mode::Split => {
                    // Side by side only when both sides stay readable.
                    let wide = width >= 900.;
                    let pane = |side: Div| side.flex_1().min_w_0().min_h_0().flex().overflow_hidden();
                    div().flex_1().min_h_0().w_full().min_w_0().flex().when(!wide, |d| d.flex_col())
                        .child(pane(div().child(source(if wide { width / 2. } else { width }))))
                        .child(div().flex_none().bg(Theme::line_divider()).when(wide, |d| d.w(px(1.)).h_full()).when(!wide, |d| d.h(px(1.)).w_full()))
                        .child(pane(div().child(editor()))).into_any_element()
                }
            }
        } else {
            let opening = self.opening.is_some();
            div().flex_1().flex().items_center().justify_center().text_size(px(13.)).text_color(Theme::text_caption()).child(if opening { tr("正在打开…", "Opening…") } else { tr("从左侧选择一个文件", "Choose a file from the sidebar") }).into_any_element()
        };
        let palette = self.palette_el(cx);
        div().relative().flex_1().min_h_0().w_full().min_w_0().flex().flex_col()
            .children(banner)
            .when_some(diagnostic, |d, t| d.child(div().flex_none().w_full().min_w_0().px(px(18.)).py(px(5.)).text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).bg(Theme::fill_hover()).child(t)))
            .child(body)
            .children(palette)
            .into_any_element()
    }
    fn git_root(&self) -> Option<PathBuf> { match &self.git { GitState::Ready(r, _) => Some(r.root.clone()), _ => None } }
    fn palette_el(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::settings::tr;
        if !self.palette.open { return None; }
        let root = self.root.clone();
        let selected = self.palette.selected;
        let files = self.palette.files.clone();
        let offset = files.len();
        let rows = files.into_iter().enumerate().map(|(i, p)| {
            let g = SharedString::from(format!("palette-{i}"));
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let dir = p.parent().and_then(|d| d.strip_prefix(&root).ok()).map(|d| d.display().to_string()).unwrap_or_default();
            menu_row(("palette-row", i), g).text_size(px(13.)).when(i == selected, |d| d.bg(Theme::fill_hover_strong()))
                .child(div().flex_none().max_w(px(260.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                .child(div().flex_1().min_w_0().pl(px(8.)).text_size(px(12.)).text_color(Theme::text_caption()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(dir))
                .on_click(cx.listener(move |f, _, window, cx| f.palette_choose(i, window, cx)))
        }).collect::<Vec<_>>();
        let hits = self.palette.hits.clone().into_iter().enumerate().map(|(n, h)| {
            let i = offset + n;
            let g = SharedString::from(format!("palette-hit-{n}"));
            let place = format!("{}:{}", h.path.strip_prefix(&root).unwrap_or(&h.path).display(), h.line);
            menu_row(("palette-hit", n), g).text_size(px(12.)).when(i == selected, |d| d.bg(Theme::fill_hover_strong()))
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(h.text))
                .child(div().flex_none().max_w(px(200.)).pl(px(8.)).text_size(px(11.)).text_color(Theme::text_caption()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(place))
                .on_click(cx.listener(move |f, _, window, cx| f.palette_choose(i, window, cx)))
        }).collect::<Vec<_>>();
        let has_hits = !hits.is_empty();
        let (searching, cut) = (self.palette.searching, self.palette.cut);
        let indexing = self.palette.index.is_none();
        let truncated = self.palette.truncated;
        Some(deferred(div().absolute().top(px(8.)).left_0().right_0().flex().justify_center().px(px(16.)).child(
            popover().occlude().w_full().max_w(px(520.)).flex().flex_col()
                .child(div().h(px(40.)).px(px(12.)).flex().items_center().gap(px(8.)).border_b_1().border_color(Theme::line_border()).child(icon("search", 15., Theme::text_caption())).child(self.palette_field.clone()))
                .child(div().id("palette-results").max_h(px(320.)).overflow_y_scroll().py(px(5.)).children(rows)
                    .when(has_hits, |d| d.child(div().px(px(12.)).pt(px(8.)).pb(px(3.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text_caption()).child(tr("文件内容", "In files"))))
                    .children(hits)
                    .when(searching, |d| d.child(div().px(px(12.)).py(px(6.)).text_size(px(12.)).text_color(Theme::text_caption()).child(tr("正在搜索内容…", "Searching inside files…"))))
                    .when(cut, |d| d.child(div().px(px(12.)).py(px(6.)).text_size(px(12.)).text_color(Theme::text_caption()).child(tr("结果过多或耗时过长，只显示部分结果", "Too many results or taking too long; showing some of them"))))
                    .when(indexing, |d| d.child(div().px(px(12.)).py(px(6.)).text_size(px(12.)).text_color(Theme::text_caption()).child(tr("正在索引文件夹…", "Indexing the folder…"))))
                    .when(truncated, |d| d.child(div().px(px(12.)).py(px(6.)).text_size(px(12.)).text_color(Theme::text_caption()).child(tr("文件过多，只索引了前 100000 个", "Too many files; indexed the first 100,000")))))
        )).with_priority(4).into_any_element())
    }
    pub fn test_state(&self, cx: &App) -> String {
        let b = self.current.as_ref().and_then(|p| self.buffers.get(p));
        let doc = b.map(|b| b.editor.read(cx).core.doc.clone());
        let git = match &self.git { GitState::Detecting => "detecting".to_string(), GitState::None => "none".into(), GitState::Failed(r, e) => format!("failed repo={} error={e}", r.as_ref().map_or("-", |r| r.branch.as_str())), GitState::Ready(r, s) => format!("ready branch={} detached={} worktree={} changes={}", r.branch, r.detached, r.worktree, s.files.len()) };
        format!("path={} mode={:?} dirty={} revision={} conflict={:?} error={} title={:?} blocks={} meta={} editable={} buffers={} tree={} diff={:?} watcher={} undo={} text_sha={:x} top={} git=[{git}]",
            self.current.as_ref().map(|p| p.strip_prefix(&self.root).unwrap_or(p).display().to_string()).unwrap_or_default(), self.mode, b.is_some_and(|b| b.source.dirty()), b.map_or(0, |b| b.source.revision), b.and_then(|b| b.conflict), b.and_then(|b| b.error.as_deref()).unwrap_or(""),
            doc.as_ref().map(|d| d.title.text.clone()).unwrap_or_default(), doc.as_ref().map_or(0, |d| d.blocks.len()), b.map_or(0, |b| b.projection.meta.fields.len()), b.is_some_and(|b| !b.editor.read(cx).read_only), self.buffers.len(), self.tree.len(),
            self.diff.as_ref().map(|d| (d.file.path.display().to_string(), d.staged, d.patch.as_ref().map_or(0, |p| p.rows.len()))), self.watcher_error.as_deref().unwrap_or("ok"), b.is_some_and(|b| b.editor.read(cx).core.can_undo()), b.map_or(0, |b| { use std::hash::{Hash, Hasher}; let mut h = std::collections::hash_map::DefaultHasher::new(); b.source.text.hash(&mut h); h.finish() }), b.map_or(0, |b| b.editor.read(cx).list.logical_scroll_top().item_ix))
    }
    /// `FDL_SCRIPT` hooks: what a script can't reach by coordinates.
    pub fn test_command(&mut self, cmd: &str, arg: &str, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            "folder-open" => self.open(PathBuf::from(arg), None, How::Record, window, cx),
            "folder-link" => if let Some(p) = self.current.clone() { self.follow_link(&p, arg, window, cx); },
            "folder-caret" => {
                let n: Vec<usize> = arg.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let Some(b) = self.current.as_ref().and_then(|p| self.buffers.get(p)) { b.editor.update(cx, |e, cx| {
                    if let Some(id) = e.core.doc.blocks.get(n[0]).map(|x| x.id) { e.core.sel = crate::editor::core::Selection::caret(crate::editor::core::Pos::new(id, n[1])); cx.notify(); }
                }); }
            }
            "folder-action" => match arg { "keep" => self.keep_mine(cx), "reload" => self.reload(cx), "copy" => self.save_copy(window, cx), "close" => self.close_current(cx), _ => {} },
            "folder-diff" => {
                let Some((path, kind)) = arg.rsplit_once(' ') else { return };
                let file = if let GitState::Ready(_, s) = &self.git { s.files.iter().find(|f| f.path == Path::new(path)).cloned() } else { None };
                if let Some(file) = file { self.show_diff(file, kind == "staged", cx); }
            }
            "folder-dark" => { crate::settings::apply_appearance(if arg == "on" { "dark" } else { "light" }); crate::theme::set_dark(arg == "on"); crate::themes::touch(); window.refresh(); }
            "folder-select" => if let Some(b) = self.current.as_ref().and_then(|p| self.buffers.get(p)) { Self::select_text(&b.editor.clone(), arg, cx); },
            "folder-check" => self.check_buffers(None, cx),
            "folder-git" => { self.git_open = !self.git_open; cx.notify(); }
            "folder-menu" => { self.menu = !self.menu; cx.notify(); }
            "folder-meta" => if let Some(b) = self.current.as_ref().and_then(|p| self.buffers.get(p)) { b.editor.update(cx, |e, cx| e.toggle_source_meta(cx)); },
            _ => eprintln!("devtools: unknown folder step {cmd}"),
        }
        cx.notify();
    }
    pub fn toggle_overlay(&mut self, cx: &mut Context<Self>) { self.overlay = !self.overlay; cx.notify(); }
    pub fn overlay_open(&self) -> bool { self.overlay }
}
fn patch_row(row: Option<&git::PatchRow>) -> AnyElement {
    use crate::settings::tr;
    use git::{LineKind, PatchNotice, PatchRow};
    let note = |text: String| div().w_full().min_w_0().px(px(18.)).py(px(4.)).text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).child(text).into_any_element();
    let number = |n: Option<u32>| div().w(px(44.)).flex_none().pr(px(10.)).flex().justify_end().text_color(Theme::text_placeholder()).child(n.map(|n| n.to_string()).unwrap_or_default());
    match row {
        Some(PatchRow::Notice(n)) => note(match n {
            PatchNotice::NewFile => tr("新文件", "New file").into(),
            PatchNotice::Deleted => tr("已删除的文件", "Deleted file").into(),
            PatchNotice::Renamed(from) => crate::localized_format!("重命名自 {}", "Renamed from {}", from),
            PatchNotice::Binary => tr("二进制文件，不显示内容", "Binary file; contents not shown").into(),
            PatchNotice::LfsPointer => tr("Git LFS 指针：这里比较的是指针，不是文件内容", "Git LFS pointer: this compares the pointer, not the file's contents").into(),
            PatchNotice::ModeChanged => tr("文件权限已更改", "File mode changed").into(),
        }),
        Some(PatchRow::Hunk(context)) => div().w_full().min_w_0().mt(px(6.)).px(px(18.)).py(px(3.)).bg(Theme::fill_hover()).font_family(theme::MONO_FONT).text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption()).overflow_hidden().whitespace_nowrap().text_ellipsis().child(if context.is_empty() { "⋯".to_string() } else { format!("⋯ {context}") }).into_any_element(),
        Some(PatchRow::NoNewline) => note(tr("文件末尾没有换行符", "No newline at end of file").into()),
        Some(PatchRow::Line { kind, old, new, text }) => {
            let (bg, mark) = match kind { LineKind::Added => (Some(theme::ca(craft::old::G500, 0.14)), "+"), LineKind::Removed => (Some(theme::ca(craft::R400, 0.14)), "−"), LineKind::Context => (None, " ") };
            div().w_full().min_w_0().flex().pl(px(8.)).pr(px(18.)).font_family(theme::MONO_FONT).text_size(px(12.)).line_height(px(19.)).when_some(bg, |d, c| d.bg(c))
                .child(number(*old)).child(number(*new))
                .child(div().w(px(16.)).flex_none().text_color(Theme::text_caption()).child(mark))
                // Wrapped, however long: documents are often one line per paragraph.
                .child(div().flex_1().min_w_0().text_color(Theme::text()).child(text.clone()))
                .into_any_element()
        }
        None => div().into_any_element(),
    }
}
fn nav_button(id: &'static str, flip: bool, on: bool) -> Stateful<Div> {
    let glyph = icon("arrow-left", 16., if on { Theme::icon() } else { Theme::text_disabled() }).when(flip, |s| s.with_transformation(Transformation::rotate(radians(std::f32::consts::PI))));
    div().id(id).no_drag().flex_none().size(px(28.)).flex().items_center().justify_center().rounded(px(8.)).when(on, |d| d.cursor_pointer().hover(|s| s.bg(Theme::fill_hover_strong())).active(|s| s.bg(Theme::fill_pressed()))).child(glyph)
}
fn head_label(r: &Repository) -> String {
    let name = r.root.file_name().unwrap_or_default().to_string_lossy();
    let head = if r.detached { crate::localized_format!("游离 HEAD {}", "detached {}", r.branch) } else { r.branch.clone() };
    if r.worktree { crate::localized_format!("{} · {}（工作树）", "{} · {} (worktree)", name, head) } else { format!("{name} · {head}") }
}
fn group_label(label: &str) -> &'static str {
    use crate::settings::tr;
    match label { "冲突" => tr("冲突", "Conflicts"), "已暂存" => tr("已暂存", "Staged"), "未暂存" => tr("未暂存", "Unstaged"), _ => tr("未跟踪", "Untracked") }
}
fn code_color(code: char) -> Hsla {
    match code { 'U' | 'D' => theme::c(craft::R400), 'A' | '?' => theme::c(craft::old::G500), 'R' | 'C' | 'T' => Theme::primary(), _ => theme::c(craft::O400) }
}
fn git_badge(g: git::Decoration, directory: bool) -> AnyElement {
    let color = if g.conflicted { theme::c(craft::R400) } else if directory { Theme::text_caption() } else { code_color(g.code) };
    let text = if directory { if g.count > 999 { "999+".to_string() } else { g.count.to_string() } } else { g.code.to_string() };
    div().flex_none().ml(px(6.)).text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(color).child(text).into_any_element()
}
impl Workspace {
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(crate::settings::tr("打开文件夹", "Open Folder").into()), extensions: None }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match prompt { Some(p) => p.await.ok().and_then(Result::ok).flatten(), None => picked };
            if let Some(path) = paths.and_then(|p| p.into_iter().next()) {
                let result = cx.background_executor().spawn(async move { files::root(&path) }).await;
                let _ = this.update_in(cx, |w, window, cx| match result {
                    Ok(root) => w.enter_folder(root, None, window, cx),
                    Err(e) => w.show_toast_kind(ToastKind::Error, &e.to_string(), cx),
                });
            }
        }).detach();
    }
    pub fn enter_folder(&mut self, root: PathBuf, first: Option<(PathBuf, Option<String>, How)>, window: &mut Window, cx: &mut Context<Self>) {
        let (retry, again) = (root.clone(), first.clone());
        if !self.save_before(cx, move |w, window, cx| w.enter_folder(retry, again, window, cx)) { return; }
        let folder = cx.new(|cx| Folder::new(root, first, window, cx));
        self.folder_subs = vec![
            cx.observe(&folder, |_, _, cx| cx.notify()),
            cx.subscribe_in(&folder, window, |w, _, ev: &FolderEvent, _, cx| match ev {
                FolderEvent::Opened { path, anchor, how } => {
                    let Some(root) = w.folder.as_ref().map(|f| f.read(cx).root.clone()) else { return };
                    match how {
                        How::Record => w.navigation.push(crate::app::Nav::File(root, path.clone(), anchor.clone())),
                        How::Travel(forward) => { w.navigation.travel(*forward); }
                    }
                    cx.notify();
                }
                FolderEvent::Moved(from, to) => w.navigation.rewrite(|n| match n {
                    crate::app::Nav::File(root, path, anchor) if path == from => Some(crate::app::Nav::File(root.clone(), to.clone(), anchor.clone())),
                    _ => None,
                }),
                FolderEvent::Toast(kind, text) => w.show_toast_kind(*kind, text, cx),
            }),
        ];
        self.folder = Some(folder); self.view = View::Doc; self.search_open = false; self.doc_menu = false; self.doc_sub = None; self.mode_menu = false; self.row_menu = None; self.editor = None; self.current = None;
        cx.notify();
    }
    pub fn close_folder(&mut self, cx: &mut Context<Self>) {
        if !self.save_before(cx, |w, _, cx| w.close_folder(cx)) { return; }
        self.folder = None; self.folder_subs.clear(); self.view = View::Home;
        cx.notify();
    }
    pub fn navigate(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        use crate::app::Nav;
        let Some(target) = self.navigation.target(forward) else { return };
        match target {
            Nav::File(root, path, anchor) => {
                if let Some(folder) = self.folder.clone().filter(|f| f.read(cx).root == root) {
                    self.view = View::Doc;
                    folder.update(cx, |f, cx| f.open(path, anchor, How::Travel(forward), window, cx));
                } else { self.enter_folder(root, Some((path, anchor, How::Travel(forward))), window, cx); }
            }
            Nav::Note(view, id) => {
                if !self.save_before(cx, move |w, window, cx| w.navigate(forward, window, cx)) { return; }
                self.navigating = true;
                let success = if view == View::Doc { id.as_deref().is_some_and(|id| self.try_open(id, window, cx)) } else { self.show_view(view, cx); true };
                self.navigating = false;
                if success { self.navigation.travel(forward); }
            }
        }
        cx.notify();
    }
    pub fn folder_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(folder) = self.folder.clone() { folder.update(cx, |f, cx| f.set_mode(mode, window, cx)); }
    }
    /// ← → in the normal top bar: same glyphs and metrics as a folder document's.
    pub fn navigation_buttons(&self, cx: &mut Context<Self>) -> AnyElement {
        let (back, forward) = (self.navigation.target(false).is_some(), self.navigation.target(true).is_some());
        div().flex().flex_none().items_center().gap(px(2.))
            .child(nav_button("navigation-back", false, back).tip(crate::settings::tr("后退", "Back"), Some("⌘ + ⌥ + ←")).on_click(cx.listener(|w, _, window, cx| w.navigate(false, window, cx))))
            .child(nav_button("navigation-forward", true, forward).tip(crate::settings::tr("前进", "Forward"), Some("⌘ + ⌥ + →")).on_click(cx.listener(|w, _, window, cx| w.navigate(true, window, cx))))
            .into_any_element()
    }
}
