//! 终端: a system shell docked under the note, shown and hidden with ⌘` /
//! Ctrl+` (as in Zed and VS Code). The PTY and the screen grid are
//! alacritty_terminal's, as in Zed; this draws the grid with GPUI and turns
//! keys into the bytes a terminal sends. The shell keeps running while the
//! panel is hidden; closing it (or turning it off in 设置) ends the shell.

use crate::app::{View, Workspace};
use crate::settings::tr;
use crate::theme::{self, Theme};
use crate::ui::{TipExt, TitleBarExt};
use alacritty_terminal::event::{Event as AlacEvent, EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Options, Shell};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{prelude::FluentBuilder, *};
use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

const FONT_SIZE: f32 = 13.;
const HEADER_H: f32 = 30.;
const MIN_H: f32 = 120.;
const DEFAULT_H: f32 = 260.;
const MIN_W: f32 = 280.;
const DEFAULT_W: f32 = 460.;
/// The note keeps at least this much beside the panel.
const NOTE_MIN_H: f32 = 200.;
const NOTE_MIN_W: f32 = 360.;

/// 设置 › 终端位置: right of the note rather than under it.
pub fn on_right() -> bool { crate::settings::prefs().terminal_right }

/// The panel on the workspace: shown or hidden, its size on either side, and
/// the running shell (kept while hidden).
pub struct Dock {
    open: bool,
    height: f32,
    width: f32,
    view: Option<Entity<TerminalView>>,
    /// The inner edge being dragged: (pointer position, size) when it started.
    resizing: Rc<Cell<Option<(f32, f32)>>>,
    /// The shell was changed while the panel was open: start the new one at the next frame.
    reopen: bool,
    _sub: Option<Subscription>,
}
impl Default for Dock { fn default() -> Self { Dock { open: false, height: DEFAULT_H, width: DEFAULT_W, view: None, resizing: Rc::default(), reopen: false, _sub: None } } }

impl Workspace {
    pub(crate) fn terminal_open(&self) -> bool { self.terminal.open }

    /// ⌘`: open the panel (starting a shell the first time), move the caret
    /// into it when it is open elsewhere, or hide it when it has the caret.
    pub(crate) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal.open && self.terminal.view.as_ref().is_some_and(|v| v.read(cx).focus.is_focused(window)) {
            self.terminal.open = false;
            self.focus_note(window, cx);
            cx.notify();
            return;
        }
        self.open_terminal(window, cx);
    }

    /// The title bar's button: shows the panel, or hides it when shown.
    pub(crate) fn terminal_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal.open {
            self.terminal.open = false;
            self.focus_note(window, cx);
            cx.notify();
        } else {
            self.open_terminal(window, cx);
        }
    }

    /// 设置 › 终端 › 外壳 changed: the running shell is replaced, at once when shown.
    pub(crate) fn shell_changed(&mut self) {
        if self.terminal.view.is_none() { return; }
        let open = self.terminal.open;
        self.close_terminal();
        self.terminal.reopen = open;
    }

    fn open_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !crate::settings::prefs().terminal {
            self.show_toast(tr("终端已在设置中关闭", "The terminal is turned off in Settings"), cx);
            return;
        }
        if self.terminal.view.is_none() {
            let (pty, events, name) = match Pty::spawn() {
                Ok(p) => p,
                Err(e) => { self.show_toast(&format!("{}: {e}", tr("无法启动终端", "Could not start the terminal")), cx); return; }
            };
            let view = cx.new(|cx| TerminalView::new(pty, events, name, cx));
            // The shell exited (`exit`, Ctrl+D) and the panel goes with it,
            // or its context menu asked.
            self.terminal._sub = Some(cx.subscribe_in(&view, window, |w, _, req: &Request, window, cx| {
                match req {
                    Request::Exited | Request::Close => { w.close_terminal(); w.focus_note(window, cx); }
                    Request::Hide => { w.terminal.open = false; w.focus_note(window, cx); }
                    Request::Flip => { let right = !on_right(); w.set_pref(|p| p.terminal_right = right, cx); }
                    Request::Shell(s) => { let s = s.to_string(); w.set_pref(|p| p.terminal_shell = s, cx); }
                }
                cx.notify();
            }));
            self.terminal.view = Some(view);
        }
        self.terminal.open = true;
        if let Some(v) = &self.terminal.view { let f = v.read(cx).focus.clone(); window.focus(&f, cx); }
        cx.notify();
    }

    /// End the shell and hide the panel.
    pub(crate) fn close_terminal(&mut self) {
        self.terminal.open = false;
        self.terminal.view = None;
        self.terminal._sub = None;
    }

    fn focus_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match (&self.editor, self.view == View::Doc) {
            (Some(ed), true) => { let f = ed.read(cx).focus.clone(); window.focus(&f, cx); }
            _ => window.focus(&self.focus, cx),
        }
    }

    pub(crate) fn render_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if std::mem::take(&mut self.terminal.reopen) { self.open_terminal(window, cx); }
        if !self.terminal.open { return None; }
        let view = self.terminal.view.clone()?;
        let right = on_right();
        // The room the note leaves: the window less the sidebar, across; the window, down.
        let room = |right: bool, w: &Self, window: &Window| if right {
            f32::from(window.viewport_size().width) - if w.sidebar_open { w.sidebar_w(window) } else { 0. } - NOTE_MIN_W
        } else { f32::from(window.viewport_size().height) - NOTE_MIN_H };
        let (min, size) = if right { (MIN_W, self.terminal.width) } else { (MIN_H, self.terminal.height) };
        let size = size.clamp(min, room(right, self, window).max(min));
        let name = view.read(cx).name.clone();
        let bg = palette().bg;
        let resizing = self.terminal.resizing.clone();
        let ws = cx.entity().downgrade();
        let button = |id: &'static str, icon: Svg| div().id(id).size(px(22.)).rounded(px(5.)).flex().items_center().justify_center().cursor_pointer()
            .hover(|s| s.bg(Theme::fill_hover())).active(|s| s.bg(Theme::fill_pressed())).child(icon)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        // The sidebar icon turned to show the side the panel moves to.
        let side_icon = crate::ui::icon("panel-left-open", 14., Theme::icon()).with_transformation(Transformation::rotate(radians(if right { -std::f32::consts::FRAC_PI_2 } else { std::f32::consts::PI })));
        let header = div().flex_none().h(px(HEADER_H)).pl(px(14.)).pr(px(6.)).flex().items_center().justify_between()
            .text_size(px(theme::ui_font_small())).text_color(Theme::text_caption())
            .child(div().flex().items_center().gap(px(6.)).child(tr("终端", "Terminal")).child(div().text_color(Theme::text_placeholder()).child(name)))
            .child(div().flex().items_center().gap(px(2.))
                .child(button("terminal-side", side_icon).tip(if right { tr("移到下方", "Move to bottom") } else { tr("移到右侧", "Move to right") }, None)
                    .on_click(cx.listener(move |w, _, _, cx| w.set_pref(|p| p.terminal_right = !right, cx))))
                .child(button("terminal-close", crate::ui::icon("x", 14., Theme::icon()))
                    .on_click(cx.listener(|w, _, window, cx| { w.close_terminal(); w.focus_note(window, cx); cx.notify(); }))));
        // Dragging the inner edge sets the size; the pointer is followed
        // across the whole window until the button comes up.
        let edge = {
            let r = resizing.clone();
            let d = div().id("terminal-resize").absolute();
            let d = if right { d.left(px(-3.)).top_0().bottom_0().w(px(6.)).cursor(CursorStyle::ResizeLeftRight) } else { d.top(px(-3.)).left_0().right_0().h(px(6.)).cursor(CursorStyle::ResizeUpDown) };
            d.on_mouse_down(MouseButton::Left, move |ev, _, cx| {
                r.set(Some((f32::from(if right { ev.position.x } else { ev.position.y }), size)));
                cx.stop_propagation();
            })
        };
        let follow = canvas(|_, _, _| {}, move |_, _, window, _| {
            let r = resizing.clone();
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble { return; }
                let Some((p0, s0)) = r.get() else { return };
                if !ev.dragging() { r.set(None); return; }
                let p = f32::from(if right { ev.position.x } else { ev.position.y });
                ws.update(cx, |w, cx| {
                    let s = (s0 + p0 - p).clamp(min, room(right, w, window).max(min));
                    if right { w.terminal.width = s } else { w.terminal.height = s }
                    cx.notify();
                }).ok();
            });
            let r = resizing.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, _, _, _| r.set(None));
        }).absolute().size_0();
        let panel = div().id("terminal-panel").relative().flex_none().flex().flex_col().bg(bg).border_color(Theme::line_divider());
        let panel = if right { panel.h_full().w(px(size)).border_l_1() } else { panel.w_full().h(px(size)).border_t_1() };
        // On the right it reaches the window's top edge, where the window's
        // own buttons are: that strip stays title bar.
        let caption = right.then(|| div().id("terminal-caption").flex_none().w_full().h(px(crate::app::HEADER_H)).title_bar());
        Some(panel
            .children(caption)
            .child(header)
            .child(div().flex_1().min_h_0().child(view))
            .child(edge)
            .child(follow)
            .into_any_element())
    }
}

#[derive(Clone)]
struct Listener(UnboundedSender<AlacEvent>);
impl EventListener for Listener {
    fn send_event(&self, event: AlacEvent) { let _ = self.0.unbounded_send(event); }
}

/// A shell on a pseudo terminal, read and parsed into `term` on
/// alacritty's own thread.
struct Pty {
    term: Arc<FairMutex<Term<Listener>>>,
    notifier: Notifier,
    size: Cell<WindowSize>,
}

impl Pty {
    fn spawn() -> anyhow::Result<(Pty, UnboundedReceiver<AlacEvent>, SharedString)> {
        let (tx, rx) = unbounded();
        let size = WindowSize { num_lines: 24, num_cols: 80, cell_width: 8, cell_height: 18 };
        let (shell, name, escape) = shell();
        let env = HashMap::from([("TERM", "xterm-256color"), ("COLORTERM", "truecolor"), ("TERM_PROGRAM", "quiet-md")].map(|(k, v)| (k.to_string(), v.to_string())));
        let options = Options {
            shell,
            working_directory: dirs::home_dir(),
            drain_on_exit: true,
            env,
            #[cfg(target_os = "windows")]
            escape_args: escape,
        };
        #[cfg(not(target_os = "windows"))]
        let _ = escape;
        let pty = tty::new(&options, size, 0)?;
        let config = Config { scrolling_history: 10_000, ..Config::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &TermSize::new(size.num_cols as usize, size.num_lines as usize), Listener(tx.clone()))));
        let event_loop = EventLoop::new(term.clone(), Listener(tx), pty, options.drain_on_exit, false)?;
        let notifier = Notifier(event_loop.channel());
        event_loop.spawn();
        Ok((Pty { term, notifier, size: Cell::new(size) }, rx, name))
    }

    fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) { self.notifier.notify(bytes); }

    /// ConPTY asserts on an empty size, hence at least 2 × 1 cells.
    fn resize(&self, cols: usize, lines: usize, cell: (f32, f32)) {
        let size = WindowSize { num_lines: lines.clamp(1, u16::MAX as usize) as u16, num_cols: cols.clamp(2, u16::MAX as usize) as u16, cell_width: cell.0 as u16, cell_height: cell.1 as u16 };
        let old = self.size.get();
        if (old.num_cols, old.num_lines) == (size.num_cols, size.num_lines) { return; }
        self.size.set(size);
        let _ = self.notifier.0.send(Msg::Resize(size));
        self.term.lock().resize(TermSize::new(size.num_cols as usize, size.num_lines as usize));
    }
}

impl Drop for Pty {
    fn drop(&mut self) { let _ = self.notifier.0.send(Msg::Shutdown); }
}

#[cfg(target_os = "windows")]
fn find(exe: &str, also: &[(&str, &str)]) -> Option<String> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(exe)).find(|f| f.is_file()))
        .or_else(|| also.iter().filter_map(|(var, rel)| std::env::var_os(var).map(|d| std::path::PathBuf::from(d).join(rel))).find(|f| f.is_file()))
        .map(|p| p.to_string_lossy().into_owned())
}
#[cfg(target_os = "windows")]
fn pwsh() -> Option<String> { find("pwsh.exe", &[("ProgramFiles", "PowerShell\\7\\pwsh.exe")]) }
#[cfg(target_os = "windows")]
fn git_bash() -> Option<String> { find("bash.exe", &[("ProgramFiles", "Git\\bin\\bash.exe")]).filter(|p| p.to_ascii_lowercase().contains("git")) }

/// 设置 › 外壳: (stored value, label) of the shells this computer has; the
/// first, stored as empty, is the default. Elsewhere the login shell, no choice.
pub fn shells() -> Vec<(&'static str, &'static str)> {
    #[cfg(target_os = "windows")]
    {
        let mut v = if pwsh().is_some() { vec![("", "PowerShell 7"), ("powershell", "Windows PowerShell")] } else { vec![("", "Windows PowerShell")] };
        v.push(("cmd", tr("命令提示符 (cmd)", "Command Prompt (cmd)")));
        if git_bash().is_some() { v.push(("git-bash", "Git Bash")); }
        v
    }
    #[cfg(not(target_os = "windows"))]
    vec![]
}

/// The shell to run, its short name, and whether its arguments are quoted
/// (not for cmd, which parses its own): 设置's `terminal_shell` (one of
/// `shells`, or any command line), else PowerShell 7 or Windows PowerShell,
/// else the login shell.
fn shell() -> (Option<Shell>, SharedString, bool) {
    let stem = |p: &str| SharedString::from(std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string()));
    let choice = crate::settings::prefs().terminal_shell;
    #[cfg(target_os = "windows")]
    {
        let (program, args): (String, Vec<String>) = match choice.as_str() {
            "" => (pwsh().unwrap_or_else(|| "powershell.exe".into()), vec!["-NoLogo".into()]),
            "powershell" => ("powershell.exe".into(), vec!["-NoLogo".into()]),
            "cmd" => (std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()), vec![]),
            "git-bash" => (git_bash().unwrap_or_else(|| "bash.exe".into()), vec!["--login".into(), "-i".into()]),
            custom => { let mut parts = custom.split_whitespace().map(String::from); (parts.next().unwrap_or_default(), parts.collect()) }
        };
        let name = stem(&program);
        let escape = !name.eq_ignore_ascii_case("cmd");
        (Some(Shell::new(program, args)), name, escape)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut parts = choice.split_whitespace().map(String::from);
        if let Some(program) = parts.next() { let name = stem(&program); return (Some(Shell::new(program, parts.collect())), name, true); }
        (None, std::env::var("SHELL").map(|s| stem(&s)).unwrap_or_else(|_| "sh".into()), true)
    }
}

/// What the panel asks of the workspace: the shell ended, or its context
/// menu's hide, close, move to the other side or change of shell.
pub enum Request { Exited, Hide, Close, Flip, Shell(&'static str) }

pub struct TerminalView {
    pty: Rc<Pty>,
    pub(crate) focus: FocusHandle,
    name: SharedString,
    /// Cell width and line height, measured from the font when painted.
    cell: Rc<Cell<(f32, f32)>>,
    /// The grid's top-left corner, for mouse positions.
    origin: Rc<Cell<Point<Pixels>>>,
    /// The cursor cell, where the IME puts its candidates.
    cursor: Rc<Cell<Bounds<Pixels>>>,
    selecting: bool,
    /// The right-click menu, where it was opened.
    menu: Option<Point<Pixels>>,
    /// Wheel movement not yet a whole line.
    scroll_px: f32,
    exited: bool,
    _events: Task<()>,
    _keys: Subscription,
}

impl EventEmitter<Request> for TerminalView {}

impl TerminalView {
    fn new(pty: Pty, mut events: UnboundedReceiver<AlacEvent>, name: SharedString, cx: &mut Context<Self>) -> Self {
        let _events = cx.spawn(async move |this, cx| {
            while let Some(first) = events.next().await {
                let mut batch = vec![first];
                while let Ok(e) = events.try_recv() { batch.push(e); }
                if this.update(cx, |t, cx| for e in batch { t.event(e, cx) }).is_err() { break; }
            }
        });
        // Keys go to the shell before the app's shortcuts see them (Ctrl+W
        // deletes a word there, Ctrl+N is the next command), except the
        // panel's own key.
        let weak = cx.entity().downgrade();
        let _keys = cx.intercept_keystrokes(move |ev, window, cx| {
            let Some(t) = weak.upgrade() else { return };
            if !t.read(cx).focus.is_focused(window) { return; }
            t.update(cx, |t, cx| t.key(ev, cx));
        });
        TerminalView {
            pty: Rc::new(pty), focus: cx.focus_handle(), name, cell: Rc::new(Cell::new((8., 18.))), origin: Rc::default(), cursor: Rc::default(),
            selecting: false, menu: None, scroll_px: 0., exited: false, _events, _keys,
        }
    }

    fn event(&mut self, event: AlacEvent, cx: &mut Context<Self>) {
        match event {
            AlacEvent::Wakeup => cx.notify(),
            // Answers to the shell's queries (cursor position, device attributes).
            AlacEvent::PtyWrite(s) => self.pty.write(s.into_bytes()),
            AlacEvent::TextAreaSizeRequest(f) => self.pty.write(f(self.pty.size.get()).into_bytes()),
            AlacEvent::ColorRequest(i, f) => {
                let rgb = self.pty.term.lock().colors()[i].unwrap_or_else(|| palette().rgb(i));
                self.pty.write(f(rgb).into_bytes());
            }
            AlacEvent::ClipboardStore(_, s) => cx.write_to_clipboard(ClipboardItem::new_string(s)),
            AlacEvent::ClipboardLoad(_, f) => {
                let text = cx.read_from_clipboard().and_then(|i| i.text()).unwrap_or_default();
                self.pty.write(f(&text).into_bytes());
            }
            AlacEvent::ChildExit(_) | AlacEvent::Exit if !self.exited => { self.exited = true; cx.emit(Request::Exited); }
            _ => {}
        }
    }

    /// Typing goes to the shell at the bottom of its output, ending any selection.
    fn input(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        { let mut t = self.pty.term.lock(); t.selection = None; t.scroll_display(Scroll::Bottom); }
        self.pty.write(bytes);
        cx.notify();
    }

    fn paste(&mut self, text: &str, cx: &mut Context<Self>) {
        let bracketed = self.pty.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        let s = if bracketed { format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")) } else { text.replace("\r\n", "\r").replace('\n', "\r") };
        self.input(s.into_bytes(), cx);
    }

    /// Copy and paste (Ctrl+Shift+C / Ctrl+V as in Windows Terminal, ⌘C / ⌘V
    /// on macOS; Ctrl+C copies when there is a selection, else interrupts),
    /// then everything a terminal sends as an escape sequence. Printable
    /// characters are left to the text input below (IME, AltGr).
    fn key(&mut self, ev: &KeystrokeEvent, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if self.menu.is_some() && k.key == "escape" { self.menu = None; cx.notify(); cx.stop_propagation(); return; }
        if ev.input_preference == InputPreference::CharacterInput && k.key_char.is_some() { return; }
        let toggle = crate::shortcuts::key_of(cx, "workspace::ToggleTerminal");
        if crate::shortcuts::parse_key(&toggle).is_ok_and(|t| t.key == k.key && t.modifiers == k.modifiers) { return; }
        let m = &k.modifiers;
        let mac = cfg!(target_os = "macos");
        let (copy, paste) = if mac {
            (m.platform && !m.control && !m.alt && k.key == "c", m.platform && !m.control && !m.alt && k.key == "v")
        } else {
            (m.control && m.shift && !m.alt && k.key == "c", m.control && !m.alt && !m.platform && k.key == "v")
        };
        let ctrl_c = !mac && m.control && !m.shift && !m.alt && !m.platform && k.key == "c";
        if copy || ctrl_c {
            if let Some(text) = self.selected_text() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.pty.term.lock().selection = None;
                cx.notify();
                cx.stop_propagation();
                return;
            }
            if copy { cx.stop_propagation(); return; }
        }
        if paste {
            self.paste_clipboard(cx);
            cx.stop_propagation();
            return;
        }
        let mode = *self.pty.term.lock().mode();
        if let Some(s) = to_esc_str(k, mode) {
            self.input(s.into_owned().into_bytes(), cx);
            cx.stop_propagation();
        }
    }

    fn selected_text(&self) -> Option<String> { self.pty.term.lock().selection_to_string().filter(|s| !s.is_empty()) }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() { cx.write_to_clipboard(ClipboardItem::new_string(text)); }
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) { self.paste(&text, cx); }
    }

    /// The whole scrollback and screen.
    fn select_all(&mut self, cx: &mut Context<Self>) {
        let mut t = self.pty.term.lock();
        let mut s = Selection::new(SelectionType::Lines, GridPoint::new(t.topmost_line(), Column(0)), Side::Left);
        s.update(GridPoint::new(t.bottommost_line(), t.last_column()), Side::Right);
        t.selection = Some(s);
        drop(t);
        cx.notify();
    }

    /// The right-click menu: copy and paste first, the panel's own commands last.
    fn render_menu(&self, at: Point<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let mac = cfg!(target_os = "macos");
        let toggle = crate::shortcuts::pretty(&crate::shortcuts::key_of(cx, "workspace::ToggleTerminal"));
        let has_selection = self.pty.term.lock().selection.as_ref().is_some_and(|s| !s.is_empty());
        let item = |id: &'static str, label: &'static str, hint: Option<SharedString>, enabled: bool, f: fn(&mut Self, &mut Context<Self>)| {
            crate::ui::menu_row(id, SharedString::from(id))
                .when(!enabled, |d| d.opacity(0.4).cursor_default())
                .child(div().flex_1().whitespace_nowrap().child(label))
                .children(hint.map(|h| div().text_size(px(12.)).text_color(Theme::text_caption()).child(h)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |t, _, _, cx| { if enabled { t.menu = None; f(t, cx); cx.notify(); } }))
        };
        let divider = || div().flex_none().mx(px(13.)).my(px(5.)).h(px(1.)).bg(Theme::line_border());
        let right = on_right();
        // The shells to switch to, the running one ticked.
        let current = crate::settings::prefs().terminal_shell;
        let shells = shells();
        let shell_rows: Vec<AnyElement> = if shells.len() > 1 {
            let caption = div().flex_none().px(px(13.)).pt(px(2.)).pb(px(2.)).text_size(px(11.)).text_color(Theme::text_caption()).child(tr("外壳", "Shell"));
            std::iter::once(divider().into_any_element()).chain(std::iter::once(caption.into_any_element())).chain(shells.into_iter().enumerate().map(|(i, (value, label))| {
                crate::ui::menu_row(("tm-shell", i), SharedString::from(format!("tm-shell-{i}")))
                    .child(div().flex_1().whitespace_nowrap().child(label))
                    .when(value == current, |d| d.child(crate::ui::icon("check", 14., Theme::icon())))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |t, _, _, cx| { t.menu = None; cx.emit(Request::Shell(value)); cx.notify(); }))
                    .into_any_element()
            })).collect()
        } else { vec![] };
        let menu = crate::ui::popover().w(px(240.)).py(px(crate::ui::MENU_PAD_Y)).flex().flex_col().occlude()
            .on_mouse_down_out(cx.listener(|t, _, _, cx| { t.menu = None; cx.notify(); }))
            .child(item("tm-copy", tr("复制", "Copy"), Some((if mac { "⌘C" } else { "Ctrl+Shift+C" }).into()), has_selection, |t, cx| t.copy(cx)))
            .child(item("tm-paste", tr("粘贴", "Paste"), Some((if mac { "⌘V" } else { "Ctrl+V" }).into()), true, |t, cx| t.paste_clipboard(cx)))
            .child(item("tm-all", tr("全选", "Select all"), None, true, |t, cx| t.select_all(cx)))
            .child(item("tm-clear", tr("清屏", "Clear"), None, true, |t, cx| t.input(vec![0x0c], cx)))
            .children(shell_rows)
            .child(divider())
            .child(item("tm-side", if right { tr("移到下方", "Move to bottom") } else { tr("移到右侧", "Move to right") }, None, true, |_, cx| cx.emit(Request::Flip)))
            .child(item("tm-hide", tr("隐藏终端", "Hide terminal"), Some(toggle.into()), true, |_, cx| cx.emit(Request::Hide)))
            .child(item("tm-close", tr("关闭终端", "Close terminal"), None, true, |_, cx| cx.emit(Request::Close)));
        deferred(anchored().position(at).snap_to_window_with_margin(px(8.)).child(menu)).with_priority(3).into_any_element()
    }

    fn scroll(&mut self, ev: &ScrollEvent, cx: &mut Context<Self>) {
        let lh = self.cell.get().1.max(1.);
        self.scroll_px += f32::from(ev.delta.pixel_delta(px(lh)).y);
        let lines = (self.scroll_px / lh) as i32;
        if lines == 0 { return; }
        self.scroll_px -= lines as f32 * lh;
        let mut t = self.pty.term.lock();
        let mode = *t.mode();
        // Full-screen programs (less, vim) scroll by the arrow keys.
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            drop(t);
            let app = mode.contains(TermMode::APP_CURSOR);
            let seq = match (lines > 0, app) { (true, true) => "\x1bOA", (true, false) => "\x1b[A", (false, true) => "\x1bOB", (false, false) => "\x1b[B" };
            self.pty.write(seq.repeat(lines.unsigned_abs() as usize).into_bytes());
        } else {
            t.scroll_display(Scroll::Delta(lines));
            drop(t);
            cx.notify();
        }
    }

    fn grid_point(&self, p: Point<Pixels>) -> (GridPoint, Side) {
        let (cw, lh) = self.cell.get();
        let o = self.origin.get();
        let x = f32::from(p.x - o.x).max(0.) / cw;
        let y = f32::from(p.y - o.y) / lh;
        let t = self.pty.term.lock();
        let col = (x as usize).min(t.columns().saturating_sub(1));
        let side = if x - col as f32 > 0.5 { Side::Right } else { Side::Left };
        let row = (y.floor() as i32).clamp(0, t.screen_lines() as i32 - 1);
        (GridPoint::new(Line(row - t.grid().display_offset() as i32), Column(col)), side)
    }

    /// A press starts a selection: a word on a double click, the line on a triple.
    fn select_start(&mut self, p: Point<Pixels>, clicks: usize, cx: &mut Context<Self>) {
        let (point, side) = self.grid_point(p);
        let ty = match clicks { 2 => SelectionType::Semantic, 3 => SelectionType::Lines, _ => SelectionType::Simple };
        self.pty.term.lock().selection = Some(Selection::new(ty, point, side));
        self.selecting = true;
        cx.notify();
    }

    fn select_to(&mut self, p: Point<Pixels>, cx: &mut Context<Self>) {
        let (point, side) = self.grid_point(p);
        if let Some(s) = self.pty.term.lock().selection.as_mut() { s.update(point, side); }
        cx.notify();
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity();
        let focus = self.focus.clone();
        let focused = self.focus.is_focused(window);
        let (pty, cell, origin, cursor) = (self.pty.clone(), self.cell.clone(), self.origin.clone(), self.cursor.clone());
        let grid = canvas(
            move |bounds, _, cx| {
                let font = mono();
                let fs = px(FONT_SIZE);
                let id = cx.text_system().resolve_font(&font);
                let cw = cx.text_system().advance(id, fs, 'm').map(|s| f32::from(s.width)).unwrap_or(8.).max(1.);
                let lh = (FONT_SIZE * 1.4).round();
                cell.set((cw, lh));
                origin.set(bounds.origin);
                pty.resize((f32::from(bounds.size.width) / cw) as usize, (f32::from(bounds.size.height) / lh) as usize, (cw, lh));
                (pty, cw, lh, cursor)
            },
            move |bounds, (pty, cw, lh, cursor), window, cx| {
                window.with_content_mask(Some(ContentMask { bounds }), |window| paint_grid(bounds, &pty.term.lock(), cw, lh, focused, &cursor, window, cx));
                window.handle_input(&focus, ElementInputHandler::new(bounds, me), cx);
            },
        ).size_full();
        div().id("terminal").key_context("Terminal").track_focus(&self.focus).size_full().pl(px(12.)).pr(px(8.)).pb(px(6.)).cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(|t, ev: &MouseDownEvent, window, cx| {
                window.focus(&t.focus, cx);
                t.select_start(ev.position, ev.click_count, cx);
                cx.stop_propagation();
            }))
            .on_mouse_move(cx.listener(|t, ev: &MouseMoveEvent, _, cx| if t.selecting && ev.dragging() { t.select_to(ev.position, cx) }))
            .on_mouse_up(MouseButton::Left, cx.listener(|t, _, _, _| t.selecting = false))
            .on_mouse_down(MouseButton::Right, cx.listener(|t, ev: &MouseDownEvent, window, cx| {
                window.focus(&t.focus, cx);
                t.menu = Some(ev.position);
                cx.notify();
                cx.stop_propagation();
            }))
            .on_scroll(cx.listener(|t, ev: &ScrollEvent, _, cx| t.scroll(ev, cx)))
            .child(grid)
            .children(self.menu.map(|at| self.render_menu(at, cx)))
    }
}

/// Text arrives here (typing, IME, AltGr); nothing is ever marked in place.
impl EntityInputHandler for TerminalView {
    fn text_for_range(&mut self, _: Range<usize>, _: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> { None }
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> { Some(UTF16Selection { range: 0..0, reversed: false }) }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> { None }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {}
    fn replace_text_in_range(&mut self, _: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        if !text.is_empty() { self.input(text.as_bytes().to_vec(), cx); }
    }
    fn replace_and_mark_text_in_range(&mut self, _: Option<Range<usize>>, _: &str, _: Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) {}
    fn bounds_for_range(&mut self, _: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> { Some(self.cursor.get()) }
    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> { None }
}

fn mono() -> Font { font(theme::MONO_FONT) }

/// A run of cells on one line drawn with one style.
struct Run { line: i32, col: i32, cells: usize, text: String, style: TextRun }

#[allow(clippy::too_many_arguments)]
fn paint_grid(bounds: Bounds<Pixels>, term: &Term<Listener>, cw: f32, lh: f32, focused: bool, cursor: &Cell<Bounds<Pixels>>, window: &mut Window, cx: &mut App) {
    let pal = palette();
    let base = mono();
    let content = term.renderable_content();
    let colors = content.colors;
    let offset = content.display_offset as i32;
    let selection = content.selection;
    let mut bgs: Vec<(i32, i32, usize, Hsla)> = vec![];
    let mut runs: Vec<Run> = vec![];
    for ic in content.display_iter {
        let line = ic.point.line.0 + offset;
        let col = ic.point.column.0 as i32;
        let cell = &*ic;
        let (mut fg, mut bg) = (cell.fg, cell.bg);
        if cell.flags.contains(Flags::INVERSE) { std::mem::swap(&mut fg, &mut bg); }
        let fill = if selection.is_some_and(|s| s.contains(ic.point)) { Some(pal.selection) }
            else if bg != Color::Named(NamedColor::Background) { Some(pal.resolve(bg, colors)) } else { None };
        if let Some(c) = fill {
            match bgs.last_mut() {
                Some(l) if l.0 == line && l.3 == c && l.1 + l.2 as i32 == col => l.2 += 1,
                _ => bgs.push((line, col, 1, c)),
            }
        }
        if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::HIDDEN) { continue; }
        if cell.c == ' ' && !cell.flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) { continue; }
        let mut color = pal.resolve(fg, colors);
        if cell.flags.contains(Flags::DIM) { color.a *= 0.7; }
        let style = TextRun {
            len: cell.c.len_utf8(),
            font: Font { weight: if cell.flags.contains(Flags::BOLD) { FontWeight::BOLD } else { base.weight }, style: if cell.flags.contains(Flags::ITALIC) { FontStyle::Italic } else { FontStyle::Normal }, ..base.clone() },
            color,
            background_color: None,
            underline: cell.flags.intersects(Flags::ALL_UNDERLINES).then(|| UnderlineStyle { color: Some(color), thickness: px(1.), wavy: cell.flags.contains(Flags::UNDERCURL) }),
            strikethrough: cell.flags.contains(Flags::STRIKEOUT).then(|| StrikethroughStyle { color: Some(color), thickness: px(1.) }),
        };
        match runs.last_mut() {
            Some(r) if r.line == line && r.col + r.cells as i32 == col && r.style.font == style.font && r.style.color == style.color
                && r.style.underline == style.underline && r.style.strikethrough == style.strikethrough => {
                r.text.push(cell.c); r.style.len += cell.c.len_utf8(); r.cells += 1;
            }
            _ => runs.push(Run { line, col, cells: 1, text: cell.c.to_string(), style }),
        }
        if let Some(zw) = cell.zerowidth() {
            let r = runs.last_mut().unwrap();
            for &c in zw { r.text.push(c); r.style.len += c.len_utf8(); }
        }
    }
    let o = bounds.origin;
    let at = |line: i32, col: i32| point(o.x + px(cw * col as f32), o.y + px(lh * line as f32));
    for (line, col, n, c) in bgs { window.paint_quad(fill(Bounds::new(at(line, col), size(px(cw * n as f32).ceil(), px(lh))), c)); }
    let (fs, lh_px, cw_px) = (px(FONT_SIZE), px(lh), px(cw));
    for r in runs {
        let shaped = window.text_system().shape_line(r.text.into(), fs, std::slice::from_ref(&r.style), Some(cw_px));
        let _ = shaped.paint(at(r.line, r.col), lh_px, TextAlign::Left, None, window, cx);
    }
    // The cursor: a block with the character redrawn over it, hollow when
    // the caret is elsewhere.
    let cur = content.cursor;
    let line = cur.point.line.0 + offset;
    if cur.shape == CursorShape::Hidden || line < 0 || line >= term.screen_lines() as i32 { return; }
    let wide = term.grid()[cur.point].flags.contains(Flags::WIDE_CHAR);
    let b = Bounds::new(at(line, cur.point.column.0 as i32), size(if wide { cw_px * 2. } else { cw_px }, lh_px));
    cursor.set(b);
    match (focused, cur.shape) {
        (false, _) | (_, CursorShape::HollowBlock) => window.paint_quad(outline(b, pal.cursor, BorderStyle::Solid)),
        (_, CursorShape::Beam) => window.paint_quad(fill(Bounds::new(b.origin, size(px(2.), lh_px)), pal.cursor)),
        (_, CursorShape::Underline) => window.paint_quad(fill(Bounds::new(point(b.origin.x, b.origin.y + lh_px - px(2.)), size(b.size.width, px(2.))), pal.cursor)),
        _ => {
            window.paint_quad(fill(b, pal.cursor));
            let c = term.grid()[cur.point].c;
            if c != ' ' {
                let run = TextRun { len: c.len_utf8(), font: base, color: pal.bg, background_color: None, underline: None, strikethrough: None };
                let _ = window.text_system().shape_line(c.to_string().into(), fs, &[run], None).paint(b.origin, lh_px, TextAlign::Left, None, window, cx);
            }
        }
    }
}

/// Zed's One Light and One Dark ANSI colours, on the note's own page colour.
const LIGHT: [u32; 16] = [0x000000, 0xde3e35, 0x3f953a, 0xb08a3e, 0x2f5af3, 0x950095, 0x0997b3, 0x8a8a8a, 0x5c5c5c, 0xde3e35, 0x3f953a, 0xb08a3e, 0x2f5af3, 0xa00095, 0x0bbcd6, 0x2a2c33];
const DARK: [u32; 16] = [0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf, 0x636d83, 0xea858b, 0xaad581, 0xffd885, 0x85c1ff, 0xd398eb, 0x6ed5de, 0xfafafa];

struct Palette { ansi: [u32; 16], fg: Hsla, bg: Hsla, cursor: Hsla, selection: Hsla }

fn palette() -> Palette {
    let fg = Theme::text();
    Palette { ansi: if theme::dark() { DARK } else { LIGHT }, fg, bg: crate::editor_theme::Theme::bg_body(), cursor: fg, selection: hsla(0.6, 0.8, 0.55, 0.28) }
}

impl Palette {
    /// The 256-colour table: the 16 above, the 6×6×6 cube, the grey ramp.
    fn rgb(&self, i: usize) -> Rgb {
        let (r, g, b) = match i {
            0..=15 => { let c = self.ansi[i]; ((c >> 16) as u8, (c >> 8) as u8, c as u8) }
            16..=231 => { let i = i - 16; let f = |v: usize| if v == 0 { 0 } else { (v * 40 + 55) as u8 }; (f(i / 36), f(i % 36 / 6), f(i % 6)) }
            232..=255 => { let v = ((i - 232) * 10 + 8) as u8; (v, v, v) }
            _ => { let c = Rgba::from(if i == NamedColor::Background as usize { self.bg } else { self.fg }); ((c.r * 255.) as u8, (c.g * 255.) as u8, (c.b * 255.) as u8) }
        };
        Rgb { r, g, b }
    }

    fn resolve(&self, c: Color, set: &Colors) -> Hsla {
        let rgb = |c: Rgb| -> Hsla { Rgba { r: c.r as f32 / 255., g: c.g as f32 / 255., b: c.b as f32 / 255., a: 1. }.into() };
        match c {
            Color::Spec(c) => rgb(c),
            Color::Indexed(i) => set[i as usize].map(rgb).unwrap_or_else(|| rgb(self.rgb(i as usize))),
            Color::Named(n) => match n as usize {
                i @ 0..=15 => set[i].map(rgb).unwrap_or_else(|| rgb(self.rgb(i))),
                i @ 259..=266 => { let mut c = rgb(self.rgb(i - 259)); c.a *= 0.7; c }
                _ if n == NamedColor::Background => self.bg,
                _ if n == NamedColor::Cursor => self.cursor,
                _ if n == NamedColor::DimForeground => { let mut c = self.fg; c.a *= 0.7; c }
                _ => self.fg,
            },
        }
    }
}

/// The bytes a terminal sends for a key that is not plain text (after Zed's
/// `to_esc_str`); `None` leaves the key to the app or the text input.
fn to_esc_str(k: &Keystroke, mode: TermMode) -> Option<Cow<'static, str>> {
    let m = &k.modifiers;
    if m.platform || m.function && k.key.len() == 1 { return None; }
    let app = mode.contains(TermMode::APP_CURSOR);
    let plain = !m.control && !m.alt && !m.shift;
    let key = k.key.as_str();
    let s: &'static str = match key {
        "tab" if plain => "\t",
        "tab" if m.shift && !m.control && !m.alt => "\x1b[Z",
        "escape" if plain => "\x1b",
        "enter" if plain => "\r",
        "enter" if m.shift && !m.control && !m.alt => "\n",
        "enter" if m.alt && !m.control && !m.shift => "\x1b\r",
        "backspace" if !m.control && !m.alt => "\x7f",
        "backspace" if m.control && !m.alt && !m.shift => "\x08",
        "backspace" if m.alt && !m.control && !m.shift => "\x1b\x7f",
        "space" if m.control && !m.alt && !m.shift => "\x00",
        "home" if plain => if app { "\x1bOH" } else { "\x1b[H" },
        "end" if plain => if app { "\x1bOF" } else { "\x1b[F" },
        "up" if plain => if app { "\x1bOA" } else { "\x1b[A" },
        "down" if plain => if app { "\x1bOB" } else { "\x1b[B" },
        "right" if plain => if app { "\x1bOC" } else { "\x1b[C" },
        "left" if plain => if app { "\x1bOD" } else { "\x1b[D" },
        "insert" if plain => "\x1b[2~",
        "delete" if plain => "\x1b[3~",
        "pageup" if plain => "\x1b[5~",
        "pagedown" if plain => "\x1b[6~",
        "f1" if plain => "\x1bOP", "f2" if plain => "\x1bOQ", "f3" if plain => "\x1bOR", "f4" if plain => "\x1bOS",
        "f5" if plain => "\x1b[15~", "f6" if plain => "\x1b[17~", "f7" if plain => "\x1b[18~", "f8" if plain => "\x1b[19~",
        "f9" if plain => "\x1b[20~", "f10" if plain => "\x1b[21~", "f11" if plain => "\x1b[23~", "f12" if plain => "\x1b[24~",
        _ => "",
    };
    if !s.is_empty() { return Some(s.into()); }
    // Ctrl+letter is the letter's control code; Ctrl and a few punctuation keys too.
    if m.control && !m.alt && key.len() == 1 {
        let b = key.as_bytes()[0];
        if b.is_ascii_alphabetic() { return Some(((b.to_ascii_lowercase() - b'a' + 1) as char).to_string().into()); }
        let c = match b { b'@' | b'2' => "\x00", b'[' => "\x1b", b'\\' => "\x1c", b']' => "\x1d", b'^' | b'6' => "\x1e", b'_' | b'-' => "\x1f", b'?' => "\x7f", _ => "" };
        if !c.is_empty() { return Some(c.into()); }
    }
    // Other modifier combinations on the special keys, xterm style.
    if !plain {
        let n = 1 + m.shift as u32 + 2 * m.alt as u32 + 4 * m.control as u32;
        let s = match key {
            "up" => format!("\x1b[1;{n}A"), "down" => format!("\x1b[1;{n}B"), "right" => format!("\x1b[1;{n}C"), "left" => format!("\x1b[1;{n}D"),
            "home" => format!("\x1b[1;{n}H"), "end" => format!("\x1b[1;{n}F"),
            "insert" => format!("\x1b[2;{n}~"), "delete" => format!("\x1b[3;{n}~"), "pageup" => format!("\x1b[5;{n}~"), "pagedown" => format!("\x1b[6;{n}~"),
            "f1" => format!("\x1b[1;{n}P"), "f2" => format!("\x1b[1;{n}Q"), "f3" => format!("\x1b[1;{n}R"), "f4" => format!("\x1b[1;{n}S"),
            "f5" => format!("\x1b[15;{n}~"), "f6" => format!("\x1b[17;{n}~"), "f7" => format!("\x1b[18;{n}~"), "f8" => format!("\x1b[19;{n}~"),
            "f9" => format!("\x1b[20;{n}~"), "f10" => format!("\x1b[21;{n}~"), "f11" => format!("\x1b[23;{n}~"), "f12" => format!("\x1b[24;{n}~"),
            _ => String::new(),
        };
        if !s.is_empty() { return Some(s.into()); }
    }
    // Alt+character is ESC then the character (not on macOS, where ⌥ types characters).
    if !cfg!(target_os = "macos") && m.alt && !m.control && key.len() == 1 && key.is_ascii() {
        let c = key.chars().next()?;
        return Some(format!("\x1b{}", if m.shift { c.to_ascii_uppercase() } else { c }).into());
    }
    None
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in `gpui::test`, which shadows `#[test]`.
    use super::{TermMode, to_esc_str};
    use gpui::Keystroke;

    fn esc(key: &'static str) -> Option<String> { to_esc_str(&Keystroke::parse(key).unwrap(), TermMode::empty()).map(|s| s.into_owned()) }

    #[test]
    fn keys_to_bytes() {
        assert_eq!(esc("enter").as_deref(), Some("\r"));
        assert_eq!(esc("ctrl-c").as_deref(), Some("\x03"));
        assert_eq!(esc("ctrl-w").as_deref(), Some("\x17"));
        assert_eq!(esc("up").as_deref(), Some("\x1b[A"));
        assert_eq!(esc("ctrl-left").as_deref(), Some("\x1b[1;5D"));
        assert_eq!(esc("shift-tab").as_deref(), Some("\x1b[Z"));
        assert_eq!(esc("a"), None);
        assert_eq!(esc("shift-a"), None);
        assert_eq!(esc("ctrl-`"), None, "the panel's own key passes through");
    }
}
