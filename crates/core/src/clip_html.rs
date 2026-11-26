//! Copied blocks as HTML for other apps (Word, browsers) and Windows'
//! `HTML Format` (CF_HTML) envelope around it. Plain tags with inline
//! styles, which those apps keep; `paste::html_to_blocks` reads it back.

use crate::model::*;
use crate::palette::Rgba;
use std::fmt::Write;
use std::path::Path;

/// `blocks` as an HTML fragment. Local images point at their files.
pub fn blocks_to_html(blocks: &[Block], library_dir: &Path) -> String {
    let mut out = String::new();
    write_blocks(blocks, library_dir, &mut out);
    out
}

fn write_blocks(list: &[Block], dir: &Path, out: &mut String) {
    let mut i = 0;
    while i < list.len() {
        // Consecutive items of one kind of list share one `<ul>` / `<ol>`.
        let tag = match list[i].kind {
            BlockKind::Bullet | BlockKind::Todo { .. } => "ul",
            BlockKind::Ordered { .. } => "ol",
            _ => {
                block(&list[i], dir, out);
                i += 1;
                continue;
            }
        };
        match list[i].kind {
            BlockKind::Ordered { seq: Some(n) } if n != 1 => { let _ = write!(out, "<ol start=\"{n}\">"); }
            _ => { let _ = write!(out, "<{tag}>"); }
        }
        while let Some(b) = list.get(i).filter(|b| match b.kind {
            BlockKind::Bullet | BlockKind::Todo { .. } => tag == "ul",
            BlockKind::Ordered { .. } => tag == "ol",
            _ => false,
        }) {
            let _ = write!(out, "<li{}>", align(b));
            if let BlockKind::Todo { done } = b.kind {
                let _ = write!(out, "<input type=\"checkbox\" disabled{}> ", if done { " checked" } else { "" });
            }
            out.push_str(&inline(&b.text));
            write_blocks(&b.children, dir, out);
            out.push_str("</li>");
            i += 1;
        }
        let _ = write!(out, "</{tag}>");
    }
}

fn align(b: &Block) -> &'static str {
    match b.align {
        Align::Left => "",
        Align::Center => " style=\"text-align:center\"",
        Align::Right => " style=\"text-align:right\"",
    }
}

fn block(b: &Block, dir: &Path, out: &mut String) {
    match &b.kind {
        BlockKind::Text => { let _ = write!(out, "<p{}>{}</p>", align(b), inline(&b.text)); }
        BlockKind::Heading { level, .. } => {
            let n = (*level).clamp(1, 6);
            let _ = write!(out, "<h{n}{}>{}</h{n}>", align(b), inline(&b.text));
        }
        BlockKind::Code { lang, .. } => {
            let class = if lang.eq_ignore_ascii_case("plain text") { String::new() } else { format!(" class=\"language-{}\"", esc(&lang.to_lowercase().replace(' ', ""))) };
            let _ = write!(out, "<pre><code{class}>{}</code></pre>", esc(&b.text.text));
        }
        BlockKind::Quote => {
            out.push_str("<blockquote>");
            write_blocks(&b.children, dir, out);
            out.push_str("</blockquote>");
        }
        // `callout-block` with these colours reads back as a 高亮块.
        BlockKind::Callout { emoji, bg, border, .. } => {
            let fill = bg.as_ref().and_then(|c| crate::palette::callout_bg(&c.0)).unwrap_or(crate::palette::hex(0xfff5eb));
            let line = border.as_ref().and_then(|c| crate::palette::callout_border(&c.0)).unwrap_or(crate::palette::hex(0xfed4a4));
            let _ = write!(out, "<div class=\"callout-block\" style=\"background-color:{};border:1px solid {};border-color:{};padding:8px 12px\"><p>{}</p>", css(fill), css(line), css(line), esc(emoji));
            write_blocks(&b.children, dir, out);
            out.push_str("</div>");
            return;
        }
        BlockKind::Divider => out.push_str("<hr>"),
        BlockKind::Bullet | BlockKind::Ordered { .. } | BlockKind::Todo { .. } => return write_blocks(std::slice::from_ref(b), dir, out),
        BlockKind::Image { src, width, caption, .. } => {
            let Some(url) = image_url(src, dir) else { return };
            let w = width.map(|w| format!(" width=\"{}\"", w.round())).unwrap_or_default();
            let alt = caption.as_deref().filter(|c| !c.is_empty()).map(|c| format!(" alt=\"{}\"", esc(c))).unwrap_or_default();
            let _ = write!(out, "<p{}><img src=\"{}\"{w}{alt}></p>", align(b), esc(&url));
        }
        BlockKind::Table(t) => table(t, dir, out),
        BlockKind::Grid | BlockKind::Column { .. } => write_blocks(&b.children, dir, out),
        BlockKind::File(f) => { let _ = write!(out, "<p>{}</p>", esc(&f.name)); }
        BlockKind::DataTable(t) => {
            let grid = crate::table::block_grid(dir, t);
            if grid.is_empty() { return }
            out.push_str("<table border=\"1\" style=\"border-collapse:collapse\">");
            for (i, row) in grid.iter().enumerate() {
                out.push_str("<tr>");
                for cell in row { let _ = write!(out, "<{0}>{1}</{0}>", if i == 0 { "th" } else { "td" }, esc(cell)); }
                out.push_str("</tr>");
            }
            out.push_str("</table>");
        }
        BlockKind::Board(bd) => {
            for line in bd.plain_text().lines().filter(|l| !l.trim().is_empty()) { let _ = write!(out, "<p>{}</p>", esc(line)); }
        }
        BlockKind::Countdown(_) | BlockKind::SubPageList => {}
        BlockKind::Raw { xml, .. } => match crate::docxml::raw_summary(xml) {
            (text, Some(link)) if crate::paste::link_ok(&link) => {
                let _ = write!(out, "<p><a href=\"{}\">{}</a></p>", esc(&link), esc(if text.is_empty() { &link } else { &text }));
            }
            (text, _) if !text.is_empty() => { let _ = write!(out, "<p>{}</p>", esc(&text).replace('\n', "<br>")); }
            _ => {}
        },
    }
    // Children of a paragraph or heading: indented under it.
    if !b.children.is_empty() && !b.kind.is_container() {
        out.push_str("<div style=\"margin-left:2em\">");
        write_blocks(&b.children, dir, out);
        out.push_str("</div>");
    }
}

fn table(t: &Table, dir: &Path, out: &mut String) {
    out.push_str("<table border=\"1\" style=\"border-collapse:collapse\">");
    for (r, row) in t.rows.iter().enumerate() {
        out.push_str("<tr>");
        for (c, cell) in row.iter().enumerate() {
            if cell.is_covered() { continue }
            let tag = if r < t.header_rows || (t.header_col && c == 0) { "th" } else { "td" };
            let _ = write!(out, "<{tag}");
            if cell.rowspan > 1 { let _ = write!(out, " rowspan=\"{}\"", cell.rowspan); }
            if cell.colspan > 1 { let _ = write!(out, " colspan=\"{}\"", cell.colspan); }
            if let Some(w) = t.col_widths.get(c).filter(|_| cell.colspan == 1) { let _ = write!(out, " width=\"{}\"", w.round()); }
            if let Some(bg) = cell.bg.as_ref().and_then(|b| crate::palette::bg_swatch(&b.0)) { let _ = write!(out, " style=\"background-color:{}\"", css(bg)); }
            out.push('>');
            write_blocks(&cell.blocks, dir, out);
            let _ = write!(out, "</{tag}>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
}

/// Remote and inline images as they are; a library file as a `file:` URL.
fn image_url(src: &str, dir: &Path) -> Option<String> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:image/") {
        return Some(src.to_string());
    }
    let path = crate::store::export_path_in(dir, src)?;
    let p = path.to_string_lossy().replace('\\', "/");
    let p = p.strip_prefix("//?/").unwrap_or(&p);
    let enc: String = p.bytes().map(|c| match c {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => (c as char).to_string(),
        _ => format!("%{c:02X}"),
    }).collect();
    Some(if enc.starts_with('/') { format!("file://{enc}") } else { format!("file:///{enc}") })
}

fn inline(t: &RichText) -> String {
    let mut out = String::new();
    for (r, m) in t.spans() {
        let s = match &m.inline {
            Some(obj) => match &obj.kind {
                InlineKind::Equation(src) => t.text[r].chars().map(|_| src.as_str()).collect::<String>(),
                InlineKind::File(f) => t.text[r].chars().map(|_| f.name.as_str()).collect(),
                _ => {
                    let (text, link) = crate::docxml::raw_summary(&obj.xml);
                    match link.filter(|l| matches!(obj.kind, InlineKind::Button(_)) && crate::paste::link_ok(l)) {
                        Some(l) => { let _ = write!(out, "<a href=\"{}\">{}</a>", esc(&l), esc(&text)); continue }
                        None => text,
                    }
                }
            },
            None => t.text[r].to_string(),
        };
        if s.is_empty() { continue }
        let mut x = esc(&s).replace('\n', "<br>");
        if m.code { x = format!("<code>{x}</code>"); }
        if m.strike { x = format!("<s>{x}</s>"); }
        if m.underline { x = format!("<u>{x}</u>"); }
        if m.italic { x = format!("<i>{x}</i>"); }
        if m.bold { x = format!("<b>{x}</b>"); }
        let color = m.text_color.as_ref().and_then(|c| crate::palette::text_color(&c.0)).map(|c| format!("color:{};", css(c)));
        let bg = m.bg_color.as_ref().and_then(|c| crate::palette::bg_color(&c.0)).map(|c| format!("background-color:{};", css(c)));
        if color.is_some() || bg.is_some() {
            x = format!("<span style=\"{}{}\">{x}</span>", color.unwrap_or_default(), bg.unwrap_or_default());
        }
        if let Some(l) = m.link.as_ref().filter(|l| crate::paste::link_ok(l)) {
            x = format!("<a href=\"{}\">{x}</a>", esc(l));
        }
        out.push_str(&x);
    }
    out
}

fn css(c: Rgba) -> String {
    if c.a >= 1. { format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b) } else { format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a) }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const START: &str = "<!--StartFragment-->";
const END: &str = "<!--EndFragment-->";

/// An HTML fragment in the CF_HTML envelope: a header of UTF-8 byte offsets
/// to the page and to the fragment between its marker comments.
pub fn cf_html(fragment: &str) -> String {
    let header = |a: usize, b: usize, c: usize, d: usize| format!("Version:0.9\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n");
    let len = header(0, 0, 0, 0).len();
    let pre = format!("<html><head><meta charset=\"utf-8\"></head><body>\r\n{START}");
    let post = format!("{END}\r\n</body></html>");
    let (sf, ef) = (len + pre.len(), len + pre.len() + fragment.len());
    format!("{}{pre}{fragment}{post}", header(len, ef + post.len(), sf, ef))
}

/// The HTML in Windows' `HTML Format`. The marker comments find the fragment
/// first (an app may count the offsets wrong, e.g. in characters); without
/// them the StartFragment/EndFragment offsets, then StartHTML/EndHTML, then
/// all that follows the header. Text with no header is HTML already.
pub fn cf_html_fragment(s: &str) -> String {
    let s = s.trim_end_matches('\0');
    if !s.starts_with("Version:") {
        return s.to_string();
    }
    let body_at = s.find('<').unwrap_or(s.len());
    let head = &s[..body_at];
    let num = |key: &str| -> Option<usize> {
        let v: i64 = head.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':')?.trim().parse().ok())?;
        usize::try_from(v).ok()
    };
    let span = |a: Option<usize>, b: Option<usize>| match (a, b) {
        (Some(a), b) if a >= body_at && a <= b.unwrap_or(s.len()).min(s.len()) && s.is_char_boundary(a) && s.is_char_boundary(b.unwrap_or(s.len()).min(s.len())) => Some(&s[a..b.unwrap_or(s.len()).min(s.len())]),
        _ => None,
    };
    let body = &s[body_at..];
    if let Some(i) = body.find(START) {
        let rest = &body[i + START.len()..];
        return rest[..rest.find(END).unwrap_or(rest.len())].to_string();
    }
    span(num("StartFragment"), num("EndFragment")).or_else(|| span(num("StartHTML"), num("EndHTML"))).unwrap_or(body).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paste::html_to_blocks;

    fn xml(blocks: Vec<Block>) -> String {
        crate::docxml::serialize(&Document { blocks, ..Default::default() })
    }

    #[test]
    fn cf_html_round_trips_with_non_ascii() {
        let frag = "<p>中文 <b>粗体</b> émoji 🎉</p>";
        let env = cf_html(frag);
        assert_eq!(cf_html_fragment(&env), frag);
        // The offsets alone (no markers) find it too: they count UTF-8 bytes.
        let bare = env.replace(START, "").replace(END, "");
        let start: usize = bare.lines().find_map(|l| l.strip_prefix("StartFragment:")).unwrap().trim().parse().unwrap();
        let shift = START.len();
        let fixed = bare
            .replace(&format!("StartFragment:{start:010}"), &format!("StartFragment:{:010}", start - shift))
            .replace(&format!("EndFragment:{:010}", start + frag.len()), &format!("EndFragment:{:010}", start - shift + frag.len()));
        assert_eq!(cf_html_fragment(&fixed), frag);
        assert_eq!(cf_html_fragment(&format!("{env}\0")), frag);
    }

    #[test]
    fn cf_html_bad_offsets() {
        // Offsets counted in characters land inside a character, or past the end:
        // the markers still find the fragment.
        let s = "Version:0.9\r\nStartHTML:0000000097\r\nEndHTML:0000000999\r\nStartFragment:0000000133\r\nEndFragment:0000000140\r\n<html><body>\r\n<!--StartFragment--><p>标题一</p><!--EndFragment-->\r\n</body></html>";
        assert_eq!(cf_html_fragment(s), "<p>标题一</p>");
        // No markers and offsets inside a character: the page after the header.
        let s = "Version:0.9\r\nStartHTML:0000000001\r\nEndHTML:0000000999\r\nStartFragment:0000000101\r\nEndFragment:0000000999\r\n<html><body><p>标题一</p></body></html>";
        let got = cf_html_fragment(s);
        assert!(got.starts_with("<html>") && got.contains("<p>标题一</p>"), "{got}");
        // -1 (no context) is not an offset.
        let head = "Version:1.0\r\nStartHTML:-1\r\nEndHTML:-1\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
        let at = head.len() + "<html><body>".len();
        let s = format!("Version:1.0\r\nStartHTML:-1\r\nEndHTML:-1\r\nStartFragment:{at:010}\r\nEndFragment:{:010}\r\n<html><body><p>abcd</p></body></html>", at + 11);
        assert_eq!(cf_html_fragment(&s), "<p>abcd</p>");
        // Not CF_HTML at all.
        assert_eq!(cf_html_fragment("<p>x</p>"), "<p>x</p>");
    }

    #[test]
    fn cf_html_missing_end_fragment() {
        // No EndFragment line and no end marker: up to the end of the text.
        let s = "Version:0.9\r\nStartHTML:0000000071\r\nStartFragment:0000000105\r\n<html><body>\r\n<!--StartFragment--><p>a</p><p>b</p></body></html>";
        let got = cf_html_fragment(s);
        assert_eq!(xml(html_to_blocks(&got)), xml(html_to_blocks("<p>a</p><p>b</p>")));
        // Offsets alone, no EndFragment: to the end.
        let s = "Version:0.9\r\nStartFragment:0000000040\r\n<html><body><p>a</p></body></html>";
        let got = cf_html_fragment(s);
        assert!(got.contains("<p>a</p>"), "{got}");
    }

    #[test]
    fn blocks_read_back() {
        let src = "<title></title>\n<h2>Head</h2>\n<p>a <b>bold</b> <em>it</em> <u>u</u> <del>s</del> <code>c</code> <a href=\"https://x.y/\">link</a></p>\n\
            <ul>\n  <li>one\n    <ul>\n      <li>sub</li>\n    </ul>\n  </li>\n</ul>\n<ol>\n  <li seq=\"3\">three</li>\n  <li>four</li>\n</ol>\n<checkbox done=\"true\">done</checkbox>\n\
            <blockquote>\n  <p>q</p>\n</blockquote>\n<hr/>\n<pre lang=\"rust\"><code>fn x() {}</code></pre>\n";
        let doc = crate::docxml::parse(src).unwrap();
        let html = blocks_to_html(&doc.blocks, Path::new("/nonexistent"));
        let back = xml(html_to_blocks(&html));
        for want in ["<h2>Head</h2>", "<b>bold</b>", "<em>it</em>", "<u>u</u>", "<del>s</del>", "<code>c</code>", "<a href=\"https://x.y/\">link</a>", "sub", "<li seq=\"3\">three</li>", "<checkbox done=\"true\">done</checkbox>", "<blockquote>", "<hr/>", "lang=\"rust\""] {
            assert!(back.contains(want), "{want} missing:\n{html}\n{back}");
        }
        assert_eq!(cf_html_fragment(&cf_html(&html)), html);
    }

    #[test]
    fn table_and_colors_read_back() {
        let src = "<title></title>\n<table header-row=\"true\">\n  <colgroup><col width=\"100\"/><col width=\"100\"/></colgroup>\n  <tbody>\n    <tr><td><p>a</p></td><td><p>b</p></td></tr>\n    <tr><td colspan=\"2\"><p><span text-color=\"red\">c</span></p></td></tr>\n  </tbody>\n</table>\n";
        let doc = crate::docxml::parse(src).unwrap();
        let html = blocks_to_html(&doc.blocks, Path::new("/nonexistent"));
        let back = xml(html_to_blocks(&html));
        assert!(back.contains("colspan=\"2\""), "{html}\n{back}");
        assert!(back.contains("text-color=\"red\""), "{html}\n{back}");
    }
}
