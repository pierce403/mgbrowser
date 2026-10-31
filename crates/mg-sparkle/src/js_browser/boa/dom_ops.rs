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

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        "host.dom.appendChild" => Some(("appendChild", 1)),
        "host.dom.removeChild" => Some(("removeChild", 1)),
        "host.dom.getElementById" => Some(("getElementById", 1)),
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
        _ => None,
    }
}

pub(super) fn install_globals(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();

    // 1. Install `DOMParser` constructor
    let parse_fn = method(DOM_PARSER_PARSE, context).map_err(|_| "DOMParser method failed")?;
    let parser_proto = JsObject::with_object_proto(context.intrinsics());
    parser_proto
        .define_property_or_throw(
            js_string!("parseFromString"),
            PropertyDescriptor::builder()
                .value(parse_fn)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .map_err(|_| "DOMParser prototype failed")?;
    let parser_ctor = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure(|new_target, _, context| {
            modern::check_context(context)?;
            if new_target.is_undefined() {
                return Err(failure("DOMParser constructor requires 'new'"));
            }
            let proto = new_target
                .as_object()
                .and_then(|o| o.get(js_string!("prototype"), context).ok())
                .and_then(|v| v.as_object());
            let proto =
                proto.unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
            Ok(JsObject::from_proto_and_data(proto, ()).into())
        }),
    )
    .name(js_string!("DOMParser"))
    .length(0)
    .constructor(true)
    .build();
    parser_ctor
        .define_property_or_throw(
            js_string!("prototype"),
            PropertyDescriptor::builder()
                .value(parser_proto.clone())
                .writable(false)
                .enumerable(false)
                .configurable(false),
            context,
        )
        .map_err(|_| "DOMParser.prototype failed")?;
    parser_proto
        .define_property_or_throw(
            js_string!("constructor"),
            PropertyDescriptor::builder()
                .value(parser_ctor.clone())
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .map_err(|_| "DOMParser.prototype.constructor failed")?;
    global
        .define_property_or_throw(
            js_string!("DOMParser"),
            PropertyDescriptor::builder()
                .value(parser_ctor)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .map_err(|_| "Global DOMParser failed")?;

    // 2. Install `window.CSS = { supports, escape }`
    let css_obj = JsObject::with_object_proto(context.intrinsics());
    let supports_fn = method(CSS_SUPPORTS, context).map_err(|_| "CSS.supports failed")?;
    let escape_fn = method(CSS_ESCAPE, context).map_err(|_| "CSS.escape failed")?;
    css_obj
        .define_property_or_throw(
            js_string!("supports"),
            PropertyDescriptor::builder()
                .value(supports_fn)
                .writable(true)
                .enumerable(true)
                .configurable(true),
            context,
        )
        .map_err(|_| "CSS.supports define failed")?;
    css_obj
        .define_property_or_throw(
            js_string!("escape"),
            PropertyDescriptor::builder()
                .value(escape_fn)
                .writable(true)
                .enumerable(true)
                .configurable(true),
            context,
        )
        .map_err(|_| "CSS.escape define failed")?;
    global
        .define_property_or_throw(
            js_string!("CSS"),
            PropertyDescriptor::builder()
                .value(css_obj)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .map_err(|_| "Global CSS failed")?;

    // 3. Install WindowProperties named-id fallback Proxy in `window`'s prototype chain:
    // global -> named_props_proxy -> original_proto (Object.prototype)
    let orig_proto = global.prototype();
    let target = JsObject::from_proto_and_data(orig_proto, ());
    let named_proxy: JsObject = JsProxy::builder(target)
        .get(window_named_get)
        .has(window_named_has)
        .build(context)
        .map_err(|_| "WindowProperties proxy failed")?
        .into();
    global.set_prototype(Some(named_proxy));

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
    if data.host.document.nodes[new_child].tag == "#document-fragment" {
        data.host.document.nodes[new_child].children.clear();
    }
    for &ch in &inserting {
        let old_p = data.host.document.nodes[ch].parent;
        if old_p != ch {
            data.host.document.nodes[old_p]
                .children
                .retain(|&x| x != ch);
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
    data.host.document.nodes[parent]
        .children
        .splice(pos..pos, inserting);
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
        return subdoc_get(sub_id, name, context).map(Some);
    }

    if handle == "document" {
        match name {
            "createDocumentFragment" => {
                return method(CREATE_DOCUMENT_FRAGMENT, context).map(|o| Some(o.into()));
            }
            "ownerDocument" | "parentElement" | "parentNode" => return Ok(Some(JsValue::null())),
            "isConnected" => return Ok(Some(true.into())),
            "nodeName" => return Ok(Some(js_string!("#document").into())),
            "hasChildNodes" => return method(HAS_CHILD_NODES, context).map(|o| Some(o.into())),
            "contains" => return method(CONTAINS, context).map(|o| Some(o.into())),
            "cloneNode" => return method(CLONE_NODE, context).map(|o| Some(o.into())),
            "insertBefore" => return method(INSERT_BEFORE, context).map(|o| Some(o.into())),
            "replaceChild" => return method(REPLACE_CHILD, context).map(|o| Some(o.into())),
            "prepend" => return method(PREPEND, context).map(|o| Some(o.into())),
            "append" => return method(APPEND, context).map(|o| Some(o.into())),
            "replaceChildren" => return method(REPLACE_CHILDREN, context).map(|o| Some(o.into())),
            "childElementCount" => {
                let count = state(context).borrow().host.document.nodes[0]
                    .children
                    .iter()
                    .filter(|&&c| {
                        !state(context).borrow().host.document.nodes[c]
                            .tag
                            .starts_with('#')
                    })
                    .count();
                return Ok(Some((count as f64).into()));
            }
            _ => {}
        }
    }

    if !handle.starts_with("node:") {
        return Ok(None);
    }
    let id = {
        let shared = state(context);
        let data = shared.borrow();
        data.host.node(handle).map_err(failure)?
    };
    let tag = state(context).borrow().host.document.nodes[id].tag.clone();

    // DocumentFragment node
    if tag == "#document-fragment" {
        match name {
            "nodeType" => return Ok(Some(11.into())),
            "nodeName" => return Ok(Some(js_string!("#document-fragment").into())),
            "tagName" => return Ok(Some(JsValue::undefined())),
            "ownerDocument" => {
                let h = owner_document_handle(&state(context).borrow().host.document.nodes, id)
                    .unwrap_or_else(|| "document".into());
                return wrap(&h, context).map(Some);
            }
            "parentNode" | "parentElement" => return Ok(Some(JsValue::null())),
            "isConnected" => return Ok(Some(false.into())),
            "childElementCount" => {
                let shared = state(context);
                let data = shared.borrow();
                let count = data.host.document.nodes[id]
                    .children
                    .iter()
                    .filter(|&&c| !data.host.document.nodes[c].tag.starts_with('#'))
                    .count();
                return Ok(Some((count as f64).into()));
            }
            "getElementById" => {
                return method("host.dom.getElementById", context).map(|o| Some(o.into()));
            }
            "hasChildNodes" => return method(HAS_CHILD_NODES, context).map(|o| Some(o.into())),
            "contains" => return method(CONTAINS, context).map(|o| Some(o.into())),
            "cloneNode" => return method(CLONE_NODE, context).map(|o| Some(o.into())),
            "insertBefore" => return method(INSERT_BEFORE, context).map(|o| Some(o.into())),
            "replaceChild" => return method(REPLACE_CHILD, context).map(|o| Some(o.into())),
            "prepend" => return method(PREPEND, context).map(|o| Some(o.into())),
            "append" => return method(APPEND, context).map(|o| Some(o.into())),
            "replaceChildren" => return method(REPLACE_CHILDREN, context).map(|o| Some(o.into())),
            "setAttribute" | "getAttribute" | "removeAttribute" | "classList" | "style"
            | "dataset" => return Ok(Some(JsValue::undefined())),
            _ => return Ok(None),
        }
    }

    // Text node specifics
    if tag == "#text" {
        match name {
            "nodeName" => return Ok(Some(js_string!("#text").into())),
            "tagName" => return Ok(Some(JsValue::undefined())),
            "data" | "nodeValue" => {
                let t = state(context).borrow().host.document.nodes[id].text.clone();
                return Ok(Some(JsString::from(t.as_str()).into()));
            }
            "length" => {
                let len = state(context).borrow().host.document.nodes[id]
                    .text
                    .encode_utf16()
                    .count();
                return Ok(Some((len as f64).into()));
            }
            "ownerDocument" => {
                let h = owner_document_handle(&state(context).borrow().host.document.nodes, id)
                    .unwrap_or_else(|| "document".into());
                return wrap(&h, context).map(Some);
            }
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
            "remove" => return method(REMOVE, context).map(|o| Some(o.into())),
            "before" => return method(BEFORE, context).map(|o| Some(o.into())),
            "after" => return method(AFTER, context).map(|o| Some(o.into())),
            "replaceWith" => return method(REPLACE_WITH, context).map(|o| Some(o.into())),
            "cloneNode" => return method(CLONE_NODE, context).map(|o| Some(o.into())),
            "contains" => return method(CONTAINS, context).map(|o| Some(o.into())),
            "hasChildNodes" => return method(HAS_CHILD_NODES, context).map(|o| Some(o.into())),
            _ => return Ok(None),
        }
    }

    // Element nodes (`tag != "#text"` and `id != 0`)
    match name {
        "ownerDocument" => {
            let h = owner_document_handle(&state(context).borrow().host.document.nodes, id)
                .unwrap_or_else(|| "document".into());
            return wrap(&h, context).map(Some);
        }
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
        "childElementCount" => {
            let shared = state(context);
            let data = shared.borrow();
            let count = data.host.document.nodes[id]
                .children
                .iter()
                .filter(|&&c| !data.host.document.nodes[c].tag.starts_with('#'))
                .count();
            return Ok(Some((count as f64).into()));
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
        // Selector & DOM manipulation methods on Elements
        "matches" => return method(selectors::MATCHES, context).map(|o| Some(o.into())),
        "webkitMatchesSelector" => {
            return method(selectors::WEBKIT_MATCHES, context).map(|o| Some(o.into()));
        }
        "closest" => return method(selectors::CLOSEST, context).map(|o| Some(o.into())),
        "cloneNode" => return method(CLONE_NODE, context).map(|o| Some(o.into())),
        "insertBefore" => return method(INSERT_BEFORE, context).map(|o| Some(o.into())),
        "replaceChild" => return method(REPLACE_CHILD, context).map(|o| Some(o.into())),
        "remove" => return method(REMOVE, context).map(|o| Some(o.into())),
        "before" => return method(BEFORE, context).map(|o| Some(o.into())),
        "after" => return method(AFTER, context).map(|o| Some(o.into())),
        "replaceWith" => return method(REPLACE_WITH, context).map(|o| Some(o.into())),
        "prepend" => return method(PREPEND, context).map(|o| Some(o.into())),
        "append" => return method(APPEND, context).map(|o| Some(o.into())),
        "replaceChildren" => return method(REPLACE_CHILDREN, context).map(|o| Some(o.into())),
        "contains" => return method(CONTAINS, context).map(|o| Some(o.into())),
        "hasChildNodes" => return method(HAS_CHILD_NODES, context).map(|o| Some(o.into())),
        "hasAttribute" => return method(HAS_ATTRIBUTE, context).map(|o| Some(o.into())),
        "toggleAttribute" => return method(TOGGLE_ATTRIBUTE, context).map(|o| Some(o.into())),
        "getAttributeNames" => return method(GET_ATTRIBUTE_NAMES, context).map(|o| Some(o.into())),
        "insertAdjacentHTML" => {
            return method(INSERT_ADJACENT_HTML, context).map(|o| Some(o.into()));
        }
        "insertAdjacentElement" => {
            return method(INSERT_ADJACENT_ELEMENT, context).map(|o| Some(o.into()));
        }
        "insertAdjacentText" => {
            return method(INSERT_ADJACENT_TEXT, context).map(|o| Some(o.into()));
        }
        _ => Ok(None),
    }
}

pub(super) fn try_set(
    handle: &str,
    name: &str,
    value: &JsValue,
    context: &mut Context,
) -> JsResult<Option<()>> {
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

    if tag == "#text" && matches!(name, "data" | "nodeValue") {
        let text = if value.is_null() && name == "nodeValue" {
            String::new()
        } else {
            utf8(value, context)?
        };
        modern::check_context(context)?;
        let shared = state(context);
        let mut data = shared.borrow_mut();
        data.host.replace_text(id, text).map_err(failure)?;
        data.dom_epoch = data.dom_epoch.wrapping_add(1);
        return Ok(Some(()));
    }

    if tag == "#document-fragment" && matches!(name, "textContent") {
        let text = if value.is_null() {
            String::new()
        } else {
            utf8(value, context)?
        };
        modern::check_context(context)?;
        let shared = state(context);
        let mut data = shared.borrow_mut();
        for old in std::mem::take(&mut data.host.document.nodes[id].children) {
            data.host.document.nodes[old].parent = old;
        }
        if !text.is_empty() {
            let tid = data.host.new_node("#text".into(), text).map_err(failure)?;
            data.host.document.nodes[tid].parent = id;
            data.host.document.nodes[id].children.push(tid);
        }
        data.dom_epoch = data.dom_epoch.wrapping_add(1);
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
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if tag == "template" {
                let res: Result<(), DomOpErr> = (|| {
                    let frag_id = ensure_template_content(&mut data, id).map_err(DomOpErr::Host)?;
                    for old in std::mem::take(&mut data.host.document.nodes[frag_id].children) {
                        data.host.document.nodes[old].parent = old;
                    }
                    let children = parse_html_into_fragment_children(&mut data, &html)
                        .map_err(DomOpErr::Host)?;
                    for ch in children {
                        insert_node_before(&mut data, frag_id, ch, None)?;
                    }
                    Ok(())
                })();
                drop(data);
                res.map_err(|e| into_js_err(e, context))?;
                return Ok(Some(()));
            }
            let prev_len = data.host.document.nodes.len();
            data.host.replace_html(id, html).map_err(failure)?;
            lift_template_contents(&mut data, prev_len).map_err(failure)?;
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            return Ok(Some(()));
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
                        data.host.document.nodes[parent]
                            .children
                            .retain(|&x| x != id);
                        data.host.document.nodes[id].parent = id;
                        data.dom_epoch = data.dom_epoch.wrapping_add(1);
                        Ok(())
                    })()
                }
            };
            res.map_err(|e| into_js_err(e, context))?;
            return Ok(Some(()));
        }
        "open" if tag == "dialog" || tag == "details" => {
            let flag = value.to_boolean();
            let was_open = state(context).borrow().host.document.nodes[id].has("open");
            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if flag {
                    data.host.attr(id, "open", String::new()).map_err(failure)?;
                } else {
                    data.host.document.nodes[id]
                        .attributes
                        .retain(|(n, _)| n != "open");
                }
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
            }
            if tag == "details" && was_open != flag {
                dom_events::queue_or_fire_simple_event(id, "toggle", false, false, context)?;
            }
            return Ok(Some(()));
        }
        "returnValue" if tag == "dialog" => {
            let rv = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            data.host.charge(rv.len()).map_err(failure)?;
            data.dialog_return_values.insert(id, rv);
            return Ok(Some(()));
        }
        "checked" if tag == "input" => {
            let flag = value.to_boolean();
            let shared = state(context);
            let mut data = shared.borrow_mut();
            set_input_checked(&mut data, id, flag).map_err(failure)?;
            return Ok(Some(()));
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
            return Ok(Some(()));
        }
        "value" if tag == "select" => {
            let v = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            set_select_value(&mut data, id, &v).map_err(failure)?;
            return Ok(Some(()));
        }
        "selectedIndex" if tag == "select" => {
            let idx = value.to_i32(context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            set_select_index(&mut data, id, idx).map_err(failure)?;
            return Ok(Some(()));
        }
        "text" if tag == "option" => {
            let t = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            data.host.replace_text(id, t).map_err(failure)?;
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            return Ok(Some(()));
        }
        "htmlFor" if tag == "label" => {
            let f = utf8(value, context)?;
            modern::check_context(context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            data.host.attr(id, "for", f).map_err(failure)?;
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            return Ok(Some(()));
        }
        _ => Ok(None),
    }
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

fn subdoc_get(sub_id: usize, name: &str, context: &mut Context) -> JsResult<JsValue> {
    match name {
        "nodeType" => Ok(9.into()),
        "nodeName" => Ok(js_string!("#document").into()),
        "ownerDocument" | "parentNode" | "parentElement" => Ok(JsValue::null()),
        "isConnected" => Ok(false.into()),
        "documentElement" => {
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
                Some(id) => wrap(&format!("node:{id}"), context),
                None => Ok(JsValue::null()),
            }
        }
        "head" | "body" => {
            let found = {
                let shared = state(context);
                let data = shared.borrow();
                find_in_subtree(&data.host.document.nodes, sub_id, |n| n.tag == name)
            };
            match found {
                Some(id) => wrap(&format!("node:{id}"), context),
                None => Ok(JsValue::null()),
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
            Ok(JsString::from(t.as_str()).into())
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
            from_host(col, context)
        }
        "firstElementChild" => {
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
                Some(id) => wrap(&format!("node:{id}"), context),
                None => Ok(JsValue::null()),
            }
        }
        "querySelector" => method("host.dom.querySelector", context).map(Into::into),
        "querySelectorAll" => method("host.dom.querySelectorAll", context).map(Into::into),
        "getElementById" => method("host.dom.getElementById", context).map(Into::into),
        "getElementsByTagName" => method("host.dom.getElementsByTagName", context).map(Into::into),
        "getElementsByClassName" => {
            method("host.dom.getElementsByClassName", context).map(Into::into)
        }
        "createElement" => method("host.dom.createElement", context).map(Into::into),
        "createTextNode" => method("host.dom.createTextNode", context).map(Into::into),
        "createDocumentFragment" => method(CREATE_DOCUMENT_FRAGMENT, context).map(Into::into),
        "appendChild" => method("host.dom.appendChild", context).map(Into::into),
        "removeChild" => method("host.dom.removeChild", context).map(Into::into),
        "insertBefore" => method(INSERT_BEFORE, context).map(Into::into),
        "replaceChild" => method(REPLACE_CHILD, context).map(Into::into),
        "contains" => method(CONTAINS, context).map(Into::into),
        "hasChildNodes" => method(HAS_CHILD_NODES, context).map(Into::into),
        "cloneNode" => method(CLONE_NODE, context).map(Into::into),
        _ => Ok(JsValue::undefined()),
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
                data.host.document.nodes[parent]
                    .children
                    .retain(|&x| x != child);
                data.host.document.nodes[child].parent = child;
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
            }
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
        "host.dom.createElement" => {
            let doc = receiver_node(this, context)?;
            if doc != 0
                && state(context).borrow().host.document.nodes[doc].tag != "#document-subroot"
            {
                return Err(failure("createElement requires a Document receiver"));
            }
            let raw = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
            modern::check_context(context)?;
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
                        data.host.document.nodes[parent]
                            .children
                            .retain(|&x| x != old_child);
                        data.host.document.nodes[old_child].parent = old_child;
                        data.dom_epoch = data.dom_epoch.wrapping_add(1);
                    }
                    Ok(())
                }
            })();
            res.map_err(|e| into_js_err(e, context))?;
            wrap_node_id(old_child, context)
        }
        REMOVE => {
            let id = receiver_node(this, context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let p = data.host.document.nodes[id].parent;
            if id != 0 && p != id {
                data.host.document.nodes[p].children.retain(|&x| x != id);
                data.host.document.nodes[id].parent = id;
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
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
                        data.host.document.nodes[parent]
                            .children
                            .retain(|&x| x != id);
                        data.host.document.nodes[id].parent = id;
                        data.dom_epoch = data.dom_epoch.wrapping_add(1);
                    }
                    Ok(())
                }
            })();
            res.map_err(|e| into_js_err(e, context))?;
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
                    for old in std::mem::take(&mut data.host.document.nodes[parent].children) {
                        data.host.document.nodes[old].parent = old;
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
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if id == 0 || data.host.document.nodes[id].tag.starts_with('#') {
                return Err(failure("toggleAttribute requires an Element receiver"));
            }
            let present = data.host.document.nodes[id].has(&attr);
            let next = force.unwrap_or(!present);
            if next && !present {
                data.host.attr(id, &attr, String::new()).map_err(failure)?;
                dom_events::invalidate_inline_handler(&mut data, id, &attr);
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
            } else if !next && present {
                data.host.document.nodes[id]
                    .attributes
                    .retain(|(n, _)| n != &attr);
                dom_events::invalidate_inline_handler(&mut data, id, &attr);
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
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
                    Ok(JsValue::undefined())
                }
            }
        }
        DIALOG_SHOW | DIALOG_SHOW_MODAL => {
            let id = receiver_node(this, context)?;
            {
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
                data.host.attr(id, "open", String::new()).map_err(failure)?;
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
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
            let was_open = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if data.host.document.nodes[id].tag != "dialog" {
                    return Err(failure("Dialog method requires a HTMLDialogElement"));
                }
                let open = data.host.document.nodes[id].has("open");
                if open {
                    data.host.document.nodes[id]
                        .attributes
                        .retain(|(n, _)| n != "open");
                    if let Some(rv) = rv_arg {
                        data.host.charge(rv.len()).map_err(failure)?;
                        data.dialog_return_values.insert(id, rv);
                    }
                    data.dom_epoch = data.dom_epoch.wrapping_add(1);
                }
                open
            };
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
        _ => Err(failure("Unknown DOM operation")),
    }
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
