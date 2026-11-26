//! Mermaid diagrams (#23). A code block whose language is Mermaid shows its
//! diagram unless switched to its code (`show_code`, saved as `view="code"`).
//! merman (pure Rust, following mermaid 12.3) parses, lays out and writes the
//! SVG on a background thread, in Mermaid's `base` theme coloured from the
//! Theme tokens and in the interface font; text is measured by GPUI's SVG
//! renderer, the resvg that then draws it (`img()`), so labels fit their
//! boxes. Its resvg-safe pipeline turns HTML labels into SVG text. Nothing
//! goes over the network; every platform draws. Results are kept in memory
//! by source and theme.

use gpui::*;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

/// The code block language (`lang::LANGS`) drawn as a diagram.
pub const LANG: &str = "Mermaid";
/// The renderer: part of the cache key, so an upgrade draws anew.
const VERSION: &str = "merman 0.11.0";
/// Cached diagrams before the oldest half is dropped.
const MEMORY: usize = 256;

/// A drawn diagram: the SVG at twice its size (so the 1× rasterization GPUI
/// gives an SVG `Image` is sharp on Retina) and its size in points.
#[derive(Clone)]
pub struct Diagram {
    pub image: Arc<Image>,
    pub size: (f32, f32),
}

/// What a block shows: the diagram, or while drawing / after an error, the
/// last diagram that block drew (if any).
#[derive(Clone)]
pub enum Look {
    Ready(Diagram),
    Pending(Option<Diagram>),
    Failed(String, Option<Diagram>),
}

/// The cache key of `source` drawn with `style` (`Style::config`).
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

/// merman's root is `width="100%"` with a `max-width` style; sized here at
/// twice its viewBox instead.
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

/// How diagrams look now: Mermaid's site config, made on the main thread
/// (the Theme tokens follow its light / dark state), and the font it names.
#[derive(Clone)]
pub struct Style {
    pub config: String,
    pub font: String,
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
    let dark = crate::theme::dark();
    let page = Theme::code_bg();
    let x = |c: Hsla| hex(c, page);
    let (text, caption, accent) = (x(Theme::text()), x(Theme::text_caption()), Theme::primary());
    let node = x(accent.opacity(0.08));
    let border = x(accent.opacity(0.5));
    let gray = x(Theme::fill_hover_strong());
    let line = x(Theme::line_sheet());
    let soft = x(Theme::bg_overlay());
    let note = x(crate::theme::block_bg("yellow").unwrap_or(page));
    let note_border = x(crate::theme::callout_border("yellow").unwrap_or(page));
    let font = font();
    // Series colours (pie slices, git branches, mindmap and timeline
    // sections, journey sections): the block colours' swatches.
    let hues = ["blue", "green", "orange", "purple", "red", "yellow", "gray", "blue"];
    let mut vars = serde_json::json!({
        "darkMode": dark, "background": x(page), "fontFamily": font, "fontSize": "14px",
        "primaryColor": node, "primaryTextColor": text, "primaryBorderColor": border,
        "secondaryColor": gray, "secondaryTextColor": text, "secondaryBorderColor": line,
        "tertiaryColor": soft, "tertiaryTextColor": text, "tertiaryBorderColor": line,
        "mainBkg": node, "nodeBorder": border, "nodeTextColor": text, "textColor": text, "titleColor": text,
        "lineColor": caption, "defaultLinkColor": caption, "edgeLabelBackground": x(page),
        "clusterBkg": soft, "clusterBorder": line,
        "noteBkgColor": note, "noteBorderColor": note_border, "noteTextColor": text,
        "actorBkg": node, "actorBorder": border, "actorTextColor": text, "actorLineColor": caption,
        "signalColor": caption, "signalTextColor": text, "labelBoxBkgColor": node, "labelBoxBorderColor": border,
        "labelTextColor": text, "loopTextColor": text, "activationBkgColor": soft, "activationBorderColor": border,
        "classText": text, "labelColor": text, "attributeBackgroundColorOdd": x(page), "attributeBackgroundColorEven": soft,
        "pieTitleTextColor": text, "pieSectionTextColor": text, "pieLegendTextColor": text,
        "pieStrokeColor": x(page), "pieOuterStrokeColor": line,
        "quadrant1Fill": node, "quadrant2Fill": soft, "quadrant3Fill": node, "quadrant4Fill": soft,
        "quadrant1TextFill": text, "quadrant2TextFill": text, "quadrant3TextFill": text, "quadrant4TextFill": text,
        "quadrantPointFill": x(accent), "quadrantPointTextFill": text, "quadrantTitleFill": text,
        "quadrantXAxisTextFill": text, "quadrantYAxisTextFill": text,
        "quadrantExternalBorderStrokeFill": line, "quadrantInternalBorderStrokeFill": line,
        "sectionBkgColor": soft, "altSectionBkgColor": x(page), "sectionBkgColor2": node,
        "taskBkgColor": node, "taskBorderColor": border, "taskTextColor": text, "taskTextOutsideColor": text, "taskTextLightColor": text,
        "taskTextDarkColor": text, "activeTaskBkgColor": x(accent.opacity(0.2)), "activeTaskBorderColor": x(accent),
        "doneTaskBkgColor": gray, "doneTaskBorderColor": line, "critBkgColor": x(Theme::danger().opacity(0.15)), "critBorderColor": x(Theme::danger()),
        "gridColor": line, "todayLineColor": x(Theme::danger()),
        "git0": x(accent), "commitLabelColor": text, "commitLabelBackground": soft, "tagLabelColor": text, "tagLabelBackground": soft, "tagLabelBorder": line,
    });
    if let Some(map) = vars.as_object_mut() {
        let solids: Vec<String> = hues[..6].iter().map(|h| x(crate::theme::callout_border(h).unwrap_or(accent))).collect();
        map.insert("xyChart".into(), serde_json::json!({
            "backgroundColor": "transparent", "titleColor": text, "plotColorPalette": solids.join(","),
            "xAxisLabelColor": caption, "xAxisTitleColor": text, "xAxisTickColor": caption, "xAxisLineColor": caption,
            "yAxisLabelColor": caption, "yAxisTitleColor": text, "yAxisTickColor": caption, "yAxisLineColor": caption,
        }));
        for i in 0..12 {
            let h = hues[i % hues.len()];
            map.insert(format!("cScaleInv{i}"), x(crate::theme::callout_border(h).unwrap_or(accent)).into());
            if i >= hues.len() { map.insert(format!("cScale{i}"), x(crate::theme::block_bg(&format!("medium-{h}")).unwrap_or(page)).into()); map.insert(format!("cScaleLabel{i}"), text.clone().into()); }
        }
        for (i, h) in hues.iter().enumerate() {
            let fill = x(crate::theme::block_bg(&format!("medium-{h}")).unwrap_or(page));
            let solid = x(crate::theme::callout_border(h).unwrap_or(accent));
            map.insert(format!("cScale{i}"), fill.clone().into());
            map.insert(format!("cScaleLabel{i}"), text.clone().into());
            map.insert(format!("pie{}", i + 1), fill.into());
            if i > 0 { map.insert(format!("git{i}"), solid.clone().into()); }
            map.insert(format!("gitBranchLabel{i}"), text.clone().into());
            map.insert(format!("gitInv{i}"), solid.into());
        }
    }
    let config = serde_json::json!({
        "theme": "base", "darkMode": dark, "fontFamily": font, "htmlLabels": false,
        "flowchart": { "htmlLabels": false }, "themeVariables": vars,
    });
    Style { config: config.to_string(), font }
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
    /// The last diagram each block drew, kept while a new one draws or fails.
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
    let key = key(source, &style.config);
    let start = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let last = s.last.get(&block).cloned();
        match s.done.get(&key) {
            Some(Done::Ready(d)) => {
                let d = d.clone();
                s.last.insert(block, d.clone());
                return Err(Look::Ready(d));
            }
            Some(Done::Failed(m)) => return Err(Look::Failed(m.clone(), last)),
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

/// `source` as a diagram (any thread).
pub fn draw(source: &str, style: &Style, renderer: &SvgRenderer) -> Result<Diagram, String> {
    let svg = render(source, style, renderer)?;
    let svg = doubled(&svg).ok_or_else(|| crate::settings::tr("图表没有尺寸", "The diagram has no size").to_string())?;
    let size = size_of(&svg).ok_or_else(|| crate::settings::tr("图表没有尺寸", "The diagram has no size").to_string())?;
    Ok(Diagram { image: Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())), size })
}

/// merman's SVG for `source`, its text measured in `style.font` by
/// `renderer` (memoized; past `TEXT_BUDGET` the drawing is cancelled).
fn render(source: &str, style: &Style, renderer: &SvgRenderer) -> Result<String, String> {
    use merman::svg::{DeterministicTextMeasurer, MeasurementProfileId, SvgPipeline, TextMeasurementPolicy, TextMeasurementProfile, TextMeasurementProfileIdentity};
    use std::sync::Mutex;
    let control = merman::OperationControl::new();
    let widths: Arc<Mutex<(FxHashMap<(String, u64, Option<String>, Option<String>), f64>, usize)>> = Arc::new(Mutex::new((FxHashMap::default(), 0)));
    let measure = {
        let (renderer, family, control, widths) = (renderer.clone(), style.font.clone(), control.clone(), widths.clone());
        move |text: &str, st: &merman::svg::TextStyle| -> f64 {
            let k = (text.to_string(), st.font_size.to_bits(), st.font_weight.clone(), st.font_style.clone());
            let mut w = widths.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(v) = w.0.get(&k) { return *v; }
            w.1 += 64 + text.len();
            if w.1 > TEXT_BUDGET { control.cancel(); return 0.; }
            let font = SvgTextFont { family: &family, size: st.font_size as f32, weight: st.font_weight.as_deref().unwrap_or("normal"), style: st.font_style.as_deref().unwrap_or("normal") };
            let v = renderer.measure_text_width(text, &font).map(f64::from).unwrap_or(0.);
            w.0.insert(k, v);
            v
        }
    };
    let id = |s: &str| TextMeasurementProfileIdentity::new(MeasurementProfileId::new(s).map_err(|e| e.to_string())?, "1").map_err(|e| e.to_string());
    let profile = TextMeasurementProfile::new(id("quiet-md.svg-renderer")?, DeterministicTextMeasurer::default().with_width_callback(measure));
    let config = merman::SourcePresentationConfig::from_value(serde_json::from_str(&style.config).map_err(|e| e.to_string())?);
    let request = merman::SvgRequest {
        environment: merman::SvgEnvironment::deterministic().with_text_measurement_policy(TextMeasurementPolicy::uniform(profile)),
        pipeline: Some(SvgPipeline::resvg_safe()),
        ..Default::default()
    };
    let renderer = merman::Renderer::new().with_presentation_engine(merman::MermaidEngine::new().with_site_config(config));
    let out = renderer.render(merman::RenderRequest::svg(source, control.clone(), request));
    if widths.lock().map(|w| w.1 > TEXT_BUDGET).unwrap_or(false) {
        return Err(crate::settings::tr("图表文字太多", "The diagram has too much text").into());
    }
    match out.map_err(|e| e.to_string())? {
        merman::RenderOutput::Svg(Some(svg)) => Ok(svg.svg().to_string()),
        _ => Err(crate::settings::tr("不是 Mermaid 图表", "Not a Mermaid diagram").into()),
    }
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
        // Pinned, not a std hash.
        assert_eq!(key("graph TD", "s"), {
            use sha2::{Digest, Sha256};
            let d = Sha256::new().chain_update(b"merman 0.11.0").chain_update([0u8]).chain_update(b"s").chain_update([0u8]).chain_update(b"graph TD").finalize();
            u64::from_le_bytes(d[..8].try_into().unwrap())
        });
    }

    #[test]
    fn roots_are_sized_at_twice_their_view_box() {
        let svg = r#"<svg id="m" width="100%" xmlns="http://www.w3.org/2000/svg" style="max-width: 180.75px;" viewBox="0 0 180.75 87" role="graphics-document"><style>#m{}</style></svg>"#;
        let d = doubled(svg).unwrap();
        assert_eq!(d, r#"<svg id="m" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 180.75 87" role="graphics-document" width="361.5" height="174"><style>#m{}</style></svg>"#);
        assert_eq!(size_of(&d), Some((180.75, 87.)));
        assert_eq!(doubled(r#"<svg width="10"/>"#), None, "no viewBox");
        assert_eq!(size_of(r#"<svg width="100%" viewBox="0 0 1 1"/>"#), None);
        assert_eq!(size_of("not svg"), None);
    }

    fn renderer() -> gpui::SvgRenderer { gpui::SvgRenderer::new(std::sync::Arc::new(crate::assets::Assets)) }

    /// One of each supported family: parsed, laid out, drawable by resvg, in both looks.
    #[test]
    fn every_family_draws() {
        let r = renderer();
        for (name, src) in SAMPLES {
            for dark in [false, true] {
                let d = super::draw(src, &crate::theme::with_dark(dark, super::style), &r).unwrap_or_else(|e| panic!("{name}: {e}"));
                let svg = std::str::from_utf8(&d.image.bytes).unwrap();
                assert!(!svg.contains("<foreignObject"), "{name}: resvg can't draw HTML labels");
                assert!(d.size.0 > 20. && d.size.1 > 20., "{name}: {:?}", d.size);
                r.parse_svg(svg.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
            }
        }
    }

    /// Colours come from the Theme tokens, so light and dark differ and both keys stay apart.
    #[test]
    fn styles_follow_the_theme() {
        let (light, dark) = (crate::theme::with_dark(false, super::style), crate::theme::with_dark(true, super::style));
        assert_ne!(light.config, dark.config);
        assert!(light.config.contains("\"theme\":\"base\""));
        let r = renderer();
        let src = SAMPLES[0].1;
        let (a, b) = (super::draw(src, &light, &r).unwrap(), super::draw(src, &dark, &r).unwrap());
        assert_ne!(a.image.bytes, b.image.bytes);
    }

    #[test]
    fn syntax_errors_say_why() {
        let r = renderer();
        let e = super::draw("flowchart TD\n  A --> B[[[ oops\n", &super::style(), &r).err().unwrap();
        assert!(e.contains("parse error"), "{e}");
        assert!(super::draw("hello there", &super::style(), &r).is_err());
        assert!(super::draw("sequenceDiagram\n  A->>", &super::style(), &r).is_err());
    }

    /// `FDL_MERMAID_TIMES=1 cargo test --release mermaid::tests::timing -- --nocapture`
    #[test]
    fn timing() {
        let r = renderer();
        let style = super::style();
        // The system font list GPUI's SVG text loads once per run, whatever draws text first.
        let t = web_time::Instant::now();
        let _ = r.measure_text_width("x", &gpui::SvgTextFont { family: &style.font, size: 14., weight: "normal", style: "normal" });
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
        ("sequence", "sequenceDiagram\n  participant U as 用户\n  U->>App: 打开\n  alt 命中\n    App-->>U: 304\n  else\n    App-->>U: 200\n  end\n  Note over U,App: 同步\n  loop 每 30 秒\n    App->>App: 心跳\n  end"),
        ("class", "classDiagram\n  class Animal {\n    +String name\n    +eat() void\n  }\n  Animal <|-- Dog\n  Animal <|-- Cat"),
        ("state", "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Running : start\n  Running --> Idle : stop\n  Running --> [*]"),
        ("er", "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  CUSTOMER {\n    string name\n  }"),
        ("gantt", "gantt\n  title 计划\n  dateFormat YYYY-MM-DD\n  section 开发\n  解析 :a1, 2026-01-01, 7d\n  布局 :after a1, 10d"),
        ("pie", "pie title 时间\n  \"写作\" : 45\n  \"阅读\" : 30\n  \"整理\" : 25"),
        ("mindmap", "mindmap\n  root((Quiet.md))\n    编辑\n      块\n    导出\n      PDF"),
        ("journey", "journey\n  title 写笔记\n  section 开始\n    打开应用: 5: 我\n    输入: 3: 我"),
        ("timeline", "timeline\n  title 历史\n  2024 : 原型\n  2025 : 公测 : 插件"),
        ("gitGraph", "gitGraph\n  commit\n  branch dev\n  commit\n  checkout main\n  merge dev"),
        ("quadrantChart", "quadrantChart\n  title Reach\n  x-axis Low --> High\n  y-axis Low --> High\n  A: [0.3, 0.6]\n  B: [0.45, 0.23]"),
        ("xychart", "xychart-beta\n  title \"Sales\"\n  x-axis [jan, feb, mar]\n  y-axis \"Revenue\" 0 --> 100\n  bar [30, 60, 90]\n  line [20, 50, 80]"),
    ];
}
