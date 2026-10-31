//! Modern CSS3 selector APIs (`querySelector`, `querySelectorAll`, `matches`,
//! `webkitMatchesSelector`, `closest`) and static iterable `NodeList` bindings.
use super::*;
use modern::boa_engine::{JsSymbol, object::builtins::JsArray};

pub(super) const MATCHES: &str = "host.dom.matches";
pub(super) const WEBKIT_MATCHES: &str = "host.dom.webkitMatchesSelector";
pub(super) const CLOSEST: &str = "host.dom.closest";
pub(super) const NODELIST_ITEM: &str = "host.nodelist.item";

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        "host.dom.querySelector" => Some(("querySelector", 1)),
        "host.dom.querySelectorAll" => Some(("querySelectorAll", 1)),
        MATCHES => Some(("matches", 1)),
        WEBKIT_MATCHES => Some(("webkitMatchesSelector", 1)),
        CLOSEST => Some(("closest", 1)),
        NODELIST_ITEM => Some(("item", 1)),
        _ => None,
    }
}

fn query_root_id(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(this, context)? else {
        return Err(failure("Selector query requires a Node receiver"));
    };
    let shared = state(context);
    let data = shared.borrow();
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
        return Err(failure("Stale document handle"));
    }
    let id = data.host.node(&handle).map_err(failure)?;
    let tag = data.host.document.nodes[id].tag.as_str();
    if tag == "#text" {
        return Err(failure("Text nodes do not support selector queries"));
    }
    Ok(id)
}

fn element_receiver_id(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(this, context)? else {
        return Err(failure(
            "Element selector method requires an Element receiver",
        ));
    };
    let shared = state(context);
    let data = shared.borrow();
    let id = data.host.node(&handle).map_err(failure)?;
    if id == 0 || data.host.document.nodes[id].tag.starts_with('#') {
        return Err(failure("Receiver is not an Element"));
    }
    Ok(id)
}

fn selector_arg(args: &[JsValue], context: &mut Context) -> JsResult<String> {
    let raw = args
        .first()
        .ok_or_else(|| failure("1 argument required, but only 0 present"))?;
    let s = utf8(raw, context)?;
    modern::check_context(context)?;
    Ok(s)
}

fn syntax_error(selector: &str, context: &mut Context) -> JsResult<JsError> {
    dom_exception::exception(
        "SyntaxError",
        &format!("'{}' is not a valid selector", bounded(selector, 128)),
        context,
    )
}

pub(super) fn alloc_nodelist(ids: Vec<usize>, context: &mut Context) -> JsResult<JsValue> {
    {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if data.node_lists.len() >= 4096 {
            return Err(failure("NodeList limit exceeded"));
        }
        data.host
            .charge(
                ids.len()
                    .saturating_mul(std::mem::size_of::<usize>())
                    .saturating_add(32),
            )
            .map_err(failure)?;
        let idx = data.node_lists.len();
        data.node_lists.push(ids);
        drop(data);
        wrap(&format!("nodelist:{idx}"), context)
    }
}

pub(super) fn call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    match name {
        "host.dom.querySelector" => {
            let root = query_root_id(this, context)?;
            let sel = selector_arg(args, context)?;
            let found = {
                let shared = state(context);
                let data = shared.borrow();
                crate::style::query_selector_dom(&data.host.document.nodes, root, &sel, true)
            };
            match found {
                Ok(ids) => match ids.first().copied() {
                    Some(id) => wrap(&format!("node:{id}"), context),
                    None => Ok(JsValue::null()),
                },
                Err(()) => Err(syntax_error(&sel, context)?),
            }
        }
        "host.dom.querySelectorAll" => {
            let root = query_root_id(this, context)?;
            let sel = selector_arg(args, context)?;
            let found = {
                let shared = state(context);
                let data = shared.borrow();
                crate::style::query_selector_dom(&data.host.document.nodes, root, &sel, false)
            };
            match found {
                Ok(ids) => alloc_nodelist(ids, context),
                Err(()) => Err(syntax_error(&sel, context)?),
            }
        }
        MATCHES | WEBKIT_MATCHES => {
            let el = element_receiver_id(this, context)?;
            let sel = selector_arg(args, context)?;
            let matched = {
                let shared = state(context);
                let data = shared.borrow();
                crate::style::matches_selector_dom(&data.host.document.nodes, el, &sel)
            };
            match matched {
                Ok(ok) => Ok(ok.into()),
                Err(()) => Err(syntax_error(&sel, context)?),
            }
        }
        CLOSEST => {
            let el = element_receiver_id(this, context)?;
            let sel = selector_arg(args, context)?;
            let found = {
                let shared = state(context);
                let data = shared.borrow();
                crate::style::closest_selector_dom(&data.host.document.nodes, el, &sel)
            };
            match found {
                Ok(Some(id)) => wrap(&format!("node:{id}"), context),
                Ok(None) => Ok(JsValue::null()),
                Err(()) => Err(syntax_error(&sel, context)?),
            }
        }
        NODELIST_ITEM => {
            let Value::Host(handle) = receiver(this, context)? else {
                return Err(failure("NodeList.item requires a NodeList receiver"));
            };
            let list_id = handle
                .strip_prefix("nodelist:")
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or_else(|| failure("Invalid NodeList receiver"))?;
            let idx = args
                .first()
                .unwrap_or(&JsValue::undefined())
                .to_u32(context)? as usize;
            modern::check_context(context)?;
            let node_id = {
                let shared = state(context);
                let data = shared.borrow();
                data.node_lists
                    .get(list_id)
                    .ok_or_else(|| failure("Stale NodeList handle"))?
                    .get(idx)
                    .copied()
            };
            match node_id {
                Some(id) => wrap(&format!("node:{id}"), context),
                None => Ok(JsValue::null()),
            }
        }
        _ => Err(failure("Unknown selector method")),
    }
}

pub(super) fn nodelist_get(
    handle: &str,
    property: &PropertyKey,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let list_id = handle
        .strip_prefix("nodelist:")
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| failure("Invalid NodeList handle"))?;
    if *property == PropertyKey::Symbol(JsSymbol::iterator()) {
        return context
            .intrinsics()
            .constructors()
            .array()
            .prototype()
            .get(JsSymbol::iterator(), context);
    }
    if *property == PropertyKey::Symbol(JsSymbol::to_string_tag()) {
        return Ok(js_string!("NodeList").into());
    }
    let Some(name) = property_name(property) else {
        return fallback_get(args, context);
    };
    match name.as_str() {
        "length" => {
            let len = state(context)
                .borrow()
                .node_lists
                .get(list_id)
                .ok_or_else(|| failure("Stale NodeList handle"))?
                .len();
            Ok((len as f64).into())
        }
        "item" => method(NODELIST_ITEM, context).map(Into::into),
        "forEach" | "entries" | "keys" | "values" => context
            .intrinsics()
            .constructors()
            .array()
            .prototype()
            .get(JsString::from(name.as_str()), context),
        _ => {
            if let Ok(idx) = name.parse::<usize>() {
                let node_id = state(context)
                    .borrow()
                    .node_lists
                    .get(list_id)
                    .ok_or_else(|| failure("Stale NodeList handle"))?
                    .get(idx)
                    .copied();
                if let Some(id) = node_id {
                    return wrap(&format!("node:{id}"), context);
                }
                return Ok(JsValue::undefined());
            }
            fallback_get(args, context)
        }
    }
}

pub(super) fn nodelist_has(
    handle: &str,
    property: &PropertyKey,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let list_id = handle
        .strip_prefix("nodelist:")
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| failure("Invalid NodeList handle"))?;
    if *property == PropertyKey::Symbol(JsSymbol::iterator())
        || *property == PropertyKey::Symbol(JsSymbol::to_string_tag())
    {
        return Ok(true.into());
    }
    if let Some(name) = property_name(property) {
        if matches!(
            name.as_str(),
            "length" | "item" | "forEach" | "entries" | "keys" | "values"
        ) {
            return Ok(true.into());
        }
        if let Ok(idx) = name.parse::<usize>() {
            let len = state(context)
                .borrow()
                .node_lists
                .get(list_id)
                .ok_or_else(|| failure("Stale NodeList handle"))?
                .len();
            return Ok((idx < len).into());
        }
    }
    args[0]
        .as_object()
        .unwrap()
        .has_property(property.clone(), context)
        .map(Into::into)
}

pub(super) fn nodelist_own_keys(handle: &str, context: &mut Context) -> JsResult<JsValue> {
    let list_id = handle
        .strip_prefix("nodelist:")
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| failure("Invalid NodeList handle"))?;
    let len = state(context)
        .borrow()
        .node_lists
        .get(list_id)
        .ok_or_else(|| failure("Stale NodeList handle"))?
        .len();
    let arr = JsArray::new(context)?;
    for i in 0..len {
        arr.push(JsString::from(i.to_string()), context)?;
    }
    Ok(arr.into())
}

pub(super) fn nodelist_own_descriptor(
    handle: &str,
    property: &PropertyKey,
    context: &mut Context,
) -> JsResult<JsValue> {
    let list_id = handle
        .strip_prefix("nodelist:")
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| failure("Invalid NodeList handle"))?;
    if let Some(name) = property_name(property)
        && let Ok(idx) = name.parse::<usize>()
    {
        let node_id = state(context)
            .borrow()
            .node_lists
            .get(list_id)
            .ok_or_else(|| failure("Stale NodeList handle"))?
            .get(idx)
            .copied();
        if let Some(id) = node_id {
            let val = wrap(&format!("node:{id}"), context)?;
            let desc = JsObject::with_object_proto(context.intrinsics());
            desc.create_data_property_or_throw(js_string!("value"), val, context)?;
            desc.create_data_property_or_throw(js_string!("writable"), false, context)?;
            desc.create_data_property_or_throw(js_string!("enumerable"), true, context)?;
            desc.create_data_property_or_throw(js_string!("configurable"), true, context)?;
            return Ok(desc.into());
        }
    }
    Ok(JsValue::undefined())
}
