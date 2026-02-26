use cosmic_text::{
    Attrs, Buffer as CosmicBuffer, Family, FontSystem, Metrics, Shaping,
};
use tide_core::Size;

use crate::atlas::{AtlasRegion, GlyphCacheKey};
use crate::WgpuRenderer;

#[cfg(target_os = "macos")]
mod coretext_fallback {
    use core_foundation::base::{CFRange, CFTypeRef, TCFType};
    use core_foundation::string::CFString;
    use core_text::font::{self as ct_font, CTFont};

    // CTFontCreateForString is not exposed by the core-text crate.
    extern "C" {
        fn CTFontCreateForString(
            current_font: CFTypeRef,
            string: CFTypeRef,
            range: CFRange,
        ) -> CFTypeRef;
    }

    /// Ask macOS CoreText for the best font to render a given character.
    /// Returns the font family name (e.g. "Apple SD Gothic Neo").
    pub fn discover_font_for_char(character: char, font_size: f64) -> Option<String> {
        let base = ct_font::new_from_name("Menlo", font_size).ok()?;
        let text = character.to_string();
        let cf_text = CFString::new(&text);
        let range = CFRange::init(0, text.encode_utf16().count() as isize);

        let fallback_ref = unsafe {
            CTFontCreateForString(
                base.as_CFTypeRef(),
                cf_text.as_CFTypeRef(),
                range,
            )
        };
        if fallback_ref.is_null() {
            return None;
        }
        let fallback: CTFont = unsafe { TCFType::wrap_under_create_rule(fallback_ref as _) };
        let name = fallback.family_name();
        // Same font = no better fallback exists.
        if name == base.family_name() {
            return None;
        }
        // Filter out Apple's placeholder font.
        if name.contains("LastResort") {
            return None;
        }
        Some(name)
    }
}

/// Min/max font sizes (must match the clamp in set_font_size).
const FONT_SIZE_MIN: u32 = 8;
const FONT_SIZE_MAX: u32 = 32;

impl WgpuRenderer {
    pub(crate) fn compute_cell_size(font_system: &mut FontSystem, scale_factor: f32, base_font_size: f32) -> Size {
        let font_size = base_font_size * scale_factor;
        let line_height = (font_size * 1.2).ceil();
        let metrics = Metrics::new(font_size, line_height);

        // Create a buffer to measure a single character
        let mut buffer = CosmicBuffer::new(font_system, metrics);
        buffer.set_text(
            font_system,
            "M",
            Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(font_system, false);

        // Get the advance width from layout
        let cell_width = buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .map(|g| g.w)
            .unwrap_or(font_size * 0.6);

        Size::new(cell_width / scale_factor, line_height / scale_factor)
    }

    /// Precompute cell sizes for every integer font size (8..=32) so that
    /// set_font_size() can do a table lookup instead of font shaping.
    pub(crate) fn precompute_cell_sizes(font_system: &mut FontSystem, scale_factor: f32) -> Vec<Size> {
        (FONT_SIZE_MIN..=FONT_SIZE_MAX)
            .map(|s| Self::compute_cell_size(font_system, scale_factor, s as f32))
            .collect()
    }

    /// Look up a precomputed cell size. Falls back to compute if out of range.
    pub(crate) fn lookup_cell_size(&mut self, base_font_size: f32) -> Size {
        let idx = (base_font_size.round() as u32).saturating_sub(FONT_SIZE_MIN) as usize;
        if let Some(&size) = self.cell_size_table.get(idx) {
            size
        } else {
            Self::compute_cell_size(&mut self.font_system, self.scale_factor, base_font_size)
        }
    }

    /// Pre-warm the glyph atlas with printable ASCII characters.
    /// Call once after initialization to avoid first-frame rasterization stalls.
    pub fn warmup_ascii(&mut self) {
        for ch in '!'..='~' {
            self.ensure_glyph_cached(ch, false, false);
            self.ensure_glyph_cached(ch, true, false);
        }
    }

    /// Pre-warm Korean Jamo (consonants + vowels, 51 chars) to avoid
    /// rasterization stalls when typing Korean.
    pub fn warmup_common_unicode(&mut self) {
        // ㄱ (U+3131) .. ㅎ (U+314E): 30 consonants
        // ㅏ (U+314F) .. ㅣ (U+3163): 21 vowels
        for ch in '\u{3131}'..='\u{3163}' {
            self.ensure_glyph_cached(ch, false, false);
            self.ensure_glyph_cached(ch, true, false);
        }
    }

    /// Use cosmic-text's shaping engine to discover which font face can
    /// render a given character. cosmic-text has a full font fallback chain
    /// (including Nerd Fonts, CJK fonts, etc.) that the direct MSDF path
    /// bypasses when only checking the primary monospace font.
    fn discover_font_via_cosmic(
        font_system: &mut FontSystem,
        character: char,
        bold: bool,
        italic: bool,
        font_size: f32,
        scale_factor: f32,
    ) -> Option<fontdb::ID> {
        let font_size_px = font_size * scale_factor;
        let line_height = (font_size_px * 1.2).ceil();
        let metrics = Metrics::new(font_size_px, line_height);

        let mut attrs = Attrs::new().family(Family::Monospace);
        if bold {
            attrs = attrs.weight(cosmic_text::Weight::BOLD);
        }
        if italic {
            attrs = attrs.style(cosmic_text::Style::Italic);
        }

        let mut buffer = CosmicBuffer::new(font_system, metrics);
        let text = character.to_string();
        buffer.set_text(font_system, &text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(font_system, false);

        buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .map(|g| g.font_id)
    }

    /// Generate and cache an MSDF glyph, returning its atlas region.
    /// Tries Monospace first, then cosmic-text font fallback (which discovers
    /// Nerd Fonts, CJK fonts, etc.), then macOS CoreText as a final fallback.
    pub(crate) fn ensure_glyph_cached(&mut self, character: char, bold: bool, italic: bool) -> AtlasRegion {
        let key = GlyphCacheKey {
            character,
            bold,
            italic,
        };

        if let Some(region) = self.atlas.cache.get(&key) {
            return *region;
        }

        // Try Monospace first
        let region = self.try_generate_msdf(character, bold, italic, "Monospace");
        if !region.is_empty() {
            self.atlas.cache.insert(key, region);
            return region;
        }

        // Use cosmic-text's shaping engine to discover the right font.
        // This leverages cosmic-text's full font fallback chain, which can
        // find Nerd Font icons, CJK glyphs, and other characters that the
        // primary monospace font doesn't contain.
        if let Some(face_id) = Self::discover_font_via_cosmic(
            &mut self.font_system,
            character,
            bold,
            italic,
            self.base_font_size,
            self.scale_factor,
        ) {
            let family_key = format!("cosmic-{face_id}");
            let mut font_data = None;
            self.font_system
                .db()
                .with_face_data(face_id, |data, index| {
                    font_data = Some((data.to_vec(), index));
                });
            if let Some((data, index)) = font_data {
                self.msdf_font_store
                    .register_font(&family_key, bold, italic, data, index);
                let region = self.try_generate_msdf(character, bold, italic, &family_key);
                if !region.is_empty() {
                    self.atlas.cache.insert(key, region);
                    return region;
                }
            }
        }

        // On macOS, ask CoreText for the best system font for this character.
        #[cfg(target_os = "macos")]
        {
            let font_size = (self.base_font_size * self.scale_factor) as f64;
            if let Some(family_name) =
                coretext_fallback::discover_font_for_char(character, font_size)
            {
                let region =
                    self.try_generate_msdf(character, bold, italic, &family_name);
                if !region.is_empty() {
                    self.atlas.cache.insert(key, region);
                    return region;
                }
            }
        }

        // Style fallback: if bold/italic variant failed, try simpler styles.
        if italic || bold {
            let fallback_attempts: &[(bool, bool)] = if bold && italic {
                &[(false, true), (true, false), (false, false)]
            } else if italic {
                &[(false, false)]
            } else {
                &[(false, false)]
            };
            for &(fb_bold, fb_italic) in fallback_attempts {
                let region = self.try_generate_msdf(character, fb_bold, fb_italic, "Monospace");
                if !region.is_empty() {
                    self.atlas.cache.insert(key, region);
                    return region;
                }
                // cosmic-text fallback for style variants
                if let Some(face_id) = Self::discover_font_via_cosmic(
                    &mut self.font_system,
                    character,
                    fb_bold,
                    fb_italic,
                    self.base_font_size,
                    self.scale_factor,
                ) {
                    let family_key = format!("cosmic-{face_id}");
                    let mut font_data = None;
                    self.font_system
                        .db()
                        .with_face_data(face_id, |data, index| {
                            font_data = Some((data.to_vec(), index));
                        });
                    if let Some((data, index)) = font_data {
                        self.msdf_font_store
                            .register_font(&family_key, fb_bold, fb_italic, data, index);
                        let region =
                            self.try_generate_msdf(character, fb_bold, fb_italic, &family_key);
                        if !region.is_empty() {
                            self.atlas.cache.insert(key, region);
                            return region;
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                {
                    let font_size = (self.base_font_size * self.scale_factor) as f64;
                    if let Some(family_name) = coretext_fallback::discover_font_for_char(character, font_size) {
                        let region = self.try_generate_msdf(character, fb_bold, fb_italic, &family_name);
                        if !region.is_empty() {
                            self.atlas.cache.insert(key, region);
                            return region;
                        }
                    }
                }
            }
        }

        // All attempts failed — cache empty region to avoid repeated retries.
        let empty = AtlasRegion {
            uv_min: [0.0, 0.0],
            uv_max: [0.0, 0.0],
            em_left: 0.0,
            em_top: 0.0,
            em_width: 0.0,
            em_height: 0.0,
        };
        self.atlas.cache.insert(key, empty);
        empty
    }

    /// Try to generate an MSDF glyph using the given font family.
    fn try_generate_msdf(
        &mut self,
        character: char,
        bold: bool,
        italic: bool,
        family: &str,
    ) -> AtlasRegion {
        let empty = AtlasRegion {
            uv_min: [0.0, 0.0],
            uv_max: [0.0, 0.0],
            em_left: 0.0,
            em_top: 0.0,
            em_width: 0.0,
            em_height: 0.0,
        };

        // Ensure font is loaded
        let loaded = self.msdf_font_store.load_font(&self.font_system, family, bold, italic);
        if !loaded {
            return empty;
        }

        // Generate MSDF
        let msdf_glyph = match self.msdf_font_store.generate(family, bold, italic, character) {
            Some(g) => g,
            None => return empty,
        };

        let cache_len_before = self.atlas.cache.len();
        let region = self.atlas.upload_glyph(
            &self.queue,
            msdf_glyph.width,
            msdf_glyph.height,
            msdf_glyph.em_left,
            msdf_glyph.em_top,
            msdf_glyph.em_width,
            msdf_glyph.em_height,
            &msdf_glyph.rgba_data,
        );
        if self.atlas.cache.is_empty() && cache_len_before > 0 {
            self.atlas_reset_count += 1;
            self.grid_needs_upload = true;
            self.chrome_needs_upload = true;
            self.warmup_ascii();
            self.warmup_common_unicode();
        }
        region
    }

    /// Get the current base font size.
    pub fn font_size(&self) -> f32 {
        self.base_font_size
    }

    /// Change the base font size at runtime (clamped to 8.0..=32.0).
    /// With MSDF, the atlas is font-size-independent, so we only need to
    /// recompute cell size and invalidate pane caches (quad positions change).
    pub fn set_font_size(&mut self, size: f32) {
        let size = size.clamp(8.0, 32.0);
        if (size - self.base_font_size).abs() < 0.01 {
            return;
        }
        self.base_font_size = size;
        self.cached_cell_size = self.lookup_cell_size(size);
        // No atlas reset! MSDF atlas is font-size-independent.
        self.invalidate_all_pane_caches();
        self.atlas_reset_count += 1;
        self.grid_needs_upload = true;
        self.chrome_needs_upload = true;
    }
}
