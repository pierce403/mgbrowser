//! Pass-local flex/grid geometry over Mg's DOM, not another owned document.
//!
//! Layout is measured completely before any scene emission. A failed callback
//! poisons the whole pass, including its caches. Limits bound admitted work and
//! scratch space; they are not process-level CPU or allocator containment.
use super::formatting_style::{LeafStyle, StyleView};
use super::{Intrinsic, Layout, MAX_DEPTH, MAX_EXTENT, Rect, Token, is_css_space};
use crate::style::{
    BoxSizing, Display, GridAutoFlow, GridLine, IntrinsicSize, Length, Position, WhiteSpace,
};
use std::{cell::Cell, ops::Range};
use taffy::{
    AvailableSpace, Cache, CacheTree, Layout as Geometry, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, RequestedAxis,
    RunMode, SizingMode, TraversePartialTree, compute_cached_layout, compute_flexbox_layout,
    compute_grid_layout, compute_leaf_layout,
    geometry::{Line, Size},
};

const MAX_ITEMS: usize = 512;
const MAX_TRACKS: usize = 128;
const MAX_SCRATCH: usize = 16 * 1024 * 1024;
// Includes the slot, Cache, raw Layout, source IDs, sorting and conservative
// allowances for Taffy's temporary item/track vectors. Text is charged apart.
const ITEM_SCRATCH: usize = 4096;
const MAX_GRID_WORK: usize = 1_000_000;

#[derive(Clone, Copy)]
enum Kind {
    Root,
    Element,
    Anonymous { start: usize, end: usize },
}

struct Slot {
    node: usize,
    kind: Kind,
    natural: Option<Size<f32>>,
    ratio: Option<f32>,
    intrinsic_min: Option<f32>,
    intrinsic_max: Option<f32>,
    definite_height: bool,
    cache: Cache,
    geometry: Geometry,
    written: bool,
}

impl Slot {
    fn new(node: usize, kind: Kind) -> Self {
        Self {
            node,
            kind,
            natural: None,
            ratio: None,
            intrinsic_min: None,
            intrinsic_max: None,
            definite_height: false,
            cache: Cache::new(),
            geometry: Geometry::default(),
            written: false,
        }
    }
}

struct Adapter<'l, 'a, 'c> {
    layout: &'l mut Layout<'a, 'c>,
    slots: Vec<Slot>,
    sources: Vec<usize>,
    depth: usize,
    charge: usize,
    invalid: Cell<bool>,
}

impl Drop for Adapter<'_, '_, '_> {
    fn drop(&mut self) {
        debug_assert!(self.layout.format_scratch >= self.charge);
        self.layout.format_scratch = self.layout.format_scratch.saturating_sub(self.charge);
    }
}

fn dimension(value: f32) -> bool {
    value.is_finite() && (0.0..=MAX_EXTENT).contains(&value)
}

fn space(value: AvailableSpace) -> bool {
    match value {
        AvailableSpace::Definite(value) => value.is_finite() && value.abs() <= MAX_EXTENT,
        AvailableSpace::MinContent | AvailableSpace::MaxContent => true,
    }
}

fn valid_size(size: Size<f32>) -> bool {
    dimension(size.width) && dimension(size.height)
}

// Visits only this context's participants. No heap-backed traversal stack or
// copied subtree. The count/admission pass runs before any context allocation.
fn participants(
    layout: &mut Layout<'_, '_>,
    id: usize,
    depth: usize,
    visit: &mut impl FnMut(usize, bool, usize) -> bool,
) -> bool {
    if depth > MAX_DEPTH {
        layout.fail(id, "formatting participant depth bound exceeded");
        return false;
    }
    for index in 0..layout.document.nodes[id].children.len() {
        let child = layout.document.nodes[id].children[index];
        if layout.budget == 0 {
            layout.fail(child, "formatting participant work budget exhausted");
            return false;
        }
        if !layout.spend(depth) {
            return false;
        }
        let style = &layout.styles[child];
        if style.display == Display::None
            || matches!(style.layout.position, Position::Absolute | Position::Fixed)
        {
            continue;
        }
        if style.display == Display::Contents && layout.document.nodes[child].tag != "#text" {
            if !participants(layout, child, depth + 1, visit) {
                return false;
            }
        } else {
            let node = &layout.document.nodes[child];
            let text = node.tag == "#text";
            if !visit(child, text, if text { node.text.len() } else { 0 }) {
                layout.fail(id, "formatting participant count bound exceeded");
                return false;
            }
        }
    }
    true
}

/// Compute this formatting root's content box. The caller owns its outer box,
/// positioning and overflow. Nested contexts reenter via Mg's measurement APIs.
pub(super) fn run<const EMIT: bool>(
    layout: &mut Layout<'_, '_>,
    id: usize,
    x: f32,
    y: f32,
    width: AvailableSpace,
    height: Option<f32>,
    depth: usize,
) -> Option<Size<f32>> {
    let result = run_inner::<EMIT>(layout, id, x, y, width, height, depth);
    if result.is_none() {
        layout.fail(id, "formatting admission or measurement rejected");
    }
    result
}

fn run_inner<const EMIT: bool>(
    layout: &mut Layout<'_, '_>,
    id: usize,
    x: f32,
    y: f32,
    width: AvailableSpace,
    height: Option<f32>,
    depth: usize,
) -> Option<Size<f32>> {
    if layout.exhausted
        || !layout.spend(depth)
        || width.into_option().is_some_and(|value| !dimension(value))
        || height.is_some_and(|height| !dimension(height))
        || !x.is_finite()
        || !y.is_finite()
        || x.abs() > MAX_EXTENT
        || y.abs() > MAX_EXTENT
        || !matches!(
            layout.styles[id].display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        )
    {
        return None;
    }
    if let Err(reason) = StyleView::root_content(&layout.styles[id]).validate() {
        layout.fail(id, reason);
        return None;
    }
    let mut count = 0usize;
    let mut text_bytes = 0usize;
    if !participants(layout, id, depth + 1, &mut |_, _, bytes| {
        count += 1;
        text_bytes = text_bytes.saturating_add(bytes);
        count <= MAX_ITEMS
    }) {
        return None;
    }
    // Tokens and the active wrapped line may each grow geometrically. Charge
    // metadata, word allocations and shaping scratch conservatively per byte.
    let text_charge = text_bytes.checked_mul(4 * std::mem::size_of::<Token>() + 64)?;
    let charge = (count + 1)
        .checked_mul(ITEM_SCRATCH)?
        .checked_add(text_charge)?;
    let total = layout.format_scratch.checked_add(charge)?;
    if total > MAX_SCRATCH {
        layout.fail(id, "formatting live scratch bound exceeded");
        return None;
    }
    layout.format_scratch = total;
    let mut adapter = Adapter {
        layout,
        slots: Vec::new(),
        sources: Vec::new(),
        depth,
        charge,
        invalid: Cell::new(false),
    };
    adapter.sources.reserve_exact(count);
    if !participants(adapter.layout, id, depth + 1, &mut |node, _, _| {
        adapter.sources.push(node);
        true
    }) {
        return None;
    }
    adapter.slots.reserve_exact(count + 1);
    adapter.slots.push(Slot::new(id, Kind::Root));
    let mut index = 0;
    while index < adapter.sources.len() {
        let node = adapter.sources[index];
        if adapter.layout.document.nodes[node].tag == "#text" {
            let start = index;
            let mut visible = false;
            while index < adapter.sources.len()
                && adapter.layout.document.nodes[adapter.sources[index]].tag == "#text"
            {
                visible |= adapter.layout.document.nodes[adapter.sources[index]]
                    .text
                    .chars()
                    .any(|ch| !is_css_space(ch));
                index += 1;
            }
            // CSS suppresses an anonymous item containing only collapsible space.
            if visible {
                adapter
                    .slots
                    .push(Slot::new(id, Kind::Anonymous { start, end: index }));
            }
        } else {
            adapter.slots.push(Slot::new(node, Kind::Element));
            index += 1;
        }
    }
    adapter.slots[1..].sort_by_key(|slot| match slot.kind {
        Kind::Element => adapter.layout.styles[slot.node].layout.order,
        _ => 0,
    });
    for index in 1..adapter.slots.len() {
        let node = adapter.slots[index].node;
        if matches!(adapter.slots[index].kind, Kind::Element) {
            let style = &adapter.layout.styles[node];
            let image_ratio_needed = matches!(
                adapter.layout.document.nodes[node].tag.as_str(),
                "img" | "svg"
            ) && (style.width == Length::Auto
                || style.height == Length::Auto);
            if image_ratio_needed {
                let (width, height) = adapter.layout.measure_natural_replaced(node, depth + 1)?;
                adapter.slots[index].natural = Some(Size { width, height });
                if width > 0.0 && height > 0.0 {
                    adapter.slots[index].ratio = Some(width / height);
                }
            }
            if let Some(keyword) = adapter.layout.styles[node].layout.min_width_intrinsic {
                if keyword == IntrinsicSize::FitContent
                    && matches!(
                        adapter.layout.styles[id].display,
                        Display::Grid | Display::InlineGrid
                    )
                {
                    adapter.layout.fail(node, "fit-content min-width on a grid item requires its resolved grid-area width");
                    return None;
                }
                let intrinsic = if keyword == IntrinsicSize::FitContent {
                    adapter.layout.measure_fit_content_intrinsic(
                        node,
                        width.into_option().unwrap_or(0.0),
                        height,
                        depth + 1,
                    )?
                } else {
                    adapter.layout.measure_content_intrinsic(node, depth + 1)?
                };
                let mut value = match keyword {
                    IntrinsicSize::MinContent => intrinsic.min,
                    IntrinsicSize::MaxContent => intrinsic.max,
                    IntrinsicSize::FitContent => {
                        let (margin, padding) = adapter
                            .layout
                            .edges(node, width.into_option().unwrap_or(0.0));
                        match width {
                            AvailableSpace::Definite(w) => intrinsic.max.min(intrinsic.min.max(
                                (w - margin[1] - margin[3] - padding[1] - padding[3]).max(0.0),
                            )),
                            AvailableSpace::MinContent => intrinsic.min,
                            AvailableSpace::MaxContent => intrinsic.max,
                        }
                    }
                };
                if adapter.layout.styles[node].layout.box_sizing == BoxSizing::BorderBox {
                    let (_, padding) = adapter
                        .layout
                        .edges(node, width.into_option().unwrap_or(0.0));
                    value += padding[1] + padding[3];
                }
                if !dimension(value) {
                    adapter.layout.fail(
                        node,
                        "formatting intrinsic minimum is outside finite extent",
                    );
                    return None;
                }
                adapter.slots[index].intrinsic_min = Some(value);
            }
            if adapter.layout.styles[node].layout.max_width_fit_content {
                if matches!(
                    adapter.layout.styles[id].display,
                    Display::Grid | Display::InlineGrid
                ) {
                    adapter.layout.fail(node, "fit-content max-width on a grid item requires its resolved grid-area width");
                    return None;
                }
                let intrinsic = adapter.layout.measure_fit_content_intrinsic(
                    node,
                    width.into_option().unwrap_or(0.0),
                    height,
                    depth + 1,
                )?;
                let (margin, padding) = adapter
                    .layout
                    .edges(node, width.into_option().unwrap_or(0.0));
                let edges = padding[1] + padding[3];
                let content = match width {
                    AvailableSpace::Definite(w) => intrinsic.max.min(
                        intrinsic
                            .min
                            .max((w - margin[1] - margin[3] - edges).max(0.0)),
                    ),
                    AvailableSpace::MinContent => intrinsic.min,
                    AvailableSpace::MaxContent => intrinsic.max,
                };
                let value = content
                    + if adapter.layout.styles[node].layout.box_sizing == BoxSizing::BorderBox {
                        edges
                    } else {
                        0.0
                    };
                if !dimension(value) {
                    adapter.layout.fail(
                        node,
                        "formatting intrinsic maximum is outside finite extent",
                    );
                    return None;
                }
                adapter.slots[index].intrinsic_max = Some(value);
            }
        }
        if let Err(reason) = adapter.view(index).validate() {
            adapter.layout.fail(node, reason);
            return None;
        }
    }
    adapter.preflight()?;
    let input = LayoutInput {
        run_mode: if EMIT {
            RunMode::PerformLayout
        } else {
            RunMode::ComputeSize
        },
        sizing_mode: SizingMode::InherentSize,
        axis: RequestedAxis::Both,
        known_dimensions: Size {
            width: width.into_option(),
            height,
        },
        known_dimensions_are_definite: Size {
            width: true,
            height: true,
        },
        parent_size: Size {
            width: width.into_option(),
            height,
        },
        available_space: Size {
            width,
            height: height.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite),
        },
        vertical_margins_are_collapsible: Line::FALSE,
    };
    let output = adapter.compute_child_layout(NodeId::from(0usize), input);
    if adapter.failed() || !valid_size(output.size) {
        adapter
            .layout
            .fail(id, "formatting result is failed or outside finite extent");
        return None;
    }
    if EMIT {
        // Validate every final allocation before the first paint operation.
        for slot in &adapter.slots[1..] {
            let rect = slot.geometry;
            if !slot.written
                || !valid_size(rect.size)
                || !rect.location.x.is_finite()
                || !rect.location.y.is_finite()
                || (x + rect.location.x).abs() > MAX_EXTENT
                || (y + rect.location.y).abs() > MAX_EXTENT
                || [
                    rect.padding.top,
                    rect.padding.right,
                    rect.padding.bottom,
                    rect.padding.left,
                    rect.border.top,
                    rect.border.right,
                    rect.border.bottom,
                    rect.border.left,
                ]
                .iter()
                .any(|value| !dimension(*value))
            {
                adapter.layout.fail(
                    slot.node,
                    "formatting final item geometry is missing or outside finite extent",
                );
                return None;
            }
        }
        for index in 1..adapter.slots.len() {
            if !adapter.layout.spend(depth + 1) {
                return None;
            }
            let geometry = adapter.slots[index].geometry;
            let rect = Rect {
                x: x + geometry.location.x,
                y: y + geometry.location.y,
                w: geometry.size.width,
                h: geometry.size.height,
            };
            match adapter.slots[index].kind {
                Kind::Element => {
                    let p = geometry.padding;
                    let b = geometry.border;
                    adapter.layout.paint_allocated(
                        adapter.slots[index].node,
                        rect,
                        [
                            p.top + b.top,
                            p.right + b.right,
                            p.bottom + b.bottom,
                            p.left + b.left,
                        ],
                        adapter.slots[index]
                            .definite_height
                            .then_some((rect.h - p.top - b.top - p.bottom - b.bottom).max(0.0)),
                        depth + 1,
                    );
                }
                Kind::Anonymous { .. } => {
                    let tokens = adapter.anonymous_tokens(index)?;
                    adapter.layout.flow_tokens::<true>(
                        id,
                        rect.x,
                        rect.y,
                        rect.w,
                        adapter.slots[index].definite_height.then_some(rect.h),
                        tokens,
                        depth + 1,
                    );
                }
                Kind::Root => unreachable!(),
            }
            if adapter.failed() {
                return None;
            }
        }
    }
    Some(output.size)
}

impl Adapter<'_, '_, '_> {
    fn spend_callback(&mut self, id: NodeId) -> bool {
        let node = self.slots[usize::from(id)].node;
        if self.depth + 1 > MAX_DEPTH {
            self.layout
                .fail(node, "formatting callback depth bound exceeded");
            return false;
        }
        if self.layout.budget == 0 {
            self.layout
                .fail(node, "formatting callback work budget exhausted");
            return false;
        }
        self.layout.spend(self.depth + 1)
    }

    fn failed(&self) -> bool {
        self.layout.exhausted || self.invalid.get()
    }

    fn view(&self, index: usize) -> StyleView<'_> {
        let slot = &self.slots[index];
        let style = &self.layout.styles[slot.node];
        match slot.kind {
            Kind::Root => StyleView::root_content(style),
            Kind::Anonymous { .. } => StyleView::anonymous(style),
            Kind::Element => {
                let view = StyleView::item(style, slot.ratio);
                let view = slot
                    .intrinsic_min
                    .map_or(view, |value| view.with_intrinsic_min_width(value));
                slot.intrinsic_max
                    .map_or(view, |value| view.with_intrinsic_max_width(value))
            }
        }
    }

    fn anonymous_tokens(&mut self, index: usize) -> Option<Vec<Token>> {
        let Kind::Anonymous { start, end } = self.slots[index].kind else {
            return None;
        };
        let mut tokens = Vec::new();
        for source in start..end {
            self.layout
                .tokens(self.sources[source], &mut tokens, self.depth + 1);
        }
        (!self.failed()).then_some(tokens)
    }

    fn anonymous_intrinsic(&mut self, index: usize) -> Option<Intrinsic> {
        let tokens = self.anonymous_tokens(index)?;
        let mut min = 0.0f32;
        let mut word_run = 0.0f32;
        let mut max = 0.0f32;
        let mut space = 0.0;
        let mut any_word = false;
        for token in tokens {
            match token {
                Token::Word { width, .. } => {
                    word_run += width;
                    min = min.max(word_run);
                    max += space + width;
                    space = 0.0;
                    any_word = true;
                }
                Token::Space { width, .. } if any_word => {
                    space = width;
                    word_run = 0.0;
                }
                Token::Space { .. } => {}
                _ => return None,
            }
        }
        if matches!(
            self.layout.styles[self.slots[0].node].white_space,
            WhiteSpace::NoWrap | WhiteSpace::Pre
        ) {
            min = max;
        }
        (dimension(min) && dimension(max)).then_some(Intrinsic { min, max })
    }

    fn intrinsic(&mut self, index: usize) -> Option<Intrinsic> {
        match self.slots[index].kind {
            Kind::Anonymous { .. } => self.anonymous_intrinsic(index),
            Kind::Element => self
                .layout
                .measure_content_intrinsic(self.slots[index].node, self.depth + 1),
            Kind::Root => None,
        }
    }

    fn measure(
        &mut self,
        index: usize,
        input: LayoutInput,
        available: Size<AvailableSpace>,
    ) -> Option<Size<f32>> {
        if self.failed()
            || !self.layout.spend(self.depth + 1)
            || !space(available.width)
            || !space(available.height)
        {
            return None;
        }
        let node = self.slots[index].node;
        let anonymous = matches!(self.slots[index].kind, Kind::Anonymous { .. });
        let own_width = !anonymous
            && input.sizing_mode == SizingMode::InherentSize
            && self.layout.styles[node]
                .width
                .resolve(input.parent_size.width.unwrap_or(0.0))
                .is_some()
            && (!matches!(self.layout.styles[node].width, Length::Percent(_))
                || input.parent_size.width.is_some());
        let fixed_width = input.known_dimensions.width.is_some() || own_width;
        let own_height = !anonymous
            && input.sizing_mode == SizingMode::InherentSize
            && super::height_length(self.layout.styles[node].height, input.parent_size.height)
                .is_some();
        let definite_height = (own_height
            || (input.known_dimensions.height.is_some()
                && input.known_dimensions_are_definite.height))
            .then(|| available.height.into_option())
            .flatten();
        if !anonymous && self.layout.is_replaced(node) {
            let natural = if let Some(size) = self.slots[index].natural {
                size
            } else {
                let (width, height) = self.layout.measure_natural_replaced(node, self.depth + 1)?;
                let size = Size { width, height };
                self.slots[index].natural = Some(size);
                size
            };
            let image = matches!(self.layout.document.nodes[node].tag.as_str(), "img" | "svg");
            let width = if fixed_width {
                available
                    .width
                    .into_option()
                    .unwrap_or(natural.width)
                    .max(0.0)
            } else if image && definite_height.is_some() && natural.height > 0.0 {
                definite_height.unwrap() * natural.width / natural.height
            } else {
                natural.width
            };
            let height = definite_height.unwrap_or_else(|| {
                if image && natural.width > 0.0 {
                    width * natural.height / natural.width
                } else {
                    natural.height
                }
            });
            let size = Size { width, height };
            return valid_size(size).then_some(size);
        }
        let width = if fixed_width {
            available.width.into_option()?.max(0.0)
        } else {
            let intrinsic = self.intrinsic(index)?;
            match available.width {
                AvailableSpace::MinContent => intrinsic.min,
                AvailableSpace::MaxContent => intrinsic.max,
                AvailableSpace::Definite(width) => {
                    intrinsic.max.min(width.max(0.0)).max(intrinsic.min)
                }
            }
        };
        let height = if anonymous {
            let tokens = self.anonymous_tokens(index)?;
            self.layout.flow_tokens::<false>(
                self.slots[0].node,
                0.0,
                0.0,
                width,
                definite_height,
                tokens,
                self.depth + 1,
            )
        } else {
            self.layout
                .measure_content_height(node, width, definite_height, self.depth + 1)?
        };
        let size = Size { width, height };
        (!self.failed() && valid_size(size)).then_some(size)
    }

    fn preflight(&mut self) -> Option<()> {
        let count = self.slots.len() - 1;
        let root = &self.layout.styles[self.slots[0].node];
        let mut work = count.checked_mul(count)?.max(1);
        if matches!(root.display, Display::Grid | Display::InlineGrid) {
            let grid = root.layout.grid.as_deref();
            let columns = grid.map_or(0, |grid| grid.template_columns.len());
            let rows = grid.map_or(0, |grid| grid.template_rows.len());
            let column_flow = grid.is_some_and(|grid| {
                matches!(
                    grid.auto_flow,
                    GridAutoFlow::Column | GridAutoFlow::ColumnDense
                )
            });
            let mut col = AxisBound::new(columns);
            let mut row = AxisBound::new(rows);
            for slot in &self.slots[1..] {
                let (columns, rows) = if matches!(slot.kind, Kind::Element) {
                    let style = &self.layout.styles[slot.node].layout;
                    (style.grid_column, style.grid_row)
                } else {
                    ([GridLine::Auto; 2], [GridLine::Auto; 2])
                };
                if col.item(columns).is_none() || row.item(rows).is_none() {
                    self.layout.fail(
                        slot.node,
                        "formatting grid line or span is outside admission bounds",
                    );
                    return None;
                }
            }
            let Some(columns) = col.tracks(column_flow) else {
                self.layout.fail(
                    self.slots[0].node,
                    "formatting grid implicit column bound exceeded",
                );
                return None;
            };
            let Some(rows) = row.tracks(!column_flow) else {
                self.layout.fail(
                    self.slots[0].node,
                    "formatting grid implicit row bound exceeded",
                );
                return None;
            };
            let cells = columns.checked_mul(rows)?;
            work = work
                .checked_mul(columns.checked_add(rows)?)?
                .checked_add(cells.checked_mul(count.max(1))?)?;
            if work > MAX_GRID_WORK {
                self.layout.fail(
                    self.slots[0].node,
                    "formatting grid placement work bound exceeded",
                );
                return None;
            }
            // Occupancy plus conservative track/intermediate allocation space.
            let additional = cells
                .checked_mul(16)?
                .checked_add((columns + rows).checked_mul(512)?)?;
            let total = self.layout.format_scratch.checked_add(additional)?;
            if total > MAX_SCRATCH {
                self.layout
                    .fail(self.slots[0].node, "formatting grid scratch bound exceeded");
                return None;
            }
            self.layout.format_scratch = total;
            self.charge += additional;
        }
        if work > self.layout.budget {
            self.layout.fail(
                self.slots[0].node,
                "formatting placement preflight exceeds remaining work budget",
            );
            return None;
        }
        self.layout.budget -= work;
        Some(())
    }
}

#[derive(Clone, Copy)]
struct AxisBound {
    explicit: usize,
    max_span: usize,
    sum_span: usize,
    before: usize,
    after: usize,
    positioned: bool,
}
impl AxisBound {
    fn new(explicit: usize) -> Self {
        Self {
            explicit,
            max_span: 1,
            sum_span: 0,
            before: 0,
            after: 0,
            positioned: false,
        }
    }
    fn item(&mut self, lines: [GridLine; 2]) -> Option<()> {
        let mut span = 1usize;
        for line in lines {
            match line {
                GridLine::Span(value) if value > 0 && usize::from(value) <= MAX_TRACKS => {
                    span = span.max(usize::from(value))
                }
                GridLine::Line(value)
                    if value != 0 && usize::from(value.unsigned_abs()) <= MAX_TRACKS =>
                {
                    self.positioned = true;
                    let extra = usize::from(value.unsigned_abs()).saturating_sub(self.explicit + 1);
                    if value < 0 {
                        self.before = self.before.max(extra);
                    } else {
                        self.after = self.after.max(extra);
                    }
                }
                GridLine::Auto => {}
                _ => return None,
            }
        }
        if let [GridLine::Line(start), GridLine::Line(end)] = lines {
            if start.signum() == end.signum() {
                span = span.max(usize::from(start.abs_diff(end)));
            } else {
                span = span.max(self.explicit + self.before + self.after);
            }
        }
        self.max_span = self.max_span.max(span);
        self.sum_span = self.sum_span.checked_add(span)?;
        Some(())
    }
    fn tracks(self, auto_flow_axis: bool) -> Option<usize> {
        let base = self.explicit.max(self.max_span).max(1);
        let extra = if self.positioned {
            self.max_span.checked_mul(2)?
        } else {
            0
        };
        let result = base
            .checked_add(self.before)?
            .checked_add(self.after)?
            .checked_add(extra)?
            .checked_add(if auto_flow_axis { self.sum_span } else { 0 })?;
        (result <= MAX_TRACKS).then_some(result)
    }
}

struct Children(Range<usize>);
impl Iterator for Children {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(NodeId::from)
    }
}
impl TraversePartialTree for Adapter<'_, '_, '_> {
    type ChildIter<'a>
        = Children
    where
        Self: 'a;
    fn child_ids(&self, id: NodeId) -> Children {
        Children(if usize::from(id) == 0 {
            1..self.slots.len()
        } else {
            0..0
        })
    }
    fn child_count(&self, id: NodeId) -> usize {
        if usize::from(id) == 0 {
            self.slots.len() - 1
        } else {
            0
        }
    }
    fn get_child_id(&self, id: NodeId, index: usize) -> NodeId {
        debug_assert_eq!(usize::from(id), 0);
        NodeId::from(index + 1)
    }
}
impl LayoutPartialTree for Adapter<'_, '_, '_> {
    type CustomIdent = String;
    type CoreContainerStyle<'a>
        = StyleView<'a>
    where
        Self: 'a;
    fn get_core_container_style(&self, id: NodeId) -> StyleView<'_> {
        self.view(usize::from(id))
    }
    fn resolve_calc_value(&self, _: *const (), _: f32) -> f32 {
        self.invalid.set(true);
        f32::NAN
    }
    fn set_unrounded_layout(&mut self, id: NodeId, geometry: &Geometry) {
        if self.failed() || !self.spend_callback(id) {
            return;
        }
        let slot = &mut self.slots[usize::from(id)];
        slot.geometry = *geometry;
        slot.written = true;
    }
    fn compute_child_layout(&mut self, id: NodeId, input: LayoutInput) -> LayoutOutput {
        // This runs even when compute_cached_layout returns a hit.
        if self.failed() || !self.spend_callback(id) {
            return LayoutOutput::HIDDEN;
        }
        if input.run_mode == RunMode::PerformLayout && usize::from(id) != 0 {
            let slot = &mut self.slots[usize::from(id)];
            // Taffy flexbox computes this flag from stretch, definite basis and
            // container sizing; a content-derived final size is not enough.
            // Grid passes known dimensions only after its sizing/alignment.
            let own = matches!(slot.kind, Kind::Element)
                && super::height_length(
                    self.layout.styles[slot.node].height,
                    input.parent_size.height,
                )
                .is_some();
            slot.definite_height = own
                || (input.known_dimensions.height.is_some()
                    && input.known_dimensions_are_definite.height);
        }
        compute_cached_layout(self, id, input, |tree, id, input| {
            if tree.failed() {
                return LayoutOutput::HIDDEN;
            }
            let index = usize::from(id);
            let output = if index == 0 {
                match tree.layout.styles[tree.slots[0].node].display {
                    Display::Flex | Display::InlineFlex => compute_flexbox_layout(tree, id, input),
                    Display::Grid | Display::InlineGrid => compute_grid_layout(tree, id, input),
                    _ => {
                        tree.invalid.set(true);
                        LayoutOutput::HIDDEN
                    }
                }
            } else {
                let style = LeafStyle::from(tree.view(index));
                let invalid_calc = Cell::new(false);
                let output = compute_leaf_layout(
                    input,
                    &style,
                    |_, _| {
                        invalid_calc.set(true);
                        f32::NAN
                    },
                    |_, available| match tree.measure(index, input, available) {
                        Some(size) => size,
                        None => {
                            tree.layout.fail(
                                tree.slots[index].node,
                                "formatting leaf measurement rejected",
                            );
                            Size::ZERO
                        }
                    },
                );
                if invalid_calc.get() {
                    tree.invalid.set(true);
                }
                output
            };
            if !valid_size(output.size) {
                tree.layout.fail(
                    tree.slots[index].node,
                    "formatting computed size is outside finite extent",
                );
                tree.invalid.set(true);
            }
            output
        })
    }
}
impl CacheTree for Adapter<'_, '_, '_> {
    fn cache_get(&mut self, id: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        if self.failed() || !self.spend_callback(id) {
            return None;
        }
        self.slots[usize::from(id)].cache.get(input)
    }
    fn cache_store(&mut self, id: NodeId, input: &LayoutInput, output: LayoutOutput) {
        if self.failed() || !self.spend_callback(id) {
            return;
        }
        self.slots[usize::from(id)].cache.store(input, output);
    }
    fn cache_clear(&mut self, id: NodeId) {
        if self.failed() || !self.spend_callback(id) {
            return;
        }
        self.slots[usize::from(id)].cache.clear();
    }
}
impl LayoutFlexboxContainer for Adapter<'_, '_, '_> {
    type FlexboxContainerStyle<'a>
        = StyleView<'a>
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = StyleView<'a>
    where
        Self: 'a;
    fn get_flexbox_container_style(&self, id: NodeId) -> StyleView<'_> {
        self.view(usize::from(id))
    }
    fn get_flexbox_child_style(&self, id: NodeId) -> StyleView<'_> {
        self.view(usize::from(id))
    }
}
impl LayoutGridContainer for Adapter<'_, '_, '_> {
    type GridContainerStyle<'a>
        = StyleView<'a>
    where
        Self: 'a;
    type GridItemStyle<'a>
        = StyleView<'a>
    where
        Self: 'a;
    fn get_grid_container_style(&self, id: NodeId) -> StyleView<'_> {
        self.view(usize::from(id))
    }
    fn get_grid_child_style(&self, id: NodeId) -> StyleView<'_> {
        self.view(usize::from(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_layout(html: &str, check: impl FnOnce(&mut Layout<'_, '_>, usize)) {
        let document = crate::document::parse(html, "https://fixture.example/");
        let root = document.query_selector(0, "#root").unwrap().unwrap();
        let styles =
            crate::style::compute_styles(&document, &document.stylesheets, (800.0, 600.0)).unwrap();
        let mut fonts = crate::paint::Fonts::from_bytes(
            std::fs::read(
                std::env::var("MGBROWSER_FONT")
                    .unwrap_or_else(|_| "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into()),
            )
            .unwrap(),
        )
        .unwrap();
        let controls = crate::render::Controls::default();
        let mut layout = Layout {
            document: &document,
            fonts: &mut fonts,
            controls: &controls,
            viewport_overflow: super::super::viewport_overflow_node(&document, &styles),
            styles,
            viewport: crate::render::Viewport {
                width: 800,
                height: 600,
                scroll: 0,
            },
            scene: super::super::Scene::default(),
            intrinsic: Default::default(),
            content_intrinsic_cache: Default::default(),
            content_height_cache: Default::default(),
            natural_images: Default::default(),
            natural_svg: Default::default(),
            inline_svg: Default::default(),
            inline_svg_bytes: 0,
            inline_svg_limit_reported: false,
            diagnostics: Default::default(),
            table_cache: Default::default(),
            format_scratch: 0,
            replaced_nodes: Vec::new(),
            anonymous_tables: Vec::new(),
            budget: 2_000_000,
            exhausted: false,
            failure: None,
        };
        layout.prepare_replaced_nodes();
        super::super::anonymous_table::prepare(&mut layout);
        check(&mut layout, root);
    }

    #[test]
    fn scratch_is_released_after_success_and_partial_admission_failure() {
        with_layout(
            "<div id=root style='display:flex'><div style='width:20px;height:10px'></div><div style='width:20px;height:10px'></div></div>",
            |layout, id| {
                layout.format_scratch = 17;
                assert_eq!(
                    run::<false>(
                        layout,
                        id,
                        0.0,
                        0.0,
                        AvailableSpace::Definite(100.0),
                        None,
                        0
                    ),
                    Some(Size {
                        width: 100.0,
                        height: 10.0
                    })
                );
                assert_eq!(layout.format_scratch, 17);
                assert!(
                    layout.scene.paints.is_empty()
                        && layout.scene.boxes.is_empty()
                        && layout.scene.hits.is_empty()
                );
                // Admission spends five visits, then cannot prepay the four-unit
                // placement allowance. Its already-reserved scratch must unwind.
                layout.budget = 8;
                assert!(
                    run::<false>(
                        layout,
                        id,
                        0.0,
                        0.0,
                        AvailableSpace::Definite(100.0),
                        None,
                        0
                    )
                    .is_none()
                );
                assert!(layout.exhausted);
                assert_eq!(
                    layout.failure,
                    Some((
                        id,
                        "formatting placement preflight exceeds remaining work budget"
                    ))
                );
                assert_eq!(layout.format_scratch, 17);
                assert!(layout.budget < 8);
                assert_eq!(layout.scene.paints.capacity(), 0);
                assert_eq!(layout.scene.boxes.capacity(), 0);
                assert_eq!(layout.scene.hits.capacity(), 0);
            },
        );
    }

    #[test]
    fn participant_and_global_scratch_limits_reject_before_emission() {
        with_layout(
            &format!(
                "<div id=root style='display:flex'>{}</div>",
                "<div></div>".repeat(MAX_ITEMS + 1)
            ),
            |layout, id| {
                assert!(
                    run::<true>(
                        layout,
                        id,
                        0.0,
                        0.0,
                        AvailableSpace::Definite(100.0),
                        None,
                        0
                    )
                    .is_none()
                );
                assert!(layout.exhausted);
                assert_eq!(
                    layout.failure,
                    Some((id, "formatting participant count bound exceeded"))
                );
                assert_eq!(layout.format_scratch, 0);
                assert_eq!(layout.scene.paints.capacity(), 0);
            },
        );
        with_layout(
            "<div id=root style='display:flex'><div></div></div>",
            |layout, id| {
                layout.format_scratch = MAX_SCRATCH - 1;
                assert!(
                    run::<false>(
                        layout,
                        id,
                        0.0,
                        0.0,
                        AvailableSpace::Definite(100.0),
                        None,
                        0
                    )
                    .is_none()
                );
                assert_eq!(layout.format_scratch, MAX_SCRATCH - 1);
                assert!(layout.exhausted);
                assert_eq!(
                    layout.failure,
                    Some((id, "formatting live scratch bound exceeded"))
                );
                assert_eq!(layout.scene.paints.capacity(), 0);
            },
        );
    }

    #[test]
    fn callback_exhaustion_keeps_first_reason_and_releases_scratch() {
        with_layout(
            "<div id=root style='display:flex'><div style='width:20px;height:10px'></div><div style='width:20px;height:10px'></div></div>",
            |layout, id| {
                // Five admission visits and four prepaid placement units leave
                // no work for the first actual Taffy callback.
                layout.budget = 9;
                assert!(
                    run::<true>(
                        layout,
                        id,
                        0.0,
                        0.0,
                        AvailableSpace::Definite(100.0),
                        None,
                        0
                    )
                    .is_none()
                );
                assert_eq!(
                    layout.failure,
                    Some((id, "formatting callback work budget exhausted"))
                );
                assert!(layout.exhausted);
                assert_eq!(layout.budget, 0);
                assert_eq!(layout.format_scratch, 0);
                assert_eq!(layout.scene.paints.capacity(), 0);
                assert_eq!(layout.scene.boxes.capacity(), 0);
                assert_eq!(layout.scene.hits.capacity(), 0);
            },
        );
    }

    #[test]
    fn anonymous_text_scratch_is_admitted_before_tokenization() {
        let text = "x".repeat(super::super::MAX_TEXT_RUN);
        let body = format!("<span style='display:contents'>{text}</span>").repeat(5);
        with_layout(
            &format!("<div id=root style='display:flex'>{body}</div>"),
            |layout, id| {
                assert!(
                    run::<false>(
                        layout,
                        id,
                        0.0,
                        0.0,
                        AvailableSpace::Definite(100.0),
                        None,
                        0
                    )
                    .is_none()
                );
                assert!(layout.exhausted);
                assert_eq!(layout.format_scratch, 0);
                assert!(layout.intrinsic.is_empty());
                assert_eq!(layout.scene.paints.capacity(), 0);
            },
        );
    }

    #[test]
    fn text_under_contents_is_not_mistaken_for_an_empty_contents_element() {
        with_layout(
            "<div id=root style='display:flex;gap:10px;font-size:12px;line-height:20px'><span style='display:contents'>inside</span><div id=box style='width:20px;height:20px'></div></div>",
            |layout, id| {
                let text = layout
                    .document
                    .nodes
                    .iter()
                    .position(|node| node.tag == "#text" && node.text == "inside")
                    .unwrap();
                let box_id = layout.document.query_selector(0, "#box").unwrap().unwrap();
                assert!(
                    run::<true>(
                        layout,
                        id,
                        0.0,
                        0.0,
                        AvailableSpace::Definite(200.0),
                        None,
                        0
                    )
                    .is_some()
                );
                assert!(
                    layout
                        .scene
                        .boxes
                        .iter()
                        .any(|rect| rect.node == text && rect.width > 0)
                );
                assert!(
                    layout
                        .scene
                        .boxes
                        .iter()
                        .any(|rect| rect.node == box_id && rect.x > 10)
                );
                assert_eq!(layout.format_scratch, 0);
            },
        );
    }

    #[test]
    fn implicit_track_preflight_counts_spans_and_rejects_unbounded_growth() {
        let mut columns = AxisBound::new(12);
        columns.item([GridLine::Span(8), GridLine::Auto]).unwrap();
        columns.item([GridLine::Span(4), GridLine::Auto]).unwrap();
        assert_eq!(columns.tracks(false), Some(12));
        let mut rows = AxisBound::new(0);
        for _ in 0..128 {
            rows.item([GridLine::Auto; 2]).unwrap();
        }
        assert_eq!(rows.tracks(true), None);
        assert!(
            AxisBound::new(0)
                .item([GridLine::Line(0), GridLine::Auto])
                .is_none()
        );
        let mut displaced = AxisBound::new(12);
        displaced
            .item([GridLine::Line(-128), GridLine::Span(128)])
            .unwrap();
        assert_eq!(displaced.tracks(false), None);
    }
}
