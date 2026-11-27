//! Writing the SVG: shapes, text blocks and arrowheads as plain elements
//! with presentation attributes, which is all resvg needs.

use crate::text::{Block, LEADING, esc};
use std::fmt::Write;

/// A number for an attribute: two decimals at most, no `-0`.
pub fn n(v: f32) -> String {
    let v = if v.is_finite() { (v * 100.).round() / 100. } else { 0. };
    let s = format!("{v}");
    if s == "-0" { "0".into() } else { s }
}

/// Stroke and fill of one shape; `extra` is more attributes as written.
#[derive(Clone, Debug, Default)]
pub struct Paint {
    pub fill: String,
    pub stroke: String,
    pub width: f32,
    pub dash: Option<String>,
    pub extra: String,
}

impl Paint {
    pub fn new(fill: &str, stroke: &str, width: f32) -> Paint { Paint { fill: fill.into(), stroke: stroke.into(), width, ..Default::default() } }
    pub fn dashed_opt(mut self, dash: Option<&str>) -> Paint { self.dash = dash.map(Into::into); self }
    fn attrs(&self) -> String {
        let mut s = format!(" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"", esc(&self.fill), esc(&self.stroke), n(self.width));
        if let Some(d) = &self.dash { let _ = write!(s, " stroke-dasharray=\"{}\"", esc(d)); }
        s.push_str(&self.extra);
        s
    }
}

pub struct Doc {
    pub out: String,
    pub font: String,
}

impl Doc {
    pub fn new(font: &str) -> Doc { Doc { out: String::with_capacity(8192), font: font.into() } }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, p: &Paint) {
        let _ = write!(self.out, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"", n(x), n(y), n(w.max(0.)), n(h.max(0.)));
        if r > 0. { let _ = write!(self.out, " rx=\"{}\" ry=\"{}\"", n(r), n(r)); }
        let _ = writeln!(self.out, "{}/>", p.attrs());
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, p: &Paint) {
        let _ = writeln!(self.out, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>", n(cx), n(cy), n(r.max(0.)), p.attrs());
    }

    pub fn path(&mut self, d: &str, p: &Paint) {
        let _ = writeln!(self.out, "<path d=\"{d}\"{}/>", p.attrs());
    }

    pub fn polygon(&mut self, pts: &[(f32, f32)], p: &Paint) {
        let mut d = String::new();
        for (i, (x, y)) in pts.iter().enumerate() { let _ = write!(d, "{}{},{}", if i == 0 { "M" } else { " L" }, n(*x), n(*y)); }
        d.push_str(" Z");
        self.path(&d, p);
    }

    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), p: &Paint) {
        self.path(&format!("M{},{} L{},{}", n(a.0), n(a.1), n(b.0), n(b.1)), p);
    }

    /// `b`'s lines centred on `cx` (or starting at it for `Anchor::Start`),
    /// the block's vertical middle at `cy`.
    pub fn text(&mut self, b: &Block, cx: f32, cy: f32, anchor: Anchor, color: &str) {
        if b.is_empty() { return }
        let lh = b.size * LEADING;
        let top = cy - b.h / 2.;
        let anchor = match anchor { Anchor::Start => "start", Anchor::Middle => "middle" };
        let _ = write!(self.out, "<text font-family=\"{}\" font-size=\"{}\" fill=\"{}\" text-anchor=\"{anchor}\"", esc(&self.font), n(b.size), esc(color));
        if b.bold { self.out.push_str(" font-weight=\"bold\""); }
        self.out.push('>');
        for (i, l) in b.lines.iter().enumerate() {
            // the baseline sits about 0.35em under the middle of its line
            let y = top + lh * (i as f32 + 0.5) + b.size * 0.35;
            let _ = write!(self.out, "<tspan x=\"{}\" y=\"{}\">{}</tspan>", n(cx), n(y), esc(if l.is_empty() { " " } else { l }));
        }
        self.out.push_str("</text>\n");
    }

    /// An arrowhead with its tip at `tip`, pointing along `dir` (a unit
    /// vector), `len` long.
    pub fn head(&mut self, tip: (f32, f32), dir: (f32, f32), head: Head, len: f32, color: &str) {
        let (dx, dy) = dir;
        let (px, py) = (-dy, dx);
        let back = |t: f32, s: f32| (tip.0 - dx * t + px * s, tip.1 - dy * t + py * s);
        match head {
            Head::None => {}
            Head::Arrow => self.polygon(&[tip, back(len, len * 0.45), back(len * 0.8, 0.), back(len, -len * 0.45)], &Paint::new(color, color, 1.)),
            Head::Open => {
                let (a, b) = (back(len, len * 0.45), back(len, -len * 0.45));
                self.path(&format!("M{},{} L{},{} L{},{}", n(a.0), n(a.1), n(tip.0), n(tip.1), n(b.0), n(b.1)), &Paint { extra: " stroke-linejoin=\"round\" stroke-linecap=\"round\"".into(), ..Paint::new("none", color, 1.5) });
            }
            Head::Circle => { let c = back(len * 0.45, 0.); self.circle(c.0, c.1, len * 0.4, &Paint::new(color, color, 1.)); }
            Head::Triangle => self.polygon(&[tip, back(len * 1.2, len * 0.55), back(len * 1.2, -len * 0.55)], &Paint::new("none", color, 1.2)),
            Head::Diamond | Head::FilledDiamond => {
                let fill = if head == Head::Diamond { "none" } else { color };
                self.polygon(&[tip, back(len * 0.9, len * 0.45), back(len * 1.8, 0.), back(len * 0.9, -len * 0.45)], &Paint::new(fill, color, 1.2));
            }
            Head::Cross => {
                let c = back(len * 0.5, 0.);
                let r = len * 0.38;
                let p = Paint::new("none", color, 2.);
                self.line((c.0 - r * (dx + px), c.1 - r * (dy + py)), (c.0 + r * (dx + px), c.1 + r * (dy + py)), &p);
                self.line((c.0 - r * (dx - px), c.1 - r * (dy - py)), (c.0 + r * (dx - px), c.1 + r * (dy - py)), &p);
            }
        }
    }

    /// The document: `body` in a `w` × `h` canvas on `bg` (none when empty).
    pub fn finish(self, w: f32, h: f32, bg: &str) -> String {
        let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n", n(w), n(h), n(w), n(h));
        if !bg.is_empty() { let _ = writeln!(s, "<rect width=\"{}\" height=\"{}\" fill=\"{}\"/>", n(w), n(h), esc(bg)); }
        s.push_str(&self.out);
        s.push_str("</svg>\n");
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor { Start, Middle }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Head {
    None, Arrow, Open, Circle, Cross,
    // class diagrams: inheritance / realization, aggregation, composition
    Triangle, Diamond, FilledDiamond,
}

/// A smooth path through `pts` (d3's `curveBasis`, the curve Mermaid draws
/// edges with): it starts and ends on the end points.
pub fn basis(pts: &[(f32, f32)]) -> String {
    let mut d = String::new();
    let Some(&p0) = pts.first() else { return d };
    let _ = write!(d, "M{},{}", n(p0.0), n(p0.1));
    let Some(&p1) = pts.get(1) else { return d };
    if pts.len() == 2 { let _ = write!(d, " L{},{}", n(p1.0), n(p1.1)); return d }
    let _ = write!(d, " L{},{}", n((5. * p0.0 + p1.0) / 6.), n((5. * p0.1 + p1.1) / 6.));
    let mut bez = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| {
        let _ = write!(d, " C{},{} {},{} {},{}", n((2. * a.0 + b.0) / 3.), n((2. * a.1 + b.1) / 3.), n((a.0 + 2. * b.0) / 3.), n((a.1 + 2. * b.1) / 3.), n((a.0 + 4. * b.0 + c.0) / 6.), n((a.1 + 4. * b.1 + c.1) / 6.));
    };
    for w in pts.windows(3) { bez(w[0], w[1], w[2]); }
    let (a, b) = (pts[pts.len() - 2], pts[pts.len() - 1]);
    bez(a, b, b);
    let _ = write!(d, " L{},{}", n(b.0), n(b.1));
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(1.234), "1.23");
        assert_eq!(n(-0.001), "0");
        assert_eq!(n(f32::NAN), "0");
    }

    #[test]
    fn basis_ends_on_the_ends() {
        let d = basis(&[(0., 0.), (10., 10.), (20., 0.)]);
        assert!(d.starts_with("M0,0") && d.ends_with("20,0"), "{d}");
    }
}
