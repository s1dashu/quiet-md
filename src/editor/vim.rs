//! GPUI adapter for Vim mode: key dispatch, visual line / half-page movement,
//! Flash jump viewport sampling + label overlay, and the bottom-right mode badge.

use super::view::Editor;
use crate::model::{BlockId, RichText};
use crate::settings::tr;
use crate::theme::{self, Theme};
use fdl_core::editor::vim::{FlashKeyResult, Operator, PendingPrefix, VimKey, VimMode, VimState, VimViewCmd};
use gpui::*;
use std::ops::Range;

impl Editor {
    pub fn vim_enabled(&self) -> bool {
        crate::settings::vim_mode() && !self.read_only
    }

    pub fn vim_accepts_text_input(&self) -> bool {
        !self.vim_enabled() || (self.vim.mode == VimMode::Insert && !self.vim.flash.active)
    }

    pub fn vim_wants_ime_disabled(&self) -> bool {
        self.vim_enabled() && crate::settings::vim_auto_ime() && !self.vim_accepts_text_input() && !self.vim_overlay_busy()
    }

    pub fn vim_block_caret(&self) -> bool {
        self.vim_enabled() && (self.vim.mode != VimMode::Insert || self.vim.flash.active)
    }

    pub fn vim_in_visual(&self) -> bool {
        self.vim_enabled() && !self.vim.flash.active && self.vim.is_visual()
    }

    pub fn vim_is_visual_line(&self) -> bool {
        self.vim_enabled() && !self.vim.flash.active && self.vim.is_visual_line()
    }

    pub fn vim_visual_range_in(&self, id: BlockId, units: &[super::core::Unit]) -> Option<Range<usize>> {
        self.vim_enabled().then(|| self.vim.visual_range_in(&self.core, id, units)).flatten()
    }

    pub fn sync_vim_pref(&mut self, cx: &mut Context<Self>) {
        if crate::settings::vim_mode() {
            if !self.read_only {
                self.vim.enter_normal(&mut self.core);
            }
        } else {
            self.vim = VimState::default();
            self.cat_focused = false;
        }
        cx.notify();
    }

    pub fn clear_vim_pending(&mut self, cx: &mut Context<Self>) {
        self.vim.clear_pending();
        self.cat_focused = false;
        cx.notify();
    }

    pub fn can_close_shortcuts_on_esc(&self, window: &Window, cx: &App) -> bool {
        if self.vim_overlay_busy()
            || self.marked.is_some()
            || self.cat_focused
            || self.history_drawer
            || self.preview
            || self.core.cell_sel.is_some()
            || self.core.in_block_mode()
        {
            return false;
        }
        let sub_focused = self.captions.values().any(|(f, _)| f.read(cx).focus_handle(cx).is_focused(window))
            || [&self.eq_field, &self.number_field, &self.lang_field, &self.link_field, &self.link_text_field, &self.emoji_field, &self.find_field, &self.replace_field]
                .into_iter()
                .any(|f| f.read(cx).focus_handle(cx).is_focused(window));
        if sub_focused {
            return false;
        }
        if !self.vim_enabled() && !self.core.sel.is_collapsed() && self.focus.is_focused(window) {
            return false;
        }
        if self.vim_enabled() {
            return self.vim.mode == VimMode::Normal
                && !self.vim.flash.active
                && self.vim.pending_prefix.is_none()
                && self.vim.pending_op.is_none()
                && self.vim.count.is_none();
        }
        true
    }

    pub(super) fn vim_overlay_busy(&self) -> bool {
        self.cd.dialog.is_some()
            || self.cropping()
            || self.slash.is_some()
            || self.mention.is_some()
            || self.menu.is_some()
            || self.find.is_some()
    }

    pub(super) fn handle_vim_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.vim_enabled() || !self.focus.is_focused(window) || self.vim_overlay_busy() {
            return;
        }
        let ks = &ev.keystroke;
        // Single Esc / Ctrl+[ while composing preedit text: discard uncommitted marked text and return to Normal.
        if self.marked.is_some()
            && self.vim.mode == VimMode::Insert
            && !self.vim.flash.active
            && (ks.key == "escape" || (ks.modifiers.control && ks.key == "["))
        {
            self.discard_marked();
            self.vim.enter_normal(&mut self.core);
            cx.stop_propagation();
            self.changed(window, cx);
            return;
        }
        // Never intercept system/platform (⌘) or alt shortcuts, or IME marked composition.
        if ks.modifiers.platform || ks.modifiers.alt || ks.modifiers.function || self.marked.is_some() {
            return;
        }
        let Some(vk) = to_vim_key(ks) else { return };

        if self.cat_focused {
            if self.handle_catalogue_vim_key(&vk, window, cx) {
                cx.stop_propagation();
            }
            return;
        }

        if self.vim.flash.active {
            match self.vim.handle_flash_key(&mut self.core, &vk) {
                FlashKeyResult::Ignored => return,
                FlashKeyResult::Stay | FlashKeyResult::Cancelled => {
                    cx.stop_propagation();
                    cx.notify();
                }
                FlashKeyResult::Refresh => {
                    self.refresh_flash_targets();
                    cx.stop_propagation();
                    cx.notify();
                }
                FlashKeyResult::Jumped { changed } => {
                    cx.stop_propagation();
                    if changed {
                        self.changed(window, cx);
                    } else {
                        cx.notify();
                    }
                }
            }
            return;
        }

        let outcome = self.vim.handle_key(&mut self.core, vk);
        if !outcome.consumed {
            return;
        }
        cx.stop_propagation();
        cx.notify();
        if let Some(cmd) = outcome.view_cmd {
            match cmd {
                VimViewCmd::MoveVertical { count, down, extend } => {
                    for _ in 0..count.max(1) {
                        self.move_v_vim(down, extend);
                    }
                    self.vim.post_vertical_move(&mut self.core);
                    self.toolbar_hidden = true;
                    self.changed(window, cx);
                    return;
                }
                VimViewCmd::HalfPage { down } => {
                    let rows = ((f32::from(self.list.viewport_bounds().size.height) / 28.) * 0.5).round().max(4.) as usize;
                    let extend = self.vim.is_visual();
                    for _ in 0..rows {
                        self.move_v_vim(down, extend);
                    }
                    self.vim.post_vertical_move(&mut self.core);
                    self.toolbar_hidden = true;
                    self.changed(window, cx);
                    return;
                }
                VimViewCmd::OpenFind => {
                    self.open_find(false, window, cx);
                    return;
                }
                VimViewCmd::FindNext(fwd) => {
                    if self.find.is_some() {
                        self.step_find(fwd, window, cx);
                    } else if let Some((id, r)) = self.find_last_query(fwd, cx) {
                        self.core.sel = super::core::Selection::caret(super::core::Pos::new(id, r.start));
                        self.vim.clamp_normal_caret(&mut self.core);
                        self.changed(window, cx);
                    }
                    return;
                }
                VimViewCmd::StartFlash => {
                    self.refresh_flash_targets();
                    cx.notify();
                    return;
                }
                VimViewCmd::FocusPane(pane) => {
                    cx.emit(super::view::EditorEvent::VimFocusPane(pane));
                    return;
                }
                VimViewCmd::JumpHistory { forward } => {
                    cx.emit(super::view::EditorEvent::VimJumpHistory { forward });
                    return;
                }
                VimViewCmd::CycleBuffer { forward } => {
                    cx.emit(super::view::EditorEvent::VimCycleBuffer { forward });
                    return;
                }
                VimViewCmd::OpenBufferSwitcher => {
                    cx.emit(super::view::EditorEvent::Search(String::new()));
                    return;
                }
                VimViewCmd::ToggleCatalogue => {
                    self.toggle_catalogue_focus(cx);
                    return;
                }
                VimViewCmd::OpenHome => {
                    cx.emit(super::view::EditorEvent::VimOpenHome);
                    return;
                }
                VimViewCmd::OpenHistory => {
                    cx.emit(super::view::EditorEvent::VimOpenHistory);
                    return;
                }
                VimViewCmd::ToggleShortcuts => {
                    cx.emit(super::view::EditorEvent::VimToggleShortcuts);
                    return;
                }
            }
        }
        if outcome.changed {
            self.toolbar_hidden = true;
            self.changed(window, cx);
        } else {
            cx.notify();
        }
    }

    /// Jump to the next/prev match of whatever `find_field` last searched, even after the Find bar closed.
    fn find_last_query(&self, forward: bool, cx: &App) -> Option<(BlockId, Range<usize>)> {
        let q = self.find_field.read(cx).text.clone();
        if q.is_empty() {
            return None;
        }
        let units = self.core.units();
        let mut matches = vec![];
        for u in units.iter().filter(|u| u.text) {
            if let Some(t) = self.core.text_of(u.id) {
                matches.extend(super::find::find_in(t, &q).into_iter().map(|r| (u.id, r)));
            }
        }
        if matches.is_empty() {
            return None;
        }
        let h = self.core.sel.head;
        let order = |id: BlockId| units.iter().position(|u| u.id == id).unwrap_or(usize::MAX);
        let cur = (order(h.block), h.offset);
        if forward {
            matches.iter().find(|(id, r)| (order(*id), r.start) > cur).or_else(|| matches.first()).cloned()
        } else {
            matches.iter().rfind(|(id, r)| (order(*id), r.start) < cur).or_else(|| matches.last()).cloned()
        }
    }

    fn refresh_flash_targets(&mut self) {
        let viewport = self.list.viewport_bounds();
        let cache = self.cache.borrow();
        let mut owned: Vec<(BlockId, &RichText, Range<usize>)> = Vec::new();
        for u in self.core.units().into_iter().filter(|u| u.text) {
            let Some(rt) = self.core.text_of(u.id) else { continue };
            let Some(l) = cache.get(&u.id) else { continue };
            if l.bounds.bottom() < viewport.top() || l.bounds.top() > viewport.bottom() {
                continue;
            }
            let mut min_s = usize::MAX;
            let mut max_e = 0usize;
            for r in &l.rows {
                let rt_y = l.bounds.top() + r.y + r.base_dy;
                let rb_y = rt_y + l.line_height;
                if rb_y >= viewport.top() && rt_y <= viewport.bottom() {
                    min_s = min_s.min(r.range.start);
                    max_e = max_e.max(r.range.end);
                }
            }
            if min_s <= max_e && max_e <= rt.len() {
                owned.push((u.id, rt, min_s..max_e));
            }
        }
        let caret = self.core.sel.head;
        self.vim.update_flash(&owned, caret);
    }

    /// Overlay painting Flash highlights and jump labels over the currently laid-out blocks.
    pub(super) fn render_flash_overlay(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.vim_enabled() || !self.vim.flash.active || self.vim.flash.targets.is_empty() {
            return None;
        }
        let targets = self.vim.flash.targets.clone();
        let entity = cx.entity();
        Some(
            canvas(
                move |_, _, _| {},
                move |_, _, window, cx| {
                    let items: Vec<(Vec<Bounds<Pixels>>, Point<Pixels>, Pixels, String)> = {
                        let ed = entity.read(cx);
                        let cache = ed.cache.borrow();
                        targets
                            .iter()
                            .filter_map(|t| {
                                let layout = cache.get(&t.block)?;
                                Some((layout.rects_for(t.range.clone()), layout.position_for(t.range.start), layout.line_height, t.label.clone()))
                            })
                            .collect()
                    };
                    let mut font = window.text_style().font();
                    font.family = theme::MONO_FONT.into();
                    font.weight = FontWeight::SEMIBOLD;
                    let fs = px(12.);
                    let lh = px(16.);
                    let mut hi_c = Theme::primary_500();
                    hi_c.a = 0.25;
                    let badge_bg = Theme::primary_500();
                    let badge_fg = gpui::white();
                    for (rects, p, row_lh, label) in items {
                        for r in rects {
                            window.paint_quad(fill(r, hi_c).corner_radii(px(2.)));
                        }
                        let run = TextRun { len: label.len(), font: font.clone(), color: badge_fg, background_color: None, underline: None, strikethrough: None };
                        let line = window.text_system().shape_line(label.into(), fs, &[run], None);
                        let pad_x = px(4.);
                        let w = (line.width + pad_x * 2.).max(px(14.));
                        let top = p.y + (row_lh - lh) / 2.;
                        let box_r = Bounds::new(point(p.x, top), size(w, lh));
                        window.paint_quad(fill(box_r, badge_bg).corner_radii(px(3.)));
                        let _ = line.paint(point(p.x + pad_x, top + (lh - fs) / 2. - px(1.)), lh, window, cx);
                    }
                },
            )
            .absolute()
            .inset_0()
            .into_any_element(),
        )
    }

    /// Subtle bottom-right mode badge, styled to match `word_counter`.
    pub(super) fn render_vim_badge(&self) -> Option<AnyElement> {
        if !self.vim_enabled() {
            return None;
        }
        let label = if self.cat_focused { "OUTLINE".into() } else { self.vim.mode_badge_text() };
        let accent = self.cat_focused || self.vim.flash.active || self.vim.mode != VimMode::Normal;
        Some(vim_badge_pill(label, accent).into_any_element())
    }

    pub(super) fn render_which_key(&self) -> Option<AnyElement> {
        if !self.vim_enabled() {
            return None;
        }
        if self.vim.flash.active {
            return Some(which_key_card(
                tr("Flash 快速跳转", "Flash Jump"),
                &[
                    ("<char>", tr("输入字符缩小目标", "Type to filter targets")),
                    ("<label>", tr("输入标签直接跳转", "Press label to jump")),
                    ("Esc", tr("取消跳转", "Cancel")),
                ],
            ));
        }
        if let Some(prefix) = self.vim.pending_prefix {
            let (title, items): (&str, Vec<(&str, &str)>) = match prefix {
                PendingPrefix::Space => (
                    tr("Leader · 空格", "Leader · Space"),
                    vec![
                        ("b / ␣", tr("搜索与切换文档", "Search / switch docs")),
                        ("h", tr("进入主页", "Go to Home")),
                        ("o", tr("目录大纲", "Document outline")),
                        ("H", tr("版本历史", "Version history")),
                        ("e", tr("聚焦左侧栏", "Focus sidebar")),
                        ("?", tr("快捷键速查", "Shortcuts cheatsheet")),
                    ],
                ),
                PendingPrefix::Window => (
                    tr("分栏导航 · ^W", "Window · ^W"),
                    vec![
                        ("h", tr("聚焦左侧栏", "Focus sidebar")),
                        ("l", tr("聚焦右侧主区", "Focus main pane")),
                        ("w", tr("循环切换分栏", "Cycle panes")),
                    ],
                ),
                PendingPrefix::G => (
                    tr("跳转 · g", "Goto · g"),
                    vec![("g", tr("跳到文档开头", "Go to document start"))],
                ),
                PendingPrefix::Bracket { next: false } => (
                    tr("上一处 · [", "Prev · ["),
                    vec![
                        ("b", tr("上一篇文档 (Buffer 环)", "Prev doc (buffer ring)")),
                        ("h / [", tr("上一处标题", "Prev heading")),
                    ],
                ),
                PendingPrefix::Bracket { next: true } => (
                    tr("下一处 · ]", "Next · ]"),
                    vec![
                        ("b", tr("下一篇文档 (Buffer 环)", "Next doc (buffer ring)")),
                        ("h / ]", tr("下一处标题", "Next heading")),
                    ],
                ),
                PendingPrefix::TextObj { inner } => (
                    if inner { tr("内部文本对象 · i", "Inner text object · i") } else { tr("含界文本对象 · a", "Around text object · a") },
                    vec![
                        ("w / W", tr("单词 / 字串", "Word / WORD")),
                        ("\"/ '/ `", tr("引号内容", "Quoted string")),
                        ("( / b", tr("圆括号", "Parentheses")),
                        ("[ / {", tr("方括号 / 花括号", "Brackets / braces")),
                        ("<", tr("尖括号", "Angle brackets")),
                    ],
                ),
                PendingPrefix::Find(_) => (
                    tr("行内查找字符 · f/F/t/T", "Find char · f/F/t/T"),
                    vec![
                        ("<char>", tr("跳转到目标字符", "Jump to character")),
                        ("; / ,", tr("重复上次查找", "Repeat last find")),
                    ],
                ),
                PendingPrefix::Replace => (
                    tr("替换字符 · r", "Replace · r"),
                    vec![("<char>", tr("替换当前字符", "Replace char under caret"))],
                ),
            };
            return Some(which_key_card(title, &items));
        }
        if let Some(op) = self.vim.pending_op {
            let title = match op {
                Operator::Delete => tr("删除操作符 · d", "Delete · d"),
                Operator::Change => tr("修改操作符 · c", "Change · c"),
                Operator::Yank => tr("复制操作符 · y", "Yank · y"),
                Operator::Indent => tr("增加缩进 · >", "Indent · >"),
                Operator::Outdent => tr("减少缩进 · <", "Outdent · <"),
            };
            return Some(which_key_card(
                title,
                &[
                    ("dd/cc/yy", tr("作用于当前整行", "Apply to current line")),
                    ("i / a", tr("文本对象 (iw / i\" ...)", "Text object (iw / i\" ...)")),
                    ("w / e / b", tr("词范围动作", "Word motion")),
                    ("0 / ^ / $", tr("行首 / 行尾范围", "Line start / end")),
                    ("gg / G", tr("到文档首 / 尾", "To doc start / end")),
                    ("[h / ]h", tr("到上 / 下一标题", "To prev / next heading")),
                    ("s", tr("Flash 跳转范围", "Flash target range")),
                ],
            ));
        }
        None
    }
}

pub(crate) fn vim_badge_pill(label: String, accent: bool) -> Div {
    let text_color = if accent { Theme::primary_500() } else { Theme::text_caption() };
    div()
        .h(px(24.))
        .px(px(9.))
        .rounded(px(8.))
        .bg(crate::theme::raised(Theme::bg_body()))
        .border_1()
        .border_color(Theme::button_border())
        .shadow(theme::shadow_button())
        .font_family(theme::MONO_FONT)
        .text_size(px(11.))
        .line_height(px(22.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(text_color)
        .child(label)
}

pub(crate) fn which_key_card(title: &str, items: &[(&str, &str)]) -> AnyElement {
    div()
        .w(px(252.))
        .px(px(10.))
        .py(px(8.))
        .rounded(px(10.))
        .bg(crate::theme::raised(Theme::bg_body()))
        .border_1()
        .border_color(Theme::line_border())
        .shadow(theme::shadow_panel())
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(
            div()
                .pb(px(4.))
                .border_b_1()
                .border_color(Theme::line_divider())
                .text_size(px(11.))
                .line_height(px(15.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(Theme::primary_500())
                .child(title.to_string()),
        )
        .children(items.iter().map(|(k, desc)| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .min_w(px(54.))
                        .px(px(5.))
                        .py(px(1.))
                        .rounded(px(4.))
                        .bg(Theme::fill_hover())
                        .font_family(theme::MONO_FONT)
                        .text_size(px(10.5))
                        .line_height(px(15.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(Theme::primary_500())
                        .child((*k).to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_color(Theme::text())
                        .child((*desc).to_string()),
                )
        }))
        .into_any_element()
}

fn normalize_vim_char(c: char) -> char {
    match c {
        '：' => ':',
        '；' => ';',
        '，' => ',',
        '。' => '.',
        '？' => '?',
        '！' => '!',
        '“' | '”' => '"',
        '‘' | '’' => '\'',
        '【' => '[',
        '】' => ']',
        '（' => '(',
        '）' => ')',
        '《' => '<',
        '》' => '>',
        '、' => '/',
        '·' => '`',
        '￥' => '$',
        '…' => '^',
        _ => c,
    }
}

fn to_vim_key(ks: &Keystroke) -> Option<VimKey> {
    let key = ks.key.clone();
    let ctrl = ks.modifiers.control;
    let shift = ks.modifiers.shift;
    let alt = ks.modifiers.alt;
    let ch = if ctrl {
        key.chars().next().filter(|_| key.chars().count() == 1)
    } else if let Some(kc) = &ks.key_char {
        let mut chars = kc.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_control() => Some(normalize_vim_char(c)),
            _ => None,
        }
    } else if key == "space" {
        Some(' ')
    } else if key.chars().count() == 1 {
        let c = key.chars().next().unwrap();
        Some(if shift { c.to_ascii_uppercase() } else { c })
    } else {
        None
    };
    Some(VimKey { key, ch, ctrl, shift, alt })
}

#[cfg(test)]
mod tests {
    use super::{to_vim_key, Keystroke, Modifiers};

    fn ks(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke {
            modifiers: Modifiers::default(),
            key: key.to_string(),
            key_char: key_char.map(str::to_string),
        }
    }

    #[test]
    fn fullwidth_cjk_punctuation_maps_to_ascii_vim_keys() {
        for (fw, ascii) in [
            ("：", ':'),
            ("；", ';'),
            ("，", ','),
            ("。", '.'),
            ("？", '?'),
            ("！", '!'),
            ("“", '"'),
            ("”", '"'),
            ("‘", '\''),
            ("’", '\''),
            ("【", '['),
            ("】", ']'),
            ("（", '('),
            ("）", ')'),
            ("《", '<'),
            ("》", '>'),
            ("、", '/'),
            ("·", '`'),
            ("￥", '$'),
            ("…", '^'),
        ] {
            let vk = to_vim_key(&ks(fw, Some(fw))).unwrap();
            assert_eq!(vk.ch, Some(ascii), "failed mapping {fw}");
        }
    }
}
