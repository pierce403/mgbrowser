//! The explicitly partial, worker-local Window message clone domain.
//!
//! A caller holds the shared work permit before entering this module. The
//! completed arena contains only owned Rust data and scalar edges: neither
//! cycles nor queued messages keep any sender-side GC object alive. Temporary
//! source handles are ordinary strong external Boa roots while getters run.
use super::{dom_exception, message_charge, message_reserve};
use boa_engine::{
    Context, JsBigInt, JsError, JsResult, JsString, JsSymbol, JsValue,
    bigint::RawBigInt,
    builtins::{
        array_buffer::{ArrayBuffer, SharedArrayBuffer},
        dataview::DataView,
        date::Date,
        error::Error,
        map::ordered_map::OrderedMap,
        regexp::RegExp,
        set::ordered_set::OrderedSet,
        typed_array::TypedArray,
    },
    js_string,
    object::{JsObject, builtins::JsArray},
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

/// No JsObject, JsValue, Rc edge or hidden source root is stored here.
pub(super) struct SerializedMessage {
    root: NodeId,
    nodes: Vec<Node>,
}

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
    Object {
        array_length: Option<u32>,
        properties: Vec<(Key, NodeId)>,
    },
}

enum Key {
    Index(u32),
    String(Vec<u16>),
}

struct Frame {
    source: JsObject,
    node: NodeId,
    keys: Vec<PropertyKey>,
    next: usize,
}

// A fallibly reserved open-addressed table avoids an uncharged native HashMap
// allocation and quadratic source-identity scans. Every occupied slot itself
// owns a strong source handle, so an address cannot be reused during getter GC.
// This is local Rust state, not a GC-heap map with ignored tracing.
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

fn copy_key(key: &PropertyKey, context: &mut Context) -> JsResult<Key> {
    match key {
        PropertyKey::Index(index) => Ok(Key::Index(index.get())),
        PropertyKey::String(value) => Ok(Key::String(copy_string(value, context)?)),
        PropertyKey::Symbol(_) => Err(invariant(context)),
    }
}

// Classification is entirely native. It never consults a page constructor,
// prototype, toStringTag, iterator or Proxy trap. Unknown host/exotic objects
// are noncloneable, not silently serialized as empty ordinary records.
fn check_object(object: &JsObject, context: &mut Context) -> JsResult<()> {
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
    // An actual Array has a nonaccessor, nonconfigurable own length. Do not
    // invoke a mutable global constructor or allocate proportional to length.
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
    // The engine's opaque scratch allocation is already admitted by the
    // production System allocator. Admit its actual retained capacity before
    // adopting the Vec as Mg builder state; no property-value prewalk exists.
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

fn intern(
    value: &JsValue,
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
            check_object(&object, context)?;
            let array_length = array_length(&object, context)?;
            message_reserve(nodes, 1, context)?;
            let id = nodes.len();
            nodes.push(Node::Object {
                array_length,
                properties: Vec::new(),
            });
            // Install the placeholder/memo before following an edge, including
            // self-cycles. No arbitrary JavaScript runs until all roots exist.
            memo.insert(object.clone(), id, context)?;
            let keys = own_enumerable_keys(&object, context)?;
            message_reserve(frames, 1, context)?;
            frames.push(Frame {
                source: object,
                node: id,
                keys,
                next: 0,
            });
            return Ok(id);
        }
    };
    message_reserve(nodes, 1, context)?;
    let id = nodes.len();
    nodes.push(node);
    Ok(id)
}

/// Serialize synchronously after caller admission. Getters may re-enter the
/// host; no native object/State/policy borrow survives an arbitrary JS call.
pub(super) fn serialize(value: &JsValue, context: &mut Context) -> JsResult<SerializedMessage> {
    modern::check_context(context)?;
    let mut nodes = Vec::new();
    let mut memo = Memo::default();
    let mut frames = Vec::new();
    let root = intern(value, &mut nodes, &mut memo, &mut frames, context)?;
    while !frames.is_empty() {
        modern::check_context(context)?;
        let step = {
            let frame = frames.last_mut().ok_or_else(|| invariant(context))?;
            if frame.next == frame.keys.len() {
                None
            } else {
                let key = frame.keys[frame.next].clone();
                frame.next += 1;
                Some((frame.source.clone(), frame.node, key))
            }
        };
        let Some((source, parent, key)) = step else {
            frames.pop();
            continue;
        };
        // Only ownership is rechecked. A saved key made nonenumerable by an
        // earlier getter is still read; deletion must not invoke an inherited
        // getter. Supported native brands make this check trap-free.
        let result = source.has_own_property(key.clone(), context);
        modern::check_context(context)?;
        if !result? {
            continue;
        }
        let owned_key = copy_key(&key, context)?;
        let result = source.get(key, context);
        modern::check_context(context)?;
        let value = result?;
        let child = intern(&value, &mut nodes, &mut memo, &mut frames, context)?;
        let Some(Node::Object { properties, .. }) = nodes.get_mut(parent) else {
            return Err(invariant(context));
        };
        message_reserve(properties, 1, context)?;
        properties.push((owned_key, child));
        // A newly interned child frame is now at the top, so its entire subtree
        // precedes reading the parent's next key. No recursive Rust traversal.
    }
    modern::check_context(context)?;
    // Memo and frames drop here. The returned graph contains no sender roots.
    Ok(SerializedMessage { root, nodes })
}

fn restored_key(key: &Key) -> PropertyKey {
    match key {
        Key::Index(index) => (*index).into(),
        Key::String(units) => JsString::from(units.as_slice()).into(),
    }
}

/// Restore intrinsic skeletons first, then fill data edges. The supported
/// domain invokes no page constructors, inherited setters or deserializer JS.
pub(super) fn restore(message: &SerializedMessage, context: &mut Context) -> JsResult<JsValue> {
    modern::check_context(context)?;
    let mut values = Vec::<JsValue>::new();
    message_reserve(&mut values, message.nodes.len(), context)?;
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
    for (index, node) in message.nodes.iter().enumerate() {
        let Node::Object { properties, .. } = node else {
            continue;
        };
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
            let result = object.create_data_property_or_throw(restored_key(key), value, context);
            modern::check_context(context)?;
            // These are fresh intrinsic extensible skeletons and only native
            // data-property writes. Failure is an invariant/resource failure,
            // not a recoverable deserialization failure or page setter error.
            result.map_err(|_| invariant(context))?;
        }
    }
    modern::check_context(context)?;
    values
        .get(message.root)
        .cloned()
        .ok_or_else(|| invariant(context))
}
