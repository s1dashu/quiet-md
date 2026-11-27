//! Mermaid diagrams (#23). A code block whose language is Mermaid shows its
//! diagram unless switched to its code (`show_code`, saved as `view="code"`).
//! crates/mermaid (`fdl_mermaid`, ours, no dependencies) parses, lays out
//! and writes the SVG on a background thread, coloured from the Theme
//! tokens and in the interface font; text is measured by GPUI's SVG
//! renderer, the resvg that then draws it (`img()`), so labels fit their
//! boxes. What it can't draw shows as code with a one-line note. Results are
//! kept in memory by source and theme.

use gpui::*;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

/// The code block language (`lang::LANGS`) drawn as a diagram.
pub const LANG: &str = "Mermaid";
/// The renderer: part of the cache key, so an upgrade draws anew.
const VERSION: &str = "fdl_mermaid 1";
/// Cached diagrams before the oldest half is dropped.
const MEMORY: usize = 256;

/// A drawn diagram: the SVG at twice its size (so the 1× rasterization GPUI
/// gives an SVG `Image` is sharp on Retina) and its size in points.
#[derive(Clone)]
pub struct Diagram {
    pub image: Arc<Image>,
    pub size: (f32, f32),
}

/// What a block shows: the diagram, the last one that block drew (if any)
/// while drawing, or why it can't be drawn.
#[derive(Clone)]
pub enum Look {
    Ready(Diagram),
    Pending(Option<Diagram>),
    Failed(String),
}

/// The cache key of `source` drawn with `style` (`Style::key`).
pub fn key(source: &str, style: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(VERSION.as_bytes());
    h.update([0]);
    h.update(style.as_bytes());
    h.update([0]);
    h.update(source.as_bytes());
    u64::from_le_bytes(h.finalize()[..8].try_into().unwrap())
}

/// The size in points of a diagram SVG (its root's width and height are
/// twice that, see `doubled`).
pub fn size_of(svg: &str) -> Option<(f32, f32)> {
    let start = svg.find("<svg")?;
    let root = &svg[start..start + svg[start..].find('>')?];
    let attr = |name: &str| -> Option<f32> {
        let i = root.find(&format!(" {name}=\""))? + name.len() + 3;
        root[i..i + root[i..].find('"')?].trim().trim_end_matches("px").parse().ok()
    };
    let (w, h) = (attr("width")? / 2., attr("height")? / 2.);
    (w.is_finite() && h.is_finite() && w > 0. && h > 0.).then_some((w, h))
}

/// The root sized at twice its viewBox.
fn doubled(svg: &str) -> Option<String> {
    let start = svg.find("<svg")?;
    let end = start + svg[start..].find('>')?;
    let root = &svg[start..end];
    let attr = |name: &str| -> Option<(usize, usize)> {
        let i = root.find(&format!(" {name}=\""))?;
        let close = i + name.len() + 3 + root[i + name.len() + 3..].find('"')?;
        Some((i, close + 1))
    };
    let (vb0, vb1) = attr("viewBox")?;
    let vb: Vec<f32> = root[vb0 + 10..vb1 - 1].split([' ', ',']).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    let [_, _, w, h] = vb[..] else { return None };
    let mut cut: Vec<(usize, usize)> = ["width", "height", "style"].iter().filter_map(|n| attr(n)).collect();
    cut.sort();
    let mut out = String::with_capacity(svg.len() + 40);
    out.push_str(&svg[..start]);
    let mut at = 0;
    for (a, b) in cut {
        out.push_str(&root[at..a]);
        at = b;
    }
    out.push_str(&root[at..]);
    out.push_str(&format!(" width=\"{}\" height=\"{}\"", 2. * w, 2. * h));
    out.push_str(&svg[end..]);
    Some(out)
}

/// How diagrams look now (made on the main thread: the Theme tokens follow
/// its light / dark state) and its cache key.
#[derive(Clone)]
pub struct Style {
    pub style: fdl_mermaid::Style,
    pub key: String,
}

/// `c` as an opaque `#rrggbb`, laid over `under` when translucent.
fn hex(c: Hsla, under: Hsla) -> String {
    let (c, u) = (Rgba::from(c), Rgba::from(under));
    let ch = |x: f32, y: f32| ((x * c.a + y * (1. - c.a)).clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}", ch(c.r, u.r), ch(c.g, u.g), ch(c.b, u.b))
}

/// A font list resvg finds: the interface font (the platform's own sans is
/// a name only the system text stack knows) and a CJK sans for fallback.
fn font() -> String {
    let ui = crate::theme::ui_family();
    let own = if cfg!(target_os = "macos") { "\"Helvetica Neue\", \"PingFang SC\"" } else if cfg!(target_os = "windows") { "\"Segoe UI\", \"Microsoft YaHei\"" } else { "\"Noto Sans\", \"Noto Sans CJK SC\"" };
    if ui == crate::theme::SYSTEM_FONT || ui.starts_with('.') { format!("{own}, sans-serif") } else { format!("\"{}\", {own}, sans-serif", ui.replace('"', "")) }
}

pub fn style() -> Style {
    use crate::theme::Theme;
    let page = Theme::code_bg();
    let x = |c: Hsla| hex(c, page);
    let accent = Theme::primary();
    let yellow = |f: fn(&str) -> Option<Hsla>| x(f("yellow").unwrap_or(page));
    let colors = fdl_mermaid::Palette {
        background: x(page),
        text: x(Theme::text()),
        line: x(Theme::text_caption()),
        node_fill: x(accent.opacity(0.08)),
        node_border: x(accent.opacity(0.5)),
        cluster_fill: x(Theme::bg_overlay()),
        cluster_border: x(Theme::line_sheet()),
        note_fill: yellow(crate::theme::block_bg),
        note_border: yellow(crate::theme::callout_border),
        activation_fill: x(Theme::bg_overlay()),
        frame: x(Theme::text_caption()),
        frame_fill: x(Theme::bg_overlay()),
        // the callout border tints: soft enough for text on them
        series: ["blue", "green", "orange", "purple", "red", "yellow", "gray"].iter().map(|h| x(crate::theme::callout_border(h).unwrap_or(page))).collect(),
    };
    let style = fdl_mermaid::Style { font: font(), font_size: 14., colors };
    let key = format!("{style:?}");
    Style { style, key }
}

enum Done {
    Ready(Diagram),
    Failed(String),
}

#[derive(Default)]
struct State {
    done: FxHashMap<u64, Done>,
    /// Insertion order of `done`, to drop the oldest.
    order: VecDeque<u64>,
    /// The last diagram each block drew, kept while a new one draws.
    last: FxHashMap<u64, Diagram>,
    /// The key each block is drawing now: typing asks again per keystroke,
    /// a newer source only starts once the one in flight is back.
    drawing: FxHashMap<u64, u64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Changes whenever a drawing finishes: rows with diagrams measure again.
pub fn generation() -> u64 { GENERATION.load(std::sync::atomic::Ordering::Relaxed) }

impl State {
    fn insert(&mut self, key: u64, d: Done) {
        if self.done.insert(key, d).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > MEMORY {
            for k in self.order.drain(..MEMORY / 2) {
                self.done.remove(&k);
            }
        }
    }
}

/// What block `block` (its id) shows for `source` in the current theme;
/// starts drawing it in the background when nothing is known yet.
pub fn look(block: u64, source: &str, cx: &mut App) -> Look {
    let style = style();
    let key = key(source, &style.key);
    let start = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let last = s.last.get(&block).cloned();
        match s.done.get(&key) {
            Some(Done::Ready(d)) => {
                let d = d.clone();
                s.last.insert(block, d.clone());
                return Err(Look::Ready(d));
            }
            Some(Done::Failed(m)) => return Err(Look::Failed(m.clone())),
            None => {}
        }
        // One drawing per block at a time; the row asks again when it lands.
        if s.drawing.contains_key(&block) {
            return Err(Look::Pending(last));
        }
        s.drawing.insert(block, key);
        Ok(last)
    });
    match start {
        Err(look) => look,
        Ok(last) => {
            let (source, renderer) = (source.to_string(), cx.svg_renderer());
            let task = cx.background_executor().spawn(async move { draw(&source, &style, &renderer) });
            cx.spawn(async move |cx| {
                let done = match task.await {
                    Ok(d) => Done::Ready(d),
                    Err(m) => Done::Failed(m),
                };
                STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    s.drawing.remove(&block);
                    if let Done::Ready(d) = &done {
                        s.last.insert(block, d.clone());
                    }
                    s.insert(key, done);
                });
                GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                cx.update(|cx| cx.refresh_windows());
            })
            .detach();
            Look::Pending(last)
        }
    }
}

/// Text measured per diagram, in bytes, before it gives up (a pasted novel).
const TEXT_BUDGET: usize = 1 << 20;

/// Widths from GPUI's SVG renderer, memoized for one drawing.
struct Widths<'a> {
    renderer: &'a SvgRenderer,
    family: &'a str,
    memo: RefCell<(FxHashMap<(String, u32, bool), f32>, usize)>,
}

impl fdl_mermaid::Measure for Widths<'_> {
    fn width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let k = (text.to_string(), size.to_bits(), bold);
        let mut w = self.memo.borrow_mut();
        if let Some(v) = w.0.get(&k) { return *v; }
        w.1 += 64 + text.len();
        if w.1 > TEXT_BUDGET { return 0.; }
        let font = SvgTextFont { family: self.family, size, weight: if bold { "bold" } else { "normal" }, style: "normal" };
        let v = self.renderer.measure_text_width(text, &font).unwrap_or(0.);
        w.0.insert(k, v);
        v
    }
}

/// `source` as a diagram (any thread).
pub fn draw(source: &str, style: &Style, renderer: &SvgRenderer) -> Result<Diagram, String> {
    let widths = Widths { renderer, family: &style.style.font, memo: RefCell::default() };
    let out = fdl_mermaid::render(source, &style.style, &widths).map_err(|e| e.to_string())?;
    if widths.memo.borrow().1 > TEXT_BUDGET { return Err("The diagram has too much text".into()); }
    let svg = doubled(&out.svg).ok_or("The diagram has no size")?;
    let size = size_of(&svg).ok_or("The diagram has no size")?;
    Ok(Diagram { image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())), size })
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in gpui's own `test` attribute.
    use super::{doubled, key, size_of};

    #[test]
    fn keys_follow_source_and_style() {
        let src = "flowchart LR\n  A --> B";
        assert_eq!(key(src, "light"), key(src, "light"));
        assert_ne!(key(src, "light"), key(src, "dark"), "light and dark draw differently");
        assert_ne!(key(src, "light"), key("flowchart LR\n  A --> C", "light"));
        assert_ne!(key("ab", "x"), key("a", "bx"), "source and style don't run together");
    }

    #[test]
    fn roots_are_sized_at_twice_their_view_box() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="180.75" height="87" viewBox="0 0 180.75 87"><rect/></svg>"#;
        let d = doubled(svg).unwrap();
        assert_eq!(d, r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 180.75 87" width="361.5" height="174"><rect/></svg>"#);
        assert_eq!(size_of(&d), Some((180.75, 87.)));
        assert_eq!(doubled(r#"<svg width="10"/>"#), None, "no viewBox");
        assert_eq!(size_of(r#"<svg width="100%" viewBox="0 0 1 1"/>"#), None);
        assert_eq!(size_of("not svg"), None);
    }

    fn renderer() -> gpui::SvgRenderer { gpui::SvgRenderer::new(std::sync::Arc::new(crate::assets::Assets)) }

    /// Every sample: parsed, laid out, drawable by resvg, in both looks.
    #[test]
    fn samples_draw() {
        let r = renderer();
        for (name, src) in SAMPLES {
            for dark in [false, true] {
                let d = super::draw(src, &crate::theme::with_dark(dark, super::style), &r).unwrap_or_else(|e| panic!("{name}: {e}"));
                let svg = std::str::from_utf8(&d.image.bytes).unwrap();
                assert!(d.size.0 > 20. && d.size.1 > 20., "{name}: {:?}", d.size);
                r.parse_svg(svg.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
    }

    /// Colours come from the Theme tokens, so light and dark differ and both keys stay apart.
    #[test]
    fn styles_follow_the_theme() {
        let (light, dark) = (crate::theme::with_dark(false, super::style), crate::theme::with_dark(true, super::style));
        assert_ne!(light.key, dark.key);
        let r = renderer();
        let src = SAMPLES[0].1;
        let (a, b) = (super::draw(src, &light, &r).unwrap(), super::draw(src, &dark, &r).unwrap());
        assert_ne!(a.image.bytes, b.image.bytes);
    }

    #[test]
    fn what_it_cant_draw_says_why() {
        let r = renderer();
        let e = super::draw("flowchart TD\n  A --> B[[ oops\n", &super::style(), &r).err().unwrap();
        assert!(e.starts_with("line 2:"), "{e}");
        assert!(super::draw("hello there", &super::style(), &r).err().unwrap().contains("aren't supported"));
        assert!(super::draw("sequenceDiagram\n  A->>", &super::style(), &r).is_err());
        assert!(super::draw("journey\n  title x", &super::style(), &r).err().unwrap().contains("aren't supported"));
        assert!(super::draw("pie title x\n \"a\" : -1", &super::style(), &r).is_err());
    }

    /// Text is measured in the font drawn: a label's box is wider than its text.
    #[test]
    fn labels_fit() {
        let r = renderer();
        let style = super::style();
        let long = "flowchart LR\n  A[一个相当长的中文标签 with English words mixed in]";
        let d = super::draw(long, &style, &r).unwrap();
        let w = r.measure_text_width("一个相当长的中文标签", &gpui::SvgTextFont { family: &style.style.font, size: 14., weight: "normal", style: "normal" }).unwrap();
        assert!(w > 100. && d.size.0 > w, "{w} {:?}", d.size);
    }

    /// `FDL_MERMAID_TIMES=1 cargo test --release mermaid::tests::timing -- --nocapture`
    #[test]
    fn timing() {
        let r = renderer();
        let style = super::style();
        // The system font list GPUI's SVG text loads once per run, whatever draws text first.
        let t = web_time::Instant::now();
        let _ = r.measure_text_width("x", &gpui::SvgTextFont { family: &style.style.font, size: 14., weight: "normal", style: "normal" });
        let fonts = t.elapsed();
        let t = web_time::Instant::now();
        super::draw(FLOW20, &style, &r).unwrap();
        let first = t.elapsed();
        let t = web_time::Instant::now();
        for _ in 0..10 { super::draw(FLOW20, &style, &r).unwrap(); }
        let warm = t.elapsed() / 10;
        if std::env::var_os("FDL_MERMAID_TIMES").is_some() {
            println!("fonts {fonts:?}; 20-node flowchart: first {first:?}, then {warm:?}");
            for (name, src) in SAMPLES {
                let t = web_time::Instant::now();
                super::draw(src, &style, &r).unwrap();
                println!("{name}: {:?}", t.elapsed());
            }
        }
    }

    const FLOW20: &str = "flowchart TD
  A[开始] --> B{已登录?}
  B -- 是 --> C[读取文档库]
  B -- 否 --> D[登录页]
  D --> E[输入账号] --> F{校验}
  F -- 失败 --> D
  F -- 成功 --> C
  C --> G[加载侧栏]
  C --> H[加载最近文档]
  G --> I[(索引)]
  H --> I
  I --> J[渲染列表]
  J --> K{有更新?}
  K -- 有 --> L[同步]
  L --> M[合并冲突]
  M --> N[写回磁盘]
  K -- 无 --> O[空闲]
  N --> O
  O --> P((结束))
  E --> Q[忘记密码]
  Q --> R[发送邮件] --> S[重置] --> D
  J --> T[预览]";

    pub(super) const SAMPLES: &[(&str, &str)] = &[
        ("flowchart", "flowchart LR\n  A[开始] --> B{检查}\n  B -- 是 --> C[完成]\n  B -- 否 --> D[重试] --> B"),
        ("subgraphs", "flowchart TB\n  c1-->a2\n  subgraph one\n  a1-->a2\n  end\n  subgraph two\n  b1-->b2\n  end\n  one --> two"),
        ("sequence", "sequenceDiagram\n  participant U as 用户\n  U->>App: 打开\n  alt 命中\n    App-->>U: 304\n  else\n    App-->>U: 200\n  end\n  Note over U,App: 同步\n  loop 每 30 秒\n    App->>App: 心跳\n  end"),
        ("state", "stateDiagram-v2\n  [*] --> 草稿\n  草稿 --> 审阅 : 提交\n  state 审阅 {\n    [*] --> 初审\n    --\n    [*] --> 复核\n  }\n  state c <<choice>>\n  审阅 --> c\n  c --> 发布 : 通过\n  c --> 草稿 : 退回\n  note right of 发布 : 对外可见\n  发布 --> [*]"),
        ("class", "classDiagram\n  class 订单 {\n    <<entity>>\n    +String id\n    +total() Money\n  }\n  客户 \"1\" --> \"*\" 订单 : 下单\n  订单 *-- 明细\n  支付 <|.. 卡支付\n  订单 ..> 支付"),
        ("pie", "pie showData title 来源\n  \"搜索\" : 42.5\n  \"直接访问\" : 30\n  \"推荐\" : 12"),
    ];
}
