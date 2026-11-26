//! A page or folder moved into another top-level folder: in the Finder, a
//! file put somewhere else. Each top-level folder is a library of its own
//! (its `.quiet/`, its index), so the move is one library handing entries to
//! another: the files go over (renamed on the same disk, else copied and the
//! originals removed), the entries keep their ids and everything the index
//! says about them (pins, order, 收藏, 页宽 …), and the source writes them
//! off the way a deletion is written (FORMAT 6: its fields go, which the
//! other devices read as gone).
//!
//! Why the same ids: ids are UUIDs, unique across top-level folders (W14),
//! and nothing in a file names them except a mention across top-level
//! folders, which is written by id (`doc-id`) because no relative path in
//! one library reaches the other. The mentions of what moved from pages left
//! behind become such mentions, and keep working; so do the window's rows,
//! its open document, its undo and anything another top-level folder says
//! about the page by id.
//!
//! What crosses the edge is rewritten as `Store::relocate` does inside one
//! library: pictures and attachments the pages keep in the source (its
//! `.assets/`, or anywhere in it but what moves) are copied into the
//! target's `.assets/`; plain links to files and 数据表 笔记 cells that leave
//! the moved folder are written as paths from where they are now.
//!
//! The order keeps both libraries whole at every step (a crash leaves the
//! page in one of them, at worst in both, never half of it): a copy is made
//! under the target's `.quiet/moving/` first, out of every listing; the
//! target's index names the entries before their files arrive (an entry
//! without its file is dropped by the next `list`); the files arrive by one
//! rename; only then does the source let go of them. Whatever is left in
//! `.quiet/moving/` afterwards is a copy the move no longer needs, and the
//! next move clears it.

use super::*;

/// Where what moves goes among its new siblings: before one (`None`: last),
/// or at the numbers it had there (撤销 and 重做 put it back so).
#[derive(Clone, Debug, PartialEq)]
pub enum Place { Before(Option<String>), At(u64, Option<u64>) }

#[cfg(test)]
thread_local! {
    /// Tests: the two libraries are on different disks.
    static OTHER_DISK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Tests: the process dies at this step (1: the target's index written, 2: the files arrived).
    static DIE_AT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// Tests: the process dies here when `DIE_AT` says so (nothing after it runs), or
/// (`DIE_AT` 10 more) the step fails.
fn die_at(_step: u8) -> Result<()> {
    #[cfg(test)]
    match DIE_AT.with(|d| d.get()) {
        n if n == _step => panic!("died at step {_step}"),
        n if n == _step + 10 => anyhow::bail!("failed at step {_step}"),
        _ => {}
    }
    Ok(())
}

/// Whether `e` says the two paths are on different disks (a rename cannot move between them).
fn other_disk(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    if e.raw_os_error() == Some(18) { return true; }
    #[cfg(windows)]
    if e.raw_os_error() == Some(17) { return true; }
    e.kind() == std::io::ErrorKind::CrossesDevices
}

/// `from` (a file or a folder) copied to `to`, which is not there yet; each file synced.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.fs_is_dir() {
        crate::vfs::create_dir_all(to)?;
        for e in crate::vfs::read_dir(from)? { let e = e?; copy_tree(&e.path(), &to.join(e.file_name()))?; }
    } else {
        crate::vfs::copy(from, to)?;
        // Opened for writing: Windows flushes no file through a read-only handle.
        crate::vfs::OpenOptions::new().write(true).open(to).and_then(|f| f.sync_all())?;
    }
    Ok(())
}

/// `path` written the way `href` was: escaped when it was, its `#…` / `?…` kept (`layout::rebase_link`).
fn written_like(href: &str, mut path: String) -> String {
    let at = href.find(['#', '?']).unwrap_or(href.len());
    if href[..at].contains('%') { path = path.replace('%', "%25").replace(' ', "%20").replace('(', "%28").replace(')', "%29"); }
    path.push_str(&href[at..]);
    path
}

fn remove_tree(p: &Path) -> std::io::Result<()> { if p.fs_is_dir() { crate::vfs::remove_dir_all(p) } else { crate::vfs::remove_file(p) } }

impl Store {
    fn moving_dir(&self) -> PathBuf { self.internal().join("moving") }

    /// What an earlier move left in `.quiet/moving/` (see the top of this file): never needed again.
    fn clear_moving(&self) {
        let Ok(list) = crate::vfs::read_dir(self.moving_dir()) else { return };
        for e in list.flatten() { let _ = remove_tree(&e.path()); }
    }

    /// `from`, a file in another library (`src`), as a picture of this one: in its
    /// `.assets/` under the same name, another when that one holds something else.
    fn copy_asset(&self, from: &Path, made: &mut HashMap<PathBuf, PathBuf>) -> Option<PathBuf> {
        if let Some(p) = made.get(from) { return Some(p.clone()); }
        let bytes = crate::vfs::read(from).ok()?;
        let dir = Self::assets_in(&self.top_dir);
        crate::vfs::create_dir_all(&dir).ok()?;
        let name = from.file_name()?.to_string_lossy().into_owned();
        let mut to = dir.join(&name);
        if to.fs_exists() && crate::vfs::read(&to).ok().as_deref() != Some(bytes.as_slice()) {
            let ext = Path::new(&name).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            to = dir.join(format!("{}{ext}", uuid::Uuid::new_v4()));
        }
        if !to.fs_exists() { write_atomic(&to, &bytes).ok()?; }
        made.insert(from.to_path_buf(), to.clone());
        Some(to)
    }

    /// Move `id` (a page, or a folder with all it holds) out of this library into folder
    /// `parent` of `to` (`None`: its top), placed as `place` says. Its name there follows
    /// the rules of a move inside one library (numbered when taken).
    pub fn move_across(&mut self, to: &mut Store, id: &str, parent: Option<&str>, place: Place) -> Result<()> {
        anyhow::ensure!(self.top_dir != to.top_dir, "已在这个文件夹里");
        // Two libraries locked in one order on every computer and window, so two moves the other way never wait on each other.
        let (_a, _b) = if self.top_dir < to.top_dir { let a = self.begin_write()?; (a, to.begin_write()?) } else { let b = to.begin_write()?; (self.begin_write()?, b) };
        anyhow::ensure!(!self.flat && !to.flat, "旧格式的文档库没有真实文件夹");
        anyhow::ensure!(self.index_has(id), "文档或文件夹不存在");
        let m = self.index[id].clone();
        anyhow::ensure!(!m.local(), "本机文件不能移入文档库的文件夹");
        to.validate_folder(None, parent)?;
        if let Place::Before(Some(anchor)) = &place {
            anyhow::ensure!(to.index.get(anchor).is_some_and(|m| m.parent.as_deref() == parent) && to.index_has(anchor), "目标位置已改变，请重试");
        }
        self.clear_moving();
        to.clear_moving();
        // What goes: `id`, and for a folder all that is in it (its files of kinds this build does not know too).
        let prefix = format!("{}/", m.rel);
        let ids: Vec<String> = self.index.iter().filter(|(k, c)| *k == id || (m.folder.is_some() && !c.local() && c.rel.starts_with(&prefix))).map(|(k, _)| k.clone()).collect();
        anyhow::ensure!(ids.iter().all(|k| !to.index.contains_key(k)), "目标文件夹里已有相同编号的文档");
        let rel = to.rel_for(&m, parent);
        let new_rel: HashMap<String, String> = ids.iter().map(|k| {
            let old = &self.index[k].rel;
            (k.clone(), if k == id { rel.clone() } else { format!("{rel}/{}", &old[prefix.len()..]) })
        }).collect();
        let foreign: Vec<(String, Attached)> = self.foreign.iter().filter(|(k, _)| m.folder.is_some() && k.starts_with(&prefix)).map(|(k, a)| (format!("{rel}/{}", &k[prefix.len()..]), a.clone())).collect();
        // From the source's top: where what moved went, in the target (also a file in a moved folder that no entry names).
        let moved = |p: &str| if p == m.rel { Some(rel.clone()) } else if m.folder.is_some() { p.strip_prefix(&prefix).map(|r| format!("{rel}/{r}")) } else { None };
        let (src_top, dst_top) = (self.top_dir.clone(), to.top_dir.clone());

        // The pages that move, read here (links as ids, pictures as paths on this computer) to be written there.
        let docs: Vec<(String, String, Document)> = ids.iter().filter(|k| { let c = &self.index[*k]; c.folder.is_none() && c.kind.is_doc() })
            .filter_map(|k| { let text = crate::vfs::read_to_string(self.path(k)).ok()?; let doc = self.parse_doc(k, &text).ok()?; Some((k.clone(), text, doc)) }).collect();
        // The pages that stay and mention or link to what moves.
        let staying: Vec<(String, String, Document)> = self.index.iter()
            .filter(|(k, c)| !new_rel.contains_key(*k) && c.folder.is_none() && !c.local() && c.kind.is_doc())
            .filter(|(k, c)| {
                let dir = parent_rel(&c.rel);
                self.parsed.get(*k).map(|p| &p.2).is_none_or(|p| p.links.iter().any(|l| new_rel.contains_key(&l.target)) || p.files.iter().any(|h| layout::link_path(dir, h).is_some_and(|t| moved(&t).is_some())))
            })
            .filter_map(|(k, _)| { let text = crate::vfs::read_to_string(self.path(k)).ok()?; let doc = self.parse_doc(k, &text).ok()?; Some((k.clone(), text, doc)) }).collect();

        // The target's index names them from here on (in memory; on disk below).
        let previous = to.index.clone();
        let t = now();
        for k in &ids {
            let mut c = self.index[k].clone();
            c.rel = new_rel[k].clone();
            c.modified = c.modified.max(t);
            to.index.insert(k.clone(), c);
        }
        for (k, a) in &foreign { to.foreign.insert(k.clone(), a.clone()); }
        derive_tree(&mut to.index);
        // Among its new siblings, in the sidebar's order (as `move_in_tree` numbers them).
        let numbers = |s: &Store| -> HashMap<String, (u64, Option<u64>)> { s.index.iter().filter(|(k, c)| c.parent.as_deref() == parent && *k != id).map(|(k, c)| (k.clone(), (c.order, c.top_order))).collect() };
        let was_numbered = numbers(to);
        match &place {
            Place::At(order, top_order) => { let c = to.index.get_mut(id).unwrap(); (c.order, c.top_order) = (*order, *top_order); }
            Place::Before(before) => {
                let mut siblings: Vec<String> = to.index.iter().filter(|(k, c)| c.parent.as_deref() == parent && *k != id).map(|(k, _)| k.clone()).collect();
                siblings.sort_by(|a, b| (to.index[a].place(), a).cmp(&(to.index[b].place(), b)));
                let pos = before.as_ref().and_then(|b| siblings.iter().position(|x| x == b)).unwrap_or(siblings.len());
                siblings.insert(pos, id.to_string());
                for (i, k) in siblings.iter().enumerate() {
                    let c = to.index.get_mut(k).unwrap();
                    if parent.is_none() { c.top_order = Some(i as u64 + 1); } else { c.order = i as u64 + 1; c.top_order = None; }
                }
            }
        }
        // Each moved page as the target writes it: mentions of what moved along as paths, of what
        // stayed by id; pictures from the source copied over; file links from where it is now.
        let mut assets: HashMap<PathBuf, PathBuf> = HashMap::new();
        let mut written: Vec<(String, String)> = vec![];
        for (k, text, mut doc) in docs {
            let (was, now_dir) = (parent_rel(&self.index[&k].rel).to_string(), parent_rel(&new_rel[&k]).to_string());
            let mut changed = doc.map_links(&mut |h| {
                let target = layout::link_path(&was, h)?;
                if moved(&target).is_some() { return layout::rebase_link(&was, &now_dir, h, &moved); }
                crate::paths::portable(&src_top.join(&target), &dst_top.join(&now_dir)).map(|p| written_like(h, p)).filter(|n| n != h)
            });
            // A Markdown page names its pictures from itself (those in the moved folder read the
            // same), a DocxXML page from the library's top.
            let markdown = self.is_markdown(&k);
            doc.map_sources(|src| {
                let at = Path::new(src);
                if !at.fs_is_absolute() { return None }
                let rel = crate::paths::portable_in(at, &src_top)?;
                let new = match moved(&rel) { Some(r) => { changed |= !markdown && r != rel; dst_top.join(r) } None => { changed = true; to.copy_asset(at, &mut assets)? } };
                Some(new.to_string_lossy().into_owned())
            });
            changed |= doc.doc_links().iter().any(|l| self.index.contains_key(&l.target));
            if !changed { continue; }
            let out = to.serialize_doc(&k, &doc, to.is_markdown(&k).then_some(text.as_str()));
            if out != text { written.push((k, out)); }
        }
        let undo = |to: &mut Store| { to.index = previous.clone(); for (k, _) in &foreign { to.foreign.remove(k); } };

        // A copy first when the target is on another disk: in its `.quiet/moving/`, listed nowhere.
        let (from, dest) = (self.abs(&m.rel), to.abs(&rel));
        let token = uuid::Uuid::new_v4().to_string();
        let staged = to.moving_dir().join(&token).join(base_name(&rel));
        let mut same_disk = {
            #[cfg(test)]
            let forced = OTHER_DISK.with(|o| o.get());
            #[cfg(not(test))]
            let forced = false;
            !forced && match (crate::vfs::metadata(&from), crate::vfs::metadata(&dst_top)) {
                #[cfg(unix)]
                (Ok(a), Ok(b)) => { use std::os::unix::fs::MetadataExt; a.dev() == b.dev() }
                _ => true,
            }
        };
        let stage = |staged: &Path| -> Result<()> { crate::vfs::create_dir_all(staged.parent().unwrap())?; copy_tree(&from, staged) };
        let r = (|| -> Result<()> {
            if let Some(p) = dest.parent() { crate::vfs::create_dir_all(p)?; }
            anyhow::ensure!(dest.fs_symlink_metadata().is_err(), "已存在同名文件：{rel}");
            if !same_disk { stage(&staged)?; }
            to.save_index()
        })();
        if let Err(e) = r { undo(to); let _ = remove_tree(&to.moving_dir().join(&token)); return Err(e); }
        // The files arrive in one step.
        let arrived = die_at(1).and_then(|_| match same_disk {
            true => match crate::vfs::rename(&from, &dest) {
                // Told apart only now (a disk mounted inside another): copied after all.
                Err(e) if other_disk(&e) => { same_disk = false; stage(&staged).and_then(|_| Ok(crate::vfs::rename(&staged, &dest)?)) }
                r => r.map_err(Into::into),
            },
            false => crate::vfs::rename(&staged, &dest).map_err(Into::into),
        });
        if let Err(e) = arrived {
            undo(to);
            let _ = to.save_index();
            let _ = remove_tree(&to.moving_dir().join(&token));
            return Err(e);
        }
        let _ = die_at(2);
        let _ = crate::vfs::remove_dir(to.moving_dir().join(&token));
        if let Some(dir) = dest.parent() && let Ok(f) = crate::vfs::File::open(dir) { let _ = f.sync_all(); }
        for (k, out) in &written {
            if let Err(e) = write_atomic(&to.path(k), out.as_bytes()) { to.warnings.push(format!("未能更新文档中的链接（{}）：{e}", new_rel[k])); }
        }
        // 数据表's 笔记 cells, paths from the table's file: those of a moved table that leave
        // the moved folder, and those of a table left behind that point into it.
        #[cfg(not(target_family = "wasm"))]
        for (k, now) in new_rel.iter().filter(|(k, _)| to.index.get(*k).is_some_and(|c| c.folder.is_none() && c.kind == DocKind::Table)) {
            let (was, now_dir) = (parent_rel(&self.index[k].rel).to_string(), parent_rel(now).to_string());
            let r = crate::table::sqlite::rewrite_notes(&to.abs(now), |href| {
                let target = layout::resolve(&was, href)?;
                let new = match moved(&target) { Some(r) => layout::relative(&now_dir, &r), None => crate::paths::portable(&src_top.join(&target), &dst_top.join(&now_dir))? };
                (new != href).then_some(new)
            });
            if let Err(e) = r { to.warnings.push(format!("未能更新数据表中的笔记路径（{now}）：{e}")); }
        }
        // 版本历史 goes along, by id.
        for k in &ids { copy_history(&self.history_base(), &to.history_base(), k); }

        // The source lets go: its entries go (written as a deletion is), its pages
        // that pointed at what moved point there now.
        let gone: HashMap<String, Meta> = ids.iter().filter_map(|k| Some((k.clone(), self.index.remove(k)?))).collect();
        self.foreign.retain(|k, _| m.folder.is_none() || !k.starts_with(&prefix));
        for k in &ids { self.parsed.remove(k); self.loaded.borrow_mut().remove(k); }
        if !same_disk {
            // The originals out of every listing first (one rename), then removed.
            let out = self.moving_dir().join(&token);
            let away = crate::vfs::create_dir_all(&out).and_then(|_| crate::vfs::rename(&from, out.join(base_name(&m.rel))));
            match away {
                Ok(()) => { let _ = crate::vfs::remove_dir_all(&out); }
                Err(e) => self.warnings.push(format!("已移到另一个文件夹，未能删除原来的文件（{}）：{e}", m.rel)),
            }
        }
        for (k, text, mut doc) in staying {
            let dir = parent_rel(&self.index[&k].rel).to_string();
            let mut changed = doc.map_links(&mut |h| {
                let target = layout::link_path(&dir, h)?;
                let r = moved(&target)?;
                crate::paths::portable(&dst_top.join(r), &src_top.join(&dir)).map(|p| written_like(h, p)).filter(|n| n != h)
            });
            // Pictures it kept in the moved folder (a DocxXML page names them from the top): where they are now.
            doc.map_sources(|src| {
                let at = Path::new(src);
                let r = crate::paths::portable_in(at, &src_top).and_then(|rel| moved(&rel))?;
                changed = true;
                Some(dst_top.join(r).to_string_lossy().into_owned())
            });
            changed |= doc.doc_links().iter().any(|l| gone.contains_key(&l.target));
            if !changed { continue; }
            let out = self.serialize_doc(&k, &doc, self.is_markdown(&k).then_some(text.as_str()));
            if out == text { continue; }
            match write_atomic(&self.path(&k), out.as_bytes()) {
                Ok(()) => {
                    let mut loaded = self.loaded.borrow_mut();
                    if loaded.get(&k).is_some_and(|l| *l == text) { loaded.insert(k.clone(), out); }
                    self.parsed.remove(&k);
                }
                Err(e) => self.warnings.push(format!("未能更新文档中的链接（{}）：{e}", self.index[&k].rel)),
            }
        }
        #[cfg(not(target_family = "wasm"))]
        {
            let tables: Vec<String> = self.index.values().filter(|c| c.folder.is_none() && !c.local() && c.kind == DocKind::Table).map(|c| c.rel.clone()).collect();
            for now in tables {
                let dir = parent_rel(&now).to_string();
                let r = crate::table::sqlite::rewrite_notes(&self.abs(&now), |href| {
                    let r = moved(&layout::resolve(&dir, href)?)?;
                    crate::paths::portable(&dst_top.join(r), &src_top.join(&dir)).filter(|n| n != href)
                });
                if let Err(e) = r { self.warnings.push(format!("未能更新数据表中的笔记路径（{now}）：{e}")); }
            }
        }
        // The page is in the target already: an index not written here only lists it until the next `list` finds its file gone.
        if let Err(e) = self.save_index() { self.warnings.push(format!("已移到另一个文件夹，原文件夹的索引未能更新：{e}")); }
        for k in &ids { remove_history(&self.history_base(), k); }
        if self.file_changes.iter().any(|w| w.strong_count() > 0) || to.file_changes.iter().any(|w| w.strong_count() > 0) {
            // The move as one change of `id`'s place; what is inside a moved folder stays where it is in it.
            let before = gone.get(id).and_then(|g| to.file_state(id).map(|mut s| { (s.parent, s.order, s.top_order) = (g.parent.clone(), g.order, g.top_order); s }));
            to.record_file_change(id, before);
            for (k, _) in numbers(to).into_iter().filter(|(k, n)| was_numbered.get(k) != Some(n)) {
                let (order, top_order) = was_numbered[&k];
                let before = to.file_state(&k).map(|mut s| { (s.order, s.top_order) = (order, top_order); s });
                to.record_file_change(&k, before);
            }
        }
        Ok(())
    }

    /// The open editor's baseline of `id` (`load_for_edit`), handed to the library it moved to.
    pub fn take_loaded(&self, id: &str) -> Option<String> { self.loaded.borrow_mut().remove(id) }
    /// `take_loaded`'s other end: `text`, or (`None`) what its file holds now.
    pub fn give_loaded(&self, id: &str, text: Option<String>) {
        if let Some(t) = text.or_else(|| self.read_doc_text(id).ok()) { self.loaded.borrow_mut().insert(id.to_string(), t); }
    }

    /// Where `m` (an entry of this library or another's) would go in folder `parent`: its name
    /// there, numbered when taken.
    pub(super) fn rel_for(&self, m: &Meta, parent: Option<&str>) -> String {
        let dir = parent.and_then(|p| self.index.get(p)).map(|p| p.rel.clone()).unwrap_or_default();
        let name = base_name(&m.rel);
        let ext = m.folder.is_none().then(|| ext_of(m));
        let stem = if ext.is_some() { stem_of(name) } else { name };
        // A folder's or Markdown file's name is its title: 「报告 (2).md」 stays that.
        let base = if m.folder.is_some() || m.named_by_file() { stem } else { layout::unnumbered(stem) };
        join_rel(&dir, &self.free_name(&dir, base, ext, None))
    }
}

/// `id`'s 版本历史 (`.quiet/history/<id>/`) into another library's, beside what is there.
fn copy_history(from: &Path, to: &Path, id: &str) {
    let (a, b) = (from.join("history").join(id), to.join("history").join(id));
    let Ok(list) = crate::vfs::read_dir(&a) else { return };
    if crate::vfs::create_dir_all(&b).is_err() { return }
    for e in list.flatten() {
        let dest = b.join(e.file_name());
        if !dest.fs_exists() && crate::vfs::rename(e.path(), &dest).is_err() { let _ = crate::vfs::copy(e.path(), &dest); }
    }
}

fn remove_history(from: &Path, id: &str) { let _ = crate::vfs::remove_dir_all(from.join("history").join(id)); }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, BlockKind, InlineDocLink, InlineKind, Marks, RichText};

    /// A library in a fresh temporary folder, removed on drop.
    struct Lib(PathBuf);
    impl Lib {
        fn new(name: &str) -> Self { Lib(crate::vfs::temp_dir().join(format!("fdl-across-{name}-{}", uuid::Uuid::new_v4()))) }
        fn open(&self) -> Store { Store::open_at(self.0.clone()).unwrap() }
        fn read(&self, rel: &str) -> String { crate::vfs::read_to_string(self.0.join(rel)).unwrap() }
    }
    impl Drop for Lib {
        fn drop(&mut self) {
            let _ = crate::vfs::remove_dir_all(&self.0);
            let _ = crate::vfs::remove_file(super::parsed_cache_path(&self.0, &library_key(&self.0)));
        }
    }

    fn doc(title: &str) -> Document { Document { title: RichText::plain(title), ..Default::default() } }
    fn mention(d: &mut Document, id: &str, title: &str) {
        let mut b = Block::text("");
        b.text = RichText::plain("见 ");
        b.text.push("\u{fffc}", Marks { inline: Some(docxml::to_inline(InlineKind::DocLink(InlineDocLink { id: id.into(), title: title.into() }))), ..Default::default() });
        d.blocks.push(b);
    }
    fn picture(d: &mut Document, src: &Path) {
        d.blocks.push(Block::new(BlockKind::Image { src_kind: "src".into(), src: src.to_string_lossy().into_owned(), width: None, height: None, caption: None, crop: None }, Default::default()));
    }
    fn entry(s: &mut Store, id: &str) -> Option<Entry> { s.list().into_iter().find(|e| e.id == id) }
    fn moving_left(l: &Lib) -> usize { crate::vfs::read_dir(l.0.join(".quiet/moving")).map(|d| d.count()).unwrap_or(0) }

    /// A folder with a page and a folder in it, from one top-level folder to the top of
    /// another: the files go, the ids and what the index says stay, pictures come along,
    /// and mentions across the edge keep finding their pages (by id).
    fn a_folder_moves(other_disk: bool) {
        OTHER_DISK.with(|o| o.set(other_disk));
        let (la, lb) = (Lib::new("a"), Lib::new("b"));
        let (mut a, mut b) = (la.open(), lb.open());
        let stay = a.create_from(&doc("留下")).unwrap();
        let f = a.create_folder("项目", None).unwrap();
        let g = a.create_folder("子", Some(&f)).unwrap();
        let pic = Store::assets_in(a.top_dir()).join("pic.png");
        crate::vfs::create_dir_all(pic.parent().unwrap()).unwrap();
        crate::vfs::write(&pic, b"png").unwrap();
        let mut d1 = doc("甲");
        picture(&mut d1, &pic);
        let p1 = a.create_from_in(&d1, Some(&f)).unwrap();
        let mut d2 = doc("乙");
        mention(&mut d2, &p1, "甲");
        mention(&mut d2, &stay, "留下");
        let p2 = a.create_from_in(&d2, Some(&g)).unwrap();
        let mut s = doc("留下");
        mention(&mut s, &p1, "甲");
        a.save(&stay, &s).unwrap();
        a.set_pinned(&p1, true).unwrap();
        a.set_starred(&f, true).unwrap();
        crate::versions::snapshot(&a.history_base(), &p1, "xml", "old", 1, 10, false).unwrap();
        let created = entry(&mut a, &p1).unwrap().meta.created;
        a.list();
        b.list();

        a.move_across(&mut b, &f, None, Place::Before(None)).unwrap();
        assert!(lb.0.join("项目/甲.xml").fs_is_file() && lb.0.join("项目/子/乙.xml").fs_is_file());
        assert!(!la.0.join("项目").fs_exists());
        assert_eq!(moving_left(&la) + moving_left(&lb), 0);
        // Read again from the disk: each library as the move left it.
        let (mut a, mut b) = (la.open(), lb.open());
        assert!(entry(&mut a, &p1).is_none() && entry(&mut a, &f).is_none() && entry(&mut a, &p2).is_none());
        let e1 = entry(&mut b, &p1).unwrap();
        assert!(e1.meta.pinned && e1.meta.created == created, "the index's fields come along");
        assert!(entry(&mut b, &f).unwrap().meta.starred > 0);
        assert_eq!(entry(&mut b, &p2).unwrap().meta.parent.as_deref(), Some(g.as_str()));
        // The picture is the target's own now; the source keeps its copy (other pages may use it).
        assert!(lb.read("项目/甲.xml").contains("src=\".assets/pic.png\""));
        assert!(lb.0.join(".assets/pic.png").fs_is_file() && pic.fs_is_file());
        // Mentions inside the moved folder stay paths; across top-level folders they are ids.
        let t2 = lb.read("项目/子/乙.xml");
        assert!(t2.contains("href=\"../甲.xml\"") && t2.contains(&format!("doc-id=\"{stay}\"")), "{t2}");
        assert!(la.read("留下.xml").contains(&format!("doc-id=\"{p1}\"")));
        let links: Vec<String> = entry(&mut b, &p2).unwrap().links.iter().map(|l| l.target.clone()).collect();
        assert!(links.contains(&p1) && links.contains(&stay));
        assert_eq!(b.load(&p2).unwrap().doc_links().len(), 2);
        // 版本历史 follows by id.
        assert_eq!(b.versions(&p1).len(), 1);
        assert!(a.versions(&p1).is_empty());
        OTHER_DISK.with(|o| o.set(false));
    }

    #[test]
    fn a_folder_moves_with_all_it_holds() { a_folder_moves(false) }

    #[test]
    fn on_another_disk_it_is_copied_then_removed() { a_folder_moves(true) }

    #[test]
    fn a_taken_name_is_numbered_and_the_place_is_kept() {
        let (la, lb) = (Lib::new("a"), Lib::new("b"));
        let (mut a, mut b) = (la.open(), lb.open());
        let f = a.create_folder("资料", None).unwrap();
        let p = a.create_from(&doc("笔记")).unwrap();
        let into = b.create_folder("收件", None).unwrap();
        b.create_folder("资料", Some(&into)).unwrap();
        let first = b.create_from_in(&doc("笔记"), Some(&into)).unwrap();
        let last = b.create_from_in(&doc("别的"), Some(&into)).unwrap();
        a.move_across(&mut b, &f, Some(&into), Place::Before(None)).unwrap();
        a.move_across(&mut b, &p, Some(&into), Place::Before(Some(last.clone()))).unwrap();
        assert!(lb.0.join("收件/资料 (2)").fs_is_dir() && lb.0.join("收件/笔记 (2).xml").fs_is_file());
        let mut b = lb.open();
        let list = b.list();
        let mut kids: Vec<&Entry> = list.iter().filter(|e| e.meta.parent.as_deref() == Some(into.as_str())).collect();
        kids.sort_by_key(|e| e.meta.place());
        let order: Vec<&str> = kids.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(order.iter().position(|k| *k == p).unwrap() + 1, order.iter().position(|k| *k == last).unwrap(), "before the anchor: {order:?}");
        assert!(order.contains(&first.as_str()));
        // Into itself is the target's business: only folders of the target are folders there.
        assert!(a.move_across(&mut b, &f, Some(&f), Place::Before(None)).is_err());
    }

    /// 撤销: the move back, to the folder and numbers it had.
    #[test]
    fn moving_back_restores_the_place() {
        let (la, lb) = (Lib::new("a"), Lib::new("b"));
        let (mut a, mut b) = (la.open(), lb.open());
        let f = a.create_folder("夹", None).unwrap();
        let p = a.create_from_in(&doc("页"), Some(&f)).unwrap();
        a.create_from_in(&doc("另一页"), Some(&f)).unwrap();
        let was = entry(&mut a, &p).unwrap().meta;
        let journal = b.track_file_changes();
        a.move_across(&mut b, &p, None, Place::Before(None)).unwrap();
        let changes = journal.take();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].before.as_ref().unwrap().parent.as_deref(), Some(f.as_str()));
        b.move_across(&mut a, &p, Some(&f), Place::At(was.order, was.top_order)).unwrap();
        let back = entry(&mut la.open(), &p).unwrap().meta;
        assert_eq!((back.parent, back.order, back.top_order, back.created), (was.parent, was.order, was.top_order, was.created));
        assert!(la.0.join("夹/页.xml").fs_is_file() && !lb.0.join("页.xml").fs_exists());
        assert!(entry(&mut lb.open(), &p).is_none());
    }

    /// A step that fails leaves both libraries as they were.
    #[test]
    fn a_failed_move_changes_nothing() {
        let (la, lb) = (Lib::new("a"), Lib::new("b"));
        let (mut a, mut b) = (la.open(), lb.open());
        let f = a.create_folder("夹", None).unwrap();
        let p = a.create_from_in(&doc("页"), Some(&f)).unwrap();
        a.set_pinned(&p, true).unwrap();
        for other in [false, true] {
            OTHER_DISK.with(|o| o.set(other));
            DIE_AT.with(|d| d.set(11));
            assert!(a.move_across(&mut b, &f, None, Place::Before(None)).is_err());
            DIE_AT.with(|d| d.set(0));
            OTHER_DISK.with(|o| o.set(false));
            assert!(la.0.join("夹/页.xml").fs_is_file() && !lb.0.join("夹").fs_exists());
            assert_eq!(moving_left(&lb), 0);
            assert!(entry(&mut la.open(), &p).unwrap().meta.pinned);
            assert!(entry(&mut lb.open(), &p).is_none() && entry(&mut b, &p).is_none());
            assert!(entry(&mut a, &p).is_some());
        }
        // And it goes the next time.
        a.move_across(&mut b, &f, None, Place::Before(None)).unwrap();
        assert!(entry(&mut lb.open(), &p).unwrap().meta.pinned);
    }

    /// The process dies midway: opened again, the page is in one library or the other,
    /// whole, and what the move left in `.quiet/moving/` goes with the next one.
    #[test]
    fn a_move_cut_short_leaves_both_libraries_whole() {
        for (step, other) in [(1, false), (1, true), (2, false), (2, true)] {
            let (la, lb) = (Lib::new("a"), Lib::new("b"));
            let (mut a, mut b) = (la.open(), lb.open());
            let f = a.create_folder("夹", None).unwrap();
            let p = a.create_from_in(&doc("页"), Some(&f)).unwrap();
            a.set_pinned(&p, true).unwrap();
            OTHER_DISK.with(|o| o.set(other));
            DIE_AT.with(|d| d.set(step));
            let died = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| a.move_across(&mut b, &f, None, Place::Before(None))));
            DIE_AT.with(|d| d.set(0));
            OTHER_DISK.with(|o| o.set(false));
            assert!(died.is_err());
            drop((a, b));
            let (mut a, mut b) = (la.open(), lb.open());
            let (in_a, in_b) = (entry(&mut a, &p), entry(&mut b, &p));
            match step {
                // Before the files arrived: still where it was, and nowhere else.
                1 => { assert!(in_a.unwrap().meta.pinned && in_b.is_none()); assert!(!lb.0.join("夹").fs_exists()); }
                // After: in the target with all it had; on another disk the original may still be there too.
                _ => { assert!(in_b.unwrap().meta.pinned); assert_eq!(in_a.is_some(), other); }
            }
            assert!(lb.0.join("夹/页.xml").fs_is_file() == (step == 2));
            // The next move clears what this one left.
            let q = a.create_from(&doc("又一页")).unwrap();
            a.move_across(&mut b, &q, None, Place::Before(None)).unwrap();
            assert_eq!(moving_left(&la) + moving_left(&lb), 0);
        }
    }

    /// A Markdown page links to files by path from itself: those that leave the moved
    /// folder are written as paths from where it is now.
    #[test]
    fn markdown_links_out_of_the_folder_follow() {
        let (la, lb) = (Lib::new("a"), Lib::new("b"));
        crate::vfs::create_dir_all(la.0.join("夹")).unwrap();
        crate::vfs::write(la.0.join("别处.md"), "# 别处\n").unwrap();
        crate::vfs::write(la.0.join("夹/页.md"), "# 页\n\n[别处](../别处.md) 和 [旁边](旁边.md) 和 [另一个](../另%20一个.md#尾)\n").unwrap();
        crate::vfs::write(la.0.join("另 一个.md"), "# 另一个\n").unwrap();
        crate::vfs::write(la.0.join("夹/旁边.md"), "# 旁边\n").unwrap();
        let (mut a, mut b) = (la.open(), lb.open());
        let f = a.list().into_iter().find(|e| e.meta.folder.is_some()).unwrap().id;
        a.move_across(&mut b, &f, None, Place::Before(None)).unwrap();
        let text = lb.read("夹/页.md");
        let back = crate::paths::resolve(text.split("[别处](").nth(1).unwrap().split(')').next().unwrap(), &lb.0.join("夹"));
        assert_eq!(back.fs_canonicalize().unwrap(), la.0.join("别处.md").fs_canonicalize().unwrap(), "{text}");
        assert!(text.contains("[旁边](旁边.md)"));
        let other = text.split("[另一个](").nth(1).unwrap().split(')').next().unwrap();
        assert!(other.ends_with("/%E5%8F%A6%20%E4%B8%80%E4%B8%AA.md#尾") || other.ends_with("/另%20一个.md#尾"), "{text}");
    }

    /// 数据表 笔记 cells are paths from the table's file: a moved table's cell to a note left
    /// behind, and a cell of a table left behind to a moved note, read the same notes afterwards.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn note_cells_follow_both_ways() {
        use crate::table::{Base, Doc, FieldType, FieldOpts, Value};
        let (la, lb) = (Lib::new("a"), Lib::new("b"));
        let (mut a, mut b) = (la.open(), lb.open());
        let f = a.create_folder("夹", None).unwrap();
        a.create_from(&doc("留下")).unwrap();
        a.create_from_in(&doc("走"), Some(&f)).unwrap();
        let table = |cell: &str| {
            let mut d = Doc::new(Base::blank_seeded(1_800_000_000_000, 9));
            let t = d.base.tables[0].id;
            let note = d.add_field(t, None, "笔记", FieldType::Note, FieldOpts::default()).unwrap();
            let row = d.base.tables[0].records[0].id;
            assert!(d.set_cells(t, vec![(row, note, Value::Text(cell.into()))]));
            (d.base, note)
        };
        let (inside, note) = table("../留下.xml");
        a.create_table_in("里面", &inside, Some(&f)).unwrap();
        let (outside, _) = table("夹/走.xml");
        a.create_table_in("外面", &outside, None).unwrap();
        a.move_across(&mut b, &f, None, Place::Before(None)).unwrap();
        let cell = |p: &Path| crate::table::sqlite::load(p).unwrap().tables[0].records[0].get(note).clone();
        let at = |dir: &Path, v: Value| { let Value::Text(t) = v else { panic!("{v:?}") }; crate::paths::resolve(&t, dir).fs_canonicalize().unwrap() };
        assert_eq!(at(&lb.0.join("夹"), cell(&lb.0.join("夹/里面.sqlite"))), la.0.join("留下.xml").fs_canonicalize().unwrap());
        let outer = a.list().into_iter().find(|e| e.title == "外面").map(|e| la.0.join(&e.meta.rel)).unwrap();
        assert_eq!(at(&la.0, cell(&outer)), lb.0.join("夹/走.xml").fs_canonicalize().unwrap());
    }
}
