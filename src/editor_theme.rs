pub use crate::theme::{craft, c, ca, rgbaf, ta, TITLE, UI_FONT, MONO_FONT, BlockMetrics};
use crate::settings::TypeContrast;
/// The active theme's resolved style (themes.rs).
pub fn style() -> &'static crate::themes::Style { crate::themes::active() }
fn doc_scale_for(st: &crate::themes::Style) -> f32 {
    let sz = crate::settings::doc_font_size();
    if sz == 0 { 1. } else { sz as f32 / st.num("page.size").max(1.) }
}
fn heading_scale_for(st: &crate::themes::Style) -> f32 {
    let s = doc_scale_for(st);
    if (s - 1.).abs() < 1e-4 { 1. } else if s > 1. { s.powf(0.84) } else { s.powf(0.92) }
}
fn scale_by(v: f32, s: f32) -> f32 { if (s - 1.).abs() < 1e-4 { v } else { (v * s).round() } }
fn contrast_mult(level: u8) -> (f32, f32) {
    match crate::settings::type_contrast() {
        TypeContrast::Compact => match level {
            0 | 1 => (0.90, 0.85),
            2 => (0.92, 0.85),
            3 => (0.94, 0.88),
            _ => (0.96, 0.90),
        },
        TypeContrast::Standard => (1., 1.),
        TypeContrast::Expressive => match level {
            0 => (1.16, 1.),
            1 => (1.14, 1.24),
            2 => (1.09, 1.18),
            3 => (1.05, 1.12),
            4 => (1.02, 1.06),
            _ => (1.00, 1.05),
        },
    }
}
fn h(level: u8) -> u8 { level.clamp(1, 9) }

pub fn body_size_of(st: &crate::themes::Style) -> f32 {
    let sz = crate::settings::doc_font_size();
    if sz == 0 { st.num("page.size") } else { sz as f32 }
}
pub fn body_line_of(st: &crate::themes::Style) -> f32 { scale_by(st.num("page.line"), doc_scale_for(st)) }
pub fn block_gap_of(st: &crate::themes::Style) -> f32 {
    let gm = match crate::settings::type_contrast() { TypeContrast::Compact => 0.88, TypeContrast::Standard => 1., TypeContrast::Expressive => 1.08 };
    scale_by(st.num("p.gap") * gm, doc_scale_for(st))
}
pub fn body_font_of(st: &crate::themes::Style) -> &'static str {
    let f = st.font("page.font");
    if f == crate::themes::SYSTEM { crate::settings::doc_font().unwrap_or(f) } else { f }
}
pub fn title_font_of(st: &crate::themes::Style) -> &'static str {
    let f = st.font("title.font");
    if f == crate::themes::SYSTEM { crate::settings::heading_font().unwrap_or(f) } else { f }
}
pub fn heading_font_of(st: &crate::themes::Style, level: u8) -> &'static str {
    let f = st.font(&format!("h{}.font", h(level)));
    if f == crate::themes::SYSTEM { crate::settings::heading_font().unwrap_or(f) } else { f }
}
pub fn mono_font_of(st: &crate::themes::Style) -> &'static str {
    crate::settings::mono_font().unwrap_or_else(|| st.font("pre.font"))
}
pub fn code_font_of(st: &crate::themes::Style) -> &'static str {
    crate::settings::mono_font().unwrap_or_else(|| st.font("code.font"))
}
pub fn title_of(st: &crate::themes::Style) -> BlockMetrics {
    let hs = heading_scale_for(st);
    let (cm, _) = contrast_mult(0);
    BlockMetrics { size: scale_by(st.num("title.size") * cm, hs), line_height: scale_by(st.num("title.line") * cm, hs), mt: 0., mb: 0. }
}
pub fn title_tracking_of(st: &crate::themes::Style) -> f32 {
    let (cm, _) = contrast_mult(0);
    st.num("title.tracking") * cm * heading_scale_for(st)
}
pub fn heading_metrics_of(st: &crate::themes::Style, level: u8) -> BlockMetrics {
    let l = h(level);
    let hs = heading_scale_for(st);
    let (cm, mm) = contrast_mult(l);
    BlockMetrics {
        size: scale_by(st.num(&format!("h{l}.size")) * cm, hs),
        line_height: scale_by(st.num(&format!("h{l}.line")) * cm, hs),
        mt: scale_by(st.num(&format!("h{l}.mt")) * mm, hs),
        mb: scale_by(st.num(&format!("h{l}.mb")), hs),
    }
}
pub fn heading_tracking_of(st: &crate::themes::Style, level: u8) -> f32 {
    let l = h(level);
    let (cm, _) = contrast_mult(l);
    st.num(&format!("h{l}.tracking")) * cm * heading_scale_for(st)
}
pub fn heading_weight_of(st: &crate::themes::Style, level: u8) -> gpui::FontWeight {
    let w = st.num(&format!("h{}.weight", h(level)));
    let adj = if crate::settings::type_contrast() == TypeContrast::Expressive && level <= 3 { w.max(700.) } else { w };
    gpui::FontWeight(adj)
}
pub fn title_weight_of(st: &crate::themes::Style) -> gpui::FontWeight {
    let w = st.num("title.weight");
    gpui::FontWeight(if crate::settings::type_contrast() == TypeContrast::Expressive { w.max(700.) } else { w })
}
pub fn inline_code_size_of(st: &crate::themes::Style) -> f32 { scale_by(st.num("code.size"), doc_scale_for(st)) }
pub fn pre_size_of(st: &crate::themes::Style) -> f32 { scale_by(st.num("pre.size"), doc_scale_for(st)) }
pub fn pre_line_of(st: &crate::themes::Style) -> f32 { scale_by(st.num("pre.line"), doc_scale_for(st)) }

pub fn body_size() -> f32 { body_size_of(style()) }
pub fn body_line() -> f32 { body_line_of(style()) }
pub fn block_gap() -> f32 { block_gap_of(style()) }
pub fn heading_metrics(level: u8) -> BlockMetrics { heading_metrics_of(style(), level) }
pub fn heading_weight(level: u8) -> gpui::FontWeight { heading_weight_of(style(), level) }
pub fn heading_tracking(level: u8) -> f32 { heading_tracking_of(style(), level) }
pub fn heading_font(level: u8) -> &'static str { heading_font_of(style(), level) }
pub fn heading_color(level: u8) -> Option<gpui::Hsla> { style().color(&format!("h{}.color", h(level))) }
/// The page title.
pub fn title() -> BlockMetrics { title_of(style()) }
pub fn title_font() -> &'static str { title_font_of(style()) }
pub fn title_weight() -> gpui::FontWeight { title_weight_of(style()) }
pub fn title_tracking() -> f32 { title_tracking_of(style()) }
/// Document text's family (UI fonts stay the system's).
pub fn body_font() -> &'static str { body_font_of(style()) }
pub fn inline_code_size() -> f32 { inline_code_size_of(style()) }
pub fn code_font() -> &'static str { code_font_of(style()) }
pub fn pre_size() -> f32 { pre_size_of(style()) }
pub fn pre_line() -> f32 { pre_line_of(style()) }
pub fn table_size() -> f32 { scale_by(style().num("table.size"), doc_scale_for(style())) }
pub fn table_line_height() -> f32 { scale_by(style().num("table.line-height"), doc_scale_for(style())) }
/// Document colour tokens: a theme file may override any of them by name
/// (docs/THEMES.md); otherwise they come from the base style.
macro_rules! tokens {
    ($($n:ident),* $(,)?) => {
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy)]
        enum Tok { $($n),* }
        /// Names a theme file uses for the colour tokens, in index order.
        pub const TOKENS: &[&str] = &[$(stringify!($n)),*];
        /// The base style's value of token `i`.
        pub fn base_color(i: usize, feishu: bool) -> gpui::Hsla {
            const ALL: &[(fn() -> gpui::Hsla, fn() -> gpui::Hsla)] = &[$((crate::feishu_theme::Theme::$n, crate::theme::Theme::$n)),*];
            if feishu { (ALL[i].0)() } else { (ALL[i].1)() }
        }
        pub struct Theme;
        impl Theme {
            $(pub fn $n() -> gpui::Hsla { crate::themes::color(Tok::$n as usize).unwrap_or_else(|| base_color(Tok::$n as usize, crate::settings::feishu())) })*
        }
    };
}
tokens!(
    text, text_caption, text_placeholder, block_placeholder, text_disabled, text_code,
    link, link_hover, primary, primary_500, primary_fill_1, bg_body,
    bg_overlay, line_divider, line_border, fill_hover, fill_hover_strong, fill_pressed,
    fill_turninto_active, selection, block_hover, block_selected, handle_active, size_cell,
    size_cell_border, size_cell_active, size_cell_disabled, swatch_border, block_bg_area, todo_done,
    scrollbar_thumb, shadow, code_hover_ring, quote_bar, caret, pinned,
    table_header, table_line, table_bar_hover, table_bar_selected, danger_bar, danger_cell,
    danger, window, sidebar_text, sidebar_hover, sidebar_selected, sidebar_section,
    button_border, code_chip, code_chip_border, code_bg, code_gutter, code_gutter_line,
    code_border, code_line_number, caption, menu_highlight, popover_bg, separator_dots,
    todo_box, code_text,
);
fn hue(name: &str) -> &str { name.strip_prefix("light-").or_else(|| name.strip_prefix("medium-")).unwrap_or(name) }
pub fn text_color(name: &str) -> Option<gpui::Hsla> { style().palette("color", name).or_else(|| if crate::settings::feishu() { crate::feishu_theme::text_color(name) } else { crate::theme::text_color(name) }) }
pub fn bg_color(name: &str) -> Option<gpui::Hsla> { style().palette("highlight", name).or_else(|| if crate::settings::feishu() { crate::feishu_theme::bg_color(name) } else { crate::theme::bg_color(name) }) }
pub fn block_bg(name: &str) -> Option<gpui::Hsla> { style().palette("background", name).or_else(|| if crate::settings::feishu() { crate::feishu_theme::block_bg(name) } else { crate::theme::block_bg(name) }) }
pub fn bg_swatch(name: &str) -> Option<gpui::Hsla> { if crate::settings::feishu() { crate::feishu_theme::bg_swatch(name) } else { crate::theme::bg_swatch(name) } }
pub fn callout_bg(name: &str) -> Option<gpui::Hsla> { style().palette("callout", name).or_else(|| if crate::settings::feishu() { crate::feishu_theme::callout_bg(name) } else { crate::theme::callout_bg(name) }) }
pub fn callout_border(name: &str) -> Option<gpui::Hsla> { style().palette("border", hue(name)).or_else(|| if crate::settings::feishu() { crate::feishu_theme::callout_border(name) } else { crate::theme::callout_border(name) }) }
pub fn callout_border_swatch(name: &str) -> Option<gpui::Hsla> { if crate::settings::feishu() { crate::feishu_theme::callout_border_swatch(name) } else { crate::theme::callout_border_swatch(name) } }
pub use crate::theme::{shadow_button, shadow_menu, shadow_panel, shadow_dialog};

pub fn mono_font() -> &'static str { mono_font_of(style()) }

impl Theme {
    pub fn icon() -> gpui::Hsla { if crate::theme::dark() { Self::text_code() } else { crate::theme::Theme::icon() } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::Style;

    #[test]
    fn heading_hierarchy_and_proximity_rhythm() {
        for feishu in [false, true] {
            let st = Style::plain(feishu);
            let t = title_of(&st);
            let h1 = heading_metrics_of(&st, 1);
            let h2 = heading_metrics_of(&st, 2);
            let h3 = heading_metrics_of(&st, 3);
            let h4 = heading_metrics_of(&st, 4);
            let h5 = heading_metrics_of(&st, 5);
            let h6 = heading_metrics_of(&st, 6);
            let body = body_size_of(&st);
            assert!(t.size > h1.size && h1.size > h2.size && h2.size > h3.size && h3.size > h4.size && h4.size > body);
            assert!(h4.size > h5.size && h5.size >= h6.size);
            // Inverse line-height: tighter ratio for Title/H1 than body text.
            assert!((t.line_height / t.size) < (body_line_of(&st) / body));
            assert!((h1.line_height / h1.size) < (body_line_of(&st) / body));
            // Proximity law: margin-top >= 2.5x margin-bottom on headings.
            for l in 1..=6 {
                let m = heading_metrics_of(&st, l);
                assert!(m.mt >= m.mb * 2.4, "level {l}: mt {} vs mb {}", m.mt, m.mb);
            }
            // Negative optical tracking on display headings, positive on H5/H6 micro-headings.
            assert!(title_tracking_of(&st) < 0. && heading_tracking_of(&st, 1) < 0.);
            assert!(heading_tracking_of(&st, 5) > 0. && heading_tracking_of(&st, 6) > 0.);
            // Weight contrast on primary headings.
            assert!(heading_weight_of(&st, 1).0 >= 700.);
            assert!(heading_weight_of(&st, 3).0 >= 600.);
        }
    }
}
