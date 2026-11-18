//! Document → one self-contained HTML page (delegates to `fdl_core::html`
//! with the app's light-mode Craft theme colours and cached remote images).

use crate::model::*;
use crate::theme;
use fdl_core::palette::Rgba as CoreRgba;
use gpui::Hsla;
use std::ops::Range;
use std::path::PathBuf;

pub type Options<'a> = fdl_core::html::Options<'a>;

struct AppStyle;

fn hsla_to_core(c: Hsla) -> CoreRgba {
    let c = c.to_rgb();
    CoreRgba {
        r: (c.r * 255.).round().clamp(0., 255.) as u8,
        g: (c.g * 255.).round().clamp(0., 255.) as u8,
        b: (c.b * 255.).round().clamp(0., 255.) as u8,
        a: c.a,
    }
}

impl fdl_core::html::StyleSource for AppStyle {
    fn library_root(&self) -> PathBuf { crate::store::Store::root_dir() }
    fn text_color(&self, name: &str) -> Option<CoreRgba> { theme::text_color(name).map(hsla_to_core) }
    fn bg_color(&self, name: &str) -> Option<CoreRgba> { theme::bg_color(name).map(hsla_to_core) }
    fn block_bg(&self, name: &str) -> Option<CoreRgba> { theme::block_bg(name).map(hsla_to_core) }
    fn callout_bg(&self, name: &str) -> Option<CoreRgba> { theme::callout_bg(name).map(hsla_to_core) }
    fn callout_border(&self, name: &str) -> Option<CoreRgba> { theme::callout_border(name).map(hsla_to_core) }
    fn code_spans(&self, lang: &str, text: &str) -> Vec<(Range<usize>, CoreRgba, bool)> {
        crate::highlight::spans(lang, text).into_iter().map(|(r, c, it)| (r, hsla_to_core(c), it)).collect()
    }
    fn cached_remote(&self, url: &str) -> Option<PathBuf> { crate::editor::image::cached_remote(url) }
    fn tr(&self, zh: &'static str, en: &'static str) -> &'static str { crate::settings::tr(zh, en) }
    fn text(&self, zh: &'static str) -> &'static str { crate::settings::text(zh) }
    fn display_title(&self, title: &str) -> String { crate::settings::display_title(title) }
    fn default_callout_fill(&self) -> CoreRgba { hsla_to_core(theme::c(theme::craft::O50)) }
    fn default_callout_line(&self) -> CoreRgba { hsla_to_core(theme::c(theme::craft::O200)) }
}

/// The document as a standalone page, always in the light colours (it is printed or opened elsewhere).
pub fn to_html(doc: &Document, opts: &Options) -> String {
    crate::theme::light(|| fdl_core::html::render(doc, opts, &AppStyle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_reject_active_data_and_unsafe_links() {
        let root = std::path::Path::new(".");
        assert!(fdl_core::html::data_uri("data:text/html;base64,PHNjcmlwdD4=", root, &AppStyle).is_none());
        assert!(fdl_core::html::data_uri("data:image/svg+xml;base64,PHN2Zz4=", root, &AppStyle).is_none());
        assert!(fdl_core::html::data_uri("data:image/png;base64,\" onerror=\"alert(1)", root, &AppStyle).is_none());
        assert!(fdl_core::html::data_uri("data:image/png;base64,aGVsbG8=", root, &AppStyle).is_some());
        let mut text = RichText::default();
        text.push("click", Marks { link: Some("javascript:alert(1)".into()), ..Default::default() });
        assert_eq!(fdl_core::html::inline(&text, &AppStyle), "click");
        assert_eq!(fdl_core::html::esc("'`\"<>"), "&#39;&#96;&quot;&lt;&gt;");
    }

    #[test]
    fn renders_blocks_and_marks() {
        let mut t = RichText::plain("a ");
        t.push("b", Marks { bold: true, text_color: Some(ColorName("red".into())), ..Default::default() });
        let doc = Document {
            title: RichText::plain("T"),
            blocks: vec![
                Block::new(BlockKind::Text, t),
                Block::new(BlockKind::Ordered { seq: None }, RichText::plain("x")),
                Block::new(BlockKind::Ordered { seq: None }, RichText::plain("y")),
                Block::new(BlockKind::SubPageList, RichText::default()),
                Block::new(BlockKind::Raw { tag: "iframe".into(), xml: "<iframe url=\"https://e.x\"/>".into() }, RichText::default()),
            ],
            ..Default::default()
        };
        let h = to_html(&doc, &Options { sub_pages: &["子页".into()] });
        assert!(h.contains("a <b><span style=\"color:#cb0424;\">b</span></b>"));
        assert!(h.contains(">2.</span>"));
        assert!(h.contains("📄 子页"));
        assert!(h.contains("href=\"https://e.x\""));
    }
}
