//! Semantic Excalidraw whiteboard / flowchart compiler and PNG renderer.
//! Compiles high-level nodes, edges, frames and sticky notes into native
//! `fdl_core::whiteboard::Board` using `ApproxMeasure`, `text::redraw_text_bounding_box`,
//! `binding::bind_binding_element` and `elbow::update_elbow_arrow_route`.

use crate::output::CliFailure;
use fdl_core::board::binding;
use fdl_core::board::consts::font_family;
use fdl_core::board::elbow;
use fdl_core::board::fractional_index::sync_invalid_indices;
use fdl_core::board::new_element::*;
use fdl_core::board::scene::Scene;
use fdl_core::board::text;
use fdl_core::board::text_measure::{Font, measure_text};
use fdl_core::board::types::*;
use fdl_core::whiteboard::{ApproxMeasure, Board};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DiagramSpec {
    pub layout: String, // "flow-lr" | "flow-tb" | "grid" | "manual"
    pub theme: String,  // "craft-pastel" | "slate" | "warm" | "contrast"
    pub background: Option<String>,
    pub height: f32,
    pub gap_x: f64,
    pub gap_y: f64,
    pub cols: usize,
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
    pub frames: Vec<FrameSpec>,
}

impl Default for DiagramSpec {
    fn default() -> Self {
        Self {
            layout: "flow-lr".into(),
            theme: "craft-pastel".into(),
            background: None,
            height: 400.,
            gap_x: 90.,
            gap_y: 56.,
            cols: 3,
            nodes: vec![],
            edges: vec![],
            frames: vec![],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct NodeSpec {
    pub id: String,
    pub shape: String, // "rectangle" | "diamond" | "ellipse" | "sticky" | "text"
    pub label: String,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub stroke_width: Option<f64>,
    pub stroke_style: Option<String>,
    pub fill_style: Option<String>,
    pub rounded: Option<bool>,
    pub roughness: Option<f64>,
    pub opacity: Option<f64>,
    pub font_size: Option<f64>,
    pub text_align: Option<String>,
    pub group: Option<String>,
}

impl Default for NodeSpec {
    fn default() -> Self {
        Self {
            id: String::new(),
            shape: "rectangle".into(),
            label: String::new(),
            x: None, y: None, width: None, height: None,
            fill: None, stroke: None, stroke_width: None, stroke_style: None,
            fill_style: None, rounded: None, roughness: None, opacity: None,
            font_size: None, text_align: None, group: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub style: String, // "elbow" | "straight" | "curved"
    pub stroke: Option<String>,
    pub stroke_width: Option<f64>,
    pub stroke_style: Option<String>,
    pub start_arrow: Option<String>,
    pub end_arrow: Option<String>,
}

impl Default for EdgeSpec {
    fn default() -> Self {
        Self {
            from: String::new(),
            to: String::new(),
            label: None,
            style: "elbow".into(),
            stroke: None,
            stroke_width: None,
            stroke_style: None,
            start_arrow: None,
            end_arrow: Some("arrow".into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct FrameSpec {
    pub id: Option<String>,
    #[serde(alias = "label")]
    pub name: String,
    #[serde(alias = "node_ids")]
    pub includes: Vec<String>,
    pub padding: Option<f64>,
}

struct ThemeSwatch {
    fills: &'static [&'static str],
    stroke: &'static str,
    sticky: &'static str,
    roughness: f64,
}

fn palette_for(name: &str) -> Result<ThemeSwatch, CliFailure> {
    match name.trim().to_ascii_lowercase().as_str() {
        "craft-pastel" | "" => Ok(ThemeSwatch {
            fills: &["#a5d8ff", "#b2f2bb", "#ffec99", "#d0bfff", "#ffd8a8", "#ffc9c9"],
            stroke: "#1e1e1e",
            sticky: "#ffdf6b",
            roughness: 0.,
        }),
        "slate" => Ok(ThemeSwatch {
            fills: &["#e7f5ff", "#f1f3f5", "#edf2ff", "#e6fcf5"],
            stroke: "#343a40",
            sticky: "#fff3bf",
            roughness: 0.,
        }),
        "warm" => Ok(ThemeSwatch {
            fills: &["#ffe8cc", "#fff3bf", "#ffd8a8", "#ffc9c9"],
            stroke: "#493023",
            sticky: "#ffec99",
            roughness: 1.,
        }),
        "contrast" => Ok(ThemeSwatch {
            fills: &["#4dabf7", "#51cf66", "#fcc419", "#ff6b6b", "#9775fa"],
            stroke: "#111827",
            sticky: "#ffe066",
            roughness: 0.,
        }),
        other => Err(CliFailure::new("INVALID_BOARD_THEME", format!("Unknown board theme '{other}'."))
            .with_valid(["craft-pastel", "slate", "warm", "contrast"])),
    }
}

fn parse_stroke_style(s: Option<&str>) -> Result<StrokeStyle, CliFailure> {
    match s.unwrap_or("solid").to_ascii_lowercase().as_str() {
        "solid" => Ok(StrokeStyle::Solid),
        "dashed" => Ok(StrokeStyle::Dashed),
        "dotted" => Ok(StrokeStyle::Dotted),
        other => Err(CliFailure::new("INVALID_STROKE_STYLE", format!("Unknown stroke_style '{other}'.")).with_valid(["solid", "dashed", "dotted"])),
    }
}

fn parse_fill_style(s: Option<&str>) -> Result<FillStyle, CliFailure> {
    match s.unwrap_or("solid").to_ascii_lowercase().as_str() {
        "solid" => Ok(FillStyle::Solid),
        "hachure" => Ok(FillStyle::Hachure),
        "cross-hatch" => Ok(FillStyle::CrossHatch),
        "zigzag" => Ok(FillStyle::Zigzag),
        other => Err(CliFailure::new("INVALID_FILL_STYLE", format!("Unknown fill_style '{other}'.")).with_valid(["solid", "hachure", "cross-hatch", "zigzag"])),
    }
}

fn parse_arrowhead(s: Option<&str>) -> Result<Option<Arrowhead>, CliFailure> {
    let Some(s) = s else { return Ok(None) };
    if s.eq_ignore_ascii_case("none") || s.is_empty() { return Ok(None); }
    Arrowhead::parse(&s.to_ascii_lowercase()).map(Some).ok_or_else(|| {
        CliFailure::new("INVALID_ARROWHEAD", format!("Unknown arrowhead '{s}'."))
            .with_valid(["none", "arrow", "triangle", "triangle_outline", "bar", "circle", "circle_outline", "diamond", "diamond_outline"])
    })
}

fn parse_align(s: Option<&str>) -> TextAlign {
    match s.unwrap_or("center").to_ascii_lowercase().as_str() {
        "left" => TextAlign::Left,
        "right" => TextAlign::Right,
        _ => TextAlign::Center,
    }
}

pub fn compile_diagram(spec: &DiagramSpec) -> Result<Board, CliFailure> {
    let swatch = palette_for(&spec.theme)?;
    if spec.nodes.is_empty() && spec.frames.is_empty() {
        return Err(CliFailure::new("EMPTY_BOARD", "Diagram must contain at least one node."));
    }
    let mut seen_ids = HashSet::new();
    for (idx, n) in spec.nodes.iter().enumerate() {
        if n.id.trim().is_empty() {
            return Err(CliFailure::new("INVALID_NODE", format!("Node at index {idx} has an empty id.")));
        }
        if !seen_ids.insert(n.id.clone()) {
            return Err(CliFailure::new("DUPLICATE_NODE_ID", format!("Duplicate node id '{}'.", n.id)));
        }
    }
    for e in &spec.edges {
        if !seen_ids.contains(&e.from) {
            return Err(CliFailure::new("UNKNOWN_EDGE_NODE", format!("Edge references unknown source node '{}'.", e.from)));
        }
        if !seen_ids.contains(&e.to) {
            return Err(CliFailure::new("UNKNOWN_EDGE_NODE", format!("Edge references unknown target node '{}'.", e.to)));
        }
    }

    // 1. Measure intrinsic size of each node using `ApproxMeasure`, accounting for container geometry
    // (ellipse / diamond inscribed text boxes) so final layout & layer centering match `redraw_text_bounding_box`.
    let m = ApproxMeasure;
    let mut sizes: Vec<(f64, f64)> = Vec::with_capacity(spec.nodes.len());
    for n in &spec.nodes {
        let fsize = n.font_size.unwrap_or(if n.shape == "sticky" { 18. } else { 20. });
        let font = Font::new(fsize, font_family::EXCALIFONT);
        let met = measure_text(&m, &n.label, font, 1.25);
        let (min_w, min_h, pad_x, pad_y) = match n.shape.as_str() {
            "diamond" => (140., 92., 56., 44.),
            "ellipse" => (150., 76., 48., 32.),
            "sticky" => (150., 140., 32., 48.),
            "text" => (20., 24., 4., 4.),
            _ => (160., 68., 36., 24.),
        };
        let mut w = n.width.unwrap_or_else(|| (met.width + pad_x).max(min_w).ceil());
        let mut h = n.height.unwrap_or_else(|| (met.height + pad_y).max(min_h).ceil());
        if !n.label.trim().is_empty() && matches!(n.shape.as_str(), "rectangle" | "diamond" | "ellipse" | "sticky") && (n.width.is_none() || n.height.is_none()) {
            let e_type = match n.shape.as_str() {
                "diamond" => ElementType::Diamond,
                "ellipse" => ElementType::Ellipse,
                _ => ElementType::Rectangle,
            };
            let mut probe_shape = if n.shape == "sticky" {
                new_sticky_note_element(ElementOpts { width: w, height: h, ..Default::default() }, Some(h))
            } else {
                new_element(e_type, ElementOpts { width: w, height: h, ..Default::default() })
            };
            let cid = probe_shape.id.clone();
            let probe_text = new_text_element(
                TextOpts {
                    text: n.label.clone(),
                    font_size: Some(fsize),
                    font_family: Some(font_family::EXCALIFONT),
                    text_align: Some(parse_align(n.text_align.as_deref())),
                    vertical_align: Some(VerticalAlign::Middle),
                    container_id: Some(cid.clone()),
                    base_font_size: (n.shape == "sticky").then_some(18.),
                    ..Default::default()
                },
                ElementOpts { x: w / 2., y: h / 2., ..Default::default() },
                &m,
            );
            probe_shape.bound_elements = Some(vec![BoundElement { id: probe_text.id.clone(), kind: BoundType::Text }]);
            let mut probe_scene = Scene::new(vec![probe_shape, probe_text.clone()]);
            text::redraw_text_bounding_box(&mut probe_scene, &probe_text, Some(&cid), &m);
            if let Some(updated) = probe_scene.get_element(&cid) {
                if n.width.is_none() { w = w.max(updated.width).ceil(); }
                if n.height.is_none() { h = h.max(updated.height).ceil(); }
            }
        }
        sizes.push((w, h));
    }

    // 2. Compute node coordinates (x, y) according to layout.
    let mut pos: Vec<(f64, f64)> = vec![(0., 0.); spec.nodes.len()];
    let id_to_ix: HashMap<&str, usize> = spec.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    match spec.layout.as_str() {
        "manual" => {
            for (i, n) in spec.nodes.iter().enumerate() {
                pos[i] = (n.x.unwrap_or(i as f64 * 220.), n.y.unwrap_or(0.));
            }
        }
        "grid" => {
            let cols = spec.cols.max(1);
            let mut col_w = vec![0.0f64; cols];
            let mut row_h = vec![0.0f64; spec.nodes.len().div_ceil(cols)];
            for (i, (w, h)) in sizes.iter().enumerate() {
                col_w[i % cols] = col_w[i % cols].max(*w);
                row_h[i / cols] = row_h[i / cols].max(*h);
            }
            for (i, n) in spec.nodes.iter().enumerate() {
                let r = i / cols;
                let c = i % cols;
                let cx: f64 = col_w[..c].iter().sum::<f64>() + c as f64 * spec.gap_x;
                let cy: f64 = row_h[..r].iter().sum::<f64>() + r as f64 * spec.gap_y;
                pos[i] = (n.x.unwrap_or(cx), n.y.unwrap_or(cy));
            }
        }
        "flow-lr" | "flow-tb" => {
            let lr = spec.layout == "flow-lr";
            let n_nodes = spec.nodes.len();
            let mut indeg = vec![0usize; n_nodes];
            let mut adj: Vec<Vec<usize>> = vec![vec![]; n_nodes];
            for e in &spec.edges {
                let u = id_to_ix[e.from.as_str()];
                let v = id_to_ix[e.to.as_str()];
                if u != v {
                    adj[u].push(v);
                    indeg[v] += 1;
                }
            }
            let mut layer = vec![0usize; n_nodes];
            let mut q: VecDeque<usize> = (0..n_nodes).filter(|&i| indeg[i] == 0).collect();
            let mut visited = 0usize;
            while let Some(u) = q.pop_front() {
                visited += 1;
                for &v in &adj[u] {
                    layer[v] = layer[v].max(layer[u] + 1);
                    indeg[v] -= 1;
                    if indeg[v] == 0 { q.push_back(v); }
                }
            }
            if visited < n_nodes {
                for i in 0..n_nodes {
                    if indeg[i] > 0 { layer[i] = i; }
                }
            }
            let max_layer = layer.iter().copied().max().unwrap_or(0);
            let mut layers: Vec<Vec<usize>> = vec![vec![]; max_layer + 1];
            for (i, &l) in layer.iter().enumerate() { layers[l].push(i); }

            let mut primary_max = vec![0.0f64; layers.len()];
            let mut cross_total = vec![0.0f64; layers.len()];
            for (l, group) in layers.iter().enumerate() {
                for (k, &i) in group.iter().enumerate() {
                    let (w, h) = sizes[i];
                    let (p, c) = if lr { (w, h) } else { (h, w) };
                    primary_max[l] = primary_max[l].max(p);
                    cross_total[l] += c + if k > 0 { if lr { spec.gap_y } else { spec.gap_x } } else { 0. };
                }
            }
            let max_cross = cross_total.iter().copied().fold(0.0f64, f64::max);
            let mut p_cursor = 0.0f64;
            let p_gap = if lr { spec.gap_x } else { spec.gap_y };
            let c_gap = if lr { spec.gap_y } else { spec.gap_x };
            let mut layer_gaps = vec![p_gap; layers.len()];
            if lr {
                let lbl_font = Font::new(16., font_family::EXCALIFONT);
                for e in &spec.edges {
                    if let Some(lbl) = e.label.as_deref().filter(|s| !s.trim().is_empty()) {
                        let lu = layer[id_to_ix[e.from.as_str()]];
                        let lv = layer[id_to_ix[e.to.as_str()]];
                        if lv > lu && let Some(gap) = layer_gaps.get_mut(lu) {
                            let lmet = measure_text(&m, lbl, lbl_font, 1.25);
                            // Keep room for text mask plus clean arrow shaft & arrowhead on both ends.
                            *gap = gap.max(lmet.width + 72.);
                        }
                    }
                }
            }
            for (l, group) in layers.iter().enumerate() {
                let mut c_cursor = (max_cross - cross_total[l]) / 2.0;
                for &i in group {
                    let (w, h) = sizes[i];
                    let (px, py) = if lr {
                        (p_cursor + (primary_max[l] - w) / 2.0, c_cursor)
                    } else {
                        (c_cursor, p_cursor + (primary_max[l] - h) / 2.0)
                    };
                    pos[i] = (spec.nodes[i].x.unwrap_or(px), spec.nodes[i].y.unwrap_or(py));
                    c_cursor += if lr { h + c_gap } else { w + c_gap };
                }
                p_cursor += primary_max[l] + layer_gaps[l];
            }
        }
        other => {
            return Err(CliFailure::new("INVALID_LAYOUT", format!("Unknown board layout '{other}'."))
                .with_valid(["flow-lr", "flow-tb", "grid", "manual"]));
        }
    }

    // 3. Create Excalidraw elements for nodes (+ bound text labels).
    let mut elements: Vec<Element> = vec![];
    let mut node_el_ids: HashMap<String, String> = HashMap::new();
    let mut text_pairs: Vec<(String, String)> = vec![];

    for (i, n) in spec.nodes.iter().enumerate() {
        let (x, y) = pos[i];
        let (w, h) = sizes[i];
        let default_fill = if n.shape == "sticky" {
            swatch.sticky
        } else if n.shape == "text" {
            "transparent"
        } else {
            swatch.fills[i % swatch.fills.len()]
        };
        let fill = n.fill.as_deref().unwrap_or(default_fill).to_string();
        let stroke = n.stroke.as_deref().unwrap_or(swatch.stroke).to_string();
        let rounded = n.rounded.unwrap_or(true);
        let roundness = match n.shape.as_str() {
            "rectangle" if rounded => Some(Roundness { kind: roundness::ADAPTIVE_RADIUS, value: None }),
            "diamond" | "sticky" if rounded => Some(Roundness { kind: roundness::PROPORTIONAL_RADIUS, value: None }),
            _ => None,
        };
        let opts = ElementOpts {
            x, y, width: w, height: h,
            stroke_color: stroke.clone(),
            background_color: fill,
            fill_style: parse_fill_style(n.fill_style.as_deref())?,
            stroke_width: n.stroke_width.unwrap_or(2.),
            stroke_style: parse_stroke_style(n.stroke_style.as_deref())?,
            roundness,
            roughness: n.roughness.unwrap_or(swatch.roughness),
            opacity: n.opacity.unwrap_or(100.),
            ..Default::default()
        };
        if n.shape == "text" {
            let t = new_text_element(
                TextOpts {
                    text: n.label.clone(),
                    font_size: n.font_size.or(Some(22.)),
                    font_family: Some(font_family::EXCALIFONT),
                    text_align: Some(parse_align(n.text_align.as_deref().or(Some("left")))),
                    vertical_align: Some(VerticalAlign::Top),
                    ..Default::default()
                },
                opts,
                &m,
            );
            node_el_ids.insert(n.id.clone(), t.id.clone());
            elements.push(t);
            continue;
        }
        let shape_el = match n.shape.as_str() {
            "rectangle" => new_element(ElementType::Rectangle, opts),
            "diamond" => new_element(ElementType::Diamond, opts),
            "ellipse" => new_element(ElementType::Ellipse, opts),
            "sticky" => new_sticky_note_element(opts, Some(h)),
            other => {
                return Err(CliFailure::new("INVALID_SHAPE", format!("Unknown node shape '{other}'."))
                    .with_valid(["rectangle", "diamond", "ellipse", "sticky", "text"]));
            }
        };
        let cid = shape_el.id.clone();
        node_el_ids.insert(n.id.clone(), cid.clone());
        elements.push(shape_el);

        if !n.label.trim().is_empty() {
            let mut to = ElementOpts {
                x: x + w / 2., y: y + h / 2.,
                stroke_color: stroke,
                roughness: n.roughness.unwrap_or(swatch.roughness),
                ..Default::default()
            };
            to.background_color = "transparent".into();
            let t_el = new_text_element(
                TextOpts {
                    text: n.label.clone(),
                    font_size: n.font_size.or(Some(if n.shape == "sticky" { 18. } else { 20. })),
                    font_family: Some(font_family::EXCALIFONT),
                    text_align: Some(parse_align(n.text_align.as_deref())),
                    vertical_align: Some(VerticalAlign::Middle),
                    container_id: Some(cid.clone()),
                    base_font_size: (n.shape == "sticky").then_some(18.),
                    ..Default::default()
                },
                to,
                &m,
            );
            text_pairs.push((cid, t_el.id.clone()));
            elements.push(t_el);
        }
    }

    // 4. Add frames wrapping included nodes.
    let mut frame_assign: HashMap<String, String> = HashMap::new();
    for f in &spec.frames {
        let pad = f.padding.unwrap_or(28.);
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut matched_ids = vec![];
        for nid in &f.includes {
            let Some(el_id) = node_el_ids.get(nid) else {
                return Err(CliFailure::new("UNKNOWN_FRAME_NODE", format!("Frame '{}' includes unknown node '{}'.", f.name, nid)));
            };
            let el = elements.iter().find(|e| e.id == *el_id).unwrap();
            min_x = min_x.min(el.x);
            min_y = min_y.min(el.y);
            max_x = max_x.max(el.x + el.width);
            max_y = max_y.max(el.y + el.height);
            matched_ids.push(el_id.clone());
        }
        if matched_ids.is_empty() { continue; }
        let frame_el = new_frame_element(
            ElementOpts {
                id: f.id.clone(),
                x: min_x - pad,
                y: min_y - pad - 10.,
                width: (max_x - min_x) + pad * 2.,
                height: (max_y - min_y) + pad * 2. + 10.,
                stroke_color: "#bbb".into(),
                roughness: 0.,
                ..Default::default()
            },
            Some(f.name.clone()),
        );
        let fid = frame_el.id.clone();
        for eid in matched_ids { frame_assign.insert(eid, fid.clone()); }
        elements.push(frame_el);
    }
    for el in &mut elements {
        let target = el.container_id().map(str::to_string).unwrap_or_else(|| el.id.clone());
        if let Some(fid) = frame_assign.get(&target) && !el.is_frame() {
            el.frame_id = Some(fid.clone());
        }
    }

    // 5. Put into Scene, align all bound container labels, then bind arrows.
    let mut scene = Scene::new(elements);
    for (cid, tid) in &text_pairs {
        if let Some(c) = scene.get_element(cid).cloned() {
            let bound = {
                let mut v = c.bound_elements.clone().unwrap_or_default();
                v.push(BoundElement { id: tid.clone(), kind: BoundType::Text });
                Some(v)
            };
            scene.mutate_element(cid, false, |e| e.bound_elements = bound);
        }
        if let Some(t) = scene.get_element(tid).cloned() {
            text::redraw_text_bounding_box(&mut scene, &t, Some(cid), &m);
        }
    }

    for edge in &spec.edges {
        let from_id = &node_el_ids[&edge.from];
        let to_id = &node_el_ids[&edge.to];
        let s = scene.get_element(from_id).unwrap().clone();
        let t = scene.get_element(to_id).unwrap().clone();
        let (scx, scy) = (s.x + s.width / 2., s.y + s.height / 2.);
        let (tcx, tcy) = (t.x + t.width / 2., t.y + t.height / 2.);
        let (dx, dy) = (tcx - scx, tcy - scy);
        const PAD: f64 = 6.;
        let (sx, sy, ex, ey, s_fp, e_fp) = if dx.abs() >= dy.abs() {
            if dx >= 0. {
                (s.x + s.width + PAD, scy, t.x - PAD, tcy, [1., 0.5], [0., 0.5])
            } else {
                (s.x - PAD, scy, t.x + t.width + PAD, tcy, [0., 0.5], [1., 0.5])
            }
        } else if dy >= 0. {
            (scx, s.y + s.height + PAD, tcx, t.y - PAD, [0.5, 1.], [0.5, 0.])
        } else {
            (scx, s.y - PAD, tcx, t.y + t.height + PAD, [0.5, 0.], [0.5, 1.])
        };
        let elbowed = edge.style.eq_ignore_ascii_case("elbow");
        let curved = edge.style.eq_ignore_ascii_case("curved");
        let pts = if curved {
            let mx = (ex - sx) / 2.;
            let my = (ey - sy) / 2.;
            let bend = if dx.abs() >= dy.abs() { [mx, my - 28.] } else { [mx + 28., my] };
            vec![[0., 0.], bend, [ex - sx, ey - sy]]
        } else {
            vec![[0., 0.], [ex - sx, ey - sy]]
        };
        let arrow = new_arrow_element(
            ElementOpts {
                x: sx, y: sy,
                stroke_color: edge.stroke.as_deref().unwrap_or(swatch.stroke).into(),
                stroke_width: edge.stroke_width.unwrap_or(2.),
                stroke_style: parse_stroke_style(edge.stroke_style.as_deref())?,
                roundness: (!elbowed).then_some(Roundness { kind: roundness::PROPORTIONAL_RADIUS, value: None }),
                roughness: swatch.roughness,
                ..Default::default()
            },
            pts,
            parse_arrowhead(edge.start_arrow.as_deref())?,
            parse_arrowhead(edge.end_arrow.as_deref())?,
            elbowed,
            None,
        );
        let aid = arrow.id.clone();
        let mut all = scene.elements_including_deleted().to_vec();
        all.push(arrow);
        scene.replace_all_elements(all);
        binding::bind_binding_element_to_fixed_point(&aid, from_id, StartOrEnd::Start, s_fp, &mut scene);
        binding::bind_binding_element_to_fixed_point(&aid, to_id, StartOrEnd::End, e_fp, &mut scene);
        if elbowed && let Some(ar) = scene.get_element(&aid).cloned() {
            let u = elbow::update_elbow_arrow_route(&ar, &scene);
            scene.mutate_element(&aid, false, |e| u.apply(e));
        }
        if let Some(lbl) = edge.label.as_deref().filter(|l| !l.trim().is_empty()) {
            let ar = scene.get_element(&aid).unwrap().clone();
            let lt = new_text_element(
                TextOpts {
                    text: lbl.into(),
                    font_size: Some(16.),
                    font_family: Some(font_family::EXCALIFONT),
                    text_align: Some(TextAlign::Center),
                    vertical_align: Some(VerticalAlign::Middle),
                    container_id: Some(aid.clone()),
                    label_position: Some(0.5),
                    ..Default::default()
                },
                ElementOpts {
                    x: ar.x + ar.width / 2., y: ar.y + ar.height / 2.,
                    stroke_color: edge.stroke.as_deref().unwrap_or(swatch.stroke).into(),
                    background_color: "transparent".into(),
                    roughness: swatch.roughness,
                    ..Default::default()
                },
                &m,
            );
            let ltid = lt.id.clone();
            let mut all = scene.elements_including_deleted().to_vec();
            all.push(lt);
            scene.replace_all_elements(all);
            scene.mutate_element(&aid, false, |e| {
                let mut v = e.bound_elements.clone().unwrap_or_default();
                v.push(BoundElement { id: ltid.clone(), kind: BoundType::Text });
                e.bound_elements = Some(v);
            });
            if let Some(lte) = scene.get_element(&ltid).cloned() {
                text::redraw_text_bounding_box(&mut scene, &lte, Some(&aid), &m);
            }
        }
    }

    let mut final_els: Vec<Element> = scene.non_deleted_elements().cloned().collect();
    sync_invalid_indices(&mut final_els);
    let mut bd = Board { elements: final_els, height: spec.height.max(120.), ..Default::default() };
    if let Some(bg) = &spec.background {
        bd.app.view_background_color = bg.clone();
    }
    // Round-trip through Board::from_scene to guarantee desktop compatibility.
    let verified = Board::from_scene(&bd.scene_json(), bd.height).ok_or_else(|| {
        CliFailure::new("BOARD_SERIALIZE_ERROR", "Generated board scene failed Excalidraw round-trip validation.")
    })?;
    Ok(Board { app: bd.app, ..verified })
}

/// Rasterize an SVG string to PNG bytes using `resvg` + system fonts.
pub fn render_svg_to_png(svg: &str, scale: f32) -> Result<Vec<u8>, CliFailure> {
    let mut opt = resvg::usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &opt)
        .map_err(|e| CliFailure::new("SVG_PARSE_ERROR", format!("Failed parsing SVG: {e}")))?;
    let sz = tree.size().to_int_size();
    let w = ((sz.width() as f32) * scale).ceil().max(1.) as u32;
    let h = ((sz.height() as f32) * scale).ceil().max(1.) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| CliFailure::new("PNG_ALLOC_ERROR", "Failed allocating PNG pixel buffer."))?;
    let ts = resvg::tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, ts, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| CliFailure::new("PNG_ENCODE_ERROR", format!("Failed encoding PNG: {e}")))
}

pub fn load_board_file(path: &Path) -> Result<Board, CliFailure> {
    let text = std::fs::read_to_string(path).map_err(|e| CliFailure::new("BOARD_READ_ERROR", format!("Failed reading {}: {e}", path.display())))?;
    Board::from_file(&text).map_err(|e| CliFailure::new("BOARD_PARSE_ERROR", format!("Invalid .excalidraw JSON: {e}")))
}
