//! Label text: Mermaid's escapes and HTML bits to plain lines, wrapping to
//! a width, and the vertical metrics the drawings share.

use crate::Measure;

/// Line height as a multiple of the font size.
pub const LEADING: f32 = 1.45;

/// A character that takes a full em and may break on either side (CJK,
/// kana, hangul, full-width forms).
pub fn wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

/// A label as written (inside its brackets or quotes) to plain text, `\n`
/// between lines: quotes and markdown-string backticks off, `<br>` to a
/// break, other tags dropped, `#quot;` / `#35;` / `&amp;` style entities
/// decoded.
pub fn label(raw: &str) -> String {
    let mut s = raw.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') { s = &s[1..s.len() - 1]; }
    let md = s.len() >= 2 && s.starts_with('`') && s.ends_with('`');
    if md { s = &s[1..s.len() - 1]; }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some(end) = rest.find('>').filter(|_| rest[1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/')) {
                let tag = rest[1..end].trim().trim_end_matches('/').trim().to_ascii_lowercase();
                let name = tag.split_whitespace().next().unwrap_or("");
                if name == "br" { out.push('\n'); rest = &rest[end + 1..]; continue }
                if !name.is_empty() && name.trim_start_matches('/').chars().all(|c| c.is_ascii_alphanumeric()) { rest = &rest[end + 1..]; continue }
            }
        } else if c == '#' || c == '&' {
            if let Some((ch, len)) = entity(rest) { out.push(ch); rest = &rest[len..]; continue }
        } else if md && (c == '*' || c == '_') && rest.starts_with(c) {
            // markdown emphasis markers: the text stays, the markers go
            let run = rest.chars().take_while(|&x| x == c).count();
            let word_inside = rest[run..].starts_with(|x: char| !x.is_whitespace()) || out.ends_with(|x: char| !x.is_whitespace());
            if word_inside && run <= 3 { rest = &rest[run..]; continue }
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    if md {
        // a markdown string's line breaks are breaks; its indents aren't
        out = out.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    }
    out.replace("\\n", "\n")
}

/// `#name;`, `#123;`, `&name;` or `&#123;` / `&#x7b;` at the start of `s`:
/// the character and the escape's byte length.
fn entity(s: &str) -> Option<(char, usize)> {
    let end = s.find(';')?;
    if !(2..=10).contains(&end) { return None }
    let body = &s[1..end];
    let amp = s.starts_with('&');
    let num = body.strip_prefix('#').filter(|_| amp).or(if amp { None } else { Some(body) });
    let ch = if let Some(n) = num.filter(|n| n.chars().all(|c| c.is_ascii_digit())) {
        char::from_u32(n.parse().ok()?)?
    } else if let Some(h) = num.and_then(|n| n.strip_prefix('x').or(n.strip_prefix('X'))) {
        char::from_u32(u32::from_str_radix(h, 16).ok()?)?
    } else {
        match body {
            "quot" => '"', "amp" => '&', "lt" => '<', "gt" => '>', "apos" => '\'', "nbsp" => '\u{a0}', "colon" => ':', "semi" => ';', "num" => '#',
            "lpar" => '(', "rpar" => ')', "lsqb" => '[', "rsqb" => ']', "lcub" => '{', "rcub" => '}', "vert" => '|', "excl" => '!',
            "copy" => '©', "reg" => '®', "trade" => '™', "hellip" => '…', "ndash" => '–', "mdash" => '—', "larr" => '←', "rarr" => '→', "uarr" => '↑', "darr" => '↓', "hearts" => '♥', "infin" => '∞',
            _ => return None,
        }
    };
    Some((ch, end + 1))
}

/// `text` (lines split at `\n`) broken to lines no wider than `max`, at
/// spaces or around wide characters; a word longer than `max` breaks
/// between characters. `max <= 0` only splits at `\n`.
pub fn wrap(text: &str, max: f32, size: f32, bold: bool, m: &dyn Measure) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let para = para.trim();
        if max <= 0. || m.width(para, size, bold) <= max { out.push(para.to_string()); continue }
        // tokens: a word with the spaces after it, or one wide character
        let mut tokens: Vec<&str> = Vec::new();
        let mut start = 0;
        let mut prev_wide = false;
        for (i, c) in para.char_indices() {
            let w = wide(c);
            // a break before a wide character, after one, and after spaces
            let ended = para[start..i].ends_with(' ');
            if i > start && (((w || prev_wide) && !ended) || (c != ' ' && ended)) { tokens.push(&para[start..i]); start = i; }
            prev_wide = w;
        }
        if start < para.len() { tokens.push(&para[start..]); }
        let mut line = String::new();
        for t in tokens {
            let candidate = format!("{line}{t}");
            if line.is_empty() || m.width(candidate.trim_end(), size, bold) <= max { line = candidate; } else {
                out.push(line.trim_end().to_string());
                line = t.to_string();
            }
            // a word wider than the whole line: by characters
            while m.width(line.trim_end(), size, bold) > max && line.chars().count() > 1 {
                let mut cut = String::new();
                for c in line.chars() {
                    if !cut.is_empty() && m.width(&format!("{cut}{c}"), size, bold) > max { break }
                    cut.push(c);
                }
                line = line[cut.len()..].to_string();
                out.push(cut);
            }
        }
        let rest = line.trim_end();
        if !rest.is_empty() || out.is_empty() { out.push(rest.to_string()); }
    }
    out
}

/// Laid-out label text: its lines and the box they fill.
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub lines: Vec<String>,
    pub w: f32,
    pub h: f32,
    pub size: f32,
    pub bold: bool,
}

impl Block {
    pub fn new(text: &str, max: f32, size: f32, bold: bool, m: &dyn Measure) -> Block {
        if text.trim().is_empty() { return Block { size, bold, ..Default::default() } }
        let lines = wrap(text, max, size, bold, m);
        let w = lines.iter().map(|l| m.width(l, size, bold)).fold(0., f32::max);
        Block { h: lines.len() as f32 * size * LEADING, lines, w, size, bold }
    }
    pub fn is_empty(&self) -> bool { self.lines.is_empty() }
}

/// `s` for an SVG text node or attribute.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Approx;

    #[test]
    fn labels() {
        assert_eq!(label("\"A b\""), "A b");
        assert_eq!(label("one<br>two<br/>three<br />four"), "one\ntwo\nthree\nfour");
        assert_eq!(label("<b>bold</b> #quot;q#quot; #35; &amp; &#65;"), "bold \"q\" # & A");
        assert_eq!(label("a < b > c"), "a < b > c");
        assert_eq!(label("\"`**Bold** and _it_\n  next`\""), "Bold and it\nnext");
        assert_eq!(label("snake_case_name"), "snake_case_name");
        assert_eq!(label("#1 item"), "#1 item");
    }

    #[test]
    fn wrapping() {
        let m = Approx;
        let w = wrap("The quick brown fox jumps over the lazy dog", 100., 14., false, &m);
        assert!(w.len() > 2, "{w:?}");
        assert!(w.iter().all(|l| m.width(l, 14., false) <= 100.), "{w:?}");
        assert_eq!(w.join(" "), "The quick brown fox jumps over the lazy dog");
        let c = wrap("这是一个很长的中文标签需要换行显示", 60., 14., false, &m);
        assert!(c.len() > 3 && c.iter().all(|l| m.width(l, 14., false) <= 60.), "{c:?}");
        assert_eq!(c.concat(), "这是一个很长的中文标签需要换行显示");
        let long = wrap("Supercalifragilisticexpialidocious", 50., 14., false, &m);
        assert!(long.len() > 1 && long.iter().all(|l| m.width(l, 14., false) <= 50.), "{long:?}");
        assert_eq!(wrap("a\nb", 0., 14., false, &m), ["a", "b"]);
    }
}
