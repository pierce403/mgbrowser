//! Headless document rendering in page-relative pixel coordinates.
//! Styled documents use the bounded DOM renderer; documents without author CSS
//! or loaded images retain the historical readable-flow fallback.
use crate::{
    document::{Document, Item},
    paint::{Canvas, Fonts},
};
use std::collections::HashMap;
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
}
pub struct Frame {
    pub canvas: Canvas,
    pub hits: Vec<Hit>,
    pub boxes: Vec<LayoutBox>,
    pub content_height: i32,
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
    let styled = !document.stylesheets.is_empty()
        || !document.resources.is_empty()
        || document.nodes.iter().any(|node| node.has("style"));
    if styled {
        match crate::style::compute_styles(
            document,
            &document.stylesheets,
            (viewport.width as f32, viewport.height as f32),
        ) {
            Ok(styles) => {
                match crate::styled_layout::render_scaled(
                    document, fonts, viewport, controls, styles, scale,
                ) {
                    Some(frame) => return frame,
                    None => eprintln!("Styled layout limit reached; using readable-flow fallback"),
                }
            }
            Err(error) => eprintln!("Styles unavailable; using readable-flow fallback: {error}"),
        }
    }
    Renderer {
        document,
        fonts,
        width: viewport.width,
        height: viewport.height,
        scroll: viewport.scroll,
        values: controls.values,
        focused_input: controls.focused_input,
        select_all: controls.select_all,
        hits: Vec::new(),
        boxes: Vec::new(),
        scale,
    }
    .paint()
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
    hits: Vec<Hit>,
    boxes: Vec<LayoutBox>,
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
                        let value = fit_tail(self.fonts, &value, 17., w as f32 - 22.);
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
            content_height,
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

#[cfg(test)]
mod scale_tests {
    use super::*;

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
