//! CSS computation and the small, renderer-facing style contract.
//!
//! Values are indexed by the document arena. Percentages remain relative until
//! layout knows the containing block; all other lengths are CSS pixels.

pub use crate::document::StylesheetSource;

#[cfg(feature = "modern")]
pub(crate) mod declarations;

use crate::document::{Document, Node};
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, SelectorCaches,
};
use selectors::matching::{ElementSelectorFlags, VisitedHandlingMode};
use selectors::{Element, OpaqueElement};
use std::hash::{Hash, Hasher};
use std::{
    cell::RefCell,
    fmt::{self, Write},
};
use style::applicable_declarations::{ApplicableDeclarationBlock, ApplicableDeclarationList};
use style::context::{CascadeInputs, QuirksMode, SharedStyleContext, TreeCountingCaches};
use style::data::{ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::device::Device;
use style::dom::{LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot};
use style::properties::{ComputedValues, PropertyDeclarationBlock};
use style::selector_parser::{AttrValue, Lang, NonTSPseudoClass, PseudoElement, SelectorImpl};
use style::servo_arc::{Arc, ArcBorrow};
use style::shared_lock::{Locked, SharedRwLock, StylesheetGuards};
use style::stylesheets::{
    AllowImportRules, CssRuleType, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData,
};
use style::stylist::{CascadeData, RuleInclusion, Stylist};
use style::values::AtomIdent;
use style::{Atom, LocalName, Namespace};
use stylo_dom::ElementState;
type BorrowedLocalName = <SelectorImpl as selectors::parser::SelectorImpl>::BorrowedLocalName;
type BorrowedNamespace = <SelectorImpl as selectors::parser::SelectorImpl>::BorrowedNamespaceUrl;

/// One bounded diagnostic from the actual CSS computation/rendering pass.
/// Parser positions are one-based within a stylesheet or style attribute, not
/// the enclosing HTML file; columns count UTF-16 code units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleDiagnostic {
    pub kind: &'static str,
    pub message: String,
    pub source: String,
    pub node: Option<usize>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    /// A message/source field was shortened; its text includes `[truncated]`.
    pub truncated: bool,
}

/// At most 64 entries, with 512-byte messages and 256-byte sources. `omitted`
/// counts additional diagnostics, saturating rather than growing storage.
/// These are parse errors and selected known renderer gaps, not an exhaustive
/// CSS support/conformance report. Collection never changes rendering policy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleDiagnostics {
    pub entries: Vec<StyleDiagnostic>,
    pub omitted: usize,
}

impl StyleDiagnostics {
    pub const MAX_ENTRIES: usize = 64;
    pub const MAX_MESSAGE_BYTES: usize = 512;
    pub const MAX_SOURCE_BYTES: usize = 256;

    pub(crate) fn record(
        &mut self,
        kind: &'static str,
        message: fmt::Arguments<'_>,
        source: fmt::Arguments<'_>,
        node: Option<usize>,
        location: Option<cssparser::SourceLocation>,
    ) {
        if self.entries.len() >= Self::MAX_ENTRIES {
            self.omitted = self.omitted.saturating_add(1);
            if matches!(kind, "style-fallback" | "layout-fallback") {
                // The reason a whole rendering path failed must not disappear
                // behind a full buffer of earlier unsupported-property notices.
                self.entries.pop();
            } else {
                return;
            }
        }
        let (message, message_cut) = bounded_diagnostic_text(message, Self::MAX_MESSAGE_BYTES);
        let (source, source_cut) = bounded_diagnostic_text(source, Self::MAX_SOURCE_BYTES);
        self.entries.push(StyleDiagnostic {
            kind,
            message,
            source,
            node,
            line: location.map(|p| p.line.saturating_add(1)),
            column: location.map(|p| p.column),
            truncated: message_cut || source_cut,
        });
    }
}

// Format directly into capped storage: a malformed declaration can contain a
// very large token, so formatting first and truncating later is insufficient.
fn bounded_diagnostic_text(args: fmt::Arguments<'_>, max: usize) -> (String, bool) {
    const MARKER: &str = "[truncated]";
    struct Text {
        value: String,
        limit: usize,
        cut: bool,
    }
    impl Write for Text {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            let available = self.limit.saturating_sub(self.value.len());
            let mut end = text.len().min(available);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            self.value.push_str(&text[..end]);
            if end < text.len() {
                self.cut = true;
                Err(fmt::Error)
            } else {
                Ok(())
            }
        }
    }
    let mut text = Text {
        value: String::with_capacity(max),
        limit: max.saturating_sub(MARKER.len()),
        cut: false,
    };
    let _ = text.write_fmt(args);
    if text.cut {
        text.value.push_str(MARKER);
    }
    (text.value, text.cut)
}

struct CssReporter<'a, 'b> {
    diagnostics: &'a RefCell<&'b mut StyleDiagnostics>,
    label: &'a str,
    node: Option<usize>,
}

impl style::error_reporting::ParseErrorReporter for CssReporter<'_, '_> {
    fn report_error(
        &self,
        url: &UrlExtraData,
        location: cssparser::SourceLocation,
        error: style::error_reporting::ContextualParseError,
    ) {
        self.diagnostics.borrow_mut().record(
            "css-parse",
            format_args!("{error}"),
            format_args!("{}: {}", self.label, url.as_str()),
            self.node,
            Some(location),
        );
    }
}

/// Comparison operator applied after resolving `px + percent * basis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcOp {
    Linear,
    MinPx,
    MaxPx,
}

/// A CSS length whose containing-block percentage is resolved during layout.
/// 8-byte aligned so `&Length` can be passed directly to `CompactLength::calc`.
#[repr(align(8))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Px(f32),
    /// A fraction: `0.85` represents `85%`.
    Percent(f32),
    /// A linear `<length-percentage>` sum (`px + percent * basis`), optionally
    /// bounded against a pixel value (`min`/`max`) and clamped to non-negative
    /// when `non_negative` is true (`width`, `height`, `padding`).
    Calc {
        px: f32,
        percent: f32,
        bound_px: f32,
        op: CalcOp,
        non_negative: bool,
    },
}

const _: [(); 16] = [(); std::mem::size_of::<Length>()];

impl Length {
    pub fn resolve(self, basis: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(value) => Some(value),
            Self::Percent(value) => Some(value * basis),
            Self::Calc {
                px,
                percent,
                bound_px,
                op,
                non_negative,
            } => {
                let linear = px + percent * basis;
                let value = match op {
                    CalcOp::Linear => linear,
                    CalcOp::MinPx => linear.min(bound_px),
                    CalcOp::MaxPx => linear.max(bound_px),
                };
                Some(if non_negative { value.max(0.0) } else { value })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    None,
    Block,
    Inline,
    InlineBlock,
    Table,
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    TableCaption,
    TableColumn,
    TableColumnGroup,
    ListItem,
    Contents,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    Visible,
    Hidden,
    Clip,
    Auto,
    Scroll,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlexWrap {
    NoWrap,
    Wrap,
    WrapReverse,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlexBasis {
    Auto,
    Content,
    Length(Length),
}

/// Intrinsic inline-size constraints need Mg's content measurement before a
/// numeric minimum is passed to the formatting engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntrinsicSize {
    MinContent,
    MaxContent,
    FitContent,
}

/// Alignment keywords remain distinct until the formatting context resolves them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignKeyword {
    Auto,
    Normal,
    Start,
    End,
    FlexStart,
    FlexEnd,
    Center,
    Left,
    Right,
    Baseline,
    LastBaseline,
    Stretch,
    SelfStart,
    SelfEnd,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    AnchorCenter,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignSafety {
    Default,
    Safe,
    Unsafe,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alignment {
    pub keyword: AlignKeyword,
    pub safety: AlignSafety,
    pub legacy: bool,
}
impl Alignment {
    pub const fn new(keyword: AlignKeyword) -> Self {
        Self {
            keyword,
            safety: AlignSafety::Default,
            legacy: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridLine {
    Auto,
    Line(i16),
    Span(u16),
    NamedLine(String, i16),
    NamedSpan(String, u16),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAutoFlow {
    Row,
    Column,
    RowDense,
    ColumnDense,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackBreadth {
    Auto,
    MinContent,
    MaxContent,
    Length(Length),
    Fr(f32),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackSize {
    Breadth(TrackBreadth),
    MinMax(TrackBreadth, TrackBreadth),
    FitContent(Length),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAutoRepeatMode {
    AutoFill,
    AutoFit,
}
#[derive(Clone, Debug, PartialEq)]
pub struct GridAutoRepeat {
    pub insert_index: usize,
    pub mode: GridAutoRepeatMode,
    pub tracks: Vec<TrackSize>,
    pub line_names: Vec<Vec<String>>,
}

/// Integer repeats are flattened after checking their expanded length. Empty
/// template vectors mean `none` unless `*_auto_repeat` is present; empty
/// implicit vectors mean the initial `auto`.
#[derive(Clone, Debug, PartialEq)]
pub struct GridStyle {
    pub template_rows: Vec<TrackSize>,
    pub template_columns: Vec<TrackSize>,
    pub row_line_names: Vec<Vec<String>>,
    pub col_line_names: Vec<Vec<String>>,
    pub row_auto_repeat: Option<GridAutoRepeat>,
    pub col_auto_repeat: Option<GridAutoRepeat>,
    pub auto_rows: Vec<TrackSize>,
    pub auto_columns: Vec<TrackSize>,
    pub auto_flow: GridAutoFlow,
}
pub const MAX_GRID_TRACKS: usize = 128;

/// Typed layout inputs, not a claim that every represented value is rendered.
/// This stays independent of the private layout implementation and its types.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutStyle {
    pub box_sizing: BoxSizing,
    /// The width keyword, not the fit-content() sizing function. Its intrinsic
    /// content size remains distinct from numeric/auto `ComputedStyle::width`.
    pub width_fit_content: bool,
    /// Preferred intrinsic maximum, without fit-content's available-space clamp.
    pub width_max_content: bool,
    pub max_width_fit_content: bool,
    pub width_intrinsic: Option<IntrinsicSize>,
    pub height_intrinsic: Option<IntrinsicSize>,
    pub min_width_intrinsic: Option<IntrinsicSize>,
    pub min_height_intrinsic: Option<IntrinsicSize>,
    pub max_width_intrinsic: Option<IntrinsicSize>,
    pub max_height_intrinsic: Option<IntrinsicSize>,
    pub flex_basis_intrinsic: Option<IntrinsicSize>,
    pub aspect_ratio: Option<f32>,
    pub aspect_ratio_auto: bool,
    pub original_was_inline_level: bool,
    pub float: FloatSide,
    pub clear: ClearSide,
    pub position: Position,
    /// Physical top, right, bottom, left offsets.
    pub inset: [Length; 4],
    /// Horizontal and vertical overflow, respectively.
    pub overflow: [Overflow; 2],
    pub z_index: Option<i32>,
    pub order: i32,
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: FlexBasis,
    /// Row and column gaps. Normal resolves to zero for flex/grid, not columns.
    pub gap: [Length; 2],
    pub align_content: Alignment,
    pub justify_content: Alignment,
    pub align_items: Alignment,
    pub justify_items: Alignment,
    pub align_self: Alignment,
    pub justify_self: Alignment,
    pub grid_row: [GridLine; 2],
    pub grid_column: [GridLine; 2],
    pub grid: Option<Box<GridStyle>>,
    /// Conversion lost a value (for example calc or a named grid line). The
    /// adapter must reject this snapshot, not interpret its fallback as support.
    pub unsupported: bool,
    pub(crate) outer_calc_only: bool,
}
impl Default for LayoutStyle {
    fn default() -> Self {
        let normal = Alignment::new(AlignKeyword::Normal);
        let auto = Alignment::new(AlignKeyword::Auto);
        Self {
            box_sizing: BoxSizing::ContentBox,
            width_fit_content: false,
            width_max_content: false,
            max_width_fit_content: false,
            width_intrinsic: None,
            height_intrinsic: None,
            min_width_intrinsic: None,
            min_height_intrinsic: None,
            max_width_intrinsic: None,
            max_height_intrinsic: None,
            flex_basis_intrinsic: None,
            aspect_ratio: None,
            aspect_ratio_auto: true,
            original_was_inline_level: false,
            float: FloatSide::None,
            clear: ClearSide::None,
            position: Position::Static,
            inset: [Length::Auto; 4],
            overflow: [Overflow::Visible; 2],
            z_index: None,
            order: 0,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            flex_grow: 0.,
            flex_shrink: 1.,
            flex_basis: FlexBasis::Auto,
            gap: [Length::Px(0.); 2],
            align_content: normal,
            justify_content: normal,
            align_items: normal,
            justify_items: normal,
            align_self: auto,
            justify_self: auto,
            grid_row: [GridLine::Auto, GridLine::Auto],
            grid_column: [GridLine::Auto, GridLine::Auto],
            grid: None,
            unsupported: false,
            outer_calc_only: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FloatSide {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClearSide {
    #[default]
    None,
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextDirection {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PseudoStyle {
    pub style: ComputedStyle,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Start,
    Left,
    Right,
    Center,
    Justify,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VerticalAlign {
    Baseline,
    Top,
    Middle,
    Bottom,
    TextTop,
    TextBottom,
    Sub,
    Super,
    Length(Length),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// Software-surface RGB, `0x00RRGGBB`.
    pub rgb: u32,
    pub alpha: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundImage {
    pub url: String,
    pub width: Length,
    pub height: Length,
    pub pos_x: Length,
    pub pos_y: Length,
}

/// Rectangular background painting/positioning areas. Non-rectangular clipping
/// must not silently become a border-box fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundBox {
    Border,
    Padding,
    Content,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientDirection {
    /// CSS radians: zero points up, positive angles rotate clockwise.
    Angle(f32),
    /// Signs towards a CSS corner; the used angle depends on the image aspect.
    Corner { right: bool, bottom: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadialExtent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadialShape {
    CircleExplicit(Length),
    CircleExtent(RadialExtent),
    EllipseExplicit(Length, Length),
    EllipseExtent(RadialExtent),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GradientKind {
    Linear(GradientDirection),
    Radial {
        shape: RadialShape,
        center: [Length; 2],
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    /// Unspecified first/last stops are represented as `Some(0%)` / `Some(100%)`;
    /// `None` means an unspecified intermediate stop resolved at paint time.
    pub position: Option<Length>,
    /// Optional color-transition midpoint hint between this stop and the next stop.
    pub hint_to_next: Option<Length>,
}

/// Bounded sRGB linear/radial background gradient (`2..=16` stops).
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundGradient {
    pub kind: GradientKind,
    pub repeating: bool,
    pub stops: Vec<GradientStop>,
    pub origin: BackgroundBox,
    pub clip: BackgroundBox,
    pub repeat: [bool; 2],
    pub size: [Length; 2],
    pub position: [Length; 2],
}
pub type LinearGradient = BackgroundGradient;

impl BackgroundGradient {
    pub fn linear_direction(&self) -> Option<GradientDirection> {
        match self.kind {
            GradientKind::Linear(dir) => Some(dir),
            GradientKind::Radial { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub spread_radius: f32,
    pub color: Color,
    pub inset: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextShadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransformOp2D {
    Translate(Length, Length),
    Scale(f32, f32),
    Matrix { a: f32, d: f32, e: f32, f: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransformStyle {
    pub origin: [Length; 2],
    pub ops: Vec<TransformOp2D>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipPathInset {
    pub inset: [Length; 4],
    pub radii: [(Length, Length); 4],
}

/// Solid SVG paint after the CSS cascade. Color alpha includes the respective
/// fill/stroke opacity, but not group opacity. Paint servers/context paint must
/// be rejected by the bounded icon serializer, never replaced with black.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvgPaint {
    None,
    Color(Color),
    Unsupported,
}

/// Computed properties consumed by the bounded block/inline/table renderer.
#[derive(Debug, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub layout: LayoutStyle,
    pub color: Color,
    pub background_color: Color,
    pub background_clip: BackgroundBox,
    pub background_gradient: Option<Box<BackgroundGradient>>,
    pub box_shadows: Vec<BoxShadow>,
    pub text_shadows: Vec<TextShadow>,
    pub transform: Option<Box<TransformStyle>>,
    pub has_transform: bool,
    pub clip_rect: Option<[Option<f32>; 4]>,
    pub clip_path_inset: Option<ClipPathInset>,
    pub text_overflow_ellipsis: bool,
    pub svg_fill: SvgPaint,
    pub svg_stroke: SvgPaint,
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    pub font_families: Vec<String>,
    /// `None` means the font's normal line height.
    pub line_height: Option<f32>,
    pub underline: bool,
    /// Sides are top, right, bottom, left.
    pub margin: [Length; 4],
    pub padding: [Length; 4],
    pub border_width: [f32; 4],
    pub border_color: [Color; 4],
    /// Corner radii `[top_left, top_right, bottom_right, bottom_left]` as `(rx, ry)`.
    pub border_radii: [(Length, Length); 4],
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub max_height: Length,
    pub text_align: TextAlign,
    pub direction: TextDirection,
    pub vertical_align: VerticalAlign,
    pub white_space: WhiteSpace,
    pub visibility: Visibility,
    pub visible: bool,
    pub own_opacity: f32,
    pub(crate) opacity: f32,
    pub pointer_events: bool,
    pub border_collapse: bool,
    /// Horizontal, vertical CSS pixel spacing.
    pub border_spacing: [f32; 2],
    /// URL layers in CSS front-to-back order. Paint in reverse order.
    pub background_images: Vec<BackgroundImage>,
    pub mask_image: Option<Box<BackgroundImage>>,
    pub before: Option<Box<PseudoStyle>>,
    pub after: Option<Box<PseudoStyle>>,
}

impl Default for ComputedStyle {
    fn default() -> Self {
        let black = Color { rgb: 0, alpha: 1.0 };
        Self {
            display: Display::Inline,
            layout: LayoutStyle::default(),
            color: black,
            background_color: Color { rgb: 0, alpha: 0.0 },
            background_clip: BackgroundBox::Border,
            background_gradient: None,
            box_shadows: Vec::new(),
            text_shadows: Vec::new(),
            transform: None,
            has_transform: false,
            clip_rect: None,
            clip_path_inset: None,
            text_overflow_ellipsis: false,
            svg_fill: SvgPaint::Color(black),
            svg_stroke: SvgPaint::None,
            font_size: 16.0,
            font_weight: 400,
            italic: false,
            font_families: vec!["sans-serif".to_owned()],
            line_height: None,
            underline: false,
            margin: [Length::Px(0.0); 4],
            padding: [Length::Px(0.0); 4],
            border_width: [0.0; 4],
            border_color: [black; 4],
            border_radii: [(Length::Px(0.0), Length::Px(0.0)); 4],
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            min_height: Length::Auto,
            max_width: Length::Auto,
            max_height: Length::Auto,
            text_align: TextAlign::Start,
            direction: TextDirection::Ltr,
            vertical_align: VerticalAlign::Baseline,
            white_space: WhiteSpace::Normal,
            visibility: Visibility::Visible,
            visible: true,
            own_opacity: 1.0,
            opacity: 1.0,
            pointer_events: true,
            border_collapse: false,
            border_spacing: [2.0; 2],
            background_images: Vec::new(),
            mask_image: None,
            before: None,
            after: None,
        }
    }
}

impl ComputedStyle {
    fn clone_with_layout(&self, layout: LayoutStyle) -> Self {
        Self {
            display: self.display,
            layout,
            color: self.color,
            background_color: self.background_color,
            background_clip: self.background_clip,
            background_gradient: self.background_gradient.clone(),
            box_shadows: self.box_shadows.clone(),
            text_shadows: self.text_shadows.clone(),
            transform: self.transform.clone(),
            has_transform: self.has_transform,
            clip_rect: self.clip_rect,
            clip_path_inset: self.clip_path_inset,
            text_overflow_ellipsis: self.text_overflow_ellipsis,
            svg_fill: self.svg_fill,
            svg_stroke: self.svg_stroke,
            font_size: self.font_size,
            font_weight: self.font_weight,
            italic: self.italic,
            font_families: self.font_families.clone(),
            line_height: self.line_height,
            underline: self.underline,
            margin: self.margin,
            padding: self.padding,
            border_width: self.border_width,
            border_color: self.border_color,
            border_radii: self.border_radii,
            width: self.width,
            height: self.height,
            min_width: self.min_width,
            min_height: self.min_height,
            max_width: self.max_width,
            max_height: self.max_height,
            text_align: self.text_align,
            direction: self.direction,
            vertical_align: self.vertical_align,
            white_space: self.white_space,
            visibility: self.visibility,
            visible: self.visible,
            own_opacity: self.own_opacity,
            opacity: self.opacity,
            pointer_events: self.pointer_events,
            border_collapse: self.border_collapse,
            border_spacing: self.border_spacing,
            background_images: self.background_images.clone(),
            mask_image: self.mask_image.clone(),
            before: self.before.clone(),
            after: self.after.clone(),
        }
    }

    // Preserve the existing text-style convention without ever cloning an
    // element's non-inherited grid or pseudo-element allocations into a text node.
    pub(crate) fn inherit_for_text(&self) -> Self {
        Self {
            display: self.display,
            layout: LayoutStyle::default(),
            color: self.color,
            background_color: self.background_color,
            background_clip: self.background_clip,
            background_gradient: None,
            box_shadows: Vec::new(),
            text_shadows: self.text_shadows.clone(),
            transform: None,
            has_transform: false,
            clip_rect: None,
            clip_path_inset: None,
            text_overflow_ellipsis: false,
            svg_fill: self.svg_fill,
            svg_stroke: self.svg_stroke,
            font_size: self.font_size,
            font_weight: self.font_weight,
            italic: self.italic,
            font_families: self.font_families.clone(),
            line_height: self.line_height,
            underline: self.underline,
            margin: self.margin,
            padding: self.padding,
            border_width: self.border_width,
            border_color: self.border_color,
            border_radii: [(Length::Px(0.0), Length::Px(0.0)); 4],
            width: self.width,
            height: self.height,
            min_width: self.min_width,
            min_height: self.min_height,
            max_width: self.max_width,
            max_height: self.max_height,
            text_align: self.text_align,
            direction: self.direction,
            vertical_align: self.vertical_align,
            white_space: self.white_space,
            visibility: self.visibility,
            visible: self.visible,
            own_opacity: 1.0,
            opacity: self.opacity,
            pointer_events: self.pointer_events,
            border_collapse: self.border_collapse,
            border_spacing: self.border_spacing,
            background_images: self.background_images.clone(),
            mask_image: None,
            before: None,
            after: None,
        }
    }
}
impl Clone for ComputedStyle {
    fn clone(&self) -> Self {
        self.clone_with_layout(self.layout.clone())
    }
}

// This is a deliberately small UA sheet, shared by all documents and fixtures.
// Author rules and inline style participate in Stylo's normal cascade above it.
const USER_AGENT_CSS: &str = r#"
html, body, div, p, section, article, aside, header, footer, main, nav,
details, summary, fieldset, legend, figure, figcaption, form, center,
blockquote, pre, ul, ol, dl, dt, dd, h1, h2, h3, h4, h5, h6 { display: block }
head, title, style, script, meta, link, source, track, template, [hidden], input[type=hidden], dialog:not([open]) { display: none }
dialog[open] { display: block; border: 2px solid #202124; padding: 16px; background: #ffffff; color: #202124 }
body { margin: 8px; font-family: sans-serif; color: black }
p, blockquote, ul, ol, dl, pre { margin-top: 1em; margin-bottom: 1em }
h1 { font-size: 2em; margin: .67em 0; font-weight: bold }
h2 { font-size: 1.5em; margin: .83em 0; font-weight: bold }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }
b, strong, th { font-weight: bold }
i, em, cite, dfn, var, address { font-style: italic }
small { font-size: smaller }
pre { white-space: pre; font-family: monospace }
center { text-align: center }
a:link { color: #0000ee; text-decoration: underline }
table { display: table; border-spacing: 2px; border-collapse: separate; box-sizing: border-box; text-indent: 0; text-align: start }
thead { display: table-header-group } tbody { display: table-row-group }
tfoot { display: table-footer-group } tr { display: table-row }
td, th { display: table-cell; vertical-align: inherit; padding: 1px }
tr, thead, tbody, tfoot { vertical-align: middle }
th { text-align: center } caption { display: table-caption; text-align: center }
col { display: table-column } colgroup { display: table-column-group }
li { display: list-item }
input, button, select, textarea { display: inline-block; font-size: 13.3333px }
input, button, select, textarea { padding: 1px 2px; border: 2px solid #888 }
select { box-sizing: border-box }
input[type=checkbox], input[type=radio] { padding: 0; border: 1px solid #666 }
form { margin-bottom: 1em }
hr { display: block; margin: .5em auto; border: 1px solid #888 }
"#;

const MAX_SNAPSHOT_HEAP_BYTES: usize = 8 * 1024 * 1024;
const MAX_BACKGROUND_LAYERS: usize = 8;
const MAX_FONT_FAMILIES: usize = 16;
const MAX_STYLE_URL_BYTES: usize = 4096;
const MAX_FONT_NAME_BYTES: usize = 256;
pub const MAX_GRADIENT_STOPS: usize = 16;
pub const MAX_BOX_SHADOWS: usize = 8;
pub const MAX_TEXT_SHADOWS: usize = 4;
pub const MAX_TRANSFORM_OPS: usize = 16;

fn pseudo_heap_bytes(pseudo: &Option<Box<PseudoStyle>>) -> usize {
    pseudo.as_ref().map_or(0, |item| {
        std::mem::size_of::<PseudoStyle>() + snapshot_heap_bytes(&item.style) + item.text.capacity()
    })
}

fn gradient_heap_bytes(gradient: &Option<Box<BackgroundGradient>>) -> usize {
    gradient.as_ref().map_or(0, |g| {
        std::mem::size_of::<BackgroundGradient>()
            + g.stops.capacity() * std::mem::size_of::<GradientStop>()
    })
}

fn transform_heap_bytes(transform: &Option<Box<TransformStyle>>) -> usize {
    transform.as_ref().map_or(0, |t| {
        std::mem::size_of::<TransformStyle>()
            + t.ops.capacity() * std::mem::size_of::<TransformOp2D>()
    })
}

fn inherited_heap_bytes(value: &ComputedStyle) -> usize {
    value.font_families.capacity() * std::mem::size_of::<String>()
        + value.background_images.capacity() * std::mem::size_of::<BackgroundImage>()
        + value.text_shadows.capacity() * std::mem::size_of::<TextShadow>()
        + value
            .font_families
            .iter()
            .map(String::capacity)
            .sum::<usize>()
        + value
            .background_images
            .iter()
            .map(|image| image.url.capacity())
            .sum::<usize>()
}

pub(crate) fn snapshot_heap_bytes(value: &ComputedStyle) -> usize {
    layout_heap_bytes(&value.layout)
        + pseudo_heap_bytes(&value.before)
        + pseudo_heap_bytes(&value.after)
        + gradient_heap_bytes(&value.background_gradient)
        + transform_heap_bytes(&value.transform)
        + value.box_shadows.capacity() * std::mem::size_of::<BoxShadow>()
        + value.mask_image.as_ref().map_or(0, |m| {
            std::mem::size_of::<BackgroundImage>() + m.url.capacity()
        })
        + inherited_heap_bytes(value)
}

fn line_names_heap_bytes(lines: &Vec<Vec<String>>) -> usize {
    lines.capacity() * std::mem::size_of::<Vec<String>>()
        + lines
            .iter()
            .map(|set| {
                set.capacity() * std::mem::size_of::<String>()
                    + set.iter().map(String::capacity).sum::<usize>()
            })
            .sum::<usize>()
}

fn auto_repeat_heap_bytes(repeat: &Option<GridAutoRepeat>) -> usize {
    repeat.as_ref().map_or(0, |ar| {
        ar.tracks.capacity() * std::mem::size_of::<TrackSize>()
            + line_names_heap_bytes(&ar.line_names)
    })
}

fn grid_line_heap_bytes(line: &GridLine) -> usize {
    match line {
        GridLine::NamedLine(s, _) | GridLine::NamedSpan(s, _) => s.capacity(),
        GridLine::Auto | GridLine::Line(_) | GridLine::Span(_) => 0,
    }
}

fn layout_heap_bytes(value: &LayoutStyle) -> usize {
    value
        .grid_row
        .iter()
        .chain(value.grid_column.iter())
        .map(grid_line_heap_bytes)
        .sum::<usize>()
        + value.grid.as_ref().map_or(0, |grid| {
            std::mem::size_of::<GridStyle>()
                + [
                    &grid.template_rows,
                    &grid.template_columns,
                    &grid.auto_rows,
                    &grid.auto_columns,
                ]
                .iter()
                .map(|tracks| tracks.capacity() * std::mem::size_of::<TrackSize>())
                .sum::<usize>()
                + line_names_heap_bytes(&grid.row_line_names)
                + line_names_heap_bytes(&grid.col_line_names)
                + auto_repeat_heap_bytes(&grid.row_auto_repeat)
                + auto_repeat_heap_bytes(&grid.col_auto_repeat)
        })
}

fn admit_snapshot(
    total: &mut usize,
    old: &ComputedStyle,
    new: &ComputedStyle,
) -> Result<(), String> {
    admit_snapshot_bytes(total, snapshot_heap_bytes(old), snapshot_heap_bytes(new))
}

fn admit_snapshot_bytes(total: &mut usize, old: usize, new: usize) -> Result<(), String> {
    let next = total
        .checked_sub(old)
        .and_then(|bytes| bytes.checked_add(new))
        .filter(|&bytes| bytes <= MAX_SNAPSHOT_HEAP_BYTES)
        .ok_or("computed style snapshot bound exceeded")?;
    *total = next;
    Ok(())
}

#[derive(Debug)]
struct NodeStyleData {
    local_name: LocalName,
    id: Option<AtomIdent>,
    classes: Vec<AtomIdent>,
    attr_names: Vec<LocalName>,
    inline: Option<Arc<Locked<PropertyDeclarationBlock>>>,
    hints: Option<Arc<Locked<PropertyDeclarationBlock>>>,
    data: ElementDataWrapper,
}

struct StyleDocument<'a> {
    nodes_slice: &'a [Node],
    lock: SharedRwLock,
    namespace: Namespace,
    nodes: Vec<NodeStyleData>,
}

#[derive(Clone, Copy)]
struct DomNode<'a> {
    // These borrows never escape compute_styles. Both arenas remain immobile
    // during matching; their addresses are used only as opaque identities.
    document: &'a StyleDocument<'a>,
    id: usize,
}

impl std::fmt::Debug for DomNode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StyleNode")
            .field(&self.id)
            .field(&self.node().tag)
            .finish()
    }
}
impl PartialEq for DomNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && std::ptr::eq(self.document, other.document)
    }
}
impl Eq for DomNode<'_> {}
impl Hash for DomNode<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        std::ptr::from_ref(self.document).hash(state);
    }
}
impl<'a> DomNode<'a> {
    fn node(&self) -> &'a Node {
        &self.document.nodes_slice[self.id]
    }
    fn data(&self) -> &'a NodeStyleData {
        &self.document.nodes[self.id]
    }
    fn at(&self, id: usize) -> Self {
        Self {
            document: self.document,
            id,
        }
    }
    fn sibling(&self, forward: bool) -> Option<Self> {
        let parent = self.parent_node()?;
        let siblings = &parent.node().children;
        let index = siblings.iter().position(|&id| id == self.id)?;
        let index = if forward {
            index.checked_add(1)?
        } else {
            index.checked_sub(1)?
        };
        siblings.get(index).map(|&id| self.at(id))
    }
}
impl NodeInfo for DomNode<'_> {
    fn is_element(&self) -> bool {
        self.id != 0 && !self.node().tag.starts_with('#')
    }
    fn is_text_node(&self) -> bool {
        self.node().tag == "#text"
    }
}
impl<'a> TNode for DomNode<'a> {
    type ConcreteElement = Self;
    type ConcreteDocument = Self;
    type ConcreteShadowRoot = Self;
    fn parent_node(&self) -> Option<Self> {
        let parent = self.node().parent;
        (self.id != 0 && parent != self.id).then(|| self.at(parent))
    }
    fn first_child(&self) -> Option<Self> {
        self.node().children.first().map(|&id| self.at(id))
    }
    fn last_child(&self) -> Option<Self> {
        self.node().children.last().map(|&id| self.at(id))
    }
    fn prev_sibling(&self) -> Option<Self> {
        self.sibling(false)
    }
    fn next_sibling(&self) -> Option<Self> {
        self.sibling(true)
    }
    fn owner_doc(&self) -> Self {
        self.at(0)
    }
    fn is_in_document(&self) -> bool {
        let mut cur = *self;
        for _ in 0..=crate::document::MAX_DEPTH {
            if cur.id == 0 {
                return true;
            }
            let Some(parent) = cur.parent_node() else {
                return false;
            };
            cur = parent;
        }
        false
    }
    fn traversal_parent(&self) -> Option<Self> {
        self.parent_node().filter(NodeInfo::is_element)
    }
    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(std::ptr::from_ref(self.node()) as usize)
    }
    fn debug_id(self) -> usize {
        self.id
    }
    fn as_element(&self) -> Option<Self> {
        self.is_element().then_some(*self)
    }
    fn as_document(&self) -> Option<Self> {
        (self.id == 0).then_some(*self)
    }
    fn as_shadow_root(&self) -> Option<Self> {
        None
    }
}
impl TDocument for DomNode<'_> {
    type ConcreteNode = Self;
    fn as_node(&self) -> Self {
        *self
    }
    fn is_html_document(&self) -> bool {
        true
    }
    fn quirks_mode(&self) -> QuirksMode {
        QuirksMode::NoQuirks
    }
    fn shared_lock(&self) -> &SharedRwLock {
        &self.document.lock
    }
}
impl TShadowRoot for DomNode<'_> {
    type ConcreteNode = Self;
    fn as_node(&self) -> Self {
        *self
    }
    fn host(&self) -> Self {
        *self
    }
    fn style_data<'a>(&self) -> Option<&'a CascadeData>
    where
        Self: 'a,
    {
        None
    }
}

impl Element for DomNode<'_> {
    type Impl = SelectorImpl;
    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.node())
    }
    fn parent_element(&self) -> Option<Self> {
        self.parent_node().filter(NodeInfo::is_element)
    }
    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }
    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }
    fn is_pseudo_element(&self) -> bool {
        false
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        let mut node = self.prev_sibling();
        while let Some(current) = node {
            if current.is_element() {
                return node;
            }
            node = current.prev_sibling();
        }
        None
    }
    fn next_sibling_element(&self) -> Option<Self> {
        let mut node = self.next_sibling();
        while let Some(current) = node {
            if current.is_element() {
                return node;
            }
            node = current.next_sibling();
        }
        None
    }
    fn first_element_child(&self) -> Option<Self> {
        self.node()
            .children
            .iter()
            .map(|&id| self.at(id))
            .find(NodeInfo::is_element)
    }
    fn is_html_element_in_html_document(&self) -> bool {
        self.is_element()
    }
    fn has_local_name(&self, name: &BorrowedLocalName) -> bool {
        self.data().local_name.0 == *name
    }
    fn has_namespace(&self, ns: &BorrowedNamespace) -> bool {
        self.document.namespace.0 == *ns
    }
    fn is_same_type(&self, other: &Self) -> bool {
        self.node().tag == other.node().tag
    }
    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&Namespace>,
        name: &LocalName,
        op: &AttrSelectorOperation<&AttrValue>,
    ) -> bool {
        if let NamespaceConstraint::Specific(namespace) = ns {
            if !namespace.0.is_empty() {
                return false;
            }
        }
        self.node()
            .attr(&name.0)
            .is_some_and(|value| op.eval_str(value))
    }
    fn match_non_ts_pseudo_class(
        &self,
        pc: &NonTSPseudoClass,
        _: &mut MatchingContext<SelectorImpl>,
    ) -> bool {
        match pc {
            NonTSPseudoClass::Lang(lang) => self.match_element_lang(None, lang),
            NonTSPseudoClass::ServoNonZeroBorder => self
                .node()
                .attr("border")
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|n| n != 0),
            NonTSPseudoClass::CustomState(_) => false,
            _ => {
                let flags = pc.state_flag();
                !flags.is_empty() && self.state().intersects(flags)
            }
        }
    }
    fn match_pseudo_element(
        &self,
        pe: &PseudoElement,
        context: &mut MatchingContext<SelectorImpl>,
    ) -> bool {
        context.matching_mode() == MatchingMode::ForStatelessPseudoElement
            && matches!(pe, PseudoElement::Before | PseudoElement::After)
    }
    fn apply_selector_flags(&self, _: ElementSelectorFlags) {}
    fn is_link(&self) -> bool {
        matches!(self.node().tag.as_str(), "a" | "area" | "link") && self.node().has("href")
    }
    fn is_html_slot_element(&self) -> bool {
        false
    }
    fn has_id(&self, id: &AtomIdent, sensitivity: CaseSensitivity) -> bool {
        self.node()
            .attr("id")
            .is_some_and(|value| sensitivity.eq(value.as_bytes(), id.0.as_bytes()))
    }
    fn has_class(&self, name: &AtomIdent, sensitivity: CaseSensitivity) -> bool {
        self.data()
            .classes
            .iter()
            .any(|value| sensitivity.eq(value.0.as_bytes(), name.0.as_bytes()))
    }
    fn has_custom_state(&self, _: &AtomIdent) -> bool {
        false
    }
    fn imported_part(&self, _: &AtomIdent) -> Option<AtomIdent> {
        None
    }
    fn is_part(&self, _: &AtomIdent) -> bool {
        false
    }
    fn is_empty(&self) -> bool {
        self.node().children.iter().all(|&id| {
            let n = self.at(id);
            !n.is_element() && (!n.is_text_node() || n.node().text.is_empty())
        })
    }
    fn is_root(&self) -> bool {
        self.is_element() && self.node().parent == 0
    }
    fn add_element_unique_hashes(&self, _: &mut selectors::bloom::BloomFilter) -> bool {
        false
    }
}

impl<'a> TElement for DomNode<'a> {
    type ConcreteNode = Self;
    type TraversalChildrenIterator = std::vec::IntoIter<Self>;
    fn as_node(&self) -> Self {
        *self
    }
    fn traversal_children(&self) -> LayoutIterator<Self::TraversalChildrenIterator> {
        LayoutIterator(
            self.node()
                .children
                .iter()
                .map(|&id| self.at(id))
                .collect::<Vec<_>>()
                .into_iter(),
        )
    }
    fn is_html_element(&self) -> bool {
        self.is_element()
    }
    fn is_mathml_element(&self) -> bool {
        false
    }
    fn is_svg_element(&self) -> bool {
        false
    }
    fn style_attribute(&self) -> Option<ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        self.data().inline.as_ref().map(Arc::borrow_arc)
    }
    fn animation_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn transition_rule(
        &self,
        _: &SharedStyleContext,
    ) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
        None
    }
    fn state(&self) -> ElementState {
        let mut state = ElementState::DEFINED;
        if self.is_link() {
            state |= ElementState::UNVISITED;
        }
        if matches!(
            self.node().tag.as_str(),
            "input" | "select" | "textarea" | "button" | "option" | "fieldset"
        ) {
            state |= if self.node().has("disabled") {
                ElementState::DISABLED
            } else {
                ElementState::ENABLED
            };
            if self.node().has("checked") || self.node().has("selected") {
                state |= ElementState::CHECKED;
            }
        }
        state
    }
    fn has_part_attr(&self) -> bool {
        false
    }
    fn exports_any_part(&self) -> bool {
        false
    }
    fn id(&self) -> Option<&Atom> {
        self.data().id.as_ref().map(|id| &id.0)
    }
    fn each_class<F: FnMut(&AtomIdent)>(&self, mut callback: F) {
        for class in &self.data().classes {
            callback(class);
        }
    }
    fn each_custom_state<F: FnMut(&AtomIdent)>(&self, _: F) {}
    fn each_attr_name<F: FnMut(&LocalName)>(&self, mut callback: F) {
        for name in &self.data().attr_names {
            callback(name);
        }
    }
    fn has_dirty_descendants(&self) -> bool {
        false
    }
    fn has_snapshot(&self) -> bool {
        false
    }
    fn handled_snapshot(&self) -> bool {
        true
    }
    // Required by Stylo's incremental-traversal interface. This adapter only
    // performs a fresh, single-threaded cascade, with checked interior borrows;
    // it contains no unsafe operations or lifetime extension.
    unsafe fn set_handled_snapshot(&self) {}
    unsafe fn set_dirty_descendants(&self) {}
    unsafe fn unset_dirty_descendants(&self) {}
    fn store_children_to_process(&self, _: isize) {}
    fn did_process_child(&self) -> isize {
        0
    }
    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        self.data().data.borrow_mut()
    }
    unsafe fn clear_data(&self) {
        *self.data().data.borrow_mut() = Default::default();
    }
    fn has_data(&self) -> bool {
        true
    }
    fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        Some(self.data().data.borrow())
    }
    fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        Some(self.data().data.borrow_mut())
    }
    fn skip_item_display_fixup(&self) -> bool {
        false
    }
    fn may_have_animations(&self) -> bool {
        false
    }
    fn has_animations(&self, _: &SharedStyleContext) -> bool {
        false
    }
    fn has_css_animations(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }
    fn has_css_transitions(&self, _: &SharedStyleContext, _: Option<PseudoElement>) -> bool {
        false
    }
    fn shadow_root(&self) -> Option<Self> {
        None
    }
    fn containing_shadow(&self) -> Option<Self> {
        None
    }
    fn lang_attr(&self) -> Option<AttrValue> {
        self.node().attr("lang").map(AttrValue::from)
    }
    fn match_element_lang(&self, override_lang: Option<Option<AttrValue>>, value: &Lang) -> bool {
        let mut node = Some(*self);
        while let Some(current) = node {
            if let Some(lang) = override_lang
                .as_ref()
                .and_then(|s| s.as_ref())
                .map(|s| s.as_ref())
                .or_else(|| current.node().attr("lang"))
            {
                return lang.eq_ignore_ascii_case(value)
                    || lang
                        .get(..value.len())
                        .is_some_and(|s| s.eq_ignore_ascii_case(value))
                        && lang.as_bytes().get(value.len()) == Some(&b'-');
            }
            if override_lang.is_some() {
                return false;
            }
            node = current.parent_node();
        }
        false
    }
    fn is_html_document_body_element(&self) -> bool {
        self.node().tag == "body"
    }
    fn synthesize_presentational_hints_for_legacy_attributes<
        V: selectors::sink::Push<ApplicableDeclarationBlock>,
    >(
        &self,
        _: VisitedHandlingMode,
        hints: &mut V,
    ) {
        if let Some(block) = &self.data().hints {
            hints.push(ApplicableDeclarationBlock::from_declarations(
                block.clone(),
                style::rule_tree::CascadeLevel::new(style::rule_tree::CascadeOrigin::PresHints),
                style::stylesheets::layer_rule::LayerOrder::root(),
            ));
        }
    }
    fn local_name(&self) -> &BorrowedLocalName {
        &self.data().local_name.0
    }
    fn namespace(&self) -> &BorrowedNamespace {
        &self.document.namespace.0
    }
    fn query_container_size(
        &self,
        _: &style::values::computed::Display,
    ) -> euclid::default::Size2D<Option<app_units::Au>> {
        euclid::default::Size2D::new(None, None)
    }
    fn has_selector_flags(&self, _: ElementSelectorFlags) -> bool {
        false
    }
    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        ElementSelectorFlags::empty()
    }
    fn get_attr(&self, attr: &LocalName, namespace: &Namespace) -> Option<String> {
        if namespace.0.is_empty() {
            self.node().attr(&attr.0).map(str::to_owned)
        } else {
            None
        }
    }
}

#[derive(Debug)]
struct FontMetrics;
impl style::device::servo::FontMetricsProvider for FontMetrics {
    fn query_font_metrics(
        &self,
        _: bool,
        _: &style::properties::style_structs::Font,
        base: style::values::computed::CSSPixelLength,
        _: style::values::computed::font::QueryFontMetricsFlags,
    ) -> style::font_metrics::FontMetrics {
        style::font_metrics::FontMetrics {
            ascent: base * 0.8,
            ..Default::default()
        }
    }
    fn base_size_for_generic(
        &self,
        _: style::values::computed::font::GenericFontFamily,
    ) -> style::values::computed::Length {
        style::values::computed::Length::new(16.0)
    }
}

fn parsed_declarations(
    css: &str,
    url: &UrlExtraData,
    lock: &SharedRwLock,
    reporter: Option<&dyn style::error_reporting::ParseErrorReporter>,
) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
    if css.is_empty() {
        return None;
    }
    let normalized = normalize_css_declarations(css, true);
    Some(Arc::new(lock.wrap(
        style::properties::parse_style_attribute(
            &normalized,
            url,
            reporter,
            QuirksMode::NoQuirks,
            CssRuleType::Style,
        ),
    )))
}

fn presentation_hints(document: &Document, id: usize) -> String {
    let node = &document.nodes[id];
    let mut css = String::new();
    let mut add = |name: &str, value: &str| {
        css.push_str(name);
        css.push(':');
        css.push_str(value);
        css.push(';');
    };
    let dimension = |value: &str| -> Option<String> {
        let value = value.trim();
        let (number, unit) = value.strip_suffix('%').map_or((value, "px"), |n| (n, "%"));
        let parsed = number.parse::<f32>().ok()?;
        (parsed.is_finite() && parsed >= 0.0).then(|| format!("{parsed}{unit}"))
    };
    if matches!(
        node.tag.as_str(),
        "table" | "td" | "th" | "img" | "hr" | "col" | "colgroup"
    ) {
        for name in ["width", "height"] {
            if let Some(value) = node.attr(name).and_then(dimension) {
                add(name, &value);
            }
        }
    }
    if let Some(color) = node
        .attr("bgcolor")
        .filter(|s| s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '#'))
    {
        add("background-color", color);
    }
    if let Some(align @ ("left" | "center" | "right" | "justify")) = node.attr("align") {
        if node.tag == "table" && align == "center" {
            add("margin-left", "auto");
            add("margin-right", "auto");
        } else {
            add("text-align", align);
        }
    }
    if let Some(align @ ("top" | "middle" | "bottom" | "baseline")) = node.attr("valign") {
        add("vertical-align", align);
    }
    if node.has("nowrap") && matches!(node.tag.as_str(), "td" | "th") {
        add("white-space", "nowrap");
    }
    if node.tag == "table" {
        if let Some(space) = node.attr("cellspacing").and_then(dimension) {
            add("border-spacing", &space);
        }
    }
    if matches!(node.tag.as_str(), "td" | "th") {
        let mut parent = node.parent;
        for _ in 0..256 {
            let ancestor = &document.nodes[parent];
            if ancestor.tag == "table" {
                if let Some(padding) = ancestor.attr("cellpadding").and_then(dimension) {
                    add("padding", &padding);
                }
                if ancestor
                    .attr("border")
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_some_and(|n| n > 0)
                {
                    add("border", "1px solid #808080");
                }
                break;
            }
            if parent == 0 {
                break;
            }
            parent = ancestor.parent;
        }
    }
    if matches!(node.tag.as_str(), "table" | "img") {
        if let Some(border) = node.attr("border").and_then(|s| s.parse::<u32>().ok()) {
            add("border", &format!("{}px solid #808080", border.min(1024)));
        }
    }
    css
}

/// Parse one SVG presentation value before serializing it into a declaration.
/// Parsing the entire value prevents an attribute from injecting declarations,
/// `!important`, or another rule. No other SVG attributes become CSS here.
fn svg_presentation_hints(
    node: &Node,
    id: usize,
    url: &UrlExtraData,
    source: &str,
    diagnostics: &mut StyleDiagnostics,
) -> String {
    use style::parser::Parse;
    use style::values::specified::{Color as SpecifiedColor, SVGOpacity, SVGPaint};
    use style_traits::ToCss;
    let context = style::parser::ParserContext::new(
        Origin::Author,
        url,
        None,
        style_traits::ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    );
    let mut css = String::new();
    for name in [
        "color",
        "fill",
        "stroke",
        "fill-opacity",
        "stroke-opacity",
        "display",
        "visibility",
    ] {
        let Some(value) = node.attr(name) else {
            continue;
        };
        let canonical = if value.len() <= 1024 {
            let value = value.trim();
            let keyword = ["inherit", "initial", "unset", "revert", "revert-layer"]
                .into_iter()
                .find(|keyword| value.eq_ignore_ascii_case(keyword));
            if let Some(keyword) = keyword {
                Some(keyword.to_owned())
            } else {
                let mut input = cssparser::ParserInput::new(value);
                let mut parser = cssparser::Parser::new(&mut input);
                if name == "display" {
                    parser
                        .parse_entirely(|parser| {
                            style::properties::longhands::display::parse(&context, parser)
                        })
                        .ok()
                        .map(|value| value.to_css_string())
                } else if name == "visibility" {
                    parser
                        .parse_entirely(|parser| {
                            style::properties::longhands::visibility::parse(&context, parser)
                        })
                        .ok()
                        .map(|value| value.to_css_string())
                } else if name == "color" {
                    parser
                        .parse_entirely(|parser| SpecifiedColor::parse(&context, parser))
                        .ok()
                        .map(|value| value.to_css_string())
                } else if matches!(name, "fill" | "stroke") {
                    parser
                        .parse_entirely(|parser| SVGPaint::parse(&context, parser))
                        .ok()
                        .map(|value| value.to_css_string())
                } else {
                    parser
                        .parse_entirely(|parser| SVGOpacity::parse(&context, parser))
                        .ok()
                        .map(|value| value.to_css_string())
                }
            }
        } else {
            None
        };
        if let Some(value) = canonical {
            css.push_str(name);
            css.push(':');
            css.push_str(&value);
            css.push(';');
        } else {
            diagnostics.record(
                "css-parse",
                format_args!("Invalid or oversized SVG presentation attribute {name}; ignored"),
                format_args!("{source} [SVG presentation attribute]"),
                Some(id),
                None,
            );
        }
    }
    css
}

fn is_supported_text_overflow_decl_value(raw_val: &str) -> bool {
    // Strip block comments cleanly before checking keyword/var syntax.
    let mut cleaned = String::with_capacity(raw_val.len());
    let bytes = raw_val.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx..].starts_with(b"/*") {
            idx = bytes[idx + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .map_or(bytes.len(), |p| idx + 2 + p + 2);
            cleaned.push(' ');
        } else {
            let ch = raw_val[idx..].chars().next().unwrap_or(' ');
            cleaned.push(ch);
            idx += ch.len_utf8();
        }
    }
    let mut trimmed = cleaned.trim();
    if trimmed.len() >= 10 {
        let lower_tail = trimmed[trimmed.len() - 9..].to_ascii_lowercase();
        if lower_tail == "important" {
            let before = trimmed[..trimmed.len() - 9].trim_end();
            if let Some(rest) = before.strip_suffix('!') {
                trimmed = rest.trim();
            }
        }
    }
    trimmed.eq_ignore_ascii_case("clip")
        || trimmed.eq_ignore_ascii_case("ellipsis")
        || trimmed.eq_ignore_ascii_case("inherit")
        || trimmed.eq_ignore_ascii_case("initial")
        || trimmed.eq_ignore_ascii_case("unset")
        || trimmed.eq_ignore_ascii_case("revert")
        || (trimmed.len() >= 5
            && trimmed[..4].eq_ignore_ascii_case("var(")
            && trimmed.ends_with(')'))
}

fn css_mentions_text_overflow(css: &str) -> bool {
    css.as_bytes()
        .windows(13)
        .any(|w| w.eq_ignore_ascii_case(b"text-overflow"))
}

fn css_mentions_mask(css: &str) -> bool {
    css.as_bytes()
        .windows(4)
        .any(|w| w.eq_ignore_ascii_case(b"mask"))
}

fn scan_decl_value_end(raw: &[u8], val_start: usize) -> usize {
    let mut vj = val_start;
    let mut paren_depth = 0usize;
    while vj < raw.len() {
        if raw[vj..].starts_with(b"/*") {
            vj = raw[vj + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .map_or(raw.len(), |p| vj + 2 + p + 2);
            continue;
        }
        let vb = raw[vj];
        if vb == b'\'' || vb == b'"' {
            let q = vb;
            vj += 1;
            while vj < raw.len() {
                let c = raw[vj];
                vj += 1;
                if c == q {
                    break;
                }
                if c == b'\\' && vj < raw.len() {
                    vj += 1;
                }
            }
            continue;
        }
        if vb == b'(' {
            paren_depth += 1;
        } else if vb == b')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if paren_depth == 0 && (vb == b';' || vb == b'}') {
            break;
        }
        vj += 1;
    }
    vj
}

/// Alias legacy vendor-prefixed declaration property names dropped by modern
/// Stylo (`-moz-border-radius*`, `-moz-box-sizing`, `-webkit-box-sizing`,
/// `-moz-opacity`, `-moz-box-shadow`) and Stylo-disabled `text-overflow`
/// (`13` bytes -> `--mg-txt-ovfl`, `13` bytes) into space-padded names of the
/// exact same byte length so 1-based line and column offsets stay identical.
fn normalize_legacy_vendor_css(css: &str) -> std::borrow::Cow<'_, str> {
    normalize_css_declarations(css, false)
}

fn normalize_css_declarations(css: &str, in_declaration_list: bool) -> std::borrow::Cow<'_, str> {
    let raw = css.as_bytes();
    let has_candidate = raw
        .windows(5)
        .any(|w| w.eq_ignore_ascii_case(b"-moz-") || w.eq_ignore_ascii_case(b"-webk"))
        || css_mentions_text_overflow(css)
        || css_mentions_mask(css);
    if !has_candidate {
        return std::borrow::Cow::Borrowed(css);
    }
    let mut out: Option<Vec<u8>> = None;
    let mut i = 0usize;
    let mut brace_depth = usize::from(in_declaration_list);
    let mut expect_prop = in_declaration_list;
    while i < raw.len() {
        if raw[i..].starts_with(b"/*") {
            i = raw[i + 2..]
                .windows(2)
                .position(|w| w == b"*/")
                .map_or(raw.len(), |p| i + 2 + p + 2);
            continue;
        }
        let b = raw[i];
        if b == b'\'' || b == b'"' {
            expect_prop = false;
            let quote = b;
            i += 1;
            while i < raw.len() {
                let c = raw[i];
                i += 1;
                if c == quote {
                    break;
                }
                if c == b'\\' && i < raw.len() {
                    i += 1;
                }
            }
            continue;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'{' {
            brace_depth += 1;
            expect_prop = true;
            i += 1;
            continue;
        }
        if b == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
            expect_prop = brace_depth > 0;
            i += 1;
            continue;
        }
        if b == b';' {
            expect_prop = brace_depth > 0;
            i += 1;
            continue;
        }
        if brace_depth > 0 && expect_prop && (b == b'-' || b == b't' || b == b'T') {
            let start = i;
            while i < raw.len() && (raw[i].is_ascii_alphanumeric() || raw[i] == b'-') {
                i += 1;
            }
            let ident_end = i;
            let mut probe = i;
            loop {
                while probe < raw.len() && raw[probe].is_ascii_whitespace() {
                    probe += 1;
                }
                if probe < raw.len() && raw[probe..].starts_with(b"/*") {
                    probe = raw[probe + 2..]
                        .windows(2)
                        .position(|w| w == b"*/")
                        .map_or(raw.len(), |p| probe + 2 + p + 2);
                } else {
                    break;
                }
            }
            expect_prop = false;
            if probe < raw.len() && raw[probe] == b':' {
                let name = &css[start..ident_end];
                let replacement: Option<&[u8]> = if name.eq_ignore_ascii_case("-moz-border-radius")
                {
                    Some(b"     border-radius")
                } else if name.eq_ignore_ascii_case("-moz-border-radius-topleft") {
                    Some(b"    border-top-left-radius")
                } else if name.eq_ignore_ascii_case("-moz-border-radius-topright") {
                    Some(b"    border-top-right-radius")
                } else if name.eq_ignore_ascii_case("-moz-border-radius-bottomright") {
                    Some(b"    border-bottom-right-radius")
                } else if name.eq_ignore_ascii_case("-moz-border-radius-bottomleft") {
                    Some(b"    border-bottom-left-radius")
                } else if name.eq_ignore_ascii_case("-moz-box-sizing") {
                    Some(b"     box-sizing")
                } else if name.eq_ignore_ascii_case("-webkit-box-sizing") {
                    Some(b"        box-sizing")
                } else if name.eq_ignore_ascii_case("-moz-opacity") {
                    Some(b"     opacity")
                } else if name.eq_ignore_ascii_case("-moz-box-shadow") {
                    Some(b"     box-shadow")
                } else if name.eq_ignore_ascii_case("text-overflow")
                    || name.eq_ignore_ascii_case("-o-text-overflow")
                {
                    let val_start = probe + 1;
                    let vj = scan_decl_value_end(raw, val_start);
                    if is_supported_text_overflow_decl_value(&css[val_start..vj]) {
                        if name.len() == 16 {
                            Some(b"   --mg-txt-ovfl".as_slice())
                        } else {
                            Some(b"--mg-txt-ovfl".as_slice())
                        }
                    } else {
                        None
                    }
                } else if matches!(name.len(), 4 | 9 | 10 | 12 | 13 | 17 | 18 | 21) {
                    let val_start = probe + 1;
                    let vj = scan_decl_value_end(raw, val_start);
                    if css[val_start..vj].trim().is_empty() {
                        None
                    } else if name.eq_ignore_ascii_case("mask-image") {
                        Some(b"--mg-mskim".as_slice())
                    } else if name.eq_ignore_ascii_case("-webkit-mask-image") {
                        Some(b"        --mg-mskim".as_slice())
                    } else if name.eq_ignore_ascii_case("mask") {
                        Some(b"--mg".as_slice())
                    } else if name.eq_ignore_ascii_case("-webkit-mask") {
                        Some(b"        --mg".as_slice())
                    } else if name.eq_ignore_ascii_case("mask-size") {
                        Some(b"--mg-msks".as_slice())
                    } else if name.eq_ignore_ascii_case("-webkit-mask-size") {
                        Some(b"        --mg-msks".as_slice())
                    } else if name.eq_ignore_ascii_case("mask-position") {
                        Some(b"--mg-mskposit".as_slice())
                    } else if name.eq_ignore_ascii_case("-webkit-mask-position") {
                        Some(b"        --mg-mskposit".as_slice())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(rep) = replacement {
                    debug_assert_eq!(rep.len(), ident_end - start);
                    let buf = out.get_or_insert_with(|| raw.to_vec());
                    buf[start..ident_end].copy_from_slice(rep);
                }
            }
            continue;
        }
        expect_prop = false;
        i += css[i..].chars().next().map_or(1, char::len_utf8);
    }
    match out {
        Some(bytes) => {
            std::borrow::Cow::Owned(String::from_utf8(bytes).unwrap_or_else(|_| css.to_owned()))
        }
        None => std::borrow::Cow::Borrowed(css),
    }
}

/// Compute a fresh static style snapshot with no network or script capability.
/// Stylesheets must be supplied in document order, including inline style blocks.
/// CSS imports, animations, pseudo-element boxes and visited history are absent.
pub fn compute_styles(
    document: &Document,
    sheets: &[StylesheetSource],
    viewport: (f32, f32),
) -> Result<Vec<ComputedStyle>, String> {
    compute_styles_with_diagnostics(document, sheets, viewport, &mut StyleDiagnostics::default())
}

/// Compute the same style snapshot, retaining bounded parser and unsupported-
/// renderer diagnostics. Errors leave any already-collected entries available.
pub fn compute_styles_with_diagnostics(
    document: &Document,
    sheets: &[StylesheetSource],
    viewport: (f32, f32),
    diagnostics: &mut StyleDiagnostics,
) -> Result<Vec<ComputedStyle>, String> {
    static PREFERENCES: std::sync::Once = std::sync::Once::new();
    PREFERENCES.call_once(|| {
        stylo_static_prefs::set_pref!("layout.grid.enabled", true);
    });
    let diagnostics = RefCell::new(diagnostics);
    if document.nodes.is_empty() || document.nodes.len() > 50_000 {
        return Err("style DOM node bound exceeded".into());
    }
    if !viewport.0.is_finite() || !viewport.1.is_finite() || viewport.0 <= 0.0 || viewport.1 <= 0.0
    {
        return Err("invalid style viewport".into());
    }
    let sheet_bytes = sheets.iter().try_fold(0usize, |total, sheet| {
        total
            .checked_add(sheet.css.len())?
            .checked_add(sheet.media.len())
    });
    let max_sheet_bytes = if document.interactive_style_budget() {
        8 * 1024 * 1024
    } else {
        2 * 1024 * 1024
    };
    if sheets.len() > 64 || sheet_bytes.is_none_or(|bytes| bytes > max_sheet_bytes) {
        return Err("stylesheet input bound exceeded".into());
    }
    if document
        .nodes
        .iter()
        .any(|node| node.parent >= document.nodes.len())
    {
        return Err("invalid style DOM parent".into());
    }
    // The parser/worker normally validate topology. Check the adapter boundary too.
    let mut order = Vec::with_capacity(document.nodes.len());
    let mut seen = vec![false; document.nodes.len()];
    let mut svg_contexts = vec![false; document.nodes.len()];
    let mut pending = vec![(0usize, 0usize)];
    while let Some((id, depth)) = pending.pop() {
        if id >= seen.len() || seen[id] || depth > 256 {
            return Err("invalid style DOM topology".into());
        }
        seen[id] = true;
        // Same bounded foreign-content boundary as the HTML parser. Children
        // of foreignObject return to HTML, until a nested svg starts anew.
        svg_contexts[id] = document.nodes[id].tag == "svg"
            || id != 0
                && svg_contexts[document.nodes[id].parent]
                && document.nodes[document.nodes[id].parent].tag != "foreignobject";
        order.push(id);
        for &child in document.nodes[id].children.iter().rev() {
            if child >= document.nodes.len() || document.nodes[child].parent != id {
                return Err("invalid style DOM parent".into());
            }
            pending.push((child, depth + 1));
        }
    }
    let base = url::Url::parse(&document.base_url)
        .unwrap_or_else(|_| url::Url::parse("about:blank").unwrap());
    let url_data = UrlExtraData::from(base);
    let lock = SharedRwLock::new();
    let nodes = document
        .nodes
        .iter()
        .enumerate()
        .map(|(id, node)| NodeStyleData {
            local_name: node.tag.as_str().into(),
            id: node.attr("id").map(AtomIdent::from),
            classes: node
                .attr("class")
                .unwrap_or("")
                .split_ascii_whitespace()
                .map(AtomIdent::from)
                .collect(),
            attr_names: node
                .attributes
                .iter()
                .map(|(name, _)| name.as_str().into())
                .collect(),
            inline: parsed_declarations(
                node.attr("style").unwrap_or(""),
                &url_data,
                &lock,
                Some(&CssReporter {
                    diagnostics: &diagnostics,
                    label: "style attribute",
                    node: Some(id),
                }),
            ),
            hints: {
                let mut hints = presentation_hints(document, id);
                if svg_contexts[id] {
                    hints.push_str(&svg_presentation_hints(
                        node,
                        id,
                        &url_data,
                        &document.base_url,
                        &mut diagnostics.borrow_mut(),
                    ));
                }
                parsed_declarations(&hints, &url_data, &lock, None)
            },
            data: ElementDataWrapper::default(),
        })
        .collect();
    let arena = StyleDocument {
        nodes_slice: &document.nodes,
        lock: lock.clone(),
        namespace: "http://www.w3.org/1999/xhtml".into(),
        nodes,
    };
    let initial = ComputedValues::initial_values_with_font_override(
        style::properties::style_structs::Font::initial_values(),
    );
    let pointer = style::servo::media_features::PointerCapabilities::FINE
        | style::servo::media_features::PointerCapabilities::HOVER;
    let device = Device::new(
        style::media_queries::MediaType::screen(),
        QuirksMode::NoQuirks,
        euclid::Size2D::new(viewport.0, viewport.1),
        euclid::Size2D::new(viewport.0, viewport.1),
        euclid::Scale::new(1.0),
        Box::new(FontMetrics),
        initial.clone(),
        style::queries::values::PrefersColorScheme::Light,
        pointer,
        pointer,
    );
    let mut stylist = Stylist::new(device, QuirksMode::NoQuirks);
    let sheet =
        |css: &str,
         base: UrlExtraData,
         origin: Origin,
         media_text: &str,
         reporter: Option<&dyn style::error_reporting::ParseErrorReporter>| {
            let mut context = style::parser::ParserContext::new(
                origin,
                &base,
                None,
                style_traits::ParsingMode::DEFAULT,
                QuirksMode::NoQuirks,
                Default::default(),
                reporter,
                None,
                Default::default(),
            );
            let mut input = cssparser::ParserInput::new(media_text);
            let media = style::media_queries::MediaList::parse(
                &mut context,
                &mut cssparser::Parser::new(&mut input),
            );
            DocumentStyleSheet(Arc::new(Stylesheet::from_str(
                css,
                base,
                origin,
                Arc::new(lock.wrap(media)),
                lock.clone(),
                None,
                reporter,
                QuirksMode::NoQuirks,
                AllowImportRules::No,
            )))
        };
    let uses_text_overflow = sheets.iter().any(|s| css_mentions_text_overflow(&s.css))
        || document
            .nodes
            .iter()
            .any(|n| n.attr("style").is_some_and(css_mentions_text_overflow));
    let uses_mask = sheets.iter().any(|s| css_mentions_mask(&s.css))
        || document
            .nodes
            .iter()
            .any(|n| n.attr("style").is_some_and(css_mentions_mask));
    let mut ua_buf = String::new();
    let ua_css = if uses_text_overflow || uses_mask {
        ua_buf.push_str(USER_AGENT_CSS);
        if uses_text_overflow {
            ua_buf.push_str("\n*{--mg-txt-ovfl:clip}");
        }
        if uses_mask {
            ua_buf.push_str("\n*{--mg:none;--mg-mskim:none;--mg-msks:auto;--mg-mskposit:0% 0%}");
        }
        ua_buf.as_str()
    } else {
        USER_AGENT_CSS
    };
    stylist.append_stylesheet(
        sheet(ua_css, url_data.clone(), Origin::UserAgent, "", None),
        &lock.read(),
    );
    for (index, source) in sheets.iter().enumerate() {
        let base = url::Url::parse(&source.base_url)
            .map(UrlExtraData::from)
            .unwrap_or_else(|_| url_data.clone());
        let label = format!("stylesheet {}", index + 1);
        let reporter = CssReporter {
            diagnostics: &diagnostics,
            label: &label,
            node: None,
        };
        let normalized_css = normalize_legacy_vendor_css(&source.css);
        stylist.append_stylesheet(
            sheet(
                &normalized_css,
                base,
                Origin::Author,
                &source.media,
                Some(&reporter),
            ),
            &lock.read(),
        );
    }
    let guard = lock.read();
    let guards = StylesheetGuards {
        author: &guard,
        ua_or_user: &guard,
    };
    stylist.flush(&guards);
    let mut values = vec![initial; document.nodes.len()];
    let mut output = vec![ComputedStyle::default(); document.nodes.len()];
    let mut snapshot_bytes = output.iter().map(snapshot_heap_bytes).sum::<usize>();
    let mut caches = SelectorCaches::default();
    let mut tree_caches = TreeCountingCaches::default();
    for id in order.into_iter().skip(1) {
        let node = DomNode {
            document: &arena,
            id,
        };
        let parent_id = node.node().parent;
        if !node.is_element() {
            values[id] = values[parent_id].clone();
            // Admit before cloning: inherited strings must not amplify a small
            // stylesheet by the number of text nodes in the arena.
            let inherited_bytes = inherited_heap_bytes(&output[parent_id]);
            admit_snapshot_bytes(
                &mut snapshot_bytes,
                snapshot_heap_bytes(&output[id]),
                inherited_bytes,
            )?;
            output[id] = output[parent_id].inherit_for_text();
            let actual_inherited_bytes = snapshot_heap_bytes(&output[id]);
            debug_assert!(actual_inherited_bytes <= inherited_bytes);
            snapshot_bytes -= inherited_bytes - actual_inherited_bytes;
            if parent_id != 0
                && document.nodes[parent_id].tag == "details"
                && !document.nodes[parent_id].has("open")
            {
                output[id].display = Display::None;
            }
            continue;
        }
        let mut declarations = ApplicableDeclarationList::new();
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        stylist.push_applicable_declarations(
            node,
            None,
            node.style_attribute(),
            None,
            Default::default(),
            RuleInclusion::All,
            &mut declarations,
            &mut context,
        );
        let rules = stylist.rule_tree().insert_ordered_rules_with_important(
            declarations
                .into_iter()
                .map(ApplicableDeclarationBlock::for_rule_tree),
            &guards,
        );
        let parent = (parent_id != 0).then_some(values[parent_id].as_ref());
        let computed = stylist.cascade_style_and_visited(
            Some(node),
            None,
            &CascadeInputs {
                rules: Some(rules),
                ..Default::default()
            },
            &guards,
            parent,
            parent,
            style::properties::cascade::FirstLineReparenting::No,
            &Default::default(),
            None,
            &mut Default::default(),
            &mut tree_caches,
        );
        let mut snapshot = renderer_style(&computed)?;
        resolve_sheet_mask_url(&mut snapshot.mask_image, document, sheets);
        snapshot.own_opacity = if svg_contexts[id] {
            1.0
        } else {
            computed.get_effects().opacity.clamp(0.0, 1.0)
        };
        snapshot.opacity = output[parent_id].opacity * snapshot.own_opacity;
        if svg_contexts[id] {
            for (property, paint) in [("fill", snapshot.svg_fill), ("stroke", snapshot.svg_stroke)]
            {
                if paint == SvgPaint::Unsupported {
                    diagnostics.borrow_mut().record("css-unsupported",
                        format_args!("SVG {property} paint server, context paint, or context opacity is not supported by the bounded inline icon renderer"),
                        format_args!("{}", document.base_url), Some(id), None);
                }
            }
        }
        let remaining = MAX_SNAPSHOT_HEAP_BYTES
            .checked_sub(snapshot_bytes)
            .and_then(|n| n.checked_add(snapshot_heap_bytes(&output[id])))
            .and_then(|n| n.checked_sub(snapshot_heap_bytes(&snapshot)))
            .ok_or("computed style snapshot bound exceeded")?;
        let formatting_grid_areas = {
            let mut anc = parent_id;
            let mut found = None;
            for _ in 0..=crate::document::MAX_DEPTH {
                if anc == 0 {
                    break;
                }
                if output[anc].display == Display::Contents {
                    anc = document.nodes[anc].parent;
                    continue;
                }
                found = active_grid_areas(Some(values[anc].as_ref()), output[anc].display);
                break;
            }
            found
        };
        snapshot.layout = renderer_layout(
            &computed,
            snapshot.display,
            formatting_grid_areas,
            remaining,
            id,
            &document.base_url,
            &mut diagnostics.borrow_mut(),
        )?;
        if document.scripting_enabled() && node.node().tag == "noscript" {
            // Inactive fallback content is not overrideable by author CSS.
            snapshot.display = Display::None;
        }
        if parent_id != 0
            && document.nodes[parent_id].tag == "details"
            && !document.nodes[parent_id].has("open")
        {
            let first_summary = document.nodes[parent_id]
                .children
                .iter()
                .copied()
                .find(|&c| document.nodes[c].tag == "summary");
            if Some(id) != first_summary {
                snapshot.display = Display::None;
            }
        }
        if !svg_contexts[id]
            && snapshot.display != Display::None
            && !matches!(
                node.node().tag.as_str(),
                "img" | "canvas" | "input" | "br" | "hr" | "svg" | "textarea" | "select"
            )
        {
            for pseudo_kind in [PseudoElement::Before, PseudoElement::After] {
                let mut pseudo_declarations = ApplicableDeclarationList::new();
                let mut pseudo_context = MatchingContext::new(
                    MatchingMode::ForStatelessPseudoElement,
                    None,
                    &mut caches,
                    QuirksMode::NoQuirks,
                    NeedsSelectorFlags::No,
                    MatchingForInvalidation::No,
                );
                stylist.push_applicable_declarations(
                    node,
                    Some(&pseudo_kind),
                    None,
                    None,
                    Default::default(),
                    RuleInclusion::All,
                    &mut pseudo_declarations,
                    &mut pseudo_context,
                );
                if pseudo_declarations.is_empty() {
                    continue;
                }
                let pseudo_rules = stylist.rule_tree().insert_ordered_rules_with_important(
                    pseudo_declarations
                        .into_iter()
                        .map(ApplicableDeclarationBlock::for_rule_tree),
                    &guards,
                );
                let pseudo_computed = stylist.cascade_style_and_visited(
                    Some(node),
                    Some(&pseudo_kind),
                    &CascadeInputs {
                        rules: Some(pseudo_rules),
                        ..Default::default()
                    },
                    &guards,
                    Some(computed.as_ref()),
                    Some(computed.as_ref()),
                    style::properties::cascade::FirstLineReparenting::No,
                    &Default::default(),
                    None,
                    &mut Default::default(),
                    &mut tree_caches,
                );
                let Some(pseudo_text) = pseudo_content_text(&pseudo_computed)? else {
                    continue;
                };
                let mut pseudo_snapshot = renderer_style(&pseudo_computed)?;
                resolve_sheet_mask_url(&mut pseudo_snapshot.mask_image, document, sheets);
                if pseudo_snapshot.display == Display::None {
                    continue;
                }
                pseudo_snapshot.own_opacity = pseudo_computed.get_effects().opacity.clamp(0.0, 1.0);
                pseudo_snapshot.opacity = snapshot.opacity * pseudo_snapshot.own_opacity;
                if matches!(pseudo_kind, PseudoElement::Before)
                    && node.node().children.is_empty()
                    && snapshot.background_images.is_empty()
                    && !pseudo_snapshot.background_images.is_empty()
                    && pseudo_snapshot.display == Display::Inline
                    && pseudo_text.is_empty()
                    && pseudo_snapshot.width == Length::Auto
                    && pseudo_snapshot.height == Length::Auto
                    && pseudo_snapshot.padding == [Length::Px(0.0); 4]
                    && pseudo_snapshot.border_width == [0.0; 4]
                {
                    snapshot.background_images =
                        std::mem::take(&mut pseudo_snapshot.background_images);
                    continue;
                }
                let pseudo_remaining = MAX_SNAPSHOT_HEAP_BYTES
                    .checked_sub(snapshot_bytes)
                    .and_then(|n| n.checked_add(snapshot_heap_bytes(&output[id])))
                    .and_then(|n| n.checked_sub(snapshot_heap_bytes(&snapshot)))
                    .and_then(|n| n.checked_sub(std::mem::size_of::<PseudoStyle>()))
                    .and_then(|n| n.checked_sub(snapshot_heap_bytes(&pseudo_snapshot)))
                    .and_then(|n| n.checked_sub(pseudo_text.capacity()))
                    .ok_or("computed style snapshot bound exceeded")?;
                let pseudo_grid_areas = if snapshot.display == Display::Contents {
                    formatting_grid_areas
                } else {
                    active_grid_areas(Some(computed.as_ref()), snapshot.display)
                };
                pseudo_snapshot.layout = renderer_layout(
                    &pseudo_computed,
                    pseudo_snapshot.display,
                    pseudo_grid_areas,
                    pseudo_remaining,
                    id,
                    &document.base_url,
                    &mut diagnostics.borrow_mut(),
                )?;
                report_unsupported(
                    &pseudo_computed,
                    &pseudo_snapshot,
                    id,
                    &document.base_url,
                    &mut diagnostics.borrow_mut(),
                );
                let boxed = Box::new(PseudoStyle {
                    style: pseudo_snapshot,
                    text: pseudo_text,
                });
                match pseudo_kind {
                    PseudoElement::Before => snapshot.before = Some(boxed),
                    PseudoElement::After => snapshot.after = Some(boxed),
                    _ => {}
                }
            }
        }
        report_unsupported(
            &computed,
            &snapshot,
            id,
            &document.base_url,
            &mut diagnostics.borrow_mut(),
        );
        admit_snapshot(&mut snapshot_bytes, &output[id], &snapshot)?;
        output[id] = snapshot;
        node.data().data.borrow_mut().styles.primary = Some(computed.clone());
        values[id] = computed;
    }
    Ok(output)
}

fn pseudo_content_text(values: &ComputedValues) -> Result<Option<String>, String> {
    use style::values::generics::counters::{Content, ContentItem};
    match &values.get_counters().content {
        Content::Normal | Content::None => Ok(None),
        Content::Items(items) => {
            let mut text = String::new();
            for item in items.items.iter().take(items.alt_start) {
                match item {
                    ContentItem::String(s) => {
                        if text.len().saturating_add(s.len()) > MAX_STYLE_URL_BYTES {
                            return Err("computed pseudo-element content bound exceeded".into());
                        }
                        text.push_str(s);
                    }
                    ContentItem::OpenQuote | ContentItem::CloseQuote => {
                        if text.len().saturating_add(1) > MAX_STYLE_URL_BYTES {
                            return Err("computed pseudo-element content bound exceeded".into());
                        }
                        text.push('"');
                    }
                    _ => {}
                }
            }
            Ok(Some(text))
        }
    }
}

#[cfg(feature = "modern")]
fn parse_dom_selector_list(selector: &str) -> Result<selectors::SelectorList<SelectorImpl>, ()> {
    if selector.trim().is_empty() || selector.len() > 8192 {
        return Err(());
    }
    let namespaces = style::stylesheets::Namespaces::default();
    let url_data = UrlExtraData::from(url::Url::parse("about:blank").unwrap());
    let parser = style::selector_parser::SelectorParser {
        stylesheet_origin: Origin::Author,
        namespaces: &namespaces,
        url_data: &url_data,
        for_supports_rule: false,
    };
    let mut input = cssparser::ParserInput::new(selector);
    let mut css_parser = cssparser::Parser::new(&mut input);
    selectors::SelectorList::parse(
        &parser,
        &mut css_parser,
        selectors::parser::ParseRelative::No,
    )
    .map_err(|_| ())
}

#[cfg(feature = "modern")]
fn is_simple_ident(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 256 {
        return false;
    }
    let valid_start = b[0].is_ascii_alphabetic()
        || b[0] == b'_'
        || (b[0] == b'-'
            && b.len() >= 2
            && (b[1].is_ascii_alphabetic() || b[1] == b'_' || b[1] == b'-'));
    valid_start
        && b.iter()
            .all(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[cfg(feature = "modern")]
fn try_simple_compound_selector<'a>(
    nodes: &[Node],
    root: usize,
    raw_selector: &'a str,
    first_only: bool,
) -> Option<Vec<usize>> {
    let s = raw_selector.trim();
    if s.is_empty() || s.len() > 512 {
        return None;
    }
    // Reject anything with combinators, commas, pseudo-classes, attributes, escapes, or namespaces.
    if s.bytes()
        .any(|b| !b.is_ascii_alphanumeric() && !matches!(b, b'-' | b'_' | b'#' | b'.'))
    {
        return None;
    }
    let mut tag_req: Option<&'a str> = None;
    let mut id_req: Option<&'a str> = None;
    let mut class_reqs: [&'a str; 4] = [""; 4];
    let mut class_count = 0usize;

    let bytes = s.as_bytes();
    let mut i = 0usize;
    if bytes[0] != b'#' && bytes[0] != b'.' {
        let start = 0;
        while i < bytes.len() && bytes[i] != b'#' && bytes[i] != b'.' {
            i += 1;
        }
        let t = &s[start..i];
        if !t.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || !t.as_bytes()[0].is_ascii_alphabetic()
        {
            return None;
        }
        tag_req = Some(t);
    }
    while i < bytes.len() {
        let kind = bytes[i];
        if kind != b'#' && kind != b'.' {
            return None;
        }
        i += 1;
        let start = i;
        while i < bytes.len() && bytes[i] != b'#' && bytes[i] != b'.' {
            i += 1;
        }
        let ident = &s[start..i];
        if !is_simple_ident(ident) {
            return None;
        }
        if kind == b'#' {
            if id_req.is_some() {
                return None;
            }
            id_req = Some(ident);
        } else {
            if class_count >= class_reqs.len() {
                return None;
            }
            class_reqs[class_count] = ident;
            class_count += 1;
        }
    }
    if tag_req.is_none() && id_req.is_none() && class_count == 0 {
        return None;
    }
    if root >= nodes.len() || nodes[root].tag == "template" {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut stack: Vec<usize> = nodes[root].children.iter().rev().copied().collect();
    let mut visited = 0usize;
    while let Some(id) = stack.pop() {
        visited += 1;
        if visited > crate::document::MAX_NODES {
            break;
        }
        let node = &nodes[id];
        if !node.tag.starts_with('#') {
            let tag_ok = tag_req.is_none_or(|t| node.tag.eq_ignore_ascii_case(t));
            let id_ok = id_req.is_none_or(|want| node.attr("id") == Some(want));
            let classes_ok = if class_count == 0 {
                true
            } else if let Some(cls_attr) = node.attr("class") {
                class_reqs[..class_count]
                    .iter()
                    .all(|&want| cls_attr.split_ascii_whitespace().any(|tok| tok == want))
            } else {
                false
            };
            if tag_ok && id_ok && classes_ok {
                out.push(id);
                if first_only {
                    return Some(out);
                }
            }
        }
        if node.tag != "template" {
            stack.extend(node.children.iter().rev().copied());
        }
    }
    Some(out)
}

#[cfg(feature = "modern")]
fn build_selector_arena(nodes_slice: &[Node]) -> StyleDocument<'_> {
    let nodes = nodes_slice
        .iter()
        .map(|node| {
            if node.tag.starts_with('#') {
                NodeStyleData {
                    local_name: Default::default(),
                    id: None,
                    classes: Vec::new(),
                    attr_names: Vec::new(),
                    inline: None,
                    hints: None,
                    data: ElementDataWrapper::default(),
                }
            } else {
                NodeStyleData {
                    local_name: node.tag.as_str().into(),
                    id: node.attr("id").map(AtomIdent::from),
                    classes: node
                        .attr("class")
                        .unwrap_or("")
                        .split_ascii_whitespace()
                        .map(AtomIdent::from)
                        .collect(),
                    attr_names: node
                        .attributes
                        .iter()
                        .map(|(name, _)| name.as_str().into())
                        .collect(),
                    inline: None,
                    hints: None,
                    data: ElementDataWrapper::default(),
                }
            }
        })
        .collect();
    StyleDocument {
        nodes_slice,
        lock: SharedRwLock::new(),
        namespace: "http://www.w3.org/1999/xhtml".into(),
        nodes,
    }
}

#[cfg(feature = "modern")]
fn element_matches_selector_list(
    element: DomNode<'_>,
    list: &selectors::SelectorList<SelectorImpl>,
    scope: Option<OpaqueElement>,
    caches: &mut SelectorCaches,
) -> bool {
    let mut context = MatchingContext::new(
        MatchingMode::Normal,
        None,
        caches,
        QuirksMode::NoQuirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    context.scope_element = scope;
    list.slice().iter().any(|selector| {
        selector.pseudo_element().is_none()
            && selectors::matching::matches_selector(selector, 0, None, &element, &mut context)
    })
}

#[cfg(feature = "modern")]
pub(crate) fn query_selector_dom(
    nodes: &[Node],
    root: usize,
    selector: &str,
    first_only: bool,
) -> Result<Vec<usize>, ()> {
    if let Some(fast) = try_simple_compound_selector(nodes, root, selector, first_only) {
        return Ok(fast);
    }
    let list = parse_dom_selector_list(selector)?;
    if root >= nodes.len() || nodes[root].tag == "template" {
        return Ok(Vec::new());
    }
    let arena = build_selector_arena(nodes);
    let scope = if root != 0 && !nodes[root].tag.starts_with('#') {
        Some(selectors::Element::opaque(&DomNode {
            document: &arena,
            id: root,
        }))
    } else if root == 0 {
        nodes[0]
            .children
            .iter()
            .copied()
            .find(|&c| !nodes[c].tag.starts_with('#'))
            .map(|c| {
                selectors::Element::opaque(&DomNode {
                    document: &arena,
                    id: c,
                })
            })
    } else {
        None
    };
    let mut caches = SelectorCaches::default();
    let mut out = Vec::new();
    let mut stack: Vec<usize> = nodes[root].children.iter().rev().copied().collect();
    let mut visited = 0usize;
    while let Some(id) = stack.pop() {
        visited += 1;
        if visited > crate::document::MAX_NODES {
            break;
        }
        let dom_node = DomNode {
            document: &arena,
            id,
        };
        if dom_node.is_element()
            && element_matches_selector_list(dom_node, &list, scope, &mut caches)
        {
            out.push(id);
            if first_only {
                return Ok(out);
            }
        }
        if nodes[id].tag != "template" {
            stack.extend(nodes[id].children.iter().rev().copied());
        }
    }
    Ok(out)
}

#[cfg(feature = "modern")]
pub(crate) fn matches_selector_dom(
    nodes: &[Node],
    element_id: usize,
    selector: &str,
) -> Result<bool, ()> {
    let list = parse_dom_selector_list(selector)?;
    if element_id == 0 || element_id >= nodes.len() || nodes[element_id].tag.starts_with('#') {
        return Ok(false);
    }
    let arena = build_selector_arena(nodes);
    let el = DomNode {
        document: &arena,
        id: element_id,
    };
    let mut caches = SelectorCaches::default();
    Ok(element_matches_selector_list(
        el,
        &list,
        Some(selectors::Element::opaque(&el)),
        &mut caches,
    ))
}

#[cfg(feature = "modern")]
pub(crate) fn closest_selector_dom(
    nodes: &[Node],
    element_id: usize,
    selector: &str,
) -> Result<Option<usize>, ()> {
    let list = parse_dom_selector_list(selector)?;
    if element_id == 0 || element_id >= nodes.len() || nodes[element_id].tag.starts_with('#') {
        return Ok(None);
    }
    let arena = build_selector_arena(nodes);
    let start = DomNode {
        document: &arena,
        id: element_id,
    };
    let scope = Some(selectors::Element::opaque(&start));
    let mut caches = SelectorCaches::default();
    let mut current = Some(start);
    for _ in 0..=crate::document::MAX_DEPTH {
        let Some(el) = current else {
            break;
        };
        if element_matches_selector_list(el, &list, scope, &mut caches) {
            return Ok(Some(el.id));
        }
        current = Element::parent_element(&el);
    }
    Ok(None)
}

#[cfg(feature = "modern")]
pub(crate) fn evaluate_media_query(
    query: &str,
    width: f32,
    height: f32,
    dpr: f32,
    prefers_dark: bool,
) -> (String, bool) {
    use style_traits::ToCss;
    let url_data = UrlExtraData::from(url::Url::parse("about:blank").unwrap());
    let mut context = style::parser::ParserContext::new(
        Origin::Author,
        &url_data,
        None,
        style_traits::ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    );
    let mut input = cssparser::ParserInput::new(query);
    let media = style::media_queries::MediaList::parse(
        &mut context,
        &mut cssparser::Parser::new(&mut input),
    );
    let serialized = media.to_css_string();
    let initial = ComputedValues::initial_values_with_font_override(
        style::properties::style_structs::Font::initial_values(),
    );
    let pointer = style::servo::media_features::PointerCapabilities::FINE
        | style::servo::media_features::PointerCapabilities::HOVER;
    let device = Device::new(
        style::media_queries::MediaType::screen(),
        QuirksMode::NoQuirks,
        euclid::Size2D::new(width.max(0.0), height.max(0.0)),
        euclid::Size2D::new(width.max(0.0), height.max(0.0)),
        euclid::Scale::new(dpr.max(0.1)),
        Box::new(FontMetrics),
        initial,
        if prefers_dark {
            style::queries::values::PrefersColorScheme::Dark
        } else {
            style::queries::values::PrefersColorScheme::Light
        },
        pointer,
        pointer,
    );
    let matched = media.evaluate(
        &device,
        QuirksMode::NoQuirks,
        &mut style::stylesheets::CustomMediaEvaluator::none(),
    );
    (serialized, matched)
}

#[cfg(feature = "modern")]
pub(crate) fn css_supports_declaration(property: &str, value: &str) -> bool {
    if property.trim().is_empty()
        || value.trim().is_empty()
        || property.len() > 1024
        || value.len() > 8192
    {
        return false;
    }
    let decl_str = format!("{}: {}", property.trim(), value.trim());
    style::stylesheets::supports_rule::Declaration(decl_str).eval(
        &style::parser::ParserContext::new(
            Origin::Author,
            &UrlExtraData::from(url::Url::parse("about:blank").unwrap()),
            Some(style::stylesheets::CssRuleType::Style),
            style_traits::ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            Default::default(),
            None,
            None,
            Default::default(),
        ),
    )
}

#[cfg(feature = "modern")]
pub(crate) fn css_supports_condition(condition: &str) -> bool {
    if condition.trim().is_empty() || condition.len() > 8192 {
        return false;
    }
    let mut input = cssparser::ParserInput::new(condition);
    let mut parser = cssparser::Parser::new(&mut input);
    let Ok(cond) =
        parser.parse_entirely(style::stylesheets::supports_rule::SupportsCondition::parse)
    else {
        return false;
    };
    cond.eval(&style::parser::ParserContext::new(
        Origin::Author,
        &UrlExtraData::from(url::Url::parse("about:blank").unwrap()),
        Some(style::stylesheets::CssRuleType::Style),
        style_traits::ParsingMode::DEFAULT,
        QuirksMode::NoQuirks,
        Default::default(),
        None,
        None,
        Default::default(),
    ))
}

fn report_unsupported(
    values: &ComputedValues,
    rendered: &ComputedStyle,
    node: usize,
    source: &str,
    diagnostics: &mut StyleDiagnostics,
) {
    use style::values::computed::Display as D;
    use style_traits::ToCss;
    let box_style = values.get_box();
    if rendered.display == Display::None {
        return;
    }
    let background = values.get_background();
    if rendered.background_clip == BackgroundBox::Unsupported {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS background-clip requires non-rectangular clipping, which is not implemented; background color omitted"),
            format_args!("{source}"), Some(node), None,
        );
    }
    if rendered.background_gradient.is_none()
        && background
            .background_image
            .0
            .iter()
            .any(|image| matches!(image, style::values::generics::image::Image::Gradient(_)))
        && !is_noop_transparent_gradient_over_other_layers(
            background,
            &values.get_inherited_text().color,
        )
    {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS gradient omitted: only one modern sRGB linear or radial gradient (2..=16 stops) with bounded lengths/percentages, scroll attachment, repeat/no-repeat tiling and rectangular clipping is implemented"),
            format_args!("{source}"), Some(node), None,
        );
    }
    if rendered.has_transform && rendered.transform.is_none() {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS transform omitted: only 2D axis-aligned translate and scale operations (up to 16 operations) are implemented"),
            format_args!("{source}"), Some(node), None,
        );
    }
    if !rendered.background_images.is_empty()
        && background
            .background_clip
            .0
            .iter()
            .any(|clip| background_box(*clip) != BackgroundBox::Border)
    {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS non-default background-clip on URL image layers is not implemented by the legacy image-background path"),
            format_args!("{source}"), Some(node), None,
        );
    }
    // Represented formatting contexts are implemented by the bounded layout
    // adapter. Rejected snapshots and actual layout failures report their own
    // specific reasons; admitting a display value alone is not success.
    if rendered.display == Display::Block && box_style.display != D::Block {
        diagnostics.record(
            "css-unsupported",
            format_args!(
                "CSS display: {} is not implemented; using block layout",
                box_style.display.to_css_string()
            ),
            format_args!("{source}"),
            Some(node),
            None,
        );
    }
}

fn linear_calc_scalar(node: &style::values::specified::calc::CalcNode) -> Option<f32> {
    use style::values::generics::calc::CalcNode;
    use style::values::specified::calc::Leaf;
    match node {
        CalcNode::Leaf(Leaf::Number(n)) => n.get().is_finite().then_some(n.get()),
        CalcNode::Negate(child) => linear_calc_scalar(child).map(|v| -v),
        CalcNode::Invert(child) => linear_calc_scalar(child)
            .map(|v| 1.0 / v)
            .filter(|v| v.is_finite()),
        CalcNode::Sum(children) => {
            let mut sum = 0.0f32;
            for child in children.iter() {
                sum += linear_calc_scalar(child)?;
            }
            sum.is_finite().then_some(sum)
        }
        CalcNode::Product(children) => {
            let mut prod = 1.0f32;
            for child in children.iter() {
                prod *= linear_calc_scalar(child)?;
            }
            prod.is_finite().then_some(prod)
        }
        _ => None,
    }
}

fn linear_calc_sum(
    node: &style::values::specified::calc::CalcNode,
    sign: f32,
) -> Option<(f32, f32)> {
    use style::values::generics::calc::{CalcNode, MinMaxOp};
    use style::values::specified::calc::Leaf;
    match node {
        CalcNode::Leaf(Leaf::Length(length)) => {
            let px = length.to_computed_pixel_length_without_context().ok()?;
            px.is_finite().then_some((sign * px, 0.0))
        }
        CalcNode::Leaf(Leaf::Percentage(percent)) => percent
            .get()
            .is_finite()
            .then_some((0.0, sign * percent.get())),
        CalcNode::Negate(child) => linear_calc_sum(child, -sign),
        CalcNode::Sum(children) => {
            let mut total_px = 0.0f32;
            let mut total_percent = 0.0f32;
            for child in children.iter() {
                let (px, percent) = linear_calc_sum(child, sign)?;
                total_px += px;
                total_percent += percent;
            }
            (total_px.is_finite() && total_percent.is_finite()).then_some((total_px, total_percent))
        }
        CalcNode::Product(children) => {
            let mut scale = sign;
            let mut term: Option<(f32, f32)> = None;
            for child in children.iter() {
                if let Some(factor) = linear_calc_scalar(child) {
                    scale *= factor;
                } else if term.is_none() {
                    term = Some(linear_calc_sum(child, 1.0)?);
                } else {
                    return None;
                }
            }
            let (px, percent) = term?;
            let (out_px, out_pct) = (px * scale, percent * scale);
            (out_px.is_finite() && out_pct.is_finite()).then_some((out_px, out_pct))
        }
        CalcNode::MinMax(children, op) => {
            let mut acc: Option<f32> = None;
            for child in children.iter() {
                let (px, percent) = linear_calc_sum(child, 1.0)?;
                if percent != 0.0 {
                    return None;
                }
                acc = Some(match (acc, op) {
                    (None, _) => px,
                    (Some(prev), MinMaxOp::Min) => prev.min(px),
                    (Some(prev), MinMaxOp::Max) => prev.max(px),
                });
            }
            acc.map(|px| (sign * px, 0.0))
        }
        CalcNode::Clamp { min, center, max } => {
            let (min_px, min_pct) = linear_calc_sum(min, 1.0)?;
            let (mid_px, mid_pct) = linear_calc_sum(center, 1.0)?;
            let (max_px, max_pct) = linear_calc_sum(max, 1.0)?;
            if min_pct != 0.0 || mid_pct != 0.0 || max_pct != 0.0 {
                return None;
            }
            Some((sign * mid_px.min(max_px).max(min_px), 0.0))
        }
        _ => None,
    }
}

fn linear_calc_length(
    value: &style::values::computed::LengthPercentage,
    non_negative: bool,
) -> Option<Length> {
    use style::values::generics::calc::{CalcNode, MinMaxOp};
    use style::values::{computed::ToComputedValue, specified::LengthPercentage as Specified};
    let Specified::Calc(calc) = Specified::from_computed_value(value) else {
        return None;
    };
    let non_negative = non_negative
        || matches!(
            calc.0.clamping_mode,
            style_traits::values::specified::AllowedNumericType::NonNegative
        );
    if let Some((px, percent)) = linear_calc_sum(&calc.0.node, 1.0) {
        return Some(Length::Calc {
            px,
            percent,
            bound_px: 0.0,
            op: CalcOp::Linear,
            non_negative,
        });
    }
    if let CalcNode::MinMax(children, op) = &calc.0.node {
        let mut pure_px: Option<f32> = None;
        let mut linear_term: Option<(f32, f32)> = None;
        for child in children.iter() {
            let (px, percent) = linear_calc_sum(child, 1.0)?;
            if percent == 0.0 {
                pure_px = Some(match (pure_px, op) {
                    (None, _) => px,
                    (Some(prev), MinMaxOp::Min) => prev.min(px),
                    (Some(prev), MinMaxOp::Max) => prev.max(px),
                });
            } else if linear_term.is_none() {
                linear_term = Some((px, percent));
            } else {
                return None;
            }
        }
        if let (Some(bound_px), Some((px, percent))) = (pure_px, linear_term) {
            return Some(Length::Calc {
                px,
                percent,
                bound_px,
                op: match op {
                    MinMaxOp::Min => CalcOp::MinPx,
                    MinMaxOp::Max => CalcOp::MaxPx,
                },
                non_negative,
            });
        }
    }
    None
}

fn plain_length(value: &style::values::computed::LengthPercentage) -> Length {
    if let Some(px) = value.to_length() {
        Length::Px(px.px())
    } else if let Some(percent) = value.to_percentage() {
        Length::Percent(percent.0)
    }
    // Mixed calculations and intrinsic/anchor sizing need a richer layout
    // contract. They are outside this desktop block/table slice.
    else {
        Length::Auto
    }
}

fn box_length(value: &style::values::computed::LengthPercentage, non_negative: bool) -> Length {
    if let Some(px) = value.to_length() {
        Length::Px(px.px())
    } else if let Some(percent) = value.to_percentage() {
        Length::Percent(percent.0)
    } else if let Some(calc) = linear_calc_length(value, non_negative) {
        calc
    } else {
        Length::Auto
    }
}

#[derive(Clone, Copy, Debug)]
enum MappingFailure {
    Unsupported(&'static str),
    OuterCalc(Length, &'static str),
    Limit(&'static str),
}
type Mapping<T> = Result<T, MappingFailure>;

fn layout_length(
    value: &style::values::computed::LengthPercentage,
    non_negative: bool,
) -> Mapping<Length> {
    if let Some(px) = value.to_length() {
        let v = px.px();
        v.is_finite()
            .then_some(Length::Px(v))
            .ok_or(MappingFailure::Limit("non-finite computed layout length"))
    } else if let Some(percent) = value.to_percentage() {
        let v = percent.0;
        v.is_finite()
            .then_some(Length::Percent(v))
            .ok_or(MappingFailure::Limit("non-finite computed layout length"))
    } else if let Some(
        calc @ Length::Calc {
            op: CalcOp::Linear, ..
        },
    ) = linear_calc_length(value, non_negative)
    {
        Ok(calc)
    } else {
        Err(MappingFailure::Unsupported("mixed calc lengths"))
    }
}
fn outer_layout_length(
    value: &style::values::computed::LengthPercentage,
    non_negative: bool,
) -> Mapping<Length> {
    if let Some(px) = value.to_length() {
        let v = px.px();
        v.is_finite()
            .then_some(Length::Px(v))
            .ok_or(MappingFailure::Limit("non-finite computed layout length"))
    } else if let Some(percent) = value.to_percentage() {
        let v = percent.0;
        v.is_finite()
            .then_some(Length::Percent(v))
            .ok_or(MappingFailure::Limit("non-finite computed layout length"))
    } else if let Some(calc) = linear_calc_length(value, non_negative) {
        if matches!(
            calc,
            Length::Calc {
                op: CalcOp::Linear,
                ..
            }
        ) {
            Ok(calc)
        } else {
            Err(MappingFailure::OuterCalc(calc, "mixed calc lengths"))
        }
    } else {
        Err(MappingFailure::Unsupported("mixed calc lengths"))
    }
}
fn flex_basis_size(value: &style::values::computed::Size) -> Mapping<Length> {
    use style::values::computed::Size;
    match value {
        Size::Auto => Ok(Length::Auto),
        Size::LengthPercentage(v) => layout_length(&v.0, true),
        _ => Err(MappingFailure::Unsupported("intrinsic or anchor sizing")),
    }
}
fn layout_size(value: &style::values::computed::Size) -> Mapping<Length> {
    use style::values::computed::Size;
    match value {
        Size::Auto => Ok(Length::Auto),
        Size::LengthPercentage(v) => outer_layout_length(&v.0, true),
        _ => Err(MappingFailure::Unsupported("intrinsic or anchor sizing")),
    }
}
fn layout_inset(value: &style::values::computed::Inset) -> Mapping<Length> {
    use style::values::computed::Inset;
    match value {
        Inset::Auto => Ok(Length::Auto),
        Inset::LengthPercentage(v) => outer_layout_length(v, false),
        _ => Err(MappingFailure::Unsupported("anchor positioning")),
    }
}
fn layout_gap(
    value: &style::values::computed::length::NonNegativeLengthPercentageOrNormal,
) -> Mapping<Length> {
    use style::values::generics::length::LengthPercentageOrNormal;
    match value {
        LengthPercentageOrNormal::Normal => Ok(Length::Px(0.)),
        LengthPercentageOrNormal::LengthPercentage(v) => layout_length(&v.0, true),
    }
}
fn alignment(flags: style::values::specified::align::AlignFlags) -> Alignment {
    use style::values::specified::align::AlignFlags as F;
    let keyword = match flags.value() {
        F::AUTO => AlignKeyword::Auto,
        F::NORMAL => AlignKeyword::Normal,
        F::START => AlignKeyword::Start,
        F::END => AlignKeyword::End,
        F::FLEX_START => AlignKeyword::FlexStart,
        F::FLEX_END => AlignKeyword::FlexEnd,
        F::CENTER => AlignKeyword::Center,
        F::LEFT => AlignKeyword::Left,
        F::RIGHT => AlignKeyword::Right,
        F::BASELINE => AlignKeyword::Baseline,
        F::LAST_BASELINE => AlignKeyword::LastBaseline,
        F::STRETCH => AlignKeyword::Stretch,
        F::SELF_START => AlignKeyword::SelfStart,
        F::SELF_END => AlignKeyword::SelfEnd,
        F::SPACE_BETWEEN => AlignKeyword::SpaceBetween,
        F::SPACE_AROUND => AlignKeyword::SpaceAround,
        F::SPACE_EVENLY => AlignKeyword::SpaceEvenly,
        _ => AlignKeyword::AnchorCenter,
    };
    Alignment {
        keyword,
        safety: if flags.contains(F::SAFE) {
            AlignSafety::Safe
        } else if flags.contains(F::UNSAFE) {
            AlignSafety::Unsafe
        } else {
            AlignSafety::Default
        },
        legacy: flags.contains(F::LEGACY),
    }
}
fn layout_overflow(value: style::values::specified::box_::Overflow) -> Overflow {
    use style::values::specified::box_::Overflow as O;
    match value {
        O::Visible => Overflow::Visible,
        O::Hidden => Overflow::Hidden,
        O::Clip => Overflow::Clip,
        O::Auto => Overflow::Auto,
        O::Scroll => Overflow::Scroll,
    }
}
#[derive(Clone, Copy)]
enum GridAxis {
    Row,
    Column,
}
#[derive(Clone, Copy)]
enum GridSide {
    Start,
    End,
}
fn active_grid_areas(
    parent_values: Option<&ComputedValues>,
    parent_display: Display,
) -> Option<&style::values::specified::position::TemplateAreas> {
    use style::values::computed::GridTemplateAreas;
    if !matches!(parent_display, Display::Grid | Display::InlineGrid) {
        return None;
    }
    match &parent_values?.get_position().grid_template_areas {
        GridTemplateAreas::Areas(areas) => Some(&areas.0),
        GridTemplateAreas::None => None,
    }
}
fn grid_line(
    value: &style::values::computed::GridLine,
    axis: GridAxis,
    side: GridSide,
    parent_areas: Option<&style::values::specified::position::TemplateAreas>,
) -> Mapping<GridLine> {
    if !value.ident.0.is_empty() {
        let ident_str: &str = value.ident.0.as_ref();
        if ident_str.len() > MAX_FONT_NAME_BYTES {
            return Err(MappingFailure::Limit(
                "computed grid line name bound exceeded",
            ));
        }
        if value.line_num == 0
            && !value.is_span
            && let Some(areas) = parent_areas
        {
            if let Some(area) = areas.areas.iter().find(|area| area.name == value.ident.0) {
                let range = match axis {
                    GridAxis::Row => &area.rows,
                    GridAxis::Column => &area.columns,
                };
                return match side {
                    GridSide::Start => {
                        if range.start == 0 || range.start as usize > MAX_GRID_TRACKS {
                            Err(MappingFailure::Limit(
                                "computed grid line/span bound exceeded",
                            ))
                        } else {
                            Ok(GridLine::Line(range.start as i16))
                        }
                    }
                    GridSide::End => {
                        let span = range.end.saturating_sub(range.start);
                        if span == 0 || span as usize > MAX_GRID_TRACKS {
                            Err(MappingFailure::Limit(
                                "computed grid line/span bound exceeded",
                            ))
                        } else if range.end as usize <= MAX_GRID_TRACKS {
                            Ok(GridLine::Line(range.end as i16))
                        } else {
                            Ok(GridLine::Span(span as u16))
                        }
                    }
                };
            }
            if let Some((stem, is_start)) = ident_str
                .strip_suffix("-start")
                .map(|s| (s, true))
                .or_else(|| ident_str.strip_suffix("-end").map(|s| (s, false)))
                && let Some(area) = areas.areas.iter().find(|area| area.name.as_ref() == stem)
            {
                let range = match axis {
                    GridAxis::Row => &area.rows,
                    GridAxis::Column => &area.columns,
                };
                let line = if is_start { range.start } else { range.end };
                if line == 0 || line as usize > MAX_GRID_TRACKS {
                    return Err(MappingFailure::Limit(
                        "computed grid line/span bound exceeded",
                    ));
                }
                return Ok(GridLine::Line(line as i16));
            }
        }
        let n = value.line_num;
        if value.is_span {
            let span = if n == 0 { 1 } else { n };
            if span <= 0 || span as usize > MAX_GRID_TRACKS {
                return Err(MappingFailure::Limit(
                    "computed grid line/span bound exceeded",
                ));
            }
            return Ok(GridLine::NamedSpan(ident_str.to_owned(), span as u16));
        }
        if n.unsigned_abs() as usize > MAX_GRID_TRACKS {
            return Err(MappingFailure::Limit(
                "computed grid line/span bound exceeded",
            ));
        }
        return Ok(GridLine::NamedLine(ident_str.to_owned(), n as i16));
    }
    if value.is_auto() {
        return Ok(GridLine::Auto);
    }
    let n = value.line_num;
    if n == 0 || n.unsigned_abs() as usize > MAX_GRID_TRACKS || value.is_span && n < 0 {
        return Err(MappingFailure::Limit(
            "computed grid line/span bound exceeded",
        ));
    }
    Ok(if value.is_span {
        GridLine::Span(n as u16)
    } else {
        GridLine::Line(n as i16)
    })
}
fn track_breadth(value: &style::values::computed::TrackBreadth) -> Mapping<TrackBreadth> {
    use style::values::generics::grid::TrackBreadth as B;
    Ok(match value {
        B::Auto => TrackBreadth::Auto,
        B::MinContent => TrackBreadth::MinContent,
        B::MaxContent => TrackBreadth::MaxContent,
        B::Breadth(v) => TrackBreadth::Length(layout_length(v, true)?),
        B::Flex(v) if v.0.is_finite() && v.0 >= 0. => TrackBreadth::Fr(v.0),
        B::Flex(_) => return Err(MappingFailure::Limit("non-finite computed grid fraction")),
    })
}
fn track_size(value: &style::values::computed::TrackSize) -> Mapping<TrackSize> {
    use style::values::generics::grid::{TrackBreadth as B, TrackSize as T};
    Ok(match value {
        T::Breadth(v) => TrackSize::Breadth(track_breadth(v)?),
        T::Minmax(min, max) => TrackSize::MinMax(track_breadth(min)?, track_breadth(max)?),
        T::FitContent(B::Breadth(v)) => TrackSize::FitContent(layout_length(v, true)?),
        T::FitContent(_) => return Err(MappingFailure::Unsupported("fit-content grid tracks")),
    })
}
const MAX_LINE_NAMES_PER_LINE: usize = 16;

fn append_custom_idents(
    dest: &mut Vec<String>,
    names: &[style::values::CustomIdent],
) -> Mapping<()> {
    for ident in names {
        let s: &str = ident.0.as_ref();
        if s.len() > MAX_FONT_NAME_BYTES {
            return Err(MappingFailure::Limit(
                "computed grid line name bound exceeded",
            ));
        }
        if dest.len() >= MAX_LINE_NAMES_PER_LINE {
            return Err(MappingFailure::Limit(
                "computed grid line name list bound exceeded",
            ));
        }
        dest.push(s.to_owned());
    }
    Ok(())
}

type ParsedGridTemplate = (Vec<TrackSize>, Vec<Vec<String>>, Option<GridAutoRepeat>);

fn parse_grid_template(
    value: &style::values::computed::GridTemplateComponent,
    remaining: usize,
) -> Mapping<ParsedGridTemplate> {
    use style::values::generics::grid::{
        GridTemplateComponent as G, RepeatCount, TrackListValue as V,
    };
    let tracks = match value {
        G::None => return Ok((Vec::new(), Vec::new(), None)),
        G::TrackList(tracks) => tracks,
        _ => return Err(MappingFailure::Unsupported("subgrid or masonry")),
    };
    let mut single_count = 0usize;
    let mut has_auto_repeat = false;
    let mut has_any_names = tracks.line_names.iter().any(|names| !names.is_empty());
    let mut est_bytes = 0usize;
    for names in tracks.line_names.iter() {
        if names.len() > MAX_LINE_NAMES_PER_LINE {
            return Err(MappingFailure::Limit(
                "computed grid line name list bound exceeded",
            ));
        }
        for ident in names.iter() {
            let len = ident.0.as_ref().len();
            if len > MAX_FONT_NAME_BYTES {
                return Err(MappingFailure::Limit(
                    "computed grid line name bound exceeded",
                ));
            }
            est_bytes = est_bytes.saturating_add(std::mem::size_of::<String>() + len);
        }
    }
    for value in tracks.values.iter() {
        match value {
            V::TrackSize(_) => {
                single_count = single_count
                    .checked_add(1)
                    .filter(|n| *n <= MAX_GRID_TRACKS)
                    .ok_or(MappingFailure::Limit("computed grid track bound exceeded"))?;
            }
            V::TrackRepeat(repeat) => {
                if repeat.line_names.iter().any(|names| !names.is_empty()) {
                    has_any_names = true;
                }
                for names in repeat.line_names.iter() {
                    if names.len() > MAX_LINE_NAMES_PER_LINE {
                        return Err(MappingFailure::Limit(
                            "computed grid line name list bound exceeded",
                        ));
                    }
                    for ident in names.iter() {
                        if ident.0.as_ref().len() > MAX_FONT_NAME_BYTES {
                            return Err(MappingFailure::Limit(
                                "computed grid line name bound exceeded",
                            ));
                        }
                    }
                }
                match repeat.count {
                    RepeatCount::Number(n) => {
                        if n <= 0 {
                            return Err(MappingFailure::Limit("invalid computed grid repeat"));
                        }
                        let amount = (n as usize)
                            .checked_mul(repeat.track_sizes.len())
                            .ok_or(MappingFailure::Limit("computed grid track bound exceeded"))?;
                        single_count = single_count
                            .checked_add(amount)
                            .filter(|c| *c <= MAX_GRID_TRACKS)
                            .ok_or(MappingFailure::Limit("computed grid track bound exceeded"))?;
                        let rep_name_bytes: usize = repeat
                            .line_names
                            .iter()
                            .flat_map(|set| set.iter())
                            .map(|id| std::mem::size_of::<String>() + id.0.as_ref().len())
                            .sum();
                        est_bytes =
                            est_bytes.saturating_add(rep_name_bytes.saturating_mul(n as usize));
                    }
                    RepeatCount::AutoFill | RepeatCount::AutoFit => {
                        if has_auto_repeat {
                            return Err(MappingFailure::Limit("multiple auto-repeat grid clauses"));
                        }
                        if repeat.track_sizes.is_empty()
                            || repeat.track_sizes.len() > MAX_GRID_TRACKS
                        {
                            return Err(MappingFailure::Limit(
                                "computed grid track bound exceeded",
                            ));
                        }
                        has_auto_repeat = true;
                        let rep_name_bytes: usize = repeat
                            .line_names
                            .iter()
                            .flat_map(|set| set.iter())
                            .map(|id| std::mem::size_of::<String>() + id.0.as_ref().len())
                            .sum();
                        est_bytes = est_bytes
                            .saturating_add(
                                repeat.track_sizes.len() * std::mem::size_of::<TrackSize>(),
                            )
                            .saturating_add(
                                repeat.line_names.len() * std::mem::size_of::<Vec<String>>(),
                            )
                            .saturating_add(rep_name_bytes);
                    }
                }
            }
        }
    }
    let component_count = single_count + usize::from(has_auto_repeat);
    if has_any_names {
        est_bytes =
            est_bytes.saturating_add((component_count + 1) * std::mem::size_of::<Vec<String>>());
    }
    est_bytes = est_bytes.saturating_add(single_count * std::mem::size_of::<TrackSize>());
    if est_bytes > remaining {
        return Err(MappingFailure::Limit(
            "computed style snapshot bound exceeded",
        ));
    }

    let mut out_tracks = Vec::with_capacity(single_count);
    let mut out_lines = if has_any_names {
        let mut v = Vec::with_capacity(component_count + 1);
        v.push(Vec::new());
        v
    } else {
        Vec::new()
    };
    let mut auto_repeat = None;

    for (idx, item) in tracks.values.iter().enumerate() {
        if has_any_names && let Some(names) = tracks.line_names.get(idx) {
            append_custom_idents(out_lines.last_mut().unwrap(), names)?;
        }
        match item {
            V::TrackSize(ts) => {
                out_tracks.push(track_size(ts)?);
                if has_any_names {
                    out_lines.push(Vec::new());
                }
            }
            V::TrackRepeat(repeat) => match repeat.count {
                RepeatCount::Number(n) => {
                    for _ in 0..n {
                        if has_any_names && let Some(first_names) = repeat.line_names.first() {
                            append_custom_idents(out_lines.last_mut().unwrap(), first_names)?;
                        }
                        for (t_idx, ts) in repeat.track_sizes.iter().enumerate() {
                            out_tracks.push(track_size(ts)?);
                            if has_any_names {
                                let mut next_set = Vec::new();
                                if let Some(after_names) = repeat.line_names.get(t_idx + 1) {
                                    append_custom_idents(&mut next_set, after_names)?;
                                }
                                out_lines.push(next_set);
                            }
                        }
                    }
                }
                RepeatCount::AutoFill | RepeatCount::AutoFit => {
                    let insert_index = out_tracks.len();
                    let mode = match repeat.count {
                        RepeatCount::AutoFill => GridAutoRepeatMode::AutoFill,
                        RepeatCount::AutoFit => GridAutoRepeatMode::AutoFit,
                        RepeatCount::Number(_) => unreachable!(),
                    };
                    let mut ar_tracks = Vec::with_capacity(repeat.track_sizes.len());
                    for ts in repeat.track_sizes.iter() {
                        ar_tracks.push(track_size(ts)?);
                    }
                    let ar_lines = if repeat.line_names.iter().any(|n| !n.is_empty()) {
                        let mut sets = Vec::with_capacity(ar_tracks.len() + 1);
                        for i in 0..=ar_tracks.len() {
                            let mut set = Vec::new();
                            if let Some(names) = repeat.line_names.get(i) {
                                append_custom_idents(&mut set, names)?;
                            }
                            sets.push(set);
                        }
                        sets
                    } else {
                        Vec::new()
                    };
                    auto_repeat = Some(GridAutoRepeat {
                        insert_index,
                        mode,
                        tracks: ar_tracks,
                        line_names: ar_lines,
                    });
                    if has_any_names {
                        out_lines.push(Vec::new());
                    }
                }
            },
        }
    }
    if has_any_names && let Some(trailing) = tracks.line_names.get(tracks.values.len()) {
        append_custom_idents(out_lines.last_mut().unwrap(), trailing)?;
    }
    Ok((out_tracks, out_lines, auto_repeat))
}

fn grid_style(
    position: &style::properties::style_structs::Position,
    remaining: usize,
) -> Mapping<Box<GridStyle>> {
    use style::values::computed::{GridAutoFlow as F, GridTemplateAreas};
    let areas_opt = match &position.grid_template_areas {
        GridTemplateAreas::None => None,
        GridTemplateAreas::Areas(areas) => {
            let r = areas.0.strings.len();
            let c = areas.0.width as usize;
            if r > MAX_GRID_TRACKS || c > MAX_GRID_TRACKS {
                return Err(MappingFailure::Limit("computed grid track bound exceeded"));
            }
            Some(&areas.0)
        }
    };
    let (area_rows, area_columns) =
        areas_opt.map_or((0, 0), |a| (a.strings.len(), a.width as usize));
    let auto_rows = position.grid_auto_rows.0.len();
    let auto_columns = position.grid_auto_columns.0.len();
    if auto_rows > MAX_GRID_TRACKS || auto_columns > MAX_GRID_TRACKS {
        return Err(MappingFailure::Limit(
            "computed implicit grid track bound exceeded",
        ));
    }
    let base_bytes = (auto_rows + auto_columns)
        .checked_mul(std::mem::size_of::<TrackSize>())
        .and_then(|n| n.checked_add(std::mem::size_of::<GridStyle>()))
        .filter(|n| *n <= remaining)
        .ok_or(MappingFailure::Limit(
            "computed style snapshot bound exceeded",
        ))?;
    let rem_after_base = remaining - base_bytes;
    let (mut template_rows, mut row_line_names, row_auto_repeat) =
        parse_grid_template(&position.grid_template_rows, rem_after_base)?;
    let row_used = template_rows.capacity() * std::mem::size_of::<TrackSize>()
        + line_names_heap_bytes(&row_line_names)
        + auto_repeat_heap_bytes(&row_auto_repeat);
    let rem_after_rows = rem_after_base
        .checked_sub(row_used)
        .ok_or(MappingFailure::Limit(
            "computed style snapshot bound exceeded",
        ))?;
    let (mut template_columns, mut col_line_names, col_auto_repeat) =
        parse_grid_template(&position.grid_template_columns, rem_after_rows)?;

    let implicit =
        |tracks: &style::values::computed::ImplicitGridTracks| -> Mapping<Vec<TrackSize>> {
            let mut output = Vec::with_capacity(tracks.0.len());
            for track in tracks.0.iter() {
                output.push(track_size(track)?);
            }
            Ok(output)
        };
    let flow = position.grid_auto_flow;
    let auto_flow = match (flow.contains(F::COLUMN), flow.contains(F::DENSE)) {
        (false, false) => GridAutoFlow::Row,
        (false, true) => GridAutoFlow::RowDense,
        (true, false) => GridAutoFlow::Column,
        (true, true) => GridAutoFlow::ColumnDense,
    };
    let auto_rows_vec = implicit(&position.grid_auto_rows)?;
    let auto_columns_vec = implicit(&position.grid_auto_columns)?;

    if row_auto_repeat.is_none() {
        let rows = template_rows.len();
        let explicit_rows = rows.max(area_rows);
        while template_rows.len() < explicit_rows {
            let idx = template_rows.len() - rows;
            let track = if auto_rows_vec.is_empty() {
                TrackSize::Breadth(TrackBreadth::Auto)
            } else {
                auto_rows_vec[idx % auto_rows_vec.len()]
            };
            template_rows.push(track);
            if !row_line_names.is_empty() {
                row_line_names.push(Vec::new());
            }
        }
        if let Some(areas) = areas_opt
            && !areas.areas.is_empty()
            && !template_rows.is_empty()
        {
            if row_line_names.is_empty() {
                row_line_names.resize_with(template_rows.len() + 1, Vec::new);
            }
            for area in areas.areas.iter() {
                let s_idx = (area.rows.start as usize).saturating_sub(1);
                let e_idx = (area.rows.end as usize).saturating_sub(1);
                if s_idx < row_line_names.len()
                    && row_line_names[s_idx].len() < MAX_LINE_NAMES_PER_LINE
                {
                    row_line_names[s_idx].push(format!("{}-start", area.name.as_ref()));
                }
                if e_idx < row_line_names.len()
                    && row_line_names[e_idx].len() < MAX_LINE_NAMES_PER_LINE
                {
                    row_line_names[e_idx].push(format!("{}-end", area.name.as_ref()));
                }
            }
        }
    }
    if col_auto_repeat.is_none() {
        let columns = template_columns.len();
        let explicit_columns = columns.max(area_columns);
        while template_columns.len() < explicit_columns {
            let idx = template_columns.len() - columns;
            let track = if auto_columns_vec.is_empty() {
                TrackSize::Breadth(TrackBreadth::Auto)
            } else {
                auto_columns_vec[idx % auto_columns_vec.len()]
            };
            template_columns.push(track);
            if !col_line_names.is_empty() {
                col_line_names.push(Vec::new());
            }
        }
        if let Some(areas) = areas_opt
            && !areas.areas.is_empty()
            && !template_columns.is_empty()
        {
            if col_line_names.is_empty() {
                col_line_names.resize_with(template_columns.len() + 1, Vec::new);
            }
            for area in areas.areas.iter() {
                let s_idx = (area.columns.start as usize).saturating_sub(1);
                let e_idx = (area.columns.end as usize).saturating_sub(1);
                if s_idx < col_line_names.len()
                    && col_line_names[s_idx].len() < MAX_LINE_NAMES_PER_LINE
                {
                    col_line_names[s_idx].push(format!("{}-start", area.name.as_ref()));
                }
                if e_idx < col_line_names.len()
                    && col_line_names[e_idx].len() < MAX_LINE_NAMES_PER_LINE
                {
                    col_line_names[e_idx].push(format!("{}-end", area.name.as_ref()));
                }
            }
        }
    }

    let grid = Box::new(GridStyle {
        template_rows,
        template_columns,
        row_line_names,
        col_line_names,
        row_auto_repeat,
        col_auto_repeat,
        auto_rows: auto_rows_vec,
        auto_columns: auto_columns_vec,
        auto_flow,
    });
    let total_grid_bytes = std::mem::size_of::<GridStyle>()
        + [
            &grid.template_rows,
            &grid.template_columns,
            &grid.auto_rows,
            &grid.auto_columns,
        ]
        .iter()
        .map(|tracks| tracks.capacity() * std::mem::size_of::<TrackSize>())
        .sum::<usize>()
        + line_names_heap_bytes(&grid.row_line_names)
        + line_names_heap_bytes(&grid.col_line_names)
        + auto_repeat_heap_bytes(&grid.row_auto_repeat)
        + auto_repeat_heap_bytes(&grid.col_auto_repeat);
    if total_grid_bytes > remaining {
        return Err(MappingFailure::Limit(
            "computed style snapshot bound exceeded",
        ));
    }
    Ok(grid)
}

struct LayoutReporter<'a> {
    diagnostics: &'a mut StyleDiagnostics,
    node: usize,
    source: &'a str,
    unsupported: bool,
    non_outer_calc_unsupported: bool,
}
impl LayoutReporter<'_> {
    fn convert<T>(&mut self, property: &str, value: Mapping<T>, fallback: T) -> Result<T, String> {
        match value {
            Ok(value) => Ok(value),
            Err(MappingFailure::Limit(message)) => Err(message.into()),
            Err(MappingFailure::Unsupported(message) | MappingFailure::OuterCalc(_, message)) => {
                self.unsupported = true;
                self.non_outer_calc_unsupported = true;
                self.diagnostics.record("css-unsupported",
                    format_args!("CSS {property}: {message} are not represented by the layout snapshot; retaining fallback"),
                    format_args!("{}", self.source), Some(self.node), None);
                Ok(fallback)
            }
        }
    }

    fn convert_length(
        &mut self,
        property: &str,
        value: Mapping<Length>,
        fallback: Length,
    ) -> Result<Length, String> {
        match value {
            Ok(value) => Ok(value),
            Err(MappingFailure::Limit(message)) => Err(message.into()),
            Err(MappingFailure::OuterCalc(value, message)) => {
                self.unsupported = true;
                self.diagnostics.record("css-unsupported",
                    format_args!("CSS {property}: {message} are not represented by the layout snapshot; retaining fallback"),
                    format_args!("{}", self.source), Some(self.node), None);
                Ok(value)
            }
            Err(MappingFailure::Unsupported(message)) => {
                self.unsupported = true;
                self.non_outer_calc_unsupported = true;
                self.diagnostics.record("css-unsupported",
                    format_args!("CSS {property}: {message} are not represented by the layout snapshot; retaining fallback"),
                    format_args!("{}", self.source), Some(self.node), None);
                Ok(fallback)
            }
        }
    }
}

fn renderer_layout(
    values: &ComputedValues,
    display: Display,
    parent_areas: Option<&style::values::specified::position::TemplateAreas>,
    remaining: usize,
    node: usize,
    source: &str,
    diagnostics: &mut StyleDiagnostics,
) -> Result<LayoutStyle, String> {
    use style::properties::longhands::{
        box_sizing, contain, direction, flex_direction, flex_wrap, writing_mode,
    };
    use style::values::computed::Display as D;
    use style::values::generics::{
        box_::PositionProperty,
        flex::FlexBasis as B,
        position::{PreferredRatio, ZIndex},
    };
    use style::values::specified::box_::{Clear as StyloClear, Float as StyloFloat};
    let p = values.get_position();
    let b = values.get_box();
    let inherited = values.get_inherited_box();
    let is_rtl = inherited.direction == direction::computed_value::T::Rtl;
    let position = match b.position {
        PositionProperty::Static => Position::Static,
        PositionProperty::Relative => Position::Relative,
        PositionProperty::Absolute => Position::Absolute,
        PositionProperty::Fixed => Position::Fixed,
        PositionProperty::Sticky => Position::Sticky,
    };
    let float = if matches!(position, Position::Absolute | Position::Fixed) {
        FloatSide::None
    } else {
        match b.float {
            StyloFloat::None => FloatSide::None,
            StyloFloat::Left => FloatSide::Left,
            StyloFloat::Right => FloatSide::Right,
            StyloFloat::InlineStart => {
                if is_rtl {
                    FloatSide::Right
                } else {
                    FloatSide::Left
                }
            }
            StyloFloat::InlineEnd => {
                if is_rtl {
                    FloatSide::Left
                } else {
                    FloatSide::Right
                }
            }
        }
    };
    let clear = match b.clear {
        StyloClear::None => ClearSide::None,
        StyloClear::Left => ClearSide::Left,
        StyloClear::Right => ClearSide::Right,
        StyloClear::Both => ClearSide::Both,
        StyloClear::InlineStart => {
            if is_rtl {
                ClearSide::Right
            } else {
                ClearSide::Left
            }
        }
        StyloClear::InlineEnd => {
            if is_rtl {
                ClearSide::Left
            } else {
                ClearSide::Right
            }
        }
    };
    let mut report = LayoutReporter {
        diagnostics,
        node,
        source,
        unsupported: false,
        non_outer_calc_unsupported: false,
    };
    let aspect_ratio = match &p.aspect_ratio.ratio {
        PreferredRatio::None => None,
        PreferredRatio::Ratio(r) => {
            let w = r.0.0;
            let h = r.1.0;
            if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
                let val = w / h;
                (val.is_finite() && val > 0.0).then_some(val)
            } else {
                None
            }
        }
    };
    let original_was_inline_level = matches!(
        b.original_display,
        D::Inline | D::InlineBlock | D::InlineFlex | D::InlineGrid | D::InlineTable
    );
    let mut output = LayoutStyle {
        box_sizing: match p.box_sizing {
            box_sizing::computed_value::T::ContentBox => BoxSizing::ContentBox,
            box_sizing::computed_value::T::BorderBox => BoxSizing::BorderBox,
        },
        aspect_ratio,
        aspect_ratio_auto: p.aspect_ratio.auto,
        original_was_inline_level,
        float,
        clear,
        position,
        overflow: [layout_overflow(b.overflow_x), layout_overflow(b.overflow_y)],
        z_index: match p.z_index {
            ZIndex::Auto => None,
            ZIndex::Integer(n) => Some(n),
        },
        order: p.order,
        flex_direction: match p.flex_direction {
            flex_direction::computed_value::T::Row => FlexDirection::Row,
            flex_direction::computed_value::T::RowReverse => FlexDirection::RowReverse,
            flex_direction::computed_value::T::Column => FlexDirection::Column,
            flex_direction::computed_value::T::ColumnReverse => FlexDirection::ColumnReverse,
        },
        flex_wrap: match p.flex_wrap {
            flex_wrap::computed_value::T::Nowrap => FlexWrap::NoWrap,
            flex_wrap::computed_value::T::Wrap => FlexWrap::Wrap,
            flex_wrap::computed_value::T::WrapReverse => FlexWrap::WrapReverse,
        },
        flex_grow: p.flex_grow.0,
        flex_shrink: p.flex_shrink.0,
        flex_basis_intrinsic: match &p.flex_basis {
            B::Size(style::values::computed::Size::MinContent) => Some(IntrinsicSize::MinContent),
            B::Size(style::values::computed::Size::MaxContent) => Some(IntrinsicSize::MaxContent),
            B::Size(style::values::computed::Size::FitContent) => Some(IntrinsicSize::FitContent),
            _ => None,
        },
        flex_basis: report.convert(
            "flex-basis",
            match &p.flex_basis {
                B::Content => Ok(FlexBasis::Content),
                B::Size(
                    style::values::computed::Size::MinContent
                    | style::values::computed::Size::MaxContent
                    | style::values::computed::Size::FitContent,
                ) => Ok(FlexBasis::Auto),
                B::Size(value) => flex_basis_size(value).map(|v| {
                    if v == Length::Auto {
                        FlexBasis::Auto
                    } else {
                        FlexBasis::Length(v)
                    }
                }),
            },
            FlexBasis::Auto,
        )?,
        align_content: alignment(p.align_content.primary()),
        justify_content: alignment(p.justify_content.primary()),
        align_items: alignment(p.align_items.0),
        justify_items: alignment(p.justify_items.computed.0.0),
        align_self: alignment(p.align_self.0),
        justify_self: alignment(p.justify_self.0),
        ..LayoutStyle::default()
    };
    // Preserve a truthful fallback for layout-affecting state absent from the
    // renderer contract. This is not an exhaustive CSS support inventory.
    let inherited = values.get_inherited_box();
    for (name, differs, reason) in [
        (
            "writing-mode",
            inherited.writing_mode != writing_mode::get_initial_value(),
            "non-horizontal writing modes",
        ),
        (
            "contain",
            b.contain != contain::get_initial_value(),
            "CSS containment semantics",
        ),
    ] {
        if differs {
            report.convert(name, Err(MappingFailure::Unsupported(reason)), ())?;
        }
    }
    if !output.flex_grow.is_finite() || !output.flex_shrink.is_finite() {
        return Err("non-finite computed flex factor".into());
    }
    for (slot, (name, value)) in output.inset.iter_mut().zip([
        ("top", &p.top),
        ("right", &p.right),
        ("bottom", &p.bottom),
        ("left", &p.left),
    ]) {
        *slot = report.convert_length(name, layout_inset(value), Length::Auto)?;
    }
    for (slot, (name, value)) in output
        .gap
        .iter_mut()
        .zip([("row-gap", &p.row_gap), ("column-gap", &p.column_gap)])
    {
        *slot = report.convert(name, layout_gap(value), Length::Px(0.))?;
    }
    use style::values::computed::{MaxSize, Size};
    output.width_intrinsic = match p.width {
        Size::MinContent => Some(IntrinsicSize::MinContent),
        Size::MaxContent => Some(IntrinsicSize::MaxContent),
        Size::FitContent => Some(IntrinsicSize::FitContent),
        _ => {
            report.convert_length("width", layout_size(&p.width), Length::Auto)?;
            None
        }
    };
    output.width_fit_content = output.width_intrinsic == Some(IntrinsicSize::FitContent);
    output.width_max_content = output.width_intrinsic == Some(IntrinsicSize::MaxContent);
    output.height_intrinsic = match p.height {
        Size::MinContent => Some(IntrinsicSize::MinContent),
        Size::MaxContent => Some(IntrinsicSize::MaxContent),
        Size::FitContent => Some(IntrinsicSize::FitContent),
        _ => {
            report.convert_length("height", layout_size(&p.height), Length::Auto)?;
            None
        }
    };
    output.min_width_intrinsic = match p.min_width {
        Size::MinContent => Some(IntrinsicSize::MinContent),
        Size::MaxContent => Some(IntrinsicSize::MaxContent),
        Size::FitContent => Some(IntrinsicSize::FitContent),
        _ => {
            report.convert_length("min-width", layout_size(&p.min_width), Length::Auto)?;
            None
        }
    };
    output.min_height_intrinsic = match p.min_height {
        Size::MinContent => Some(IntrinsicSize::MinContent),
        Size::MaxContent => Some(IntrinsicSize::MaxContent),
        Size::FitContent => Some(IntrinsicSize::FitContent),
        _ => {
            report.convert_length("min-height", layout_size(&p.min_height), Length::Auto)?;
            None
        }
    };
    output.max_width_intrinsic = match p.max_width {
        MaxSize::MinContent => Some(IntrinsicSize::MinContent),
        MaxSize::MaxContent => Some(IntrinsicSize::MaxContent),
        MaxSize::FitContent => Some(IntrinsicSize::FitContent),
        _ => {
            report.convert_length(
                "max-width",
                match &p.max_width {
                    MaxSize::None => Ok(Length::Auto),
                    MaxSize::LengthPercentage(v) => outer_layout_length(&v.0, true),
                    _ => Err(MappingFailure::Unsupported("intrinsic or anchor sizing")),
                },
                Length::Auto,
            )?;
            None
        }
    };
    output.max_width_fit_content = output.max_width_intrinsic == Some(IntrinsicSize::FitContent);
    output.max_height_intrinsic = match p.max_height {
        MaxSize::MinContent => Some(IntrinsicSize::MinContent),
        MaxSize::MaxContent => Some(IntrinsicSize::MaxContent),
        MaxSize::FitContent => Some(IntrinsicSize::FitContent),
        _ => {
            report.convert_length(
                "max-height",
                match &p.max_height {
                    MaxSize::None => Ok(Length::Auto),
                    MaxSize::LengthPercentage(v) => outer_layout_length(&v.0, true),
                    _ => Err(MappingFailure::Unsupported("intrinsic or anchor sizing")),
                },
                Length::Auto,
            )?;
            None
        }
    };
    let margin = values.get_margin();
    for (name, value) in [
        ("margin-top", &margin.margin_top),
        ("margin-right", &margin.margin_right),
        ("margin-bottom", &margin.margin_bottom),
        ("margin-left", &margin.margin_left),
    ] {
        use style::values::computed::Margin;
        report.convert_length(
            name,
            match value {
                Margin::Auto => Ok(Length::Auto),
                Margin::LengthPercentage(v) => outer_layout_length(v, false),
                _ => Err(MappingFailure::Unsupported("anchor margin")),
            },
            Length::Auto,
        )?;
    }
    let padding = values.get_padding();
    for (name, value) in [
        ("padding-top", &padding.padding_top),
        ("padding-right", &padding.padding_right),
        ("padding-bottom", &padding.padding_bottom),
        ("padding-left", &padding.padding_left),
    ] {
        report.convert_length(name, outer_layout_length(&value.0, true), Length::Px(0.))?;
    }
    for (slot, (name, side, value)) in output.grid_row.iter_mut().zip([
        ("grid-row-start", GridSide::Start, &p.grid_row_start),
        ("grid-row-end", GridSide::End, &p.grid_row_end),
    ]) {
        *slot = report.convert(
            name,
            grid_line(value, GridAxis::Row, side, parent_areas),
            GridLine::Auto,
        )?;
    }
    for (slot, (name, side, value)) in output.grid_column.iter_mut().zip([
        ("grid-column-start", GridSide::Start, &p.grid_column_start),
        ("grid-column-end", GridSide::End, &p.grid_column_end),
    ]) {
        *slot = report.convert(
            name,
            grid_line(value, GridAxis::Column, side, parent_areas),
            GridLine::Auto,
        )?;
    }
    let placement_heap = output
        .grid_row
        .iter()
        .chain(output.grid_column.iter())
        .map(grid_line_heap_bytes)
        .sum::<usize>();
    let remaining = remaining
        .checked_sub(placement_heap)
        .ok_or_else(|| "computed style snapshot bound exceeded".to_owned())?;
    if matches!(display, Display::Grid | Display::InlineGrid) {
        output.grid = report.convert("grid-template", grid_style(p, remaining).map(Some), None)?;
    }
    output.unsupported = report.unsupported;
    output.outer_calc_only = report.unsupported && !report.non_outer_calc_unsupported;
    Ok(output)
}

fn size_length(value: &style::values::computed::Size) -> Length {
    match value {
        style::values::computed::Size::LengthPercentage(value) => box_length(&value.0, true),
        _ => Length::Auto,
    }
}

fn max_size_length(value: &style::values::computed::MaxSize) -> Length {
    match value {
        style::values::computed::MaxSize::LengthPercentage(value) => box_length(&value.0, true),
        _ => Length::Auto,
    }
}

fn margin_length(value: &style::values::computed::Margin) -> Length {
    match value {
        style::values::computed::Margin::LengthPercentage(value) => box_length(value, false),
        _ => Length::Auto,
    }
}

fn absolute_color(color: &style::color::AbsoluteColor) -> Color {
    let color = color.to_color_space(style::color::ColorSpace::Srgb);
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    Color {
        rgb: (byte(color.components.0) << 16)
            | (byte(color.components.1) << 8)
            | byte(color.components.2),
        alpha: color.alpha.clamp(0.0, 1.0),
    }
}

fn background_box(value: style::values::specified::background::BackgroundClip) -> BackgroundBox {
    use style::values::specified::background::BackgroundClip as B;
    match value {
        B::BorderBox => BackgroundBox::Border,
        B::PaddingBox => BackgroundBox::Padding,
        B::ContentBox => BackgroundBox::Content,
        _ => BackgroundBox::Unsupported,
    }
}

fn bounded_box_length(
    value: &style::values::computed::LengthPercentage,
    non_negative: bool,
) -> Option<Length> {
    let len = box_length(value, non_negative);
    match len {
        Length::Px(v) | Length::Percent(v) if v.is_finite() && v.abs() <= 1_000_000. => Some(len),
        Length::Calc {
            px,
            percent,
            bound_px,
            ..
        } if px.is_finite()
            && percent.is_finite()
            && bound_px.is_finite()
            && px.abs() <= 1_000_000.
            && percent.abs() <= 1_000_000.
            && bound_px.abs() <= 1_000_000. =>
        {
            Some(len)
        }
        _ => None,
    }
}

fn is_noop_transparent_gradient_over_other_layers(
    background: &style::properties::style_structs::Background,
    current_color: &style::color::AbsoluteColor,
) -> bool {
    use style::values::generics::image::{Gradient, GradientCompatMode, GradientItem, Image};
    let layers = background.background_image.0.as_ref();
    if layers.len() < 2 || !layers.iter().any(|l| !matches!(l, Image::Gradient(_))) {
        return false;
    }
    layers.iter().all(|layer| match layer {
        Image::Gradient(grad) => match &**grad {
            Gradient::Linear {
                color_interpolation_method,
                items,
                compat_mode,
                ..
            }
            | Gradient::Radial {
                color_interpolation_method,
                items,
                compat_mode,
                ..
            } => {
                *compat_mode == GradientCompatMode::Modern
                    && color_interpolation_method.space == style::color::ColorSpace::Srgb
                    && items.len() >= 2
                    && items.iter().all(|item| match item {
                        GradientItem::SimpleColorStop(c)
                        | GradientItem::ComplexColorStop { color: c, .. } => {
                            let resolved = absolute_color(&c.resolve_to_absolute(current_color));
                            resolved.alpha == 0.0
                        }
                        GradientItem::InterpolationHint(_) => true,
                    })
            }
            Gradient::Conic { .. } => false,
        },
        _ => true,
    })
}

/// Convert the explicitly implemented sRGB linear/radial background gradient
/// slice from Stylo's computed values (`2..=16` stops, optional midpoint hints).
fn renderer_gradient(values: &ComputedValues) -> Option<Box<BackgroundGradient>> {
    use style::properties::longhands::{background_attachment, background_origin};
    use style::values::computed::image::LineDirection;
    use style::values::generics::background::BackgroundSize;
    use style::values::generics::image::{
        Circle, Ellipse, EndingShape, Gradient, GradientCompatMode, GradientFlags, GradientItem,
        Image, ShapeExtent,
    };
    use style::values::generics::length::LengthPercentageOrAuto;
    use style::values::specified::background::BackgroundRepeatKeyword as Repeat;
    use style::values::specified::position::{
        HorizontalPositionKeyword as H, VerticalPositionKeyword as V,
    };
    let background = values.get_background();
    let [Image::Gradient(gradient)] = background.background_image.0.as_ref() else {
        return None;
    };
    let (kind, items, repeating) = match &**gradient {
        Gradient::Linear {
            direction,
            color_interpolation_method,
            items,
            flags,
            compat_mode,
        } => {
            if *compat_mode != GradientCompatMode::Modern
                || color_interpolation_method.space != style::color::ColorSpace::Srgb
            {
                return None;
            }
            let dir = match *direction {
                LineDirection::Angle(angle) => {
                    let radians = angle.radians();
                    if !radians.is_finite() {
                        return None;
                    }
                    GradientDirection::Angle(radians.rem_euclid(std::f32::consts::TAU))
                }
                LineDirection::Horizontal(H::Left) => {
                    GradientDirection::Angle(std::f32::consts::PI * 1.5)
                }
                LineDirection::Horizontal(H::Right) => {
                    GradientDirection::Angle(std::f32::consts::FRAC_PI_2)
                }
                LineDirection::Vertical(V::Top) => GradientDirection::Angle(0.),
                LineDirection::Vertical(V::Bottom) => {
                    GradientDirection::Angle(std::f32::consts::PI)
                }
                LineDirection::Corner(horizontal, vertical) => GradientDirection::Corner {
                    right: horizontal == H::Right,
                    bottom: vertical == V::Bottom,
                },
            };
            (
                GradientKind::Linear(dir),
                items,
                flags.contains(GradientFlags::REPEATING),
            )
        }
        Gradient::Radial {
            shape,
            position,
            color_interpolation_method,
            items,
            flags,
            compat_mode,
        } => {
            if *compat_mode != GradientCompatMode::Modern
                || color_interpolation_method.space != style::color::ColorSpace::Srgb
            {
                return None;
            }
            let map_extent = |ext: ShapeExtent| match ext {
                ShapeExtent::ClosestSide | ShapeExtent::Contain => RadialExtent::ClosestSide,
                ShapeExtent::FarthestSide => RadialExtent::FarthestSide,
                ShapeExtent::ClosestCorner => RadialExtent::ClosestCorner,
                ShapeExtent::FarthestCorner | ShapeExtent::Cover => RadialExtent::FarthestCorner,
            };
            let shape = match *shape {
                EndingShape::Circle(Circle::Radius(r)) => {
                    let px = r.0.px();
                    if !px.is_finite() || !(0.0..=1_000_000.0).contains(&px) {
                        return None;
                    }
                    RadialShape::CircleExplicit(Length::Px(px))
                }
                EndingShape::Circle(Circle::Extent(ext)) => {
                    RadialShape::CircleExtent(map_extent(ext))
                }
                EndingShape::Ellipse(Ellipse::Radii(ref rx, ref ry)) => {
                    RadialShape::EllipseExplicit(
                        bounded_box_length(&rx.0, true)?,
                        bounded_box_length(&ry.0, true)?,
                    )
                }
                EndingShape::Ellipse(Ellipse::Extent(ext)) => {
                    RadialShape::EllipseExtent(map_extent(ext))
                }
            };
            let center = [
                bounded_box_length(&position.horizontal, false)?,
                bounded_box_length(&position.vertical, false)?,
            ];
            (
                GradientKind::Radial { shape, center },
                items,
                flags.contains(GradientFlags::REPEATING),
            )
        }
        Gradient::Conic { .. } => return None,
    };

    let size = match background.background_size.0.first()? {
        BackgroundSize::ExplicitSize { width, height } => {
            let w = match width {
                LengthPercentageOrAuto::Auto => Length::Auto,
                LengthPercentageOrAuto::LengthPercentage(lp) => bounded_box_length(&lp.0, true)?,
            };
            let h = match height {
                LengthPercentageOrAuto::Auto => Length::Auto,
                LengthPercentageOrAuto::LengthPercentage(lp) => bounded_box_length(&lp.0, true)?,
            };
            [w, h]
        }
        BackgroundSize::Contain | BackgroundSize::Cover => {
            [Length::Percent(1.0), Length::Percent(1.0)]
        }
    };
    let position = [
        bounded_box_length(background.background_position_x.0.first()?, false)?,
        bounded_box_length(background.background_position_y.0.first()?, false)?,
    ];
    if *background.background_attachment.0.first()?
        != background_attachment::single_value::computed_value::T::Scroll
    {
        return None;
    }
    let clip = background_box(*background.background_clip.0.first()?);
    if clip == BackgroundBox::Unsupported {
        return None;
    }
    let origin = match background.background_origin.0.first()? {
        background_origin::single_value::computed_value::T::BorderBox => BackgroundBox::Border,
        background_origin::single_value::computed_value::T::PaddingBox => BackgroundBox::Padding,
        background_origin::single_value::computed_value::T::ContentBox => BackgroundBox::Content,
    };
    let repeat = background.background_repeat.0.first()?;
    let repeats = |value| match value {
        Repeat::Repeat => Some(true),
        Repeat::NoRepeat => Some(false),
        _ => None,
    };
    let repeat = [repeats(repeat.0)?, repeats(repeat.1)?];

    let text = values.get_inherited_text();
    let mut stops: Vec<GradientStop> = Vec::with_capacity(items.len().min(MAX_GRADIENT_STOPS + 1));
    for item in items.iter() {
        match item {
            GradientItem::SimpleColorStop(color) => {
                if stops.len() >= MAX_GRADIENT_STOPS {
                    return None;
                }
                let color = absolute_color(&color.resolve_to_absolute(&text.color));
                if !color.alpha.is_finite() {
                    return None;
                }
                stops.push(GradientStop {
                    color,
                    position: None,
                    hint_to_next: None,
                });
            }
            GradientItem::ComplexColorStop { color, position } => {
                if stops.len() >= MAX_GRADIENT_STOPS {
                    return None;
                }
                let pos = bounded_box_length(position, false)?;
                let color = absolute_color(&color.resolve_to_absolute(&text.color));
                if !color.alpha.is_finite() {
                    return None;
                }
                stops.push(GradientStop {
                    color,
                    position: Some(pos),
                    hint_to_next: None,
                });
            }
            GradientItem::InterpolationHint(pos) => {
                let prev = stops.last_mut()?;
                if prev.hint_to_next.is_some() {
                    return None;
                }
                prev.hint_to_next = Some(bounded_box_length(pos, false)?);
            }
        }
    }
    if !(2..=MAX_GRADIENT_STOPS).contains(&stops.len()) || stops.last()?.hint_to_next.is_some() {
        return None;
    }
    if stops[0].position.is_none() {
        stops[0].position = Some(Length::Percent(0.0));
    }
    if let Some(last) = stops.last_mut()
        && last.position.is_none()
    {
        last.position = Some(Length::Percent(1.0));
    }
    stops.shrink_to_fit();
    Some(Box::new(BackgroundGradient {
        kind,
        repeating,
        stops,
        origin,
        clip,
        repeat,
        size,
        position,
    }))
}

fn renderer_transform(
    box_style: &style::properties::style_structs::Box,
) -> (bool, Option<Box<TransformStyle>>) {
    use style::values::generics::transform::{Rotate, Scale, TransformOperation, Translate};
    let has_transform = !box_style.transform.0.is_empty()
        || !matches!(box_style.translate, Translate::None)
        || !matches!(box_style.rotate, Rotate::None)
        || !matches!(box_style.scale, Scale::None);
    if !has_transform {
        return (false, None);
    }
    let Some(origin_x) = bounded_box_length(&box_style.transform_origin.horizontal, false) else {
        return (true, None);
    };
    let Some(origin_y) = bounded_box_length(&box_style.transform_origin.vertical, false) else {
        return (true, None);
    };
    let origin_z = box_style.transform_origin.depth.px();
    if !origin_z.is_finite() || origin_z.abs() > 1e-5 {
        return (true, None);
    }

    let valid_scale = |v: f32| v.is_finite() && v.abs() <= 1_000_000.0;
    let angle_scale_op = |radians: f32| -> Result<Option<TransformOp2D>, ()> {
        if !radians.is_finite() {
            return Err(());
        }
        let (sin, cos) = radians.sin_cos();
        if (cos - 1.0).abs() <= 1e-5 && sin.abs() <= 1e-5 {
            Ok(None)
        } else if (cos + 1.0).abs() <= 1e-5 && sin.abs() <= 1e-5 {
            Ok(Some(TransformOp2D::Scale(-1.0, -1.0)))
        } else {
            Err(())
        }
    };

    let mut ops = Vec::new();
    let push_op = |ops: &mut Vec<TransformOp2D>, op: TransformOp2D| -> Result<(), ()> {
        if ops.len() >= MAX_TRANSFORM_OPS {
            return Err(());
        }
        ops.push(op);
        Ok(())
    };

    match &box_style.translate {
        Translate::None => {}
        Translate::Translate(tx, ty, tz) => {
            let z = tz.px();
            if !z.is_finite() || z.abs() > 1e-5 {
                return (true, None);
            }
            let (Some(x), Some(y)) = (bounded_box_length(tx, false), bounded_box_length(ty, false))
            else {
                return (true, None);
            };
            if push_op(&mut ops, TransformOp2D::Translate(x, y)).is_err() {
                return (true, None);
            }
        }
    }
    match &box_style.rotate {
        Rotate::None => {}
        Rotate::Rotate(angle) => match angle_scale_op(angle.radians()) {
            Ok(Some(op)) => {
                if push_op(&mut ops, op).is_err() {
                    return (true, None);
                }
            }
            Ok(None) => {}
            Err(()) => return (true, None),
        },
        Rotate::Rotate3D(x, y, z, angle) => {
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return (true, None);
            }
            if x.abs() <= 1e-5 && y.abs() <= 1e-5 && z.abs() > 1e-5 {
                let dir = if *z < 0.0 { -1.0 } else { 1.0 };
                match angle_scale_op(angle.radians() * dir) {
                    Ok(Some(op)) => {
                        if push_op(&mut ops, op).is_err() {
                            return (true, None);
                        }
                    }
                    Ok(None) => {}
                    Err(()) => return (true, None),
                }
            } else if angle.radians().abs() <= 1e-5 {
                // Zero-angle rotation around any finite axis is identity.
            } else {
                return (true, None);
            }
        }
    }
    match &box_style.scale {
        Scale::None => {}
        Scale::Scale(sx, sy, sz) => {
            if !sz.is_finite() || (*sz - 1.0).abs() > 1e-5 || !valid_scale(*sx) || !valid_scale(*sy)
            {
                return (true, None);
            }
            if push_op(&mut ops, TransformOp2D::Scale(*sx, *sy)).is_err() {
                return (true, None);
            }
        }
    }

    for op in box_style.transform.0.iter() {
        let mapped = match op {
            TransformOperation::Translate(tx, ty) => {
                let (Some(x), Some(y)) =
                    (bounded_box_length(tx, false), bounded_box_length(ty, false))
                else {
                    return (true, None);
                };
                Some(TransformOp2D::Translate(x, y))
            }
            TransformOperation::TranslateX(tx) => {
                let Some(x) = bounded_box_length(tx, false) else {
                    return (true, None);
                };
                Some(TransformOp2D::Translate(x, Length::Px(0.0)))
            }
            TransformOperation::TranslateY(ty) => {
                let Some(y) = bounded_box_length(ty, false) else {
                    return (true, None);
                };
                Some(TransformOp2D::Translate(Length::Px(0.0), y))
            }
            TransformOperation::TranslateZ(tz) => {
                let z = tz.px();
                if !z.is_finite() || z.abs() > 1e-5 {
                    return (true, None);
                }
                None
            }
            TransformOperation::Translate3D(tx, ty, tz) => {
                let z = tz.px();
                if !z.is_finite() || z.abs() > 1e-5 {
                    return (true, None);
                }
                let (Some(x), Some(y)) =
                    (bounded_box_length(tx, false), bounded_box_length(ty, false))
                else {
                    return (true, None);
                };
                Some(TransformOp2D::Translate(x, y))
            }
            TransformOperation::Scale(sx, sy) => {
                if !valid_scale(*sx) || !valid_scale(*sy) {
                    return (true, None);
                }
                Some(TransformOp2D::Scale(*sx, *sy))
            }
            TransformOperation::ScaleX(sx) => {
                if !valid_scale(*sx) {
                    return (true, None);
                }
                Some(TransformOp2D::Scale(*sx, 1.0))
            }
            TransformOperation::ScaleY(sy) => {
                if !valid_scale(*sy) {
                    return (true, None);
                }
                Some(TransformOp2D::Scale(1.0, *sy))
            }
            TransformOperation::ScaleZ(sz) => {
                if !sz.is_finite() || (*sz - 1.0).abs() > 1e-5 {
                    return (true, None);
                }
                None
            }
            TransformOperation::Scale3D(sx, sy, sz) => {
                if !sz.is_finite()
                    || (*sz - 1.0).abs() > 1e-5
                    || !valid_scale(*sx)
                    || !valid_scale(*sy)
                {
                    return (true, None);
                }
                Some(TransformOp2D::Scale(*sx, *sy))
            }
            TransformOperation::Matrix(m) => {
                if !valid_scale(m.a)
                    || !valid_scale(m.d)
                    || !valid_scale(m.e)
                    || !valid_scale(m.f)
                    || !m.b.is_finite()
                    || !m.c.is_finite()
                    || m.b.abs() > 1e-5
                    || m.c.abs() > 1e-5
                {
                    return (true, None);
                }
                Some(TransformOp2D::Matrix {
                    a: m.a,
                    d: m.d,
                    e: m.e,
                    f: m.f,
                })
            }
            TransformOperation::Matrix3D(m) => {
                let zeros = [
                    m.m12, m.m13, m.m14, m.m21, m.m23, m.m24, m.m31, m.m32, m.m34, m.m43,
                ];
                if zeros.iter().any(|v| !v.is_finite() || v.abs() > 1e-5)
                    || !m.m33.is_finite()
                    || (m.m33 - 1.0).abs() > 1e-5
                    || !m.m44.is_finite()
                    || (m.m44 - 1.0).abs() > 1e-5
                    || !valid_scale(m.m11)
                    || !valid_scale(m.m22)
                    || !valid_scale(m.m41)
                    || !valid_scale(m.m42)
                {
                    return (true, None);
                }
                Some(TransformOp2D::Matrix {
                    a: m.m11,
                    d: m.m22,
                    e: m.m41,
                    f: m.m42,
                })
            }
            TransformOperation::Rotate(angle) | TransformOperation::RotateZ(angle) => {
                match angle_scale_op(angle.radians()) {
                    Ok(opt) => opt,
                    Err(()) => return (true, None),
                }
            }
            _ => return (true, None),
        };
        if let Some(op) = mapped
            && push_op(&mut ops, op).is_err()
        {
            return (true, None);
        }
    }
    ops.shrink_to_fit();
    (
        true,
        Some(Box::new(TransformStyle {
            origin: [origin_x, origin_y],
            ops,
        })),
    )
}

fn renderer_clip_path_inset(values: &ComputedValues) -> Option<ClipPathInset> {
    use style::values::generics::basic_shape::{BasicShape, ClipPath, ShapeBox, ShapeGeometryBox};
    let ClipPath::Shape(shape, geom_box) = &values.get_svg().clip_path else {
        return None;
    };
    if !matches!(
        geom_box,
        ShapeGeometryBox::ElementDependent | ShapeGeometryBox::ShapeBox(ShapeBox::BorderBox)
    ) {
        return None;
    }
    let BasicShape::Rect(inset_rect) = shape.as_ref() else {
        return None;
    };
    let inset = [
        bounded_box_length(&inset_rect.rect.0, false)?,
        bounded_box_length(&inset_rect.rect.1, false)?,
        bounded_box_length(&inset_rect.rect.2, false)?,
        bounded_box_length(&inset_rect.rect.3, false)?,
    ];
    let radii = [
        &inset_rect.round.top_left,
        &inset_rect.round.top_right,
        &inset_rect.round.bottom_right,
        &inset_rect.round.bottom_left,
    ];
    let mut out_radii = [(Length::Px(0.0), Length::Px(0.0)); 4];
    for (slot, r) in out_radii.iter_mut().zip(radii) {
        *slot = (
            bounded_box_length(&r.0.width.0, true)?,
            bounded_box_length(&r.0.height.0, true)?,
        );
    }
    Some(ClipPathInset {
        inset,
        radii: out_radii,
    })
}

fn renderer_style(values: &ComputedValues) -> Result<ComputedStyle, String> {
    use style::properties::longhands::{
        border_collapse, direction, text_wrap_mode, visibility, white_space_collapse,
    };
    use style::values::computed::font::SingleFontFamily;
    use style::values::computed::{AlignmentBaseline, Display as D, LineHeight};
    use style::values::generics::ClipRectOrAuto;
    use style::values::generics::box_::{BaselineShift, BaselineShiftKeyword};
    use style::values::generics::length::LengthPercentageOrAuto;
    use style::values::specified::text::{TextAlignKeyword, TextDecorationLine, TextOverflowSide};
    use style::values::specified::ui::PointerEvents;
    use style_traits::ToCss;

    let box_style = values.get_box();
    let font = values.get_font();
    let text = values.get_inherited_text();
    let reset_text = values.get_text();
    let effects = values.get_effects();
    let border = values.get_border();
    let margin = values.get_margin();
    let padding = values.get_padding();
    let position = values.get_position();
    let table = values.get_inherited_table();
    let background = values.get_background();
    if background.background_image.0.len() > MAX_BACKGROUND_LAYERS {
        return Err("computed background layer bound exceeded".into());
    }
    let mut font_families = Vec::new();
    for family in font.font_family.families.iter() {
        if font_families.len() == MAX_FONT_FAMILIES {
            return Err("computed font family bound exceeded".into());
        }
        let name = match family {
            SingleFontFamily::FamilyName(name) => {
                if name.name.len() > MAX_FONT_NAME_BYTES {
                    return Err("computed font name bound exceeded".into());
                }
                name.name.to_string()
            }
            SingleFontFamily::Generic(name) => name.to_css_string(),
        };
        font_families.push(name);
    }
    let d = box_style.display;
    let display = match d {
        D::None => Display::None,
        D::Contents => Display::Contents,
        D::Inline => Display::Inline,
        D::InlineBlock => Display::InlineBlock,
        D::Flex => Display::Flex,
        D::InlineFlex => Display::InlineFlex,
        D::Grid => Display::Grid,
        D::InlineGrid => Display::InlineGrid,
        D::Table => Display::Table,
        D::InlineTable => Display::InlineTable,
        D::TableRowGroup => Display::TableRowGroup,
        D::TableHeaderGroup => Display::TableHeaderGroup,
        D::TableFooterGroup => Display::TableFooterGroup,
        D::TableRow => Display::TableRow,
        D::TableCell => Display::TableCell,
        D::TableColumn => Display::TableColumn,
        D::TableColumnGroup => Display::TableColumnGroup,
        D::TableCaption => Display::TableCaption,
        _ if d.to_u16() & D::LIST_ITEM_MASK != 0 => Display::ListItem,
        _ => Display::Block,
    };
    let font_size = font.font_size.computed_size().px();
    let color = absolute_color(&text.color);
    let resolve_color = |value: &style::values::computed::Color| {
        absolute_color(&value.resolve_to_absolute(&text.color))
    };
    let svg = values.get_inherited_svg();
    let svg_paint = |paint: &style::values::computed::SVGPaint,
                     opacity: style::values::computed::SVGOpacity| {
        use style::values::computed::{SVGOpacity, SVGPaintKind};
        let SVGOpacity::Opacity(opacity) = opacity else {
            return SvgPaint::Unsupported;
        };
        if !opacity.is_finite() {
            return SvgPaint::Unsupported;
        }
        match &paint.kind {
            SVGPaintKind::None => SvgPaint::None,
            SVGPaintKind::Color(value) => {
                let mut color = resolve_color(value);
                color.alpha *= opacity.clamp(0.0, 1.0);
                SvgPaint::Color(color)
            }
            SVGPaintKind::PaintServer(_)
            | SVGPaintKind::ContextFill
            | SVGPaintKind::ContextStroke => SvgPaint::Unsupported,
        }
    };
    let line_height = match font.line_height {
        LineHeight::Normal => None,
        LineHeight::Number(number) => Some(number.0 * font_size),
        LineHeight::Length(length) => Some(length.0.px()),
    };
    let vertical_align = match &box_style.baseline_shift {
        BaselineShift::Keyword(BaselineShiftKeyword::Top) => VerticalAlign::Top,
        BaselineShift::Keyword(BaselineShiftKeyword::Bottom) => VerticalAlign::Bottom,
        BaselineShift::Keyword(BaselineShiftKeyword::Center) => VerticalAlign::Middle,
        BaselineShift::Keyword(BaselineShiftKeyword::Sub) => VerticalAlign::Sub,
        BaselineShift::Keyword(BaselineShiftKeyword::Super) => VerticalAlign::Super,
        BaselineShift::Length(value) if plain_length(value) != Length::Px(0.0) => {
            VerticalAlign::Length(plain_length(value))
        }
        _ => match box_style.alignment_baseline {
            AlignmentBaseline::TextTop => VerticalAlign::TextTop,
            AlignmentBaseline::TextBottom => VerticalAlign::TextBottom,
            AlignmentBaseline::Middle => VerticalAlign::Middle,
            _ => VerticalAlign::Baseline,
        },
    };
    let text_align = match text.text_align {
        TextAlignKeyword::Left | TextAlignKeyword::MozLeft => TextAlign::Left,
        TextAlignKeyword::Right | TextAlignKeyword::MozRight => TextAlign::Right,
        TextAlignKeyword::Center | TextAlignKeyword::MozCenter => TextAlign::Center,
        TextAlignKeyword::Justify => TextAlign::Justify,
        TextAlignKeyword::End => TextAlign::End,
        _ => TextAlign::Start,
    };
    let nowrap = text.text_wrap_mode == text_wrap_mode::computed_value::T::Nowrap;
    let white_space = match text.white_space_collapse {
        white_space_collapse::computed_value::T::Preserve => {
            if nowrap {
                WhiteSpace::Pre
            } else {
                WhiteSpace::PreWrap
            }
        }
        white_space_collapse::computed_value::T::PreserveBreaks => WhiteSpace::PreLine,
        white_space_collapse::computed_value::T::BreakSpaces => WhiteSpace::BreakSpaces,
        _ => {
            if nowrap {
                WhiteSpace::NoWrap
            } else {
                WhiteSpace::Normal
            }
        }
    };
    let mut background_images = Vec::new();
    for (index, image) in background.background_image.0.iter().enumerate() {
        if let style::values::generics::image::Image::Url(url) = image {
            if let Some(url) = url.url() {
                if url.as_str().len() > MAX_STYLE_URL_BYTES {
                    return Err("computed background URL bound exceeded".into());
                }
                let (mut width, mut height) = (Length::Auto, Length::Auto);
                if let Some(size) = background
                    .background_size
                    .0
                    .get(index % background.background_size.0.len().max(1))
                {
                    match size {
                        style::values::generics::background::BackgroundSize::ExplicitSize {
                            width: w,
                            height: h,
                        } => {
                            if let LengthPercentageOrAuto::LengthPercentage(v) = w {
                                width = plain_length(&v.0);
                            }
                            if let LengthPercentageOrAuto::LengthPercentage(v) = h {
                                height = plain_length(&v.0);
                            }
                        }
                        style::values::generics::background::BackgroundSize::Contain
                        | style::values::generics::background::BackgroundSize::Cover => {
                            width = Length::Percent(1.0);
                            height = Length::Percent(1.0);
                        }
                    }
                }
                let pos_x = background
                    .background_position_x
                    .0
                    .get(index % background.background_position_x.0.len().max(1))
                    .and_then(|p| bounded_box_length(p, false))
                    .unwrap_or(Length::Px(0.0));
                let pos_y = background
                    .background_position_y
                    .0
                    .get(index % background.background_position_y.0.len().max(1))
                    .and_then(|p| bounded_box_length(p, false))
                    .unwrap_or(Length::Px(0.0));
                background_images.push(BackgroundImage {
                    url: url.as_str().to_owned(),
                    width,
                    height,
                    pos_x,
                    pos_y,
                });
            }
        }
    }
    let widths = [
        &border.border_top_width,
        &border.border_right_width,
        &border.border_bottom_width,
        &border.border_left_width,
    ];
    let border_styles = [
        border.border_top_style,
        border.border_right_style,
        border.border_bottom_style,
        border.border_left_style,
    ];
    let mut border_width = [0.0; 4];
    for side in 0..4 {
        if !matches!(
            border_styles[side],
            style::values::specified::BorderStyle::None
                | style::values::specified::BorderStyle::Hidden
        ) {
            border_width[side] = widths[side].0.to_f32_px();
        }
    }
    let mut box_shadows = Vec::with_capacity(effects.box_shadow.0.len().min(MAX_BOX_SHADOWS));
    for shadow in effects.box_shadow.0.iter().take(MAX_BOX_SHADOWS) {
        let ox = shadow.base.horizontal.px();
        let oy = shadow.base.vertical.px();
        let blur = shadow.base.blur.0.px().max(0.0);
        let spread = shadow.spread.px();
        if ox.is_finite()
            && oy.is_finite()
            && blur.is_finite()
            && spread.is_finite()
            && ox.abs() <= 1_000_000.0
            && oy.abs() <= 1_000_000.0
            && blur <= 1_000_000.0
            && spread.abs() <= 1_000_000.0
        {
            box_shadows.push(BoxShadow {
                offset_x: ox,
                offset_y: oy,
                blur_radius: blur,
                spread_radius: spread,
                color: resolve_color(&shadow.base.color),
                inset: shadow.inset,
            });
        }
    }
    box_shadows.shrink_to_fit();

    let mut text_shadows = Vec::with_capacity(text.text_shadow.0.len().min(MAX_TEXT_SHADOWS));
    for shadow in text.text_shadow.0.iter().take(MAX_TEXT_SHADOWS) {
        let ox = shadow.horizontal.px();
        let oy = shadow.vertical.px();
        let blur = shadow.blur.0.px().max(0.0);
        if ox.is_finite()
            && oy.is_finite()
            && blur.is_finite()
            && ox.abs() <= 1_000_000.0
            && oy.abs() <= 1_000_000.0
            && blur <= 1_000_000.0
        {
            text_shadows.push(TextShadow {
                offset_x: ox,
                offset_y: oy,
                blur_radius: blur,
                color: resolve_color(&shadow.color),
            });
        }
    }
    text_shadows.shrink_to_fit();

    let (has_transform, transform) = renderer_transform(box_style);
    let clip_rect = match &effects.clip {
        ClipRectOrAuto::Auto => None,
        ClipRectOrAuto::Rect(r) => {
            let side = |v: &style::values::computed::LengthOrAuto| match v {
                LengthPercentageOrAuto::Auto => Ok(None),
                LengthPercentageOrAuto::LengthPercentage(l) => {
                    let px = l.px();
                    (px.is_finite() && px.abs() <= 1_000_000.0)
                        .then_some(Some(px))
                        .ok_or(())
                }
            };
            match (side(&r.top), side(&r.right), side(&r.bottom), side(&r.left)) {
                (Ok(t), Ok(ri), Ok(b), Ok(l)) => Some([t, ri, b, l]),
                _ => None,
            }
        }
    };
    let clip_path_inset = renderer_clip_path_inset(values);
    let direction_val = match values.get_inherited_box().direction {
        direction::computed_value::T::Ltr => TextDirection::Ltr,
        direction::computed_value::T::Rtl => TextDirection::Rtl,
    };
    let text_overflow_ellipsis = (if reset_text.text_overflow.sides_are_logical {
        reset_text.text_overflow.second == TextOverflowSide::Ellipsis
    } else if direction_val == TextDirection::Rtl {
        reset_text.text_overflow.first == TextOverflowSide::Ellipsis
    } else {
        reset_text.text_overflow.second == TextOverflowSide::Ellipsis
    }) || values
        .custom_properties()
        .inherited
        .get(&style::Atom::from("mg-txt-ovfl"))
        .is_some_and(|v| v.to_css_string().trim().eq_ignore_ascii_case("ellipsis"));

    Ok(ComputedStyle {
        display,
        layout: LayoutStyle::default(),
        color,
        background_color: resolve_color(&background.background_color),
        background_clip: background
            .background_clip
            .0
            .get(
                background.background_image.0.len().saturating_sub(1)
                    % background.background_clip.0.len().max(1),
            )
            .map_or(BackgroundBox::Border, |value| background_box(*value)),
        background_gradient: renderer_gradient(values),
        box_shadows,
        text_shadows,
        transform,
        has_transform,
        clip_rect,
        clip_path_inset,
        text_overflow_ellipsis,
        svg_fill: svg_paint(&svg.fill, svg.fill_opacity),
        svg_stroke: svg_paint(&svg.stroke, svg.stroke_opacity),
        font_size,
        font_weight: font.font_weight.value().round() as u16,
        italic: font.font_style != style::values::computed::FontStyle::NORMAL,
        font_families,
        line_height,
        underline: reset_text
            .text_decoration_line
            .contains(TextDecorationLine::UNDERLINE),
        margin: [
            &margin.margin_top,
            &margin.margin_right,
            &margin.margin_bottom,
            &margin.margin_left,
        ]
        .map(margin_length),
        padding: [
            &padding.padding_top,
            &padding.padding_right,
            &padding.padding_bottom,
            &padding.padding_left,
        ]
        .map(|v| box_length(&v.0, true)),
        border_width,
        border_color: [
            &border.border_top_color,
            &border.border_right_color,
            &border.border_bottom_color,
            &border.border_left_color,
        ]
        .map(resolve_color),
        border_radii: [
            &border.border_top_left_radius,
            &border.border_top_right_radius,
            &border.border_bottom_right_radius,
            &border.border_bottom_left_radius,
        ]
        .map(|r| {
            (
                box_length(&r.0.width.0, true),
                box_length(&r.0.height.0, true),
            )
        }),
        width: size_length(&position.width),
        height: size_length(&position.height),
        min_width: size_length(&position.min_width),
        min_height: size_length(&position.min_height),
        max_width: max_size_length(&position.max_width),
        max_height: max_size_length(&position.max_height),
        text_align,
        direction: direction_val,
        vertical_align,
        white_space,
        visibility: match values.get_inherited_box().visibility {
            visibility::computed_value::T::Visible => Visibility::Visible,
            visibility::computed_value::T::Hidden => Visibility::Hidden,
            visibility::computed_value::T::Collapse => Visibility::Collapse,
        },
        visible: values.get_inherited_box().visibility == visibility::computed_value::T::Visible,
        own_opacity: effects.opacity.clamp(0.0, 1.0),
        opacity: effects.opacity.clamp(0.0, 1.0),
        pointer_events: values.get_inherited_ui().pointer_events != PointerEvents::None,
        border_collapse: table.border_collapse == border_collapse::computed_value::T::Collapse,
        border_spacing: [
            table.border_spacing.0.width.0.px(),
            table.border_spacing.0.height.0.px(),
        ],
        background_images,
        mask_image: renderer_mask_image(values, font_size),
        before: None,
        after: None,
    })
}

fn parse_mask_url(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let idx = lower.find("url(")?;
    let rest = &raw[idx + 4..];
    let trimmed = rest.trim_start();
    let content = if let Some(q) = trimmed.chars().next().filter(|&c| c == '"' || c == '\'') {
        let inner = &trimmed[1..];
        let end = inner.find(q)?;
        &inner[..end]
    } else {
        let end = trimmed.find(')')?;
        trimmed[..end].trim()
    };
    if content.is_empty() || content.len() > MAX_STYLE_URL_BYTES {
        return None;
    }
    Some(content.to_owned())
}

fn parse_mask_len_token(raw: &str, font_size: f32) -> Option<Length> {
    let t = raw.trim().trim_end_matches('!').trim();
    if t.eq_ignore_ascii_case("auto") {
        return Some(Length::Auto);
    }
    if t.eq_ignore_ascii_case("contain") || t.eq_ignore_ascii_case("cover") {
        return Some(Length::Percent(1.0));
    }
    let lower = t.to_ascii_lowercase();
    if let Some(inner) = lower
        .strip_prefix("calc(")
        .and_then(|_| t[5..].strip_suffix(')'))
    {
        return parse_mask_len_token(inner, font_size);
    }
    if let Some(is_max) = lower
        .strip_prefix("max(")
        .map(|_| true)
        .or_else(|| lower.strip_prefix("min(").map(|_| false))
        && let Some(inner) = t[4..].strip_suffix(')')
    {
        let mut vals = Vec::new();
        for part in inner.split(',') {
            if let Some(Length::Px(px)) = parse_mask_len_token(part, font_size) {
                vals.push(px);
            }
        }
        if !vals.is_empty() {
            let v = if is_max {
                vals.into_iter().fold(f32::NEG_INFINITY, f32::max)
            } else {
                vals.into_iter().fold(f32::INFINITY, f32::min)
            };
            return Some(Length::Px(v));
        }
    }
    if let Some(n) = lower.strip_suffix("rem") {
        return n.trim().parse::<f32>().ok().map(|v| Length::Px(v * 16.0));
    }
    if let Some(n) = lower.strip_suffix("em") {
        return n
            .trim()
            .parse::<f32>()
            .ok()
            .map(|v| Length::Px(v * font_size));
    }
    if let Some(n) = lower.strip_suffix("px") {
        return n.trim().parse::<f32>().ok().map(Length::Px);
    }
    if let Some(n) = lower.strip_suffix('%') {
        return n
            .trim()
            .parse::<f32>()
            .ok()
            .map(|v| Length::Percent(v / 100.0));
    }
    if t == "0" {
        return Some(Length::Px(0.0));
    }
    None
}

fn parse_mask_pos_pair(raw: &str, font_size: f32) -> (Length, Length) {
    let mut parts = raw.split_whitespace();
    let map_kw = |s: &str, horiz: bool| -> Option<Length> {
        if s.eq_ignore_ascii_case("center") {
            Some(Length::Percent(0.5))
        } else if (horiz && s.eq_ignore_ascii_case("left"))
            || (!horiz && s.eq_ignore_ascii_case("top"))
        {
            Some(Length::Px(0.0))
        } else if (horiz && s.eq_ignore_ascii_case("right"))
            || (!horiz && s.eq_ignore_ascii_case("bottom"))
        {
            Some(Length::Percent(1.0))
        } else {
            parse_mask_len_token(s, font_size)
        }
    };
    let Some(a) = parts.next() else {
        return (Length::Px(0.0), Length::Px(0.0));
    };
    if let Some(b) = parts.next() {
        let x = map_kw(a, true).unwrap_or(Length::Px(0.0));
        let y = map_kw(b, false).unwrap_or(Length::Px(0.0));
        (x, y)
    } else if a.eq_ignore_ascii_case("center") {
        (Length::Percent(0.5), Length::Percent(0.5))
    } else if a.eq_ignore_ascii_case("top") {
        (Length::Percent(0.5), Length::Px(0.0))
    } else if a.eq_ignore_ascii_case("bottom") {
        (Length::Percent(0.5), Length::Percent(1.0))
    } else {
        (
            map_kw(a, true).unwrap_or(Length::Px(0.0)),
            Length::Percent(0.5),
        )
    }
}

fn split_top_level_tokens(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        let mut depth = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\'' || b == b'"' {
                let q = b;
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            } else if b == b'(' {
                depth += 1;
            } else if b == b')' {
                depth = depth.saturating_sub(1);
            } else if depth == 0 && b.is_ascii_whitespace() {
                break;
            }
            i += 1;
        }
        out.push(&raw[start..i]);
    }
    out
}

fn parse_mask_size_pair(raw: &str, font_size: f32) -> (Length, Length) {
    let toks = split_top_level_tokens(raw);
    let Some(&first) = toks.first() else {
        return (Length::Auto, Length::Auto);
    };
    if first.eq_ignore_ascii_case("contain") || first.eq_ignore_ascii_case("cover") {
        return (Length::Percent(1.0), Length::Percent(1.0));
    }
    let w = parse_mask_len_token(first, font_size).unwrap_or(Length::Auto);
    let h = toks
        .get(1)
        .and_then(|s| parse_mask_len_token(s, font_size))
        .unwrap_or(w);
    (w, h)
}

fn renderer_mask_image(values: &ComputedValues, font_size: f32) -> Option<Box<BackgroundImage>> {
    use style_traits::ToCss;
    let cp = &values.custom_properties().inherited;
    let short_val = cp.get(&style::Atom::from("mg")).map(|v| v.to_css_string());
    let img_val = cp
        .get(&style::Atom::from("mg-mskim"))
        .map(|v| v.to_css_string());
    let mut url = None;
    let mut width = Length::Auto;
    let mut height = Length::Auto;
    let mut pos_x = Length::Px(0.0);
    let mut pos_y = Length::Px(0.0);
    if let Some(s) = short_val.as_deref() {
        let st = s.trim();
        if !st.eq_ignore_ascii_case("none") {
            url = parse_mask_url(st);
            if let Some(after_url) = st.find(')').map(|p| &st[p + 1..]) {
                let (before_slash, after_slash) = after_url
                    .split_once('/')
                    .map_or((after_url, None), |(a, b)| (a, Some(b)));
                if before_slash
                    .split_whitespace()
                    .any(|w| w.eq_ignore_ascii_case("center"))
                {
                    pos_x = Length::Percent(0.5);
                    pos_y = Length::Percent(0.5);
                }
                if let Some(sz) = after_slash {
                    (width, height) = parse_mask_size_pair(sz, font_size);
                }
            }
        }
    }
    if let Some(im) = img_val.as_deref() {
        let imt = im.trim();
        if imt.eq_ignore_ascii_case("none") {
            if short_val
                .as_deref()
                .is_none_or(|s| s.trim().eq_ignore_ascii_case("none"))
            {
                return None;
            }
        } else if let Some(u) = parse_mask_url(imt) {
            url = Some(u);
        }
    }
    let url = url?;
    if let Some(sz) = cp.get(&style::Atom::from("mg-msks")) {
        let szt = sz.to_css_string();
        if !szt.trim().eq_ignore_ascii_case("auto") {
            (width, height) = parse_mask_size_pair(szt.trim(), font_size);
        }
    }
    if let Some(ps) = cp.get(&style::Atom::from("mg-mskposit")) {
        let pst = ps.to_css_string();
        if pst.trim() != "0% 0%" {
            (pos_x, pos_y) = parse_mask_pos_pair(pst.trim(), font_size);
        }
    }
    Some(Box::new(BackgroundImage {
        url,
        width,
        height,
        pos_x,
        pos_y,
    }))
}

fn resolve_sheet_mask_url(
    mask: &mut Option<Box<BackgroundImage>>,
    document: &Document,
    sheets: &[StylesheetSource],
) {
    let Some(m) = mask.as_deref_mut() else {
        return;
    };
    if document.resources.is_empty()
        || (m.url.len() >= 5 && m.url.as_bytes()[..5].eq_ignore_ascii_case(b"data:"))
    {
        return;
    }
    if let Ok(base) = url::Url::parse(&document.base_url)
        && let Ok(u) = base.join(&m.url)
        && document.resources.contains_key(u.as_str())
    {
        m.url = u.into();
        return;
    }
    for sh in sheets.iter().rev() {
        if let Ok(base) = url::Url::parse(&sh.base_url)
            && let Ok(u) = base.join(&m.url)
            && document.resources.contains_key(u.as_str())
        {
            m.url = u.into();
            return;
        }
    }
}
