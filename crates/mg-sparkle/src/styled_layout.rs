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

const MAX_OPS: usize = 200_000;
const MAX_COLUMNS: usize = 256;
const MAX_DEPTH: usize = 256;
const MAX_EXTENT: f32 = 1_000_000.0;
const MAX_TEXT_RUN: usize = 16_384;

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

#[derive(Clone)]
enum Paint {
    Rect(Rect, Color),
    Text(Rect, usize, String),
    Image(Rect, String, Option<(usize, String)>),
    Control(Rect, usize, String, bool),
}

impl Paint {
    fn rect_mut(&mut self) -> &mut Rect {
        match self {
            Self::Rect(r, _) | Self::Text(r, ..) | Self::Image(r, ..) | Self::Control(r, ..) => r,
        }
    }
}

#[derive(Default)]
struct Scene {
    paints: Vec<Paint>,
    boxes: Vec<LayoutBox>,
    hits: Vec<(Rect, Action)>,
}

impl Scene {
    fn mark(&self) -> (usize, usize, usize) {
        (self.paints.len(), self.boxes.len(), self.hits.len())
    }
    fn shift(&mut self, start: (usize, usize, usize), dx: f32, dy: f32) {
        for op in &mut self.paints[start.0..] {
            let r = op.rect_mut();
            r.x += dx;
            r.y += dy;
        }
        for b in &mut self.boxes[start.1..] {
            b.x += dx.round() as i32;
            b.y += dy.round() as i32;
        }
        for (r, _) in &mut self.hits[start.2..] {
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
        }
    }
    fn paint(&mut self, op: Paint) {
        if self.paints.len() < MAX_OPS {
            self.paints.push(op);
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
    rows: Vec<(usize, Vec<Cell>)>,
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
    render_scaled(document, fonts, viewport, controls, styles, 1.0)
}

pub(crate) fn render_scaled(
    document: &Document,
    fonts: &mut Fonts,
    viewport: Viewport,
    controls: &Controls<'_>,
    styles: Vec<ComputedStyle>,
    scale: f32,
) -> Option<Frame> {
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
        return None;
    }
    let mut layout = Layout {
        document,
        fonts,
        controls,
        styles,
        scene: Scene::default(),
        intrinsic: HashMap::new(),
        natural_images: HashMap::new(),
        table_cache: HashMap::new(),
        budget: 2_000_000,
        exhausted: false,
    };
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
    if layout.exhausted
        || layout.scene.paints.len() >= MAX_OPS
        || layout.scene.boxes.len() >= MAX_OPS
    {
        return None;
    }
    // Explicit block heights can be smaller than their visible contents. Keep
    // that laid-out overflow reachable even when html/body has height:100%.
    let height = layout
        .scene
        .boxes
        .iter()
        .fold(height, |height, rect| {
            height.max(rect.y as f32 + rect.height as f32)
        })
        .clamp(0.0, MAX_EXTENT);
    let mut canvas = Canvas::new_scaled(viewport.width, viewport.height, background.rgb, scale);
    let mut decoded = HashMap::new();
    let mut decoded_bytes = 0usize;
    for op in &layout.scene.paints {
        match op {
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
            Paint::Image(rect, url, fallback) => {
                if !visible(*rect, viewport) || rect.w <= 0.0 || rect.h <= 0.0 {
                    continue;
                }
                let key = (url.clone(), rect.w.ceil() as u32, rect.h.ceil() as u32);
                if !decoded.contains_key(&key)
                    && let Some(resource) = document.resources.get(url)
                {
                    if image_cache_admits(key.1, key.2, decoded_bytes, decoded.len()) {
                        let image = crate::images::decode(
                            &resource.bytes,
                            &resource.content_type,
                            Some(key.1),
                            Some(key.2),
                        )
                        .ok();
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
    Some(Frame {
        canvas,
        hits,
        boxes: layout.scene.boxes,
        content_height: height.ceil().max(0.0) as i32,
        diagnostics: Default::default(),
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
        let left = canvas
            .physical_edge(r.x.round() as i64)
            .clamp(0, i64::from(canvas.width)) as usize;
        let top = canvas
            .physical_edge((r.y.round() - scroll as f32) as i64)
            .clamp(0, i64::from(canvas.height)) as usize;
        let right = canvas
            .physical_edge((r.x + r.w).ceil() as i64)
            .clamp(0, i64::from(canvas.width)) as usize;
        let bottom = canvas
            .physical_edge((r.y + r.h - scroll as f32).ceil() as i64)
            .clamp(0, i64::from(canvas.height)) as usize;
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
    scene: Scene,
    intrinsic: HashMap<usize, Intrinsic>,
    natural_images: HashMap<String, (f32, f32)>,
    table_cache: HashMap<usize, Table>,
    budget: usize,
    exhausted: bool,
}

impl Layout<'_, '_> {
    fn spend(&mut self, depth: usize) -> bool {
        if depth > MAX_DEPTH || self.budget == 0 {
            self.exhausted = true;
            false
        } else {
            self.budget -= 1;
            true
        }
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
            if self.scene.hits.len() < MAX_OPS {
                self.scene.hits.push((r, action));
            }
        }
    }
    fn decorate(&mut self, id: usize, r: Rect) {
        let s = &self.styles[id];
        if !s.visible {
            return;
        }
        self.scene.paint(Paint::Rect(r, s.background_color));
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
            self.scene.paint(Paint::Rect(edge, s.border_color[i]));
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
            self.scene
                .paint(Paint::Image(Rect { w, h, ..r }, url, None));
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
        let width = self.styles[id].width.resolve(basis);
        let height = height_length(self.styles[id].height, containing_height);
        if let (Some(width), Some(height)) = (width, height) {
            return (
                width.clamp(0.0, MAX_EXTENT),
                constrain_height(&self.styles[id], height, containing_height),
            );
        }
        let image_url = self.image_url(id);
        if self.document.nodes[id].tag == "img"
            && width != Some(0.0)
            && height != Some(0.0)
            && let Some(url) = &image_url
            && !self.natural_images.contains_key(url)
            && self.natural_images.len() < 128
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
        let (default_w, default_h) = if node.tag == "img" {
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
        };
        if node.tag != "img" {
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
    fn intrinsic(&mut self, id: usize, depth: usize) -> Intrinsic {
        if let Some(size) = self.intrinsic.get(&id) {
            return *size;
        }
        if !self.spend(depth) || self.styles[id].display == Display::None {
            return Intrinsic::default();
        }
        let node = &self.document.nodes[id];
        let s = self.styles[id].clone();
        let mut result = if node.tag == "#text" {
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
        } else if matches!(node.tag.as_str(), "img" | "input" | "button" | "textarea") {
            let (w, _) = self.replaced_size(id, 0.0, None);
            Intrinsic { min: w, max: w }
        } else if matches!(s.display, Display::Table | Display::InlineTable) {
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
                let v = self.intrinsic(child, depth + 1);
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
        };
        let (margin, padding) = self.edges(id, 0.0);
        let edges = margin[1] + margin[3] + padding[1] + padding[3];
        if let Length::Px(width) = s.width {
            result.min = result.min.max(width);
            result.max = result.max.max(width);
        }
        if let Length::Px(width) = s.min_width {
            result.min = result.min.max(width);
            result.max = result.max.max(width);
        }
        result.min = (result.min + edges).clamp(0.0, MAX_EXTENT);
        result.max = (result.max + edges).max(result.min).min(MAX_EXTENT);
        self.intrinsic.insert(id, result);
        result
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
                        rows.push((child, cells));
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
        if !self.spend(depth) || self.styles[id].display == Display::None {
            return (0.0, 0.0);
        }
        let s = self.styles[id].clone();
        let (mut margin, padding) = self.edges(id, available);
        let is_table = matches!(s.display, Display::Table | Display::InlineTable);
        let horizontal = padding[1] + padding[3];
        let mut width = forced.unwrap_or_else(|| {
            s.width
                .resolve(available)
                .map(|w| w + if is_table { 0.0 } else { horizontal })
                .unwrap_or_else(|| {
                    if is_table {
                        let i = self.intrinsic(id, depth + 1);
                        i.max.min(available).max(i.min)
                    } else {
                        (available - margin[1] - margin[3]).max(horizontal)
                    }
                })
        });
        width = width.max(s.min_width.resolve(available).unwrap_or(0.0) + horizontal);
        if let Some(max) = s.max_width.resolve(available) {
            width = width.min(max + horizontal);
        }
        width = width.clamp(0.0, MAX_EXTENT);
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
        let bx = x + margin[3];
        let by = y + margin[0];
        // Reserve decoration before children so parent backgrounds stay behind them.
        let start = self.scene.paints.len();
        self.decorate(
            id,
            Rect {
                x: bx,
                y: by,
                w: width,
                h: 0.0,
            },
        );
        let content_x = bx + padding[3];
        let content_y = by + padding[0];
        let content_w = (width - horizontal).max(0.0);
        let specified_height = height_length(s.height, containing_height)
            .map(|height| constrain_height(&s, height, containing_height));
        // The synthetic document represents the initial containing block.
        // Minimum/maximum height alone must not make an auto height definite.
        let child_height = if id == 0 {
            containing_height
        } else {
            specified_height
        };
        let content_h = if is_table {
            self.layout_table(id, content_x, content_y, content_w, child_height, depth + 1)
        } else if matches!(
            self.document.nodes[id].tag.as_str(),
            "img" | "input" | "button" | "textarea"
        ) {
            self.replaced(id, content_x, content_y, content_w, containing_height)
                .1
        } else {
            self.flow(id, content_x, content_y, content_w, child_height, depth + 1)
        };
        // Table and cell heights remain minimums for their contents. Ordinary
        // blocks can overflow their specified height without enlarging it.
        let used_height = if is_table || s.display == Display::TableCell {
            content_h.max(specified_height.unwrap_or(0.0))
        } else {
            specified_height.unwrap_or(content_h)
        };
        let h = constrain_height(&s, used_height, containing_height) + padding[0] + padding[2];
        let h = h.clamp(0.0, MAX_EXTENT);
        let rect = Rect {
            x: bx,
            y: by,
            w: width,
            h,
        };
        // Update reserved background/border boxes after determining content height.
        self.finish_decoration(start, id, rect);
        self.scene.box_for(id, rect);
        self.hit(id, rect);
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
            if let Some(Paint::Rect(old, _)) = self.scene.paints.get_mut(start + i) {
                *old = rect;
            }
        }
    }
    fn layout_table(
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
            let row_start = self.scene.paints.len();
            self.decorate(
                row,
                Rect {
                    x,
                    y: top,
                    w: width,
                    h: 0.0,
                },
            );
            let row_height = height_length(self.styles[row].height, containing_height)
                .map(|height| constrain_height(&self.styles[row], height, containing_height));
            let mut height = constrain_height(
                &self.styles[row],
                row_height.unwrap_or(0.0),
                containing_height,
            );
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
                let start = self.scene.mark();
                let (_, h) = self.block(cell.node, left, top, cw, row_height, Some(cw), depth + 1);
                height = height.max(h);
                ranges.push((cell.node, start, self.scene.mark(), h, left, cw));
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
                    for b in &mut self.scene.boxes[start.1..end.1] {
                        b.y += dy.round() as i32;
                    }
                    for (r, _) in &mut self.scene.hits[start.2..end.2] {
                        r.y += dy;
                    }
                }
                // The cell background/border spans the complete row, not only its content.
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
            top = (top + height + spacing[1]).min(MAX_EXTENT);
        }
        top - y
    }
    fn tokens(&mut self, id: usize, out: &mut Vec<Token>, depth: usize) {
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
        } else if matches!(node.tag.as_str(), "img" | "input" | "button" | "textarea")
            || matches!(s.display, Display::InlineBlock | Display::InlineTable)
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
    fn flow(
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
                        top +=
                            self.line(id, x, top, width, containing_height, &line, used, depth + 1);
                        line.clear();
                    }
                    used = 0.0;
                    pending_space = None;
                    top += self
                        .block(child, x, top, width, containing_height, None, depth + 1)
                        .1;
                }
                Token::Break => {
                    top += if line.is_empty() {
                        self.line_height(id)
                    } else {
                        self.line(id, x, top, width, containing_height, &line, used, depth + 1)
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
                        top +=
                            self.line(id, x, top, width, containing_height, &line, used, depth + 1);
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
            top += self.line(id, x, top, width, containing_height, &line, used, depth + 1);
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
        if self.document.nodes[id].tag == "img" && self.styles[id].width == Length::Auto {
            // A definite percentage height can change an image's auto width.
            // Use that same width for wrapping/alignment and for painting.
            let (w, _) = self.replaced_size(id, width, containing_height);
            let (m, p) = self.edges(id, width);
            w + m[1] + m[3] + p[1] + p[3]
        } else {
            self.intrinsic(id, depth).max.min(width)
        }
    }
    fn line(
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
                    let (_, h) = self.replaced_size(*child, width, containing_height);
                    let (m, p) = self.edges(*child, width);
                    height = height.max(h + m[0] + m[2] + p[0] + p[2]);
                }
                _ => {}
            }
        }
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
                    let s = &self.styles[*node];
                    let ty = y + (ascent - s.font_size).max(0.0) + (height - ascent).max(0.0) * 0.1;
                    let r = Rect {
                        x: left,
                        y: ty,
                        w: *tw,
                        h: self.line_height(*node),
                    };
                    if s.visible {
                        self.scene.paint(Paint::Text(r, *node, text.clone()));
                        self.hit(*node, r);
                    }
                    self.scene.box_for(*node, r);
                    left += tw;
                }
                Token::Space { width, .. } | Token::Gap(width) => left += width,
                Token::Box(child) => {
                    let start = self.scene.mark();
                    let (w, h) = if matches!(
                        self.document.nodes[*child].tag.as_str(),
                        "img" | "input" | "button" | "textarea"
                    ) {
                        self.replaced(*child, left, y, width, containing_height)
                    } else {
                        let w = self.intrinsic(*child, depth + 1).max.min(width);
                        self.block(*child, left, y, w, containing_height, Some(w), depth + 1)
                    };
                    if h < height {
                        self.scene.shift(start, 0.0, height - h);
                    }
                    left += w;
                }
                _ => {}
            }
        }
        height
    }
    fn replaced(
        &mut self,
        id: usize,
        x: f32,
        y: f32,
        basis: f32,
        containing_height: Option<f32>,
    ) -> (f32, f32) {
        let (w, h) = self.replaced_size(id, basis, containing_height);
        let (m, p) = self.edges(id, basis);
        let r = Rect {
            x: x + m[3],
            y: y + m[0],
            w: w + p[1] + p[3],
            h: h + p[0] + p[2],
        };
        self.decorate(id, r);
        let content = Rect {
            x: r.x + p[3],
            y: r.y + p[0],
            w,
            h,
        };
        let node = &self.document.nodes[id];
        if !self.styles[id].visible {
            self.scene.box_for(id, r);
            return (r.w + m[1] + m[3], r.h + m[0] + m[2]);
        }
        if node.tag == "img" {
            if let Some(url) = self.image_url(id)
                && self.document.resources.contains_key(&url)
            {
                self.scene.paint(Paint::Image(
                    content,
                    url,
                    Some((id, node.attr("alt").unwrap_or("").to_owned())),
                ));
            } else if w > 0.0 && h > 0.0 {
                self.scene.paint(Paint::Rect(
                    content,
                    Color {
                        rgb: 0xdddddd,
                        alpha: 1.0,
                    },
                ));
                if let Some(alt) = node.attr("alt").filter(|s| !s.is_empty()) {
                    self.scene.paint(Paint::Text(content, id, alt.to_owned()));
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
            self.scene.paint(Paint::Control(r, id, text, submit));
            if self.styles[id].pointer_events {
                self.scene.hits.push((
                    r,
                    if submit {
                        Action::Submit(id)
                    } else {
                        Action::Input(id)
                    },
                ));
            }
        }
        self.scene.box_for(id, r);
        (r.w + m[1] + m[3], r.h + m[0] + m[2])
    }
}

fn is_block(display: Display) -> bool {
    matches!(
        display,
        Display::Block
            | Display::Table
            | Display::TableRowGroup
            | Display::TableHeaderGroup
            | Display::TableFooterGroup
            | Display::TableRow
            | Display::TableCell
            | Display::ListItem
    )
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
            styles: computed,
            scene: Scene::default(),
            intrinsic: HashMap::new(),
            natural_images: HashMap::new(),
            table_cache: HashMap::new(),
            budget: 2_000_000,
            exhausted: false,
        };
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
