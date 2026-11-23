//! 公式原位编辑 (experimental, 设置 › 通用): the caret goes into an inline
//! equation and edits its LaTeX source where the rendering was, instead of in
//! the popover. The document keeps the equation object the whole time: only
//! `Core::math` (the equation and a selection in its source) is extra, and
//! every change to the source is `set_equation_source`, so saving, syncing
//! and undo see an ordinary equation edit (one undo step per visit).
//!
//! While a visit lasts the document caret sits right before the equation.
//! Anything that moves it elsewhere (or removes the equation) ends the visit:
//! `math_valid` checks that before every use.

use super::core::{Core, EQUATION, Pos, Selection, next_grapheme, next_word, prev_grapheme, prev_word};
use std::ops::Range;

/// A visit: the equation's U+FFFC at `at`, and a selection in its source
/// (byte offsets, `anchor == head` for a caret).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathEdit {
    pub at: Pos,
    pub anchor: usize,
    pub head: usize,
}

impl MathEdit {
    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
}

fn floor_boundary(s: &str, mut o: usize) -> usize {
    o = o.min(s.len());
    while !s.is_char_boundary(o) {
        o -= 1;
    }
    o
}

impl Core {
    /// The visit, if it still applies: the equation is still at `at` and the
    /// caret still before it. A stale one is dropped.
    pub fn math_valid(&mut self) -> Option<MathEdit> {
        let m = self.math?;
        let ok = self.sel == Selection::caret(m.at) && self.block_sel.is_none() && self.cell_sel.is_none();
        let Some(src) = self.equation_source(m.at).filter(|_| ok) else {
            self.math = None;
            return None;
        };
        let m = MathEdit { at: m.at, anchor: floor_boundary(&src, m.anchor), head: floor_boundary(&src, m.head) };
        self.math = Some(m);
        Some(m)
    }

    /// The source being edited.
    pub fn math_source(&mut self) -> Option<String> {
        let m = self.math_valid()?;
        self.equation_source(m.at)
    }

    /// Start a visit to the equation at `at` with the caret at `offset` of its
    /// source (clamped). False when there is no equation there.
    pub fn math_enter(&mut self, at: Pos, offset: usize) -> bool {
        let Some(src) = self.equation_source(at) else { return false };
        if !self.text_of(at.block).is_some_and(|t| t.text.get(at.offset..at.offset + EQUATION.len()) == Some(EQUATION)) {
            return false;
        }
        let o = floor_boundary(&src, offset);
        self.block_sel = None;
        self.cell_sel = None;
        self.pending_marks = None;
        self.sel = Selection::caret(at);
        self.break_coalescing();
        self.math = Some(MathEdit { at, anchor: o, head: o });
        true
    }

    /// A collapsed caret next to an equation moving onto it (→ before it,
    /// ← after it) enters its source at the near end instead.
    pub fn math_enter_from_side(&mut self, right: bool) -> bool {
        if !self.sel.is_collapsed() || self.block_sel.is_some() || self.cell_sel.is_some() {
            return false;
        }
        let p = self.sel.head;
        let Some(t) = self.text_of(p.block) else { return false };
        if right {
            return self.math_enter(p, 0);
        }
        if p.offset < EQUATION.len() || !t.text.is_char_boundary(p.offset - EQUATION.len()) {
            return false;
        }
        let at = Pos::new(p.block, p.offset - EQUATION.len());
        self.math_enter(at, usize::MAX)
    }

    /// End the visit with the caret before (or after) the equation.
    pub fn math_leave(&mut self, after: bool) {
        let Some(m) = self.math_valid() else { return };
        self.math = None;
        self.break_coalescing();
        self.sel = Selection::caret(if after { Pos::new(m.at.block, m.at.offset + EQUATION.len()) } else { m.at });
    }

    /// Replace `range` of the source with `s`; the caret goes after it.
    pub fn math_replace(&mut self, range: Range<usize>, s: &str) -> bool {
        let Some(src) = self.math_source() else { return false };
        let m = self.math.unwrap();
        let r = floor_boundary(&src, range.start.min(range.end))..floor_boundary(&src, range.end.max(range.start));
        let mut new = src.clone();
        new.replace_range(r.clone(), s);
        // A blank source reads back empty from the file (`<latex> </latex>`):
        // keep it empty here too.
        if new.trim().is_empty() {
            new.clear();
        }
        if new != src {
            self.set_equation_source(m.at, &new);
        }
        let o = (r.start + s.len()).min(new.len());
        self.math = Some(MathEdit { anchor: o, head: o, ..m });
        true
    }

    /// Typing: replaces the selection in the source.
    pub fn math_type(&mut self, s: &str) -> bool {
        let Some(m) = self.math_valid() else { return false };
        self.math_replace(m.range(), s)
    }

    /// Backspace / Delete (by word with `word`): the selection, else the
    /// character (word) before / after. At the edge of an empty source the
    /// equation itself goes; at the edge of any other nothing happens.
    pub fn math_delete(&mut self, forward: bool, word: bool) -> bool {
        let Some(src) = self.math_source() else { return false };
        let m = self.math.unwrap();
        let r = m.range();
        if !r.is_empty() {
            return self.math_replace(r, "");
        }
        if src.is_empty() {
            self.math = None;
            self.sel = Selection { anchor: m.at, head: Pos::new(m.at.block, m.at.offset + EQUATION.len()) };
            self.delete_selection();
            self.break_coalescing();
            return true;
        }
        let h = m.head;
        let r = match (forward, word) {
            (false, false) => prev_grapheme(&src, h)..h,
            (false, true) => prev_word(&src, h)..h,
            (true, false) => h..next_grapheme(&src, h),
            (true, true) => h..next_word(&src, h),
        };
        if r.is_empty() {
            return true;
        }
        self.math_replace(r, "")
    }

    /// ← / → (by word with `word`, ⇧ with `extend`). Past either end of the
    /// source without ⇧ the visit ends on that side; true while it lasts.
    pub fn math_move(&mut self, right: bool, word: bool, extend: bool) -> bool {
        let Some(src) = self.math_source() else { return false };
        let mut m = self.math.unwrap();
        let r = m.range();
        if !extend && !r.is_empty() {
            let o = if right { r.end } else { r.start };
            self.math = Some(MathEdit { anchor: o, head: o, ..m });
            return true;
        }
        let h = m.head;
        if (right && h >= src.len()) || (!right && h == 0) {
            if !extend {
                self.math_leave(right);
                return false;
            }
            return true;
        }
        m.head = match (right, word) {
            (true, false) => next_grapheme(&src, h),
            (true, true) => next_word(&src, h),
            (false, false) => prev_grapheme(&src, h),
            (false, true) => prev_word(&src, h),
        };
        if !extend {
            m.anchor = m.head;
        }
        self.math = Some(m);
        true
    }

    /// ⌘← / ⌘→ (and Home / End): to the start or end of the source.
    pub fn math_edge(&mut self, end: bool, extend: bool) -> bool {
        let Some(src) = self.math_source() else { return false };
        let mut m = self.math.unwrap();
        m.head = if end { src.len() } else { 0 };
        if !extend {
            m.anchor = m.head;
        }
        self.math = Some(m);
        true
    }

    /// Set the selection in the source (a click, a drag, ⌘A).
    pub fn math_select(&mut self, anchor: usize, head: usize) -> bool {
        let Some(src) = self.math_source() else { return false };
        let m = self.math.unwrap();
        self.math = Some(MathEdit { anchor: floor_boundary(&src, anchor), head: floor_boundary(&src, head), ..m });
        true
    }

    /// The selected part of the source (what ⌘C copies during a visit).
    pub fn math_selected(&mut self) -> Option<String> {
        let m = self.math_valid()?;
        let src = self.equation_source(m.at)?;
        Some(src[m.range()].to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docxml::{parse, serialize};

    fn doc(xml: &str) -> Core {
        Core::new(parse(xml).unwrap())
    }

    fn src(c: &mut Core) -> String {
        c.math_source().unwrap()
    }

    /// → from before the equation goes into its source, through it one
    /// character at a time, and out after it; ← the same way back.
    #[test]
    fn arrows_walk_through_the_source() {
        let mut c = doc("<title>T</title><p>a <latex>x^2</latex> b</p>");
        let b = c.doc.blocks[0].id;
        c.sel = Selection::caret(Pos::new(b, 2));
        assert!(c.math_enter_from_side(true));
        let m = c.math.unwrap();
        assert_eq!((m.at, m.head), (Pos::new(b, 2), 0));
        let mut heads = vec![];
        while c.math_move(true, false, false) {
            heads.push(c.math.unwrap().head);
        }
        assert_eq!(heads, [1, 2, 3]);
        assert!(c.math.is_none());
        assert_eq!(c.sel, Selection::caret(Pos::new(b, 2 + EQUATION.len())));
        // Back in from the right, at the end of the source.
        assert!(c.math_enter_from_side(false));
        assert_eq!(c.math.unwrap().head, 3);
        while c.math_move(false, false, false) {}
        assert_eq!(c.sel, Selection::caret(Pos::new(b, 2)));
        // Not next to an equation: nothing to enter.
        c.sel = Selection::caret(Pos::new(b, 1));
        assert!(!c.math_enter_from_side(true));
        assert!(!c.math_enter_from_side(false));
        assert!(c.math.is_none());
    }

    /// Typing, deleting and moving by grapheme keep char boundaries in a
    /// source with CJK and emoji; the document keeps one equation object.
    #[test]
    fn editing_keeps_the_object_and_boundaries() {
        let mut c = doc("<title>T</title><p>前 <latex>x</latex> 后</p>");
        let b = c.doc.blocks[0].id;
        let at = Pos::new(b, "前 ".len());
        assert!(c.math_enter(at, 1));
        for s in ["_", "t", "+", "中文", "👍🏽"] {
            assert!(c.math_type(s));
            assert_eq!(c.text_of(b).unwrap().text, format!("前 {EQUATION} 后"));
        }
        assert_eq!(src(&mut c), "x_t+中文👍🏽");
        assert!(c.math_delete(false, false));
        assert_eq!(src(&mut c), "x_t+中文");
        assert!(c.math_move(false, false, false));
        assert_eq!(c.math.unwrap().head, "x_t+中".len());
        assert!(c.math_move(false, false, true));
        assert_eq!(c.math_selected().as_deref(), Some("中"));
        assert!(c.math_type("y"));
        assert_eq!(src(&mut c), "x_t+y文");
        assert!(c.math_delete(true, false));
        assert_eq!(src(&mut c), "x_t+y");
        assert!(c.math_delete(true, false), "at the end: nothing to delete, still visiting");
        assert_eq!(src(&mut c), "x_t+y");
        // A word back, then a selection collapsing to its side.
        assert!(c.math_delete(false, true));
        assert_eq!(src(&mut c), "x_t+");
        assert!(c.math_select(0, 2));
        assert!(c.math_move(true, false, false));
        assert_eq!(c.math.unwrap().range(), 2..2);
        assert!(c.math_edge(true, true));
        assert_eq!(c.math_selected().as_deref(), Some("t+"));
        // A blank source stays empty, as it would read back from the file.
        assert!(c.math_select(0, 100));
        c.math_type(" \n");
        assert_eq!(src(&mut c), "");
        assert_eq!(c.math.unwrap().head, 0);
        c.math_type("x_t+");
        // Offsets inside a character are clamped down.
        assert!(c.math_select(0, 100));
        assert_eq!(c.math.unwrap().head, 4);
    }

    /// One visit is one undo step, and the saved file has the equation.
    #[test]
    fn a_visit_is_one_undo_step() {
        let mut c = doc("<title>T</title><p>a<latex>x</latex>b</p>");
        let b = c.doc.blocks[0].id;
        let before = serialize(&c.doc);
        assert!(c.math_enter(Pos::new(b, 1), 1));
        for s in ["^", "2", "+", "1"] {
            c.math_type(s);
        }
        c.math_leave(true);
        let after = serialize(&c.doc);
        assert!(after.contains("<latex>x^2+1</latex>"), "{after}");
        assert_eq!(crate::markdown::to_markdown(&c.doc).trim_end(), "# T\n\na$x^2+1$b");
        assert_eq!(serialize(&parse(&after).unwrap()), after);
        // A second visit is a step of its own.
        assert!(c.math_enter(Pos::new(b, 1), 0));
        c.math_type("y");
        c.math_leave(false);
        assert!(c.undo());
        assert_eq!(serialize(&c.doc), after);
        assert!(c.undo());
        assert_eq!(serialize(&c.doc), before);
        assert!(c.redo());
        assert_eq!(serialize(&c.doc), after);
    }

    /// Backspace in an empty source removes the equation; at the start of a
    /// non-empty one it does nothing.
    #[test]
    fn backspace_at_the_edges() {
        let mut c = doc("<title>T</title><p>a<latex>x</latex>b</p>");
        let b = c.doc.blocks[0].id;
        assert!(c.math_enter(Pos::new(b, 1), 0));
        assert!(c.math_delete(false, false));
        assert_eq!(src(&mut c), "x");
        c.math_edge(true, false);
        c.math_delete(false, false);
        assert_eq!(src(&mut c), "");
        assert!(c.math_delete(false, false));
        assert!(c.math.is_none());
        assert_eq!(c.text_of(b).unwrap().text, "ab");
        assert_eq!(c.sel, Selection::caret(Pos::new(b, 1)));
        assert!(c.undo());
        assert_eq!(c.equation_source(Pos::new(b, 1)).as_deref(), Some(""));
    }

    /// The caret moving away (a click, another command) or the equation
    /// going ends the visit; nothing then edits through it.
    #[test]
    fn a_stale_visit_is_dropped() {
        let mut c = doc("<title>T</title><p>a<latex>x</latex>b</p>");
        let b = c.doc.blocks[0].id;
        assert!(c.math_enter(Pos::new(b, 1), 1));
        c.sel = Selection::caret(Pos::new(b, 0));
        assert!(!c.math_type("z"));
        assert!(c.math.is_none());
        assert_eq!(c.equation_source(Pos::new(b, 1)).as_deref(), Some("x"));
        assert!(c.math_enter(Pos::new(b, 1), 1));
        c.math_type("y");
        assert!(c.undo());
        assert!(c.math_valid().is_none());
        // Not an equation.
        assert!(!c.math_enter(Pos::new(b, 0), 0));
    }
}
