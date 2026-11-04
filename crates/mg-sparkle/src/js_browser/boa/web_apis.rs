//! Modern Web Platform APIs for Boa realms:
//! - `URL` (`URL.canParse`) & `URLSearchParams`
//! - `TextEncoder` & `TextDecoder` (`encoding_rs`)
//! - `atob` & `btoa` (RFC 4648 + `InvalidCharacterError`)
//! - `crypto.getRandomValues` & `crypto.randomUUID` (pure-Rust RFC 8439 ChaCha20)
//! - `performance.now()` & `performance.timeOrigin`
//! - `setInterval` / `clearInterval`, `requestAnimationFrame` / `cancelAnimationFrame`, `queueMicrotask`
//! - `structuredClone` (`CloneMode::StructuredClone`)
//! - `window.matchMedia(query)`
//! - `window.localStorage`, `window.sessionStorage`, `document.cookie`, `window.history`
use super::{
    State, def_cap, def_fn, def_get_set_cap, def_getter_cap, def_getter_fn, def_prop,
    dom_exception, failure, key, link_ctor_global, message_clone, modern, property_name, receiver,
    state, utf8, wrap,
};
use crate::page_session::platform::{
    HistoryOp, MAX_COOKIE_WRITE_BYTES, MAX_COOKIE_WRITES_PER_TX, MAX_HISTORY_OPS_PER_TX,
    MAX_HISTORY_STATE_BYTES, MAX_HISTORY_URL_BYTES, MAX_STORAGE_OPS_PER_TX,
    MAX_STORAGE_QUOTA_BYTES, StorageOp,
};
use mg_butane::runtime::Value;
use modern::boa_engine::{
    Context, JsData, JsNativeError, JsResult, JsString, JsSymbol, JsValue, NativeFunction,
    job::{GenericJob, Job},
    js_string,
    object::{
        FunctionObjectBuilder, JsObject, ObjectInitializer,
        builtins::{JsArray, JsArrayBuffer, JsDataView, JsTypedArray, JsUint8Array},
    },
    property::{Attribute, PropertyKey},
};
use modern::boa_gc::{Finalize, GcRefCell, Trace};
use std::{cell::RefCell, rc::Rc};

// --- Pure-Rust RFC 8439 ChaCha20 CSPRNG ---

#[derive(Clone)]
pub(super) struct ChaCha20Rng {
    key: [u32; 8],
    nonce: [u32; 3],
    counter: u64,
    buf: [u8; 64],
    pos: usize,
}

impl ChaCha20Rng {
    pub(super) fn from_seed(seed: [u8; 32]) -> Self {
        let mut key = [0u32; 8];
        for (i, slot) in key.iter_mut().enumerate() {
            let off = i * 4;
            *slot = u32::from_le_bytes([seed[off], seed[off + 1], seed[off + 2], seed[off + 3]]);
        }
        Self {
            key,
            nonce: [0x6d67_6272, 0x6f77_7365, 0x725f_6d32],
            counter: 0,
            buf: [0u8; 64],
            pos: 64,
        }
    }

    fn refill(&mut self) {
        let mut x = [
            0x6170_7865,
            0x3320_646e,
            0x7962_2d32,
            0x6b20_6574,
            self.key[0],
            self.key[1],
            self.key[2],
            self.key[3],
            self.key[4],
            self.key[5],
            self.key[6],
            self.key[7],
            self.counter as u32,
            (self.counter >> 32) as u32 ^ self.nonce[0],
            self.nonce[1],
            self.nonce[2],
        ];
        self.counter = self.counter.wrapping_add(1);
        let initial = x;
        for _ in 0..10 {
            quarter_round(&mut x, 0, 4, 8, 12);
            quarter_round(&mut x, 1, 5, 9, 13);
            quarter_round(&mut x, 2, 6, 10, 14);
            quarter_round(&mut x, 3, 7, 11, 15);
            quarter_round(&mut x, 0, 5, 10, 15);
            quarter_round(&mut x, 1, 6, 11, 12);
            quarter_round(&mut x, 2, 7, 8, 13);
            quarter_round(&mut x, 3, 4, 9, 14);
        }
        for (i, word) in x.iter().enumerate() {
            let bytes = word.wrapping_add(initial[i]).to_le_bytes();
            let off = i * 4;
            self.buf[off..off + 4].copy_from_slice(&bytes);
        }
        self.pos = 0;
    }

    pub(super) fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut written = 0;
        while written < dest.len() {
            if self.pos >= 64 {
                self.refill();
            }
            let avail = 64 - self.pos;
            let take = avail.min(dest.len() - written);
            dest[written..written + take].copy_from_slice(&self.buf[self.pos..self.pos + take]);
            self.pos += take;
            written += take;
        }
    }
}

#[inline]
fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

// --- RFC 4648 Base64 Helpers (re-exported from `page_session::platform`) ---

pub use crate::page_session::platform::{base64_decode_forgiving, base64_encode};

// --- Method Constants ---

pub(super) const STORAGE_GET_ITEM: &str = "host.m2.storage.getItem";
pub(super) const STORAGE_SET_ITEM: &str = "host.m2.storage.setItem";
pub(super) const STORAGE_REMOVE_ITEM: &str = "host.m2.storage.removeItem";
pub(super) const STORAGE_CLEAR: &str = "host.m2.storage.clear";
pub(super) const STORAGE_KEY: &str = "host.m2.storage.key";

pub(super) const HISTORY_PUSH_STATE: &str = "host.m2.history.pushState";
pub(super) const HISTORY_REPLACE_STATE: &str = "host.m2.history.replaceState";
pub(super) const HISTORY_BACK: &str = "host.m2.history.back";
pub(super) const HISTORY_FORWARD: &str = "host.m2.history.forward";
pub(super) const HISTORY_GO: &str = "host.m2.history.go";
pub(super) const NAVIGATOR_SEND_BEACON: &str = "host.navigator.sendBeacon";

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        "host.window.setInterval" => Some(("setInterval", 1)),
        "host.window.clearInterval" => Some(("clearInterval", 0)),
        "host.window.requestAnimationFrame" => Some(("requestAnimationFrame", 1)),
        "host.window.cancelAnimationFrame" => Some(("cancelAnimationFrame", 1)),
        "host.window.queueMicrotask" => Some(("queueMicrotask", 1)),
        "host.window.structuredClone" => Some(("structuredClone", 1)),
        "host.window.atob" => Some(("atob", 1)),
        "host.window.btoa" => Some(("btoa", 1)),
        "host.window.matchMedia" => Some(("matchMedia", 1)),
        STORAGE_GET_ITEM => Some(("getItem", 1)),
        STORAGE_SET_ITEM => Some(("setItem", 2)),
        STORAGE_REMOVE_ITEM => Some(("removeItem", 1)),
        STORAGE_CLEAR => Some(("clear", 0)),
        STORAGE_KEY => Some(("key", 1)),
        HISTORY_PUSH_STATE => Some(("pushState", 2)),
        HISTORY_REPLACE_STATE => Some(("replaceState", 2)),
        HISTORY_BACK => Some(("back", 0)),
        HISTORY_FORWARD => Some(("forward", 0)),
        HISTORY_GO => Some(("go", 0)),
        NAVIGATOR_SEND_BEACON => Some(("sendBeacon", 1)),
        _ => None,
    }
}

// --- Native JsData Types for `URL`, `URLSearchParams`, `TextEncoder`, `TextDecoder` ---

#[derive(Trace, Finalize, JsData)]
struct SharedUrlData {
    #[unsafe_ignore_trace]
    inner: Rc<RefCell<url::Url>>,
    params_obj: GcRefCell<Option<JsObject>>,
}

#[derive(Trace, Finalize, JsData)]
struct SearchParamsData {
    #[unsafe_ignore_trace]
    pairs: Rc<RefCell<Vec<(String, String)>>>,
    #[unsafe_ignore_trace]
    linked_url: Option<Rc<RefCell<url::Url>>>,
}

impl SearchParamsData {
    fn sync_from_url(&self) {
        if let Some(u) = &self.linked_url {
            let parsed: Vec<(String, String)> = u
                .borrow()
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
            *self.pairs.borrow_mut() = parsed;
        }
    }

    fn sync_to_url(&self) {
        if let Some(u) = &self.linked_url {
            let pairs = self.pairs.borrow();
            let mut u_mut = u.borrow_mut();
            if pairs.is_empty() {
                u_mut.set_query(None);
            } else {
                let encoded = serialize_query_pairs(&pairs);
                u_mut.set_query(Some(&encoded));
            }
        }
    }
}

fn serialize_query_pairs(pairs: &[(String, String)]) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        ser.append_pair(k, v);
    }
    ser.finish()
}

pub(super) fn try_serialize_search_params(obj: &JsObject) -> Option<String> {
    let spd = obj.downcast_ref::<SearchParamsData>()?;
    spd.sync_from_url();
    Some(serialize_query_pairs(&spd.pairs.borrow()))
}

pub(super) fn try_url_href(obj: &JsObject) -> Option<String> {
    let ud = obj.downcast_ref::<SharedUrlData>()?;
    Some(ud.inner.borrow().as_str().to_owned())
}

#[derive(Trace, Finalize, JsData)]
struct TextEncoderData;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DecoderEncoding {
    Utf8,
    Windows1252,
    Utf16Le,
    Utf16Be,
}

impl DecoderEncoding {
    fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" | "unicode-1-1-utf-8" => Some(Self::Utf8),
            "windows-1252" | "cp1252" | "iso-8859-1" | "iso8859-1" | "iso_8859-1" | "latin1"
            | "l1" | "ascii" | "us-ascii" | "x-cp1252" => Some(Self::Windows1252),
            "utf-16le" | "utf-16" | "ucs-2" | "unicode" => Some(Self::Utf16Le),
            "utf-16be" | "unicodefffe" => Some(Self::Utf16Be),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Windows1252 => "windows-1252",
            Self::Utf16Le => "utf-16le",
            Self::Utf16Be => "utf-16be",
        }
    }

    fn decode(self, bytes: &[u8], fatal: bool, ignore_bom: bool) -> Result<String, ()> {
        match self {
            Self::Utf8 => {
                let slice = if !ignore_bom && bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                    &bytes[3..]
                } else {
                    bytes
                };
                if fatal {
                    std::str::from_utf8(slice)
                        .map(str::to_owned)
                        .map_err(|_| ())
                } else {
                    Ok(String::from_utf8_lossy(slice).into_owned())
                }
            }
            Self::Windows1252 => {
                const CP1252_80_9F: [char; 32] = [
                    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}',
                    '\u{2020}', '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}',
                    '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}', '\u{0090}', '\u{2018}',
                    '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
                    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}',
                    '\u{017E}', '\u{0178}',
                ];
                let mut out = String::with_capacity(bytes.len());
                for &b in bytes {
                    if (0x80..=0x9F).contains(&b) {
                        out.push(CP1252_80_9F[(b - 0x80) as usize]);
                    } else {
                        out.push(b as char);
                    }
                }
                Ok(out)
            }
            Self::Utf16Le | Self::Utf16Be => {
                let le = self == Self::Utf16Le;
                let mut slice = bytes;
                if !ignore_bom && slice.len() >= 2 {
                    let bom = if le { [0xFF, 0xFE] } else { [0xFE, 0xFF] };
                    if slice.starts_with(&bom) {
                        slice = &slice[2..];
                    }
                }
                if slice.len() % 2 != 0 && fatal {
                    return Err(());
                }
                let mut units = Vec::with_capacity((slice.len() + 1) / 2);
                let mut chunks = slice.chunks_exact(2);
                for c in &mut chunks {
                    let u = if le {
                        u16::from_le_bytes([c[0], c[1]])
                    } else {
                        u16::from_be_bytes([c[0], c[1]])
                    };
                    units.push(u);
                }
                let mut out = String::with_capacity(units.len());
                for r in char::decode_utf16(units) {
                    match r {
                        Ok(ch) => out.push(ch),
                        Err(_) if fatal => return Err(()),
                        Err(_) => out.push('\u{FFFD}'),
                    }
                }
                if !chunks.remainder().is_empty() {
                    out.push('\u{FFFD}');
                }
                Ok(out)
            }
        }
    }
}

#[derive(Trace, Finalize, JsData)]
struct TextDecoderData {
    #[unsafe_ignore_trace]
    encoding: DecoderEncoding,
    fatal: bool,
    ignore_bom: bool,
}

// --- Global Registration ---

pub(super) fn register(context: &mut Context) -> Result<(), String> {
    register_annex_b_compat(context)?;
    register_url_and_params(context)?;
    register_text_encoding(context)?;
    register_crypto(context)?;
    register_performance(context)?;
    register_window_globals(context)?;
    register_storage_and_history(context)?;
    Ok(())
}

fn register_annex_b_compat(context: &mut Context) -> Result<(), String> {
    let str_proto = context.intrinsics().constructors().string().prototype();
    def_fn(&str_proto, "substr", 2, false, string_proto_substr, context)
        .map_err(|_| "String.prototype.substr bind failed")?;
    for (alias, canonical) in [("trimLeft", "trimStart"), ("trimRight", "trimEnd")] {
        if let Ok(fn_val) = str_proto.get(JsString::from(canonical), context)
            && fn_val.is_callable()
        {
            def_prop(
                &str_proto,
                JsString::from(alias),
                fn_val,
                true,
                false,
                true,
                context,
            )
            .map_err(|_| "String.prototype trim alias bind failed")?;
        }
    }
    let date_proto = context.intrinsics().constructors().date().prototype();
    if let Ok(utc_fn) = date_proto.get(js_string!("toUTCString"), context)
        && utc_fn.is_callable()
    {
        def_prop(
            &date_proto,
            js_string!("toGMTString"),
            utc_fn,
            true,
            false,
            true,
            context,
        )
        .map_err(|_| "Date.prototype.toGMTString bind failed")?;
    }
    Ok(())
}

fn string_proto_substr(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    if this.is_null_or_undefined() {
        return Err(JsNativeError::typ()
            .with_message("String.prototype.substr called on null or undefined")
            .into());
    }
    let s = this.to_string(context)?;
    let len_usize = s.len();
    let size = len_usize as f64;
    let raw_start = match args.first() {
        None => 0.0,
        Some(v) if v.is_undefined() => 0.0,
        Some(v) => v.to_number(context)?,
    };
    if raw_start == f64::INFINITY {
        return Ok(js_string!("").into());
    }
    let st = if raw_start.is_nan() || raw_start == 0.0 || raw_start == f64::NEG_INFINITY {
        0usize
    } else {
        let trunc = raw_start.trunc();
        if trunc < 0.0 {
            ((size + trunc).max(0.0) as usize).min(len_usize)
        } else {
            (trunc as usize).min(len_usize)
        }
    };
    let end = match args.get(1) {
        None => len_usize,
        Some(v) if v.is_undefined() => len_usize,
        Some(v) => {
            let n = v.to_number(context)?;
            if n.is_nan() || n <= 0.0 {
                0usize
            } else if n == f64::INFINITY {
                len_usize
            } else {
                st.saturating_add(n.trunc() as usize).min(len_usize)
            }
        }
    };
    if end <= st {
        return Ok(js_string!("").into());
    }
    let units: Vec<u16> = s.iter().skip(st).take(end - st).collect();
    Ok(JsString::from(units.as_slice()).into())
}

fn register_window_globals(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();
    for name in [
        "setInterval",
        "clearInterval",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "queueMicrotask",
        "structuredClone",
        "atob",
        "btoa",
        "matchMedia",
    ] {
        let fn_obj = super::method(&format!("host.window.{name}"), context)
            .map_err(|_| "Window Web API method failed")?;
        def_prop(
            &global,
            JsString::from(name),
            fn_obj,
            true,
            true,
            true,
            context,
        )
        .map_err(|_| "Global Web API property failed")?;
    }
    Ok(())
}

fn register_storage_and_history(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();
    for (prop, handle) in [
        ("localStorage", "storage:local"),
        ("sessionStorage", "storage:session"),
    ] {
        def_getter_cap(
            &global,
            prop,
            true,
            handle,
            |_, _, handle, ctx| {
                modern::check_context(ctx)?;
                ensure_tuple_origin(ctx)?;
                wrap(handle, ctx)
            },
            context,
        )
        .map_err(|_| "Storage accessor failed")?;
    }

    let history_obj = wrap("history", context).map_err(|_| "History wrap failed")?;
    def_prop(
        &global,
        js_string!("history"),
        history_obj,
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "Global history failed".to_owned())
}

pub(super) fn ensure_tuple_origin(context: &mut Context) -> JsResult<()> {
    let ok = {
        let shared = state(context);
        let data = shared.borrow();
        matches!(data.host.url.scheme(), "http" | "https") && data.host.url.host_str().is_some()
    };
    if !ok {
        return Err(dom_exception::throw(
            "SecurityError",
            "Access is denied for an opaque origin.",
            context,
        ));
    }
    Ok(())
}

// --- `crypto` (`getRandomValues`, `randomUUID`) ---

fn register_crypto(context: &mut Context) -> Result<(), String> {
    let crypto_obj = JsObject::with_object_proto(context.intrinsics());
    def_fn(
        &crypto_obj,
        "getRandomValues",
        1,
        false,
        crypto_get_random_values,
        context,
    )
    .map_err(|_| "crypto.getRandomValues failed")?;
    def_fn(
        &crypto_obj,
        "randomUUID",
        0,
        false,
        crypto_random_uuid,
        context,
    )
    .map_err(|_| "crypto.randomUUID failed")?;
    def_prop(
        &context.global_object(),
        js_string!("crypto"),
        crypto_obj,
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "Global crypto failed".to_owned())
}

fn crypto_get_random_values(
    _: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let Some(arg) = args.first() else {
        return Err(JsNativeError::typ()
            .with_message("crypto.getRandomValues requires 1 argument")
            .into());
    };
    let Some(obj) = arg.as_object() else {
        return Err(dom_exception::throw(
            "TypeMismatchError",
            "Argument 1 of crypto.getRandomValues is not an integer TypedArray.",
            context,
        ));
    };
    let Ok(ta) = JsTypedArray::from_object(obj.clone()) else {
        return Err(dom_exception::throw(
            "TypeMismatchError",
            "Argument 1 of crypto.getRandomValues is not an integer TypedArray.",
            context,
        ));
    };
    // Reject Float32Array / Float64Array by checking constructor / toStringTag
    let tag_val = obj.get(JsSymbol::to_string_tag(), context)?;
    let tag_name = tag_val
        .as_string()
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default();
    if !matches!(
        tag_name.as_str(),
        "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "BigInt64Array"
            | "BigUint64Array"
    ) {
        return Err(dom_exception::throw(
            "TypeMismatchError",
            "Float TypedArrays are not supported by crypto.getRandomValues.",
            context,
        ));
    }
    let buf_val = ta.buffer(context)?;
    let Some(buf_obj) = buf_val.as_object() else {
        return Err(failure("Invalid TypedArray buffer"));
    };
    let ab = JsArrayBuffer::from_object(buf_obj)?;
    let byte_offset = ta.byte_offset(context)?;
    let byte_length = ta.byte_length(context)?;
    if byte_length > 65_536 {
        return Err(dom_exception::throw(
            "QuotaExceededError",
            "crypto.getRandomValues quota exceeded (maximum 65536 bytes).",
            context,
        ));
    }
    let mut data_guard = ab.data_mut();
    let Some(slice) = data_guard.as_deref_mut() else {
        return Err(dom_exception::throw(
            "InvalidStateError",
            "TypedArray buffer is detached.",
            context,
        ));
    };
    let end = byte_offset.saturating_add(byte_length);
    if end > slice.len() {
        return Err(failure("TypedArray bounds out of range"));
    }
    state(context)
        .borrow_mut()
        .csprng
        .fill_bytes(&mut slice[byte_offset..end]);
    drop(data_guard);
    Ok(arg.clone())
}

fn crypto_random_uuid(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let mut b = [0u8; 16];
    state(context).borrow_mut().csprng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let uuid = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    );
    Ok(JsString::from(uuid).into())
}

// --- `performance` (`now()`, `timeOrigin`, User Timing) ---

#[derive(Clone)]
struct PerfEntry {
    name: String,
    entry_type: &'static str,
    start_time: f64,
    duration: f64,
}

#[derive(Clone, Trace, Finalize)]
struct PerfStore(#[unsafe_ignore_trace] Rc<RefCell<Vec<PerfEntry>>>);

fn navigation_entry_to_js(context: &mut Context) -> JsObject {
    let (url, complete_ms) = {
        let shared = state(context);
        let data = shared.borrow();
        let ms = if data.host.ready == "complete" {
            super::dom_events::elapsed_ms(&data)
        } else {
            0.0
        };
        (data.host.url.as_str().to_owned(), ms)
    };
    let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    let mut init = ObjectInitializer::new(context);
    for (k, v) in [
        ("name", url.as_str()),
        ("entryType", "navigation"),
        ("initiatorType", "navigation"),
        ("nextHopProtocol", "h2"),
        ("type", "navigate"),
    ] {
        init.property(JsString::from(k), JsString::from(v), ec);
    }
    init.property(js_string!("redirectCount"), 0, ec);
    for zprop in [
        "startTime",
        "redirectStart",
        "redirectEnd",
        "unloadEventStart",
        "unloadEventEnd",
        "workerStart",
        "fetchStart",
        "domainLookupStart",
        "domainLookupEnd",
        "connectStart",
        "connectEnd",
        "secureConnectionStart",
        "requestStart",
        "responseStart",
        "responseEnd",
        "domInteractive",
        "domContentLoadedEventStart",
        "domContentLoadedEventEnd",
        "transferSize",
        "encodedBodySize",
        "decodedBodySize",
    ] {
        init.property(JsString::from(zprop), 0.0, ec);
    }
    for cprop in ["duration", "domComplete", "loadEventStart", "loadEventEnd"] {
        init.property(JsString::from(cprop), complete_ms, ec);
    }
    init.build()
}

fn perf_entry_to_js(entry: &PerfEntry, context: &mut Context) -> JsObject {
    let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    ObjectInitializer::new(context)
        .property(js_string!("name"), JsString::from(entry.name.as_str()), ec)
        .property(
            js_string!("entryType"),
            JsString::from(entry.entry_type),
            ec,
        )
        .property(js_string!("startTime"), entry.start_time, ec)
        .property(js_string!("duration"), entry.duration, ec)
        .build()
}

fn register_performance(context: &mut Context) -> Result<(), String> {
    let entries = PerfStore(Rc::new(RefCell::new(Vec::new())));
    let perf = JsObject::with_object_proto(context.intrinsics());
    def_fn(
        &perf,
        "now",
        0,
        false,
        |_, _, ctx| {
            modern::check_context(ctx)?;
            Ok(super::dom_events::elapsed_ms(&state(ctx).borrow()).into())
        },
        context,
    )
    .map_err(|_| "performance.now failed")?;
    def_fn(
        &perf,
        "toJSON",
        0,
        false,
        |_, _, ctx| {
            modern::check_context(ctx)?;
            let to = state(ctx).borrow().time_origin_ms;
            Ok(ObjectInitializer::new(ctx)
                .property(
                    js_string!("timeOrigin"),
                    to,
                    Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
                )
                .build()
                .into())
        },
        context,
    )
    .map_err(|_| "performance.toJSON failed")?;

    let add_store_fn = |name: &'static str,
                        len: usize,
                        f: fn(&[JsValue], &PerfStore, &mut Context) -> JsResult<JsValue>,
                        ctx: &mut Context|
     -> Result<(), String> {
        let fobj = FunctionObjectBuilder::new(
            ctx.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_, args, entries: &PerfStore, ctx| {
                    modern::check_context(ctx)?;
                    f(args, entries, ctx)
                },
                entries.clone(),
            ),
        )
        .name(JsString::from(name))
        .length(len)
        .build();
        def_prop(&perf, JsString::from(name), fobj, true, false, true, ctx)
            .map_err(|_| format!("performance.{name} failed"))
    };

    add_store_fn(
        "mark",
        1,
        |args, entries, ctx| {
            let Some(first) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("performance.mark requires 1 argument")
                    .into());
            };
            let entry = PerfEntry {
                name: utf8(first, ctx)?,
                entry_type: "mark",
                start_time: super::dom_events::elapsed_ms(&state(ctx).borrow()),
                duration: 0.0,
            };
            let obj = perf_entry_to_js(&entry, ctx);
            let mut list = entries.0.borrow_mut();
            if list.len() >= 512 {
                list.remove(0);
            }
            list.push(entry);
            Ok(obj.into())
        },
        context,
    )?;

    add_store_fn(
        "measure",
        1,
        |args, entries, ctx| {
            let Some(first) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("performance.measure requires 1 argument")
                    .into());
            };
            let name = utf8(first, ctx)?;
            let now_ms = super::dom_events::elapsed_ms(&state(ctx).borrow());
            let find_mark = |arg: Option<&JsValue>, ctx: &mut Context| -> JsResult<Option<f64>> {
                let Some(v) = arg.filter(|v| v.is_string()) else {
                    return Ok(None);
                };
                let mn = utf8(v, ctx)?;
                Ok(entries
                    .0
                    .borrow()
                    .iter()
                    .rev()
                    .find(|e| e.entry_type == "mark" && e.name == mn)
                    .map(|e| e.start_time))
            };
            let start_time = find_mark(args.get(1), ctx)?.unwrap_or(0.0);
            let end_time = find_mark(args.get(2), ctx)?.unwrap_or(now_ms);
            let entry = PerfEntry {
                name,
                entry_type: "measure",
                start_time,
                duration: (end_time - start_time).max(0.0),
            };
            let obj = perf_entry_to_js(&entry, ctx);
            let mut list = entries.0.borrow_mut();
            if list.len() >= 512 {
                list.remove(0);
            }
            list.push(entry);
            Ok(obj.into())
        },
        context,
    )?;

    for (fn_name, etype) in [("clearMarks", "mark"), ("clearMeasures", "measure")] {
        let fobj = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                move |_, args, (entries, etype): &(PerfStore, &'static str), ctx| {
                    modern::check_context(ctx)?;
                    let filter = match args.first() {
                        Some(v) if !v.is_undefined() => Some(utf8(v, ctx)?),
                        _ => None,
                    };
                    entries.0.borrow_mut().retain(|e| {
                        e.entry_type != *etype || filter.as_deref().is_some_and(|n| e.name != n)
                    });
                    Ok(JsValue::undefined())
                },
                (entries.clone(), etype),
            ),
        )
        .name(JsString::from(fn_name))
        .length(0)
        .build();
        def_prop(
            &perf,
            JsString::from(fn_name),
            fobj,
            true,
            false,
            true,
            context,
        )
        .map_err(|_| "performance.clear failed")?;
    }

    add_store_fn(
        "getEntries",
        0,
        |_, entries, ctx| {
            let snapshot: Vec<PerfEntry> = entries.0.borrow().clone();
            let arr = JsArray::new(ctx)?;
            arr.push(navigation_entry_to_js(ctx), ctx)?;
            for e in &snapshot {
                arr.push(perf_entry_to_js(e, ctx), ctx)?;
            }
            Ok(arr.into())
        },
        context,
    )?;

    add_store_fn(
        "getEntriesByType",
        1,
        |args, entries, ctx| {
            let ty = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            let arr = JsArray::new(ctx)?;
            if ty == "navigation" {
                arr.push(navigation_entry_to_js(ctx), ctx)?;
                return Ok(arr.into());
            }
            let snapshot: Vec<PerfEntry> = entries
                .0
                .borrow()
                .iter()
                .filter(|e| e.entry_type == ty)
                .cloned()
                .collect();
            for e in &snapshot {
                arr.push(perf_entry_to_js(e, ctx), ctx)?;
            }
            Ok(arr.into())
        },
        context,
    )?;

    add_store_fn(
        "getEntriesByName",
        1,
        |args, entries, ctx| {
            let name = utf8(args.first().unwrap_or(&JsValue::undefined()), ctx)?;
            let ty = match args.get(1) {
                Some(v) if !v.is_undefined() => Some(utf8(v, ctx)?),
                _ => None,
            };
            let doc_url = state(ctx).borrow().host.url.as_str().to_owned();
            let arr = JsArray::new(ctx)?;
            if name == doc_url && ty.as_deref().is_none_or(|t| t == "navigation") {
                arr.push(navigation_entry_to_js(ctx), ctx)?;
            }
            let snapshot: Vec<PerfEntry> = entries
                .0
                .borrow()
                .iter()
                .filter(|e| e.name == name && ty.as_deref().is_none_or(|t| e.entry_type == t))
                .cloned()
                .collect();
            for e in &snapshot {
                arr.push(perf_entry_to_js(e, ctx), ctx)?;
            }
            Ok(arr.into())
        },
        context,
    )?;

    let time_origin = state(context).borrow().time_origin_ms;
    let nav_start = time_origin.floor().max(0.0);
    let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
    let mut timing_init = ObjectInitializer::new(context);
    for zprop in [
        "redirectStart",
        "redirectEnd",
        "unloadEventStart",
        "unloadEventEnd",
    ] {
        timing_init.property(JsString::from(zprop), 0.0, ec);
    }
    for ts_prop in [
        "navigationStart",
        "fetchStart",
        "domainLookupStart",
        "domainLookupEnd",
        "connectStart",
        "connectEnd",
        "secureConnectionStart",
        "requestStart",
        "responseStart",
        "responseEnd",
        "domLoading",
        "domInteractive",
        "domContentLoadedEventStart",
        "domContentLoadedEventEnd",
        "domComplete",
        "loadEventStart",
        "loadEventEnd",
    ] {
        timing_init.property(JsString::from(ts_prop), nav_start, ec);
    }
    let timing_obj = timing_init.build();
    let nav_obj = ObjectInitializer::new(context)
        .property(js_string!("type"), 0, ec)
        .property(js_string!("redirectCount"), 0, ec)
        .property(js_string!("TYPE_NAVIGATE"), 0, Attribute::ENUMERABLE)
        .property(js_string!("TYPE_RELOAD"), 1, Attribute::ENUMERABLE)
        .property(js_string!("TYPE_BACK_FORWARD"), 2, Attribute::ENUMERABLE)
        .property(js_string!("TYPE_RESERVED"), 255, Attribute::ENUMERABLE)
        .build();

    def_prop(
        &perf,
        js_string!("timeOrigin"),
        time_origin,
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "performance.timeOrigin failed")?;
    def_prop(
        &perf,
        js_string!("timing"),
        timing_obj,
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "performance.timing failed")?;
    def_prop(
        &perf,
        js_string!("navigation"),
        nav_obj,
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "performance.navigation failed")?;
    def_prop(
        &context.global_object(),
        js_string!("performance"),
        perf,
        true,
        true,
        true,
        context,
    )
    .map_err(|_| "Global performance failed".to_owned())
}

// --- `TextEncoder` & `TextDecoder` ---

fn register_text_encoding(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();
    // TextEncoder
    let enc_proto = JsObject::with_object_proto(context.intrinsics());
    def_prop(
        &enc_proto,
        js_string!("encoding"),
        js_string!("utf-8"),
        false,
        true,
        true,
        context,
    )
    .map_err(|_| "TextEncoder.encoding failed")?;
    def_fn(&enc_proto, "encode", 0, false, text_encoder_encode, context)
        .map_err(|_| "TextEncoder.encode failed")?;
    def_fn(
        &enc_proto,
        "encodeInto",
        2,
        false,
        text_encoder_encode_into,
        context,
    )
    .map_err(|_| "TextEncoder.encodeInto failed")?;
    let enc_ctor = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, _, proto, context| {
                modern::check_context(context)?;
                if new_target.is_undefined() {
                    return Err(JsNativeError::typ()
                        .with_message("TextEncoder requires 'new'")
                        .into());
                }
                Ok(JsObject::from_proto_and_data(Some(proto.clone()), TextEncoderData).into())
            },
            enc_proto.clone(),
        ),
    )
    .name(js_string!("TextEncoder"))
    .length(0)
    .constructor(true)
    .build();
    link_ctor_global(
        &global,
        "TextEncoder",
        &enc_ctor.into(),
        &enc_proto,
        context,
    )?;

    // TextDecoder
    let dec_proto = JsObject::with_object_proto(context.intrinsics());
    def_fn(&dec_proto, "decode", 0, false, text_decoder_decode, context)
        .map_err(|_| "TextDecoder.decode failed")?;
    for prop in ["encoding", "fatal", "ignoreBOM"] {
        def_getter_cap(
            &dec_proto,
            prop,
            true,
            prop,
            |this, _, prop, ctx| {
                modern::check_context(ctx)?;
                let obj = this
                    .as_object()
                    .ok_or_else(|| failure("Invalid TextDecoder receiver"))?;
                let data = obj
                    .downcast_ref::<TextDecoderData>()
                    .ok_or_else(|| failure("Invalid TextDecoder receiver"))?;
                Ok(match *prop {
                    "encoding" => JsString::from(data.encoding.name()).into(),
                    "fatal" => data.fatal.into(),
                    _ => data.ignore_bom.into(),
                })
            },
            context,
        )
        .map_err(|_| "TextDecoder getter failed")?;
    }
    let dec_ctor = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, args, proto, context| {
                modern::check_context(context)?;
                if new_target.is_undefined() {
                    return Err(JsNativeError::typ()
                        .with_message("TextDecoder requires 'new'")
                        .into());
                }
                let label = match args.first() {
                    None => "utf-8".to_owned(),
                    Some(v) if v.is_undefined() => "utf-8".to_owned(),
                    Some(v) => utf8(v, context)?,
                };
                let encoding = DecoderEncoding::from_label(&label).ok_or_else(|| {
                    JsNativeError::range().with_message(format!(
                        "The encoding label provided ('{label}') is invalid."
                    ))
                })?;
                let mut fatal = false;
                let mut ignore_bom = false;
                if let Some(opts) = args.get(1)
                    && !opts.is_undefined()
                    && !opts.is_null()
                {
                    let Some(dict) = opts.as_object() else {
                        return Err(JsNativeError::typ()
                            .with_message("TextDecoderOptions must be an object")
                            .into());
                    };
                    let f = dict.get(js_string!("fatal"), context)?;
                    if !f.is_undefined() {
                        fatal = f.to_boolean();
                    }
                    let ib = dict.get(js_string!("ignoreBOM"), context)?;
                    if !ib.is_undefined() {
                        ignore_bom = ib.to_boolean();
                    }
                }
                Ok(JsObject::from_proto_and_data(
                    Some(proto.clone()),
                    TextDecoderData {
                        encoding,
                        fatal,
                        ignore_bom,
                    },
                )
                .into())
            },
            dec_proto.clone(),
        ),
    )
    .name(js_string!("TextDecoder"))
    .length(0)
    .constructor(true)
    .build();
    link_ctor_global(
        &global,
        "TextDecoder",
        &dec_ctor.into(),
        &dec_proto,
        context,
    )
}

fn text_encoder_encode(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid TextEncoder receiver"))?;
    if !obj.is::<TextEncoderData>() {
        return Err(failure("Invalid TextEncoder receiver"));
    }
    let s = match args.first() {
        None => String::new(),
        Some(v) if v.is_undefined() => String::new(),
        Some(v) => utf8(v, context)?,
    };
    let u8_arr = JsUint8Array::from_iter(s.into_bytes(), context)?;
    Ok(u8_arr.into())
}

fn text_encoder_encode_into(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid TextEncoder receiver"))?;
    if !obj.is::<TextEncoderData>() {
        return Err(failure("Invalid TextEncoder receiver"));
    }
    let src_val = args.first().cloned().unwrap_or_default();
    let dst_val = args.get(1).cloned().unwrap_or_default();
    let js_str = src_val.to_string(context)?;
    let Some(dst_obj) = dst_val.as_object() else {
        return Err(failure("Argument 2 of encodeInto must be a Uint8Array"));
    };
    let u8_view = JsUint8Array::from_object(dst_obj)?;
    let buf_val = u8_view.buffer(context)?;
    let ab = JsArrayBuffer::from_object(
        buf_val
            .as_object()
            .ok_or_else(|| failure("Invalid Uint8Array buffer"))?,
    )?;
    let byte_offset = u8_view.byte_offset(context)?;
    let byte_length = u8_view.byte_length(context)?;
    let units: Vec<u16> = js_str.iter().collect();
    let mut read_units = 0usize;
    let mut written_bytes = 0usize;
    {
        let mut guard = ab.data_mut();
        if let Some(slice) = guard.as_deref_mut() {
            let dest = &mut slice[byte_offset..byte_offset + byte_length];
            for decoded in char::decode_utf16(units.into_iter()) {
                let (ch, unit_len) = match decoded {
                    Ok(c) => (c, c.len_utf16()),
                    Err(_) => ('\u{FFFD}', 1),
                };
                let need = ch.len_utf8();
                if written_bytes + need > dest.len() {
                    break;
                }
                ch.encode_utf8(&mut dest[written_bytes..written_bytes + need]);
                written_bytes += need;
                read_units += unit_len;
            }
        }
    }
    let res = ObjectInitializer::new(context)
        .property(
            js_string!("read"),
            read_units as f64,
            Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("written"),
            written_bytes as f64,
            Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
        )
        .build();
    Ok(res.into())
}

fn text_decoder_decode(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid TextDecoder receiver"))?;
    let (encoding, fatal, ignore_bom) = {
        let data = obj
            .downcast_ref::<TextDecoderData>()
            .ok_or_else(|| failure("Invalid TextDecoder receiver"))?;
        (data.encoding, data.fatal, data.ignore_bom)
    };
    let bytes = match args.first() {
        None => Vec::new(),
        Some(v) if v.is_undefined() => Vec::new(),
        Some(v) => extract_buffer_source_bytes(v, context)?,
    };
    let decoded = encoding
        .decode(&bytes, fatal, ignore_bom)
        .map_err(|()| JsNativeError::typ().with_message("The encoded data was not valid."))?;
    Ok(JsString::from(decoded).into())
}

pub(super) fn extract_buffer_source_bytes(
    val: &JsValue,
    context: &mut Context,
) -> JsResult<Vec<u8>> {
    let slice_ab = |ab: JsArrayBuffer, off: usize, len: Option<usize>| -> JsResult<Vec<u8>> {
        let guard = ab.data();
        let Some(slice) = guard.as_deref() else {
            return Err(JsNativeError::typ()
                .with_message("BufferSource is detached")
                .into());
        };
        Ok(match len {
            Some(l) => slice[off..off + l].to_vec(),
            None => slice.to_vec(),
        })
    };
    let Some(obj) = val.as_object() else {
        return Err(JsNativeError::typ()
            .with_message("Expected BufferSource (ArrayBuffer, TypedArray, or DataView)")
            .into());
    };
    if let Ok(ab) = JsArrayBuffer::from_object(obj.clone()) {
        return slice_ab(ab, 0, None);
    }
    if let Ok(ta) = JsTypedArray::from_object(obj.clone()) {
        let ab = JsArrayBuffer::from_object(
            ta.buffer(context)?
                .as_object()
                .ok_or_else(|| failure("Invalid TypedArray buffer"))?,
        )?;
        return slice_ab(ab, ta.byte_offset(context)?, Some(ta.byte_length(context)?));
    }
    if let Ok(dv) = JsDataView::from_object(obj) {
        let ab = JsArrayBuffer::from_object(
            dv.buffer(context)?
                .as_object()
                .ok_or_else(|| failure("Invalid DataView buffer"))?,
        )?;
        return slice_ab(
            ab,
            dv.byte_offset(context)? as usize,
            Some(dv.byte_length(context)? as usize),
        );
    }
    Err(JsNativeError::typ()
        .with_message("Expected BufferSource (ArrayBuffer, TypedArray, or DataView)")
        .into())
}

// --- `URL` & `URLSearchParams` ---

fn parse_url_args(
    args: &[JsValue],
    err_msg: &'static str,
    context: &mut Context,
) -> JsResult<Result<url::Url, url::ParseError>> {
    let Some(first) = args.first() else {
        return Err(JsNativeError::typ().with_message(err_msg).into());
    };
    let url_str = utf8(first, context)?;
    let base_str = match args.get(1) {
        None => None,
        Some(b) if b.is_undefined() => None,
        Some(b) => Some(utf8(b, context)?),
    };
    Ok(if let Some(base) = base_str.as_deref() {
        url::Url::parse(base).and_then(|b| b.join(&url_str))
    } else {
        url::Url::parse(&url_str)
    })
}

fn register_url_and_params(context: &mut Context) -> Result<(), String> {
    let global = context.global_object();
    let sp_proto = build_search_params_proto(context)?;
    let sp_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, args, sp_proto, context| {
                construct_search_params(new_target, args, sp_proto, context)
            },
            sp_proto.clone(),
        ),
    )
    .name(js_string!("URLSearchParams"))
    .length(0)
    .constructor(true)
    .build()
    .into();
    link_ctor_global(&global, "URLSearchParams", &sp_ctor, &sp_proto, context)?;

    let url_proto = build_url_proto(sp_proto, context)?;
    let url_ctor: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |new_target, args, url_proto, context| {
                modern::check_context(context)?;
                if new_target.is_undefined() {
                    return Err(JsNativeError::typ()
                        .with_message("Failed to construct 'URL': Please use the 'new' operator")
                        .into());
                }
                let parsed = parse_url_args(
                    args,
                    "URL constructor requires at least 1 argument",
                    context,
                )?
                .map_err(|e| JsNativeError::typ().with_message(format!("Invalid URL: {e}")))?;
                Ok(JsObject::from_proto_and_data(
                    Some(url_proto.clone()),
                    SharedUrlData {
                        inner: Rc::new(RefCell::new(parsed)),
                        params_obj: GcRefCell::new(None),
                    },
                )
                .into())
            },
            url_proto.clone(),
        ),
    )
    .name(js_string!("URL"))
    .length(1)
    .constructor(true)
    .build()
    .into();

    def_fn(
        &url_ctor,
        "canParse",
        1,
        true,
        |_, args, ctx| {
            modern::check_context(ctx)?;
            Ok(
                parse_url_args(args, "URL.canParse requires 1 argument", ctx)?
                    .is_ok()
                    .into(),
            )
        },
        context,
    )
    .map_err(|_| "URL.canParse failed")?;
    let parse_fn = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |_, args, url_proto, context| {
                modern::check_context(context)?;
                Ok(
                    match parse_url_args(args, "URL.parse requires 1 argument", context)? {
                        Ok(parsed) => JsObject::from_proto_and_data(
                            Some(url_proto.clone()),
                            SharedUrlData {
                                inner: Rc::new(RefCell::new(parsed)),
                                params_obj: GcRefCell::new(None),
                            },
                        )
                        .into(),
                        Err(_) => JsValue::null(),
                    },
                )
            },
            url_proto.clone(),
        ),
    )
    .name(js_string!("parse"))
    .length(1)
    .build();
    def_prop(
        &url_ctor,
        js_string!("parse"),
        parse_fn,
        true,
        true,
        true,
        context,
    )
    .map_err(|_| "URL.parse failed")?;
    link_ctor_global(&global, "URL", &url_ctor, &url_proto, context)
}

fn url_data(this: &JsValue) -> JsResult<Rc<RefCell<url::Url>>> {
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid URL receiver"))?;
    let data = obj
        .downcast_ref::<SharedUrlData>()
        .ok_or_else(|| failure("Invalid URL receiver"))?;
    Ok(data.inner.clone())
}

fn url_prop_get(this: &JsValue, prop: &str, context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let shared = url_data(this)?;
    let u = shared.borrow();
    let s: String = match prop {
        "href" => u.as_str().to_owned(),
        "origin" => u.origin().ascii_serialization(),
        "protocol" => format!("{}:", u.scheme()),
        "username" => u.username().to_owned(),
        "password" => u.password().unwrap_or("").to_owned(),
        "host" => match (u.host_str(), u.port()) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => h.to_owned(),
            _ => String::new(),
        },
        "hostname" => u.host_str().unwrap_or("").to_owned(),
        "port" => u.port().map(|p| p.to_string()).unwrap_or_default(),
        "pathname" => u.path().to_owned(),
        "search" => match u.query() {
            Some("") | None => String::new(),
            Some(q) => format!("?{q}"),
        },
        "hash" => match u.fragment() {
            Some("") | None => String::new(),
            Some(f) => format!("#{f}"),
        },
        _ => String::new(),
    };
    Ok(JsString::from(s).into())
}

fn url_prop_set(
    this: &JsValue,
    args: &[JsValue],
    prop: &str,
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let shared = url_data(this)?;
    let val = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
    {
        let mut u = shared.borrow_mut();
        match prop {
            "href" => {
                *u = url::Url::parse(&val)
                    .map_err(|e| JsNativeError::typ().with_message(format!("Invalid URL: {e}")))?;
            }
            "protocol" => {
                let _ = u.set_scheme(val.trim_end_matches(':'));
            }
            "username" => {
                let _ = u.set_username(&val);
            }
            "password" => {
                let _ = u.set_password((!val.is_empty()).then_some(&val));
            }
            "host" => {
                if let Some((h, p)) = val.rsplit_once(':')
                    && let Ok(port_num) = p.parse::<u16>()
                {
                    let _ = u.set_host(Some(h));
                    let _ = u.set_port(Some(port_num));
                } else {
                    let _ = u.set_host(Some(&val));
                }
            }
            "hostname" => {
                let _ = u.set_host(Some(&val));
            }
            "port" => {
                if val.is_empty() {
                    let _ = u.set_port(None);
                } else if let Ok(p) = val.parse::<u16>() {
                    let _ = u.set_port(Some(p));
                }
            }
            "pathname" => u.set_path(&val),
            "search" => {
                let q = val.strip_prefix('?').unwrap_or(&val);
                u.set_query((!q.is_empty()).then_some(q));
            }
            "hash" => {
                let f = val.strip_prefix('#').unwrap_or(&val);
                u.set_fragment((!f.is_empty()).then_some(f));
            }
            _ => {}
        }
    }
    // Sync attached URLSearchParams if already instantiated
    if let Some(obj) = this.as_object()
        && let Some(ud) = obj.downcast_ref::<SharedUrlData>()
        && let Some(sp_obj) = ud.params_obj.borrow().as_ref()
        && let Some(spd) = sp_obj.downcast_ref::<SearchParamsData>()
    {
        spd.sync_from_url();
    }
    Ok(JsValue::undefined())
}

fn build_url_proto(sp_proto: JsObject, context: &mut Context) -> Result<JsObject, String> {
    let proto = JsObject::with_object_proto(context.intrinsics());
    def_getter_cap(
        &proto,
        "origin",
        true,
        "origin",
        |this, _, p, ctx| url_prop_get(this, p, ctx),
        context,
    )
    .map_err(|_| "URL.origin failed")?;
    for prop in [
        "href", "protocol", "username", "password", "host", "hostname", "port", "pathname",
        "search", "hash",
    ] {
        def_get_set_cap(
            &proto,
            prop,
            true,
            prop,
            |this, _, p, ctx| url_prop_get(this, p, ctx),
            |this, args, p, ctx| url_prop_set(this, args, p, ctx),
            context,
        )
        .map_err(|_| "URL accessor failed")?;
    }

    let sp_getter = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |this, _, sp_proto, context| {
                modern::check_context(context)?;
                let obj = this
                    .as_object()
                    .ok_or_else(|| failure("Invalid URL receiver"))?;
                let inner = {
                    let ud = obj
                        .downcast_ref::<SharedUrlData>()
                        .ok_or_else(|| failure("Invalid URL receiver"))?;
                    if let Some(existing) = ud.params_obj.borrow().clone() {
                        return Ok(existing.into());
                    }
                    ud.inner.clone()
                };
                let pairs: Vec<(String, String)> = inner
                    .borrow()
                    .query_pairs()
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect();
                let sp_obj = JsObject::from_proto_and_data(
                    Some(sp_proto.clone()),
                    SearchParamsData {
                        pairs: Rc::new(RefCell::new(pairs)),
                        linked_url: Some(inner),
                    },
                );
                if let Some(ud) = obj.downcast_ref::<SharedUrlData>() {
                    *ud.params_obj.borrow_mut() = Some(sp_obj.clone());
                }
                Ok(sp_obj.into())
            },
            sp_proto,
        ),
    )
    .name(js_string!("get searchParams"))
    .build();
    proto
        .define_property_or_throw(
            js_string!("searchParams"),
            modern::boa_engine::property::PropertyDescriptor::builder()
                .get(sp_getter)
                .enumerable(true)
                .configurable(true),
            context,
        )
        .map_err(|_| "URL.searchParams failed")?;

    def_fn(
        &proto,
        "toString",
        0,
        false,
        |this, _, ctx| url_prop_get(this, "href", ctx),
        context,
    )
    .map_err(|_| "URL.toString failed")?;
    let to_str = proto
        .get(js_string!("toString"), context)
        .map_err(|_| "URL.toString get failed")?;
    def_prop(
        &proto,
        js_string!("toJSON"),
        to_str,
        true,
        false,
        true,
        context,
    )
    .map_err(|_| "URL.toJSON failed")?;
    Ok(proto)
}

fn construct_search_params(
    new_target: &JsValue,
    args: &[JsValue],
    sp_proto: &JsObject,
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    if new_target.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("URLSearchParams requires 'new'")
            .into());
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(init) = args.first()
        && !init.is_undefined()
    {
        if let Some(obj) = init.as_object() {
            if let Some(spd) = obj.downcast_ref::<SearchParamsData>() {
                spd.sync_from_url();
                pairs = spd.pairs.borrow().clone();
            } else if let Some(iter_fn) = obj.get(JsSymbol::iterator(), context)?.as_callable() {
                let iter_val = iter_fn.call(init, &[], context)?;
                let Some(iter_obj) = iter_val.as_object() else {
                    return Err(JsNativeError::typ()
                        .with_message("Iterator must be an object")
                        .into());
                };
                let next_fn = iter_obj
                    .get(js_string!("next"), context)?
                    .as_callable()
                    .ok_or_else(|| {
                        JsNativeError::typ().with_message("Iterator.next is not callable")
                    })?;
                for _ in 0..8_192 {
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
                            .with_message("URLSearchParams sequence item must be a 2-tuple")
                            .into());
                    };
                    let len = pair_obj
                        .get(js_string!("length"), context)?
                        .to_length(context)?;
                    if len != 2 {
                        return Err(JsNativeError::typ()
                            .with_message("URLSearchParams sequence item must have length 2")
                            .into());
                    }
                    let k = utf8(&pair_obj.get(0, context)?, context)?;
                    let v = utf8(&pair_obj.get(1, context)?, context)?;
                    pairs.push((k, v));
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
                            let val = obj.get(k.clone(), context)?;
                            pairs.push((ks.to_std_string_lossy(), utf8(&val, context)?));
                        }
                        PropertyKey::Index(idx) => {
                            let val = obj.get(k.clone(), context)?;
                            pairs.push((idx.get().to_string(), utf8(&val, context)?));
                        }
                        PropertyKey::Symbol(_) => {}
                    }
                }
            }
        } else {
            let s = utf8(init, context)?;
            let query = s.strip_prefix('?').unwrap_or(&s);
            pairs = url::form_urlencoded::parse(query.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
        }
    }
    Ok(JsObject::from_proto_and_data(
        Some(sp_proto.clone()),
        SearchParamsData {
            pairs: Rc::new(RefCell::new(pairs)),
            linked_url: None,
        },
    )
    .into())
}

fn sp_receiver(this: &JsValue) -> JsResult<JsObject> {
    let obj = this
        .as_object()
        .ok_or_else(|| failure("Invalid URLSearchParams receiver"))?;
    if !obj.is::<SearchParamsData>() {
        return Err(failure("Invalid URLSearchParams receiver"));
    }
    Ok(obj)
}

fn sp_call(op: &str, this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let obj = sp_receiver(this)?;
    match op {
        "append" => {
            if args.len() < 2 {
                return Err(failure("URLSearchParams.append requires 2 arguments"));
            }
            let (k, v) = (utf8(&args[0], context)?, utf8(&args[1], context)?);
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            spd.pairs.borrow_mut().push((k, v));
            spd.sync_to_url();
            Ok(JsValue::undefined())
        }
        "delete" => {
            let Some(first) = args.first() else {
                return Err(failure("URLSearchParams.delete requires 1 argument"));
            };
            let k = utf8(first, context)?;
            let vf = match args.get(1) {
                Some(v) if !v.is_undefined() => Some(utf8(v, context)?),
                _ => None,
            };
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            spd.pairs
                .borrow_mut()
                .retain(|(ek, ev)| ek != &k || vf.as_ref().is_some_and(|f| ev != f));
            spd.sync_to_url();
            Ok(JsValue::undefined())
        }
        "get" => {
            let Some(first) = args.first() else {
                return Err(failure("URLSearchParams.get requires 1 argument"));
            };
            let k = utf8(first, context)?;
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            Ok(spd
                .pairs
                .borrow()
                .iter()
                .find(|(ek, _)| ek == &k)
                .map_or(JsValue::null(), |(_, ev)| {
                    JsString::from(ev.as_str()).into()
                }))
        }
        "getAll" => {
            let Some(first) = args.first() else {
                return Err(failure("URLSearchParams.getAll requires 1 argument"));
            };
            let k = utf8(first, context)?;
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            let vals: Vec<JsValue> = spd
                .pairs
                .borrow()
                .iter()
                .filter(|(ek, _)| ek == &k)
                .map(|(_, ev)| JsString::from(ev.as_str()).into())
                .collect();
            Ok(JsArray::from_iter(vals, context).into())
        }
        "has" => {
            let Some(first) = args.first() else {
                return Err(failure("URLSearchParams.has requires 1 argument"));
            };
            let k = utf8(first, context)?;
            let vf = match args.get(1) {
                Some(v) if !v.is_undefined() => Some(utf8(v, context)?),
                _ => None,
            };
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            Ok(spd
                .pairs
                .borrow()
                .iter()
                .any(|(ek, ev)| ek == &k && vf.as_ref().is_none_or(|f| ev == f))
                .into())
        }
        "set" => {
            if args.len() < 2 {
                return Err(failure("URLSearchParams.set requires 2 arguments"));
            }
            let (k, v) = (utf8(&args[0], context)?, utf8(&args[1], context)?);
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            {
                let mut pairs = spd.pairs.borrow_mut();
                let mut replaced = false;
                pairs.retain_mut(|(ek, ev)| {
                    if ek != &k {
                        true
                    } else if !replaced {
                        *ev = v.clone();
                        replaced = true;
                        true
                    } else {
                        false
                    }
                });
                if !replaced {
                    pairs.push((k, v));
                }
            }
            spd.sync_to_url();
            Ok(JsValue::undefined())
        }
        "sort" => {
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            spd.pairs
                .borrow_mut()
                .sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            spd.sync_to_url();
            Ok(JsValue::undefined())
        }
        "toString" => {
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            Ok(JsString::from(serialize_query_pairs(&spd.pairs.borrow())).into())
        }
        "entries" | "keys" | "values" => {
            let snapshot = {
                let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
                spd.sync_from_url();
                spd.pairs.borrow().clone()
            };
            let items: Vec<JsValue> = snapshot
                .into_iter()
                .map(|(k, v)| match op {
                    "keys" => JsString::from(k).into(),
                    "values" => JsString::from(v).into(),
                    _ => JsArray::from_iter(
                        [JsString::from(k).into(), JsString::from(v).into()],
                        context,
                    )
                    .into(),
                })
                .collect();
            JsArray::from_iter(items, context).values(context)
        }
        _ => {
            let cb = args
                .first()
                .and_then(JsValue::as_callable)
                .ok_or_else(|| failure("URLSearchParams.forEach requires a callback"))?;
            let this_arg = args.get(1).cloned().unwrap_or_default();
            let snapshot = {
                let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
                spd.sync_from_url();
                spd.pairs.borrow().clone()
            };
            for (k, v) in snapshot {
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

fn build_search_params_proto(context: &mut Context) -> Result<JsObject, String> {
    let proto = JsObject::with_object_proto(context.intrinsics());
    for (mname, len) in [
        ("append", 2),
        ("delete", 1),
        ("get", 1),
        ("getAll", 1),
        ("has", 1),
        ("set", 2),
        ("sort", 0),
        ("toString", 0),
        ("entries", 0),
        ("keys", 0),
        ("values", 0),
        ("forEach", 1),
    ] {
        def_cap(
            &proto,
            mname,
            len,
            true,
            mname,
            |this, args, op, ctx| sp_call(op, this, args, ctx),
            context,
        )
        .map_err(|_| "URLSearchParams method failed")?;
    }
    let entries_fn = proto
        .get(js_string!("entries"), context)
        .map_err(|_| "URLSearchParams.entries get failed")?;
    def_prop(
        &proto,
        JsSymbol::iterator(),
        entries_fn,
        true,
        false,
        true,
        context,
    )
    .map_err(|_| "URLSearchParams iterator failed")?;
    def_getter_fn(
        &proto,
        "size",
        true,
        |this, _, ctx| {
            modern::check_context(ctx)?;
            let obj = sp_receiver(this)?;
            let spd = obj.downcast_ref::<SearchParamsData>().unwrap();
            spd.sync_from_url();
            Ok((spd.pairs.borrow().len() as f64).into())
        },
        context,
    )
    .map_err(|_| "URLSearchParams.size failed")?;
    Ok(proto)
}

// --- Method Calls (`atob`, `btoa`, `queueMicrotask`, `structuredClone`, `matchMedia`, Storage, History) ---

pub(super) fn call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    match name {
        "host.window.btoa" => {
            let Some(first) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("btoa requires 1 argument")
                    .into());
            };
            let js_str = first.to_string(context)?;
            let mut bytes = Vec::with_capacity(js_str.len());
            for u in js_str.iter() {
                if u > 0xFF {
                    return Err(dom_exception::throw(
                        "InvalidCharacterError",
                        "The string to be encoded contains characters outside of the Latin1 range.",
                        context,
                    ));
                }
                bytes.push(u as u8);
            }
            Ok(JsString::from(base64_encode(&bytes)).into())
        }
        "host.window.atob" => {
            let Some(first) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("atob requires 1 argument")
                    .into());
            };
            let js_str = first.to_string(context)?;
            let mut ascii = String::with_capacity(js_str.len());
            for u in js_str.iter() {
                if u > 0x7F {
                    return Err(dom_exception::throw(
                        "InvalidCharacterError",
                        "The string to be decoded contains characters outside of the Base64 range.",
                        context,
                    ));
                }
                ascii.push(u as u8 as char);
            }
            let decoded = base64_decode_forgiving(&ascii)
                .map_err(|msg| dom_exception::throw("InvalidCharacterError", msg, context))?;
            let units: Vec<u16> = decoded.into_iter().map(u16::from).collect();
            Ok(JsString::from(units.as_slice()).into())
        }
        "host.window.queueMicrotask" => {
            let cb = args.first().and_then(JsValue::as_callable).ok_or_else(|| {
                JsNativeError::typ().with_message("queueMicrotask requires a callable function")
            })?;
            let realm = context.realm().clone();
            let job = GenericJob::new(move |ctx| cb.call(&JsValue::undefined(), &[], ctx), realm);
            context.enqueue_job(Job::GenericJob(job));
            modern::check_context(context)?;
            Ok(JsValue::undefined())
        }
        "host.window.structuredClone" => {
            let Some(val) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("structuredClone requires at least 1 argument")
                    .into());
            };
            let mut transfers = Vec::new();
            if let Some(opts) = args.get(1)
                && !opts.is_undefined()
                && !opts.is_null()
            {
                let Some(opts_obj) = opts.as_object() else {
                    return Err(JsNativeError::typ()
                        .with_message("structuredClone options must be an object")
                        .into());
                };
                let tr = opts_obj.get(js_string!("transfer"), context)?;
                if !tr.is_undefined() {
                    let Some(tr_obj) = tr.as_object() else {
                        return Err(JsNativeError::typ()
                            .with_message("structuredClone transfer option must be a sequence")
                            .into());
                    };
                    let len = tr_obj
                        .get(js_string!("length"), context)?
                        .to_length(context)?;
                    for i in 0..len.min(1024) {
                        let item = tr_obj.get(i as usize, context)?;
                        let Some(item_obj) = item.as_object() else {
                            return Err(dom_exception::throw(
                                "DataCloneError",
                                "Transfer item must be an object",
                                context,
                            ));
                        };
                        transfers.push(item_obj);
                    }
                }
            }
            let graph = message_clone::serialize_structured(val, &transfers, context)?;
            message_clone::restore(&graph, context)
        }
        "host.window.matchMedia" => {
            let Some(first) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("matchMedia requires 1 argument")
                    .into());
            };
            let query = utf8(first, context)?;
            let vp = state(context).borrow().viewport;
            let (serialized_media, matches) = crate::style::evaluate_media_query(
                &query,
                vp.inner_width as f32,
                vp.inner_height as f32,
                vp.device_pixel_ratio,
                false,
            );
            let noop = FunctionObjectBuilder::new(
                context.realm(),
                NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
            )
            .build();
            let dispatch_stub = FunctionObjectBuilder::new(
                context.realm(),
                NativeFunction::from_fn_ptr(|_, _, _| Ok(true.into())),
            )
            .build();
            let ec = Attribute::ENUMERABLE | Attribute::CONFIGURABLE;
            let wc = Attribute::WRITABLE | Attribute::CONFIGURABLE;
            let mut mql = ObjectInitializer::new(context);
            mql.property(js_string!("media"), JsString::from(serialized_media), ec)
                .property(js_string!("matches"), matches, ec)
                .property(
                    js_string!("onchange"),
                    JsValue::null(),
                    wc | Attribute::ENUMERABLE,
                )
                .property(js_string!("dispatchEvent"), dispatch_stub, wc);
            for m in [
                "addEventListener",
                "removeEventListener",
                "addListener",
                "removeListener",
            ] {
                mql.property(JsString::from(m), noop.clone(), wc);
            }
            Ok(mql.build().into())
        }
        STORAGE_GET_ITEM => {
            let is_local = storage_receiver(this, context)?;
            let Some(first) = args.first() else {
                return Err(failure("Storage.getItem requires 1 argument"));
            };
            let k = utf8(first, context)?;
            let val = storage_get_value(is_local, &k, &state(context).borrow());
            Ok(match val {
                Some(v) => JsString::from(v).into(),
                None => JsValue::null(),
            })
        }
        STORAGE_SET_ITEM => {
            let is_local = storage_receiver(this, context)?;
            if args.len() < 2 {
                return Err(failure("Storage.setItem requires 2 arguments"));
            }
            let k = utf8(&args[0], context)?;
            let v = utf8(&args[1], context)?;
            storage_set_value(is_local, k, v, context)?;
            Ok(JsValue::undefined())
        }
        STORAGE_REMOVE_ITEM => {
            let is_local = storage_receiver(this, context)?;
            let Some(first) = args.first() else {
                return Err(failure("Storage.removeItem requires 1 argument"));
            };
            let k = utf8(first, context)?;
            storage_remove_value(is_local, &k, context)?;
            Ok(JsValue::undefined())
        }
        STORAGE_CLEAR => {
            let is_local = storage_receiver(this, context)?;
            storage_clear_all(is_local, context)?;
            Ok(JsValue::undefined())
        }
        STORAGE_KEY => {
            let is_local = storage_receiver(this, context)?;
            let Some(first) = args.first() else {
                return Err(failure("Storage.key requires 1 argument"));
            };
            let idx = first.to_u32(context)? as usize;
            let shared = state(context);
            let data = shared.borrow();
            let map = if is_local {
                &data.local_storage_map
            } else {
                &data.session_storage_map
            };
            Ok(match map.get(idx) {
                Some((k, _)) => JsString::from(k.as_str()).into(),
                None => JsValue::null(),
            })
        }
        HISTORY_PUSH_STATE | HISTORY_REPLACE_STATE => {
            history_receiver(this, context)?;
            if args.len() < 2 {
                return Err(failure(
                    "History state method requires at least 2 arguments",
                ));
            }
            let is_push = name == HISTORY_PUSH_STATE;
            history_push_or_replace(is_push, &args[0], args.get(2), context)?;
            Ok(JsValue::undefined())
        }
        HISTORY_BACK => {
            history_receiver(this, context)?;
            record_history_go(-1, context)?;
            Ok(JsValue::undefined())
        }
        HISTORY_FORWARD => {
            history_receiver(this, context)?;
            record_history_go(1, context)?;
            Ok(JsValue::undefined())
        }
        HISTORY_GO => {
            history_receiver(this, context)?;
            let delta = match args.first() {
                None => 0,
                Some(v) if v.is_undefined() => 0,
                Some(v) => v.to_i32(context)?,
            };
            record_history_go(delta, context)?;
            Ok(JsValue::undefined())
        }
        NAVIGATOR_SEND_BEACON => navigator_send_beacon(this, args, context),
        _ => Err(failure("Unknown Web API method")),
    }
}

fn navigator_send_beacon(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    match receiver(this, context)? {
        Value::Host(h) if h == "navigator" => {}
        _ => return Err(failure("Invalid Navigator receiver")),
    }
    let Some(url_arg) = args.first() else {
        return Err(failure("Navigator.sendBeacon requires at least 1 argument"));
    };
    let raw_url = utf8(url_arg, context)?;
    let base_url = state(context).borrow().host.url.clone();
    let Ok(parsed_url) = base_url.join(&raw_url) else {
        return Err(failure(
            "Failed to execute 'sendBeacon' on 'Navigator': Invalid URL",
        ));
    };
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(failure(
            "Failed to execute 'sendBeacon' on 'Navigator': URL scheme must be http or https",
        ));
    }
    let payload_len = match args.get(1) {
        None => 0usize,
        Some(v) if v.is_null_or_undefined() => 0usize,
        Some(v) => {
            if let Some(s) = v.as_string() {
                s.to_std_string_escaped().len()
            } else if let Some(obj) = v.as_object() {
                if let Some(encoded) = try_serialize_search_params(&obj) {
                    encoded.len()
                } else if JsArrayBuffer::from_object(obj.clone()).is_ok()
                    || JsTypedArray::from_object(obj.clone()).is_ok()
                    || JsDataView::from_object(obj.clone()).is_ok()
                {
                    extract_buffer_source_bytes(v, context)?.len()
                } else {
                    utf8(v, context)?.len()
                }
            } else {
                utf8(v, context)?.len()
            }
        }
    };
    const MAX_BEACON_PAYLOAD_BYTES: usize = 65_536;
    if payload_len > MAX_BEACON_PAYLOAD_BYTES
        || parsed_url.as_str().len() > crate::page_session::platform::MAX_FETCH_URL_BYTES
    {
        return Ok(false.into());
    }
    Ok(true.into())
}

// --- Web Storage (`storage:local` & `storage:session`) ---

fn storage_receiver(this: &JsValue, context: &mut Context) -> JsResult<bool> {
    let Value::Host(handle) = receiver(this, context)? else {
        return Err(failure("Invalid Storage receiver"));
    };
    match handle.as_str() {
        "storage:local" => Ok(true),
        "storage:session" => Ok(false),
        _ => Err(failure("Invalid Storage receiver")),
    }
}

fn storage_get_value(is_local: bool, k: &str, data: &State) -> Option<String> {
    let map = if is_local {
        &data.local_storage_map
    } else {
        &data.session_storage_map
    };
    map.iter().find(|(ek, _)| ek == k).map(|(_, ev)| ev.clone())
}

fn storage_pair_mut(
    data: &mut State,
    is_local: bool,
) -> (&mut Vec<(String, String)>, &mut Vec<StorageOp>) {
    if is_local {
        (&mut data.local_storage_map, &mut data.local_storage_ops)
    } else {
        (&mut data.session_storage_map, &mut data.session_storage_ops)
    }
}

fn storage_set_value(is_local: bool, k: String, v: String, context: &mut Context) -> JsResult<()> {
    ensure_tuple_origin(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let (map, ops) = storage_pair_mut(&mut data, is_local);
    let existing_idx = map.iter().position(|(ek, _)| ek == &k);
    let current_bytes: usize = map
        .iter()
        .enumerate()
        .filter(|(idx, _)| Some(*idx) != existing_idx)
        .map(|(_, (ek, ev))| ek.len().saturating_add(ev.len()))
        .sum();
    let new_bytes = current_bytes
        .saturating_add(k.len())
        .saturating_add(v.len());
    if new_bytes > MAX_STORAGE_QUOTA_BYTES {
        drop(data);
        return Err(dom_exception::throw(
            "QuotaExceededError",
            "Storage 512 KiB quota exceeded.",
            context,
        ));
    }
    if ops.len() >= MAX_STORAGE_OPS_PER_TX {
        // Coalesce an earlier Set for the same key if present
        if let Some(prev_op) = ops
            .iter_mut()
            .rev()
            .find(|op| matches!(op, StorageOp::Set { key, .. } if key == &k))
        {
            *prev_op = StorageOp::Set {
                key: k.clone(),
                value: v.clone(),
            };
        } else {
            drop(data);
            return Err(dom_exception::throw(
                "QuotaExceededError",
                "Storage operations per transaction limit exceeded.",
                context,
            ));
        }
    } else {
        ops.push(StorageOp::Set {
            key: k.clone(),
            value: v.clone(),
        });
    }
    if let Some(idx) = existing_idx {
        map[idx].1 = v;
    } else {
        map.push((k, v));
    }
    Ok(())
}

fn storage_remove_value(is_local: bool, k: &str, context: &mut Context) -> JsResult<()> {
    ensure_tuple_origin(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let (map, ops) = storage_pair_mut(&mut data, is_local);
    if let Some(idx) = map.iter().position(|(ek, _)| ek == k) {
        if ops.len() >= MAX_STORAGE_OPS_PER_TX {
            drop(data);
            return Err(dom_exception::throw(
                "QuotaExceededError",
                "Storage operations per transaction limit exceeded.",
                context,
            ));
        }
        map.remove(idx);
        ops.push(StorageOp::Remove { key: k.to_owned() });
    }
    Ok(())
}

fn storage_clear_all(is_local: bool, context: &mut Context) -> JsResult<()> {
    ensure_tuple_origin(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let (map, ops) = storage_pair_mut(&mut data, is_local);
    map.clear();
    ops.clear();
    ops.push(StorageOp::Clear);
    Ok(())
}

pub(super) fn storage_get(
    is_local: bool,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let property = key(&args[1], context)?;
    let Some(name) = property_name(&property) else {
        return super::fallback_get(args, context);
    };
    match name.as_str() {
        "length" => {
            let shared = state(context);
            let data = shared.borrow();
            let len = if is_local {
                data.local_storage_map.len()
            } else {
                data.session_storage_map.len()
            };
            Ok((len as f64).into())
        }
        "getItem" => super::method(STORAGE_GET_ITEM, context).map(Into::into),
        "setItem" => super::method(STORAGE_SET_ITEM, context).map(Into::into),
        "removeItem" => super::method(STORAGE_REMOVE_ITEM, context).map(Into::into),
        "clear" => super::method(STORAGE_CLEAR, context).map(Into::into),
        "key" => super::method(STORAGE_KEY, context).map(Into::into),
        _ => {
            if let Some(v) = storage_get_value(is_local, &name, &state(context).borrow()) {
                Ok(JsString::from(v).into())
            } else {
                super::fallback_get(args, context)
            }
        }
    }
}

pub(super) fn storage_set(
    is_local: bool,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let property = key(&args[1], context)?;
    let Some(name) = property_name(&property) else {
        return Err(failure("Symbol Storage properties are not supported"));
    };
    let val_str = utf8(&args[2], context)?;
    storage_set_value(is_local, name, val_str, context)?;
    Ok(true.into())
}

pub(super) fn storage_has(
    is_local: bool,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let property = key(&args[1], context)?;
    if let Some(name) = property_name(&property) {
        if matches!(
            name.as_str(),
            "length" | "getItem" | "setItem" | "removeItem" | "clear" | "key"
        ) || storage_get_value(is_local, &name, &state(context).borrow()).is_some()
        {
            return Ok(true.into());
        }
    }
    args[0]
        .as_object()
        .unwrap()
        .has_property(property, context)
        .map(Into::into)
}

pub(super) fn storage_delete(
    is_local: bool,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let property = key(&args[1], context)?;
    if let Some(name) = property_name(&property) {
        storage_remove_value(is_local, &name, context)?;
    }
    Ok(true.into())
}

pub(super) fn storage_own_keys(
    is_local: bool,
    _: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let keys: Vec<JsValue> = {
        let shared = state(context);
        let data = shared.borrow();
        let map = if is_local {
            &data.local_storage_map
        } else {
            &data.session_storage_map
        };
        map.iter()
            .map(|(k, _)| JsString::from(k.as_str()).into())
            .collect()
    };
    Ok(JsArray::from_iter(keys, context).into())
}

pub(super) fn storage_own_descriptor(
    is_local: bool,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let property = key(&args[1], context)?;
    if let Some(name) = property_name(&property)
        && let Some(val) = storage_get_value(is_local, &name, &state(context).borrow())
    {
        let desc = ObjectInitializer::new(context)
            .property(
                js_string!("value"),
                JsString::from(val),
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                js_string!("writable"),
                true,
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                js_string!("enumerable"),
                true,
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .property(
                js_string!("configurable"),
                true,
                Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE,
            )
            .build();
        return Ok(desc.into());
    }
    Ok(JsValue::undefined())
}

// --- `document.cookie` ---

pub(super) fn parse_initial_cookies(header: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in header.split(';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (name, val) = match trimmed.split_once('=') {
            Some((n, v)) => (n.trim().to_owned(), v.trim().to_owned()),
            None => (String::new(), trimmed.to_owned()),
        };
        if let Some(existing) = out
            .iter_mut()
            .find(|(ek, _): &&mut (String, String)| *ek == name)
        {
            existing.1 = val;
        } else {
            out.push((name, val));
        }
    }
    out
}

pub(super) fn document_cookie_get(context: &mut Context) -> JsResult<JsValue> {
    ensure_tuple_origin(context)?;
    let shared = state(context);
    let data = shared.borrow();
    let mut s = String::new();
    for (idx, (k, v)) in data.cookie_entries.iter().enumerate() {
        if idx > 0 {
            s.push_str("; ");
        }
        if k.is_empty() {
            s.push_str(v);
        } else {
            s.push_str(k);
            s.push('=');
            s.push_str(v);
        }
    }
    Ok(JsString::from(s).into())
}

pub(super) fn document_cookie_set(value: &JsValue, context: &mut Context) -> JsResult<()> {
    ensure_tuple_origin(context)?;
    let raw = utf8(value, context)?;
    if raw.len() > MAX_COOKIE_WRITE_BYTES
        || raw.bytes().any(|b| (b < 0x20 && b != b'\t') || b == 0x7F)
    {
        return Ok(());
    }
    let mut segments = raw.split(';');
    let Some(first) = segments.next() else {
        return Ok(());
    };
    let (name, mut val) = match first.split_once('=') {
        Some((n, v)) => (n.trim().to_owned(), v.trim().to_owned()),
        None => {
            let trimmed = first.trim();
            if trimmed.is_empty() {
                return Ok(());
            }
            (String::new(), trimmed.to_owned())
        }
    };
    if name.is_empty() && val.is_empty() {
        return Ok(());
    }
    if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
        val = val[1..val.len() - 1].to_owned();
    }

    let mut is_http_only = false;
    let mut is_secure = false;
    let mut delete_cookie = false;
    for attr in segments {
        let attr = attr.trim();
        if attr.is_empty() {
            continue;
        }
        let (aname, aval) = match attr.split_once('=') {
            Some((k, v)) => (k.trim(), Some(v.trim())),
            None => (attr, None),
        };
        if aname.eq_ignore_ascii_case("httponly") {
            is_http_only = true;
        } else if aname.eq_ignore_ascii_case("secure") {
            is_secure = true;
        } else if aname.eq_ignore_ascii_case("max-age") {
            if let Some(v) = aval
                && let Ok(secs) = v.parse::<i64>()
                && secs <= 0
            {
                delete_cookie = true;
            }
        } else if aname.eq_ignore_ascii_case("expires")
            && let Some(v) = aval
            && (v.contains("1970") || v.contains(" 00 ") || v.contains("-70 "))
        {
            delete_cookie = true;
        }
    }
    if is_http_only {
        // RFC 6265: non-HTTP APIs MUST ignore Set-Cookie strings containing HttpOnly
        return Ok(());
    }

    let shared = state(context);
    let mut data = shared.borrow_mut();
    if is_secure && data.host.url.scheme() != "https" {
        return Ok(());
    }
    if data.cookie_writes.len() >= MAX_COOKIE_WRITES_PER_TX {
        return Ok(());
    }
    if delete_cookie {
        data.cookie_entries.retain(|(ek, _)| *ek != name);
    } else if let Some(existing) = data.cookie_entries.iter_mut().find(|(ek, _)| *ek == name) {
        existing.1 = val;
    } else {
        data.cookie_entries.push((name, val));
    }
    data.cookie_writes.push(raw);
    Ok(())
}

// --- `window.history` (`history` handle) ---

fn history_receiver(this: &JsValue, context: &mut Context) -> JsResult<()> {
    match receiver(this, context)? {
        Value::Host(h) if h == "history" => Ok(()),
        _ => Err(failure("Invalid History receiver")),
    }
}

pub(super) fn try_get(
    handle: &str,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<JsValue>> {
    if handle == "document" && name == "cookie" {
        return document_cookie_get(context).map(Some);
    }
    if handle == "history" {
        return match name {
            "length" => Ok(Some((state(context).borrow().history_length as f64).into())),
            "scrollRestoration" => {
                let mode = state(context).borrow().scroll_restoration;
                Ok(Some(JsString::from(mode).into()))
            }
            "state" => {
                if let Some(cached) = state(context).borrow().cached_history_state.clone() {
                    return Ok(Some(cached));
                }
                let graph_opt = state(context).borrow().history_state_graph.clone();
                let Some(graph) = graph_opt else {
                    return Ok(Some(JsValue::null()));
                };
                let restored = message_clone::restore(&graph, context)?;
                state(context).borrow_mut().cached_history_state = Some(restored.clone());
                Ok(Some(restored))
            }
            "pushState" => super::method(HISTORY_PUSH_STATE, context).map(|o| Some(o.into())),
            "replaceState" => super::method(HISTORY_REPLACE_STATE, context).map(|o| Some(o.into())),
            "back" => super::method(HISTORY_BACK, context).map(|o| Some(o.into())),
            "forward" => super::method(HISTORY_FORWARD, context).map(|o| Some(o.into())),
            "go" => super::method(HISTORY_GO, context).map(|o| Some(o.into())),
            _ => Ok(None),
        };
    }
    Ok(None)
}

pub(super) fn try_set(
    handle: &str,
    name: &str,
    value: &JsValue,
    context: &mut Context,
) -> JsResult<bool> {
    if handle == "document" && name == "cookie" {
        document_cookie_set(value, context)?;
        return Ok(true);
    }
    if handle == "history" && name == "scrollRestoration" {
        let s = utf8(value, context)?;
        if s == "manual" {
            state(context).borrow_mut().scroll_restoration = "manual";
        } else if s == "auto" {
            state(context).borrow_mut().scroll_restoration = "auto";
        }
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn try_has(handle: &str, name: &str) -> Option<bool> {
    if handle == "document" && name == "cookie" {
        return Some(true);
    }
    if handle == "history"
        && matches!(
            name,
            "length"
                | "scrollRestoration"
                | "state"
                | "pushState"
                | "replaceState"
                | "back"
                | "forward"
                | "go"
        )
    {
        return Some(true);
    }
    None
}

fn history_push_or_replace(
    is_push: bool,
    state_arg: &JsValue,
    url_arg: Option<&JsValue>,
    context: &mut Context,
) -> JsResult<()> {
    ensure_tuple_origin(context)?;
    let graph = message_clone::serialize_structured(state_arg, &[], context)?;
    let state_json =
        message_clone::to_json_lossy(&graph).filter(|s| s.len() <= MAX_HISTORY_STATE_BYTES);

    let resolved_url = match url_arg {
        None => state(context).borrow().host.url.clone(),
        Some(v) if v.is_undefined() || v.is_null() => state(context).borrow().host.url.clone(),
        Some(v) => {
            let url_str = utf8(v, context)?;
            if url_str.is_empty() {
                state(context).borrow().host.url.clone()
            } else {
                let cur = state(context).borrow().host.url.clone();
                let Ok(joined) = cur.join(&url_str) else {
                    return Err(dom_exception::throw(
                        "SecurityError",
                        "Failed to parse target URL in History state update.",
                        context,
                    ));
                };
                if joined.as_str().len() > MAX_HISTORY_URL_BYTES
                    || (
                        joined.scheme(),
                        joined.host_str(),
                        joined.port_or_known_default(),
                    ) != (cur.scheme(), cur.host_str(), cur.port_or_known_default())
                {
                    return Err(dom_exception::throw(
                        "SecurityError",
                        "History URL must be same-origin with the document.",
                        context,
                    ));
                }
                joined
            }
        }
    };

    let shared = state(context);
    let mut data = shared.borrow_mut();
    if data.history_ops.len() >= MAX_HISTORY_OPS_PER_TX {
        drop(data);
        return Err(dom_exception::throw(
            "SecurityError",
            "Too many History API calls in a single transaction.",
            context,
        ));
    }
    data.next_history_state_id = data.next_history_state_id.wrapping_add(1).max(1);
    let state_id = data.next_history_state_id;
    let url_string = resolved_url.as_str().to_owned();
    data.host.url = resolved_url;
    data.history_states.insert(state_id, graph.clone());
    data.history_state_graph = Some(graph);
    data.cached_history_state = None;
    if is_push {
        data.history_length = data.history_length.saturating_add(1);
        data.history_ops.push(HistoryOp::PushState {
            url: url_string,
            state_id,
            state_json,
        });
    } else {
        data.history_ops.push(HistoryOp::ReplaceState {
            url: url_string,
            state_id,
            state_json,
        });
    }
    Ok(())
}

fn record_history_go(delta: i32, context: &mut Context) -> JsResult<()> {
    ensure_tuple_origin(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if data.history_ops.len() < MAX_HISTORY_OPS_PER_TX {
        data.history_ops.push(HistoryOp::Go { delta });
    }
    Ok(())
}
