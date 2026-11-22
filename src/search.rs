//! 搜索 dialog: a 540-wide white dialog with the input row, 最近浏览 / hit
//! rows and a gray footer. Locally there is one owner and no sessions or
//! spaces, so each row's extra line is just 所有者 + the opened / modified
//! times. Radius 16, quiet row fills, monochrome page glyphs.

use crate::app::Workspace;
use crate::settings::display_title;
use crate::store::Entry;
use crate::theme::{self, Theme};
use crate::trash::scrim;
use crate::ui::{ROW_RADIUS, TipExt, TipSide, format_visit, icon, ui_runs, ui_text};
use gpui::{prelude::FluentBuilder, *};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

/// How many documents 最近浏览 lists.
const RECENT: usize = 20;
/// Body text kept for a hit's two-line summary.
const SUMMARY_CHARS: usize = 160;
/// The summary joins its blocks with three no-break spaces.
const SUMMARY_SEP: &str = "\u{a0}\u{a0}\u{a0}";
/// Text column of a row: 540 − list padding 6 / 6 (the right one holds the
/// scrollbar) − row padding 12 / 8 − icon 24 + 10.
const INFO_W: f32 = 474.;
/// A hit row without a summary: padding 8 / 8, title 20, extra line 2 + 20.
const ROW_H: f32 = 58.;

/// Case-insensitive match position in `hay`, in `hay`'s own byte offsets.
pub(crate) fn find_ci(hay: &str, needle_lower: &str) -> Option<Range<usize>> {
    if needle_lower.is_empty() {
        return None;
    }
    find_folded(hay, &hay.to_lowercase(), needle_lower)
}

/// [`find_ci`] with `hay.to_lowercase()` already at hand.
fn find_folded(hay: &str, lower: &str, needle_lower: &str) -> Option<Range<usize>> {
    if lower.len() == hay.len() {
        let i = lower.find(needle_lower)?;
        (hay.is_char_boundary(i) && hay.is_char_boundary(i + needle_lower.len())).then(|| i..i + needle_lower.len())
    } else {
        let i = hay.find(needle_lower)?;
        Some(i..i + needle_lower.len())
    }
}

/// The 搜索 toggles at the right of the input row.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SearchOptions {
    /// 区分大小写: the query is matched as typed, not lowercased.
    pub case: bool,
    /// 全词匹配: a match may not run on into the letters or digits around it.
    pub word: bool,
    /// 只搜标题: bodies are not searched.
    pub title_only: bool,
}

/// A letter, digit or `_` that 全词匹配 won't cut through. CJK ideographs,
/// kana and Hangul are written without spaces, so each stands alone.
fn wordy(c: char) -> bool {
    (c.is_alphanumeric() || c == '_') && !matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7a3 | 0xf900..=0xfaff | 0x20000..=0x2fa1f)
}

/// `m` (a match of `q`) is a whole word of `hay`: a word character at
/// either end of `q` doesn't continue into one outside.
fn whole(hay: &str, m: &Range<usize>, q: &str) -> bool {
    let joins = |inner: Option<char>, outer: Option<char>| inner.is_some_and(wordy) && outer.is_some_and(wordy);
    !joins(q.chars().next(), hay[..m.start].chars().next_back()) && !joins(q.chars().next_back(), hay[m.end..].chars().next())
}

/// The first match of `q` in `hay` at or after `from`, under `o`, in `hay`'s
/// byte offsets. `lower` is `hay.to_lowercase()` (unused with `o.case`); `q`
/// is lowercase unless `o.case`.
fn find_opt(hay: &str, lower: &str, q: &str, o: SearchOptions, mut from: usize) -> Option<Range<usize>> {
    if q.is_empty() {
        return None;
    }
    loop {
        let r = if o.case {
            hay[from..].find(q).map(|i| i..i + q.len())
        } else if lower.len() == hay.len() && lower.is_char_boundary(from) {
            find_folded(&hay[from..], &lower[from..], q)
        } else {
            find_ci(&hay[from..], q)
        }?;
        let m = from + r.start..from + r.end;
        if !o.word || whole(hay, &m, q) {
            return Some(m);
        }
        from = m.start + hay[m.start..].chars().next().map_or(1, char::len_utf8);
    }
}

/// Every match of `q` in `hay`.
fn find_all(hay: &str, q: &str, o: SearchOptions) -> Vec<Range<usize>> {
    let lower = if o.case { String::new() } else { hay.to_lowercase() };
    let mut out = vec![];
    let mut at = 0;
    while let Some(m) = find_opt(hay, &lower, q, o, at) {
        at = m.end;
        out.push(m);
    }
    out
}

/// Body text from the block holding the first match, with about 16
/// characters of context before it and the following blocks after it.
pub(crate) fn snippet(body: &str, q: &str, o: SearchOptions) -> Option<(String, Vec<Range<usize>>)> {
    if q.is_empty() || o.title_only {
        return None;
    }
    snippet_folded(body, &if o.case { String::new() } else { body.to_lowercase() }, q, o)
}

/// [`snippet`] with `body.to_lowercase()` already at hand.
fn snippet_folded(body: &str, lower: &str, q: &str, o: SearchOptions) -> Option<(String, Vec<Range<usize>>)> {
    let text = snippet_text(body, lower, q, o)?;
    let hits = find_all(&text, q, o);
    Some((text, hits))
}

/// [`snippet_folded`]'s text without the hits; none under 只搜标题.
fn snippet_text(body: &str, lower: &str, q: &str, o: SearchOptions) -> Option<String> {
    if q.is_empty() || o.title_only {
        return None;
    }
    let m = find_opt(body, lower, q, o, 0)?;
    let line_start = body[..m.start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let before: Vec<(usize, char)> = body[line_start..m.start].char_indices().collect();
    let cut = if before.len() > 16 { line_start + before[before.len() - 16].0 } else { line_start };
    let mut text = String::from(if cut > line_start { "…" } else { "" });
    let mut chars = 0;
    for line in body[cut..].lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
        if !text.is_empty() && text != "…" {
            text.push_str(SUMMARY_SEP);
        }
        for ch in line.chars() {
            text.push(ch);
            chars += 1;
            if chars >= SUMMARY_CHARS {
                break;
            }
        }
        if chars >= SUMMARY_CHARS {
            break;
        }
    }
    Some(text)
}

/// The summary as two lines at most, ending in "…" when cut. GPUI's own
/// `line_clamp` truncates at twice the width, ignoring wrap leftovers.
pub(crate) fn summary_text(text: String, mut hits: Vec<Range<usize>>, width: f32, window: &Window) -> StyledText {
    let style = summary_style(window);
    let lines = |s: &str| summary_lines(s, &style, &[], width, window);
    let text = if lines(&text) <= 2 {
        text
    } else {
        let ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        let (mut lo, mut hi) = (0, ends.len());
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if lines(&format!("{}…", &text[..ends[mid]])) <= 2 { lo = mid } else { hi = mid }
        }
        let cut = ends[lo];
        hits.retain(|r| r.end <= cut);
        format!("{}…", &text[..cut])
    };
    let runs = ui_runs(&text, &style, &hits, Theme::link());
    StyledText::new(text).with_runs(runs)
}

fn summary_style(window: &Window) -> TextStyle {
    let mut style = window.text_style();
    style.font_family = theme::ui_family().into();
    style.font_size = px(12.).into();
    style.color = Theme::text_caption();
    style
}

/// How many lines `s`, with `hits` in the link colour, wraps to in a summary `width` wide.
fn summary_lines(s: &str, style: &TextStyle, hits: &[Range<usize>], width: f32, window: &Window) -> usize {
    window
        .text_system()
        .shape_text(s.to_string().into(), px(12.), &ui_runs(s, style, hits, Theme::link()), Some(px(width)), None)
        .map(|ls| ls.iter().map(|l| l.wrap_boundaries().len() + 1).sum::<usize>())
        .unwrap_or(1)
}

/// A library document's title and body, lowercased once (`None` when that
/// changes nothing): lowercasing all of them on every keystroke, several
/// times a frame, made typing in 搜索 lag on big libraries (#332).
struct Folded {
    id: String,
    title: String,
    body: Arc<str>,
    title_l: Option<String>,
    body_l: Option<Box<str>>,
}

impl Folded {
    fn new(e: &Entry) -> Self {
        let title_l = e.title.to_lowercase();
        let body_l = e.body.to_lowercase();
        Folded {
            id: e.id.clone(),
            title: e.title.clone(),
            body: e.body.clone(),
            title_l: (title_l != e.title).then_some(title_l),
            body_l: (*body_l != *e.body).then(|| body_l.into()),
        }
    }
    fn title_l(&self) -> &str { self.title_l.as_deref().unwrap_or(&self.title) }
    fn body_l(&self) -> &str { self.body_l.as_deref().unwrap_or(&self.body) }
    /// Still describes `e`'s text.
    fn holds(&self, e: &Entry) -> bool {
        self.id == e.id && self.title == e.title && (Arc::ptr_eq(&self.body, &e.body) || self.body == e.body)
    }
}

/// The search index: [`Folded`] copies aligned with `Workspace::entries`,
/// checked against them on each use (they are replaced in many places), and
/// the last query's hits with their row heights.
#[derive(Default)]
pub(crate) struct SearchCache {
    docs: Vec<Folded>,
    /// Bumped whenever `docs` changes; older hits are stale.
    generation: u64,
    hits: Option<Hits>,
    /// The toggles in effect.
    pub(crate) options: SearchOptions,
    /// Advances of printable ASCII in the summary font (界面字体), for [`min_width`].
    ascii: Option<(&'static str, [f32; 128])>,
}

struct Hits {
    generation: u64,
    q: String,
    options: SearchOptions,
    /// Indices into the entries: title hits, then body hits, each in list order.
    rows: Vec<usize>,
    /// Each row's height in the 界面字体 it was measured in on first render.
    heights: Option<(&'static str, Vec<f32>)>,
}

impl SearchCache {
    fn sync(&mut self, entries: &[Entry]) {
        if self.docs.len() == entries.len() && self.docs.iter().zip(entries).all(|(d, e)| d.holds(e)) {
            // Same text in a new allocation: keep the new one so the next check is a pointer compare.
            for (d, e) in self.docs.iter_mut().zip(entries) { d.body = e.body.clone(); }
            return;
        }
        let mut old: HashMap<String, Folded> = std::mem::take(&mut self.docs).into_iter().map(|d| (d.id.clone(), d)).collect();
        self.docs = entries
            .iter()
            .map(|e| match old.remove(&e.id) {
                Some(d) if d.holds(e) => Folded { body: e.body.clone(), ..d },
                _ => Folded::new(e),
            })
            .collect();
        self.generation += 1;
    }

    /// The hits for `q` (not empty; lowercase unless 区分大小写) among `entries`.
    fn rows(&mut self, entries: &[Entry], q: &str) -> &mut Hits {
        self.sync(entries);
        let o = self.options;
        if !matches!(&self.hits, Some(h) if h.generation == self.generation && h.q == q && h.options == o) {
            // No toggle on: a plain `contains` on the lowercased text.
            let plain = o == SearchOptions::default();
            let hit = |text: &str, lower: &str| if plain { lower.contains(q) } else { find_opt(text, lower, q, o, 0).is_some() };
            let (mut rows, mut body) = (vec![], vec![]);
            for (i, d) in self.docs.iter().enumerate() {
                // A folder with a 正文 is found as that document (#357), not twice.
                if entries[i].note.is_some() { continue; }
                if hit(&d.title, d.title_l()) {
                    rows.push(i)
                } else if !o.title_only && hit(&d.body, d.body_l()) {
                    body.push(i)
                }
            }
            rows.extend(body);
            self.hits = Some(Hits { generation: self.generation, q: q.into(), options: o, rows, heights: None });
        }
        self.hits.as_mut().unwrap()
    }
}

/// A lower bound for the summary width of `s`: ASCII at 90% of its advance
/// (cached in `ascii`), CJK ideographs, kana and Hangul at 80% of the em,
/// the rest 0.
fn min_width(ascii: &mut Option<(&'static str, [f32; 128])>, s: &str, style: &TextStyle, window: &Window) -> f32 {
    let family = theme::ui_family();
    if ascii.as_ref().is_none_or(|(f, _)| *f != family) {
        let ts = window.text_system();
        let font = ts.resolve_font(&style.font());
        *ascii = Some((family, std::array::from_fn(|c| match c as u8 {
            b' '..=b'~' => ts.advance(font, px(12.), c as u8 as char).map(|s| f32::from(s.width)).unwrap_or(0.),
            _ => 0.,
        })));
    }
    let adv = &ascii.as_ref().unwrap().1;
    s.chars()
        .map(|c| match c as u32 {
            c @ 0..128 => adv[c as usize] * 0.9,
            0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7a3 => 12. * 0.8,
            _ => 0.,
        })
        .sum()
}

/// The glyph of a file titled by its name: Markdown, a 画板; else a page.
pub(crate) fn file_glyph(m: &crate::store::Meta) -> &'static str {
    match m.kind {
        crate::store::DocKind::Board => "whiteboard",
        crate::store::DocKind::Table => "storage",
        _ if m.markdown => "doc-md",
        _ => "doc",
    }
}

/// A result's icon: the page's emoji, or the page glyph.
pub(crate) fn entry_icon(e: &Entry) -> Div {
    match &e.icon {
        _ if e.meta.named_by_file() => doc_icon_named(file_glyph(&e.meta)),
        Some(em) => div().flex_none().size(px(24.)).mr(px(10.)).flex().items_center().justify_center().text_size(px(20.)).child(em.clone()),
        None => doc_icon(),
    }
}

/// A page without an emoji: the monochrome page glyph (the sidebar's), in
/// a 24 slot.
pub(crate) fn doc_icon() -> Div { doc_icon_named("doc") }

/// A page glyph by name (`doc`, `doc-md` for a Markdown file), in a 24 slot.
pub(crate) fn doc_icon_named(name: &str) -> Div {
    div()
        .flex_none()
        .size(px(24.))
        .mr(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .child(icon(name, 18., Theme::icon()))
}

/// The 24px square buttons at the right of the input row.
fn input_button(id: &'static str, glyph: &'static str, size: f32, color: Hsla) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .hover(|s| s.bg(Theme::fill_hover()))
        .child(icon(glyph, size, color))
}

/// A text-glyph toggle of the input row, as 文档内查找's `.*`: 22 square,
/// accent-filled while on.
fn option_toggle(id: &'static str, glyph: &'static str, underline: bool, on: bool, tip: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if on { Theme::primary() } else { Theme::text_caption() })
        .when(on, |d| d.bg(crate::feishu_theme::Theme::fill_active()))
        .when(!on, |d| d.hover(|s| s.bg(Theme::fill_hover_strong())))
        .when(underline, |d| d.underline())
        .child(glyph)
        .tip_label(tip.into(), TipSide::Below)
}

/// One `·`-separated part of a row's extra line.
fn extra_segment(glyph: Option<&'static str>, text: String) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .children(glyph.map(|g| icon(g, 12., Theme::icon()).mr(px(4.))))
        .child(ui_text(text))
}

/// A result's extra line: 所有者 and the opened / modified times.
pub(crate) fn extra_line(e: &Entry) -> Div {
    let mut extra = vec![extra_segment(None, crate::settings::tr("所有者：我", "Owner: me").into())];
    if e.meta.opened > 0 {
        extra.push(extra_segment(Some("eye"), crate::localized_format!("你在 {}打开过", "Opened {}", format_visit(e.meta.opened))));
    }
    extra.push(extra_segment(Some("pen-line"), crate::localized_format!("我在 {}修改过", "Modified {}", format_visit(e.meta.modified))));
    let last = extra.len() - 1;
    let extra = extra.into_iter().enumerate().flat_map(|(i, seg)| {
        let seg = if i == last { seg.flex_shrink_1().min_w_0().overflow_hidden() } else { seg };
        [(i > 0).then(|| div().flex_none().child(" · ")), Some(seg)].into_iter().flatten()
    });
    div()
        .mt(px(2.))
        .h(px(20.))
        .flex()
        .items_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(12.))
        .line_height(px(20.))
        .text_color(Theme::text_caption())
        .children(extra)
}

/// A result's title (13pt semibold, hits in the link colour), summary and extra line.
pub(crate) fn result_info(e: &Entry, q: &str, o: SearchOptions, summary: Option<(String, Vec<Range<usize>>)>, width: f32, window: &Window) -> Div {
    let title = display_title(&e.title);
    let title_hit = find_opt(&title, &if o.case { String::new() } else { title.to_lowercase() }, q, o, 0);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Theme::text())
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(ui_text(title).hit(title_hit, Theme::link())),
        )
        .children(summary.map(|(text, hits)| div().text_size(px(12.)).line_height(px(20.)).child(summary_text(text, hits, width, window))))
        .child(extra_line(e))
}

impl Workspace {
    pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = true;
        self.search_sel = Some(0);
        self.row_menu = None;
        self.doc_menu = false;
        self.doc_sub = None;
        let search = self.search.clone();
        search.update(cx, |s, cx| {
            // The dialog's query reads at body size, above the 13pt rows.
            s.size = 15.;
            s.line_height = 22.;
            s.set_text("", cx)
        });
        window.focus(&search.focus_handle(cx), cx);
        self.search_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    pub(crate) fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search_open {
            return;
        }
        self.search_open = false;
        if let Some(ed) = &self.editor {
            { let f = ed.read(cx).focus.clone(); window.focus(&f, cx); }
        }
        cx.notify();
    }

    /// First result is selected; an empty result list has no selection.
    pub(crate) fn default_search_sel(&self, cx: &App) -> Option<usize> {
        (!self.search_results(cx).is_empty()).then_some(0)
    }

    pub(crate) fn pick_search(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.search_results(cx).get(row).map(|e| e.id.clone()) else { return };
        self.close_search(window, cx);
        self.open(&id, window, cx);
        if let Some(ed) = &self.editor {
            { let f = ed.read(cx).focus.clone(); window.focus(&f, cx); }
        }
    }

    pub(crate) fn move_search_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.search_results(cx).len();
        if n == 0 { self.search_sel = None; return; }
        let next = match self.search_sel {
            Some(i) => (i as isize + delta).rem_euclid(n as isize) as usize,
            None if delta > 0 => 0,
            None => n - 1,
        };
        self.search_sel = Some(next);
        let title = usize::from(self.search_query(cx).is_empty());
        self.search_scroll.scroll_to_item(next + title);
        cx.notify();
    }

    /// Title hits first, then body hits; with no query, 最近浏览.
    pub fn search_results(&self, cx: &App) -> Vec<&Entry> {
        self.search_rows(&self.search_query(cx)).into_iter().map(|i| &self.entries[i]).collect()
    }

    /// The query: trimmed, and lowercased unless 区分大小写.
    fn search_query(&self, cx: &App) -> String {
        if self.search_cache.borrow().options.case { self.search.read(cx).text.trim().to_string() } else { self.query(cx) }
    }

    fn search_options(&self) -> SearchOptions {
        self.search_cache.borrow().options
    }

    /// Flip one toggle; the list starts over at its first hit.
    fn toggle_search_option(&mut self, flip: fn(&mut SearchOptions), window: &mut Window, cx: &mut Context<Self>) {
        flip(&mut self.search_cache.borrow_mut().options);
        self.search_sel = self.default_search_sel(cx);
        self.search_scroll.set_offset(point(px(0.), px(0.)));
        window.focus(&self.search.focus_handle(cx), cx);
        cx.notify();
    }

    /// [`Self::search_results`] as indices into `entries`.
    fn search_rows(&self, q: &str) -> Vec<usize> {
        if q.is_empty() {
            let mut recent: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].note.is_none()).collect();
            recent.sort_by_key(|&i| std::cmp::Reverse(self.entries[i].meta.opened.max(self.entries[i].meta.modified)));
            recent.truncate(RECENT);
            return recent;
        }
        self.search_cache.borrow_mut().rows(&self.entries, q).rows.clone()
    }

    /// Each hit row's height for `q` (not empty): the summary's line count
    /// decides it, so only rows near the view need building.
    fn search_heights(&self, q: &str, window: &Window) -> Vec<f32> {
        let mut cache = self.search_cache.borrow_mut();
        let cache = &mut *cache;
        if let Some((font, h)) = &cache.rows(&self.entries, q).heights && *font == theme::ui_family() {
            return h.clone();
        }
        let SearchCache { docs, hits, ascii, options, .. } = cache;
        let o = *options;
        let hits = hits.as_mut().unwrap();
        let style = summary_style(window);
        let mut heights = Vec::with_capacity(hits.rows.len());
        for &i in &hits.rows {
            let d = &docs[i];
            heights.push(match snippet_text(&d.body, d.body_l(), q, o) {
                None => ROW_H,
                // Clearly wider than a line: two lines, cut by `summary_text` if longer.
                Some(text) if min_width(ascii, &text, &style, window) > INFO_W + 12. => ROW_H + 40.,
                // Else shaped as `summary_text` and the row will.
                Some(text) if summary_lines(&text, &style, &[], INFO_W, window) > 2 => ROW_H + 40.,
                Some(text) => {
                    let hits = find_all(&text, q, o);
                    ROW_H + 20. * summary_lines(&text, &style, &hits, INFO_W, window) as f32
                }
            });
        }
        hits.heights = Some((theme::ui_family(), heights.clone()));
        heights
    }

    /// Result `ix`, the entry at `i`.
    fn search_row(&self, i: usize, ix: usize, q: &str, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let e = &self.entries[i];
        let o = self.search_options();
        let summary = match self.search_cache.borrow().docs.get(i) {
            Some(d) if d.holds(e) && !o.title_only => snippet_folded(&d.body, d.body_l(), q, o),
            _ => snippet(&e.body, q, o),
        };
        // Result indices match keyboard selection.
        let row = ix;
        div()
            .id(("search-hit", ix))
            .flex_none()
            .pt(px(8.))
            .pr(px(8.))
            .pb(px(8.))
            .pl(px(12.))
            .flex()
            .items_center()
            .rounded(px(ROW_RADIUS))
            .cursor_pointer()
            .when(self.search_sel == Some(row), |d| d.bg(Theme::fill_hover()))
            .on_hover(cx.listener(move |w, hovered: &bool, _, cx| {
                if *hovered && w.search_sel != Some(row) {
                    w.search_sel = Some(row);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |w, _, window, cx| w.pick_search(row, window, cx)))
            .child(entry_icon(e))
            .child(result_info(e, q, o, summary, INFO_W, window))
    }

    pub(crate) fn render_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.search_open {
            return None;
        }
        let q = self.search_query(cx);
        let o = self.search_options();
        let has_text = !self.search.read(cx).text.is_empty();
        let results = self.search_rows(&q);
        let vh = f32::from(window.viewport_size().height);
        let dialog_h = 630f32.min(vh - 120.).max(200.);

        let input = div()
            .flex_none()
            .h(px(46.))
            .pl(px(12.))
            .pr(px(8.))
            .flex()
            .items_center()
            .child(
                // The glyph's ink fills 14.6 of the 16px box; Lucide's fills 20/24.
                div()
                    .relative()
                    .flex_none()
                    .size(px(16.))
                    .ml(px(4.))
                    .mr(px(10.))
                    .child(icon("search", 17.5, Theme::icon()).absolute().top(px(-0.75)).left(px(-0.75))),
            )
            .child(div().flex_1().min_w_0().pl(px(2.)).flex().child(self.search.clone()))
            .when(has_text, |d| {
                d.child(input_button("search-clear", "search-clear", 18., Theme::text_disabled()).ml(px(8.)).on_click(cx.listener(
                    |w, _, window, cx| {
                        w.search.update(cx, |s, cx| s.set_text("", cx));
                        window.focus(&w.search.focus_handle(cx), cx);
                        w.search_sel = Some(0);
                        w.search_scroll.set_offset(point(px(0.), px(0.)));
                        cx.notify();
                    },
                )))
            })
            // Aa / ab / T: 区分大小写, 全词匹配, 只搜标题 — styled as 文档内查找's `.*`.
            .child(div().flex_none().ml(px(8.)).flex().items_center().gap(px(2.))
                .child(option_toggle("search-case", "Aa", false, o.case, crate::settings::tr("区分大小写", "Match case"))
                    .on_click(cx.listener(|w, _, window, cx| w.toggle_search_option(|o| o.case = !o.case, window, cx))))
                .child(option_toggle("search-word", "ab", true, o.word, crate::settings::tr("全词匹配", "Match whole word"))
                    .on_click(cx.listener(|w, _, window, cx| w.toggle_search_option(|o| o.word = !o.word, window, cx))))
                .child(option_toggle("search-title", "T", false, o.title_only, crate::settings::tr("只搜标题", "Titles only"))
                    .on_click(cx.listener(|w, _, window, cx| w.toggle_search_option(|o| o.title_only = !o.title_only, window, cx)))));

        let mut list = div()
            .id("search-results")
            .flex_1()
            .min_h_0()
            .pt(px(4.))
            .px(px(6.))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.search_scroll);
        if q.is_empty() && !results.is_empty() {
            list = list.child(
                div()
                    .flex_none()
                    .h(px(36.))
                    .pt(px(2.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(Theme::text_caption())
                    .child(crate::settings::tr("最近浏览", "Recently viewed")),
            );
        }
        if results.is_empty() && !q.is_empty() {
            list = list.child(
                div()
                    .flex_none()
                    .m(px(8.))
                    .px(px(10.))
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(Theme::text_caption())
                    .child(crate::settings::text("没有找到匹配的结果，尝试其他关键词进行搜索")),
            );
        }
        if q.is_empty() {
            for (ix, &i) in results.iter().enumerate() {
                list = list.child(self.search_row(i, ix, &q, window, cx));
            }
        } else {
            // Hits can number thousands: rows away from the view and the
            // selection (which a keyboard move scrolls to this frame) are
            // spacers of the row's height, so the scrollbar and scroll
            // positions stay the same.
            let heights = self.search_heights(&q, window);
            let view = dialog_h - 46. - 1. - 36.;
            let top = -f32::from(self.search_scroll.offset().y);
            let (mut y, mut sel) = (4., None);
            if let Some(s) = self.search_sel.filter(|&s| s < heights.len()) {
                let (y0, y1) = (4. + heights[..s].iter().sum::<f32>(), 4. + heights[..=s].iter().sum::<f32>());
                // Off screen, it is scrolled to the top or bottom edge.
                if y0 < top || y1 > top + view { sel = Some((y0 - view, y1 + view)) }
            }
            for (ix, (&i, &h)) in results.iter().zip(&heights).enumerate() {
                let near = |(a, b): (f32, f32)| y + h > a && y < b;
                list = if near((top - 100., top + view + 100.)) || sel.is_some_and(near) {
                    list.child(self.search_row(i, ix, &q, window, cx).into_any_element())
                } else {
                    list.child(div().flex_none().h(px(h)).into_any_element())
                };
                y += h;
            }
        }

        let footer = div()
            .flex_none()
            .h(px(36.))
            .pl(px(14.))
            .pr(px(8.))
            .flex()
            .items_center()
            // GPUI clips children to the dialog's rect, not its rounded corners.
            .rounded_b(px(16.))
            .border_t_1()
            .border_color(Theme::line_divider())
            .bg(Theme::bg_overlay())
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(Theme::text_caption())
            .whitespace_nowrap()
            .overflow_hidden()
            .child(ui_text(crate::settings::tr("支持 ↑↓ 键选择、Enter 键打开", "Use ↑↓ to select, Enter to open")));

        let modal = div()
            .id("search-dialog")
            .occlude()
            .w(px(540.))
            .h(px(dialog_h))
            .flex()
            .flex_col()
            .rounded(px(16.))
            .bg(crate::theme::raised(Theme::bg_body()))
            .shadow(theme::shadow_dialog())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(input)
            .child(div().flex_none().h(px(1.)).bg(Theme::line_divider()))
            // The thumb sits in the list's right padding, over no row.
            .child(div().flex_1().min_h_0().relative().flex().flex_col().child(list).child(self.home.search_scrollbar.render(&self.search_scroll)))
            .child(footer);

        Some(
            deferred(
                div()
                    .id("search-mask")
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .bg(scrim())
                    .on_mouse_down(MouseButton::Left, cx.listener(|w, _, window, cx| w.close_search(window, cx)))
                    .child(modal),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchCache, SearchOptions, find_all, snippet};
    use crate::store::Entry;

    fn entry(id: &str, title: &str, body: &str) -> Entry {
        Entry { id: id.into(), meta: Default::default(), title: title.into(), icon: None, body: body.into(), reminders: vec![], links: vec![], note: None, root: String::new() }
    }

    /// The pre-#332 ranking: lowercase each title, then each body.
    fn reference(entries: &[Entry], q: &str) -> Vec<usize> {
        let mut hits: Vec<(u8, usize)> = entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if e.title.to_lowercase().contains(q) { Some((0, i)) } else if e.body.to_lowercase().contains(q) { Some((1, i)) } else { None }
            })
            .collect();
        hits.sort_by_key(|(rank, _)| *rank);
        hits.into_iter().map(|(_, i)| i).collect()
    }

    #[test]
    fn hits_rank_titles_first_in_list_order() {
        let entries = vec![
            entry("a", "Notes", "the Apple pie"),
            entry("b", "APPLE harvest", ""),
            entry("c", "Ärger", "ÄPFEL und apple"),
            entry("d", "plain", "nothing"),
            entry("e", "Apple again", "apple"),
            entry("f", "苹果", "Apple 苹果"),
        ];
        let mut cache = SearchCache::default();
        for q in ["apple", "äp", "ä", "苹果", "e", "zzz"] {
            assert_eq!(cache.rows(&entries, q).rows, reference(&entries, q), "{q}");
        }
        assert_eq!(cache.rows(&entries, "apple").rows, [1, 4, 0, 2, 5]);
    }

    #[test]
    fn hits_follow_library_changes() {
        let mut entries = vec![entry("a", "One", "alpha"), entry("b", "Two", "beta")];
        let mut cache = SearchCache::default();
        assert_eq!(cache.rows(&entries, "beta").rows, [1]);
        cache.rows(&entries, "beta").heights = Some(("", vec![58.]));
        let generation = cache.generation;
        // The same text in new allocations (a fresh `Store::list`) keeps the hits.
        let fresh: Vec<Entry> = entries.iter().map(|e| entry(&e.id, &e.title, e.body.as_ref())).collect();
        assert!(cache.rows(&fresh, "beta").heights.is_some());
        assert_eq!(cache.generation, generation);
        // An edited body, a renamed title, an added and a removed document each drop them.
        entries[0].body = "alpha beta".into();
        assert_eq!(cache.rows(&entries, "beta").rows, [0, 1]);
        assert!(cache.rows(&entries, "beta").heights.is_none());
        entries[1].title = "Beta".into();
        assert_eq!(cache.rows(&entries, "beta").rows, [1, 0]);
        entries.insert(0, entry("c", "Gamma", "BETA"));
        assert_eq!(cache.rows(&entries, "beta").rows, [2, 0, 1]);
        entries.remove(2);
        assert_eq!(cache.rows(&entries, "beta").rows, [0, 1]);
        assert_eq!(cache.rows(&entries, "gamma").rows, [0]);
        // Reordered (sorted by modification) entries map to their new places.
        entries.reverse();
        assert_eq!(cache.rows(&entries, "gamma").rows, [1]);
        assert_eq!(cache.rows(&[], "gamma").rows, Vec::<usize>::new());
    }

    #[test]
    fn snippet_context() {
        let d = SearchOptions::default();
        let (s, r) = snippet("first line\nsome long prefix text before the Needle here\nnext", "needle", d).unwrap();
        assert_eq!(&s[r[0].clone()], "Needle");
        assert!(s.starts_with('…'));
        assert!(s.ends_with("\u{a0}\u{a0}\u{a0}next"));
        let (s, r) = snippet("中文内容测试内容", "内容", d).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(&s[r[1].clone()], "内容");
        assert!(snippet("abc", "x", d).is_none());
    }

    const CASE: SearchOptions = SearchOptions { case: true, word: false, title_only: false };
    const WORD: SearchOptions = SearchOptions { case: false, word: true, title_only: false };
    const TITLE: SearchOptions = SearchOptions { case: false, word: false, title_only: true };

    #[test]
    fn match_case_and_whole_word() {
        let all = |hay: &str, q: &str, o| find_all(hay, q, o).into_iter().map(|r| hay[r].to_string()).collect::<Vec<_>>();
        assert_eq!(all("Apple apple APPLE", "apple", SearchOptions::default()), ["Apple", "apple", "APPLE"]);
        assert_eq!(all("Apple apple APPLE", "Apple", CASE), ["Apple"]);
        // Whole words: not inside pineapple / apples / apple_pie; punctuation and CJK end a word.
        assert_eq!(all("pineapple apples apple_pie (apple) 吃apple了 Apple.", "apple", WORD), ["apple", "apple", "Apple"]);
        let both = SearchOptions { case: true, word: true, title_only: false };
        assert_eq!(all("Apple apple Apples", "Apple", both), ["Apple"]);
        // A query ending in punctuation only needs the word side bounded.
        assert_eq!(all("a.b a.bc xa.b", "a.b", WORD), ["a.b"]);
        // CJK has no word breaks: a CJK query still matches inside CJK text.
        assert_eq!(all("中文内容测试内容", "内容", WORD), ["内容", "内容"]);
        // Folding that changes the byte length (İ) keeps offsets on `hay`.
        assert_eq!(all("İstanbul istanbul", "istanbul", WORD).len(), 1);
    }

    #[test]
    fn options_filter_and_rank_hits() {
        let entries = vec![
            entry("a", "Rust notes", "trust the borrow checker"),
            entry("b", "Plans", "learn rust this year"),
            entry("c", "rusty tools", "RUST"),
            entry("d", "Other", "Rust"),
        ];
        let mut cache = SearchCache::default();
        assert_eq!(cache.rows(&entries, "rust").rows, [0, 2, 1, 3]);
        cache.options = WORD;
        assert_eq!(cache.rows(&entries, "rust").rows, [0, 1, 2, 3]);
        cache.options = CASE;
        assert_eq!(cache.rows(&entries, "Rust").rows, [0, 3]);
        assert_eq!(cache.rows(&entries, "rust").rows, [2, 0, 1]);
        cache.options = TITLE;
        assert_eq!(cache.rows(&entries, "rust").rows, [0, 2]);
        assert!(snippet("learn rust", "rust", TITLE).is_none());
        cache.options = SearchOptions { case: true, word: true, title_only: true };
        assert_eq!(cache.rows(&entries, "Rust").rows, [0]);
        // Switching back gives the plain hits again, not a stale cache.
        cache.options = SearchOptions::default();
        assert_eq!(cache.rows(&entries, "rust").rows, [0, 2, 1, 3]);
    }
}
