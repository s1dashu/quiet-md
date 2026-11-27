//! 数据表 (多维表格): the engine. `model` is the data, `field` what each
//! field type does with a value, `ops` every change (with undo), `calc` the
//! computed columns, `formula` the 公式 language, `query` a view's rows
//! (筛选 / 排序 / 分组 / 统计), `actions` the edits the UI asks for, `sqlite`
//! the file, `csv` import / export and the clipboard. No GPUI.

pub mod actions;
pub mod calc;
pub mod csv;
pub mod field;
pub mod formula;
#[cfg(test)]
mod fuzz_tests;
pub mod model;
pub mod ops;
pub mod order;
pub mod query;
pub mod sample;
#[cfg(not(target_family = "wasm"))]
pub mod sqlite;

pub use model::*;
pub use ops::{BaseOp, Dirty, Doc, Op};

/// The block's default height in a page.
pub const DEFAULT_HEIGHT: f32 = 420.;

/// A table or column name the file can carry as it is: not empty, not one of
/// our bookkeeping names (`_fields`, `id`), nothing SQLite reserves.
pub fn sqlite_name_ok(n: &str) -> bool {
    let l = n.to_ascii_lowercase();
    !n.trim().is_empty() && n.trim() == n && !n.starts_with('_') && !l.starts_with("sqlite_") && l != "id" && !n.contains('\0')
}

/// A 数据表 block: which file, and which of its tables and views shows.
/// The data itself lives in the file (`.sqlite`), not in the page.
#[derive(Clone, Debug, PartialEq)]
pub struct TableRef {
    /// Relative to the library (`.assets/<uuid>.sqlite`, or a table file's own path).
    pub src: String,
    /// The table and view shown, by name (None: the first).
    pub table: Option<String>,
    pub view: Option<String>,
    pub height: f32,
    /// Dragged wider or narrower than the text column, centred on it (None: the column's width).
    pub width: Option<f32>,
}

impl TableRef {
    pub fn new(src: impl Into<String>) -> Self { TableRef { src: src.into(), table: None, view: None, height: DEFAULT_HEIGHT, width: None } }
}

/// A table file as the page the store hands out: its title and one 数据表 block showing it.
pub fn file_doc(title: &str, rel: &str) -> crate::model::Document {
    let mut doc = crate::model::Document::default();
    doc.title = crate::model::RichText::plain(title);
    doc.blocks = vec![crate::model::Block::new(crate::model::BlockKind::DataTable(TableRef::new(rel)), crate::model::RichText::default())];
    doc
}

/// The table and view a block shows: by name, else the first.
pub fn shown<'a>(base: &'a Base, r: &TableRef) -> Option<(&'a Table, &'a View)> {
    let t = r.table.as_deref().and_then(|n| base.table_named(n)).or(base.tables.first())?;
    let v = r.view.as_deref().and_then(|n| t.views.iter().find(|v| v.name == n)).or(t.views.first())?;
    Some((t, v))
}

/// A 数据表 block as a Markdown table (导出): the view it shows, as it is filtered and sorted.
#[cfg(not(target_family = "wasm"))]
pub fn block_markdown(root: &std::path::Path, r: &TableRef) -> String {
    let Ok(base) = sqlite::load(&root.join(&r.src)) else { return String::new() };
    let base = Doc::new(base).base;
    let Some((t, v)) = shown(&base, r) else { return String::new() };
    csv::to_markdown(&base, t, v, &query::run(&base, t, v).records)
}
#[cfg(target_family = "wasm")]
pub fn block_markdown(_root: &std::path::Path, _r: &TableRef) -> String { String::new() }

/// A 数据表 block's words (搜索): every cell of the file.
#[cfg(not(target_family = "wasm"))]
pub fn block_text(root: &std::path::Path, r: &TableRef) -> String {
    let Ok(base) = sqlite::load(&root.join(&r.src)) else { return String::new() };
    let base = Doc::new(base).base;
    base.tables.iter().map(|t| csv::plain_text(&base, t)).collect::<Vec<_>>().join("\n")
}
#[cfg(target_family = "wasm")]
pub fn block_text(_root: &std::path::Path, _r: &TableRef) -> String { String::new() }

/// A 数据表 file as lines of text, a line per row (版本历史 counts changes on these).
#[cfg(not(target_family = "wasm"))]
pub fn diff_text(path: &std::path::Path) -> String {
    let Ok(base) = sqlite::load(path) else { return String::new() };
    let base = Doc::new(base).base;
    let Some((t, v)) = base.tables.first().and_then(|t| Some((t, t.views.first()?))) else { return String::new() };
    csv::export(&base, t, v, &query::run(&base, t, v).records).iter().map(|r| r.join("\t")).collect::<Vec<_>>().join("\n")
}
#[cfg(target_family = "wasm")]
pub fn diff_text(_path: &std::path::Path) -> String { String::new() }

/// A 数据表 block as a grid of text, header first (HTML 导出, printing).
#[cfg(not(target_family = "wasm"))]
pub fn block_grid(root: &std::path::Path, r: &TableRef) -> Vec<Vec<String>> {
    let Ok(base) = sqlite::load(&root.join(&r.src)) else { return vec![] };
    let base = Doc::new(base).base;
    let Some((t, v)) = shown(&base, r) else { return vec![] };
    csv::export(&base, t, v, &query::run(&base, t, v).records)
}
#[cfg(target_family = "wasm")]
pub fn block_grid(_root: &std::path::Path, _r: &TableRef) -> Vec<Vec<String>> { vec![] }

/// The top-left of a 数据库 block's grid as it shows (a folder page's card, a page's thumbnail):
/// the base cut down to what is drawn, so a card holds a few dozen cells, not the file.
#[derive(Clone, Debug, Default)]
pub struct Still {
    /// The shown table, only its rows drawn and its columns in sight; the tables its 关联 point at,
    /// only the records named (their titles); no other table.
    pub base: Base,
    pub table: TableId,
    pub view: View,
    /// The grid's first rows, as `query::run` lists them (`Row::Record` into the cut table's records).
    pub rows: Vec<query::Row>,
    /// Each row's number (the gutter's) and 填色, as the grid gives them; groups have none.
    pub numbers: Vec<usize>,
    pub tints: Vec<Option<u8>>,
    /// The view lists more rows than these, more columns.
    pub more: bool,
    pub more_cols: bool,
}

impl Still {
    /// `rows` rows of the view `r` shows, and its columns until `width` (a point past the frozen ones).
    pub fn of(base: &Base, r: &TableRef, rows: usize, width: f32) -> Option<Still> {
        let (t, v) = shown(base, r)?;
        let result = query::run(base, t, v);
        let today = field::today();
        let place: std::collections::HashMap<usize, usize> = result.records.iter().enumerate().map(|(p, i)| (*i, p)).collect();
        // columns: the visible ones in order, until the frozen ones and `width` are covered
        let mut ids: std::collections::HashSet<FieldId> = Default::default();
        let mut x = 0.;
        let mut more_cols = false;
        for (ci, f) in t.visible_fields(v).into_iter().enumerate() {
            if ci >= v.opts.frozen.max(1) && x >= width { more_cols = true; break }
            ids.insert(f.id);
            x += t.width(v, f.id);
        }
        let mut cut = Table { id: t.id, name: t.name.clone(), fields: t.fields.iter().filter(|f| ids.contains(&f.id) || t.primary().is_some_and(|p| p.id == f.id)).cloned().collect(), ..Default::default() };
        let mut still = Still { table: t.id, view: v.clone(), more: result.rows.len() > rows, more_cols, ..Default::default() };
        for row in result.rows.iter().take(rows) {
            match row {
                query::Row::Record(i) => {
                    let rec = &t.records[*i];
                    still.numbers.push(place.get(i).map_or(0, |p| p + 1));
                    still.tints.push(query::row_color(base, t, rec, v, today));
                    still.rows.push(query::Row::Record(cut.records.len()));
                    let mut rec = rec.clone();
                    rec.cells.retain(|f, _| ids.contains(f));
                    cut.records.push(rec);
                }
                g => { still.numbers.push(0); still.tints.push(None); still.rows.push(g.clone()); }
            }
        }
        // the tables 关联 cells point at, with the records named (a chip shows the title)
        let mut linked: Vec<Table> = vec![];
        for f in cut.fields.iter().filter(|f| f.ty == FieldType::Link) {
            let Some(other) = f.opts.link_table.and_then(|id| base.table(id)) else { continue };
            let named: std::collections::HashSet<RecId> = cut.records.iter().flat_map(|r| match r.get(f.id) { Value::Links(ids) => ids.clone(), _ => vec![] }).collect();
            let at = match linked.iter().position(|l| l.id == other.id) { Some(p) => p, None => { linked.push(Table { id: other.id, name: other.name.clone(), fields: other.fields.clone(), ..Default::default() }); linked.len() - 1 } };
            for rec in other.records.iter().filter(|r| named.contains(&r.id)) { if linked[at].record(rec.id).is_none() { linked[at].records.push(rec.clone()); } }
        }
        // a table linked to itself: its named records are in the cut one already or added there
        if let Some(p) = linked.iter().position(|l| l.id == cut.id) { let own = linked.remove(p); for r in own.records { if cut.record(r.id).is_none() { cut.records.push(r); } } }
        still.base.tables.push(cut);
        still.base.tables.extend(linked);
        Some(still)
    }

    pub fn table(&self) -> Option<&Table> { self.base.table(self.table) }
}

/// A 数据库 block's `Still` (`rows`, `width` as there), read from its file.
#[cfg(not(target_family = "wasm"))]
pub fn block_still(root: &std::path::Path, r: &TableRef, rows: usize, width: f32) -> Option<Still> {
    let base = Doc::new(sqlite::load(&root.join(&r.src)).ok()?).base;
    Still::of(&base, r, rows, width)
}
#[cfg(target_family = "wasm")]
pub fn block_still(_root: &std::path::Path, r: &TableRef, rows: usize, width: f32) -> Option<Still> { Still::of(&sample::demo(field::now()), r, rows, width) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn still_keeps_what_shows() {
        let base = Doc::new(sample::demo(field::now())).base;
        let r = TableRef::new("x.sqlite");
        let (t, v) = shown(&base, &r).unwrap();
        let s = Still::of(&base, &r, 5, 700.).unwrap();
        let cut = s.table().unwrap();
        // the first five rows, numbered as the grid numbers them, their cells as they are
        let want = query::run(&base, t, v);
        assert_eq!(s.rows.len(), 5.min(want.rows.len()));
        assert_eq!(s.numbers, (1..=s.rows.len()).collect::<Vec<_>>());
        for (row, i) in s.rows.iter().zip(&want.records) {
            let query::Row::Record(j) = row else { panic!("a group") };
            let (a, b) = (&cut.records[*j], &t.records[*i]);
            assert_eq!(a.id, b.id);
            for f in &cut.fields { assert_eq!(a.get(f.id), b.get(f.id)); }
        }
        // the columns in sight and no more: 240 + 180 + 180 + 180 reach 700
        let shown: Vec<&str> = cut.visible_fields(&s.view).iter().map(|f| f.name.as_str()).collect();
        let all: Vec<&str> = t.visible_fields(v).iter().map(|f| f.name.as_str()).collect();
        assert_eq!(shown, all[..4]);
        // a 关联's chips find their titles; the linked table keeps only the records named
        let s = Still::of(&base, &r, 50, 1e9).unwrap();
        let cut = s.table().unwrap();
        let link = cut.fields.iter().find(|f| f.ty == FieldType::Link).unwrap();
        let other = s.base.table(link.opts.link_table.unwrap()).unwrap();
        for rec in &cut.records { if let Value::Links(ids) = rec.get(link.id) { for id in ids { assert!(other.record(*id).is_some()); } } }
        assert!(other.records.len() <= base.table(other.id).unwrap().records.len());
        let first = t.records.iter().find(|x| x.id == cut.records[0].id).unwrap();
        assert_eq!(field::display_in(&s.base, link, cut.records[0].get(link.id)), field::display_in(&base, link, first.get(link.id)));
    }
}
