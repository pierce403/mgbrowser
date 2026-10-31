//! Genuine Error-backed DOMException objects with context-private weak brands.
//!
//! Native functions never capture a Context or consult a page-replaced global
//! constructor. The registry roots only setup objects; its WeakMap values have
//! no reference back to their instance keys.
use super::message_charge;
use mg_butane::modern;
use modern::boa_engine::{
    Context, JsData, JsError, JsNativeError, JsResult, JsString, JsSymbol, JsValue, NativeFunction,
    builtins::error::{Error as NativeError, ErrorKind},
    js_string,
    object::{FunctionObjectBuilder, JsObject, builtins::JsWeakMap},
    property::{Attribute, PropertyDescriptor},
    realm::Realm,
};
use modern::boa_gc::{Finalize, Trace};

const BINDING_FAILURE: &str = "DOMException binding invariant or storage admission failed";
// Logical native-object/membership admission, following the existing weak-map
// bindings. The worker System allocator separately admits actual Boa storage.
const NATIVE_MEMBERSHIP_BYTES: usize = 256;

const LEGACY_CONSTANTS: [(&str, u16); 25] = [
    ("INDEX_SIZE_ERR", 1),
    ("DOMSTRING_SIZE_ERR", 2),
    ("HIERARCHY_REQUEST_ERR", 3),
    ("WRONG_DOCUMENT_ERR", 4),
    ("INVALID_CHARACTER_ERR", 5),
    ("NO_DATA_ALLOWED_ERR", 6),
    ("NO_MODIFICATION_ALLOWED_ERR", 7),
    ("NOT_FOUND_ERR", 8),
    ("NOT_SUPPORTED_ERR", 9),
    ("INUSE_ATTRIBUTE_ERR", 10),
    ("INVALID_STATE_ERR", 11),
    ("SYNTAX_ERR", 12),
    ("INVALID_MODIFICATION_ERR", 13),
    ("NAMESPACE_ERR", 14),
    ("INVALID_ACCESS_ERR", 15),
    ("VALIDATION_ERR", 16),
    ("TYPE_MISMATCH_ERR", 17),
    ("SECURITY_ERR", 18),
    ("NETWORK_ERR", 19),
    ("ABORT_ERR", 20),
    ("URL_MISMATCH_ERR", 21),
    ("QUOTA_EXCEEDED_ERR", 22),
    ("TIMEOUT_ERR", 23),
    ("INVALID_NODE_TYPE_ERR", 24),
    ("DATA_CLONE_ERR", 25),
];

const LEGACY_NAMES: [(&str, u16); 22] = [
    ("IndexSizeError", 1),
    ("HierarchyRequestError", 3),
    ("WrongDocumentError", 4),
    ("InvalidCharacterError", 5),
    ("NoModificationAllowedError", 7),
    ("NotFoundError", 8),
    ("NotSupportedError", 9),
    ("InUseAttributeError", 10),
    ("InvalidStateError", 11),
    ("SyntaxError", 12),
    ("InvalidModificationError", 13),
    ("NamespaceError", 14),
    ("InvalidAccessError", 15),
    ("TypeMismatchError", 17),
    ("SecurityError", 18),
    ("NetworkError", 19),
    ("AbortError", 20),
    ("URLMismatchError", 21),
    ("QuotaExceededError", 22),
    ("TimeoutError", 23),
    ("InvalidNodeTypeError", 24),
    ("DataCloneError", 25),
];

#[derive(Clone)]
struct Registry {
    prototype: JsObject,
    brands: JsWeakMap,
    realm: Realm,
}

#[derive(Trace, Finalize, JsData)]
struct ExceptionData {
    name: JsString,
    message: JsString,
    code: u16,
}

fn invariant(context: &mut Context) -> JsError {
    modern::latch_context(context, BINDING_FAILURE)
}

fn registry(context: &mut Context) -> JsResult<Registry> {
    modern::check_context(context)?;
    let Some(registry) = context.get_data::<Registry>().cloned() else {
        return Err(invariant(context));
    };
    // Context data is authority for this one owned page realm, not a way to
    // attach an imported native function to another Context's work ledger.
    if registry.realm != *context.realm() {
        return Err(JsNativeError::typ()
            .with_message("DOMException requires its owning page realm")
            .into());
    }
    Ok(registry)
}

fn instance_admission(
    name_units: usize,
    message_units: usize,
    context: &mut Context,
) -> JsResult<()> {
    // This is logical retained UTF-16 extent, not a claim that immutable
    // JsString clones allocate their backing bytes again. System accounts for
    // their actual allocations and all native GC/hash/property storage.
    let bytes = name_units
        .checked_add(message_units)
        .and_then(|units| units.checked_mul(std::mem::size_of::<u16>()))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<ExceptionData>()))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<NativeError>()))
        .and_then(|bytes| bytes.checked_add(NATIVE_MEMBERSHIP_BYTES))
        .ok_or_else(|| invariant(context))?;
    message_charge(bytes, context)
}

fn legacy_code(name: &JsString) -> u16 {
    LEGACY_NAMES
        .iter()
        .find(|(candidate, _)| {
            name.len() == candidate.len() && name.iter().eq(candidate.encode_utf16())
        })
        .map_or(0, |(_, code)| *code)
}

// Call only after admission. This path invokes no JavaScript, including while
// assigning the native weak brand; both constructors and host errors use it.
fn allocate(
    name: JsString,
    message: JsString,
    prototype: JsObject,
    registry: &Registry,
    context: &mut Context,
) -> JsResult<JsObject> {
    modern::check_context(context)?;
    let code = legacy_code(&name);
    let instance = JsObject::from_proto_and_data(prototype, NativeError::new(ErrorKind::Error));
    let payload = JsObject::from_proto_and_data(
        None,
        ExceptionData {
            name,
            message,
            code,
        },
    );
    registry
        .brands
        .set(&instance, payload.into(), context)
        .map_err(|_| invariant(context))?;
    Ok(instance)
}

fn constructor(new_target: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let registry = registry(context)?;
    // Pinned Boa's native Base [[Call]] always supplies undefined, ignoring an
    // explicit .call/.apply receiver; only [[Construct]] supplies newTarget.
    if new_target.is_undefined() {
        return Err(JsNativeError::typ()
            .with_message("DOMException constructor requires new")
            .into());
    }
    let message = match args.first().filter(|value| !value.is_undefined()) {
        Some(value) => {
            let converted = value.to_string(context);
            modern::check_context(context)?;
            converted?
        }
        None => js_string!(),
    };
    let name = match args.get(1).filter(|value| !value.is_undefined()) {
        Some(value) => {
            let converted = value.to_string(context);
            modern::check_context(context)?;
            converted?
        }
        None => js_string!("Error"),
    };
    // Web IDL converts both arguments before internally creating the object.
    // This observable Get can re-enter through a Proxy newTarget, so no State,
    // policy or native-object borrow spans it and fatal admission is rechecked.
    let target = new_target.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("DOMException requires a constructor newTarget")
    })?;
    let prototype = target.get(js_string!("prototype"), context);
    modern::check_context(context)?;
    let prototype = prototype?
        .as_object()
        .unwrap_or_else(|| registry.prototype.clone());
    instance_admission(name.len(), message.len(), context)?;
    allocate(name, message, prototype, &registry, context).map(Into::into)
}

fn payload(object: &JsObject, context: &mut Context) -> JsResult<Option<JsObject>> {
    let registry = registry(context)?;
    let value = registry
        .brands
        .get(object, context)
        .map_err(|_| invariant(context))?;
    if value.is_undefined() {
        return Ok(None);
    }
    let Some(data) = value.as_object() else {
        return Err(invariant(context));
    };
    if !data.is::<ExceptionData>() || !object.is::<NativeError>() {
        return Err(invariant(context));
    }
    Ok(Some(data))
}

fn getter(this: &JsValue, field: u8, context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let object = this.as_object().ok_or_else(|| {
        JsNativeError::typ().with_message("DOMException getter requires a native instance")
    })?;
    let Some(payload) = payload(&object, context)? else {
        return Err(JsNativeError::typ()
            .with_message("DOMException getter requires a native instance")
            .into());
    };
    let data = payload
        .downcast_ref::<ExceptionData>()
        .ok_or_else(|| invariant(context))?;
    match field {
        0 => Ok(data.name.clone().into()),
        1 => Ok(data.message.clone().into()),
        2 => Ok(u32::from(data.code).into()),
        _ => {
            drop(data);
            Err(invariant(context))
        }
    }
}

/// Install once, after the parent-owned page State and before any page code.
pub(super) fn register(context: &mut Context) -> JsResult<()> {
    modern::check_context(context)?;
    if !context.has_data::<super::Shared>() || context.has_data::<Registry>() {
        return Err(invariant(context));
    }
    let property_storage = (LEGACY_CONSTANTS.len() * 2 + 6)
        .checked_mul(std::mem::size_of::<PropertyDescriptor>())
        .ok_or_else(|| invariant(context))?;
    let key_units: usize = LEGACY_CONSTANTS.iter().map(|(name, _)| name.len()).sum();
    let storage = std::mem::size_of::<Registry>()
        .checked_add(std::mem::size_of::<NativeError>())
        .and_then(|bytes| bytes.checked_add(property_storage))
        .and_then(|bytes| bytes.checked_add(key_units * std::mem::size_of::<u16>()))
        .and_then(|bytes| bytes.checked_add(5 * NATIVE_MEMBERSHIP_BYTES))
        .ok_or_else(|| invariant(context))?;
    message_charge(storage, context)?;
    let prototype = JsObject::from_proto_and_data(
        context.intrinsics().constructors().error().prototype(),
        NativeError::new(ErrorKind::Error),
    );
    let constructor =
        FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(constructor))
            .constructor(true)
            .name(js_string!("DOMException"))
            .length(0)
            .build();
    constructor.define_property_or_throw(
        js_string!("prototype"),
        PropertyDescriptor::builder()
            .value(prototype.clone())
            .writable(false)
            .enumerable(false)
            .configurable(false),
        context,
    )?;
    prototype.define_property_or_throw(
        js_string!("constructor"),
        PropertyDescriptor::builder()
            .value(constructor.clone())
            .writable(true)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    for (key, function_name, field) in [
        ("name", "get name", 0_u8),
        ("message", "get message", 1_u8),
        ("code", "get code", 2_u8),
    ] {
        let get = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_copy_closure_with_captures(
                |this, _, field, context| getter(this, *field, context),
                field,
            ),
        )
        .name(JsString::from(function_name))
        .length(0)
        .build();
        prototype.define_property_or_throw(
            JsString::from(key),
            PropertyDescriptor::builder()
                .get(get)
                .enumerable(true)
                .configurable(true),
            context,
        )?;
    }
    for (name, code) in LEGACY_CONSTANTS {
        let key = JsString::from(name);
        let descriptor = PropertyDescriptor::builder()
            .value(u32::from(code))
            .writable(false)
            .enumerable(true)
            .configurable(false)
            .build();
        constructor.define_property_or_throw(key.clone(), descriptor.clone(), context)?;
        prototype.define_property_or_throw(key, descriptor, context)?;
    }
    prototype.define_property_or_throw(
        JsSymbol::to_string_tag(),
        PropertyDescriptor::builder()
            .value(js_string!("DOMException"))
            .writable(false)
            .enumerable(false)
            .configurable(true),
        context,
    )?;
    let registry = Registry {
        prototype,
        brands: JsWeakMap::new(context),
        realm: context.realm().clone(),
    };
    modern::check_context(context)?;
    context.register_global_property(
        js_string!("DOMException"),
        constructor,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    )?;
    context.insert_data(registry);
    Ok(())
}

/// Make an ordinary catchable host exception without invoking page code.
pub(super) fn exception(name: &str, message: &str, context: &mut Context) -> JsResult<JsError> {
    let registry = registry(context)?;
    // Admit the new retained strings before constructing their native storage.
    instance_admission(
        name.encode_utf16().count(),
        message.encode_utf16().count(),
        context,
    )?;
    let instance = allocate(
        JsString::from(name),
        JsString::from(message),
        registry.prototype.clone(),
        &registry,
        context,
    )?;
    Ok(JsError::from_opaque(instance.into()))
}

pub(super) fn throw(name: &str, message: &str, context: &mut Context) -> JsError {
    match exception(name, message, context) {
        Ok(err) => err,
        Err(err) => err,
    }
}

/// Native identity only: no prototype walk, tag/name access or Proxy traps.
pub(super) fn is_dom_exception(object: &JsObject, context: &mut Context) -> JsResult<bool> {
    payload(object, context).map(|payload| payload.is_some())
}

// Private controls frozen before the implementation bodies. Public authored
// messaging/DOMException inputs have their separate old-engine baseline.
#[cfg(test)]
mod tests {
    use super::super::{BoaPageRealm, Request};
    use super::*;
    use crate::page_session::RealmState;
    use mg_butane::modern::boa_engine::builtins::error::Error as ErrorData;
    use std::{cell::Cell, rc::Rc};

    fn realm() -> BoaPageRealm {
        let (realm, reply) = BoaPageRealm::start(Request {
            url: "https://example.test/dom-exception".into(),
            html: "<html><body><p>owned DOMException control</p></body></html>".into(),
            external_scripts: None,
        });
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        realm.unwrap()
    }

    fn assertion(realm: &mut BoaPageRealm, source: &str) {
        assert_eq!(
            realm.engine.evaluate(source).unwrap().as_boolean(),
            Some(true),
            "{source}"
        );
        realm.engine.checkpoint().unwrap();
    }

    fn collect(realm: &mut BoaPageRealm) {
        realm.engine.checkpoint().unwrap();
        modern::boa_gc::force_collect();
    }

    #[test]
    fn native_dom_exception_has_real_error_data_and_private_instance_brand() {
        let mut realm = realm();
        let before = realm.engine.stats();
        let context = realm.engine.context_mut();
        let prototype = context.get_data::<Registry>().unwrap().prototype.clone();
        assert!(prototype.is::<ErrorData>());
        assert!(!is_dom_exception(&prototype, context).unwrap());
        let value = exception("DataCloneError", "owned native exception", context)
            .unwrap()
            .into_opaque(context)
            .unwrap();
        let object = value.as_object().unwrap();
        assert!(object.is::<ErrorData>());
        assert_eq!(object.prototype(), Some(prototype));
        assert!(is_dom_exception(&object, context).unwrap());
        assert_eq!(realm.engine.stats(), before);
    }

    #[test]
    fn native_dom_exception_call_apply_and_reflect_apply_cannot_spoof_new() {
        let mut realm = realm();
        assertion(
            &mut realm,
            r#"(() => {
            let touched=0;
            const value={toString(){touched++;return 'unexpected';}};
            const calls=[
                ()=>DOMException(value,value),
                ()=>DOMException.call(DOMException,value,value),
                ()=>DOMException.apply(function forged(){},[value,value]),
                ()=>Reflect.apply(DOMException,DOMException,[value,value]),
                ()=>DOMException.bind(DOMException)(value,value),
            ];
            for(const call of calls){
                let rejected=false;
                try{call();}catch(error){rejected=error instanceof TypeError;}
                if(!rejected)return false;
            }
            const direct=new DOMException('direct','DataCloneError');
            const reflected=Reflect.construct(DOMException,['reflected','SyntaxError']);
            class Child extends DOMException {}
            const child=new Child('child','NotSupportedError');
            return touched===0 && direct.code===25 && reflected.code===12 &&
                child instanceof Child && child instanceof DOMException &&
                child instanceof Error && child.message==='child' && child.code===9;
        })()"#,
        );
    }

    #[test]
    fn native_dom_exception_uses_private_prototype_after_global_replacement() {
        let mut realm = realm();
        assertion(
            &mut realm,
            r#"(() => {
            globalThis.savedDOMException=DOMException;
            globalThis.savedDOMExceptionPrototype=DOMException.prototype;
            globalThis.savedNameGetter=Object.getOwnPropertyDescriptor(DOMException.prototype,'name').get;
            globalThis.savedMessageGetter=Object.getOwnPropertyDescriptor(DOMException.prototype,'message').get;
            globalThis.savedCodeGetter=Object.getOwnPropertyDescriptor(DOMException.prototype,'code').get;
            globalThis.constructorTrap=0;
            globalThis.DOMException=function(){constructorTrap++;throw 'wrong constructor';};
            return true;
        })()"#,
        );
        let context = realm.engine.context_mut();
        let value = exception("SyntaxError", "native survives replacement", context)
            .unwrap()
            .into_opaque(context)
            .unwrap();
        context
            .register_global_property(js_string!("nativeException"), value, Attribute::all())
            .unwrap();
        assertion(
            &mut realm,
            r#"constructorTrap===0 &&
            nativeException instanceof savedDOMException && nativeException instanceof Error &&
            Object.getPrototypeOf(nativeException)===savedDOMExceptionPrototype &&
            savedNameGetter.call(nativeException)==='SyntaxError' &&
            savedMessageGetter.call(nativeException)==='native survives replacement' &&
            savedCodeGetter.call(nativeException)===12"#,
        );
    }

    #[test]
    fn native_dom_exception_guards_between_conversions_after_caught_fatal() {
        let mut realm = realm();
        realm
            .engine
            .context_mut()
            .register_global_builtin_callable(
                js_string!("ownedFatal"),
                0,
                NativeFunction::from_fn_ptr(|_, _, context| {
                    Err(modern::latch_context(
                        context,
                        "Owned DOMException fatal conversion control",
                    ))
                }),
            )
            .unwrap();
        realm.engine.evaluate("var nameTouched=0;").unwrap();
        assert!(
            realm
                .engine
                .evaluate(
                    r#"new DOMException(
            {toString(){try{ownedFatal();}catch(error){}return 'message';}},
            {toString(){nameTouched++;return 'DataCloneError';}}
        )"#
                )
                .is_err()
        );
        assert!(realm.engine.is_fatal());
        let global = realm.engine.context().global_object();
        let global = global.borrow();
        let property = global
            .properties()
            .get(&js_string!("nameTouched").into())
            .unwrap();
        assert_eq!(property.value().unwrap().as_number(), Some(0.0));
    }

    #[test]
    fn native_dom_exception_weak_map_keeps_neither_key_nor_payload_alive() {
        let mut realm = realm();
        realm
            .engine
            .evaluate("globalThis.keptException=new DOMException('weak payload','DataCloneError');")
            .unwrap();
        {
            let key = realm
                .engine
                .get_global("keptException")
                .unwrap()
                .as_object()
                .unwrap();
            let context = realm.engine.context_mut();
            let map = context.get_data::<Registry>().unwrap().brands.clone();
            let payload = map.get(&key, context).unwrap();
            let weak_constructor = context.intrinsics().constructors().weak_ref().constructor();
            let weak_key = weak_constructor
                .construct(&[key.into()], None, context)
                .unwrap();
            let weak_payload = weak_constructor
                .construct(&[payload], None, context)
                .unwrap();
            context
                .register_global_property(js_string!("weakException"), weak_key, Attribute::all())
                .unwrap();
            context
                .register_global_property(
                    js_string!("weakExceptionPayload"),
                    weak_payload,
                    Attribute::all(),
                )
                .unwrap();
        }
        collect(&mut realm);
        assertion(
            &mut realm,
            "weakException.deref()===keptException && weakExceptionPayload.deref()!==undefined",
        );
        realm.engine.evaluate("keptException=null;").unwrap();
        collect(&mut realm);
        assertion(
            &mut realm,
            "weakException.deref()===undefined && weakExceptionPayload.deref()===undefined",
        );
    }

    #[derive(Trace, JsData)]
    struct DropProbe {
        #[unsafe_ignore_trace]
        finalized: Rc<Cell<usize>>,
    }

    impl Finalize for DropProbe {
        fn finalize(&self) {
            self.finalized.set(self.finalized.get() + 1);
        }
    }

    #[test]
    fn native_dom_exception_context_teardown_releases_retained_instance() {
        let finalized = Rc::new(Cell::new(0));
        let mut realm = realm();
        {
            let context = realm.engine.context_mut();
            let value = exception("DataCloneError", "teardown", context)
                .unwrap()
                .into_opaque(context)
                .unwrap();
            let object = value.as_object().unwrap();
            let probe = JsObject::from_proto_and_data(
                None,
                DropProbe {
                    finalized: finalized.clone(),
                },
            );
            object
                .create_data_property_or_throw(js_string!("ownedProbe"), probe, context)
                .unwrap();
            context
                .register_global_property(js_string!("retainedException"), value, Attribute::all())
                .unwrap();
        }
        collect(&mut realm);
        assert_eq!(finalized.get(), 0);
        drop(realm);
        modern::boa_gc::force_collect();
        assert_eq!(finalized.get(), 1);
    }
}
