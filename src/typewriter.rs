//! 打字机模式: the caret's line stays in the middle of the page while one
//! writes, as in iA Writer, Typora and Scrivener (docs/design/typewriter.md).
//! A switch in the "…" menu and ⌘⇧T (Ctrl+Shift+T), remembered for every
//! document. The scrolling itself is the editor's caret reveal
//! (`Editor::apply_reveal`); this holds the switch.
//!
//! 段落聚焦 is its own switch beside it (⌘⇧F, Ctrl+Shift+F), off by default: the paragraph being
//! written (every one a selection spans) stays clear and the others fade
//! (`Editor::item_opacity`), as iA Writer's and Typora's focus do.
use crate::app::Workspace;
use gpui::*;

actions!(workspace, [ToggleTypewriter, ToggleParagraphFocus]);

/// The "…" menu row's glyph: a line held between two arrows.
pub const ICON: &str = "valign-middle";
/// 段落聚焦's glyph: a framed line between faded ones (project artwork).
pub const FOCUS_ICON: &str = "paragraph-focus";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-shift-t", ToggleTypewriter, None), KeyBinding::new("cmd-shift-f", ToggleParagraphFocus, None)]);
}

impl Workspace {
    /// The "…" menu's switch and ⌘⇧T; the shortcut says what it did, the
    /// menu shows it on its switch.
    pub fn set_typewriter(&mut self, on: bool, toast: bool, cx: &mut Context<Self>) {
        self.set_pref(|p| p.typewriter = on, cx);
        if let Some(ed) = &self.editor {
            ed.update(cx, |e, cx| e.typewriter_changed(cx));
        }
        if toast {
            self.show_toast(if on { crate::settings::tr("打字机模式已打开", "Typewriter mode on") } else { crate::settings::tr("打字机模式已关闭", "Typewriter mode off") }, cx);
        }
        cx.notify();
    }

    /// The "…" menu's 段落聚焦 switch and ⌘⇧F; the shortcut says what it did.
    pub fn set_paragraph_focus(&mut self, on: bool, toast: bool, cx: &mut Context<Self>) {
        self.set_pref(|p| p.paragraph_focus = on, cx);
        if toast {
            self.show_toast(if on { crate::settings::tr("段落聚焦已打开", "Paragraph focus on") } else { crate::settings::tr("段落聚焦已关闭", "Paragraph focus off") }, cx);
        }
        if let Some(ed) = &self.editor {
            ed.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }
}
