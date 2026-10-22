//! Boa page execution over the same bounded Rust DOM and typed activation API.
//!
//! The context owns all JS roots. Native bindings contain only private brands,
//! never an Rc back to their context. Node wrappers are weakly cached; active
//! listeners root their callbacks, and removing a listener releases that root.
//! Active listeners on detached nodes remain rooted until removal or realm
//! teardown, within the unchanged 32-registration/300-second session bounds.
//! This bounded retention policy is not full DOM ephemeron garbage collection.
use super::events::{Kind, Target};
use super::{
    BrowserHost, ClassicErrorVisibility, MAX_SOURCE, Request, bounded, inert, text_content,
};
use crate::{document, page_session::*};
use mg_butane::{
    modern::{self, Engine},
    runtime::{Host, Value},
};
use modern::boa_engine::{
    Context, JsData, JsError, JsNativeError, JsResult, JsString, JsValue, NativeFunction,
    builtins::promise::{OperationType, Promise, PromiseState},
    js_string,
    object::{
        FunctionObjectBuilder, JsObject,
        builtins::{JsPromise, JsProxy, JsWeakMap},
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
const REJECTION_FAILURE: &str = "Promise rejection tracking invariant or storage admission failed";
const UNHANDLED_REJECTION: &str = "Unhandled Promise rejection";
const REJECTION_GAP: &str =
    "Promise rejection observation suppressed by source-exposure policy; handled status unknown.";

#[derive(Trace, JsData)]
struct Brand {
    handle: String,
    realm: u64,
    // Only the actual wrapper retains a thrown value. The Rust-only event
    // record and weak wrapper cache must not become hidden exception roots.
    error: Option<ErrorPayload>,
    rejection: Option<RejectionPayload>,
    #[unsafe_ignore_trace]
    finalized: Rc<Cell<usize>>,
}

#[derive(Clone, Trace, Finalize)]
struct ErrorPayload {
    message: JsString,
    error: JsValue,
}

#[derive(Clone, Trace, Finalize)]
struct RejectionPayload {
    promise: JsObject<Promise>,
    reason: JsValue,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CandidatePhase {
    Pending,
    Frozen,
    Active { handled: bool },
}

#[derive(Trace, Finalize)]
struct RejectionCandidate {
    #[unsafe_ignore_trace]
    id: u64,
    #[unsafe_ignore_trace]
    phase: CandidatePhase,
    #[unsafe_ignore_trace]
    permit: Option<modern::WorkPermit>,
    payload: RejectionPayload,
}

#[derive(Trace, Finalize)]
enum DomTaskKind {
    Batch(#[unsafe_ignore_trace] Vec<u64>),
    Handled {
        #[unsafe_ignore_trace]
        permit: modern::WorkPermit,
        payload: RejectionPayload,
    },
}

enum SelectedTask {
    Timer(Timer),
    Dom(DomTask),
}

#[derive(Trace, Finalize)]
struct DomTask {
    #[unsafe_ignore_trace]
    handle: u64,
    #[unsafe_ignore_trace]
    deadline: Instant,
    kind: DomTaskKind,
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
    rejections: Vec<RejectionCandidate>,
    dom_tasks: Vec<DomTask>,
    // Values are scalar membership markers. This private native map must never
    // turn notified promises/reasons into strong realm-lifetime roots.
    outstanding_rejections: Option<JsWeakMap>,
    #[unsafe_ignore_trace]
    next_candidate: u64,
    #[unsafe_ignore_trace]
    rejection_observation_gap: bool,
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
    #[unsafe_ignore_trace]
    error_exposure_allowed: bool,
    #[unsafe_ignore_trace]
    reporting_error: bool,
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
    fn take_work(
        &mut self,
    ) -> (
        Vec<Timer>,
        Vec<RejectionCandidate>,
        Vec<DomTask>,
        Option<JsWeakMap>,
    ) {
        self.armed_wake = None;
        (
            std::mem::take(&mut self.timers),
            std::mem::take(&mut self.rejections),
            std::mem::take(&mut self.dom_tasks),
            self.outstanding_rejections.take(),
        )
    }
}

// The host ledger admits actual retained Vec capacity, separately from JS
// source statistics. System allocator admission independently covers the native
// allocation itself in a production worker. No queue has a prepaid allowance.
fn reserve_storage<T>(host: &mut BrowserHost, values: &mut Vec<T>, extra: usize) -> Result<(), ()> {
    let needed = values.len().checked_add(extra).ok_or(())?;
    if needed <= values.capacity() {
        return Ok(());
    }
    let old = values.capacity();
    let bytes = (needed - old)
        .checked_mul(std::mem::size_of::<T>())
        .ok_or(())?;
    host.charge(bytes).map_err(|_| ())?;
    values
        .try_reserve_exact(needed - values.len())
        .map_err(|_| ())?;
    let excess = values.capacity().checked_sub(needed).ok_or(())?;
    if excess != 0 {
        host.charge(excess.checked_mul(std::mem::size_of::<T>()).ok_or(())?)
            .map_err(|_| ())?;
    }
    Ok(())
}

fn rejection_exposure(context: &Context) -> Result<bool, ()> {
    let dynamic = modern::context_dynamic_compilation_seen(context).ok_or(())?;
    let shared = context.get_data::<Shared>().ok_or(())?;
    Ok(shared.borrow().error_exposure_allowed && !dynamic)
}

fn observe_rejection(promise: &JsObject<Promise>, operation: OperationType, context: &mut Context) {
    // This no-result hook is bookkeeping only: no script calls, property reads,
    // coercion, diagnostics allocation or retained values before admission.
    if modern::check_context(context).is_err() {
        return;
    }
    if track_rejection(promise, operation, context).is_err() {
        // All State/policy borrows and local permits have ended before latching.
        modern::latch_context(context, REJECTION_FAILURE);
    }
}

fn track_rejection(
    promise: &JsObject<Promise>,
    operation: OperationType,
    context: &mut Context,
) -> Result<(), ()> {
    let allowed = rejection_exposure(context)?;
    let shared = context.get_data::<Shared>().ok_or(())?.clone();
    match operation {
        OperationType::Reject => {
            if !allowed {
                shared.borrow_mut().rejection_observation_gap = true;
                return Ok(());
            }
            if shared
                .borrow()
                .rejections
                .iter()
                .any(|candidate| &candidate.payload.promise == promise)
            {
                return Err(());
            }
            let permit = modern::admit_host_task(context).map_err(|_| ())?;
            let id = {
                let mut data = shared.borrow_mut();
                let id = data.next_candidate.checked_add(1).ok_or(())?;
                let State {
                    host, rejections, ..
                } = &mut *data;
                reserve_storage(host, rejections, 1)?;
                id
            };
            let PromiseState::Rejected(reason) = JsPromise::from(promise.clone()).state() else {
                return Err(());
            };
            let mut data = shared.borrow_mut();
            data.next_candidate = id;
            data.rejections.push(RejectionCandidate {
                id,
                phase: CandidatePhase::Pending,
                permit: Some(permit),
                payload: RejectionPayload {
                    promise: promise.clone(),
                    reason,
                },
            });
        }
        OperationType::Handle => {
            let removed = {
                let mut data = shared.borrow_mut();
                if let Some(index) = data
                    .rejections
                    .iter()
                    .position(|candidate| &candidate.payload.promise == promise)
                {
                    if let CandidatePhase::Active { handled } = &mut data.rejections[index].phase {
                        *handled = true;
                        return Ok(());
                    }
                    Some(data.rejections.swap_remove(index))
                } else {
                    None
                }
            };
            if removed.is_some() {
                drop(removed);
                return Ok(());
            }
            let map = shared.borrow().outstanding_rejections.clone();
            let Some(map) = map else {
                return Ok(());
            };
            let object = promise.clone().upcast();
            let membership = map.get(&object, context).map_err(|_| ())?;
            if membership.is_undefined() {
                return Ok(());
            }
            if membership.as_number() != Some(1.0) {
                return Err(());
            }
            if !map.delete(&object, context).map_err(|_| ())? {
                return Err(());
            }
            if !allowed {
                shared.borrow_mut().rejection_observation_gap = true;
                return Ok(());
            }
            // Late handling creates a genuinely new task, not a refund or reuse
            // of the already consumed notification's reservation.
            let permit = modern::admit_host_task(context).map_err(|_| ())?;
            let (handle, deadline) = {
                let mut data = shared.borrow_mut();
                let handle = data.next_handle.checked_add(1).ok_or(())?;
                let deadline = data.now();
                let State {
                    host, dom_tasks, ..
                } = &mut *data;
                reserve_storage(host, dom_tasks, 1)?;
                (handle, deadline)
            };
            let PromiseState::Rejected(reason) = JsPromise::from(promise.clone()).state() else {
                return Err(());
            };
            let mut data = shared.borrow_mut();
            data.next_handle = handle;
            data.dom_tasks.push(DomTask {
                handle,
                deadline,
                kind: DomTaskKind::Handled {
                    permit,
                    payload: RejectionPayload {
                        promise: promise.clone(),
                        reason,
                    },
                },
            });
        }
    }
    Ok(())
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
    wrap_payload(handle, None, context)
}

fn wrap_payload(
    handle: &str,
    error: Option<ErrorPayload>,
    context: &mut Context,
) -> JsResult<JsValue> {
    wrap_event_payload(handle, error, None, context)
}

fn wrap_event_payload(
    handle: &str,
    error: Option<ErrorPayload>,
    rejection: Option<RejectionPayload>,
    context: &mut Context,
) -> JsResult<JsValue> {
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
            error,
            rejection,
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
    if let Some(value) =
        error_property(&args[0], &name).or_else(|| rejection_property(&args[0], &name))
    {
        return Ok(value);
    }
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
            "cancelable" => {
                return Ok(matches!(
                    event.kind,
                    Kind::Click | Kind::Submit | Kind::Error | Kind::UnhandledRejection
                )
                .into());
            }
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
        if error_property(&args[0], &name).is_some()
            || rejection_property(&args[0], &name).is_some()
        {
            return Ok(true.into());
        }
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

fn error_property(target: &JsValue, name: &str) -> Option<JsValue> {
    let object = target.as_object()?;
    let data = object.downcast_ref::<Brand>()?;
    let payload = data.error.as_ref()?;
    match name {
        "message" => Some(payload.message.clone().into()),
        "filename" => Some(js_string!("").into()),
        "lineno" | "colno" => Some(0.into()),
        "error" => Some(payload.error.clone()),
        _ => None,
    }
}
fn rejection_property(target: &JsValue, name: &str) -> Option<JsValue> {
    let object = target.as_object()?;
    let data = object.downcast_ref::<Brand>()?;
    let payload = data.rejection.as_ref()?;
    match name {
        "promise" => Some(payload.promise.clone().upcast().into()),
        "reason" => Some(payload.reason.clone()),
        _ => None,
    }
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
        if !matches!(
            event.kind,
            Kind::Click | Kind::Submit | Kind::Error | Kind::UnhandledRejection
        ) {
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
        let allocated = match request.validate_modern_external_sources(&document) {
            Ok(bytes) => bytes,
            Err(error) => {
                reply.errors.push(error);
                return (None, reply);
            }
        };
        // Validate and decide the realm's floor before Engine::new, parsing or
        // evaluating even the first inline source. Later foreign/deferred code
        // cannot retroactively make an earlier exposed object safe.
        let error_exposure_allowed = request.external_scripts.as_ref().is_none_or(|sources| {
            sources.iter().all(|source| {
                source.provenance.as_ref().is_some_and(|provenance| {
                    provenance.error_visibility == ClassicErrorVisibility::SameOrigin
                })
            })
        });
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
        let mut realm = match Self::new(host, error_exposure_allowed) {
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
                realm.engine.evaluate_raw(source)
            };
            match result {
                Ok(_) => reply.scripts_executed += 1,
                Err(error) => realm.execution_failure(
                    &format!("{label} script {}", index + 1),
                    error,
                    &mut reply.errors,
                ),
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
    fn new(host: BrowserHost, error_exposure_allowed: bool) -> Result<Self, String> {
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
            rejections: Vec::new(),
            dom_tasks: Vec::new(),
            outstanding_rejections: None,
            next_candidate: 0,
            rejection_observation_gap: false,
            next_timer: 0,
            next_handle: 0,
            next_wake: 0,
            armed_wake: None,
            timer_nesting: None,
            js_depth: 0,
            checkpointing: false,
            error_exposure_allowed,
            reporting_error: false,
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
            "onerror",
            "onunhandledrejection",
            "onrejectionhandled",
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
        engine
            .set_rejection_observer(observe_rejection)
            .map_err(|e| e.to_string())?;
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
    fn execution_failure(
        &mut self,
        prefix: &str,
        failure: modern::ExecutionFailure,
        errors: &mut Vec<String>,
    ) {
        let message = bounded(&failure.diagnostic.to_string(), 512);
        if errors.len() < 64 {
            errors.push(format!("{prefix}: {message}"));
        }
        // The data-only diagnostic survives cancellation, recursive reporting
        // and fatal failure. Never turn a policy failure into another JS call.
        if self.closed || self.engine.is_fatal() || self.state.borrow().reporting_error {
            return;
        }
        let Some(exception) = failure.exception else {
            return;
        };
        let expose =
            self.state.borrow().error_exposure_allowed && !self.engine.dynamic_compilation_seen();
        let (message, error) = if expose {
            // Native conversion uses only captured engine intrinsics, not
            // script properties/coercion. Opaque thrown values retain identity.
            match exception.into_opaque(self.engine.context_mut()) {
                Ok(value) => (message, value),
                Err(_) => {
                    if errors.len() < 64 {
                        errors.push("Error payload conversion rejected".into());
                    }
                    self.closed = true;
                    return;
                }
            }
        } else {
            // Drop the actual foreign value before wrapper allocation/dispatch.
            // An escaped muted event must not retain a hidden foreign root.
            drop(exception);
            ("Script error.".into(), JsValue::null())
        };
        self.window_error(message, error, errors);
    }

    fn window_error(&mut self, message: String, error: JsValue, errors: &mut Vec<String>) {
        if !self.state.borrow().listeners.iter().any(|listener| {
            listener.target == Target::Window
                && listener.kind == Kind::Error
                && listener.callback.is_some()
        }) {
            return;
        }
        self.state.borrow_mut().reporting_error = true;
        let event = (|| {
            let mut data = self.state.borrow_mut();
            // Native payload/storage admission is separate from JS source-text
            // statistics and uses the unchanged cumulative DOM host allowance.
            data.host
                .charge(std::mem::size_of::<ErrorPayload>() + message.len() * 2)?;
            data.host.new_event(Kind::Error, Target::Window, None)
        })();
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                if errors.len() < 64 {
                    errors.push(bounded(&error, 512));
                }
                self.closed = true;
                self.state.borrow_mut().reporting_error = false;
                return;
            }
        };
        let message = JsString::from(message);
        let wrapper = wrap_payload(
            &format!("event:{event}"),
            Some(ErrorPayload {
                message: message.clone(),
                error: error.clone(),
            }),
            self.engine.context_mut(),
        );
        if let Ok(wrapper) = wrapper {
            // This single rooted wrapper spans all callbacks and checkpoints.
            // Only it (or an escaped JS reference) retains the actual payload.
            for capture in [true, false] {
                // Capture and noncapture are separate at-target invocations.
                // stopPropagation leaves the current invocation's remaining
                // listeners eligible, but prevents the following invocation.
                if self.state.borrow().host.events[event].stopped {
                    break;
                }
                let ids: Vec<_> = {
                    let mut data = self.state.borrow_mut();
                    data.host.events[event].current = Some(Target::Window);
                    data.host.events[event].phase = 2;
                    data.listeners
                        .iter()
                        .enumerate()
                        .filter(|(_, listener)| {
                            listener.target == Target::Window
                                && listener.kind == Kind::Error
                                && listener.capture == capture
                                && listener.callback.is_some()
                        })
                        .map(|(id, _)| id)
                        .collect()
                };
                for id in ids {
                    if self.closed
                        || self.engine.is_fatal()
                        || self.state.borrow().host.events[event].immediate
                    {
                        break;
                    }
                    let listener = {
                        let data = self.state.borrow();
                        data.listeners[id]
                            .callback
                            .clone()
                            .map(|callback| (callback, data.listeners[id].property))
                    };
                    let Some((callback, property)) = listener else {
                        continue;
                    };
                    let arguments = if property {
                        vec![
                            message.clone().into(),
                            js_string!("").into(),
                            0.into(),
                            0.into(),
                            error.clone(),
                        ]
                    } else {
                        vec![wrapper.clone()]
                    };
                    let this = self.engine.context().global_object().into();
                    match self.call_function(&callback, &this, &arguments) {
                        Ok(value) if property && value.as_boolean() == Some(true) => {
                            self.state.borrow_mut().host.events[event].canceled = true;
                        }
                        Ok(_) => {}
                        Err(error) => self.execution_failure("Event handler", error, errors),
                    }
                    self.checkpoint(errors);
                }
            }
        } else {
            if errors.len() < 64 {
                errors.push("Error event wrapper rejected".into());
            }
            self.closed = true;
        }
        let mut data = self.state.borrow_mut();
        data.host.events[event].current = None;
        data.host.events[event].phase = 0;
        data.reporting_error = false;
    }

    fn checkpoint(&mut self, errors: &mut Vec<String>) {
        if self.closed {
            self.engine.context_mut().clear_kept_objects();
            return;
        }
        let old_nesting = {
            let mut data = self.state.borrow_mut();
            if data.js_depth != 0 || data.checkpointing {
                return;
            }
            data.checkpointing = true;
            // A timer created by a Promise job is not nested in a timer task.
            data.timer_nesting.take()
        };
        let result = self.engine.checkpoint_raw();
        // run_jobs has returned and no JS stack is active. Error reporting may
        // now checkpoint its own callbacks, never re-enter an active executor.
        self.state.borrow_mut().checkpointing = false;
        if let Err(error) = result {
            self.execution_failure("Promise checkpoint", error, errors);
        }
        self.state.borrow_mut().timer_nesting = old_nesting;
        if !self.closed && !self.engine.is_fatal() {
            if self.freeze_rejections().is_err() {
                self.rejection_failure(errors);
            }
        }
    }

    fn clear_work(&mut self) {
        // In particular, a permit's Drop must not run under the State borrow.
        let discarded = self.state.borrow_mut().take_work();
        drop(discarded);
    }

    fn rejection_failure(&mut self, errors: &mut Vec<String>) {
        let error = self.engine.latch_failure(REJECTION_FAILURE);
        if errors.len() < 64 {
            errors.push(bounded(&error.to_string(), 512));
        }
    }

    fn observation_notice(&self, errors: &mut Vec<String>) {
        if self.state.borrow().rejection_observation_gap
            && errors.len() < 64
            && !errors.iter().any(|error| error == REJECTION_GAP)
        {
            errors.push(REJECTION_GAP.into());
        }
    }

    fn prune_batches(&mut self) {
        let mut data = self.state.borrow_mut();
        let State {
            rejections,
            dom_tasks,
            ..
        } = &mut *data;
        dom_tasks.retain(|task| match &task.kind {
            DomTaskKind::Batch(ids) => ids
                .iter()
                .any(|id| rejections.iter().any(|candidate| candidate.id == *id)),
            DomTaskKind::Handled { .. } => true,
        });
    }

    fn purge_private_notices(&mut self) -> Result<bool, ()> {
        if rejection_exposure(self.engine.context())? {
            return Ok(false);
        }
        let discarded = {
            let mut data = self.state.borrow_mut();
            if !data.rejections.is_empty()
                || !data.dom_tasks.is_empty()
                || data.outstanding_rejections.is_some()
            {
                data.rejection_observation_gap = true;
            }
            (
                std::mem::take(&mut data.rejections),
                std::mem::take(&mut data.dom_tasks),
                data.outstanding_rejections.take(),
            )
        };
        drop(discarded);
        Ok(true)
    }

    fn freeze_rejections(&mut self) -> Result<(), ()> {
        if self.purge_private_notices()? {
            return Ok(());
        }
        self.prune_batches();
        let mut data = self.state.borrow_mut();
        let count = data
            .rejections
            .iter()
            .filter(|candidate| candidate.phase == CandidatePhase::Pending)
            .count();
        if count == 0 {
            return Ok(());
        }
        let handle = data.next_handle.checked_add(1).ok_or(())?;
        let deadline = data.now();
        let mut ids = Vec::new();
        reserve_storage(&mut data.host, &mut ids, count)?;
        let State {
            host, dom_tasks, ..
        } = &mut *data;
        reserve_storage(host, dom_tasks, 1)?;
        for candidate in &mut data.rejections {
            if candidate.phase == CandidatePhase::Pending {
                ids.push(candidate.id);
                candidate.phase = CandidatePhase::Frozen;
            }
        }
        // Vec swap-removal on Handle must not change original Reject order.
        ids.sort_unstable();
        data.next_handle = handle;
        data.dom_tasks.push(DomTask {
            handle,
            deadline,
            kind: DomTaskKind::Batch(ids),
        });
        Ok(())
    }

    fn add_outstanding(&mut self, promise: &JsObject<Promise>) -> Result<(), ()> {
        // The native WeakMap's own GC/hash allocations are admitted by the
        // worker System allocator. Charge each retained native membership and
        // the optional map wrapper separately in the cumulative host ledger.
        let map = {
            let mut data = self.state.borrow_mut();
            let map_storage = usize::from(data.outstanding_rejections.is_none())
                * std::mem::size_of::<JsWeakMap>();
            data.host.charge(256 + map_storage).map_err(|_| ())?;
            data.outstanding_rejections.clone()
        };
        let map = match map {
            Some(map) => map,
            None => {
                let map = JsWeakMap::new(self.engine.context_mut());
                self.state.borrow_mut().outstanding_rejections = Some(map.clone());
                map
            }
        };
        let object = promise.clone().upcast();
        if map
            .has(&object, self.engine.context_mut())
            .map_err(|_| ())?
        {
            return Err(());
        }
        map.set(&object, 1.into(), self.engine.context_mut())
            .map_err(|_| ())?;
        Ok(())
    }

    fn rejection_event(&mut self, kind: Kind, payload: RejectionPayload, errors: &mut Vec<String>) {
        let event = {
            let mut data = self.state.borrow_mut();
            data.host
                .charge(std::mem::size_of::<RejectionPayload>())
                .and_then(|_| data.host.new_event(kind, Target::Window, None))
        };
        let event = match event {
            Ok(event) => event,
            Err(_) => {
                self.rejection_failure(errors);
                return;
            }
        };
        let wrapper = match wrap_event_payload(
            &format!("event:{event}"),
            None,
            Some(payload),
            self.engine.context_mut(),
        ) {
            Ok(wrapper) => wrapper,
            Err(_) => {
                self.rejection_failure(errors);
                return;
            }
        };
        for capture in [true, false] {
            if self.closed
                || self.engine.is_fatal()
                || self.state.borrow().host.events[event].stopped
            {
                break;
            }
            let ids = {
                let mut data = self.state.borrow_mut();
                data.host.events[event].current = Some(Target::Window);
                data.host.events[event].phase = 2;
                let count = data
                    .listeners
                    .iter()
                    .filter(|listener| {
                        listener.target == Target::Window
                            && listener.kind == kind
                            && listener.capture == capture
                            && listener.callback.is_some()
                    })
                    .count();
                let mut ids = Vec::new();
                match reserve_storage(&mut data.host, &mut ids, count) {
                    Ok(()) => {
                        ids.extend(
                            data.listeners
                                .iter()
                                .enumerate()
                                .filter(|(_, listener)| {
                                    listener.target == Target::Window
                                        && listener.kind == kind
                                        && listener.capture == capture
                                        && listener.callback.is_some()
                                })
                                .map(|(id, _)| id),
                        );
                        Some(ids)
                    }
                    Err(()) => None,
                }
            };
            let Some(ids) = ids else {
                self.rejection_failure(errors);
                break;
            };
            for id in ids {
                if self.closed
                    || self.engine.is_fatal()
                    || self.state.borrow().host.events[event].immediate
                {
                    break;
                }
                let listener = {
                    let data = self.state.borrow();
                    data.listeners[id]
                        .callback
                        .clone()
                        .map(|callback| (callback, data.listeners[id].property))
                };
                let Some((callback, property)) = listener else {
                    continue;
                };
                let this = self.engine.context().global_object().into();
                match self.call_function(&callback, &this, std::slice::from_ref(&wrapper)) {
                    Ok(value)
                        if property
                            && kind == Kind::UnhandledRejection
                            && value.as_boolean() == Some(false) =>
                    {
                        self.state.borrow_mut().host.events[event].canceled = true;
                    }
                    Ok(_) => {}
                    Err(error) => self.execution_failure("Event handler", error, errors),
                }
                self.checkpoint(errors);
            }
        }
        let mut data = self.state.borrow_mut();
        data.host.events[event].current = None;
        data.host.events[event].phase = 0;
    }

    fn rejection_batch(&mut self, ids: &[u64], errors: &mut Vec<String>) {
        for id in ids {
            if self.closed || self.engine.is_fatal() {
                break;
            }
            match self.purge_private_notices() {
                Ok(true) => break,
                Ok(false) => {}
                Err(()) => {
                    self.rejection_failure(errors);
                    break;
                }
            }
            let permit = {
                let mut data = self.state.borrow_mut();
                data.rejections
                    .iter_mut()
                    .find(|candidate| candidate.id == *id)
                    .map(|candidate| {
                        if candidate.phase != CandidatePhase::Frozen {
                            return Err(());
                        }
                        candidate.phase = CandidatePhase::Active { handled: false };
                        candidate.permit.take().ok_or(())
                    })
            };
            let mut permit = match permit {
                None => continue, // handled while frozen, before its turn
                Some(Ok(permit)) => permit,
                Some(Err(())) => {
                    self.rejection_failure(errors);
                    break;
                }
            };
            if modern::begin_host_task(self.engine.context_mut(), &mut permit).is_err() {
                self.rejection_failure(errors);
                break;
            }
            let payload = self
                .state
                .borrow()
                .rejections
                .iter()
                .find(|candidate| candidate.id == *id)
                .map(|candidate| candidate.payload.clone());
            let Some(payload) = payload else {
                self.rejection_failure(errors);
                break;
            };
            if errors.len() < 64 {
                errors.push(UNHANDLED_REJECTION.into());
            }
            self.rejection_event(Kind::UnhandledRejection, payload, errors);
            // Active tracking survives every callback/error-report checkpoint.
            // Boa's Handle hook runs before its private handled flag changes.
            if self.closed || self.engine.is_fatal() {
                break;
            }
            match self.purge_private_notices() {
                Ok(true) => break,
                Ok(false) => {}
                Err(()) => {
                    self.rejection_failure(errors);
                    break;
                }
            }
            let candidate = {
                let mut data = self.state.borrow_mut();
                data.rejections
                    .iter()
                    .position(|candidate| candidate.id == *id)
                    .map(|index| data.rejections.swap_remove(index))
            };
            match candidate {
                Some(candidate)
                    if candidate.phase == (CandidatePhase::Active { handled: false }) =>
                {
                    if self.add_outstanding(&candidate.payload.promise).is_err() {
                        self.rejection_failure(errors);
                        break;
                    }
                }
                Some(candidate)
                    if candidate.phase == (CandidatePhase::Active { handled: true }) => {}
                _ => {
                    self.rejection_failure(errors);
                    break;
                }
            }
        }
    }
    fn call_function(
        &mut self,
        callback: &JsObject,
        this: &JsValue,
        arguments: &[JsValue],
    ) -> Result<JsValue, modern::ExecutionFailure> {
        let _ingress = Ingress::enter(self.state.clone());
        self.engine.call_raw(callback, this, arguments)
    }
    fn invoke(
        &mut self,
        callback: JsObject,
        target: Target,
        event: &str,
        property: bool,
        errors: &mut Vec<String>,
    ) -> bool {
        let prepared: Result<(JsValue, JsValue), String> = (|| {
            let this = from_host(target.value(), self.engine.context_mut())
                .map_err(|_| "Invalid callback target".to_string())?;
            let event = wrap(event, self.engine.context_mut())
                .map_err(|_| "Invalid event wrapper".to_string())?;
            Ok((this, event))
        })();
        let canceled = match prepared {
            Ok((this, event)) => match self.call_function(&callback, &this, &[event]) {
                Ok(value) => property && value.as_boolean() == Some(false),
                Err(error) => {
                    self.execution_failure("Event handler", error, errors);
                    false
                }
            },
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
        if !self.engine.is_fatal() && !self.closed {
            match self.purge_private_notices() {
                Ok(_) => self.prune_batches(),
                Err(()) => self.rejection_failure(&mut reply.errors),
            }
        }
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
            self.clear_work();
        } else if let Err(error) = self.advertise_wake(reply) {
            reply.errors.push(error);
            reply.state = RealmState::Closed;
            reply.default_action = DefaultAction::None;
            self.closed = true;
            self.clear_work();
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
        }
        drop(data);
        if reply.state != RealmState::Ready {
            self.clear_work();
        }
        self.observation_notice(&mut reply.errors);
        reply.boa = Some(self.engine.stats());
    }

    fn advertise_wake(&mut self, reply: &mut SessionReply) -> Result<(), String> {
        let mut data = self.state.borrow_mut();
        data.armed_wake = None;
        reply.task_wake = None;
        let timer = data
            .timers
            .iter()
            .min_by_key(|timer| (timer.deadline, timer.handle))
            .map(|timer| (timer.handle, timer.deadline));
        let dom = data
            .dom_tasks
            .iter()
            .min_by_key(|task| (task.deadline, task.handle))
            .map(|task| (task.handle, task.deadline));
        let Some((handle, deadline)) = timer
            .into_iter()
            .chain(dom)
            .min_by_key(|(handle, deadline)| (*deadline, *handle))
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

    /// Run one timer, frozen batch or handled task after a parent transaction.
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
                    if let Some(index) = data
                        .timers
                        .iter()
                        .position(|timer| timer.handle == wake.handle)
                    {
                        Some((true, index))
                    } else {
                        data.dom_tasks
                            .iter()
                            .position(|task| task.handle == wake.handle)
                            .map(|index| (false, index))
                    }
                })
        };
        if self.closed || self.engine.is_fatal() || eligible.is_none() {
            self.closed = true;
            self.clear_work();
            reply
                .errors
                .push("Inactive realm or early, stale or replayed task wake".into());
            reply.state = if self.engine.is_fatal() {
                RealmState::Fatal
            } else {
                RealmState::Closed
            };
            reply.boa = Some(self.engine.stats());
            self.observation_notice(&mut reply.errors);
            return reply;
        }
        self.revision += 1;
        reply.revision = self.revision;
        let selected = {
            let mut data = self.state.borrow_mut();
            data.armed_wake = None;
            data.host.navigation = None;
            let (timer, index) = eligible.unwrap();
            if timer {
                SelectedTask::Timer(data.timers.swap_remove(index))
            } else {
                SelectedTask::Dom(data.dom_tasks.swap_remove(index))
            }
        };
        match selected {
            SelectedTask::Timer(mut timer) => {
                match modern::begin_host_task(self.engine.context_mut(), &mut timer.permit) {
                    Ok(()) => {
                        self.state.borrow_mut().timer_nesting = Some(timer.nesting);
                        let this = self.engine.context().global_object().into();
                        if let Err(error) =
                            self.call_function(&timer.callback, &this, &timer.arguments)
                        {
                            self.execution_failure("Timer callback", error, &mut reply.errors);
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
            }
            SelectedTask::Dom(mut task) => {
                self.state.borrow_mut().timer_nesting = None;
                match &mut task.kind {
                    DomTaskKind::Batch(ids) => self.rejection_batch(&ids, &mut reply.errors),
                    DomTaskKind::Handled { permit, payload } => {
                        match rejection_exposure(self.engine.context()) {
                            Ok(true) => {
                                if modern::begin_host_task(self.engine.context_mut(), permit)
                                    .is_err()
                                {
                                    self.rejection_failure(&mut reply.errors);
                                } else {
                                    self.rejection_event(
                                        Kind::RejectionHandled,
                                        payload.clone(),
                                        &mut reply.errors,
                                    );
                                }
                            }
                            Ok(false) => self.state.borrow_mut().rejection_observation_gap = true,
                            Err(()) => self.rejection_failure(&mut reply.errors),
                        }
                    }
                }
            }
        }
        // Selected roots are dropped before the final real checkpoint, including
        // WeakRef kept-object cleanup. Later tasks are never recursively pumped.
        self.checkpoint(&mut reply.errors);
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
            if self.closed
                || self.engine.is_fatal()
                || self.state.borrow().host.events[event].immediate
            {
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
            self.clear_work();
            reply.boa = Some(self.engine.stats());
            reply.state = if self.engine.is_fatal() {
                RealmState::Fatal
            } else {
                RealmState::Closed
            };
            reply.errors.push("Page realm is no longer active".into());
            self.observation_notice(&mut reply.errors);
            return reply;
        }
        let validation = self.state.borrow().host.validate_input(&input);
        if let Err(error) = validation {
            self.closed = true;
            // End the validation borrow before dropping callback/argument roots.
            reply.errors.push(error);
            self.clear_work();
            reply.boa = Some(self.engine.stats());
            self.observation_notice(&mut reply.errors);
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

impl Drop for BoaPageRealm {
    fn drop(&mut self) {
        self.clear_work();
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

    fn terminal_rejection_snapshot(realm: &mut BoaPageRealm) -> SessionReply {
        let mut reply = BoaPageRealm::empty_reply();
        realm.snapshot(&mut reply);
        assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
        assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
        assert!(reply.task_wake.is_none());
        let data = realm.state.borrow();
        assert!(data.rejections.is_empty());
        assert!(data.dom_tasks.is_empty());
        assert!(data.timers.is_empty());
        assert!(data.outstanding_rejections.is_none());
        reply
    }

    #[test]
    fn rejection_candidate_storage_failure_latches_before_retaining_payloads() {
        let mut realm =
            realm("var value={owned:true};window.onerror=()=>document.title='wrong reporter';");
        realm.state.borrow_mut().host.allocated = super::super::MAX_DOM_BYTES;
        assert!(realm.engine.evaluate("Promise.reject(value);").is_err());
        assert!(realm.engine.is_fatal());
        assert_eq!(realm.engine.stats().jobs_admitted, 1);
        assert!(realm.state.borrow().rejections.is_empty());
        assert!(realm.state.borrow().dom_tasks.is_empty());
        terminal_rejection_snapshot(&mut realm);
    }

    #[test]
    fn rejection_batch_storage_failure_latches_and_terminal_publication_releases_roots() {
        let mut realm = realm("");
        collect(&mut realm);
        let before = realm.state.borrow().finalized.get();
        realm
            .engine
            .evaluate("(()=>{const value=document.createElement('div');Promise.reject(value);})();")
            .unwrap();
        assert_eq!(realm.state.borrow().rejections.len(), 1);
        realm.state.borrow_mut().host.allocated = super::super::MAX_DOM_BYTES;
        let mut errors = Vec::new();
        realm.checkpoint(&mut errors);
        assert!(realm.engine.is_fatal());
        assert_eq!(realm.engine.stats().jobs_admitted, 1);
        terminal_rejection_snapshot(&mut realm);
        modern::boa_gc::force_collect();
        assert!(realm.state.borrow().finalized.get() > before);
    }

    #[test]
    fn rejection_candidate_and_batch_id_overflow_are_fatal_not_catchable_success() {
        for candidate_id in [true, false] {
            let mut realm = realm("window.onerror=()=>document.title='wrong reporter';");
            if candidate_id {
                realm.state.borrow_mut().next_candidate = u64::MAX;
                assert!(realm.engine.evaluate("Promise.reject(1);").is_err());
            } else {
                realm.engine.evaluate("Promise.reject(1);").unwrap();
                realm.state.borrow_mut().next_handle = u64::MAX;
                realm.checkpoint(&mut Vec::new());
            }
            assert!(realm.engine.is_fatal());
            assert_eq!(realm.engine.stats().jobs_admitted, 1);
            terminal_rejection_snapshot(&mut realm);
        }
    }

    #[test]
    fn late_handled_task_id_overflow_latches_from_the_no_result_hook() {
        let mut realm = realm("var p=Promise.reject(1);");
        assert_eq!(pump_next(&mut realm).errors, [UNHANDLED_REJECTION]);
        realm.state.borrow_mut().next_handle = u64::MAX;
        assert!(realm.engine.evaluate("p.catch(()=>{});").is_err());
        assert!(realm.engine.is_fatal());
        assert_eq!(realm.engine.stats().jobs_admitted, 2);
        terminal_rejection_snapshot(&mut realm);
    }

    #[test]
    fn malformed_private_weak_membership_latches_from_the_no_result_handle_hook() {
        let mut realm = realm("var p=Promise.reject(1);");
        assert_eq!(pump_next(&mut realm).errors, [UNHANDLED_REJECTION]);
        let map = realm.state.borrow().outstanding_rejections.clone().unwrap();
        let context = realm.engine.context_mut();
        let object = context
            .global_object()
            .get(js_string!("p"), context)
            .unwrap()
            .as_object()
            .unwrap();
        map.set(&object, 2.into(), context).unwrap();
        assert!(realm.engine.evaluate("p.catch(()=>{});").is_err());
        assert!(realm.engine.is_fatal());
        terminal_rejection_snapshot(&mut realm);
    }

    #[test]
    fn missing_context_policy_cannot_authorize_rejection_exposure_or_retention() {
        let mut realm = realm("");
        let mut bare = Context::default();
        // A binding State alone is not execution-policy authority. The policy
        // type remains Butane-private; do not add a public removal/fault API.
        bare.insert_data(realm.state.clone());
        assert_eq!(modern::context_dynamic_compilation_seen(&bare), None);
        assert!(rejection_exposure(&bare).is_err());
        assert!(
            modern::check_context(&mut bare)
                .unwrap_err()
                .as_engine()
                .is_some()
        );
        assert_eq!(realm.engine.stats().jobs_admitted, 0);
        assert!(realm.state.borrow().rejections.is_empty());
        realm.engine.check().unwrap();
    }

    #[test]
    fn invalid_input_and_realm_drop_release_pending_native_reason_roots() {
        for invalid_input in [true, false] {
            let mut realm = realm(
                "(()=>{const value=document.createElement('div');Promise.reject(value);})();",
            );
            collect(&mut realm);
            let finalized = realm.state.borrow().finalized.clone();
            let before = finalized.get();
            if invalid_input {
                let reply = realm.dispatch(SessionInput {
                    kind: InputKind::Click { target: usize::MAX },
                    edits: vec![],
                });
                assert_eq!(reply.state, RealmState::Closed);
                assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
                assert!(realm.state.borrow().rejections.is_empty());
                collect(&mut realm);
                assert!(finalized.get() > before);
            } else {
                drop(realm);
                modern::boa_gc::force_collect();
                assert!(finalized.get() > before);
            }
        }
    }

    #[test]
    fn active_fatal_delivery_clears_binding_roots_and_realm_teardown_releases_vm_payloads() {
        for escaped in [false, true] {
            let mut realm = realm(&format!(
                "window.onerror=()=>document.title='wrong reporter';document.getElementById('a').onclick=()=>document.title='wrong later listener';window.addEventListener('unhandledrejection',event=>{{document.title='callback entered';{}while(true){{}}}});(()=>{{const value=document.createElement('div');Promise.reject(value);}})();",
                if escaped {
                    "globalThis.saved=event;"
                } else {
                    ""
                },
            ));
            collect(&mut realm);
            let finalized = realm.state.borrow().finalized.clone();
            let before = finalized.get();
            let reply = pump_next(&mut realm);
            assert_eq!(reply.state, RealmState::Fatal);
            assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
            assert!(reply.task_wake.is_none());
            assert!(reply.navigation.is_none());
            {
                let data = realm.state.borrow();
                assert!(data.rejections.is_empty());
                assert!(data.dom_tasks.is_empty());
                assert!(data.timers.is_empty());
                assert!(data.outstanding_rejections.is_none());
                assert!(data.armed_wake.is_none());
                assert_eq!(
                    document::project_nodes(
                        data.host.document.nodes.clone(),
                        "https://example.test/owned"
                    )
                    .unwrap()
                    .title,
                    "callback entered"
                );
            }
            let target = realm
                .state
                .borrow()
                .host
                .document
                .query_selector(0, "#a")
                .unwrap()
                .unwrap();
            let rejected = realm.dispatch(SessionInput {
                kind: InputKind::Click { target },
                edits: vec![],
            });
            assert_eq!(rejected.state, RealmState::Fatal);
            assert!(rejected.snapshot.is_none());
            assert_eq!(
                document::project_nodes(
                    realm.state.borrow().host.document.nodes.clone(),
                    "https://example.test/owned"
                )
                .unwrap()
                .title,
                "callback entered"
            );
            modern::boa_gc::force_collect();
            // Pinned Boa's fuzz-opcode early return can retain the fatal call's
            // argument in its private VM value stack until Context teardown.
            // The original stricter control measured before=0, after=0; do not
            // fake reclamation with ClearKeptObjects or reset an Engine/budget.
            // Terminal library callers must drop the realm, as workers do.
            drop(realm);
            modern::boa_gc::force_collect();
            assert!(finalized.get() >= before + 2);
        }
    }

    #[test]
    fn newly_private_handled_task_cancels_instead_of_claiming_execution() {
        let mut realm = realm("var p=Promise.reject(1);");
        pump_next(&mut realm);
        evaluate(&mut realm, "p.catch(()=>{});");
        let mut queued = BoaPageRealm::empty_reply();
        realm.snapshot(&mut queued);
        assert_eq!(queued.boa.as_ref().unwrap().pending_jobs, 1);
        let executed = queued.boa.as_ref().unwrap().jobs_executed;
        // Private transition control, not a new page-controlled policy API.
        realm.state.borrow_mut().error_exposure_allowed = false;
        let reply = pump_next(&mut realm);
        assert_eq!(reply.state, RealmState::Ready);
        assert_eq!(reply.errors, [REJECTION_GAP]);
        assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, executed);
        assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
        assert!(reply.task_wake.is_none());
    }

    #[test]
    fn native_membership_storage_failure_is_fatal_without_another_reporter() {
        let mut realm = realm("");
        realm
            .engine
            .context_mut()
            .register_global_builtin_callable(
                js_string!("testExhaustHost"),
                0,
                NativeFunction::from_fn_ptr(|_, _, context| {
                    state(context).borrow_mut().host.allocated = super::super::MAX_DOM_BYTES;
                    Ok(JsValue::undefined())
                }),
            )
            .unwrap();
        evaluate(
            &mut realm,
            "window.onerror=()=>document.title='wrong reporter';window.addEventListener('unhandledrejection',()=>testExhaustHost());Promise.reject(1);",
        );
        realm.checkpoint(&mut Vec::new());
        let mut queued = BoaPageRealm::empty_reply();
        realm.snapshot(&mut queued);
        let reply = pump_next(&mut realm);
        assert_eq!(reply.state, RealmState::Fatal);
        assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
        assert!(reply.task_wake.is_none());
        assert!(realm.state.borrow().outstanding_rejections.is_none());
    }

    #[test]
    fn rejection_delivery_preserves_the_existing_cumulative_event_record_cap() {
        let mut realm = realm(
            "window.onerror=()=>document.title='wrong reporter';window.addEventListener('unhandledrejection',()=>document.title='wrong notification');Promise.reject(1);",
        );
        {
            let mut data = realm.state.borrow_mut();
            for _ in 0..MAX_EVENTS {
                data.host
                    .new_event(Kind::Click, Target::Window, None)
                    .unwrap();
            }
        }
        let reply = pump_next(&mut realm);
        assert_eq!(reply.state, RealmState::Fatal);
        assert_eq!(realm.state.borrow().host.events.len(), MAX_EVENTS);
        let title = document::project_nodes(
            realm.state.borrow().host.document.nodes.clone(),
            "https://example.test/owned",
        )
        .unwrap()
        .title;
        assert_ne!(title, "wrong reporter");
        assert_ne!(title, "wrong notification");
        assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
        assert!(reply.task_wake.is_none());
    }
    #[test]
    fn escaped_error_event_roots_its_exception_then_releases_it_when_unreachable() {
        let mut realm = realm(
            "window.addEventListener('error',event=>globalThis.savedError=event);setTimeout(()=>{const node=document.createElement('div');node.id='thrown';throw node;},0);",
        );
        collect(&mut realm);
        let before = realm.state.borrow().finalized.get();
        let reply = pump_next(&mut realm);
        assert_eq!(reply.errors.len(), 1);
        collect(&mut realm);
        let retained = realm.state.borrow().finalized.get();
        evaluate(
            &mut realm,
            "if(savedError.error.id!=='thrown')throw 'exception root lost';",
        );
        collect(&mut realm);
        assert_eq!(realm.state.borrow().finalized.get(), retained);
        evaluate(&mut realm, "savedError=null;");
        collect(&mut realm);
        assert!(
            realm.state.borrow().finalized.get() >= before + 2,
            "both event and thrown wrapper must release"
        );
    }

    #[test]
    fn escaped_muted_error_event_does_not_root_the_unexposed_exception() {
        let html = "<html><body><script>window.addEventListener('error',event=>globalThis.savedError=event);setTimeout(()=>{const node=document.createElement('div');node.id='foreign';throw node;},0);</script><script id=unknown src='/unknown.js'></script></body></html>";
        let parsed = document::parse_with_scripting(html, "https://example.test/owned", true);
        let node = parsed.query_selector(0, "#unknown").unwrap().unwrap();
        let (realm, reply) = BoaPageRealm::start(Request {
            url: "https://example.test/owned".into(),
            html: html.into(),
            external_scripts: Some(vec![super::super::ExternalScriptSource {
                node,
                source: String::new(),
                provenance: None,
            }]),
        });
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        let mut realm = realm.unwrap();
        collect(&mut realm);
        let before = realm.state.borrow().finalized.get();
        let fired = pump_next(&mut realm);
        assert_eq!(fired.errors.len(), 1);
        collect(&mut realm);
        assert!(
            realm.state.borrow().finalized.get() > before,
            "muted exception must release even while event escapes"
        );
        evaluate(
            &mut realm,
            "if(savedError.error!==null||savedError.message!=='Script error.')throw 'muted payload';",
        );
    }

    #[test]
    fn collection_between_error_listeners_keeps_the_same_event_and_exception() {
        let mut realm = realm("");
        realm
            .engine
            .context_mut()
            .register_global_builtin_callable(
                JsString::from("testCollect"),
                0,
                NativeFunction::from_fn_ptr(|_, _, context| {
                    let _ = context;
                    modern::boa_gc::force_collect();
                    Ok(JsValue::undefined())
                }),
            )
            .unwrap();
        evaluate(
            &mut realm,
            r#"
            var sameEvent,value={proof:42};
            window.addEventListener('error',event=>{sameEvent=event;testCollect();if(event.error!==value)throw 'payload lost';});
            window.addEventListener('error',event=>{testCollect();if(event!==sameEvent||event.error!==value)throw 'identity lost';document.title='same rooted event';});
            setTimeout(()=>{throw value;},0);
        "#,
        );
        let mut advertised = BoaPageRealm::empty_reply();
        realm.snapshot(&mut advertised);
        let reply = pump_next(&mut realm);
        assert_eq!(reply.errors.len(), 1, "{:?}", reply.errors);
        assert_eq!(
            document::project_nodes(reply.snapshot.unwrap().nodes, "https://example.test/owned")
                .unwrap()
                .title,
            "same rooted event"
        );
    }

    #[test]
    fn native_error_message_is_bounded_in_unicode_scalars_not_utf16_units() {
        let mut realm = realm(
            r#"
            window.addEventListener('error',event=>{
                if(Array.from(event.message).length!==512||event.message.length<=512||
                    event.filename!==''||event.lineno!==0||event.colno!==0)throw 'message bound';
                document.title='Unicode bounded';
            });
            TypeError=function(){throw 'page constructor ran';};
        "#,
        );
        let exception: JsError = JsNativeError::typ().with_message("🦀".repeat(800)).into();
        let diagnostic = modern::safe_error(&exception);
        let mut errors = Vec::new();
        realm.execution_failure(
            "Owned native exception",
            modern::ExecutionFailure {
                diagnostic,
                exception: Some(exception),
            },
            &mut errors,
        );
        assert_eq!(errors.len(), 1, "{errors:?}");
        let mut reply = BoaPageRealm::empty_reply();
        realm.snapshot(&mut reply);
        assert_eq!(
            document::project_nodes(reply.snapshot.unwrap().nodes, "https://example.test/owned")
                .unwrap()
                .title,
            "Unicode bounded"
        );
    }

    #[test]
    fn ordinary_native_job_failure_still_cancels_old_queue_then_reports_and_drains_new_jobs() {
        use modern::boa_engine::job::GenericJob;
        let mut realm = realm(
            r#"
            window.onerror=(message,file,line,column,error)=>{
                if(error!==42)throw 'native job identity';
                Promise.resolve().then(()=>document.title='reporter checkpoint');
            };
        "#,
        );
        let canceled_job = Rc::new(Cell::new(false));
        let observed = canceled_job.clone();
        let js_realm = realm.engine.context().realm().clone();
        realm.engine.context_mut().enqueue_job(
            GenericJob::new(|_| Err(JsError::from_opaque(42.into())), js_realm.clone()).into(),
        );
        realm.engine.context_mut().enqueue_job(
            GenericJob::new(
                move |_| {
                    observed.set(true);
                    Ok(JsValue::undefined())
                },
                js_realm,
            )
            .into(),
        );
        let mut errors = Vec::new();
        realm.checkpoint(&mut errors);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            !canceled_job.get(),
            "old native-job failure queue behavior changed"
        );
        assert_eq!(realm.engine.stats().pending_jobs, 0);
        assert_eq!(realm.engine.stats().jobs_admitted, 3);
        assert_eq!(realm.engine.stats().jobs_executed, 2);
        let mut reply = BoaPageRealm::empty_reply();
        realm.snapshot(&mut reply);
        assert_eq!(reply.state, RealmState::Ready);
        assert!(reply.task_wake.is_none());
        assert_eq!(
            document::project_nodes(reply.snapshot.unwrap().nodes, "https://example.test/owned")
                .unwrap()
                .title,
            "reporter checkpoint"
        );
    }

    #[test]
    fn error_payload_admission_failure_runs_no_reporter_and_releases_tasks() {
        let mut realm = realm(
            "var reported=false;window.onerror=()=>{reported=true;};setTimeout(()=>{},100000);",
        );
        let events = realm.state.borrow().host.events.len();
        realm.state.borrow_mut().host.allocated = super::super::MAX_DOM_BYTES;
        let exception = JsError::from_opaque(42.into());
        let mut errors = Vec::new();
        realm.execution_failure(
            "Owned admission failure",
            modern::ExecutionFailure {
                diagnostic: modern::safe_error(&exception),
                exception: Some(exception),
            },
            &mut errors,
        );
        assert!(realm.closed);
        assert_eq!(realm.state.borrow().host.events.len(), events);
        assert_eq!(
            realm.engine.get_global("reported").unwrap().as_boolean(),
            Some(false)
        );
        let mut reply = BoaPageRealm::empty_reply();
        realm.snapshot(&mut reply);
        assert_eq!(reply.state, RealmState::Closed);
        assert!(reply.task_wake.is_none());
        assert_eq!(reply.boa.unwrap().pending_jobs, 0);
        assert!(realm.state.borrow().timers.is_empty());
        assert_eq!(errors.len(), 2, "original diagnostic retained: {errors:?}");
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
