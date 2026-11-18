//! Workspace window: two panes — the document list on the left
//! (see `sidebar.rs`) and the editor on the right, with the
//! 64px top bar (breadcrumb title + save status) above the page.

use crate::docinfo::DocInfo;
use crate::docmenu::DocSub;
use crate::editor::view::{Editor, EditorEvent};
use crate::input::{TextField, TextFieldEvent};
use crate::settings::display_title;
use crate::store::Entry;
use crate::shared_store::SharedStore;
use crate::theme::{self, Theme, craft};
use crate::ui::{TipExt, TipSide, TitleBarExt, ToastKind, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Autosave waits for this pause in editing, but never longer than
/// SAVE_MAX_DELAY after the oldest change not yet on disk.
const SAVE_DELAY: Duration = Duration::from_millis(500);
const SAVE_MAX_DELAY: Duration = Duration::from_secs(2);

actions!(workspace, [NewDoc, OpenFile, ImportDoc, ImportFolder, ToggleSidebar, FocusSearch, FindInDoc, ReplaceInDoc, OpenSettings, OpenAbout, CheckForUpdates, GoHome, CloseWindow, Quit,
    ReplayOnboarding, HideApp, HideOthers, ShowAll, Minimize, Zoom, ToggleFullScreen]);

/// The menu bar, in the interface language.
pub fn set_menus(cx: &mut App) {
    use crate::settings::tr;
    cx.set_menus(vec![
        Menu {
            name: "Quiet.md".into(),
            items: vec![
                MenuItem::action(tr("关于 Quiet.md", "About Quiet.md"), OpenAbout),
                MenuItem::action(tr("检查更新…", "Check for Updates…"), CheckForUpdates),
                MenuItem::separator(),
                MenuItem::action(tr("设置…", "Settings…"), OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu(tr("服务", "Services"), SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action(tr("隐藏 Quiet.md", "Hide Quiet.md"), HideApp),
                MenuItem::action(tr("隐藏其他", "Hide Others"), HideOthers),
                MenuItem::action(tr("全部显示", "Show All"), ShowAll),
                MenuItem::separator(),
                MenuItem::action(tr("退出 Quiet.md", "Quit Quiet.md"), Quit),
            ],
        },
        Menu {
            name: tr("文件", "File").into(),
            items: vec![
                MenuItem::action(tr("新建文档", "New Document"), NewDoc),
                MenuItem::action(tr("新建笔记窗口", "New Note Window"), crate::quick_note::QuickNew),
                MenuItem::action(tr("打开 Markdown 文件…", "Open Markdown File…"), OpenFile),
                MenuItem::action(tr("导入为文档…", "Import as Document…"), ImportDoc),
                MenuItem::action(tr("导入文件夹…", "Import Folder…"), ImportFolder),
                MenuItem::separator(),
                MenuItem::action(tr("关闭窗口", "Close Window"), CloseWindow),
            ],
        },
        // The standard Edit menu: its items act on the focused editor, and
        // macOS adds 听写 and 表情与符号 to it.
        Menu {
            name: tr("编辑", "Edit").into(),
            items: {
                use crate::editor::keymap as k;
                vec![
                    MenuItem::os_action(tr("撤销", "Undo"), k::Undo, OsAction::Undo),
                    MenuItem::os_action(tr("重做", "Redo"), k::Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action(tr("剪切", "Cut"), k::Cut, OsAction::Cut),
                    MenuItem::os_action(tr("拷贝", "Copy"), k::Copy, OsAction::Copy),
                    MenuItem::action(tr("拷贝为 Markdown", "Copy as Markdown"), k::CopyMarkdown),
                    MenuItem::os_action(tr("粘贴", "Paste"), k::Paste, OsAction::Paste),
                    MenuItem::os_action(tr("全选", "Select All"), k::SelectAll, OsAction::SelectAll),
                ]
            },
        },
        Menu {
            name: tr("窗口", "Window").into(),
            items: vec![
                MenuItem::action(tr("最小化", "Minimize"), Minimize),
                MenuItem::action(tr("缩放", "Zoom"), Zoom),
                MenuItem::separator(),
                MenuItem::action(tr("进入 / 退出全屏幕", "Enter / Exit Full Screen"), ToggleFullScreen),
            ],
        },
    ]);
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-z", crate::editor::keymap::Undo, Some("Workspace")),
        KeyBinding::new("secondary-shift-z", crate::editor::keymap::Redo, Some("Workspace")),
        KeyBinding::new("secondary-y", crate::editor::keymap::Redo, Some("Workspace")),
        KeyBinding::new("cmd-n", NewDoc, None),
        KeyBinding::new("cmd-shift-n", crate::quick_note::QuickNew, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        // For trying the first-launch pages; not listed in Settings › Shortcuts.
        KeyBinding::new("cmd-alt-shift-o", ReplayOnboarding, None),
        KeyBinding::new("cmd-shift-i", ImportDoc, None),
        KeyBinding::new("cmd-o", OpenFile, None),
        KeyBinding::new("cmd-alt-home", GoHome, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-]", ToggleSidebar, None),
        KeyBinding::new("cmd-\\", ToggleSidebar, None),
        KeyBinding::new("cmd-j", FocusSearch, None),
        KeyBinding::new("cmd-f", FindInDoc, None),
        KeyBinding::new("cmd-shift-h", ReplaceInDoc, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    // macOS app and window keys. ⌥⌘H (隐藏其他) stays with the editor's
    // 应用上次的颜色, the Feishu shortcut; 隐藏其他 is in the menu. On Windows
    // the system handles these (Win+↓, Alt+F4), and F11 is full screen.
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
    ]);
    // Windows: F11 for full screen, and Ctrl+H for 查找和替换 as in Office
    // and Notepad (written cmd-h: `shortcuts::pc_binding` maps cmd to
    // ctrl and drops a literal ctrl-h, Cocoa's backspace).
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([KeyBinding::new("f11", ToggleFullScreen, None), KeyBinding::new("cmd-h", ReplaceInDoc, None)]);
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(handle) = cx.active_window().and_then(|w| w.downcast::<Workspace>()) {
            // Key dispatch already borrows this window; close after it returns.
            cx.defer(move |cx| {
                if let Err(e) = handle.update(cx, |ws, window, cx| {
                    crate::quick_note::persist(cx);
                    if ws.save_before(cx, |_, window, _| window.remove_window()) { window.remove_window(); }
                }) { eprintln!("无法关闭笔记窗口：{e}"); }
            });
        }
    });
    cx.on_action(|_: &Quit, cx| cx.defer(quit));
    crate::modes::bind_keys(cx);
}

/// 退出: the open document is saved first; when that fails its dialog asks
/// (重试 / 放弃修改) and quits afterwards. Call it outside any window update
/// (`cx.defer`): a window on the update stack cannot be asked to save.
pub fn quit(cx: &mut App) {
    let ready = cx
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<Workspace>())
        .map(|w| w.update(cx, |ws, _, cx| ws.save_before(cx, |_, _, cx| cx.defer(quit))).unwrap_or(false))
        .fold(true, |ready, saved| ready & saved);
    if ready {
        cx.quit();
    }
}

/// 更改文档库: open documents are saved first (as for 退出), the folder is
/// set up as a library if it isn't one, the current preferences go with
/// it when it has none, and the app restarts into it. The old library
/// stays where it is. Call it outside any window update (`cx.defer`).
pub fn switch_library(root: std::path::PathBuf, cx: &mut App) {
    let ready = cx
        .windows()
        .into_iter()
        .filter_map(|w| w.downcast::<Workspace>())
        .map(|w| { let again = root.clone(); w.update(cx, |ws, _, cx| ws.save_before(cx, move |_, _, cx| cx.defer(move |cx| switch_library(again, cx)))).unwrap_or(false) })
        .fold(true, |ready, saved| ready & saved);
    if !ready { return }
    let prefs = crate::store::Store::internal_dir().join("preferences.json");
    let set = crate::guides::initialize(&root).and_then(|_| crate::store::Store::set_root_dir(root.clone()));
    if let Err(e) = set {
        let msg = format!("{}{e}", crate::settings::tr("无法在这里创建文档库：", "Couldn't create the library here: "));
        if let Some(ws) = cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()) { let _ = ws.update(cx, |w, _, cx| w.show_toast(&msg, cx)); }
        return;
    }
    let mine = crate::store::Store::internal_dir().join("preferences.json");
    if !mine.exists() { let _ = std::fs::create_dir_all(mine.parent().unwrap()).and_then(|_| std::fs::copy(&prefs, &mine)); }
    cx.restart();
}

#[derive(Clone, Debug, PartialEq)]
pub enum SaveStatus {
    /// Freshly opened, nothing saved this session: show the modification time.
    Idle,
    Saved,
    Saving,
    Failed(String),
}

/// Right-click / "…" menu on a sidebar row.
#[derive(Clone, Debug)]
pub struct RowMenu {
    pub id: String,
    pub pos: Point<Pixels>,
    /// Which corner of the menu sits at `pos`.
    pub corner: Corner,
    /// 下载 submenu open.
    pub sub: bool,
    /// The 批量操作 menu for the checked rows.
    pub batch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PaneFocus {
    #[default]
    Main,
    Sidebar,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarItem {
    Home,
    Pinned(String),
    Doc(String),
    Local(String),
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavTarget {
    Doc(String),
    Home,
    Folder(String),
    Trash,
    Settings(crate::settings::Page),
}

#[derive(Clone, Debug, Default)]
pub struct DocHistory {
    pub entries: Vec<NavTarget>,
    pub ix: usize,
    pub navigating: bool,
}

impl DocHistory {
    pub fn record(&mut self, target: NavTarget) {
        if self.navigating {
            return;
        }
        if !self.entries.is_empty() && self.entries.get(self.ix) == Some(&target) {
            return;
        }
        if self.ix + 1 < self.entries.len() {
            self.entries.truncate(self.ix + 1);
        }
        if self.entries.last() != Some(&target) {
            self.entries.push(target);
        }
        if self.entries.len() > 100 {
            self.entries.remove(0);
        }
        self.ix = self.entries.len().saturating_sub(1);
    }

    pub fn step<F: Fn(&NavTarget) -> bool>(&self, forward: bool, valid: F) -> Option<(usize, NavTarget)> {
        if self.entries.len() < 2 {
            return None;
        }
        let cur = self.entries.get(self.ix);
        if forward {
            for i in (self.ix + 1)..self.entries.len() {
                let t = &self.entries[i];
                if Some(t) != cur && valid(t) {
                    return Some((i, t.clone()));
                }
            }
        } else {
            for i in (0..self.ix).rev() {
                let t = &self.entries[i];
                if Some(t) != cur && valid(t) {
                    return Some((i, t.clone()));
                }
            }
        }
        None
    }
}

pub(crate) fn build_buffer_ring(entries: &[Entry], local: &[&Entry]) -> Vec<String> {
    let mut ring = Vec::new();
    let mut seen = HashSet::new();
    let tree = crate::tree::Tree::new(entries);
    for (e, _) in tree.rows(|_| true) {
        if e.meta.folder.is_none() && seen.insert(e.id.clone()) {
            ring.push(e.id.clone());
        }
    }
    for e in local {
        if e.meta.folder.is_none() && seen.insert(e.id.clone()) {
            ring.push(e.id.clone());
        }
    }
    for e in entries {
        if e.meta.folder.is_none() && seen.insert(e.id.clone()) {
            ring.push(e.id.clone());
        }
    }
    ring
}

/// What the main pane shows: the open document, 主页 or 回收站.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Doc,
    Home,
    Folder,
    Trash,
    Settings,
}

/// A confirmation dialog and the pages it is about.
#[derive(Clone, Debug)]
pub enum Confirm {
    /// 删除 (to the 回收站); `batch` for the 批量操作 selection.
    Delete { ids: Vec<String>, batch: bool },
    /// 回收站 → 恢复.
    Restore(String),
    /// 回收站 → 彻底删除.
    Purge(String),
    /// 回收站 → 清空回收站: every trashed page, permanently.
    EmptyTrash,
    /// 隐藏引用列表 (with 不再提示) / 显示引用列表.
    Refs { show: bool, no_ask: bool },
    /// The open document could not be written before it was to be replaced
    /// (another document, 新建, 退出): 重试 / 放弃修改, then carry on.
    SaveFailed,
    /// The library should move to real folders, and the user is asked first
    /// (it is synced, or used from another computer): 改用文件夹 / 稍后.
    Migrate(crate::store::ConsentReason),
    /// 设置 › 文档库 › 更改…: open this folder as the library (#94), after a restart.
    SwitchLibrary(std::path::PathBuf),
    /// Dropped on the sidebar from outside: import it into the library there?
    Import { what: DropImport, target: crate::sidebar_drop::SidebarDropAt },
}

/// What a sidebar drop would import: files and folders from the Finder, or a 本机文件.
#[derive(Clone, Debug)]
pub enum DropImport { Files(Vec<std::path::PathBuf>), Local(String) }

/// What to do once the open document is safely on disk (or its changes given up).
pub type AfterSave = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

#[derive(Default)]
struct ListedDocs { entries: Vec<crate::store::Entry>, trash: Vec<crate::store::TrashEntry>, warnings: Vec<String> }
struct DeletedDocs { deleted: Vec<String>, errors: Vec<String>, changes: Vec<crate::store::FileDelta> }

/// Inline 重命名 of a sidebar row.
pub struct Rename {
    pub id: String,
    pub field: Entity<TextField>,
    _subs: Vec<Subscription>,
}

pub struct Workspace {
    pub quick_note: bool,
    pub shortcut_recording: Option<String>,
    pub shortcut_strokes: Vec<String>,
    /// The failed change: (action, message); an empty action is page-wide.
    pub shortcut_error: Option<(String, String)>,
    pub settings_page: crate::settings::Page,
    /// 版本历史: snapshots kept per document (1–30), cached for the settings page.
    pub history_limit: u32,
    /// 外观's theme dropdown while open: the theme its preview shows.
    pub theme_menu: Option<String>,
    /// Where the theme dropdown's button was drawn last frame.
    pub theme_anchor: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// 外观's font / preset dropdown while open: target and highlighted value.
    pub font_menu: Option<(crate::settings::FontTarget, String)>,
    pub font_anchors: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<crate::settings::FontTarget, Bounds<Pixels>>>>,
    pub doc_font_list: Vec<String>,
    pub mono_font_list: Vec<String>,
    /// 通用's language dropdown while open: the highlighted language.
    pub lang_menu: Option<crate::settings::Lang>,
    pub lang_anchor: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// 通用's other dropdowns (快捷新建, 网页搜索, 打开链接): the open one, and
    /// where each button was drawn last frame.
    pub pref_menu: Option<crate::settings::PrefMenu>,
    pub pref_anchors: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<&'static str, Bounds<Pixels>>>>,
    /// 快捷键's filter field.
    pub settings_filter: Entity<TextField>,
    native_sidebar: crate::native_header::NativeSidebar,
    pub store: SharedStore,
    pub entries: Vec<Entry>,
    pub current: Option<String>,
    pub editor: Option<Entity<Editor>>,
    pub search: Entity<TextField>,
    /// Keyboard highlight inside search results.
    pub search_sel: Option<usize>,
    pub sidebar_open: bool,
    /// Dragged width (300 by default, 268 to min(800, window − 540)).
    pub sidebar_width: f32,
    /// The resize handle is being dragged.
    pub sidebar_drag: bool,
    /// The window's minimum width as last given to it (`DOC_MIN_W` plus the sidebar).
    min_w: f32,
    pub folder_current: Option<String>,
    pub create_menu: Option<(Option<String>, Point<Pixels>)>,
    /// The header's 导入 menu: the folder it imports into, and where it hangs.
    pub import_menu: Option<(Option<String>, Point<Pixels>)>,
    /// The header's … menu on Home (None) or a folder page: where it hangs.
    pub place_menu: Option<(Option<String>, Point<Pixels>)>,
    pub folder_drag: crate::sidebar_drag::SidebarDrag,
    /// The search dialog (command-center style) is open.
    pub search_open: bool,
    pub status: SaveStatus,
    pub row_menu: Option<RowMenu>,
    pub rename: Option<Rename>,
    pub(crate) file_history: crate::file_history::FileHistory,
    /// Last-frame bounds of each row's "…" button, to hang its menu from.
    pub more_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// Header "…" menu and its open submenu.
    pub doc_menu: bool,
    pub doc_sub: Option<DocSub>,
    /// Last-frame bounds of the open submenu (clicks there keep the menu open).
    pub doc_sub_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub doc_info: Option<DocInfo>,
    /// 版本历史 drawer, open for `HistoryView::id`.
    pub history: Option<crate::history::HistoryView>,
    /// A snapshot is on screen: the editor state to put back, and which one.
    pub preview: Option<crate::history::Preview>,
    /// 命名版本: the row's name being typed.
    pub version_rename: Option<crate::history::VersionRename>,
    pub list_scroll: ScrollHandle,
    pub search_scroll: ScrollHandle,
    /// Last-frame bounds of the row menu's 下载 submenu.
    pub row_sub_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// 批量操作 mode: the checked docs of 我的文档, in check order.
    pub batch: Option<Vec<String>>,
    /// The open confirmation dialog.
    pub confirm: Option<Confirm>,
    /// The first-launch pages laid over the window again (⌘⌥⇧O, Settings › About).
    pub onboarding: Option<Entity<crate::onboarding::Onboarding>>,
    pub view: View,
    pub home: crate::home::Home,
    pub settings_scroll: ScrollHandle,
    /// 回收站 rows, refreshed when the page opens or changes.
    pub trash: Vec<crate::store::TrashEntry>,
    /// A 回收站 row's menu: the page and where it opens.
    pub trash_menu: Option<(String, Point<Pixels>, Corner)>,
    /// The toast and when it appeared.
    pub toast: Option<(ToastKind, SharedString, Instant)>,
    toast_task: Option<Task<()>>,
    /// Folded sidebar sections: 置顶, 我的文档.
    pub folded: [bool; 3],
    /// Expanded pages of the 我的文档 tree.
    pub expanded: HashSet<String>,
    /// Where a drag from outside the tree (the Finder, 本机文件) would land in the sidebar.
    pub sidebar_drop: Option<crate::sidebar_drop::SidebarDropAt>,
    pub move_dlg: Option<crate::tree::MoveDialog>,
    /// 导出 Markdown 设置.
    pub export_md: Option<crate::transfer::ExportMd>,
    /// 编辑 / 阅读 dropdown, and the mode button's last-frame bounds.
    pub mode_menu: bool,
    pub mode_btn_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The header "…" button, which the doc menu hangs from.
    pub more_btn_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// The 快捷键 drawer, and the help button's menu (open while hovered).
    pub shortcuts: bool,
    pub pane_focus: PaneFocus,
    pub sidebar_cursor: Option<SidebarItem>,
    pub sidebar_pending_g: bool,
    pub sidebar_pending_ctrl_w: bool,
    pub non_doc_pending: Option<char>,
    pub home_cursor: Option<String>,
    pub trash_scroll: ScrollHandle,
    pub trash_cursor: Option<String>,
    pub settings_nav_cursor: usize,
    pub settings_row_cursor: usize,
    pub doc_history: DocHistory,
    pub(crate) focus: FocusHandle,
    save_task: Option<Task<()>>,
    /// When the oldest edit not yet on disk was made: a save waits for a
    /// pause in typing, but never longer than `SAVE_MAX_DELAY` after it.
    dirty_since: Option<Instant>,
    /// The open document has changes not yet on disk (cleared only by a
    /// successful save or by 放弃修改).
    pub(crate) unsaved: bool,
    /// Held back by a failed save: runs after 重试 succeeds or 放弃修改.
    pub(crate) after_save: Option<AfterSave>,
    pub(crate) editor_sub: Option<Subscription>,
    _subs: Vec<Subscription>,
}


/// A macOS notification (only logged when scripted).
fn notify_system(title: &str, body: &str) {
    if crate::devtools::scripted() {
        return eprintln!("notification: {title} / {body}");
    }
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("display notification \"{}\" with title \"{}\"", q(body), q(title));
    std::thread::spawn(move || std::process::Command::new("osascript").arg("-e").arg(script).status());
}

impl Workspace {
    /// Other active windows take the lists a background write left; `trash`
    /// only when that write touched it.
    pub(crate) fn sync_peers(&self, cx: &mut Context<Self>, entries: &[Entry], trash: Option<&[crate::store::TrashEntry]>) {
        let me = cx.entity_id();
        for handle in cx.windows().into_iter().filter_map(|w| w.downcast::<Workspace>()) {
            if !handle.is_active(cx).is_some_and(|yes| yes) || handle.entity(cx).is_ok_and(|e| e.entity_id() == me) { continue }
            let (entries, trash) = (entries.to_vec(), trash.map(<[_]>::to_vec));
            let _ = handle.update(cx, |ws, _, cx| {
                ws.entries = entries; if let Some(trash) = trash { ws.trash = trash; } ws.sort();
                cx.notify();
            });
        }
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_window(false, window, cx)
    }

    pub fn new_window(quick: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build_window(quick, None, window, cx)
    }

    pub fn new_document_window(document: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build_window(true, document, window, cx)
    }

    fn build_window(quick: bool, document: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::settings::init_available_fonts(|| cx.text_system().all_font_names());
        crate::settings::load();
        if !cx.has_global::<SharedStore>() { cx.set_global(SharedStore::open().expect("open document library")); }
        let mut store = cx.global::<SharedStore>().clone();
        let entries = store.list();
        let search = cx.new(|cx| {
            let mut f = TextField::new("搜索", cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let settings_filter = cx.new(|cx| {
            let mut f = TextField::new(crate::settings::tr("搜索快捷键", "Search shortcuts"), cx);
            f.placeholder_color = Theme::text_disabled();
            f
        });
        let subs = vec![
            cx.subscribe_in(&settings_filter, window, |this, _, ev: &TextFieldEvent, window, cx| {
                match ev {
                    TextFieldEvent::Changed => { this.settings_row_cursor = 0; }
                    TextFieldEvent::Cancel | TextFieldEvent::Submit | TextFieldEvent::Down => {
                        this.settings_row_cursor = 0;
                        window.focus(&this.focus);
                    }
                    _ => {}
                }
                cx.notify();
            }),
            cx.observe_window_bounds(window, |this, window, cx| crate::quick_note::remember(window, this.quick_note, cx)),
            // The system switched between light and dark (or the app's own look changed).
            cx.observe_window_appearance(window, |this, _, cx| if crate::settings::sync_dark(cx) { crate::settings::refresh_all(this, cx); }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    if let Some(id) = &this.current { crate::quick_note::used(id, cx); }
                    if !this.store.file_task_busy() { this.entries = this.store.list(); this.sort(); }
                    crate::update::check_if_stale(cx); cx.notify();
                }
                else if !this.store.file_task_busy() { this.cancel_home_drag(cx); this.end_folder_drag(true, cx); this.cancel_shortcut_recording(cx); this.flush(cx); }
            }),
            cx.subscribe_in(&search, window, |this, _, ev: &TextFieldEvent, window, cx| {
                match ev {
                    TextFieldEvent::Changed => {
                        this.search_sel = this.default_search_sel(cx);
                        this.search_scroll.set_offset(point(px(0.), px(0.)));
                    }
                    TextFieldEvent::Up => this.move_search_sel(-1, cx),
                    TextFieldEvent::Down => this.move_search_sel(1, cx),
                    TextFieldEvent::Submit => {
                        if let Some(ix) = this.search_sel {
                            this.pick_search(ix, window, cx)
                        }
                    }
                    TextFieldEvent::Cancel => this.close_search(window, cx),
                    _ => {}
                }
                cx.notify();
            }),
            // Last resort (the system logging out): ⌘Q and the close button
            // go through `save_before` and ask when the save fails.
            cx.on_app_quit(|this, cx| {
                crate::quick_note::persist(cx);
                if !this.store.file_task_busy() && !this.flush(cx) && let Some(ed) = &this.editor
                    && let Err(error) = this.store.recover_later(&ed.read(cx).core.doc) {
                    crate::logging::write(&format!("Could not preserve unsaved document: {error:#}"));
                }
                async {}
            }),
        ];
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            crate::quick_note::persist(cx);
            this.update(cx, |w, cx| { w.cancel_shortcut_recording(cx); w.save_before(cx, |_, window, _| window.remove_window()) }).unwrap_or(true)
        });
        if !quick { crate::settings::watch_themes(cx); }
        // A file written into the library from outside (a model writing a
        // note, say) joins it without going away and back: the library's
        // shape is compared every couple of seconds and the list re-read only
        // when it changed.
        cx.spawn(async move |this, cx| {
            let bg = cx.background_executor().clone();
            let shape = || bg.spawn(async { crate::store::Store::shape_at(&crate::store::Store::root_dir()) });
            let mut last = shape().await;
            loop {
                bg.timer(Duration::from_secs(2)).await;
                let now = shape().await;
                if now == last { continue }
                let reloaded = this.update(cx, |w, cx| {
                    // Not mid-rename: the row under the field would be rebuilt.
                    if w.rename.is_some() || w.move_dlg.is_some() || w.store.file_task_busy() { return false }
                    w.entries = w.store.list();
                    w.sort();
                    cx.notify();
                    true
                });
                match reloaded { Ok(true) => last = now, Ok(false) => {}, Err(_) => break }
            }
        })
        .detach();
        // 日期提醒: post what fell due since the last look (not what was
        // missed while the app was closed).
        cx.spawn(async |this, cx| {
            let mut last = crate::time::now_ms() as i64;
            loop {
                cx.background_executor().timer(Duration::from_secs(15)).await;
                let now = crate::time::now_ms() as i64;
                if this.update(cx, |w, cx| w.check_reminders(last, now, cx)).is_err() {
                    break;
                }
                last = now;
            }
        })
        .detach();
        let settings = store.settings();
        let sidebar_width = settings.sidebar_width.unwrap_or(theme::SIDEBAR_WIDTH);
        let history_limit = settings.history_limit();
        let mut ws = Workspace {
            quick_note: quick,
            shortcut_recording: None,
            shortcut_strokes: vec![],
            shortcut_error: None,
            settings_page: Default::default(),
            history_limit,
            theme_menu: None,
            theme_anchor: Default::default(),
            font_menu: None,
            font_anchors: Default::default(),
            doc_font_list: vec![],
            mono_font_list: vec![],
            lang_menu: None,
            lang_anchor: Default::default(),
            pref_menu: None,
            pref_anchors: Default::default(),
            settings_filter,
            store,
            entries,
            current: None,
            editor: None,
            search,
            search_sel: None,
            sidebar_open: !quick,
            sidebar_width,
            min_w: 0.,
            sidebar_drag: false,
            folder_current: None,
            create_menu: None,
            import_menu: None,
            place_menu: None,
            folder_drag: Default::default(),
            search_open: false,
            status: SaveStatus::Idle,
            row_menu: None,
            rename: None,
            file_history: Default::default(),
            more_bounds: Default::default(),
            doc_menu: false,
            doc_sub: None,
            doc_sub_bounds: Rc::new(Cell::new(None)),
            doc_info: None,
            history: None,
            preview: None,
            version_rename: None,
            list_scroll: ScrollHandle::new(),
            search_scroll: ScrollHandle::new(),
            row_sub_bounds: Rc::new(Cell::new(None)),
            batch: None,
            confirm: None,
            onboarding: None,
            view: View::Doc,
            home: Default::default(),
            settings_scroll: ScrollHandle::new(),
            trash: vec![],
            trash_menu: None,
            toast: None,
            toast_task: None,
            mode_menu: false,
            mode_btn_bounds: Rc::new(Cell::new(None)),
            more_btn_bounds: Rc::new(Cell::new(None)),
            native_sidebar: Default::default(),
            shortcuts: false,
            folded: [false; 3],
            expanded: HashSet::new(),
            sidebar_drop: None,
            move_dlg: None,
            export_md: None,
            pane_focus: PaneFocus::Main,
            sidebar_cursor: None,
            sidebar_pending_g: false,
            sidebar_pending_ctrl_w: false,
            non_doc_pending: None,
            home_cursor: None,
            trash_scroll: ScrollHandle::new(),
            trash_cursor: None,
            settings_nav_cursor: 0,
            settings_row_cursor: 0,
            doc_history: DocHistory::default(),
            focus: cx.focus_handle(),
            save_task: None,
            dirty_since: None,
            unsaved: false,
            after_save: None,
            editor_sub: None,
            _subs: subs,
        };
        ws.sort();
        ws.load_home();
        // An empty library opens on 主页's empty state; only a quick note
        // window starts a document of its own.
        match document.or_else(|| (!quick).then(|| ws.entries.first().map(|e| e.id.clone())).flatten()) {
            Some(id) => ws.open(&id, window, cx),
            None if quick => ws.new_doc(window, cx),
            None => {
                ws.view = View::Home;
                ws.doc_history.record(NavTarget::Home);
            }
        }
        // Kept in the top bar (not just a toast): the user should notice.
        let warnings = ws.store.take_warnings();
        if !warnings.is_empty() {
            let msg = warnings.iter().map(|s| crate::settings::diagnostic(s)).collect::<Vec<_>>().join(crate::settings::tr("；", "; "));
            ws.show_toast_kind(ToastKind::Error, &msg, cx);
            ws.status = SaveStatus::Failed(msg);
        }
        ws
    }

    /// Most recently modified first.
    pub(crate) fn sort(&mut self) {
        self.reapply_pending_tree_moves();
        self.entries.sort_by(|a, b| b.meta.modified.cmp(&a.meta.modified).then_with(|| a.id.cmp(&b.id)));
    }

    pub fn query(&self, cx: &App) -> String {
        self.search.read(cx).text.trim().to_lowercase()
    }

    pub fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.try_open(id, window, cx);
    }

    /// Show `id`; false when it could not be opened (or the open document
    /// could not be saved first, see [`Self::save_before`]).
    pub fn try_open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // A snapshot is on screen: back to the live document before anything
        // else touches the editor.
        if self.history.is_some() { self.close_history(cx); }
        // 阅读位置: remember where the document being left was, before the
        // editor is swapped out.
        self.remember_scroll(cx);
        if self.entries.iter().any(|e| e.id == id && e.meta.folder.is_some()) { return self.open_folder(id, window, cx); }
        if self.view != View::Doc {
            self.view = View::Doc;
            cx.notify();
        }
        if self.current.as_deref() == Some(id) {
            return true;
        }
        let target = id.to_string();
        if !self.save_before(cx, move |w, window, cx| {
            w.open(&target, window, cx);
        }) {
            return false;
        }
        let shared = !self.document_peers(id, window, cx).is_empty();
        let doc = match self.store.load_for_edit(id, shared) {
            Ok(d) => d,
            Err(e) => {
                self.show_toast_kind(ToastKind::Error, &crate::localized_format!("无法打开文档：{}", "Could not open document: {}", crate::settings::diagnostic(&e.to_string())), cx);
                cx.notify();
                return false;
            }
        };
        let meta = self.entries.iter().find(|e| e.id == id).map(|e| e.meta.clone());
        let modified = meta.as_ref().map_or_else(crate::time::now, |m| m.modified);
        let width = meta.as_ref().map(|m| m.width).unwrap_or_default();
        let pinned = meta.as_ref().is_some_and(|m| !m.catalogue_closed);
        let editor = cx.new(|cx| {
            let mut e = Editor::new(doc, modified, width, window, cx);
            e.cat_pinned = pinned;
            e.markdown = meta.as_ref().is_some_and(|m| m.markdown);
            e.board_file = meta.as_ref().is_some_and(|m| m.kind == crate::store::DocKind::Board);
            if let Some(m) = &meta {
                e.created = m.created;
                e.info = m.info;
                if !e.board_file { e.set_read_only(m.reading, cx); }
            }
            e
        });
        // Reuse the live state, including unsaved edits and undo history.
        if let Some(peer) = self.document_peers(id, window, cx).first() {
            let core = peer.read(cx).core.clone();
            editor.update(cx, |e, cx| e.receive_document(&core, cx));
        }
        // 阅读位置: back to where this document was left, on its first frame.
        let top = self.store.scroll_top(id);
        editor.update(cx, |e, _| e.restore_top = top);
        self.editor_sub = Some(cx.subscribe_in(&editor, window, |this, _, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Changed => {
                this.file_history.prefer_files = false;
                // 阅读模式 put the document back: nothing to save.
                let rejected = this.editor.clone().is_some_and(|ed| ed.update(cx, |e, cx| e.reject_read_only_edit(cx)));
                if !rejected {
                    this.sync_document(window, cx);
                    this.schedule_save(window, cx);
                }
            }
            EditorEvent::Mode(reading) => this.set_mode(*reading, window, cx),
            EditorEvent::Info(info) => this.set_info_row(*info),
            EditorEvent::Toast => {
                this.toast = None;
                cx.notify();
            }
            EditorEvent::Save => {
                this.flush(cx);
            }
            EditorEvent::Search(q) => {
                this.open_search(window, cx);
                this.search.update(cx, |s, cx| s.set_text(q, cx));
                this.search_sel = this.default_search_sel(cx);
            }
            EditorEvent::OpenDoc(id) => this.open(id, window, cx),
            EditorEvent::OpenDocAt(id, ordinal) => this.open_at(id, *ordinal, window, cx),
            EditorEvent::HideRefs => this.ask_refs(false, cx),
            EditorEvent::HistoryEscape => {
                // Arrives from inside the editor's own update: leave the
                // preview (then the drawer) once that finishes.
                let this = cx.entity();
                cx.defer(move |cx| {
                    this.update(cx, |w, cx| {
                        if w.preview.is_some() { w.exit_preview(cx) } else { w.close_history(cx) }
                    });
                });
            }
            EditorEvent::Duplicate => {
                if let Some(id) = this.current.clone() {
                    this.duplicate_doc(&id, window, cx);
                }
            }
            EditorEvent::Leave => cx.defer_in(window, |this, window, cx| this.leave_board(window, cx)),
            EditorEvent::Catalogue(pinned) => {
                if let Some(id) = this.current.clone() {
                    if let Some(e) = this.entries.iter_mut().find(|e| e.id == id) {
                        e.meta.catalogue_closed = !pinned;
                    }
                    let _ = this.store.set_catalogue(&id, *pinned);
                }
            }
            EditorEvent::VimFocusPane(pane) => this.vim_focus_pane(*pane, window, cx),
            EditorEvent::VimJumpHistory { forward } => this.jump_doc_history(*forward, window, cx),
            EditorEvent::VimCycleBuffer { forward } => this.cycle_buffer(*forward, window, cx),
            EditorEvent::VimOpenHome => {
                this.show_view(View::Home, cx);
                this.vim_focus_pane(fdl_core::editor::vim::VimPane::Right, window, cx);
            }
            EditorEvent::VimOpenHistory => {
                if let Some(id) = this.current.clone() {
                    this.open_history(&id, window, cx);
                }
            }
            EditorEvent::VimToggleShortcuts => {
                this.shortcuts = !this.shortcuts;
                cx.notify();
            }
        }));
        // Opening a document puts the caret in it, unless the user is typing a search.
        if !self.search_open {
            self.pane_focus = PaneFocus::Main;
            editor.read(cx).focus(window);
        }
        // A 画板 opens straight into its board, over the whole window.
        editor.update(cx, |e, cx| e.open_board_file(window, cx));
        if let Some(m) = self.store.touch(id)
            && let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                e.meta.opened = m.opened;
            }
        if let Some(previous) = &self.current
            && self.document_peers(previous, window, cx).is_empty() { self.store.forget_loaded(previous); }
        self.editor = Some(editor);
        self.current = Some(id.to_string());
        self.doc_history.record(NavTarget::Doc(id.to_string()));
        // Web images pasted earlier whose download landed after it closed (#220).
        if let Some(e) = self.editor.clone() { e.update(cx, |e, cx| e.adopt_remote(cx)); }
        crate::quick_note::used(id, cx);
        self.reveal_in_tree(id);
        self.sync_sub_pages(cx);
        self.status = SaveStatus::Idle;
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        self.mode_menu = false;
        cx.notify();
        true
    }

    /// ⌘N / the 文件 menu: create the 快捷新建 type (a fixed one, or the last
    /// one made from the 新建 menu) in the current place.
    pub fn new_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let parent = self.creation_parent();
        self.create_doc(crate::settings::quick_new_markdown(), parent, window, cx);
    }

    /// Ask before 删除: a confirmation, then the 回收站.
    pub fn ask_delete(&mut self, id: &str, cx: &mut Context<Self>) {
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        self.confirm = Some(Confirm::Delete { ids: vec![id.to_string()], batch: false });
        cx.notify();
    }

    /// Move documents to the trash; if the open one goes, open the next.
    pub fn delete_docs(&mut self, ids: &[String], window: &mut Window, cx: &mut Context<Self>) {
        if self.store.file_task_busy() || self.file_history.busy || self.tree_writes_pending() { return; }
        // A page goes with its child pages: each topmost page is
        // trashed as one item that carries its subtree.
        let tree = crate::tree::Tree::new(&self.entries);
        let mut all: Vec<(String, Option<String>)> = vec![];
        for id in ids {
            let covered = self.ancestors(id).iter().any(|a| ids.contains(&a.id));
            if covered {
                continue;
            }
            for d in tree.subtree(id) {
                if !all.iter().any(|(x, _)| *x == d) {
                    let root = (d != *id).then(|| id.clone());
                    all.push((d, root));
                }
            }
        }
        let groups = all;
        let all: Vec<String> = groups.iter().map(|(d, _)| d.clone()).collect();
        for handle in cx.windows().into_iter().filter(|w| w.window_id() != window.window_handle().window_id()) {
            if let Some(handle) = handle.downcast::<Workspace>()
                && handle.update(cx, |ws, _, _| ws.current.as_ref().is_some_and(|id| all.contains(id))).unwrap_or(false)
            {
                self.show_toast_kind(ToastKind::Error, crate::settings::text("请先关闭正在编辑这些文档的其他窗口，再删除"), cx);
                return;
            }
        }
        if self.current.as_ref().is_some_and(|c| all.contains(c)) {
            let retry = ids.to_vec();
            if !self.save_before(cx, move |w, window, cx| w.delete_docs(&retry, window, cx)) {
                return;
            }
        }
        let Some(task_guard) = self.store.begin_file_task() else { return };
        let mut writer = self.store.tree_writer();
        let task = cx.background_executor().spawn(async move {
            let journal = writer.track_file_changes();
            let mut deleted = vec![];
            let mut errors = vec![];
            let batch = writer.begin_batch().is_ok();
            for (id, root) in &groups {
                if root.as_ref().is_some_and(|r| !deleted.contains(r)) { continue; }
                match writer.delete(id, root.as_deref()) {
                    Ok(()) => deleted.push(id.clone()),
                    Err(e) => errors.push(crate::localized_format!("删除失败：{}", "Could not delete document: {}", crate::settings::diagnostic(&e.to_string()))),
                }
            }
            if batch && let Err(e) = writer.end_batch() {
                errors.push(crate::localized_format!("删除失败：{}", "Could not delete document: {}", crate::settings::diagnostic(&e.to_string())));
            }
            let warnings = writer.take_warnings();
            let listed = ListedDocs { entries: writer.list(), trash: writer.list_trash(), warnings };
            (DeletedDocs { deleted, errors, changes: journal.take() }, listed)
        });
        self.batch = None;
        self.row_menu = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let _task_guard = task_guard;
            let (done, listed) = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                if w.current.as_ref().is_some_and(|c| done.deleted.contains(c)) {
                    w.save_task = None;
                    w.unsaved = false;
                    w.current = None;
                    w.editor = None;
                    w.editor_sub = None;
                }
                w.sync_peers(cx, &listed.entries, Some(&listed.trash));
                w.entries = listed.entries; w.trash = listed.trash;
                if w.folder_current.as_ref().is_some_and(|id| done.deleted.contains(id)) {
                    w.folder_current = None;
                    if w.view == View::Folder { w.view = View::Home; }
                }
                w.row_menu = None;
                if w.current.is_none() && w.view != View::Folder {
                    let view = w.view;
                    match w.entries.first().map(|e| e.id.clone()) {
                        Some(next) => { w.open(&next, window, cx); w.view = view; }
                        None => w.view = if view == View::Doc { View::Home } else { view },
                    }
                }
                w.remember_file_changes(done.changes, crate::settings::tr("删除", "Delete"));
                w.sync_sub_pages(cx);
                let mut errors = done.errors;
                errors.extend(listed.warnings.iter().map(|s| crate::settings::diagnostic(s)));
                if errors.is_empty() { w.show_toast(crate::settings::text("删除成功"), cx); }
                else {
                    let msg = errors.join("；");
                    w.show_toast_kind(ToastKind::Error, &msg, cx);
                    w.status = SaveStatus::Failed(msg);
                }
                cx.notify();
            });
        }).detach();
    }

    pub fn toggle_pin(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut on = false;
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.meta.pinned = !e.meta.pinned;
            on = e.meta.pinned;
            let _ = self.store.set_pinned(id, e.meta.pinned);
        }
        self.sort();
        self.row_menu = None;
        self.show_toast(if on { crate::settings::text("已添加到“置顶”") } else { crate::settings::text("已从“置顶”移除") }, cx);
        cx.notify();
    }

    /// 收藏 / 取消收藏 (listed under 主页 → 收藏).
    pub fn toggle_star(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(e) = self.entries.iter_mut().find(|e| e.id == id) else { return };
        let on = e.meta.starred == 0;
        if let Ok(t) = self.store.set_starred(id, on) {
            e.meta.starred = t;
        }
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        self.show_toast(if on { crate::settings::text("已添加到“收藏”") } else { crate::settings::text("已从“收藏”中移除") }, cx);
        cx.notify();
    }

    /// 退出 on a standalone 画板: save it, close it, and show the folder it
    /// is in (主页 at the top).
    fn leave_board(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush(cx) { return; }
        let Some(id) = self.current.take() else { return };
        self.editor = None;
        self.editor_sub = None;
        if self.document_peers(&id, window, cx).is_empty() { self.store.forget_loaded(&id); }
        match self.entries.iter().find(|e| e.id == id).and_then(|e| e.meta.parent.clone()) {
            Some(p) => { self.open_folder(&p, window, cx); }
            None => self.show_view(View::Home, cx),
        }
        cx.notify();
    }

    /// Switch the main pane to 主页 or 回收站 (or back to the document).
    pub fn show_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.cancel_shortcut_recording(cx);
        self.close_history(cx);
        if !matches!(self.view, View::Settings | View::Trash) && matches!(view, View::Settings | View::Trash) {
            self.pane_focus = PaneFocus::Main;
        }
        // 阅读位置: leaving the document with ⌘H or 回收站 also counts.
        if view != View::Doc { self.remember_scroll(cx) }
        if view == View::Trash {
            self.trash = self.store.list_trash();
            self.settings_nav_cursor = 4;
        } else if view == View::Settings {
            self.settings_nav_cursor = crate::settings::nav_index(self.settings_page);
        }
        self.view = view;
        let target = match view {
            View::Doc => self.current.as_ref().map(|id| NavTarget::Doc(id.clone())),
            View::Home => Some(NavTarget::Home),
            View::Folder => self.folder_current.as_ref().map(|id| NavTarget::Folder(id.clone())),
            View::Trash => Some(NavTarget::Trash),
            View::Settings => Some(NavTarget::Settings(self.settings_page)),
        };
        if let Some(t) = target {
            self.doc_history.record(t);
        }
        self.non_doc_pending = None;
        self.row_menu = None;
        self.trash_menu = None;
        self.theme_menu = None;
        self.font_menu = None;
        self.lang_menu = None;
        self.pref_menu = None;
        self.home.sort_menu = false;
        cx.notify();
    }

    pub fn buffer_ring(&self) -> Vec<String> {
        let local = self.local_entries();
        build_buffer_ring(&self.entries, &local)
    }

    pub fn cycle_buffer(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let ring = self.buffer_ring();
        if ring.is_empty() {
            return;
        }
        let target = if self.view == View::Doc
            && let Some(cur) = self.current.as_deref()
            && let Some(ix) = ring.iter().position(|id| id == cur)
        {
            if ring.len() == 1 {
                return;
            }
            let next_ix = if forward { (ix + 1) % ring.len() } else { (ix + ring.len() - 1) % ring.len() };
            ring[next_ix].clone()
        } else if self.view == View::Folder
            && let Some(fid) = self.folder_current.as_deref()
        {
            let sub = crate::tree::Tree::new(&self.entries).subtree(fid);
            let in_folder: Vec<&String> = ring.iter().filter(|id| sub.contains(id)).collect();
            if let Some(id) = if forward { in_folder.first() } else { in_folder.last() } {
                (*id).clone()
            } else if let Some(cur) = self.current.as_ref().filter(|id| ring.contains(id)) {
                cur.clone()
            } else if forward {
                ring[0].clone()
            } else {
                ring[ring.len() - 1].clone()
            }
        } else if let Some(cur) = self.current.as_ref().filter(|id| ring.contains(id)) {
            cur.clone()
        } else if forward {
            ring[0].clone()
        } else {
            ring[ring.len() - 1].clone()
        };
        if self.try_open(&target, window, cx) {
            self.pane_focus = PaneFocus::Main;
            if let Some(ed) = &self.editor {
                ed.read(cx).focus(window);
            }
        }
    }

    pub fn jump_doc_history(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((next_ix, target)) = self.doc_history.step(forward, |t| match t {
            NavTarget::Doc(id) => self.entries.iter().any(|e| e.id == *id && e.meta.folder.is_none()),
            NavTarget::Folder(id) => self.entries.iter().any(|e| e.id == *id && e.meta.folder.is_some()),
            NavTarget::Home | NavTarget::Trash | NavTarget::Settings(_) => true,
        }) {
            self.doc_history.navigating = true;
            let ok = match target {
                NavTarget::Doc(id) => self.try_open(&id, window, cx),
                NavTarget::Folder(id) => self.open_folder(&id, window, cx),
                NavTarget::Home => { self.show_view(View::Home, cx); true }
                NavTarget::Trash => { self.show_view(View::Trash, cx); true }
                NavTarget::Settings(page) => { self.open_settings(page, cx); true }
            };
            self.doc_history.navigating = false;
            if ok {
                self.doc_history.ix = next_ix;
                self.pane_focus = PaneFocus::Main;
                if self.view == View::Doc && let Some(ed) = &self.editor {
                    ed.read(cx).focus(window);
                } else {
                    window.focus(&self.focus);
                }
            }
        } else if !forward && self.view != View::Doc && let Some(cur) = self.current.clone() {
            self.doc_history.navigating = true;
            let ok = self.try_open(&cur, window, cx);
            self.doc_history.navigating = false;
            if ok {
                self.pane_focus = PaneFocus::Main;
                if let Some(ed) = &self.editor {
                    ed.read(cx).focus(window);
                }
            }
        }
    }

    fn handle_non_doc_vim_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        use fdl_core::editor::vim::VimPane;
        if self.settings_filter.read(cx).is_focused(window) {
            return false;
        }
        let ks = &ev.keystroke;
        if ks.modifiers.platform || ks.modifiers.alt || ks.modifiers.function {
            return false;
        }
        let ch = ks.key_char.as_deref().and_then(|s| s.chars().next()).filter(|_| ks.key_char.as_ref().is_some_and(|s| s.chars().count() == 1));
        if self.non_doc_pending == Some('?') {
            self.non_doc_pending = None;
            cx.notify();
            if ks.key == "escape" || (!ks.modifiers.control && ch == Some('?')) {
                return true;
            }
        }
        if let Some(prefix) = self.non_doc_pending.filter(|p| matches!(p, 'w' | '[' | ']' | ' ')) {
            self.non_doc_pending = None;
            match prefix {
                'w' => match ks.key.as_str() {
                    "h" => { self.vim_focus_pane(VimPane::Left, window, cx); return true; }
                    "l" => { self.vim_focus_pane(VimPane::Right, window, cx); return true; }
                    "w" => { self.vim_focus_pane(VimPane::Cycle, window, cx); return true; }
                    _ => { cx.notify(); return true; }
                },
                '[' if !ks.modifiers.control && ks.key == "b" => {
                    self.cycle_buffer(false, window, cx);
                    return true;
                }
                ']' if !ks.modifiers.control && ks.key == "b" => {
                    self.cycle_buffer(true, window, cx);
                    return true;
                }
                ' ' if !ks.modifiers.control => {
                    if matches!(ch, Some('b' | ' ')) || matches!(ks.key.as_str(), "b" | "space") {
                        self.open_search(window, cx);
                    } else if matches!(ch, Some('e')) || ks.key == "e" {
                        self.vim_focus_pane(VimPane::Left, window, cx);
                    } else if ch == Some('h') || (ks.key == "h" && !ks.modifiers.shift) {
                        self.show_view(View::Home, cx);
                        self.vim_focus_pane(VimPane::Right, window, cx);
                    } else if (ch == Some('H') || (ks.key == "h" && ks.modifiers.shift)) && self.current.is_some() {
                        if let Some(id) = self.current.clone() {
                            self.open_history(&id, window, cx);
                        }
                    } else if matches!(ch, Some('?' | '/')) || matches!(ks.key.as_str(), "?" | "/") {
                        self.shortcuts = !self.shortcuts;
                        cx.notify();
                    } else {
                        cx.notify();
                    }
                    return true;
                }
                _ => { cx.notify(); return true; }
            }
        }
        if ks.modifiers.control {
            return match ks.key.as_str() {
                "w" => { self.non_doc_pending = Some('w'); cx.notify(); true }
                "o" => { self.jump_doc_history(false, window, cx); true }
                "i" => { self.jump_doc_history(true, window, cx); true }
                _ => false,
            };
        }
        let handled = match self.view {
            View::Home | View::Folder => self.handle_home_vim_key(ev, window, cx),
            View::Trash => self.handle_trash_vim_key(ev, window, cx),
            View::Settings => self.handle_settings_vim_key(ev, window, cx),
            View::Doc => false,
        };
        if handled {
            return true;
        }
        if matches!(ch, Some('[' | ']' | ' ' | '?')) || ks.key == "space" {
            self.non_doc_pending = Some(ch.unwrap_or(' '));
            cx.notify();
            return true;
        }
        false
    }

    /// A success toast (删除成功, 移动成功 …).
    pub fn show_toast(&mut self, msg: &str, cx: &mut Context<Self>) {
        self.show_toast_kind(ToastKind::Success, msg, cx);
    }

    pub fn show_toast_kind(&mut self, kind: ToastKind, msg: &str, cx: &mut Context<Self>) {
        self.toast = Some((kind, SharedString::from(msg.to_string()), Instant::now()));
        if let Some(ed) = &self.editor {
            ed.update(cx, |e, cx| e.clear_toast(cx));
        }
        self.toast_task = Some(cx.spawn(async |this, cx| {
            cx.background_executor().timer(Duration::from_millis(2700)).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
        cx.notify();
    }

    fn render_toast(&self, window: &mut Window) -> Option<AnyElement> {
        let (kind, msg, t0) = self.toast.as_ref()?;
        crate::ui::toast_el(*kind, msg, *t0, window)
    }

    /// 重命名: the row's title turns into an input; Enter or clicking away saves, Esc cancels.
    pub fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.entries.iter().find(|e| e.id == id).map(|e| e.title.trim().to_string()) else { return };
        let field = cx.new(|cx| {
            let mut f = TextField::new(crate::model::UNTITLED, cx);
            f.set_text(&title, cx);
            f.select_all_text(cx);
            f
        });
        let focus = field.focus_handle(cx);
        window.focus(&focus);
        let subs = vec![
            cx.subscribe_in(&field, window, |w, _, ev: &TextFieldEvent, window, cx| match ev {
                TextFieldEvent::Submit => w.finish_rename(true, window, cx),
                TextFieldEvent::Cancel => w.finish_rename(false, window, cx),
                _ => {}
            }),
            cx.on_blur(&focus, window, |w, window, cx| w.finish_rename(true, window, cx)),
        ];
        self.rename = Some(Rename { id: id.to_string(), field, _subs: subs });
        // From 主页 the row may be hidden: show it in the sidebar.
        self.sidebar_open = true;
        self.reveal_in_tree(id);
        self.row_menu = None;
        cx.notify();
    }

    fn finish_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rename.take() else { return };
        let refocus = r.field.read(cx).is_focused(window);
        let title = r.field.read(cx).text.trim().to_string();
        let old = self.entries.iter().find(|e| e.id == r.id).map(|e| e.title.trim().to_string());
        if commit && old.is_some_and(|o| o != title) {
            if let Some(peer) = self.document_peers(&r.id, window, cx).first() {
                peer.update(cx, |e, cx| e.set_title(&title, cx));
            } else {
                if !self.flush(cx) { return; }
                let file_before = self.store.track_file_changes();
                match self.store.rename(&r.id, &title) {
                    Ok(meta) => {
                        if let Some(e) = self.entries.iter_mut().find(|e| e.id == r.id) {
                            e.title = title.clone();
                            e.meta = meta;
                        }
                        self.sort();
                        if self.current.as_deref() == Some(r.id.as_str()) && let Some(ed) = &self.editor {
                            ed.update(cx, |e, cx| e.receive_file_title(&title, cx));
                        }
                        self.remember_files(file_before, crate::settings::tr("重命名", "Rename"));
                    }
                    Err(e) => self.status = SaveStatus::Failed(crate::localized_format!("重命名失败：{}", "Could not rename document: {}", crate::settings::diagnostic(&e.to_string()))),
                }
            }
        }
        if refocus
            && let Some(ed) = &self.editor {
                ed.read(cx).focus(window);
            }
        self.sync_sub_pages(cx);
        cx.notify();
    }

    pub(crate) fn document_peers(&self, id: &str, window: &Window, cx: &mut Context<Self>) -> Vec<Entity<Editor>> {
        cx.windows().into_iter().filter(|w| w.window_id() != window.window_handle().window_id())
            .filter_map(|w| w.downcast::<Workspace>())
            .filter_map(|w| w.update(cx, |ws, _, _| {
                (ws.current.as_deref() == Some(id)).then(|| ws.editor.clone()).flatten()
            }).ok().flatten()).collect()
    }

    /// 阅读位置: hand the open document's scroll offset to the library, so
    /// coming back to it lands where it was left.
    fn remember_scroll(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone()) else { return };
        let top = editor.read(cx).scroll_top();
        let _ = self.store.remember_scroll(&id, top);
    }

    pub(crate) fn sync_document(&mut self, window: &Window, cx: &mut Context<Self>) {
        // The previewed snapshot is not the live document: never hand it to a
        // sibling window.
        if self.preview.is_some() { return }
        let (Some(id), Some(editor)) = (self.current.clone(), self.editor.clone()) else { return };
        let peers = self.document_peers(&id, window, cx);
        if peers.is_empty() { return; }
        let core = editor.read(cx).core.clone();
        for peer in peers { peer.update(cx, |e, cx| e.receive_document(&core, cx)); }
        for handle in cx.windows().into_iter().filter(|w| w.window_id() != window.window_handle().window_id()).filter_map(|w| w.downcast::<Workspace>()) {
            let _ = handle.update(cx, |ws, _, cx| {
                if let Some(entry) = ws.entries.iter_mut().find(|e| e.id == id) {
                    entry.title = core.doc.title.text.clone();
                    entry.icon = core.doc.icon.clone();
                }
                // Every remaining view can save the latest state if its sibling closes.
                if ws.current.as_deref() == Some(&id) { ws.unsaved = true; }
                cx.notify();
            });
        }
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview.is_some() { return }
        self.status = SaveStatus::Saving;
        if !self.unsaved || self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
        self.unsaved = true;
        // Each edit replaces the timer (dropping a Task cancels it), so steady
        // typing would put the save off for as long as it lasts: the deadline
        // caps that at SAVE_MAX_DELAY after the oldest unsaved edit.
        let left = SAVE_MAX_DELAY.saturating_sub(self.dirty_since.map_or(Duration::ZERO, |t| t.elapsed()));
        let delay = SAVE_DELAY.min(left);
        // Keep the list title live while typing; the body/snippet updates on save.
        if let (Some(id), Some(ed)) = (&self.current, &self.editor) {
            let title = ed.read(cx).core.doc.title.text.clone();
            if let Some(e) = self.entries.iter_mut().find(|e| &e.id == id) {
                e.title = title;
            }
        }
        self.save_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            while this.update(cx, |w, _| w.store.file_task_busy()).unwrap_or(false) {
                cx.background_executor().timer(Duration::from_millis(50)).await;
            }
            let _ = this.update(cx, |w, cx| w.flush(cx));
        }));
        cx.notify();
    }

    fn check_reminders(&mut self, from: i64, to: i64, cx: &mut Context<Self>) {
        // The open document's reminders are read live (unsaved edits too).
        let open = self.editor.as_ref().map(|e| e.read(cx).core.doc.reminders());
        for e in &self.entries {
            let rs = match &open {
                Some(rs) if self.current.as_deref() == Some(e.id.as_str()) => rs,
                _ => &e.reminders,
            };
            for r in rs.iter().filter(|r| r.should_notify && from < r.notify && r.notify <= to) {
                notify_system(&display_title(&e.title), &crate::editor::reminder::notification(r));
            }
        }
    }

    /// Write the open document to disk if it has unsaved changes. False when
    /// that failed: the changes stay pending (a later save retries them) and
    /// the top bar shows the error.
    pub fn flush(&mut self, cx: &mut Context<Self>) -> bool {
        self.save_task = None;
        // 版本历史 preview: the editor holds an older snapshot, so writing it
        // would overwrite the live document with history.
        if self.preview.is_some() { return true }
        if !self.unsaved {
            return true;
        }
        let (Some(id), Some(ed)) = (self.current.clone(), self.editor.clone()) else {
            self.unsaved = false;
            return true;
        };
        let doc = &ed.read(cx).core.doc;
        let (title, icon, body, reminders, links) = (doc.title.text.clone(), doc.icon.clone(), doc.plain_text(), doc.reminders(), doc.doc_links());
        match self.store.save(&id, doc) {
            Ok(meta) => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                    e.title = title;
                    e.icon = icon;
                    e.body = body.into();
                    e.reminders = reminders;
                    e.links = links;
                    e.meta = meta.clone();
                }
                self.sync_links(cx);
                ed.update(cx, |e, cx| {
                    e.modified = meta.modified;
                    cx.notify();
                });
                self.status = SaveStatus::Saved;
                self.unsaved = false;
                self.sort();
            }
            Err(e) => self.status = SaveStatus::Failed(crate::localized_format!("保存失败：{}", "Could not save: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        // A failed save tries again SAVE_MAX_DELAY after this attempt, not on every keystroke.
        self.dirty_since = self.unsaved.then(Instant::now);
        cx.notify();
        !self.unsaved
    }

    /// Save the open document before something replaces it. When the save
    /// fails nothing is replaced: the 保存失败 dialog offers 重试 / 放弃修改
    /// and runs `then` after either.
    /// 改用文件夹: move the library to real folders, then reopen the main
    /// window on it at the same place (documents get new ids in the new layout).
    pub(crate) fn migrate_library(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.save_before(cx, |w, window, cx| w.migrate_library(window, cx)) { return }
        let root = crate::store::Store::root_dir();
        let moved = crate::store::Store::migrate(&root).and_then(|r| Ok((r, crate::store::Store::open_at(root.clone())?)));
        match moved {
            Err(e) => self.show_toast_kind(ToastKind::Error, &crate::localized_format!("没能改为真实文件夹，文档库保持原样：{}", "Couldn't move to real folders; the library is unchanged: {}", crate::settings::diagnostic(&e.to_string())), cx),
            Ok((report, store)) => {
                self.store.with(|s| *s = store);
                crate::changelog::migrated(&report);
                crate::open_main_window(crate::devtools::scripted(), Some(window.bounds()), cx);
                window.remove_window();
            }
        }
    }

    pub fn save_before(&mut self, cx: &mut Context<Self>, then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> bool {
        if self.store.file_task_busy() { return false; }
        if self.flush(cx) {
            return true;
        }
        self.after_save = Some(Box::new(then));
        self.confirm = Some(Confirm::SaveFailed);
        cx.notify();
        false
    }

    /// The 保存失败 dialog's choices: `retry` saves again, otherwise the
    /// changes are given up. Either way the held-back action runs once the
    /// document no longer needs saving.
    pub(crate) fn resolve_save_failed(&mut self, retry: bool, window: &mut Window, cx: &mut Context<Self>) {
        if retry {
            if !self.flush(cx) {
                self.confirm = Some(Confirm::SaveFailed);
                return;
            }
        } else {
            let Some(id) = self.current.clone() else { return };
            match self.store.load_for_edit(&id, false) {
                Ok(doc) => {
                    if let Some(ed) = &self.editor {
                        ed.update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc), cx));
                    }
                    self.sync_document(window, cx);
                    self.save_task = None;
                    self.unsaved = false;
                    self.status = SaveStatus::Idle;
                    self.dirty_since = None;
                    self.entries = self.store.list();
                    self.sort();
                }
                Err(e) => {
                    self.status = SaveStatus::Failed(e.to_string());
                    self.confirm = Some(Confirm::SaveFailed);
                    return;
                }
            }
        }
        if let Some(then) = self.after_save.take() {
            then(self, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn save_failed_copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ed) = &self.editor else { return };
        let mut doc = ed.read(cx).core.doc.clone();
        doc.title = crate::model::RichText::plain(format!("{}{}", doc.title.text, crate::settings::tr("（恢复副本）", " (recovered copy)")));
        match self.store.create_from(&doc) {
            Ok(_) => {
                self.entries = self.store.list();
                self.sort();
                self.confirm = None;
                self.resolve_save_failed(false, window, cx);
            }
            Err(e) => {
                self.status = SaveStatus::Failed(e.to_string());
                self.confirm = Some(Confirm::SaveFailed);
                cx.notify();
            }
        }
    }

    /// The error of the last failed save, for the 保存失败 dialog.
    pub(crate) fn save_error(&self) -> String {
        match &self.status {
            SaveStatus::Failed(e) => e.clone(),
            _ => crate::settings::text("保存失败").to_string(),
        }
    }

    /// Hand the open document the library's titles and the documents
    /// linking to it (双向链接).
    pub(crate) fn sync_links(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(ed)) = (self.current.clone(), self.editor.clone()) else { return };
        let docs = self
            .entries
            .iter()
            .map(|e| crate::editor::links::DocRef { id: e.id.clone(), title: e.title.clone(), icon: e.icon.clone(), folder: e.meta.folder.is_some(), modified: e.meta.modified, opened: e.meta.opened })
            .collect();
        let backlinks = self
            .entries
            .iter()
            .filter(|e| e.id != id)
            .filter_map(|e| {
                let lines: Vec<_> = e.links.iter().filter(|l| l.target == id).cloned().collect();
                (!lines.is_empty()).then(|| crate::editor::links::Backlink { id: e.id.clone(), lines })
            })
            .collect();
        let hidden = self.entries.iter().find(|e| e.id == id).is_some_and(|e| e.meta.refs_hidden);
        ed.update(cx, |e, cx| e.set_links(id, docs, backlinks, hidden, cx));
    }

    /// A 反向引用 line: open that document at its `ordinal`th block.
    fn open_at(&mut self, id: &str, ordinal: usize, window: &mut Window, cx: &mut Context<Self>) {
        if !self.try_open(id, window, cx) {
            return;
        }
        let Some(ed) = self.editor.clone() else { return };
        ed.update(cx, |e, cx| {
            let mut n = 0;
            let mut target = None;
            e.core.doc.for_each_block(|b| {
                if n == ordinal {
                    target = Some(b.id);
                }
                n += 1;
            });
            if let Some(b) = target {
                e.scroll_to_block(b, cx);
            }
        });
    }

    /// 隐藏引用列表 / 显示引用列表: confirm first (hiding can skip it with 不再提示).
    pub(crate) fn ask_refs(&mut self, show: bool, cx: &mut Context<Self>) {
        if !show && self.store.settings().refs_hide_no_ask {
            return self.set_refs_hidden(true, cx);
        }
        self.confirm = Some(Confirm::Refs { show, no_ask: false });
        cx.notify();
    }

    pub(crate) fn set_refs_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let Some(id) = self.current.clone() else { return };
        if self.store.update_meta(&id, |m| m.refs_hidden = hidden).is_ok()
            && let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
        {
            e.meta.refs_hidden = hidden;
        }
        self.sync_links(cx);
        cx.notify();
    }

    /// 主页's search box, centred on the pane but never over what sits on
    /// either side of it: the title (and, with the sidebar hidden, the
    /// traffic lights and 展开侧栏) on the left, + on the right. Below a
    /// usable width it is None and the caller shows a search icon instead.
    pub(crate) fn header_search(&self, window: &Window, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let main = f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        let title = crate::settings::label(crate::settings::text("主页"));
        let title_w = f32::from(window.text_system().shape_line(title.clone().into(), px(17.), &[window.text_style().highlight(HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() }).to_run(title.len())], None).width);
        // page_header's left inset (26 with the sidebar, else the traffic
        // lights plus the 4 + 28 + 12 of 展开侧栏), then 16 clear of the title.
        let left = if self.sidebar_open { 26. } else { theme::TRAFFIC_LIGHTS_W + 44. + if cfg!(target_os = "windows") { 32. } else { 0. } } + title_w + 16.;
        // + is 32 wide with an 8pt inset; 16 clear of it.
        let right = HEADER_ACTIONS_H + crate::ui::header_right_inset() + 16.;
        let half = (main / 2. - left).min(main / 2. - right);
        let width = (main - 320.).min(2. * half).min(360.);
        if width < 120. { return None }
        Some(div().id("header-search").absolute().left(px((main - width) / 2.)).top(px(8.))
            .w(px(width)).h(px(32.)).no_drag().flex().items_center().gap(px(8.)).px(px(10.))
            .rounded(px(9.)).bg(Theme::fill_hover())
            .cursor_pointer().hover(|s| s.bg(Theme::fill_hover_strong()))
            .child(icon("search", 15., Theme::icon()))
            .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).text_color(Theme::text_caption()).child(crate::settings::tr("搜索文档…", "Search documents…")))
            .when(width >= 200., |d| d.child(div().flex_none().text_size(px(11.)).text_color(Theme::text_placeholder()).child(crate::shortcuts::hint("workspace::FocusSearch", "⌘ + J").replace(" + ", " "))))
            .tip(crate::settings::tr("搜索文档", "Search documents"), Some(crate::shortcuts::hint("workspace::FocusSearch", "⌘ + J")))
            .on_click(cx.listener(|w, _, window, cx| w.open_search(window, cx))))
    }

    /// `pane_w`: the width the header spans, to fold the breadcrumbs when narrow.
    fn top_bar(&self, pane_w: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let mut title = self
            .editor
            .as_ref()
            .map(|e| display_title(&e.read(cx).core.doc.title.text))
            .unwrap_or_default();
        // a 画板 block open in the document's place: the document becomes a
        // crumb back to it, and the board is the page
        let board_block = self.editor.as_ref().is_some_and(|e| e.read(cx).board_block_open());
        let save_error = match &self.status {
            SaveStatus::Failed(error) => Some(error.clone()),
            _ => None,
        };
        // Independent icon controls, with equal top and right insets.
        let seg = crate::ui::header_icon;
        let divider = || div().flex_none().w(px(1.)).h(px(18.)).bg(Theme::line_divider());
        // Breadcrumbs: the parent pages, each linking to itself, with the
        // middle ones folded into "…" when the chain is long.
        let chain: Vec<(String, String, Option<String>)> = self
            .current
            .as_ref()
            .map(|id| self.ancestors(id).iter().map(|e| (e.id.clone(), display_title(&e.title), e.icon.clone())).collect())
            .unwrap_or_default();
        let mut crumbs: Vec<AnyElement> = vec![];
        let sep = || div().mx(px(3.)).flex_none().child(icon("chevron-right", 12., Theme::icon())).into_any_element();
        // Every path starts at the library, as a folder page's does.
        if self.current.is_some() {
            crumbs.push(div().id("crumb-library").no_drag().flex_none().px(px(4.)).rounded(px(4.)).cursor_pointer()
                .whitespace_nowrap().text_color(Theme::text_caption()).hover(|s| s.text_color(Theme::primary_500()))
                .child(ui_text(crate::settings::tr("我的文档", "My documents").to_string()))
                .on_click(cx.listener(|w, _, _, cx| w.show_view(View::Home, cx)))
                .into_any_element());
            crumbs.push(sep());
        }
        for (i, (id, name, em)) in chain.iter().enumerate() {
            if chain.len() > 2 && i > 0 && i < chain.len() - 1 {
                if i == 1 {
                    let hidden: Vec<&str> = chain[1..chain.len() - 1].iter().map(|c| c.1.as_str()).collect();
                    crumbs.push(
                        div()
                            .id("crumb-more")
                            .no_drag()
                            .flex_none()
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .text_color(Theme::text_caption())
                            .tip(&hidden.join(" / "), None)
                            .child("…")
                            .into_any_element(),
                    );
                    crumbs.push(sep());
                }
                continue;
            }
            let id = id.clone();
            crumbs.push(
                div()
                    .id(SharedString::from(format!("crumb-{id}")))
                    .no_drag()
                    .min_w(px(28.))
                    .max_w(px(200.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_color(Theme::text_caption())
                    .hover(|s| s.text_color(Theme::primary_500()))
                    .when_some(em.clone(), |d, em| {
                        d.child(div().size(px(20.)).mr(px(4.)).flex_none().flex().items_center().justify_center().text_size(px(16.)).child(em))
                    })
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(name.clone())))
                    .on_click(cx.listener(move |w, _, window, cx| w.open(&id, window, cx)))
                    .into_any_element(),
            );
            crumbs.push(sep());
        }
        if board_block {
            crumbs.push(
                div().id("crumb-board-doc").no_drag().min_w(px(28.)).max_w(px(200.)).px(px(4.)).flex().items_center().rounded(px(4.)).cursor_pointer()
                    .text_color(Theme::text_caption()).hover(|s| s.text_color(Theme::primary_500()))
                    .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(title.clone())))
                    .on_click(cx.listener(|w, _, window, cx| if let Some(e) = w.editor.clone() { e.update(cx, |e, cx| e.close_open_board(window, cx)) }))
                    .into_any_element(),
            );
            crumbs.push(sep());
            title = crate::settings::tr("画板", "Board").to_string();
        }
        // Too narrow for the path: it folds into one "…" (the path on hover),
        // and the title keeps the room. Left: the sidebar's buttons when it is
        // closed; right: the three actions.
        let left = if self.sidebar_open { 18. } else { theme::TRAFFIC_LIGHTS_W + 4. + 28. + 4. + 28. + 25. };
        let room = pane_w - left - crate::ui::header_right_inset() - 3. * HEADER_ACTIONS_H - 8. - 16.;
        if room < 360. && !crumbs.is_empty() {
            let path: Vec<&str> = std::iter::once(crate::settings::tr("我的文档", "My documents")).chain(chain.iter().map(|c| c.1.as_str())).collect();
            crumbs = vec![
                div().id("crumb-more").no_drag().flex_none().size(px(20.)).flex().items_center().justify_center().rounded(px(4.))
                    .text_color(Theme::text_caption()).tip(&path.join(" / "), None).child("…").into_any_element(),
                sep(),
            ];
        }
        let record_more = self.more_btn_bounds.clone();
        div()
            .id("top-bar").relative()
            .h(px(HEADER_H))
            .flex_none()
            .flex()
            .items_center()
            .pr(px(crate::ui::header_right_inset()))
            // Clear of the traffic lights, which sit over the note when the sidebar is closed.
            .pl(if self.sidebar_open { px(18.) } else { px(theme::TRAFFIC_LIGHTS_W) })
            .title_bar()
            .when(!self.sidebar_open, |d| {
                d.child(
                    div()
                        .id("open-sidebar")
                        .no_drag()
                        .flex_none()
                        .ml(px(4.))
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|s| s.bg(Theme::fill_hover_strong()))
                        .tip(crate::settings::tr("展开侧栏", "Show sidebar"), Some(crate::shortcuts::hint("workspace::ToggleSidebar", "⌘ + ]")))
                        .child(icon("panel-left-open", 17., Theme::icon()))
                        .on_click(cx.listener(|w, _, _, cx| {
                            w.sidebar_open = true;
                            cx.notify();
                        })),
                )
                .child(
                    div().id("header-home").no_drag().flex_none().ml(px(4.)).size(px(28.))
                        .flex().items_center().justify_center().rounded(px(8.)).cursor_pointer()
                        .hover(|s| s.bg(Theme::fill_hover_strong()))
                        .active(|s| s.bg(Theme::fill_pressed()))
                        .tip(crate::settings::text("返回主页"), None)
                        .child(icon("home", 18., Theme::icon()))
                        .on_click(cx.listener(|w, _, _, cx| w.show_view(View::Home, cx))),
                )
                .child(div().mx(px(12.)).child(divider()))
            })
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .h(px(20.))
                            .min_w_0()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .children(crumbs)
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(14.))
                                    .line_height(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(Theme::text())
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(ui_text(title)),
                            ),

                    )
                    .when_some(save_error, |d, error| d.child(
                        div().min_w_0().max_w(px(220.)).text_size(px(12.)).line_height(px(18.))
                            .text_color(theme::c(craft::R400)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(error),
                    )),
            )
            .child(div().flex_1().min_w(px(16.)))
            .child(
                div().gap(px(4.))
                    .h(px(HEADER_ACTIONS_H)).flex_none()
                    .flex()
                    .items_center()
                    .when(self.current.is_some(), |d| {
                        d.child(
                            // No tooltip while its dropdown is open; the menu hangs
                            // from this button's right edge.
                            seg("doc-more", "ellipsis")
                                .relative()
                                .when(self.doc_menu, |d| d.bg(Theme::fill_hover()))
                                .when(!self.doc_menu, |d| d.tip(crate::settings::tr("更多", "More"), None))
                                .child(canvas(move |b, _, _| record_more.set(Some(b)), |_, _, _, _| {}).absolute().inset_0())
                                .on_click(cx.listener(|w, _, _, cx| {
                                    w.doc_menu = !w.doc_menu;
                                    w.doc_sub = None;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        seg("doc-search", "search")
                            .tip_label(crate::shortcuts::keys(crate::settings::text("搜索(⌘ + J)")).into(), TipSide::Below)
                            .on_click(cx.listener(|_, _, window, cx| window.dispatch_action(Box::new(FocusSearch), cx))),
                    )
                    .child(
                        // Straight to a new document: no 新建 menu, as 导入 is rare.
                        seg("doc-new", "plus").tip(crate::settings::tr("新建文档", "New document"), None).on_click(cx.listener(|w, _, window, cx| w.new_doc(window, cx))),
                    ).into_any_element()
            )
    }

    fn render_workspace_vim_hud(&self) -> Option<AnyElement> {
        use crate::editor::vim::{vim_badge_pill, which_key_card};
        use crate::settings::tr;
        if !crate::settings::vim_mode()
            || self.search_open
            || self.rename.is_some()
            || self.version_rename.is_some()
            || self.move_dlg.is_some()
            || self.onboarding.is_some()
            || self.confirm.is_some()
            || self.export_md.is_some()
            || self.shortcut_recording.is_some()
        {
            return None;
        }
        if self.view == View::Doc && self.pane_focus == PaneFocus::Main && self.history.is_none() {
            return None;
        }
        let mode = if self.history.is_some() {
            "HISTORY"
        } else if self.pane_focus == PaneFocus::Sidebar {
            if matches!(self.view, View::Settings | View::Trash) { "SETTINGS NAV" } else { "SIDEBAR" }
        } else {
            match self.view {
                View::Home => if self.batch.is_some() { "HOME · BATCH" } else { "HOME" },
                View::Folder => if self.batch.is_some() { "FOLDER · BATCH" } else { "FOLDER" },
                View::Trash => "TRASH",
                View::Settings => "SETTINGS",
                View::Doc => return None,
            }
        };
        let pend = if self.sidebar_pending_ctrl_w || self.non_doc_pending == Some('w') {
            Some("^W".to_string())
        } else if self.sidebar_pending_g {
            Some("g".to_string())
        } else {
            self.non_doc_pending.map(|c| if c == ' ' { "␣".to_string() } else { c.to_string() })
        };
        let badge_label = match &pend {
            Some(p) => format!("{mode} · {p}"),
            None => mode.to_string(),
        };
        let accent = pend.is_some() || self.batch.is_some() || self.pane_focus == PaneFocus::Sidebar || self.history.is_some();
        let guide: Option<AnyElement> = if self.sidebar_pending_ctrl_w || self.non_doc_pending == Some('w') {
            Some(which_key_card(
                tr("分栏导航 · ^W", "Window · ^W"),
                &[
                    ("h", tr("聚焦左侧栏", "Focus sidebar")),
                    ("l", tr("聚焦右侧主区", "Focus main pane")),
                    ("w", tr("循环切换分栏", "Cycle panes")),
                ],
            ))
        } else if self.non_doc_pending == Some(' ') {
            Some(which_key_card(
                tr("Leader · 空格", "Leader · Space"),
                &[
                    ("b / ␣", tr("搜索与切换文档", "Search / switch docs")),
                    ("h", tr("进入主页", "Go to Home")),
                    ("e", tr("切换侧栏 / 主区焦点", "Toggle sidebar focus")),
                    ("H", tr("版本历史 (已打开文档)", "Version history (if doc)")),
                    ("?", tr("快捷键速查", "Shortcuts cheatsheet")),
                ],
            ))
        } else if matches!(self.non_doc_pending, Some('[' | ']')) {
            Some(which_key_card(
                tr("切换文档 · [ / ]", "Buffer · [ / ]"),
                &[("b", tr("上 / 下一篇文档 (Buffer 环)", "Prev / next doc (buffer ring)"))],
            ))
        } else if self.sidebar_pending_g || self.non_doc_pending == Some('g') {
            Some(which_key_card(
                tr("跳转 · g", "Goto · g"),
                &[("g", tr("跳到列表第一项", "Jump to first item"))],
            ))
        } else if self.non_doc_pending == Some('d') {
            Some(which_key_card(
                tr("删除 · d", "Delete · d"),
                &[("d", tr("删除当前选中项", "Delete selected item"))],
            ))
        } else if self.non_doc_pending == Some('?') {
            Some(if self.history.is_some() {
                which_key_card(
                    tr("版本历史 · History", "Version history"),
                    &[
                        ("j / k", tr("上下选择并预览版本", "Select & preview version")),
                        ("Enter", tr("恢复所选版本", "Restore selected version")),
                        ("m / f", tr("显示改动 / 仅看命名", "Toggle marks / named")),
                        ("r / s", tr("命名版本 / 保存快照", "Name / save snapshot")),
                        ("c / x", tr("另存副本 / 删除版本", "Save copy / delete")),
                        ("^D / ^U", tr("滚动右侧预览正文", "Scroll preview")),
                        ("q / Esc", tr("关闭 / 退出预览", "Close / exit preview")),
                    ],
                )
            } else if self.pane_focus == PaneFocus::Sidebar {
                if matches!(self.view, View::Settings | View::Trash) {
                    which_key_card(
                        tr("设置导航 · Sidebar", "Settings Nav"),
                        &[
                            ("j / k", tr("切换设置分类 / 回收站", "Switch section / Trash")),
                            ("gg / G", tr("首项 / 末项（返回）", "First / last (Back)")),
                            ("l / Enter", tr("进入右侧配置 / 返回", "Focus pane / back")),
                            ("Esc / q", tr("关闭设置", "Close settings")),
                        ],
                    )
                } else {
                    which_key_card(
                        tr("侧栏导航 · Sidebar", "Sidebar"),
                        &[
                            ("j / k", tr("上下移动光标", "Move cursor up / down")),
                            ("h / l", tr("折叠 / 展开或打开", "Collapse / expand or open")),
                            ("gg / G", tr("跳到顶部 / 底部", "Jump to top / bottom")),
                            ("Enter", tr("打开文档并聚焦正文", "Open & focus main")),
                            ("a / A", tr("新建文档 / 文件夹", "New doc / folder")),
                            ("r / d", tr("重命名 / 删除", "Rename / delete")),
                            ("p / s", tr("置顶 / 收藏", "Pin / favorite")),
                            ("Esc / q", tr("回到主内容区", "Return to main pane")),
                        ],
                    )
                }
            } else {
                match self.view {
                    View::Home | View::Folder => {
                        if self.batch.is_some() {
                            which_key_card(
                                tr("批量操作 · Batch", "Batch Select"),
                                &[
                                    ("h/j/k/l", tr("移动选中光标", "Move cursor")),
                                    ("Space / a", tr("勾选当前 / 全选", "Toggle item / all")),
                                    ("c / m", tr("创建副本 / 移动到", "Duplicate / move to")),
                                    ("d / x", tr("批量删除", "Delete selected")),
                                    ("Esc / q", tr("退出批量操作", "Exit batch mode")),
                                ],
                            )
                        } else {
                            let title = if self.view == View::Home { tr("主页导航 · Home", "Home") } else { tr("文件夹 · Folder", "Folder") };
                            which_key_card(
                                title,
                                &[
                                    ("h/j/k/l", tr("移动选框（宫格 / 列表）", "Move selection")),
                                    ("Enter", tr("打开文档 / 进入文件夹", "Open doc / folder")),
                                    ("Tab / v", tr("切换标签 / 宫格列表", "Tab / grid-list")),
                                    ("b", tr("进入批量管理", "Enter batch mode")),
                                    ("n / N", tr("新建文档 / 文件夹", "New doc / folder")),
                                    ("r / p / s", tr("重命名 / 置顶 / 收藏", "Rename / pin / star")),
                                    ("x / dd", tr("删除当前项", "Delete item")),
                                    ("- / Esc", tr("返回上级 / 打开的文档", "Go up / back to doc")),
                                ],
                            )
                        }
                    }
                    View::Trash => which_key_card(
                        tr("回收站 · Trash", "Trash"),
                        &[
                            ("j / k", tr("上下选择已删文档", "Move selection")),
                            ("gg / G", tr("首项 / 末项", "First / last item")),
                            ("r / Enter", tr("恢复所选文档", "Restore selected")),
                            ("d / x", tr("彻底删除所选文档", "Purge selected")),
                            ("E", tr("清空回收站", "Empty trash")),
                            ("Esc / q", tr("退出回收站", "Leave trash")),
                        ],
                    ),
                    View::Settings => which_key_card(
                        tr("设置操作 · Settings", "Settings"),
                        &[
                            ("j / k", tr("上下选择配置项", "Select setting row")),
                            ("h/l/Enter", tr("调整值 / 开关 / 菜单", "Adjust / toggle / menu")),
                            ("Tab", tr("切换左侧导航 / 配置区", "Switch nav / content")),
                            ("/", tr("搜索快捷键（快捷键页）", "Filter shortcuts")),
                            ("r / x", tr("恢复默认 / 移除快捷键", "Reset / clear key")),
                            ("Esc / q", tr("关闭设置", "Close settings")),
                        ],
                    ),
                    View::Doc => return None,
                }
            })
        } else {
            None
        };
        let right = if self.history.is_some() { 324. } else if self.view == View::Doc && self.shortcuts { 338. } else { 34. };
        Some(
            div()
                .absolute()
                .bottom(px(10.))
                .right(px(right))
                .flex()
                .flex_col()
                .items_end()
                .gap(px(6.))
                .children(guide)
                .child(vim_badge_pill(badge_label, accent))
                .into_any_element(),
        )
    }
}

/// Header and action group share an 8px top/right inset.
pub const HEADER_H: f32 = 48.;
/// The narrowest the main pane (document, Home, settings) gets: the window's
/// minimum width is this plus the open sidebar.
pub const DOC_MIN_W: f32 = 480.;
pub(crate) const HEADER_ACTIONS_H: f32 = 32.;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Clicks no longer take focus from the editor (see the root's mouse
        // down), so leaving the document hands it back to the window's keys.
        let modal_busy = self.search_open
            || self.rename.is_some()
            || self.version_rename.is_some()
            || self.move_dlg.is_some()
            || self.onboarding.is_some()
            || self.shortcut_recording.is_some()
            || self.confirm.is_some()
            || self.export_md.is_some();
        if self.view != View::Doc
            && !modal_busy
            && !self.settings_filter.read(cx).is_focused(window)
            && (self.editor.as_ref().is_some_and(|e| e.read(cx).focus.is_focused(window)) || window.focused(cx).is_none())
        {
            window.focus(&self.focus);
        }
        if self.pane_focus == PaneFocus::Sidebar
            && (!self.sidebar_open
                || (self.view == View::Doc && self.editor.as_ref().is_some_and(|e| e.read(cx).focus.is_focused(window))))
        {
            self.pane_focus = PaneFocus::Main;
        }
        if crate::settings::vim_mode()
            && crate::settings::vim_auto_ime()
            && !modal_busy
            && !self.settings_filter.read(cx).is_focused(window)
            && (self.pane_focus == PaneFocus::Sidebar || self.view != View::Doc || self.history.is_some())
            && window.is_window_active()
        {
            window.set_ime_active(false);
        }
        crate::editor::image::frame_drawn(window, cx);
        // Full screen: the sidebar is solid (`Theme::window`) and the native glass stays hidden.
        crate::theme::set_fullscreen(window.is_fullscreen());
        self.native_sidebar.sync(if self.sidebar_open && !crate::theme::fullscreen() { self.sidebar_w(window) } else { 0. }, window);
        // The pane keeps its minimum beside the sidebar: opening the sidebar in
        // a narrow window widens the window rather than squeezing the note.
        let min_w = DOC_MIN_W + if self.sidebar_open { self.sidebar_w(window) } else { 0. };
        if min_w != self.min_w {
            self.min_w = min_w;
            window.set_window_min_size(size(px(min_w), px(480.)));
            let vs = window.viewport_size();
            if f32::from(vs.width) < min_w - 0.5 && !window.is_fullscreen() { window.resize(size(px(min_w), vs.height)); }
        }
        let sidebar = if self.sidebar_open { Some(if matches!(self.view, View::Settings | View::Trash) { self.render_settings_sidebar(window, cx) } else { self.render_sidebar(window, cx) }) } else { None };
        let row_menu = self.render_row_menu(window, cx);
        let resize = self.sidebar_open.then(|| self.render_resize_handle(window, cx));
        let resize_overlay = self.render_resize_overlay(cx);
        let folder_drag_overlay = self.render_folder_drag_overlay(cx);
        let home_drag_overlay = self.render_home_drag_overlay(window, cx);
        let file_drop = self.render_file_drop(window, cx);
        let create_menu = self.render_create_menu(cx);
        let import_menu = self.render_import_menu(cx);
        let place_menu = self.render_place_menu(cx);
        let search = self.render_search(window, cx);
        let doc_menu = self.render_doc_menu(window, cx);
        let doc_info = self.render_doc_info(cx);
        let history = self.render_history(cx);
        let confirm = self.render_confirm(cx);
        let trash_menu = self.render_trash_menu(cx);
        let home_menus = self.render_home_menus(cx);
        let theme_menu = self.render_theme_menu(cx);
        let font_menu = self.render_font_menu(cx);
        let lang_menu = self.render_lang_menu(cx);
        let pref_menu = self.render_pref_menu(cx);
        let toast = self.render_toast(window);
        let mode_menu = self.render_mode_menu(cx);
        let main = match self.view {
            View::Doc => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .child(self.top_bar(f32::from(window.viewport_size().width) - if self.sidebar_open { self.sidebar_w(window) } else { 0. }, cx))
                .children(self.render_history_banner(cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .children(self.editor.clone()),
                        )
                        .children(self.render_shortcuts(cx)),
                )
                .into_any_element(),
            View::Home => self.render_home(window, cx),
            View::Folder => self.render_folder(window, cx),
            View::Trash => self.render_trash(cx),
            View::Settings => self.render_settings(cx),
        };
        let move_dlg = self.render_move_dialog(cx);
        let export_md = self.render_export_md(cx);
        // Only the note stands out: a white plane flush with the window's top,
        // right and bottom edges. The sidebar sits sunken on the window
        // surface beside it; the change of value is the only line between
        // them (no gutter, radius, border or shadow).
        // A document sits on its theme's page colour.
        let page = if self.view == View::Doc { crate::editor_theme::Theme::bg_body() } else { Theme::bg_body() };
        let vim_hud = self.render_workspace_vim_hud();
        let note = div().id("note").relative().flex_1().min_w_0().h_full().flex().bg(page).child(main)
            // Files from the Finder dropped on 主页 or a folder's page are imported there.
            .when(matches!(self.view, View::Home | View::Folder), |d| d
                .on_drag_move(cx.listener(|w, ev: &DragMoveEvent<ExternalPaths>, _, cx| w.file_drag_move(ev, cx)))
                .on_drop(cx.listener(|w, paths: &ExternalPaths, window, cx| w.file_dropped(paths, window, cx))))
            .children(file_drop)
            .children(history)
            .children(vim_hud);
        div()
            .id("workspace")
            .key_context("Workspace")
            .capture_action(cx.listener(|w, _: &crate::editor::keymap::Undo, window, cx| w.capture_file_history(false, window, cx)))
            .capture_action(cx.listener(|w, _: &crate::editor::keymap::Redo, window, cx| w.capture_file_history(true, window, cx)))
            .track_focus(&self.focus)
            // A click on what takes no input (title bar, page margins, the
            // sidebar's blank space) leaves the caret in the document, also
            // the click that brings the window back (#59). Runs before the
            // root takes focus; anything inside that takes focus did so first.
            .on_mouse_down(MouseButton::Left, cx.listener(|w, _, window, cx| {
                if w.view == View::Doc && w.editor.as_ref().is_some_and(|e| e.read(cx).focus.is_focused(window)) { window.prevent_default(); }
            }))
            .relative()
            .size_full()
            .flex()
            .bg(Theme::window())
            .font(theme::font_with_fallbacks(theme::UI_FONT, false))
            .text_color(Theme::text())
            // Window keys act on the window they were pressed in, not on
            // whichever window macOS considers active.
            .on_mouse_move(cx.listener(|w, ev: &MouseMoveEvent, window, cx| w.track_sidebar_press(ev, window, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|w, _, window, cx| w.finish_sidebar_click(window, cx)))
            .on_action(cx.listener(|w, _: &CloseWindow, window, cx| {
                crate::quick_note::persist(cx);
                if w.save_before(cx, |_, window, _| window.remove_window()) { window.remove_window(); }
            }))
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|w, _: &OpenSettings, _, cx| w.show_view(View::Settings, cx)))
            .on_action(cx.listener(|w, _: &ReplayOnboarding, window, cx| w.replay_onboarding(window, cx)))
            .on_action(cx.listener(|w, _: &OpenAbout, _, cx| w.open_settings(crate::settings::Page::About, cx)))
            .on_action(cx.listener(|w, _: &CheckForUpdates, _, cx| { w.open_settings(crate::settings::Page::About, cx); crate::update::check(true, cx); }))
            .on_action(cx.listener(|w, _: &GoHome, _, cx| w.show_view(View::Home, cx)))
            .on_action(cx.listener(|w, _: &NewDoc, window, cx| w.new_doc(window, cx)))
            .on_action(cx.listener(|w, _: &ImportDoc, window, cx| w.open_import(window, cx)))
            .on_action(cx.listener(|w, _: &OpenFile, window, cx| w.open_file_dialog(window, cx)))
            .on_action(cx.listener(|w, _: &ImportFolder, window, cx| w.open_import_folder(window, cx)))
            .on_action(cx.listener(|w, _: &ToggleSidebar, window, cx| {
                w.sidebar_open = !w.sidebar_open;
                if !w.sidebar_open && w.pane_focus == PaneFocus::Sidebar {
                    w.vim_focus_pane(fdl_core::editor::vim::VimPane::Right, window, cx);
                } else {
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|w, _: &FindInDoc, window, cx| w.find_in_doc(false, window, cx)))
            .on_action(cx.listener(|w, _: &ReplaceInDoc, window, cx| {
                let replace = !w.reading();
                w.find_in_doc(replace, window, cx)
            }))
            .on_action(cx.listener(|w, _: &crate::modes::EditMode, window, cx| w.set_mode(false, window, cx)))
            .on_action(cx.listener(|w, _: &crate::modes::ReadMode, window, cx| w.set_mode(true, window, cx)))
            .on_action(cx.listener(|w, _: &crate::modes::ToggleShortcuts, _, cx| {
                w.shortcuts = !w.shortcuts;
                cx.notify();
            }))
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| {
                // The theme dropdown: ↑ ↓ preview, ↩ applies, Esc closes it (not 设置).
                if (w.theme_menu.is_some() && w.theme_menu_key(&ev.keystroke.key, cx)) || (w.font_menu.is_some() && w.font_menu_key(&ev.keystroke.key, cx))
                    || (w.lang_menu.is_some() && w.lang_menu_key(&ev.keystroke.key, cx)) || (w.pref_menu.is_some() && w.pref_menu_key(&ev.keystroke.key, cx)) {
                    cx.stop_propagation();
                    return;
                }
                // Esc leaves 设置 (unless its filter field has the caret or a Vim prefix is pending).
                if ev.keystroke.key == "escape" && w.view == View::Settings && !w.settings_filter.read(cx).is_focused(window) && w.non_doc_pending.is_none() {
                    w.close_settings(cx);
                    if w.view == View::Doc && let Some(ed) = &w.editor { ed.read(cx).focus(window); }
                    cx.stop_propagation();
                    return;
                }
                if ev.keystroke.key == "escape" && (w.doc_info.is_some() || w.history.is_some() || w.doc_menu || w.mode_menu) {
                    w.doc_info = None;
                    if w.history.is_some() {
                        w.close_history(cx);
                        if let Some(ed) = &w.editor { ed.read(cx).focus(window); }
                    }
                    w.mode_menu = false;
                    w.doc_menu = false;
                    w.doc_sub = None;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if crate::settings::vim_mode()
                    && !w.search_open
                    && w.rename.is_none()
                    && w.version_rename.is_none()
                    && w.confirm.is_none()
                    && w.export_md.is_none()
                    && w.move_dlg.is_none()
                    && w.onboarding.is_none()
                    && w.row_menu.is_none()
                    && !w.doc_menu
                    && !w.mode_menu
                {
                    if w.pane_focus == PaneFocus::Sidebar && w.focus.is_focused(window) {
                        if w.handle_sidebar_vim_key(ev, window, cx) {
                            cx.stop_propagation();
                        }
                    } else if w.view != View::Doc && w.handle_non_doc_vim_key(ev, window, cx) {
                        cx.stop_propagation();
                    }
                }
            }))
            // Esc leaves 批量操作 (closing its menu or confirmation first), even from the editor.
            .capture_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| {
                let undo = ev.keystroke.modifiers.secondary() && matches!(ev.keystroke.key.as_str(), "z" | "y");
                if w.store.file_task_busy() || (w.file_history.busy && !undo) { cx.stop_propagation(); return; }
                // 版本历史: the editor keeps the caret, so Esc has to be taken
                // before it reaches the read-only key handler.
                if ev.keystroke.key == "escape" && w.preview.is_some() {
                    w.exit_preview(cx); cx.stop_propagation(); return;
                }
                if ev.keystroke.key == "escape" && w.history.is_some() {
                    if w.non_doc_pending.take().is_some() {
                        cx.notify();
                    } else {
                        w.close_history(cx);
                        if let Some(ed) = &w.editor { ed.read(cx).focus(window); }
                    }
                    cx.stop_propagation(); return;
                }
                if ev.keystroke.key == "escape" && w.has_home_drag() {
                    w.cancel_home_drag(cx); cx.stop_propagation(); return;
                }
                if ev.keystroke.key == "escape" && w.folder_drag.active.is_some() {
                    w.end_folder_drag(true, cx); cx.stop_propagation(); return;
                }
                if w.record_shortcut(ev, cx) { return; }
                // Esc cancels a confirmation dialog; in Vim mode Enter/y confirms and n/q cancels.
                if ev.keystroke.key == "escape" && (w.confirm.is_some() || w.export_md.is_some()) {
                    if w.confirm.is_some() { w.cancel_confirm(cx); } else { w.export_md = None; cx.notify(); }
                    cx.stop_propagation();
                    return;
                }
                if ev.keystroke.key == "escape"
                    && !ev.keystroke.modifiers.platform
                    && !ev.keystroke.modifiers.control
                    && !ev.keystroke.modifiers.alt
                    && w.shortcuts
                    && !w.search_open
                    && w.rename.is_none()
                    && w.version_rename.is_none()
                    && w.move_dlg.is_none()
                    && w.onboarding.is_none()
                    && w.doc_info.is_none()
                    && !w.doc_menu
                    && !w.mode_menu
                    && w.row_menu.is_none()
                    && w.batch.is_none()
                    && w.theme_menu.is_none()
                    && w.font_menu.is_none()
                    && w.lang_menu.is_none()
                    && w.pref_menu.is_none()
                    && !w.settings_filter.read(cx).is_focused(window)
                    && w.non_doc_pending.is_none()
                    && !w.sidebar_pending_g
                    && !w.sidebar_pending_ctrl_w
                    && (!crate::settings::vim_mode() || w.pane_focus == PaneFocus::Main)
                    && (w.view != View::Doc || w.editor.as_ref().is_none_or(|ed| ed.read(cx).can_close_shortcuts_on_esc(window, cx)))
                {
                    w.shortcuts = false;
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if crate::settings::vim_mode()
                    && w.confirm.is_some()
                    && !w.search_open
                    && w.rename.is_none()
                    && w.onboarding.is_none()
                {
                    let ks = &ev.keystroke;
                    if !ks.modifiers.platform && !ks.modifiers.control && !ks.modifiers.alt {
                        if ks.key == "enter" || ks.key == "y" {
                            w.accept_confirm(window, cx);
                            window.prevent_default();
                            cx.stop_propagation();
                            return;
                        }
                        if ks.key == "n" || ks.key == "q" {
                            w.cancel_confirm(cx);
                            window.prevent_default();
                            cx.stop_propagation();
                            return;
                        }
                    }
                }
                if crate::settings::vim_mode()
                    && w.confirm.is_none()
                    && w.export_md.is_none()
                    && w.rename.is_none()
                    && w.version_rename.is_none()
                    && w.move_dlg.is_none()
                    && w.onboarding.is_none()
                {
                    let ks = &ev.keystroke;
                    if w.search_open {
                        if (ks.modifiers.control && !ks.modifiers.alt && !ks.modifiers.platform && matches!(ks.key.as_str(), "n" | "j"))
                            || (ks.modifiers.alt && !ks.modifiers.control && !ks.modifiers.platform && ks.key == "j")
                        {
                            w.move_search_sel(1, cx);
                            window.prevent_default();
                            cx.stop_propagation();
                            return;
                        }
                        if (ks.modifiers.control && !ks.modifiers.alt && !ks.modifiers.platform && matches!(ks.key.as_str(), "p" | "k"))
                            || (ks.modifiers.alt && !ks.modifiers.control && !ks.modifiers.platform && ks.key == "k")
                        {
                            w.move_search_sel(-1, cx);
                            window.prevent_default();
                            cx.stop_propagation();
                            return;
                        }
                        if ks.modifiers.control && !ks.modifiers.alt && !ks.modifiers.platform && ks.key == "[" {
                            w.close_search(window, cx);
                            window.prevent_default();
                            cx.stop_propagation();
                            return;
                        }
                    } else if ks.modifiers.alt && !ks.modifiers.platform && !ks.modifiers.control && !ks.modifiers.shift {
                        use fdl_core::editor::vim::VimPane;
                        match ks.key.as_str() {
                            "h" => {
                                w.vim_focus_pane(VimPane::Left, window, cx);
                                window.prevent_default();
                                cx.stop_propagation();
                                return;
                            }
                            "l" => {
                                w.vim_focus_pane(VimPane::Right, window, cx);
                                window.prevent_default();
                                cx.stop_propagation();
                                return;
                            }
                            "j" => {
                                if w.pane_focus == PaneFocus::Sidebar {
                                    w.move_sidebar_cursor(1, cx);
                                } else {
                                    w.cycle_buffer(true, window, cx);
                                }
                                window.prevent_default();
                                cx.stop_propagation();
                                return;
                            }
                            "k" => {
                                if w.pane_focus == PaneFocus::Sidebar {
                                    w.move_sidebar_cursor(-1, cx);
                                } else {
                                    w.cycle_buffer(false, window, cx);
                                }
                                window.prevent_default();
                                cx.stop_propagation();
                                return;
                            }
                            _ => {}
                        }
                    } else if w.history.is_some() && w.handle_history_vim_key(ev, window, cx) {
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }
                }
                if ev.keystroke.key != "escape" || w.batch.is_none() || w.search_open || w.rename.is_some() {
                    return;
                }
                if w.row_menu.is_some() {
                    w.row_menu = None;
                } else {
                    w.batch = None;
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .on_action(cx.listener(|w, _: &FocusSearch, window, cx| {
                if w.search_open { w.close_search(window, cx); } else { w.open_search(window, cx); }
                cx.stop_propagation();
            }))
            .children(sidebar)
            .child(note)
            .children(resize)
            .children(crate::ui::window_controls(window))
            .children(row_menu)
            .children(doc_menu)
            .children(mode_menu)
            .children(doc_info)
            .children(trash_menu)
            .children(home_menus)
            .children(theme_menu)
            .children(font_menu)
            .children(lang_menu)
            .children(pref_menu)
            .children(confirm)
            .children(move_dlg)
            .children(export_md)
            .children(search)
            .children(resize_overlay)
            .children(folder_drag_overlay)
            .when(self.file_history.busy || self.store.file_task_busy(), |d| d.child(deferred(div().id("file-history-pending").absolute().inset_0().occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())).with_priority(20)))
            .children(home_drag_overlay)
            .children(create_menu)
            .children(import_menu)
            .children(place_menu)
            .children(toast)
            // Over everything, toasts included, until its last page is done.
            .when_some(self.onboarding.clone(), |d, o| d.child(deferred(div().id("onboarding-cover").absolute().inset_0().occlude().child(o)).with_priority(30)))
    }
}

impl Workspace {
    /// Lay the first-launch pages over this window; finishing them uncovers it.
    pub(crate) fn replay_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.onboarding.is_some() { return }
        let o = cx.new(|cx| crate::onboarding::Onboarding::replay(window, cx));
        cx.subscribe_in(&o, window, |w, _, _: &crate::onboarding::Done, window, cx| {
            w.onboarding = None;
            // The language may have changed on the first page.
            crate::settings::refresh_all(w, cx);
            window.focus(&w.focus);
            cx.notify();
        }).detach();
        self.onboarding = Some(o);
        cx.notify();
    }
}

/// Fault injection against real window/save code, available only in the harness build.
#[cfg(feature = "devtools")]
#[derive(Clone)]
struct SaveSafetyQa(Vec<(WindowHandle<Workspace>, String)>);
#[cfg(feature = "devtools")]
impl Global for SaveSafetyQa {}

#[cfg(feature = "devtools")]
pub(crate) fn check_save_safety(phase: &str, cx: &mut App) {
    use crate::model::{Document, RichText};
    let doc = |title: &str| Document { title: RichText::plain(title), ..Default::default() };
    if phase == "setup" {
        let mut store = cx.global::<SharedStore>().clone();
        let mut windows = vec![];
        for name in ["A", "B", "C"] {
            let id = store.create_from(&doc(name)).unwrap();
            let open_id = id.clone();
            let handle = cx.open_window(WindowOptions { focus: false, ..Default::default() }, |window, cx| {
                cx.new(|cx| Workspace::new_document_window(Some(open_id), window, cx))
            }).unwrap();
            handle.update(cx, |ws, _, cx| {
                ws.editor.as_ref().unwrap().update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc(&format!("local-{name}"))), cx));
                ws.unsaved = true;
            }).unwrap();
            if name != "C" {
                std::fs::write(store.file_path(&id).unwrap(), crate::docxml::serialize(&doc(&format!("disk-{name}")))).unwrap();
            }
            windows.push((handle, id));
        }
        quit(cx);
        for (handle, _) in &windows[..2] {
            handle.update(cx, |ws, _, _| assert!(ws.unsaved && matches!(ws.confirm, Some(Confirm::SaveFailed)))).unwrap();
        }
        assert_eq!(store.load(&windows[2].1).unwrap().title.text, "local-C");
        windows[0].0.update(cx, |ws, window, cx| {
            ws.resolve_save_failed(true, window, cx);
            assert!(ws.unsaved);
            ws.confirm = None;
            ws.resolve_save_failed(false, window, cx);
            assert!(!ws.unsaved);
            assert_eq!(ws.editor.as_ref().unwrap().read(cx).core.doc.title.text, "disk-A");
        }).unwrap();
        cx.set_global(SaveSafetyQa(windows));
        eprintln!("save-safety: all windows checked; successful sibling saved; retry and reload checked");
    } else {
        let windows = cx.global::<SaveSafetyQa>().0.clone();
        windows[1].0.update(cx, |ws, window, cx| {
            assert!(ws.unsaved && matches!(ws.confirm, Some(Confirm::SaveFailed)));
            ws.after_save = None;
            ws.save_failed_copy(window, cx);
            assert!(!ws.unsaved);
            assert_eq!(ws.editor.as_ref().unwrap().read(cx).core.doc.title.text, "disk-B");
            assert!(ws.store.list().iter().any(|e| e.title.starts_with("local-B")));
        }).unwrap();
        windows[0].0.update(cx, |ws, _, cx| {
            ws.editor.as_ref().unwrap().update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc("after-reload")), cx));
            ws.unsaved = true;
            assert!(ws.flush(cx));
            assert_eq!(ws.store.load(ws.current.as_ref().unwrap()).unwrap().title.text, "after-reload");
        }).unwrap();
        eprintln!("save-safety: PASS; remaining conflict blocked quit; copy preserved; edit after reload saved cleanly");
        for (handle, _) in windows { handle.update(cx, |_, window, _| window.remove_window()).unwrap(); }
    }
}

#[cfg(test)]
mod doc_history_tests {
    use super::{DocHistory, NavTarget, build_buffer_ring};
    use crate::settings::Page;
    use crate::store::{Entry, Meta};

    fn entry(id: &str, parent: Option<&str>, folder: Option<&str>, local: bool) -> Entry {
        let mut meta = Meta::default();
        meta.parent = parent.map(str::to_string);
        meta.folder = folder.map(str::to_string);
        if local {
            meta.path = Some(format!("/tmp/{id}.md"));
        }
        Entry { id: id.to_string(), title: id.to_string(), body: "".into(), icon: None, meta, reminders: vec![], links: vec![] }
    }

    #[test]
    fn doc_history_back_forward_and_branch_truncation() {
        let mut h = DocHistory::default();
        h.record(NavTarget::Doc("a".into()));
        h.record(NavTarget::Doc("a".into()));
        assert_eq!(h.entries, vec![NavTarget::Doc("a".into())]);
        assert_eq!(h.ix, 0);

        h.record(NavTarget::Home);
        h.record(NavTarget::Folder("f1".into()));
        h.record(NavTarget::Settings(Page::General));
        h.record(NavTarget::Trash);
        h.record(NavTarget::Doc("b".into()));
        assert_eq!(h.ix, 5);

        // Jump back across views and documents: Doc(b) -> Trash -> Settings -> Folder(f1) -> Home -> Doc(a).
        for expected in [NavTarget::Trash, NavTarget::Settings(Page::General), NavTarget::Folder("f1".into()), NavTarget::Home, NavTarget::Doc("a".into())] {
            let (ix, target) = h.step(false, |_| true).unwrap();
            assert_eq!(target, expected);
            h.ix = ix;
        }
        assert!(h.step(false, |_| true).is_none());

        // Forward skips deleted doc/folder entries.
        let (ix, target) = h.step(true, |_| true).unwrap();
        assert_eq!((ix, target), (1, NavTarget::Home));
        h.ix = ix;
        let (ix, target) = h.step(true, |t| !matches!(t, NavTarget::Folder(f) if f == "f1")).unwrap();
        assert_eq!((ix, target), (3, NavTarget::Settings(Page::General)));

        // Recording a new branch while at `Doc(a)` truncates forward entries.
        h.ix = 0;
        h.record(NavTarget::Doc("d".into()));
        assert_eq!(h.entries, vec![NavTarget::Doc("a".into()), NavTarget::Doc("d".into())]);
        assert_eq!(h.ix, 1);

        // Recording while navigating is ignored.
        h.navigating = true;
        h.record(NavTarget::Home);
        h.navigating = false;
        assert_eq!(h.entries, vec![NavTarget::Doc("a".into()), NavTarget::Doc("d".into())]);
    }

    #[test]
    fn buffer_ring_covers_nested_unopened_and_local_docs() {
        let f1 = entry("f1", None, Some("f1"), false);
        let d1 = entry("d1", Some("f1"), None, false);
        let d2 = entry("d2", None, None, false);
        let loc = entry("loc", None, None, true);
        let entries = vec![f1, d1, d2, loc.clone()];
        let ring = build_buffer_ring(&entries, &[&loc]);
        assert_eq!(ring.len(), 3);
        assert!(ring.contains(&"d1".to_string()));
        assert!(ring.contains(&"d2".to_string()));
        assert!(ring.contains(&"loc".to_string()));
        assert!(!ring.contains(&"f1".to_string()));
    }
}
