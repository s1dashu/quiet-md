pub(crate) use gpui_wgpu::CosmicTextSystem;

/// quiet.md: the family `.SystemUIFont` stands for, read when the platform
/// starts: fontconfig's match for sans-serif in the locale's language (what
/// `fc-match sans-serif` prints), so it follows the user's and the
/// distribution's settings and covers CJK under a CJK locale. Noto Sans, the
/// family this used to be, only when fontconfig matches nothing.
pub fn system_ui_font_family() -> String {
    fontconfig_sans().unwrap_or_else(|| "Noto Sans".into())
}

fn fontconfig_sans() -> Option<String> {
    use fontconfig_sys::{constants::FC_FAMILY, *};
    use std::ptr::null_mut;
    // SAFETY: FcInit loads the default configuration that the null config
    // pointers below refer to; every pattern made here is destroyed here, and
    // the family string is copied before its pattern goes.
    unsafe {
        if FcInit() == 0 {
            return None;
        }
        let pattern = FcNameParse(c"sans-serif".as_ptr().cast());
        if pattern.is_null() {
            return None;
        }
        FcConfigSubstitute(null_mut(), pattern, FcMatchPattern);
        FcDefaultSubstitute(pattern);
        let mut result = FcResultNoMatch;
        let matched = FcFontMatch(null_mut(), pattern, &mut result);
        FcPatternDestroy(pattern);
        if matched.is_null() {
            return None;
        }
        let mut family = null_mut();
        let found = FcPatternGetString(matched, FC_FAMILY.as_ptr(), 0, &mut family) == FcResultMatch;
        let name = (found && !family.is_null())
            .then(|| std::ffi::CStr::from_ptr(family.cast()).to_string_lossy().trim().to_string());
        FcPatternDestroy(matched);
        name.filter(|n| !n.is_empty())
    }
}
