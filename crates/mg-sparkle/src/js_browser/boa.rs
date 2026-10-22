//! Boa page execution over the same bounded Rust DOM and typed activation API.
//!
//! The context owns all JS roots. Native bindings contain only private brands,
//! never an Rc back to their context. Node wrappers are weakly cached; active
//! listeners root their callbacks, and removing a listener releases that root.
//! Active listeners on detached nodes remain rooted until removal or realm
//! teardown, within the unchanged 32-registration/300-second session bounds.
//! This bounded retention policy is not full DOM ephemeron garbage collection.
use super::events::{Kind, Target};
use super::{BrowserHost, MAX_SOURCE, Request, bounded, inert, text_content};
use crate::{document, page_session::*};
use mg_butane::{
    modern::{self, Engine},
    runtime::{Host, Value},
};
use modern::boa_engine::{
    Context, JsData, JsError, JsNativeError, JsResult, JsString, JsValue, NativeFunction,
    js_string,
    object::{
        FunctionObjectBuilder, JsObject,
        builtins::{JsProxy, JsWeakMap},
    },
    property::{Attribute, PropertyDescriptor, PropertyKey},
};
use modern::boa_gc::{Finalize, Gc, GcRefCell, Trace};
use std::{
    cell::Cell,
    collections::HashMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT_REALM: AtomicU64 = AtomicU64::new(1);

#[derive(Trace, JsData)]
struct Brand {
    handle: String,
    realm: u64,
    #[unsafe_ignore_trace]
    finalized: Rc<Cell<usize>>,
}
impl Finalize for Brand {
    fn finalize(&self) {
        self.finalized.set(self.finalized.get() + 1);
    }
}

#[derive(Trace, Finalize)]
struct Listener {
    #[unsafe_ignore_trace]
    target: Target,
    #[unsafe_ignore_trace]
    kind: Kind,
    callback: Option<JsObject>,
    capture: bool,
    property: bool,
}

#[derive(Trace, Finalize)]
struct Timer {
    #[unsafe_ignore_trace]
    id: u32,
    #[unsafe_ignore_trace]
    handle: u64,
    #[unsafe_ignore_trace]
    deadline: Instant,
    #[unsafe_ignore_trace]
    nesting: u32,
    #[unsafe_ignore_trace]
    permit: modern::WorkPermit,
    callback: JsObject,
    arguments: Vec<JsValue>,
}

#[derive(Clone, Copy)]
struct ArmedWake {
    token: u64,
    handle: u64,
    deadline: Instant,
}

#[derive(Trace, Finalize)]
struct State {
    // BrowserHost contains only Rust DOM data and original value *descriptors*.
    // No original Runtime or Boa values live inside it.
    #[unsafe_ignore_trace]
    host: BrowserHost,
    realm: u64,
    listeners: Vec<Listener>,
    timers: Vec<Timer>,
    #[unsafe_ignore_trace]
    next_timer: u32,
    #[unsafe_ignore_trace]
    next_handle: u64,
    #[unsafe_ignore_trace]
    next_wake: u64,
    #[unsafe_ignore_trace]
    armed_wake: Option<ArmedWake>,
    #[unsafe_ignore_trace]
    timer_nesting: Option<u32>,
    #[unsafe_ignore_trace]
    js_depth: usize,
    #[unsafe_ignore_trace]
    checkpointing: bool,
    #[cfg(test)]
    #[unsafe_ignore_trace]
    test_now: Option<Instant>,
    wrappers: HashMap<String, JsObject>, // rooted WeakRef objects, not their targets
    methods: HashMap<String, JsObject>,
    brands: JsWeakMap,
    weak_constructor: JsObject,
    weak_deref: JsObject,
    reflect_get: JsObject,
    #[unsafe_ignore_trace]
    finalized: Rc<Cell<usize>>,
    wrappers_created: usize,
}
type Shared = Gc<GcRefCell<State>>;

impl State {
    fn now(&self) -> Instant {
        #[cfg(test)]
        {
            self.test_now.unwrap_or_else(Instant::now)
        }
        #[cfg(not(test))]
        {
            Instant::now()
        }
    }
    fn clear_timers(&mut self) {
        self.armed_wake = None;
        self.timers.clear();
    }
}

// A native conversion may re-enter JS. Only the outer driver, after that whole
// ingress returns, is allowed to run a checkpoint.
struct Ingress(Shared);
impl Ingress {
    fn enter(shared: Shared) -> Self {
        shared.borrow_mut().js_depth += 1;
        Self(shared)
    }
}
impl Drop for Ingress {
    fn drop(&mut self) {
        self.0.borrow_mut().js_depth -= 1;
    }
}

/// Owned only by a restricted worker (or a caller's owned test fixture).
pub struct BoaPageRealm {
    engine: Engine,
    state: Shared,
    revision: u64,
    closed: bool,
}

fn failure(message: impl Into<String>) -> JsError {
    JsNativeError::typ().with_message(message.into()).into()
}
fn state(context: &Context) -> Shared {
    context
        .get_data::<Shared>()
        .expect("page bindings initialized")
        .clone()
}
fn utf8(value: &JsValue, context: &mut Context) -> JsResult<String> {
    // This is callback-capable. Callers must not hold a State/DOM borrow.
    // The DOM arena stores UTF-8: preserve its existing replacement behavior
    // for lone surrogates, after genuine ECMAScript ToString (Symbols reject).
    Ok(value.to_string(context)?.to_std_string_lossy())
}
fn key(value: &JsValue, context: &mut Context) -> JsResult<PropertyKey> {
    value.to_property_key(context)
}
fn property_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::String(name) => Some(name.to_std_string_escaped()),
        PropertyKey::Index(index) => Some(index.get().to_string()),
        PropertyKey::Symbol(_) => None,
    }
}
fn brand(target: &JsValue, context: &Context) -> JsResult<String> {
    let object = target
        .as_object()
        .ok_or_else(|| failure("Invalid DOM target"))?;
    let data = object
        .downcast_ref::<Brand>()
        .ok_or_else(|| failure("Invalid DOM target"))?;
    if data.realm != state(context).borrow().realm {
        return Err(failure("DOM object belongs to another realm"));
    }
    Ok(data.handle.clone())
}
fn receiver(value: &JsValue, context: &mut Context) -> JsResult<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    if value.is_undefined() {
        return Ok(Value::Undefined);
    }
    let object = value
        .as_object()
        .ok_or_else(|| failure("DOM method requires its receiver"))?;
    if object == context.global_object() {
        return Ok(Value::Object(0));
    }
    let brands = state(context).borrow().brands.clone();
    let handle = brands
        .get(&object, context)?
        .as_string()
        .ok_or_else(|| failure("Invalid or foreign DOM receiver"))?;
    Ok(Value::Host(handle.to_std_string_escaped()))
}
fn event_target(value: &JsValue, context: &mut Context) -> JsResult<Target> {
    match receiver(value, context)? {
        Value::Null | Value::Undefined | Value::Object(0) => Ok(Target::Window),
        Value::Host(handle) => state(context)
            .borrow()
            .host
            .node(&handle)
            .map(Target::Node)
            .map_err(failure),
        _ => Err(failure("Unsupported EventTarget receiver")),
    }
}

fn wrap(handle: &str, context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let handle = if handle == "node:0" {
        "document"
    } else {
        handle
    };
    if handle == "window" {
        return Ok(context.global_object().into());
    }
    let shared = state(context);
    let (old, deref) = {
        let data = shared.borrow();
        (data.wrappers.get(handle).cloned(), data.weak_deref.clone())
    };
    if let Some(old) = old {
        // This is the captured native intrinsic, not a page-controlled lookup.
        let object = deref.call(&old.into(), &[], context)?;
        if object.is_object() {
            return Ok(object);
        }
    }
    let (realm, finalized, constructor, brands) = {
        let mut data = shared.borrow_mut();
        data.host.charge(256 + handle.len()).map_err(failure)?;
        data.wrappers_created += 1;
        (
            data.realm,
            data.finalized.clone(),
            data.weak_constructor.clone(),
            data.brands.clone(),
        )
    };
    let target = JsObject::from_proto_and_data(
        context.intrinsics().constructors().object().prototype(),
        Brand {
            handle: handle.into(),
            realm,
            finalized,
        },
    );
    let object: JsObject = JsProxy::builder(target)
        .get(get)
        .set(set)
        .has(has)
        .define_property(define_property)
        .delete_property(delete_property)
        .build(context)?
        .into();
    brands.set(&object, JsString::from(handle).into(), context)?;
    let weak = constructor.construct(&[object.clone().into()], None, context)?;
    shared.borrow_mut().wrappers.insert(handle.into(), weak);
    Ok(object.into())
}

fn from_host(value: Value, context: &mut Context) -> JsResult<JsValue> {
    match value {
        Value::Undefined => Ok(JsValue::undefined()),
        Value::Null => Ok(JsValue::null()),
        Value::Bool(value) => Ok(value.into()),
        Value::Number(value) => Ok(value.into()),
        Value::String(value) => Ok(JsString::from(value.as_slice()).into()),
        Value::Host(handle) => wrap(&handle, context),
        Value::Object(0) => Ok(context.global_object().into()),
        Value::Native(name) => method(&name, context).map(Into::into),
        _ => Err(failure("Unsupported host value descriptor")),
    }
}

fn method(name: &str, context: &mut Context) -> JsResult<JsObject> {
    let shared = state(context);
    if let Some(function) = shared.borrow().methods.get(name).cloned() {
        return Ok(function);
    }
    let function: JsObject = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure_with_captures(
            |this, args, name, context| call(name, this, args, context),
            name.to_string(),
        ),
    )
    .name(JsString::from(name))
    .length(1)
    .build()
    .into();
    shared
        .borrow_mut()
        .methods
        .insert(name.into(), function.clone());
    Ok(function)
}

fn property_callback(target: Target, property: &str, context: &Context) -> Option<JsObject> {
    let kind = Kind::modern_property(property)?;
    state(context)
        .borrow()
        .listeners
        .iter()
        .find(|l| l.property && l.target == target && l.kind == kind && l.callback.is_some())
        .and_then(|l| l.callback.clone())
}
fn property_set(
    target: Target,
    property: &str,
    value: &JsValue,
    context: &mut Context,
) -> JsResult<()> {
    modern::check_context(context)?;
    let kind =
        Kind::modern_property(property).ok_or_else(|| failure("Unsupported event property"))?;
    let callback = value.as_callable();
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if let Some(listener) = data
        .listeners
        .iter_mut()
        .find(|l| l.property && l.target == target && l.kind == kind && l.callback.is_some())
    {
        listener.callback = callback;
    } else if callback.is_some() {
        if data.listeners.len() >= 32 {
            return Err(failure("Listener limit exceeded"));
        }
        data.host.charge(128).map_err(failure)?;
        data.listeners.push(Listener {
            target,
            kind,
            callback,
            capture: false,
            property: true,
        });
    }
    Ok(())
}
fn property_target(handle: &str, context: &Context) -> JsResult<Target> {
    if handle == "window" {
        return Ok(Target::Window);
    }
    state(context)
        .borrow()
        .host
        .node(handle)
        .map(Target::Node)
        .map_err(failure)
}
fn fallback_get(args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    // Preserve the Proxy receiver for inherited accessors. JsObject::get uses
    // the raw native target as `this`, exposing that private object to JS.
    // Capture the intrinsic before page execution, and drop the State borrow
    // before the getter can re-enter DOM bindings or trigger collection.
    let reflect_get = state(context).borrow().reflect_get.clone();
    reflect_get.call(&JsValue::undefined(), args, context)
}

fn get(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let handle = brand(&args[0], context)?;
    let property = key(&args[1], context)?;
    let Some(name) = property_name(&property) else {
        if (handle.starts_with("collection:") || handle.starts_with("classlist:"))
            && property == PropertyKey::Symbol(modern::boa_engine::JsSymbol::iterator())
        {
            return context
                .intrinsics()
                .constructors()
                .array()
                .prototype()
                .get(modern::boa_engine::JsSymbol::iterator(), context);
        }
        return fallback_get(args, context);
    };
    if (handle.starts_with("collection:") || handle.starts_with("classlist:"))
        && matches!(name.as_str(), "forEach" | "values" | "entries" | "keys")
    {
        return context
            .intrinsics()
            .constructors()
            .array()
            .prototype()
            .get(JsString::from(name.as_str()), context);
    }
    if Kind::modern_property(&name).is_some() {
        let target = property_target(&handle, context)?;
        return Ok(property_callback(target, &name, context)
            .map(Into::into)
            .unwrap_or_else(JsValue::null));
    }
    if let Some(id) = handle
        .strip_prefix("event:")
        .and_then(|id| id.parse::<usize>().ok())
    {
        let shared = state(context);
        let data = shared.borrow();
        let event = data
            .host
            .events
            .get(id)
            .ok_or_else(|| failure("Stale event handle"))?;
        match name.as_str() {
            "bubbles" => {
                return Ok(
                    matches!(event.kind, Kind::DomReady | Kind::Click | Kind::Submit).into(),
                );
            }
            "cancelable" => return Ok(matches!(event.kind, Kind::Click | Kind::Submit).into()),
            _ => {}
        }
    }
    let value = state(context)
        .borrow_mut()
        .host
        .get(&handle, &name)
        .map_err(failure)?;
    if matches!(value, Value::Undefined) {
        return fallback_get(args, context);
    }
    from_host(value, context)
}
fn set(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let handle = brand(&args[0], context)?;
    let property = key(&args[1], context)?;
    let Some(name) = property_name(&property) else {
        return Err(failure("Symbol DOM properties are not implemented"));
    };
    if Kind::modern_property(&name).is_some() {
        property_set(property_target(&handle, context)?, &name, &args[2], context)?;
    } else if handle.starts_with("node:") && super::bool_attr_name(&name).is_some() {
        let flag = args[2].to_boolean();
        state(context)
            .borrow_mut()
            .host
            .set(&handle, &name, Value::Bool(flag))
            .map_err(failure)?;
    } else {
        let is_string = state(context)
            .borrow()
            .host
            .string_assignment(&handle, &name);
        if !is_string {
            return Err(failure(format!(
                "Setting DOM property {name} is not implemented"
            )));
        }
        let text = utf8(&args[2], context)?;
        modern::check_context(context)?;
        state(context)
            .borrow_mut()
            .host
            .set(&handle, &name, Value::text(&text))
            .map_err(failure)?;
    }
    Ok(true.into())
}
fn has(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let handle = brand(&args[0], context)?;
    let property = key(&args[1], context)?;
    if let Some(name) = property_name(&property) {
        if handle.starts_with("collection:") {
            if let Ok(index) = name.parse::<usize>() {
                return state(context)
                    .borrow_mut()
                    .host
                    .has_indexed_property(&handle, index)
                    .map(JsValue::from)
                    .map_err(failure);
            }
        }
        if Kind::modern_property(&name).is_some() {
            return Ok(true.into());
        }
        let value = state(context)
            .borrow_mut()
            .host
            .get(&handle, &name)
            .map_err(failure)?;
        if !matches!(value, Value::Undefined) {
            return Ok(true.into());
        }
    }
    args[0]
        .as_object()
        .unwrap()
        .has_property(property, context)
        .map(Into::into)
}
fn define_property(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    Err(failure("Defining DOM properties is not implemented"))
}
fn delete_property(_: &JsValue, _: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    Err(failure("Deleting DOM properties is not implemented"))
}

fn listener_call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let target = event_target(this, context)?;
    let kind = Kind::modern_parse(&utf8(
        args.first().unwrap_or(&JsValue::undefined()),
        context,
    )?)
    .map_err(failure)?;
    let value = args.get(1).cloned().unwrap_or_default();
    let capture = match args.get(2) {
        None => false,
        Some(value) if value.is_undefined() => false,
        Some(value) => value
            .as_boolean()
            .ok_or_else(|| failure("Only boolean listener capture is implemented"))?,
    };
    if value.is_null() || value.is_undefined() {
        return Ok(JsValue::undefined());
    }
    let callback = value
        .as_callable()
        .ok_or_else(|| failure("Listener must be a function"))?;
    modern::check_context(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let index = data.listeners.iter().position(|l| {
        !l.property
            && l.target == target
            && l.kind == kind
            && l.capture == capture
            && l.callback.as_ref() == Some(&callback)
    });
    if name.ends_with("removeEventListener") {
        if let Some(index) = index {
            data.listeners[index].callback = None;
        }
    } else if index.is_none() {
        if data.listeners.len() >= 32 {
            return Err(failure("Listener limit exceeded"));
        }
        data.host.charge(128).map_err(failure)?;
        data.listeners.push(Listener {
            target,
            kind,
            callback: Some(callback),
            capture,
            property: false,
        });
    }
    Ok(JsValue::undefined())
}

fn timer_call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    if event_target(this, context)? != Target::Window {
        return Err(failure("Timer requires its Window receiver"));
    }
    if name.ends_with("clearTimeout") {
        let id = args
            .first()
            .unwrap_or(&JsValue::undefined())
            .to_i32(context)?;
        modern::check_context(context)?;
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if let Some(index) = data.timers.iter().position(|timer| timer.id as i32 == id) {
            data.timers.swap_remove(index);
        }
        return Ok(JsValue::undefined());
    }
    // String compilation is explicitly unsupported, not a hidden eval path.
    let callback = args
        .first()
        .and_then(JsValue::as_callable)
        .ok_or_else(|| failure("Only function-valued setTimeout is implemented"))?;
    let delay = args
        .get(1)
        .unwrap_or(&JsValue::undefined())
        .to_i32(context)?
        .max(0) as u64;
    modern::check_context(context)?;
    // One common pending/cumulative ledger with Promise jobs. Admit before
    // retaining callback/arguments or allocating their native record.
    let permit = modern::admit_host_task(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    let nesting = data.timer_nesting.unwrap_or(0);
    let delay = if nesting > 5 { delay.max(4) } else { delay };
    let bytes = std::mem::size_of::<Timer>()
        .checked_add(
            args.len()
                .saturating_sub(2)
                .checked_mul(std::mem::size_of::<JsValue>())
                .ok_or_else(|| failure("Timer argument storage overflow"))?,
        )
        .ok_or_else(|| failure("Timer argument storage overflow"))?;
    data.host.charge(bytes).map_err(failure)?;
    let id = data
        .next_timer
        .checked_add(1)
        .filter(|id| *id <= i32::MAX as u32)
        .ok_or_else(|| failure("Timer ID exhausted"))?;
    let handle = data
        .next_handle
        .checked_add(1)
        .ok_or_else(|| failure("Timer handle exhausted"))?;
    let deadline = data
        .now()
        .checked_add(Duration::from_millis(delay))
        .ok_or_else(|| failure("Timer deadline overflow"))?;
    data.next_timer = id;
    data.next_handle = handle;
    data.timers.push(Timer {
        id,
        handle,
        deadline,
        nesting: nesting + 1,
        permit,
        callback,
        arguments: args.get(2..).unwrap_or_default().to_vec(),
    });
    Ok((id as f64).into())
}

fn call(name: &str, this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    if matches!(name, "host.window.setTimeout" | "host.window.clearTimeout") {
        return timer_call(name, this, args, context);
    }
    if name == "host.event.preventDefault" {
        let Value::Host(handle) = receiver(this, context)? else {
            return Err(failure("Invalid Event receiver"));
        };
        let id = handle
            .strip_prefix("event:")
            .and_then(|id| id.parse::<usize>().ok())
            .ok_or_else(|| failure("Invalid Event receiver"))?;
        let shared = state(context);
        let data = shared.borrow();
        let event = data
            .host
            .events
            .get(id)
            .ok_or_else(|| failure("Stale event handle"))?;
        if !matches!(event.kind, Kind::Click | Kind::Submit) {
            return Ok(JsValue::undefined());
        }
    }
    if name.ends_with("addEventListener") || name.ends_with("removeEventListener") {
        return listener_call(name, this, args, context);
    }
    if name == "host.console.log" {
        return Ok(JsValue::undefined());
    }
    let strings = state(context).borrow().host.string_arguments(name);
    let mut converted = Vec::new();
    // Do not convert unused arguments: an ignored arbitrary object is not a node
    // and must not gain observable conversion or receiver-validation effects.
    let count = match name {
        "host.classlist.add" | "host.classlist.remove" => args.len().min(8),
        "host.dom.setAttribute" | "host.classlist.replace" | "host.classlist.toggle" => 2,
        "host.dom.appendChild" | "host.dom.removeChild" => 1,
        _ if strings.contains(&0) => 1,
        _ => 0,
    };
    for (index, value) in args.iter().take(count).enumerate() {
        let value = if strings.contains(&index) {
            Value::text(&utf8(value, context)?)
        } else if value.is_undefined() {
            Value::Undefined
        } else if value.is_null() {
            Value::Null
        } else if let Some(value) = value.as_boolean() {
            Value::Bool(value)
        } else if let Some(value) = value.as_number() {
            Value::Number(value)
        } else if let Some(value) = value.as_string() {
            Value::text(&value.to_std_string_lossy())
        } else {
            receiver(value, context)?
        };
        converted.push(value);
    }
    let this = if name.starts_with("host.location.") {
        Value::Host("location".into())
    } else {
        receiver(this, context)?
    };
    modern::check_context(context)?;
    let value = state(context)
        .borrow_mut()
        .host
        .call(name, this, converted)
        .map_err(failure)?;
    from_host(value, context)
}

impl BoaPageRealm {
    pub fn start(request: Request) -> (Option<Self>, SessionReply) {
        let mut reply = Self::empty_reply();
        if request.html.len() > MAX_SOURCE || request.url.len() > 16_384 {
            reply
                .errors
                .push("Script document exceeds the source/URL limit".into());
            return (None, reply);
        }
        let url = match url::Url::parse(&request.url) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => url,
            _ => {
                reply
                    .errors
                    .push("Script document requires an HTTP(S) origin".into());
                return (None, reply);
            }
        };
        let document = document::parse_with_scripting(&request.html, url.as_str(), true);
        let allocated = match request.validate_external_sources(&document) {
            Ok(bytes) => bytes,
            Err(error) => {
                reply.errors.push(error);
                return (None, reply);
            }
        };
        let mut sources = Vec::new();
        let mut deferred = Vec::new();
        let mut count = 0;
        for (index, node) in document.nodes.iter().enumerate() {
            if node.tag != "script" || inert(&document.nodes, index) {
                continue;
            }
            let kind = node.attr("type").unwrap_or("").trim().to_ascii_lowercase();
            if !matches!(
                kind.as_str(),
                "" | "module"
                    | "text/javascript"
                    | "application/javascript"
                    | "text/ecmascript"
                    | "application/ecmascript"
                    | "application/x-javascript"
            ) {
                continue;
            }
            count += 1;
            if count > 32 {
                reply.errors.push("Script count limit reached".into());
                break;
            }
            if kind == "module" {
                reply
                    .errors
                    .push("Module scripts are not implemented".into());
                continue;
            }
            if node.attr("src").is_some() {
                if let Some(source) = request.external_source(index) {
                    if node.attr("defer").is_some() {
                        deferred.push((true, source.to_owned()));
                    } else {
                        sources.push((true, source.to_owned()));
                    }
                } else if request.external_scripts.is_none() {
                    reply
                        .errors
                        .push("External script loading is not implemented".into());
                }
                continue;
            }
            sources.push((false, text_content(&document.nodes, index)));
        }
        let ordinary_sources = sources.len();
        sources.extend(deferred);
        let host = BrowserHost {
            document,
            url,
            navigation: None,
            allocated,
            collections: Vec::new(),
            listeners: Vec::new(),
            events: Vec::new(),
            ready: "loading",
        };
        let mut realm = match Self::new(host) {
            Ok(realm) => realm,
            Err(error) => {
                reply.errors.push(error);
                return (None, reply);
            }
        };
        for (index, (is_external, source)) in sources.iter().enumerate() {
            if index == ordinary_sources {
                realm.set_ready("interactive", &mut reply.errors);
                if realm.closed
                    || realm.engine.is_fatal()
                    || realm.state.borrow().host.navigation.is_some()
                {
                    break;
                }
            }
            let label = if *is_external { "External" } else { "Inline" };
            let result = {
                let _ingress = Ingress::enter(realm.state.clone());
                realm.engine.evaluate(source)
            };
            match result {
                Ok(_) => reply.scripts_executed += 1,
                Err(error) => reply.errors.push(format!(
                    "{label} script {}: {}",
                    index + 1,
                    bounded(&error.to_string(), 512)
                )),
            }
            realm.checkpoint(&mut reply.errors);
            if realm.closed
                || realm.engine.is_fatal()
                || realm.state.borrow().host.navigation.is_some()
            {
                break;
            }
        }
        if !realm.closed
            && !realm.engine.is_fatal()
            && realm.state.borrow().host.navigation.is_none()
        {
            if realm.state.borrow().host.ready == "loading" {
                realm.set_ready("interactive", &mut reply.errors);
            }
            realm.startup(&mut reply.errors);
        }
        realm.snapshot(&mut reply);
        reply.errors.truncate(64);
        (reply.snapshot.is_some().then_some(realm), reply)
    }
    fn new(host: BrowserHost) -> Result<Self, String> {
        let mut engine = Engine::new().map_err(|e| e.to_string())?;
        let context = engine.context_mut();
        let constructor = context.intrinsics().constructors().weak_ref().constructor();
        let deref = context
            .intrinsics()
            .constructors()
            .weak_ref()
            .prototype()
            .get(js_string!("deref"), context)
            .map_err(|_| "WeakRef intrinsic unavailable")?
            .as_callable()
            .ok_or("WeakRef intrinsic is not callable")?;
        let reflect_get = context
            .intrinsics()
            .objects()
            .reflect()
            .get(js_string!("get"), context)
            .map_err(|_| "Reflect.get intrinsic unavailable")?
            .as_callable()
            .ok_or("Reflect.get intrinsic is not callable")?;
        let shared = Gc::new(GcRefCell::new(State {
            host,
            realm: NEXT_REALM.fetch_add(1, Ordering::Relaxed),
            listeners: Vec::new(),
            timers: Vec::new(),
            next_timer: 0,
            next_handle: 0,
            next_wake: 0,
            armed_wake: None,
            timer_nesting: None,
            js_depth: 0,
            checkpointing: false,
            #[cfg(test)]
            test_now: None,
            wrappers: HashMap::new(),
            methods: HashMap::new(),
            brands: JsWeakMap::new(context),
            weak_constructor: constructor,
            weak_deref: deref,
            reflect_get,
            finalized: Rc::new(Cell::new(0)),
            wrappers_created: 0,
        }));
        context.insert_data(shared.clone());
        let global = context.global_object();
        for name in ["window", "self", "globalThis", "parent"] {
            context
                .register_global_property(JsString::from(name), global.clone(), Attribute::all())
                .map_err(|_| "Global binding failed")?;
        }
        let top = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_fn_ptr(|_, _, context| Ok(context.global_object().into())),
        )
        .build();
        global
            .define_property_or_throw(
                js_string!("top"),
                PropertyDescriptor::builder()
                    .get(top)
                    .enumerable(true)
                    .configurable(false),
                context,
            )
            .map_err(|_| "Window top binding failed")?;
        context
            .register_global_property(js_string!("opener"), JsValue::null(), Attribute::all())
            .map_err(|_| "Window opener binding failed")?;
        for name in ["document", "navigator", "console"] {
            let object = wrap(name, context).map_err(|_| "DOM binding failed")?;
            context
                .register_global_property(JsString::from(name), object, Attribute::all())
                .map_err(|_| "DOM global failed")?;
        }
        for name in [
            "addEventListener",
            "removeEventListener",
            "setTimeout",
            "clearTimeout",
        ] {
            let function = method(&format!("host.window.{name}"), context)
                .map_err(|_| "Event binding failed")?;
            context
                .register_global_property(JsString::from(name), function, Attribute::all())
                .map_err(|_| "Event global failed")?;
        }
        for name in [
            "location",
            "onclick",
            "onsubmit",
            "onload",
            "onreadystatechange",
        ] {
            let getter = FunctionObjectBuilder::new(
                context.realm(),
                NativeFunction::from_copy_closure_with_captures(
                    |_, _, name, context| {
                        modern::check_context(context)?;
                        if name == "location" {
                            wrap("location", context)
                        } else {
                            Ok(property_callback(Target::Window, name, context)
                                .map(Into::into)
                                .unwrap_or_else(JsValue::null))
                        }
                    },
                    name.to_string(),
                ),
            )
            .build();
            let setter = FunctionObjectBuilder::new(
                context.realm(),
                NativeFunction::from_copy_closure_with_captures(
                    |_, args, name, context| {
                        modern::check_context(context)?;
                        let value = args.first().cloned().unwrap_or_default();
                        if name == "location" {
                            let text = utf8(&value, context)?;
                            modern::check_context(context)?;
                            state(context)
                                .borrow_mut()
                                .host
                                .navigate(&text)
                                .map_err(failure)?;
                        } else {
                            property_set(Target::Window, name, &value, context)?;
                        }
                        Ok(JsValue::undefined())
                    },
                    name.to_string(),
                ),
            )
            .build();
            global
                .define_property_or_throw(
                    JsString::from(name),
                    PropertyDescriptor::builder()
                        .get(getter)
                        .set(setter)
                        .enumerable(true)
                        .configurable(name != "location"),
                    context,
                )
                .map_err(|_| "Global accessor failed")?;
        }
        engine.check().map_err(|e| e.to_string())?;
        Ok(Self {
            engine,
            state: shared,
            revision: 0,
            closed: false,
        })
    }
    fn empty_reply() -> SessionReply {
        SessionReply {
            revision: 0,
            snapshot: None,
            outcome: EventOutcome::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: Vec::new(),
            scripts_executed: 0,
            allocations: None,
            boa: None,
            state: RealmState::Closed,
            acknowledgements: Vec::new(),
            task_wake: None,
        }
    }
    fn checkpoint(&mut self, errors: &mut Vec<String>) {
        let old_nesting = {
            let mut data = self.state.borrow_mut();
            if data.js_depth != 0 || data.checkpointing {
                return;
            }
            data.checkpointing = true;
            // A timer created by a Promise job is not nested in a timer task.
            data.timer_nesting.take()
        };
        if let Err(error) = self.engine.checkpoint() {
            if errors.len() < 64 {
                errors.push(format!(
                    "Promise checkpoint: {}",
                    bounded(&error.to_string(), 512)
                ));
            }
        }
        let mut data = self.state.borrow_mut();
        data.timer_nesting = old_nesting;
        data.checkpointing = false;
    }
    fn call_function(
        &mut self,
        callback: &JsObject,
        this: &JsValue,
        arguments: &[JsValue],
    ) -> Result<JsValue, modern::Error> {
        let _ingress = Ingress::enter(self.state.clone());
        self.engine.call(callback, this, arguments)
    }
    fn invoke(
        &mut self,
        callback: JsObject,
        target: Target,
        event: &str,
        property: bool,
        errors: &mut Vec<String>,
    ) -> bool {
        let result = (|| {
            let this = from_host(target.value(), self.engine.context_mut())
                .map_err(|_| "Invalid callback target".to_string())?;
            let event = wrap(event, self.engine.context_mut())
                .map_err(|_| "Invalid event wrapper".to_string())?;
            self.call_function(&callback, &this, &[event])
                .map_err(|e| e.to_string())
        })();
        let canceled = match result {
            Ok(value) => property && value.as_boolean() == Some(false),
            Err(error) => {
                if errors.len() < 64 {
                    errors.push(format!("Event handler: {}", bounded(&error, 512)));
                }
                false
            }
        };
        self.checkpoint(errors);
        canceled
    }
    fn startup(&mut self, errors: &mut Vec<String>) {
        // Deliberately bundled snapshot startup, not a streaming parser or
        // fully task-separated HTML lifecycle. Timers never run in this phase.
        if self.closed || self.engine.is_fatal() || self.state.borrow().host.navigation.is_some() {
            return;
        }
        self.lifecycle(Kind::DomReady, errors);
        if self.closed || self.engine.is_fatal() || self.state.borrow().host.navigation.is_some() {
            return;
        }
        self.set_ready("complete", errors);
        if self.closed || self.engine.is_fatal() || self.state.borrow().host.navigation.is_some() {
            return;
        }
        self.lifecycle(Kind::Load, errors);
        self.checkpoint(errors);
    }
    fn set_ready(&mut self, ready: &'static str, errors: &mut Vec<String>) {
        self.state.borrow_mut().host.ready = ready;
        self.lifecycle(Kind::ReadyState, errors);
    }
    fn lifecycle(&mut self, kind: Kind, errors: &mut Vec<String>) {
        let path: &[Target] = if kind == Kind::Load {
            &[Target::Window]
        } else {
            &[Target::Node(0), Target::Window]
        };
        // Unobserved startup events need no retained record. Preserve the old
        // cumulative 256-record allowance without allocating invisible events.
        if !self.state.borrow().listeners.iter().any(|listener| {
            listener.kind == kind && listener.callback.is_some() && path.contains(&listener.target)
        }) {
            return;
        }
        let event = match self
            .state
            .borrow_mut()
            .host
            .new_event(kind, Target::Node(0), None)
        {
            Ok(event) => event,
            Err(error) => {
                errors.push(error);
                self.closed = true;
                return;
            }
        };
        for target in path[1..].iter().rev() {
            if self.engine.is_fatal() || self.state.borrow().host.events[event].stopped {
                break;
            }
            self.listeners_at(event, *target, true, 1, errors);
        }
        if !self.engine.is_fatal() && !self.state.borrow().host.events[event].stopped {
            self.listeners_at(event, path[0], true, 2, errors);
            if !self.engine.is_fatal() && !self.state.borrow().host.events[event].immediate {
                self.listeners_at(event, path[0], false, 2, errors);
            }
        }
        if kind == Kind::DomReady {
            for target in &path[1..] {
                if self.engine.is_fatal() || self.state.borrow().host.events[event].stopped {
                    break;
                }
                self.listeners_at(event, *target, false, 3, errors);
            }
        }
        let mut data = self.state.borrow_mut();
        data.host.events[event].current = None;
        data.host.events[event].phase = 0;
    }
    fn snapshot(&mut self, reply: &mut SessionReply) {
        if let Err(error) = self.engine.check() {
            if reply.errors.len() < 64 {
                reply.errors.push(bounded(&error.to_string(), 512));
            }
        }
        reply.state = if self.engine.is_fatal() {
            RealmState::Fatal
        } else if self.closed {
            RealmState::Closed
        } else {
            RealmState::Ready
        };
        if reply.state != RealmState::Ready {
            reply.default_action = DefaultAction::None;
            self.closed = true;
            self.state.borrow_mut().clear_timers();
        } else if let Err(error) = self.advertise_wake(reply) {
            reply.errors.push(error);
            reply.state = RealmState::Closed;
            reply.default_action = DefaultAction::None;
            self.closed = true;
            self.state.borrow_mut().clear_timers();
        }
        reply.boa = Some(self.engine.stats());
        let mut data = self.state.borrow_mut();
        match data.host.serialize(0, true) {
            Ok(_) => {
                reply.snapshot = Some(ArenaSnapshot {
                    nodes: data.host.document.nodes.clone(),
                });
                if reply.state == RealmState::Ready {
                    reply.navigation = data.host.navigation.take();
                }
            }
            Err(error) => {
                self.closed = true;
                reply.state = RealmState::Closed;
                reply.default_action = DefaultAction::None;
                reply.errors.push(error);
            }
        }
        if reply.state != RealmState::Ready {
            reply.task_wake = None;
            data.clear_timers();
        }
        drop(data);
        reply.boa = Some(self.engine.stats());
    }

    fn advertise_wake(&mut self, reply: &mut SessionReply) -> Result<(), String> {
        let mut data = self.state.borrow_mut();
        data.armed_wake = None;
        reply.task_wake = None;
        let Some((handle, deadline)) = data
            .timers
            .iter()
            .min_by_key(|timer| (timer.deadline, timer.handle))
            .map(|timer| (timer.handle, timer.deadline))
        else {
            return Ok(());
        };
        let token = data
            .next_wake
            .checked_add(1)
            .ok_or("Task wake token exhausted")?;
        let remaining = deadline.saturating_duration_since(data.now());
        let delay_ms = u32::try_from(remaining.as_nanos().div_ceil(1_000_000))
            .map_err(|_| "Task wake delay overflow")?;
        data.next_wake = token;
        data.armed_wake = Some(ArmedWake {
            token,
            handle,
            deadline,
        });
        reply.task_wake = Some(TaskWake { token, delay_ms });
        Ok(())
    }

    /// Run one advertised eligible timer, only after a parent-issued transaction.
    /// No native edits/defaults are implied by a task; later handlers share its realm.
    pub fn pump_tasks(&mut self, pump: TaskPump) -> SessionReply {
        let mut reply = Self::empty_reply();
        reply.revision = self.revision;
        let eligible = {
            let data = self.state.borrow();
            data.armed_wake
                .filter(|wake| {
                    pump.wake_token != 0
                        && wake.token == pump.wake_token
                        && data.now() >= wake.deadline
                })
                .and_then(|wake| {
                    data.timers
                        .iter()
                        .position(|timer| timer.handle == wake.handle)
                })
        };
        if self.closed || self.engine.is_fatal() || eligible.is_none() {
            self.closed = true;
            self.state.borrow_mut().clear_timers();
            reply
                .errors
                .push("Inactive realm or early, stale or replayed task wake".into());
            reply.state = if self.engine.is_fatal() {
                RealmState::Fatal
            } else {
                RealmState::Closed
            };
            reply.boa = Some(self.engine.stats());
            return reply;
        }
        self.revision += 1;
        reply.revision = self.revision;
        let mut timer = {
            let mut data = self.state.borrow_mut();
            data.armed_wake = None;
            data.host.navigation = None;
            data.timers.swap_remove(eligible.unwrap())
        };
        match modern::begin_host_task(self.engine.context_mut(), &mut timer.permit) {
            Ok(()) => {
                self.state.borrow_mut().timer_nesting = Some(timer.nesting);
                let this = self.engine.context().global_object().into();
                if let Err(error) = self.call_function(&timer.callback, &this, &timer.arguments) {
                    reply.errors.push(format!(
                        "Timer callback: {}",
                        bounded(&error.to_string(), 512)
                    ));
                }
                self.checkpoint(&mut reply.errors);
                self.state.borrow_mut().timer_nesting = None;
            }
            Err(error) => {
                self.closed = true;
                reply
                    .errors
                    .push(bounded(&modern::safe_error(&error).to_string(), 512));
            }
        }
        // Callback/argument roots and the consumed permit need not survive reply
        // allocation. Unexecuted timers remain rooted in State.
        drop(timer);
        self.snapshot(&mut reply);
        reply.errors.truncate(64);
        reply
    }

    fn listeners_at(
        &mut self,
        event: usize,
        target: Target,
        capture: bool,
        phase: u8,
        errors: &mut Vec<String>,
    ) {
        let ids: Vec<_> = {
            let mut data = self.state.borrow_mut();
            data.host.events[event].current = Some(target);
            data.host.events[event].phase = phase;
            let kind = data.host.events[event].kind;
            data.listeners
                .iter()
                .enumerate()
                .filter(|(_, l)| {
                    l.callback.is_some()
                        && l.kind == kind
                        && l.target == target
                        && l.capture == capture
                })
                .map(|(id, _)| id)
                .collect()
        };
        for id in ids {
            if self.engine.is_fatal() || self.state.borrow().host.events[event].immediate {
                break;
            }
            let listener = {
                let data = self.state.borrow();
                let listener = &data.listeners[id];
                listener
                    .callback
                    .clone()
                    .map(|callback| (callback, listener.property))
            };
            if let Some((callback, property)) = listener {
                if self.invoke(
                    callback,
                    target,
                    &format!("event:{event}"),
                    property,
                    errors,
                ) {
                    let mut data = self.state.borrow_mut();
                    if matches!(data.host.events[event].kind, Kind::Click | Kind::Submit) {
                        data.host.events[event].canceled = true;
                    }
                }
            }
        }
    }
    fn emit(
        &mut self,
        kind: Kind,
        target: usize,
        submitter: Option<usize>,
        errors: &mut Vec<String>,
    ) -> Result<bool, String> {
        let (path, length) = self.state.borrow().host.connected_path(target)?;
        let id = self
            .state
            .borrow_mut()
            .host
            .new_event(kind, Target::Node(target), submitter)?;
        for owner in path[1..length].iter().rev() {
            if self.engine.is_fatal() || self.state.borrow().host.events[id].stopped {
                break;
            }
            self.listeners_at(id, *owner, true, 1, errors);
        }
        if !self.engine.is_fatal() && !self.state.borrow().host.events[id].stopped {
            self.listeners_at(id, path[0], true, 2, errors);
            if !self.engine.is_fatal() && !self.state.borrow().host.events[id].immediate {
                self.listeners_at(id, path[0], false, 2, errors);
            }
        }
        for owner in &path[1..length] {
            if self.engine.is_fatal() || self.state.borrow().host.events[id].stopped {
                break;
            }
            self.listeners_at(id, *owner, false, 3, errors);
        }
        let mut data = self.state.borrow_mut();
        data.host.events[id].current = None;
        data.host.events[id].phase = 0;
        Ok(data.host.events[id].canceled)
    }
    fn submit_activation(
        &mut self,
        form: usize,
        submitter: Option<usize>,
        outcome: &mut EventOutcome,
        errors: &mut Vec<String>,
    ) -> Result<DefaultAction, String> {
        if self.engine.is_fatal()
            || self.state.borrow().host.navigation.is_some()
            || self.state.borrow().host.connected_path(form).is_err()
        {
            return Ok(DefaultAction::None);
        }
        let canceled = self.emit(Kind::Submit, form, submitter, errors)?;
        outcome.submit_canceled = Some(canceled);
        let data = self.state.borrow();
        let stale_submitter = submitter.is_some_and(|node| {
            data.host.connected_path(node).is_err()
                || !data.host.is_submitter(node)
                || data.host.form_owner(node) != Some(form)
        });
        if canceled
            || self.engine.is_fatal()
            || data.host.navigation.is_some()
            || data.host.connected_path(form).is_err()
            || stale_submitter
        {
            Ok(DefaultAction::None)
        } else {
            Ok(DefaultAction::SubmitForm { form, submitter })
        }
    }
    fn click_activation(
        &mut self,
        target: usize,
        outcome: &mut EventOutcome,
        errors: &mut Vec<String>,
    ) -> Result<DefaultAction, String> {
        let activation = {
            let data = self.state.borrow();
            let (path, length) = data.host.connected_path(target)?;
            path[..length].iter().find_map(|target| match target {
                Target::Node(id)
                    if data.host.is_submitter(*id)
                        || data.host.document.nodes[*id].tag == "a"
                            && data.host.document.nodes[*id].has("href") =>
                {
                    Some(*id)
                }
                _ => None,
            })
        };
        let canceled = self.emit(Kind::Click, target, None, errors)?;
        outcome.click_canceled = Some(canceled);
        if canceled || self.engine.is_fatal() || self.state.borrow().host.navigation.is_some() {
            return Ok(DefaultAction::None);
        }
        let Some(node) = activation else {
            return Ok(DefaultAction::None);
        };
        let submit = {
            let data = self.state.borrow();
            if data.host.connected_path(node).is_err() {
                return Ok(DefaultAction::None);
            }
            if data.host.is_submitter(node) {
                data.host.form_owner(node)
            } else if data.host.document.nodes[node].tag == "a"
                && data.host.document.nodes[node].has("href")
            {
                return Ok(DefaultAction::FollowLink { node });
            } else {
                None
            }
        };
        if let Some(form) = submit {
            self.submit_activation(form, Some(node), outcome, errors)
        } else {
            Ok(DefaultAction::None)
        }
    }
    pub fn dispatch(&mut self, input: SessionInput) -> SessionReply {
        let mut reply = Self::empty_reply();
        reply.revision = self.revision;
        reply.boa = Some(self.engine.stats());
        if self.closed || self.engine.is_fatal() {
            self.closed = true;
            self.state.borrow_mut().clear_timers();
            reply.boa = Some(self.engine.stats());
            reply.state = if self.engine.is_fatal() {
                RealmState::Fatal
            } else {
                RealmState::Closed
            };
            reply.errors.push("Page realm is no longer active".into());
            return reply;
        }
        let validation = self.state.borrow().host.validate_input(&input);
        if let Err(error) = validation {
            self.closed = true;
            // End the validation borrow before dropping callback/argument roots.
            reply.errors.push(error);
            self.state.borrow_mut().clear_timers();
            reply.boa = Some(self.engine.stats());
            return reply;
        }
        self.revision += 1;
        reply.revision = self.revision;
        {
            let mut data = self.state.borrow_mut();
            data.armed_wake = None;
            data.host.navigation = None;
        }
        let result = (|| {
            for ControlEdit {
                node,
                version,
                value,
            } in input.edits
            {
                let mut data = self.state.borrow_mut();
                if data.host.document.nodes[node].tag == "textarea" {
                    data.host.replace_text(node, value)?;
                } else {
                    data.host.attr(node, "value", value)?;
                }
                reply.acknowledgements.push(ControlAck { node, version });
            }
            match input.kind {
                InputKind::Click { target }
                | InputKind::Submit {
                    submitter: Some(target),
                    ..
                } => self.click_activation(target, &mut reply.outcome, &mut reply.errors),
                InputKind::Submit {
                    form,
                    submitter: None,
                } => self.submit_activation(form, None, &mut reply.outcome, &mut reply.errors),
            }
        })();
        match result {
            Ok(default) => reply.default_action = default,
            Err(error) => {
                self.closed = true;
                reply.errors.push(error);
            }
        }
        self.snapshot(&mut reply);
        reply.errors.truncate(64);
        reply
    }
}

/// One-shot execution shares the actual Boa page path; it never invokes Butane.
pub fn execute(request: Request) -> super::Reply {
    let original = request.html.clone();
    let (realm, reply) = BoaPageRealm::start(request);
    let html = realm
        .as_ref()
        .and_then(|realm| realm.state.borrow().host.serialize(0, true).ok());
    super::Reply {
        applied: html.is_some(),
        html: html.unwrap_or(original),
        navigation: reply.navigation,
        errors: reply.errors,
        scripts_executed: reply.scripts_executed,
        allocations: None,
        boa: reply.boa,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pump_next(realm: &mut BoaPageRealm) -> SessionReply {
        let wake = {
            let mut data = realm.state.borrow_mut();
            let wake = data.armed_wake.expect("advertised owned timer");
            data.test_now = Some(data.now().max(wake.deadline));
            wake
        };
        realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        })
    }
    fn realm(source: &str) -> BoaPageRealm {
        let (realm, reply) = BoaPageRealm::start(Request {
            external_scripts: None,
            url: "https://example.test/owned".into(),
            html: format!(
                "<html><body><p id=out>before</p><a id=a href=/next>Next</a><script>{source}</script></body></html>"
            ),
        });
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        realm.unwrap()
    }
    fn collect(realm: &mut BoaPageRealm) {
        // The real public checkpoint must release WeakRef kept-alive entries;
        // do not repair a broken production checkpoint from test-only code.
        realm.engine.checkpoint().unwrap();
        modern::boa_gc::force_collect();
    }
    fn evaluate(realm: &mut BoaPageRealm, source: &str) {
        realm.engine.evaluate(source).unwrap();
        realm.engine.checkpoint().unwrap();
    }
    #[test]
    fn weak_wrapper_cache_does_not_permanently_root_detached_nodes() {
        let mut realm =
            realm("(() => { const node = document.createElement('div'); node.id='detached'; })();");
        collect(&mut realm);
        let finalized = realm.state.borrow().finalized.get();
        assert!(
            finalized >= 1,
            "detached wrapper should be collected despite arena/cache membership"
        );
        let before = realm.state.borrow().wrappers_created;
        evaluate(&mut realm, "document.getElementById('out');");
        collect(&mut realm);
        evaluate(&mut realm, "document.getElementById('out');");
        assert!(realm.state.borrow().wrappers_created >= before + 2);
    }
    #[test]
    fn retained_node_and_listener_survive_collection_then_release_on_removal() {
        let mut realm = realm(
            r#"
            (() => {
                const node = document.createElement('div');
                const callback = () => node.textContent='called';
                node.addEventListener('click', callback);
                globalThis.removeDetached = () => { node.removeEventListener('click', callback); };
            })();
            globalThis.retained = document.getElementById('a');
            retained.onclick = event => { event.preventDefault(); retained.textContent = 'clicked'; };
        "#,
        );
        collect(&mut realm);
        let before = realm.state.borrow().finalized.get();
        evaluate(
            &mut realm,
            "if(retained!==document.querySelector('#a')) throw 'lost identity';",
        );
        let target = realm
            .state
            .borrow()
            .host
            .document
            .query_selector(0, "#a")
            .unwrap()
            .unwrap();
        let reply = realm.dispatch(SessionInput {
            kind: InputKind::Click { target },
            edits: Vec::new(),
        });
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(reply.outcome.click_canceled, Some(true));
        evaluate(&mut realm, "removeDetached(); removeDetached=null;");
        collect(&mut realm);
        assert!(
            realm.state.borrow().finalized.get() > before,
            "removed listener must release callback/node cycle"
        );
    }
    #[test]
    fn gc_inside_nested_coercion_preserves_native_roots_and_identity() {
        let mut realm = realm("");
        realm
            .engine
            .context_mut()
            .register_global_builtin_callable(
                JsString::from("testCollect"),
                0,
                NativeFunction::from_fn_ptr(|_, _, context| {
                    context.clear_kept_objects();
                    modern::boa_gc::force_collect();
                    Ok(JsValue::undefined())
                }),
            )
            .unwrap();
        evaluate(
            &mut realm,
            r#"
            const node = document.getElementById('out');
            node.textContent = { toString() {
                testCollect();
                node.setAttribute('proof', { toString() { testCollect(); return 'nested'; } });
                if (node !== document.getElementById('out')) throw 'root lost';
                return 'after gc';
            }};
        "#,
        );
        let data = realm.state.borrow();
        let id = data
            .host
            .document
            .query_selector(0, "#out")
            .unwrap()
            .unwrap();
        assert_eq!(data.host.document.nodes[id].attr("proof"), Some("nested"));
        assert_eq!(text_content(&data.host.document.nodes, id), "after gc");
    }
    #[test]
    fn foreign_and_stale_dom_objects_are_rejected_without_old_context_access() {
        let mut first = realm("");
        let foreign = first.engine.evaluate("document.body").unwrap();
        let mut second = realm("");
        second
            .engine
            .context_mut()
            .register_global_property(JsString::from("foreign"), foreign, Attribute::all())
            .unwrap();
        assert!(
            second
                .engine
                .evaluate("document.body.appendChild(foreign)")
                .is_err()
        );
        assert!(
            second
                .engine
                .evaluate("foreign.textContent='wrong'")
                .is_err()
        );
        drop(first);
        modern::boa_gc::force_collect();
        assert!(second.engine.evaluate("foreign.id").is_err());
        evaluate(&mut second, "document.title='still usable';");
    }
    #[test]
    fn realm_drop_releases_all_native_wrapper_roots() {
        let mut realm =
            realm("globalThis.node=document.getElementById('out'); node.onclick=()=>node;");
        collect(&mut realm);
        let (counter, created) = {
            let state = realm.state.borrow();
            (state.finalized.clone(), state.wrappers_created)
        };
        drop(realm);
        modern::boa_gc::force_collect();
        assert_eq!(counter.get(), created);
    }

    #[test]
    fn direct_timer_nesting_clamps_but_promise_scheduled_timers_do_not_inherit_it() {
        let mut direct = realm(
            "var calls=0;function next(){calls++;if(calls<8)setTimeout(next,0);}setTimeout(next,0);",
        );
        for fired in 1..=6 {
            let reply = pump_next(&mut direct);
            assert!(reply.errors.is_empty(), "{:?}", reply.errors);
            assert_eq!(
                reply.task_wake.unwrap().delay_ms,
                if fired == 6 { 4 } else { 0 }
            );
        }
        let mut promises = realm(
            "var calls=0;function next(){calls++;if(calls<8)Promise.resolve().then(()=>setTimeout(next,0));}setTimeout(next,0);",
        );
        for fired in 1..=8 {
            let reply = pump_next(&mut promises);
            assert!(reply.errors.is_empty(), "{:?}", reply.errors);
            if fired < 8 {
                assert_eq!(reply.task_wake.unwrap().delay_ms, 0);
            } else {
                assert!(reply.task_wake.is_none());
            }
        }
    }

    #[test]
    fn timer_callback_and_argument_roots_survive_gc_then_release_on_cancel_or_fire() {
        for cancel in [false, true] {
            let mut realm = realm(
                r#"
                (()=>{
                    const node=document.createElement('div');
                    node.textContent='owned';
                    const id=setTimeout(argument=>{
                        if(argument.node!==node||node.textContent!=='owned')throw 'timer root lost';
                        document.title='rooted';
                    },0,{node});
                    globalThis.cancelOwned=()=>clearTimeout(id);
                })();
            "#,
            );
            let before = realm.state.borrow().finalized.get();
            collect(&mut realm);
            assert_eq!(
                realm.state.borrow().finalized.get(),
                before,
                "pending callback/argument lost its node"
            );
            if cancel {
                evaluate(&mut realm, "cancelOwned();cancelOwned=null;");
                assert!(realm.state.borrow().timers.is_empty());
                assert_eq!(realm.engine.stats().pending_jobs, 0);
            } else {
                let reply = pump_next(&mut realm);
                assert!(reply.errors.is_empty(), "{:?}", reply.errors);
                assert_eq!(
                    document::project_nodes(
                        reply.snapshot.unwrap().nodes,
                        "https://example.test/owned"
                    )
                    .unwrap()
                    .title,
                    "rooted"
                );
                evaluate(&mut realm, "cancelOwned=null;");
            }
            collect(&mut realm);
            assert!(
                realm.state.borrow().finalized.get() > before,
                "completed/canceled roots survived GC"
            );
        }
    }

    #[test]
    fn invalid_input_closes_and_releases_pending_timer_roots_and_accounting() {
        let mut realm = realm(
            "(()=>{const node=document.createElement('div');setTimeout(()=>node.textContent='must not run',100000);})();",
        );
        let before = realm.state.borrow().finalized.get();
        let reply = realm.dispatch(SessionInput {
            kind: InputKind::Click { target: usize::MAX },
            edits: vec![],
        });
        assert_eq!(reply.state, RealmState::Closed);
        assert!(reply.task_wake.is_none());
        assert_eq!(reply.boa.unwrap().pending_jobs, 0);
        assert!(realm.state.borrow().timers.is_empty());
        assert!(realm.state.borrow().armed_wake.is_none());
        collect(&mut realm);
        assert!(realm.state.borrow().finalized.get() > before);
    }

    #[test]
    fn realm_drop_releases_pending_timer_callback_and_argument_roots() {
        let realm = realm(
            "(()=>{const node=document.createElement('div');setTimeout(value=>node.textContent=value,100000,node);})();",
        );
        let finalized = realm.state.borrow().finalized.clone();
        let before = finalized.get();
        drop(realm);
        modern::boa_gc::force_collect();
        assert!(
            finalized.get() > before,
            "timer callback/argument roots survived realm teardown"
        );
    }

    #[test]
    fn fatal_readiness_stops_before_later_startup_event_allocation() {
        let (realm, reply) = BoaPageRealm::start(Request {
            external_scripts: None,
            url: "https://example.test/owned".into(),
            html: "<p>readable</p><script>document.onreadystatechange=()=>{if(document.readyState==='interactive')while(true){}};document.addEventListener('DOMContentLoaded',()=>document.title='must not run');</script>".into(),
        });
        assert_eq!(reply.state, RealmState::Fatal);
        let realm = realm.unwrap();
        assert_eq!(realm.state.borrow().host.ready, "interactive");
        assert_eq!(realm.state.borrow().host.events.len(), 1);
        assert_eq!(reply.boa.unwrap().pending_jobs, 0);
        assert!(reply.task_wake.is_none());
    }

    #[test]
    fn timer_delay_reentry_and_gc_do_not_hold_native_borrows_or_run_jobs_early() {
        let mut realm = realm("");
        realm
            .engine
            .context_mut()
            .register_global_builtin_callable(
                js_string!("ownedCollect"),
                0,
                NativeFunction::from_fn_ptr(|_, _, _| {
                    modern::boa_gc::force_collect();
                    Ok(JsValue::undefined())
                }),
            )
            .unwrap();
        evaluate(
            &mut realm,
            r#"
            var sequence=[];
            (()=>{
                const node=document.createElement('div');
                setTimeout(argument=>{
                    if(argument!==node)throw 'argument identity';
                    document.title=sequence.join(',');
                },{valueOf(){
                    ownedCollect();node.textContent='survived';
                    Promise.resolve().then(()=>sequence.push('job'));
                    sequence.push('convert');return 0;
                }},node);
            })();
            sequence.push('outer');
        "#,
        );
        let mut reply = BoaPageRealm::empty_reply();
        realm.snapshot(&mut reply);
        let reply = pump_next(&mut realm);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(
            document::project_nodes(reply.snapshot.unwrap().nodes, "https://example.test/owned")
                .unwrap()
                .title,
            "convert,outer,job"
        );
    }
}
