//! Allocation-free adapters from Mg's computed snapshot to Taffy's style traits.
//!
//! The owner validates each view before invoking layout. Taffy handles geometry,
//! not painting, clipping, scrolling or stacking: those remain Mg responsibilities.

use crate::style as mg;
use taffy::geometry::{Line, MinMax, Point, Rect, Size};
use taffy::style::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoxMode {
    /// Mg has already laid out the root's outer box. Keep its formatting rules.
    RootContent,
    Item,
    /// Anonymous text item: inherit typography in measurement, not box styles.
    Anonymous,
}

#[derive(Clone, Copy)]
pub(super) struct StyleView<'a> {
    pub(super) style: &'a mg::ComputedStyle,
    pub(super) mode: BoxMode,
    pub(super) natural_ratio: Option<f32>,
    intrinsic_min_width: Option<f32>,
    intrinsic_max_width: Option<f32>,
}

impl<'a> StyleView<'a> {
    pub(super) fn root_content(style: &'a mg::ComputedStyle) -> Self {
        Self {
            style,
            mode: BoxMode::RootContent,
            natural_ratio: None,
            intrinsic_min_width: None,
            intrinsic_max_width: None,
        }
    }
    pub(super) fn item(style: &'a mg::ComputedStyle, natural_ratio: Option<f32>) -> Self {
        Self {
            style,
            mode: BoxMode::Item,
            natural_ratio,
            intrinsic_min_width: None,
            intrinsic_max_width: None,
        }
    }
    pub(super) fn anonymous(style: &'a mg::ComputedStyle) -> Self {
        Self {
            style,
            mode: BoxMode::Anonymous,
            natural_ratio: None,
            intrinsic_min_width: None,
            intrinsic_max_width: None,
        }
    }
    /// Measured intrinsic content width expressed in this item's sizing box:
    /// add padding/borders for BorderBox, but not for ContentBox. RootContent
    /// deliberately strips the constraint: Mg's outer wrapper must enforce it.
    pub(super) fn with_intrinsic_min_width(mut self, width: f32) -> Self {
        self.intrinsic_min_width = Some(width);
        self
    }
    pub(super) fn with_intrinsic_max_width(mut self, width: f32) -> Self {
        self.intrinsic_max_width = Some(width);
        self
    }
    fn item_box(self) -> bool {
        self.mode == BoxMode::Item
    }
    fn formatting(self) -> bool {
        self.mode != BoxMode::Anonymous
    }

    /// Refuse lost or unimplemented semantics before giving Taffy any values.
    /// Outer-root positioning is deliberately not handled by this content view.
    pub(super) fn validate(self) -> Result<(), &'static str> {
        if !self.formatting() {
            return Ok(());
        }
        let l = &self.style.layout;
        if l.unsupported {
            return Err("computed layout contains an unrepresented CSS value");
        }
        if self.item_box() && matches!(l.position, mg::Position::Fixed | mg::Position::Sticky) {
            return Err("fixed/sticky items require a viewport/scroll containing block");
        }
        if self.item_box()
            && l.max_width_fit_content
            && !self
                .intrinsic_max_width
                .is_some_and(|v| v.is_finite() && v >= 0.)
        {
            return Err("fit-content max-width requires a finite content measurement");
        }
        if self.item_box()
            && l.min_width_intrinsic.is_some()
            && !self
                .intrinsic_min_width
                .is_some_and(|v| v.is_finite() && v >= 0.)
        {
            return Err("intrinsic min-width requires a finite content measurement");
        }
        if let mg::FlexBasis::Length(length) = l.flex_basis {
            if !finite_length(length) {
                return Err("non-finite flex basis");
            }
        }
        if !l.flex_grow.is_finite()
            || l.flex_grow < 0.
            || !l.flex_shrink.is_finite()
            || l.flex_shrink < 0.
        {
            return Err("non-finite or negative flex factor");
        }
        for value in [l.align_content, l.justify_content] {
            content_alignment(value)?;
        }
        for value in [l.align_items, l.justify_items, l.align_self, l.justify_self] {
            item_alignment(value)?;
        }
        for length in l
            .inset
            .iter()
            .chain(l.gap.iter())
            .chain(self.style.margin.iter())
            .chain(self.style.padding.iter())
            .chain([
                &self.style.width,
                &self.style.height,
                &self.style.min_width,
                &self.style.min_height,
                &self.style.max_width,
                &self.style.max_height,
            ])
        {
            if !finite_length(*length) {
                return Err("non-finite layout length");
            }
        }
        if l.gap
            .iter()
            .chain(self.style.padding.iter())
            .any(|v| matches!(v, mg::Length::Auto))
        {
            return Err("auto padding or gap is not represented by this adapter");
        }
        if self
            .style
            .border_width
            .iter()
            .any(|v| !v.is_finite() || *v < 0.)
        {
            return Err("non-finite or negative border width");
        }
        if self
            .natural_ratio
            .is_some_and(|v| !v.is_finite() || v <= 0.)
        {
            return Err("invalid intrinsic image aspect ratio");
        }
        for value in l.grid_row.iter().chain(l.grid_column.iter()) {
            match value {
                mg::GridLine::Line(n)
                    if *n == 0 || n.unsigned_abs() as usize > mg::MAX_GRID_TRACKS =>
                {
                    return Err("grid line exceeds the bounded numeric range");
                }
                mg::GridLine::Span(n) if *n == 0 || *n as usize > mg::MAX_GRID_TRACKS => {
                    return Err("grid span exceeds the bounded numeric range");
                }
                _ => {}
            }
        }
        if let Some(grid) = &l.grid {
            for tracks in [
                &grid.template_rows,
                &grid.template_columns,
                &grid.auto_rows,
                &grid.auto_columns,
            ] {
                if tracks.len() > mg::MAX_GRID_TRACKS {
                    return Err("grid track list exceeds the bounded numeric range");
                }
                for track in tracks {
                    let (min, max) = match *track {
                        mg::TrackSize::Breadth(mg::TrackBreadth::Fr(v)) => {
                            (mg::TrackBreadth::Auto, mg::TrackBreadth::Fr(v))
                        }
                        mg::TrackSize::Breadth(v) => (v, v),
                        mg::TrackSize::MinMax(min, max) => (min, max),
                    };
                    if matches!(min, mg::TrackBreadth::Fr(_)) {
                        return Err("flexible grid minimum is invalid");
                    }
                    for breadth in [min, max] {
                        match breadth {
                            mg::TrackBreadth::Fr(v) if !v.is_finite() || v < 0. => {
                                return Err("invalid grid fraction");
                            }
                            mg::TrackBreadth::Length(v)
                                if !finite_length(v) || matches!(v, mg::Length::Auto) =>
                            {
                                return Err("invalid grid track length");
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn grid(self) -> Option<&'a mg::GridStyle> {
        self.formatting()
            .then_some(self.style.layout.grid.as_deref())
            .flatten()
    }
    fn gap_value(self) -> Size<LengthPercentage> {
        if self.formatting() {
            Size {
                width: lp(self.style.layout.gap[1]),
                height: lp(self.style.layout.gap[0]),
            }
        } else {
            Size {
                width: LengthPercentage::length(0.),
                height: LengthPercentage::length(0.),
            }
        }
    }
}

fn finite_length(value: mg::Length) -> bool {
    match value {
        mg::Length::Auto => true,
        mg::Length::Px(v) | mg::Length::Percent(v) => v.is_finite(),
    }
}
fn lp(value: mg::Length) -> LengthPercentage {
    match value {
        mg::Length::Px(v) => LengthPercentage::length(v),
        mg::Length::Percent(v) => LengthPercentage::percent(v),
        // Invalid for the properties calling this helper; validate rejects it.
        mg::Length::Auto => LengthPercentage::length(0.),
    }
}
fn lpa(value: mg::Length) -> LengthPercentageAuto {
    match value {
        mg::Length::Px(v) => LengthPercentageAuto::length(v),
        mg::Length::Percent(v) => LengthPercentageAuto::percent(v),
        mg::Length::Auto => LengthPercentageAuto::auto(),
    }
}
fn dimension(value: mg::Length) -> Dimension {
    match value {
        mg::Length::Px(v) => Dimension::length(v),
        mg::Length::Percent(v) => Dimension::percent(v),
        mg::Length::Auto => Dimension::auto(),
    }
}
fn rect<T: Copy>(values: [T; 4]) -> Rect<T> {
    Rect {
        top: values[0],
        right: values[1],
        bottom: values[2],
        left: values[3],
    }
}
fn overflow(value: mg::Overflow) -> Overflow {
    match value {
        mg::Overflow::Visible => Overflow::Visible,
        mg::Overflow::Hidden => Overflow::Hidden,
        mg::Overflow::Clip => Overflow::Clip,
        // Preserve scroll-container sizing semantics. Mg paints/clips only the
        // initial offset and retains the explicit unsupported-scrolling warning;
        // this mapping adds no element-scroll input, state or scrollbars.
        mg::Overflow::Auto | mg::Overflow::Scroll => Overflow::Scroll,
    }
}
fn safety(value: mg::AlignSafety) -> AlignmentSafety {
    if value == mg::AlignSafety::Safe {
        AlignmentSafety::Safe
    } else {
        AlignmentSafety::Unsafe
    }
}
fn item_alignment(value: mg::Alignment) -> Result<Option<AlignItems>, &'static str> {
    use mg::AlignKeyword as K;
    if value.legacy {
        return Err("legacy alignment is not implemented");
    }
    let keyword = match value.keyword {
        K::Auto | K::Normal => return Ok(None),
        K::Start => AlignItemsKeyword::Start,
        K::End => AlignItemsKeyword::End,
        K::FlexStart => AlignItemsKeyword::FlexStart,
        K::FlexEnd => AlignItemsKeyword::FlexEnd,
        K::SelfStart => AlignItemsKeyword::SelfStart,
        K::SelfEnd => AlignItemsKeyword::SelfEnd,
        K::Center => AlignItemsKeyword::Center,
        K::Baseline => AlignItemsKeyword::Baseline,
        K::Stretch => AlignItemsKeyword::Stretch,
        _ => return Err("item alignment keyword is not implemented"),
    };
    Ok(Some(AlignItems {
        keyword,
        safety: safety(value.safety),
    }))
}
fn content_alignment(value: mg::Alignment) -> Result<Option<AlignContent>, &'static str> {
    use mg::AlignKeyword as K;
    if value.legacy {
        return Err("legacy alignment is not implemented");
    }
    let keyword = match value.keyword {
        K::Auto | K::Normal => return Ok(None),
        K::Start => AlignContentKeyword::Start,
        K::End => AlignContentKeyword::End,
        K::FlexStart => AlignContentKeyword::FlexStart,
        K::FlexEnd => AlignContentKeyword::FlexEnd,
        K::Center => AlignContentKeyword::Center,
        K::Stretch => AlignContentKeyword::Stretch,
        K::SpaceBetween => AlignContentKeyword::SpaceBetween,
        K::SpaceAround => AlignContentKeyword::SpaceAround,
        K::SpaceEvenly => AlignContentKeyword::SpaceEvenly,
        _ => return Err("content alignment keyword is not implemented"),
    };
    Ok(Some(AlignContent {
        keyword,
        safety: safety(value.safety),
    }))
}

impl CoreStyle for StyleView<'_> {
    type CustomIdent = String;
    fn box_generation_mode(&self) -> BoxGenerationMode {
        if self.formatting() && self.style.display == mg::Display::None {
            BoxGenerationMode::None
        } else {
            BoxGenerationMode::Normal
        }
    }
    fn is_block(&self) -> bool {
        self.item_box() && self.style.display == mg::Display::Block
    }
    fn is_compressible_replaced(&self) -> bool {
        self.item_box() && self.natural_ratio.is_some()
    }
    fn box_sizing(&self) -> BoxSizing {
        if self.item_box() && self.style.layout.box_sizing == mg::BoxSizing::BorderBox {
            BoxSizing::BorderBox
        } else {
            BoxSizing::ContentBox
        }
    }
    fn overflow(&self) -> Point<Overflow> {
        if self.formatting() {
            Point {
                x: overflow(self.style.layout.overflow[0]),
                y: overflow(self.style.layout.overflow[1]),
            }
        } else {
            Point {
                x: Overflow::Visible,
                y: Overflow::Visible,
            }
        }
    }
    fn position(&self) -> Position {
        if self.item_box() && self.style.layout.position == mg::Position::Absolute {
            Position::Absolute
        } else {
            Position::Relative
        }
    }
    fn inset(&self) -> Rect<LengthPercentageAuto> {
        if self.item_box() && self.style.layout.position != mg::Position::Static {
            rect(self.style.layout.inset.map(lpa))
        } else {
            rect([LengthPercentageAuto::auto(); 4])
        }
    }
    fn size(&self) -> Size<Dimension> {
        if self.item_box() {
            Size {
                width: if self.style.layout.width_fit_content {
                    Dimension::fit_content()
                } else if self.style.layout.width_max_content {
                    Dimension::max_content()
                } else {
                    dimension(self.style.width)
                },
                height: dimension(self.style.height),
            }
        } else {
            Size {
                width: Dimension::auto(),
                height: Dimension::auto(),
            }
        }
    }
    fn min_size(&self) -> Size<LengthPercentageAuto> {
        if self.item_box() {
            Size {
                width: if self.style.layout.min_width_intrinsic.is_some() {
                    self.intrinsic_min_width
                        .map(LengthPercentageAuto::length)
                        .unwrap_or_else(LengthPercentageAuto::auto)
                } else {
                    lpa(self.style.min_width)
                },
                height: lpa(self.style.min_height),
            }
        } else {
            Size {
                width: LengthPercentageAuto::auto(),
                height: LengthPercentageAuto::auto(),
            }
        }
    }
    fn max_size(&self) -> Size<LengthPercentageAuto> {
        if self.item_box() {
            Size {
                width: if self.style.layout.max_width_fit_content {
                    self.intrinsic_max_width
                        .map(LengthPercentageAuto::length)
                        .unwrap_or_else(LengthPercentageAuto::auto)
                } else {
                    lpa(self.style.max_width)
                },
                height: lpa(self.style.max_height),
            }
        } else {
            Size {
                width: LengthPercentageAuto::auto(),
                height: LengthPercentageAuto::auto(),
            }
        }
    }
    fn aspect_ratio(&self) -> Option<f32> {
        if self.item_box() && !self.style.layout.width_max_content {
            self.natural_ratio
        } else {
            // A max-content width must be measured, not synthesized from an
            // opposite-axis border-box size before Taffy resolves the keyword.
            // Mg's replaced measurement transfers the natural content ratio
            // after subtracting padding/borders and preserves forced used sizes.
            None
        }
    }
    fn margin(&self) -> Rect<LengthPercentageAuto> {
        if self.item_box() {
            rect(self.style.margin.map(lpa))
        } else {
            rect([LengthPercentageAuto::length(0.); 4])
        }
    }
    fn padding(&self) -> Rect<LengthPercentage> {
        if self.item_box() {
            rect(self.style.padding.map(lp))
        } else {
            rect([LengthPercentage::length(0.); 4])
        }
    }
    fn border(&self) -> Rect<LengthPercentage> {
        if self.item_box() {
            rect(self.style.border_width.map(LengthPercentage::length))
        } else {
            rect([LengthPercentage::length(0.); 4])
        }
    }
}

/// Scalar leaf geometry, copied onto the stack to release the style borrow
/// before a measurement callback mutably borrows Mg's layout/paint state.
#[derive(Clone, Copy)]
pub(super) struct LeafStyle {
    generation: BoxGenerationMode,
    block: bool,
    replaced: bool,
    sizing: BoxSizing,
    overflow: Point<Overflow>,
    position: Position,
    inset: Rect<LengthPercentageAuto>,
    size: Size<Dimension>,
    min: Size<LengthPercentageAuto>,
    max: Size<LengthPercentageAuto>,
    ratio: Option<f32>,
    margin: Rect<LengthPercentageAuto>,
    padding: Rect<LengthPercentage>,
    border: Rect<LengthPercentage>,
}
impl From<StyleView<'_>> for LeafStyle {
    fn from(view: StyleView<'_>) -> Self {
        Self {
            generation: view.box_generation_mode(),
            block: view.is_block(),
            replaced: view.is_compressible_replaced(),
            sizing: view.box_sizing(),
            overflow: view.overflow(),
            position: view.position(),
            inset: view.inset(),
            size: view.size(),
            min: view.min_size(),
            max: view.max_size(),
            ratio: view.aspect_ratio(),
            margin: view.margin(),
            padding: view.padding(),
            border: view.border(),
        }
    }
}
impl CoreStyle for LeafStyle {
    type CustomIdent = String;
    fn box_generation_mode(&self) -> BoxGenerationMode {
        self.generation
    }
    fn is_block(&self) -> bool {
        self.block
    }
    fn is_compressible_replaced(&self) -> bool {
        self.replaced
    }
    fn box_sizing(&self) -> BoxSizing {
        self.sizing
    }
    fn overflow(&self) -> Point<Overflow> {
        self.overflow
    }
    fn position(&self) -> Position {
        self.position
    }
    fn inset(&self) -> Rect<LengthPercentageAuto> {
        self.inset
    }
    fn size(&self) -> Size<Dimension> {
        self.size
    }
    fn min_size(&self) -> Size<LengthPercentageAuto> {
        self.min
    }
    fn max_size(&self) -> Size<LengthPercentageAuto> {
        self.max
    }
    fn aspect_ratio(&self) -> Option<f32> {
        self.ratio
    }
    fn margin(&self) -> Rect<LengthPercentageAuto> {
        self.margin
    }
    fn padding(&self) -> Rect<LengthPercentage> {
        self.padding
    }
    fn border(&self) -> Rect<LengthPercentage> {
        self.border
    }
}

impl FlexboxContainerStyle for StyleView<'_> {
    fn flex_direction(&self) -> FlexDirection {
        if !self.formatting() {
            return FlexDirection::Row;
        }
        match self.style.layout.flex_direction {
            mg::FlexDirection::Row => FlexDirection::Row,
            mg::FlexDirection::RowReverse => FlexDirection::RowReverse,
            mg::FlexDirection::Column => FlexDirection::Column,
            mg::FlexDirection::ColumnReverse => FlexDirection::ColumnReverse,
        }
    }
    fn flex_wrap(&self) -> FlexWrap {
        if !self.formatting() {
            return FlexWrap::NoWrap;
        }
        match self.style.layout.flex_wrap {
            mg::FlexWrap::NoWrap => FlexWrap::NoWrap,
            mg::FlexWrap::Wrap => FlexWrap::Wrap,
            mg::FlexWrap::WrapReverse => FlexWrap::WrapReverse,
        }
    }
    fn gap(&self) -> Size<LengthPercentage> {
        self.gap_value()
    }
    fn align_content(&self) -> Option<AlignContent> {
        if self.formatting() {
            content_alignment(self.style.layout.align_content)
                .ok()
                .flatten()
        } else {
            None
        }
    }
    fn align_items(&self) -> Option<AlignItems> {
        if self.formatting() {
            item_alignment(self.style.layout.align_items).ok().flatten()
        } else {
            None
        }
    }
    fn justify_content(&self) -> Option<JustifyContent> {
        if self.formatting() {
            content_alignment(self.style.layout.justify_content)
                .ok()
                .flatten()
        } else {
            None
        }
    }
}
impl FlexboxItemStyle for StyleView<'_> {
    fn flex_basis(&self) -> Dimension {
        if !self.item_box() {
            return Dimension::auto();
        }
        match self.style.layout.flex_basis {
            mg::FlexBasis::Length(value) => dimension(value),
            mg::FlexBasis::Auto => Dimension::auto(),
            mg::FlexBasis::Content => Dimension::content(),
        }
    }
    fn flex_grow(&self) -> f32 {
        if self.item_box() {
            self.style.layout.flex_grow
        } else {
            0.
        }
    }
    fn flex_shrink(&self) -> f32 {
        if self.item_box() {
            self.style.layout.flex_shrink
        } else {
            1.
        }
    }
    fn align_self(&self) -> Option<AlignSelf> {
        if self.item_box() {
            // Unlike auto, normal does not inherit a parent's center/end.
            if self.style.layout.align_self.keyword == mg::AlignKeyword::Normal {
                Some(AlignSelf::STRETCH)
            } else {
                item_alignment(self.style.layout.align_self).ok().flatten()
            }
        } else {
            None
        }
    }
}

fn min_track(value: mg::TrackBreadth) -> MinTrackSizingFunction {
    match value {
        mg::TrackBreadth::Auto => MinTrackSizingFunction::auto(),
        mg::TrackBreadth::MinContent => MinTrackSizingFunction::min_content(),
        mg::TrackBreadth::MaxContent => MinTrackSizingFunction::max_content(),
        mg::TrackBreadth::Length(mg::Length::Px(v)) => MinTrackSizingFunction::length(v),
        mg::TrackBreadth::Length(mg::Length::Percent(v)) => MinTrackSizingFunction::percent(v),
        _ => MinTrackSizingFunction::auto(), // validate rejects unrepresentable minima
    }
}
fn max_track(value: mg::TrackBreadth) -> MaxTrackSizingFunction {
    match value {
        mg::TrackBreadth::Auto => MaxTrackSizingFunction::auto(),
        mg::TrackBreadth::MinContent => MaxTrackSizingFunction::min_content(),
        mg::TrackBreadth::MaxContent => MaxTrackSizingFunction::max_content(),
        mg::TrackBreadth::Fr(v) => MaxTrackSizingFunction::fr(v),
        mg::TrackBreadth::Length(mg::Length::Px(v)) => MaxTrackSizingFunction::length(v),
        mg::TrackBreadth::Length(mg::Length::Percent(v)) => MaxTrackSizingFunction::percent(v),
        mg::TrackBreadth::Length(mg::Length::Auto) => MaxTrackSizingFunction::auto(),
    }
}
fn track(value: &mg::TrackSize) -> TrackSizingFunction {
    let (min, max) = match *value {
        mg::TrackSize::Breadth(mg::TrackBreadth::Fr(v)) => {
            (mg::TrackBreadth::Auto, mg::TrackBreadth::Fr(v))
        }
        mg::TrackSize::Breadth(v) => (v, v),
        mg::TrackSize::MinMax(min, max) => (min, max),
    };
    MinMax {
        min: min_track(min),
        max: max_track(max),
    }
}

type Component<'a> = GenericGridTemplateComponent<String, &'a GridTemplateRepetition<String>>;
pub(super) type TrackIter<'a> =
    std::iter::Map<std::slice::Iter<'a, mg::TrackSize>, fn(&mg::TrackSize) -> TrackSizingFunction>;
type LineNames<'a> = std::iter::Map<
    std::slice::Iter<'a, Vec<String>>,
    fn(&Vec<String>) -> std::slice::Iter<'_, String>,
>;

/// Repeats are already bounded and expanded by the snapshot. This iterator
/// borrows those tracks and emits only Single components, without allocations.
#[derive(Clone)]
pub(super) struct TemplateIter<'a>(std::slice::Iter<'a, mg::TrackSize>);
impl<'a> Iterator for TemplateIter<'a> {
    type Item = Component<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|value| GenericGridTemplateComponent::Single(track(value)))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl ExactSizeIterator for TemplateIter<'_> {}

impl GridContainerStyle for StyleView<'_> {
    type Repetition<'a>
        = &'a GridTemplateRepetition<String>
    where
        Self: 'a;
    type TemplateTrackList<'a>
        = TemplateIter<'a>
    where
        Self: 'a;
    type AutoTrackList<'a>
        = TrackIter<'a>
    where
        Self: 'a;
    type TemplateLineNames<'a>
        = LineNames<'a>
    where
        Self: 'a;
    type GridTemplateAreas<'a>
        = std::iter::Empty<GridTemplateArea<String>>
    where
        Self: 'a;

    fn grid_template_rows(&self) -> Option<TemplateIter<'_>> {
        self.grid()
            .filter(|g| !g.template_rows.is_empty())
            .map(|g| TemplateIter(g.template_rows.iter()))
    }
    fn grid_template_columns(&self) -> Option<TemplateIter<'_>> {
        self.grid()
            .filter(|g| !g.template_columns.is_empty())
            .map(|g| TemplateIter(g.template_columns.iter()))
    }
    fn grid_auto_rows(&self) -> TrackIter<'_> {
        self.grid()
            .map_or(&[][..], |g| g.auto_rows.as_slice())
            .iter()
            .map(track)
    }
    fn grid_auto_columns(&self) -> TrackIter<'_> {
        self.grid()
            .map_or(&[][..], |g| g.auto_columns.as_slice())
            .iter()
            .map(track)
    }
    fn grid_template_areas(&self) -> Option<Self::GridTemplateAreas<'_>> {
        None
    }
    fn grid_template_column_names(&self) -> Option<LineNames<'_>> {
        None
    }
    fn grid_template_row_names(&self) -> Option<LineNames<'_>> {
        None
    }
    fn grid_auto_flow(&self) -> GridAutoFlow {
        match self
            .grid()
            .map(|g| g.auto_flow)
            .unwrap_or(mg::GridAutoFlow::Row)
        {
            mg::GridAutoFlow::Row => GridAutoFlow::Row,
            mg::GridAutoFlow::Column => GridAutoFlow::Column,
            mg::GridAutoFlow::RowDense => GridAutoFlow::RowDense,
            mg::GridAutoFlow::ColumnDense => GridAutoFlow::ColumnDense,
        }
    }
    fn gap(&self) -> Size<LengthPercentage> {
        self.gap_value()
    }
    fn align_content(&self) -> Option<AlignContent> {
        FlexboxContainerStyle::align_content(self)
    }
    fn justify_content(&self) -> Option<JustifyContent> {
        FlexboxContainerStyle::justify_content(self)
    }
    fn align_items(&self) -> Option<AlignItems> {
        FlexboxContainerStyle::align_items(self)
    }
    fn justify_items(&self) -> Option<JustifyItems> {
        if self.formatting() {
            item_alignment(self.style.layout.justify_items)
                .ok()
                .flatten()
        } else {
            None
        }
    }
}

fn placement(value: mg::GridLine) -> GridPlacement<String> {
    match value {
        mg::GridLine::Auto => GridPlacement::Auto,
        mg::GridLine::Line(n) => taffy::style_helpers::line(n),
        mg::GridLine::Span(n) => taffy::style_helpers::span(n),
    }
}
impl GridItemStyle for StyleView<'_> {
    fn grid_row(&self) -> Line<GridPlacement<String>> {
        if self.item_box() {
            Line {
                start: placement(self.style.layout.grid_row[0]),
                end: placement(self.style.layout.grid_row[1]),
            }
        } else {
            Line {
                start: GridPlacement::Auto,
                end: GridPlacement::Auto,
            }
        }
    }
    fn grid_column(&self) -> Line<GridPlacement<String>> {
        if self.item_box() {
            Line {
                start: placement(self.style.layout.grid_column[0]),
                end: placement(self.style.layout.grid_column[1]),
            }
        } else {
            Line {
                start: GridPlacement::Auto,
                end: GridPlacement::Auto,
            }
        }
    }
    fn align_self(&self) -> Option<AlignSelf> {
        if !self.item_box() {
            return None;
        }
        if self.style.layout.align_self.keyword == mg::AlignKeyword::Normal {
            Some(
                if self.style.height != mg::Length::Auto || self.natural_ratio.is_some() {
                    AlignSelf::START
                } else {
                    AlignSelf::STRETCH
                },
            )
        } else {
            item_alignment(self.style.layout.align_self).ok().flatten()
        }
    }
    fn justify_self(&self) -> Option<AlignSelf> {
        if self.item_box() {
            if self.style.layout.justify_self.keyword == mg::AlignKeyword::Normal {
                Some(if self.style.width != mg::Length::Auto {
                    AlignSelf::START
                } else {
                    AlignSelf::STRETCH
                })
            } else {
                item_alignment(self.style.layout.justify_self)
                    .ok()
                    .flatten()
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computed(css: &str) -> mg::ComputedStyle {
        let doc = crate::document::parse(
            &format!("<html><body><div id=x style='{css}'></div></body></html>"),
            "https://fixture.example/",
        );
        let id = doc.query_selector(0, "#x").unwrap().unwrap();
        mg::compute_styles(&doc, &doc.stylesheets, (1024., 768.))
            .unwrap()
            .swap_remove(id)
    }

    #[test]
    fn root_content_strips_outer_box_but_preserves_formatting() {
        let s = computed(
            "display:flex;position:absolute;left:3px;width:40%;height:100px;padding:5px;margin:7px;border:2px solid;gap:12px 6px;flex-direction:column;align-items:safe center",
        );
        let root = StyleView::root_content(&s);
        assert!(root.validate().is_ok());
        assert_eq!(
            root.size(),
            Size {
                width: Dimension::auto(),
                height: Dimension::auto()
            }
        );
        assert_eq!(root.padding(), rect([LengthPercentage::length(0.); 4]));
        assert_eq!(root.margin(), rect([LengthPercentageAuto::length(0.); 4]));
        assert_eq!(root.border(), rect([LengthPercentage::length(0.); 4]));
        assert_eq!(root.position(), Position::Relative);
        assert_eq!(root.inset(), rect([LengthPercentageAuto::auto(); 4]));
        assert_eq!(root.flex_direction(), FlexDirection::Column);
        assert_eq!(
            FlexboxContainerStyle::gap(&root),
            Size {
                width: LengthPercentage::length(6.),
                height: LengthPercentage::length(12.)
            }
        );
        assert_eq!(
            FlexboxContainerStyle::align_items(&root).unwrap().safety,
            AlignmentSafety::Safe
        );
    }

    #[test]
    fn item_and_leaf_keep_physical_edges_percentages_and_intrinsic_ratio() {
        let s = computed(
            "position:absolute;left:25%;top:4px;width:40%;min-width:12px;max-height:80%;margin:1px 2px 3px 4px;padding:5%;border:2px solid;box-sizing:border-box",
        );
        let view = StyleView::item(&s, Some(1.5));
        assert!(view.validate().is_ok());
        let leaf = LeafStyle::from(view);
        assert_eq!(leaf.size().width, Dimension::percent(0.4));
        assert_eq!(leaf.margin().left, LengthPercentageAuto::length(4.));
        assert_eq!(leaf.margin().bottom, LengthPercentageAuto::length(3.));
        assert_eq!(leaf.inset().left, LengthPercentageAuto::percent(0.25));
        assert_eq!(leaf.padding().top, LengthPercentage::percent(0.05));
        assert_eq!(leaf.box_sizing(), BoxSizing::BorderBox);
        assert_eq!(leaf.position(), Position::Absolute);
        assert_eq!(leaf.aspect_ratio(), Some(1.5));
        assert!(leaf.is_compressible_replaced());
        fn assert_copy<T: Copy>() {}
        assert_copy::<LeafStyle>();
        assert!(std::mem::size_of::<LeafStyle>() < std::mem::size_of::<taffy::Style>());
    }

    #[test]
    fn max_content_replaced_width_is_measured_without_border_box_ratio_transfer() {
        for sizing in ["content-box", "border-box"] {
            let s = computed(&format!(
                "width:max-content;height:74px;box-sizing:{sizing};padding:5px;border:2px solid"
            ));
            let view = StyleView::item(&s, Some(2.));
            assert!(view.validate().is_ok());
            assert_eq!(view.size().width, Dimension::max_content());
            assert_eq!(view.size().height, Dimension::length(74.));
            assert_eq!(view.aspect_ratio(), None);
            assert!(view.is_compressible_replaced());
            let leaf = LeafStyle::from(view);
            assert_eq!(leaf.aspect_ratio(), None);
            assert!(leaf.is_compressible_replaced());
        }
        for width in ["auto", "fit-content", "80px"] {
            let s = computed(&format!("width:{width};height:74px;box-sizing:border-box"));
            assert_eq!(StyleView::item(&s, Some(2.)).aspect_ratio(), Some(2.));
        }
    }

    #[test]
    fn anonymous_items_do_not_inherit_parent_constraints_or_unsupported_position() {
        let mut s = computed(
            "display:grid;position:fixed;width:100px;grid-column:span 8;flex:2 3 20%;align-self:end;grid-template-columns:repeat(12,1fr)",
        );
        s.layout.unsupported = true;
        let anon = StyleView::anonymous(&s);
        assert!(anon.validate().is_ok());
        assert_eq!(anon.flex_grow(), 0.);
        assert_eq!(anon.flex_shrink(), 1.);
        assert_eq!(anon.flex_basis(), Dimension::auto());
        assert_eq!(GridItemStyle::align_self(&anon), None);
        assert_eq!(anon.grid_column().start, GridPlacement::Auto);
        assert!(anon.grid_template_columns().is_none());
        assert_eq!(anon.grid_auto_columns().len(), 0);
    }

    #[test]
    fn static_insets_are_ignored_and_content_box_is_the_css_default() {
        let s = computed("left:10px;top:20%;width:100px");
        let view = StyleView::item(&s, None);
        assert_eq!(view.box_sizing(), BoxSizing::ContentBox);
        assert_eq!(view.position(), Position::Relative);
        assert_eq!(view.inset(), rect([LengthPercentageAuto::auto(); 4]));
    }

    #[test]
    fn tracks_are_borrowed_exact_iterators_and_keep_fraction_minmax_semantics() {
        let s = computed(
            "display:grid;grid-template-columns:repeat(12,minmax(0,1fr));grid-auto-rows:20px 10%;grid-auto-flow:column dense;grid-column:2 / span 8;grid-row:-1 / auto",
        );
        let view = StyleView::item(&s, None);
        assert!(view.validate().is_ok());
        let mut tracks = view.grid_template_columns().unwrap();
        assert_eq!(tracks.len(), 12);
        match tracks.next().unwrap() {
            GenericGridTemplateComponent::Single(t) => {
                assert_eq!(t.min, MinTrackSizingFunction::length(0.));
                assert_eq!(t.max, MaxTrackSizingFunction::fr(1.));
            }
            _ => panic!("integer repeat was not flattened"),
        }
        assert_eq!(tracks.len(), 11);
        assert_eq!(tracks.clone().count(), 11);
        assert_eq!(view.grid_auto_rows().len(), 2);
        assert_eq!(view.grid_auto_flow(), GridAutoFlow::ColumnDense);
        assert_eq!(view.grid_column().end, taffy::style_helpers::span(8));
        assert_eq!(view.grid_row().start, taffy::style_helpers::line(-1));
        assert!(view.grid_template_row_names().is_none());
        assert!(view.grid_template_areas().is_none());
        let fr = track(&mg::TrackSize::Breadth(mg::TrackBreadth::Fr(2.)));
        assert_eq!(fr.min, MinTrackSizingFunction::auto());
        assert_eq!(fr.max, MaxTrackSizingFunction::fr(2.));
    }

    #[test]
    fn unsupported_semantics_are_rejected_before_layout() {
        for css in [
            "position:fixed",
            "position:sticky",
            "justify-content:left",
            "align-items:last baseline",
            "width:calc(50% + 10px)",
        ] {
            assert!(
                StyleView::item(&computed(css), None).validate().is_err(),
                "{css}"
            );
        }
        let mut s = computed("display:flex");
        s.layout.flex_grow = f32::NAN;
        assert!(StyleView::item(&s, None).validate().is_err());
        s.layout.flex_grow = 0.;
        assert!(StyleView::item(&s, Some(f32::INFINITY)).validate().is_err());
        s.layout.grid_row = [mg::GridLine::Line(0), mg::GridLine::Auto];
        assert!(StyleView::item(&s, None).validate().is_err());
    }

    #[test]
    fn flex_content_is_distinct_from_auto_and_self_normal_does_not_inherit() {
        let s = computed("flex-basis:content;width:120px;align-self:normal;justify-self:normal");
        let view = StyleView::item(&s, None);
        assert!(view.validate().is_ok());
        assert_eq!(view.flex_basis(), Dimension::content());
        assert_ne!(view.flex_basis(), Dimension::auto());
        assert_eq!(
            FlexboxItemStyle::align_self(&view),
            Some(AlignSelf::STRETCH)
        );
        assert_eq!(GridItemStyle::justify_self(&view), Some(AlignSelf::START));
        assert_eq!(GridItemStyle::align_self(&view), Some(AlignSelf::STRETCH));
        assert_eq!(
            GridItemStyle::align_self(&StyleView::item(&s, Some(1.5))),
            Some(AlignSelf::START)
        );
        let auto = computed("align-self:auto;justify-self:auto");
        let view = StyleView::item(&auto, None);
        assert_eq!(FlexboxItemStyle::align_self(&view), None);
        assert_eq!(GridItemStyle::align_self(&view), None);
        assert_eq!(GridItemStyle::justify_self(&view), None);
    }

    #[test]
    fn intrinsic_min_width_requires_measurement_without_clearing_other_loss() {
        for css in [
            "min-width:min-content",
            "min-width:max-content;box-sizing:border-box;padding:5px",
        ] {
            let s = computed(css);
            assert!(s.layout.min_width_intrinsic.is_some());
            let view = StyleView::item(&s, None);
            assert!(view.validate().is_err());
            assert!(view.with_intrinsic_min_width(f32::NAN).validate().is_err());
            let resolved = view.with_intrinsic_min_width(123.);
            assert!(resolved.validate().is_ok());
            assert_eq!(
                resolved.min_size().width,
                LengthPercentageAuto::length(123.)
            );
            assert_eq!(
                LeafStyle::from(resolved).min_size().width,
                LengthPercentageAuto::length(123.)
            );
            assert_eq!(
                StyleView::root_content(&s).min_size().width,
                LengthPercentageAuto::auto()
            );
        }
        let s = computed("min-width:min-content;width:calc(50% + 3px)");
        assert!(s.layout.unsupported);
        assert!(
            StyleView::item(&s, None)
                .with_intrinsic_min_width(123.)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn fit_content_width_is_a_keyword_not_auto_or_a_root_constraint() {
        let s = computed("width:fit-content;box-sizing:border-box;padding:5px");
        assert!(s.layout.width_fit_content);
        let view = StyleView::item(&s, None);
        assert!(view.validate().is_ok());
        assert_eq!(view.size().width, Dimension::fit_content());
        assert_eq!(LeafStyle::from(view).size().width, Dimension::fit_content());
        assert_eq!(StyleView::root_content(&s).size().width, Dimension::auto());
        for css in [
            "width:fit-content(40px)",
            "width:min-content",
            "height:fit-content",
            "width:fit-content;height:calc(50% + 3px)",
        ] {
            assert!(
                StyleView::item(&computed(css), None).validate().is_err(),
                "{css}"
            );
        }
    }

    #[test]
    fn auto_and_scroll_retain_scroll_container_sizing_for_static_paint() {
        for css in ["overflow:auto", "overflow:scroll"] {
            let s = computed(css);
            let view = StyleView::item(&s, None);
            assert!(view.validate().is_ok());
            assert_eq!(
                view.overflow(),
                Point {
                    x: Overflow::Scroll,
                    y: Overflow::Scroll
                }
            );
            assert_eq!(LeafStyle::from(view).overflow(), view.overflow());
        }
    }
}
