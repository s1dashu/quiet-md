use crate::{app::{Workspace, View}, store::{Entry, FileState, FileDelta}};
use gpui::*;
use std::collections::{HashMap, VecDeque};

#[derive(Clone)]
struct Record { token: u64, label: &'static str, changes: Vec<FileDelta>, entries: Vec<(Option<Entry>, Option<Entry>)>,
    /// Pages this step moved into another top-level folder: undone by moving them back (`Store::move_across`).
    across: Vec<Across>,
    /// Top-level folders this step took off the sidebar with their folder (deleted through the one
    /// holding them): back on it when undone, off again when redone.
    tops: Vec<fdl_core::top_folders::TopFolderEntry>,
    /// Places only, no file touched: 置顶's (id, `pin_order` before, after) and the top-level folders'
    /// order (keys before, after), dragged by hand.
    places: Places }

#[derive(Clone, Default)]
pub(crate) struct Places { pub pins: Vec<(String, Option<String>, Option<String>)>, pub tops: Option<(Vec<String>, Vec<String>)> }

/// A change as the window's rows have it, at a top-level folder's top level (under its row), not as its library does.
fn rows_only(c: &FileDelta) -> bool {
    [&c.before, &c.after].into_iter().flatten().any(|s| s.parent.as_deref().is_some_and(|p| crate::shared_store::top_of_row(p).is_some()))
}

/// A page (or folder) moved from top-level folder `from` to `to`, with its place in each as that library has it.
#[derive(Clone)]
struct Across { id: String, from: String, to: String, before: FileState, after: FileState }
#[derive(Default)]
pub(crate) struct FileHistory {
    undo: Vec<Record>, redo: Vec<Record>, next: u64,
    pub busy: bool,
    requests: VecDeque<bool>,
}
impl Workspace {
    pub(crate) fn remember_files(&mut self, journal: crate::store::FileChanges, label: &'static str) -> u64 {
        self.remember_file_changes(journal.take(), label)
    }

    /// `remember_files` for changes written by a background store (导入文件夹).
    pub(crate) fn remember_file_changes(&mut self, changes: Vec<FileDelta>, label: &'static str) -> u64 {
        let entries = self.file_record_entries(&changes);
        self.push_file_record(changes, entries, label)
    }

    fn file_record_entries(&self, changes: &[FileDelta]) -> Vec<(Option<Entry>, Option<Entry>)> {
        changes.iter().map(|c| {
            let entry = self.entries.iter().find(|e| e.id == c.id).cloned();
            let make = |state: &FileState| {
                let mut e = entry.clone().unwrap_or_else(|| Entry { id: c.id.clone(), meta: Default::default(), title: String::new(), icon: None, body: Default::default(), reminders: vec![], links: vec![], note: None, top: String::new() });
                e.title = state.title.clone(); e.meta.folder = state.folder.then(|| state.title.clone());
                e.meta.parent = state.parent.clone(); e.meta.order = state.order; e.meta.top_order = state.top_order;
                e
            };
            (c.before.as_ref().map(make), c.after.as_ref().map(make))
        }).collect()
    }

    pub(crate) fn confirm_tree_history(&mut self, token: u64, changes: Vec<FileDelta>) {
        if changes.is_empty() { self.forget_file_change(token); return; }
        let entries = self.file_record_entries(&changes);
        if let Some(record) = self.file_history.undo.iter_mut().chain(self.file_history.redo.iter_mut()).find(|r| r.token == token) {
            record.changes = changes; record.entries = entries;
        }
    }

    /// `confirm_tree_history` for one of several moves under one record (移动到): what
    /// this one wrote replaces what the rows recorded for the same pages; the rest stands.
    pub(crate) fn join_tree_history(&mut self, token: u64, changes: Vec<FileDelta>) {
        let Some(mut merged) = self.file_history.undo.iter().chain(self.file_history.redo.iter()).find(|r| r.token == token).map(|r| r.changes.clone()) else { return };
        for delta in changes {
            match merged.iter_mut().find(|c| c.id == delta.id) { Some(c) => c.after = delta.after, None => merged.push(delta) }
        }
        let entries = self.file_record_entries(&merged);
        if let Some(record) = self.file_history.undo.iter_mut().chain(self.file_history.redo.iter_mut()).find(|r| r.token == token) { record.changes = merged; record.entries = entries; }
    }

    /// A move into another top-level folder landed: what the target wrote (`changes`) stands for
    /// what the rows recorded. `id`'s own change becomes a move between the two (undone by
    /// moving it back); the target's pages numbered anew stay changes of that one.
    pub(crate) fn cross_tree_history(&mut self, token: u64, from: &str, to: &str, id: &str, changes: Vec<FileDelta>) {
        let Some(mut merged) = self.file_history.undo.iter().chain(self.file_history.redo.iter()).find(|r| r.token == token).map(|r| r.changes.clone()) else { return };
        let mut across = None;
        merged.retain(|c| c.id != id);
        for delta in changes {
            if delta.id == id {
                if let (Some(before), Some(after)) = (delta.before, delta.after) { across = Some(Across { id: id.to_string(), from: from.to_string(), to: to.to_string(), before, after }); }
                continue;
            }
            match merged.iter_mut().find(|c| c.id == delta.id) {
                Some(c) if rows_only(c) => *c = delta,
                Some(c) => c.after = delta.after,
                None => merged.push(delta),
            }
        }
        // What only the rows recorded (a place under a top-level folder's row): the libraries wrote it, in their own terms, above.
        merged.retain(|c| c.before != c.after && !rows_only(c));
        let entries = self.file_record_entries(&merged);
        if let Some(record) = self.file_history.undo.iter_mut().chain(self.file_history.redo.iter_mut()).find(|r| r.token == token) {
            record.changes = merged; record.entries = entries;
            if let Some(a) = across { record.across.retain(|x| x.id != a.id); record.across.push(a); }
        }
    }

    pub(crate) fn remember_tree_move(&mut self, before: Vec<Entry>, label: &'static str) -> u64 {
        let old: HashMap<_, _> = before.into_iter().map(|e| (e.id.clone(), e)).collect();
        let new: HashMap<_, _> = self.entries.iter().map(|e| (e.id.clone(), e.clone())).collect();
        let mut ids: Vec<_> = old.keys().chain(new.keys()).cloned().collect(); ids.sort(); ids.dedup();
        let mut changes = vec![]; let mut entries = vec![];
        for id in ids {
            let a = old.get(&id).map(FileState::of); let b = new.get(&id).map(FileState::of);
            if a == b { continue; }
            entries.push((old.get(&id).cloned(), new.get(&id).cloned()));
            changes.push(FileDelta { id, before: a, after: b });
        }
        self.push_file_record(changes, entries, label)
    }

    /// 重命名 shown before it is written (`rename_ahead`): the record as the rows have it,
    /// replaced by what was written once it lands (`confirm_tree_history`).
    pub(crate) fn remember_title(&mut self, old: Entry, new: Option<Entry>, label: &'static str) -> u64 {
        let change = FileDelta { id: old.id.clone(), before: Some(FileState::of(&old)), after: new.as_ref().map(FileState::of) };
        self.push_file_record(vec![change], vec![(Some(old), new)], label)
    }

    fn push_file_record(&mut self, changes: Vec<FileDelta>, entries: Vec<(Option<Entry>, Option<Entry>)>, label: &'static str) -> u64 {
        if changes.is_empty() { return 0; }
        self.file_history.next += 1;
        let token = self.file_history.next;
        self.file_history.undo.push(Record { token, label, changes, entries, across: vec![], tops: vec![], places: Places::default() });
        if self.file_history.undo.len() > 100 { self.file_history.undo.remove(0); }
        self.file_history.redo.clear();
        token
    }

    /// A step that only places things (`Places`): undone and redone at once, without the libraries' file history.
    pub(crate) fn remember_places(&mut self, places: Places, label: &'static str) {
        if places.pins.is_empty() && places.tops.is_none() { return; }
        self.file_history.next += 1;
        let token = self.file_history.next;
        self.file_history.undo.push(Record { token, label, changes: vec![], entries: vec![], across: vec![], tops: vec![], places });
        if self.file_history.undo.len() > 100 { self.file_history.undo.remove(0); }
        self.file_history.redo.clear();
    }

    /// `Places` back (undo) or again (redo): a pin no longer pinned stays as it is.
    fn apply_places(&mut self, p: &Places, redo: bool, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let mut result = Ok(());
        for (id, before, after) in &p.pins {
            let key = if redo { after } else { before };
            let Some(e) = self.entries.edit().iter_mut().find(|e| &e.id == id && e.meta.pinned) else { continue };
            e.meta.pin_order = key.clone();
            if let Err(err) = self.store.set_pin(id, Some(key.clone())) { result = Err(err); }
        }
        if let Some((before, after)) = &p.tops {
            if let Err(err) = self.store.arrange_tops(if redo { after } else { before }) { result = Err(err); }
            self.tops_changed(cx);
        }
        result
    }

    /// Fold `changes` into the newest record when it made one of `ids`: a
    /// drop places what it imported, and one undo takes both back.
    pub(crate) fn amend_file_record(&mut self, ids: &[String], changes: Vec<FileDelta>) {
        let Some(record) = self.file_history.undo.last_mut().filter(|r| r.changes.iter().any(|c| ids.contains(&c.id))) else { return };
        let mut merged = std::mem::take(&mut record.changes);
        for delta in changes {
            match merged.iter_mut().find(|c| c.id == delta.id) { Some(c) => c.after = delta.after, None => merged.push(delta) }
        }
        merged.retain(|c| c.before != c.after);
        let entries = self.file_record_entries(&merged);
        let record = self.file_history.undo.last_mut().unwrap();
        record.changes = merged; record.entries = entries;
    }

    /// Step `token` also took top-level folders `tops` off the sidebar (`delete_docs`).
    pub(crate) fn file_change_took_tops(&mut self, token: u64, tops: Vec<fdl_core::top_folders::TopFolderEntry>) {
        if let Some(r) = self.file_history.undo.iter_mut().find(|r| r.token == token) { r.tops = tops; }
    }

    pub(crate) fn forget_file_change(&mut self, token: u64) {
        self.file_history.undo.retain(|r| r.token != token);
        self.file_history.redo.retain(|r| r.token != token);
    }

    /// 撤销 / 重做 in the Edit menu: the folders' next steps by name while ⌘Z is theirs (`capture_file_history`);
    /// none (plain 撤销 / 重做) while it is a page's, whose steps have no names.
    pub(crate) fn undo_menu_names(&self, window: &Window, cx: &App) -> (Option<String>, Option<String>) {
        let doc_keys = self.view == View::Doc && self.editor.as_ref().is_some_and(|e| e.read(cx).holds_focus(window, cx));
        if !files_take_undo(self.rename.is_some() || self.search_open || self.move_dlg.is_some(), doc_keys) { return (None, None) }
        let name = |r: &Record, redo: bool| {
            // one page or folder: its title too
            let title = match &r.entries[..] { [(a, b)] => b.as_ref().or(a.as_ref()).map(|e| crate::settings::display_title(&e.title)), _ => None };
            match (title, redo) {
                (Some(t), false) => crate::localized_format!("撤销{}「{}」", "Undo {} “{}”", r.label, t),
                (Some(t), true) => crate::localized_format!("重做{}「{}」", "Redo {} “{}”", r.label, t),
                (None, false) => crate::localized_format!("撤销{}", "Undo {}", r.label),
                (None, true) => crate::localized_format!("重做{}", "Redo {}", r.label),
            }
        };
        (self.file_history.undo.last().map(|r| name(r, false)), self.file_history.redo.last().map(|r| name(r, true)))
    }

    /// ⌘Z / ⌘⇧Z, caught on the way down: the folders' history (创建副本, 移动, 删除…) only while the keys are
    /// in the sidebar or a folder's page. With the open document holding them it is that document's own, even
    /// with nothing left to undo there (as in Notion and 飞书: undo in a page never undoes the sidebar).
    pub(crate) fn capture_file_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        let doc_keys = self.view == View::Doc && self.editor.as_ref().is_some_and(|e| e.read(cx).holds_focus(window, cx));
        if !files_take_undo(self.rename.is_some() || self.search_open || self.move_dlg.is_some(), doc_keys) { return; }
        cx.stop_propagation();
        self.file_history.requests.push_back(redo);
        self.run_file_history(window, cx);
    }

    fn run_file_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_history.busy || self.file_history.requests.is_empty() { return; }
        if self.tree_writes_pending() {
            self.file_history.busy = true;
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(std::time::Duration::from_millis(12)).await;
                    let ready = this.update(cx, |w, _| !w.tree_writes_pending());
                    match ready { Ok(false) => continue, Err(_) => return, Ok(true) => break }
                }
                let _ = this.update_in(cx, |w, window, cx| { w.file_history.busy = false; w.run_file_history(window, cx); cx.notify(); });
            }).detach();
            cx.notify(); return;
        }
        let redo = self.file_history.requests.pop_front().unwrap();
        let record = if redo { self.file_history.redo.last() } else { self.file_history.undo.last() }.cloned();
        let Some(record) = record else { self.file_history.requests.clear(); return; };
        if record.changes.is_empty() && record.across.is_empty() && record.tops.is_empty() {
            let done = self.apply_places(&record.places, redo, cx);
            if redo { self.file_history.redo.pop(); self.file_history.undo.push(record.clone()); }
            else { self.file_history.undo.pop(); self.file_history.redo.push(record.clone()); }
            match done {
                Ok(()) => self.show_toast(&format!("{}{}", if redo { crate::settings::tr("已重做：", "Redone: ") } else { crate::settings::tr("已撤销：", "Undone: ") }, record.label), cx),
                Err(e) => { self.file_history.requests.clear(); self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx); }
            }
            self.sort();
            cx.notify();
            return self.run_file_history(window, cx);
        }
        if !self.flush(cx) { self.file_history.requests.clear(); return; }
        for c in &record.changes {
            let content = c.before.is_none() || c.after.is_none() || c.before.as_ref().zip(c.after.as_ref()).is_some_and(|(a,b)| a.title != b.title);
            if content && !self.document_peers(&c.id, window, cx).is_empty() {
                self.show_toast_kind(crate::ui::ToastKind::Error, crate::settings::tr("请先关闭正在编辑这些文档的其他窗口", "Close other windows editing these documents first"), cx);
                self.file_history.requests.clear(); return;
            }
        }
        self.file_history.busy = true;
        let previous = self.entries.to_vec();
        for (a, b) in &record.entries {
            let target = if redo { b } else { a };
            let id = a.as_ref().or(b.as_ref()).unwrap().id.clone();
            if let Some(target) = target {
                if let Some(entry) = self.entries.edit().iter_mut().find(|e| e.id == id) {
                    let delta = record.changes.iter().find(|c| c.id == id).unwrap();
                    let fields_changed = delta.before.as_ref().zip(delta.after.as_ref());
                    if fields_changed.is_some_and(|(a,b)| a.title != b.title) { entry.title = target.title.clone(); entry.meta.folder = target.meta.folder.clone(); }
                    if fields_changed.is_some_and(|(a,b)| (a.parent.clone(),a.order,a.top_order) != (b.parent.clone(),b.order,b.top_order)) {
                        entry.meta.parent = target.meta.parent.clone(); entry.meta.order = target.meta.order; entry.meta.top_order = target.meta.top_order;
                    }
                } else { self.entries.edit().push(target.clone()); }
                if let Some(parent) = &target.meta.parent { self.expanded.insert(parent.clone()); }
            } else { self.entries.edit().retain(|e| e.id != id); }
        }
        // Each change goes to the top-level folder holding its page now (one that holds none, to the
        // first: it fails there as changed); a move between two, to both.
        let mut groups: Vec<(String, Vec<FileDelta>)> = vec![];
        let first = record.changes.iter().find_map(|c| self.store.top_of(&c.id).or_else(|| self.store.trash_top(&c.id)));
        for c in &record.changes {
            let Some(k) = self.store.top_of(&c.id).or_else(|| self.store.trash_top(&c.id)).or_else(|| first.clone()) else { continue };
            match groups.iter_mut().find(|g| g.0 == k) { Some(g) => g.1.push(c.clone()), None => groups.push((k, vec![c.clone()])) }
        }
        let across = record.across.clone();
        let mut keys: Vec<String> = groups.iter().map(|g| g.0.clone()).chain(across.iter().flat_map(|a| [a.from.clone(), a.to.clone()])).collect();
        keys.dedup();
        let keys: Vec<String> = keys.iter().enumerate().filter(|(i, k)| !keys[..*i].contains(k)).map(|(_, k)| k.clone()).collect();
        let writers = self.store.writers(&keys);
        if writers.is_empty() || writers.len() != keys.len() { self.file_history.busy = false; self.entries.set(previous); self.file_history.requests.clear(); return; }
        let task = cx.background_executor().spawn(async move {
            let mut ws = writers;
            let at = |ws: &[(String, u64, crate::store::Store)], k: &str| ws.iter().position(|w| w.0 == k).unwrap();
            // A move between two top-level folders, back (undo) or again (redo), to the numbers it had there.
            let cross = |ws: &mut [(String, u64, crate::store::Store)], a: &Across| -> anyhow::Result<()> {
                let (from, to, place) = if redo { (&a.from, &a.to, &a.after) } else { (&a.to, &a.from, &a.before) };
                let (i, j) = (at(ws, from), at(ws, to));
                let (x, y) = if i < j { let (l, r) = ws.split_at_mut(j); (&mut l[i], &mut r[0]) } else { let (l, r) = ws.split_at_mut(i); (&mut r[0], &mut l[j]) };
                x.2.move_across(&mut y.2, &a.id, place.parent.as_deref(), crate::store::Place::At(place.order, place.top_order))
            };
            let mut result = Ok(());
            if redo { for a in &across { result = result.and_then(|_| cross(&mut ws, a)); } }
            for (k, changes) in &groups { let i = at(&ws, k); result = result.and_then(|_| ws[i].2.apply_file_history(changes, redo)); }
            if !redo { for a in across.iter().rev() { result = result.and_then(|_| cross(&mut ws, a)); } }
            let lists: Vec<(String, u64, crate::store::Store, Vec<Entry>)> = ws.into_iter().map(|(k, n, mut w)| { let e = w.list(); (k, n, w, e) }).collect();
            (result, lists)
        });
        cx.spawn_in(window, async move |this, cx| {
            let (result, lists) = task.await;
            let _ = this.update_in(cx, |w, window, cx| {
                let mut entries = vec![];
                for (key, n, writer, listed) in lists { entries = w.store.adopt_written(&key, n, writer, listed); }
                w.tops_moved(cx);
                if !record.across.is_empty() {
                    w.store.moved_across();
                    for a in &record.across { let (from, to) = if redo { (&a.from, &a.to) } else { (&a.to, &a.from) }; w.moved_open_document(&a.id, from, to); }
                }
                w.file_history.busy = false;
                match result {
                    Ok(()) => {
                        // Top-level folders deleted with a folder of another: back with it, or off again.
                        if !record.tops.is_empty() {
                            if redo { let dirs: Vec<std::path::PathBuf> = record.tops.iter().map(|t| t.path.clone()).collect(); let _ = w.store.take_tops_within(&dirs); }
                            else { w.store.put_back(record.tops.clone()); }
                            w.tops_changed(cx);
                        }
                        let entries = if record.tops.is_empty() { entries } else { w.store.list() };
                        w.entries.set(entries);
                        if redo { w.file_history.redo.pop(); w.file_history.undo.push(record.clone()); }
                        else { w.file_history.undo.pop(); w.file_history.redo.push(record.clone()); }
                        if let Some(id) = w.current.clone() {
                            if let Some(entry) = w.entries.iter().find(|e| e.id == id) {
                                if let Some(ed) = &w.editor { let title = entry.title.clone(); let _ = w.store.accept_file_title(&id, &title); ed.update(cx, |e, cx| e.receive_file_title(&title, cx)); }
                            } else { w.editor = None; w.editor_sub = None; w.current = None; w.view = View::Default; window.focus(&w.focus, cx); }
                        }
                        if w.folder_current.as_ref().is_some_and(|id| !w.entries.iter().any(|e| &e.id == id)) { w.folder_current = None; w.view = View::Default; }
                        w.trash = w.store.list_trash();
                        w.batch = None; w.row_menu = None;
                        w.sync_sub_pages(cx);
                        w.show_toast(&format!("{}{}", if redo { crate::settings::tr("已重做：", "Redone: ") } else { crate::settings::tr("已撤销：", "Undone: ") }, record.label), cx);
                    }
                    Err(e) => {
                        w.entries.set(if entries.is_empty() && !previous.is_empty() { previous } else { entries });
                        w.file_history.requests.clear();
                        w.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx);
                    }
                }
                w.sort();
                w.run_file_history(window, cx); cx.notify();
            });
        }).detach();
        cx.notify();
    }
}

/// Whose ⌘Z it is: the folders', unless a field of the workspace's own (重命名, 搜索, 移动到) or the open
/// document has the keys.
fn files_take_undo(field: bool, doc_keys: bool) -> bool { !field && !doc_keys }

#[cfg(test)]
mod tests {
    // (`test` here is GPUI's, through the glob)
    use super::*;

    /// The report (2026-11-27): 创建副本 of a 数据库 on a folder's page, the copy opened and a cell edited,
    /// ⌘Z undid the 创建副本. Whatever the folders did last, the document's keys keep its ⌘Z, its own
    /// history spent or not; back in the sidebar or a folder's page, ⌘Z is the folders' again.
    #[core::prelude::v1::test]
    fn the_folders_undo_only_without_the_document() {
        let mut h = FileHistory::default();
        h.undo.push(Record { token: 1, label: "创建副本", changes: vec![], entries: vec![], across: vec![], tops: vec![], places: Places::default() });
        let mut table = fdl_core::table::Doc::new(fdl_core::table::Base::blank_seeded(0, 2));
        let (t, f) = (table.base.tables[0].id, table.base.tables[0].fields[0].id);
        let r = table.base.tables[0].records[0].id;
        assert!(table.set_cells(t, vec![(r, f, fdl_core::table::Value::Text("改过".into()))]));
        // in the table: its ⌘Z, the cell back, the folders' step still there
        assert!(!files_take_undo(false, true));
        assert!(table.undo());
        assert_eq!(h.undo.len(), 1);
        // its history spent: still not the folders'
        assert!(!files_take_undo(false, true) && !table.undo());
        // the sidebar or the folder's page: the folders'; a field of the workspace's own keeps its keys
        assert!(files_take_undo(false, false));
        assert!(!files_take_undo(true, false));
    }
}
