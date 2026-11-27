//! Mind maps (`mindmap`): a tree by indentation, drawn from the root in
//! the middle with its branches shared out to the right and the left (the
//! side with less height so far takes the next), each subtree stacked
//! tidily; every branch in a colour of its own (`Palette::series`), nodes
//! in Mermaid's shapes (square, rounded, circle, bang, cloud, hexagon, the
//! default rounded box). Icons (`::icon`) and classes (`:::`) are skipped.

use crate::svg::{Anchor, Doc, Paint, n};
use crate::text::{self, Block};
use crate::{Error, Line, Measure, Style, Svg, err};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape { Default, Square, Rounded, Circle, Bang, Cloud, Hexagon }

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub label: String,
    pub shape: Shape,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// `id[text]`, `(text)`, `((text))`, `))text((`, `)text(`, `{{text}}` or
/// plain text.
fn node(t: &str) -> (String, Shape) {
    const OPEN: [(&str, &str, Shape); 6] = [("((", "))", Shape::Circle), ("))", "((", Shape::Bang), ("{{", "}}", Shape::Hexagon), ("(", ")", Shape::Rounded), (")", "(", Shape::Cloud), ("[", "]", Shape::Square)];
    // the first bracket after an id (an id is a word: no spaces)
    let at = t.find(['(', ')', '[', '{']).filter(|&i| !t[..i].contains(char::is_whitespace));
    if let Some(i) = at {
        let rest = &t[i..];
        for (o, c, shape) in OPEN {
            if let Some(inner) = rest.strip_prefix(o).and_then(|r| r.strip_suffix(c)) { return (text::label(inner), shape) }
        }
    }
    (text::label(t), Shape::Default)
}

/// The tree in `lines`, each node's indent read from `src`.
pub fn parse(lines: &[Line], src: &str) -> Result<Vec<Node>, Error> {
    let raw: Vec<&str> = src.lines().collect();
    let indent = |no: usize| raw.get(no.wrapping_sub(1)).map_or(0, |l| l.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>());
    let mut nodes: Vec<Node> = Vec::new();
    // (indent, node) of the open path, outermost first
    let mut path: Vec<(usize, usize)> = Vec::new();
    let mut i = 1;
    while i < lines.len() {
        let l = lines[i];
        i += 1;
        let t = l.text;
        if t.starts_with("::icon(") || t.starts_with(":::") { continue }
        // a quoted label may run over lines (markdown strings)
        let mut text = t.to_string();
        while text.matches('"').count() % 2 == 1 && i < lines.len() { text.push('\n'); text.push_str(lines[i].text); i += 1; }
        let ind = indent(l.no);
        let (label, shape) = node(&text);
        if label.trim().is_empty() { return err(l.no, "A node needs some text") }
        while path.last().is_some_and(|&(d, _)| d >= ind) && path.len() > 1 { path.pop(); }
        // (a second root: under the first, as Mermaid's lenient cousins do)
        let parent = path.last().map(|&(_, p)| p);
        // (laid out recursively: a sane depth keeps the stack whole)
        if path.len() > 200 { return err(l.no, "The mind map is nested too deep") }
        let k = nodes.len();
        nodes.push(Node { label, shape, parent, children: Vec::new() });
        if let Some(p) = parent { nodes[p].children.push(k); }
        path.push((ind, k));
    }
    if nodes.is_empty() { return err(0, "A mind map needs a root") }
    Ok(nodes)
}

const H_GAP: f32 = 56.;
const V_GAP: f32 = 14.;

fn size(shape: Shape, b: &Block, fs: f32) -> (f32, f32) {
    let (tw, th) = (b.w, b.h.max(fs * text::LEADING));
    match shape {
        Shape::Circle => { let d = tw.max(th) + 28.; (d, d) }
        Shape::Bang | Shape::Cloud => (tw + 48., th + 36.),
        Shape::Hexagon => (tw + 24. + th, th + 20.),
        _ => (tw + 28., th + 18.),
    }
}

pub fn render(lines: &[Line], src: &str, title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let nodes = parse(lines, src)?;
    let fs = style.font_size;
    let col = &style.colors;
    let blocks: Vec<Block> = nodes.iter().enumerate().map(|(i, nd)| Block::new(&nd.label, 180., if i == 0 { fs * 1.15 } else { fs }, i == 0, m)).collect();
    let sizes: Vec<(f32, f32)> = nodes.iter().zip(&blocks).map(|(nd, b)| size(nd.shape, b, fs)).collect();
    // each subtree's height, leaves first (children come after parents)
    let mut tall = vec![0f32; nodes.len()];
    for i in (0..nodes.len()).rev() {
        let kids: f32 = nodes[i].children.iter().map(|&c| tall[c]).sum::<f32>() + V_GAP * nodes[i].children.len().saturating_sub(1) as f32;
        tall[i] = sizes[i].1.max(kids);
    }
    // the root's branches: each to the lighter side
    let mut side = vec![1f32; nodes.len()];
    let (mut right, mut left) = (Vec::new(), Vec::new());
    let (mut rh, mut lh) = (0f32, 0f32);
    for &c in &nodes[0].children {
        if rh <= lh { right.push(c); rh += tall[c] + V_GAP; } else { left.push(c); lh += tall[c] + V_GAP; side[c] = -1.; }
    }
    // branch colour: the root's child each node comes from
    let mut branch = vec![0usize; nodes.len()];
    for (k, &c) in nodes[0].children.iter().enumerate() {
        let mut stack = vec![c];
        while let Some(x) = stack.pop() { branch[x] = k + 1; side[x] = side[c]; stack.extend(&nodes[x].children); }
    }
    // centres: root at 0, children stacked about their parent
    let mut at = vec![(0f32, 0f32); nodes.len()];
    fn place(i: usize, x_edge: f32, y_top: f32, s: f32, nodes: &[Node], sizes: &[(f32, f32)], tall: &[f32], at: &mut [(f32, f32)]) {
        let (w, _) = sizes[i];
        at[i] = (x_edge + s * w / 2., y_top + tall[i] / 2.);
        let kids: f32 = nodes[i].children.iter().map(|&c| tall[c]).sum::<f32>() + V_GAP * nodes[i].children.len().saturating_sub(1) as f32;
        let mut y = y_top + (tall[i] - kids) / 2.;
        for &c in &nodes[i].children {
            place(c, x_edge + s * (w + H_GAP), y, s, nodes, sizes, tall, at);
            y += tall[c] + V_GAP;
        }
    }
    let rw = sizes[0].0;
    for (group, s) in [(&right, 1f32), (&left, -1f32)] {
        let total: f32 = group.iter().map(|&c| tall[c]).sum::<f32>() + V_GAP * group.len().saturating_sub(1) as f32;
        let mut y = -total / 2.;
        for &c in group.iter() {
            place(c, s * (rw / 2. + H_GAP), y, s, &nodes, &sizes, &tall, &mut at);
            y += tall[c] + V_GAP;
        }
    }
    // to the page
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (i, &(x, y)) in at.iter().enumerate() {
        let (w, h) = sizes[i];
        x0 = x0.min(x - w / 2.); x1 = x1.max(x + w / 2.); y0 = y0.min(y - h / 2.); y1 = y1.max(y + h / 2.);
    }
    let margin = 12.;
    let title_block = title.as_deref().map(|t| Block::new(t, 0., fs * 1.3, true, m)).filter(|b| !b.is_empty());
    let top = margin + title_block.as_ref().map_or(0., |b| b.h + 10.);
    let width = (x1 - x0 + 2. * margin).max(title_block.as_ref().map_or(0., |b| b.w + 2. * margin));
    let height = y1 - y0 + top + margin;
    let dx = margin - x0 + (width - 2. * margin - (x1 - x0)) / 2.;
    let dy = top - y0;
    let at: Vec<(f32, f32)> = at.iter().map(|&(x, y)| (x + dx, y + dy)).collect();
    let mut doc = Doc::new(&style.font);
    doc.paper = col.background.clone();
    if let Some(b) = &title_block { doc.text(b, width / 2., margin + b.h / 2., Anchor::Middle, &col.text); }
    let fill = |i: usize| -> &str { if branch[i] == 0 || col.series.is_empty() { &col.node_fill } else { &col.series[(branch[i] - 1) % col.series.len()] } };

    // links: from the parent's side to the child's, an S curve
    let link = Paint::new("none", &col.line, 1.5);
    for (i, nd) in nodes.iter().enumerate() {
        let Some(p) = nd.parent else { continue };
        let s = side[i];
        let (a, b) = (at[p], at[i]);
        let ax = a.0 + s * sizes[p].0 / 2. * if p == 0 { 0.6 } else { 1. };
        let bx = b.0 - s * sizes[i].0 / 2.;
        let mx = (ax + bx) / 2.;
        doc.path(&format!("M{},{} C{},{} {},{} {},{}", n(ax), n(a.1), n(mx), n(a.1), n(mx), n(b.1), n(bx), n(b.1)), &link);
    }
    for (i, nd) in nodes.iter().enumerate() {
        let (cx, cy) = at[i];
        let (w, h) = sizes[i];
        let (l, t) = (cx - w / 2., cy - h / 2.);
        let p = if i == 0 { Paint::new(&col.node_fill, &col.node_border, 1.5) } else { Paint::new(fill(i), "none", 0.) };
        match nd.shape {
            Shape::Square => doc.rect(l, t, w, h, 0., &p),
            Shape::Circle => doc.circle(cx, cy, w / 2., &p),
            Shape::Hexagon => { let k = h / 2.; doc.polygon(&[(l + k, t), (l + w - k, t), (l + w, cy), (l + w - k, t + h), (l + k, t + h), (l, cy)], &p); }
            Shape::Cloud | Shape::Bang => {
                // round bumps for a cloud, spikes for a bang, about an ellipse
                let (rx, ry) = (w / 2., h / 2.);
                let k = 64;
                let pts: Vec<(f32, f32)> = (0..k).map(|j| {
                    let a = j as f32 * std::f32::consts::TAU / k as f32;
                    let f = if nd.shape == Shape::Cloud { 0.9 + 0.1 * (a * 8.).sin().abs() } else if j % 4 == 0 { 1. } else { 0.86 };
                    (cx + rx * f * a.cos(), cy + ry * f * a.sin())
                }).collect();
                doc.polygon(&pts, &p);
            }
            Shape::Rounded | Shape::Default => doc.rect(l, t, w, h, if nd.shape == Shape::Rounded { h / 2. } else { 6. }, &p),
        }
        doc.text(&blocks[i], cx, cy, Anchor::Middle, &col.text);
    }
    Ok(Svg { svg: doc.finish(width, height, ""), width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Result<Vec<Node>, Error> { let (l, _) = crate::lines(src); parse(&l, src) }

    #[test]
    fn tree_by_indent() {
        let x = p("mindmap\n  root((mindmap))\n    Origins\n      Long history\n      ::icon(fa fa-book)\n      Popularisation\n    Research\n\t\tTab\n  :::urgent").unwrap();
        let labels: Vec<&str> = x.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["mindmap", "Origins", "Long history", "Popularisation", "Research", "Tab"]);
        assert_eq!(x[0].shape, Shape::Circle);
        assert_eq!(x[0].children, [1, 4]);
        assert_eq!(x[1].children, [2, 3]);
        assert_eq!(x[5].parent, Some(4));
        // an indent between two levels: under the nearest shallower one
        let y = p("mindmap\nRoot\n    A\n        B\n      C").unwrap();
        assert_eq!((y[2].parent, y[3].parent), (Some(1), Some(1)));
    }

    #[test]
    fn shapes() {
        let s = |t: &str| node(t);
        assert_eq!(s("id[I am a square]"), ("I am a square".into(), Shape::Square));
        assert_eq!(s("id(rounded)"), ("rounded".into(), Shape::Rounded));
        assert_eq!(s("id((circle))"), ("circle".into(), Shape::Circle));
        assert_eq!(s("id))bang(("), ("bang".into(), Shape::Bang));
        assert_eq!(s("id)cloud("), ("cloud".into(), Shape::Cloud));
        assert_eq!(s("id{{hex}}"), ("hex".into(), Shape::Hexagon));
        assert_eq!(s("On effectiveness<br/>and features"), ("On effectiveness\nand features".into(), Shape::Default));
        assert_eq!(s("Say (hi)"), ("Say (hi)".into(), Shape::Default));
        let x = p("mindmap\n  id1[\"`**Root** with\na second line`\"]\n    id2[Child]").unwrap();
        assert_eq!(x[0].label, "Root with\na second line");
        assert_eq!(x[1].parent, Some(0));
    }

    #[test]
    fn mistakes_say_where() {
        assert_eq!(p("mindmap").unwrap_err().line, 0);
        assert_eq!(p("mindmap\n  root\n    id[]").unwrap_err().line, 3);
        let deep: String = (0..300).map(|i| format!("{}n{i}\n", " ".repeat(i))).collect();
        assert!(p(&format!("mindmap\n{deep}")).is_err());
    }
}
