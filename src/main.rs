#![recursion_limit = "512"]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod shared_store;
mod quick_note;
mod assets;
mod guides;
mod clipboard;
mod devtools;
mod docinfo;
mod docmenu;
mod editor;
mod highlight;
mod home;
mod html;
mod input;
mod modes;
#[cfg(target_os = "macos")]
mod native_header;
#[cfg(not(target_os = "macos"))]
#[path = "native_header_fallback.rs"]
mod native_header;
mod printing;
mod search;
mod sidebar;
mod spaces;
mod settings;
mod themes;
mod shortcuts;
#[allow(dead_code, unused_imports)]
mod feishu_theme;
#[allow(dead_code, unused_imports)]
mod editor_theme;
#[allow(dead_code, unused_imports)]
mod editor_ui;
mod theme;
mod transfer;
mod trash;
mod tree;
mod ui;
mod update;

use fdl_core::{docxml, lang, markdown, model, palette, paste, store, time};

use gpui::*;

#[cfg(target_os = "macos")]
fn set_app_icon() {
    use objc::{class, msg_send, sel, sel_impl};
    use objc::runtime::Object;
    let png = include_bytes!("../assets/branding/app-icon.png");
    unsafe {
        let data: *mut Object = msg_send![class!(NSData), dataWithBytes:png.as_ptr() length:png.len()];
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData:data];
        if !image.is_null() {
            let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, setApplicationIconImage:image];
            let _: () = msg_send![image, release];
        }
    }
}

fn main() {
    let scripted = std::env::var_os("FDL_SCRIPT").is_some();
    // No single-instance guard: copies may share a library (builds tested
    // side by side). `Store` rereads an index another copy rewrote before
    // writing its own, and `guides` seeds a new library only once.
    spaces::load();
    if !scripted && let Err(err) = guides::initialize(&store::Store::base_dir()) {
        eprintln!("无法初始化开始指南：{err:#}");
        return;
    }
    let app = Application::new().with_assets(assets::Assets);
    // macOS keeps running after ⌘W closes the last window (quick notes stay
    // available); clicking the Dock icon then brings the main window back.
    app.on_reopen(|cx| if cx.windows().is_empty() { app::open_main_window(None, None, cx); });
    app.run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        set_app_icon();
        editor::math::load_fonts(cx);
        input::bind_keys(cx);
        editor::keymap::bind_keys(cx);
        editor::image::bind_keys(cx);
        app::bind_keys(cx);
        spaces::bind_keys(cx);
        settings::load();
        settings::apply_appearance(&settings::prefs().appearance);
        settings::sync_dark(cx);
        themes::load(&settings::prefs().theme_id(), false, || cx.text_system().all_font_names());
        shortcuts::init(cx);
        // A space whose folder is gone (an unplugged disk …) opens the default library.
        let (_, store, _) = spaces::open(&spaces::last_id(), cx).expect("open document library");
        let ui = spaces::store_for(spaces::DEFAULT, cx).unwrap_or_else(|_| store.clone());
        cx.set_global(spaces::UiStore(ui));
        cx.set_global(store);
        let quick_enabled = !scripted;
        if quick_enabled { quick_note::install(cx); }
        app::set_menus(cx);
        update::init(cx);
        app::open_main_window(None, None, cx);
        cx.on_window_closed(move |cx| {
            if (!quick_enabled || !cfg!(target_os = "macos")) && cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        if !scripted {
            cx.activate(true);
        }
    });
}
