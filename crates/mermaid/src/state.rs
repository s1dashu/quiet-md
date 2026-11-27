//! State diagrams (`stateDiagram` / `stateDiagram-v2`), read into the
//! flowchart model and drawn by flow.rs: states are rounded boxes, `[*]`
//! the start dot or end bullseye of its scope, composite states and their
//! `--` regions subgraphs, `<<choice>>` a small diamond, `<<fork>>` /
//! `<<join>>` bars, notes yellow boxes tied to their state by a dotted line.

use crate::flow::{self, Dir, End, Flow, Link, Node, Shape, Stroke, Sub};
use crate::svg::Head;
use crate::{Error, Line, Measure, Style, Svg, err, text};
use std::collections::HashMap;

/// A state as mentioned: its id (start, end and note stand-ins get ids
/// with a NUL in them, which no source can write), description lines.
struct St { id: String, desc: Vec<String>, shape: Shape, sub: Option<usize>, classes: Vec<String> }

struct P {
    dir: Dir,
    states: Vec<St>,
    ids: HashMap<String, usize>,
    subs: Vec<Sub>,
    /// Which subs are `--` regions (drawn dashed, untitled).
    region: Vec<bool>,
    /// A composite state's id → its sub.
    comp: HashMap<String, usize>,
    /// Open scopes, innermost last.
    open: Vec<usize>,
    links: Vec<(usize, usize, String, bool)>,
    class_defs: HashMap<String, Vec<(String, String)>>,
    notes: usize,
}

/// `s` up to its first `:` that isn't part of a `:::class`, and the rest.
fn split_colon(s: &str) -> (&str, Option<&str>) {
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(":::") { i += 3; continue }
        if s[i..].starts_with(':') { return (s[..i].trim(), Some(s[i + 1..].trim())) }
        i += s[i..].chars().next().map_or(1, char::len_utf8);
    }
    (s.trim(), None)
}

impl P {
    fn scope(&self) -> Option<usize> { self.open.last().copied() }

    /// The state `id` (`[*]` is the start or, `end`, the end of the scope),
    /// made on first mention; a `:::class` after it is taken.
    fn state(&mut self, raw: &str, end: bool, no: usize) -> Result<usize, Error> {
        let (id, class) = match raw.split_once(":::") { Some((a, b)) => (a.trim(), Some(b.trim())), None => (raw.trim(), None) };
        if id.is_empty() { return err(no, "A transition needs a state at each end") }
        if id.contains(char::is_whitespace) { return err(no, format!("“{id}” isn't a state id (no spaces)")) }
        let (key, shape) = if id == "[*]" { (format!("{}\0{:?}", if end { "end" } else { "start" }, self.scope()), if end { Shape::FrDot } else { Shape::FDot }) } else { (id.to_string(), Shape::Round) };
        let i = match self.ids.get(&key) {
            Some(&i) => i,
            None => {
                let i = self.states.len();
                self.states.push(St { id: key.clone(), desc: Vec::new(), shape, sub: self.scope(), classes: Vec::new() });
                self.ids.insert(key, i);
                i
            }
        };
        if let Some(c) = class.filter(|c| !c.is_empty()) { self.states[i].classes.push(c.to_string()); }
        Ok(i)
    }

    fn note(&mut self, side: &str, target: &str, text: String, no: usize) -> Result<(), Error> {
        let t = self.state(target, false, no)?;
        let i = self.states.len();
        self.states.push(St { id: format!("note\0{}", self.notes), desc: vec![text], shape: Shape::Rect, sub: self.states[t].sub, classes: Vec::new() });
        self.notes += 1;
        // the layout has no "beside": a left note goes before its state,
        // a right one after it
        if side == "left" { self.links.push((i, t, String::new(), true)); } else { self.links.push((t, i, String::new(), true)); }
        Ok(())
    }

    fn open_sub(&mut self, id: &str, region: bool) -> usize {
        let s = self.subs.len();
        self.subs.push(Sub { id: id.to_string(), title: String::new(), parent: self.scope(), classes: Vec::new(), style: Vec::new() });
        self.region.push(region);
        self.open.push(s);
        s
    }
}

pub fn parse(lines: &[Line]) -> Result<Flow, Error> {
    let mut p = P { dir: Dir::TB, states: Vec::new(), ids: HashMap::new(), subs: Vec::new(), region: Vec::new(), comp: HashMap::new(), open: Vec::new(), links: Vec::new(), class_defs: HashMap::new(), notes: 0 };
    // a multi-line note being read: side, target, its lines, where it began
    let mut note: Option<(String, String, Vec<String>, usize)> = None;
    let mut acc_block = false;
    for l in lines.iter().skip(1) {
        let no = l.no;
        // `%%` comments after a statement
        let t = l.text.find("%%").map_or(l.text, |i| l.text[..i].trim());
        if t.is_empty() { continue }
        if let Some((side, target, body, _)) = &mut note {
            if t == "end note" || t == "endnote" {
                let (side, target, body) = (side.clone(), target.clone(), body.join("\n"));
                note = None;
                p.note(&side, &target, body, no)?;
            } else { body.push(text::label(t)); }
            continue
        }
        if acc_block { if t.ends_with('}') { acc_block = false; } continue }
        let word = t.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
        let rest = t[word.len()..].trim();
        match word {
            "accTitle" | "accDescr" => { if word == "accDescr" && rest.starts_with('{') && !rest.ends_with('}') { acc_block = true; } continue }
            "hide" | "scale" | "click" | "title" => continue,
            "direction" => {
                if p.open.is_empty() && let Some(d) = flow::dir_of(rest) { p.dir = d; }
                continue
            }
            "classDef" => {
                let Some((names, st)) = rest.split_once(char::is_whitespace) else { return err(no, "classDef needs a name and styles") };
                let st = flow::styles(st);
                for n in names.split(',') { p.class_defs.insert(n.trim().to_string(), st.clone()); }
                continue
            }
            "class" => {
                let Some((ids, name)) = rest.trim_end_matches(';').rsplit_once(char::is_whitespace) else { return err(no, "class needs states and a class name") };
                for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    if let Some(&i) = p.ids.get(id) { p.states[i].classes.push(name.to_string()); }
                    if let Some(&s) = p.comp.get(id) { p.subs[s].classes.push(name.to_string()); }
                }
                continue
            }
            "style" => {
                let Some((id, st)) = rest.split_once(char::is_whitespace) else { return err(no, "style needs a state and styles") };
                let st = flow::styles(st);
                if let Some(&s) = p.comp.get(id) { p.subs[s].style.extend(st); } else {
                    let i = p.state(id, false, no)?;
                    // (kept as a class of its own)
                    let name = format!("style\0{i}");
                    p.class_defs.entry(name.clone()).or_default().extend(st);
                    p.states[i].classes.push(name);
                }
                continue
            }
            "note" => {
                let mut it = rest.splitn(3, char::is_whitespace);
                let (side, of, tail) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or("").trim());
                // `note "text" as N`: a floating note, not drawn
                if rest.starts_with('"') { continue }
                if !matches!(side, "left" | "right") || of != "of" { return err(no, "Write a note as “note left of State” or “note right of State”") }
                let (target, text) = split_colon(tail);
                match text { Some(x) => p.note(side, target, text::label(x), no)?, None => note = Some((side.into(), target.into(), Vec::new(), no)) }
                continue
            }
            _ => {}
        }
        if t == "}" {
            if p.scope().is_some_and(|s| p.region[s]) { p.open.pop(); }
            if p.open.pop().is_none() { return err(no, "A “}” with no state open") }
            continue
        }
        if t == "--" {
            let Some(s) = p.scope() else { return err(no, "“--” splits a composite state: write it inside one") };
            if p.region[s] { p.open.pop(); } else {
                // what is in the composite so far is its first region
                let id = p.subs[s].id.clone();
                let r = p.open_sub(&format!("{id}\0r0"), true);
                for st in &mut p.states { if st.sub == Some(s) { st.sub = Some(r); } }
                for k in 0..r { if p.subs[k].parent == Some(s) { p.subs[k].parent = Some(r); } }
                p.open.pop();
            }
            let c = p.scope().unwrap_or(s);
            let n = p.subs.iter().filter(|x| x.parent == Some(c)).count();
            let id = p.subs[c].id.clone();
            p.open_sub(&format!("{id}\0r{n}"), true);
            continue
        }
        if word == "state" {
            // state "desc" as id, state id, then `{`, `<<kind>>` or `: desc`
            let mut rest = rest;
            let mut desc = None;
            if let Some(q) = rest.strip_prefix('"') {
                let Some(end) = q.find('"') else { return err(no, "A state's description is missing its closing quote") };
                desc = Some(text::label(&q[..end]));
                rest = q[end + 1..].trim_start();
                let Some(r) = rest.strip_prefix("as") else { return err(no, "Write “state \"description\" as id”") };
                rest = r.trim_start();
            }
            let open = rest.ends_with('{');
            let rest = rest.trim_end_matches('{').trim();
            let (head, after) = split_colon(rest);
            let (id, kind) = match head.split_once("<<") { Some((a, b)) => (a.trim(), Some(b.trim_end_matches(">>").trim())), None => (head, None) };
            let i = p.state(id, false, no)?;
            if let Some(d) = desc { p.states[i].desc.push(d); }
            if let Some(d) = after.filter(|d| !d.is_empty()) { p.states[i].desc.push(text::label(d)); }
            match kind {
                Some("choice") => p.states[i].shape = Shape::Rhombus,
                Some("fork") | Some("join") => p.states[i].shape = Shape::Fork,
                _ => {}
            }
            if open {
                let id = p.states[i].id.clone();
                if p.comp.contains_key(&id) { return err(no, format!("State “{id}” is opened twice")) }
                let s = p.open_sub(&id, false);
                p.comp.insert(id, s);
            }
            continue
        }
        if let Some(at) = t.find("-->") {
            let (a, b) = (&t[..at], &t[at + 3..]);
            let (b, label) = split_colon(b);
            let from = p.state(a, false, no)?;
            let to = p.state(b, true, no)?;
            p.links.push((from, to, label.map(text::label).unwrap_or_default(), false));
            continue
        }
        // `id`, `id : description`, `id:::class`
        let (id, desc) = split_colon(t);
        let i = p.state(id, false, no)?;
        if let Some(d) = desc.filter(|d| !d.is_empty()) { p.states[i].desc.push(text::label(d)); }
    }
    if let Some((_, target, _, no)) = note { return err(no, format!("The note on “{target}” has no “end note”")) }
    if let Some(&s) = p.open.iter().find(|&&s| !p.region[s]) { return err(0, format!("State “{}” has no closing “}}”", p.subs[s].id)) }

    // to the flowchart model: composite states are their subgraphs
    let mut f = Flow { dir: p.dir, nodes: Vec::new(), links: Vec::new(), subs: p.subs, class_defs: p.class_defs, default_link: Vec::new() };
    let mut end_of: Vec<Option<End>> = Vec::with_capacity(p.states.len());
    for st in p.states {
        if let Some(&s) = p.comp.get(&st.id) {
            let sub = &mut f.subs[s];
            sub.title = if st.desc.is_empty() { st.id } else { st.desc.join("\n") };
            sub.classes.extend(st.classes);
            end_of.push(Some(End::Sub(s)));
            continue
        }
        end_of.push(Some(End::Node(f.nodes.len())));
        let label = match st.shape { Shape::Round if st.desc.is_empty() => st.id.clone(), Shape::Round | Shape::Rect => st.desc.join("\n"), _ => String::new() };
        f.nodes.push(Node { id: st.id, label, shape: st.shape, classes: st.classes, style: Vec::new(), sub: st.sub, members: None });
    }
    for (a, b, label, dotted) in p.links {
        let (Some(Some(from)), Some(Some(to))) = (end_of.get(a).copied(), end_of.get(b).copied()) else { continue };
        f.links.push(Link { from, to, label, stroke: if dotted { Stroke::Dotted } else { Stroke::Normal }, start: Head::None, end: if dotted { Head::None } else { Head::Arrow }, len: 1, style: Vec::new(), ends: Default::default() });
    }
    Ok(f)
}

pub fn render(lines: &[Line], title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let mut f = parse(lines)?;
    let c = &style.colors;
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    for nd in &mut f.nodes {
        nd.style = match nd.shape {
            Shape::FDot | Shape::Fork => vec![pair("stroke", &c.text)],
            Shape::FrDot => vec![pair("fill", &c.background), pair("stroke", &c.text)],
            Shape::Rect => vec![pair("fill", &c.note_fill), pair("stroke", &c.note_border)],
            _ => continue,
        };
    }
    let regions: Vec<usize> = f.subs.iter().enumerate().filter(|(_, s)| s.id.contains("\0r")).map(|(i, _)| i).collect();
    for s in regions { f.subs[s].style = vec![pair("fill", "none"), pair("stroke-dasharray", "4 4")]; }
    flow::draw(&f, title, style, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Flow { let (l, _) = crate::lines(src); parse(&l).unwrap_or_else(|e| panic!("{e}: {src}")) }
    fn bad(src: &str) -> Error { let (l, _) = crate::lines(src); parse(&l).expect_err(src) }
    fn labels(f: &Flow) -> Vec<&str> { f.nodes.iter().map(|n| n.label.as_str()).collect() }

    #[test]
    fn start_and_end_are_one_per_scope() {
        let f = p("stateDiagram-v2\n[*] --> A\n[*] --> B\nA --> [*]\nB --> [*]\nstate C {\n[*] --> c\nc --> [*]\n}\nA --> C");
        let shapes: Vec<Shape> = f.nodes.iter().map(|n| n.shape).collect();
        assert_eq!(shapes.iter().filter(|&&s| s == Shape::FDot).count(), 2);
        assert_eq!(shapes.iter().filter(|&&s| s == Shape::FrDot).count(), 2);
        assert_eq!(f.subs.len(), 1);
        assert_eq!(f.subs[0].title, "C");
        assert!(f.links.iter().any(|l| l.to == End::Sub(0)));
        assert_eq!(f.nodes.iter().filter(|n| n.sub == Some(0)).count(), 3);
    }

    #[test]
    fn descriptions_and_kinds() {
        let f = p("stateDiagram\nstate \"Long name\" as s2\ns3 : Third <br> state\nstate c <<choice>>\nstate j <<join>>\ns2 --> s3: go: now\nN:::hot");
        assert_eq!(labels(&f), ["Long name", "Third \n state", "", "", "N"]);
        assert_eq!(f.nodes[2].shape, Shape::Rhombus);
        assert_eq!(f.nodes[3].shape, Shape::Fork);
        assert_eq!(f.links[0].label, "go: now");
        assert_eq!(f.nodes[4].classes, ["hot"]);
    }

    #[test]
    fn a_composite_mentioned_first_becomes_its_box() {
        let f = p("stateDiagram-v2\nA --> Review\nstate Review {\nx --> y\n}\nReview --> B");
        assert_eq!(labels(&f), ["A", "x", "y", "B"]);
        assert_eq!(f.links[0].to, End::Sub(0));
        assert_eq!(f.links[2].from, End::Sub(0));
    }

    #[test]
    fn regions_split_a_composite() {
        let f = p("stateDiagram-v2\nstate Active {\n[*] --> a\n--\n[*] --> b\n--\n[*] --> c\n}");
        assert_eq!(f.subs.len(), 4);
        let regions: Vec<usize> = (1..4).filter(|&s| f.subs[s].parent == Some(0)).collect();
        assert_eq!(regions.len(), 3);
        // each region its own start
        assert_eq!(f.nodes.iter().filter(|n| n.shape == Shape::FDot).count(), 3);
        assert!(f.nodes.iter().all(|n| n.sub.is_some_and(|s| s > 0)));
    }

    #[test]
    fn notes_and_classes() {
        let f = p("stateDiagram-v2\nclassDef bad fill:#f00\nS1: The state\nnote right of S1\n  Important\n  info\nend note\nnote left of S2 : left\nclass S1, S2 bad\nS1 --> S2 %% comment");
        assert_eq!(labels(&f), ["The state", "Important\ninfo", "S2", "left"]);
        assert_eq!(f.links.len(), 3);
        assert_eq!(f.links[0].stroke, Stroke::Dotted);
        assert_eq!(f.nodes[2].classes, ["bad"]);
    }

    #[test]
    fn mistakes_say_where() {
        assert_eq!(bad("stateDiagram\nstate A {\nx --> y").line, 0);
        assert_eq!(bad("stateDiagram\nA --> B\n}").line, 3);
        assert_eq!(bad("stateDiagram\nnote over A: x").line, 2);
        assert_eq!(bad("stateDiagram\nA B C --> D").line, 2);
        assert_eq!(bad("stateDiagram\nnote left of A\nx").line, 2);
    }
}
