//! Deterministic document, folder and block locators for CLI commands.
//! Because `BlockId` is regenerated per `docxml::parse`, CLI locators address
//! blocks by structural path (`0`, `2.1`, `3[0,1].0`, `4.col1.0`), flat order
//! (`#5`), or unique semantic match (`@heading:Title`, `@match:text`, `@kind:table`).

use crate::output::CliFailure;
use fdl_core::model::{Block, BlockId, BlockKind, Document};
use fdl_core::store::{Entry, Store};

pub fn resolve_doc(store: &mut Store, target: &str) -> Result<Entry, CliFailure> {
    let entries = store.list();
    let docs: Vec<Entry> = entries.into_iter().filter(|e| e.meta.folder.is_none()).collect();
    if let Some(e) = docs.iter().find(|e| e.id == target) {
        return Ok(e.clone());
    }
    let norm = target.trim().trim_start_matches("./");
    let rel_matches: Vec<&Entry> = docs.iter().filter(|e| {
        let r = e.meta.rel.as_str();
        r == norm || r.rsplit_once('.').is_some_and(|(stem, _)| stem == norm)
    }).collect();
    if rel_matches.len() == 1 {
        return Ok(rel_matches[0].clone());
    }
    let title_matches: Vec<&Entry> = docs.iter().filter(|e| e.title == target).collect();
    if title_matches.len() == 1 {
        return Ok(title_matches[0].clone());
    }
    let cands: Vec<&Entry> = if !rel_matches.is_empty() { rel_matches } else { title_matches };
    if cands.len() > 1 {
        let json_cands = cands.iter().map(|e| serde_json::json!({ "id": e.id, "title": e.title, "rel": e.meta.rel })).collect();
        return Err(CliFailure::new("AMBIGUOUS_DOC", format!("Multiple documents match '{target}'."))
            .with_candidates(json_cands)
            .with_hint("Pass the document's UUID id or library-relative path (e.g. 'Folder/Title.xml')."));
    }
    Err(CliFailure::new("DOC_NOT_FOUND", format!("No document found matching '{target}'."))
        .with_hint("Run `qmd library tree --json` to inspect available document ids, paths and titles."))
}

pub fn resolve_folder(store: &mut Store, target: &str) -> Result<Entry, CliFailure> {
    let entries = store.list();
    let folders: Vec<Entry> = entries.into_iter().filter(|e| e.meta.folder.is_some()).collect();
    if let Some(e) = folders.iter().find(|e| e.id == target) {
        return Ok(e.clone());
    }
    let norm = target.trim().trim_matches('/');
    let rel_matches: Vec<&Entry> = folders.iter().filter(|e| e.meta.rel.trim_end_matches('/') == norm).collect();
    if rel_matches.len() == 1 {
        return Ok(rel_matches[0].clone());
    }
    let name_matches: Vec<&Entry> = folders.iter().filter(|e| e.title == norm).collect();
    if name_matches.len() == 1 {
        return Ok(name_matches[0].clone());
    }
    let cands = if !rel_matches.is_empty() { rel_matches } else { name_matches };
    if cands.len() > 1 {
        let json_cands = cands.iter().map(|e| serde_json::json!({ "id": e.id, "title": e.title, "rel": e.meta.rel })).collect();
        return Err(CliFailure::new("AMBIGUOUS_FOLDER", format!("Multiple folders match '{target}'."))
            .with_candidates(json_cands)
            .with_hint("Pass the folder's UUID id or relative path (e.g. 'Engineering/RFCs')."));
    }
    Err(CliFailure::new("FOLDER_NOT_FOUND", format!("No folder found matching '{target}'."))
        .with_hint("Run `qmd library tree --json` to list folders."))
}

/// Optional folder target where empty / `"/"` / `"root"` means library top-level.
pub fn resolve_optional_folder(store: &mut Store, target: Option<&str>) -> Result<Option<String>, CliFailure> {
    match target.map(str::trim) {
        None | Some("") | Some("/") | Some("root") => Ok(None),
        Some(t) => Ok(Some(resolve_folder(store, t)?.id)),
    }
}

#[derive(Clone, Debug)]
pub struct IndexedBlock {
    pub id: BlockId,
    pub loc: String,
    pub order: usize,
    pub kind: String,
    pub preview: String,
}

pub fn block_kind_name(k: &BlockKind) -> &'static str {
    match k {
        BlockKind::Text => "p",
        BlockKind::Heading { level, .. } => match level {
            1 => "h1", 2 => "h2", 3 => "h3", 4 => "h4", 5 => "h5", 6 => "h6", 7 => "h7", 8 => "h8", _ => "h9",
        },
        BlockKind::Bullet => "bullet",
        BlockKind::Ordered { .. } => "ordered",
        BlockKind::Todo { .. } => "todo",
        BlockKind::Code { .. } => "code",
        BlockKind::Quote => "quote",
        BlockKind::Callout { .. } => "callout",
        BlockKind::Divider => "divider",
        BlockKind::Image { .. } => "image",
        BlockKind::Table(_) => "table",
        BlockKind::Grid => "grid",
        BlockKind::Column { .. } => "column",
        BlockKind::SubPageList => "sub_page_list",
        BlockKind::Countdown(_) => "countdown",
        BlockKind::File(_) => "file",
        BlockKind::Board(_) => "board",
        BlockKind::Raw { .. } => "raw",
    }
}

fn snippet(b: &Block) -> String {
    let raw = match &b.kind {
        BlockKind::Board(bd) => bd.plain_text(),
        BlockKind::Image { caption, src, .. } => caption.clone().unwrap_or_else(|| src.clone()),
        BlockKind::File(f) => f.name.clone(),
        _ => b.text.plain_text(),
    };
    let s: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = s.chars().take(60).collect();
    if s.chars().count() > 60 { out.push('…'); }
    out
}

pub fn index_blocks(doc: &Document) -> Vec<IndexedBlock> {
    fn walk(list: &[Block], prefix: &str, out: &mut Vec<IndexedBlock>) {
        for (i, b) in list.iter().enumerate() {
            let loc = if prefix.is_empty() { format!("{i}") } else { format!("{prefix}.{i}") };
            let order = out.len();
            out.push(IndexedBlock { id: b.id, loc: loc.clone(), order, kind: block_kind_name(&b.kind).into(), preview: snippet(b) });
            match &b.kind {
                BlockKind::Grid => {
                    for (ci, col) in b.children.iter().enumerate() {
                        let col_loc = format!("{loc}.col{ci}");
                        let c_order = out.len();
                        out.push(IndexedBlock { id: col.id, loc: col_loc.clone(), order: c_order, kind: "column".into(), preview: String::new() });
                        walk(&col.children, &col_loc, out);
                    }
                }
                BlockKind::Table(t) => {
                    for (r, row) in t.rows.iter().enumerate() {
                        for (c, cell) in row.iter().enumerate() {
                            if !cell.is_covered() {
                                walk(&cell.blocks, &format!("{loc}[{r},{c}]"), out);
                            }
                        }
                    }
                }
                _ => walk(&b.children, &loc, out),
            }
        }
    }
    let mut out = vec![];
    walk(&doc.blocks, "", &mut out);
    out
}

pub fn find_block(doc: &Document, id: BlockId) -> Option<&Block> {
    fn walk(list: &[Block], id: BlockId) -> Option<&Block> {
        for b in list {
            if b.id == id { return Some(b); }
            if let BlockKind::Table(t) = &b.kind {
                for row in &t.rows {
                    for cell in row {
                        if let Some(found) = walk(&cell.blocks, id) { return Some(found); }
                    }
                }
            } else if let Some(found) = walk(&b.children, id) {
                return Some(found);
            }
        }
        None
    }
    walk(&doc.blocks, id)
}

pub fn loc_of(doc: &Document, id: BlockId) -> Option<String> {
    index_blocks(doc).into_iter().find(|b| b.id == id).map(|b| b.loc)
}

/// Parse a block locator:
/// - `last` / `first`
/// - `#N` (0-based order index)
/// - `0`, `2.1`, `3[1,2].0`, `4.col1.0` (tree path)
/// - `@heading:<text>`, `@match:<substr>`, `@kind:<kind>`
pub fn resolve_block(doc: &Document, sel: &str) -> Result<BlockId, CliFailure> {
    let idx = index_blocks(doc);
    let s = sel.trim();
    if s == "first" {
        return doc.blocks.first().map(|b| b.id).ok_or_else(|| CliFailure::new("EMPTY_DOCUMENT", "Document has no blocks."));
    }
    if s == "last" {
        return doc.blocks.last().map(|b| b.id).ok_or_else(|| CliFailure::new("EMPTY_DOCUMENT", "Document has no blocks."));
    }
    if let Some(num) = s.strip_prefix('#') {
        let n: usize = num.parse().map_err(|_| CliFailure::usage(format!("Invalid order locator '{sel}'.")))?;
        return idx.get(n).map(|b| b.id).ok_or_else(|| {
            CliFailure::new("BLOCK_NOT_FOUND", format!("No block at order #{n} (document has {} indexed blocks).", idx.len()))
        });
    }
    if let Some(q) = s.strip_prefix("@heading:") {
        let q = q.trim();
        let hits: Vec<&IndexedBlock> = idx.iter().filter(|b| b.kind.starts_with('h') && b.kind.len() == 2 && b.preview.contains(q)).collect();
        return pick_unique(&hits, sel);
    }
    if let Some(q) = s.strip_prefix("@match:") {
        let q = q.trim();
        let hits: Vec<&IndexedBlock> = idx.iter().filter(|b| b.preview.contains(q)).collect();
        return pick_unique(&hits, sel);
    }
    if let Some(q) = s.strip_prefix("@kind:") {
        let q = q.trim();
        let hits: Vec<&IndexedBlock> = idx.iter().filter(|b| b.kind == q).collect();
        return pick_unique(&hits, sel);
    }
    // Canonical tree locator match (case-insensitive for `col`)
    let norm = s.to_ascii_lowercase();
    if let Some(b) = idx.iter().find(|b| b.loc.to_ascii_lowercase() == norm) {
        return Ok(b.id);
    }
    Err(CliFailure::new("BLOCK_NOT_FOUND", format!("No block at locator '{sel}'."))
        .with_hint("Run `qmd doc get <doc> --json` to inspect each block's `loc` (e.g. '0', '2.1', '3[0,1].0', '4.col1.0') or use 'first', 'last', '#N', '@heading:...', '@match:...'."))
}

fn pick_unique(hits: &[&IndexedBlock], sel: &str) -> Result<BlockId, CliFailure> {
    if hits.len() == 1 {
        return Ok(hits[0].id);
    }
    if hits.is_empty() {
        return Err(CliFailure::new("BLOCK_NOT_FOUND", format!("Selector '{sel}' matched 0 blocks.")));
    }
    let cands = hits.iter().map(|b| serde_json::json!({ "loc": b.loc, "order": b.order, "kind": b.kind, "preview": b.preview })).collect();
    Err(CliFailure::new("AMBIGUOUS_BLOCK", format!("Selector '{sel}' matched {} blocks.", hits.len()))
        .with_candidates(cands)
        .with_hint("Narrow the selector or pass one of the exact `loc` values above."))
}
