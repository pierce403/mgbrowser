//! The worker-local Window message clone and `structuredClone` domain.
//!
//! `CloneMode::PostMessage` preserves the frozen Window `postMessage` contract,
//! while `CloneMode::StructuredClone` adds rich structured-clone support for
//! `structuredClone()` and `history.pushState`/`replaceState`.
use super::{dom_exception, message_charge, message_reserve};
use boa_engine::{
    Context, JsBigInt, JsError, JsNativeError, JsResult, JsString, JsSymbol, JsValue,
    bigint::RawBigInt,
    builtins::{
        array_buffer::{ArrayBuffer, SharedArrayBuffer},
        dataview::DataView,
        date::Date,
        error::Error,
        map::ordered_map::OrderedMap,
        regexp::RegExp,
        set::ordered_set::OrderedSet,
        typed_array::{TypedArray, TypedArrayKind},
    },
    js_string,
    object::{
        JsObject,
        builtins::{
            JsArray, JsArrayBuffer, JsDataView, JsDate, JsMap, JsRegExp, JsSet, JsTypedArray,
        },
    },
    property::{PropertyDescriptor, PropertyKey},
    value::JsVariant,
};
use mg_butane::modern::{self, boa_engine};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    mem::size_of,
};

const INVARIANT: &str = "Window message clone invariant or storage admission failed";
type NodeId = usize;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CloneMode {
    PostMessage,
    StructuredClone,
}

/// No JsObject, JsValue, Rc edge or hidden source root is stored here.
#[derive(Clone)]
pub(super) struct SerializedMessage {
    root: NodeId,
    nodes: Vec<Node>,
}

#[derive(Clone, Copy)]
enum BoxedKind {
    Boolean(bool),
    Number(f64),
}

#[derive(Clone)]
enum Node {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    String(Vec<u16>),
    BigInt {
        negative: bool,
        digits: Vec<u32>,
    },
    BoxedPrimitive(BoxedKind),
    BoxedString(Vec<u16>),
    Date(f64),
    RegExp {
        pattern: String,
        flags: String,
    },
    ArrayBuffer(Vec<u8>),
    TypedArray {
        kind: TypedArrayKind,
        buffer: NodeId,
        byte_offset: usize,
        byte_length: usize,
    },
    DataView {
        buffer: NodeId,
        byte_offset: usize,
        byte_length: usize,
    },
    Map {
        entries: Vec<(NodeId, NodeId)>,
    },
    Set {
        values: Vec<NodeId>,
    },
    Error {
        kind_name: String,
        message: Option<Vec<u16>>,
        stack: Option<Vec<u16>>,
    },
    Object {
        array_length: Option<u32>,
        properties: Vec<(Key, NodeId)>,
    },
}

#[derive(Clone)]
enum Key {
    Index(u32),
    String(Vec<u16>),
}

enum FrameWork {
    Keys {
        keys: Vec<PropertyKey>,
        next: usize,
    },
    MapEntries {
        pairs: Vec<(JsValue, JsValue)>,
        next: usize,
    },
    SetValues {
        items: Vec<JsValue>,
        next: usize,
    },
}

struct Frame {
    source: JsObject,
    node: NodeId,
    work: FrameWork,
}

#[derive(Default)]
struct Memo {
    slots: Vec<Option<(JsObject, NodeId)>>,
    entries: usize,
}

impl Memo {
    fn hash(object: &JsObject) -> usize {
        let mut hasher = DefaultHasher::new();
        object.hash(&mut hasher);
        hasher.finish() as usize
    }

    fn get(&self, object: &JsObject) -> Option<NodeId> {
        if self.slots.is_empty() {
            return None;
        }
        let start = Self::hash(object) % self.slots.len();
        for offset in 0..self.slots.len() {
            match &self.slots[(start + offset) % self.slots.len()] {
                Some((source, id)) if source == object => return Some(*id),
                None => return None,
                _ => {}
            }
        }
        None
    }

    fn place(
        slots: &mut [Option<(JsObject, NodeId)>],
        object: JsObject,
        id: NodeId,
        context: &mut Context,
    ) -> JsResult<()> {
        if slots.is_empty() {
            return Err(invariant(context));
        }
        let start = Self::hash(&object) % slots.len();
        for offset in 0..slots.len() {
            let index = (start + offset) % slots.len();
            if slots[index].is_none() {
                slots[index] = Some((object, id));
                return Ok(());
            }
        }
        Err(invariant(context))
    }

    fn insert(&mut self, object: JsObject, id: NodeId, context: &mut Context) -> JsResult<()> {
        modern::check_context(context)?;
        if self.entries >= self.slots.len() / 2 {
            let length = if self.slots.is_empty() {
                8
            } else {
                self.slots
                    .len()
                    .checked_mul(2)
                    .ok_or_else(|| invariant(context))?
            };
            let mut slots = Vec::new();
            message_reserve(&mut slots, length, context)?;
            slots.resize_with(length, || None);
            for entry in self.slots.drain(..).flatten() {
                Self::place(&mut slots, entry.0, entry.1, context)?;
            }
            self.slots = slots;
        }
        Self::place(&mut self.slots, object, id, context)?;
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or_else(|| invariant(context))?;
        Ok(())
    }
}

fn invariant(context: &mut Context) -> JsError {
    modern::latch_context(context, INVARIANT)
}

fn clone_error(name: &str, message: &str, context: &mut Context) -> JsResult<JsError> {
    modern::check_context(context)?;
    dom_exception::exception(name, message, context)
}

fn copy_string(value: &JsString, context: &mut Context) -> JsResult<Vec<u16>> {
    let mut units = Vec::new();
    message_reserve(&mut units, value.len(), context)?;
    units.extend(value.iter());
    modern::check_context(context)?;
    Ok(units)
}

fn copy_bytes(bytes: &[u8], context: &mut Context) -> JsResult<Vec<u8>> {
    let mut out = Vec::new();
    message_reserve(&mut out, bytes.len(), context)?;
    out.extend_from_slice(bytes);
    modern::check_context(context)?;
    Ok(out)
}

fn copy_key(key: &PropertyKey, context: &mut Context) -> JsResult<Key> {
    match key {
        PropertyKey::Index(index) => Ok(Key::Index(index.get())),
        PropertyKey::String(value) => Ok(Key::String(copy_string(value, context)?)),
        PropertyKey::Symbol(_) => Err(invariant(context)),
    }
}

fn check_object_post_message(object: &JsObject, context: &mut Context) -> JsResult<()> {
    modern::check_context(context)?;
    if object == &context.global_object()
        || object.is_callable()
        || object.is::<JsSymbol>()
        || object.is::<SharedArrayBuffer>()
    {
        return Err(clone_error(
            "DataCloneError",
            "Value cannot be cloned",
            context,
        )?);
    }
    if let Some(buffer) = object.downcast_ref::<ArrayBuffer>() {
        let detached = buffer.data().is_none();
        drop(buffer);
        return Err(clone_error(
            if detached {
                "DataCloneError"
            } else {
                "NotSupportedError"
            },
            if detached {
                "Detached ArrayBuffer cannot be cloned"
            } else {
                "ArrayBuffer cloning is not supported"
            },
            context,
        )?);
    }
    if object.is::<bool>()
        || object.is::<f64>()
        || object.is::<JsString>()
        || object.is::<JsBigInt>()
        || object.is::<Date>()
        || object.is::<RegExp>()
        || object.is::<OrderedMap<JsValue>>()
        || object.is::<OrderedSet>()
        || object.is::<TypedArray>()
        || object.is::<DataView>()
        || object.is::<Error>()
    {
        return Err(clone_error(
            "NotSupportedError",
            "This cloneable builtin is not supported",
            context,
        )?);
    }
    if object.is_array() || object.is_ordinary() {
        Ok(())
    } else {
        Err(clone_error(
            "DataCloneError",
            "Value cannot be cloned",
            context,
        )?)
    }
}

fn array_length(object: &JsObject, context: &mut Context) -> JsResult<Option<u32>> {
    if !object.is_array() {
        return Ok(None);
    }
    let descriptor = {
        let source = object.try_borrow().map_err(|_| invariant(context))?;
        source.properties().get(&js_string!("length").into())
    };
    let number = descriptor
        .as_ref()
        .and_then(PropertyDescriptor::value)
        .and_then(JsValue::as_number)
        .ok_or_else(|| invariant(context))?;
    if !number.is_finite() || number < 0.0 || number > f64::from(u32::MAX) || number.fract() != 0.0
    {
        return Err(invariant(context));
    }
    Ok(Some(number as u32))
}

fn own_enumerable_keys(object: &JsObject, context: &mut Context) -> JsResult<Vec<PropertyKey>> {
    modern::check_context(context)?;
    let result = object.own_property_keys(context);
    modern::check_context(context)?;
    let mut keys = result?;
    message_charge(
        keys.capacity()
            .checked_mul(size_of::<PropertyKey>())
            .ok_or_else(|| invariant(context))?,
        context,
    )?;
    {
        let source = object.try_borrow().map_err(|_| invariant(context))?;
        keys.retain(|key| {
            !matches!(key, PropertyKey::Symbol(_))
                && source
                    .properties()
                    .get(key)
                    .is_some_and(|descriptor| descriptor.enumerable() == Some(true))
        });
    }
    Ok(keys)
}

fn intern_array_buffer(
    buffer_obj: &JsObject,
    nodes: &mut Vec<Node>,
    memo: &mut Memo,
    context: &mut Context,
) -> JsResult<NodeId> {
    if let Some(id) = memo.get(buffer_obj) {
        return Ok(id);
    }
    let data = {
        let buf = buffer_obj
            .downcast_ref::<ArrayBuffer>()
            .ok_or_else(|| invariant(context))?;
        match buf.data() {
            Some(slice) => copy_bytes(&slice, context)?,
            None => {
                drop(buf);
                return Err(clone_error(
                    "DataCloneError",
                    "Detached ArrayBuffer cannot be cloned",
                    context,
                )?);
            }
        }
    };
    message_reserve(nodes, 1, context)?;
    let id = nodes.len();
    nodes.push(Node::ArrayBuffer(data));
    memo.insert(buffer_obj.clone(), id, context)?;
    Ok(id)
}

fn intern(
    value: &JsValue,
    mode: CloneMode,
    nodes: &mut Vec<Node>,
    memo: &mut Memo,
    frames: &mut Vec<Frame>,
    context: &mut Context,
) -> JsResult<NodeId> {
    modern::check_context(context)?;
    let node = match value.variant() {
        JsVariant::Undefined => Node::Undefined,
        JsVariant::Null => Node::Null,
        JsVariant::Boolean(value) => Node::Boolean(value),
        JsVariant::Integer32(value) => Node::Number(f64::from(value)),
        JsVariant::Float64(value) => Node::Number(value),
        JsVariant::String(value) => Node::String(copy_string(&value, context)?),
        JsVariant::BigInt(value) => {
            let raw = value.as_inner();
            let source = raw.iter_u32_digits();
            let mut digits = Vec::new();
            message_reserve(&mut digits, source.len(), context)?;
            digits.extend(source);
            Node::BigInt {
                negative: raw.sign() == RawBigInt::from(-1).sign(),
                digits,
            }
        }
        JsVariant::Symbol(_) => {
            return Err(clone_error(
                "DataCloneError",
                "Symbol cannot be cloned",
                context,
            )?);
        }
        JsVariant::Object(object) => {
            if let Some(id) = memo.get(&object) {
                return Ok(id);
            }
            if mode == CloneMode::PostMessage {
                check_object_post_message(&object, context)?;
                let array_length = array_length(&object, context)?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::Object {
                    array_length,
                    properties: Vec::new(),
                });
                memo.insert(object.clone(), id, context)?;
                let keys = own_enumerable_keys(&object, context)?;
                message_reserve(frames, 1, context)?;
                frames.push(Frame {
                    source: object,
                    node: id,
                    work: FrameWork::Keys { keys, next: 0 },
                });
                return Ok(id);
            }

            // StructuredClone mode:
            if object == context.global_object()
                || object.is_callable()
                || object.is::<JsSymbol>()
                || object.is::<SharedArrayBuffer>()
                || object.is::<boa_engine::builtins::proxy::Proxy>()
            {
                return Err(clone_error(
                    "DataCloneError",
                    "Value cannot be cloned",
                    context,
                )?);
            }
            // Reject DOMException or branded host objects if not standard Error
            if dom_exception::is_dom_exception(&object, context)? {
                // Allow DOMException cloning as Error-like or reject? Note: in WebIDL DOMException is serializable,
                // let's clone name + message cleanly if Error or DOMException!
                let name_val = object.get(js_string!("name"), context)?;
                let msg_val = object.get(js_string!("message"), context)?;
                let name_str = name_val.to_string(context)?.to_std_string_escaped();
                let msg_u16 = copy_string(&msg_val.to_string(context)?, context)?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::Error {
                    kind_name: format!("DOMException:{name_str}"),
                    message: Some(msg_u16),
                    stack: None,
                });
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if let Some(v) = object.downcast_ref::<bool>().map(|b| *b) {
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::BoxedPrimitive(BoxedKind::Boolean(v)));
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if let Some(v) = object.downcast_ref::<f64>().map(|n| *n) {
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::BoxedPrimitive(BoxedKind::Number(v)));
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if let Some(cloned) = object.downcast_ref::<JsString>().map(|s| s.clone()) {
                let units = copy_string(&cloned, context)?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::BoxedString(units));
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if object.is::<Date>() {
                let js_date =
                    JsDate::from_object(object.clone()).map_err(|_| invariant(context))?;
                let time_val = js_date.get_time(context)?;
                let ms = time_val.as_number().unwrap_or(f64::NAN);
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::Date(ms));
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if object.is::<RegExp>() {
                let regexp =
                    JsRegExp::from_object(object.clone()).map_err(|_| invariant(context))?;
                let pattern = regexp.source(context)?;
                let flags = regexp.flags(context)?;
                message_charge(pattern.len().saturating_add(flags.len()), context)?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::RegExp { pattern, flags });
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if object.is::<ArrayBuffer>() {
                return intern_array_buffer(&object, nodes, memo, context);
            }
            if object.is::<TypedArray>() {
                let ta =
                    JsTypedArray::from_object(object.clone()).map_err(|_| invariant(context))?;
                let kind = ta.kind().ok_or_else(|| invariant(context))?;
                let buf_val = ta.buffer(context)?;
                let buf_obj = buf_val.as_object().ok_or_else(|| invariant(context))?;
                if buf_obj.is::<SharedArrayBuffer>() {
                    return Err(clone_error(
                        "DataCloneError",
                        "SharedArrayBuffer views cannot be cloned",
                        context,
                    )?);
                }
                let is_detached = buf_obj
                    .downcast_ref::<ArrayBuffer>()
                    .is_none_or(|ab| ab.data().is_none());
                if is_detached {
                    return Err(clone_error(
                        "DataCloneError",
                        "Detached TypedArray cannot be cloned",
                        context,
                    )?);
                }
                let byte_offset = ta.byte_offset(context)?;
                let byte_length = ta.byte_length(context)?;
                let buffer_id = intern_array_buffer(&buf_obj, nodes, memo, context)?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::TypedArray {
                    kind,
                    buffer: buffer_id,
                    byte_offset,
                    byte_length,
                });
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if object.is::<DataView>() {
                let dv = JsDataView::from_object(object.clone()).map_err(|_| invariant(context))?;
                let buf_val = dv.buffer(context)?;
                let buf_obj = buf_val.as_object().ok_or_else(|| invariant(context))?;
                let byte_offset = dv.byte_offset(context)? as usize;
                let byte_length = dv.byte_length(context)? as usize;
                let buffer_id = intern_array_buffer(&buf_obj, nodes, memo, context)?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::DataView {
                    buffer: buffer_id,
                    byte_offset,
                    byte_length,
                });
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if object.is::<OrderedMap<JsValue>>() {
                let pairs: Vec<(JsValue, JsValue)> = {
                    let map_ref = object
                        .downcast_ref::<OrderedMap<JsValue>>()
                        .ok_or_else(|| invariant(context))?;
                    map_ref
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                };
                message_charge(
                    pairs
                        .len()
                        .checked_mul(size_of::<(JsValue, JsValue)>())
                        .ok_or_else(|| invariant(context))?,
                    context,
                )?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::Map {
                    entries: Vec::new(),
                });
                memo.insert(object.clone(), id, context)?;
                message_reserve(frames, 1, context)?;
                frames.push(Frame {
                    source: object,
                    node: id,
                    work: FrameWork::MapEntries { pairs, next: 0 },
                });
                return Ok(id);
            }
            if object.is::<OrderedSet>() {
                let items: Vec<JsValue> = {
                    let set_ref = object
                        .downcast_ref::<OrderedSet>()
                        .ok_or_else(|| invariant(context))?;
                    set_ref.iter().cloned().collect()
                };
                message_charge(
                    items
                        .len()
                        .checked_mul(size_of::<JsValue>())
                        .ok_or_else(|| invariant(context))?,
                    context,
                )?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::Set { values: Vec::new() });
                memo.insert(object.clone(), id, context)?;
                message_reserve(frames, 1, context)?;
                frames.push(Frame {
                    source: object,
                    node: id,
                    work: FrameWork::SetValues { items, next: 0 },
                });
                return Ok(id);
            }
            if object.is::<Error>() {
                let raw_name = object
                    .get(js_string!("name"), context)?
                    .to_string(context)?
                    .to_std_string_escaped();
                let kind_name = match raw_name.as_str() {
                    "TypeError" | "RangeError" | "SyntaxError" | "ReferenceError" | "URIError"
                    | "EvalError" => raw_name,
                    _ => "Error".to_owned(),
                };
                let message = if object.has_own_property(js_string!("message"), context)? {
                    let v = object.get(js_string!("message"), context)?;
                    Some(copy_string(&v.to_string(context)?, context)?)
                } else {
                    None
                };
                let stack = if object.has_own_property(js_string!("stack"), context)? {
                    let v = object.get(js_string!("stack"), context)?;
                    if v.is_string() {
                        Some(copy_string(&v.to_string(context)?, context)?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::Error {
                    kind_name,
                    message,
                    stack,
                });
                memo.insert(object, id, context)?;
                return Ok(id);
            }
            if object.is_array() || object.is_ordinary() {
                let array_length = array_length(&object, context)?;
                message_reserve(nodes, 1, context)?;
                let id = nodes.len();
                nodes.push(Node::Object {
                    array_length,
                    properties: Vec::new(),
                });
                memo.insert(object.clone(), id, context)?;
                let keys = own_enumerable_keys(&object, context)?;
                message_reserve(frames, 1, context)?;
                frames.push(Frame {
                    source: object,
                    node: id,
                    work: FrameWork::Keys { keys, next: 0 },
                });
                return Ok(id);
            }
            return Err(clone_error(
                "DataCloneError",
                "Value cannot be cloned",
                context,
            )?);
        }
    };
    message_reserve(nodes, 1, context)?;
    let id = nodes.len();
    nodes.push(node);
    Ok(id)
}

pub(super) fn serialize(value: &JsValue, context: &mut Context) -> JsResult<SerializedMessage> {
    serialize_with_mode(value, CloneMode::PostMessage, &[], context)
}

pub(super) fn serialize_structured(
    value: &JsValue,
    transfers: &[JsObject],
    context: &mut Context,
) -> JsResult<SerializedMessage> {
    for (idx, obj) in transfers.iter().enumerate() {
        if transfers[..idx].contains(obj) {
            return Err(clone_error(
                "DataCloneError",
                "Duplicate transfer entry",
                context,
            )?);
        }
        let Ok(ab) = JsArrayBuffer::from_object(obj.clone()) else {
            return Err(clone_error(
                "DataCloneError",
                "Object in transfer list is not transferable",
                context,
            )?);
        };
        if ab.data().is_none() {
            return Err(clone_error(
                "DataCloneError",
                "Detached ArrayBuffer in transfer list",
                context,
            )?);
        }
    }
    let msg = serialize_with_mode(value, CloneMode::StructuredClone, transfers, context)?;
    for obj in transfers {
        let ab = JsArrayBuffer::from_object(obj.clone()).map_err(|_| invariant(context))?;
        let _ = ab.detach(&JsValue::undefined());
    }
    Ok(msg)
}

pub(super) fn serialize_with_mode(
    value: &JsValue,
    mode: CloneMode,
    _transfers: &[JsObject],
    context: &mut Context,
) -> JsResult<SerializedMessage> {
    modern::check_context(context)?;
    let mut nodes = Vec::new();
    let mut memo = Memo::default();
    let mut frames = Vec::new();
    let root = intern(value, mode, &mut nodes, &mut memo, &mut frames, context)?;
    while !frames.is_empty() {
        modern::check_context(context)?;
        enum Step {
            Done,
            Prop(JsObject, NodeId, PropertyKey),
            MapEntry(NodeId, JsValue, JsValue),
            SetItem(NodeId, JsValue),
        }
        let step = {
            let frame = frames.last_mut().ok_or_else(|| invariant(context))?;
            match &mut frame.work {
                FrameWork::Keys { keys, next } => {
                    if *next == keys.len() {
                        Step::Done
                    } else {
                        let k = keys[*next].clone();
                        *next += 1;
                        Step::Prop(frame.source.clone(), frame.node, k)
                    }
                }
                FrameWork::MapEntries { pairs, next } => {
                    if *next == pairs.len() {
                        Step::Done
                    } else {
                        let (k, v) = pairs[*next].clone();
                        *next += 1;
                        Step::MapEntry(frame.node, k, v)
                    }
                }
                FrameWork::SetValues { items, next } => {
                    if *next == items.len() {
                        Step::Done
                    } else {
                        let v = items[*next].clone();
                        *next += 1;
                        Step::SetItem(frame.node, v)
                    }
                }
            }
        };
        match step {
            Step::Done => {
                frames.pop();
            }
            Step::Prop(source, parent, key) => {
                let result = source.has_own_property(key.clone(), context);
                modern::check_context(context)?;
                if !result? {
                    continue;
                }
                let owned_key = copy_key(&key, context)?;
                let result = source.get(key, context);
                modern::check_context(context)?;
                let val = result?;
                let child = intern(&val, mode, &mut nodes, &mut memo, &mut frames, context)?;
                let Some(Node::Object { properties, .. }) = nodes.get_mut(parent) else {
                    return Err(invariant(context));
                };
                message_reserve(properties, 1, context)?;
                properties.push((owned_key, child));
            }
            Step::MapEntry(parent, k, v) => {
                let kid = intern(&k, mode, &mut nodes, &mut memo, &mut frames, context)?;
                let vid = intern(&v, mode, &mut nodes, &mut memo, &mut frames, context)?;
                let Some(Node::Map { entries }) = nodes.get_mut(parent) else {
                    return Err(invariant(context));
                };
                message_reserve(entries, 1, context)?;
                entries.push((kid, vid));
            }
            Step::SetItem(parent, v) => {
                let vid = intern(&v, mode, &mut nodes, &mut memo, &mut frames, context)?;
                let Some(Node::Set { values }) = nodes.get_mut(parent) else {
                    return Err(invariant(context));
                };
                message_reserve(values, 1, context)?;
                values.push(vid);
            }
        }
    }
    modern::check_context(context)?;
    Ok(SerializedMessage { root, nodes })
}

fn restored_key(key: &Key) -> PropertyKey {
    match key {
        Key::Index(index) => (*index).into(),
        Key::String(units) => JsString::from(units.as_slice()).into(),
    }
}

fn construct_typed_array(
    kind: TypedArrayKind,
    buffer: JsArrayBuffer,
    byte_offset: usize,
    byte_length: usize,
    context: &mut Context,
) -> JsResult<JsValue> {
    let elem_size: usize = match kind {
        TypedArrayKind::Int8 | TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => 1,
        TypedArrayKind::Int16 | TypedArrayKind::Uint16 => 2,
        TypedArrayKind::Int32 | TypedArrayKind::Uint32 | TypedArrayKind::Float32 => 4,
        TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 | TypedArrayKind::Float64 => 8,
    };
    let length = byte_length / elem_size;
    let ctor = match kind {
        TypedArrayKind::Int8 => context.intrinsics().constructors().typed_int8_array(),
        TypedArrayKind::Uint8 => context.intrinsics().constructors().typed_uint8_array(),
        TypedArrayKind::Uint8Clamped => context
            .intrinsics()
            .constructors()
            .typed_uint8clamped_array(),
        TypedArrayKind::Int16 => context.intrinsics().constructors().typed_int16_array(),
        TypedArrayKind::Uint16 => context.intrinsics().constructors().typed_uint16_array(),
        TypedArrayKind::Int32 => context.intrinsics().constructors().typed_int32_array(),
        TypedArrayKind::Uint32 => context.intrinsics().constructors().typed_uint32_array(),
        TypedArrayKind::BigInt64 => context.intrinsics().constructors().typed_bigint64_array(),
        TypedArrayKind::BigUint64 => context.intrinsics().constructors().typed_biguint64_array(),
        TypedArrayKind::Float32 => context.intrinsics().constructors().typed_float32_array(),
        TypedArrayKind::Float64 => context.intrinsics().constructors().typed_float64_array(),
    };
    let buf_obj: JsObject = buffer.into();
    let obj = ctor.constructor().construct(
        &[
            JsValue::from(buf_obj),
            JsValue::from(byte_offset as f64),
            JsValue::from(length as f64),
        ],
        None,
        context,
    )?;
    let _ = JsTypedArray::from_object(obj.clone()).map_err(|_| invariant(context))?;
    Ok(obj.into())
}

pub(super) fn restore(message: &SerializedMessage, context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let mut values = Vec::<JsValue>::new();
    message_reserve(&mut values, message.nodes.len(), context)?;
    // Pass 1: allocate primitives, ArrayBuffers, Dates, RegExps, Errors, and empty containers
    for node in &message.nodes {
        modern::check_context(context)?;
        let value = match node {
            Node::Undefined => JsValue::undefined(),
            Node::Null => JsValue::null(),
            Node::Boolean(value) => (*value).into(),
            Node::Number(value) => (*value).into(),
            Node::String(units) => JsString::from(units.as_slice()).into(),
            Node::BigInt { negative, digits } => {
                let sign = RawBigInt::from(if *negative { -1 } else { 1 }).sign();
                JsBigInt::from(RawBigInt::from_slice(sign, digits)).into()
            }
            Node::BoxedPrimitive(BoxedKind::Boolean(b)) => context
                .intrinsics()
                .constructors()
                .boolean()
                .constructor()
                .construct(&[(*b).into()], None, context)?
                .into(),
            Node::BoxedPrimitive(BoxedKind::Number(n)) => context
                .intrinsics()
                .constructors()
                .number()
                .constructor()
                .construct(&[(*n).into()], None, context)?
                .into(),
            Node::BoxedString(units) => context
                .intrinsics()
                .constructors()
                .string()
                .constructor()
                .construct(&[JsString::from(units.as_slice()).into()], None, context)?
                .into(),
            Node::Date(ms) => context
                .intrinsics()
                .constructors()
                .date()
                .constructor()
                .construct(&[(*ms).into()], None, context)?
                .into(),
            Node::RegExp { pattern, flags } => JsRegExp::new(
                JsString::from(pattern.as_str()),
                JsString::from(flags.as_str()),
                context,
            )?
            .into(),
            Node::ArrayBuffer(bytes) => {
                let ab = JsArrayBuffer::new(bytes.len(), context)?;
                if let Some(mut slice) = ab.data_mut() {
                    slice.copy_from_slice(bytes);
                }
                ab.into()
            }
            Node::TypedArray { .. } | Node::DataView { .. } => JsValue::undefined(),
            Node::Map { .. } => JsMap::new(context).into(),
            Node::Set { .. } => JsSet::new(context).into(),
            Node::Error {
                kind_name,
                message,
                stack,
            } => {
                if let Some(dom_name) = kind_name.strip_prefix("DOMException:") {
                    let msg_str = message
                        .as_ref()
                        .map(|u| String::from_utf16_lossy(u))
                        .unwrap_or_default();
                    let err = dom_exception::exception(dom_name, &msg_str, context)?;
                    err.into_opaque(context)?
                } else {
                    let native = match kind_name.as_str() {
                        "TypeError" => JsNativeError::typ(),
                        "RangeError" => JsNativeError::range(),
                        "SyntaxError" => JsNativeError::syntax(),
                        "ReferenceError" => JsNativeError::reference(),
                        "URIError" => JsNativeError::uri(),
                        "EvalError" => JsNativeError::eval(),
                        _ => JsNativeError::error(),
                    };
                    let err_obj = native.into_opaque(context);
                    if let Some(msg) = message {
                        err_obj.set(
                            js_string!("message"),
                            JsString::from(msg.as_slice()),
                            false,
                            context,
                        )?;
                    }
                    if let Some(st) = stack {
                        let _ = err_obj.set(
                            js_string!("stack"),
                            JsString::from(st.as_slice()),
                            false,
                            context,
                        );
                    }
                    err_obj.into()
                }
            }
            Node::Object { array_length, .. } => {
                let object = if let Some(length) = array_length {
                    let result = JsArray::new(context);
                    modern::check_context(context)?;
                    let object: JsObject = result.map_err(|_| invariant(context))?.into();
                    let result = object.define_property_or_throw(
                        js_string!("length"),
                        PropertyDescriptor::builder().value(*length),
                        context,
                    );
                    modern::check_context(context)?;
                    result.map_err(|_| invariant(context))?;
                    object
                } else {
                    JsObject::with_object_proto(context.intrinsics())
                };
                object.into()
            }
        };
        modern::check_context(context)?;
        values.push(value);
    }
    // Pass 1b: construct TypedArrays and DataViews now that ArrayBuffers exist
    for (index, node) in message.nodes.iter().enumerate() {
        match node {
            Node::TypedArray {
                kind,
                buffer,
                byte_offset,
                byte_length,
            } => {
                let buf_obj = values
                    .get(*buffer)
                    .and_then(JsValue::as_object)
                    .ok_or_else(|| invariant(context))?;
                let ab = JsArrayBuffer::from_object(buf_obj).map_err(|_| invariant(context))?;
                values[index] =
                    construct_typed_array(*kind, ab, *byte_offset, *byte_length, context)?;
            }
            Node::DataView {
                buffer,
                byte_offset,
                byte_length,
            } => {
                let buf_obj = values
                    .get(*buffer)
                    .and_then(JsValue::as_object)
                    .ok_or_else(|| invariant(context))?;
                let dv = context
                    .intrinsics()
                    .constructors()
                    .data_view()
                    .constructor()
                    .construct(
                        &[
                            JsValue::from(buf_obj),
                            JsValue::from(*byte_offset as f64),
                            JsValue::from(*byte_length as f64),
                        ],
                        None,
                        context,
                    )?;
                values[index] = dv.into();
            }
            _ => {}
        }
    }
    // Pass 2: wire Object properties, Map entries, and Set values
    for (index, node) in message.nodes.iter().enumerate() {
        match node {
            Node::Object { properties, .. } => {
                let object = values
                    .get(index)
                    .and_then(JsValue::as_object)
                    .ok_or_else(|| invariant(context))?;
                for (key, child) in properties {
                    modern::check_context(context)?;
                    let value = values
                        .get(*child)
                        .ok_or_else(|| invariant(context))?
                        .clone();
                    let result =
                        object.create_data_property_or_throw(restored_key(key), value, context);
                    modern::check_context(context)?;
                    result.map_err(|_| invariant(context))?;
                }
            }
            Node::Map { entries } => {
                let obj = values
                    .get(index)
                    .and_then(JsValue::as_object)
                    .ok_or_else(|| invariant(context))?;
                let map = JsMap::from_object(obj).map_err(|_| invariant(context))?;
                for (kid, vid) in entries {
                    let k = values.get(*kid).ok_or_else(|| invariant(context))?.clone();
                    let v = values.get(*vid).ok_or_else(|| invariant(context))?.clone();
                    map.set(k, v, context)?;
                }
            }
            Node::Set { values: elems } => {
                let obj = values
                    .get(index)
                    .and_then(JsValue::as_object)
                    .ok_or_else(|| invariant(context))?;
                let set = JsSet::from_object(obj).map_err(|_| invariant(context))?;
                for vid in elems {
                    let v = values.get(*vid).ok_or_else(|| invariant(context))?.clone();
                    set.add(v, context)?;
                }
            }
            _ => {}
        }
    }
    modern::check_context(context)?;
    values
        .get(message.root)
        .cloned()
        .ok_or_else(|| invariant(context))
}

pub(super) fn to_json_lossy(message: &SerializedMessage) -> Option<String> {
    fn visit(id: NodeId, nodes: &[Node], depth: usize) -> Option<serde_json::Value> {
        if depth > 32 {
            return None;
        }
        match nodes.get(id)? {
            Node::Undefined | Node::Null => Some(serde_json::Value::Null),
            Node::Boolean(b) | Node::BoxedPrimitive(BoxedKind::Boolean(b)) => {
                Some(serde_json::Value::Bool(*b))
            }
            Node::Number(n) | Node::BoxedPrimitive(BoxedKind::Number(n)) | Node::Date(n) => {
                serde_json::Number::from_f64(*n)
                    .map(serde_json::Value::Number)
                    .or(Some(serde_json::Value::Null))
            }
            Node::String(u) | Node::BoxedString(u) => {
                Some(serde_json::Value::String(String::from_utf16_lossy(u)))
            }
            Node::Object {
                array_length: Some(len),
                properties,
            } => {
                let cap = (*len as usize).min(4096);
                let mut arr = vec![serde_json::Value::Null; cap];
                for (k, cid) in properties {
                    if let Key::Index(i) = k
                        && (*i as usize) < cap
                        && let Some(v) = visit(*cid, nodes, depth + 1)
                    {
                        arr[*i as usize] = v;
                    }
                }
                Some(serde_json::Value::Array(arr))
            }
            Node::Object {
                array_length: None,
                properties,
            } => {
                let mut map = serde_json::Map::new();
                for (k, cid) in properties {
                    let key_str = match k {
                        Key::Index(i) => i.to_string(),
                        Key::String(u) => String::from_utf16_lossy(u),
                    };
                    if let Some(v) = visit(*cid, nodes, depth + 1) {
                        map.insert(key_str, v);
                    }
                }
                Some(serde_json::Value::Object(map))
            }
            _ => Some(serde_json::Value::Null),
        }
    }
    let val = visit(message.root, &message.nodes, 0)?;
    serde_json::to_string(&val).ok()
}

pub(super) fn restore_json_str(json: &str, context: &mut Context) -> JsResult<JsValue> {
    fn from_serde(val: &serde_json::Value, context: &mut Context) -> JsResult<JsValue> {
        Ok(match val {
            serde_json::Value::Null => JsValue::null(),
            serde_json::Value::Bool(b) => (*b).into(),
            serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0).into(),
            serde_json::Value::String(s) => JsString::from(s.as_str()).into(),
            serde_json::Value::Array(items) => {
                let arr = JsArray::new(context)?;
                for item in items {
                    let v = from_serde(item, context)?;
                    arr.push(v, context)?;
                }
                arr.into()
            }
            serde_json::Value::Object(map) => {
                let obj = JsObject::with_object_proto(context.intrinsics());
                for (k, v) in map {
                    let js_v = from_serde(v, context)?;
                    obj.create_data_property_or_throw(JsString::from(k.as_str()), js_v, context)?;
                }
                obj.into()
            }
        })
    }
    let parsed: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::Value::Null);
    from_serde(&parsed, context)
}
