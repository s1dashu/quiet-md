use fdl_core::{docxml, markdown, model::*};
use fdl_core::editor::core::{BlockSel, Core, Pos, Selection, TITLE};
use fdl_core::editor::table_ops::{CellSel, CellSelKind};

fn core(body: &str) -> Core { Core::new(docxml::parse(&format!("<title>Title</title>{body}")).unwrap()) }
fn select(c: &mut Core, start: usize, end: usize) {
    let id = c.doc.blocks[0].id;
    c.sel = Selection { anchor: Pos::new(id, start), head: Pos::new(id, end) };
}

#[test]
fn ordinary_partial_and_reversed_selection_keep_whitespace() {
    let mut c = core("<p>one  中文😀 two </p>");
    let end = c.doc.blocks[0].text.len();
    for (a, b) in [(3, end), (end, 3)] {
        select(&mut c, a, b);
        assert_eq!(c.clipboard_content().unwrap().0, "  中文😀 two ");
    }
    select(&mut c, 3, 3);
    assert!(c.clipboard_content().is_none());
}

#[test]
fn rendered_formula_copy_uses_latex_not_object_placeholder() {
    let mut c = core(r"<p>前 <latex>\frac{x_t}{中文😀}</latex> 后</p>");
    let start = "前 ".len();
    let end = start + '\u{fffc}'.len_utf8();
    select(&mut c, start, end);
    // This was the old clipboard payload: an invisible object, not its source.
    assert_eq!(c.selected_text(), "\u{fffc}");
    let (text, xml) = c.clipboard_content().unwrap();
    assert_eq!(text, r"$\frac{x_t}{中文😀}$");
    let fragment = docxml::parse(&xml).unwrap();
    assert_eq!(fragment.blocks[0].text.plain_text(), r"\frac{x_t}{中文😀}");
    assert_eq!(fragment.blocks[0].text.text, "\u{fffc}");
    select(&mut c, end, start);
    assert_eq!(c.clipboard_content().unwrap().0, text);
    let len = c.doc.blocks[0].text.len();
    select(&mut c, 0, len);
    assert_eq!(c.clipboard_content().unwrap().0, r"前 $\frac{x_t}{中文😀}$ 后");
}

#[test]
fn mixed_blocks_keep_markdown_and_internal_paste_objects() {
    let mut c = core(r"<h2>Heading</h2><p><b>bold</b> <latex>x_t</latex></p><ul><li>item <latex>\alpha</latex></li></ul>");
    let first = c.doc.blocks[0].id;
    let last = c.doc.blocks.last().unwrap().id;
    c.block_sel = Some(BlockSel { anchor: first, head: last });
    let (text, xml) = c.clipboard_content().unwrap();
    assert_eq!(text, "## Heading\n\n**bold** $x_t$\n\n- item $\\alpha$");
    let fragment = docxml::parse(&xml).unwrap();
    let mut target = core("<p></p>");
    let id = target.doc.blocks[0].id;
    target.sel = Selection::caret(Pos::new(id, 0));
    target.paste_blocks(fragment.blocks);
    let pasted = docxml::serialize(&target.doc);
    assert!(pasted.contains("<latex>x_t</latex>"));
    assert!(pasted.contains("<latex>\\alpha</latex>"));
}

#[test]
fn cross_paragraph_copy_includes_selected_title() {
    let mut c = core("<p>before <latex>x^2</latex> after</p><p>end</p>");
    c.sel = Selection { anchor: Pos::new(TITLE, 2), head: Pos::new(c.doc.blocks[1].id, 2) };
    assert_eq!(c.clipboard_content().unwrap().0, "tle\n\nbefore $x^2$ after\n\nen");
}

#[test]
fn table_cell_copy_keeps_equations_and_structure() {
    let equation = core(r"<p><latex>\frac{a_1}{b}</latex></p>").doc.blocks.remove(0);
    let t = Table { col_widths: vec![Table::DEFAULT_WIDTH; 2], header_rows: 1, header_col: false, rows: vec![vec![
        TableCell { blocks: vec![equation], ..TableCell::empty() },
        TableCell { blocks: vec![Block::text("label")], ..TableCell::empty() },
    ]] };
    let table = Block::new(BlockKind::Table(t), RichText::default());
    let id = table.id;
    let mut c = Core::new(Document { blocks: vec![table], ..Default::default() });
    c.select_cells(CellSel { table: id, anchor: (0, 0), head: (0, 1), kind: CellSelKind::Range });
    let (text, xml) = c.clipboard_content().unwrap();
    assert_eq!(text, "| $\\frac{a_1}{b}$ | label |\n| --- | --- |");
    assert!(xml.contains("<latex>\\frac{a_1}{b}</latex>"));
}

#[test]
fn latex_is_not_escaped_but_ordinary_markdown_characters_are() {
    let c = core("<p>text_x <latex>\\frac{x_t}{y} &lt; 2</latex> text_y</p>");
    assert_eq!(markdown::clipboard_text(&c.doc.blocks), r"text\_x $\frac{x_t}{y} < 2$ text\_y");
    let exported = markdown::export(&c.doc, &markdown::Options::default()).text;
    assert!(exported.contains(r"$\frac{x_t}{y} < 2$"));
}

#[test]
fn multiline_and_empty_formula_sources_are_not_dropped() {
    let c = core("<p><latex>a_1\\\\\nb_2</latex></p><p><latex/></p>");
    assert_eq!(markdown::clipboard_text(&c.doc.blocks), "$$\na_1\\\\\nb_2\n$$\n\n$$");
}

#[test]
fn source_editor_partial_copy_stays_literal() {
    let c = core(r"<p><latex>\frac{x_t}{y}</latex></p>");
    let p = Pos::new(c.doc.blocks[0].id, 0);
    let mut edit = fdl_core::editor::equation_edit::EquationEdit::new(&c, p, 0).unwrap();
    edit.input.sel = Selection { anchor: Pos::new(p.block, 6), head: Pos::new(p.block, 9) };
    assert_eq!(edit.input.selected_text(), "x_t");
}

#[test]
fn adjacent_formulas_and_multiline_prose_have_clear_boundaries() {
    let c = core("<p><latex>x</latex><latex>x</latex><latex>y</latex></p>");
    assert_eq!(markdown::clipboard_text(&c.doc.blocks), "$x$ $x$ $y$");
    let c = core("<p>before <latex>a\nb</latex> after</p>");
    assert_eq!(markdown::clipboard_text(&c.doc.blocks), "before \n\n$$\na\nb\n$$\n\n after");
}

#[test]
fn soft_breaks_keep_existing_markdown_representation() {
    let blocks = vec![Block::text("\na\n")];
    assert_eq!(markdown::clipboard_text(&blocks), "<br>a<br>");
}

#[test]
fn copy_is_read_only_and_cut_undo_restores_formula() {
    let mut c = core("<p>before <latex>x_t</latex> after</p>");
    select(&mut c, 7, 10);
    let before = docxml::serialize(&c.doc);
    let payload = c.clipboard_content().unwrap();
    assert_eq!(c.clipboard_content().unwrap(), payload);
    assert_eq!(docxml::serialize(&c.doc), before);
    c.delete_selection();
    assert!(!docxml::serialize(&c.doc).contains("<latex>"));
    c.undo();
    assert_eq!(docxml::serialize(&c.doc), before);
}

#[test]
fn multiline_math_stays_inside_its_table_row() {
    let equation = core("<p><latex>a\nb</latex></p>").doc.blocks.remove(0);
    let table = Table { col_widths: vec![Table::DEFAULT_WIDTH], header_rows: 1, header_col: false,
        rows: vec![vec![TableCell { blocks: vec![equation], ..TableCell::empty() }]] };
    let blocks = vec![Block::new(BlockKind::Table(table), RichText::default())];
    assert_eq!(markdown::clipboard_text(&blocks), "| $$<br>a<br>b<br>$$ |\n| --- |");
}
