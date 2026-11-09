use fdl_core::{diagram::TextDiagram, markdown, model::*};

#[test]
fn markdown_file_and_export_preserve_indented_fences_in_drawings() {
    for fence in ["  ```", "\t````", "   ``````"] {
        let source = format!("flowchart LR\nA --> B\n{fence}\nB --> C");
        let block = Block::new(BlockKind::Diagram(TextDiagram::new(&source)), RichText::default());
        let doc = Document { blocks: vec![block], ..Default::default() };
        let saved = markdown::write_file(&doc, "", None);
        let export = markdown::export(&doc, &markdown::Options::default()).text;
        for text in [saved, export] {
            let reopened = markdown::parse_file(&text, "Drawing", None);
            assert_eq!(reopened.blocks.len(), 1, "{text}");
            let BlockKind::Diagram(d) = &reopened.blocks[0].kind else { panic!("{text}") };
            assert_eq!(d.source, source);
            let opts = markdown::Options { original: Some(&text), ..Default::default() };
            assert_eq!(markdown::write_file_with(&reopened, "", &opts).text, text);
        }
    }
}
