//! Six ES5-shaped intrinsic Error families; no new global constructors or host API.
//! Active JavaScript conversion and callback-free failure diagnostics are separate.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ErrorKind {
    Error,
    Type,
    Range,
    Reference,
    Syntax,
    Uri,
}
impl ErrorKind {
    const ALL: [Self; 6] = [
        Self::Error,
        Self::Type,
        Self::Range,
        Self::Reference,
        Self::Syntax,
        Self::Uri,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Type => "TypeError",
            Self::Range => "RangeError",
            Self::Reference => "ReferenceError",
            Self::Syntax => "SyntaxError",
            Self::Uri => "URIError",
        }
    }
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

impl Runtime {
    pub(super) fn error_bootstrap(&mut self) -> Eval<()> {
        for kind in ErrorKind::ALL {
            let parent = if kind == ErrorKind::Error {
                self.object_prototype
            } else {
                self.error_prototypes[ErrorKind::Error as usize]
            };
            let prototype = self.object(Some(parent), None)?;
            // ES5.1 15.11.4/15.11.7.7: these prototype objects also have Error
            // branding. Merely inheriting one does not confer that own brand.
            self.objects[prototype].error = Some(kind);
            self.error_prototypes[kind as usize] = prototype;
            let name = self.text(kind.name())?;
            self.put_own(prototype, "name", name, false)?;
            self.put_own(prototype, "message", Value::String(Vec::new()), false)?;

            // The native identity retains its name before copying/allocating.
            // Real constructor property storage uses the existing descriptors;
            // no separate virtual prototype or mutable-prototype path is added.
            let native = self.native_identity(kind.name())?;
            let constructor = self.native_properties[native].1;
            self.put_own(constructor, "prototype", Value::Object(prototype), false)?;
            let property = self.objects[constructor].properties.last_mut().unwrap();
            property.writable = false;
            property.configurable = false;
            let value = self.identity_value(PrototypeIdentity::Native(native))?;
            self.put_own(prototype, "constructor", value, false)?;
        }
        let method = self.native_identity("Error.toString")?;
        let storage = self.native_properties[method].1;
        self.put_own(storage, "length", Value::Number(0.0), false)?;
        let property = self.objects[storage].properties.last_mut().unwrap();
        property.writable = false;
        property.configurable = false;
        let value = self.identity_value(PrototypeIdentity::Native(method))?;
        self.put_own(
            self.error_prototypes[ErrorKind::Error as usize],
            "toString",
            value,
            false,
        )?;
        Ok(())
    }

    pub(super) fn error_instance(&mut self, kind: ErrorKind) -> Eval<usize> {
        let id = self.object(Some(self.error_prototypes[kind as usize]), None)?;
        self.objects[id].error = Some(kind);
        Ok(id)
    }

    pub(super) fn error_to_string(&mut self, this: Value, host: &mut impl Host) -> Eval<Value> {
        if this.primitive() {
            return Err(exception("TypeError: Error.toString requires an object"));
        }
        // ES5.1 15.11.4.4: each Get uses the original receiver; name conversion
        // must finish before even reading message. These are ordinary live JS
        // operations and may execute fallible getters/coercion/Host callbacks.
        let name = self.get(&this, "name", host)?;
        let name = if matches!(name, Value::Undefined) {
            let Value::String(name) = self.text("Error")? else {
                unreachable!()
            };
            name
        } else {
            self.units(name, host)?
        };
        let message = self.get(&this, "message", host)?;
        let message = if matches!(message, Value::Undefined) {
            Vec::new()
        } else {
            self.units(message, host)?
        };
        if name.is_empty() {
            return Ok(Value::String(message));
        }
        if message.is_empty() {
            return Ok(Value::String(name));
        }
        let length = name.len().saturating_add(2).saturating_add(message.len());
        self.budget.allocate(length.saturating_mul(2))?;
        let mut output = Vec::with_capacity(length);
        output.extend(name);
        output.extend([b':' as u16, b' ' as u16]);
        output.extend(message);
        Ok(Value::String(output))
    }

    // Formatting follows an already thrown Error. Do not call get/copy, consume
    // realm fuel, admit native identities, invoke getters/Host or replace its
    // first failure. Existing non-Error diagnostics deliberately remain separate.
    fn error_diagnostic_field(&self, id: usize, key: &str) -> DiagnosticField<'_> {
        let mut current = Some(PrototypeIdentity::Object(id));
        for _ in 0..MAX_CALLS {
            let Some(owner) = current else {
                return DiagnosticField::Missing;
            };
            let storage = match owner {
                PrototypeIdentity::Object(id) => Some(id),
                PrototypeIdentity::Function(id) => self.functions.get(id).map(|f| f.properties),
                PrototypeIdentity::Native(id) => self.native_properties.get(id).map(|p| p.1),
            };
            let Some(object) = storage.and_then(|id| self.objects.get(id)) else {
                return DiagnosticField::Inaccessible;
            };
            if let Some(property) = object
                .properties
                .iter()
                .find(|property| property.key == key)
            {
                return if property.getter {
                    DiagnosticField::Accessor
                } else {
                    DiagnosticField::Value(property.value.raw())
                };
            }
            current = object.prototype;
        }
        DiagnosticField::Inaccessible
    }

    pub(super) fn error_diagnostic(&self, id: usize, kind: ErrorKind) -> String {
        let name = self.error_diagnostic_field(id, "name");
        let message = self.error_diagnostic_field(id, "message");
        let mut output = ErrorDiagnostic::new();
        output.text("Uncaught JavaScript exception: ");
        let empty_name =
            matches!(name, DiagnosticField::Value(Value::String(units)) if units.is_empty());
        let empty_message = matches!(
            message,
            DiagnosticField::Missing | DiagnosticField::Value(Value::Undefined)
        ) || matches!(message, DiagnosticField::Value(Value::String(units)) if units.is_empty());
        match name {
            DiagnosticField::Missing
            | DiagnosticField::Inaccessible
            | DiagnosticField::Accessor
            | DiagnosticField::Value(Value::Undefined) => output.text(kind.name()),
            DiagnosticField::Value(value) => output.value(value),
        }
        if !empty_name && !empty_message {
            output.text(": ");
        }
        match message {
            DiagnosticField::Missing | DiagnosticField::Value(Value::Undefined) => {}
            DiagnosticField::Inaccessible => output.text("[unavailable]"),
            DiagnosticField::Accessor => output.text("[accessor]"),
            DiagnosticField::Value(value) => output.value(value),
        }
        output.finish()
    }
}

enum DiagnosticField<'a> {
    Missing,
    Inaccessible,
    Accessor,
    Value(&'a Value),
}

// Independent host-output bound, not a realm allocation or a JS string limit.
// Includes the prefix and, when needed, the suffix. At most 3 UTF-8 bytes per
// UTF-16 unit (including replacement characters), so the one buffer is <=12 KiB.
const MAX_ERROR_DIAGNOSTIC_UNITS: usize = 4096;
const DIAGNOSTIC_SUFFIX: &str = "... [truncated]";
struct ErrorDiagnostic {
    text: String,
    units: usize,
    truncated: bool,
}
impl ErrorDiagnostic {
    fn new() -> Self {
        Self {
            text: String::with_capacity(MAX_ERROR_DIAGNOSTIC_UNITS * 3),
            units: 0,
            truncated: false,
        }
    }
    fn character(&mut self, value: char) -> bool {
        let length = value.len_utf16();
        if self.units + length > MAX_ERROR_DIAGNOSTIC_UNITS - DIAGNOSTIC_SUFFIX.len() {
            self.truncated = true;
            return false;
        }
        self.text.push(value);
        self.units += length;
        true
    }
    fn text(&mut self, value: &str) {
        if self.truncated {
            return;
        }
        for value in value.chars() {
            if !self.character(value) {
                break;
            }
        }
    }
    fn value(&mut self, value: &Value) {
        if self.truncated {
            return;
        }
        match value {
            Value::String(units) => {
                for value in char::decode_utf16(units.iter().copied()) {
                    if !self.character(value.unwrap_or(char::REPLACEMENT_CHARACTER)) {
                        break;
                    }
                }
            }
            Value::Null => self.text("null"),
            Value::Undefined => self.text("undefined"),
            Value::Bool(true) => self.text("true"),
            Value::Bool(false) => self.text("false"),
            Value::Number(value) => self.text(&number_text(*value)),
            Value::Symbol(_) => self.text("[symbol]"),
            Value::Object(_) => self.text("[object]"),
            Value::Function(_) | Value::Native(_) => self.text("[function]"),
            Value::Host(_) => self.text("[host]"),
        }
    }
    fn finish(mut self) -> String {
        if self.truncated {
            self.text.push_str(DIAGNOSTIC_SUFFIX);
        }
        self.text
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

    fn object_id(value: Value) -> usize {
        let Value::Object(id) = value else {
            panic!("expected object")
        };
        id
    }

    // The existing private descriptor representation supports builtin getters;
    // installing one here adds no public descriptor or accessor syntax surface.
    fn getter(runtime: &mut Runtime, object: usize, key: &str, value: Value) {
        runtime.put_own(object, key, value, false).unwrap();
        runtime.objects[object]
            .properties
            .iter_mut()
            .find(|property| property.key == key)
            .unwrap()
            .getter = true;
    }

    #[test]
    fn intrinsic_layout_bootstrap_and_all_six_shared_prototypes_are_bounded() {
        let mut runtime = Runtime::new();
        let report = runtime.allocation_report();
        assert!(report.is_valid());
        assert_eq!(report.accepted_bytes, report.phases.bootstrap);
        assert_eq!(report.phases.runtime, 0);
        assert!(std::mem::size_of::<Object>() <= 128);
        assert!(std::mem::size_of::<Property>() <= 128);
        assert!(std::mem::size_of::<Option<ErrorKind>>() <= std::mem::size_of::<usize>());
        eprintln!(
            "ERROR_LAYOUT object={} property={} kind={} optional_kind={} bootstrap={}",
            std::mem::size_of::<Object>(),
            std::mem::size_of::<Property>(),
            std::mem::size_of::<ErrorKind>(),
            std::mem::size_of::<Option<ErrorKind>>(),
            report.phases.bootstrap
        );
        let before_objects = runtime.objects.len();
        for kind in ErrorKind::ALL {
            let id = runtime.error_instance(kind).unwrap();
            assert_eq!(runtime.objects[id].error, Some(kind));
            assert_eq!(
                runtime.objects[id].prototype,
                Some(PrototypeIdentity::Object(
                    runtime.error_prototypes[kind as usize]
                ))
            );
            assert!(runtime.objects[id].properties.is_empty());
            assert_eq!(
                runtime.objects[runtime.error_prototypes[kind as usize]].error,
                Some(kind)
            );
        }
        assert_eq!(runtime.objects.len(), before_objects + 6);
        assert_eq!(runtime.allocation_report().phases.runtime, 6 * 128);
    }

    #[test]
    fn inherited_getters_and_conversions_keep_original_receiver_and_order() {
        let mut runtime = Runtime::new();
        runtime.execute(
            "var trace='';var owner={};var receiver=Object.create(owner);var nameValue={};var messageValue={};nameValue[Symbol.toPrimitive]=function(h){if(this!==nameValue||h!=='string')throw 1;trace+='n';return 'Name';};messageValue[Symbol.toPrimitive]=function(h){if(this!==messageValue||h!=='string')throw 2;trace+='m';return 'Message';};function nameGet(){if(this!==receiver)throw 3;trace+='N';return nameValue;}function messageGet(){if(this!==receiver)throw 4;trace+='M';return messageValue;}",
            &mut NoIo,
        ).unwrap();
        let owner = object_id(runtime.get_global("owner"));
        let name = runtime.get_global("nameGet");
        let message = runtime.get_global("messageGet");
        getter(&mut runtime, owner, "name", name);
        getter(&mut runtime, owner, "message", message);
        assert_eq!(
            runtime
                .execute("Error.prototype.toString.call(receiver)", &mut NoIo)
                .unwrap(),
            Value::text("Name: Message")
        );
        assert_eq!(runtime.get_global("trace"), Value::text("NnMm"));
        runtime
            .execute("trace='';nameValue=Symbol('bad');", &mut NoIo)
            .unwrap();
        assert!(
            runtime
                .execute("Error.prototype.toString.call(receiver)", &mut NoIo)
                .unwrap_err()
                .contains("cannot convert Symbol to string")
        );
        assert_eq!(runtime.get_global("trace"), Value::text("N"));
        runtime
            .execute(
                "trace='';nameValue={toString:function(){trace+='x';throw 37;}};",
                &mut NoIo,
            )
            .unwrap();
        assert_eq!(
            runtime
                .execute("Error.prototype.toString.call(receiver)", &mut NoIo)
                .unwrap_err(),
            "Uncaught JavaScript exception: 37"
        );
        assert_eq!(runtime.get_global("trace"), Value::text("Nx"));
    }

    #[test]
    fn real_get_and_join_allocations_reject_before_allocating_or_later_getters() {
        for remaining in [1999, 6000] {
            let mut runtime = Runtime::new();
            let id = runtime
                .object(Some(runtime.object_prototype), None)
                .unwrap();
            runtime
                .put_own(id, "name", Value::String(vec![b'n' as u16; 1000]), false)
                .unwrap();
            runtime
                .put_own(id, "message", Value::String(vec![b'm' as u16; 1000]), false)
                .unwrap();
            runtime
                .budget
                .allocate(MAX_HEAP - runtime.budget.allocated - remaining)
                .unwrap();
            let before = runtime.budget.allocated;
            let result = runtime.error_to_string(Value::Object(id), &mut NoIo);
            assert!(matches!(result, Err(Fault::Fatal(_))));
            let error = runtime.finish(result).unwrap_err();
            let report = runtime.allocation_report();
            let rejected = report.first_rejected.unwrap();
            assert_eq!(rejected.phase, AllocationPhase::Runtime);
            assert_eq!(
                rejected.requested_bytes,
                if remaining == 1999 { 2000 } else { 4004 }
            );
            assert_eq!(
                report.accepted_bytes as usize,
                before + if remaining == 1999 { 0 } else { 4000 }
            );
            assert_eq!(
                runtime.execute("throw 'later';", &mut NoIo).unwrap_err(),
                error
            );
            assert_eq!(runtime.allocation_report(), report);
        }
    }

    #[test]
    fn empty_conversion_branches_transfer_the_already_paid_result() {
        for (name, message, expected) in [("", "abc", "abc"), ("abc", "", "abc"), ("", "", "")] {
            let mut runtime = Runtime::new();
            let id = runtime
                .object(Some(runtime.object_prototype), None)
                .unwrap();
            runtime
                .put_own(id, "name", Value::text(name), false)
                .unwrap();
            runtime
                .put_own(id, "message", Value::text(message), false)
                .unwrap();
            let before = runtime.budget.allocated;
            assert_eq!(
                runtime
                    .error_to_string(Value::Object(id), &mut NoIo)
                    .unwrap(),
                Value::text(expected)
            );
            assert_eq!(
                runtime.budget.allocated - before,
                2 * (name.len() + message.len())
            );
        }
    }

    #[test]
    fn intrinsic_diagnostics_do_not_invoke_getters_hooks_or_mutate_budget() {
        let mut runtime = Runtime::new();
        let id = runtime.error_instance(ErrorKind::Type).unwrap();
        getter(
            &mut runtime,
            id,
            "name",
            Value::Native("host.must_not_run".into()),
        );
        getter(
            &mut runtime,
            id,
            "message",
            Value::Native("host.must_not_run".into()),
        );
        runtime
            .put_own(
                id,
                "toString",
                Value::Native("host.must_not_run".into()),
                false,
            )
            .unwrap();
        runtime.budget.fuel = 0;
        let before = runtime.allocation_report();
        let native_count = runtime.native_properties.len();
        assert_eq!(
            runtime
                .finish(Err(Fault::Throw(Value::Object(id))))
                .unwrap_err(),
            "Uncaught JavaScript exception: TypeError: [accessor]"
        );
        assert_eq!(runtime.allocation_report(), before);
        assert_eq!(runtime.budget.fuel, 0);
        assert_eq!(runtime.native_properties.len(), native_count);
        assert!(runtime.fatal.is_none());
        let fatal = runtime
            .finish(Err(Fault::Fatal("existing fatal".into())))
            .unwrap_err();
        assert_eq!(fatal, "existing fatal");
        assert_eq!(runtime.execute("throw 99;", &mut NoIo).unwrap_err(), fatal);
        assert_eq!(runtime.allocation_report(), before);
    }

    #[test]
    fn diagnostics_bound_utf16_output_and_never_copy_entire_payloads() {
        let mut runtime = Runtime::new();
        let id = runtime.error_instance(ErrorKind::Syntax).unwrap();
        runtime
            .put_own(id, "message", Value::String(vec![0xd800; 100_000]), false)
            .unwrap();
        let report = runtime.allocation_report();
        let before_fuel = runtime.budget.fuel;
        let output = runtime.error_diagnostic(id, ErrorKind::Syntax);
        assert!(output.starts_with("Uncaught JavaScript exception: SyntaxError: "));
        assert!(output.ends_with(DIAGNOSTIC_SUFFIX));
        assert_eq!(output.encode_utf16().count(), MAX_ERROR_DIAGNOSTIC_UNITS);
        assert!(output.len() <= 3 * MAX_ERROR_DIAGNOSTIC_UNITS);
        assert_eq!(runtime.allocation_report(), report);
        assert_eq!(runtime.budget.fuel, before_fuel);
        // Only the host boundary replaces invalid UTF-16. The original survives.
        let Value::String(stored) = runtime.objects[id].properties[0].value.raw() else {
            panic!()
        };
        assert_eq!(stored.len(), 100_000);
        assert!(stored.iter().all(|unit| *unit == 0xd800));
    }

    #[test]
    fn error_name_deletion_object_placeholders_and_non_error_formatting_are_distinct() {
        let mut runtime = Runtime::new();
        assert_eq!(
            runtime
                .execute("throw new TypeError('message')", &mut NoIo)
                .unwrap_err(),
            "Uncaught JavaScript exception: TypeError: message"
        );
        assert_eq!(runtime.execute("var e=new TypeError();delete TypeError.prototype.name;delete Error.prototype.name;e.message={toString:function(){throw 99;}};throw e;", &mut NoIo).unwrap_err(),
            "Uncaught JavaScript exception: TypeError: [object]");
        assert_eq!(
            runtime
                .execute("throw {name:'ordinary',message:'unchanged'};", &mut NoIo)
                .unwrap_err(),
            "Uncaught JavaScript exception: ordinary: unchanged"
        );
        assert_eq!(
            runtime
                .execute("throw 'ordinary string';", &mut NoIo)
                .unwrap_err(),
            "Uncaught JavaScript exception: ordinary string"
        );
    }
}
