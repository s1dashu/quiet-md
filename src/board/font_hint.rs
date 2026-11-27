//! A quiet note at the foot of the board while Xiaolai, its CJK handwriting
//! font, downloads (`crate::font_downloads`): 正在下载中文手写字体… 45%, then
//! 已就绪 for a moment. Meanwhile the board draws CJK with the system fonts
//! and redraws once the font is here. A failed download says so once per run
//! (with 重试); later attempts, after the backoff, stay silent. Its own view,
//! so progress repaints the note and not the board. Styled as the chrome's
//! bars (`design::Toolbar`), out of the way of the zoom and help controls.

use super::chrome::{self, style};
use crate::design::{self, Button, ButtonSize};
use crate::font_downloads::{self as fd, XIAOLAI};
use crate::settings::tr;
use gpui::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// A failure was shown this run (until 重试).
static FAILURE_SHOWN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq)]
enum Shown { Nothing, Downloading, Done, Failed }

pub struct FontHint { shown: Shown, hide: Option<Task<()>> }

impl FontHint {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.observe_global::<fd::Tick>(|me, cx| {
            if fd::pending(XIAOLAI) { me.shown = Shown::Downloading; cx.notify(); }
        }).detach();
        cx.observe_global::<fd::Changed>(|me, cx| {
            let c = cx.global::<fd::Changed>().clone();
            if c.id != XIAOLAI { return; }
            if c.ok {
                if me.shown == Shown::Downloading { me.show(Shown::Done, 2, cx); }
            } else if c.error.is_some() && !FAILURE_SHOWN.swap(true, Ordering::Relaxed) {
                me.show(Shown::Failed, 8, cx);
            } else if me.shown == Shown::Downloading {
                me.shown = Shown::Nothing;
            }
            cx.notify();
        }).detach();
        FontHint { shown: if fd::pending(XIAOLAI) { Shown::Downloading } else { Shown::Nothing }, hide: None }
    }

    /// Show `s`, then nothing after `secs`.
    fn show(&mut self, s: Shown, secs: u64, cx: &mut Context<Self>) {
        self.shown = s;
        self.hide = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(secs)).await;
            let _ = this.update(cx, |me, cx| { me.shown = Shown::Nothing; cx.notify(); });
        }));
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        FAILURE_SHOWN.store(false, Ordering::Relaxed);
        self.hide = None;
        self.shown = Shown::Downloading;
        fd::want(XIAOLAI, fd::Mode::Now);
        cx.notify();
    }
}

impl Render for FontHint {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let row = div().absolute().left_0().right_0().bottom(px(style::PADDING)).flex().justify_center();
        if self.shown == Shown::Nothing { return row; }
        let g = design::geo();
        let text = |t: SharedString| div().text_size(px(g.text)).text_color(design::color::foreground()).child(t);
        let note = match self.shown {
            Shown::Downloading => {
                let p = fd::progress(XIAOLAI);
                div().flex().items_center().gap(px(10.))
                    .child(text(tr("正在下载中文手写字体…", "Downloading the handwriting font…").into()))
                    // a progress bar: the slider's track and range
                    .child(div().w(px(56.)).h(px(6.)).rounded_full().overflow_hidden().bg(design::color::muted()).child(div().h_full().w(px(56. * p)).bg(design::color::primary())))
                    .child(div().w(px(36.)).whitespace_nowrap().text_size(px(g.text_xs)).text_color(design::color::muted_foreground()).child(format!("{:.0}%", p * 100.)))
            }
            Shown::Done => div().flex().items_center().gap(px(8.))
                .child(design::icon("check", 16., design::color::foreground()))
                .child(text(tr("中文手写字体已就绪", "Handwriting font ready").into())),
            _ => div().flex().items_center().gap(px(4.))
                .child(text(tr("手写字体下载失败，暂用系统字体", "Couldn't download the handwriting font; using the system font").into()))
                .child(Button::link("font-hint-retry").sized(ButtonSize::Sm).label(tr("重试", "Retry")).on_click(cx.listener(|me, _, _, cx| me.retry(cx)))),
        };
        row.child(chrome::bar("font-hint").child(div().px(px(8.)).flex().items_center().child(note)))
    }
}
