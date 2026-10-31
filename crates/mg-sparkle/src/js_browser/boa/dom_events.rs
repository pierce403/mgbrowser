//! Modern DOM Events: options objects (`{ capture, once, passive, signal }`),
//! `Event`, `CustomEvent`, `MouseEvent`, `KeyboardEvent`, `FocusEvent`,
//! `PopStateEvent`, `HashChangeEvent`, `target.dispatchEvent(event)`,
//! `el.click()`, `el.focus()`, `el.blur()`, `document.activeElement`,
//! and rich interactive control activation.
use super::{
    BoaPageRealm, Brand, Listener, State, Target, dom_exception, dom_ops, event_target, failure,
    from_host, modern, receiver, state, utf8, wrap,
};
use crate::document::Node;
use mg_butane::runtime::Value;
use modern::boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::{FunctionObjectBuilder, JsObject, ObjectInitializer, builtins::JsProxy},
    property::{Attribute, PropertyDescriptor},
};
use modern::boa_gc::{Finalize, Trace};

pub(super) const MAX_BOA_LISTENERS: usize = 32;
pub(super) const MAX_MODERN_EVENTS: usize = 512;

pub(super) const DISPATCH_EVENT: &str = "host.m2.dispatchEvent";
pub(super) const ELEMENT_CLICK: &str = "host.m2.click";
pub(super) const ELEMENT_FOCUS: &str = "host.m2.focus";
pub(super) const ELEMENT_BLUR: &str = "host.m2.blur";
pub(super) const ELEMENT_FORM_SUBMIT: &str = "host.m2.form.submit";
pub(super) const ELEMENT_FORM_RESET: &str = "host.m2.form.reset";
pub(super) const ELEMENT_FORM_REQUEST_SUBMIT: &str = "host.m2.form.requestSubmit";
pub(super) const MEVENT_PREVENT_DEFAULT: &str = "host.m2.mevent.preventDefault";
pub(super) const MEVENT_STOP_PROPAGATION: &str = "host.m2.mevent.stopPropagation";
pub(super) const MEVENT_STOP_IMMEDIATE: &str = "host.m2.mevent.stopImmediatePropagation";
pub(super) const MEVENT_COMPOSED_PATH: &str = "host.m2.mevent.composedPath";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EventFlavor {
    Event,
    CustomEvent,
    UIEvent,
    MouseEvent,
    KeyboardEvent,
    FocusEvent,
    PopStateEvent,
    HashChangeEvent,
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct EventPrototypes {
    pub event: JsObject,
    pub custom_event: JsObject,
    pub ui_event: JsObject,
    pub mouse_event: JsObject,
    pub keyboard_event: JsObject,
    pub focus_event: JsObject,
    pub popstate_event: JsObject,
    pub hashchange_event: JsObject,
}

#[derive(Clone, Trace, Finalize)]
pub(super) struct ModernEvent {
    #[unsafe_ignore_trace]
    pub flavor: EventFlavor,
    #[unsafe_ignore_trace]
    pub event_type: String,
    #[unsafe_ignore_trace]
    pub bubbles: bool,
    #[unsafe_ignore_trace]
    pub cancelable: bool,
    #[unsafe_ignore_trace]
    pub composed: bool,
    #[unsafe_ignore_trace]
    pub canceled: bool,
    #[unsafe_ignore_trace]
    pub stopped: bool,
    #[unsafe_ignore_trace]
    pub immediate: bool,
    #[unsafe_ignore_trace]
    pub dispatching: bool,
    #[unsafe_ignore_trace]
    pub is_trusted: bool,
    #[unsafe_ignore_trace]
    pub phase: u8,
    #[unsafe_ignore_trace]
    pub target: Option<Target>,
    #[unsafe_ignore_trace]
    pub current: Option<Target>,
    #[unsafe_ignore_trace]
    pub related_target: Option<Target>,
    #[unsafe_ignore_trace]
    pub submitter: Option<usize>,
    #[unsafe_ignore_trace]
    pub time_stamp: f64,
    pub detail: JsValue,
    pub state_val: JsValue,
    #[unsafe_ignore_trace]
    pub old_url: String,
    #[unsafe_ignore_trace]
    pub new_url: String,
    #[unsafe_ignore_trace]
    pub key: String,
    #[unsafe_ignore_trace]
    pub code: String,
    #[unsafe_ignore_trace]
    pub ctrl_key: bool,
    #[unsafe_ignore_trace]
    pub shift_key: bool,
    #[unsafe_ignore_trace]
    pub alt_key: bool,
    #[unsafe_ignore_trace]
    pub meta_key: bool,
    #[unsafe_ignore_trace]
    pub repeat: bool,
    #[unsafe_ignore_trace]
    pub button: i16,
    #[unsafe_ignore_trace]
    pub buttons: u16,
    #[unsafe_ignore_trace]
    pub client_x: f64,
    #[unsafe_ignore_trace]
    pub client_y: f64,
    #[unsafe_ignore_trace]
    pub page_x: f64,
    #[unsafe_ignore_trace]
    pub page_y: f64,
}

impl ModernEvent {
    pub(super) fn simple(
        event_type: &str,
        bubbles: bool,
        cancelable: bool,
        is_trusted: bool,
        time_stamp: f64,
    ) -> Self {
        Self {
            flavor: EventFlavor::Event,
            event_type: event_type.to_owned(),
            bubbles,
            cancelable,
            composed: false,
            canceled: false,
            stopped: false,
            immediate: false,
            dispatching: false,
            is_trusted,
            phase: 0,
            target: None,
            current: None,
            related_target: None,
            submitter: None,
            time_stamp,
            detail: JsValue::null(),
            state_val: JsValue::null(),
            old_url: String::new(),
            new_url: String::new(),
            key: String::new(),
            code: String::new(),
            ctrl_key: false,
            shift_key: false,
            alt_key: false,
            meta_key: false,
            repeat: false,
            button: 0,
            buttons: 0,
            client_x: 0.0,
            client_y: 0.0,
            page_x: 0.0,
            page_y: 0.0,
        }
    }
}

pub(super) fn is_event_handler_property(name: &str) -> Option<&'static str> {
    match name {
        "onclick" => Some("click"),
        "onsubmit" => Some("submit"),
        "onload" => Some("load"),
        "onreadystatechange" => Some("readystatechange"),
        "onerror" => Some("error"),
        "onunhandledrejection" => Some("unhandledrejection"),
        "onrejectionhandled" => Some("rejectionhandled"),
        "onmessage" => Some("message"),
        "onmessageerror" => Some("messageerror"),
        "oninput" => Some("input"),
        "onchange" => Some("change"),
        "ontoggle" => Some("toggle"),
        "onclose" => Some("close"),
        "onreset" => Some("reset"),
        "onfocus" => Some("focus"),
        "onblur" => Some("blur"),
        "onkeydown" => Some("keydown"),
        "onkeyup" => Some("keyup"),
        "onkeypress" => Some("keypress"),
        "onmousedown" => Some("mousedown"),
        "onmouseup" => Some("mouseup"),
        "onmousemove" => Some("mousemove"),
        "onpopstate" => Some("popstate"),
        "onhashchange" => Some("hashchange"),
        _ => None,
    }
}

pub(super) const WINDOW_HANDLER_PROPERTIES: &[&str] = &[
    "onclick",
    "onsubmit",
    "onload",
    "onreadystatechange",
    "onerror",
    "onunhandledrejection",
    "onrejectionhandled",
    "onmessage",
    "onmessageerror",
    "oninput",
    "onchange",
    "ontoggle",
    "onclose",
    "onreset",
    "onfocus",
    "onblur",
    "onkeydown",
    "onkeyup",
    "onkeypress",
    "onmousedown",
    "onmouseup",
    "onmousemove",
    "onpopstate",
    "onhashchange",
];

pub(super) fn method_metadata(name: &str) -> Option<(&'static str, usize)> {
    match name {
        DISPATCH_EVENT | "host.window.dispatchEvent" => Some(("dispatchEvent", 1)),
        ELEMENT_CLICK => Some(("click", 0)),
        ELEMENT_FOCUS => Some(("focus", 0)),
        ELEMENT_BLUR => Some(("blur", 0)),
        ELEMENT_FORM_SUBMIT => Some(("submit", 0)),
        ELEMENT_FORM_RESET => Some(("reset", 0)),
        ELEMENT_FORM_REQUEST_SUBMIT => Some(("requestSubmit", 0)),
        MEVENT_PREVENT_DEFAULT => Some(("preventDefault", 0)),
        MEVENT_STOP_PROPAGATION => Some(("stopPropagation", 0)),
        MEVENT_STOP_IMMEDIATE => Some(("stopImmediatePropagation", 0)),
        MEVENT_COMPOSED_PATH => Some(("composedPath", 0)),
        _ => None,
    }
}

pub(super) fn register(context: &mut Context) -> Result<(), String> {
    let event_proto = ObjectInitializer::new(context)
        .property(js_string!("NONE"), 0, Attribute::READONLY)
        .property(js_string!("CAPTURING_PHASE"), 1, Attribute::READONLY)
        .property(js_string!("AT_TARGET"), 2, Attribute::READONLY)
        .property(js_string!("BUBBLING_PHASE"), 3, Attribute::READONLY)
        .build();
    let custom_proto = JsObject::from_proto_and_data(Some(event_proto.clone()), ());
    let ui_proto = JsObject::from_proto_and_data(Some(event_proto.clone()), ());
    let mouse_proto = JsObject::from_proto_and_data(Some(ui_proto.clone()), ());
    let keyboard_proto = JsObject::from_proto_and_data(Some(ui_proto.clone()), ());
    let focus_proto = JsObject::from_proto_and_data(Some(ui_proto.clone()), ());
    let popstate_proto = JsObject::from_proto_and_data(Some(event_proto.clone()), ());
    let hashchange_proto = JsObject::from_proto_and_data(Some(event_proto.clone()), ());

    let protos = EventPrototypes {
        event: event_proto.clone(),
        custom_event: custom_proto.clone(),
        ui_event: ui_proto.clone(),
        mouse_event: mouse_proto.clone(),
        keyboard_event: keyboard_proto.clone(),
        focus_event: focus_proto.clone(),
        popstate_event: popstate_proto.clone(),
        hashchange_event: hashchange_proto.clone(),
    };
    state(context).borrow_mut().event_prototypes = Some(protos);

    install_event_ctor("Event", EventFlavor::Event, event_proto, true, context)?;
    install_event_ctor(
        "CustomEvent",
        EventFlavor::CustomEvent,
        custom_proto,
        false,
        context,
    )?;
    install_event_ctor("UIEvent", EventFlavor::UIEvent, ui_proto, false, context)?;
    install_event_ctor(
        "MouseEvent",
        EventFlavor::MouseEvent,
        mouse_proto,
        false,
        context,
    )?;
    install_event_ctor(
        "KeyboardEvent",
        EventFlavor::KeyboardEvent,
        keyboard_proto,
        false,
        context,
    )?;
    install_event_ctor(
        "FocusEvent",
        EventFlavor::FocusEvent,
        focus_proto,
        false,
        context,
    )?;
    install_event_ctor(
        "PopStateEvent",
        EventFlavor::PopStateEvent,
        popstate_proto,
        false,
        context,
    )?;
    install_event_ctor(
        "HashChangeEvent",
        EventFlavor::HashChangeEvent,
        hashchange_proto,
        false,
        context,
    )?;

    let dispatch_fn = super::method("host.window.dispatchEvent", context)
        .map_err(|_| "Window dispatchEvent binding failed")?;
    context
        .register_global_property(js_string!("dispatchEvent"), dispatch_fn, Attribute::all())
        .map_err(|_| "Global dispatchEvent failed")?;
    Ok(())
}

fn install_event_ctor(
    name: &'static str,
    flavor: EventFlavor,
    proto: JsObject,
    add_phase_constants: bool,
    context: &mut Context,
) -> Result<(), String> {
    let ctor = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_copy_closure(move |new_target, args, context| {
            construct_event(flavor, new_target, args, context)
        }),
    )
    .name(JsString::from(name))
    .length(1)
    .constructor(true)
    .build();

    ctor.define_property_or_throw(
        js_string!("prototype"),
        PropertyDescriptor::builder()
            .value(proto.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )
    .map_err(|_| "Event prototype bind failed")?;
    proto
        .define_property_or_throw(
            js_string!("constructor"),
            PropertyDescriptor::builder()
                .value(ctor.clone())
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .map_err(|_| "Event constructor backlink failed")?;

    if add_phase_constants {
        for (const_name, val) in [
            ("NONE", 0),
            ("CAPTURING_PHASE", 1),
            ("AT_TARGET", 2),
            ("BUBBLING_PHASE", 3),
        ] {
            ctor.define_property_or_throw(
                JsString::from(const_name),
                PropertyDescriptor::builder()
                    .value(val)
                    .writable(false)
                    .enumerable(true)
                    .configurable(false),
                context,
            )
            .map_err(|_| "Event phase constant failed")?;
        }
    }

    context
        .global_object()
        .define_property_or_throw(
            JsString::from(name),
            PropertyDescriptor::builder()
                .value(ctor)
                .writable(true)
                .enumerable(false)
                .configurable(true),
            context,
        )
        .map_err(|_| "Global Event constructor failed")?;
    Ok(())
}

fn construct_event(
    flavor: EventFlavor,
    new_target: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    if new_target.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("Failed to construct 'Event': Please use the 'new' operator")
            .into());
    }
    let Some(first) = args.first() else {
        return Err(JsNativeError::typ()
            .with_message("Failed to construct 'Event': 1 argument required, but only 0 present.")
            .into());
    };
    let event_type = utf8(first, context)?;
    let init = args.get(1).cloned().unwrap_or_default();

    let mut bubbles = false;
    let mut cancelable = false;
    let mut composed = false;
    let mut detail = match flavor {
        EventFlavor::UIEvent
        | EventFlavor::MouseEvent
        | EventFlavor::KeyboardEvent
        | EventFlavor::FocusEvent => JsValue::from(0),
        _ => JsValue::null(),
    };
    let mut state_val = JsValue::null();
    let mut old_url = String::new();
    let mut new_url = String::new();
    let mut key = String::new();
    let mut code = String::new();
    let mut ctrl_key = false;
    let mut shift_key = false;
    let mut alt_key = false;
    let mut meta_key = false;
    let mut repeat = false;
    let mut button: i16 = 0;
    let mut buttons: u16 = 0;
    let mut client_x = 0.0;
    let mut client_y = 0.0;
    let mut page_x = 0.0;
    let mut page_y = 0.0;
    let mut related_target = None;

    if !init.is_undefined() && !init.is_null() {
        let Some(dict) = init.as_object() else {
            return Err(JsNativeError::typ()
                .with_message("EventInit dictionary must be an object")
                .into());
        };
        let b = dict.get(js_string!("bubbles"), context)?;
        if !b.is_undefined() {
            bubbles = b.to_boolean();
        }
        let c = dict.get(js_string!("cancelable"), context)?;
        if !c.is_undefined() {
            cancelable = c.to_boolean();
        }
        let comp = dict.get(js_string!("composed"), context)?;
        if !comp.is_undefined() {
            composed = comp.to_boolean();
        }
        match flavor {
            EventFlavor::CustomEvent => {
                let d = dict.get(js_string!("detail"), context)?;
                if !d.is_undefined() {
                    detail = d;
                }
            }
            EventFlavor::UIEvent
            | EventFlavor::MouseEvent
            | EventFlavor::KeyboardEvent
            | EventFlavor::FocusEvent => {
                let d = dict.get(js_string!("detail"), context)?;
                if !d.is_undefined() {
                    detail = d.to_i32(context)?.into();
                }
            }
            _ => {}
        }
        if matches!(flavor, EventFlavor::MouseEvent | EventFlavor::KeyboardEvent) {
            let v = dict.get(js_string!("ctrlKey"), context)?;
            if !v.is_undefined() {
                ctrl_key = v.to_boolean();
            }
            let v = dict.get(js_string!("shiftKey"), context)?;
            if !v.is_undefined() {
                shift_key = v.to_boolean();
            }
            let v = dict.get(js_string!("altKey"), context)?;
            if !v.is_undefined() {
                alt_key = v.to_boolean();
            }
            let v = dict.get(js_string!("metaKey"), context)?;
            if !v.is_undefined() {
                meta_key = v.to_boolean();
            }
        }
        if flavor == EventFlavor::MouseEvent {
            let v = dict.get(js_string!("button"), context)?;
            if !v.is_undefined() {
                button = v.to_i32(context)? as i16;
            }
            let v = dict.get(js_string!("buttons"), context)?;
            if !v.is_undefined() {
                buttons = v.to_u32(context)? as u16;
            }
            let v = dict.get(js_string!("clientX"), context)?;
            if !v.is_undefined() {
                client_x = v.to_number(context)?;
                if !client_x.is_finite() {
                    client_x = 0.0;
                }
            }
            let v = dict.get(js_string!("clientY"), context)?;
            if !v.is_undefined() {
                client_y = v.to_number(context)?;
                if !client_y.is_finite() {
                    client_y = 0.0;
                }
            }
            page_x = client_x;
            page_y = client_y;
        }
        if flavor == EventFlavor::KeyboardEvent {
            let v = dict.get(js_string!("key"), context)?;
            if !v.is_undefined() {
                key = utf8(&v, context)?;
            }
            let v = dict.get(js_string!("code"), context)?;
            if !v.is_undefined() {
                code = utf8(&v, context)?;
            }
            let v = dict.get(js_string!("repeat"), context)?;
            if !v.is_undefined() {
                repeat = v.to_boolean();
            }
        }
        if matches!(flavor, EventFlavor::MouseEvent | EventFlavor::FocusEvent) {
            let v = dict.get(js_string!("relatedTarget"), context)?;
            if !v.is_undefined() && !v.is_null() {
                related_target = Some(event_target(&v, context)?);
            }
        }
        if flavor == EventFlavor::PopStateEvent {
            let v = dict.get(js_string!("state"), context)?;
            if !v.is_undefined() {
                state_val = v;
            }
        }
        if flavor == EventFlavor::HashChangeEvent {
            let v = dict.get(js_string!("oldURL"), context)?;
            if !v.is_undefined() {
                old_url = utf8(&v, context)?;
            }
            let v = dict.get(js_string!("newURL"), context)?;
            if !v.is_undefined() {
                new_url = utf8(&v, context)?;
            }
        }
    }

    let time_stamp = elapsed_ms(&state(context).borrow());
    let record = ModernEvent {
        flavor,
        event_type,
        bubbles,
        cancelable,
        composed,
        canceled: false,
        stopped: false,
        immediate: false,
        dispatching: false,
        is_trusted: false,
        phase: 0,
        target: None,
        current: None,
        related_target,
        submitter: None,
        time_stamp,
        detail,
        state_val,
        old_url,
        new_url,
        key,
        code,
        ctrl_key,
        shift_key,
        alt_key,
        meta_key,
        repeat,
        button,
        buttons,
        client_x,
        client_y,
        page_x,
        page_y,
    };
    let id = alloc_modern_event(record, context)?;
    wrap_modern_event(id, flavor, context)
}

pub(super) fn elapsed_ms(data: &State) -> f64 {
    data.now()
        .saturating_duration_since(data.origin_instant)
        .as_secs_f64()
        * 1000.0
}

pub(super) fn alloc_modern_event(record: ModernEvent, context: &mut Context) -> JsResult<usize> {
    modern::check_context(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if data.modern_events.len() >= MAX_MODERN_EVENTS {
        return Err(failure("Modern event limit exceeded"));
    }
    let bytes = std::mem::size_of::<ModernEvent>()
        + record.event_type.len()
        + record.old_url.len()
        + record.new_url.len()
        + record.key.len()
        + record.code.len();
    data.host.charge(bytes).map_err(failure)?;
    let id = data.modern_events.len();
    data.modern_events.push(record);
    Ok(id)
}

pub(super) fn proto_for_flavor(data: &State, flavor: EventFlavor) -> Option<JsObject> {
    let protos = data.event_prototypes.as_ref()?;
    Some(match flavor {
        EventFlavor::Event => protos.event.clone(),
        EventFlavor::CustomEvent => protos.custom_event.clone(),
        EventFlavor::UIEvent => protos.ui_event.clone(),
        EventFlavor::MouseEvent => protos.mouse_event.clone(),
        EventFlavor::KeyboardEvent => protos.keyboard_event.clone(),
        EventFlavor::FocusEvent => protos.focus_event.clone(),
        EventFlavor::PopStateEvent => protos.popstate_event.clone(),
        EventFlavor::HashChangeEvent => protos.hashchange_event.clone(),
    })
}

pub(super) fn wrap_modern_event(
    id: usize,
    flavor: EventFlavor,
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let handle = format!("mevent:{id}");
    let shared = state(context);
    let (old, deref) = {
        let data = shared.borrow();
        (data.wrappers.get(&handle).cloned(), data.weak_deref.clone())
    };
    if let Some(old) = old {
        let object = deref.call(&old.into(), &[], context)?;
        if object.is_object() {
            return Ok(object);
        }
    }
    let (realm, finalized, constructor, brands, proto) = {
        let mut data = shared.borrow_mut();
        data.host.charge(256 + handle.len()).map_err(failure)?;
        data.wrappers_created += 1;
        let proto = proto_for_flavor(&data, flavor)
            .unwrap_or_else(|| context.intrinsics().constructors().object().prototype());
        (
            data.realm,
            data.finalized.clone(),
            data.weak_constructor.clone(),
            data.brands.clone(),
            proto,
        )
    };
    let target = JsObject::from_proto_and_data(
        Some(proto),
        Brand {
            handle: handle.clone(),
            realm,
            error: None,
            rejection: None,
            message: None,
            finalized,
        },
    );
    let object: JsObject = JsProxy::builder(target)
        .get(super::get)
        .set(super::set)
        .has(super::has)
        .define_property(super::define_property)
        .delete_property(super::delete_property)
        .build(context)?
        .into();
    brands.set(&object, JsString::from(handle.as_str()).into(), context)?;
    let weak = constructor.construct(&[object.clone().into()], None, context)?;
    shared.borrow_mut().wrappers.insert(handle, weak);
    Ok(object.into())
}

// --- Listener Registration with Options Object (`{ capture, once, passive, signal }`) ---

fn parse_listener_options(
    arg: Option<&JsValue>,
    is_add: bool,
    context: &mut Context,
) -> JsResult<(bool, bool, bool, Option<usize>)> {
    let Some(val) = arg else {
        return Ok((false, false, false, None));
    };
    if val.is_undefined() || val.is_null() {
        return Ok((false, false, false, None));
    }
    if let Some(b) = val.as_boolean() {
        return Ok((b, false, false, None));
    }
    if let Some(obj) = val.as_object() {
        let cap_val = obj.get(js_string!("capture"), context)?;
        let capture = if cap_val.is_undefined() {
            false
        } else {
            cap_val.to_boolean()
        };
        if !is_add {
            return Ok((capture, false, false, None));
        }
        let once_val = obj.get(js_string!("once"), context)?;
        let once = !once_val.is_undefined() && once_val.to_boolean();
        let passive_val = obj.get(js_string!("passive"), context)?;
        let passive = !passive_val.is_undefined() && passive_val.to_boolean();
        let sig_val = obj.get(js_string!("signal"), context)?;
        let signal_id = if sig_val.is_undefined() || sig_val.is_null() {
            None
        } else {
            let sid = super::fetch_xhr::abort_signal_id(&sig_val, context)?;
            Some(sid)
        };
        return Ok((capture, once, passive, signal_id));
    }
    Ok((val.to_boolean(), false, false, None))
}

pub(super) fn listener_call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let target = match receiver(this, context)? {
        Value::Null | Value::Undefined | Value::Object(0) => Target::Window,
        Value::Host(handle) => {
            if let Some(sig_id) = handle
                .strip_prefix("abortsignal:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                return super::fetch_xhr::signal_listener_call(name, sig_id, args, context);
            }
            if let Some(xhr_id) = handle
                .strip_prefix("xhr:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                return super::fetch_xhr::xhr_listener_call(name, xhr_id, args, context);
            }
            let shared = state(context);
            let data = shared.borrow();
            dom_ops::node_id_from_handle(&handle, &data)
                .map(Target::Node)
                .map_err(failure)?
        }
        _ => return Err(failure("Unsupported EventTarget receiver")),
    };

    let event_type = utf8(args.first().unwrap_or(&JsValue::undefined()), context)?;
    let value = args.get(1).cloned().unwrap_or_default();
    let is_add = !name.ends_with("removeEventListener");
    let (capture, once, passive, signal_id) = parse_listener_options(args.get(2), is_add, context)?;

    if value.is_null() || value.is_undefined() {
        return Ok(JsValue::undefined());
    }
    let callback = if let Some(cb) = value.as_callable() {
        cb
    } else if let Some(obj) = value.as_object() {
        // Support EventListener `{ handleEvent(e) }` objects!
        obj
    } else {
        return Err(failure(
            "Listener must be a function or EventListener object",
        ));
    };

    modern::check_context(context)?;
    let shared = state(context);
    let mut data = shared.borrow_mut();

    if is_add
        && let Some(sid) = signal_id
        && data.abort_signals.get(sid).is_some_and(|sig| sig.aborted)
    {
        return Ok(JsValue::undefined());
    }

    let index = data.listeners.iter().position(|l| {
        !l.property
            && l.target == target
            && l.event_type == event_type
            && l.capture == capture
            && l.callback.as_ref() == Some(&callback)
    });
    if !is_add {
        if let Some(index) = index {
            data.listeners[index].callback = None;
        }
    } else if index.is_none() {
        if data.listeners.len() >= MAX_BOA_LISTENERS {
            return Err(failure("Listener limit exceeded"));
        }
        data.host.charge(128 + event_type.len()).map_err(failure)?;
        if let Target::Node(nid) = target
            && has_uncompiled_inline_handler(&data, nid, &event_type)
            && !data
                .listeners
                .iter()
                .any(|l| l.property && l.target == target && l.event_type == event_type)
            && data.listeners.len() + 1 < MAX_BOA_LISTENERS
        {
            data.listeners.push(Listener {
                target,
                event_type: event_type.clone(),
                callback: None,
                capture: false,
                once: false,
                passive: false,
                property: true,
            });
        }
        let slot = data.listeners.len();
        data.listeners.push(Listener {
            target,
            event_type,
            callback: Some(callback),
            capture,
            once,
            passive,
            property: false,
        });
        if let Some(sid) = signal_id
            && let Some(sig) = data.abort_signals.get_mut(sid)
        {
            sig.bound_listeners.push(slot);
        }
    }
    Ok(JsValue::undefined())
}

// --- Property Interception (`try_get`, `try_set`, `try_has`) ---

pub(super) fn try_get(
    handle: &str,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<JsValue>> {
    if let Some(id) = handle
        .strip_prefix("mevent:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        return mevent_get(id, name, context);
    }

    if handle == "document" {
        match name {
            "activeElement" => {
                let target_id = {
                    let shared = state(context);
                    let data = shared.borrow();
                    resolve_active_element(&data)
                };
                return match target_id {
                    Some(id) => wrap(&format!("node:{id}"), context).map(Some),
                    None => Ok(Some(JsValue::null())),
                };
            }
            "dispatchEvent" => {
                return super::method(DISPATCH_EVENT, context).map(|o| Some(o.into()));
            }
            _ => {}
        }
    }

    if let Some(sub_id) = handle
        .strip_prefix("subdoc:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        let _ = sub_id;
        if name == "dispatchEvent" {
            return super::method(DISPATCH_EVENT, context).map(|o| Some(o.into()));
        }
        if name == "activeElement" {
            return Ok(Some(JsValue::null()));
        }
    }

    if handle.starts_with("node:") {
        match name {
            "dispatchEvent" => {
                return super::method(DISPATCH_EVENT, context).map(|o| Some(o.into()));
            }
            "click" | "focus" | "blur" => {
                let is_el = {
                    let shared = state(context);
                    let data = shared.borrow();
                    let id = data.host.node(handle).map_err(failure)?;
                    !data.host.document.nodes[id].tag.starts_with('#')
                };
                if is_el {
                    let m = match name {
                        "click" => ELEMENT_CLICK,
                        "focus" => ELEMENT_FOCUS,
                        _ => ELEMENT_BLUR,
                    };
                    return super::method(m, context).map(|o| Some(o.into()));
                }
            }
            "submit" | "reset" | "requestSubmit" => {
                let is_form = {
                    let shared = state(context);
                    let data = shared.borrow();
                    let id = data.host.node(handle).map_err(failure)?;
                    data.host.document.nodes[id].tag == "form"
                };
                if is_form {
                    let m = match name {
                        "submit" => ELEMENT_FORM_SUBMIT,
                        "reset" => ELEMENT_FORM_RESET,
                        _ => ELEMENT_FORM_REQUEST_SUBMIT,
                    };
                    return super::method(m, context).map(|o| Some(o.into()));
                }
            }
            _ => {}
        }
    }

    Ok(None)
}

pub(super) fn event_extra_property(
    event_id: usize,
    name: &str,
    context: &mut Context,
) -> JsResult<Option<JsValue>> {
    let shared = state(context);
    let data = shared.borrow();
    let Some(ev) = data.host.events.get(event_id) else {
        return Ok(None);
    };
    let is_click = ev.kind == super::Kind::Click;
    let canceled = ev.canceled;
    let target = ev.target;
    let ts = elapsed_ms(&data);
    drop(data);
    match name {
        "NONE" => Ok(Some(0.into())),
        "CAPTURING_PHASE" => Ok(Some(1.into())),
        "AT_TARGET" => Ok(Some(2.into())),
        "BUBBLING_PHASE" => Ok(Some(3.into())),
        "isTrusted" => Ok(Some(true.into())),
        "composed" => Ok(Some(is_click.into())),
        "timeStamp" => Ok(Some(ts.into())),
        "returnValue" => Ok(Some((!canceled).into())),
        "srcElement" => from_host(target.value(), context).map(Some),
        "composedPath" => super::method(MEVENT_COMPOSED_PATH, context).map(|o| Some(o.into())),
        "button" | "buttons" | "clientX" | "clientY" | "pageX" | "pageY" if is_click => {
            Ok(Some(0.into()))
        }
        "detail" if is_click => Ok(Some(1.into())),
        "ctrlKey" | "shiftKey" | "altKey" | "metaKey" if is_click => Ok(Some(false.into())),
        "relatedTarget" if is_click => Ok(Some(JsValue::null())),
        _ => Ok(None),
    }
}

fn mevent_get(id: usize, name: &str, context: &mut Context) -> JsResult<Option<JsValue>> {
    let shared = state(context);
    let data = shared.borrow();
    let ev = data
        .modern_events
        .get(id)
        .ok_or_else(|| failure("Stale event handle"))?
        .clone();
    drop(data);

    match name {
        "NONE" => Ok(Some(0.into())),
        "CAPTURING_PHASE" => Ok(Some(1.into())),
        "AT_TARGET" => Ok(Some(2.into())),
        "BUBBLING_PHASE" => Ok(Some(3.into())),
        "type" => Ok(Some(JsString::from(ev.event_type.as_str()).into())),
        "bubbles" => Ok(Some(ev.bubbles.into())),
        "cancelable" => Ok(Some(ev.cancelable.into())),
        "composed" => Ok(Some(ev.composed.into())),
        "defaultPrevented" => Ok(Some(ev.canceled.into())),
        "returnValue" => Ok(Some((!ev.canceled).into())),
        "cancelBubble" => Ok(Some(ev.stopped.into())),
        "eventPhase" => Ok(Some((ev.phase as i32).into())),
        "isTrusted" => Ok(Some(ev.is_trusted.into())),
        "timeStamp" => Ok(Some(ev.time_stamp.into())),
        "target" | "srcElement" => match ev.target {
            Some(t) => from_host(t.value(), context).map(Some),
            None => Ok(Some(JsValue::null())),
        },
        "currentTarget" => match ev.current {
            Some(t) => from_host(t.value(), context).map(Some),
            None => Ok(Some(JsValue::null())),
        },
        "relatedTarget" => match ev.related_target {
            Some(t) => from_host(t.value(), context).map(Some),
            None => Ok(Some(JsValue::null())),
        },
        "submitter" => match ev.submitter {
            Some(nid) => wrap(&format!("node:{nid}"), context).map(Some),
            None => Ok(Some(JsValue::null())),
        },
        "detail"
            if matches!(
                ev.flavor,
                EventFlavor::CustomEvent
                    | EventFlavor::UIEvent
                    | EventFlavor::MouseEvent
                    | EventFlavor::KeyboardEvent
                    | EventFlavor::FocusEvent
            ) =>
        {
            Ok(Some(ev.detail.clone()))
        }
        "state" if ev.flavor == EventFlavor::PopStateEvent => Ok(Some(ev.state_val.clone())),
        "oldURL" if ev.flavor == EventFlavor::HashChangeEvent => {
            Ok(Some(JsString::from(ev.old_url.as_str()).into()))
        }
        "newURL" if ev.flavor == EventFlavor::HashChangeEvent => {
            Ok(Some(JsString::from(ev.new_url.as_str()).into()))
        }
        "key" if ev.flavor == EventFlavor::KeyboardEvent => {
            Ok(Some(JsString::from(ev.key.as_str()).into()))
        }
        "code" if ev.flavor == EventFlavor::KeyboardEvent => {
            Ok(Some(JsString::from(ev.code.as_str()).into()))
        }
        "repeat" if ev.flavor == EventFlavor::KeyboardEvent => Ok(Some(ev.repeat.into())),
        "ctrlKey"
            if matches!(
                ev.flavor,
                EventFlavor::MouseEvent | EventFlavor::KeyboardEvent
            ) =>
        {
            Ok(Some(ev.ctrl_key.into()))
        }
        "shiftKey"
            if matches!(
                ev.flavor,
                EventFlavor::MouseEvent | EventFlavor::KeyboardEvent
            ) =>
        {
            Ok(Some(ev.shift_key.into()))
        }
        "altKey"
            if matches!(
                ev.flavor,
                EventFlavor::MouseEvent | EventFlavor::KeyboardEvent
            ) =>
        {
            Ok(Some(ev.alt_key.into()))
        }
        "metaKey"
            if matches!(
                ev.flavor,
                EventFlavor::MouseEvent | EventFlavor::KeyboardEvent
            ) =>
        {
            Ok(Some(ev.meta_key.into()))
        }
        "button" if ev.flavor == EventFlavor::MouseEvent => Ok(Some((ev.button as i32).into())),
        "buttons" if ev.flavor == EventFlavor::MouseEvent => Ok(Some((ev.buttons as i32).into())),
        "clientX" if ev.flavor == EventFlavor::MouseEvent => Ok(Some(ev.client_x.into())),
        "clientY" if ev.flavor == EventFlavor::MouseEvent => Ok(Some(ev.client_y.into())),
        "pageX" if ev.flavor == EventFlavor::MouseEvent => Ok(Some(ev.page_x.into())),
        "pageY" if ev.flavor == EventFlavor::MouseEvent => Ok(Some(ev.page_y.into())),
        "preventDefault" => super::method(MEVENT_PREVENT_DEFAULT, context).map(|o| Some(o.into())),
        "stopPropagation" => {
            super::method(MEVENT_STOP_PROPAGATION, context).map(|o| Some(o.into()))
        }
        "stopImmediatePropagation" => {
            super::method(MEVENT_STOP_IMMEDIATE, context).map(|o| Some(o.into()))
        }
        "composedPath" => super::method(MEVENT_COMPOSED_PATH, context).map(|o| Some(o.into())),
        _ => Ok(None),
    }
}

pub(super) fn try_set(
    handle: &str,
    name: &str,
    value: &JsValue,
    context: &mut Context,
) -> JsResult<bool> {
    if let Some(id) = handle
        .strip_prefix("mevent:")
        .and_then(|s| s.parse::<usize>().ok())
    {
        match name {
            "cancelBubble" => {
                if value.to_boolean() {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    if let Some(ev) = data.modern_events.get_mut(id) {
                        ev.stopped = true;
                    }
                }
                return Ok(true);
            }
            "returnValue" => {
                if !value.to_boolean() {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    let passive = data.passive_listener_depth > 0;
                    if !passive
                        && let Some(ev) = data.modern_events.get_mut(id)
                        && ev.cancelable
                    {
                        ev.canceled = true;
                    }
                }
                return Ok(true);
            }
            _ => return Ok(false),
        }
    }
    Ok(false)
}

pub(super) fn try_has(handle: &str, name: &str, context: &mut Context) -> JsResult<Option<bool>> {
    if handle.starts_with("mevent:") {
        let found = try_get(handle, name, context)?.is_some();
        return Ok(if found { Some(true) } else { None });
    }
    if handle == "document" && matches!(name, "activeElement" | "dispatchEvent") {
        return Ok(Some(true));
    }
    if handle.starts_with("subdoc:") && matches!(name, "activeElement" | "dispatchEvent") {
        return Ok(Some(true));
    }
    if handle.starts_with("node:") {
        if name == "dispatchEvent" {
            return Ok(Some(true));
        }
        if matches!(name, "click" | "focus" | "blur") {
            let shared = state(context);
            let data = shared.borrow();
            let id = data.host.node(handle).map_err(failure)?;
            if !data.host.document.nodes[id].tag.starts_with('#') {
                return Ok(Some(true));
            }
        }
        if matches!(name, "submit" | "reset" | "requestSubmit") {
            let shared = state(context);
            let data = shared.borrow();
            let id = data.host.node(handle).map_err(failure)?;
            if data.host.document.nodes[id].tag == "form" {
                return Ok(Some(true));
            }
        }
    }
    Ok(None)
}

// --- Method Calls ---

pub(super) fn call(
    name: &str,
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    modern::check_context(context)?;
    match name {
        MEVENT_PREVENT_DEFAULT => {
            let id = mevent_receiver(this, context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let passive = data.passive_listener_depth > 0;
            if !passive
                && let Some(ev) = data.modern_events.get_mut(id)
                && ev.cancelable
            {
                ev.canceled = true;
            }
            Ok(JsValue::undefined())
        }
        MEVENT_STOP_PROPAGATION => {
            let id = mevent_receiver(this, context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if let Some(ev) = data.modern_events.get_mut(id) {
                ev.stopped = true;
            }
            Ok(JsValue::undefined())
        }
        MEVENT_STOP_IMMEDIATE => {
            let id = mevent_receiver(this, context)?;
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if let Some(ev) = data.modern_events.get_mut(id) {
                ev.stopped = true;
                ev.immediate = true;
            }
            Ok(JsValue::undefined())
        }
        MEVENT_COMPOSED_PATH => {
            let arr = modern::boa_engine::object::builtins::JsArray::new(context)?;
            Ok(arr.into())
        }
        DISPATCH_EVENT | "host.window.dispatchEvent" => {
            let target = match receiver(this, context)? {
                Value::Null | Value::Undefined | Value::Object(0) => {
                    if name == DISPATCH_EVENT {
                        return Err(failure("Invalid EventTarget receiver"));
                    }
                    Target::Window
                }
                Value::Host(handle) => {
                    if let Some(sig_id) = handle
                        .strip_prefix("abortsignal:")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        let ev_arg = args.first().cloned().unwrap_or_default();
                        let mevent_id = mevent_receiver(&ev_arg, context)?;
                        return super::fetch_xhr::signal_dispatch_event(sig_id, mevent_id, context);
                    }
                    if let Some(xhr_id) = handle
                        .strip_prefix("xhr:")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        let ev_arg = args.first().cloned().unwrap_or_default();
                        let mevent_id = mevent_receiver(&ev_arg, context)?;
                        return super::fetch_xhr::xhr_dispatch_event(xhr_id, mevent_id, context);
                    }
                    let shared = state(context);
                    let data = shared.borrow();
                    dom_ops::node_id_from_handle(&handle, &data)
                        .map(Target::Node)
                        .map_err(failure)?
                }
                _ => return Err(failure("Invalid EventTarget receiver")),
            };
            let Some(ev_arg) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("Failed to execute 'dispatchEvent': 1 argument required")
                    .into());
            };
            let mevent_id = mevent_receiver(ev_arg, context)?;
            let not_canceled = dispatch_modern_event(target, mevent_id, context)?;
            Ok(not_canceled.into())
        }
        ELEMENT_CLICK => {
            let id = element_receiver(this, context)?;
            js_element_click(id, context)?;
            Ok(JsValue::undefined())
        }
        ELEMENT_FOCUS => {
            let id = element_receiver(this, context)?;
            focus_element(id, context)?;
            Ok(JsValue::undefined())
        }
        ELEMENT_BLUR => {
            let id = element_receiver(this, context)?;
            blur_element(id, context)?;
            Ok(JsValue::undefined())
        }
        ELEMENT_FORM_SUBMIT => {
            let id = element_receiver(this, context)?;
            let is_form = state(context).borrow().host.document.nodes[id].tag == "form";
            if !is_form {
                return Err(failure("Invalid HTMLFormElement receiver"));
            }
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if data.host.navigation.is_none() && data.host.connected_path(id).is_ok() {
                submit_form_via_navigation(&mut data, id, None);
            }
            Ok(JsValue::undefined())
        }
        ELEMENT_FORM_RESET => {
            let id = element_receiver(this, context)?;
            let is_form = state(context).borrow().host.document.nodes[id].tag == "form";
            if !is_form {
                return Err(failure("Invalid HTMLFormElement receiver"));
            }
            let proceed = queue_or_fire_simple_event(id, "reset", true, true, context)?;
            if proceed {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                reset_form_controls(&mut data, id).map_err(failure)?;
            }
            Ok(JsValue::undefined())
        }
        ELEMENT_FORM_REQUEST_SUBMIT => {
            let id = element_receiver(this, context)?;
            let is_form = state(context).borrow().host.document.nodes[id].tag == "form";
            if !is_form {
                return Err(failure("Invalid HTMLFormElement receiver"));
            }
            let submitter = if let Some(arg) = args.first()
                && !arg.is_undefined()
                && !arg.is_null()
            {
                let sub_id = element_receiver(arg, context)?;
                let valid = {
                    let shared = state(context);
                    let data = shared.borrow();
                    data.host.is_submitter(sub_id) && data.host.form_owner(sub_id) == Some(id)
                };
                if !valid {
                    return Err(dom_exception::throw(
                        "NotFoundError",
                        "The specified element is not a submit button owned by this form.",
                        context,
                    ));
                }
                Some(sub_id)
            } else {
                None
            };
            let ts = elapsed_ms(&state(context).borrow());
            let mut sev = ModernEvent::simple("submit", true, true, false, ts);
            sev.submitter = submitter;
            let sid = alloc_modern_event(sev, context)?;
            let submit_ok = dispatch_modern_event(Target::Node(id), sid, context)?;
            if submit_ok {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                if data.host.navigation.is_none() && data.host.connected_path(id).is_ok() {
                    submit_form_via_navigation(&mut data, id, submitter);
                }
            }
            Ok(JsValue::undefined())
        }
        _ => Err(failure("Unknown DOM event method")),
    }
}

fn mevent_receiver(val: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(val, context)? else {
        return Err(JsNativeError::typ()
            .with_message("Argument 1 is not an Event")
            .into());
    };
    if handle.starts_with("event:") {
        return Err(dom_exception::throw(
            "InvalidStateError",
            "The event is already being dispatched or cannot be re-dispatched.",
            context,
        ));
    }
    let id = handle
        .strip_prefix("mevent:")
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| -> super::JsError {
            JsNativeError::typ()
                .with_message("Argument 1 is not an Event")
                .into()
        })?;
    if id >= state(context).borrow().modern_events.len() {
        return Err(failure("Stale event handle"));
    }
    Ok(id)
}

fn element_receiver(val: &JsValue, context: &mut Context) -> JsResult<usize> {
    let Value::Host(handle) = receiver(val, context)? else {
        return Err(failure("HTMLElement method requires an element receiver"));
    };
    let shared = state(context);
    let data = shared.borrow();
    let id = data.host.node(&handle).map_err(failure)?;
    if data.host.document.nodes[id].tag.starts_with('#') {
        return Err(failure("HTMLElement method requires an element receiver"));
    }
    Ok(id)
}

// --- Synchronous Modern Event Dispatch ---

fn build_propagation_path(target: Target, nodes: &[Node]) -> Vec<Target> {
    match target {
        Target::Window => vec![Target::Window],
        Target::Node(start) => {
            let mut path = Vec::with_capacity(16);
            let mut cur = start;
            for _ in 0..=crate::document::MAX_DEPTH {
                path.push(Target::Node(cur));
                if cur == 0 {
                    path.push(Target::Window);
                    break;
                }
                let p = nodes[cur].parent;
                if p == cur {
                    // Detached / DocumentFragment subtree: does NOT reach Window
                    break;
                }
                cur = p;
            }
            path
        }
    }
}

pub(super) fn invoke_callback_object(
    callback: &JsObject,
    this_val: &JsValue,
    event_wrapper: &JsValue,
    property: bool,
    passive: bool,
    context: &mut Context,
) -> JsResult<bool> {
    modern::check_context(context)?;
    if passive {
        state(context).borrow_mut().passive_listener_depth += 1;
    }
    let call_res = if callback.is_callable() {
        callback.call(this_val, std::slice::from_ref(event_wrapper), context)
    } else {
        let handle_fn = callback.get(js_string!("handleEvent"), context);
        match handle_fn {
            Ok(v) => {
                if let Some(callable) = v.as_callable() {
                    callable.call(
                        &JsValue::from(callback.clone()),
                        std::slice::from_ref(event_wrapper),
                        context,
                    )
                } else {
                    Ok(JsValue::undefined())
                }
            }
            Err(e) => Err(e),
        }
    };
    if passive {
        state(context).borrow_mut().passive_listener_depth -= 1;
    }
    match call_res {
        Ok(ret) => Ok(property && ret.as_boolean() == Some(false)),
        Err(err) => {
            // Propagate fatal VM errors (fuel/memory/recursion) immediately!
            modern::check_context(context)?;
            let msg = super::bounded(&format!("Event handler: {err}"), 512);
            let shared = state(context);
            let mut data = shared.borrow_mut();
            if data.async_errors.len() < 64 {
                data.async_errors.push(msg);
            }
            Ok(false)
        }
    }
}

pub(super) fn dispatch_modern_event(
    target: Target,
    mevent_id: usize,
    context: &mut Context,
) -> JsResult<bool> {
    modern::check_context(context)?;
    let (flavor, event_type, bubbles, cancelable, path) = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let ev = data
            .modern_events
            .get_mut(mevent_id)
            .ok_or_else(|| failure("Stale event handle"))?;
        if ev.dispatching {
            drop(data);
            return Err(dom_exception::throw(
                "InvalidStateError",
                "The event is already being dispatched.",
                context,
            ));
        }
        ev.dispatching = true;
        ev.stopped = false;
        ev.immediate = false;
        ev.target = Some(target);
        ev.current = None;
        ev.phase = 0;
        let flavor = ev.flavor;
        let event_type = ev.event_type.clone();
        let bubbles = ev.bubbles;
        let cancelable = ev.cancelable;
        let path = build_propagation_path(target, &data.host.document.nodes);
        (flavor, event_type, bubbles, cancelable, path)
    };

    let wrapper = match wrap_modern_event(mevent_id, flavor, context) {
        Ok(w) => w,
        Err(e) => {
            state(context).borrow_mut().modern_events[mevent_id].dispatching = false;
            return Err(e);
        }
    };

    let run_result = (|| -> JsResult<()> {
        // 1. Capturing phase
        for owner in path[1..].iter().rev() {
            if state(context).borrow().modern_events[mevent_id].stopped {
                break;
            }
            dispatch_modern_at(
                mevent_id,
                *owner,
                &event_type,
                true,
                1,
                cancelable,
                &wrapper,
                context,
            )?;
        }
        // 2. At-target phase
        if !state(context).borrow().modern_events[mevent_id].stopped {
            dispatch_modern_at(
                mevent_id,
                path[0],
                &event_type,
                true,
                2,
                cancelable,
                &wrapper,
                context,
            )?;
            if !state(context).borrow().modern_events[mevent_id].immediate {
                dispatch_modern_at(
                    mevent_id,
                    path[0],
                    &event_type,
                    false,
                    2,
                    cancelable,
                    &wrapper,
                    context,
                )?;
            }
        }
        // 3. Bubbling phase
        if bubbles {
            for owner in &path[1..] {
                if state(context).borrow().modern_events[mevent_id].stopped {
                    break;
                }
                dispatch_modern_at(
                    mevent_id,
                    *owner,
                    &event_type,
                    false,
                    3,
                    cancelable,
                    &wrapper,
                    context,
                )?;
            }
        }
        Ok(())
    })();

    let canceled = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        let ev = &mut data.modern_events[mevent_id];
        ev.current = None;
        ev.phase = 0;
        ev.dispatching = false;
        ev.stopped = false;
        ev.immediate = false;
        ev.canceled
    };

    run_result?;
    Ok(!canceled)
}

pub(super) fn has_uncompiled_inline_handler(
    data: &State,
    node_id: usize,
    event_type: &str,
) -> bool {
    if node_id == 0 || node_id >= data.host.document.nodes.len() {
        return false;
    }
    let node = &data.host.document.nodes[node_id];
    if node.tag.starts_with('#') || super::inert(&data.host.document.nodes, node_id) {
        return false;
    }
    let attr_name = format!("on{event_type}");
    if is_event_handler_property(&attr_name) != Some(event_type) {
        return false;
    }
    if data
        .suppressed_inline_handlers
        .contains(&(node_id, event_type.to_owned()))
    {
        return false;
    }
    node.attr(&attr_name).is_some()
}

pub(super) fn invalidate_inline_handler(data: &mut State, node_id: usize, attr_name: &str) {
    let lower = attr_name.to_ascii_lowercase();
    let Some(event_type) = is_event_handler_property(&lower) else {
        return;
    };
    data.suppressed_inline_handlers
        .remove(&(node_id, event_type.to_owned()));
    for l in &mut data.listeners {
        if l.property && l.target == Target::Node(node_id) && l.event_type == event_type {
            l.callback = None;
        }
    }
}

pub(super) fn ensure_inline_handler_compiled(
    target: Target,
    event_type: &str,
    context: &mut Context,
) -> JsResult<()> {
    let Target::Node(node_id) = target else {
        return Ok(());
    };
    let attr_name = format!("on{event_type}");
    if is_event_handler_property(&attr_name) != Some(event_type) {
        return Ok(());
    }
    let raw_body = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if node_id == 0
            || node_id >= data.host.document.nodes.len()
            || data.host.document.nodes[node_id].tag.starts_with('#')
            || super::inert(&data.host.document.nodes, node_id)
        {
            return Ok(());
        }
        if data
            .suppressed_inline_handlers
            .contains(&(node_id, event_type.to_owned()))
        {
            return Ok(());
        }
        if data.listeners.iter().any(|l| {
            l.property && l.target == target && l.event_type == event_type && l.callback.is_some()
        }) {
            return Ok(());
        }
        let Some(body) = data.host.document.nodes[node_id]
            .attr(&attr_name)
            .map(str::to_owned)
        else {
            return Ok(());
        };
        data.suppressed_inline_handlers
            .insert((node_id, event_type.to_owned()));
        if body.len() > 65_536 || data.listeners.len() >= MAX_BOA_LISTENERS {
            return Ok(());
        }
        data.host
            .charge(body.len().saturating_add(128))
            .map_err(failure)?;
        body
    };
    modern::check_context(context)?;
    let wrapped = format!("(function(event) {{\n{raw_body}\n}})");
    let parsed = modern::boa_engine::Script::parse(
        modern::boa_engine::Source::from_bytes(wrapped.as_bytes()),
        None,
        context,
    );
    let Ok(script) = parsed else {
        modern::check_context(context)?;
        return Ok(());
    };
    let Ok(val) = script.evaluate(context) else {
        modern::check_context(context)?;
        return Ok(());
    };
    modern::check_context(context)?;
    let Some(fn_obj) = val.as_callable() else {
        return Ok(());
    };
    let shared = state(context);
    let mut data = shared.borrow_mut();
    if let Some(slot) = data.listeners.iter_mut().find(|l| {
        l.property && l.target == target && l.event_type == event_type && l.callback.is_none()
    }) {
        slot.callback = Some(fn_obj);
    } else if data.listeners.len() < MAX_BOA_LISTENERS {
        data.listeners.push(super::Listener {
            target,
            event_type: event_type.to_owned(),
            callback: Some(fn_obj),
            capture: false,
            property: true,
            once: false,
            passive: false,
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn dispatch_modern_at(
    mevent_id: usize,
    owner: Target,
    event_type: &str,
    capture: bool,
    phase: u8,
    cancelable: bool,
    wrapper: &JsValue,
    context: &mut Context,
) -> JsResult<()> {
    if !capture {
        ensure_inline_handler_compiled(owner, event_type, context)?;
    }
    let ids: Vec<usize> = {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        data.modern_events[mevent_id].current = Some(owner);
        data.modern_events[mevent_id].phase = phase;
        data.listeners
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                l.callback.is_some()
                    && l.target == owner
                    && l.event_type == event_type
                    && l.capture == capture
            })
            .map(|(i, _)| i)
            .collect()
    };
    if ids.is_empty() {
        return Ok(());
    }
    let this_val = from_host(owner.value(), context)?;
    for id in ids {
        modern::check_context(context)?;
        if state(context).borrow().modern_events[mevent_id].immediate {
            break;
        }
        let entry = {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            let l = &mut data.listeners[id];
            let Some(cb) = l.callback.clone() else {
                continue;
            };
            let prop = l.property;
            let passive = l.passive;
            if l.once {
                l.callback = None;
            }
            (cb, prop, passive)
        };
        let returned_false =
            invoke_callback_object(&entry.0, &this_val, wrapper, entry.1, entry.2, context)?;
        if returned_false && cancelable && !entry.2 {
            state(context).borrow_mut().modern_events[mevent_id].canceled = true;
        }
    }
    Ok(())
}

pub(super) fn queue_or_fire_simple_event(
    node_id: usize,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    context: &mut Context,
) -> JsResult<bool> {
    // Skip allocating if no listener could observe this event
    let any_listener = {
        let shared = state(context);
        let data = shared.borrow();
        let path = build_propagation_path(Target::Node(node_id), &data.host.document.nodes);
        data.listeners
            .iter()
            .any(|l| l.callback.is_some() && l.event_type == event_type && path.contains(&l.target))
            || path.iter().any(|t| match *t {
                Target::Node(nid) => has_uncompiled_inline_handler(&data, nid, event_type),
                Target::Window => false,
            })
    };
    if !any_listener {
        return Ok(true);
    }
    let ts = elapsed_ms(&state(context).borrow());
    let record = ModernEvent::simple(event_type, bubbles, cancelable, true, ts);
    let mevent_id = alloc_modern_event(record, context)?;
    dispatch_modern_event(Target::Node(node_id), mevent_id, context)
}

// --- Focus & ActiveElement ---

fn resolve_active_element(data: &State) -> Option<usize> {
    if let Some(id) = data.focused_node
        && id < data.host.document.nodes.len()
        && !data.host.document.nodes[id].tag.starts_with('#')
        && dom_ops::is_connected_node(&data.host.document.nodes, id)
    {
        return Some(id);
    }
    let nodes = &data.host.document.nodes;
    let html_id = nodes[0]
        .children
        .iter()
        .copied()
        .find(|&c| nodes[c].tag == "html");
    if let Some(hid) = html_id
        && let Some(body_id) = nodes[hid]
            .children
            .iter()
            .copied()
            .find(|&c| nodes[c].tag == "body")
    {
        return Some(body_id);
    }
    html_id
}

pub(super) fn is_disabled_element(nodes: &[Node], id: usize) -> bool {
    let tag = nodes[id].tag.as_str();
    if !matches!(tag, "input" | "button" | "select" | "textarea" | "option") {
        return false;
    }
    if nodes[id].has("disabled") {
        return true;
    }
    // Check ancestor <fieldset disabled>
    let mut cur = nodes[id].parent;
    for _ in 0..=crate::document::MAX_DEPTH {
        if cur == 0 || nodes[cur].parent == cur {
            break;
        }
        if nodes[cur].tag == "fieldset" && nodes[cur].has("disabled") {
            return true;
        }
        cur = nodes[cur].parent;
    }
    false
}

fn focus_element(id: usize, context: &mut Context) -> JsResult<()> {
    let (prev, can_focus) = {
        let shared = state(context);
        let data = shared.borrow();
        let can = dom_ops::is_connected_node(&data.host.document.nodes, id)
            && !is_disabled_element(&data.host.document.nodes, id);
        (data.focused_node, can)
    };
    if !can_focus || prev == Some(id) {
        return Ok(());
    }
    if let Some(old) = prev {
        state(context).borrow_mut().focused_node = None;
        queue_or_fire_simple_event(old, "blur", false, false, context)?;
        queue_or_fire_simple_event(old, "focusout", true, false, context)?;
    }
    state(context).borrow_mut().focused_node = Some(id);
    queue_or_fire_simple_event(id, "focus", false, false, context)?;
    queue_or_fire_simple_event(id, "focusin", true, false, context)?;
    Ok(())
}

fn blur_element(id: usize, context: &mut Context) -> JsResult<()> {
    let is_focused = state(context).borrow().focused_node == Some(id);
    if !is_focused {
        return Ok(());
    }
    state(context).borrow_mut().focused_node = None;
    queue_or_fire_simple_event(id, "blur", false, false, context)?;
    queue_or_fire_simple_event(id, "focusout", true, false, context)?;
    Ok(())
}

// --- Rich Click Activation (Shared by JS `el.click()` and Native `BoaPageRealm::click_activation`) ---

#[derive(Clone)]
pub(super) enum PreActivation {
    None,
    Checkbox {
        node: usize,
        prev_checked: bool,
    },
    Radio {
        node: usize,
        prev_group: Vec<(usize, bool)>,
        changed: bool,
    },
    OptionItem {
        option: usize,
        select: usize,
    },
    Summary {
        details: usize,
    },
    Label {
        target_control: usize,
    },
    Reset {
        button: usize,
    },
}

pub(super) fn prepare_pre_activation(
    data: &mut State,
    target: usize,
) -> Result<PreActivation, String> {
    let nodes = &data.host.document.nodes;
    let mut chain = Vec::new();
    let mut cur = target;
    for _ in 0..=crate::document::MAX_DEPTH {
        chain.push(cur);
        if cur == 0 || nodes[cur].parent == cur {
            break;
        }
        cur = nodes[cur].parent;
    }

    for &id in &chain {
        let tag = data.host.document.nodes[id].tag.as_str();
        if tag.starts_with('#') {
            continue;
        }
        if is_disabled_element(&data.host.document.nodes, id) {
            return Ok(PreActivation::None);
        }
        match tag {
            "input" => {
                let kind = data.host.document.nodes[id]
                    .attr("type")
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                if kind == "checkbox" {
                    let prev = data.host.document.nodes[id].has("checked");
                    dom_ops::set_input_checked(data, id, !prev)?;
                    return Ok(PreActivation::Checkbox {
                        node: id,
                        prev_checked: prev,
                    });
                }
                if kind == "radio" {
                    let prev_self = data.host.document.nodes[id].has("checked");
                    let group_name = data.host.document.nodes[id]
                        .attr("name")
                        .unwrap_or("")
                        .to_owned();
                    let mut prev_group = Vec::new();
                    if !group_name.is_empty() {
                        let owner = data.host.form_owner(id);
                        let connected = dom_ops::is_connected_node(&data.host.document.nodes, id);
                        for peer in 1..data.host.document.nodes.len() {
                            if data.host.document.nodes[peer].tag == "input"
                                && data.host.document.nodes[peer]
                                    .attr("type")
                                    .is_some_and(|t| t.eq_ignore_ascii_case("radio"))
                                && data.host.document.nodes[peer].attr("name")
                                    == Some(group_name.as_str())
                                && (!connected
                                    || (dom_ops::is_connected_node(
                                        &data.host.document.nodes,
                                        peer,
                                    ) && data.host.form_owner(peer) == owner))
                            {
                                prev_group
                                    .push((peer, data.host.document.nodes[peer].has("checked")));
                            }
                        }
                    } else {
                        prev_group.push((id, prev_self));
                    }
                    dom_ops::set_input_checked(data, id, true)?;
                    return Ok(PreActivation::Radio {
                        node: id,
                        prev_group,
                        changed: !prev_self,
                    });
                }
                if kind == "reset" {
                    return Ok(PreActivation::Reset { button: id });
                }
                return Ok(PreActivation::None);
            }
            "button" => {
                let kind = data.host.document.nodes[id]
                    .attr("type")
                    .unwrap_or("submit")
                    .to_ascii_lowercase();
                if kind == "reset" {
                    return Ok(PreActivation::Reset { button: id });
                }
                return Ok(PreActivation::None);
            }
            "option" => {
                let mut p = data.host.document.nodes[id].parent;
                if p != id && data.host.document.nodes[p].tag == "optgroup" {
                    if data.host.document.nodes[p].has("disabled") {
                        return Ok(PreActivation::None);
                    }
                    p = data.host.document.nodes[p].parent;
                }
                if p != id
                    && data.host.document.nodes[p].tag == "select"
                    && !is_disabled_element(&data.host.document.nodes, p)
                {
                    return Ok(PreActivation::OptionItem {
                        option: id,
                        select: p,
                    });
                }
                return Ok(PreActivation::None);
            }
            "select" | "textarea" | "a" => {
                return Ok(PreActivation::None);
            }
            "summary" => {
                let p = data.host.document.nodes[id].parent;
                if p != id && data.host.document.nodes[p].tag == "details" {
                    let first_summary = data.host.document.nodes[p]
                        .children
                        .iter()
                        .copied()
                        .find(|&c| data.host.document.nodes[c].tag == "summary");
                    if first_summary == Some(id) {
                        return Ok(PreActivation::Summary { details: p });
                    }
                }
            }
            "label" => {
                if let Some(ctrl) = data.host.document.label_target(id)
                    && ctrl != target
                    && !chain.contains(&ctrl)
                {
                    return Ok(PreActivation::Label {
                        target_control: ctrl,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(PreActivation::None)
}

pub(super) fn revert_pre_activation(data: &mut State, pre: &PreActivation) -> Result<(), String> {
    match pre {
        PreActivation::Checkbox { node, prev_checked } => {
            dom_ops::set_input_checked(data, *node, *prev_checked)?;
        }
        PreActivation::Radio { prev_group, .. } => {
            for &(peer, was_checked) in prev_group {
                if was_checked {
                    data.host.attr(peer, "checked", String::new())?;
                } else {
                    data.host.document.nodes[peer]
                        .attributes
                        .retain(|(n, _)| n != "checked");
                }
            }
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn reset_form_controls(data: &mut State, form_id: usize) -> Result<(), String> {
    let count = data.host.document.nodes.len();
    for id in 1..count {
        if !dom_ops::is_connected_node(&data.host.document.nodes, id)
            || data.host.form_owner(id) != Some(form_id)
        {
            continue;
        }
        let tag = data.host.document.nodes[id].tag.clone();
        match tag.as_str() {
            "input" => {
                let kind = data.host.document.nodes[id]
                    .attr("type")
                    .unwrap_or("text")
                    .to_ascii_lowercase();
                if kind == "checkbox" || kind == "radio" {
                    let def = data
                        .initial_checked
                        .get(&id)
                        .copied()
                        .unwrap_or_else(|| data.host.document.nodes[id].has("checked"));
                    if def {
                        data.host.attr(id, "checked", String::new())?;
                    } else {
                        data.host.document.nodes[id]
                            .attributes
                            .retain(|(n, _)| n != "checked");
                    }
                } else if !matches!(kind.as_str(), "submit" | "reset" | "button" | "hidden") {
                    if let Some(def_val) = data.initial_values.get(&id).cloned() {
                        match def_val {
                            Some(v) => data.host.attr(id, "value", v)?,
                            None => data.host.document.nodes[id]
                                .attributes
                                .retain(|(n, _)| n != "value"),
                        }
                    }
                }
            }
            "textarea" => {
                if let Some(Some(def_text)) = data.initial_values.get(&id).cloned() {
                    data.host.replace_text(id, def_text)?;
                }
            }
            "select" => {
                data.unselected_selects.remove(&id);
                let opts = dom_ops::select_option_nodes(&data.host.document.nodes, id);
                for oid in opts {
                    let def_sel = data.initial_checked.get(&oid).copied().unwrap_or(false);
                    if def_sel {
                        data.host.attr(oid, "selected", String::new())?;
                    } else {
                        data.host.document.nodes[oid]
                            .attributes
                            .retain(|(n, _)| n != "selected");
                    }
                }
            }
            _ => {}
        }
    }
    data.dom_epoch = data.dom_epoch.wrapping_add(1);
    Ok(())
}

pub(super) fn snapshot_initial_form_defaults(data: &mut State) {
    for (id, node) in data.host.document.nodes.iter().enumerate() {
        match node.tag.as_str() {
            "input" => {
                let kind = node.attr("type").unwrap_or("text").to_ascii_lowercase();
                if kind == "checkbox" || kind == "radio" {
                    data.initial_checked.insert(id, node.has("checked"));
                } else {
                    data.initial_values
                        .insert(id, node.attr("value").map(str::to_owned));
                }
            }
            "textarea" => {
                let txt = super::text_content(&data.host.document.nodes, id);
                data.initial_values.insert(id, Some(txt));
            }
            "option" => {
                data.initial_checked.insert(id, node.has("selected"));
            }
            _ => {}
        }
    }
}

fn finish_post_activation_in_context(
    pre: PreActivation,
    canceled: bool,
    context: &mut Context,
) -> JsResult<()> {
    if canceled {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        revert_pre_activation(&mut data, &pre).map_err(failure)?;
        return Ok(());
    }
    match pre {
        PreActivation::None => {}
        PreActivation::Checkbox { node, .. } => {
            queue_or_fire_simple_event(node, "input", true, false, context)?;
            queue_or_fire_simple_event(node, "change", true, false, context)?;
        }
        PreActivation::Radio { node, changed, .. } => {
            if changed {
                queue_or_fire_simple_event(node, "input", true, false, context)?;
                queue_or_fire_simple_event(node, "change", true, false, context)?;
            }
        }
        PreActivation::OptionItem { option, select } => {
            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let opts = dom_ops::select_option_nodes(&data.host.document.nodes, select);
                if let Some(pos) = opts.iter().position(|&oid| oid == option) {
                    dom_ops::set_select_index(&mut data, select, pos as i32).map_err(failure)?;
                }
            }
            queue_or_fire_simple_event(select, "input", true, false, context)?;
            queue_or_fire_simple_event(select, "change", true, false, context)?;
        }
        PreActivation::Summary { details } => {
            {
                let shared = state(context);
                let mut data = shared.borrow_mut();
                let is_open = data.host.document.nodes[details].has("open");
                if is_open {
                    data.host.document.nodes[details]
                        .attributes
                        .retain(|(n, _)| n != "open");
                } else {
                    data.host
                        .attr(details, "open", String::new())
                        .map_err(failure)?;
                }
                data.dom_epoch = data.dom_epoch.wrapping_add(1);
            }
            queue_or_fire_simple_event(details, "toggle", false, false, context)?;
        }
        PreActivation::Label { target_control } => {
            focus_element(target_control, context)?;
            js_element_click(target_control, context)?;
        }
        PreActivation::Reset { button } => {
            let form = {
                let shared = state(context);
                let data = shared.borrow();
                if dom_ops::is_connected_node(&data.host.document.nodes, button) {
                    data.host.form_owner(button)
                } else {
                    None
                }
            };
            if let Some(form_id) = form {
                let proceed = queue_or_fire_simple_event(form_id, "reset", true, true, context)?;
                if proceed {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    reset_form_controls(&mut data, form_id).map_err(failure)?;
                }
            }
        }
    }
    Ok(())
}

fn js_element_click(target: usize, context: &mut Context) -> JsResult<()> {
    {
        let shared = state(context);
        let mut data = shared.borrow_mut();
        if data.clicking_nodes.contains(&target)
            || is_disabled_element(&data.host.document.nodes, target)
        {
            return Ok(());
        }
        data.clicking_nodes.insert(target);
    }

    let result = (|| -> JsResult<()> {
        let pre = {
            let shared = state(context);
            let mut data = shared.borrow_mut();
            prepare_pre_activation(&mut data, target).map_err(failure)?
        };
        let ts = elapsed_ms(&state(context).borrow());
        let mut ev = ModernEvent::simple("click", true, true, false, ts);
        ev.flavor = EventFlavor::MouseEvent;
        ev.composed = true;
        ev.detail = 1.into();
        let mevent_id = alloc_modern_event(ev, context)?;
        let not_canceled = dispatch_modern_event(Target::Node(target), mevent_id, context)?;
        let canceled = !not_canceled;

        finish_post_activation_in_context(pre.clone(), canceled, context)?;

        if !canceled && matches!(pre, PreActivation::None) {
            // Check submitter or <a href> default action for connected elements
            let default_act = {
                let shared = state(context);
                let data = shared.borrow();
                if let Ok((path, length)) = data.host.connected_path(target) {
                    path[..length].iter().find_map(|t| match t {
                        Target::Node(id)
                            if data.host.is_submitter(*id)
                                || (data.host.document.nodes[*id].tag == "a"
                                    && data.host.document.nodes[*id].has("href")) =>
                        {
                            Some(*id)
                        }
                        _ => None,
                    })
                } else {
                    None
                }
            };
            if let Some(node) = default_act {
                let (submit_form, link_href) = {
                    let shared = state(context);
                    let data = shared.borrow();
                    if data.host.connected_path(node).is_ok() {
                        if data.host.is_submitter(node) {
                            (data.host.form_owner(node), None)
                        } else if data.host.document.nodes[node].tag == "a" {
                            (
                                None,
                                data.host.document.nodes[node]
                                    .attr("href")
                                    .map(str::to_owned),
                            )
                        } else {
                            (None, None)
                        }
                    } else {
                        (None, None)
                    }
                };
                if let Some(form) = submit_form {
                    let ts2 = elapsed_ms(&state(context).borrow());
                    let mut sev = ModernEvent::simple("submit", true, true, false, ts2);
                    sev.submitter = Some(node);
                    let sid = alloc_modern_event(sev, context)?;
                    let submit_ok = dispatch_modern_event(Target::Node(form), sid, context)?;
                    if submit_ok {
                        let shared = state(context);
                        let mut data = shared.borrow_mut();
                        if data.host.navigation.is_none()
                            && data.host.connected_path(form).is_ok()
                            && data.host.connected_path(node).is_ok()
                            && data.host.is_submitter(node)
                            && data.host.form_owner(node) == Some(form)
                        {
                            submit_form_via_navigation(&mut data, form, Some(node));
                        }
                    }
                } else if let Some(href) = link_href {
                    let shared = state(context);
                    let mut data = shared.borrow_mut();
                    if data.host.navigation.is_none() {
                        let _ = data.host.navigate(&href);
                    }
                }
            }
        }
        Ok(())
    })();

    state(context).borrow_mut().clicking_nodes.remove(&target);
    result
}

// ---Realm-level helper for native `click_activation` post-activation & `HistoryTraverse` ---

pub(super) fn finish_realm_post_activation(
    realm: &mut BoaPageRealm,
    pre: PreActivation,
    canceled: bool,
    errors: &mut Vec<String>,
) -> Result<(), String> {
    if matches!(pre, PreActivation::None) {
        return Ok(());
    }
    let res = {
        let _ingress = super::Ingress::enter(realm.state.clone());
        finish_post_activation_in_context(pre, canceled, realm.engine.context_mut())
    };
    if let Err(err) = res {
        if realm.engine.is_fatal() {
            return Ok(());
        }
        if errors.len() < 64 {
            errors.push(super::bounded(&err.to_string(), 512));
        }
    }
    realm.checkpoint(errors);
    flush_async_errors(realm, errors);
    Ok(())
}

pub(super) fn fire_select_events(
    realm: &mut BoaPageRealm,
    select_node: usize,
    errors: &mut Vec<String>,
) {
    let res = {
        let _ingress = super::Ingress::enter(realm.state.clone());
        let ctx = realm.engine.context_mut();
        (|| -> JsResult<()> {
            queue_or_fire_simple_event(select_node, "input", true, false, ctx)?;
            if !modern::check_context(ctx).is_err() {
                queue_or_fire_simple_event(select_node, "change", true, false, ctx)?;
            }
            Ok(())
        })()
    };
    if let Err(err) = res
        && !realm.engine.is_fatal()
        && errors.len() < 64
    {
        errors.push(super::bounded(&err.to_string(), 512));
    }
    realm.checkpoint(errors);
    flush_async_errors(realm, errors);
}

pub(super) fn flush_async_errors(realm: &mut BoaPageRealm, errors: &mut Vec<String>) {
    let drained = std::mem::take(&mut realm.state.borrow_mut().async_errors);
    for msg in drained {
        if errors.len() < 64 {
            errors.push(msg);
        }
    }
}

pub(super) fn fire_popstate_and_hashchange(
    realm: &mut BoaPageRealm,
    state_val: JsValue,
    old_url: &str,
    new_url: &str,
    errors: &mut Vec<String>,
) {
    let hash_changed = url_only_hash_differs(old_url, new_url);
    let res = {
        let _ingress = super::Ingress::enter(realm.state.clone());
        let ctx = realm.engine.context_mut();
        (|| -> JsResult<()> {
            let ts = elapsed_ms(&state(ctx).borrow());
            let mut pop = ModernEvent::simple("popstate", true, false, true, ts);
            pop.flavor = EventFlavor::PopStateEvent;
            pop.state_val = state_val;
            let pid = alloc_modern_event(pop, ctx)?;
            dispatch_modern_event(Target::Window, pid, ctx)?;

            if hash_changed && !modern::check_context(ctx).is_err() {
                let ts2 = elapsed_ms(&state(ctx).borrow());
                let mut hc = ModernEvent::simple("hashchange", true, false, true, ts2);
                hc.flavor = EventFlavor::HashChangeEvent;
                hc.old_url = old_url.to_owned();
                hc.new_url = new_url.to_owned();
                let hid = alloc_modern_event(hc, ctx)?;
                dispatch_modern_event(Target::Window, hid, ctx)?;
            }
            Ok(())
        })()
    };
    if let Err(err) = res
        && !realm.engine.is_fatal()
        && errors.len() < 64
    {
        errors.push(super::bounded(&err.to_string(), 512));
    }
    realm.checkpoint(errors);
    flush_async_errors(realm, errors);
}

fn url_only_hash_differs(old_url: &str, new_url: &str) -> bool {
    let Ok(u1) = url::Url::parse(old_url) else {
        return false;
    };
    let Ok(u2) = url::Url::parse(new_url) else {
        return false;
    };
    if u1.fragment() == u2.fragment() {
        return false;
    }
    let mut c1 = u1;
    let mut c2 = u2;
    c1.set_fragment(None);
    c2.set_fragment(None);
    c1 == c2
}

pub(super) fn submit_form_via_navigation(
    data: &mut State,
    form_id: usize,
    submitter: Option<usize>,
) {
    let Ok(projected) =
        crate::document::project_nodes(data.host.document.nodes.clone(), data.host.url.as_str())
    else {
        return;
    };
    let Some(form_idx) = projected.form_nodes.iter().position(|&n| n == form_id) else {
        return;
    };
    let Some(form_data) = projected.forms.get(form_idx) else {
        return;
    };
    if form_data.method != "get" {
        return;
    }
    let mut fields = form_data.fields.clone();
    for item in &projected.items {
        match item {
            crate::document::Item::Input {
                form: Some(f),
                name,
                value,
                ..
            } if *f == form_idx && !name.is_empty() => {
                fields.push((name.clone(), value.clone()));
            }
            crate::document::Item::Toggle {
                form: Some(f),
                node,
                name,
                value,
                checked: true,
                ..
            } if *f == form_idx
                && !name.is_empty()
                && !projected
                    .nodes
                    .get(*node)
                    .is_some_and(|n| n.has("disabled")) =>
            {
                fields.push((name.clone(), value.clone()));
            }
            crate::document::Item::Select {
                form: Some(f),
                node,
                name,
                options,
                selected: Some(idx),
            } if *f == form_idx
                && !name.is_empty()
                && !projected
                    .nodes
                    .get(*node)
                    .is_some_and(|n| n.has("disabled")) =>
            {
                if let Some(opt) = options.get(*idx).filter(|o| !o.disabled) {
                    fields.push((name.clone(), opt.value.clone()));
                }
            }
            _ => {}
        }
    }
    if let Some(sub_node) = submitter
        && let Some(item_idx) = projected.item_nodes.iter().position(|&n| n == sub_node)
        && let Some(crate::document::Item::Submit { name, value, .. }) =
            projected.items.get(item_idx)
        && !name.is_empty()
    {
        fields.push((name.clone(), value.clone()));
    }
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish();
    if let Ok(mut target) = url::Url::parse(&form_data.action) {
        target.set_query(Some(&encoded));
        let _ = data.host.navigate(target.as_str());
    }
}
