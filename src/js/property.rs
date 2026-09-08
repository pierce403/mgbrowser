//! Bounded descriptor creation and genuine accessor ownership. The public
//! definition target is always a fresh ordinary object; this is not a general
//! defineProperty implementation or a Host reflection capability.
use super::*;
use std::cmp::Ordering;
use std::mem::size_of;

const BLOCK: usize = 16;

#[derive(Debug)]
pub(super) enum Stored {
    Inline(Value),
    Accessor(Box<AccessorPair>),
}

#[derive(Debug)]
pub(super) struct AccessorPair {
    get: Value,
    set: Value,
}

impl Stored {
    // Existing host inspection is deliberately non-executing. This view also
    // supports legacy private setup; ordinary JS reads use Property::read_value.
    pub(super) fn raw(&self) -> &Value {
        match self {
            Self::Inline(value) => value,
            Self::Accessor(pair) => &pair.get,
        }
    }
}

#[cfg(test)]
impl PartialEq<Value> for Stored {
    fn eq(&self, other: &Value) -> bool {
        matches!(self, Self::Inline(value) if value == other)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum WriteAction {
    Own,
    Ignore,
    Setter { object: usize, index: usize },
}

impl Property {
    pub(super) fn read_value(&self) -> Eval<&Value> {
        if matches!(self.value, Stored::Accessor(_)) && !self.getter {
            return Err(Fault::Fatal("Invalid JavaScript accessor storage".into()));
        }
        // A legacy host-installed getter can be overwritten by public setup.
        // Its noncallable value must still fail through ordinary call dispatch.
        Ok(self.value.raw())
    }

    pub(super) fn diagnostic_text(&self) -> String {
        if self.getter {
            "[accessor]".into()
        } else {
            self.value.raw().as_text()
        }
    }

    pub(super) fn write_action(
        &self,
        object: usize,
        index: usize,
        host_adapter: bool,
    ) -> Eval<WriteAction> {
        match (&self.value, self.getter) {
            (Stored::Accessor(_), false) => {
                Err(Fault::Fatal("Invalid JavaScript accessor storage".into()))
            }
            (Stored::Accessor(_), true) => Ok(WriteAction::Setter { object, index }),
            (Stored::Inline(_), true) if !host_adapter => Ok(WriteAction::Ignore),
            (Stored::Inline(_), _) => Ok(if self.writable {
                WriteAction::Own
            } else {
                WriteAction::Ignore
            }),
        }
    }
}

struct DescriptorKey {
    key: PropertyKey,
    order: usize,
}

struct Shape {
    storage: usize,
    names: &'static [&'static str],
    available: u32,
    indexed: Indexed,
}

enum Indexed {
    None,
    String(usize),
    Array(usize),
}

impl Runtime {
    pub(super) fn invoke_property_setter(
        &mut self,
        object: usize,
        index: usize,
        receiver: &Value,
        value: Value,
        host: &mut impl Host,
    ) -> Eval<()> {
        // A setter creates a real new one-slot argument vector. It does not
        // clone the already-owned assigned payload or the accessor pair.
        self.budget.allocate(64)?;
        let property = self
            .objects
            .get(object)
            .and_then(|object| object.properties.get(index))
            .ok_or_else(|| Fault::Fatal("Invalid JavaScript setter location".into()))?;
        let Stored::Accessor(pair) = &property.value else {
            return Err(Fault::Fatal("Invalid JavaScript setter storage".into()));
        };
        if !property.getter || !pair.set.callable() {
            return Err(Fault::Fatal("Invalid JavaScript setter storage".into()));
        }
        let setter = self.budget.copy(&pair.set)?;
        let receiver = self.copy(receiver)?;
        self.call(setter, receiver, vec![value], None, host)?;
        Ok(())
    }

    fn descriptor_shape(&mut self, owner: PrototypeIdentity) -> Eval<Shape> {
        let storage = self.identity_storage(owner)?;
        let names = match owner {
            PrototypeIdentity::Object(_) => &[][..],
            PrototypeIdentity::Function(id) => match &self.functions[id].kind {
                FunctionKind::Ordinary(_) => &["length", "name"][..],
                FunctionKind::Bound(_) => &["length", "caller", "arguments"][..],
            },
            PrototypeIdentity::Native(id) => native_virtual_names(&self.native_properties[id].0),
        };
        if names.len() > 32 {
            return Err(Fault::Fatal(
                "Invalid JavaScript descriptor metadata".into(),
            ));
        }
        let mut available = 0;
        for (index, name) in names.iter().enumerate() {
            self.budget.step()?;
            if let PrototypeIdentity::Native(id) = owner
                && self.native_identity_deleted(id, name)?
            {
                continue;
            }
            available |= 1 << index;
        }
        let entry = &self.objects[storage];
        let indexed = match (&entry.boxed, &entry.array) {
            (Some(Value::String(units)), _) => Indexed::String(units.len()),
            (_, Some(values)) => Indexed::Array(values.len()),
            _ => Indexed::None,
        };
        Ok(Shape {
            storage,
            names,
            available,
            indexed,
        })
    }

    // No callbacks here. Each call is paid by its bounded metadata scan. Name
    // comparisons are against a fixed small native table; index parsing is at
    // most ten ASCII bytes, and Symbols never alias string metadata.
    fn descriptor_virtual_key(&self, shape: &Shape, key: &PropertyKey) -> bool {
        let PropertyKey::String(key) = key else {
            return false;
        };
        if shape
            .names
            .iter()
            .enumerate()
            .any(|(index, name)| shape.available & (1 << index) != 0 && key == name)
        {
            return true;
        }
        match shape.indexed {
            Indexed::None => false,
            Indexed::String(length) => {
                key == "length" || array_index(key).is_some_and(|i| i < length)
            }
            Indexed::Array(_) => {
                key == "length"
                    || array_index(key).is_some_and(|i| {
                        self.objects[shape.storage]
                            .array
                            .as_ref()
                            .is_some_and(|values| values.get(i).is_some_and(Option::is_some))
                    })
            }
        }
    }

    fn descriptor_key_count(&mut self, shape: &Shape) -> Eval<usize> {
        let mut count = shape.available.count_ones() as usize;
        match shape.indexed {
            Indexed::None => {}
            Indexed::String(length) => count = count.saturating_add(length).saturating_add(1),
            Indexed::Array(length) => {
                count += 1; // Nonenumerable length is still an own key.
                for index in 0..length {
                    self.budget.step()?;
                    if self.objects[shape.storage].array.as_ref().unwrap()[index].is_some() {
                        count += 1;
                    }
                }
            }
        }
        if count > MAX_ARRAY {
            return Err(Fault::Fatal(
                "JavaScript descriptor key limit exhausted".into(),
            ));
        }
        for property in &self.objects[shape.storage].properties {
            self.budget.step()?;
            if !self.descriptor_virtual_key(shape, &property.key) {
                count += 1;
                if count > MAX_ARRAY {
                    return Err(Fault::Fatal(
                        "JavaScript descriptor key limit exhausted".into(),
                    ));
                }
            }
        }
        Ok(count)
    }

    fn descriptor_string_key(&mut self, keys: &mut Vec<DescriptorKey>, name: &str) -> Eval<()> {
        self.budget.allocate(
            name.len()
                .saturating_add(if name.is_empty() { 0 } else { BLOCK }),
        )?;
        keys.push(DescriptorKey {
            key: PropertyKey::String(name.to_owned()),
            order: keys.len(),
        });
        Ok(())
    }

    fn descriptor_keys(&mut self, owner: PrototypeIdentity) -> Eval<Vec<DescriptorKey>> {
        let shape = self.descriptor_shape(owner)?;
        let count = self.descriptor_key_count(&shape)?;
        self.budget.allocate(buffer_bytes::<DescriptorKey>(count))?;
        let mut keys = Vec::with_capacity(count);
        let length = match shape.indexed {
            Indexed::None => 0,
            Indexed::String(n) | Indexed::Array(n) => n,
        };
        for index in 0..length {
            self.budget.step()?;
            let present = match shape.indexed {
                Indexed::Array(_) => {
                    self.objects[shape.storage].array.as_ref().unwrap()[index].is_some()
                }
                _ => true,
            };
            if present {
                let name = array::IndexKey::new(index, &mut self.budget)?;
                self.descriptor_string_key(&mut keys, name.as_str())?;
            }
        }
        if !matches!(shape.indexed, Indexed::None) {
            self.descriptor_string_key(&mut keys, "length")?;
        }
        for (index, name) in shape.names.iter().enumerate() {
            if shape.available & (1 << index) != 0 {
                self.descriptor_string_key(&mut keys, name)?;
            }
        }
        for index in 0..self.objects[shape.storage].properties.len() {
            self.budget.step()?;
            let property = &self.objects[shape.storage].properties[index];
            if self.descriptor_virtual_key(&shape, &property.key) {
                continue;
            }
            let key = match &property.key {
                PropertyKey::String(name) => {
                    self.budget
                        .allocate(name.len().saturating_add(if name.is_empty() {
                            0
                        } else {
                            BLOCK
                        }))?;
                    PropertyKey::String(name.clone())
                }
                PropertyKey::Symbol(symbol) => {
                    let symbol = symbol.clone();
                    self.admit(&Value::Symbol(symbol.clone()))?;
                    PropertyKey::Symbol(symbol)
                }
            };
            keys.push(DescriptorKey {
                key,
                order: keys.len(),
            });
        }
        if keys.len() != count || keys.capacity() < count {
            return Err(Fault::Fatal(
                "Invalid JavaScript descriptor key storage".into(),
            ));
        }
        order_keys(&mut self.budget, &mut keys)?;
        Ok(keys)
    }

    #[inline(never)]
    pub(super) fn create_described_object(
        &mut self,
        object: usize,
        map: Value,
        host: &mut impl Host,
    ) -> Eval<Value> {
        if matches!(map, Value::Host(_)) {
            return Err(unsupported("Object.create host descriptor maps"));
        }
        if matches!(map, Value::Null | Value::Undefined) {
            return Err(exception("TypeError: null or undefined descriptor map"));
        }
        let map = self.boxed(map)?;
        let owner = self.object_identity(&map)?;
        let keys = self.descriptor_keys(owner)?;
        self.budget.allocate(buffer_bytes::<Property>(keys.len()))?;
        let mut properties = Vec::with_capacity(keys.len());
        for entry in keys {
            let key = match &entry.key {
                PropertyKey::String(name) => KeyRef::String(name),
                PropertyKey::Symbol(symbol) => KeyRef::Symbol(symbol),
            };
            let enumerable = self
                .own_descriptor(owner, key)?
                .is_some_and(|d| d.enumerable);
            if !enumerable {
                continue;
            }
            let descriptor = self.get_key(&map, &entry.key, host)?;
            self.convert_descriptor(entry.key, descriptor, &mut properties, host)?;
        }
        let target = self
            .objects
            .get_mut(object)
            .ok_or_else(|| Fault::Fatal("Invalid JavaScript descriptor target".into()))?;
        if !target.properties.is_empty() || target.array.is_some() || target.boxed.is_some() {
            return Err(Fault::Fatal("Invalid JavaScript descriptor target".into()));
        }
        target.properties = properties;
        Ok(Value::Object(object))
    }

    #[inline(never)]
    fn convert_descriptor(
        &mut self,
        key: PropertyKey,
        descriptor: Value,
        properties: &mut Vec<Property>,
        host: &mut impl Host,
    ) -> Eval<()> {
        if matches!(descriptor, Value::Host(_)) {
            return Err(unsupported("Object.create host property descriptors"));
        }
        if descriptor.primitive() {
            return Err(exception(
                "TypeError: property descriptor must be an object",
            ));
        }
        self.object_identity(&descriptor)?;
        let mut enumerable = false;
        let mut configurable = false;
        let mut writable = false;
        let mut value = Value::Undefined;
        let mut get = Value::Undefined;
        let mut set = Value::Undefined;
        let mut data = false;
        let mut accessor = false;
        for field in [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ] {
            if !self.has_property(&descriptor, KeyRef::String(field), false)? {
                continue;
            }
            let read = self.get(&descriptor, field, host)?;
            match field {
                "enumerable" => enumerable = read.truthy(),
                "configurable" => configurable = read.truthy(),
                "value" => {
                    data = true;
                    value = read;
                }
                "writable" => {
                    data = true;
                    writable = read.truthy();
                }
                "get" | "set" => {
                    if !matches!(read, Value::Undefined) && !read.callable() {
                        return Err(exception(
                            "TypeError: accessor must be callable or undefined",
                        ));
                    }
                    accessor = true;
                    if field == "get" {
                        get = read;
                    } else {
                        set = read;
                    }
                }
                _ => return Err(Fault::Fatal("Invalid JavaScript descriptor field".into())),
            }
        }
        if data && accessor {
            return Err(exception("TypeError: mixed data and accessor descriptor"));
        }
        let stored = if !accessor {
            Stored::Inline(value)
        } else if matches!(set, Value::Undefined) {
            writable = false;
            Stored::Inline(get)
        } else {
            writable = false;
            self.budget
                .allocate(size_of::<AccessorPair>().saturating_add(BLOCK))?;
            Stored::Accessor(Box::new(AccessorPair { get, set }))
        };
        if properties.len() == properties.capacity() {
            return Err(Fault::Fatal(
                "Invalid JavaScript descriptor staging capacity".into(),
            ));
        }
        properties.push(Property {
            key,
            value: stored,
            enumerable,
            configurable,
            writable,
            getter: accessor,
        });
        Ok(())
    }
}

fn buffer_bytes<T>(count: usize) -> usize {
    if count == 0 {
        0
    } else {
        count.saturating_mul(size_of::<T>()).saturating_add(BLOCK)
    }
}

fn key_order(a: &DescriptorKey, b: &DescriptorKey) -> Ordering {
    let rank = |key: &PropertyKey| match key {
        PropertyKey::String(name) => match array_index(name) {
            Some(index) => (0, index),
            None => (1, 0),
        },
        PropertyKey::Symbol(_) => (2, 0),
    };
    rank(&a.key)
        .cmp(&rank(&b.key))
        .then_with(|| a.order.cmp(&b.order))
}

fn order_keys(budget: &mut Budget, keys: &mut [DescriptorKey]) -> Eval<()> {
    let mut ordered = true;
    for pair in keys.windows(2) {
        budget.step()?;
        if key_order(&pair[0], &pair[1]) == Ordering::Greater {
            ordered = false;
            break;
        }
    }
    if ordered {
        return Ok(());
    }
    // Fallible in-place heapsort: no hidden sort scratch allocation and no
    // comparator that continues running after fatal fuel exhaustion.
    fn sift(
        budget: &mut Budget,
        keys: &mut [DescriptorKey],
        mut root: usize,
        end: usize,
    ) -> Eval<()> {
        loop {
            let left = root * 2 + 1;
            if left >= end {
                return Ok(());
            }
            let mut child = left;
            if left + 1 < end {
                budget.step()?;
                if key_order(&keys[left], &keys[left + 1]) == Ordering::Less {
                    child += 1;
                }
            }
            budget.step()?;
            if key_order(&keys[root], &keys[child]) != Ordering::Less {
                return Ok(());
            }
            keys.swap(root, child);
            root = child;
        }
    }
    let len = keys.len();
    for root in (0..len / 2).rev() {
        sift(budget, keys, root, len)?;
    }
    for end in (1..len).rev() {
        keys.swap(0, end);
        sift(budget, keys, 0, end)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestHost {
        calls: usize,
        payload: Option<Vec<u16>>,
        receiver: Option<Value>,
        arguments: Vec<Value>,
    }
    impl Host for TestHost {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected Host Get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected Host Set")
        }
        fn call(
            &mut self,
            name: &str,
            receiver: Value,
            arguments: Vec<Value>,
        ) -> Result<Value, String> {
            self.calls += 1;
            self.receiver = Some(receiver);
            self.arguments = arguments;
            match name {
                "host.payload" => Ok(Value::String(self.payload.take().unwrap())),
                "host.setter" => Ok(Value::Number(999.0)),
                _ => panic!("unexpected Host call: {name}"),
            }
        }
    }
    fn id(value: Value) -> usize {
        let Value::Object(id) = value else {
            panic!("expected object");
        };
        id
    }
    fn leave(runtime: &mut Runtime, remaining: usize) {
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
            .unwrap();
    }
    fn property_pair(runtime: &mut Runtime, object: usize, get: Value, set: Value) {
        runtime
            .put_own(object, "p", Value::Undefined, true)
            .unwrap();
        runtime
            .budget
            .allocate(size_of::<AccessorPair>() + BLOCK)
            .unwrap();
        let property = runtime.objects[object].properties.last_mut().unwrap();
        property.value = Stored::Accessor(Box::new(AccessorPair { get, set }));
        property.getter = true;
        property.writable = false;
    }
    fn pair_pointer(runtime: &Runtime, object: usize) -> *const AccessorPair {
        let Stored::Accessor(pair) = &runtime.objects[object].properties[0].value else {
            panic!();
        };
        &**pair
    }

    #[test]
    fn compact_storage_layout_and_legacy_bootstrap_are_preserved() {
        assert_eq!(size_of::<Value>(), 32);
        assert_eq!(size_of::<Stored>(), 32);
        assert_eq!(size_of::<PropertyKey>(), 24);
        assert_eq!(size_of::<Property>(), 64);
        assert_eq!(size_of::<AccessorPair>(), 64);
        assert_eq!(size_of::<Fault>(), 40);
        assert_eq!(size_of::<Eval<Value>>(), 40);
        assert_eq!(size_of::<Eval<Flow>>(), 64);
        assert_eq!(size_of::<Eval<Reference>>(), 56);
        assert!(size_of::<Object>() <= 128);
        let mut runtime = Runtime::new();
        assert_eq!(runtime.allocation_report().phases.bootstrap, 26_880);
        let before = runtime.allocation_report();
        let ticks = runtime.budget.fuel;
        let created = runtime
            .native(
                "Object.create",
                Value::Undefined,
                vec![Value::Null],
                &mut TestHost::default(),
            )
            .unwrap();
        assert_eq!(
            runtime.allocation_report().phases.runtime - before.phases.runtime,
            128
        );
        assert_eq!(runtime.budget.fuel, ticks);
        assert!(runtime.objects[id(created)].properties.is_empty());
        assert_eq!(buffer_bytes::<Property>(0), 0);
        assert_eq!(buffer_bytes::<Property>(1), 80);
        assert_eq!(buffer_bytes::<Property>(usize::MAX), usize::MAX);
    }

    #[test]
    fn snapshot_and_staging_reject_at_precise_pre_get_boundaries() {
        let snapshot_slots = buffer_bytes::<DescriptorKey>(1);
        let key_buffer = 1 + BLOCK;
        let staging = buffer_bytes::<Property>(1);
        for (remaining, requested, accepted) in [
            (snapshot_slots - 1, snapshot_slots, 0),
            (
                snapshot_slots + key_buffer + staging - 1,
                staging,
                snapshot_slots + key_buffer,
            ),
        ] {
            let mut runtime = Runtime::new();
            let map = runtime.object(None, None).unwrap();
            runtime
                .put_own(map, "p", Value::Native("host.map".into()), true)
                .unwrap();
            runtime.objects[map].properties[0].getter = true;
            runtime.objects[map].properties[0].writable = false;
            let target = runtime.object(None, None).unwrap();
            leave(&mut runtime, remaining);
            let before = runtime.allocation_report();
            let mut host = TestHost::default();
            assert!(matches!(
                runtime.create_described_object(target, Value::Object(map), &mut host),
                Err(Fault::Fatal(_))
            ));
            assert_eq!(host.calls, 0);
            assert!(runtime.objects[target].properties.is_empty());
            let after = runtime.allocation_report();
            assert_eq!(
                after.accepted_bytes - before.accepted_bytes,
                accepted as u64
            );
            assert_eq!(
                after.first_rejected.unwrap().requested_bytes,
                requested as u64
            );
            assert_eq!(
                after.first_rejected.unwrap().phase,
                AllocationPhase::Runtime
            );
        }
    }

    #[test]
    fn completed_setter_pair_is_prepaid_and_invalid_state_is_fallible() {
        for remaining in [79, 80] {
            let mut runtime = Runtime::new();
            let descriptor = runtime
                .execute(
                    "({get:undefined,set:function(value){}});",
                    &mut TestHost::default(),
                )
                .unwrap();
            let mut staged = Vec::with_capacity(1);
            leave(&mut runtime, remaining);
            let before = runtime.allocation_report();
            let outcome = runtime.convert_descriptor(
                PropertyKey::String("p".into()),
                descriptor,
                &mut staged,
                &mut TestHost::default(),
            );
            if remaining == 79 {
                assert!(matches!(outcome, Err(Fault::Fatal(_))));
                assert!(staged.is_empty());
                assert_eq!(
                    runtime
                        .allocation_report()
                        .first_rejected
                        .unwrap()
                        .requested_bytes,
                    80
                );
                assert_eq!(
                    runtime.allocation_report().accepted_bytes,
                    before.accepted_bytes
                );
            } else {
                outcome.unwrap();
                assert_eq!(
                    runtime.allocation_report().accepted_bytes - before.accepted_bytes,
                    80
                );
                assert!(matches!(staged[0].value, Stored::Accessor(_)));
                assert!(matches!(staged[0].read_value().unwrap(), Value::Undefined));
                staged[0].getter = false;
                assert!(matches!(staged[0].read_value(), Err(Fault::Fatal(_))));
                assert!(matches!(
                    staged[0].write_action(0, 0, false),
                    Err(Fault::Fatal(_))
                ));
            }
        }
        let mut runtime = Runtime::new();
        let object = runtime.object(None, None).unwrap();
        property_pair(
            &mut runtime,
            object,
            Value::Undefined,
            Value::Native("host.setter".into()),
        );
        runtime.objects[object].properties[0].getter = false;
        let mut host = TestHost::default();
        assert!(matches!(
            runtime.get(&Value::Object(object), "p", &mut host),
            Err(Fault::Fatal(_))
        ));
        assert_eq!(host.calls, 0);
    }

    #[test]
    fn setter_slot_then_callable_then_receiver_copies_precede_callback() {
        let setter = "host.setter";
        let receiver = "owned.receiver";
        for (remaining, requested) in [
            (63, 64),
            (64 + setter.len() - 1, setter.len()),
            (64 + setter.len() + receiver.len() - 1, receiver.len()),
        ] {
            let mut runtime = Runtime::new();
            let object = runtime.object(None, None).unwrap();
            property_pair(
                &mut runtime,
                object,
                Value::Undefined,
                Value::Native(setter.into()),
            );
            let pointer = pair_pointer(&runtime, object);
            leave(&mut runtime, remaining);
            let mut host = TestHost::default();
            assert!(matches!(
                runtime.invoke_property_setter(
                    object,
                    0,
                    &Value::Host(receiver.into()),
                    Value::Number(3.0),
                    &mut host
                ),
                Err(Fault::Fatal(_))
            ));
            assert_eq!(host.calls, 0);
            assert_eq!(
                runtime
                    .allocation_report()
                    .first_rejected
                    .unwrap()
                    .requested_bytes,
                requested as u64
            );
            assert_eq!(pair_pointer(&runtime, object), pointer);
        }
        let mut runtime = Runtime::new();
        let object = runtime.object(None, None).unwrap();
        property_pair(
            &mut runtime,
            object,
            Value::Undefined,
            Value::Native(setter.into()),
        );
        let pointer = pair_pointer(&runtime, object);
        let payload = vec![0xd800, b'x' as u16];
        let payload_pointer = payload.as_ptr();
        let before = runtime.allocation_report();
        let mut host = TestHost::default();
        runtime
            .invoke_property_setter(
                object,
                0,
                &Value::Host(receiver.into()),
                Value::String(payload),
                &mut host,
            )
            .unwrap();
        assert_eq!(host.calls, 1);
        assert_eq!(host.receiver, Some(Value::Host(receiver.into())));
        let Value::String(argument) = &host.arguments[0] else {
            panic!();
        };
        assert_eq!(argument.as_ptr(), payload_pointer);
        assert_eq!(
            runtime.allocation_report().accepted_bytes - before.accepted_bytes,
            (64 + setter.len() + receiver.len()) as u64
        );
        assert_eq!(pair_pointer(&runtime, object), pointer);
        assert_eq!(runtime.budget.calls, 0);
        assert_eq!(runtime.budget.evaluation_entries, 0);
    }

    #[test]
    fn admitted_getter_payload_moves_into_exact_capacity_result() {
        let mut runtime = Runtime::new();
        let descriptor = runtime.object(None, None).unwrap();
        runtime
            .put_own(
                descriptor,
                "value",
                Value::Native("host.payload".into()),
                true,
            )
            .unwrap();
        runtime.objects[descriptor].properties[0].getter = true;
        runtime.objects[descriptor].properties[0].writable = false;
        let map = runtime.object(None, None).unwrap();
        runtime
            .put_own(map, "p", Value::Object(descriptor), true)
            .unwrap();
        let target = runtime.object(None, None).unwrap();
        let payload = vec![0xd800, 0xdc00, 0xdfff];
        let pointer = payload.as_ptr();
        let mut host = TestHost {
            payload: Some(payload),
            ..TestHost::default()
        };
        let before = runtime.allocation_report();
        assert_eq!(
            runtime
                .create_described_object(target, Value::Object(map), &mut host)
                .unwrap(),
            Value::Object(target)
        );
        assert_eq!(host.calls, 1);
        let Value::String(value) = runtime.objects[target].properties[0].value.raw() else {
            panic!();
        };
        assert_eq!(value.as_ptr(), pointer);
        assert_eq!(value, &[0xd800, 0xdc00, 0xdfff]);
        assert_eq!(runtime.objects[target].properties.capacity(), 1);
        let expected = buffer_bytes::<DescriptorKey>(1)
            + 1
            + BLOCK
            + buffer_bytes::<Property>(1)
            + "host.payload".len()
            + 6;
        assert_eq!(
            runtime.allocation_report().accepted_bytes - before.accepted_bytes,
            expected as u64
        );
    }

    #[test]
    fn key_order_fast_path_and_fallible_sort_have_no_scratch_allocation() {
        let mut runtime = Runtime::new();
        let mut keys = (0..10_000)
            .map(|index| DescriptorKey {
                key: PropertyKey::String(index.to_string()),
                order: index,
            })
            .collect::<Vec<_>>();
        let pointer = keys.as_ptr();
        let report = runtime.allocation_report();
        runtime.budget.fuel = 9_999;
        order_keys(&mut runtime.budget, &mut keys).unwrap();
        assert_eq!(runtime.budget.fuel, 0);
        assert_eq!(runtime.allocation_report(), report);
        assert_eq!(keys.as_ptr(), pointer);
        keys.reverse();
        assert!(matches!(
            order_keys(&mut runtime.budget, &mut keys),
            Err(Fault::Fatal(_))
        ));
        runtime.budget.fuel = MAX_FUEL;
        order_keys(&mut runtime.budget, &mut keys).unwrap();
        assert_eq!(keys.as_ptr(), pointer);
        assert_eq!(runtime.allocation_report(), report);
        assert!(
            keys.windows(2)
                .all(|pair| key_order(&pair[0], &pair[1]) != Ordering::Greater)
        );
    }

    #[test]
    fn legacy_invalid_host_getters_stay_ordinary_and_error_accessors_do_not_execute() {
        for (replacement, source) in [
            (Value::Number(7.0), "setting;"),
            (Value::Undefined, "setting;"),
            (Value::Undefined, "globalThis.setting;"),
        ] {
            let mut runtime = Runtime::new();
            runtime.set_global_accessor(
                "setting",
                "target",
                "setting",
                Value::Native("host.setting".into()),
            );
            runtime.set_global("setting", replacement);
            let mut host = TestHost::default();
            let error = runtime.execute(source, &mut host).unwrap_err();
            assert!(
                error.contains("TypeError: value is not callable"),
                "{error}"
            );
            assert!(!runtime.is_fatal());
            assert_eq!(host.calls, 0);
        }
        let mut runtime = Runtime::new();
        let error = id(runtime
            .execute("new TypeError('safe');", &mut TestHost::default())
            .unwrap());
        property_pair(
            &mut runtime,
            error,
            Value::Native("host.should.not.run".into()),
            Value::Native("host.setter".into()),
        );
        let index = runtime.objects[error]
            .properties
            .iter()
            .position(|p| p.key == "p")
            .unwrap();
        runtime.objects[error].properties[index].key = PropertyKey::String("name".into());
        let before = runtime.allocation_report();
        let ticks = runtime.budget.fuel;
        let diagnostic = runtime.error_diagnostic(error, ErrorKind::Type);
        assert_eq!(diagnostic, "Uncaught JavaScript exception: TypeError: safe");
        assert_eq!(runtime.allocation_report(), before);
        assert_eq!(runtime.budget.fuel, ticks);
    }
}
