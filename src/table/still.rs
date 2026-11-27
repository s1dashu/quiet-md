//! A 数据库 in miniature: the top-left of its grid as `grid::render` draws it, at rest (no hover,
//! selection or scrolling), every measure times the thumbnail's scale. A folder page's card shows a
//! table file this way, and a page's card its 数据库 blocks.
//!
//! A folder page shows dozens of these, redrawn every frame it scrolls or a card's ring fades: built
//! as the grid's elements (a few hundred per card) they tripled the page's frame time. So the grid is
//! laid out once into a list of marks (fills, lines, shaped text, glyphs), in the grid's own tokens
//! and measures (`style.rs`, `grid::render`, `cell::value_view`: a change there belongs here too), and
//! each frame paints that list where the card is. The data is a `Still`, read on the background executor
//! each time the file changes (`folder_page::card_grid`); the marks are kept while the card's size, scale and look stay.

use super::style::*;
use super::*;
use std::cell::RefCell;

/// Rows and columns read for a card: enough for the tallest and widest card (a 336pt column's
/// 403pt card at 0.28 shows about 1300 × 1100 of the grid).
const ROOM_H: f32 = 1400.;
const ROOM_W: f32 = 1200.;

pub struct Thumb {
    still: Still,
    /// In a page, the block's name line over its frame (`menus::inline_header`); a table file's card has its title.
    name: Option<String>,
    /// 适应内容's row heights, measured the first time the card is drawn.
    fit: RefCell<Option<Vec<f32>>>,
    /// The marks last laid out, and what they were laid out for.
    face: RefCell<Option<(Key, Rc<Vec<Mark>>)>>,
}

/// What a layout depends on: scale, width, height (bits) and the look (a few of its colours).
type Key = (u32, u32, u32, u64);

enum Mark {
    Quad(PaintQuad),
    /// A line of text: its top-left and line height.
    Text(Point<Pixels>, Pixels, ShapedLine),
    /// Wrapped lines (a tall row's text), clipped to the cell.
    Wrapped(Point<Pixels>, Pixels, Rc<Vec<WrappedLine>>, Bounds<Pixels>),
    Glyph(Bounds<Pixels>, SharedString, Hsla),
}

impl Thumb {
    /// How much of a block to read for a card (`block_still`'s `rows` and `width`): the rows a card
    /// can show at the lowest row (a group's line or 低), the columns of the widest card.
    pub fn room() -> (usize, f32) { (((ROOM_H - header_h()) / group_h().min(row_px(RowHeight::Short))).ceil() as usize + 1, ROOM_W) }

    /// `still`, read (off the main thread) of the block `r`; `inline`: a block in a page (else a table file's card).
    pub fn new(still: Still, r: &TableRef, inline: bool) -> Rc<Thumb> {
        // a file made inside a page sits in .assets under an id, and goes by its table's name; a library file by its own
        let name = inline.then(|| if r.src.split('/').any(|c| c == ".assets") { still.table().map(|t| t.name.clone()).unwrap_or_default() }
            else { Path::new(&r.src).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() });
        Rc::new(Thumb { still, name, fit: RefCell::new(None), face: RefCell::new(None) })
    }

    /// Drawn `k` times its size, cut at `room` points high.
    pub fn element(self: &Rc<Self>, k: f32, room: f32) -> AnyElement { Grid { thumb: self.clone(), k, room }.into_any_element() }

    /// Each row's height (unscaled): a group's line, the view's row height or 适应内容's measure.
    fn heights(&self, cx: &App) -> Vec<f32> {
        let st = &self.still;
        let Some(t) = st.table() else { return vec![] };
        let fit = (st.view.opts.row_height == RowHeight::Fit).then(|| self.fit.borrow_mut().get_or_insert_with(|| {
            grid::fit_heights(&mut HashMap::new(), &st.base, t, &st.view, &query::ViewResult { rows: st.rows.clone(), records: vec![] }, cx)
        }).clone());
        st.rows.iter().enumerate().map(|(ri, r)| match r {
            query::Row::Group(_) => group_h(),
            query::Row::Record(_) => fit.as_ref().and_then(|f| f.get(ri).copied()).unwrap_or(row_px(st.view.opts.row_height)),
        }).collect()
    }
}

#[derive(IntoElement)]
struct Grid { thumb: Rc<Thumb>, k: f32, room: f32 }

impl RenderOnce for Grid {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (th, k) = (self.thumb, self.k);
        let heights = th.heights(cx);
        let rows_h: f32 = heights.iter().sum::<f32>() + if th.still.more { 0. } else { add_row_h() };
        let h = ((header_h() + rows_h) * k).min(self.room.max(0.));
        let inline = th.name.is_some();
        let painter = th.clone();
        let grid = canvas(|_, _, _| {}, move |bounds, _, window, cx| {
            let key: Key = (k.to_bits(), f32::from(bounds.size.width).to_bits(), f32::from(bounds.size.height).to_bits(), look());
            let marks = {
                let mut face = painter.face.borrow_mut();
                match &*face {
                    Some((had, m)) if *had == key => m.clone(),
                    _ => { let m = Rc::new(lay_out(&painter, &heights, k, bounds.size, window)); *face = Some((key, m.clone())); m }
                }
            };
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                let o = bounds.origin;
                for m in marks.iter() {
                    match m {
                        Mark::Quad(q) => { let mut q = q.clone(); q.bounds.origin += o; window.paint_quad(q); }
                        Mark::Text(at, lh, line) => { let _ = line.paint(o + *at, *lh, TextAlign::Left, None, window, cx); }
                        Mark::Wrapped(at, lh, lines, clip) => window.with_content_mask(Some(ContentMask { bounds: Bounds::new(o + clip.origin, clip.size) }), |window| {
                            let mut y = px(0.);
                            for l in lines.iter() { let _ = l.paint(o + *at + point(px(0.), y), *lh, TextAlign::Left, None, window, cx); y += l.size(*lh).height; }
                        }),
                        Mark::Glyph(b, path, color) => { let _ = window.paint_svg(Bounds::new(o + b.origin, b.size), path.clone(), None, TransformationMatrix::unit(), *color, cx); }
                    }
                }
            });
        }).w_full().h(px(h));
        // in a page: the block's name line, then the grid in its frame (the frame's shadow at the card's scale)
        let o = &th.still.view.opts;
        let shadow = crate::design::shadow::frame().into_iter().map(|mut s| { s.offset *= k; s.blur_radius *= k; s.spread_radius *= k; s }).collect();
        div().w_full().flex().flex_col()
            .when_some(th.name.clone(), |d, name| d.child(div().flex_none().h(px(INLINE_HEAD_H * k)).flex().items_center().justify_between().text_size(px(text_sm() * k)).font_family(crate::theme::DOC_UI_FONT).text_color(Theme::text())
                .child(div().flex_none().flex().items_center().gap(px(6. * k)).px(px(4. * k))
                    .child(img("illus/kinds/datatable.svg").flex_none().size(px(16. * k)))
                    .child(div().max_w(px(260. * k)).overflow_hidden().whitespace_nowrap().text_ellipsis().font_weight(FontWeight::MEDIUM).child(name)))
                // an active filter / sort / group shows at rest: the rows are not all there
                .when(!o.filter.conds.is_empty() || !o.sorts.is_empty() || !o.groups.is_empty(), |d| d.child(div().flex_none().mr(px(4. * k)).size(px(6. * k)).rounded_full().bg(crate::design::color::foreground())))))
            .child(div().w_full().overflow_hidden().bg(Theme::bg_body()).when(inline, |d| d.rounded(px(frame_r() * k)).shadow(shadow)).child(grid))
    }
}

/// The look, as a number: the colours the grid takes, so a theme or appearance change lays out again.
fn look() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    for c in [Theme::text(), Theme::bg_body(), header_bg(), line(), tag_colors(0).0, Theme::link()] { for v in [c.h, c.s, c.l, c.a] { v.to_bits().hash(&mut h); } }
    crate::settings::english().hash(&mut h);
    h.finish()
}

/// Lays marks down: points from the grid's top-left, every measure already scaled.
struct Pen<'a> { k: f32, marks: Vec<Mark>, ts: &'a WindowTextSystem }

/// A chip laid at (x, y's middle) in at most the width given; its width.
type Chip = Box<dyn Fn(&mut Pen, Pixels, Pixels, Pixels) -> Pixels>;

impl Pen<'_> {
    fn s(&self, v: f32) -> Pixels { px(v * self.k) }
    fn fill(&mut self, b: Bounds<Pixels>, c: Hsla, r: Pixels) { self.marks.push(Mark::Quad(fill(b, c).corner_radii(r))); }
    fn frame(&mut self, b: Bounds<Pixels>, bg: Hsla, edge: Hsla, r: Pixels) { self.marks.push(Mark::Quad(quad(b, r, bg, px(self.k), edge, BorderStyle::Solid))); }
    /// A grid line: `w` × `h` at (x, y).
    fn rule(&mut self, x: Pixels, y: Pixels, w: Pixels, h: Pixels, c: Hsla) { self.fill(Bounds::new(point(x, y), size(w, h)), c, px(0.)); }
    fn glyph(&mut self, name: &str, x: Pixels, y_mid: Pixels, sz: f32, c: Hsla) {
        let sz = self.s(sz);
        self.marks.push(Mark::Glyph(Bounds::new(point(x, y_mid - sz / 2.), size(sz, sz)), SharedString::from(format!("icons/{name}.svg")), c));
    }
    fn run(len: usize, weight: FontWeight, color: Hsla) -> TextRun {
        let mut f = font(crate::theme::DOC_UI_FONT);
        f.weight = weight;
        TextRun { len, font: f, color, background_color: None, underline: None, strikethrough: None }
    }
    fn shape(&self, text: &str, size: f32, weight: FontWeight, color: Hsla) -> ShapedLine {
        let text: SharedString = text.replace('\n', " ").into();
        self.ts.shape_line(text.clone(), self.s(size), &[Self::run(text.len(), weight, color)], None)
    }
    /// One line of text at `x`, centred on `y_mid` (a 20pt line), ending in … past `max`; its width.
    fn text(&mut self, text: &str, x: Pixels, y_mid: Pixels, max: Pixels, size: f32, weight: FontWeight, color: Hsla) -> Pixels {
        if text.is_empty() || max <= px(0.) { return px(0.) }
        let mut line = self.shape(text, size, weight, color);
        if line.width > max {
            let text: SharedString = text.replace('\n', " ").into();
            let run = Self::run(text.len(), weight, color);
            let (cut, runs) = self.ts.line_wrapper(run.font.clone(), self.s(size)).truncate_line(text, max, "…", std::slice::from_ref(&run), TruncateFrom::End);
            line = self.ts.shape_line(cut, self.s(size), &runs, None);
        }
        let (w, lh) = (line.width, self.s(20.));
        self.marks.push(Mark::Text(point(x, y_mid - lh / 2.), lh, line));
        w
    }
    /// A pill with words in it (`design::Badge`: h 20, px 8, text-xs); its width.
    fn badge(&mut self, text: &str, (bg, fg): (Hsla, Hsla), x: Pixels, y_mid: Pixels, max: Pixels) -> Pixels {
        let g = crate::design::geo();
        let tw = self.shape(text, g.text_xs, FontWeight::NORMAL, fg).width;
        let w = (tw + self.s(2. * g.badge_px + 2.)).min(self.s(180.)).min(max);
        if w <= px(0.) { return px(0.) }
        let h = self.s(g.badge_h);
        self.fill(Bounds::new(point(x, y_mid - h / 2.), size(w, h)), bg, h / 2.);
        self.text(text, x + self.s(g.badge_px + 1.), y_mid, w - self.s(2. * g.badge_px + 2.), g.text_xs, FontWeight::NORMAL, fg);
        w
    }
    /// `design::Checkbox`: 16, corners 4; on, `primary` with its tick.
    fn checkbox(&mut self, on: bool, x_mid: Pixels, y_mid: Pixels) {
        use crate::design::color;
        let g = crate::design::geo();
        let s = self.s(g.checkbox);
        let b = Bounds::new(point(x_mid - s / 2., y_mid - s / 2.), size(s, s));
        if on { self.fill(b, color::primary(), self.s(g.checkbox_r)); self.glyph("todo-check", x_mid - self.s(7.), y_mid, 14., color::primary_foreground()); }
        else { self.frame(b, color::field(), color::input(), self.s(g.checkbox_r)); }
    }
    /// `style::person`: the initial in its colour's circle, then the name, on a grey pill; its width.
    fn person(&mut self, name: &str, slot: u8, x: Pixels, y_mid: Pixels, max: Pixels) -> Pixels {
        let (bg, fg) = tag_colors(slot);
        let tw = self.shape(name, text_xs(), FontWeight::NORMAL, Theme::text()).width;
        let w = (tw + self.s(2. + 18. + 4. + 8.)).min(max);
        if w <= px(0.) { return px(0.) }
        let h = self.s(22.);
        self.fill(Bounds::new(point(x, y_mid - h / 2.), size(w, h)), crate::design::color::secondary(), h / 2.);
        let d = self.s(18.);
        self.fill(Bounds::new(point(x + self.s(2.), y_mid - d / 2.), size(d, d)), bg, d / 2.);
        let initial: String = name.chars().next().map(|c| c.to_string()).unwrap_or_default();
        let iw = self.shape(&initial, 10., FontWeight::NORMAL, fg).width;
        self.text(&initial, x + self.s(2.) + (d - iw) / 2., y_mid, d, 10., FontWeight::NORMAL, fg);
        self.text(name, x + self.s(24.), y_mid, w - self.s(32.), text_xs(), FontWeight::NORMAL, Theme::text());
        w
    }
    /// An attachment's chip (`cell::value_view`'s): a paperclip and the name in a hairline box; its width.
    fn file(&mut self, name: &str, x: Pixels, y_mid: Pixels, max: Pixels) -> Pixels {
        let tw = self.shape(name, 12., FontWeight::NORMAL, Theme::text()).width;
        let w = (tw + self.s(6. + 12. + 4. + 6. + 2.)).min(self.s(160.)).min(max);
        if w <= px(0.) { return px(0.) }
        let h = self.s(22.);
        self.frame(Bounds::new(point(x, y_mid - h / 2.), size(w, h)), transparent_black(), line(), self.s(4.));
        self.glyph("paperclip", x + self.s(7.), y_mid, 12., Theme::text_caption());
        self.text(name, x + self.s(23.), y_mid, w - self.s(30.), 12., FontWeight::NORMAL, Theme::text());
        w
    }

    /// A cell's value, as `cell::value_view` (and the grid's `cell::content`) draws it, in the box
    /// (`x`, `y_mid`, `w`) of a one-line row; a tall row's text wraps over `tall` (its top and foot).
    fn value(&mut self, base: &Base, f: &Field, v: &Value, x: Pixels, y_mid: Pixels, w: Pixels, tall: Option<(Pixels, Pixels)>) {
        // chips side by side (onto the next lines in a tall row)
        let mut chips: Vec<Chip> = vec![];
        match (f.ty, v) {
            (FieldType::Checkbox, v) => return self.checkbox(matches!(v, Value::Bool(true)), x + w / 2., y_mid),
            (_, Value::Empty) => {}
            (_, Value::Error(e)) => { self.text(e, x, y_mid, w, text_sm(), FontWeight::NORMAL, Theme::danger()); }
            (_, Value::Bool(b)) => self.checkbox(*b, x + w / 2., y_mid),
            (_, Value::Select(ids)) => for o in ids.iter().filter_map(|id| f.option(*id)) {
                let (name, color, person) = (o.name.clone(), o.color, f.ty == FieldType::Person);
                chips.push(Box::new(move |p, x, y, max| if person { p.person(&name, color, x, y, max) } else { p.badge(&name, tag_colors(color), x, y, max) }));
            },
            (_, Value::Links(ids)) => {
                let other = f.opts.link_table.and_then(|t| base.table(t));
                for id in ids {
                    let title = other.and_then(|t| t.record(*id).map(|r| t.title(r))).unwrap_or_default();
                    let title = if title.is_empty() { crate::settings::text("未命名记录").to_string() } else { title };
                    chips.push(Box::new(move |p, x, y, max| p.badge(&title, (crate::design::color::secondary(), crate::design::color::secondary_foreground()), x, y, max)));
                }
            }
            (_, Value::Files(fs)) => for a in fs { let name = a.name.clone(); chips.push(Box::new(move |p, x, y, max| p.file(&name, x, y, max))); },
            // the grid's stars: only those given show (the rest come with the pointer)
            (FieldType::Rating, Value::Number(n)) => for s in 1..=f.stars() {
                if (s as f64) <= *n { self.glyph("star", x + self.s((s - 1) as f32 * 17.), y_mid, 15., star()); }
            },
            (FieldType::Progress, Value::Number(n)) => {
                let label = field::format_number(*n, f);
                let tw = self.shape(&label, text_sm(), FontWeight::NORMAL, Theme::text()).width;
                let bar = (w - tw - self.s(8.)).max(self.s(20.));
                let (h, r) = (self.s(6.), self.s(3.));
                let b = Bounds::new(point(x, y_mid - h / 2.), size(bar, h));
                self.fill(b, crate::design::color::muted(), r);
                self.fill(Bounds::new(b.origin, size(bar * n.clamp(0., 1.) as f32, h)), Theme::primary(), r);
                self.text(&label, x + bar + self.s(8.), y_mid, w - bar - self.s(8.), text_sm(), FontWeight::NORMAL, Theme::text());
            }
            // numbers to the right
            (_, Value::Number(n)) => {
                let s = field::format_number(*n, f);
                let tw = self.shape(&s, text_sm(), FontWeight::NORMAL, Theme::text()).width.min(w);
                self.text(&s, x + w - tw, y_mid, w, text_sm(), FontWeight::NORMAL, Theme::text());
            }
            (FieldType::Url | FieldType::Email, Value::Text(s)) => { self.text(s, x, y_mid, w, text_sm(), FontWeight::NORMAL, Theme::link()); }
            // a 笔记 cell: its note, by the title its place carries
            (FieldType::Note, Value::Text(href)) => {
                self.glyph("doc", x, y_mid, 14., Theme::icon());
                self.text(field::note_title(href), x + self.s(20.), y_mid, w - self.s(20.), text_sm(), FontWeight::NORMAL, Theme::text());
            }
            (_, v) => {
                let s = match v { Value::Text(s) => s.clone(), v => field::display_in(base, f, v) };
                match tall {
                    Some((top, foot)) => {
                        let n = (((foot - top) / self.s(20.)).floor() as usize).max(1);
                        let run = Self::run(s.len(), FontWeight::NORMAL, Theme::text());
                        if let Ok(lines) = self.ts.shape_text(s.into(), self.s(text_sm()), &[run], Some(w), Some(n)) {
                            self.marks.push(Mark::Wrapped(point(x, top), self.s(20.), Rc::new(lines.into_vec()), Bounds::new(point(x, top), size(w, foot - top))));
                        }
                    }
                    None => { self.text(&s, x, y_mid, w, text_sm(), FontWeight::NORMAL, Theme::text()); }
                }
            }
        }
        // the chips: one line in a short row, wrapping in a tall one
        let (mut at, mut y) = (x, match tall { Some((top, _)) => top + self.s(10.), None => y_mid });
        for chip in chips {
            if at >= x + w {
                let Some((_, foot)) = tall else { break };
                if y + self.s(26.) > foot { break }
                (at, y) = (x, y + self.s(26.));
            }
            at += chip(self, at, y, x + w - at) + self.s(4.);
        }
    }
}

/// The grid's marks, as `grid::render` draws a view scrolled to its top-left, `room` wide and high.
fn lay_out(th: &Thumb, heights: &[f32], k: f32, room: Size<Pixels>, window: &Window) -> Vec<Mark> {
    let st = &th.still;
    let mut p = Pen { k, marks: vec![], ts: window.text_system() };
    let Some(t) = st.table() else { return p.marks };
    let (base, view) = (&st.base, &st.view);
    let fields = t.visible_fields(view);
    let s = |v: f32| px(v * k);
    let hair = s(1.);
    let (w_room, h_room) = (room.width, room.height);
    // columns: the gutter, then each field's width
    let mut xs = vec![s(GUTTER_W)];
    for f in &fields { xs.push(*xs.last().unwrap() + s(t.width(view, f.id))); }
    let total = *xs.last().unwrap();
    let frozen = view.opts.frozen.min(fields.len());
    let frozen_w = xs[frozen];
    // the columns that start inside the room
    let shown = xs.iter().take(fields.len()).take_while(|x| **x < w_room).count();
    let (hdr_h, pad) = (s(header_h()), s(cell_pad_x()));

    // ---- header: its fill, each name with its type in muted words, the gutter's checkbox ----
    p.fill(Bounds::new(point(px(0.), px(0.)), size(w_room, hdr_h)), header_bg(), px(0.));
    let mid = hdr_h / 2.;
    p.checkbox(false, s(grid::CHECK_X + grid::NUM_W / 2.), mid);
    p.rule(s(GUTTER_W) - hair, px(0.), hair, hdr_h, line());
    for (ci, f) in fields.iter().enumerate().take(shown) {
        let (x0, x1) = (xs[ci], xs[ci + 1]);
        let sorted = view.opts.sorts.iter().find(|o| o.field == f.id).map(|o| o.desc);
        let room_ = x1 - x0 - pad * 2. - if sorted.is_some() { s(16.) } else { px(0.) };
        let nw = p.text(&f.name, x0 + pad, mid, room_, text_sm(), FontWeight::MEDIUM, header_text());
        // the type only when it fits beside the name (the head wraps it out of sight)
        let ty = crate::settings::label(f.ty.label()).to_string();
        let tw = p.shape(&ty, text_xs(), FontWeight::NORMAL, Theme::text_placeholder()).width;
        if nw + s(6.) + tw <= room_ { p.text(&ty, x0 + pad + nw + s(6.), mid, tw + hair, text_xs(), FontWeight::NORMAL, Theme::text_placeholder()); }
        if let Some(desc) = sorted { p.glyph(if desc { "caret-down" } else { "caret-up" }, x1 - pad - s(12.), mid, 12., Theme::primary()); }
        p.rule(x1 - hair, px(0.), hair, hdr_h, line());
    }
    // the add-field column after the last one
    if !st.more_cols && total < w_room { p.glyph("plus", total + s((ADD_FIELD_W - 15.) / 2.), mid, 15., Theme::icon()); }
    p.rule(frozen_w - hair, px(0.), hair, hdr_h, frozen_line());
    p.rule(px(0.), hdr_h - hair, w_room, hair, header_line());

    // ---- rows, to the foot of the room ----
    let mut y = hdr_h;
    for (ri, row) in st.rows.iter().enumerate() {
        if y >= h_room { break }
        let h = s(heights[ri]);
        match row {
            query::Row::Group(g) => {
                p.fill(Bounds::new(point(px(0.), y), size(w_room, h)), if g.level == 0 { header_bg() } else { Theme::bg_body() }, px(0.));
                let mid = y + h / 2.;
                let mut x = s(12. + g.level as f32 * GROUP_INDENT);
                p.glyph(if g.folded { "triangle-right" } else { "triangle-down" }, x + s(3.), mid, 10., Theme::text_caption());
                x += s(16. + 6.);
                let f = t.field(g.field);
                x += p.text(&f.map(|f| f.name.clone()).unwrap_or_default(), x, mid, w_room - x, 12., FontWeight::NORMAL, Theme::text_caption()) + s(6.);
                match (&g.value, f) {
                    (Value::Select(ids), Some(f)) if !ids.is_empty() => for o in ids.iter().filter_map(|id| f.option(*id)) {
                        x += if f.ty == FieldType::Person { p.person(&o.name, o.color, x, mid, w_room - x) } else { p.badge(&o.name, tag_colors(o.color), x, mid, w_room - x) } + s(4.);
                    },
                    _ if g.label.is_empty() => x += p.text(crate::settings::label("空").as_ref(), x, mid, w_room - x, text_sm(), FontWeight::NORMAL, Theme::text_placeholder()) + s(6.),
                    _ => x += p.text(&g.label, x, mid, w_room - x, text_sm(), FontWeight::NORMAL, Theme::text()) + s(6.),
                }
                p.text(&g.count.to_string(), x + s(2.), mid, w_room - x, 12., FontWeight::NORMAL, Theme::text_placeholder());
                p.rule(px(0.), y + h - hair, w_room, hair, line());
            }
            query::Row::Record(i) => {
                let r = &t.records[*i];
                let row_w = total.max(frozen_w);
                if let Some(slot) = st.tints[ri] { p.fill(Bounds::new(point(px(0.), y), size(row_w, h)), row_tint(slot), px(0.)); }
                let one = s(heights[ri].min(one_line_h()) - 1.);
                let tall = heights[ri] > one_line_h();
                let n = st.numbers[ri].to_string();
                let nw = p.shape(&n, 12., FontWeight::NORMAL, Theme::text_caption()).width;
                p.text(&n, (s(GUTTER_W) - nw) / 2., y + h / 2., s(GUTTER_W), 12., FontWeight::NORMAL, Theme::text_caption());
                p.rule(s(GUTTER_W) - hair, y, hair, h, line());
                for (ci, f) in fields.iter().enumerate().take(shown) {
                    let (x0, x1) = (xs[ci], xs[ci + 1]);
                    // a one-line row centres its content; a taller one sets it 10 from the top, whole lines only
                    let span = tall.then(|| (y + s(10.), y + s(10.) + s(((heights[ri] - 20.) / 20.).floor() * 20.)));
                    p.value(base, f, r.get(f.id), x0 + pad, if tall { y + s(20.) } else { y + one / 2. }, x1 - x0 - pad * 2., span);
                    p.rule(x1 - hair, y, hair, h, line());
                }
                p.rule(frozen_w - hair, y, hair, h, frozen_line());
                p.rule(px(0.), y + h - hair, row_w, hair, line());
            }
        }
        y += h;
    }
    // the add-record line after the last row, the last column's edge running down through it
    if !st.more && y < h_room {
        let h = s(add_row_h());
        p.glyph("plus", (s(GUTTER_W) - s(15.)) / 2., y + h / 2., 15., Theme::text_placeholder());
        if total > frozen_w { p.rule(total - hair, y, hair, h, line()); }
        if th.name.is_none() { p.rule(px(0.), y + h - hair, w_room, hair, line()); }
    }
    p.marks
}
