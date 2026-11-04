//! A disposable projection of source text. No preview serialization is used for saves.
use crate::model::*;
use markdown::{mdast::Node, ParseOptions};
use std::{collections::HashMap, ops::Range, path::Path};

#[derive(Clone, Debug)]
pub struct Preview {
    pub doc: Document,
    pub tasks: HashMap<BlockId, Range<usize>>,
    pub diagrams: Vec<(BlockId, String)>,
    pub anchors: HashMap<String, BlockId>,
    pub diagnostic: Option<String>,
}
fn raw<'a>(node: &Node, source: &'a str) -> &'a str {
    node.position().and_then(|p| source.get(p.start.offset..p.end.offset)).unwrap_or("")
}
fn opaque(node: &Node) -> bool {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        if matches!(node, Node::MdxjsEsm(_) | Node::MdxJsxFlowElement(_) | Node::MdxJsxTextElement(_) | Node::MdxFlowExpression(_) | Node::MdxTextExpression(_) | Node::Html(_) | Node::Yaml(_) | Node::Toml(_) | Node::LinkReference(_) | Node::ImageReference(_) | Node::FootnoteReference(_) | Node::FootnoteDefinition(_)) { return true; }
        if let Some(children) = node.children() { pending.extend(children); }
    }
    false
}
fn code(text: &str, lang: &str) -> Block {
    Block::new(BlockKind::Code { lang: lang.into(), caption: None, wrap: true }, RichText::plain(text))
}
fn markdown_with_links(node: &Node, source: &str) -> Vec<Block> {
    let Some(position) = node.position() else { return vec![]; };
    let start = position.start.offset;
    let mut text = raw(node, source).to_string();
    let mut links = Vec::new(); let mut pending = vec![node];
    while let Some(n) = pending.pop() {
        if let Node::Link(link) = n && let Some(pos) = n.position() {
            let original = raw(n, source);
            let mut depth = 0usize; let mut escaped = false; let mut end = None;
            for (i, c) in original.char_indices() {
                if escaped { escaped = false; continue; }
                if c == '\\' { escaped = true; continue; }
                if c == '[' { depth += 1; }
                if c == ']' { depth = depth.saturating_sub(1); if depth == 0 { end = Some(i); break; } }
            }
            if let Some(end) = end && pos.start.offset >= start && pos.end.offset <= position.end.offset {
                let placeholder = format!("https://quiet-preview.invalid/link-{}", links.len());
                links.push((pos.start.offset - start, pos.end.offset - start, format!("[{}]({placeholder})", &original[1..end]), placeholder, link.url.clone()));
            }
            continue;
        }
        if let Some(children) = n.children() { pending.extend(children); }
    }
    links.sort_by_key(|(start, _, _, _, _)| std::cmp::Reverse(*start));
    for (a, b, replacement, _, _) in &links { if text.get(*a..*b).is_some() { text.replace_range(*a..*b, replacement); } }
    let mut blocks = crate::paste::markdown_to_blocks(&text, crate::paste::Off { file: true, ..Default::default() });
    fn restore(blocks: &mut [Block], links: &[(usize, usize, String, String, String)]) {
        for block in blocks {
            for run in &mut block.text.runs { if let Some(link) = &mut run.marks.link && let Some((_, _, _, _, target)) = links.iter().find(|(_, _, _, placeholder, _)| placeholder == link) { *link = target.clone(); } }
            if let BlockKind::Table(table) = &mut block.kind { for row in &mut table.rows { for cell in row { restore(&mut cell.blocks, links); } } }
            restore(&mut block.children, links);
        }
    }
    restore(&mut blocks, &links); blocks
}
pub fn parse(source: &str, path: &Path, root: &Path) -> Preview {
    let mdx = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mdx"));
    let mut options = if mdx { ParseOptions::mdx() } else { ParseOptions::gfm() };
    options.constructs.frontmatter = true;
    options.constructs.gfm_table = true;
    options.constructs.gfm_task_list_item = true;
    let mut preview = Preview { doc: Document { title: RichText::plain(path.file_name().unwrap_or_default().to_string_lossy()), icon: None, cover: None, blocks: vec![] }, tasks: HashMap::new(), diagrams: vec![], anchors: HashMap::new(), diagnostic: mdx.then(|| "Partial MDX preview: components, imports and expressions are shown as source; no project code runs.".into()) };
    if source.len() as u64 > crate::workspace_files::MAX_SOURCE {
        preview.diagnostic = Some("Preview exceeds the 2 MiB limit; source remains available.".into());
        preview.doc.blocks.push(Block::text("Preview exceeds the 2 MiB limit."));
        return preview;
    }
    match markdown::to_mdast(source, &options) {
        Ok(tree) => {
            let children = tree.children().cloned().unwrap_or_default();
            preview.doc.blocks = preview.blocks(&children, source, None, 0);
        }
        Err(e) => { preview.diagnostic = Some(format!("Preview parse failed; source is intact: {e}")); preview.doc.blocks.push(code(source, if mdx { "mdx" } else { "markdown" })); }
    }
    // Resolve local media without copying it. Keep remote and outside paths visible as text.
    fn images(blocks: &mut [Block], root: &Path, path: &Path, count: &mut usize, pixels: &mut u64) {
        for block in blocks {
            if let BlockKind::Image { src, .. } = &mut block.kind {
                *count += 1;
                let resolved = crate::workspace_files::relative_link(root, path, src).ok().map(|(p, _)| p)
                    .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= 8 * 1024 * 1024))
                    .filter(|p| {
                        let reader = image::ImageReader::open(p).ok().and_then(|r| r.with_guessed_format().ok());
                        reader.is_some_and(|r| matches!(r.format(), Some(image::ImageFormat::Png | image::ImageFormat::Jpeg))
                            && r.into_dimensions().is_ok_and(|(w, h)| {
                                let size = u64::from(w) * u64::from(h);
                                if w == 0 || h == 0 || *pixels + size > 8_000_000 { false } else { *pixels += size; true }
                            }))
                    });
                if let Some(local) = resolved.filter(|_| *count <= 32) { *src = local.to_string_lossy().into(); }
                else { let text = format!("Image: {src} (unsupported, unavailable, or over limit)"); block.kind = BlockKind::Text; block.text = RichText::plain(text); }
            }
            if let BlockKind::Table(table) = &mut block.kind {
                for row in &mut table.rows { for cell in row { images(&mut cell.blocks, root, path, count, pixels); } }
            }
            images(&mut block.children, root, path, count, pixels);
        }
    }
    images(&mut preview.doc.blocks, root, path, &mut 0, &mut 0);
    if preview.doc.blocks.is_empty() { preview.doc.blocks.push(Block::text("")); }
    preview
}
impl Preview {
    fn blocks(&mut self, nodes: &[Node], source: &str, ordered: Option<u32>, depth: usize) -> Vec<Block> {
        let mut result = vec![];
        for (index, node) in nodes.iter().enumerate() {
            let text = raw(node, source);
            if depth >= 64 { result.push(code(text, "markdown")); continue; }
            match node {
                Node::List(list) => result.extend(self.blocks(&list.children, source, list.ordered.then_some(list.start.unwrap_or(1)), depth + 1)),
                Node::ListItem(item) => {
                    let mut children = self.blocks(&item.children, source, None, depth + 1);
                    let mut block = if !children.is_empty() && matches!(children[0].kind, BlockKind::Text) { children.remove(0) } else { Block::text("") };
                    block.kind = if let Some(done) = item.checked { BlockKind::Todo { done } } else if let Some(start) = ordered { BlockKind::Ordered { seq: Some(start + index as u32) } } else { BlockKind::Bullet };
                    block.children = children;
                    if item.checked.is_some() && let Some(position) = node.position() {
                        // Only the marker immediately following the list prefix is editable.
                        let first = text.lines().next().unwrap_or("");
                        let prefix = first.find('[');
                        if let Some(i) = prefix && matches!(first.get(i..i+3), Some("[ ]" | "[x]" | "[X]")) {
                            self.tasks.insert(block.id, position.start.offset+i..position.start.offset+i+3);
                        }
                    }
                    result.push(block);
                }
                Node::Blockquote(quote) => { let mut b = Block::new(BlockKind::Quote, RichText::default()); b.children = self.blocks(&quote.children, source, None, depth + 1); result.push(b); }
                Node::Code(c) => {
                    let block = code(&c.value, c.lang.as_deref().unwrap_or(""));
                    if c.lang.as_deref() == Some("mermaid") { self.diagrams.push((block.id, c.value.clone())); }
                    result.push(block);
                }
                Node::Definition(_) => {},
                _ if opaque(node) => result.push(code(text, "mdx")),
                _ => {
                    let mut blocks = markdown_with_links(node, source);
                    if blocks.is_empty() && !text.trim().is_empty() { blocks.push(code(text, "markdown")); }
                    if let Node::Heading(_) = node && let Some(block) = blocks.first() {
                        let slug: String = block.text.text.to_lowercase().chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).map(|c| if c == ' ' { '-' } else { c }).collect();
                        let mut key = slug.clone(); let mut i = 1;
                        while self.anchors.contains_key(&key) { key = format!("{slug}-{i}"); i += 1; }
                        self.anchors.insert(key, block.id);
                    }
                    result.extend(blocks);
                }
            }
        }
        result
    }
    pub fn toggle(&self, text: &str, id: BlockId) -> Option<String> {
        let range = self.tasks.get(&id)?.clone(); let marker = text.get(range.clone())?;
        let replacement = match marker { "[ ]" => "[x]", "[x]" | "[X]" => "[ ]", _ => return None };
        let mut result = text.to_string(); result.replace_range(range, replacement); Some(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_preview_preserves_mdx_and_task_ranges() {
        let root = Path::new("/tmp");
        let source = "---\ntitle: Test\n---\n# Hello\n\n<Component value={2}>child</Component>\n\n- [ ] duplicate\n- [X] duplicate\n\n```mermaid\nflowchart LR\nA-->B\n```\n";
        let p = parse(source, &root.join("a.mdx"), root);
        assert_eq!(p.tasks.len(), 2, "{:?}", p.diagnostic);
        assert_eq!(p.diagrams.len(), 1);
        assert!(p.doc.blocks.iter().any(|b| b.text.text.contains("<Component")));
        let id = *p.tasks.iter().find(|(_, r)| &source[(*r).clone()] == "[ ]").unwrap().0;
        assert_eq!(p.toggle(source, id).unwrap(), source.replacen("[ ]", "[x]", 1));
        assert!(p.anchors.contains_key("hello"));
        let headings = parse("# Same\n\n# Same\n", &root.join("a.md"), root);
        assert!(headings.anchors.contains_key("same") && headings.anchors.contains_key("same-1"));
        let nested_source = "- parent\n  - [ ] child\n";
        let nested = parse(nested_source, &root.join("a.md"), root);
        let nested_id = *nested.tasks.keys().next().unwrap();
        assert_eq!(nested.toggle(nested_source, nested_id).unwrap(), "- parent\n  - [x] child\n");
        let mdx = "import Widget from './widget'\n\n# Heading\n\n<Widget value={3} />\n";
        let imported = parse(mdx, &root.join("a.mdx"), root);
        assert!(imported.doc.blocks.iter().any(|b| b.text.text.contains("import Widget")));
        assert!(imported.doc.blocks.iter().any(|b| b.text.text.contains("<Widget")));
        let links = parse("[next](other.md#heading) and [local](#same)\n", &root.join("a.md"), root);
        let urls = links.doc.blocks.iter().flat_map(|b| b.text.runs.iter().filter_map(|r| r.marks.link.as_deref())).collect::<Vec<_>>();
        assert!(urls.contains(&"other.md#heading") && urls.contains(&"#same"));
        let broken = parse("<Component value={", &root.join("a.mdx"), root);
        assert!(broken.diagnostic.unwrap().contains("failed"));
        assert_eq!(broken.doc.blocks[0].text.text, "<Component value={");
        let references = parse("See [label][id].\n\n[id]: https://example.com\n\n![pic][img]\n\n[img]: image.png\n", &root.join("a.md"), root);
        assert!(references.doc.blocks.iter().any(|b| b.text.text.contains("[label][id]")));
        assert!(references.doc.blocks.iter().any(|b| b.text.text.contains("![pic][img]")));
    }
    #[test]
    fn workspace_preview_local_media_limits() {
        let dir = std::env::temp_dir().join(format!("quiet-media-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let root = dir.canonicalize().unwrap();
        image::save_buffer(root.join("ok.png"), &[255, 0, 0, 255], 1, 1, image::ColorType::Rgba8).unwrap();
        std::fs::write(root.join("unsafe.svg"), "<svg xmlns='http://www.w3.org/2000/svg'/>").unwrap();
        let path = root.join("a.md");
        let p = parse("![ok](ok.png)\n\n![svg](unsafe.svg)\n\n![remote](https://example.com/x.png)", &path, &root);
        assert!(p.doc.blocks.iter().any(|b| matches!(&b.kind, BlockKind::Image { src, .. } if src == root.join("ok.png").to_str().unwrap())));
        assert!(p.doc.blocks.iter().filter(|b| b.text.text.contains("unsupported, unavailable, or over limit")).count() >= 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
