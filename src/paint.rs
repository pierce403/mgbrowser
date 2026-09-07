//! Rust software painting. Pixels are packed 0x00RRGGBB for the window surface.
//!
//! Font files are data: shaping and rasterization never call a native font API.
//! A single face is used for now; mixed-direction bidi layout and fallback faces
//! are not implemented. Callers split document text into lines before painting.

use std::collections::HashMap;

const GLYPH_CACHE_BYTES: usize = 8 * 1024 * 1024;
const GLYPH_CACHE_ENTRIES: usize = 4096;

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, bg: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![bg & 0x00ff_ffff; width as usize * height as usize],
        }
    }

    /// Fill the intersection with the canvas, including negative coordinates.
    pub fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        let left = i64::from(x).clamp(0, i64::from(self.width)) as usize;
        let top = i64::from(y).clamp(0, i64::from(self.height)) as usize;
        let right = (i64::from(x) + i64::from(w)).clamp(0, i64::from(self.width)) as usize;
        let bottom = (i64::from(y) + i64::from(h)).clamp(0, i64::from(self.height)) as usize;
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
            let cached = fonts.glyph(glyph.id, size);
            let metrics = cached.0;
            let left = (pen + glyph.x_offset).round() as i64 + i64::from(metrics.xmin);
            let top = (baseline - glyph.y_offset).round() as i64
                - i64::from(metrics.ymin)
                - metrics.height as i64;
            let from_x = left.max(0).min(i64::from(self.width));
            let from_y = top.max(0).min(i64::from(self.height));
            let to_x = (left + metrics.width as i64)
                .max(0)
                .min(i64::from(self.width));
            let to_y = (top + metrics.height as i64)
                .max(0)
                .min(i64::from(self.height));
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
}

impl Fonts {
    pub fn load() -> Result<Self, String> {
        if let Some(path) = std::env::var_os("MGBROWSER_FONT") {
            return Self::from_path(std::path::Path::new(&path));
        }
        let paths = [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        ];
        let mut failures = Vec::new();
        for path in paths {
            match Self::from_path(std::path::Path::new(path)) {
                Ok(fonts) => return Ok(fonts),
                Err(error) => failures.push(error),
            }
        }
        Err(format!(
            "No usable font found. Set MGBROWSER_FONT to a TrueType/OpenType font file. {}",
            failures.join("; ")
        ))
    }

    fn from_path(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("Cannot read font {}: {error}", path.display()))?;
        let font = fontdue::Font::from_bytes(&*bytes, fontdue::FontSettings::default())
            .map_err(|error| format!("Cannot rasterize font {}: {error}", path.display()))?;
        rustybuzz::Face::from_slice(&bytes, 0)
            .ok_or_else(|| format!("Cannot shape font {}", path.display()))?;
        Ok(Self {
            bytes,
            font,
            cache: HashMap::new(),
            cached_bytes: 0,
        })
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
    fn font_measurement_scales_and_paint_is_clipped() {
        let mut fonts = Fonts::load().expect("install DejaVu Sans or set MGBROWSER_FONT");
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
