//! Entity relationship diagrams (`erDiagram`), read into the flowchart
//! model and drawn by flow.rs: an entity with attributes is a box of its
//! name over a table (type, name, keys, comment), one without a plain box;
//! relations carry crow's feet at both ends, solid when identifying and
//! dashed when not, in symbols (`||--o{`) or words (`1 to zero or more`);
//! entity aliases, subgraphs, `classDef` / `:::` / `style`.

use crate::flow::{self, Dir, End, Flow, Link, Members, Node, Shape, Stroke, Sub};
use crate::svg::Head;
use crate::{Error, Line, Measure, Style, Svg, err, text};
use std::collections::HashMap;

struct P {
    f: Flow,
    ids: HashMap<String, usize>,
    open: Vec<usize>,
}

/// A two-character end mark, either way round (`|o` or `o|`).
fn mark(s: &str) -> Option<Head> {
    if s.len() != 2 || !s.chars().all(|c| matches!(c, '|' | 'o' | '{' | '}')) { return None }
    let many = s.contains(['{', '}']);
    let zero = s.contains('o');
    if many && s.contains('|') == s.contains('o') { return None }
    Some(match (many, zero) { (true, true) => Head::ZeroMany, (true, false) => Head::Many, (false, true) => Head::ZeroOne, (false, false) => Head::One })
}

/// A cardinality in words.
fn word_mark(s: &str) -> Option<Head> {
    Some(match s.trim() {
        "one or zero" | "zero or one" => Head::ZeroOne,
        "one or more" | "one or many" | "many(1)" | "1+" => Head::Many,
        "zero or more" | "zero or many" | "many(0)" | "0+" => Head::ZeroMany,
        "only one" | "1" | "exactly one" => Head::One,
        _ => return None,
    })
}

/// An entity name and what follows it: `"quoted name"` or a plain word,
/// then maybe `[alias]` and `:::class`.
fn entity(s: &str) -> (&str, Option<String>, Option<&str>, &str) {
    let s = s.trim_start();
    let (name, mut rest) = if let Some(q) = s.strip_prefix('"') {
        match q.find('"') { Some(e) => (&q[..e], &q[e + 1..]), None => (q, "") }
    } else {
        // (`|` marks may follow a name directly: `id1||--||id2`)
        let e = s.find(|c: char| c.is_whitespace() || matches!(c, '[' | '{' | '|' | '}' | ':')).unwrap_or(s.len());
        (&s[..e], &s[e..])
    };
    let mut alias = None;
    if let Some(r) = rest.strip_prefix('[') && let Some(e) = r.find(']') { alias = Some(text::label(&r[..e])); rest = &r[e + 1..]; }
    let mut class = None;
    if let Some(r) = rest.strip_prefix(":::") {
        let e = r.find(|c: char| c.is_whitespace() || matches!(c, '{' | '|' | '}')).unwrap_or(r.len());
        class = Some(&r[..e]);
        rest = &r[e..];
    }
    (name, alias, class, rest)
}

impl P {
    fn scope(&self) -> Option<usize> { self.open.last().copied() }

    fn ent(&mut self, name: &str, alias: Option<String>, class: Option<&str>, no: usize) -> Result<usize, Error> {
        if name.is_empty() { return err(no, "An entity needs a name") }
        let i = match self.ids.get(name) {
            Some(&i) => i,
            None => {
                let i = self.f.nodes.len();
                self.f.nodes.push(Node { id: name.to_string(), label: text::label(name), shape: Shape::Rect, classes: Vec::new(), style: Vec::new(), sub: self.scope(), members: None });
                self.ids.insert(name.to_string(), i);
                i
            }
        };
        if let Some(a) = alias { self.f.nodes[i].label = a; }
        if let Some(c) = class.filter(|c| !c.is_empty()) { self.f.nodes[i].classes.push(c.to_string()); }
        Ok(i)
    }
}

/// An attribute line: type, name, keys (`PK, FK`), `"comment"`.
fn attribute(t: &str) -> [String; 4] {
    let (main, comment) = match t.find('"') { Some(q) => (&t[..q], t[q + 1..].trim_end_matches('"').to_string()), None => (t, String::new()) };
    let mut words = main.split_whitespace();
    let ty = words.next().unwrap_or("").to_string();
    let name = words.next().unwrap_or("").to_string();
    let keys = words.collect::<Vec<_>>().join(" ");
    [ty, name, keys, comment]
}

pub fn parse(lines: &[Line]) -> Result<Flow, Error> {
    let mut p = P { f: Flow { dir: Dir::TB, nodes: Vec::new(), links: Vec::new(), subs: Vec::new(), class_defs: HashMap::new(), default_link: Vec::new() }, ids: HashMap::new(), open: Vec::new() };
    let mut body: Option<usize> = None;
    let mut acc_block = false;
    for l in lines.iter().skip(1) {
        let no = l.no;
        let t = l.text.find("%%").map_or(l.text, |i| l.text[..i].trim());
        if t.is_empty() { continue }
        if acc_block { if t.ends_with('}') { acc_block = false; } continue }
        if let Some(e) = body {
            if t == "}" { body = None; continue }
            let row = attribute(t.trim_end_matches('}'));
            if let Some(mb) = p.f.nodes[e].members.as_deref_mut() { mb.table.push(row); }
            if t.ends_with('}') { body = None; }
            continue
        }
        let word = t.split(|c: char| c.is_whitespace() || c == ':').next().unwrap_or("");
        let rest = t[word.len()..].trim();
        match word {
            "accTitle" | "accDescr" => { acc_block = word == "accDescr" && rest.starts_with('{') && !rest.ends_with('}'); continue }
            "title" | "click" => continue,
            "direction" => { if p.open.is_empty() && let Some(d) = flow::dir_of(rest) { p.f.dir = d; } continue }
            "classDef" => {
                let Some((names, st)) = rest.split_once(char::is_whitespace) else { return err(no, "classDef needs a name and styles") };
                let st = flow::styles(st);
                for n in names.split(',') { p.f.class_defs.insert(n.trim().to_string(), st.clone()); }
                continue
            }
            "class" => {
                let Some((ids, name)) = rest.trim_end_matches(';').rsplit_once(char::is_whitespace) else { return err(no, "class needs entities and a class name") };
                for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) { let i = p.ent(id, None, None, no)?; p.f.nodes[i].classes.push(name.to_string()); }
                continue
            }
            "style" => {
                let Some((id, st)) = rest.split_once(char::is_whitespace) else { return err(no, "style needs an entity and styles") };
                let i = p.ent(id, None, None, no)?;
                p.f.nodes[i].style.extend(flow::styles(st));
                continue
            }
            "subgraph" => {
                let (id, title) = if let Some(q) = rest.strip_prefix('"') { let q = q.trim_end_matches('"'); (q.to_string(), q.to_string()) } else {
                    match rest.split_once('[') { Some((a, b)) => (a.trim().to_string(), text::label(b.trim().trim_end_matches(']'))), None => (rest.to_string(), rest.to_string()) }
                };
                let s = p.f.subs.len();
                p.f.subs.push(Sub { id, title, parent: p.scope(), classes: Vec::new(), style: Vec::new() });
                p.open.push(s);
                continue
            }
            "end" if rest.is_empty() => { if p.open.pop().is_none() { return err(no, "An “end” with no subgraph open") } continue }
            _ => {}
        }
        // a relation in symbols: A ||--o{ B : label
        let (a_name, alias, class, after) = entity(t);
        let after_t = after.trim_start();
        if after_t.len() >= 6 {
            let (m1, line, m2) = (after_t.get(..2).unwrap_or(""), after_t.get(2..4).unwrap_or(""), after_t.get(4..6).unwrap_or(""));
            if matches!(line, "--" | "..") && let (Some(h1), Some(h2)) = (mark(m1), mark(m2)) {
                let a = p.ent(a_name, alias, class, no)?;
                let (b_name, b_alias, b_class, tail) = entity(&after_t[6..]);
                let b = p.ent(b_name, b_alias, b_class, no)?;
                let label = tail.trim().strip_prefix(':').map(|x| text::label(x.trim())).unwrap_or_default();
                p.f.links.push(Link { from: End::Node(a), to: End::Node(b), label, stroke: if line == ".." { Stroke::Dotted } else { Stroke::Normal }, start: h1, end: h2, len: 1, style: Vec::new(), ends: Default::default() });
                continue
            }
        }
        // in words: A 1 to zero or more B : label
        if let Some((lhs, label)) = after.split_once(':') && let Some((left, right, dashed)) = lhs.split_once(" optionally to ").map(|(a, b)| (a, b, true)).or_else(|| lhs.split_once(" to ").map(|(a, b)| (a, b, false))) {
            let left = left.trim();
            let right = right.trim();
            let (Some(h1), Some((card, b_raw))) = (word_mark(left), right.rsplit_once(' ')) else { return err(no, format!("Couldn't read the cardinalities in “{t}”")) };
            let Some(h2) = word_mark(card) else { return err(no, format!("“{card}” isn't a cardinality (only one, zero or one, one or more, zero or more)")) };
            let a = p.ent(a_name, alias, class, no)?;
            let (b_name, b_alias, b_class, _) = entity(b_raw);
            let b = p.ent(b_name, b_alias, b_class, no)?;
            p.f.links.push(Link { from: End::Node(a), to: End::Node(b), label: text::label(label.trim()), stroke: if dashed { Stroke::Dotted } else { Stroke::Normal }, start: h1, end: h2, len: 1, style: Vec::new(), ends: Default::default() });
            continue
        }
        // an entity, maybe with its attributes: A["alias"]:::c {
        let rest = after.trim();
        if !rest.is_empty() && rest != "{" { return err(no, format!("Couldn't read “{t}”")) }
        let e = p.ent(a_name, alias, class, no)?;
        if rest == "{" {
            let nd = &mut p.f.nodes[e];
            nd.shape = Shape::Class;
            nd.members.get_or_insert_with(|| Box::new(Members::default()));
            body = Some(e);
        }
    }
    if body.is_some() { return err(0, "An entity's attributes have no closing “}”") }
    if let Some(&s) = p.open.last() { return err(0, format!("Subgraph “{}” has no “end”", p.f.subs[s].id)) }
    // an entity named like a subgraph is that subgraph
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

pub fn render(lines: &[Line], title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    flow::draw(&parse(lines)?, title, style, m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Flow { let (l, _) = crate::lines(src); parse(&l).unwrap_or_else(|e| panic!("{e}: {src}")) }
    fn bad(src: &str) -> Error { let (l, _) = crate::lines(src); parse(&l).expect_err(src) }

    #[test]
    fn relations_in_symbols_and_words() {
        let f = p("erDiagram\nCUSTOMER ||--o{ ORDER : places\nORDER ||--|{ LINE-ITEM : contains\nA }|..|{ B : uses\nid1||--||id2 : label\nP o{--|| H:::bar : has\nCAR 1 to zero or more DRIVER : allows\nPERSON many(0) optionally to 0+ DRIVER : is\nX |o--o| Y : \"appears in\"");
        let l = &f.links;
        assert_eq!((l[0].start, l[0].end, l[0].label.as_str()), (Head::One, Head::ZeroMany, "places"));
        assert_eq!(l[1].end, Head::Many);
        assert_eq!((l[2].start, l[2].stroke), (Head::Many, Stroke::Dotted));
        assert_eq!((l[3].start, l[3].end), (Head::One, Head::One));
        assert_eq!(l[4].start, Head::ZeroMany);
        assert_eq!((l[5].start, l[5].end, l[5].stroke), (Head::One, Head::ZeroMany, Stroke::Normal));
        assert_eq!((l[6].start, l[6].end, l[6].stroke), (Head::ZeroMany, Head::ZeroMany, Stroke::Dotted));
        assert_eq!((l[7].start, l[7].end, l[7].label.as_str()), (Head::ZeroOne, Head::ZeroOne, "appears in"));
        let ids: Vec<&str> = f.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["CUSTOMER", "ORDER", "LINE-ITEM", "A", "B", "id1", "id2", "P", "H", "CAR", "DRIVER", "PERSON", "X", "Y"]);
        assert_eq!(f.nodes[8].classes, ["bar"]);
    }

    #[test]
    fn attributes_aliases_and_subgraphs() {
        let f = p("erDiagram\np[Person] {\n  string driversLicense PK \"The license #\"\n  string(99) firstName\n  string id PK, FK\n}\n\"This ❤ Unicode\"\nsubgraph g [Group]\n  a[\"Customer Account\"]:::c\nend\np ||--o| g : has");
        assert_eq!(f.nodes[0].label, "Person");
        assert_eq!(f.nodes[0].shape, Shape::Class);
        let t = &f.nodes[0].members.as_deref().unwrap().table;
        assert_eq!(t[0], ["string", "driversLicense", "PK", "The license #"].map(String::from));
        assert_eq!(t[2][2], "PK, FK");
        assert_eq!(f.nodes[1].label, "This ❤ Unicode");
        assert_eq!((f.nodes[2].label.as_str(), f.nodes[2].sub), ("Customer Account", Some(0)));
        assert_eq!(f.subs[0].title, "Group");
        assert_eq!(f.links[0].to, End::Sub(0));
    }

    #[test]
    fn mistakes_say_where() {
        assert_eq!(bad("erDiagram\nA {\n string x").line, 0);
        assert_eq!(bad("erDiagram\nA B C").line, 2);
        assert_eq!(bad("erDiagram\nA 1 to lots B : x").line, 2);
        assert_eq!(bad("erDiagram\nsubgraph s\nA").line, 0);
        assert_eq!(bad("erDiagram\nend").line, 2);
        assert_eq!(p("erDiagram\nCAR:::someclass {\n  string x\n}\nPERSON:::foo ||--|| CAR : owns").nodes[0].classes, ["someclass"]);
    }
}
