//! Pele runs locally in QuickJS with no host functions or project imports.
use anyhow::{Result, ensure};
use std::{collections::HashMap, sync::{Arc, LazyLock, Mutex}, time::{Duration, Instant}};

const BUNDLE: &str = include_str!("../assets/pele/pele.global.js");
const FONT: &str = if cfg!(target_os = "windows") { "Segoe UI" } else { "Helvetica Neue" };
const MONO: &str = if cfg!(target_os = "windows") { "Consolas" } else { "Menlo" };
static SERIAL: Mutex<()> = Mutex::new(());
#[derive(Clone)]
pub struct Palette { pub background: String, pub text: String, pub muted: String, pub surface: String, pub border: String, pub accent: String }
impl Palette {
    pub fn current() -> Self {
        fn hex(c: gpui::Hsla) -> String { let c = c.to_rgb(); let bg = crate::theme::Theme::bg_body().to_rgb(); let mix = |v: f32, b: f32| ((v * c.a + b * (1. - c.a)) * 255.).round() as u8; format!("#{:02x}{:02x}{:02x}", mix(c.r, bg.r), mix(c.g, bg.g), mix(c.b, bg.b)) }
        use crate::theme::Theme;
        Self { background: hex(Theme::bg_body()), text: hex(Theme::text()), muted: hex(Theme::text_caption()), surface: hex(Theme::window()), border: hex(Theme::line_border()), accent: hex(Theme::primary()) }
    }
}
pub fn render(source: &str, width: f32, palette: &Palette) -> Result<Vec<u8>> {
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
    let scale = 2f32.min(1600. / size.width().max(size.height())).min((1_048_576. / (size.width() * size.height())).sqrt());
    let mut pixmap = resvg::tiny_skia::Pixmap::new((size.width() * scale).ceil().max(1.) as u32, (size.height() * scale).ceil().max(1.) as u32).ok_or_else(|| anyhow::anyhow!("Diagram too large"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
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

// Decode into bounded temporary PNGs before the editor's image loader sees them.
pub fn prepare_images(doc: &mut crate::model::Document) -> Vec<std::path::PathBuf> {
    use std::io::Read;
    fn visit(blocks: &mut [crate::model::Block], files: &mut Vec<std::path::PathBuf>, pixels: &mut u64) {
        use crate::model::{BlockKind, RichText};
        for block in blocks {
            if let BlockKind::Image { src, .. } = &block.kind {
                let original = src.clone();
                let converted = (|| -> Result<std::path::PathBuf> {
                    ensure!(files.len() < 32, "Too many images");
                    let path = std::path::Path::new(&original);
                    ensure!(path.is_absolute() && path.canonicalize()? == path, "Invalid image path");
                    let mut bytes = Vec::new();
                    std::fs::File::open(path)?.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                    ensure!(bytes.len() <= 8 * 1024 * 1024, "Image too large");
                    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
                    ensure!(matches!(reader.format(), Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)), "Unsupported image");
                    let (w, h) = reader.into_dimensions()?;
                    let n = u64::from(w) * u64::from(h);
                    ensure!(w > 0 && h > 0 && w <= 8192 && h <= 8192 && *pixels + n <= 8_000_000, "Image dimensions exceed limit");
                    let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format()?;
                    let mut limits = image::Limits::default(); limits.max_image_width = Some(8192); limits.max_image_height = Some(8192); limits.max_alloc = Some(64 * 1024 * 1024);
                    reader.limits(limits);
                    let image = reader.decode()?;
                    let mut png = std::io::Cursor::new(Vec::new()); image.write_to(&mut png, image::ImageFormat::Png)?;
                    let temp = std::env::temp_dir().join(format!("quiet-image-{}.png", uuid::Uuid::new_v4()));
                    use std::io::Write;
                    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
                    if let Err(e) = f.write_all(&png.into_inner()) { let _ = std::fs::remove_file(&temp); return Err(e.into()); }
                    *pixels += n; Ok(temp)
                })();
                match converted {
                    Ok(path) => { if let BlockKind::Image { src, .. } = &mut block.kind { *src = path.to_string_lossy().into(); } files.push(path); }
                    Err(_) => { block.kind = BlockKind::Text; block.text = RichText::plain(format!("Image unavailable: {original}")); }
                }
            }
            if let BlockKind::Table(table) = &mut block.kind { for row in &mut table.rows { for cell in row { visit(&mut cell.blocks, files, pixels); } } }
            visit(&mut block.children, files, pixels);
        }
    }
    let mut files = vec![]; visit(&mut doc.blocks, &mut files, &mut 0); files
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_preview_pele_offline() {
        let p = Palette { background: "white".into(), text: "black".into(), muted: "gray".into(), surface: "white".into(), border: "black".into(), accent: "blue".into() };
        for source in ["flowchart LR\nA[Start]-->B[Finish]", "sequenceDiagram\nAlice->>Bob: Hello", "classDiagram\nAnimal <|-- Duck"] {
            let bytes = render(source, 700., &p).unwrap();
            let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
            assert!(image.width() > 30 && image.height() > 20);
            assert!(image.pixels().filter(|p| p.0[3] > 0 && p.0[0] < 200).count() > 50);
        }
        assert!(render("not a diagram", 700., &p).is_err());
        assert!(render(&"x".repeat(17_000), 700., &p).is_err());
    }
    #[test]
    fn workspace_preview_media_copy() {
        let dir = std::env::temp_dir().join(format!("quiet-preview-media-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap(); let dir = dir.canonicalize().unwrap();
        let path = dir.join("image.png"); image::save_buffer(&path, &[255, 0, 0, 255], 1, 1, image::ColorType::Rgba8).unwrap();
        let source = "![image](image.png)\n\n![missing](no.png)";
        let mut doc = fdl_core::workspace_preview::parse(source, &dir.join("note.md"), &dir).doc;
        let paths = prepare_images(&mut doc); assert_eq!(paths.len(), 1);
        assert_ne!(paths[0], path); assert_eq!(image::open(&paths[0]).unwrap().width(), 1);
        assert!(path.exists()); for path in paths { std::fs::remove_file(path).unwrap(); }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
