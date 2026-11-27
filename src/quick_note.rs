use gpui::*;

actions!(quick_note, [QuickNew]);

#[derive(Default)]
struct QuickWindows(Vec<WindowHandle<crate::app::Workspace>>);
impl Global for QuickWindows {}

pub fn open(cx: &mut App) { open_document(None, None, cx); }

#[cfg(target_os = "macos")]
#[derive(Default)]
struct LastDocument(Option<String>);
#[cfg(target_os = "macos")]
impl Global for LastDocument {}
#[cfg(target_os = "macos")]
pub fn used(id: &str, cx: &mut App) { cx.set_global(LastDocument(Some(id.into()))); }
#[cfg(not(target_os = "macos"))]
pub fn used(_: &str, _: &mut App) {}
#[cfg(target_os = "macos")]
pub fn open_last(cx: &mut App) {
    let Some(mut store) = cx.try_global::<crate::shared_store::SharedStore>().cloned() else { return };
    let entries: Vec<_> = store.list().into_iter().filter(|e| e.meta.folder.is_none()).collect();
    let id = cx.try_global::<LastDocument>().and_then(|s| s.0.clone()).filter(|id| entries.iter().any(|e| &e.id == id))
        .or_else(|| entries.iter().max_by_key(|e| e.meta.opened).map(|e| e.id.clone()));
    let Some(id) = id else { return };
    show_document(id, None, cx);
}

/// The window showing document `id` comes forward; none does: a window of its own,
/// sized after the main window it was asked from (`opener`).
pub fn show_document(id: String, opener: Option<Opener>, cx: &mut App) {
    for handle in cx.windows() {
        if let Some(handle) = handle.downcast::<crate::app::Workspace>()
            && handle.update(cx, |w, window, _| {
                if w.current.as_ref() != Some(&id) { return false; }
                // A scripted run never takes the keyboard.
                if !crate::devtools::scripted() { window.activate_window(); }
                true
            }).unwrap_or(false)
        { if !crate::devtools::scripted() { cx.activate(true); } return; }
    }
    open_document(Some(id), opener, cx);
}

/// What a window opened from a main window (在新窗口中打开) takes from it: its size, and its
/// sidebar, open (how wide) or not. The new window's own sidebar is its own afterwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opener { pub size: [f32; 2], pub sidebar: Option<f32> }

/// Whose size a window's frame is written down as when it changes (`remember`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frame {
    /// The main window: its size and its spot (`window_size`, `window_origin`).
    Main,
    /// 快速笔记 / 快捷新建: `quick_note_size`.
    Quick,
    /// A document in a window of its own: sized after a main window, writes nothing down.
    Document,
}

impl crate::app::Workspace {
    /// What a window this one opens takes from it; only a main window passes anything on.
    pub(crate) fn opener(&self, window: &Window) -> Option<Opener> {
        if self.frame != Frame::Main { return None }
        let s = window.window_bounds().get_bounds().size;
        // In 专注模式 or too narrow, the sidebar is only away for now: as it will be again.
        let open = self.focus_mode.sidebar_before().unwrap_or(self.sidebar_open || self.sidebar_auto_hidden);
        Some(Opener { size: [f32::from(s.width), f32::from(s.height)], sidebar: open.then_some(self.sidebar_width) })
    }
}

/// A new window's sidebar, open or not and how wide: the opener's; nothing to go by, closed at
/// `width` (the last one dragged to).
pub(crate) fn new_window_sidebar(opener: Option<Opener>, width: f32) -> (bool, f32) {
    match opener.and_then(|o| o.sidebar) { Some(w) => (true, w), None => (false, width) }
}

/// A new window's size: a document's that of the main window it was opened from, else the main
/// window's last (`main`); a quick note's its own last (`quick`), first 640 × 800.
fn new_window_size(document: bool, opener: Option<Opener>, main: [f32; 2], quick: Option<[f32; 2]>) -> [f32; 2] {
    if document { opener.map_or(main, |o| o.size) } else { quick.unwrap_or([640., 800.]) }
}

/// `document` in a window of its own (`None`: a new quick note).
pub fn open_document(document: Option<String>, opener: Option<Opener>, cx: &mut App) {
    let stored = cx.try_global::<crate::shared_store::SharedStore>().and_then(|s| s.settings().quick_note_size);
    let main = cx.try_global::<Sizes>().and_then(|s| s.main).unwrap_or_else(|| { let s = main_window_size(cx); [f32::from(s.width), f32::from(s.height)] });
    let remembered = new_window_size(document.is_some(), opener, main, cx.try_global::<Sizes>().and_then(|s| s.quick).or(stored));
    let mut previous = None;
    let handles = cx.try_global::<QuickWindows>().map(|w| w.0.clone()).unwrap_or_default();
    let mut alive = Vec::new();
    for handle in handles {
        if let Ok(b) = handle.update(cx, |_, window, cx| (window.bounds(), window.display(cx))) {
            previous = Some(b);
            alive.push(handle);
        }
    }
    cx.set_global(QuickWindows(alive));
    let display = previous.as_ref().and_then(|(_, d)| d.clone()).or_else(|| cx.primary_display());
    let mut bounds = Bounds::centered(display.as_ref().map(|d| d.id()), size(px(remembered[0].max(480.)), px(remembered[1].max(480.))), cx);
    if let Some(display) = display {
        let screen = display.visible_bounds();
        bounds.size.width = bounds.size.width.min(screen.size.width - px(32.));
        bounds.size.height = bounds.size.height.min(screen.size.height - px(64.));
        bounds.origin = screen.origin + point((screen.size.width - bounds.size.width) / 2., (screen.size.height - bounds.size.height) / 2.);
        if let Some((last, _)) = previous {
            let next = last.origin + point(px(28.), px(28.));
            if next.x + bounds.size.width <= screen.right() - px(8.) && next.y + bounds.size.height <= screen.bottom() - px(8.) { bounds.origin = next; }
        }
    }
    // Whole points: see `main_window_options`.
    bounds.origin = bounds.origin.map(|v| v.round());
    persist(cx);
    let result = cx.open_window(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_background: crate::theme::window_background(),
        app_id: Some(crate::APP_ID.into()),
        window_decorations: if cfg!(target_os = "linux") { Some(WindowDecorations::Client) } else { None },
        titlebar: Some(TitlebarOptions {
            title: Some(crate::settings::text("Quiet.md · 快速笔记").into()),
            appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
            traffic_light_position: Some(crate::focus::lights_at()),
        }),
        window_min_size: Some(size(px(480.), px(480.))),
        app_owns_titlebar_drag: true,
        focus: !crate::devtools::scripted(),
        ..Default::default()
    }, |window, cx| { crate::devtools::opened(window, cx); cx.new(|cx| crate::app::Workspace::new_document_window(document, opener, window, cx)) });
    match result {
        Ok(handle) => { cx.global_mut::<QuickWindows>().0.push(handle); if !crate::devtools::scripted() { cx.activate(true); } },
        Err(e) => eprintln!("无法打开快速笔记：{e}"),
    }
}

#[cfg(target_os = "macos")]
mod hotkey {
    use super::*;
    use std::{cell::RefCell, ffi::c_void, ptr};
    type Ref = *mut c_void;
    #[repr(C)] struct EventType { class: u32, kind: u32 }
    #[repr(C)] struct HotkeyId { signature: u32, id: u32 }
    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> Ref;
        fn GetEventKind(event: Ref) -> u32;
        fn GetEventParameter(event: Ref, name: u32, kind: u32, actual: *mut u32, size: usize, actual_size: *mut usize, data: Ref) -> i32;
        fn InstallEventHandler(target: Ref, handler: unsafe extern "C" fn(Ref, Ref, Ref) -> i32, count: u32, types: *const EventType, data: Ref, out: *mut Ref) -> i32;
        fn RegisterEventHotKey(key: u32, modifiers: u32, id: HotkeyId, target: Ref, options: u32, out: *mut Ref) -> i32;
        fn UnregisterEventHotKey(key: Ref) -> i32;
    }
    thread_local! { static KEY: RefCell<(Ref, String)> = const { RefCell::new((ptr::null_mut(), String::new())) }; static LAST_KEY: RefCell<(Ref, String)> = const { RefCell::new((ptr::null_mut(), String::new())) }; static HELD: RefCell<[bool; 2]> = const { RefCell::new([false; 2]) }; static APP: RefCell<Option<AsyncApp>> = const { RefCell::new(None) }; }
    unsafe extern "C" fn pressed(_: Ref, event: Ref, _: Ref) -> i32 {
        let mut id = HotkeyId { signature: 0, id: 0 };
        if unsafe { GetEventParameter(event, u32::from_be_bytes(*b"----"), u32::from_be_bytes(*b"hkid"), ptr::null_mut(), std::mem::size_of::<HotkeyId>(), ptr::null_mut(), &mut id as *mut _ as Ref) } != 0 || id.signature != u32::from_be_bytes(*b"FlNt") || !(1..=2).contains(&id.id) { return -9874; }
        let release = unsafe { GetEventKind(event) } == 6;
        let repeated = HELD.with(|held| { let mut held = held.borrow_mut(); let old = held[(id.id - 1) as usize]; held[(id.id - 1) as usize] = !release; old });
        if release || repeated { return 0; }
        APP.with(|app| {
            if let Some(cx) = app.borrow().clone() {
                cx.foreground_executor().clone().spawn(async move { cx.update(|cx| if id.id == 2 { super::open_last(cx) } else { super::open(cx) }); }).detach();
            }
        });
        0
    }
    pub fn suspend() {
        HELD.with(|h| *h.borrow_mut() = [false; 2]);
        for slot in [&KEY, &LAST_KEY] { slot.with(|k| { let mut k = k.borrow_mut(); if !k.0.is_null() { unsafe { UnregisterEventHotKey(k.0); } } *k = (ptr::null_mut(), String::new()); }); }
    }
    /// Hotkey `id` (1 快速笔记, 2 打开上次的文档) on `combo`; an empty `combo` turns it off.
    pub fn install(cx: &mut App, id: u32, code: u32, modifiers: u32, combo: &str) -> Result<(), i32> {
        let slot = if id == 2 { &LAST_KEY } else { &KEY };
        if slot.with(|k| k.borrow().1 == combo) { return Ok(()); }
        unsafe {
            let target = GetApplicationEventTarget();
            if APP.with(|a| a.borrow().is_none()) {
                let events = [EventType { class: u32::from_be_bytes(*b"keyb"), kind: 5 }, EventType { class: u32::from_be_bytes(*b"keyb"), kind: 6 }];
                let mut handler = ptr::null_mut();
                let status = InstallEventHandler(target, pressed, 2, events.as_ptr(), ptr::null_mut(), &mut handler);
                if status != 0 { return Err(status); }
                APP.with(|app| *app.borrow_mut() = Some(cx.to_async()));
            }
            let mut key = ptr::null_mut();
            if !combo.is_empty() {
                let status = RegisterEventHotKey(code, modifiers, HotkeyId { signature: u32::from_be_bytes(*b"FlNt"), id }, target, 0, &mut key);
                if status != 0 { return Err(status); }
            }
            slot.with(|k| { let mut k = k.borrow_mut(); if !k.0.is_null() { UnregisterEventHotKey(k.0); } *k = (key, combo.into()); });
            Ok(())
        }
    }
}

pub fn install(cx: &mut App) -> bool {
    cx.on_action(|_: &QuickNew, cx| open(cx));
    // Settings › 快捷键 shows a failure under its row; no alert on every launch.
    register(cx)
}

/// Why each global hotkey (快速笔记, 打开上次的文档) isn't registered, if it failed.
static HOTKEY_ERROR: std::sync::Mutex<[Option<String>; 2]> = std::sync::Mutex::new([None, None]);
pub fn hotkey_error(id: u32) -> Option<String> { HOTKEY_ERROR.lock().unwrap()[id as usize - 1].clone() }
/// Register the global hotkeys from the shortcuts file, each on its own: a cleared one is let go.
/// Whether 快速笔记's went through.
pub fn register(cx: &mut App) -> bool {
    let r = set_hotkey(&crate::shortcuts::global_key(), cx);
    if let Err(e) = &r { eprintln!("全局快捷键注册失败：{e}"); }
    if let Err(e) = set_last_hotkey(&crate::shortcuts::last_key(), cx) { eprintln!("全局快捷键注册失败：{e}"); }
    r.is_ok()
}

/// Window sizes seen this session (quick note and main), saved on quit.
#[derive(Clone, Debug, Default, PartialEq)]
struct Sizes { quick: Option<[f32; 2]>, main: Option<[f32; 2]>, main_pos: Option<([f32; 2], Option<String>)> }
impl Global for Sizes {}
pub fn remember(window: &Window, frame: Frame, cx: &mut App) {
    if frame == Frame::Document { return }
    if let WindowBounds::Windowed(bounds) = window.window_bounds() {
        // The display's UUID: a `DisplayId` is only meaningful inside one
        // run, so it cannot be the thing we write down.
        let display = if frame == Frame::Main { window.display(cx).and_then(|d| d.uuid().ok()).map(|u| u.to_string()) } else { None };
        let sizes = cx.try_global::<Sizes>().cloned().unwrap_or_default();
        cx.set_global(remembered(sizes, frame, bounds, display));
    }
}
/// `sizes` with a window's new frame written down as its `frame` says.
fn remembered(mut sizes: Sizes, frame: Frame, bounds: gpui::Bounds<Pixels>, display: Option<String>) -> Sizes {
    let s = [f32::from(bounds.size.width), f32::from(bounds.size.height)];
    match frame {
        Frame::Quick => sizes.quick = Some(s),
        Frame::Main => { sizes.main = Some(s); sizes.main_pos = Some(([f32::from(bounds.origin.x), f32::from(bounds.origin.y)], display)); }
        Frame::Document => {}
    }
    sizes
}
pub fn persist(cx: &mut App) {
    let (Some(sizes), Some(store)) = (cx.try_global::<Sizes>().cloned(), cx.try_global::<crate::shared_store::SharedStore>()) else { return };
    let mut settings = store.settings();
    if sizes.quick.is_some() { settings.quick_note_size = sizes.quick }
    if sizes.main.is_some() { settings.window_size = sizes.main }
    if let Some((o, d)) = sizes.main_pos { settings.window_origin = Some(o); settings.window_display = d; }
    if let Err(e) = store.save_settings(&settings) { eprintln!("无法保存窗口尺寸：{e}"); }
}

/// The main window's frame, and the display whose corner it is measured from:
/// the remembered spot clamped into that display's work area (clear of the menu
/// bar and Dock), else centred in the primary display's (#253). Before there is
/// a library there is nothing remembered, and the first window's size centres
/// in the same area.
pub fn main_window_bounds(cx: &App) -> (Option<DisplayId>, gpui::Bounds<Pixels>) {
    let wanted = main_window_size(cx);
    let saved = cx.try_global::<crate::shared_store::SharedStore>().map(|s| s.settings());
    let display = saved.as_ref().and_then(|s| s.window_display.as_deref())
        .and_then(|uuid| cx.displays().into_iter().find(|d| d.uuid().ok().is_some_and(|u| u.to_string() == uuid)))
        .or_else(|| cx.primary_display());
    let Some(display) = display else { return (None, gpui::Bounds { origin: point(px(0.), px(0.)), size: wanted }) };
    let usable = display.visible_bounds();
    let size = wanted.min(&usable.size);
    let mut bounds = match saved.as_ref().and_then(|s| s.window_origin) {
        Some(o) => gpui::Bounds { origin: point(px(o[0]), px(o[1])), size },
        None => Bounds::centered_at(usable.center(), size),
    };
    // The Dock may sit somewhere else since the position was saved, or the
    // display changed; pull the frame back inside the work area.
    let clamp = |o: Pixels, low: Pixels, high: Pixels| px(f32::from(o).max(f32::from(low)).min(f32::from(high)));
    bounds.origin.x = clamp(bounds.origin.x, usable.origin.x, usable.right() - bounds.size.width);
    bounds.origin.y = clamp(bounds.origin.y, usable.origin.y, usable.bottom() - bounds.size.height);
    (Some(display.id()), bounds)
}

/// The main window's size: last used, or 1280 × 800 (also before there is a library).
pub fn main_window_size(cx: &App) -> gpui::Size<Pixels> {
    let s = cx.try_global::<crate::shared_store::SharedStore>().and_then(|s| s.settings().window_size).unwrap_or_else(|| first_window_size(cx));
    size(px(s[0].max(480.)), px(s[1].max(480.)))
}

/// The first window, before the user has sized one: 1280×800. At 16:10 the
/// first-launch film fills it with little cropped, and the 280pt sidebar
/// leaves the notes 1000. A screen too small for it gets the same shape,
/// shrunk to leave room for the menu bar and the Dock.
/// `QUIET_MD_WINDOW=1280x800` sets it instead, for previews at a given size.
fn first_window_size(cx: &App) -> [f32; 2] {
    if let Some((w, h)) = std::env::var("QUIET_MD_WINDOW").ok().and_then(|v| { let (w, h) = v.split_once('x')?; Some((w.parse().ok()?, h.parse().ok()?)) }) { return [w, h] }
    let Some(d) = cx.primary_display().map(|d| d.bounds().size) else { return [1280., 800.] };
    let k = ((f32::from(d.width) - 40.) / 1280.).min(f32::from(d.height) * 0.84 / 800.).min(1.);
    [(1280. * k).round(), (800. * k).round()]
}

pub fn set_hotkey(combo: &str, cx: &mut App) -> anyhow::Result<()> { set(1, combo, cx) }
/// 打开上次使用的文档; empty: off.
pub fn set_last_hotkey(combo: &str, cx: &mut App) -> anyhow::Result<()> { set(2, combo, cx) }

#[cfg(target_os = "macos")]
fn set(id: u32, combo: &str, cx: &mut App) -> anyhow::Result<()> {
    if combo.is_empty() { let _ = hotkey::install(cx, id, 0, 0, ""); HOTKEY_ERROR.lock().unwrap()[id as usize - 1] = None; return Ok(()); }
    let key = crate::shortcuts::parse_key(combo)?;
    let keys = [("a",0),("s",1),("d",2),("f",3),("h",4),("g",5),("z",6),("x",7),("c",8),("v",9),("b",11),("q",12),("w",13),("e",14),("r",15),("y",16),("t",17),("1",18),("2",19),("3",20),("4",21),("6",22),("5",23),("9",25),("7",26),("8",28),("0",29),("o",31),("u",32),("i",34),("p",35),("l",37),("j",38),("k",40),("n",45),("m",46),("space",49)];
    let code = keys.iter().find(|(k,_)| *k == key.key).map(|(_,v)| *v).ok_or_else(|| anyhow::anyhow!(crate::settings::tr("全局快捷键请使用字母、数字或空格", "Use a letter, digit or Space for the global shortcut")))?;
    let m = key.modifiers;
    let modifiers = (if m.platform { 1 << 8 } else { 0 }) | (if m.shift { 1 << 9 } else { 0 }) | (if m.alt { 1 << 11 } else { 0 }) | (if m.control { 1 << 12 } else { 0 });
    #[cfg(target_os = "macos")]
    let r = hotkey::install(cx, id, code, modifiers, combo).map_err(|e| anyhow::anyhow!("{}（{e}）", crate::settings::tr("无法注册这个组合键，可能已被系统或其他应用占用", "This shortcut is taken by macOS or another app")));
    HOTKEY_ERROR.lock().unwrap()[id as usize - 1] = r.as_ref().err().map(|e| e.to_string());
    r
}

pub fn suspend_hotkey() { #[cfg(target_os = "macos")] hotkey::suspend(); *HOTKEY_ERROR.lock().unwrap() = [None, None]; }

#[cfg(not(target_os = "macos"))]
fn set(id: u32, combo: &str, _: &mut App) -> anyhow::Result<()> {
    if combo.is_empty() { HOTKEY_ERROR.lock().unwrap()[id as usize - 1] = None; return Ok(()); }
    let error = crate::settings::tr("当前系统暂不支持全局快捷键", "Global shortcuts are not supported on this platform yet");
    HOTKEY_ERROR.lock().unwrap()[id as usize - 1] = Some(error.into());
    Err(anyhow::anyhow!(error))
}

#[cfg(test)]
mod tests {
    use super::{Frame, Opener, Sizes, new_window_sidebar, new_window_size, remembered};
    use gpui::{point, px, size};
    const MAIN: [f32; 2] = [1280., 800.];

    #[test]
    fn a_document_window_takes_the_main_windows_size() {
        let opener = Opener { size: [1500., 900.], sidebar: None };
        // the main window it was opened from, else the main window's last; never the quick note's
        assert_eq!(new_window_size(true, Some(opener), MAIN, Some([640., 800.])), [1500., 900.]);
        assert_eq!(new_window_size(true, None, MAIN, Some([640., 800.])), MAIN);
        // a quick note keeps its own
        assert_eq!(new_window_size(false, Some(opener), MAIN, Some([700., 600.])), [700., 600.]);
        assert_eq!(new_window_size(false, None, MAIN, None), [640., 800.]);
    }

    #[test]
    fn a_document_window_takes_the_main_windows_sidebar() {
        let open = Opener { size: MAIN, sidebar: Some(320.) };
        assert_eq!(new_window_sidebar(Some(open), 280.), (true, 320.));
        assert_eq!(new_window_sidebar(Some(Opener { sidebar: None, ..open }), 280.), (false, 280.));
        // opened from no main window (a global shortcut, a file from Finder): closed, as before
        assert_eq!(new_window_sidebar(None, 280.), (false, 280.));
    }

    #[test]
    fn only_its_own_kind_of_window_writes_a_size_down() {
        let at = |x: f32, w: f32| gpui::Bounds { origin: point(px(x), px(40.)), size: size(px(w), px(700.)) };
        let start = Sizes { quick: Some([640., 800.]), main: Some(MAIN), main_pos: Some(([10., 20.], Some("d".into()))) };
        // a document's window resized or moved: neither the quick note's size nor the main window's frame
        assert_eq!(remembered(start.clone(), Frame::Document, at(300., 900.), None), start);
        let quick = remembered(start.clone(), Frame::Quick, at(300., 900.), None);
        assert_eq!((quick.quick, quick.main, quick.main_pos.clone()), (Some([900., 700.]), start.main, start.main_pos.clone()));
        let main = remembered(start.clone(), Frame::Main, at(300., 1000.), Some("e".into()));
        assert_eq!((main.quick, main.main, main.main_pos), (start.quick, Some([1000., 700.]), Some(([300., 40.], Some("e".into())))));
    }
}
