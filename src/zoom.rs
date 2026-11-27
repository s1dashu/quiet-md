//! 放大 / 缩小 / 实际大小 (⌘+ ⌘− ⌘0, Ctrl off the Mac): the note's text size, as Typora
//! zooms only the document, never the sidebar or the bar above it. A step is
//! 外观 › 正文字号 moved (`themes::doc_zoom_step`), so the setting follows and is
//! kept as it is (with the top-level folder in front, W21). The 画板 and the image
//! viewer bind the same keys in their own contexts and keep them.
use crate::app::Workspace;
use crate::themes;
use gpui::*;

actions!(workspace, [ZoomInText, ZoomOutText, ResetTextZoom]);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-=", ZoomInText, None),
        KeyBinding::new("cmd-+", ZoomInText, None),
        KeyBinding::new("cmd--", ZoomOutText, None),
        KeyBinding::new("cmd-0", ResetTextZoom, None),
    ]);
}

impl Workspace {
    /// One step up (`Some(true)`), down (`Some(false)`) or back to the theme's size (`None`).
    pub fn zoom_text(&mut self, up: Option<bool>, cx: &mut Context<Self>) {
        let body = themes::active().num("page.size");
        let pt = themes::doc_size_pt(&crate::settings::prefs().doc_size, body);
        let next = match up { Some(up) => themes::doc_zoom_step(pt, body, up), None => body };
        let pct = (next / body * 100.).round() as u32;
        if next != pt {
            if let Some(ed) = &self.editor {
                ed.update(cx, |e, _| e.hold_position(next / pt));
            }
            self.set_doc_size_soon(themes::doc_pref_for(next, body), cx);
        }
        match &self.editor {
            Some(ed) => ed.update(cx, |e, cx| e.show_zoom_badge(pct, cx)),
            None => self.show_toast(&format!("{pct}%"), cx),
        }
    }
}
