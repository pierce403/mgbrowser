//! Fixed two-stop linear backgrounds painted directly into the existing canvas.
//! Every write is bounded by both the active physical clip and canvas storage;
//! author geometry never allocates a raster surface or changes image budgets.

use crate::{
    paint::Canvas,
    style::{GradientDirection, Length, LinearGradient},
};

/// Rectangles are `[x, y, width, height]` in page logical coordinates. `origin`
/// is the background positioning area; `clip` is its rectangular painting area.
/// The caller supplies the respective used border/padding/content boxes. The
/// active Canvas clip is additionally respected and is never changed here.
pub(crate) fn paint(
    canvas: &mut Canvas,
    origin: [f32; 4],
    clip: [f32; 4],
    gradient: &LinearGradient,
    scroll: i32,
) -> usize {
    if origin
        .iter()
        .chain(clip.iter())
        .any(|value| !value.is_finite())
        || origin[2] <= 0.
        || origin[3] <= 0.
        || clip[2] <= 0.
        || clip[3] <= 0.
    {
        return 0;
    }
    let [ox, oy, width, height] = origin.map(f64::from);
    let (dx, dy) = match gradient.direction {
        GradientDirection::Angle(angle) if angle.is_finite() => {
            (f64::from(angle).sin(), -f64::from(angle).cos())
        }
        GradientDirection::Angle(_) => return 0,
        GradientDirection::Corner { right, bottom } => {
            let norm = width.hypot(height);
            (
                height / norm * if right { 1. } else { -1. },
                width / norm * if bottom { 1. } else { -1. },
            )
        }
    };
    let length = width * dx.abs() + height * dy.abs();
    if !length.is_finite() || length <= 0. {
        return 0;
    }
    let position = |position| match position {
        Length::Px(value) if value.is_finite() => Some(f64::from(value)),
        Length::Percent(value) if value.is_finite() => Some(f64::from(value) * length),
        _ => None,
    };
    let (Some(first), Some(last)) = (
        position(gradient.stops[0].position),
        position(gradient.stops[1].position),
    ) else {
        return 0;
    };
    // CSS stop fixup: a later stop cannot precede an earlier explicit stop.
    let last = last.max(first);
    if !first.is_finite() || !last.is_finite() {
        return 0;
    }
    let mut alphas = [0.; 2];
    let mut colors = [[0.; 3]; 2];
    for i in 0..2 {
        let color = gradient.stops[i].color;
        if !color.alpha.is_finite() {
            return 0;
        }
        alphas[i] = f64::from(color.alpha.clamp(0., 1.));
        for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
            colors[i][channel] = f64::from((color.rgb >> shift) & 255) * alphas[i];
        }
    }
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
    let mut written = 0;
    for py in top..bottom {
        let mut y = (py as f64 + 0.5) / scale + f64::from(scroll) - oy;
        if gradient.repeat[1] {
            y = y.rem_euclid(height);
        } else if y < 0. || y >= height {
            continue;
        }
        for px in left..right {
            let mut x = (px as f64 + 0.5) / scale - ox;
            if gradient.repeat[0] {
                x = x.rem_euclid(width);
            } else if x < 0. || x >= width {
                continue;
            }
            let projected = (x - width * 0.5) * dx + (y - height * 0.5) * dy + length * 0.5;
            let mix = if last > first {
                ((projected - first) / (last - first)).clamp(0., 1.)
            } else if projected >= last {
                1.
            } else {
                0.
            };
            let alpha = alphas[0] * (1. - mix) + alphas[1] * mix;
            if alpha <= 0. {
                continue;
            }
            let pixel = &mut canvas.pixels[py * canvas.width as usize + px];
            let mut rgb = 0;
            for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
                let foreground = colors[0][channel] * (1. - mix) + colors[1][channel] * mix;
                let background = f64::from((*pixel >> shift) & 255);
                rgb |= ((foreground + background * (1. - alpha))
                    .round()
                    .clamp(0., 255.) as u32)
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
    use crate::style::{BackgroundBox, Color, GradientStop};

    fn horizontal() -> LinearGradient {
        LinearGradient {
            direction: GradientDirection::Angle(std::f32::consts::FRAC_PI_2),
            stops: [
                GradientStop {
                    color: Color {
                        rgb: 0xffffff,
                        alpha: 0.,
                    },
                    position: Length::Percent(0.),
                },
                GradientStop {
                    color: Color {
                        rgb: 0xffffff,
                        alpha: 1.,
                    },
                    position: Length::Px(4.),
                },
            ],
            origin: BackgroundBox::Padding,
            clip: BackgroundBox::Content,
            repeat: [true; 2],
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
        gradient.stops[0].position = Length::Px(2.);
        gradient.stops[1].position = Length::Px(1.);
        let mut canvas = Canvas::new(4, 1, 0);
        paint(
            &mut canvas,
            [0., 0., 4., 1.],
            [0., 0., 4., 1.],
            &gradient,
            0,
        );
        assert_eq!(canvas.pixels, [0, 0, 0xffffff, 0xffffff]);
        gradient.direction = GradientDirection::Corner {
            right: true,
            bottom: true,
        };
        gradient.stops[0].position = Length::Percent(0.);
        gradient.stops[1].position = Length::Percent(1.);
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
}
