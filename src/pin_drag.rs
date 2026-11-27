//! 置顶's rows dragged into an order of their own (`fdl_core::pins`), as the tree's rows are
//! (`sidebar_drag`): a press opens the page unless it moves, the row lifts and follows the
//! pointer up and down, the others make room, and the drop writes the moved pin's key alone
//! (the first among pins from before 置顶 had an order places them all). One ⌘Z puts it back.

use crate::app::Workspace;
use crate::sidebar_drag::{Fade, Spring, faded, row_face};
use crate::theme::{self, Theme};
use gpui::{prelude::FluentBuilder, *};
use rustc_hash::FxHashMap;
use std::{cell::RefCell, rc::Rc};
use web_time::Instant;

const PITCH: f32 = crate::sidebar::ROW_H + crate::sidebar::ROW_GAP;

pub(crate) struct PinDrag {
    id: String,
    /// 置顶's order when it was picked up.
    order: Vec<String>,
    start: Point<Pixels>,
    at: Point<Pixels>,
    /// The pointer from the row's top left.
    offset: Point<Pixels>,
    left: Pixels,
    width: Pixels,
    started: bool,
    from: usize,
    to: usize,
    lift: Fade,
}

#[derive(Default)]
pub(crate) struct PinRows {
    pub active: Option<PinDrag>,
    /// Each row's place as laid out last frame (before it is moved aside), by id.
    laid: Rc<RefCell<FxHashMap<String, Bounds<Pixels>>>>,
    /// How far each row stands from its place: making room, or settling after a drop.
    springs: FxHashMap<String, Spring<1>>,
    last_frame: Option<Instant>,
}

/// Where the row picked up at `from` goes when its middle is at `y`, among rows whose
/// middles are `mids` (in order, its own included): before the first of the others below it.
fn slot(mids: &[f32], from: usize, y: f32) -> usize {
    mids.iter().enumerate().filter(|(i, m)| *i != from && **m < y).count()
}

/// How far row `i` moves aside while the one picked up at `from` is held over slot `to`.
fn aside(i: usize, from: usize, to: usize) -> f32 {
    if from < to && i > from && i <= to { -PITCH } else if to < from && i >= to && i < from { PITCH } else { 0. }
}

impl Workspace {
    /// A press on a 置顶 row: a click opens it (`finish_pin_click`), a drag moves it.
    pub(crate) fn arm_pin_drag(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) {
        let order: Vec<String> = fdl_core::pins::sorted(self.entries.iter()).into_iter().map(|e| e.id.clone()).collect();
        let Some(from) = order.iter().position(|x| x == id) else { return };
        let Some(b) = self.folder_drag.pins.laid.borrow().get(id).copied() else { return };
        self.folder_drag.pins.active = Some(PinDrag { id: id.to_string(), order, start: at, at, offset: at - b.origin, left: b.left(), width: b.size.width,
            started: false, from, to: from, lift: Fade::new(0., 520., 24.) });
        cx.notify();
    }

    pub(crate) fn move_pin_drag(&mut self, at: Point<Pixels>, pressed: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !pressed { return self.end_pin_drag(true, cx); }
        let laid = self.folder_drag.pins.laid.clone();
        let Some(d) = self.folder_drag.pins.active.as_mut() else { return };
        d.at = at;
        if !d.started {
            if d.order.len() < 2 || f32::from(at.x - d.start.x).hypot(f32::from(at.y - d.start.y)) < 4. { return; }
            d.started = true; d.lift.to = 1.;
            self.row_menu = None; self.create_menu = None;
            window.focus(&self.focus, cx);
        }
        let laid = laid.borrow();
        let mids: Vec<f32> = d.order.iter().map(|id| laid.get(id).map_or(f32::MAX, |b| f32::from(b.center().y))).collect();
        d.to = slot(&mids, d.from, f32::from(at.y - d.offset.y) + crate::sidebar::ROW_H / 2.);
        cx.notify();
    }

    /// The press let go without moving: the click a 置顶 row always was.
    pub(crate) fn finish_pin_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.folder_drag.pins.active.as_ref().filter(|d| !d.started).map(|d| d.id.clone()) else { return };
        self.folder_drag.pins.active = None;
        self.open(&id, window, cx);
    }

    /// Let go (or Esc): the pin takes its new place, written and remembered for ⌘Z, and the row
    /// settles there from where it was let go.
    pub(crate) fn end_pin_drag(&mut self, cancel: bool, cx: &mut Context<Self>) {
        let Some(d) = self.folder_drag.pins.active.take() else { return };
        if !d.started { cx.notify(); return; }
        let to = if cancel { d.from } else { d.to };
        let mut order = d.order.clone();
        let id = order.remove(d.from);
        order.insert(to, id.clone());
        let laid = self.folder_drag.pins.laid.borrow().clone();
        let springs = &mut self.folder_drag.pins.springs;
        // The others stand where they made room, which is their new place.
        springs.clear();
        let top = laid.get(&d.order[0]).map_or(0., |b| f32::from(b.top()));
        let mut s = Spring::new([f32::from(d.at.y - d.offset.y) - (top + to as f32 * PITCH)]);
        s.target = [0.];
        springs.insert(id.clone(), s);
        if to != d.from {
            let keys: Vec<(String, Option<String>)> = order.iter().map(|x| (x.clone(), self.entries.iter().find(|e| &e.id == x).and_then(|e| e.meta.pin_order.clone()))).collect();
            let shown: Vec<(&str, Option<&str>)> = keys.iter().map(|(x, k)| (x.as_str(), k.as_deref())).collect();
            let mut pins = vec![];
            for (x, k) in fdl_core::pins::keys_for(&shown) {
                let was = keys.iter().find(|y| y.0 == x).and_then(|y| y.1.clone());
                if let Some(e) = self.entries.edit().iter_mut().find(|e| e.id == x) { e.meta.pin_order = Some(k.clone()); }
                if let Err(err) = self.store.set_pin(&x, Some(Some(k.clone()))) { self.show_toast_kind(crate::ui::ToastKind::Error, &err.to_string(), cx); }
                pins.push((x, was, Some(k)));
            }
            self.remember_places(crate::file_history::Places { pins, tops: None }, crate::settings::tr("拖拽移动", "Drag move"));
        }
        cx.notify();
    }

    /// Steps the rows' springs for this frame; each row's offset by id.
    pub(crate) fn pin_offsets(&mut self, ids: &[String], window: &mut Window) -> Vec<f32> {
        let now = Instant::now();
        let p = &mut self.folder_drag.pins;
        let dt = p.last_frame.replace(now).map_or(1. / 120., |last| now.duration_since(last).as_secs_f32().min(0.032));
        let held = p.active.as_ref().filter(|d| d.started).map(|d| (d.from, d.to));
        let mut moving = false;
        if let Some(d) = p.active.as_mut() { moving |= d.lift.step(dt); }
        let out = ids.iter().enumerate().map(|(i, id)| {
            let target = held.map_or(0., |(from, to)| aside(i, from, to));
            let s = p.springs.entry(id.clone()).or_insert_with(|| Spring::new([0.]));
            s.target = [target];
            if !crate::settings::motion_high() { s.p = [target]; s.v = [0.]; }
            moving |= s.step(dt);
            s.p[0]
        }).collect();
        p.springs.retain(|id, _| ids.contains(id));
        if moving { window.request_animation_frame(); } else { p.last_frame = None; }
        out
    }

    /// A 置顶 row in place: its laid-out place recorded, moved aside by `off`, hidden while it is the one held.
    pub(crate) fn pin_row(&self, id: &str, off: f32, row: impl IntoElement) -> Div {
        let laid = self.folder_drag.pins.laid.clone();
        let key = id.to_string();
        let held = self.folder_drag.pins.active.as_ref().is_some_and(|d| d.started && d.id == id);
        div().relative()
            .child(canvas(move |b, _, _| { laid.borrow_mut().insert(key.clone(), b); }, |_, _, _, _| {}).absolute().inset_0())
            .child(div().relative().top(px(off)).when(held, |d| d.opacity(0.)).child(row))
    }

    /// The row held: lifted, following the pointer up and down.
    pub(crate) fn render_pin_drag_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let d = self.folder_drag.pins.active.as_ref().filter(|d| d.started)?;
        let e = self.entries.iter().find(|e| e.id == d.id)?;
        let l = d.lift.p.max(0.);
        let (x, y, width) = (f32::from(d.left), f32::from(d.at.y - d.offset.y), f32::from(d.width));
        let layer = div().id("pin-drag-overlay").absolute().inset_0().occlude()
            .on_mouse_move(cx.listener(|w, ev: &MouseMoveEvent, window, cx| w.move_pin_drag(ev.position, ev.pressed_button == Some(MouseButton::Left), window, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|w, _, _, cx| w.end_pin_drag(false, cx)))
            .child(div().absolute().left(px(x - 3. * l)).top(px(y - l)).w(px(width + 6. * l)).h(px(crate::sidebar::ROW_H)).pr(px(6.)).rounded(px(crate::sidebar::ROW_R))
                .bg(theme::raised(Theme::bg_body())).shadow(faded(crate::design::shadow::toast(), l.min(1.))).flex().items_center().text_color(Theme::text())
                .child(row_face(e).flex_1().min_w_0().pl(px(6. + 3. * l))));
        Some(deferred(layer).with_priority(5).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::{PITCH, aside, slot};

    #[test]
    fn a_held_pin_lands_where_its_middle_is() {
        let mids = [15., 45., 75., 105.];
        // picked up second, held over the fourth's middle and past it
        assert_eq!(slot(&mids, 1, 50.), 1);
        assert_eq!(slot(&mids, 1, 80.), 2);
        assert_eq!(slot(&mids, 1, 200.), 3);
        assert_eq!(slot(&mids, 3, 0.), 0);
        // the rows between make room, toward where it came from
        assert_eq!([0, 1, 2, 3].map(|i| aside(i, 1, 3)), [0., 0., -PITCH, -PITCH]);
        assert_eq!([0, 1, 2, 3].map(|i| aside(i, 3, 0)), [PITCH, PITCH, PITCH, 0.]);
    }
}
