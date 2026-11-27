//! Flowcharts (`graph` / `flowchart`): nodes in Mermaid's shapes, links of
//! every stroke, head and length, labels on both, subgraphs, `classDef` /
//! `class` / `:::` / `style` / `linkStyle`. Laid out by layout.rs.

use crate::layout::{self, Rect};
use crate::svg::{self, Anchor, Doc, Head, Paint, n};
use crate::text::{self, Block};
use crate::{Error, Line, Measure, Style, Svg, err};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir { TB, BT, LR, RL }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Rect, Round, Stadium, Subroutine, Cylinder, Circle, DoubleCircle, Asym, Rhombus, Hexagon, LeanR, LeanL, Trap, TrapAlt,
    // Mermaid 11's `@{ shape: … }` set
    Doc, Docs, LinDoc, TagDoc, Delay, HCyl, LinCyl, CurvTrap, DivRect, Tri, FlipTri, Fork, WinPane, Dot, FDot, FrDot,
    LinRect, NotchPent, SlRect, StRect, Flag, BowRect, CrossCirc, TagRect, NotchRect, Hourglass, Bolt, Text, BraceL, BraceR, Braces, Datastore,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stroke { Normal, Thick, Dotted, Invisible }

#[derive(Clone, Debug)]
pub struct Node {
    /// (as written; the parser looks nodes up by it)
    #[allow(dead_code)]
    pub id: String,
    pub label: String,
    pub shape: Shape,
    pub classes: Vec<String>,
    pub style: Vec<(String, String)>,
    /// The innermost subgraph it was mentioned in.
    pub sub: Option<usize>,
}

/// A link end: a node, or a subgraph by its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum End { Node(usize), Sub(usize) }

#[derive(Clone, Debug)]
pub struct Link {
    pub from: End,
    pub to: End,
    pub label: String,
    pub stroke: Stroke,
    pub start: Head,
    pub end: Head,
    pub len: usize,
    pub style: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Sub {
    pub id: String,
    pub title: String,
    pub parent: Option<usize>,
    pub classes: Vec<String>,
    pub style: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Flow {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub subs: Vec<Sub>,
    pub class_defs: HashMap<String, Vec<(String, String)>>,
    pub default_link: Vec<(String, String)>,
}

pub(crate) fn dir_of(s: &str) -> Option<Dir> {
    Some(match s { "TB" | "TD" | "v" => Dir::TB, "BT" | "^" => Dir::BT, "LR" | ">" => Dir::LR, "RL" | "<" => Dir::RL, _ => return None })
}

/// `fill:#f9f,stroke:#333,stroke-width:4px` as pairs (commas inside
/// `rgb(…)` stay).
pub(crate) fn styles(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0, 0);
    let s = s.trim().trim_end_matches(';');
    let mut push = |part: &str| {
        if let Some((k, v)) = part.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().trim_end_matches("!important").trim());
            if !k.is_empty() && !v.is_empty() { out.push((k, v.to_string())); }
        }
    };
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' | ';' if depth <= 0 => { push(&s[start..i]); start = i + 1; }
            _ => {}
        }
    }
    push(&s[start..]);
    out
}

/// A character that can be in a node id.
fn id_char(c: char) -> bool { c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '\'' | '`' | '+' | '*' | '/' | '\\' | '?' | '#' | '!') }

struct Cur<'a> {
    s: &'a str,
    at: usize,
    line: usize,
}

impl<'a> Cur<'a> {
    fn rest(&self) -> &'a str { &self.s[self.at..] }
    fn ws(&mut self) { let r = self.rest(); self.at += r.len() - r.trim_start().len(); }
    fn eat(&mut self, p: &str) -> bool { if self.rest().starts_with(p) { self.at += p.len(); true } else { false } }
    fn done(&self) -> bool { self.rest().trim().is_empty() }
    fn fail<T>(&self, what: &str) -> Result<T, Error> {
        let near: String = self.rest().chars().take(16).collect();
        if near.is_empty() { err(self.line, format!("{what} at the end of the line")) } else { err(self.line, format!("{what} near “{near}”")) }
    }

    /// A node id: letters, digits and a few marks; `-` only between them.
    fn id(&mut self) -> Option<&'a str> {
        let r = self.rest();
        let mut end = 0;
        let mut it = r.char_indices().peekable();
        while let Some((i, c)) = it.next() {
            let next = it.peek().map(|&(_, c)| c);
            let ok = id_char(c) || (c == '-' && end > 0 && next.is_some_and(|n| n.is_alphanumeric() || n == '_'));
            if !ok { break }
            // `o--` / `x--` after an id are link heads, not the id's end
            end = i + c.len_utf8();
        }
        if end == 0 { return None }
        self.at += end;
        Some(&r[..end])
    }

    /// Text up to `close` (or a quoted string followed by `close`).
    fn text_until(&mut self, close: &str) -> Result<String, Error> {
        let r = self.rest();
        let lead = r.len() - r.trim_start().len();
        if r[lead..].starts_with('"') {
            let body = &r[lead + 1..];
            if let Some(q) = body.find('"') {
                let after = &body[q + 1..];
                if after.trim_start().starts_with(close) {
                    self.at += lead + 1 + q + 1 + (after.len() - after.trim_start().len()) + close.len();
                    return Ok(text::label(&body[..q]));
                }
            }
        }
        match r.find(close) {
            Some(i) => { self.at += i + close.len(); Ok(text::label(&r[..i])) }
            None => self.fail(&format!("Missing “{close}”")),
        }
    }
}

const SHAPES: [(&str, &str, Shape); 14] = [
    ("(((", ")))", Shape::DoubleCircle), ("((", "))", Shape::Circle), ("([", "])", Shape::Stadium), ("[[", "]]", Shape::Subroutine),
    ("[(", ")]", Shape::Cylinder), ("{{", "}}", Shape::Hexagon), ("[/", "/]", Shape::LeanR), ("[\\", "\\]", Shape::LeanL),
    ("(", ")", Shape::Round), ("[", "]", Shape::Rect), ("{", "}", Shape::Rhombus), (">", "]", Shape::Asym),
    // (trapezoids are `[/ … \]` and `[\ … /]`, sorted out after the text)
    ("[/", "\\]", Shape::Trap), ("[\\", "/]", Shape::TrapAlt),
];

/// The shape named in `@{ shape: … }`.
fn shape_named(s: &str) -> Shape {
    match s {
        "rounded" | "event" => Shape::Round, "stadium" | "pill" | "terminal" => Shape::Stadium, "subroutine" | "subproc" | "fr-rect" | "framed-rectangle" => Shape::Subroutine,
        "cyl" | "cylinder" | "db" | "database" => Shape::Cylinder, "circle" | "circ" => Shape::Circle, "dbl-circ" | "double-circle" => Shape::DoubleCircle,
        "diam" | "diamond" | "decision" | "question" => Shape::Rhombus, "hex" | "hexagon" | "prepare" => Shape::Hexagon, "lean-r" | "lean-right" | "in-out" => Shape::LeanR, "lean-l" | "lean-left" | "out-in" => Shape::LeanL,
        "trap-b" | "trapezoid" | "priority" | "trapezoid-bottom" => Shape::Trap, "trap-t" | "inv-trapezoid" | "manual" | "trapezoid-top" => Shape::TrapAlt, "odd" => Shape::Asym,
        "doc" | "document" => Shape::Doc, "docs" | "documents" | "st-doc" | "stacked-document" => Shape::Docs, "lin-doc" | "lined-document" => Shape::LinDoc, "tag-doc" | "tagged-document" => Shape::TagDoc,
        "delay" | "half-rounded-rectangle" => Shape::Delay, "h-cyl" | "das" | "horizontal-cylinder" => Shape::HCyl, "lin-cyl" | "disk" | "lined-cylinder" => Shape::LinCyl,
        "curv-trap" | "curved-trapezoid" | "display" => Shape::CurvTrap, "div-rect" | "div-proc" | "divided-process" | "divided-rectangle" => Shape::DivRect,
        "tri" | "extract" | "triangle" => Shape::Tri, "flip-tri" | "flipped-triangle" | "manual-file" => Shape::FlipTri, "fork" | "join" => Shape::Fork,
        "win-pane" | "internal-storage" | "window-pane" => Shape::WinPane, "sm-circ" | "small-circle" | "start" => Shape::Dot, "f-circ" | "filled-circle" | "junction" => Shape::FDot, "fr-circ" | "framed-circle" | "stop" => Shape::FrDot,
        "lin-rect" | "lin-proc" | "lined-process" | "lined-rectangle" | "shaded-process" => Shape::LinRect, "notch-pent" | "loop-limit" | "notched-pentagon" => Shape::NotchPent,
        "sl-rect" | "manual-input" | "sloped-rectangle" => Shape::SlRect, "st-rect" | "processes" | "procs" | "stacked-rectangle" => Shape::StRect, "flag" | "paper-tape" => Shape::Flag,
        "bow-rect" | "bow-tie-rectangle" | "stored-data" => Shape::BowRect, "cross-circ" | "crossed-circle" | "summary" => Shape::CrossCirc, "tag-rect" | "tag-proc" | "tagged-process" | "tagged-rectangle" => Shape::TagRect,
        "notch-rect" | "card" | "notched-rectangle" => Shape::NotchRect, "hourglass" | "collate" => Shape::Hourglass, "bolt" | "com-link" | "lightning-bolt" => Shape::Bolt, "text" => Shape::Text,
        "brace" | "brace-l" | "comment" => Shape::BraceL, "brace-r" => Shape::BraceR, "braces" => Shape::Braces, "datastore" | "data-store" => Shape::Datastore,
        _ => Shape::Rect,
    }
}

struct Parser {
    f: Flow,
    ids: HashMap<String, usize>,
    sub_ids: HashMap<String, usize>,
    /// Open subgraphs, innermost last.
    open: Vec<usize>,
    /// Link ids (`A e1@--> B`) and the link each names: `class e1 x`
    /// styles it; its `e1@{…}` settings (animation) are skipped.
    edge_ids: Vec<(String, usize)>,
}

/// A link between groups, as written.
struct LinkSpec { label: String, stroke: Stroke, start: Head, end: Head, len: usize }

impl Parser {
    fn node(&mut self, id: &str) -> usize {
        let here = self.open.last().copied();
        if let Some(&i) = self.ids.get(id) {
            // the first subgraph that mentions a node holds it
            if self.f.nodes[i].sub.is_none() && here.is_some() { self.f.nodes[i].sub = here; }
            return i;
        }
        self.ids.insert(id.to_string(), self.f.nodes.len());
        self.f.nodes.push(Node { id: id.to_string(), label: id.to_string(), shape: Shape::Rect, classes: Vec::new(), style: Vec::new(), sub: here });
        self.f.nodes.len() - 1
    }

    /// One vertex: `id`, `id[text]` (any shape), `id@{…}`, then `:::class`.
    fn vertex(&mut self, c: &mut Cur) -> Result<End, Error> {
        let Some(id) = c.id() else { return c.fail("Expected a node") };
        let id = id.to_string();
        let mut shaped = None;
        if c.rest().starts_with("@{") {
            c.at += 2;
            let body = c.rest();
            let Some(close) = body.find('}') else { return c.fail("Missing “}”") };
            let (mut shape, mut label) = (Shape::Rect, None);
            for part in body[..close].split(',') {
                if let Some((k, v)) = part.split_once(':') {
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    match k.trim() { "shape" => shape = shape_named(v), "label" => label = Some(text::label(v)), _ => {} }
                }
            }
            c.at += close + 1;
            shaped = Some((shape, label));
        } else {
            let save = c.at;
            c.ws();
            let opener = SHAPES.iter().find(|(o, _, _)| c.rest().starts_with(o) && !(*o == ">" && c.at != save));
            match opener {
                Some(&(o, close, shape)) => {
                    c.at += o.len();
                    let mut shape = shape;
                    let t = match shape {
                        // `[/text/]` or `[/text\]`, `[\text\]` or `[\text/]`
                        Shape::LeanR | Shape::LeanL => {
                            let r = c.rest();
                            let (a, b) = (r.find("/]"), r.find("\\]"));
                            let (alt, alt_shape) = if shape == Shape::LeanR { (b, Shape::Trap) } else { (a, Shape::TrapAlt) };
                            let own = if shape == Shape::LeanR { a } else { b };
                            match (own, alt) {
                                (Some(x), Some(y)) if y < x => { shape = alt_shape; c.text_until(if shape == Shape::Trap { "\\]" } else { "/]" })? }
                                (None, Some(_)) => { shape = alt_shape; c.text_until(if shape == Shape::Trap { "\\]" } else { "/]" })? }
                                _ => c.text_until(close)?,
                            }
                        }
                        _ => c.text_until(close)?,
                    };
                    shaped = Some((shape, Some(t)));
                }
                None => c.at = save,
            }
        }
        let end = match shaped {
            // (`sub@{ view: collapsed }` and the like: not drawn)
            _ if self.sub_ids.contains_key(&id) && (shaped.is_none() || c.s[..c.at].ends_with('}')) => End::Sub(self.sub_ids[&id]),
            _ => {
                let i = self.node(&id);
                if let Some((shape, label)) = shaped {
                    self.f.nodes[i].shape = shape;
                    if let Some(l) = label { self.f.nodes[i].label = l; }
                }
                End::Node(i)
            }
        };
        if c.eat(":::") {
            let Some(cls) = c.id() else { return c.fail("Expected a class name") };
            match end {
                End::Node(i) => self.f.nodes[i].classes.push(cls.to_string()),
                End::Sub(i) => self.f.subs[i].classes.push(cls.to_string()),
            }
        }
        Ok(end)
    }

    /// `a & b & c`
    fn group(&mut self, c: &mut Cur) -> Result<Vec<End>, Error> {
        let mut g = vec![self.vertex(c)?];
        loop {
            let save = c.at;
            c.ws();
            if c.eat("&") { c.ws(); g.push(self.vertex(c)?); } else { c.at = save; return Ok(g) }
        }
    }

    /// A link at the cursor (after spaces), with its `|text|`.
    fn link(&mut self, c: &mut Cur) -> Result<Option<LinkSpec>, Error> {
        let save = c.at;
        c.ws();
        // a link id: `e1@-->`
        let r = c.rest();
        if let Some(i) = r.find('@').filter(|&i| i > 0 && r[..i].chars().all(|c| c.is_alphanumeric() || c == '_') && r[i + 1..].starts_with(['-', '=', '~', '<', 'o', 'x'])) {
            self.edge_ids.push((r[..i].to_string(), self.f.links.len()));
            c.at += i + 1;
        }
        let r = c.rest();
        let b = r.as_bytes();
        let at = |i: usize| b.get(i).copied().unwrap_or(0);
        let mut i = 0;
        let mut start = Head::None;
        match at(0) {
            b'<' if matches!(at(1), b'-' | b'=' | b'.') => { start = Head::Arrow; i = 1; }
            b'o' if matches!(at(1), b'-' | b'=' | b'.') && matches!(at(2), b'-' | b'=' | b'.') => { start = Head::Circle; i = 1; }
            b'x' if matches!(at(1), b'-' | b'=' | b'.') && matches!(at(2), b'-' | b'=' | b'.') => { start = Head::Cross; i = 1; }
            _ => {}
        }
        let head_at = |j: usize| -> Option<Head> {
            match at(j) {
                b'>' => Some(Head::Arrow),
                // `--o` / `--x` unless an id follows straight on (`--oB` is text)
                b'o' if !(at(j + 1) as char).is_alphanumeric() => Some(Head::Circle),
                b'x' if !(at(j + 1) as char).is_alphanumeric() => Some(Head::Cross),
                _ => None,
            }
        };
        let mut spec = LinkSpec { label: String::new(), stroke: Stroke::Normal, start, end: Head::None, len: 1 };
        let ch = at(i);
        let run = |from: usize, c: u8| (from..b.len()).take_while(|&j| b[j] == c).count();
        match ch {
            b'~' => {
                let k = run(i, b'~');
                if k < 3 { c.at = save; return Ok(None) }
                spec.stroke = Stroke::Invisible;
                spec.len = k - 2;
                c.at += i + k;
            }
            b'-' if at(i + 1) == b'.' => {
                // -.- -.-> -..-> and -. text .->
                let dots = run(i + 1, b'.');
                let j = i + 1 + dots;
                spec.stroke = Stroke::Dotted;
                if at(j) == b'-' {
                    spec.len = dots;
                    let h = head_at(j + 1);
                    spec.end = h.unwrap_or(Head::None);
                    c.at += j + 1 + h.map_or(0, |_| 1);
                } else {
                    // text form: find `.-` closing it
                    let body = &r[j..];
                    let Some(k) = body.find(".-") else { c.at = save; return Ok(None) };
                    spec.label = text::label(body[..k].trim());
                    let rest = &r[j + k..];
                    let d = rest.bytes().take_while(|&x| x == b'.').count();
                    let after = j + k + d + 1;
                    let h = head_at(after);
                    spec.end = h.unwrap_or(Head::None);
                    spec.len = d;
                    c.at += after + h.map_or(0, |_| 1);
                }
            }
            b'-' | b'=' => {
                let k = run(i, ch);
                if k < 2 { c.at = save; return Ok(None) }
                if ch == b'=' { spec.stroke = Stroke::Thick; }
                if let Some(h) = head_at(i + k) {
                    spec.end = h;
                    spec.len = k - 1;
                    c.at += i + k + 1;
                } else if k >= 3 {
                    spec.len = k - 2;
                    c.at += i + k;
                } else {
                    // `-- text -->` / `== text ==>`: the closing run
                    let body = &r[i + k..];
                    let bb = body.as_bytes();
                    let mut found = None;
                    let mut j = 0;
                    while j < bb.len() {
                        if bb[j] == ch {
                            let kk = (j..bb.len()).take_while(|&x| bb[x] == ch).count();
                            let next = bb.get(j + kk).copied().unwrap_or(0);
                            let head = match next { b'>' => Some(Head::Arrow), b'o' => Some(Head::Circle), b'x' => Some(Head::Cross), _ => None };
                            if kk >= 2 && head.is_some() { found = Some((j, kk, head, 1)); break }
                            if kk >= 3 { found = Some((j, kk, None, 0)); break }
                            j += kk;
                        } else { j += 1; }
                    }
                    let Some((j, kk, head, hl)) = found else { c.at = save; return Ok(None) };
                    spec.label = text::label(body[..j].trim());
                    spec.end = head.unwrap_or(Head::None);
                    spec.len = if head.is_some() { kk - 1 } else { kk - 2 };
                    c.at += i + k + j + kk + hl;
                }
            }
            _ => { c.at = save; return Ok(None) }
        }
        // `|text|`
        let save2 = c.at;
        c.ws();
        if c.eat("|") { spec.label = c.text_until("|")?; } else { c.at = save2; }
        spec.len = spec.len.max(1);
        Ok(Some(spec))
    }

    /// `a --> b & c -- x --> d ...`
    fn chain(&mut self, c: &mut Cur) -> Result<(), Error> {
        let mut left = self.group(c)?;
        while let Some(spec) = self.link(c)? {
            c.ws();
            let right = self.group(c)?;
            for &a in &left {
                for &b in &right {
                    self.f.links.push(Link { from: a, to: b, label: spec.label.clone(), stroke: spec.stroke, start: spec.start, end: spec.end, len: spec.len, style: Vec::new() });
                }
            }
            left = right;
        }
        Ok(())
    }

    fn statement(&mut self, c: &mut Cur) -> Result<(), Error> {
        c.ws();
        let r = c.rest();
        let word = r.split(|ch: char| ch.is_whitespace() || ch == ';').next().unwrap_or("");
        let arg = || r[word.len()..].trim().trim_end_matches(';').trim();
        match word {
            "subgraph" => {
                let a = arg();
                let (id, title) = if let Some(i) = a.find(['[', '(', '{']).filter(|&i| i > 0 && !a[..i].contains('"')) {
                    let id = a[..i].trim();
                    let mut cc = Cur { s: a, at: i, line: c.line };
                    let close = match &a[i..i + 1] { "[" => "]", "(" => ")", _ => "}" };
                    cc.at += 1;
                    (id.to_string(), cc.text_until(close)?)
                } else if a.is_empty() {
                    (format!("subGraph{}", self.f.subs.len()), String::new())
                } else {
                    let t = text::label(a);
                    let id = if a.starts_with('"') { format!("subGraph{}", self.f.subs.len()) } else { a.to_string() };
                    (id, t)
                };
                let i = self.f.subs.len();
                self.f.subs.push(Sub { id: id.clone(), title, parent: self.open.last().copied(), classes: Vec::new(), style: Vec::new() });
                self.sub_ids.insert(id, i);
                self.open.push(i);
                c.at = c.s.len();
            }
            "end" if arg().is_empty() => {
                if self.open.pop().is_none() { return err(c.line, "“end” without a subgraph") }
                c.at = c.s.len();
            }
            "direction" => { c.at = c.s.len(); }
            "classDef" => {
                let a = arg();
                let Some((names, st)) = a.split_once(char::is_whitespace) else { return err(c.line, "classDef needs a name and styles") };
                for name in names.split(',') { self.f.class_defs.insert(name.trim().to_string(), styles(st)); }
                c.at = c.s.len();
            }
            "class" => {
                let a = arg();
                let Some((ids, name)) = a.rsplit_once(char::is_whitespace) else { return err(c.line, "class needs nodes and a class name") };
                for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    if let Some(&s) = self.sub_ids.get(id) { self.f.subs[s].classes.push(name.trim().to_string()); continue }
                    if let Some(&(_, k)) = self.edge_ids.iter().find(|e| e.0 == id) {
                        let st = self.f.class_defs.get(name.trim()).cloned().unwrap_or_default();
                        if let Some(l) = self.f.links.get_mut(k) { l.style.extend(st); }
                        continue
                    }
                    let i = self.node(id);
                    self.f.nodes[i].classes.push(name.trim().to_string());
                }
                c.at = c.s.len();
            }
            "style" => {
                let a = arg();
                let Some((id, st)) = a.split_once(char::is_whitespace) else { return err(c.line, "style needs a node and styles") };
                if let Some(&s) = self.sub_ids.get(id) { self.f.subs[s].style.extend(styles(st)); } else { let i = self.node(id); self.f.nodes[i].style.extend(styles(st)); }
                c.at = c.s.len();
            }
            "linkStyle" => {
                let a = arg();
                let Some((which, st)) = a.split_once(char::is_whitespace) else { return err(c.line, "linkStyle needs links and styles") };
                let st = styles(st);
                for w in which.split(',') {
                    if w == "default" { self.f.default_link = st.clone(); continue }
                    // (links are numbered as written; later ones are styled at the end)
                    let Ok(k) = w.trim().parse::<usize>() else { return err(c.line, format!("“{w}” isn't a link number")) };
                    if let Some(l) = self.f.links.get_mut(k) { l.style.extend(st.iter().cloned()); } else { return err(c.line, format!("There is no link {k}")) }
                }
                c.at = c.s.len();
            }
            _ if word.split_once("@{").is_some_and(|(id, _)| self.edge_ids.iter().any(|e| e.0 == id)) => { c.at = c.s.len(); }
            "click" | "callback" | "href" | "accTitle" | "accDescr" | "accTitle:" | "accDescr:" => { c.at = c.s.len(); }
            _ if word.starts_with("accTitle") || word.starts_with("accDescr") => { c.at = c.s.len(); }
            _ => {
                self.chain(c)?;
                c.ws();
                if !c.done() && !c.eat(";") { return c.fail("Unexpected text") }
            }
        }
        Ok(())
    }
}

pub fn parse(lines: &[Line]) -> Result<Flow, Error> {
    let Some(first) = lines.first() else { return err(0, "The diagram is empty") };
    let mut p = Parser { f: Flow { dir: Dir::TB, nodes: Vec::new(), links: Vec::new(), subs: Vec::new(), class_defs: HashMap::new(), default_link: Vec::new() }, ids: HashMap::new(), sub_ids: HashMap::new(), open: Vec::new(), edge_ids: Vec::new() };
    // header: `graph`, a direction, maybe statements after `;`
    let head = first.text;
    let kw = head.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("");
    let rest = head[kw.len()..].trim_start();
    let d = rest.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("");
    let mut tail = rest;
    if let Some(dir) = dir_of(d) { p.f.dir = dir; tail = &rest[d.len()..]; } else if !d.is_empty() { return err(first.no, format!("“{d}” isn't a direction (TB, TD, BT, LR or RL)")) }
    let tail = tail.trim_start().trim_start_matches(';');
    let run = |s: &str, no: usize, p: &mut Parser| -> Result<(), Error> {
        let mut c = Cur { s, at: 0, line: no };
        while !c.done() {
            p.statement(&mut c)?;
            c.ws();
            while c.eat(";") { c.ws(); }
        }
        Ok(())
    };
    run(tail, first.no, &mut p)?;
    // a quoted label may run over lines (markdown strings): joined first
    let mut i = 1;
    while i < lines.len() {
        let mut text = lines[i].text.to_string();
        let no = lines[i].no;
        i += 1;
        while text.matches('"').count() % 2 == 1 && i < lines.len() { text.push('\n'); text.push_str(lines[i].text); i += 1; }
        run(&text, no, &mut p)?;
    }
    if let Some(&s) = p.open.last() { return err(0, format!("Subgraph “{}” has no “end”", p.f.subs[s].id)) }
    // a link to a subgraph written before the subgraph: to the subgraph
    let mut f = p.f;
    let gone: Vec<(usize, usize)> = f.subs.iter().enumerate().filter_map(|(s, sub)| f.nodes.iter().position(|n| n.id == sub.id).map(|i| (i, s))).collect();
    if !gone.is_empty() {
        let mut map = Vec::with_capacity(f.nodes.len());
        let mut k = 0;
        for i in 0..f.nodes.len() { map.push(if gone.iter().any(|g| g.0 == i) { None } else { k += 1; Some(k - 1) }); }
        let fix = |e: End| match e { End::Node(i) => match gone.iter().find(|g| g.0 == i) { Some(g) => End::Sub(g.1), None => End::Node(map[i].unwrap_or(0)) }, s => s };
        for l in &mut f.links { l.from = fix(l.from); l.to = fix(l.to); }
        let mut i = 0;
        f.nodes.retain(|_| { i += 1; map[i - 1].is_some() });
    }
    Ok(f)
}

/// Colours and strokes a node or subgraph's classes and style set.
#[derive(Clone, Debug, Default)]
struct Look { fill: Option<String>, stroke: Option<String>, width: Option<f32>, color: Option<String>, dash: Option<String>, bold: bool }

fn look(pairs: &[&[(String, String)]]) -> Look {
    let mut l = Look::default();
    for set in pairs { for (k, v) in set.iter() {
        match k.as_str() {
            "fill" => l.fill = Some(v.clone()),
            "stroke" => l.stroke = Some(v.clone()),
            "stroke-width" => l.width = v.trim_end_matches("px").trim().parse().ok(),
            "color" => l.color = Some(v.clone()),
            "stroke-dasharray" => l.dash = Some(v.replace(',', " ")),
            "font-weight" => l.bold = v == "bold" || v.parse::<u32>().is_ok_and(|w| w >= 600),
            _ => {}
        }
    } }
    l
}

/// Horizontal and vertical padding around a node's text.
const PAD: (f32, f32) = (16., 10.);
/// Labels wrap at this width, like Mermaid's.
const WRAP: f32 = 200.;

/// A node's size for its text and shape.
fn size(shape: Shape, t: &Block) -> (f32, f32) {
    let (tw, th) = (t.w, t.h.max(t.size * text::LEADING));
    let (w, h) = (tw + 2. * PAD.0, th + 2. * PAD.1);
    match shape {
        Shape::Rect | Shape::Round => (w.max(48.), h),
        Shape::Subroutine => (w.max(48.) + 16., h),
        Shape::Stadium => (tw + h + PAD.0, h),
        Shape::Cylinder => { let ry = (w / 2.) / (2.5 + w / 50.); (w.max(48.), h + 2. * ry) }
        Shape::Circle => { let d = tw.max(th) + 2. * PAD.1 + 4.; (d, d) }
        Shape::DoubleCircle => { let d = tw.max(th) + 2. * PAD.1 + 14.; (d, d) }
        Shape::Rhombus if t.is_empty() => (28., 28.),
        Shape::Rhombus => { let s = tw + th + 2. * PAD.1 + 8.; (s.max(h * 1.6), s.max(h * 1.6).min(s * 0.75).max(h)) }
        Shape::Hexagon => (w + h / 2., h),
        Shape::LeanR | Shape::LeanL | Shape::Trap | Shape::TrapAlt => (w + h * 0.8, h),
        Shape::Asym => (w + h / 4., h),
        Shape::Doc | Shape::LinDoc | Shape::TagDoc => (w + if shape == Shape::LinDoc { 8. } else { 0. }, h * 1.15),
        Shape::Docs | Shape::StRect => (w + 10., h * if shape == Shape::Docs { 1.15 } else { 1. } + 10.),
        Shape::Delay | Shape::CurvTrap | Shape::NotchPent | Shape::BowRect => (w + h * 0.5, h),
        Shape::HCyl => (w + h * 0.5, h),
        Shape::LinCyl => { let (w, h) = size(Shape::Cylinder, t); (w, h + 6.) }
        Shape::DivRect => (w, h + 10.),
        Shape::Tri | Shape::FlipTri => { let s = (tw + 2. * PAD.0) * 1.8; (s, (th + 2. * PAD.1) * 1.8) }
        Shape::Fork => (70., 10.),
        Shape::WinPane | Shape::LinRect => (w + 8., h + if shape == Shape::WinPane { 8. } else { 0. }),
        Shape::Dot | Shape::FDot | Shape::FrDot => (14., 14.),
        Shape::CrossCirc => (30., 30.),
        Shape::Bolt => (24., 36.),
        Shape::SlRect => (w, h + 10.),
        Shape::Flag => (w, h * 1.3),
        Shape::TagRect | Shape::NotchRect | Shape::Text | Shape::Datastore => (w, h),
        // a small hourglass with its label under it
        Shape::Hourglass => (tw.max(30.), 34. + th),
        Shape::BraceL | Shape::BraceR => (w + 10., h),
        Shape::Braces => (w + 20., h),
    }
}

/// The outline of a node centred at (cx, cy), as a polygon (curves in
/// short segments), for clipping edges to.
fn outline(shape: Shape, cx: f32, cy: f32, w: f32, h: f32) -> Vec<(f32, f32)> {
    let (l, r, t, b) = (cx - w / 2., cx + w / 2., cy - h / 2., cy + h / 2.);
    let ellipse = |rx: f32, ry: f32| (0..32).map(|i| { let a = i as f32 * std::f32::consts::TAU / 32.; (cx + rx * a.cos(), cy + ry * a.sin()) }).collect();
    match shape {
        Shape::Circle | Shape::DoubleCircle => ellipse(w / 2., h / 2.),
        Shape::Rhombus => vec![(cx, t), (r, cy), (cx, b), (l, cy)],
        Shape::Hexagon => { let k = h / 4.; vec![(l + k, t), (r - k, t), (r, cy), (r - k, b), (l + k, b), (l, cy)] }
        Shape::LeanR => { let k = h * 0.4; vec![(l + k, t), (r, t), (r - k, b), (l, b)] }
        Shape::LeanL => { let k = h * 0.4; vec![(l, t), (r - k, t), (r, b), (l + k, b)] }
        Shape::Trap => { let k = h * 0.4; vec![(l + k, t), (r - k, t), (r, b), (l, b)] }
        Shape::TrapAlt => { let k = h * 0.4; vec![(l, t), (r, t), (r - k, b), (l + k, b)] }
        Shape::Asym => { let k = h / 4.; vec![(l, t), (r, t), (r, b), (l, b), (l + k, cy)] }
        Shape::Doc | Shape::LinDoc | Shape::TagDoc => { let mut p = vec![(l, t), (r, t)]; p.extend(wave(r, l, b - h * 0.08, h * 0.08)); p }
        Shape::Flag => { let a = h * 0.08; let mut p = wave(l, r, t + a, a); p.extend(wave(r, l, b - a, a)); p }
        Shape::Delay => {
            let rr = h / 2.;
            let mut p = vec![(l, t)];
            for i in 0..=8 { let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((r - rr + rr * a.cos(), cy + rr * a.sin())); }
            p.push((l, b));
            p
        }
        Shape::HCyl => {
            let rx = h / 4.;
            let mut p = Vec::new();
            for i in 0..=8 { let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((r - rx + rx * a.cos(), cy + h / 2. * a.sin())); }
            for i in 0..=8 { let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((l + rx + rx * a.cos(), cy + h / 2. * a.sin())); }
            p
        }
        Shape::CurvTrap => {
            let k = h * 0.25;
            let mut p = vec![(l + k, t)];
            for i in 0..=8 { let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((r - k + k * a.cos(), cy + h / 2. * a.sin())); }
            p.extend([(l + k, b), (l, cy)]);
            p
        }
        Shape::BowRect => {
            let k = h * 0.2;
            let mut p = vec![(l + k, t), (r, t)];
            for i in 1..8 { let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((r - k * a.cos(), cy + h / 2. * a.sin())); }
            p.push((r, b));
            p.push((l + k, b));
            for i in 1..8 { let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((l + k + k * a.cos(), cy + h / 2. * a.sin())); }
            p
        }
        Shape::Tri => vec![(cx, t), (r, b), (l, b)],
        Shape::FlipTri => vec![(l, t), (r, t), (cx, b)],
        Shape::NotchPent => { let k = h * 0.3; vec![(l + k, t), (r - k, t), (r, t + k), (r, b), (l, b), (l, t + k)] }
        Shape::SlRect => vec![(l, t + 10.), (r, t), (r, b), (l, b)],
        Shape::NotchRect => vec![(l + 12., t), (r, t), (r, b), (l, b), (l, t + 12.)],
        Shape::Bolt => vec![(l + w * 0.55, t), (l, cy + h * 0.08), (cx, cy + h * 0.08), (l + w * 0.4, b), (r, cy - h * 0.08), (cx, cy - h * 0.08)],
        Shape::Dot | Shape::FDot | Shape::FrDot | Shape::CrossCirc => ellipse(w / 2., h / 2.),
        Shape::Stadium => {
            let rr = h / 2.;
            let mut p = Vec::new();
            for i in 0..=8 { let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((r - rr + rr * a.cos(), cy + rr * a.sin())); }
            for i in 0..=8 { let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 8.; p.push((l + rr + rr * a.cos(), cy + rr * a.sin())); }
            p
        }
        _ => vec![(l, t), (r, t), (r, b), (l, b)],
    }
}

/// A wave from `x0` to `x1` about `y`: one period, `amp` high.
fn wave(x0: f32, x1: f32, y: f32, amp: f32) -> Vec<(f32, f32)> {
    (0..=16).map(|i| { let u = i as f32 / 16.; (x0 + (x1 - x0) * u, y + amp * (u * std::f32::consts::TAU).sin()) }).collect()
}

/// Where the segment from `inside` to `outside` leaves `poly`.
fn clip(poly: &[(f32, f32)], inside: (f32, f32), outside: (f32, f32)) -> (f32, f32) {
    let mut best: Option<f32> = None;
    let (dx, dy) = (outside.0 - inside.0, outside.1 - inside.1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (ex, ey) = (b.0 - a.0, b.1 - a.1);
        let den = dx * ey - dy * ex;
        if den.abs() < 1e-6 { continue }
        let t = ((a.0 - inside.0) * ey - (a.1 - inside.1) * ex) / den;
        let u = ((a.0 - inside.0) * dy - (a.1 - inside.1) * dx) / den;
        if (0. ..=1.).contains(&t) && (-1e-4..=1. + 1e-4).contains(&u) { best = Some(best.map_or(t, |x: f32| x.max(t))); }
    }
    match best { Some(t) => (inside.0 + dx * t, inside.1 + dy * t), None => inside }
}

pub fn render(lines: &[Line], title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let f = parse(lines)?;
    draw(&f, title, style, m)
}

pub(crate) fn draw(f: &Flow, title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let fs = style.font_size;
    let col = &style.colors;
    let horizontal = matches!(f.dir, Dir::LR | Dir::RL);
    let none: Vec<(String, String)> = Vec::new();
    let class = |name: &String| f.class_defs.get(name).unwrap_or(&none).as_slice();
    let node_look: Vec<Look> = f.nodes.iter().map(|nd| {
        let mut sets: Vec<&[(String, String)]> = vec![f.class_defs.get("default").map_or(&[][..], |v| v.as_slice())];
        sets.extend(nd.classes.iter().map(class));
        sets.push(&nd.style);
        look(&sets)
    }).collect();
    let blocks: Vec<Block> = f.nodes.iter().zip(&node_look).map(|(nd, lk)| Block::new(&nd.label, WRAP, fs, lk.bold, m)).collect();
    // (a fork bar lies across the flow)
    let sizes: Vec<(f32, f32)> = f.nodes.iter().zip(&blocks).map(|(nd, b)| { let (w, h) = size(nd.shape, b); if horizontal && nd.shape == Shape::Fork { (h, w) } else { (w, h) } }).collect();
    let link_blocks: Vec<Block> = f.links.iter().map(|l| Block::new(&l.label, WRAP, fs, false, m)).collect();
    let sub_blocks: Vec<Block> = f.subs.iter().map(|s| Block::new(&s.title, 0., fs, false, m)).collect();
    let swap = |(w, h): (f32, f32)| if horizontal { (h, w) } else { (w, h) };

    // the layout graph: nodes, then a stand-in for each empty subgraph
    let mut g = layout::Graph { nodes: Vec::new(), edges: Vec::new(), clusters: Vec::new(), ranks: Vec::new() };
    for (i, nd) in f.nodes.iter().enumerate() { let (w, h) = swap(sizes[i]); g.nodes.push(layout::Node { w, h, cluster: nd.sub }); }
    let title_side = match f.dir { Dir::TB => 0, Dir::BT => 2, _ => 3 };
    for (i, s) in f.subs.iter().enumerate() {
        let b = &sub_blocks[i];
        let mut extra = [0.; 4];
        if !b.is_empty() { extra[title_side] = b.h + 4.; }
        let min = swap((b.w + 24., 0.));
        g.clusters.push(layout::Cluster { parent: s.parent, extra, min });
    }
    // a subgraph at a link's end stands in as one of its nodes: one no
    // link inside it leaves (as a source), or enters (as a target), so the
    // rest of the graph ranks past the whole box
    let within = |mut c: Option<usize>, s: usize| { let mut k = 0; while let Some(x) = c { if x == s { return true } k += 1; if k > 64 { break } c = f.subs.get(x).and_then(|x| x.parent); } false };
    // (and the rest of those ends get rank-only edges, so every region of
    // a box ranks past what links to it)
    let mut proxy: Vec<(usize, usize)> = Vec::new();
    let mut ends: Vec<(Vec<usize>, Vec<usize>)> = Vec::new();
    for s in 0..f.subs.len() {
        let members: Vec<usize> = (0..f.nodes.len()).filter(|&i| within(f.nodes[i].sub, s)).collect();
        if members.is_empty() {
            let i = g.nodes.len();
            g.nodes.push(layout::Node { w: 1., h: 1., cluster: Some(s) });
            proxy.push((i, i));
            ends.push((Vec::new(), Vec::new()));
            continue;
        }
        let inner = |e: End| matches!(e, End::Node(i) if members.contains(&i));
        let has = |v: usize, out: bool| f.links.iter().any(|l| inner(l.from) && inner(l.to) && if out { l.from == End::Node(v) } else { l.to == End::Node(v) });
        let firsts: Vec<usize> = members.iter().copied().filter(|&v| !has(v, false)).collect();
        let lasts: Vec<usize> = members.iter().rev().copied().filter(|&v| !has(v, true)).collect();
        proxy.push((lasts.first().copied().unwrap_or(members[members.len() - 1]), firsts.first().copied().unwrap_or(members[0])));
        ends.push((lasts, firsts));
    }
    let endpoint = |e: End, out: bool| match e { End::Node(i) => Some(i), End::Sub(s) => proxy.get(s).map(|p| if out { p.0 } else { p.1 }) };
    let mut extra: Vec<layout::Edge> = Vec::new();
    let mut edge_of: Vec<Option<usize>> = Vec::new();
    for (i, l) in f.links.iter().enumerate() {
        let (Some(a), Some(b)) = (endpoint(l.from, true), endpoint(l.to, false)) else { edge_of.push(None); continue };
        // a subgraph linked to its own member: nothing to draw
        if a == b && (matches!(l.from, End::Sub(_)) || matches!(l.to, End::Sub(_))) { edge_of.push(None); continue }
        let lb = &link_blocks[i];
        let label = if lb.is_empty() { (0., 0.) } else { swap((lb.w + 8., lb.h + 4.)) };
        edge_of.push(Some(g.edges.len()));
        g.edges.push(layout::Edge { from: a, to: b, minlen: l.len, label });
        let others = |e: End, out: bool| match e { End::Sub(s) => ends.get(s).map_or(&[][..], |x| if out { &x.0[..] } else { &x.1[..] }).iter().skip(1).copied().take(16).collect(), _ => Vec::new() };
        for x in others(l.from, true) { if x != b { extra.push(layout::Edge { from: x, to: b, minlen: l.len, label: (0., 0.) }); } }
        for x in others(l.to, false) { if x != a { extra.push(layout::Edge { from: a, to: x, minlen: l.len, label: (0., 0.) }); } }
    }
    g.ranks = extra;
    let sp = layout::Spacing { node: 40., rank: 64., edge: 12., pad: 14. };
    let o = layout::layout(&g, &sp);

    // to the drawing's own axes, with a margin and the title on top
    let margin = 8.;
    let title_block = title.as_deref().map(|t| Block::new(t, 0., fs * 1.3, true, m)).filter(|b| !b.is_empty());
    let top = margin + title_block.as_ref().map_or(0., |b| b.h + 10.);
    let (lw, lh) = (o.width, o.height);
    let tf = |(x, y): (f32, f32)| -> (f32, f32) {
        let (x, y) = match f.dir { Dir::TB => (x, y), Dir::BT => (x, lh - y), Dir::LR => (y, x), Dir::RL => (lh - y, x) };
        (x + margin, y + top)
    };
    let tf_rect = |r: Rect| -> Rect {
        let (a, b) = (tf((r.x, r.y)), tf((r.x + r.w, r.y + r.h)));
        Rect { x: a.0.min(b.0), y: a.1.min(b.1), w: (a.0 - b.0).abs(), h: (a.1 - b.1).abs() }
    };
    let (dw, dh) = swap((lw, lh));
    let title_w = title_block.as_ref().map_or(0., |b| b.w);
    let width = dw.max(title_w) + 2. * margin;
    let height = dh + top + margin;
    let mut doc = Doc::new(&style.font);
    if let Some(b) = &title_block { doc.text(b, width / 2., margin + b.h / 2., Anchor::Middle, &col.text); }

    // subgraphs, outermost first
    let depth = |mut s: usize| { let mut d = 0; while let Some(p) = f.subs.get(s).and_then(|x| x.parent) { d += 1; s = p; if d > 64 { break } } d };
    let mut subs: Vec<usize> = (0..f.subs.len()).collect();
    subs.sort_by_key(|&s| depth(s));
    let sub_rect: Vec<Rect> = (0..f.subs.len()).map(|s| tf_rect(o.clusters.get(s).copied().unwrap_or_default())).collect();
    for &s in &subs {
        let r = sub_rect[s];
        if r.w <= 0. { continue }
        let mut sets: Vec<&[(String, String)]> = f.subs[s].classes.iter().map(class).collect();
        sets.push(&f.subs[s].style);
        let lk = look(&sets);
        doc.rect(r.x, r.y, r.w, r.h, 4., &Paint::new(lk.fill.as_deref().unwrap_or(&col.cluster_fill), lk.stroke.as_deref().unwrap_or(&col.cluster_border), lk.width.unwrap_or(1.)).dashed_opt(lk.dash.as_deref()));
        let b = &sub_blocks[s];
        doc.text(b, r.x + r.w / 2., r.y + 8. + b.h / 2., Anchor::Middle, lk.color.as_deref().unwrap_or(&col.text));
    }

    // node outlines in drawing space
    let centre: Vec<(f32, f32)> = (0..f.nodes.len()).map(|i| tf(o.nodes.get(i).copied().unwrap_or_default())).collect();
    let poly = |e: End| -> Vec<(f32, f32)> {
        match e {
            End::Node(i) => { let (cx, cy) = centre[i]; outline(f.nodes[i].shape, cx, cy, sizes[i].0, sizes[i].1) }
            End::Sub(s) => { let r = sub_rect[s]; vec![(r.x, r.y), (r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h)] }
        }
    };

    // edges, then their labels over them
    let mut labels: Vec<(usize, (f32, f32))> = Vec::new();
    for (i, l) in f.links.iter().enumerate() {
        let Some(e) = edge_of[i] else { continue };
        let mut pts: Vec<(f32, f32)> = o.edges.get(e).map(|p| p.iter().map(|&q| tf(q)).collect()).unwrap_or_default();
        if pts.len() < 2 { continue }
        if let Some(c) = o.labels.get(e).copied().flatten() { labels.push((i, tf(c))); }
        if l.stroke == Stroke::Invisible { continue }
        let self_loop = endpoint(l.from, true) == endpoint(l.to, false);
        if !self_loop {
            let k = pts.len();
            // where a subgraph is the end, the path runs to its box: drop
            // the points inside it
            if let End::Sub(s) = l.from { let r = sub_rect[s]; while pts.len() > 2 && inside(r, pts[1]) { pts.remove(0); } }
            if let End::Sub(s) = l.to { let r = sub_rect[s]; while pts.len() > 2 && inside(r, pts[pts.len() - 2]) { pts.pop(); } }
            let _ = k;
            let n0 = pts.len();
            pts[0] = clip(&poly(l.from), pts[0], pts[1]);
            pts[n0 - 1] = clip(&poly(l.to), pts[n0 - 1], pts[n0 - 2]);
        }
        let mut lk = look(&[&f.default_link, &l.style]);
        if lk.dash.is_none() && l.stroke == Stroke::Dotted { lk.dash = Some("3 3".into()); }
        let color = lk.stroke.clone().unwrap_or_else(|| col.line.clone());
        let width = lk.width.unwrap_or(if l.stroke == Stroke::Thick { 3. } else { 1.5 });
        let head_len = 8. + width * 1.5;
        // the line stops where an arrowhead starts
        let ends = [(l.start, 0usize, 1usize), (l.end, pts.len() - 1, pts.len() - 2)];
        let mut heads = Vec::new();
        for &(h, at, prev) in &ends {
            if h == Head::None { continue }
            let (tip, from) = (pts[at], pts[prev]);
            let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-3 { continue }
            let dir = (dx / len, dy / len);
            heads.push((tip, dir, h));
            let back = match h { Head::Arrow => head_len * 0.8, Head::Circle => head_len * 0.85, _ => 0. }.min(len * 0.9);
            pts[at] = (tip.0 - dir.0 * back, tip.1 - dir.1 * back);
        }
        let mut p = Paint::new("none", &color, width);
        p.dash = lk.dash.clone();
        doc.path(&svg::basis(&pts), &p);
        for (tip, dir, h) in heads { doc.head(tip, dir, h, head_len, &color); }
    }
    for (i, (cx, cy)) in labels {
        let b = &link_blocks[i];
        let lk = look(&[&f.default_link, &f.links[i].style]);
        doc.rect(cx - b.w / 2. - 4., cy - b.h / 2. - 2., b.w + 8., b.h + 4., 2., &Paint::new(&col.background, "none", 0.));
        doc.text(b, cx, cy, Anchor::Middle, lk.color.as_deref().unwrap_or(&col.text));
    }

    // nodes
    for (i, nd) in f.nodes.iter().enumerate() {
        let (cx, cy) = centre[i];
        let (w, h) = sizes[i];
        let lk = &node_look[i];
        let mut p = Paint::new(lk.fill.as_deref().unwrap_or(&col.node_fill), lk.stroke.as_deref().unwrap_or(&col.node_border), lk.width.unwrap_or(1.));
        p.dash = lk.dash.clone();
        let (l, t) = (cx - w / 2., cy - h / 2.);
        match nd.shape {
            Shape::Rect => doc.rect(l, t, w, h, 0., &p),
            Shape::Round => doc.rect(l, t, w, h, 6., &p),
            Shape::Stadium => doc.rect(l, t, w, h, h / 2., &p),
            Shape::Subroutine => {
                doc.rect(l, t, w, h, 0., &p);
                let q = Paint::new("none", &p.stroke, p.width);
                doc.line((l + 8., t), (l + 8., t + h), &q);
                doc.line((l + w - 8., t), (l + w - 8., t + h), &q);
            }
            Shape::Cylinder => {
                let rx = w / 2.;
                let ry = rx / (2.5 + w / 50.);
                let (x0, x1, y0, y1) = (l, l + w, t + ry, t + h - ry);
                let (a, b, c, d) = (n(x0), n(x1), n(y0), n(y1));
                let (rxs, rys) = (n(rx), n(ry));
                doc.path(&format!("M{a},{c} A{rxs},{rys} 0 0 1 {b},{c} L{b},{d} A{rxs},{rys} 0 0 1 {a},{d} Z"), &p);
                doc.path(&format!("M{a},{c} A{rxs},{rys} 0 0 0 {b},{c}"), &Paint { fill: "none".into(), ..p.clone() });
            }
            Shape::Circle => doc.circle(cx, cy, w / 2., &p),
            Shape::DoubleCircle => { doc.circle(cx, cy, w / 2., &p); doc.circle(cx, cy, w / 2. - 5., &Paint { fill: "none".into(), ..p.clone() }); }
            Shape::Docs | Shape::StRect => {
                // two sheets behind the front one
                let s = if nd.shape == Shape::Docs { Shape::Doc } else { Shape::Rect };
                for k in [10., 5., 0.] { doc.polygon(&outline(s, cx - 5. + k, cy + 5. - k, w - 10., h - 10.), &p); }
            }
            Shape::LinCyl => {
                let rx = w / 2.;
                let ry = rx / (2.5 + w / 50.);
                let (a, b, c, d, e) = (n(l), n(l + w), n(t + ry), n(t + h - ry), n(t + ry + 6.));
                let (rxs, rys) = (n(rx), n(ry));
                doc.path(&format!("M{a},{c} A{rxs},{rys} 0 0 1 {b},{c} L{b},{d} A{rxs},{rys} 0 0 1 {a},{d} Z"), &p);
                doc.path(&format!("M{a},{c} A{rxs},{rys} 0 0 0 {b},{c} M{a},{e} A{rxs},{rys} 0 0 0 {b},{e}"), &Paint { fill: "none".into(), ..p.clone() });
            }
            Shape::FDot => doc.circle(cx, cy, w / 2., &Paint::new(&p.stroke, &p.stroke, p.width)),
            Shape::FrDot => { doc.circle(cx, cy, w / 2., &p); doc.circle(cx, cy, w / 2. - 3., &Paint::new(&p.stroke, "none", 0.)); }
            Shape::Fork => doc.rect(l, t, w, h, 0., &Paint::new(&p.stroke, &p.stroke, p.width)),
            Shape::Text => {}
            Shape::Hourglass => doc.polygon(&[(cx - 15., t), (cx + 15., t), (cx - 15., t + 30.), (cx + 15., t + 30.)], &p),
            Shape::Datastore => {
                doc.rect(l, t, w, h, 0., &Paint::new(&p.fill, "none", 0.));
                let q = Paint::new("none", &p.stroke, p.width);
                doc.line((l, t), (l + w, t), &q);
                doc.line((l, t + h), (l + w, t + h), &q);
            }
            Shape::BraceL | Shape::BraceR | Shape::Braces => {
                let q = Paint::new("none", &p.stroke, p.width.max(1.));
                let brace = |x: f32, k: f32| format!("M{},{} Q{},{} {},{} L{},{} Q{},{} {},{} Q{},{} {},{} L{},{} Q{},{} {},{}",
                    n(x + 8. * k), n(t), n(x), n(t), n(x), n(t + 8.), n(x), n(cy - 6.), n(x), n(cy), n(x - 5. * k), n(cy), n(x), n(cy), n(x), n(cy + 6.), n(x), n(t + h - 8.), n(x), n(t + h), n(x + 8. * k), n(t + h));
                if nd.shape != Shape::BraceR { doc.path(&brace(l + 5., 1.), &q); }
                if nd.shape != Shape::BraceL { doc.path(&brace(l + w - 5., -1.), &q); }
            }
            _ => doc.polygon(&outline(nd.shape, cx, cy, w, h), &p),
        }
        // the lines some shapes carry over their body
        let q = Paint::new("none", &p.stroke, p.width);
        match nd.shape {
            Shape::LinDoc | Shape::LinRect => doc.line((l + 8., t), (l + 8., t + h * if nd.shape == Shape::LinDoc { 0.9 } else { 1. }), &q),
            Shape::DivRect => doc.line((l, t + 10.), (l + w, t + 10.), &q),
            Shape::WinPane => { doc.line((l, t + 8.), (l + w, t + 8.), &q); doc.line((l + 8., t), (l + 8., t + h), &q); }
            Shape::HCyl => {
                let rx = h / 4.;
                doc.path(&format!("M{},{} A{},{} 0 0 0 {},{}", n(l + w - rx), n(t), n(rx), n(h / 2.), n(l + w - rx), n(t + h)), &q);
            }
            Shape::TagDoc | Shape::TagRect => {
                let (x, y) = (l + w, t + h * if nd.shape == Shape::TagDoc { 0.85 } else { 1. });
                doc.polygon(&[(x, y - 14.), (x, y), (x - 14., y)], &Paint::new(&p.stroke, &p.stroke, p.width));
            }
            Shape::CrossCirc => {
                let r = w / 2. * std::f32::consts::FRAC_1_SQRT_2;
                doc.line((cx - r, cy - r), (cx + r, cy + r), &q);
                doc.line((cx - r, cy + r), (cx + r, cy - r), &q);
            }
            _ => {}
        }
        let (dx, dy) = match nd.shape { Shape::Asym => (h / 8., 0.), Shape::Tri => (0., h / 6.), Shape::FlipTri => (0., -h / 6.), Shape::Doc | Shape::TagDoc => (0., -h * 0.05), Shape::LinDoc => (4., -h * 0.05), Shape::Docs | Shape::StRect => (-5., 5.), Shape::DivRect | Shape::SlRect => (0., 5.), Shape::WinPane => (4., 4.), Shape::LinRect => (4., 0.), Shape::Hourglass => (0., 17.), _ => (0., 0.) };
        // the small marks carry no text
        if !matches!(nd.shape, Shape::Dot | Shape::FDot | Shape::FrDot | Shape::Fork | Shape::Bolt | Shape::CrossCirc) {
            doc.text(&blocks[i], cx + dx, cy + dy, Anchor::Middle, lk.color.as_deref().unwrap_or(&col.text));
        }
    }
    Ok(Svg { svg: doc.finish(width, height, ""), width, height })
}

fn inside(r: Rect, p: (f32, f32)) -> bool { p.0 > r.x && p.0 < r.x + r.w && p.1 > r.y && p.1 < r.y + r.h }

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Flow { let (l, _) = crate::lines(src); parse(&l).unwrap_or_else(|e| panic!("{e}: {src}")) }
    fn ids(f: &Flow) -> Vec<&str> { f.nodes.iter().map(|n| n.id.as_str()).collect() }
    fn bad(src: &str) -> Error { let (l, _) = crate::lines(src); parse(&l).expect_err(src) }

    #[test]
    fn header_and_direction() {
        assert_eq!(p("graph LR\nA-->B").dir, Dir::LR);
        assert_eq!(p("flowchart TD\nA").dir, Dir::TB);
        assert_eq!(p("graph\nA").dir, Dir::TB);
        assert_eq!(p("graph BT;A-->B;B-->C").links.len(), 2);
        assert_eq!(bad("graph XY\nA").line, 1);
    }

    #[test]
    fn shapes() {
        let f = p("flowchart LR\nA[rect] --> B(round) --> C([stadium]) --> D[[sub]] --> E[(db)]\nF((circle)) --> G(((double))) --> H>asym] --> I{rhombus} --> J{{hex}}\nK[/lean r/] --> L[\\lean l\\] --> M[/trap\\] --> N[\\trap alt/]\nO@{ shape: diamond, label: \"new\" }");
        let shapes: Vec<Shape> = f.nodes.iter().map(|n| n.shape).collect();
        use Shape::*;
        assert_eq!(shapes, [Rect, Round, Stadium, Subroutine, Cylinder, Circle, DoubleCircle, Asym, Rhombus, Hexagon, LeanR, LeanL, Trap, TrapAlt, Rhombus]);
        assert_eq!(f.nodes[0].label, "rect");
        assert_eq!(f.nodes[14].label, "new");
        assert_eq!(f.nodes[12].label, "trap");
    }

    #[test]
    fn links() {
        let f = p("graph TD\nA --> B\nA --- B\nA -.-> B\nA ==> B\nA -- text --> B\nA -->|text| B\nA -. dotted .-> B\nA == thick ==> B\nA --o B\nA --x B\nA <--> B\nA o--o B\nA ~~~ B\nA ----> B\nA -- t --- B\nA---B\nA-->B");
        let l = &f.links;
        assert_eq!(l.len(), 17);
        assert_eq!((l[0].stroke, l[0].end, l[0].len), (Stroke::Normal, Head::Arrow, 1));
        assert_eq!((l[1].stroke, l[1].end), (Stroke::Normal, Head::None));
        assert_eq!((l[2].stroke, l[2].end), (Stroke::Dotted, Head::Arrow));
        assert_eq!((l[3].stroke, l[3].end), (Stroke::Thick, Head::Arrow));
        assert_eq!((l[4].label.as_str(), l[4].end), ("text", Head::Arrow));
        assert_eq!(l[5].label, "text");
        assert_eq!((l[6].label.as_str(), l[6].stroke, l[6].end), ("dotted", Stroke::Dotted, Head::Arrow));
        assert_eq!((l[7].label.as_str(), l[7].stroke), ("thick", Stroke::Thick));
        assert_eq!(l[8].end, Head::Circle);
        assert_eq!(l[9].end, Head::Cross);
        assert_eq!((l[10].start, l[10].end), (Head::Arrow, Head::Arrow));
        assert_eq!((l[11].start, l[11].end), (Head::Circle, Head::Circle));
        assert_eq!(l[12].stroke, Stroke::Invisible);
        assert_eq!(l[13].len, 3);
        assert_eq!((l[14].label.as_str(), l[14].end), ("t", Head::None));
        assert_eq!(ids(&f), ["A", "B"]);
    }

    #[test]
    fn chains_and_groups() {
        let f = p("graph LR\na --> b & c--> d\nA & B--> C & D");
        assert_eq!(f.links.len(), 2 + 2 + 4);
        assert_eq!(ids(&f), ["a", "b", "c", "d", "A", "B", "C", "D"]);
    }

    #[test]
    fn ids_with_dashes_and_unicode() {
        let f = p("graph LR\nmy-node-->other_1\n节点A --> 节点B[中文]");
        assert_eq!(ids(&f), ["my-node", "other_1", "节点A", "节点B"]);
        assert_eq!(f.nodes[3].label, "中文");
    }

    #[test]
    fn quoted_labels_and_entities() {
        let f = p("graph LR\nA[\"a (paren) [b]\"] -->|\"x|y\"| B[\"#quot;q#quot;\"]\nC[a<br/>b]");
        assert_eq!(f.nodes[0].label, "a (paren) [b]");
        assert_eq!(f.nodes[1].label, "\"q\"");
        assert_eq!(f.nodes[2].label, "a\nb");
    }

    #[test]
    fn subgraphs() {
        let f = p("flowchart TB\n c1-->a2\n subgraph one\n a1-->a2\n end\n subgraph two [Two]\n b1-->b2\n subgraph inner\n x\n end\n end\n subgraph \"Quoted title\"\n q\n end\n one --> two\n c1 --> inner");
        assert_eq!(f.subs.len(), 4);
        assert_eq!(f.subs[1].title, "Two");
        assert_eq!(f.subs[2].parent, Some(1));
        assert_eq!(f.subs[3].title, "Quoted title");
        let n = |id: &str| f.nodes.iter().find(|n| n.id == id).unwrap();
        assert_eq!(n("a2").sub, Some(0));
        assert_eq!(n("c1").sub, None);
        assert_eq!(n("x").sub, Some(2));
        assert_eq!(f.links[3].from, End::Sub(0));
        assert_eq!(f.links[4].to, End::Sub(2));
        assert!(bad("graph TD\nsubgraph a\nb").message.contains("end"));
        assert!(bad("graph TD\nend").message.contains("end"));
    }

    #[test]
    fn classes_and_styles() {
        let f = p("graph LR\nclassDef hot fill:#f96,stroke:#333,stroke-width:4px\nclassDef a,b color:red\nA:::hot --> B\nclass B hot\nstyle A fill:rgb(1, 2, 3),stroke-dasharray: 5 5\nlinkStyle 0 stroke:#ff3,stroke-width:4px");
        assert_eq!(f.nodes[0].classes, ["hot"]);
        assert_eq!(f.nodes[1].classes, ["hot"]);
        assert_eq!(f.nodes[0].style[0], ("fill".into(), "rgb(1, 2, 3)".into()));
        assert_eq!(f.class_defs["hot"].len(), 3);
        assert!(f.class_defs.contains_key("b"));
        assert_eq!(f.links[0].style.len(), 2);
        assert!(bad("graph LR\nA-->B\nlinkStyle 5 stroke:red").message.contains("no link"));
    }

    #[test]
    fn link_ids_and_long_labels() {
        let f = p("flowchart LR\n  Client e1@--> API\n  API e2@==>|x| Client\n  e1@{ animate: true }\n  m[\"`Line1\n    Line 2`\"] --> Client");
        assert_eq!(ids(&f), ["Client", "API", "m"]);
        assert_eq!((f.links[1].stroke, f.links[1].label.as_str()), (Stroke::Thick, "x"));
        assert_eq!(f.nodes[2].label, "Line1\nLine 2");
    }

    #[test]
    fn subgraphs_named_before_they_open() {
        let f = p("flowchart TD\n Start --> one\n subgraph one [My Group]\n A --> B\n end\n one --> End\n one@{ view: collapsed }\n x e1@--> y\n classDef dash stroke-dasharray: 9 5\n class e1 dash");
        assert_eq!(ids(&f), ["Start", "A", "B", "End", "x", "y"]);
        assert_eq!(f.links[0].to, End::Sub(0));
        assert_eq!(f.links[1].from, End::Node(1));
        assert_eq!(f.links[3].style, [("stroke-dasharray".to_string(), "9 5".to_string())]);
    }

    #[test]
    fn errors_name_the_line() {
        let e = bad("graph TD\nA --> B\nA[unclosed --> C");
        assert_eq!(e.line, 3);
        let e = bad("graph TD\nA --> B -->");
        assert_eq!(e.line, 2);
    }
}
