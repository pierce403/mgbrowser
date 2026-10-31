//! Multi-stop linear and radial sRGB backgrounds painted directly into Canvas.
//! Every write is bounded by both the active physical clip and canvas storage;
//! author geometry never allocates a raster surface or changes image budgets.

use crate::{
    paint::Canvas,
    style::{
        BackgroundGradient, CalcOp, GradientDirection, GradientKind, Length, RadialExtent,
        RadialShape,
    },
};

const MAX_STOPS: usize = 16;

fn resolve_bg_position(len: Length, free_space: f64) -> Option<f64> {
    let val = match len {
        Length::Auto => return None,
        Length::Px(px) => f64::from(px),
        Length::Percent(pct) => f64::from(pct) * free_space,
        Length::Calc {
            px,
            percent,
            bound_px,
            op,
            non_negative,
        } => {
            let linear = f64::from(px) + f64::from(percent) * free_space;
            let bounded = match op {
                CalcOp::Linear => linear,
                CalcOp::MinPx => linear.min(f64::from(bound_px)),
                CalcOp::MaxPx => linear.max(f64::from(bound_px)),
            };
            if non_negative {
                bounded.max(0.0)
            } else {
                bounded
            }
        }
    };
    val.is_finite().then_some(val)
}

fn resolve_length(len: Length, basis: f64) -> Option<f64> {
    let val = f64::from(len.resolve(basis as f32)?);
    val.is_finite().then_some(val)
}

#[derive(Clone, Copy)]
enum Geometry {
    Linear {
        dx: f64,
        dy: f64,
        line_len: f64,
    },
    Radial {
        cx: f64,
        cy: f64,
        inv_rx: f64,
        inv_ry: f64,
        line_len: f64,
        zero_radius: bool,
    },
}

fn build_geometry(kind: GradientKind, tile_w: f64, tile_h: f64) -> Option<Geometry> {
    match kind {
        GradientKind::Linear(direction) => {
            let (dx, dy) = match direction {
                GradientDirection::Angle(angle) if angle.is_finite() => {
                    (f64::from(angle).sin(), -f64::from(angle).cos())
                }
                GradientDirection::Angle(_) => return None,
                GradientDirection::Corner { right, bottom } => {
                    let norm = tile_w.hypot(tile_h);
                    if !norm.is_finite() || norm <= 0.0 {
                        return None;
                    }
                    (
                        tile_h / norm * if right { 1.0 } else { -1.0 },
                        tile_w / norm * if bottom { 1.0 } else { -1.0 },
                    )
                }
            };
            let line_len = tile_w * dx.abs() + tile_h * dy.abs();
            if !line_len.is_finite() || line_len <= 0.0 {
                return None;
            }
            Some(Geometry::Linear { dx, dy, line_len })
        }
        GradientKind::Radial { shape, center } => {
            let cx = resolve_length(center[0], tile_w)?;
            let cy = resolve_length(center[1], tile_h)?;
            let d_left = cx.abs();
            let d_right = (tile_w - cx).abs();
            let d_top = cy.abs();
            let d_bottom = (tile_h - cy).abs();
            let close_x = d_left.min(d_right);
            let close_y = d_top.min(d_bottom);
            let far_x = d_left.max(d_right);
            let far_y = d_top.max(d_bottom);
            let (rx, ry) = match shape {
                RadialShape::CircleExplicit(r) => {
                    let rad = resolve_length(r, tile_w)?.max(0.0);
                    (rad, rad)
                }
                RadialShape::CircleExtent(ext) => {
                    let rad = match ext {
                        RadialExtent::ClosestSide => close_x.min(close_y),
                        RadialExtent::FarthestSide => far_x.max(far_y),
                        RadialExtent::ClosestCorner => close_x.hypot(close_y),
                        RadialExtent::FarthestCorner => far_x.hypot(far_y),
                    };
                    (rad, rad)
                }
                RadialShape::EllipseExplicit(rx_len, ry_len) => (
                    resolve_length(rx_len, tile_w)?.max(0.0),
                    resolve_length(ry_len, tile_h)?.max(0.0),
                ),
                RadialShape::EllipseExtent(ext) => match ext {
                    RadialExtent::ClosestSide => (close_x, close_y),
                    RadialExtent::FarthestSide => (far_x, far_y),
                    RadialExtent::ClosestCorner => {
                        if close_x <= 0.0 || close_y <= 0.0 {
                            (0.0, 0.0)
                        } else {
                            (
                                close_x * std::f64::consts::SQRT_2,
                                close_y * std::f64::consts::SQRT_2,
                            )
                        }
                    }
                    RadialExtent::FarthestCorner => {
                        if far_x <= 0.0 && far_y <= 0.0 {
                            (0.0, 0.0)
                        } else if far_x <= 0.0 {
                            (0.0, far_y)
                        } else if far_y <= 0.0 {
                            (far_x, 0.0)
                        } else {
                            (
                                far_x * std::f64::consts::SQRT_2,
                                far_y * std::f64::consts::SQRT_2,
                            )
                        }
                    }
                },
            };
            if !rx.is_finite() || !ry.is_finite() {
                return None;
            }
            if rx <= 1e-6 && ry <= 1e-6 {
                Some(Geometry::Radial {
                    cx,
                    cy,
                    inv_rx: 0.0,
                    inv_ry: 0.0,
                    line_len: 0.0,
                    zero_radius: true,
                })
            } else {
                let eff_rx = rx.max(1e-6);
                let eff_ry = ry.max(1e-6);
                Some(Geometry::Radial {
                    cx,
                    cy,
                    inv_rx: 1.0 / eff_rx,
                    inv_ry: 1.0 / eff_ry,
                    line_len: eff_rx,
                    zero_radius: false,
                })
            }
        }
    }
}

/// Rectangles are `[x, y, width, height]` in page logical coordinates. `origin`
/// is the background positioning area; `clip` is its rectangular painting area.
/// The caller supplies the respective used border/padding/content boxes. The
/// active Canvas clip is additionally respected and is never changed here.
#[cfg(test)]
pub(crate) fn paint(
    canvas: &mut Canvas,
    origin: [f32; 4],
    clip: [f32; 4],
    gradient: &BackgroundGradient,
    scroll: i32,
) -> usize {
    paint_background_gradient(canvas, origin, clip, gradient, 1.0, scroll)
}

pub(crate) fn paint_with_opacity(
    canvas: &mut Canvas,
    origin: [f32; 4],
    clip: [f32; 4],
    gradient: &BackgroundGradient,
    opacity: f32,
    scroll: i32,
) -> usize {
    paint_background_gradient(canvas, origin, clip, gradient, opacity, scroll)
}

fn paint_background_gradient(
    canvas: &mut Canvas,
    origin: [f32; 4],
    clip: [f32; 4],
    gradient: &BackgroundGradient,
    opacity: f32,
    scroll: i32,
) -> usize {
    if !opacity.is_finite()
        || opacity <= 0.0
        || origin
            .iter()
            .chain(clip.iter())
            .any(|value| !value.is_finite())
        || origin[2] <= 0.0
        || origin[3] <= 0.0
        || clip[2] <= 0.0
        || clip[3] <= 0.0
    {
        return 0;
    }
    let stop_count = gradient.stops.len();
    if !(2..=MAX_STOPS).contains(&stop_count) {
        return 0;
    }
    let opacity = f64::from(opacity.clamp(0.0, 1.0));
    let [ox, oy, origin_w, origin_h] = origin.map(f64::from);
    let tile_w = match gradient.size[0] {
        Length::Auto => origin_w,
        len => match resolve_length(len, origin_w) {
            Some(v) if v > 0.0 => v,
            _ => return 0,
        },
    };
    let tile_h = match gradient.size[1] {
        Length::Auto => origin_h,
        len => match resolve_length(len, origin_h) {
            Some(v) if v > 0.0 => v,
            _ => return 0,
        },
    };
    let (Some(pos_x), Some(pos_y)) = (
        resolve_bg_position(gradient.position[0], origin_w - tile_w),
        resolve_bg_position(gradient.position[1], origin_h - tile_h),
    ) else {
        return 0;
    };
    let tile_ox = ox + pos_x;
    let tile_oy = oy + pos_y;

    let Some(geom) = build_geometry(gradient.kind, tile_w, tile_h) else {
        return 0;
    };
    let line_len = match geom {
        Geometry::Linear { line_len, .. } | Geometry::Radial { line_len, .. } => line_len,
    };

    let mut pos_opt = [None; MAX_STOPS];
    let mut alphas = [0.0_f64; MAX_STOPS];
    let mut colors = [[0.0_f64; 3]; MAX_STOPS];
    for i in 0..stop_count {
        let stop = &gradient.stops[i];
        if !stop.color.alpha.is_finite() {
            return 0;
        }
        alphas[i] = f64::from(stop.color.alpha.clamp(0.0, 1.0));
        for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
            colors[i][channel] = f64::from((stop.color.rgb >> shift) & 255) * alphas[i];
        }
        if let Some(len) = stop.position {
            let Some(resolved) = resolve_length(len, line_len) else {
                return 0;
            };
            pos_opt[i] = Some(resolved);
        }
    }
    if pos_opt[0].is_none() {
        pos_opt[0] = Some(0.0);
    }
    if pos_opt[stop_count - 1].is_none() {
        pos_opt[stop_count - 1] = Some(line_len);
    }

    // CSS Images 3 §3.4 step 2: enforce monotonic explicit positions before spacing runs.
    let mut max_so_far = pos_opt[0].unwrap_or(0.0);
    pos_opt[0] = Some(max_so_far);
    for val in pos_opt[1..stop_count].iter_mut().flatten() {
        if *val < max_so_far {
            *val = max_so_far;
        } else {
            max_so_far = *val;
        }
    }

    // CSS Images 3 §3.4 step 3: evenly space contiguous runs of unspecified stops.
    let mut idx = 1;
    while idx < stop_count {
        if pos_opt[idx].is_none() {
            let start_idx = idx - 1;
            while idx < stop_count && pos_opt[idx].is_none() {
                idx += 1;
            }
            let p0 = pos_opt[start_idx].unwrap();
            let p1 = pos_opt[idx].unwrap();
            let span = (idx - start_idx) as f64;
            for (offset, slot) in pos_opt[(start_idx + 1)..idx].iter_mut().enumerate() {
                *slot = Some(p0 + (p1 - p0) * ((offset + 1) as f64 / span));
            }
        } else {
            idx += 1;
        }
    }

    let mut pos = [0.0_f64; MAX_STOPS];
    for i in 0..stop_count {
        let p = pos_opt[i].unwrap_or(0.0);
        if !p.is_finite() {
            return 0;
        }
        pos[i] = p;
    }

    let mut hints = [None; MAX_STOPS];
    for i in 0..(stop_count - 1) {
        if let Some(hlen) = gradient.stops[i].hint_to_next {
            let Some(h) = resolve_length(hlen, line_len) else {
                return 0;
            };
            hints[i] = Some(h.clamp(pos[i], pos[i + 1]));
        }
    }

    let first_pos = pos[0];
    let last_pos = pos[stop_count - 1];
    let period = last_pos - first_pos;
    let degenerate_solid = if gradient.repeating && period <= 1e-5 {
        let mut avg_c = [0.0_f64; 3];
        let mut avg_a = 0.0_f64;
        for i in 0..stop_count {
            avg_a += alphas[i];
            for ch in 0..3 {
                avg_c[ch] += colors[i][ch];
            }
        }
        let inv = 1.0 / (stop_count as f64);
        Some((
            [avg_c[0] * inv, avg_c[1] * inv, avg_c[2] * inv],
            avg_a * inv,
        ))
    } else {
        None
    };

    let scale = f64::from(canvas.scale());
    let (active_left, active_top, active_right, active_bottom) = canvas.physical_clip_bounds();
    // Keep explicit background clipping on the same rounded logical edges as
    // the surrounding rectangular paint path, including fractional scaling.
    let left = (f64::from(clip[0].round()) * scale)
        .round()
        .clamp(f64::from(active_left), f64::from(active_right)) as usize;
    let top = ((f64::from(clip[1].round()) - f64::from(scroll)) * scale)
        .round()
        .clamp(f64::from(active_top), f64::from(active_bottom)) as usize;
    let right = ((f64::from(clip[0].round()) + f64::from(clip[2].ceil())) * scale)
        .round()
        .clamp(f64::from(active_left), f64::from(active_right)) as usize;
    let bottom = ((f64::from(clip[1].round()) + f64::from(clip[3].ceil()) - f64::from(scroll))
        * scale)
        .round()
        .clamp(f64::from(active_top), f64::from(active_bottom)) as usize;
    let has_rclip = canvas.has_rounded_clip();
    let mut written = 0;

    for py in top..bottom {
        let mut y = (py as f64 + 0.5) / scale + f64::from(scroll) - tile_oy;
        if gradient.repeat[1] {
            y = y.rem_euclid(tile_h);
        } else if y < 0.0 || y >= tile_h {
            continue;
        }
        for px in left..right {
            let mut x = (px as f64 + 0.5) / scale - tile_ox;
            if gradient.repeat[0] {
                x = x.rem_euclid(tile_w);
            } else if x < 0.0 || x >= tile_w {
                continue;
            }

            let (pm_rgb, mut alpha) = if let Some(solid) = degenerate_solid {
                solid
            } else {
                let mut d = match geom {
                    Geometry::Linear { dx, dy, line_len } => {
                        (x - tile_w * 0.5) * dx + (y - tile_h * 0.5) * dy + line_len * 0.5
                    }
                    Geometry::Radial {
                        cx,
                        cy,
                        inv_rx,
                        inv_ry,
                        line_len,
                        zero_radius,
                    } => {
                        if zero_radius {
                            last_pos + 1.0
                        } else {
                            let u = ((x - cx) * inv_rx).hypot((y - cy) * inv_ry);
                            u * line_len
                        }
                    }
                };
                if gradient.repeating {
                    d = first_pos + (d - first_pos).rem_euclid(period);
                }
                if d < first_pos {
                    (colors[0], alphas[0])
                } else if d >= last_pos {
                    (colors[stop_count - 1], alphas[stop_count - 1])
                } else {
                    let mut seg = 0;
                    while seg + 1 < stop_count - 1 && d >= pos[seg + 1] {
                        seg += 1;
                    }
                    let p0 = pos[seg];
                    let p1 = pos[seg + 1];
                    let span = p1 - p0;
                    let mut mix = if span > 0.0 {
                        ((d - p0) / span).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    if let Some(h) = hints[seg]
                        && span > 0.0
                    {
                        let rel_h = ((h - p0) / span).clamp(0.0, 1.0);
                        if rel_h <= 1e-5 {
                            mix = if mix > 0.0 { 1.0 } else { 0.0 };
                        } else if rel_h >= 1.0 - 1e-5 {
                            mix = if mix >= 1.0 { 1.0 } else { 0.0 };
                        } else if (rel_h - 0.5).abs() > 1e-5 {
                            mix = mix.powf(-std::f64::consts::LN_2 / rel_h.ln());
                        }
                    }
                    let inv_mix = 1.0 - mix;
                    (
                        [
                            colors[seg][0] * inv_mix + colors[seg + 1][0] * mix,
                            colors[seg][1] * inv_mix + colors[seg + 1][1] * mix,
                            colors[seg][2] * inv_mix + colors[seg + 1][2] * mix,
                        ],
                        alphas[seg] * inv_mix + alphas[seg + 1] * mix,
                    )
                }
            };

            let mut clip_scale = opacity;
            alpha *= opacity;
            if alpha <= 0.0 {
                continue;
            }
            if has_rclip {
                let rcov = canvas.rounded_clip_coverage(px as u32, py as u32);
                if rcov == 0 {
                    continue;
                }
                let rscale = f64::from(rcov) / 255.0;
                clip_scale *= rscale;
                alpha *= rscale;
            }
            let pixel = &mut canvas.pixels[py * canvas.width as usize + px];
            let mut rgb = 0;
            for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
                let foreground = pm_rgb[channel] * clip_scale;
                let background = f64::from((*pixel >> shift) & 255);
                rgb |= ((foreground + background * (1.0 - alpha))
                    .round()
                    .clamp(0.0, 255.0) as u32)
                    << shift;
            }
            *pixel = rgb;
            written += 1;
        }
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{BackgroundBox, Color, GradientStop, LinearGradient};

    fn horizontal() -> LinearGradient {
        LinearGradient {
            kind: GradientKind::Linear(GradientDirection::Angle(std::f32::consts::FRAC_PI_2)),
            repeating: false,
            stops: vec![
                GradientStop {
                    color: Color {
                        rgb: 0xffffff,
                        alpha: 0.,
                    },
                    position: Some(Length::Percent(0.)),
                    hint_to_next: None,
                },
                GradientStop {
                    color: Color {
                        rgb: 0xffffff,
                        alpha: 1.,
                    },
                    position: Some(Length::Px(4.)),
                    hint_to_next: None,
                },
            ],
            origin: BackgroundBox::Padding,
            clip: BackgroundBox::Content,
            repeat: [true; 2],
            size: [Length::Auto; 2],
            position: [Length::Percent(0.); 2],
        }
    }

    #[test]
    fn transparent_white_to_four_pixel_white_uses_premultiplied_alpha() {
        let mut canvas = Canvas::new(8, 1, 0);
        assert_eq!(
            paint(
                &mut canvas,
                [0., 0., 8., 1.],
                [0., 0., 8., 1.],
                &horizontal(),
                0
            ),
            8
        );
        assert_eq!(
            canvas.pixels,
            [
                0x202020, 0x606060, 0x9f9f9f, 0xdfdfdf, 0xffffff, 0xffffff, 0xffffff, 0xffffff
            ]
        );
        let mut gradient = horizontal();
        gradient.stops[0].color.rgb = 0xff0000;
        let mut other = Canvas::new(8, 1, 0);
        paint(&mut other, [0., 0., 8., 1.], [0., 0., 8., 1.], &gradient, 0);
        assert_eq!(
            canvas.pixels, other.pixels,
            "transparent RGB cannot tint premultiplied interpolation"
        );
    }

    #[test]
    fn active_clip_and_scroll_bound_writes_at_100_125_200_percent() {
        for scale in [1., 1.25, 2.] {
            let mut canvas = Canvas::new_scaled(12, 8, 0x123456, scale);
            let saved = canvas.intersect_clip(3, 2, 4, 2);
            let bounds = canvas.physical_clip_bounds();
            let count = paint(
                &mut canvas,
                [0., 10., 12., 8.],
                [-1_000_000., -1_000_000., 2_000_000., 2_000_000.],
                &horizontal(),
                10,
            );
            assert_eq!(
                count,
                ((bounds.2 - bounds.0) * (bounds.3 - bounds.1)) as usize
            );
            for y in 0..canvas.height {
                for x in 0..canvas.width {
                    let in_clip = x >= bounds.0 && x < bounds.2 && y >= bounds.1 && y < bounds.3;
                    assert_eq!(
                        canvas.pixels[(y * canvas.width + x) as usize] != 0x123456,
                        in_clip
                    );
                }
            }
            assert_eq!(canvas.physical_clip_bounds(), bounds);
            canvas.restore_clip(saved);
        }
    }

    #[test]
    fn zero_invalid_offscreen_and_no_repeat_never_expand_paint_area() {
        let mut canvas = Canvas::new(10, 10, 0x123456);
        for origin in [
            [0., 0., 0., 5.],
            [0., 0., 5., -1.],
            [f32::NAN, 0., 5., 5.],
            [0., 0., f32::INFINITY, 5.],
        ] {
            assert_eq!(
                paint(&mut canvas, origin, [0., 0., 10., 10.], &horizontal(), 0),
                0
            );
        }
        assert_eq!(
            paint(
                &mut canvas,
                [0., 0., 10., 10.],
                [-20., -20., 5., 5.],
                &horizontal(),
                0
            ),
            0
        );
        assert!(canvas.pixels.iter().all(|pixel| *pixel == 0x123456));
        let mut gradient = horizontal();
        gradient.repeat = [false; 2];
        assert_eq!(
            paint(
                &mut canvas,
                [2., 3., 4., 2.],
                [0., 0., 10., 10.],
                &gradient,
                0
            ),
            8
        );
    }

    #[test]
    fn reversed_stops_form_a_hard_stop_and_corner_direction_depends_on_aspect() {
        let mut gradient = horizontal();
        gradient.stops[0].position = Some(Length::Px(2.));
        gradient.stops[1].position = Some(Length::Px(1.));
        let mut canvas = Canvas::new(4, 1, 0);
        paint(
            &mut canvas,
            [0., 0., 4., 1.],
            [0., 0., 4., 1.],
            &gradient,
            0,
        );
        assert_eq!(canvas.pixels, [0, 0, 0xffffff, 0xffffff]);
        gradient.kind = GradientKind::Linear(GradientDirection::Corner {
            right: true,
            bottom: true,
        });
        gradient.stops[0].position = Some(Length::Percent(0.));
        gradient.stops[1].position = Some(Length::Percent(1.));
        let mut canvas = Canvas::new(8, 4, 0);
        paint(
            &mut canvas,
            [0., 0., 8., 4.],
            [0., 0., 8., 4.],
            &gradient,
            0,
        );
        assert_eq!(canvas.pixels[0], 0x181818);
        assert_eq!(canvas.pixels[31], 0xe7e7e7);
        assert_eq!(canvas.pixels[3], 0x484848);
    }

    #[test]
    fn multi_stop_hint_repeating_and_radial_gradients_rasterize_cleanly() {
        let mut multi = horizontal();
        multi.stops = vec![
            GradientStop {
                color: Color {
                    rgb: 0xff0000,
                    alpha: 1.0,
                },
                position: Some(Length::Px(0.)),
                hint_to_next: None,
            },
            GradientStop {
                color: Color {
                    rgb: 0x00ff00,
                    alpha: 1.0,
                },
                position: None,
                hint_to_next: None,
            },
            GradientStop {
                color: Color {
                    rgb: 0x0000ff,
                    alpha: 1.0,
                },
                position: Some(Length::Px(8.)),
                hint_to_next: None,
            },
        ];
        let mut canvas = Canvas::new(9, 1, 0);
        paint(&mut canvas, [0., 0., 9., 1.], [0., 0., 9., 1.], &multi, 0);
        assert!((canvas.pixels[0] >> 16) > 200);
        assert!(((canvas.pixels[4] >> 8) & 255) > 200);
        assert!((canvas.pixels[8] & 255) > 200);

        let mut radial = multi;
        radial.kind = GradientKind::Radial {
            shape: RadialShape::CircleExplicit(Length::Px(4.)),
            center: [Length::Percent(0.5), Length::Percent(0.5)],
        };
        radial.stops = vec![
            GradientStop {
                color: Color {
                    rgb: 0xff0000,
                    alpha: 1.0,
                },
                position: Some(Length::Percent(0.)),
                hint_to_next: None,
            },
            GradientStop {
                color: Color {
                    rgb: 0x0000ff,
                    alpha: 1.0,
                },
                position: Some(Length::Percent(1.)),
                hint_to_next: None,
            },
        ];
        let mut rcanvas = Canvas::new(9, 9, 0);
        paint(&mut rcanvas, [0., 0., 9., 9.], [0., 0., 9., 9.], &radial, 0);
        assert_eq!(rcanvas.pixels[4 * 9 + 4], 0xff0000);
        assert_eq!(rcanvas.pixels[0], 0x0000ff);
    }
}
