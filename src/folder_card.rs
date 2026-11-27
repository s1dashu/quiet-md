//! A folder page's folders: a glass folder with its documents tucked inside as paper,
//! and dragging a document card into one.
//!
//! The folder is painted, not laid out: the papers tilt and fan, which GPUI
//! elements cannot, so back plate, papers and the frosted front are paths in
//! two canvases (papers below, glass above), with the labels as ordinary
//! elements between and over them. Every motion is a spring
//! (`sidebar_drag::{Fade, Spring}`): hover lifts the papers out one after
//! another while the front leans back; a dragged card turns into a paper,
//! the folder opens its mouth under it, and a drop slides it in behind the
//! glass while the folder dips.
use fdl_core::vfs::PathExt as _;
use crate::app::{RowMenu, Workspace};
use crate::settings::display_title;
use crate::sidebar_drag::{Fade, Spring};
use crate::store::Entry;
use crate::theme::{self, Theme, craft, ca};
use crate::ui::{TipExt, icon, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use web_time::Instant;

/// The tab's share of a folder card's height: the card is a document card's
/// size, tab included (`folder_page.rs`), the body below the tab.
pub(crate) const TAB: f32 = 0.06;
/// Papers peeking out of a folder.
const PAPERS: usize = 3;
/// The carried paper: the size a paper has inside a folder, from the moment it
/// is picked up until it is let go (same proportions as a card).
const MINI_W: f32 = 110.;
const MINI_H: f32 = MINI_W * 264. / 236.;

#[derive(Default)]
pub(crate) struct FolderCards {
    fx: HashMap<String, FolderFx>,
    hovered: Option<String>,
    pub(crate) drag: Option<CardDrag>,
    /// Files dragged in from outside.
    pub(crate) file_drop: Option<FileDrop>,
    /// Last-frame window bounds of the folder and document cards, by entry id.
    bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// Each card's / list row's place in the layout, before `motion` moves it
    /// (`card_motion`): a reorder is aimed at these, so cards making room never move the targets.
    slots: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    /// How far each card / row stands from its layout place, gliding to make room (手动排序).
    motion: RefCell<HashMap<String, (Spring<2>, Instant)>>,
    /// The folder cards' papers and counts, kept while the entries hold (`Entries::generation`).
    papers: RefCell<Papers>,
}

/// Each folder's children (indices into the entries, in their order), and
/// each card's papers and count by (folder, front paper). Looking for a
/// card's papers walked every entry once per folder under it, each frame.
#[derive(Default)]
struct Papers { key: Option<u64>, children: HashMap<String, Vec<usize>>, cards: HashMap<(String, Option<String>), (Vec<Entry>, usize)> }

struct FolderFx {
    /// Hover: the papers rise, the front leans back.
    open: Fade,
    /// A card held over it: the papers part and the front drops further.
    mouth: Fade,
    /// Each paper's rise; the back ones are softer, so they follow the front.
    papers: [Fade; PAPERS],
    /// Kicked on press and on a drop: the front dips and springs back.
    dip: Fade,
    /// A dropped document sliding in (card-local x, y, w, h).
    arrival: Option<(String, Spring<4>)>,
    /// The paper kept in front: the last one dropped in.
    front: Option<String>,
    last: Option<Instant>,
}

impl FolderFx {
    fn new() -> Self {
        Self {
            open: Fade::new(0., 260., 21.),
            mouth: Fade::new(0., 300., 23.),
            papers: [Fade::new(0., 320., 21.), Fade::new(0., 230., 18.), Fade::new(0., 170., 15.5)],
            dip: Fade::new(0., 520., 15.),
            arrival: None,
            front: None,
            last: None,
        }
    }
}

pub(crate) struct CardDrag {
    id: String,
    start: Point<Pixels>,
    at: Point<Pixels>,
    started: bool,
    /// Where on the card it was picked up (0–1), kept on the carried paper.
    grab: (f32, f32),
    /// The card's bounds when picked up; a cancelled drag flies back there.
    origin: Bounds<Pixels>,
    /// The carried paper in window coordinates (x, y, w, h).
    rect: Spring<4>,
    /// The folder under the pointer: a drop puts the document inside.
    over: Option<String>,
    returning: bool,
    last: Option<Instant>,
    /// 手动排序: the page's cards / rows as laid out when it was picked up, in their order.
    /// Empty otherwise: a drop only goes into a folder.
    slots: Vec<(String, Option<Bounds<Pixels>>)>,
    /// Where it goes among the others (an index into `slots` without it): the gap the others open.
    target: usize,
    /// Carried as a row's face rather than a paper: a folder card, or any row of the list.
    chip: bool,
    list: bool,
    /// Picked up by a long press (touch): the finger carries it and the grid
    /// scrolls when it nears the top or bottom.
    touch: bool,
}

/// Touch: how near the grid's top or bottom edge a carried card scrolls it, and how fast.
const EDGE: f32 = 48.;
const EDGE_SPEED: f32 = 600.;

/// Which way a carried card at `y` scrolls a list spanning `top`..`bottom`: -1 up, 1 down.
fn edge_scroll(y: f32, top: f32, bottom: f32) -> f32 {
    if y < top + EDGE { -1. } else if y > bottom - EDGE { 1. } else { 0. }
}

/// The carried row face (a folder card's, a list row's).
const CHIP_H: f32 = 40.;
const CHIP_W: f32 = 260.;

/// What lies on a paper: drawn lines, a real title over drawn lines, or real content.
#[derive(Clone, Copy, PartialEq)]
enum Face { Sketch, Titled, Content }

/// One paper's pose in card coordinates: centre, size and tilt (radians).
#[derive(Clone, Copy)]
struct Paper { cx: f32, cy: f32, w: f32, h: f32, rot: f32, seed: u64 }

/// Everything both canvases need for one frame.
#[derive(Clone)]
struct Pose {
    w: f32,
    h: f32,
    lift: f32,
    open: f32,
    /// Front's top edge: left (higher) and right, and how far its top corners reach in
    /// (negative: out, the flap tipping toward the viewer as it opens).
    yl: f32,
    yr: f32,
    inset: f32,
    papers: Vec<Paper>,
}

fn seed(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    id.hash(&mut h);
    h.finish()
}

/// A rounded rectangle turned by `rot` about its centre (card-local `c`, painted at `o`).
fn rounded(o: Point<Pixels>, c: (f32, f32), w: f32, h: f32, r: f32, rot: f32) -> Option<Path> {
    let mut pb = PathBuilder::fill();
    append_rounded(&mut pb, o, c, w, h, r, rot);
    pb.build().ok()
}

fn append_rounded(pb: &mut PathBuilder, o: Point<Pixels>, c: (f32, f32), w: f32, h: f32, r: f32, rot: f32) {
    let (s, k) = rot.sin_cos();
    let r = r.min(w / 2.).min(h / 2.);
    let mut first = true;
    for (ox, oy, start) in [(w / 2. - r, -h / 2. + r, -90f32), (w / 2. - r, h / 2. - r, 0.), (-w / 2. + r, h / 2. - r, 90.), (-w / 2. + r, -h / 2. + r, 180.)] {
        for i in 0..=6 {
            let a = (start + 15. * i as f32).to_radians();
            let (x, y) = (ox + r * a.cos(), oy + r * a.sin());
            let p = o + point(px(c.0 + x * k - y * s), px(c.1 + x * s + y * k));
            if first { pb.move_to(p); first = false; } else { pb.line_to(p); }
        }
    }
    pb.close();
}

impl Paper {
    /// A point on the paper, from its own top-left corner.
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        let (s, k) = self.rot.sin_cos();
        let (lx, ly) = (x - self.w / 2., y - self.h / 2.);
        (self.cx + lx * k - ly * s, self.cy + lx * s + ly * k)
    }

    /// The sheet, its hairline edge, a soft shadow and, unless real content
    /// lies on it, lines of "text" (below a real title when `titled`).
    fn paint(&self, o: Point<Pixels>, face: Face, window: &mut Window) {
        let b = Bounds::new(o + point(px(self.cx - self.w / 2.), px(self.cy - self.h / 2.)), size(px(self.w), px(self.h)));
        window.paint_drop_shadows(b, Corners::all(px(10.)), &[BoxShadow { color: theme::rgbaf(0, 0, 0, if theme::dark() { 0.3 } else { 0.07 }), offset: point(px(0.), px(3.)), blur_radius: px(10.), spread_radius: px(0.), inset: false }]);
        if let Some(p) = rounded(o, (self.cx, self.cy), self.w + 1., self.h + 1., 10.5, self.rot) { window.paint_path(p, ca(craft::N1000, 0.08)); }
        if let Some(p) = rounded(o, (self.cx, self.cy), self.w, self.h, 10., self.rot) { window.paint_path(p, theme::raised(Theme::bg_body())); }
        let mut rng = self.seed;
        let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; (rng % 1000) as f32 / 1000. };
        if face == Face::Content { return; }
        let pad = 12.;
        let mut y = if face == Face::Titled { 36. } else {
            // An untitled paper still carries a heading.
            let len = (self.w - 2. * pad) * (0.42 + 0.3 * next());
            if let Some(p) = rounded(o, self.at(pad + len / 2., 16.), len, 4., 2., self.rot) { window.paint_path(p, ca(craft::N900, 0.55)); }
            30.
        };
        let mut pb = PathBuilder::fill();
        let mut has_lines = false;
        while y < self.h - 14. {
            let full = self.w - 2. * pad;
            let len = if next() < 0.2 { full * (0.3 + 0.3 * next()) } else { full * (0.72 + 0.28 * next()) };
            append_rounded(&mut pb, o, self.at(pad + len / 2., y), len, 3., 1.5, self.rot);
            has_lines = true;
            y += 8.5;
        }
        if has_lines && let Ok(p) = pb.build() { window.paint_path(p, ca(craft::N1000, 0.10)); }
    }
}

/// The folder's back: a plate with a tab on its top-left.
fn back_plate(o: Point<Pixels>, p: &Pose, window: &mut Window) {
    let (w, h, r) = (p.w, p.h, 18.);
    let top = TAB * h + p.lift;
    let tab_top = p.lift;
    let tab = 0.40 * w;
    let pt = |x: f32, y: f32| o + point(px(x), px(y));
    let mut pb = PathBuilder::fill();
    pb.move_to(pt(0., h - r + p.lift));
    pb.line_to(pt(0., tab_top + r));
    pb.curve_to(pt(r, tab_top), pt(0., tab_top));
    pb.line_to(pt(tab - 22., tab_top));
    pb.cubic_bezier_to(pt(tab + 14., top), pt(tab - 6., tab_top), pt(tab - 2., top));
    pb.line_to(pt(w - r, top));
    pb.curve_to(pt(w, top + r), pt(w, top));
    pb.line_to(pt(w, h - r + p.lift));
    pb.curve_to(pt(w - r, h + p.lift), pt(w, h + p.lift));
    pb.line_to(pt(r, h + p.lift));
    pb.curve_to(pt(0., h - r + p.lift), pt(0., h + p.lift));
    pb.close();
    if let Ok(path) = pb.build() {
        window.paint_path(path, linear_gradient(180., linear_color_stop(ca(craft::N1000, 0.11), 0.), linear_color_stop(ca(craft::N1000, 0.07), 1.)));
    }
}

/// The frosted front: a sheet whose top steps down from a raised left part,
/// milky at the top and nearly opaque where the name sits, with a lit edge.
fn glass(o: Point<Pixels>, p: &Pose, window: &mut Window) {
    let (w, h, r) = (p.w, p.h + p.lift, 18.);
    let (yl, yr, i) = (p.yl + p.lift, p.yr + p.lift, p.inset);
    let step = 0.44 * w;
    let pt = |x: f32, y: f32| o + point(px(x), px(y));
    let edge = |pb: &mut PathBuilder| {
        pb.move_to(pt(i, yl + r));
        pb.curve_to(pt(i + r, yl), pt(i, yl));
        pb.line_to(pt(step - 16., yl));
        pb.cubic_bezier_to(pt(step + 26., yr), pt(step + 6., yl), pt(step + 6., yr));
        pb.line_to(pt(w - i - r, yr));
        pb.curve_to(pt(w - i, yr + r), pt(w - i, yr));
    };
    // The whole folder's shadow sits under the front.
    let lift = -p.lift / 4.;
    window.paint_drop_shadows(Bounds::new(pt(6., yr + 10.), size(px(w - 12.), px(h - yr - 10.))), Corners::all(px(r)),
        &[BoxShadow { color: theme::rgbaf(0, 0, 0, if theme::dark() { 0.35 } else { 0.06 + 0.04 * lift }), offset: point(px(0.), px(8. + 6. * lift)), blur_radius: px(22. + 10. * lift), spread_radius: px(-2.), inset: false }]);
    let mut pb = PathBuilder::fill();
    edge(&mut pb);
    pb.line_to(pt(w, h - r));
    pb.curve_to(pt(w - r, h), pt(w, h));
    pb.line_to(pt(r, h));
    pb.curve_to(pt(0., h - r), pt(0., h));
    pb.close();
    let face = theme::raised(Theme::bg_body());
    if let Ok(path) = pb.build() {
        // Milky where the papers show through, all but opaque by the name; a faint
        // cool tint keeps the glass apart from the white papers behind it.
        window.paint_path(path.clone(), linear_gradient(180., linear_color_stop(face.opacity(0.78), 0.), linear_color_stop(face.opacity(0.97), 0.42)));
        window.paint_path(path, linear_gradient(180., linear_color_stop(ca(craft::N1000, 0.012), 0.), linear_color_stop(ca(craft::N1000, 0.035), 1.)));
    }
    let mut hair = PathBuilder::stroke(px(1.));
    edge(&mut hair);
    hair.line_to(pt(w, h - r));
    hair.curve_to(pt(w - r, h), pt(w, h));
    hair.line_to(pt(r, h));
    hair.curve_to(pt(0., h - r), pt(0., h));
    hair.close();
    if let Ok(path) = hair.build() { window.paint_path(path, ca(craft::N1000, 0.07)); }
    // The lit top edge: brighter as the front leans into the light.
    let mut lit = PathBuilder::stroke(px(1.5));
    edge(&mut lit);
    if let Ok(path) = lit.build() { window.paint_path(path, theme::rgbaf(255, 255, 255, if theme::dark() { 0.18 } else { 0.85 + 0.15 * p.open.clamp(0., 1.) })); }
}

/// The order shown while dragging: the others as they were, the carried one at its gap.
fn drag_order(d: &CardDrag) -> Vec<&str> {
    let mut order: Vec<&str> = d.slots.iter().map(|(id, _)| id.as_str()).filter(|id| *id != d.id).collect();
    order.insert(d.target.min(order.len()), &d.id);
    order
}

/// Where the pointer aims a reorder: the folder whose middle it is on (`over`), or
/// the gap before or after the card / row under it, by which half it is on (else
/// the nearest one). The places are the pickup layout's, read through the drag's
/// order, so the gap moving never moves what is under the pointer; on the gap
/// itself nothing changes.
fn aim_reorder(d: &CardDrag, at: Point<Pixels>, takes: &dyn Fn(&str) -> bool) -> (Option<String>, Option<usize>) {
    let order = drag_order(d);
    let placed = d.slots.iter().enumerate().filter_map(|(i, (_, b))| b.map(|b| (i, b)));
    let far = |b: &Bounds<Pixels>| { let (dx, dy) = (f32::from(at.x - b.center().x), f32::from(at.y - b.center().y)); if d.list { dy.abs() } else { dx * dx + dy * dy } };
    let hit = placed.clone().find(|(_, b)| b.contains(&at));
    let Some((k, b)) = hit.or_else(|| placed.min_by(|(_, a), (_, b)| far(a).total_cmp(&far(b)))) else { return (None, None) };
    let Some(&occupant) = order.get(k).filter(|o| **o != d.id) else { return (None, None) };
    let (x, y) = (f32::from(at.x - b.left()) / f32::from(b.size.width), f32::from(at.y - b.top()) / f32::from(b.size.height));
    // A folder's middle takes it in; its sides, like any other card, are gaps.
    if hit.is_some() && takes(occupant) && (0.25..0.75).contains(if d.list { &y } else { &x }) { return (Some(occupant.to_string()), None); }
    let Some(j) = order.iter().filter(|o| **o != d.id).position(|o| *o == occupant) else { return (None, None) };
    let after = if d.list { y >= 0.5 } else { x >= 0.5 };
    (None, Some(j + usize::from(after)))
}

impl Workspace {
    fn folder_papers(&self, id: &str, front: Option<&String>) -> (Vec<Entry>, usize) {
        let key = self.entries.generation();
        let mut p = self.folder_page.folders.papers.borrow_mut();
        if p.key != Some(key) {
            let mut children: HashMap<String, Vec<usize>> = HashMap::new();
            for (i, e) in self.entries.iter().enumerate() {
                if let Some(parent) = &e.meta.parent { children.entry(parent.clone()).or_default().push(i); }
            }
            *p = Papers { key: Some(key), children, cards: HashMap::new() };
        }
        if let Some(c) = p.cards.get(&(id.to_string(), front.cloned())) { return c.clone(); }
        let n = p.children.get(id).map_or(0, Vec::len);
        // Papers stand for documents. A folder drawn as a paper reads as a note
        // with the folder's name on it, so only documents are painted — but a
        // folder usually holds folders for a while before a document appears,
        // and its documents usually sit deeper, so look all the way down and
        // paint the newest ones (#123).
        let mut kids: Vec<&Entry> = vec![];
        self.documents_below(&p.children, id, &mut kids, 0);
        kids.sort_by(|a, b| {
            let lead = |e: &Entry| front.is_some_and(|f| *f == e.id);
            lead(b).cmp(&lead(a)).then(b.meta.opened.max(b.meta.modified).cmp(&a.meta.opened.max(a.meta.modified)))
        });
        let card = (kids.into_iter().take(PAPERS).cloned().collect(), n);
        p.cards.insert((id.to_string(), front.cloned()), card.clone());
        card
    }

    /// Every document anywhere under `id`, newest first once sorted by the caller.
    fn documents_below<'a>(&'a self, children: &HashMap<String, Vec<usize>>, id: &str, out: &mut Vec<&'a Entry>, depth: usize) {
        // A folder that contains itself would otherwise recurse forever.
        if depth > 32 {
            return;
        }
        for &i in children.get(id).map_or(&[][..], Vec::as_slice) {
            let e = &self.entries[i];
            if e.meta.folder.is_some() {
                self.documents_below(children, &e.id, out, depth + 1);
            } else {
                out.push(e);
            }
        }
    }

    /// `w`: the grid's column width, this frame's (the recorded bounds are last
    /// frame's, stale for a frame after the sidebar opens or closes).
    pub(crate) fn render_folder_card(&mut self, e: &Entry, w: f32, card_h: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let id = e.id.clone();
        let h = card_h;
        // Carried away: its place an empty slot of a card's shape, below the tab as a document's.
        if self.card_dragging(&id) {
            return div().id(SharedString::from(format!("page-folder-{id}"))).relative().w_full().min_w_0().h(px(h)).flex().flex_col().justify_end()
                .child(self.record_card(&id))
                .child(crate::ui::squircle_surface(32., Theme::fill_hover()).w_full().h(px(h * (1. - TAB))))
                .into_any_element();
        }
        let hot = self.folder_page.folders.hovered.as_ref() == Some(&id) && self.folder_page.folders.drag.is_none();
        let over = self.folder_page.folders.drag.as_ref().is_some_and(|d| d.started && !d.returning && d.over.as_ref() == Some(&id)) || self.file_drop_over(&id);
        let front = self.folder_page.folders.fx.get(&id).and_then(|f| f.front.clone());
        let (kids, count) = self.folder_papers(&id, front.as_ref());

        // ---- Springs.
        let fx = self.folder_page.folders.fx.entry(id.clone()).or_insert_with(FolderFx::new);
        let now = Instant::now();
        let dt = fx.last.replace(now).map_or(1. / 120., |t| now.duration_since(t).as_secs_f32().min(0.032));
        fx.open.to = if hot || over { 1. } else { 0. };
        fx.mouth.to = if over { 1. } else { 0. };
        for p in &mut fx.papers { p.to = fx.open.to + 0.35 * fx.mouth.to; }
        let mut moving = fx.open.step(dt) | fx.mouth.step(dt) | fx.dip.step(dt);
        for p in &mut fx.papers { moving |= p.step(dt); }
        let (o, m, dip) = (fx.open.p, fx.mouth.p, fx.dip.p);
        let lift = -5. * o - 2. * m + 3. * dip;
        let pose_of = |i: usize, r: f32| -> Paper {
            // (dx, top, tilt°) at rest; how far hover moves them; how far a held card parts them.
            let (rest, rise, part) = [
                // Kept inside the card: tilted corners stay clear of the neighbours.
                ((0.0, 0.16, 0.0), (0.0, -0.07, 0.0), (0.0, -0.03, 0.0)),
                ((0.03, 0.135, 2.5), (0.03, -0.055, 2.0), (0.01, -0.02, 1.5)),
                ((-0.035, 0.115, -3.0), (-0.03, -0.05, -2.5), (-0.01, -0.02, -1.5)),
            ][i];
            let (pw, ph) = (0.80 * w, 0.64 * h);
            let dx = (rest.0 + rise.0 * r + part.0 * m) * w;
            let top = (rest.1 + rise.1 * r + part.1 * m) * h + lift;
            Paper { cx: w / 2. + dx, cy: top + ph / 2., w: pw, h: ph, rot: (rest.2 + rise.2 * r + part.2 * m).to_radians(), seed: 0 }
        };
        let mut papers: Vec<Paper> = kids.iter().enumerate().map(|(i, k)| Paper { seed: seed(&k.id), ..pose_of(i, fx.papers[i].p) }).collect();
        if let Some((aid, s)) = &mut fx.arrival {
            let t = pose_of(0, fx.papers[0].p);
            s.target = [t.cx - t.w / 2., t.cy - t.h / 2., t.w, t.h];
            let still = !s.step(dt);
            moving |= !still;
            if let Some(first) = papers.first_mut().filter(|_| kids.first().is_some_and(|k| &k.id == aid)) {
                let [x, y, pw, ph] = s.p;
                *first = Paper { cx: x + pw / 2., cy: y + ph / 2., w: pw, h: ph, rot: 0., seed: first.seed };
            }
            if still { fx.arrival = None; }
        }
        if moving { window.request_animation_frame(); }
        let lean = 0.03 * h * o + 0.045 * h * m + 0.05 * h * dip;
        let pose = Pose { w, h, lift, open: o, yl: 0.49 * h + lean, yr: 0.54 * h + lean, inset: -(3. * o + 7. * m), papers };

        // ---- Elements: back canvas, the front paper's title, the glass, the labels.
        let record = self.folder_page.folders.bounds.clone();
        let rid = id.clone();
        // The front paper is never tilted, so it carries the document's real first page.
        let front_face = if kids.first().is_some_and(|k| k.meta.folder.is_none()) { Face::Content } else { Face::Titled };
        let back = { let pose = pose.clone(); canvas(move |b, _, _| { record.borrow_mut().insert(rid.clone(), b); }, move |b, _, window, _| {
            back_plate(b.origin, &pose, window);
            for (i, p) in pose.papers.iter().enumerate().rev() { p.paint(b.origin, if i == 0 { front_face } else { Face::Sketch }, window); }
        }).absolute().inset_0() };
        let title = pose.papers.first().zip(kids.first()).map(|(p, k)| if front_face == Face::Content {
            div().absolute().left(px(p.cx - p.w / 2.)).top(px(p.cy - p.h / 2.)).w(px(p.w)).h(px(p.h)).rounded(px(10.)).overflow_hidden()
                .child(self.thumb_page(k, 12.))
        } else {
            div().absolute().left(px(p.cx - p.w / 2. + 12.)).top(px(p.cy - p.h / 2. + 10.)).w(px(p.w - 24.))
                .flex().items_center().gap(px(4.)).overflow_hidden()
                .text_size(px(11.)).line_height(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text())
                .when_some(k.icon.clone(), |d, em| d.child(div().flex_none().child(em)))
                .when(k.meta.folder.is_some(), |d| d.child(icon("folder", 12., Theme::icon())))
                .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(display_title(&k.title))))
        });
        let front_glass = { let pose = pose.clone(); canvas(|_, _, _| {}, move |b, _, window, _| glass(b.origin, &pose, window)).absolute().inset_0() };
        let menu_on = self.row_menu.as_ref().is_some_and(|r| r.id == id);
        // The buttons come with hover only, not under a carried card.
        let reveal = if menu_on { 1. } else { (o - m).clamp(0., 1.) };
        let (open_id, menu_id, new_id) = (id.clone(), id.clone(), id.clone());
        let hover_id = id.clone();
        let label = div().absolute().left(px(16.)).right(px(14.)).bottom(px(14. - lift)).flex().items_end().gap(px(8.))
            .child(div().flex_1().min_w_0().flex().flex_col()
                .child(div().text_size(px(15.)).line_height(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text())
                    .overflow_hidden().whitespace_nowrap().text_ellipsis().child(ui_text(display_title(&e.title))))
                .child(div().text_size(px(12.)).line_height(px(18.)).text_color(Theme::text_caption())
                    .child(crate::localized_count!(count, "{} 项", "{} item", "{} items", count))))
            // touch: no hover to show them by (a long press is the folder's menu), and they would take the name's room
            .when(self.batch.is_none() && !crate::design::touch(), |d| d
                // Outline buttons floating on the tile, round.
                .child(crate::design::Button::outline(SharedString::from(format!("page-fnew-{id}"))).sized(crate::design::ButtonSize::Tool).rounded_full().opacity(reveal)
                    .tip(crate::settings::tr("新建文档", "New document"), None)
                    .child(crate::design::icon("plus", 16., crate::design::color::muted_foreground()).with_transformation(Transformation::scale(size(0.7 + 0.3 * reveal, 0.7 + 0.3 * reveal))))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |w, _, window, cx| { cx.stop_propagation(); w.new_child(&new_id, window, cx); })))
                .child(self.card_more(e, format!("page-fmore-{id}"), cx).variant(crate::design::ButtonVariant::Outline).sized(crate::design::ButtonSize::Tool).rounded_full().opacity(reveal)));
        // 批量管理: checked like a document card, its box where a card's is (12 into the folder's body).
        let check = self.card_check(e).map(|c| div().absolute().left(px(12.)).top(px(TAB * h + 12. + lift)).child(c));
        div().id(SharedString::from(format!("page-folder-{id}"))).relative().w_full().min_w_0().h(px(h)).cursor_pointer()
            .on_hover(cx.listener(move |w, on: &bool, _, cx| {
                if *on { w.folder_page.folders.hovered = Some(hover_id.clone()); }
                else if w.folder_page.folders.hovered.as_ref() == Some(&hover_id) { w.folder_page.folders.hovered = None; }
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener({ let id = id.clone(); move |w, ev: &MouseDownEvent, _, cx| {
                // A press sinks the front a little; the release springs it back.
                if let Some(f) = w.folder_page.folders.fx.get_mut(&id) { f.dip.v += 2.2; }
                // Picked up like a document card (not in 批量管理, where a press checks it).
                if w.batch.is_none() { w.arm_card_drag(&id, ev.position, cx); }
                cx.notify();
            }}))
            .on_click(cx.listener(move |w, _, window, cx| if w.batch.is_some() { w.card_click(&open_id, window, cx) } else { w.try_open(&open_id, window, cx); }))
            .when(self.batch.is_none(), |d| d
                .on_mouse_down(MouseButton::Right, cx.listener(move |w, ev: &MouseDownEvent, _, cx| {
                    w.row_menu = Some(RowMenu { id: menu_id.clone(), pos: ev.position, corner: Anchor::TopLeft, sub: false, batch: false, from: None });
                    cx.notify();
                })))
            .child(back).children(title).child(front_glass).child(label).children(check)
            .children(crate::devtools::anchor_probe(format!("card:{}", display_title(&e.title))))
            .into_any_element()
    }

    /// The card's bounds are kept for picking it up (and for the flight back).
    pub(crate) fn record_card(&self, id: &str) -> impl IntoElement {
        let record = self.folder_page.folders.bounds.clone();
        let id = id.to_string();
        canvas(move |b, _, _| { record.borrow_mut().insert(id.clone(), b); }, |_, _, _, _| {}).absolute().inset_0()
    }

    /// A card or list row picked up on a folder page. A document card is
    /// carried as a small page skeleton, a folder card or a row as its row's
    /// face. Dropped on a folder's middle it goes inside; under 手动排序, put
    /// anywhere else it takes the gap the others opened for it (Finder's icon
    /// view: the half of a card the pointer is on says before or after it).
    pub(crate) fn arm_card_drag(&mut self, id: &str, at: Point<Pixels>, _cx: &mut Context<Self>) {
        let Some(origin) = self.folder_page.folders.bounds.borrow().get(id).copied() else { return };
        let grab = (((at.x - origin.left()) / origin.size.width).clamp(0., 1.), ((at.y - origin.top()) / origin.size.height).clamp(0., 1.));
        let r = [f32::from(origin.left()), f32::from(origin.top()), f32::from(origin.size.width), f32::from(origin.size.height)];
        // a phone's page keeps its own view (`phone.rs`)
        let list = if self.phone_paged() { self.folder_page.phone_list } else { !self.folder_page.grid };
        let chip = list || self.entries.iter().any(|e| e.id == id && e.meta.folder.is_some() && e.note.is_none());
        let slots: Vec<_> = if self.manual_page_sort() {
            let placed = self.folder_page.folders.slots.borrow();
            self.level_entries(self.folder_current.as_deref()).iter().map(|e| (e.id.clone(), placed.get(&e.id).copied())).collect()
        } else { vec![] };
        let target = slots.iter().position(|(k, _)| k == id).unwrap_or(0);
        self.folder_page.folders.drag = Some(CardDrag { id: id.to_string(), start: at, at, started: false, grab, origin, rect: Spring::new(r), over: None, returning: false, last: None, slots, target, chip, list, touch: false });
    }

    /// Touch: a long press on a document card picks it up at once (there is
    /// no pointer to move 5pt first). Returns whether it did.
    fn touch_card_press(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        // A phone's page: the card's menu first; the finger moving on carries the card (`phone.rs`).
        if self.phone_paged() { self.hold_card(id, at, cx); return true; }
        self.lift_card(id, at, cx)
    }

    pub(crate) fn lift_card(&mut self, id: &str, at: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        self.arm_card_drag(id, at, cx);
        let Some(d) = self.folder_page.folders.drag.as_mut() else { return false };
        d.started = true;
        d.touch = true;
        log::info!("touch: card {id} picked up at {at:?}");
        crate::phone::haptic(crate::phone::Haptic::Medium);
        self.row_menu = None;
        cx.notify();
        true
    }

    /// Touch: the rest of a long press that picked a card up.
    fn on_card_long_press(&mut self, ev: &LongPressEvent, cx: &mut Context<Self>) {
        if !self.folder_page.folders.drag.as_ref().is_some_and(|d| d.touch) { return; }
        match ev.phase {
            TouchPhase::Started => {}
            TouchPhase::Moved => self.move_card_drag(ev.position, cx),
            TouchPhase::Ended => {
                log::info!("touch: card let go at {:?} over {:?}", ev.position, self.folder_page.folders.drag.as_ref().and_then(|d| d.over.clone()));
                self.move_card_drag(ev.position, cx);
                self.end_card_drag(cx);
            }
            // Taken away (a system gesture): it flies back.
            TouchPhase::Cancelled => {
                log::info!("touch: card drag cancelled");
                if let Some(d) = self.folder_page.folders.drag.as_mut() { d.over = None; }
                self.end_card_drag(cx);
            }
        }
    }

    /// Touch: a document card's long press, which picks it up. The hitbox makes
    /// a menu or the sidebar's drawer over the card take the press instead.
    pub(crate) fn doc_card_hold(&self, id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, ws) = (id.to_string(), cx.entity());
        canvas(|b, window, _| window.insert_hitbox(b, HitboxBehavior::Normal), move |_, hitbox, window, _| {
            window.on_mouse_event(move |ev: &LongPressEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || ev.phase != TouchPhase::Started || !hitbox.is_hovered(window) { return; }
                if ws.update(cx, |w, cx| w.touch_card_press(&id, ev.position, cx)) { window.prevent_default(); cx.stop_propagation(); }
            });
        }).absolute().inset_0()
    }

    /// How far `id` stands from its own place in the drag's order (0 when nothing is reordered).
    fn drag_offset(&self, id: &str) -> [f32; 2] {
        let Some(d) = self.folder_page.folders.drag.as_ref().filter(|d| d.started && !d.returning && !d.slots.is_empty()) else { return [0., 0.] };
        let (Some(from), Some(to)) = (d.slots.iter().position(|(k, _)| k == id), drag_order(d).iter().position(|k| *k == id)) else { return [0., 0.] };
        match (d.slots[from].1, d.slots.get(to).and_then(|s| s.1)) {
            (Some(a), Some(b)) => [f32::from(b.left() - a.left()), f32::from(b.top() - a.top())],
            _ => [0., 0.],
        }
    }

    /// A card or list row in its layout place, moved aside by its spring while a drag reorders.
    pub(crate) fn card_motion(&self, id: &str, child: AnyElement) -> AnyElement {
        let target = self.drag_offset(id);
        let now = Instant::now();
        let mut motions = self.folder_page.folders.motion.borrow_mut();
        let (spring, last) = motions.entry(id.to_string()).or_insert((Spring::new([0., 0.]), now));
        spring.target = target;
        let dt = now.duration_since(*last).as_secs_f32().min(0.032);
        *last = now;
        let moving = spring.step(dt);
        let [x, y] = spring.p;
        if !moving && target == [0., 0.] { motions.remove(id); }
        let (slots, key) = (self.folder_page.folders.slots.clone(), id.to_string());
        div().relative().w_full().min_w_0()
            .child(canvas(move |b, _, _| { slots.borrow_mut().insert(key.clone(), b); }, move |_, _, window, _| if moving { window.request_animation_frame(); }).absolute().inset_0())
            .child(div().relative().left(px(x)).top(px(y)).child(child))
            .into_any_element()
    }

    /// The layout changed under the springs (a drop reordered the page, or took the carried
    /// card away): each keeps where it is drawn, measured from its new place.
    fn settle_card_motion(&self, order: &[&str]) {
        let Some(d) = self.folder_page.folders.drag.as_ref() else { return };
        let mut motions = self.folder_page.folders.motion.borrow_mut();
        for (to, id) in order.iter().enumerate() {
            let (Some(from), Some((spring, _))) = (d.slots.iter().position(|(k, _)| k == id), motions.get_mut(*id)) else { continue };
            if let (Some(a), Some(b)) = (d.slots[from].1, d.slots.get(to).and_then(|s| s.1)) {
                spring.p[0] -= f32::from(b.left() - a.left()); spring.p[1] -= f32::from(b.top() - a.top());
            }
            spring.target = [0., 0.];
        }
    }

    /// Before the drag starts the pointer is tracked by the folder page's body.
    pub(crate) fn track_card_press(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(d) = self.folder_page.folders.drag.as_mut().filter(|d| !d.started) else { return };
        if ev.pressed_button != Some(MouseButton::Left) { self.folder_page.folders.drag = None; return; }
        d.at = ev.position;
        if f32::from(d.at.x - d.start.x).hypot(f32::from(d.at.y - d.start.y)) >= 5. {
            d.started = true;
            self.row_menu = None;
            cx.notify();
        }
    }

    /// A folder that would take what is carried (its card or list row lights up).
    pub(crate) fn card_drop_over(&self, id: &str) -> bool {
        self.folder_page.folders.drag.as_ref().is_some_and(|d| d.started && !d.returning && d.over.as_deref() == Some(id))
    }

    pub(crate) fn card_dragging(&self, id: &str) -> bool {
        self.folder_page.folders.drag.as_ref().is_some_and(|d| d.started && d.id == id)
    }

    fn move_card_drag(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.folder_page.folders.drag.as_ref().is_some_and(|d| d.started && !d.returning) { return; }
        // Only folders on the page now: bounds linger for cards of pages left behind.
        let shown = self.level_entries(self.folder_current.as_deref());
        let takes = |id: &str| id != self.folder_page.folders.drag.as_ref().map_or("", |d| d.id.as_str()) && shown.iter().any(|e| e.id == id && e.meta.folder.is_some() && e.note.is_none());
        let (over, target) = match self.folder_page.folders.drag.as_ref().filter(|d| !d.slots.is_empty()) {
            Some(d) => aim_reorder(d, at, &takes),
            None => (shown.iter().filter(|e| takes(&e.id))
                .find(|e| self.folder_page.folders.bounds.borrow().get(&e.id).is_some_and(|b| b.contains(&at)))
                .map(|e| e.id.clone()), None),
        };
        let Some(d) = self.folder_page.folders.drag.as_mut() else { return };
        d.at = at;
        if let Some(t) = target { d.target = t; }
        if over != d.over {
            // The folder notices: a small dip as its mouth opens.
            if let Some(f) = over.as_ref().and_then(|k| self.folder_page.folders.fx.get_mut(k)) { f.dip.v -= 1.6; }
            // Touch: a tick as the card snaps to a folder.
            if over.is_some() { crate::phone::haptic(crate::phone::Haptic::Light); }
            d.over = over;
        }
        cx.notify();
    }

    /// Where the carried one flies back to: its card, or its row's face over its row.
    fn rest_rect(d: &CardDrag) -> [f32; 4] {
        let o = d.origin;
        if !d.chip { return [f32::from(o.left()), f32::from(o.top()), f32::from(o.size.width), f32::from(o.size.height)]; }
        let w = CHIP_W.min(f32::from(o.size.width));
        let x = f32::from(o.left()) + if d.list { 0. } else { (f32::from(o.size.width) - w) / 2. };
        [x, f32::from(o.top()) + (f32::from(o.size.height) - CHIP_H) / 2., w, CHIP_H]
    }

    pub(crate) fn has_card_drag(&self) -> bool { self.folder_page.folders.drag.is_some() }

    pub(crate) fn cancel_card_drag(&mut self, cx: &mut Context<Self>) {
        self.folder_page.folders.drag = None;
        cx.notify();
    }

    fn end_card_drag(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.folder_page.folders.drag.as_ref().filter(|d| d.started && !d.returning) else { self.folder_page.folders.drag = None; return };
        let target = d.over.clone().and_then(|f| self.folder_page.folders.bounds.borrow().get(&f).copied().map(|b| (f, b)));
        let source = d.slots.iter().position(|(k, _)| *k == d.id);
        if target.is_none() && source.is_some_and(|s| s != d.target) {
            // 手动排序: into the gap. The others stay where they are drawn; the card
            // comes down from where the carried one was let go.
            let (id, order) = (d.id.clone(), drag_order(d).into_iter().map(str::to_string).collect::<Vec<_>>());
            let before = order.iter().skip_while(|k| **k != id).nth(1).cloned();
            let [x, y, ..] = d.rect.p;
            let landing = d.slots.get(d.target).and_then(|s| s.1).map(|b| [x - f32::from(b.left()), y - f32::from(b.top())]);
            self.settle_card_motion(&order.iter().map(String::as_str).collect::<Vec<_>>());
            if let Some(p) = landing { self.folder_page.folders.motion.borrow_mut().insert(id.clone(), (Spring { p, v: [0.; 2], target: [0., 0.] }, Instant::now())); }
            self.folder_page.folders.drag = None;
            self.reorder_page_item(&id, before, cx);
            return;
        }
        let Some((folder, b)) = target else {
            // Not over a folder, nor moved: the carried one flies back and becomes the card again.
            let Some(d) = self.folder_page.folders.drag.as_mut() else { return };
            d.returning = true;
            d.rect.target = Self::rest_rect(d);
            cx.notify();
            return;
        };
        let id = d.id.clone();
        // Hand the paper to the folder where it is; it slides in behind the glass.
        let [x, y, w, h] = d.rect.p;
        let local = [x - f32::from(b.left()), y - f32::from(b.top()), w, h];
        // The others close the gap it leaves, from where they are drawn.
        let rest: Vec<&str> = d.slots.iter().map(|(k, _)| k.as_str()).filter(|k| *k != id).collect();
        self.settle_card_motion(&rest);
        self.folder_page.folders.drag = None;
        let fx = self.folder_page.folders.fx.entry(folder.clone()).or_insert_with(FolderFx::new);
        fx.arrival = Some((id.clone(), Spring::new(local)));
        fx.front = Some(id.clone());
        fx.dip.v += 5.5;
        self.move_into_folder(&id, &folder, cx);
    }

    pub(crate) fn render_card_drag_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !matches!(self.view, crate::app::View::Default | crate::app::View::Folder) { self.folder_page.folders.drag = None; return None; }
        let now = Instant::now();
        let dt = self.folder_page.folders.drag.as_ref().and_then(|d| d.last).map_or(1. / 120., |t| now.duration_since(t).as_secs_f32().min(0.032));
        // Touch: held near the grid's top or bottom, the grid scrolls under the
        // card, and the folder under it is looked for again.
        if let Some(at) = self.folder_page.folders.drag.as_ref().filter(|d| d.touch && d.started && !d.returning).map(|d| d.at) {
            let scroll = self.phone_grid_scroll().unwrap_or_else(|| self.page_grid_scroll());
            let (v, off, max) = (scroll.bounds(), scroll.offset(), scroll.max_offset());
            let dir = edge_scroll(f32::from(at.y), f32::from(v.top()), f32::from(v.bottom()));
            let y = (off.y - px(dir * EDGE_SPEED * dt)).clamp(-max.y, px(0.));
            if y != off.y { scroll.set_offset(point(off.x, y)); }
            self.move_card_drag(at, cx);
        }
        let folder_bounds = self.folder_page.folders.drag.as_ref().and_then(|d| d.over.clone()).and_then(|f| self.folder_page.folders.bounds.borrow().get(&f).copied());
        let d = self.folder_page.folders.drag.as_mut().filter(|d| d.started)?;
        d.last = Some(now);
        if !d.returning {
            // The size a paper has inside a folder, from pickup to drop; a row's face, a chip's.
            let (w, h) = if d.chip { (CHIP_W.min(f32::from(d.origin.size.width)), CHIP_H) } else { (MINI_W, MINI_H) };
            let grab_x = if d.chip { (d.grab.0 * f32::from(d.origin.size.width)).min(w - 24.).max(24.) / w } else { d.grab.0 };
            let (mut x, mut y) = (f32::from(d.at.x) - grab_x * w, f32::from(d.at.y) - if d.chip { 0.5 } else { d.grab.1 } * h);
            if let Some(b) = folder_bounds {
                // Drawn toward the folder's mouth, held just above the glass (a list row: toward its row).
                let (fx, fy) = if d.list { (x, f32::from(b.center().y) - h / 2.) } else { (f32::from(b.center().x) - w / 2., f32::from(b.top()) + 0.56 * f32::from(b.size.height) - h) };
                x += (fx - x) * 0.45; y += (fy - y) * 0.45;
            }
            d.rect.target = [x, y, w, h];
        }
        let moving = d.rect.step(dt);
        if d.returning && !moving { self.folder_page.folders.drag = None; cx.notify(); return None; }
        window.request_animation_frame();
        let [x, y, w, h] = d.rect.p;
        let e = self.entries.iter().find(|e| e.id == d.id)?.clone();
        let chip = d.chip;
        // The carried card is a page's skeleton (a heading bar and lines of text),
        // painted straight onto the GPU: nothing is laid out while it moves.
        let paper = Paper { cx: w / 2., cy: h / 2., w, h, rot: 0., seed: seed(&e.id) };
        let lines = canvas(|_, _, _| {}, move |b, _, window, _| paper.paint(b.origin, Face::Sketch, window)).absolute().inset_0();
        // A folder or a list row: its face as the sidebar draws it, on a lifted card.
        let face = chip.then(|| div().size_full().flex().items_center().pr(px(10.)).rounded(px(10.)).bg(theme::raised(Theme::bg_body())).text_color(Theme::text())
            .child(crate::sidebar_drag::row_face(&e).pl(px(10.))));
        let ws = cx.entity();
        let held = canvas(|_, _, _| {}, move |_, _, window, _| {
            window.on_mouse_event(move |ev: &LongPressEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble { ws.update(cx, |w, cx| w.on_card_long_press(ev, cx)); }
            });
        }).absolute().inset_0();
        let layer = div().id("page-drag-overlay").absolute().inset_0().occlude().cursor(CursorStyle::ClosedHand).child(held)
            .on_mouse_move(cx.listener(|w, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button != Some(MouseButton::Left) { w.cancel_card_drag(cx); return; }
                w.move_card_drag(ev.position, cx);
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|w, ev: &MouseUpEvent, window, cx| {
                let p = ev.position;
                let size = window.viewport_size();
                if p.x < px(0.) || p.y < px(0.) || p.x >= size.width || p.y >= size.height { w.cancel_card_drag(cx); return; }
                w.move_card_drag(p, cx);
                w.end_card_drag(cx);
            }))
            .child(div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h))
                .shadow(crate::design::shadow::toast()).rounded(px(10.))
                .map(|d| match face { Some(f) => d.child(f), None => d.child(lines) }));
        Some(deferred(layer).with_priority(5).into_any_element())
    }
}

// ---- Files dragged in from the Finder

/// A drag of files from outside (the Finder) over a folder's page:
/// the list dims behind a receiving tray, a glass folder whose papers (one
/// per file, up to three) rise out of it, follow the pointer and part as it
/// comes near; let go, they slide in, the folder dips, and the documents are
/// imported where it says. Held over a folder card, that folder opens its
/// mouth instead and the tray steps back.
pub(crate) struct FileDrop {
    paths: Vec<std::path::PathBuf>,
    md: usize,
    xml: usize,
    /// CSV / TSV: each becomes a 数据库.
    csv: usize,
    dirs: usize,
    other: usize,
    at: Point<Pixels>,
    /// The folder card under the pointer.
    over: Option<String>,
    /// The pane's bounds last frame: anywhere in it takes the drop.
    zone: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// Where it was let go, which the ripple spreads from.
    drop_at: Point<Pixels>,
    inside: bool,
    show: Fade,
    veil: Fade,
    arm: Fade,
    dip: Fade,
    land: Fade,
    shake: Fade,
    rise: [Fade; PAPERS],
    /// The pointer's place over the tray (−1…1 each way), which the papers lean toward.
    tilt: Spring<2>,
    /// Let go: where the files went (None: the top of 我的文档).
    landed: Option<Option<String>>,
    leaving: bool,
    born: Instant,
    last: Option<Instant>,
}

/// Tray illustration: the glass folder's box.
const ART_W: f32 = 196.;
const ART_H: f32 = 168.;

impl FileDrop {
    fn new(paths: Vec<std::path::PathBuf>) -> Self {
        let (mut md, mut xml, mut csv, mut dirs, mut other) = (0, 0, 0, 0, 0);
        for p in &paths {
            if p.fs_is_dir() { dirs += 1; }
            else if crate::local_files::is_markdown_path(p) { md += 1; }
            else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")) { xml += 1; }
            else if crate::transfer::is_csv_path(p) { csv += 1; }
            else { other += 1; }
        }
        Self {
            paths, md, xml, csv, dirs, other, at: point(px(0.), px(0.)), over: None, zone: Default::default(), inside: false, drop_at: point(px(0.), px(0.)),
            show: Fade::new(0., 240., 21.), veil: Fade::new(0., 160., 25.), arm: Fade::new(0., 300., 23.),
            dip: Fade::new(0., 520., 15.), land: Fade::new(0., 150., 21.), shake: Fade::new(0., 700., 9.),
            // Each paper surfaces a little after the one in front.
            rise: [Fade::new(0., 320., 21.), Fade::new(0., 210., 17.), Fade::new(0., 140., 14.)],
            tilt: Spring::new([0., 0.]), landed: None, leaving: false, born: Instant::now(), last: None,
        }
    }

    fn importable(&self) -> usize { self.md + self.xml + self.csv + self.dirs }

    /// The pointer went somewhere else that takes the drop (the sidebar).
    pub(crate) fn leave(&mut self) { if self.landed.is_none() { self.leaving = true; } }

    /// "2 个 Markdown 文件 · 1 个文件夹".
    fn summary(&self) -> String {
        let mut parts = vec![];
        if self.md > 0 { parts.push(crate::localized_count!(self.md, "{} 个 Markdown 文件", "{} Markdown file", "{} Markdown files", self.md)); }
        if self.xml > 0 { parts.push(crate::localized_count!(self.xml, "{} 个 DocxXML 文件", "{} DocxXML file", "{} DocxXML files", self.xml)); }
        if self.csv > 0 { parts.push(crate::localized_count!(self.csv, "{} 个 CSV 文件", "{} CSV file", "{} CSV files", self.csv)); }
        if self.dirs > 0 { parts.push(crate::localized_count!(self.dirs, "{} 个文件夹", "{} folder", "{} folders", self.dirs)); }
        if self.other > 0 && !parts.is_empty() { parts.push(crate::localized_format!("{} 个不支持的文件会被跳过", "{} unsupported skipped", self.other)); }
        parts.join(" · ")
    }

    /// The front paper's tag: what kind of file leads.
    fn tag(&self) -> Option<&'static str> {
        if self.md > 0 { Some("MD") } else if self.xml > 0 { Some("XML") } else if self.csv > 0 { Some("CSV") } else { None }
    }
}

impl Workspace {
    /// Files dragged over a folder's page (the pointer, the folder
    /// card under it, whether it is over the tray).
    pub(crate) fn file_drag_move(&mut self, ev: &DragMoveEvent<ExternalPaths>, cx: &mut Context<Self>) {
        // Every listener hears every move: off the page (over the sidebar) it steps back.
        if !ev.bounds.contains(&ev.event.position) {
            if let Some(d) = self.folder_page.folders.file_drop.as_mut() { d.leave(); cx.notify(); }
            return;
        }
        let paths = ev.drag(cx).paths().to_vec();
        self.file_drag_at(ev.event.position, paths, cx);
    }

    /// `file_drag_move` from a position and the dragged paths (scripted runs too).
    pub(crate) fn file_drag_at(&mut self, at: Point<Pixels>, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        if !matches!(self.view, crate::app::View::Default | crate::app::View::Folder) { return; }
        let fresh = self.folder_page.folders.file_drop.as_ref().is_none_or(|d| d.landed.is_some());
        if fresh { self.folder_page.folders.file_drop = Some(FileDrop::new(paths)); }
        let shown = self.level_entries(self.folder_current.as_deref());
        let over = shown.iter().filter(|e| e.meta.folder.is_some() && e.note.is_none())
            .find(|e| self.folder_page.folders.bounds.borrow().get(&e.id).is_some_and(|b| b.contains(&at)))
            .map(|e| e.id.clone());
        let Some(d) = self.folder_page.folders.file_drop.as_mut() else { return };
        d.leaving = false;
        if over != d.over {
            // The folder notices, as it does for a dragged card.
            if let Some(f) = over.as_ref().and_then(|k| self.folder_page.folders.fx.get_mut(k)) { f.dip.v -= 1.6; }
            d.over = over;
        }
        d.at = at;
        d.inside = d.zone.get().is_some_and(|z| z.contains(&at));
        cx.notify();
    }

    /// Let go over the list: import into the folder under the pointer, the
    /// folder page shown, or the top of 我的文档; nothing to import, a shake.
    pub(crate) fn file_dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        self.file_drop_paths(paths.paths().to_vec(), window, cx);
    }

    pub(crate) fn file_drop_paths(&mut self, paths: Vec<std::path::PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.view, crate::app::View::Default | crate::app::View::Folder) { return; }
        // The first listing still running: Markdown files and folders open where they are, as from the Finder.
        if self.store.loading() {
            self.folder_page.folders.file_drop = None;
            let open: Vec<_> = paths.into_iter().filter(|p| p.fs_is_dir() || crate::local_files::is_markdown_path(p)).collect();
            if open.is_empty() { self.still_loading(cx); } else { self.open_paths(&open, window, cx); }
            return;
        }
        if self.folder_page.folders.file_drop.is_none() { self.folder_page.folders.file_drop = Some(FileDrop::new(paths)); }
        let page = if self.view == crate::app::View::Folder { self.folder_current.clone() } else { None };
        let Some(d) = self.folder_page.folders.file_drop.as_mut() else { return };
        if d.importable() == 0 {
            d.shake.v += 34.;
            d.leaving = true;
            cx.notify();
            return;
        }
        let target = d.over.clone().or(page);
        d.landed = Some(target.clone());
        d.drop_at = d.at;
        d.land.to = 1.;
        d.dip.v += 5.5;
        if let Some(f) = d.over.as_ref().and_then(|k| self.folder_page.folders.fx.get_mut(k)) { f.dip.v += 5.5; }
        let (dirs, files): (Vec<_>, Vec<_>) = d.paths.iter().cloned().partition(|p| p.fs_is_dir());
        let files: Vec<_> = files.into_iter().filter(|p| crate::local_files::is_markdown_path(p) || p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml"))).collect();
        if !files.is_empty() { self.import_paths(&files, target.clone(), false, window, cx); }
        for dir in &dirs { self.import_folder(dir, target.clone(), false, window, cx); }
        cx.notify();
    }

    pub(crate) fn file_drop_over(&self, id: &str) -> bool {
        self.folder_page.folders.file_drop.as_ref().is_some_and(|d| d.landed.is_none() && !d.leaving && d.importable() > 0 && d.over.as_deref() == Some(id))
    }

    /// The veil and the tray, over the page while files are dragged in.
    pub(crate) fn render_file_drop(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !matches!(self.view, crate::app::View::Default | crate::app::View::Folder) { self.folder_page.folders.file_drop = None; return None; }
        let dragging = cx.has_active_drag() || crate::devtools::file_drag();
        let target_title = |w: &Self, id: Option<&String>| id.and_then(|id| w.entries.iter().find(|e| &e.id == id)).map(|e| display_title(&e.title))
            .unwrap_or_else(|| w.front_top_name());
        let page = if self.view == crate::app::View::Folder { self.folder_current.clone() } else { None };
        let d = self.folder_page.folders.file_drop.as_ref()?;
        let place = target_title(self, d.landed.clone().flatten().as_ref().or(d.over.as_ref()).or(page.as_ref()));
        let d = self.folder_page.folders.file_drop.as_mut()?;
        // The drag went away without a drop here (left the window, or let go elsewhere).
        if !dragging && d.landed.is_none() { d.leaving = true; }
        let now = Instant::now();
        let dt = d.last.replace(now).map_or(1. / 120., |t| now.duration_since(t).as_secs_f32().min(0.032));
        let reject = d.importable() == 0;
        // Over a folder card (and after a drop into one) the folder answers; the tray steps back.
        let over = d.over.is_some() && !reject && d.landed.as_ref().is_none_or(|t| t.is_some() && *t == d.over);
        d.show.to = if d.leaving { 0. } else { 1. };
        d.veil.to = if d.leaving { 0. } else if over { 0.12 } else { 1. };
        d.arm.to = if !reject && !d.leaving && (d.inside || d.landed.is_some()) { 1. } else { 0. };
        for (i, r) in d.rise.iter_mut().enumerate() { r.to = if d.leaving && d.landed.is_none() { 0. } else { 1. - 0.12 * i as f32 }; }
        let zone = d.zone.get();
        d.tilt.target = match zone.filter(|_| d.inside && d.landed.is_none()) {
            Some(z) => [((d.at.x - z.center().x) / (z.size.width / 2.)).clamp(-1., 1.), ((d.at.y - z.center().y) / (z.size.height / 2.)).clamp(-1., 1.)],
            None => [0., 0.],
        };
        for f in [&mut d.show, &mut d.veil, &mut d.arm, &mut d.dip, &mut d.land, &mut d.shake] { f.step(dt); }
        d.tilt.step(dt);
        for r in &mut d.rise { r.step(dt); }
        // Landed and settled: the tray goes.
        if d.landed.is_some() && d.land.p > 0.97 && !d.leaving { d.leaving = true; }
        if d.leaving && d.show.p < 0.01 && d.veil.p < 0.01 { self.folder_page.folders.file_drop = None; cx.notify(); return None; }
        // The tray breathes while it waits, so it draws every frame while shown.
        window.request_animation_frame();

        // ---- The illustration's pose.
        // `lit`: the pane's glow, full while the pointer is over it. The glass folder
        // opens only a little with it, so it keeps its shape.
        let (show, lit, land, dip) = (d.show.unit(), d.arm.p, d.land.p, d.dip.p);
        let arm = 0.3 * lit;
        let t = d.born.elapsed().as_secs_f32();
        let breath = 0.5 + 0.5 * (t * std::f32::consts::TAU / 1.8).sin();
        let shake = d.shake.p * (t * 46.).sin() * 0.35;
        let n = d.importable().clamp(1, PAPERS);
        let (w, h) = (ART_W, ART_H);
        let (o, m) = (arm.max(0.35 * show), arm);
        let lift = -5. * o - 2. * m + 3. * dip + 6. * land;
        let [tx, ty] = d.tilt.p;
        let pose_of = |i: usize, r: f32| -> Paper {
            let (rest, rise, part) = [
                ((0.0, 0.16, 0.0), (0.0, -0.07, 0.0), (0.0, -0.03, 0.0)),
                ((0.03, 0.135, 2.5), (0.03, -0.055, 2.0), (0.06, -0.02, 4.0)),
                ((-0.035, 0.115, -3.0), (-0.03, -0.05, -2.5), (-0.06, -0.02, -4.0)),
            ][i];
            let (pw, ph) = (0.62 * w, 0.62 * h);
            // Out of the folder by `r`, higher while armed, sunk back in by `land`.
            let up = r * (1.6 + 1.1 * arm) - 2.4 * land;
            let dx = (rest.0 + rise.0 * up + part.0 * m) * w + tx * (5. + 3. * i as f32) * (1. - land);
            let top = (rest.1 + rise.1 * up + part.1 * m) * h + lift + ty * 3. * (1. - land);
            Paper { cx: w / 2. + dx, cy: top + ph / 2., w: pw, h: ph, rot: (rest.2 + rise.2 * up + part.2 * m + 2.5 * tx * (1. - land)).to_radians(), seed: 7 + i as u64 }
        };
        let papers: Vec<Paper> = (0..n).map(|i| pose_of(i, d.rise[i].p)).collect();
        let lean = 0.03 * h * o + 0.045 * h * m + 0.05 * h * dip;
        let pose = Pose { w, h, lift, open: o, yl: 0.49 * h + lean, yr: 0.54 * h + lean, inset: -(3. * o + 7. * m), papers };
        let tag = d.tag();
        // The front paper's name: the first file of the kind it shows.
        let first = d.paths.iter().find(|p| match tag { Some("MD") => crate::local_files::is_markdown_path(p), Some("CSV") => crate::transfer::is_csv_path(p), Some(_) => p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")), None => p.fs_is_dir() })
            .or(d.paths.first()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let summary = d.summary();
        let record = d.zone.clone();
        let (at, drop_at, landed, veil_p, shake_x) = (d.at, d.drop_at, d.landed.is_some(), d.veil.p, shake * 18.);

        // ---- Elements.
        let accent = Theme::primary();
        let quiet = if reject { 1. } else { 0. };
        let (title, sub) = if reject {
            (crate::settings::tr("这些文件不能导入", "These files can't be imported").to_string(), crate::settings::tr("支持 Markdown、DocxXML 文件和文件夹", "Markdown, DocxXML files and folders are supported").to_string())
        } else if d.landed.is_some() {
            (crate::localized_format!("已复制到「{}」", "Copied into “{}”", place), crate::settings::tr("原文件没有改动", "The originals are untouched").to_string())
        } else {
            // 导入 copies: say so where the drop lands, and that the originals stay as they are.
            let _ = over;
            (crate::localized_format!("松手即复制到「{}」", "Release to copy into “{}”", place), crate::localized_format!("{} · 原文件不变", "{} · originals untouched", summary))
        };
        let back = { let pose = pose.clone(); canvas(|_, _, _| {}, move |b, _, window, _| {
            back_plate(b.origin, &pose, window);
            // The front paper is the first file: its kind and name in place of a heading.
            for (i, p) in pose.papers.iter().enumerate().rev() { p.paint(b.origin, if i == 0 { Face::Titled } else { Face::Sketch }, window); }
        }).absolute().inset_0() };
        let chip = pose.papers.first().copied().map(|p| div().absolute().left(px(p.cx - p.w / 2. + 10.)).top(px(p.cy - p.h / 2. + 9.)).w(px(p.w - 20.))
            .flex().items_center().gap(px(5.)).overflow_hidden()
            .map(|d| match tag {
                Some(t) => d.child(div().flex_none().px(px(4.)).h(px(14.)).rounded(px(4.)).bg(accent).flex().items_center()
                    .text_size(px(8.5)).line_height(px(12.)).font_weight(FontWeight::BOLD).text_color(gpui::white()).child(t)),
                None => d.child(icon("folder", 12., Theme::icon())),
            })
            .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(10.)).line_height(px(14.))
                .font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).child(ui_text(first.clone()))));
        let front = { let pose = pose.clone(); canvas(|_, _, _| {}, move |b, _, window, _| glass(b.origin, &pose, window)).absolute().inset_0() };
        let grow = 0.9 + 0.1 * show + 0.05 * lit;
        let art = div().relative().flex_none().w(px(w)).h(px(h)).ml(px(shake_x)).opacity(1. - 0.35 * quiet)
            .child(back).children(chip).child(front);
        // The middle: the glass folder on a halo of the accent, the line under it. No box:
        // the whole pane is the place to drop.
        let middle = div().relative().flex().flex_col().items_center().gap(px(6.))
            .opacity(show * (0.08 + 0.92 * ((veil_p - 0.12) / 0.88).clamp(0., 1.)).min(show))
            .child(div().relative().flex_none().w(px(w * grow)).h(px(h * grow)).flex().items_center().justify_center()
                .child(div().absolute().left(px(w * grow / 2. - 40.)).top(px(h * grow / 2. - 30.)).size(px(80.)).rounded_full()
                    .shadow(vec![BoxShadow { color: if reject { ca(craft::N1000, 0.06) } else { accent.opacity(0.10 + 0.10 * lit + 0.06 * breath) }, offset: point(px(0.), px(0.)), blur_radius: px(90.), spread_radius: px(40. + 24. * lit), inset: false }]))
                .child(art))
            .child(div().mt(px(14.)).text_size(px(18.)).line_height(px(26.)).font_weight(FontWeight::SEMIBOLD)
                .text_color(if reject { Theme::text_caption() } else { Theme::text() }).child(ui_text(title)))
            .child(div().text_size(px(13.)).line_height(px(20.)).text_color(Theme::text_caption()).child(ui_text(sub)));
        // The pane's edge: a glow breathing in from every side, a dashed line settling
        // onto it, a halo under the pointer, and on a drop a
        // ripple from where it landed while the edge flares.
        let flare = if landed { (land * std::f32::consts::PI).sin().max(0.) } else { 0. };
        let edge = canvas(move |b, _, _| record.set(Some(b)), move |b, _, window, _| window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
            let tint = if reject { ca(craft::N1000, 1.) } else { accent };
            let clear = tint.opacity(0.);
            let (x0, y0, wd, ht) = (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height));
            let rect = |x: f32, y: f32, w: f32, h: f32| Bounds::new(point(px(x), px(y)), size(px(w.max(0.)), px(h.max(0.))));
            // A dashed edge, set in from the pane: the place to let go. It settles
            // from further inside onto its place as it shows.
            let inset = 22. + 30. * (1. - show);
            let r: f32 = 88.;
            let ring = rect(x0 + inset + shake_x * 0.4, y0 + inset, wd - 2. * inset, ht - 2. * inset);
            // The glow starts at the dashed line and fades inward: the blurred shadow of a
            // frame just outside it, seen only inside. Blur keeps the falloff continuous
            // and rounds the corners.
            let depth = (46. + 34. * lit + 30. * flare) * show;
            let a = if reject { 0.10 } else { (0.16 + 0.06 * breath + 0.16 * lit + 0.2 * flare) * show };
            let (l, t, rw, rh, big) = (f32::from(ring.left()), f32::from(ring.top()), f32::from(ring.size.width), f32::from(ring.size.height), 400.);
            let glow = [BoxShadow { color: tint.opacity(a), offset: point(px(0.), px(0.)), blur_radius: px(depth), spread_radius: px(0.), inset: false }];
            // Masks are only rectangles, so the ring's squircle inside (see `ui::squircle_path`)
            // is cut in 1px rows through the corner bands (the dashed line covers the steps);
            // one rect between.
            let rr = r.min(rw / 2.).min(rh / 2.).max(0.);
            let n = rr.ceil() as usize;
            let rows = (0..n).flat_map(|i| {
                let (y, hh) = (i as f32, (rr - i as f32).min(1.));
                let dx = rr - (rr.powi(4) - (rr - y - hh / 2.).powi(4)).max(0.).powf(0.25);
                [rect(l + dx, t + y, rw - 2. * dx, hh), rect(l + dx, t + rh - y - hh, rw - 2. * dx, hh)]
            }).chain([rect(l, t + rr, rw, rh - 2. * rr)]);
            for clip in rows {
                window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                    // Each strip reaches a point inside, or GPUI culls a shadow wholly outside the clip.
                    for side in [rect(l - big, t - big, rw + 2. * big, big + 1.), rect(l - big, t + rh - 1., rw + 2. * big, big + 1.), rect(l - big, t, big + 1., rh), rect(l + rw - 1., t, big + 1., rh)] {
                        window.paint_drop_shadows(side, px(0.).into(), &glow);
                    }
                });
            }
            // The halo under the pointer.
            if !reject && !landed {
                let c = point(at.x.max(b.left()).min(b.right()), at.y.max(b.top()).min(b.bottom()));
                window.paint_drop_shadows(Bounds::centered_at(c, size(px(40.), px(40.))), px(20.).into(),
                    &[BoxShadow { color: accent.opacity(0.12 * lit * show), offset: point(px(0.), px(0.)), blur_radius: px(120.), spread_radius: px(70.), inset: false }]);
            }
            let ring_a = if reject { 0.22 } else { (0.40 + 0.12 * breath + 0.35 * lit + 0.2 * flare).min(1.) } * show;
            if let Some(p) = crate::ui::squircle_path(ring, r, 1., PathBuilder::stroke(px(2.)).dash_array(&[px(6.), px(5.)])) { window.paint_path(p, tint.opacity(ring_a)); }
            // The ripple from the drop.
            if landed {
                let far = wd.hypot(ht);
                for (k, delay) in [(0, 0.), (1, 0.18)] {
                    let q = ((land - delay) / (1. - delay)).clamp(0., 1.);
                    if q <= 0. { continue; }
                    let e = 1. - (1. - q).powi(3);
                    let rad = 30. + e * far * 0.8;
                    let c = point(drop_at.x, drop_at.y);
                    window.paint_quad(quad(Bounds::centered_at(c, size(px(2. * rad), px(2. * rad))), px(rad), clear, px(3. - k as f32), accent.opacity(0.5 * (1. - q)), BorderStyle::Solid));
                }
            }
        })).absolute().inset_0();
        let veil = theme::raised(Theme::bg_body()).opacity(0.86 * d.veil.p.clamp(0., 1.));
        Some(div().absolute().inset_0().bg(veil).child(edge).flex().items_center().justify_center().child(middle).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::{CardDrag, Spring, aim_reorder, drag_order};
    use gpui::{Bounds, point, px, size};

    /// Four cards in a row, 100 wide with 20 between; C is a folder; A is carried.
    fn row(target: usize) -> CardDrag {
        let slot = |i: usize| Some(Bounds::new(point(px(i as f32 * 120.), px(0.)), size(px(100.), px(120.))));
        let slots = ["A", "B", "C", "D"].iter().enumerate().map(|(i, k)| (k.to_string(), slot(i))).collect();
        CardDrag { id: "A".into(), start: point(px(50.), px(60.)), at: point(px(50.), px(60.)), started: true, grab: (0.5, 0.5), origin: slot(0).unwrap(),
            rect: Spring::new([0.; 4]), over: None, returning: false, last: None, slots, target, chip: false, list: false, touch: false }
    }
    fn aim(d: &CardDrag, x: f32) -> (Option<String>, Option<usize>) { aim_reorder(d, point(px(x), px(60.)), &|k| k == "C") }

    #[test]
    fn the_half_of_a_card_says_before_or_after_it() {
        let d = row(0);
        assert_eq!(aim(&d, 130.), (None, Some(0)), "B's left half: before B, where it was");
        assert_eq!(aim(&d, 190.), (None, Some(1)), "B's right half: after B");
        assert_eq!(aim(&d, 470.), (None, Some(3)), "past D: last");
        assert_eq!(aim(&d, 112.), (None, Some(0)), "the gutter: the nearest card's half");
    }

    #[test]
    fn a_folders_middle_takes_it_in_its_sides_reorder() {
        let d = row(0);
        assert_eq!(aim(&d, 290.), (Some("C".into()), None));
        assert_eq!(aim(&d, 245.), (None, Some(1)), "C's left edge: before C");
        assert_eq!(aim(&d, 335.), (None, Some(2)), "C's right edge: after C");
    }

    /// The places are the pickup layout's: once the gap moved under the pointer, it stays.
    #[test]
    fn the_gap_under_the_pointer_holds_still() {
        let mut d = row(0);
        let (_, t) = aim(&d, 190.);
        d.target = t.unwrap();
        assert_eq!(drag_order(&d), ["B", "A", "C", "D"]);
        assert_eq!(aim(&d, 150.), (None, None), "on the gap itself nothing changes");
        assert_eq!(aim(&d, 170.), (None, None));
        assert_eq!(aim(&d, 60.), (None, Some(1)), "B, now first, right half: after it, as it is");
    }

    #[test]
    fn a_carried_card_scrolls_the_grid_only_near_its_edges() {
        assert_eq!(super::edge_scroll(110., 100., 900.), -1.);
        assert_eq!(super::edge_scroll(500., 100., 900.), 0.);
        assert_eq!(super::edge_scroll(880., 100., 900.), 1.);
        // Past the edges (the finger on the toolbar above) it keeps going.
        assert_eq!(super::edge_scroll(40., 100., 900.), -1.);
        assert_eq!(super::edge_scroll(960., 100., 900.), 1.);
    }
}
