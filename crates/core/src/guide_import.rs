//! 导入最新的开始指南: the guides built into this binary (`crate::guides`) as a
//! folder of their own at a library's top, beside what is there; nothing in the
//! library is overwritten or merged into.
//!
//! The guides are a library on their own (build.py): files where they go,
//! links between them as paths relative to the file, the 数据库's 笔记 cells
//! as paths from the table's folder, pictures and attachments under
//! `.assets/guides/` from the library's top. Put whole into one folder, the
//! relative paths still meet; what names `.assets/` does too, as long as the
//! pictures are where it says. Those of an earlier copy (the first launch's
//! guides) may be there already: the same file is used again, and when one
//! there is another (an older picture, a 数据库 the user edited) this
//! import's go to `.assets/guides-2/` and what names them says so.
//!
//! Only the files and the order come from the bundle: every entry gets an id
//! of its own (the first launch's guides keep the bundle's), and the device
//! build.py writes the metadata as is left out.

use super::*;

impl Store {
    /// `files` (`guides::files`, the documents as this system reads them) into a new
    /// folder `title` at this library's top, numbered when taken, in the guides'
    /// order. Every entry made is in the file journal (撤销 takes them back). The folder's id.
    pub fn import_guides(&mut self, files: &[(&str, std::borrow::Cow<'_, [u8]>)], title: &str) -> Result<String> {
        anyhow::ensure!(!self.flat, "旧格式的文档库没有真实文件夹");
        self.begin_batch()?;
        let mut made = vec![];
        let r = self.put_guides(files, title, &mut made);
        let ids = r.as_ref().map(|(_, ids)| ids.clone()).unwrap_or_default();
        let saved = self.end_batch();
        if r.is_err() || saved.is_err() {
            // Nothing half there: what was written goes again.
            if saved.is_err() { self.index.retain(|k, _| !ids.contains(k)); }
            for p in made.iter().rev() { let _ = if p.fs_is_dir() { crate::vfs::remove_dir_all(p) } else { crate::vfs::remove_file(p) }; }
        }
        let (folder, ids) = r?;
        saved?;
        for id in &ids { self.record_file_change(id, None); }
        // First at the top, where a new folder shows; placed by hand there (the others numbered
        // with it, as a drag does) so a folder page's first opening has no order of its own to
        // write: 撤销 then takes back the import, not that.
        let first = self.index.iter().filter(|(k, m)| m.parent.is_none() && !m.local() && **k != folder).min_by(|a, b| (a.1.place(), a.0).cmp(&(b.1.place(), b.0))).map(|(k, _)| k.clone());
        if let Err(e) = self.move_in_tree_with(&folder, None, first.as_deref(), self.place_by_hand) { self.warnings.push(format!("未能排列导入的指南：{e}")); }
        Ok(folder)
    }

    /// `import_guides` inside its batch: (the folder, every entry made); `made`: the files
    /// and folders written, the first one the guides' folder.
    fn put_guides(&mut self, files: &[(&str, std::borrow::Cow<'_, [u8]>)], title: &str, made: &mut Vec<PathBuf>) -> Result<(String, Vec<String>)> {
        let assets = Self::assets_in(&self.top_dir);
        let bundled: Vec<(&str, &[u8])> = files.iter().filter_map(|(n, b)| Some((n.strip_prefix(crate::guides::ASSETS)?, b.as_ref()))).collect();
        // `guides`, else the first `guides-N` where each picture is missing or the same file.
        let fits = |dir: &Path| bundled.iter().all(|(n, b)| crate::vfs::read(dir.join(n)).map_or(true, |have| have == *b));
        let pictures = (1..).map(|n| if n == 1 { "guides".to_string() } else { format!("guides-{n}") }).find(|d| fits(&assets.join(d))).unwrap();
        let (from, to) = (crate::guides::ASSETS, format!(".assets/{pictures}/"));
        let renamed = from != to;
        let dir = self.free_name("", &file_name_for(title), None, None);
        let top = self.abs(&dir);
        crate::vfs::create_dir(&top)?;
        made.push(top.clone());
        for (n, b) in &bundled {
            let p = assets.join(&pictures).join(n);
            if p.fs_exists() { continue; }
            if let Some(up) = p.parent() { crate::vfs::create_dir_all(up)?; }
            write_atomic(&p, b)?;
            made.push(p);
        }
        // Where the guides go: every file but the metadata and the pictures.
        let pages: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_ref())).filter(|(n, _)| !n.starts_with('.')).collect();
        for (n, b) in &pages {
            let p = top.join(n);
            if let Some(up) = p.parent() { crate::vfs::create_dir_all(up)?; }
            match std::str::from_utf8(b) {
                Ok(text) if renamed && text.contains(from) => self.write_file(&p, text.replace(from, &to).as_bytes())?,
                _ => self.write_file(&p, b)?,
            }
        }
        // The 数据库 that name a picture (an attachment cell, an image of its own), here and among the pictures.
        #[cfg(not(target_family = "wasm"))]
        if renamed {
            for (n, _) in pages.iter().filter(|(n, _)| n.ends_with(".sqlite")) { crate::table::sqlite::replace_text(&top.join(n), from, &to)?; }
            for p in made.iter().filter(|p| p.extension().is_some_and(|e| e == "sqlite")) { crate::table::sqlite::replace_text(p, from, &to)?; }
        }
        // The folders and pages, in the guides' order: the bundle's top level is the new folder's.
        let mut rels: Vec<String> = vec![];
        for (n, _) in &pages {
            let mut up = parent_rel(n);
            while !up.is_empty() { let k = format!("{up}/"); if !rels.contains(&k) { rels.push(k); } up = parent_rel(up); }
            rels.push(n.to_string());
        }
        let order = bundle_order(files);
        let t = now();
        let mut ids = vec![uuid::Uuid::new_v4().to_string()];
        self.index.insert(ids[0].clone(), Meta { folder: Some(String::new()), rel: dir.clone(), created: t, modified: t, ..Default::default() });
        let mut groups: HashMap<&str, Vec<(&str, String)>> = HashMap::new();
        for r in &rels {
            let id = uuid::Uuid::new_v4().to_string();
            let plain = r.trim_end_matches('/');
            let folder = r.ends_with('/').then(String::new);
            self.index.insert(id.clone(), Meta { folder, rel: join_rel(&dir, plain), created: t, modified: t, ..Default::default() });
            groups.entry(parent_rel(plain)).or_default().push((r.as_str(), id.clone()));
            ids.push(id);
        }
        for list in groups.values_mut() {
            // By the bundle's keys; anything it does not place after, by name.
            list.sort_by_key(|(r, _)| (!order.contains_key(*r), order.get(*r).cloned(), r.to_string()));
            for (i, (_, id)) in list.iter().enumerate() { if let Some(m) = self.index.get_mut(id) { m.order = i as u64 + 1; } }
        }
        derive_tree(&mut self.index);
        self.save_index()?;
        Ok((ids[0].clone(), ids))
    }
}

/// The bundle's places (build.py's `order`, or `root_order` at its top), by path as
/// `put_guides` names them (a folder's ends in `/`): fractional keys, compared as text.
fn bundle_order(files: &[(&str, std::borrow::Cow<'_, [u8]>)]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (_, b) in files.iter().filter(|(n, _)| n.starts_with(".quiet/meta/") && !n.ends_with("/device.json")) {
        let Ok(shard) = serde_json::from_slice::<serde_json::Value>(b) else { continue };
        for f in shard["items"].as_object().into_iter().flat_map(|m| m.values()) {
            let (Some(path), Some(key)) = (f["path"][0].as_str(), f["order"][0].as_str().or_else(|| f["root_order"][0].as_str())) else { continue };
            out.insert(path.to_string(), key.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn bundle(chinese: bool) -> Vec<(&'static str, Cow<'static, [u8]>)> { crate::guides::files(chinese).into_iter().map(|(n, b)| (n, Cow::Borrowed(b))).collect() }

    fn library() -> PathBuf {
        let root = crate::vfs::temp_dir().join(format!("qmd-guide-import-{}", uuid::Uuid::new_v4()));
        crate::vfs::create_dir_all(&root).unwrap();
        Store::open_at(root.clone()).unwrap();
        root
    }

    /// Imported into a library that has pages of its own, twice: two folders beside them,
    /// the second numbered, every link of each (mentions, 笔记 cells, pictures and
    /// attachments) reaching a file of its own copy, as a fresh opening reads them.
    #[test]
    fn imported_guides_link_within_their_folder() {
        for chinese in [false, true] {
            let root = library();
            let mut s = Store::open_at(root.clone()).unwrap();
            let (mine, _) = s.create().unwrap();
            let name = crate::guides::folder_name(chinese);
            let a = s.import_guides(&bundle(chinese), name).unwrap();
            let b = s.import_guides(&bundle(chinese), name).unwrap();
            let mut s = Store::open_at(root.clone()).unwrap();
            let list = s.list();
            assert!(s.warnings.is_empty(), "{:?}", s.warnings);
            assert!(list.iter().any(|e| e.id == mine));
            let top: Vec<_> = list.iter().filter(|e| e.meta.parent.is_none() && e.meta.folder.is_some()).map(|e| (e.id.clone(), e.meta.rel.clone())).collect();
            assert!(top.contains(&(a.clone(), name.to_string())) && top.contains(&(b.clone(), format!("{name} (2)"))), "{top:?}");
            // The same pictures twice: one copy of them.
            assert!(!root.join(".assets/guides-2").fs_exists());
            // The newest import first at the top, every entry there placed.
            let mut tops: Vec<&Entry> = list.iter().filter(|e| e.meta.parent.is_none()).collect();
            tops.sort_by_key(|e| e.meta.top_order);
            assert!(tops.iter().all(|e| e.meta.top_order.is_some()) && tops[0].id == b, "{:?}", tops.iter().map(|e| (&e.title, e.meta.top_order)).collect::<Vec<_>>());
            for folder in [&a, &b] { check_folder(&mut s, &list, folder, chinese, "guides"); }
            crate::vfs::remove_dir_all(&root).unwrap();
        }
    }

    /// What `folder` holds: every guide, in the bundle's order, each link resolved
    /// inside it; pictures and attachments under `.assets/<pictures>/`.
    fn check_folder(s: &mut Store, list: &[Entry], folder: &str, chinese: bool, pictures: &str) {
        let rel = list.iter().find(|e| e.id == folder).unwrap().meta.rel.clone();
        let inside = |e: &&Entry| e.meta.rel.starts_with(&format!("{rel}/"));
        let docs: Vec<&Entry> = list.iter().filter(inside).filter(|e| e.meta.folder.is_none()).collect();
        let bundle = bundle(chinese);
        assert_eq!(docs.len(), bundle.iter().filter(|(n, _)| !n.starts_with('.') && crate::store::DocKind::of_name(n).is_some()).count());
        // The new folder's own order is the bundle's top level: Welcome, then the folders.
        let mut kids: Vec<&Entry> = list.iter().filter(|e| e.meta.parent.as_deref() == Some(folder)).collect();
        kids.sort_by_key(|e| e.meta.order);
        assert_eq!(kids[0].title, if chinese { "欢迎使用 Quiet.md" } else { "Welcome to Quiet.md" });
        assert!(kids[1..].iter().all(|e| e.meta.folder.is_some()) && kids.len() > 3);
        let ids: HashSet<&str> = list.iter().filter(inside).map(|e| e.id.as_str()).collect();
        let links: Vec<_> = docs.iter().flat_map(|e| e.links.iter()).collect();
        assert!(links.len() > 10);
        for l in links { assert!(ids.contains(l.target.as_str()), "{}: link to {} leaves the folder", rel, l.target); }
        let top = s.top_dir().to_path_buf();
        let mut pics = 0;
        for e in &docs {
            let path = s.file_path(&e.id).unwrap();
            let text = if e.meta.kind == crate::store::DocKind::Table { String::new() } else { crate::vfs::read_to_string(&path).unwrap() };
            // Every picture, attachment and cover it names, where it says.
            for part in text.split("\".assets/").skip(1) {
                let end = part.find('"').unwrap();
                let at = format!(".assets/{}", &part[..end]);
                assert!(at.starts_with(&format!(".assets/{pictures}/")), "{}: {at}", e.meta.rel);
                assert!(crate::store::image_path_in(&top, &at).is_some_and(|p| p.fs_is_file()), "{}: {at} is missing", e.meta.rel);
                pics += 1;
            }
            // The 数据库's 笔记 cells: a guide in this folder.
            #[cfg(not(target_family = "wasm"))]
            if e.meta.kind == crate::store::DocKind::Table {
                use crate::table::{field, FieldType, Value};
                let base = crate::table::sqlite::load(&path).unwrap();
                for t in &base.tables { for f in t.fields.iter().filter(|f| f.ty == FieldType::Note) { for r in &t.records {
                    let Value::Text(href) = r.get(f.id) else { continue };
                    let to = field::note_rel(&e.meta.rel, href);
                    assert!(docs.iter().any(|d| Some(&d.meta.rel) == to.as_ref()), "{}: {href}", e.meta.rel);
                } } }
            }
        }
        assert!(pics > 5, "{pics}");
        // Opening the guides' own pages names them in this folder too.
        for e in docs.iter().filter(|e| e.meta.kind.is_doc() && !e.meta.markdown) { s.load(&e.id).unwrap(); }
    }

    /// A library whose guide pictures were changed (an older first launch's, a 数据库
    /// edited): nothing of it overwritten, the new guides' pictures beside it in
    /// `.assets/guides-2/`, and their pages (the 数据库 and 画板 too) naming those.
    #[test]
    fn other_pictures_of_the_same_name_stay() {
        let root = library();
        let old = root.join(".assets/guides/home.jpg");
        crate::vfs::create_dir_all(old.parent().unwrap()).unwrap();
        crate::vfs::write(&old, b"an older picture").unwrap();
        let mut s = Store::open_at(root.clone()).unwrap();
        let a = s.import_guides(&bundle(false), "Quiet.md Guide").unwrap();
        assert_eq!(crate::vfs::read(&old).unwrap(), b"an older picture");
        let mut s = Store::open_at(root.clone()).unwrap();
        let list = s.list();
        check_folder(&mut s, &list, &a, false, "guides-2");
        let all = |dir: &Path| { let mut v = vec![]; fn walk(d: &Path, v: &mut Vec<PathBuf>) { for e in crate::vfs::read_dir(d).unwrap().flatten() { let p = e.path(); if p.fs_is_dir() { walk(&p, v) } else { v.push(p) } } } walk(dir, &mut v); v };
        // Nothing the import wrote still names `.assets/guides/` (a 数据库's attachment, the 画板's picture).
        for p in all(&root.join("Quiet.md Guide")).into_iter().chain(all(&root.join(".assets/guides-2"))) {
            let bytes = crate::vfs::read(&p).unwrap();
            assert!(!bytes.windows(15).any(|w| w == b".assets/guides/"), "{} names the old pictures", p.display());
        }
        crate::vfs::remove_dir_all(&root).unwrap();
    }

    /// 撤销 sends the whole import to the 回收站 as one step, and 重做 brings it back.
    #[test]
    fn an_import_undoes_and_redoes() {
        let root = library();
        let mut s = Store::open_at(root.clone()).unwrap();
        let journal = s.track_file_changes();
        let a = s.import_guides(&bundle(false), "Quiet.md Guide").unwrap();
        let changes = journal.take();
        assert!(changes.len() > 15 && changes.iter().all(|c| c.before.is_none() && c.after.is_some()), "{}", changes.len());
        s.list();
        s.apply_file_history(&changes, false).unwrap();
        assert!(s.list().iter().all(|e| !e.meta.rel.starts_with("Quiet.md Guide")));
        assert!(!root.join("Quiet.md Guide").fs_exists());
        s.apply_file_history(&changes, true).unwrap();
        let list = s.list();
        check_folder(&mut s, &list, &a, false, "guides");
        // Imported again after an undo: other ids, the trash's left alone.
        s.apply_file_history(&changes, false).unwrap();
        let b = s.import_guides(&bundle(false), "Quiet.md Guide").unwrap();
        assert_ne!(a, b);
        let list = s.list();
        check_folder(&mut s, &list, &b, false, "guides");
        crate::vfs::remove_dir_all(&root).unwrap();
    }
}
