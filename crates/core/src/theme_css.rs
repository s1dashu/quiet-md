//! Pure-Rust document CSS theme parser, validator and HTML override generator.
//! Shared by the GPUI app (`src/themes.rs`) and headless CLI (`crates/cli`).

use crate::palette::{self, Rgba};
use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

pub const BUNDLED: [(&str, &str); 4] = [
    ("craft", include_str!("../../../assets/themes/craft.css")),
    ("feishu", include_str!("../../../assets/themes/feishu.css")),
    ("notion", include_str!("../../../assets/themes/notion.css")),
    ("obsidian", include_str!("../../../assets/themes/obsidian.css")),
];

pub const TOKEN_NAMES: &[&str] = &[
    "text", "text_caption", "text_placeholder", "block_placeholder", "text_disabled", "text_code",
    "link", "link_hover", "primary", "primary_500", "primary_fill_1", "bg_body",
    "bg_overlay", "line_divider", "line_border", "fill_hover", "fill_hover_strong", "fill_pressed",
    "fill_turninto_active", "selection", "block_hover", "block_selected", "handle_active", "size_cell",
    "size_cell_border", "size_cell_active", "size_cell_disabled", "swatch_border", "block_bg_area", "todo_done",
    "scrollbar_thumb", "shadow", "code_hover_ring", "quote_bar", "caret", "pinned",
    "table_header", "table_line", "table_bar_hover", "table_bar_selected", "danger_bar", "danger_cell",
    "danger", "window", "sidebar_text", "sidebar_hover", "sidebar_selected", "sidebar_section",
    "button_border", "code_chip", "code_chip_border", "code_bg", "code_gutter", "code_gutter_line",
    "code_border", "code_line_number", "caption", "menu_highlight", "popover_bg", "separator_dots",
    "todo_box", "code_text",
];

#[derive(Clone, Debug)]
pub struct Rule {
    pub sels: Vec<String>,
    pub decls: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct ThemeMeta {
    pub name: Option<String>,
    pub name_en: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropKind { Len, Weight, Color, Font, Underline, Strike, Display, Italic, LineStyle }

pub struct PropSpec {
    pub sel: &'static str,
    pub prop: &'static str,
    pub key: &'static str,
    pub kind: PropKind,
    pub feishu_used: bool,
}

pub const PROPS: &[PropSpec] = &[
    PropSpec { sel: ".page", prop: "background-color", key: "page.bg", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".page", prop: "color", key: "page.color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".page", prop: "font-family", key: "page.font", kind: PropKind::Font, feishu_used: true },
    PropSpec { sel: ".page", prop: "font-size", key: "page.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: ".page", prop: "line-height", key: "page.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: ".page", prop: "max-width", key: "page.width", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: ".page", prop: "caret-color", key: "page.caret", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "::selection", prop: "background-color", key: "selection", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "p", prop: "margin-bottom", key: "p.gap", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: ".title", prop: "font-family", key: "title.font", kind: PropKind::Font, feishu_used: true },
    PropSpec { sel: ".title", prop: "font-size", key: "title.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: ".title", prop: "line-height", key: "title.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: ".title", prop: "font-weight", key: "title.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: ".title", prop: "color", key: "title.color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "h1", prop: "font-family", key: "h1.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h1", prop: "font-size", key: "h1.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h1", prop: "line-height", key: "h1.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h1", prop: "font-weight", key: "h1.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h1", prop: "color", key: "h1.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h1", prop: "margin-top", key: "h1.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h1", prop: "margin-bottom", key: "h1.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h2", prop: "font-family", key: "h2.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h2", prop: "font-size", key: "h2.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h2", prop: "line-height", key: "h2.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h2", prop: "font-weight", key: "h2.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h2", prop: "color", key: "h2.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h2", prop: "margin-top", key: "h2.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h2", prop: "margin-bottom", key: "h2.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h3", prop: "font-family", key: "h3.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h3", prop: "font-size", key: "h3.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h3", prop: "line-height", key: "h3.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h3", prop: "font-weight", key: "h3.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h3", prop: "color", key: "h3.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h3", prop: "margin-top", key: "h3.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h3", prop: "margin-bottom", key: "h3.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h4", prop: "font-family", key: "h4.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h4", prop: "font-size", key: "h4.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h4", prop: "line-height", key: "h4.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h4", prop: "font-weight", key: "h4.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h4", prop: "color", key: "h4.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h4", prop: "margin-top", key: "h4.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h4", prop: "margin-bottom", key: "h4.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h5", prop: "font-family", key: "h5.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h5", prop: "font-size", key: "h5.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h5", prop: "line-height", key: "h5.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h5", prop: "font-weight", key: "h5.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h5", prop: "color", key: "h5.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h5", prop: "margin-top", key: "h5.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h5", prop: "margin-bottom", key: "h5.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h6", prop: "font-family", key: "h6.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h6", prop: "font-size", key: "h6.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h6", prop: "line-height", key: "h6.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h6", prop: "font-weight", key: "h6.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h6", prop: "color", key: "h6.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h6", prop: "margin-top", key: "h6.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h6", prop: "margin-bottom", key: "h6.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h7", prop: "font-family", key: "h7.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h7", prop: "font-size", key: "h7.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h7", prop: "line-height", key: "h7.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h7", prop: "font-weight", key: "h7.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h7", prop: "color", key: "h7.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h7", prop: "margin-top", key: "h7.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h7", prop: "margin-bottom", key: "h7.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h8", prop: "font-family", key: "h8.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h8", prop: "font-size", key: "h8.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h8", prop: "line-height", key: "h8.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h8", prop: "font-weight", key: "h8.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h8", prop: "color", key: "h8.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h8", prop: "margin-top", key: "h8.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h8", prop: "margin-bottom", key: "h8.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h9", prop: "font-family", key: "h9.font", kind: PropKind::Font, feishu_used: false },
    PropSpec { sel: "h9", prop: "font-size", key: "h9.size", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h9", prop: "line-height", key: "h9.line", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h9", prop: "font-weight", key: "h9.weight", kind: PropKind::Weight, feishu_used: true },
    PropSpec { sel: "h9", prop: "color", key: "h9.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "h9", prop: "margin-top", key: "h9.mt", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "h9", prop: "margin-bottom", key: "h9.mb", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: "strong", prop: "font-weight", key: "strong.weight", kind: PropKind::Weight, feishu_used: false },
    PropSpec { sel: "strong", prop: "color", key: "strong.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "a", prop: "color", key: "a.color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "a", prop: "text-decoration", key: "a.underline", kind: PropKind::Underline, feishu_used: false },
    PropSpec { sel: "a", prop: "text-decoration-color", key: "a.underline-color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "a:hover", prop: "color", key: "a.hover", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "code", prop: "color", key: "code.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "code", prop: "background-color", key: "code.bg", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "code", prop: "border-color", key: "code.border", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "code", prop: "border-radius", key: "code.radius", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "code", prop: "padding-left", key: "code.pad", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "pre", prop: "font-family", key: "pre.font", kind: PropKind::Font, feishu_used: true },
    PropSpec { sel: "pre", prop: "font-size", key: "pre.size", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "pre", prop: "line-height", key: "pre.line", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "pre", prop: "color", key: "pre.color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "pre", prop: "background-color", key: "pre.bg", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "pre", prop: "border-color", key: "pre.border", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "pre", prop: "border-radius", key: "pre.radius", kind: PropKind::Len, feishu_used: true },
    PropSpec { sel: ".line-numbers", prop: "display", key: "pre.numbers", kind: PropKind::Display, feishu_used: false },
    PropSpec { sel: ".line-numbers", prop: "color", key: "pre.number-color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".line-numbers", prop: "background-color", key: "pre.gutter", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "blockquote", prop: "border-left-width", key: "quote.bar", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "blockquote", prop: "border-left-color", key: "quote.bar-color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "blockquote", prop: "padding-left", key: "quote.indent", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "blockquote", prop: "color", key: "quote.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "blockquote", prop: "font-style", key: "quote.italic", kind: PropKind::Italic, feishu_used: false },
    PropSpec { sel: "ul", prop: "padding-left", key: "list.indent", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "ul li::marker", prop: "color", key: "bullet.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: "ol li::marker", prop: "color", key: "number.color", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: ".task", prop: "width", key: "task.size", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: ".task", prop: "border-radius", key: "task.radius", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: ".task", prop: "border-color", key: "task.border", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".task:checked", prop: "background-color", key: "task.checked", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: ".task:checked", prop: "color", key: "task.check", kind: PropKind::Color, feishu_used: false },
    PropSpec { sel: ".task-done", prop: "color", key: "done.color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".task-done", prop: "text-decoration", key: "done.strike", kind: PropKind::Strike, feishu_used: false },
    PropSpec { sel: "hr", prop: "border-top-style", key: "hr.style", kind: PropKind::LineStyle, feishu_used: false },
    PropSpec { sel: "hr", prop: "border-top-width", key: "hr.width", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "hr", prop: "border-top-color", key: "hr.color", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "table", prop: "font-size", key: "table.size", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "table", prop: "line-height", key: "table.line-height", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "table", prop: "border-color", key: "table.line", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: "table", prop: "border-radius", key: "table.radius", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: "th", prop: "background-color", key: "th.bg", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".callout", prop: "border-radius", key: "callout.radius", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: ".callout", prop: "border-width", key: "callout.border", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: ".callout", prop: "padding-top", key: "callout.pad-y", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: ".callout", prop: "padding-left", key: "callout.pad-x", kind: PropKind::Len, feishu_used: false },
    PropSpec { sel: ".token.comment", prop: "color", key: "syntax.comment", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.keyword", prop: "color", key: "syntax.keyword", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.tag", prop: "color", key: "syntax.tag", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.string", prop: "color", key: "syntax.string", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.number", prop: "color", key: "syntax.number", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.boolean", prop: "color", key: "syntax.literal", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.class-name", prop: "color", key: "syntax.type", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.attr-name", prop: "color", key: "syntax.attr", kind: PropKind::Color, feishu_used: true },
    PropSpec { sel: ".token.function", prop: "color", key: "syntax.function", kind: PropKind::Color, feishu_used: true },
];

pub fn canon_sel(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    match s.as_str() {
        "body" | ".markdown" | ".editor" => ".page".into(),
        "b" => "strong".into(),
        ".inline-code" | "p code" | ":not(pre) > code" => "code".into(),
        ".code-block" | "pre code" => "pre".into(),
        ".quote" => "blockquote".into(),
        ".divider" => "hr".into(),
        "ul ::marker" | "ul li::marker" => "ul li::marker".into(),
        "ol ::marker" | "ol li::marker" => "ol li::marker".into(),
        "input[type=checkbox]" | "input[type=\"checkbox\"]" | ".todo" => ".task".into(),
        "input[type=checkbox]:checked" | "input[type=\"checkbox\"]:checked" | ".todo:checked" | ".task.checked" => ".task:checked".into(),
        ".todo-done" | ".task.done" => ".task-done".into(),
        "td" | "th, td" => "table".into(),
        _ => s,
    }
}

pub fn rules(css: &str) -> Vec<Rule> {
    let mut s = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        s.push_str(&rest[..i]);
        rest = rest[i + 2..].find("*/").map_or("", |j| &rest[i + 2 + j + 2..]);
    }
    s.push_str(rest);
    let chars: Vec<char> = s.chars().collect();
    let mut out = vec![];
    let (mut i, mut start) = (0, 0);
    while i < chars.len() {
        if chars[i] != '{' { i += 1; continue }
        let sel: String = chars[start..i].iter().collect();
        let (mut depth, mut j) = (0, i);
        while j < chars.len() {
            match chars[j] { '{' => depth += 1, '}' => { depth -= 1; if depth == 0 { break } } _ => {} }
            j += 1;
        }
        let body: String = chars[i + 1..j.min(chars.len())].iter().collect();
        i = j + 1;
        start = i;
        let sel = sel.trim();
        if sel.starts_with('@') || body.contains('{') { continue }
        let mut decls = vec![];
        let (mut paren, mut quote, mut from) = (0, None::<char>, 0);
        for (k, ch) in body.char_indices() {
            match ch {
                '"' | '\'' if quote == Some(ch) => quote = None,
                '"' | '\'' if quote.is_none() => quote = Some(ch),
                '(' if quote.is_none() => paren += 1,
                ')' if quote.is_none() => paren -= 1,
                ';' if quote.is_none() && paren == 0 => { decls.push(&body[from..k]); from = k + 1; }
                _ => {}
            }
        }
        decls.push(&body[from..]);
        let decls = decls.into_iter().filter_map(|d| {
            let (n, v) = d.split_once(':')?;
            let v = v.trim().trim_end_matches("!important").trim();
            (!n.trim().is_empty() && !v.is_empty()).then(|| (n.trim().to_lowercase(), v.to_string()))
        }).collect();
        out.push(Rule { sels: sel.split(',').map(canon_sel).filter(|s| !s.is_empty()).collect(), decls });
    }
    out
}

pub fn subst(v: &str, vars: &HashMap<String, String>, depth: u8) -> String {
    let Some(i) = v.find("var(") else { return v.to_string() };
    if depth > 8 { return v.to_string() }
    let open = i + 4;
    let (mut d, mut end) = (1, v.len());
    for (k, ch) in v[open..].char_indices() {
        match ch { '(' => d += 1, ')' => { d -= 1; if d == 0 { end = open + k; break } } _ => {} }
    }
    let inner = &v[open..end];
    let (name, fallback) = match inner.split_once(',') { Some((n, f)) => (n.trim(), Some(f.trim())), None => (inner.trim(), None) };
    let rep = vars.get(name).cloned().or(fallback.map(str::to_string)).unwrap_or_default();
    let rep = subst(&rep, vars, depth + 1);
    subst(&format!("{}{}{}", &v[..i], rep, &v[(end + 1).min(v.len())..]), vars, depth + 1)
}

pub fn unquote(s: &str) -> String { s.trim().trim_matches(|c| c == '"' || c == '\'').to_string() }

pub fn parse_color_rgba(v: &str) -> Option<Rgba> {
    let v = v.trim().to_lowercase();
    let rgba = |r: f32, g: f32, b: f32, a: f32| Rgba {
        r: r.round().clamp(0., 255.) as u8,
        g: g.round().clamp(0., 255.) as u8,
        b: b.round().clamp(0., 255.) as u8,
        a: a.clamp(0., 1.),
    };
    match v.as_str() {
        "transparent" => return Some(rgba(0., 0., 0., 0.)),
        "white" => return Some(rgba(255., 255., 255., 1.)),
        "black" => return Some(rgba(0., 0., 0., 1.)),
        _ => {}
    }
    if let Some(h) = v.strip_prefix('#') {
        let x = |s: &str| u8::from_str_radix(s, 16).ok().map(f32::from);
        let c: Vec<char> = h.chars().collect();
        let d = |c: char| x(&c.to_string()).map(|n| n * 17.);
        return match c.len() {
            3 | 4 => Some(rgba(d(c[0])?, d(c[1])?, d(c[2])?, if c.len() == 4 { d(c[3])? / 255. } else { 1. })),
            6 | 8 => Some(rgba(x(&h[0..2])?, x(&h[2..4])?, x(&h[4..6])?, if c.len() == 8 { x(&h[6..8])? / 255. } else { 1. })),
            _ => None,
        };
    }
    let (f, inner) = v.split_once('(')?;
    let inner = inner.strip_suffix(')')?;
    let parts: Vec<&str> = inner.split([',', ' ', '/']).filter(|p| !p.is_empty()).collect();
    if parts.len() < 3 { return None }
    let alpha = |s: Option<&&str>| -> Option<f32> {
        match s { None => Some(1.), Some(s) => s.strip_suffix('%').map_or_else(|| s.parse().ok(), |p| p.parse::<f32>().ok().map(|n| n / 100.)) }
    };
    match f.trim() {
        "rgb" | "rgba" => {
            let ch = |s: &str| -> Option<f32> { s.strip_suffix('%').map_or_else(|| s.parse().ok(), |p| p.parse::<f32>().ok().map(|n| n * 2.55)) };
            Some(rgba(ch(parts[0])?, ch(parts[1])?, ch(parts[2])?, alpha(parts.get(3))?))
        }
        "hsl" | "hsla" => {
            let h: f32 = parts[0].trim_end_matches("deg").parse().ok()?;
            let s: f32 = (parts[1].trim_end_matches('%').parse::<f32>().ok()? / 100.).clamp(0., 1.);
            let l: f32 = (parts[2].trim_end_matches('%').parse::<f32>().ok()? / 100.).clamp(0., 1.);
            let a = alpha(parts.get(3))?;
            let [r, g, b] = hsl_to_rgb(h, s, l);
            Some(rgba(r, g, b, a))
        }
        _ => None,
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.) / 360.;
    if s == 0. { return [l * 255.; 3]; }
    let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
    let p = 2. * l - q;
    let f = |mut t: f32| {
        if t < 0. { t += 1.; }
        if t > 1. { t -= 1.; }
        255. * if t < 1. / 6. { p + (q - p) * 6. * t } else if t < 0.5 { q } else if t < 2. / 3. { p + (q - p) * (2. / 3. - t) * 6. } else { p }
    };
    [f(h + 1. / 3.), f(h), f(h - 1. / 3.)]
}

pub fn parse_len(v: &str, em: f32, unitless: Option<f32>) -> Option<f32> {
    let v = v.trim().to_lowercase();
    let num = |s: &str| s.trim().parse::<f32>().ok();
    if let Some(n) = v.strip_suffix("px").or_else(|| v.strip_suffix("pt")) { return num(n) }
    if let Some(n) = v.strip_suffix("rem") { return num(n).map(|n| n * 16.) }
    if let Some(n) = v.strip_suffix("em") { return num(n).map(|n| n * em) }
    if let Some(n) = v.strip_suffix('%') { return num(n).map(|n| n / 100. * unitless.unwrap_or(em)) }
    if v == "0" { return Some(0.) }
    num(&v).and_then(|n| unitless.map(|u| n * u))
}

pub fn parse_weight(v: &str) -> Option<f32> {
    match v.trim() { "normal" => Some(400.), "bold" => Some(700.), n => n.parse().ok().filter(|n| (1. ..=1000.).contains(n)) }
}

pub fn border(v: &str) -> (Option<String>, Option<&'static str>, Option<String>) {
    let (mut w, mut s, mut c) = (None, None, None);
    let mut parts = vec![];
    let (mut depth, mut cur) = (0, String::new());
    for ch in v.chars() {
        match ch {
            '(' => { depth += 1; cur.push(ch) }
            ')' => { depth -= 1; cur.push(ch) }
            ' ' if depth == 0 => { if !cur.is_empty() { parts.push(std::mem::take(&mut cur)) } }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() { parts.push(cur) }
    for p in parts {
        match p.as_str() {
            "solid" => s = Some("solid"),
            "dotted" => s = Some("dotted"),
            "dashed" => s = Some("dashed"),
            "none" => w = Some("0".into()),
            p if p.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.') => w = Some(p.to_string()),
            p => c = Some(p.to_string()),
        }
    }
    (w, s, c)
}

pub fn palette_names(kind: &str) -> Vec<&'static str> {
    match kind {
        "color" | "border" => palette::HUES.to_vec(),
        "highlight" => palette::BG_COLORS.iter().flatten().copied().collect(),
        _ => palette::CALLOUT_BG_COLORS.iter().flatten().copied().collect(),
    }
}

#[derive(Clone, Debug)]
pub enum CssVal {
    Len(f32),
    Weight(f32),
    Color(Rgba),
    Inherit,
    Font(String),
    Bool(bool),
    Line(&'static str),
}

#[derive(Clone, Debug)]
pub struct ParsedTheme {
    pub id: String,
    pub feishu: bool,
    pub meta: ThemeMeta,
    pub vars: HashMap<String, String>,
    pub palette: HashMap<String, Rgba>,
    pub tokens: HashMap<String, Rgba>,
    pub props: HashMap<&'static str, CssVal>,
    pub warnings: Vec<String>,
}

impl ParsedTheme {
    pub fn font_family(&self, key: &str) -> Option<&str> {
        match self.props.get(key) { Some(CssVal::Font(f)) => Some(f.as_str()), _ => None }
    }
    pub fn color(&self, key: &str) -> Option<Rgba> {
        match self.props.get(key) { Some(CssVal::Color(c)) => Some(*c), _ => None }
    }
    pub fn num(&self, key: &str) -> Option<f32> {
        match self.props.get(key) { Some(CssVal::Len(n)) | Some(CssVal::Weight(n)) => Some(*n), _ => None }
    }
    pub fn named(&self, kind: &str, name: &str) -> Option<Rgba> {
        self.palette.get(&format!("{kind}.{name}")).copied()
    }
}

/// Pure-Rust parse & lint of a Quiet.md theme stylesheet.
pub fn parse_css(id: &str, css: &str) -> Result<ParsedTheme, String> {
    let rules = rules(css);
    let mut vars: HashMap<String, String> = HashMap::new();
    for r in rules.iter().filter(|r| r.sels.iter().any(|s| s == ":root")) {
        for (n, v) in &r.decls { if n.starts_with("--") { vars.insert(n.clone(), v.clone()); } }
    }
    let var = |n: &str| vars.get(n).map(|v| unquote(&subst(v, &vars, 0)));
    let feishu = match var("--theme-base").as_deref() { None | Some("craft") => false, Some("feishu") => true, Some(b) => return Err(format!("--theme-base: {b}")) };
    let meta = ThemeMeta { name: var("--theme-name"), name_en: var("--theme-name-en"), author: var("--theme-author"), description: var("--theme-description") };
    let mut palette = HashMap::new();
    let mut tokens = HashMap::new();
    let mut warn = vec![];
    for (n, v) in &vars {
        let v = subst(v, &vars, 0);
        for kind in ["color", "background", "callout", "highlight", "border"] {
            if let Some(name) = n.strip_prefix(&format!("--{kind}-")).filter(|name| palette_names(kind).contains(name)) {
                match parse_color_rgba(&v) { Some(c) => { palette.insert(format!("{kind}.{name}"), c); } None => warn.push(format!("{n}: {v}")) }
            }
        }
        if let Some(t) = n.strip_prefix("--token-") {
            let t = t.replace('-', "_");
            match (TOKEN_NAMES.iter().find(|x| **x == t), parse_color_rgba(&v)) {
                (Some(tok), Some(c)) => { tokens.insert((*tok).to_string(), c); }
                _ => warn.push(format!("{n}: {v}")),
            }
        }
    }
    let mut raw: Vec<(&'static PropSpec, String)> = vec![];
    for r in &rules {
        for sel in &r.sels {
            if sel == ":root" { continue }
            for (n, v) in &r.decls {
                let v = subst(v, &vars, 0);
                let mut set = |prop: &str, val: String| {
                    let targets: Vec<&str> = if sel == "li::marker" { vec!["ul li::marker", "ol li::marker"] } else { vec![sel.as_str()] };
                    let mut hit = false;
                    for t in targets {
                        if let Some(p) = PROPS.iter().find(|p| p.sel == t && p.prop == prop) { raw.push((p, val.clone())); hit = true; }
                    }
                    if !hit { warn.push(format!("{sel} {{ {prop} }}")) }
                };
                match n.as_str() {
                    "border-left" | "border-top" | "border" => {
                        let side = if n == "border" && sel == "hr" { "border-top" } else { n.as_str() };
                        let (w, s, c) = border(&v);
                        if let Some(w) = w { set(&format!("{side}-width"), w) }
                        if let Some(s) = s.filter(|_| side == "border-top") { set("border-top-style", s.into()) }
                        if let Some(c) = c { set(&format!("{side}-color"), c) }
                    }
                    "background" => set("background-color", v),
                    "padding" => {
                        let p: Vec<&str> = v.split_whitespace().collect();
                        let (top, left) = match p.len() { 1 => (p[0], p[0]), 2 | 3 => (p[0], p[1]), _ => (p[0], p[3]) };
                        set("padding-top", top.into());
                        set("padding-left", left.into());
                    }
                    "height" if sel == ".task" => set("width", v),
                    "text-decoration" | "text-decoration-line" => {
                        let mut parts = v.split_whitespace();
                        let first = parts.next().unwrap_or("").to_string();
                        let rest: Vec<&str> = parts.collect();
                        set("text-decoration", first);
                        if !rest.is_empty() && sel == "a" { set("text-decoration-color", rest.join(" ")) }
                    }
                    "border-style" if sel == "hr" => set("border-top-style", v),
                    "border-width" if sel == "hr" => set("border-top-width", v),
                    "border-color" if sel == "hr" => set("border-top-color", v),
                    _ => set(n, v),
                }
            }
        }
    }
    let last = |key: &str| raw.iter().rev().find(|(p, _)| p.key == key).map(|(_, v)| v.clone());
    let body_def = if feishu { 16. } else { 15. };
    let body = last("page.size").and_then(|v| parse_len(&v, 16., None)).unwrap_or(body_def);
    let size_of = |el: &str| -> f32 {
        let key = format!("{el}.size");
        last(&key).and_then(|v| parse_len(&v, body, None)).unwrap_or(body)
    };
    let mut props = HashMap::new();
    for (p, v) in &raw {
        let el = p.key.split('.').next().unwrap_or("");
        let em = match el { "page" | "p" | "list" | "quote" | "task" | "code" | "callout" | "hr" | "strong" | "a" | "done" | "bullet" | "number" | "selection" | "th" => body, e => size_of(e) };
        let unitless = (p.prop == "line-height").then_some(em);
        let val = match p.kind {
            PropKind::Len => parse_len(v, if p.prop == "font-size" { body } else { em }, unitless).filter(|n| (0. ..2000.).contains(n)).map(CssVal::Len),
            PropKind::Weight => parse_weight(v).map(CssVal::Weight),
            PropKind::Color if matches!(v.trim().to_lowercase().as_str(), "inherit" | "currentcolor") => Some(CssVal::Inherit),
            PropKind::Color => parse_color_rgba(v).map(CssVal::Color),
            PropKind::Font => (!v.trim().is_empty()).then(|| CssVal::Font(v.clone())),
            PropKind::Underline => match v.trim() { "underline" => Some(CssVal::Bool(true)), "none" => Some(CssVal::Bool(false)), _ => None },
            PropKind::Strike => match v.trim() { "line-through" => Some(CssVal::Bool(true)), "none" => Some(CssVal::Bool(false)), _ => None },
            PropKind::Display => match v.trim() { "none" => Some(CssVal::Bool(false)), "block" | "inline" | "flex" => Some(CssVal::Bool(true)), _ => None },
            PropKind::Italic => match v.trim() { "italic" | "oblique" => Some(CssVal::Bool(true)), "normal" => Some(CssVal::Bool(false)), _ => None },
            PropKind::LineStyle => match v.trim() { "dotted" => Some(CssVal::Line("dotted")), "solid" => Some(CssVal::Line("solid")), "dashed" => Some(CssVal::Line("dashed")), _ => None },
        };
        let Some(val) = val else { warn.push(format!("{} {{ {}: {v} }}", p.sel, p.prop)); continue };
        if feishu && !p.feishu_used { warn.push(format!("{} {{ {} }} — unused by the feishu base", p.sel, p.prop)); continue }
        props.insert(p.key, val);
    }
    warn.sort();
    warn.dedup();
    Ok(ParsedTheme { id: id.to_string(), feishu, meta, vars, palette, tokens, props, warnings: warn })
}

pub fn load_theme_for_library(root: &Path, id: &str) -> Result<ParsedTheme, String> {
    let custom = root.join(crate::store::INTERNAL).join("themes").join(format!("{id}.css"));
    if custom.is_file() {
        let css = std::fs::read_to_string(&custom).map_err(|e| e.to_string())?;
        return parse_css(id, &css);
    }
    let (_, css) = BUNDLED.iter().find(|(b, _)| *b == id).ok_or_else(|| format!("unknown theme '{id}'"))?;
    parse_css(id, css)
}

pub fn active_theme_id(root: &Path) -> String {
    let p = root.join(crate::store::INTERNAL).join("preferences.json");
    let Ok(bytes) = std::fs::read(&p) else { return "feishu".into() };
    let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return "feishu".into() };
    if let Some(t) = val.get("theme").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) { return t.to_string(); }
    if val.get("feishu").and_then(|v| v.as_bool()) == Some(false) { "craft".into() } else { "feishu".into() }
}

pub fn rgba_css(c: Rgba) -> String {
    if c.a >= 0.999 { format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b) }
    else { format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a) }
}

/// Extra CSS rules so headless HTML export reflects the active document theme.
pub fn html_overrides(t: &ParsedTheme) -> String {
    let mut s = String::new();
    let _ = write!(s, "body{{padding:32px 24px 64px;");
    if let Some(bg) = t.color("page.bg") { let _ = write!(s, "background:{};", rgba_css(bg)); }
    if let Some(fg) = t.color("page.color") { let _ = write!(s, "color:{};", rgba_css(fg)); }
    if let Some(f) = t.font_family("page.font") { let _ = write!(s, "font-family:{f};"); }
    if let Some(sz) = t.num("page.size") { let _ = write!(s, "font-size:{sz:.1}px;"); }
    if let Some(lh) = t.num("page.line") { let _ = write!(s, "line-height:{lh:.1}px;"); }
    s.push('}');
    let max_w = t.num("page.width").unwrap_or(820.);
    let _ = write!(s, ".page-wrap{{max-width:{max_w:.0}px;margin:0 auto;}}");
    if let Some(n) = t.num("p.gap") { let _ = write!(s, ".b,.li{{margin:{n:.1}px 0;}}"); }
    if let Some(c) = t.color("title.color") { let _ = write!(s, "h1.title{{color:{};}}", rgba_css(c)); }
    if let Some(sz) = t.num("title.size") { let _ = write!(s, "h1.title{{font-size:{sz:.1}px;}}"); }
    if let Some(w) = t.num("title.weight") { let _ = write!(s, "h1.title{{font-weight:{w:.0};}}"); }
    for l in 1..=9 {
        let mut decls = String::new();
        if let Some(sz) = t.num(&format!("h{l}.size")) { let _ = write!(decls, "font-size:{sz:.1}px;"); }
        if let Some(lh) = t.num(&format!("h{l}.line")) { let _ = write!(decls, "line-height:{lh:.1}px;"); }
        if let Some(w) = t.num(&format!("h{l}.weight")) { let _ = write!(decls, "font-weight:{w:.0};"); }
        if let Some(mt) = t.num(&format!("h{l}.mt")) { let _ = write!(decls, "margin-top:{mt:.1}px;"); }
        if let Some(mb) = t.num(&format!("h{l}.mb")) { let _ = write!(decls, "margin-bottom:{mb:.1}px;"); }
        if let Some(c) = t.color(&format!("h{l}.color")) { let _ = write!(decls, "color:{};", rgba_css(c)); }
        if !decls.is_empty() { let _ = write!(s, ".h{l}{{{decls}}}"); }
    }
    if let Some(c) = t.color("a.color") { let _ = write!(s, "a{{color:{};}}", rgba_css(c)); }
    if let Some(c) = t.color("quote.bar-color") { let _ = write!(s, ".quote{{border-left-color:{};}}", rgba_css(c)); }
    if let Some(w) = t.num("quote.bar") { let _ = write!(s, ".quote{{border-left-width:{w:.1}px;}}"); }
    if let Some(c) = t.color("quote.color") { let _ = write!(s, ".quote{{color:{};}}", rgba_css(c)); }
    if let Some(c) = t.color("pre.bg") { let _ = write!(s, "pre{{background:{};}}", rgba_css(c)); }
    if let Some(c) = t.color("pre.border") { let _ = write!(s, "pre{{border-color:{};}}", rgba_css(c)); }
    if let Some(c) = t.color("pre.color") { let _ = write!(s, "pre{{color:{};}}", rgba_css(c)); }
    if let Some(c) = t.color("th.bg") { let _ = write!(s, "th{{background:{};}}", rgba_css(c)); }
    if let Some(c) = t.color("table.line") {
        let rc = rgba_css(c);
        let _ = write!(s, "td,th,tr:first-child>*,tr>:first-child{{border-color:{rc};}}");
    }
    s
}
