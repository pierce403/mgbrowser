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

/// Computed properties consumed by the bounded block/inline/table renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct ComputedStyle {
    pub display: Display,
    pub color: Color,
    pub background_color: Color,
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
            color: black,
            background_color: Color { rgb: 0, alpha: 0.0 },
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
    value.font_families.capacity() * std::mem::size_of::<String>()
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

fn admit_snapshot(
    total: &mut usize,
    old: &ComputedStyle,
    new: &ComputedStyle,
) -> Result<(), String> {
    let next = total
        .checked_sub(snapshot_heap_bytes(old))
        .and_then(|bytes| bytes.checked_add(snapshot_heap_bytes(new)))
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
) -> Option<Arc<Locked<PropertyDeclarationBlock>>> {
    if css.is_empty() {
        return None;
    }
    Some(Arc::new(lock.wrap(
        style::properties::parse_style_attribute(
            css,
            url,
            None,
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

/// Compute a fresh static style snapshot with no network or script capability.
/// Stylesheets must be supplied in document order, including inline style blocks.
/// CSS imports, animations, pseudo-element boxes and visited history are absent.
pub fn compute_styles(
    document: &Document,
    sheets: &[StylesheetSource],
    viewport: (f32, f32),
) -> Result<Vec<ComputedStyle>, String> {
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
    let mut pending = vec![(0usize, 0usize)];
    while let Some((id, depth)) = pending.pop() {
        if id >= seen.len() || seen[id] || depth > 256 {
            return Err("invalid style DOM topology".into());
        }
        seen[id] = true;
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
            inline: parsed_declarations(node.attr("style").unwrap_or(""), &url_data, &lock),
            hints: parsed_declarations(&presentation_hints(document, id), &url_data, &lock),
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
    let sheet = |css: &str, base: UrlExtraData, origin: Origin, media_text: &str| {
        let mut context = style::parser::ParserContext::new(
            origin,
            &base,
            None,
            style_traits::ParsingMode::DEFAULT,
            QuirksMode::NoQuirks,
            Default::default(),
            None,
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
            None,
            QuirksMode::NoQuirks,
            AllowImportRules::No,
        )))
    };
    stylist.append_stylesheet(
        sheet(USER_AGENT_CSS, url_data.clone(), Origin::UserAgent, ""),
        &lock.read(),
    );
    for source in sheets {
        let base = url::Url::parse(&source.base_url)
            .map(UrlExtraData::from)
            .unwrap_or_else(|_| url_data.clone());
        stylist.append_stylesheet(
            sheet(&source.css, base, Origin::Author, &source.media),
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
            admit_snapshot(&mut snapshot_bytes, &output[id], &output[parent_id])?;
            output[id] = output[parent_id].clone();
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
        if document.scripting_enabled() && node.node().tag == "noscript" {
            // Inactive fallback content is not overrideable by author CSS.
            snapshot.display = Display::None;
        }
        admit_snapshot(&mut snapshot_bytes, &output[id], &snapshot)?;
        output[id] = snapshot;
        node.data().data.borrow_mut().styles.primary = Some(computed.clone());
        values[id] = computed;
    }
    Ok(output)
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
        color,
        background_color: resolve_color(&background.background_color),
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
