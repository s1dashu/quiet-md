//! Built-in cover gallery, emoji resolution, image validation & `.assets/` storage.

use crate::output::CliFailure;
use fdl_core::model::Cover;
use fdl_core::store::Store;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/covers.rs"));

pub const COVER_SERIES: [(&str, &str, usize); 4] = [
    ("gradient", "渐变 / Gradient", 8),
    ("aurora", "极光 / Aurora", 6),
    ("landscape", "山峦 / Landscape", 6),
    ("geometry", "几何 / Geometry", 6),
];

pub const IMAGE_MAX_BYTES: usize = 20 * 1024 * 1024;
pub const IMAGE_FORMATS: [&str; 6] = ["jpeg", "jpg", "png", "gif", "webp", "bmp"];

pub fn cover_names() -> Vec<&'static str> {
    COVERS.iter().map(|(n, _)| *n).collect()
}

pub fn cover_bytes(name: &str) -> Option<&'static [u8]> {
    let key = name.trim().strip_suffix(".jpg").unwrap_or(name.trim());
    COVERS.iter().find(|(n, _)| *n == key).map(|(_, b)| *b)
}

/// Resolve an emoji string OR an emoji-mart id (`"rocket"` -> `"🚀"`).
pub fn resolve_emoji(input: &str) -> Result<String, CliFailure> {
    let s = input.trim();
    if s.is_empty() {
        return Err(CliFailure::new("INVALID_EMOJI", "Emoji cannot be empty."));
    }
    if let Some(ch) = fdl_core::emoji::by_id(s) {
        return Ok(ch.to_string());
    }
    // Accept literal emoji / unicode icon directly if non-ASCII or short symbol.
    if s.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-') {
        return Ok(s.to_string());
    }
    Err(CliFailure::new("UNKNOWN_EMOJI_ID", format!("Unknown emoji id '{s}'."))
        .with_hint("Pass a literal emoji like '🚀' or a valid emoji-mart id such as 'rocket', 'bulb', 'memo', 'sparkles', 'warning'."))
}

/// Validate image magic bytes and extract pixel `(width, height)` for PNG/JPEG/GIF/WebP/BMP.
pub fn inspect_image(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return (w > 0 && h > 0).then_some(("png", w, h));
    }
    if (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) && bytes.len() >= 10 {
        let w = u16::from_le_bytes(bytes[6..8].try_into().ok()?) as u32;
        let h = u16::from_le_bytes(bytes[8..10].try_into().ok()?) as u32;
        return (w > 0 && h > 0).then_some(("gif", w, h));
    }
    if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        let dib = u32::from_le_bytes(bytes[14..18].try_into().ok()?);
        let (w, h) = if dib == 12 && bytes.len() >= 26 {
            (u16::from_le_bytes(bytes[18..20].try_into().ok()?) as u32, u16::from_le_bytes(bytes[20..22].try_into().ok()?) as u32)
        } else {
            let w = i32::from_le_bytes(bytes[18..22].try_into().ok()?).unsigned_abs();
            let h = i32::from_le_bytes(bytes[22..26].try_into().ok()?).unsigned_abs();
            (w, h)
        };
        return (w > 0 && h > 0).then_some(("bmp", w, h));
    }
    if bytes.len() >= 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        match &bytes[12..16] {
            b"VP8 " if bytes.len() >= 30 => {
                let w = (u16::from_le_bytes(bytes[26..28].try_into().ok()?) & 0x3fff) as u32;
                let h = (u16::from_le_bytes(bytes[28..30].try_into().ok()?) & 0x3fff) as u32;
                return (w > 0 && h > 0).then_some(("webp", w, h));
            }
            b"VP8L" if bytes.len() >= 25 => {
                let b0 = bytes[21] as u32;
                let b1 = bytes[22] as u32;
                let b2 = bytes[23] as u32;
                let b3 = bytes[24] as u32;
                let bits = b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
                let w = (bits & 0x3fff) + 1;
                let h = ((bits >> 14) & 0x3fff) + 1;
                return Some(("webp", w, h));
            }
            b"VP8X" if bytes.len() >= 30 => {
                let w = 1 + (bytes[24] as u32 | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16));
                let h = 1 + (bytes[27] as u32 | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16));
                return Some(("webp", w, h));
            }
            _ => {}
        }
    }
    if bytes.starts_with(b"\xff\xd8") {
        let mut i = 2usize;
        while i + 4 <= bytes.len() {
            if bytes[i] != 0xff { i += 1; continue; }
            while i < bytes.len() && bytes[i] == 0xff { i += 1; }
            if i >= bytes.len() { break; }
            let marker = bytes[i];
            i += 1;
            if marker == 0xd9 || marker == 0xda { break; }
            if marker == 0x00 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) { continue; }
            if i + 2 > bytes.len() { break; }
            let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
            if seg_len < 2 || i + seg_len > bytes.len() { break; }
            let is_sof = matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf);
            if is_sof && seg_len >= 7 {
                let h = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                return (w > 0 && h > 0).then_some(("jpg", w, h));
            }
            i += seg_len;
        }
    }
    None
}

pub fn store_image_bytes(
    store: &Store,
    bytes: &[u8],
    ext_hint: Option<&str>,
    name_hint: Option<&str>,
    dry_run: bool,
) -> Result<(String, u32, u32), CliFailure> {
    if bytes.len() > IMAGE_MAX_BYTES {
        return Err(CliFailure::new("IMAGE_TOO_LARGE", format!("Image exceeds 20 MB limit ({} bytes).", bytes.len())));
    }
    let (detected_ext, w, h) = inspect_image(bytes).ok_or_else(|| {
        CliFailure::new("UNSUPPORTED_IMAGE", "Invalid or unsupported image file.")
            .with_valid(IMAGE_FORMATS)
            .with_hint("Provide a valid JPEG, PNG, GIF, WebP or BMP image <= 20 MB.")
    })?;
    let ext = ext_hint
        .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| IMAGE_FORMATS.contains(&e.as_str()))
        .unwrap_or_else(|| detected_ext.to_string());
    let id = uuid::Uuid::new_v4().to_string();
    let clean_name: Option<String> = name_hint
        .map(|n| n.chars().map(|c| if matches!(c, '/' | '\\' | ':') || c.is_control() { '_' } else { c }).take(80).collect::<String>())
        .map(|n| n.trim().trim_start_matches('.').to_string())
        .filter(|n| !n.is_empty());
    let file_name = match clean_name {
        Some(n) => format!("{}-{n}.{ext}", &id[..8]),
        None => format!("{id}.{ext}"),
    };
    if dry_run {
        return Ok((format!(".assets/.dry-run-{file_name}"), w, h));
    }
    let rel = store.write_asset_bytes(None, &file_name, bytes)?;
    Ok((rel, w, h))
}

pub fn store_image_file(store: &Store, path: &Path, dry_run: bool) -> Result<(String, u32, u32), CliFailure> {
    let meta = std::fs::metadata(path).map_err(|e| CliFailure::new("ASSET_NOT_FOUND", format!("Cannot read image {}: {e}", path.display())))?;
    if !meta.is_file() {
        return Err(CliFailure::new("ASSET_NOT_FOUND", format!("Not a regular file: {}", path.display())));
    }
    if meta.len() > IMAGE_MAX_BYTES as u64 {
        return Err(CliFailure::new("IMAGE_TOO_LARGE", format!("Image exceeds 20 MB limit: {}", path.display())));
    }
    let bytes = std::fs::read(path).map_err(|e| CliFailure::new("ASSET_READ_FAILED", format!("Failed reading {}: {e}", path.display())))?;
    let ext = path.extension().and_then(|s| s.to_str());
    let stem = path.file_stem().and_then(|s| s.to_str());
    store_image_bytes(store, &bytes, ext, stem, dry_run)
}

pub fn store_attachment_file(store: &Store, path: &Path, dry_run: bool) -> Result<(String, String), CliFailure> {
    let name = path.file_name().and_then(|s| s.to_str()).map(str::to_string)
        .ok_or_else(|| CliFailure::new("INVALID_PATH", format!("Invalid file name: {}", path.display())))?;
    let meta = std::fs::metadata(path).map_err(|e| CliFailure::new("ASSET_NOT_FOUND", format!("Cannot read file {}: {e}", path.display())))?;
    if !meta.is_file() {
        return Err(CliFailure::new("ASSET_NOT_FOUND", format!("Not a regular file: {}", path.display())));
    }
    let sub = format!("files/{}", uuid::Uuid::new_v4());
    if dry_run {
        return Ok((format!(".assets/.dry-run-{sub}/{name}"), name));
    }
    let bytes = std::fs::read(path).map_err(|e| CliFailure::new("ASSET_READ_FAILED", format!("Failed reading {}: {e}", path.display())))?;
    let rel = store.write_asset_bytes(Some(&sub), &name, &bytes)?;
    Ok((rel, name))
}

pub fn resolve_cover(
    store: &Store,
    gallery: Option<&str>,
    file: Option<&Path>,
    x: f32,
    y: f32,
    dry_run: bool,
) -> Result<Cover, CliFailure> {
    let cx = x.clamp(-1., 1.);
    let cy = y.clamp(-1., 1.);
    match (gallery, file) {
        (Some(name), None) => {
            let bytes = cover_bytes(name).ok_or_else(|| {
                CliFailure::new("UNKNOWN_COVER", format!("Unknown built-in cover '{name}'."))
                    .with_valid(cover_names())
                    .with_hint("Choose one of the 26 built-in gallery covers (e.g. 'aurora-2', 'gradient-5', 'landscape-1', 'geometry-3') or pass --file <path>.")
            })?;
            let key = name.trim().strip_suffix(".jpg").unwrap_or(name.trim());
            let (rel, _, _) = store_image_bytes(store, bytes, Some("jpg"), Some(key), dry_run)?;
            Ok(Cover { src: rel, x: cx, y: cy })
        }
        (None, Some(path)) => {
            let (rel, _, _) = store_image_file(store, path, dry_run)?;
            Ok(Cover { src: rel, x: cx, y: cy })
        }
        (Some(_), Some(_)) => Err(CliFailure::usage("Specify either gallery cover or local file, not both.")),
        (None, None) => Err(CliFailure::usage("Cover requires either gallery name or local file path.")),
    }
}
