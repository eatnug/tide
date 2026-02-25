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

    /// Rasterize and cache a glyph, returning its atlas region.
    /// Tries Monospace first (with cosmic-text internal fallback), then asks
    /// macOS CoreText for the best system font as a final fallback.
    pub(crate) fn ensure_glyph_cached(&mut self, character: char, bold: bool, italic: bool) -> AtlasRegion {
        let key = GlyphCacheKey {
            character,
            bold,
            italic,
        };

        if let Some(region) = self.atlas.cache.get(&key) {
            return *region;
        }

        // Try Monospace first (cosmic-text does internal fallback during shaping).
        let region = self.try_rasterize_glyph(character, bold, italic, Family::Monospace);
        if region.width > 0 && region.height > 0 {
            self.atlas.cache.insert(key, region);
            return region;
        }

        // On macOS, ask CoreText for the best system font for this character.
        #[cfg(target_os = "macos")]
        {
            let font_size = (self.base_font_size * self.scale_factor) as f64;
            if let Some(family_name) = coretext_fallback::discover_font_for_char(character, font_size) {
                let region = self.try_rasterize_glyph(
                    character, bold, italic,
                    Family::Name(&family_name),
                );
                if region.width > 0 && region.height > 0 {
                    self.atlas.cache.insert(key, region);
                    return region;
                }
            }
        }

        // Style fallback: if bold/italic variant failed, try progressively
        // simpler styles. Better to show a character in regular style than
        // show nothing (common for CJK fonts missing italic variants).
        if italic || bold {
            let fallback_attempts: &[(bool, bool)] = if bold && italic {
                &[(false, true), (true, false), (false, false)]
            } else if italic {
                &[(false, false)]
            } else {
                // bold only
                &[(false, false)]
            };
            for &(fb_bold, fb_italic) in fallback_attempts {
                let region = self.try_rasterize_glyph(character, fb_bold, fb_italic, Family::Monospace);
                if region.width > 0 && region.height > 0 {
                    self.atlas.cache.insert(key, region);
                    return region;
                }
                #[cfg(target_os = "macos")]
                {
                    let font_size = (self.base_font_size * self.scale_factor) as f64;
                    if let Some(family_name) = coretext_fallback::discover_font_for_char(character, font_size) {
                        let region = self.try_rasterize_glyph(
                            character, fb_bold, fb_italic,
                            Family::Name(&family_name),
                        );
                        if region.width > 0 && region.height > 0 {
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
            width: 0,
            height: 0,
            left: 0.0,
            top: 0.0,
        };
        self.atlas.cache.insert(key, empty);
        empty
    }

    /// Try to rasterize a single glyph using the given font family.
    /// Returns an AtlasRegion with width=0 if the glyph couldn't be rendered.
    fn try_rasterize_glyph(
        &mut self,
        character: char,
        bold: bool,
        italic: bool,
        family: Family,
    ) -> AtlasRegion {
        let font_size = self.base_font_size * self.scale_factor;
        let line_height = (font_size * 1.2).ceil();
        let metrics = Metrics::new(font_size, line_height);

        let mut attrs = Attrs::new().family(family);
        if bold {
            attrs = attrs.weight(cosmic_text::Weight::BOLD);
        }
        if italic {
            attrs = attrs.style(cosmic_text::Style::Italic);
        }

        let mut buffer = CosmicBuffer::new(&mut self.font_system, metrics);
        let text = character.to_string();
        buffer.set_text(&mut self.font_system, &text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        let empty = AtlasRegion {
            uv_min: [0.0, 0.0],
            uv_max: [0.0, 0.0],
            width: 0,
            height: 0,
            left: 0.0,
            top: 0.0,
        };

        let run = match buffer.layout_runs().next() {
            Some(r) => r,
            None => return empty,
        };
        let glyph = match run.glyphs.first() {
            Some(g) => g,
            None => return empty,
        };

        let physical = glyph.physical((0.0, 0.0), 1.0);
        let image = match self
            .swash_cache
            .get_image(&mut self.font_system, physical.cache_key)
        {
            Some(img) => img,
            None => return empty,
        };

        let width = image.placement.width;
        let height = image.placement.height;
        if width == 0 || height == 0 {
            return empty;
        }

        let left = image.placement.left as f32;
        let top = image.placement.top as f32;

        let alpha_data: Vec<u8> = match image.content {
            cosmic_text::SwashContent::Mask => image.data.clone(),
            cosmic_text::SwashContent::Color => {
                image.data.chunks(4).map(|c| c.get(3).copied().unwrap_or(255)).collect()
            }
            cosmic_text::SwashContent::SubpixelMask => {
                image.data.chunks(3).map(|c| {
                    let r = c.first().copied().unwrap_or(0) as u16;
                    let g = c.get(1).copied().unwrap_or(0) as u16;
                    let b = c.get(2).copied().unwrap_or(0) as u16;
                    ((r + g + b) / 3) as u8
                }).collect()
            }
        };

        let cache_len_before = self.atlas.cache.len();
        let region = self.atlas.upload_glyph(
            &self.queue,
            width,
            height,
            left,
            top,
            &alpha_data,
        );
        if self.atlas.cache.is_empty() && cache_len_before > 0 {
            self.atlas_reset_count += 1;
            self.grid_needs_upload = true;
            self.chrome_needs_upload = true;
        }
        region
    }

    /// Get the current base font size.
    pub fn font_size(&self) -> f32 {
        self.base_font_size
    }

    /// Change the base font size at runtime (clamped to 8.0..=32.0).
    /// Recomputes cell size, resets the glyph atlas, and invalidates all pane caches.
    pub fn set_font_size(&mut self, size: f32) {
        let size = size.clamp(8.0, 32.0);
        if (size - self.base_font_size).abs() < 0.01 {
            return;
        }
        self.base_font_size = size;
        self.cached_cell_size = Self::compute_cell_size(&mut self.font_system, self.scale_factor, size);
        self.atlas.reset();
        self.swash_cache = cosmic_text::SwashCache::new();
        self.invalidate_all_pane_caches();
        self.warmup_ascii();
        self.warmup_common_unicode();
        self.atlas_reset_count += 1;
        self.grid_needs_upload = true;
        self.chrome_needs_upload = true;
    }
}
