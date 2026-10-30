//! Worker-isolated WHATWG / Chromium `console.*` API and interactive Console REPL.
//!
//! Security & semantic invariants:
//! - Never executes JS in the parent process; runs exclusively inside `BoaPageRealm`.
//! - Formatting is 100% structural and non-coercive: it never invokes user getters,
//!   `Proxy` traps, `toString`, `valueOf`, or `Symbol.toPrimitive`.
//! - Retains at most `MAX_RETAINED_OBJECTS` inspectable handles in a bounded FIFO
//!   ring and clears all retained roots on `clear()` / `ClearObjects` / realm close.
use super::*;
use crate::page_session::console::{
    ConsoleCommand, ConsolePropertyPreview, ConsoleResult, ConsoleValueKind, ConsoleWireEntry,
    ConsoleWireLevel, MAX_CONSOLE_ENTRIES_PER_TX, MAX_CONSOLE_PROPERTIES, MAX_CONSOLE_TEXT_BYTES,
};
use modern::boa_engine::{
    Script, Source, builtins::promise::PromiseState, object::builtins::JsArray, value::JsVariant,
};

const MAX_RETAINED_OBJECTS: usize = 64;
const MAX_COUNTERS: usize = 64;
const MAX_TIMERS: usize = 64;
const MAX_LABEL_BYTES: usize = 128;

#[derive(Trace, Finalize, Default)]
pub(super) struct ConsoleState {
    #[unsafe_ignore_trace]
    pub entries: Vec<ConsoleWireEntry>,
    #[unsafe_ignore_trace]
    pub group_depth: u8,
    #[unsafe_ignore_trace]
    counts: HashMap<String, u64>,
    #[unsafe_ignore_trace]
    timers: HashMap<String, Instant>,
    pub last_result: Option<JsValue>,
    objects: Vec<(u64, JsObject)>,
    #[unsafe_ignore_trace]
    next_object_id: u64,
}

impl ConsoleState {
    pub(super) fn clear_roots(&mut self) {
        self.last_result = None;
        self.objects.clear();
    }

    fn retain_object(&mut self, obj: JsObject) -> u64 {
        for (id, existing) in &self.objects {
            if JsObject::equals(existing, &obj) {
                return *id;
            }
        }
        self.next_object_id = self.next_object_id.saturating_add(1).max(1);
        let id = self.next_object_id;
        if self.objects.len() >= MAX_RETAINED_OBJECTS {
            self.objects.remove(0);
        }
        self.objects.push((id, obj));
        id
    }

    fn find_object(&self, id: u64) -> Option<JsObject> {
        self.objects
            .iter()
            .find_map(|(stored, obj)| (*stored == id).then(|| obj.clone()))
    }

    fn push_entry(&mut self, mut entry: ConsoleWireEntry) {
        if self.entries.len() >= MAX_CONSOLE_ENTRIES_PER_TX {
            return;
        }
        entry.text = truncate_utf8(&entry.text, MAX_CONSOLE_TEXT_BYTES);
        self.entries.push(entry);
    }
}

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        "host.console.log" => Some(("log", 0)),
        "host.console.info" => Some(("info", 0)),
        "host.console.debug" => Some(("debug", 0)),
        "host.console.warn" => Some(("warn", 0)),
        "host.console.error" => Some(("error", 0)),
        "host.console.assert" => Some(("assert", 0)),
        "host.console.clear" => Some(("clear", 0)),
        "host.console.count" => Some(("count", 0)),
        "host.console.countReset" => Some(("countReset", 0)),
        "host.console.time" => Some(("time", 0)),
        "host.console.timeLog" => Some(("timeLog", 0)),
        "host.console.timeEnd" => Some(("timeEnd", 0)),
        "host.console.group" => Some(("group", 0)),
        "host.console.groupCollapsed" => Some(("groupCollapsed", 0)),
        "host.console.groupEnd" => Some(("groupEnd", 0)),
        "host.console.dir" => Some(("dir", 0)),
        "host.console.dirxml" => Some(("dirxml", 0)),
        "host.console.table" => Some(("table", 0)),
        "host.console.trace" => Some(("trace", 0)),
        _ => None,
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes.saturating_sub(1);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 3);
    out.push_str(&text[..end]);
    out.push('…');
    out
}

fn label_from_arg(arg: Option<&JsValue>, context: &mut Context) -> String {
    match arg {
        None => "default".into(),
        Some(v) if v.is_undefined() => "default".into(),
        Some(v) => truncate_utf8(&format_value_inline(v, false, 0, context), MAX_LABEL_BYTES),
    }
}

fn host_handle(obj: &JsObject, context: &mut Context) -> Option<String> {
    let brands = state(context).borrow().brands.clone();
    let val = brands.get(obj, context).ok()?;
    val.as_string().map(|s| s.to_std_string_lossy())
}

fn node_id_from_handle(handle: &str) -> Option<usize> {
    if handle == "document" {
        Some(0)
    } else {
        handle.strip_prefix("node:")?.parse::<usize>().ok()
    }
}

fn format_dom_handle(handle: &str, context: &Context) -> (ConsoleValueKind, String, Option<usize>) {
    let shared = state(context);
    let data = shared.borrow();
    if handle == "document" {
        return (
            ConsoleValueKind::Node,
            format!("#document ({})", data.host.url),
            Some(0),
        );
    }
    if handle == "location" {
        return (
            ConsoleValueKind::Object,
            format!("Location {{ href: {:?} }}", data.host.url.as_str()),
            None,
        );
    }
    if handle == "navigator" {
        return (
            ConsoleValueKind::Object,
            format!("Navigator {{ userAgent: {:?} }}", crate::USER_AGENT),
            None,
        );
    }
    if handle == "console" {
        return (ConsoleValueKind::Object, "console {…}".into(), None);
    }
    if let Some(style_node) = handle.strip_prefix("style:") {
        return (
            ConsoleValueKind::Object,
            format!("CSSStyleDeclaration (node {style_node})"),
            None,
        );
    }
    if let Some(id) = handle
        .strip_prefix("node:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        if let Some(node) = data.host.document.nodes.get(id) {
            if node.tag == "#text" {
                let snippet = truncate_utf8(node.text.trim(), 48);
                return (
                    ConsoleValueKind::Node,
                    format!("#text {snippet:?}"),
                    Some(id),
                );
            }
            if node.tag == "#comment" {
                let snippet = truncate_utf8(node.text.trim(), 48);
                return (
                    ConsoleValueKind::Node,
                    format!("<!-- {snippet} -->"),
                    Some(id),
                );
            }
            let mut label = format!("<{}", node.tag);
            if let Some(el_id) = node.attr("id").filter(|s| !s.is_empty()) {
                label.push('#');
                label.push_str(&truncate_utf8(el_id, 32));
            }
            if let Some(cls) = node.attr("class").filter(|s| !s.trim().is_empty()) {
                for token in cls.split_whitespace().take(3) {
                    label.push('.');
                    label.push_str(&truncate_utf8(token, 24));
                }
            } else if let Some(href) = node.attr("href").filter(|s| !s.is_empty()) {
                label.push_str(&format!(" href={:?}", truncate_utf8(href, 36)));
            }
            label.push('>');
            return (ConsoleValueKind::Node, label, Some(id));
        }
    }
    (ConsoleValueKind::Object, format!("{handle} {{…}}"), None)
}

fn own_data_prop(obj: &JsObject, key: PropertyKey) -> Option<JsValue> {
    let borrow = obj.try_borrow().ok()?;
    let desc = borrow.properties().get(&key)?;
    desc.value().cloned()
}

fn property_key_name(key: &PropertyKey) -> String {
    match key {
        PropertyKey::String(s) => s.to_std_string_lossy(),
        PropertyKey::Index(i) => i.get().to_string(),
        PropertyKey::Symbol(sym) => match sym.description() {
            Some(desc) => format!("[Symbol({})]", desc.to_std_string_lossy()),
            None => "[Symbol()]".into(),
        },
    }
}

fn format_number(n: f64) -> String {
    if n == 0.0 && n.is_sign_negative() {
        "-0".into()
    } else if n.is_nan() {
        "NaN".into()
    } else if n == f64::INFINITY {
        "Infinity".into()
    } else if n == f64::NEG_INFINITY {
        "-Infinity".into()
    } else {
        n.to_string()
    }
}

/// Non-coercive structural JS value previewer. Never invokes user JS code.
pub(super) fn classify_and_format(
    value: &JsValue,
    quote_strings: bool,
    depth: usize,
    context: &mut Context,
) -> (ConsoleValueKind, String, Option<JsObject>, Option<usize>) {
    match value.variant() {
        JsVariant::Undefined => (ConsoleValueKind::Undefined, "undefined".into(), None, None),
        JsVariant::Null => (ConsoleValueKind::Null, "null".into(), None, None),
        JsVariant::Boolean(b) => (ConsoleValueKind::Boolean, b.to_string(), None, None),
        JsVariant::Integer32(n) => (ConsoleValueKind::Number, n.to_string(), None, None),
        JsVariant::Float64(n) => (ConsoleValueKind::Number, format_number(n), None, None),
        JsVariant::BigInt(b) => {
            let text = if b.as_inner().bits() <= 4096 {
                format!("{b}n")
            } else {
                "<BigInt>".into()
            };
            (ConsoleValueKind::BigInt, text, None, None)
        }
        JsVariant::String(s) => {
            let raw = s.to_std_string_lossy();
            let formatted = if quote_strings {
                format!("{:?}", truncate_utf8(&raw, 240))
            } else {
                truncate_utf8(&raw, MAX_CONSOLE_TEXT_BYTES)
            };
            (ConsoleValueKind::String, formatted, None, None)
        }
        JsVariant::Symbol(sym) => {
            let text = match sym.description() {
                Some(desc) => format!("Symbol({})", desc.to_std_string_lossy()),
                None => "Symbol()".into(),
            };
            (ConsoleValueKind::Symbol, text, None, None)
        }
        JsVariant::Object(obj) => {
            if JsObject::equals(&obj, &context.global_object()) {
                return (
                    ConsoleValueKind::Object,
                    "Window {…}".into(),
                    Some(obj),
                    None,
                );
            }
            if let Some(handle) = host_handle(&obj, context) {
                let (kind, preview, node_id) = format_dom_handle(&handle, context);
                return (kind, preview, Some(obj), node_id);
            }
            if obj.is::<modern::boa_engine::builtins::proxy::Proxy>() {
                return (
                    ConsoleValueKind::Object,
                    "Proxy {…}".into(),
                    Some(obj),
                    None,
                );
            }
            if obj.is_callable() {
                let name = own_data_prop(&obj, js_string!("name").into())
                    .and_then(|v| v.as_string().map(|s| s.to_std_string_lossy()))
                    .unwrap_or_default();
                let preview = if name.is_empty() {
                    "ƒ ()".into()
                } else {
                    format!("ƒ {}()", truncate_utf8(&name, 48))
                };
                return (ConsoleValueKind::Function, preview, Some(obj), None);
            }
            if obj.is::<modern::boa_engine::builtins::error::Error>() {
                let name = own_data_prop(&obj, js_string!("name").into())
                    .and_then(|v| v.as_string().map(|s| s.to_std_string_lossy()))
                    .unwrap_or_else(|| "Error".into());
                let msg = own_data_prop(&obj, js_string!("message").into())
                    .and_then(|v| v.as_string().map(|s| s.to_std_string_lossy()))
                    .unwrap_or_default();
                let preview = if msg.is_empty() {
                    name
                } else {
                    format!("{name}: {}", truncate_utf8(&msg, 240))
                };
                return (ConsoleValueKind::Error, preview, Some(obj), None);
            }
            if let Ok(promise) = JsPromise::from_object(obj.clone()) {
                let preview = match promise.state() {
                    PromiseState::Pending => "Promise {<pending>}".into(),
                    PromiseState::Fulfilled(v) => {
                        let inner = format_value_inline(&v, true, depth + 1, context);
                        format!("Promise {{<fulfilled>: {inner}}}")
                    }
                    PromiseState::Rejected(v) => {
                        let inner = format_value_inline(&v, true, depth + 1, context);
                        format!("Promise {{<rejected>: {inner}}}")
                    }
                };
                return (ConsoleValueKind::Object, preview, Some(obj), None);
            }
            if obj.is_array() {
                let len = own_data_prop(&obj, js_string!("length").into())
                    .and_then(|v| v.as_number())
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                if depth >= 2 {
                    return (
                        ConsoleValueKind::Array,
                        format!("Array({len})"),
                        Some(obj),
                        None,
                    );
                }
                let mut items = Vec::new();
                let shown = len.min(6);
                for i in 0..shown {
                    let elem = match obj
                        .try_borrow()
                        .ok()
                        .and_then(|b| b.properties().get(&PropertyKey::from(i as u32)))
                    {
                        Some(desc) => match desc.value() {
                            Some(val) => format_value_inline(val, true, depth + 1, context),
                            None => "(…)".into(),
                        },
                        None => "empty".into(),
                    };
                    items.push(elem);
                }
                if len > shown {
                    items.push(format!("… +{}", len - shown));
                }
                let prefix = if len > 0 {
                    format!("({len}) ")
                } else {
                    String::new()
                };
                return (
                    ConsoleValueKind::Array,
                    format!("{prefix}[{}]", items.join(", ")),
                    Some(obj),
                    None,
                );
            }
            if depth >= 2 {
                return (ConsoleValueKind::Object, "{…}".into(), Some(obj), None);
            }
            let keys = obj.own_property_keys(context).unwrap_or_default();
            let mut pairs = Vec::new();
            let mut extra = 0usize;
            for key in keys {
                let Some(desc) = obj.try_borrow().ok().and_then(|b| b.properties().get(&key))
                else {
                    continue;
                };
                if desc.enumerable() != Some(true) && !matches!(key, PropertyKey::Symbol(_)) {
                    continue;
                }
                if pairs.len() >= 5 {
                    extra += 1;
                    continue;
                }
                let k_str = property_key_name(&key);
                let v_str = match desc.value() {
                    Some(val) => format_value_inline(val, true, depth + 1, context),
                    None => "(…)".into(),
                };
                pairs.push(format!("{k_str}: {v_str}"));
            }
            if extra > 0 {
                pairs.push("…".into());
            }
            let preview = if pairs.is_empty() {
                "{}".into()
            } else {
                format!("{{{}}}", pairs.join(", "))
            };
            (ConsoleValueKind::Object, preview, Some(obj), None)
        }
    }
}

fn format_value_inline(
    value: &JsValue,
    quote_strings: bool,
    depth: usize,
    context: &mut Context,
) -> String {
    classify_and_format(value, quote_strings, depth, context).1
}

fn numeric_arg(value: &JsValue) -> Option<f64> {
    match value.variant() {
        JsVariant::Integer32(n) => Some(f64::from(n)),
        JsVariant::Float64(n) => Some(n),
        JsVariant::Boolean(b) => Some(if b { 1.0 } else { 0.0 }),
        JsVariant::Null => Some(0.0),
        JsVariant::String(s) => s.to_std_string_lossy().trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn safe_json(value: &JsValue, depth: usize, context: &mut Context) -> String {
    if depth > 4 {
        return "\"[Object]\"".into();
    }
    match value.variant() {
        JsVariant::Undefined | JsVariant::Symbol(_) => "null".into(),
        JsVariant::Null => "null".into(),
        JsVariant::Boolean(b) => b.to_string(),
        JsVariant::Integer32(n) => n.to_string(),
        JsVariant::Float64(n) if n.is_finite() => n.to_string(),
        JsVariant::Float64(_) => "null".into(),
        JsVariant::BigInt(b) => format!("{:?}", b.to_string()),
        JsVariant::String(s) => format!("{:?}", truncate_utf8(&s.to_std_string_lossy(), 200)),
        JsVariant::Object(obj) => {
            if obj.is::<modern::boa_engine::builtins::proxy::Proxy>() || obj.is_callable() {
                return "null".into();
            }
            if obj.is_array() {
                let len = own_data_prop(&obj, js_string!("length").into())
                    .and_then(|v| v.as_number())
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                let mut out = Vec::new();
                for i in 0..len.min(16) {
                    let elem = obj
                        .try_borrow()
                        .ok()
                        .and_then(|b| b.properties().get(&PropertyKey::from(i as u32)))
                        .and_then(|d| d.value().cloned())
                        .unwrap_or_default();
                    out.push(safe_json(&elem, depth + 1, context));
                }
                return format!("[{}]", out.join(","));
            }
            let keys = obj.own_property_keys(context).unwrap_or_default();
            let mut out = Vec::new();
            for key in keys.into_iter().take(16) {
                let (PropertyKey::String(_) | PropertyKey::Index(_)) = key else {
                    continue;
                };
                let Some(desc) = obj.try_borrow().ok().and_then(|b| b.properties().get(&key))
                else {
                    continue;
                };
                if desc.enumerable() != Some(true) {
                    continue;
                }
                if let Some(val) = desc.value() {
                    if val.is_undefined() || val.is_symbol() {
                        continue;
                    }
                    let k = property_key_name(&key);
                    out.push(format!("{:?}:{}", k, safe_json(val, depth + 1, context)));
                }
            }
            format!("{{{}}}", out.join(","))
        }
    }
}

/// Formats `console.log` / `warn` / `error` / `info` / `debug` arguments with
/// WHATWG `%s`, `%d`, `%i`, `%f`, `%o`, `%O`, `%j`, `%c`, and `%%` specifiers.
fn format_console_args(
    args: &[JsValue],
    context: &mut Context,
) -> (String, Option<JsObject>, Option<usize>) {
    if args.is_empty() {
        return (String::new(), None, None);
    }
    if args.len() == 1 {
        let (_, text, obj, node_id) = classify_and_format(&args[0], false, 0, context);
        return (text, obj, node_id);
    }
    let mut primary_obj: Option<JsObject> = None;
    let mut primary_node: Option<usize> = None;
    let mut out = String::new();
    let mut index = 0usize;

    if let Some(first_str) = args[0].as_string().map(|s| s.to_std_string_lossy())
        && first_str.contains('%')
    {
        index = 1;
        let chars: Vec<char> = first_str.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] == '%' && i + 1 < chars.len() {
                let spec = chars[i + 1];
                if spec == '%' {
                    out.push('%');
                    i += 2;
                    continue;
                }
                if index < args.len() {
                    let arg = &args[index];
                    match spec {
                        's' => {
                            out.push_str(&format_value_inline(arg, false, 0, context));
                            index += 1;
                            i += 2;
                            continue;
                        }
                        'd' | 'i' => {
                            match numeric_arg(arg) {
                                Some(n) if n.is_finite() => {
                                    out.push_str(&format!("{}", n.trunc() as i64))
                                }
                                Some(n) => out.push_str(&format_number(n)),
                                None => out.push_str("NaN"),
                            }
                            index += 1;
                            i += 2;
                            continue;
                        }
                        'f' => {
                            match numeric_arg(arg) {
                                Some(n) => out.push_str(&format_number(n)),
                                None => out.push_str("NaN"),
                            }
                            index += 1;
                            i += 2;
                            continue;
                        }
                        'o' | 'O' => {
                            let (_, preview, obj, nid) = classify_and_format(arg, true, 0, context);
                            if primary_obj.is_none() {
                                primary_obj = obj;
                                primary_node = nid;
                            }
                            out.push_str(&preview);
                            index += 1;
                            i += 2;
                            continue;
                        }
                        'j' => {
                            out.push_str(&safe_json(arg, 0, context));
                            index += 1;
                            i += 2;
                            continue;
                        }
                        'c' => {
                            // Consume CSS style specifier cleanly without printing raw CSS.
                            index += 1;
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            out.push(chars[i]);
            i += 1;
        }
    }

    for arg in &args[index..] {
        if !out.is_empty() {
            out.push(' ');
        }
        let (_, preview, obj, nid) = classify_and_format(arg, false, 0, context);
        if primary_obj.is_none() {
            primary_obj = obj;
            primary_node = nid;
        }
        out.push_str(&preview);
    }
    (out, primary_obj, primary_node)
}

fn format_table(
    data: &JsValue,
    columns_arg: Option<&JsValue>,
    context: &mut Context,
) -> Option<(String, JsObject)> {
    let obj = data.as_object()?;
    if obj.is::<modern::boa_engine::builtins::proxy::Proxy>()
        || host_handle(&obj, context).is_some()
    {
        return None;
    }
    let filter_cols: Option<Vec<String>> = columns_arg
        .and_then(JsValue::as_object)
        .filter(|c| c.is_array())
        .map(|arr| {
            let len = own_data_prop(&arr, js_string!("length").into())
                .and_then(|v| v.as_number())
                .unwrap_or(0.0)
                .max(0.0) as usize;
            let mut cols = Vec::new();
            for i in 0..len.min(12) {
                if let Some(s) = own_data_prop(&arr, PropertyKey::from(i as u32))
                    .and_then(|v| v.as_string().map(|s| s.to_std_string_lossy()))
                {
                    cols.push(s);
                }
            }
            cols
        });

    let mut row_keys: Vec<(String, JsValue)> = Vec::new();
    if obj.is_array() {
        let len = own_data_prop(&obj, js_string!("length").into())
            .and_then(|v| v.as_number())
            .unwrap_or(0.0)
            .max(0.0) as usize;
        for i in 0..len.min(20) {
            if let Some(v) = own_data_prop(&obj, PropertyKey::from(i as u32)) {
                row_keys.push((i.to_string(), v));
            }
        }
    } else {
        for key in obj.own_property_keys(context).unwrap_or_default() {
            if row_keys.len() >= 20 {
                break;
            }
            let Some(desc) = obj.try_borrow().ok().and_then(|b| b.properties().get(&key)) else {
                continue;
            };
            if desc.enumerable() != Some(true) {
                continue;
            }
            if let Some(v) = desc.value().cloned() {
                row_keys.push((property_key_name(&key), v));
            }
        }
    }
    if row_keys.is_empty() {
        return None;
    }

    let mut columns: Vec<String> = Vec::new();
    let mut has_values_col = false;
    for (_, row_val) in &row_keys {
        if let Some(row_obj) = row_val
            .as_object()
            .filter(|o| !o.is::<modern::boa_engine::builtins::proxy::Proxy>() && !o.is_callable())
        {
            for key in row_obj.own_property_keys(context).unwrap_or_default() {
                let Some(desc) = row_obj
                    .try_borrow()
                    .ok()
                    .and_then(|b| b.properties().get(&key))
                else {
                    continue;
                };
                if desc.enumerable() != Some(true) {
                    continue;
                }
                let col_name = property_key_name(&key);
                if let Some(allowed) = &filter_cols
                    && !allowed.contains(&col_name)
                {
                    continue;
                }
                if !columns.contains(&col_name) && columns.len() < 8 {
                    columns.push(col_name);
                }
            }
        } else {
            has_values_col = true;
        }
    }
    if has_values_col {
        columns.push("Values".into());
    }

    let mut headers = vec!["(index)".to_owned()];
    headers.extend(columns.iter().cloned());
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count().max(3)).collect();
    let mut matrix: Vec<Vec<String>> = Vec::with_capacity(row_keys.len());

    for (idx_label, row_val) in &row_keys {
        let mut row = Vec::with_capacity(headers.len());
        let idx_cell = truncate_utf8(idx_label, 18);
        widths[0] = widths[0].max(idx_cell.chars().count());
        row.push(idx_cell);

        for (col_i, col) in columns.iter().enumerate() {
            let cell = if col == "Values" && row_val.as_object().is_none() {
                truncate_utf8(&format_value_inline(row_val, true, 1, context), 24)
            } else if let Some(row_obj) = row_val.as_object() {
                let key = if let Ok(n) = col.parse::<u32>() {
                    PropertyKey::from(n)
                } else {
                    JsString::from(col.as_str()).into()
                };
                match own_data_prop(&row_obj, key) {
                    Some(v) => truncate_utf8(&format_value_inline(&v, true, 1, context), 24),
                    None => String::new(),
                }
            } else {
                String::new()
            };
            widths[col_i + 1] = widths[col_i + 1].max(cell.chars().count());
            row.push(cell);
        }
        matrix.push(row);
    }

    let pad = |s: &str, w: usize| -> String {
        let len = s.chars().count();
        format!(" {s}{} ", " ".repeat(w.saturating_sub(len)))
    };
    let rule = |left: char, mid: char, right: char| -> String {
        let segs: Vec<String> = widths.iter().map(|w| "─".repeat(*w + 2)).collect();
        format!("{left}{}{right}", segs.join(&mid.to_string()))
    };

    let mut lines = Vec::with_capacity(matrix.len() + 4);
    lines.push(rule('┌', '┬', '┐'));
    let hdr_cells: Vec<String> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| pad(h, widths[i]))
        .collect();
    lines.push(format!("│{}│", hdr_cells.join("│")));
    lines.push(rule('├', '┼', '┤'));
    for row in &matrix {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| pad(c, widths[i]))
            .collect();
        lines.push(format!("│{}│", cells.join("│")));
    }
    lines.push(rule('└', '┴', '┘'));
    Some((lines.join("\n"), obj))
}

pub(super) fn call(
    method: &str,
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let shared = state(context);
    match method {
        "log" | "info" | "debug" | "warn" | "error" => {
            let level = match method {
                "info" => ConsoleWireLevel::Info,
                "debug" => ConsoleWireLevel::Debug,
                "warn" => ConsoleWireLevel::Warn,
                "error" => ConsoleWireLevel::Error,
                _ => ConsoleWireLevel::Log,
            };
            let (text, obj, node_id) = format_console_args(args, context);
            let mut data = shared.borrow_mut();
            let depth = data.console.group_depth;
            let object_id = obj.map(|o| data.console.retain_object(o));
            data.console.push_entry(ConsoleWireEntry {
                level,
                text,
                object_id,
                node_id,
                group_depth: depth,
            });
        }
        "assert" => {
            let cond = args.first().is_some_and(JsValue::to_boolean);
            if !cond {
                let rest = args.get(1..).unwrap_or_default();
                let (suffix, obj, node_id) = format_console_args(rest, context);
                let text = if suffix.is_empty() {
                    "Assertion failed: console.assert".into()
                } else {
                    format!("Assertion failed: {suffix}")
                };
                let mut data = shared.borrow_mut();
                let depth = data.console.group_depth;
                let object_id = obj.map(|o| data.console.retain_object(o));
                data.console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::Error,
                    text,
                    object_id,
                    node_id,
                    group_depth: depth,
                });
            }
        }
        "clear" => {
            let mut data = shared.borrow_mut();
            data.console.entries.clear();
            data.console.group_depth = 0;
            data.console.clear_roots();
            data.console.push_entry(ConsoleWireEntry {
                level: ConsoleWireLevel::Clear,
                text: "Console was cleared".into(),
                object_id: None,
                node_id: None,
                group_depth: 0,
            });
        }
        "count" => {
            let label = label_from_arg(args.first(), context);
            let mut data = shared.borrow_mut();
            if !data.console.counts.contains_key(&label)
                && data.console.counts.len() >= MAX_COUNTERS
            {
                return Ok(JsValue::undefined());
            }
            let count = data.console.counts.entry(label.clone()).or_insert(0);
            *count = count.saturating_add(1);
            let current = *count;
            let depth = data.console.group_depth;
            data.console.push_entry(ConsoleWireEntry {
                level: ConsoleWireLevel::Info,
                text: format!("{label}: {current}"),
                object_id: None,
                node_id: None,
                group_depth: depth,
            });
        }
        "countReset" => {
            let label = label_from_arg(args.first(), context);
            let mut data = shared.borrow_mut();
            let depth = data.console.group_depth;
            if let Some(count) = data.console.counts.get_mut(&label) {
                *count = 0;
            } else {
                data.console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::Warn,
                    text: format!("Count for '{label}' does not exist"),
                    object_id: None,
                    node_id: None,
                    group_depth: depth,
                });
            }
        }
        "time" => {
            let label = label_from_arg(args.first(), context);
            let mut data = shared.borrow_mut();
            let now = data.now();
            let depth = data.console.group_depth;
            if data.console.timers.contains_key(&label) {
                data.console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::Warn,
                    text: format!("Timer '{label}' already exists"),
                    object_id: None,
                    node_id: None,
                    group_depth: depth,
                });
            } else if data.console.timers.len() < MAX_TIMERS {
                data.console.timers.insert(label, now);
            }
        }
        "timeLog" | "timeEnd" => {
            let label = label_from_arg(args.first(), context);
            let extra = if method == "timeLog" {
                let rest = args.get(1..).unwrap_or_default();
                format_console_args(rest, context).0
            } else {
                String::new()
            };
            let mut data = shared.borrow_mut();
            let now = data.now();
            let depth = data.console.group_depth;
            let started = if method == "timeEnd" {
                data.console.timers.remove(&label)
            } else {
                data.console.timers.get(&label).copied()
            };
            if let Some(started) = started {
                let ms = now.saturating_duration_since(started).as_secs_f64() * 1000.0;
                let text = if extra.is_empty() {
                    format!("{label}: {ms:.2} ms")
                } else {
                    format!("{label}: {ms:.2} ms {extra}")
                };
                data.console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::Info,
                    text,
                    object_id: None,
                    node_id: None,
                    group_depth: depth,
                });
            } else {
                data.console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::Warn,
                    text: format!("Timer '{label}' does not exist"),
                    object_id: None,
                    node_id: None,
                    group_depth: depth,
                });
            }
        }
        "group" | "groupCollapsed" => {
            let (mut label, obj, node_id) = format_console_args(args, context);
            if label.is_empty() {
                label = "console.group".into();
            }
            let level = if method == "groupCollapsed" {
                ConsoleWireLevel::GroupCollapsed
            } else {
                ConsoleWireLevel::Group
            };
            let mut data = shared.borrow_mut();
            let depth = data.console.group_depth;
            let object_id = obj.map(|o| data.console.retain_object(o));
            data.console.push_entry(ConsoleWireEntry {
                level,
                text: label,
                object_id,
                node_id,
                group_depth: depth,
            });
            data.console.group_depth = data.console.group_depth.saturating_add(1).min(16);
        }
        "groupEnd" => {
            let mut data = shared.borrow_mut();
            data.console.group_depth = data.console.group_depth.saturating_sub(1);
            let depth = data.console.group_depth;
            data.console.push_entry(ConsoleWireEntry {
                level: ConsoleWireLevel::GroupEnd,
                text: String::new(),
                object_id: None,
                node_id: None,
                group_depth: depth,
            });
        }
        "dir" | "dirxml" => {
            let val = args.first().cloned().unwrap_or_default();
            let (_, preview, obj, node_id) = classify_and_format(&val, true, 0, context);
            let mut data = shared.borrow_mut();
            let depth = data.console.group_depth;
            let object_id = obj.map(|o| data.console.retain_object(o));
            data.console.push_entry(ConsoleWireEntry {
                level: ConsoleWireLevel::Dir,
                text: preview,
                object_id,
                node_id,
                group_depth: depth,
            });
        }
        "table" => {
            let first = args.first().cloned().unwrap_or_default();
            if let Some((table_text, obj)) = format_table(&first, args.get(1), context) {
                let mut data = shared.borrow_mut();
                let depth = data.console.group_depth;
                let object_id = Some(data.console.retain_object(obj));
                data.console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::Table,
                    text: table_text,
                    object_id,
                    node_id: None,
                    group_depth: depth,
                });
            } else {
                let (text, obj, node_id) = format_console_args(args, context);
                let mut data = shared.borrow_mut();
                let depth = data.console.group_depth;
                let object_id = obj.map(|o| data.console.retain_object(o));
                data.console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::Log,
                    text,
                    object_id,
                    node_id,
                    group_depth: depth,
                });
            }
        }
        "trace" => {
            let (msg, obj, node_id) = format_console_args(args, context);
            let url = shared.borrow().host.url.as_str().to_owned();
            let text = if msg.is_empty() {
                format!("console.trace\n    at {url}")
            } else {
                format!("console.trace: {msg}\n    at {url}")
            };
            let mut data = shared.borrow_mut();
            let depth = data.console.group_depth;
            let object_id = obj.map(|o| data.console.retain_object(o));
            data.console.push_entry(ConsoleWireEntry {
                level: ConsoleWireLevel::Info,
                text,
                object_id,
                node_id,
                group_depth: depth,
            });
        }
        _ => {}
    }
    Ok(JsValue::undefined())
}

fn inspect_properties(obj: &JsObject, context: &mut Context) -> Vec<ConsolePropertyPreview> {
    let mut out = Vec::new();
    if let Some(handle) = host_handle(obj, context) {
        if let Some(id) = node_id_from_handle(&handle) {
            let dom_props = {
                let shared = state(context);
                let data = shared.borrow();
                if let Some(node) = data.host.document.nodes.get(id) {
                    let child_elements = node
                        .children
                        .iter()
                        .filter(|&&c| {
                            data.host
                                .document
                                .nodes
                                .get(c)
                                .is_some_and(|n| !n.tag.starts_with('#'))
                        })
                        .count();
                    let text_val = truncate_utf8(&text_content(&data.host.document.nodes, id), 120);
                    let mut v = vec![
                        (
                            "nodeName".to_owned(),
                            format!("{:?}", node.tag.to_ascii_uppercase()),
                            ConsoleValueKind::String,
                        ),
                        (
                            "id".to_owned(),
                            format!("{:?}", node.attr("id").unwrap_or("")),
                            ConsoleValueKind::String,
                        ),
                        (
                            "className".to_owned(),
                            format!("{:?}", node.attr("class").unwrap_or("")),
                            ConsoleValueKind::String,
                        ),
                        (
                            "childElementCount".to_owned(),
                            child_elements.to_string(),
                            ConsoleValueKind::Number,
                        ),
                        (
                            "textContent".to_owned(),
                            format!("{text_val:?}"),
                            ConsoleValueKind::String,
                        ),
                    ];
                    if let Some(href) = node.attr("href") {
                        v.push((
                            "href".to_owned(),
                            format!("{:?}", truncate_utf8(href, 120)),
                            ConsoleValueKind::String,
                        ));
                    }
                    v
                } else {
                    Vec::new()
                }
            };
            for (name, value, kind) in dom_props {
                out.push(ConsolePropertyPreview {
                    name,
                    value,
                    kind,
                    object_id: None,
                    node_id: None,
                    enumerable: true,
                });
            }
            return out;
        }
    }
    if obj.is::<modern::boa_engine::builtins::proxy::Proxy>() {
        return out;
    }

    let keys = obj.own_property_keys(context).unwrap_or_default();
    let shared = state(context);
    for key in keys.into_iter().take(MAX_CONSOLE_PROPERTIES) {
        let Some(desc) = obj.try_borrow().ok().and_then(|b| b.properties().get(&key)) else {
            continue;
        };
        let enumerable = desc.enumerable().unwrap_or(false);
        let name = property_key_name(&key);
        if let Some(val) = desc.value() {
            let (kind, preview, child_obj, node_id) = classify_and_format(val, true, 1, context);
            let object_id = child_obj.map(|o| shared.borrow_mut().console.retain_object(o));
            out.push(ConsolePropertyPreview {
                name,
                value: preview,
                kind,
                object_id,
                node_id,
                enumerable,
            });
        } else {
            let mut label = Vec::new();
            if desc.get().is_some_and(|g| !g.is_undefined()) {
                label.push("getter");
            }
            if desc.set().is_some_and(|s| !s.is_undefined()) {
                label.push("setter");
            }
            out.push(ConsolePropertyPreview {
                name,
                value: format!("({})", label.join("/")),
                kind: ConsoleValueKind::Function,
                object_id: None,
                node_id: None,
                enumerable,
            });
        }
    }
    out
}

fn install_command_line_api(
    realm: &mut BoaPageRealm,
    inspected_nodes: &[usize],
) -> Vec<(JsString, JsValue)> {
    let mut installed = Vec::new();
    let context = realm.engine.context_mut();
    let global = context.global_object();

    let mut define_temp = |name: &str, value: JsValue, context: &mut Context| {
        let key = JsString::from(name);
        if global
            .has_own_property(key.clone(), context)
            .unwrap_or(true)
        {
            return;
        }
        if global
            .define_property_or_throw(
                key.clone(),
                PropertyDescriptor::builder()
                    .value(value.clone())
                    .writable(true)
                    .enumerable(false)
                    .configurable(true),
                context,
            )
            .is_ok()
        {
            installed.push((key, value));
        }
    };

    let last = state(context)
        .borrow()
        .console
        .last_result
        .clone()
        .unwrap_or_default();
    define_temp("$_", last, context);

    for slot in 0..5 {
        let name = format!("${slot}");
        let node_val = inspected_nodes
            .get(slot)
            .copied()
            .and_then(|id| {
                let exists = state(context)
                    .borrow()
                    .host
                    .document
                    .nodes
                    .get(id)
                    .is_some();
                if !exists {
                    return None;
                }
                let handle = if id == 0 {
                    "document".to_owned()
                } else {
                    format!("node:{id}")
                };
                wrap(&handle, context).ok()
            })
            .unwrap_or_default();
        define_temp(&name, node_val, context);
    }

    // $(selector, startNode?) -> Element | null
    let dollar = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|_, args, context| {
            modern::check_context(context)?;
            let selector = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
            let root =
                if let Some(root_arg) = args.get(1).filter(|v| !v.is_undefined() && !v.is_null()) {
                    match receiver(root_arg, context)? {
                        Value::Host(h) if h == "document" => 0,
                        Value::Host(h) => state(context).borrow().host.node(&h).map_err(failure)?,
                        _ => return Err(failure("Root must be a DOM Node")),
                    }
                } else {
                    0
                };
            let found = state(context)
                .borrow()
                .host
                .document
                .query_selector(root, &selector)
                .map_err(failure)?;
            match found {
                Some(id) => wrap(&format!("node:{id}"), context),
                None => Ok(JsValue::null()),
            }
        }),
    )
    .name(js_string!("$"))
    .length(1)
    .build();
    define_temp("$", dollar.into(), context);

    // $$(selector, startNode?) -> Array<Element>
    let dollar_all = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|_, args, context| {
            modern::check_context(context)?;
            let selector = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
            let root =
                if let Some(root_arg) = args.get(1).filter(|v| !v.is_undefined() && !v.is_null()) {
                    match receiver(root_arg, context)? {
                        Value::Host(h) if h == "document" => 0,
                        Value::Host(h) => state(context).borrow().host.node(&h).map_err(failure)?,
                        _ => return Err(failure("Root must be a DOM Node")),
                    }
                } else {
                    0
                };
            let ids = state(context)
                .borrow()
                .host
                .document
                .query_selector_all(root, &selector)
                .map_err(failure)?;
            let mut elements = Vec::with_capacity(ids.len().min(256));
            for id in ids.into_iter().take(256) {
                elements.push(wrap(&format!("node:{id}"), context)?);
            }
            Ok(JsArray::from_iter(elements, context).into())
        }),
    )
    .name(js_string!("$$"))
    .length(1)
    .build();
    define_temp("$$", dollar_all.into(), context);

    // keys(obj) & values(obj)
    for (helper_name, is_values) in [("keys", false), ("values", true)] {
        let func = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure(move |_, args, context| {
                modern::check_context(context)?;
                let target = args
                    .first()
                    .and_then(JsValue::as_object)
                    .ok_or_else(|| failure("Argument must be an object"))?;
                if target.is::<modern::boa_engine::builtins::proxy::Proxy>()
                    && host_handle(&target, context).is_none()
                {
                    return Ok(JsArray::new(context)?.into());
                }
                let own_keys = target.own_property_keys(context)?;
                let mut result = Vec::new();
                for k in own_keys.into_iter().take(256) {
                    let (PropertyKey::String(_) | PropertyKey::Index(_)) = k else {
                        continue;
                    };
                    let Some(desc) = target
                        .try_borrow()
                        .ok()
                        .and_then(|b| b.properties().get(&k))
                    else {
                        continue;
                    };
                    if desc.enumerable() != Some(true) {
                        continue;
                    }
                    if is_values {
                        if let Some(v) = desc.value() {
                            result.push(v.clone());
                        }
                    } else {
                        result.push(JsString::from(property_key_name(&k)).into());
                    }
                }
                Ok(JsArray::from_iter(result, context).into())
            }),
        )
        .name(JsString::from(helper_name))
        .length(1)
        .build();
        define_temp(helper_name, func.into(), context);
    }

    // dir, dirxml, table, clear aliases
    for alias in ["dir", "dirxml", "table", "clear"] {
        let func = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |this, args, m, context| call(m, this, args, context),
                alias.to_owned(),
            ),
        )
        .name(JsString::from(alias))
        .length(usize::from(alias != "clear"))
        .build();
        define_temp(alias, func.into(), context);
    }

    // copy(value) -> copies string/JSON representation to host clipboard
    let copy_fn = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|_, args, context| {
            modern::check_context(context)?;
            let first = args.first().cloned().unwrap_or_default();
            let payload = if let Some(s) = first.as_string() {
                truncate_utf8(&s.to_std_string_lossy(), MAX_CONSOLE_TEXT_BYTES * 4)
            } else if first.is_object() {
                safe_json(&first, 0, context)
            } else {
                format_value_inline(&first, false, 0, context)
            };
            let shared = state(context);
            let mut data = shared.borrow_mut();
            data.console.push_entry(ConsoleWireEntry {
                level: ConsoleWireLevel::Copy(payload),
                text: String::new(),
                object_id: None,
                node_id: None,
                group_depth: 0,
            });
            Ok(JsValue::undefined())
        }),
    )
    .name(js_string!("copy"))
    .length(1)
    .build();
    define_temp("copy", copy_fn.into(), context);

    // inspect(node_or_obj) -> selects DOM node in Elements panel (or logs dir)
    let inspect_fn = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|this, args, context| {
            modern::check_context(context)?;
            let first = args.first().cloned().unwrap_or_default();
            if let Some(obj) = first.as_object()
                && let Some(handle) = host_handle(&obj, context)
                && let Some(id) = node_id_from_handle(&handle)
            {
                let shared = state(context);
                shared.borrow_mut().console.push_entry(ConsoleWireEntry {
                    level: ConsoleWireLevel::InspectNode(id),
                    text: String::new(),
                    object_id: None,
                    node_id: Some(id),
                    group_depth: 0,
                });
                return Ok(first);
            }
            call("dir", this, args, context)?;
            Ok(first)
        }),
    )
    .name(js_string!("inspect"))
    .length(1)
    .build();
    define_temp("inspect", inspect_fn.into(), context);

    installed
}

fn uninstall_command_line_api(realm: &mut BoaPageRealm, installed: Vec<(JsString, JsValue)>) {
    let context = realm.engine.context_mut();
    let global = context.global_object();
    for (key, expected) in installed {
        let unchanged = global
            .try_borrow()
            .ok()
            .and_then(|b| b.properties().get(&PropertyKey::from(key.clone())))
            .and_then(|d| d.value().cloned())
            .is_some_and(|current| JsValue::same_value(&current, &expected));
        if unchanged {
            let _ = global.delete_property_or_throw(key, context);
        }
    }
}

pub(super) fn run(
    realm: &mut BoaPageRealm,
    command: ConsoleCommand,
) -> (SessionReply, ConsoleResult) {
    let mut reply = BoaPageRealm::empty_reply();
    reply.revision = realm.revision;

    let preflight = (|| {
        command.validate()?;
        if realm.closed || realm.engine.is_fatal() {
            return Err("Page realm is no longer active".into());
        }
        if realm.lifecycle.phase != LifecyclePhase::LoadComplete
            || realm.state.borrow().observer.is_some()
        {
            return Err("Console requires a loaded ordinary page realm".into());
        }
        Ok::<(), String>(())
    })();
    if let Err(diagnostic) = preflight {
        reply.boa = Some(realm.engine.stats());
        reply.state = realm.realm_state();
        reply.lifecycle = Some(realm.lifecycle);
        if realm.closed || realm.engine.is_fatal() {
            realm.clear_work();
            reply.boa = Some(realm.engine.stats());
        }
        return (
            reply,
            ConsoleResult::Rejected {
                diagnostic: truncate_utf8(&diagnostic, 512),
            },
        );
    }

    realm.revision += 1;
    reply.revision = realm.revision;
    {
        let mut data = realm.state.borrow_mut();
        data.armed_wake = None;
        data.host.navigation = None;
    }

    let result = match command {
        ConsoleCommand::ClearObjects => {
            realm.state.borrow_mut().console.clear_roots();
            ConsoleResult::Cleared
        }
        ConsoleCommand::InspectObject { object_id } => {
            let found = realm.state.borrow().console.find_object(object_id);
            match found {
                Some(obj) => {
                    let entries = inspect_properties(&obj, realm.engine.context_mut());
                    ConsoleResult::Properties { object_id, entries }
                }
                None => ConsoleResult::Rejected {
                    diagnostic: "Object reference is no longer retained".into(),
                },
            }
        }
        ConsoleCommand::Evaluate {
            source,
            inspected_nodes,
        } => {
            realm.state.borrow_mut().error_exposure_allowed = false;
            if realm.purge_private_notices().is_err() {
                realm.rejection_failure(&mut reply.errors);
                realm.snapshot(&mut reply);
                return (
                    reply,
                    ConsoleResult::Rejected {
                        diagnostic: REJECTION_FAILURE.into(),
                    },
                );
            }

            // Chromium Console convenience: wrap `{...}` as `({...})` when valid as an expression.
            let trimmed = source.trim();
            let effective_source = if trimmed.starts_with('{') && trimmed.ends_with('}') {
                let candidate = format!("(\n{source}\n)");
                if Script::parse(
                    Source::from_bytes(candidate.as_bytes()),
                    None,
                    realm.engine.context_mut(),
                )
                .is_ok()
                {
                    candidate
                } else {
                    source
                }
            } else {
                source
            };

            let installed = install_command_line_api(realm, &inspected_nodes);
            let evaluated = {
                let _ingress = Ingress::enter(realm.state.clone());
                realm.engine.evaluate_raw(&effective_source)
            };
            uninstall_command_line_api(realm, installed);
            realm.checkpoint(&mut reply.errors);

            match evaluated {
                Ok(val) => {
                    realm.state.borrow_mut().console.last_result = Some(val.clone());
                    let (value_kind, preview, obj, node_id) =
                        classify_and_format(&val, true, 0, realm.engine.context_mut());
                    let object_id = obj.map(|o| realm.state.borrow_mut().console.retain_object(o));
                    ConsoleResult::Evaluated {
                        value_kind,
                        preview: truncate_utf8(&preview, MAX_CONSOLE_TEXT_BYTES),
                        object_id,
                        node_id,
                    }
                }
                Err(failure) => {
                    let fallback = truncate_utf8(&failure.diagnostic.to_string(), 512);
                    if let Some(exc) = failure.exception {
                        match exc.into_opaque(realm.engine.context_mut()) {
                            Ok(val) => {
                                let (_, preview, obj, _) =
                                    classify_and_format(&val, false, 0, realm.engine.context_mut());
                                let object_id =
                                    obj.map(|o| realm.state.borrow_mut().console.retain_object(o));
                                let text = if preview.starts_with("Uncaught") {
                                    preview
                                } else {
                                    format!("Uncaught {preview}")
                                };
                                ConsoleResult::Evaluated {
                                    value_kind: ConsoleValueKind::Exception,
                                    preview: truncate_utf8(&text, MAX_CONSOLE_TEXT_BYTES),
                                    object_id,
                                    node_id: None,
                                }
                            }
                            Err(_) => ConsoleResult::Evaluated {
                                value_kind: ConsoleValueKind::Exception,
                                preview: format!("Uncaught {fallback}"),
                                object_id: None,
                                node_id: None,
                            },
                        }
                    } else {
                        ConsoleResult::Evaluated {
                            value_kind: ConsoleValueKind::Exception,
                            preview: format!("Uncaught {fallback}"),
                            object_id: None,
                            node_id: None,
                        }
                    }
                }
            }
        }
    };

    realm.snapshot(&mut reply);
    let final_result = if reply.state != RealmState::Ready {
        ConsoleResult::Rejected {
            diagnostic: "Console realm terminated".into(),
        }
    } else {
        result
    };
    reply.errors.truncate(64);
    (reply, final_result)
}
