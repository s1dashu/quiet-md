//! A Markdown file opened on its own (打开文件…, the Finder's Open With, the Dock):
//! edited where it is in a window of its own, never listed in the sidebar, which
//! holds top-level folders only (folders opened the same way are `local_folders.rs`).
//! The library keeps an id for it, for 最近浏览 and so it opens as the same page.

use fdl_core::vfs::PathExt as _;
use crate::app::Workspace;
use crate::settings::tr;
use crate::ui::ToastKind;
use gpui::*;
use std::path::PathBuf;

/// What 打开文件… offers: Markdown, edited in place.
pub const MARKDOWN_EXTS: [&str; 3] = ["md", "markdown", "mdown"];

pub fn is_markdown_path(p: &std::path::Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| MARKDOWN_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// A file Quiet.md edits where it is, by the one list of what it opens
/// (`shown::LIBRARY_FILES`): Markdown. `Store::open_local` keeps a 本机文件 as
/// Markdown only: a DocxXML page's file is named by its title and its links and
/// pictures are read from the library, a 数据表 is found by its place there, and a
/// 画板 keeps its pictures there, so those are not opened in place.
pub fn opens_here(p: &std::path::Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).and_then(fdl_core::shown::kind_of_name).is_some_and(|(kind, markdown)| kind.is_doc() && markdown)
}

/// Markdown files to open: the system open panel, or a scripted answer.
fn pick(cx: &mut App) -> impl std::future::Future<Output = Option<Vec<PathBuf>>> + use<> {
    let picked = crate::devtools::take_pick();
    let extensions = Some(MARKDOWN_EXTS.iter().map(|e| SharedString::from(*e)).collect());
    let prompt = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(tr("打开", "Open").into()), extensions }));
    async move {
        match prompt {
            Some(p) => p.await.ok().and_then(Result::ok).flatten(),
            None => picked,
        }
    }
}

impl Workspace {
    /// 打开文件…: pick Markdown files and open them in place.
    pub fn open_file_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.row_menu = None;
        let paths = pick(cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(paths) = paths.await else { return };
            let _ = this.update_in(cx, |w, window, cx| w.open_paths(&paths, window, cx));
        })
        .detach();
    }

    /// Open Markdown files and folders where they are: a file outside the top-level
    /// folders in a window of its own, one inside them here; anything else is imported as before.
    pub fn open_paths(&mut self, paths: &[PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) = paths.iter().cloned().partition(|p| p.fs_is_dir());
        let (md, other): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().partition(|p| is_markdown_path(p));
        if !other.is_empty() { self.import_paths(&other, None, true, window, cx); }
        if !dirs.is_empty() { self.add_folders(&dirs, window, cx); }
        let mut last = None;
        for p in &md {
            match self.store.open_local(p) {
                Ok(id) => last = Some(id),
                Err(e) => self.show_toast_with(crate::design::Toast::new(ToastKind::Error, tr("打开文件失败", "Opening the file failed")).description(crate::settings::diagnostic(&e.to_string())), cx),
            }
        }
        self.entries.set(self.store.list());
        self.sort();
        if let Some(id) = last {
            if self.entries.iter().any(|e| e.id == id && e.meta.local()) {
                // This window is being updated: the others are looked through once it is done.
                cx.defer(move |cx| crate::quick_note::show_document(id, cx));
            } else { self.open(&id, window, cx); }
        }
        cx.notify();
    }

    /// An attachment Quiet.md edits (`opens_here`): opened where it is like
    /// 打开文件…, in a window of its own, or the window already showing it.
    pub(crate) fn open_in_own_window(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        match self.store.open_local(path) {
            Ok(id) => {
                self.entries.set(self.store.list());
                self.sort();
                // This window is being updated: the others are looked through once it is done.
                cx.defer(move |cx| crate::quick_note::show_document(id, cx));
            }
            Err(e) => self.show_toast_with(crate::design::Toast::new(ToastKind::Error, tr("打开文件失败", "Opening the file failed")).description(crate::settings::diagnostic(&e.to_string())), cx),
        }
        cx.notify();
    }

    /// A `quietmd://` link another program handed over (`quiet open`): what it names is
    /// shown here, or a toast says why not. Links only navigate (`fdl_core::app_url`).
    pub(crate) fn open_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        use fdl_core::app_url::Target;
        crate::logging::write(&format!("link {url}"));
        let missing = |w: &mut Self, msg: &str, cx: &mut Context<Self>| { w.show_toast_kind(ToastKind::Error, msg, cx); cx.notify(); };
        match Target::parse(url) {
            Err(e) => missing(self, &crate::localized_format!("无法打开链接：{}", "Could not open the link: {}", e), cx),
            Ok(Target::Doc { doc, block, .. }) => {
                // By id, which is unique across the top-level folders (W14).
                if !self.entries.iter().any(|e| e.id == doc && e.meta.folder.is_none()) { return missing(self, tr("找不到这篇文档：它可能已被移走或删除，或者所在的文件夹不在侧栏里", "Document not found: it may have been moved or deleted, or its folder isn't in the sidebar"), cx) }
                if !self.try_open(&doc, window, cx) { return }
                let Some(block) = block else { return };
                let found = self.editor.clone().and_then(|ed| ed.update(cx, |e, cx| {
                    let id = fdl_core::block_ref::locate(&e.core.doc, &block)?;
                    e.scroll_to_block(id, cx);
                    e.core.select_block(id);
                    Some(())
                }));
                if found.is_none() { missing(self, tr("已打开文档，但找不到那个块：文档可能已经改过", "Opened the document, but the block wasn't found: the document may have changed"), cx) }
            }
            Ok(Target::Folder { top, folder }) => {
                if !self.store.shown_tops().iter().any(|r| r.0 == top) { return missing(self, tr("找不到这个文件夹：它不在侧栏里", "Folder not found: it isn't in the sidebar"), cx) }
                let id = match folder {
                    Some(id) if self.entries.iter().any(|e| e.id == id && e.meta.folder.is_some() && e.top == top) => id,
                    Some(_) => return missing(self, tr("找不到这个文件夹：它可能已被移走或删除", "Folder not found: it may have been moved or deleted"), cx),
                    None => crate::shared_store::top_row(&top),
                };
                self.open_folder(&id, window, cx);
            }
            Ok(Target::Trash) => self.show_view(crate::app::View::Trash, cx),
        }
    }

    /// A relative link (`[周报](../工作/周报.md)`, `[报告](附件/报告.pdf)`,
    /// `[[page]]`): a document of the library, or a Markdown file, opens
    /// here; any other file opens in its app, and a folder, or a file that
    /// would run a program, shows in the Finder.
    pub(crate) fn open_relative(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.current.as_deref().and_then(|id| self.store.file_path(id)) else { return };
        // A move may have just rewritten the link on disk.
        let url = &self.follow_moved_links(cx).and_then(|m| m.get(url).cloned()).unwrap_or_else(|| url.to_string());
        let wiki = url.strip_prefix("quiet-wiki:");
        let (path, anchor) = match wiki {
            // `[[page]]`, looked up across the vault the way Obsidian does.
            Some(raw) => {
                let (name, anchor) = crate::markdown::wiki_parts(raw);
                let vault = crate::markdown::wiki_vault(&source, self.store.folder_dir(None).as_deref(), &self.store.top_dirs());
                (crate::markdown::resolve_wiki(name, &source, &vault).unwrap_or_default(), anchor.to_string())
            }
            None => {
                let (name, anchor) = url.split_once('#').unwrap_or((url, ""));
                let name = crate::markdown::percent_decode(name);
                let Some(dir) = source.parent() else { return };
                (if name.is_empty() { source.clone() } else { dir.join(&name) }, crate::markdown::percent_decode(anchor))
            }
        };
        let Some(path) = path.fs_canonicalize().ok().filter(|p| p.fs_exists()) else {
            self.show_toast_kind(ToastKind::Error, tr("文件已移走或删除", "The file was moved or deleted"), cx);
            return;
        };
        let existing = self.entries.iter().find(|e| self.store.file_path(&e.id)
            .is_some_and(|p| p.file_name() == path.file_name() && p.fs_canonicalize().is_ok_and(|p| p == path))).map(|e| e.id.clone());
        if existing.is_none() && !(is_markdown_path(&path) && path.fs_is_file()) {
            if path.fs_is_file() && !runs_program(&path) { cx.open_with_system(&path); } else { cx.reveal_path(&path); }
            return;
        }
        let id = existing.or_else(|| self.store.open_local(&path).ok());
        let Some(id) = id else { return };
        self.entries.set(self.store.list());
        self.sort();
        if self.try_open(&id, window, cx) && !anchor.is_empty() && let Some(ed) = &self.editor {
            let key = anchor.to_lowercase();
            ed.update(cx, |e, cx| {
                let mut found = None;
                e.core.doc.for_each_block(|b| {
                    if found.is_none() && matches!(b.kind, crate::model::BlockKind::Heading { .. }) {
                        let label = b.text.text.trim().to_lowercase();
                        if label == key || label.replace(' ', "-") == key { found = Some(b.id); }
                    }
                });
                if let Some(id) = found { e.scroll_to_block(id, cx); }
            });
        }
    }

}

/// A `quietmd://` link (`fdl_core::app_url`), as the system hands it over.
fn is_link(s: &str) -> bool { s.get(..fdl_core::app_url::SCHEME.len() + 3).is_some_and(|p| p.eq_ignore_ascii_case(&format!("{}://", fdl_core::app_url::SCHEME))) }

/// Opening it would run something (an app, a script, an installer): a link
/// in a note someone sent must not, so it only shows in the Finder.
pub(crate) fn runs_program(p: &std::path::Path) -> bool {
    const RUN: &[&str] = &["app", "command", "tool", "terminal", "sh", "bash", "zsh", "csh", "fish", "py", "pyw", "pl", "rb", "php", "js", "jse",
        "scpt", "applescript", "workflow", "action", "pkg", "mpkg", "dmg", "jar", "exe", "com", "bat", "cmd", "msi", "msix", "ps1", "vbs", "vbe",
        "wsf", "wsh", "scr", "pif", "hta", "cpl", "reg", "lnk", "desktop", "appimage", "run", "url", "webloc", "inetloc", "fileloc"];
    // No extension: it may be a program.
    p.extension().and_then(|e| e.to_str()).is_none_or(|e| RUN.contains(&e.to_ascii_lowercase().as_str()))
}

/// Files the system asked us to open (Finder's Open With, a double-click,
/// the command line), and `quietmd://` links (`quiet open`), waiting for a main window.
#[derive(Clone, Default)]
pub struct Incoming(std::rc::Rc<std::cell::RefCell<(Vec<PathBuf>, Vec<String>)>>);

impl Incoming {
    /// Markdown files, folders and links named on the command line (Windows and Linux
    /// "Open with", and a link the system starts the app with there; the Linux desktop
    /// entry's `%U` hands files over as `file://` links).
    pub fn from_args() -> Self {
        let me = Self::default();
        let (links, paths): (Vec<String>, Vec<String>) = std::env::args_os().skip(1).filter_map(|a| a.into_string().ok()).partition(|a| is_link(a) || a.starts_with("file://"));
        me.push_urls(links);
        me.0.borrow_mut().0.extend(paths.into_iter().map(PathBuf::from).filter(|p| (is_markdown_path(p) && p.fs_is_file()) || p.fs_is_dir()));
        me
    }

    pub fn push_urls(&self, urls: Vec<String>) {
        let (links, files): (Vec<String>, Vec<String>) = urls.into_iter().partition(|u| is_link(u));
        let paths = files.iter().filter_map(|u| u.strip_prefix("file://")).map(|p| PathBuf::from(crate::markdown::percent_decode(p.trim_end_matches('/'))));
        let mut q = self.0.borrow_mut();
        q.0.extend(paths.filter(|p| is_markdown_path(p) || p.fs_is_dir()));
        q.1.extend(links);
    }

    /// Hand waiting files and links to the front main window, now and whenever more arrive.
    /// A link waits until the window has listed its folders: what it names may be in one.
    pub fn deliver(self, cx: &mut App) {
        cx.spawn(async move |cx| loop {
            let waiting = { let q = self.0.borrow(); !q.0.is_empty() || !q.1.is_empty() };
            if waiting {
                cx.update(|cx| {
                    let target = cx.active_window().and_then(|w| w.downcast::<Workspace>())
                        .or_else(|| cx.windows().into_iter().find_map(|w| w.downcast::<Workspace>()));
                    let Some(target) = target else { return };
                    let paths = std::mem::take(&mut self.0.borrow_mut().0);
                    let _ = target.update(cx, |w, window, cx| {
                        window.activate_window();
                        if !paths.is_empty() { w.open_paths(&paths, window, cx); }
                        if w.store.loading() { return }
                        for link in std::mem::take(&mut self.0.borrow_mut().1) { w.open_link(&link, window, cx); }
                    });
                });
            }
            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn programs_only_show() {
        for p in ["a/报告.pdf", "幻灯片.PPTX", "页.html", "表.xlsx"] { assert!(!super::runs_program(std::path::Path::new(p)), "{p}"); }
        for p in ["安装.pkg", "x.command", "run.SH", "a.app", "setup.exe", "无扩展名"] { assert!(super::runs_program(std::path::Path::new(p)), "{p}"); }
    }

    /// What `open_link` says, by its Chinese key.
    #[test]
    fn every_language_says_why_a_link_did_not_open() {
        const TEXTS: [&str; 5] = ["无法打开链接：{}", "找不到这篇文档：它可能已被移走或删除，或者所在的文件夹不在侧栏里", "已打开文档，但找不到那个块：文档可能已经改过", "找不到这个文件夹：它不在侧栏里", "找不到这个文件夹：它可能已被移走或删除"];
        for l in crate::settings::Lang::ALL.into_iter().filter(|l| !matches!(l, crate::settings::Lang::Zh | crate::settings::Lang::En)) {
            for k in TEXTS { assert!(crate::l10n::get(l, k).is_some(), "{l:?}: {k}"); }
        }
    }

    /// Only Markdown is edited where it is; the list is `shown::LIBRARY_FILES`.
    #[test]
    fn markdown_opens_here() {
        for p in ["a/会议记录.md", ".assets/files/x/notes.MD", "b.markdown", "c.mdown"] { assert!(super::opens_here(std::path::Path::new(p)), "{p}"); }
        for p in ["报告.pdf", "页.xml", "图.excalidraw", "表.sqlite", "幻灯片.pptx", "a.txt", ".md", "无扩展名"] { assert!(!super::opens_here(std::path::Path::new(p)), "{p}"); }
        for (ext, _, markdown) in fdl_core::shown::LIBRARY_FILES { assert_eq!(super::opens_here(std::path::Path::new(&format!("x.{ext}"))), markdown, "{ext}"); }
    }
}
