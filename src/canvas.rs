//! Canvas ink paths shared by the GPUI view and static exports.

use fdl_core::canvas::{Arrow, Drawing, Edge, HEIGHT, Node, Rect, Shape, WIDTH};
use gpui::Hsla;
use std::fmt::Write;
use unicode_segmentation::UnicodeSegmentation;
use crate::editor_theme::Theme;

pub(crate) struct Palette { pub paper: Hsla, pub fill: Hsla, pub hatch: Hsla, pub line: Hsla, pub text: Hsla }

pub(crate) fn palette() -> Palette {
    let fill = Theme::bg_overlay();
    let line = Theme::text_code();
    Palette { paper: Theme::bg_body(), fill, hatch: fill.blend(line.opacity(0.29)), line, text: Theme::text() }
}

pub(crate) const OUTLINE_WIDTH: f32 = 1.35;
pub(crate) const ARROW_WIDTH: f32 = 1.55;
pub(crate) const HATCH_WIDTH: f32 = 0.9;
pub(crate) const NODE_FONT: f32 = 14.;
pub(crate) const EDGE_FONT: f32 = 13.;
pub(crate) const LABEL_LEADING: f32 = 1.22;

type Coord = [f32; 2];

#[derive(Clone, Copy)]
pub(crate) enum Ink {
    Move(Coord),
    Curve(Coord, Coord, Coord),
    Close,
}

pub(crate) struct ShapeInk {
    pub fill: Vec<Ink>,
    pub hatch: Vec<Ink>,
    pub outline: Vec<Ink>,
}

struct Pen(u32);

impl Pen {
    fn offset(&mut self, amount: f32) -> f32 {
        // Seeded from the saved IDs, so redraws, dragging and exports use the same strokes.
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((self.0 >> 8) as f32 / 8_388_608. - 1.) * amount
    }

    fn bend(&mut self, from: Coord, to: Coord, amount: f32) -> Ink {
        let [dx, dy] = [to[0] - from[0], to[1] - from[1]];
        let length = dx.hypot(dy);
        let normal = [-dy / length.max(f32::EPSILON), dx / length.max(f32::EPSILON)];
        let mut control = |t| {
            let offset = self.offset(amount.min(length * 0.16));
            [from[0] + dx * t + normal[0] * offset, from[1] + dy * t + normal[1] * offset]
        };
        Ink::Curve(control(0.3), control(0.7), to)
    }

    fn contour(&mut self, vertices: &[Coord], round: bool, amount: f32) -> Vec<Ink> {
        let points: Vec<_> = vertices.iter().map(|&[x, y]| [x + self.offset(amount), y + self.offset(amount)]).collect();
        let n = points.len();
        let mut path = vec![Ink::Move(points[0])];
        for (i, &from) in points.iter().enumerate() {
            let to = points[(i + 1) % n];
            path.push(if round {
                // Smooth the circle through its perturbed vertices with Catmull–Rom tangents.
                let prev = points[(i + n - 1) % n];
                let next = points[(i + 2) % n];
                Ink::Curve(
                    [from[0] + (to[0] - prev[0]) / 6., from[1] + (to[1] - prev[1]) / 6.],
                    [to[0] - (next[0] - from[0]) / 6., to[1] - (next[1] - from[1]) / 6.],
                    to,
                )
            } else {
                self.bend(from, to, amount * 1.35)
            });
        }
        path.push(Ink::Close);
        path
    }
}

fn vertices(node: &Node, inset: f32) -> Vec<Coord> {
    let [x, y] = node.center.map(|n| n as f32);
    let [w, h] = node.size.map(|n| n as f32 / 2.);
    match node.shape {
        Shape::Rectangle => {
            let (w, h) = (w - inset, h - inset);
            vec![[x - w, y - h], [x + w, y - h], [x + w, y + h], [x - w, y + h]]
        }
        Shape::Circle => (0..16).map(|i| {
            let (sin, cos) = (i as f32 * std::f32::consts::TAU / 16.).sin_cos();
            [x + (w - inset) * cos, y + (h - inset) * sin]
        }).collect(),
        Shape::Diamond => {
            let scale = 1. - inset * w.recip().hypot(h.recip());
            let (w, h) = (w * scale, h * scale);
            vec![[x, y - h], [x + w, y], [x, y + h], [x - w, y]]
        }
        Shape::Text => Vec::new(),
    }
}

fn hatching(boundary: &[Coord], pen: &mut Pen) -> Vec<Ink> {
    let sum = |[x, y]: Coord| x + y;
    let (min, max) = boundary.iter().copied().map(sum).fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), p| (min.min(p), max.max(p)));
    let mut path = Vec::new();
    let mut band = min + 5.5;
    // Intersect parallel x + y = band lines with the inset convex boundary.
    while band < max {
        let mut ends = boundary.iter().copied().zip(boundary.iter().copied().cycle().skip(1)).filter_map(|(a, b)| {
            let (start, end) = (sum(a), sum(b));
            if (start <= band) == (end <= band) { return None }
            let t = (band - start) / (end - start);
            Some([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
        });
        if let (Some(from), Some(to)) = (ends.next(), ends.next()) {
            path.push(Ink::Move(from));
            path.push(pen.bend(from, to, 0.7));
        }
        band += 11.;
    }
    path
}

pub(crate) fn shape_ink(node: &Node) -> Option<ShapeInk> {
    if node.shape == Shape::Text { return None }
    let vertices_outer = vertices(node, 0.);
    let mut pen = Pen(node.id.wrapping_mul(0x9e37_79b9));
    let round = node.shape == Shape::Circle;
    let fill = pen.contour(&vertices_outer, round, 1.15);
    let mut outline = fill.clone();
    outline.extend(pen.contour(&vertices_outer, round, 1.7));
    // Leave room for the small variations in the two ink contours.
    let hatch = hatching(&vertices(node, 4.5), &mut pen);
    Some(ShapeInk { fill, hatch, outline })
}

pub(crate) fn arrow_ink(edge: &Edge, arrow: &Arrow) -> Vec<Ink> {
    let mut pen = Pen(edge.seed);
    let tip = arrow.tip();
    let [left, right] = arrow.wings;
    let mut strokes = Vec::with_capacity(2 * (arrow.points.len() + 3));
    for (shaft, head) in [(3.4, 1.35), (2.7, 1.05)] {
        strokes.push(Ink::Move(arrow.tail()));
        strokes.extend(arrow.points.windows(2).map(|pair| pen.bend(pair[0], pair[1], shaft)));
        strokes.extend([Ink::Move(left), pen.bend(left, tip, head), pen.bend(tip, right, head)]);
    }
    strokes
}

fn glyph_width(g: &str) -> f32 { if g.is_ascii() { 0.72 } else { 1. } }

pub(crate) fn text_width(s: &str, font: f32) -> f32 { s.graphemes(true).map(glyph_width).sum::<f32>() * font }

pub(crate) fn label_width(node: &Node) -> f32 {
    node.size[0] as f32 * match node.shape { Shape::Circle => 0.75, Shape::Diamond => 0.68, _ => 0.88 }
}

pub(crate) fn label_lines(node: &Node) -> (f32, Vec<&str>) {
    let width = label_width(node);
    let height = node.size[1] as f32 * if node.shape == Shape::Diamond { 0.64 } else { 0.88 };
    let units = node.label.lines().map(|line| text_width(line, 1.).max(1.)).sum::<f32>();
    let font = (width * height / (units.max(1.) * LABEL_LEADING)).sqrt().clamp(2., NODE_FONT);
    let mut lines = Vec::new();
    for line in node.label.split('\n') {
        let (mut start, mut used) = (0, 0.);
        for (i, glyph) in line.grapheme_indices(true) {
            let advance = glyph_width(glyph) * font;
            if i > start && used + advance > width {
                lines.push(&line[start..i]);
                (start, used) = (i, 0.);
            }
            used += advance;
        }
        lines.push(&line[start..]);
    }
    (font.min(height / (lines.len() as f32 * LABEL_LEADING)), lines)
}

pub(crate) fn edge_label_bounds(arrow: &Arrow, label: &str) -> Rect {
    let [x, y] = arrow.label_position();
    let w = text_width(label, EDGE_FONT) / 2. + 5.;
    Rect::between([x - w, y - 10.], [x + w, y + 10.])
}

pub(crate) fn frame(drawing: &Drawing) -> Rect {
    let mut nodes = drawing.nodes().iter();
    let Some(first) = nodes.next() else { return Rect::between([0., 0.], [WIDTH as f32, HEIGHT as f32]) };
    let mut frame = nodes.fold(first.bounds(), |b, n| b.union(n.bounds()));
    for edge in drawing.edges() {
        if let Some(arrow) = drawing.arrow(edge) {
            frame = frame.union(arrow.bounds());
            if !edge.label.is_empty() { frame = frame.union(edge_label_bounds(&arrow, &edge.label)); }
        }
    }
    frame.pad(24.)
}

fn hex(color: Hsla) -> String {
    let c = color.to_rgb();
    format!("#{:02x}{:02x}{:02x}", (c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8)
}

fn svg_path(svg: &mut String, ink: &[Ink], fill: &str, stroke: &str, width: f32) {
    svg.push_str("<path d=\"");
    for op in ink {
        match *op {
            Ink::Move([x, y]) => { let _ = write!(svg, "M{x} {y}"); }
            Ink::Curve([ax, ay], [bx, by], [x, y]) => { let _ = write!(svg, "C{ax} {ay} {bx} {by} {x} {y}"); }
            Ink::Close => svg.push('Z'),
        }
    }
    let _ = write!(svg, r##"" fill="{fill}" stroke="{stroke}" stroke-width="{width}"/>"##);
}

fn svg_label(svg: &mut String, [x, y]: Coord, font: f32, label: &str, text: &str) {
    let escaped = label.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let _ = write!(svg, r##"<text x="{x}" y="{y}" text-anchor="middle" font-family="Arial, PingFang SC, Microsoft YaHei, sans-serif" font-size="{font}" fill="{text}">{escaped}</text>"##);
}

fn svg_at(drawing: &Drawing, requested_scale: f32) -> String {
    let palette = crate::theme::light(palette);
    let (paper, fill, hatch, line, text) = (hex(palette.paper), hex(palette.fill), hex(palette.hatch), hex(palette.line), hex(palette.text));
    let bounds = frame(drawing);
    let [x, y] = bounds.min;
    let [w, h] = bounds.size();
    let scale = requested_scale.min(8192. / w.max(h)).min((16_000_000. / (w * h)).sqrt());
    let [width, height] = [(w * scale).ceil().max(1.), (h * scale).ceil().max(1.)];
    let mut svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="{x} {y} {w} {h}"><rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{paper}"/>"##);
    for edge in drawing.edges() {
        if let Some(arrow) = drawing.arrow(edge) {
            svg_path(&mut svg, &arrow_ink(edge, &arrow), "none", &line, ARROW_WIDTH);
            if !edge.label.is_empty() {
                let bounds = edge_label_bounds(&arrow, &edge.label);
                let [x, y] = bounds.min;
                let [w, h] = bounds.size();
                let _ = write!(svg, r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="4" fill="{paper}"/>"##);
                let [x, y] = arrow.label_position();
                svg_label(&mut svg, [x, y + EDGE_FONT * 0.34], EDGE_FONT, &edge.label, &text);
            }
        }
    }
    for node in drawing.nodes() {
        let [x, y] = node.center;
        if let Some(ink) = shape_ink(node) {
            svg_path(&mut svg, &ink.fill, &fill, "none", 0.);
            svg_path(&mut svg, &ink.hatch, "none", &hatch, HATCH_WIDTH);
            svg_path(&mut svg, &ink.outline, "none", &line, OUTLINE_WIDTH);
        }
        if node.label.is_empty() { continue }
        let (font, lines) = label_lines(node);
        let line_height = font * LABEL_LEADING;
        for (index, label) in lines.iter().enumerate() {
            let baseline = y as f32 + font * 0.34 + (index as f32 - (lines.len() - 1) as f32 / 2.) * line_height;
            svg_label(&mut svg, [x as f32, baseline], font, label, &text);
        }
    }
    svg.push_str("</svg>");
    svg
}

pub(crate) fn svg(drawing: &Drawing) -> String { svg_at(drawing, 1.) }

pub(crate) fn png(drawing: &Drawing, renderer: &gpui::SvgRenderer) -> anyhow::Result<Vec<u8>> {
    use image::ImageEncoder;
    let pixmap = renderer.render_pixmap(svg_at(drawing, 1.5).as_bytes(), gpui::SvgSize::ScaleFactor(1.))?;
    let mut png = Vec::new();
    // The canvas has an opaque paper background, so these RGBA pixels need no alpha conversion.
    image::codecs::png::PngEncoder::new(&mut png).write_image(pixmap.data(), pixmap.width(), pixmap.height(), image::ExtendedColorType::Rgba8)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_drawn_shapes_have_two_contours_and_inset_hatching() {
        for shape in [Shape::Rectangle, Shape::Circle, Shape::Diamond] {
            let mut drawing = Drawing::default();
            let id = drawing.add_node(shape, [240, 140]).unwrap();
            let ink = shape_ink(drawing.node(id).unwrap()).unwrap();
            assert_eq!(ink.outline.iter().filter(|op| matches!(op, Ink::Close)).count(), 2);
            assert!(!ink.hatch.is_empty(), "{shape:?}");
            // The endpoints and control points lie inside this convex shape,
            // so each hatch curve stays inside too.
            for op in ink.hatch {
                let inside = |point| drawing.node_at(point) == Some(id);
                assert!(match op {
                    Ink::Move(p) => inside(p),
                    Ink::Curve(a, b, p) => inside(a) && inside(b) && inside(p),
                    Ink::Close => false,
                }, "{shape:?}");
            }
        }
    }
}
