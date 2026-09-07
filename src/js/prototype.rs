//! Typed object identity, distinct from its property storage. All prototype
//! operations share these bounded owner/descriptor helpers; no mutable chains.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum KeyRef<'a> {
    String(&'a str),
    Symbol(&'a SymbolHandle),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PropertyDescriptor {
    pub(super) enumerable: bool,
    pub(super) writable: bool,
}

impl Runtime {
    pub(super) fn native_identity(&mut self, name: &str) -> Eval<usize> {
        for (index, (existing, _)) in self.native_properties.iter().enumerate() {
            if enumeration_equal(&mut self.budget, existing, name)? {
                return Ok(index);
            }
        }
        // A record owns one stable UTF-8 name and an ordinary property bag.
        // Existing object limits also bound the native identity table. Reserve
        // before cloning the name or growing the table; failed charges stay paid.
        if self.objects.len() >= MAX_OBJECTS {
            return Err(Fault::Fatal("JavaScript object limit exhausted".into()));
        }
        self.budget.allocate(64usize.saturating_add(name.len()))?;
        let properties = self.object(Some(self.function_prototype), None)?;
        let index = self.native_properties.len();
        self.native_properties.push((name.to_owned(), properties));
        Ok(index)
    }

    pub(super) fn object_identity(&mut self, value: &Value) -> Eval<PrototypeIdentity> {
        let identity = match value {
            Value::Object(id) => PrototypeIdentity::Object(*id),
            Value::Function(id) => PrototypeIdentity::Function(*id),
            Value::Native(name) => PrototypeIdentity::Native(self.native_identity(name)?),
            Value::Host(_) => return Err(unsupported("host prototype identities")),
            _ => return Err(exception("TypeError: expected an object identity")),
        };
        self.identity_storage(identity)?;
        Ok(identity)
    }

    pub(super) fn property_root(&mut self, value: &Value) -> Eval<PrototypeIdentity> {
        let prototype = match value {
            Value::Symbol(_) => self.symbol_prototype,
            Value::String(_) => self.string_prototype,
            Value::Number(_) => self.number_prototype,
            Value::Bool(_) => self.boolean_prototype,
            Value::Null | Value::Undefined => {
                return Err(exception("TypeError: property access on null or undefined"));
            }
            _ => return self.object_identity(value),
        };
        Ok(PrototypeIdentity::Object(prototype))
    }

    pub(super) fn identity_storage(&self, owner: PrototypeIdentity) -> Eval<usize> {
        let id = match owner {
            PrototypeIdentity::Object(id) => id,
            PrototypeIdentity::Function(id) => {
                self.functions
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown function"))?
                    .properties
            }
            PrototypeIdentity::Native(id) => {
                self.native_properties
                    .get(id)
                    .ok_or_else(|| exception("TypeError: unknown native identity"))?
                    .1
            }
        };
        if id >= self.objects.len() {
            return Err(exception("TypeError: unknown object"));
        }
        Ok(id)
    }

    pub(super) fn identity_parent(
        &mut self,
        owner: PrototypeIdentity,
    ) -> Eval<Option<PrototypeIdentity>> {
        self.budget.step()?;
        let id = self.identity_storage(owner)?;
        Ok(self.objects[id].prototype)
    }

    pub(super) fn identity_value(&mut self, owner: PrototypeIdentity) -> Eval<Value> {
        self.identity_storage(owner)?;
        Ok(match owner {
            PrototypeIdentity::Object(id) => Value::Object(id),
            PrototypeIdentity::Function(id) => Value::Function(id),
            PrototypeIdentity::Native(id) => {
                let name = &self.native_properties[id].0;
                self.budget.allocate(name.len())?;
                Value::Native(name.clone())
            }
        })
    }

    pub(super) fn native_identity_deleted(&mut self, id: usize, key: &str) -> Eval<bool> {
        let name = &self.native_properties[id].0;
        for (owner, property) in &self.native_deleted {
            if enumeration_equal(&mut self.budget, owner, name)?
                && enumeration_equal(&mut self.budget, property, key)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn matching_property(&mut self, id: usize, key: KeyRef<'_>) -> Eval<Option<usize>> {
        for (index, property) in self.objects[id].properties.iter().enumerate() {
            let equal = match (&property.key, key) {
                (PropertyKey::String(name), KeyRef::String(key)) => {
                    enumeration_equal(&mut self.budget, name, key)?
                }
                (PropertyKey::Symbol(symbol), KeyRef::Symbol(key)) => {
                    self.budget.step()?;
                    symbol == key
                }
                _ => {
                    self.budget.step()?;
                    false
                }
            };
            if equal {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    pub(super) fn own_descriptor(
        &mut self,
        owner: PrototypeIdentity,
        key: KeyRef<'_>,
    ) -> Eval<Option<PropertyDescriptor>> {
        self.budget.step()?;
        let id = self.identity_storage(owner)?;
        if let KeyRef::String(key) = key {
            if matches!(owner, PrototypeIdentity::Function(_)) && matches!(key, "name" | "length") {
                return Ok(Some(PropertyDescriptor {
                    enumerable: false,
                    writable: false,
                }));
            }
            if let PrototypeIdentity::Native(native) = owner {
                let names = native_virtual_names(&self.native_properties[native].0);
                if matches!(key, "name" | "length")
                    || key == "prototype" && names.contains(&"prototype")
                {
                    return Ok(Some(PropertyDescriptor {
                        enumerable: false,
                        writable: false,
                    }));
                }
            }
            let object = &self.objects[id];
            // Virtual own fields precede retained data properties. In particular
            // RegExp's stored metadata must not become writable through a child.
            if object.regexp.is_some()
                && matches!(key, "source" | "global" | "ignoreCase" | "multiline")
            {
                return Ok(Some(PropertyDescriptor {
                    enumerable: false,
                    writable: false,
                }));
            }
            if let Some(Value::String(units)) = &object.boxed {
                if key == "length" {
                    return Ok(Some(PropertyDescriptor {
                        enumerable: false,
                        writable: false,
                    }));
                }
                if array_index(key).is_some_and(|index| index < units.len()) {
                    return Ok(Some(PropertyDescriptor {
                        enumerable: true,
                        writable: false,
                    }));
                }
            }
            if let Some(array) = &object.array {
                if key == "length" {
                    return Ok(Some(PropertyDescriptor {
                        enumerable: false,
                        writable: true,
                    }));
                }
                if array_index(key)
                    .is_some_and(|index| array.get(index).is_some_and(Option::is_some))
                {
                    return Ok(Some(PropertyDescriptor {
                        enumerable: true,
                        writable: true,
                    }));
                }
            }
        }
        if let Some(index) = self.matching_property(id, key)? {
            let property = &self.objects[id].properties[index];
            return Ok(Some(PropertyDescriptor {
                enumerable: property.enumerable,
                writable: property.writable,
            }));
        }
        if let (PrototypeIdentity::Native(native), KeyRef::String(key)) = (owner, key) {
            if native_virtual_names(&self.native_properties[native].0).contains(&key)
                && !self.native_identity_deleted(native, key)?
            {
                return Ok(Some(PropertyDescriptor {
                    enumerable: false,
                    writable: true,
                }));
            }
        }
        Ok(None)
    }

    fn native_own_value(
        &mut self,
        native: usize,
        key: &str,
        receiver: &Value,
        host: &mut impl Host,
    ) -> Eval<Option<Value>> {
        let storage = self.native_properties[native].1;
        if let Some(value) = self.get_own_value(storage, key, receiver, host)? {
            return Ok(Some(value));
        }
        if self.native_identity_deleted(native, key)? {
            return Ok(None);
        }
        let name = &self.native_properties[native].0;
        if key == "name" {
            let name = match name.as_str() {
                "Symbol.toPrimitive" => "[Symbol.toPrimitive]",
                "Symbol.description" => "get description",
                _ => name.rsplit('.').next().unwrap_or(name),
            };
            self.budget
                .allocate(name.encode_utf16().count().saturating_mul(2))?;
            return Ok(Some(Value::text(name)));
        }
        if key == "prototype" && native_virtual_names(name).contains(&"prototype") {
            let prototype = match name.as_str() {
                "Object" => self.object_prototype,
                "Array" => self.array_prototype,
                "Function" => self.function_prototype,
                "String" => self.string_prototype,
                "Number" => self.number_prototype,
                "Boolean" => self.boolean_prototype,
                "RegExp" => self.regexp_prototype,
                "Symbol" => self.symbol_prototype,
                _ => return Ok(None),
            };
            return Ok(Some(Value::Object(prototype)));
        }
        if matches!(
            (name.as_str(), key),
            ("Array", "isArray")
                | ("String", "fromCharCode")
                | (
                    "Object",
                    "keys"
                        | "create"
                        | "getPrototypeOf"
                        | "getOwnPropertyNames"
                        | "getOwnPropertySymbols"
                )
                | ("Number", "isNaN" | "isFinite" | "isInteger")
        ) {
            self.budget
                .allocate(name.len().saturating_add(1).saturating_add(key.len()))?;
            return Ok(Some(Value::Native(format!("{name}.{key}"))));
        }
        if key == "length" {
            let length = match name.as_str() {
                "Symbol" | "Symbol.toString" | "Symbol.valueOf" | "Symbol.description"
                | "RegExp.toString" => 0.0,
                "parseInt" | "RegExp" => 2.0,
                _ => 1.0,
            };
            return Ok(Some(Value::Number(length)));
        }
        Ok(None)
    }

    pub(super) fn identity_own_value(
        &mut self,
        owner: PrototypeIdentity,
        key: KeyRef<'_>,
        receiver: &Value,
        host: &mut impl Host,
    ) -> Eval<Option<Value>> {
        self.budget.step()?;
        let id = self.identity_storage(owner)?;
        if let KeyRef::String(key) = key {
            match owner {
                PrototypeIdentity::Function(function) => {
                    if key == "length" {
                        return Ok(Some(Value::Number(
                            self.functions[function].code.params.len() as f64,
                        )));
                    }
                    if key == "name" {
                        let name = self.functions[function].code.name.as_deref().unwrap_or("");
                        self.budget
                            .allocate(name.encode_utf16().count().saturating_mul(2))?;
                        return Ok(Some(Value::text(name)));
                    }
                }
                PrototypeIdentity::Native(native) => {
                    return self.native_own_value(native, key, receiver, host);
                }
                PrototypeIdentity::Object(_) => {}
            }
            return self.get_own_value(id, key, receiver, host);
        }
        if let Some(index) = self.matching_property(id, key)? {
            let property = &self.objects[id].properties[index];
            let getter = property.getter;
            let value = self.budget.copy(&property.value)?;
            if getter {
                let receiver = self.copy(receiver)?;
                return self.call(value, receiver, vec![], None, host).map(Some);
            }
            return Ok(Some(value));
        }
        Ok(None)
    }

    pub(super) fn read_property(
        &mut self,
        receiver: &Value,
        key: KeyRef<'_>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let mut current = Some(self.property_root(receiver)?);
        // Preserve the established read boundary: ordinary/native string reads
        // inspect their own fields before the 64-parent walk. Function/primitive
        // string reads and all Symbol reads count their root within 64 owners.
        if matches!(key, KeyRef::String(_))
            && matches!(receiver, Value::Object(_) | Value::Native(_))
        {
            let owner = current.unwrap();
            if let Some(value) = self.identity_own_value(owner, key, receiver, host)? {
                return Ok(value);
            }
            current = self.identity_parent(owner)?;
        }
        for _ in 0..MAX_CALLS {
            let Some(owner) = current else {
                return Ok(Value::Undefined);
            };
            if let Some(value) = self.identity_own_value(owner, key, receiver, host)? {
                return Ok(value);
            }
            current = self.identity_parent(owner)?;
        }
        Err(Fault::Fatal(
            "JavaScript prototype depth limit exhausted".into(),
        ))
    }

    pub(super) fn readonly_property(
        &mut self,
        owner: PrototypeIdentity,
        key: KeyRef<'_>,
    ) -> Eval<bool> {
        let mut current = Some(owner);
        for _ in 0..MAX_CALLS {
            let Some(owner) = current else {
                return Ok(false);
            };
            if let Some(descriptor) = self.own_descriptor(owner, key)? {
                return Ok(!descriptor.writable);
            }
            current = self.identity_parent(owner)?;
        }
        if current.is_none() && matches!(key, KeyRef::String(_)) {
            Ok(false)
        } else {
            Err(Fault::Fatal(
                "JavaScript prototype depth limit exhausted".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoIo;
    impl Host for NoIo {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected host get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected host set")
        }
        fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
            panic!("unexpected host call")
        }
    }

    #[test]
    fn typed_metadata_fits_existing_allowances_without_changing_value_transport() {
        use std::mem::size_of;
        fn transport<T: Send + Sync>() {}
        transport::<Value>();
        assert!(size_of::<Object>() <= 128);
        assert!(size_of::<Property>() <= 128);
        assert!(size_of::<(String, usize)>() <= 64);
        assert_eq!(
            size_of::<Option<PrototypeIdentity>>(),
            size_of::<Option<usize>>()
        );
        eprintln!(
            "prototype layout: Object={} Property={} Identity={} OptionIdentity={} NativeRecord={} Value={}",
            size_of::<Object>(),
            size_of::<Property>(),
            size_of::<PrototypeIdentity>(),
            size_of::<Option<PrototypeIdentity>>(),
            size_of::<(String, usize)>(),
            size_of::<Value>()
        );
    }

    #[test]
    fn first_native_identity_charges_before_retention_and_reuses_storage() {
        let mut runtime = Runtime::new();
        let name = "owned.native.identity".repeat(32);
        let before = runtime.allocation_report();
        let objects = runtime.objects.len();
        let records = runtime.native_properties.len();
        let identity = runtime.native_identity(&name).unwrap();
        assert_eq!(runtime.objects.len(), objects + 1);
        assert_eq!(runtime.native_properties.len(), records + 1);
        assert_eq!(runtime.native_properties[identity].0, name);
        assert_ne!(
            runtime.native_properties[identity].0.as_ptr(),
            name.as_ptr()
        );
        assert_eq!(
            runtime.allocation_report().accepted_bytes - before.accepted_bytes,
            (64 + name.len() + 128) as u64
        );
        let pointer = runtime.native_properties[identity].0.as_ptr();
        let after = runtime.allocation_report();
        assert_eq!(runtime.native_identity(&name).unwrap(), identity);
        assert_eq!(runtime.native_properties[identity].0.as_ptr(), pointer);
        assert_eq!(runtime.allocation_report(), after);
        assert_ne!(
            PrototypeIdentity::Native(identity),
            PrototypeIdentity::Object(runtime.native_properties[identity].1)
        );
    }

    #[test]
    fn rejected_native_retention_does_not_create_a_record_or_property_bag() {
        let mut runtime = Runtime::new();
        let name = "unretained-native".repeat(64);
        let requested = 64 + name.len();
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - requested + 1)
            .unwrap();
        let objects = runtime.objects.len();
        let records = runtime.native_properties.len();
        let accepted = runtime.allocation_report().accepted_bytes;
        assert!(matches!(
            runtime.native_identity(&name),
            Err(Fault::Fatal(_))
        ));
        let report = runtime.allocation_report();
        assert!(report.is_valid());
        assert_eq!(report.accepted_bytes, accepted);
        assert_eq!(
            report.first_rejected.unwrap().requested_bytes,
            requested as u64
        );
        assert_eq!(runtime.objects.len(), objects);
        assert_eq!(runtime.native_properties.len(), records);
        assert!(matches!(
            runtime.native_identity("another new identity"),
            Err(Fault::Fatal(_))
        ));
        assert_eq!(runtime.allocation_report(), report);
    }

    #[test]
    fn returned_native_identity_is_a_real_charged_name_copy() {
        let mut runtime = Runtime::new();
        let name = "identity-return".repeat(64);
        let native = runtime.native_identity(&name).unwrap();
        let before = runtime.allocation_report();
        let Value::Native(copy) = runtime
            .identity_value(PrototypeIdentity::Native(native))
            .unwrap()
        else {
            panic!("native identity lost")
        };
        assert_eq!(copy, name);
        assert_ne!(copy.as_ptr(), runtime.native_properties[native].0.as_ptr());
        assert_eq!(
            runtime.allocation_report().accepted_bytes - before.accepted_bytes,
            name.len() as u64
        );
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - name.len() + 1)
            .unwrap();
        let records = runtime.native_properties.len();
        assert!(matches!(
            runtime.identity_value(PrototypeIdentity::Native(native)),
            Err(Fault::Fatal(_))
        ));
        assert_eq!(
            runtime
                .allocation_report()
                .first_rejected
                .unwrap()
                .requested_bytes,
            name.len() as u64
        );
        assert_eq!(runtime.native_properties.len(), records);
    }

    #[test]
    fn generated_native_method_names_are_preflighted_before_return() {
        let mut runtime = Runtime::new();
        let native = runtime.native_identity("Object").unwrap();
        let receiver = Value::Native("Object".into());
        let requested = "Object.getPrototypeOf".len();
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - requested + 1)
            .unwrap();
        let before = runtime.allocation_report();
        assert!(matches!(
            runtime.native_own_value(native, "getPrototypeOf", &receiver, &mut NoIo),
            Err(Fault::Fatal(_))
        ));
        let after = runtime.allocation_report();
        assert_eq!(after.accepted_bytes, before.accepted_bytes);
        assert_eq!(
            after.first_rejected.unwrap().requested_bytes,
            requested as u64
        );
        assert!(after.is_valid());
    }

    #[test]
    fn native_identity_registration_uses_existing_object_cap() {
        let mut runtime = Runtime::new();
        while runtime.objects.len() < MAX_OBJECTS {
            runtime.object(None, None).unwrap();
        }
        let records = runtime.native_properties.len();
        let before = runtime.allocation_report();
        assert!(
            matches!(runtime.native_identity("object-cap-native"), Err(Fault::Fatal(error)) if error.contains("object limit"))
        );
        assert_eq!(runtime.native_properties.len(), records);
        assert_eq!(runtime.allocation_report(), before);
    }

    #[test]
    fn ordinary_and_function_identity_do_not_alias_the_function_property_bag() {
        let mut runtime = Runtime::new();
        let function = runtime
            .execute("function Parent(a){}Parent;", &mut NoIo)
            .unwrap();
        let owner = runtime.object_identity(&function).unwrap();
        let storage = runtime.identity_storage(owner).unwrap();
        assert_ne!(owner, PrototypeIdentity::Object(storage));
        let child = runtime.object_with_prototype(Some(owner), None).unwrap();
        assert_eq!(
            runtime
                .identity_parent(PrototypeIdentity::Object(child))
                .unwrap(),
            Some(owner)
        );
        assert_eq!(runtime.identity_value(owner).unwrap(), function);
        assert_eq!(
            runtime
                .get(&Value::Object(child), "length", &mut NoIo)
                .unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn primitive_string_out_of_range_index_retains_its_existing_boundary() {
        assert_eq!(Runtime::new().execute(
            "String.prototype[99]='inherited';'a'[99]===undefined && Object('a')[99]==='inherited';", &mut NoIo).unwrap(), Value::Bool(true));
    }
}
