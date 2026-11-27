//! Class diagrams (`classDiagram`), read into the flowchart model and drawn
//! by flow.rs: classes are boxes with their annotations and name, then
//! attributes and methods; relations of every head (inheritance,
//! composition, aggregation, association, dependency, realization,
//! lollipop) with labels and multiplicities; namespaces as subgraphs;
//! notes tied to their class; `classDef` / `cssClass` / `:::` / `style`.

use crate::flow::{self, Dir, End, Flow, Link, Node, Shape, Stroke, Sub};
use crate::svg::Head;
use crate::{Error, Line, Measure, Style, Svg, err, text};
use std::collections::HashMap;

struct P {
    f: Flow,
    ids: HashMap<String, usize>,
    open: Vec<usize>,
    notes: usize,
}

/// `List~int~` as `List<int>`: a `~` with a name before it and one after
/// opens, any other closes.
fn generics(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in cs.iter().enumerate() {
        if c != '~' { out.push(c); continue }
        let before = i > 0 && cs[i - 1].is_alphanumeric();
        let after = cs.get(i + 1).is_some_and(|c| c.is_alphanumeric() || *c == '_');
        out.push(if before && after { '<' } else { '>' });
    }
    out
}

/// A member as shown: generics bracketed, a method's return type after a
/// colon, the static / abstract marks (`$`, `*`) off.
fn member(raw: &str) -> String {
    let t = raw.trim().trim_end_matches(';');
    let t = if t.contains(')') { t.trim_end_matches(['$', '*']) } else { t.trim_end_matches('$') };
    if let Some(close) = t.rfind(')') && t.contains('(') {
        let (sig, ret) = (&t[..=close], t[close + 1..].trim().trim_start_matches(['$', '*']).trim());
        let sig = sig.trim_end_matches(['$', '*']);
        return if ret.is_empty() { generics(sig) } else { format!("{} : {}", generics(sig), generics(ret)) };
    }
    generics(t)
}

impl P {
    fn scope(&self) -> Option<usize> { self.open.last().copied() }

    /// The class `raw` names (backticks off, `~T~` taken as part of its
    /// shown name, `:::style` taken), made on first mention.
    fn class(&mut self, raw: &str, no: usize) -> Result<usize, Error> {
        let raw = raw.trim();
        let (raw, css) = match raw.split_once(":::") { Some((a, b)) => (a.trim(), Some(b.trim())), None => (raw, None) };
        let (id, shown) = match raw.find('~') { Some(i) => (raw[..i].trim(), generics(raw)), None => (raw, raw.to_string()) };
        let id = id.trim_matches('`');
        let shown = shown.trim_matches('`').to_string();
        if id.is_empty() { return err(no, "A relation needs a class at each end") }
        if id.contains(char::is_whitespace) && !raw.starts_with('`') { return err(no, format!("“{id}” isn't a class name (no spaces)")) }
        let i = match self.ids.get(id) {
            Some(&i) => i,
            None => {
                let i = self.f.nodes.len();
                self.f.nodes.push(Node { id: id.to_string(), label: shown, shape: Shape::Class, classes: Vec::new(), style: Vec::new(), sub: self.scope(), members: Some(Box::default()) });
                self.ids.insert(id.to_string(), i);
                i
            }
        };
        if let Some(c) = css.filter(|c| !c.is_empty()) { self.f.nodes[i].classes.push(c.to_string()); }
        Ok(i)
    }

    fn add(&mut self, i: usize, raw: &str) {
        let t = raw.trim();
        if t.is_empty() { return }
        let Some(mb) = self.f.nodes[i].members.as_deref_mut() else { return };
        if let Some(a) = t.strip_prefix("<<").and_then(|a| a.strip_suffix(">>")) { mb.notes.push(a.trim().to_string()); }
        else if t.contains('(') { mb.methods.push(member(t)); } else { mb.attrs.push(member(t)); }
    }

    fn note(&mut self, target: Option<&str>, raw: &str, no: usize) -> Result<(), Error> {
        let t = raw.trim();
        let body = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
        let i = self.f.nodes.len();
        self.f.nodes.push(Node { id: format!("note\0{}", self.notes), label: text::label(body), shape: Shape::Rect, classes: Vec::new(), style: Vec::new(), sub: self.scope(), members: None });
        self.notes += 1;
        if let Some(target) = target {
            let c = self.class(target, no)?;
            self.f.nodes[i].sub = self.f.nodes[c].sub;
            self.f.links.push(Link { from: End::Node(i), to: End::Node(c), label: String::new(), stroke: Stroke::Dotted, start: Head::None, end: Head::None, len: 1, style: Vec::new(), ends: Default::default() });
        }
        Ok(())
    }
}

/// A relation's line (`--` or `..`) in `t`, outside quotes: its byte range
/// with the heads on both sides.
fn relation(t: &str) -> Option<(usize, usize, Head, Head, bool)> {
    let b = t.as_bytes();
    let mut quoted = false;
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'"' { quoted = !quoted; }
        if !quoted && ((b[i] == b'-' && b[i + 1] == b'-') || (b[i] == b'.' && b[i + 1] == b'.')) {
            let dashed = b[i] == b'.';
            let (mut s, mut e) = (i, i + 2);
            let left = |s: usize, w: &str| t[..s].ends_with(w);
            let start = if left(s, "<|") { s -= 2; Head::Triangle } else if left(s, "()") { s -= 2; Head::Circle } else if left(s, "*") { s -= 1; Head::FilledDiamond } else if left(s, "o") && t[..s - 1].ends_with(' ') { s -= 1; Head::Diamond } else if left(s, "<") { s -= 1; Head::Arrow } else { Head::None };
            let right = |e: usize, w: &str| t[e..].starts_with(w);
            let end = if right(e, "|>") { e += 2; Head::Triangle } else if right(e, "()") { e += 2; Head::Circle } else if right(e, "*") { e += 1; Head::FilledDiamond } else if right(e, "o") && t[e + 1..].starts_with(' ') { e += 1; Head::Diamond } else if right(e, ">") { e += 1; Head::Arrow } else { Head::None };
            return Some((s, e, start, end, dashed));
        }
        i += 1;
    }
    None
}

/// `"1" ` before or ` "many"` after a class: the class and the quoted text.
fn split_card(s: &str, before: bool) -> (String, String) {
    let s = s.trim();
    if before {
        if let Some(r) = s.strip_suffix('"') && let Some(q) = r.rfind('"') { return (s[..q].trim().to_string(), r[q + 1..].to_string()) }
    } else if let Some(r) = s.strip_prefix('"') && let Some(q) = r.find('"') { return (r[q + 1..].trim().to_string(), r[..q].to_string()) }
    (s.to_string(), String::new())
}

pub fn parse(lines: &[Line]) -> Result<Flow, Error> {
    let mut p = P { f: Flow { dir: Dir::TB, nodes: Vec::new(), links: Vec::new(), subs: Vec::new(), class_defs: HashMap::new(), default_link: Vec::new() }, ids: HashMap::new(), open: Vec::new(), notes: 0 };
    // the class whose `{ … }` body is being read
    let mut body: Option<usize> = None;
    let mut acc_block = false;
    for l in lines.iter().skip(1) {
        let no = l.no;
        let t = l.text.find("%%").map_or(l.text, |i| l.text[..i].trim());
        if t.is_empty() { continue }
        if acc_block { if t.ends_with('}') { acc_block = false; } continue }
        if let Some(c) = body {
            if t == "}" { body = None; } else {
                // `+name }` closes too
                let (m, done) = match t.strip_suffix('}') { Some(x) => (x, true), None => (t, false) };
                p.add(c, m);
                if done { body = None; }
            }
            continue
        }
        let word = t.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
        let rest = t[word.len()..].trim();
        match word {
            "accTitle" | "accDescr" => { acc_block = word == "accDescr" && rest.starts_with('{') && !rest.ends_with('}'); continue }
            "click" | "link" | "callback" | "title" => continue,
            "direction" => { if let Some(d) = flow::dir_of(rest) { p.f.dir = d; } continue }
            "classDef" => {
                let Some((names, st)) = rest.split_once(char::is_whitespace) else { return err(no, "classDef needs a name and styles") };
                let st = flow::styles(st);
                for n in names.split(',') { p.f.class_defs.insert(n.trim().to_string(), st.clone()); }
                continue
            }
            "cssClass" => {
                let Some((ids, name)) = rest.trim_end_matches(';').rsplit_once(char::is_whitespace) else { return err(no, "cssClass needs classes and a style class") };
                for id in ids.trim_matches('"').split(',').map(str::trim).filter(|s| !s.is_empty()) { let i = p.class(id, no)?; p.f.nodes[i].classes.push(name.to_string()); }
                continue
            }
            "style" => {
                let Some((id, st)) = rest.split_once(char::is_whitespace) else { return err(no, "style needs a class and styles") };
                let i = p.class(id, no)?;
                p.f.nodes[i].style.extend(flow::styles(st));
                continue
            }
            "note" => {
                if let Some(r) = rest.strip_prefix("for ") {
                    let r = r.trim();
                    let Some(q) = r.find('"') else { return err(no, "Write a note as “note for Class \"text\"”") };
                    p.note(Some(&r[..q]), &r[q..], no)?;
                } else { p.note(None, rest, no)?; }
                continue
            }
            "namespace" => {
                let Some(head) = rest.strip_suffix('{') else { return err(no, "A namespace opens with “{”") };
                let head = head.trim();
                let (id, title) = match head.split_once('[') { Some((a, b)) => (a.trim(), text::label(b.trim_end_matches(']'))), None => (head, head.to_string()) };
                let s = p.f.subs.len();
                p.f.subs.push(Sub { id: id.to_string(), title, parent: p.scope(), classes: Vec::new(), style: Vec::new() });
                p.open.push(s);
                continue
            }
            _ => {}
        }
        if t == "}" {
            if p.open.pop().is_none() { return err(no, "A “}” with no namespace open") }
            continue
        }
        if word == "class" {
            // class Name~T~["label"]:::css <<annotation>> {
            let mut r = rest;
            let open = r.ends_with('{');
            r = r.trim_end_matches('{').trim();
            let mut note = None;
            if let Some(a) = r.find("<<") { note = Some(r[a..].trim_start_matches('<').trim_end_matches('>').trim().to_string()); r = r[..a].trim(); }
            // (`Name["label"]:::css`: the style class after the label)
            let mut css = "";
            if let Some(k) = r.find("]:::") { css = &r[k + 4..]; r = &r[..=k]; }
            let mut label = None;
            if let Some(a) = r.find('[') { label = Some(text::label(r[a + 1..].trim_end_matches(']'))); r = r[..a].trim(); }
            let i = p.class(r, no)?;
            if !css.trim().is_empty() { p.f.nodes[i].classes.push(css.trim().to_string()); }
            if let Some(lb) = label { p.f.nodes[i].label = lb; }
            if let (Some(n), Some(mb)) = (note, p.f.nodes[i].members.as_deref_mut()) { mb.notes.push(n); }
            if open { body = Some(i); }
            continue
        }
        if let Some(r) = t.strip_prefix("<<") {
            let Some((a, cls)) = r.split_once(">>") else { return err(no, "An annotation is written “<<interface>> Class”") };
            let i = p.class(cls, no)?;
            if let Some(mb) = p.f.nodes[i].members.as_deref_mut() { mb.notes.push(a.trim().to_string()); }
            continue
        }
        if let Some((s, e, start, end, dashed)) = relation(t) {
            let (a, card_a) = split_card(&t[..s], true);
            let (b, label) = match t[e..].split_once(':') { Some((b, lb)) => (b, text::label(lb)), None => (&t[e..], String::new()) };
            let (b, card_b) = split_card(b, false);
            let (x, y) = (p.class(&a, no)?, p.class(&b, no)?);
            p.f.links.push(Link { from: End::Node(x), to: End::Node(y), label, stroke: if dashed { Stroke::Dotted } else { Stroke::Normal }, start, end, len: 1, style: Vec::new(), ends: [card_a, card_b] });
            continue
        }
        // Class : member
        if let Some((c, m)) = t.split_once(':').filter(|(c, _)| !c.trim().is_empty() && !c.contains(":::")) {
            let i = p.class(c, no)?;
            p.add(i, m);
            continue
        }
        if t.contains(char::is_whitespace) && !t.starts_with('`') { return err(no, format!("Couldn't read “{t}”")) }
        p.class(t, no)?;
    }
    if body.is_some() { return err(0, "A class body has no closing “}”") }
    if let Some(&s) = p.open.last() { return err(0, format!("Namespace “{}” has no closing “}}”", p.f.subs[s].id)) }
    Ok(p.f)
}

pub fn render(lines: &[Line], title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let mut f = parse(lines)?;
    let c = &style.colors;
    for nd in &mut f.nodes {
        if nd.shape == Shape::Rect { nd.style.splice(0..0, [("fill".to_string(), c.note_fill.clone()), ("stroke".to_string(), c.note_border.clone())]); }
    }
    flow::draw(&f, title, style, m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::Members;

    fn p(src: &str) -> Flow { let (l, _) = crate::lines(src); parse(&l).unwrap_or_else(|e| panic!("{e}: {src}")) }
    fn bad(src: &str) -> Error { let (l, _) = crate::lines(src); parse(&l).expect_err(src) }
    fn mb(f: &Flow, i: usize) -> &Members { f.nodes[i].members.as_deref().unwrap() }

    #[test]
    fn members_from_bodies_and_lines() {
        let f = p("classDiagram\nclass Square~Shape~{\n  int id\n  <<interface>>\n  getPoints() List~int~\n  +area()$ double\n}\nSquare : +getMatrix() List~List~int~~\nSquare : -List~string~ messages\nclass Shape <<abstract>>");
        assert_eq!(f.nodes[0].label, "Square<Shape>");
        assert_eq!(mb(&f, 0).notes, ["interface"]);
        assert_eq!(mb(&f, 0).attrs, ["int id", "-List<string> messages"]);
        assert_eq!(mb(&f, 0).methods, ["getPoints() : List<int>", "+area() : double", "+getMatrix() : List<List<int>>"]);
        assert_eq!(mb(&f, 1).notes, ["abstract"]);
    }

    #[test]
    fn relations_and_their_heads() {
        let f = p("classDiagram\nA <|-- B\nC *-- D\nE o-- F\nG <-- H\nI -- J\nK <.. L\nM <|.. N\nO .. P\nQ --|> R : is\nS ()-- T\nU \"1\" --> \"*\" V : places\nW <|--|> X");
        let l = &f.links;
        assert_eq!((l[0].start, l[0].end, l[0].stroke), (Head::Triangle, Head::None, Stroke::Normal));
        assert_eq!(l[1].start, Head::FilledDiamond);
        assert_eq!(l[2].start, Head::Diamond);
        assert_eq!(l[3].start, Head::Arrow);
        assert_eq!((l[4].start, l[4].end), (Head::None, Head::None));
        assert_eq!((l[5].start, l[5].stroke), (Head::Arrow, Stroke::Dotted));
        assert_eq!((l[6].start, l[6].stroke), (Head::Triangle, Stroke::Dotted));
        assert_eq!((l[8].end, l[8].label.as_str()), (Head::Triangle, "is"));
        assert_eq!(l[9].start, Head::Circle);
        assert_eq!((l[10].ends.clone(), l[10].label.as_str()), (["1".to_string(), "*".to_string()], "places"));
        assert_eq!((l[11].start, l[11].end), (Head::Triangle, Head::Triangle));
        assert_eq!(f.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>()[..4], ["A", "B", "C", "D"]);
    }

    #[test]
    fn namespaces_notes_and_styles() {
        let f = p("classDiagram\nnamespace Auth[\"Auth service\"] {\n  class User {\n    +login()\n  }\n}\nnote for User \"logs in\"\nnote \"general\"\nclass `Animal Class!`\nclass Car[\"A car\"]:::hot\nclassDef hot fill:#f96\ncssClass \"User,Car\" cold\nstyle Car stroke:#333");
        assert_eq!(f.subs[0].title, "Auth service");
        assert_eq!(f.nodes[0].sub, Some(0));
        assert_eq!(f.nodes[1].label, "logs in");
        assert_eq!(f.links.len(), 1);
        assert_eq!(f.nodes[3].label, "Animal Class!");
        assert_eq!(f.nodes[4].label, "A car");
        assert_eq!(f.nodes[4].classes, ["hot", "cold"]);
        assert_eq!(f.nodes[4].style.len(), 1);
    }

    #[test]
    fn mistakes_say_where() {
        assert_eq!(bad("classDiagram\nclass A {\n x").line, 0);
        assert_eq!(bad("classDiagram\nnamespace N {\nclass A").line, 0);
        assert_eq!(bad("classDiagram\nA B C").line, 2);
        assert_eq!(bad("classDiagram\nA --> B\n}").line, 3);
        assert_eq!(bad("classDiagram\n<<interface A").line, 2);
    }
}
