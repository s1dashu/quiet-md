//! Pointer interactions remain previews until committed through Core.

mod paint;

use super::{render::R, view::Editor};
use crate::canvas as ink;
use crate::editor_theme::Theme;
use crate::editor_ui::{TipExt, icon};
use crate::input::{TextField, TextFieldEvent};
use crate::model::{Align, BlockId, BlockKind};
use crate::settings::{text, tr};
use fdl_core::canvas::{Axis, Coord, Drawing, EDGE_LABEL_LIMIT, Endpoint, LABEL_LIMIT, Node, Object, Port, Rect, Route, Shape};
use gpui::{prelude::FluentBuilder, *};
use paint::{Cache, Camera, Marks, Scene, Viewport};
use std::{cell::{Cell, RefCell}, rc::Rc, time::Instant};

actions!(drawing_canvas, [CancelCanvas, DeleteCanvasObject, UndoCanvas, RedoCanvas]);

pub fn bind_keys(cx: &mut App) {
    let context = Some("DrawingCanvas");
    cx.bind_keys([
        KeyBinding::new("escape", CancelCanvas, context),
        KeyBinding::new("backspace", DeleteCanvasObject, context),
        KeyBinding::new("delete", DeleteCanvasObject, context),
        KeyBinding::new("cmd-z", UndoCanvas, context),
        KeyBinding::new("cmd-shift-z", RedoCanvas, context),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool { Select, Shape(Shape), Connect }

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Self::Select => tr("选择", "Select"),
            Self::Shape(Shape::Rectangle) => tr("矩形", "Rectangle"),
            Self::Shape(Shape::Circle) => tr("圆形", "Circle"),
            Self::Shape(Shape::Diamond) => tr("菱形", "Diamond"),
            Self::Shape(Shape::Text) => tr("文字", "Text"),
            Self::Connect => tr("连线", "Arrow"),
        }
    }
}

#[derive(Clone, Copy)]
struct Placement { shape: Shape, center: [i32; 2], from: Option<Endpoint> }

impl Placement {
    fn add(self, drawing: &mut Drawing) -> Option<u32> {
        let id = drawing.add_node(self.shape, self.center)?;
        if let Some(from) = self.from {
            let port = if from.port == Port::Left { Port::Right } else { Port::Left };
            if drawing.connect_ports(from, Endpoint { node: id, port }).is_none() {
                drawing.remove(Object::Node(id));
                return None;
            }
        }
        Some(id)
    }
}

#[derive(Clone, Copy)]
struct LabelEdit { target: Object, placement: Option<Placement> }

#[derive(Clone)]
enum Gesture {
    Pan { at: Point<Pixels>, camera: Camera },
    Move { at: Coord, delta: [i32; 2], guides: [Option<f32>; 2] },
    Resize { at: Coord, corner: Coord, fixed: Coord, sign: Coord, id: u32, center: [i32; 2], size: [i32; 2] },
    Marquee { at: Coord, rect: Rect, before: Vec<u32> },
    Link { origin: Endpoint, rebind: Option<(u32, bool)>, cursor: Coord, target: Option<Endpoint> },
}

impl Gesture {
    fn update_drawing(&self, drawing: &mut Drawing, selected: &[u32]) {
        match self {
            Self::Move { delta, .. } => drawing.translate(selected, *delta),
            Self::Resize { id, center, size, .. } => drawing.resize_node(*id, *center, *size),
            _ => {}
        }
    }
}

pub(super) struct CanvasEdit {
    id: BlockId,
    tool: Tool,
    nodes: Vec<u32>,
    edge: Option<u32>,
    from: Option<Endpoint>,
    gesture: Option<Gesture>,
    space: bool,
    hover: Option<Point<Pixels>>,
    camera: Option<Camera>,
    viewport: Rc<Cell<Option<Viewport>>>,
    cache: Rc<RefCell<Cache>>,
    focus: FocusHandle,
    label: Entity<TextField>,
    label_edit: Option<LabelEdit>,
    landing: Option<Instant>,
    _subscriptions: [Subscription; 3],
}

impl CanvasEdit {
    fn select(&mut self, object: Option<Object>) {
        self.nodes.clear();
        self.edge = None;
        match object {
            Some(Object::Node(id)) => self.nodes.push(id),
            Some(Object::Edge(id)) => self.edge = Some(id),
            None => {}
        }
    }

    fn view(&self) -> Option<Viewport> {
        let view = self.viewport.get()?;
        Some(self.camera.map_or(view, |camera| Viewport::new(view.bounds, camera)))
    }

    fn freeze(&mut self) {
        if self.camera.is_none() { self.camera = self.viewport.get().map(|v| v.camera); }
    }
}

fn tool_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div().id(id).tab_index(0).px(px(8.)).h(px(29.)).flex().items_center().rounded(px(5.))
        .cursor_pointer().text_size(px(12.)).text_color(Theme::text()).hover(|d| d.bg(Theme::fill_hover()))
        .focus(|d| d.bg(Theme::fill_turninto_active())).child(label)
}

fn icon_button(id: &'static str, active: bool, enabled: bool) -> Stateful<Div> {
    div().id(id).w(px(36.)).h(px(34.)).flex_none().flex().items_center().justify_center().rounded(px(6.))
        .when(enabled, |d| d.tab_index(0).cursor_pointer().hover(|d| d.bg(Theme::fill_hover())).focus(|d| d.bg(Theme::fill_turninto_active())))
        .when(active, |d| d.bg(Theme::fill_turninto_active()))
}

fn distance(a: Coord, b: Coord) -> f32 { (a[0] - b[0]).hypot(a[1] - b[1]) }

fn edge_at(drawing: &Drawing, cache: &mut Cache, at: Coord, tolerance: f32) -> Option<u32> {
    drawing.edges().iter().rev().find(|e| cache.wire(drawing, e).is_some_and(|w| {
        w.arrow.hit(at, tolerance) || (!e.label.is_empty() && ink::edge_label_bounds(&w.arrow, &e.label).contains(at))
    })).map(|e| e.id)
}

fn facing_port(node: &Node, at: Coord) -> Port {
    let [x, y] = node.position();
    let [dx, dy] = [at[0] - x, at[1] - y];
    if dx.abs() / node.size[0] as f32 > dy.abs() / node.size[1] as f32 {
        if dx > 0. { Port::Right } else { Port::Left }
    } else if dy > 0. { Port::Bottom } else { Port::Top }
}

fn link_target(drawing: &Drawing, from: Endpoint, at: Coord, tolerance: f32) -> Option<Endpoint> {
    let n = drawing.nodes().iter().rev().find(|n| n.id != from.node && n.bounds().pad(tolerance).contains(at))?;
    let aim = if distance(n.position(), at) <= tolerance { drawing.node(from.node)?.position() } else { at };
    Some(Endpoint { node: n.id, port: facing_port(n, aim) })
}

fn downstream(drawing: &Drawing, node: &Node) -> Option<Placement> {
    let [w, h] = Shape::Rectangle.size();
    let [x, y] = node.center;
    let gap = node.size[0] / 2 + 56 + w / 2;
    for (x, port) in [(x + gap, Port::Right), (x - gap, Port::Left)] {
        let mut center = [x, y];
        for _ in 0..=drawing.nodes().len() {
            let next = drawing.new_node(Shape::Rectangle, center)?;
            if next.center != center { break }
            if !drawing.nodes().iter().any(|n| next.bounds().pad(14.).intersects(n.bounds())) {
                return Some(Placement { shape: next.shape, center, from: Some(Endpoint { node: node.id, port }) });
            }
            center[1] += h + 36;
        }
    }
    None
}

fn snap(drawing: &Drawing, selected: &[u32], delta: &mut Coord, tolerance: f32) -> [Option<f32>; 2] {
    let Some(bounds) = drawing.nodes().iter().filter(|n| selected.contains(&n.id)).map(Node::bounds).reduce(Rect::union) else { return [None; 2] };
    let axes = |rect: Rect, i| [rect.min[i], rect.center()[i], rect.max[i]];
    let mut guides = [None; 2];
    for i in 0..2 {
        let moving = axes(bounds, i).map(|v| v + delta[i]);
        let mut nearest = tolerance;
        let mut adjustment = 0.;
        for n in drawing.nodes().iter().filter(|n| !selected.contains(&n.id)) {
            for (mine, theirs) in moving.into_iter().zip(axes(n.bounds(), i)) {
                if (theirs - mine).abs() < nearest {
                    adjustment = theirs - mine;
                    nearest = adjustment.abs();
                    guides[i] = Some(theirs);
                }
            }
        }
        delta[i] += adjustment;
    }
    guides
}

fn motion_enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc::{class, msg_send, sel, sel_impl};
        unsafe {
            let workspace: *mut objc::runtime::Object = msg_send![class!(NSWorkspace), sharedWorkspace];
            let reduce: objc::runtime::BOOL = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
            reduce == objc::runtime::NO
        }
    }
    #[cfg(not(target_os = "macos"))]
    { false }
}

impl Editor {
    pub(super) fn open_canvas(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.core.kind_of(id), Some(BlockKind::Canvas(_))) { return }
        self.menu = None;
        self.hover = None;
        self.core.break_coalescing();
        let focus = cx.focus_handle();
        let label = cx.new(|cx| TextField::new("", cx));
        let subscription = cx.subscribe_in(&label, window, |e, _, event, window, cx| {
            match event {
                TextFieldEvent::Changed => cx.notify(),
                TextFieldEvent::Submit => e.finish_canvas_label(true, true, window, cx),
                TextFieldEvent::Cancel => e.finish_canvas_label(false, true, window, cx),
                _ => {}
            }
        });
        let blur = cx.on_blur(&label.focus_handle(cx), window, |e, window, cx| e.finish_canvas_label(true, false, window, cx));
        let inactive = cx.observe_window_activation(window, |e, window, cx| {
            if window.is_window_active() { return }
            if let Some(edit) = &mut e.canvas_ui {
                if let Some(Gesture::Marquee { before, .. }) = edit.gesture.take() { edit.nodes = before; }
                edit.space = false;
                edit.hover = None;
                cx.notify();
            }
        });
        self.canvas_ui = Some(CanvasEdit {
            id, tool: Tool::Select, nodes: vec![], edge: None, from: None, gesture: None, space: false, hover: None,
            camera: None, viewport: Default::default(), cache: Default::default(), focus: focus.clone(), label,
            label_edit: None, landing: None, _subscriptions: [subscription, blur, inactive],
        });
        window.focus(&focus);
        cx.notify();
    }

    fn canvas_model(&self) -> Option<(BlockId, &Drawing)> {
        let id = self.canvas_ui.as_ref()?.id;
        let BlockKind::Canvas(drawing) = self.core.kind_of(id)? else { return None };
        Some((id, drawing))
    }

    fn set_canvas(&mut self, id: BlockId, next: Drawing, window: &mut Window, cx: &mut Context<Self>) {
        if !self.read_only && self.core.set_canvas(id, next, None) { self.changed(window, cx); }
        cx.notify();
    }

    fn start_canvas_label(&mut self, target: Object, placement: Option<Placement>, animate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, drawing)) = self.canvas_model() else { return };
        let label = if placement.is_some() { "" } else {
            match target {
                Object::Node(id) => drawing.node(id).map(|n| n.label.as_str()),
                Object::Edge(id) => drawing.edge(id).map(|e| e.label.as_str()),
            }.unwrap_or("")
        }.to_string();
        let Some(edit) = &mut self.canvas_ui else { return };
        edit.freeze();
        edit.select(Some(target));
        edit.tool = Tool::Select;
        edit.from = None;
        edit.label_edit = Some(LabelEdit { target, placement });
        edit.landing = (animate && motion_enabled()).then(Instant::now);
        let node = matches!(target, Object::Node(_));
        edit.label.update(cx, |f, cx| {
            f.multiline = node;
            f.wrap = node;
            f.align = Align::Center;
            f.max_len = Some(if node { LABEL_LIMIT } else { EDGE_LABEL_LIMIT });
            f.max_lines = node.then_some(8);
            f.placeholder_color = Theme::text_caption();
            f.color = Theme::text();
            f.set_placeholder(if node { tr("文字", "Text") } else { tr("分支", "Branch") }, cx);
            f.set_text(&label, cx);
            f.select_all_text(cx);
        });
        window.focus(&edit.label.focus_handle(cx));
        cx.notify();
    }

    fn apply_canvas_label(&mut self, use_text: bool, cx: &mut Context<Self>) -> bool {
        let Some(edit) = &mut self.canvas_ui else { return false };
        let Some(input) = edit.label_edit.take() else { return false };
        edit.landing = None;
        cx.notify();
        if self.read_only { return false }
        let Some(BlockKind::Canvas(drawing)) = self.core.kind_of(edit.id) else { return false };
        let mut next = drawing.clone();
        let target = match input.placement {
            Some(p) => {
                let Some(id) = p.add(&mut next) else { return false };
                Object::Node(id)
            }
            None => input.target,
        };
        if use_text {
            let multiline = matches!(target, Object::Node(_));
            let limit = if multiline { LABEL_LIMIT } else { EDGE_LABEL_LIMIT };
            let field = edit.label.read(cx);
            let label: String = field.text.split('\n').take(if multiline { 8 } else { 1 }).collect::<Vec<_>>().join("\n")
                .chars().filter(|&ch| !ch.is_control() || (multiline && ch == '\n')).take(limit).collect();
            match target {
                Object::Node(id) => next.label_node(id, &label),
                Object::Edge(id) => next.label_edge(id, &label),
            }
        }
        edit.select(Some(target));
        self.core.set_canvas(edit.id, next, None)
    }

    /// Save-before-close also captures an inline edit that still has focus.
    pub(crate) fn commit_canvas_input(&mut self, cx: &mut Context<Self>) -> bool { self.apply_canvas_label(true, cx) }

    fn finish_canvas_label(&mut self, use_text: bool, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.apply_canvas_label(use_text, cx) { self.changed(window, cx); }
        if focus && let Some(edit) = &self.canvas_ui { window.focus(&edit.focus); }
    }

    fn choose_canvas_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only { return }
        self.finish_canvas_label(true, false, window, cx);
        if let Some(edit) = &mut self.canvas_ui {
            edit.tool = tool;
            edit.from = None;
            edit.gesture = None;
            if matches!(tool, Tool::Shape(_)) { edit.select(None); }
            edit.hover = Some(window.mouse_position());
            window.focus(&edit.focus);
            cx.notify();
        }
    }

    fn press_canvas(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_canvas_label(true, false, window, cx);
        let Some(edit) = &mut self.canvas_ui else { return };
        let Some(view) = edit.view() else { return };
        edit.freeze();
        window.focus(&edit.focus);
        let at = view.local(ev.position);
        if ev.button == MouseButton::Middle || edit.space {
            edit.gesture = Some(Gesture::Pan { at: ev.position, camera: view.camera });
            cx.notify();
            return;
        }
        if self.read_only { return }
        let Some(BlockKind::Canvas(drawing)) = self.core.kind_of(edit.id) else { return };
        let tol = 8. / view.camera.scale;
        let hit = drawing.node_at(at);
        let edge = hit.is_none().then(|| edge_at(drawing, &mut edit.cache.borrow_mut(), at, tol)).flatten();
        match edit.tool {
            Tool::Shape(shape) => {
                if hit.is_none() && edge.is_none() {
                    let center = at.map(|n| n.round() as i32);
                    if let Some(n) = drawing.new_node(shape, center) {
                        self.start_canvas_label(Object::Node(n.id), Some(Placement { shape, center, from: None }), true, window, cx);
                    }
                }
            }
            Tool::Connect => {
                let endpoint = hit.map(|node| Endpoint { node, port: Port::Auto });
                if let Some(from) = edit.from.take() && let Some(to) = endpoint && from.node != to.node {
                    let mut next = drawing.clone();
                    if let Some(id) = next.connect_ports(from, to) {
                        edit.select(Some(Object::Edge(id)));
                        let block = edit.id;
                        self.set_canvas(block, next, window, cx);
                        return;
                    }
                } else {
                    edit.from = endpoint;
                    edit.select(None);
                }
                cx.notify();
            }
            Tool::Select => {
                if let Some(e) = edit.edge.and_then(|id| drawing.edge(id)) {
                    let wire = edit.cache.borrow_mut().wire(drawing, e);
                    if let Some(w) = wire {
                        let start = distance(w.arrow.tail(), at) < tol;
                        if start || distance(w.arrow.tip(), at) < tol {
                            let origin = if start { e.to } else { e.from };
                            edit.gesture = Some(Gesture::Link { origin, rebind: Some((e.id, start)), cursor: at, target: None });
                            cx.notify();
                            return;
                        }
                    }
                }
                if let [id] = edit.nodes.as_slice() && let Some(n) = drawing.node(*id) {
                    if let Some(port) = Port::SIDES.into_iter().find(|&p| distance(n.port(p), at) < tol) {
                        edit.gesture = Some(Gesture::Link { origin: Endpoint { node: n.id, port }, rebind: None, cursor: at, target: None });
                        cx.notify();
                        return;
                    }
                    let corners = paint::corners(n);
                    if let Some(i) = corners.iter().position(|&p| distance(p, at) < tol) {
                        let corner = corners[i];
                        let fixed = corners[(i + 2) % 4];
                        let sign = [(corner[0] - fixed[0]).signum(), (corner[1] - fixed[1]).signum()];
                        edit.gesture = Some(Gesture::Resize { at, corner, fixed, sign, id: n.id, center: n.center, size: n.size });
                        cx.notify();
                        return;
                    }
                }
                if let Some(id) = hit {
                    if ev.click_count > 1 {
                        self.start_canvas_label(Object::Node(id), None, false, window, cx);
                        return;
                    }
                    if ev.modifiers.shift {
                        edit.edge = None;
                        if edit.nodes.contains(&id) { edit.nodes.retain(|&n| n != id); }
                        else { edit.nodes.push(id); }
                    } else if !edit.nodes.contains(&id) { edit.select(Some(Object::Node(id))); }
                    if edit.nodes.contains(&id) { edit.gesture = Some(Gesture::Move { at, delta: [0, 0], guides: [None; 2] }); }
                } else if let Some(id) = edge {
                    edit.select(Some(Object::Edge(id)));
                    if ev.click_count > 1 {
                        self.start_canvas_label(Object::Edge(id), None, false, window, cx);
                        return;
                    }
                } else {
                    if !ev.modifiers.shift { edit.select(None); }
                    edit.edge = None;
                    edit.gesture = Some(Gesture::Marquee { at, rect: Rect::between(at, at), before: edit.nodes.clone() });
                }
                cx.notify();
            }
        }
    }

    fn move_canvas(&mut self, position: Point<Pixels>, alt: bool, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.canvas_ui else { return };
        let Some(view) = edit.view() else { return };
        let was_hovered = edit.hover.is_some();
        edit.hover = view.bounds.contains(&position).then_some(position);
        let Some(gesture) = &mut edit.gesture else {
            if matches!(edit.tool, Tool::Shape(_) | Tool::Connect) && (was_hovered || edit.hover.is_some()) { cx.notify(); }
            return;
        };
        let at = view.local(position);
        let Some(BlockKind::Canvas(drawing)) = self.core.kind_of(edit.id) else { return };
        match gesture {
            Gesture::Pan { at, camera } => {
                let moved = position - *at;
                edit.camera = Some(Camera {
                    center: [
                        (camera.center[0] - f32::from(moved.x) / camera.scale).clamp(-200_000., 200_000.),
                        (camera.center[1] - f32::from(moved.y) / camera.scale).clamp(-200_000., 200_000.),
                    ],
                    scale: camera.scale,
                });
            }
            Gesture::Move { at: start, delta, guides } => {
                let mut moved = [at[0] - start[0], at[1] - start[1]];
                *guides = if alt || moved[0].hypot(moved[1]) * view.camera.scale < 2. { [None; 2] }
                    else { snap(drawing, &edit.nodes, &mut moved, 6. / view.camera.scale) };
                *delta = moved.map(|n| n.round() as i32);
            }
            Gesture::Resize { at: start, corner, fixed, sign, id, center, size } => {
                if let Some(n) = drawing.node(*id) {
                    let mut desired = [0; 2];
                    for i in 0..2 {
                        desired[i] = ((corner[i] + at[i] - start[i] - fixed[i]) * sign[i]).round() as i32;
                    }
                    if n.shape == Shape::Circle { desired[0] = desired[0].max(desired[1]); }
                    *size = n.shape.constrain_size(desired);
                    *center = [0, 1].map(|i| (fixed[i] + sign[i] * size[i] as f32 / 2.).round() as i32);
                }
            }
            Gesture::Marquee { at: start, rect, before } => {
                *rect = Rect::between(*start, at);
                edit.nodes = before.clone();
                edit.nodes.extend(drawing.nodes().iter().filter(|n| !before.contains(&n.id) && rect.contains(n.bounds().min) && rect.contains(n.bounds().max)).map(|n| n.id));
            }
            Gesture::Link { origin, cursor, target, .. } => {
                *cursor = at;
                *target = edit.hover.and_then(|_| link_target(drawing, *origin, at, 12. / view.camera.scale));
            }
        }
        cx.notify();
    }

    fn release_canvas(&mut self, ev: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.canvas_ui.as_ref().is_none_or(|e| e.gesture.is_none()) { return }
        self.move_canvas(ev.position, ev.modifiers.alt, cx);
        let Some(edit) = &mut self.canvas_ui else { return };
        let Some(gesture) = edit.gesture.take() else { return };
        if edit.hover.is_none() {
            if let Gesture::Marquee { before, .. } = gesture { edit.nodes = before; }
            cx.notify();
            return;
        }
        let Some(BlockKind::Canvas(drawing)) = self.core.kind_of(edit.id) else { return };
        let mut next = drawing.clone();
        match &gesture {
            Gesture::Link { origin, rebind, target: Some(target), .. } => {
                if let Some((id, start)) = rebind {
                    if next.reconnect(*id, *start, *target) { edit.select(Some(Object::Edge(*id))); }
                } else if let Some(id) = next.connect_ports(*origin, *target) { edit.select(Some(Object::Edge(id))); }
            }
            _ => gesture.update_drawing(&mut next, &edit.nodes),
        }
        let id = edit.id;
        self.set_canvas(id, next, window, cx);
    }

    fn zoom_canvas(&mut self, point: Point<Pixels>, factor: f32, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.canvas_ui else { return };
        let Some(view) = edit.view() else { return };
        if edit.gesture.is_some() { return }
        let old = view.camera.scale;
        let scale = (old * factor).clamp(0.0001, 4.);
        let at = view.local(point);
        let center = [0, 1].map(|i| (at[i] - (at[i] - view.camera.center[i]) * old / scale).clamp(-200_000., 200_000.));
        edit.camera = Some(Camera { center, scale });
        cx.notify();
    }

    fn scroll_canvas(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.canvas_ui else { return };
        let Some(view) = edit.view() else { return };
        if edit.gesture.is_some() { return }
        let [dx, dy] = match ev.delta {
            ScrollDelta::Pixels(p) => [f32::from(p.x), f32::from(p.y)],
            ScrollDelta::Lines(p) => [p.x * 34., p.y * 34.],
        };
        if ev.magnification != 0. {
            self.zoom_canvas(ev.position, (1. + ev.magnification).clamp(0.1, 3.), cx);
        } else if ev.modifiers.platform || ev.modifiers.control {
            self.zoom_canvas(ev.position, (dy * 0.006).clamp(-1., 1.).exp(), cx);
        } else {
            edit.camera = Some(Camera {
                center: [
                    (view.camera.center[0] - dx / view.camera.scale).clamp(-200_000., 200_000.),
                    (view.camera.center[1] - dy / view.camera.scale).clamp(-200_000., 200_000.),
                ],
                scale: view.camera.scale,
            });
            cx.notify();
        }
    }

    fn fit_canvas(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.canvas_ui {
            edit.camera = edit.viewport.get().and_then(|view| {
                let BlockKind::Canvas(drawing) = self.core.kind_of(edit.id)? else { return None };
                Some(Viewport::fit(view.bounds, ink::frame(drawing), 1.35).camera)
            });
            cx.notify();
        }
    }

    fn edit_canvas_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.canvas_ui else { return };
        let target = match edit.nodes.as_slice() {
            [id] => Some(Object::Node(*id)),
            _ => edit.edge.map(Object::Edge),
        };
        if let Some(target) = target { self.start_canvas_label(target, None, false, window, cx); }
    }

    fn quick_canvas_node(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_canvas_label(true, false, window, cx);
        let Some(edit) = &mut self.canvas_ui else { return };
        let [id] = edit.nodes.as_slice() else { return };
        let Some(BlockKind::Canvas(drawing)) = self.core.kind_of(edit.id) else { return };
        let Some(node) = drawing.node(*id) else { return };
        let Some(placement) = downstream(drawing, node) else { return };
        let mut draft = drawing.clone();
        let Some(id) = placement.add(&mut draft) else { return };
        edit.freeze();
        if let Some(view) = edit.view() && let Some(n) = draft.node(id) {
            edit.camera = Some(view.reveal(n.bounds().pad(24.)));
        }
        self.start_canvas_label(Object::Node(id), Some(placement), false, window, cx);
    }

    fn canvas_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only { return }
        self.finish_canvas_label(true, false, window, cx);
        let Some(edit) = &mut self.canvas_ui else { return };
        let Some(BlockKind::Canvas(drawing)) = self.core.kind_of(edit.id) else { return };
        edit.freeze();
        let mut next = drawing.clone();
        match command {
            Command::Delete => {
                for id in &edit.nodes { next.remove(Object::Node(*id)); }
                if let Some(id) = edit.edge { next.remove(Object::Edge(id)); }
                edit.select(None);
            }
            Command::Arrange(axis, distribute) => next.arrange(&edit.nodes, axis, distribute),
            Command::Nudge(delta) => next.translate(&edit.nodes, delta),
            Command::Route => if let Some(id) = edit.edge { next.toggle_route(id); },
        }
        let id = edit.id;
        window.focus(&edit.focus);
        self.set_canvas(id, next, window, cx);
    }

    fn canvas_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only { return }
        self.finish_canvas_label(true, false, window, cx);
        let Some(edit) = &mut self.canvas_ui else { return };
        edit.freeze();
        edit.gesture = None;
        edit.from = None;
        let changed = if redo { self.core.redo() } else { self.core.undo() };
        if let Some(BlockKind::Canvas(drawing)) = self.core.kind_of(edit.id) {
            edit.nodes.retain(|&n| drawing.node(n).is_some());
            edit.edge = edit.edge.filter(|&id| drawing.edge(id).is_some());
            window.focus(&edit.focus);
        } else {
            self.close_canvas(window, cx);
        }
        if changed { self.changed(window, cx); }
        cx.notify();
    }

    fn close_canvas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_canvas_label(true, false, window, cx);
        self.canvas_ui = None;
        self.core.break_coalescing();
        window.focus(&self.focus);
        cx.notify();
    }

    fn cancel_canvas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.canvas_ui else { return };
        if edit.label_edit.is_some() { return self.finish_canvas_label(false, true, window, cx) }
        if let Some(gesture) = edit.gesture.take() {
            match gesture {
                Gesture::Marquee { before, .. } => edit.nodes = before,
                Gesture::Pan { camera, .. } => edit.camera = Some(camera),
                _ => {}
            }
        } else if edit.tool != Tool::Select {
            edit.tool = Tool::Select;
            edit.from = None;
        } else if !edit.nodes.is_empty() || edit.edge.is_some() {
            edit.select(None);
        } else { return self.close_canvas(window, cx) }
        cx.notify();
    }

    fn canvas_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.canvas_ui else { return };
        let key = ev.keystroke.key.as_str();
        let mods = ev.keystroke.modifiers;
        if edit.label_edit.is_some() {
            if key == "enter" && (mods.platform || mods.control) && !edit.label.read(cx).is_composing() {
                cx.stop_propagation();
                self.finish_canvas_label(true, true, window, cx);
            }
            return;
        }
        if mods.platform || mods.control || mods.alt || !edit.focus.is_focused(window) { return }
        match key {
            "space" => { edit.space = true; cx.notify(); }
            "f" => self.fit_canvas(cx),
            _ if self.read_only => return,
            "v" => self.choose_canvas_tool(Tool::Select, window, cx),
            "r" => self.choose_canvas_tool(Tool::Shape(Shape::Rectangle), window, cx),
            "d" => self.choose_canvas_tool(Tool::Shape(Shape::Diamond), window, cx),
            "o" => self.choose_canvas_tool(Tool::Shape(Shape::Circle), window, cx),
            "t" => self.choose_canvas_tool(Tool::Shape(Shape::Text), window, cx),
            "a" => self.choose_canvas_tool(Tool::Connect, window, cx),
            "enter" => self.edit_canvas_selection(window, cx),
            "left" | "right" | "up" | "down" => {
                let n = if mods.shift { 10 } else { 1 };
                let delta = match key { "left" => [-n, 0], "right" => [n, 0], "up" => [0, -n], _ => [0, n] };
                self.canvas_command(Command::Nudge(delta), window, cx);
            }
            "tab" => {
                if mods.shift { window.focus_prev(); } else { window.focus_next(); }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn canvas_inline(&self, edit: &CanvasEdit, drawing: &Drawing, target: Object, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let view = edit.view()?;
        let scale = view.camera.scale;
        let (center, max_width, max_height, font) = match target {
            Object::Node(id) => {
                let n = drawing.node(id)?;
                (n.position(), ink::label_width(n), n.size[1] as f32 * 0.75, ink::NODE_FONT)
            }
            Object::Edge(id) => {
                let e = drawing.edge(id)?;
                let wire = edit.cache.borrow_mut().wire(drawing, e)?;
                (wire.arrow.label_position(), 320., 21., ink::EDGE_FONT)
            }
        };
        let max_width = px(max_width * scale).min(view.bounds.size.width - px(8.)).max(px(1.));
        let mut font = (font * scale).max(1.);
        let field = edit.label.read(cx);
        let displayed = if field.text.is_empty() {
            if matches!(target, Object::Node(_)) { tr("文字", "Text") } else { tr("分支", "Branch") }
        } else { &field.text };
        let mut widest = displayed.split('\n').map(|s| paint::label_size(window, s, font)).fold(px(0.), Pixels::max);
        let max_height = px(max_height * scale).min(view.bounds.size.height - px(4.));
        let mut width = (widest + px(font * 0.65 + 6.)).max(px(font * 2.)).min(max_width);
        let mut rows = if matches!(target, Object::Node(_)) { paint::label_rows(window, displayed, font, (width - px(4.)).max(px(1.))) } else { 1 };
        if px(font * ink::LABEL_LEADING * rows as f32) > max_height && rows > 1 {
            font = font.min((f32::from(max_height) / (ink::LABEL_LEADING * rows as f32)).max(1.));
            widest = displayed.split('\n').map(|s| paint::label_size(window, s, font)).fold(px(0.), Pixels::max);
            width = (widest + px(font * 0.65 + 6.)).max(px(font * 2.)).min(max_width);
            rows = paint::label_rows(window, displayed, font, (width - px(4.)).max(px(1.)));
        }
        let line_height = px(font * ink::LABEL_LEADING);
        let height = (line_height * rows as f32).min(max_height).max(line_height);
        let position = view.screen(center) - view.bounds.origin;
        let x = (position.x - width / 2.).clamp(px(0.), (view.bounds.size.width - width).max(px(0.)));
        let y = (position.y - height / 2.).clamp(px(0.), (view.bounds.size.height - height).max(px(0.)));
        let label = edit.label.clone();
        label.update(cx, |f, _| {
            f.size = font;
            f.line_height = f32::from(line_height);
        });
        Some(div().absolute().left(x).top(y).w(width).h(height).flex().px(px(2.)).overflow_hidden().occlude()
            .when(matches!(target, Object::Edge(_)), |d| d.bg(Theme::bg_body()))
            .child(label).into_any_element())
    }
}

#[derive(Clone, Copy)]
enum Command { Delete, Arrange(Axis, bool), Nudge([i32; 2]), Route }

impl Editor {
    pub(super) fn canvas_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let edit = self.canvas_ui.as_ref()?;
        let Some(BlockKind::Canvas(model)) = self.core.kind_of(edit.id) else {
            self.close_canvas(window, cx);
            return None;
        };
        let editable = !self.read_only;
        let mut drawing = model.clone();
        let mut marks = Marks { nodes: edit.nodes.clone(), edge: edit.edge, anchor: edit.from, ..Default::default() };
        if let Some(gesture) = &edit.gesture {
            gesture.update_drawing(&mut drawing, &edit.nodes);
            match gesture {
                Gesture::Move { guides, .. } => marks.guides = *guides,
                Gesture::Marquee { rect, .. } => marks.marquee = Some(*rect),
                _ => {}
            }
        }
        if let Some(input) = edit.label_edit {
            let target = if let Some(placement) = input.placement {
                placement.add(&mut drawing).map(Object::Node)
            } else { Some(input.target) };
            marks.editing = target;
            if let Some(Object::Node(id)) = target && let Some(at) = edit.landing {
                marks.landing = Some((id, at));
            }
        }

        if let Some(view) = edit.view() {
            let at = edit.hover.filter(|p| view.bounds.contains(p)).map(|p| view.local(p));
            let preview_link = match &edit.gesture {
                Some(Gesture::Link { origin, rebind, cursor, target }) => Some((*origin, *rebind, *cursor, *target)),
                _ => edit.from.zip(at).map(|(from, at)| (from, None, at, link_target(&drawing, from, at, 12. / view.camera.scale))),
            };
            if let Some((source, rebind, cursor, target)) = preview_link && let Some(node) = drawing.node(source.node) {
                let point = rebind.and_then(|(id, start)| {
                    let edge = drawing.edge(id)?;
                    let wire = edit.cache.borrow_mut().wire(&drawing, edge)?;
                    Some(if start { wire.arrow.tip() } else { wire.arrow.tail() })
                }).unwrap_or_else(|| node.port(source.port));
                let end = target.and_then(|e| drawing.node(e.node).map(|n| n.port(e.port)));
                marks.connection = Some((point, end.unwrap_or(cursor), end.is_some()));
                marks.anchor = target.or(Some(source));
            }
            if let Tool::Shape(shape) = edit.tool && let Some(at) = at && edit.gesture.is_none() && !edit.space {
                let blocked = drawing.node_at(at).is_some() || edge_at(&drawing, &mut edit.cache.borrow_mut(), at, 8. / view.camera.scale).is_some();
                if let Some(node) = drawing.new_node(shape, at.map(|n| n.round() as i32)) {
                    marks.ghost = Some((node, if blocked { 0.2 } else { 0.5 }));
                }
            }
        }
        marks.handles = editable && edit.label_edit.is_none() && edit.gesture.is_none() && edit.tool == Tool::Select;
        let inline = marks.editing.and_then(|target| self.canvas_inline(edit, &drawing, target, window, cx));
        let cursor = if matches!(edit.gesture, Some(Gesture::Pan { .. })) { CursorStyle::ClosedHand }
            else if edit.space { CursorStyle::OpenHand }
            else if matches!(edit.gesture, Some(Gesture::Resize { .. })) { CursorStyle::ResizeUpLeftDownRight }
            else if edit.tool != Tool::Select || matches!(edit.gesture, Some(Gesture::Link { .. })) { CursorStyle::Crosshair }
            else { CursorStyle::Arrow };
        let scene = paint::scene(drawing, Scene {
            camera: edit.camera, viewport: Some(edit.viewport.clone()), cache: edit.cache.clone(), grid: editable, marks,
        });

        let mut surface = div().id("canvas-surface").size_full().cursor(cursor).child(scene)
            .on_mouse_move(cx.listener(|e, ev: &MouseMoveEvent, _, cx| e.move_canvas(ev.position, ev.modifiers.alt, cx)))
            .on_hover(cx.listener(|e, hovered: &bool, _, cx| {
                if !*hovered && let Some(edit) = &mut e.canvas_ui && edit.hover.take().is_some() { cx.notify(); }
            }))
            .on_scroll_wheel(cx.listener(|e, ev, _, cx| { cx.stop_propagation(); e.scroll_canvas(ev, cx); }));
        for button in [MouseButton::Left, MouseButton::Middle] {
            surface = surface
                .on_mouse_down(button, cx.listener(|e, ev, window, cx| { cx.stop_propagation(); e.press_canvas(ev, window, cx); }))
                .on_mouse_up(button, cx.listener(|e, ev, window, cx| e.release_canvas(ev, window, cx)))
                .on_mouse_up_out(button, cx.listener(|e, ev, window, cx| e.release_canvas(ev, window, cx)));
        }

        let mut tools = div().flex().flex_wrap().gap(px(4.)).items_center().min_h(px(34.)).flex_none();
        if editable {
            for (id, tool, key) in [
                ("select", Tool::Select, "V"),
                ("rectangle", Tool::Shape(Shape::Rectangle), "R"),
                ("diamond", Tool::Shape(Shape::Diamond), "D"),
                ("circle", Tool::Shape(Shape::Circle), "O"),
                ("text", Tool::Shape(Shape::Text), "T"),
                ("arrow", Tool::Connect, "A"),
            ] {
                let selected = edit.tool == tool;
                let color = if selected { Theme::primary() } else { Theme::text() };
                let glyph = match tool {
                    Tool::Select => paint::symbol(true, color).into_any_element(),
                    Tool::Connect => paint::symbol(false, color).into_any_element(),
                    Tool::Shape(Shape::Text) => icon("type", 19., color).into_any_element(),
                    Tool::Shape(shape) => paint::legend(shape).into_any_element(),
                };
                tools = tools.child(icon_button(id, selected, true).child(glyph).tip(tool.label(), Some(key))
                    .on_click(cx.listener(move |e, _, window, cx| e.choose_canvas_tool(tool, window, cx))));
            }
            tools = tools.child(div().w(px(1.)).h(px(23.)).mx(px(4.)).bg(Theme::line_border()));
            for (id, name, redo, keys) in [
                ("undo", tr("撤销", "Undo"), false, if cfg!(target_os = "macos") { "⌘ + Z" } else { "Ctrl + Z" }),
                ("redo", tr("重做", "Redo"), true, if cfg!(target_os = "macos") { "⌘ + ⇧ + Z" } else { "Ctrl + Shift + Z" }),
            ] {
                let enabled = if redo { edit.label_edit.is_none() && self.core.can_redo() }
                    else { edit.label_edit.is_some() || self.core.can_undo() };
                let color = if enabled { Theme::text_caption() } else { Theme::text_disabled() };
                tools = tools.child(icon_button(id, false, enabled).child(icon(id, 18., color)).tip(name, Some(keys))
                    .when(enabled, |d| d.on_click(cx.listener(move |e, _, window, cx| e.canvas_history(redo, window, cx)))));
            }
        }

        let mut context = div().id("canvas-context").flex().items_center().gap(px(5.)).h(px(35.)).flex_none().overflow_x_scroll();
        if editable {
            let count = edit.nodes.len();
            if count > 0 || edit.edge.is_some() {
                context = context.child(div().flex_none().text_size(px(12.)).text_color(Theme::text_caption()).child(if count == 0 {
                    tr("已选连线", "Arrow selected").to_string()
                } else if crate::settings::english() { format!("{count} selected") } else { format!("已选 {count} 件") }));
                if count == 1 {
                    context = context.child(tool_button("next", tr("＋ 下游", "＋ Next")).flex_none().bg(Theme::fill_turninto_active())
                        .on_click(cx.listener(|e, _, window, cx| e.quick_canvas_node(window, cx))));
                }
                if count == 1 || edit.edge.is_some() {
                    context = context.child(tool_button("edit-label", tr("编辑文字", "Edit label")).flex_none()
                        .on_click(cx.listener(|e, _, window, cx| e.edit_canvas_selection(window, cx))));
                }
                if let Some(edge) = edit.edge.and_then(|id| model.edge(id)) {
                    let label = if edge.route == Route::Straight { tr("改为折线", "Elbow") } else { tr("改为直线", "Straight") };
                    context = context.child(tool_button("route", label).flex_none()
                        .on_click(cx.listener(|e, _, window, cx| e.canvas_command(Command::Route, window, cx))));
                }
                for (id, name, axis, distribute) in [
                    ("align-left", tr("左对齐", "Align left"), Axis::X, false),
                    ("align-top", tr("顶对齐", "Align top"), Axis::Y, false),
                    ("space-x", tr("水平等距", "Space horizontally"), Axis::X, true),
                    ("space-y", tr("垂直等距", "Space vertically"), Axis::Y, true),
                ] {
                    if count >= if distribute { 3 } else { 2 } {
                        context = context.child(tool_button(id, name).flex_none()
                            .on_click(cx.listener(move |e, _, window, cx| e.canvas_command(Command::Arrange(axis, distribute), window, cx))));
                    }
                }
                context = context.child(tool_button("delete", text("删除")).flex_none()
                    .on_click(cx.listener(|e, _, window, cx| e.canvas_command(Command::Delete, window, cx))));
            } else {
                context = context.child(div().text_size(px(12.)).text_color(Theme::text_caption()).child(tr("选择对象后显示操作", "Select an object to show its actions")));
            }
        }

        let hint = if edit.label_edit.is_some() {
            if cfg!(target_os = "macos") { tr("⌘ + Enter 完成 · Esc 取消", "⌘ + Enter to finish · Esc to cancel") }
            else { tr("Ctrl + Enter 完成 · Esc 取消", "Ctrl + Enter to finish · Esc to cancel") }
        } else if matches!(edit.gesture, Some(Gesture::Link { .. })) {
            tr("松手绑定到目标节点", "Release to bind to the target")
        } else {
            match edit.tool {
                Tool::Shape(_) => tr("移入画布预览 · 点击放置", "Move onto the canvas to preview · Click to place"),
                Tool::Connect => tr("从端口拖线，或点起点和终点", "Drag a port or click both endpoints"),
                Tool::Select => tr("空格拖动平移 · Shift 多选", "Space to pan · Shift to multi-select"),
            }
        };
        let scale = edit.view().map_or(1., |v| v.camera.scale);
        let mut zoom = div().flex().items_center().gap(px(4.)).flex_none();
        for (id, label, factor) in [("minus", tr("缩小", "Zoom out"), 1. / 1.2), ("plus", tr("放大", "Zoom in"), 1.2)] {
            zoom = zoom.child(icon_button(id, false, true).w(px(28.)).child(icon(id, 14., Theme::text())).tip(label, None)
                .on_click(cx.listener(move |e, _, _, cx| {
                    if let Some(view) = e.canvas_ui.as_ref().and_then(CanvasEdit::view) { e.zoom_canvas(view.bounds.center(), factor, cx); }
                })));
            if id == "minus" {
                zoom = zoom.child(div().min_w(px(40.)).text_center().text_size(px(12.)).child(format!("{}%", (scale * 100.).round() as i32)));
            }
        }
        zoom = zoom.child(tool_button("fit", tr("适应内容", "Fit content")).tip(tr("适应内容", "Fit content"), Some("F"))
            .on_click(cx.listener(|e, _, _, cx| e.fit_canvas(cx))));

        Some(div().id("drawing-canvas").absolute().inset_0().p(px(16.)).bg(Theme::bg_body()).occlude()
            .track_focus(&edit.focus).tab_group().key_context("DrawingCanvas").flex().flex_col().gap(px(6.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|e, ev, window, cx| e.canvas_key(ev, window, cx)))
            .on_key_up(cx.listener(|e, ev: &KeyUpEvent, _, cx| {
                if ev.keystroke.key == "space" && let Some(edit) = &mut e.canvas_ui { edit.space = false; cx.notify(); }
            }))
            .on_action(cx.listener(|e, _: &CancelCanvas, window, cx| e.cancel_canvas(window, cx)))
            .on_action(cx.listener(|e, _: &DeleteCanvasObject, window, cx| e.canvas_command(Command::Delete, window, cx)))
            .on_action(cx.listener(|e, _: &UndoCanvas, window, cx| e.canvas_history(false, window, cx)))
            .on_action(cx.listener(|e, _: &RedoCanvas, window, cx| e.canvas_history(true, window, cx)))
            .child(div().flex().items_center().justify_between().child(text("画布"))
                .child(tool_button("close-canvas", tr("完成", "Done")).on_click(cx.listener(|e, _, window, cx| e.close_canvas(window, cx)))))
            .when(editable, |d| d.child(tools).child(context))
            .child(div().relative().flex_1().min_h(px(0.)).min_w(px(0.)).border_1().border_color(Theme::line_border()).overflow_hidden()
                .child(surface).when_some(inline, |d, inline| d.child(inline)))
            .child(div().flex().items_center().justify_between().gap(px(8.)).min_h(px(34.))
                .child(div().text_size(px(12.)).text_color(Theme::text_caption()).truncate().child(hint))
                .child(zoom))
            .into_any_element())
    }
}

impl R<'_> {
    pub(super) fn canvas_block(&self, id: BlockId, drawing: &Drawing, width: f32, cx: &mut Context<Editor>) -> AnyElement {
        let [w, h] = ink::frame(drawing).size();
        let height = (width * h / w).clamp(112., 340.);
        let preview = div().w_full().h(px(height)).bg(Theme::bg_body()).overflow_hidden()
            .child(paint::scene(drawing.clone(), Scene { camera: None, viewport: None, cache: Default::default(), grid: false, marks: Marks::default() }));
        div().id(("canvas-block", id.0)).w_full().border_1().border_color(Theme::line_border()).rounded(px(6.)).overflow_hidden()
            .when(!self.ghost, |d| d.child(
                div().flex().justify_between().items_center().px(px(10.)).py(px(4.)).text_size(px(12.)).text_color(Theme::text_caption()).child(text("画布"))
                    .child(tool_button("open-canvas", if self.ed.read_only { tr("查看画布", "View canvas") } else { tr("编辑画布", "Edit canvas") })
                        .on_click(cx.listener(move |e, _, window, cx| e.open_canvas(id, window, cx)))),
            ))
            .child(preview).into_any_element()
    }
}
