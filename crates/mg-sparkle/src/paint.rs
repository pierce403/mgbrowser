//! Rust software painting. Pixels are packed 0x00RRGGBB for the window surface.
//!
//! Font files are data: shaping and rasterization never call a native font API.
//! A host may supply a separate bold face; mixed-direction bidi layout and
//! script fallback faces are not implemented. Callers split text into lines.

use std::collections::HashMap;

const GLYPH_CACHE_BYTES: usize = 8 * 1024 * 1024;
const GLYPH_CACHE_ENTRIES: usize = 4096;

/// Opaque saved physical clip. Callers own nesting tokens; Canvas allocates no
/// clip stack. Restore on the same surface to recover its previous exact bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanvasClip {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

pub struct Canvas {
    /// Physical output pixels. All painting coordinates remain logical pixels.
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
    scale: f32,
    clip: CanvasClip,
}

impl Canvas {
    pub fn new(width: u32, height: u32, bg: u32) -> Self {
        Self::new_scaled(width, height, bg, 1.0)
    }

    /// Rasterize logical geometry into a physical surface without bitmap scaling.
    /// The host owns the allocation budget, as with `new`. Positive finite scale
    /// is bounded to 0.75..=3; invalid scale falls back to 1. Dimensions and pixel
    /// count are checked before allocation rather than wrapping on overflow.
    pub fn new_scaled(logical_width: u32, logical_height: u32, bg: u32, scale: f32) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale.clamp(0.75, 3.0)
        } else {
            1.0
        };
        let dimension = |logical: u32| {
            let physical = (f64::from(logical) * f64::from(scale)).round();
            assert!(physical <= f64::from(u32::MAX), "Canvas dimension overflow");
            physical as u32
        };
        let width = dimension(logical_width);
        let height = dimension(logical_height);
        let len = (width as usize)
            .checked_mul(height as usize)
            .filter(|len| *len <= isize::MAX as usize / std::mem::size_of::<u32>())
            .expect("Canvas allocation size overflow");
        Self {
            width,
            height,
            pixels: vec![bg & 0x00ff_ffff; len],
            scale,
            clip: CanvasClip {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            },
        }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Map a logical edge consistently for adjacent rectangles and composition.
    pub fn physical_edge(&self, logical: i64) -> i64 {
        (logical as f64 * f64::from(self.scale)).round() as i64
    }

    /// Intersect with a logical rectangle and return the previous clip for
    /// caller-owned restoration. Edges are converted once using the same
    /// rounding as rectangles. Empty/outside/extreme rectangles remain empty;
    /// they cannot wrap or expand the current clip.
    pub fn intersect_clip(&mut self, x: i32, y: i32, w: u32, h: u32) -> CanvasClip {
        let previous = self.clip;
        let edge_x = |value| self.physical_edge(value).clamp(0, i64::from(self.width)) as u32;
        let edge_y = |value| self.physical_edge(value).clamp(0, i64::from(self.height)) as u32;
        let left = previous.left.max(edge_x(i64::from(x)));
        let top = previous.top.max(edge_y(i64::from(y)));
        let right = previous
            .right
            .min(edge_x(i64::from(x) + i64::from(w)))
            .max(left);
        let bottom = previous
            .bottom
            .min(edge_y(i64::from(y) + i64::from(h)))
            .max(top);
        self.clip = CanvasClip {
            left,
            top,
            right,
            bottom,
        };
        previous
    }

    /// Restore a saved clip without allocating or reconverting logical edges.
    /// A token from another surface is clamped to this canvas for safe bounds.
    pub fn restore_clip(&mut self, clip: CanvasClip) {
        self.clip = CanvasClip {
            left: clip.left.min(self.width),
            top: clip.top.min(self.height),
            right: clip.right.min(self.width),
            bottom: clip.bottom.min(self.height),
        };
    }

    /// Active physical half-open `(left, top, right, bottom)` bounds. Custom
    /// pixel loops must respect these as well as the surface allocation.
    pub fn physical_clip_bounds(&self) -> (u32, u32, u32, u32) {
        (
            self.clip.left,
            self.clip.top,
            self.clip.right,
            self.clip.bottom,
        )
    }

    /// Fill the intersection with the active clip, including negative coordinates.
    pub fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        let left =
            self.physical_edge(i64::from(x))
                .clamp(i64::from(self.clip.left), i64::from(self.clip.right)) as usize;
        let top =
            self.physical_edge(i64::from(y))
                .clamp(i64::from(self.clip.top), i64::from(self.clip.bottom)) as usize;
        let right =
            self.physical_edge(i64::from(x) + i64::from(w))
                .clamp(i64::from(self.clip.left), i64::from(self.clip.right)) as usize;
        let bottom =
            self.physical_edge(i64::from(y) + i64::from(h))
                .clamp(i64::from(self.clip.top), i64::from(self.clip.bottom)) as usize;
        let color = color & 0x00ff_ffff;
        for row in top..bottom {
            let start = row * self.width as usize;
            self.pixels[start + left..start + right].fill(color);
        }
    }

    /// Paint a shaped, single line; `y` is the top of its font line box.
    pub fn text(&mut self, fonts: &mut Fonts, x: i32, y: i32, text: &str, size: f32, color: u32) {
        let size = font_size(size);
        let ascent = fonts
            .font
            .horizontal_line_metrics(size)
            .map_or(size, |metrics| metrics.ascent);
        let baseline = y as f32 + ascent;
        let mut pen = x as f32;
        for glyph in fonts.shape(text, size) {
            // Shape and advance in logical units, but rasterize the outline at
            // the actual output resolution. Do not enlarge a 1x glyph bitmap.
            let cached = fonts.glyph(glyph.id, size * self.scale);
            let metrics = cached.0;
            let left =
                ((pen + glyph.x_offset) * self.scale).round() as i64 + i64::from(metrics.xmin);
            let top = ((baseline - glyph.y_offset) * self.scale).round() as i64
                - i64::from(metrics.ymin)
                - metrics.height as i64;
            let from_x = left.clamp(i64::from(self.clip.left), i64::from(self.clip.right));
            let from_y = top.clamp(i64::from(self.clip.top), i64::from(self.clip.bottom));
            let to_x = (left + metrics.width as i64)
                .clamp(i64::from(self.clip.left), i64::from(self.clip.right));
            let to_y = (top + metrics.height as i64)
                .clamp(i64::from(self.clip.top), i64::from(self.clip.bottom));
            for screen_y in from_y..to_y {
                let source_row = (screen_y - top) as usize * metrics.width;
                let target_row = screen_y as usize * self.width as usize;
                for screen_x in from_x..to_x {
                    let coverage = cached.1[source_row + (screen_x - left) as usize];
                    let target = &mut self.pixels[target_row + screen_x as usize];
                    *target = blend(*target, color, coverage);
                }
            }
            pen += glyph.advance;
        }
    }

    /// Paint with the host-provided bold face, falling back to the regular face.
    pub fn text_weight(
        &mut self,
        fonts: &mut Fonts,
        position: (i32, i32),
        text: &str,
        size: f32,
        color: u32,
        bold: bool,
    ) {
        if bold && let Some(face) = fonts.bold.as_deref_mut() {
            self.text(face, position.0, position.1, text, size, color);
        } else {
            self.text(fonts, position.0, position.1, text, size, color);
        }
    }

    pub fn save_png(&self, path: &str) -> Result<(), String> {
        let mut image = image::RgbImage::new(self.width, self.height);
        for (target, &pixel) in image.pixels_mut().zip(&self.pixels) {
            *target = image::Rgb([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
        }
        image
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|error| format!("Cannot save PNG {path}: {error}"))
    }
}

fn blend(background: u32, foreground: u32, coverage: u8) -> u32 {
    let alpha = u32::from(coverage);
    let inverse = 255 - alpha;
    let channel = |shift: u32| -> u32 {
        (((foreground >> shift) & 255) * alpha + ((background >> shift) & 255) * inverse + 127)
            / 255
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

fn font_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(1.0, 256.0)
    } else {
        16.0
    }
}

struct PositionedGlyph {
    id: u16,
    advance: f32,
    x_offset: f32,
    y_offset: f32,
}

pub struct Fonts {
    bytes: Vec<u8>,
    font: fontdue::Font,
    cache: HashMap<(u16, u32), (fontdue::Metrics, Vec<u8>)>,
    cached_bytes: usize,
    bold: Option<Box<Fonts>>,
}

impl Fonts {
    /// Parse caller-owned font data without selecting a platform font or doing I/O.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let font = fontdue::Font::from_bytes(&*bytes, fontdue::FontSettings::default())
            .map_err(|error| format!("Cannot rasterize font: {error}"))?;
        rustybuzz::Face::from_slice(&bytes, 0).ok_or_else(|| "Cannot shape font".to_owned())?;
        Ok(Self {
            bytes,
            font,
            cache: HashMap::new(),
            cached_bytes: 0,
            bold: None,
        })
    }

    /// Supply a separately parsed bold font without performing platform I/O.
    pub fn from_bytes_with_bold(bytes: Vec<u8>, bold: Option<Vec<u8>>) -> Result<Self, String> {
        let mut fonts = Self::from_bytes(bytes)?;
        fonts.bold = bold.map(Self::from_bytes).transpose()?.map(Box::new);
        Ok(fonts)
    }

    /// Measure the same selected face used by `Canvas::text_weight`.
    pub fn width_weight(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        if bold && let Some(face) = self.bold.as_deref_mut() {
            face.width(text, size)
        } else {
            self.width(text, size)
        }
    }

    /// Measure the same shaped advances that `Canvas::text` paints.
    pub fn width(&mut self, text: &str, size: f32) -> f32 {
        self.shape(text, font_size(size))
            .iter()
            .map(|glyph| glyph.advance)
            .sum::<f32>()
            .max(0.0)
    }

    pub fn line_height(&self, size: f32) -> i32 {
        let size = font_size(size);
        self.font
            .horizontal_line_metrics(size)
            .map_or(size * 1.25, |metrics| metrics.new_line_size)
            .ceil() as i32
    }

    /// Logical-pixel-aligned normal CSS line metrics. Round ascent and descent
    /// separately, retaining a stable baseline independent of output scale.
    pub fn css_line_height(&self, size: f32) -> f32 {
        let size = font_size(size);
        self.font
            .horizontal_line_metrics(size)
            .map_or(size * 1.2, |metrics| {
                metrics.ascent.round() - metrics.descent.round() + metrics.line_gap.round()
            })
    }

    fn shape(&self, text: &str, size: f32) -> Vec<PositionedGlyph> {
        // The face was validated when the font was loaded and bytes are private.
        let face = rustybuzz::Face::from_slice(&self.bytes, 0).expect("validated font face");
        let scale = size / face.units_per_em() as f32;
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(&face, &[], buffer);
        glyphs
            .glyph_infos()
            .iter()
            .zip(glyphs.glyph_positions())
            .map(|(info, position)| PositionedGlyph {
                id: info.glyph_id as u16,
                advance: position.x_advance as f32 * scale,
                x_offset: position.x_offset as f32 * scale,
                y_offset: position.y_offset as f32 * scale,
            })
            .collect()
    }

    fn glyph(&mut self, id: u16, size: f32) -> &(fontdue::Metrics, Vec<u8>) {
        let key = (id, size.to_bits());
        if !self.cache.contains_key(&key) {
            let glyph = self.font.rasterize_indexed(id, size);
            if self.cached_bytes + glyph.1.len() > GLYPH_CACHE_BYTES
                || self.cache.len() >= GLYPH_CACHE_ENTRIES
            {
                self.cache.clear();
                self.cached_bytes = 0;
            }
            self.cached_bytes += glyph.1.len();
            self.cache.insert(key, glyph);
        }
        &self.cache[&key]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_clip_negative_and_extreme_coordinates() {
        let mut canvas = Canvas::new(4, 3, 0xffffff);
        canvas.rect(-2, -1, 4, 3, 0x123456);
        assert_eq!(
            canvas.pixels,
            [
                0x123456, 0x123456, 0xffffff, 0xffffff, 0x123456, 0x123456, 0xffffff, 0xffffff,
                0xffffff, 0xffffff, 0xffffff, 0xffffff
            ]
        );
        canvas.rect(i32::MAX, i32::MAX, u32::MAX, u32::MAX, 0);
        canvas.rect(i32::MIN, i32::MIN, 10, 10, 0);
        assert_eq!(canvas.pixels[0], 0x123456);
        canvas.rect(i32::MIN, i32::MIN, u32::MAX, u32::MAX, 0);
        assert!(canvas.pixels.iter().all(|&pixel| pixel == 0));
    }

    #[test]
    fn coverage_blends_without_tinting_other_channels() {
        assert_eq!(blend(0x123456, 0xaabbcc, 0), 0x123456);
        assert_eq!(blend(0x123456, 0xaabbcc, 255), 0xaabbcc);
        assert_eq!(blend(0xffffff, 0, 128), 0x7f7f7f);
    }

    #[test]
    fn scaled_canvas_dimensions_edges_and_negative_clipping() {
        for scale in [1.0, 1.25, 2.0] {
            let mut canvas = Canvas::new_scaled(8, 4, 0xffffff, scale);
            assert_eq!(canvas.scale(), scale);
            assert_eq!(canvas.width, (8.0 * scale).round() as u32);
            assert_eq!(canvas.height, (4.0 * scale).round() as u32);
            assert_eq!(canvas.pixels.len(), (canvas.width * canvas.height) as usize);
            canvas.rect(-1, -1, 3, 3, 0x123456);
            let end = canvas.physical_edge(2) as usize;
            for y in 0..canvas.height as usize {
                for x in 0..canvas.width as usize {
                    assert_eq!(
                        canvas.pixels[y * canvas.width as usize + x],
                        if x < end && y < end {
                            0x123456
                        } else {
                            0xffffff
                        }
                    );
                }
            }
            // Adjacent logical rectangles share a physical edge at fractional
            // scale instead of accumulating rounded-width gaps or overlap.
            canvas.rect(2, 0, 3, 4, 0xabcdef);
            canvas.rect(5, 0, 3, 4, 0x987654);
            let edge = canvas.physical_edge(5) as usize;
            assert_eq!(canvas.pixels[edge - 1], 0xabcdef);
            assert_eq!(canvas.pixels[edge], 0x987654);
            let before = canvas.pixels.clone();
            canvas.rect(i32::MAX, i32::MAX, u32::MAX, u32::MAX, 0);
            canvas.rect(i32::MIN, i32::MIN, 10, 10, 0);
            assert_eq!(canvas.pixels, before);
        }
    }

    #[test]
    fn scaled_canvas_bounds_invalid_scale_and_checks_dimension_overflow() {
        for scale in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -2.0] {
            let canvas = Canvas::new_scaled(8, 4, 0, scale);
            assert_eq!((canvas.width, canvas.height, canvas.scale()), (8, 4, 1.0));
        }
        assert_eq!(Canvas::new_scaled(8, 4, 0, f32::MAX).scale(), 3.0);
        assert_eq!(Canvas::new_scaled(8, 4, 0, f32::MIN_POSITIVE).scale(), 0.75);
        assert!(std::panic::catch_unwind(|| Canvas::new_scaled(u32::MAX, 0, 0, 3.0)).is_err());
        assert!(std::panic::catch_unwind(|| Canvas::new(u32::MAX, u32::MAX, 0)).is_err());
    }

    #[test]
    fn nested_rectangular_clips_round_once_and_restore_exactly() {
        for scale in [1.0, 1.25, 2.0] {
            let mut canvas = Canvas::new_scaled(16, 12, 0xffffff, scale);
            let full = canvas.intersect_clip(-2, 2, 10, 8);
            let outer = canvas.physical_clip_bounds();
            assert_eq!(
                outer,
                (
                    0,
                    canvas.physical_edge(2) as u32,
                    canvas.physical_edge(8) as u32,
                    canvas.physical_edge(10) as u32
                )
            );
            let saved_outer = canvas.intersect_clip(4, -2, 12, 8);
            let nested = canvas.physical_clip_bounds();
            assert_eq!(
                nested,
                (
                    canvas.physical_edge(4) as u32,
                    canvas.physical_edge(2) as u32,
                    canvas.physical_edge(8) as u32,
                    canvas.physical_edge(6) as u32
                )
            );
            canvas.rect(i32::MIN, i32::MIN, u32::MAX, u32::MAX, 0x123456);
            for y in 0..canvas.height {
                for x in 0..canvas.width {
                    let inside = x >= nested.0 && x < nested.2 && y >= nested.1 && y < nested.3;
                    assert_eq!(
                        canvas.pixels[(y * canvas.width + x) as usize],
                        if inside { 0x123456 } else { 0xffffff }
                    );
                }
            }
            canvas.restore_clip(saved_outer);
            assert_eq!(canvas.physical_clip_bounds(), outer);
            canvas.restore_clip(full);
            assert_eq!(
                canvas.physical_clip_bounds(),
                (0, 0, canvas.width, canvas.height)
            );
            canvas.rect(0, 0, 16, 12, 0xabcdef);
            assert!(canvas.pixels.iter().all(|&pixel| pixel == 0xabcdef));
        }
    }

    #[test]
    fn empty_extreme_and_foreign_clip_tokens_cannot_escape_surface() {
        for scale in [1.0, 1.25, 2.0] {
            let mut canvas = Canvas::new_scaled(16, 12, 0xffffff, scale);
            for (x, y, w, h) in [
                (i32::MIN, i32::MIN, 1, 1),
                (i32::MAX, i32::MAX, u32::MAX, u32::MAX),
                (0, 0, 0, 12),
                (0, 0, 16, 0),
            ] {
                let restore = canvas.intersect_clip(x, y, w, h);
                canvas.rect(0, 0, 16, 12, 0);
                assert!(canvas.pixels.iter().all(|&pixel| pixel == 0xffffff));
                canvas.restore_clip(restore);
            }
            let restore = canvas.intersect_clip(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
            assert_eq!(
                canvas.physical_clip_bounds(),
                (0, 0, canvas.width, canvas.height)
            );
            canvas.restore_clip(restore);

            let mut large = Canvas::new_scaled(30, 30, 0, scale);
            large.intersect_clip(25, 25, 5, 5);
            let foreign = large.intersect_clip(0, 0, 1, 1);
            canvas.restore_clip(foreign);
            assert_eq!(
                canvas.physical_clip_bounds(),
                (canvas.width, canvas.height, canvas.width, canvas.height)
            );
            canvas.rect(0, 0, 16, 12, 0);
            assert!(canvas.pixels.iter().all(|&pixel| pixel == 0xffffff));
        }
        let mut empty = Canvas::new(0, 0, 0);
        let saved = empty.intersect_clip(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
        empty.rect(0, 0, 4, 4, 0);
        empty.restore_clip(saved);
        assert!(empty.pixels.is_empty());
    }

    #[test]
    fn text_clip_preserves_exact_glyph_coverage_inside_and_background_outside() {
        let mut fonts = Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap();
        for scale in [1.0, 1.25, 2.0] {
            let mut reference = Canvas::new_scaled(160, 40, 0xfedcba, scale);
            reference.text(&mut fonts, 1, 1, "Magnesium", 22.0, 0x123456);
            let mut explicit_full = Canvas::new_scaled(160, 40, 0xfedcba, scale);
            explicit_full.intersect_clip(0, 0, 160, 40);
            explicit_full.text(&mut fonts, 1, 1, "Magnesium", 22.0, 0x123456);
            assert_eq!(explicit_full.pixels, reference.pixels);

            let mut clipped = Canvas::new_scaled(160, 40, 0xfedcba, scale);
            let full = clipped.intersect_clip(9, 7, 31, 9);
            let bounds = clipped.physical_clip_bounds();
            clipped.text(&mut fonts, 1, 1, "Magnesium", 22.0, 0x123456);
            let mut changed = 0;
            for y in 0..clipped.height {
                for x in 0..clipped.width {
                    let index = (y * clipped.width + x) as usize;
                    let inside = x >= bounds.0 && x < bounds.2 && y >= bounds.1 && y < bounds.3;
                    let expected = if inside {
                        reference.pixels[index]
                    } else {
                        0xfedcba
                    };
                    assert_eq!(clipped.pixels[index], expected);
                    changed += usize::from(clipped.pixels[index] != 0xfedcba);
                }
            }
            assert!(changed > 0);
            clipped.intersect_clip(9, 7, 0, 9);
            let before = clipped.pixels.clone();
            clipped.text(&mut fonts, 1, 1, "Magnesium", 22.0, 0);
            assert_eq!(clipped.pixels, before);
            clipped.restore_clip(full);
            clipped.text(&mut fonts, 1, 1, "Magnesium", 22.0, 0x123456);
            // Text outside the old clip is now visible; blending is intentionally
            // cumulative, so only compare pixels outside the old clipped region.
            for y in 0..clipped.height {
                for x in 0..clipped.width {
                    if x < bounds.0 || x >= bounds.2 || y < bounds.1 || y >= bounds.3 {
                        let index = (y * clipped.width + x) as usize;
                        assert_eq!(clipped.pixels[index], reference.pixels[index]);
                    }
                }
            }
        }
    }

    #[test]
    fn scaled_text_rasterizes_outlines_and_retains_logical_measurement() {
        let mut fonts = Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap();
        let text = "Mg browser";
        let measured = fonts.width(text, 17.0);
        let mut original = Canvas::new(180, 40, 0xffffff);
        original.text(&mut fonts, 4, 2, text, 17.0, 0);
        for scale in [1.0, 1.25, 2.0] {
            let mut canvas = Canvas::new_scaled(180, 40, 0xffffff, scale);
            canvas.text(&mut fonts, 4, 2, text, 17.0, 0);
            assert_eq!(fonts.width(text, 17.0), measured);
            assert!(
                fonts
                    .cache
                    .keys()
                    .any(|(_, bits)| *bits == (17.0 * scale).to_bits())
            );
            assert!(fonts.cached_bytes <= GLYPH_CACHE_BYTES);
            assert!(fonts.cache.len() <= GLYPH_CACHE_ENTRIES);
            if scale == 1.0 {
                assert_eq!(canvas.pixels, original.pixels);
            } else if scale == 2.0 {
                // A nearest-neighbor enlargement of the old whole frame has
                // identical pixels in every 2x2 block. Outline rasterization
                // produces actual additional edge coverage within those blocks.
                assert!((0..canvas.height as usize).step_by(2).any(|y| {
                    (0..canvas.width as usize).step_by(2).any(|x| {
                        let i = y * canvas.width as usize + x;
                        let first = canvas.pixels[i];
                        canvas.pixels[i + 1] != first
                            || canvas.pixels[i + canvas.width as usize] != first
                            || canvas.pixels[i + canvas.width as usize + 1] != first
                    })
                }));
            }
            let before = canvas.pixels.clone();
            canvas.text(&mut fonts, i32::MIN, i32::MIN, text, 17.0, 0);
            canvas.text(&mut fonts, i32::MAX, i32::MAX, text, 17.0, 0);
            assert_eq!(canvas.pixels, before);
            canvas.text(&mut fonts, -8, -8, text, 32.0, 0);
        }
        // The largest allowed logical font at the largest output scale remains
        // a 768px raster request, without expanding the retained cache budget.
        let mut maximum = Canvas::new_scaled(8, 8, 0xffffff, 3.0);
        let alphabet: String = (b'!'..=b'~').map(char::from).collect();
        maximum.text(&mut fonts, 0, 0, &alphabet, 256.0, 0);
        assert!(
            fonts
                .cache
                .keys()
                .any(|(_, bits)| *bits == 768.0f32.to_bits())
        );
        assert!(fonts.cached_bytes <= GLYPH_CACHE_BYTES);
        assert!(fonts.cache.len() <= GLYPH_CACHE_ENTRIES);
    }

    #[test]
    fn font_measurement_scales_and_paint_is_clipped() {
        let mut fonts = Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .expect("install DejaVu Sans or set MGBROWSER_FONT");
        let width = fonts.width("Hello, browser!", 16.0);
        assert!(width > 50.0 && width < 250.0);
        assert!((fonts.width("Hello, browser!", 32.0) - 2.0 * width).abs() < 0.01);
        assert_eq!(fonts.width("", 16.0), 0.0);
        let mut canvas = Canvas::new(220, 50, 0xffffff);
        canvas.text(&mut fonts, 5, 3, "Hello, browser!", 16.0, 0);
        assert!(canvas.pixels.iter().any(|&pixel| pixel != 0xffffff));
        assert!(
            canvas.pixels[..3 * 220]
                .iter()
                .all(|&pixel| pixel == 0xffffff)
        );
        let before = canvas.pixels.clone();
        canvas.text(&mut fonts, i32::MIN, i32::MIN, "clipped", 16.0, 0);
        canvas.text(&mut fonts, i32::MAX, i32::MAX, "clipped", 16.0, 0);
        assert_eq!(canvas.pixels, before);
        // Painting a partly visible glyph at a negative origin must also clip.
        canvas.text(&mut fonts, -8, -8, "Overflow", 32.0, 0);
    }
}
