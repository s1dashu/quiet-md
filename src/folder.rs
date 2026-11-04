//! Native filesystem workspace. Source buffers own saves; previews are disposable.
use crate::{app::{Workspace, View}, editor::view::{Editor, EditorEvent}, input::{TextField, TextFieldEvent}, model::*, theme::Theme, ui::{icon, TitleBarExt}};
use fdl_core::{workspace_files::{self as files, Source}, workspace_git::{self as git, Snapshot}, workspace_navigation::{History, Swipe}, workspace_preview::{self, Preview}};
use gpui::{prelude::FluentBuilder, *};
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::{Arc, atomic::{AtomicU64, Ordering}}, time::Duration};

actions!(folder, [OpenFolder, CloseFolder, Back, Forward, SaveSource, SourceMode, PreviewMode, SplitMode]);
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-o", OpenFolder, None), KeyBinding::new("alt-left", Back, None), KeyBinding::new("alt-right", Forward, None),
        KeyBinding::new("cmd-s", SaveSource, Some("Folder")),
        KeyBinding::new("cmd-alt-1", SourceMode, Some("Folder")), KeyBinding::new("cmd-alt-2", PreviewMode, Some("Folder")), KeyBinding::new("cmd-alt-3", SplitMode, Some("Folder")),
    ]);
}
#[derive(Clone, Copy, PartialEq)]
enum Mode { Source, Preview, Split }
struct Buffer {
    source: Source, input: Entity<TextField>, preview: Option<Entity<Editor>>, projection: Option<Preview>,
    scroll: ScrollHandle, _input_sub: Subscription, preview_sub: Option<Subscription>, parsed: u64, request: u64,
    error: Option<String>, used: u64, diagrams: ImageFiles, pending_anchor: Option<String>, media: Vec<(PathBuf,Option<(u64,std::time::SystemTime)>)>,
}
#[derive(Default)]
struct ImageFiles(Vec<PathBuf>);
impl Drop for ImageFiles { fn drop(&mut self) { for path in &self.0 { let _ = std::fs::remove_file(path); } } }
pub struct Folder {
    pub root: PathBuf, pub sidebar_open: bool, current: Option<PathBuf>, buffers: HashMap<PathBuf, Buffer>, directories: HashMap<PathBuf, Vec<files::Entry>>,
    expanded: HashSet<PathBuf>, hidden: bool, git: Option<Snapshot>, git_error: Option<String>, history: History<(PathBuf, Option<String>)>,
    mode: Mode, error: Option<String>, diff: Option<(PathBuf, bool, String)>, diff_scroll: ScrollHandle, tree_scroll: ScrollHandle,
    focus: FocusHandle, refresh: Option<Task<()>>, parse_task: Option<Task<()>>, tick: u64, swipe: Swipe, theme: String, generation: Arc<AtomicU64>, reload_confirm: bool, recent_open: bool, recoveries: Vec<PathBuf>, traveling: Option<bool>, open_task: Option<Task<()>>, open_request: u64,
}
fn recovery_dir() -> PathBuf {
    std::env::var_os("QUIET_MD_HOME").map(PathBuf::from).map(|p| p.join("workspace-recovery")).unwrap_or_else(|| dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("quiet.md/workspace-recovery"))
}
fn preferences() -> PathBuf { recovery_dir().with_file_name("workspace-folders.json") }
fn recent() -> Vec<PathBuf> { std::fs::read(preferences()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default() }
fn remember(root: &Path) {
    let mut paths = recent(); paths.retain(|p| p != root); paths.insert(0, root.to_path_buf()); paths.truncate(12);
    if let Some(parent) = preferences().parent() { let _ = std::fs::create_dir_all(parent); }
    if let Ok(bytes) = serde_json::to_vec(&paths) { let _ = std::fs::write(preferences(), bytes); }
}
impl Folder {
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let directories = HashMap::new();
        remember(&root);
        let mut this = Self { root: root.clone(), sidebar_open: true, current: None, buffers: HashMap::new(), directories, expanded: HashSet::from([root]), hidden: false,
            git: None, git_error: None, history: History::default(), mode: Mode::Split, error: None, diff: None, diff_scroll: ScrollHandle::new(), tree_scroll: ScrollHandle::new(), focus: cx.focus_handle(), refresh: None, parse_task: None, tick: 0, swipe: Swipe::default(), theme: String::new(), generation: Arc::new(AtomicU64::new(0)), reload_confirm: false, recent_open: false, recoveries: vec![], traveling: None, open_task: None, open_request: 0 };
        this.start_refresh(cx);
        let root=this.root.clone();
        cx.spawn_in(window,async move |this,cx| {
            let result=cx.background_executor().spawn(async move { (files::list(&root,&root,false),recovery_paths(&root),root) }).await;
            let _=this.update_in(cx,|f,window,cx| {
                match result.0 { Ok(entries) => {
                    let first=entries.iter().find(|e|e.path.file_name().is_some_and(|s|s.eq_ignore_ascii_case("readme.md"))).or_else(||entries.iter().find(|e|!e.directory)).map(|e|e.path.clone());
                    f.directories.insert(result.2,entries); f.recoveries=result.1;
                    if f.current.is_none() && let Some(path)=first { f.open(path,None,true,window,cx); }
                }, Err(e)=>f.error=Some(e.to_string()) }
                cx.notify();
            });
        }).detach();
        this
    }
    pub fn flush(&mut self, cx: &mut Context<Self>) -> bool {
        let mut success = true;
        for buffer in self.buffers.values_mut() {
            if !buffer.source.dirty() { buffer.source.clear_recovery(&recovery_dir()); continue; }
            let recovery = buffer.source.recover(&recovery_dir());
            match buffer.source.save(&self.root) {
                Ok(()) => { buffer.error = None; buffer.source.clear_recovery(&recovery_dir()); },
                Err(e) => { buffer.error = Some(format!("{e}{}", recovery.err().map(|e| format!("; recovery failed: {e}")).unwrap_or_default())); success = false; }
            }
        }
        self.recoveries.retain(|path| self.buffers.get(path).is_none_or(|b|b.source.dirty()));
        cx.notify(); success
    }
    pub fn discard(&mut self, cx: &mut Context<Self>) {
        for buffer in self.buffers.values_mut() { buffer.source.clear_recovery(&recovery_dir()); }
        self.buffers.clear(); self.current = None; self.error = None; cx.notify();
    }
    fn start_refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh = Some(cx.spawn(async |this, cx| {
            loop {
                let Ok((root, expanded, hidden, sources, media)) = this.update(cx, |f, _| (f.root.clone(), f.expanded.clone(), f.hidden, f.buffers.iter().map(|(p,b)| (p.clone(), b.source.text.clone(), b.source.baseline.clone(), b.source.revision)).collect::<Vec<_>>(), f.current.as_ref().and_then(|p|f.buffers.get(p)).map(|b|b.media.clone()).unwrap_or_default())) else { break; };
                let result = cx.background_executor().spawn(async move {
                    let directories = expanded.into_iter().map(|p| { let entries = files::list(&root, &p, hidden).map_err(|e| e.to_string()); (p, entries) }).collect::<Vec<_>>();
                    let status = git::snapshot(&root).map_err(|e| e.to_string());
                    let changes = sources.into_iter().map(|(p,text,baseline,rev)| { let result = files::read(&p).map_err(|e| e.to_string()); (p,rev,text,baseline,result) }).collect::<Vec<_>>();
                    let media_changed=media.iter().any(|(path,previous)| media_stamp(path)!=*previous);
                    (directories, status, changes, media_changed)
                }).await;
                if this.update(cx, |f, cx| {
                    for (p, entries) in result.0 { if let Ok(entries) = entries && f.expanded.contains(&p) { f.directories.insert(p, entries); } }
                    match result.1 { Ok(status) => { f.git = status; f.git_error = None; }, Err(e) => { f.git = None; f.git_error = Some(e); } }
                    for (path, rev, text, baseline, result) in result.2 {
                        let Some(b) = f.buffers.get_mut(&path).filter(|b| b.source.revision == rev) else { continue; };
                        match result {
                            Ok(disk) if disk == text => { b.source.baseline = disk; },
                            Ok(disk) if disk == baseline => {},
                            Ok(_) if text != baseline => b.error = Some("File changed on disk. Your edits are retained; save a copy or reload explicitly.".into()),
                            Ok(disk) => { b.source.edit(disk.clone()); b.source.baseline = disk; b.input.update(cx, |field,cx| field.set_text(&b.source.text,cx)); b.error = None; },
                            Err(e) => { b.error = Some(e); b.preview = None; b.projection = None; },
                        }
                    }
                    if result.3 && let Some(b)=f.current.as_ref().and_then(|p|f.buffers.get_mut(p)) { b.parsed=u64::MAX; }
                    cx.notify();
                }).is_err() { break; }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        }));
    }
    fn open(&mut self, path: PathBuf, anchor: Option<String>, record: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open_request += 1; self.open_task=None;
        if record { self.traveling=None; }
        if self.buffers.contains_key(&path) { self.finish_open(path,anchor,record,window,cx); return; }
        if self.buffers.values().filter(|b|b.source.dirty()).count() >= 24 || self.buffers.values().filter(|b|b.source.dirty()).map(|b|b.source.text.len()+b.source.baseline.len()).sum::<usize>() > 32*1024*1024 { self.error=Some("Save unsaved files before opening another file".into()); cx.notify(); return; }
        let request = self.open_request; let root = self.root.clone();
        self.open_task = Some(cx.spawn_in(window,async move |this,cx| {
            let result = cx.background_executor().spawn(async move {
                anyhow::ensure!(files::supported(&path),"This file type has no source editor");
                if let Some(source)=Source::recovery(&root,&path,&recovery_dir())? { return Ok(source); }
                let path=files::contained(&root,&path)?; Source::open(&root,&path)
            }).await;
            let _=this.update_in(cx,|f,window,cx| {
                if f.open_request != request { return; } f.open_task=None;
                match result {
                    Ok(source) => { let path=source.path.clone(); f.add_buffer(source,window,cx); f.finish_open(path,anchor,record,window,cx); },
                    Err(e) => { f.traveling=None; f.error=Some(e.to_string()); cx.notify(); }
                }
            });
        }));
    }
    fn add_buffer(&mut self,source: Source,window: &mut Window,cx: &mut Context<Self>) {
        let path=source.path.clone();
        if self.buffers.contains_key(&path) { return; }
        let input = cx.new(|cx| { let mut f = TextField::new("", cx); f.multiline = true; f.source = true; f.mono = true; f.wrap = true; f.size = 13.; f.line_height = 21.; f.set_text(&source.text, cx); f });
        let p=path.clone();
        let sub = cx.subscribe_in(&input, window, move |f, field, ev: &TextFieldEvent, window, cx| {
            let Some(b) = f.buffers.get_mut(&p) else { return; };
            match ev {
                TextFieldEvent::Changed => b.source.edit(field.read(cx).text.clone()),
                TextFieldEvent::Undo | TextFieldEvent::Redo => { if b.source.undo(*ev == TextFieldEvent::Redo) { field.update(cx, |field,cx| field.set_text(&b.source.text,cx)); } },
                _ => return,
            }
            if let Err(e) = b.source.recover(&recovery_dir()) { b.error = Some(format!("Recovery failed: {e}")); }
            f.schedule_preview(window, cx); cx.notify();
        });
        self.buffers.insert(path,Buffer { source, input, preview: None, projection: None, scroll: ScrollHandle::new(), _input_sub: sub, preview_sub: None, parsed: u64::MAX, request: 0, error: None, used: 0, diagrams: ImageFiles::default(), pending_anchor: None, media: vec![] });
    }
    fn finish_open(&mut self,path: PathBuf,anchor: Option<String>,record: bool,window: &mut Window,cx: &mut Context<Self>) {
        self.buffers.get_mut(&path).unwrap().pending_anchor=anchor.clone(); self.reload_confirm=false;
        self.tick += 1; self.buffers.get_mut(&path).unwrap().used = self.tick;
        self.current = Some(path.clone()); self.error = None; self.diff = None;
        if record { self.history.push((path.clone(), anchor.clone())); } else if let Some(forward)=self.traveling.take() { self.history.travel(forward); }
        // Dirty buffers are never evicted. Refuse additional opens when their aggregate becomes large.
        while self.buffers.len() > 24 || self.buffers.values().map(|b|b.source.text.len()+b.source.baseline.len()).sum::<usize>() > 32*1024*1024 {
            let oldest = self.buffers.iter().filter(|(p,b)| **p != path && !b.source.dirty()).min_by_key(|(_,b)| b.used).map(|(p,_)| p.clone());
            if let Some(oldest) = oldest { self.buffers.remove(&oldest); } else { self.error=Some("Many unsaved buffers are retained. Save files before opening more.".into()); break; }
        }
        if let Some(anchor) = anchor && let Some(b) = self.buffers.get(&path) && let Some(id) = b.projection.as_ref().and_then(|p| p.anchors.get(&anchor)).copied() && let Some(editor) = &b.preview { editor.update(cx, |e,cx| e.scroll_to_block(id,cx)); }
        self.focus_mode(window,cx);
        self.schedule_preview(window,cx); cx.notify();
    }
    pub fn travel(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((path, anchor)) = self.history.target(forward) { self.traveling=Some(forward); self.open(path,anchor,false,window,cx); }
    }
    fn focus_mode(&self,window: &mut Window,cx: &App) {
        if let Some(b)=self.current.as_ref().and_then(|p|self.buffers.get(p)) {
            if self.mode==Mode::Preview { if let Some(editor)=&b.preview { editor.read(cx).focus(window); } else { window.focus(&self.focus); } }
            else { window.focus(&b.input.read(cx).focus_handle(cx)); }
        } else { window.focus(&self.focus); }
    }
    fn change_mode(&mut self,mode: Mode,window: &mut Window,cx: &mut Context<Self>) {
        self.mode=mode; self.diff=None; self.focus_mode(window,cx); self.schedule_preview(window,cx); cx.notify();
    }
    fn schedule_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode==Mode::Source { self.generation.fetch_add(1,Ordering::Relaxed); self.parse_task=None; return; }
        let Some(path) = self.current.clone() else { return; };
        let b = self.buffers.get_mut(&path).unwrap();
        let revision = b.source.revision; b.request += 1; let request = b.request;
        let source = b.source.text.clone(); let root = self.root.clone(); let palette = crate::pele::Palette::current();
        let available=(f32::from(window.viewport_size().width)-if self.sidebar_open {260.} else {0.}).max(200.); let width=if self.mode==Mode::Split {available/2.-40.} else {available-80.};
        let generation = self.generation.clone(); let job = generation.fetch_add(1,Ordering::Relaxed)+1; let diagrams = self.mode != Mode::Source;
        self.parse_task = Some(cx.spawn_in(window, async move |this,cx| {
            cx.background_executor().timer(Duration::from_millis(180)).await;
            let result = cx.background_executor().spawn(async move {
                let mut projection = workspace_preview::parse(&source, &path, &root);
                let mut media=vec![]; collect_media(&projection.doc.blocks,&mut media);
                let mut images = ImageFiles(crate::pele::prepare_images(&mut projection.doc));
                for (id, diagram) in projection.diagrams.iter().take(if diagrams {8} else {0}) {
                    if generation.load(Ordering::Relaxed) != job { break; }
                    match crate::pele::render(diagram, width, &palette) {
                        Ok(bytes) => {
                            let temp = std::env::temp_dir().join(format!("quiet-diagram-{}.png", uuid::Uuid::new_v4()));
                            if std::fs::write(&temp, bytes).is_ok() {
                                fn replace(blocks: &mut [Block], id: BlockId, path: &Path) { for b in blocks { if b.id == id { b.kind = BlockKind::Image { src: path.to_string_lossy().into(), src_kind: "src".into(), width: None, height: None, caption: None, crop: None }; b.text = RichText::default(); } replace(&mut b.children, id,path); } }
                                replace(&mut projection.doc.blocks,*id,&temp); images.0.push(temp);
                            }
                        },
                        Err(e) => projection.diagnostic = Some(format!("Diagram preview: {e}. Source is available.")),
                    }
                }
                (path, projection, images, media)
            }).await;
            let _ = this.update_in(cx, |f, window,cx| {
                let (path, projection, images, media) = result;
                let Some(buffer) = f.buffers.get(&path).filter(|b| b.source.revision == revision && b.request == request) else { return; };
                f.parse_task = None;
                let offset = buffer.preview.as_ref().map(|e| e.read(cx).list.logical_scroll_top());
                let preview_focused=buffer.preview.as_ref().is_some_and(|e|e.read(cx).focus.is_focused(window));
                let editor = cx.new(|cx| { let mut e = Editor::new(projection.doc.clone(),0,Default::default(),window,cx); e.source_preview = true; e.set_read_only(true,cx); if let Some(offset) = offset { e.list.scroll_to(offset); } e });
                if f.current.as_ref()==Some(&path) && (f.mode==Mode::Preview || preview_focused) { editor.read(cx).focus(window); }
                let p = path.clone();
                let sub = cx.subscribe_in(&editor,window,move |f, editor,ev: &EditorEvent,window,cx| {
                    match ev {
                        EditorEvent::Changed => { editor.update(cx, |e,cx| e.reject_read_only_edit(cx)); },
                        EditorEvent::SourceTask(id) => {
                            if let Some(b) = f.buffers.get_mut(&p) && b.parsed == b.source.revision && let Some(text) = b.projection.as_ref().and_then(|pr| pr.toggle(&b.source.text,*id)) {
                                b.source.edit(text); b.input.update(cx,|field,cx| field.set_text(&b.source.text,cx));
                                if let Err(e) = b.source.recover(&recovery_dir()) { b.error = Some(e.to_string()); }
                                f.schedule_preview(window,cx);
                            }
                        },
                        EditorEvent::SourceLink(url) if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:") => cx.open_url(url),
                        EditorEvent::SourceLink(url) => match files::relative_link(&f.root,&p,url) { Ok((path,anchor)) => f.open(path,anchor,true,window,cx), Err(e) => { f.error = Some(e.to_string()); cx.notify(); } },
                        EditorEvent::Save => { f.flush(cx); }, _ => {},
                    }
                });
                let b = f.buffers.get_mut(&path).unwrap();
                b.diagrams = images; b.media=media; b.preview = Some(editor); b.preview_sub = Some(sub); b.projection = Some(projection); b.parsed = revision;
                if let Some(anchor) = b.pending_anchor.take() && let Some(id) = b.projection.as_ref().and_then(|p| p.anchors.get(&anchor)).copied() && let Some(editor) = &b.preview { editor.update(cx, |e,cx| e.scroll_to_block(id,cx)); }
                cx.notify();
            });
        }));
    }
    fn toggle(&mut self,path: PathBuf,cx: &mut Context<Self>) {
        if !self.expanded.remove(&path) {
            if self.expanded.len() >= 128 { self.error=Some("Collapse a folder before expanding another; 128 folders are open".into()); cx.notify(); return; }
            self.expanded.insert(path.clone());
            let root=self.root.clone(); let hidden=self.hidden;
            cx.spawn(async move |this,cx| {
                let p=path.clone(); let entries=cx.background_executor().spawn(async move { files::list(&root,&p,hidden).map_err(|e|e.to_string()) }).await;
                let _=this.update(cx,|f,cx| { match entries { Ok(entries) if f.expanded.contains(&path) => { f.directories.insert(path,entries); }, Err(e)=>f.error=Some(e), _=>{} } cx.notify(); });
            }).detach();
        }
        cx.notify();
    }
    fn rows(&self,path: &Path,depth: usize,out: &mut Vec<(files::Entry,usize)>) {
        if depth > 32 || out.len() >= 4000 { return; }
        if let Some(entries) = self.directories.get(path) { for entry in entries { if out.len() >= 4000 { break; } out.push((entry.clone(),depth)); if self.expanded.contains(&entry.path) { self.rows(&entry.path,depth+1,out); } } }
    }
    fn show_diff(&mut self,path: PathBuf,staged: bool,window: &mut Window,cx: &mut Context<Self>) {
        let Some(root) = self.git.as_ref().map(|g| g.root.clone()) else { return; };
        self.diff = Some((path.clone(),staged,"Loading diff...".into()));
        cx.spawn_in(window,async move |this,cx| {
            let p = path.clone(); let text = cx.background_executor().spawn(async move { git::diff(&root,&p,staged).map(|s| if s.is_empty() { "No tracked diff. Untracked files are shown in the source editor.".into() } else { s }).unwrap_or_else(|e| e.to_string()) }).await;
            let _ = this.update_in(cx,|f,_,cx| { if f.diff.as_ref().is_some_and(|(p,s,_)| p == &path && *s == staged) { f.diff = Some((path,staged,text)); cx.notify(); } });
        }).detach(); cx.notify();
    }
}
fn button(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Stateful<Div> {
    let text=text.into(); let text=folder_label(&text);
    div().id(id).no_drag().px(px(9.)).h(px(28.)).flex_none().flex().items_center().justify_center().rounded(px(6.)).cursor_pointer().hover(|s| s.bg(Theme::fill_hover_strong())).text_size(px(12.)).child(text)
}
impl Render for Folder {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = format!("{}:{:?}:{:?}:{}",crate::themes::generation(),Theme::bg_body(),Theme::primary(),(f32::from(window.viewport_size().width)/40.) as u32);
        let needs_parse = self.current.as_ref().and_then(|p| self.buffers.get(p)).is_some_and(|b| b.parsed != b.source.revision);
        if self.theme != theme { self.theme = theme; self.schedule_preview(window,cx); }
        else if needs_parse && self.parse_task.is_none() { self.schedule_preview(window,cx); }
        let mut rows = vec![]; self.rows(&self.root,0,&mut rows);
        let tree = rows.into_iter().map(|(entry,depth)| {
            let path = entry.path; let directory = entry.directory; let selected = self.current.as_ref() == Some(&path); let expanded = self.expanded.contains(&path);
            let status = self.git.as_ref().map(|g| g.decoration(&path,directory)).unwrap_or_default();
            let dirty = self.buffers.get(&path).is_some_and(|b| b.source.dirty());
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            div().id(SharedString::from(format!("folder-entry-{}",path.display()))).h(px(29.)).pl(px(9. + depth as f32*14.)).pr(px(8.)).flex().items_center().gap(px(7.)).rounded(px(6.)).cursor_pointer()
                .when(selected,|d| d.bg(Theme::sidebar_hover())).hover(|s| s.bg(Theme::sidebar_hover()))
                .child(icon(if directory { if expanded { "chevron-down" } else { "chevron-right" } } else { "file" },14.,Theme::icon()))
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
                .when(dirty,|d| d.child(div().text_color(Theme::primary()).child("*")))
                .when(!status.is_empty(),|d| d.child(div().text_size(px(11.)).font_family("Menlo").text_color(Theme::primary()).child(status)))
                .on_click(cx.listener(move |f,_,window,cx| if directory { f.toggle(path.clone(),cx); } else { f.open(path.clone(),None,true,window,cx); }))
        }).collect::<Vec<_>>();
        let branch = self.git.as_ref().map(|g| crate::localized_format!("{}（{} 个更改）","{} ({} changed)",g.branch,g.files.len())).unwrap_or_else(|| crate::settings::tr("无 Git 仓库","No Git repository").into());
        let mut changes = vec![];
        if let Some(git) = &self.git {
            for (index,status) in git.files.iter().take(300).enumerate() {
                let path = git.root.join(&status.path); let p = path.clone(); let staged = status.index != ' ' && status.index != '?'; let unstaged = status.worktree != ' ';
                changes.push(div().flex().items_center().gap(px(3.)).child(button(SharedString::from(format!("git-path-{index}")),status.path.to_string_lossy().into_owned()).flex_1().min_w_0().overflow_hidden().on_click(cx.listener(move |f,_,window,cx| f.open(p.clone(),None,true,window,cx))))
                    .child(button(SharedString::from(format!("git-index-{index}")),format!("I:{}",status.index)).when(staged,|d| { let p=path.clone(); d.on_click(cx.listener(move |f,_,window,cx| f.show_diff(p.clone(),true,window,cx))) }))
                    .child(button(SharedString::from(format!("git-worktree-{index}")),format!("W:{}",status.worktree)).when(unstaged,|d| { let p=path.clone(); d.on_click(cx.listener(move |f,_,window,cx| f.show_diff(p.clone(),false,window,cx))) })));
            }
        }
        let retained = self.buffers.iter().filter(|(p,b)| b.source.dirty() && !p.exists()).map(|(p,_)| p.clone()).collect::<Vec<_>>();
        let retained=retained.into_iter().chain(self.recoveries.clone()).collect::<HashSet<_>>();
        let retained_rows = retained.into_iter().map(|p| { let name = crate::localized_format!("保留的修改：{}","Retained: {}",p.file_name().unwrap_or_default().to_string_lossy()); button(SharedString::from(format!("retained-{}",p.display())),name).on_click(cx.listener(move |f,_,window,cx| { f.open(p.clone(),None,true,window,cx); })) }).collect::<Vec<_>>();
        let sidebar = div().w(px(260.)).min_w(px(170.)).h_full().flex_none().flex().flex_col().bg(Theme::window()).text_size(px(12.))
            .child(div().h(px(48.)).flex_none().pl(px(crate::theme::TRAFFIC_LIGHTS_W)).title_bar())
            .child(div().px(px(10.)).pb(px(7.)).flex().items_center().gap(px(4.)).child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().font_weight(FontWeight::SEMIBOLD).child(self.root.file_name().unwrap_or_default().to_string_lossy().into_owned()))
                .child(button("folder-hidden",if self.hidden { "Hide dotfiles" } else { "Dotfiles" }).on_click(cx.listener(|f,_,_,cx| { f.hidden = !f.hidden; f.start_refresh(cx); cx.notify(); }))))
            .child(div().id("folder-tree").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.tree_scroll).px(px(8.)).children(tree).children(retained_rows).when(rows_limit(&self.directories),|d| d.child(crate::settings::tr("目录过大：打开较小的文件夹以查看其余文件","Large tree: expand a smaller folder to see more files")))
                .child(div().pt(px(14.)).pb(px(5.)).text_color(Theme::text_caption()).child(branch)).children(changes)
                .when(self.git.as_ref().is_some_and(|g|g.files.len()>300),|d| d.child(crate::localized_format!("仅显示前 300 个更改，共 {} 个", "Showing first 300 of {} changes",self.git.as_ref().map_or(0,|g|g.files.len()))))
                .when_some(self.git_error.clone(),|d,e| d.child(e)))
            .child(div().p(px(8.)).flex().child(button("folder-notebook","Notebook").on_click(|_,window,cx| window.dispatch_action(Box::new(CloseFolder),cx))).child(button("folder-recent","Recent").on_click(cx.listener(|f,_,_,cx| { f.recent_open = !f.recent_open; cx.notify(); }))).child(button("folder-open","Open Folder...").on_click(|_,window,cx| window.dispatch_action(Box::new(OpenFolder),cx))));
        let sidebar = sidebar.when(self.recent_open,|d| d.children(recent().into_iter().enumerate().map(|(i,path)| { let name = path.to_string_lossy().into_owned(); button(SharedString::from(format!("recent-folder-{i}")),name).on_click(move |_,window,cx| { let p=path.clone(); window.defer(cx, move |window,cx| { if let Some(handle)=window.window_handle().downcast::<Workspace>() { let _=handle.update(cx,|w,window,cx| w.enter_folder(p,window,cx)); } }); }) })));
        let path = self.current.clone();
        let name = path.as_ref().map(|p| p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy().into_owned()).unwrap_or_else(|| crate::settings::tr("选择 Markdown 或 MDX 文件","Choose a Markdown or MDX file").into());
        let b = path.as_ref().and_then(|p| self.buffers.get(p));
        let dirty = b.is_some_and(|b| b.source.dirty());
        let error = self.error.clone().or_else(|| b.and_then(|b| b.error.clone()));
        let diagnostic = b.and_then(|b| b.projection.as_ref()).and_then(|p| p.diagnostic.clone());
        let source = b.map(|b| div().id("folder-source-scroll").flex_1().min_w_0().h_full().overflow_y_scroll().track_scroll(&b.scroll).p(px(20.)).child(b.input.clone())
            .on_scroll_wheel(|ev,_,cx| if let ScrollDelta::Pixels(delta) = ev.delta && delta.x.abs() > delta.y.abs() { cx.stop_propagation(); }));
        let preview = b.and_then(|b| b.preview.clone());
        let mode = self.mode;
        let main = div().flex_1().min_w_0().h_full().flex().flex_col().bg(Theme::bg_body())
            .child(div().h(px(48.)).flex_none().px(px(8.)).flex().items_center().gap(px(3.)).title_bar()
                .when(!self.sidebar_open,|d| d.pl(px(crate::theme::TRAFFIC_LIGHTS_W)))
                .child(button("folder-toggle-sidebar","Sidebar").on_click(cx.listener(|f,_,_,cx| { f.sidebar_open = !f.sidebar_open; cx.notify(); })))
                .child(button("folder-back","<").opacity(if self.history.target(false).is_some() {1.} else {0.3}).on_click(cx.listener(|f,_,window,cx| f.travel(false,window,cx))))
                .child(button("folder-forward",">").opacity(if self.history.target(true).is_some() {1.} else {0.3}).on_click(cx.listener(|f,_,window,cx| f.travel(true,window,cx))))
                .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_size(px(12.)).child(format!("{name}{}",if dirty { crate::settings::tr(" * 未保存"," * Unsaved") } else { "" })))
                .children([(Mode::Source,"folder-source","Source"),(Mode::Preview,"folder-preview","Preview"),(Mode::Split,"folder-split","Split")].into_iter().map(|(mode,id,label)| button(id,label).when(self.mode == mode,|d| d.bg(Theme::fill_hover_strong())).on_click(cx.listener(move |f,_,window,cx| f.change_mode(mode,window,cx)))))
                .child(button("folder-save","Save").on_click(cx.listener(|f,_,_,cx| { f.flush(cx); }))))
            .when_some(error,|d,e| d.child(div().flex_none().px(px(18.)).py(px(7.)).text_size(px(12.)).text_color(Theme::primary()).child(folder_diagnostic(&e))
                .child(div().flex().gap(px(8.)).child(button("folder-copy-save","Save Copy...").on_click(cx.listener(|f,_,window,cx| f.save_copy(window,cx)))).child(button("folder-reload",if self.reload_confirm { "Confirm Reload (discard edits)" } else { "Reload from Disk" }).on_click(cx.listener(|f,_,window,cx| f.reload(window,cx)))))))
            .when_some(diagnostic,|d,text| d.child(div().flex_none().px(px(18.)).py(px(5.)).text_size(px(11.)).text_color(Theme::text_caption()).child(text)))
            .child(if let Some((path,staged,text)) = &self.diff {
                div().flex_1().min_h_0().flex().flex_col().child(button("diff-close",crate::localized_format!("{}差异：{} - 关闭", "{} diff: {} - Close",if *staged {crate::settings::tr("暂存区","Index")} else {crate::settings::tr("工作区","Working tree")},path.display())).on_click(cx.listener(|f,_,_,cx| { f.diff = None; cx.notify(); })))
                    .child(div().id("folder-diff").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.diff_scroll).p(px(16.)).font_family("Menlo").text_size(px(12.)).child(text.clone())).into_any_element()
            } else {
                div().flex_1().min_h_0().flex().when(mode != Mode::Preview,|d| d.children(source)).when(mode == Mode::Split,|d| d.child(div().w(px(1.)).h_full().bg(Theme::line_divider())))
                    .when(mode != Mode::Source,|d| d.child(div().flex_1().min_w_0().h_full().children(preview))).into_any_element()
            });
        div().id("folder-workspace").size_full().flex().key_context("Folder").track_focus(&self.focus)
            .on_action(cx.listener(|f,_: &crate::app::ToggleSidebar,_,cx| { f.sidebar_open = !f.sidebar_open; cx.stop_propagation(); cx.notify(); }))
            .on_action(cx.listener(|f,_: &SaveSource,_,cx| { f.flush(cx); }))
            .on_action(cx.listener(|f,_: &SourceMode,window,cx| f.change_mode(Mode::Source,window,cx)))
            .on_action(cx.listener(|f,_: &PreviewMode,window,cx| f.change_mode(Mode::Preview,window,cx)))
            .on_action(cx.listener(|f,_: &SplitMode,window,cx| f.change_mode(Mode::Split,window,cx)))
            .on_action(cx.listener(|f,_: &Back,window,cx| { f.travel(false,window,cx); cx.stop_propagation(); }))
            .on_action(cx.listener(|f,_: &Forward,window,cx| { f.travel(true,window,cx); cx.stop_propagation(); }))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back),cx.listener(|f,_,window,cx| { f.travel(false,window,cx); cx.stop_propagation(); }))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward),cx.listener(|f,_,window,cx| { f.travel(true,window,cx); cx.stop_propagation(); }))
            .on_scroll_wheel(cx.listener(|f,ev: &ScrollWheelEvent,window,cx| {
                if ev.modifiers != Modifiers::default() { return; }
                if matches!(ev.touch_phase, TouchPhase::Started) { f.swipe.start(); }
                if matches!(ev.touch_phase, TouchPhase::Ended) { f.swipe.end(); }
                if let ScrollDelta::Pixels(delta) = ev.delta && let Some(forward) = f.swipe.update(delta.x.into(),delta.y.into()) { f.travel(forward,window,cx); cx.stop_propagation(); }
            })).when(self.sidebar_open,|d|d.child(sidebar)).child(main)
    }
}
impl Folder {
    fn save_copy(&mut self,window: &mut Window,cx: &mut Context<Self>) {
        let Some(source) = self.current.as_ref().and_then(|p| self.buffers.get(p)).map(|b| b.source.clone()) else { return; };
        let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
        let prompt = picked.is_none().then(|| cx.prompt_for_new_path(&self.root,source.path.file_name().and_then(|s| s.to_str())));
        cx.spawn_in(window,async move |this,cx| {
            let path = match prompt { Some(p) => p.await.ok().and_then(Result::ok).flatten(), None => picked };
            if let Some(path) = path { let result = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).and_then(|mut f| { use std::io::Write; f.write_all(source.text.as_bytes()) });
                let _ = this.update_in(cx,|f,_,cx| { f.error = Some(match result { Ok(()) => format!("Copy saved: {}. Original edits retained.",path.display()), Err(e) => format!("Could not save copy: {e}") }); cx.notify(); });
            }
        }).detach();
    }
}
impl Workspace {
    pub fn open_folder(&mut self,window: &mut Window,cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files:false,directories:true,multiple:false,prompt:Some(crate::settings::tr("打开文件夹","Open Folder").into()),extensions:None }));
        cx.spawn_in(window,async move |this,cx| {
            let paths = match prompt { Some(p) => p.await.ok().and_then(Result::ok).flatten(), None => picked };
            if let Some(path) = paths.and_then(|p| p.into_iter().next()) {
                let result = cx.background_executor().spawn(async move { files::root(&path) }).await;
                let _ = this.update_in(cx,|w,window,cx| match result {
                    Ok(root) => w.enter_folder(root,window,cx), Err(e) => w.show_toast_kind(crate::ui::ToastKind::Error,&e.to_string(),cx),
                });
            }
        }).detach();
    }
    fn enter_folder(&mut self,root: PathBuf,window: &mut Window,cx: &mut Context<Self>) {
        let retry = root.clone();
        if !self.save_before(cx,move |w,window,cx| w.enter_folder(retry,window,cx)) { return; }
        self.folder = Some(cx.new(|cx| Folder::new(root,window,cx))); self.view = View::Doc; self.search_open = false; self.doc_menu=false; self.doc_sub=None; self.mode_menu=false; self.row_menu=None; cx.notify();
    }
    pub fn close_folder(&mut self,cx: &mut Context<Self>) {
        if !self.save_before(cx,|w,_,cx| w.close_folder(cx)) { return; }
        self.folder = None; cx.notify();
    }
    pub fn navigate(&mut self,forward: bool,window: &mut Window,cx: &mut Context<Self>) {
        if let Some(folder) = self.folder.clone() { folder.update(cx,|f,cx| f.travel(forward,window,cx)); return; }
        let Some((view,id)) = self.navigation.target(forward) else { return; };
        if !self.save_before(cx,move |w,window,cx|w.navigate(forward,window,cx)) { return; }
        self.navigating = true;
        let success = if view == View::Doc { id.as_deref().is_some_and(|id| self.try_open(id,window,cx)) } else { self.show_view(view,cx); true };
        self.navigating = false;
        if success { self.navigation.travel(forward); }
        cx.notify();
    }
    pub fn navigation_buttons(&self,cx: &mut Context<Self>) -> AnyElement {
        div().flex().items_center().pl(px(8.)).child(button("navigation-back","<").opacity(if self.navigation.target(false).is_some() {1.} else {0.3}).on_click(cx.listener(|w,_,window,cx| w.navigate(false,window,cx))))
            .child(button("navigation-forward",">").opacity(if self.navigation.target(true).is_some() {1.} else {0.3}).on_click(cx.listener(|w,_,window,cx| w.navigate(true,window,cx)))).into_any_element()
    }
}
fn rows_limit(directories: &HashMap<PathBuf,Vec<files::Entry>>) -> bool { directories.values().map(Vec::len).sum::<usize>() >= 4000 }
impl Folder {
    pub fn save_error(&self) -> String { self.buffers.values().find_map(|b|b.error.clone()).unwrap_or_else(||"Could not save source".into()) }
    pub fn test_state(&self) -> String {
        let b = self.current.as_ref().and_then(|p| self.buffers.get(p));
        format!("path={} mode={} dirty={} revision={} parsed={} branch={} buffers={} error={} diagrams={} diff={:?}",self.current.as_ref().map(|p|p.strip_prefix(&self.root).unwrap_or(p).display().to_string()).unwrap_or_default(),match self.mode { Mode::Source=>"source",Mode::Preview=>"preview",Mode::Split=>"split" },b.is_some_and(|b|b.source.dirty()),b.map_or(0,|b|b.source.revision),b.map_or(u64::MAX,|b|b.parsed),self.git.as_ref().map_or("",|g|g.branch.as_str()),self.buffers.len(),self.error.as_deref().or_else(||b.and_then(|b|b.error.as_deref())).unwrap_or(""),b.map_or(0,|b|b.diagrams.0.iter().filter(|p|p.file_name().is_some_and(|s|s.to_string_lossy().starts_with("quiet-diagram-"))).count()),self.diff.as_ref().map(|(p,s,t)|(p.display().to_string(),if *s {"index"} else {"worktree"},t.chars().take(1200).collect::<String>())))
    }
    fn reload(&mut self,window: &mut Window,cx: &mut Context<Self>) {
        let Some(path)=self.current.clone() else { return; };
        if self.buffers.get(&path).is_some_and(|b|b.source.dirty()) && !self.reload_confirm { self.reload_confirm=true; cx.notify(); return; }
        match Source::open(&self.root,&path) {
            Ok(source) => { if let Some(b)=self.buffers.get_mut(&path) { b.source.clear_recovery(&recovery_dir()); b.source=source; b.parsed=u64::MAX; b.input.update(cx,|field,cx|field.set_text(&b.source.text,cx)); b.error=None; } self.reload_confirm=false; self.schedule_preview(window,cx); },
            Err(e) => self.error=Some(e.to_string()),
        }
        cx.notify();
    }
}
fn media_stamp(path: &Path) -> Option<(u64,std::time::SystemTime)> { std::fs::metadata(path).ok().and_then(|m|Some((m.len(),m.modified().ok()?))) }
fn collect_media(blocks: &[Block],out: &mut Vec<(PathBuf,Option<(u64,std::time::SystemTime)>)>) {
    for block in blocks { if let BlockKind::Image {src,..}=&block.kind { let path=PathBuf::from(src); out.push((path.clone(),media_stamp(&path))); } else if let BlockKind::Table(table)=&block.kind { for row in &table.rows { for cell in row { collect_media(&cell.blocks,out); } } } collect_media(&block.children,out); }
}
fn recovery_paths(root: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(recovery_dir()).ok().into_iter().flatten().take(200).filter_map(Result::ok).filter_map(|entry| {
        if entry.path().extension().is_none_or(|s|s!="json") || entry.metadata().ok()?.len()>64*1024*1024 { return None; }
        let source: Source=serde_json::from_slice(&std::fs::read(entry.path()).ok()?).ok()?;
        (source.path.starts_with(root) && source.dirty()).then_some(source.path)
    }).collect()
}
fn folder_label(text: &str) -> SharedString {
    use crate::settings::tr;
    let label=match text {
        "Source"=>tr("源码","Source"), "Preview"=>tr("预览","Preview"), "Split"=>tr("分屏","Split"), "Save"=>tr("保存","Save"),
        "Open Folder..."=>tr("打开文件夹…","Open Folder..."), "Notebook"=>tr("笔记库","Notebook"), "Recent"=>tr("最近打开","Recent"), "Sidebar"=>tr("侧栏","Sidebar"),
        "Save Copy..."=>tr("保存副本…","Save Copy..."), "Reload from Disk"=>tr("从磁盘重新加载","Reload from Disk"), "Confirm Reload (discard edits)"=>tr("确认重新加载（放弃修改）","Confirm Reload (discard edits)"),
        "Dotfiles"=>tr("显示隐藏文件","Dotfiles"), "Hide dotfiles"=>tr("隐藏点文件","Hide dotfiles"), _=>return text.to_string().into(),
    }; label.into()
}
fn folder_diagnostic(text: &str) -> String {
    use crate::settings::tr;
    match text {
        "File changed on disk. Your edits are retained; save a copy or reload explicitly."=>tr("文件已在磁盘上更改。修改已保留，请保存副本或重新加载。", "File changed on disk. Your edits are retained; save a copy or reload explicitly."),
        "Save unsaved files before opening another file"=>tr("请先保存未保存的文件，再打开其他文件", "Save unsaved files before opening another file"),
        "Many unsaved buffers are retained. Save files before opening more."=>tr("已保留多份未保存的修改。请先保存，再打开更多文件。", "Many unsaved buffers are retained. Save files before opening more."),
        "Collapse a folder before expanding another; 128 folders are open"=>tr("已展开 128 个文件夹，请先折叠部分文件夹", "Collapse a folder before expanding another; 128 folders are open"),
        "This file type has no source editor"=>tr("此文件类型没有源码编辑器", "This file type has no source editor"),
        _=>return crate::settings::diagnostic(text),
    }.to_string()
}
