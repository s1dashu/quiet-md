//! `qmd export` — headless HTML, Markdown, DocxXML, preview bundle & harness script generation.

use super::{load_doc, write_file_bytes};
use crate::args::ParsedCmd;
use crate::board_builder;
use crate::locator;
use crate::output::{CliFailure, GlobalOpts, print_success};
use fdl_core::docxml;
use fdl_core::html::{self, HeadlessStyle};
use fdl_core::markdown as md_export;
use fdl_core::model::{BlockKind, Document};
use fdl_core::store::{Entry, Store};
use fdl_core::theme_css;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_text_file(path: &Path, content: &str, dry_run: bool) -> Result<(), CliFailure> {
    write_file_bytes(path, content.as_bytes(), dry_run)
}

fn sub_page_titles(store: &mut Store, doc_id: &str) -> Vec<String> {
    store.list().into_iter()
        .filter(|e| e.meta.folder.is_none() && e.meta.parent.as_deref() == Some(doc_id))
        .map(|e| e.title)
        .collect()
}

fn render_doc_html(store: &mut Store, entry: &Entry, doc: &Document, theme_override: Option<&str>) -> Result<String, CliFailure> {
    let root = store.root().to_path_buf();
    let mut style = HeadlessStyle::for_library(root.clone(), entry.meta.width.max());
    if let Some(tid) = theme_override {
        let parsed = theme_css::load_theme_for_library(&root, tid)
            .map_err(|e| CliFailure::new("THEME_NOT_FOUND", e))?;
        style.theme = Some(parsed);
    }
    let subs = sub_page_titles(store, &entry.id);
    Ok(html::render(doc, &html::Options { sub_pages: &subs }, &style))
}

fn find_chromium() -> Option<PathBuf> {
    for bin in ["google-chrome", "chromium", "chromium-browser", "microsoft-edge"] {
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                let cand = dir.join(bin);
                if cand.is_file() { return Some(cand); }
            }
        }
    }
    for mac in [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ] {
        let p = PathBuf::from(mac);
        if p.is_file() { return Some(p); }
    }
    None
}

pub fn run(cmd: &ParsedCmd, opts: &GlobalOpts) -> Result<(), CliFailure> {
    let mut store = super::open_store(opts)?;
    let root = store.root().to_path_buf();
    let (entry, doc) = load_doc(&mut store, cmd.pos(0, "<doc>")?)?;

    match cmd.command_path.as_str() {
        "export html" => {
            let html_text = render_doc_html(&mut store, &entry, &doc, cmd.flag("theme"))?;
            let out = cmd.flag("out").map(Path::new);
            if let Some(p) = out {
                write_text_file(p, &html_text, opts.dry_run)?;
            }
            let human = match out {
                Some(p) => format!("Exported HTML ({} bytes) -> {}", html_text.len(), p.display()),
                None => html_text.clone(),
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "id": entry.id,
                    "bytes": html_text.len(),
                    "out": out.map(|p| p.display().to_string()),
                    "html": if out.is_none() { Some(html_text) } else { None },
                }),
                &store.warnings, &[], &human,
            );
            Ok(())
        }
        "export markdown" => {
            let subs = sub_page_titles(&mut store, &entry.id);
            let assets_dir = cmd.flag("assets-dir");
            let out = cmd.flag("out").map(Path::new);
            let export = md_export::export(&doc, &md_export::Options {
                library_root: Some(&root),
                sub_pages: &subs,
                assets: assets_dir,
                ..Default::default()
            });
            if let Some(p) = out {
                write_text_file(p, &export.text, opts.dry_run)?;
                if !opts.dry_run {
                    let base_dir = p.parent().filter(|x| !x.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
                    for (src, rel) in &export.files {
                        let dst = base_dir.join(rel);
                        if let Some(dp) = dst.parent() { let _ = std::fs::create_dir_all(dp); }
                        let _ = std::fs::copy(src, dst);
                    }
                    for (rel, body) in &export.generated {
                        let _ = write_text_file(&base_dir.join(rel), body, false);
                    }
                }
            }
            let human = match out {
                Some(p) => format!("Exported Markdown ({} bytes, {} missing assets) -> {}", export.text.len(), export.missing, p.display()),
                None => export.text.clone(),
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "id": entry.id,
                    "bytes": export.text.len(),
                    "missing_assets": export.missing,
                    "copied_files": export.files.len(),
                    "generated_files": export.generated.len(),
                    "out": out.map(|p| p.display().to_string()),
                    "markdown": if out.is_none() { Some(export.text) } else { None },
                }),
                &store.warnings, &[], &human,
            );
            Ok(())
        }
        "export docxml" => {
            let xml = docxml::serialize(&doc);
            let out = cmd.flag("out").map(Path::new);
            if let Some(p) = out {
                write_text_file(p, &xml, opts.dry_run)?;
            }
            let human = match out {
                Some(p) => format!("Exported DocxXML ({} bytes) -> {}", xml.len(), p.display()),
                None => xml.clone(),
            };
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "bytes": xml.len(), "out": out.map(|p| p.display().to_string()), "xml": if out.is_none() { Some(xml) } else { None } }),
                &store.warnings, &[], &human,
            );
            Ok(())
        }
        "export preview" => {
            let out_dir = Path::new(cmd.req_flag("out-dir")?);
            let vw: u32 = cmd.flag("width").unwrap_or("1280").parse().map_err(|_| CliFailure::usage("Invalid --width"))?;
            let vh: u32 = cmd.flag("height").unwrap_or("960").parse().map_err(|_| CliFailure::usage("Invalid --height"))?;
            let html_text = render_doc_html(&mut store, &entry, &doc, cmd.flag("theme"))?;
            let html_path = out_dir.join("index.html");
            write_text_file(&html_path, &html_text, opts.dry_run)?;

            let mut board_outputs = vec![];
            for item in locator::index_blocks(&doc) {
                if let Some(b) = locator::find_block(&doc, item.id) && let BlockKind::Board(bd) = &b.kind {
                    let svg = bd.svg_in(&root);
                    let safe_loc = item.loc.replace(['.', '[', ']', ','], "_");
                    let svg_path = out_dir.join("boards").join(format!("{safe_loc}.svg"));
                    let png_path = out_dir.join("boards").join(format!("{safe_loc}.png"));
                    write_text_file(&svg_path, &svg, opts.dry_run)?;
                    let png = board_builder::render_svg_to_png(&svg, 2.0)?;
                    if !opts.dry_run {
                        std::fs::write(&png_path, &png).map_err(|e| CliFailure::new("WRITE_FAILED", format!("{e}")))?;
                    }
                    board_outputs.push(serde_json::json!({
                        "loc": item.loc,
                        "svg": svg_path.display().to_string(),
                        "png": png_path.display().to_string(),
                        "png_bytes": png.len(),
                    }));
                }
            }

            let mut page_png: Option<String> = None;
            let mut browser_used: Option<String> = None;
            if !opts.dry_run && let Some(chrome) = find_chromium() {
                let abs_html = std::fs::canonicalize(&html_path).unwrap_or_else(|_| html_path.clone());
                let shot_path = out_dir.join("page.png");
                let status = Command::new(&chrome)
                    .arg("--headless")
                    .arg("--disable-gpu")
                    .arg("--no-sandbox")
                    .arg(format!("--window-size={vw},{vh}"))
                    .arg(format!("--screenshot={}", shot_path.display()))
                    .arg(format!("file://{}", abs_html.display()))
                    .output();
                if let Ok(out) = status && out.status.success() && shot_path.is_file() {
                    page_png = Some(shot_path.display().to_string());
                    browser_used = Some(chrome.display().to_string());
                }
            }

            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({
                    "id": entry.id,
                    "html": html_path.display().to_string(),
                    "boards": board_outputs,
                    "page_png": page_png,
                    "browser": browser_used,
                }),
                &store.warnings, &[],
                &format!("Preview rendered -> {} ({} board PNGs, page_png={})", html_path.display(), board_outputs.len(), page_png.as_deref().unwrap_or("none (install chromium for full-page PNG)")),
            );
            Ok(())
        }
        "export harness-script" => {
            let out = Path::new(cmd.req_flag("out")?);
            let shot = cmd.flag("shot").unwrap_or("doc");
            let script = format!(
                "# tools/harness FDL_SCRIPT for '{}' ({})\n# Run on macOS after `cargo build --release --features devtools`:\n#   python3 tools/harness/run.py {} {}\nmove 20 20\nwait 1500\nshot {shot}\nwheel 820 470 500\nwait 400\nshot {shot}-scroll\nquit\n",
                entry.title, entry.meta.rel, out.display(), root.display()
            );
            write_text_file(out, &script, opts.dry_run)?;
            print_success(
                opts, &cmd.command_path, Some(&root),
                serde_json::json!({ "id": entry.id, "out": out.display().to_string(), "shot": shot }),
                &store.warnings, &[],
                &format!("Generated FDL_SCRIPT -> {}", out.display()),
            );
            Ok(())
        }
        _ => Err(CliFailure::usage(format!("Unhandled export command {}", cmd.command_path))),
    }
}
