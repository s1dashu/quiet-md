//! A view-only source buffer. The document always keeps its equation object.
use super::core::{Core, Pos, Selection, EQUATION};
use crate::model::{Block, Document, Marks, RichText};

/// Coordinates belong to the layout that produced them, even after its edit closes.
#[derive(Clone, Copy, Debug)]
pub struct EquationProjection {
    pub at: usize,
    pub len: usize,
}

impl EquationProjection {
    pub fn model_offset(self, visual: usize) -> usize {
        if visual <= self.at { visual }
        else if visual < self.at + self.len { self.at }
        else { visual - self.len + EQUATION.len() }
    }

    pub fn visual_offset(self, model: usize) -> usize {
        if model <= self.at { model }
        else if model < self.at + EQUATION.len() { self.at }
        else { model - EQUATION.len() + self.len }
    }
}

pub struct EquationEdit {
    pub pos: Pos,
    pub generation: u64,
    pub input: Core,
}

impl EquationEdit {
    pub fn new(core: &Core, pos: Pos, offset: usize) -> Option<Self> {
        let src = core.equation_source(pos)?;
        let mut b = Block::text(&src);
        b.id = pos.block;
        let mut input = Core::new(Document { blocks: vec![b], ..Default::default() });
        input.sel = Selection::caret(Pos::new(pos.block, super::core::char_boundary(&src, offset)));
        Some(Self { pos, generation: core.generation, input })
    }

    pub fn text(&self) -> &str { &self.input.doc.blocks[0].text.text }
    pub fn display_len(&self) -> usize { self.text().len().max(1) }

    pub fn project(&self, text: &RichText) -> RichText {
        let mut out = text.clone();
        let mut ins = RichText::default();
        let mut marks = Marks::default();
        marks.code = true;
        ins.push(if self.text().is_empty() { " " } else { self.text() }, marks);
        out.replace(self.pos.offset..self.pos.offset + EQUATION.len(), &ins);
        out
    }

    pub fn projection(&self) -> EquationProjection { EquationProjection { at: self.pos.offset, len: self.display_len() } }

    pub fn model_offset(&self, visual: usize) -> usize { self.projection().model_offset(visual) }

    pub fn visual_offset(&self, model: usize) -> usize { self.projection().visual_offset(model) }

    pub fn sync(&self, core: &mut Core) { core.set_equation_source(self.pos, self.text()); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docxml::{parse, serialize};

    #[test]
    fn source_buffer_rejects_interior_object_offsets() {
        let c = Core::new(parse("<title>T</title><p>before <latex>x^2</latex> after</p>").unwrap());
        let b = c.doc.blocks[0].id;
        assert!(EquationEdit::new(&c, Pos::new(b, 7), 3).is_some());
        for offset in [8, 9, 10, 12] {
            assert!(EquationEdit::new(&c, Pos::new(b, offset), 0).is_none(), "accepted offset {offset}");
        }
    }

    #[test]
    fn adjacent_identical_equations_accept_only_each_object_start() {
        let c = Core::new(parse("<title>T</title><p><latex>x</latex><latex>x</latex></p>").unwrap());
        let b = c.doc.blocks[0].id;
        for offset in 0..=6 {
            assert_eq!(EquationEdit::new(&c, Pos::new(b, offset), 0).is_some(), offset == 0 || offset == 3);
        }
    }

    #[test]
    fn projection_snapshot_and_navigation_keep_unicode_boundaries() {
        for src in ["", "x", "x^2", "中文😀x", "\\frac{中文}{😀}"] {
            let c = Core::new(parse(&format!("<title>T</title><p>before <latex>{src}</latex> after</p>")).unwrap());
            let b = c.doc.blocks[0].id;
            let e = EquationEdit::new(&c, Pos::new(b, 7), usize::MAX).unwrap();
            let projection = e.projection();
            let model = &c.text_of(b).unwrap().text;
            let display = e.project(c.text_of(b).unwrap());
            for visual in 0..=display.len() {
                if display.text.is_char_boundary(visual) {
                    assert!(model.is_char_boundary(projection.model_offset(visual)));
                }
            }
            drop(e);
            assert_eq!(projection.model_offset(7 + projection.len + 2), 12);
            assert_eq!(projection.visual_offset(12), 7 + projection.len + 2);
            for offset in 0..=model.len() + 4 {
                assert!(model.is_char_boundary(super::super::core::prev_grapheme(model, offset)));
                assert!(model.is_char_boundary(super::super::core::next_grapheme(model, offset)));
            }
            let e = EquationEdit::new(&c, Pos::new(b, 7), 1).unwrap();
            assert!(src.is_char_boundary(e.input.sel.head.offset));
        }
    }

    #[test]
    fn source_buffer_preserves_object_save_reopen_and_undo() {
        let mut c = Core::new(parse("<title>T</title><p>前 <latex>x</latex> 后</p>").unwrap());
        let b = c.doc.blocks[0].id;
        let p = Pos::new(b, "前 ".len());
        let original = serialize(&c.doc);
        c.break_coalescing();
        let mut e = EquationEdit::new(&c, p, 1).unwrap();
        for s in ["_", "t", "+", "中文", "😀"] {
            e.input.insert_text(s);
            e.sync(&mut c);
            assert_eq!(c.text_of(b).unwrap().text, format!("前 {EQUATION} 后"));
        }
        let saved = serialize(&c.doc);
        assert!(saved.contains("<latex>x_t+中文😀</latex>"));
        assert_eq!(serialize(&parse(&saved).unwrap()), saved);
        c.undo();
        assert_eq!(serialize(&c.doc), original);
        c.redo();
        assert_eq!(serialize(&c.doc), saved);
    }

    #[test]
    fn projection_maps_suffix_and_empty_source_without_mutating_document() {
        for src in ["", "x", "\\frac{中文}{😀}"] {
            let c = Core::new(parse(&format!("<title>T</title><p>a<latex>{src}</latex>z</p>")).unwrap());
            let b = c.doc.blocks[0].id;
            let e = EquationEdit::new(&c, Pos::new(b, 1), 0).unwrap();
            let t = c.text_of(b).unwrap();
            let original = t.clone();
            let projected = e.project(t);
            assert_eq!(projected.text, format!("a{}z", if src.is_empty() { " " } else { src }));
            assert_eq!(e.model_offset(1 + e.display_len()), 4);
            assert_eq!(e.visual_offset(4), 1 + e.display_len());
            assert_eq!(c.text_of(b).unwrap(), &original);
        }
    }

    #[test]
    fn composing_replaces_marked_source_and_keeps_unicode_selection() {
        let mut c = Core::new(parse("<title>T</title><p><latex>x</latex></p>").unwrap());
        let p = Pos::new(c.doc.blocks[0].id, 0);
        let mut e = EquationEdit::new(&c, p, 1).unwrap();
        for text in ["ni", "你", "中文😀"] {
            let end = e.text().len();
            e.input.replace_in_block(p.block, 1..end, text);
            e.sync(&mut c);
        }
        assert_eq!(e.text(), "x中文😀");
        e.input.sel = Selection { anchor: Pos::new(p.block, 1), head: Pos::new(p.block, e.text().len()) };
        e.input.backspace();
        e.sync(&mut c);
        assert_eq!(c.equation_source(p).as_deref(), Some("x"));
    }
}
