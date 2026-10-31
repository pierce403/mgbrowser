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

/// Parse and decode a bounded `data:image/{png,jpeg,gif,svg+xml}` URL in memory
/// (`<= 512 KiB` decoded bytes), without performing any file or network I/O.
pub fn decode_data_url(url: &str) -> Result<(String, Vec<u8>), String> {
    let trimmed = url.trim_matches(|c: char| c.is_ascii_whitespace());
    if trimmed.len() < 5 || !trimmed.as_bytes()[..5].eq_ignore_ascii_case(b"data:") {
        return Err("Not a data: URL".into());
    }
    if trimmed.len() > MAX_SOURCE * 2 {
        return Err("Data image URL exceeds 512 KiB budget".into());
    }
    let (meta, payload) = trimmed[5..]
        .split_once(',')
        .ok_or_else(|| "Malformed data: URL".to_owned())?;
    let mut parts = meta.split(';');
    let raw_mime = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let content_type = match raw_mime.as_str() {
        "image/png" | "image/jpeg" | "image/gif" | "image/svg+xml" => raw_mime,
        "image/jpg" => "image/jpeg".to_owned(),
        _ => return Err("Unsupported data: image MIME type".into()),
    };
    let mut is_base64 = false;
    for param in parts {
        if param.trim().eq_ignore_ascii_case("base64") {
            is_base64 = true;
        }
    }
    let bytes = if is_base64 {
        decode_forgiving_base64(payload, MAX_SOURCE)?
    } else {
        decode_percent_bytes(payload, MAX_SOURCE)?
    };
    Ok((content_type, bytes))
}

fn decode_forgiving_base64(input: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut quad = [0u8; 4];
    let mut qlen = 0usize;
    let mut pad = 0usize;
    let mut finished = false;
    for b in input.bytes() {
        if matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ') {
            continue;
        }
        if finished {
            return Err("Unexpected data after base64 padding".into());
        }
        if b == b'=' {
            pad += 1;
            if pad > 2 || qlen < 2 || qlen + pad > 4 {
                return Err("Invalid base64 padding in data: URL".into());
            }
            if qlen + pad == 4 {
                let n = 3 - pad;
                if out.len().saturating_add(n) > max_bytes {
                    return Err("Image source exceeds 512 KiB".into());
                }
                let v = (u32::from(quad[0]) << 18)
                    | (u32::from(quad[1]) << 12)
                    | (u32::from(quad[2]) << 6);
                out.push((v >> 16) as u8);
                if n >= 2 {
                    out.push((v >> 8) as u8);
                }
                qlen = 0;
                pad = 0;
                finished = true;
            }
            continue;
        }
        if pad > 0 {
            return Err("Unexpected data after base64 padding".into());
        }
        let sextet = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("Invalid base64 character in data: URL".into()),
        };
        quad[qlen] = sextet;
        qlen += 1;
        if qlen == 4 {
            if out.len().saturating_add(3) > max_bytes {
                return Err("Image source exceeds 512 KiB".into());
            }
            let v = (u32::from(quad[0]) << 18)
                | (u32::from(quad[1]) << 12)
                | (u32::from(quad[2]) << 6)
                | u32::from(quad[3]);
            out.push((v >> 16) as u8);
            out.push((v >> 8) as u8);
            out.push(v as u8);
            qlen = 0;
        }
    }
    if pad != 0 || qlen == 1 {
        return Err("Truncated base64 payload in data: URL".into());
    }
    if qlen == 2 {
        if out.len().saturating_add(1) > max_bytes {
            return Err("Image source exceeds 512 KiB".into());
        }
        let v = (u32::from(quad[0]) << 18) | (u32::from(quad[1]) << 12);
        out.push((v >> 16) as u8);
    } else if qlen == 3 {
        if out.len().saturating_add(2) > max_bytes {
            return Err("Image source exceeds 512 KiB".into());
        }
        let v = (u32::from(quad[0]) << 18) | (u32::from(quad[1]) << 12) | (u32::from(quad[2]) << 6);
        out.push((v >> 16) as u8);
        out.push((v >> 8) as u8);
    }
    Ok(out)
}

fn decode_percent_bytes(input: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let raw = input.as_bytes();
    let mut out = Vec::with_capacity(raw.len().min(max_bytes));
    let mut i = 0usize;
    while i < raw.len() {
        let byte = if raw[i] == b'%' && i + 2 < raw.len() {
            let hi = (raw[i + 1] as char).to_digit(16);
            let lo = (raw[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                i += 3;
                ((h << 4) | l) as u8
            } else {
                let b = raw[i];
                i += 1;
                b
            }
        } else {
            let b = raw[i];
            i += 1;
            b
        };
        if out.len() >= max_bytes {
            return Err("Image source exceeds 512 KiB".into());
        }
        out.push(byte);
    }
    Ok(out)
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

fn svg_target_size(
    nw: f32,
    nh: f32,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<(u32, u32), String> {
    if !nw.is_finite() || !nh.is_finite() || nw <= 0.0 || nh <= 0.0 {
        return Err("Image dimensions exceed the small-image limit".into());
    }
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
        (None, None) => {
            let cw = nw.ceil() as u64;
            let ch = nh.ceil() as u64;
            if cw <= u64::from(MAX_SIDE)
                && ch <= u64::from(MAX_SIDE)
                && cw.saturating_mul(ch) <= MAX_PIXELS
            {
                (cw as u32, ch as u32)
            } else {
                // Vector coordinate-space viewBoxes (e.g. 0 260 1792 1260 or 0 0 4096 4096)
                // do not allocate source pixels; scale default natural rasterization down
                // proportionally to fit within MAX_SIDE and MAX_PIXELS while preserving aspect ratio.
                let side_scale =
                    (f64::from(MAX_SIDE) / f64::from(nw)).min(f64::from(MAX_SIDE) / f64::from(nh));
                let area_scale = ((MAX_PIXELS as f64) / (f64::from(nw) * f64::from(nh))).sqrt();
                let scale = side_scale.min(area_scale);
                let sw = (f64::from(nw) * scale).floor().max(1.0) as u32;
                let sh = (f64::from(nh) * scale).floor().max(1.0) as u32;
                (sw, sh)
            }
        }
    };
    check_size(w, h)?;
    Ok((w, h))
}

pub(crate) fn svg_is_aspect_ratio_only(bytes: &[u8], content_type: &str) -> bool {
    if content_type.split(';').next() != Some("image/svg+xml")
        && bytes.iter().find(|b| !b.is_ascii_whitespace()) != Some(&b'<')
    {
        return false;
    }
    let Ok(source) = std::str::from_utf8(bytes) else {
        return false;
    };
    if crate::document::has_internal_dtd_or_entity(source) {
        return false;
    }
    let Ok(xml) = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: 2048,
        },
    ) else {
        return false;
    };
    let root = xml.root_element();
    root.tag_name().name() == "svg"
        && root.attribute("viewBox").is_some()
        && root.attribute("width").is_none()
        && root.attribute("height").is_none()
}

fn svg(bytes: &[u8], width: Option<u32>, height: Option<u32>) -> Result<RasterImage, String> {
    let source = std::str::from_utf8(bytes).map_err(|_| "SVG must be UTF-8")?;
    if crate::document::has_internal_dtd_or_entity(source) {
        return Err("Invalid or oversized SVG: internal DTD or entity declaration".into());
    }
    let xml = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: true,
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
        let tag = node.tag_name().name();
        if !matches!(
            tag,
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
                | "defs"
                | "clipPath"
                | "linearGradient"
                | "radialGradient"
                | "stop"
                | "symbol"
                | "mask"
                | "metadata"
        ) {
            return Err("Unsupported SVG element; only simple shapes are supported".into());
        }
        for attribute in node.attributes() {
            let attr_name = attribute.name();
            let attr_val = attribute.value();
            let invalid_href = if attr_name == "href" {
                !matches!(tag, "linearGradient" | "radialGradient")
                    || !is_internal_svg_fragment_id(attr_val.trim())
            } else {
                false
            };
            if invalid_href
                || attr_name.starts_with("on")
                || has_disallowed_svg_url(attr_val)
                || attr_val.len() > 32 * 1024
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
    let (width, height) = svg_target_size(natural.width(), natural.height(), width, height)?;
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

fn is_internal_svg_fragment_id(value: &str) -> bool {
    let Some(id) = value.strip_prefix('#') else {
        return false;
    };
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn has_disallowed_svg_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut orig = value;
    while let Some(pos) = rest.find("url(") {
        let after = &orig[pos + 4..];
        let Some(end) = after.find(')') else {
            return true;
        };
        let inner = after[..end]
            .trim()
            .trim_matches(|c| c == '\'' || c == '"')
            .trim();
        if !is_internal_svg_fragment_id(inner) {
            return true;
        }
        let step = pos + 4 + end + 1;
        rest = &rest[step..];
        orig = &orig[step..];
    }
    false
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
    let has_rclip = canvas.has_rounded_clip();
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
            let mut alpha = u32::from(rgba[3]);
            if has_rclip {
                let rcov = u32::from(canvas.rounded_clip_coverage(sx as u32, sy as u32));
                if rcov == 0 {
                    continue;
                }
                alpha = (alpha * rcov + 127) / 255;
            }
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

    #[test]
    fn svg_gradients_defs_and_clip_paths_render_with_internal_fragment_urls_only() {
        let icon = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16">
            <defs>
                <clipPath id="clip-box"><rect x="2" y="2" width="12" height="12"/></clipPath>
                <linearGradient id="brand_grad" x1="0" y1="0" x2="1" y2="1">
                    <stop offset="0%" stop-color="#ff4500"/>
                    <stop offset="100%" stop-color="#0079d3"/>
                </linearGradient>
            </defs>
            <g clip-path="url(#clip-box)">
                <rect width="16" height="16" fill="url(#brand_grad)"/>
            </g>
        </svg>"##;
        let image = decode(icon, "image/svg+xml", None, None).unwrap();
        assert_eq!((image.width, image.height), (16, 16));
        // Clipped corner (0, 0) stays transparent while center (8, 8) is opaque gradient fill.
        assert_eq!(image.pixels[3], 0);
        assert_eq!(image.pixels[(8 * 16 + 8) * 4 + 3], 255);
        // External or malformed url() references remain rejected.
        for bad in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="url(https://evil.test/#g)"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="url(data:image/png;base64,AAAA)"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="url(#bad:colon)"/></svg>"#,
        ] {
            assert!(decode(bad.as_bytes(), "image/svg+xml", None, None).is_err());
        }
    }

    #[test]
    fn data_image_urls_decode_base64_and_percent_encoded_sources_within_limits() {
        let svg_url = r##"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='4'%3E%3Crect width='8' height='4' fill='#ff6600'/%3E%3C/svg%3E"##;
        let (mime, bytes) = decode_data_url(svg_url).unwrap();
        assert_eq!(mime, "image/svg+xml");
        let img = decode(&bytes, &mime, None, None).unwrap();
        assert_eq!((img.width, img.height), (8, 4));
        assert_eq!(&img.pixels[..4], &[255, 102, 0, 255]);

        let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 200, 30, 255]));
        let mut png = Cursor::new(Vec::new());
        pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut b64 = String::from("data:image/png;base64,");
        for chunk in png.get_ref().chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            let v = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
            b64.push(alphabet[((v >> 18) & 63) as usize] as char);
            b64.push(alphabet[((v >> 12) & 63) as usize] as char);
            b64.push(if chunk.len() > 1 {
                alphabet[((v >> 6) & 63) as usize] as char
            } else {
                '='
            });
            b64.push(if chunk.len() > 2 {
                alphabet[(v & 63) as usize] as char
            } else {
                '='
            });
        }
        let (mime, bytes) = decode_data_url(&b64).unwrap();
        assert_eq!(mime, "image/png");
        let decoded = decode(&bytes, &mime, None, None).unwrap();
        assert_eq!(&decoded.pixels[..4], &[10, 200, 30, 255]);

        assert!(decode_data_url("data:image/webp;base64,AAAA").is_err());
        assert!(decode_data_url("https://example.org/a.png").is_err());
    }
}
