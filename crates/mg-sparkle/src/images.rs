//! Bounded, Rust-only page images. No image resolver may read files or fetch URLs.
//! PNG, JPEG, the first GIF frame, and a small SVG shape subset are supported.
use crate::paint::Canvas;
use std::io::Cursor;

const MAX_SOURCE: usize = 512 * 1024;
const MAX_SIDE: u32 = 2048;
const MAX_PIXELS: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA, in row-major order.
    pub pixels: Vec<u8>,
}

/// Decode supplied bytes only. Missing target dimensions use the intrinsic size;
/// one specified dimension preserves aspect ratio. Explicit zero paints nothing.
pub fn decode(
    bytes: &[u8],
    content_type: &str,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<RasterImage, String> {
    if bytes.len() > MAX_SOURCE {
        return Err("Image source exceeds 512 KiB".into());
    }
    if width == Some(0) || height == Some(0) {
        return Ok(RasterImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        });
    }
    if content_type.split(';').next() == Some("image/svg+xml")
        || bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
    {
        return svg(bytes, width, height);
    }
    let format = image::guess_format(bytes).map_err(|e| format!("Unsupported image: {e}"))?;
    if !matches!(
        format,
        image::ImageFormat::Png | image::ImageFormat::Gif | image::ImageFormat::Jpeg
    ) {
        return Err("Only PNG, JPEG, GIF and simple SVG page images are supported".into());
    }
    let reader = || {
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits.max_alloc = Some(8 * 1024 * 1024);
        reader.limits(limits);
        reader
    };
    let (natural_width, natural_height) = reader().into_dimensions().map_err(|e| e.to_string())?;
    check_size(natural_width, natural_height)?;
    let (width, height) = target_size(natural_width, natural_height, width, height)?;
    let decoded = reader().decode().map_err(|e| e.to_string())?.into_rgba8();
    let pixels = if decoded.dimensions() == (width, height) {
        decoded.into_raw()
    } else {
        image::imageops::resize(
            &decoded,
            width,
            height,
            image::imageops::FilterType::Triangle,
        )
        .into_raw()
    };
    Ok(RasterImage {
        width,
        height,
        pixels,
    })
}

fn check_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_SIDE
        || height > MAX_SIDE
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err("Image dimensions exceed the small-image limit".into());
    }
    Ok(())
}

fn target_size(
    nw: u32,
    nh: u32,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<(u32, u32), String> {
    check_size(nw, nh)?;
    let (w, h) = match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (
            w,
            (f64::from(nh) * f64::from(w) / f64::from(nw))
                .round()
                .max(1.0) as u32,
        ),
        (None, Some(h)) => (
            (f64::from(nw) * f64::from(h) / f64::from(nh))
                .round()
                .max(1.0) as u32,
            h,
        ),
        (None, None) => (nw, nh),
    };
    check_size(w, h)?;
    Ok((w, h))
}

fn svg(bytes: &[u8], width: Option<u32>, height: Option<u32>) -> Result<RasterImage, String> {
    let source = std::str::from_utf8(bytes).map_err(|_| "SVG must be UTF-8")?;
    let xml = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 2048,
        },
    )
    .map_err(|e| format!("Invalid or oversized SVG: {e}"))?;
    if xml.root_element().tag_name().name() != "svg" {
        return Err("Image is not SVG".into());
    }
    // Restrict complexity before the renderer sees the document. In particular,
    // exclude filters, recursive use, embedded images, stylesheets and SVG text.
    for node in xml.descendants().filter(|node| node.is_element()) {
        if node.ancestors().take(34).count() > 33 {
            return Err("SVG nesting exceeds 32 levels".into());
        }
        if !matches!(
            node.tag_name().name(),
            "svg"
                | "g"
                | "path"
                | "rect"
                | "circle"
                | "ellipse"
                | "line"
                | "polyline"
                | "polygon"
                | "title"
                | "desc"
        ) {
            return Err("Unsupported SVG element; only simple shapes are supported".into());
        }
        for attribute in node.attributes() {
            if attribute.name() == "href"
                || attribute.name().starts_with("on")
                || attribute.value().to_ascii_lowercase().contains("url(")
                || attribute.value().len() > 32 * 1024
            {
                return Err("Unsupported SVG reference, event or oversized attribute".into());
            }
        }
    }
    let options = resvg::usvg::Options {
        // usvg's default string resolver can read local files. Neither resolver
        // is allowed here, even if the structural allowlist is later expanded.
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(source, &options).map_err(|e| e.to_string())?;
    let natural = tree.size();
    let (width, height) = target_size(
        natural.width().ceil() as u32,
        natural.height().ceil() as u32,
        width,
        height,
    )?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or("Cannot allocate image")?;
    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / natural.width(),
        height as f32 / natural.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut pixels = pixmap.take();
    for rgba in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(rgba[3]);
        if alpha != 0 {
            for channel in &mut rgba[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    Ok(RasterImage {
        width,
        height,
        pixels,
    })
}

/// Blend a logical-size image onto the scaled surface, clipping in wide signed
/// arithmetic. Sampling writes directly into the destination: display scaling
/// never expands the bounded decoded-image allocation or cache budget.
pub fn blit(canvas: &mut Canvas, image: &RasterImage, x: i32, y: i32) {
    if image.width == 0
        || image.height == 0
        || (image.width as usize)
            .checked_mul(image.height as usize)
            .and_then(|n| n.checked_mul(4))
            != Some(image.pixels.len())
    {
        return;
    }
    let (clip_left, clip_top, clip_right, clip_bottom) = canvas.physical_clip_bounds();
    let left = canvas
        .physical_edge(i64::from(x))
        .clamp(i64::from(clip_left), i64::from(clip_right));
    let right = canvas
        .physical_edge(i64::from(x) + i64::from(image.width))
        .clamp(i64::from(clip_left), i64::from(clip_right));
    let top = canvas
        .physical_edge(i64::from(y))
        .clamp(i64::from(clip_top), i64::from(clip_bottom));
    let bottom = canvas
        .physical_edge(i64::from(y) + i64::from(image.height))
        .clamp(i64::from(clip_top), i64::from(clip_bottom));
    let scale = f64::from(canvas.scale());
    for sy in top..bottom {
        let source_y = ((sy as f64 + 0.5) / scale - f64::from(y))
            .floor()
            .clamp(0.0, f64::from(image.height - 1)) as usize;
        for sx in left..right {
            let source_x = ((sx as f64 + 0.5) / scale - f64::from(x))
                .floor()
                .clamp(0.0, f64::from(image.width - 1)) as usize;
            let source = (source_y * image.width as usize + source_x) * 4;
            let rgba = &image.pixels[source..source + 4];
            let alpha = u32::from(rgba[3]);
            let target = &mut canvas.pixels[sy as usize * canvas.width as usize + sx as usize];
            let blend = |channel: usize, shift: u32| {
                (u32::from(rgba[channel]) * alpha
                    + ((*target >> shift) & 255) * (255 - alpha)
                    + 127)
                    / 255
            };
            *target = (blend(0, 16) << 16) | (blend(1, 8) << 8) | blend(2, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SHAPE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 20 10"><path d="M0 0H20V10H0Z" fill="#ff6600"/></svg>"##;

    #[test]
    fn scaled_blit_alpha_and_clipping_match_logical_rectangle_edges() {
        let image = RasterImage {
            width: 4,
            height: 3,
            pixels: [0, 0, 0, 128].repeat(12),
        };
        let source_bytes = image.pixels.len();
        for scale in [1.0, 1.25, 2.0] {
            let mut canvas = Canvas::new_scaled(8, 6, 0xffffff, scale);
            blit(&mut canvas, &image, -1, 1);
            let mut expected = Canvas::new_scaled(8, 6, 0xffffff, scale);
            expected.rect(-1, 1, 4, 3, 0x7f7f7f);
            assert_eq!(canvas.pixels, expected.pixels);
            let before = canvas.pixels.clone();
            blit(&mut canvas, &image, i32::MIN, i32::MIN);
            blit(&mut canvas, &image, i32::MAX, i32::MAX);
            assert_eq!(canvas.pixels, before);
            assert_eq!(image.pixels.len(), source_bytes);
        }
        let image = RasterImage {
            width: 2,
            height: 1,
            pixels: vec![255, 0, 0, 255, 0, 0, 0, 0],
        };
        let mut canvas = Canvas::new_scaled(4, 2, 0xffffff, 2.0);
        blit(&mut canvas, &image, 1, 0);
        assert_eq!(
            &canvas.pixels[..8],
            &[
                0xffffff, 0xffffff, 0xff0000, 0xff0000, 0xffffff, 0xffffff, 0xffffff, 0xffffff
            ]
        );
    }

    #[test]
    fn image_alpha_blit_obeys_nested_canvas_clips_without_resampling_changes() {
        let image = RasterImage {
            width: 4,
            height: 3,
            pixels: [11, 80, 160, 128].repeat(12),
        };
        for scale in [1.0, 1.25, 2.0] {
            let mut reference = Canvas::new_scaled(8, 6, 0xffffff, scale);
            blit(&mut reference, &image, -1, 1);
            let mut clipped = Canvas::new_scaled(8, 6, 0xffffff, scale);
            let full = clipped.intersect_clip(-2, 0, 5, 5);
            let outer = clipped.intersect_clip(1, 2, 5, 2);
            let bounds = clipped.physical_clip_bounds();
            blit(&mut clipped, &image, -1, 1);
            let mut changed = 0;
            for y in 0..clipped.height {
                for x in 0..clipped.width {
                    let index = (y * clipped.width + x) as usize;
                    let inside = x >= bounds.0 && x < bounds.2 && y >= bounds.1 && y < bounds.3;
                    let expected = if inside {
                        reference.pixels[index]
                    } else {
                        0xffffff
                    };
                    assert_eq!(clipped.pixels[index], expected);
                    changed += usize::from(clipped.pixels[index] != 0xffffff);
                }
            }
            assert!(changed > 0);
            clipped.restore_clip(outer);
            clipped.intersect_clip(i32::MAX, i32::MAX, u32::MAX, u32::MAX);
            let before = clipped.pixels.clone();
            blit(&mut clipped, &image, -1, 1);
            blit(&mut clipped, &image, i32::MIN, i32::MIN);
            assert_eq!(clipped.pixels, before);
            clipped.restore_clip(full);
            assert_eq!(
                clipped.physical_clip_bounds(),
                (0, 0, clipped.width, clipped.height)
            );
            let mut explicit_full = Canvas::new_scaled(8, 6, 0xffffff, scale);
            explicit_full.intersect_clip(0, 0, 8, 6);
            blit(&mut explicit_full, &image, -1, 1);
            assert_eq!(explicit_full.pixels, reference.pixels);
            assert_eq!(image.pixels.len(), 48);
        }
    }

    #[test]
    fn svg_paths_viewbox_and_aspect_ratio() {
        let image = decode(SHAPE, "image/svg+xml", Some(10), None).unwrap();
        assert_eq!((image.width, image.height), (10, 5));
        assert_eq!(&image.pixels[0..4], &[255, 102, 0, 255]);
        assert!(
            decode(SHAPE, "image/svg+xml", Some(0), Some(10))
                .unwrap()
                .pixels
                .is_empty()
        );
    }

    #[test]
    fn svg_cannot_load_files_embedded_images_or_active_content() {
        for content in [
            "<image href='/etc/passwd'/>",
            "<image href='data:image/svg+xml,garbage'/>",
            "<use href='#x'/>",
            "<script>alert(1)</script>",
            "<filter/>",
            "<text>not enabled</text>",
            "<path fill='url(file:///etc/passwd)'/>",
        ] {
            let source = format!("<svg xmlns='http://www.w3.org/2000/svg'>{content}</svg>");
            assert!(decode(source.as_bytes(), "image/svg+xml", None, None).is_err());
        }
        assert!(
            decode(
                b"<!DOCTYPE svg [<!ENTITY x 'hello'>]><svg>&x;</svg>",
                "image/svg+xml",
                None,
                None
            )
            .is_err()
        );
    }

    #[test]
    fn image_bounds_and_malformed_input_are_rejected() {
        assert!(decode(SHAPE, "image/svg+xml", Some(2048), Some(2048)).is_err());
        assert!(decode(&vec![0; MAX_SOURCE + 1], "image/png", None, None).is_err());
        assert!(decode(b"not an image", "image/png", None, None).is_err());
        let deep = format!(
            "<svg xmlns='http://www.w3.org/2000/svg'>{}{}{}</svg>",
            "<g>".repeat(40),
            "<rect width='1' height='1'/>",
            "</g>".repeat(40)
        );
        assert!(decode(deep.as_bytes(), "image/svg+xml", None, None).is_err());
    }

    #[test]
    fn png_and_transparent_gif_decode_without_native_codecs() {
        let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([11, 22, 33, 0]));
        let mut png = Cursor::new(Vec::new());
        pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let image = decode(png.get_ref(), "image/png", None, None).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.pixels[3], 0);
        let mut gif = Vec::new();
        image::codecs::gif::GifEncoder::new(&mut gif)
            .encode_frame(image::Frame::new(pixels))
            .unwrap();
        let image = decode(&gif, "image/gif", None, None).unwrap();
        assert!(image.pixels.chunks_exact(4).all(|rgba| rgba[3] == 0));
    }

    fn authored_jpeg() -> Vec<u8> {
        let pixels = image::RgbImage::from_pixel(8, 4, image::Rgb([80, 120, 160]));
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 95)
            .encode_image(&pixels)
            .unwrap();
        encoded
    }

    #[test]
    fn jpeg_rgb_and_grayscale_decode_and_preserve_bounded_aspect_ratio() {
        let encoded = authored_jpeg();
        for (width, height, expected) in [
            (None, None, (8, 4)),
            (Some(4), None, (4, 2)),
            (None, Some(8), (16, 8)),
            (Some(3), Some(5), (3, 5)),
        ] {
            let decoded = decode(&encoded, "image/jpeg", width, height).unwrap();
            assert_eq!((decoded.width, decoded.height), expected);
            assert_eq!(decoded.pixels.len(), (expected.0 * expected.1 * 4) as usize);
            for pixel in decoded.pixels.chunks_exact(4) {
                for (actual, expected) in pixel[..3].iter().zip([80u8, 120, 160]) {
                    assert!(actual.abs_diff(expected) <= 3);
                }
                assert_eq!(pixel[3], 255);
            }
        }
        let mut gray = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut gray, 95)
            .encode(&[128; 8], 4, 2, image::ExtendedColorType::L8)
            .unwrap();
        let decoded = decode(&gray, "image/jpeg", None, None).unwrap();
        assert_eq!((decoded.width, decoded.height), (4, 2));
        assert!(decoded.pixels.chunks_exact(4).all(|rgba| rgba[0] == rgba[1]
            && rgba[1] == rgba[2]
            && rgba[0].abs_diff(128) <= 2
            && rgba[3] == 255));
    }

    #[test]
    fn jpeg_malformed_source_and_output_bounds_remain_fail_closed() {
        let encoded = authored_jpeg();
        for malformed in [
            &encoded[..2],
            &encoded[..32],
            b"\xff\xd8\xff\xd9".as_slice(),
        ] {
            assert!(decode(malformed, "image/jpeg", None, None).is_err());
        }
        let mut too_large = encoded.clone();
        too_large.resize(MAX_SOURCE + 1, 0);
        assert_eq!(
            decode(&too_large, "image/jpeg", None, None).unwrap_err(),
            "Image source exceeds 512 KiB"
        );
        assert!(decode(&encoded, "image/jpeg", Some(MAX_SIDE + 1), None).is_err());
        assert!(decode(&encoded, "image/jpeg", Some(MAX_SIDE), Some(MAX_SIDE)).is_err());
        assert!(
            decode(&encoded, "image/jpeg", Some(0), None)
                .unwrap()
                .pixels
                .is_empty()
        );

        // Header dimensions are checked before allocating/decompressing pixels.
        let sof = encoded
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xc0])
            .unwrap();
        for (width, height) in [(2049u16, 4u16), (1025, 1024)] {
            let mut dimensions = encoded.clone();
            dimensions[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
            dimensions[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
            let error = decode(&dimensions, "image/jpeg", None, None).unwrap_err();
            assert_eq!(
                error,
                if width > MAX_SIDE as u16 {
                    // ImageReader rejects its explicit maximum side first.
                    "Image size exceeds limit"
                } else {
                    // Both sides fit, but Mg's independent total-pixel cap rejects.
                    "Image dimensions exceed the small-image limit"
                }
            );
        }
        assert!(
            decode(b"BMnot-enabled", "image/bmp", None, None)
                .unwrap_err()
                .contains("Only PNG, JPEG, GIF")
        );
    }

    #[test]
    fn alpha_blending_clips_extreme_and_negative_origins() {
        let image = RasterImage {
            width: 2,
            height: 2,
            pixels: [0, 0, 0, 128].repeat(4),
        };
        let mut canvas = Canvas::new(2, 2, 0xffffff);
        blit(&mut canvas, &image, -1, -1);
        assert_eq!(canvas.pixels, vec![0x7f7f7f, 0xffffff, 0xffffff, 0xffffff]);
        let before = canvas.pixels.clone();
        blit(&mut canvas, &image, i32::MAX, i32::MAX);
        blit(&mut canvas, &image, i32::MIN, i32::MIN);
        assert_eq!(before, canvas.pixels);
    }
}
