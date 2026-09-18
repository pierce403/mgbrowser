//! Bounded DOM block, inline and separated-table layout.
//!
//! This is intentionally not a general CSS formatting implementation. It uses
//! real computed styles, shares table columns across rows, and records geometry
//! before viewport clipping so scrolling and links use the painted positions.

use crate::{
    document::{Document, Item},
    paint::{Canvas, Fonts},
    render::{Action, Controls, Frame, Hit, LayoutBox, Viewport},
    style::{Color, ComputedStyle, Display, Length, TextAlign, VerticalAlign, WhiteSpace},
};
use std::collections::HashMap;

mod anonymous_table;
mod formatting;
mod formatting_style;
mod positioned;
mod stacking;

const MAX_OPS: usize = 200_000;
const MAX_COLUMNS: usize = 256;
const MAX_DEPTH: usize = 256;
const MAX_EXTENT: f32 = 1_000_000.0;
const MAX_TEXT_RUN: usize = 16_384;
const MAX_MEASURE_CACHE: usize = 8192;
const MAX_INLINE_SVG_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct LayoutFailure {
    pub node: Option<usize>,
    pub reason: &'static str,
}

// Unlike percentage margins and padding, vertical sizes use the containing
// block's definite content height. An auto-height ancestor breaks this chain.
fn height_length(length: Length, containing_height: Option<f32>) -> Option<f32> {
    match length {
        Length::Auto => None,
        Length::Px(value) => Some(value),
        Length::Percent(value) => containing_height.map(|height| value * height),
    }
    .map(|height| height.clamp(0.0, MAX_EXTENT))
}

fn height_length_signed(length: Length, containing_height: Option<f32>) -> Option<f32> {
    match length {
        Length::Auto => None,
        Length::Px(value) => Some(value),
        Length::Percent(value) => containing_height.map(|height| value * height),
    }
    .map(|value| value.clamp(-MAX_EXTENT, MAX_EXTENT))
}

fn constrain_height(style: &ComputedStyle, height: f32, containing_height: Option<f32>) -> f32 {
    let maximum = height_length(style.max_height, containing_height).unwrap_or(MAX_EXTENT);
    let minimum = height_length(style.min_height, containing_height).unwrap_or(0.0);
    // CSS min-height wins if the minimum exceeds the maximum.
    height.min(maximum).max(minimum).clamp(0.0, MAX_EXTENT)
}

#[derive(Clone, Copy, Default, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

fn background_box(
    r: Rect,
    border: [f32; 4],
    padding: [f32; 4],
    kind: crate::style::BackgroundBox,
) -> Option<Rect> {
    use crate::style::BackgroundBox;
    let edges = match kind {
        BackgroundBox::Border => [0.0; 4],
        BackgroundBox::Padding => border,
        BackgroundBox::Content => padding,
        BackgroundBox::Unsupported => return None,
    };
    Some(Rect {
        x: r.x + edges[3],
        y: r.y + edges[0],
        w: (r.w - edges[1] - edges[3]).max(0.0),
        h: (r.h - edges[0] - edges[2]).max(0.0),
    })
}

#[derive(Clone)]
enum Paint {
    Background(Rect, usize, [f32; 4]),
    Rect(Rect, Color),
    Text(Rect, usize, String),
    Image(Rect, ImageSource, Option<(usize, String)>),
    Control(Rect, usize, String, bool),
}

#[derive(Clone, Hash, PartialEq, Eq)]
enum ImageSource {
    Resource(String),
    InlineSvg(usize),
}

impl Paint {
    fn rect_mut(&mut self) -> &mut Rect {
        match self {
            Self::Background(r, ..)
            | Self::Rect(r, _)
            | Self::Text(r, ..)
            | Self::Image(r, ..)
            | Self::Control(r, ..) => r,
        }
    }
}

impl Rect {
    /// Match software paint's logical-edge contract for explicit clip paths.
    /// Keep the historical unclipped layout/hit rounding otherwise unchanged.
    fn paint_edges(self) -> Self {
        Self {
            x: self.x.round(),
            y: self.y.round(),
            w: self.w.ceil().max(0.0),
            h: self.h.ceil().max(0.0),
        }
    }
    fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            w: ((self.x + self.w).min(other.x + other.w) - x).max(0.0),
            h: ((self.y + self.h).min(other.y + other.h) - y).max(0.0),
        }
    }
}

#[derive(Default)]
struct Scene {
    paints: Vec<Paint>,
    /// Exact DOM owner for every paint, including rect/background operations.
    /// Parallel to paints/clips so numeric stacking can move them atomically.
    paint_nodes: Vec<usize>,
    /// Parallel, bounded clip rectangles in document coordinates. None retains
    /// the historical full-viewport paint path without rounding changes.
    clips: Vec<Option<Rect>>,
    boxes: Vec<LayoutBox>,
    /// Fixed-position subtrees keep Inspector geometry without increasing the
    /// page's scroll range. Parallel to boxes, changed only for emitted ranges.
    box_scroll: Vec<bool>,
    hits: Vec<(Rect, Action)>,
    /// Emitting box/text owner, distinct from an inherited anchor action node.
    hit_nodes: Vec<usize>,
    /// Unclipped, fractional structural border boxes for containing blocks.
    /// Kept separate from clipped Inspector coverage and shifted with children.
    borders: Vec<(usize, Rect)>,
}

impl Scene {
    fn mark(&self) -> (usize, usize, usize, usize) {
        (
            self.paints.len(),
            self.boxes.len(),
            self.hits.len(),
            self.borders.len(),
        )
    }
    fn shift(&mut self, start: (usize, usize, usize, usize), dx: f32, dy: f32) {
        for op in &mut self.paints[start.0..] {
            let r = op.rect_mut();
            r.x += dx;
            r.y += dy;
        }
        for clip in self.clips[start.0..].iter_mut().flatten() {
            clip.x += dx;
            clip.y += dy;
        }
        for b in &mut self.boxes[start.1..] {
            b.x += dx.round() as i32;
            b.y += dy.round() as i32;
        }
        for (r, _) in &mut self.hits[start.2..] {
            r.x += dx;
            r.y += dy;
        }
        for (_, r) in &mut self.borders[start.3..] {
            r.x += dx;
            r.y += dy;
        }
    }
    fn box_for(&mut self, node: usize, r: Rect) {
        if self.boxes.len() < MAX_OPS {
            self.boxes.push(LayoutBox {
                node,
                x: r.x.round() as i32,
                y: r.y.round() as i32,
                width: r.w.ceil().max(0.0) as u32,
                height: r.h.ceil().max(0.0) as u32,
            });
            self.box_scroll.push(true);
        }
    }
    fn paint(&mut self, node: usize, op: Paint) {
        if self.paints.len() < MAX_OPS {
            self.paints.push(op);
            self.clips.push(None);
            self.paint_nodes.push(node);
        }
    }
    fn hit(&mut self, node: usize, rect: Rect, action: Action) {
        if self.hits.len() < MAX_OPS {
            self.hits.push((rect, action));
            self.hit_nodes.push(node);
        }
    }
    fn clip(&mut self, start: (usize, usize, usize, usize), clip: Rect) {
        self.clip_range(start, self.mark(), clip);
    }
    fn clip_range(
        &mut self,
        start: (usize, usize, usize, usize),
        end: (usize, usize, usize, usize),
        clip: Rect,
    ) {
        let clip = clip.paint_edges();
        for current in &mut self.clips[start.0..end.0] {
            *current = Some(current.map_or(clip, |r| r.intersect(clip)));
        }
        for b in &mut self.boxes[start.1..end.1] {
            let r = Rect {
                x: b.x as f32,
                y: b.y as f32,
                w: b.width as f32,
                h: b.height as f32,
            }
            .intersect(clip);
            b.x = r.x.round() as i32;
            b.y = r.y.round() as i32;
            b.width = r.w.ceil().max(0.0) as u32;
            b.height = r.h.ceil().max(0.0) as u32;
        }
        for (r, _) in &mut self.hits[start.2..end.2] {
            if r.x < clip.x
                || r.y < clip.y
                || r.x + r.w > clip.x + clip.w
                || r.y + r.h > clip.y + clip.h
            {
                *r = r.paint_edges().intersect(clip);
            }
        }
    }
    fn border_for(&mut self, id: usize, r: Rect) {
        if self.borders.len() < MAX_OPS {
            self.borders.push((id, r));
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
struct Intrinsic {
    min: f32,
    max: f32,
}

#[derive(Clone)]
enum Token {
    Word {
        node: usize,
        text: String,
        width: f32,
    },
    Space {
        node: usize,
        width: f32,
    },
    Gap(f32),
    Box(usize),
    Block(usize),
    Break,
}

#[derive(Clone)]
struct Cell {
    node: usize,
    column: usize,
    span: usize,
}
struct Table {
    rows: Vec<(Option<usize>, Vec<Cell>)>,
    columns: usize,
}

#[cfg(test)]
fn render(
    document: &Document,
    fonts: &mut Fonts,
    viewport: Viewport,
    controls: &Controls<'_>,
    styles: Vec<ComputedStyle>,
) -> Option<Frame> {
    render_scaled(document, fonts, viewport, controls, styles, 1.0).ok()
}

pub(crate) fn render_scaled(
    document: &Document,
    fonts: &mut Fonts,
    viewport: Viewport,
    controls: &Controls<'_>,
    styles: Vec<ComputedStyle>,
    scale: f32,
) -> Result<Frame, LayoutFailure> {
    // Reject this path as a whole rather than silently dropping long text.
    if styles.len() != document.nodes.len()
        || document.nodes.iter().enumerate().any(|(id, node)| {
            if node.tag != "#text"
                || node.text.chars().take(MAX_TEXT_RUN + 1).count() <= MAX_TEXT_RUN
            {
                return false;
            }
            let mut ancestor = id;
            for _ in 0..=MAX_DEPTH {
                if styles[ancestor].display == Display::None {
                    return false;
                }
                if ancestor == 0 {
                    return true;
                }
                ancestor = document.nodes[ancestor].parent;
            }
            true
        })
    {
        return Err(LayoutFailure {
            node: None,
            reason: "document/style or text-run admission limit",
        });
    }
    let viewport_overflow = viewport_overflow_node(document, &styles);
    let mut layout = Layout {
        document,
        fonts,
        controls,
        styles,
        viewport,
        viewport_overflow,
        scene: Scene::default(),
        intrinsic: HashMap::new(),
        content_intrinsic_cache: HashMap::new(),
        content_height_cache: HashMap::new(),
        natural_images: HashMap::new(),
        natural_svg: HashMap::new(),
        inline_svg: HashMap::new(),
        inline_svg_bytes: 0,
        inline_svg_limit_reported: false,
        diagnostics: Default::default(),
        table_cache: HashMap::new(),
        format_scratch: 0,
        replaced_nodes: Vec::new(),
        anonymous_tables: Vec::new(),
        budget: 2_000_000,
        exhausted: false,
        failure: None,
    };
    layout.prepare_replaced_nodes();
    anonymous_table::prepare(&mut layout);
    let background = document
        .nodes
        .iter()
        .enumerate()
        .find(|(_, n)| n.tag == "body")
        .map(|(id, _)| layout.styles[id].background_color)
        .filter(|color| color.alpha > 0.0)
        .unwrap_or(Color {
            rgb: 0xffffff,
            alpha: 1.0,
        });
    let height = layout
        .block(
            0,
            0.0,
            0.0,
            viewport.width as f32,
            Some(viewport.height as f32),
            None,
            0,
        )
        .1;
    positioned::run(&mut layout);
    stacking::run(&mut layout);
    if layout.exhausted
        || layout.scene.paints.len() >= MAX_OPS
        || layout.scene.boxes.len() >= MAX_OPS
        || layout.scene.borders.len() >= MAX_OPS
    {
        let (node, reason) = layout
            .failure
            .unwrap_or((0, "layout work/depth or scene admission limit"));
        return Err(LayoutFailure {
            node: Some(node),
            reason,
        });
    }
    // Explicit block heights can be smaller than their visible contents. Keep
    // that laid-out overflow reachable even when html/body has height:100%.
    let height = layout
        .scene
        .boxes
        .iter()
        .zip(&layout.scene.box_scroll)
        .fold(height, |height, (rect, contributes)| {
            if *contributes && rect.width != 0 && rect.height != 0 {
                height.max(rect.y as f32 + rect.height as f32)
            } else {
                height
            }
        })
        .clamp(0.0, MAX_EXTENT);
    let mut canvas = Canvas::new_scaled(viewport.width, viewport.height, background.rgb, scale);
    let mut decoded = HashMap::new();
    let mut decoded_bytes = 0usize;
    let full_clip = canvas.intersect_clip(0, 0, viewport.width, viewport.height);
    for (index, op) in layout.scene.paints.iter().enumerate() {
        canvas.restore_clip(full_clip);
        if let Some(clip) = layout.scene.clips[index] {
            canvas.intersect_clip(
                clip.x.round() as i32,
                clip.y.round() as i32 - viewport.scroll,
                clip.w.ceil().max(0.0) as u32,
                clip.h.ceil().max(0.0) as u32,
            );
        }
        match op {
            Paint::Background(rect, node, padding) => {
                if !visible(*rect, viewport) {
                    continue;
                }
                let style = &layout.styles[*node];
                let area = |kind| background_box(*rect, style.border_width, *padding, kind);
                if let Some(color_box) = area(style.background_clip) {
                    paint_rect(
                        &mut canvas,
                        color_box,
                        style.background_color,
                        viewport.scroll,
                    );
                }
                if let Some(gradient) = &style.background_gradient
                    && let (Some(origin), Some(clip)) = (area(gradient.origin), area(gradient.clip))
                {
                    crate::gradient::paint(
                        &mut canvas,
                        [origin.x, origin.y, origin.w, origin.h],
                        [clip.x, clip.y, clip.w, clip.h],
                        gradient,
                        viewport.scroll,
                    );
                }
            }
            Paint::Rect(rect, color) => paint_rect(&mut canvas, *rect, *color, viewport.scroll),
            Paint::Text(rect, node, text) => {
                if !visible(*rect, viewport) {
                    continue;
                }
                let style = &layout.styles[*node];
                canvas.text_weight(
                    layout.fonts,
                    (
                        rect.x.round() as i32,
                        rect.y.round() as i32 - viewport.scroll,
                    ),
                    text,
                    style.font_size,
                    style.color.rgb,
                    style.font_weight >= 600,
                );
                if style.underline {
                    canvas.rect(
                        rect.x.round() as i32,
                        (rect.y + style.font_size).round() as i32 - viewport.scroll,
                        rect.w.ceil() as u32,
                        1,
                        style.color.rgb,
                    );
                }
            }
            Paint::Image(rect, source, fallback) => {
                if !visible(*rect, viewport) || rect.w <= 0.0 || rect.h <= 0.0 {
                    continue;
                }
                let key = (source.clone(), rect.w.ceil() as u32, rect.h.ceil() as u32);
                let resource = match source {
                    ImageSource::Resource(url) => document.resources.get(url).map(|resource| {
                        (resource.bytes.as_slice(), resource.content_type.as_str())
                    }),
                    ImageSource::InlineSvg(id) => layout
                        .inline_svg
                        .get(id)
                        .and_then(|source| source.as_deref())
                        .map(|bytes| (bytes, "image/svg+xml")),
                };
                if !decoded.contains_key(&key)
                    && let Some((bytes, mime)) = resource
                {
                    if image_cache_admits(key.1, key.2, decoded_bytes, decoded.len()) {
                        // The admitted base source is the cache/structure gate.
                        // Inline SVG gets its actual CSS viewport only on this
                        // decoded-cache miss, so viewBox meet/slice/none stays
                        // the SVG renderer's job. External images are unchanged.
                        let result = match source {
                            ImageSource::InlineSvg(id) => {
                                crate::inline_svg::source_with_styles_at_size(
                                    document,
                                    *id,
                                    &layout.styles,
                                    key.1,
                                    key.2,
                                )
                                .and_then(|source| {
                                    crate::images::decode(
                                        &source,
                                        "image/svg+xml",
                                        Some(key.1),
                                        Some(key.2),
                                    )
                                })
                            }
                            ImageSource::Resource(_) => {
                                crate::images::decode(bytes, mime, Some(key.1), Some(key.2))
                            }
                        };
                        let image = match result {
                            Ok(image) => Some(image),
                            Err(error) => {
                                let (source_url, node) = match source {
                                    ImageSource::Resource(url) => {
                                        (url.as_str(), fallback.as_ref().map(|(id, _)| *id))
                                    }
                                    ImageSource::InlineSvg(id) => {
                                        (document.base_url.as_str(), Some(*id))
                                    }
                                };
                                layout.diagnostics.record(
                                    "image-unsupported",
                                    format_args!("{error}"),
                                    format_args!("{source_url}"),
                                    node,
                                    None,
                                );
                                None
                            }
                        };
                        if let Some(image) = &image {
                            decoded_bytes += image.pixels.len();
                        }
                        decoded.insert(key.clone(), image);
                    }
                }
                if let Some(Some(image)) = decoded.get(&key) {
                    crate::images::blit(
                        &mut canvas,
                        image,
                        rect.x.round() as i32,
                        rect.y.round() as i32 - viewport.scroll,
                    );
                } else if let Some((node, alt)) = fallback {
                    paint_rect(
                        &mut canvas,
                        *rect,
                        Color {
                            rgb: 0xdddddd,
                            alpha: 1.0,
                        },
                        viewport.scroll,
                    );
                    let s = &layout.styles[*node];
                    let alt = super::render::fit_tail(layout.fonts, alt, s.font_size, rect.w);
                    canvas.text(
                        layout.fonts,
                        rect.x.round() as i32,
                        rect.y.round() as i32 - viewport.scroll,
                        &alt,
                        s.font_size,
                        s.color.rgb,
                    );
                }
            }
            Paint::Control(rect, node, text, submit) => {
                if !visible(*rect, viewport) {
                    continue;
                }
                let focused = controls.focused_input == Some(*node);
                let y = rect.y.round() as i32 - viewport.scroll;
                canvas.rect(
                    rect.x.round() as i32,
                    y,
                    rect.w.ceil() as u32,
                    rect.h.ceil() as u32,
                    if focused { 0x277453 } else { 0x888888 },
                );
                canvas.rect(
                    rect.x.round() as i32 + 1,
                    y + 1,
                    (rect.w - 2.0).max(0.0) as u32,
                    (rect.h - 2.0).max(0.0) as u32,
                    if *submit { 0xeeeeee } else { 0xffffff },
                );
                let style = &layout.styles[*node];
                let shown =
                    super::render::fit_tail(layout.fonts, text, style.font_size, rect.w - 8.0);
                if focused && controls.select_all {
                    canvas.rect(
                        rect.x.round() as i32 + 3,
                        y + 2,
                        layout.fonts.width(&shown, style.font_size).ceil() as u32,
                        (rect.h - 4.0).max(0.0) as u32,
                        0xc6dfed,
                    );
                }
                canvas.text_weight(
                    layout.fonts,
                    (rect.x.round() as i32 + 3, y + 2),
                    &shown,
                    style.font_size,
                    style.color.rgb,
                    style.font_weight >= 600,
                );
            }
        }
    }
    canvas.restore_clip(full_clip);
    let hits = layout
        .scene
        .hits
        .into_iter()
        .filter_map(|(r, action)| {
            let top = (r.y - viewport.scroll as f32).max(0.0);
            let bottom = (r.y + r.h - viewport.scroll as f32).min(viewport.height as f32);
            let left = r.x.max(0.0);
            let right = (r.x + r.w).min(viewport.width as f32);
            (bottom > top && right > left).then(|| Hit {
                x: left.floor() as i32,
                y: top.floor() as i32,
                w: (right - left).ceil() as u32,
                h: (bottom - top).ceil() as u32,
                action,
            })
        })
        .collect();
    // Existing Chassis/CDP expects viewport-relative boxes, including offscreen boxes.
    for b in &mut layout.scene.boxes {
        b.y -= viewport.scroll;
    }
    Ok(Frame {
        canvas,
        hits,
        boxes: layout.scene.boxes,
        content_height: height.ceil().max(0.0) as i32,
        diagnostics: layout.diagnostics,
    })
}

fn visible(r: Rect, v: Viewport) -> bool {
    r.x + r.w > 0.0
        && r.x < v.width as f32
        && r.y + r.h > v.scroll as f32
        && r.y < (v.scroll + v.height as i32) as f32
}

fn image_cache_admits(width: u32, height: u32, bytes: usize, entries: usize) -> bool {
    width <= 2048
        && height <= 2048
        && u64::from(width) * u64::from(height) <= 1024 * 1024
        && entries < 128
        && u64::from(width) * u64::from(height) * 4
            <= (16 * 1024 * 1024usize).saturating_sub(bytes) as u64
}

fn paint_rect(canvas: &mut Canvas, r: Rect, color: Color, scroll: i32) {
    if color.alpha <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    if color.alpha >= 1.0 {
        canvas.rect(
            r.x.round() as i32,
            r.y.round() as i32 - scroll,
            r.w.ceil() as u32,
            r.h.ceil() as u32,
            color.rgb,
        );
    } else {
        let (clip_left, clip_top, clip_right, clip_bottom) = canvas.physical_clip_bounds();
        let left = canvas
            .physical_edge(r.x.round() as i64)
            .clamp(i64::from(clip_left), i64::from(clip_right)) as usize;
        let top = canvas
            .physical_edge((r.y.round() - scroll as f32) as i64)
            .clamp(i64::from(clip_top), i64::from(clip_bottom)) as usize;
        let right = canvas
            .physical_edge((r.x + r.w).ceil() as i64)
            .clamp(i64::from(clip_left), i64::from(clip_right)) as usize;
        let bottom = canvas
            .physical_edge((r.y + r.h - scroll as f32).ceil() as i64)
            .clamp(i64::from(clip_top), i64::from(clip_bottom)) as usize;
        for y in top..bottom {
            for x in left..right {
                let pixel = &mut canvas.pixels[y * canvas.width as usize + x];
                let channel = |shift: u32| {
                    (((color.rgb >> shift) & 255u32) as f32 * color.alpha
                        + ((*pixel >> shift) & 255u32) as f32 * (1.0 - color.alpha))
                        .round() as u32
                };
                *pixel = channel(16) << 16 | channel(8) << 8 | channel(0);
            }
        }
    }
}

struct Layout<'a, 'c> {
    document: &'a Document,
    fonts: &'a mut Fonts,
    controls: &'a Controls<'c>,
    styles: Vec<ComputedStyle>,
    viewport: Viewport,
    viewport_overflow: Option<usize>,
    scene: Scene,
    intrinsic: HashMap<usize, Intrinsic>,
    // Pass-local, at most8192 entries each (under2MiB of conservative table
    // storage including spare capacity). Depth stays in the key so a shallower
    // measurement cannot bypass the recursive admission at a deeper call site.
    content_intrinsic_cache: HashMap<(usize, usize), Intrinsic>,
    content_height_cache: HashMap<(usize, u32, Option<u32>, usize), f32>,
    natural_images: HashMap<String, (f32, f32)>,
    natural_svg: HashMap<usize, (f32, f32)>,
    inline_svg: HashMap<usize, Option<Vec<u8>>>,
    inline_svg_bytes: usize,
    inline_svg_limit_reported: bool,
    diagnostics: crate::style::StyleDiagnostics,
    table_cache: HashMap<usize, Table>,
    /// Conservatively charged live formatting-context scratch, including
    /// contexts suspended while measuring descendants. Released with each pass.
    format_scratch: usize,
    /// Shared discriminator for every measurement/paint path. Classify once so
    /// repeated callbacks never rescan arbitrarily many button children.
    replaced_nodes: Vec<bool>,
    /// Pure orphan-cell containers admitted once per pass, without DOM changes.
    anonymous_tables: Vec<bool>,
    budget: usize,
    exhausted: bool,
    failure: Option<(usize, &'static str)>,
}

/// CSS Overflow 3 viewport propagation: the root supplies overflow, except
/// an HTML root visible on both axes uses its first displayed body child.
/// That one supplier is not an element clip: the output surface clips at the
/// viewport after scrolling. Ordinary descendant scroll containers stay clipped.
fn viewport_overflow_node(document: &Document, styles: &[ComputedStyle]) -> Option<usize> {
    use crate::style::Overflow;
    let root = document
        .nodes
        .first()?
        .children
        .iter()
        .copied()
        .find(|&id| document.nodes[id].tag == "html")?;
    if styles[root].layout.overflow == [Overflow::Visible, Overflow::Visible] {
        if let Some(body) = document.nodes[root]
            .children
            .iter()
            .copied()
            .find(|&id| document.nodes[id].tag == "body")
            .filter(|&id| !matches!(styles[id].display, Display::None | Display::Contents))
        {
            return Some(body);
        }
    }
    Some(root)
}

impl Layout<'_, '_> {
    fn prepare_replaced_nodes(&mut self) {
        self.replaced_nodes.resize(self.document.nodes.len(), false);
        for id in 0..self.document.nodes.len() {
            if !self.spend(0) {
                self.fail(id, "replaced-content classification work budget exhausted");
                return;
            }
            let tag = self.document.nodes[id].tag.as_str();
            self.replaced_nodes[id] =
                matches!(tag, "img" | "svg" | "input" | "textarea" | "button");
            if tag == "button" {
                for index in 0..self.document.nodes[id].children.len() {
                    if !self.spend(0) {
                        self.fail(id, "button-content classification work budget exhausted");
                        return;
                    }
                    let child = self.document.nodes[id].children[index];
                    if self.document.nodes[child].tag != "#text" {
                        self.replaced_nodes[id] = false;
                        break;
                    }
                }
            }
        }
    }

    fn is_replaced(&self, id: usize) -> bool {
        self.replaced_nodes[id]
    }

    fn is_structured_button(&self, id: usize) -> bool {
        self.document.nodes[id].tag == "button" && !self.is_replaced(id)
    }

    fn prepare_inline_svg(&mut self, id: usize) {
        if self.inline_svg.contains_key(&id) {
            return;
        }
        if self.inline_svg.len() >= 128 || self.inline_svg_bytes >= MAX_INLINE_SVG_BYTES {
            if !self.inline_svg_limit_reported {
                self.diagnostics.record(
                    "image-unsupported",
                    format_args!("Inline SVG source-cache admission limit"),
                    format_args!("{}", self.document.base_url),
                    Some(id),
                    None,
                );
                self.inline_svg_limit_reported = true;
            }
            return;
        }
        let result = crate::inline_svg::source_with_styles(self.document, id, &self.styles);
        let result = result.and_then(|bytes| {
            if bytes.len() > MAX_INLINE_SVG_BYTES.saturating_sub(self.inline_svg_bytes) {
                Err("Inline SVG source-cache byte limit".to_owned())
            } else {
                Ok(bytes)
            }
        });
        let source = match result {
            Ok(bytes) => {
                self.inline_svg_bytes += bytes.len();
                Some(bytes)
            }
            Err(error) => {
                self.diagnostics.record(
                    "image-unsupported",
                    format_args!("{error}"),
                    format_args!("{}", self.document.base_url),
                    Some(id),
                    None,
                );
                None
            }
        };
        if self.inline_svg.len() < 128 {
            self.inline_svg.insert(id, source);
        }
    }
    fn fail(&mut self, node: usize, reason: &'static str) {
        self.failure.get_or_insert((node, reason));
        self.exhausted = true;
    }
    fn border_box(&self, id: usize) -> Option<Rect> {
        self.scene
            .borders
            .iter()
            .rev()
            .find(|(node, _)| *node == id)
            .map(|(_, r)| *r)
    }
    /// Overflow clips descendants at the padding edge. An unconstrained axis
    /// remains unbounded within this renderer's finite coordinate envelope.
    fn clip_for(&self, id: usize, r: Rect) -> Option<Rect> {
        use crate::style::Overflow;
        let s = &self.styles[id];
        // The legacy automatic-table path retains its established line/cell
        // metrics. Table-cell clipping awaits that formatting model's own
        // acceptance, rather than cutting glyphs at an approximate cell edge.
        // Style diagnostics explicitly report this remaining boundary.
        if self.viewport_overflow == Some(id) || s.display == Display::TableCell {
            return None;
        }
        let x = !matches!(s.layout.overflow[0], Overflow::Visible);
        let y = !matches!(s.layout.overflow[1], Overflow::Visible);
        if !x && !y {
            return None;
        }
        let b = s.border_width;
        Some(Rect {
            x: if x { r.x + b[3] } else { -MAX_EXTENT },
            y: if y { r.y + b[0] } else { -MAX_EXTENT },
            w: if x {
                (r.w - b[1] - b[3]).max(0.0)
            } else {
                MAX_EXTENT * 2.0
            },
            h: if y {
                (r.h - b[0] - b[2]).max(0.0)
            } else {
                MAX_EXTENT * 2.0
            },
        })
    }

    fn spend(&mut self, depth: usize) -> bool {
        if depth > MAX_DEPTH || self.budget == 0 {
            self.exhausted = true;
            false
        } else {
            self.budget -= 1;
            true
        }
    }

    /// Content min/max widths for a future formatting-context adapter. This
    /// node's CSS width/min-width and box edges are excluded; descendant outer
    /// contributions retain their own constraints. Replaced nodes use their
    /// natural content size. The document/style/text admission performed by
    /// `render_scaled` is a precondition, as it is for the paint path.
    ///
    /// All measurement borrows this pass's fonts, caches and cumulative work
    /// budget. None means the entire pass must be discarded, not a zero-sized
    /// leaf to cache. These entry points do not allocate or write a Scene.
    fn measure_content_intrinsic(&mut self, id: usize, depth: usize) -> Option<Intrinsic> {
        if self.exhausted || !self.spend(depth) {
            return None;
        }
        if let Some(size) = self.content_intrinsic_cache.get(&(id, depth)) {
            return Some(*size);
        }
        let size = if self.styles[id].display == Display::None {
            Intrinsic::default()
        } else {
            self.content_intrinsic(id, depth, true, &self.styles[id].clone())
        };
        if self.exhausted {
            return None;
        }
        let size = Intrinsic {
            min: size.min.clamp(0.0, MAX_EXTENT),
            max: size.max.clamp(0.0, MAX_EXTENT),
        };
        if self.content_intrinsic_cache.len() < MAX_MEASURE_CACHE {
            self.content_intrinsic_cache.insert((id, depth), size);
        }
        Some(size)
    }

    // Fit-content's inline contribution preserves a replaced element's
    // definite opposite-axis size. Keep this outside the natural-size cache:
    // percentage heights and border-box edges depend on this containing block.
    fn measure_fit_content_intrinsic(
        &mut self,
        id: usize,
        basis: f32,
        containing_height: Option<f32>,
        depth: usize,
    ) -> Option<Intrinsic> {
        let measured = self.measure_content_intrinsic(id, depth)?;
        if !matches!(self.document.nodes[id].tag.as_str(), "img" | "svg") {
            return Some(measured);
        }
        let natural = self.natural_replaced_size(id, true);
        let width = self.replaced_intrinsic_width(id, basis, containing_height, natural);
        Some(Intrinsic {
            min: width,
            max: width,
        })
    }

    fn replaced_intrinsic_width(
        &self,
        id: usize,
        basis: f32,
        containing_height: Option<f32>,
        (natural_width, natural_height): (f32, f32),
    ) -> f32 {
        let style = &self.styles[id];
        if matches!(self.document.nodes[id].tag.as_str(), "img" | "svg")
            && natural_height > 0.0
            && let Some(height) = height_length(style.height, containing_height)
        {
            let (_, padding) = self.edges(id, basis);
            let edges = if style.layout.box_sizing == crate::style::BoxSizing::BorderBox {
                padding[0] + padding[2]
            } else {
                0.0
            };
            let content_height =
                (constrain_height(style, height, containing_height) - edges).max(0.0);
            return (content_height * natural_width / natural_height).clamp(0.0, MAX_EXTENT);
        }
        natural_width
    }

    /// Wrapped content height at a definite *content-box* width. This omits this
    /// node's sizing/edges. The optional definite content height is only the
    /// percentage-height basis for children; it does not force the result.
    /// Text leaves, block/inline flow and tables share the paint calculations.
    /// Replaced nodes use `measure_natural_replaced` instead: the adapter must
    /// apply its known dimensions/aspect ratio once, not reapply Mg box sizing.
    fn measure_content_height(
        &mut self,
        id: usize,
        content_width: f32,
        definite_content_height: Option<f32>,
        depth: usize,
    ) -> Option<f32> {
        if self.exhausted
            || !measurement_dimension(content_width)
            || definite_content_height.is_some_and(|height| !measurement_dimension(height))
            || !self.spend(depth)
        {
            return None;
        }
        let key = (
            id,
            content_width.to_bits(),
            definite_content_height.map(f32::to_bits),
            depth,
        );
        if let Some(height) = self.content_height_cache.get(&key) {
            return Some(*height);
        }
        let height = if self.styles[id].display == Display::None {
            0.0
        } else if self.document.nodes[id].tag != "#text" && is_format(self.styles[id].display) {
            formatting::run::<false>(
                self,
                id,
                0.0,
                0.0,
                taffy::AvailableSpace::Definite(content_width),
                definite_content_height,
                depth + 1,
            )?
            .height
        } else if self.is_replaced(id) {
            return None;
        } else if matches!(
            self.styles[id].display,
            Display::Table | Display::InlineTable
        ) {
            self.layout_table::<false>(
                id,
                0.0,
                0.0,
                content_width,
                definite_content_height,
                depth + 1,
            )
        } else if anonymous_table::contains(self, id) {
            anonymous_table::run::<false>(
                self,
                id,
                0.0,
                0.0,
                content_width,
                definite_content_height,
                depth + 1,
            )
        } else if self.document.nodes[id].tag == "#text" {
            let mut tokens = Vec::new();
            self.tokens(id, &mut tokens, depth + 1);
            self.flow_tokens::<false>(
                id,
                0.0,
                0.0,
                content_width,
                definite_content_height,
                tokens,
                depth + 1,
            )
        } else {
            self.flow::<false>(
                id,
                0.0,
                0.0,
                content_width,
                definite_content_height,
                depth + 1,
            )
        };
        if self.exhausted {
            return None;
        }
        if self.content_height_cache.len() < MAX_MEASURE_CACHE {
            self.content_height_cache.insert(key, height);
        }
        Some(height)
    }

    /// Natural replaced *content-box* dimensions, independent of CSS sizing and
    /// edges. Image decoding uses the same bounded, pass-local natural-size
    /// cache as paint; an unsupported resource retains the existing placeholder.
    fn measure_natural_replaced(&mut self, id: usize, depth: usize) -> Option<(f32, f32)> {
        if self.exhausted || !self.spend(depth) {
            return None;
        }
        if !self.is_replaced(id) {
            return None;
        }
        Some(self.natural_replaced_size(id, true))
    }

    /// Existing block/table/replaced layout as a measured *outer* size, including
    /// margins. Available width resolves percentage edges; containing height
    /// resolves percentage heights. Forced width is a border-box input, still
    /// subject to the legacy node's min/max width rules. The origin is zero;
    /// final positioned painting must keep its own fractional coordinates.
    #[allow(dead_code)] // Internal adapter boundary, exercised independently below.
    fn measure_outer(
        &mut self,
        id: usize,
        available_width: f32,
        containing_height: Option<f32>,
        forced_border_width: Option<f32>,
        depth: usize,
    ) -> Option<(f32, f32)> {
        if self.exhausted
            || !measurement_dimension(available_width)
            || containing_height.is_some_and(|height| !measurement_dimension(height))
            || forced_border_width.is_some_and(|width| !measurement_dimension(width))
        {
            return None;
        }
        let size = self.block_layout::<false>(
            id,
            0.0,
            0.0,
            available_width,
            containing_height,
            forced_border_width,
            depth,
        );
        (!self.exhausted).then_some(size)
    }
    fn line_height(&self, node: usize) -> f32 {
        let s = &self.styles[node];
        s.line_height
            .unwrap_or_else(|| self.fonts.css_line_height(s.font_size))
            .clamp(0.0, 1024.0)
    }
    fn edges(&self, node: usize, basis: f32) -> ([f32; 4], [f32; 4]) {
        let s = &self.styles[node];
        let m = s
            .margin
            .map(|v| v.resolve(basis).unwrap_or(0.0).clamp(-1024.0, MAX_EXTENT));
        let mut p = s
            .padding
            .map(|v| v.resolve(basis).unwrap_or(0.0).clamp(0.0, MAX_EXTENT));
        for (i, p) in p.iter_mut().enumerate() {
            *p += s.border_width[i].clamp(0.0, 1024.0);
        }
        (m, p)
    }
    fn action(&self, mut id: usize) -> Option<Action> {
        for _ in 0..MAX_DEPTH {
            let node = &self.document.nodes[id];
            if self.is_structured_button(id) {
                return Some(Action::Submit(id));
            }
            if node.tag == "a"
                && let Some(href) = node.attr("href")
            {
                let url = url::Url::parse(&self.document.base_url)
                    .and_then(|u| u.join(href))
                    .ok()?;
                if matches!(url.scheme(), "http" | "https") {
                    return Some(Action::Link {
                        node: id,
                        href: url.to_string(),
                    });
                }
                return None;
            }
            if id == 0 {
                return None;
            }
            id = node.parent;
        }
        None
    }
    fn hit(&mut self, id: usize, r: Rect) {
        if self.styles[id].visible
            && self.styles[id].pointer_events
            && let Some(action) = self.action(id)
        {
            if let Action::Link { node, .. } = &action {
                self.scene.box_for(*node, r);
            }
            self.scene.hit(id, r, action);
        }
    }
    fn decorate(&mut self, id: usize, r: Rect, padding: [f32; 4]) {
        let s = &self.styles[id];
        if !s.visible {
            return;
        }
        self.scene.paint(id, Paint::Background(r, id, padding));
        let b = s.border_width;
        let edges = [
            Rect { h: b[0], ..r },
            Rect {
                x: r.x + r.w - b[1],
                w: b[1],
                ..r
            },
            Rect {
                y: r.y + r.h - b[2],
                h: b[2],
                ..r
            },
            Rect { w: b[3], ..r },
        ];
        for (i, edge) in edges.into_iter().enumerate() {
            self.scene.paint(id, Paint::Rect(edge, s.border_color[i]));
        }
        for bg in s.background_images.iter().rev() {
            let w = bg.width.resolve(r.w).unwrap_or(r.w);
            let h = bg.height.resolve(r.h).unwrap_or(w);
            let url = url::Url::parse(&bg.url)
                .map(|mut url| {
                    url.set_fragment(None);
                    url.to_string()
                })
                .unwrap_or_else(|_| bg.url.clone());
            self.scene.paint(
                id,
                Paint::Image(Rect { w, h, ..r }, ImageSource::Resource(url), None),
            );
        }
    }
    fn image_url(&self, id: usize) -> Option<String> {
        let reference = self.document.nodes[id].attr("src")?;
        url::Url::parse(&self.document.base_url)
            .and_then(|u| u.join(reference))
            .ok()
            .map(|mut url| {
                url.set_fragment(None);
                url.into()
            })
    }
    fn replaced_size(
        &mut self,
        id: usize,
        basis: f32,
        containing_height: Option<f32>,
    ) -> (f32, f32) {
        let (width, mut height) = self.replaced_size_base(id, basis, containing_height);
        if !self.styles[id].layout.max_width_fit_content
            && self.styles[id].layout.min_width_intrinsic
                != Some(crate::style::IntrinsicSize::FitContent)
        {
            return (width, height);
        }
        // Replaced content has equal intrinsic minimum/maximum widths. The
        // keyword constrains content independently of the declared sizing box.
        let (natural, natural_height) = self.natural_replaced_size(id, true);
        let intrinsic =
            self.replaced_intrinsic_width(id, basis, containing_height, (natural, natural_height));
        let (_, padding) = self.edges(id, basis);
        let s = &self.styles[id];
        let edges = if s.layout.box_sizing == crate::style::BoxSizing::BorderBox {
            padding[1] + padding[3]
        } else {
            0.0
        };
        let minimum =
            if s.layout.min_width_intrinsic == Some(crate::style::IntrinsicSize::FitContent) {
                intrinsic
            } else {
                (s.min_width.resolve(basis).unwrap_or(0.0) - edges).max(0.0)
            };
        let maximum = if s.layout.max_width_fit_content {
            intrinsic
        } else {
            MAX_EXTENT
        };
        let constrained = width.min(maximum).max(minimum).clamp(0.0, MAX_EXTENT);
        if matches!(self.document.nodes[id].tag.as_str(), "img" | "svg")
            && height_length(s.height, containing_height).is_none()
            && natural > 0.0
        {
            let vertical = if s.layout.box_sizing == crate::style::BoxSizing::BorderBox {
                padding[0] + padding[2]
            } else {
                0.0
            };
            height = (constrain_height(
                s,
                natural_height * constrained / natural + vertical,
                containing_height,
            ) - vertical)
                .max(0.0);
        }
        (constrained, height)
    }
    fn replaced_size_base(
        &mut self,
        id: usize,
        basis: f32,
        containing_height: Option<f32>,
    ) -> (f32, f32) {
        if self.styles[id].layout.box_sizing == crate::style::BoxSizing::BorderBox {
            let (_, p) = self.edges(id, basis);
            let horizontal = p[1] + p[3];
            let vertical = p[0] + p[2];
            let width = self.styles[id]
                .width
                .resolve(basis)
                .map(|n| (n - horizontal).max(0.0));
            let height = height_length(self.styles[id].height, containing_height)
                .map(|n| (n - vertical).max(0.0));
            let (natural_w, natural_h) = if width.is_some() && height.is_some() {
                (0.0, 0.0)
            } else {
                self.natural_replaced_size(id, width != Some(0.0) && height != Some(0.0))
            };
            let ratio = (natural_w / natural_h.max(1.0)).max(0.001);
            let is_image = matches!(self.document.nodes[id].tag.as_str(), "img" | "svg");
            let s = &self.styles[id];
            let mut w = width.unwrap_or_else(|| {
                if is_image {
                    height.map_or(natural_w, |h| h * ratio)
                } else {
                    natural_w
                }
            });
            let max_w = s
                .max_width
                .resolve(basis)
                .map(|n| (n - horizontal).max(0.0))
                .unwrap_or(MAX_EXTENT);
            let min_w = s
                .min_width
                .resolve(basis)
                .map(|n| (n - horizontal).max(0.0))
                .unwrap_or(0.0);
            w = w.min(max_w).max(min_w).clamp(0.0, MAX_EXTENT);
            let h = height.unwrap_or_else(|| {
                if is_image && width.is_some() {
                    w / ratio
                } else {
                    natural_h
                }
            });
            let h = (constrain_height(s, h + vertical, containing_height) - vertical).max(0.0);
            return (w, h);
        }
        let width = self.styles[id].width.resolve(basis);
        let height = height_length(self.styles[id].height, containing_height);
        if let (Some(width), Some(height)) = (width, height) {
            return (
                width.clamp(0.0, MAX_EXTENT),
                constrain_height(&self.styles[id], height, containing_height),
            );
        }
        let (default_w, default_h) =
            self.natural_replaced_size(id, width != Some(0.0) && height != Some(0.0));
        let node = &self.document.nodes[id];
        let s = &self.styles[id];
        if !matches!(node.tag.as_str(), "img" | "svg") {
            return (
                width.unwrap_or(default_w).clamp(0.0, MAX_EXTENT),
                constrain_height(s, height.unwrap_or(default_h), containing_height),
            );
        }
        let ratio = default_w / default_h.max(1.0);
        let used_height = constrain_height(
            s,
            height.unwrap_or_else(|| width.map_or(default_h, |w| w / ratio.max(0.001))),
            containing_height,
        );
        (
            width.unwrap_or(used_height * ratio).clamp(0.0, MAX_EXTENT),
            used_height,
        )
    }
    // Natural content dimensions only: no CSS sizes, margins, padding or borders.
    // The legacy paint path still skips decoding explicitly zero-sized images.
    fn natural_replaced_size(&mut self, id: usize, decode_image: bool) -> (f32, f32) {
        if self.document.nodes[id].tag == "svg" {
            if !decode_image {
                return (18.0, 18.0);
            }
            if let Some(size) = self.natural_svg.get(&id) {
                return *size;
            }
            if self.natural_images.len() + self.natural_svg.len() >= 128 {
                return (18.0, 18.0);
            }
            self.prepare_inline_svg(id);
            let decoded = self
                .inline_svg
                .get(&id)
                .and_then(|source| source.as_deref())
                .map(|source| crate::images::decode(source, "image/svg+xml", None, None));
            let size = match decoded {
                Some(Ok(image)) => (image.width as f32, image.height as f32),
                Some(Err(error)) => {
                    self.diagnostics.record(
                        "image-unsupported",
                        format_args!("{error}"),
                        format_args!("{}", self.document.base_url),
                        Some(id),
                        None,
                    );
                    self.inline_svg.insert(id, None);
                    (18.0, 18.0)
                }
                None => (18.0, 18.0),
            };
            if self.natural_images.len() + self.natural_svg.len() < 128 {
                self.natural_svg.insert(id, size);
            }
            return size;
        }
        let image_url = self.image_url(id);
        if self.document.nodes[id].tag == "img"
            && decode_image
            && let Some(url) = &image_url
            && !self.natural_images.contains_key(url)
            && self.natural_images.len() + self.natural_svg.len() < 128
        {
            let dimensions = self
                .document
                .resources
                .get(url)
                .and_then(|resource| {
                    crate::images::decode(&resource.bytes, &resource.content_type, None, None).ok()
                })
                .map(|image| (image.width as f32, image.height as f32))
                .unwrap_or((18.0, 18.0));
            self.natural_images.insert(url.clone(), dimensions);
        }
        let node = &self.document.nodes[id];
        let s = &self.styles[id];
        if node.tag == "img" {
            image_url
                .as_ref()
                .and_then(|url| self.natural_images.get(url))
                .copied()
                .unwrap_or((18.0, 18.0))
        } else if node.tag == "button"
            || node
                .attr("type")
                .is_some_and(|kind| matches!(kind, "submit" | "button" | "reset"))
        {
            let label = self
                .document
                .item_nodes
                .iter()
                .position(|&n| n == id)
                .and_then(|i| self.document.items.get(i))
                .and_then(|item| match item {
                    Item::Submit { label, .. } => Some(label.as_str()),
                    _ => None,
                })
                .unwrap_or("Submit");
            (
                self.fonts
                    .width_weight(label, s.font_size, s.font_weight >= 600),
                self.line_height(id),
            )
        } else {
            let size = node
                .attr("size")
                .and_then(|s| s.parse::<f32>().ok())
                .unwrap_or(20.0)
                .clamp(1.0, 1000.0);
            (
                self.fonts.width("0", s.font_size) * size,
                self.line_height(id),
            )
        }
    }
    fn intrinsic(&mut self, id: usize, depth: usize) -> Intrinsic {
        if let Some(size) = self.intrinsic.get(&id) {
            return *size;
        }
        if !self.spend(depth) || self.styles[id].display == Display::None {
            return Intrinsic::default();
        }
        let s = self.styles[id].clone();
        let mut result = self.content_intrinsic(id, depth, false, &s);
        let (margin, padding) = self.edges(id, 0.0);
        let edges = margin[1] + margin[3] + padding[1] + padding[3];
        let sizing_edges = if s.layout.box_sizing == crate::style::BoxSizing::BorderBox {
            padding[1] + padding[3]
        } else {
            0.0
        };
        if let Length::Px(width) = s.width {
            let width = (width - sizing_edges).max(0.0);
            result.min = result.min.max(width);
            result.max = result.max.max(width);
        }
        if let Length::Px(width) = s.min_width {
            let width = (width - sizing_edges).max(0.0);
            result.min = result.min.max(width);
            result.max = result.max.max(width);
        }
        result.min = (result.min + edges).clamp(0.0, MAX_EXTENT);
        result.max = (result.max + edges).max(result.min).min(MAX_EXTENT);
        self.intrinsic.insert(id, result);
        result
    }
    // The caller spends the current node. Descendant intrinsic contributions
    // retain their own CSS sizing/edges; this node's own constraints are omitted.
    fn content_intrinsic(
        &mut self,
        id: usize,
        depth: usize,
        natural_replaced: bool,
        s: &ComputedStyle,
    ) -> Intrinsic {
        if self.document.nodes[id].tag != "#text" && is_format(s.display) {
            let min = formatting::run::<false>(
                self,
                id,
                0.0,
                0.0,
                taffy::AvailableSpace::MinContent,
                None,
                depth + 1,
            );
            let max = formatting::run::<false>(
                self,
                id,
                0.0,
                0.0,
                taffy::AvailableSpace::MaxContent,
                None,
                depth + 1,
            );
            return match (min, max) {
                (Some(min), Some(max)) => Intrinsic {
                    min: min.width,
                    max: max.width,
                },
                _ => {
                    self.exhausted = true;
                    Intrinsic::default()
                }
            };
        }
        let node = &self.document.nodes[id];
        if node.tag == "#text" {
            let text = collapse(&node.text);
            let max = self
                .fonts
                .width_weight(&text, s.font_size, s.font_weight >= 600);
            let min = if matches!(s.white_space, WhiteSpace::NoWrap | WhiteSpace::Pre) {
                max
            } else {
                text.split(is_css_space)
                    .map(|word| {
                        self.fonts
                            .width_weight(word, s.font_size, s.font_weight >= 600)
                    })
                    .fold(0.0, f32::max)
            };
            Intrinsic { min, max }
        } else if self.is_replaced(id) {
            let (w, _) = if natural_replaced {
                self.natural_replaced_size(id, true)
            } else {
                self.replaced_size(id, 0.0, None)
            };
            Intrinsic { min: w, max: w }
        } else if matches!(s.display, Display::Table | Display::InlineTable)
            || anonymous_table::contains(self, id)
        {
            let (mins, maxs) = self.table_columns(id, depth + 1);
            let spacing = self.table_spacing(id)[0] * (mins.len() + 1) as f32;
            Intrinsic {
                min: mins.iter().sum::<f32>() + spacing,
                max: maxs.iter().sum::<f32>() + spacing,
            }
        } else {
            let children = node.children.clone();
            let mut current = 0.0;
            let mut result = Intrinsic::default();
            for child in children {
                if matches!(
                    self.styles[child].layout.position,
                    crate::style::Position::Absolute | crate::style::Position::Fixed
                ) {
                    continue;
                }
                // A content-only text measurement inherits typography, not an
                // element box. The legacy snapshot keeps parent box fields on
                // text nodes for other paths: do not count those edges twice.
                // Keep the historical outer/table intrinsic path unchanged.
                let v = if natural_replaced && self.document.nodes[child].tag == "#text" {
                    self.measure_content_intrinsic(child, depth + 1)
                        .unwrap_or_default()
                } else {
                    self.intrinsic(child, depth + 1)
                };
                result.min = result.min.max(v.min);
                if is_block(self.styles[child].display) {
                    result.max = result.max.max(current).max(v.max);
                    current = 0.0;
                } else {
                    current += v.max;
                }
            }
            result.max = result.max.max(current);
            if matches!(s.white_space, WhiteSpace::NoWrap | WhiteSpace::Pre) {
                result.min = result.max;
            }
            result
        }
    }
    fn table(&mut self, id: usize) -> &Table {
        if !self.table_cache.contains_key(&id) {
            let mut rows = Vec::new();
            let mut todo = self.document.nodes[id].children.clone();
            todo.reverse();
            let mut count = 0;
            while let Some(child) = todo.pop() {
                if self.styles[child].display == Display::None {
                    continue;
                }
                match self.styles[child].display {
                    Display::TableRow => {
                        let mut column = 0;
                        let mut cells = Vec::new();
                        for &cell in &self.document.nodes[child].children {
                            if self.styles[cell].display != Display::TableCell
                                || column >= MAX_COLUMNS
                            {
                                continue;
                            }
                            let span = self.document.nodes[cell]
                                .attr("colspan")
                                .and_then(|n| n.parse::<usize>().ok())
                                .unwrap_or(1)
                                .clamp(1, MAX_COLUMNS - column);
                            cells.push(Cell {
                                node: cell,
                                column,
                                span,
                            });
                            column += span;
                        }
                        count = count.max(column);
                        rows.push((Some(child), cells));
                    }
                    Display::TableRowGroup
                    | Display::TableHeaderGroup
                    | Display::TableFooterGroup => {
                        todo.extend(self.document.nodes[child].children.iter().rev())
                    }
                    _ => {}
                }
            }
            self.table_cache.insert(
                id,
                Table {
                    rows,
                    columns: count,
                },
            );
        }
        &self.table_cache[&id]
    }
    fn table_spacing(&self, id: usize) -> [f32; 2] {
        if self.styles[id].border_collapse {
            [0.0, 0.0]
        } else {
            self.styles[id].border_spacing.map(|n| n.clamp(0.0, 1024.0))
        }
    }
    fn table_columns(&mut self, id: usize, depth: usize) -> (Vec<f32>, Vec<f32>) {
        let columns = self.table(id).columns;
        let rows = self.table(id).rows.clone();
        let mut min = vec![0.0f32; columns];
        let mut max = min.clone();
        let mut spans = Vec::new();
        for (_, cells) in rows {
            for cell in cells {
                let intrinsic = self.intrinsic(cell.node, depth + 1);
                if cell.span == 1 {
                    min[cell.column] = min[cell.column].max(intrinsic.min);
                    max[cell.column] = max[cell.column].max(intrinsic.max);
                } else {
                    spans.push((cell, intrinsic));
                }
            }
        }
        for (cell, intrinsic) in spans {
            let range = cell.column..cell.column + cell.span;
            let spacing = self.table_spacing(id)[0] * (cell.span - 1) as f32;
            distribute_deficit(&mut min[range.clone()], (intrinsic.min - spacing).max(0.0));
            distribute_deficit(&mut max[range.clone()], (intrinsic.max - spacing).max(0.0));
            for i in range {
                max[i] = max[i].max(min[i]);
            }
        }
        (min, max)
    }

    /// Returns outer width and height, including margins.
    fn block(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        available: f32,
        containing_height: Option<f32>,
        forced: Option<f32>,
        depth: usize,
    ) -> (f32, f32) {
        self.block_layout::<true>(id, x, y, available, containing_height, forced, depth)
    }

    // One calculation path for paint and measurement. EMIT=false never touches
    // Scene: no temporary paint commands, rounded boxes, hits or scene shifts.
    fn block_layout<const EMIT: bool>(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        available: f32,
        containing_height: Option<f32>,
        forced: Option<f32>,
        depth: usize,
    ) -> (f32, f32) {
        if !self.spend(depth) || self.styles[id].display == Display::None {
            return (0.0, 0.0);
        }
        if self.is_replaced(id) {
            return self.replaced::<EMIT>(id, x, y, available, containing_height);
        }
        let s = self.styles[id].clone();
        let (mut margin, padding) = self.edges(id, available);
        let is_table = matches!(s.display, Display::Table | Display::InlineTable);
        let horizontal = padding[1] + padding[3];
        let sizing_edges = if is_table || s.layout.box_sizing == crate::style::BoxSizing::BorderBox
        {
            0.0
        } else {
            horizontal
        };
        let mut width = forced.unwrap_or_else(|| {
            s.width
                .resolve(available)
                .map(|w| w + sizing_edges)
                .unwrap_or_else(|| {
                    if s.layout.width_fit_content {
                        let Some(i) = self.measure_content_intrinsic(id, depth + 1) else {
                            return 0.0;
                        };
                        let stretch = (available - margin[1] - margin[3] - horizontal).max(0.0);
                        // Intrinsic width keywords describe the content box,
                        // independently of box-sizing. Numeric min/max below
                        // still use the declared sizing box exactly once.
                        i.max.min(i.min.max(stretch)) + horizontal
                    } else if is_table {
                        let i = self.intrinsic(id, depth + 1);
                        i.max.min(available).max(i.min)
                    } else {
                        (available - margin[1] - margin[3]).max(horizontal)
                    }
                })
        });
        if let Some(max) = s.max_width.resolve(available) {
            width = width.min(max + sizing_edges);
        }
        if s.layout.max_width_fit_content {
            let Some(intrinsic) = self.measure_content_intrinsic(id, depth + 1) else {
                return (0.0, 0.0);
            };
            let stretch = (available - margin[1] - margin[3] - horizontal).max(0.0);
            width = width.min(intrinsic.max.min(intrinsic.min.max(stretch)) + horizontal);
        }
        width = width.max(s.min_width.resolve(available).unwrap_or(0.0) + sizing_edges);
        if let Some(intrinsic) = s.layout.min_width_intrinsic {
            let measured = self
                .measure_content_intrinsic(id, depth + 1)
                .unwrap_or_default();
            let minimum = match intrinsic {
                crate::style::IntrinsicSize::MinContent => measured.min,
                crate::style::IntrinsicSize::MaxContent => measured.max,
                crate::style::IntrinsicSize::FitContent => measured.max.min(
                    measured
                        .min
                        .max((available - margin[1] - margin[3] - horizontal).max(0.0)),
                ),
            };
            width = width.max(minimum + horizontal);
        }
        width = width.max(horizontal).clamp(0.0, MAX_EXTENT);
        let extra = (available - width - margin[1] - margin[3]).max(0.0);
        match (s.margin[3], s.margin[1]) {
            (Length::Auto, Length::Auto) => {
                margin[3] = extra / 2.0;
                margin[1] = extra / 2.0;
            }
            (Length::Auto, _) => margin[3] = extra,
            (_, Length::Auto) => margin[1] = extra,
            _ if is_table && id != 0 => {
                let parent = self.document.nodes[id].parent;
                if self.styles[parent].text_align == TextAlign::Center {
                    margin[3] += extra / 2.0;
                }
            }
            _ => {}
        }
        let (dx, dy) = if s.layout.position == crate::style::Position::Relative {
            (
                s.layout.inset[3]
                    .resolve(available)
                    .or_else(|| s.layout.inset[1].resolve(available).map(|v| -v))
                    .unwrap_or(0.0),
                height_length_signed(s.layout.inset[0], containing_height)
                    .or_else(|| {
                        height_length_signed(s.layout.inset[2], containing_height).map(|v| -v)
                    })
                    .unwrap_or(0.0),
            )
        } else {
            (0.0, 0.0)
        };
        let bx = x + margin[3] + dx;
        let by = y + margin[0] + dy;
        // Reserve decoration before children so parent backgrounds stay behind them.
        let start = if EMIT {
            let start = self.scene.paints.len();
            self.decorate(
                id,
                Rect {
                    x: bx,
                    y: by,
                    w: width,
                    h: 0.0,
                },
                padding,
            );
            start
        } else {
            0
        };
        let children_start = if EMIT {
            self.scene.mark()
        } else {
            (0, 0, 0, 0)
        };
        let content_x = bx + padding[3];
        let content_y = by + padding[0];
        let content_w = (width - horizontal).max(0.0);
        let specified_height = height_length(s.height, containing_height)
            .map(|height| constrain_height(&s, height, containing_height))
            .map(|height| {
                if s.layout.box_sizing == crate::style::BoxSizing::BorderBox {
                    (height - padding[0] - padding[2]).max(0.0)
                } else {
                    height
                }
            });
        // The synthetic document represents the initial containing block.
        // Minimum/maximum height alone must not make an auto height definite.
        let child_height = if id == 0 {
            containing_height
        } else {
            specified_height
        };
        let content_h = if is_format(s.display) {
            formatting::run::<EMIT>(
                self,
                id,
                content_x,
                content_y,
                taffy::AvailableSpace::Definite(content_w),
                child_height,
                depth + 1,
            )
            .map_or_else(|| 0.0, |size| size.height)
        } else if is_table {
            self.layout_table::<EMIT>(id, content_x, content_y, content_w, child_height, depth + 1)
        } else if anonymous_table::contains(self, id) {
            anonymous_table::run::<EMIT>(
                self,
                id,
                content_x,
                content_y,
                content_w,
                child_height,
                depth + 1,
            )
        } else if self.is_replaced(id) {
            self.replaced::<EMIT>(id, content_x, content_y, content_w, containing_height)
                .1
        } else {
            self.flow::<EMIT>(id, content_x, content_y, content_w, child_height, depth + 1)
        };
        // Table and cell heights remain minimums for their contents. Ordinary
        // blocks can overflow their specified height without enlarging it.
        let used_height = if is_table || s.display == Display::TableCell {
            content_h.max(specified_height.unwrap_or(0.0))
        } else {
            specified_height.unwrap_or(content_h)
        };
        let h = if s.layout.box_sizing == crate::style::BoxSizing::BorderBox {
            constrain_height(&s, used_height + padding[0] + padding[2], containing_height)
                .max(padding[0] + padding[2])
        } else {
            constrain_height(&s, used_height, containing_height) + padding[0] + padding[2]
        };
        let h = h.clamp(0.0, MAX_EXTENT);
        let rect = Rect {
            x: bx,
            y: by,
            w: width,
            h,
        };
        // Update reserved background/border boxes after determining content height.
        if EMIT {
            if s.display != Display::TableCell
                && let Some(clip) = self.clip_for(id, rect)
            {
                self.scene.clip(children_start, clip);
            }
            self.finish_decoration(start, id, rect);
            self.scene.border_for(id, rect);
            self.scene.box_for(id, rect);
            self.hit(id, rect);
        }
        (
            width + margin[1] + margin[3],
            (h + margin[0] + margin[2]).max(0.0),
        )
    }
    fn finish_decoration(&mut self, start: usize, id: usize, r: Rect) {
        if !self.styles[id].visible {
            return;
        }
        let b = self.styles[id].border_width;
        let rects = [
            r,
            Rect { h: b[0], ..r },
            Rect {
                x: r.x + r.w - b[1],
                w: b[1],
                ..r
            },
            Rect {
                y: r.y + r.h - b[2],
                h: b[2],
                ..r
            },
            Rect { w: b[3], ..r },
        ];
        for (i, rect) in rects.into_iter().enumerate() {
            if let Some(Paint::Rect(old, _) | Paint::Background(old, ..)) =
                self.scene.paints.get_mut(start + i)
            {
                *old = rect;
            }
        }
    }
    fn layout_table<const EMIT: bool>(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        width: f32,
        containing_height: Option<f32>,
        depth: usize,
    ) -> f32 {
        let (mut min, mut max) = self.table_columns(id, depth + 1);
        let rows = self.table(id).rows.clone();
        let spacing = self.table_spacing(id);
        let space = spacing[0] * (min.len() + 1) as f32;
        let mut fixed = vec![false; min.len()];
        for (_, cells) in &rows {
            for cell in cells {
                if cell.span == 1 {
                    let style = &self.styles[cell.node];
                    if let Some(w) = style.width.resolve(width) {
                        min[cell.column] = min[cell.column].max(w);
                        max[cell.column] = max[cell.column].max(min[cell.column]);
                        fixed[cell.column] = true;
                    }
                }
            }
        }
        let available = (width - space).max(0.0);
        let total_max = max.iter().sum::<f32>();
        let widths = if available > total_max && fixed.iter().any(|f| !f) {
            let flexible = max
                .iter()
                .zip(&fixed)
                .filter(|(_, fixed)| !**fixed)
                .map(|(max, _)| *max)
                .sum::<f32>();
            let count = fixed.iter().filter(|f| !**f).count() as f32;
            max.iter()
                .zip(&fixed)
                .map(|(max, fixed)| {
                    max + if *fixed {
                        0.0
                    } else {
                        (available - total_max)
                            * if flexible > 0.0 {
                                max / flexible
                            } else {
                                1.0 / count
                            }
                    }
                })
                .collect()
        } else {
            allocate_columns(&min, &max, available)
        };
        let mut top = y + spacing[1];
        for (row, cells) in rows {
            let row_start = if EMIT && let Some(row) = row {
                let start = self.scene.paints.len();
                self.decorate(
                    row,
                    Rect {
                        x,
                        y: top,
                        w: width,
                        h: 0.0,
                    },
                    self.styles[row].border_width,
                );
                start
            } else {
                0
            };
            let row_height = row.and_then(|row| {
                height_length(self.styles[row].height, containing_height)
                    .map(|height| constrain_height(&self.styles[row], height, containing_height))
            });
            let mut height = row.map_or(0.0, |row| {
                constrain_height(
                    &self.styles[row],
                    row_height.unwrap_or(0.0),
                    containing_height,
                )
            });
            let mut ranges = Vec::new();
            for cell in cells {
                let left = x
                    + spacing[0]
                    + widths[..cell.column].iter().sum::<f32>()
                    + spacing[0] * cell.column as f32;
                let cw = widths[cell.column..cell.column + cell.span]
                    .iter()
                    .sum::<f32>()
                    + spacing[0] * (cell.span - 1) as f32;
                let start = if EMIT {
                    self.scene.mark()
                } else {
                    (0, 0, 0, 0)
                };
                let (_, h) = self.block_layout::<EMIT>(
                    cell.node,
                    left,
                    top,
                    cw,
                    row_height,
                    Some(cw),
                    depth + 1,
                );
                height = height.max(h);
                if EMIT {
                    ranges.push((cell.node, start, self.scene.mark(), h, left, cw));
                }
            }
            for (cell, start, end, h, left, cw) in ranges {
                let dy = match self.styles[cell].vertical_align {
                    VerticalAlign::Bottom => height - h,
                    VerticalAlign::Middle => (height - h) / 2.0,
                    _ => 0.0,
                };
                if dy > 0.0 {
                    for op in &mut self.scene.paints[start.0..end.0] {
                        op.rect_mut().y += dy;
                    }
                    for clip in self.scene.clips[start.0..end.0].iter_mut().flatten() {
                        clip.y += dy;
                    }
                    for (_, r) in &mut self.scene.borders[start.3..end.3] {
                        r.y += dy;
                    }
                    for b in &mut self.scene.boxes[start.1..end.1] {
                        b.y += dy.round() as i32;
                    }
                    for (r, _) in &mut self.scene.hits[start.2..end.2] {
                        r.y += dy;
                    }
                }
                // The cell background/border spans the complete row, not only its content.
                self.scene.border_for(
                    cell,
                    Rect {
                        x: left,
                        y: top,
                        w: cw,
                        h: height,
                    },
                );
                self.finish_decoration(
                    start.0,
                    cell,
                    Rect {
                        x: left,
                        y: top,
                        w: cw,
                        h: height,
                    },
                );
            }
            if EMIT && let Some(row) = row {
                self.scene.border_for(
                    row,
                    Rect {
                        x,
                        y: top,
                        w: width,
                        h: height,
                    },
                );
                self.finish_decoration(
                    row_start,
                    row,
                    Rect {
                        x,
                        y: top,
                        w: width,
                        h: height,
                    },
                );
                self.scene.box_for(
                    row,
                    Rect {
                        x,
                        y: top,
                        w: width,
                        h: height,
                    },
                );
            }
            top = (top + height + spacing[1]).min(MAX_EXTENT);
        }
        top - y
    }
    fn tokens(&mut self, id: usize, out: &mut Vec<Token>, depth: usize) {
        if matches!(
            self.styles[id].layout.position,
            crate::style::Position::Absolute | crate::style::Position::Fixed
        ) {
            return;
        }
        if !self.spend(depth) || out.len() >= MAX_OPS || self.styles[id].display == Display::None {
            return;
        }
        let node = &self.document.nodes[id];
        let s = self.styles[id].clone();
        if node.tag == "#text" {
            let mut word = String::new();
            for ch in node.text.chars() {
                if out.len() >= MAX_OPS {
                    self.exhausted = true;
                    break;
                }
                if is_css_space(ch) {
                    if !word.is_empty() {
                        let width =
                            self.fonts
                                .width_weight(&word, s.font_size, s.font_weight >= 600);
                        out.push(Token::Word {
                            node: id,
                            text: std::mem::take(&mut word),
                            width,
                        });
                    }
                    if !matches!(out.last(), Some(Token::Space { .. })) {
                        out.push(Token::Space {
                            node: id,
                            width: self
                                .fonts
                                .width_weight(" ", s.font_size, s.font_weight >= 600),
                        });
                    }
                } else {
                    word.push(ch);
                }
            }
            if !word.is_empty() {
                let width = self
                    .fonts
                    .width_weight(&word, s.font_size, s.font_weight >= 600);
                out.push(Token::Word {
                    node: id,
                    text: word,
                    width,
                });
            }
        } else if node.tag == "br" {
            out.push(Token::Break);
        } else if node.tag == "svg" && is_block(s.display) {
            // SVG is a newly admitted replaced element, but an authored block
            // display must still start a block instead of getting a baseline.
            // Keep existing image/native-control inline geometry unchanged.
            out.push(Token::Block(id));
        } else if self.is_replaced(id)
            || matches!(
                s.display,
                Display::InlineBlock
                    | Display::InlineTable
                    | Display::InlineFlex
                    | Display::InlineGrid
            )
        {
            out.push(Token::Box(id));
        } else if is_block(s.display) {
            out.push(Token::Block(id));
        } else {
            let (margin, padding) = self.edges(id, 0.0);
            if margin[3] + padding[3] != 0.0 {
                out.push(Token::Gap(margin[3] + padding[3]));
            }
            for child in node.children.clone() {
                self.tokens(child, out, depth + 1);
            }
            if margin[1] + padding[1] != 0.0 {
                out.push(Token::Gap(margin[1] + padding[1]));
            }
        }
    }
    fn flow<const EMIT: bool>(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        width: f32,
        containing_height: Option<f32>,
        depth: usize,
    ) -> f32 {
        let mut tokens = Vec::new();
        for child in self.document.nodes[id].children.clone() {
            self.tokens(child, &mut tokens, depth + 1);
        }
        self.flow_tokens::<EMIT>(id, x, y, width, containing_height, tokens, depth)
    }
    fn flow_tokens<const EMIT: bool>(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        width: f32,
        containing_height: Option<f32>,
        tokens: Vec<Token>,
        depth: usize,
    ) -> f32 {
        let mut top = y;
        let mut line = Vec::new();
        let mut used = 0.0;
        let mut pending_space = None;
        for token in tokens {
            match token {
                Token::Space { node, width } => {
                    if !line.is_empty() {
                        pending_space = Some((node, width));
                    }
                }
                Token::Block(child) => {
                    if !line.is_empty() {
                        top += self.line::<EMIT>(
                            id,
                            x,
                            top,
                            width,
                            containing_height,
                            &line,
                            used,
                            depth + 1,
                        );
                        line.clear();
                    }
                    used = 0.0;
                    pending_space = None;
                    top += self
                        .block_layout::<EMIT>(
                            child,
                            x,
                            top,
                            width,
                            containing_height,
                            None,
                            depth + 1,
                        )
                        .1;
                }
                Token::Break => {
                    top += if line.is_empty() {
                        self.line_height(id)
                    } else {
                        self.line::<EMIT>(
                            id,
                            x,
                            top,
                            width,
                            containing_height,
                            &line,
                            used,
                            depth + 1,
                        )
                    };
                    line.clear();
                    used = 0.0;
                    pending_space = None;
                }
                token => {
                    let tw = match &token {
                        Token::Word { width, .. } => *width,
                        Token::Gap(w) => *w,
                        Token::Box(child) => {
                            self.inline_box_width(*child, width, containing_height, depth + 1)
                        }
                        _ => 0.0,
                    };
                    let sw = pending_space.map_or(0.0, |(_, w)| w);
                    let nowrap = matches!(
                        self.styles[id].white_space,
                        WhiteSpace::NoWrap | WhiteSpace::Pre
                    );
                    if !nowrap && !line.is_empty() && used + sw + tw > width {
                        top += self.line::<EMIT>(
                            id,
                            x,
                            top,
                            width,
                            containing_height,
                            &line,
                            used,
                            depth + 1,
                        );
                        line.clear();
                        used = 0.0;
                        pending_space = None;
                    }
                    if let Some((node, w)) = pending_space.take() {
                        line.push(Token::Space { node, width: w });
                        used += w;
                    }
                    used += tw;
                    line.push(token);
                }
            }
            if top >= MAX_EXTENT {
                break;
            }
        }
        if !line.is_empty() {
            top += self.line::<EMIT>(id, x, top, width, containing_height, &line, used, depth + 1);
        }
        top - y
    }
    fn inline_box_width(
        &mut self,
        id: usize,
        width: f32,
        containing_height: Option<f32>,
        depth: usize,
    ) -> f32 {
        if self.is_structured_button(id) {
            self.inline_button::<false>(id, 0.0, 0.0, width, containing_height, depth)
                .0
        } else if matches!(self.document.nodes[id].tag.as_str(), "img" | "svg")
            && self.styles[id].width == Length::Auto
        {
            // A definite percentage height can change an image's auto width.
            // Use that same width for wrapping/alignment and for painting.
            let (w, _) = self.replaced_size(id, width, containing_height);
            let (m, p) = self.edges(id, width);
            w + m[1] + m[3] + p[1] + p[3]
        } else {
            self.intrinsic(id, depth).max.min(width)
        }
    }

    // Structured inline buttons shrink-wrap only when width is auto. A definite
    // CSS width is interpreted by the same box-sizing path as block buttons,
    // not treated as an intrinsic content width and padded a second time.
    fn inline_button<const EMIT: bool>(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        width: f32,
        containing_height: Option<f32>,
        depth: usize,
    ) -> (f32, f32) {
        let forced = if self.styles[id].width.resolve(width).is_some() {
            None
        } else {
            let outer = self.intrinsic(id, depth + 1).max.min(width);
            let (margin, _) = self.edges(id, width);
            Some((outer - margin[1] - margin[3]).max(0.0))
        };
        self.block_layout::<EMIT>(id, x, y, width, containing_height, forced, depth + 1)
    }

    fn line_metrics(
        &mut self,
        id: usize,
        width: f32,
        containing_height: Option<f32>,
        tokens: &[Token],
        depth: usize,
    ) -> (f32, f32) {
        // Normal line boxes are driven by their participating inline content.
        // An explicit containing line-height still contributes a minimum strut.
        let mut height = self.styles[id].line_height.unwrap_or(0.0);
        let mut ascent = if height > 0.0 {
            self.styles[id].font_size
        } else {
            0.0
        };
        for token in tokens {
            match token {
                Token::Word { node, .. } | Token::Space { node, .. } => {
                    height = height.max(self.line_height(*node));
                    ascent = ascent.max(self.styles[*node].font_size);
                }
                Token::Box(child) => {
                    if self.is_structured_button(*child) {
                        let (_, h) = self.inline_button::<false>(
                            *child,
                            0.0,
                            0.0,
                            width,
                            containing_height,
                            depth + 1,
                        );
                        height = height.max(h);
                    } else {
                        let (_, h) = self.replaced_size(*child, width, containing_height);
                        let (m, p) = self.edges(*child, width);
                        height = height.max(h + m[0] + m[2] + p[0] + p[2]);
                    }
                }
                _ => {}
            }
        }
        (height, ascent)
    }
    fn line<const EMIT: bool>(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        width: f32,
        containing_height: Option<f32>,
        tokens: &[Token],
        used: f32,
        depth: usize,
    ) -> f32 {
        let (height, ascent) = self.line_metrics(id, width, containing_height, tokens, depth);
        let mut left = x + match self.styles[id].text_align {
            TextAlign::Center => (width - used).max(0.0) / 2.0,
            TextAlign::Right | TextAlign::End => (width - used).max(0.0),
            _ => 0.0,
        };
        for token in tokens {
            match token {
                Token::Word {
                    node,
                    text,
                    width: tw,
                } => {
                    if EMIT {
                        let s = &self.styles[*node];
                        let ty =
                            y + (ascent - s.font_size).max(0.0) + (height - ascent).max(0.0) * 0.1;
                        let r = Rect {
                            x: left,
                            y: ty,
                            w: *tw,
                            h: self.line_height(*node),
                        };
                        if s.visible {
                            self.scene.paint(*node, Paint::Text(r, *node, text.clone()));
                            self.hit(*node, r);
                        }
                        self.scene.box_for(*node, r);
                    }
                    left += tw;
                }
                Token::Space { width, .. } | Token::Gap(width) => left += width,
                Token::Box(child) => {
                    let start = if EMIT {
                        self.scene.mark()
                    } else {
                        (0, 0, 0, 0)
                    };
                    let (w, h) = if self.is_replaced(*child) {
                        self.replaced::<EMIT>(*child, left, y, width, containing_height)
                    } else if self.is_structured_button(*child) {
                        self.inline_button::<EMIT>(
                            *child,
                            left,
                            y,
                            width,
                            containing_height,
                            depth + 1,
                        )
                    } else {
                        let w = self.intrinsic(*child, depth + 1).max.min(width);
                        self.block_layout::<EMIT>(
                            *child,
                            left,
                            y,
                            w,
                            containing_height,
                            Some(w),
                            depth + 1,
                        )
                    };
                    if EMIT && h < height {
                        self.scene.shift(start, 0.0, height - h);
                    }
                    left += w;
                }
                _ => {}
            }
        }
        height
    }
    fn replaced<const EMIT: bool>(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        basis: f32,
        containing_height: Option<f32>,
    ) -> (f32, f32) {
        let (w, h) = self.replaced_size(id, basis, containing_height);
        let (m, p) = self.edges(id, basis);
        let l = &self.styles[id].layout;
        let (dx, dy) = if l.position == crate::style::Position::Relative {
            (
                l.inset[3]
                    .resolve(basis)
                    .or_else(|| l.inset[1].resolve(basis).map(|n| -n))
                    .unwrap_or(0.0),
                height_length_signed(l.inset[0], containing_height)
                    .or_else(|| height_length_signed(l.inset[2], containing_height).map(|n| -n))
                    .unwrap_or(0.0),
            )
        } else {
            (0.0, 0.0)
        };
        let r = Rect {
            x: x + m[3] + dx,
            y: y + m[0] + dy,
            w: w + p[1] + p[3],
            h: h + p[0] + p[2],
        };
        if !EMIT {
            return (r.w + m[1] + m[3], r.h + m[0] + m[2]);
        }
        let content = Rect {
            x: r.x + p[3],
            y: r.y + p[0],
            w,
            h,
        };
        self.paint_replaced_allocated(id, r, content);
        (r.w + m[1] + m[3], r.h + m[0] + m[2])
    }

    /// Paint an already measured border/content pair. Formatting algorithms own
    /// sizing; this must not resolve CSS dimensions or add margins a second time.
    fn paint_replaced_allocated(&mut self, id: usize, r: Rect, content: Rect) {
        self.scene.border_for(id, r);
        self.decorate(
            id,
            r,
            [
                content.y - r.y,
                (r.x + r.w - content.x - content.w).max(0.0),
                (r.y + r.h - content.y - content.h).max(0.0),
                content.x - r.x,
            ],
        );
        if self.document.nodes[id].tag == "svg" {
            self.prepare_inline_svg(id);
        }
        let node = &self.document.nodes[id];
        if !self.styles[id].visible && node.tag != "svg" {
            self.scene.box_for(id, r);
            return;
        }
        if node.tag == "svg" {
            self.scene.paint(
                id,
                Paint::Image(
                    content,
                    ImageSource::InlineSvg(id),
                    self.styles[id]
                        .visible
                        .then(|| (id, node.attr("aria-label").unwrap_or("").to_owned())),
                ),
            );
            self.hit(id, r);
        } else if node.tag == "img" {
            if let Some(url) = self.image_url(id)
                && self.document.resources.contains_key(&url)
            {
                self.scene.paint(
                    id,
                    Paint::Image(
                        content,
                        ImageSource::Resource(url),
                        Some((id, node.attr("alt").unwrap_or("").to_owned())),
                    ),
                );
            } else if content.w > 0.0 && content.h > 0.0 {
                self.scene.paint(
                    id,
                    Paint::Rect(
                        content,
                        Color {
                            rgb: 0xdddddd,
                            alpha: 1.0,
                        },
                    ),
                );
                if let Some(alt) = node.attr("alt").filter(|s| !s.is_empty()) {
                    self.scene
                        .paint(id, Paint::Text(content, id, alt.to_owned()));
                }
            }
            self.hit(id, r);
        } else {
            let item = self
                .document
                .item_nodes
                .iter()
                .position(|&n| n == id)
                .and_then(|i| self.document.items.get(i));
            let (text, submit) = match item {
                Some(Item::Input { value, kind, .. }) => {
                    let value = self
                        .controls
                        .values
                        .and_then(|values| values.get(&id))
                        .unwrap_or(value);
                    (
                        if kind == "password" {
                            "•".repeat(value.chars().count())
                        } else {
                            value.clone()
                        },
                        false,
                    )
                }
                Some(Item::Submit { label, .. }) => (label.clone(), true),
                _ => (node.attr("value").unwrap_or("").to_owned(), false),
            };
            self.scene.paint(id, Paint::Control(r, id, text, submit));
            if self.styles[id].pointer_events {
                self.scene.hit(
                    id,
                    r,
                    if submit {
                        Action::Submit(id)
                    } else {
                        Action::Input(id)
                    },
                );
            }
        }
        self.scene.box_for(id, r);
    }

    /// Emit once from the final border rectangle, retaining fractional origins
    /// until Scene records paint/inspection/hit geometry. The allocation already
    /// includes resolved min/max sizes, box sizing, margins and alignment.
    fn paint_allocated(
        &mut self,
        id: usize,
        r: Rect,
        padding: [f32; 4],
        definite_content_height: Option<f32>,
        depth: usize,
    ) {
        if !self.spend(depth) {
            return;
        }
        let content = Rect {
            x: r.x + padding[3],
            y: r.y + padding[0],
            w: (r.w - padding[1] - padding[3]).max(0.0),
            h: (r.h - padding[0] - padding[2]).max(0.0),
        };
        if self.is_replaced(id) {
            self.paint_replaced_allocated(id, r, content);
            return;
        }
        self.scene.border_for(id, r);
        self.decorate(id, r, padding);
        let children_start = self.scene.mark();
        match self.styles[id].display {
            display if is_format(display) => {
                formatting::run::<true>(
                    self,
                    id,
                    content.x,
                    content.y,
                    taffy::AvailableSpace::Definite(content.w),
                    definite_content_height,
                    depth + 1,
                );
            }
            Display::Table | Display::InlineTable => {
                self.layout_table::<true>(
                    id,
                    content.x,
                    content.y,
                    content.w,
                    definite_content_height,
                    depth + 1,
                );
            }
            _ if anonymous_table::contains(self, id) => {
                anonymous_table::run::<true>(
                    self,
                    id,
                    content.x,
                    content.y,
                    content.w,
                    definite_content_height,
                    depth + 1,
                );
            }
            _ => {
                self.flow::<true>(
                    id,
                    content.x,
                    content.y,
                    content.w,
                    definite_content_height,
                    depth + 1,
                );
            }
        }
        if let Some(clip) = self.clip_for(id, r) {
            self.scene.clip(children_start, clip);
        }
        self.scene.box_for(id, r);
        self.hit(id, r);
    }
}

fn is_format(display: Display) -> bool {
    matches!(
        display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    )
}

fn is_block(display: Display) -> bool {
    matches!(
        display,
        Display::Block
            | Display::Flex
            | Display::Grid
            | Display::Table
            | Display::TableRowGroup
            | Display::TableHeaderGroup
            | Display::TableFooterGroup
            | Display::TableRow
            | Display::TableCell
            | Display::ListItem
    )
}
fn measurement_dimension(value: f32) -> bool {
    value.is_finite() && (0.0..=MAX_EXTENT).contains(&value)
}
fn is_css_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0c')
}
fn collapse(text: &str) -> String {
    let mut result = String::new();
    let mut space = false;
    for ch in text.chars() {
        if is_css_space(ch) {
            if !space {
                result.push(' ');
            }
            space = true;
        } else {
            result.push(ch);
            space = false;
        }
    }
    result
}
fn distribute_deficit(widths: &mut [f32], target: f32) {
    let missing = (target - widths.iter().sum::<f32>()).max(0.0);
    if !widths.is_empty() {
        let each = missing / widths.len() as f32;
        for width in widths {
            *width += each;
        }
    }
}
fn allocate_columns(min: &[f32], max: &[f32], available: f32) -> Vec<f32> {
    let total_min = min.iter().sum::<f32>();
    let total_max = max.iter().sum::<f32>();
    let extra = (available - total_min).max(0.0);
    if total_max > total_min && available < total_max {
        min.iter()
            .zip(max)
            .map(|(min, max)| min + extra * (max - min) / (total_max - total_min))
            .collect()
    } else if total_max > 0.0 {
        max.iter()
            .map(|max| max + (available - total_max).max(0.0) * max / total_max)
            .collect()
    } else {
        vec![
            if min.is_empty() {
                0.0
            } else {
                available / min.len() as f32
            };
            min.len()
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn columns_share_space_without_violating_intrinsic_minimums() {
        assert_eq!(
            allocate_columns(&[10.0, 20.0], &[30.0, 60.0], 60.0),
            vec![20.0, 40.0]
        );
        assert_eq!(
            allocate_columns(&[10.0, 20.0], &[30.0, 60.0], 10.0),
            vec![10.0, 20.0]
        );
        assert_eq!(
            allocate_columns(&[10.0, 20.0], &[30.0, 60.0], 180.0),
            vec![60.0, 120.0]
        );
    }
    #[test]
    fn spanning_cells_distribute_only_missing_width() {
        let mut cols = [12.0, 18.0];
        distribute_deficit(&mut cols, 40.0);
        assert_eq!(cols, [17.0, 23.0]);
    }
    #[test]
    fn whitespace_keeps_nonbreaking_spaces() {
        assert_eq!(collapse("a\n\t b\u{a0}c"), "a b\u{a0}c");
    }

    fn fonts() -> Fonts {
        Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn styles(document: &Document) -> Vec<ComputedStyle> {
        document
            .nodes
            .iter()
            .map(|node| {
                let mut s = ComputedStyle {
                    font_size: 12.0,
                    line_height: Some(15.0),
                    ..ComputedStyle::default()
                };
                s.display = match node.tag.as_str() {
                    "#document" | "html" | "body" | "center" | "div" => Display::Block,
                    "table" => Display::Table,
                    "tbody" => Display::TableRowGroup,
                    "tr" => Display::TableRow,
                    "td" | "th" => Display::TableCell,
                    "head" | "style" => Display::None,
                    _ => Display::Inline,
                };
                if node.tag == "center" {
                    s.text_align = TextAlign::Center;
                }
                if let Some(width) = node.attr("width").and_then(|n| n.parse().ok()) {
                    s.width = Length::Px(width);
                }
                s
            })
            .collect()
    }
    fn render_test(
        document: &Document,
        styles: Vec<ComputedStyle>,
        width: u32,
        scroll: i32,
    ) -> Frame {
        render(
            document,
            &mut fonts(),
            Viewport {
                width,
                height: 300,
                scroll,
            },
            &Controls::default(),
            styles,
        )
        .unwrap()
    }
    fn element_box<'a>(document: &Document, frame: &'a Frame, selector: &str) -> &'a LayoutBox {
        let id = document.query_selector(0, selector).unwrap().unwrap();
        frame.boxes.iter().rev().find(|b| b.node == id).unwrap()
    }

    fn measurement_layout<'a, 'c>(
        document: &'a Document,
        fonts: &'a mut Fonts,
        controls: &'a Controls<'c>,
        styles: Vec<ComputedStyle>,
    ) -> Layout<'a, 'c> {
        let mut layout = Layout {
            document,
            fonts,
            controls,
            viewport_overflow: viewport_overflow_node(document, &styles),
            styles,
            viewport: Viewport {
                width: 800,
                height: 600,
                scroll: 0,
            },
            scene: Scene::default(),
            intrinsic: HashMap::new(),
            content_intrinsic_cache: HashMap::new(),
            content_height_cache: HashMap::new(),
            natural_images: HashMap::new(),
            natural_svg: HashMap::new(),
            inline_svg: HashMap::new(),
            inline_svg_bytes: 0,
            inline_svg_limit_reported: false,
            diagnostics: Default::default(),
            table_cache: HashMap::new(),
            format_scratch: 0,
            replaced_nodes: Vec::new(),
            anonymous_tables: Vec::new(),
            budget: 2_000_000,
            exhausted: false,
            failure: None,
        };
        layout.prepare_replaced_nodes();
        anonymous_table::prepare(&mut layout);
        layout
    }

    fn assert_no_scene_allocation(layout: &Layout<'_, '_>) {
        assert_eq!(layout.scene.mark(), (0, 0, 0, 0));
        assert_eq!(layout.scene.clips.capacity(), 0);
        assert_eq!(layout.scene.borders.capacity(), 0);
        assert_eq!(layout.scene.paints.capacity(), 0);
        assert_eq!(layout.scene.paint_nodes.capacity(), 0);
        assert_eq!(layout.scene.boxes.capacity(), 0);
        assert_eq!(layout.scene.box_scroll.capacity(), 0);
        assert_eq!(layout.scene.hits.capacity(), 0);
        assert_eq!(layout.scene.hit_nodes.capacity(), 0);
    }

    #[test]
    fn paint_free_outer_measurement_matches_legacy_sizes_and_work() {
        let document = crate::document::parse(
            "<style>*{font-size:12px;line-height:15px} #root{padding:3.5px;border:1px solid;margin:2px} td{padding:2px} #short{height:8px} #inline{display:inline-block;padding:1px} #hidden{visibility:hidden}</style><div id=root><a href=/x>alpha beta gamma delta epsilon</a><br><span id=inline>nested words wrap here</span><input size=4><img width=11 height=7><div id=short>overflow wraps below its box</div><table width='87%'><tr><td width=20>A</td><td>longer cell text here</td></tr><tr><td colspan=2><table><tr><td>nested table</td><td>other</td></tr></table></td></tr></table><div id=hidden>hidden geometry</div></div>",
            "https://fixture.example/",
        );
        let id = document.query_selector(0, "#root").unwrap().unwrap();
        let controls = Controls::default();
        for width in [45.0, 123.5, 257.0] {
            for containing_height in [None, Some(160.0)] {
                let computed =
                    crate::style::compute_styles(&document, &document.stylesheets, (width, 300.0))
                        .unwrap();
                let mut measured_fonts = fonts();
                let mut measured =
                    measurement_layout(&document, &mut measured_fonts, &controls, computed.clone());
                let size = measured
                    .measure_outer(id, width, containing_height, None, 0)
                    .unwrap();
                assert_no_scene_allocation(&measured);
                let mut painted_fonts = fonts();
                let mut painted =
                    measurement_layout(&document, &mut painted_fonts, &controls, computed);
                assert_eq!(
                    size,
                    painted.block(id, 0.0, 0.0, width, containing_height, None, 0)
                );
                assert_eq!(measured.budget, painted.budget);
                assert!(!painted.scene.paints.is_empty());
                assert!(!painted.scene.boxes.is_empty());
                assert!(!painted.scene.hits.is_empty());
                assert!(!measured.exhausted && !painted.exhausted);
                // Measuring an already-painted pass must not append, clear or
                // shift its scene. Cached intrinsic work remains shared.
                let mark = painted.scene.mark();
                let boxes = format!("{:?}", painted.scene.boxes);
                assert_eq!(
                    painted.measure_outer(id, width, containing_height, None, 0),
                    Some(size)
                );
                assert_eq!(painted.scene.mark(), mark);
                assert_eq!(format!("{:?}", painted.scene.boxes), boxes);
            }
        }
    }

    #[test]
    fn paint_free_content_intrinsic_excludes_own_constraints_and_edges() {
        let document = crate::document::parse(
            "<div id=root>alpha beta<div id=child>gamma</div></div>",
            "https://fixture.example/",
        );
        let id = document.query_selector(0, "#root").unwrap().unwrap();
        let child = document.query_selector(0, "#child").unwrap().unwrap();
        let mut computed = styles(&document);
        computed[id].width = Length::Px(500.0);
        computed[id].min_width = Length::Px(600.0);
        computed[id].margin = [Length::Px(3.0); 4];
        computed[id].padding = [Length::Px(5.0); 4];
        computed[id].border_width = [2.0; 4];
        computed[child].width = Length::Px(80.0);
        computed[child].padding = [Length::Px(4.0); 4];
        let mut fonts = fonts();
        let controls = Controls::default();
        let mut layout = measurement_layout(&document, &mut fonts, &controls, computed);
        let content = layout.measure_content_intrinsic(id, 0).unwrap();
        assert_eq!(content.min, 88.0);
        assert_eq!(content.max, 88.0);
        let outer = layout.intrinsic(id, 0);
        assert_eq!(outer.min, 620.0);
        assert_eq!(outer.max, 620.0);
        assert_no_scene_allocation(&layout);
    }

    #[test]
    fn measurement_cache_hits_preserve_keys_budget_and_empty_scene() {
        let document = crate::document::parse(
            "<div id=root><div id=half>alpha beta</div></div>",
            "https://fixture.example/",
        );
        let id = document.query_selector(0, "#root").unwrap().unwrap();
        let half = document.query_selector(0, "#half").unwrap().unwrap();
        let mut computed = styles(&document);
        computed[half].height = Length::Percent(0.5);
        let mut fonts = fonts();
        let controls = Controls::default();
        let mut layout = measurement_layout(&document, &mut fonts, &controls, computed);
        let automatic = layout.measure_content_height(id, 100.0, None, 0).unwrap();
        let remaining = layout.budget;
        assert_eq!(
            layout.measure_content_height(id, 100.0, None, 0),
            Some(automatic)
        );
        assert_eq!(
            layout.budget,
            remaining - 1,
            "cache hit still spends entry work"
        );
        assert_eq!(
            layout.measure_content_height(id, 100.0, Some(100.0), 0),
            Some(50.0)
        );
        assert_ne!(automatic, 50.0);
        assert!(layout.measure_content_height(id, 100.0, None, 1).is_some());
        assert_eq!(
            layout.content_height_cache.len(),
            3,
            "definiteness and depth are cache inputs"
        );
        let size = layout.measure_content_intrinsic(id, 0).unwrap();
        let remaining = layout.budget;
        let cached = layout.measure_content_intrinsic(id, 0).unwrap();
        assert_eq!((size.min, size.max), (cached.min, cached.max));
        assert_eq!(layout.budget, remaining - 1);
        assert_no_scene_allocation(&layout);
        layout.budget = 0;
        assert!(layout.measure_content_height(id, 100.0, None, 0).is_none());
        assert!(layout.measure_content_intrinsic(id, 0).is_none());
        assert_eq!(
            layout.content_height_cache.len(),
            3,
            "failed work is not cached"
        );
    }

    #[test]
    fn measurement_cache_capacity_is_bounded_without_truncating_measurement() {
        let document = crate::document::parse("<div id=hidden></div>", "https://fixture.example/");
        let id = document.query_selector(0, "#hidden").unwrap().unwrap();
        let mut computed = styles(&document);
        computed[id].display = Display::None;
        let mut fonts = fonts();
        let controls = Controls::default();
        let mut layout = measurement_layout(&document, &mut fonts, &controls, computed);
        for width in 0..MAX_MEASURE_CACHE + 10 {
            assert_eq!(
                layout.measure_content_height(id, width as f32, None, 0),
                Some(0.0)
            );
        }
        assert_eq!(layout.content_height_cache.len(), MAX_MEASURE_CACHE);
        assert!(!layout.exhausted);
        assert!(
            layout.content_height_cache.capacity()
                * (std::mem::size_of::<(usize, u32, Option<u32>, usize)>()
                    + std::mem::size_of::<f32>()
                    + 16)
                < 2 * 1024 * 1024
        );
        assert_no_scene_allocation(&layout);
    }

    #[test]
    fn paint_free_wrapping_uses_content_width_and_definite_height_only_for_children() {
        let document = crate::document::parse(
            "<div id=root>alpha beta gamma delta epsilon</div><div id=percent><div id=half></div></div><table id=table><tr><td>alpha beta gamma delta</td><td>other</td></tr></table>",
            "https://fixture.example/",
        );
        let id = document.query_selector(0, "#root").unwrap().unwrap();
        let text = document.nodes[id].children[0];
        let percent = document.query_selector(0, "#percent").unwrap().unwrap();
        let half = document.query_selector(0, "#half").unwrap().unwrap();
        let table = document.query_selector(0, "#table").unwrap().unwrap();
        let mut computed = styles(&document);
        computed[id].width = Length::Px(900.0);
        computed[id].height = Length::Px(200.0);
        computed[id].padding = [Length::Px(10.0); 4];
        computed[half].height = Length::Percent(0.5);
        let mut fonts = fonts();
        let controls = Controls::default();
        let mut layout = measurement_layout(&document, &mut fonts, &controls, computed);
        let narrow = layout.measure_content_height(id, 65.5, None, 0).unwrap();
        let wide = layout.measure_content_height(id, 600.0, None, 0).unwrap();
        assert!(narrow > wide);
        assert_eq!(wide, 15.0);
        assert_eq!(
            layout.measure_content_height(text, 65.5, None, 0),
            Some(narrow)
        );
        assert_eq!(
            layout.measure_content_height(percent, 100.0, None, 0),
            Some(0.0)
        );
        assert_eq!(
            layout.measure_content_height(percent, 100.0, Some(80.0), 0),
            Some(40.0)
        );
        let table_height = layout.measure_content_height(table, 70.5, None, 0).unwrap();
        assert_no_scene_allocation(&layout);
        assert_eq!(narrow, layout.flow::<true>(id, 0.0, 0.0, 65.5, None, 1));
        assert_eq!(
            table_height,
            layout.layout_table::<true>(table, 0.0, 0.0, 70.5, None, 1)
        );
    }

    #[test]
    fn paint_free_replaced_natural_size_ignores_css_and_reuses_bounded_cache() {
        let mut document = crate::document::parse(
            "<img id=image src=natural.svg width=0 height=0><input id=input size=4>",
            "https://fixture.example/",
        );
        document.resources.insert("https://fixture.example/natural.svg".into(), crate::document::ResourceData {
            bytes: b"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20'/></svg>".to_vec(),
            content_type: "image/svg+xml".into(),
        });
        let image = document.query_selector(0, "#image").unwrap().unwrap();
        let input = document.query_selector(0, "#input").unwrap().unwrap();
        let mut computed = styles(&document);
        computed[image].height = Length::Px(0.0);
        computed[input].width = Length::Px(600.0);
        computed[input].height = Length::Px(200.0);
        let mut fonts = fonts();
        let expected_input_width = fonts.width("0", 12.0) * 4.0;
        let controls = Controls::default();
        let mut layout = measurement_layout(&document, &mut fonts, &controls, computed);
        assert_eq!(layout.replaced_size(image, 200.0, None), (0.0, 0.0));
        assert!(layout.natural_images.is_empty());
        assert_eq!(
            layout.measure_natural_replaced(image, 0),
            Some((40.0, 20.0))
        );
        assert_eq!(
            layout.measure_content_intrinsic(image, 0).unwrap().max,
            40.0
        );
        assert_eq!(layout.natural_images.len(), 1);
        assert_eq!(layout.replaced_size(image, 200.0, None), (0.0, 0.0));
        assert_eq!(
            layout.measure_natural_replaced(input, 0),
            Some((expected_input_width, 15.0))
        );
        assert_eq!(layout.measure_content_height(image, 200.0, None, 0), None);
        assert_no_scene_allocation(&layout);
    }

    #[test]
    fn paint_free_measurement_budget_is_cumulative_and_failure_is_not_a_size() {
        let document =
            crate::document::parse("<div>alpha beta gamma</div>", "https://fixture.example/");
        let id = document.query_selector(0, "div").unwrap().unwrap();
        let controls = Controls::default();
        let mut fonts = fonts();
        let mut layout = measurement_layout(&document, &mut fonts, &controls, styles(&document));
        layout.budget = 12;
        let mut calls = 0;
        while layout.measure_content_height(id, 60.0, None, 0).is_some() {
            calls += 1;
            assert!(calls < 12, "measurement must not reset the work budget");
        }
        assert!(calls > 1);
        assert!(layout.exhausted);
        assert_eq!(layout.budget, 0);
        assert!(layout.measure_outer(id, 60.0, None, None, 0).is_none());
        assert!(layout.measure_content_intrinsic(id, 0).is_none());
        assert_no_scene_allocation(&layout);

        let mut fresh = measurement_layout(&document, &mut fonts, &controls, styles(&document));
        assert!(
            fresh
                .measure_outer(id, f32::INFINITY, None, None, 0)
                .is_none()
        );
        assert!(
            fresh
                .measure_content_height(id, 60.0, Some(f32::NAN), 0)
                .is_none()
        );
        assert!(fresh.measure_content_intrinsic(id, MAX_DEPTH + 1).is_none());
        assert!(fresh.exhausted);
        assert_no_scene_allocation(&fresh);
    }
    #[test]
    fn dom_table_shares_columns_and_spans_across_rows() {
        let doc = crate::document::parse(
            "<table width=240><tr><td id=a>1</td><td id=b>longer text</td></tr><tr><td id=c>100</td><td id=d>end</td></tr><tr><td id=span colspan=2>footer</td></tr></table>",
            "https://example.test/",
        );
        let frame = render_test(&doc, styles(&doc), 500, 0);
        let a = element_box(&doc, &frame, "#a");
        let b = element_box(&doc, &frame, "#b");
        let c = element_box(&doc, &frame, "#c");
        let d = element_box(&doc, &frame, "#d");
        let span = element_box(&doc, &frame, "#span");
        assert_eq!(a.x, c.x);
        assert_eq!(a.width, c.width);
        assert_eq!(b.x, d.x);
        assert_eq!(span.x, a.x);
        assert!(span.width >= a.width + b.width + 1);
        assert!(c.y > a.y);
        assert!(span.y > c.y);
    }
    #[test]
    fn centered_percentage_table_respects_minimum_width() {
        let doc = crate::document::parse(
            "<body><center><table id=panel><tr><td>content</td></tr></table></center></body>",
            "https://example.test/",
        );
        let mut s = styles(&doc);
        let table = doc.query_selector(0, "table").unwrap().unwrap();
        let body = doc.query_selector(0, "body").unwrap().unwrap();
        s[body].margin = [Length::Px(8.0); 4];
        s[table].width = Length::Percent(0.85);
        s[table].min_width = Length::Px(796.0);
        let frame = render_test(&doc, s, 1024, 0);
        let panel = element_box(&doc, &frame, "#panel");
        assert_eq!(panel.x, 84);
        assert_eq!(panel.width, 857);
        assert_eq!(panel.y, 8);
    }
    #[test]
    fn wrapped_link_hits_follow_scrolling_and_keep_url() {
        let doc = crate::document::parse(
            "<div width=95><a href='/destination'>first second third fourth fifth sixth seventh eighth</a></div>",
            "https://example.test/",
        );
        let s = styles(&doc);
        let first = render_test(&doc, s.clone(), 200, 0);
        let scrolled = render_test(&doc, s, 200, 20);
        assert!(first.hits.len() >= 8);
        assert!(first.content_height >= 45);
        assert!(first.hits.iter().any(|hit| hit.y >= 30));
        assert!(scrolled.hits.len() < first.hits.len());
        assert!(scrolled.hits.iter().all(|hit|matches!(&hit.action,Action::Link{href,..} if href=="https://example.test/destination")));
        assert_eq!(first.content_height, scrolled.content_height);
    }
    #[test]
    fn zero_sized_image_keeps_no_visible_placeholder_or_hit() {
        let doc = crate::document::parse(
            "<a href='/x'><img width=0 height=0 alt=spacer></a>",
            "https://example.test/",
        );
        let mut s = styles(&doc);
        let image = doc.query_selector(0, "img").unwrap().unwrap();
        s[image].height = Length::Px(0.0);
        let frame = render_test(&doc, s, 200, 0);
        assert!(frame.hits.is_empty());
        assert!(frame.canvas.pixels.iter().all(|pixel| *pixel == 0xffffff));
    }

    #[test]
    fn image_admission_checks_dimensions_before_multiplication() {
        assert!(!image_cache_admits(u32::MAX, u32::MAX, 0, 0));
        assert!(!image_cache_admits(2048, 2048, 0, 0));
        assert!(image_cache_admits(1024, 1024, 12 * 1024 * 1024, 127));
        assert!(!image_cache_admits(1024, 1024, 12 * 1024 * 1024 + 1, 127));
        assert!(!image_cache_admits(1, 1, 0, 128));
        assert!(!image_cache_admits(1, 1, usize::MAX, 0));
    }

    #[test]
    fn scaled_translucent_rect_keeps_opaque_edges_and_scroll_alignment() {
        for scale in [1.0, 1.25, 2.0] {
            let mut alpha = Canvas::new_scaled(8, 6, 0xffffff, scale);
            let mut opaque = Canvas::new_scaled(8, 6, 0xffffff, scale);
            let rect = Rect {
                x: -1.0,
                y: 3.0,
                w: 4.0,
                h: 3.0,
            };
            paint_rect(&mut alpha, rect, Color { rgb: 0, alpha: 0.5 }, 2);
            paint_rect(
                &mut opaque,
                rect,
                Color {
                    rgb: 0x808080,
                    alpha: 1.0,
                },
                2,
            );
            assert_eq!(alpha.pixels, opaque.pixels);
            assert_eq!(
                alpha.pixels[alpha.physical_edge(1) as usize * alpha.width as usize],
                0x808080
            );
            assert_eq!(
                alpha.pixels[alpha.physical_edge(4) as usize * alpha.width as usize],
                0xffffff
            );
        }
    }

    #[test]
    fn natural_dimensions_are_cached_by_resource_url_not_image_node() {
        let doc = crate::document::parse(
            "<img src='shared.svg#first'><img src='shared.svg#second'><img src='shared.svg' width=0 height=0>",
            "https://example.test/",
        );
        let ids = doc.query_selector_all(0, "img").unwrap();
        let mut computed = styles(&doc);
        computed[ids[2]].height = Length::Px(0.0);
        let mut fonts = fonts();
        let controls = Controls::default();
        let mut layout = Layout {
            document: &doc,
            fonts: &mut fonts,
            controls: &controls,
            viewport_overflow: viewport_overflow_node(&doc, &computed),
            styles: computed,
            viewport: Viewport {
                width: 800,
                height: 600,
                scroll: 0,
            },
            scene: Scene::default(),
            intrinsic: HashMap::new(),
            content_intrinsic_cache: HashMap::new(),
            content_height_cache: HashMap::new(),
            natural_images: HashMap::new(),
            natural_svg: HashMap::new(),
            inline_svg: HashMap::new(),
            inline_svg_bytes: 0,
            inline_svg_limit_reported: false,
            diagnostics: Default::default(),
            table_cache: HashMap::new(),
            format_scratch: 0,
            replaced_nodes: Vec::new(),
            anonymous_tables: Vec::new(),
            budget: 2_000_000,
            exhausted: false,
            failure: None,
        };
        layout.prepare_replaced_nodes();
        anonymous_table::prepare(&mut layout);
        assert_eq!(layout.replaced_size(ids[0], 200.0, None), (18.0, 18.0));
        assert_eq!(layout.replaced_size(ids[1], 200.0, None), (18.0, 18.0));
        assert_eq!(layout.natural_images.len(), 1);
        assert!(
            layout
                .natural_images
                .contains_key("https://example.test/shared.svg")
        );
        assert_eq!(layout.replaced_size(ids[2], 200.0, None), (0.0, 0.0));
        assert_eq!(layout.natural_images.len(), 1);
    }

    #[test]
    fn oversized_background_dimension_does_not_overflow() {
        let mut doc = crate::document::parse("<div>visible</div>", "https://example.test/");
        doc.resources.insert(
            "https://example.test/image.svg".into(),
            crate::document::ResourceData {
                bytes: b"<svg/>".to_vec(),
                content_type: "image/svg+xml".into(),
            },
        );
        let mut computed = styles(&doc);
        let id = doc.query_selector(0, "div").unwrap().unwrap();
        computed[id]
            .background_images
            .push(crate::style::BackgroundImage {
                url: "https://example.test/image.svg".into(),
                width: Length::Px(f32::MAX),
                height: Length::Px(f32::MAX),
            });
        let frame = render_test(&doc, computed, 200, 0);
        assert!(frame.canvas.pixels.iter().any(|pixel| *pixel != 0xffffff));
    }

    #[test]
    fn styled_limits_reject_whole_path_without_truncating_document() {
        let doc = crate::document::parse(
            &format!("<div>{}</div>", "x".repeat(MAX_TEXT_RUN + 1)),
            "https://example.test/",
        );
        assert!(
            render(
                &doc,
                &mut fonts(),
                Viewport {
                    width: 200,
                    height: 100,
                    scroll: 0
                },
                &Controls::default(),
                styles(&doc)
            )
            .is_none()
        );
        assert_eq!(
            doc.nodes.iter().map(|node| node.text.len()).sum::<usize>(),
            MAX_TEXT_RUN + 1
        );
        let doc = crate::document::parse(
            &format!(
                "<script>{}</script><div>readable</div>",
                "x".repeat(MAX_TEXT_RUN + 1)
            ),
            "https://example.test/",
        );
        let mut computed = styles(&doc);
        let script = doc.query_selector(0, "script").unwrap().unwrap();
        computed[script].display = Display::None;
        assert!(
            render(
                &doc,
                &mut fonts(),
                Viewport {
                    width: 200,
                    height: 100,
                    scroll: 0
                },
                &Controls::default(),
                computed
            )
            .is_some()
        );
    }

    #[test]
    fn nested_table_is_contained_and_keeps_independent_columns() {
        let doc = crate::document::parse(
            "<table width=300><tr><td width=50 id=left>L</td><td id=host><table width=180><tr><td id=inner1>A</td><td id=inner2>B</td></tr></table></td></tr></table>",
            "https://example.test/",
        );
        let frame = render_test(&doc, styles(&doc), 400, 0);
        let host = element_box(&doc, &frame, "#host");
        let first = element_box(&doc, &frame, "#inner1");
        let second = element_box(&doc, &frame, "#inner2");
        assert!(first.x > host.x);
        assert!(second.x > first.x);
        assert!(second.x + second.width as i32 <= host.x + host.width as i32);
    }
}
