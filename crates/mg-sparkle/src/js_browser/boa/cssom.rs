//! Modern declared-style bindings. The DOM attribute is the only retained store.
//! Parsing is synchronous native Rust work within the existing worker profile.
use super::*;
use crate::style::declarations::{self, DeclarationError, Mutation};

pub(super) const GET_PROPERTY_VALUE: &str = "host.style.getPropertyValue";
const SET_PROPERTY: &str = "host.style.setProperty";
const REMOVE_PROPERTY: &str = "host.style.removeProperty";
const GET_PROPERTY_PRIORITY: &str = "host.style.getPropertyPriority";

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        GET_PROPERTY_VALUE => Some(("getPropertyValue", 1)),
        SET_PROPERTY => Some(("setProperty", 2)),
        REMOVE_PROPERTY => Some(("removeProperty", 1)),
        GET_PROPERTY_PRIORITY => Some(("getPropertyPriority", 1)),
        _ => None,
    }
}

fn method_key(name: &str) -> Option<&'static str> {
    match name {
        "getPropertyValue" => Some(GET_PROPERTY_VALUE),
        "setProperty" => Some(SET_PROPERTY),
        "removeProperty" => Some(REMOVE_PROPERTY),
        "getPropertyPriority" => Some(GET_PROPERTY_PRIORITY),
        _ => None,
    }
}

fn declaration_error(error: DeclarationError) -> JsError {
    failure(format!("CSS declaration operation failed: {error:?}"))
}

fn style_id(handle: &str, context: &Context) -> JsResult<usize> {
    let id = handle
        .strip_prefix("style:")
        .and_then(|suffix| suffix.parse::<usize>().ok())
        .ok_or_else(|| failure("CSS declaration requires its style receiver"))?;
    let shared = state(context);
    let data = shared.borrow();
    if id == 0
        || data
            .host
            .document
            .nodes
            .get(id)
            .is_none_or(|node| node.tag == "#text")
    {
        return Err(failure("Invalid or stale style receiver"));
    }
    Ok(id)
}

fn receiver_id(value: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(value, context)? else {
        return Err(failure("CSS declaration requires its style receiver"));
    };
    style_id(&handle, context)
}

// This is an IDL-name mapping, deliberately separate from getPropertyValue's
// CSS-name argument. Custom properties are not generated named IDL accessors.
fn named_property(name: &str) -> JsResult<Option<String>> {
    if name.len() > declarations::ATTRIBUTE_BYTES {
        return Err(declaration_error(DeclarationError::TooLarge(
            declarations::Part::CssName,
        )));
    }
    if name.starts_with("--") {
        return Ok(None);
    }
    let css = super::super::css_property_name(name);
    declarations::enabled_non_custom_css_name(&css)
        .map(|id| id.map(|_| css))
        .map_err(declaration_error)
}

fn convert(value: &JsValue, null_is_empty: bool, context: &mut Context) -> JsResult<String> {
    // CSSOMString is USVString. Preserve real ToString/throw identity, with
    // scalar replacement only at the existing UTF-8 boundary. No State borrow
    // survives callback-capable conversion. A preexisting fatal takes priority.
    let converted = if null_is_empty && value.is_null() {
        Ok(String::new())
    } else {
        utf8(value, context)
    };
    modern::check_context(context)?;
    converted
}

fn text_value(text: String, context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let count = text.encode_utf16().count();
    let bytes = count
        .checked_mul(std::mem::size_of::<u16>())
        .and_then(|bytes| bytes.checked_add(text.capacity()))
        .ok_or_else(|| failure("CSS declaration result storage overflow"))?;
    // Ordinary Host admission, not the message-specific fatal storage policy.
    state(context)
        .borrow_mut()
        .host
        .charge(bytes)
        .map_err(failure)?;
    let mut units = Vec::<u16>::new();
    units
        .try_reserve_exact(count)
        .map_err(|_| failure("CSS declaration result storage unavailable"))?;
    let excess = units
        .capacity()
        .checked_sub(count)
        .and_then(|units| units.checked_mul(std::mem::size_of::<u16>()))
        .ok_or_else(|| failure("CSS declaration result storage overflow"))?;
    state(context)
        .borrow_mut()
        .host
        .charge(excess)
        .map_err(failure)?;
    units.extend(text.encode_utf16());
    // The runtime's own string copy remains under requested-System admission;
    // the actual serializer/String and UTF-16 bridge capacities were admitted.
    let result = JsString::from(units.as_slice());
    modern::check_context(context)?;
    Ok(result.into())
}

fn read_value(id: usize, property: Option<&str>, context: &mut Context) -> JsResult<JsValue> {
    let text = {
        let shared = state(context);
        let data = shared.borrow();
        let raw = data.host.document.nodes[id].attr("style").unwrap_or("");
        let base = data.host.base_url();
        // These native declaration APIs cannot call JS, fetch, cascade, create
        // a Device or reach the separate Stylo thread pool.
        match property {
            Some(name) => declarations::declared_value(raw, name, &base),
            None => declarations::declared_css_text(raw, &base),
        }
        .map_err(declaration_error)?
    };
    modern::check_context(context)?;
    text_value(text, context)
}

pub(super) fn get(name: &str, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    if let Some(key) = method_key(name) {
        return method(key, context).map(Into::into);
    }
    if name == "cssText" {
        let id = receiver_id(&args[2], context)?;
        return read_value(id, None, context);
    }
    if let Some(property) = named_property(name)? {
        let id = receiver_id(&args[2], context)?;
        return read_value(id, Some(&property), context);
    }
    // Do not fall through to BrowserHost's intentionally preserved raw legacy
    // style getter. Unknown interface/property names use ordinary lookup only.
    let result = fallback_get(args, context);
    modern::check_context(context)?;
    result
}

pub(super) fn set(name: &str, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let id = receiver_id(&args[3], context)?;
    let property = if name == "cssText" {
        None
    } else {
        Some(
            named_property(name)?
                .ok_or_else(|| failure(format!("Setting style.{name} is not implemented")))?,
        )
    };
    let text = convert(&args[2], property.is_some(), context)?;
    modern::check_context(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let base = data.host.base_url();
    let raw = data.host.document.nodes[id].attr("style").unwrap_or("");
    let mutation = match property {
        Some(property) => declarations::assign_declared_value(raw, &property, &text, &base),
        None => declarations::replace_declared_css_text(raw, &text, &base),
    }
    .map_err(declaration_error)?;
    if let Mutation::Replace(value) = mutation {
        // attr admits its original name+len before mutation. Admit additional
        // canonical String capacity first; all errors leave prior DOM intact.
        data.host
            .charge(value.capacity() - value.len())
            .map_err(failure)?;
        data.host.attr(id, "style", value).map_err(failure)?;
    }
    Ok(true.into())
}

pub(super) fn has(name: &str, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    // Structural presence only: no declaration parsing/serialization, method
    // creation, wrapper creation or result/DOM charge, including at full quota.
    if name == "cssText" || method_key(name).is_some() || named_property(name)?.is_some() {
        return Ok(true.into());
    }
    let result = args[0]
        .as_object()
        .unwrap()
        .has_property(JsString::from(name), context);
    modern::check_context(context)?;
    result.map(Into::into)
}

pub(super) fn get_property_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    // Unlike the generic historical dispatch, WebIDL receiver validation
    // precedes required arity and every callback-capable argument conversion.
    let id = receiver_id(this, context)?;
    let argument = args
        .first()
        .ok_or_else(|| failure("getPropertyValue requires a property argument"))?;
    let name = convert(argument, false, context)?;
    read_value(id, Some(&name), context)
}

fn commit(id: usize, mutation: Mutation, context: &mut Context) -> JsResult<()> {
    modern::check_context(context)?;
    if let Mutation::Replace(value) = mutation {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        // Admit capacity excess before attr's existing name+len admission.
        // Neither admission failure may replace any portion of the old value.
        data.host
            .charge(value.capacity() - value.len())
            .map_err(failure)?;
        data.host.attr(id, "style", value).map_err(failure)?;
    }
    Ok(())
}

pub(super) fn call(
    method_name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    if method_name == GET_PROPERTY_VALUE {
        return get_property_value(this, args, context);
    }
    modern::check_context(context)?;
    let id = receiver_id(this, context)?;
    let required = if method_name == SET_PROPERTY { 2 } else { 1 };
    if args.len() < required {
        return Err(failure(
            "CSS declaration method is missing required arguments",
        ));
    }
    let name = convert(&args[0], false, context)?;
    if method_name == SET_PROPERTY {
        let value = convert(&args[1], true, context)?;
        let priority = match args.get(2).filter(|value| !value.is_undefined()) {
            Some(value) => convert(value, true, context)?,
            None => String::new(),
        };
        // All callbacks/conversions finish before reading the current DOM.
        let mutation = {
            let shared = state(context);
            let data = shared.borrow();
            let raw = data.host.document.nodes[id].attr("style").unwrap_or("");
            declarations::set_declared_value(raw, &name, &value, &priority, &data.host.base_url())
                .map_err(declaration_error)?
        };
        commit(id, mutation, context)?;
        return Ok(JsValue::undefined());
    }
    if method_name == REMOVE_PROPERTY {
        let (text, mutation) = {
            let shared = state(context);
            let data = shared.borrow();
            let raw = data.host.document.nodes[id].attr("style").unwrap_or("");
            declarations::remove_declared_value(raw, &name, &data.host.base_url())
                .map_err(declaration_error)?
        };
        // Admit and construct the return value before any DOM commit. Host
        // failures remain ordinary errors, with attempted charges retained.
        let result = text_value(text, context)?;
        commit(id, mutation, context)?;
        return Ok(result);
    }
    let text = {
        let shared = state(context);
        let data = shared.borrow();
        let raw = data.host.document.nodes[id].attr("style").unwrap_or("");
        declarations::declared_priority(raw, &name, &data.host.base_url())
            .map_err(declaration_error)?
    };
    text_value(text, context)
}
