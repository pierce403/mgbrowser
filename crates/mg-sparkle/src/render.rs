//! Headless document rendering in page-relative pixel coordinates.
//! Styled documents use the bounded DOM renderer; documents without author CSS
//! or loaded images retain the historical readable-flow fallback.
use crate::{
    document::{Document, Item},
    paint::{Canvas, Fonts},
};
use std::collections::HashMap;
use std::sync::Arc;
const BG: u32 = 0xfafbf8;
const INK: u32 = 0x26342b;
const LINK: u32 = 0x174ea6;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Input(usize),
    Submit(usize),
    Link { node: usize, href: String },
}
#[derive(Clone, Debug)]
pub struct Hit {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub action: Action,
}
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutBox {
    pub node: usize,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub node: usize,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub font_size: f32,
    pub text: String,
}
pub(crate) const MAX_TEXT_RUNS: usize = 25_000;
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scroll: i32,
}
#[derive(Default)]
pub struct Controls<'a> {
    pub values: Option<&'a HashMap<usize, String>>,
    pub focused_input: Option<usize>,
    pub select_all: bool,
    /// Optional `(sel_start_chars, sel_end_chars, caret_chars)` for `focused_input`.
    pub input_selection: Option<(usize, usize, usize)>,
}
#[derive(Clone)]
pub struct Frame {
    pub canvas: Canvas,
    pub hits: Vec<Hit>,
    pub boxes: Vec<LayoutBox>,
    pub text_runs: Vec<TextRun>,
    pub content_height: i32,
    /// Bounded diagnostics from this frame's actual style/layout pass. Resource
    /// loading and script execution have separate reports owned by their callers.
    pub diagnostics: crate::style::StyleDiagnostics,
}

impl Frame {
    /// Retained capacities and owned string payload, excluding allocator headers.
    pub fn cache_payload_bytes(&self) -> usize {
        self.canvas
            .pixels
            .capacity()
            .saturating_mul(std::mem::size_of::<u32>())
            .saturating_add(
                self.hits
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Hit>()),
            )
            .saturating_add(
                self.boxes
                    .capacity()
                    .saturating_mul(std::mem::size_of::<LayoutBox>()),
            )
            .saturating_add(
                self.text_runs
                    .capacity()
                    .saturating_mul(std::mem::size_of::<TextRun>()),
            )
            .saturating_add(
                self.hits
                    .iter()
                    .map(|hit| match &hit.action {
                        Action::Link { href, .. } => href.capacity(),
                        _ => 0,
                    })
                    .sum::<usize>(),
            )
            .saturating_add(
                self.text_runs
                    .iter()
                    .map(|run| run.text.capacity())
                    .sum::<usize>(),
            )
            .saturating_add(diagnostic_payload_bytes(&self.diagnostics))
    }
}

fn diagnostic_payload_bytes(diagnostics: &crate::style::StyleDiagnostics) -> usize {
    diagnostics.entries.capacity() * std::mem::size_of::<crate::style::StyleDiagnostic>()
        + diagnostics
            .entries
            .iter()
            .map(|entry| entry.message.capacity() + entry.source.capacity())
            .sum::<usize>()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderTimings {
    pub style_ms: f64,
    pub layout_ms: f64,
    pub paint_ms: f64,
    /// Browser composition, geometry and chrome are measured by Chassis.
    pub composition_ms: f64,
    pub chrome_ms: f64,
    pub total_ms: f64,
    pub style_cached: bool,
    pub frame_cached: bool,
    pub paint_ops: usize,
    pub layout_boxes: usize,
    pub dom_nodes: usize,
}

impl RenderTimings {
    pub fn cache_label(&self) -> &'static str {
        if self.frame_cached {
            "frame cache hit"
        } else if self.style_cached {
            "style cache hit"
        } else {
            "uncached render"
        }
    }
}

/// Retention only: exceeding this never rejects CSS or changes rendered output.
/// Counts computed structs and their owned allocations plus diagnostics.
pub const STYLE_CACHE_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct StyleCache {
    key: Option<(u64, u32, u32)>,
    styles: Arc<[crate::style::ComputedStyle]>,
    diagnostics: crate::style::StyleDiagnostics,
    payload_bytes: usize,
}

impl StyleCache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn payload_bytes(&self) -> usize {
        self.payload_bytes
    }

    pub fn style_for(&self, node: usize) -> Option<&crate::style::ComputedStyle> {
        self.styles.get(node)
    }

    fn retain(
        &mut self,
        key: (u64, u32, u32),
        styles: Arc<[crate::style::ComputedStyle]>,
        diagnostics: &crate::style::StyleDiagnostics,
    ) {
        let bytes = std::mem::size_of_val(styles.as_ref())
            + styles
                .iter()
                .map(crate::style::snapshot_heap_bytes)
                .sum::<usize>()
            + diagnostic_payload_bytes(diagnostics);
        self.clear();
        if bytes <= STYLE_CACHE_PAYLOAD_BYTES {
            self.key = Some(key);
            self.styles = styles;
            self.diagnostics = diagnostics.clone();
            self.payload_bytes = bytes;
        }
    }
}

/// Render a document without a window, network service, or JavaScript execution.
/// Like Canvas, the output allocation is owned and dimensioned by the host.
pub fn render(
    document: &Document,
    fonts: &mut Fonts,
    viewport: Viewport,
    controls: &Controls<'_>,
) -> Frame {
    render_scaled(document, fonts, viewport, controls, 1.0)
}

/// Render at an output scale while keeping layout, hits and scrolling in logical
/// pixels. Font outlines are rasterized at physical resolution by the Canvas.
pub fn render_scaled(
    document: &Document,
    fonts: &mut Fonts,
    viewport: Viewport,
    controls: &Controls<'_>,
    scale: f32,
) -> Frame {
    render_scaled_cached(
        document,
        fonts,
        viewport,
        controls,
        scale,
        0,
        &mut StyleCache::default(),
    )
    .0
}

pub fn render_scaled_cached(
    document: &Document,
    fonts: &mut Fonts,
    viewport: Viewport,
    controls: &Controls<'_>,
    scale: f32,
    doc_key: u64,
    style_cache: &mut StyleCache,
) -> (Frame, RenderTimings) {
    let total_started = std::time::Instant::now();
    let mut diagnostics = crate::style::StyleDiagnostics::default();
    let mut timings = RenderTimings {
        dom_nodes: document.nodes.len(),
        ..Default::default()
    };
    let styled = !document.stylesheets.is_empty()
        || !document.resources.is_empty()
        || document
            .nodes
            .iter()
            .any(|node| node.has("style") || node.tag == "svg");
    if styled {
        let cache_key = (doc_key, viewport.width, viewport.height);
        let computed = if doc_key != 0
            && style_cache.key == Some(cache_key)
            && style_cache.styles.len() == document.nodes.len()
        {
            timings.style_cached = true;
            diagnostics = style_cache.diagnostics.clone();
            Ok(Arc::clone(&style_cache.styles))
        } else {
            let style_started = std::time::Instant::now();
            // Release a stale snapshot before computing the next document.
            style_cache.clear();
            let result = crate::style::compute_styles_with_diagnostics(
                document,
                &document.stylesheets,
                (viewport.width as f32, viewport.height as f32),
                &mut diagnostics,
            )
            .map(Arc::<[crate::style::ComputedStyle]>::from);
            timings.style_ms = style_started.elapsed().as_secs_f64() * 1000.0;
            if let Ok(styles) = &result {
                if doc_key != 0 {
                    style_cache.retain(cache_key, Arc::clone(styles), &diagnostics);
                }
            } else {
                style_cache.clear();
            }
            result
        };
        match computed {
            Ok(styles) => {
                match crate::styled_layout::render_scaled_profiled(
                    document, fonts, viewport, controls, styles, scale,
                ) {
                    Ok((mut frame, layout_ms, paint_ms, paint_ops)) => {
                        timings.layout_ms = layout_ms;
                        timings.paint_ms = paint_ms;
                        timings.paint_ops = paint_ops;
                        timings.layout_boxes = frame.boxes.len();
                        diagnostics.omitted = diagnostics
                            .omitted
                            .saturating_add(frame.diagnostics.omitted);
                        for entry in frame.diagnostics.entries.drain(..) {
                            if diagnostics.entries.len()
                                < crate::style::StyleDiagnostics::MAX_ENTRIES
                            {
                                diagnostics.entries.push(entry);
                            } else {
                                diagnostics.omitted = diagnostics.omitted.saturating_add(1);
                            }
                        }
                        frame.diagnostics = diagnostics;
                        timings.total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
                        return (frame, timings);
                    }
                    Err(failure) => {
                        eprintln!(
                            "Styled layout unavailable at node {:?}: {}; using readable-flow fallback",
                            failure.node, failure.reason
                        );
                        diagnostics.record(
                            "layout-fallback",
                            format_args!(
                                "Styled layout unavailable: {}; using readable-flow fallback",
                                failure.reason
                            ),
                            format_args!("{}", document.base_url),
                            failure.node,
                            None,
                        );
                    }
                }
            }
            Err(error) => {
                eprintln!("Styles unavailable; using readable-flow fallback: {error}");
                diagnostics.record(
                    "style-fallback",
                    format_args!("Styles unavailable; using readable-flow fallback: {error}"),
                    format_args!("{}", document.base_url),
                    None,
                    None,
                );
            }
        }
    } else {
        style_cache.clear();
    }
    let paint_started = std::time::Instant::now();
    let mut frame = Renderer {
        document,
        fonts,
        width: viewport.width,
        height: viewport.height,
        scroll: viewport.scroll,
        values: controls.values,
        focused_input: controls.focused_input,
        select_all: controls.select_all,
        input_selection: controls.input_selection,
        hits: Vec::new(),
        boxes: Vec::new(),
        text_runs: Vec::new(),
        scale,
    }
    .paint();
    timings.paint_ms = paint_started.elapsed().as_secs_f64() * 1000.0;
    timings.layout_boxes = frame.boxes.len();
    frame.diagnostics = diagnostics;
    timings.total_ms = total_started.elapsed().as_secs_f64() * 1000.0;
    (frame, timings)
}
struct Renderer<'a> {
    document: &'a Document,
    fonts: &'a mut Fonts,
    width: u32,
    height: u32,
    scroll: i32,
    values: Option<&'a HashMap<usize, String>>,
    focused_input: Option<usize>,
    select_all: bool,
    input_selection: Option<(usize, usize, usize)>,
    hits: Vec<Hit>,
    boxes: Vec<LayoutBox>,
    text_runs: Vec<TextRun>,
    scale: f32,
}
impl Renderer<'_> {
    fn paint(mut self) -> Frame {
        let mut c = Canvas::new_scaled(self.width, self.height, BG, self.scale);
        let left = 32;
        let right = self.width as i32 - 38;
        let mut x = left;
        let mut y = 24 - self.scroll;
        let mut row = 27;
        for i in 0..self.document.items.len() {
            let item = self.document.items[i].clone();
            let node = self.document.item_nodes[i];
            match item {
                Item::Break => {
                    if x > left {
                        y += row;
                        x = left;
                    } else {
                        y += 8;
                    }
                    row = 27;
                }
                Item::Text {
                    text,
                    href,
                    heading,
                } => {
                    let size = if heading { 24. } else { 17. };
                    row = row.max(if heading { 34 } else { 27 });
                    for word in text.split_whitespace() {
                        let space = self.fonts.width(" ", size).ceil() as i32;
                        let ww = self.fonts.width(word, size).ceil() as i32;
                        if x > left && x + ww > right {
                            x = left;
                            y += row;
                        }
                        self.layout_box(i, x, y, ww.max(1) as u32, row as u32);
                        if self.text_runs.len() < MAX_TEXT_RUNS && !word.is_empty() {
                            self.text_runs.push(TextRun {
                                node,
                                x,
                                y,
                                width: ww.max(1) as u32,
                                height: row as u32,
                                font_size: size,
                                text: word.to_owned(),
                            });
                        }
                        if y + row > 0 && y < self.height as i32 - 1 {
                            c.text(
                                self.fonts,
                                x,
                                y,
                                word,
                                size,
                                if href.is_some() { LINK } else { INK },
                            );
                            if let Some(ref url) = href {
                                self.hit(
                                    x,
                                    y,
                                    ww.max(1) as u32,
                                    row as u32,
                                    Action::Link {
                                        node,
                                        href: url.clone(),
                                    },
                                );
                            }
                        }
                        x += ww + space;
                    }
                }
                Item::Input { value, kind, .. } => {
                    if x > left {
                        y += row;
                        x = left;
                    }
                    let w = (right - left).clamp(120, 550) as u32;
                    self.layout_box(i, x, y, w, 40);
                    if y + 40 > 0 && y < self.height as i32 - 1 {
                        c.rect(
                            x,
                            y,
                            w,
                            40,
                            if self.focused_input == Some(node) {
                                0x277453
                            } else {
                                0x9ca79a
                            },
                        );
                        c.rect(x + 2, y + 2, w - 4, 36, 0xffffff);
                        let mut value = self
                            .values
                            .and_then(|values| values.get(&node))
                            .cloned()
                            .unwrap_or(value);
                        if kind == "password" {
                            value = "•".repeat(value.chars().count());
                        }
                        let avail = w as f32 - 22.;
                        if self.focused_input == Some(node)
                            && let Some((sa, sb, caret)) = self.input_selection
                        {
                            let (shown, sx0, sx1, cx) =
                                slice_visible_input(self.fonts, &value, 17., avail, sa, sb, caret);
                            if sx1 > sx0 {
                                c.rect(x + 8 + sx0, y + 7, (sx1 - sx0) as u32 + 1, 25, 0xc6dfed);
                            }
                            c.text(self.fonts, x + 9, y + 8, &shown, 17., INK);
                            let caret_x = (x + 9 + cx).min(x + w as i32 - 6);
                            c.rect(caret_x, y + 8, 2, 23, 0x277453);
                        } else {
                            let value = fit_tail(self.fonts, &value, 17., avail);
                            if self.focused_input == Some(node) && self.select_all {
                                c.rect(
                                    x + 7,
                                    y + 7,
                                    self.fonts.width(&value, 17.) as u32 + 2,
                                    25,
                                    0xc6dfed,
                                );
                            }
                            c.text(self.fonts, x + 9, y + 8, &value, 17., INK);
                        }
                        self.hit(x, y, w, 40, Action::Input(node));
                    }
                    y += 50;
                    row = 27;
                }
                Item::Submit { label, .. } => {
                    let w = (self.fonts.width(&label, 16.) as u32 + 28)
                        .min((right - left).max(80) as u32);
                    if x > left && x + w as i32 > right {
                        x = left;
                        y += 42;
                    }
                    self.layout_box(i, x, y, w, 36);
                    if y + 38 > 0 && y < self.height as i32 - 1 {
                        c.rect(x, y, w, 36, 0xe0e8f3);
                        c.text(self.fonts, x + 12, y + 7, &label, 16., LINK);
                        self.hit(x, y, w, 36, Action::Submit(node));
                    }
                    x += w as i32 + 12;
                    row = 46;
                }
                Item::Image { alt, .. } => {
                    let label = if alt.is_empty() {
                        "[image unsupported]".to_string()
                    } else {
                        format!("[image: {alt}]")
                    };
                    let ww = self.fonts.width(&label, 14.).ceil() as i32;
                    if x + ww > right {
                        x = left;
                        y += row;
                    }
                    self.layout_box(i, x, y, ww.max(1) as u32, row as u32);
                    if y + row > 0 {
                        c.text(self.fonts, x, y, &label, 14., 0x667164);
                    }
                    x += ww + 8;
                }
            }
        }
        let content_height = y + self.scroll + row;
        Frame {
            canvas: c,
            hits: self.hits,
            boxes: self.boxes,
            text_runs: self.text_runs,
            content_height,
            diagnostics: Default::default(),
        }
    }
    fn hit(&mut self, x: i32, y: i32, w: u32, h: u32, action: Action) {
        let top = y.max(0);
        let bottom = (y + h as i32).min(self.height as i32 - 1);
        if bottom > top {
            self.hits.push(Hit {
                x,
                y: top,
                w,
                h: (bottom - top) as u32,
                action,
            });
        }
    }
    fn layout_box(&mut self, item: usize, x: i32, y: i32, width: u32, height: u32) {
        if self.boxes.len() < 100_000
            && let Some(&node) = self.document.item_nodes.get(item)
        {
            self.boxes.push(LayoutBox {
                node,
                x,
                y,
                width,
                height,
            });
        }
    }
}
pub(crate) fn fit_tail(fonts: &mut Fonts, text: &str, size: f32, width: f32) -> String {
    let chars: Vec<_> = text.chars().rev().take(512).collect();
    let mut low = 0;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high).div_ceil(2);
        let candidate: String = chars[..mid].iter().rev().collect();
        if fonts.width(&candidate, size) <= width {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    chars[..low].iter().rev().collect()
}

pub(crate) fn slice_visible_input(
    fonts: &mut Fonts,
    text: &str,
    size: f32,
    width: f32,
    sel_a: usize,
    sel_b: usize,
    caret: usize,
) -> (String, i32, i32, i32) {
    let chars: Vec<char> = text.chars().take(4096).collect();
    let len = chars.len();
    let caret = caret.min(len);
    let (s0, s1) = (sel_a.min(sel_b).min(len), sel_a.max(sel_b).min(len));
    let width = width.max(4.0);
    let mut start = 0usize;
    if fonts.width(text, size) > width {
        start = caret;
        while start > 0 {
            let cand: String = chars[start - 1..caret].iter().collect();
            if fonts.width(&cand, size) <= width {
                start -= 1;
            } else {
                break;
            }
        }
    }
    let mut end = caret;
    while end < len {
        let cand: String = chars[start..=end].iter().collect();
        if fonts.width(&cand, size) <= width {
            end += 1;
        } else {
            break;
        }
    }
    let shown: String = chars[start..end].iter().collect();
    let measure = |to: usize, fonts: &mut Fonts| -> i32 {
        let to = to.clamp(start, end);
        if to <= start {
            0
        } else {
            let sub: String = chars[start..to].iter().collect();
            fonts.width(&sub, size).round() as i32
        }
    };
    let sx0 = measure(s0, fonts);
    let sx1 = measure(s1, fonts);
    let cx = measure(caret, fonts);
    (shown, sx0, sx1, cx)
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn style_cache_shares_styles_and_matches_uncached_scroll_pixels_and_hits() {
        let mut fonts = Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap();
        let doc = crate::document::parse(
            &format!(
                "<style>p{{margin:4px;color:#123456}}</style>{}",
                "<p><a href='/next'>Cached layout and glyphs</a></p>".repeat(30)
            ),
            "https://example.test/",
        );
        let mut cache = StyleCache::default();
        let initial = Viewport {
            width: 320,
            height: 240,
            scroll: 0,
        };
        let (_, timing) = render_scaled_cached(
            &doc,
            &mut fonts,
            initial,
            &Controls::default(),
            1.25,
            1,
            &mut cache,
        );
        assert!(!timing.style_cached);
        assert!(cache.payload_bytes() > 0 && cache.payload_bytes() <= STYLE_CACHE_PAYLOAD_BYTES);
        let styles = Arc::clone(&cache.styles);
        let scrolled = Viewport {
            scroll: 65,
            ..initial
        };
        let (cached, timing) = render_scaled_cached(
            &doc,
            &mut fonts,
            scrolled,
            &Controls::default(),
            1.25,
            1,
            &mut cache,
        );
        assert!(timing.style_cached);
        assert!(Arc::ptr_eq(&styles, &cache.styles));
        let fresh = render_scaled(&doc, &mut fonts, scrolled, &Controls::default(), 1.25);
        assert_eq!(cached.canvas.pixels, fresh.canvas.pixels);
        assert_eq!(cached.boxes, fresh.boxes);
        assert_eq!(cached.content_height, fresh.content_height);
        assert_eq!(cached.diagnostics, fresh.diagnostics);
        let hits = |frame: &Frame| {
            frame
                .hits
                .iter()
                .map(|h| (h.x, h.y, h.w, h.h, h.action.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(hits(&cached), hits(&fresh));
        for (key, viewport) in [
            (2, scrolled),
            (
                2,
                Viewport {
                    width: 360,
                    ..scrolled
                },
            ),
            (
                2,
                Viewport {
                    height: 300,
                    width: 360,
                    ..scrolled
                },
            ),
        ] {
            let (_, timing) = render_scaled_cached(
                &doc,
                &mut fonts,
                viewport,
                &Controls::default(),
                1.25,
                key,
                &mut cache,
            );
            assert!(!timing.style_cached);
        }
        let plain = crate::document::parse("<p>Plain replacement</p>", "https://example.test/");
        render_scaled_cached(
            &plain,
            &mut fonts,
            initial,
            &Controls::default(),
            1.,
            3,
            &mut cache,
        );
        assert_eq!(cache.payload_bytes(), 0);
    }

    #[test]
    fn style_cache_admission_counts_owned_heap_and_drops_oversized_snapshots() {
        let diagnostics = crate::style::StyleDiagnostics::default();
        let mut cache = StyleCache::default();
        cache.retain(
            (1, 320, 240),
            vec![crate::style::ComputedStyle::default()].into(),
            &diagnostics,
        );
        assert!(cache.payload_bytes() > 0);
        let mut large = crate::style::ComputedStyle::default();
        large.font_families = vec!["x".repeat(STYLE_CACHE_PAYLOAD_BYTES)];
        let oversized: Arc<[_]> = vec![large].into();
        cache.retain((2, 320, 240), Arc::clone(&oversized), &diagnostics);
        assert!(cache.key.is_none());
        assert_eq!(cache.payload_bytes(), 0);
        assert!(cache.styles.is_empty());
        // Admission changes ownership only; the live render input survives.
        assert_eq!(
            oversized[0].font_families[0].len(),
            STYLE_CACHE_PAYLOAD_BYTES
        );
    }

    #[test]
    fn bare_inline_svg_uses_the_same_renderer_as_an_empty_stylesheet() {
        let mut fonts = Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap();
        let svg = "<svg width='10' height='10'><rect width='10' height='10' fill='red'/></svg>";
        let plain = crate::document::parse(svg, "https://fixture.example/");
        let styled =
            crate::document::parse(&format!("<style></style>{svg}"), "https://fixture.example/");
        let viewport = Viewport {
            width: 100,
            height: 100,
            scroll: 0,
        };
        let a = render(&plain, &mut fonts, viewport, &Controls::default());
        let b = render(&styled, &mut fonts, viewport, &Controls::default());
        assert_eq!(
            a.canvas
                .pixels
                .iter()
                .filter(|&&pixel| pixel == 0xff0000)
                .count(),
            100
        );
        assert_eq!(a.canvas.pixels, b.canvas.pixels);
        assert_eq!(a.content_height, b.content_height);
        assert!(!a.boxes.is_empty());
        assert!(!a.diagnostics.entries.iter().any(|d| matches!(
            d.kind,
            "image-unsupported" | "layout-fallback" | "style-fallback"
        )));
    }

    #[test]
    fn scaled_styled_and_fallback_frames_keep_logical_layout_and_hits() {
        let mut fonts = Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap();
        for style in ["", " style='font-size:17px;width:210px'"] {
            let doc = crate::document::parse(
                &format!(
                    "<body{style}><p><a href='/next'>A longer link that wraps across several lines</a></p><form><input name=q value=test><button>Go</button></form></body>"
                ),
                "https://example.test/",
            );
            let viewport = Viewport {
                width: 320,
                height: 240,
                scroll: 8,
            };
            let controls = Controls::default();
            let original = render(&doc, &mut fonts, viewport, &controls);
            assert!(!original.hits.is_empty());
            assert!(!original.boxes.is_empty());
            for scale in [1.0, 1.25, 2.0] {
                let frame = render_scaled(&doc, &mut fonts, viewport, &controls, scale);
                assert_eq!(frame.canvas.width, (320.0 * scale).round() as u32);
                assert_eq!(frame.canvas.height, (240.0 * scale).round() as u32);
                assert_eq!(frame.content_height, original.content_height);
                assert_eq!(frame.boxes, original.boxes);
                let hit_geometry = |frame: &Frame| {
                    frame
                        .hits
                        .iter()
                        .map(|hit| (hit.x, hit.y, hit.w, hit.h, hit.action.clone()))
                        .collect::<Vec<_>>()
                };
                assert_eq!(hit_geometry(&frame), hit_geometry(&original));
                if scale == 1.0 {
                    assert_eq!(frame.canvas.pixels, original.canvas.pixels);
                }
            }
        }
    }
}
