//! Experimental source editing through the editor's existing input focus.
use super::core::{self, Pos, Selection};
use super::view::{Editor, from16, to16};
use fdl_core::editor::equation_edit::EquationEdit;
use gpui::*;
use std::ops::Range;

pub(super) fn enabled() -> bool {
    cfg!(feature = "experimental-inline-math") && std::env::var_os("QUIET_MD_INLINE_MATH").is_some_and(|v| v == "1")
}

impl Editor {
    #[cfg(feature = "devtools")]
    pub(crate) fn check_inline_math(&self, expected: &str, window: &Window) {
        for p in [self.core.sel.anchor, self.core.sel.head] {
            if let Some(t) = self.core.text_of(p.block) { assert!(p.offset <= t.len() && t.text.is_char_boundary(p.offset), "invalid model caret {p:?}"); }
        }
        let expected: serde_json::Value = serde_json::from_str(expected).expect("math-check JSON");
        let e = self.inline_math.as_ref();
        let state = serde_json::json!({"active": e.is_some(), "model_head": self.core.sel.head.offset, "model_anchor": self.core.sel.anchor.offset, "read_only": self.read_only, "origin": e.map(|e| e.pos.offset), "source": e.map(|e| e.text()),
            "focus": self.focus.is_focused(window), "marked": self.marked.is_some(),
            "anchor": e.map(|e| e.input.sel.anchor.offset), "head": e.map(|e| e.input.sel.head.offset),
            "renders": super::math::SOURCE_RENDERS.load(std::sync::atomic::Ordering::Relaxed)});
        for (k, v) in expected.as_object().expect("math-check object") {
            if k == "renderable" {
                for u in self.core.units() {
                    if let Some(t) = self.core.text_of(u.id) {
                        for (_, marks) in t.spans() {
                            if let Some(src) = marks.inline.as_ref().and_then(|o| o.equation()).filter(|s| !s.trim().is_empty()) {
                                assert_eq!(super::math::layout(src, super::math::is_block(src), super::math::FONT_PX, gpui::black(), 0, window).is_some(), v.as_bool().unwrap(), "render source {src:?}");
                            }
                        }
                    }
                }
            } else if k == "reset_renders" {
                super::math::SOURCE_RENDERS.store(0, std::sync::atomic::Ordering::Relaxed);
            } else if k == "contains" {
                assert!(crate::docxml::serialize(&self.core.doc).contains(v.as_str().unwrap()), "math-check missing {v}");
            } else { assert_eq!(&state[k], v, "math-check {k}: {state}"); }
        }
        eprintln!("math-check PASS {state}");
    }

    pub(super) fn inline_open(&mut self, p: Pos, offset: usize) {
        if self.read_only { return; }
        let Some(edit) = EquationEdit::new(&self.core, p, offset) else { return };
        self.inline_math = Some(edit);
        self.core.clear_block_sel();
        self.core.break_coalescing();
        self.core.sel = Selection::caret(p);
        self.menu = None;
        self.slash = None;
        self.marked = None;
        self.toolbar_hidden = true;
        self.dragging = false;
    }

    pub(super) fn inline_close(&mut self, after: bool) {
        if let Some(e) = self.inline_math.take() {
            self.core.sel = Selection::caret(Pos::new(e.pos.block, e.pos.offset + if after { core::EQUATION.len() } else { 0 }));
            self.core.break_coalescing();
            self.marked = None;
            self.items_key = None;
            self.frame.generation = None;
            self.cat_generation = None;
            self.inline_refresh = true;
        }
    }

    pub(super) fn inline_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inline_math.is_none() { return self.changed(window, cx); }
        let generation = self.core.generation;
        if let Some(e) = &self.inline_math { e.sync(&mut self.core); }
        if self.frame.generation == Some(generation) { self.frame.generation = Some(self.core.generation); }
        if self.cat_generation == Some(generation) { self.cat_generation = Some(self.core.generation); }
        if self.core.dirty {
            self.core.dirty = false;
            cx.emit(super::view::EditorEvent::Changed);
        }
        self.typing = true;
        self.autoscroll = true;
        self.restart_blink(window, cx);
        cx.notify();
    }

    pub(super) fn inline_replace(&mut self, range: Option<Range<usize>>, text: &str, new_sel: Option<Range<usize>>, mark: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = &mut self.inline_math else { return };
        let src = e.text().to_string();
        let text = text.replace('\r', "\n");
        let sel = e.input.sel;
        let r = self.marked.take().map(|(_, r)| r).or_else(|| range.map(|r| from16(&src, r.start)..from16(&src, r.end)))
            .unwrap_or(sel.anchor.offset.min(sel.head.offset)..sel.anchor.offset.max(sel.head.offset));
        e.input.replace_in_block(e.pos.block, r.clone(), &text);
        if mark && !text.is_empty() { self.marked = Some((e.pos.block, r.start..r.start + text.len())); }
        if let Some(ns) = new_sel {
            let base = to16(e.text(), r.start);
            e.input.sel = Selection { anchor: Pos::new(e.pos.block, from16(e.text(), base + ns.start)), head: Pos::new(e.pos.block, from16(e.text(), base + ns.end)) };
        }
        self.inline_sync(window, cx);
    }

    pub(super) fn inline_pointer(&mut self, p: Point<Pixels>, extend: bool, cx: &mut Context<Self>) -> bool {
        let Some(e) = &mut self.inline_math else { return false };
        let cache = self.cache.borrow();
        let Some(l) = cache.get(&e.pos.block) else { return false };
        let Some(projection) = l.projection.filter(|p| p.at == e.pos.offset) else { return false };
        let ix = l.index_for(p);
        if !l.bounds.contains(&p) || ix < projection.at || ix > projection.at + projection.len { return false; }
        let pos = Pos::new(e.pos.block, core::char_boundary(e.text(), ix - projection.at));
        if extend { e.input.sel.head = pos; } else { e.input.sel = Selection::caret(pos); }
        self.marked = None;
        self.dragging = true;
        drop(cache);
        cx.notify();
        true
    }

    pub(super) fn inline_action(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.inline_math.is_none() || !self.focus.is_focused(window) { return false; }
        let name = name.rsplit("::").next().unwrap_or(name);
        let e = self.inline_math.as_mut().unwrap();
        let text = e.text().to_string();
        let s = e.input.sel;
        let h = s.head.offset;
        let extend = name.starts_with("Select") && name != "SelectAll";
        let right = matches!(name, "MoveRight" | "SelectRight" | "WordRight" | "SelectWordRight" | "LineEnd" | "SelectLineEnd" | "DocEnd");
        match name {
            "MoveLeft" | "MoveRight" | "SelectLeft" | "SelectRight" | "WordLeft" | "WordRight" | "SelectWordLeft" | "SelectWordRight" | "LineStart" | "LineEnd" | "SelectLineStart" | "SelectLineEnd" | "DocStart" | "DocEnd" => {
                if (s.is_collapsed() || extend) && ((!right && h == 0) || (right && h == text.len())) && matches!(name, "MoveLeft" | "MoveRight" | "SelectLeft" | "SelectRight") {
                    let pos = e.pos;
                    let selected = !s.is_collapsed();
                    self.inline_close(right);
                    if extend {
                        if selected { self.core.sel.anchor = Pos::new(pos.block, pos.offset + if right { 0 } else { core::EQUATION.len() }); }
                        return false;
                    }
                } else {
                    let o = if !extend && !s.is_collapsed() { if right { s.anchor.offset.max(h) } else { s.anchor.offset.min(h) } }
                        else if name.contains("Start") { 0 } else if name.contains("End") { text.len() }
                        else if name.contains("Word") { if right { core::next_word(&text, h) } else { core::prev_word(&text, h) } }
                        else if right { core::next_grapheme(&text, h) } else { core::prev_grapheme(&text, h) };
                    let p = Pos::new(e.pos.block, o);
                    if extend { e.input.sel.head = p; } else { e.input.sel = Selection::caret(p); }
                }
            }
            "MoveUp" | "MoveDown" | "SelectUp" | "SelectDown" => {
                let down = name.ends_with("Down");
                let target = self.cache.borrow().get(&e.pos.block).and_then(|l| {
                    let p = l.position_for(e.pos.offset + h);
                    let row = l.row_index_for(e.pos.offset + h);
                    let row = if down { (row + 1 < l.rows.len()).then_some(row + 1) } else { row.checked_sub(1) }?;
                    Some(l.index_in_row(row, p.x))
                });
                if let Some(o) = target.filter(|o| *o >= e.pos.offset && *o <= e.pos.offset + text.len()) {
                    let p = Pos::new(e.pos.block, core::char_boundary(&text, o - e.pos.offset));
                    if extend { e.input.sel.head = p; } else { e.input.sel = Selection::caret(p); }
                } else { self.inline_close(down); return false; }
            }
            "Backspace" | "DeleteWordBack" if s.is_collapsed() && h == 0 => self.inline_close(false),
            "Delete" if s.is_collapsed() && h == text.len() => self.inline_close(true),
            "Backspace" => e.input.backspace(),
            "Delete" => e.input.delete_forward(),
            "DeleteWordBack" => e.input.delete_word_back(),
            "DeleteWordForward" => { if s.is_collapsed() { e.input.sel.head.offset = core::next_word(&text, h); } e.input.delete_selection(); }
            "DeleteToLineStart" => e.input.delete_to_start(),
            "SelectAll" => e.input.sel = Selection { anchor: Pos::new(e.pos.block, 0), head: Pos::new(e.pos.block, text.len()) },
            "Copy" | "Cut" => {
                if !s.is_collapsed() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text[s.anchor.offset.min(h)..s.anchor.offset.max(h)].to_string()));
                    if name == "Cut" { e.input.delete_selection(); }
                }
            }
            "Paste" | "PastePlain" => {
                let fake = crate::clipboard::FAKE_CLIPBOARD.lock().unwrap().clone();
                let text = fake.map(|(_, t)| t).or_else(|| cx.read_from_clipboard().and_then(|i| i.text()));
                if let Some(text) = text { self.inline_replace(None, &text, None, false, window, cx); }
                return true;
            }
            "SoftBreak" | "Tab" => { self.inline_replace(None, if name == "Tab" { "\t" } else { "\n" }, None, false, window, cx); return true; }
            "Enter" | "Escape" => self.inline_close(true),
            "Save" => return false,
            _ => { self.inline_close(true); return false; }
        }
        self.marked = None;
        self.inline_sync(window, cx);
        true
    }
}
