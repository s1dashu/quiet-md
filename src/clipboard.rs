//! The system pasteboard beyond what GPUI reads.

/// Devtools stand-in for the pasteboard: scripted tests
/// must not touch the shared system clipboard.
#[derive(Clone, Default)]
pub struct FakeClipboard {
    pub html: Option<String>,
    pub text: String,
    pub images: Vec<gpui::Image>,
}

pub static FAKE_CLIPBOARD: std::sync::Mutex<Option<FakeClipboard>> = std::sync::Mutex::new(None);

/// The HTML other apps copied beside the text, `item` as read from the
/// clipboard. macOS: the `public.html` flavor of the general pasteboard
/// (gpui reads only plain text and images there).
#[cfg(target_os = "macos")]
pub fn clipboard_html(_item: Option<&gpui::ClipboardItem>) -> Option<String> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    const TYPE: &str = "public.html";
    unsafe {
        let pb: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
        let ty: *mut Object = msg_send![class!(NSString), alloc];
        let ty: *mut Object = msg_send![ty,
            initWithBytes: TYPE.as_ptr()
            length: TYPE.len()
            encoding: 4usize]; // NSUTF8StringEncoding
        let s: *mut Object = msg_send![pb, stringForType: ty];
        let _: () = msg_send![ty, release];
        if s.is_null() {
            return None;
        }
        let p: *const std::os::raw::c_char = msg_send![s, UTF8String];
        (!p.is_null()).then(|| std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned())
    }
}

/// Windows: `HTML Format` (CF_HTML), its fragment; Linux: the `text/html`
/// target (X11 and Wayland). The vendored gpui reads them with the text.
#[cfg(not(target_os = "macos"))]
pub fn clipboard_html(item: Option<&gpui::ClipboardItem>) -> Option<String> {
    let html = item?.entries().iter().find_map(|e| match e {
        gpui::ClipboardEntry::String(s) => s.html.as_deref(),
        _ => None,
    })?;
    Some(fdl_core::clip_html::cf_html_fragment(html)).filter(|h| !h.trim().is_empty())
}

/// Our copy for the clipboard: the text, the blocks as DocxXML for pasting
/// back here and, on Windows, as CF_HTML for Word and browsers.
pub fn copied(text: String, xml: String, _blocks: &[fdl_core::model::Block]) -> gpui::ClipboardItem {
    #[allow(unused_mut)]
    let mut s = gpui::ClipboardString::new(text).with_json_metadata(xml);
    #[cfg(target_os = "windows")]
    {
        let html = fdl_core::clip_html::blocks_to_html(_blocks, &crate::store::Store::active_dir());
        s = s.with_html(fdl_core::clip_html::cf_html(&html));
    }
    gpui::ClipboardItem { entries: vec![gpui::ClipboardEntry::String(s)] }
}
