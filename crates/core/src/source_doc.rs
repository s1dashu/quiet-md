//! A Markdown / MDX file as a normal `Document`, with a map back to its bytes.
//!
//! `project` parses the file once into a base `Document` plus, for every block,
//! where its pieces live in the source. The editor works on that document with
//! the ordinary `Core`. `render` then diffs the current document against the
//! base and turns only the differences into byte patches, so everything the
//! user did not touch (frontmatter, imports, JSX, comments, link definitions,
//! spacing, line endings, marker style) is copied, not regenerated. The diff is
//! always against the base: undoing every edit produces no patches and so the
//! original bytes. The source stays authoritative; a `Projection` is disposable.
use crate::{editor::core::TITLE, model::*};
use markdown::{mdast::Node, MdxSignal, ParseOptions};
use std::{collections::{HashMap, HashSet}, ops::Range, path::{Path, PathBuf}};

/// Largest image the document will decode (its file header is checked first).
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 64_000_000;
const OBJECT: &str = "\u{fffc}";

#[derive(Clone, Debug, PartialEq)]
enum Shape { Para, Heading, Item, Quote, Fence { info: Range<usize>, body: Range<usize> }, Opaque, Table, Cell, Image, Divider, Math }
/// One piece of a block's text and the source bytes it came from. `literal`
/// pieces are byte-identical to their source, so offsets inside them map 1:1.
#[derive(Clone, Debug)]
struct Seg { text: Range<usize>, src: Range<usize>, literal: bool, marks: Marks }
#[derive(Clone, Debug)]
struct Span { outer: Range<usize>, head: Range<usize>, content: Range<usize>, marker: Option<Range<usize>>, segs: Vec<Seg>, shape: Shape, child_prefix: String }

/// Frontmatter and imports: shown as document metadata, never as body blocks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta { pub fields: Vec<(String, String)>, pub imports: Vec<String> }
impl Meta { pub fn is_empty(&self) -> bool { self.fields.is_empty() && self.imports.is_empty() } }

#[derive(Clone, Debug)]
pub struct Projection {
    pub doc: Document,
    pub source: String,
    pub meta: Meta,
    /// Heading slugs, for `#anchor` links.
    pub anchors: HashMap<String, BlockId>,
    /// What the rendered view cannot show faithfully (MDX, a syntax error).
    pub diagnostic: Option<String>,
    spans: HashMap<BlockId, Span>,
    title: Option<Span>,
    /// Where a new first block goes when nothing precedes it.
    body_start: usize,
    eol: &'static str,
    file: PathBuf,
    root: PathBuf,
    mdx: bool,
    /// Inline source shown as code: if rewritten, it goes back without backticks.
    raws: HashSet<String>,
    /// Links this view added to code spans naming a file: never written back.
    code_links: HashSet<String>,
}

fn options(mdx: bool) -> ParseOptions {
    let mut o = if mdx { ParseOptions::mdx() } else { ParseOptions::gfm() };
    let c = &mut o.constructs;
    c.frontmatter = true; c.gfm_table = true; c.gfm_task_list_item = true; c.gfm_strikethrough = true; c.gfm_footnote_definition = true; c.gfm_label_start_footnote = true;
    // `$5 and $10` is prose: only `$$` opens math.
    c.gfm_autolink_literal = true;
    c.math_flow = true; c.math_text = true;
    o.math_text_single_dollar = false;
    // Imports are never evaluated: this only tells the parser where one ends.
    if mdx { o.mdx_esm_parse = Some(Box::new(|value: &str| {
        let depth = value.chars().fold(0i32, |d, c| match c { '{' | '(' | '[' => d + 1, '}' | ')' | ']' => d - 1, _ => d });
        if depth > 0 { MdxSignal::Eof("Unclosed import".into(), Box::new("quiet.md".into()), Box::new("esm".into())) } else { MdxSignal::Ok }
    })); }
    o
}
pub fn project(source: &str, file: &Path, root: &Path) -> Projection {
    let wants_mdx = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("mdx"));
    let plain = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt"));
    let offset = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let body = &source[offset..];
    let eol = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let mut diagnostic = None;
    let mut mdx = wants_mdx;
    let tree = if plain { None } else {
        match markdown::to_mdast(body, &options(wants_mdx)) {
            Ok(tree) => Some(tree),
            Err(e) => {
                // A JSX typo must not cost the whole reading view.
                mdx = false;
                diagnostic = Some(format!("MDX syntax error, shown as Markdown; the file is unchanged: {e}"));
                markdown::to_mdast(body, &options(false)).ok()
            }
        }
    };
    let stem = file.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let mut b = Builder { src: source, offset, root, file, mdx, spans: HashMap::new(), anchors: HashMap::new(), slugs: HashSet::new(), defs: HashMap::new(), meta: Meta::default(), opaque: false, frontmatter_title: None, raws: HashSet::new(), code_links: HashMap::new() };
    let mut doc = Document { title: RichText::default(), icon: None, cover: None, blocks: vec![] };
    let mut title = None;
    let mut body_start = offset;
    if let Some(tree) = &tree {
        let children = tree.children().cloned().unwrap_or_default();
        b.collect_defs(&children);
        // Leading frontmatter and imports stay above the body.
        let mut first = 0;
        while let Some(node) = children.get(first) {
            match node {
                Node::Yaml(y) => { let t = parse_fields(&y.value); b.take_fields(t); },
                Node::Toml(y) => { let t = parse_fields(&y.value.replace(" = ", ": ")); b.take_fields(t); },
                Node::MdxjsEsm(_) => b.meta.imports.push(b.raw(node).trim().to_string()),
                Node::Definition(_) => {},
                _ => break,
            }
            body_start = b.next_line(b.range(node).end);
            first += 1;
        }
        if let Some(Node::Heading(h)) = children.get(first) && h.depth == 1 {
            let (rich, segs, content) = b.rich(&h.children, Marks::default(), b.range(&children[first]));
            if let Some(anchor) = slug(&rich.text) { b.anchors.insert(anchor.clone(), TITLE); b.slugs.insert(anchor); }
            let outer = b.range(&children[first]);
            title = Some(Span { head: outer.clone(), content, marker: None, segs, shape: Shape::Heading, child_prefix: String::new(), outer });
            doc.title = rich;
            body_start = b.next_line(title.as_ref().unwrap().outer.end);
            first += 1;
        }
        doc.blocks = b.blocks(&children[first..], 0);
    } else { doc.blocks = if body.is_empty() { vec![] } else { b.plain(body) }; }
    if title.is_none() { doc.title = RichText::plain(b.frontmatter_title.clone().unwrap_or(stem)); }
    let (spans, anchors, meta, opaque, raws, code_links) = (b.spans, b.anchors, b.meta, b.opaque, b.raws, b.code_links.into_values().flatten().collect());
    if diagnostic.is_none() && wants_mdx && (opaque || !meta.imports.is_empty()) {
        diagnostic = Some("MDX components, imports and expressions are shown as source; project code does not run.".into());
    }
    if doc.blocks.is_empty() { doc.blocks.push(Block::text("")); }
    Projection { doc, source: source.to_string(), meta, anchors, diagnostic, spans, title, body_start, eol, file: file.to_path_buf(), root: root.to_path_buf(), mdx, raws, code_links }
}
/// GitHub's alert types: (marker, callout emoji, palette tint).
const ALERTS: [(&str, &str, &str); 5] = [("NOTE", "💡", "blue"), ("TIP", "✅", "green"), ("IMPORTANT", "❗", "purple"), ("WARNING", "⚠️", "yellow"), ("CAUTION", "🚫", "red")];
fn callout(emoji: &str, tint: &str) -> BlockKind {
    BlockKind::Callout { emoji: emoji.into(), bg: Some(ColorName(format!("light-{tint}"))), border: Some(ColorName(tint.into())), text_color: None }
}
/// `$x$` as math, without reading prose as formulas: an opener before a space or
/// digit is a price or a variable, and a closer needs text right before it and
/// none right after (`$5 and $10`, `$HOME/$PATH` stay text).
fn dollar_math(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < bytes.len() {
        if bytes[i] != b'$' { i += 1; continue; }
        let next = text[i + 1..].chars().next();
        let opens = next.is_some_and(|c| !c.is_whitespace() && !c.is_ascii_digit() && c != '$') && (i == 0 || !matches!(bytes[i - 1], b'$' | b'\\') && !(bytes[i - 1] as char).is_alphanumeric());
        if !opens { i += 1; continue; }
        let line_end = text[i..].find(['\n', '\r']).map_or(text.len(), |n| i + n);
        let close = text[i + 1..line_end].char_indices().find(|(n, c)| {
            let at = i + 1 + n;
            *c == '$' && *n > 0 && *n <= 200 && !bytes[at - 1].is_ascii_whitespace() && bytes[at - 1] != b'\\' && text[at + 1..].chars().next().is_none_or(|a| !a.is_alphanumeric() && a != '$')
        });
        match close { Some((n, _)) => { let end = i + 1 + n + 1; out.push(i..end); i = end; } None => i += 1 }
    }
    out
}
fn parse_fields(yaml: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    for line in yaml.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') { continue; }
        // A nested or continued value joins the key above it.
        if line.starts_with([' ', '\t', '-']) {
            if let Some((_, v)) = out.last_mut() { if !v.is_empty() { v.push(' '); } v.push_str(line.trim().trim_start_matches("- ")); }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\''))).unwrap_or(v);
        out.push((k.trim().to_string(), v.replace("\\\"", "\"")));
        if out.len() >= 64 { break; }
    }
    out
}
pub fn slug(text: &str) -> Option<String> {
    let s: String = text.trim().to_lowercase().chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).map(|c| if c == ' ' { '-' } else { c }).collect();
    (!s.is_empty()).then_some(s)
}
fn prefixify(s: &str) -> String { s.chars().map(|c| if c == '>' || c == '\t' { c } else { ' ' }).collect() }

struct Builder<'a> { src: &'a str, offset: usize, root: &'a Path, file: &'a Path, mdx: bool, spans: HashMap<BlockId, Span>, anchors: HashMap<String, BlockId>, slugs: HashSet<String>, defs: HashMap<String, String>, meta: Meta, opaque: bool, frontmatter_title: Option<String>, raws: HashSet<String>, code_links: HashMap<String, Option<String>> }
impl Builder<'_> {
    fn range(&self, n: &Node) -> Range<usize> { n.position().map_or(0..0, |p| p.start.offset + self.offset..p.end.offset + self.offset) }
    fn raw(&self, n: &Node) -> &str { self.src.get(self.range(n)).unwrap_or("") }
    fn next_line(&self, at: usize) -> usize { self.src[at..].find('\n').map_or(self.src.len(), |i| at + i + 1) }
    fn line_start(&self, at: usize) -> usize { self.src[..at].rfind('\n').map_or(self.offset, |i| i + 1) }
    fn line_prefix(&self, at: usize) -> String { prefixify(&self.src[self.line_start(at)..at]) }
    fn take_fields(&mut self, fields: Vec<(String, String)>) {
        self.frontmatter_title = fields.iter().find(|(k, v)| k.eq_ignore_ascii_case("title") && !v.is_empty()).map(|(_, v)| v.clone());
        self.meta.fields = fields;
    }
    fn collect_defs(&mut self, nodes: &[Node]) {
        for n in nodes {
            if let Node::Definition(d) = n { self.defs.entry(d.identifier.clone()).or_insert_with(|| d.url.clone()); }
            if let Some(c) = n.children() { self.collect_defs(c); }
        }
    }
    fn plain(&mut self, body: &str) -> Vec<Block> {
        // A .txt file: one paragraph per line, bytes untouched.
        let mut out = vec![]; let mut at = self.offset;
        for line in body.split_inclusive('\n') {
            let t = line.trim_end_matches(['\r', '\n']);
            if !t.is_empty() {
                let b = Block::text(t); let r = at..at + t.len();
                self.spans.insert(b.id, Span { outer: r.clone(), head: r.clone(), content: r.clone(), marker: None, segs: vec![Seg { text: 0..t.len(), src: r, literal: true, marks: Marks::default() }], shape: Shape::Para, child_prefix: String::new() });
                out.push(b);
            }
            at += line.len();
        }
        out
    }
    fn blocks(&mut self, nodes: &[Node], depth: usize) -> Vec<Block> {
        let mut out = vec![];
        for node in nodes {
            let outer = self.range(node);
            if depth >= 48 { out.push(self.opaque_block(node, "markdown")); continue; }
            match node {
                Node::Yaml(_) | Node::Toml(_) | Node::Definition(_) => {},
                Node::MdxjsEsm(_) => self.meta.imports.push(self.raw(node).trim().to_string()),
                Node::Html(h) if h.value.trim_start().starts_with("<!--") && h.value.trim_end().ends_with("-->") => {},
                Node::Heading(h) => {
                    let (rich, segs, content) = self.rich(&h.children, Marks::default(), outer.clone());
                    let b = Block::new(BlockKind::Heading { level: h.depth.clamp(1, 6), seq_auto: false }, rich);
                    if let Some(base) = slug(&b.text.text) {
                        let (mut key, mut i) = (base.clone(), 1);
                        while !self.slugs.insert(key.clone()) { key = format!("{base}-{i}"); i += 1; }
                        self.anchors.insert(key, b.id);
                    }
                    let child_prefix = self.line_prefix(outer.start);
                    self.spans.insert(b.id, Span { head: outer.clone(), outer, content, marker: None, segs, shape: Shape::Heading, child_prefix });
                    out.push(b);
                }
                Node::Paragraph(p) => {
                    let images = p.children.iter().all(|c| matches!(c, Node::Image(_) | Node::ImageReference(_)) || matches!(c, Node::Text(t) if t.value.trim().is_empty()));
                    if images && p.children.iter().any(|c| matches!(c, Node::Image(_) | Node::ImageReference(_))) {
                        for c in &p.children { if !matches!(c, Node::Text(_)) { out.push(self.image_block(c)); } }
                    } else { out.push(self.paragraph(&p.children, outer)); }
                }
                Node::List(list) => {
                    let mut seq = list.ordered.then_some(list.start.unwrap_or(1));
                    for item in &list.children { if let Node::ListItem(i) = item { out.push(self.item(item, i, seq, depth)); seq = seq.map(|n| n + 1); } }
                }
                Node::Blockquote(q) => {
                    let (kind, children) = self.alert(&q.children);
                    let mut b = Block::new(kind, RichText::default());
                    b.children = self.blocks(&children, depth + 1);
                    let child_prefix = q.children.first().map(|c| self.line_prefix(self.range(c).start)).unwrap_or_else(|| format!("{}> ", self.line_prefix(outer.start)));
                    self.spans.insert(b.id, Span { head: outer.start..outer.start, content: outer.start..outer.start, outer, marker: None, segs: vec![], shape: Shape::Quote, child_prefix });
                    out.push(b);
                }
                Node::Code(c) => {
                    let lang = crate::lang::from_token(c.lang.as_deref().unwrap_or(""));
                    let b = Block::new(BlockKind::Code { lang, caption: None, wrap: true }, RichText::plain(&c.value));
                    let raw = &self.src[outer.clone()];
                    let fenced = raw.starts_with("```") || raw.starts_with("~~~");
                    let shape = if fenced && let Some(nl) = raw.find('\n') {
                        let line_end = outer.start + nl;
                        let ticks = raw.chars().take_while(|c| *c == raw.chars().next().unwrap()).count();
                        let info = outer.start + ticks..self.src[..line_end].trim_end().len().max(outer.start + ticks);
                        // Without a closing fence the body runs to the end.
                        let tail = raw.rfind('\n').unwrap_or(raw.len());
                        let closed = tail > nl && raw[tail + 1..].trim_start_matches([' ', '>']).starts_with(&raw[..ticks]);
                        let end = if closed { outer.start + tail } else { outer.end };
                        let end = if closed && self.src[..end].ends_with('\r') { end - 1 } else { end };
                        Shape::Fence { info, body: (line_end + 1).min(end)..end }
                    } else { Shape::Opaque };
                    let child_prefix = self.line_prefix(outer.start);
                    self.spans.insert(b.id, Span { head: outer.clone(), content: outer.clone(), outer, marker: None, segs: vec![], shape, child_prefix });
                    out.push(b);
                }
                Node::Math(m) => {
                    let mut t = RichText::default();
                    t.push(OBJECT, Marks { inline: Some(crate::docxml::to_inline(InlineKind::Equation(m.value.clone()))), ..Default::default() });
                    let b = Block::new(BlockKind::Text, t);
                    let child_prefix = self.line_prefix(outer.start);
                    self.spans.insert(b.id, Span { head: outer.clone(), content: outer.clone(), outer, marker: None, segs: vec![], shape: Shape::Math, child_prefix });
                    out.push(b);
                }
                Node::ThematicBreak(_) => {
                    let b = Block::new(BlockKind::Divider, RichText::default());
                    let child_prefix = self.line_prefix(outer.start);
                    self.spans.insert(b.id, Span { head: outer.clone(), content: outer.clone(), outer, marker: None, segs: vec![], shape: Shape::Divider, child_prefix });
                    out.push(b);
                }
                Node::Table(t) => {
                    let cols = t.align.len().max(1);
                    let mut rows = vec![];
                    for row in &t.children {
                        let mut cells = vec![];
                        if let Node::TableRow(r) = row { for cell in &r.children { if let Node::TableCell(c) = cell { cells.push(self.cell(&c.children, self.range(cell))); } } }
                        cells.truncate(cols);
                        while cells.len() < cols { cells.push(TableCell { blocks: vec![Block::text("")], ..TableCell::empty() }); }
                        rows.push(cells);
                    }
                    let mut table = Table { col_widths: vec![200.; cols], header_rows: 1, header_col: false, rows };
                    table.repair();
                    let b = Block::new(BlockKind::Table(table), RichText::default());
                    let child_prefix = self.line_prefix(outer.start);
                    self.spans.insert(b.id, Span { head: outer.clone(), content: outer.clone(), outer, marker: None, segs: vec![], shape: Shape::Table, child_prefix });
                    out.push(b);
                }
                Node::Html(_) => { self.opaque |= self.mdx; out.push(self.opaque_block(node, "HTML")); }
                _ => { self.opaque = true; out.push(self.opaque_block(node, if self.mdx { "mdx" } else { "markdown" })); }
            }
        }
        out
    }
    /// A GitHub alert (`> [!NOTE]`) is a callout. The marker stays in the source,
    /// outside every block's span, so it is never rewritten.
    fn alert(&self, children: &[Node]) -> (BlockKind, Vec<Node>) {
        let plain = || (BlockKind::Quote, children.to_vec());
        let Some(Node::Paragraph(p)) = children.first() else { return plain() };
        let Some(Node::Text(first)) = p.children.first() else { return plain() };
        let Some((name, kind)) = ALERTS.iter().find_map(|(name, emoji, tint)| first.value.starts_with(&format!("[!{name}]")).then(|| (name, callout(emoji, tint)))) else { return plain() };
        let marker = name.len() + 3;
        let rest = first.value[marker..].trim_start_matches([' ', '\t']);
        if !rest.is_empty() && !rest.starts_with(['\n', '\r']) { return plain(); }
        let value = rest.trim_start_matches(['\r', '\n']).to_string();
        let mut out = children.to_vec();
        if value.is_empty() && p.children.len() == 1 { out.remove(0); return (kind, out); }
        // The paragraph now starts after the marker's line and its `> ` prefix.
        let Some(start) = first.position.as_ref().map(|pos| pos.start.offset + self.offset + marker) else { return plain() };
        let tail = &self.src[start..];
        let skip = tail.len() - tail.trim_start_matches([' ', '\t', '\r', '\n', '>']).len();
        let offset = start + skip - self.offset;
        let Some(Node::Paragraph(para)) = out.first_mut() else { return plain() };
        if let Some(Node::Text(text)) = para.children.first_mut() {
            text.value = value;
            if let Some(pos) = text.position.as_mut() { pos.start.offset = offset; }
        }
        if let Some(pos) = para.position.as_mut() { pos.start.offset = offset; }
        (kind, out)
    }
    fn opaque_block(&mut self, node: &Node, lang: &str) -> Block {
        let outer = self.range(node);
        let caption = match node {
            Node::MdxJsxFlowElement(_) | Node::MdxFlowExpression(_) => Some("MDX source, not executed".to_string()),
            Node::Html(_) => Some("HTML source".to_string()),
            _ => None,
        };
        let b = Block::new(BlockKind::Code { lang: lang.into(), caption, wrap: true }, RichText::plain(self.raw(node)));
        let child_prefix = self.line_prefix(outer.start);
        self.spans.insert(b.id, Span { head: outer.clone(), content: outer.clone(), outer, marker: None, segs: vec![], shape: Shape::Opaque, child_prefix });
        b
    }
    fn paragraph(&mut self, children: &[Node], outer: Range<usize>) -> Block {
        let (rich, segs, content) = self.rich(children, Marks::default(), outer.clone());
        let b = Block::new(BlockKind::Text, rich);
        let child_prefix = self.line_prefix(outer.start);
        self.spans.insert(b.id, Span { head: outer.clone(), outer, content, marker: None, segs, shape: Shape::Para, child_prefix });
        b
    }
    fn cell(&mut self, children: &[Node], outer: Range<usize>) -> TableCell {
        let (rich, segs, content) = self.rich(children, Marks::default(), outer.clone());
        // An empty cell's content is the blank between its pipes.
        let content = if children.is_empty() { let m = self.src[outer.clone()].trim_end().trim_end_matches('|').len(); outer.start + m..outer.start + m } else { content };
        let b = Block::new(BlockKind::Text, rich);
        self.spans.insert(b.id, Span { head: content.clone(), outer, content, marker: None, segs, shape: Shape::Cell, child_prefix: String::new() });
        TableCell { blocks: vec![b], ..TableCell::empty() }
    }
    fn item(&mut self, node: &Node, item: &markdown::mdast::ListItem, seq: Option<u32>, depth: usize) -> Block {
        let outer = self.range(node);
        let kind = match (item.checked, seq) { (Some(done), _) => BlockKind::Todo { done }, (None, Some(n)) => BlockKind::Ordered { seq: Some(n) }, _ => BlockKind::Bullet };
        let first = item.children.first().filter(|c| matches!(c, Node::Paragraph(_)));
        let (rich, segs, content, rest) = match first {
            Some(Node::Paragraph(p)) => { let r = self.range(first.unwrap()); let (a, b, c) = self.rich(&p.children, Marks::default(), r); (a, b, c, &item.children[1..]) },
            _ => { let (_, marker_end) = self.marker_end(&outer, item.checked.is_some()); (RichText::default(), vec![], marker_end..marker_end, &item.children[..]) },
        };
        let mut b = Block::new(kind, rich);
        let marker = item.checked.is_some().then(|| self.task_marker(&outer)).flatten();
        // Children align with this item's content column, however it was indented.
        let child_prefix = rest.first().map(|c| self.line_prefix(self.range(c).start)).unwrap_or_else(|| self.line_prefix(content.start));
        let head_end = content.end.max(content.start);
        b.children = self.blocks(rest, depth + 1);
        self.spans.insert(b.id, Span { head: outer.start..head_end, outer, content, marker, segs, shape: Shape::Item, child_prefix });
        b
    }
    /// (marker start, content start) of an item with no leading paragraph.
    fn marker_end(&self, outer: &Range<usize>, task: bool) -> (usize, usize) {
        let raw = &self.src[outer.clone()];
        let mut i = raw.find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '*' | '.' | ')'))).unwrap_or(raw.len());
        if task && let Some(m) = self.task_marker(outer) { i = m.end - outer.start; }
        let spaces = raw[i..].chars().take_while(|c| *c == ' ').count();
        (outer.start, outer.start + i + spaces)
    }
    fn task_marker(&self, outer: &Range<usize>) -> Option<Range<usize>> {
        let raw = &self.src[outer.clone()];
        let open = raw.find('[')?;
        // Only the marker straight after the list prefix.
        if raw[..open].chars().any(|c| !(c.is_ascii_digit() || matches!(c, '-' | '+' | '*' | '.' | ')' | ' ' | '\t'))) { return None; }
        matches!(raw.get(open..open + 3), Some("[ ]" | "[x]" | "[X]")).then_some(outer.start + open..outer.start + open + 3)
    }
    fn image_block(&mut self, node: &Node) -> Block {
        let outer = self.range(node);
        let (url, alt) = match node {
            Node::Image(i) => (i.url.clone(), i.alt.clone()),
            Node::ImageReference(i) => (self.defs.get(&i.identifier).cloned().unwrap_or_default(), i.alt.clone()),
            _ => Default::default(),
        };
        let child_prefix = self.line_prefix(outer.start);
        let b = match resolve_image(self.root, self.file, &url) {
            Some(src) => Block::new(BlockKind::Image { src_kind: "src".into(), src, width: None, height: None, caption: (!alt.is_empty()).then_some(alt), crop: None }, RichText::default()),
            None => {
                let (raw, mut t, mut segs) = (self.raw(node).to_string(), RichText::default(), vec![]);
                self.raw_run(&raw, outer.clone(), &Marks::default(), &mut t, &mut segs);
                let b = Block::new(BlockKind::Text, t);
                self.spans.insert(b.id, Span { head: outer.clone(), content: outer.clone(), outer, marker: None, segs, shape: Shape::Para, child_prefix });
                return b;
            }
        };
        self.spans.insert(b.id, Span { head: outer.clone(), content: outer.clone(), outer, marker: None, segs: vec![], shape: Shape::Image, child_prefix });
        b
    }
    fn rich(&mut self, nodes: &[Node], marks: Marks, fallback: Range<usize>) -> (RichText, Vec<Seg>, Range<usize>) {
        let (mut t, mut segs) = (RichText::default(), vec![]);
        self.inline(nodes, &marks, &mut t, &mut segs);
        let content = match (nodes.first().and_then(|n| n.position()), nodes.last().and_then(|n| n.position())) {
            (Some(a), Some(b)) => a.start.offset + self.offset..b.end.offset + self.offset,
            _ => fallback.end..fallback.end,
        };
        (t, segs, content)
    }
    fn inline(&mut self, nodes: &[Node], marks: &Marks, t: &mut RichText, segs: &mut Vec<Seg>) {
        for n in nodes {
            let r = self.range(n);
            match n {
                Node::Text(x) if x.value.contains('$') && self.src[r.clone()] == x.value => {
                    let mut at = 0;
                    for span in dollar_math(&x.value) {
                        self.text_node(&x.value[at..span.start], r.start + at..r.start + span.start, marks, t, segs);
                        let eq = crate::docxml::to_inline(InlineKind::Equation(x.value[span.start + 1..span.end - 1].to_string()));
                        self.object(eq, r.start + span.start..r.start + span.end, marks, t, segs);
                        at = span.end;
                    }
                    self.text_node(&x.value[at..], r.start + at..r.end, marks, t, segs);
                }
                Node::Text(x) => self.text_node(&x.value, r, marks, t, segs),
                Node::Emphasis(x) => self.inline(&x.children, &Marks { italic: true, ..marks.clone() }, t, segs),
                Node::Strong(x) => self.inline(&x.children, &Marks { bold: true, ..marks.clone() }, t, segs),
                Node::Delete(x) => self.inline(&x.children, &Marks { strike: true, ..marks.clone() }, t, segs),
                Node::Link(x) => self.inline(&x.children, &Marks { link: Some(x.url.clone()), ..marks.clone() }, t, segs),
                Node::LinkReference(x) => {
                    let url = self.defs.get(&x.identifier).cloned();
                    self.inline(&x.children, &Marks { link: url, ..marks.clone() }, t, segs);
                }
                Node::InlineCode(x) => {
                    let link = if marks.link.is_none() { self.code_link(&x.value) } else { None };
                    let m = Marks { code: true, link: link.or_else(|| marks.link.clone()), ..marks.clone() };
                    let at = t.len();
                    t.push(&x.value, m.clone());
                    let raw = &self.src[r.clone()];
                    let seg = raw.find(&x.value).map_or_else(
                        || Seg { text: at..t.len(), src: r.clone(), literal: false, marks: m.clone() },
                        |i| Seg { text: at..t.len(), src: r.start + i..r.start + i + x.value.len(), literal: true, marks: m.clone() });
                    segs.push(seg);
                }
                Node::Break(_) => { let at = t.len(); t.push("\n", marks.clone()); segs.push(Seg { text: at..t.len(), src: r, literal: false, marks: marks.clone() }); }
                Node::InlineMath(x) => self.object(crate::docxml::to_inline(InlineKind::Equation(x.value.clone())), r, marks, t, segs),
                // JSX, expressions, HTML, footnote marks, inline images: their source,
                // shown as code and mapped 1:1, so editing it is a plain byte patch.
                _ => {
                    self.opaque |= self.mdx && !matches!(n, Node::Html(_));
                    let raw = self.raw(n).to_string();
                    self.raw_run(&raw, r, marks, t, segs);
                }
            }
        }
    }
    /// A code span that names a real file or folder here links to it. Probes
    /// are memoized and bounded; a span with nothing behind it stays plain code.
    fn code_link(&mut self, text: &str) -> Option<String> {
        if let Some(known) = self.code_links.get(text) { return known.clone(); }
        if self.code_links.len() >= 2000 || text.len() > 240 || text.starts_with('-') || text.contains(|c: char| c.is_whitespace() || matches!(c, '*' | '?' | '<' | '>' | '|' | '"' | '\'' | ':' | '$' | '(' | ')')) || !(text.contains('/') || text.contains('.')) { return None; }
        let name = text.trim_end_matches('/');
        let found = [self.file.parent().map(|d| d.join(name)), Some(self.root.join(name.trim_start_matches('/')))].into_iter().flatten().find_map(|candidate| {
            let path = crate::workspace_files::contained(self.root, &candidate).ok()?;
            (path.is_dir() || crate::workspace_files::supported(&path)).then_some(path)
        });
        let url = found.and_then(|p| p.strip_prefix(self.root).ok().map(|r| format!("/{}", r.display())));
        self.code_links.insert(text.to_string(), url.clone());
        url
    }
    fn raw_run(&mut self, raw: &str, src: Range<usize>, marks: &Marks, t: &mut RichText, segs: &mut Vec<Seg>) {
        let m = Marks { code: true, ..marks.clone() };
        let at = t.len();
        t.push(raw, m.clone());
        segs.push(Seg { text: at..t.len(), src, literal: true, marks: m });
        self.raws.insert(raw.to_string());
    }
    fn object(&self, o: Inline, r: Range<usize>, marks: &Marks, t: &mut RichText, segs: &mut Vec<Seg>) {
        let m = Marks { inline: Some(o), ..marks.clone() };
        let at = t.len();
        t.push(OBJECT, m.clone());
        segs.push(Seg { text: at..t.len(), src: r, literal: false, marks: m });
    }
    /// Walk the value and its source together: literal stretches map 1:1;
    /// escapes and line joins become atomic pieces.
    fn text_node(&self, value: &str, range: Range<usize>, marks: &Marks, t: &mut RichText, segs: &mut Vec<Seg>) {
        let raw = &self.src[range.clone()];
        let (mut r, mut v, mut lit) = (0usize, 0usize, 0usize);
        let flush = |t: &mut RichText, segs: &mut Vec<Seg>, from_v: usize, to_v: usize, from_r: usize| {
            if to_v > from_v { let at = t.len(); t.push(&value[from_v..to_v], marks.clone()); segs.push(Seg { text: at..t.len(), src: range.start + from_r..range.start + from_r + (to_v - from_v), literal: true, marks: marks.clone() }); }
        };
        let (mut lit_r, atomic) = (0usize, |t: &mut RichText, segs: &mut Vec<Seg>, s: &str, a: usize, b: usize| {
            let at = t.len(); t.push(s, marks.clone()); segs.push(Seg { text: at..t.len(), src: range.start + a..range.start + b, literal: false, marks: marks.clone() });
        });
        while v < value.len() {
            let vc = value[v..].chars().next().unwrap();
            let Some(rc) = raw[r..].chars().next() else { break };
            if matches!(vc, '\n' | '\r') && matches!(rc, '\n' | '\r' | ' ' | '\t') {
                flush(t, segs, lit, v, lit_r);
                let start = r;
                while raw[r..].starts_with([' ', '\t']) { r += 1; }
                if raw[r..].starts_with('\r') { r += 1; }
                if raw[r..].starts_with('\n') { r += 1; }
                while raw[r..].starts_with([' ', '\t', '>']) { r += 1; }
                atomic(t, segs, " ", start, r);
                v += if value[v..].starts_with("\r\n") { 2 } else { 1 }; lit = v; lit_r = r;
                continue;
            }
            // An entity: one decoded character, atomic.
            if rc == '&' && let Some(semi) = raw[r..].char_indices().take(40).find(|(_, c)| *c == ';').map(|(i, _)| i).filter(|semi| raw[r + 1..r + semi].chars().all(|c| c.is_alphanumeric() || c == '#') && !value[v..].starts_with(&raw[r..=r + semi])) {
                flush(t, segs, lit, v, lit_r);
                atomic(t, segs, &value[v..v + vc.len_utf8()], r, r + semi + 1);
                r += semi + 1; v += vc.len_utf8(); lit = v; lit_r = r;
                continue;
            }
            if rc == vc { r += rc.len_utf8(); v += vc.len_utf8(); continue; }
            if rc == '\\' && raw[r + 1..].starts_with(vc) {
                flush(t, segs, lit, v, lit_r);
                atomic(t, segs, &value[v..v + vc.len_utf8()], r, r + 1 + vc.len_utf8());
                r += 1 + vc.len_utf8(); v += vc.len_utf8(); lit = v; lit_r = r;
                continue;
            }
            // Anything else the walk cannot follow.
            flush(t, segs, lit, v, lit_r);
            atomic(t, segs, &value[v..], r, raw.len());
            return;
        }
        flush(t, segs, lit, v, lit_r);
    }
}
/// A local image inside the folder, or a remote one; anything else stays text.
pub fn resolve_image(root: &Path, file: &Path, url: &str) -> Option<String> {
    if url.starts_with("http://") || url.starts_with("https://") { return Some(url.to_string()); }
    let (path, _) = crate::workspace_files::relative_link(root, file, url).ok()?;
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES { return None; }
    let reader = image::ImageReader::open(&path).ok()?.with_guessed_format().ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    (u64::from(w) * u64::from(h) <= MAX_IMAGE_PIXELS).then(|| path.to_string_lossy().into_owned())
}

// ---- document -> source

type Edit = (Range<usize>, String);
fn same_kind(a: &BlockKind, b: &BlockKind) -> bool {
    match (a, b) {
        (BlockKind::Heading { level: x, .. }, BlockKind::Heading { level: y, .. }) => x == y,
        (BlockKind::Code { lang: x, .. }, BlockKind::Code { lang: y, .. }) => x == y,
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}
fn head_like(k: &BlockKind) -> bool { matches!(k, BlockKind::Text | BlockKind::Heading { .. } | BlockKind::Bullet | BlockKind::Ordered { .. } | BlockKind::Todo { .. }) }
/// Longest increasing subsequence: which positions keep their relative order.
fn lis(values: &[usize]) -> Vec<usize> {
    let (mut tails, mut prev): (Vec<usize>, Vec<Option<usize>>) = (vec![], vec![None; values.len()]);
    for i in 0..values.len() {
        let at = tails.partition_point(|&t| values[t] < values[i]);
        if at > 0 { prev[i] = Some(tails[at - 1]); }
        if at == tails.len() { tails.push(i); } else { tails[at] = i; }
    }
    let mut out = vec![]; let mut cur = tails.last().copied();
    while let Some(i) = cur { out.push(i); cur = prev[i]; }
    out.reverse(); out
}
impl Projection {
    /// The source text for `doc`. No difference from the base, no change.
    pub fn render(&self, doc: &Document) -> String {
        let mut edits: Vec<Edit> = vec![];
        self.patch_title(doc, &mut edits);
        self.diff_siblings(&self.doc.blocks, &doc.blocks, "", self.body_insert(), &mut edits);
        if edits.is_empty() { return self.source.clone(); }
        edits.sort_by_key(|(r, _)| (r.start, r.end));
        let mut out = String::with_capacity(self.source.len() + 64);
        let mut at = 0;
        for (r, text) in edits {
            // Nested inside a region already replaced.
            if r.start < at { continue; }
            out.push_str(&self.source[at..r.start]); out.push_str(&text); at = r.end;
        }
        out.push_str(&self.source[at..]);
        out
    }
    fn body_insert(&self) -> usize { self.title.as_ref().map_or(self.body_start, |t| t.outer.end) }
    fn patch_title(&self, doc: &Document, edits: &mut Vec<Edit>) {
        if doc.title == self.doc.title { return; }
        match &self.title {
            Some(span) if doc.title.text.trim().is_empty() => {
                let end = self.skip_blank_lines(span.outer.end);
                edits.push((span.outer.start..end, String::new()));
            }
            Some(span) => self.patch_text(span, &self.doc.title, &doc.title, edits),
            None if doc.title.text.trim().is_empty() => {},
            None => {
                let heading = format!("# {}", self.inline_md(&doc.title, ""));
                let at = self.body_start;
                let tail = if self.source[at..].trim().is_empty() { self.eol.to_string() } else { format!("{0}{0}", self.eol) };
                edits.push((at..at, format!("{heading}{tail}")));
            }
        }
    }
    fn line_start(&self, at: usize) -> usize { self.source[..at].rfind('\n').map_or(0, |i| i + 1) }
    /// Past this line's end and any blank lines (`>` counts as blank in a quote).
    fn skip_blank_lines(&self, at: usize) -> usize {
        let Some(nl) = self.source[at..].find('\n') else { return self.source.len() };
        if !self.source[at..at + nl].trim().is_empty() { return at; }
        let mut end = at + nl + 1;
        while let Some(nl) = self.source[end..].find('\n') {
            if self.source[end..end + nl].chars().all(|c| matches!(c, ' ' | '\t' | '>' | '\r')) { end += nl + 1; } else { break; }
        }
        end
    }
    fn back_over_blank(&self, at: usize) -> usize {
        let s = &self.source[..at];
        s.trim_end_matches([' ', '\t', '\r', '\n', '>']).len()
    }
    fn diff_siblings(&self, old: &[Block], new: &[Block], prefix: &str, empty_at: usize, edits: &mut Vec<Edit>) {
        let position: HashMap<BlockId, usize> = old.iter().enumerate().filter(|(_, b)| self.spans.contains_key(&b.id)).map(|(i, b)| (b.id, i)).collect();
        let candidates: Vec<usize> = new.iter().filter_map(|b| position.get(&b.id).copied()).collect();
        let kept_old: HashSet<usize> = lis(&candidates).into_iter().map(|i| candidates[i]).collect();
        // Deletions: whole runs, so adjacent removals never overlap.
        let mut i = 0;
        while i < old.len() {
            if kept_old.contains(&i) || !self.spans.contains_key(&old[i].id) { i += 1; continue; }
            let start = i;
            while i < old.len() && !kept_old.contains(&i) && self.spans.contains_key(&old[i].id) { i += 1; }
            let first = &self.spans[&old[start].id]; let last = &self.spans[&old[i - 1].id];
            let from = self.line_start(first.outer.start);
            let has_next = old[i..].iter().any(|b| self.spans.contains_key(&b.id));
            let has_prev = old[..start].iter().any(|b| self.spans.contains_key(&b.id));
            let range = if has_next {
                let limit = old[i..].iter().find_map(|b| self.spans.get(&b.id)).map_or(self.source.len(), |s| self.line_start(s.outer.start));
                from..self.skip_blank_lines(last.outer.end).min(limit)
            } else if has_prev { self.back_over_blank(from).max(old[..start].iter().rev().find_map(|b| self.spans.get(&b.id)).map_or(0, |s| s.outer.end))..last.outer.end }
            else { first.outer.start..last.outer.end };
            edits.push((range, String::new()));
        }
        // Insertions and patches, in new order.
        let mut pending: Vec<&Block> = vec![];
        let mut prev_kept: Option<&Block> = None;
        for b in new {
            let keep = position.get(&b.id).is_some_and(|i| kept_old.contains(i));
            if !keep { pending.push(b); continue; }
            self.insert_run(&pending, prev_kept, Some(b), prefix, empty_at, edits); pending.clear();
            self.patch_block(&old[position[&b.id]], b, edits);
            if let Some(a) = prev_kept { self.separate(old, &position, a, b, prefix, edits); }
            prev_kept = Some(b);
        }
        self.insert_run(&pending, prev_kept, None, prefix, empty_at, edits);
    }
    /// A list item turned paragraph must not read as a lazy continuation of
    /// its tight neighbours, nor they of it.
    fn separate(&self, old: &[Block], position: &HashMap<BlockId, usize>, a: &Block, b: &Block, prefix: &str, edits: &mut Vec<Edit>) {
        let changed = |x: &Block| !same_kind(&old[position[&x.id]].kind, &x.kind);
        if (a.kind.is_list() && b.kind.is_list()) || !(changed(a) || changed(b)) { return; }
        let (Some(x), Some(y)) = (self.spans.get(&a.id), self.spans.get(&b.id)) else { return };
        let at = self.line_start(y.outer.start);
        if at <= x.outer.end || self.source[x.outer.end..at].matches('\n').count() >= 2 { return; }
        edits.push((at..at, format!("{}{}", prefix.trim_end(), self.eol)));
    }
    fn insert_run(&self, run: &[&Block], prev: Option<&Block>, next: Option<&Block>, prefix: &str, empty_at: usize, edits: &mut Vec<Edit>) {
        let blocks: Vec<&Block> = run.iter().copied().filter(|b| !self.is_blank(b)).collect();
        let Some(first) = blocks.first() else { return };
        let body = self.join(&blocks, prefix);
        let gap = |a: &BlockKind, b: &BlockKind| if a.is_list() && b.is_list() { self.eol.to_string() } else { format!("{0}{1}{0}", self.eol, prefix.trim_end()) };
        if let Some(p) = prev.and_then(|p| self.spans.get(&p.id).map(|s| (p, s))) {
            edits.push((p.1.outer.end..p.1.outer.end, format!("{}{prefix}{body}", gap(&p.0.kind, &first.kind))));
        } else if let Some(n) = next.and_then(|n| self.spans.get(&n.id).map(|s| (n, s))) {
            let at = self.line_start(n.1.outer.start);
            edits.push((at..at, format!("{prefix}{body}{}", gap(&blocks[blocks.len() - 1].kind, &n.0.kind))));
        } else {
            let at = empty_at;
            let before = &self.source[..at];
            let lead = if before.is_empty() || (before.ends_with('\n') && prefix.is_empty()) { String::new() } else { format!("{0}{1}{0}{prefix}", self.eol, prefix.trim_end()) };
            let tail = if at == self.source.len() && at == 0 { self.eol } else { "" };
            edits.push((at..at, format!("{lead}{body}{tail}")));
        }
    }
    fn is_blank(&self, b: &Block) -> bool { matches!(b.kind, BlockKind::Text) && b.text.text.is_empty() && b.children.is_empty() }
    fn patch_block(&self, old: &Block, new: &Block, edits: &mut Vec<Edit>) {
        let span = &self.spans[&old.id];
        let regenerate = |edits: &mut Vec<Edit>| edits.push((span.outer.clone(), self.generate(new, &span.child_prefix, None)));
        match &span.shape {
            Shape::Fence { info, body } => {
                let BlockKind::Code { lang, .. } = &new.kind else { return regenerate(edits) };
                if !same_kind(&old.kind, &new.kind) { edits.push((info.clone(), crate::lang::to_token(lang))); }
                if old.text.text != new.text.text {
                    let lines = self.with_prefix(&new.text.text, &span.child_prefix);
                    // A body that was empty has no line of its own yet.
                    let text = if body.is_empty() { format!("{lines}{}", self.eol) } else { lines };
                    edits.push((body.clone(), text));
                }
            }
            Shape::Opaque => {
                if !matches!(new.kind, BlockKind::Code { .. }) { return regenerate(edits); }
                if old.text.text != new.text.text { edits.push((span.outer.clone(), self.with_prefix(&new.text.text, &span.child_prefix))); }
            }
            Shape::Image | Shape::Math | Shape::Divider => { if old.kind != new.kind || old.text != new.text { regenerate(edits); } }
            Shape::Quote => {
                if std::mem::discriminant(&old.kind) != std::mem::discriminant(&new.kind) { return regenerate(edits); }
                self.diff_siblings(&old.children, &new.children, &span.child_prefix, span.outer.end, edits);
            }
            Shape::Table => self.patch_table(old, new, span, edits),
            Shape::Para | Shape::Heading | Shape::Item | Shape::Cell => {
                if !head_like(&new.kind) { return regenerate(edits); }
                if let (BlockKind::Todo { done: a }, BlockKind::Todo { done: b }, Some(m)) = (&old.kind, &new.kind, &span.marker) && a != b { edits.push((m.clone(), if *b { "[x]" } else { "[ ]" }.into())); }
                let ordered = matches!((&old.kind, &new.kind), (BlockKind::Ordered { .. }, BlockKind::Ordered { .. }));
                if !same_kind(&old.kind, &new.kind) && !ordered {
                    let at = span.head.start;
                    edits.push((at..span.content.end.max(at), self.head_line(new, None, &span.child_prefix)));
                } else if old.text != new.text { self.patch_text(span, &old.text, &new.text, edits); }
                self.diff_siblings(&old.children, &new.children, &span.child_prefix, span.outer.end, edits);
            }
        }
    }
    fn patch_table(&self, old: &Block, new: &Block, span: &Span, edits: &mut Vec<Edit>) {
        let (BlockKind::Table(a), BlockKind::Table(b)) = (&old.kind, &new.kind) else { return edits.push((span.outer.clone(), self.generate(new, &span.child_prefix, None))) };
        let same_shape = a.rows.len() == b.rows.len() && a.rows.iter().zip(&b.rows).all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.blocks.len() == 1 && q.blocks.len() == 1 && p.blocks[0].id == q.blocks[0].id && self.spans.contains_key(&p.blocks[0].id)));
        if !same_shape { return edits.push((span.outer.clone(), self.generate(new, &span.child_prefix, None))); }
        for (x, y) in a.rows.iter().zip(&b.rows) { for (p, q) in x.iter().zip(y) {
            let (p, q) = (&p.blocks[0], &q.blocks[0]);
            if p.text != q.text { self.patch_text(&self.spans[&p.id], &p.text, &q.text, edits); }
        } }
    }
    /// The smallest source change for a text edit: only when that cannot be
    /// done safely does the block's whole inline content get rewritten.
    fn patch_text(&self, span: &Span, old: &RichText, new: &RichText, edits: &mut Vec<Edit>) {
        if let Some(edit) = self.minimal_patch(span, old, new) { edits.push(edit); return; }
        let cell = span.shape == Shape::Cell;
        let prefix = if cell { String::new() } else { prefixify(&self.source[self.line_start(span.content.start)..span.content.start]) };
        let mut text = self.inline_md(new, &prefix);
        if cell { text = text.replace('|', "\\|").replace(self.eol, "<br>"); }
        if matches!(span.shape, Shape::Para) { text = escape_line_start(&text); }
        // `-` alone, or `#` alone, has no space before its content yet.
        let needs_space = span.content.is_empty() && !cell && !text.is_empty() && !self.source[..span.content.start].ends_with([' ', '\t', '\n']);
        edits.push((span.content.clone(), if needs_space { format!(" {text}") } else { text }));
    }
    fn minimal_patch(&self, span: &Span, old: &RichText, new: &RichText) -> Option<Edit> {
        if span.segs.is_empty() { return None; }
        // Common prefix and suffix, where both the text and the marks agree.
        let (a, b) = (old.text.as_str(), new.text.as_str());
        let mut p = 0;
        for (x, y) in a.chars().zip(b.chars()) { if x != y || old.marks_at(p + x.len_utf8()) != new.marks_at(p + x.len_utf8()) { break; } p += x.len_utf8(); }
        let mut s = 0;
        for (x, y) in a[p..].chars().rev().zip(b[p..].chars().rev()) { if x != y || old.marks_at(a.len() - s) != new.marks_at(b.len() - s) { break; } s += x.len_utf8(); }
        let (old_mid, new_mid) = (p..a.len() - s, p..b.len() - s);
        let inserted = &b[new_mid.clone()];
        if inserted.contains(['\n', '\u{fffc}']) { return None; }
        let wanted = (!inserted.is_empty()).then(|| new.marks_at(new_mid.start + inserted.chars().next().map_or(0, char::len_utf8)));
        if wanted.as_ref().is_some_and(|m| m.inline.is_some()) { return None; }
        // Mixed marks in what was typed cannot go into one source run.
        if new.spans().filter(|(r, _)| r.start < new_mid.end && new_mid.start < r.end).nth(1).is_some() { return None; }
        let (from, to) = if old_mid.is_empty() {
            // An insertion: inside the piece carrying the marks it should have.
            let wanted = wanted.as_ref()?;
            let seg = span.segs.iter().find(|g| g.text.start <= p && p <= g.text.end && g.marks == *wanted && (g.literal || p == g.text.start || p == g.text.end));
            let at = match seg {
                Some(seg) => if p == seg.text.end { seg.src.end } else if p == seg.text.start { seg.src.start } else { seg.src.start + p - seg.text.start },
                // Plain text at either end of the block sits outside all markup.
                None if *wanted == Marks::default() && p == 0 => span.content.start,
                None if *wanted == Marks::default() && p == a.len() => span.content.end,
                None => return None,
            };
            (at, at)
        } else {
            let first = span.segs.iter().position(|g| g.text.start <= old_mid.start && old_mid.start < g.text.end)?;
            let last = span.segs.iter().position(|g| g.text.start < old_mid.end && old_mid.end <= g.text.end)?;
            let (f, l) = (&span.segs[first], &span.segs[last]);
            // Across markup, or emptying a marked run, needs a rewrite.
            if span.segs[first..=last].iter().any(|g| g.marks != f.marks || g.marks.inline.is_some()) { return None; }
            if wanted.as_ref().is_some_and(|m| *m != f.marks) { return None; }
            if f.marks != Marks::default() && old_mid.start <= f.text.start && old_mid.end >= l.text.end && inserted.is_empty() { return None; }
            let map = |g: &Seg, at: usize| if at == g.text.start { Some(g.src.start) } else if at == g.text.end { Some(g.src.end) } else if g.literal { Some(g.src.start + at - g.text.start) } else { None };
            (map(f, old_mid.start)?, map(l, old_mid.end)?)
        };
        // Leading space that would turn a paragraph into a code block.
        if span.shape == Shape::Para && (b.starts_with("    ") || b.starts_with('\t')) { return None; }
        // `word_it_` is not emphasis: `_` only opens at a word boundary.
        let wordy = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
        let left = inserted.chars().last().or_else(|| self.source[..from].chars().last());
        let right = inserted.chars().next().or_else(|| self.source[to..].chars().next());
        if (self.source[to..].starts_with('_') && wordy(left)) || (self.source[..from].ends_with('_') && wordy(right)) { return None; }
        // `**a **` is not emphasis: a run edged with space needs a rewrite.
        let runs: Vec<_> = new.spans().collect();
        let covers = |o: &Marks, m: &Marks| (!m.bold || o.bold) && (!m.italic || o.italic) && (!m.strike || o.strike);
        let edged = runs.iter().enumerate().any(|(i, (r, m))| (m.bold || m.italic || m.strike) && r.start <= new_mid.end && p <= r.end && {
            let t = &b[r.clone()];
            (t.starts_with(char::is_whitespace) && !i.checked_sub(1).is_some_and(|j| covers(runs[j].1, m))) || (t.ends_with(char::is_whitespace) && !runs.get(i + 1).is_some_and(|n| covers(n.1, m)))
        });
        if edged { return None; }
        let mut text = escape_inline(inserted, self.mdx);
        if span.shape == Shape::Cell { text = text.replace('|', "\\|"); }
        if span.shape == Shape::Para && p == 0 { text = escape_line_start(&text); }
        Some((from..to, text))
    }
    fn with_prefix(&self, text: &str, prefix: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for (i, line) in text.split('\n').enumerate() { if i > 0 { out.push_str(self.eol); if !line.is_empty() { out.push_str(prefix); } else { out.push_str(prefix.trim_end()); } } out.push_str(line); }
        out
    }
    fn join(&self, blocks: &[&Block], prefix: &str) -> String {
        let mut out = String::new(); let mut prev: Option<&Block> = None; let mut seq = 0u32;
        for b in blocks {
            match b.kind { BlockKind::Ordered { seq: Some(n) } => seq = n, BlockKind::Ordered { seq: None } => seq += 1, _ => seq = 0 }
            let text = self.generate(b, prefix, Some(seq));
            if text.is_empty() { continue; }
            if let Some(p) = prev {
                if p.kind.is_list() && b.kind.is_list() { out.push_str(self.eol); out.push_str(prefix); } else { out.push_str(self.eol); out.push_str(prefix.trim_end()); out.push_str(self.eol); out.push_str(prefix); }
            }
            out.push_str(&text); prev = Some(b);
        }
        out
    }
    fn head_line(&self, b: &Block, seq: Option<u32>, prefix: &str) -> String {
        let unchanged = self.spans.get(&b.id).filter(|s| !s.content.is_empty() && matches!(s.shape, Shape::Para | Shape::Heading | Shape::Item) && self.original(b.id).is_some_and(|o| o.text == b.text));
        let text = unchanged.map_or_else(|| self.inline_md(&b.text, prefix), |s| self.source[s.content.clone()].to_string());
        match &b.kind {
            BlockKind::Heading { level, .. } => format!("{} {text}", "#".repeat((*level).clamp(1, 6) as usize)),
            BlockKind::Bullet => format!("- {text}"),
            BlockKind::Ordered { seq: n } => format!("{}. {text}", n.or(seq).unwrap_or(1).max(1)),
            BlockKind::Todo { done } => format!("- [{}] {text}", if *done { "x" } else { " " }),
            _ if unchanged.is_some() => text,
            _ => escape_line_start(&text),
        }
    }
    fn original(&self, id: BlockId) -> Option<&Block> {
        fn find(blocks: &[Block], id: BlockId) -> Option<&Block> { for b in blocks { if b.id == id { return Some(b); } else if let Some(x) = find(&b.children, id) { return Some(x); } } None }
        find(&self.doc.blocks, id)
    }
    /// Markdown for a block with no usable source. `prefix` starts every line after the first.
    fn generate(&self, b: &Block, prefix: &str, seq: Option<u32>) -> String {
        match &b.kind {
            BlockKind::Code { lang, .. } => {
                let token = if lang == "Plain Text" { String::new() } else { crate::lang::to_token(lang) };
                let fence = if b.text.text.contains("```") { "````" } else { "```" };
                format!("{fence}{token}{0}{prefix}{1}{0}{prefix}{fence}", self.eol, self.with_prefix(&b.text.text, prefix))
            }
            BlockKind::Divider => "---".into(),
            BlockKind::Quote | BlockKind::Callout { .. } => {
                let inner = format!("{prefix}> ");
                let blocks: Vec<&Block> = b.children.iter().filter(|c| !self.is_blank(c)).collect();
                let body = self.join(&blocks, &inner);
                if let BlockKind::Callout { emoji, .. } = &b.kind {
                    let name = ALERTS.iter().find(|(_, e, _)| e == emoji).map_or("NOTE", |(n, _, _)| n);
                    return if body.is_empty() { format!("> [!{name}]") } else { format!("> [!{name}]{}{inner}{body}", self.eol) };
                }
                format!("> {body}")
            }
            BlockKind::Image { src, caption, .. } => match self.spans.get(&b.id).filter(|_| self.original(b.id).is_some_and(|o| o.kind == b.kind)) {
                Some(s) => self.source[s.outer.clone()].to_string(),
                None => format!("![{}]({})", escape_inline(caption.as_deref().unwrap_or(""), self.mdx), self.relative(src).replace(' ', "%20")),
            },
            BlockKind::Table(t) => {
                let cols = t.ncols().max(1);
                let row = |r: &Vec<TableCell>| {
                    let mut cells: Vec<String> = r.iter().map(|c| c.blocks.iter().map(|x| self.inline_md(&x.text, "").replace('|', "\\|").replace(self.eol, "<br>")).collect::<Vec<_>>().join("<br>")).collect();
                    cells.resize(cols, String::new());
                    format!("| {} |", cells.join(" | "))
                };
                let mut lines = vec![t.rows.first().map_or_else(|| row(&vec![]), row), format!("|{}", " --- |".repeat(cols))];
                lines.extend(t.rows.iter().skip(1).map(row));
                lines.join(&format!("{}{prefix}", self.eol))
            }
            BlockKind::Grid | BlockKind::Column { .. } => { let blocks: Vec<&Block> = b.children.iter().collect(); self.join(&blocks, prefix) }
            BlockKind::Countdown(_) | BlockKind::SubPageList | BlockKind::File(_) | BlockKind::Raw { .. } => String::new(),
            _ => {
                let width = match &b.kind { BlockKind::Bullet => 2, BlockKind::Todo { .. } => 2, BlockKind::Ordered { .. } => 3, _ => 0 };
                let head = self.head_line(b, seq, prefix);
                if b.children.is_empty() { return head; }
                let inner = format!("{prefix}{}", " ".repeat(width));
                let blocks: Vec<&Block> = b.children.iter().filter(|c| !self.is_blank(c)).collect();
                if blocks.is_empty() { return head; }
                let tight = b.kind.is_list() && blocks[0].kind.is_list();
                let sep = if tight { format!("{}{inner}", self.eol) } else { format!("{0}{1}{0}{inner}", self.eol, inner.trim_end()) };
                format!("{head}{sep}{}", self.join(&blocks, &inner))
            }
        }
    }
    fn relative(&self, src: &str) -> String {
        let path = Path::new(src);
        if !path.is_absolute() || src.contains("://") { return src.to_string(); }
        let Ok(inside) = path.strip_prefix(&self.root) else { return src.to_string() };
        let dir = self.file.parent().and_then(|d| d.strip_prefix(&self.root).ok()).unwrap_or(Path::new(""));
        let ups = dir.components().count();
        let mut rel = "../".repeat(ups);
        rel.push_str(&inside.to_string_lossy());
        rel
    }
    fn inline_md(&self, t: &RichText, prefix: &str) -> String {
        let mut out = String::new();
        // Group runs that only differ in marks Markdown has no syntax for.
        type Key = (bool, bool, bool, bool, Option<String>);
        let mut groups: Vec<(Key, String)> = vec![];
        for (r, m) in t.spans() {
            let key = (m.bold, m.italic, m.strike, m.code, m.link.clone());
            let s = match m.inline.as_ref() {
                Some(o) => {
                    let piece = match &o.kind {
                        InlineKind::Equation(src) => format!("$${src}$$"),
                        InlineKind::File(f) => escape_inline(&f.name, self.mdx),
                        InlineKind::DocLink(l) => escape_inline(&l.title, self.mdx),
                        _ => crate::docxml::raw_summary(&o.xml).0,
                    };
                    t.text[r].chars().map(|_| piece.as_str()).collect::<String>()
                }
                None if m.code => t.text[r].to_string(),
                None => escape_inline(&t.text[r], self.mdx),
            };
            match groups.last_mut() { Some((k, text)) if *k == key => text.push_str(&s), _ => groups.push((key, s)) }
        }
        for ((bold, italic, strike, code, link), s) in groups {
            let body = s.trim();
            if body.is_empty() { out.push_str(&s); continue; }
            let (lead, trail) = (&s[..s.len() - s.trim_start().len()], &s[s.trim_end().len()..]);
            let mut x = if code && self.raws.contains(body) { body.to_string() } else if code { let tick = if body.contains('`') { "``" } else { "`" }; format!("{tick}{body}{tick}") } else { body.to_string() };
            if strike { x = format!("~~{x}~~"); }
            if italic { x = format!("*{x}*"); }
            if bold { x = format!("**{x}**"); }
            if let Some(url) = link.filter(|u| !self.code_links.contains(u)) { x = format!("[{x}]({})", url.replace(' ', "%20").replace(')', "%29")); }
            out.push_str(lead); out.push_str(&x); out.push_str(trail);
        }
        out.replace('\n', &format!("  {}{prefix}", self.eol))
    }
}
/// Only what would otherwise change the meaning: `snake_case` stays readable.
fn escape_inline(s: &str, mdx: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let before = i.checked_sub(1).and_then(|j| chars.get(j)).copied();
        let after = chars.get(i + 1).copied();
        let esc = match c {
            '\\' | '`' | '[' | ']' => true,
            '*' => after.is_some_and(|a| !a.is_whitespace()) || before.is_some_and(|b| !b.is_whitespace()),
            '_' => !(before.is_some_and(char::is_alphanumeric) && after.is_some_and(char::is_alphanumeric)),
            '~' => after == Some('~') || before == Some('~'),
            '<' => mdx || after.is_some_and(|a| a.is_ascii_alphabetic() || matches!(a, '/' | '!' | '?')),
            '{' | '}' => mdx,
            '&' => { let rest: String = chars[i + 1..].iter().take(12).collect(); rest.split_once(';').is_some_and(|(a, _)| !a.is_empty() && a.chars().all(|x| x.is_alphanumeric() || x == '#')) }
            '$' => after == Some('$') || before == Some('$'),
            _ => false,
        };
        if esc { out.push('\\'); }
        out.push(c);
    }
    out
}
/// A paragraph that would otherwise read as a heading, list or quote.
fn escape_line_start(s: &str) -> String {
    let t = s.trim_start();
    // Four spaces would make it a code block; Markdown ignores leading space anyway.
    let s = if s.starts_with('\t') || s.len() - t.len() >= 4 { t } else { s };
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    let list_number = digits > 0 && digits < 10 && t[digits..].starts_with(['.', ')']) && (t.len() == digits + 1 || t[digits + 1..].starts_with(' '));
    let marker = t.starts_with('>') || t.starts_with('#') && t.trim_start_matches('#').starts_with(' ') || (t.starts_with(['-', '+', '*']) && t[1..].starts_with(' ')) || t.starts_with("---") || t.starts_with("===");
    if marker { format!("\\{s}") } else if list_number { let at = s.len() - t.len() + digits; format!("{}\\{}", &s[..at], &s[at..]) } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::core::{Core, EditKind, Pos, Selection};
    fn at() -> (PathBuf, PathBuf) { (PathBuf::from("/nonexistent/root"), PathBuf::from("/nonexistent/root/a.md")) }
    fn p(s: &str) -> Projection { let (r, f) = at(); project(s, &f, &r) }
    fn mdx(s: &str) -> Projection { let (r, f) = at(); project(s, &f.with_extension("mdx"), &r) }
    /// Edit through the real Core, as the editor does.
    fn edit(pr: &Projection, f: impl FnOnce(&mut Core)) -> String { let mut c = Core::new(pr.doc.clone()); f(&mut c); pr.render(&c.doc) }
    fn caret(c: &mut Core, i: usize, offset: usize) { let id = c.doc.blocks[i].id; c.sel = Selection::caret(Pos::new(id, offset)); }
    /// Everything visible, for comparing a document with its re-parse.
    fn visible(d: &Document) -> Vec<String> {
        fn walk(bs: &[Block], out: &mut Vec<String>) { for b in bs {
            if let BlockKind::Table(t) = &b.kind { for r in &t.rows { for c in r { walk(&c.blocks, out); } } }
            if !b.text.text.trim().is_empty() { out.push(b.text.text.trim().to_string()); }
            walk(&b.children, out);
        } }
        let mut out = vec![d.title.text.clone()]; walk(&d.blocks, &mut out); out
    }

    const MESSY: &str = "\u{feff}---\r\ntitle: \"Hi\"\r\ntags:\r\n  - a\r\n---\r\n\r\n# Title   #\r\n\r\nSome *emph* __bold__ and `code` and [link][ref].\r\nSecond  line\\_x &amp; more\r\n\r\n* star\r\n* [X] task\r\n   1) nested\r\n\r\n<!-- hidden -->\r\n\r\n> quote\r\n> more\r\n\r\n~~~rust\r\nfn a() {}\r\n~~~\r\n\r\n| a | b |\r\n|:--|--:|\r\n| 1 |   |\r\n\r\n$$\r\nx^2\r\n$$\r\n\r\n***\r\n\r\n[ref]: https://example.com \"t\"\r\n\r\n\r\n";
    #[test]
    fn untouched_source_is_byte_identical() {
        for source in [MESSY, "", "\n\n", "no newline", "# only", "- a\n- b", "```\nopen fence", "---\na: 1\n---\n", "a\r\nb\r\n", "> \n", "- \n", "#\n"] {
            let pr = p(source);
            assert_eq!(pr.render(&pr.doc), source, "{source:?}");
            let mut core = Core::new(pr.doc.clone()); core.fix_selection();
            assert_eq!(pr.render(&core.doc), source, "{source:?}");
        }
    }
    #[test]
    fn frontmatter_is_metadata_and_h1_is_the_title() {
        let pr = p(MESSY);
        assert_eq!(pr.doc.title.text, "Title");
        assert_eq!(pr.meta.fields[0], ("title".to_string(), "Hi".to_string()));
        assert_eq!(pr.meta.fields[1].1, "a");
        assert!(!visible(&pr.doc).iter().any(|t| t.contains("tags")));
        assert!(!visible(&pr.doc).iter().any(|t| t.contains("hidden")));
        assert_eq!(pr.doc.blocks[0].text.text, "Some emph bold and code and link. Second  line_x & more");
        let stem = p("just text\n"); assert_eq!(stem.doc.title.text, "a");
        let named = p("---\ntitle: Named\n---\nbody\n"); assert_eq!(named.doc.title.text, "Named");
    }
    #[test]
    fn typing_changes_only_the_typed_bytes() {
        let source = "# T\n\nkeep __this__ and *that*\n\nedit me\n\n[x]: http://y\n";
        let pr = p(source);
        let out = edit(&pr, |c| { caret(c, 1, 4); c.insert_text("ED"); });
        assert_eq!(out, source.replace("edit me", "editED me"));
        // Inside a bold run: the run's own markers stay as written.
        let out = edit(&pr, |c| { caret(c, 0, 6); c.insert_text("Z"); });
        assert_eq!(out, source.replace("__this__", "__tZhis__"));
        // After a marked run: outside its markers.
        let source = "a *b* c\n"; let pr = p(source);
        let out = edit(&pr, |c| { caret(c, 0, 3); c.insert_text("Q"); });
        assert_eq!(p(&out).doc.blocks[0].text.text, "a bQ c"); assert!(out.contains("*b"));
    }
    #[test]
    fn undo_all_returns_the_original_bytes() {
        let pr = p(MESSY);
        let mut c = Core::new(pr.doc.clone());
        caret(&mut c, 0, 2); c.insert_text("x"); c.enter(); c.insert_text("new para"); c.toggle_todo(c.doc.blocks.iter().find(|b| matches!(b.kind, BlockKind::Todo { .. })).unwrap().id);
        assert_ne!(pr.render(&c.doc), MESSY);
        while c.undo() {}
        assert_eq!(pr.render(&c.doc), MESSY);
    }
    #[test]
    fn task_toggle_touches_only_its_marker() {
        let source = "- parent\n  - [ ] same\n* [X] same\n\n1. [ ] third\n";
        let pr = p(source);
        let todo = pr.doc.blocks[0].children[0].id;
        assert_eq!(edit(&pr, |c| c.toggle_todo(todo)), "- parent\n  - [x] same\n* [X] same\n\n1. [ ] third\n");
        let top = pr.doc.blocks[1].id;
        assert_eq!(edit(&pr, |c| c.toggle_todo(top)), "- parent\n  - [ ] same\n* [ ] same\n\n1. [ ] third\n");
    }
    #[test]
    fn new_and_removed_blocks_keep_their_neighbours() {
        let source = "one\n\n<!-- keep -->\n\ntwo\n\n[a]: http://b\n\nthree\n";
        let pr = p(source);
        let two = pr.doc.blocks[1].id;
        assert_eq!(edit(&pr, |c| c.delete_block(two)), "one\n\n<!-- keep -->\n\n[a]: http://b\n\nthree\n");
        let three = pr.doc.blocks[2].id;
        assert_eq!(edit(&pr, |c| c.delete_block(three)), "one\n\n<!-- keep -->\n\ntwo\n\n[a]: http://b\n");
        let out = edit(&pr, |c| { caret(c, 2, 5); c.enter(); c.insert_text("four"); });
        assert_eq!(out, "one\n\n<!-- keep -->\n\ntwo\n\n[a]: http://b\n\nthree\n\nfour\n");
        let source = "- a\n- b\n- c\n"; let pr = p(source);
        let b = pr.doc.blocks[1].id;
        assert_eq!(edit(&pr, |c| c.delete_block(b)), "- a\n- c\n");
        let c_id = pr.doc.blocks[2].id;
        assert_eq!(edit(&pr, |c| c.delete_block(c_id)), "- a\n- b\n");
        let out = edit(&pr, |c| { caret(c, 0, 1); c.enter(); c.insert_text("x"); });
        assert_eq!(out, "- a\n- x\n- b\n- c\n");
    }
    #[test]
    fn nested_edits_keep_indentation_and_siblings() {
        let source = "* top\n    * deep *one*\n    * deep two\n\n> q1\n>\n> q2\n";
        let pr = p(source);
        let deep = pr.doc.blocks[0].children[1].id;
        let out = edit(&pr, |c| { c.sel = Selection::caret(Pos::new(deep, 8)); c.insert_text("!"); });
        assert_eq!(out, source.replace("deep two", "deep two!"));
        let q2 = pr.doc.blocks[1].children[1].id;
        let out = edit(&pr, |c| { c.sel = Selection::caret(Pos::new(q2, 2)); c.enter(); c.insert_text("q3"); });
        assert_eq!(out, "* top\n    * deep *one*\n    * deep two\n\n> q1\n>\n> q2\n>\n> q3\n");
    }
    #[test]
    fn crlf_is_kept_for_new_lines() {
        let source = "a\r\n\r\nb\r\n"; let pr = p(source);
        let out = edit(&pr, |c| { caret(c, 0, 1); c.enter(); c.insert_text("x"); });
        assert_eq!(out, "a\r\n\r\nx\r\n\r\nb\r\n");
    }
    #[test]
    fn mdx_constructs_survive_edits_elsewhere() {
        let source = "---\na: 1\n---\nimport Widget from './w'\n\n# Hello\n\n<Widget value={2}>\n  child\n</Widget>\n\nsome {1 + 1} text with <Inline /> here\n\nlast\n";
        let pr = mdx(source);
        assert_eq!(pr.render(&pr.doc), source);
        assert_eq!(pr.meta.imports, vec!["import Widget from './w'"]);
        assert!(pr.diagnostic.as_deref().unwrap().contains("does not run"));
        assert!(matches!(&pr.doc.blocks[0].kind, BlockKind::Code { caption: Some(c), .. } if c.contains("not executed")));
        let out = edit(&pr, |c| { caret(c, 2, 4); c.insert_text("!"); });
        assert_eq!(out, source.replace("last", "last!"));
        let out = edit(&pr, |c| { caret(c, 1, 4); c.insert_text("X"); });
        assert_eq!(out, source.replace("some {1", "someX {1"));
        // Rewriting a paragraph still writes inline objects back verbatim.
        let id = pr.doc.blocks[1].id;
        let out = edit(&pr, |c| { c.sel = Selection { anchor: Pos::new(id, 0), head: Pos::new(id, 2) }; c.apply_marks(|m| m.bold = true); });
        assert!(out.contains("{1 + 1}") && out.contains("<Inline />") && !out.contains("`{"), "{out}");
        // Inline source is shown, and editing it is a plain patch.
        assert!(pr.doc.blocks[1].text.text.contains("{1 + 1}") && pr.doc.blocks[1].text.text.contains("<Inline />"));
        let out = edit(&pr, |c| { let at = c.doc.blocks[1].text.text.find("1 + 1").unwrap(); caret(c, 1, at + 4); c.insert_text("0"); });
        assert_eq!(out, source.replace("{1 + 1}", "{1 + 01}"));
        let broken = mdx("<Component value={\n\nreadable\n");
        assert!(broken.diagnostic.unwrap().contains("syntax error"));
        assert!(visible(&broken.doc).iter().any(|t| t.contains("readable")));
    }
    #[test]
    fn title_edits_map_to_the_heading() {
        let source = "---\na: 1\n---\n\n# Old *title*\n\nbody\n"; let pr = p(source);
        let out = edit(&pr, |c| { c.sel = Selection::caret(Pos::new(TITLE, 3)); c.insert_text("er"); });
        assert_eq!(out, source.replace("Old", "Older"));
        let source = "---\na: 1\n---\n\nbody\n"; let pr = p(source);
        let out = edit(&pr, |c| { c.checkpoint(EditKind::Other); c.doc.title = RichText::plain("Made"); });
        assert_eq!(p(&out).doc.title.text, "Made"); assert!(out.starts_with("---\na: 1\n---\n") && out.ends_with("body\n"));
    }
    #[test]
    fn code_fence_body_and_tables_patch_in_place() {
        let source = "```rust title=\"x\"\nfn a() {}\n```\n\n| a | b |\n|:--|--:|\n| 1 | *2* |\n";
        let pr = p(source);
        let out = edit(&pr, |c| { caret(c, 0, 3); c.insert_text("zz"); });
        assert_eq!(out, source.replace("fn a()", "fn zza()"));
        let cell = if let BlockKind::Table(t) = &pr.doc.blocks[1].kind { t.rows[1][0].blocks[0].id } else { panic!() };
        let out = edit(&pr, |c| { c.sel = Selection::caret(Pos::new(cell, 1)); c.insert_text("|9"); });
        assert_eq!(out, source.replace("| 1 |", "| 1\\|9 |"));
    }
    #[test]
    fn changing_a_code_block_language_writes_the_fence() {
        let source = "```\nflowchart LR\n```\n";
        let pr = p(source);
        let out = edit(&pr, |c| { c.checkpoint(EditKind::Other); if let BlockKind::Code { lang, .. } = &mut c.doc.blocks[0].kind { *lang = "Mermaid".into(); } });
        assert_eq!(out, "```mermaid\nflowchart LR\n```\n");
        assert!(matches!(&p(&out).doc.blocks[0].kind, BlockKind::Code { lang, .. } if lang == "Mermaid"));
    }
    #[test]
    fn generated_text_escapes_only_what_it_must() {
        let source = "a\n"; let pr = p(source);
        let out = edit(&pr, |c| { caret(c, 0, 1); c.insert_text(" snake_case 2*3 *x* <div> [y]"); });
        assert_eq!(out, "a snake_case 2\\*3 \\*x\\* \\<div> \\[y\\]\n");
        assert_eq!(visible(&p(&out).doc)[1], "a snake_case 2*3 *x* <div> [y]");
    }
    #[test]
    fn single_dollar_math_leaves_prices_and_variables_alone() {
        let d = |s: &str| dollar_math(s).into_iter().map(|r| s[r].to_string()).collect::<Vec<_>>();
        assert_eq!(d("area $x^2$ here and $y$."), ["$x^2$", "$y$"]);
        for prose in ["$5 and $10", "costs $5", "$HOME/$PATH", "$HOME and $PATH", "a$b$c", "\\\\$x", "$ x$", "$x $", "$$"] { assert!(d(prose).is_empty(), "{prose}"); }
        let source = "Energy $E=mc^2$ costs $5 and $10.\n";
        let pr = p(source);
        assert_eq!(pr.render(&pr.doc), source);
        assert!(pr.doc.blocks[0].text.spans().any(|(_, m)| m.inline.as_ref().and_then(Inline::equation) == Some("E=mc^2")));
        assert!(pr.doc.blocks[0].text.text.contains("costs $5 and $10."));
        let out = edit(&pr, |c| { let at = c.doc.blocks[0].text.len() - 1; caret(c, 0, at); c.insert_text("!"); });
        assert_eq!(out, source.replace("$10.", "$10!."));
    }
    #[test]
    fn github_alerts_are_callouts_and_keep_their_marker() {
        let source = "# T\n\n> [!WARNING]\n> Careful *here*.\n>\n> Second.\n\n> [!NOTE]\n>\n> Spaced.\n\n> plain quote\n\n> [!NOPE] not an alert\n";
        let pr = p(source);
        assert_eq!(pr.render(&pr.doc), source);
        assert!(matches!(&pr.doc.blocks[0].kind, BlockKind::Callout { emoji, .. } if emoji == "⚠️"));
        assert_eq!(pr.doc.blocks[0].children[0].text.text, "Careful here.");
        assert!(matches!(pr.doc.blocks[1].kind, BlockKind::Callout { .. }));
        assert!(matches!(pr.doc.blocks[2].kind, BlockKind::Quote));
        assert!(matches!(pr.doc.blocks[3].kind, BlockKind::Quote));
        let id = pr.doc.blocks[0].children[0].id;
        let out = edit(&pr, |c| { c.sel = Selection::caret(Pos::new(id, 7)); c.insert_text("ly"); });
        assert_eq!(out, source.replace("Careful *here*", "Carefully *here*"));
        let second = pr.doc.blocks[0].children[1].id;
        let out = edit(&pr, |c| { c.sel = Selection::caret(Pos::new(second, 7)); c.enter(); c.insert_text("Third."); });
        assert_eq!(out, source.replace("> Second.\n", "> Second.\n>\n> Third.\n"));
        // A callout made here is an alert when read back.
        let plain = p("a\n"); let mut c = Core::new(plain.doc.clone());
        let mut made = Block::new(callout("💡", "blue"), RichText::default()); made.children = vec![Block::text("hi")];
        c.doc.blocks.push(made);
        let out = plain.render(&c.doc);
        assert_eq!(out, "a\n\n> [!NOTE]\n> hi\n");
        assert!(matches!(p(&out).doc.blocks[1].kind, BlockKind::Callout { .. }));
    }
    #[test]
    fn code_spans_naming_files_link_without_changing_the_source() {
        let dir = std::env::temp_dir().join(format!("quiet-code-{}", uuid::Uuid::new_v4())); std::fs::create_dir_all(dir.join("people")).unwrap();
        let root = dir.canonicalize().unwrap();
        std::fs::write(root.join("people/a.md"), "x").unwrap();
        let source = "See `people/`, `people/a.md`, `nope.md`, `git status` and __bold__.\n";
        let pr = project(source, &root.join("r.md"), &root);
        let links: Vec<_> = pr.doc.blocks[0].text.spans().filter_map(|(r, m)| m.link.clone().map(|l| (pr.doc.blocks[0].text.text[r].to_string(), l))).collect();
        assert_eq!(links, [("people/".to_string(), "/people".to_string()), ("people/a.md".to_string(), "/people/a.md".to_string())]);
        assert_eq!(pr.render(&pr.doc), source);
        // Rewriting the paragraph must not bake the link into the file.
        let id = pr.doc.blocks[0].id;
        let out = edit(&pr, |c| { c.sel = Selection { anchor: Pos::new(id, 0), head: Pos::new(id, 3) }; c.apply_marks(|m| m.bold = true); });
        assert!(out.contains("`people/a.md`") && !out.contains("](/people"), "{out}");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bare_urls_link_in_markdown_and_mdx() {
        for pr in [p("see https://example.com/a now\n"), mdx("see https://example.com/a now\n")] {
            assert!(pr.doc.blocks[0].text.spans().any(|(_, m)| m.link.as_deref() == Some("https://example.com/a")), "{:?}", pr.doc.blocks[0].text);
            assert_eq!(pr.render(&pr.doc), "see https://example.com/a now\n");
        }
    }
    /// Random edits must leave a file that parses back to what the editor shows,
    /// and undoing them must restore every byte.
    #[test]
    fn random_edits_round_trip() {
        fn walk_ids(bs: &[Block], out: &mut Vec<BlockId>) { for b in bs { if b.kind.has_text() { out.push(b.id); } walk_ids(&b.children, out); } }
        let seeds: u64 = std::env::var("FDL_FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
        let corpus = [MESSY, "# T\n\n- a\n  - b\n\n  para in item\n- [ ] c\n\n> q\n\n1. x\n2. y\n", "plain\n\n**bold** _it_ ~~st~~ `co`\n\n| h |\n|---|\n| v |\n", "> [!TIP]\n> first\n>\n> second\n\nmath $x^2$ and $5\n"];
        for seed in 0..seeds {
            let mut r = seed.wrapping_mul(0x9E3779B97F4A7C15) | 1;
            let mut next = |n: usize| { r ^= r << 13; r ^= r >> 7; r ^= r << 17; (r as usize) % n.max(1) };
            let source = corpus[next(corpus.len())];
            let pr = p(source);
            let mut c = Core::new(pr.doc.clone());
            for _ in 0..1 + next(12) {
                let mut ids = vec![]; walk_ids(&c.doc.blocks, &mut ids);
                if ids.is_empty() { break; }
                let id = ids[next(ids.len())];
                if c.block(id).is_none() { continue; }
                let len = c.block(id).map_or(0, |b| b.text.len());
                let text = c.block(id).map(|b| b.text.text.clone()).unwrap_or_default();
                let mut off = next(len + 1); while !text.is_char_boundary(off) { off -= 1; }
                // Inline objects are atomic to the caret; stay off them.
                if text.contains('\u{fffc}') { off = 0; }
                c.sel = Selection::caret(Pos::new(id, off));
                c.fix_selection();
                match next(4) { 0 => c.insert_text("word"), 1 => c.insert_text(" "), 2 => c.backspace(), _ => c.enter() }
            }
            let out = pr.render(&c.doc);
            let again = p(&out);
            // An empty task or list item has no stable Markdown form until it has text.
            let normal = |d: &Document| visible(d).join(" ").replace("[ ]", "").replace("[X]", "").replace("[x]", "").split_whitespace().filter(|w| !(matches!(*w, "-" | "*" | "+") || w.strip_suffix(['.', ')']).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())))).collect::<Vec<_>>().join(" ");
            assert_eq!(normal(&again.doc), normal(&c.doc), "seed {seed}\n--- out {out:?}\n--- source {source:?}\n--- doc {:?}", c.doc.blocks.iter().map(|b| b.text.spans().map(|(r, m)| (b.text.text[r].to_string(), m.bold)).collect::<Vec<_>>()).collect::<Vec<_>>());
            while c.undo() {}
            assert_eq!(pr.render(&c.doc), source, "seed {seed}");
        }
    }
    /// Every Markdown file of a real folder: FDL_SOURCE_CORPUS=<dir>.
    #[test]
    #[ignore]
    fn corpus_round_trips() {
        let root = PathBuf::from(std::env::var("FDL_SOURCE_CORPUS").expect("set FDL_SOURCE_CORPUS"));
        let (mut stack, mut n) = (vec![root.clone()], 0);
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let path = e.path();
                if path.file_name().is_some_and(|s| s == ".git") { continue; }
                if path.is_dir() { stack.push(path); continue; }
                if !path.extension().is_some_and(|x| x == "md" || x == "mdx") { continue; }
                let Ok(source) = std::fs::read_to_string(&path) else { continue };
                let pr = project(&source, &path, &root);
                assert_eq!(pr.render(&pr.doc), source, "{}", path.display());
                let mut c = Core::new(pr.doc.clone());
                c.fix_selection();
                let id = c.doc.blocks.iter().find(|b| matches!(b.kind, BlockKind::Text) && !b.text.text.contains('\u{fffc}')).map(|b| b.id);
                if let Some(id) = id {
                    c.sel = Selection::caret(Pos::new(id, 0)); c.insert_text("zq");
                    let out = pr.render(&c.doc);
                    assert_eq!(out.len(), source.len() + 2, "{}", path.display());
                    while c.undo() {}
                    assert_eq!(pr.render(&c.doc), source);
                }
                n += 1;
            }
        }
        println!("round-tripped {n} files");
    }
    #[test]
    fn local_images_resolve_inside_the_folder_only() {
        let dir = std::env::temp_dir().join(format!("quiet-source-{}", uuid::Uuid::new_v4())); std::fs::create_dir(&dir).unwrap();
        let root = dir.canonicalize().unwrap();
        image::save_buffer(root.join("ok.png"), &[255, 0, 0, 255], 1, 1, image::ColorType::Rgba8).unwrap();
        let source = "![ok](ok.png)\n\n![gone](no.png)\n\n![far](../x.png)\n\ninline ![i](ok.png) text\n";
        let pr = project(source, &root.join("a.md"), &root);
        assert!(matches!(&pr.doc.blocks[0].kind, BlockKind::Image { src, .. } if src == root.join("ok.png").to_str().unwrap()));
        assert!(pr.doc.blocks[1].text.text.contains("no.png") && pr.doc.blocks[1].text.marks_at(1).code);
        assert_eq!(pr.render(&pr.doc), source);
        std::fs::remove_dir_all(root).unwrap();
    }
}
