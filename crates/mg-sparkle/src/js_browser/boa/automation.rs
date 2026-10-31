//! Worker-owned synchronous Runtime adapter; authored controls precede implementation.
use super::*;
use crate::page_session::automation::{self as wire, *};
use modern::boa_engine::{JsBigInt, object::builtins::JsArray, value::JsVariant};
use std::mem::size_of;

const STORAGE_FAILURE: &str = "Runtime registry storage admission failed";

#[derive(Trace, Finalize)]
struct Root {
    #[unsafe_ignore_trace]
    owner: u64,
    #[unsafe_ignore_trace]
    published: bool,
    #[unsafe_ignore_trace]
    world_id: u32,
    #[unsafe_ignore_trace]
    group: Option<String>,
    value: Option<JsObject>,
}

#[derive(Trace, Finalize, Default)]
pub(super) struct Registry {
    // The index + 1 is never reused; released entries retain only scalar history.
    roots: Vec<Root>,
    #[unsafe_ignore_trace]
    owners: Vec<(u64, bool)>,
}

fn rejected(message: impl Into<String>) -> AutomationResult {
    AutomationResult::Rejected {
        diagnostic: diagnostic(&message.into()),
    }
}
fn diagnostic(value: &str) -> String {
    let mut end = value.len().min(MAX_DIAGNOSTIC_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
fn charge(realm: &mut BoaPageRealm, bytes: usize) -> Result<(), String> {
    let result = realm.state.borrow_mut().host.charge(bytes);
    result.map_err(|_| realm.engine.latch_failure(STORAGE_FAILURE).to_string())
}
fn reserve<T>(realm: &mut BoaPageRealm, values: &mut Vec<T>, extra: usize) -> Result<(), String> {
    let result = reserve_storage(&mut realm.state.borrow_mut().host, values, extra);
    result.map_err(|_| realm.engine.latch_failure(STORAGE_FAILURE).to_string())
}
fn text(realm: &mut BoaPageRealm, value: &JsString) -> Result<Vec<u16>, String> {
    if value.len() > MAX_STRING_UNITS {
        return Err("Runtime string limit exceeded".into());
    }
    let mut units = Vec::new();
    reserve(realm, &mut units, value.len())?;
    units.extend(value.iter());
    Ok(units)
}
fn clone_group(realm: &mut BoaPageRealm, group: Option<&str>) -> Result<Option<String>, String> {
    group
        .map(|group| {
            charge(realm, group.len())?;
            let result = group.to_owned();
            charge(realm, result.capacity().saturating_sub(group.len()))?;
            Ok(result)
        })
        .transpose()
}
fn registry(realm: &mut BoaPageRealm) -> Result<Gc<GcRefCell<Registry>>, String> {
    if let Some(registry) = &realm.automation_registry {
        return Ok(registry.clone());
    }
    charge(realm, size_of::<Registry>())?;
    let registry = Gc::new(GcRefCell::new(Registry::default()));
    realm.automation_registry = Some(registry.clone());
    Ok(registry)
}
fn lookup(registry: &Registry, owner: u64, id: u64) -> Result<&Root, String> {
    let root = usize::try_from(id)
        .ok()
        .and_then(|id| id.checked_sub(1))
        .and_then(|i| registry.roots.get(i))
        .ok_or("Unknown Runtime object")?;
    if root.owner != owner {
        return Err("Foreign Runtime object owner".into());
    }
    if !root.published {
        return Err("Unpublished Runtime object".into());
    }
    Ok(root)
}
fn live(registry: &Registry, owner: u64, id: u64) -> Result<JsObject, String> {
    lookup(registry, owner, id)?
        .value
        .clone()
        .ok_or("Released Runtime object".into())
}
fn owner_preflight(registry: &Registry, owner: u64) -> Result<(), String> {
    if let Some((_, active)) = registry.owners.iter().find(|(id, _)| *id == owner) {
        return if *active {
            Ok(())
        } else {
            Err("Revoked Runtime owner".into())
        };
    }
    if registry.owners.len() >= MAX_OWNER_IDENTITIES
        || registry.owners.iter().filter(|(_, active)| *active).count() >= MAX_OWNERS
    {
        return Err("Runtime owner limit exceeded".into());
    }
    Ok(())
}
fn ensure_owner(
    realm: &mut BoaPageRealm,
    registry: &Gc<GcRefCell<Registry>>,
    owner: u64,
) -> Result<(), String> {
    owner_preflight(&registry.borrow(), owner)?;
    if !registry.borrow().owners.iter().any(|(id, _)| *id == owner) {
        let mut data = registry.borrow_mut();
        reserve(realm, &mut data.owners, 1)?;
        data.owners.push((owner, true));
    }
    Ok(())
}
fn kind(realm: &mut BoaPageRealm, object: &JsObject) -> ObjectKind {
    if object.is_callable() {
        ObjectKind::Function
    } else if object.is::<modern::boa_engine::builtins::proxy::Proxy>() {
        let brands = realm.state.borrow().brands.clone();
        if let Ok(val) = brands.get(object, realm.engine.context_mut())
            && let Some(s) = val.as_string()
        {
            let h = s.to_std_string_escaped();
            if h == "document" {
                return ObjectKind::Node { node: 0 };
            }
            if let Some(id_str) = h.strip_prefix("node:")
                && let Ok(node) = id_str.parse::<usize>()
            {
                return ObjectKind::Node { node };
            }
        }
        ObjectKind::Proxy
    } else if object.is_array() {
        ObjectKind::Array
    } else if object.is::<Promise>() {
        ObjectKind::Promise
    } else if object.is::<modern::boa_engine::builtins::error::Error>() {
        ObjectKind::Error
    } else if object.is_ordinary() {
        ObjectKind::Object
    } else {
        ObjectKind::Other
    }
}

fn safe_thrown_diagnostic(value: &JsValue, fallback: &str) -> String {
    if let Some(s) = value.as_string() {
        return diagnostic(&s.to_std_string_escaped());
    }
    if let Some(obj) = value.as_object()
        && obj.is::<modern::boa_engine::builtins::error::Error>()
        && let Ok(desc) = descriptor(&obj, &js_string!("message").into())
        && let Some(msg_val) = desc.value()
        && let Some(msg_str) = msg_val.as_string()
    {
        let msg = msg_str.to_std_string_escaped();
        return diagnostic(&format!("Error: {msg}"));
    }
    diagnostic(fallback)
}
fn inspectable(object: &JsObject) -> Result<(), String> {
    if object.is_ordinary() || object.is_array() {
        Ok(())
    } else {
        Err("Runtime inspection requires an ordinary object or array".into())
    }
}
fn keys(realm: &mut BoaPageRealm, object: &JsObject) -> Result<Vec<PropertyKey>, String> {
    inspectable(object)?;
    realm.engine.check().map_err(|e| e.to_string())?;
    let result = object.own_property_keys(realm.engine.context_mut());
    realm.engine.check().map_err(|e| e.to_string())?;
    let result = result.map_err(|_| "Runtime own-key enumeration failed")?;
    charge(
        realm,
        result
            .capacity()
            .checked_mul(size_of::<PropertyKey>())
            .ok_or("Runtime key capacity overflow")?,
    )?;
    if result
        .iter()
        .any(|key| matches!(key, PropertyKey::Symbol(_)))
    {
        return Err("Runtime symbol keys are unsupported".into());
    }
    Ok(result)
}
fn descriptor(
    object: &JsObject,
    key: &PropertyKey,
) -> Result<modern::boa_engine::property::PropertyDescriptor, String> {
    object
        .try_borrow()
        .map_err(|_| "Runtime object borrow conflict")?
        .properties()
        .get(key)
        .ok_or("Runtime own descriptor unavailable".into())
}
fn key_text(realm: &mut BoaPageRealm, key: &PropertyKey) -> Result<Vec<u16>, String> {
    match key {
        PropertyKey::String(s) => text(realm, s),
        PropertyKey::Index(index) => text(realm, &JsString::from(index.get().to_string())),
        PropertyKey::Symbol(_) => Err("Runtime symbol keys are unsupported".into()),
    }
}

// The worker wire remains lossless UTF-16. The current external CDP codec is
// narrower: admit its text before publishing roots, without getters or coercion.
// Call only after typed validation has bounded every tree, string and descriptor.
fn external_text(text: &[u16]) -> Result<(), &'static str> {
    if char::decode_utf16(text.iter().copied()).all(|unit| unit.is_ok()) {
        Ok(())
    } else {
        Err("Runtime external UTF-16 contains an unpaired surrogate")
    }
}
fn external_tree(value: &SerializedValue) -> Result<(), &'static str> {
    match value {
        SerializedValue::String(text) => external_text(text)?,
        SerializedValue::Array(values) => {
            for value in values {
                external_tree(value)?;
            }
        }
        SerializedValue::Object(properties) => {
            for (key, value) in properties {
                external_text(key)?;
                external_tree(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn external_value(value: &RemoteValue) -> Result<(), &'static str> {
    match value {
        RemoteValue::String(text) => external_text(text),
        RemoteValue::ByValue(tree) => external_tree(tree),
        _ => Ok(()),
    }
}
fn external_result(result: &AutomationResult) -> Result<(), &'static str> {
    match result {
        AutomationResult::Value(value)
        | AutomationResult::Exception {
            exception: Some(value),
            ..
        } => external_value(value)?,
        AutomationResult::Properties(properties) => {
            for property in properties {
                external_text(&property.name)?;
                match &property.descriptor {
                    wire::PropertyDescriptor::Data { value, .. } => external_value(value)?,
                    wire::PropertyDescriptor::Accessor { get, set } => {
                        for value in [get, set].into_iter().flatten() {
                            external_value(value)?;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Newly returned objects remain staged external roots until the complete result
/// passes encoded admission. No partial property/exception handle can escape.
struct Export<'a> {
    realm: &'a mut BoaPageRealm,
    registry: Gc<GcRefCell<Registry>>,
    owner: u64,
    group: Option<String>,
    pending: Vec<(usize, Root)>,
}
impl Export<'_> {
    fn root(&mut self, object: JsObject) -> Result<RemoteValue, String> {
        let data = self.registry.borrow();
        let base = data.roots.len();
        if self.pending.len() >= MAX_RESULT_ROOTS
            || base >= MAX_ISSUED
            || data.roots.iter().filter(|r| r.value.is_some()).count() + self.pending.len()
                >= MAX_ROOTS
        {
            return Err("Runtime root limit exceeded".into());
        }
        if let Some(group) = self.group.as_deref() {
            let exists = data
                .roots
                .iter()
                .chain(self.pending.iter().map(|(_, root)| root))
                .any(|r| {
                    r.value.is_some() && r.owner == self.owner && r.group.as_deref() == Some(group)
                });
            if !exists {
                let mut count = 0;
                for (i, r) in data.roots.iter().enumerate() {
                    if r.value.is_some()
                        && r.group.is_some()
                        && !data.roots[..i].iter().any(|old| {
                            old.value.is_some() && old.owner == r.owner && old.group == r.group
                        })
                    {
                        count += 1;
                    }
                }
                if count >= MAX_GROUPS {
                    return Err("Runtime group limit exceeded".into());
                }
            }
        }
        let id = (base + 1) as u64;
        drop(data);
        let active_world = self.realm.state.borrow().active_world;
        let kind = kind(self.realm, &object);
        let group = clone_group(self.realm, self.group.as_deref())?;
        reserve(self.realm, &mut self.pending, 1)?;
        // Reserve a permanent scalar tombstone before retaining the root. Failed
        // result admission drops all staged values but never refunds an ID.
        let mut data = self.registry.borrow_mut();
        reserve(self.realm, &mut data.roots, 1)?;
        data.roots.push(Root {
            owner: self.owner,
            published: false,
            world_id: 0,
            group: None,
            value: None,
        });
        drop(data);
        self.pending.push((
            base,
            Root {
                owner: self.owner,
                published: true,
                world_id: active_world,
                group,
                value: Some(object),
            },
        ));
        Ok(RemoteValue::Object { object: id, kind })
    }
    fn value(&mut self, value: &JsValue, by_value: bool) -> Result<RemoteValue, String> {
        match value.variant() {
            JsVariant::Undefined => Ok(RemoteValue::Undefined),
            JsVariant::Null => Ok(RemoteValue::Null),
            JsVariant::Boolean(v) => Ok(RemoteValue::Boolean(v)),
            JsVariant::Integer32(v) => Ok(RemoteValue::Number(Number::Finite(f64::from(v)))),
            JsVariant::Float64(v) => Ok(RemoteValue::Number(Number::from_f64(v))),
            JsVariant::String(v) => Ok(RemoteValue::String(text(self.realm, &v)?)),
            JsVariant::BigInt(v) => {
                if v.as_inner().bits() > 4096 {
                    return Err("Runtime BigInt limit exceeded".into());
                }
                charge(self.realm, 1235)?;
                let text = v.to_string();
                charge(self.realm, text.capacity().saturating_sub(1235))?;
                Ok(RemoteValue::BigInt(text))
            }
            JsVariant::Symbol(_) => Err("Runtime Symbols are unsupported".into()),
            JsVariant::Object(object) => {
                if by_value {
                    let mut tree = Tree::default();
                    Ok(RemoteValue::ByValue(tree.value(self.realm, value, 0)?))
                } else {
                    self.root(object)
                }
            }
        }
    }
    fn properties(&mut self, object: &JsObject) -> Result<AutomationResult, String> {
        let keys = keys(self.realm, object)?;
        if keys.len() > MAX_PROPERTIES {
            return Err("Runtime descriptor count exceeded".into());
        }
        let mut result = Vec::new();
        reserve(self.realm, &mut result, keys.len())?;
        for key in keys {
            let desc = descriptor(object, &key)?;
            let descriptor = if let Some(value) = desc.value() {
                wire::PropertyDescriptor::Data {
                    value: self.value(value, false)?,
                    writable: desc.writable().ok_or("Incomplete Runtime descriptor")?,
                }
            } else {
                let mut accessor =
                    |value: Option<&JsValue>| -> Result<Option<RemoteValue>, String> {
                        let Some(value) = value else {
                            return Err("Incomplete Runtime accessor".into());
                        };
                        if value.is_undefined() {
                            return Ok(None);
                        }
                        if !value.as_object().is_some_and(|o| o.is_callable()) {
                            return Err("Invalid Runtime accessor".into());
                        }
                        self.value(value, false).map(Some)
                    };
                wire::PropertyDescriptor::Accessor {
                    get: accessor(desc.get())?,
                    set: accessor(desc.set())?,
                }
            };
            result.push(OwnProperty {
                name: key_text(self.realm, &key)?,
                enumerable: desc.enumerable().ok_or("Incomplete Runtime descriptor")?,
                configurable: desc.configurable().ok_or("Incomplete Runtime descriptor")?,
                descriptor,
            });
        }
        Ok(AutomationResult::Properties(result))
    }
    fn commit(
        mut self,
        mut result: AutomationResult,
        request: &AutomationRequest,
    ) -> Result<AutomationResult, String> {
        result.validate_for(request)?;
        if let Err(reason) = external_result(&result) {
            if let AutomationResult::Exception {
                diagnostic: message,
                exception,
            } = &mut result
            {
                self.pending.clear();
                *message = diagnostic(&format!(
                    "Exception representation unavailable ({reason}): {message}"
                ));
                *exception = None;
                result.validate_for(request)?;
            } else {
                return Err(reason.into());
            }
        }
        let mut registry = self.registry.borrow_mut();
        for (index, root) in self.pending.drain(..) {
            registry.roots[index] = root;
        }
        Ok(result)
    }
}

#[derive(Default)]
struct Tree {
    entries: usize,
    units: usize,
    path: Vec<JsObject>,
}
impl Tree {
    fn text(&mut self, realm: &mut BoaPageRealm, value: &JsString) -> Result<Vec<u16>, String> {
        self.units = self
            .units
            .checked_add(value.len())
            .ok_or("Runtime tree overflow")?;
        if self.units > MAX_TREE_UNITS {
            return Err("Runtime tree string limit exceeded".into());
        }
        text(realm, value)
    }
    fn value(
        &mut self,
        realm: &mut BoaPageRealm,
        value: &JsValue,
        depth: usize,
    ) -> Result<SerializedValue, String> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES || depth > MAX_DEPTH {
            return Err("Runtime tree limit exceeded".into());
        }
        match value.variant() {
            JsVariant::Null => Ok(SerializedValue::Null),
            JsVariant::Boolean(v) => Ok(SerializedValue::Boolean(v)),
            JsVariant::Integer32(v) => Ok(SerializedValue::FiniteNumber(f64::from(v))),
            JsVariant::Float64(v) => {
                Number::Finite(v).validate()?;
                Ok(SerializedValue::FiniteNumber(v))
            }
            JsVariant::String(v) => Ok(SerializedValue::String(self.text(realm, &v)?)),
            JsVariant::Object(object) => {
                inspectable(&object)?;
                if self.path.contains(&object) {
                    return Err("Runtime cyclic by-value result is unsupported".into());
                }
                reserve(realm, &mut self.path, 1)?;
                self.path.push(object.clone());
                let keys = keys(realm, &object)?;
                let result = if object.is_array() {
                    let length = descriptor(&object, &js_string!("length").into())?
                        .value()
                        .and_then(JsValue::as_number)
                        .ok_or("Invalid Runtime array length")?;
                    if length < 0.0 || length > MAX_ENTRIES as f64 || length.fract() != 0.0 {
                        return Err("Runtime array length limit exceeded".into());
                    }
                    let length = length as usize;
                    // Reject non-index enumerable properties rather than silently dropping them.
                    for key in &keys {
                        if descriptor(&object, key)?.enumerable() == Some(true)
                            && !matches!(key,PropertyKey::Index(index) if (index.get() as usize)<length)
                        {
                            return Err("Runtime array extra properties are unsupported".into());
                        }
                    }
                    let mut values = Vec::new();
                    reserve(realm, &mut values, length)?;
                    for i in 0..length {
                        let desc = descriptor(&object, &PropertyKey::from(i as u32))?;
                        let value = desc
                            .value()
                            .ok_or("Runtime array accessor is unsupported")?;
                        values.push(self.value(realm, value, depth + 1)?);
                    }
                    SerializedValue::Array(values)
                } else {
                    let mut values = Vec::new();
                    for key in keys {
                        let desc = descriptor(&object, &key)?;
                        if desc.enumerable() != Some(true) {
                            continue;
                        }
                        let value = desc
                            .value()
                            .ok_or("Runtime by-value accessor is unsupported")?;
                        let key = key_text(realm, &key)?;
                        self.units = self
                            .units
                            .checked_add(key.len())
                            .ok_or("Runtime tree overflow")?;
                        if self.units > MAX_TREE_UNITS {
                            return Err("Runtime tree string limit exceeded".into());
                        }
                        let value = self.value(realm, value, depth + 1)?;
                        reserve(realm, &mut values, 1)?;
                        values.push((key, value));
                    }
                    SerializedValue::Object(values)
                };
                self.path.pop();
                Ok(result)
            }
            _ => Err("Runtime nested value is unsupported".into()),
        }
    }
}

fn restore(realm: &mut BoaPageRealm, value: &SerializedValue) -> Result<JsValue, String> {
    realm.engine.check().map_err(|e| e.to_string())?;
    match value {
        SerializedValue::Null => Ok(JsValue::null()),
        SerializedValue::Boolean(v) => Ok((*v).into()),
        SerializedValue::FiniteNumber(v) => Ok((*v).into()),
        SerializedValue::String(v) => {
            charge(realm, v.len() * 2)?;
            Ok(JsString::from(v.as_slice()).into())
        }
        SerializedValue::Array(values) => {
            let mut restored = Vec::new();
            reserve(realm, &mut restored, values.len())?;
            for value in values {
                restored.push(restore(realm, value)?);
            }
            Ok(JsArray::from_iter(restored, realm.engine.context_mut()).into())
        }
        SerializedValue::Object(values) => {
            let object = JsObject::with_object_proto(realm.engine.context().intrinsics());
            for (key, value) in values {
                charge(realm, key.len() * 2)?;
                let key = JsString::from(key.as_slice());
                let value = restore(realm, value)?;
                object
                    .create_data_property_or_throw(key, value, realm.engine.context_mut())
                    .map_err(|_| "Runtime argument property construction failed")?;
            }
            Ok(object.into())
        }
    }
}

fn perform(
    realm: &mut BoaPageRealm,
    request: &AutomationRequest,
    registry: Gc<GcRefCell<Registry>>,
    errors: &mut Vec<String>,
) -> Result<AutomationResult, String> {
    match &request.operation {
        Operation::ReleaseOwners { owners } => {
            let mut data = registry.borrow_mut();
            let fresh = owners
                .iter()
                .filter(|id| !data.owners.iter().any(|(old, _)| old == *id))
                .count();
            if data.owners.len() + fresh > MAX_OWNER_IDENTITIES {
                return Err("Runtime owner history limit exceeded".into());
            }
            reserve(realm, &mut data.owners, fresh)?;
            // Unknown broker-issued owners are retained as inactive identities;
            // cleanup cannot allow their later reactivation or refund history.
            for owner in owners {
                if let Some((_, active)) = data.owners.iter_mut().find(|(id, _)| id == owner) {
                    *active = false;
                } else {
                    data.owners.push((*owner, false));
                }
                for root in &mut data.roots {
                    if root.owner == *owner {
                        root.value = None;
                        root.group = None;
                    }
                }
            }
            return Ok(AutomationResult::Released);
        }
        Operation::ReleaseObject { object } => {
            let mut data = registry.borrow_mut();
            lookup(&data, request.owner, *object)?;
            let root = &mut data.roots[*object as usize - 1];
            root.value = None;
            root.group = None;
            return Ok(AutomationResult::Released);
        }
        Operation::ReleaseGroup { group } => {
            for root in &mut registry.borrow_mut().roots {
                if root.owner == request.owner && root.group.as_ref() == Some(group) {
                    root.value = None;
                    root.group = None;
                }
            }
            return Ok(AutomationResult::Released);
        }
        Operation::CreateIsolatedWorld { world } => {
            realm.ensure_isolated_world(world)?;
            return Ok(AutomationResult::Released);
        }
        Operation::ResolveNode { node, world, group } => {
            let group = clone_group(realm, group.as_deref())?;
            let wid = realm.resolve_world_id(world.as_deref())?;
            let exists = *node < realm.state.borrow().host.document.nodes.len();
            if !exists {
                return Ok(AutomationResult::Value(RemoteValue::Null));
            }
            let prev_world = super::enter_world_in_context(wid, realm.engine.context_mut());
            let handle = if *node == 0 {
                "document".to_owned()
            } else {
                format!("node:{node}")
            };
            let wrapped = super::wrap(&handle, realm.engine.context_mut())
                .map_err(|_| "Runtime DOM node resolution failed".to_string());
            let res = match wrapped {
                Ok(val) => {
                    let mut export = Export {
                        realm,
                        registry,
                        owner: request.owner,
                        group,
                        pending: Vec::new(),
                    };
                    let rv = export.value(&val, false)?;
                    export.commit(AutomationResult::Value(rv), request)
                }
                Err(e) => Err(e),
            };
            super::enter_world_in_context(prev_world, realm.engine.context_mut());
            return res;
        }
        _ => {}
    }
    let (group, by_value) = match &request.operation {
        Operation::Evaluate {
            group,
            return_by_value,
            ..
        } => (clone_group(realm, group.as_deref())?, *return_by_value),
        Operation::CallFunction {
            group,
            receiver,
            return_by_value,
            ..
        } => {
            let data = registry.borrow();
            let inherited = receiver
                .map(|id| lookup(&data, request.owner, id))
                .transpose()?
                .and_then(|root| root.group.as_deref());
            (
                clone_group(realm, group.as_deref().or(inherited))?,
                *return_by_value,
            )
        }
        Operation::GetOwnProperties { object } => {
            let data = registry.borrow();
            (
                clone_group(
                    realm,
                    lookup(&data, request.owner, *object)?.group.as_deref(),
                )?,
                false,
            )
        }
        _ => unreachable!(),
    };
    if let Operation::GetOwnProperties { object } = request.operation {
        let (object, wid) = {
            let data = registry.borrow();
            let root = lookup(&data, request.owner, object)?;
            (
                root.value.clone().ok_or("Released Runtime object")?,
                root.world_id,
            )
        };
        let prev_world = super::enter_world_in_context(wid, realm.engine.context_mut());
        let res = (|| {
            let mut export = Export {
                realm,
                registry,
                owner: request.owner,
                group,
                pending: Vec::new(),
            };
            let result = export.properties(&object)?;
            export.commit(result, request)
        })();
        super::enter_world_in_context(prev_world, realm.engine.context_mut());
        return res;
    }
    let (wid, await_promise) = match &request.operation {
        Operation::Evaluate {
            world,
            await_promise,
            ..
        } => (realm.resolve_world_id(world.as_deref())?, *await_promise),
        Operation::CallFunction {
            world,
            receiver,
            arguments,
            await_promise,
            ..
        } => {
            let wid = if world.is_some() {
                realm.resolve_world_id(world.as_deref())?
            } else {
                let data = registry.borrow();
                if let Some(id) = receiver {
                    lookup(&data, request.owner, *id)?.world_id
                } else if let Some(id) = arguments.iter().find_map(|a| match a {
                    Argument::Object(id) => Some(*id),
                    _ => None,
                }) {
                    lookup(&data, request.owner, id)?.world_id
                } else {
                    0
                }
            };
            (wid, *await_promise)
        }
        _ => unreachable!(),
    };
    realm.state.borrow_mut().error_exposure_allowed = false;
    if realm.purge_private_notices().is_err() {
        realm.rejection_failure(&mut Vec::new());
        return Err(REJECTION_FAILURE.into());
    }
    let _prev_world = super::enter_world_in_context(wid, realm.engine.context_mut());
    let mut not_callable = false;
    let evaluated = {
        let _ingress = Ingress::enter(realm.state.clone());
        match &request.operation {
            Operation::Evaluate { source, .. } => realm.engine.evaluate_raw(source),
            Operation::CallFunction {
                source,
                receiver,
                arguments,
                ..
            } => {
                let receiver = receiver
                    .map(|id| live(&registry.borrow(), request.owner, id))
                    .transpose()?
                    .unwrap_or_else(|| realm.engine.context().global_object());
                let mut values = Vec::new();
                reserve(realm, &mut values, arguments.len())?;
                for argument in arguments {
                    values.push(match argument {
                        Argument::Undefined => JsValue::undefined(),
                        Argument::Number(n) => n.as_f64().into(),
                        Argument::BigInt(n) => {
                            charge(realm, n.len())?;
                            JsBigInt::from_string(n)
                                .ok_or("Invalid Runtime BigInt argument")?
                                .into()
                        }
                        Argument::Value(v) => restore(realm, v)?,
                        Argument::Object(id) => {
                            live(&registry.borrow(), request.owner, *id)?.into()
                        }
                    });
                }
                charge(realm, source.len() + 4)?;
                let wrapper = format!("({source}\n)");
                charge(realm, wrapper.capacity().saturating_sub(source.len() + 4))?;
                match realm.engine.evaluate_raw(&wrapper) {
                    Ok(value) => {
                        if let Some(function) = value.as_object().filter(|f| f.is_callable()) {
                            realm.engine.call_raw(&function, &receiver.into(), &values)
                        } else {
                            not_callable = true;
                            Ok(JsValue::undefined())
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            _ => unreachable!(),
        }
    };
    // Keep the actual result/exception rooted across the ordinary checkpoint.
    // Runtime's caught exception is deliberately not sent to execution_failure.
    realm.checkpoint(errors);
    super::enter_world_in_context(wid, realm.engine.context_mut());
    realm.engine.check().map_err(|e| e.to_string())?;
    if not_callable {
        return Err("Runtime function source did not evaluate to a callable".into());
    }
    let mut evaluated = evaluated;
    if await_promise {
        if let Ok(val) = &evaluated
            && let Some(obj) = val.as_object()
            && let Ok(js_promise) = JsPromise::from_object(obj.clone())
        {
            let mut steps = 0;
            while matches!(js_promise.state(), PromiseState::Pending) && steps < 16 {
                steps += 1;
                if !realm.step_immediate_timer_for_await(errors) {
                    break;
                }
                realm.engine.check().map_err(|e| e.to_string())?;
            }
            super::enter_world_in_context(wid, realm.engine.context_mut());
            evaluated = match js_promise.state() {
                PromiseState::Fulfilled(v) => Ok(v),
                PromiseState::Rejected(reason) => {
                    let js_error = JsError::from_opaque(reason);
                    Err(modern::ExecutionFailure {
                        diagnostic: modern::safe_error(&js_error),
                        exception: Some(js_error),
                    })
                }
                PromiseState::Pending => {
                    return Err("Awaited Promise did not settle".into());
                }
            };
        }
    }
    let mut export = Export {
        realm,
        registry,
        owner: request.owner,
        group,
        pending: Vec::new(),
    };
    let result = match evaluated {
        Ok(value) => AutomationResult::Value(export.value(&value, by_value)?),
        Err(failure) => {
            let mut message = diagnostic(&failure.diagnostic.to_string());
            let exception = match failure.exception {
                Some(error) => {
                    let opaque = error
                        .into_opaque(export.realm.engine.context_mut())
                        .map_err(|_| "Runtime exception conversion failed".to_string());
                    if let Ok(ref val) = opaque
                        && failure.diagnostic.kind == "ThrownValue"
                    {
                        message = safe_thrown_diagnostic(val, &message);
                    }
                    let represented = opaque.and_then(|value| export.value(&value, false));
                    match represented {
                        Ok(value) => Some(value),
                        Err(reason) => {
                            export.realm.engine.check().map_err(|e| e.to_string())?;
                            export.pending.clear();
                            message = diagnostic(&format!(
                                "Exception representation unavailable ({reason}): {message}"
                            ));
                            None
                        }
                    }
                }
                None => None,
            };
            AutomationResult::Exception {
                diagnostic: message,
                exception,
            }
        }
    };
    export.commit(result, request)
}

pub(super) fn run(
    realm: &mut BoaPageRealm,
    mut request: AutomationRequest,
) -> (SessionReply, AutomationResult) {
    let mut reply = BoaPageRealm::empty_reply();
    reply.revision = realm.revision;
    let preflight = (|| {
        request.validate()?;
        if realm.closed || realm.engine.is_fatal() {
            return Err("Page realm is no longer active".into());
        }
        if realm.lifecycle.phase != LifecyclePhase::LoadComplete
            || realm.state.borrow().observer.is_some()
        {
            return Err("Runtime requires a loaded ordinary page realm".into());
        }
        // Reuse the exact existing edit validator, but never deliver its synthetic
        // validation-only root target as an activation or expose an outcome.
        let input = SessionInput {
            kind: InputKind::Click { target: 0 },
            edits: std::mem::take(&mut request.edits),
        };
        let valid = realm.state.borrow().host.validate_input(&input);
        request.edits = input.edits;
        valid?;
        Ok::<(), String>(())
    })();
    if let Err(error) = preflight {
        reply.boa = Some(realm.engine.stats());
        reply.state = realm.realm_state();
        reply.lifecycle = Some(realm.lifecycle);
        if realm.closed || realm.engine.is_fatal() {
            realm.clear_work();
            reply.boa = Some(realm.engine.stats());
        }
        return (reply, rejected(error));
    }
    let semantic_preflight = (|| {
        if let Some(data) = &realm.automation_registry {
            let data = data.borrow();
            if request.owner != 0 {
                owner_preflight(&data, request.owner)?;
            }
            match &request.operation {
                Operation::ReleaseOwners { owners } => {
                    let fresh = owners
                        .iter()
                        .filter(|id| !data.owners.iter().any(|(old, _)| old == *id))
                        .count();
                    if data.owners.len() + fresh > MAX_OWNER_IDENTITIES {
                        return Err("Runtime owner history limit exceeded".into());
                    }
                }
                Operation::GetOwnProperties { object } => {
                    live(&data, request.owner, *object)?;
                }
                Operation::ReleaseObject { object } => {
                    lookup(&data, request.owner, *object)?;
                }
                Operation::CallFunction {
                    receiver,
                    arguments,
                    ..
                } => {
                    if let Some(id) = receiver {
                        live(&data, request.owner, *id)?;
                    }
                    for argument in arguments {
                        if let Argument::Object(id) = argument {
                            live(&data, request.owner, *id)?;
                        }
                    }
                }
                _ => {}
            }
        } else if matches!(
            &request.operation,
            Operation::GetOwnProperties { .. }
                | Operation::ReleaseObject { .. }
                | Operation::CallFunction {
                    receiver: Some(_),
                    ..
                }
        ) || matches!(&request.operation,Operation::CallFunction{arguments,..} if arguments.iter().any(|a|matches!(a,Argument::Object(_))))
        {
            return Err("Unknown Runtime object".into());
        }
        Ok::<(), String>(())
    })();
    realm.revision += 1;
    reply.revision = realm.revision;
    {
        let mut data = realm.state.borrow_mut();
        data.armed_wake = None;
        data.host.navigation = None;
    }
    let result = (|| {
        for edit in &request.edits {
            let mut data = realm.state.borrow_mut();
            if data.host.document.nodes[edit.node].tag == "textarea" {
                data.host.replace_text(edit.node, edit.value.clone())?;
            } else {
                data.host.attr(edit.node, "value", edit.value.clone())?;
            }
            data.dom_epoch = data.dom_epoch.wrapping_add(1);
            reply.acknowledgements.push(ControlAck {
                node: edit.node,
                version: edit.version,
            });
        }
        semantic_preflight?;
        let registry = registry(realm)?;
        if request.owner != 0 {
            ensure_owner(realm, &registry, request.owner)?;
        }
        perform(realm, &request, registry, &mut reply.errors)
    })();
    super::enter_world_in_context(0, realm.engine.context_mut());
    let mut result = result.unwrap_or_else(rejected);
    realm.snapshot(&mut reply);
    if reply.state != RealmState::Ready {
        result = rejected("Runtime realm terminated");
    }
    reply.errors.truncate(64);
    (reply, result)
}

pub(super) fn clear(registry: &Gc<GcRefCell<Registry>>) {
    let mut data = registry.borrow_mut();
    for root in &mut data.roots {
        root.value = None;
        root.group = None;
    }
    for (_, active) in &mut data.owners {
        *active = false;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::js_browser::Request;
    use crate::page_session::automation::PropertyDescriptor;
    use crate::page_session::{LifecyclePhase, RealmState, TaskPump};

    fn loaded(source: &str) -> super::super::BoaPageRealm {
        let (realm, mut reply) = super::super::BoaPageRealm::start(Request {
            url: "https://example.test/runtime".into(),
            html: format!("<title>before</title><input id='input'><script>{source}</script>"),
            external_scripts: None,
        });
        let mut realm = realm.unwrap();
        while reply.lifecycle.unwrap().phase != LifecyclePhase::LoadComplete {
            assert_eq!(reply.state, RealmState::Ready);
            reply = realm.pump_tasks(TaskPump {
                wake_token: reply.task_wake.unwrap().token,
            });
        }
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        realm
    }
    fn eval(
        realm: &mut super::super::BoaPageRealm,
        source: &str,
        by_value: bool,
    ) -> AutomationResult {
        let (reply, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: source.into(),
                return_by_value: by_value,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
        result
    }
    fn object(result: AutomationResult) -> u64 {
        match result {
            AutomationResult::Value(RemoteValue::Object { object, .. }) => object,
            other => panic!("real object expected: {other:?}"),
        }
    }
    #[test]
    fn arbitrary_globals_closures_receiver_aliases_and_gc_are_real() {
        let mut realm = loaded("");
        let id = object(eval(
            &mut realm,
            "globalThis.weak = null; (()=>{let x={n:7}; weak=new WeakRef(x); return x})()",
            false,
        ));
        modern::boa_gc::force_collect();
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CallFunction {
                source: "function(x){document.title='changed'; return this===x && this.n===7 && weak.deref()===this}".into(),
                receiver: Some(id),
                arguments: vec![Argument::Object(id)],
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(result, AutomationResult::Value(RemoteValue::Boolean(true)));
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::ReleaseObject { object: id },
            edits: vec![],
        });
        assert_eq!(result, AutomationResult::Released);
        eval(&mut realm, "0", false);
        modern::boa_gc::force_collect();
        assert_eq!(
            eval(&mut realm, "weak.deref()===undefined", false),
            AutomationResult::Value(RemoteValue::Boolean(true))
        );
    }
    #[test]
    fn descriptors_do_not_run_getters_or_proxy_traps_and_admission_is_atomic() {
        let mut realm = loaded("globalThis.traps=0");
        let id = object(eval(
            &mut realm,
            "Object.defineProperty({x:1},'secret',{get(){traps++;return 99},enumerable:false})",
            false,
        ));
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::GetOwnProperties { object: id },
            edits: vec![],
        });
        let AutomationResult::Properties(properties) = result else {
            panic!("descriptors required")
        };
        assert_eq!(properties.len(), 2);
        assert!(matches!(
            properties[1].descriptor,
            PropertyDescriptor::Accessor {
                get: Some(RemoteValue::Object {
                    kind: ObjectKind::Function,
                    ..
                }),
                set: None
            }
        ));
        assert_eq!(
            eval(&mut realm, "traps", false),
            AutomationResult::Value(RemoteValue::Number(Number::Finite(0.0)))
        );
        let proxy = object(eval(
            &mut realm,
            "new Proxy({},{ownKeys(){traps++;return []}})",
            false,
        ));
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::GetOwnProperties { object: proxy },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(
            eval(&mut realm, "traps", false),
            AutomationResult::Value(RemoteValue::Number(Number::Finite(0.0)))
        );
        assert!(matches!(
            eval(&mut realm, "({get x(){traps++;return 1}})", true),
            AutomationResult::Rejected { .. }
        ));
    }
    #[test]
    fn caught_exception_has_effects_without_window_error_and_privacy_is_sticky() {
        let mut realm = loaded("globalThis.errors=0; window.onerror=()=>{errors++}");
        let result = eval(
            &mut realm,
            "document.title='effect';throw {get message(){errors+=100}}",
            false,
        );
        assert!(matches!(result, AutomationResult::Exception { .. }));
        assert_eq!(
            eval(&mut realm, "errors", false),
            AutomationResult::Value(RemoteValue::Number(Number::Finite(0.0)))
        );
        assert!(!realm.state.borrow().error_exposure_allowed);
        assert!(!realm.engine.dynamic_compilation_seen());
    }
    #[test]
    fn owners_groups_tombstones_and_unsupported_trees() {
        let mut realm = loaded("");
        let id = object(eval(&mut realm, "({})", false));
        let (_, result) = realm.automation(AutomationRequest {
            owner: 2,
            operation: Operation::ReleaseObject { object: id },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        for _ in 0..2 {
            let (_, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::ReleaseObject { object: id },
                edits: vec![],
            });
            assert_eq!(result, AutomationResult::Released);
        }
        for source in [
            "Symbol()",
            "[,1]",
            "(()=>{let a={};a.a=a;return a})()",
            "({n:NaN})",
            "({x:undefined})",
            "({x:-0})",
        ] {
            assert!(
                matches!(
                    eval(&mut realm, source, true),
                    AutomationResult::Rejected { .. }
                ),
                "{source}"
            );
        }
        assert_eq!(
            eval(&mut realm, "-0", false),
            AutomationResult::Value(RemoteValue::Number(Number::NegativeZero))
        );
        assert_eq!(
            eval(&mut realm, "123n", false),
            AutomationResult::Value(RemoteValue::BigInt("123".into()))
        );
    }

    #[test]
    fn lazy_registry_preflight_and_post_checkpoint_values() {
        let mut realm = loaded("");
        assert!(
            realm.automation_registry.is_none(),
            "ordinary bootstrap pays no registry reserve"
        );
        let before = realm.engine.report();
        let revision = realm.revision;
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: " ".repeat(MAX_SOURCE_BYTES + 1),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(realm.engine.report(), before);
        assert_eq!(realm.revision, revision);
        assert!(realm.state.borrow().error_exposure_allowed);
        assert!(realm.automation_registry.is_none());
        assert_eq!(
            eval(
                &mut realm,
                "let result={n:1};Promise.resolve().then(()=>result.n=2);result",
                true
            ),
            AutomationResult::Value(RemoteValue::ByValue(SerializedValue::Object(vec![(
                vec![110],
                SerializedValue::FiniteNumber(2.0)
            )])))
        );
    }

    #[test]
    fn full_argument_domain_global_receiver_and_group_inheritance() {
        let mut realm = loaded("");
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "({n:7})".into(),
                return_by_value: false,
                group: Some("".into()),
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        let parent = object(result);
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CallFunction {
                source: "function(a,b,c,d){if(this.n!==7||a!==undefined||!Object.is(b,-0)||c!==Infinity||d!==9n)throw 'bad';return {child:this}}".into(),
                receiver: Some(parent),
                arguments: vec![
                    Argument::Undefined,
                    Argument::Number(Number::NegativeZero),
                    Argument::Number(Number::PositiveInfinity),
                    Argument::BigInt("9".into()),
                ],
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        let child = object(result);
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::ReleaseGroup { group: "".into() },
            edits: vec![],
        });
        assert_eq!(result, AutomationResult::Released);
        for id in [parent, child] {
            assert!(live(&realm.automation_registry.as_ref().unwrap().borrow(), 1, id).is_err());
        }
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CallFunction {
                source: "function(){return this===window}".into(),
                receiver: None,
                arguments: vec![],
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(result, AutomationResult::Value(RemoteValue::Boolean(true)));
    }

    #[test]
    fn atomic_descriptor_failure_burns_ids_without_leaking_roots() {
        let mut realm = loaded("");
        let id = object(eval(
            &mut realm,
            "Object.fromEntries(Array.from({length:33},(_,i)=>['p'+i,{}]))",
            false,
        ));
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::GetOwnProperties { object: id },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        let registry = realm.automation_registry.as_ref().unwrap().borrow();
        assert_eq!(
            registry.roots.len(),
            33,
            "parent plus 32 unrefunded staged IDs"
        );
        assert_eq!(
            registry.roots.iter().filter(|r| r.value.is_some()).count(),
            1,
            "no partial descriptor roots"
        );
        assert!(
            lookup(&registry, 1, 2).is_err(),
            "unpublished tombstone is not a released valid handle"
        );
        drop(registry);
        assert_eq!(
            object(eval(&mut realm, "({})", false)),
            34,
            "failed IDs never reused"
        );
    }

    #[test]
    fn exact_property_string_bigint_and_tree_boundaries() {
        let mut realm = loaded("");
        assert!(
            matches!(eval(&mut realm,"'x'.repeat(4096)",false),AutomationResult::Value(RemoteValue::String(s)) if s.len()==4096)
        );
        assert!(matches!(
            eval(&mut realm, "'x'.repeat(4097)", false),
            AutomationResult::Rejected { .. }
        ));
        assert!(matches!(
            eval(&mut realm, "(1n<<4096n)-1n", false),
            AutomationResult::Value(RemoteValue::BigInt(_))
        ));
        assert!(matches!(
            eval(&mut realm, "1n<<4096n", false),
            AutomationResult::Rejected { .. }
        ));
        for (count, pass) in [(64, true), (65, false)] {
            let id = object(eval(
                &mut realm,
                &format!("Object.fromEntries(Array.from({{length:{count}}},(_,i)=>['p'+i,i]))"),
                false,
            ));
            let (_, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::GetOwnProperties { object: id },
                edits: vec![],
            });
            assert_eq!(matches!(result, AutomationResult::Properties(_)), pass);
        }
        assert!(matches!(
            eval(&mut realm, "Array.from({length:511},()=>null)", true),
            AutomationResult::Value(_)
        ));
        assert!(matches!(
            eval(&mut realm, "Array.from({length:512},()=>null)", true),
            AutomationResult::Rejected { .. }
        ));
        assert!(
            matches!(
                eval(&mut realm, "(()=>{let x={};return [x,x]})()", true),
                AutomationResult::Value(_)
            ),
            "acyclic aliases may duplicate"
        );
    }

    #[test]
    fn cleanup_is_js_free_and_owner_slots_and_id_history_are_not_refunded() {
        let mut realm = loaded("");
        for owner in 1..=32 {
            let (_, result) = realm.automation(AutomationRequest {
                owner,
                operation: Operation::Evaluate {
                    source: "({})".into(),
                    return_by_value: false,
                    group: None,
                    world: None,
                    await_promise: false,
                    user_gesture: false,
                },
                edits: vec![],
            });
            object(result);
        }
        let before = realm.engine.report();
        let (_, result) = realm.automation(AutomationRequest {
            owner: 33,
            operation: Operation::Evaluate {
                source: "throw 'must not execute'".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(realm.engine.report(), before);
        let (_, result) = realm.automation(AutomationRequest {
            owner: 0,
            operation: Operation::ReleaseOwners {
                owners: (1..=32).collect(),
            },
            edits: vec![],
        });
        assert_eq!(result, AutomationResult::Released);
        assert_eq!(
            realm.engine.report(),
            before,
            "cleanup calls no engine source/checkpoint"
        );
        assert_eq!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .len(),
            32
        );
        assert!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .iter()
                .all(|r| r.value.is_none())
        );
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "1".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert!(
            matches!(result, AutomationResult::Rejected { .. }),
            "revoked identity is never reactivated"
        );
    }

    #[test]
    fn fatal_completion_clears_actual_registry_roots_and_stops_later_js() {
        let mut realm = loaded("");
        object(eval(&mut realm, "({})", false));
        let (reply, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "while(true){}".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(reply.state, RealmState::Fatal);
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .iter()
                .all(|r| r.value.is_none())
        );
        let before = realm.engine.report();
        let (reply, _) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "document.title='after fatal'".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(reply.state, RealmState::Fatal);
        assert_eq!(realm.engine.report(), before);
    }

    #[test]
    fn semantic_rejection_acknowledges_real_edits_without_source_or_privacy_mutation() {
        let mut realm = loaded("");
        let node = realm
            .state
            .borrow()
            .host
            .document
            .nodes
            .iter()
            .position(|n| n.attr("id") == Some("input"))
            .unwrap();
        let before = realm.engine.report();
        let revision = realm.revision;
        let (reply, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::GetOwnProperties { object: 1 },
            edits: vec![ControlEdit {
                node,
                version: 7,
                value: "actual 🦀 input".into(),
            }],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(reply.state, RealmState::Ready);
        assert_eq!(reply.revision, revision + 1);
        assert_eq!(
            reply.acknowledgements,
            vec![ControlAck { node, version: 7 }]
        );
        assert_eq!(
            reply.snapshot.unwrap().nodes[node].attr("value"),
            Some("actual 🦀 input")
        );
        assert_eq!(realm.engine.report(), before);
        assert!(realm.state.borrow().error_exposure_allowed);
        assert!(realm.automation_registry.is_none());
    }

    #[test]
    fn privacy_purges_real_queued_notices_and_immediate_caught_rejections_stay_partial() {
        let mut realm = loaded(
            "document.getElementById('input').onclick=()=>{globalThis.pending=Promise.reject({secret:1})}",
        );
        let node = realm
            .state
            .borrow()
            .host
            .document
            .nodes
            .iter()
            .position(|n| n.attr("id") == Some("input"))
            .unwrap();
        let event = realm.dispatch(SessionInput {
            kind: InputKind::Click { target: node },
            edits: vec![],
        });
        assert!(event.errors.is_empty());
        assert_eq!(event.boa.as_ref().unwrap().pending_jobs, 1);
        let admitted = event.boa.unwrap().jobs_admitted;
        let (reply, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "7".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(
            result,
            AutomationResult::Value(RemoteValue::Number(Number::Finite(7.0)))
        );
        assert_eq!(reply.errors, vec![REJECTION_GAP]);
        assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
        assert_eq!(
            reply.boa.unwrap().jobs_admitted,
            admitted,
            "purging has no lifetime refund"
        );
        assert!(realm.state.borrow().rejections.is_empty());
        let (reply, _) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "Promise.reject({}).catch(()=>{});9".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(reply.errors, vec![REJECTION_GAP]);
        assert_eq!(reply.boa.unwrap().pending_jobs, 0);
    }

    #[test]
    fn caught_unrepresentable_exception_stays_an_exception_not_a_fake_value() {
        let mut realm = loaded("globalThis.errors=0;window.onerror=()=>{errors++}");
        for source in ["throw Symbol('private')", "throw 'x'.repeat(4097)"] {
            let result = eval(&mut realm, source, false);
            assert!(
                matches!(result,AutomationResult::Exception{diagnostic,exception:None} if diagnostic.contains("representation unavailable"))
            );
        }
        assert_eq!(
            eval(&mut realm, "errors", false),
            AutomationResult::Value(RemoteValue::Number(Number::Finite(0.0)))
        );
    }

    #[test]
    fn unseen_cleanup_owner_is_permanently_revoked_and_capacity_is_atomic() {
        let mut realm = loaded("");
        for range in [1..=32, 33..=64] {
            let (_, result) = realm.automation(AutomationRequest {
                owner: 0,
                operation: Operation::ReleaseOwners {
                    owners: range.collect(),
                },
                edits: vec![],
            });
            assert_eq!(result, AutomationResult::Released);
        }
        let before = realm.engine.report();
        let (_, result) = realm.automation(AutomationRequest {
            owner: 0,
            operation: Operation::ReleaseOwners {
                owners: vec![64, 65],
            },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .owners
                .len(),
            64
        );
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "throw 'not executed'".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(realm.engine.report(), before);
    }

    #[test]
    fn exact_cumulative_512_ids_survive_repeated_group_release() {
        let mut realm = loaded("");
        for batch in 0..16 {
            let (_, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::Evaluate {
                    source: "Object.fromEntries(Array.from({length:31},(_,i)=>['p'+i,{}]))".into(),
                    return_by_value: false,
                    group: Some("batch".into()),
                    world: None,
                    await_promise: false,
                    user_gesture: false,
                },
                edits: vec![],
            });
            let id = object(result);
            assert_eq!(id, batch * 32 + 1);
            let (_, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::GetOwnProperties { object: id },
                edits: vec![],
            });
            assert!(matches!(result,AutomationResult::Properties(v) if v.len()==31));
            let (_, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::ReleaseGroup {
                    group: "batch".into(),
                },
                edits: vec![],
            });
            assert_eq!(result, AutomationResult::Released);
        }
        assert_eq!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .len(),
            512
        );
        assert!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .iter()
                .all(|r| r.value.is_none())
        );
        assert!(matches!(
            eval(&mut realm, "({})", false),
            AutomationResult::Rejected { .. }
        ));
        assert_eq!(
            eval(&mut realm, "true", false),
            AutomationResult::Value(RemoteValue::Boolean(true)),
            "exhausted object IDs do not fake a fatal engine limit"
        );
    }

    #[test]
    fn exact_live_root_and_group_caps_never_evict_a_live_handle() {
        let mut realm = loaded("");
        for group in 0..32 {
            let (_, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::Evaluate {
                    source: "({})".into(),
                    return_by_value: false,
                    group: Some(group.to_string()),
                    world: None,
                    await_promise: false,
                    user_gesture: false,
                },
                edits: vec![],
            });
            object(result);
        }
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "({})".into(),
                return_by_value: false,
                group: Some("overflow".into()),
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .iter()
                .filter(|r| r.value.is_some())
                .count(),
            32
        );
        let parent = object(eval(
            &mut realm,
            "Object.fromEntries(Array.from({length:31},(_,i)=>['p'+i,{}]))",
            false,
        ));
        for _ in 0..7 {
            let (_, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::GetOwnProperties { object: parent },
                edits: vec![],
            });
            assert!(matches!(result, AutomationResult::Properties(_)));
        }
        for _ in 0..6 {
            object(eval(&mut realm, "({})", false));
        }
        assert_eq!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .iter()
                .filter(|r| r.value.is_some())
                .count(),
            256
        );
        assert!(matches!(
            eval(&mut realm, "({})", false),
            AutomationResult::Rejected { .. }
        ));
        assert!(live(&realm.automation_registry.as_ref().unwrap().borrow(), 1, 1).is_ok());
    }

    #[test]
    fn external_strings_and_tree_keys_reject_lone_surrogates_without_replacement() {
        let mut realm = loaded("");
        for (source, by_value) in [
            ("'\\uD800'", false),
            ("'\\uDC00'", true),
            ("'a\\uD800b'", false),
            ("({nested:['\\uD800']})", true),
            ("({nested:{['\\uDC00']:7}})", true),
        ] {
            assert!(
                matches!(eval(&mut realm, source, by_value), AutomationResult::Rejected { diagnostic } if diagnostic.contains("UTF-16")),
                "external unpaired code units must be rejected, not replaced or escaped: {source}"
            );
        }
        assert_eq!(
            eval(&mut realm, "'a\\uD83D\\uDE00z'", false),
            AutomationResult::Value(RemoteValue::String(vec![97, 0xd83d, 0xde00, 122]))
        );
        assert_eq!(
            eval(&mut realm, "({['\\uD83D\\uDE00']:'\\uD83D\\uDE00'})", true),
            AutomationResult::Value(RemoteValue::ByValue(SerializedValue::Object(vec![(
                vec![0xd83d, 0xde00],
                SerializedValue::String(vec![0xd83d, 0xde00]),
            )])))
        );
    }

    #[test]
    fn external_text_admission_does_not_restrict_lossless_worker_arguments() {
        let mut realm = loaded("");
        let (reply, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CallFunction {
                source: "function(s,o){return s.length===1 && s.charCodeAt(0)===55296 && Object.keys(o)[0].charCodeAt(0)===56320 && o[Object.keys(o)[0]].charCodeAt(0)===55296}".into(),
                receiver: None,
                arguments: vec![
                    Argument::Value(SerializedValue::String(vec![0xd800])),
                    Argument::Value(SerializedValue::Object(vec![(
                        vec![0xdc00],
                        SerializedValue::String(vec![0xd800]),
                    )])),
                ],
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(reply.state, RealmState::Ready);
        assert!(reply.errors.is_empty());
        assert_eq!(result, AutomationResult::Value(RemoteValue::Boolean(true)));
    }

    #[test]
    fn external_descriptor_text_failure_rolls_back_all_roots_without_id_refund() {
        for (source, issued) in [
            ("({first:{},['\\uD800']:{},last:{}})", 4),
            ("({first:{},bad:'\\uDC00',last:{}})", 3),
            (
                "({first:{},get ['\\uD800'](){hits++;return 1},set ['\\uD800'](v){hits++},last:{}})",
                5,
            ),
        ] {
            let mut realm = loaded("globalThis.hits=0");
            let id = object(eval(&mut realm, source, false));
            assert_eq!(id, 1);
            let (reply, result) = realm.automation(AutomationRequest {
                owner: 1,
                operation: Operation::GetOwnProperties { object: id },
                edits: vec![],
            });
            assert_eq!(reply.state, RealmState::Ready);
            assert!(reply.errors.is_empty());
            assert!(
                matches!(result, AutomationResult::Rejected { diagnostic } if diagnostic.contains("UTF-16"))
            );
            let registry = realm.automation_registry.as_ref().unwrap().borrow();
            assert_eq!(
                registry.roots.len(),
                issued,
                "all reserved IDs remain spent"
            );
            assert_eq!(
                registry
                    .roots
                    .iter()
                    .filter(|root| root.value.is_some())
                    .count(),
                1
            );
            assert!(
                registry.roots[1..]
                    .iter()
                    .all(|root| !root.published && root.value.is_none() && root.group.is_none())
            );
            assert!(
                live(&registry, 1, id).is_ok(),
                "existing root is not evicted"
            );
            for id in 2..=issued as u64 {
                assert!(lookup(&registry, 1, id).is_err());
            }
            drop(registry);
            assert_eq!(
                eval(&mut realm, "hits", false),
                AutomationResult::Value(RemoteValue::Number(Number::Finite(0.0)))
            );
            assert_eq!(object(eval(&mut realm, "({})", false)), issued as u64 + 1);
        }
    }

    #[test]
    fn thrown_lone_surrogate_remains_a_caught_exception_with_real_effects() {
        let mut realm = loaded("globalThis.errors=0;window.onerror=()=>{errors++}");
        let (reply, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "document.title='before checkpoint';Promise.resolve().then(()=>{document.title='after checkpoint'});throw '\\uD800'".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(reply.state, RealmState::Ready);
        assert!(reply.errors.is_empty());
        assert!(
            matches!(result, AutomationResult::Exception { diagnostic, exception: None } if diagnostic.contains("representation unavailable") && diagnostic.contains("UTF-16"))
        );
        assert_eq!(
            eval(
                &mut realm,
                "document.title==='after checkpoint' && errors===0",
                false
            ),
            AutomationResult::Value(RemoteValue::Boolean(true))
        );
        assert!(!realm.state.borrow().error_exposure_allowed);
        assert!(
            realm
                .automation_registry
                .as_ref()
                .unwrap()
                .borrow()
                .roots
                .is_empty()
        );
    }

    #[test]
    fn callable_proxy_accessor_metadata_is_native_and_inspection_never_traps() {
        let mut realm = loaded("globalThis.traps=0");
        let id = object(eval(
            &mut realm,
            "Object.defineProperty({},'x',{get:new Proxy(function(){return 21},{ownKeys(){traps+=100;return []},apply(target,receiver,args){traps++;return Reflect.apply(target,receiver,args)}})})",
            false,
        ));
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::GetOwnProperties { object: id },
            edits: vec![],
        });
        let AutomationResult::Properties(properties) = result else {
            panic!("real descriptor required")
        };
        let PropertyDescriptor::Accessor {
            get:
                Some(RemoteValue::Object {
                    object: getter,
                    kind: ObjectKind::Function,
                }),
            set: None,
        } = properties[0].descriptor
        else {
            panic!("native callable metadata required")
        };
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::GetOwnProperties { object: getter },
            edits: vec![],
        });
        assert!(matches!(result, AutomationResult::Rejected { .. }));
        assert_eq!(
            eval(&mut realm, "traps", false),
            AutomationResult::Value(RemoteValue::Number(Number::Finite(0.0)))
        );
        let (_, result) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CallFunction {
                source: "function(){return this()}".into(),
                receiver: Some(getter),
                arguments: vec![],
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(
            result,
            AutomationResult::Value(RemoteValue::Number(Number::Finite(21.0)))
        );
        assert_eq!(
            eval(&mut realm, "traps", false),
            AutomationResult::Value(RemoteValue::Number(Number::Finite(1.0)))
        );
    }

    #[test]
    fn isolated_worlds_await_promise_resolve_node_and_automation_only() {
        let mut init = crate::page_session::platform::PlatformInit::default();
        init.automation_only = true;
        let (realm, mut reply) = super::super::BoaPageRealm::start_with_platform(
            Request {
                url: "https://example.test/pw".into(),
                html: "<title>PW</title><button id='btn' onclick='window.leaked=1'>Go</button><input id='q'><script>window.leaked=1; document.title='SCRIPT_RAN';</script>".into(),
                external_scripts: None,
            },
            crate::page_session::observer::ObserverInit::Disabled,
            init,
            [7u8; 32],
        );
        let mut realm = realm.unwrap();
        while reply.lifecycle.unwrap().phase != LifecyclePhase::LoadComplete {
            reply = realm.pump_tasks(TaskPump {
                wake_token: reply.task_wake.unwrap().token,
            });
        }
        assert_eq!(
            eval(
                &mut realm,
                "document.title + ':' + typeof window.leaked",
                false
            ),
            AutomationResult::Value(RemoteValue::String("PW:undefined".encode_utf16().collect()))
        );
        let (_, created) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CreateIsolatedWorld {
                world: "__playwright_utility_world__".into(),
            },
            edits: vec![],
        });
        assert_eq!(created, AutomationResult::Released);
        // Set a global in main world and verify isolated world has separate global, own constructors, and shared DOM.
        eval(&mut realm, "window.pageSecret = 99", false);
        let (_, util_handle_res) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "window.utilState = 42; ({ check(el) { return typeof window.pageSecret === 'undefined' && window.utilState === 42 && (el instanceof HTMLElement) && (el instanceof Element) && (el instanceof Node); } })".into(),
                return_by_value: false,
                group: None,
                world: Some("__playwright_utility_world__".into()),
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        let util_obj = object(util_handle_res);
        // Query a DOM element in isolated world: must export ObjectKind::Node { node }!
        let (_, btn_res) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "document.getElementById('btn')".into(),
                return_by_value: false,
                group: None,
                world: Some("__playwright_utility_world__".into()),
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        let (btn_obj, btn_node) = match btn_res {
            AutomationResult::Value(RemoteValue::Object {
                object,
                kind: ObjectKind::Node { node },
            }) => (object, node),
            other => panic!("expected Node object: {other:?}"),
        };
        // ResolveNode in isolated world returns the exact same node kind.
        let (_, resolved) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::ResolveNode {
                node: btn_node,
                world: Some("__playwright_utility_world__".into()),
                group: None,
            },
            edits: vec![],
        });
        assert!(matches!(
            resolved,
            AutomationResult::Value(RemoteValue::Object {
                kind: ObjectKind::Node { node },
                ..
            }) if node == btn_node
        ));
        // CallFunction WITHOUT world inherits isolated world from receiver!
        let (_, check_res) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CallFunction {
                source: "function(el) { return this.check(el); }".into(),
                receiver: Some(util_obj),
                arguments: vec![Argument::Object(btn_obj)],
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert_eq!(
            check_res,
            AutomationResult::Value(RemoteValue::Boolean(true))
        );
        // await_promise settles IntersectionObserver (microtask) + requestAnimationFrame (timer)!
        let (_, async_res) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::CallFunction {
                source: "async function(el) {
                    const ratio = await new Promise(resolve => {
                        const io = new IntersectionObserver(entries => { io.disconnect(); resolve(entries[0].intersectionRatio); });
                        io.observe(el);
                    });
                    const rafOk = await new Promise(resolve => requestAnimationFrame(() => resolve(true)));
                    const hit = document.elementFromPoint(10, 10);
                    return ratio > 0 && rafOk && hit !== null;
                }".into(),
                receiver: Some(util_obj),
                arguments: vec![Argument::Object(btn_obj)],
                return_by_value: false,
                group: None,
                world: None,
                await_promise: true,
                user_gesture: true,
            },
            edits: vec![],
        });
        assert_eq!(
            async_res,
            AutomationResult::Value(RemoteValue::Boolean(true))
        );
        // Rejected awaited promise surfaces Error message cleanly.
        let (_, rej_res) = realm.automation(AutomationRequest {
            owner: 1,
            operation: Operation::Evaluate {
                source: "Promise.reject(new Error('not visible yet'))".into(),
                return_by_value: false,
                group: None,
                world: Some("__playwright_utility_world__".into()),
                await_promise: true,
                user_gesture: false,
            },
            edits: vec![],
        });
        assert!(
            matches!(rej_res, AutomationResult::Exception { diagnostic, .. } if diagnostic.contains("not visible yet"))
        );
    }
}
