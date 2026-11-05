//! Pele runs locally in QuickJS with no host functions or project imports. The
//! code block stays the document's truth; a rendered diagram is a disposable
//! image in a bounded per-editor cache, keyed by everything that shapes it.
use anyhow::{Result, ensure};
use gpui::RenderImage;
use std::{collections::{HashMap, hash_map::DefaultHasher}, hash::{Hash, Hasher}, sync::{Arc, LazyLock, Mutex}, time::{Duration, Instant}};

const BUNDLE: &str = include_str!("../assets/pele/pele.global.js");
const FONT: &str = if cfg!(target_os = "windows") { "Segoe UI" } else { "Helvetica Neue" };
const MONO: &str = if cfg!(target_os = "windows") { "Consolas" } else { "Menlo" };
static SERIAL: Mutex<()> = Mutex::new(());
#[derive(Clone)]
pub struct Palette { pub background: String, pub text: String, pub muted: String, pub surface: String, pub border: String, pub accent: String }
impl Palette {
    fn key(&self) -> String { format!("{}{}{}{}{}{}", self.background, self.text, self.muted, self.surface, self.border, self.accent) }
    pub fn current() -> Self {
        fn hex(c: gpui::Hsla) -> String { let c = c.to_rgb(); let bg = crate::theme::Theme::bg_body().to_rgb(); let mix = |v: f32, b: f32| ((v * c.a + b * (1. - c.a)) * 255.).round() as u8; format!("#{:02x}{:02x}{:02x}", mix(c.r, bg.r), mix(c.g, bg.g), mix(c.b, bg.b)) }
        use crate::theme::Theme;
        Self { background: hex(Theme::bg_body()), text: hex(Theme::text()), muted: hex(Theme::text_caption()), surface: hex(Theme::window()), border: hex(Theme::text_caption()), accent: hex(Theme::primary()) }
    }
}
/// A rasterized diagram. `width`/`height` are logical pixels.
pub struct Raster { pub bgra: image::RgbaImage, pub width: f32, pub height: f32 }
pub fn render(source: &str, width: f32, palette: &Palette, density: f32) -> Result<Raster> {
    ensure!(source.len() <= 16 * 1024 && source.lines().count() <= 256, "Diagram exceeds preview limits; source is available");
    let _guard = SERIAL.lock().map_err(|_| anyhow::anyhow!("Diagram worker unavailable"))?;
    let runtime = rquickjs::Runtime::new()?;
    runtime.set_memory_limit(64 * 1024 * 1024);
    runtime.set_max_stack_size(1024 * 1024);
    let deadline = Instant::now() + Duration::from_secs(2);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let context = rquickjs::Context::full(&runtime)?;
    let svg = context.with(|ctx| -> Result<String> {
        ctx.eval::<(), _>(BUNDLE).map_err(|e| anyhow::anyhow!("Could not load Pele: {e}"))?;
        ctx.globals().set("source", source)?;
        ctx.globals().set("width", width.clamp(200., 1600.))?;
        // Source is a JS string value, never evaluated as code.
        let result: String = ctx.eval("JSON.stringify((()=>{try{return {svg:pele.render(source,{responsive:false,maxWidth:width,links:false,images:false,limit:16384,outputLimit:2097152,maxEdges:256}).svg}}catch(e){return {error:String(e.message)}}})())")
            .map_err(|e| anyhow::anyhow!("Diagram exceeded runtime limits: {e}"))?;
        let value: serde_json::Value = serde_json::from_str(&result)?;
        ensure!(value["error"].is_null(), "{}", value["error"].as_str().unwrap_or("Diagram error"));
        Ok(value["svg"].as_str().unwrap_or_default().to_string())
    })?;
    let svg = resolve_variables(&svg, palette);
    static FONTS: LazyLock<Arc<resvg::usvg::fontdb::Database>> = LazyLock::new(|| {
        let mut db = resvg::usvg::fontdb::Database::new(); db.load_system_fonts(); Arc::new(db)
    });
    let options = resvg::usvg::Options {
        fontdb: FONTS.clone(),
        font_family: FONT.into(),
        image_href_resolver: resvg::usvg::ImageHrefResolver { resolve_data: Box::new(|_, _, _| None), resolve_string: Box::new(|_, _| None) },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(&svg, &options)?;
    let size = tree.size();
    // Sharp on this display, and bounded: 16 MB of pixels at most.
    let scale = density.clamp(1., 3.).min(4096. / size.width().max(size.height())).min((4_194_304. / (size.width() * size.height())).sqrt());
    let mut pixmap = resvg::tiny_skia::Pixmap::new((size.width() * scale).ceil().max(1.) as u32, (size.height() * scale).ceil().max(1.) as u32).ok_or_else(|| anyhow::anyhow!("Diagram too large"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut bgra = pixmap.take();
    // GPUI wants premultiplied BGRA, which tiny-skia already is apart from the order.
    for [r, _, b, _] in bgra.as_chunks_mut::<4>().0 { std::mem::swap(r, b); }
    let bgra = image::RgbaImage::from_raw(w, h, bgra).ok_or_else(|| anyhow::anyhow!("Diagram buffer"))?;
    Ok(Raster { bgra, width: size.width(), height: size.height() })
}

#[derive(Clone)]
pub enum Diagram { Pending, Ready { image: Arc<RenderImage>, width: f32, height: f32 }, Failed(String) }
struct Slot { diagram: Diagram, used: u64, bytes: usize }
pub struct Job { pub key: u64, pub source: String, pub width: f32, pub palette: Palette, pub density: f32 }
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;
/// Diagrams render only when painted, one at a time, newest request first; a
/// request whose row scrolled away before its turn simply never runs.
#[derive(Default)]
pub struct Diagrams { slots: HashMap<u64, Slot>, queue: Vec<u64>, jobs: HashMap<u64, Job>, tick: u64, pub running: bool, pub density: f32 }
impl Diagrams {
    /// What to show for `source`, requesting it if needed; the bool says a new job was queued.
    pub fn request(&mut self, source: &str, width: f32, palette: &Palette) -> (Diagram, bool) {
        let density = self.density.max(1.);
        let mut h = DefaultHasher::new();
        source.hash(&mut h); (width as u32).hash(&mut h); palette.key().hash(&mut h); ((density * 4.) as u32).hash(&mut h);
        let key = h.finish();
        self.tick += 1;
        if let Some(slot) = self.slots.get_mut(&key) { slot.used = self.tick; return (slot.diagram.clone(), false); }
        let queued = !self.jobs.contains_key(&key);
        if queued { self.jobs.insert(key, Job { key, source: source.to_string(), width, palette: palette.clone(), density }); }
        self.queue.retain(|k| *k != key); self.queue.push(key);
        (Diagram::Pending, queued)
    }
    pub fn next_job(&mut self) -> Option<Job> {
        let key = self.queue.pop()?;
        // Older requests lost the race for the viewport: they ask again if painted.
        let job = self.jobs.remove(&key);
        if self.queue.len() > 8 { let stale = self.queue.drain(..self.queue.len() - 8).collect::<Vec<_>>(); for k in stale { self.jobs.remove(&k); } }
        job
    }
    /// Store a result; returns images evicted to stay within the budget.
    pub fn finish(&mut self, key: u64, result: Result<Raster>) -> Vec<Arc<RenderImage>> {
        self.tick += 1;
        let (diagram, bytes) = match result {
            Ok(r) => { let bytes = r.bgra.as_raw().len(); (Diagram::Ready { image: Arc::new(RenderImage::new(vec![image::Frame::new(r.bgra)])), width: r.width, height: r.height }, bytes) }
            Err(e) => (Diagram::Failed(e.to_string()), 0),
        };
        self.slots.insert(key, Slot { diagram, used: self.tick, bytes });
        let mut evicted = vec![];
        while self.slots.len() > 1 && (self.slots.len() > MAX_ENTRIES || self.slots.values().map(|s| s.bytes).sum::<usize>() > MAX_BYTES) {
            let Some(oldest) = self.slots.iter().min_by_key(|(_, s)| s.used).map(|(k, _)| *k) else { break };
            if let Some(Slot { diagram: Diagram::Ready { image, .. }, .. }) = self.slots.remove(&oldest) { evicted.push(image); }
        }
        evicted
    }
}
fn resolve_variables(svg: &str, palette: &Palette) -> String {
    let mut variables = HashMap::<String, String>::new();
    for (name, value) in [("bg", &palette.background), ("fg", &palette.text), ("muted", &palette.muted), ("line", &palette.text), ("surface", &palette.surface), ("surface-alt", &palette.surface), ("border", &palette.border), ("accent", &palette.accent)] {
        variables.insert(format!("--pele-{name}"), value.clone());
    }
    variables.insert("--pele-font".into(), FONT.into());
    variables.insert("--pele-font-mono".into(), MONO.into());
    let declaration = regex::Regex::new(r#"(--_[a-z]+):([^;"]+)"#).unwrap();
    for captures in declaration.captures_iter(svg) { variables.insert(captures[1].into(), captures[2].into()); }
    let var = regex::Regex::new(r"var\((--[a-zA-Z_0-9-]+)(?:,([^()]*))?\)").unwrap();
    let mut output = svg.to_string();
    for _ in 0..12 {
        let next = var.replace_all(&output, |caps: &regex::Captures<'_>| variables.get(&caps[1]).cloned().unwrap_or_else(|| caps.get(2).map_or("", |v| v.as_str()).to_string())).into_owned();
        if next == output { break; } output = next;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_preview_pele_offline() {
        let p = Palette { background: "white".into(), text: "black".into(), muted: "gray".into(), surface: "white".into(), border: "black".into(), accent: "blue".into() };
        for source in ["flowchart LR\nA[Start]-->B[Finish]", "sequenceDiagram\nAlice->>Bob: Hello", "classDiagram\nAnimal <|-- Duck"] {
            let raster = render(source, 700., &p, 2.).unwrap();
            assert!(raster.width > 30. && raster.height > 20. && raster.bgra.width() as f32 >= raster.width);
            assert!(raster.bgra.pixels().filter(|p| p.0[3] > 0 && p.0[0] < 200).count() > 50);
        }
        assert!(render("not a diagram", 700., &p, 2.).is_err());
        assert!(render(&"x".repeat(17_000), 700., &p, 2.).is_err());
        // A malformed diagram is a visible failure in the cache, and the budget holds.
        let mut d = Diagrams::default();
        let (first, queued) = d.request("flowchart LR\nA-->B", 500., &p); assert!(matches!(first, Diagram::Pending) && queued);
        let (_, again) = d.request("flowchart LR\nA-->B", 500., &p); assert!(!again);
        let job = d.next_job().unwrap();
        d.finish(job.key, render(&job.source, job.width, &job.palette, 1.));
        assert!(matches!(d.request("flowchart LR\nA-->B", 500., &p).0, Diagram::Ready { .. }));
        d.request("nonsense", 500., &p); let job = d.next_job().unwrap();
        d.finish(job.key, render(&job.source, job.width, &job.palette, 1.));
        assert!(matches!(d.request("nonsense", 500., &p).0, Diagram::Failed(_)));
        for i in 0..80 { let k = i as u64; d.finish(k, Err(anyhow::anyhow!("x"))); }
        assert!(d.slots.len() <= MAX_ENTRIES);
    }
}
