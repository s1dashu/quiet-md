//! GUI-independent Vim state machine, motions, operators, text objects and
//! Flash jump target allocation, operating solely through [`Core`].

use super::core::{next_grapheme, prev_grapheme, Core, DropPlace, EditKind, Pos, Selection, TITLE};
use crate::model::{Block, BlockKind, Marks, RichText};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    VisualChar,
    VisualLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
}

impl Operator {
    pub fn label(self) -> char {
        match self {
            Operator::Delete => 'd',
            Operator::Change => 'c',
            Operator::Yank => 'y',
            Operator::Indent => '>',
            Operator::Outdent => '<',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindKind {
    Forward,      // f
    Backward,     // F
    TillForward,  // t
    TillBackward, // T
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingPrefix {
    G,
    Find(FindKind),
    Replace,
    TextObj { inner: bool },
    Window,
    Bracket { next: bool },
    Space,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Register {
    pub text: String,
    pub blocks: Vec<Block>,
    pub linewise: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlashTarget {
    pub block: crate::model::BlockId,
    pub range: Range<usize>,
    pub label: String,
}

#[derive(Clone, Debug, Default)]
pub struct FlashState {
    pub active: bool,
    pub query: String,
    pub op: Option<Operator>,
    pub visual: Option<VimMode>,
    pub targets: Vec<FlashTarget>,
}

/// Platform-independent keystroke fed into [`VimState::handle_key`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VimKey {
    pub key: String,
    pub ch: Option<char>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl VimKey {
    pub fn char(c: char) -> Self {
        VimKey { key: c.to_string(), ch: Some(c), ctrl: false, shift: c.is_ascii_uppercase(), alt: false }
    }

    pub fn named(name: &str) -> Self {
        VimKey { key: name.to_string(), ch: None, ctrl: false, shift: false, alt: false }
    }

    pub fn ctrl_char(c: char) -> Self {
        VimKey { key: c.to_string(), ch: Some(c), ctrl: true, shift: false, alt: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimPane {
    Left,
    Right,
    Cycle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VimViewCmd {
    /// Move cursor visually up/down `count` rows (delegated to LayoutCache).
    MoveVertical { count: usize, down: bool, extend: bool },
    /// Scroll roughly half a viewport up/down.
    HalfPage { down: bool },
    /// Open built-in Find bar (`/`).
    OpenFind,
    /// Jump to next/prev Find match (`n` / `N`).
    FindNext(bool),
    /// Request entering Flash jump mode (view collects visible ranges and calls `update_flash`).
    StartFlash,
    /// Move focus between workspace panes (`Ctrl+w h/l/w`).
    FocusPane(VimPane),
    /// Jump backward (`false`) or forward (`true`) in the workspace document history (`Ctrl+o`/`Ctrl+i`).
    JumpHistory { forward: bool },
    /// Cycle backward (`false`) or forward (`true`) in the workspace buffer ring (`[b`/`]b`).
    CycleBuffer { forward: bool },
    /// Open the workspace quick buffer / document switcher (`Space b` / `Space Space`).
    OpenBufferSwitcher,
    /// Toggle focus/visibility of the document outline catalogue (`Space o`).
    ToggleCatalogue,
    /// Switch to the workspace Home view (`Space h`).
    OpenHome,
    /// Open the version history drawer (`Space H`).
    OpenHistory,
    /// Toggle the shortcuts cheatsheet drawer (`Space ?` / `Space /`).
    ToggleShortcuts,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VimOutcome {
    pub consumed: bool,
    pub changed: bool,
    pub view_cmd: Option<VimViewCmd>,
}

impl VimOutcome {
    fn ignored() -> Self {
        VimOutcome { consumed: false, changed: false, view_cmd: None }
    }

    fn stay(changed: bool) -> Self {
        VimOutcome { consumed: true, changed, view_cmd: None }
    }

    fn cmd(view_cmd: VimViewCmd) -> Self {
        VimOutcome { consumed: true, changed: false, view_cmd: Some(view_cmd) }
    }
}

#[derive(Clone, Debug)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordStart,
    BigWordStart,
    WordBack,
    BigWordBack,
    WordEnd,
    BigWordEnd,
    LineStart,
    LineFirstNonBlank,
    LineEnd,
    DocStart,
    DocEnd,
    ParagraphPrev,
    ParagraphNext,
    HeadingPrev,
    HeadingNext,
    FindChar(FindKind, char),
}

#[derive(Clone, Debug)]
enum LastChange {
    Keys(Vec<VimKey>),
    Insert { opener: Vec<VimKey>, inserted: String },
}

#[derive(Clone, Debug, Default)]
pub struct VimState {
    pub mode: VimMode,
    pub count: Option<usize>,
    pub pending_op: Option<Operator>,
    pub pending_prefix: Option<PendingPrefix>,
    pub last_find: Option<(FindKind, char)>,
    pub register: Register,
    pub flash: FlashState,
    last_change: Option<LastChange>,
    recording_insert: Option<(Vec<VimKey>, String)>,
    replaying: bool,
    /// Keys buffered for the current Normal command (used to capture repeatable edits).
    cmd_buf: Vec<VimKey>,
}

impl VimState {
    pub fn is_visual(&self) -> bool {
        matches!(self.mode, VimMode::VisualChar | VimMode::VisualLine)
    }

    pub fn is_visual_line(&self) -> bool {
        self.mode == VimMode::VisualLine
    }

    /// Inclusive selection range inside `id` for rendering Visual / VisualLine modes.
    pub fn visual_range_in(&self, core: &Core, id: crate::model::BlockId, units: &[crate::editor::core::Unit]) -> Option<Range<usize>> {
        if !self.is_visual() {
            return None;
        }
        let (a, b) = order_pair_in(units, core.sel.anchor, core.sel.head);
        let (start, end) = match self.mode {
            VimMode::VisualChar => (a, inclusive_visual_end(core, a, b)),
            VimMode::VisualLine => (line_start_pos(core, a), line_end_pos(core, b)),
            _ => return None,
        };
        if start.block == end.block {
            return (start.block == id).then_some(start.offset..end.offset);
        }
        let si = units.iter().position(|u| u.id == start.block)?;
        let ei = units.iter().position(|u| u.id == end.block)?;
        let i = units.iter().position(|u| u.id == id)?;
        if i < si || i > ei {
            return None;
        }
        let len = core.len_of(id);
        let ra = if i == si { start.offset } else { 0 };
        let rb = if i == ei { end.offset } else { len };
        Some(ra..rb)
    }

    pub fn clear_pending(&mut self) {
        self.count = None;
        self.pending_op = None;
        self.pending_prefix = None;
        self.cmd_buf.clear();
    }

    pub fn enter_normal(&mut self, core: &mut Core) {
        let was_insert = self.mode == VimMode::Insert;
        self.mode = VimMode::Normal;
        self.clear_pending();
        self.flash = FlashState::default();
        if was_insert {
            core.break_coalescing();
            if let Some((opener, inserted)) = self.recording_insert.take() {
                self.last_change = Some(LastChange::Insert { opener, inserted });
            }
            // Standard Vim: leaving Insert steps one grapheme left inside the logical line.
            let h = core.sel.head;
            if let Some(t) = core.text_of(h.block) {
                let (ls, _) = line_bounds(&t.text, h.offset);
                if h.offset > ls {
                    let prev = prev_grapheme(&t.text, h.offset).max(ls);
                    core.sel = Selection::caret(Pos::new(h.block, prev));
                } else {
                    core.sel = Selection::caret(h);
                }
            }
        } else if !core.sel.is_collapsed() {
            core.sel = Selection::caret(core.sel.head);
        }
        core.block_sel = None;
        core.cell_sel = None;
        self.clamp_normal_caret(core);
    }

    pub fn note_inserted_text(&mut self, text: &str) {
        if self.mode == VimMode::Insert
            && !self.replaying
            && let Some((_, buf)) = &mut self.recording_insert
        {
            buf.push_str(text);
        }
    }

    pub fn pending_display(&self) -> String {
        let mut s = String::new();
        if let Some(c) = self.count {
            s.push_str(&c.to_string());
        }
        if let Some(op) = self.pending_op {
            s.push(op.label());
        }
        if let Some(p) = self.pending_prefix {
            match p {
                PendingPrefix::G => s.push('g'),
                PendingPrefix::Find(k) => s.push(match k {
                    FindKind::Forward => 'f',
                    FindKind::Backward => 'F',
                    FindKind::TillForward => 't',
                    FindKind::TillBackward => 'T',
                }),
                PendingPrefix::Replace => s.push('r'),
                PendingPrefix::TextObj { inner } => s.push(if inner { 'i' } else { 'a' }),
                PendingPrefix::Window => s.push_str("^W"),
                PendingPrefix::Bracket { next } => s.push(if next { ']' } else { '[' }),
                PendingPrefix::Space => s.push('␣'),
            }
        }
        s
    }

    pub fn mode_badge_text(&self) -> String {
        if self.flash.active {
            return if self.flash.query.is_empty() {
                "FLASH".into()
            } else {
                format!("FLASH: {}", self.flash.query)
            };
        }
        let name = match self.mode {
            VimMode::Normal => "NORMAL",
            VimMode::Insert => "INSERT",
            VimMode::VisualChar => "VISUAL",
            VimMode::VisualLine => "V-LINE",
        };
        let pend = self.pending_display();
        if pend.is_empty() { name.into() } else { format!("{name} · {pend}") }
    }

    /// Keep Normal-mode caret inside `[ls, last_grapheme_start]` of its logical line.
    pub fn clamp_normal_caret(&self, core: &mut Core) {
        if self.mode == VimMode::Insert {
            return;
        }
        let h = core.sel.head;
        let Some(t) = core.text_of(h.block) else { return };
        let offset = h.offset.min(t.len());
        let (ls, le) = line_bounds(&t.text, offset);
        let max_off = if le > ls { prev_grapheme(&t.text, le).max(ls) } else { ls };
        let clamped = offset.clamp(ls, max_off);
        let head = Pos::new(h.block, clamped);
        if self.is_visual() {
            core.sel.head = head;
        } else if clamped != h.offset || !core.sel.is_collapsed() {
            core.sel = Selection::caret(head);
        }
    }

    /// Enter Insert mode and begin recording inserted characters for `.`.
    fn begin_insert(&mut self, opener: Vec<VimKey>) {
        self.mode = VimMode::Insert;
        self.clear_pending();
        if !self.replaying {
            self.recording_insert = Some((opener, String::new()));
        }
    }

    fn finish_edit(&mut self, keys: Vec<VimKey>, core: &mut Core) {
        core.break_coalescing();
        if !self.replaying && !keys.is_empty() {
            self.last_change = Some(LastChange::Keys(keys));
        }
        self.clear_pending();
        self.clamp_normal_caret(core);
    }

    pub fn handle_key(&mut self, core: &mut Core, key: VimKey) -> VimOutcome {
        if self.flash.active {
            // Flash keys are handled via `handle_flash_key` because target recomputation
            // needs the editor's visible text slices; Escape cancels right away.
            if key.key == "escape" || (key.ctrl && key.key == "[") {
                self.flash = FlashState::default();
                return VimOutcome::stay(true);
            }
            return VimOutcome::ignored();
        }
        if self.mode == VimMode::Insert {
            if key.key == "escape" || (key.ctrl && key.key == "[") {
                self.enter_normal(core);
                return VimOutcome::stay(true);
            }
            return VimOutcome::ignored();
        }
        if key.key == "escape" || (key.ctrl && key.key == "[") {
            let had_sel = !core.sel.is_collapsed() || self.mode != VimMode::Normal;
            self.enter_normal(core);
            return VimOutcome::stay(had_sel);
        }
        if key.alt {
            return VimOutcome::ignored();
        }

        // Ctrl combinations in Normal / Visual modes (allow `Ctrl+w` followed by `Ctrl+h/l/w`).
        if key.ctrl {
            if self.pending_prefix == Some(PendingPrefix::Window) {
                return self.handle_prefixed(core, PendingPrefix::Window, key);
            }
            return self.handle_ctrl_key(core, &key);
        }

        // Buffer key for repeatable `.` commands.
        if !self.replaying && !(self.pending_op.is_none() && self.pending_prefix.is_none() && key.ch == Some('.')) {
            self.cmd_buf.push(key.clone());
        }

        // Handle pending single-char prefixes (`f/F/t/T`, `r`, `g`, `i/a`).
        if let Some(prefix) = self.pending_prefix {
            return self.handle_prefixed(core, prefix, key);
        }

        let Some(ch) = key.ch else {
            return self.handle_non_char(core, &key);
        };

        // Numeric count prefix (`1..=9`, or `0` when count already started).
        if ch.is_ascii_digit() && (ch != '0' || self.count.is_some()) {
            let d = (ch as u8 - b'0') as usize;
            let next = self.count.unwrap_or(0).saturating_mul(10).saturating_add(d).min(9999);
            self.count = Some(next);
            return VimOutcome::stay(false);
        }

        // Visual modes.
        if matches!(self.mode, VimMode::VisualChar | VimMode::VisualLine) {
            return self.handle_visual_char_key(core, ch);
        }

        // Operator-pending doubled operators (`dd`, `cc`, `yy`, `>>`, `<<`) or Flash (`s`).
        if let Some(op) = self.pending_op {
            if ch == op.label() {
                let count = self.count.unwrap_or(1);
                let keys = std::mem::take(&mut self.cmd_buf);
                let changed = self.exec_line_operator(core, op, count);
                if op == Operator::Change {
                    self.begin_insert(keys);
                } else {
                    self.finish_edit(keys, core);
                }
                return VimOutcome::stay(changed);
            }
            if ch == 's' {
                self.flash = FlashState { active: true, op: Some(op), ..Default::default() };
                self.clear_pending();
                return VimOutcome::cmd(VimViewCmd::StartFlash);
            }
            if matches!(ch, 'i' | 'a') {
                self.pending_prefix = Some(PendingPrefix::TextObj { inner: ch == 'i' });
                return VimOutcome::stay(false);
            }
        }

        self.handle_normal_char(core, ch)
    }

    fn handle_ctrl_key(&mut self, core: &mut Core, key: &VimKey) -> VimOutcome {
        match key.key.as_str() {
            "r" => {
                self.clear_pending();
                let ok = core.redo();
                self.clamp_normal_caret(core);
                VimOutcome::stay(ok)
            }
            "d" => {
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::HalfPage { down: true })
            }
            "u" => {
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::HalfPage { down: false })
            }
            "w" if self.mode == VimMode::Normal && self.pending_op.is_none() => {
                self.clear_pending();
                self.pending_prefix = Some(PendingPrefix::Window);
                VimOutcome::stay(false)
            }
            "o" if self.mode == VimMode::Normal && self.pending_op.is_none() => {
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::JumpHistory { forward: false })
            }
            "i" if self.mode == VimMode::Normal && self.pending_op.is_none() => {
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::JumpHistory { forward: true })
            }
            _ => VimOutcome::ignored(),
        }
    }

    fn handle_non_char(&mut self, core: &mut Core, key: &VimKey) -> VimOutcome {
        let extend = matches!(self.mode, VimMode::VisualChar | VimMode::VisualLine);
        match key.key.as_str() {
            "space" if self.mode == VimMode::Normal && self.pending_op.is_none() && self.pending_prefix.is_none() => {
                self.pending_prefix = Some(PendingPrefix::Space);
                VimOutcome::stay(false)
            }
            "backspace" => {
                if self.pending_op.is_none() && self.pending_prefix.is_none() {
                    let count = self.count.unwrap_or(1);
                    self.clear_pending();
                    self.apply_motion(core, Motion::Left, count, extend);
                    return VimOutcome::stay(true);
                }
                self.clear_pending();
                VimOutcome::stay(false)
            }
            _ => VimOutcome::ignored(),
        }
    }

    fn handle_prefixed(&mut self, core: &mut Core, prefix: PendingPrefix, key: VimKey) -> VimOutcome {
        self.pending_prefix = None;
        match prefix {
            PendingPrefix::G => {
                if key.ch == Some('g') {
                    let count = self.count.unwrap_or(1);
                    return self.dispatch_motion(core, Motion::DocStart, count);
                }
                self.clear_pending();
                VimOutcome::stay(false)
            }
            PendingPrefix::Find(kind) => {
                if let Some(c) = key.ch {
                    self.last_find = Some((kind, c));
                    let count = self.count.unwrap_or(1);
                    return self.dispatch_motion(core, Motion::FindChar(kind, c), count);
                }
                self.clear_pending();
                VimOutcome::stay(false)
            }
            PendingPrefix::Replace => {
                let Some(c) = key.ch else {
                    self.clear_pending();
                    return VimOutcome::stay(false);
                };
                let count = self.count.unwrap_or(1);
                let keys = std::mem::take(&mut self.cmd_buf);
                let changed = self.exec_replace_char(core, c, count);
                self.finish_edit(keys, core);
                VimOutcome::stay(changed)
            }
            PendingPrefix::TextObj { inner } => {
                let Some(c) = key.ch else {
                    self.clear_pending();
                    return VimOutcome::stay(false);
                };
                if let Some(range) = resolve_text_object(core, c, inner) {
                    if let Some(op) = self.pending_op {
                        let keys = std::mem::take(&mut self.cmd_buf);
                        let changed = self.apply_operator_range(core, op, range, false);
                        if op == Operator::Change {
                            self.begin_insert(keys);
                        } else {
                            self.finish_edit(keys, core);
                        }
                        return VimOutcome::stay(changed);
                    }
                    if matches!(self.mode, VimMode::VisualChar | VimMode::VisualLine) {
                        self.mode = VimMode::VisualChar;
                        let head = prev_pos_in_block(core, range.1);
                        core.sel = Selection { anchor: range.0, head };
                        self.clear_pending();
                        return VimOutcome::stay(true);
                    }
                }
                self.clear_pending();
                VimOutcome::stay(false)
            }
            PendingPrefix::Window => {
                let k = key.ch.or_else(|| key.key.chars().next().filter(|_| key.key.len() == 1));
                self.clear_pending();
                match k {
                    Some('h') => VimOutcome::cmd(VimViewCmd::FocusPane(VimPane::Left)),
                    Some('l') => VimOutcome::cmd(VimViewCmd::FocusPane(VimPane::Right)),
                    Some('w') => VimOutcome::cmd(VimViewCmd::FocusPane(VimPane::Cycle)),
                    _ => VimOutcome::stay(false),
                }
            }
            PendingPrefix::Bracket { next } => {
                if !key.ctrl {
                    if key.ch == Some('b') && self.pending_op.is_none() && self.mode == VimMode::Normal {
                        self.clear_pending();
                        return VimOutcome::cmd(VimViewCmd::CycleBuffer { forward: next });
                    }
                    let same = if next { ']' } else { '[' };
                    if key.ch == Some('h') || key.ch == Some(same) {
                        let count = self.count.unwrap_or(1);
                        let m = if next { Motion::HeadingNext } else { Motion::HeadingPrev };
                        return self.dispatch_motion(core, m, count);
                    }
                }
                self.clear_pending();
                VimOutcome::stay(false)
            }
            PendingPrefix::Space => {
                self.clear_pending();
                if !key.ctrl {
                    match key.ch {
                        Some('b' | ' ') => return VimOutcome::cmd(VimViewCmd::OpenBufferSwitcher),
                        Some('o') => return VimOutcome::cmd(VimViewCmd::ToggleCatalogue),
                        Some('h') => return VimOutcome::cmd(VimViewCmd::OpenHome),
                        Some('H') => return VimOutcome::cmd(VimViewCmd::OpenHistory),
                        Some('e') => return VimOutcome::cmd(VimViewCmd::FocusPane(VimPane::Left)),
                        Some('?' | '/') => return VimOutcome::cmd(VimViewCmd::ToggleShortcuts),
                        _ if key.key == "space" => return VimOutcome::cmd(VimViewCmd::OpenBufferSwitcher),
                        _ => {}
                    }
                }
                VimOutcome::stay(false)
            }
        }
    }

    fn handle_normal_char(&mut self, core: &mut Core, ch: char) -> VimOutcome {
        let count = self.count.unwrap_or(1);
        // Operators
        if self.pending_op.is_none() {
            match ch {
                'd' => {
                    self.pending_op = Some(Operator::Delete);
                    return VimOutcome::stay(false);
                }
                'c' => {
                    self.pending_op = Some(Operator::Change);
                    return VimOutcome::stay(false);
                }
                'y' => {
                    self.pending_op = Some(Operator::Yank);
                    return VimOutcome::stay(false);
                }
                '>' => {
                    self.pending_op = Some(Operator::Indent);
                    return VimOutcome::stay(false);
                }
                '<' => {
                    self.pending_op = Some(Operator::Outdent);
                    return VimOutcome::stay(false);
                }
                _ => {}
            }
        }

        // Prefixes
        match ch {
            'g' => {
                self.pending_prefix = Some(PendingPrefix::G);
                return VimOutcome::stay(false);
            }
            'f' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::Forward));
                return VimOutcome::stay(false);
            }
            'F' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::Backward));
                return VimOutcome::stay(false);
            }
            't' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::TillForward));
                return VimOutcome::stay(false);
            }
            'T' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::TillBackward));
                return VimOutcome::stay(false);
            }
            'r' if self.pending_op.is_none() => {
                self.pending_prefix = Some(PendingPrefix::Replace);
                return VimOutcome::stay(false);
            }
            '[' => {
                self.pending_prefix = Some(PendingPrefix::Bracket { next: false });
                return VimOutcome::stay(false);
            }
            ']' => {
                self.pending_prefix = Some(PendingPrefix::Bracket { next: true });
                return VimOutcome::stay(false);
            }
            ' ' if self.pending_op.is_none() => {
                self.pending_prefix = Some(PendingPrefix::Space);
                return VimOutcome::stay(false);
            }
            _ => {}
        }

        // Motions (shared with operator-pending mode)
        if let Some(m) = char_to_motion(ch, self.last_find) {
            return self.dispatch_motion(core, m, count);
        }

        // If an operator was pending and the key isn't a motion, cancel.
        if self.pending_op.is_some() {
            self.clear_pending();
            return VimOutcome::stay(false);
        }

        // Normal-only commands
        let keys = std::mem::take(&mut self.cmd_buf);
        match ch {
            's' => {
                self.flash = FlashState { active: true, ..Default::default() };
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::StartFlash)
            }
            'i' => {
                self.begin_insert(keys);
                VimOutcome::stay(true)
            }
            'I' => {
                self.apply_motion(core, Motion::LineFirstNonBlank, 1, false);
                self.begin_insert(keys);
                VimOutcome::stay(true)
            }
            'a' => {
                let h = core.sel.head;
                if let Some(t) = core.text_of(h.block) {
                    let (_, le) = line_bounds(&t.text, h.offset);
                    if h.offset < le {
                        let next = next_grapheme(&t.text, h.offset).min(le);
                        core.sel = Selection::caret(Pos::new(h.block, next));
                    }
                }
                self.begin_insert(keys);
                VimOutcome::stay(true)
            }
            'A' => {
                let h = core.sel.head;
                if let Some(t) = core.text_of(h.block) {
                    let (_, le) = line_bounds(&t.text, h.offset);
                    core.sel = Selection::caret(Pos::new(h.block, le));
                }
                self.begin_insert(keys);
                VimOutcome::stay(true)
            }
            'o' => {
                self.open_line(core, true);
                self.begin_insert(keys);
                VimOutcome::stay(true)
            }
            'O' => {
                self.open_line(core, false);
                self.begin_insert(keys);
                VimOutcome::stay(true)
            }
            'v' => {
                self.mode = VimMode::VisualChar;
                core.sel = Selection::caret(core.sel.head);
                self.clear_pending();
                VimOutcome::stay(true)
            }
            'V' => {
                self.mode = VimMode::VisualLine;
                core.sel = Selection::caret(core.sel.head);
                self.clear_pending();
                VimOutcome::stay(true)
            }
            'x' => {
                let changed = self.exec_delete_chars(core, count, true);
                self.finish_edit(keys, core);
                VimOutcome::stay(changed)
            }
            'X' => {
                let changed = self.exec_delete_chars(core, count, false);
                self.finish_edit(keys, core);
                VimOutcome::stay(changed)
            }
            'D' => {
                let changed = self.dispatch_operator_to_eol(core, Operator::Delete);
                self.finish_edit(keys, core);
                VimOutcome::stay(changed)
            }
            'C' => {
                let changed = self.dispatch_operator_to_eol(core, Operator::Change);
                self.begin_insert(keys);
                VimOutcome::stay(changed)
            }
            'Y' => {
                self.exec_line_operator(core, Operator::Yank, count);
                self.clear_pending();
                VimOutcome::stay(false)
            }
            'S' => {
                let changed = self.exec_line_operator(core, Operator::Change, count);
                self.begin_insert(keys);
                VimOutcome::stay(changed)
            }
            'p' => {
                let changed = self.exec_paste(core, true);
                self.finish_edit(keys, core);
                VimOutcome::stay(changed)
            }
            'P' => {
                let changed = self.exec_paste(core, false);
                self.finish_edit(keys, core);
                VimOutcome::stay(changed)
            }
            'J' => {
                let changed = self.exec_join(core);
                self.finish_edit(keys, core);
                VimOutcome::stay(changed)
            }
            'u' => {
                self.clear_pending();
                let ok = core.undo();
                self.clamp_normal_caret(core);
                VimOutcome::stay(ok)
            }
            '.' => {
                self.clear_pending();
                let changed = self.replay_last_change(core);
                VimOutcome::stay(changed)
            }
            '/' => {
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::OpenFind)
            }
            'n' => {
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::FindNext(true))
            }
            'N' => {
                self.clear_pending();
                VimOutcome::cmd(VimViewCmd::FindNext(false))
            }
            _ => {
                self.clear_pending();
                VimOutcome::stay(false)
            }
        }
    }

    fn handle_visual_char_key(&mut self, core: &mut Core, ch: char) -> VimOutcome {
        let count = self.count.unwrap_or(1);
        match ch {
            'i' | 'a' if self.mode == VimMode::VisualChar => {
                self.pending_prefix = Some(PendingPrefix::TextObj { inner: ch == 'i' });
                return VimOutcome::stay(false);
            }
            'g' => {
                self.pending_prefix = Some(PendingPrefix::G);
                return VimOutcome::stay(false);
            }
            '[' => {
                self.pending_prefix = Some(PendingPrefix::Bracket { next: false });
                return VimOutcome::stay(false);
            }
            ']' => {
                self.pending_prefix = Some(PendingPrefix::Bracket { next: true });
                return VimOutcome::stay(false);
            }
            'f' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::Forward));
                return VimOutcome::stay(false);
            }
            'F' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::Backward));
                return VimOutcome::stay(false);
            }
            't' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::TillForward));
                return VimOutcome::stay(false);
            }
            'T' => {
                self.pending_prefix = Some(PendingPrefix::Find(FindKind::TillBackward));
                return VimOutcome::stay(false);
            }
            's' => {
                self.flash = FlashState { active: true, visual: Some(self.mode), ..Default::default() };
                self.clear_pending();
                return VimOutcome::cmd(VimViewCmd::StartFlash);
            }
            'v' => {
                if self.mode == VimMode::VisualChar {
                    self.enter_normal(core);
                } else {
                    self.mode = VimMode::VisualChar;
                }
                self.clear_pending();
                return VimOutcome::stay(true);
            }
            'V' => {
                if self.mode == VimMode::VisualLine {
                    self.enter_normal(core);
                } else {
                    self.mode = VimMode::VisualLine;
                }
                self.clear_pending();
                return VimOutcome::stay(true);
            }
            'o' => {
                core.sel = Selection { anchor: core.sel.head, head: core.sel.anchor };
                self.clear_pending();
                return VimOutcome::stay(true);
            }
            'd' | 'x' | 'y' | 'c' | '>' | '<' => {
                let op = match ch {
                    'd' | 'x' => Operator::Delete,
                    'y' => Operator::Yank,
                    'c' => Operator::Change,
                    '>' => Operator::Indent,
                    _ => Operator::Outdent,
                };
                let linewise = self.mode == VimMode::VisualLine;
                let (a, b) = core.ordered();
                // VisualChar is inclusive of the character under head; VisualLine covers full lines.
                let (start, end) = if linewise {
                    (line_start_pos(core, a), line_end_pos(core, b))
                } else {
                    (a, inclusive_visual_end(core, a, b))
                };
                let keys = std::mem::take(&mut self.cmd_buf);
                let changed = self.apply_operator_range(core, op, (start, end), linewise);
                if op == Operator::Change {
                    self.begin_insert(keys);
                } else {
                    self.enter_normal(core);
                    if op != Operator::Yank {
                        self.last_change = (!self.replaying).then_some(LastChange::Keys(keys));
                    }
                }
                return VimOutcome::stay(changed);
            }
            _ => {}
        }

        if let Some(m) = char_to_motion(ch, self.last_find) {
            return self.dispatch_motion(core, m, count);
        }

        self.clear_pending();
        VimOutcome::stay(false)
    }

    fn dispatch_motion(&mut self, core: &mut Core, m: Motion, count: usize) -> VimOutcome {
        if let Some(op) = self.pending_op {
            let start = core.sel.head;
            let (end, inclusive, linewise) = self.compute_motion_target(core, start, &m, count);
            let range = ordered_operator_range(core, start, end, inclusive, linewise);
            let keys = std::mem::take(&mut self.cmd_buf);
            let changed = self.apply_operator_range(core, op, range, linewise);
            if op == Operator::Change {
                self.begin_insert(keys);
            } else {
                self.finish_edit(keys, core);
            }
            return VimOutcome::stay(changed);
        }

        let extend = matches!(self.mode, VimMode::VisualChar | VimMode::VisualLine);
        if matches!(m, Motion::Up | Motion::Down) {
            let down = matches!(m, Motion::Down);
            self.clear_pending();
            return VimOutcome::cmd(VimViewCmd::MoveVertical { count, down, extend });
        }

        self.apply_motion(core, m, count, extend);
        self.clear_pending();
        VimOutcome::stay(true)
    }

    /// After the view finishes a `MoveVertical`, keep `head` clamped onto valid characters.
    pub fn post_vertical_move(&mut self, core: &mut Core) {
        if self.mode != VimMode::Insert {
            self.clamp_normal_caret(core);
        }
    }

    fn apply_motion(&self, core: &mut Core, m: Motion, count: usize, extend: bool) {
        let mut p = core.sel.head;
        let clamp = self.mode != VimMode::Insert;
        for _ in 0..count.max(1) {
            p = step_motion(core, p, &m, clamp);
        }
        if extend {
            core.sel.head = p;
        } else {
            core.sel = Selection::caret(p);
        }
    }

    fn compute_motion_target(&self, core: &Core, start: Pos, m: &Motion, count: usize) -> (Pos, bool, bool) {
        let inclusive = matches!(m, Motion::WordEnd | Motion::BigWordEnd | Motion::LineEnd | Motion::FindChar(FindKind::Forward | FindKind::TillForward, _));
        let mut linewise = matches!(m, Motion::Up | Motion::Down | Motion::DocStart | Motion::DocEnd | Motion::ParagraphPrev | Motion::ParagraphNext);
        let mut p = start;
        for _ in 0..count.max(1) {
            p = step_motion(core, p, m, false);
        }
        if matches!(m, Motion::HeadingNext) && p.block != start.block && p.offset == 0 {
            let us = core.units();
            if let Some(ei) = us.iter().position(|u| u.id == p.block)
                && ei > 0
            {
                let prev = us[ei - 1].id;
                p = line_end_pos(core, Pos::new(prev, core.len_of(prev)));
                linewise = true;
            }
        }
        (p, inclusive, linewise)
    }

    fn dispatch_operator_to_eol(&mut self, core: &mut Core, op: Operator) -> bool {
        let h = core.sel.head;
        let end = line_end_pos(core, h);
        self.apply_operator_range(core, op, (h, end), false)
    }

    fn apply_operator_range(&mut self, core: &mut Core, op: Operator, range: (Pos, Pos), linewise: bool) -> bool {
        let (a, b) = order_pair(core, range.0, range.1);
        match op {
            Operator::Indent => {
                core.sel = Selection { anchor: a, head: b };
                if core.is_code(a.block) {
                    indent_code_lines(core, a, b, true);
                } else {
                    core.indent();
                }
                true
            }
            Operator::Outdent => {
                core.sel = Selection { anchor: a, head: b };
                if core.is_code(a.block) {
                    indent_code_lines(core, a, b, false);
                } else {
                    core.outdent();
                }
                true
            }
            Operator::Yank | Operator::Delete | Operator::Change => {
                if a == b && !linewise {
                    return false;
                }
                let prev_sel = core.sel;
                core.sel = Selection { anchor: a, head: b };
                let yanked_text = core.selected_text();
                let yanked_blocks = if linewise && a.block != b.block {
                    core.selected_blocks_content()
                } else if linewise && is_full_block_span(core, a, b) {
                    core.block(a.block).cloned().into_iter().collect()
                } else {
                    vec![]
                };
                self.register = Register { text: yanked_text, blocks: yanked_blocks, linewise };
                if op == Operator::Yank {
                    core.sel = if self.mode == VimMode::Normal { Selection::caret(a) } else { prev_sel };
                    return false;
                }
                // Delete or Change
                if linewise && a.block == b.block && is_full_block_span(core, a, b) && a.block != TITLE {
                    if op == Operator::Delete {
                        delete_single_block_safely(core, a.block);
                    } else {
                        core.checkpoint(EditKind::Other);
                        core.delete_selection();
                    }
                    return true;
                }
                // For multi-line block with trailing `\n` on linewise delete
                if linewise && a.block == b.block && let Some(t) = core.text_of(a.block) {
                    let del = expand_linewise_delete_in_block(&t.text, a.offset..b.offset, op == Operator::Delete);
                    core.sel = Selection { anchor: Pos::new(a.block, del.start), head: Pos::new(a.block, del.end) };
                }
                core.delete_selection();
                true
            }
        }
    }

    fn exec_line_operator(&mut self, core: &mut Core, op: Operator, count: usize) -> bool {
        let h = core.sel.head;
        match op {
            Operator::Indent => {
                if core.is_code(h.block) {
                    indent_code_lines(core, h, h, true);
                } else {
                    core.indent();
                }
                true
            }
            Operator::Outdent => {
                if core.is_code(h.block) {
                    indent_code_lines(core, h, h, false);
                } else {
                    core.outdent();
                }
                true
            }
            Operator::Yank | Operator::Delete | Operator::Change => {
                let text_has_nl = core.text_of(h.block).is_some_and(|t| t.text.contains('\n'));
                if !text_has_nl && count == 1 {
                    if h.block == TITLE {
                        let len = core.len_of(TITLE);
                        let txt = core.text_of(TITLE).map(|t| t.text.clone()).unwrap_or_default();
                        self.register = Register { text: txt, blocks: vec![], linewise: true };
                        if op == Operator::Yank {
                            return false;
                        }
                        if len > 0 {
                            core.checkpoint(EditKind::Other);
                            core.replace_in_block(TITLE, 0..len, "");
                            return true;
                        }
                        return op == Operator::Change;
                    }
                    if let Some(b) = core.block(h.block).cloned() {
                        self.register = Register {
                            text: b.text.plain_text(),
                            blocks: vec![b],
                            linewise: true,
                        };
                    }
                    if op == Operator::Yank {
                        return false;
                    }
                    if op == Operator::Delete {
                        delete_single_block_safely(core, h.block);
                    } else {
                        let len = core.len_of(h.block);
                        core.sel = Selection { anchor: Pos::new(h.block, 0), head: Pos::new(h.block, len) };
                        core.delete_selection();
                    }
                    return true;
                }
                // Logical-line operation (inside code block / soft-broken text, or count > 1).
                let start = line_start_pos(core, h);
                let mut end_pos = h;
                for _ in 1..count {
                    end_pos = step_motion(core, end_pos, &Motion::Down, false);
                }
                let end = line_end_pos(core, end_pos);
                self.apply_operator_range(core, op, (start, end), true)
            }
        }
    }

    fn exec_delete_chars(&mut self, core: &mut Core, count: usize, forward: bool) -> bool {
        let h = core.sel.head;
        let Some(t) = core.text_of(h.block) else {
            if forward && h.block != TITLE {
                if let Some(b) = core.block(h.block).cloned() {
                    self.register = Register { text: String::new(), blocks: vec![b], linewise: true };
                }
                delete_single_block_safely(core, h.block);
                return true;
            }
            return false;
        };
        let (ls, le) = line_bounds(&t.text, h.offset);
        if ls == le {
            return false;
        }
        let range = if forward {
            let mut end = h.offset;
            for _ in 0..count {
                if end < le {
                    end = next_grapheme(&t.text, end).min(le);
                }
            }
            h.offset..end
        } else {
            let mut start = h.offset;
            for _ in 0..count {
                if start > ls {
                    start = prev_grapheme(&t.text, start).max(ls);
                }
            }
            start..h.offset
        };
        if range.is_empty() {
            return false;
        }
        self.register = Register { text: t.text[range.clone()].to_string(), blocks: vec![], linewise: false };
        core.sel = Selection { anchor: Pos::new(h.block, range.start), head: Pos::new(h.block, range.end) };
        core.delete_selection();
        true
    }

    fn exec_replace_char(&self, core: &mut Core, ch: char, count: usize) -> bool {
        let h = core.sel.head;
        let Some(t) = core.text_of(h.block) else { return false };
        let (_, le) = line_bounds(&t.text, h.offset);
        let mut end = h.offset;
        let mut n = 0;
        for _ in 0..count {
            if end < le {
                end = next_grapheme(&t.text, end).min(le);
                n += 1;
            }
        }
        if n < count || n == 0 {
            return false;
        }
        let s: String = std::iter::repeat_n(ch, n).collect();
        core.checkpoint(EditKind::Other);
        core.replace_in_block(h.block, h.offset..end, &s);
        let final_off = prev_grapheme(&core.text_of(h.block).unwrap().text, h.offset + s.len()).max(h.offset);
        core.sel = Selection::caret(Pos::new(h.block, final_off));
        true
    }

    fn exec_paste(&self, core: &mut Core, after: bool) -> bool {
        let h = core.sel.head;
        if self.register.linewise {
            if !self.register.blocks.is_empty() && h.block != TITLE && !core.is_code(h.block) {
                let mut blocks: Vec<Block> = self.register.blocks.iter().map(|b| b.fresh_ids()).collect();
                if core.is_in_table(h.block) {
                    blocks.retain(|b| !matches!(b.kind, BlockKind::Table(_)));
                }
                if blocks.is_empty() {
                    return false;
                }
                let first_id = blocks[0].id;
                let place = if after { DropPlace::After } else { DropPlace::Before };
                core.insert_blocks_at(blocks, h.block, place);
                core.sel = Selection::caret(Pos::new(first_id, 0));
                return true;
            }
            if let Some(t) = core.text_of(h.block) {
                let (ls, le) = line_bounds(&t.text, h.offset);
                let line = self.register.text.trim_end_matches('\n');
                if h.block != TITLE && !core.is_code(h.block) && !t.text.contains('\n') {
                    let b = Block::text(line);
                    let id = b.id;
                    let place = if after { DropPlace::After } else { DropPlace::Before };
                    core.insert_blocks_at(vec![b], h.block, place);
                    core.sel = Selection::caret(Pos::new(id, 0));
                    return true;
                }
                core.checkpoint(EditKind::Other);
                if after {
                    let ins = format!("\n{line}");
                    core.replace_in_block(h.block, le..le, &ins);
                    core.sel = Selection::caret(Pos::new(h.block, le + 1));
                } else {
                    let ins = format!("{line}\n");
                    core.replace_in_block(h.block, ls..ls, &ins);
                    core.sel = Selection::caret(Pos::new(h.block, ls));
                }
                return true;
            }
            return false;
        }
        if self.register.text.is_empty() {
            return false;
        }
        let Some(t) = core.text_of(h.block) else { return false };
        let (_, le) = line_bounds(&t.text, h.offset);
        let at = if after && h.offset < le { next_grapheme(&t.text, h.offset).min(le) } else { h.offset };
        core.sel = Selection::caret(Pos::new(h.block, at));
        core.checkpoint(EditKind::Other);
        core.insert_text(&self.register.text);
        let end = core.sel.head.offset;
        if let Some(t2) = core.text_of(h.block)
            && end > at
        {
            core.sel = Selection::caret(Pos::new(h.block, prev_grapheme(&t2.text, end)));
        }
        true
    }

    fn exec_join(&self, core: &mut Core) -> bool {
        let h = core.sel.head;
        let Some(t) = core.text_of(h.block) else { return false };
        let (_, le) = line_bounds(&t.text, h.offset);
        if le < t.len() && t.text[le..].starts_with('\n') {
            // Join two logical lines inside the same block.
            let next_start = le + 1;
            let ws = t.text[next_start..].bytes().take_while(|b| *b == b' ' || *b == b'\t').count();
            let need_space = le > 0 && !t.text[..le].ends_with(' ') && next_start + ws < t.len() && !t.text[next_start + ws..].starts_with('\n');
            core.checkpoint(EditKind::Other);
            core.replace_in_block(h.block, le..next_start + ws, if need_space { " " } else { "" });
            core.sel = Selection::caret(Pos::new(h.block, le));
            return true;
        }
        // Otherwise merge the next block via Core::delete_forward at the end of this block.
        let us = core.units();
        let Some(i) = us.iter().position(|u| u.id == h.block) else { return false };
        let Some(next) = us.get(i + 1) else { return false };
        if !next.text || next.scope != us[i].scope || core.is_code(h.block) || core.is_code(next.id) || h.block == TITLE {
            return false;
        }
        let next_nonempty = core.len_of(next.id) > 0;
        let prev_ends_space = t.text.ends_with(' ');
        core.sel = Selection::caret(Pos::new(h.block, le));
        core.delete_forward();
        if le > 0 && next_nonempty && !prev_ends_space {
            core.insert_text(" ");
            core.sel = Selection::caret(Pos::new(h.block, le));
        }
        true
    }

    fn open_line(&self, core: &mut Core, below: bool) {
        let h = core.sel.head;
        if h.block == TITLE {
            if below {
                let len = core.len_of(TITLE);
                core.sel = Selection::caret(Pos::new(TITLE, len));
                core.enter();
            }
            return;
        }
        if core.is_code(h.block) {
            let text = core.text_of(h.block).map(|t| t.text.clone()).unwrap_or_default();
            let (ls, le) = line_bounds(&text, h.offset);
            let indent: String = text[ls..le].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            core.checkpoint(EditKind::Other);
            if below {
                let ins = format!("\n{indent}");
                core.replace_in_block(h.block, le..le, &ins);
                core.sel = Selection::caret(Pos::new(h.block, le + ins.len()));
            } else {
                let ins = format!("{indent}\n");
                core.replace_in_block(h.block, ls..ls, &ins);
                core.sel = Selection::caret(Pos::new(h.block, ls + indent.len()));
            }
            return;
        }
        if below {
            let non_empty = core.text_of(h.block).is_some_and(|t| !t.is_empty());
            if non_empty {
                let len = core.len_of(h.block);
                core.sel = Selection::caret(Pos::new(h.block, len));
                core.enter();
            } else {
                let nid = core.insert_paragraph_after(h.block);
                core.sel = Selection::caret(Pos::new(nid, 0));
            }
        } else {
            let b = Block::text("");
            let nid = b.id;
            core.insert_blocks_at(vec![b], h.block, DropPlace::Before);
            core.sel = Selection::caret(Pos::new(nid, 0));
        }
    }

    fn replay_last_change(&mut self, core: &mut Core) -> bool {
        let Some(last) = self.last_change.clone() else { return false };
        self.replaying = true;
        let changed = match last {
            LastChange::Keys(keys) => {
                let mut any = false;
                for k in keys {
                    any |= self.handle_key(core, k).changed;
                }
                any
            }
            LastChange::Insert { opener, inserted } => {
                for k in opener {
                    self.handle_key(core, k);
                }
                if !inserted.is_empty() {
                    core.insert_text(&inserted);
                }
                self.enter_normal(core);
                true
            }
        };
        self.replaying = false;
        changed
    }

    // ---- Flash jump

    /// Recompute visible Flash matches and conflict-free jump labels.
    pub fn update_flash(&mut self, visible: &[(crate::model::BlockId, &RichText, Range<usize>)], caret: Pos) {
        self.flash.targets = compute_flash_targets(visible, caret, &self.flash.query);
    }

    /// Handle one key in Flash mode. Returns `Some(jump_done)` if consumed, `None` if ignored.
    /// When `needs_refresh` is set on the returned outcome, caller recomputes targets from visible slices.
    pub fn handle_flash_key(&mut self, core: &mut Core, key: &VimKey) -> FlashKeyResult {
        if !self.flash.active {
            return FlashKeyResult::Ignored;
        }
        if key.key == "escape" || (key.ctrl && key.key == "[") {
            let back = self.flash.visual.unwrap_or(VimMode::Normal);
            self.flash = FlashState::default();
            self.mode = back;
            return FlashKeyResult::Cancelled;
        }
        if key.key == "backspace" {
            self.flash.query.pop();
            return FlashKeyResult::Refresh;
        }
        let Some(ch) = key.ch else {
            return FlashKeyResult::Stay;
        };
        if key.ctrl || key.alt {
            return FlashKeyResult::Stay;
        }
        // Check if `ch` directly matches a unique label (or single-char prefix of a two-char label).
        if !self.flash.query.is_empty() {
            let mut exact = None;
            let mut prefix_hits = Vec::new();
            for t in &self.flash.targets {
                if t.label.len() == 1 && t.label.starts_with(ch) {
                    exact = Some(t.clone());
                    break;
                }
                if t.label.starts_with(ch) {
                    prefix_hits.push(t.clone());
                }
            }
            if let Some(target) = exact {
                return self.finish_flash_jump(core, &target);
            }
            if !prefix_hits.is_empty() {
                // Narrow down to targets starting with this two-char label prefix, stripping the first char.
                self.flash.targets = prefix_hits
                    .into_iter()
                    .map(|mut t| {
                        t.label.remove(0);
                        t
                    })
                    .collect();
                if self.flash.targets.len() == 1 && self.flash.targets[0].label.is_empty() {
                    let target = self.flash.targets[0].clone();
                    return self.finish_flash_jump(core, &target);
                }
                return FlashKeyResult::Stay;
            }
        }
        self.flash.query.push(ch);
        FlashKeyResult::Refresh
    }

    fn finish_flash_jump(&mut self, core: &mut Core, target: &FlashTarget) -> FlashKeyResult {
        let op = self.flash.op;
        let visual = self.flash.visual;
        self.flash = FlashState::default();
        let dest = Pos::new(target.block, target.range.start);
        if let Some(op) = op {
            let start = core.sel.head;
            let range = order_pair(core, start, dest);
            let changed = self.apply_operator_range(core, op, range, false);
            if op == Operator::Change {
                self.begin_insert(vec![]);
            } else {
                self.finish_edit(vec![], core);
            }
            return FlashKeyResult::Jumped { changed };
        }
        if let Some(vm) = visual {
            self.mode = vm;
            core.sel.head = dest;
            return FlashKeyResult::Jumped { changed: true };
        }
        core.sel = Selection::caret(dest);
        self.clamp_normal_caret(core);
        FlashKeyResult::Jumped { changed: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashKeyResult {
    Ignored,
    Stay,
    Refresh,
    Cancelled,
    Jumped { changed: bool },
}

// ------------------------------------------------------------------ Flash algorithm

const FLASH_LABELS: &str = "asdghklqwertyuiopzxcvbnmfj";

/// Find all matches of `query` inside the given visible block ranges, sort by
/// distance to `caret`, and assign labels that never collide with continuation chars.
pub fn compute_flash_targets(
    visible: &[(crate::model::BlockId, &RichText, Range<usize>)],
    caret: Pos,
    query: &str,
) -> Vec<FlashTarget> {
    if query.is_empty() {
        return vec![];
    }
    let smart_case = query.chars().any(|c| c.is_uppercase());
    let q_chars: Vec<char> = query
        .chars()
        .map(|c| if smart_case { c } else { c.to_lowercase().next().unwrap_or(c) })
        .collect();

    struct RawHit {
        block: crate::model::BlockId,
        range: Range<usize>,
        next_char: Option<char>,
        dist: usize,
    }

    let caret_bi = visible.iter().position(|(id, _, _)| *id == caret.block);
    let mut hits = Vec::new();
    for (bi, (id, rt, vis_range)) in visible.iter().enumerate() {
        let raw_inlines: Vec<Range<usize>> = rt.spans().filter(|(_, m)| m.inline.is_some()).map(|(r, _)| r).collect();
        let chars: Vec<(usize, char)> = rt.text.char_indices().collect();
        let mut i = 0;
        while i + q_chars.len() <= chars.len() {
            let start = chars[i].0;
            let end = chars.get(i + q_chars.len()).map_or(rt.len(), |(o, _)| *o);
            if start >= vis_range.start && end <= vis_range.end {
                let matched = chars[i..i + q_chars.len()].iter().zip(&q_chars).all(|((_, c), q)| {
                    let lc = if smart_case { *c } else { c.to_lowercase().next().unwrap_or(*c) };
                    lc == *q
                });
                if matched && !raw_inlines.iter().any(|r| r.start < end && start < r.end) {
                    let next_char = chars.get(i + q_chars.len()).map(|(_, c)| *c);
                    let dist = match caret_bi {
                        Some(cb) if cb == bi => start.abs_diff(caret.offset),
                        Some(cb) => cb.abs_diff(bi) * 10_000 + start,
                        None => bi * 10_000 + start,
                    };
                    hits.push(RawHit { block: *id, range: start..end, next_char, dist });
                    i += q_chars.len();
                    continue;
                }
            }
            i += 1;
        }
    }
    hits.sort_by_key(|h| h.dist);

    // Exclude characters that appear immediately after any match so continuing to type
    // `query` never misfires as a label jump.
    let mut forbidden = Vec::<char>::new();
    for h in &hits {
        if let Some(nc) = h.next_char {
            if !forbidden.contains(&nc) {
                forbidden.push(nc);
            }
            let lower = nc.to_lowercase().next().unwrap_or(nc);
            if !forbidden.contains(&lower) {
                forbidden.push(lower);
            }
        }
    }

    let avail: Vec<char> = FLASH_LABELS.chars().filter(|c| !forbidden.contains(c)).collect();
    if avail.is_empty() {
        return vec![];
    }

    let n = hits.len();
    let mut labels = Vec::with_capacity(n);
    if n <= avail.len() {
        for &c in avail.iter().take(n) {
            labels.push(c.to_string());
        }
    } else {
        // Reserve enough prefix letters for two-char labels when hits exceed single labels.
        let group_size = avail.len();
        let two_groups = ((n - avail.len()) + group_size - 1) / (group_size - 1).max(1);
        let single_count = avail.len().saturating_sub(two_groups);
        for &c in avail.iter().take(single_count) {
            labels.push(c.to_string());
        }
        'outer: for &p in avail.iter().skip(single_count) {
            for &s in &avail {
                if labels.len() >= n {
                    break 'outer;
                }
                labels.push(format!("{p}{s}"));
            }
        }
    }

    hits.into_iter()
        .zip(labels)
        .map(|(h, label)| FlashTarget { block: h.block, range: h.range, label })
        .collect()
}

// ------------------------------------------------------------------ helpers

pub fn line_bounds(text: &str, offset: usize) -> (usize, usize) {
    let off = offset.min(text.len());
    let start = text[..off].rfind('\n').map_or(0, |i| i + 1);
    let end = text[off..].find('\n').map_or(text.len(), |i| off + i);
    (start, end)
}

fn line_start_pos(core: &Core, p: Pos) -> Pos {
    let Some(t) = core.text_of(p.block) else { return Pos::new(p.block, 0) };
    let (ls, _) = line_bounds(&t.text, p.offset);
    Pos::new(p.block, ls)
}

fn line_end_pos(core: &Core, p: Pos) -> Pos {
    let Some(t) = core.text_of(p.block) else { return Pos::new(p.block, 0) };
    let (_, le) = line_bounds(&t.text, p.offset);
    Pos::new(p.block, le)
}

fn prev_pos_in_block(core: &Core, p: Pos) -> Pos {
    let Some(t) = core.text_of(p.block) else { return p };
    let (ls, _) = line_bounds(&t.text, p.offset);
    if p.offset > ls {
        Pos::new(p.block, prev_grapheme(&t.text, p.offset).max(ls))
    } else {
        p
    }
}

fn inclusive_visual_end(core: &Core, a: Pos, b: Pos) -> Pos {
    let _ = a;
    let Some(t) = core.text_of(b.block) else { return b };
    let (_, le) = line_bounds(&t.text, b.offset);
    if b.offset < le {
        Pos::new(b.block, next_grapheme(&t.text, b.offset).min(le))
    } else {
        b
    }
}

fn char_to_motion(ch: char, last_find: Option<(FindKind, char)>) -> Option<Motion> {
    Some(match ch {
        'h' => Motion::Left,
        'l' => Motion::Right,
        'j' => Motion::Down,
        'k' => Motion::Up,
        'w' => Motion::WordStart,
        'W' => Motion::BigWordStart,
        'b' => Motion::WordBack,
        'B' => Motion::BigWordBack,
        'e' => Motion::WordEnd,
        'E' => Motion::BigWordEnd,
        '0' => Motion::LineStart,
        '^' => Motion::LineFirstNonBlank,
        '$' => Motion::LineEnd,
        'G' => Motion::DocEnd,
        '{' => Motion::ParagraphPrev,
        '}' => Motion::ParagraphNext,
        ';' => {
            let (kind, c) = last_find?;
            Motion::FindChar(kind, c)
        }
        ',' => {
            let (kind, c) = last_find?;
            let rev = match kind {
                FindKind::Forward => FindKind::Backward,
                FindKind::Backward => FindKind::Forward,
                FindKind::TillForward => FindKind::TillBackward,
                FindKind::TillBackward => FindKind::TillForward,
            };
            Motion::FindChar(rev, c)
        }
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

fn classify(c: char, big_word: bool) -> CharClass {
    if c.is_whitespace() {
        CharClass::Space
    } else if big_word || c.is_alphanumeric() || c == '_' || c == '\u{FFFC}' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

fn vim_next_word(s: &str, offset: usize, big_word: bool) -> usize {
    let mut it = s[offset..].grapheme_indices(true).map(|(i, g)| (offset + i, g));
    let Some((_, first)) = it.next() else { return s.len() };
    let cls = classify(first.chars().next().unwrap_or(' '), big_word);
    let mut saw_space = cls == CharClass::Space;
    for (i, g) in it {
        let c = classify(g.chars().next().unwrap_or(' '), big_word);
        if c == CharClass::Space {
            saw_space = true;
        } else if saw_space || c != cls {
            return i;
        }
    }
    s.len()
}

fn vim_prev_word(s: &str, offset: usize, big_word: bool) -> usize {
    let mut gs: Vec<(usize, &str)> = s[..offset].grapheme_indices(true).collect();
    while gs.last().is_some_and(|(_, g)| g.chars().next().is_some_and(char::is_whitespace)) {
        gs.pop();
    }
    let Some((_, last)) = gs.pop() else { return 0 };
    let cls = classify(last.chars().next().unwrap_or(' '), big_word);
    let mut start = gs.last().map_or(0, |(i, _)| *i);
    if gs.is_empty() {
        return 0;
    }
    // Walk back while same non-space class.
    let mut idx = gs.len();
    while idx > 0 {
        let (pos, g) = gs[idx - 1];
        if classify(g.chars().next().unwrap_or(' '), big_word) != cls {
            return start;
        }
        start = pos;
        idx -= 1;
    }
    0
}

fn vim_word_end(s: &str, offset: usize, big_word: bool) -> usize {
    let mut gs: Vec<(usize, &str)> = s[offset..].grapheme_indices(true).map(|(i, g)| (offset + i, g)).collect();
    if gs.is_empty() {
        return s.len();
    }
    // Step past current char first, like Vim `e`.
    gs.remove(0);
    let mut it = gs.into_iter().peekable();
    while it.peek().is_some_and(|(_, g)| g.chars().next().is_some_and(char::is_whitespace)) {
        it.next();
    }
    let Some((mut cur_pos, first)) = it.next() else { return s.len() };
    let cls = classify(first.chars().next().unwrap_or(' '), big_word);
    for (i, g) in it {
        if classify(g.chars().next().unwrap_or(' '), big_word) != cls {
            break;
        }
        cur_pos = i;
    }
    cur_pos
}

fn is_heading_unit(core: &Core, id: crate::model::BlockId) -> bool {
    id == TITLE || matches!(core.block(id).map(|b| &b.kind), Some(BlockKind::Heading { .. }))
}

fn step_motion(core: &Core, p: Pos, m: &Motion, clamp_normal: bool) -> Pos {
    let us = core.units();
    let Some(ui) = us.iter().position(|u| u.id == p.block) else { return p };
    let text_opt = core.text_of(p.block);
    match m {
        Motion::Left => {
            let Some(t) = text_opt else { return p };
            let (ls, _) = line_bounds(&t.text, p.offset);
            Pos::new(p.block, if p.offset > ls { prev_grapheme(&t.text, p.offset).max(ls) } else { ls })
        }
        Motion::Right => {
            let Some(t) = text_opt else { return p };
            let (ls, le) = line_bounds(&t.text, p.offset);
            let limit = if clamp_normal && le > ls { prev_grapheme(&t.text, le).max(ls) } else { le };
            Pos::new(p.block, if p.offset < limit { next_grapheme(&t.text, p.offset).min(limit) } else { limit })
        }
        Motion::Up => {
            if let Some(t) = text_opt {
                let (ls, _) = line_bounds(&t.text, p.offset);
                if ls > 0 {
                    let (pls, _) = line_bounds(&t.text, ls - 1);
                    return Pos::new(p.block, pls);
                }
            }
            ui.checked_sub(1).map_or(p, |k| Pos::new(us[k].id, 0))
        }
        Motion::Down => {
            if let Some(t) = text_opt {
                let (_, le) = line_bounds(&t.text, p.offset);
                if le < t.len() && t.text[le..].starts_with('\n') {
                    return Pos::new(p.block, le + 1);
                }
            }
            us.get(ui + 1).map_or(p, |u| Pos::new(u.id, 0))
        }
        Motion::LineStart => {
            let Some(t) = text_opt else { return p };
            let (ls, _) = line_bounds(&t.text, p.offset);
            Pos::new(p.block, ls)
        }
        Motion::LineFirstNonBlank => {
            let Some(t) = text_opt else { return p };
            let (ls, le) = line_bounds(&t.text, p.offset);
            let skip = t.text[ls..le].grapheme_indices(true).find(|(_, g)| !g.chars().all(char::is_whitespace)).map_or(0, |(i, _)| i);
            Pos::new(p.block, ls + skip)
        }
        Motion::LineEnd => {
            let Some(t) = text_opt else { return p };
            let (ls, le) = line_bounds(&t.text, p.offset);
            let target = if clamp_normal && le > ls { prev_grapheme(&t.text, le).max(ls) } else { le };
            Pos::new(p.block, target)
        }
        Motion::WordStart | Motion::BigWordStart => {
            let big = matches!(m, Motion::BigWordStart);
            if let Some(t) = text_opt {
                let (_, le) = line_bounds(&t.text, p.offset);
                if p.offset < le {
                    let nw = vim_next_word(&t.text, p.offset, big);
                    if nw < le || !clamp_normal {
                        return Pos::new(p.block, nw.min(le));
                    }
                }
                if le < t.len() && t.text[le..].starts_with('\n') {
                    let next_ls = le + 1;
                    let skip = t.text[next_ls..].grapheme_indices(true).take_while(|(_, g)| *g != "\n" && g.chars().all(char::is_whitespace)).last().map_or(0, |(i, g)| i + g.len());
                    return Pos::new(p.block, (next_ls + skip).min(t.len()));
                }
            }
            if let Some(n) = us.get(ui + 1) { Pos::new(n.id, 0) } else { p }
        }
        Motion::WordBack | Motion::BigWordBack => {
            let big = matches!(m, Motion::BigWordBack);
            if let Some(t) = text_opt {
                let (ls, _) = line_bounds(&t.text, p.offset);
                if p.offset > ls {
                    let pw = vim_prev_word(&t.text, p.offset, big);
                    if pw >= ls {
                        return Pos::new(p.block, pw);
                    }
                }
                if ls > 0 {
                    return Pos::new(p.block, vim_prev_word(&t.text, ls - 1, big));
                }
            }
            if let Some(prev_u) = ui.checked_sub(1).map(|k| us[k]) {
                let len = core.len_of(prev_u.id);
                let off = core.text_of(prev_u.id).map_or(0, |t| if len > 0 { vim_prev_word(&t.text, len, big) } else { 0 });
                Pos::new(prev_u.id, off)
            } else {
                p
            }
        }
        Motion::WordEnd | Motion::BigWordEnd => {
            let big = matches!(m, Motion::BigWordEnd);
            if let Some(t) = text_opt {
                let (_, le) = line_bounds(&t.text, p.offset);
                let we = vim_word_end(&t.text, p.offset, big);
                if we < le {
                    return Pos::new(p.block, we);
                }
                if le < t.len() && t.text[le..].starts_with('\n') {
                    return Pos::new(p.block, vim_word_end(&t.text, le, big));
                }
            }
            if let Some(n) = us.get(ui + 1) {
                let off = core.text_of(n.id).map_or(0, |t| vim_word_end(&t.text, 0, big).min(t.len()));
                Pos::new(n.id, off)
            } else {
                p
            }
        }
        Motion::DocStart => {
            let first = us.get(1).unwrap_or(&us[0]);
            Pos::new(first.id, 0)
        }
        Motion::DocEnd => {
            let last = us.last().unwrap_or(&us[0]);
            Pos::new(last.id, 0)
        }
        Motion::ParagraphNext => {
            for k in ui + 1..us.len() {
                if core.text_of(us[k].id).is_some_and(|t| t.is_empty()) {
                    return Pos::new(us[k].id, 0);
                }
            }
            let last = us.last().unwrap_or(&us[0]);
            Pos::new(last.id, 0)
        }
        Motion::ParagraphPrev => {
            for k in (0..ui).rev() {
                if core.text_of(us[k].id).is_some_and(|t| t.is_empty()) {
                    return Pos::new(us[k].id, 0);
                }
            }
            let first = us.get(1).unwrap_or(&us[0]);
            Pos::new(first.id, 0)
        }
        Motion::HeadingNext => {
            for k in ui + 1..us.len() {
                if is_heading_unit(core, us[k].id) {
                    return Pos::new(us[k].id, 0);
                }
            }
            p
        }
        Motion::HeadingPrev => {
            if p.offset > 0 && is_heading_unit(core, p.block) {
                return Pos::new(p.block, 0);
            }
            for k in (0..ui).rev() {
                if is_heading_unit(core, us[k].id) {
                    return Pos::new(us[k].id, 0);
                }
            }
            p
        }
        Motion::FindChar(kind, target) => {
            let Some(t) = text_opt else { return p };
            let (ls, le) = line_bounds(&t.text, p.offset);
            let slice = &t.text[ls..le];
            let rel = p.offset.saturating_sub(ls);
            match kind {
                FindKind::Forward | FindKind::TillForward => {
                    let after_cur = next_grapheme(slice, rel);
                    if let Some((i, _)) = slice[after_cur..].char_indices().find(|(_, c)| *c == *target) {
                        let hit = ls + after_cur + i;
                        let pos = if matches!(kind, FindKind::TillForward) { prev_grapheme(&t.text, hit).max(ls) } else { hit };
                        Pos::new(p.block, pos)
                    } else {
                        p
                    }
                }
                FindKind::Backward | FindKind::TillBackward => {
                    if let Some((i, g)) = slice[..rel].grapheme_indices(true).rfind(|(_, g)| g.starts_with(*target)) {
                        let hit = ls + i;
                        let pos = if matches!(kind, FindKind::TillBackward) { (hit + g.len()).min(le) } else { hit };
                        Pos::new(p.block, pos)
                    } else {
                        p
                    }
                }
            }
        }
    }
}

fn order_pair_in(units: &[crate::editor::core::Unit], a: Pos, b: Pos) -> (Pos, Pos) {
    let ai = units.iter().position(|u| u.id == a.block).unwrap_or(0);
    let bi = units.iter().position(|u| u.id == b.block).unwrap_or(0);
    if (ai, a.offset) <= (bi, b.offset) { (a, b) } else { (b, a) }
}

fn order_pair(core: &Core, a: Pos, b: Pos) -> (Pos, Pos) {
    order_pair_in(&core.units(), a, b)
}

fn ordered_operator_range(core: &Core, start: Pos, end: Pos, inclusive: bool, linewise: bool) -> (Pos, Pos) {
    let (a, mut b) = order_pair(core, start, end);
    if linewise {
        return (line_start_pos(core, a), line_end_pos(core, b));
    }
    if inclusive && let Some(t) = core.text_of(b.block) {
        let (_, le) = line_bounds(&t.text, b.offset);
        if b.offset < le {
            b.offset = next_grapheme(&t.text, b.offset).min(le);
        }
    }
    (a, b)
}

fn is_full_block_span(core: &Core, a: Pos, b: Pos) -> bool {
    a.block == b.block && a.offset == 0 && b.offset >= core.len_of(b.block)
}

fn delete_single_block_safely(core: &mut Core, id: crate::model::BlockId) {
    if id == TITLE {
        return;
    }
    // If deleting the last remaining body block, clear it instead of creating a new id unnecessarily,
    // or let `delete_block` normalize cleanly.
    if core.doc.blocks.len() == 1 && core.doc.blocks[0].id == id && core.doc.blocks[0].children.is_empty() && matches!(core.doc.blocks[0].kind, BlockKind::Text) {
        let len = core.len_of(id);
        if len > 0 {
            core.checkpoint(EditKind::Other);
            core.replace_in_block(id, 0..len, "");
            core.sel = Selection::caret(Pos::new(id, 0));
        }
        return;
    }
    core.delete_block(id);
}

fn expand_linewise_delete_in_block(text: &str, r: Range<usize>, delete_newline: bool) -> Range<usize> {
    if !delete_newline {
        return r;
    }
    if r.end < text.len() && text[r.end..].starts_with('\n') {
        r.start..r.end + 1
    } else if r.start > 0 && text[..r.start].ends_with('\n') {
        r.start - 1..r.end
    } else {
        r
    }
}

fn indent_code_lines(core: &mut Core, a: Pos, b: Pos, indent: bool) {
    let (a, b) = order_pair(core, a, b);
    let Some(t) = core.text_of(a.block).map(|r| r.text.clone()) else { return };
    let (ls, _) = line_bounds(&t, a.offset);
    let (_, le) = line_bounds(&t, b.offset);
    let slice = &t[ls..le];
    let out: String = slice
        .split('\n')
        .map(|line| {
            if indent {
                format!("    {line}")
            } else {
                let n = line.chars().take(4).take_while(|c| *c == ' ').count();
                line[n..].to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    core.checkpoint(EditKind::Other);
    let mut rt = RichText::default();
    rt.push(&out, Marks::default());
    if let Some(tm) = core.text_mut(a.block) {
        tm.replace(ls..le, &rt);
    }
    core.sel = Selection::caret(Pos::new(a.block, ls));
}

fn resolve_text_object(core: &Core, ch: char, inner: bool) -> Option<(Pos, Pos)> {
    let h = core.sel.head;
    let t = core.text_of(h.block)?;
    let (ls, le) = line_bounds(&t.text, h.offset);
    let line = &t.text[ls..le];
    let rel = h.offset.saturating_sub(ls);
    let (s, e) = match ch {
        'w' | 'W' => {
            let big = ch == 'W';
            let gs: Vec<(usize, &str)> = line.grapheme_indices(true).collect();
            let idx = gs.iter().rposition(|(i, _)| *i <= rel).unwrap_or(0);
            let cls = gs.get(idx).map(|(_, g)| classify(g.chars().next().unwrap_or(' '), big))?;
            let mut l = idx;
            while l > 0 && classify(gs[l - 1].1.chars().next().unwrap_or(' '), big) == cls {
                l -= 1;
            }
            let mut r = idx;
            while r + 1 < gs.len() && classify(gs[r + 1].1.chars().next().unwrap_or(' '), big) == cls {
                r += 1;
            }
            let mut start = gs[l].0;
            let mut end = gs[r].0 + gs[r].1.len();
            if !inner && cls != CharClass::Space {
                let after = gs[r + 1..].iter().take_while(|(_, g)| g.chars().all(char::is_whitespace)).map(|(_, g)| g.len()).sum::<usize>();
                if after > 0 {
                    end += after;
                } else {
                    let before = gs[..l].iter().rev().take_while(|(_, g)| g.chars().all(char::is_whitespace)).map(|(_, g)| g.len()).sum::<usize>();
                    start -= before;
                }
            }
            (start, end)
        }
        '"' | '\'' | '`' => find_quote_pair(line, rel, ch, inner)?,
        '(' | ')' | 'b' => find_bracket_pair(line, rel, '(', ')', inner)?,
        '[' | ']' => find_bracket_pair(line, rel, '[', ']', inner)?,
        '{' | '}' | 'B' => find_bracket_pair(line, rel, '{', '}', inner)?,
        '<' | '>' => find_bracket_pair(line, rel, '<', '>', inner)?,
        _ => return None,
    };
    Some((Pos::new(h.block, ls + s), Pos::new(h.block, ls + e)))
}

fn find_quote_pair(line: &str, rel: usize, q: char, inner: bool) -> Option<(usize, usize)> {
    let qs: Vec<usize> = line.char_indices().filter_map(|(i, c)| (c == q).then_some(i)).collect();
    for pair in qs.as_chunks::<2>().0 {
        let (a, b) = (pair[0], pair[1]);
        if rel <= b {
            let qlen = q.len_utf8();
            return Some(if inner { (a + qlen, b) } else { (a, b + qlen) });
        }
    }
    None
}

fn find_bracket_pair(line: &str, rel: usize, open: char, close: char, inner: bool) -> Option<(usize, usize)> {
    let mut stack = Vec::new();
    let mut best: Option<(usize, usize)> = None;
    for (i, c) in line.char_indices() {
        if c == open {
            stack.push(i);
        } else if c == close
            && let Some(a) = stack.pop()
            && a <= rel
            && rel <= i
        {
            best = Some((a, i));
            break;
        }
    }
    let (a, b) = best?;
    let olen = open.len_utf8();
    let clen = close.len_utf8();
    Some(if inner { (a + olen, b) } else { (a, b + clen) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Document;

    fn doc_with(lines: &[&str]) -> Core {
        let doc = Document {
            title: RichText::plain("标题"),
            blocks: lines.iter().map(|s| Block::text(s)).collect(),
            ..Default::default()
        };
        let mut c = Core::new(doc);
        let first = c.doc.blocks[0].id;
        c.sel = Selection::caret(Pos::new(first, 0));
        c
    }

    fn type_seq(vim: &mut VimState, core: &mut Core, seq: &str) {
        for ch in seq.chars() {
            vim.handle_key(core, VimKey::char(ch));
        }
    }

    #[test]
    fn normal_motions_and_utf8_boundaries() {
        let mut core = doc_with(&["hello 世界 👍🏽 end"]);
        let mut vim = VimState::default();
        let id = core.doc.blocks[0].id;

        type_seq(&mut vim, &mut core, "w");
        assert_eq!(core.sel.head, Pos::new(id, 6)); // "世"
        type_seq(&mut vim, &mut core, "l");
        assert_eq!(core.sel.head, Pos::new(id, 9)); // "界"
        type_seq(&mut vim, &mut core, "$");
        assert_eq!(core.sel.head, Pos::new(id, "hello 世界 👍🏽 en".len()));
        type_seq(&mut vim, &mut core, "0");
        assert_eq!(core.sel.head, Pos::new(id, 0));
        type_seq(&mut vim, &mut core, "f👍");
        assert_eq!(core.sel.head, Pos::new(id, "hello 世界 ".len()));
    }

    #[test]
    fn insert_esc_and_dot_repeat() {
        let mut core = doc_with(&["one two three"]);
        let mut vim = VimState::default();

        type_seq(&mut vim, &mut core, "ciw");
        assert_eq!(vim.mode, VimMode::Insert);
        core.insert_text("UNO");
        vim.note_inserted_text("UNO");
        vim.handle_key(&mut core, VimKey::named("escape"));
        assert_eq!(vim.mode, VimMode::Normal);
        assert_eq!(core.text_of(core.doc.blocks[0].id).unwrap().text, "UNO two three");

        type_seq(&mut vim, &mut core, "w.");
        assert_eq!(core.text_of(core.doc.blocks[0].id).unwrap().text, "UNO UNO three");
    }

    #[test]
    fn dd_yy_p_and_undo_redo() {
        let mut core = doc_with(&["first", "second"]);
        let mut vim = VimState::default();

        type_seq(&mut vim, &mut core, "dd");
        assert_eq!(core.doc.blocks.len(), 1);
        assert_eq!(core.doc.blocks[0].text.text, "second");

        type_seq(&mut vim, &mut core, "p");
        assert_eq!(core.doc.blocks.len(), 2);
        assert_eq!(core.doc.blocks[1].text.text, "first");

        type_seq(&mut vim, &mut core, "u");
        assert_eq!(core.doc.blocks.len(), 1);
        vim.handle_key(&mut core, VimKey::ctrl_char('r'));
        assert_eq!(core.doc.blocks.len(), 2);
    }

    #[test]
    fn code_block_logical_line_dd_and_o() {
        let mut core = Core::new(Document {
            title: RichText::plain("T"),
            blocks: vec![Block::new(BlockKind::Code { lang: "rust".into(), caption: None, wrap: false }, RichText::plain("  let a = 1;\n  let b = 2;"))],
            ..Default::default()
        });
        let id = core.doc.blocks[0].id;
        core.sel = Selection::caret(Pos::new(id, 0));
        let mut vim = VimState::default();

        type_seq(&mut vim, &mut core, "dd");
        assert_eq!(core.text_of(id).unwrap().text, "  let b = 2;");

        type_seq(&mut vim, &mut core, "o");
        assert_eq!(vim.mode, VimMode::Insert);
        assert_eq!(core.text_of(id).unwrap().text, "  let b = 2;\n  ");
    }

    #[test]
    fn text_objects_and_visual_mode() {
        let mut core = doc_with(&[r#"say("hello world")"#]);
        let mut vim = VimState::default();
        let id = core.doc.blocks[0].id;
        core.sel = Selection::caret(Pos::new(id, 6));

        type_seq(&mut vim, &mut core, "di\"");
        assert_eq!(core.text_of(id).unwrap().text, r#"say("")"#);

        type_seq(&mut vim, &mut core, "u");
        core.sel = Selection::caret(Pos::new(id, 4));
        type_seq(&mut vim, &mut core, "v");
        // Immediately highlights the character under the cursor on `v`.
        let us = core.units();
        assert_eq!(vim.visual_range_in(&core, id, &us), Some(4..5));
        // Backward Visual selection keeps `anchor` inclusive too.
        type_seq(&mut vim, &mut core, "h");
        assert_eq!(vim.visual_range_in(&core, id, &us), Some(3..5));
        type_seq(&mut vim, &mut core, "llllly");
        assert_eq!(vim.mode, VimMode::Normal);
        assert_eq!(vim.register.text, "\"hell");
    }

    #[test]
    fn visual_line_keeps_head_column_and_covers_full_lines() {
        let mut core = doc_with(&["alpha beta", "gamma delta", "omega"]);
        let mut vim = VimState::default();
        let b0 = core.doc.blocks[0].id;
        let b1 = core.doc.blocks[1].id;
        let b2 = core.doc.blocks[2].id;
        core.sel = Selection::caret(Pos::new(b0, 3));

        type_seq(&mut vim, &mut core, "V");
        assert_eq!(vim.mode, VimMode::VisualLine);
        assert_eq!(core.sel.head, Pos::new(b0, 3));
        let us = core.units();
        assert_eq!(vim.visual_range_in(&core, b0, &us), Some(0.."alpha beta".len()));
        assert_eq!(vim.visual_range_in(&core, b1, &us), None);

        // Extend down onto b1 at col 2; both full blocks are covered without clobbering `head`.
        core.sel.head = Pos::new(b1, 2);
        assert_eq!(vim.visual_range_in(&core, b0, &us), Some(0.."alpha beta".len()));
        assert_eq!(vim.visual_range_in(&core, b1, &us), Some(0.."gamma delta".len()));
        assert_eq!(vim.visual_range_in(&core, b2, &us), None);

        // `o` swaps endpoints, `v` converts cleanly back to charwise preserving true offsets.
        type_seq(&mut vim, &mut core, "ov");
        assert_eq!(vim.mode, VimMode::VisualChar);
        assert_eq!(core.sel.anchor, Pos::new(b1, 2));
        assert_eq!(core.sel.head, Pos::new(b0, 3));
        assert_eq!(vim.visual_range_in(&core, b0, &us), Some(3.."alpha beta".len()));
        assert_eq!(vim.visual_range_in(&core, b1, &us), Some(0..3));
    }

    #[test]
    fn flash_targets_avoid_continuation_collisions_and_jump() {
        let mut core = doc_with(&["apple apply apricot alpha"]);
        let id = core.doc.blocks[0].id;
        let rt = core.text_of(id).unwrap().clone();
        let mut vim = VimState::default();
        vim.flash = FlashState { active: true, query: "ap".into(), ..Default::default() };
        vim.update_flash(&[(id, &rt, 0..rt.len())], Pos::new(id, 0));

        assert_eq!(vim.flash.targets.len(), 3);
        // Next chars are 'p' and 'r'; labels must never use 'p' or 'r'.
        for t in &vim.flash.targets {
            assert!(!t.label.contains('p'));
            assert!(!t.label.contains('r'));
        }
        let third_label = vim.flash.targets[2].label.chars().next().unwrap();
        let res = vim.handle_flash_key(&mut core, &VimKey::char(third_label));
        assert_eq!(res, FlashKeyResult::Jumped { changed: true });
        assert_eq!(core.sel.head, Pos::new(id, 12)); // "apricot"
    }

    #[test]
    fn window_and_buffer_commands_and_dot_isolation() {
        let mut core = doc_with(&["abcdef"]);
        let mut vim = VimState::default();

        // Perform a repeatable edit (`x`) first.
        let out = vim.handle_key(&mut core, VimKey::char('x'));
        assert!(out.changed);
        assert_eq!(core.text_of(core.doc.blocks[0].id).unwrap().text, "bcdef");

        // Ctrl+w h / l / w and held-Ctrl Ctrl+w Ctrl+w.
        let out = vim.handle_key(&mut core, VimKey::ctrl_char('w'));
        assert!(out.consumed && out.view_cmd.is_none());
        assert_eq!(vim.mode_badge_text(), "NORMAL · ^W");
        assert_eq!(vim.handle_key(&mut core, VimKey::char('h')).view_cmd, Some(VimViewCmd::FocusPane(VimPane::Left)));

        vim.handle_key(&mut core, VimKey::ctrl_char('w'));
        assert_eq!(vim.handle_key(&mut core, VimKey::ctrl_char('l')).view_cmd, Some(VimViewCmd::FocusPane(VimPane::Right)));

        vim.handle_key(&mut core, VimKey::ctrl_char('w'));
        assert_eq!(vim.handle_key(&mut core, VimKey::ctrl_char('w')).view_cmd, Some(VimViewCmd::FocusPane(VimPane::Cycle)));

        // Ctrl+o / Ctrl+i and [b / ]b.
        assert_eq!(vim.handle_key(&mut core, VimKey::ctrl_char('o')).view_cmd, Some(VimViewCmd::JumpHistory { forward: false }));
        assert_eq!(vim.handle_key(&mut core, VimKey::ctrl_char('i')).view_cmd, Some(VimViewCmd::JumpHistory { forward: true }));

        vim.handle_key(&mut core, VimKey::char('['));
        assert_eq!(vim.mode_badge_text(), "NORMAL · [");
        assert_eq!(vim.handle_key(&mut core, VimKey::char('b')).view_cmd, Some(VimViewCmd::CycleBuffer { forward: false }));

        vim.handle_key(&mut core, VimKey::char(']'));
        assert_eq!(vim.mode_badge_text(), "NORMAL · ]");
        assert_eq!(vim.handle_key(&mut core, VimKey::char('b')).view_cmd, Some(VimViewCmd::CycleBuffer { forward: true }));

        // Space b / Space Space / Space o / Space h / Space e / Space ?.
        vim.handle_key(&mut core, VimKey::char(' '));
        assert_eq!(vim.mode_badge_text(), "NORMAL · ␣");
        assert_eq!(vim.handle_key(&mut core, VimKey::char('b')).view_cmd, Some(VimViewCmd::OpenBufferSwitcher));

        vim.handle_key(&mut core, VimKey::char(' '));
        assert_eq!(vim.handle_key(&mut core, VimKey::char(' ')).view_cmd, Some(VimViewCmd::OpenBufferSwitcher));

        vim.handle_key(&mut core, VimKey::char(' '));
        assert_eq!(vim.handle_key(&mut core, VimKey::char('o')).view_cmd, Some(VimViewCmd::ToggleCatalogue));

        vim.handle_key(&mut core, VimKey::char(' '));
        assert_eq!(vim.handle_key(&mut core, VimKey::char('h')).view_cmd, Some(VimViewCmd::OpenHome));

        vim.handle_key(&mut core, VimKey::char(' '));
        assert_eq!(vim.handle_key(&mut core, VimKey::char('H')).view_cmd, Some(VimViewCmd::OpenHistory));

        vim.handle_key(&mut core, VimKey::char(' '));
        assert_eq!(vim.handle_key(&mut core, VimKey::char('e')).view_cmd, Some(VimViewCmd::FocusPane(VimPane::Left)));

        vim.handle_key(&mut core, VimKey::char(' '));
        assert_eq!(vim.handle_key(&mut core, VimKey::char('?')).view_cmd, Some(VimViewCmd::ToggleShortcuts));

        // `.` must still repeat `x`, unaffected by navigation commands.
        let out = vim.handle_key(&mut core, VimKey::char('.'));
        assert!(out.changed);
        assert_eq!(core.text_of(core.doc.blocks[0].id).unwrap().text, "cdef");
    }

    #[test]
    fn heading_motions_work_across_normal_visual_and_operator() {
        let mut core = Core::new(Document {
            title: RichText::plain("Doc"),
            blocks: vec![
                Block::new(BlockKind::Heading { level: 1, seq_auto: false }, RichText::plain("H1")),
                Block::text("body one"),
                Block::new(BlockKind::Heading { level: 2, seq_auto: false }, RichText::plain("H2")),
                Block::text("body two"),
            ],
            ..Default::default()
        });
        let h1 = core.doc.blocks[0].id;
        let p1 = core.doc.blocks[1].id;
        let h2 = core.doc.blocks[2].id;
        core.sel = Selection::caret(Pos::new(h1, 0));
        let mut vim = VimState::default();

        type_seq(&mut vim, &mut core, "]h");
        assert_eq!(core.sel.head, Pos::new(h2, 0));
        type_seq(&mut vim, &mut core, "[h");
        assert_eq!(core.sel.head, Pos::new(h1, 0));
        type_seq(&mut vim, &mut core, "]]");
        assert_eq!(core.sel.head, Pos::new(h2, 0));
        type_seq(&mut vim, &mut core, "[[");
        assert_eq!(core.sel.head, Pos::new(h1, 0));
        type_seq(&mut vim, &mut core, "[[");
        assert_eq!(core.sel.head, Pos::new(TITLE, 0));

        core.sel = Selection::caret(Pos::new(p1, 0));
        type_seq(&mut vim, &mut core, "V]h");
        assert_eq!(vim.mode, VimMode::VisualLine);
        assert_eq!(core.sel.head, Pos::new(h2, 0));
        vim.handle_key(&mut core, VimKey::named("escape"));
        assert_eq!(vim.mode, VimMode::Normal);

        core.sel = Selection::caret(Pos::new(p1, 0));
        type_seq(&mut vim, &mut core, "d]h");
        assert_eq!(core.doc.blocks.len(), 3);
        assert_eq!(core.doc.blocks[1].id, h2);
    }
}
