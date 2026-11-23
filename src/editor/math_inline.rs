//! 公式原位编辑 (experimental; 设置 › 通用 › 在原文中编辑公式): instead of the
//! popover, ← / → and clicks go into an inline equation, which shows its
//! LaTeX source in place (text.rs `MathSource`) with the rendering in a
//! preview below. The visit itself lives in core (`math_edit.rs`); this is the
//! keyboard, IME, pointer and preview side.

use super::core::{self, Pos};
use super::keymap as k;
use super::text::MathSource;
use super::view::{Editor, from16, to16};
use crate::editor_theme::Theme;
use gpui::*;
use std::any::Any;
use std::ops::Range;

/// The setting: off keeps the popover.
pub fn enabled() -> bool {
    crate::settings::prefs().inline_math
}

impl Editor {
    /// What block `id` draws for the visit, if it is there.
    pub(super) fn math_shown(&self, id: crate::model::BlockId) -> Option<MathSource> {
        let m = self.core.math.filter(|m| !self.read_only && m.at.block == id && self.core.sel == core::Selection::caret(m.at))?;
        let src = self.core.equation_source(m.at)?;
        let r = m.range();
        let ok = |o: usize| o <= src.len() && src.is_char_boundary(o);
        if !ok(r.start) || !ok(r.end) {
            return None;
        }
        let marked = self.math_marked.clone().filter(|r| ok(r.start) && ok(r.end) && r.start <= r.end);
        Some(MathSource { ix: m.at.offset, src, sel: r, head: m.head, marked })
    }

    /// Start a visit to the equation at `at`, the caret at `offset` of its source.
    pub(super) fn math_open(&mut self, at: Pos, offset: usize, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.read_only || !self.core.math_enter(at, offset) {
            return false;
        }
        self.math_marked = None;
        self.marked = None;
        self.menu = None;
        self.changed(window, cx);
        true
    }

    /// ← / → next to an equation: into its source instead of over it.
    pub(super) fn math_enter_side(&mut self, right: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !enabled() || self.read_only || !self.core.math_enter_from_side(right) {
            return false;
        }
        self.math_marked = None;
        self.goal_x = None;
        self.changed(window, cx);
        true
    }

    /// An editor action during a visit: the ones that edit or move in the
    /// source run there (true). Any other one ends the visit, the caret
    /// before the equation, and then runs as usual (false).
    pub(super) fn math_action(&mut self, a: &dyn Any, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.core.math.is_none() || !self.focus.is_focused(window) || self.core.math_valid().is_none() {
            return false;
        }
        let c = &mut self.core;
        let is = |t: std::any::TypeId| a.type_id() == t;
        use std::any::TypeId as T;
        let len = c.math_source().map_or(0, |s| s.len());
        if is(T::of::<k::MoveLeft>()) { c.math_move(false, false, false); }
        else if is(T::of::<k::MoveRight>()) { c.math_move(true, false, false); }
        else if is(T::of::<k::SelectLeft>()) { c.math_move(false, false, true); }
        else if is(T::of::<k::SelectRight>()) { c.math_move(true, false, true); }
        else if is(T::of::<k::WordLeft>()) { c.math_move(false, true, false); }
        else if is(T::of::<k::WordRight>()) { c.math_move(true, true, false); }
        else if is(T::of::<k::SelectWordLeft>()) { c.math_move(false, true, true); }
        else if is(T::of::<k::SelectWordRight>()) { c.math_move(true, true, true); }
        else if is(T::of::<k::LineStart>()) { c.math_edge(false, false); }
        else if is(T::of::<k::LineEnd>()) { c.math_edge(true, false); }
        else if is(T::of::<k::SelectLineStart>()) { c.math_edge(false, true); }
        else if is(T::of::<k::SelectLineEnd>()) { c.math_edge(true, true); }
        else if is(T::of::<k::SelectAll>()) { c.math_select(0, len); }
        else if is(T::of::<k::Backspace>()) { c.math_delete(false, false); }
        else if is(T::of::<k::Delete>()) { c.math_delete(true, false); }
        else if is(T::of::<k::DeleteWordBack>()) { c.math_delete(false, true); }
        else if is(T::of::<k::DeleteWordForward>()) { c.math_delete(true, true); }
        else if is(T::of::<k::DeleteToLineStart>()) {
            let h = c.math.map_or(0, |m| m.head);
            c.math_replace(0..h, "");
        }
        // Enter and Esc finish: the caret after the equation.
        else if is(T::of::<k::Enter>()) || is(T::of::<k::Escape>()) { c.math_leave(true); }
        // Copy / cut the selected source as plain text (nothing selected: nothing).
        else if is(T::of::<k::Copy>()) || is(T::of::<k::Cut>()) {
            if let Some(s) = c.math_selected().filter(|s| !s.is_empty()) {
                write_text(&s, cx);
                if is(T::of::<k::Cut>()) { c.math_type(""); }
            }
        }
        else if is(T::of::<k::Paste>()) || is(T::of::<k::PastePlain>()) {
            if let Some(s) = read_text(cx) { c.math_type(&s); }
        }
        else {
            c.math_leave(false);
            self.math_marked = None;
            return false;
        }
        self.math_marked = None;
        self.marked = None;
        self.changed(window, cx);
        true
    }

    /// A press during a visit or, with the setting on, on an equation: the
    /// caret into its source (true). A press elsewhere ends the visit (false).
    pub(super) fn math_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return false;
        }
        if let Some(m) = self.core.math_valid() {
            let hit = self.cache.borrow().get(&m.at.block).and_then(|l| l.source.as_ref().filter(|b| b.ix == m.at.offset && b.view.contains(&ev.position)).map(|b| b.index_for_x(ev.position.x)));
            if let Some(i) = hit {
                let src = self.core.math_source().unwrap_or_default();
                let (a, h) = match ev.click_count {
                    1 if ev.modifiers.shift => (m.anchor, i),
                    1 => (i, i),
                    2 => { let w = core::word_at(&src, i); (w.start, w.end) }
                    _ => (0, src.len()),
                };
                self.core.math_select(a, h);
                self.math_marked = None;
                self.math_drag = ev.click_count == 1;
                self.dragging = self.math_drag;
                window.prevent_default();
                self.restart_blink(window, cx);
                cx.notify();
                return true;
            }
            self.core.math_leave(false);
            self.math_marked = None;
        }
        if !enabled() || ev.modifiers.shift {
            return false;
        }
        let Some((pos, _)) = self.pos_at(ev.position) else { return false };
        let Some(ix) = self.cache.borrow().get(&pos.block).and_then(|l| l.equation_at(ev.position)) else { return false };
        window.prevent_default();
        self.math_open(Pos::new(pos.block, ix), usize::MAX, window, cx)
    }

    /// Dragging from a press in the source selects in it.
    pub(super) fn math_drag_to(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(m) = self.core.math_valid() else { return };
        let i = self.cache.borrow().get(&m.at.block).and_then(|l| l.source.as_ref().map(|b| b.index_for_x(p.x)));
        if let Some(i) = i && i != m.head {
            self.core.math_select(m.anchor, i);
            cx.notify();
        }
    }

    // ---- IME: during a visit the platform edits the source, not the block

    pub(super) fn math_ime_source(&mut self) -> Option<String> {
        self.core.math?;
        self.core.math_source()
    }

    pub(super) fn math_ime_selected(&mut self) -> Option<(Range<usize>, bool)> {
        let src = self.math_ime_source()?;
        let m = self.core.math?;
        let r = m.range();
        Some((to16(&src, r.start)..to16(&src, r.end), m.head < m.anchor))
    }

    pub(super) fn math_ime_marked(&self) -> Option<Range<usize>> {
        let m = self.core.math?;
        let src = self.core.equation_source(m.at)?;
        let r = self.math_marked.clone()?;
        Some(to16(&src, r.start)..to16(&src, r.end))
    }

    /// `replace_text_in_range` / `replace_and_mark_text_in_range` in the
    /// source: the marked text, else `range16`, else the selection.
    pub(super) fn math_ime_replace(&mut self, range16: Option<Range<usize>>, text: &str, mark: bool, new_sel: Option<Range<usize>>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(src) = self.math_ime_source() else { return false };
        let m = self.core.math.unwrap();
        let r = match (self.math_marked.take(), range16) {
            (Some(r), _) => r,
            (None, Some(r16)) => from16(&src, r16.start)..from16(&src, r16.end),
            _ => m.range(),
        };
        self.core.math_replace(r.clone(), text);
        if mark && !text.is_empty() {
            self.math_marked = Some(r.start..r.start + text.len());
            if let (Some(ns), Some(now)) = (new_sel, self.core.math_source()) {
                let base = to16(&now, r.start);
                self.core.math_select(from16(&now, base + ns.start), from16(&now, base + ns.end));
            }
        }
        self.marked = None;
        self.pointer_hidden = true;
        self.changed(window, cx);
        true
    }

    pub(super) fn math_ime_bounds(&mut self, range16: Range<usize>) -> Option<Bounds<Pixels>> {
        let src = self.math_ime_source()?;
        let m = self.core.math?;
        let cache = self.cache.borrow();
        let b = cache.get(&m.at.block)?.source.as_ref()?;
        let (a, z) = (b.caret_bounds(from16(&src, range16.start)), b.caret_bounds(from16(&src, range16.end)));
        Some(Bounds::from_corners(a.origin, point(z.left().max(a.left()), a.bottom())))
    }

    pub(super) fn math_ime_index(&mut self, p: Point<Pixels>) -> Option<usize> {
        let src = self.math_ime_source()?;
        let m = self.core.math?;
        let cache = self.cache.borrow();
        let b = cache.get(&m.at.block)?.source.as_ref()?;
        Some(to16(&src, b.index_for_x(p.x)))
    }

    // ---- preview

    /// The rendering under the source while it is edited; an invalid source
    /// shows 无效公式 instead, the document keeps it as it is.
    pub(super) fn math_preview(&self, window: &mut Window) -> Option<AnyElement> {
        let m = self.core.math.filter(|m| !self.read_only && self.core.sel == core::Selection::caret(m.at))?;
        let src = self.core.equation_source(m.at)?;
        let b = self.cache.borrow().get(&m.at.block)?.source.as_ref().filter(|b| b.ix == m.at.offset)?.view;
        let color = Theme::text();
        let body = if src.trim().is_empty() {
            note(crate::settings::text("添加 LaTeX 公式"))
        } else if let Some(math) = super::math::layout(&src, super::math::is_block(&src), super::math::FONT_PX, color, 0, window) {
            // A display equation's number follows it, 16px on.
            let tag_w = math.tag.as_ref().map_or(0., |t| 16. + t.width);
            let (w, h) = (math.width + tag_w, math.ascent + math.depth);
            let asc = math.ascent;
            div().w(px(w)).h(px(h)).child(canvas(|_, _, _| {}, move |bounds, _, window, cx| {
                let base = bounds.top() + px(asc);
                math.paint(point(bounds.left(), base), window, cx);
                if let Some(t) = &math.tag { t.paint(point(bounds.left() + px(math.width + 16.), base), window, cx); }
            }).size_full()).into_any_element()
        } else {
            note(crate::settings::text("无效公式"))
        };
        let el = crate::ui::popover()
            .id("math-preview")
            .occlude()
            .max_w(px(560.))
            .overflow_hidden()
            .px(px(12.))
            .py(px(8.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(body);
        Some(super::overlays::at(self.rel(point(b.left(), b.bottom() + px(6.))), Anchor::TopLeft, el))
    }
}

fn note(s: &str) -> AnyElement {
    div().text_size(px(13.)).line_height(px(20.)).text_color(Theme::text_caption()).child(s.to_string()).into_any_element()
}

fn write_text(s: &str, cx: &mut App) {
    // Scripts read what a copy put there instead of the system clipboard.
    if let Some(f) = crate::clipboard::FAKE_CLIPBOARD.lock().unwrap().as_mut() {
        f.text = s.to_string();
        f.html = None;
        return;
    }
    cx.write_to_clipboard(ClipboardItem::new_string(s.to_string()));
}

fn read_text(cx: &mut App) -> Option<String> {
    if let Some(f) = crate::clipboard::FAKE_CLIPBOARD.lock().unwrap().as_ref() {
        return Some(f.text.clone());
    }
    cx.read_from_clipboard()?.text()
}
