//! Guarded Window postMessage overload conversion and shared-task admission.
use super::*;
use modern::boa_engine::{JsSymbol, object::builtins::JsArrayBuffer};

pub(super) enum TargetOrigin {
    Any,
    Exact(url::Origin),
}

impl TargetOrigin {
    pub(super) fn matches(&self, url: &url::Url) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(origin) => origin == &url.origin(),
        }
    }
}

fn guarded<T>(result: JsResult<T>, context: &mut Context) -> JsResult<T> {
    // A callback can exhaust policy and then make an otherwise ordinary native
    // operation return. Fatal state always wins over an ordinary completion.
    if result
        .as_ref()
        .err()
        .is_some_and(|error| error.as_engine().is_some())
    {
        return Err(modern::latch_context(
            context,
            "Window message conversion reached an engine runtime limit",
        ));
    }
    modern::check_context(context)?;
    result
}

fn usv_string(value: &JsValue, context: &mut Context) -> JsResult<JsString> {
    let result = value.to_string(context);
    guarded(result, context)
}

fn close_failed_item(iterator: &JsObject, error: JsError, context: &mut Context) -> JsResult<()> {
    let result = iterator.get(js_string!("return"), context);
    // IteratorClose with a throw completion preserves that original ordinary
    // exception, even when return/GetMethod throws. A fatal latch still wins.
    let method = guarded(result, context);
    if let Ok(method) = method {
        if let Some(callback) = method.as_callable() {
            let result = callback.call(&iterator.clone().into(), &[], context);
            let result = guarded(result, context);
            modern::check_context(context)?;
            drop(result);
        }
    }
    modern::check_context(context)?;
    Err(error)
}

fn transfers(value: &JsValue, context: &mut Context) -> JsResult<Vec<JsObject>> {
    let mut objects = Vec::new();
    // An omitted/undefined dictionary member or optional argument defaults to
    // []. Null is not an iterable sequence and must not become that default.
    if value.is_undefined() {
        return Ok(objects);
    }
    let object = value
        .as_object()
        .ok_or_else(|| failure("Transfer must be an iterable object"))?;
    let result = object.get(JsSymbol::iterator(), context);
    let method = guarded(result, context)?
        .as_callable()
        .ok_or_else(|| failure("Transfer must be iterable"))?;
    let result = method.call(value, &[], context);
    let iterator = guarded(result, context)?
        .as_object()
        .ok_or_else(|| failure("Transfer iterator must return an object"))?;
    let result = iterator.get(js_string!("next"), context);
    let next = guarded(result, context)?
        .as_callable()
        .ok_or_else(|| failure("Transfer iterator next must be callable"))?;
    loop {
        let result = next.call(&iterator.clone().into(), &[], context);
        let item = guarded(result, context)?
            .as_object()
            .ok_or_else(|| failure("Transfer iterator result must be an object"))?;
        let result = item.get(js_string!("done"), context);
        if guarded(result, context)?.to_boolean() {
            return Ok(objects);
        }
        let result = item.get(js_string!("value"), context);
        let value = guarded(result, context)?;
        let Some(object) = value.as_object() else {
            close_failed_item(
                &iterator,
                failure("Transfer entries must be objects"),
                context,
            )?;
            unreachable!("IteratorClose always returns the original item error")
        };
        message_reserve(&mut objects, 1, context)?;
        objects.push(object);
    }
}

fn origin_argument(text: &JsString, context: &mut Context) -> JsResult<TargetOrigin> {
    if text == &js_string!("*") {
        return Ok(TargetOrigin::Any);
    }
    if text == &js_string!("/") {
        let origin = state(context).borrow().host.url.origin();
        return retain_origin(origin, context);
    }
    // Web IDL USVString replaces unpaired UTF-16 surrogates. Own and admit the
    // actual UTF-8 builder, rather than charging a guessed maximum allowance.
    let mut bytes = Vec::new();
    for value in char::decode_utf16(text.iter()) {
        let character = value.unwrap_or(char::REPLACEMENT_CHARACTER);
        let mut encoded = [0; 4];
        let encoded = character.encode_utf8(&mut encoded).as_bytes();
        message_reserve(&mut bytes, encoded.len(), context)?;
        bytes.extend_from_slice(encoded);
    }
    let value =
        String::from_utf8(bytes).map_err(|_| modern::latch_context(context, MESSAGE_FAILURE))?;
    let url = url::Url::parse(&value).map_err(|_| ());
    let url = match url {
        Ok(url) => url,
        Err(()) => {
            return Err(dom_exception::exception(
                "SyntaxError",
                "Invalid target origin",
                context,
            )?);
        }
    };
    // URL's opaque native allocations are independently System-admitted in the
    // worker. Charge retained native origin storage before adopting it here.
    retain_origin(url.origin(), context)
}

fn retain_origin(origin: url::Origin, context: &mut Context) -> JsResult<TargetOrigin> {
    let bytes = match &origin {
        url::Origin::Tuple(scheme, host, _) => {
            scheme.capacity()
                + match host {
                    url::Host::Domain(name) => name.capacity(),
                    url::Host::Ipv4(_) | url::Host::Ipv6(_) => 0,
                }
        }
        url::Origin::Opaque(_) => 0,
    };
    // The enum itself moves into the separately admitted queue Vec. Only its
    // actual owned String buffers are additional retained allocations here.
    message_charge(bytes, context)?;
    Ok(TargetOrigin::Exact(origin))
}

fn validate_transfers(objects: &[JsObject], context: &mut Context) -> JsResult<()> {
    for (index, object) in objects.iter().enumerate() {
        if objects[..index].contains(object) {
            return Err(dom_exception::exception(
                "DataCloneError",
                "Duplicate transfer entry",
                context,
            )?);
        }
        let Ok(buffer) = JsArrayBuffer::from_object(object.clone()) else {
            return Err(dom_exception::exception(
                "DataCloneError",
                "Object is not transferable",
                context,
            )?);
        };
        if buffer.data().is_none() {
            return Err(dom_exception::exception(
                "DataCloneError",
                "Detached transfer buffer",
                context,
            )?);
        }
    }
    if !objects.is_empty() {
        return Err(dom_exception::exception(
            "NotSupportedError",
            "Transfer lists are not implemented",
            context,
        )?);
    }
    Ok(())
}

pub(super) fn post(this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    if !(this.is_null() || this.is_undefined() || this.as_object() == Some(context.global_object()))
    {
        return Err(failure("postMessage requires its Window receiver"));
    }
    let message = args
        .first()
        .ok_or_else(|| failure("postMessage requires a message"))?;
    let undefined = JsValue::undefined();
    let second = args.get(1).unwrap_or(&undefined);
    let (origin, transfer) =
        if args.len() >= 3 || !(second.is_object() || second.is_null() || second.is_undefined()) {
            let origin = usv_string(second, context)?;
            let transfer = transfers(args.get(2).unwrap_or(&undefined), context)?;
            (origin, transfer)
        } else if let Some(options) = second.as_object() {
            // Web IDL processes inherited StructuredSerializeOptions members first.
            let result = options.get(js_string!("transfer"), context);
            let transfer = transfers(&guarded(result, context)?, context)?;
            let result = options.get(js_string!("targetOrigin"), context);
            let origin = guarded(result, context)?;
            let origin = if origin.is_undefined() {
                js_string!("/")
            } else {
                usv_string(&origin, context)?
            };
            (origin, transfer)
        } else {
            (js_string!("/"), Vec::new())
        };
    let target = origin_argument(&origin, context)?;
    validate_transfers(&transfer, context)?;
    drop(transfer);
    let permit = modern::admit_host_task(context)?;
    // Hold admission before retaining clone roots/storage or allocating a queue
    // record. IDs/deadlines are assigned only after all getter re-entry ends.
    let graph = message_clone::serialize(message, context)?;
    modern::check_context(context)?;
    let shared = state(context);
    let origin = shared.borrow().host.url.origin().ascii_serialization();
    message_charge(origin.capacity(), context)?;
    let admitted = (|| {
        let mut data = shared.borrow_mut();
        let handle = data.next_handle.checked_add(1).ok_or(())?;
        let deadline = data.now();
        let State {
            host, dom_tasks, ..
        } = &mut *data;
        reserve_storage(host, dom_tasks, 1)?;
        Ok((handle, deadline))
    })();
    let (handle, deadline) =
        admitted.map_err(|()| modern::latch_context(context, MESSAGE_FAILURE))?;
    let mut data = shared.borrow_mut();
    data.next_handle = handle;
    data.dom_tasks.push(DomTask {
        handle,
        deadline,
        kind: DomTaskKind::Message {
            permit,
            graph,
            target,
            origin,
        },
    });
    Ok(JsValue::undefined())
}
