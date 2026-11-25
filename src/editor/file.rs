//! 视频或文件 (`<figure><source/></figure>`): the file card, the preview
//! (the first page from Quick Look's thumbnailer, inline video and audio
//! players), its toolbar, and inserting picked or dropped files.

use fdl_core::vfs::{PathExt as _, KindExt as _};
use super::core::{DropPlace, Pos, Selection, TITLE};
use super::overlays::{Probe, at};
use super::render::{Env, R};
use super::view::{Editor, Menu};
use crate::store::image_path;
use crate::model::*;
use crate::store::Store;
use crate::editor_theme::{self as theme, Theme, craft};
use crate::editor_ui::{TipExt, icon};
use crate::design::toolbar::{TOOLBAR_H, toolbar_layout, w as tw};
use crate::design::{Button, ButtonGroupSeparator, ButtonSize, Toggle, Toolbar};
use super::image::picture;
use gpui::{prelude::FluentBuilder, *};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

#[cfg(target_os = "macos")]
mod video;

actions!(video_player, [VideoClose, VideoToggle]);

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", VideoClose, Some("VideoPlayer")),
        KeyBinding::new("space", VideoToggle, Some("VideoPlayer")),
    ]);
}

/// `.docx-width-mode .preview-wrap:not(audio):not(video) { max-width: 818px }`.
const PREVIEW_MAX_W: f32 = 818.;
/// New attachments open as 预览视图 under these sizes (documents, video).
const DOC_PREVIEW_MAX: u64 = 10 << 20;
const VIDEO_PREVIEW_MAX: u64 = 100 << 20;
/// Cards in the page stay crisp: radius 5 with a 1pt hairline.
const RADIUS: f32 = 5.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Pdf,
    Word,
    Excel,
    Ppt,
    Video,
    Audio,
    Image,
    Zip,
    Text,
    Code,
    Other,
}

fn ext_of(name: &str) -> String {
    Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase()
}

pub fn kind_of(name: &str) -> Kind {
    match ext_of(name).as_str() {
        "pdf" => Kind::Pdf,
        "doc" | "docx" | "rtf" | "pages" | "odt" | "wps" => Kind::Word,
        "xls" | "xlsx" | "csv" | "numbers" | "ods" | "et" => Kind::Excel,
        "ppt" | "pptx" | "key" | "odp" | "dps" => Kind::Ppt,
        "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "wmv" | "flv" | "3gp" | "mpg" | "mpeg" => Kind::Video,
        "mp3" | "m4a" | "wav" | "aac" | "flac" | "aiff" | "aif" | "caf" | "ogg" | "opus" | "wma" | "amr" => Kind::Audio,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "tif" | "tiff" | "svg" => Kind::Image,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" => Kind::Zip,
        "txt" | "md" | "markdown" | "log" => Kind::Text,
        "rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "go" | "java" | "c" | "cc" | "cpp" | "h" | "hpp" | "json" | "html" | "css" | "sh" | "swift"
        | "kt" | "rb" | "php" | "xml" | "yaml" | "yml" | "toml" | "sql" => Kind::Code,
        _ => Kind::Other,
    }
}

/// What 预览视图 can show: a first page or an inline media player for
/// the formats AVFoundation plays.
fn preview_of(name: &str) -> Option<Kind> {
    let k = kind_of(name);
    let ext = ext_of(name);
    match k {
        Kind::Zip | Kind::Other => None,
        Kind::Audio if !matches!(ext.as_str(), "mp3" | "m4a" | "wav" | "aac" | "flac" | "aiff" | "aif" | "caf") => None,
        Kind::Video if !matches!(ext.as_str(), "mp4" | "mov" | "m4v") => None,
        k => Some(k),
    }
}

pub fn previewable(name: &str) -> bool {
    preview_of(name).is_some()
}

/// The type's colour: the content palette (Tailwind v4 500 shades),
/// grey for plain text and unknown files.
fn kind_color(k: Kind) -> u32 {
    match k {
        Kind::Pdf => craft::R400,
        Kind::Word | Kind::Code => 0x2b7fff,
        Kind::Excel => 0x00bc7d,
        Kind::Ppt => craft::O400,
        Kind::Video => 0x8e51ff,
        Kind::Audio => 0x00b8db,
        Kind::Image => 0x00bba7,
        Kind::Zip => 0xfe9a00,
        Kind::Text | Kind::Other => 0x9ea4aa,
    }
}

fn glyph(k: Kind) -> &'static str {
    match k {
        Kind::Pdf => "editor-icons/file-g-pdf.svg",
        Kind::Word => "editor-icons/file-g-word.svg",
        Kind::Excel => "editor-icons/file-g-excel.svg",
        Kind::Ppt => "editor-icons/file-g-ppt.svg",
        Kind::Video => "editor-icons/file-g-video.svg",
        Kind::Audio => "editor-icons/file-g-audio.svg",
        Kind::Image => "editor-icons/file-g-image.svg",
        Kind::Zip => "editor-icons/file-g-zip.svg",
        Kind::Code => "editor-icons/file-g-code.svg",
        Kind::Text | Kind::Other => "editor-icons/file-g-text.svg",
    }
}

/// The file type icon: a page in the type's colour, its folded corner and
/// a white glyph.
pub fn type_icon(k: Kind, s: f32) -> Div {
    let layer = |path: &'static str, color: Hsla| svg().path(path).absolute().top_0().left_0().size(px(s)).text_color(color);
    div()
        .relative()
        .flex_none()
        .size(px(s))
        .child(layer("editor-icons/file-page.svg", theme::c(kind_color(k))))
        .child(layer("editor-icons/file-fold.svg", theme::rgbaf(255, 255, 255, 0.45)))
        .child(layer(glyph(k), gpui::white()))
}

/// The same icon painted into `b` (the inline chip).
pub fn paint_type_icon(k: Kind, b: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    for (path, color) in [("editor-icons/file-page.svg", theme::c(kind_color(k))), ("editor-icons/file-fold.svg", theme::rgbaf(255, 255, 255, 0.45)), (glyph(k), gpui::white())] {
        let _ = window.paint_svg(b, path.into(), None, TransformationMatrix::unit(), color, cx);
    }
}

/// `512 B`, `1.50KB`, `12.30MB`.
pub fn format_size(n: u64) -> String {
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.;
    let mut i = 0;
    while v >= 1024. && i < UNITS.len() - 1 {
        v /= 1024.;
        i += 1;
    }
    format!("{v:.2}{}", UNITS[i])
}

fn skip_time(time: f64, duration: f64, delta: f64) -> Option<f64> {
    (time.is_finite() && duration.is_finite() && duration > 0. && delta.is_finite()).then(|| (time + delta).clamp(0., duration))
}

fn clock(t: f64) -> String {
    let s = t.max(0.).round() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// The file behind an attachment (cloud tokens have none).
pub fn local_path(f: &FileBlock) -> Option<PathBuf> {
    (f.src_kind != "token").then(|| image_path(&f.src)).flatten()
}

#[derive(Clone, PartialEq)]
enum Thumb {
    Pending,
    Ready(PathBuf),
    Failed,
}

pub(super) struct FileUi {
    thumbs: RefCell<HashMap<PathBuf, Thumb>>,
    queue: RefCell<Vec<PathBuf>>,
    sizes: RefCell<HashMap<PathBuf, Option<u64>>>,
    /// Box of every card / preview drawn this frame (window coordinates).
    pub(super) boxes: Rc<RefCell<HashMap<BlockId, Bounds<Pixels>>>>,
    /// Media players by file, made when the player first shows.
    players: RefCell<HashMap<PathBuf, Option<Rc<player::Player>>>>,
    /// Progress bars of the media players drawn this frame.
    bars: Rc<RefCell<HashMap<BlockId, Bounds<Pixels>>>>,
    video_hover: RefCell<HashMap<(BlockId, bool), Rc<Cell<bool>>>>,
    opened_video: Option<PathBuf>,
    fullscreen: Option<BlockId>,
    fullscreen_focus: FocusHandle,
    seeking: Option<BlockId>,
    muted: bool,
    ticking: bool,
}

impl FileUi {
    pub fn new(cx: &mut App) -> Self {
        FileUi {
            thumbs: RefCell::default(),
            queue: RefCell::default(),
            sizes: RefCell::default(),
            boxes: Rc::default(),
            players: RefCell::default(),
            bars: Rc::default(),
            video_hover: RefCell::default(),
            opened_video: None,
            fullscreen: None,
            fullscreen_focus: cx.focus_handle(),
            seeking: None,
            muted: false,
            ticking: false,
        }
    }

    fn size(&self, p: &Path) -> Option<u64> {
        if let Some(s) = self.sizes.borrow().get(p) {
            return *s;
        }
        let s = fdl_core::vfs::metadata(p).ok().filter(|m| m.fs_is_file()).map(|m| m.len());
        self.sizes.borrow_mut().insert(p.to_path_buf(), s);
        s
    }

    fn thumb(&self, p: &Path, k: Kind) -> Thumb {
        if k == Kind::Image {
            return Thumb::Ready(p.to_path_buf());
        }
        if let Some(t) = self.thumbs.borrow().get(p) {
            return t.clone();
        }
        let t = match first_png(&thumb_dir(p)) {
            Some(png) => Thumb::Ready(png),
            None => {
                self.queue.borrow_mut().push(p.to_path_buf());
                Thumb::Pending
            }
        };
        self.thumbs.borrow_mut().insert(p.to_path_buf(), t.clone());
        t
    }

    fn player(&self, p: &Path) -> Option<Rc<player::Player>> {
        if let Some(pl) = self.players.borrow().get(p) {
            return pl.clone();
        }
        let pl = player::Player::open(p).map(Rc::new);
        if let Some(pl) = &pl {
            pl.set_volume(if self.muted { 0. } else { 1. });
        }
        self.players.borrow_mut().insert(p.to_path_buf(), pl.clone());
        pl
    }

    fn needs_tick(&self) -> bool {
        self.players.borrow().values().flatten().any(|p| p.needs_tick())
    }
}

/// Where Quick Look's thumbnail of `p` goes (keyed by path, size and time).
fn thumb_dir(p: &Path) -> PathBuf {
    let mut h = rustc_hash::FxHasher::default();
    p.hash(&mut h);
    if let Ok(m) = fdl_core::vfs::metadata(p) {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    Store::assets_dir().join("thumbs").join(format!("{:016x}", h.finish()))
}

fn first_png(dir: &Path) -> Option<PathBuf> {
    fdl_core::vfs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "png"))
}

fn make_thumb(p: &Path) -> Option<PathBuf> {
    let dir = thumb_dir(p);
    let _ = fdl_core::vfs::create_dir_all(&dir);
    let _ = std::process::Command::new("qlmanage")
        .args(["-t", "-s", "1200", "-o"])
        .arg(&dir)
        .arg(p)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    first_png(&dir)
}

/// Opens the file in Quick Look (scripted runs only log it).
pub fn quick_look(p: &Path) {
    if crate::devtools::scripted() || cfg!(target_family = "wasm") {
        eprintln!("quicklook {}", p.display());
        return;
    }
    let mut cmd = std::process::Command::new("qlmanage");
    cmd.arg("-p").arg(p).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    std::thread::spawn(move || {
        let _ = cmd.status();
    });
}

/// A new attachment for `p`, copied into `assets/files/<uuid>/<name>`.
fn file_block_from_path(p: &Path) -> Option<Block> {
    let name = p.file_name()?.to_string_lossy().to_string();
    let size = fdl_core::vfs::metadata(p).ok().filter(|m| m.fs_is_file())?.len();
    let dir = Store::assets_dir().join("files").join(uuid::Uuid::new_v4().to_string());
    fdl_core::vfs::create_dir_all(&dir).ok()?;
    let dest = dir.join(&name);
    fdl_core::vfs::copy(p, &dest).ok()?;
    let view = match preview_of(&name) {
        Some(Kind::Audio) => FileView::Preview,
        Some(Kind::Video) if size < VIDEO_PREVIEW_MAX => FileView::Preview,
        Some(Kind::Pdf | Kind::Word | Kind::Excel | Kind::Ppt) if size < DOC_PREVIEW_MAX => FileView::Preview,
        _ => FileView::Card,
    };
    let f = FileBlock { src_kind: "path".into(), src: dest.to_string_lossy().to_string(), name, view };
    Some(Block::new(BlockKind::File(f), RichText::default()))
}

/// Opens the attachment right away (the card's 预览, the video's play).
fn open_listener(p: Option<PathBuf>, cx: &mut Context<Editor>) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |e: &mut Editor, _: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        window.prevent_default();
        e.open_file(p.as_deref(), cx);
    })
}

/// A floating toolbar's icon button.
/// A toolbar action (design::toolbar): a ghost `icon-sm` button.
fn tool_btn(id: impl Into<ElementId>) -> Button { Button::ghost(id).sized(ButtonSize::Sm) }

impl R<'_> {
    pub(super) fn file(&self, b: &Block, f: &FileBlock, env: Env, cx: &mut Context<Editor>) -> AnyElement {
        let ed = self.ed;
        let id = b.id;
        let container = if env.width > 0. { env.width } else { ed.content_width() };
        let active = !self.ghost && ed.active_file(self.focused) == Some(id);
        let selected = active
            || self.selected.contains(&id)
            || !self.ghost && !ed.core.sel.is_collapsed() && !ed.core.in_block_mode() && ed.core.atomic_selected(id, &self.units);
        let hovered = !self.ghost && !selected && ed.handle_block() == Some(id);
        // `hovered` covers the pointer over the block and its open handle
        // menu; only the menu gets the accent-tinted ring (as code blocks do).
        let menu_on = hovered && matches!(&ed.menu, Some(Menu::Block { block, .. }) | Some(Menu::Add { block, .. }) if *block == id);
        let path = local_path(f);
        let size = path.as_ref().and_then(|p| ed.file.size(p));
        let kind = kind_of(&f.name);
        let opened_video = kind == Kind::Video && path.as_ref().is_some_and(|p| ed.file.opened_video.as_ref() == Some(p));
        let preview = ((f.view == FileView::Preview || opened_video) && size.is_some()).then(|| preview_of(&f.name)).flatten();
        // The card rim: a hairline, a step darker under the pointer, the
        // accent once selected.
        let border = if selected {
            Theme::primary()
        } else if menu_on {
            Theme::code_hover_ring()
        } else if hovered {
            theme::c(craft::N400)
        } else {
            Theme::button_border()
        };
        let ring = || div().absolute().top(px(-2.)).left(px(-2.)).right(px(-2.)).bottom(px(-2.)).border_1().border_color(Theme::code_hover_ring());
        let boxes = ed.file.boxes.clone();
        let recorder = (!self.ghost).then(|| {
            canvas(move |bounds, _, _| { boxes.borrow_mut().insert(id, bounds); }, |_, _, _, _| {})
                .absolute()
                .inset_0()
        });
        // A click on the selected attachment (any click when reading) opens it.
        let open_on_click = {
            let p = path.clone();
            cx.listener(move |e: &mut Editor, ev: &MouseDownEvent, window, cx| {
                if ev.click_count == 1 && !ev.modifiers.shift && (e.read_only || e.active_file(e.focus.is_focused(window)) == Some(id)) {
                    cx.stop_propagation();
                    window.prevent_default();
                    e.open_file(p.as_deref(), cx);
                }
            })
        };
        let body: AnyElement = match (preview, &path) {
            (Some(Kind::Audio | Kind::Video), Some(p)) => ed.media(id, f, p, container, selected, self.ghost, None, cx),
            (Some(k), Some(p)) => {
                let w = container.min(PREVIEW_MAX_W);
                let iw = (w - 2.).max(0.);
                let thumb = ed.file.thumb(p, k);
                let content: AnyElement = match &thumb {
                    Thumb::Ready(t) => {
                        let (tw, th) = ed.img.dims(t).unwrap_or((16., 9.));
                        let full = iw * th / tw.max(1.);
                        let h = full.clamp(120., 600.);
                        let pic = picture(t, iw.max(full)).object_fit(ObjectFit::Fill).id(("file-thumb", id.0));
                        let pic = pic.absolute().top_0().left_0().w(px(iw)).h(px(full));
                        div()
                            .relative()
                            .w(px(iw))
                            .h(px(h))
                            .overflow_hidden()
                            .bg(Theme::bg_body())
                            .child(pic)
                            .into_any_element()
                    }
                    t => div()
                        .w(px(iw))
                        .h(px(220.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(Theme::bg_overlay())
                        .child(if *t == Thumb::Pending {
                            super::image::spinning("file-thumb-loading", 32., theme::c(craft::N400), 2.)
                        } else {
                            type_icon(k, 64.).into_any_element()
                        })
                        .into_any_element(),
                };
                let footer = div()
                    .h(px(42.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .bg(Theme::bg_overlay())
                    .border_t_1()
                    .border_color(Theme::line_divider())
                    .child(type_icon(kind, 20.))
                    .child(div().ml(px(8.)).min_w_0().text_size(px(13.)).text_color(Theme::text()).text_ellipsis().whitespace_nowrap().overflow_hidden().child(f.name.clone()));
                // A crisp card: hairline, radius 5, the button drop.
                div()
                    .id(("file-preview", id.0))
                    .relative()
                    .w(px(w))
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(border)
                    .shadow(theme::shadow_button())
                    .cursor_pointer()
                    .when(menu_on, |d| d.child(ring().rounded(px(RADIUS + 2.))))
                    .child(div().rounded(px(RADIUS - 1.)).overflow_hidden().child(content).child(footer))
                    .children(recorder)
                    .on_mouse_down(MouseButton::Left, open_on_click)
                    .into_any_element()
            }
            _ => {
                let missing = size.is_none();
                let desc = match size {
                    Some(n) => format_size(n),
                    None => crate::settings::text("文件不存在").into(),
                };
                let av = matches!(kind, Kind::Video | Kind::Audio);
                let l = open_listener(path.clone(), cx);
                let btn = div()
                    .id(("file-open", id.0))
                    .size(px(34.))
                    .mx(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS))
                    .cursor_pointer()
                    .hover(|s| s.bg(Theme::fill_hover()))
                    .tip_above(if av { crate::settings::text("播放") } else { crate::settings::text("预览") }, None)
                    .child(icon(if av { "circle-play" } else { "eye-open" }, 20., if missing { Theme::text_disabled() } else { Theme::icon() }))
                    .on_mouse_down(MouseButton::Left, l);
                // The file card: white, hairline, crisp radius 5, the button drop.
                div()
                    .id(("file-card", id.0))
                    .relative()
                    .flex()
                    .w(px(400.))
                    .max_w_full()
                    .min_w(px(200.))
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(border)
                    .bg(crate::theme::raised(Theme::bg_body()))
                    .shadow(theme::shadow_button())
                    .cursor_pointer()
                    .when(menu_on, |d| d.child(ring().rounded(px(RADIUS + 2.))))
                    .child(div().flex_none().size(px(48.)).mt(px(8.)).mb(px(8.)).ml(px(8.)).mr(px(12.)).child(type_icon(kind, 48.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(64.))
                            .flex()
                            .flex_col()
                            .justify_center()
                            .child(div().max_h(px(42.)).text_size(px(15.)).line_height(px(21.)).text_color(Theme::text()).line_clamp(2).overflow_hidden().child(f.name.clone()))
                            .child(
                                div()
                                    .h(px(17.))
                                    .text_size(px(12.))
                                    .line_height(px(17.))
                                    .text_color(if missing { Theme::danger() } else { Theme::text_caption() })
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(desc),
                            ),
                    )
                    .child(div().flex_none().pr(px(4.)).mt(px(8.)).h(px(48.)).flex().items_center().child(btn))
                    .children(recorder)
                    .on_mouse_down(MouseButton::Left, open_on_click)
                    .into_any_element()
            }
        };
        div()
            .w_full()
            .flex()
            .font_family(theme::DOC_UI_FONT)
            .map(|d| match b.align {
                Align::Left => d.justify_start(),
                Align::Center => d.justify_center(),
                Align::Right => d.justify_end(),
            })
            .child(body)
            .into_any_element()
    }
}

impl Editor {
    /// Shared inline and fullscreen controls keep the same player, time and volume.
    fn media(&self, id: BlockId, f: &FileBlock, p: &Path, container: f32, selected: bool, ghost: bool, height: Option<f32>, cx: &mut Context<Self>) -> AnyElement {
        let ed = self;
        let kind = kind_of(&f.name);
        let boxes = ed.file.boxes.clone();
        let recorder = (!ghost && height.is_none()).then(|| {
            canvas(move |bounds, _, _| { boxes.borrow_mut().insert(id, bounds); }, |_, _, _, _| {}).absolute().inset_0()
        });
        let video = kind == Kind::Video;
        let scope = if height.is_some() { "video-full" } else { "video-inline" };
        let anchor = |part: &str| crate::devtools::anchor_probe(format!("{scope}-{part}"));
        let pl = (!ghost).then(|| ed.file.player(p)).flatten();
        let (t, d) = pl.as_ref().map_or((0., 0.), |pl| (pl.time(), pl.duration()));
        let playing = pl.as_ref().is_some_and(|pl| pl.playing());
        let frac = if d > 0. { (t / d).clamp(0., 1.) as f32 } else { 0. };
        let bars = ed.file.bars.clone();
        let bar_rec = (!ghost).then(|| {
            canvas(move |bounds, _, _| { bars.borrow_mut().insert(id, bounds); }, |_, _, _, _| {})
                .absolute()
                .inset_0()
        });
        let ok = pl.as_ref().is_some_and(|pl| !pl.failed());
        let title = div()
            .min_h(px(30.))
            .mb(px(2.))
            .flex()
            .items_center()
            .when(video, |d| d.child(div().mr(px(8.)).child(type_icon(kind, 20.))))
            .child(div().flex_1().min_w_0().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(Theme::text()).text_ellipsis().whitespace_nowrap().overflow_hidden().child(f.name.clone()))
            .child(div().ml(px(8.)).flex_none().text_size(px(12.)).text_color(Theme::text_caption()).child(if ok { format!("{} / {}", clock(t), clock(d)) } else { crate::settings::text("无法播放").into() }));
        let play = Button::ghost((if playing { "file-pause" } else { "file-play" }, id.0)).sized(ButtonSize::Xs).disabled(!ok)
            .tip_above(if playing { crate::settings::text("暂停") } else { crate::settings::text("播放") }, None)
            .editor_icon(if playing { "pause-filled" } else { "play-filled" })
            .children(video.then(|| anchor("play")).flatten())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |e, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    e.toggle_media(id, cx);
                }),
            );
        let muted = ed.file.muted;
        let volume = Button::ghost((if muted { "file-unmute" } else { "file-mute" }, id.0)).sized(ButtonSize::Xs).disabled(!ok)
            .tip_above(if muted { crate::settings::text("取消静音") } else { crate::settings::text("静音") }, None)
            .editor_icon(if muted { "volume-x" } else { "volume-2" })
            .children(video.then(|| anchor("mute")).flatten())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|e, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    e.toggle_mute(cx);
                }),
            );
        let progress = div()
            .id(("file-progress", id.0))
            .relative()
            .flex_1()
            .min_w_0()
            .h(px(12.))
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(crate::design::color::border())
                    .child(div().absolute().left_0().top_0().bottom_0().w(relative(frac)).rounded(px(2.)).bg(crate::design::color::primary())),
            )
            .children(bar_rec)
            .children(video.then(|| anchor("progress")).flatten())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |e, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    e.file.seeking = Some(id);
                    e.seek_media(id, ev.position.x, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |e, ev: &MouseMoveEvent, _, cx| {
                if e.file.seeking == Some(id) && ev.pressed_button == Some(MouseButton::Left) {
                    cx.stop_propagation();
                    e.seek_media(id, ev.position.x, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|e, _, _, _| e.file.seeking = None))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|e, _, _, _| e.file.seeking = None));
        let body = if video {
            let full = height.is_some();
            let hover = ed.file.video_hover.borrow_mut().entry((id, full)).or_default().clone();
            let probe = hover.clone();
            let enabled = !ghost && if full { ed.file.fullscreen == Some(id) } else { ed.file.fullscreen.is_none() };
            // Popovers occlude their parent's hitbox. Use the video's bounds so
            // moving onto the floating controls keeps them visible too.
            let hover_probe = canvas(|bounds, window, _| bounds.intersect(&window.content_mask().bounds), move |_, bounds, window, _| {
                let inside = enabled && bounds.contains(&window.mouse_position());
                if probe.replace(inside) != inside { window.request_animation_frame(); }
                let probe = probe.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, _| {
                    if phase != DispatchPhase::Capture { return; }
                    let inside = enabled && bounds.contains(&ev.position);
                    if probe.replace(inside) != inside { window.refresh(); }
                });
            }).absolute().inset_0();
            let skip = |forward: bool, cx: &mut Context<Self>| {
                let part = if forward { "forward" } else { "back" };
                Button::ghost((if forward { "file-forward" } else { "file-back" }, id.0)).sized(ButtonSize::Xs)
                    .label(if forward { "+3s" } else { "−3s" }).disabled(!ok || d <= 0. || if forward { t >= d } else { t <= 0. })
                    .tip_above(if forward { crate::settings::tr("前进 3 秒", "Skip forward 3 seconds") } else { crate::settings::tr("后退 3 秒", "Skip back 3 seconds") }, None)
                    .children(anchor(part))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |e, _, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        e.skip_media(id, if forward { 3. } else { -3. }, cx);
                    }))
            };
            let controls = div().absolute().bottom(px(12.)).left(px(12.)).right(px(12.))
                .when(playing && !hover.get() && ed.file.seeking != Some(id), |d| d.invisible())
                .child(Toolbar::new(("file-video-controls", id.0)).w_full()
                    .on_mouse_down(MouseButton::Left, |_, window, cx| { window.prevent_default(); cx.stop_propagation(); })
                    .child(play).child(skip(false, cx)).child(progress).child(skip(true, cx)).child(volume));
            let center = (!playing && ok).then(|| {
                div().absolute().inset_0().flex().items_center().justify_center().child(
                    Button::primary(("file-video-center-play", id.0)).sized(ButtonSize::Lg).rounded_full()
                        .editor_icon("play-filled").shadow(crate::design::shadow::popover())
                        .tip_above(crate::settings::text("播放"), None).children(anchor("center-play"))
                        .on_mouse_down(MouseButton::Left, cx.listener(move |e, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            e.toggle_media(id, cx);
                        }))
                )
            });
            let content = pl.as_ref().and_then(|pl| pl.picture(container, height)).unwrap_or_else(|| {
                div().w_full().h(px(height.unwrap_or((container * 9. / 16.).clamp(160., 600.)))).flex().items_center().justify_center()
                    .child(if ok && pl.as_ref().is_some_and(|pl| pl.needs_tick()) {
                        super::image::spinning("file-video-loading", 32., crate::design::color::muted_foreground(), 2.)
                    } else { div().child(crate::settings::text("无法播放")).into_any_element() }).into_any_element()
            });
            div().id(("file-video", id.0)).relative().w_full().mb(px(6.))
                .bg(gpui::black()).text_color(gpui::white()).child(content).children(center).child(controls).child(hover_probe)
                .on_mouse_down(MouseButton::Left, cx.listener(move |e, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    e.toggle_media(id, cx);
                })).into_any_element()
        } else {
            div().id(("file-audio", id.0)).relative().px(px(18.)).py(px(6.)).flex().items_center().gap(px(14.))
                .rounded(px(crate::design::geo().button_r)).bg(crate::design::color::muted())
                .child(play).child(progress).child(volume).into_any_element()
        };
        let mut title = Some(title);
        div().relative().w_full().flex().flex_col().bg(crate::design::color::background())
            .when(selected, |d| d.shadow(vec![crate::design::shadow::ring(crate::design::color::ring(), 0.5)]))
            .when(!video, |d| d.children(title.take()))
            .child(body).children(title).children(recorder).into_any_element()
    }

    /// The attachment showing as selected (toolbar, accent rim).
    pub(super) fn active_file(&self, focused: bool) -> Option<BlockId> {
        let h = self.core.sel.head.block;
        (focused
            && !self.core.in_block_mode()
            && self.core.cell_sel.is_none()
            && self.core.sel.is_collapsed()
            && matches!(self.core.kind_of(h), Some(BlockKind::File(_))))
        .then_some(h)
    }

    pub(super) fn open_file(&mut self, p: Option<&Path>, cx: &mut Context<Self>) {
        match p.filter(|p| p.fs_is_file()) {
            Some(p) if kind_of(&p.to_string_lossy()) == Kind::Video && previewable(&p.to_string_lossy()) => {
                let mut in_page = false;
                self.core.doc.for_each_block(|b| {
                    if let BlockKind::File(f) = &b.kind { in_page |= local_path(f).as_deref() == Some(p); }
                });
                if !in_page { return quick_look(p); }
                let Some(pl) = self.file.player(p).filter(|pl| !pl.failed()) else { return self.toast(crate::settings::text("无法播放"), cx) };
                for other in self.file.players.borrow().values().flatten() { other.pause(); }
                self.file.opened_video = Some(p.to_path_buf());
                pl.play();
                self.tick_media(cx);
                cx.notify();
            }
            Some(p) => quick_look(p),
            None => self.toast(crate::settings::text("文件不存在"), cx),
        }
    }

    /// 下载: a copy where the save panel says (Downloads first).
    pub(super) fn download_file(&mut self, f: &FileBlock, cx: &mut Context<Self>) {
        let Some(src) = local_path(f).filter(|p| p.fs_is_file()) else { return self.toast(crate::settings::text("文件不存在"), cx) };
        let picked = crate::devtools::take_pick().and_then(|p| p.into_iter().next());
        let dir = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_default();
        let rx = picked.is_none().then(|| cx.prompt_for_new_path(&dir, Some(&f.name)));
        let bg = cx.background_executor().clone();
        cx.spawn(async move |_, _| {
            let dest = match rx {
                Some(rx) => rx.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(dest) = dest else { return };
            bg.spawn(async move {
                let _ = fdl_core::vfs::copy(&src, &dest);
            })
            .await;
        })
        .detach();
    }

    // ---- media

    fn open_video_fullscreen(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pl) = self.media_player(id).filter(|p| !p.failed()) else { return self.toast(crate::settings::text("无法播放"), cx) };
        self.file.fullscreen = Some(id);
        self.slash = None;
        self.menu = None;
        window.focus(&self.file.fullscreen_focus, cx);
        if pl.needs_tick() { self.tick_media(cx); }
        cx.notify();
    }

    fn close_video_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.file.fullscreen = None;
        self.file.seeking = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn video_fullscreen_el(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.file.fullscreen?;
        let Some(BlockKind::File(f)) = self.core.kind_of(id).cloned() else {
            self.close_video_fullscreen(window, cx);
            return None;
        };
        let Some(path) = local_path(&f) else { self.close_video_fullscreen(window, cx); return None };
        let vp = window.viewport_size();
        let width = (f32::from(vp.width) - 32.).max(1.);
        let height = (f32::from(vp.height) - 116.).max(80.);
        let close = Button::ghost("video-close").sized(ButtonSize::Sm).editor_icon("x")
            .tip_above(crate::settings::text("关闭"), Some("Esc"))
            .children(crate::devtools::anchor_probe("video-close"))
            .on_mouse_down(MouseButton::Left, cx.listener(|e, _, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                e.close_video_fullscreen(window, cx);
            }));
        let layer = div().id("video-fullscreen").key_context("VideoPlayer").track_focus(&self.file.fullscreen_focus)
            .occlude().relative().w(vp.width).h(vp.height).p(px(16.)).flex().flex_col().gap(px(8.))
            .bg(crate::design::color::background())
            .on_action(cx.listener(|e, _: &VideoClose, window, cx| e.close_video_fullscreen(window, cx)))
            .on_action(cx.listener(move |e, _: &VideoToggle, _, cx| e.toggle_media(id, cx)))
            .on_mouse_down(MouseButton::Left, |_, window, cx| { window.prevent_default(); cx.stop_propagation(); })
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, window, cx| { window.prevent_default(); cx.stop_propagation(); })
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(div().w_full().h(px(32.)).flex_none().flex().justify_end().child(close))
            .child(self.media(id, &f, &path, width, false, false, Some(height), cx));
        Some(deferred(anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.), px(0.))).child(layer)).with_priority(10).into_any_element())
    }

    fn media_player(&self, id: BlockId) -> Option<Rc<player::Player>> {
        let Some(BlockKind::File(f)) = self.core.kind_of(id) else { return None };
        self.file.player(&local_path(f)?)
    }

    fn toggle_media(&mut self, id: BlockId, cx: &mut Context<Self>) {
        let Some(pl) = self.media_player(id).filter(|pl| !pl.failed()) else { return self.toast(crate::settings::text("无法播放"), cx) };
        if pl.playing() {
            pl.pause();
        } else {
            // One at a time.
            for other in self.file.players.borrow().values().flatten() {
                if other.playing() {
                    other.pause();
                }
            }
            pl.play();
            self.tick_media(cx);
        }
        cx.notify();
    }

    fn pause_media(&self, id: BlockId) {
        let Some(BlockKind::File(f)) = self.core.kind_of(id) else { return };
        let Some(p) = local_path(f) else { return };
        if let Some(Some(pl)) = self.file.players.borrow().get(&p) { pl.pause(); }
    }

    fn seek_media(&mut self, id: BlockId, x: Pixels, cx: &mut Context<Self>) {
        let Some(pl) = self.media_player(id) else { return };
        let Some(b) = self.file.bars.borrow().get(&id).copied() else { return };
        let k = ((x - b.left()) / b.size.width.max(px(1.))).clamp(0., 1.) as f64;
        pl.seek(k * pl.duration());
        self.tick_media(cx);
        cx.notify();
    }

    fn skip_media(&mut self, id: BlockId, delta: f64, cx: &mut Context<Self>) {
        let Some(pl) = self.media_player(id).filter(|pl| !pl.failed()) else { return };
        let Some(target) = skip_time(pl.seek_time(), pl.duration(), delta) else { return };
        pl.seek(target);
        self.tick_media(cx);
        cx.notify();
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.file.muted = !self.file.muted;
        let v = if self.file.muted { 0. } else { 1. };
        for pl in self.file.players.borrow().values().flatten() {
            pl.set_volume(v);
        }
        cx.notify();
    }

    /// Redraws the time and progress while something plays.
    fn tick_media(&mut self, cx: &mut Context<Self>) {
        if self.file.ticking {
            return;
        }
        self.file.ticking = true;
        cx.spawn(async |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                let go = this
                    .update(cx, |e, cx| {
                        cx.notify();
                        let go = e.file.needs_tick();
                        e.file.ticking = go;
                        go
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        })
        .detach();
    }

    // ---- thumbnails

    /// Makes queued Quick Look thumbnails off the main thread.
    pub(super) fn start_thumbs(&mut self, cx: &mut Context<Self>) {
        if self.file.needs_tick() { self.tick_media(cx); }
        let queued: Vec<PathBuf> = self.file.queue.borrow_mut().drain(..).collect();
        for p in queued {
            let q = p.clone();
            let task = cx.background_executor().spawn(async move { make_thumb(&q) });
            cx.spawn(async move |this, cx| {
                let t = task.await;
                let _ = this.update(cx, |e, cx| {
                    e.file.thumbs.borrow_mut().insert(p, t.map_or(Thumb::Failed, Thumb::Ready));
                    e.img.generation += 1;
                    cx.notify();
                });
            })
            .detach();
        }
    }

    // ---- toolbar

    /// 全屏 · 下载 | 文本视图 · 卡片视图 · 预览视图 above the selected attachment.
    pub(super) fn file_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.active_file(self.focus.is_focused(window))?;
        if self.read_only || self.menu.is_some() || self.slash.is_some() || self.dragging || self.block_drag.is_some() {
            return None;
        }
        let rect = self.probed(Probe::File(id))?;
        let viewport = self.list.viewport_bounds();
        if !rect.intersects(&viewport) {
            return None;
        }
        let Some(BlockKind::File(f)) = self.core.kind_of(id) else { return None };
        let preview = preview_of(&f.name);
        let path = local_path(f);
        let view = if path.as_ref().is_some_and(|p| self.file.opened_video.as_ref() == Some(p)) { FileView::Preview } else { f.view };
        let act = |f: fn(&mut Editor, BlockId, &mut Window, &mut Context<Editor>)| {
            cx.listener(move |e: &mut Editor, _: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                f(e, id, window, cx);
            })
        };
        let sep = ButtonGroupSeparator::new;
        let mut bar = Toolbar::new("file-toolbar").on_mouse_down(MouseButton::Left, |_, window, cx| {
            cx.stop_propagation();
            window.prevent_default();
        });
        let mut widths = vec![];
        if preview.is_some_and(|k| k != Kind::Audio) {
            let p = path.clone();
            bar = bar.child(
                tool_btn("ftb-full").editor_icon("maximize").children(crate::devtools::anchor_probe("file-fullscreen")).tip_above(crate::settings::text("全屏"), None).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |e, _, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        match p.as_deref().filter(|p| p.fs_is_file()) {
                            Some(p) if kind_of(&p.to_string_lossy()) == Kind::Video => e.open_video_fullscreen(id, window, cx),
                            Some(p) => quick_look(p),
                            None => e.toast(crate::settings::text("文件不存在"), cx),
                        }
                    }),
                ),
            );
            widths.push(tw::BUTTON);
        }
        bar = bar
            .child(tool_btn("ftb-download").editor_icon("download").tip_above(crate::settings::text("下载"), None).on_mouse_down(
                MouseButton::Left,
                act(|e, id, _, cx| {
                    if let Some(BlockKind::File(f)) = e.core.kind_of(id).cloned() {
                        e.download_file(&f, cx);
                    }
                }),
            ))
            .child(sep())
            .child(tool_btn("ftb-text").editor_icon("view-text").tip_above(crate::settings::text("文本视图"), None).on_mouse_down(
                MouseButton::Left,
                act(|e, id, window, cx| {
                    e.pause_media(id);
                    e.file.opened_video = None;
                    e.core.file_to_inline(id);
                    e.changed(window, cx);
                }),
            ));
        widths.extend([tw::BUTTON, tw::SEPARATOR, tw::BUTTON]);
        let views: &[(&'static str, &'static str, &'static str, FileView)] = if preview.is_some() {
            &[("ftb-card", "view-card", "卡片视图", FileView::Card), ("ftb-preview", "view-preview", "预览视图", FileView::Preview)]
        } else {
            &[("ftb-card", "view-card", "卡片视图", FileView::Card)]
        };
        for &(key, ic, tip, v) in views {
            let on = view == v || preview.is_none();
            bar = bar.child(
                Toggle::new(key)
                    .editor_icon(ic)
                    .pressed(on)
                    .tip_above(tip, None)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |e, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            e.file.opened_video = None;
                            if v == FileView::Card { e.pause_media(id); }
                            e.core.set_file_view(id, v);
                            e.changed_preserving_viewport(window, cx);
                        }),
                    ),
            );
            widths.push(tw::TOGGLE);
        }
        let (_, w) = toolbar_layout(&widths);
        let x = (f32::from(rect.center().x) - w / 2.).clamp(f32::from(viewport.left()) + 8., (f32::from(viewport.right()) - w - 8.).max(f32::from(viewport.left()) + 8.));
        let above = f32::from(rect.top()) - 8. - TOOLBAR_H;
        let y = if above >= f32::from(viewport.top()) { above } else { f32::from(rect.bottom()) + 8. };
        Some(at(self.rel(point(px(x), px(y))), Anchor::TopLeft, bar))
    }

    /// The hover card of an inline attachment: 下载 | 文本视图 · 卡片视图 ·
    /// 预览视图, centred 4px above it.
    pub(super) fn file_card(&self, p: Pos, f: &FileBlock, cx: &mut Context<Self>) -> Option<AnyElement> {
        let r = self.probed(Probe::Button(p))?;
        let preview = previewable(&f.name);
        // 下载 | 文本视图 卡片视图 (预览视图): the views are toggles, this one pressed
        let n = if preview { 3 } else { 2 };
        let (_, w) = toolbar_layout(&[&[tw::BUTTON, tw::SEPARATOR][..], &vec![tw::TOGGLE; n]].concat());
        let file = f.clone();
        let sep = ButtonGroupSeparator::new();
        let mut bar = Toolbar::new("file-card-pop")
            .child(tool_btn("fcard-download").editor_icon("download").tip_above(crate::settings::text("下载"), None).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |e, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    e.hide_button_card(cx);
                    e.download_file(&file, cx);
                }),
            ))
            .child(sep)
            .child(Toggle::new("fcard-text").editor_icon("view-text").pressed(true).tip_above(crate::settings::text("文本视图"), None));
        let views: &[(&'static str, &'static str, &'static str, FileView)] = if preview {
            &[("fcard-card", "view-card", "卡片视图", FileView::Card), ("fcard-preview", "view-preview", "预览视图", FileView::Preview)]
        } else {
            &[("fcard-card", "view-card", "卡片视图", FileView::Card)]
        };
        for &(key, ic, tip, v) in views {
            bar = bar.child(Toggle::new(key).editor_icon(ic).tip_above(tip, None).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |e, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    if e.core.inline_to_file(p, v).is_some() {
                        e.hide_button_card(cx);
                        window.focus(&e.focus, cx);
                        e.changed(window, cx);
                    }
                }),
            ));
        }
        let el = bar
            .on_hover(cx.listener(|e, h: &bool, _, cx| {
                e.btn.card_over = *h;
                e.sync_button_card(cx);
            }))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            });
        Some(at(self.rel(point(r.center().x - px(w / 2.), r.top() - px(4.))), Anchor::BottomLeft, el))
    }

    // ---- inserting

    /// Copied files replace the selection just like pasted images. Copy off
    /// the UI thread; use the same attachment representation as picked files.
    pub(super) fn paste_files(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async move {
            paths.iter().map(|p| {
                if kind_of(&p.to_string_lossy()) == Kind::Image {
                    super::image::image_block_from_file(p).map_err(str::to_string)
                } else { file_block_from_path(p).ok_or_else(|| crate::settings::text("文件读取失败").to_string()) }
            }).collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let results = task.await;
            let _ = this.update_in(cx, |e, window, cx| {
                let mut blocks = Vec::new();
                for result in results {
                    match result { Ok(b) => blocks.push(b), Err(msg) => e.toast(&msg, cx) }
                }
                if blocks.is_empty() { return; }
                blocks.push(Block::text(""));
                e.core.paste_blocks(blocks);
                e.slash = None;
                e.changed(window, cx);
            });
        }).detach();
    }

    /// 视频或文件: the picked files become attachments at the caret.
    pub(super) fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::devtools::take_pick();
        let rx = picked.is_none().then(|| cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(crate::settings::text("插入视频或文件").into()), extensions: None }));
        cx.spawn_in(window, async move |this, cx| {
            let paths = match rx {
                Some(rx) => rx.await.ok().and_then(Result::ok).flatten(),
                None => picked,
            };
            let Some(paths) = paths.filter(|p| !p.is_empty()) else { return };
            let _ = this.update_in(cx, |e, window, cx| e.import_files(paths, None, window, cx));
        })
        .detach();
    }

    /// Copies `paths` into the library off the main thread, then inserts
    /// them at `target` (or the caret).
    pub(super) fn import_files(&mut self, paths: Vec<PathBuf>, target: Option<(BlockId, DropPlace)>, window: &mut Window, cx: &mut Context<Self>) {
        // A Markdown file keeps no attachments (dropped images still come in as images).
        if self.markdown { return; }
        let task = cx.background_executor().spawn(async move { paths.iter().map(|p| file_block_from_path(p)).collect::<Vec<_>>() });
        cx.spawn_in(window, async move |this, cx| {
            let blocks = task.await;
            let _ = this.update_in(cx, |e, window, cx| {
                let failed = blocks.iter().any(Option::is_none);
                let blocks: Vec<Block> = blocks.into_iter().flatten().collect();
                if failed {
                    e.toast(crate::settings::text("文件读取失败"), cx);
                }
                let Some(last) = blocks.last().map(|b| b.id) else { return };
                e.core.clear_block_sel();
                e.core.cell_sel = None;
                match target.filter(|(t, _)| e.core.block(*t).is_some()) {
                    Some((t, place)) => {
                        e.core.insert_blocks_at(blocks, t, place);
                        e.core.sel = Selection::caret(Pos::new(last, 0));
                    }
                    None => {
                        if e.core.sel.head.block == TITLE
                            && let Some(first) = e.core.doc.blocks.first().map(|b| b.id)
                        {
                            e.core.insert_blocks_at(blocks, first, DropPlace::Before);
                        } else {
                            e.core.insert_blocks(blocks);
                        }
                        let p = e.core.ensure_paragraph_after(last);
                        e.core.sel = Selection::caret(Pos::new(p, 0));
                    }
                }
                window.focus(&e.focus, cx);
                e.changed(window, cx);
            });
        })
        .detach();
    }
}

/// AVAudioPlayer for audio, AVPlayer with decoded surfaces for video.
#[cfg(target_os = "macos")]
mod player {
    use objc::runtime::{BOOL, Object, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use std::path::Path;

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {}

    pub enum Player { Audio(*mut Object), Video(super::video::Player) }

    impl Player {
        pub fn open(path: &Path) -> Option<Self> {
            if super::kind_of(&path.to_string_lossy()) == super::Kind::Video { return super::video::Player::open(path).map(Self::Video); }
            let s = path.to_str()?;
            unsafe {
                let ns: *mut Object = msg_send![class!(NSString), alloc];
                let ns: *mut Object = msg_send![ns, initWithBytes: s.as_ptr() as *const std::ffi::c_void length: s.len() encoding: 4usize];
                if ns.is_null() {
                    return None;
                }
                let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: ns];
                let _: () = msg_send![ns, release];
                if url.is_null() {
                    return None;
                }
                let p: *mut Object = msg_send![class!(AVAudioPlayer), alloc];
                let p: *mut Object = msg_send![p, initWithContentsOfURL: url error: std::ptr::null_mut::<*mut Object>()];
                if p.is_null() {
                    return None;
                }
                let _: BOOL = msg_send![p, prepareToPlay];
                Some(Player::Audio(p))
            }
        }

        pub fn play(&self) {
            match self { Self::Video(v) => v.play(), Self::Audio(p) => unsafe { let _: BOOL = msg_send![*p, play]; } }
        }
        pub fn pause(&self) {
            match self { Self::Video(v) => v.pause(), Self::Audio(p) => unsafe { let _: () = msg_send![*p, pause]; } }
        }
        pub fn playing(&self) -> bool {
            match self { Self::Video(v) => v.playing(), Self::Audio(p) => unsafe { let b: BOOL = msg_send![*p, isPlaying]; b == YES } }
        }
        pub fn time(&self) -> f64 {
            match self { Self::Video(v) => v.time(), Self::Audio(p) => unsafe { msg_send![*p, currentTime] } }
        }
        pub fn seek(&self, t: f64) {
            match self { Self::Video(v) => v.seek(t), Self::Audio(p) => unsafe { let _: () = msg_send![*p, setCurrentTime: t]; } }
        }
        pub fn duration(&self) -> f64 {
            match self { Self::Video(v) => v.duration(), Self::Audio(p) => unsafe { msg_send![*p, duration] } }
        }
        pub fn set_volume(&self, v: f32) {
            match self { Self::Video(p) => p.set_volume(v), Self::Audio(p) => unsafe { let _: () = msg_send![*p, setVolume: v]; } }
        }
        pub fn seek_time(&self) -> f64 { match self { Self::Video(v) => v.seek_time(), _ => self.time() } }
        pub fn failed(&self) -> bool { matches!(self, Self::Video(v) if v.failed()) }
        pub fn needs_tick(&self) -> bool { match self { Self::Audio(_) => self.playing(), Self::Video(v) => v.needs_tick() } }
        pub fn picture(&self, width: f32, height: Option<f32>) -> Option<gpui::AnyElement> { match self { Self::Video(v) => v.picture(width, height), _ => None } }
    }

    impl Drop for Player {
        fn drop(&mut self) {
            let Self::Audio(p) = self else { return };
            unsafe {
                let _: () = msg_send![*p, stop];
                let _: () = msg_send![*p, release];
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod player {
    use std::path::Path;

    pub struct Player;

    impl Player {
        pub fn seek_time(&self) -> f64 { self.time() }
        pub fn failed(&self) -> bool { true }
        pub fn needs_tick(&self) -> bool { false }
        pub fn picture(&self, _: f32, _: Option<f32>) -> Option<gpui::AnyElement> { None }
        pub fn open(_: &Path) -> Option<Self> {
            None
        }
        pub fn play(&self) {}
        pub fn pause(&self) {}
        pub fn playing(&self) -> bool {
            false
        }
        pub fn time(&self) -> f64 {
            0.
        }
        pub fn seek(&self, _: f64) {}
        pub fn duration(&self) -> f64 {
            0.
        }
        pub fn set_volume(&self, _: f32) {}
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, clock, format_size, kind_of, previewable, skip_time};

    #[test]
    fn skips_stop_at_the_start_and_end() {
        assert_eq!(skip_time(4., 10., 3.), Some(7.));
        assert_eq!(skip_time(4., 10., -3.), Some(1.));
        assert_eq!(skip_time(0.5, 10., -3.), Some(0.));
        assert_eq!(skip_time(9.5, 10., 3.), Some(10.));
        assert_eq!(skip_time(0., 2., 3.), Some(2.));
        assert_eq!(skip_time(2., 2., -3.), Some(0.));
        assert_eq!(skip_time(0., 0., 3.), None);
        assert_eq!(skip_time(0., f64::NAN, 3.), None);
    }

    #[test]
    fn sizes_and_kinds() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1536), "1.50KB");
        assert_eq!(format_size(5 << 20), "5.00MB");
        assert_eq!(kind_of("报告.DOCX"), Kind::Word);
        assert_eq!(kind_of("a.tar.gz"), Kind::Zip);
        assert!(previewable("录音.m4a"));
        assert!(!previewable("clip.ogg"));
        assert!(!previewable("archive.zip"));
        assert_eq!(clock(75.4), "01:15");
    }
}
