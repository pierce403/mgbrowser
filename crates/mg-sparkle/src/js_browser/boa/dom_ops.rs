//! Modern DOM tree manipulation, traversal, `<template>`, `DOMParser`, `<dialog>`,
//! form-control properties, `CSS.supports`/`CSS.escape`, and Window named `id` access.
use super::*;
use crate::document::Node;

pub(super) const CREATE_DOCUMENT_FRAGMENT: &str = "host.dom.createDocumentFragment";
pub(super) const CLONE_NODE: &str = "host.dom.cloneNode";
pub(super) const INSERT_BEFORE: &str = "host.dom.insertBefore";
pub(super) const REPLACE_CHILD: &str = "host.dom.replaceChild";
pub(super) const REMOVE: &str = "host.dom.remove";
pub(super) const BEFORE: &str = "host.dom.before";
pub(super) const AFTER: &str = "host.dom.after";
pub(super) const REPLACE_WITH: &str = "host.dom.replaceWith";
pub(super) const PREPEND: &str = "host.dom.prepend";
pub(super) const APPEND: &str = "host.dom.append";
pub(super) const REPLACE_CHILDREN: &str = "host.dom.replaceChildren";
pub(super) const CONTAINS: &str = "host.dom.contains";
pub(super) const HAS_CHILD_NODES: &str = "host.dom.hasChildNodes";
pub(super) const HAS_ATTRIBUTE: &str = "host.dom.hasAttribute";
pub(super) const TOGGLE_ATTRIBUTE: &str = "host.dom.toggleAttribute";
pub(super) const GET_ATTRIBUTE_NAMES: &str = "host.dom.getAttributeNames";
pub(super) const INSERT_ADJACENT_HTML: &str = "host.dom.insertAdjacentHTML";
pub(super) const INSERT_ADJACENT_ELEMENT: &str = "host.dom.insertAdjacentElement";
pub(super) const INSERT_ADJACENT_TEXT: &str = "host.dom.insertAdjacentText";
pub(super) const DIALOG_SHOW: &str = "host.dialog.show";
pub(super) const DIALOG_SHOW_MODAL: &str = "host.dialog.showModal";
pub(super) const DIALOG_CLOSE: &str = "host.dialog.close";
pub(super) const DOM_PARSER_PARSE: &str = "host.domparser.parseFromString";
pub(super) const CSS_SUPPORTS: &str = "host.css.supports";
pub(super) const CSS_ESCAPE: &str = "host.css.escape";
pub(super) const COMPARE_DOCUMENT_POSITION: &str = "host.dom.compareDocumentPosition";
pub(super) const GET_ROOT_NODE: &str = "host.dom.getRootNode";
pub(super) const CREATE_TREE_WALKER: &str = "host.dom.createTreeWalker";
pub(super) const DOC_HAS_FOCUS: &str = "host.dom.hasFocus";
pub(super) const DOC_WRITE: &str = "host.dom.write";
pub(super) const DOC_WRITELN: &str = "host.dom.writeln";
pub(super) const DOC_OPEN: &str = "host.dom.docOpen";
pub(super) const DOC_CLOSE: &str = "host.dom.docClose";
pub(super) const IS_SAME_NODE: &str = "host.dom.isSameNode";
pub(super) const IS_EQUAL_NODE: &str = "host.dom.isEqualNode";
pub(super) const NORMALIZE: &str = "host.dom.normalize";
pub(super) const LOOKUP_NAMESPACE_URI: &str = "host.dom.lookupNamespaceURI";
pub(super) const LOOKUP_PREFIX: &str = "host.dom.lookupPrefix";
pub(super) const IS_DEFAULT_NAMESPACE: &str = "host.dom.isDefaultNamespace";
pub(super) const HAS_ATTRIBUTES: &str = "host.dom.hasAttributes";
pub(super) const GET_ATTRIBUTE_NS: &str = "host.dom.getAttributeNS";
pub(super) const SET_ATTRIBUTE_NS: &str = "host.dom.setAttributeNS";
pub(super) const REMOVE_ATTRIBUTE_NS: &str = "host.dom.removeAttributeNS";
pub(super) const HAS_ATTRIBUTE_NS: &str = "host.dom.hasAttributeNS";
pub(super) const GET_ELEMENTS_BY_TAG_NAME_NS: &str = "host.dom.getElementsByTagNameNS";
pub(super) const GET_ATTRIBUTE_NODE: &str = "host.dom.getAttributeNode";
pub(super) const GET_ATTRIBUTE_NODE_NS: &str = "host.dom.getAttributeNodeNS";
pub(super) const SET_ATTRIBUTE_NODE: &str = "host.dom.setAttributeNode";
pub(super) const SET_ATTRIBUTE_NODE_NS: &str = "host.dom.setAttributeNodeNS";
pub(super) const REMOVE_ATTRIBUTE_NODE: &str = "host.dom.removeAttributeNode";
pub(super) const ATTACH_SHADOW: &str = "host.dom.attachShadow";
pub(super) const CHECK_VISIBILITY: &str = "host.dom.checkVisibility";
pub(super) const REQUEST_FULLSCREEN: &str = "host.dom.requestFullscreen";
pub(super) const EL_SCROLL: &str = "host.dom.elScroll";
pub(super) const EL_SCROLL_TO: &str = "host.dom.elScrollTo";
pub(super) const EL_SCROLL_BY: &str = "host.dom.elScrollBy";
pub(super) const EL_SCROLL_INTO_VIEW_IF_NEEDED: &str = "host.dom.scrollIntoViewIfNeeded";
pub(super) const IMG_DECODE: &str = "host.dom.imgDecode";

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        "host.dom.appendChild" => Some(("appendChild", 1)),
        "host.dom.removeChild" => Some(("removeChild", 1)),
        "host.dom.getElementById" => Some(("getElementById", 1)),
        "host.dom.getElementsByTagName" => Some(("getElementsByTagName", 1)),
        "host.dom.getElementsByClassName" => Some(("getElementsByClassName", 1)),
        "host.dom.createElement" => Some(("createElement", 1)),
        "host.dom.createTextNode" => Some(("createTextNode", 1)),
        CREATE_DOCUMENT_FRAGMENT => Some(("createDocumentFragment", 0)),
        CLONE_NODE => Some(("cloneNode", 0)),
        INSERT_BEFORE => Some(("insertBefore", 2)),
        REPLACE_CHILD => Some(("replaceChild", 2)),
        REMOVE => Some(("remove", 0)),
        BEFORE => Some(("before", 0)),
        AFTER => Some(("after", 0)),
        REPLACE_WITH => Some(("replaceWith", 0)),
        PREPEND => Some(("prepend", 0)),
        APPEND => Some(("append", 0)),
        REPLACE_CHILDREN => Some(("replaceChildren", 0)),
        CONTAINS => Some(("contains", 1)),
        HAS_CHILD_NODES => Some(("hasChildNodes", 0)),
        HAS_ATTRIBUTE => Some(("hasAttribute", 1)),
        TOGGLE_ATTRIBUTE => Some(("toggleAttribute", 1)),
        GET_ATTRIBUTE_NAMES => Some(("getAttributeNames", 0)),
        INSERT_ADJACENT_HTML => Some(("insertAdjacentHTML", 2)),
        INSERT_ADJACENT_ELEMENT => Some(("insertAdjacentElement", 2)),
        INSERT_ADJACENT_TEXT => Some(("insertAdjacentText", 2)),
        DIALOG_SHOW => Some(("show", 0)),
        DIALOG_SHOW_MODAL => Some(("showModal", 0)),
        DIALOG_CLOSE => Some(("close", 0)),
        DOM_PARSER_PARSE => Some(("parseFromString", 2)),
        CSS_SUPPORTS => Some(("supports", 1)),
        CSS_ESCAPE => Some(("escape", 1)),
        COMPARE_DOCUMENT_POSITION => Some(("compareDocumentPosition", 1)),
        GET_ROOT_NODE => Some(("getRootNode", 0)),
        CREATE_TREE_WALKER => Some(("createTreeWalker", 1)),
        DOC_HAS_FOCUS => Some(("hasFocus", 0)),
        DOC_WRITE => Some(("write", 0)),
        DOC_WRITELN => Some(("writeln", 0)),
        DOC_OPEN => Some(("open", 0)),
        DOC_CLOSE => Some(("close", 0)),
        IS_SAME_NODE => Some(("isSameNode", 1)),
        IS_EQUAL_NODE => Some(("isEqualNode", 1)),
        NORMALIZE => Some(("normalize", 0)),
        LOOKUP_NAMESPACE_URI => Some(("lookupNamespaceURI", 1)),
        LOOKUP_PREFIX => Some(("lookupPrefix", 1)),
        IS_DEFAULT_NAMESPACE => Some(("isDefaultNamespace", 1)),
        HAS_ATTRIBUTES => Some(("hasAttributes", 0)),
        GET_ATTRIBUTE_NS => Some(("getAttributeNS", 2)),
        SET_ATTRIBUTE_NS => Some(("setAttributeNS", 3)),
        REMOVE_ATTRIBUTE_NS => Some(("removeAttributeNS", 2)),
        HAS_ATTRIBUTE_NS => Some(("hasAttributeNS", 2)),
        GET_ELEMENTS_BY_TAG_NAME_NS => Some(("getElementsByTagNameNS", 2)),
        GET_ATTRIBUTE_NODE => Some(("getAttributeNode", 1)),
        GET_ATTRIBUTE_NODE_NS => Some(("getAttributeNodeNS", 2)),
        SET_ATTRIBUTE_NODE => Some(("setAttributeNode", 1)),
        SET_ATTRIBUTE_NODE_NS => Some(("setAttributeNodeNS", 1)),
        REMOVE_ATTRIBUTE_NODE => Some(("removeAttributeNode", 1)),
        ATTACH_SHADOW => Some(("attachShadow", 1)),
        CHECK_VISIBILITY => Some(("checkVisibility", 0)),
        REQUEST_FULLSCREEN => Some(("requestFullscreen", 0)),
        EL_SCROLL => Some(("scroll", 0)),
        EL_SCROLL_TO => Some(("scrollTo", 0)),
        EL_SCROLL_BY => Some(("scrollBy", 0)),
        EL_SCROLL_INTO_VIEW_IF_NEEDED => Some(("scrollIntoViewIfNeeded", 0)),
        IMG_DECODE => Some(("decode", 0)),
        _ => None,
    }
}

pub(super) fn install_globals(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();

    // 1. Install `DOMParser` constructor
    let parse_fn = method(DOM_PARSER_PARSE, context).map_err(|_| "DOMParser method failed")?;
    let parser_proto = JsObject::with_object_proto(context.intrinsics());
    def_prop(
        &parser_proto,
        js_string!("parseFromString"),
        parse_fn,
        true,
        false,
        true,
        context,
    )
    .map_err(|_| "DOMParser prototype failed")?;
    let parser_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure(|new_target, _, context| {
            modern::check_context(context)?;
            if new_target.is_undefined() {
                return Err(failure("DOMParser constructor requires 'new'"));
            }
            let proto = new_target
                .as_object()
                .and_then(|o| o.get(js_string!("prototype"), context).ok())
                .and_then(|v| v.as_object())
                .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
            Ok(JsObject::from_proto_and_data(proto, ()).into())
        }),
    )
    .name(js_string!("DOMParser"))
    .length(0)
    .constructor(true)
    .build()
    .into();
    link_ctor_global(&global, "DOMParser", &parser_ctor, &parser_proto, context)?;

    // 2. Install `window.CSS = { supports, escape }`
    let css_obj = JsObject::with_object_proto(context.intrinsics());
    let supports_fn = method(CSS_SUPPORTS, context).map_err(|_| "CSS.supports failed")?;
    let escape_fn = method(CSS_ESCAPE, context).map_err(|_| "CSS.escape failed")?;
    def_prop(
        &css_obj,
        js_string!("supports"),
        supports_fn,
        true,
        true,
        true,
        context,
    )
    .map_err(|_| "CSS.supports define failed")?;
    def_prop(
        &css_obj,
        js_string!("escape"),
        escape_fn,
        true,
        true,
        true,
        context,
    )
    .map_err(|_| "CSS.escape define failed")?;
    def_prop(
        &global,
        js_string!("CSS"),
        css_obj,
        true,
        false,
        true,
        context,
    )
    .map_err(|_| "Global CSS failed")?;

    // 3. Install DOM interface constructors, NodeFilter, and Observer stubs
    install_dom_interfaces(&global, context)?;

    // 4. Install WindowProperties named-id fallback Proxy in `window`'s prototype chain:
    // global -> Window.prototype -> named_props_proxy -> original_proto (Object.prototype)
    let orig_proto = global.prototype();
    let target = JsObject::from_proto_and_data(orig_proto, ());
    let named_proxy: JsObject = JsProxy::builder(target)
        .get(window_named_get)
        .has(window_named_has)
        .build(context)
        .map_err(|_| "WindowProperties proxy failed")?
        .into();
    let window_proto = state(context)
        .borrow()
        .dom_prototypes
        .as_ref()
        .and_then(|dp| dp.proto("Window"));
    if let Some(wp) = window_proto {
        wp.set_prototype(Some(named_proxy));
        global.set_prototype(Some(wp));
    } else {
        global.set_prototype(Some(named_proxy));
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum DomInterfaceKind {
    Window,
    Navigator,
    PluginArray,
    MimeTypeArray,
    Location,
    History,
    Node,
    Document,
    DocumentFragment,
    ShadowRoot,
    CharacterData,
    Text,
    Comment,
    Element,
    HtmlElement,
    SvgElement,
    MediaElement,
    Tag(&'static str),
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct CachedIframeWindow {
    #[unsafe_ignore_trace]
    pub iframe_id: usize,
    #[unsafe_ignore_trace]
    pub sub_id: usize,
    pub window: JsObject,
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct DomPrototypes {
    pub protos: Vec<JsObject>,
    pub empty_plugins: JsObject,
    pub empty_mime_types: JsObject,
    pub iframe_windows: Vec<CachedIframeWindow>,
    #[unsafe_ignore_trace]
    pub initial_window_keys: Vec<String>,
}

impl DomPrototypes {
    pub(super) fn proto(&self, name: &str) -> Option<JsObject> {
        DOM_INTERFACES
            .iter()
            .position(|(n, _)| *n == name)
            .and_then(|i| self.protos.get(i).cloned())
    }
}

pub(super) fn is_svg_element(nodes: &[Node], mut id: usize) -> bool {
    for _ in 0..=crate::document::MAX_DEPTH {
        if id == 0 || id >= nodes.len() {
            return false;
        }
        let tag = nodes[id].tag.as_str();
        if tag == "svg" {
            return true;
        }
        if tag == "foreignobject" {
            return false;
        }
        let p = nodes[id].parent;
        if p == id {
            return false;
        }
        id = p;
    }
    false
}

pub(super) fn effective_input_type(node: &Node) -> &'static str {
    let Some(raw) = node.attr("type") else {
        return "text";
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "hidden" => "hidden",
        "text" => "text",
        "search" => "search",
        "tel" => "tel",
        "url" => "url",
        "email" => "email",
        "password" => "password",
        "date" => "date",
        "month" => "month",
        "week" => "week",
        "time" => "time",
        "datetime-local" => "datetime-local",
        "number" => "number",
        "range" => "range",
        "color" => "color",
        "checkbox" => "checkbox",
        "radio" => "radio",
        "file" => "file",
        "submit" => "submit",
        "image" => "image",
        "reset" => "reset",
        "button" => "button",
        _ => "text",
    }
}

fn node_constant(name: &str) -> Option<i32> {
    match name {
        "ELEMENT_NODE" => Some(1),
        "ATTRIBUTE_NODE" => Some(2),
        "TEXT_NODE" => Some(3),
        "CDATA_SECTION_NODE" => Some(4),
        "ENTITY_REFERENCE_NODE" => Some(5),
        "ENTITY_NODE" => Some(6),
        "PROCESSING_INSTRUCTION_NODE" => Some(7),
        "COMMENT_NODE" => Some(8),
        "DOCUMENT_NODE" => Some(9),
        "DOCUMENT_TYPE_NODE" => Some(10),
        "DOCUMENT_FRAGMENT_NODE" => Some(11),
        "NOTATION_NODE" => Some(12),
        "DOCUMENT_POSITION_DISCONNECTED" => Some(1),
        "DOCUMENT_POSITION_PRECEDING" => Some(2),
        "DOCUMENT_POSITION_FOLLOWING" => Some(4),
        "DOCUMENT_POSITION_CONTAINS" => Some(8),
        "DOCUMENT_POSITION_CONTAINED_BY" => Some(16),
        "DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC" => Some(32),
        _ => None,
    }
}

fn matches_dom_interface(
    kind: DomInterfaceKind,
    val: &JsValue,
    context: &mut Context,
) -> JsResult<bool> {
    let Some(obj) = val.as_object() else {
        return Ok(false);
    };
    if matches!(kind, DomInterfaceKind::Window) {
        if obj == context.global_object() {
            return Ok(true);
        }
        return Ok(state(context)
            .borrow()
            .dom_prototypes
            .as_ref()
            .is_some_and(|dp| dp.iframe_windows.iter().any(|e| e.window == obj)));
    }
    if matches!(kind, DomInterfaceKind::PluginArray) {
        return Ok(state(context)
            .borrow()
            .dom_prototypes
            .as_ref()
            .is_some_and(|dp| dp.empty_plugins == obj));
    }
    if matches!(kind, DomInterfaceKind::MimeTypeArray) {
        return Ok(state(context)
            .borrow()
            .dom_prototypes
            .as_ref()
            .is_some_and(|dp| dp.empty_mime_types == obj));
    }
    if matches!(
        kind,
        DomInterfaceKind::ShadowRoot | DomInterfaceKind::Comment
    ) {
        return Ok(false);
    }
    let brands = state(context).borrow().brands.clone();
    let Some(h_str) = brands
        .get(&obj, context)?
        .as_string()
        .map(|s| s.to_std_string_escaped())
    else {
        return Ok(false);
    };
    if matches!(kind, DomInterfaceKind::Navigator) {
        return Ok(h_str == "navigator");
    }
    if matches!(kind, DomInterfaceKind::Location) {
        return Ok(h_str == "location");
    }
    if matches!(kind, DomInterfaceKind::History) {
        return Ok(h_str == "history");
    }
    if h_str == "document" || h_str.starts_with("subdoc:") {
        return Ok(matches!(
            kind,
            DomInterfaceKind::Node | DomInterfaceKind::Document
        ));
    }
    if !h_str.starts_with("node:") {
        return Ok(false);
    }
    let shared = state(context);
    let data = shared.borrow();
    let Ok(id) = data.host.node(&h_str) else {
        return Ok(false);
    };
    let tag = data.host.document.nodes[id].tag.as_str();
    match kind {
        DomInterfaceKind::Node => Ok(true),
        DomInterfaceKind::DocumentFragment => Ok(tag == "#document-fragment"),
        DomInterfaceKind::CharacterData | DomInterfaceKind::Text => Ok(tag == "#text"),
        DomInterfaceKind::Element => Ok(!tag.starts_with('#')),
        DomInterfaceKind::HtmlElement => {
            Ok(!tag.starts_with('#') && !is_svg_element(&data.host.document.nodes, id))
        }
        DomInterfaceKind::SvgElement => {
            Ok(!tag.starts_with('#') && is_svg_element(&data.host.document.nodes, id))
        }
        DomInterfaceKind::MediaElement => {
            Ok(matches!(tag, "audio" | "video") && !is_svg_element(&data.host.document.nodes, id))
        }
        DomInterfaceKind::Tag(expected) => {
            Ok(tag == expected && !is_svg_element(&data.host.document.nodes, id))
        }
        DomInterfaceKind::Window
        | DomInterfaceKind::Navigator
        | DomInterfaceKind::PluginArray
        | DomInterfaceKind::MimeTypeArray
        | DomInterfaceKind::Location
        | DomInterfaceKind::History
        | DomInterfaceKind::Document
        | DomInterfaceKind::ShadowRoot
        | DomInterfaceKind::Comment => Ok(false),
    }
}

const DOM_INTERFACES: &[(&str, DomInterfaceKind)] = &[
    ("Window", DomInterfaceKind::Window),
    ("Navigator", DomInterfaceKind::Navigator),
    ("PluginArray", DomInterfaceKind::PluginArray),
    ("MimeTypeArray", DomInterfaceKind::MimeTypeArray),
    ("Location", DomInterfaceKind::Location),
    ("History", DomInterfaceKind::History),
    ("Node", DomInterfaceKind::Node),
    ("Document", DomInterfaceKind::Document),
    ("HTMLDocument", DomInterfaceKind::Document),
    ("DocumentFragment", DomInterfaceKind::DocumentFragment),
    ("ShadowRoot", DomInterfaceKind::ShadowRoot),
    ("CharacterData", DomInterfaceKind::CharacterData),
    ("Text", DomInterfaceKind::Text),
    ("Comment", DomInterfaceKind::Comment),
    ("Element", DomInterfaceKind::Element),
    ("HTMLElement", DomInterfaceKind::HtmlElement),
    ("SVGElement", DomInterfaceKind::SvgElement),
    ("HTMLHtmlElement", DomInterfaceKind::Tag("html")),
    ("HTMLHeadElement", DomInterfaceKind::Tag("head")),
    ("HTMLBodyElement", DomInterfaceKind::Tag("body")),
    ("HTMLDivElement", DomInterfaceKind::Tag("div")),
    ("HTMLSpanElement", DomInterfaceKind::Tag("span")),
    ("HTMLScriptElement", DomInterfaceKind::Tag("script")),
    ("HTMLStyleElement", DomInterfaceKind::Tag("style")),
    ("HTMLLinkElement", DomInterfaceKind::Tag("link")),
    ("HTMLMetaElement", DomInterfaceKind::Tag("meta")),
    ("HTMLMediaElement", DomInterfaceKind::MediaElement),
    ("HTMLAudioElement", DomInterfaceKind::Tag("audio")),
    ("HTMLVideoElement", DomInterfaceKind::Tag("video")),
    ("HTMLInputElement", DomInterfaceKind::Tag("input")),
    ("HTMLTextAreaElement", DomInterfaceKind::Tag("textarea")),
    ("HTMLSelectElement", DomInterfaceKind::Tag("select")),
    ("HTMLButtonElement", DomInterfaceKind::Tag("button")),
    ("HTMLOptionElement", DomInterfaceKind::Tag("option")),
    ("HTMLOptGroupElement", DomInterfaceKind::Tag("optgroup")),
    ("HTMLLabelElement", DomInterfaceKind::Tag("label")),
    ("HTMLFormElement", DomInterfaceKind::Tag("form")),
    ("HTMLAnchorElement", DomInterfaceKind::Tag("a")),
    ("HTMLImageElement", DomInterfaceKind::Tag("img")),
    ("HTMLIFrameElement", DomInterfaceKind::Tag("iframe")),
    ("HTMLFrameElement", DomInterfaceKind::Tag("frame")),
    ("HTMLSlotElement", DomInterfaceKind::Tag("slot")),
    ("HTMLDetailsElement", DomInterfaceKind::Tag("details")),
    ("HTMLDialogElement", DomInterfaceKind::Tag("dialog")),
    ("HTMLCanvasElement", DomInterfaceKind::Tag("canvas")),
];

fn iframe_element_id(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(this, context)? else {
        return Err(failure("Illegal invocation"));
    };
    if !handle.starts_with("node:") {
        return Err(failure("Illegal invocation"));
    }
    let shared = state(context);
    let data = shared.borrow();
    let id = data
        .host
        .node(&handle)
        .map_err(|_| failure("Illegal invocation"))?;
    if id == 0
        || id >= data.host.document.nodes.len()
        || data.host.document.nodes[id].tag != "iframe"
        || is_svg_element(&data.host.document.nodes, id)
    {
        return Err(failure("Illegal invocation"));
    }
    Ok(id)
}

pub(super) fn clear_cached_iframe_window(data: &mut State, iframe_id: usize) {
    if let Some(dp) = data.dom_prototypes.as_mut() {
        dp.iframe_windows.retain(|e| e.iframe_id != iframe_id);
    }
}

pub(super) fn prototype_for_handle(data: &State, handle: &str) -> Option<JsObject> {
    let dp = data.dom_prototypes.as_ref()?;
    match handle {
        "document" => return dp.proto("HTMLDocument"),
        "navigator" => return dp.proto("Navigator"),
        "location" => return dp.proto("Location"),
        "history" => return dp.proto("History"),
        _ if handle.starts_with("subdoc:") => return dp.proto("HTMLDocument"),
        _ => {}
    }
    let nid = handle.strip_prefix("node:")?.parse::<usize>().ok()?;
    let tag = data.host.document.nodes.get(nid)?.tag.as_str();
    match tag {
        "#document" | "#document-subroot" => dp.proto("HTMLDocument"),
        "#document-fragment" => dp.proto("DocumentFragment"),
        "#shadow-root" => dp.proto("ShadowRoot"),
        "#text" => dp.proto("Text"),
        "#comment" => dp.proto("Comment"),
        _ if tag.starts_with('#') => None,
        _ if is_svg_element(&data.host.document.nodes, nid) => dp.proto("SVGElement"),
        _ => DOM_INTERFACES
            .iter()
            .position(|(_, k)| matches!(k, DomInterfaceKind::Tag(t) if *t == tag))
            .and_then(|i| dp.protos.get(i).cloned())
            .or_else(|| dp.proto("HTMLElement")),
    }
}

fn install_dom_interfaces(global: &JsObject, context: &mut Context) -> Result<(), String> {
    let has_instance_sym = modern::boa_engine::JsSymbol::has_instance();
    let to_string_tag_sym = modern::boa_engine::JsSymbol::to_string_tag();
    let mut created: HashMap<&'static str, (JsObject, JsObject)> =
        HashMap::with_capacity(DOM_INTERFACES.len());
    let mut protos: Vec<JsObject> = Vec::with_capacity(DOM_INTERFACES.len());

    for (idx, &(iface_name, kind)) in DOM_INTERFACES.iter().enumerate() {
        let parent_name = match iface_name {
            "Window" | "Navigator" | "PluginArray" | "MimeTypeArray" | "Location" | "History"
            | "Node" => None,
            "Document" | "DocumentFragment" | "CharacterData" | "Element" => Some("Node"),
            "HTMLDocument" => Some("Document"),
            "ShadowRoot" => Some("DocumentFragment"),
            "Text" | "Comment" => Some("CharacterData"),
            "HTMLElement" | "SVGElement" => Some("Element"),
            "HTMLAudioElement" | "HTMLVideoElement" => Some("HTMLMediaElement"),
            _ => Some("HTMLElement"),
        };
        let parent_pair = parent_name.and_then(|pname| created.get(pname).cloned());
        let proto = match &parent_pair {
            Some((_, parent_proto)) => {
                JsObject::from_proto_and_data(Some(parent_proto.clone()), ())
            }
            None => JsObject::with_object_proto(context.intrinsics()),
        };
        let ctor_fn = if iface_name == "HTMLElement" {
            NativeFunction::from_fn_ptr(|new_target, _, context| {
                super::observers_wc::construct_html_element(new_target, context)
            })
        } else {
            NativeFunction::from_copy_closure(|_, _, context| {
                modern::check_context(context)?;
                Err(failure("Illegal constructor"))
            })
        };
        let ctor: JsObject = FunctionObjectBuilder::new(context.realm(), ctor_fn)
            .name(JsString::from(iface_name))
            .length(0)
            .constructor(true)
            .build()
            .into();
        if let Some((parent_ctor, _)) = &parent_pair {
            ctor.set_prototype(Some(parent_ctor.clone()));
        }
        def_prop(
            &proto,
            to_string_tag_sym.clone(),
            JsString::from(iface_name),
            false,
            false,
            true,
            context,
        )
        .map_err(|_| "DOM interface Symbol.toStringTag failed")?;

        if iface_name == "Window" {
            install_window_prototype_members(&proto, context)?;
        }

        if iface_name == "PluginArray" || iface_name == "MimeTypeArray" {
            let is_plugins = iface_name == "PluginArray";
            let check_pm = |this: &JsValue, is_p: bool, ctx: &mut Context| -> JsResult<()> {
                modern::check_context(ctx)?;
                let Some(obj) = this.as_object() else {
                    return Err(failure("Illegal invocation"));
                };
                let ok = state(ctx)
                    .borrow()
                    .dom_prototypes
                    .as_ref()
                    .is_some_and(|dp| {
                        if is_p {
                            dp.empty_plugins == obj
                        } else {
                            dp.empty_mime_types == obj
                        }
                    });
                if ok {
                    Ok(())
                } else {
                    Err(failure("Illegal invocation"))
                }
            };
            def_getter_cap(
                &proto,
                "length",
                true,
                (is_plugins, check_pm),
                |this, _, &(is_p, chk), ctx| {
                    chk(this, is_p, ctx)?;
                    Ok(0.into())
                },
                context,
            )
            .map_err(|_| "PluginArray/MimeTypeArray length failed")?;
            for mname in ["item", "namedItem"] {
                def_cap(
                    &proto,
                    mname,
                    1,
                    true,
                    (is_plugins, check_pm),
                    |this, _, &(is_p, chk), ctx| {
                        chk(this, is_p, ctx)?;
                        Ok(JsValue::null())
                    },
                    context,
                )
                .map_err(|_| "PluginArray/MimeTypeArray method failed")?;
            }
            if is_plugins {
                def_cap(
                    &proto,
                    "refresh",
                    0,
                    true,
                    check_pm,
                    |this, _, &chk, ctx| {
                        chk(this, true, ctx)?;
                        Ok(JsValue::undefined())
                    },
                    context,
                )
                .map_err(|_| "PluginArray.refresh failed")?;
            }
            if let Ok(arr_iter) = context
                .intrinsics()
                .constructors()
                .array()
                .prototype()
                .get(modern::boa_engine::JsSymbol::iterator(), context)
            {
                let _ = def_prop(
                    &proto,
                    modern::boa_engine::JsSymbol::iterator(),
                    arr_iter,
                    true,
                    false,
                    true,
                    context,
                );
            }
        }

        if iface_name == "Navigator" {
            for prop in [
                "webdriver",
                "plugins",
                "mimeTypes",
                "languages",
                "language",
                "userAgent",
                "appVersion",
                "platform",
                "vendor",
                "product",
                "productSub",
                "appCodeName",
                "appName",
                "hardwareConcurrency",
                "maxTouchPoints",
                "cookieEnabled",
                "onLine",
                "pdfViewerEnabled",
            ] {
                def_getter_cap(
                    &proto,
                    prop,
                    true,
                    prop,
                    |this, _, &p, ctx| {
                        modern::check_context(ctx)?;
                        if !matches!(receiver(this, ctx)?, Value::Host(ref h) if h == "navigator") {
                            return Err(failure("Illegal invocation"));
                        }
                        match p {
                            "webdriver" | "pdfViewerEnabled" => Ok(false.into()),
                            "cookieEnabled" | "onLine" => Ok(true.into()),
                            "hardwareConcurrency" => Ok(4.into()),
                            "maxTouchPoints" => Ok(0.into()),
                            "userAgent" => Ok(JsString::from(crate::USER_AGENT).into()),
                            "appVersion" => Ok(JsString::from(
                                crate::USER_AGENT
                                    .strip_prefix("Mozilla/")
                                    .unwrap_or(crate::USER_AGENT),
                            )
                            .into()),
                            "language" => Ok(js_string!("en-US").into()),
                            "languages" => {
                                Ok(modern::boa_engine::object::builtins::JsArray::from_iter(
                                    [
                                        JsValue::from(js_string!("en-US")),
                                        JsValue::from(js_string!("en")),
                                    ],
                                    ctx,
                                )
                                .into())
                            }
                            "platform" => Ok(js_string!("Linux").into()),
                            "vendor" => Ok(js_string!("").into()),
                            "product" => Ok(js_string!("Gecko").into()),
                            "productSub" => Ok(js_string!("20030107").into()),
                            "appCodeName" => Ok(js_string!("Mozilla").into()),
                            "appName" => Ok(js_string!("Netscape").into()),
                            "plugins" => state(ctx)
                                .borrow()
                                .dom_prototypes
                                .as_ref()
                                .map(|dp| dp.empty_plugins.clone().into())
                                .ok_or_else(|| failure("Missing PluginArray")),
                            "mimeTypes" => state(ctx)
                                .borrow()
                                .dom_prototypes
                                .as_ref()
                                .map(|dp| dp.empty_mime_types.clone().into())
                                .ok_or_else(|| failure("Missing MimeTypeArray")),
                            _ => Ok(JsValue::undefined()),
                        }
                    },
                    context,
                )
                .map_err(|_| "Navigator.prototype getter failed")?;
            }
            let send_beacon_fn = super::method(super::web_apis::NAVIGATOR_SEND_BEACON, context)
                .map_err(|_| "Navigator.prototype.sendBeacon failed")?;
            def_prop(
                &proto,
                js_string!("sendBeacon"),
                send_beacon_fn,
                true,
                true,
                true,
                context,
            )
            .map_err(|_| "Navigator.prototype.sendBeacon define failed")?;
            def_fn(
                &proto,
                "javaEnabled",
                0,
                true,
                |this, _, ctx| {
                    modern::check_context(ctx)?;
                    if !matches!(receiver(this, ctx)?, Value::Host(ref h) if h == "navigator") {
                        return Err(failure("Illegal invocation"));
                    }
                    Ok(false.into())
                },
                context,
            )
            .map_err(|_| "Navigator.prototype.javaEnabled failed")?;
        }

        if iface_name == "HTMLMediaElement" {
            def_fn(
                &proto,
                "canPlayType",
                1,
                true,
                |this, args, context| {
                    modern::check_context(context)?;
                    let Value::Host(handle) = receiver(this, context)? else {
                        return Err(failure("Illegal invocation"));
                    };
                    let ok = {
                        let shared = state(context);
                        let data = shared.borrow();
                        handle.starts_with("node:")
                            && data.host.node(&handle).ok().is_some_and(|id| {
                                id != 0
                                    && id < data.host.document.nodes.len()
                                    && matches!(
                                        data.host.document.nodes[id].tag.as_str(),
                                        "audio" | "video"
                                    )
                                    && !is_svg_element(&data.host.document.nodes, id)
                            })
                    };
                    if !ok {
                        return Err(failure("Illegal invocation"));
                    }
                    let Some(arg0) = args.first() else {
                        return Err(failure("HTMLMediaElement.canPlayType requires 1 argument"));
                    };
                    let _ = utf8(arg0, context)?;
                    modern::check_context(context)?;
                    Ok(js_string!("").into())
                },
                context,
            )
            .map_err(|_| "HTMLMediaElement.prototype.canPlayType failed")?;
        }

        if iface_name == "HTMLIFrameElement" {
            def_getter_fn(
                &proto,
                "contentWindow",
                true,
                |this, _, context| {
                    modern::check_context(context)?;
                    iframe_content_window(iframe_element_id(this, context)?, context)
                },
                context,
            )
            .map_err(|_| "HTMLIFrameElement.prototype.contentWindow failed")?;
            def_getter_fn(
                &proto,
                "contentDocument",
                true,
                |this, _, context| {
                    modern::check_context(context)?;
                    iframe_content_document(iframe_element_id(this, context)?, context)
                },
                context,
            )
            .map_err(|_| "HTMLIFrameElement.prototype.contentDocument failed")?;
            def_get_set_fn(
                &proto,
                "srcdoc",
                true,
                |this, _, context| {
                    modern::check_context(context)?;
                    let id = iframe_element_id(this, context)?;
                    let val = state(context).borrow().host.document.nodes[id]
                        .attr("srcdoc")
                        .unwrap_or("")
                        .to_owned();
                    Ok(JsString::from(val.as_str()).into())
                },
                |this, args, context| {
                    modern::check_context(context)?;
                    let id = iframe_element_id(this, context)?;
                    let Some(val) = args.first() else {
                        return Err(failure(
                            "HTMLIFrameElement.srcdoc setter requires 1 argument",
                        ));
                    };
                    let text = utf8(val, context)?;
                    modern::check_context(context)?;
                    set_element_attr_observed(id, "srcdoc", text, context)?;
                    Ok(JsValue::undefined())
                },
                context,
            )
            .map_err(|_| "HTMLIFrameElement.prototype.srcdoc failed")?;
        }

        let has_inst = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |this, args, &i, ctx| {
                    modern::check_context(ctx)?;
                    let v = args.first().cloned().unwrap_or_default();
                    if let Some(this_obj) = this.as_object()
                        && !this_obj
                            .has_own_property(modern::boa_engine::JsSymbol::has_instance(), ctx)?
                    {
                        return modern::boa_engine::JsValue::ordinary_has_instance(this, &v, ctx)
                            .map(Into::into);
                    }
                    matches_dom_interface(DOM_INTERFACES[i].1, &v, ctx).map(Into::into)
                },
                idx,
            ),
        )
        .name(js_string!("[Symbol.hasInstance]"))
        .length(1)
        .build();
        def_prop(
            &ctor,
            has_instance_sym.clone(),
            has_inst,
            false,
            false,
            true,
            context,
        )
        .map_err(|_| "DOM interface Symbol.hasInstance failed")?;

        if matches!(kind, DomInterfaceKind::Node) {
            for cname in [
                "ELEMENT_NODE",
                "ATTRIBUTE_NODE",
                "TEXT_NODE",
                "CDATA_SECTION_NODE",
                "ENTITY_REFERENCE_NODE",
                "ENTITY_NODE",
                "PROCESSING_INSTRUCTION_NODE",
                "COMMENT_NODE",
                "DOCUMENT_NODE",
                "DOCUMENT_TYPE_NODE",
                "DOCUMENT_FRAGMENT_NODE",
                "NOTATION_NODE",
                "DOCUMENT_POSITION_DISCONNECTED",
                "DOCUMENT_POSITION_PRECEDING",
                "DOCUMENT_POSITION_FOLLOWING",
                "DOCUMENT_POSITION_CONTAINS",
                "DOCUMENT_POSITION_CONTAINED_BY",
                "DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC",
            ] {
                let cval = node_constant(cname).unwrap_or(0);
                for obj in [&ctor, &proto] {
                    def_prop(
                        obj,
                        JsString::from(cname),
                        cval,
                        false,
                        true,
                        false,
                        context,
                    )
                    .map_err(|_| "Node constant failed")?;
                }
            }
        }

        link_ctor_global(global, iface_name, &ctor, &proto, context)?;

        if iface_name == "HTMLImageElement" {
            let image_ctor: JsObject = FunctionObjectBuilder::new(
                context.realm(),
                NativeFunction::from_copy_closure(|new_target, args, context| {
                    modern::check_context(context)?;
                    if new_target.is_undefined() {
                        return Err(failure("Image constructor requires 'new'"));
                    }
                    let w_arg = match args.first() {
                        Some(v) if !v.is_undefined() => Some(v.to_u32(context)?),
                        _ => None,
                    };
                    let h_arg = match args.get(1) {
                        Some(v) if !v.is_undefined() => Some(v.to_u32(context)?),
                        _ => None,
                    };
                    modern::check_context(context)?;
                    let id = {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        let id = data
                            .host
                            .new_node("img".to_owned(), String::new())
                            .map_err(failure)?;
                        if let Some(w) = w_arg {
                            data.host.document.nodes[id]
                                .attributes
                                .push(("width".to_owned(), w.to_string()));
                        }
                        if let Some(h) = h_arg {
                            data.host.document.nodes[id]
                                .attributes
                                .push(("height".to_owned(), h.to_string()));
                        }
                        id
                    };
                    wrap(&format!("node:{id}"), context)
                }),
            )
            .name(js_string!("Image"))
            .length(0)
            .constructor(true)
            .build()
            .into();
            def_prop(
                &image_ctor,
                js_string!("prototype"),
                proto.clone(),
                false,
                false,
                false,
                context,
            )
            .map_err(|_| "Image.prototype failed")?;
            if let Ok(hi) = ctor.get(has_instance_sym.clone(), context) {
                let _ = def_prop(
                    &image_ctor,
                    has_instance_sym.clone(),
                    hi,
                    false,
                    false,
                    true,
                    context,
                );
            }
            def_prop(
                global,
                js_string!("Image"),
                image_ctor,
                true,
                false,
                true,
                context,
            )
            .map_err(|_| "Global Image failed")?;
        }
        protos.push(proto.clone());
        created.insert(iface_name, (ctor, proto));
    }

    let get_proto = |name: &'static str| -> Result<JsObject, &'static str> {
        created
            .get(name)
            .map(|(_, p)| p.clone())
            .ok_or("Missing DOM interface prototype")
    };
    let empty_plugins = JsObject::from_proto_and_data(Some(get_proto("PluginArray")?), ());
    let empty_mime_types = JsObject::from_proto_and_data(Some(get_proto("MimeTypeArray")?), ());
    state(context).borrow_mut().dom_prototypes = Some(DomPrototypes {
        protos,
        empty_plugins,
        empty_mime_types,
        iframe_windows: Vec::new(),
        initial_window_keys: Vec::new(),
    });

    // Install `NodeFilter`
    let nf = JsObject::with_object_proto(context.intrinsics());
    for (k, v) in [
        ("FILTER_ACCEPT", 1f64),
        ("FILTER_REJECT", 2.0),
        ("FILTER_SKIP", 3.0),
        ("SHOW_ALL", 0xFFFF_FFFFu32 as f64),
        ("SHOW_ELEMENT", 0x1 as f64),
        ("SHOW_ATTRIBUTE", 0x2 as f64),
        ("SHOW_TEXT", 0x4 as f64),
        ("SHOW_CDATA_SECTION", 0x8 as f64),
        ("SHOW_PROCESSING_INSTRUCTION", 0x40 as f64),
        ("SHOW_COMMENT", 0x80 as f64),
        ("SHOW_DOCUMENT", 0x100 as f64),
        ("SHOW_DOCUMENT_TYPE", 0x200 as f64),
        ("SHOW_DOCUMENT_FRAGMENT", 0x400 as f64),
    ] {
        def_prop(&nf, JsString::from(k), v, false, true, false, context)
            .map_err(|_| "NodeFilter constant failed")?;
    }
    def_prop(
        global,
        js_string!("NodeFilter"),
        nf,
        true,
        false,
        true,
        context,
    )
    .map_err(|_| "Global NodeFilter failed")?;

    Ok(())
}

fn window_named_get(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let prop = key(&args[1], context)?;
    let target_obj = args[0]
        .as_object()
        .ok_or_else(|| failure("Invalid WindowProperties target"))?;
    // Prototype properties on Object.prototype ALWAYS win over named elements!
    if target_obj.has_property(prop.clone(), context)? {
        let reflect_get = state(context).borrow().reflect_get.clone();
        return reflect_get.call(&JsValue::undefined(), args, context);
    }
    let is_top_window = args
        .get(2)
        .and_then(JsValue::as_object)
        .is_some_and(|r| r == context.global_object());
    if !is_top_window {
        return Ok(JsValue::undefined());
    }
    if let PropertyKey::String(s) = &prop {
        let name = s.to_std_string_escaped();
        if !name.is_empty() {
            let found = state(context)
                .borrow()
                .host
                .find_connected(|n| !n.tag.starts_with('#') && n.attr("id") == Some(name.as_str()));
            if let Some(id) = found {
                return wrap(&format!("node:{id}"), context);
            }
        }
    }
    Ok(JsValue::undefined())
}

fn window_named_has(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let prop = key(&args[1], context)?;
    let target_obj = args[0]
        .as_object()
        .ok_or_else(|| failure("Invalid WindowProperties target"))?;
    if target_obj.has_property(prop.clone(), context)? {
        return Ok(true.into());
    }
    if let PropertyKey::String(s) = &prop {
        let name = s.to_std_string_escaped();
        if !name.is_empty() {
            let found = state(context)
                .borrow()
                .host
                .find_connected(|n| !n.tag.starts_with('#') && n.attr("id") == Some(name.as_str()))
                .is_some();
            if found {
                return Ok(true.into());
            }
        }
    }
    Ok(false.into())
}

/// Move direct children of any `<template>` element in `range` into an owned `#document-fragment`.
pub(super) fn lift_template_contents(data: &mut State, start_id: usize) -> Result<(), String> {
    let end_id = data.host.document.nodes.len();
    let mut templates = Vec::new();
    for id in start_id..end_id {
        if data.host.document.nodes[id].tag == "template"
            && !data.template_contents.contains_key(&id)
        {
            templates.push(id);
        }
    }
    for tpl_id in templates {
        let frag_id = data
            .host
            .new_node("#document-fragment".into(), String::new())?;
        let old_children = std::mem::take(&mut data.host.document.nodes[tpl_id].children);
        for &ch in &old_children {
            data.host.document.nodes[ch].parent = frag_id;
        }
        data.host.document.nodes[frag_id].children = old_children;
        data.template_contents.insert(tpl_id, frag_id);
    }
    Ok(())
}

pub(super) fn ensure_template_content(data: &mut State, tpl_id: usize) -> Result<usize, String> {
    if let Some(&frag_id) = data.template_contents.get(&tpl_id) {
        return Ok(frag_id);
    }
    let frag_id = data
        .host
        .new_node("#document-fragment".into(), String::new())?;
    let old_children = std::mem::take(&mut data.host.document.nodes[tpl_id].children);
    for &ch in &old_children {
        data.host.document.nodes[ch].parent = frag_id;
    }
    data.host.document.nodes[frag_id].children = old_children;
    data.template_contents.insert(tpl_id, frag_id);
    Ok(frag_id)
}

pub(super) fn node_id_from_handle(handle: &str, data: &State) -> Result<usize, String> {
    if handle == "document" {
        return Ok(0);
    }
    if let Some(sub) = handle
        .strip_prefix("subdoc:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        if sub < data.host.document.nodes.len() {
            return Ok(sub);
        }
        return Err("Stale document handle".into());
    }
    data.host.node(handle)
}

pub(super) fn wrap_node_id(id: usize, context: &mut Context) -> JsResult<JsValue> {
    if id == 0 {
        return wrap("document", context);
    }
    let is_subdoc = state(context)
        .borrow()
        .host
        .document
        .nodes
        .get(id)
        .is_some_and(|n| n.tag == "#document-subroot");
    if is_subdoc {
        return wrap(&format!("subdoc:{id}"), context);
    }
    wrap(&format!("node:{id}"), context)
}

fn root_of(nodes: &[Node], mut id: usize) -> usize {
    for _ in 0..=crate::document::MAX_DEPTH {
        let p = nodes[id].parent;
        if id == 0 || p == id {
            return id;
        }
        id = p;
    }
    id
}

pub(super) fn is_connected_node(nodes: &[Node], id: usize) -> bool {
    root_of(nodes, id) == 0
}

fn owner_document_handle(nodes: &[Node], id: usize) -> Option<String> {
    if id == 0 || nodes[id].tag == "#document-subroot" {
        return None;
    }
    let r = root_of(nodes, id);
    if r != 0 && nodes[r].tag == "#document-subroot" {
        Some(format!("subdoc:{r}"))
    } else {
        Some("document".into())
    }
}

fn check_insert_hierarchy(
    nodes: &[Node],
    parent: usize,
    child: usize,
) -> Result<(usize, Vec<usize>), (&'static str, &'static str)> {
    let parent_tag = nodes[parent].tag.as_str();
    if parent_tag == "#text" || super::super::void(parent_tag) {
        return Err(("HierarchyRequestError", "Parent node cannot have children"));
    }
    let child_tag = nodes[child].tag.as_str();
    if child == 0 || child_tag == "#document-subroot" {
        return Err((
            "HierarchyRequestError",
            "Document nodes cannot be inserted as children",
        ));
    }
    let mut anc = parent;
    let mut parent_depth = 0usize;
    for depth in 0..=crate::document::MAX_DEPTH {
        if anc == child {
            return Err((
                "HierarchyRequestError",
                "The new child is an ancestor of the parent",
            ));
        }
        let next = nodes[anc].parent;
        if anc == 0 || next == anc {
            break;
        }
        if depth == crate::document::MAX_DEPTH {
            return Err(("HierarchyRequestError", "DOM depth limit exceeded"));
        }
        anc = next;
        parent_depth = depth + 1;
    }
    let inserting = if child_tag == "#document-fragment" {
        nodes[child].children.clone()
    } else {
        vec![child]
    };
    if parent == 0 || parent_tag == "#document-subroot" {
        for &ins in &inserting {
            if nodes[ins].tag == "#text" {
                return Err((
                    "HierarchyRequestError",
                    "Cannot insert a Text node directly under a Document",
                ));
            }
        }
    }
    for &ins in &inserting {
        let mut pending = vec![(ins, 1usize)];
        let mut visited = 0usize;
        while let Some((cur, d)) = pending.pop() {
            visited += 1;
            if visited > crate::document::MAX_NODES || parent_depth + d > crate::document::MAX_DEPTH
            {
                return Err(("HierarchyRequestError", "DOM subtree depth limit exceeded"));
            }
            pending.extend(nodes[cur].children.iter().map(|&c| (c, d + 1)));
        }
    }
    Ok((parent_depth, inserting))
}

enum DomOpErr {
    Host(String),
    Dom(&'static str, &'static str),
}

fn into_js_err(err: DomOpErr, context: &mut Context) -> JsError {
    match err {
        DomOpErr::Host(s) => failure(s),
        DomOpErr::Dom(kind, msg) => dom_exception::throw(kind, msg, context),
    }
}

fn detach_child_node(data: &mut State, parent: usize, child: usize) {
    if super::observers_wc::has_any_interest(data) {
        let was_connected = is_connected_node(&data.host.document.nodes, parent);
        let (prev_sib, next_sib) = {
            let kids = &data.host.document.nodes[parent].children;
            if let Some(pos) = kids.iter().position(|&x| x == child) {
                (
                    pos.checked_sub(1).and_then(|i| kids.get(i).copied()),
                    kids.get(pos + 1).copied(),
                )
            } else {
                (None, None)
            }
        };
        data.host.document.nodes[parent]
            .children
            .retain(|&x| x != child);
        data.host.document.nodes[child].parent = child;
        super::observers_wc::record_child_list_mutation(
            data,
            parent,
            &[],
            &[child],
            prev_sib,
            next_sib,
            was_connected,
        );
    } else {
        data.host.document.nodes[parent]
            .children
            .retain(|&x| x != child);
        data.host.document.nodes[child].parent = child;
    }
}

fn insert_node_before(
    data: &mut State,
    parent: usize,
    new_child: usize,
    ref_child: Option<usize>,
) -> Result<usize, DomOpErr> {
    if let Some(rc) = ref_child
        && !data.host.document.nodes[parent].children.contains(&rc)
    {
        return Err(DomOpErr::Dom(
            "NotFoundError",
            "The node before which the new node is to be inserted is not a child of this node",
        ));
    }
    let (_, inserting) = match check_insert_hierarchy(&data.host.document.nodes, parent, new_child)
    {
        Ok(ok) => ok,
        Err((kind, msg)) => return Err(DomOpErr::Dom(kind, msg)),
    };
    if Some(new_child) == ref_child {
        return Ok(new_child);
    }
    let track = super::observers_wc::has_any_interest(data);
    if data.host.document.nodes[new_child].tag == "#document-fragment" {
        if track && !data.host.document.nodes[new_child].children.is_empty() {
            let removed = std::mem::take(&mut data.host.document.nodes[new_child].children);
            super::observers_wc::record_child_list_mutation(
                data,
                new_child,
                &[],
                &removed,
                None,
                None,
                false,
            );
        } else {
            data.host.document.nodes[new_child].children.clear();
        }
    }
    for &ch in &inserting {
        let old_p = data.host.document.nodes[ch].parent;
        if old_p != ch {
            detach_child_node(data, old_p, ch);
        }
        data.host.document.nodes[ch].parent = parent;
    }
    let pos = match ref_child {
        Some(rc) => data.host.document.nodes[parent]
            .children
            .iter()
            .position(|&x| x == rc)
            .unwrap_or(data.host.document.nodes[parent].children.len()),
        None => data.host.document.nodes[parent].children.len(),
    };
    if track && !inserting.is_empty() {
        let prev_sib = pos
            .checked_sub(1)
            .and_then(|i| data.host.document.nodes[parent].children.get(i).copied());
        let next_sib = data.host.document.nodes[parent].children.get(pos).copied();
        let parent_connected = is_connected_node(&data.host.document.nodes, parent);
        data.host.document.nodes[parent]
            .children
            .splice(pos..pos, inserting.iter().copied());
        super::observers_wc::record_child_list_mutation(
            data,
            parent,
            &inserting,
            &[],
            prev_sib,
            next_sib,
            parent_connected,
        );
    } else {
        data.host.document.nodes[parent]
            .children
            .splice(pos..pos, inserting);
    }
    data.dom_epoch = data.dom_epoch.wrapping_add(1);
    Ok(new_child)
}

fn clone_subtree(data: &mut State, src: usize, deep: bool) -> Result<usize, String> {
    let (tag, text, attrs, children) = {
        let n = &data.host.document.nodes[src];
        (
            n.tag.clone(),
            n.text.clone(),
            n.attributes.clone(),
            if deep { n.children.clone() } else { Vec::new() },
        )
    };
    let clone_id = if src == 0 || tag == "#document-subroot" {
        data.host
            .new_node("#document-subroot".into(), String::new())?
    } else {
        data.host.new_node(tag.clone(), text)?
    };
    for (k, v) in attrs {
        if k == crate::document::INTERNAL_CANVAS_ATTR {
            continue;
        }
        data.host.attr(clone_id, &k, v)?;
    }
    if deep {
        for ch in children {
            let ch_clone = clone_subtree(data, ch, true)?;
            data.host.document.nodes[ch_clone].parent = clone_id;
            data.host.document.nodes[clone_id].children.push(ch_clone);
        }
        if tag == "template"
            && let Some(&src_frag) = data.template_contents.get(&src)
        {
            let frag_clone = clone_subtree(data, src_frag, true)?;
            data.template_contents.insert(clone_id, frag_clone);
        }
    } else if tag == "template" {
        let empty_frag = data
            .host
            .new_node("#document-fragment".into(), String::new())?;
        data.template_contents.insert(clone_id, empty_frag);
    }
    Ok(clone_id)
}

fn import_parsed_children(
    data: &mut State,
    mut parsed_nodes: Vec<Node>,
    source_bytes: usize,
) -> Result<Vec<usize>, String> {
    if data.host.document.nodes.len() + parsed_nodes.len() > crate::document::MAX_NODES {
        return Err("DOM node limit exceeded".into());
    }
    data.host
        .charge(source_bytes.saturating_add(parsed_nodes.len() * 128))?;
    let start_id = data.host.document.nodes.len();
    let offset = start_id - 1;
    let top_children: Vec<usize> = parsed_nodes[0]
        .children
        .iter()
        .map(|&i| i + offset)
        .collect();
    for (idx, mut node) in parsed_nodes.drain(..).enumerate().skip(1) {
        let my_id = idx + offset;
        node.parent = if node.parent == 0 {
            my_id
        } else {
            node.parent + offset
        };
        for ch in &mut node.children {
            *ch += offset;
        }
        data.host.document.nodes.push(node);
    }
    lift_template_contents(data, start_id)?;
    data.dom_epoch = data.dom_epoch.wrapping_add(1);
    Ok(top_children)
}

fn parse_html_into_fragment_children(data: &mut State, html: &str) -> Result<Vec<usize>, String> {
    if html.len() > MAX_SOURCE {
        return Err("HTML fragment source limit exceeded".into());
    }
    let parsed_nodes = document::parse_html_fragment(html);
    import_parsed_children(data, parsed_nodes, html.len())
}

fn convert_nodes_or_strings(args: &[JsValue], context: &mut Context) -> JsResult<Vec<usize>> {
    enum ItemArg {
        Node(usize),
        Text(String),
    }
    let mut converted = Vec::with_capacity(args.len());
    for arg in args {
        if let Some(obj) = arg.as_object() {
            let handle_opt = {
                let brands = state(context).borrow().brands.clone();
                brands
                    .get(&obj, context)?
                    .as_string()
                    .map(|s| s.to_std_string_escaped())
            };
            if let Some(handle) = handle_opt {
                let id = {
                    let shared = state(context);
                    let data = shared.borrow();
                    node_id_from_handle(&handle, &data).map_err(failure)?
                };
                converted.push(ItemArg::Node(id));
                continue;
            }
        }
        let s = utf8(arg, context)?;
        modern::check_context(context)?;
        converted.push(ItemArg::Text(s));
    }
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let mut ids = Vec::with_capacity(converted.len());
    for item in converted {
        match item {
            ItemArg::Node(id) => ids.push(id),
            ItemArg::Text(text) => {
                let tid = data.host.new_node("#text".into(), text).map_err(failure)?;
                ids.push(tid);
            }
        }
    }
    Ok(ids)
}

fn build_temp_fragment_if_needed(
    data: &mut State,
    ids: Vec<usize>,
) -> Result<Option<usize>, DomOpErr> {
    if ids.is_empty() {
        return Ok(None);
    }
    if ids.len() == 1 {
        return Ok(Some(ids[0]));
    }
    let frag = data
        .host
        .new_node("#document-fragment".into(), String::new())
        .map_err(DomOpErr::Host)?;
    for id in ids {
        insert_node_before(data, frag, id, None)?;
    }
    Ok(Some(frag))
}

// --- Form Control Helpers ---

pub(super) fn select_option_nodes(nodes: &[Node], select_id: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack: Vec<usize> = nodes[select_id].children.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let tag = nodes[id].tag.as_str();
        if tag == "option" {
            out.push(id);
        } else if tag == "optgroup" {
            stack.extend(nodes[id].children.iter().rev().copied());
        }
    }
    out
}

pub(super) fn option_value(nodes: &[Node], opt_id: usize) -> String {
    if let Some(v) = nodes[opt_id].attr("value") {
        v.to_owned()
    } else {
        text_content(nodes, opt_id).trim().to_owned()
    }
}

pub(super) fn select_effective_index(data: &State, select_id: usize) -> i32 {
    let opts = select_option_nodes(&data.host.document.nodes, select_id);
    if opts.is_empty() {
        return -1;
    }
    if let Some(pos) = opts
        .iter()
        .rposition(|&oid| data.host.document.nodes[oid].has("selected"))
    {
        return pos as i32;
    }
    if data.unselected_selects.contains(&select_id) {
        return -1;
    }
    if let Some(pos) = opts
        .iter()
        .position(|&oid| !data.host.document.nodes[oid].has("disabled"))
    {
        pos as i32
    } else {
        -1
    }
}

pub(super) fn set_select_index(
    data: &mut State,
    select_id: usize,
    index: i32,
) -> Result<(), String> {
    let opts = select_option_nodes(&data.host.document.nodes, select_id);
    for &oid in &opts {
        data.host.document.nodes[oid]
            .attributes
            .retain(|(n, _)| n != "selected");
    }
    if index >= 0 && (index as usize) < opts.len() {
        data.unselected_selects.remove(&select_id);
        data.host
            .attr(opts[index as usize], "selected", String::new())?;
    } else {
        data.unselected_selects.insert(select_id);
    }
    data.dom_epoch = data.dom_epoch.wrapping_add(1);
    Ok(())
}

pub(super) fn set_select_value(
    data: &mut State,
    select_id: usize,
    value: &str,
) -> Result<(), String> {
    let opts = select_option_nodes(&data.host.document.nodes, select_id);
    if let Some(pos) = opts
        .iter()
        .position(|&oid| option_value(&data.host.document.nodes, oid) == value)
    {
        set_select_index(data, select_id, pos as i32)
    } else {
        set_select_index(data, select_id, -1)
    }
}

pub(super) fn set_input_checked(
    data: &mut State,
    input_id: usize,
    checked: bool,
) -> Result<(), String> {
    let is_radio = data.host.document.nodes[input_id].tag == "input"
        && data.host.document.nodes[input_id]
            .attr("type")
            .is_some_and(|t| t.eq_ignore_ascii_case("radio"));
    if checked {
        if is_radio {
            let group_name = data.host.document.nodes[input_id]
                .attr("name")
                .unwrap_or("")
                .to_owned();
            if !group_name.is_empty() {
                let owner = data.host.form_owner(input_id);
                let connected = is_connected_node(&data.host.document.nodes, input_id);
                if connected {
                    let count = data.host.document.nodes.len();
                    for peer in 1..count {
                        if peer != input_id
                            && data.host.document.nodes[peer].tag == "input"
                            && data.host.document.nodes[peer]
                                .attr("type")
                                .is_some_and(|t| t.eq_ignore_ascii_case("radio"))
                            && data.host.document.nodes[peer].attr("name")
                                == Some(group_name.as_str())
                            && is_connected_node(&data.host.document.nodes, peer)
                            && data.host.form_owner(peer) == owner
                        {
                            data.host.document.nodes[peer]
                                .attributes
                                .retain(|(n, _)| n != "checked");
                        }
                    }
                }
            }
        }
        data.host.attr(input_id, "checked", String::new())?;
    } else {
        data.host.document.nodes[input_id]
            .attributes
            .retain(|(n, _)| n != "checked");
    }
    data.dom_epoch = data.dom_epoch.wrapping_add(1);
    Ok(())
}

// --- Property Interception (`get`, `set`, `has`) ---

fn common_node_method(name: &str) -> Option<&'static str> {
    match name {
        "compareDocumentPosition" => Some(COMPARE_DOCUMENT_POSITION),
        "getRootNode" => Some(GET_ROOT_NODE),
        "hasChildNodes" => Some(HAS_CHILD_NODES),
        "contains" => Some(CONTAINS),
        "cloneNode" => Some(CLONE_NODE),
        "isSameNode" => Some(IS_SAME_NODE),
        "isEqualNode" => Some(IS_EQUAL_NODE),
        "normalize" => Some(NORMALIZE),
        "lookupNamespaceURI" => Some(LOOKUP_NAMESPACE_URI),
        "lookupPrefix" => Some(LOOKUP_PREFIX),
        "isDefaultNamespace" => Some(IS_DEFAULT_NAMESPACE),
        _ => None,
    }
}

fn parent_node_method(name: &str) -> Option<&'static str> {
    match name {
        "insertBefore" => Some(INSERT_BEFORE),
        "replaceChild" => Some(REPLACE_CHILD),
        "prepend" => Some(PREPEND),
        "append" => Some(APPEND),
        "replaceChildren" => Some(REPLACE_CHILDREN),
        _ => None,
    }
}

fn child_node_method(name: &str) -> Option<&'static str> {
    match name {
        "remove" => Some(REMOVE),
        "before" => Some(BEFORE),
        "after" => Some(AFTER),
        "replaceWith" => Some(REPLACE_WITH),
        _ => None,
    }
}

fn doc_shared_method(name: &str) -> Option<&'static str> {
    match name {
        "createDocumentFragment" => Some(CREATE_DOCUMENT_FRAGMENT),
        "createEvent" => Some(super::dom_events::DOC_CREATE_EVENT),
        "getElementsByTagNameNS" => Some(GET_ELEMENTS_BY_TAG_NAME_NS),
        "getAnimations" => Some(super::observers_wc::DOC_GET_ANIMATIONS),
        "write" => Some(DOC_WRITE),
        "writeln" => Some(DOC_WRITELN),
        "open" => Some(DOC_OPEN),
        "close" => Some(DOC_CLOSE),
        _ => None,
    }
}

fn element_only_method(name: &str) -> Option<&'static str> {
    match name {
        "matches" => Some(selectors::MATCHES),
        "webkitMatchesSelector" => Some(selectors::WEBKIT_MATCHES),
        "closest" => Some(selectors::CLOSEST),
        "hasAttribute" => Some(HAS_ATTRIBUTE),
        "hasAttributes" => Some(HAS_ATTRIBUTES),
        "getAttributeNS" => Some(GET_ATTRIBUTE_NS),
        "setAttributeNS" => Some(SET_ATTRIBUTE_NS),
        "removeAttributeNS" => Some(REMOVE_ATTRIBUTE_NS),
        "hasAttributeNS" => Some(HAS_ATTRIBUTE_NS),
        "getElementsByTagNameNS" => Some(GET_ELEMENTS_BY_TAG_NAME_NS),
        "getAttributeNode" => Some(GET_ATTRIBUTE_NODE),
        "getAttributeNodeNS" => Some(GET_ATTRIBUTE_NODE_NS),
        "setAttributeNode" => Some(SET_ATTRIBUTE_NODE),
        "setAttributeNodeNS" => Some(SET_ATTRIBUTE_NODE_NS),
        "removeAttributeNode" => Some(REMOVE_ATTRIBUTE_NODE),
        "attachShadow" => Some(ATTACH_SHADOW),
        "checkVisibility" => Some(CHECK_VISIBILITY),
        "requestFullscreen" => Some(REQUEST_FULLSCREEN),
        "scroll" => Some(EL_SCROLL),
        "scrollTo" => Some(EL_SCROLL_TO),
        "scrollBy" => Some(EL_SCROLL_BY),
        "scrollIntoViewIfNeeded" => Some(EL_SCROLL_INTO_VIEW_IF_NEEDED),
        "toggleAttribute" => Some(TOGGLE_ATTRIBUTE),
        "getAttributeNames" => Some(GET_ATTRIBUTE_NAMES),
        "insertAdjacentHTML" => Some(INSERT_ADJACENT_HTML),
        "insertAdjacentElement" => Some(INSERT_ADJACENT_ELEMENT),
        "insertAdjacentText" => Some(INSERT_ADJACENT_TEXT),
        _ => None,
    }
}

fn child_element_count(id: usize, context: &mut Context) -> f64 {
    let shared = state(context);
    let data = shared.borrow();
    data.host.document.nodes[id]
        .children
        .iter()
        .filter(|&&c| !data.host.document.nodes[c].tag.starts_with('#'))
        .count() as f64
}

pub(super) fn try_get(
    handle: &str,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<JsValue>> {
    // Sub-document from DOMParser (`subdoc:{root_id}`)
    if let Some(sub_id) = handle
        .strip_prefix("subdoc:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        return subdoc_get(sub_id, name, context);
    }

    if handle == "document" {
        if let Some(cval) = node_constant(name) {
            return Ok(Some(cval.into()));
        }
        if let Some(m) = doc_shared_method(name)
            .or_else(|| common_node_method(name))
            .or_else(|| parent_node_method(name))
        {
            return method(m, context).map(|o| Some(o.into()));
        }
        match name {
            "createTreeWalker" => {
                return method(CREATE_TREE_WALKER, context).map(|o| Some(o.into()));
            }
            "hasFocus" => return method(DOC_HAS_FOCUS, context).map(|o| Some(o.into())),
            "currentScript" => {
                let cs = state(context).borrow().current_script;
                return match cs {
                    Some(sid) => wrap(&format!("node:{sid}"), context).map(Some),
                    None => Ok(Some(JsValue::null())),
                };
            }
            "defaultView" => return Ok(Some(context.global_object().into())),
            "documentElement" => {
                let target = {
                    let shared = state(context);
                    let data = shared.borrow();
                    let kids = &data.host.document.nodes[0].children;
                    kids.iter()
                        .copied()
                        .find(|&c| data.host.document.nodes[c].tag == "html")
                        .or_else(|| {
                            find_in_subtree(&data.host.document.nodes, 0, |n| n.tag == "html")
                        })
                        .or_else(|| {
                            kids.iter()
                                .copied()
                                .find(|&c| data.host.document.nodes[c].tag == "body")
                        })
                        .or_else(|| {
                            kids.iter()
                                .copied()
                                .find(|&c| !data.host.document.nodes[c].tag.starts_with('#'))
                        })
                };
                return match target {
                    Some(tid) => wrap(&format!("node:{tid}"), context).map(Some),
                    None => Ok(Some(JsValue::null())),
                };
            }
            "scrollingElement" => {
                let target = {
                    let shared = state(context);
                    let data = shared.borrow();
                    data.host.document.nodes[0]
                        .children
                        .iter()
                        .copied()
                        .find(|&c| data.host.document.nodes[c].tag == "html")
                        .or_else(|| {
                            find_in_subtree(&data.host.document.nodes, 0, |n| n.tag == "body")
                        })
                };
                return match target {
                    Some(tid) => wrap(&format!("node:{tid}"), context).map(Some),
                    None => Ok(Some(JsValue::null())),
                };
            }
            "compatMode" => return Ok(Some(js_string!("CSS1Compat").into())),
            "characterSet" | "charset" | "inputEncoding" => {
                return Ok(Some(js_string!("UTF-8").into()));
            }
            "contentType" => return Ok(Some(js_string!("text/html").into())),
            "visibilityState" => return Ok(Some(js_string!("visible").into())),
            "hidden" => return Ok(Some(false.into())),
            "dir" => {
                let d = {
                    let shared = state(context);
                    let data = shared.borrow();
                    data.host.document.nodes[0]
                        .children
                        .iter()
                        .find(|&&c| data.host.document.nodes[c].tag == "html")
                        .and_then(|&hid| data.host.document.nodes[hid].attr("dir"))
                        .unwrap_or("")
                        .to_owned()
                };
                return Ok(Some(JsString::from(d.as_str()).into()));
            }
            "ownerDocument" | "parentElement" | "parentNode" | "nodeValue" => {
                return Ok(Some(JsValue::null()));
            }
            "isConnected" => return Ok(Some(true.into())),
            "nodeName" => return Ok(Some(js_string!("#document").into())),
            "childElementCount" => return Ok(Some(child_element_count(0, context).into())),
            _ => {}
        }
    }

    if !handle.starts_with("node:") {
        return Ok(None);
    }
    let (id, tag) = {
        let shared = state(context);
        let data = shared.borrow();
        let id = data.host.node(handle).map_err(failure)?;
        (id, data.host.document.nodes[id].tag.clone())
    };

    if let Some(cval) = node_constant(name) {
        return Ok(Some(cval.into()));
    }
    if let Some(m) = common_node_method(name) {
        return method(m, context).map(|o| Some(o.into()));
    }
    match name {
        "ownerDocument" => {
            let h = owner_document_handle(&state(context).borrow().host.document.nodes, id)
                .unwrap_or_else(|| "document".into());
            return wrap(&h, context).map(Some);
        }
        "baseURI" => {
            let u = state(context).borrow().host.base_url().to_string();
            return Ok(Some(JsString::from(u.as_str()).into()));
        }
        _ => {}
    }

    // DocumentFragment node
    if tag == "#document-fragment" {
        if let Some(m) = parent_node_method(name) {
            return method(m, context).map(|o| Some(o.into()));
        }
        return match name {
            "nodeType" => Ok(Some(11.into())),
            "nodeName" => Ok(Some(js_string!("#document-fragment").into())),
            "tagName" | "setAttribute" | "getAttribute" | "removeAttribute" | "classList"
            | "style" | "dataset" => Ok(Some(JsValue::undefined())),
            "parentNode" | "parentElement" | "nodeValue" => Ok(Some(JsValue::null())),
            "isConnected" => Ok(Some(false.into())),
            "childElementCount" => Ok(Some(child_element_count(id, context).into())),
            "getElementById" => method("host.dom.getElementById", context).map(|o| Some(o.into())),
            _ => Ok(None),
        };
    }

    // Shared properties/methods across `#text` and `Element`
    if let Some(m) = child_node_method(name) {
        return method(m, context).map(|o| Some(o.into()));
    }
    match name {
        "isConnected" => {
            let c = is_connected_node(&state(context).borrow().host.document.nodes, id);
            return Ok(Some(c.into()));
        }
        "parentElement" => {
            let shared = state(context);
            let data = shared.borrow();
            let p = data.host.document.nodes[id].parent;
            if p == 0 || p == id || data.host.document.nodes[p].tag.starts_with('#') {
                return Ok(Some(JsValue::null()));
            }
            drop(data);
            return wrap(&format!("node:{p}"), context).map(Some);
        }
        "parentNode" => {
            let p = state(context).borrow().host.document.nodes[id].parent;
            if p == id {
                return Ok(Some(JsValue::null()));
            }
            return wrap_node_id(p, context).map(Some);
        }
        _ => {}
    }

    // Text node specifics
    if tag == "#text" {
        return match name {
            "nodeName" => Ok(Some(js_string!("#text").into())),
            "tagName" => Ok(Some(JsValue::undefined())),
            "data" | "nodeValue" => {
                let t = state(context).borrow().host.document.nodes[id].text.clone();
                Ok(Some(JsString::from(t.as_str()).into()))
            }
            "length" => {
                let len = state(context).borrow().host.document.nodes[id]
                    .text
                    .encode_utf16()
                    .count();
                Ok(Some((len as f64).into()))
            }
            _ => Ok(None),
        };
    }

    // Element nodes (`tag != "#text"` and `id != 0`)
    if let Some(m) = parent_node_method(name).or_else(|| element_only_method(name)) {
        return method(m, context).map(|o| Some(o.into()));
    }
    match name {
        "childElementCount" => {
            return Ok(Some(child_element_count(id, context).into()));
        }
        "content" if tag == "template" => {
            let frag_id = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                ensure_template_content(&mut data, id).map_err(failure)?
            };
            return wrap(&format!("node:{frag_id}"), context).map(Some);
        }
        "innerHTML" if tag == "template" => {
            let html = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let frag_id = ensure_template_content(&mut data, id).map_err(failure)?;
                data.host.serialize(frag_id, false).map_err(failure)?
            };
            return Ok(Some(JsString::from(html.as_str()).into()));
        }
        // <dialog> properties & methods
        "open" if tag == "dialog" || tag == "details" => {
            let open = state(context).borrow().host.document.nodes[id].has("open");
            return Ok(Some(open.into()));
        }
        "returnValue" if tag == "dialog" => {
            let rv = state(context)
                .borrow()
                .dialog_return_values
                .get(&id)
                .cloned()
                .unwrap_or_default();
            return Ok(Some(JsString::from(rv.as_str()).into()));
        }
        "show" if tag == "dialog" => return method(DIALOG_SHOW, context).map(|o| Some(o.into())),
        "showModal" if tag == "dialog" => {
            return method(DIALOG_SHOW_MODAL, context).map(|o| Some(o.into()));
        }
        "close" if tag == "dialog" => {
            return method(DIALOG_CLOSE, context).map(|o| Some(o.into()));
        }
        // Form control properties
        "checked" if tag == "input" => {
            let c = state(context).borrow().host.document.nodes[id].has("checked");
            return Ok(Some(c.into()));
        }
        "selected" if tag == "option" => {
            let selected = {
                let shared = state(context);
                let data = shared.borrow();
                let mut p = data.host.document.nodes[id].parent;
                if p != id && data.host.document.nodes[p].tag == "optgroup" {
                    p = data.host.document.nodes[p].parent;
                }
                if p != id && data.host.document.nodes[p].tag == "select" {
                    let opts = select_option_nodes(&data.host.document.nodes, p);
                    let eff = select_effective_index(&data, p);
                    eff >= 0 && opts.get(eff as usize) == Some(&id)
                } else {
                    data.host.document.nodes[id].has("selected")
                }
            };
            return Ok(Some(selected.into()));
        }
        "text" if tag == "option" => {
            let t = {
                let shared = state(context);
                let data = shared.borrow();
                text_content(&data.host.document.nodes, id)
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            return Ok(Some(JsString::from(t.as_str()).into()));
        }
        "value" if tag == "option" => {
            let v = option_value(&state(context).borrow().host.document.nodes, id);
            return Ok(Some(JsString::from(v.as_str()).into()));
        }
        "value" if tag == "select" => {
            let val = {
                let shared = state(context);
                let data = shared.borrow();
                let eff = select_effective_index(&data, id);
                let opts = select_option_nodes(&data.host.document.nodes, id);
                if eff >= 0 && (eff as usize) < opts.len() {
                    option_value(&data.host.document.nodes, opts[eff as usize])
                } else {
                    String::new()
                }
            };
            return Ok(Some(JsString::from(val.as_str()).into()));
        }
        "selectedIndex" if tag == "select" => {
            let idx = select_effective_index(&state(context).borrow(), id);
            return Ok(Some(idx.into()));
        }
        "options" if tag == "select" => {
            let col = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let opts = select_option_nodes(&data.host.document.nodes, id);
                data.host.collection(opts).map_err(failure)?
            };
            return from_host(col, context).map(Some);
        }
        "type" if tag == "input" => {
            let t = effective_input_type(&state(context).borrow().host.document.nodes[id]);
            return Ok(Some(js_string!(t).into()));
        }
        "type" if tag == "button" => {
            let t = state(context).borrow().host.document.nodes[id]
                .attr("type")
                .map(|s| s.to_ascii_lowercase())
                .filter(|s| matches!(s.as_str(), "submit" | "reset" | "button"))
                .unwrap_or_else(|| "submit".into());
            return Ok(Some(JsString::from(t.as_str()).into()));
        }
        "type" if tag == "select" => {
            let multi = state(context).borrow().host.document.nodes[id].has("multiple");
            return Ok(Some(
                js_string!(if multi {
                    "select-multiple"
                } else {
                    "select-one"
                })
                .into(),
            ));
        }
        "type" if tag == "textarea" => return Ok(Some(js_string!("textarea").into())),
        "form"
            if matches!(
                tag.as_str(),
                "input" | "button" | "select" | "textarea" | "fieldset"
            ) =>
        {
            let owner = state(context).borrow().host.form_owner(id);
            return match owner {
                Some(fid) => wrap(&format!("node:{fid}"), context).map(Some),
                None => Ok(Some(JsValue::null())),
            };
        }
        "elements" if tag == "form" => {
            let col = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let mut items = Vec::new();
                for nid in 1..data.host.document.nodes.len() {
                    if matches!(
                        data.host.document.nodes[nid].tag.as_str(),
                        "input" | "button" | "select" | "textarea" | "fieldset"
                    ) && is_connected_node(&data.host.document.nodes, nid)
                        && data.host.form_owner(nid) == Some(id)
                    {
                        items.push(nid);
                    }
                }
                data.host.collection(items).map_err(failure)?
            };
            return from_host(col, context).map(Some);
        }
        "htmlFor" if tag == "label" => {
            let f = state(context).borrow().host.document.nodes[id]
                .attr("for")
                .unwrap_or("")
                .to_owned();
            return Ok(Some(JsString::from(f.as_str()).into()));
        }
        "control" if tag == "label" => {
            let target = {
                let shared = state(context);
                let data = shared.borrow();
                data.host.document.label_target(id)
            };
            return match target {
                Some(tid) => wrap(&format!("node:{tid}"), context).map(Some),
                None => Ok(Some(JsValue::null())),
            };
        }
        "labels" if matches!(tag.as_str(), "input" | "button" | "select" | "textarea") => {
            let items = {
                let shared = state(context);
                let data = shared.borrow();
                let mut items = Vec::new();
                if tag != "input" || effective_input_type(&data.host.document.nodes[id]) != "hidden"
                {
                    let root = root_of(&data.host.document.nodes, id);
                    for lid in 1..data.host.document.nodes.len() {
                        if data.host.document.nodes[lid].tag == "label"
                            && root_of(&data.host.document.nodes, lid) == root
                            && data.host.document.label_target(lid) == Some(id)
                        {
                            items.push(lid);
                        }
                    }
                }
                items
            };
            return selectors::alloc_nodelist(items, context).map(Some);
        }
        "readOnly" if tag == "input" || tag == "textarea" => {
            let ro = state(context).borrow().host.document.nodes[id].has("readonly");
            return Ok(Some(ro.into()));
        }
        "required" if matches!(tag.as_str(), "input" | "select" | "textarea") => {
            let req = state(context).borrow().host.document.nodes[id].has("required");
            return Ok(Some(req.into()));
        }
        "multiple" if matches!(tag.as_str(), "input" | "select") => {
            let m = state(context).borrow().host.document.nodes[id].has("multiple");
            return Ok(Some(m.into()));
        }
        "isMap" if tag == "img" => {
            let m = state(context).borrow().host.document.nodes[id].has("ismap");
            return Ok(Some(m.into()));
        }
        "naturalWidth" if tag == "img" => {
            let v = state(context).borrow().host.document.nodes[id]
                .attr("width")
                .and_then(parse_non_negative_uint)
                .unwrap_or(0);
            return Ok(Some((v as f64).into()));
        }
        "naturalHeight" if tag == "img" => {
            let v = state(context).borrow().host.document.nodes[id]
                .attr("height")
                .and_then(parse_non_negative_uint)
                .unwrap_or(0);
            return Ok(Some((v as f64).into()));
        }
        "complete" if tag == "img" => return Ok(Some(true.into())),
        "currentSrc" if tag == "img" => {
            let cs = {
                let shared = state(context);
                let data = shared.borrow();
                data.host.document.nodes[id]
                    .attr("src")
                    .filter(|s| !s.is_empty())
                    .and_then(|s| data.host.base_url().join(s).ok())
                    .map(|u| u.to_string())
                    .unwrap_or_default()
            };
            return Ok(Some(JsString::from(cs.as_str()).into()));
        }
        "x" | "y" if tag == "img" => return Ok(Some(0.into())),
        "decode" if tag == "img" => return method(IMG_DECODE, context).map(|o| Some(o.into())),
        "shadowRoot" | "assignedSlot" | "prefix" | "nodeValue" => {
            return Ok(Some(JsValue::null()));
        }
        "part" => return build_part_token_list(id, context).map(Some),
        "outerText" => {
            let is_svg = is_svg_element(&state(context).borrow().host.document.nodes, id);
            if !is_svg {
                let t = text_content(&state(context).borrow().host.document.nodes, id);
                return Ok(Some(JsString::from(t.as_str()).into()));
            }
            return Ok(None);
        }
        "translate" => {
            let is_svg = is_svg_element(&state(context).borrow().host.document.nodes, id);
            if !is_svg {
                let tr = !matches!(
                    state(context).borrow().host.document.nodes[id]
                        .attr("translate")
                        .map(|v| v.trim().to_ascii_lowercase())
                        .as_deref(),
                    Some("no")
                );
                return Ok(Some(tr.into()));
            }
            return Ok(None);
        }
        "inert" | "autofocus" => {
            let b = state(context).borrow().host.document.nodes[id].has(name);
            return Ok(Some(b.into()));
        }
        "draggable" => {
            let is_svg = is_svg_element(&state(context).borrow().host.document.nodes, id);
            if !is_svg {
                let dr = {
                    let shared = state(context);
                    let data = shared.borrow();
                    let n = &data.host.document.nodes[id];
                    match n
                        .attr("draggable")
                        .map(|v| v.trim().to_ascii_lowercase())
                        .as_deref()
                    {
                        Some("true") => true,
                        Some("false") => false,
                        _ => tag == "img" || (tag == "a" && n.has("href")),
                    }
                };
                return Ok(Some(dr.into()));
            }
            return Ok(None);
        }
        "spellcheck" => {
            let is_svg = is_svg_element(&state(context).borrow().host.document.nodes, id);
            if !is_svg {
                let sp = !matches!(
                    state(context).borrow().host.document.nodes[id]
                        .attr("spellcheck")
                        .map(|v| v.trim().to_ascii_lowercase())
                        .as_deref(),
                    Some("false")
                );
                return Ok(Some(sp.into()));
            }
            return Ok(None);
        }
        "slot" => {
            let s = state(context).borrow().host.document.nodes[id]
                .attr("slot")
                .unwrap_or("")
                .to_owned();
            return Ok(Some(JsString::from(s.as_str()).into()));
        }
        "localName" => return Ok(Some(JsString::from(tag.as_str()).into())),
        "namespaceURI" => {
            let is_svg = is_svg_element(&state(context).borrow().host.document.nodes, id);
            return Ok(Some(
                js_string!(if is_svg {
                    "http://www.w3.org/2000/svg"
                } else {
                    "http://www.w3.org/1999/xhtml"
                })
                .into(),
            ));
        }
        "isContentEditable" => {
            let ce = is_element_content_editable(&state(context).borrow().host.document.nodes, id);
            return Ok(Some(ce.into()));
        }
        "contentEditable" => {
            let val = state(context).borrow().host.document.nodes[id]
                .attr("contenteditable")
                .map(|v| match v.trim().to_ascii_lowercase().as_str() {
                    "" | "true" => "true",
                    "plaintext-only" => "plaintext-only",
                    "false" => "false",
                    _ => "inherit",
                })
                .unwrap_or("inherit");
            return Ok(Some(js_string!(val).into()));
        }
        "tabIndex" => {
            let ti = element_tab_index(&state(context).borrow().host.document.nodes, id);
            return Ok(Some(ti.into()));
        }
        "attributes" => {
            return build_named_node_map(id, context).map(Some);
        }
        "contentDocument" if matches!(tag.as_str(), "frame" | "object") => {
            if tag == "object" {
                return Ok(Some(JsValue::null()));
            }
            return iframe_content_document(id, context).map(Some);
        }
        "contentWindow" if tag == "frame" => {
            return iframe_content_window(id, context).map(Some);
        }
        "getContext" if tag == "canvas" => {
            method(super::canvas::CANVAS_GET_CONTEXT, context).map(|o| Some(o.into()))
        }
        "toDataURL" if tag == "canvas" => {
            method(super::canvas::CANVAS_TO_DATA_URL, context).map(|o| Some(o.into()))
        }
        "toBlob" if tag == "canvas" => {
            method(super::canvas::CANVAS_TO_BLOB, context).map(|o| Some(o.into()))
        }
        "animate" if !tag.starts_with('#') => {
            method(super::observers_wc::EL_ANIMATE, context).map(|o| Some(o.into()))
        }
        "getAnimations" if !tag.starts_with('#') => {
            method(super::observers_wc::EL_GET_ANIMATIONS, context).map(|o| Some(o.into()))
        }
        _ => {
            let is_svg = is_svg_element(&state(context).borrow().host.document.nodes, id);
            if let Some(attr) = reflected_string_attr(tag.as_str(), name, is_svg) {
                let val = state(context).borrow().host.document.nodes[id]
                    .attr(attr)
                    .unwrap_or("")
                    .to_owned();
                return Ok(Some(JsString::from(val.as_str()).into()));
            }
            if !is_svg
                && let Some((attr, def_val, non_zero)) = reflected_uint_attr(tag.as_str(), name)
            {
                let v = state(context).borrow().host.document.nodes[id]
                    .attr(attr)
                    .and_then(parse_non_negative_uint)
                    .filter(|&n| !non_zero || n > 0)
                    .unwrap_or(def_val);
                return Ok(Some((v as f64).into()));
            }
            Ok(None)
        }
    }
}

pub(super) fn try_set(
    handle: &str,
    name: &str,
    value: &JsValue,
    context: &mut Context,
) -> JsResult<Option<()>> {
    if handle == "document" && name == "nodeValue" {
        return Ok(Some(()));
    }
    if handle == "document" && name == "dir" {
        let text = utf8(value, context)?;
        modern::check_context(context)?;
        let hid_opt = state(context).borrow().host.document.nodes[0]
            .children
            .iter()
            .copied()
            .find(|&c| state(context).borrow().host.document.nodes[c].tag == "html");
        if let Some(hid) = hid_opt {
            set_element_attr_observed(hid, "dir", text, context)?;
        }
        return Ok(Some(()));
    }
    if let Some(sub_id) = handle
        .strip_prefix("subdoc:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        if name == "title" {
            let text = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let title_id = find_in_subtree(&data.host.document.nodes, sub_id, |n| n.tag == "title");
            let tid = match title_id {
                Some(t) => t,
                None => {
                    let t = data
                        .host
                        .new_node("title".into(), String::new())
                        .map_err(failure)?;
                    let head =
                        find_in_subtree(&data.host.document.nodes, sub_id, |n| n.tag == "head")
                            .unwrap_or(sub_id);
                    data.host.append(head, t).map_err(failure)?;
                    t
                }
            };
            data.host.replace_text(tid, text).map_err(failure)?;
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            return Ok(Some(()));
        }
        return Ok(None);
    }

    if !handle.starts_with("node:") {
        return Ok(None);
    }
    let (id, tag) = {
        let shared = state(context);
        let data = shared.borrow();
        let id = data.host.node(handle).map_err(failure)?;
        (id, data.host.document.nodes[id].tag.clone())
    };

    if tag != "#text" && name == "nodeValue" {
        return Ok(Some(()));
    }

    if tag == "#text" && matches!(name, "data" | "nodeValue" | "textContent") {
        let text = if value.is_null() && matches!(name, "nodeValue" | "textContent") {
            String::new()
        } else {
            utf8(value, context)?
        };
        modern::check_context(context)?;
        let mutated = {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            data.host.charge(text.len()).map_err(failure)?;
            let track = super::observers_wc::has_any_interest(&data);
            let old = if track {
                std::mem::replace(&mut data.host.document.nodes[id].text, text)
            } else {
                data.host.document.nodes[id].text = text;
                String::new()
            };
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            if track {
                super::observers_wc::record_character_data_mutation(&mut data, id, old);
                true
            } else {
                false
            }
        };
        if mutated {
            super::observers_wc::after_dom_mutation(context)?;
        }
        return Ok(Some(()));
    }

    if (tag == "#document-fragment" && matches!(name, "textContent"))
        || (matches!(tag.as_str(), "script" | "style")
            && matches!(name, "textContent" | "innerText" | "innerHTML"))
    {
        let text = if value.is_null() {
            String::new()
        } else {
            utf8(value, context)?
        };
        modern::check_context(context)?;
        let mutated = {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let track = super::observers_wc::has_any_interest(&data);
            let was_connected = if track {
                is_connected_node(&data.host.document.nodes, id)
            } else {
                false
            };
            let old_children = std::mem::take(&mut data.host.document.nodes[id].children);
            for &old in &old_children {
                data.host.document.nodes[old].parent = old;
            }
            let mut added = Vec::new();
            if !text.is_empty() {
                let tid = data.host.new_node("#text".into(), text).map_err(failure)?;
                data.host.document.nodes[tid].parent = id;
                data.host.document.nodes[id].children.push(tid);
                if track {
                    added.push(tid);
                }
            }
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            if track && (!old_children.is_empty() || !added.is_empty()) {
                super::observers_wc::record_child_list_mutation(
                    &mut data,
                    id,
                    &added,
                    &old_children,
                    None,
                    None,
                    was_connected,
                );
                true
            } else {
                false
            }
        };
        if mutated {
            super::observers_wc::after_dom_mutation(context)?;
        }
        return Ok(Some(()));
    }

    match name {
        "innerHTML" => {
            let html = if value.is_null() {
                String::new()
            } else {
                utf8(value, context)?
            };
            modern::check_context(context)?;
            if tag == "template" {
                let res: Result<(), DomOpErr> = {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    (|| {
                        let frag_id =
                            ensure_template_content(&mut data, id).map_err(DomOpErr::Host)?;
                        let old_children =
                            std::mem::take(&mut data.host.document.nodes[frag_id].children);
                        for &old in &old_children {
                            data.host.document.nodes[old].parent = old;
                        }
                        if super::observers_wc::has_any_interest(&data) && !old_children.is_empty()
                        {
                            super::observers_wc::record_child_list_mutation(
                                &mut data,
                                frag_id,
                                &[],
                                &old_children,
                                None,
                                None,
                                false,
                            );
                        }
                        let children = parse_html_into_fragment_children(&mut data, &html)
                            .map_err(DomOpErr::Host)?;
                        for ch in children {
                            insert_node_before(&mut data, frag_id, ch, None)?;
                        }
                        Ok(())
                    })()
                };
                res.map_err(|e| into_js_err(e, context))?;
                super::observers_wc::after_dom_mutation(context)?;
                return Ok(Some(()));
            }
            let mutated = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let track = super::observers_wc::has_any_interest(&data);
                let old_children = if track {
                    data.host.document.nodes[id].children.clone()
                } else {
                    Vec::new()
                };
                let was_connected = if track {
                    is_connected_node(&data.host.document.nodes, id)
                } else {
                    false
                };
                let prev_len = data.host.document.nodes.len();
                data.host.replace_html(id, html).map_err(failure)?;
                lift_template_contents(&mut data, prev_len).map_err(failure)?;
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
                if track {
                    let added = data.host.document.nodes[id].children.clone();
                    super::observers_wc::record_child_list_mutation(
                        &mut data,
                        id,
                        &added,
                        &old_children,
                        None,
                        None,
                        was_connected,
                    );
                    true
                } else {
                    false
                }
            };
            if mutated {
                super::observers_wc::after_dom_mutation(context)?;
            }
            Ok(Some(()))
        }
        "outerHTML" if !tag.starts_with('#') => {
            let html = if value.is_null() {
                String::new()
            } else {
                utf8(value, context)?
            };
            modern::check_context(context)?;
            let res: Result<(), DomOpErr> = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let parent = data.host.document.nodes[id].parent;
                if parent == id {
                    return Ok(Some(()));
                }
                if parent == 0 || data.host.document.nodes[parent].tag == "#document-subroot" {
                    Err(DomOpErr::Dom(
                        "NoModificationAllowedError",
                        "Cannot set outerHTML on an element whose parent is a Document",
                    ))
                } else {
                    (|| {
                        let children = parse_html_into_fragment_children(&mut data, &html)
                            .map_err(DomOpErr::Host)?;
                        for ch in children {
                            insert_node_before(&mut data, parent, ch, Some(id))?;
                        }
                        detach_child_node(&mut data, parent, id);
                        data.dom_epoch = data.dom_epoch.wrapping_add(1);
                        Ok(())
                    })()
                }
            };
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            Ok(Some(()))
        }
        "open" if tag == "dialog" || tag == "details" => {
            let flag = value.to_boolean();
            let was_open = state(context).borrow().host.document.nodes[id].has("open");
            if was_open != flag {
                if flag {
                    set_element_attr_observed(id, "open", String::new(), context)?;
                } else {
                    remove_element_attr_observed(id, "open", context)?;
                }
                if tag == "details" {
                    dom_events::queue_or_fire_simple_event(id, "toggle", false, false, context)?;
                }
            }
            Ok(Some(()))
        }
        "returnValue" if tag == "dialog" => {
            let rv = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            data.host.charge(rv.len()).map_err(failure)?;
            data.dialog_return_values.insert(id, rv);
            Ok(Some(()))
        }
        "checked" if tag == "input" => {
            let flag = value.to_boolean();
            let shared = state(context);
            let mut data = shared.borrow_mut();
            set_input_checked(&mut data, id, flag).map_err(failure)?;
            Ok(Some(()))
        }
        "selected" if tag == "option" => {
            let flag = value.to_boolean();
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let mut p = data.host.document.nodes[id].parent;
            if p != id && data.host.document.nodes[p].tag == "optgroup" {
                p = data.host.document.nodes[p].parent;
            }
            if p != id && data.host.document.nodes[p].tag == "select" {
                let opts = select_option_nodes(&data.host.document.nodes, p);
                if let Some(pos) = opts.iter().position(|&oid| oid == id) {
                    if flag {
                        set_select_index(&mut data, p, pos as i32).map_err(failure)?;
                    } else {
                        data.host.document.nodes[id]
                            .attributes
                            .retain(|(n, _)| n != "selected");
                        data.dom_epoch = data.dom_epoch.wrapping_add(1);
                    }
                    return Ok(Some(()));
                }
            }
            if flag {
                data.host
                    .attr(id, "selected", String::new())
                    .map_err(failure)?;
            } else {
                data.host.document.nodes[id]
                    .attributes
                    .retain(|(n, _)| n != "selected");
            }
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            Ok(Some(()))
        }
        "value" if tag == "select" => {
            let v = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            set_select_value(&mut data, id, &v).map_err(failure)?;
            Ok(Some(()))
        }
        "selectedIndex" if tag == "select" => {
            let idx = value.to_i32(context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            set_select_index(&mut data, id, idx).map_err(failure)?;
            Ok(Some(()))
        }
        "text" if tag == "option" => {
            let t = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            data.host.replace_text(id, t).map_err(failure)?;
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            Ok(Some(()))
        }
        "htmlFor" if tag == "label" => {
            let f = utf8(value, context)?;
            modern::check_context(context)?;
            set_element_attr_observed(id, "for", f, context)?;
            Ok(Some(()))
        }
        "tabIndex" if !tag.starts_with('#') => {
            let ti = value.to_i32(context)?;
            modern::check_context(context)?;
            set_element_attr_observed(id, "tabindex", ti.to_string(), context)?;
            Ok(Some(()))
        }
        "contentEditable" if !tag.starts_with('#') => {
            let raw = utf8(value, context)?;
            modern::check_context(context)?;
            let norm = raw.trim().to_ascii_lowercase();
            if !matches!(
                norm.as_str(),
                "true" | "false" | "plaintext-only" | "inherit"
            ) {
                return Err(dom_exception::exception(
                    "SyntaxError",
                    "Invalid contentEditable value",
                    context,
                )?);
            }
            if norm == "inherit" {
                remove_element_attr_observed(id, "contenteditable", context)?;
            } else {
                set_element_attr_observed(id, "contenteditable", norm, context)?;
            }
            Ok(Some(()))
        }
        "isMap" if tag == "img" => {
            let flag = value.to_boolean();
            modern::check_context(context)?;
            if flag {
                set_element_attr_observed(id, "ismap", String::new(), context)?;
            } else {
                remove_element_attr_observed(id, "ismap", context)?;
            }
            Ok(Some(()))
        }
        "slot" if !tag.starts_with('#') => {
            let s = utf8(value, context)?;
            modern::check_context(context)?;
            set_element_attr_observed(id, "slot", s, context)?;
            Ok(Some(()))
        }
        "inert" | "autofocus" if !tag.starts_with('#') => {
            let flag = value.to_boolean();
            modern::check_context(context)?;
            if flag {
                set_element_attr_observed(id, name, String::new(), context)?;
            } else {
                remove_element_attr_observed(id, name, context)?;
            }
            Ok(Some(()))
        }
        "translate" | "draggable" | "spellcheck" if !tag.starts_with('#') => {
            if is_svg_element(&state(context).borrow().host.document.nodes, id) {
                return Ok(None);
            }
            let flag = value.to_boolean();
            let s = if name == "translate" {
                if flag { "yes" } else { "no" }
            } else if flag {
                "true"
            } else {
                "false"
            };
            set_element_attr_observed(id, name, s.to_owned(), context)?;
            Ok(Some(()))
        }
        "outerText" if !tag.starts_with('#') => {
            if is_svg_element(&state(context).borrow().host.document.nodes, id) {
                return Ok(None);
            }
            let text = if value.is_null() {
                String::new()
            } else {
                utf8(value, context)?
            };
            modern::check_context(context)?;
            let res: Result<(), DomOpErr> = (|| {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let parent = data.host.document.nodes[id].parent;
                if parent == id {
                    return Ok(());
                }
                if parent == 0 || data.host.document.nodes[parent].tag == "#document-subroot" {
                    Err(DomOpErr::Dom(
                        "NoModificationAllowedError",
                        "Cannot set outerText on an element whose parent is a Document",
                    ))
                } else {
                    let tid = data
                        .host
                        .new_node("#text".into(), text)
                        .map_err(DomOpErr::Host)?;
                    insert_node_before(&mut data, parent, tid, Some(id))?;
                    detach_child_node(&mut data, parent, id);
                    data.dom_epoch = data.dom_epoch.wrapping_add(1);
                    Ok(())
                }
            })();
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            Ok(Some(()))
        }
        _ if !tag.starts_with('#') => {
            let is_svg = is_svg_element(&state(context).borrow().host.document.nodes, id);
            if let Some(attr) = reflected_string_attr(tag.as_str(), name, is_svg) {
                let text = utf8(value, context)?;
                modern::check_context(context)?;
                set_element_attr_observed(id, attr, text, context)?;
                return Ok(Some(()));
            }
            if !is_svg
                && let Some((attr, def_val, non_zero)) = reflected_uint_attr(tag.as_str(), name)
            {
                let v = value.to_u32(context)?;
                modern::check_context(context)?;
                let effective = if non_zero && v == 0 { def_val } else { v };
                set_element_attr_observed(id, attr, effective.to_string(), context)?;
                return Ok(Some(()));
            }
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn parse_non_negative_uint(raw: &str) -> Option<u32> {
    let trimmed = raw.trim_start_matches(['\t', '\n', '\x0C', '\r', ' ']);
    let trimmed = trimmed.strip_prefix('+').unwrap_or(trimmed);
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u32>().ok()
}

fn reflected_string_attr(tag: &str, prop: &str, is_svg: bool) -> Option<&'static str> {
    match prop {
        "title" => return Some("title"),
        "lang" => return Some("lang"),
        "dir" => return Some("dir"),
        "role" => return Some("role"),
        "ariaLabel" => return Some("aria-label"),
        "ariaHidden" => return Some("aria-hidden"),
        _ => {}
    }
    if is_svg {
        return None;
    }
    match prop {
        "accessKey" => return Some("accesskey"),
        "autocapitalize" => return Some("autocapitalize"),
        "enterKeyHint" => return Some("enterkeyhint"),
        "inputMode" => return Some("inputmode"),
        "nonce" => return Some("nonce"),
        _ => {}
    }
    match (tag, prop) {
        ("img" | "area" | "input", "alt") => Some("alt"),
        ("img" | "object", "useMap") => Some("usemap"),
        ("img" | "script" | "link" | "iframe", "fetchPriority") => Some("fetchpriority"),
        ("img", "lowsrc") => Some("lowsrc"),
        ("img", "align") => Some("align"),
        ("img", "longDesc") => Some("longdesc"),
        ("img" | "table", "border") => Some("border"),
        ("a" | "area" | "form" | "base", "target") => Some("target"),
        ("a" | "area" | "link" | "form", "rel") => Some("rel"),
        ("a" | "area", "download") => Some("download"),
        ("input" | "textarea", "placeholder") => Some("placeholder"),
        ("input" | "meter", "min") => Some("min"),
        ("input" | "meter" | "progress", "max") => Some("max"),
        ("input", "step") => Some("step"),
        ("input", "pattern") => Some("pattern"),
        ("input" | "select" | "textarea" | "form", "autocomplete") => Some("autocomplete"),
        ("img" | "iframe", "loading") => Some("loading"),
        ("img", "decoding") => Some("decoding"),
        ("img" | "source", "srcset") => Some("srcset"),
        ("img" | "source" | "link", "sizes") => Some("sizes"),
        ("img" | "script" | "link" | "audio" | "video", "crossOrigin") => Some("crossorigin"),
        ("a" | "area" | "img" | "iframe" | "script" | "link", "referrerPolicy") => {
            Some("referrerpolicy")
        }
        ("iframe", "sandbox") => Some("sandbox"),
        ("iframe", "allow") => Some("allow"),
        ("iframe" | "frame", "frameBorder") => Some("frameborder"),
        ("iframe" | "frame", "scrolling") => Some("scrolling"),
        (
            "iframe" | "embed" | "object" | "table" | "td" | "th" | "col" | "colgroup" | "hr"
            | "tr",
            "width",
        ) => Some("width"),
        (
            "iframe" | "embed" | "object" | "table" | "td" | "th" | "col" | "colgroup" | "hr"
            | "tr",
            "height",
        ) => Some("height"),
        ("td" | "th", "headers") => Some("headers"),
        ("td" | "th", "scope") => Some("scope"),
        ("blockquote" | "q" | "del" | "ins", "cite") => Some("cite"),
        ("time" | "del" | "ins", "dateTime") => Some("datetime"),
        ("form", "enctype" | "encoding") => Some("enctype"),
        ("form", "acceptCharset") => Some("accept-charset"),
        ("link" | "style" | "source", "media") => Some("media"),
        ("link", "as") => Some("as"),
        ("script" | "link", "integrity") => Some("integrity"),
        ("script" | "meta", "charset") => Some("charset"),
        ("meta", "content") => Some("content"),
        ("meta", "httpEquiv") => Some("http-equiv"),
        _ => None,
    }
}

fn reflected_uint_attr(tag: &str, prop: &str) -> Option<(&'static str, u32, bool)> {
    match (tag, prop) {
        ("canvas", "width") => Some(("width", 300, false)),
        ("canvas", "height") => Some(("height", 150, false)),
        ("img" | "video" | "input", "width") => Some(("width", 0, false)),
        ("img" | "video" | "input", "height") => Some(("height", 0, false)),
        ("img", "hspace") => Some(("hspace", 0, false)),
        ("img", "vspace") => Some(("vspace", 0, false)),
        ("textarea", "rows") => Some(("rows", 2, true)),
        ("textarea", "cols") => Some(("cols", 20, true)),
        ("input", "size") => Some(("size", 20, true)),
        ("select", "size") => Some(("size", 0, false)),
        ("col" | "colgroup", "span") => Some(("span", 1, true)),
        ("td" | "th", "colSpan") => Some(("colspan", 1, true)),
        ("td" | "th", "rowSpan") => Some(("rowspan", 1, false)),
        _ => None,
    }
}

fn iframe_content_document(id: usize, context: &mut Context) -> JsResult<JsValue> {
    let sub_opt = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let res = ensure_iframe_document(&mut data, id);
        if matches!(res, Ok(None)) {
            clear_cached_iframe_window(&mut data, id);
        }
        res
    }
    .map_err(|e| into_js_err(e, context))?;
    match sub_opt {
        Some(sub_id) => wrap(&format!("subdoc:{sub_id}"), context),
        None => Ok(JsValue::null()),
    }
}

fn iframe_content_window(id: usize, context: &mut Context) -> JsResult<JsValue> {
    let (sub_opt, cached_cw, window_proto, initial_keys) = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let sub_opt = ensure_iframe_document(&mut data, id).map_err(|e| into_js_err(e, context))?;
        let cached = match sub_opt {
            Some(sub_id) => data.dom_prototypes.as_ref().and_then(|dp| {
                dp.iframe_windows
                    .iter()
                    .find(|e| e.iframe_id == id && e.sub_id == sub_id)
                    .map(|e| e.window.clone())
            }),
            None => {
                clear_cached_iframe_window(&mut data, id);
                None
            }
        };
        let (wp, ik) = match data.dom_prototypes.as_ref() {
            Some(dp) => (dp.proto("Window"), dp.initial_window_keys.clone()),
            None => (None, Vec::new()),
        };
        (sub_opt, cached, wp, ik)
    };
    let Some(sub_id) = sub_opt else {
        return Ok(JsValue::null());
    };
    if let Some(cw) = cached_cw {
        return Ok(cw.into());
    }
    let subdoc_js = wrap(&format!("subdoc:{sub_id}"), context)?;
    let cw = match window_proto {
        Some(wp) => JsObject::from_proto_and_data(Some(wp), ()),
        None => JsObject::with_object_proto(context.intrinsics()),
    };
    def_prop(
        &cw,
        js_string!("document"),
        subdoc_js,
        false,
        true,
        true,
        context,
    )?;
    let cw_val: JsValue = cw.clone().into();
    for alias in ["window", "self", "frames"] {
        def_prop(
            &cw,
            JsString::from(alias),
            cw_val.clone(),
            false,
            true,
            true,
            context,
        )?;
    }
    let top_obj = context.global_object();
    let top_val: JsValue = top_obj.clone().into();
    for alias in ["parent", "top"] {
        def_prop(
            &cw,
            JsString::from(alias),
            top_val.clone(),
            false,
            true,
            true,
            context,
        )?;
    }
    for global_name in &initial_keys {
        if matches!(
            global_name.as_str(),
            "window" | "self" | "frames" | "parent" | "top" | "document" | "globalThis"
        ) {
            continue;
        }
        let pk = PropertyKey::from(JsString::from(global_name.as_str()));
        if let Some(desc) = top_obj.borrow().properties().get(&pk) {
            let mut copy_desc = desc.clone();
            if !copy_desc.configurable().unwrap_or(true) {
                if let Some(val) = copy_desc.value().cloned() {
                    copy_desc = PropertyDescriptor::builder()
                        .value(val)
                        .writable(copy_desc.writable().unwrap_or(false))
                        .enumerable(copy_desc.enumerable().unwrap_or(false))
                        .configurable(true)
                        .build();
                } else {
                    continue;
                }
            }
            let _ = cw.define_property_or_throw(pk, copy_desc, context);
        }
    }
    def_prop(
        &cw,
        js_string!("globalThis"),
        cw_val.clone(),
        true,
        false,
        true,
        context,
    )?;
    {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if let Some(dp) = data.dom_prototypes.as_mut() {
            dp.iframe_windows.retain(|e| e.iframe_id != id);
            if dp.iframe_windows.len() < 64 {
                dp.iframe_windows.push(CachedIframeWindow {
                    iframe_id: id,
                    sub_id,
                    window: cw.clone(),
                });
            }
        }
    }
    Ok(cw_val)
}

fn ensure_iframe_document(data: &mut State, iframe_id: usize) -> Result<Option<usize>, DomOpErr> {
    if !is_connected_node(&data.host.document.nodes, iframe_id) {
        return Ok(None);
    }
    let is_blank_or_srcdoc = match data.host.document.nodes[iframe_id].attr("src") {
        None => true,
        Some(raw) => {
            let trimmed = raw.trim();
            trimmed.is_empty() || trimmed.eq_ignore_ascii_case("about:blank")
        }
    };
    if !is_blank_or_srcdoc {
        return Ok(None);
    }
    if let Some(&existing) = data.iframe_documents.get(&iframe_id)
        && existing < data.host.document.nodes.len()
    {
        return Ok(Some(existing));
    }
    let srcdoc = data.host.document.nodes[iframe_id]
        .attr("srcdoc")
        .map(str::to_owned);
    let sub_root = data
        .host
        .new_node("#document-subroot".into(), String::new())
        .map_err(DomOpErr::Host)?;
    let top_nodes = if let Some(html) = srcdoc {
        parse_html_into_fragment_children(data, &html).map_err(DomOpErr::Host)?
    } else {
        Vec::new()
    };
    ensure_html_skeleton(data, sub_root, top_nodes)?;
    data.iframe_documents.insert(iframe_id, sub_root);
    Ok(Some(sub_root))
}

fn find_in_subtree(
    nodes: &[Node],
    root: usize,
    predicate: impl Fn(&Node) -> bool,
) -> Option<usize> {
    let mut stack: Vec<usize> = nodes[root].children.iter().rev().copied().collect();
    for _ in 0..crate::document::MAX_NODES {
        let id = stack.pop()?;
        if predicate(&nodes[id]) {
            return Some(id);
        }
        stack.extend(nodes[id].children.iter().rev().copied());
    }
    None
}

fn subdoc_get(sub_id: usize, name: &str, context: &mut Context) -> JsResult<Option<JsValue>> {
    if let Some(cval) = node_constant(name) {
        return Ok(Some(cval.into()));
    }
    if let Some(m) = doc_shared_method(name)
        .or_else(|| common_node_method(name))
        .or_else(|| parent_node_method(name))
    {
        return method(m, context).map(|o| Some(o.into()));
    }
    match name {
        "nodeType" => Ok(Some(9.into())),
        "nodeName" => Ok(Some(js_string!("#document").into())),
        "ownerDocument" | "parentNode" | "parentElement" | "nodeValue" | "currentScript" => {
            Ok(Some(JsValue::null()))
        }
        "baseURI" => {
            let u = state(context).borrow().host.base_url().to_string();
            Ok(Some(JsString::from(u.as_str()).into()))
        }
        "isConnected" => Ok(Some(false.into())),
        "readyState" => Ok(Some(js_string!("complete").into())),
        "documentElement" | "firstElementChild" => {
            let found = {
                let shared = state(context);
                let data = shared.borrow();
                data.host.document.nodes[sub_id]
                    .children
                    .iter()
                    .copied()
                    .find(|&c| !data.host.document.nodes[c].tag.starts_with('#'))
            };
            match found {
                Some(id) => wrap(&format!("node:{id}"), context).map(Some),
                None => Ok(Some(JsValue::null())),
            }
        }
        "head" | "body" => {
            let found = {
                let shared = state(context);
                let data = shared.borrow();
                find_in_subtree(&data.host.document.nodes, sub_id, |n| n.tag == name)
            };
            match found {
                Some(id) => wrap(&format!("node:{id}"), context).map(Some),
                None => Ok(Some(JsValue::null())),
            }
        }
        "title" => {
            let t = {
                let shared = state(context);
                let data = shared.borrow();
                find_in_subtree(&data.host.document.nodes, sub_id, |n| n.tag == "title")
                    .map(|tid| text_content(&data.host.document.nodes, tid))
                    .unwrap_or_default()
            };
            Ok(Some(JsString::from(t.as_str()).into()))
        }
        "children" | "childNodes" => {
            let col = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let kids: Vec<usize> = data.host.document.nodes[sub_id]
                    .children
                    .iter()
                    .copied()
                    .filter(|&c| {
                        name == "childNodes" || !data.host.document.nodes[c].tag.starts_with('#')
                    })
                    .collect();
                data.host.collection(kids).map_err(failure)?
            };
            from_host(col, context).map(Some)
        }
        "querySelector" => method("host.dom.querySelector", context).map(|o| Some(o.into())),
        "querySelectorAll" => method("host.dom.querySelectorAll", context).map(|o| Some(o.into())),
        "getElementById" => method("host.dom.getElementById", context).map(|o| Some(o.into())),
        "getElementsByTagName" => {
            method("host.dom.getElementsByTagName", context).map(|o| Some(o.into()))
        }
        "getElementsByClassName" => {
            method("host.dom.getElementsByClassName", context).map(|o| Some(o.into()))
        }
        "createElement" => method("host.dom.createElement", context).map(|o| Some(o.into())),
        "createTextNode" => method("host.dom.createTextNode", context).map(|o| Some(o.into())),
        "appendChild" => method("host.dom.appendChild", context).map(|o| Some(o.into())),
        "removeChild" => method("host.dom.removeChild", context).map(|o| Some(o.into())),
        _ => Ok(None),
    }
}

// --- Method Calls ---

fn node_arg(val: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(val, context)? else {
        return Err(failure("Expected a Node argument"));
    };
    let shared = state(context);
    let data = shared.borrow();
    node_id_from_handle(&handle, &data).map_err(failure)
}

fn receiver_node(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(this, context)? else {
        return Err(failure("DOM method requires a Node receiver"));
    };
    let shared = state(context);
    let data = shared.borrow();
    node_id_from_handle(&handle, &data).map_err(failure)
}

pub(super) fn call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    match name {
        // Intercept `appendChild` so `DocumentFragment` splices its children cleanly!
        "host.dom.appendChild" => {
            let parent = receiver_node(this, context)?;
            let first = args
                .first()
                .ok_or_else(|| failure("appendChild requires 1 argument"))?;
            let child = node_arg(first, context)?;
            let res = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                insert_node_before(&mut data, parent, child, None)
            };
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            wrap_node_id(child, context)
        }
        "host.dom.removeChild" => {
            let parent = receiver_node(this, context)?;
            let first = args
                .first()
                .ok_or_else(|| failure("removeChild requires 1 argument"))?;
            let child = node_arg(first, context)?;
            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if !data.host.document.nodes[parent].children.contains(&child) {
                    drop(data);
                    return Err(dom_exception::exception(
                        "NotFoundError",
                        "The node to be removed is not a child of this node",
                        context,
                    )?);
                }
                detach_child_node(&mut data, parent, child);
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
            }
            super::observers_wc::after_dom_mutation(context)?;
            wrap_node_id(child, context)
        }
        "host.dom.getElementById" => {
            let root = receiver_node(this, context)?;
            let is_valid_root = {
                let shared = state(context);
                let data = shared.borrow();
                root == 0
                    || matches!(
                        data.host.document.nodes[root].tag.as_str(),
                        "#document-subroot" | "#document-fragment"
                    )
            };
            if !is_valid_root {
                return Err(failure(
                    "getElementById requires a Document or DocumentFragment receiver",
                ));
            }
            let id_str = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
            modern::check_context(context)?;
            if id_str.is_empty() {
                return Ok(JsValue::null());
            }
            let found = {
                let shared = state(context);
                let data = shared.borrow();
                find_in_subtree(&data.host.document.nodes, root, |n| {
                    !n.tag.starts_with('#') && n.attr("id") == Some(id_str.as_str())
                })
            };
            match found {
                Some(id) => wrap(&format!("node:{id}"), context),
                None => Ok(JsValue::null()),
            }
        }
        "host.dom.getElementsByTagName" => {
            let root = receiver_node(this, context)?;
            if state(context).borrow().host.document.nodes[root].tag == "#text" {
                return Err(failure(
                    "getElementsByTagName requires a Document or Element receiver",
                ));
            }
            let first = args
                .first()
                .ok_or_else(|| failure("getElementsByTagName requires 1 argument"))?;
            let q = utf8(first, context)?;
            modern::check_context(context)?;
            let wanted = q.to_ascii_lowercase();
            let col = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let mut ids = Vec::new();
                if !wanted.is_empty() {
                    let mut pending: Vec<usize> = data.host.document.nodes[root]
                        .children
                        .iter()
                        .rev()
                        .copied()
                        .collect();
                    for _ in 0..crate::document::MAX_NODES {
                        let Some(id) = pending.pop() else { break };
                        let node = &data.host.document.nodes[id];
                        if !node.tag.starts_with('#') && (wanted == "*" || node.tag == wanted) {
                            ids.push(id);
                        }
                        pending.extend(node.children.iter().rev().copied());
                    }
                }
                data.host.collection(ids).map_err(failure)?
            };
            from_host(col, context)
        }
        "host.dom.getElementsByClassName" => {
            let root = receiver_node(this, context)?;
            if state(context).borrow().host.document.nodes[root].tag == "#text" {
                return Err(failure(
                    "getElementsByClassName requires a Document or Element receiver",
                ));
            }
            let first = args
                .first()
                .ok_or_else(|| failure("getElementsByClassName requires 1 argument"))?;
            let q = utf8(first, context)?;
            modern::check_context(context)?;
            let col = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let wanted: Vec<&str> = q.split_ascii_whitespace().collect();
                let mut ids = Vec::new();
                if !wanted.is_empty() {
                    let mut pending: Vec<usize> = data.host.document.nodes[root]
                        .children
                        .iter()
                        .rev()
                        .copied()
                        .collect();
                    for _ in 0..crate::document::MAX_NODES {
                        let Some(id) = pending.pop() else { break };
                        let node = &data.host.document.nodes[id];
                        if !node.tag.starts_with('#') {
                            let classes = node.attr("class").unwrap_or("");
                            if wanted
                                .iter()
                                .all(|w| classes.split_ascii_whitespace().any(|c| c == *w))
                            {
                                ids.push(id);
                            }
                        }
                        pending.extend(node.children.iter().rev().copied());
                    }
                }
                data.host.collection(ids).map_err(failure)?
            };
            from_host(col, context)
        }
        "host.dom.createElement" => {
            let doc = receiver_node(this, context)?;
            if doc != 0
                && state(context).borrow().host.document.nodes[doc].tag != "#document-subroot"
            {
                return Err(failure("createElement requires a Document receiver"));
            }
            let raw = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
            modern::check_context(context)?;
            let is_opt = if let Some(opts) = args.get(1).and_then(JsValue::as_object) {
                let v = opts.get(js_string!("is"), context)?;
                if v.is_undefined() {
                    None
                } else {
                    let s = utf8(&v, context)?;
                    modern::check_context(context)?;
                    Some(s)
                }
            } else {
                None
            };
            let tag = raw.to_ascii_lowercase();
            if tag.is_empty()
                || !tag.as_bytes()[0].is_ascii_alphabetic()
                || !super::super::valid_name(&tag)
            {
                return Err(dom_exception::exception(
                    "InvalidCharacterError",
                    "Invalid element tag name",
                    context,
                )?);
            }
            let id = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let id = data
                    .host
                    .new_node(tag.clone(), String::new())
                    .map_err(failure)?;
                if tag == "template" {
                    ensure_template_content(&mut data, id).map_err(failure)?;
                }
                id
            };
            if doc == 0 {
                super::observers_wc::upgrade_created_element_if_defined(
                    id,
                    is_opt.as_deref(),
                    context,
                )?;
            }
            wrap(&format!("node:{id}"), context)
        }
        "host.dom.createTextNode" => {
            let doc = receiver_node(this, context)?;
            if doc != 0
                && state(context).borrow().host.document.nodes[doc].tag != "#document-subroot"
            {
                return Err(failure("createTextNode requires a Document receiver"));
            }
            let text = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
            modern::check_context(context)?;
            let id = state(context)
                .borrow_mut()
                .host
                .new_node("#text".into(), text)
                .map_err(failure)?;
            wrap(&format!("node:{id}"), context)
        }
        CREATE_DOCUMENT_FRAGMENT => {
            let doc = receiver_node(this, context)?;
            if doc != 0
                && state(context).borrow().host.document.nodes[doc].tag != "#document-subroot"
            {
                return Err(failure(
                    "createDocumentFragment requires a Document receiver",
                ));
            }
            let id = state(context)
                .borrow_mut()
                .host
                .new_node("#document-fragment".into(), String::new())
                .map_err(failure)?;
            wrap(&format!("node:{id}"), context)
        }
        CLONE_NODE => {
            let src = receiver_node(this, context)?;
            let deep = args.first().is_some_and(JsValue::to_boolean);
            let clone_id = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                clone_subtree(&mut data, src, deep).map_err(failure)?
            };
            wrap_node_id(clone_id, context)
        }
        INSERT_BEFORE => {
            let parent = receiver_node(this, context)?;
            if args.len() < 2 {
                return Err(failure("insertBefore requires 2 arguments"));
            }
            let new_child = node_arg(&args[0], context)?;
            let ref_child = if args[1].is_null() || args[1].is_undefined() {
                None
            } else {
                Some(node_arg(&args[1], context)?)
            };
            let res = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                insert_node_before(&mut data, parent, new_child, ref_child)
            };
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            wrap_node_id(new_child, context)
        }
        REPLACE_CHILD => {
            let parent = receiver_node(this, context)?;
            if args.len() < 2 {
                return Err(failure("replaceChild requires 2 arguments"));
            }
            let new_child = node_arg(&args[0], context)?;
            let old_child = node_arg(&args[1], context)?;
            let res: Result<(), DomOpErr> = (|| {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if !data.host.document.nodes[parent]
                    .children
                    .contains(&old_child)
                {
                    Err(DomOpErr::Dom(
                        "NotFoundError",
                        "The node to be replaced is not a child of this node",
                    ))
                } else {
                    if new_child != old_child {
                        insert_node_before(&mut data, parent, new_child, Some(old_child))?;
                        detach_child_node(&mut data, parent, old_child);
                        data.dom_epoch = data.dom_epoch.wrapping_add(1);
                    }
                    Ok(())
                }
            })();
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            wrap_node_id(old_child, context)
        }
        REMOVE => {
            let id = receiver_node(this, context)?;
            let mutated = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let p = data.host.document.nodes[id].parent;
                if id != 0 && p != id {
                    detach_child_node(&mut data, p, id);
                    data.dom_epoch = data.dom_epoch.wrapping_add(1);
                    true
                } else {
                    false
                }
            };
            if mutated {
                super::observers_wc::after_dom_mutation(context)?;
            }
            Ok(JsValue::undefined())
        }
        BEFORE | AFTER | REPLACE_WITH => {
            let id = receiver_node(this, context)?;
            let arg_ids = convert_nodes_or_strings(args, context)?;
            let res: Result<(), DomOpErr> = (|| {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let parent = data.host.document.nodes[id].parent;
                if id == 0 || parent == id {
                    Ok(())
                } else {
                    // Find next viable sibling not in `arg_ids`
                    let ref_sibling = {
                        let kids = &data.host.document.nodes[parent].children;
                        let pos = kids.iter().position(|&x| x == id).unwrap_or(0);
                        if name == BEFORE {
                            kids[..pos]
                                .iter()
                                .rev()
                                .copied()
                                .find(|x| !arg_ids.contains(x))
                                .map(|prev| {
                                    let p_idx = kids.iter().position(|&k| k == prev).unwrap();
                                    kids.get(p_idx + 1).copied()
                                })
                                .unwrap_or_else(|| {
                                    kids.iter().copied().find(|x| !arg_ids.contains(x))
                                })
                        } else {
                            kids[pos + 1..]
                                .iter()
                                .copied()
                                .find(|x| !arg_ids.contains(x))
                        }
                    };
                    if let Some(node_to_insert) = build_temp_fragment_if_needed(&mut data, arg_ids)?
                    {
                        insert_node_before(&mut data, parent, node_to_insert, ref_sibling)?;
                    }
                    if name == REPLACE_WITH && data.host.document.nodes[id].parent == parent {
                        detach_child_node(&mut data, parent, id);
                        data.dom_epoch = data.dom_epoch.wrapping_add(1);
                    }
                    Ok(())
                }
            })();
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            Ok(JsValue::undefined())
        }
        PREPEND | APPEND | REPLACE_CHILDREN => {
            let parent = receiver_node(this, context)?;
            let arg_ids = convert_nodes_or_strings(args, context)?;
            let res: Result<(), DomOpErr> = (|| {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let node_opt = build_temp_fragment_if_needed(&mut data, arg_ids)?;
                if name == REPLACE_CHILDREN {
                    if let Some(n) = node_opt {
                        check_insert_hierarchy(&data.host.document.nodes, parent, n)
                            .map_err(|(k, m)| DomOpErr::Dom(k, m))?;
                    }
                    let track = super::observers_wc::has_any_interest(&data);
                    let was_connected = if track {
                        is_connected_node(&data.host.document.nodes, parent)
                    } else {
                        false
                    };
                    let old_children =
                        std::mem::take(&mut data.host.document.nodes[parent].children);
                    for &old in &old_children {
                        data.host.document.nodes[old].parent = old;
                    }
                    if track && !old_children.is_empty() {
                        super::observers_wc::record_child_list_mutation(
                            &mut data,
                            parent,
                            &[],
                            &old_children,
                            None,
                            None,
                            was_connected,
                        );
                    }
                    if let Some(n) = node_opt {
                        insert_node_before(&mut data, parent, n, None)?;
                    }
                    data.dom_epoch = data.dom_epoch.wrapping_add(1);
                } else if let Some(n) = node_opt {
                    let ref_child = if name == PREPEND {
                        data.host.document.nodes[parent]
                            .children
                            .iter()
                            .copied()
                            .find(|&c| c != n)
                    } else {
                        None
                    };
                    insert_node_before(&mut data, parent, n, ref_child)?;
                }
                Ok(())
            })();
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            Ok(JsValue::undefined())
        }
        CONTAINS => {
            let ancestor = receiver_node(this, context)?;
            let Some(first) = args.first() else {
                return Ok(false.into());
            };
            if first.is_null() || first.is_undefined() {
                return Ok(false.into());
            }
            let other = node_arg(first, context)?;
            let shared = state(context);
            let data = shared.borrow();
            let mut cur = other;
            for _ in 0..=crate::document::MAX_DEPTH {
                if cur == ancestor {
                    return Ok(true.into());
                }
                let p = data.host.document.nodes[cur].parent;
                if cur == 0 || p == cur {
                    break;
                }
                cur = p;
            }
            Ok(false.into())
        }
        HAS_CHILD_NODES => {
            let id = receiver_node(this, context)?;
            let has = !state(context).borrow().host.document.nodes[id]
                .children
                .is_empty();
            Ok(has.into())
        }
        HAS_ATTRIBUTE => {
            let id = receiver_node(this, context)?;
            let attr =
                utf8(args.first().unwrap_or(&JsValue::undefined()), context)?.to_ascii_lowercase();
            modern::check_context(context)?;
            let shared = state(context);
            let data = shared.borrow();
            if id == 0 || data.host.document.nodes[id].tag.starts_with('#') {
                return Err(failure("hasAttribute requires an Element receiver"));
            }
            if attr == crate::document::INTERNAL_CANVAS_ATTR {
                return Ok(false.into());
            }
            Ok(data.host.document.nodes[id].has(&attr).into())
        }
        TOGGLE_ATTRIBUTE => {
            let id = receiver_node(this, context)?;
            let attr =
                utf8(args.first().unwrap_or(&JsValue::undefined()), context)?.to_ascii_lowercase();
            let force = args
                .get(1)
                .filter(|v| !v.is_undefined())
                .map(JsValue::to_boolean);
            modern::check_context(context)?;
            if !super::super::valid_name(&attr) {
                return Err(dom_exception::exception(
                    "InvalidCharacterError",
                    "Invalid attribute name",
                    context,
                )?);
            }
            let present = {
                let shared = state(context);
                let data = shared.borrow();
                if id == 0 || data.host.document.nodes[id].tag.starts_with('#') {
                    return Err(failure("toggleAttribute requires an Element receiver"));
                }
                if attr == crate::document::INTERNAL_CANVAS_ATTR {
                    return Ok(force.unwrap_or(true).into());
                }
                data.host.document.nodes[id].has(&attr)
            };
            let next = force.unwrap_or(!present);
            if next && !present {
                set_element_attr_observed(id, &attr, String::new(), context)?;
            } else if !next && present {
                remove_element_attr_observed(id, &attr, context)?;
            }
            Ok(next.into())
        }
        GET_ATTRIBUTE_NAMES => {
            let id = receiver_node(this, context)?;
            let names: Vec<String> = {
                let shared = state(context);
                let data = shared.borrow();
                if id == 0 || data.host.document.nodes[id].tag.starts_with('#') {
                    return Err(failure("getAttributeNames requires an Element receiver"));
                }
                data.host.document.nodes[id]
                    .attributes
                    .iter()
                    .filter(|(k, _)| k != crate::document::INTERNAL_CANVAS_ATTR)
                    .map(|(k, _)| k.clone())
                    .collect()
            };
            let arr = modern::boa_engine::object::builtins::JsArray::new(context)?;
            for n in names {
                arr.push(JsString::from(n.as_str()), context)?;
            }
            Ok(arr.into())
        }
        INSERT_ADJACENT_HTML | INSERT_ADJACENT_ELEMENT | INSERT_ADJACENT_TEXT => {
            let target = receiver_node(this, context)?;
            if args.len() < 2 {
                return Err(failure("2 arguments required"));
            }
            let pos = utf8(&args[0], context)?.to_ascii_lowercase();
            modern::check_context(context)?;
            if !matches!(
                pos.as_str(),
                "beforebegin" | "afterbegin" | "beforeend" | "afterend"
            ) {
                return Err(dom_exception::exception(
                    "SyntaxError",
                    "Invalid insertAdjacent position",
                    context,
                )?);
            }
            {
                let shared = state(context);
                let data = shared.borrow();
                if target == 0 || data.host.document.nodes[target].tag.starts_with('#') {
                    return Err(failure("insertAdjacent requires an Element receiver"));
                }
                if matches!(pos.as_str(), "beforebegin" | "afterend") {
                    let p = data.host.document.nodes[target].parent;
                    if p == target
                        || p == 0
                        || data.host.document.nodes[p].tag == "#document-subroot"
                    {
                        drop(data);
                        return Err(dom_exception::exception(
                            "NoModificationAllowedError",
                            "The element has no parent or parent is a Document",
                            context,
                        )?);
                    }
                }
            }
            match name {
                INSERT_ADJACENT_ELEMENT => {
                    let el = node_arg(&args[1], context)?;
                    {
                        let shared = state(context);
                        let data = shared.borrow();
                        if el == 0 || data.host.document.nodes[el].tag.starts_with('#') {
                            return Err(failure("insertAdjacentElement requires an Element"));
                        }
                    }
                    let inserted = insert_adjacent_nodes(target, &pos, &[el], context)?;
                    if !inserted {
                        return Ok(JsValue::null());
                    }
                    super::observers_wc::after_dom_mutation(context)?;
                    wrap(&format!("node:{el}"), context)
                }
                INSERT_ADJACENT_TEXT => {
                    let text = utf8(&args[1], context)?;
                    modern::check_context(context)?;
                    let tid = state(context)
                        .borrow_mut()
                        .host
                        .new_node("#text".into(), text)
                        .map_err(failure)?;
                    insert_adjacent_nodes(target, &pos, &[tid], context)?;
                    super::observers_wc::after_dom_mutation(context)?;
                    Ok(JsValue::undefined())
                }
                _ => {
                    let html = utf8(&args[1], context)?;
                    modern::check_context(context)?;
                    let children = {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        parse_html_into_fragment_children(&mut data, &html).map_err(failure)?
                    };
                    insert_adjacent_nodes(target, &pos, &children, context)?;
                    super::observers_wc::after_dom_mutation(context)?;
                    Ok(JsValue::undefined())
                }
            }
        }
        DIALOG_SHOW | DIALOG_SHOW_MODAL => {
            let id = receiver_node(this, context)?;
            let mutated = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if data.host.document.nodes[id].tag != "dialog" {
                    return Err(failure("Dialog method requires a HTMLDialogElement"));
                }
                if data.host.document.nodes[id].has("open") {
                    if name == DIALOG_SHOW_MODAL {
                        drop(data);
                        return Err(dom_exception::exception(
                            "InvalidStateError",
                            "The dialog is already open",
                            context,
                        )?);
                    }
                    return Ok(JsValue::undefined());
                }
                if name == DIALOG_SHOW_MODAL && !is_connected_node(&data.host.document.nodes, id) {
                    drop(data);
                    return Err(dom_exception::exception(
                        "InvalidStateError",
                        "The dialog is not connected to a document",
                        context,
                    )?);
                }
                let track = super::observers_wc::has_any_interest(&data);
                data.host.attr(id, "open", String::new()).map_err(failure)?;
                if track {
                    super::observers_wc::record_attribute_mutation(
                        &mut data,
                        id,
                        "open",
                        None,
                        Some(String::new()),
                    );
                }
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
                track
            };
            if mutated {
                super::observers_wc::after_dom_mutation(context)?;
            }
            Ok(JsValue::undefined())
        }
        DIALOG_CLOSE => {
            let id = receiver_node(this, context)?;
            let rv_arg = if let Some(first) = args.first()
                && !first.is_undefined()
            {
                let s = utf8(first, context)?;
                modern::check_context(context)?;
                Some(s)
            } else {
                None
            };
            let (was_open, mutated) = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if data.host.document.nodes[id].tag != "dialog" {
                    return Err(failure("Dialog method requires a HTMLDialogElement"));
                }
                let open = data.host.document.nodes[id].has("open");
                let mut did_obs = false;
                if open {
                    let track = super::observers_wc::has_any_interest(&data);
                    let old = if track {
                        data.host.document.nodes[id].attr("open").map(str::to_owned)
                    } else {
                        None
                    };
                    data.host.document.nodes[id]
                        .attributes
                        .retain(|(n, _)| n != "open");
                    if let Some(rv) = rv_arg {
                        data.host.charge(rv.len()).map_err(failure)?;
                        data.dialog_return_values.insert(id, rv);
                    }
                    if track {
                        super::observers_wc::record_attribute_mutation(
                            &mut data, id, "open", old, None,
                        );
                        did_obs = true;
                    }
                    data.dom_epoch = data.dom_epoch.wrapping_add(1);
                }
                (open, did_obs)
            };
            if mutated {
                super::observers_wc::after_dom_mutation(context)?;
            }
            if was_open {
                dom_events::queue_or_fire_simple_event(id, "close", false, false, context)?;
            }
            Ok(JsValue::undefined())
        }
        DOM_PARSER_PARSE => {
            if args.len() < 2 {
                return Err(failure("DOMParser.parseFromString requires 2 arguments"));
            }
            let markup = utf8(&args[0], context)?;
            let mime = utf8(&args[1], context)?;
            modern::check_context(context)?;
            match mime.as_str() {
                "text/html" => {
                    let res: Result<usize, DomOpErr> = {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        (|| {
                            let sub = data
                                .host
                                .new_node("#document-subroot".into(), String::new())
                                .map_err(DomOpErr::Host)?;
                            let top = parse_html_into_fragment_children(&mut data, &markup)
                                .map_err(DomOpErr::Host)?;
                            ensure_html_skeleton(&mut data, sub, top)?;
                            Ok(sub)
                        })()
                    };
                    let sub_id = res.map_err(|e| into_js_err(e, context))?;
                    wrap(&format!("subdoc:{sub_id}"), context)
                }
                "text/xml" | "application/xml" | "application/xhtml+xml" | "image/svg+xml" => {
                    let res: Result<usize, DomOpErr> = {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        (|| {
                            let sub = data
                                .host
                                .new_node("#document-subroot".into(), String::new())
                                .map_err(DomOpErr::Host)?;
                            match crate::document::parse_xml_nodes(&markup) {
                                Ok(xml_nodes) => {
                                    let top =
                                        import_parsed_children(&mut data, xml_nodes, markup.len())
                                            .map_err(DomOpErr::Host)?;
                                    for ch in top {
                                        insert_node_before(&mut data, sub, ch, None)?;
                                    }
                                }
                                Err(msg) => {
                                    let err_el = data
                                        .host
                                        .new_node("parsererror".into(), String::new())
                                        .map_err(DomOpErr::Host)?;
                                    data.host
                                        .replace_text(err_el, msg.into())
                                        .map_err(DomOpErr::Host)?;
                                    insert_node_before(&mut data, sub, err_el, None)?;
                                }
                            }
                            Ok(sub)
                        })()
                    };
                    let sub_id = res.map_err(|e| into_js_err(e, context))?;
                    wrap(&format!("subdoc:{sub_id}"), context)
                }
                _ => Err(failure(format!(
                    "DOMParser.parseFromString: unsupported MIME type '{mime}'"
                ))),
            }
        }
        CSS_SUPPORTS => {
            if args.is_empty() {
                return Err(failure("CSS.supports requires at least 1 argument"));
            }
            if args.len() >= 2 {
                let prop = utf8(&args[0], context)?;
                let val = utf8(&args[1], context)?;
                modern::check_context(context)?;
                Ok(crate::style::css_supports_declaration(&prop, &val).into())
            } else {
                let cond = utf8(&args[0], context)?;
                modern::check_context(context)?;
                Ok(crate::style::css_supports_condition(&cond).into())
            }
        }
        CSS_ESCAPE => {
            let Some(first) = args.first() else {
                return Err(failure("CSS.escape requires 1 argument"));
            };
            let ident = utf8(first, context)?;
            modern::check_context(context)?;
            Ok(JsString::from(css_escape(&ident).as_str()).into())
        }
        DOC_HAS_FOCUS => {
            let Value::Host(handle) = receiver(this, context)? else {
                return Err(failure("Document.hasFocus requires a Document receiver"));
            };
            if handle != "document" && !handle.starts_with("subdoc:") {
                return Err(failure("Document.hasFocus requires a Document receiver"));
            }
            Ok((handle == "document").into())
        }
        GET_ROOT_NODE => {
            let id = receiver_node(this, context)?;
            let root = {
                let shared = state(context);
                let data = shared.borrow();
                root_of(&data.host.document.nodes, id)
            };
            wrap_node_id(root, context)
        }
        COMPARE_DOCUMENT_POSITION => {
            let self_id = receiver_node(this, context)?;
            let Some(first) = args.first() else {
                return Err(failure("compareDocumentPosition requires 1 Node argument"));
            };
            let Value::Host(other_h) = receiver(first, context)? else {
                return Err(failure("compareDocumentPosition requires a Node argument"));
            };
            let bits = {
                let shared = state(context);
                let data = shared.borrow();
                let other_id = node_id_from_handle(&other_h, &data).map_err(failure)?;
                compare_document_position_impl(&data.host.document.nodes, self_id, other_id)
            };
            Ok(bits.into())
        }
        CREATE_TREE_WALKER => {
            let Value::Host(doc_h) = receiver(this, context)? else {
                return Err(failure(
                    "Document.createTreeWalker requires a Document receiver",
                ));
            };
            if doc_h != "document" && !doc_h.starts_with("subdoc:") {
                return Err(failure(
                    "Document.createTreeWalker requires a Document receiver",
                ));
            }
            let Some(root_arg) = args.first() else {
                return Err(failure("createTreeWalker requires a root Node"));
            };
            let Value::Host(root_h) = receiver(root_arg, context)? else {
                return Err(failure("createTreeWalker root must be a Node"));
            };
            let _ = {
                let shared = state(context);
                let data = shared.borrow();
                node_id_from_handle(&root_h, &data).map_err(failure)?
            };
            let what_to_show = match args.get(1) {
                Some(v) if !v.is_undefined() => v.to_u32(context)?,
                _ => 0xFFFF_FFFF,
            };
            let filter_val = match args.get(2) {
                Some(v) if v.is_object() => v.clone(),
                _ => JsValue::null(),
            };
            modern::check_context(context)?;
            build_tree_walker(root_arg.clone(), what_to_show, filter_val, context)
        }
        DOC_OPEN => {
            let Value::Host(doc_h) = receiver(this, context)? else {
                return Err(failure("Document.open requires a Document receiver"));
            };
            if doc_h != "document" && !doc_h.starts_with("subdoc:") {
                return Err(failure("Document.open requires a Document receiver"));
            }
            if let Some(sub_id) = doc_h
                .strip_prefix("subdoc:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                let res: Result<(), DomOpErr> = {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    for old in std::mem::take(&mut data.host.document.nodes[sub_id].children) {
                        data.host.document.nodes[old].parent = old;
                    }
                    ensure_html_skeleton(&mut data, sub_id, Vec::new())
                };
                res.map_err(|e| into_js_err(e, context))?;
            }
            Ok(this.clone())
        }
        DOC_CLOSE => {
            let Value::Host(doc_h) = receiver(this, context)? else {
                return Err(failure("Document.close requires a Document receiver"));
            };
            if doc_h != "document" && !doc_h.starts_with("subdoc:") {
                return Err(failure("Document.close requires a Document receiver"));
            }
            Ok(JsValue::undefined())
        }
        DOC_WRITE | DOC_WRITELN => {
            let Value::Host(doc_h) = receiver(this, context)? else {
                return Err(failure("Document.write requires a Document receiver"));
            };
            if doc_h != "document" && !doc_h.starts_with("subdoc:") {
                return Err(failure("Document.write requires a Document receiver"));
            }
            let mut combined = String::new();
            for arg in args {
                combined.push_str(&utf8(arg, context)?);
                modern::check_context(context)?;
            }
            if name == DOC_WRITELN {
                combined.push('\n');
            }
            if combined.is_empty() {
                return Ok(JsValue::undefined());
            }
            if let Some(sub_id) = doc_h
                .strip_prefix("subdoc:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                let res: Result<(), DomOpErr> = (|| {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    let top_nodes = parse_html_into_fragment_children(&mut data, &combined)
                        .map_err(DomOpErr::Host)?;
                    let head_id =
                        find_in_subtree(&data.host.document.nodes, sub_id, |n| n.tag == "head");
                    let body_id =
                        find_in_subtree(&data.host.document.nodes, sub_id, |n| n.tag == "body");
                    if head_id.is_none() || body_id.is_none() {
                        for old in std::mem::take(&mut data.host.document.nodes[sub_id].children) {
                            data.host.document.nodes[old].parent = old;
                        }
                        ensure_html_skeleton(&mut data, sub_id, top_nodes)
                    } else {
                        let (hid, bid) = (head_id.unwrap(), body_id.unwrap());
                        let mut queue = top_nodes;
                        let mut expanded = Vec::new();
                        for nid in queue.drain(..) {
                            let t = data.host.document.nodes[nid].tag.clone();
                            if t == "html" {
                                expanded.extend(data.host.document.nodes[nid].children.clone());
                            } else {
                                expanded.push(nid);
                            }
                        }
                        for nid in expanded {
                            let t = data.host.document.nodes[nid].tag.clone();
                            if t == "head" {
                                let kids = data.host.document.nodes[nid].children.clone();
                                for k in kids {
                                    insert_node_before(&mut data, hid, k, None)?;
                                }
                            } else if t == "body" {
                                let kids = data.host.document.nodes[nid].children.clone();
                                for k in kids {
                                    insert_node_before(&mut data, bid, k, None)?;
                                }
                            } else if matches!(
                                t.as_str(),
                                "title" | "meta" | "link" | "style" | "base"
                            ) {
                                insert_node_before(&mut data, hid, nid, None)?;
                            } else {
                                insert_node_before(&mut data, bid, nid, None)?;
                            }
                        }
                        Ok(())
                    }
                })();
                res.map_err(|e| into_js_err(e, context))?;
                return Ok(JsValue::undefined());
            }
            let res: Result<(), DomOpErr> = (|| {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if data.host.ready != "loading" {
                    return Ok(());
                }
                let children = parse_html_into_fragment_children(&mut data, &combined)
                    .map_err(DomOpErr::Host)?;
                let all_head_meta = !children.is_empty()
                    && children.iter().all(|&cid| {
                        matches!(
                            data.host.document.nodes[cid].tag.as_str(),
                            "title" | "meta" | "link" | "style" | "base"
                        )
                    });
                let script_insertion = data.current_script.and_then(|sid| {
                    if !is_connected_node(&data.host.document.nodes, sid) {
                        return None;
                    }
                    let p = data.host.document.nodes[sid].parent;
                    if p == sid {
                        return None;
                    }
                    if data.host.document.nodes[p].tag == "head" && !all_head_meta {
                        return None;
                    }
                    let kids = &data.host.document.nodes[p].children;
                    let idx = kids.iter().position(|&c| c == sid)?;
                    Some((p, kids.get(idx + 1).copied()))
                });
                if let Some((parent, next_sib)) = script_insertion {
                    for ch in children {
                        insert_node_before(&mut data, parent, ch, next_sib)?;
                    }
                } else {
                    let fallback_parent = if all_head_meta {
                        data.host
                            .find_connected(|n| n.tag == "head")
                            .or_else(|| data.host.find_connected(|n| n.tag == "body"))
                            .unwrap_or(0)
                    } else {
                        data.host
                            .find_connected(|n| n.tag == "body")
                            .or_else(|| data.host.find_connected(|n| n.tag == "html"))
                            .unwrap_or(0)
                    };
                    for ch in children {
                        insert_node_before(&mut data, fallback_parent, ch, None)?;
                    }
                }
                Ok(())
            })();
            res.map_err(|e| into_js_err(e, context))?;
            super::observers_wc::after_dom_mutation(context)?;
            Ok(JsValue::undefined())
        }
        IS_SAME_NODE => {
            let id = receiver_node(this, context)?;
            let Some(first) = args.first() else {
                return Ok(false.into());
            };
            if first.is_null() || first.is_undefined() {
                return Ok(false.into());
            }
            let other = node_arg(first, context)?;
            Ok((id == other).into())
        }
        IS_EQUAL_NODE => {
            let id = receiver_node(this, context)?;
            let Some(first) = args.first() else {
                return Ok(false.into());
            };
            if first.is_null() || first.is_undefined() {
                return Ok(false.into());
            }
            let other = node_arg(first, context)?;
            if id == other {
                return Ok(true.into());
            }
            let eq = {
                let shared = state(context);
                let data = shared.borrow();
                nodes_equal(&data.host.document.nodes, id, other)
            };
            Ok(eq.into())
        }
        NORMALIZE => {
            let id = receiver_node(this, context)?;
            let mutated = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                normalize_subtree(&mut data, id)
            };
            if mutated {
                super::observers_wc::after_dom_mutation(context)?;
            }
            Ok(JsValue::undefined())
        }
        LOOKUP_NAMESPACE_URI => {
            let id = receiver_node(this, context)?;
            let prefix = match args.first() {
                None => None,
                Some(v) if v.is_null() || v.is_undefined() => None,
                Some(v) => {
                    let s = utf8(v, context)?;
                    modern::check_context(context)?;
                    if s.is_empty() { None } else { Some(s) }
                }
            };
            match prefix.as_deref() {
                Some("xml") => return Ok(js_string!("http://www.w3.org/XML/1998/namespace").into()),
                Some("xmlns") => return Ok(js_string!("http://www.w3.org/2000/xmlns/").into()),
                Some(_) => return Ok(JsValue::null()),
                None => {
                    let ns = {
                        let shared = state(context);
                        let data = shared.borrow();
                        default_namespace_for_node(&data.host.document.nodes, id)
                    };
                    return Ok(match ns {
                        Some(s) => JsString::from(s).into(),
                        None => JsValue::null(),
                    });
                }
            }
        }
        LOOKUP_PREFIX => {
            let _id = receiver_node(this, context)?;
            let ns = match args.first() {
                None => None,
                Some(v) if v.is_null() || v.is_undefined() => None,
                Some(v) => {
                    let s = utf8(v, context)?;
                    modern::check_context(context)?;
                    if s.is_empty() { None } else { Some(s) }
                }
            };
            Ok(match ns.as_deref() {
                Some("http://www.w3.org/XML/1998/namespace") => js_string!("xml").into(),
                Some("http://www.w3.org/2000/xmlns/") => js_string!("xmlns").into(),
                _ => JsValue::null(),
            })
        }
        IS_DEFAULT_NAMESPACE => {
            let id = receiver_node(this, context)?;
            let ns = match args.first() {
                None => None,
                Some(v) if v.is_null() || v.is_undefined() => None,
                Some(v) => {
                    let s = utf8(v, context)?;
                    modern::check_context(context)?;
                    if s.is_empty() { None } else { Some(s) }
                }
            };
            let def_ns = {
                let shared = state(context);
                let data = shared.borrow();
                default_namespace_for_node(&data.host.document.nodes, id)
            };
            Ok((ns.as_deref() == def_ns).into())
        }
        HAS_ATTRIBUTES => {
            let id = receiver_element_node(this, "hasAttributes", context)?;
            let has = state(context).borrow().host.document.nodes[id]
                .attributes
                .iter()
                .any(|(k, _)| k != crate::document::INTERNAL_CANVAS_ATTR);
            Ok(has.into())
        }
        GET_ATTRIBUTE_NS => {
            let id = receiver_element_node(this, "getAttributeNS", context)?;
            if args.len() < 2 {
                return Err(failure("getAttributeNS requires 2 arguments"));
            }
            let ns = opt_nullable_str(&args[0], context)?;
            let local = utf8(&args[1], context)?;
            modern::check_context(context)?;
            if ns.is_some() {
                return Ok(JsValue::null());
            }
            let lower = local.to_ascii_lowercase();
            if lower == crate::document::INTERNAL_CANVAS_ATTR {
                return Ok(JsValue::null());
            }
            let val = state(context).borrow().host.document.nodes[id]
                .attr(&lower)
                .map(str::to_owned);
            Ok(match val {
                Some(v) => JsString::from(v.as_str()).into(),
                None => JsValue::null(),
            })
        }
        SET_ATTRIBUTE_NS => {
            let id = receiver_element_node(this, "setAttributeNS", context)?;
            if args.len() < 3 {
                return Err(failure("setAttributeNS requires 3 arguments"));
            }
            let _ns = opt_nullable_str(&args[0], context)?;
            let qname = utf8(&args[1], context)?;
            let val = utf8(&args[2], context)?;
            modern::check_context(context)?;
            let local = qname
                .rsplit_once(':')
                .map(|(_, l)| l)
                .unwrap_or(qname.as_str())
                .to_ascii_lowercase();
            if !super::super::valid_name(&local) || local == crate::document::INTERNAL_CANVAS_ATTR {
                return Err(dom_exception::exception(
                    "InvalidCharacterError",
                    "Invalid attribute name",
                    context,
                )?);
            }
            set_element_attr_observed(id, &local, val, context)?;
            Ok(JsValue::undefined())
        }
        REMOVE_ATTRIBUTE_NS => {
            let id = receiver_element_node(this, "removeAttributeNS", context)?;
            if args.len() < 2 {
                return Err(failure("removeAttributeNS requires 2 arguments"));
            }
            let ns = opt_nullable_str(&args[0], context)?;
            let local = utf8(&args[1], context)?.to_ascii_lowercase();
            modern::check_context(context)?;
            if ns.is_none() && local != crate::document::INTERNAL_CANVAS_ATTR {
                remove_element_attr_observed(id, &local, context)?;
            }
            Ok(JsValue::undefined())
        }
        HAS_ATTRIBUTE_NS => {
            let id = receiver_element_node(this, "hasAttributeNS", context)?;
            if args.len() < 2 {
                return Err(failure("hasAttributeNS requires 2 arguments"));
            }
            let ns = opt_nullable_str(&args[0], context)?;
            let local = utf8(&args[1], context)?.to_ascii_lowercase();
            modern::check_context(context)?;
            if ns.is_some() || local == crate::document::INTERNAL_CANVAS_ATTR {
                return Ok(false.into());
            }
            Ok(state(context).borrow().host.document.nodes[id]
                .has(&local)
                .into())
        }
        GET_ELEMENTS_BY_TAG_NAME_NS => {
            let root = receiver_node(this, context)?;
            if args.len() < 2 {
                return Err(failure("getElementsByTagNameNS requires 2 arguments"));
            }
            let ns = opt_nullable_str(&args[0], context)?;
            let local = utf8(&args[1], context)?;
            modern::check_context(context)?;
            let col = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let nodes = &data.host.document.nodes;
                if nodes[root].tag == "#text" || nodes[root].tag == "#comment" {
                    return Err(failure(
                        "getElementsByTagNameNS requires Document or Element",
                    ));
                }
                let mut items = Vec::new();
                let mut stack: Vec<usize> = nodes[root].children.iter().rev().copied().collect();
                for _ in 0..crate::document::MAX_NODES {
                    let Some(nid) = stack.pop() else { break };
                    let t = nodes[nid].tag.as_str();
                    if !t.starts_with('#') {
                        let el_ns = if is_svg_element(nodes, nid) {
                            "http://www.w3.org/2000/svg"
                        } else {
                            "http://www.w3.org/1999/xhtml"
                        };
                        let ns_ok = match ns.as_deref() {
                            Some("*") => true,
                            Some(want) => want == el_ns,
                            None => false,
                        };
                        let local_ok = local == "*" || t == local;
                        if ns_ok && local_ok {
                            items.push(nid);
                        }
                        if t != "template" {
                            stack.extend(nodes[nid].children.iter().rev().copied());
                        }
                    }
                }
                data.host.collection(items).map_err(failure)?
            };
            from_host(col, context)
        }
        GET_ATTRIBUTE_NODE => {
            let id = receiver_element_node(this, "getAttributeNode", context)?;
            let name_arg = args
                .first()
                .ok_or_else(|| failure("getAttributeNode requires 1 argument"))?;
            let attr_name = utf8(name_arg, context)?.to_ascii_lowercase();
            modern::check_context(context)?;
            if attr_name == crate::document::INTERNAL_CANVAS_ATTR {
                return Ok(JsValue::null());
            }
            let val = state(context).borrow().host.document.nodes[id]
                .attr(&attr_name)
                .map(str::to_owned);
            match val {
                Some(v) => build_single_attr_object(id, &attr_name, &v, context),
                None => Ok(JsValue::null()),
            }
        }
        GET_ATTRIBUTE_NODE_NS => {
            let id = receiver_element_node(this, "getAttributeNodeNS", context)?;
            if args.len() < 2 {
                return Err(failure("getAttributeNodeNS requires 2 arguments"));
            }
            let ns = opt_nullable_str(&args[0], context)?;
            let local = utf8(&args[1], context)?.to_ascii_lowercase();
            modern::check_context(context)?;
            if ns.is_some() || local == crate::document::INTERNAL_CANVAS_ATTR {
                return Ok(JsValue::null());
            }
            let val = state(context).borrow().host.document.nodes[id]
                .attr(&local)
                .map(str::to_owned);
            match val {
                Some(v) => build_single_attr_object(id, &local, &v, context),
                None => Ok(JsValue::null()),
            }
        }
        SET_ATTRIBUTE_NODE | SET_ATTRIBUTE_NODE_NS => {
            let id = receiver_element_node(this, "setAttributeNode", context)?;
            let attr_obj = args
                .first()
                .and_then(JsValue::as_object)
                .ok_or_else(|| failure("setAttributeNode requires an Attr object"))?;
            let raw_name = utf8(&attr_obj.get(js_string!("name"), context)?, context)?;
            let raw_val = utf8(&attr_obj.get(js_string!("value"), context)?, context)?;
            modern::check_context(context)?;
            let lower = raw_name.to_ascii_lowercase();
            if !super::super::valid_name(&lower) || lower == crate::document::INTERNAL_CANVAS_ATTR {
                return Err(dom_exception::exception(
                    "InvalidCharacterError",
                    "Invalid attribute name",
                    context,
                )?);
            }
            let prev = state(context).borrow().host.document.nodes[id]
                .attr(&lower)
                .map(str::to_owned);
            set_element_attr_observed(id, &lower, raw_val, context)?;
            match prev {
                Some(old_v) => build_single_attr_object(id, &lower, &old_v, context),
                None => Ok(JsValue::null()),
            }
        }
        REMOVE_ATTRIBUTE_NODE => {
            let id = receiver_element_node(this, "removeAttributeNode", context)?;
            let attr_obj = args
                .first()
                .and_then(JsValue::as_object)
                .ok_or_else(|| failure("removeAttributeNode requires an Attr object"))?;
            let raw_name = utf8(&attr_obj.get(js_string!("name"), context)?, context)?;
            modern::check_context(context)?;
            let lower = raw_name.to_ascii_lowercase();
            let prev = state(context).borrow().host.document.nodes[id]
                .attr(&lower)
                .map(str::to_owned);
            let Some(old_v) = prev else {
                return Err(dom_exception::exception(
                    "NotFoundError",
                    "The node provided is owned by another element or not found",
                    context,
                )?);
            };
            remove_element_attr_observed(id, &lower, context)?;
            build_single_attr_object(id, &lower, &old_v, context)
        }
        ATTACH_SHADOW => {
            let _id = receiver_element_node(this, "attachShadow", context)?;
            let init = args
                .first()
                .and_then(JsValue::as_object)
                .ok_or_else(|| failure("attachShadow requires a ShadowRootInit dictionary"))?;
            let mode_val = init.get(js_string!("mode"), context)?;
            let mode = utf8(&mode_val, context)?;
            modern::check_context(context)?;
            if !matches!(mode.as_str(), "open" | "closed") {
                return Err(failure(
                    "Failed to execute 'attachShadow': invalid ShadowRootMode",
                ));
            }
            Err(dom_exception::exception(
                "NotSupportedError",
                "Shadow DOM attachment is not supported for this element",
                context,
            )?)
        }
        CHECK_VISIBILITY => {
            let id = receiver_element_node(this, "checkVisibility", context)?;
            let (mut check_opacity, mut check_vis_css) = (false, false);
            if let Some(opts) = args.first().and_then(JsValue::as_object) {
                let op = opts.get(js_string!("checkOpacity"), context)?;
                let op_alt = opts.get(js_string!("opacityProperty"), context)?;
                let vc = opts.get(js_string!("checkVisibilityCSS"), context)?;
                let vc_alt = opts.get(js_string!("visibilityProperty"), context)?;
                modern::check_context(context)?;
                check_opacity = op.to_boolean() || op_alt.to_boolean();
                check_vis_css = vc.to_boolean() || vc_alt.to_boolean();
            }
            let visible = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if !is_connected_node(&data.host.document.nodes, id) {
                    false
                } else {
                    geometry_style::ensure_layout_cached(&mut data);
                    let cached = data.cached_layout.as_ref();
                    let mut cur = id;
                    let mut ok = true;
                    for _ in 0..=crate::document::MAX_DEPTH {
                        if cur == 0 || cur >= data.host.document.nodes.len() {
                            break;
                        }
                        let n = &data.host.document.nodes[cur];
                        if n.tag.starts_with('#') {
                            break;
                        }
                        if n.has("hidden") {
                            ok = false;
                            break;
                        }
                        if let Some(st) = cached.and_then(|c| c.styles.get(cur)) {
                            if st.display == crate::style::Display::None {
                                ok = false;
                                break;
                            }
                            if check_vis_css && !st.visible {
                                ok = false;
                                break;
                            }
                            if check_opacity && st.opacity <= 0.0 {
                                ok = false;
                                break;
                            }
                        }
                        let p = n.parent;
                        if p == cur {
                            break;
                        }
                        cur = p;
                    }
                    ok
                }
            };
            Ok(visible.into())
        }
        REQUEST_FULLSCREEN => {
            let _id = receiver_element_node(this, "requestFullscreen", context)?;
            let err = dom_exception::exception(
                "NotAllowedError",
                "Fullscreen request denied without active user gesture",
                context,
            )?;
            Ok(JsPromise::reject(err, context)?.into())
        }
        EL_SCROLL | EL_SCROLL_TO | EL_SCROLL_BY => {
            let id = receiver_element_node(this, "scroll", context)?;
            let is_by = name == EL_SCROLL_BY;
            let (dx, dy) = if let Some(opts) = args.first().and_then(JsValue::as_object) {
                let left_v = opts.get(js_string!("left"), context)?;
                let top_v = opts.get(js_string!("top"), context)?;
                let lx = if left_v.is_undefined() {
                    None
                } else {
                    Some(left_v.to_number(context)?)
                };
                let ty = if top_v.is_undefined() {
                    None
                } else {
                    Some(top_v.to_number(context)?)
                };
                modern::check_context(context)?;
                (lx, ty)
            } else {
                let lx = match args.first() {
                    Some(v) => Some(v.to_number(context)?),
                    None => Some(0.0),
                };
                let ty = match args.get(1) {
                    Some(v) => Some(v.to_number(context)?),
                    None => Some(0.0),
                };
                modern::check_context(context)?;
                (lx, ty)
            };
            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let resolve_axis = |cur: f32, d: Option<f64>| -> f32 {
                    match d {
                        Some(v) if v.is_finite() => {
                            (if is_by { cur as f64 + v } else { v }).max(0.0) as f32
                        }
                        _ => cur,
                    }
                };
                if data.host.document.nodes[id].tag == "html" {
                    let nx = resolve_axis(data.viewport.scroll_x, dx);
                    let ny = resolve_axis(data.viewport.scroll_y, dy);
                    data.viewport.scroll_x = nx;
                    data.viewport.scroll_y = ny;
                    data.scroll_request =
                        Some(crate::page_session::platform::ScrollRequest::Absolute {
                            x: nx,
                            y: ny,
                        });
                } else {
                    let (cur_x, cur_y) = data
                        .element_scroll_offsets
                        .get(&id)
                        .copied()
                        .unwrap_or((0.0, 0.0));
                    data.element_scroll_offsets
                        .insert(id, (resolve_axis(cur_x, dx), resolve_axis(cur_y, dy)));
                }
            }
            Ok(JsValue::undefined())
        }
        EL_SCROLL_INTO_VIEW_IF_NEEDED => {
            let _id = receiver_element_node(this, "scrollIntoViewIfNeeded", context)?;
            geometry_style::call(geometry_style::EL_SCROLL_INTO_VIEW, this, &[], context)
        }
        IMG_DECODE => {
            let id = receiver_element_node(this, "decode", context)?;
            let is_img = state(context).borrow().host.document.nodes[id].tag == "img";
            if !is_img {
                return Err(failure(
                    "HTMLImageElement.decode requires an <img> receiver",
                ));
            }
            Ok(JsPromise::resolve(JsValue::undefined(), context)?.into())
        }
        _ => Err(failure("Unknown DOM operation")),
    }
}

fn receiver_element_node(this: &JsValue, op: &str, context: &mut Context) -> JsResult<usize> {
    let id = receiver_node(this, context)?;
    let is_el = {
        let shared = state(context);
        let data = shared.borrow();
        id != 0 && !data.host.document.nodes[id].tag.starts_with('#')
    };
    if !is_el {
        return Err(failure(format!("{op} requires an Element receiver")));
    }
    Ok(id)
}

fn opt_nullable_str(val: &JsValue, context: &mut Context) -> JsResult<Option<String>> {
    if val.is_null() || val.is_undefined() {
        return Ok(None);
    }
    let s = utf8(val, context)?;
    modern::check_context(context)?;
    if s.is_empty() { Ok(None) } else { Ok(Some(s)) }
}

fn default_namespace_for_node(nodes: &[Node], mut id: usize) -> Option<&'static str> {
    if id == 0 || nodes[id].tag == "#document-subroot" {
        let doc_el = nodes[id]
            .children
            .iter()
            .copied()
            .find(|&c| !nodes[c].tag.starts_with('#'))?;
        id = doc_el;
    }
    for _ in 0..=crate::document::MAX_DEPTH {
        if id == 0 || id >= nodes.len() {
            return None;
        }
        if !nodes[id].tag.starts_with('#') {
            return Some(if is_svg_element(nodes, id) {
                "http://www.w3.org/2000/svg"
            } else {
                "http://www.w3.org/1999/xhtml"
            });
        }
        let p = nodes[id].parent;
        if p == id {
            return None;
        }
        id = p;
    }
    None
}

fn nodes_equal(nodes: &[Node], a: usize, b: usize) -> bool {
    let mut stack = vec![(a, b)];
    let mut steps = 0usize;
    while let Some((x, y)) = stack.pop() {
        steps += 1;
        if steps > crate::document::MAX_NODES {
            return false;
        }
        let (nx, ny) = (&nodes[x], &nodes[y]);
        let tx = if x == 0 {
            "#document-subroot"
        } else {
            nx.tag.as_str()
        };
        let ty = if y == 0 {
            "#document-subroot"
        } else {
            ny.tag.as_str()
        };
        if tx != ty {
            return false;
        }
        if matches!(tx, "#text" | "#comment") && nx.text != ny.text {
            return false;
        }
        if !tx.starts_with('#') {
            let ax: Vec<&(String, String)> = nx
                .attributes
                .iter()
                .filter(|(k, _)| k != crate::document::INTERNAL_CANVAS_ATTR)
                .collect();
            let ay: Vec<&(String, String)> = ny
                .attributes
                .iter()
                .filter(|(k, _)| k != crate::document::INTERNAL_CANVAS_ATTR)
                .collect();
            if ax.len() != ay.len() {
                return false;
            }
            for &(k, v) in &ax {
                if ny.attr(k) != Some(v.as_str()) {
                    return false;
                }
            }
        }
        if nx.children.len() != ny.children.len() {
            return false;
        }
        for (&cx, &cy) in nx.children.iter().zip(ny.children.iter()) {
            stack.push((cx, cy));
        }
    }
    true
}

fn normalize_subtree(data: &mut State, root: usize) -> bool {
    let mut postorder = Vec::new();
    let mut stack = vec![(root, false)];
    let mut visited = 0usize;
    while let Some((nid, expanded)) = stack.pop() {
        visited += 1;
        if visited > crate::document::MAX_NODES * 2 {
            break;
        }
        if expanded {
            postorder.push(nid);
        } else {
            stack.push((nid, true));
            for &ch in data.host.document.nodes[nid].children.iter().rev() {
                if !matches!(
                    data.host.document.nodes[ch].tag.as_str(),
                    "#text" | "#comment"
                ) {
                    stack.push((ch, false));
                }
            }
        }
    }
    let mut mutated = false;
    for parent in postorder {
        let old_kids = data.host.document.nodes[parent].children.clone();
        let mut new_kids: Vec<usize> = Vec::with_capacity(old_kids.len());
        for ch in old_kids {
            if data.host.document.nodes[ch].tag == "#text" {
                let txt = data.host.document.nodes[ch].text.clone();
                if txt.is_empty() {
                    data.host.document.nodes[ch].parent = ch;
                    mutated = true;
                    continue;
                }
                if let Some(&prev) = new_kids.last()
                    && data.host.document.nodes[prev].tag == "#text"
                {
                    data.host.document.nodes[prev].text.push_str(&txt);
                    data.host.document.nodes[ch].parent = ch;
                    mutated = true;
                    continue;
                }
            }
            new_kids.push(ch);
        }
        if new_kids != data.host.document.nodes[parent].children {
            data.host.document.nodes[parent].children = new_kids;
        }
    }
    if mutated {
        data.dom_epoch = data.dom_epoch.wrapping_add(1);
    }
    mutated
}

fn set_element_attr_observed(
    id: usize,
    attr: &str,
    val: String,
    context: &mut Context,
) -> JsResult<()> {
    let mutated = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if matches!(
            data.host.document.nodes[id].tag.as_str(),
            "iframe" | "frame"
        ) && matches!(attr, "src" | "srcdoc")
        {
            data.iframe_documents.remove(&id);
            clear_cached_iframe_window(&mut data, id);
        }
        let track = super::observers_wc::has_any_interest(&data);
        let old = if track {
            data.host.document.nodes[id].attr(attr).map(str::to_owned)
        } else {
            None
        };
        data.host.attr(id, attr, val.clone()).map_err(failure)?;
        super::dom_events::invalidate_inline_handler(&mut data, id, attr);
        if matches!(attr, "width" | "height") {
            super::canvas::on_canvas_dimension_mutated(&mut data, id).map_err(failure)?;
        }
        data.dom_epoch = data.dom_epoch.wrapping_add(1);
        if track {
            super::observers_wc::record_attribute_mutation(&mut data, id, attr, old, Some(val));
            true
        } else {
            false
        }
    };
    if mutated {
        super::observers_wc::after_dom_mutation(context)?;
    }
    Ok(())
}

fn remove_element_attr_observed(id: usize, attr: &str, context: &mut Context) -> JsResult<()> {
    let mutated = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let old = data.host.document.nodes[id].attr(attr).map(str::to_owned);
        if let Some(old_val) = old {
            if matches!(
                data.host.document.nodes[id].tag.as_str(),
                "iframe" | "frame"
            ) && matches!(attr, "src" | "srcdoc")
            {
                data.iframe_documents.remove(&id);
                clear_cached_iframe_window(&mut data, id);
            }
            data.host.document.nodes[id]
                .attributes
                .retain(|(k, _)| k != attr);
            super::dom_events::invalidate_inline_handler(&mut data, id, attr);
            if matches!(attr, "width" | "height") {
                super::canvas::on_canvas_dimension_mutated(&mut data, id).map_err(failure)?;
            }
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            if super::observers_wc::has_any_interest(&data) {
                super::observers_wc::record_attribute_mutation(
                    &mut data,
                    id,
                    attr,
                    Some(old_val),
                    None,
                );
                true
            } else {
                false
            }
        } else {
            false
        }
    };
    if mutated {
        super::observers_wc::after_dom_mutation(context)?;
    }
    Ok(())
}

fn build_attr_obj(
    owner_el: JsValue,
    aname: &str,
    aval: &str,
    writable_val: bool,
    context: &mut Context,
) -> JsObject {
    let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    let vc = if writable_val {
        ec | Attribute::WRITABLE
    } else {
        ec
    };
    modern::boa_engine::object::ObjectInitializer::new(context)
        .property(js_string!("name"), JsString::from(aname), ec)
        .property(js_string!("localName"), JsString::from(aname), ec)
        .property(js_string!("value"), JsString::from(aval), vc)
        .property(js_string!("nodeValue"), JsString::from(aval), vc)
        .property(js_string!("nodeType"), 2, ec)
        .property(js_string!("specified"), true, ec)
        .property(js_string!("ownerElement"), owner_el, ec)
        .build()
}

fn build_single_attr_object(
    owner_id: usize,
    aname: &str,
    aval: &str,
    context: &mut Context,
) -> JsResult<JsValue> {
    state(context)
        .borrow_mut()
        .host
        .charge(128)
        .map_err(failure)?;
    let owner_el = wrap(&format!("node:{owner_id}"), context)?;
    Ok(build_attr_obj(owner_el, aname, aval, true, context).into())
}

fn build_part_token_list(id: usize, context: &mut Context) -> JsResult<JsValue> {
    let raw = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        data.host.charge(128).map_err(failure)?;
        data.host.document.nodes[id]
            .attr("part")
            .unwrap_or("")
            .to_owned()
    };
    let mut tokens: Vec<String> = Vec::new();
    for tok in raw.split_ascii_whitespace() {
        if !tokens.iter().any(|t| t == tok) {
            tokens.push(tok.to_owned());
        }
    }
    let obj = JsObject::with_object_proto(context.intrinsics());
    for (idx, tok) in tokens.iter().enumerate() {
        def_prop(
            &obj,
            idx as u32,
            JsString::from(tok.as_str()),
            false,
            true,
            true,
            context,
        )?;
    }
    def_prop(
        &obj,
        js_string!("length"),
        tokens.len() as f64,
        false,
        true,
        true,
        context,
    )?;
    def_prop(
        &obj,
        js_string!("value"),
        JsString::from(raw.as_str()),
        true,
        true,
        true,
        context,
    )?;
    let read_tokens = |nid: usize, ctx: &Context| -> Vec<String> {
        state(ctx)
            .borrow()
            .host
            .document
            .nodes
            .get(nid)
            .and_then(|n| n.attr("part"))
            .unwrap_or("")
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect()
    };
    let check_tok = |tok: &str, ctx: &mut Context| -> JsResult<()> {
        if tok.is_empty() || tok.chars().any(|c| c.is_ascii_whitespace()) {
            return Err(dom_exception::exception(
                "InvalidCharacterError",
                "Invalid DOMTokenList token",
                ctx,
            )?);
        }
        Ok(())
    };
    for (mname, mlen) in [
        ("contains", 1usize),
        ("add", 0),
        ("remove", 0),
        ("toggle", 1),
        ("toString", 0),
    ] {
        def_cap(
            &obj,
            mname,
            mlen,
            true,
            (id, mname, read_tokens, check_tok),
            |_, args, &(nid, op, rt, ct), ctx| {
                modern::check_context(ctx)?;
                match op {
                    "contains" => {
                        let want = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
                        modern::check_context(ctx)?;
                        Ok(rt(nid, ctx).iter().any(|t| t == &want).into())
                    }
                    "add" | "remove" => {
                        let mut cur = rt(nid, ctx);
                        for a in args {
                            let tok = utf8(a, ctx)?;
                            modern::check_context(ctx)?;
                            ct(&tok, ctx)?;
                            if op == "add" {
                                if !cur.iter().any(|t| t == &tok) {
                                    cur.push(tok);
                                }
                            } else {
                                cur.retain(|t| t != &tok);
                            }
                        }
                        set_element_attr_observed(nid, "part", cur.join(" "), ctx)?;
                        Ok(JsValue::undefined())
                    }
                    "toggle" => {
                        let tok = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
                        let force = args
                            .get(1)
                            .filter(|v| !v.is_undefined())
                            .map(JsValue::to_boolean);
                        modern::check_context(ctx)?;
                        ct(&tok, ctx)?;
                        let mut cur = rt(nid, ctx);
                        let had = cur.iter().any(|t| t == &tok);
                        let next = force.unwrap_or(!had);
                        if next && !had {
                            cur.push(tok);
                        } else if !next {
                            cur.retain(|t| t != &tok);
                        }
                        set_element_attr_observed(nid, "part", cur.join(" "), ctx)?;
                        Ok(next.into())
                    }
                    _ => {
                        let val = state(ctx)
                            .borrow()
                            .host
                            .document
                            .nodes
                            .get(nid)
                            .and_then(|n| n.attr("part"))
                            .unwrap_or("")
                            .to_owned();
                        Ok(JsString::from(val.as_str()).into())
                    }
                }
            },
            context,
        )?;
    }
    Ok(obj.into())
}

#[derive(Clone, Copy)]
enum WindowReceiver {
    Top,
    Iframe(usize),
}

fn check_window_receiver(
    this: &JsValue,
    context: &mut Context,
) -> JsResult<(WindowReceiver, JsObject)> {
    if this.is_null() || this.is_undefined() {
        return Ok((WindowReceiver::Top, context.global_object()));
    }
    let Some(obj) = this.as_object() else {
        return Err(failure("Illegal invocation"));
    };
    let global = context.global_object();
    if obj == global {
        return Ok((WindowReceiver::Top, global));
    }
    let iframe_id = state(context)
        .borrow()
        .dom_prototypes
        .as_ref()
        .and_then(|dp| {
            dp.iframe_windows
                .iter()
                .find(|e| e.window == obj)
                .map(|e| e.iframe_id)
        });
    if let Some(id) = iframe_id {
        return Ok((WindowReceiver::Iframe(id), obj));
    }
    Err(failure("Illegal invocation"))
}

fn install_window_prototype_members(proto: &JsObject, context: &mut Context) -> Result<(), String> {
    for (cname, cval) in [("TEMPORARY", 0), ("PERSISTENT", 1)] {
        def_prop(
            proto,
            JsString::from(cname),
            cval,
            false,
            true,
            false,
            context,
        )
        .map_err(|_| "Window constant failed")?;
    }

    for prop in ["name", "status"] {
        def_get_set_cap(
            proto,
            prop,
            true,
            prop == "name",
            |this, _, &is_n, ctx| {
                modern::check_context(ctx)?;
                let (kind, _) = check_window_receiver(this, ctx)?;
                let s = match kind {
                    WindowReceiver::Top => {
                        let s = state(ctx);
                        let d = s.borrow();
                        if is_n {
                            d.window_name.clone()
                        } else {
                            d.window_status.clone()
                        }
                    }
                    WindowReceiver::Iframe(id) if is_n => state(ctx)
                        .borrow()
                        .host
                        .document
                        .nodes
                        .get(id)
                        .and_then(|n| n.attr("name"))
                        .unwrap_or("")
                        .to_owned(),
                    WindowReceiver::Iframe(_) => String::new(),
                };
                Ok(JsString::from(s.as_str()).into())
            },
            |this, args, &is_n, ctx| {
                modern::check_context(ctx)?;
                let (kind, obj) = check_window_receiver(this, ctx)?;
                let v = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
                modern::check_context(ctx)?;
                match kind {
                    WindowReceiver::Top => {
                        let shared = state(ctx);
                        let mut data = shared.borrow_mut();
                        data.host.charge(v.len()).map_err(failure)?;
                        if is_n {
                            data.window_name = v;
                        } else {
                            data.window_status = v;
                        }
                    }
                    WindowReceiver::Iframe(_) => {
                        let pname = if is_n { "name" } else { "status" };
                        def_prop(
                            &obj,
                            JsString::from(pname),
                            JsString::from(v.as_str()),
                            true,
                            true,
                            true,
                            ctx,
                        )?;
                    }
                }
                Ok(JsValue::undefined())
            },
            context,
        )
        .map_err(|_| "Window string property failed")?;
    }

    for prop in ["closed", "frameElement", "offscreenBuffering"] {
        def_getter_cap(
            proto,
            prop,
            true,
            prop,
            |this, _, &pname, ctx| {
                modern::check_context(ctx)?;
                let (kind, _) = check_window_receiver(this, ctx)?;
                match pname {
                    "closed" => Ok(false.into()),
                    "offscreenBuffering" => Ok(true.into()),
                    "frameElement" => match kind {
                        WindowReceiver::Top => Ok(JsValue::null()),
                        WindowReceiver::Iframe(id) => {
                            if is_connected_node(&state(ctx).borrow().host.document.nodes, id) {
                                wrap(&format!("node:{id}"), ctx)
                            } else {
                                Ok(JsValue::null())
                            }
                        }
                    },
                    _ => Ok(JsValue::undefined()),
                }
            },
            context,
        )
        .map_err(|_| "Window readonly getter failed")?;
    }

    for prop in [
        "frames",
        "length",
        "origin",
        "screenX",
        "screenY",
        "screenLeft",
        "screenTop",
        "clientInformation",
        "event",
        "locationbar",
        "menubar",
        "personalbar",
        "scrollbars",
        "statusbar",
        "toolbar",
        "external",
    ] {
        def_get_set_cap(
            proto,
            prop,
            true,
            prop,
            |this, _, &pname, ctx| {
                modern::check_context(ctx)?;
                let (kind, obj) = check_window_receiver(this, ctx)?;
                match pname {
                    "frames" => Ok(obj.into()),
                    "length" => match kind {
                        WindowReceiver::Top => {
                            let s = state(ctx);
                            let d = s.borrow();
                            let count = (1..d.host.document.nodes.len())
                                .filter(|&nid| {
                                    matches!(
                                        d.host.document.nodes[nid].tag.as_str(),
                                        "iframe" | "frame"
                                    ) && is_connected_node(&d.host.document.nodes, nid)
                                })
                                .count();
                            Ok((count as f64).into())
                        }
                        WindowReceiver::Iframe(_) => Ok(0.into()),
                    },
                    "origin" => {
                        let o = state(ctx)
                            .borrow()
                            .host
                            .base_url()
                            .origin()
                            .ascii_serialization();
                        Ok(JsString::from(o.as_str()).into())
                    }
                    "screenX" | "screenY" | "screenLeft" | "screenTop" => Ok(0.into()),
                    "clientInformation" => wrap("navigator", ctx),
                    "event" => {
                        let active = state(ctx)
                            .borrow()
                            .modern_events
                            .iter()
                            .enumerate()
                            .rev()
                            .find(|(_, ev)| ev.dispatching)
                            .map(|(i, _)| i);
                        match active {
                            Some(i) => wrap(&format!("mevent:{i}"), ctx),
                            None => Ok(JsValue::undefined()),
                        }
                    }
                    "locationbar" | "menubar" | "personalbar" | "scrollbars" | "statusbar"
                    | "toolbar" => Ok(modern::boa_engine::object::ObjectInitializer::new(ctx)
                        .property(
                            js_string!("visible"),
                            true,
                            Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
                        )
                        .build()
                        .into()),
                    "external" => {
                        let ext = JsObject::with_object_proto(ctx.intrinsics());
                        def_fn(
                            &ext,
                            "AddSearchProvider",
                            0,
                            true,
                            |_, _, c| {
                                modern::check_context(c)?;
                                Ok(JsValue::undefined())
                            },
                            ctx,
                        )?;
                        def_fn(
                            &ext,
                            "IsSearchProviderInstalled",
                            0,
                            true,
                            |_, _, c| {
                                modern::check_context(c)?;
                                Ok(0.into())
                            },
                            ctx,
                        )?;
                        Ok(ext.into())
                    }
                    _ => Ok(JsValue::undefined()),
                }
            },
            |this, args, &pname, ctx| {
                modern::check_context(ctx)?;
                let (_, obj) = check_window_receiver(this, ctx)?;
                let val = args.first().cloned().unwrap_or_default();
                def_prop(&obj, JsString::from(pname), val, true, true, true, ctx)?;
                Ok(JsValue::undefined())
            },
            context,
        )
        .map_err(|_| "Window replaceable property failed")?;
    }

    for (mname, mlen) in [
        ("alert", 0usize),
        ("blur", 0),
        ("cancelIdleCallback", 1),
        ("captureEvents", 0),
        ("close", 0),
        ("confirm", 0),
        ("createImageBitmap", 1),
        ("find", 0),
        ("focus", 0),
        ("getSelection", 0),
        ("moveBy", 2),
        ("moveTo", 2),
        ("open", 0),
        ("print", 0),
        ("prompt", 0),
        ("releaseEvents", 0),
        ("reportError", 1),
        ("requestIdleCallback", 1),
        ("resizeBy", 2),
        ("resizeTo", 2),
        ("stop", 0),
    ] {
        def_cap(
            proto,
            mname,
            mlen,
            true,
            mname,
            |this, args, &op, ctx| {
                modern::check_context(ctx)?;
                let (_, win_obj) = check_window_receiver(this, ctx)?;
                match op {
                    "alert" | "blur" | "captureEvents" | "close" | "focus" | "moveBy"
                    | "moveTo" | "print" | "releaseEvents" | "resizeBy" | "resizeTo" | "stop" => {
                        Ok(JsValue::undefined())
                    }
                    "confirm" | "find" => Ok(false.into()),
                    "open" | "prompt" => Ok(JsValue::null()),
                    "reportError" => {
                        if args.is_empty() {
                            return Err(failure("Window.reportError requires 1 argument"));
                        }
                        Ok(JsValue::undefined())
                    }
                    "requestIdleCallback" => {
                        let cb = args
                            .first()
                            .filter(|v| v.is_callable())
                            .cloned()
                            .ok_or_else(|| {
                                failure("requestIdleCallback requires a callback function")
                            })?;
                        let set_timeout = ctx.global_object().get(js_string!("setTimeout"), ctx)?;
                        if let Some(st_fn) = set_timeout.as_callable() {
                            let deadline = modern::boa_engine::object::ObjectInitializer::new(ctx)
                                .property(
                                    js_string!("didTimeout"),
                                    false,
                                    Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
                                )
                                .function(
                                    NativeFunction::from_fn_ptr(|_, _, c| {
                                        modern::check_context(c)?;
                                        Ok(16.0.into())
                                    }),
                                    js_string!("timeRemaining"),
                                    0,
                                )
                                .build();
                            st_fn.call(&win_obj.into(), &[cb, 1.into(), deadline.into()], ctx)
                        } else {
                            Ok(0.into())
                        }
                    }
                    "cancelIdleCallback" => {
                        let id_val = args.first().cloned().unwrap_or_else(|| 0.into());
                        let clear_timeout =
                            ctx.global_object().get(js_string!("clearTimeout"), ctx)?;
                        if let Some(ct_fn) = clear_timeout.as_callable() {
                            ct_fn.call(&win_obj.into(), &[id_val], ctx)?;
                        }
                        Ok(JsValue::undefined())
                    }
                    "createImageBitmap" => {
                        if args.is_empty() {
                            return Err(failure("createImageBitmap requires 1 argument"));
                        }
                        let bmp = modern::boa_engine::object::ObjectInitializer::new(ctx)
                            .property(
                                js_string!("width"),
                                1,
                                Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
                            )
                            .property(
                                js_string!("height"),
                                1,
                                Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
                            )
                            .function(
                                NativeFunction::from_fn_ptr(|_, _, c| {
                                    modern::check_context(c)?;
                                    Ok(JsValue::undefined())
                                }),
                                js_string!("close"),
                                0,
                            )
                            .build();
                        Ok(JsPromise::resolve(bmp, ctx)?.into())
                    }
                    "getSelection" => build_empty_selection(ctx),
                    _ => Ok(JsValue::undefined()),
                }
            },
            context,
        )
        .map_err(|_| "Window method failed")?;
    }

    Ok(())
}

fn build_empty_selection(context: &mut Context) -> JsResult<JsValue> {
    state(context)
        .borrow_mut()
        .host
        .charge(128)
        .map_err(failure)?;
    let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    let sel = modern::boa_engine::object::ObjectInitializer::new(context)
        .property(js_string!("anchorNode"), JsValue::null(), ec)
        .property(js_string!("anchorOffset"), 0, ec)
        .property(js_string!("focusNode"), JsValue::null(), ec)
        .property(js_string!("focusOffset"), 0, ec)
        .property(js_string!("isCollapsed"), true, ec)
        .property(js_string!("rangeCount"), 0, ec)
        .property(js_string!("type"), js_string!("None"), ec)
        .property(js_string!("direction"), js_string!("none"), ec)
        .build();
    for (n, len) in [
        ("removeAllRanges", 0usize),
        ("empty", 0),
        ("collapse", 1),
        ("selectAllChildren", 1),
        ("addRange", 1),
        ("removeRange", 1),
    ] {
        def_fn(
            &sel,
            n,
            len,
            true,
            |_, _, c| {
                modern::check_context(c)?;
                Ok(JsValue::undefined())
            },
            context,
        )?;
    }
    def_fn(
        &sel,
        "containsNode",
        1,
        true,
        |_, _, c| {
            modern::check_context(c)?;
            Ok(false.into())
        },
        context,
    )?;
    def_fn(
        &sel,
        "getRangeAt",
        1,
        true,
        |_, _, c| {
            modern::check_context(c)?;
            Err(dom_exception::exception(
                "IndexSizeError",
                "Selection has no ranges",
                c,
            )?)
        },
        context,
    )?;
    def_fn(
        &sel,
        "toString",
        0,
        true,
        |_, _, c| {
            modern::check_context(c)?;
            Ok(js_string!("").into())
        },
        context,
    )?;
    Ok(sel.into())
}

fn is_element_content_editable(nodes: &[Node], mut id: usize) -> bool {
    for _ in 0..=crate::document::MAX_DEPTH {
        if id == 0 || id >= nodes.len() || nodes[id].tag.starts_with('#') {
            return false;
        }
        if let Some(raw) = nodes[id].attr("contenteditable") {
            match raw.trim().to_ascii_lowercase().as_str() {
                "" | "true" | "plaintext-only" => return true,
                "false" => return false,
                _ => {}
            }
        }
        let p = nodes[id].parent;
        if p == id {
            break;
        }
        id = p;
    }
    false
}

fn element_tab_index(nodes: &[Node], id: usize) -> i32 {
    if let Some(raw) = nodes[id].attr("tabindex")
        && let Ok(parsed) = raw.trim().parse::<i32>()
    {
        return parsed;
    }
    let n = &nodes[id];
    let default_focusable = (n.tag == "a" && n.has("href"))
        || matches!(n.tag.as_str(), "button" | "select" | "textarea" | "summary")
        || (n.tag == "input" && effective_input_type(n) != "hidden")
        || is_element_content_editable(nodes, id);
    if default_focusable { 0 } else { -1 }
}

fn build_named_node_map(id: usize, context: &mut Context) -> JsResult<JsValue> {
    let attrs: Vec<(String, String)> = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let list: Vec<(String, String)> = data.host.document.nodes[id]
            .attributes
            .iter()
            .filter(|(k, _)| k != crate::document::INTERNAL_CANVAS_ATTR)
            .cloned()
            .collect();
        data.host.charge(128 + list.len() * 64).map_err(failure)?;
        list
    };
    let map_obj = JsObject::with_object_proto(context.intrinsics());
    let owner_el = wrap(&format!("node:{id}"), context)?;
    for (idx, (aname, aval)) in attrs.iter().enumerate() {
        let aobj = build_attr_obj(owner_el.clone(), aname, aval, false, context);
        def_prop(
            &map_obj,
            idx as u32,
            aobj.clone(),
            false,
            true,
            true,
            context,
        )?;
        if !matches!(aname.as_str(), "length" | "item" | "getNamedItem")
            && aname.parse::<usize>().is_err()
        {
            let _ = def_prop(
                &map_obj,
                JsString::from(aname.as_str()),
                aobj,
                false,
                false,
                true,
                context,
            );
        }
    }
    def_prop(
        &map_obj,
        js_string!("length"),
        attrs.len() as f64,
        false,
        false,
        true,
        context,
    )?;
    def_fn(
        &map_obj,
        "item",
        1,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let obj = this
                .as_object()
                .ok_or_else(|| failure("NamedNodeMap.item requires receiver"))?;
            let idx = args.first().unwrap_or(&JsValue::undefined()).to_u32(ctx)?;
            let v = obj.get(idx, ctx)?;
            Ok(if v.is_undefined() { JsValue::null() } else { v })
        },
        context,
    )?;
    def_fn(
        &map_obj,
        "getNamedItem",
        1,
        false,
        |this, args, ctx| {
            modern::check_context(ctx)?;
            let obj = this
                .as_object()
                .ok_or_else(|| failure("NamedNodeMap.getNamedItem requires receiver"))?;
            let qname = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            let len = obj.get(js_string!("length"), ctx)?.to_u32(ctx)?;
            for i in 0..len {
                let item = obj.get(i, ctx)?;
                if let Some(io) = item.as_object()
                    && utf8(&io.get(js_string!("name"), ctx)?, ctx)? == qname
                {
                    return Ok(item);
                }
            }
            Ok(JsValue::null())
        },
        context,
    )?;
    let arr_iter = context
        .intrinsics()
        .constructors()
        .array()
        .prototype()
        .get(modern::boa_engine::JsSymbol::iterator(), context)?;
    def_prop(
        &map_obj,
        modern::boa_engine::JsSymbol::iterator(),
        arr_iter,
        true,
        false,
        true,
        context,
    )?;
    Ok(map_obj.into())
}

fn ancestor_chain_from_root(nodes: &[Node], mut id: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    for _ in 0..=crate::document::MAX_DEPTH {
        chain.push(id);
        let p = nodes[id].parent;
        if id == 0 || p == id {
            break;
        }
        id = p;
    }
    chain.reverse();
    chain
}

fn compare_document_position_impl(nodes: &[Node], a: usize, b: usize) -> i32 {
    if a == b {
        return 0;
    }
    let pa = ancestor_chain_from_root(nodes, a);
    let pb = ancestor_chain_from_root(nodes, b);
    if pa[0] != pb[0] {
        return 1 | 32 | if a < b { 4 } else { 2 };
    }
    let common_len = pa.len().min(pb.len());
    let mut k = 0;
    while k < common_len && pa[k] == pb[k] {
        k += 1;
    }
    if k == pa.len() {
        // `a` is an ancestor of `b`: `b` is CONTAINED_BY (16) and FOLLOWING (4) `a`
        return 16 | 4;
    }
    if k == pb.len() {
        // `b` is an ancestor of `a`: `b` CONTAINS (8) and PRECEDES (2) `a`
        return 8 | 2;
    }
    let parent = pa[k - 1];
    let sib_a = pa[k];
    let sib_b = pb[k];
    let idx_a = nodes[parent]
        .children
        .iter()
        .position(|&c| c == sib_a)
        .unwrap_or(0);
    let idx_b = nodes[parent]
        .children
        .iter()
        .position(|&c| c == sib_b)
        .unwrap_or(0);
    if idx_a < idx_b { 4 } else { 2 }
}

fn tree_walker_accept(
    id: usize,
    what_to_show: u32,
    filter_val: &JsValue,
    context: &mut Context,
) -> JsResult<u32> {
    let bit = {
        let shared = state(context);
        let data = shared.borrow();
        let tag = data.host.document.nodes[id].tag.as_str();
        if id == 0 || tag == "#document-subroot" {
            0x100u32
        } else if tag == "#document-fragment" {
            0x400
        } else if tag == "#text" {
            0x4
        } else {
            0x1
        }
    };
    if (what_to_show & bit) == 0 {
        return Ok(3); // FILTER_SKIP
    }
    let Some(fobj) = filter_val.as_object() else {
        return Ok(1); // FILTER_ACCEPT
    };
    let node_js = wrap_node_id(id, context)?;
    let res = if fobj.is_callable() {
        fobj.call(&JsValue::undefined(), &[node_js], context)?
    } else {
        let accept = fobj.get(js_string!("acceptNode"), context)?;
        let Some(afn) = accept.as_callable() else {
            return Err(failure("NodeFilter.acceptNode is not callable"));
        };
        afn.call(filter_val, &[node_js], context)?
    };
    Ok(res.to_u32(context)?)
}

fn preorder_next(nodes: &[Node], root: usize, cur: usize, skip_children: bool) -> Option<usize> {
    if !skip_children && let Some(&first) = nodes[cur].children.first() {
        return Some(first);
    }
    let mut node = cur;
    for _ in 0..=crate::document::MAX_DEPTH {
        if node == root {
            return None;
        }
        let p = nodes[node].parent;
        if p == node {
            return None;
        }
        let kids = &nodes[p].children;
        if let Some(pos) = kids.iter().position(|&c| c == node)
            && let Some(&next_sib) = kids.get(pos + 1)
        {
            return Some(next_sib);
        }
        node = p;
    }
    None
}

fn preorder_prev(nodes: &[Node], root: usize, cur: usize) -> Option<usize> {
    if cur == root {
        return None;
    }
    let p = nodes[cur].parent;
    if p == cur {
        return None;
    }
    let kids = &nodes[p].children;
    if let Some(pos) = kids.iter().position(|&c| c == cur)
        && pos > 0
    {
        let mut desc = kids[pos - 1];
        for _ in 0..=crate::document::MAX_DEPTH {
            if let Some(&last) = nodes[desc].children.last() {
                desc = last;
            } else {
                break;
            }
        }
        return Some(desc);
    }
    Some(p)
}

fn build_tree_walker(
    root_js: JsValue,
    what_to_show: u32,
    filter_val: JsValue,
    context: &mut Context,
) -> JsResult<JsValue> {
    state(context)
        .borrow_mut()
        .host
        .charge(256)
        .map_err(failure)?;
    let tw = JsObject::with_object_proto(context.intrinsics());
    def_prop(
        &tw,
        js_string!("root"),
        root_js.clone(),
        false,
        true,
        true,
        context,
    )?;
    def_prop(
        &tw,
        js_string!("whatToShow"),
        what_to_show as f64,
        false,
        true,
        true,
        context,
    )?;
    def_prop(
        &tw,
        js_string!("filter"),
        filter_val,
        false,
        true,
        true,
        context,
    )?;
    def_prop(
        &tw,
        js_string!("currentNode"),
        root_js,
        true,
        true,
        true,
        context,
    )?;

    for (mname, mode) in [
        ("nextNode", 0u8),
        ("previousNode", 1),
        ("parentNode", 2),
        ("firstChild", 3),
        ("lastChild", 4),
        ("nextSibling", 5),
        ("previousSibling", 6),
    ] {
        def_cap(
            &tw,
            mname,
            0,
            false,
            mode,
            |this, _, &mode, ctx| {
                modern::check_context(ctx)?;
                let obj = this
                    .as_object()
                    .ok_or_else(|| failure("TreeWalker method requires receiver"))?;
                let root_v = obj.get(js_string!("root"), ctx)?;
                let cur_v = obj.get(js_string!("currentNode"), ctx)?;
                let wts = obj.get(js_string!("whatToShow"), ctx)?.to_u32(ctx)?;
                let flt = obj.get(js_string!("filter"), ctx)?;
                let Value::Host(root_h) = receiver(&root_v, ctx)? else {
                    return Err(failure("Invalid TreeWalker.root"));
                };
                let Value::Host(cur_h) = receiver(&cur_v, ctx)? else {
                    return Err(failure("Invalid TreeWalker.currentNode"));
                };
                let (root_id, mut cur_id) = {
                    let shared = state(ctx);
                    let data = shared.borrow();
                    (
                        node_id_from_handle(&root_h, &data).map_err(failure)?,
                        node_id_from_handle(&cur_h, &data).map_err(failure)?,
                    )
                };
                match mode {
                    0 => {
                        let mut skip_children = false;
                        for _ in 0..=crate::document::MAX_NODES {
                            let next = {
                                let s = state(ctx);
                                let d = s.borrow();
                                preorder_next(
                                    &d.host.document.nodes,
                                    root_id,
                                    cur_id,
                                    skip_children,
                                )
                            };
                            let Some(nid) = next else {
                                return Ok(JsValue::null());
                            };
                            cur_id = nid;
                            let res = tree_walker_accept(cur_id, wts, &flt, ctx)?;
                            if res == 1 {
                                let wrapped = wrap_node_id(cur_id, ctx)?;
                                obj.set(js_string!("currentNode"), wrapped.clone(), true, ctx)?;
                                return Ok(wrapped);
                            }
                            skip_children = res == 2;
                        }
                        Ok(JsValue::null())
                    }
                    1 => {
                        for _ in 0..=crate::document::MAX_NODES {
                            let prev = {
                                let s = state(ctx);
                                let d = s.borrow();
                                preorder_prev(&d.host.document.nodes, root_id, cur_id)
                            };
                            let Some(pid) = prev else {
                                return Ok(JsValue::null());
                            };
                            cur_id = pid;
                            if cur_id == root_id {
                                return Ok(JsValue::null());
                            }
                            if tree_walker_accept(cur_id, wts, &flt, ctx)? == 1 {
                                let wrapped = wrap_node_id(cur_id, ctx)?;
                                obj.set(js_string!("currentNode"), wrapped.clone(), true, ctx)?;
                                return Ok(wrapped);
                            }
                        }
                        Ok(JsValue::null())
                    }
                    2 => {
                        while cur_id != root_id {
                            let p = {
                                let s = state(ctx);
                                let d = s.borrow();
                                let p = d.host.document.nodes[cur_id].parent;
                                if p == cur_id { None } else { Some(p) }
                            };
                            let Some(pid) = p else {
                                break;
                            };
                            cur_id = pid;
                            if cur_id == root_id {
                                break;
                            }
                            if tree_walker_accept(cur_id, wts, &flt, ctx)? == 1 {
                                let wrapped = wrap_node_id(cur_id, ctx)?;
                                obj.set(js_string!("currentNode"), wrapped.clone(), true, ctx)?;
                                return Ok(wrapped);
                            }
                        }
                        Ok(JsValue::null())
                    }
                    3 | 4 => {
                        let kids: Vec<usize> = {
                            let s = state(ctx);
                            let d = s.borrow();
                            let mut k = d.host.document.nodes[cur_id].children.clone();
                            if mode == 4 {
                                k.reverse();
                            }
                            k
                        };
                        for kid in kids {
                            if tree_walker_accept(kid, wts, &flt, ctx)? == 1 {
                                let wrapped = wrap_node_id(kid, ctx)?;
                                obj.set(js_string!("currentNode"), wrapped.clone(), true, ctx)?;
                                return Ok(wrapped);
                            }
                        }
                        Ok(JsValue::null())
                    }
                    5 | 6 => {
                        if cur_id == root_id {
                            return Ok(JsValue::null());
                        }
                        let sibs: Vec<usize> = {
                            let s = state(ctx);
                            let d = s.borrow();
                            let p = d.host.document.nodes[cur_id].parent;
                            if p == cur_id {
                                Vec::new()
                            } else {
                                let kids = &d.host.document.nodes[p].children;
                                match kids.iter().position(|&c| c == cur_id) {
                                    Some(pos) if mode == 5 => kids[pos + 1..].to_vec(),
                                    Some(pos) => {
                                        let mut v = kids[..pos].to_vec();
                                        v.reverse();
                                        v
                                    }
                                    None => Vec::new(),
                                }
                            }
                        };
                        for sib in sibs {
                            if tree_walker_accept(sib, wts, &flt, ctx)? == 1 {
                                let wrapped = wrap_node_id(sib, ctx)?;
                                obj.set(js_string!("currentNode"), wrapped.clone(), true, ctx)?;
                                return Ok(wrapped);
                            }
                        }
                        Ok(JsValue::null())
                    }
                    _ => Ok(JsValue::null()),
                }
            },
            context,
        )?;
    }
    Ok(tw.into())
}

fn ensure_html_skeleton(
    data: &mut State,
    sub_root: usize,
    top_nodes: Vec<usize>,
) -> Result<(), DomOpErr> {
    // If markup already had an <html> root element, attach top_nodes directly and ensure <head>/<body>
    if let Some(&html_id) = top_nodes
        .iter()
        .find(|&&id| data.host.document.nodes[id].tag == "html")
    {
        insert_node_before(data, sub_root, html_id, None)?;
        let has_head = data.host.document.nodes[html_id]
            .children
            .iter()
            .any(|&c| data.host.document.nodes[c].tag == "head");
        let has_body = data.host.document.nodes[html_id]
            .children
            .iter()
            .any(|&c| data.host.document.nodes[c].tag == "body");
        if !has_head {
            let head = data
                .host
                .new_node("head".into(), String::new())
                .map_err(DomOpErr::Host)?;
            let first = data.host.document.nodes[html_id].children.first().copied();
            insert_node_before(data, html_id, head, first)?;
        }
        if !has_body {
            let body = data
                .host
                .new_node("body".into(), String::new())
                .map_err(DomOpErr::Host)?;
            insert_node_before(data, html_id, body, None)?;
        }
        return Ok(());
    }
    let html_id = data
        .host
        .new_node("html".into(), String::new())
        .map_err(DomOpErr::Host)?;
    let head_id = data
        .host
        .new_node("head".into(), String::new())
        .map_err(DomOpErr::Host)?;
    let body_id = data
        .host
        .new_node("body".into(), String::new())
        .map_err(DomOpErr::Host)?;
    insert_node_before(data, sub_root, html_id, None)?;
    insert_node_before(data, html_id, head_id, None)?;
    insert_node_before(data, html_id, body_id, None)?;
    for id in top_nodes {
        let tag = data.host.document.nodes[id].tag.as_str();
        if tag == "head" {
            let kids = data.host.document.nodes[id].children.clone();
            for k in kids {
                insert_node_before(data, head_id, k, None)?;
            }
        } else if tag == "body" {
            let kids = data.host.document.nodes[id].children.clone();
            for k in kids {
                insert_node_before(data, body_id, k, None)?;
            }
        } else if matches!(tag, "title" | "meta" | "link" | "style" | "base") {
            insert_node_before(data, head_id, id, None)?;
        } else {
            insert_node_before(data, body_id, id, None)?;
        }
    }
    Ok(())
}

fn insert_adjacent_nodes(
    target: usize,
    pos: &str,
    nodes_to_insert: &[usize],
    context: &mut Context,
) -> JsResult<bool> {
    let res: Result<bool, DomOpErr> = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        (|| match pos {
            "beforebegin" => {
                let p = data.host.document.nodes[target].parent;
                if p == target {
                    return Ok(false);
                }
                for &n in nodes_to_insert {
                    insert_node_before(&mut data, p, n, Some(target))?;
                }
                Ok(true)
            }
            "afterbegin" => {
                let first = data.host.document.nodes[target].children.first().copied();
                for &n in nodes_to_insert {
                    insert_node_before(&mut data, target, n, first)?;
                }
                Ok(true)
            }
            "beforeend" => {
                for &n in nodes_to_insert {
                    insert_node_before(&mut data, target, n, None)?;
                }
                Ok(true)
            }
            "afterend" => {
                let p = data.host.document.nodes[target].parent;
                if p == target {
                    return Ok(false);
                }
                let next_sib = {
                    let kids = &data.host.document.nodes[p].children;
                    let idx = kids.iter().position(|&x| x == target).unwrap_or(0);
                    kids.get(idx + 1).copied()
                };
                for &n in nodes_to_insert {
                    insert_node_before(&mut data, p, n, next_sib)?;
                }
                Ok(true)
            }
            _ => Ok(false),
        })()
    };
    res.map_err(|e| into_js_err(e, context))
}

/// CSSOM `CSS.escape` implementation (https://drafts.csswg.org/cssom/#serialize-an-identifier).
pub(super) fn css_escape(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() == 1 && chars[0] == '-' {
        return "\\-".into();
    }
    let mut out = String::with_capacity(value.len() + 4);
    for (idx, &ch) in chars.iter().enumerate() {
        if ch == '\0' {
            out.push('\u{FFFD}');
            continue;
        }
        let code = ch as u32;
        if (0x0001..=0x001F).contains(&code)
            || code == 0x007F
            || (idx == 0 && ch.is_ascii_digit())
            || (idx == 1 && ch.is_ascii_digit() && chars[0] == '-')
        {
            out.push_str(&format!("\\{code:x} "));
        } else if code >= 0x0080 || ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('\\');
            out.push(ch);
        }
    }
    out
}
