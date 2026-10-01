//! CSS computation and the small, renderer-facing style contract.
//!
//! Values are indexed by the document arena. Percentages remain relative until
//! layout knows the containing block; all other lengths are CSS pixels.

pub use crate::document::StylesheetSource;

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

/// A CSS length whose containing-block percentage is resolved during layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Px(f32),
    /// A fraction: `0.85` represents `85%`.
    Percent(f32),
}

impl Length {
    pub fn resolve(self, basis: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(value) => Some(value),
            Self::Percent(value) => Some(value * basis),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridLine {
    Auto,
    Line(i16),
    Span(u16),
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
}

/// Integer repeats are flattened after checking their expanded length. Empty
/// template vectors mean `none`; empty implicit vectors mean the initial `auto`.
#[derive(Clone, Debug, PartialEq)]
pub struct GridStyle {
    pub template_rows: Vec<TrackSize>,
    pub template_columns: Vec<TrackSize>,
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
    pub min_width_intrinsic: Option<IntrinsicSize>,
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
            min_width_intrinsic: None,
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
            grid_row: [GridLine::Auto; 2],
            grid_column: [GridLine::Auto; 2],
            grid: None,
            unsupported: false,
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub color: Color,
    /// Unspecified first/last stops are represented as 0%/100%.
    pub position: Length,
}

/// One non-repeating, two-stop sRGB linear gradient. No heap-owned source,
/// raster surface or unbounded stop list is retained by this snapshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearGradient {
    pub direction: GradientDirection,
    pub stops: [GradientStop; 2],
    pub origin: BackgroundBox,
    pub clip: BackgroundBox,
    /// Background tiling, distinct from repeating-linear-gradient().
    pub repeat: [bool; 2],
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
    pub background_gradient: Option<LinearGradient>,
    pub svg_fill: SvgPaint,
    pub svg_stroke: SvgPaint,
    pub font_size: f32,
    pub font_weight: u16,
    pub font_families: Vec<String>,
    /// `None` means the font's normal line height.
    pub line_height: Option<f32>,
    pub underline: bool,
    /// Sides are top, right, bottom, left.
    pub margin: [Length; 4],
    pub padding: [Length; 4],
    pub border_width: [f32; 4],
    pub border_color: [Color; 4],
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub min_height: Length,
    pub max_width: Length,
    pub max_height: Length,
    pub text_align: TextAlign,
    pub vertical_align: VerticalAlign,
    pub white_space: WhiteSpace,
    pub visible: bool,
    pub pointer_events: bool,
    pub border_collapse: bool,
    /// Horizontal, vertical CSS pixel spacing.
    pub border_spacing: [f32; 2],
    /// URL layers in CSS front-to-back order. Paint in reverse order.
    pub background_images: Vec<BackgroundImage>,
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
            svg_fill: SvgPaint::Color(black),
            svg_stroke: SvgPaint::None,
            font_size: 16.0,
            font_weight: 400,
            font_families: vec!["sans-serif".to_owned()],
            line_height: None,
            underline: false,
            margin: [Length::Px(0.0); 4],
            padding: [Length::Px(0.0); 4],
            border_width: [0.0; 4],
            border_color: [black; 4],
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            min_height: Length::Auto,
            max_width: Length::Auto,
            max_height: Length::Auto,
            text_align: TextAlign::Start,
            vertical_align: VerticalAlign::Baseline,
            white_space: WhiteSpace::Normal,
            visible: true,
            pointer_events: true,
            border_collapse: false,
            border_spacing: [2.0; 2],
            background_images: Vec::new(),
        }
    }
}

impl ComputedStyle {
    // Preserve the existing text-style convention without ever cloning an
    // element's non-inherited grid container allocation into a text node.
    fn clone_with_layout(&self, layout: LayoutStyle) -> Self {
        Self {
            display: self.display,
            layout,
            color: self.color,
            background_color: self.background_color,
            background_clip: self.background_clip,
            background_gradient: self.background_gradient,
            svg_fill: self.svg_fill,
            svg_stroke: self.svg_stroke,
            font_size: self.font_size,
            font_weight: self.font_weight,
            font_families: self.font_families.clone(),
            line_height: self.line_height,
            underline: self.underline,
            margin: self.margin,
            padding: self.padding,
            border_width: self.border_width,
            border_color: self.border_color,
            width: self.width,
            height: self.height,
            min_width: self.min_width,
            min_height: self.min_height,
            max_width: self.max_width,
            max_height: self.max_height,
            text_align: self.text_align,
            vertical_align: self.vertical_align,
            white_space: self.white_space,
            visible: self.visible,
            pointer_events: self.pointer_events,
            border_collapse: self.border_collapse,
            border_spacing: self.border_spacing,
            background_images: self.background_images.clone(),
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
html, body, div, p, section, article, header, footer, main, nav, form, center,
blockquote, pre, ul, ol, dl, dt, dd, h1, h2, h3, h4, h5, h6 { display: block }
head, title, style, script, meta, link, template, [hidden], input[type=hidden] { display: none }
body { margin: 8px; font-family: sans-serif; color: black }
p, blockquote, ul, ol, dl, pre { margin-top: 1em; margin-bottom: 1em }
h1 { font-size: 2em; margin: .67em 0; font-weight: bold }
h2 { font-size: 1.5em; margin: .83em 0; font-weight: bold }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }
b, strong, th { font-weight: bold }
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
input, button { padding: 1px 2px; border: 2px solid #888 }
form { margin-bottom: 1em }
hr { display: block; margin: .5em auto; border: 1px solid #888 }
"#;

const MAX_SNAPSHOT_HEAP_BYTES: usize = 8 * 1024 * 1024;
const MAX_BACKGROUND_LAYERS: usize = 8;
const MAX_FONT_FAMILIES: usize = 16;
const MAX_STYLE_URL_BYTES: usize = 4096;
const MAX_FONT_NAME_BYTES: usize = 256;

fn snapshot_heap_bytes(value: &ComputedStyle) -> usize {
    layout_heap_bytes(&value.layout)
        + value.font_families.capacity() * std::mem::size_of::<String>()
        + value.background_images.capacity() * std::mem::size_of::<BackgroundImage>()
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

fn layout_heap_bytes(value: &LayoutStyle) -> usize {
    value.grid.as_ref().map_or(0, |grid| {
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
    document: &'a Document,
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
        &self.document.document.nodes[self.id]
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
        (self.id != 0).then(|| self.at(self.node().parent))
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
        true
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
        _: &PseudoElement,
        _: &mut MatchingContext<SelectorImpl>,
    ) -> bool {
        false
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
    Some(Arc::new(lock.wrap(
        style::properties::parse_style_attribute(
            css,
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
    if sheets.len() > 64 || sheet_bytes.is_none_or(|bytes| bytes > 2 * 1024 * 1024) {
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
        document,
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
    stylist.append_stylesheet(
        sheet(
            USER_AGENT_CSS,
            url_data.clone(),
            Origin::UserAgent,
            "",
            None,
        ),
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
        stylist.append_stylesheet(
            sheet(
                &source.css,
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
            let inherited_bytes = snapshot_heap_bytes(&output[parent_id])
                - layout_heap_bytes(&output[parent_id].layout);
            admit_snapshot_bytes(
                &mut snapshot_bytes,
                snapshot_heap_bytes(&output[id]),
                inherited_bytes,
            )?;
            output[id] = output[parent_id].clone_with_layout(LayoutStyle::default());
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
        snapshot.layout = renderer_layout(
            &computed,
            snapshot.display,
            remaining,
            id,
            &document.base_url,
            &mut diagnostics.borrow_mut(),
        )?;
        if document.scripting_enabled() && node.node().tag == "noscript" {
            // Inactive fallback content is not overrideable by author CSS.
            snapshot.display = Display::None;
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
    let border = values.get_border();
    if [
        &border.border_top_left_radius,
        &border.border_top_right_radius,
        &border.border_bottom_right_radius,
        &border.border_bottom_left_radius,
    ]
    .iter()
    .any(|radius| {
        [&radius.0.width.0, &radius.0.height.0]
            .iter()
            .any(|value| !matches!(plain_length(value), Length::Px(0.) | Length::Percent(0.)))
    }) {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS nonzero border-radius is not implemented; backgrounds, borders and clipping use square corners"),
            format_args!("{source}"), Some(node), None,
        );
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
    {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS gradient omitted: only one modern non-repeating two-stop sRGB linear gradient with bounded plain lengths/percentages, auto size, zero position, scroll attachment and rectangular clipping is implemented"),
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
    if rendered.layout.position == Position::Sticky {
        diagnostics.record(
            "css-unsupported",
            format_args!(
                "CSS position: {} is not implemented; using normal flow without sticky offsets",
                box_style.position.to_css_string()
            ),
            format_args!("{source}"),
            Some(node),
            None,
        );
    }
    if matches!(
        rendered.layout.position,
        Position::Absolute | Position::Fixed
    ) {
        for (axis, before, after) in [("horizontal", 3, 1), ("vertical", 0, 2)] {
            if rendered.layout.inset[before] == Length::Auto
                && rendered.layout.inset[after] == Length::Auto
            {
                diagnostics.record(
                    "css-unsupported",
                    format_args!("CSS position: {} has both {axis} insets auto; hypothetical static position is not implemented; using containing-block start", box_style.position.to_css_string()),
                    format_args!("{source}"), Some(node), None,
                );
            }
        }
    }
    if let Some(z) = rendered.layout.z_index {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS z-index: {z} uses limited positioned paint ordering; full stacking contexts and negative layers behind normal flow are not implemented"),
            format_args!("{source}"), Some(node), None,
        );
    }
    if rendered.display == Display::TableCell
        && rendered
            .layout
            .overflow
            .iter()
            .any(|value| *value != Overflow::Visible)
    {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS table-cell overflow clipping is not implemented by the legacy automatic-table path"),
            format_args!("{source}"), Some(node), None,
        );
    }
    if rendered
        .layout
        .overflow
        .iter()
        .any(|value| matches!(value, Overflow::Auto | Overflow::Scroll))
    {
        diagnostics.record(
            "css-unsupported",
            format_args!("CSS overflow-x: {}; overflow-y: {} require element scrolling, which is not implemented", box_style.overflow_x.to_css_string(), box_style.overflow_y.to_css_string()),
            format_args!("{source}"),
            Some(node),
            None,
        );
    }
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

#[derive(Clone, Copy, Debug)]
enum MappingFailure {
    Unsupported(&'static str),
    Limit(&'static str),
}
type Mapping<T> = Result<T, MappingFailure>;

fn layout_length(value: &style::values::computed::LengthPercentage) -> Mapping<Length> {
    let result = if let Some(px) = value.to_length() {
        Length::Px(px.px())
    } else if let Some(percent) = value.to_percentage() {
        Length::Percent(percent.0)
    } else {
        return Err(MappingFailure::Unsupported("mixed calc lengths"));
    };
    match result {
        Length::Px(v) | Length::Percent(v) if v.is_finite() => Ok(result),
        _ => Err(MappingFailure::Limit("non-finite computed layout length")),
    }
}
fn layout_size(value: &style::values::computed::Size) -> Mapping<Length> {
    use style::values::computed::Size;
    match value {
        Size::Auto => Ok(Length::Auto),
        Size::LengthPercentage(v) => layout_length(&v.0),
        _ => Err(MappingFailure::Unsupported("intrinsic or anchor sizing")),
    }
}
fn layout_inset(value: &style::values::computed::Inset) -> Mapping<Length> {
    use style::values::computed::Inset;
    match value {
        Inset::Auto => Ok(Length::Auto),
        Inset::LengthPercentage(v) => layout_length(v),
        _ => Err(MappingFailure::Unsupported("anchor positioning")),
    }
}
fn layout_gap(
    value: &style::values::computed::length::NonNegativeLengthPercentageOrNormal,
) -> Mapping<Length> {
    use style::values::generics::length::LengthPercentageOrNormal;
    match value {
        LengthPercentageOrNormal::Normal => Ok(Length::Px(0.)),
        LengthPercentageOrNormal::LengthPercentage(v) => layout_length(&v.0),
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
fn grid_line(value: &style::values::computed::GridLine) -> Mapping<GridLine> {
    if !value.ident.0.is_empty() {
        return Err(MappingFailure::Unsupported("named grid placement"));
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
        B::Breadth(v) => TrackBreadth::Length(layout_length(v)?),
        B::Flex(v) if v.0.is_finite() && v.0 >= 0. => TrackBreadth::Fr(v.0),
        B::Flex(_) => return Err(MappingFailure::Limit("non-finite computed grid fraction")),
    })
}
fn track_size(value: &style::values::computed::TrackSize) -> Mapping<TrackSize> {
    use style::values::generics::grid::TrackSize as T;
    Ok(match value {
        T::Breadth(v) => TrackSize::Breadth(track_breadth(v)?),
        T::Minmax(min, max) => TrackSize::MinMax(track_breadth(min)?, track_breadth(max)?),
        T::FitContent(_) => return Err(MappingFailure::Unsupported("fit-content grid tracks")),
    })
}
fn template_count(value: &style::values::computed::GridTemplateComponent) -> Mapping<usize> {
    use style::values::generics::grid::{
        GridTemplateComponent as G, RepeatCount, TrackListValue as V,
    };
    let tracks = match value {
        G::None => return Ok(0),
        G::TrackList(tracks) => tracks,
        _ => return Err(MappingFailure::Unsupported("subgrid or masonry")),
    };
    if tracks.line_names.iter().any(|names| !names.is_empty()) {
        return Err(MappingFailure::Unsupported("named grid lines"));
    }
    let mut count = 0usize;
    for value in tracks.values.iter() {
        let amount = match value {
            V::TrackSize(_) => 1,
            V::TrackRepeat(repeat) => {
                if repeat.line_names.iter().any(|names| !names.is_empty()) {
                    return Err(MappingFailure::Unsupported("named repeated grid lines"));
                }
                let RepeatCount::Number(n) = repeat.count else {
                    return Err(MappingFailure::Unsupported(
                        "auto-fill/auto-fit grid repeats",
                    ));
                };
                if n <= 0 {
                    return Err(MappingFailure::Limit("invalid computed grid repeat"));
                }
                (n as usize)
                    .checked_mul(repeat.track_sizes.len())
                    .ok_or(MappingFailure::Limit("computed grid track bound exceeded"))?
            }
        };
        count = count
            .checked_add(amount)
            .filter(|n| *n <= MAX_GRID_TRACKS)
            .ok_or(MappingFailure::Limit("computed grid track bound exceeded"))?;
    }
    Ok(count)
}
fn template_tracks(
    value: &style::values::computed::GridTemplateComponent,
    count: usize,
) -> Mapping<Vec<TrackSize>> {
    use style::values::generics::grid::{
        GridTemplateComponent as G, RepeatCount, TrackListValue as V,
    };
    let mut result = Vec::with_capacity(count);
    if let G::TrackList(tracks) = value {
        for value in tracks.values.iter() {
            match value {
                V::TrackSize(v) => result.push(track_size(v)?),
                V::TrackRepeat(repeat) => {
                    let RepeatCount::Number(n) = repeat.count else {
                        unreachable!("preflighted repeat")
                    };
                    for _ in 0..n {
                        for v in repeat.track_sizes.iter() {
                            result.push(track_size(v)?);
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}
fn grid_style(
    position: &style::properties::style_structs::Position,
    remaining: usize,
) -> Mapping<Box<GridStyle>> {
    use style::values::computed::{GridAutoFlow as F, GridTemplateAreas};
    if !matches!(position.grid_template_areas, GridTemplateAreas::None) {
        return Err(MappingFailure::Unsupported("named grid areas"));
    }
    let rows = template_count(&position.grid_template_rows)?;
    let columns = template_count(&position.grid_template_columns)?;
    let auto_rows = position.grid_auto_rows.0.len();
    let auto_columns = position.grid_auto_columns.0.len();
    if auto_rows > MAX_GRID_TRACKS || auto_columns > MAX_GRID_TRACKS {
        return Err(MappingFailure::Limit(
            "computed implicit grid track bound exceeded",
        ));
    }
    let bytes = (rows + columns + auto_rows + auto_columns)
        .checked_mul(std::mem::size_of::<TrackSize>())
        .and_then(|n| n.checked_add(std::mem::size_of::<GridStyle>()))
        .filter(|n| *n <= remaining)
        .ok_or(MappingFailure::Limit(
            "computed style snapshot bound exceeded",
        ))?;
    let _ = bytes; // Admission above precedes every owned grid-vector allocation.
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
    Ok(Box::new(GridStyle {
        template_rows: template_tracks(&position.grid_template_rows, rows)?,
        template_columns: template_tracks(&position.grid_template_columns, columns)?,
        auto_rows: implicit(&position.grid_auto_rows)?,
        auto_columns: implicit(&position.grid_auto_columns)?,
        auto_flow,
    }))
}

struct LayoutReporter<'a> {
    diagnostics: &'a mut StyleDiagnostics,
    node: usize,
    source: &'a str,
    unsupported: bool,
}
impl LayoutReporter<'_> {
    fn convert<T>(&mut self, property: &str, value: Mapping<T>, fallback: T) -> Result<T, String> {
        match value {
            Ok(value) => Ok(value),
            Err(MappingFailure::Limit(message)) => Err(message.into()),
            Err(MappingFailure::Unsupported(message)) => {
                self.unsupported = true;
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
    remaining: usize,
    node: usize,
    source: &str,
    diagnostics: &mut StyleDiagnostics,
) -> Result<LayoutStyle, String> {
    use style::properties::longhands::{
        aspect_ratio, box_sizing, contain, direction, flex_direction, flex_wrap, writing_mode,
    };
    use style::values::generics::{box_::PositionProperty, flex::FlexBasis as B, position::ZIndex};
    let p = values.get_position();
    let b = values.get_box();
    let mut report = LayoutReporter {
        diagnostics,
        node,
        source,
        unsupported: false,
    };
    let mut output = LayoutStyle {
        box_sizing: match p.box_sizing {
            box_sizing::computed_value::T::ContentBox => BoxSizing::ContentBox,
            box_sizing::computed_value::T::BorderBox => BoxSizing::BorderBox,
        },
        position: match b.position {
            PositionProperty::Static => Position::Static,
            PositionProperty::Relative => Position::Relative,
            PositionProperty::Absolute => Position::Absolute,
            PositionProperty::Fixed => Position::Fixed,
            PositionProperty::Sticky => Position::Sticky,
        },
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
        flex_basis: report.convert(
            "flex-basis",
            match &p.flex_basis {
                B::Content => Ok(FlexBasis::Content),
                B::Size(value) => layout_size(value).map(|v| {
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
            "direction",
            inherited.direction != direction::get_initial_value(),
            "non-default direction",
        ),
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
        (
            "aspect-ratio",
            p.aspect_ratio != aspect_ratio::get_initial_value(),
            "CSS preferred aspect ratios",
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
        *slot = report.convert(name, layout_inset(value), Length::Auto)?;
    }
    for (slot, (name, value)) in output
        .gap
        .iter_mut()
        .zip([("row-gap", &p.row_gap), ("column-gap", &p.column_gap)])
    {
        *slot = report.convert(name, layout_gap(value), Length::Px(0.))?;
    }
    use style::values::computed::Size;
    output.width_fit_content = matches!(p.width, Size::FitContent);
    output.width_max_content = matches!(p.width, Size::MaxContent);
    if !output.width_fit_content && !output.width_max_content {
        report.convert("width", layout_size(&p.width), Length::Auto)?;
    }
    for (name, value) in [("height", &p.height), ("min-height", &p.min_height)] {
        report.convert(name, layout_size(value), Length::Auto)?;
    }
    output.min_width_intrinsic = match p.min_width {
        Size::MinContent => Some(IntrinsicSize::MinContent),
        Size::MaxContent => Some(IntrinsicSize::MaxContent),
        Size::FitContent => Some(IntrinsicSize::FitContent),
        _ => {
            report.convert("min-width", layout_size(&p.min_width), Length::Auto)?;
            None
        }
    };
    for (name, value) in [("max-width", &p.max_width), ("max-height", &p.max_height)] {
        use style::values::computed::MaxSize;
        if name == "max-width" && matches!(value, MaxSize::FitContent) {
            output.max_width_fit_content = true;
            continue;
        }
        report.convert(
            name,
            match value {
                MaxSize::None => Ok(Length::Auto),
                MaxSize::LengthPercentage(v) => layout_length(&v.0),
                _ => Err(MappingFailure::Unsupported("intrinsic or anchor sizing")),
            },
            Length::Auto,
        )?;
    }
    let margin = values.get_margin();
    for (name, value) in [
        ("margin-top", &margin.margin_top),
        ("margin-right", &margin.margin_right),
        ("margin-bottom", &margin.margin_bottom),
        ("margin-left", &margin.margin_left),
    ] {
        use style::values::computed::Margin;
        report.convert(
            name,
            match value {
                Margin::Auto => Ok(Length::Auto),
                Margin::LengthPercentage(v) => layout_length(v),
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
        report.convert(name, layout_length(&value.0), Length::Px(0.))?;
    }
    for (slot, (name, value)) in output.grid_row.iter_mut().zip([
        ("grid-row-start", &p.grid_row_start),
        ("grid-row-end", &p.grid_row_end),
    ]) {
        *slot = report.convert(name, grid_line(value), GridLine::Auto)?;
    }
    for (slot, (name, value)) in output.grid_column.iter_mut().zip([
        ("grid-column-start", &p.grid_column_start),
        ("grid-column-end", &p.grid_column_end),
    ]) {
        *slot = report.convert(name, grid_line(value), GridLine::Auto)?;
    }
    if matches!(display, Display::Grid | Display::InlineGrid) {
        output.grid = report.convert("grid-template", grid_style(p, remaining).map(Some), None)?;
    }
    output.unsupported = report.unsupported;
    Ok(output)
}

fn size_length(value: &style::values::computed::Size) -> Length {
    match value {
        style::values::computed::Size::LengthPercentage(value) => plain_length(&value.0),
        _ => Length::Auto,
    }
}

fn max_size_length(value: &style::values::computed::MaxSize) -> Length {
    match value {
        style::values::computed::MaxSize::LengthPercentage(value) => plain_length(&value.0),
        _ => Length::Auto,
    }
}

fn margin_length(value: &style::values::computed::Margin) -> Length {
    match value {
        style::values::computed::Margin::LengthPercentage(value) => plain_length(value),
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

/// Convert only the explicitly implemented background slice. This is a fixed
/// two-stop read of Stylo's actual computed values, not CSS string reparsing.
fn renderer_gradient(values: &ComputedValues) -> Option<LinearGradient> {
    use style::properties::longhands::{background_attachment, background_origin};
    use style::values::computed::image::LineDirection;
    use style::values::generics::background::BackgroundSize;
    use style::values::generics::image::{
        Gradient, GradientCompatMode, GradientFlags, GradientItem, Image,
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
    let Gradient::Linear {
        direction,
        color_interpolation_method,
        items,
        flags,
        compat_mode,
    } = &**gradient
    else {
        return None;
    };
    if *compat_mode != GradientCompatMode::Modern
        || flags.contains(GradientFlags::REPEATING)
        || color_interpolation_method.space != style::color::ColorSpace::Srgb
        || items.len() != 2
    {
        return None;
    }
    let size = background.background_size.0.first()?;
    if !matches!(
        size,
        BackgroundSize::ExplicitSize {
            width: LengthPercentageOrAuto::Auto,
            height: LengthPercentageOrAuto::Auto
        }
    ) {
        return None;
    }
    for position in [
        background.background_position_x.0.first()?,
        background.background_position_y.0.first()?,
    ] {
        if !matches!(plain_length(position), Length::Px(0.) | Length::Percent(0.)) {
            return None;
        }
    }
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
    let direction = match *direction {
        LineDirection::Angle(angle) => {
            let radians = angle.radians();
            if !radians.is_finite() {
                return None;
            }
            GradientDirection::Angle(radians.rem_euclid(std::f32::consts::TAU))
        }
        LineDirection::Horizontal(H::Left) => GradientDirection::Angle(std::f32::consts::PI * 1.5),
        LineDirection::Horizontal(H::Right) => {
            GradientDirection::Angle(std::f32::consts::FRAC_PI_2)
        }
        LineDirection::Vertical(V::Top) => GradientDirection::Angle(0.),
        LineDirection::Vertical(V::Bottom) => GradientDirection::Angle(std::f32::consts::PI),
        LineDirection::Corner(horizontal, vertical) => GradientDirection::Corner {
            right: horizontal == H::Right,
            bottom: vertical == V::Bottom,
        },
    };
    let text = values.get_inherited_text();
    let stop = |index: usize| {
        let (color, position) = match &items[index] {
            GradientItem::SimpleColorStop(color) => (color, Length::Percent(index as f32)),
            GradientItem::ComplexColorStop { color, position } => (color, plain_length(position)),
            GradientItem::InterpolationHint(_) => return None,
        };
        if !matches!(position, Length::Px(value) | Length::Percent(value) if value.is_finite() && value.abs() <= 1_000_000.)
        {
            return None;
        }
        let color = absolute_color(&color.resolve_to_absolute(&text.color));
        if !color.alpha.is_finite() {
            return None;
        }
        Some(GradientStop { color, position })
    };
    Some(LinearGradient {
        direction,
        stops: [stop(0)?, stop(1)?],
        origin,
        clip,
        repeat,
    })
}

fn renderer_style(values: &ComputedValues) -> Result<ComputedStyle, String> {
    use style::properties::longhands::{
        border_collapse, text_wrap_mode, visibility, white_space_collapse,
    };
    use style::values::computed::font::SingleFontFamily;
    use style::values::computed::{AlignmentBaseline, Display as D, LineHeight};
    use style::values::generics::box_::{BaselineShift, BaselineShiftKeyword};
    use style::values::specified::text::{TextAlignKeyword, TextDecorationLine};
    use style::values::specified::ui::PointerEvents;
    use style_traits::ToCss;

    let box_style = values.get_box();
    let font = values.get_font();
    let text = values.get_inherited_text();
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
                    if let style::values::generics::background::BackgroundSize::ExplicitSize {
                        width: w,
                        height: h,
                    } = size
                    {
                        if let style::values::generics::length::LengthPercentageOrAuto::LengthPercentage(v) = w { width = plain_length(&v.0); }
                        if let style::values::generics::length::LengthPercentageOrAuto::LengthPercentage(v) = h { height = plain_length(&v.0); }
                    }
                }
                background_images.push(BackgroundImage {
                    url: url.as_str().to_owned(),
                    width,
                    height,
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
        svg_fill: svg_paint(&svg.fill, svg.fill_opacity),
        svg_stroke: svg_paint(&svg.stroke, svg.stroke_opacity),
        font_size,
        font_weight: font.font_weight.value().round() as u16,
        font_families,
        line_height,
        underline: values
            .get_text()
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
        .map(|v| plain_length(&v.0)),
        border_width,
        border_color: [
            &border.border_top_color,
            &border.border_right_color,
            &border.border_bottom_color,
            &border.border_left_color,
        ]
        .map(resolve_color),
        width: size_length(&position.width),
        height: size_length(&position.height),
        min_width: size_length(&position.min_width),
        min_height: size_length(&position.min_height),
        max_width: max_size_length(&position.max_width),
        max_height: max_size_length(&position.max_height),
        text_align,
        vertical_align,
        white_space,
        visible: values.get_inherited_box().visibility == visibility::computed_value::T::Visible,
        pointer_events: values.get_inherited_ui().pointer_events != PointerEvents::None,
        border_collapse: table.border_collapse == border_collapse::computed_value::T::Collapse,
        border_spacing: [
            table.border_spacing.0.width.0.px(),
            table.border_spacing.0.height.0.px(),
        ],
        background_images,
    })
}
