//! 前往 › 后退 / 前进: the documents a window has shown, in order, like a
//! browser's history. `Workspace::try_open` reports each document it shows
//! (`visit`); 后退 / 前进 open the previous / next one still in the library.
//! Where each was scrolled to comes back through 阅读位置 as on any open.

use crate::app::Workspace;
use gpui::*;

actions!(nav_history, [GoBack, GoForward]);

/// Entries kept behind the current document.
const MAX: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dir { Back, Forward }

#[derive(Default)]
pub struct NavHistory {
    back: Vec<String>,
    forward: Vec<String>,
    at: Option<String>,
    /// A 后退 / 前进 under way: the direction and the document it opens.
    going: Option<(Dir, String)>,
}

impl NavHistory {
    /// `id` is on screen now: after a 后退 / 前进 to it, that step; else a new
    /// page, which drops what was ahead.
    pub fn visit(&mut self, id: &str) {
        match self.going.take().filter(|(_, to)| to == id) {
            Some((dir, _)) => {
                let (from, to) = match dir { Dir::Back => (&mut self.back, &mut self.forward), Dir::Forward => (&mut self.forward, &mut self.back) };
                while from.pop().is_some_and(|x| x != id) {}
                to.extend(self.at.take());
            }
            None if self.at.as_deref() == Some(id) => {}
            None => {
                self.back.extend(self.at.take());
                if self.back.len() > MAX { self.back.remove(0); }
                self.forward.clear();
            }
        }
        self.at = Some(id.to_string());
    }

    /// The document 后退 / 前进 opens: the nearest one `exists` still knows
    /// (the ones gone since are dropped). It counts once `visit` sees it open.
    pub fn target(&mut self, dir: Dir, exists: impl Fn(&str) -> bool) -> Option<String> {
        let at = self.at.clone();
        let list = match dir { Dir::Back => &mut self.back, Dir::Forward => &mut self.forward };
        while list.last().is_some_and(|x| !exists(x) || Some(x) == at.as_ref()) { list.pop(); }
        let to = list.last().cloned()?;
        self.going = Some((dir, to.clone()));
        Some(to)
    }

    #[cfg(test)]
    pub fn can_go(&self, dir: Dir) -> bool {
        !match dir { Dir::Back => &self.back, Dir::Forward => &self.forward }.is_empty()
    }
}

/// ⌘[ / ⌘] (Ctrl+[ / Ctrl+] on Windows and Linux, where Alt+← / Alt+→ work
/// too, kept as written by `shortcuts::pc_binding`). In a board ⌘[ / ⌘] stay
/// 下移一层 / 上移一层: its context is deeper.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-[", GoBack, None), KeyBinding::new("cmd-]", GoForward, None)]);
    if !crate::shortcuts::mac() { cx.bind_keys([KeyBinding::new("alt-left", GoBack, None), KeyBinding::new("alt-right", GoForward, None)]); }
}

/// The 前往 menu.
pub fn menu() -> Menu {
    use crate::settings::tr;
    Menu {
        name: tr("前往", "Go").into(),
        items: vec![MenuItem::action(tr("后退", "Back"), GoBack), MenuItem::action(tr("前进", "Forward"), GoForward)],
        disabled: false,
    }
}

impl Workspace {
    /// 后退 / 前进: a document gone from the library (or now a folder) is skipped.
    pub(crate) fn go(&mut self, dir: Dir, window: &mut Window, cx: &mut Context<Self>) {
        let entries = &self.entries;
        let Some(id) = self.nav.target(dir, |id| entries.iter().any(|e| e.id == id && e.meta.folder.is_none())) else { return };
        self.open(&id, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{Dir, NavHistory};

    fn all(_: &str) -> bool { true }

    #[test]
    fn back_and_forward_walk_the_visits() {
        let mut h = NavHistory::default();
        for id in ["a", "b", "c"] { h.visit(id); }
        assert!(!h.can_go(Dir::Forward));
        assert_eq!(h.target(Dir::Back, all).as_deref(), Some("b"));
        h.visit("b");
        assert_eq!(h.target(Dir::Back, all).as_deref(), Some("a"));
        h.visit("a");
        assert!(!h.can_go(Dir::Back));
        assert_eq!(h.target(Dir::Back, all), None);
        assert_eq!(h.target(Dir::Forward, all).as_deref(), Some("b"));
        h.visit("b");
        assert_eq!(h.target(Dir::Forward, all).as_deref(), Some("c"));
        h.visit("c");
        assert!(!h.can_go(Dir::Forward));
        // Reopening what is on screen adds nothing.
        h.visit("c");
        assert_eq!(h.target(Dir::Back, all).as_deref(), Some("b"));
    }

    #[test]
    fn a_new_page_drops_what_was_ahead() {
        let mut h = NavHistory::default();
        for id in ["a", "b", "c"] { h.visit(id); }
        h.target(Dir::Back, all);
        h.visit("b");
        h.visit("d");
        assert!(!h.can_go(Dir::Forward));
        assert_eq!(h.target(Dir::Back, all).as_deref(), Some("b"));
    }

    /// A step that never opened (the save before it failed, or something
    /// else opened instead) is a plain visit; the history stays put.
    #[test]
    fn a_step_counts_only_once_open() {
        let mut h = NavHistory::default();
        for id in ["a", "b"] { h.visit(id); }
        assert_eq!(h.target(Dir::Back, all).as_deref(), Some("a"));
        h.visit("x");
        assert!(!h.can_go(Dir::Forward));
        assert_eq!(h.target(Dir::Back, all).as_deref(), Some("b"));
        h.visit("b");
        assert_eq!(h.target(Dir::Back, all).as_deref(), Some("a"));
    }

    #[test]
    fn documents_gone_are_skipped() {
        let mut h = NavHistory::default();
        for id in ["a", "b", "a", "c"] { h.visit(id); }
        // `b` was deleted; `a` twice in a row once it goes collapses.
        let live = |id: &str| id != "b";
        assert_eq!(h.target(Dir::Back, live).as_deref(), Some("a"));
        h.visit("a");
        assert_eq!(h.target(Dir::Back, live), None);
        assert!(!h.can_go(Dir::Back));
        assert_eq!(h.target(Dir::Forward, live).as_deref(), Some("c"));
    }

    #[test]
    fn history_is_bounded() {
        let mut h = NavHistory::default();
        for i in 0..250 { h.visit(&i.to_string()); }
        let mut n = 0;
        while let Some(id) = h.target(Dir::Back, all) { h.visit(&id); n += 1; }
        assert_eq!(n, super::MAX);
    }
}
