//! Original Symbol identities and typed property keys. Not a string encoding.
use super::*;
use std::sync::Arc;

const MAX_SYMBOLS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WellKnown {
    ToPrimitive,
    ToStringTag,
}

struct SymbolData {
    description: Option<Vec<u16>>,
    well_known: Option<WellKnown>,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoIo;
    impl Host for NoIo {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            Err("no host access".into())
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            Err("no host access".into())
        }
        fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
            Err("no host access".into())
        }
    }

    #[test]
    fn immutable_identity_preserves_public_transport_and_metadata_allowances() {
        fn assert_transport<T: Send + Sync>() {}
        assert_transport::<SymbolHandle>();
        assert_transport::<Value>();
        // These are logical storage allowances, not an allocator/RSS proof.
        assert!(std::mem::size_of::<Property>() <= 128);
        assert!(
            std::mem::size_of::<SymbolData>()
                + 2 * std::mem::size_of::<usize>()
                + 2 * std::mem::size_of::<SymbolHandle>()
                <= 128
        );
        assert_eq!(
            std::mem::size_of::<SymbolHandle>(),
            std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn well_known_semantics_and_foreign_storage_admission_are_distinct() {
        let mut runtime = Runtime::new();
        let foreign = Runtime::new().to_primitive.unwrap();
        let local = runtime.to_primitive.as_ref().unwrap();
        assert_eq!(local, &foreign);
        assert!(!Arc::ptr_eq(&local.0, &foreign.0));
        let before = runtime.allocation_report();
        let count = runtime.symbols.len();
        runtime.admit(&Value::Symbol(foreign.clone())).unwrap();
        assert_eq!(runtime.symbols.len(), count + 1);
        assert!(Arc::ptr_eq(&runtime.symbols.last().unwrap().0, &foreign.0));
        assert_eq!(
            runtime.allocation_report().accepted_bytes - before.accepted_bytes,
            foreign.allowance() as u64
        );
        let admitted = runtime.allocation_report();
        runtime.admit(&Value::Symbol(foreign.clone())).unwrap();
        assert_eq!(runtime.allocation_report(), admitted);
        assert_eq!(
            foreign.0.description.as_ref().unwrap(),
            &"Symbol.toPrimitive".encode_utf16().collect::<Vec<_>>()
        );
    }

    #[test]
    fn description_capacity_is_retained_and_foreign_admission_preflights() {
        let mut source = Runtime::new();
        let mut description = Vec::with_capacity(64);
        description.extend([0xd800, 0, 0xdfff]);
        let pointer = description.as_ptr();
        let capacity = description.capacity();
        let before = source.allocation_report();
        let symbol = source.create_symbol(Some(description), None).unwrap();
        let retained = symbol.0.description.as_ref().unwrap();
        assert_eq!(retained.as_ptr(), pointer);
        assert_eq!(retained.capacity(), capacity);
        assert_eq!(retained, &[0xd800, 0, 0xdfff]);
        assert_eq!(
            source.allocation_report().accepted_bytes - before.accepted_bytes,
            (128 + capacity * 2) as u64
        );
        drop(source);
        let mut destination = Runtime::new();
        let count = destination.symbols.len();
        destination
            .budget
            .allocate(MAX_HEAP - destination.budget.allocated - 1)
            .unwrap();
        let before = destination.allocation_report();
        assert!(matches!(
            destination.admit(&Value::Symbol(symbol.clone())),
            Err(Fault::Fatal(_))
        ));
        assert_eq!(destination.symbols.len(), count);
        let after = destination.allocation_report();
        assert_eq!(after.accepted_bytes, before.accepted_bytes);
        assert_eq!(
            after.first_rejected.unwrap().requested_bytes,
            symbol.allowance() as u64
        );
        assert!(after.is_valid());
    }

    #[test]
    fn in_checks_rhs_before_property_key_hook() {
        let result = Runtime::new().execute(
            "var called=false,key={};key[Symbol.toPrimitive]=function(){called=true;throw 37;};var error;try{key in 0;}catch(e){error=String(e);}!called && error.indexOf('TypeError')>=0;",
            &mut NoIo).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn numeric_binary_left_conversion_precedes_right_hook() {
        for operator in ["*", "/", "%", "-", "<<", ">>", ">>>", "&", "^", "|"] {
            let source = format!(
                "var called=false,key={{}};key[Symbol.toPrimitive]=function(){{called=true;return 2;}};var error;try{{Symbol('x') {operator} key;}}catch(e){{error=String(e);}}!called && error.indexOf('TypeError')>=0;"
            );
            assert_eq!(
                Runtime::new().execute(&source, &mut NoIo).unwrap(),
                Value::Bool(true),
                "{operator}"
            );
        }
        for operator in ["+", "<", ">", "<=", ">="] {
            let source = format!(
                "var called=false,key={{}};key[Symbol.toPrimitive]=function(){{called=true;return 2;}};try{{Symbol('x') {operator} key;}}catch(e){{}}called;"
            );
            assert_eq!(
                Runtime::new().execute(&source, &mut NoIo).unwrap(),
                Value::Bool(true),
                "{operator}"
            );
        }
    }

    #[test]
    fn getter_native_copy_is_charged_before_copy_or_invocation() {
        let mut runtime = Runtime::new();
        let symbol = Value::Symbol(runtime.create_symbol(None, None).unwrap());
        let before = runtime.allocation_report();
        assert_eq!(
            runtime
                .get_own_value(runtime.symbol_prototype, "description", &symbol, &mut NoIo)
                .unwrap(),
            Some(Value::Undefined)
        );
        assert_eq!(
            runtime.allocation_report().accepted_bytes - before.accepted_bytes,
            "Symbol.description".len() as u64
        );

        let mut runtime = Runtime::new();
        let symbol = Value::Symbol(runtime.create_symbol(None, None).unwrap());
        let remaining = "Symbol.description".len() - 1;
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
            .unwrap();
        let before = runtime.allocation_report();
        let result =
            runtime.get_own_value(runtime.symbol_prototype, "description", &symbol, &mut NoIo);
        assert!(matches!(result, Err(Fault::Fatal(_))));
        let after = runtime.allocation_report();
        assert_eq!(after.accepted_bytes, before.accepted_bytes);
        assert_eq!(
            after.first_rejected.unwrap().requested_bytes,
            "Symbol.description".len() as u64
        );
        assert_eq!(runtime.budget.calls, 0);
        assert!(after.is_valid());
        let rejected = match result {
            Err(error) => error,
            _ => unreachable!(),
        };
        let first = runtime.finish(Err(rejected)).unwrap_err();
        assert_eq!(
            runtime
                .execute("var afterGetterFailure=1;", &mut NoIo)
                .unwrap_err(),
            first
        );
        assert_eq!(runtime.get_global("afterGetterFailure"), Value::Undefined);
    }

    #[test]
    fn coercion_hook_prepays_its_argument_slot_before_callback() {
        const HOOK: &str = "host.fixture.hook";
        struct HookHost {
            calls: usize,
        }
        impl Host for HookHost {
            fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
                unreachable!()
            }
            fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
                unreachable!()
            }
            fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
                assert_eq!(name, HOOK);
                assert!(matches!(this, Value::Object(_)));
                assert_eq!(args, vec![Value::text("string")]);
                self.calls += 1;
                Ok(Value::Number(7.0))
            }
        }
        fn fixture() -> (Runtime, Value) {
            let mut runtime = Runtime::new();
            let id = runtime
                .object(Some(runtime.object_prototype), None)
                .unwrap();
            let key = runtime.to_primitive.as_ref().unwrap().clone();
            runtime
                .put_symbol(id, &key, Value::Native(HOOK.into()), true)
                .unwrap();
            (runtime, Value::Object(id))
        }
        let (mut runtime, value) = fixture();
        let mut host = HookHost { calls: 0 };
        let before = runtime.allocation_report();
        assert_eq!(
            runtime.primitive(value, Hint::String, &mut host).unwrap(),
            Value::Number(7.0)
        );
        assert_eq!(host.calls, 1);
        assert_eq!(
            runtime.allocation_report().accepted_bytes - before.accepted_bytes,
            (HOOK.len() + "string".len() * 2 + 64) as u64
        );

        let (mut runtime, value) = fixture();
        let mut host = HookHost { calls: 0 };
        let before_slot = HOOK.len() + "string".len() * 2;
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated - before_slot - 63)
            .unwrap();
        let before = runtime.allocation_report();
        let result = runtime.primitive(value, Hint::String, &mut host);
        assert!(matches!(result, Err(Fault::Fatal(_))));
        assert_eq!(host.calls, 0);
        assert_eq!(runtime.budget.calls, 0);
        let after = runtime.allocation_report();
        assert_eq!(
            after.accepted_bytes - before.accepted_bytes,
            before_slot as u64
        );
        assert_eq!(after.first_rejected.unwrap().requested_bytes, 64);
        assert!(after.is_valid());
        let first = runtime.finish(result).unwrap_err();
        assert_eq!(
            runtime
                .execute("var afterHookFailure=1;", &mut host)
                .unwrap_err(),
            first
        );
        assert_eq!(runtime.get_global("afterHookFailure"), Value::Undefined);
        assert_eq!(host.calls, 0);
    }

    #[test]
    fn intrinsic_symbol_hook_and_getter_names_are_correct() {
        let mut runtime = Runtime::new();
        assert_eq!(runtime.execute(
            "Symbol.prototype[Symbol.toPrimitive].name==='[Symbol.toPrimitive]' && Symbol.prototype[Symbol.toPrimitive].length===1;",
            &mut NoIo).unwrap(), Value::Bool(true));
        assert_eq!(
            runtime
                .get(
                    &Value::Native("Symbol.description".into()),
                    "name",
                    &mut NoIo
                )
                .unwrap(),
            Value::text("get description")
        );
    }
}

/// An immutable, opaque primitive identity; cloning retains the same symbol.
#[derive(Clone)]
pub struct SymbolHandle(Arc<SymbolData>);
impl PartialEq for SymbolHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || self.0.well_known.is_some() && self.0.well_known == other.0.well_known
    }
}
impl Eq for SymbolHandle {}
impl std::fmt::Debug for SymbolHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Symbol(<opaque identity>)")
    }
}
impl SymbolHandle {
    fn allowance(&self) -> usize {
        128usize.saturating_add(
            self.0
                .description
                .as_ref()
                .map_or(0, |s| s.capacity().saturating_mul(2)),
        )
    }
    pub(super) fn display(&self) -> String {
        format!(
            "Symbol({})",
            self.0
                .description
                .as_ref()
                .map(|s| String::from_utf16_lossy(s))
                .unwrap_or_default()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PropertyKey {
    String(String),
    Symbol(SymbolHandle),
}
impl PropertyKey {
    pub(super) fn string(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            Self::Symbol(_) => None,
        }
    }
}
impl PartialEq<&str> for PropertyKey {
    fn eq(&self, other: &&str) -> bool {
        self.string() == Some(*other)
    }
}
impl PartialEq<&String> for PropertyKey {
    fn eq(&self, other: &&String) -> bool {
        self.string() == Some(other.as_str())
    }
}

#[derive(Clone, Copy)]
pub(super) enum Hint {
    Default,
    String,
    Number,
}

impl Runtime {
    pub(super) fn admit(&mut self, value: &Value) -> Eval<()> {
        let Value::Symbol(symbol) = value else {
            return Ok(());
        };
        for admitted in &self.symbols {
            self.budget.step()?;
            // Equal well-known identities may still own distinct foreign Arc
            // records: storage admission follows allocation identity, not ==.
            if Arc::ptr_eq(&admitted.0, &symbol.0) {
                return Ok(());
            }
        }
        if self.symbols.len() >= MAX_SYMBOLS {
            return Err(Fault::Fatal("JavaScript symbol limit exhausted".into()));
        }
        self.budget.allocate(symbol.allowance())?;
        self.symbols.push(symbol.clone());
        Ok(())
    }

    fn create_symbol(
        &mut self,
        description: Option<Vec<u16>>,
        well_known: Option<WellKnown>,
    ) -> Eval<SymbolHandle> {
        if self.symbols.len() >= MAX_SYMBOLS {
            return Err(Fault::Fatal("JavaScript symbol limit exhausted".into()));
        }
        self.budget.allocate(
            128usize.saturating_add(
                description
                    .as_ref()
                    .map_or(0, |s| s.capacity().saturating_mul(2)),
            ),
        )?;
        let symbol = SymbolHandle(Arc::new(SymbolData {
            description,
            well_known,
        }));
        self.symbols.push(symbol.clone());
        Ok(symbol)
    }

    pub(super) fn symbol_bootstrap(&mut self) -> Eval<()> {
        let primitive = self.create_symbol(
            Some("Symbol.toPrimitive".encode_utf16().collect()),
            Some(WellKnown::ToPrimitive),
        )?;
        let tag = self.create_symbol(
            Some("Symbol.toStringTag".encode_utf16().collect()),
            Some(WellKnown::ToStringTag),
        )?;
        self.to_primitive = Some(primitive.clone());
        self.to_string_tag = Some(tag.clone());
        self.put_own(0, "Symbol", Value::Native("Symbol".into()), true)?;
        let constructor = self.object(Some(self.function_prototype), None)?;
        self.budget.allocate(64 + "Symbol".len())?;
        self.native_properties.push(("Symbol".into(), constructor));
        for (key, value, writable, configurable) in [
            (
                "prototype",
                Value::Object(self.symbol_prototype),
                false,
                false,
            ),
            (
                "toPrimitive",
                Value::Symbol(primitive.clone()),
                false,
                false,
            ),
            ("toStringTag", Value::Symbol(tag.clone()), false, false),
            ("for", Value::Native("Symbol.for".into()), true, true),
            ("keyFor", Value::Native("Symbol.keyFor".into()), true, true),
        ] {
            self.put_own(constructor, key, value, false)?;
            let property = self.objects[constructor].properties.last_mut().unwrap();
            property.writable = writable;
            property.configurable = configurable;
        }
        for (name, native) in [
            ("constructor", "Symbol"),
            ("toString", "Symbol.toString"),
            ("valueOf", "Symbol.valueOf"),
            ("description", "Symbol.description"),
        ] {
            self.put_own(
                self.symbol_prototype,
                name,
                Value::Native(native.into()),
                false,
            )?;
            if name == "description" {
                let property = self.objects[self.symbol_prototype]
                    .properties
                    .last_mut()
                    .unwrap();
                property.getter = true;
                property.writable = false;
            }
        }
        self.put_symbol(
            self.symbol_prototype,
            &primitive,
            Value::Native("Symbol.toPrimitive".into()),
            false,
        )?;
        self.objects[self.symbol_prototype]
            .properties
            .last_mut()
            .unwrap()
            .writable = false;
        self.put_symbol(self.symbol_prototype, &tag, Value::text("Symbol"), false)?;
        self.objects[self.symbol_prototype]
            .properties
            .last_mut()
            .unwrap()
            .writable = false;
        Ok(())
    }

    pub(super) fn put_symbol(
        &mut self,
        id: usize,
        symbol: &SymbolHandle,
        value: Value,
        enumerable: bool,
    ) -> Eval<()> {
        self.budget.step()?;
        self.admit(&Value::Symbol(symbol.clone()))?;
        self.admit(&value)?;
        let key = PropertyKey::Symbol(symbol.clone());
        let object = self
            .objects
            .get_mut(id)
            .ok_or_else(|| exception("TypeError: unknown object"))?;
        if let Some(property) = object.properties.iter_mut().find(|p| p.key == key) {
            if property.writable {
                self.budget.allocate(value_bytes(&value))?;
                property.value = value;
            }
        } else {
            self.budget.allocate(value_bytes(&value))?;
            self.budget.allocate(128)?;
            object.properties.push(Property {
                key,
                value,
                enumerable,
                writable: true,
                configurable: true,
                getter: false,
            });
        }
        Ok(())
    }

    pub(super) fn symbol_brand(&mut self, value: &Value) -> Eval<SymbolHandle> {
        match value {
            Value::Symbol(symbol) => Ok(symbol.clone()),
            Value::Object(id) => match self.objects.get(*id).and_then(|o| o.boxed.as_ref()) {
                Some(Value::Symbol(symbol)) => Ok(symbol.clone()),
                _ => Err(exception("TypeError: incompatible Symbol receiver")),
            },
            _ => Err(exception("TypeError: incompatible Symbol receiver")),
        }
    }

    fn symbol_string(&mut self, symbol: &SymbolHandle) -> Eval<Value> {
        let length = symbol
            .0
            .description
            .as_ref()
            .map_or(0, Vec::len)
            .saturating_add(8);
        self.budget.allocate(length.saturating_mul(2))?;
        let mut text = Vec::with_capacity(length);
        text.extend("Symbol(".encode_utf16());
        if let Some(description) = &symbol.0.description {
            text.extend(description);
        }
        text.push(b')' as u16);
        Ok(Value::String(text))
    }

    pub(super) fn symbol_native(
        &mut self,
        name: &str,
        this: Value,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Eval<Value> {
        match name {
            "Symbol" => {
                let argument = args.into_iter().next().unwrap_or(Value::Undefined);
                let description = if matches!(argument, Value::Undefined) {
                    None
                } else {
                    Some(self.units(argument, host)?)
                };
                Ok(Value::Symbol(self.create_symbol(description, None)?))
            }
            "Symbol.for" => {
                let key = self.units(args.into_iter().next().unwrap_or(Value::Undefined), host)?;
                for symbol in &self.symbol_registry {
                    self.budget.step()?;
                    let existing = symbol.0.description.as_ref().unwrap();
                    self.budget.step()?;
                    if existing.len() == key.len() {
                        let mut equal = true;
                        for (a, b) in existing.iter().zip(&key) {
                            self.budget.step()?;
                            if a != b {
                                equal = false;
                                break;
                            }
                        }
                        if equal {
                            return Ok(Value::Symbol(symbol.clone()));
                        }
                    }
                }
                self.budget.allocate(64)?;
                let symbol = self.create_symbol(Some(key), None)?;
                self.symbol_registry.push(symbol.clone());
                Ok(Value::Symbol(symbol))
            }
            "Symbol.keyFor" => {
                let Some(Value::Symbol(symbol)) = args.first() else {
                    return Err(exception("TypeError: Symbol.keyFor requires a symbol"));
                };
                for registered in &self.symbol_registry {
                    self.budget.step()?;
                    if registered == symbol {
                        let units = registered.0.description.as_ref().unwrap();
                        self.budget.allocate(units.len().saturating_mul(2))?;
                        return Ok(Value::String(units.clone()));
                    }
                }
                Ok(Value::Undefined)
            }
            "Symbol.toString" => {
                let symbol = self.symbol_brand(&this)?;
                self.symbol_string(&symbol)
            }
            "Symbol.valueOf" | "Symbol.toPrimitive" => Ok(Value::Symbol(self.symbol_brand(&this)?)),
            "Symbol.description" => {
                let symbol = self.symbol_brand(&this)?;
                if let Some(description) = &symbol.0.description {
                    self.budget.allocate(description.len().saturating_mul(2))?;
                    Ok(Value::String(description.clone()))
                } else {
                    Ok(Value::Undefined)
                }
            }
            _ => Err(unsupported(name)),
        }
    }

    pub(super) fn string_symbol(&mut self, value: Value) -> Eval<Value> {
        let Value::Symbol(symbol) = value else {
            unreachable!()
        };
        self.symbol_string(&symbol)
    }

    pub(super) fn get_key(
        &mut self,
        value: &Value,
        key: &PropertyKey,
        host: &mut impl Host,
    ) -> Eval<Value> {
        let PropertyKey::Symbol(symbol) = key else {
            return self.get(value, key.string().unwrap(), host);
        };
        self.admit(&Value::Symbol(symbol.clone()))?;
        if matches!(value, Value::Host(_)) {
            return Err(unsupported("Symbol keys on host objects"));
        }
        self.read_property(value, KeyRef::Symbol(symbol), host)
    }

    pub(super) fn set_key(
        &mut self,
        object: Value,
        key: &PropertyKey,
        value: Value,
        host: &mut impl Host,
    ) -> Eval<()> {
        let PropertyKey::Symbol(symbol) = key else {
            return self.set(object, key.string().unwrap(), value, host);
        };
        self.admit(&Value::Symbol(symbol.clone()))?;
        if matches!(object, Value::Host(_)) {
            return Err(unsupported("Symbol keys on host objects"));
        }
        if matches!(object, Value::Null | Value::Undefined) {
            return Err(exception(
                "TypeError: property assignment on null or undefined",
            ));
        }
        if object.primitive() {
            return Ok(());
        }
        let owner = self.object_identity(&object)?;
        if self.readonly_property(owner, KeyRef::Symbol(symbol))? {
            return Ok(());
        }
        let id = self.identity_storage(owner)?;
        self.put_symbol(id, symbol, value, true)
    }

    pub(super) fn delete_key(&mut self, object: Value, key: &PropertyKey) -> Eval<Value> {
        let PropertyKey::Symbol(symbol) = key else {
            return self.delete(object, key.string().unwrap());
        };
        self.admit(&Value::Symbol(symbol.clone()))?;
        if matches!(object, Value::Host(_)) {
            return Err(unsupported("Symbol keys on host objects"));
        }
        if matches!(object, Value::Null | Value::Undefined) {
            return Err(exception("TypeError: delete on null or undefined"));
        }
        if object.primitive() {
            return Ok(Value::Bool(true));
        }
        // A native without its own property store has no own symbol to delete.
        if let Value::Native(name) = &object {
            if self.native_properties_id(name)?.is_none() {
                return Ok(Value::Bool(true));
            }
        }
        let owner = self.object_identity(&object)?;
        let id = self.identity_storage(owner)?;
        self.budget.step()?;
        if let Some(index) = self.objects[id]
            .properties
            .iter()
            .position(|p| &p.key == key)
        {
            if !self.objects[id].properties[index].configurable {
                return Ok(Value::Bool(false));
            }
            self.objects[id].properties.remove(index);
        }
        Ok(Value::Bool(true))
    }

    pub(super) fn has_key(
        &mut self,
        object: &Value,
        key: &PropertyKey,
        own_only: bool,
    ) -> Eval<bool> {
        let key = match key {
            PropertyKey::String(key) => KeyRef::String(key),
            PropertyKey::Symbol(key) => KeyRef::Symbol(key),
        };
        self.has_property(object, key, own_only)
    }

    pub(super) fn has_property(
        &mut self,
        object: &Value,
        key: KeyRef<'_>,
        own_only: bool,
    ) -> Eval<bool> {
        if matches!(object, Value::Host(_)) {
            return Err(unsupported(if matches!(key, KeyRef::Symbol(_)) {
                "Symbol keys on host objects"
            } else {
                "property inspection on host objects"
            }));
        }
        if own_only && object.primitive() {
            return Ok(false);
        }
        let mut current = Some(self.object_identity(object)?);
        for _ in 0..MAX_CALLS {
            let Some(owner) = current else {
                return Ok(false);
            };
            if self.own_descriptor(owner, key)?.is_some() {
                return Ok(true);
            }
            if own_only {
                return Ok(false);
            }
            current = self.identity_parent(owner)?;
        }
        Err(Fault::Fatal(
            "JavaScript prototype depth limit exhausted".into(),
        ))
    }

    pub(super) fn get_own_value(
        &mut self,
        id: usize,
        key: &str,
        receiver: &Value,
        host: &mut impl Host,
    ) -> Eval<Option<Value>> {
        let getter = self.objects[id]
            .properties
            .iter()
            .find(|p| p.key == key)
            .filter(|p| p.getter)
            .map(|p| &p.value);
        if let Some(getter) = getter {
            let getter = self.budget.copy(getter)?;
            let receiver = self.copy(receiver)?;
            return self.call(getter, receiver, vec![], None, host).map(Some);
        }
        self.own(id, key)
    }

    pub(super) fn own_symbols(&mut self, value: Value) -> Eval<Value> {
        if matches!(value, Value::Host(_)) {
            return Err(unsupported("Symbol keys on host objects"));
        }
        if matches!(value, Value::Null | Value::Undefined) {
            return Err(exception("TypeError: cannot inspect null or undefined"));
        }
        let mut result = PrepaidArray::growing(AllocationPhase::Runtime);
        let native_missing = if let Value::Native(name) = &value {
            self.native_properties_id(name)?.is_none()
        } else {
            false
        };
        if !value.primitive() && !native_missing {
            let owner = self.object_identity(&value)?;
            let id = self.identity_storage(owner)?;
            for property in &self.objects[id].properties {
                self.budget.step()?;
                if let PropertyKey::Symbol(symbol) = &property.key {
                    result.push_owned(&mut self.budget, Some(Value::Symbol(symbol.clone())))?;
                }
            }
        }
        Ok(Value::Object(
            self.object(Some(self.array_prototype), Some(result))?,
        ))
    }

    pub(super) fn object_tag(&mut self, value: Value, host: &mut impl Host) -> Eval<Value> {
        if matches!(value, Value::Undefined) {
            return self.text("[object Undefined]");
        }
        if matches!(value, Value::Null) {
            return self.text("[object Null]");
        }
        let key = PropertyKey::Symbol(self.to_string_tag.as_ref().unwrap().clone());
        let tag = if matches!(value, Value::Host(_)) {
            Value::Undefined
        } else {
            self.get_key(&value, &key, host)?
        };
        if let Value::String(units) = tag {
            let length = units.len().saturating_add(9);
            self.budget.allocate(length.saturating_mul(2))?;
            let mut output = Vec::with_capacity(length);
            output.extend("[object ".encode_utf16());
            output.extend(units);
            output.push(b']' as u16);
            return Ok(Value::String(output));
        }
        let fallback = match &value {
            Value::Object(id) if self.objects[*id].arguments => "Arguments",
            Value::Object(id) if self.objects[*id].array.is_some() => "Array",
            Value::Object(id) if self.objects[*id].regexp.is_some() => "RegExp",
            Value::Object(id) if self.objects[*id].error.is_some() => "Error",
            Value::Object(id) => match self.objects[*id].boxed {
                Some(Value::String(_)) => "String",
                Some(Value::Bool(_)) => "Boolean",
                Some(Value::Number(_)) => "Number",
                _ => "Object",
            },
            Value::Function(_) | Value::Native(_) => "Function",
            Value::String(_) => "String",
            Value::Number(_) => "Number",
            Value::Bool(_) => "Boolean",
            _ => "Object",
        };
        self.text(&format!("[object {fallback}]"))
    }
}
