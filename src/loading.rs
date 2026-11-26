//! 首次打开大文档库: the first listing reads every document's file (about
//! 2 s for 15,000 notes without the parse cache), so it runs off the main
//! thread and hands everything back at once. The window waits up to `WAIT`
//! for it before its first frame, so a small or already-read library opens
//! as it always did; past that, sidebar and the page hold still until
//! `SHOW_AFTER`, then skeleton rows and the onboarding's drifting light
//! show with how far it got, and fade out over what was found.
//!
//! One hand-back, not batches: rows coming in batches would reorder the
//! tree, 置顶 and the page under the pointer as each batch lands, and every batch
//! costs the main thread the full sort and shape hash of the list again.
//!
//! Meanwhile search, 新建 and the imports say the library is still loading
//! (nothing is queued to happen by surprise later); a Markdown file opened
//! from the Finder or dropped on the window opens as always.

use crate::settings::tr;
use crate::store::{Entry, ListProgress, Store};
use crate::theme::Theme;
use crate::design::color;
use gpui::*;
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;
use web_time::Instant;

/// How long the window waits for the listing before its first frame.
pub const WAIT: Duration = Duration::from_millis(150);
/// Nothing moves for a listing done sooner than this.
pub const SHOW_AFTER: Duration = Duration::from_millis(300);
const FADE_IN: Duration = Duration::from_millis(220);
const FADE_OUT: Duration = Duration::from_millis(420);

/// What the loading layer looks like at a moment. The layer is a cover in
/// the page's colour (so the empty page under it never shows) with the
/// skeleton and light on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Loading, too soon to show anything: the cover alone, as an empty page.
    Waiting,
    /// Loading; what is on the cover shows at this opacity.
    Shown(f32),
    /// Done; the cover fading out over what was found.
    Fading { cover: f32, content: f32 },
    /// Done (or done before anything showed): nothing over the window.
    Gone,
}

impl Phase {
    /// `started` the listing, `done` when it came back, `fading` since the
    /// first frame drawn with what it found (taking it in holds the main
    /// thread a moment; the fade counts from after that, or it would be over
    /// before it is seen).
    pub fn at(started: Instant, done: Option<Instant>, fading: Option<Instant>, now: Instant) -> Phase {
        let secs = |d: Duration| d.as_secs_f32();
        let shown = |t: Instant| ease((secs(t.saturating_duration_since(started)) - secs(SHOW_AFTER)) / secs(FADE_IN));
        match done {
            None if now.saturating_duration_since(started) < SHOW_AFTER => Phase::Waiting,
            None => Phase::Shown(shown(now)),
            Some(d) if d.saturating_duration_since(started) < SHOW_AFTER => Phase::Gone,
            Some(d) => {
                let out = fading.map_or(0., |f| secs(now.saturating_duration_since(f)) / secs(FADE_OUT));
                if out >= 1. { Phase::Gone } else { Phase::Fading { cover: 1. - ease(out), content: shown(d) } }
            }
        }
    }
    pub fn loading(self) -> bool { matches!(self, Phase::Waiting | Phase::Shown(_)) }
    pub fn cover(self) -> f32 { match self { Phase::Waiting | Phase::Shown(_) => 1., Phase::Fading { cover, .. } => cover, Phase::Gone => 0. } }
    pub fn content(self) -> f32 { match self { Phase::Shown(a) | Phase::Fading { content: a, .. } => a, _ => 0. } }
    /// Still changing: the window draws again next frame.
    pub fn moving(self) -> bool { !matches!(self, Phase::Gone) }
}

fn ease(t: f32) -> f32 { let t = t.clamp(0., 1.); t * t * (3. - 2. * t) }

/// The first listing, while it runs and as it fades; or a folder just added
/// (`over`: what the window shows stays, the layer comes only if it takes a while).
pub struct Load { started: Instant, done: Option<Instant>, fading: Option<Instant>, frames: u8, progress: Arc<ListProgress>, over: bool }
impl Global for Load {}

impl Load {
    pub fn phase(&self) -> Phase { Phase::at(self.started, self.done, self.fading, Instant::now()) }
    /// (documents gone through, of how many; 0 until the folders are scanned).
    pub fn counts(&self) -> (usize, usize) { (self.progress.done.load(Ordering::Relaxed), self.progress.total.load(Ordering::Relaxed)) }
    /// How opaque the layer over the page is: over a folder being added, nothing at first
    /// and then only as far as it faded in.
    pub fn cover(&self) -> f32 {
        let p = self.phase();
        if !self.over { return p.cover() }
        match p { Phase::Waiting | Phase::Gone => 0., Phase::Shown(a) => a, Phase::Fading { cover, content } => cover * content }
    }
}

pub fn phase(cx: &App) -> Phase { cx.try_global::<Load>().map_or(Phase::Gone, Load::phase) }
/// Seconds since the listing began: what the skeleton and spinner move by.
/// A window is drawing a frame: once the listing is back, the fade starts
/// with the second (the first lays out every row found, which takes a while).
pub fn drawn(cx: &mut App) {
    if let Some(l) = cx.try_global::<Load>() && l.done.is_some() && l.fading.is_none() {
        let l = cx.global_mut::<Load>();
        l.frames += 1;
        if l.frames >= 2 { l.fading = Some(Instant::now()); }
    }
}
pub fn t(cx: &App) -> f32 { cx.try_global::<Load>().map_or(0., |l| l.started.elapsed().as_secs_f32()) }
pub fn active(cx: &App) -> bool { phase(cx).loading() }
/// `Load::cover`.
pub fn cover(cx: &App) -> f32 { cx.try_global::<Load>().map_or(0., Load::cover) }
/// A folder being added: the sidebar keeps its rows.
pub fn adding(cx: &App) -> bool { cx.try_global::<Load>().is_some_and(|l| l.over) }

/// The entries a new window starts with: listed here when quick enough,
/// else none for now (`None`): the listing goes on in the background and
/// every window takes its result (`Workspace::loaded`).
#[cfg(target_family = "wasm")]
pub fn begin(store: &mut crate::shared_store::SharedStore, _: &mut App) -> Option<Vec<Entry>> { Some(store.list()) }
#[cfg(target_family = "wasm")]
pub fn begin_added(store: &mut crate::shared_store::SharedStore, _: &str, _: &mut App) -> Option<Vec<Entry>> { Some(store.list()) }

#[cfg(not(target_family = "wasm"))]
pub fn begin(store: &mut crate::shared_store::SharedStore, cx: &mut App) -> Option<Vec<Entry>> {
    // Once a launch; a window opened while it runs waits with the others.
    if let Some(l) = cx.try_global::<Load>() { return if l.done.is_some() { Some(store.list()) } else { None } }
    let keys = store.top_folders().into_iter().map(|(k, ..)| k).collect();
    start(store, keys, false, cx)
}

/// Top-level folder `key`, just added (添加文件夹…), listed the same way: here when
/// quick enough, else in the background with the layer over the page once it takes a while.
#[cfg(not(target_family = "wasm"))]
pub fn begin_added(store: &mut crate::shared_store::SharedStore, key: &str, cx: &mut App) -> Option<Vec<Entry>> {
    start(store, vec![key.to_string()], true, cx)
}

#[cfg(not(target_family = "wasm"))]
fn start(store: &mut crate::shared_store::SharedStore, keys: Vec<String>, over: bool, cx: &mut App) -> Option<Vec<Entry>> {
    let writers: Vec<(String, Store)> = keys.into_iter().filter_map(|k| store.top_writer(&k).map(|w| (k, w))).collect();
    let progress = Arc::new(ListProgress::default());
    let counted = progress.clone();
    let started = Instant::now();
    // 手动排序 numbers every page the first time a folder is opened: for a
    // big library that is a write of every entry, done here too.
    let by_hand = crate::app::Workspace::sorts_by_hand(&store.settings());
    let task = cx.background_executor().spawn(async move {
        writers.into_iter().map(|(k, mut w)| {
            let mut e = w.list_with(Some(&counted), true);
            if by_hand && w.initialize_manual_order().is_ok_and(|c| c) { e = w.list(); }
            (k, w, e)
        }).collect::<Vec<_>>()
    });
    store.begin_loading();
    cx.set_global(Load { started, done: None, fading: None, frames: 0, progress, over });
    // A folder added under what the window shows needs no wait: nothing is blank meanwhile.
    match cx.foreground_executor().block_with_timeout(if over { Duration::ZERO } else { WAIT }, task) {
        Ok(done) => {
            let entries = finish(store, done);
            cx.global_mut::<Load>().done = Some(Instant::now());
            Some(entries)
        }
        Err(task) => {
            crate::logging::write("library: listing in the background");
            cx.spawn(async move |cx| {
                let done = task.await;
                cx.update(|cx| {
                    let back = Instant::now();
                    let mut store = cx.global::<crate::shared_store::SharedStore>().clone();
                    let entries = finish(&mut store, done);
                    cx.global_mut::<Load>().done = Some(back);
                    let taken = back.elapsed();
                    for handle in cx.windows().into_iter().filter_map(|w| w.downcast::<crate::app::Workspace>()) {
                        let entries = entries.clone();
                        let _ = handle.update(cx, |ws, window, cx| ws.loaded(entries, window, cx));
                    }
                    crate::logging::write(&format!("library: listed {} entries in {:.2} s; taken over in {} ms, windows {} ms",
                        entries.len(), back.duration_since(started).as_secs_f32(), taken.as_millis(), back.elapsed().saturating_sub(taken).as_millis()));
                });
            }).detach();
            None
        }
    }
}

/// The libraries take over what their writers read; the window's rows.
#[cfg(not(target_family = "wasm"))]
fn finish(store: &mut crate::shared_store::SharedStore, done: Vec<(String, Store, Vec<Entry>)>) -> Vec<Entry> {
    let mut lists = vec![];
    let writers = done.into_iter().map(|(k, w, e)| { lists.push((k.clone(), e)); (k, w) }).collect();
    // A file opened from the Finder meanwhile: list again (everything is read now).
    if store.end_loading(writers) { return store.list() }
    let mut entries = vec![];
    for (k, e) in lists { entries = store.merge_entries(&k, e); }
    entries
}

/// What this module says, by its Chinese key (each language's table has them).
#[cfg(test)]
const TEXTS: [&str; 5] = ["马上就好…", "正在载入文档库", "正在载入 {} / {} 篇", "已读取 {} 篇", "文档库还在载入，稍后再试"];

/// 3,214 with the language's thousands mark.
pub fn grouped(n: usize) -> String {
    use crate::settings::Lang;
    let mark = match crate::settings::lang() { Lang::Fr => "\u{202f}", Lang::Es => ".", _ => "," };
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) { out.push_str(mark); }
        out.push(c);
    }
    out
}

/// 正在载入 3,214 / 15,000 篇; before the folders are scanned only what was read.
pub fn progress_line(done: usize, total: usize) -> String {
    if total == 0 { crate::localized_format!("已读取 {} 篇", "{} notes read", grouped(done)) }
    // Every file read; what was found is being written down.
    else if done >= total { tr("马上就好…", "Almost done…").to_string() }
    else { crate::localized_format!("正在载入 {} / {} 篇", "Loading {} of {} notes", grouped(done.min(total)), grouped(total)) }
}

/// What the page shows meanwhile: the onboarding's drifting light, the spinner,
/// how far it got and a thin bar of it.
pub fn note_layer(phase: Phase, cover: f32, t: f32, (done, total): (usize, usize)) -> Stateful<Div> {
    let bar = (total > 0).then(|| {
        let share = (done as f32 / total as f32).clamp(0., 1.);
        div().w(px(220.)).h(px(3.)).rounded(px(2.)).bg(color::muted()).overflow_hidden()
            .child(div().h_full().w(px(220. * share)).rounded(px(2.)).bg(color::primary()))
    });
    div().id("library-loading").absolute().inset_0().opacity(cover).bg(color::background()).occlude()
        .child(div().absolute().inset_0().opacity(phase.content())
            .child(canvas(|b, _, _| b, move |_, b, window, _| crate::onboarding::paint_backdrop(b, t, window)).absolute().inset_0())
            .child(div().absolute().inset_0().flex().flex_col().items_center().justify_center().gap(px(14.))
                .child(crate::ui::spinner(t, color::foreground()))
                .child(div().text_size(px(15.)).line_height(px(22.)).font_weight(FontWeight::MEDIUM).text_color(color::foreground())
                    .child(crate::ui::ui_text(tr("正在载入文档库", "Loading the library"))))
                .child(div().text_size(px(12.)).line_height(px(18.)).text_color(color::muted_foreground()).child(progress_line(done, total)))
                .children(bar)))
}

/// Grey rows where the sidebar's will be, a light passing down them.
pub fn sidebar_rows(phase: Phase, t: f32) -> Div {
    // Widths as shares of the row, so it reads as titles of different lengths.
    const ROWS: [f32; 12] = [0.62, 0.48, 0.74, 0.55, 0.68, 0.41, 0.58, 0.71, 0.46, 0.64, 0.52, 0.38];
    let mut list = div().flex().flex_col().pt(px(6.)).gap(px(2.)).opacity(phase.content());
    list = list.child(div().h(px(28.)).flex().items_center().pl(px(10.)).child(div().w(px(56.)).h(px(8.)).rounded(px(4.)).bg(Theme::sidebar_hover().opacity(1.8))));
    for (i, w) in ROWS.iter().enumerate() {
        let lit = 0.5 + 0.5 * (std::f32::consts::TAU * (t * 0.6 - i as f32 * 0.07)).cos();
        let fill = Theme::sidebar_hover().opacity(1.4 + 0.9 * lit);
        list = list.child(div().h(px(34.)).flex().items_center().gap(px(10.)).pl(px(10.)).pr(px(16.))
            .child(div().size(px(16.)).flex_none().rounded(px(4.)).bg(fill))
            .child(div().flex_1().child(div().h(px(9.)).rounded(px(4.5)).bg(fill).w(relative(*w)))));
    }
    div().opacity(phase.cover()).child(list)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's `test` attribute would stand in for the standard one.
    use super::{Phase, SHOW_AFTER, FADE_OUT, TEXTS, grouped, progress_line, Store};
    use std::time::Duration;
    use web_time::Instant;

    fn ms(n: u64) -> Duration { Duration::from_millis(n) }

    #[test]
    fn a_quick_listing_never_shows() {
        let s = Instant::now();
        assert_eq!(Phase::at(s, None, None, s + ms(100)), Phase::Waiting);
        assert_eq!(Phase::at(s, None, None, s + ms(299)), Phase::Waiting);
        // Done at 280 ms: gone at once, nothing fades, also before a frame is drawn.
        assert_eq!(Phase::at(s, Some(s + ms(280)), None, s + ms(281)), Phase::Gone);
        assert_eq!(Phase::at(s, Some(s + ms(280)), Some(s + ms(300)), s + ms(310)), Phase::Gone);
        assert!(Phase::Waiting.loading() && Phase::Waiting.content() == 0. && Phase::Waiting.cover() == 1.);
    }

    #[test]
    fn a_slow_listing_fades_in_then_out() {
        let s = Instant::now();
        let p = Phase::at(s, None, None, s + SHOW_AFTER + ms(10));
        assert!(matches!(p, Phase::Shown(a) if a > 0. && a < 0.2), "{p:?}");
        assert_eq!(Phase::at(s, None, None, s + ms(2000)), Phase::Shown(1.));
        let done = s + ms(2000);
        // Taken in, but no frame drawn yet (the main thread was busy): still fully there.
        assert_eq!(Phase::at(s, Some(done), None, done + ms(600)), Phase::Fading { cover: 1., content: 1. });
        let drawn = done + ms(600);
        let mid = Phase::at(s, Some(done), Some(drawn), drawn + FADE_OUT / 2);
        assert!(matches!(mid, Phase::Fading { cover, content } if (cover - 0.5).abs() < 0.01 && content == 1.), "{mid:?}");
        assert!(!mid.loading() && mid.moving());
        assert_eq!(Phase::at(s, Some(done), Some(drawn), drawn + FADE_OUT), Phase::Gone);
        // Done just after showing: what showed fades from where it was.
        let early = s + SHOW_AFTER + ms(20);
        let f = Phase::at(s, Some(early), Some(early), early + ms(1));
        assert!(matches!(f, Phase::Fading { content, .. } if content > 0. && content < 0.3), "{f:?}");
    }

    #[test]
    fn counts_read_with_thousands_marks() {
        assert_eq!(grouped(15000), "15,000");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(progress_line(3214, 15000), "Loading 3,214 of 15,000 notes");
        assert_eq!(progress_line(42, 0), "42 notes read");
        assert_eq!(progress_line(15000, 15000), "Almost done…");
    }

    /// While the first listing runs, a write (a file opened from the Finder)
    /// is listed without reading every document, and the hand-back then
    /// knows to list once more; without one the writer's rows stand.
    #[test]
    fn listing_meanwhile_reads_nothing_and_is_noticed() {
        let dir = fdl_core::vfs::temp_dir().join(format!("qmd-loading-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..5 { std::fs::write(dir.join(format!("n{i}.md")), format!("# 第 {i} 篇\n\n正文")).unwrap(); }
        let outside = dir.with_extension("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let ext = outside.join("外部.md");
        std::fs::write(&ext, "# 外部\n").unwrap();
        let active = Store::active_dir();
        let entry = fdl_core::top_folders::TopFolderEntry { key: "k".into(), path: dir.clone(), bookmark: None, added: 0 };
        let mut store = crate::shared_store::SharedStore::with_top_folders(fdl_core::top_folders::TopFolders::open(vec![entry], true));
        let mut writer = store.top_writer("k").unwrap();
        store.begin_loading();
        assert!(store.loading());
        let read = |es: &[crate::store::Entry], t: &str| es.iter().any(|e| e.title == t && e.body.contains("正文"));
        // Listed by name meanwhile (no file read), the Finder's file listed too.
        store.open_local(&ext).unwrap();
        let meanwhile = store.list();
        assert!(meanwhile.iter().any(|e| e.title == "n0") && !read(&meanwhile, "n0") && meanwhile.iter().any(|e| e.title == "外部"));
        let listed = writer.list_with(None, true);
        assert!(read(&listed, "n0"));
        assert!(store.end_loading(vec![("k".into(), writer)]), "a list is due");
        assert!(!store.loading());
        let after = store.list();
        assert!(read(&after, "n0") && after.iter().any(|e| e.title == "外部"));
        // Nothing written meanwhile: the writer's rows stand.
        let writer = store.top_writer("k").unwrap();
        store.begin_loading();
        assert!(!store.end_loading(vec![("k".into(), writer)]));
        Store::set_active_dir(&active);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// Every language says it in its own words, the counts carried over.
    #[test]
    fn every_language_has_the_loading_words() {
        use crate::settings::Lang;
        for l in Lang::ALL.into_iter().filter(|l| !matches!(l, Lang::Zh | Lang::En)) {
            for k in TEXTS { assert!(crate::l10n::get(l, k).is_some(), "{l:?}: {k}"); }
            let line = crate::settings::localized(l, "正在载入 {} / {} 篇", || "正在载入 3,214 / 15,000 篇".into(), || "en".into());
            assert!(line.contains("3,214") && line.contains("15,000") && line != "en", "{l:?}: {line}");
        }
    }
}
