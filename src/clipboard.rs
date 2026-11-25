//! The system pasteboard beyond what GPUI reads.

/// Devtools stand-in for the pasteboard: scripted tests
/// must not touch the shared system clipboard.
#[derive(Clone, Default)]
pub struct FakeClipboard {
    pub html: Option<String>,
    pub text: String,
    pub images: Vec<gpui::Image>,
    pub files: Vec<std::path::PathBuf>,
}

pub static FAKE_CLIPBOARD: std::sync::Mutex<Option<FakeClipboard>> = std::sync::Mutex::new(None);

/// The `public.html` flavor of the general pasteboard (gpui reads only
/// plain text and images).
#[cfg(target_os = "macos")]
pub fn clipboard_html() -> Option<String> {
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

#[cfg(not(target_os = "macos"))]
pub fn clipboard_html() -> Option<String> {
    None
}

/// Native file copies can also contain a rendered Finder icon. Read the
/// URLs independently of GPUI's image fallback and legacy filenames flavor.
#[cfg(target_os = "macos")]
pub fn clipboard_files() -> Vec<std::path::PathBuf> {
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let pb: *mut objc::runtime::Object = msg_send![class!(NSPasteboard), generalPasteboard];
        files_from_pasteboard(pb)
    }
}

#[cfg(target_os = "macos")]
unsafe fn files_from_pasteboard(pb: *mut objc::runtime::Object) -> Vec<std::path::PathBuf> {
    use objc::runtime::{BOOL, Object, YES};
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let classes: *mut Object = msg_send![class!(NSArray), arrayWithObject: class!(NSURL)];
        let urls: *mut Object = msg_send![pb, readObjectsForClasses: classes options: std::ptr::null_mut::<Object>()];
        let count: usize = msg_send![urls, count];
        let mut paths = Vec::new();
        for i in 0..count {
            let url: *mut Object = msg_send![urls, objectAtIndex: i];
            let local: BOOL = msg_send![url, isFileURL];
            if local != YES { continue; }
            let path: *mut Object = msg_send![url, path];
            let s: *const std::os::raw::c_char = msg_send![path, UTF8String];
            if !s.is_null() { paths.push(std::path::PathBuf::from(std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned())); }
        }
        paths
    }
}

#[cfg(not(target_os = "macos"))]
pub fn clipboard_files() -> Vec<std::path::PathBuf> { Vec::new() }

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc::runtime::{BOOL, Object, YES};
    use objc::{class, msg_send, sel, sel_impl};

    #[test]
    fn native_file_urls_survive_an_icon_fallback() {
        objc::rc::autoreleasepool(|| unsafe {
            // A private pasteboard: never read or change the user's clipboard.
            let pb: *mut Object = msg_send![class!(NSPasteboard), pasteboardWithUniqueName];
            let item: *mut Object = msg_send![class!(NSPasteboardItem), new];
            let ns = crate::printing::native::ns_string;
            let ok: BOOL = msg_send![item, setString: ns("file:///tmp/%E8%A7%86%E9%A2%91%20clip.mp4") forType: ns("public.file-url")];
            assert_eq!(ok, YES);
            let ok: BOOL = msg_send![item, setString: ns("Finder icon fallback") forType: ns("public.utf8-plain-text")];
            assert_eq!(ok, YES);
            let icon = include_bytes!("../assets/branding/app-icon-dev.png");
            let data: *mut Object = msg_send![class!(NSData), dataWithBytes: icon.as_ptr() length: icon.len()];
            let ok: BOOL = msg_send![item, setData: data forType: ns("public.png")];
            assert_eq!(ok, YES);
            let items: *mut Object = msg_send![class!(NSArray), arrayWithObject: item];
            let ok: BOOL = msg_send![pb, writeObjects: items];
            assert_eq!(ok, YES);
            assert_eq!(super::files_from_pasteboard(pb), vec![std::path::PathBuf::from("/tmp/视频 clip.mp4")]);
            let _: () = msg_send![item, release];
            let _: () = msg_send![pb, releaseGlobally];
        });
    }
}
