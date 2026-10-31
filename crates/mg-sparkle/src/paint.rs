//! Rust software painting. Pixels are packed 0x00RRGGBB for the window surface.
//!
//! Font files are data: shaping and rasterization never call a native font API.
//! A host may supply a separate bold face; mixed-direction bidi layout and
//! script fallback faces are not implemented. Callers split text into lines.

use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};

const GLYPH_CACHE_BYTES: usize = 8 * 1024 * 1024;
const GLYPH_CACHE_ENTRIES: usize = 4096;
const SHAPE_CACHE_BYTES: usize = 1024 * 1024;
const SHAPE_CACHE_ENTRIES: usize = 4096;

/// Actual shaping/measurement lookups, summed over all faces supplied to Fonts.
/// Payload counts retained text/glyph data, not allocator overhead or process RSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShapingCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
    pub payload_bytes: usize,
}

impl ShapingCacheStats {
    fn add(&mut self, other: Self) {
        self.hits = self.hits.saturating_add(other.hits);
        self.misses = self.misses.saturating_add(other.misses);
        self.evictions = self.evictions.saturating_add(other.evictions);
        self.entries = self.entries.saturating_add(other.entries);
        self.payload_bytes = self.payload_bytes.saturating_add(other.payload_bytes);
    }
}

/// Resolved rounded rectangle in logical or physical coordinates.
/// Corner order is `[top-left, top-right, bottom-right, bottom-left]` as `(rx, ry)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub radii: [(f32, f32); 4],
}

const SUBSAMPLES_4X4: [f32; 4] = [0.125, 0.375, 0.625, 0.875];

impl RoundedRect {
    /// Resolve corner radii per CSS Backgrounds 3 §4.1 and §4.5 (clamping negatives,
    /// zeroing corners where either axis is zero, and scaling all radii by `f` when
    /// adjacent corners would overlap box sides).
    pub fn from_raw(x: f32, y: f32, w: f32, h: f32, raw_radii: [(f32, f32); 4]) -> Self {
        let x = if x.is_finite() { x } else { 0.0 };
        let y = if y.is_finite() { y } else { 0.0 };
        let w = if w.is_finite() { w.max(0.0) } else { 0.0 };
        let h = if h.is_finite() { h.max(0.0) } else { 0.0 };
        if w <= 0.0 || h <= 0.0 {
            return Self {
                x,
                y,
                w,
                h,
                radii: [(0.0, 0.0); 4],
            };
        }
        let mut radii = raw_radii.map(|(rx, ry)| {
            let rx = if rx.is_finite() && rx > 0.0 { rx } else { 0.0 };
            let ry = if ry.is_finite() && ry > 0.0 { ry } else { 0.0 };
            if rx == 0.0 || ry == 0.0 {
                (0.0, 0.0)
            } else {
                (rx, ry)
            }
        });
        let s_top = radii[0].0 + radii[1].0;
        let s_bottom = radii[3].0 + radii[2].0;
        let s_left = radii[0].1 + radii[3].1;
        let s_right = radii[1].1 + radii[2].1;
        let mut f = 1.0_f32;
        for (len, sum) in [(w, s_top), (w, s_bottom), (h, s_left), (h, s_right)] {
            if sum > len && sum > 0.0 {
                f = f.min(len / sum);
            }
        }
        if f < 1.0 {
            radii = radii.map(|(rx, ry)| {
                let srx = rx * f;
                let sry = ry * f;
                if srx <= 0.0 || sry <= 0.0 {
                    (0.0, 0.0)
                } else {
                    (srx, sry)
                }
            });
        }
        Self { x, y, w, h, radii }
    }

    pub fn has_any_radius(&self) -> bool {
        self.radii.iter().any(|&(rx, ry)| rx > 0.0 && ry > 0.0)
    }

    /// Inset by `[top, right, bottom, left]` per CSS Backgrounds 3 §4.3.
    pub fn inset(&self, insets: [f32; 4]) -> Self {
        let [top, right, bottom, left] =
            insets.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
        let h_sum = left + right;
        let (nx, nw) = if h_sum <= self.w {
            (self.x + left, (self.w - h_sum).max(0.0))
        } else if h_sum > 0.0 {
            (self.x + self.w * (left / h_sum), 0.0)
        } else {
            (self.x, 0.0)
        };
        let v_sum = top + bottom;
        let (ny, nh) = if v_sum <= self.h {
            (self.y + top, (self.h - v_sum).max(0.0))
        } else if v_sum > 0.0 {
            (self.y + self.h * (top / v_sum), 0.0)
        } else {
            (self.y, 0.0)
        };
        let inner = [
            (self.radii[0].0 - left, self.radii[0].1 - top),
            (self.radii[1].0 - right, self.radii[1].1 - top),
            (self.radii[2].0 - right, self.radii[2].1 - bottom),
            (self.radii[3].0 - left, self.radii[3].1 - bottom),
        ];
        Self::from_raw(nx, ny, nw, nh, inner)
    }

    /// Snap logical box edges to physical canvas coordinates and scale radii.
    pub fn to_physical(&self, canvas: &Canvas, scroll: i32) -> Self {
        let x0 = canvas.physical_edge(self.x.round() as i64) as f32;
        let y0 = canvas.physical_edge(self.y.round() as i64 - i64::from(scroll)) as f32;
        let x1 = canvas.physical_edge(self.x.round() as i64 + self.w.ceil().max(0.0) as i64) as f32;
        let y1 = canvas.physical_edge(self.y.round() as i64 + self.h.ceil().max(0.0) as i64) as f32;
        let scale = canvas.scale();
        let raw_radii = self.radii.map(|(rx, ry)| (rx * scale, ry * scale));
        Self::from_raw(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0), raw_radii)
    }

    #[inline]
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        let x0 = self.x;
        let y0 = self.y;
        let x1 = x0 + self.w;
        let y1 = y0 + self.h;
        if px < x0 || px >= x1 || py < y0 || py >= y1 {
            return false;
        }
        let [(tl_x, tl_y), (tr_x, tr_y), (br_x, br_y), (bl_x, bl_y)] = self.radii;
        if tl_x > 0.0 && tl_y > 0.0 {
            let cx = x0 + tl_x;
            let cy = y0 + tl_y;
            if px < cx && py < cy {
                let dx = (px - cx) / tl_x;
                let dy = (py - cy) / tl_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        if tr_x > 0.0 && tr_y > 0.0 {
            let cx = x1 - tr_x;
            let cy = y0 + tr_y;
            if px > cx && py < cy {
                let dx = (px - cx) / tr_x;
                let dy = (py - cy) / tr_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        if br_x > 0.0 && br_y > 0.0 {
            let cx = x1 - br_x;
            let cy = y1 - br_y;
            if px > cx && py > cy {
                let dx = (px - cx) / br_x;
                let dy = (py - cy) / br_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        if bl_x > 0.0 && bl_y > 0.0 {
            let cx = x0 + bl_x;
            let cy = y1 - bl_y;
            if px < cx && py > cy {
                let dx = (px - cx) / bl_x;
                let dy = (py - cy) / bl_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        true
    }

    #[inline]
    pub fn pixel_is_wholly_outside(&self, sx: u32, sy: u32) -> bool {
        let px0 = sx as f32;
        let py0 = sy as f32;
        let px1 = px0 + 1.0;
        let py1 = py0 + 1.0;
        let x0 = self.x;
        let y0 = self.y;
        let x1 = x0 + self.w;
        let y1 = y0 + self.h;
        if px1 <= x0 || px0 >= x1 || py1 <= y0 || py0 >= y1 {
            return true;
        }
        let [(tl_x, tl_y), (tr_x, tr_y), (br_x, br_y), (bl_x, bl_y)] = self.radii;
        if tl_x > 0.0 && tl_y > 0.0 {
            let cx = x0 + tl_x;
            let cy = y0 + tl_y;
            if px1 <= cx && py1 <= cy {
                let dx = (px1 - cx) / tl_x;
                let dy = (py1 - cy) / tl_y;
                if dx * dx + dy * dy > 1.0 {
                    return true;
                }
            }
        }
        if tr_x > 0.0 && tr_y > 0.0 {
            let cx = x1 - tr_x;
            let cy = y0 + tr_y;
            if px0 >= cx && py1 <= cy {
                let dx = (px0 - cx) / tr_x;
                let dy = (py1 - cy) / tr_y;
                if dx * dx + dy * dy > 1.0 {
                    return true;
                }
            }
        }
        if br_x > 0.0 && br_y > 0.0 {
            let cx = x1 - br_x;
            let cy = y1 - br_y;
            if px0 >= cx && py0 >= cy {
                let dx = (px0 - cx) / br_x;
                let dy = (py0 - cy) / br_y;
                if dx * dx + dy * dy > 1.0 {
                    return true;
                }
            }
        }
        if bl_x > 0.0 && bl_y > 0.0 {
            let cx = x0 + bl_x;
            let cy = y1 - bl_y;
            if px1 <= cx && py0 >= cy {
                let dx = (px1 - cx) / bl_x;
                let dy = (py0 - cy) / bl_y;
                if dx * dx + dy * dy > 1.0 {
                    return true;
                }
            }
        }
        false
    }

    #[inline]
    pub fn pixel_is_wholly_inside(&self, sx: u32, sy: u32) -> bool {
        let px0 = sx as f32;
        let py0 = sy as f32;
        let px1 = px0 + 1.0;
        let py1 = py0 + 1.0;
        let x0 = self.x;
        let y0 = self.y;
        let x1 = x0 + self.w;
        let y1 = y0 + self.h;
        if px0 < x0 || px1 > x1 || py0 < y0 || py1 > y1 {
            return false;
        }
        let [(tl_x, tl_y), (tr_x, tr_y), (br_x, br_y), (bl_x, bl_y)] = self.radii;
        if tl_x > 0.0 && tl_y > 0.0 {
            let cx = x0 + tl_x;
            let cy = y0 + tl_y;
            if px0 < cx && py0 < cy {
                let dx = (px0 - cx) / tl_x;
                let dy = (py0 - cy) / tl_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        if tr_x > 0.0 && tr_y > 0.0 {
            let cx = x1 - tr_x;
            let cy = y0 + tr_y;
            if px1 > cx && py0 < cy {
                let dx = (px1 - cx) / tr_x;
                let dy = (py0 - cy) / tr_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        if br_x > 0.0 && br_y > 0.0 {
            let cx = x1 - br_x;
            let cy = y1 - br_y;
            if px1 > cx && py1 > cy {
                let dx = (px1 - cx) / br_x;
                let dy = (py1 - cy) / br_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        if bl_x > 0.0 && bl_y > 0.0 {
            let cx = x0 + bl_x;
            let cy = y1 - bl_y;
            if px0 < cx && py1 > cy {
                let dx = (px0 - cx) / bl_x;
                let dy = (py1 - cy) / bl_y;
                if dx * dx + dy * dy > 1.0 {
                    return false;
                }
            }
        }
        true
    }
}

/// Opaque saved physical clip. Callers own nesting tokens; Canvas allocates no
/// clip stack when rectangular. Restore on the same surface to recover bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasClip {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    rounded: Vec<RoundedRect>,
}

#[derive(Clone)]
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
                rounded: Vec::new(),
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
        let previous = self.clip.clone();
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
        self.clip.left = left;
        self.clip.top = top;
        self.clip.right = right;
        self.clip.bottom = bottom;
        previous
    }

    /// Intersect the active clip with an already-physical rounded rectangle.
    pub fn intersect_rounded_clip(&mut self, phys_rect: RoundedRect) -> CanvasClip {
        let previous = self.clip.clone();
        let clamp_x = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, self.width as f32) as u32
            } else {
                0
            }
        };
        let clamp_y = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, self.height as f32) as u32
            } else {
                0
            }
        };
        let rx0 = clamp_x(phys_rect.x.floor());
        let ry0 = clamp_y(phys_rect.y.floor());
        let rx1 = clamp_x((phys_rect.x + phys_rect.w).ceil());
        let ry1 = clamp_y((phys_rect.y + phys_rect.h).ceil());
        self.clip.left = previous.left.max(rx0);
        self.clip.top = previous.top.max(ry0);
        self.clip.right = previous.right.min(rx1).max(self.clip.left);
        self.clip.bottom = previous.bottom.min(ry1).max(self.clip.top);
        if phys_rect.has_any_radius() {
            self.clip.rounded.push(phys_rect);
        }
        previous
    }

    /// Restore a saved clip without reconverting logical edges.
    /// A token from another surface is clamped to this canvas for safe bounds.
    pub fn restore_clip(&mut self, clip: CanvasClip) {
        self.clip = CanvasClip {
            left: clip.left.min(self.width),
            top: clip.top.min(self.height),
            right: clip.right.min(self.width),
            bottom: clip.bottom.min(self.height),
            rounded: clip.rounded,
        };
    }

    #[inline]
    pub fn has_rounded_clip(&self) -> bool {
        !self.clip.rounded.is_empty()
    }

    /// 0..=255 coverage of physical pixel `(sx, sy)` against active rounded clips.
    #[inline]
    pub fn rounded_clip_coverage(&self, sx: u32, sy: u32) -> u8 {
        if self.clip.rounded.is_empty() {
            return 255;
        }
        let mut all_inside = true;
        for r in &self.clip.rounded {
            if r.pixel_is_wholly_outside(sx, sy) {
                return 0;
            }
            if !r.pixel_is_wholly_inside(sx, sy) {
                all_inside = false;
            }
        }
        if all_inside {
            return 255;
        }
        let mut hits = 0u32;
        for dy in SUBSAMPLES_4X4 {
            let py = sy as f32 + dy;
            for dx in SUBSAMPLES_4X4 {
                let px = sx as f32 + dx;
                if self.clip.rounded.iter().all(|r| r.contains_point(px, py)) {
                    hits += 1;
                }
            }
        }
        ((hits * 255 + 8) / 16) as u8
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
        if !self.has_rounded_clip() {
            for row in top..bottom {
                let start = row * self.width as usize;
                self.pixels[start + left..start + right].fill(color);
            }
        } else {
            let width = self.width as usize;
            for row in top..bottom {
                let start = row * width;
                for col in left..right {
                    let cov = self.rounded_clip_coverage(col as u32, row as u32);
                    if cov == 255 {
                        self.pixels[start + col] = color;
                    } else if cov > 0 {
                        let dst = self.pixels[start + col];
                        self.pixels[start + col] = blend(dst, color, cov);
                    }
                }
            }
        }
    }

    /// Paint a rounded background color and/or 4-side rounded border in a single
    /// 4x4 supersampled pass over physical coordinates, preventing outer background
    /// bleed under opaque borders and inner border-to-background AA seams.
    pub fn paint_rounded_decoration(
        &mut self,
        outer: RoundedRect,
        inner: RoundedRect,
        bg: Option<(RoundedRect, u32, f32)>,
        borders: [(u32, f32); 4],
    ) {
        if outer.w <= 0.0 || outer.h <= 0.0 {
            return;
        }
        let has_bg = bg.is_some_and(|(r, _, a)| r.w > 0.0 && r.h > 0.0 && a > 0.0);
        let has_border = borders.iter().any(|&(_, a)| a > 0.0)
            && (inner.x > outer.x
                || inner.y > outer.y
                || inner.x + inner.w < outer.x + outer.w
                || inner.y + inner.h < outer.y + outer.h);
        if !has_bg && !has_border {
            return;
        }
        let clamp_x = |v: f32| v.clamp(self.clip.left as f32, self.clip.right as f32) as usize;
        let clamp_y = |v: f32| v.clamp(self.clip.top as f32, self.clip.bottom as f32) as usize;
        let left = clamp_x(outer.x.floor());
        let top = clamp_y(outer.y.floor());
        let right = clamp_x((outer.x + outer.w).ceil());
        let bottom = clamp_y((outer.y + outer.h).ceil());
        if left >= right || top >= bottom {
            return;
        }
        let pre_color = |rgb: u32, a: f32| -> ([f32; 3], f32) {
            let ca = a.clamp(0.0, 1.0);
            (
                [
                    ((rgb >> 16) & 255) as f32 * ca,
                    ((rgb >> 8) & 255) as f32 * ca,
                    (rgb & 255) as f32 * ca,
                ],
                ca,
            )
        };
        let bg_pre = bg.and_then(|(r, rgb, a)| {
            let ca = a.clamp(0.0, 1.0);
            (ca > 0.0 && r.w > 0.0 && r.h > 0.0).then(|| (r, rgb & 0x00ff_ffff, pre_color(rgb, ca)))
        });
        let border_pre = borders.map(|(rgb, a)| pre_color(rgb, a));
        let has_rclip = self.has_rounded_clip();
        let stride = self.width as usize;

        for sy in top..bottom {
            let row_start = sy * stride;
            for sx in left..right {
                let ux = sx as u32;
                let uy = sy as u32;
                if outer.pixel_is_wholly_outside(ux, uy) {
                    continue;
                }
                let clip_cov = if has_rclip {
                    let c = self.rounded_clip_coverage(ux, uy);
                    if c == 0 {
                        continue;
                    }
                    c
                } else {
                    255
                };
                // Fast solid interior path when pixel is strictly inside padding box.
                if (!has_border && outer.pixel_is_wholly_inside(ux, uy))
                    || inner.pixel_is_wholly_inside(ux, uy)
                {
                    if let Some((bg_r, bg_rgb, (bg_pm, bg_a))) = bg_pre {
                        if bg_r.pixel_is_wholly_inside(ux, uy) {
                            let eff_a = bg_a * (f32::from(clip_cov) / 255.0);
                            if eff_a >= 1.0 {
                                self.pixels[row_start + sx] = bg_rgb;
                            } else if eff_a > 0.0 {
                                let scale = f32::from(clip_cov) / 255.0;
                                let dst = self.pixels[row_start + sx];
                                let inv = 1.0 - eff_a;
                                let r = (bg_pm[0] * scale + ((dst >> 16) & 255) as f32 * inv)
                                    .round()
                                    .clamp(0.0, 255.0)
                                    as u32;
                                let g = (bg_pm[1] * scale + ((dst >> 8) & 255) as f32 * inv)
                                    .round()
                                    .clamp(0.0, 255.0)
                                    as u32;
                                let b = (bg_pm[2] * scale + (dst & 255) as f32 * inv)
                                    .round()
                                    .clamp(0.0, 255.0)
                                    as u32;
                                self.pixels[row_start + sx] = (r << 16) | (g << 8) | b;
                            }
                            continue;
                        } else if bg_r.pixel_is_wholly_outside(ux, uy) {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                let mut sum_r = 0.0_f32;
                let mut sum_g = 0.0_f32;
                let mut sum_b = 0.0_f32;
                let mut sum_a = 0.0_f32;
                for dy in SUBSAMPLES_4X4 {
                    let py = sy as f32 + dy;
                    for dx in SUBSAMPLES_4X4 {
                        let px = sx as f32 + dx;
                        if !outer.contains_point(px, py) {
                            continue;
                        }
                        if has_border && !inner.contains_point(px, py) {
                            let side = border_side_for_point(px, py, &outer, &inner);
                            let (b_pm, b_a) = border_pre[side];
                            if b_a >= 1.0 {
                                sum_r += b_pm[0];
                                sum_g += b_pm[1];
                                sum_b += b_pm[2];
                                sum_a += 1.0;
                            } else if let Some((bg_r, _, (bg_pm, bg_a))) = bg_pre
                                && bg_r.contains_point(px, py)
                            {
                                let rem = 1.0 - b_a;
                                sum_r += b_pm[0] + bg_pm[0] * rem;
                                sum_g += b_pm[1] + bg_pm[1] * rem;
                                sum_b += b_pm[2] + bg_pm[2] * rem;
                                sum_a += b_a + bg_a * rem;
                            } else {
                                sum_r += b_pm[0];
                                sum_g += b_pm[1];
                                sum_b += b_pm[2];
                                sum_a += b_a;
                            }
                        } else if let Some((bg_r, _, (bg_pm, bg_a))) = bg_pre
                            && bg_r.contains_point(px, py)
                        {
                            sum_r += bg_pm[0];
                            sum_g += bg_pm[1];
                            sum_b += bg_pm[2];
                            sum_a += bg_a;
                        }
                    }
                }
                if sum_a <= 0.0 {
                    continue;
                }
                let scale = (f32::from(clip_cov) / 255.0) * (1.0 / 16.0);
                let out_a = (sum_a * scale).clamp(0.0, 1.0);
                let dst = self.pixels[row_start + sx];
                let inv = 1.0 - out_a;
                let r = (sum_r * scale + ((dst >> 16) & 255) as f32 * inv)
                    .round()
                    .clamp(0.0, 255.0) as u32;
                let g = (sum_g * scale + ((dst >> 8) & 255) as f32 * inv)
                    .round()
                    .clamp(0.0, 255.0) as u32;
                let b = (sum_b * scale + (dst & 255) as f32 * inv)
                    .round()
                    .clamp(0.0, 255.0) as u32;
                self.pixels[row_start + sx] = (r << 16) | (g << 8) | b;
            }
        }
    }

    /// Paint a shaped, single line; `y` is the top of its font line box.
    pub fn text(&mut self, fonts: &mut Fonts, x: i32, y: i32, text: &str, size: f32, color: u32) {
        self.text_alpha(fonts, x, y, text, size, color, 1.0);
    }

    pub fn text_alpha(
        &mut self,
        fonts: &mut Fonts,
        x: i32,
        y: i32,
        text: &str,
        size: f32,
        color: u32,
        alpha: f32,
    ) {
        self.text_face_alpha(fonts, (x, y), text, size, color, false, false, alpha);
    }

    #[allow(clippy::too_many_arguments)]
    fn text_face_alpha(
        &mut self,
        fonts: &mut Fonts,
        (x, y): (i32, i32),
        text: &str,
        size: f32,
        color: u32,
        synth_bold: bool,
        synth_italic: bool,
        alpha: f32,
    ) {
        if alpha <= 0.0 {
            return;
        }
        let size = font_size(size);
        let Some(ref font_face) = fonts.font else {
            return;
        };
        let ascent = font_face
            .horizontal_line_metrics(size)
            .map_or(size, |metrics| metrics.ascent);
        let baseline = y as f32 + ascent;
        let has_rclip = self.has_rounded_clip();
        let mut pen = x as f32;
        for glyph in fonts.shape(text, size).iter() {
            // Shape and advance in logical units, but rasterize the outline at
            // the actual output resolution. Do not enlarge a 1x glyph bitmap.
            let cached = fonts.glyph(glyph.id, size * self.scale);
            let metrics = cached.0;
            let baseline_phys = (baseline - glyph.y_offset) * self.scale;
            let left =
                ((pen + glyph.x_offset) * self.scale).round() as i64 + i64::from(metrics.xmin);
            let top =
                baseline_phys.round() as i64 - i64::from(metrics.ymin) - metrics.height as i64;
            let from_y = top.clamp(i64::from(self.clip.top), i64::from(self.clip.bottom));
            let to_y = (top + metrics.height as i64)
                .clamp(i64::from(self.clip.top), i64::from(self.clip.bottom));
            if !synth_bold && !synth_italic {
                let from_x = left.clamp(i64::from(self.clip.left), i64::from(self.clip.right));
                let to_x = (left + metrics.width as i64)
                    .clamp(i64::from(self.clip.left), i64::from(self.clip.right));
                for screen_y in from_y..to_y {
                    let source_row = (screen_y - top) as usize * metrics.width;
                    let target_row = screen_y as usize * self.width as usize;
                    for screen_x in from_x..to_x {
                        let raw = cached.1[source_row + (screen_x - left) as usize];
                        let mut coverage = if alpha >= 1.0 {
                            raw
                        } else {
                            (f32::from(raw) * alpha.clamp(0.0, 1.0)).round() as u8
                        };
                        if has_rclip {
                            if coverage == 0 {
                                continue;
                            }
                            let rcov = self.rounded_clip_coverage(screen_x as u32, screen_y as u32);
                            if rcov == 0 {
                                continue;
                            }
                            if rcov < 255 {
                                coverage =
                                    ((u32::from(coverage) * u32::from(rcov) + 127) / 255) as u8;
                            }
                        }
                        let target = &mut self.pixels[target_row + screen_x as usize];
                        *target = blend(*target, color, coverage);
                    }
                }
            } else {
                let extra_bold = if synth_bold {
                    ((size * self.scale) / 24.0).round().clamp(1.0, 3.0) as i64
                } else {
                    0
                };
                let left_base = left - extra_bold / 2;
                let width_i64 = metrics.width as i64;
                for screen_y in from_y..to_y {
                    let source_row = (screen_y - top) as usize * metrics.width;
                    let target_row = screen_y as usize * self.width as usize;
                    let shear_x = if synth_italic {
                        0.20 * (baseline_phys - (screen_y as f32 + 0.5))
                    } else {
                        0.0
                    };
                    let row_from_x = (left_base + shear_x.floor() as i64)
                        .clamp(i64::from(self.clip.left), i64::from(self.clip.right));
                    let row_to_x = (left_base + width_i64 + extra_bold + shear_x.ceil() as i64 + 1)
                        .clamp(i64::from(self.clip.left), i64::from(self.clip.right));
                    let dilated = |k: i64| -> u8 {
                        let mut m = 0u8;
                        for d in 0..=extra_bold {
                            let sx = k - d;
                            if sx >= 0 && sx < width_i64 {
                                m = m.max(cached.1[source_row + sx as usize]);
                            }
                        }
                        m
                    };
                    for screen_x in row_from_x..row_to_x {
                        let raw = if synth_italic {
                            let u = (screen_x - left_base) as f32 - shear_x;
                            let u0 = u.floor();
                            let frac = u - u0;
                            let k0 = u0 as i64;
                            (f32::from(dilated(k0)) * (1.0 - frac)
                                + f32::from(dilated(k0 + 1)) * frac)
                                .round() as u8
                        } else {
                            dilated(screen_x - left_base)
                        };
                        if raw == 0 {
                            continue;
                        }
                        let mut coverage = if alpha >= 1.0 {
                            raw
                        } else {
                            (f32::from(raw) * alpha.clamp(0.0, 1.0)).round() as u8
                        };
                        if has_rclip {
                            let rcov = self.rounded_clip_coverage(screen_x as u32, screen_y as u32);
                            if rcov == 0 {
                                continue;
                            }
                            if rcov < 255 {
                                coverage =
                                    ((u32::from(coverage) * u32::from(rcov) + 127) / 255) as u8;
                            }
                        }
                        let target = &mut self.pixels[target_row + screen_x as usize];
                        *target = blend(*target, color, coverage);
                    }
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
        self.text_weight_alpha(fonts, position, text, size, color, bold, 1.0);
    }

    pub fn text_weight_alpha(
        &mut self,
        fonts: &mut Fonts,
        position: (i32, i32),
        text: &str,
        size: f32,
        color: u32,
        bold: bool,
        alpha: f32,
    ) {
        self.text_styled_alpha(fonts, position, text, size, color, bold, false, alpha);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_styled_alpha(
        &mut self,
        fonts: &mut Fonts,
        position: (i32, i32),
        text: &str,
        size: f32,
        color: u32,
        bold: bool,
        italic: bool,
        alpha: f32,
    ) {
        if bold && let Some(face) = fonts.bold.as_deref_mut() {
            let synth_italic = italic && face.synthetic_styles;
            self.text_face_alpha(
                face,
                position,
                text,
                size,
                color,
                false,
                synth_italic,
                alpha,
            );
        } else {
            let synth_bold = bold && fonts.synthetic_styles;
            let synth_italic = italic && fonts.synthetic_styles;
            self.text_face_alpha(
                fonts,
                position,
                text,
                size,
                color,
                synth_bold,
                synth_italic,
                alpha,
            );
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

#[inline]
fn border_side_for_point(px: f32, py: f32, outer: &RoundedRect, inner: &RoundedRect) -> usize {
    let ox0 = outer.x;
    let oy0 = outer.y;
    let ox1 = ox0 + outer.w;
    let oy1 = oy0 + outer.h;
    let ix0 = inner.x;
    let iy0 = inner.y;
    let ix1 = ix0 + inner.w;
    let iy1 = iy0 + inner.h;
    let [
        (itl_x, itl_y),
        (itr_x, itr_y),
        (ibr_x, ibr_y),
        (ibl_x, ibl_y),
    ] = inner.radii;
    if px < ix0 + itl_x && py < iy0 + itl_y {
        let icx = ix0 + itl_x;
        let icy = iy0 + itl_y;
        return if (icx - px) * (icy - oy0) <= (icy - py) * (icx - ox0) {
            0
        } else {
            3
        };
    }
    if px >= ix1 - itr_x && py < iy0 + itr_y {
        let icx = ix1 - itr_x;
        let icy = iy0 + itr_y;
        return if (px - icx) * (icy - oy0) <= (icy - py) * (ox1 - icx) {
            0
        } else {
            1
        };
    }
    if px >= ix1 - ibr_x && py >= iy1 - ibr_y {
        let icx = ix1 - ibr_x;
        let icy = iy1 - ibr_y;
        return if (px - icx) * (oy1 - icy) <= (py - icy) * (ox1 - icx) {
            2
        } else {
            1
        };
    }
    if px < ix0 + ibl_x && py >= iy1 - ibl_y {
        let icx = ix0 + ibl_x;
        let icy = iy1 - ibl_y;
        return if (icx - px) * (oy1 - icy) <= (py - icy) * (icx - ox0) {
            2
        } else {
            3
        };
    }
    if py < iy0 {
        0
    } else if px >= ix1 {
        1
    } else if py >= iy1 {
        2
    } else {
        3
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
    font: Option<fontdue::Font>,
    cache: HashMap<(u16, u32), (fontdue::Metrics, Vec<u8>)>,
    cached_bytes: usize,
    shape_cache: HashMap<(u64, u32), (Box<str>, f32, Rc<[PositionedGlyph]>)>,
    shape_cached_bytes: usize,
    shaping_stats: ShapingCacheStats,
    synthetic_styles: bool,
    bold: Option<Box<Fonts>>,
    families: HashMap<String, Box<Fonts>>,
}

impl Fonts {
    /// Lightweight fallback text-metric provider for worker-side DOM geometry queries
    /// where no font bytes or filesystem access exist.
    pub(crate) fn fallback_metrics() -> Self {
        Self {
            bytes: Vec::new(),
            font: None,
            cache: HashMap::new(),
            cached_bytes: 0,
            shape_cache: HashMap::new(),
            shape_cached_bytes: 0,
            shaping_stats: Default::default(),
            synthetic_styles: false,
            bold: None,
            families: HashMap::new(),
        }
    }

    /// Parse caller-owned font data without selecting a platform font or doing I/O.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let bytes = crate::woff::decode_font_bytes(bytes)?;
        let font = fontdue::Font::from_bytes(&*bytes, fontdue::FontSettings::default())
            .map_err(|error| format!("Cannot rasterize font: {error}"))?;
        rustybuzz::Face::from_slice(&bytes, 0).ok_or_else(|| "Cannot shape font".to_owned())?;
        Ok(Self {
            bytes,
            font: Some(font),
            cache: HashMap::new(),
            cached_bytes: 0,
            shape_cache: HashMap::new(),
            shape_cached_bytes: 0,
            shaping_stats: Default::default(),
            synthetic_styles: false,
            bold: None,
            families: HashMap::new(),
        })
    }

    /// Supply a separately parsed bold font without performing platform I/O.
    pub fn from_bytes_with_bold(bytes: Vec<u8>, bold: Option<Vec<u8>>) -> Result<Self, String> {
        let mut fonts = Self::from_bytes(bytes)?;
        fonts.bold = bold.map(Self::from_bytes).transpose()?.map(Box::new);
        Ok(fonts)
    }

    pub fn clear_families(&mut self) {
        // Keep lookup totals when pass-local downloaded font faces are retired.
        for font in self.families.values() {
            let mut stats = font.shaping_cache_stats();
            stats.evictions = stats.evictions.saturating_add(stats.entries as u64);
            stats.entries = 0;
            stats.payload_bytes = 0;
            self.shaping_stats.add(stats);
        }
        self.families.clear();
    }

    /// Counters cover this Fonts lifetime, including retired downloaded faces.
    /// Retained entries/payload cover only the currently loaded faces.
    pub fn shaping_cache_stats(&self) -> ShapingCacheStats {
        let mut stats = self.shaping_stats;
        stats.entries = self.shape_cache.len();
        stats.payload_bytes = self.shape_cached_bytes;
        if let Some(bold) = &self.bold {
            stats.add(bold.shaping_cache_stats());
        }
        for font in self.families.values() {
            stats.add(font.shaping_cache_stats());
        }
        stats
    }

    pub fn register_family(&mut self, name: &str, bytes: Vec<u8>) {
        if self.families.len() >= 8 {
            return;
        }
        let key = name.trim().to_ascii_lowercase();
        if key.is_empty() || self.families.contains_key(&key) {
            return;
        }
        if let Ok(mut font) = Self::from_bytes(bytes) {
            font.synthetic_styles = true;
            self.families.insert(key, Box::new(font));
        }
    }

    pub fn select_family(&mut self, families: &[String]) -> &mut Fonts {
        for family in families {
            let key = family.trim().to_ascii_lowercase();
            if self.families.contains_key(&key) {
                return self.families.get_mut(&key).unwrap();
            }
        }
        self
    }

    pub fn select_family_ref(&self, families: &[String]) -> &Fonts {
        for family in families {
            let key = family.trim().to_ascii_lowercase();
            if let Some(font) = self.families.get(&key) {
                return font;
            }
        }
        self
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
        let size = font_size(size);
        if self.font.is_none() {
            return (text.chars().count() as f32 * size * 0.6).max(0.0);
        }
        if text.len() <= 256 {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            let key = (hasher.finish(), size.to_bits());
            if let Some((cached_text, width, _)) = self.shape_cache.get(&key)
                && &**cached_text == text
            {
                self.shaping_stats.hits = self.shaping_stats.hits.saturating_add(1);
                return *width;
            }
        }
        self.shape(text, size)
            .iter()
            .map(|glyph| glyph.advance)
            .sum::<f32>()
            .max(0.0)
    }

    pub fn line_height(&self, size: f32) -> i32 {
        let size = font_size(size);
        self.font
            .as_ref()
            .and_then(|f| f.horizontal_line_metrics(size))
            .map_or(size * 1.25, |metrics| metrics.new_line_size)
            .ceil() as i32
    }

    /// Logical-pixel-aligned normal CSS line metrics. Round ascent and descent
    /// separately, retaining a stable baseline independent of output scale.
    pub fn css_line_height(&self, size: f32) -> f32 {
        let size = font_size(size);
        self.font
            .as_ref()
            .and_then(|f| f.horizontal_line_metrics(size))
            .map_or(size * 1.2, |metrics| {
                metrics.ascent.round() - metrics.descent.round() + metrics.line_gap.round()
            })
    }

    fn shape(&mut self, text: &str, size: f32) -> Rc<[PositionedGlyph]> {
        let key = if text.len() <= 256 {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            let key = (hasher.finish(), size.to_bits());
            if let Some((cached_text, _, glyphs)) = self.shape_cache.get(&key)
                && &**cached_text == text
            {
                self.shaping_stats.hits = self.shaping_stats.hits.saturating_add(1);
                return glyphs.clone();
            }
            Some(key)
        } else {
            None
        };
        self.shaping_stats.misses = self.shaping_stats.misses.saturating_add(1);
        // The face was validated when the font was loaded and bytes are private.
        let face = rustybuzz::Face::from_slice(&self.bytes, 0).expect("validated font face");
        let scale = size / face.units_per_em() as f32;
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let glyphs = rustybuzz::shape(&face, &[], buffer);
        let positioned: Rc<[PositionedGlyph]> = glyphs
            .glyph_infos()
            .iter()
            .zip(glyphs.glyph_positions())
            .map(|(info, position)| PositionedGlyph {
                id: info.glyph_id as u16,
                advance: position.x_advance as f32 * scale,
                x_offset: position.x_offset as f32 * scale,
                y_offset: position.y_offset as f32 * scale,
            })
            .collect();
        if let Some(key) = key {
            let width = positioned
                .iter()
                .map(|glyph| glyph.advance)
                .sum::<f32>()
                .max(0.0);
            let entry_bytes =
                text.len() + positioned.len() * std::mem::size_of::<PositionedGlyph>();
            if entry_bytes > SHAPE_CACHE_BYTES {
                return positioned;
            }
            if self.shape_cached_bytes + entry_bytes > SHAPE_CACHE_BYTES
                || self.shape_cache.len() >= SHAPE_CACHE_ENTRIES
            {
                self.shaping_stats.evictions = self
                    .shaping_stats
                    .evictions
                    .saturating_add(self.shape_cache.len() as u64);
                self.shape_cache.clear();
                self.shape_cached_bytes = 0;
            }
            self.shape_cached_bytes += entry_bytes;
            self.shape_cache
                .insert(key, (text.into(), width, positioned.clone()));
        }
        positioned
    }

    fn glyph(&mut self, id: u16, size: f32) -> &(fontdue::Metrics, Vec<u8>) {
        let key = (id, size.to_bits());
        if !self.cache.contains_key(&key) {
            let glyph = self
                .font
                .as_ref()
                .expect("validated font face")
                .rasterize_indexed(id, size);
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

    fn shaping_fonts() -> Fonts {
        Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn shaping_cache_reuses_exact_widths_and_pixels_and_bounds_retention() {
        let mut fonts = shaping_fonts();
        let text = "office café مرحبا";
        let width = fonts.width(text, 17.);
        let cold = fonts.shaping_cache_stats();
        assert_eq!((cold.hits, cold.misses, cold.entries), (0, 1, 1));
        assert_eq!(fonts.width(text, 17.), width);
        let mut cached = Canvas::new(300, 40, 0xffffff);
        cached.text(&mut fonts, 2, 2, text, 17., 0x123456);
        let mut fresh = Canvas::new(300, 40, 0xffffff);
        fresh.text(&mut shaping_fonts(), 2, 2, text, 17., 0x123456);
        assert_eq!(cached.pixels, fresh.pixels);
        assert_eq!(fonts.shaping_cache_stats().hits, 2);
        assert!((fonts.width(text, 34.) - width * 2.).abs() < 0.01);
        let long = "a".repeat(257);
        let before = fonts.shaping_cache_stats();
        fonts.width(&long, 17.);
        fonts.width(&long, 17.);
        assert_eq!(fonts.shaping_cache_stats().misses, before.misses + 2);
        assert_eq!(fonts.shaping_cache_stats().entries, before.entries);
        for index in 0..1200 {
            fonts.width(&format!("{index:04}{}", "W".repeat(100)), 17.);
            let stats = fonts.shaping_cache_stats();
            assert!(stats.entries <= SHAPE_CACHE_ENTRIES);
            assert!(stats.payload_bytes <= SHAPE_CACHE_BYTES);
        }
        assert!(fonts.shaping_cache_stats().evictions > 0);
        assert_eq!(fonts.width(text, 17.), width);
    }

    #[test]
    fn shaping_totals_survive_retired_downloaded_faces() {
        let mut fonts = shaping_fonts();
        fonts.register_family("fixture", fonts.bytes.clone());
        fonts
            .select_family(&["fixture".into()])
            .width("fixture", 16.);
        assert_eq!(fonts.shaping_cache_stats().misses, 1);
        assert_eq!(fonts.shaping_cache_stats().entries, 1);
        fonts.clear_families();
        let stats = fonts.shaping_cache_stats();
        assert_eq!(
            (
                stats.misses,
                stats.evictions,
                stats.entries,
                stats.payload_bytes
            ),
            (1, 1, 0, 0)
        );
    }

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

    #[test]
    fn synthetic_bold_and_italic_apply_only_to_downloaded_families() {
        let mut fonts = shaping_fonts();
        fonts.register_family("WebMono", fonts.bytes.clone());

        // System fonts without a bold face keep exact regular pixels.
        let mut sys_reg = Canvas::new(160, 40, 0xffffff);
        sys_reg.text_styled_alpha(&mut fonts, (4, 4), "Test", 24.0, 0, false, false, 1.0);
        let mut sys_styled = Canvas::new(160, 40, 0xffffff);
        sys_styled.text_styled_alpha(&mut fonts, (4, 4), "Test", 24.0, 0, true, true, 1.0);
        assert_eq!(sys_reg.pixels, sys_styled.pixels);

        // Registered @font-face families synthesize bolder and slanted strokes.
        let web = fonts.select_family(&["WebMono".into()]);
        let mut web_reg = Canvas::new(160, 40, 0xffffff);
        web_reg.text_styled_alpha(web, (4, 4), "Test", 24.0, 0, false, false, 1.0);
        let mut web_bold = Canvas::new(160, 40, 0xffffff);
        web_bold.text_styled_alpha(web, (4, 4), "Test", 24.0, 0, true, false, 1.0);
        let mut web_italic = Canvas::new(160, 40, 0xffffff);
        web_italic.text_styled_alpha(web, (4, 4), "Test", 24.0, 0, false, true, 1.0);

        let ink = |c: &Canvas| {
            c.pixels
                .iter()
                .map(|&px| 255u64 - u64::from(px & 0xff))
                .sum::<u64>()
        };
        assert!(ink(&web_bold) > ink(&web_reg));
        assert_ne!(web_italic.pixels, web_reg.pixels);
    }

    #[test]
    fn rounded_rect_overlap_scaling_and_zero_bleed_border_aa() {
        // CSS Backgrounds 3 §4.5: radii 80px on 100x60 box scale by f = min(100/160, 60/160) = 0.375 -> 30px.
        let r = RoundedRect::from_raw(0.0, 0.0, 100.0, 60.0, [(80.0, 80.0); 4]);
        for &(rx, ry) in &r.radii {
            assert!((rx - 30.0).abs() < 1e-4);
            assert!((ry - 30.0).abs() < 1e-4);
        }
        // Zero on one axis makes corner sharp.
        let sharp = RoundedRect::from_raw(0.0, 0.0, 40.0, 40.0, [(20.0, 0.0); 4]);
        assert!(!sharp.has_any_radius());

        // Red border-box background under opaque green rounded border must NEVER bleed red on outer fringe.
        let mut canvas = Canvas::new(40, 40, 0xffffff);
        let outer = RoundedRect::from_raw(0.0, 0.0, 40.0, 40.0, [(20.0, 20.0); 4]);
        let inner = outer.inset([6.0, 6.0, 6.0, 6.0]);
        canvas.paint_rounded_decoration(
            outer,
            inner,
            Some((outer, 0xff0000, 1.0)),
            [(0x00ff00, 1.0); 4],
        );
        // Corner (0, 0) is outside the circle -> pure white.
        assert_eq!(canvas.pixels[0], 0xffffff);
        // Center (20, 20) is inside inner -> pure red.
        assert_eq!(canvas.pixels[20 * 40 + 20], 0xff0000);
        // Top border midpoint (20, 2) is inside border ring -> pure green.
        assert_eq!(canvas.pixels[2 * 40 + 20], 0x00ff00);
        // Any pixel outside the inner padding circle must have R == B (blend of green 0x00ff00 and white 0xffffff only, zero red bleed!).
        for y in 0..40u32 {
            for x in 0..40u32 {
                if inner.pixel_is_wholly_outside(x, y) {
                    let px = canvas.pixels[(y * 40 + x) as usize];
                    let red = (px >> 16) & 255;
                    let blue = px & 255;
                    assert_eq!(red, blue, "red bled at outer pixel ({x}, {y}): {px:#08x}");
                }
            }
        }
    }
}
