//! Blocks named the way the `quiet` CLI names them, shared with the app so a
//! ref the CLI hands over (`quiet open DOC#REF`) finds the same block there. A
//! ref is a block's position, 1-based and dotted through children (`#3.1`),
//! with a fingerprint of the block's kind and text (`#3.1~k7f2`): the block at
//! that position must still have that fingerprint, else the one block in the
//! document that has it is taken (it moved).

use crate::model::{Block, BlockId, BlockKind, Document, RichText};

pub struct Item<'a> {
    pub path: Vec<usize>,
    pub block: &'a Block,
}

/// Every block in document order, children after their parent.
pub fn items(doc: &Document) -> Vec<Item<'_>> {
    fn walk<'a>(list: &'a [Block], at: &mut Vec<usize>, out: &mut Vec<Item<'a>>) {
        for (i, b) in list.iter().enumerate() {
            at.push(i);
            out.push(Item { path: at.clone(), block: b });
            walk(&b.children, at, out);
            at.pop();
        }
    }
    let mut out = vec![];
    walk(&doc.blocks, &mut vec![], &mut out);
    out
}

pub fn ref_of(path: &[usize]) -> String {
    format!("#{}", path.iter().map(|i| (i + 1).to_string()).collect::<Vec<_>>().join("."))
}

/// Four base-32 characters from the block's kind and its own text, inline
/// objects (mentions, equations) included.
pub fn fingerprint(b: &Block) -> String {
    let inline: String = b.text.runs.iter().filter_map(|r| r.marks.inline.as_ref()).map(|i| i.xml.as_str()).collect();
    let d = sha1_smol::Sha1::from(format!("{}\0{}\0{inline}", kind_name(&b.kind), text_of(b))).digest().bytes();
    const A: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let n = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
    (0..4).map(|i| A[((n >> (i * 5)) & 31) as usize] as char).collect()
}

/// The self-checking ref of a listed block: `#3.1~k7f2`.
pub fn pin(it: &Item) -> String { format!("{}~{}", ref_of(&it.path), fingerprint(it.block)) }

/// The self-checking ref of the block `id`, if it is in the document.
pub fn pin_of(doc: &Document, id: BlockId) -> Option<String> { items(doc).iter().find(|it| it.block.id == id).map(pin) }

pub fn kind_name(k: &BlockKind) -> String {
    match k {
        BlockKind::Text => "text".into(),
        BlockKind::Heading { level, .. } => format!("h{level}"),
        BlockKind::Bullet => "bullet".into(),
        BlockKind::Ordered { .. } => "ordered".into(),
        BlockKind::Todo { .. } => "todo".into(),
        BlockKind::Code { .. } => "code".into(),
        BlockKind::Quote => "quote".into(),
        BlockKind::Callout { .. } => "callout".into(),
        BlockKind::Divider => "divider".into(),
        BlockKind::Image { .. } => "image".into(),
        BlockKind::Table(_) => "table".into(),
        BlockKind::Grid => "grid".into(),
        BlockKind::Column { .. } => "column".into(),
        BlockKind::SubPageList => "subpages".into(),
        BlockKind::Countdown(_) => "countdown".into(),
        BlockKind::File(_) => "file".into(),
        BlockKind::Board(_) => "board".into(),
        BlockKind::DataTable(_) => "datatable".into(),
        BlockKind::Raw { tag, .. } => format!("raw:{tag}"),
    }
}

/// A block's text as a reader sees it; a table as its rows, cells joined by ` | `.
pub fn text_of(b: &Block) -> String {
    match &b.kind {
        BlockKind::Table(t) => t.rows.iter().map(|row| row.iter().filter(|c| !c.is_covered()).map(|c| c.blocks.iter().map(|b| shown(&b.text)).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" | ")).collect::<Vec<_>>().join("\n"),
        BlockKind::Image { caption, src, .. } => caption.clone().unwrap_or_else(|| src.clone()),
        BlockKind::File(f) => f.name.clone(),
        BlockKind::Board(bd) => bd.plain_text(),
        BlockKind::DataTable(t) => t.src.clone(),
        _ => shown(&b.text),
    }
}

/// Text as the page shows it: a mention as the title of what it points at.
fn shown(t: &RichText) -> String {
    if !t.text.contains('\u{fffc}') { return t.text.clone(); }
    let mut out = String::new();
    for (r, m) in t.spans() {
        match m.inline.as_ref().and_then(|i| i.doc_link()) {
            Some(l) => out.push_str(&l.title),
            None => { let mut one = RichText::default(); one.push(&t.text[r], m.clone()); out.push_str(&one.plain_text()); }
        }
    }
    out
}


/// Pinned ref `pin` (`#3.1~k7f2`, `3.1`, `#3`) in `doc`: the block at that position
/// when its fingerprint matches (or none is given), else the one block with that
/// fingerprint; None when neither finds one block.
pub fn locate(doc: &Document, pin: &str) -> Option<BlockId> {
    let (at, fp) = parse(pin)?;
    let all = items(doc);
    let here = all.iter().find(|it| it.path == at);
    let Some(fp) = fp else { return here.map(|it| it.block.id) };
    if let Some(it) = here.filter(|it| fingerprint(it.block) == fp) { return Some(it.block.id); }
    let same: Vec<&Item> = all.iter().filter(|it| fingerprint(it.block) == fp).collect();
    (same.len() == 1).then(|| same[0].block.id)
}

/// A ref's position (0-based) and fingerprint; None when it is not one.
pub fn parse(pin: &str) -> Option<(Vec<usize>, Option<&str>)> {
    let bare = pin.trim().trim_start_matches('#');
    let (at, fp) = match bare.split_once('~') { Some((p, f)) => (p, Some(f)), None => (bare, None) };
    if at.is_empty() || fp.is_some_and(|f| f.len() != 4 || !f.bytes().all(|c| c.is_ascii_alphanumeric())) { return None; }
    let path = at.split('.').map(|p| p.parse::<usize>().ok().filter(|n| *n > 0 && p.bytes().all(|c| c.is_ascii_digit())).map(|n| n - 1)).collect::<Option<Vec<_>>>()?;
    Some((path, fp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RichText;

    fn block(id: u64, kind: BlockKind, text: &str) -> Block { Block { id: BlockId(id), ..Block::new(kind, RichText::plain(text)) } }

    #[test]
    fn a_pinned_ref_finds_its_block_where_it_moved() {
        let mut quote = block(2, BlockKind::Quote, "");
        quote.children = vec![block(3, BlockKind::Text, "inside")];
        let mut doc = Document { blocks: vec![block(1, BlockKind::Heading { level: 1, seq_auto: false }, "风险"), quote, block(4, BlockKind::Text, "last")], ..Default::default() };
        let inside = pin_of(&doc, BlockId(3)).unwrap();
        assert!(inside.starts_with("#2.1~"), "{inside}");
        assert_eq!(locate(&doc, &inside), Some(BlockId(3)));
        assert_eq!((locate(&doc, "3"), locate(&doc, "#1"), locate(&doc, "#9")), (Some(BlockId(4)), Some(BlockId(1)), None));
        // Moved down one: found by its fingerprint.
        doc.blocks.insert(0, block(5, BlockKind::Text, "new"));
        assert_eq!(locate(&doc, &inside), Some(BlockId(3)));
        // Its text changed: no longer the block the ref was read from.
        doc.blocks[2].children[0].text = RichText::plain("changed");
        assert_eq!(locate(&doc, &inside), None);
        for bad in ["", "#", "#0", "1..2", "#1~abc", "#a", "#1~ab$d"] { assert!(parse(bad).is_none(), "{bad}"); }
    }
}
