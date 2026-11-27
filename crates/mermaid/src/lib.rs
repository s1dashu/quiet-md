//! Mermaid diagrams drawn as SVG, written for Quiet.md instead of a port of
//! mermaid.js: flowcharts (`graph` / `flowchart`) and sequence diagrams,
//! parsed here, laid out in layers (flowcharts, `layout.rs`) or in lanes
//! (sequence diagrams) and written as plain SVG that resvg draws: no CSS, no
//! foreignObject, no markers. Colours come from the host (`Palette`) and text
//! is measured by it (`Measure`), so labels fit their boxes in the font the
//! SVG names. Anything not understood is an `Error` with its line, never a
//! panic (the app is built with `panic = "abort"`).

mod flow;
mod layout;
mod seq;
mod svg;
mod text;

/// Text widths in the font the SVG names (`Style::font`), from the host.
pub trait Measure {
    fn width(&self, text: &str, size: f32, bold: bool) -> f32;
}

/// A rough measure for tests and hosts without fonts: Latin at about half
/// an em, CJK and other wide characters at a full em.
pub struct Approx;
impl Measure for Approx {
    fn width(&self, text: &str, size: f32, bold: bool) -> f32 {
        text.chars().map(|c| if text::wide(c) { 1. } else if c == ' ' { 0.28 } else if c.is_ascii_uppercase() || c == 'm' || c == 'w' { 0.68 } else { 0.54 }).sum::<f32>() * size * if bold { 1.06 } else { 1. }
    }
}

/// The colours a diagram is drawn in, as `#rrggbb` (any SVG paint works).
#[derive(Clone, Debug)]
pub struct Palette {
    pub background: String,
    pub text: String,
    /// Edges, arrows, lifelines.
    pub line: String,
    pub node_fill: String,
    pub node_border: String,
    pub cluster_fill: String,
    pub cluster_border: String,
    pub note_fill: String,
    pub note_border: String,
    pub activation_fill: String,
    /// Frames of loop / alt / opt blocks and their label tabs.
    pub frame: String,
    pub frame_fill: String,
}

impl Default for Palette {
    fn default() -> Self {
        let s = |x: &str| x.to_string();
        Palette {
            background: s("#ffffff"), text: s("#1f2329"), line: s("#646a73"),
            node_fill: s("#eef3fe"), node_border: s("#8fb0f8"), cluster_fill: s("#f5f6f7"), cluster_border: s("#dee0e3"),
            note_fill: s("#fef7d9"), note_border: s("#f7d67a"), activation_fill: s("#f5f6f7"), frame: s("#8f959e"), frame_fill: s("#f5f6f7"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Style {
    /// An SVG `font-family` list.
    pub font: String,
    pub font_size: f32,
    pub colors: Palette,
}

impl Default for Style {
    fn default() -> Self { Style { font: "sans-serif".into(), font_size: 14., colors: Palette::default() } }
}

/// A drawn diagram: the SVG document and its size in user units (points).
#[derive(Clone, Debug)]
pub struct Svg {
    pub svg: String,
    pub width: f32,
    pub height: f32,
}

/// Why a source isn't drawn: a message (English) and its 1-based line, 0
/// when it is about the whole source.
#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.line > 0 { write!(f, "line {}: {}", self.line, self.message) } else { f.write_str(&self.message) }
    }
}

pub(crate) fn err<T>(line: usize, message: impl Into<String>) -> Result<T, Error> { Err(Error { line, message: message.into() }) }

/// The diagram kinds drawn here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Flowchart,
    Sequence,
}

/// A source line: its 1-based number and its text, trimmed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line<'a> {
    pub no: usize,
    pub text: &'a str,
}

/// The source without front matter, `%%{init}%%` directives and `%%`
/// comments, as numbered non-empty lines; the front matter's `title`.
pub(crate) fn lines(src: &str) -> (Vec<Line<'_>>, Option<String>) {
    let mut out = Vec::new();
    let mut title = None;
    let mut it = src.lines().enumerate().peekable();
    // front matter: `---` … `---` before anything else
    while let Some((_, l)) = it.peek() {
        if l.trim().is_empty() { it.next(); } else { break }
    }
    if it.peek().is_some_and(|(_, l)| l.trim() == "---") {
        it.next();
        for (_, l) in it.by_ref() {
            let t = l.trim();
            if t == "---" { break }
            if let Some(v) = t.strip_prefix("title:") { title = Some(v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()); }
        }
    }
    let mut directive = false;
    for (i, l) in it {
        let t = l.trim();
        if directive || t.starts_with("%%{") {
            directive = !t.ends_with("}%%");
            continue;
        }
        if t.is_empty() || t.starts_with("%%") { continue }
        out.push(Line { no: i + 1, text: t });
    }
    (out, title)
}

/// Which kind `src` is, from its first line.
pub fn kind(src: &str) -> Option<Kind> {
    let (lines, _) = lines(src);
    let first = lines.first()?.text;
    let word = first.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("");
    match word {
        "graph" | "flowchart" | "flowchart-v2" | "flowchart-elk" => Some(Kind::Flowchart),
        "sequenceDiagram" => Some(Kind::Sequence),
        _ => None,
    }
}

/// `src` drawn in `style`, its text measured by `m`.
pub fn render(src: &str, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let (lines, title) = lines(src);
    let Some(first) = lines.first() else { return err(0, "The diagram is empty") };
    match kind(src) {
        Some(Kind::Flowchart) => flow::render(&lines, title, style, m),
        Some(Kind::Sequence) => seq::render(&lines, title, style, m),
        None => {
            let word = first.text.split(|c: char| c.is_whitespace() || c == ';' || c == ':').next().unwrap_or("");
            err(first.no, format!("“{word}” diagrams aren't supported yet"))
        }
    }
}
