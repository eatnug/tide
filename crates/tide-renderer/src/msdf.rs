// MSDF (Multi-channel Signed Distance Field) glyph generation.
//
// Generates resolution-independent distance fields from font glyph outlines
// using the fdsm crate. MSDF data is stored in RGB channels of an RGBA texture;
// the fragment shader reconstructs sharp edges at any render size.

use std::collections::HashMap;

/// Font data loaded for MSDF generation (owns the bytes).
pub(crate) struct FontData {
    data: Vec<u8>,
    face_index: u32,
}

impl FontData {
    pub fn new(data: Vec<u8>, face_index: u32) -> Self {
        Self { data, face_index }
    }

    pub fn face(&self) -> ttf_parser::Face<'_> {
        ttf_parser::Face::parse(&self.data, self.face_index)
            .expect("failed to parse font face")
    }
}

/// Key for looking up cached fonts.
#[derive(Hash, Eq, PartialEq, Clone)]
struct FontKey {
    family: String,
    bold: bool,
    italic: bool,
}

/// Manages font loading and MSDF glyph generation.
pub(crate) struct MsdfFontStore {
    fonts: HashMap<FontKey, FontData>,
}

impl MsdfFontStore {
    pub fn new() -> Self {
        Self {
            fonts: HashMap::new(),
        }
    }

    /// Register a font directly from raw data.
    /// Used to ensure MSDF uses the exact same font face that cosmic-text resolved.
    pub fn register_font(&mut self, family: &str, bold: bool, italic: bool, data: Vec<u8>, face_index: u32) {
        let key = FontKey {
            family: family.to_string(),
            bold,
            italic,
        };
        self.fonts.insert(key, FontData::new(data, face_index));
    }

    /// Get em-relative ascender and descender for a loaded font.
    /// Returns (em_ascender, em_descender) where both are positive.
    pub fn font_metrics(&self, family: &str, bold: bool, italic: bool) -> Option<(f32, f32)> {
        let key = FontKey {
            family: family.to_string(),
            bold,
            italic,
        };
        let font_data = self.fonts.get(&key)?;
        let face = font_data.face();
        let upm = face.units_per_em() as f32;
        let ascender = face.ascender() as f32 / upm;
        let descender = -(face.descender() as f32) / upm;
        Some((ascender, descender))
    }

    /// Load a font by family name and style, using fontdb from cosmic-text.
    pub fn load_font(
        &mut self,
        font_system: &cosmic_text::FontSystem,
        family: &str,
        bold: bool,
        italic: bool,
    ) -> bool {
        let key = FontKey {
            family: family.to_string(),
            bold,
            italic,
        };
        if self.fonts.contains_key(&key) {
            return true;
        }

        let db = font_system.db();
        let weight = if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL };
        let style = if italic { fontdb::Style::Italic } else { fontdb::Style::Normal };

        let face_id = if family == "Monospace" {
            // fontdb::Family::Monospace requires set_monospace_family() which
            // cosmic-text doesn't configure.  Try well-known monospace fonts
            // in preference order to match cosmic-text's resolution.
            const MONOSPACE_CANDIDATES: &[&str] = &[
                "Menlo", "SF Mono", "Monaco", "DejaVu Sans Mono",
                "Liberation Mono", "Courier New", "Courier",
            ];
            let mut found = None;
            for candidate in MONOSPACE_CANDIDATES {
                let name = candidate.to_string();
                let families = vec![fontdb::Family::Name(&name)];
                let query = fontdb::Query {
                    families: &families,
                    weight,
                    stretch: fontdb::Stretch::Normal,
                    style,
                };
                if let Some(id) = db.query(&query) {
                    found = Some(id);
                    break;
                }
            }
            // Fallback: pick any monospace face from the database
            found.or_else(|| {
                db.faces()
                    .find(|f| f.monospaced && f.weight == weight && f.style == style)
                    .or_else(|| db.faces().find(|f| f.monospaced))
                    .map(|f| f.id)
            })
        } else {
            let family_name = family.to_string();
            let families = vec![fontdb::Family::Name(&family_name)];
            let query = fontdb::Query {
                families: &families,
                weight,
                stretch: fontdb::Stretch::Normal,
                style,
            };
            db.query(&query)
        };

        if let Some(face_id) = face_id {
            let mut font_data = None;
            db.with_face_data(face_id, |data, index| {
                font_data = Some(FontData::new(data.to_vec(), index));
            });
            if let Some(fd) = font_data {
                self.fonts.insert(key, fd);
                return true;
            }
        }
        false
    }

    /// Generate MSDF for a glyph using the specified font.
    /// Returns None if the font isn't loaded, glyph has no outline, etc.
    pub fn generate(
        &self,
        family: &str,
        bold: bool,
        italic: bool,
        character: char,
    ) -> Option<MsdfGlyph> {
        let key = FontKey {
            family: family.to_string(),
            bold,
            italic,
        };
        let font_data = self.fonts.get(&key)?;
        let face = font_data.face();
        generate_msdf_glyph(&face, character)
    }
}

/// MSDF generation result for a single glyph.
pub(crate) struct MsdfGlyph {
    /// RGBA pixel data (RGB = distance channels, A = 255)
    pub rgba_data: Vec<u8>,
    /// Texel dimensions of the MSDF image
    pub width: u32,
    pub height: u32,
    /// Glyph metrics in em-relative units (already divided by units_per_em)
    pub em_left: f32,
    pub em_top: f32,
    pub em_width: f32,
    pub em_height: f32,
}

/// Distance range in texels for MSDF generation.
pub(crate) const PX_RANGE: f64 = 4.0;

/// Target texel height for a full em-square glyph in the MSDF atlas.
const TARGET_EM_TEXELS: f64 = 48.0;

/// Generate an MSDF for a single glyph from its font outline.
fn generate_msdf_glyph(face: &ttf_parser::Face<'_>, character: char) -> Option<MsdfGlyph> {
    use fdsm::bezier::scanline::FillRule;
    use fdsm::generate::generate_msdf;
    use fdsm::render::correct_sign_msdf;
    use fdsm::shape::Shape;
    use fdsm::transform::Transform;
    use image::RgbImage;

    let glyph_id = face.glyph_index(character)?;
    let bbox = match face.glyph_bounding_box(glyph_id) {
        Some(b) => b,
        None => {
            if character.is_ascii_graphic() {
                log::warn!("MSDF: no bounding box for '{character}' glyph_id={:?}", glyph_id);
            }
            return None;
        }
    };
    let units_per_em = face.units_per_em() as f64;
    let shrinkage = units_per_em / TARGET_EM_TEXELS;
    let scale = 1.0 / shrinkage;

    // Compute MSDF texel dimensions (glyph bbox + distance range padding)
    let glyph_w_texels = (bbox.x_max as f64 - bbox.x_min as f64) * scale;
    let glyph_h_texels = (bbox.y_max as f64 - bbox.y_min as f64) * scale;
    let width = (glyph_w_texels + 2.0 * PX_RANGE).ceil().max(1.0) as u32;
    let height = (glyph_h_texels + 2.0 * PX_RANGE).ceil().max(1.0) as u32;

    // Load glyph outline and convert to fdsm shape
    let mut shape = match fdsm_ttf_parser::load_shape_from_face(face, glyph_id) {
        Some(s) => s,
        None => {
            if character.is_ascii_graphic() {
                log::warn!("MSDF: load_shape_from_face returned None for '{character}' glyph_id={:?}", glyph_id);
            }
            return None;
        }
    };

    // Transform: scale font units to texels, translate so bbox starts at (PX_RANGE, PX_RANGE)
    let tx = PX_RANGE - bbox.x_min as f64 * scale;
    let ty = PX_RANGE - bbox.y_min as f64 * scale;
    let transformation = nalgebra::convert::<_, nalgebra::Affine2<f64>>(
        nalgebra::Similarity2::new(nalgebra::Vector2::new(tx, ty), 0.0, scale),
    );
    shape.transform(&transformation);

    // Color edges for multi-channel distance field
    let colored = Shape::edge_coloring_simple(shape, 0.03, 69441337420);
    let prepared = colored.prepare();

    // Generate MSDF
    let mut msdf = RgbImage::new(width, height);
    generate_msdf(&prepared, PX_RANGE, &mut msdf);
    correct_sign_msdf(&mut msdf, &prepared, FillRule::Nonzero);

    // Flip vertically: font y-up → image y-down
    image::imageops::flip_vertical_in_place(&mut msdf);

    // Convert RGB → RGBA (GPU requires Rgba8, not Rgb8)
    let rgb_bytes = msdf.into_raw();
    let mut rgba_data = Vec::with_capacity((width * height * 4) as usize);
    for chunk in rgb_bytes.chunks(3) {
        rgba_data.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
    }

    // Em-relative metrics for the FULL MSDF texture (glyph bbox + PX_RANGE padding).
    // The quad must match the MSDF extent so that UV→texel mapping preserves
    // the distance field scale and the shader's fwidth() anti-aliasing works correctly.
    let pad_font_units = PX_RANGE * shrinkage; // padding in font units
    let msdf_x_min = bbox.x_min as f64 - pad_font_units;
    let msdf_y_max = bbox.y_max as f64 + pad_font_units;
    // Use actual texture dimensions for width/height (accounts for ceil rounding)
    let em_left = (msdf_x_min / units_per_em) as f32;
    let em_top = (msdf_y_max / units_per_em) as f32;
    let em_width = (width as f64 / TARGET_EM_TEXELS) as f32;
    let em_height = (height as f64 / TARGET_EM_TEXELS) as f32;

    Some(MsdfGlyph {
        rgba_data,
        width,
        height,
        em_left,
        em_top,
        em_width,
        em_height,
    })
}
