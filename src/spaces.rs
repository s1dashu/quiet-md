//! 空间 (spaces), after Arc's and Zen's: separate libraries side by side,
//! each with its own folder, document theme and CSS laid over that theme
//! (`space.css` in its folder). The strip at the sidebar's foot switches
//! them, so do ⌃1 – ⌃9, ⌘⌥← / ⌘⌥→ and a sideways two-finger swipe over the
//! sidebar, and the mouse's back / forward side buttons; 设置 › 空间 edits them.
//!
//! As in Arc, each window shows a space of its own: switching changes only
//! that window, and a new window (⌃⌘N) opens in the space of the one it came
//! from. Windows of one space share its `SharedStore` (`Libraries`). Paths
//! and the document theme are app-wide statics (`Store::root_dir`,
//! `themes::active`), so a window `enter`s its space before it draws or
//! handles input.
//!
//! `spaces.json` in the default library (`Store::base_dir`) lists them, and
//! the space last used, which new windows and the next launch open. The
//! first space is that default library itself and cannot be removed; the
//! others keep their folder's path. Removing a space never touches its folder.

use crate::app::{View, Workspace};
use crate::input::{TextField, TextFieldEvent};
use crate::settings::{self, tr, Page};
use crate::shared_store::SharedStore;
use crate::sidebar as sb;
use crate::store::Store;
use crate::theme::Theme;
use crate::ui::{self, TipExt};
use gpui::{prelude::FluentBuilder, *};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, Instant};

actions!(workspace, [NewWindow, NextSpace, PrevSpace, GoSpace1, GoSpace2, GoSpace3, GoSpace4, GoSpace5, GoSpace6, GoSpace7, GoSpace8, GoSpace9]);

/// The default library's id in `spaces.json`.
pub const DEFAULT: &str = "default";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Space {
    pub id: String,
    /// Empty shows the default name (`name`).
    pub name: String,
    /// An emoji or a letter or two; empty shows the name's first letter.
    pub icon: String,
    /// The library folder; empty for the default library. A relative path
    /// is inside the default library, so moving that keeps its spaces.
    pub dir: String,
    /// Document theme id; empty follows 设置 › 外观.
    pub theme: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    spaces: Vec<Space>,
    /// The space last used: new windows and the next launch open it.
    active: String,
}

static CONFIG: RwLock<Option<Config>> = RwLock::new(None);
/// The space of the window being drawn or handled (`enter`).
static CURRENT: RwLock<Option<String>> = RwLock::new(None);

fn path() -> PathBuf { Store::base_dir().join("spaces.json") }

/// `spaces.json`, repaired: the default library first and only once, ids
/// unique, the active one present.
fn read() -> Config { repair(std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()) }

fn repair(mut c: Config) -> Config {
    let mut seen = std::collections::HashSet::new();
    c.spaces.retain(|s| !s.id.is_empty() && seen.insert(s.id.clone()));
    match c.spaces.iter().position(|s| s.id == DEFAULT) {
        Some(i) => { let s = c.spaces.remove(i); c.spaces.insert(0, s); }
        None => c.spaces.insert(0, Space { id: DEFAULT.into(), ..Default::default() }),
    }
    c.spaces[0].dir.clear();
    if !c.spaces.iter().any(|s| s.id == c.active) { c.active = DEFAULT.into(); }
    c
}

fn config() -> Config {
    if CONFIG.read().unwrap().is_none() { *CONFIG.write().unwrap() = Some(read()); }
    CONFIG.read().unwrap().clone().unwrap_or_default()
}

fn write(c: Config) -> anyhow::Result<()> {
    use std::io::Write;
    let base = Store::base_dir();
    std::fs::create_dir_all(&base)?;
    let tmp = base.join("spaces.json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(&c)?)?;
    f.sync_all()?;
    std::fs::rename(tmp, path())?;
    *CONFIG.write().unwrap() = Some(c);
    Ok(())
}

/// Startup: read the spaces; paths resolve in the last used one's library.
pub fn load() {
    let c = read();
    let last = c.spaces.iter().find(|s| s.id == c.active).cloned().unwrap_or_default();
    Store::set_root_dir(root(&last));
    *CONFIG.write().unwrap() = Some(c);
}

pub fn list() -> Vec<Space> { config().spaces }
fn find(id: &str) -> Option<Space> { config().spaces.into_iter().find(|s| s.id == id) }
/// The space of the window being drawn or handled; before any window, the last used.
pub fn active() -> Space {
    let c = config();
    let id = CURRENT.read().unwrap().clone().unwrap_or(c.active.clone());
    c.spaces.iter().find(|s| s.id == id).or(c.spaces.first()).cloned().unwrap_or_default()
}
/// The space last used (new windows open it).
pub fn last_id() -> String { config().active }
fn index_of(id: &str) -> usize { list().iter().position(|s| s.id == id).unwrap_or(0) }

/// Remember `id` as the space last used, and make its library the one
/// windows that are not a space's (quick notes, window sizes) use.
fn set_last(id: &str, cx: &mut App) {
    if let Some(st) = cx.try_global::<Libraries>().and_then(|l| find(id).and_then(|s| l.0.iter().find(|st| st.root() == folder(&s)).cloned())) { cx.set_global(st) }
    let mut c = config();
    if c.active == id { return }
    c.active = id.into();
    if let Err(e) = write(c) { eprintln!("无法保存 spaces.json：{e:#}"); }
}

/// The default library's store: `settings.json` there holds the window and
/// sidebar settings for every space, so they look the same whichever is open.
pub struct UiStore(pub SharedStore);
impl Global for UiStore {}

/// The library of every space a window has open; windows of one space share it.
#[derive(Default)]
struct Libraries(Vec<SharedStore>);
impl Global for Libraries {}

/// Space `id`'s library, opened once.
pub fn store_for(id: &str, cx: &mut App) -> anyhow::Result<SharedStore> {
    let s = find(id).ok_or_else(|| anyhow::anyhow!(tr("找不到这个空间", "That space no longer exists")))?;
    let dir = folder(&s);
    if let Some(st) = cx.default_global::<Libraries>().0.iter().find(|st| st.root() == dir) { return Ok(st.clone()) }
    let st = SharedStore::open_at(dir)?;
    cx.default_global::<Libraries>().0.push(st.clone());
    Ok(st)
}

/// Space `id` and its library, or (its folder gone: an unplugged disk …)
/// the default one and why.
pub fn open(id: &str, cx: &mut App) -> anyhow::Result<(String, SharedStore, Option<String>)> {
    match store_for(id, cx) {
        Ok(st) => Ok((id.into(), st, None)),
        Err(e) if id != DEFAULT => store_for(DEFAULT, cx).map(|st| (DEFAULT.into(), st, Some(crate::localized_format!("无法打开空间：{}", "Could not open the space: {}", settings::diagnostic(&format!("{e:#}")))))),
        Err(e) => Err(e),
    }
}

/// Resolve paths in space `id`'s library and draw with its theme and CSS,
/// until another window enters its own.
pub fn enter(id: &str, cx: &App) {
    let c = config();
    let s = c.spaces.iter().find(|s| s.id == id).or(c.spaces.first()).cloned().unwrap_or_default();
    Store::set_root_dir(root(&s));
    let theme = if s.theme.is_empty() { settings::prefs().theme_id() } else { s.theme.clone() };
    let css = css_path(&s);
    *CURRENT.write().unwrap() = Some(s.id);
    crate::themes::show(&theme, &css, || cx.text_system().all_font_names());
}

/// `enter` the space of the window `window` (from an editor, after an await).
pub fn enter_window(window: &Window, cx: &App) {
    if let Some(Some(w)) = window.root::<Workspace>() { enter(&w.read(cx).space, cx) }
}

/// The CSS a space lays over its document theme.
pub fn css_path(s: &Space) -> PathBuf { folder(s).join("space.css") }
/// Every space's CSS file (watched like the theme files).
pub fn css_files() -> Vec<PathBuf> { list().iter().map(css_path).collect() }

/// The library folder `Store::set_root_dir` takes: `None` for the default library.
pub fn root(s: &Space) -> Option<PathBuf> {
    if s.id == DEFAULT || s.dir.trim().is_empty() { return None }
    let d = s.dir.trim();
    Some(match d.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| PathBuf::from(d)),
        None if std::path::Path::new(d).is_relative() => Store::base_dir().join(d),
        None => PathBuf::from(d),
    })
}

pub fn folder(s: &Space) -> PathBuf { root(s).unwrap_or_else(Store::base_dir) }

/// The folder as 设置 shows it: `~/…` under the home folder (not on Windows).
pub fn folder_label(s: &Space) -> String {
    let p = folder(s);
    if cfg!(target_os = "windows") { return p.display().to_string() }
    dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(|r| format!("~/{}", r.display()))).unwrap_or_else(|| p.display().to_string())
}

pub fn name(s: &Space) -> String {
    let n = s.name.trim();
    if !n.is_empty() { n.to_string() } else if s.id == DEFAULT { tr("个人", "Personal").into() } else { tr("空间", "Space").into() }
}

/// What the strip shows: the icon, else the name's first letter.
pub fn glyph(s: &Space) -> String {
    let i = s.icon.trim();
    if !i.is_empty() { return i.to_string() }
    name(s).chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

/// The current window's space's own document theme.
pub fn theme_override() -> Option<String> { Some(active().theme).filter(|t| !t.is_empty()) }

/// A new space with a folder of its own inside the default library's `spaces/` (kept relative).
fn add() -> anyhow::Result<Space> {
    let mut c = config();
    let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    let n = (2..).find(|n| !c.spaces.iter().any(|s| name(s) == crate::localized_format!("空间 {}", "Space {}", n))).unwrap_or(2);
    let s = Space { id: id.clone(), name: crate::localized_format!("空间 {}", "Space {}", n), dir: format!("spaces/{id}"), ..Default::default() };
    c.spaces.push(s.clone());
    write(c)?;
    Ok(s)
}

pub fn update(id: &str, f: impl FnOnce(&mut Space)) -> anyhow::Result<()> {
    let mut c = config();
    let Some(s) = c.spaces.iter_mut().find(|s| s.id == id) else { return Ok(()) };
    f(s);
    if s.id == DEFAULT { s.dir.clear(); }
    write(c)
}

fn remove(id: &str) -> anyhow::Result<()> {
    let mut c = config();
    c.spaces.retain(|s| s.id != id || s.id == DEFAULT);
    write(c)
}

/// One place along the strip from space `from`, wrapping like Arc.
fn neighbour(from: &str, step: isize) -> Option<String> {
    let list = list();
    (list.len() > 1).then(|| list[(index_of(from) as isize + step).rem_euclid(list.len() as isize) as usize].id.clone())
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-cmd-n", NewWindow, None),
        KeyBinding::new("cmd-alt-right", NextSpace, None),
        KeyBinding::new("cmd-alt-left", PrevSpace, None),
        KeyBinding::new("ctrl-1", GoSpace1, None),
        KeyBinding::new("ctrl-2", GoSpace2, None),
        KeyBinding::new("ctrl-3", GoSpace3, None),
        KeyBinding::new("ctrl-4", GoSpace4, None),
        KeyBinding::new("ctrl-5", GoSpace5, None),
        KeyBinding::new("ctrl-6", GoSpace6, None),
        KeyBinding::new("ctrl-7", GoSpace7, None),
        KeyBinding::new("ctrl-8", GoSpace8, None),
        KeyBinding::new("ctrl-9", GoSpace9, None),
    ]);
}

/// The action that jumps to the `i`th space (0-based), for key hints.
fn jump_action(i: usize) -> Option<(String, &'static str)> {
    const DEFAULTS: [&str; 9] = ["Control + 1", "Control + 2", "Control + 3", "Control + 4", "Control + 5", "Control + 6", "Control + 7", "Control + 8", "Control + 9"];
    (i < 9).then(|| (format!("workspace::GoSpace{}", i + 1), DEFAULTS[i]))
}

/// What `space.css` starts as.
fn css_template(s: &Space) -> String {
    crate::localized_format!(
        "/* 空间「{}」的 CSS：叠加在这个空间的文档主题上，写法与主题 CSS 相同（见 docs/THEMES.md）。\n   这里的变量和规则优先于主题，保存后立即生效。例如：\n\n.page {{ background-color: #fbf8f1; font-size: 17px; }}\n.title {{ color: #9a3412; }}\n*/\n",
        "/* CSS of the space “{}”: laid over this space’s document theme, written like theme CSS (see docs/THEMES.md).\n   Its variables and rules win over the theme’s, as soon as the file is saved. For example:\n\n.page {{ background-color: #fbf8f1; font-size: 17px; }}\n.title {{ color: #9a3412; }}\n*/\n",
        name(s))
}

/// The other main windows, and note windows, that may be updated now (not
/// one on the update stack: GPUI panics on that, and `is_active` is None for it).
fn other_windows(me: EntityId, cx: &mut App) -> Vec<WindowHandle<Workspace>> {
    cx.windows().into_iter().filter_map(|w| w.downcast::<Workspace>())
        .filter(|h| h.is_active(cx).is_some() && h.entity(cx).is_ok_and(|e| e.entity_id() != me)).collect()
}

/// The space edited in 设置 › 空间: its id and its name and icon fields.
pub struct SpaceEdit {
    pub id: String,
    name: Entity<TextField>,
    icon: Entity<TextField>,
    _subs: Vec<Subscription>,
}

/// A sideways two-finger swipe over the sidebar: how far it went, whether
/// it already switched (one switch per swipe), and whether the fingers
/// lifted (the momentum events macOS sends after that are not a new swipe).
#[derive(Clone, Copy)]
pub struct Swipe { dx: f32, done: bool, lifted: bool, at: Instant }
impl Default for Swipe { fn default() -> Self { Swipe { dx: 0., done: false, lifted: false, at: Instant::now() } } }

/// Sideways travel that switches the space.
const SWIPE: f32 = 90.;

/// The switch animation, after Arc's: the new space's documents slide in
/// from the side it lies on and the page fades in.
const SLIDE: Duration = Duration::from_millis(300);
/// How far the list travels (the page half that).
const SLIDE_PX: f32 = 36.;

/// The system's 减弱动态效果 (Reduce motion): switch without the slide.
fn reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc::{class, msg_send, sel, sel_impl};
        use objc::runtime::{Object, BOOL, YES};
        let ws: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let on: BOOL = msg_send![ws, accessibilityDisplayShouldReduceMotion];
        on == YES
    }
    #[cfg(not(target_os = "macos"))]
    false
}

impl Workspace {
    /// Make this window's space the current one (`enter`).
    pub(crate) fn enter(&self, cx: &App) { enter(&self.space, cx) }

    /// This window became the active one: its space is the last used.
    pub(crate) fn space_activated(&self, cx: &mut Context<Self>) {
        self.enter(cx);
        if !self.quick_note { set_last(&self.space, cx) }
    }

    /// The switch animation now: (eased progress 0 → 1, direction ±1), or
    /// `None` when there is none; asks for the next frame while it runs.
    pub(crate) fn space_motion(&mut self, window: &mut Window) -> Option<(f32, f32)> {
        let (at, dir) = self.space_anim?;
        let t = at.elapsed().as_secs_f32() / SLIDE.as_secs_f32();
        if t >= 1. { self.space_anim = None; return None }
        window.request_animation_frame();
        Some((1. - (1. - t).powi(3), dir))
    }

    /// The new space's sidebar list, sliding in.
    pub(crate) fn slide_list(&self, d: Stateful<Div>, m: Option<(f32, f32)>) -> Stateful<Div> {
        match m { Some((e, dir)) => d.relative().left(px(dir * SLIDE_PX * (1. - e))).opacity(e), None => d }
    }

    /// The new space's page, fading in.
    pub(crate) fn slide_page(m: Option<(f32, f32)>) -> impl Fn(Div) -> Div {
        move |d| match m { Some((e, dir)) => d.relative().left(px(dir * SLIDE_PX * 0.5 * (1. - e))).opacity(0.15 + 0.85 * e), None => d }
    }

    /// Key and menu actions; attached to the window's root, which also
    /// enters the window's space before any of its handlers run.
    pub(crate) fn space_actions(&self, root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        fn go(w: &mut Workspace, i: usize, window: &mut Window, cx: &mut Context<Workspace>) {
            if let Some(s) = list().get(i) { let id = s.id.clone(); w.switch_space(&id, window, cx); }
        }
        fn step(w: &mut Workspace, by: isize, window: &mut Window, cx: &mut Context<Workspace>) { step_by_swipe(w, by, window, cx) }
        let (a, b, c) = (self.space.clone(), self.space.clone(), self.space.clone());
        // The mouse's side buttons, as in Arc and Zen: back is the previous space, forward the next.
        let side = |by: isize| cx.listener(move |w: &mut Workspace, _: &MouseDownEvent, window, cx| step(w, by, window, cx));
        root.capture_any_mouse_down(move |_, _, cx| enter(&a, cx))
            .capture_any_mouse_up(move |_, _, cx| enter(&b, cx))
            .capture_key_down(move |_, _, cx| enter(&c, cx))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), side(-1))
            .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), side(1))
            .on_action(cx.listener(|w, _: &NewWindow, window, cx| {
                // Saved first: the new window may open the same document.
                w.flush(cx);
                let (space, from) = (w.space.clone(), window.bounds());
                cx.defer(move |cx| crate::app::open_main_window(Some(space), Some(from), cx));
            }))
            .on_action(cx.listener(|w, _: &NextSpace, window, cx| step(w, 1, window, cx)))
            .on_action(cx.listener(|w, _: &PrevSpace, window, cx| step(w, -1, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace1, window, cx| go(w, 0, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace2, window, cx| go(w, 1, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace3, window, cx| go(w, 2, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace4, window, cx| go(w, 3, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace5, window, cx| go(w, 4, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace6, window, cx| go(w, 5, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace7, window, cx| go(w, 6, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace8, window, cx| go(w, 7, window, cx)))
            .on_action(cx.listener(|w, _: &GoSpace9, window, cx| go(w, 8, window, cx)))
    }

    /// Show space `id` in this window (other windows keep theirs). Its
    /// document is saved first; when it cannot be, nothing changes (this
    /// window asks 重试 / 放弃修改 first).
    pub(crate) fn switch_space(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.space == id { return }
        let target = id.to_string();
        if !self.save_before(cx, move |w, window, cx| w.switch_space(&target, window, cx)) { return }
        if self.open_space(id, window, cx) {
            self.show_toast(&crate::localized_format!("已切换到「{}」", "Switched to “{}”", name(&active())), cx);
        }
    }

    /// Show space `id` (this window's document already saved) from its
    /// folder as it is now. A folder that cannot be opened leaves the window as it was.
    fn open_space(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let store = match store_for(id, cx) {
            Ok(st) => st,
            Err(e) => { self.show_toast_kind(ui::ToastKind::Error, &crate::localized_format!("无法打开空间：{}", "Could not open the space: {}", settings::diagnostic(&format!("{e:#}"))), cx); return false }
        };
        let dir = if index_of(id) >= index_of(&self.space) { 1. } else { -1. };
        self.space_anim = (!reduce_motion()).then(|| (Instant::now(), dir));
        self.space = id.into();
        self.store = store;
        self.space_edit = None;
        self.space_activated(cx);
        self.reload_library(window, cx);
        if let Some(editor) = &self.editor { editor.update(cx, |e, cx| { e.cache.borrow_mut().clear(); e.refresh_theme(); cx.notify(); }) }
        true
    }

    /// Every other window showing space `id` runs `f` (deferred: this one may be mid-update).
    fn each_window_of(&self, id: &str, cx: &mut Context<Self>, f: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static) {
        let id = id.to_string();
        let others = other_windows(cx.entity_id(), cx);
        cx.defer(move |cx| for h in others {
            let _ = h.update(cx, |w, window, cx| if w.space == id { f(w, window, cx) });
        });
    }

    fn add_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match add() {
            Ok(s) => {
                self.edit_space(&s.id, window, cx);
                self.switch_space(&s.id, window, cx);
                if self.view != View::Settings || self.settings_page != Page::Spaces { self.open_settings(Page::Spaces, cx); }
            }
            Err(e) => self.show_toast_kind(ui::ToastKind::Error, &format!("{}: {e}", tr("无法新建空间", "Could not create the space")), cx),
        }
    }

    /// Every window that shows space `id` goes to the default one first;
    /// one whose document cannot be saved keeps the space in the list.
    fn remove_space(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == DEFAULT { return }
        if self.space == id {
            self.switch_space(DEFAULT, window, cx);
            if self.space == id { return }
        }
        for h in other_windows(cx.entity_id(), cx) {
            let left = h.update(cx, |w, window, cx| { if w.space == id && w.flush(cx) { w.open_space(DEFAULT, window, cx); } w.space != id }).unwrap_or(true);
            if !left { return self.show_toast_kind(ui::ToastKind::Error, tr("另一个窗口的文档未能保存，没有移除空间", "A document in another window could not be saved, so the space was not removed"), cx) }
        }
        if let Err(e) = remove(id) { return self.show_toast_kind(ui::ToastKind::Error, &e.to_string(), cx) }
        if last_id() == id { set_last(DEFAULT, cx) }
        self.space_edit = None;
        settings::refresh_all(self, cx);
        self.show_toast(tr("已移除空间，文件夹里的文档保留在原处", "Space removed; its documents stay in their folder"), cx);
    }

    /// Open (and first write) space `id`'s `space.css` in the user's editor.
    fn edit_space_css(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(s) = find(id) else { return };
        let p = css_path(&s);
        let made = if p.exists() { Ok(()) } else { std::fs::create_dir_all(folder(&s)).and_then(|_| std::fs::write(&p, css_template(&s))) };
        if let Err(e) = made { return self.show_toast_kind(ui::ToastKind::Error, &e.to_string(), cx) }
        settings::open_in_editor(&p, cx);
        self.reload_themes(cx);
    }

    /// Choose space `id`'s folder; windows showing it reopen from there.
    fn pick_space_folder(&mut self, id: &str, cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(tr("选择", "Choose").into()), extensions: None }));
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let paths = match prompt { Some(p) => p.await.ok().and_then(Result::ok).flatten(), None => picked };
            let Some(dir) = paths.and_then(|p| p.into_iter().next()) else { return };
            let _ = this.update(cx, |w, cx| {
                if list().iter().any(|s| s.id != id && folder(s) == dir) {
                    return w.show_toast_kind(ui::ToastKind::Error, tr("另一个空间已经使用这个文件夹", "Another space already uses this folder"), cx);
                }
                if let Err(e) = update(&id, |s| s.dir = dir.display().to_string()) { return w.show_toast_kind(ui::ToastKind::Error, &e.to_string(), cx) }
                // Windows showing it: saved to the old folder, then reopened from the new one.
                let reopen = |w: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>| { let id = w.space.clone(); if w.flush(cx) { w.open_space(&id, window, cx); } };
                if w.space == id {
                    let me = cx.entity_id();
                    let Some(h) = cx.windows().into_iter().find_map(|h| h.downcast::<Workspace>().filter(|h| h.entity(cx).is_ok_and(|e| e.entity_id() == me))) else { return };
                    cx.defer(move |cx| { let _ = h.update(cx, reopen); });
                }
                w.each_window_of(&id, cx, reopen);
                cx.notify();
            });
        }).detach();
    }

    pub(crate) fn set_space_theme(&mut self, id: &str, theme: String, cx: &mut Context<Self>) {
        self.space_theme_menu = false;
        if let Err(e) = update(id, |s| s.theme = theme) { return self.show_toast_kind(ui::ToastKind::Error, &e.to_string(), cx) }
        // Windows of that space draw with it from their next frame.
        settings::refresh_all(self, cx);
    }

    /// Show space `id` in 设置 › 空间's editor.
    fn edit_space(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = list().into_iter().find(|s| s.id == id) else { return };
        let field = |text: &str, placeholder: String, max: usize, cx: &mut Context<Self>| cx.new(|cx| {
            let mut f = TextField::new(placeholder, cx);
            f.placeholder_color = Theme::text_disabled();
            f.max_len = Some(max);
            f.set_text(text, cx);
            f
        });
        let name_f = field(&s.name, name(&s), 40, cx);
        let icon_f = field(&s.icon, glyph(&s), 2, cx);
        let save = |kind: fn(&mut Space, String)| move |w: &mut Workspace, f: &Entity<TextField>, ev: &TextFieldEvent, window: &mut Window, cx: &mut Context<Workspace>| match ev {
            TextFieldEvent::Changed => {
                let (Some(id), text) = (w.space_edit.as_ref().map(|e| e.id.clone()), f.read(cx).text.trim().to_string()) else { return };
                if let Err(e) = update(&id, |s| kind(s, text)) { w.show_toast_kind(ui::ToastKind::Error, &e.to_string(), cx) }
                cx.notify();
            }
            TextFieldEvent::Submit | TextFieldEvent::Cancel => window.focus(&w.focus),
            _ => {}
        };
        let subs = vec![
            cx.subscribe_in(&name_f, window, save(|s, t| s.name = t)),
            cx.subscribe_in(&icon_f, window, save(|s, t| s.icon = t)),
        ];
        self.space_edit = Some(SpaceEdit { id: id.into(), name: name_f, icon: icon_f, _subs: subs });
        self.space_theme_menu = false;
        cx.notify();
    }

    /// Before drawing 设置 › 空间: an editor for a space that still exists.
    pub(crate) fn sync_space_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view != View::Settings || self.settings_page != Page::Spaces { return }
        // The list names every space's theme, not only the one in use.
        if !crate::themes::all_loaded() { crate::themes::load(&settings::theme_id(), true, || cx.text_system().all_font_names()); }
        if self.space_edit.as_ref().is_some_and(|e| list().iter().any(|s| s.id == e.id)) { return }
        let id = self.space.clone();
        self.edit_space(&id, window, cx);
    }

    /// Sideways scrolling over the sidebar: a two-finger swipe (or a
    /// sideways wheel) past `SWIPE` moves one space, left to the next.
    pub(crate) fn space_swipe(&mut self, ev: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let d = ev.delta.pixel_delta(px(20.));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        let now = Instant::now();
        // A new swipe: fingers down, or (a wheel, which has no phases) a pause.
        if matches!(ev.touch_phase, TouchPhase::Started) || now.duration_since(self.swipe.at) > Duration::from_millis(350) { self.swipe = Swipe::default(); }
        self.swipe.at = now;
        // Fingers up: the momentum that follows keeps coming without pauses,
        // and must not switch again (with two spaces that switched straight back).
        if matches!(ev.touch_phase, TouchPhase::Ended) { self.swipe.lifted = true; return }
        if self.swipe.lifted || self.swipe.done || dx.abs() <= dy.abs() * 1.5 { return }
        self.swipe.dx += dx;
        if self.swipe.dx.abs() < SWIPE { return }
        self.swipe.done = true;
        step_by_swipe(self, if self.swipe.dx < 0. { 1 } else { -1 }, window, cx);
    }

    /// The strip above the sidebar's footer: one button per space (the
    /// active one on the selection pill), the active name, and +.
    pub(crate) fn space_strip(&self, cx: &mut Context<Self>) -> Div {
        let spaces = list();
        let at = index_of(&self.space);
        let current = spaces.get(at).map(name).unwrap_or_default();
        let chip = |id: SharedString| div().id(id).flex_none().size(px(28.)).flex().items_center().justify_center().rounded(px(8.)).cursor_pointer();
        div().flex_none().px(px(sb::SB_X)).pt(px(4.)).pb(px(2.)).flex().items_center().gap(px(2.))
            .children(spaces.iter().enumerate().map(|(i, s)| {
                let on = i == at;
                let (id, edit_id) = (s.id.clone(), s.id.clone());
                let (title, hint) = (name(s), jump_action(i).map(|(a, d)| crate::shortcuts::hint(&a, d)));
                chip(format!("space-{}", s.id).into())
                    .text_size(px(14.)).line_height(px(18.)).font_weight(FontWeight::MEDIUM)
                    .when(on, |d| d.bg(Theme::sidebar_selected()).text_color(Theme::text()))
                    .when(!on, |d| d.text_color(Theme::sidebar_text()).hover(|s| s.bg(Theme::sidebar_hover())))
                    .child(glyph(s))
                    .tip(&title, hint)
                    .on_click(cx.listener(move |w, _, window, cx| w.switch_space(&id, window, cx)))
                    .on_mouse_down(MouseButton::Right, cx.listener(move |w, _, window, cx| { w.edit_space(&edit_id, window, cx); w.open_settings(Page::Spaces, cx); }))
            }))
            .child(div().flex_1().min_w_0().ml(px(6.)).truncate().text_size(px(12.)).line_height(px(16.)).text_color(Theme::sidebar_section()).child(current))
            .child(chip("space-add".into()).hover(|s| s.bg(Theme::sidebar_hover())).child(ui::icon("plus", 16., Theme::icon()))
                .tip(tr("新建空间", "New space"), None)
                .on_click(cx.listener(|w, _, window, cx| w.add_space(window, cx))))
    }

    /// 设置 › 空间: the list (click to edit, 切换 to open), then the edited space's name, icon, folder and theme.
    pub(crate) fn spaces_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let spaces = list();
        let at = index_of(&self.space);
        let edit = self.space_edit.as_ref();
        let rows: Vec<AnyElement> = spaces.iter().enumerate().map(|(i, s)| {
            let on = edit.is_some_and(|e| e.id == s.id);
            let (id, open_id) = (s.id.clone(), s.id.clone());
            let key = jump_action(i).map(|(a, _)| crate::shortcuts::pretty(&crate::shortcuts::key_of(cx, &a)));
            div().id(SharedString::from(format!("space-row-{}", s.id))).w_full().min_h(px(52.)).px(px(16.)).py(px(8.)).flex().items_center().gap(px(12.)).cursor_pointer()
                .when(on, |d| d.bg(Theme::fill_hover()))
                .when(!on, |d| d.hover(|s| s.bg(Theme::fill_hover())))
                .child(div().flex_none().size(px(32.)).flex().items_center().justify_center().rounded(px(9.)).bg(Theme::fill_hover_strong()).text_size(px(16.)).child(glyph(s)))
                .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.))
                    .child(div().text_size(px(13.)).line_height(px(20.)).text_color(Theme::text()).child(name(s)))
                    .child(div().truncate().text_size(px(12.)).line_height(px(17.)).text_color(Theme::text_caption()).child(folder_label(s))))
                .children(key.filter(|_| i < 9).map(|k| div().flex_none().text_size(px(12.)).text_color(Theme::text_caption()).child(k)))
                .child(if i == at {
                    div().flex_none().w(px(64.)).flex().justify_center().text_size(px(12.)).text_color(Theme::text_caption()).child(tr("当前", "Current")).into_any_element()
                } else {
                    settings::button(SharedString::from(format!("space-open-{}", s.id)), tr("切换", "Switch")).w(px(64.))
                        .on_click(cx.listener(move |w, _, window, cx| { cx.stop_propagation(); w.switch_space(&open_id, window, cx) })).into_any_element()
                })
                .on_click(cx.listener(move |w, _, window, cx| w.edit_space(&id, window, cx)))
                .into_any_element()
        }).collect();
        let list_group = settings::group(None, rows);
        let add = div().flex().justify_between().items_center().gap(px(16.)).px(px(4.))
            .child(div().flex_1().min_w_0().text_size(px(12.)).line_height(px(17.)).text_color(Theme::text_caption())
                .child(tr("每个空间有自己的文档库文件夹、文档主题和 CSS，每个窗口显示自己的空间。⌃1–⌃9 直接切换，⌘⌥← / ⌘⌥→、鼠标侧键或在侧栏左右轻扫依次切换，⌃⌘N 在新窗口打开当前空间。", "Each space has its own documents folder, theme and CSS, and each window shows a space of its own. ⌃1–⌃9 jump to a space; ⌘⌥← / ⌘⌥→, the mouse’s side buttons or a sideways swipe on the sidebar step through them; ⌃⌘N opens this space in a new window.")))
            .child(settings::button("space-add-btn", tr("新建空间", "New Space")).on_click(cx.listener(|w, _, window, cx| w.add_space(window, cx))));
        let detail = edit.and_then(|e| spaces.iter().find(|s| s.id == e.id).map(|s| (e, s.clone()))).map(|(e, s)| {
            let field = |f: &Entity<TextField>, w: f32| div().w(px(w)).h(px(28.)).px(px(8.)).flex().items_center().rounded(px(8.)).bg(Theme::fill_hover()).child(f.clone());
            let theme_label = if s.theme.is_empty() { tr("跟随外观设置", "Same as Appearance").to_string() } else {
                crate::themes::themes().iter().find(|t| t.id == s.theme).map(|t| t.name().to_string()).unwrap_or_else(|| s.theme.clone())
            };
            let anchor = self.space_theme_anchor.clone();
            let picker = settings::popup_button("space-theme", theme_label, self.space_theme_menu)
                .child(canvas(move |b, _, _| anchor.set(Some(b)), |_, _, _, _| {}).absolute().inset_0()).relative()
                .on_click(cx.listener(|w, _, _, cx| {
                    if !w.space_theme_menu && !crate::themes::all_loaded() { crate::themes::load(&settings::theme_id(), true, || cx.text_system().all_font_names()); }
                    w.space_theme_menu = !w.space_theme_menu;
                    cx.notify();
                }));
            let (pick_id, rm_id, css_id, reveal) = (s.id.clone(), s.id.clone(), s.id.clone(), folder(&s));
            let is_default = s.id == DEFAULT;
            let css = css_path(&s);
            let theme = if s.theme.is_empty() { settings::prefs().theme_id() } else { s.theme.clone() };
            let css_note: SharedString = if css.exists() { tr("文件夹里的 space.css，叠加在文档主题上，保存后立即生效。", "space.css in its folder, laid over the document theme; applies as soon as it is saved.").into() }
                else { tr("还没有。编辑时在文件夹里新建 space.css，叠加在文档主题上。", "None yet. Editing creates space.css in its folder, laid over the document theme.").into() };
            let mut rows = vec![
                settings::row(tr("名称", "Name"), None, field(&e.name, 200.)).into_any_element(),
                settings::row(tr("图标", "Icon"), Some(tr("一个表情或一两个字母；留空用名称的首字。", "An emoji or a letter or two; empty uses the name’s first letter.").into()), field(&e.icon, 64.)).into_any_element(),
                settings::row(tr("文档库文件夹", "Documents folder"), Some(folder_label(&s).into()), div().flex().gap(px(8.))
                    .child(settings::button("space-reveal", if cfg!(target_os = "macos") { tr("在访达中显示", "Show in Finder") } else { tr("显示", "Show") })
                        .on_click(move |_, _, cx| { let _ = std::fs::create_dir_all(&reveal); cx.reveal_path(&reveal) }))
                    .when(!is_default, |d| d.child(settings::button("space-folder", tr("更改…", "Change…")).on_click(cx.listener(move |w, _, _, cx| w.pick_space_folder(&pick_id, cx))))))
                    .into_any_element(),
                settings::row(tr("文档主题", "Document theme"), None, picker).into_any_element(),
                settings::row(tr("空间 CSS", "Space CSS"), Some(css_note),
                    settings::button("space-css", tr("编辑 CSS", "Edit CSS")).on_click(cx.listener(move |w, _, _, cx| w.edit_space_css(&css_id, cx)))).into_any_element(),
            ];
            if !is_default {
                rows.push(settings::row(tr("移除空间", "Remove space"), Some(tr("只从列表移除，文件夹和其中的文档不会删除。", "Only leaves the list; the folder and its documents are kept.").into()),
                    settings::link_button("space-remove", tr("移除", "Remove")).on_click(cx.listener(move |w, _, window, cx| w.remove_space(&rm_id, window, cx)))).into_any_element());
            }
            let warnings = crate::themes::space_warnings(&theme, &css);
            div().w_full().flex().flex_col().gap(px(8.)).child(settings::group(None, rows))
                .children(warnings.into_iter().take(8).map(|w| div().px(px(4.)).text_size(px(12.)).line_height(px(18.)).text_color(Theme::danger()).child(format!("space.css {}：{w}", tr("未生效", "ignored")))))
        });
        div().w_full().flex().flex_col().gap(px(28.))
            .child(div().w_full().flex().flex_col().gap(px(8.)).child(list_group).child(add))
            .children(detail)
            .into_any_element()
    }

    /// 设置 › 空间's theme dropdown: 跟随外观设置, then every theme.
    pub(crate) fn render_space_theme_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.space_theme_menu || self.view != View::Settings || self.settings_page != Page::Spaces { return None }
        let b = self.space_theme_anchor.get()?;
        let id = self.space_edit.as_ref()?.id.clone();
        let current = list().into_iter().find(|s| s.id == id)?.theme;
        let choices: Vec<(String, String)> = std::iter::once((String::new(), tr("跟随外观设置", "Same as Appearance").to_string()))
            .chain(crate::themes::themes().iter().map(|t| (t.id.clone(), t.name().to_string()))).collect();
        let rows: Vec<AnyElement> = choices.into_iter().map(|(theme, label)| {
            let g = SharedString::from(format!("space-theme-{theme}"));
            let (sid, on) = (id.clone(), theme == current);
            ui::menu_row(g.clone(), g.clone())
                .child(div().flex_1().min_w_0().overflow_hidden().child(ui::menu_label(label, g.clone(), gpui::white())))
                .child(div().flex_none().w(px(14.)).when(on, |d| d.child(ui::menu_glyph("check", 14., g.clone()))))
                .on_click(cx.listener(move |w, _, _, cx| w.set_space_theme(&sid, theme.clone(), cx)))
                .into_any_element()
        }).collect();
        let anchor = self.space_theme_anchor.clone();
        let menu = ui::popover().id("space-theme-menu").occlude().flex()
            .on_mouse_down_out(cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                if anchor.get().is_some_and(|b| b.contains(&ev.position)) { return }
                w.space_theme_menu = false;
                cx.notify();
            }))
            .child(div().id("space-theme-list").w(px(220.)).max_h(px(302.)).overflow_y_scroll().py(px(ui::MENU_PAD_Y)).flex().flex_col().children(rows));
        Some(deferred(anchored().anchor(Corner::TopRight).position(point(b.right(), b.bottom() + px(4.))).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(2).into_any_element())
    }
}

/// One space along: the slide follows the step, also where it wraps around.
fn step_by_swipe(w: &mut Workspace, by: isize, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(id) = neighbour(&w.space, by) else { return };
    w.switch_space(&id, window, cx);
    if let Some(a) = w.space_anim.as_mut().filter(|_| w.space == id) { a.1 = by.signum() as f32 }
}

#[cfg(test)]
mod tests {
    use super::{glyph, repair, root, Config, Space, DEFAULT};

    #[test]
    fn default_space_is_first_and_kept() {
        let c = repair(serde_json::from_str(r#"{"spaces":[{"id":"a","name":"Work","dir":"/tmp/w"},{"id":"default","dir":"/x"},{"id":"a"},{"id":""}],"active":"gone"}"#).unwrap());
        assert_eq!(c.spaces.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["default", "a"]);
        assert_eq!((c.spaces[0].dir.as_str(), c.active.as_str()), ("", DEFAULT));
        assert_eq!(repair(Config::default()).spaces.len(), 1);
        let s = Space { id: DEFAULT.into(), dir: "/x".into(), ..Default::default() };
        assert_eq!(root(&s), None);
        let w = Space { id: "a".into(), dir: "~/Notes".into(), ..Default::default() };
        assert_eq!(root(&w), dirs::home_dir().map(|h| h.join("Notes")));
        let r = Space { id: "c".into(), dir: "spaces/c".into(), ..Default::default() };
        assert_eq!(root(&r), Some(crate::store::Store::base_dir().join("spaces/c")));
        assert_eq!(glyph(&Space { id: "b".into(), name: "work".into(), ..Default::default() }), "W");
        assert_eq!(glyph(&Space { id: "b".into(), name: "work".into(), icon: "🚀".into(), ..Default::default() }), "🚀");
    }
}
