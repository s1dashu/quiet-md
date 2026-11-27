//! Sequence diagrams: participants and actors (with `as` aliases, `create`
//! and `destroy`, `box` groups), messages of every arrow, activations
//! (`+` / `-` and `activate`), notes, `loop` / `alt` / `opt` / `par` /
//! `critical` / `break` / `rect` blocks, `autonumber`, `title`. Lanes are
//! spaced so every label fits, then events go down the page one by one.

use crate::svg::{Anchor, Doc, Head, Paint, n};
use crate::text::{self, Block};
use crate::{Error, Line, Measure, Style, Svg, err};

#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub id: String,
    pub label: String,
    /// A stick figure (`actor`) rather than a box.
    pub person: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::enum_variant_names)]
pub enum Arrow {
    /// `->`
    Line,
    /// `->>`
    Arrow,
    /// `-x`
    Cross,
    /// `-)`
    Async,
    /// `<<->>`
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Side { Left, Right, Over }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frame { Loop, Alt, Opt, Par, Critical, Break, Rect }

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Message { from: usize, to: usize, text: String, arrow: Arrow, dotted: bool, activate: bool, deactivate: bool },
    Note { side: Side, a: usize, b: usize, text: String },
    Activate(usize),
    Deactivate(usize),
    Start(Frame, String),
    /// `else` / `and` / `option`.
    Divide(String),
    End,
    Create(usize),
    Destroy(usize),
    Number(Option<(i64, i64)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxGroup {
    pub title: String,
    pub color: Option<String>,
    pub actors: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Seq {
    pub actors: Vec<Actor>,
    pub events: Vec<Event>,
    pub boxes: Vec<BoxGroup>,
    pub title: Option<String>,
}

const ARROWS: [(&str, Arrow, bool); 10] = [
    ("<<-->>", Arrow::Both, true), ("<<->>", Arrow::Both, false), ("-->>", Arrow::Arrow, true), ("->>", Arrow::Arrow, false), ("-->", Arrow::Line, true),
    ("--x", Arrow::Cross, true), ("--)", Arrow::Async, true), ("->", Arrow::Line, false), ("-x", Arrow::Cross, false), ("-)", Arrow::Async, false),
];

/// A colour as `box` / `rect` take it: a word, `#hex`, `rgb(…)`.
fn color_of(s: &str) -> Option<&str> {
    const NAMED: [&str; 30] = ["aqua", "black", "blue", "fuchsia", "gray", "grey", "green", "lime", "maroon", "navy", "olive", "orange", "purple", "red", "silver", "teal", "white", "yellow", "pink", "brown", "cyan", "magenta", "gold", "violet", "indigo", "coral", "salmon", "khaki", "lightblue", "lightgreen"];
    let s = s.trim();
    let l = s.to_ascii_lowercase();
    (s.starts_with('#') || l.starts_with("rgb") || l.starts_with("hsl") || l == "transparent" || NAMED.contains(&l.as_str())).then_some(s)
}

struct P {
    s: Seq,
    open_box: Option<usize>,
    depth: usize,
}

impl P {
    fn actor(&mut self, id: &str) -> usize {
        let id = id.trim();
        if let Some(i) = self.s.actors.iter().position(|a| a.id == id) { return i }
        self.s.actors.push(Actor { id: id.into(), label: text::label(id), person: false });
        if let Some(b) = self.open_box { self.s.boxes[b].actors.push(self.s.actors.len() - 1); }
        self.s.actors.len() - 1
    }

    /// `participant A`, `actor A as Alice`, `participant A@{…}`.
    fn declare(&mut self, rest: &str, person: bool) -> usize {
        let rest = rest.trim();
        let (id, label) = match rest.split_once(" as ") { Some((a, b)) => (a.trim(), Some(b.trim())), None => (rest, None) };
        let (id, meta) = match id.split_once("@{") { Some((a, m)) => (a.trim(), Some(m)), None => (id, None) };
        let person = person || meta.is_some_and(|m| m.contains("actor"));
        let i = self.actor(id);
        if let Some(l) = label { self.s.actors[i].label = text::label(l); }
        self.s.actors[i].person = person;
        i
    }

    fn line(&mut self, l: &Line) -> Result<(), Error> {
        let t = l.text.trim_end_matches(';').trim();
        let (word, rest) = match t.split_once(char::is_whitespace) { Some((w, r)) => (w, r.trim()), None => (t, "") };
        let lower = word.to_ascii_lowercase();
        let ev = &mut self.s.events;
        match lower.as_str() {
            "participant" => { self.declare(rest, false); }
            "actor" => { self.declare(rest, true); }
            "create" => {
                let (kind, r) = rest.split_once(char::is_whitespace).unwrap_or(("participant", rest));
                let i = self.declare(r, kind == "actor");
                self.s.events.push(Event::Create(i));
            }
            "destroy" => { let i = self.actor(rest); self.s.events.push(Event::Destroy(i)); }
            "autonumber" => {
                if rest == "off" { ev.push(Event::Number(None)); } else {
                    let mut it = rest.split_whitespace().filter_map(|x| x.parse::<i64>().ok());
                    let start = it.next().unwrap_or(1);
                    ev.push(Event::Number(Some((start, it.next().unwrap_or(1)))));
                }
            }
            "activate" => { let i = self.actor(rest); self.s.events.push(Event::Activate(i)); }
            "deactivate" => { let i = self.actor(rest); self.s.events.push(Event::Deactivate(i)); }
            "title" | "title:" => self.s.title = Some(text::label(rest.trim_start_matches(':').trim())),
            _ if t.starts_with("title:") => self.s.title = Some(text::label(t[6..].trim())),
            "loop" | "alt" | "opt" | "par" | "par_over" | "critical" | "break" | "rect" => {
                let f = match lower.as_str() { "loop" => Frame::Loop, "alt" => Frame::Alt, "opt" => Frame::Opt, "critical" => Frame::Critical, "break" => Frame::Break, "rect" => Frame::Rect, _ => Frame::Par };
                ev.push(Event::Start(f, if f == Frame::Rect { rest.to_string() } else { text::label(rest) }));
                self.depth += 1;
            }
            "else" | "and" | "option" => {
                if self.depth == 0 { return err(l.no, format!("“{word}” outside a block")) }
                ev.push(Event::Divide(text::label(rest)));
            }
            "box" => {
                if self.open_box.is_some() { return err(l.no, "A box inside a box") }
                let (color, title) = match rest.split_once(char::is_whitespace) {
                    _ if color_of(rest).is_some() && !rest.contains(' ') => (Some(rest.to_string()), String::new()),
                    Some((c, t)) if color_of(c).is_some() && !c.to_ascii_lowercase().starts_with("rgb") => (Some(c.to_string()), t.to_string()),
                    _ if rest.to_ascii_lowercase().starts_with("rgb") => match rest.find(')') { Some(e) => (Some(rest[..=e].to_string()), rest[e + 1..].trim().to_string()), None => (None, rest.to_string()) },
                    _ => (None, rest.to_string()),
                };
                self.s.boxes.push(BoxGroup { title: text::label(&title), color, actors: Vec::new() });
                self.open_box = Some(self.s.boxes.len() - 1);
            }
            "end" if rest.is_empty() => {
                if self.depth > 0 { self.depth -= 1; ev.push(Event::End); }
                else if self.open_box.take().is_none() { return err(l.no, "“end” without a block") }
            }
            "note" => {
                let lr = rest.to_ascii_lowercase();
                let (side, after) = if lr.starts_with("left of") { (Side::Left, &rest[7..]) } else if lr.starts_with("right of") { (Side::Right, &rest[8..]) } else if lr.starts_with("over") { (Side::Over, &rest[4..]) } else { return err(l.no, "A note goes “left of”, “right of” or “over” a participant") };
                let Some((who, txt)) = after.split_once(':') else { return err(l.no, "A note needs “:” before its text") };
                let mut names = who.split(',');
                let a = self.actor(names.next().unwrap_or(""));
                let b = match names.next() { Some(x) => self.actor(x), None => a };
                self.s.events.push(Event::Note { side, a, b, text: text::label(txt) });
            }
            "links" | "link" | "properties" | "details" | "acctitle" | "accdescr" | "acctitle:" | "accdescr:" => {}
            _ if lower.starts_with("acctitle") || lower.starts_with("accdescr") => {}
            _ => return self.message(t, l.no),
        }
        Ok(())
    }

    /// `A->>+B: text`
    fn message(&mut self, t: &str, no: usize) -> Result<(), Error> {
        let head = t.split(':').next().unwrap_or(t);
        let mut found = None;
        'scan: for (i, _) in head.char_indices() {
            for &(tok, arrow, dotted) in &ARROWS {
                if head[i..].starts_with(tok) { found = Some((i, tok.len(), arrow, dotted)); break 'scan }
            }
        }
        let Some((i, len, arrow, dotted)) = found else {
            let w: String = t.chars().take(24).collect();
            return err(no, format!("Not a message or a statement: “{w}”"));
        };
        // `A()->>()B`: Mermaid's central connections, drawn as plain messages
        let from = t[..i].trim().trim_end_matches("()").trim();
        let rest = &t[i + len..];
        let (target, txt) = match rest.split_once(':') { Some((a, b)) => (a, b.trim()), None => (rest, "") };
        let mut target = target.trim();
        let (mut activate, mut deactivate) = (false, false);
        if let Some(x) = target.strip_prefix('+') { activate = true; target = x.trim(); } else if let Some(x) = target.strip_prefix('-') { deactivate = true; target = x.trim(); }
        let target = target.trim_start_matches("()").trim();
        if from.is_empty() || target.is_empty() { return err(no, "A message needs a sender and a receiver") }
        let (from, to) = (self.actor(from), self.actor(target));
        self.s.events.push(Event::Message { from, to, text: text::label(txt), arrow, dotted, activate, deactivate });
        Ok(())
    }
}

pub fn parse(lines: &[Line]) -> Result<Seq, Error> {
    let mut p = P { s: Seq::default(), open_box: None, depth: 0 };
    for l in lines.iter().skip(1) { p.line(l)?; }
    if p.depth > 0 { return err(0, "A block has no “end”") }
    Ok(p.s)
}

pub fn render(lines: &[Line], title: Option<String>, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let mut s = parse(lines)?;
    if s.title.is_none() { s.title = title; }
    if s.actors.is_empty() { return err(0, "The diagram has no participants") }
    draw(&s, style, m)
}

/// Lane spacing and vertical rhythm.
const GAP: f32 = 36.;
const MARGIN: f32 = 10.;
const BAR: f32 = 10.;
const WRAP: f32 = 260.;

fn draw(s: &Seq, style: &Style, m: &dyn Measure) -> Result<Svg, Error> {
    let fs = style.font_size;
    let col = &style.colors;
    let na = s.actors.len();
    let heads: Vec<Block> = s.actors.iter().map(|a| Block::new(&a.label, 0., fs, false, m)).collect();
    let bw: Vec<f32> = heads.iter().map(|b| (b.w + 28.).max(84.)).collect();
    let bh = heads.iter().map(|b| b.h).fold(fs * text::LEADING, f32::max) + 22.;
    // an actor's figure stands over its name
    let figure = 34.;
    let top_h = if s.actors.iter().any(|a| a.person) { bh.max(figure + fs * text::LEADING + 8.) } else { bh };

    // 1. lanes: gaps between neighbours, widened so each label fits
    let blocks: Vec<Block> = s.events.iter().map(|e| match e {
        Event::Message { text, .. } => Block::new(text, WRAP, fs, false, m),
        Event::Note { text, .. } => Block::new(text, WRAP - 40., fs, false, m),
        Event::Start(f, t) if *f != Frame::Rect => Block::new(&format!("[{t}]"), 0., fs * 0.9, false, m),
        Event::Divide(t) => Block::new(&format!("[{t}]"), 0., fs * 0.9, false, m),
        _ => Block::default(),
    }).collect();
    let mut gap: Vec<f32> = (0..na.saturating_sub(1)).map(|i| (bw[i] + bw[i + 1]) / 2. + GAP).collect();
    let (mut left_over, mut right_over) = (bw[0] / 2., bw[na - 1] / 2.);
    let mut need: Vec<(usize, usize, f32)> = Vec::new();
    for (k, e) in s.events.iter().enumerate() {
        let w = blocks[k].w;
        match *e {
            Event::Message { from, to, .. } if from == to => {
                let d = w + 52.;
                if from + 1 < na { need.push((from, from + 1, d + bw[from + 1] / 2.)); } else { right_over = right_over.max(d); }
            }
            Event::Message { from, to, .. } => need.push((from.min(to), from.max(to), w + 36.)),
            Event::Note { side: Side::Left, a, .. } => { let d = w + 24. + 16.; if a > 0 { need.push((a - 1, a, d + 10.)); } else { left_over = left_over.max(d); } }
            Event::Note { side: Side::Right, a, .. } => { let d = w + 24. + 16.; if a + 1 < na { need.push((a, a + 1, d + 10.)); } else { right_over = right_over.max(d); } }
            Event::Note { side: Side::Over, a, b, .. } => {
                let (a, b) = (a.min(b), a.max(b));
                let half = (w + 24.) / 2.;
                if a == b {
                    if a > 0 { need.push((a - 1, a, half + bw[a - 1] / 2. + 10.)); } else { left_over = left_over.max(half); }
                    if a + 1 < na { need.push((a, a + 1, half + bw[a + 1] / 2. + 10.)); } else { right_over = right_over.max(half); }
                } else { need.push((a, b, w + 24. - 50.)); }
            }
            _ => {}
        }
    }
    need.sort_by_key(|&(a, b, _)| b - a);
    for (a, b, d) in need {
        if a >= b || b > gap.len() { continue }
        let have: f32 = gap[a..b].iter().sum();
        if have < d { let add = (d - have) / (b - a) as f32; for g in &mut gap[a..b] { *g += add; } }
    }
    let title = s.title.as_deref().map(|t| Block::new(t, 0., fs * 1.3, true, m)).filter(|b| !b.is_empty());
    // frames hang out past their lanes; boxes need room for their titles
    let frame_pad = 22.;
    let mut x = vec![MARGIN + left_over.max(bw[0] / 2.).max(frame_pad + 6.); na];
    for i in 1..na { x[i] = x[i - 1] + gap[i - 1]; }
    let right = x[na - 1] + right_over.max(frame_pad + 6.);
    let mut width = right + MARGIN;

    // 2. down the page
    let mut d = Doc::new(&style.font);
    let mut back = Doc::new(&style.font);
    let mut mid = Doc::new(&style.font);
    let y0 = MARGIN + title.as_ref().map_or(0., |b| b.h + 10.) + if s.boxes.iter().any(|b| !b.title.is_empty()) { fs * text::LEADING + 8. } else if s.boxes.is_empty() { 0. } else { 8. };
    let created: Vec<bool> = (0..na).map(|i| s.events.contains(&Event::Create(i))).collect();
    let mut y = y0 + top_h + 18.;
    // per lane: where its line starts, open activations (start y), whether it's gone
    // a box's line hangs from the box, a figure's from its name
    let head_h = |i: usize| if s.actors[i].person { top_h } else { bh };
    let mut life_top: Vec<f32> = (0..na).map(|i| y0 + head_h(i)).collect();
    let mut life_end: Vec<Option<f32>> = vec![None; na];
    let mut acts: Vec<Vec<f32>> = vec![Vec::new(); na];
    let mut bars: Vec<(usize, f32, f32, usize)> = Vec::new();
    let mut pending_create: Vec<usize> = Vec::new();
    let mut pending_destroy: Vec<usize> = Vec::new();
    let mut number: Option<(i64, i64)> = None;
    struct Open { kind: Frame, label: String, y: f32, x0: f32, x1: f32, divides: Vec<(f32, usize)>, k: usize }
    let mut frames: Vec<Open> = Vec::new();
    let line = |c: &str, w: f32| Paint::new("none", c, w);
    // the edge of a lane's activation bar facing `toward`
    let edge = |acts: &Vec<Vec<f32>>, i: usize, toward: f32| -> f32 {
        let depth = acts[i].len();
        if depth == 0 { return x[i] }
        let off = (depth as f32 - 1.) * 5.;
        if toward >= x[i] { x[i] + off + BAR / 2. } else { x[i] + off - BAR / 2. }
    };
    let touch = |frames: &mut Vec<Open>, a: f32, b: f32| { for f in frames.iter_mut() { f.x0 = f.x0.min(a); f.x1 = f.x1.max(b); } };
    let draw_head = |d: &mut Doc, i: usize, cy_top: f32| {
        let (cx, w) = (x[i], bw[i]);
        if s.actors[i].person {
            let p = Paint::new(&col.node_fill, &col.node_border, 1.5);
            let top = cy_top + 2.;
            d.circle(cx, top + 7., 7., &p);
            let q = line(&col.node_border, 1.5);
            d.line((cx, top + 14.), (cx, top + 25.), &q);
            d.line((cx - 10., top + 18.), (cx + 10., top + 18.), &q);
            d.line((cx, top + 25.), (cx - 8., top + 33.), &q);
            d.line((cx, top + 25.), (cx + 8., top + 33.), &q);
            d.text(&heads[i], cx, top + figure + heads[i].h / 2. + 2., Anchor::Middle, &col.text);
        } else {
            d.rect(cx - w / 2., cy_top, w, bh, 4., &Paint::new(&col.node_fill, &col.node_border, 1.));
            d.text(&heads[i], cx, cy_top + bh / 2., Anchor::Middle, &col.text);
        }
    };
    for (k, e) in s.events.iter().enumerate() {
        let b = &blocks[k];
        match e {
            Event::Message { from, to, arrow, dotted, activate, deactivate, .. } => {
                let (from, to) = (*from, *to);
                let num = number.map(|(n, _)| n);
                if let Some((n, step)) = number { number = Some((n + step, step)); }
                y += b.h + 4.;
                let color = &col.line;
                let mut p = line(color, 1.5);
                if *dotted { p.dash = Some("4 3".into()); }
                let head = match arrow { Arrow::Line => Head::None, Arrow::Arrow | Arrow::Both => Head::Arrow, Arrow::Cross => Head::Cross, Arrow::Async => Head::Open };
                if pending_create.contains(&to) {
                    // the participant appears where it's first spoken to
                    pending_create.retain(|&c| c != to);
                    let top = y - top_h / 2.;
                    draw_head(&mut d, to, top);
                    life_top[to] = top + head_h(to);
                    let half = if s.actors[to].person { 14. } else { bw[to] / 2. };
                    let (sx, tx) = (edge(&acts, from, x[to]), if x[to] >= x[from] { x[to] - half } else { x[to] + half });
                    d.line((sx, y), (tx, y), &p);
                    d.head((tx, y), (if tx > sx { 1. } else { -1. }, 0.), head, 10., color);
                    d.text(b, (sx + tx) / 2., y - 4. - b.h / 2., Anchor::Middle, &col.text);
                    touch(&mut frames, sx.min(tx), sx.max(tx));
                    y += top_h / 2.;
                } else if from == to {
                    let sx = edge(&acts, from, f32::INFINITY);
                    let h = 22.;
                    let path = format!("M{},{} C{},{} {},{} {},{}", n(sx), n(y - 6.), n(sx + 46.), n(y - 10.), n(sx + 46.), n(y + h + 4.), n(sx + 2.), n(y + h));
                    d.path(&path, &p);
                    d.head((sx, y + h), (-0.98, -0.2), head, 10., color);
                    d.text(b, sx + 44. + b.w / 2., y - 2. - b.h / 2. + 4., Anchor::Middle, &col.text);
                    touch(&mut frames, sx - 10., sx + 52. + b.w);
                    y += h;
                } else {
                    let (sx, tx) = (edge(&acts, from, x[to]), x[to]);
                    // the target's bar after this message, if it starts here
                    let tx = if *activate { if tx > sx { tx - BAR / 2. + acts[to].len() as f32 * 5. } else { tx + BAR / 2. + acts[to].len() as f32 * 5. } } else { edge(&acts, to, sx) };
                    let dir = if tx > sx { 1. } else { -1. };
                    d.line((sx, y), (tx - dir * if head == Head::Arrow { 7. } else { 0. }, y), &p);
                    d.head((tx, y), (dir, 0.), head, 10., color);
                    if *arrow == Arrow::Both { d.head((sx, y), (-dir, 0.), Head::Arrow, 10., color); }
                    d.text(b, (sx + tx) / 2., y - 4. - b.h / 2., Anchor::Middle, &col.text);
                    touch(&mut frames, sx.min(tx), sx.max(tx));
                }
                if let Some(nv) = num {
                    let sx = edge(&acts, from, x[to]);
                    let t = Block::new(&nv.to_string(), 0., fs * 0.75, true, m);
                    let r = (t.w / 2. + 4.).max(8.);
                    d.circle(sx, y, r, &Paint::new(&col.line, "none", 0.));
                    d.text(&t, sx, y, Anchor::Middle, &col.background);
                }
                if *activate { acts[to].push(y); }
                if *deactivate && let Some(st) = acts[from].pop() { bars.push((from, st, y, acts[from].len())); }
                for i in std::mem::take(&mut pending_destroy) {
                    if i == from || i == to {
                        life_end[i] = Some(y);
                        let c = line(&col.line, 2.);
                        d.line((x[i] - 9., y - 9.), (x[i] + 9., y + 9.), &c);
                        d.line((x[i] - 9., y + 9.), (x[i] + 9., y - 9.), &c);
                    } else { pending_destroy.push(i); }
                }
                y += 14.;
            }
            Event::Note { side, a, b: bb, .. } => {
                y += 6.;
                let w = b.w + 24.;
                let h = b.h + 14.;
                let (x0, x1) = match side {
                    Side::Left => (x[*a] - 14. - w, x[*a] - 14.),
                    Side::Right => (x[*a] + 14., x[*a] + 14. + w),
                    Side::Over => {
                        let (l, r) = (x[*a].min(x[*bb]), x[*a].max(x[*bb]));
                        let (l, r) = if a == bb { (l - w / 2., r + w / 2.) } else { (l - 25., r + 25.) };
                        if r - l < w { let c = (l + r) / 2.; (c - w / 2., c + w / 2.) } else { (l, r) }
                    }
                };
                d.rect(x0, y, x1 - x0, h, 2., &Paint::new(&col.note_fill, &col.note_border, 1.));
                d.text(b, (x0 + x1) / 2., y + h / 2., Anchor::Middle, &col.text);
                touch(&mut frames, x0, x1);
                width = width.max(x1 + MARGIN);
                y += h + 12.;
            }
            Event::Activate(i) => acts[*i].push(y - 10.),
            Event::Deactivate(i) => if let Some(st) = acts[*i].pop() { bars.push((*i, st, y - 10., acts[*i].len())); },
            Event::Start(f, label) => {
                y += 8.;
                let head = if *f == Frame::Rect { 6. } else { b.h.max(fs * 1.3) + 12. };
                frames.push(Open { kind: *f, label: label.clone(), y, x0: f32::INFINITY, x1: f32::NEG_INFINITY, divides: Vec::new(), k });
                y += head;
            }
            Event::Divide(_) => {
                y += 6.;
                if let Some(f) = frames.last_mut() { f.divides.push((y, k)); }
                y += b.h + 10.;
            }
            Event::End => {
                y += 6.;
                let Some(f) = frames.pop() else { continue };
                let (x0, x1) = if f.x0.is_finite() { (f.x0 - 14., f.x1 + 14.) } else { (x[0] - bw[0] / 2., x[na - 1] + bw[na - 1] / 2.) };
                let fb = &blocks[f.k];
                // wide enough for its tab and condition, and each divider's
                let tab = Block::new("critical", 0., fs * 0.9, true, m).w + 16.;
                let mut x1 = x1;
                if f.kind != Frame::Rect { x1 = x1.max(x0 + tab + 8. + fb.w + 10.); }
                for &(_, dk) in &f.divides { x1 = x1.max(x0 + blocks[dk].w + 20.); }
                let r = (x0, f.y, x1 - x0, y - f.y);
                if f.kind == Frame::Rect {
                    let c = f.label.trim();
                    let p = Paint { extra: " fill-opacity=\"0.3\"".into(), ..Paint::new(if c.is_empty() { &col.frame_fill } else { c }, "none", 0.) };
                    back.rect(r.0, r.1, r.2, r.3, 0., &p);
                } else {
                    mid.rect(r.0, r.1, r.2, r.3, 0., &line(&col.frame, 1.));
                    let name = match f.kind { Frame::Loop => "loop", Frame::Alt => "alt", Frame::Opt => "opt", Frame::Par => "par", Frame::Critical => "critical", Frame::Break => "break", Frame::Rect => "" };
                    let nb = Block::new(name, 0., fs * 0.9, true, m);
                    let (tw, th) = (nb.w + 16., nb.h + 6.);
                    mid.polygon(&[(x0, f.y), (x0 + tw, f.y), (x0 + tw, f.y + th - 6.), (x0 + tw - 6., f.y + th), (x0, f.y + th)], &Paint::new(&col.frame_fill, &col.frame, 1.));
                    mid.text(&nb, x0 + tw / 2., f.y + th / 2., Anchor::Middle, &col.text);
                    if !f.label.trim().is_empty() { mid.text(fb, x0 + tw + 8., f.y + th / 2., Anchor::Start, &col.text); }
                    for (dy, dk) in &f.divides {
                        let mut p = line(&col.frame, 1.);
                        p.dash = Some("4 3".into());
                        mid.line((x0, *dy), (x1, *dy), &p);
                        let db = &blocks[*dk];
                        if db.lines.first().is_some_and(|l| l != "[]") { mid.text(db, (x0 + x1) / 2., dy + 4. + db.h / 2., Anchor::Middle, &col.text); }
                    }
                }
                width = width.max(x1 + MARGIN);
                touch(&mut frames, x0, x1);
                y += 8.;
            }
            Event::Create(i) => pending_create.push(*i),
            Event::Destroy(i) => pending_destroy.push(*i),
            Event::Number(v) => number = *v,
        }
    }
    y += 8.;
    for (i, st) in acts.iter().enumerate() { for (dep, &s0) in st.iter().enumerate() { bars.push((i, s0, y, dep)); } }
    let bottom = y;
    let height = bottom + top_h + MARGIN;

    // the page, back to front
    let mut out = Doc::new(&style.font);
    if let Some(t) = &title { out.text(t, width / 2., MARGIN + t.h / 2., Anchor::Middle, &col.text); }
    for bx in &s.boxes {
        let (Some(l), Some(r)) = (bx.actors.iter().map(|&i| x[i] - bw[i] / 2.).reduce(f32::min), bx.actors.iter().map(|&i| x[i] + bw[i] / 2.).reduce(f32::max)) else { continue };
        let top = y0 - if bx.title.is_empty() { 6. } else { fs * text::LEADING + 6. };
        let p = match &bx.color { Some(c) if c != "transparent" => Paint { extra: " fill-opacity=\"0.2\"".into(), ..Paint::new(c, &col.cluster_border, 1.) }, _ => Paint::new("none", &col.cluster_border, 1.) };
        out.rect(l - 8., top, r - l + 16., height - MARGIN - top + 6., 4., &p);
        let tb = Block::new(&bx.title, 0., fs, true, m);
        out.text(&tb, (l + r) / 2., top + 4. + tb.h / 2., Anchor::Middle, &col.text);
    }
    out.out.push_str(&back.out);
    for i in 0..na {
        let end = life_end[i].unwrap_or(bottom);
        let mut p = line(&col.line, 1.);
        p.dash = Some("3 3".into());
        out.line((x[i], life_top[i]), (x[i], end), &p);
    }
    for &(i, a, b, dep) in &bars {
        let cx = x[i] + dep as f32 * 5.;
        out.rect(cx - BAR / 2., a, BAR, (b - a).max(4.), 0., &Paint::new(&col.activation_fill, &col.node_border, 1.));
    }
    out.out.push_str(&mid.out);
    out.out.push_str(&d.out);
    for i in 0..na {
        if !created[i] { draw_head(&mut out, i, y0); }
        if life_end[i].is_none() { draw_head(&mut out, i, bottom); }
    }
    Ok(Svg { svg: out.finish(width, height, ""), width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(src: &str) -> Seq { let (l, _) = crate::lines(src); parse(&l).unwrap_or_else(|e| panic!("{e}: {src}")) }

    #[test]
    fn participants_and_messages() {
        let s = p("sequenceDiagram\nparticipant A as Alice\nactor B as Bob\nA->>B: Hello\nB-->>A: Hi\nA-)C: async\nC--xA: cross\nA->B: line\nA<<->>B: both\nJohn Doe->>Jane: spaced\nA()->>()B: central");
        assert_eq!(s.actors.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["A", "B", "C", "John Doe", "Jane"]);
        assert_eq!(s.actors[0].label, "Alice");
        assert!(s.actors[1].person);
        let arrows: Vec<(Arrow, bool)> = s.events.iter().filter_map(|e| match e { Event::Message { arrow, dotted, .. } => Some((*arrow, *dotted)), _ => None }).collect();
        assert_eq!(arrows, [(Arrow::Arrow, false), (Arrow::Arrow, true), (Arrow::Async, false), (Arrow::Cross, true), (Arrow::Line, false), (Arrow::Both, false), (Arrow::Arrow, false), (Arrow::Arrow, false)]);
        assert_eq!(s.actors.len(), 5);
    }

    #[test]
    fn activations_notes_blocks() {
        let s = p("sequenceDiagram\nA->>+B: x\nB-->>-A: y\nactivate A\ndeactivate A\nNote right of A: r\nnote over A,B: o\nloop Every minute\nA->>B: z\nend\nalt ok\nA->>B: 1\nelse not ok\nA->>B: 2\nend\nrect rgb(0, 0, 255)\nA->>B: in rect\nend");
        assert!(matches!(s.events[0], Event::Message { activate: true, .. }));
        assert!(matches!(s.events[1], Event::Message { deactivate: true, .. }));
        assert!(matches!(&s.events[5], Event::Note { side: Side::Over, a: 0, b: 1, .. }));
        assert!(matches!(&s.events[6], Event::Start(Frame::Loop, t) if t == "Every minute"));
        assert!(s.events.iter().any(|e| matches!(e, Event::Divide(t) if t == "not ok")));
        assert!(matches!(&s.events[14], Event::Start(Frame::Rect, t) if t == "rgb(0, 0, 255)"));
    }

    #[test]
    fn errors() {
        let bad = |src: &str| { let (l, _) = crate::lines(src); parse(&l).expect_err(src) };
        assert_eq!(bad("sequenceDiagram\nA->>B: ok\nwhat is this").line, 3);
        assert_eq!(bad("sequenceDiagram\nloop x\nA->>B: y").line, 0);
        assert_eq!(bad("sequenceDiagram\nend").line, 2);
        assert_eq!(bad("sequenceDiagram\nelse x").line, 2);
    }

    #[test]
    fn boxes() {
        let s = p("sequenceDiagram\nbox Aqua Group A\nparticipant A\nparticipant B\nend\nbox rgb(1,2,3)\nparticipant C\nend\nbox Plain title\nparticipant D\nend\nA->>D: x");
        assert_eq!(s.boxes[0], BoxGroup { title: "Group A".into(), color: Some("Aqua".into()), actors: vec![0, 1] });
        assert_eq!(s.boxes[1].color.as_deref(), Some("rgb(1,2,3)"));
        assert_eq!((s.boxes[2].title.as_str(), s.boxes[2].color.is_none()), ("Plain title", true));
    }
}
