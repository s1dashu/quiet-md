use crate::{
    Bounds, DevicePixels, Font, FontFallbacks, FontFeatures, FontId, FontMetrics, FontRun,
    FontStyle, FontWeight, GlyphId, LineLayout, Pixels, PlatformTextSystem, Point,
    RenderGlyphParams, SUBPIXEL_VARIANTS_X, SUBPIXEL_VARIANTS_Y, ShapedGlyph, ShapedRun,
    SharedString, Size, point, size,
};
use anyhow::{Context as _, Ok, Result};
use collections::HashMap;
use cosmic_text::{
    Attrs, AttrsList, CacheKey, Family, Font as CosmicTextFont,
    FontFeatures as CosmicFontFeatures, FontSystem, ShapeBuffer, ShapeLine, SwashCache,
};

use itertools::Itertools;
use parking_lot::RwLock;
use pathfinder_geometry::{
    rect::{RectF, RectI},
    vector::{Vector2F, Vector2I},
};
use smallvec::SmallVec;
use std::{
    borrow::Cow,
    ops::Range,
    process::{Command, Stdio},
    sync::{Arc, OnceLock},
};

pub(crate) struct CosmicTextSystem(RwLock<CosmicTextSystemState>);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FontKey {
    family: SharedString,
    features: FontFeatures,
    fallbacks: Option<FontFallbacks>,
}

impl FontKey {
    fn new(family: SharedString, features: FontFeatures, fallbacks: Option<FontFallbacks>) -> Self {
        Self {
            family,
            features,
            fallbacks,
        }
    }
}

// quiet.md: resolve system sans/mono/CJK/emoji families from Linux fontconfig
// while picking static-weight faces for UI sans (cosmic-text + swash do not
// instantiate variable font weight axes).
#[derive(Clone, Copy)]
struct LinuxFonts {
    ui_sans: &'static str,
    ui_serif: &'static str,
    ui_mono: &'static str,
    cjk_sans: &'static str,
    cjk_mono: &'static str,
    sans_fallbacks: &'static [&'static str],
    mono_fallbacks: &'static [&'static str],
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn fc_families(pattern: &str) -> Vec<String> {
    let std::result::Result::Ok(out) = Command::new("fc-match")
        .args(["-s", "-f", "%{family[0]}\n", pattern])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let mut list = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let fam = line.trim();
        if !fam.is_empty() && !list.iter().any(|x: &String| x == fam) {
            list.push(fam.to_string());
        }
    }
    list
}

fn db_has_family(db: &cosmic_text::fontdb::Database, name: &str) -> bool {
    db.faces()
        .any(|f| f.families.iter().any(|(fam, _)| fam.eq_ignore_ascii_case(name)))
}

fn db_has_static_bold(db: &cosmic_text::fontdb::Database, name: &str) -> bool {
    db.faces().any(|f| {
        f.families.iter().any(|(fam, _)| fam.eq_ignore_ascii_case(name)) && f.weight.0 >= 600
    })
}

fn db_supports_char(db: &cosmic_text::fontdb::Database, name: &str, ch: char) -> bool {
    db.faces()
        .filter(|f| f.families.iter().any(|(fam, _)| fam.eq_ignore_ascii_case(name)))
        .any(|f| {
            db.with_face_data(f.id, |data, ix| {
                swash::FontRef::from_index(data, ix as usize).is_some_and(|sf| sf.charmap().map(ch) != 0)
            })
            .unwrap_or(false)
        })
}

impl LinuxFonts {
    fn detect(db: &cosmic_text::fontdb::Database) -> Self {
        static CACHE: OnceLock<LinuxFonts> = OnceLock::new();
        *CACHE.get_or_init(|| Self::compute(db))
    }

    fn compute(db: &cosmic_text::fontdb::Database) -> Self {
        let mut sans_list = fc_families("sans-serif");
        for d in ["Noto Sans", "Inter", "Ubuntu", "Adwaita Sans", "Cantarell", "DejaVu Sans", "Liberation Sans"] {
            if !sans_list.iter().any(|x| x == d) {
                sans_list.push(d.to_string());
            }
        }
        let ui_sans = sans_list
            .iter()
            .find(|c| {
                db_has_family(db, c)
                    && db_has_static_bold(db, c)
                    && db_supports_char(db, c, 'm')
                    && !db_supports_char(db, c, '中')
            })
            .or_else(|| {
                sans_list.iter().find(|c| {
                    db_has_family(db, c)
                        && db_supports_char(db, c, 'm')
                        && !db_supports_char(db, c, '中')
                })
            })
            .map(|s| leak_str(s.clone()))
            .unwrap_or("Noto Sans");

        let mut serif_list = fc_families("serif");
        for d in ["Noto Serif", "Liberation Serif", "DejaVu Serif"] {
            if !serif_list.iter().any(|x| x == d) {
                serif_list.push(d.to_string());
            }
        }
        let ui_serif = serif_list
            .iter()
            .find(|c| db_has_family(db, c) && db_supports_char(db, c, 'm') && !db_supports_char(db, c, '中'))
            .map(|s| leak_str(s.clone()))
            .unwrap_or("Noto Serif");

        let mut mono_list = fc_families("monospace");
        for d in [
            "JetBrainsMono Nerd Font",
            "JetBrains Mono",
            "Noto Sans Mono",
            "DejaVu Sans Mono",
            "Liberation Mono",
            "Adwaita Mono",
        ] {
            if !mono_list.iter().any(|x| x == d) {
                mono_list.push(d.to_string());
            }
        }
        let ui_mono = mono_list
            .iter()
            .find(|c| db_has_family(db, c) && db_supports_char(db, c, 'm'))
            .map(|s| leak_str(s.clone()))
            .unwrap_or("JetBrainsMono Nerd Font");

        let mut han_candidates = fc_families("sans-serif:lang=zh-cn");
        for d in ["LXGW WenKai", "Noto Sans CJK SC", "WenQuanYi Micro Hei"] {
            if !han_candidates.iter().any(|x| x == d) {
                han_candidates.push(d.to_string());
            }
        }
        let mut sans_fb_vec: Vec<&'static str> = Vec::new();
        for c in &han_candidates {
            if db_has_family(db, c)
                && db_supports_char(db, c, '中')
                && !c.contains("Mono")
                && !sans_fb_vec.contains(&c.as_str())
            {
                sans_fb_vec.push(leak_str(c.clone()));
            }
        }
        let cjk_sans = sans_fb_vec.first().copied().unwrap_or("Noto Sans CJK SC");

        let mut mono_zh_candidates = fc_families("monospace:lang=zh-cn");
        for d in ["LXGW WenKai Mono", "Noto Sans Mono CJK SC"] {
            if !mono_zh_candidates.iter().any(|x| x == d) {
                mono_zh_candidates.push(d.to_string());
            }
        }
        let mut mono_fb_vec: Vec<&'static str> = Vec::new();
        for c in &mono_zh_candidates {
            if db_has_family(db, c)
                && db_supports_char(db, c, '中')
                && !mono_fb_vec.contains(&c.as_str())
            {
                mono_fb_vec.push(leak_str(c.clone()));
            }
        }
        for c in &sans_fb_vec {
            if !mono_fb_vec.contains(c) {
                mono_fb_vec.push(c);
            }
        }
        let cjk_mono = mono_fb_vec.first().copied().unwrap_or(cjk_sans);

        let mut emoji_list = fc_families("emoji");
        for d in ["Noto Color Emoji", "Twemoji", "EmojiOne"] {
            if !emoji_list.iter().any(|x| x == d) {
                emoji_list.push(d.to_string());
            }
        }
        for e in emoji_list {
            if db_has_family(db, &e) {
                let leaked = leak_str(e);
                if !sans_fb_vec.contains(&leaked) {
                    sans_fb_vec.push(leaked);
                }
                if !mono_fb_vec.contains(&leaked) {
                    mono_fb_vec.push(leaked);
                }
                break;
            }
        }

        Self {
            ui_sans,
            ui_serif,
            ui_mono,
            cjk_sans,
            cjk_mono,
            sans_fallbacks: Box::leak(sans_fb_vec.into_boxed_slice()),
            mono_fallbacks: Box::leak(mono_fb_vec.into_boxed_slice()),
        }
    }
}

struct CosmicTextSystemState {
    swash_cache: SwashCache,
    font_system: FontSystem,
    linux_fonts: LinuxFonts,
    scratch: ShapeBuffer,
    /// Contains all already loaded fonts, including all faces. Indexed by `FontId`.
    loaded_fonts: Vec<LoadedFont>,
    /// Caches the `FontId`s associated with a specific family to avoid iterating the font database
    /// for every font face in a family.
    font_ids_by_family_cache: HashMap<FontKey, SmallVec<[FontId; 4]>>,
}

struct LoadedFont {
    font: Arc<CosmicTextFont>,
    features: CosmicFontFeatures,
    fallbacks: Option<FontFallbacks>,
    is_known_emoji_font: bool,
}

impl CosmicTextSystem {
    pub(crate) fn new() -> Self {
        // todo(linux) make font loading non-blocking
        let mut font_system = FontSystem::new();
        let linux_fonts = LinuxFonts::detect(font_system.db());
        font_system.db_mut().set_sans_serif_family(linux_fonts.ui_sans);
        font_system.db_mut().set_serif_family(linux_fonts.ui_serif);
        font_system.db_mut().set_monospace_family(linux_fonts.ui_mono);

        Self(RwLock::new(CosmicTextSystemState {
            font_system,
            linux_fonts,
            swash_cache: SwashCache::new(),
            scratch: ShapeBuffer::default(),
            loaded_fonts: Vec::new(),
            font_ids_by_family_cache: HashMap::default(),
        }))
    }
}

impl Default for CosmicTextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformTextSystem for CosmicTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.0.write().add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        let mut result = self
            .0
            .read()
            .font_system
            .db()
            .faces()
            .filter_map(|face| face.families.first().map(|family| family.0.clone()))
            .collect_vec();
        result.sort();
        result.dedup();
        result
    }

    fn font_id(&self, font: &Font) -> Result<FontId> {
        // todo(linux): Do we need to use CosmicText's Font APIs? Can we consolidate this to use font_kit?
        let mut state = self.0.write();
        let key = FontKey::new(font.family.clone(), font.features.clone(), font.fallbacks.clone());
        let candidates = if let Some(font_ids) = state.font_ids_by_family_cache.get(&key) {
            font_ids.as_slice()
        } else {
            let font_ids = state.load_family(&font.family, &font.features, font.fallbacks.as_ref())?;
            state.font_ids_by_family_cache.insert(key.clone(), font_ids);
            state.font_ids_by_family_cache[&key].as_ref()
        };

        // todo(linux) ideally we would make fontdb's `find_best_match` pub instead of using font-kit here
        let candidate_properties = candidates
            .iter()
            .map(|font_id| {
                let database_id = state.loaded_font(*font_id).font.id();
                let face_info = state.font_system.db().face(database_id).expect("");
                face_info_into_properties(face_info)
            })
            .collect::<SmallVec<[_; 4]>>();

        let ix =
            font_kit::matching::find_best_match(&candidate_properties, &font_into_properties(font))
                .context("requested font family contains no font matching the other parameters")?;

        Ok(candidates[ix])
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        let metrics = self
            .0
            .read()
            .loaded_font(font_id)
            .font
            .as_swash()
            .metrics(&[]);

        FontMetrics {
            units_per_em: metrics.units_per_em as u32,
            ascent: metrics.ascent,
            descent: -metrics.descent, // todo(linux) confirm this is correct
            line_gap: metrics.leading,
            underline_position: metrics.underline_offset,
            underline_thickness: metrics.stroke_size,
            cap_height: metrics.cap_height,
            x_height: metrics.x_height,
            // todo(linux): Compute this correctly
            bounding_box: Bounds {
                origin: point(0.0, 0.0),
                size: size(metrics.max_width, metrics.ascent + metrics.descent),
            },
        }
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        let lock = self.0.read();
        let glyph_metrics = lock.loaded_font(font_id).font.as_swash().glyph_metrics(&[]);
        let glyph_id = glyph_id.0 as u16;
        // todo(linux): Compute this correctly
        // see https://github.com/servo/font-kit/blob/master/src/loaders/freetype.rs#L614-L620
        Ok(Bounds {
            origin: point(0.0, 0.0),
            size: size(
                glyph_metrics.advance_width(glyph_id),
                glyph_metrics.advance_height(glyph_id),
            ),
        })
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.0.read().advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.0.read().glyph_for_char(font_id, ch)
    }

    // quiet.md: swash's unscaled outline (font units, y up); like the
    // rasterizer here, no synthetic oblique or bold.
    fn glyph_outline(&self, font_id: FontId, glyph_id: GlyphId) -> Option<Vec<crate::GlyphOutlineCmd>> {
        use crate::GlyphOutlineCmd as C;
        use swash::zeno::{Command, PathData};
        let state = self.0.read();
        let font = &state.loaded_font(font_id).font;
        let mut ctx = swash::scale::ScaleContext::new();
        let outline = ctx.builder(font.as_swash()).build().scale_outline(glyph_id.0 as u16)?;
        let p = |v: swash::zeno::Vector| [v.x, v.y];
        Some(outline.path().commands().map(|c| match c {
            Command::MoveTo(a) => C::MoveTo(p(a)),
            Command::LineTo(a) => C::LineTo(p(a)),
            Command::QuadTo(a, b) => C::QuadTo(p(a), p(b)),
            Command::CurveTo(a, b, c) => C::CubicTo(p(a), p(b), p(c)),
            Command::Close => C::Close,
        }).collect())
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.0.write().raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.0.write().rasterize_glyph(params, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> LineLayout {
        self.0.write().layout_line(text, font_size, runs)
    }
}

impl CosmicTextSystemState {
    fn loaded_font(&self, font_id: FontId) -> &LoadedFont {
        &self.loaded_fonts[font_id.0]
    }

    #[profiling::function]
    fn add_fonts(&mut self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        let db = self.font_system.db_mut();
        for bytes in fonts {
            // quiet.md: a bundled face replaces the system's faces of the same
            // family and style (Fedora's COLRv1 Noto Color Emoji, which swash
            // draws blank, gives way to the bundled CBDT one).
            let before: std::collections::HashSet<_> = db.faces().map(|f| f.id).collect();
            db.load_font_data(bytes.into_owned());
            let key = |f: &cosmic_text::fontdb::FaceInfo| (f.families.first().map(|x| x.0.clone()), f.style, f.weight, f.stretch);
            let added: Vec<_> = db.faces().filter(|f| !before.contains(&f.id)).map(key).collect();
            let stale: Vec<_> = db.faces().filter(|f| before.contains(&f.id) && added.contains(&key(f))).map(|f| f.id).collect();
            for id in stale {
                db.remove_face(id);
            }
        }
        self.font_ids_by_family_cache.clear();
        Ok(())
    }

    #[profiling::function]
    fn load_family(
        &mut self,
        name: &str,
        features: &FontFeatures,
        fallbacks: Option<&FontFallbacks>,
    ) -> Result<SmallVec<[FontId; 4]>> {
        // quiet.md: resolve generic system/CJK aliases from Linux fontconfig.
        let name = match name {
            ".SystemUIFont" => self.linux_fonts.ui_sans,
            ".SystemUIFontMonospaced" | ".AppleSystemUIFontMonospaced" => self.linux_fonts.ui_mono,
            ".SystemSerifFont" => self.linux_fonts.ui_serif,
            ".SystemCJKFont" => self.linux_fonts.cjk_sans,
            ".SystemCJKMonoFont" => self.linux_fonts.cjk_mono,
            _ => crate::text_system::font_name_with_fallbacks(name, self.linux_fonts.ui_sans),
        };

        let families = self
            .font_system
            .db()
            .faces()
            .filter(|face| face.families.iter().any(|family| family.0.eq_ignore_ascii_case(name)))
            .map(|face| (face.id, face.post_script_name.clone()))
            .collect::<SmallVec<[_; 4]>>();

        let mut loaded_font_ids = SmallVec::new();
        for (font_id, postscript_name) in families {
            let font = self
                .font_system
                .get_font(font_id)
                .context("Could not load font")?;

            // HACK: To let the storybook run and render Windows caption icons. We should actually do better font fallback.
            let allowed_bad_font_names = [
                "SegoeFluentIcons", // NOTE: Segoe fluent icons postscript name is inconsistent
                "Segoe Fluent Icons",
                "XiaolaiSC", // quiet.md: the board's CJK-only font
            ];
            let is_emoji = check_is_known_emoji_font(&postscript_name);

            if font.as_swash().charmap().map('m') == 0
                && !is_emoji
                && !postscript_name.starts_with("KaTeX_")
                && !allowed_bad_font_names.contains(&postscript_name.as_str())
            {
                self.font_system.db_mut().remove_face(font.id());
                continue;
            };

            let font_id = FontId(self.loaded_fonts.len());
            loaded_font_ids.push(font_id);
            self.loaded_fonts.push(LoadedFont {
                font,
                features: features.try_into()?,
                fallbacks: fallbacks.cloned(),
                is_known_emoji_font: is_emoji,
            });
        }

        Ok(loaded_font_ids)
    }

    fn resolve_fallback_font_id(
        &mut self,
        family: &str,
        props: &font_kit::properties::Properties,
    ) -> Option<FontId> {
        let features = FontFeatures::default();
        let key = FontKey::new(SharedString::from(family.to_string()), features.clone(), None);
        if !self.font_ids_by_family_cache.contains_key(&key) {
            let font_ids = self.load_family(family, &features, None).ok()?;
            self.font_ids_by_family_cache.insert(key.clone(), font_ids);
        }
        let candidates = self.font_ids_by_family_cache.get(&key)?.as_slice();
        if candidates.is_empty() {
            return None;
        }
        let candidate_properties = candidates
            .iter()
            .map(|font_id| {
                let database_id = self.loaded_font(*font_id).font.id();
                let face_info = self.font_system.db().face(database_id).expect("");
                face_info_into_properties(face_info)
            })
            .collect::<SmallVec<[_; 4]>>();
        let ix = font_kit::matching::find_best_match(&candidate_properties, props).ok()?;
        Some(candidates[ix])
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        let glyph_metrics = self.loaded_font(font_id).font.as_swash().glyph_metrics(&[]);
        Ok(Size {
            width: glyph_metrics.advance_width(glyph_id.0 as u16),
            height: glyph_metrics.advance_height(glyph_id.0 as u16),
        })
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        let glyph_id = self.loaded_font(font_id).font.as_swash().charmap().map(ch);
        if glyph_id == 0 {
            None
        } else {
            Some(GlyphId(glyph_id.into()))
        }
    }

    fn raster_bounds(&mut self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let font = &self.loaded_fonts[params.font_id.0].font;
        let subpixel_shift = point(
            params.subpixel_variant.x as f32 / SUBPIXEL_VARIANTS_X as f32 / params.scale_factor,
            params.subpixel_variant.y as f32 / SUBPIXEL_VARIANTS_Y as f32 / params.scale_factor,
        );
        let image = self
            .swash_cache
            .get_image(
                &mut self.font_system,
                CacheKey::new(
                    font.id(),
                    params.glyph_id.0 as u16,
                    (params.font_size * params.scale_factor).into(),
                    (subpixel_shift.x, subpixel_shift.y.trunc()),
                    cosmic_text::CacheKeyFlags::empty(),
                )
                .0,
            )
            .clone()
            .with_context(|| format!("no image for {params:?} in font {font:?}"))?;
        Ok(Bounds {
            origin: point(image.placement.left.into(), (-image.placement.top).into()),
            size: size(image.placement.width.into(), image.placement.height.into()),
        })
    }

    #[profiling::function]
    fn rasterize_glyph(
        &mut self,
        params: &RenderGlyphParams,
        glyph_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        if glyph_bounds.size.width.0 == 0 || glyph_bounds.size.height.0 == 0 {
            anyhow::bail!("glyph bounds are empty");
        } else {
            let bitmap_size = glyph_bounds.size;
            let font = &self.loaded_fonts[params.font_id.0].font;
            let subpixel_shift = point(
                params.subpixel_variant.x as f32 / SUBPIXEL_VARIANTS_X as f32 / params.scale_factor,
                params.subpixel_variant.y as f32 / SUBPIXEL_VARIANTS_Y as f32 / params.scale_factor,
            );
            let mut image = self
                .swash_cache
                .get_image(
                    &mut self.font_system,
                    CacheKey::new(
                        font.id(),
                        params.glyph_id.0 as u16,
                        (params.font_size * params.scale_factor).into(),
                        (subpixel_shift.x, subpixel_shift.y.trunc()),
                        cosmic_text::CacheKeyFlags::empty(),
                    )
                    .0,
                )
                .clone()
                .with_context(|| format!("no image for {params:?} in font {font:?}"))?;

            if params.is_emoji {
                // Convert from RGBA to BGRA.
                for pixel in image.data.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
            }

            Ok((bitmap_size, image.data))
        }
    }

    /// This is used when cosmic_text has chosen a fallback font instead of using the requested
    /// font, typically to handle some unicode characters. When this happens, `loaded_fonts` may not
    /// yet have an entry for this fallback font, and so one is added.
    ///
    /// Note that callers shouldn't use this `FontId` somewhere that will retrieve the corresponding
    /// `LoadedFont.features`, as it will have an arbitrarily chosen or empty value. The only
    /// current use of this field is for the *input* of `layout_line`, and so it's fine to use
    /// `font_id_for_cosmic_id` when computing the *output* of `layout_line`.
    fn font_id_for_cosmic_id(&mut self, id: cosmic_text::fontdb::ID) -> FontId {
        if let Some(ix) = self
            .loaded_fonts
            .iter()
            .position(|loaded_font| loaded_font.font.id() == id)
        {
            FontId(ix)
        } else {
            let font = self.font_system.get_font(id).unwrap();
            let face = self.font_system.db().face(id).unwrap();

            let font_id = FontId(self.loaded_fonts.len());
            self.loaded_fonts.push(LoadedFont {
                font,
                features: CosmicFontFeatures::new(),
                fallbacks: None,
                is_known_emoji_font: check_is_known_emoji_font(&face.post_script_name),
            });

            font_id
        }
    }

    #[profiling::function]
    fn layout_line(&mut self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        // quiet.md: honor per-Font `fallbacks` on Linux so mono/sans runs pick
        // matching CJK and emoji faces before cosmic_text's global fallback.
        let mut span_runs: SmallVec<[(Range<usize>, FontId); 8]> = SmallVec::new();
        let mut offs = 0;
        for run in font_runs {
            let range = offs..(offs + run.len);
            offs += run.len;
            let (primary_font, fallbacks) = {
                let loaded = self.loaded_font(run.font_id);
                (loaded.font.clone(), loaded.fallbacks.clone())
            };
            let slice = &text[range.clone()];
            let has_missing = slice
                .chars()
                .any(|ch| !ch.is_control() && primary_font.as_swash().charmap().map(ch) == 0);
            if !has_missing {
                span_runs.push((range, run.font_id));
                continue;
            }
            let (props, is_mono) = {
                let face = self.font_system.db().face(primary_font.id()).unwrap();
                (face_info_into_properties(face), face.monospaced)
            };
            let default_fb = || -> &'static [&'static str] {
                let loc = self.font_system.locale();
                if loc.starts_with("ja") || loc.starts_with("ko") || loc.starts_with("zh-TW") || loc.starts_with("zh-HK") {
                    &[]
                } else if is_mono {
                    self.linux_fonts.mono_fallbacks
                } else {
                    self.linux_fonts.sans_fallbacks
                }
            };
            let mut fb_ids: SmallVec<[FontId; 4]> = SmallVec::new();
            match fallbacks.as_ref().filter(|fb| !fb.fallback_list().is_empty()) {
                Some(fb) => {
                    for fam in fb.fallback_list() {
                        if let Some(fid) = self.resolve_fallback_font_id(fam, &props) && !fb_ids.contains(&fid) {
                            fb_ids.push(fid);
                        }
                    }
                }
                None => {
                    for fam in default_fb() {
                        if let Some(fid) = self.resolve_fallback_font_id(fam, &props) && !fb_ids.contains(&fid) {
                            fb_ids.push(fid);
                        }
                    }
                }
            }
            if fb_ids.is_empty() {
                span_runs.push((range, run.font_id));
                continue;
            }
            let mut cur_start = range.start;
            let mut cur_fid: Option<FontId> = None;
            for (i, ch) in slice.char_indices() {
                let attaching = matches!(
                    ch as u32,
                    0x300..=0x36F | 0x200C | 0x200D | 0x20D0..=0x20FF | 0xFE00..=0xFE0F | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F | 0xE0100..=0xE01EF
                );
                let fid = if attaching {
                    cur_fid.unwrap_or(run.font_id)
                } else if ch.is_control() || primary_font.as_swash().charmap().map(ch) != 0 {
                    run.font_id
                } else {
                    fb_ids
                        .iter()
                        .copied()
                        .find(|&fid| self.loaded_fonts[fid.0].font.as_swash().charmap().map(ch) != 0)
                        .unwrap_or(run.font_id)
                };
                if let Some(prev) = cur_fid {
                    if prev != fid {
                        span_runs.push((cur_start..(range.start + i), prev));
                        cur_start = range.start + i;
                        cur_fid = Some(fid);
                    }
                } else {
                    cur_fid = Some(fid);
                }
            }
            if let Some(last) = cur_fid {
                if cur_start < range.end {
                    span_runs.push((cur_start..range.end, last));
                }
            }
        }

        let mut attrs_list = AttrsList::new(&Attrs::new());
        for (span_range, fid) in span_runs {
            let loaded_font = self.loaded_font(fid);
            let font = self.font_system.db().face(loaded_font.font.id()).unwrap();

            attrs_list.add_span(
                span_range,
                &Attrs::new()
                    .metadata(fid.0)
                    .family(Family::Name(&font.families.first().unwrap().0))
                    .stretch(font.stretch)
                    .style(font.style)
                    .weight(font.weight)
                    .font_features(loaded_font.features.clone()),
            );
        }

        let line = ShapeLine::new(
            &mut self.font_system,
            text,
            &attrs_list,
            cosmic_text::Shaping::Advanced,
            4,
        );
        let mut layout_lines = Vec::with_capacity(1);
        line.layout_to_buffer(
            &mut self.scratch,
            font_size.0,
            None, // We do our own wrapping
            cosmic_text::Wrap::None,
            None,
            &mut layout_lines,
            None,
        );
        let layout = layout_lines.first().unwrap();

        let mut runs: Vec<ShapedRun> = Vec::new();
        for glyph in &layout.glyphs {
            let mut font_id = FontId(glyph.metadata);
            let mut loaded_font = self.loaded_font(font_id);
            if loaded_font.font.id() != glyph.font_id {
                font_id = self.font_id_for_cosmic_id(glyph.font_id);
                loaded_font = self.loaded_font(font_id);
            }
            let is_emoji = loaded_font.is_known_emoji_font;

            // HACK: Prevent crash caused by variation selectors.
            if glyph.glyph_id == 3 && is_emoji {
                continue;
            }

            let shaped_glyph = ShapedGlyph {
                id: GlyphId(glyph.glyph_id as u32),
                position: point(glyph.x.into(), glyph.y.into()),
                index: glyph.start,
                is_emoji,
            };

            if let Some(last_run) = runs
                .last_mut()
                .filter(|last_run| last_run.font_id == font_id)
            {
                last_run.glyphs.push(shaped_glyph);
            } else {
                runs.push(ShapedRun {
                    font_id,
                    glyphs: vec![shaped_glyph],
                });
            }
        }

        LineLayout {
            font_size,
            width: layout.w.into(),
            ascent: layout.max_ascent.into(),
            descent: layout.max_descent.into(),
            runs,
            len: text.len(),
        }
    }
}

impl TryFrom<&FontFeatures> for CosmicFontFeatures {
    type Error = anyhow::Error;

    fn try_from(features: &FontFeatures) -> Result<Self> {
        let mut result = CosmicFontFeatures::new();
        for feature in features.0.iter() {
            let name_bytes: [u8; 4] = feature
                .0
                .as_bytes()
                .try_into()
                .context("Incorrect feature flag format")?;

            let tag = cosmic_text::FeatureTag::new(&name_bytes);

            result.set(tag, feature.1);
        }
        Ok(result)
    }
}

impl From<RectF> for Bounds<f32> {
    fn from(rect: RectF) -> Self {
        Bounds {
            origin: point(rect.origin_x(), rect.origin_y()),
            size: size(rect.width(), rect.height()),
        }
    }
}

impl From<RectI> for Bounds<DevicePixels> {
    fn from(rect: RectI) -> Self {
        Bounds {
            origin: point(DevicePixels(rect.origin_x()), DevicePixels(rect.origin_y())),
            size: size(DevicePixels(rect.width()), DevicePixels(rect.height())),
        }
    }
}

impl From<Vector2I> for Size<DevicePixels> {
    fn from(value: Vector2I) -> Self {
        size(value.x().into(), value.y().into())
    }
}

impl From<RectI> for Bounds<i32> {
    fn from(rect: RectI) -> Self {
        Bounds {
            origin: point(rect.origin_x(), rect.origin_y()),
            size: size(rect.width(), rect.height()),
        }
    }
}

impl From<Point<u32>> for Vector2I {
    fn from(size: Point<u32>) -> Self {
        Vector2I::new(size.x as i32, size.y as i32)
    }
}

impl From<Vector2F> for Size<f32> {
    fn from(vec: Vector2F) -> Self {
        size(vec.x(), vec.y())
    }
}

impl From<FontWeight> for cosmic_text::Weight {
    fn from(value: FontWeight) -> Self {
        cosmic_text::Weight(value.0 as u16)
    }
}

impl From<FontStyle> for cosmic_text::Style {
    fn from(style: FontStyle) -> Self {
        match style {
            FontStyle::Normal => cosmic_text::Style::Normal,
            FontStyle::Italic => cosmic_text::Style::Italic,
            FontStyle::Oblique => cosmic_text::Style::Oblique,
        }
    }
}

fn font_into_properties(font: &crate::Font) -> font_kit::properties::Properties {
    font_kit::properties::Properties {
        style: match font.style {
            crate::FontStyle::Normal => font_kit::properties::Style::Normal,
            crate::FontStyle::Italic => font_kit::properties::Style::Italic,
            crate::FontStyle::Oblique => font_kit::properties::Style::Oblique,
        },
        weight: font_kit::properties::Weight(font.weight.0),
        stretch: Default::default(),
    }
}

fn face_info_into_properties(
    face_info: &cosmic_text::fontdb::FaceInfo,
) -> font_kit::properties::Properties {
    font_kit::properties::Properties {
        style: match face_info.style {
            cosmic_text::Style::Normal => font_kit::properties::Style::Normal,
            cosmic_text::Style::Italic => font_kit::properties::Style::Italic,
            cosmic_text::Style::Oblique => font_kit::properties::Style::Oblique,
        },
        // both libs use the same values for weight
        weight: font_kit::properties::Weight(face_info.weight.0.into()),
        stretch: match face_info.stretch {
            cosmic_text::Stretch::Condensed => font_kit::properties::Stretch::CONDENSED,
            cosmic_text::Stretch::Expanded => font_kit::properties::Stretch::EXPANDED,
            cosmic_text::Stretch::ExtraCondensed => font_kit::properties::Stretch::EXTRA_CONDENSED,
            cosmic_text::Stretch::ExtraExpanded => font_kit::properties::Stretch::EXTRA_EXPANDED,
            cosmic_text::Stretch::Normal => font_kit::properties::Stretch::NORMAL,
            cosmic_text::Stretch::SemiCondensed => font_kit::properties::Stretch::SEMI_CONDENSED,
            cosmic_text::Stretch::SemiExpanded => font_kit::properties::Stretch::SEMI_EXPANDED,
            cosmic_text::Stretch::UltraCondensed => font_kit::properties::Stretch::ULTRA_CONDENSED,
            cosmic_text::Stretch::UltraExpanded => font_kit::properties::Stretch::ULTRA_EXPANDED,
        },
    }
}

/// quiet.md: a window-less text system over cosmic-text, for tests that shape
/// text outside an app (the bundled emoji font).
#[doc(hidden)]
pub fn test_text_system() -> Arc<crate::WindowTextSystem> {
    let ts = Arc::new(crate::TextSystem::new(Arc::new(CosmicTextSystem::new())));
    Arc::new(crate::WindowTextSystem::new(ts))
}

fn check_is_known_emoji_font(postscript_name: &str) -> bool {
    // TODO: Include other common emoji fonts
    postscript_name == "NotoColorEmoji"
}

/// quiet.md: shape `text` on Linux and return the resolved font family for each run.
#[doc(hidden)]
pub fn linux_shaped_run_families(
    text: &str,
    family: &str,
    fallbacks: Option<FontFallbacks>,
) -> Result<Vec<String>> {
    let ts = CosmicTextSystem::new();
    let mut f = crate::font(family.to_string());
    f.fallbacks = fallbacks;
    let fid = ts.font_id(&f)?;
    let line = ts.layout_line(text, crate::px(15.), &[FontRun { len: text.len(), font_id: fid }]);
    let state = ts.0.read();
    Ok(line
        .runs
        .iter()
        .map(|r| {
            let db_id = state.loaded_font(r.font_id).font.id();
            state.font_system.db().face(db_id).unwrap().families[0].0.clone()
        })
        .collect())
}
