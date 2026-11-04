//! Parent-brokered same-origin `fetch()` and async `XMLHttpRequest`, plus
//! `Headers`, `Request`, `Response`, `AbortController`, and `AbortSignal`.
use super::{
    BoaPageRealm, def_cap, def_fn, def_getter_cap, def_getter_fn, def_prop, dom_events,
    dom_exception, failure, link_ctor_global, modern, receiver, state, utf8, web_apis,
    wrap_with_proto,
};
use crate::page_session::platform::{
    MAX_FETCH_HEADER_BYTES, MAX_FETCH_HEADERS, MAX_FETCH_REQ_BODY_B64_BYTES,
    MAX_FETCH_REQUESTS_PER_TX, MAX_FETCH_URL_BYTES, ScriptFetchRequest, ScriptFetchResponse,
};
use mg_butane::runtime::Value;
use modern::boa_engine::{
    Context, JsData, JsError, JsNativeError, JsResult, JsString, JsSymbol, JsValue, NativeFunction,
    js_string,
    object::{
        FunctionObjectBuilder, JsObject, ObjectInitializer,
        builtins::{JsArray, JsArrayBuffer, JsPromise, JsUint8Array},
    },
    property::{Attribute, PropertyKey},
};
use modern::boa_gc::{Finalize, Gc, GcRefCell, Trace};
use std::cell::Cell;

const MAX_ABORT_SIGNALS: usize = 256;
const MAX_XHR_INSTANCES: usize = 128;
const MAX_PENDING_FETCHES: usize = 64;

pub(super) const SIGNAL_THROW_IF_ABORTED: &str = "host.m2.signal.throwIfAborted";
pub(super) const XHR_OPEN: &str = "host.m2.xhr.open";
pub(super) const XHR_SET_REQUEST_HEADER: &str = "host.m2.xhr.setRequestHeader";
pub(super) const XHR_SEND: &str = "host.m2.xhr.send";
pub(super) const XHR_ABORT: &str = "host.m2.xhr.abort";
pub(super) const XHR_GET_RESPONSE_HEADER: &str = "host.m2.xhr.getResponseHeader";
pub(super) const XHR_GET_ALL_RESPONSE_HEADERS: &str = "host.m2.xhr.getAllResponseHeaders";
pub(super) const XHR_OVERRIDE_MIME_TYPE: &str = "host.m2.xhr.overrideMimeType";
pub(super) const WINDOW_FETCH: &str = "host.window.fetch";

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        SIGNAL_THROW_IF_ABORTED => Some(("throwIfAborted", 0)),
        XHR_OPEN => Some(("open", 2)),
        XHR_SET_REQUEST_HEADER => Some(("setRequestHeader", 2)),
        XHR_SEND => Some(("send", 0)),
        XHR_ABORT => Some(("abort", 0)),
        XHR_GET_RESPONSE_HEADER => Some(("getResponseHeader", 1)),
        XHR_GET_ALL_RESPONSE_HEADERS => Some(("getAllResponseHeaders", 0)),
        XHR_OVERRIDE_MIME_TYPE => Some(("overrideMimeType", 1)),
        WINDOW_FETCH => Some(("fetch", 1)),
        _ => None,
    }
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct FetchXhrPrototypes {
    pub abort_signal: JsObject,
    pub xhr: JsObject,
    pub headers: JsObject,
    pub request: JsObject,
    pub response: JsObject,
    pub headers_iter: JsObject,
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct SignalListener {
    #[unsafe_ignore_trace]
    pub event_type: String,
    pub callback: Option<JsObject>,
    pub capture: bool,
    pub once: bool,
    pub passive: bool,
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct AbortSignalRecord {
    pub aborted: bool,
    pub reason: JsValue,
    pub onabort: Option<JsObject>,
    pub listeners: Vec<SignalListener>,
    #[unsafe_ignore_trace]
    pub bound_listeners: Vec<usize>,
    #[unsafe_ignore_trace]
    pub bound_fetches: Vec<u64>,
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct PendingFetch {
    #[unsafe_ignore_trace]
    pub request_id: u64,
    #[unsafe_ignore_trace]
    pub request_url: String,
    pub resolve: JsObject,
    pub reject: JsObject,
    pub aborted: bool,
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct XhrRecord {
    #[unsafe_ignore_trace]
    pub ready_state: u8,
    #[unsafe_ignore_trace]
    pub method: String,
    #[unsafe_ignore_trace]
    pub request_url: String,
    #[unsafe_ignore_trace]
    pub same_origin: bool,
    #[unsafe_ignore_trace]
    pub send_flag: bool,
    #[unsafe_ignore_trace]
    pub active_request_id: Option<u64>,
    #[unsafe_ignore_trace]
    pub request_headers: Vec<(String, String)>,
    #[unsafe_ignore_trace]
    pub status: u16,
    #[unsafe_ignore_trace]
    pub status_text: String,
    #[unsafe_ignore_trace]
    pub response_url: String,
    #[unsafe_ignore_trace]
    pub response_type: String,
    #[unsafe_ignore_trace]
    pub response_headers: Vec<(String, String)>,
    #[unsafe_ignore_trace]
    pub response_bytes: Vec<u8>,
    #[unsafe_ignore_trace]
    pub override_mime: Option<String>,
    #[unsafe_ignore_trace]
    pub timeout_ms: u32,
    #[unsafe_ignore_trace]
    pub with_credentials: bool,
    #[unsafe_ignore_trace]
    pub network_error: bool,
    pub onreadystatechange: Option<JsObject>,
    pub onload: Option<JsObject>,
    pub onerror: Option<JsObject>,
    pub onabort: Option<JsObject>,
    pub onloadstart: Option<JsObject>,
    pub onloadend: Option<JsObject>,
    pub ontimeout: Option<JsObject>,
    pub listeners: Vec<SignalListener>,
}

impl XhrRecord {
    fn new() -> Self {
        Self {
            ready_state: 0,
            method: String::new(),
            request_url: String::new(),
            same_origin: false,
            send_flag: false,
            active_request_id: None,
            request_headers: Vec::new(),
            status: 0,
            status_text: String::new(),
            response_url: String::new(),
            response_type: String::new(),
            response_headers: Vec::new(),
            response_bytes: Vec::new(),
            override_mime: None,
            timeout_ms: 0,
            with_credentials: false,
            network_error: false,
            onreadystatechange: None,
            onload: None,
            onerror: None,
            onabort: None,
            onloadstart: None,
            onloadend: None,
            ontimeout: None,
            listeners: Vec::new(),
        }
    }
}

#[derive(Trace, Finalize, JsData)]
struct AbortControllerData {
    sig_id: usize,
}

#[derive(Trace, Finalize, JsData)]
struct HeadersData {
    #[unsafe_ignore_trace]
    entries: Gc<GcRefCell<Vec<(String, String)>>>,
}

#[derive(Clone, Copy)]
enum IterKind {
    Keys,
    Values,
    Entries,
}

#[derive(Trace, Finalize, JsData)]
struct HeadersIterData {
    #[unsafe_ignore_trace]
    snapshot: Vec<(String, String)>,
    #[unsafe_ignore_trace]
    index: Cell<usize>,
    #[unsafe_ignore_trace]
    kind: IterKind,
}

#[derive(Trace, Finalize, JsData)]
struct RequestData {
    #[unsafe_ignore_trace]
    method: String,
    #[unsafe_ignore_trace]
    url: String,
    headers: JsObject,
    sig_id: usize,
    #[unsafe_ignore_trace]
    body: GcRefCell<Option<Vec<u8>>>,
    #[unsafe_ignore_trace]
    body_used: Cell<bool>,
}

#[derive(Trace, Finalize, JsData)]
struct ResponseData {
    #[unsafe_ignore_trace]
    status: u16,
    #[unsafe_ignore_trace]
    status_text: String,
    #[unsafe_ignore_trace]
    url: String,
    #[unsafe_ignore_trace]
    redirected: bool,
    #[unsafe_ignore_trace]
    rtype: &'static str,
    headers: JsObject,
    #[unsafe_ignore_trace]
    body: GcRefCell<Option<Vec<u8>>>,
    #[unsafe_ignore_trace]
    body_used: Cell<bool>,
}

// --- HTTP Token & Header Validation ---

fn is_tchar(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.'
            | b'^' | b'_' | b'`' | b'|' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z'
    )
}

fn validate_header_name(name: &str) -> JsResult<String> {
    if name.is_empty() || name.len() > MAX_FETCH_HEADER_BYTES || !name.bytes().all(is_tchar) {
        return Err(JsNativeError::typ()
            .with_message("Invalid HTTP header name")
            .into());
    }
    Ok(name.to_ascii_lowercase())
}

fn normalize_header_value(raw: &str) -> JsResult<String> {
    if raw.chars().any(|c| (c as u32) > 0xFF) {
        return Err(JsNativeError::typ()
            .with_message("Header value is not a valid ByteString")
            .into());
    }
    let trimmed = raw.trim_matches(|c| matches!(c, '\x09' | '\x0A' | '\x0D' | '\x20'));
    if trimmed.len() > MAX_FETCH_HEADER_BYTES
        || trimmed.bytes().any(|b| matches!(b, 0x00 | 0x0A | 0x0D))
    {
        return Err(JsNativeError::typ()
            .with_message("Invalid HTTP header value")
            .into());
    }
    Ok(trimmed.to_owned())
}

fn normalize_method(raw: &str) -> JsResult<String> {
    if raw.is_empty() || raw.len() > 32 || !raw.bytes().all(is_tchar) {
        return Err(JsNativeError::typ()
            .with_message("Invalid HTTP method")
            .into());
    }
    let upper = raw.to_ascii_uppercase();
    if matches!(upper.as_str(), "CONNECT" | "TRACE" | "TRACK") {
        return Err(JsNativeError::typ()
            .with_message("Forbidden HTTP method")
            .into());
    }
    if matches!(
        upper.as_str(),
        "DELETE" | "GET" | "HEAD" | "OPTIONS" | "POST" | "PUT"
    ) {
        Ok(upper)
    } else {
        Ok(raw.to_owned())
    }
}

fn is_forbidden_xhr_header(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "accept-charset"
            | "accept-encoding"
            | "access-control-request-headers"
            | "access-control-request-method"
            | "connection"
            | "content-length"
            | "cookie"
            | "cookie2"
            | "date"
            | "dnt"
            | "expect"
            | "host"
            | "keep-alive"
            | "origin"
            | "referer"
            | "set-cookie"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "via"
    ) || lower_name.starts_with("proxy-")
        || lower_name.starts_with("sec-")
}

fn is_same_http_origin(page: &url::Url, target: &url::Url) -> bool {
    if !matches!(target.scheme(), "http" | "https") {
        return false;
    }
    if !matches!(page.scheme(), "http" | "https") {
        return false;
    }
    page.scheme() == target.scheme()
        && page.host_str() == target.host_str()
        && page.port_or_known_default() == target.port_or_known_default()
}

fn resolve_against_document(raw_url: &str, context: &mut Context) -> JsResult<url::Url> {
    let base = state(context).borrow().host.url.clone();
    let parsed = base.join(raw_url).or_else(|_| url::Url::parse(raw_url));
    let url = parsed.map_err(|e| JsNativeError::typ().with_message(format!("Invalid URL: {e}")))?;
    if url.as_str().len() > MAX_FETCH_URL_BYTES {
        return Err(JsNativeError::typ()
            .with_message("Request URL exceeds 16 KiB limit")
            .into());
    }
    Ok(url)
}

// --- Registration ---

pub(super) fn register(context: &mut Context, enable_network: bool) -> Result<(), String> {
    let abort_signal_proto = ObjectInitializer::new(context).build();
    let headers_iter_proto = build_headers_iter_proto(context);
    let headers_proto = build_headers_proto(headers_iter_proto.clone(), context);
    let request_proto = build_request_proto(context);
    let response_proto = build_response_proto(context);
    let xhr_proto = ObjectInitializer::new(context).build();

    state(context).borrow_mut().fetch_xhr_prototypes = Some(FetchXhrPrototypes {
        abort_signal: abort_signal_proto.clone(),
        xhr: xhr_proto.clone(),
        headers: headers_proto.clone(),
        request: request_proto.clone(),
        response: response_proto.clone(),
        headers_iter: headers_iter_proto,
    });

    register_abort_globals(abort_signal_proto, context)?;
    register_headers_global(headers_proto, context)?;
    register_request_global(request_proto, context)?;
    register_response_global(response_proto, context)?;

    if enable_network {
        register_xhr_global(xhr_proto, context)?;
        let fetch_fn = super::method(WINDOW_FETCH, context).map_err(|e| e.to_string())?;
        def_prop(
            &context.global_object(),
            js_string!("fetch"),
            fetch_fn,
            true,
            true,
            true,
            context,
        )
        .map_err(|_| "Failed to register fetch")?;
    }
    Ok(())
}

// --- `AbortController` & `AbortSignal` ---

fn alloc_abort_signal(aborted: bool, reason: JsValue, context: &mut Context) -> JsResult<usize> {
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if data.abort_signals.len() >= MAX_ABORT_SIGNALS {
        return Err(failure("AbortSignal limit exceeded"));
    }
    data.host.charge(128).map_err(failure)?;
    let sig_id = data.abort_signals.len();
    data.abort_signals.push(AbortSignalRecord {
        aborted,
        reason,
        onabort: None,
        listeners: Vec::new(),
        bound_listeners: Vec::new(),
        bound_fetches: Vec::new(),
    });
    Ok(sig_id)
}

fn wrap_abort_signal(sig_id: usize, context: &mut Context) -> JsResult<JsValue> {
    let proto = state(context)
        .borrow()
        .fetch_xhr_prototypes
        .as_ref()
        .map(|p| p.abort_signal.clone())
        .ok_or_else(|| failure("AbortSignal prototype unavailable"))?;
    wrap_with_proto(format!("abortsignal:{sig_id}"), proto, context)
}

pub(super) fn abort_signal_id(val: &JsValue, context: &mut Context) -> JsResult<usize> {
    if let Ok(Value::Host(handle)) = receiver(val, context)
        && let Some(sid) = handle
            .strip_prefix("abortsignal:")
            .and_then(|s| s.parse::<usize>().ok())
        && sid < state(context).borrow().abort_signals.len()
    {
        return Ok(sid);
    }
    Err(JsNativeError::typ()
        .with_message("Expected an AbortSignal instance")
        .into())
}

fn default_abort_reason(context: &mut Context) -> JsResult<JsValue> {
    let err = dom_exception::exception("AbortError", "signal is aborted without reason", context)?;
    err.into_opaque(context)
}

fn trigger_abort_signal(
    sig_id: usize,
    explicit_reason: Option<JsValue>,
    context: &mut Context,
) -> JsResult<()> {
    modern::check_context(context)?;
    {
        let shared = state(context);
        let data = shared.borrow();
        let Some(rec) = data.abort_signals.get(sig_id) else {
            return Ok(());
        };
        if rec.aborted {
            return Ok(());
        }
    }
    let reason = match explicit_reason {
        Some(v) if !v.is_undefined() => v,
        _ => default_abort_reason(context)?,
    };

    let bound_fetches = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let bound_listeners = std::mem::take(&mut data.abort_signals[sig_id].bound_listeners);
        let bound_fetches = std::mem::take(&mut data.abort_signals[sig_id].bound_fetches);
        data.abort_signals[sig_id].aborted = true;
        data.abort_signals[sig_id].reason = reason.clone();
        for slot in bound_listeners {
            if let Some(l) = data.listeners.get_mut(slot) {
                l.callback = None;
            }
        }
        bound_fetches
    };

    // Reject any pending fetches linked to this signal
    for req_id in bound_fetches {
        let reject_fn = {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let mut found = None;
            if let Some(pf) = data
                .pending_fetches
                .iter_mut()
                .find(|pf| pf.request_id == req_id && !pf.aborted)
            {
                pf.aborted = true;
                found = Some(pf.reject.clone());
            }
            data.fetch_requests.retain(|r| r.request_id != req_id);
            found
        };
        if let Some(reject) = reject_fn {
            let _ = reject.call(&JsValue::undefined(), &[reason.clone()], context);
        }
    }

    // Fire `abort` event on the AbortSignal
    let ts = dom_events::elapsed_ms(&state(context).borrow());
    let mevent_id = dom_events::alloc_modern_event(
        dom_events::ModernEvent::simple("abort", false, false, true, ts),
        context,
    )?;
    let _ = signal_dispatch_event(sig_id, mevent_id, context)?;
    Ok(())
}

fn register_abort_globals(
    abort_signal_proto: JsObject,
    context: &mut Context,
) -> Result<(), String> {
    let global = context.global_object();
    let signal_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|_, _, context| {
            modern::check_context(context)?;
            Err(JsNativeError::typ()
                .with_message("Illegal constructor")
                .into())
        }),
    )
    .name(js_string!("AbortSignal"))
    .length(0)
    .constructor(true)
    .build()
    .into();
    def_fn(
        &signal_ctor,
        "abort",
        0,
        true,
        |_, args, context| {
            modern::check_context(context)?;
            let reason = match args.first() {
                Some(v) if !v.is_undefined() => v.clone(),
                _ => default_abort_reason(context)?,
            };
            let sig_id = alloc_abort_signal(true, reason, context)?;
            wrap_abort_signal(sig_id, context)
        },
        context,
    )
    .map_err(|_| "AbortSignal.abort bind failed")?;
    link_ctor_global(
        &global,
        "AbortSignal",
        &signal_ctor,
        &abort_signal_proto,
        context,
    )?;

    let ctrl_proto = ObjectInitializer::new(context).build();
    let ctrl_id = |this: &JsValue| -> JsResult<usize> {
        this.as_object()
            .and_then(|o| o.downcast_ref::<AbortControllerData>().map(|d| d.sig_id))
            .ok_or_else(|| failure("Invalid AbortController receiver"))
    };
    def_getter_fn(
        &ctrl_proto,
        "signal",
        true,
        |this, _, context| {
            modern::check_context(context)?;
            let obj = this
                .as_object()
                .ok_or_else(|| failure("Invalid AbortController receiver"))?;
            let sig_id = obj
                .downcast_ref::<AbortControllerData>()
                .ok_or_else(|| failure("Invalid AbortController receiver"))?
                .sig_id;
            wrap_abort_signal(sig_id, context)
        },
        context,
    )
    .map_err(|_| "AbortController.signal failed")?;
    let _ = ctrl_id;
    def_fn(
        &ctrl_proto,
        "abort",
        0,
        false,
        |this, args, context| {
            modern::check_context(context)?;
            let obj = this
                .as_object()
                .ok_or_else(|| failure("Invalid AbortController receiver"))?;
            let sig_id = obj
                .downcast_ref::<AbortControllerData>()
                .ok_or_else(|| failure("Invalid AbortController receiver"))?
                .sig_id;
            trigger_abort_signal(sig_id, args.first().cloned(), context)?;
            Ok(JsValue::undefined())
        },
        context,
    )
    .map_err(|_| "AbortController.abort failed")?;

    let ctrl_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, _, ctrl_proto, context| {
                modern::check_context(context)?;
                if new_target.is_undefined() {
                    return Err(JsNativeError::typ()
                        .with_message("Failed to construct 'AbortController': Please use 'new'")
                        .into());
                }
                let sig_id = alloc_abort_signal(false, JsValue::undefined(), context)?;
                Ok(JsObject::from_proto_and_data(
                    Some(ctrl_proto.clone()),
                    AbortControllerData { sig_id },
                )
                .into())
            },
            ctrl_proto.clone(),
        ),
    )
    .name(js_string!("AbortController"))
    .length(0)
    .constructor(true)
    .build()
    .into();

    link_ctor_global(&global, "AbortController", &ctrl_ctor, &ctrl_proto, context)
}

// --- `Headers` ---

fn parse_headers_init(
    init: Option<&JsValue>,
    context: &mut Context,
) -> JsResult<Vec<(String, String)>> {
    let mut entries = Vec::new();
    let Some(val) = init else {
        return Ok(entries);
    };
    if val.is_undefined() {
        return Ok(entries);
    }
    let Some(obj) = val.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("Headers init must be an object or sequence")
            .into());
    };
    if let Some(hd) = obj.downcast_ref::<HeadersData>() {
        return Ok(hd.entries.borrow().clone());
    }
    if let Some(iter_fn) = obj.get(JsSymbol::iterator(), context)?.as_callable() {
        let iter_val = iter_fn.call(val, &[], context)?;
        let Some(iter_obj) = iter_val.as_object() else {
            return Err(JsNativeError::typ()
                .with_message("Iterator must be an object")
                .into());
        };
        let next_fn = iter_obj
            .get(js_string!("next"), context)?
            .as_callable()
            .ok_or_else(|| JsNativeError::typ().with_message("Iterator.next is not callable"))?;
        for _ in 0..(MAX_FETCH_HEADERS + 8) {
            modern::check_context(context)?;
            let step_res = next_fn.call(&iter_val, &[], context)?;
            let Some(step_obj) = step_res.as_object() else {
                return Err(JsNativeError::typ()
                    .with_message("Iterator result must be an object")
                    .into());
            };
            if step_obj.get(js_string!("done"), context)?.to_boolean() {
                break;
            }
            let step = step_obj.get(js_string!("value"), context)?;
            let Some(pair_obj) = step.as_object() else {
                return Err(JsNativeError::typ()
                    .with_message("Headers sequence item must be a 2-tuple")
                    .into());
            };
            let len = pair_obj
                .get(js_string!("length"), context)?
                .to_length(context)?;
            if len != 2 {
                return Err(JsNativeError::typ()
                    .with_message("Headers sequence item must have length 2")
                    .into());
            }
            let raw_name = utf8(&pair_obj.get(0, context)?, context)?;
            let raw_val = utf8(&pair_obj.get(1, context)?, context)?;
            headers_append_vec(&mut entries, &raw_name, &raw_val)?;
        }
    } else {
        let keys = obj.own_property_keys(context)?;
        for k in keys {
            let is_enum = obj
                .try_borrow()
                .map(|b| {
                    b.properties()
                        .get(&k)
                        .is_some_and(|d| d.enumerable() == Some(true))
                })
                .unwrap_or(true);
            if !is_enum {
                continue;
            }
            match &k {
                PropertyKey::String(ks) => {
                    let v = obj.get(k.clone(), context)?;
                    let raw_val = utf8(&v, context)?;
                    headers_append_vec(&mut entries, &ks.to_std_string_escaped(), &raw_val)?;
                }
                PropertyKey::Index(idx) => {
                    let v = obj.get(k.clone(), context)?;
                    let raw_val = utf8(&v, context)?;
                    headers_append_vec(&mut entries, &idx.get().to_string(), &raw_val)?;
                }
                PropertyKey::Symbol(_) => {}
            }
        }
    }
    Ok(entries)
}

fn headers_append_vec(
    entries: &mut Vec<(String, String)>,
    raw_name: &str,
    raw_val: &str,
) -> JsResult<()> {
    let name = validate_header_name(raw_name)?;
    let val = normalize_header_value(raw_val)?;
    if entries.len() >= MAX_FETCH_HEADERS {
        return Err(JsNativeError::typ()
            .with_message("Headers count exceeds limit")
            .into());
    }
    entries.push((name, val));
    Ok(())
}

fn headers_sorted_snapshot(entries: &[(String, String)]) -> Vec<(String, String)> {
    let mut combined: Vec<(String, String)> = Vec::new();
    for (k, v) in entries {
        if k == "set-cookie" {
            combined.push((k.clone(), v.clone()));
        } else if let Some(existing) = combined.iter_mut().find(|(ek, _)| ek == k) {
            existing.1.push_str(", ");
            existing.1.push_str(v);
        } else {
            combined.push((k.clone(), v.clone()));
        }
    }
    combined.sort_by(|a, b| a.0.cmp(&b.0));
    combined
}

fn headers_data(this: &JsValue) -> JsResult<Gc<GcRefCell<Vec<(String, String)>>>> {
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid Headers receiver"))?;
    let hd = obj
        .downcast_ref::<HeadersData>()
        .ok_or_else(|| failure("Invalid Headers receiver"))?;
    Ok(hd.entries.clone())
}

fn build_headers_iter_proto(context: &mut Context) -> JsObject {
    let proto = ObjectInitializer::new(context).build();
    let _ = def_fn(
        &proto,
        "next",
        0,
        false,
        |this, _, context| {
            modern::check_context(context)?;
            let obj = this
                .as_object()
                .ok_or_else(|| failure("Invalid HeadersIterator receiver"))?;
            let data = obj
                .downcast_ref::<HeadersIterData>()
                .ok_or_else(|| failure("Invalid HeadersIterator receiver"))?;
            let idx = data.index.get();
            if idx >= data.snapshot.len() {
                return Ok(ObjectInitializer::new(context)
                    .property(js_string!("value"), JsValue::undefined(), Attribute::all())
                    .property(js_string!("done"), true, Attribute::all())
                    .build()
                    .into());
            }
            data.index.set(idx + 1);
            let (k, v) = data.snapshot[idx].clone();
            let value: JsValue = match data.kind {
                IterKind::Keys => JsString::from(k).into(),
                IterKind::Values => JsString::from(v).into(),
                IterKind::Entries => JsArray::from_iter(
                    [JsString::from(k).into(), JsString::from(v).into()],
                    context,
                )
                .into(),
            };
            Ok(ObjectInitializer::new(context)
                .property(js_string!("value"), value, Attribute::all())
                .property(js_string!("done"), false, Attribute::all())
                .build()
                .into())
        },
        context,
    );
    let self_iter = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|this, _, _| Ok(this.clone())),
    )
    .name(js_string!("[Symbol.iterator]"))
    .length(0)
    .build();
    let _ = def_prop(
        &proto,
        JsSymbol::iterator(),
        self_iter,
        true,
        false,
        true,
        context,
    );
    proto
}

fn headers_call(
    op: u8,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let entries = headers_data(this)?;
    match op {
        0 => {
            if args.len() < 2 {
                return Err(JsNativeError::typ()
                    .with_message("Headers.append requires 2 arguments")
                    .into());
            }
            let name = utf8(&args[0], context)?;
            let val = utf8(&args[1], context)?;
            headers_append_vec(&mut entries.borrow_mut(), &name, &val)?;
            Ok(JsValue::undefined())
        }
        1 => {
            let first = args.first().ok_or_else(|| {
                JsNativeError::typ().with_message("Headers.delete requires 1 argument")
            })?;
            let name = validate_header_name(&utf8(first, context)?)?;
            entries.borrow_mut().retain(|(k, _)| k != &name);
            Ok(JsValue::undefined())
        }
        2 => {
            let first = args.first().ok_or_else(|| {
                JsNativeError::typ().with_message("Headers.get requires 1 argument")
            })?;
            let name = validate_header_name(&utf8(first, context)?)?;
            let borrowed = entries.borrow();
            let vals: Vec<&str> = borrowed
                .iter()
                .filter(|(k, _)| k == &name)
                .map(|(_, v)| v.as_str())
                .collect();
            if vals.is_empty() {
                Ok(JsValue::null())
            } else {
                Ok(JsString::from(vals.join(", ")).into())
            }
        }
        3 => {
            let cookies: Vec<JsValue> = entries
                .borrow()
                .iter()
                .filter(|(k, _)| k == "set-cookie")
                .map(|(_, v)| JsString::from(v.as_str()).into())
                .collect();
            Ok(JsArray::from_iter(cookies, context).into())
        }
        4 => {
            let first = args.first().ok_or_else(|| {
                JsNativeError::typ().with_message("Headers.has requires 1 argument")
            })?;
            let name = validate_header_name(&utf8(first, context)?)?;
            Ok(entries.borrow().iter().any(|(k, _)| k == &name).into())
        }
        5 => {
            if args.len() < 2 {
                return Err(JsNativeError::typ()
                    .with_message("Headers.set requires 2 arguments")
                    .into());
            }
            let name = validate_header_name(&utf8(&args[0], context)?)?;
            let val = normalize_header_value(&utf8(&args[1], context)?)?;
            let mut vec = entries.borrow_mut();
            let mut replaced = false;
            vec.retain_mut(|(k, v)| {
                if k == &name {
                    if !replaced {
                        *v = val.clone();
                        replaced = true;
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            });
            if !replaced {
                if vec.len() >= MAX_FETCH_HEADERS {
                    return Err(JsNativeError::typ()
                        .with_message("Headers count exceeds limit")
                        .into());
                }
                vec.push((name, val));
            }
            Ok(JsValue::undefined())
        }
        _ => {
            let cb = args
                .first()
                .and_then(|v| v.as_callable())
                .ok_or_else(|| failure("Headers.forEach requires a callback function"))?;
            let this_arg = args.get(1).cloned().unwrap_or_default();
            for (k, v) in headers_sorted_snapshot(&entries.borrow()) {
                cb.call(
                    &this_arg,
                    &[
                        JsString::from(v).into(),
                        JsString::from(k).into(),
                        this.clone(),
                    ],
                    context,
                )?;
            }
            Ok(JsValue::undefined())
        }
    }
}

fn build_headers_proto(iter_proto: JsObject, context: &mut Context) -> JsObject {
    let _ = iter_proto;
    let proto = ObjectInitializer::new(context).build();
    for (name, len, op) in [
        ("append", 2, 0u8),
        ("delete", 1, 1),
        ("get", 1, 2),
        ("getSetCookie", 0, 3),
        ("has", 1, 4),
        ("set", 2, 5),
        ("forEach", 1, 6),
    ] {
        let _ = def_cap(
            &proto,
            name,
            len,
            false,
            op,
            |this, args, &op, ctx| headers_call(op, this, args, ctx),
            context,
        );
    }
    for (fn_name, kind) in [
        ("entries", IterKind::Entries),
        ("keys", IterKind::Keys),
        ("values", IterKind::Values),
    ] {
        let _ = def_cap(
            &proto,
            fn_name,
            0,
            false,
            kind,
            |this, _, &kind, context| {
                modern::check_context(context)?;
                let entries = headers_data(this)?;
                let snapshot = headers_sorted_snapshot(&entries.borrow());
                let ip = state(context)
                    .borrow()
                    .fetch_xhr_prototypes
                    .as_ref()
                    .map(|p| p.headers_iter.clone())
                    .ok_or_else(|| failure("HeadersIterator prototype unavailable"))?;
                Ok(JsObject::from_proto_and_data(
                    Some(ip),
                    HeadersIterData {
                        snapshot,
                        index: Cell::new(0),
                        kind,
                    },
                )
                .into())
            },
            context,
        );
    }
    if let Ok(entries_fn) = proto.get(js_string!("entries"), context) {
        let _ = def_prop(
            &proto,
            JsSymbol::iterator(),
            entries_fn,
            true,
            false,
            true,
            context,
        );
    }
    proto
}

fn new_headers_object(entries: Vec<(String, String)>, context: &mut Context) -> JsResult<JsObject> {
    let proto = state(context)
        .borrow()
        .fetch_xhr_prototypes
        .as_ref()
        .map(|p| p.headers.clone())
        .ok_or_else(|| failure("Headers prototype unavailable"))?;
    Ok(JsObject::from_proto_and_data(
        Some(proto),
        HeadersData {
            entries: Gc::new(GcRefCell::new(entries)),
        },
    ))
}

fn register_headers_global(headers_proto: JsObject, context: &mut Context) -> Result<(), String> {
    let ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, args, headers_proto, context| {
                modern::check_context(context)?;
                if new_target.is_undefined() {
                    return Err(JsNativeError::typ()
                        .with_message("Failed to construct 'Headers': Please use 'new'")
                        .into());
                }
                let entries = parse_headers_init(args.first(), context)?;
                Ok(JsObject::from_proto_and_data(
                    Some(headers_proto.clone()),
                    HeadersData {
                        entries: Gc::new(GcRefCell::new(entries)),
                    },
                )
                .into())
            },
            headers_proto.clone(),
        ),
    )
    .name(js_string!("Headers"))
    .length(0)
    .constructor(true)
    .build()
    .into();
    link_ctor_global(
        &context.global_object(),
        "Headers",
        &ctor,
        &headers_proto,
        context,
    )
}

// --- Shared Body Extraction & Consumption ---

fn extract_body_init(
    val: &JsValue,
    context: &mut Context,
) -> JsResult<(Vec<u8>, Option<&'static str>)> {
    if let Some(s) = val.as_string() {
        return Ok((
            s.to_std_string_escaped().into_bytes(),
            Some("text/plain;charset=UTF-8"),
        ));
    }
    if let Some(obj) = val.as_object() {
        if let Some(encoded) = web_apis::try_serialize_search_params(&obj) {
            return Ok((
                encoded.into_bytes(),
                Some("application/x-www-form-urlencoded;charset=UTF-8"),
            ));
        }
        if let Ok(bytes) = web_apis::extract_buffer_source_bytes(val, context) {
            return Ok((bytes, None));
        }
    }
    let s = utf8(val, context)?;
    Ok((s.into_bytes(), Some("text/plain;charset=UTF-8")))
}

fn settle_resolved(val: JsValue, context: &mut Context) -> JsResult<JsValue> {
    let (promise, resolvers) = JsPromise::new_pending(context);
    resolvers
        .resolve
        .call(&JsValue::undefined(), &[val], context)?;
    Ok(promise.into())
}

fn settle_rejected(err: JsError, context: &mut Context) -> JsResult<JsValue> {
    let (promise, resolvers) = JsPromise::new_pending(context);
    let reason = err.into_opaque(context)?;
    resolvers
        .reject
        .call(&JsValue::undefined(), &[reason], context)?;
    Ok(promise.into())
}

#[derive(Clone, Copy)]
enum BodyConsumeAs {
    Text,
    Json,
    ArrayBuffer,
    Bytes,
}

fn consume_body_promise(
    bytes_res: Result<Vec<u8>, &'static str>,
    mode: BodyConsumeAs,
    context: &mut Context,
) -> JsResult<JsValue> {
    let bytes = match bytes_res {
        Ok(b) => b,
        Err(msg) => {
            return settle_rejected(JsNativeError::typ().with_message(msg).into(), context);
        }
    };
    match mode {
        BodyConsumeAs::Text => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            settle_resolved(JsString::from(text).into(), context)
        }
        BodyConsumeAs::Json => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let json_obj = context
                .global_object()
                .get(js_string!("JSON"), context)?
                .as_object()
                .ok_or_else(|| failure("JSON global missing"))?;
            let parse_fn = json_obj
                .get(js_string!("parse"), context)?
                .as_callable()
                .ok_or_else(|| failure("JSON.parse missing"))?;
            match parse_fn.call(&json_obj.into(), &[JsString::from(text).into()], context) {
                Ok(parsed) => settle_resolved(parsed, context),
                Err(err) => settle_rejected(err, context),
            }
        }
        BodyConsumeAs::ArrayBuffer => {
            let ab = JsArrayBuffer::new(bytes.len(), context)?;
            if let Some(mut s) = ab.data_mut() {
                s.copy_from_slice(&bytes);
            }
            settle_resolved(ab.into(), context)
        }
        BodyConsumeAs::Bytes => {
            let u8a = JsUint8Array::from_iter(bytes, context)?;
            settle_resolved(u8a.into(), context)
        }
    }
}

// --- `Request` ---

fn take_request_body(this: &JsValue) -> JsResult<Result<Vec<u8>, &'static str>> {
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid Request receiver"))?;
    let rd = obj
        .downcast_ref::<RequestData>()
        .ok_or_else(|| failure("Invalid Request receiver"))?;
    if rd.body_used.get() {
        return Ok(Err("Body has already been consumed"));
    }
    if rd.body.borrow().is_some() {
        rd.body_used.set(true);
    }
    Ok(Ok(rd.body.borrow().clone().unwrap_or_default()))
}

fn build_request_proto(context: &mut Context) -> JsObject {
    let proto = ObjectInitializer::new(context).build();
    for prop in [
        "method",
        "url",
        "headers",
        "signal",
        "bodyUsed",
        "mode",
        "credentials",
        "redirect",
    ] {
        let _ = def_getter_cap(
            &proto,
            prop,
            true,
            prop,
            |this, _, &prop, context| {
                modern::check_context(context)?;
                let obj = this
                    .as_object()
                    .ok_or_else(|| failure("Invalid Request receiver"))?;
                let rd = obj
                    .downcast_ref::<RequestData>()
                    .ok_or_else(|| failure("Invalid Request receiver"))?;
                Ok(match prop {
                    "method" => JsString::from(rd.method.as_str()).into(),
                    "url" => JsString::from(rd.url.as_str()).into(),
                    "headers" => rd.headers.clone().into(),
                    "signal" => wrap_abort_signal(rd.sig_id, context)?,
                    "bodyUsed" => rd.body_used.get().into(),
                    "mode" | "credentials" => js_string!("same-origin").into(),
                    "redirect" => js_string!("follow").into(),
                    _ => JsValue::undefined(),
                })
            },
            context,
        );
    }
    for (name, mode) in [
        ("text", BodyConsumeAs::Text),
        ("json", BodyConsumeAs::Json),
        ("arrayBuffer", BodyConsumeAs::ArrayBuffer),
        ("bytes", BodyConsumeAs::Bytes),
    ] {
        let _ = def_cap(
            &proto,
            name,
            0,
            false,
            mode,
            |this, _, &mode, context| {
                modern::check_context(context)?;
                consume_body_promise(take_request_body(this)?, mode, context)
            },
            context,
        );
    }
    let _ = def_fn(
        &proto,
        "clone",
        0,
        false,
        |this, _, context| {
            modern::check_context(context)?;
            let obj = this
                .as_object()
                .ok_or_else(|| failure("Invalid Request receiver"))?;
            let rd = obj
                .downcast_ref::<RequestData>()
                .ok_or_else(|| failure("Invalid Request receiver"))?;
            if rd.body_used.get() {
                return Err(JsNativeError::typ()
                    .with_message("Cannot clone a Request whose body is already used")
                    .into());
            }
            let hdr_entries = headers_data(&rd.headers.clone().into())?.borrow().clone();
            let (method, url, sig_id, body) = (
                rd.method.clone(),
                rd.url.clone(),
                rd.sig_id,
                rd.body.borrow().clone(),
            );
            drop(rd);
            let new_hdrs = new_headers_object(hdr_entries, context)?;
            let proto = state(context)
                .borrow()
                .fetch_xhr_prototypes
                .as_ref()
                .map(|p| p.request.clone())
                .ok_or_else(|| failure("Request prototype unavailable"))?;
            Ok(JsObject::from_proto_and_data(
                Some(proto),
                RequestData {
                    method,
                    url,
                    headers: new_hdrs,
                    sig_id,
                    body: GcRefCell::new(body),
                    body_used: Cell::new(false),
                },
            )
            .into())
        },
        context,
    );
    proto
}

fn parse_request_parts(
    input: &JsValue,
    init_opt: Option<&JsValue>,
    context: &mut Context,
) -> JsResult<(
    String,
    url::Url,
    Vec<(String, String)>,
    Option<Vec<u8>>,
    usize,
)> {
    let mut method = "GET".to_owned();
    let mut headers_vec: Vec<(String, String)>;
    let mut body: Option<Vec<u8>> = None;
    let mut sig_id: Option<usize> = None;
    let resolved_url: url::Url;

    if let Some(in_obj) = input.as_object()
        && in_obj.is::<RequestData>()
    {
        let rd = in_obj.downcast_ref::<RequestData>().unwrap();
        if rd.body_used.get() && rd.body.borrow().is_some() {
            return Err(JsNativeError::typ()
                .with_message("Request body is already used")
                .into());
        }
        method = rd.method.clone();
        resolved_url = url::Url::parse(&rd.url)
            .map_err(|e| JsNativeError::typ().with_message(format!("Invalid URL: {e}")))?;
        headers_vec = headers_data(&rd.headers.clone().into())?.borrow().clone();
        body = rd.body.borrow().clone();
        if body.is_some() {
            rd.body_used.set(true);
        }
        sig_id = Some(rd.sig_id);
    } else if let Some(in_obj) = input.as_object()
        && let Some(href) = web_apis::try_url_href(&in_obj)
    {
        resolved_url = resolve_against_document(&href, context)?;
        headers_vec = Vec::new();
    } else {
        let raw_url = utf8(input, context)?;
        resolved_url = resolve_against_document(&raw_url, context)?;
        headers_vec = Vec::new();
    }

    if let Some(init_val) = init_opt
        && !init_val.is_undefined()
        && !init_val.is_null()
    {
        let Some(init_obj) = init_val.as_object() else {
            return Err(JsNativeError::typ()
                .with_message("RequestInit must be an object")
                .into());
        };
        let m_val = init_obj.get(js_string!("method"), context)?;
        if !m_val.is_undefined() {
            method = normalize_method(&utf8(&m_val, context)?)?;
        }
        let h_val = init_obj.get(js_string!("headers"), context)?;
        if !h_val.is_undefined() {
            headers_vec = parse_headers_init(Some(&h_val), context)?;
        }
        let b_val = init_obj.get(js_string!("body"), context)?;
        if !b_val.is_undefined() {
            if b_val.is_null() {
                body = None;
            } else {
                let (bytes, default_ct) = extract_body_init(&b_val, context)?;
                if let Some(ct) = default_ct
                    && !headers_vec.iter().any(|(k, _)| k == "content-type")
                    && headers_vec.len() < MAX_FETCH_HEADERS
                {
                    headers_vec.push(("content-type".into(), ct.into()));
                }
                body = Some(bytes);
            }
        }
        let s_val = init_obj.get(js_string!("signal"), context)?;
        if !s_val.is_undefined() && !s_val.is_null() {
            sig_id = Some(abort_signal_id(&s_val, context)?);
        }
    }

    if matches!(method.as_str(), "GET" | "HEAD") && body.is_some() {
        return Err(JsNativeError::typ()
            .with_message("Request with GET/HEAD method cannot have body")
            .into());
    }

    let final_sig = match sig_id {
        Some(sid) => sid,
        None => alloc_abort_signal(false, JsValue::undefined(), context)?,
    };

    Ok((method, resolved_url, headers_vec, body, final_sig))
}

fn register_request_global(request_proto: JsObject, context: &mut Context) -> Result<(), String> {
    let ctor = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, args, request_proto, context| {
                modern::check_context(context)?;
                if new_target.is_undefined() {
                    return Err(JsNativeError::typ()
                        .with_message("Failed to construct 'Request': Please use 'new'")
                        .into());
                }
                let Some(first) = args.first() else {
                    return Err(JsNativeError::typ()
                        .with_message("Failed to construct 'Request': 1 argument required")
                        .into());
                };
                let (method, url, headers_vec, body, sig_id) =
                    parse_request_parts(first, args.get(1), context)?;
                let headers = new_headers_object(headers_vec, context)?;
                Ok(JsObject::from_proto_and_data(
                    Some(request_proto.clone()),
                    RequestData {
                        method,
                        url: url.into(),
                        headers,
                        sig_id,
                        body: GcRefCell::new(body),
                        body_used: Cell::new(false),
                    },
                )
                .into())
            },
            request_proto.clone(),
        ),
    )
    .name(js_string!("Request"))
    .length(1)
    .constructor(true)
    .build();
    let ctor: JsObject = ctor.into();
    link_ctor_global(
        &context.global_object(),
        "Request",
        &ctor,
        &request_proto,
        context,
    )
}

// --- `Response` ---

fn make_response(
    status: u16,
    status_text: String,
    url: String,
    redirected: bool,
    rtype: &'static str,
    headers_vec: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    context: &mut Context,
) -> JsResult<JsValue> {
    let headers = new_headers_object(headers_vec, context)?;
    let proto = state(context)
        .borrow()
        .fetch_xhr_prototypes
        .as_ref()
        .map(|p| p.response.clone())
        .ok_or_else(|| failure("Response prototype unavailable"))?;
    Ok(JsObject::from_proto_and_data(
        Some(proto),
        ResponseData {
            status,
            status_text,
            url,
            redirected,
            rtype,
            headers,
            body: GcRefCell::new(body),
            body_used: Cell::new(false),
        },
    )
    .into())
}

fn take_response_body(this: &JsValue) -> JsResult<Result<Vec<u8>, &'static str>> {
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid Response receiver"))?;
    let rd = obj
        .downcast_ref::<ResponseData>()
        .ok_or_else(|| failure("Invalid Response receiver"))?;
    if rd.body_used.get() {
        return Ok(Err("Body has already been consumed"));
    }
    if rd.body.borrow().is_some() {
        rd.body_used.set(true);
    }
    Ok(Ok(rd.body.borrow().clone().unwrap_or_default()))
}

fn build_response_proto(context: &mut Context) -> JsObject {
    let proto = ObjectInitializer::new(context).build();
    for prop in [
        "status",
        "statusText",
        "ok",
        "redirected",
        "url",
        "type",
        "headers",
        "bodyUsed",
    ] {
        let _ = def_getter_cap(
            &proto,
            prop,
            true,
            prop,
            |this, _, &prop, context| {
                modern::check_context(context)?;
                let obj = this
                    .as_object()
                    .ok_or_else(|| failure("Invalid Response receiver"))?;
                let rd = obj
                    .downcast_ref::<ResponseData>()
                    .ok_or_else(|| failure("Invalid Response receiver"))?;
                Ok(match prop {
                    "status" => (rd.status as u32).into(),
                    "statusText" => JsString::from(rd.status_text.as_str()).into(),
                    "ok" => (200..=299).contains(&rd.status).into(),
                    "redirected" => rd.redirected.into(),
                    "url" => JsString::from(rd.url.as_str()).into(),
                    "type" => JsString::from(rd.rtype).into(),
                    "headers" => rd.headers.clone().into(),
                    "bodyUsed" => rd.body_used.get().into(),
                    _ => JsValue::undefined(),
                })
            },
            context,
        );
    }
    for (name, mode) in [
        ("text", BodyConsumeAs::Text),
        ("json", BodyConsumeAs::Json),
        ("arrayBuffer", BodyConsumeAs::ArrayBuffer),
        ("bytes", BodyConsumeAs::Bytes),
    ] {
        let _ = def_cap(
            &proto,
            name,
            0,
            false,
            mode,
            |this, _, &mode, context| {
                modern::check_context(context)?;
                consume_body_promise(take_response_body(this)?, mode, context)
            },
            context,
        );
    }
    let _ = def_fn(
        &proto,
        "clone",
        0,
        false,
        |this, _, context| {
            modern::check_context(context)?;
            let obj = this
                .as_object()
                .ok_or_else(|| failure("Invalid Response receiver"))?;
            let rd = obj
                .downcast_ref::<ResponseData>()
                .ok_or_else(|| failure("Invalid Response receiver"))?;
            if rd.body_used.get() {
                return Err(JsNativeError::typ()
                    .with_message("Cannot clone a Response whose body is already used")
                    .into());
            }
            let hdr_entries = headers_data(&rd.headers.clone().into())?.borrow().clone();
            let (status, status_text, url, redirected, rtype, body) = (
                rd.status,
                rd.status_text.clone(),
                rd.url.clone(),
                rd.redirected,
                rd.rtype,
                rd.body.borrow().clone(),
            );
            drop(rd);
            make_response(
                status,
                status_text,
                url,
                redirected,
                rtype,
                hdr_entries,
                body,
                context,
            )
        },
        context,
    );
    proto
}

fn parse_response_init(
    init_opt: Option<&JsValue>,
    context: &mut Context,
) -> JsResult<(u16, String, Vec<(String, String)>)> {
    let mut status: u16 = 200;
    let mut status_text = String::new();
    let mut headers_vec = Vec::new();
    if let Some(init_val) = init_opt
        && !init_val.is_undefined()
        && !init_val.is_null()
    {
        let Some(obj) = init_val.as_object() else {
            return Err(JsNativeError::typ()
                .with_message("ResponseInit must be an object")
                .into());
        };
        let s_val = obj.get(js_string!("status"), context)?;
        if !s_val.is_undefined() {
            let n = s_val.to_number(context)?;
            if !n.is_finite() || !(200.0..=599.0).contains(&n) {
                return Err(JsNativeError::range()
                    .with_message("Response status must be in the range 200-599")
                    .into());
            }
            status = n as u16;
        }
        let st_val = obj.get(js_string!("statusText"), context)?;
        if !st_val.is_undefined() {
            let raw = utf8(&st_val, context)?;
            if raw
                .chars()
                .any(|c| (c as u32) > 0xFF || matches!(c, '\r' | '\n' | '\0'))
            {
                return Err(JsNativeError::typ()
                    .with_message("Invalid Response statusText")
                    .into());
            }
            status_text = raw;
        }
        let h_val = obj.get(js_string!("headers"), context)?;
        if !h_val.is_undefined() {
            headers_vec = parse_headers_init(Some(&h_val), context)?;
        }
    }
    Ok((status, status_text, headers_vec))
}

fn register_response_global(response_proto: JsObject, context: &mut Context) -> Result<(), String> {
    let ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|new_target, args, context| {
            modern::check_context(context)?;
            if new_target.is_undefined() {
                return Err(JsNativeError::typ()
                    .with_message("Failed to construct 'Response': Please use 'new'")
                    .into());
            }
            let mut body = None;
            let mut default_ct = None;
            if let Some(first) = args.first()
                && !first.is_undefined()
                && !first.is_null()
            {
                let (bytes, ct) = extract_body_init(first, context)?;
                body = Some(bytes);
                default_ct = ct;
            }
            let (status, status_text, mut headers_vec) = parse_response_init(args.get(1), context)?;
            if matches!(status, 101 | 204 | 205 | 304) && body.is_some() {
                return Err(JsNativeError::typ()
                    .with_message("Response with null body status cannot have a body")
                    .into());
            }
            if let Some(ct) = default_ct
                && !headers_vec.iter().any(|(k, _)| k == "content-type")
                && headers_vec.len() < MAX_FETCH_HEADERS
            {
                headers_vec.push(("content-type".into(), ct.into()));
            }
            make_response(
                status,
                status_text,
                String::new(),
                false,
                "default",
                headers_vec,
                body,
                context,
            )
        }),
    )
    .name(js_string!("Response"))
    .length(0)
    .constructor(true)
    .build()
    .into();

    def_fn(
        &ctor,
        "json",
        1,
        true,
        |_, args, context| {
            modern::check_context(context)?;
            let data = args.first().cloned().unwrap_or_default();
            let json_obj = context
                .global_object()
                .get(js_string!("JSON"), context)?
                .as_object()
                .ok_or_else(|| failure("JSON global missing"))?;
            let stringify_fn = json_obj
                .get(js_string!("stringify"), context)?
                .as_callable()
                .ok_or_else(|| failure("JSON.stringify missing"))?;
            let serialized = stringify_fn.call(&json_obj.into(), &[data], context)?;
            let Some(json_str) = serialized.as_string() else {
                return Err(JsNativeError::typ()
                    .with_message("Value is not JSON serializable")
                    .into());
            };
            let (status, status_text, mut headers_vec) = parse_response_init(args.get(1), context)?;
            if matches!(status, 101 | 204 | 205 | 304) {
                return Err(JsNativeError::typ()
                    .with_message("Response.json status cannot be a null body status")
                    .into());
            }
            if !headers_vec.iter().any(|(k, _)| k == "content-type")
                && headers_vec.len() < MAX_FETCH_HEADERS
            {
                headers_vec.push(("content-type".into(), "application/json".into()));
            }
            make_response(
                status,
                status_text,
                String::new(),
                false,
                "default",
                headers_vec,
                Some(json_str.to_std_string_escaped().into_bytes()),
                context,
            )
        },
        context,
    )
    .map_err(|_| "Response.json bind failed")?;

    def_fn(
        &ctor,
        "error",
        0,
        true,
        |_, _, context| {
            modern::check_context(context)?;
            make_response(
                0,
                String::new(),
                String::new(),
                false,
                "error",
                Vec::new(),
                None,
                context,
            )
        },
        context,
    )
    .map_err(|_| "Response.error bind failed")?;

    def_fn(
        &ctor,
        "redirect",
        1,
        true,
        |_, args, context| {
            modern::check_context(context)?;
            let first = args.first().ok_or_else(|| {
                JsNativeError::typ().with_message("Response.redirect requires 1 argument")
            })?;
            let parsed = resolve_against_document(&utf8(first, context)?, context)?;
            let status = match args.get(1) {
                Some(v) if !v.is_undefined() => v.to_number(context)? as u16,
                _ => 302,
            };
            if !matches!(status, 301 | 302 | 303 | 307 | 308) {
                return Err(JsNativeError::range()
                    .with_message("Invalid redirect status code")
                    .into());
            }
            make_response(
                status,
                String::new(),
                String::new(),
                false,
                "default",
                vec![("location".into(), parsed.as_str().to_owned())],
                None,
                context,
            )
        },
        context,
    )
    .map_err(|_| "Response.redirect bind failed")?;

    link_ctor_global(
        &context.global_object(),
        "Response",
        &ctor,
        &response_proto,
        context,
    )
}

// --- `window.fetch` ---

fn invoke_window_fetch(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    match receiver(this, context) {
        Ok(Value::Null | Value::Undefined | Value::Object(0)) => {}
        _ => {
            return settle_rejected(
                JsNativeError::typ()
                    .with_message("Illegal invocation")
                    .into(),
                context,
            );
        }
    }
    let Some(first) = args.first() else {
        return settle_rejected(
            JsNativeError::typ()
                .with_message("Failed to execute 'fetch': 1 argument required")
                .into(),
            context,
        );
    };

    let (method, resolved_url, headers_vec, body_opt, sig_id) =
        match parse_request_parts(first, args.get(1), context) {
            Ok(parts) => parts,
            Err(err) => return settle_rejected(err, context),
        };

    // Check AbortSignal upfront
    {
        let shared = state(context);
        let data = shared.borrow();
        if let Some(sig) = data.abort_signals.get(sig_id)
            && sig.aborted
        {
            let reason = sig.reason.clone();
            drop(data);
            let (promise, resolvers) = JsPromise::new_pending(context);
            resolvers
                .reject
                .call(&JsValue::undefined(), &[reason], context)?;
            return Ok(promise.into());
        }
    }

    // Strict same-origin HTTP(S) enforcement
    let page_url = state(context).borrow().host.url.clone();
    if !is_same_http_origin(&page_url, &resolved_url) {
        return settle_rejected(
            JsNativeError::typ()
                .with_message("Failed to fetch: cross-origin requests are not permitted")
                .into(),
            context,
        );
    }

    let body_b64 = match body_opt {
        None => None,
        Some(bytes) => {
            let encoded = web_apis::base64_encode(&bytes);
            if encoded.len() > MAX_FETCH_REQ_BODY_B64_BYTES {
                return settle_rejected(
                    JsNativeError::typ()
                        .with_message("Failed to fetch: request body exceeds limit")
                        .into(),
                    context,
                );
            }
            Some(encoded)
        }
    };

    let (promise, resolvers) = JsPromise::new_pending(context);
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if data.fetch_requests.len() >= MAX_FETCH_REQUESTS_PER_TX
        || data.pending_fetches.len() >= MAX_PENDING_FETCHES
    {
        drop(data);
        return settle_rejected(
            JsNativeError::typ()
                .with_message("Failed to fetch: concurrent fetch limit exceeded")
                .into(),
            context,
        );
    }
    data.next_fetch_id = data.next_fetch_id.saturating_add(1);
    let req_id = data.next_fetch_id;
    let url_str = resolved_url.as_str().to_owned();
    data.fetch_requests.push(ScriptFetchRequest {
        request_id: req_id,
        method,
        url: url_str.clone(),
        headers: headers_vec,
        body_b64,
    });
    data.pending_fetches.push(PendingFetch {
        request_id: req_id,
        request_url: url_str,
        resolve: resolvers.resolve.into(),
        reject: resolvers.reject.into(),
        aborted: false,
    });
    if let Some(sig) = data.abort_signals.get_mut(sig_id) {
        sig.bound_fetches.push(req_id);
    }
    Ok(promise.into())
}

// --- `XMLHttpRequest` ---

fn attach_xhr_constants(target: &JsObject, context: &mut Context) -> Result<(), String> {
    for (name, val) in [
        ("UNSENT", 0u32),
        ("OPENED", 1),
        ("HEADERS_RECEIVED", 2),
        ("LOADING", 3),
        ("DONE", 4),
    ] {
        def_prop(
            target,
            JsString::from(name),
            val,
            false,
            true,
            false,
            context,
        )
        .map_err(|_| "XHR constant bind failed")?;
    }
    Ok(())
}

fn register_xhr_global(xhr_proto: JsObject, context: &mut Context) -> Result<(), String> {
    attach_xhr_constants(&xhr_proto, context)?;
    let ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, _, xhr_proto, context| {
                modern::check_context(context)?;
                if new_target.is_undefined() {
                    return Err(JsNativeError::typ()
                        .with_message("Failed to construct 'XMLHttpRequest': Please use 'new'")
                        .into());
                }
                let xhr_id = {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    if data.xhr_instances.len() >= MAX_XHR_INSTANCES {
                        return Err(failure("XMLHttpRequest instance limit exceeded"));
                    }
                    data.host.charge(256).map_err(failure)?;
                    let id = data.xhr_instances.len();
                    data.xhr_instances.push(XhrRecord::new());
                    id
                };
                wrap_with_proto(format!("xhr:{xhr_id}"), xhr_proto.clone(), context)
            },
            xhr_proto.clone(),
        ),
    )
    .name(js_string!("XMLHttpRequest"))
    .length(0)
    .constructor(true)
    .build()
    .into();
    attach_xhr_constants(&ctor, context)?;
    link_ctor_global(
        &context.global_object(),
        "XMLHttpRequest",
        &ctor,
        &xhr_proto,
        context,
    )
}

fn xhr_id_from_this(this: &JsValue, context: &mut Context) -> JsResult<usize> {
    if let Ok(Value::Host(handle)) = receiver(this, context)
        && let Some(xid) = handle
            .strip_prefix("xhr:")
            .and_then(|s| s.parse::<usize>().ok())
        && xid < state(context).borrow().xhr_instances.len()
    {
        return Ok(xid);
    }
    Err(JsNativeError::typ()
        .with_message("Illegal XMLHttpRequest invocation")
        .into())
}

fn fire_xhr_simple_event(xhr_id: usize, event_type: &str, context: &mut Context) -> JsResult<()> {
    let ts = dom_events::elapsed_ms(&state(context).borrow());
    let mevent_id = dom_events::alloc_modern_event(
        dom_events::ModernEvent::simple(event_type, false, false, true, ts),
        context,
    )?;
    let _ = xhr_dispatch_event(xhr_id, mevent_id, context)?;
    Ok(())
}

// --- EventTarget Support for `abortsignal:{id}` and `xhr:{id}` ---

fn vec_listener_call(
    name: &str,
    args: &[JsValue],
    context: &mut Context,
    get_vec: impl FnOnce(&mut super::State) -> Option<&mut Vec<SignalListener>>,
) -> JsResult<JsValue> {
    let event_type = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let value = args.get(1).cloned().unwrap_or_default();
    let is_add = !name.ends_with("removeEventListener");
    let (capture, once, passive) = parse_basic_options(args.get(2), context)?;
    if value.is_null() || value.is_undefined() {
        return Ok(JsValue::undefined());
    }
    let Some(callback) = value.as_object() else {
        return Err(failure(
            "Listener must be a function or EventListener object",
        ));
    };
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let max_listeners = data.max_listeners;
    let Some(listeners) = get_vec(&mut data) else {
        return Ok(JsValue::undefined());
    };
    let idx = listeners.iter().position(|l| {
        l.event_type == event_type && l.capture == capture && l.callback.as_ref() == Some(&callback)
    });
    if !is_add {
        if let Some(i) = idx {
            listeners[i].callback = None;
        }
    } else if idx.is_none() {
        if listeners.len() >= max_listeners {
            return Err(failure("Listener limit exceeded"));
        }
        listeners.push(SignalListener {
            event_type,
            callback: Some(callback),
            capture,
            once,
            passive,
        });
    }
    Ok(JsValue::undefined())
}

pub(super) fn signal_listener_call(
    name: &str,
    sig_id: usize,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    vec_listener_call(name, args, context, |d| {
        d.abort_signals.get_mut(sig_id).map(|s| &mut s.listeners)
    })
}

pub(super) fn xhr_listener_call(
    name: &str,
    xhr_id: usize,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    vec_listener_call(name, args, context, |d| {
        d.xhr_instances.get_mut(xhr_id).map(|x| &mut x.listeners)
    })
}

fn parse_basic_options(
    arg: Option<&JsValue>,
    context: &mut Context,
) -> JsResult<(bool, bool, bool)> {
    let Some(val) = arg else {
        return Ok((false, false, false));
    };
    if val.is_undefined() || val.is_null() {
        return Ok((false, false, false));
    }
    if let Some(b) = val.as_boolean() {
        return Ok((b, false, false));
    }
    if let Some(obj) = val.as_object() {
        return Ok((
            obj.get(js_string!("capture"), context)?.to_boolean(),
            obj.get(js_string!("once"), context)?.to_boolean(),
            obj.get(js_string!("passive"), context)?.to_boolean(),
        ));
    }
    Ok((val.to_boolean(), false, false))
}

fn dispatch_target_listeners(
    receiver_val: JsValue,
    mevent_id: usize,
    is_signal: bool,
    target_id: usize,
    context: &mut Context,
) -> JsResult<JsValue> {
    let (flavor, event_type, prop_cb, slots) = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let Some(ev) = data.modern_events.get_mut(mevent_id) else {
            return Err(failure("Invalid Event object"));
        };
        if ev.dispatching {
            drop(data);
            return Err(dom_exception::throw(
                "InvalidStateError",
                "The event is already being dispatched.",
                context,
            ));
        }
        ev.dispatching = true;
        ev.phase = 2;
        let flavor = ev.flavor;
        let event_type = ev.event_type.clone();
        let (prop_cb, listeners_opt) = if is_signal {
            let cb = if event_type == "abort" {
                data.abort_signals
                    .get(target_id)
                    .and_then(|s| s.onabort.clone())
            } else {
                None
            };
            (cb, data.abort_signals.get(target_id).map(|s| &s.listeners))
        } else {
            let cb = data
                .xhr_instances
                .get(target_id)
                .and_then(|x| match event_type.as_str() {
                    "readystatechange" => x.onreadystatechange.clone(),
                    "load" => x.onload.clone(),
                    "error" => x.onerror.clone(),
                    "abort" => x.onabort.clone(),
                    "loadstart" => x.onloadstart.clone(),
                    "loadend" => x.onloadend.clone(),
                    "timeout" => x.ontimeout.clone(),
                    _ => None,
                });
            (cb, data.xhr_instances.get(target_id).map(|x| &x.listeners))
        };
        let slots: Vec<usize> = listeners_opt
            .map(|ls| {
                ls.iter()
                    .enumerate()
                    .filter(|(_, l)| l.callback.is_some() && l.event_type == event_type)
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        (flavor, event_type, prop_cb, slots)
    };
    let ev_js = dom_events::wrap_modern_event(mevent_id, flavor, context)?;
    if let Some(cb) = prop_cb {
        let _ =
            dom_events::invoke_callback_object(&cb, &receiver_val, &ev_js, true, false, context);
    }
    for slot in slots {
        let (cb_opt, passive) = {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if data.modern_events[mevent_id].immediate {
                break;
            }
            let l = if is_signal {
                &mut data.abort_signals[target_id].listeners[slot]
            } else {
                &mut data.xhr_instances[target_id].listeners[slot]
            };
            if l.event_type != event_type {
                continue;
            }
            let passive = l.passive;
            let cb = if l.once {
                l.callback.take()
            } else {
                l.callback.clone()
            };
            (cb, passive)
        };
        if let Some(cb) = cb_opt {
            let _ = dom_events::invoke_callback_object(
                &cb,
                &receiver_val,
                &ev_js,
                false,
                passive,
                context,
            );
        }
    }
    let not_canceled = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let ev = &mut data.modern_events[mevent_id];
        ev.dispatching = false;
        ev.phase = 0;
        !ev.canceled
    };
    Ok(not_canceled.into())
}

pub(super) fn signal_dispatch_event(
    sig_id: usize,
    mevent_id: usize,
    context: &mut Context,
) -> JsResult<JsValue> {
    let receiver_val = wrap_abort_signal(sig_id, context)?;
    dispatch_target_listeners(receiver_val, mevent_id, true, sig_id, context)
}

pub(super) fn xhr_dispatch_event(
    xhr_id: usize,
    mevent_id: usize,
    context: &mut Context,
) -> JsResult<JsValue> {
    let proto = state(context)
        .borrow()
        .fetch_xhr_prototypes
        .as_ref()
        .map(|p| p.xhr.clone())
        .ok_or_else(|| failure("XMLHttpRequest prototype unavailable"))?;
    let receiver_val = wrap_with_proto(format!("xhr:{xhr_id}"), proto, context)?;
    dispatch_target_listeners(receiver_val, mevent_id, false, xhr_id, context)
}

// --- Proxy Property Access (`try_get`, `try_set`, `try_has`) & Method Dispatch (`call`) ---

pub(super) fn try_get(
    handle: &str,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<JsValue>> {
    if let Some(sig_id) = handle
        .strip_prefix("abortsignal:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        if let Some(m) = match name {
            "throwIfAborted" => Some(SIGNAL_THROW_IF_ABORTED),
            "addEventListener" => Some("dom.addEventListener"),
            "removeEventListener" => Some("dom.removeEventListener"),
            "dispatchEvent" => Some(dom_events::DISPATCH_EVENT),
            _ => None,
        } {
            return Ok(Some(super::method(m, context)?.into()));
        }
        let shared = state(context);
        let data = shared.borrow();
        let sig = data.abort_signals.get(sig_id);
        return Ok(match name {
            "aborted" => Some(sig.is_some_and(|s| s.aborted).into()),
            "reason" => Some(sig.map(|s| s.reason.clone()).unwrap_or_default()),
            "onabort" => Some(
                sig.and_then(|s| s.onabort.clone())
                    .map_or_else(JsValue::null, JsValue::from),
            ),
            _ => None,
        });
    }

    if let Some(xhr_id) = handle
        .strip_prefix("xhr:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        if let Some(c) = match name {
            "UNSENT" => Some(0u32),
            "OPENED" => Some(1),
            "HEADERS_RECEIVED" => Some(2),
            "LOADING" => Some(3),
            "DONE" => Some(4),
            _ => None,
        } {
            return Ok(Some(c.into()));
        }
        if let Some(m) = match name {
            "open" => Some(XHR_OPEN),
            "setRequestHeader" => Some(XHR_SET_REQUEST_HEADER),
            "send" => Some(XHR_SEND),
            "abort" => Some(XHR_ABORT),
            "getResponseHeader" => Some(XHR_GET_RESPONSE_HEADER),
            "getAllResponseHeaders" => Some(XHR_GET_ALL_RESPONSE_HEADERS),
            "overrideMimeType" => Some(XHR_OVERRIDE_MIME_TYPE),
            "addEventListener" => Some("dom.addEventListener"),
            "removeEventListener" => Some("dom.removeEventListener"),
            "dispatchEvent" => Some(dom_events::DISPATCH_EVENT),
            _ => None,
        } {
            return Ok(Some(super::method(m, context)?.into()));
        }
        if matches!(name, "responseText" | "response") {
            let (rt, rs, err, bytes) = {
                let shared = state(context);
                let data = shared.borrow();
                let Some(x) = data.xhr_instances.get(xhr_id) else {
                    return Ok(Some(js_string!("").into()));
                };
                (
                    x.response_type.clone(),
                    x.ready_state,
                    x.network_error,
                    x.response_bytes.clone(),
                )
            };
            if name == "responseText" && !rt.is_empty() && rt != "text" {
                return Err(dom_exception::throw(
                    "InvalidStateError",
                    "responseText is only available when responseType is '' or 'text'.",
                    context,
                ));
            }
            if rt.is_empty() || rt == "text" {
                if rs != 3 && rs != 4 || err {
                    return Ok(Some(js_string!("").into()));
                }
                return Ok(Some(
                    JsString::from(String::from_utf8_lossy(&bytes).into_owned()).into(),
                ));
            }
            if rs != 4 || err {
                return Ok(Some(JsValue::null()));
            }
            return match rt.as_str() {
                "arraybuffer" => {
                    let ab = JsArrayBuffer::new(bytes.len(), context)?;
                    if let Some(mut s) = ab.data_mut() {
                        s.copy_from_slice(&bytes);
                    }
                    Ok(Some(ab.into()))
                }
                "json" => {
                    if bytes.is_empty() {
                        return Ok(Some(JsValue::null()));
                    }
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    let json_obj = context
                        .global_object()
                        .get(js_string!("JSON"), context)?
                        .as_object()
                        .ok_or_else(|| failure("JSON global missing"))?;
                    let parse_fn = json_obj
                        .get(js_string!("parse"), context)?
                        .as_callable()
                        .ok_or_else(|| failure("JSON.parse missing"))?;
                    Ok(Some(
                        parse_fn
                            .call(&json_obj.into(), &[JsString::from(text).into()], context)
                            .unwrap_or(JsValue::null()),
                    ))
                }
                _ => Ok(Some(JsValue::null())),
            };
        }
        let shared = state(context);
        let data = shared.borrow();
        let x = data.xhr_instances.get(xhr_id);
        return Ok(match name {
            "readyState" => Some((x.map_or(0, |x| x.ready_state) as u32).into()),
            "status" => Some((x.map_or(0, |x| x.status) as u32).into()),
            "statusText" => {
                Some(JsString::from(x.map(|x| x.status_text.as_str()).unwrap_or("")).into())
            }
            "responseURL" => {
                Some(JsString::from(x.map(|x| x.response_url.as_str()).unwrap_or("")).into())
            }
            "responseType" => {
                Some(JsString::from(x.map(|x| x.response_type.as_str()).unwrap_or("")).into())
            }
            "timeout" => Some(x.map_or(0, |x| x.timeout_ms).into()),
            "withCredentials" => Some(x.is_some_and(|x| x.with_credentials).into()),
            "onreadystatechange" | "onload" | "onerror" | "onabort" | "onloadstart"
            | "onloadend" | "ontimeout" => Some(
                x.and_then(|x| match name {
                    "onreadystatechange" => x.onreadystatechange.clone(),
                    "onload" => x.onload.clone(),
                    "onerror" => x.onerror.clone(),
                    "onabort" => x.onabort.clone(),
                    "onloadstart" => x.onloadstart.clone(),
                    "onloadend" => x.onloadend.clone(),
                    _ => x.ontimeout.clone(),
                })
                .map_or_else(JsValue::null, JsValue::from),
            ),
            _ => None,
        });
    }

    Ok(None)
}

pub(super) fn try_set(
    handle: &str,
    name: &str,
    value: &JsValue,
    context: &mut Context,
) -> JsResult<Option<bool>> {
    if let Some(sig_id) = handle
        .strip_prefix("abortsignal:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        return match name {
            "onabort" => {
                let cb = value.as_callable();
                if let Some(sig) = state(context).borrow_mut().abort_signals.get_mut(sig_id) {
                    sig.onabort = cb;
                }
                Ok(Some(true))
            }
            "aborted" | "reason" => Ok(Some(true)),
            _ => Ok(None),
        };
    }

    if let Some(xhr_id) = handle
        .strip_prefix("xhr:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        return match name {
            "responseType" => {
                let rt = utf8(value, context)?;
                let rs = state(context)
                    .borrow()
                    .xhr_instances
                    .get(xhr_id)
                    .map_or(0, |x| x.ready_state);
                if rs == 3 || rs == 4 {
                    return Err(dom_exception::throw(
                        "InvalidStateError",
                        "Cannot set responseType in LOADING or DONE state.",
                        context,
                    ));
                }
                if matches!(rt.as_str(), "" | "text" | "json" | "arraybuffer")
                    && let Some(x) = state(context).borrow_mut().xhr_instances.get_mut(xhr_id)
                {
                    x.response_type = rt;
                }
                Ok(Some(true))
            }
            "timeout" => {
                let n = value.to_u32(context)?;
                if let Some(x) = state(context).borrow_mut().xhr_instances.get_mut(xhr_id) {
                    x.timeout_ms = n;
                }
                Ok(Some(true))
            }
            "withCredentials" => {
                let b = value.to_boolean();
                let rs = state(context)
                    .borrow()
                    .xhr_instances
                    .get(xhr_id)
                    .map_or(0, |x| x.ready_state);
                let send_flag = state(context)
                    .borrow()
                    .xhr_instances
                    .get(xhr_id)
                    .is_some_and(|x| x.send_flag);
                if (rs != 0 && rs != 1) || send_flag {
                    return Err(dom_exception::throw(
                        "InvalidStateError",
                        "withCredentials can only be set in UNSENT or unsent OPENED state.",
                        context,
                    ));
                }
                if let Some(x) = state(context).borrow_mut().xhr_instances.get_mut(xhr_id) {
                    x.with_credentials = b;
                }
                Ok(Some(true))
            }
            "onreadystatechange" | "onload" | "onerror" | "onabort" | "onloadstart"
            | "onloadend" | "ontimeout" => {
                let cb = value.as_callable();
                if let Some(x) = state(context).borrow_mut().xhr_instances.get_mut(xhr_id) {
                    match name {
                        "onreadystatechange" => x.onreadystatechange = cb,
                        "onload" => x.onload = cb,
                        "onerror" => x.onerror = cb,
                        "onabort" => x.onabort = cb,
                        "onloadstart" => x.onloadstart = cb,
                        "onloadend" => x.onloadend = cb,
                        "ontimeout" => x.ontimeout = cb,
                        _ => {}
                    }
                }
                Ok(Some(true))
            }
            "readyState" | "status" | "statusText" | "responseURL" | "responseText"
            | "response" => Ok(Some(true)),
            _ => Ok(None),
        };
    }

    Ok(None)
}

pub(super) fn try_has(handle: &str, name: &str) -> Option<bool> {
    if handle.starts_with("abortsignal:") {
        return Some(matches!(
            name,
            "aborted"
                | "reason"
                | "onabort"
                | "throwIfAborted"
                | "addEventListener"
                | "removeEventListener"
                | "dispatchEvent"
        ));
    }
    if handle.starts_with("xhr:") {
        return Some(matches!(
            name,
            "UNSENT"
                | "OPENED"
                | "HEADERS_RECEIVED"
                | "LOADING"
                | "DONE"
                | "readyState"
                | "status"
                | "statusText"
                | "responseURL"
                | "responseType"
                | "responseText"
                | "response"
                | "timeout"
                | "withCredentials"
                | "onreadystatechange"
                | "onload"
                | "onerror"
                | "onabort"
                | "onloadstart"
                | "onloadend"
                | "ontimeout"
                | "open"
                | "setRequestHeader"
                | "send"
                | "abort"
                | "getResponseHeader"
                | "getAllResponseHeaders"
                | "overrideMimeType"
                | "addEventListener"
                | "removeEventListener"
                | "dispatchEvent"
        ));
    }
    None
}

pub(super) fn call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    match name {
        WINDOW_FETCH => invoke_window_fetch(this, args, context),
        SIGNAL_THROW_IF_ABORTED => {
            let sig_id = abort_signal_id(this, context)?;
            let (aborted, reason) = {
                let shared = state(context);
                let data = shared.borrow();
                let sig = &data.abort_signals[sig_id];
                (sig.aborted, sig.reason.clone())
            };
            if aborted {
                Err(JsError::from_opaque(reason))
            } else {
                Ok(JsValue::undefined())
            }
        }
        XHR_OPEN => {
            let xhr_id = xhr_id_from_this(this, context)?;
            if args.len() < 2 {
                return Err(JsNativeError::typ()
                    .with_message("XMLHttpRequest.open requires at least 2 arguments")
                    .into());
            }
            let method = normalize_method(&utf8(&args[0], context)?).map_err(|_| {
                dom_exception::throw("SyntaxError", "Invalid XMLHttpRequest method.", context)
            })?;
            let raw_url = if let Some(obj) = args[1].as_object()
                && let Some(href) = web_apis::try_url_href(&obj)
            {
                href
            } else {
                utf8(&args[1], context)?
            };
            let is_async = match args.get(2) {
                Some(v) if !v.is_undefined() => v.to_boolean(),
                _ => true,
            };
            if !is_async {
                return Err(dom_exception::throw(
                    "InvalidAccessError",
                    "Synchronous XMLHttpRequest on the main thread is not supported.",
                    context,
                ));
            }
            let resolved = resolve_against_document(&raw_url, context).map_err(|_| {
                dom_exception::throw("SyntaxError", "Invalid XMLHttpRequest URL.", context)
            })?;
            let page_url = state(context).borrow().host.url.clone();
            let same_origin = is_same_http_origin(&page_url, &resolved);
            let prev_rs = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if let Some(old_req) = data.xhr_instances[xhr_id].active_request_id.take() {
                    data.fetch_requests.retain(|r| r.request_id != old_req);
                }
                let x = &mut data.xhr_instances[xhr_id];
                let prev = x.ready_state;
                x.method = method;
                x.request_url = resolved.into();
                x.same_origin = same_origin;
                x.send_flag = false;
                x.request_headers.clear();
                x.status = 0;
                x.status_text.clear();
                x.response_url.clear();
                x.response_headers.clear();
                x.response_bytes.clear();
                x.network_error = false;
                x.ready_state = 1;
                prev
            };
            if prev_rs != 1 {
                fire_xhr_simple_event(xhr_id, "readystatechange", context)?;
            }
            Ok(JsValue::undefined())
        }
        XHR_SET_REQUEST_HEADER => {
            let xhr_id = xhr_id_from_this(this, context)?;
            if args.len() < 2 {
                return Err(JsNativeError::typ()
                    .with_message("setRequestHeader requires 2 arguments")
                    .into());
            }
            let (rs, send_flag) = {
                let shared = state(context);
                let data = shared.borrow();
                let x = &data.xhr_instances[xhr_id];
                (x.ready_state, x.send_flag)
            };
            if rs != 1 || send_flag {
                return Err(dom_exception::throw(
                    "InvalidStateError",
                    "XMLHttpRequest must be OPENED and not sent.",
                    context,
                ));
            }
            let h_name = validate_header_name(&utf8(&args[0], context)?).map_err(|_| {
                dom_exception::throw("SyntaxError", "Invalid header name.", context)
            })?;
            let h_val = normalize_header_value(&utf8(&args[1], context)?).map_err(|_| {
                dom_exception::throw("SyntaxError", "Invalid header value.", context)
            })?;
            if is_forbidden_xhr_header(&h_name) {
                return Ok(JsValue::undefined());
            }
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let x = &mut data.xhr_instances[xhr_id];
            if let Some(existing) = x.request_headers.iter_mut().find(|(k, _)| k == &h_name) {
                existing.1.push_str(", ");
                existing.1.push_str(&h_val);
            } else if x.request_headers.len() < MAX_FETCH_HEADERS {
                x.request_headers.push((h_name, h_val));
            }
            Ok(JsValue::undefined())
        }
        XHR_SEND => {
            let xhr_id = xhr_id_from_this(this, context)?;
            let (rs, send_flag, method, req_url, same_origin) = {
                let shared = state(context);
                let data = shared.borrow();
                let x = &data.xhr_instances[xhr_id];
                (
                    x.ready_state,
                    x.send_flag,
                    x.method.clone(),
                    x.request_url.clone(),
                    x.same_origin,
                )
            };
            if rs != 1 || send_flag {
                return Err(dom_exception::throw(
                    "InvalidStateError",
                    "XMLHttpRequest must be OPENED and not already sent.",
                    context,
                ));
            }
            let mut body_b64: Option<String> = None;
            let mut auto_ct: Option<&'static str> = None;
            if !matches!(method.as_str(), "GET" | "HEAD")
                && let Some(first) = args.first()
                && !first.is_undefined()
                && !first.is_null()
            {
                let (bytes, ct) = extract_body_init(first, context)?;
                let encoded = web_apis::base64_encode(&bytes);
                if encoded.len() > MAX_FETCH_REQ_BODY_B64_BYTES {
                    return Err(dom_exception::throw(
                        "NetworkError",
                        "XMLHttpRequest body exceeds limit.",
                        context,
                    ));
                }
                body_b64 = Some(encoded);
                auto_ct = ct;
            }
            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let x = &mut data.xhr_instances[xhr_id];
                x.send_flag = true;
                if let Some(ct) = auto_ct
                    && !x.request_headers.iter().any(|(k, _)| k == "content-type")
                    && x.request_headers.len() < MAX_FETCH_HEADERS
                {
                    x.request_headers.push(("content-type".into(), ct.into()));
                }
            }

            fire_xhr_simple_event(xhr_id, "loadstart", context)?;

            if !same_origin {
                // Cross-origin XHR fails cleanly with network error (status = 0, DONE = 4)
                {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    let x = &mut data.xhr_instances[xhr_id];
                    x.send_flag = false;
                    x.network_error = true;
                    x.status = 0;
                    x.ready_state = 4;
                }
                fire_xhr_simple_event(xhr_id, "readystatechange", context)?;
                fire_xhr_simple_event(xhr_id, "error", context)?;
                fire_xhr_simple_event(xhr_id, "loadend", context)?;
                return Ok(JsValue::undefined());
            }

            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if data.fetch_requests.len() >= MAX_FETCH_REQUESTS_PER_TX {
                    let x = &mut data.xhr_instances[xhr_id];
                    x.send_flag = false;
                    x.network_error = true;
                    x.status = 0;
                    x.ready_state = 4;
                    drop(data);
                    fire_xhr_simple_event(xhr_id, "readystatechange", context)?;
                    fire_xhr_simple_event(xhr_id, "error", context)?;
                    fire_xhr_simple_event(xhr_id, "loadend", context)?;
                    return Ok(JsValue::undefined());
                }
                data.next_fetch_id = data.next_fetch_id.saturating_add(1);
                let req_id = data.next_fetch_id;
                let headers = data.xhr_instances[xhr_id].request_headers.clone();
                data.xhr_instances[xhr_id].active_request_id = Some(req_id);
                data.fetch_requests.push(ScriptFetchRequest {
                    request_id: req_id,
                    method,
                    url: req_url,
                    headers,
                    body_b64,
                });
            }
            Ok(JsValue::undefined())
        }
        XHR_ABORT => {
            let xhr_id = xhr_id_from_this(this, context)?;
            let was_active = {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if let Some(old_req) = data.xhr_instances[xhr_id].active_request_id.take() {
                    data.fetch_requests.retain(|r| r.request_id != old_req);
                }
                let x = &mut data.xhr_instances[xhr_id];
                let active =
                    (x.ready_state == 1 && x.send_flag) || x.ready_state == 2 || x.ready_state == 3;
                if active {
                    x.send_flag = false;
                    x.network_error = true;
                    x.status = 0;
                    x.status_text.clear();
                    x.response_headers.clear();
                    x.response_bytes.clear();
                    x.ready_state = 4;
                }
                if x.ready_state == 4 {
                    x.ready_state = 0;
                }
                active
            };
            if was_active {
                {
                    let shared = state(context);
                    shared.borrow_mut().xhr_instances[xhr_id].ready_state = 4;
                }
                fire_xhr_simple_event(xhr_id, "readystatechange", context)?;
                fire_xhr_simple_event(xhr_id, "abort", context)?;
                fire_xhr_simple_event(xhr_id, "loadend", context)?;
                {
                    let shared = state(context);
                    shared.borrow_mut().xhr_instances[xhr_id].ready_state = 0;
                }
            }
            Ok(JsValue::undefined())
        }
        XHR_GET_RESPONSE_HEADER => {
            let xhr_id = xhr_id_from_this(this, context)?;
            let Some(first) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("getResponseHeader requires 1 argument")
                    .into());
            };
            let query = utf8(first, context)?.to_ascii_lowercase();
            let shared = state(context);
            let data = shared.borrow();
            let x = &data.xhr_instances[xhr_id];
            if x.ready_state < 2 || x.network_error {
                return Ok(JsValue::null());
            }
            let vals: Vec<&str> = x
                .response_headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case(&query))
                .map(|(_, v)| v.as_str())
                .collect();
            if vals.is_empty() {
                Ok(JsValue::null())
            } else {
                Ok(JsString::from(vals.join(", ")).into())
            }
        }
        XHR_GET_ALL_RESPONSE_HEADERS => {
            let xhr_id = xhr_id_from_this(this, context)?;
            let shared = state(context);
            let data = shared.borrow();
            let x = &data.xhr_instances[xhr_id];
            if x.ready_state < 2 || x.network_error {
                return Ok(js_string!("").into());
            }
            let mut normalized: Vec<(String, String)> = x
                .response_headers
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
                .collect();
            let snap = headers_sorted_snapshot(&mut normalized);
            let mut out = String::new();
            for (k, v) in snap {
                out.push_str(&k);
                out.push_str(": ");
                out.push_str(&v);
                out.push_str("\r\n");
            }
            Ok(JsString::from(out).into())
        }
        XHR_OVERRIDE_MIME_TYPE => {
            let xhr_id = xhr_id_from_this(this, context)?;
            let Some(first) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("overrideMimeType requires 1 argument")
                    .into());
            };
            let mime = utf8(first, context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let x = &mut data.xhr_instances[xhr_id];
            if x.ready_state == 3 || x.ready_state == 4 {
                drop(data);
                return Err(dom_exception::throw(
                    "InvalidStateError",
                    "Cannot call overrideMimeType in LOADING or DONE state.",
                    context,
                ));
            }
            x.override_mime = Some(mime);
            Ok(JsValue::undefined())
        }
        _ => Err(failure("Unsupported fetch/XHR operation")),
    }
}

// --- Parent `FetchComplete` Delivery ---

pub(super) fn complete_fetch(
    realm: &mut BoaPageRealm,
    response: ScriptFetchResponse,
    errors: &mut Vec<String>,
) {
    if realm.closed || realm.engine.is_fatal() {
        return;
    }

    if let Some(updated_cookies) = &response.updated_cookies {
        realm.state.borrow_mut().cookie_entries = web_apis::parse_initial_cookies(updated_cookies);
    }

    // 1. Check `pending_fetches`
    let pf_opt = {
        let mut data = realm.state.borrow_mut();
        data.pending_fetches
            .iter()
            .position(|pf| pf.request_id == response.request_id)
            .map(|idx| data.pending_fetches.remove(idx))
    };

    if let Some(pf) = pf_opt {
        if pf.aborted {
            return;
        }
        let step_res = {
            let _ingress = super::Ingress::enter(realm.state.clone());
            let context = realm.engine.context_mut();
            (|| -> JsResult<(JsObject, JsValue)> {
                modern::check_context(context)?;
                if !response.ok_transport {
                    let msg = response
                        .error
                        .as_deref()
                        .unwrap_or("NetworkError when attempting to fetch resource");
                    let err: JsError = JsNativeError::typ()
                        .with_message(format!("Failed to fetch: {msg}"))
                        .into();
                    let reason = err.into_opaque(context)?;
                    let fn_obj: JsObject = pf.reject.clone();
                    return Ok((fn_obj, reason));
                }
                let body_bytes =
                    web_apis::base64_decode_forgiving(&response.body_b64).map_err(failure)?;
                let normalized_headers: Vec<(String, String)> = response
                    .headers
                    .iter()
                    .filter_map(|(k, v)| {
                        let lk = k.to_ascii_lowercase();
                        if lk == "set-cookie" || lk == "set-cookie2" {
                            None
                        } else {
                            Some((lk, v.clone()))
                        }
                    })
                    .collect();
                let redirected =
                    !response.final_url.is_empty() && response.final_url != pf.request_url;
                let resp_val = make_response(
                    response.status,
                    response.status_text.clone(),
                    response.final_url.clone(),
                    redirected,
                    "basic",
                    normalized_headers,
                    Some(body_bytes),
                    context,
                )?;
                Ok((pf.resolve.clone(), resp_val))
            })()
        };
        match step_res {
            Ok((fn_obj, arg)) => {
                if let Err(failure) = realm.call_function(&fn_obj, &JsValue::undefined(), &[arg]) {
                    realm.execution_failure("Fetch settle", failure, errors);
                }
            }
            Err(err) => {
                if !realm.engine.is_fatal() && errors.len() < 64 {
                    errors.push(super::bounded(&err.to_string(), 512));
                }
            }
        }
        realm.checkpoint(errors);
        dom_events::flush_async_errors(realm, errors);
        return;
    }

    // 2. Check `xhr_instances`
    let xhr_idx = {
        let mut data = realm.state.borrow_mut();
        data.xhr_instances
            .iter_mut()
            .position(|x| x.active_request_id == Some(response.request_id))
            .inspect(|&idx| {
                data.xhr_instances[idx].active_request_id = None;
            })
    };

    if let Some(xhr_id) = xhr_idx {
        realm.state.borrow_mut().pending_xhr_delivery = Some((xhr_id, response));
        let runner = FunctionObjectBuilder::new(
            realm.engine.context().realm(),
            NativeFunction::from_fn_ptr(|_, _, context| {
                modern::check_context(context)?;
                let Some((xhr_id, response)) =
                    state(context).borrow_mut().pending_xhr_delivery.take()
                else {
                    return Ok(JsValue::undefined());
                };
                if !response.ok_transport {
                    {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        let x = &mut data.xhr_instances[xhr_id];
                        x.send_flag = false;
                        x.network_error = true;
                        x.status = 0;
                        x.status_text.clear();
                        x.ready_state = 4;
                    }
                    fire_xhr_simple_event(xhr_id, "readystatechange", context)?;
                    fire_xhr_simple_event(xhr_id, "error", context)?;
                    fire_xhr_simple_event(xhr_id, "loadend", context)?;
                    return Ok(JsValue::undefined());
                }

                let body_bytes =
                    web_apis::base64_decode_forgiving(&response.body_b64).map_err(failure)?;
                let exposed_headers: Vec<(String, String)> = response
                    .headers
                    .iter()
                    .filter_map(|(k, v)| {
                        let lk = k.to_ascii_lowercase();
                        if lk == "set-cookie" || lk == "set-cookie2" {
                            None
                        } else {
                            Some((lk, v.clone()))
                        }
                    })
                    .collect();

                {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    let x = &mut data.xhr_instances[xhr_id];
                    x.status = response.status;
                    x.status_text = response.status_text.clone();
                    x.response_url = response.final_url.clone();
                    x.response_headers = exposed_headers;
                    x.ready_state = 2;
                }
                fire_xhr_simple_event(xhr_id, "readystatechange", context)?;

                if state(context).borrow().xhr_instances[xhr_id].ready_state == 2 {
                    {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        let x = &mut data.xhr_instances[xhr_id];
                        x.response_bytes = body_bytes;
                        x.ready_state = 3;
                    }
                    fire_xhr_simple_event(xhr_id, "readystatechange", context)?;
                }

                if state(context).borrow().xhr_instances[xhr_id].ready_state == 3 {
                    {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        let x = &mut data.xhr_instances[xhr_id];
                        x.send_flag = false;
                        x.ready_state = 4;
                    }
                    fire_xhr_simple_event(xhr_id, "readystatechange", context)?;
                    fire_xhr_simple_event(xhr_id, "load", context)?;
                    fire_xhr_simple_event(xhr_id, "loadend", context)?;
                }
                Ok(JsValue::undefined())
            }),
        )
        .build();
        if let Err(failure) = realm.call_function(&runner, &JsValue::undefined(), &[]) {
            realm.execution_failure("XMLHttpRequest completion", failure, errors);
        }
        realm.checkpoint(errors);
        dom_events::flush_async_errors(realm, errors);
    }
}
