//! Code block syntax colouring: adapts `fdl_core::highlight` tokens to the
//! active theme's `.token.*` colours.

use crate::theme::c;
use gpui::Hsla;
use std::ops::Range;

pub type Span = (Range<usize>, Hsla, bool);

fn color(t: fdl_core::highlight::Tok) -> Hsla {
    crate::editor_theme::style()
        .color(fdl_core::highlight::theme_key(t))
        .unwrap_or_else(|| c(0x8d9096))
}

pub fn spans(lang: &str, text: &str) -> Vec<Span> {
    fdl_core::highlight::raw_spans(lang, text)
        .into_iter()
        .map(|(r, t, italic)| (r, color(t), italic))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(lang: &str, text: &str) -> Vec<(String, u32)> {
        spans(lang, text)
            .into_iter()
            .map(|(r, h, _)| {
                let rgb: gpui::Rgba = h.into();
                let v = ((rgb.r * 255.).round() as u32) << 16 | ((rgb.g * 255.).round() as u32) << 8 | (rgb.b * 255.).round() as u32;
                (text[r].to_string(), v)
            })
            .collect()
    }

    #[test]
    fn rust() {
        let k = kinds("Rust", "fn main() {\n    println!(\"hi\"); // x\n    let n: u8 = 3;\n}");
        assert_eq!(
            k,
            vec![
                ("fn".into(), 0xb2565c),
                ("main".into(), 0x5f7e43),
                ("println!".into(), 0x5f7e43),
                ("\"hi\"".into(), 0x89713a),
                ("// x".into(), 0x8d9096),
                ("let".into(), 0xb2565c),
                ("u8".into(), 0x3878b1),
                ("3".into(), 0x9759b3),
            ]
        );
    }

    #[test]
    fn markup_tags() {
        let k = kinds("HTML", "<a href=\"x\">t</a>");
        assert_eq!(k[0], ("a".into(), 0xb2565c));
        assert_eq!(k[1], ("href".into(), 0x3878b1));
        assert_eq!(k[2], ("\"x\"".into(), 0x89713a));
        assert_eq!(k[3], ("a".into(), 0xb2565c));
    }

    #[test]
    fn python_strings() {
        let k = kinds("Python", "x = f\"a{b}\"\n\"\"\"l1\nl2\"\"\"");
        assert_eq!(k, vec![("f\"a{b}\"".into(), 0x89713a), ("\"\"\"l1\nl2\"\"\"".into(), 0x89713a)]);
    }

    #[test]
    fn plain_is_uncoloured() {
        assert!(spans("Plain Text", "fn main() {}").is_empty());
    }
}
