//! Serialize only an existing bounded simple-shape SVG subtree.
//!
//! This is not an SVG parser, CSS cascade or resource loader. The computed-style
//! entry point consumes the caller's already-computed fill/stroke snapshot.
//! Successful bytes still pass through `images::decode` with disabled resolvers.
use crate::{
    document::Document,
    style::{Color, ComputedStyle, Display, SvgPaint},
};

const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 2048;
const MAX_SOURCE: usize = 512 * 1024;
const MAX_ATTRIBUTE: usize = 32 * 1024;
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// Preserve admitted DOM geometry and presentation attributes as escaped XML.
/// Root `color` comes from the computed HTML style so explicit `currentColor`
/// attributes inherit correctly. No implicit fill is invented from that color.
/// Inline styles and stylesheet-derived fill/stroke are not implemented here.
#[cfg(test)]
pub(crate) fn source(document: &Document, node: usize, color: Color) -> Result<Vec<u8>, String> {
    serialize_source(document, node, PaintSource::Original(color), None)
}

/// Preserve the admitted DOM geometry with actual computed per-node SVG paint.
/// Fill/stroke alpha already includes its respective paint opacity. Original
/// paint/paint-opacity attributes are omitted to avoid applying opacity twice.
/// Unsupported paint servers fail closed; stylesheets are never copied to SVG.
pub(crate) fn source_with_styles(
    document: &Document,
    node: usize,
    styles: &[ComputedStyle],
) -> Result<Vec<u8>, String> {
    if styles.len() != document.nodes.len() {
        return Err("Inline SVG style snapshot does not match the DOM".into());
    }
    serialize_source(document, node, PaintSource::Computed(styles), None)
}

/// Resolve an inline SVG viewBox against its actual CSS content-box viewport.
/// Only root width/height are replaced. External image decoding is unchanged.
pub(crate) fn source_with_styles_at_size(
    document: &Document,
    node: usize,
    styles: &[ComputedStyle],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if styles.len() != document.nodes.len() {
        return Err("Inline SVG style snapshot does not match the DOM".into());
    }
    if width == 0 || height == 0 {
        return Err("Inline SVG viewport must have positive dimensions".into());
    }
    serialize_source(
        document,
        node,
        PaintSource::Computed(styles),
        Some((width, height)),
    )
}

#[derive(Clone, Copy)]
enum PaintSource<'a> {
    #[cfg(test)]
    Original(Color),
    Computed(&'a [ComputedStyle]),
}

fn serialize_source(
    document: &Document,
    node: usize,
    paint: PaintSource<'_>,
    viewport: Option<(u32, u32)>,
) -> Result<Vec<u8>, String> {
    if document
        .nodes
        .get(node)
        .is_none_or(|node| node.tag != "svg")
    {
        return Err("Inline SVG source must start at an SVG element".into());
    }
    // Count escaped bytes and validate the entire tree before allocating the
    // output. The DOM is borrowed across both passes, never cloned into XML.
    let mut measure = Output::default();
    serialize(
        document,
        node,
        None,
        1,
        paint,
        viewport,
        &mut 0,
        &mut measure,
    )?;
    let mut output = Output {
        bytes: Some(Vec::with_capacity(measure.len)),
        len: 0,
    };
    serialize(
        document,
        node,
        None,
        1,
        paint,
        viewport,
        &mut 0,
        &mut output,
    )?;
    debug_assert_eq!(measure.len, output.len);
    Ok(output.bytes.unwrap())
}

#[derive(Default)]
struct Output {
    bytes: Option<Vec<u8>>,
    len: usize,
}

impl Output {
    fn color(&mut self, color: Color) -> Result<(), String> {
        if !color.alpha.is_finite() || !(0.0..=1.0).contains(&color.alpha) {
            return Err("Inline SVG computed color is invalid".into());
        }
        self.push(&format!(
            "rgba({},{},{},{})",
            (color.rgb >> 16) & 255,
            (color.rgb >> 8) & 255,
            color.rgb & 255,
            color.alpha
        ))
    }

    fn paint(&mut self, paint: SvgPaint) -> Result<(), String> {
        match paint {
            SvgPaint::None => self.push("none"),
            SvgPaint::Color(color) => self.color(color),
            SvgPaint::Unsupported => Err("Inline SVG computed paint is unsupported".into()),
        }
    }

    fn push(&mut self, value: &str) -> Result<(), String> {
        self.len = self
            .len
            .checked_add(value.len())
            .filter(|len| *len <= MAX_SOURCE)
            .ok_or("Inline SVG source exceeds 512 KiB")?;
        if let Some(bytes) = &mut self.bytes {
            bytes.extend_from_slice(value.as_bytes());
        }
        Ok(())
    }

    fn escaped(&mut self, value: &str) -> Result<(), String> {
        let mut start = 0;
        for (index, character) in value.char_indices() {
            if !matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
            {
                return Err("Inline SVG contains an invalid XML character".into());
            }
            let replacement = match character {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&apos;",
                _ => continue,
            };
            self.push(&value[start..index])?;
            self.push(replacement)?;
            start = index + character.len_utf8();
        }
        self.push(&value[start..])
    }
}

fn serialize(
    document: &Document,
    id: usize,
    parent: Option<usize>,
    depth: usize,
    paint: PaintSource<'_>,
    viewport: Option<(u32, u32)>,
    visited: &mut usize,
    output: &mut Output,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err("Inline SVG nesting exceeds 32 levels".into());
    }
    *visited += 1;
    if *visited > MAX_NODES {
        return Err("Inline SVG exceeds 2048 nodes".into());
    }
    let node = document
        .nodes
        .get(id)
        .ok_or("Inline SVG contains a missing DOM node")?;
    if parent.is_some_and(|parent| node.parent != parent) {
        return Err("Inline SVG contains an inconsistent DOM parent".into());
    }
    if node.tag == "#text" {
        if !node.children.is_empty() || !node.attributes.is_empty() {
            return Err("Inline SVG text node has unexpected content".into());
        }
        if parent
            .is_some_and(|parent| matches!(document.nodes[parent].tag.as_str(), "title" | "desc"))
        {
            return output.escaped(&node.text);
        }
        if node.text.chars().all(char::is_whitespace) {
            return Ok(());
        }
        return Err("Inline SVG text is supported only in title or description".into());
    }
    if !matches!(
        node.tag.as_str(),
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
        return Err("Unsupported inline SVG element; only simple shapes are supported".into());
    }
    if let Some(parent) = parent {
        match document.nodes[parent].tag.as_str() {
            "svg" | "g" => {}
            "title" | "desc" => {
                return Err("Inline SVG title or description contains an element".into());
            }
            _ if matches!(node.tag.as_str(), "title" | "desc") => {}
            _ => return Err("Inline SVG shape contains unsupported element children".into()),
        }
    }
    if !node.text.is_empty() {
        return Err("Inline SVG element has unexpected direct text".into());
    }
    output.push("<")?;
    output.push(&node.tag)?;
    if parent.is_none() {
        output.push(" xmlns=\"")?;
        output.push(SVG_NAMESPACE)?;
        output.push("\"")?;
        if let Some((width, height)) = viewport {
            output.push(&format!(" width=\"{width}\" height=\"{height}\""))?;
        }
    }
    match paint {
        #[cfg(test)]
        PaintSource::Original(color) if parent.is_none() => {
            output.push(" color=\"")?;
            output.color(color)?;
            output.push("\"")?;
        }
        PaintSource::Computed(styles) => {
            let style = &styles[id];
            output.push(" color=\"")?;
            output.color(style.color)?;
            output.push("\" fill=\"")?;
            output.paint(style.svg_fill)?;
            output.push("\" stroke=\"")?;
            output.paint(style.svg_stroke)?;
            // Inside the SVG image, display controls subtree rendering rather
            // than HTML formatting. Visibility is projected on every node so a
            // visible child can override a hidden group's inherited value.
            output.push(if style.display == Display::None {
                "\" display=\"none\""
            } else {
                "\" display=\"inline\""
            })?;
            output.push(if style.visible {
                " visibility=\"visible\""
            } else {
                " visibility=\"hidden\""
            })?;
        }
        #[cfg(test)]
        PaintSource::Original(_) => {}
    }
    for (name, value) in &node.attributes {
        attribute(name, value)?;
        if parent.is_none() && matches!(name.as_str(), "xmlns" | "color") {
            continue;
        }
        if parent.is_none() && viewport.is_some() && matches!(name.as_str(), "width" | "height") {
            continue;
        }
        if matches!(paint, PaintSource::Computed(_))
            && matches!(
                name.as_str(),
                "color"
                    | "fill"
                    | "stroke"
                    | "fill-opacity"
                    | "stroke-opacity"
                    | "display"
                    | "visibility"
            )
        {
            continue;
        }
        output.push(" ")?;
        output.push(match name.as_str() {
            "viewbox" => "viewBox",
            "preserveaspectratio" => "preserveAspectRatio",
            "pathlength" => "pathLength",
            _ => name,
        })?;
        output.push("=\"")?;
        output.escaped(value)?;
        output.push("\"")?;
    }
    output.push(">")?;
    for &child in &node.children {
        serialize(
            document,
            child,
            Some(id),
            depth + 1,
            paint,
            viewport,
            visited,
            output,
        )?;
    }
    output.push("</")?;
    output.push(&node.tag)?;
    output.push(">")
}

fn attribute(name: &str, value: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > MAX_ATTRIBUTE
        || value.len() > MAX_ATTRIBUTE
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("Inline SVG attribute name or size is unsupported".into());
    }
    if name.eq_ignore_ascii_case("style") {
        return Err("Inline SVG style attributes are not supported".into());
    }
    if name.eq_ignore_ascii_case("href")
        || name
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("on"))
        || value
            .as_bytes()
            .windows(4)
            .any(|word| word.eq_ignore_ascii_case(b"url("))
        || value.contains('\\')
    {
        return Err("Unsupported inline SVG reference or event attribute".into());
    }
    if name == "xmlns" && value != SVG_NAMESPACE {
        return Err("Inline SVG foreign namespaces are not supported".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const COLOR: Color = Color {
        rgb: 0x2468ac,
        alpha: 1.0,
    };

    fn document(html: &str) -> (Document, usize) {
        let document = crate::document::parse(html, "https://fixture.example/");
        let root = document.query_selector(0, "svg").unwrap().unwrap();
        (document, root)
    }

    #[test]
    fn preserves_geometry_case_and_computed_current_color() {
        let (document, root) = document(
            "<svg viewBox='0 0 20 10' preserveAspectRatio='xMidYMid meet' width='20' height='10'><path d='M0 0H20V10H0Z' fill='currentColor'></path></svg>",
        );
        let source = source(&document, root, COLOR).unwrap();
        let xml = std::str::from_utf8(&source).unwrap();
        assert!(xml.contains("viewBox=\"0 0 20 10\""));
        assert!(xml.contains("preserveAspectRatio=\"xMidYMid meet\""));
        let image = crate::images::decode(&source, "image/svg+xml", None, None).unwrap();
        assert_eq!((image.width, image.height), (20, 10));
        assert_eq!(&image.pixels[..4], &[0x24, 0x68, 0xac, 255]);
    }

    #[test]
    fn root_color_does_not_invent_fill_or_override_explicit_paint() {
        for (fill, expected) in [("", [0, 0, 0, 255]), ("fill='#f02040'", [240, 32, 64, 255])] {
            let (document, root) = document(&format!(
                "<svg width='2' height='2'><rect width='2' height='2' {fill}></rect></svg>"
            ));
            let image = crate::images::decode(
                &source(&document, root, COLOR).unwrap(),
                "image/svg+xml",
                None,
                None,
            )
            .unwrap();
            assert_eq!(&image.pixels[..4], &expected);
        }
    }

    #[test]
    fn current_color_preserves_computed_alpha() {
        let (document, root) = document(
            "<svg width='2' height='2'><rect width='2' height='2' fill='currentColor'></rect></svg>",
        );
        let bytes = source(
            &document,
            root,
            Color {
                alpha: 0.5,
                ..COLOR
            },
        )
        .unwrap();
        let image = crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
        assert_eq!(image.pixels[3], 128);
        assert!((i16::from(image.pixels[0]) - 0x24).abs() <= 1);
        assert!((i16::from(image.pixels[1]) - 0x68).abs() <= 1);
        assert!((i16::from(image.pixels[2]) - 0xac).abs() <= 1);
    }

    fn computed(document: &Document) -> Vec<ComputedStyle> {
        crate::style::compute_styles(document, &document.stylesheets, (100.0, 100.0)).unwrap()
    }

    #[test]
    fn computed_fill_uses_class_cascade_and_child_current_color() {
        let (document, root) = document(
            "<style>.icon{fill:currentColor;color:#2468ac}.child{color:#ac6824}</style><svg class='icon' width='2' height='2'><g class='child'><rect width='2' height='2'/></g></svg>",
        );
        let bytes = source_with_styles(&document, root, &computed(&document)).unwrap();
        let image = crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
        assert_eq!(&image.pixels[..4], &[0xac, 0x68, 0x24, 255]);
    }

    #[test]
    fn computed_presentation_paint_opacity_is_not_applied_twice() {
        for (extra, expected_alpha) in [("", 128), ("opacity='.5'", 64)] {
            let (document, root) = document(&format!(
                "<svg width='2' height='2' fill='#2468ac' fill-opacity='.5' {extra}><g><rect width='2' height='2'/></g></svg>"
            ));
            let bytes = source_with_styles(&document, root, &computed(&document)).unwrap();
            let xml = std::str::from_utf8(&bytes).unwrap();
            assert!(!xml.contains("fill-opacity"));
            if !extra.is_empty() {
                assert!(xml.contains("opacity=\".5\""));
            }
            let image = crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
            assert_eq!(image.pixels[3], expected_alpha);
        }
    }

    #[test]
    fn computed_none_can_override_original_filled_geometry() {
        let (document, root) = document(
            "<style>.icon{fill:none}</style><svg class='icon' width='2' height='2' fill='red'><rect width='2' height='2'/></svg>",
        );
        let bytes = source_with_styles(&document, root, &computed(&document)).unwrap();
        let image = crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
        assert!(image.pixels.chunks_exact(4).all(|pixel| pixel[3] == 0));
    }

    #[test]
    fn svg_visibility_hints_cannot_inject_declarations_or_important() {
        let (document, root) = document(
            "<svg width='2' height='2'><rect width='2' height='2' fill='red' display='none;fill:blue' visibility='hidden!important'/></svg>",
        );
        let mut diagnostics = crate::style::StyleDiagnostics::default();
        let styles = crate::style::compute_styles_with_diagnostics(
            &document,
            &document.stylesheets,
            (100., 100.),
            &mut diagnostics,
        )
        .unwrap();
        assert_eq!(diagnostics.entries.len(), 2);
        assert!(
            diagnostics
                .entries
                .iter()
                .all(|entry| entry.kind == "css-parse"
                    && entry.message.contains("presentation attribute"))
        );
        let bytes = source_with_styles(&document, root, &styles).unwrap();
        let xml = std::str::from_utf8(&bytes).unwrap();
        assert!(!xml.contains("none;fill") && !xml.contains("!important"));
        let image = crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
        assert_eq!(&image.pixels[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn computed_stroke_keeps_alpha_without_duplicate_stroke_opacity() {
        let (document, root) = document(
            "<style>.icon{color:#2468ac;stroke:currentColor;stroke-opacity:.5;fill:none}</style><svg class='icon' width='10' height='10'><path d='M1 5H9' stroke-width='2'/></svg>",
        );
        let bytes = source_with_styles(&document, root, &computed(&document)).unwrap();
        assert!(
            !std::str::from_utf8(&bytes)
                .unwrap()
                .contains("stroke-opacity")
        );
        let image = crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
        let pixel = &image.pixels[4 * (5 * 10 + 5)..][..4];
        assert_eq!(pixel[3], 128);
        assert!((i16::from(pixel[0]) - 0x24).abs() <= 1);
        assert!((i16::from(pixel[1]) - 0x68).abs() <= 1);
        assert!((i16::from(pixel[2]) - 0xac).abs() <= 1);
    }

    #[test]
    fn computed_snapshot_mismatch_and_unsupported_child_paint_are_rejected() {
        let (document, root) =
            document("<svg width='2' height='2'><rect width='2' height='2'/></svg>");
        let mut styles = computed(&document);
        assert_eq!(
            source_with_styles(&document, root, &styles[..styles.len() - 1]).unwrap_err(),
            "Inline SVG style snapshot does not match the DOM"
        );
        let child = document.nodes[root].children[0];
        styles[child].svg_fill = SvgPaint::Unsupported;
        assert_eq!(
            source_with_styles(&document, root, &styles).unwrap_err(),
            "Inline SVG computed paint is unsupported"
        );
        styles[child].svg_fill = SvgPaint::None;
        styles[child].svg_stroke = SvgPaint::Unsupported;
        assert_eq!(
            source_with_styles(&document, root, &styles).unwrap_err(),
            "Inline SVG computed paint is unsupported"
        );
    }

    #[test]
    fn css_viewport_changes_only_root_dimensions_and_keeps_source_bound() {
        let (mut document, root) = document(
            "<svg width='20' height='10' viewBox='0 0 20 10' preserveAspectRatio='xMidYMid meet'><rect width='20' height='10' fill='#2468ac'/></svg>",
        );
        let styles = computed(&document);
        let bytes = source_with_styles_at_size(&document, root, &styles, 40, 40).unwrap();
        let xml = roxmltree::Document::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(xml.root_element().attribute("width"), Some("40"));
        assert_eq!(xml.root_element().attribute("height"), Some("40"));
        let rect = xml
            .descendants()
            .find(|node| node.has_tag_name("rect"))
            .unwrap();
        assert_eq!(rect.attribute("width"), Some("20"));
        assert_eq!(rect.attribute("height"), Some("10"));
        let image = crate::images::decode(&bytes, "image/svg+xml", Some(40), Some(40)).unwrap();
        assert_eq!(image.pixels[4 * (4 * 40 + 20) + 3], 0);
        assert_eq!(
            &image.pixels[4 * (20 * 40 + 20)..][..4],
            &[0x24, 0x68, 0xac, 255]
        );
        assert!(source_with_styles_at_size(&document, root, &styles, 0, 40).is_err());
        document.nodes[root]
            .attributes
            .extend((0..4).map(|n| (format!("data-{n}"), "&".repeat(MAX_ATTRIBUTE))));
        assert_eq!(
            source_with_styles_at_size(&document, root, &styles, 40, 40).unwrap_err(),
            "Inline SVG source exceeds 512 KiB"
        );
    }

    #[test]
    fn metadata_and_custom_values_are_escaped_without_new_markup() {
        let (document, root) = document(
            "<svg width='2' height='2' data-label='&quot;&lt;&amp;&apos;' id='a&amp;b'><title>A &lt; B &amp; C</title><desc>safe text</desc><rect width='2' height='2'></rect></svg>",
        );
        let bytes = source(&document, root, COLOR).unwrap();
        let xml = roxmltree::Document::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(xml.root_element().attribute("data-label"), Some("\"<&'"));
        assert_eq!(xml.root_element().attribute("id"), Some("a&b"));
        assert_eq!(
            xml.descendants()
                .find(|node| node.has_tag_name("title"))
                .unwrap()
                .text(),
            Some("A < B & C")
        );
        crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
    }

    #[test]
    fn rejects_active_referenced_styled_foreign_and_text_content() {
        for body in [
            "<script></script>",
            "<foreignObject></foreignObject>",
            "<use href='#x'></use>",
            "<image href='https://fixture.example/a'></image>",
            "<text>hello</text>",
            "plain text",
            "<g onclick='a()'></g>",
            "<path fill='URL(#a)'></path>",
            "<path style='fill:red'></path>",
            "<g xmlns='urn:foreign'></g>",
            "<path xlink:href='#x'></path>",
            "<path fill='u\\72l(#x)'></path>",
        ] {
            let (document, root) = document(&format!("<svg>{body}</svg>"));
            assert!(source(&document, root, COLOR).is_err(), "accepted {body}");
        }
    }

    #[test]
    fn self_closing_shapes_remain_siblings_and_malformed_dom_is_not_repaired() {
        let (mut document, root) =
            document("<svg width='2' height='2'><path d='M0 0H2V2Z'/><circle r='1'/></svg>");
        assert_eq!(document.nodes[root].children.len(), 2);
        let bytes = source(&document, root, COLOR).unwrap();
        let xml = roxmltree::Document::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
        assert_eq!(
            xml.root_element()
                .children()
                .filter(|node| node.is_element())
                .count(),
            2
        );
        crate::images::decode(&bytes, "image/svg+xml", None, None).unwrap();
        let path = document.nodes[root].children[0];
        let circle = document.nodes[root].children.pop().unwrap();
        document.nodes[path].children.push(circle);
        document.nodes[circle].parent = path;
        assert_eq!(
            source(&document, root, COLOR).unwrap_err(),
            "Inline SVG shape contains unsupported element children"
        );
    }

    #[test]
    fn depth_nodes_attributes_and_escaped_source_are_bounded() {
        let (deep, root) = document(&format!(
            "<svg>{}<rect></rect>{}</svg>",
            "<g>".repeat(32),
            "</g>".repeat(32)
        ));
        assert_eq!(
            source(&deep, root, COLOR).unwrap_err(),
            "Inline SVG nesting exceeds 32 levels"
        );
        let (wide, root) = document(&format!("<svg>{}</svg>", "<g></g>".repeat(2048)));
        assert_eq!(
            source(&wide, root, COLOR).unwrap_err(),
            "Inline SVG exceeds 2048 nodes"
        );
        let (mut large, root) = document("<svg></svg>");
        large.nodes[root]
            .attributes
            .push(("data-large".into(), "x".repeat(MAX_ATTRIBUTE + 1)));
        assert_eq!(
            source(&large, root, COLOR).unwrap_err(),
            "Inline SVG attribute name or size is unsupported"
        );
        large.nodes[root].attributes = (0..4)
            .map(|n| (format!("data-{n}"), "&".repeat(MAX_ATTRIBUTE)))
            .collect();
        assert_eq!(
            source(&large, root, COLOR).unwrap_err(),
            "Inline SVG source exceeds 512 KiB"
        );
    }

    #[test]
    fn invalid_dom_and_xml_characters_fail_closed() {
        let (mut document, root) = document("<svg><g></g></svg>");
        let child = document.nodes[root].children[0];
        document.nodes[child].parent = 0;
        assert!(source(&document, root, COLOR).is_err());
        document.nodes[child].parent = root;
        document.nodes[child]
            .attributes
            .push(("data-control".into(), "\0".into()));
        assert_eq!(
            source(&document, root, COLOR).unwrap_err(),
            "Inline SVG contains an invalid XML character"
        );
        assert!(source(&document, usize::MAX, COLOR).is_err());
        assert!(
            source(
                &document,
                root,
                Color {
                    alpha: f32::NAN,
                    ..COLOR
                }
            )
            .is_err()
        );
    }
}
