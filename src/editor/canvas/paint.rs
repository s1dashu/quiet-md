use crate::canvas::{self as ink, Ink, Palette, ShapeInk};
use crate::editor_theme::{Theme, UI_FONT};
use fdl_core::canvas::{Arrow, Coord, Drawing, Edge, Endpoint, Node, Object, Port, Rect, Route, Shape};
use gpui::*;
use std::{cell::{Cell, RefCell}, collections::HashMap, rc::Rc, time::Instant};

#[derive(Clone, Copy)]
pub(super) struct Camera { pub center: Coord, pub scale: f32 }

#[derive(Clone, Copy)]
pub(super) struct Viewport { pub bounds: Bounds<Pixels>, pub camera: Camera, origin: Point<Pixels> }

impl Viewport {
    pub fn new(bounds: Bounds<Pixels>, camera: Camera) -> Self {
        let [x, y] = camera.center;
        let origin = bounds.center() - point(px(x * camera.scale), px(y * camera.scale));
        Self { bounds, camera, origin }
    }

    pub fn fit(bounds: Bounds<Pixels>, frame: Rect, max_scale: f32) -> Self {
        let [w, h] = frame.size();
        let scale = (f32::from(bounds.size.width) / w).min(f32::from(bounds.size.height) / h).clamp(0.0001, max_scale);
        Self::new(bounds, Camera { center: frame.center(), scale })
    }

    pub fn screen(self, [x, y]: Coord) -> Point<Pixels> {
        self.origin + point(px(x * self.camera.scale), px(y * self.camera.scale))
    }

    pub fn local(self, point: Point<Pixels>) -> Coord {
        [f32::from(point.x - self.origin.x) / self.camera.scale, f32::from(point.y - self.origin.y) / self.camera.scale]
    }

    fn region(self) -> Rect { Rect::between(self.local(self.bounds.origin), self.local(self.bounds.bottom_right())) }

    pub fn reveal(self, object: Rect) -> Camera {
        let region = self.region();
        let available = region.size();
        let object_size = object.size();
        let center = [0, 1].map(|i| {
            if object_size[i] > available[i] { return object.center()[i] }
            let before = (object.min[i] - region.min[i]).min(0.);
            let after = (object.max[i] - region.max[i]).max(0.);
            self.camera.center[i] + before + after
        });
        Camera { center, ..self.camera }
    }

    pub fn screen_rect(self, rect: Rect) -> Bounds<Pixels> {
        Bounds::from_corners(self.screen(rect.min), self.screen(rect.max))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct NodeKey { center: [i32; 2], size: [i32; 2], shape: Shape }

impl From<&Node> for NodeKey {
    fn from(n: &Node) -> Self { Self { center: n.center, size: n.size, shape: n.shape } }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct EdgeKey { from: Endpoint, to: Endpoint, a: NodeKey, b: NodeKey, route: Route, seed: u32 }

pub(super) struct Wire { pub arrow: Arrow, strokes: Vec<Ink> }

#[derive(Default)]
pub(super) struct Cache {
    shapes: HashMap<u32, (NodeKey, Rc<ShapeInk>)>,
    arrows: HashMap<u32, (EdgeKey, Option<Rc<Wire>>)>,
}

impl Cache {
    fn shape(&mut self, n: &Node) -> Option<Rc<ShapeInk>> {
        let key = NodeKey::from(n);
        if let Some((old, paths)) = self.shapes.get(&n.id) && *old == key { return Some(paths.clone()); }
        let paths = Rc::new(ink::shape_ink(n)?);
        self.shapes.insert(n.id, (key, paths.clone()));
        Some(paths)
    }

    pub fn wire(&mut self, drawing: &Drawing, edge: &Edge) -> Option<Rc<Wire>> {
        let key = EdgeKey {
            from: edge.from, to: edge.to, route: edge.route, seed: edge.seed,
            a: drawing.node(edge.from.node)?.into(), b: drawing.node(edge.to.node)?.into(),
        };
        if let Some((old, wire)) = self.arrows.get(&edge.id) && *old == key { return wire.clone(); }
        let wire = drawing.arrow(edge).map(|arrow| Rc::new(Wire { strokes: ink::arrow_ink(edge, &arrow), arrow }));
        self.arrows.insert(edge.id, (key, wire.clone()));
        wire
    }

    fn trim(&mut self) {
        if self.shapes.len() > 400 { self.shapes.clear(); }
        if self.arrows.len() > 800 { self.arrows.clear(); }
    }
}

#[derive(Default)]
pub(super) struct Marks {
    pub nodes: Vec<u32>,
    pub edge: Option<u32>,
    pub editing: Option<Object>,
    pub anchor: Option<Endpoint>,
    pub connection: Option<(Coord, Coord, bool)>,
    pub marquee: Option<Rect>,
    pub guides: [Option<f32>; 2],
    pub ghost: Option<(Node, f32)>,
    pub landing: Option<(u32, Instant)>,
    pub handles: bool,
}

pub(super) struct Scene {
    pub camera: Option<Camera>,
    pub viewport: Option<Rc<Cell<Option<Viewport>>>>,
    pub cache: Rc<RefCell<Cache>>,
    pub grid: bool,
    pub marks: Marks,
}

fn paint_ink(window: &mut Window, view: Viewport, mut builder: PathBuilder, path: &[Ink], color: Hsla) {
    for op in path {
        match *op {
            Ink::Move(p) => builder.move_to(view.screen(p)),
            Ink::Curve(a, b, end) => builder.cubic_bezier_to(view.screen(end), view.screen(a), view.screen(b)),
            Ink::Close => builder.close(),
        }
    }
    if let Ok(p) = builder.build() { window.paint_path(p, color); }
}

fn ink_shape(window: &mut Window, view: Viewport, paths: &ShapeInk, palette: &Palette, opacity: f32) {
    let scale = view.camera.scale;
    paint_ink(window, view, PathBuilder::fill(), &paths.fill, palette.fill.opacity(opacity));
    paint_ink(window, view, PathBuilder::stroke(px(ink::HATCH_WIDTH * scale)), &paths.hatch, palette.hatch.opacity(opacity));
    paint_ink(window, view, PathBuilder::stroke(px(ink::OUTLINE_WIDTH * scale)), &paths.outline, palette.line.opacity(opacity));
}

fn line(window: &mut Window, start: Point<Pixels>, end: Point<Pixels>, color: Hsla, dashed: bool) {
    let mut path = PathBuilder::stroke(px(1.4));
    if dashed { path = path.dash_array(&[px(5.), px(4.)]); }
    path.move_to(start);
    path.line_to(end);
    if let Ok(path) = path.build() { window.paint_path(path, color); }
}

pub(super) fn label_size(window: &Window, label: &str, font_size: f32) -> Pixels {
    let run = TextRun { len: label.len(), font: font(UI_FONT), color: Theme::text(), background_color: None, underline: None, strikethrough: None };
    window.text_system().shape_line(label.to_string().into(), px(font_size), &[run], None).width
}

pub(super) fn label_rows(window: &Window, label: &str, font_size: f32, width: Pixels) -> usize {
    let run = TextRun { len: label.len(), font: font(UI_FONT), color: Theme::text(), background_color: None, underline: None, strikethrough: None };
    window.text_system().shape_text(label.to_string().into(), px(font_size), &[run], Some(width), None)
        .map_or(1, |lines| lines.iter().map(|line| line.wrap_boundaries.len() + 1).sum::<usize>().max(1))
}

fn paint_label(window: &mut Window, cx: &mut App, center: Point<Pixels>, font_size: f32, lines: &[&str], color: Hsla) {
    let line_height = px(font_size * ink::LABEL_LEADING);
    let top = center.y - line_height * lines.len() as f32 / 2.;
    for (i, label) in lines.iter().enumerate().filter(|(_, line)| !line.is_empty()) {
        let run = TextRun { len: label.len(), font: font(UI_FONT), color, background_color: None, underline: None, strikethrough: None };
        let line = window.text_system().shape_line(label.to_string().into(), px(font_size), &[run], None);
        let _ = line.paint(point(center.x - line.width / 2., top + line_height * i as f32), line_height, window, cx);
    }
}

fn handle(window: &mut Window, at: Point<Pixels>, r: f32, circle: bool, palette: &Palette) {
    let bounds = Bounds::new(at - point(px(r), px(r)), size(px(2. * r), px(2. * r)));
    window.paint_quad(quad(bounds, px(if circle { r } else { 1. }), palette.paper, px(1.4), Theme::primary(), BorderStyle::Solid));
}

pub(super) fn corners(n: &Node) -> [Coord; 4] {
    let Rect { min: [x0, y0], max: [x1, y1] } = n.bounds();
    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn grid(window: &mut Window, view: Viewport, visible: Rect, color: Hsla) {
    let step = 24.0_f32.max(16. / view.camera.scale);
    let mut y = (visible.min[1] / step).ceil() * step;
    while y < visible.max[1] {
        let mut x = (visible.min[0] / step).ceil() * step;
        while x < visible.max[0] {
            let p = view.screen([x, y]);
            window.paint_quad(fill(Bounds::new(p, size(px(1.), px(1.))), color));
            x += step;
        }
        y += step;
    }
}

pub(super) fn scene(drawing: Drawing, scene: Scene) -> impl IntoElement {
    let palette = ink::palette();
    let active = Theme::primary();
    let grid_color = Theme::text_caption().opacity(0.32);
    let frame = scene.camera.is_none().then(|| ink::frame(&drawing));
    let cache = scene.cache;
    let marks = scene.marks;
    canvas(move |bounds, window, _| {
        let view = match scene.camera {
            Some(camera) => Viewport::new(bounds, camera),
            None => Viewport::fit(bounds, frame.unwrap(), if scene.grid { 1.35 } else { 1. }),
        };
        if let Some(t) = scene.viewport && t.replace(Some(view)).is_none() { window.request_animation_frame(); }
        view
    }, move |_, view, window, cx| {
        window.paint_quad(fill(view.bounds, palette.paper));
        let scale = view.camera.scale;
        let visible = view.region();
        let mut cache = cache.borrow_mut();
        cache.trim();
        window.with_content_mask(Some(ContentMask { bounds: view.bounds }), |window| {
            if scene.grid { grid(window, view, visible, grid_color); }
            for e in drawing.edges() {
                let Some(wire) = cache.wire(&drawing, e) else { continue };
                let label = (!e.label.is_empty()).then(|| ink::edge_label_bounds(&wire.arrow, &e.label));
                let bounds = label.map_or_else(|| wire.arrow.bounds(), |l| wire.arrow.bounds().union(l));
                if !bounds.pad(6.).intersects(visible) { continue }
                let picked = marks.edge == Some(e.id);
                let color = if picked { active } else { palette.line };
                paint_ink(window, view, PathBuilder::stroke(px((ink::ARROW_WIDTH + if picked { 0.7 } else { 0. }) * scale)), &wire.strokes, color);
                if let Some(label) = label && marks.editing != Some(Object::Edge(e.id)) {
                    window.paint_quad(quad(view.screen_rect(label), px(4. * scale), palette.paper, px(0.), palette.paper, BorderStyle::Solid));
                    paint_label(window, cx, view.screen(label.center()), ink::EDGE_FONT * scale, &[&e.label], palette.text);
                }
            }
            for n in drawing.nodes().iter().filter(|n| n.bounds().pad(8.).intersects(visible)) {
                let editing = marks.editing == Some(Object::Node(n.id));
                let opacity = if let Some((id, at)) = marks.landing && id == n.id {
                    let progress = (at.elapsed().as_secs_f32() / 0.12).min(1.);
                    if progress < 1. { window.request_animation_frame(); }
                    1. - 0.5 * (1. - progress).powi(2)
                } else { 1. };
                if let Some(paths) = cache.shape(n) { ink_shape(window, view, &paths, &palette, opacity); }
                if !editing && !n.label.is_empty() {
                    let (font, lines) = ink::label_lines(n);
                    paint_label(window, cx, view.screen(n.position()), font * scale, &lines, palette.text);
                }
                if !editing && (marks.nodes.contains(&n.id) || marks.anchor.is_some_and(|e| e.node == n.id)) {
                    window.paint_quad(outline(view.screen_rect(n.bounds().pad(5. / scale)), active, BorderStyle::Dashed));
                }
            }
            if let Some((node, opacity)) = &marks.ghost {
                if let Some(paths) = cache.shape(node) { ink_shape(window, view, &paths, &palette, *opacity); }
                else {
                    paint_label(window, cx, view.screen(node.position()), ink::NODE_FONT * scale, &[crate::settings::tr("文字", "Text")], palette.text.opacity(*opacity));
                }
            }
            if let Some((start, end, bound)) = marks.connection {
                let color = if bound { active } else { active.opacity(0.7) };
                line(window, view.screen(start), view.screen(end), color, true);
                if bound { handle(window, view.screen(end), 6., true, &palette); }
            }
            if let Some(rect) = marks.marquee {
                let bounds = view.screen_rect(rect);
                window.paint_quad(fill(bounds, active.opacity(0.08)));
                window.paint_quad(outline(bounds, active, BorderStyle::Dashed));
            }
            if let Some(x) = marks.guides[0] {
                line(window, view.screen([x, visible.min[1]]), view.screen([x, visible.max[1]]), active.opacity(0.65), true);
            }
            if let Some(y) = marks.guides[1] {
                line(window, view.screen([visible.min[0], y]), view.screen([visible.max[0], y]), active.opacity(0.65), true);
            }
            if marks.handles && let [id] = marks.nodes.as_slice() && let Some(n) = drawing.node(*id) {
                for port in Port::SIDES { handle(window, view.screen(n.port(port)), 4.5, true, &palette); }
                for corner in corners(n) { handle(window, view.screen(corner), 3.5, false, &palette); }
            }
            if marks.handles && let Some(e) = marks.edge.and_then(|id| drawing.edge(id)) && let Some(wire) = cache.wire(&drawing, e) {
                handle(window, view.screen(wire.arrow.tail()), 5., true, &palette);
                handle(window, view.screen(wire.arrow.tip()), 5., true, &palette);
            }
        });
    }).size_full()
}

pub(super) fn legend(shape: Shape) -> impl IntoElement {
    let size = match shape { Shape::Rectangle => [22, 15], Shape::Circle => [19, 19], _ => [21, 21] };
    let sample = Node { id: 2, center: [0, 0], size, shape, label: String::new() };
    let paths = ink::shape_ink(&sample);
    let palette = ink::palette();
    canvas(|bounds, _, _| Viewport::new(bounds, Camera { center: [0., 0.], scale: 1. }), move |_, view, window, _| {
        if let Some(paths) = &paths { ink_shape(window, view, paths, &palette, 1.); }
    }).size(px(26.))
}

pub(super) fn symbol(pointer: bool, color: Hsla) -> impl IntoElement {
    canvas(|bounds, _, _| bounds.center(), move |_, center, window, _| {
        let at = |x, y| center + point(px(x), px(y));
        if pointer {
            let mut path = PathBuilder::fill();
            path.add_polygon(&[at(-6., -9.), at(8., 1.), at(2., 2.), at(-1., 9.)], true);
            if let Ok(path) = path.build() { window.paint_path(path, color); }
        } else {
            line(window, at(-8., 8.), at(8., -8.), color, false);
            line(window, at(1., -7.), at(8., -8.), color, false);
            line(window, at(8., -8.), at(7., -1.), color, false);
        }
    }).size(px(26.))
}
