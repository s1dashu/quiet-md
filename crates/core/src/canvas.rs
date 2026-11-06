//! Editable canvas geometry. Legacy records keep their original encoding until edited.

use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};

pub const VERSION: &str = "2";
const LEGACY_VERSION: &str = "1";
pub const WIDTH: i32 = 900;
pub const HEIGHT: i32 = 520;
pub const LABEL_LIMIT: usize = 160;
pub const EDGE_LABEL_LIMIT: usize = 48;
const COORD_LIMIT: i32 = 100_000;
const SIZE_LIMIT: i32 = 4096;
const NODE_LIMIT: usize = 200;
const EDGE_LIMIT: usize = 400;
const RECORD_LIMIT: usize = 1024 * 1024;

pub type Coord = [f32; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect { pub min: Coord, pub max: Coord }

impl Rect {
    pub fn between(a: Coord, b: Coord) -> Self {
        Self { min: [a[0].min(b[0]), a[1].min(b[1])], max: [a[0].max(b[0]), a[1].max(b[1])] }
    }

    pub fn size(self) -> Coord { [self.max[0] - self.min[0], self.max[1] - self.min[1]] }
    pub fn center(self) -> Coord { [(self.min[0] + self.max[0]) / 2., (self.min[1] + self.max[1]) / 2.] }

    pub fn pad(self, n: f32) -> Self {
        Self { min: self.min.map(|v| v - n), max: self.max.map(|v| v + n) }
    }

    pub fn union(self, other: Self) -> Self {
        Self { min: [self.min[0].min(other.min[0]), self.min[1].min(other.min[1])], max: [self.max[0].max(other.max[0]), self.max[1].max(other.max[1])] }
    }

    pub fn contains(self, p: Coord) -> bool {
        (self.min[0]..=self.max[0]).contains(&p[0]) && (self.min[1]..=self.max[1]).contains(&p[1])
    }

    pub fn intersects(self, other: Self) -> bool {
        (0..2).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape { Rectangle, Circle, Diamond, Text }

impl Shape {
    pub fn size(self) -> [i32; 2] {
        match self {
            Self::Rectangle => [152, 64],
            Self::Circle => [104, 104],
            Self::Diamond => [152, 96],
            Self::Text => [160, 46],
        }
    }

    pub fn constrain_size(self, [w, h]: [i32; 2]) -> [i32; 2] {
        let w = w.clamp(48, SIZE_LIMIT);
        [w, if self == Self::Circle { w } else { h.clamp(32, SIZE_LIMIT) }]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: u32,
    pub shape: Shape,
    pub center: [i32; 2],
    #[serde(default)]
    pub size: [i32; 2],
    pub label: String,
}

impl Node {
    pub fn position(&self) -> Coord { self.center.map(|n| n as f32) }

    pub fn bounds(&self) -> Rect {
        let [x, y] = self.position();
        let [w, h] = self.size.map(|n| n as f32 / 2.);
        Rect { min: [x - w, y - h], max: [x + w, y + h] }
    }

    fn hit(&self, [x, y]: Coord) -> bool {
        let [w, h] = self.size;
        let dx = ((x - self.center[0] as f32) * 2. / w as f32).abs();
        let dy = ((y - self.center[1] as f32) * 2. / h as f32).abs();
        match self.shape {
            Shape::Circle => dx * dx + dy * dy <= 1.,
            Shape::Diamond => dx + dy <= 1.,
            _ => dx.max(dy) <= 1.,
        }
    }

    pub fn port(&self, port: Port) -> Coord {
        port.direction().map_or_else(|| self.position(), |dir| self.anchor(dir))
    }

    fn anchor(&self, [dx, dy]: Coord) -> Coord {
        let [w, h] = self.size;
        let a = (dx * 2. / w as f32).abs();
        let b = (dy * 2. / h as f32).abs();
        let scale = match self.shape {
            Shape::Circle => a.hypot(b),
            Shape::Diamond => a + b,
            _ => a.max(b),
        };
        [self.center[0] as f32 + dx / scale, self.center[1] as f32 + dy / scale]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Port { #[default] Auto, Top, Right, Bottom, Left }

impl Port {
    pub const SIDES: [Self; 4] = [Self::Top, Self::Right, Self::Bottom, Self::Left];

    fn direction(self) -> Option<Coord> {
        Some(match self {
            Self::Top => [0., -1.],
            Self::Right => [1., 0.],
            Self::Bottom => [0., 1.],
            Self::Left => [-1., 0.],
            Self::Auto => return None,
        })
    }

    fn facing([x, y]: Coord) -> Self {
        if x.abs() > y.abs() {
            if x >= 0. { Self::Right } else { Self::Left }
        } else if y >= 0. { Self::Bottom } else { Self::Top }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint { pub node: u32, #[serde(default)] pub port: Port }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route { #[default] Straight, Elbow }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub id: u32,
    pub from: Endpoint,
    pub to: Endpoint,
    #[serde(default)]
    pub route: Route,
    #[serde(default)]
    pub label: String,
    pub seed: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Object { Node(u32), Edge(u32) }

#[derive(Clone, Copy)]
pub enum Axis { X, Y }

impl Axis {
    fn index(self) -> usize { match self { Self::X => 0, Self::Y => 1 } }
}

#[derive(Clone, Debug)]
pub struct Arrow { pub points: Vec<Coord>, pub wings: [Coord; 2] }

impl Arrow {
    pub fn tail(&self) -> Coord { self.points[0] }
    pub fn tip(&self) -> Coord { self.points[self.points.len() - 1] }

    pub fn bounds(&self) -> Rect {
        self.points.iter().chain(&self.wings).fold(Rect::between(self.tail(), self.tail()), |b, &p| b.union(Rect::between(p, p)))
    }

    pub fn label_position(&self) -> Coord {
        let mut halfway = path_length(&self.points) / 2.;
        for segment in self.points.windows(2) {
            let [a, b] = [segment[0], segment[1]];
            let [dx, dy] = [b[0] - a[0], b[1] - a[1]];
            let length = dx.hypot(dy);
            if halfway <= length && length > 0. { return [a[0] + dx * halfway / length, a[1] + dy * halfway / length] }
            halfway -= length;
        }
        self.tip()
    }

    pub fn hit(&self, point: Coord, tolerance: f32) -> bool {
        self.points.windows(2).any(|segment| distance_to_segment(point, segment[0], segment[1]) <= tolerance)
    }
}

fn distance_to_segment(p: Coord, a: Coord, b: Coord) -> f32 {
    let [dx, dy] = [b[0] - a[0], b[1] - a[1]];
    let [x, y] = [p[0] - a[0], p[1] - a[1]];
    let along = ((x * dx + y * dy) / (dx * dx + dy * dy).max(f32::EPSILON)).clamp(0., 1.);
    (x - dx * along).hypot(y - dy * along)
}

fn path_length(points: &[Coord]) -> f32 {
    points.windows(2).map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1])).sum()
}

// Keep the exit stubs fixed and choose the shortest turn that stays outside
// both endpoints. Exterior lanes cover ports facing away from one another.
fn elbow(from: &Node, to: &Node, tail: Coord, tip: Coord, out: Coord, into: Coord) -> Vec<Coord> {
    let start = [tail[0] + out[0] * 18., tail[1] + out[1] * 18.];
    let end = [tip[0] + into[0] * 18., tip[1] + into[1] * 18.];
    let span = from.bounds().union(to.bounds()).pad(24.);
    let mid = [(start[0] + end[0]) / 2., (start[1] + end[1]) / 2.];
    let crosses = |line: &[Coord], node: &Node| {
        let interior = node.bounds().pad(-0.5);
        line.windows(2).any(|p| Rect::between(p[0], p[1]).intersects(interior))
    };
    let candidates = [
        [[start[0], end[1]], [start[0], end[1]]],
        [[end[0], start[1]], [end[0], start[1]]],
        [[mid[0], start[1]], [mid[0], end[1]]],
        [[start[0], mid[1]], [end[0], mid[1]]],
        [[span.min[0], start[1]], [span.min[0], end[1]]],
        [[span.max[0], start[1]], [span.max[0], end[1]]],
        [[start[0], span.min[1]], [end[0], span.min[1]]],
        [[start[0], span.max[1]], [end[0], span.max[1]]],
    ];
    let mut path = candidates.into_iter().map(|[a, b]| [start, a, b, end])
        .filter(|p| !crosses(p, from) && !crosses(p, to))
        .min_by(|a, b| path_length(a).total_cmp(&path_length(b)))
        .map_or_else(|| vec![tail, start, [start[0], end[1]], end, tip], |p| {
            let [s, a, b, e] = p;
            vec![tail, s, a, b, e, tip]
        });
    path.dedup();
    path
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drawing {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    #[serde(skip)]
    legacy: Option<Arc<str>>,
}

#[derive(Deserialize)]
struct LegacyDrawing { nodes: Vec<Node>, edges: Vec<LegacyEdge> }

#[derive(Deserialize)]
struct LegacyEdge { from: u32, to: u32 }

impl Drawing {
    pub fn from_record(version: &str, record: &str) -> Option<Self> {
        if record.len() > RECORD_LIMIT { return None }
        let drawing: Self = match version {
            VERSION => serde_json::from_str(record).ok()?,
            LEGACY_VERSION => {
                let mut data: LegacyDrawing = serde_json::from_str(record).ok()?;
                for n in &mut data.nodes {
                    n.size = n.shape.size();
                    let [x, y] = n.center;
                    let [w, h] = n.size;
                    if !(w / 2..=WIDTH - w / 2).contains(&x) || !(h / 2..=HEIGHT - h / 2).contains(&y)
                        || !valid_label(&n.label, 60, false) { return None }
                }
                let edges = data.edges.into_iter().enumerate().map(|(i, e)| Edge {
                    id: i as u32 + 1, from: Endpoint { node: e.from, port: Port::Auto }, to: Endpoint { node: e.to, port: Port::Auto },
                    route: Route::Straight, label: String::new(), seed: e.from.wrapping_mul(0x9e37_79b9) ^ e.to.rotate_left(16),
                }).collect();
                Self { nodes: data.nodes, edges, legacy: Some(Arc::from(record)) }
            }
            _ => return None,
        };
        let mut ids = HashSet::new();
        let mut edge_ids = HashSet::new();
        let mut pairs = HashSet::new();
        (drawing.nodes.len() <= NODE_LIMIT && drawing.edges.len() <= EDGE_LIMIT
            && drawing.nodes.iter().all(|n| {
                n.id > 0 && ids.insert(n.id) && n.center == clamp_center(n.center)
                    && n.size == n.shape.constrain_size(n.size) && valid_label(&n.label, LABEL_LIMIT, true)
            })
            && drawing.edges.iter().all(|e| {
                let (from, to) = (e.from.node, e.to.node);
                e.id > 0 && edge_ids.insert(e.id) && from != to && ids.contains(&from) && ids.contains(&to)
                    && pairs.insert((from, to)) && valid_label(&e.label, EDGE_LABEL_LIMIT, false)
            })).then_some(drawing)
    }

    pub fn version(&self) -> &'static str { if self.legacy.is_some() { LEGACY_VERSION } else { VERSION } }

    pub fn record(&self) -> String {
        self.legacy.as_ref().map_or_else(|| serde_json::to_string(self).expect("serialize canvas"), |s| s.to_string())
    }

    pub fn nodes(&self) -> &[Node] { &self.nodes }
    pub fn edges(&self) -> &[Edge] { &self.edges }
    pub fn node(&self, id: u32) -> Option<&Node> { self.nodes.iter().find(|n| n.id == id) }
    pub fn edge(&self, id: u32) -> Option<&Edge> { self.edges.iter().find(|e| e.id == id) }

    pub fn new_node(&self, shape: Shape, center: [i32; 2]) -> Option<Node> {
        if self.nodes.len() >= NODE_LIMIT { return None }
        let id = (1..).find(|id| self.node(*id).is_none())?;
        Some(Node { id, shape, center: clamp_center(center), size: shape.size(), label: String::new() })
    }

    pub fn add_node(&mut self, shape: Shape, center: [i32; 2]) -> Option<u32> {
        let n = self.new_node(shape, center)?;
        let id = n.id;
        self.nodes.push(n);
        self.legacy = None;
        Some(id)
    }

    pub fn move_node(&mut self, id: u32, center: [i32; 2]) {
        let center = clamp_center(center);
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id && n.center != center) {
            n.center = center;
            self.legacy = None;
        }
    }

    pub fn resize_node(&mut self, id: u32, center: [i32; 2], size: [i32; 2]) {
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            let center = clamp_center(center);
            let size = n.shape.constrain_size(size);
            if (n.center, n.size) != (center, size) {
                (n.center, n.size) = (center, size);
                self.legacy = None;
            }
        }
    }

    pub fn translate(&mut self, ids: &[u32], mut delta: [i32; 2]) {
        for n in self.nodes.iter().filter(|n| ids.contains(&n.id)) {
            for (d, center) in delta.iter_mut().zip(n.center) {
                *d = (*d).clamp(-COORD_LIMIT - center, COORD_LIMIT - center);
            }
        }
        if delta == [0, 0] { return }
        for n in self.nodes.iter_mut().filter(|n| ids.contains(&n.id)) {
            n.center = [n.center[0] + delta[0], n.center[1] + delta[1]];
            self.legacy = None;
        }
    }

    pub fn arrange(&mut self, ids: &[u32], axis: Axis, distribute: bool) {
        let axis = axis.index();
        let mut nodes: Vec<_> = self.nodes.iter_mut().filter(|n| ids.contains(&n.id)).collect();
        if nodes.len() < if distribute { 3 } else { 2 } { return }
        nodes.sort_by_key(|n| (2 * n.center[axis] - n.size[axis], n.id));
        let mut start = nodes[0].bounds().min[axis];
        let gap = if distribute {
            let end = nodes.iter().map(|n| n.bounds().max[axis]).fold(start, f32::max);
            let sizes = nodes.iter().map(|n| n.size[axis] as f32).sum::<f32>();
            ((end - start - sizes) / (nodes.len() - 1) as f32).max(0.)
        } else { 0. };
        for n in nodes {
            let width = n.size[axis] as f32;
            let next = (start + width / 2.).round() as i32;
            if n.center[axis] != next {
                n.center[axis] = next.clamp(-COORD_LIMIT, COORD_LIMIT);
                self.legacy = None;
            }
            if distribute { start += width + gap; }
        }
    }

    pub fn label_node(&mut self, id: u32, label: &str) {
        if valid_label(label, LABEL_LIMIT, true)
            && let Some(n) = self.nodes.iter_mut().find(|n| n.id == id && n.label != label)
        {
            n.label = label.into();
            self.legacy = None;
        }
    }

    pub fn label_edge(&mut self, id: u32, label: &str) {
        if valid_label(label, EDGE_LABEL_LIMIT, false)
            && let Some(e) = self.edges.iter_mut().find(|e| e.id == id && e.label != label)
        {
            e.label = label.into();
            self.legacy = None;
        }
    }

    pub fn toggle_route(&mut self, id: u32) {
        if let Some(e) = self.edges.iter_mut().find(|e| e.id == id) {
            e.route = if e.route == Route::Straight { Route::Elbow } else { Route::Straight };
            self.legacy = None;
        }
    }

    pub fn connect(&mut self, from: u32, to: u32) -> Option<u32> {
        self.connect_ports(Endpoint { node: from, port: Port::Auto }, Endpoint { node: to, port: Port::Auto })
    }

    fn valid_connection(&self, from: Endpoint, to: Endpoint, except: Option<u32>) -> bool {
        from.node != to.node && self.node(from.node).is_some() && self.node(to.node).is_some()
            && !self.edges.iter().any(|e| Some(e.id) != except && e.from.node == from.node && e.to.node == to.node)
    }

    pub fn connect_ports(&mut self, from: Endpoint, to: Endpoint) -> Option<u32> {
        if self.edges.len() >= EDGE_LIMIT || !self.valid_connection(from, to, None) { return None }
        let id = (1..).find(|id| self.edge(*id).is_none())?;
        self.edges.push(Edge { id, from, to, route: Route::Straight, label: String::new(), seed: id.wrapping_mul(0x9e37_79b9) });
        self.legacy = None;
        Some(id)
    }

    pub fn reconnect(&mut self, id: u32, start: bool, endpoint: Endpoint) -> bool {
        let Some(e) = self.edge(id) else { return false };
        let (from, to) = if start { (endpoint, e.to) } else { (e.from, endpoint) };
        if (from, to) == (e.from, e.to) || !self.valid_connection(from, to, Some(id)) { return false }
        let edge = self.edges.iter_mut().find(|e| e.id == id).unwrap();
        (edge.from, edge.to) = (from, to);
        self.legacy = None;
        true
    }

    pub fn remove(&mut self, object: Object) {
        let before = (self.nodes.len(), self.edges.len());
        match object {
            Object::Node(id) => {
                self.nodes.retain(|n| n.id != id);
                self.edges.retain(|e| e.from.node != id && e.to.node != id);
            }
            Object::Edge(id) => self.edges.retain(|e| e.id != id),
        }
        if before != (self.nodes.len(), self.edges.len()) { self.legacy = None; }
    }

    pub fn arrow(&self, edge: &Edge) -> Option<Arrow> {
        let from = self.node(edge.from.node)?;
        let to = self.node(edge.to.node)?;
        let dx = (to.center[0] - from.center[0]) as f32;
        let dy = (to.center[1] - from.center[1]) as f32;
        let length = dx.hypot(dy);
        if length == 0. { return None }
        let normal = [dx / length, dy / length];
        let back = [-normal[0], -normal[1]];
        let ports = (edge.from.port, edge.to.port);
        let points = match edge.route {
            Route::Straight => {
                let tail = from.anchor(ports.0.direction().unwrap_or(normal));
                let tip = to.anchor(ports.1.direction().unwrap_or(back));
                if ports == (Port::Auto, Port::Auto)
                    && (tip[0] - tail[0]) * normal[0] + (tip[1] - tail[1]) * normal[1] <= 0. { return None }
                vec![tail, tip]
            }
            Route::Elbow => {
                let out = ports.0.direction().unwrap_or_else(|| Port::facing(normal).direction().unwrap());
                let into = ports.1.direction().unwrap_or_else(|| Port::facing(back).direction().unwrap());
                elbow(from, to, from.anchor(out), to.anchor(into), out, into)
            }
        };
        let tip = *points.last()?;
        let before = points.iter().rev().skip(1).find(|&&p| p != tip)?;
        let [dx, dy] = [tip[0] - before[0], tip[1] - before[1]];
        let last = dx.hypot(dy);
        let [x, y] = [dx / last, dy / last];
        let head = last.min(13.);
        let base = [tip[0] - x * head, tip[1] - y * head];
        let spread = head * 0.42;
        Some(Arrow { points, wings: [[base[0] - y * spread, base[1] + x * spread], [base[0] + y * spread, base[1] - x * spread]] })
    }

    pub fn node_at(&self, point: Coord) -> Option<u32> {
        self.nodes.iter().rev().find(|n| n.hit(point)).map(|n| n.id)
    }

    pub fn retained_bytes(&self) -> usize {
        self.nodes.iter().map(|n| std::mem::size_of::<Node>() + n.label.len()).sum::<usize>()
            + self.edges.iter().map(|e| std::mem::size_of::<Edge>() + e.label.len()).sum::<usize>()
            + self.legacy.as_ref().map_or(0, |r| r.len())
    }
}

fn clamp_center(center: [i32; 2]) -> [i32; 2] { center.map(|v| v.clamp(-COORD_LIMIT, COORD_LIMIT)) }

fn valid_label(label: &str, limit: usize, multiline: bool) -> bool {
    label.chars().count() <= limit && label.lines().count() <= 8
        && !label.chars().any(|ch| ch.is_control() && !(multiline && ch == '\n'))
}
