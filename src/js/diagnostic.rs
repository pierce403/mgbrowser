//! Bounded host-only context for the originating nullish member reference.
//! No source, arbitrary key buffer, value handle, or mutable error history is kept.
use super::{Fault, PropertyKey, Value};

const NULLISH_MESSAGE: &str = "TypeError: property access on null or undefined";
const MAX_FORMAT_BYTES: usize = 256;
const MAX_MATCH_UNITS: usize = 32;
const STANDARD_KEYS: &[&str] = &[
    "prototype",
    "constructor",
    "length",
    "name",
    "message",
    "call",
    "apply",
    "bind",
    "toString",
    "valueOf",
    "forEach",
    "map",
    "filter",
    "some",
    "every",
    "reduce",
    "push",
    "pop",
    "shift",
    "unshift",
    "slice",
    "join",
    "concat",
    "indexOf",
    "includes",
    "reverse",
    "appendChild",
    "removeChild",
    "insertBefore",
    "remove",
    "addEventListener",
    "removeEventListener",
    "querySelector",
    "querySelectorAll",
    "getElementById",
    "getElementsByTagName",
    "getElementsByClassName",
    "createElement",
    "createTextNode",
    "setAttribute",
    "getAttribute",
    "hasAttribute",
    "removeAttribute",
    "textContent",
    "innerHTML",
    "innerText",
    "style",
    "classList",
    "className",
    "id",
    "parentNode",
    "parentElement",
    "firstChild",
    "lastChild",
    "nextSibling",
    "previousSibling",
    "ownerDocument",
    "documentElement",
    "head",
    "body",
    "children",
    "childNodes",
    "forms",
    "elements",
    "document",
    "navigator",
    "location",
    "href",
    "search",
    "cookie",
    "userAgent",
    "getComputedStyle",
    "onload",
    "onclick",
    "submit",
    "focus",
];
const _: () = assert!(STANDARD_KEYS.len() <= u8::MAX as usize + 1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MemberOperation {
    Read,
    WriteTarget,
    CompoundTarget,
    UpdateTarget,
    DeleteTarget,
    CallTarget,
    ForInTarget,
}
impl MemberOperation {
    fn label(self) -> &'static str {
        match self {
            Self::Read => "resolve-read",
            Self::WriteTarget => "resolve-write-target",
            Self::CompoundTarget => "resolve-compound-target",
            Self::UpdateTarget => "resolve-update-target",
            Self::DeleteTarget => "resolve-delete-target",
            Self::CallTarget => "resolve-call-target",
            Self::ForInTarget => "resolve-for-in-target",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NullishBase {
    Null,
    Undefined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyCategory {
    Standard(u8),
    String,
    Number,
    Boolean,
    Null,
    Undefined,
    Symbol,
    Object,
    Function,
    Native,
    Host,
}
impl KeyCategory {
    fn property(key: &PropertyKey) -> Self {
        match key {
            PropertyKey::Symbol(_) => Self::Symbol,
            PropertyKey::String(key) => {
                if key.len() <= MAX_MATCH_UNITS {
                    for (index, name) in STANDARD_KEYS.iter().enumerate() {
                        if key == name {
                            return Self::Standard(index as u8);
                        }
                    }
                }
                Self::String
            }
        }
    }
    fn classify(key: &Value) -> Self {
        match key {
            Value::String(units) => {
                // Length is available without scanning. Even hostile long or
                // non-ASCII keys cost at most this fixed public vocabulary scan.
                if units.len() <= MAX_MATCH_UNITS {
                    for (index, name) in STANDARD_KEYS.iter().enumerate() {
                        if units.len() == name.len()
                            && units
                                .iter()
                                .zip(name.bytes())
                                .all(|(a, b)| *a == u16::from(b))
                        {
                            return Self::Standard(index as u8);
                        }
                    }
                }
                Self::String
            }
            Value::Number(_) => Self::Number,
            Value::Bool(_) => Self::Boolean,
            Value::Null => Self::Null,
            Value::Undefined => Self::Undefined,
            Value::Symbol(_) => Self::Symbol,
            Value::Object(_) => Self::Object,
            Value::Function(_) => Self::Function,
            Value::Native(_) => Self::Native,
            Value::Host(_) => Self::Host,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Standard(index) => STANDARD_KEYS
                .get(usize::from(index))
                .copied()
                .unwrap_or("<string>"),
            Self::String => "<string>",
            Self::Number => "<number>",
            Self::Boolean => "<boolean>",
            Self::Null => "<null>",
            Self::Undefined => "<undefined>",
            Self::Symbol => "<symbol>",
            Self::Object => "<object>",
            Self::Function => "<function>",
            Self::Native => "<native>",
            Self::Host => "<host>",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProducerKind {
    Binding,
    Expression,
    PresentProperty,
    MissingProperty,
    GetterResult,
    HostGet,
    UserCall,
    NativeCall,
    HostCall,
    BoundCall,
}
impl ProducerKind {
    fn label(self) -> &'static str {
        match self {
            Self::Binding => "binding",
            Self::Expression => "expression",
            Self::PresentProperty => "present-property",
            Self::MissingProperty => "missing-property",
            Self::GetterResult => "getter-result",
            Self::HostGet => "host-get",
            Self::UserCall => "user-call",
            Self::NativeCall => "native-call",
            Self::HostCall => "host-call",
            Self::BoundCall => "bound-call",
        }
    }
    pub(super) fn record(self, observation: Option<&mut Self>) {
        if let Some(observation) = observation {
            *observation = self;
        }
    }
}

/// One immediate successful evaluation, kept only by the enclosing reference.
/// No Value, source, property buffer, or history can survive through this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Producer {
    kind: ProducerKind,
    key: Option<KeyCategory>,
}
impl Producer {
    pub(super) const fn simple(kind: ProducerKind) -> Self {
        Self { kind, key: None }
    }
    pub(super) fn property(kind: ProducerKind, key: &PropertyKey) -> Self {
        // An existing fast path may return a value without establishing either
        // property presence or absence. Do not claim a completed lookup there.
        if kind == ProducerKind::Expression {
            return Self::simple(kind);
        }
        Self {
            kind,
            key: Some(KeyCategory::property(key)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MemberContext {
    operation: MemberOperation,
    base: NullishBase,
    key: KeyCategory,
    producer: Producer,
}
impl MemberContext {
    pub(super) fn format(self) -> String {
        // Formatting happens only after the fault escapes. Its fixed, ASCII-only
        // labels cannot call page code, consume realm fuel, or replace a failure.
        // Limit every append, rather than allocating from an arbitrary input and
        // truncating afterward. This is host output, not new realm-owned storage.
        let mut output = String::with_capacity(MAX_FORMAT_BYTES);
        for text in [
            "Uncaught JavaScript exception: ",
            NULLISH_MESSAGE,
            " [member operation=",
            self.operation.label(),
            " base=",
            match self.base {
                NullishBase::Null => "null",
                NullishBase::Undefined => "undefined",
            },
            " key=",
            self.key.label(),
            "]",
            " [producer kind=",
            self.producer.kind.label(),
            if self.producer.key.is_some() {
                " key="
            } else {
                ""
            },
            self.producer.key.map_or("", KeyCategory::label),
            "]",
        ] {
            let length = text.len().min(MAX_FORMAT_BYTES - output.len());
            output.push_str(&text[..length]);
        }
        output
    }
}

#[cfg(test)]
fn member_fault(operation: MemberOperation, base: &Value, key: &Value) -> Fault {
    member_fault_observed(
        operation,
        base,
        key,
        Producer::simple(ProducerKind::Expression),
    )
}

pub(super) fn member_fault_observed(
    operation: MemberOperation,
    base: &Value,
    key: &Value,
    producer: Producer,
) -> Fault {
    let base = match base {
        Value::Null => NullishBase::Null,
        Value::Undefined => NullishBase::Undefined,
        _ => unreachable!("member context is created only at a nullish rejection"),
    };
    Fault::Member {
        // Exactly the preexisting exception Value; context is not page-visible.
        value: Value::text(NULLISH_MESSAGE),
        context: MemberContext {
            operation,
            base,
            key: KeyCategory::classify(key),
            producer,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::{AllocationTotals, Eval, Flow, Host, MAX_HEAP, Reference, Runtime};
    use super::*;

    struct NoIo;
    impl Host for NoIo {
        fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
            panic!("unexpected Host get")
        }
        fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
            panic!("unexpected Host set")
        }
        fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
            panic!("unexpected Host call")
        }
    }
    const OPERATIONS: [MemberOperation; 7] = [
        MemberOperation::Read,
        MemberOperation::WriteTarget,
        MemberOperation::CompoundTarget,
        MemberOperation::UpdateTarget,
        MemberOperation::DeleteTarget,
        MemberOperation::CallTarget,
        MemberOperation::ForInTarget,
    ];
    fn counters(runtime: &Runtime) {
        assert_eq!(runtime.budget.calls, 0);
        assert_eq!(runtime.budget.active_expressions, 0);
        assert_eq!(runtime.budget.evaluation_entries, 0);
    }
    fn context(fault: Fault) -> MemberContext {
        let Fault::Member { value, context } = fault else {
            panic!("annotated member fault expected")
        };
        assert_eq!(value, Value::text(NULLISH_MESSAGE));
        context
    }

    #[test]
    fn fixed_context_and_fault_result_layouts_remain_bounded() {
        use std::mem::{needs_drop, size_of};
        // Exact prechange shape, retained solely as a layout comparison. Value,
        // Flow and Reference themselves are unchanged by this increment.
        #[allow(dead_code)]
        enum OriginalFault {
            Throw(Value),
            Fatal(String),
        }
        eprintln!(
            "DIAGNOSTIC_LAYOUT context={} old_fault={} fault={} old_value_result={} value_result={} old_flow_result={} flow_result={} old_reference_result={} reference_result={} object={} function={}",
            size_of::<MemberContext>(),
            size_of::<OriginalFault>(),
            size_of::<Fault>(),
            size_of::<Result<Value, OriginalFault>>(),
            size_of::<Eval<Value>>(),
            size_of::<Result<Flow, OriginalFault>>(),
            size_of::<Eval<Flow>>(),
            size_of::<Result<Reference, OriginalFault>>(),
            size_of::<Eval<Reference>>(),
            size_of::<super::super::Object>(),
            size_of::<super::super::Function>(),
        );
        assert!(!needs_drop::<MemberContext>());
        assert!(size_of::<MemberContext>() <= 7);
        assert!(size_of::<Producer>() <= 3);
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(size_of::<Fault>(), 40);
            assert_eq!(size_of::<Eval<Value>>(), 40);
            assert_eq!(size_of::<Eval<Flow>>(), 64);
            assert_eq!(size_of::<Eval<Reference>>(), 56);
        }
        assert!(size_of::<Fault>() <= size_of::<OriginalFault>() + 8);
        assert!(size_of::<Eval<Value>>() <= size_of::<Result<Value, OriginalFault>>() + 8);
        assert!(size_of::<Eval<Flow>>() <= size_of::<Result<Flow, OriginalFault>>() + 8);
        assert!(size_of::<Eval<Reference>>() <= size_of::<Result<Reference, OriginalFault>>() + 8);
        assert!(size_of::<super::super::Object>() <= 128);
    }

    #[test]
    fn fixed_vocabulary_and_categories_are_ascii_bounded_and_never_capture_keys() {
        assert!(
            STANDARD_KEYS
                .iter()
                .all(|name| name.is_ascii() && name.len() <= MAX_MATCH_UNITS)
        );
        for (index, name) in STANDARD_KEYS.iter().enumerate() {
            assert!(!STANDARD_KEYS[..index].contains(name));
            assert_eq!(KeyCategory::classify(&Value::text(name)).label(), *name);
        }
        let unknown = [
            Value::text("private-query=https://example.invalid/secret"),
            Value::text("Prototype"),
            Value::text("prototype\n"),
            Value::String(vec![0xd800]),
            Value::String(vec![120; 1_000_000]),
        ];
        for value in unknown {
            assert_eq!(KeyCategory::classify(&value), KeyCategory::String);
        }
        let categories = [
            KeyCategory::String,
            KeyCategory::Number,
            KeyCategory::Boolean,
            KeyCategory::Null,
            KeyCategory::Undefined,
            KeyCategory::Symbol,
            KeyCategory::Object,
            KeyCategory::Function,
            KeyCategory::Native,
            KeyCategory::Host,
        ];
        for operation in OPERATIONS {
            for base in [NullishBase::Null, NullishBase::Undefined] {
                for key in (0..STANDARD_KEYS.len())
                    .map(|id| KeyCategory::Standard(id as u8))
                    .chain(categories)
                {
                    let output = MemberContext {
                        operation,
                        base,
                        key,
                        producer: Producer::simple(ProducerKind::Expression),
                    }
                    .format();
                    assert!(output.is_ascii() && output.len() <= MAX_FORMAT_BYTES);
                    assert!(output.starts_with("Uncaught JavaScript exception: TypeError: property access on null or undefined [member operation="));
                    assert!(output.ends_with(']'));
                    assert!(!output.contains("private-query"));
                }
            }
        }
    }

    #[test]
    fn host_formatting_preserves_original_value_and_does_not_touch_realm_state() {
        let mut runtime = Runtime::new();
        let function = runtime
            .execute("function untouched(){}untouched;", &mut NoIo)
            .unwrap();
        let Value::Function(id) = function else {
            panic!("function expected")
        };
        assert!(runtime.functions[id].pending_default_prototype);
        runtime
            .budget
            .allocate(MAX_HEAP - runtime.budget.allocated)
            .unwrap();
        let report = runtime.allocation_report();
        let fuel = runtime.budget.fuel;
        let objects = runtime.objects.len();
        let natives = runtime.native_properties.len();
        let fault = member_fault(
            MemberOperation::Read,
            &Value::Undefined,
            &Value::text("prototype"),
        );
        let expected = context(member_fault(
            MemberOperation::Read,
            &Value::Undefined,
            &Value::text("prototype"),
        ))
        .format();
        assert_eq!(runtime.finish(Err(fault)).unwrap_err(), expected);
        assert_eq!(
            super::super::fault_text(member_fault(
                MemberOperation::Read,
                &Value::Undefined,
                &Value::text("prototype")
            )),
            expected
        );
        assert_eq!(runtime.allocation_report(), report);
        assert_eq!(runtime.budget.fuel, fuel);
        assert_eq!(runtime.objects.len(), objects);
        assert_eq!(runtime.native_properties.len(), natives);
        assert!(runtime.functions[id].pending_default_prototype);
        assert!(runtime.fatal.is_none());
        assert_eq!(
            runtime
                .finish(Err(super::super::exception(NULLISH_MESSAGE)))
                .unwrap_err(),
            format!("Uncaught JavaScript exception: {NULLISH_MESSAGE}"),
        );
        counters(&runtime);
    }

    #[test]
    fn originating_reference_keeps_exact_fuel_and_internal_checks_stay_unannotated() {
        use super::super::Expr;
        for operation in OPERATIONS {
            let mut runtime = Runtime::new();
            let before = runtime.allocation_report();
            let fuel = runtime.budget.fuel;
            let expr = Expr::Member {
                object: Box::new(Expr::Null),
                property: Box::new(Expr::String("length".encode_utf16().collect())),
            };
            let Err(fault) = runtime.reference(&expr, operation, 0, &Value::Undefined, &mut NoIo)
            else {
                panic!("null reference must fail")
            };
            assert_eq!(context(fault).operation, operation);
            // Exactly the original base and key expression steps, and the real
            // evaluated key-string copy. Classification introduces neither.
            assert_eq!(fuel - runtime.budget.fuel, 2);
            assert_eq!(
                runtime.allocation_report().accepted_bytes - before.accepted_bytes,
                12
            );
            counters(&runtime);
        }
        let mut runtime = Runtime::new();
        for result in [
            runtime.get(&Value::Null, "length", &mut NoIo),
            runtime
                .set(Value::Null, "length", Value::Number(1.0), &mut NoIo)
                .map(|_| Value::Undefined),
            runtime.delete(Value::Null, "length"),
        ] {
            assert!(matches!(result, Err(Fault::Throw(_))));
        }
        runtime.set_global("hostKey", Value::Host("private-host-handle".into()));
        assert!(
            runtime
                .execute("null[hostKey];", &mut NoIo)
                .unwrap_err()
                .ends_with("key=<host>] [producer kind=expression]")
        );
        assert!(
            runtime
                .execute(
                    "null[{toString:function(){throw 'must not run';}}];",
                    &mut NoIo
                )
                .unwrap_err()
                .ends_with("key=<object>] [producer kind=expression]")
        );
    }

    #[test]
    fn catches_finally_and_rethrows_keep_only_the_actual_pending_context() {
        let mut runtime = Runtime::new();
        assert_eq!(runtime.execute("try{null.length;}catch(e){e===\"TypeError: property access on null or undefined\";}", &mut NoIo).unwrap(), Value::Bool(true));
        let retained = runtime
            .execute(
                "try{null.prototype;}finally{try{null.length;}catch(e){}}",
                &mut NoIo,
            )
            .unwrap_err();
        assert!(retained.ends_with(
            "operation=resolve-read base=null key=prototype] [producer kind=expression]"
        ));
        let replaced = runtime
            .execute("try{null.prototype;}finally{null.length;}", &mut NoIo)
            .unwrap_err();
        assert!(
            replaced.ends_with(
                "operation=resolve-read base=null key=length] [producer kind=expression]"
            )
        );
        assert_eq!(
            runtime
                .execute("try{null.prototype;}catch(e){throw e;}", &mut NoIo)
                .unwrap_err(),
            format!("Uncaught JavaScript exception: {NULLISH_MESSAGE}")
        );
        assert_eq!(
            runtime
                .execute(
                    "function f(){try{null.prototype;}finally{return 7;}}f();",
                    &mut NoIo
                )
                .unwrap(),
            Value::Number(7.0)
        );
        assert_eq!(
            runtime.execute("throw 'later';", &mut NoIo).unwrap_err(),
            "Uncaught JavaScript exception: later"
        );
        let error = runtime
            .execute("try{null.prototype;}finally{Array(10001);}", &mut NoIo)
            .unwrap_err();
        assert_eq!(error, "JavaScript array limit exhausted");
        let report = runtime.allocation_report();
        assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
        assert_eq!(runtime.allocation_report(), report);
        counters(&runtime);
    }

    #[test]
    fn authored_prechange_charge_and_fuel_checkpoints_are_unchanged() {
        // Frozen public baseline: tmp/diagnostic-resource-baseline.log. Exact
        // authored sources, including punctuation, preserve source/AST charges.
        // Later real reduceRight/core backlinks add 156 + 725 Bootstrap bytes.
        let cases = [
            (
                "var rounds=0;for(var i=0;i<128;i++){try{null.length;}catch(e){rounds++;}}rounds;",
                76_200,
                208,
                3195,
                46_798,
            ),
            (
                "var prior=0;try{null.length;}finally{prior=7;}",
                28_270,
                174,
                1915,
                182,
            ),
            (
                "var rounds=0,ticks=0;for(var i=0;i<128;i++){try{null.length;}catch(e){rounds++;}}while(true){ticks++;}",
                76_934,
                230,
                3737,
                46_968,
            ),
        ];
        for (index, (source, total, source_bytes, ast, runtime_bytes)) in cases.iter().enumerate() {
            let mut runtime = Runtime::new();
            let result = runtime.execute(source, &mut NoIo);
            match index {
                0 => assert_eq!(result.unwrap(), Value::Number(128.0)),
                1 => {
                    assert!(
                        result
                            .unwrap_err()
                            .ends_with("key=length] [producer kind=expression]")
                    );
                    assert_eq!(runtime.get_global("prior"), Value::Number(7.0));
                }
                _ => {
                    assert_eq!(result.unwrap_err(), "JavaScript fuel exhausted");
                    assert_eq!(runtime.get_global("ticks"), Value::Number(21_462.0));
                }
            }
            let report = runtime.allocation_report();
            assert_eq!(report.accepted_bytes, *total + 156 + 725);
            assert_eq!(
                report.phases,
                AllocationTotals {
                    bootstrap: 25_999 + 156 + 725,
                    source: *source_bytes,
                    ast: *ast,
                    runtime: *runtime_bytes,
                    function_code: 0,
                    regex_compile: 0,
                    regex_result: 0
                }
            );
            assert!(report.is_valid() && report.first_rejected.is_none());
            counters(&runtime);
        }
        let mut runtime = Runtime::new();
        let function = runtime
            .execute("(function(){null[void 0];});", &mut NoIo)
            .unwrap();
        assert_eq!(
            runtime.allocation_report().accepted_bytes,
            27_264 + 156 + 725
        );
        assert!(
            runtime
                .invoke(function, Value::Undefined, vec![], &mut NoIo)
                .unwrap_err()
                .ends_with("key=<undefined>] [producer kind=expression]")
        );
        assert_eq!(
            runtime.allocation_report().accepted_bytes,
            27_529 + 156 + 725
        );
        counters(&runtime);
    }

    #[test]
    fn pending_member_faults_and_recursive_finally_stay_bounded_on_default_stack() {
        const CHILD: &str = "MGBROWSER_MEMBER_DIAGNOSTIC_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let mut runtime = Runtime::new();
            let error = runtime.execute("function recurse(n){try{null[n?'length':'prototype'];}finally{if(n)return recurse(n-1);}}recurse(8);", &mut NoIo).unwrap_err();
            assert!(error.ends_with("key=prototype] [producer kind=expression]"));
            assert_eq!(
                runtime.execute("42;", &mut NoIo).unwrap(),
                Value::Number(42.0)
            );
            counters(&runtime);
            for depth in [0, 8, 32, 96] {
                let mut runtime = Runtime::new();
                let source = format!(
                    "var caught=false,finalized=false;function recurse(){{try{{null.length;}}finally{{return {}recurse();}}}}try{{recurse();}}catch(e){{caught=true;}}finally{{finalized=true;}}",
                    "+ ".repeat(depth)
                );
                let error = runtime.execute(&source, &mut NoIo).unwrap_err();
                assert!(error.contains("depth"), "depth {depth}: {error}");
                assert_eq!(runtime.get_global("caught"), Value::Bool(false));
                assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
                assert_eq!(runtime.execute("42;", &mut NoIo).unwrap_err(), error);
                counters(&runtime);
            }
            return;
        }
        use std::{
            process::{Child, Command, Stdio},
            time::{Duration, Instant},
        };
        struct OwnedChild(Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "js::runtime::diagnostic::tests::pending_member_faults_and_recursive_finally_stay_bounded_on_default_stack", "--nocapture"])
            .env(CHILD, "1").env("RUST_BACKTRACE", "0").env_remove("RUST_MIN_STACK")
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::inherit()).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(
                    status.success(),
                    "owned member-fault child failed: {status}"
                );
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned member-fault child deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn producer_vocabulary_is_fixed_and_combined_output_remains_bounded() {
        use std::mem::needs_drop;
        assert!(!needs_drop::<Producer>());
        let kinds = [
            ProducerKind::Binding,
            ProducerKind::Expression,
            ProducerKind::PresentProperty,
            ProducerKind::MissingProperty,
            ProducerKind::GetterResult,
            ProducerKind::HostGet,
            ProducerKind::UserCall,
            ProducerKind::NativeCall,
            ProducerKind::HostCall,
            ProducerKind::BoundCall,
        ];
        for kind in kinds {
            for operation in OPERATIONS {
                for base in [NullishBase::Null, NullishBase::Undefined] {
                    for key in (0..STANDARD_KEYS.len())
                        .map(|index| KeyCategory::Standard(index as u8))
                        .chain([KeyCategory::String, KeyCategory::Symbol])
                    {
                        let producer = if matches!(
                            kind,
                            ProducerKind::PresentProperty
                                | ProducerKind::MissingProperty
                                | ProducerKind::GetterResult
                                | ProducerKind::HostGet
                        ) {
                            Producer {
                                kind,
                                key: Some(key),
                            }
                        } else {
                            Producer::simple(kind)
                        };
                        let output = MemberContext {
                            operation,
                            base,
                            key,
                            producer,
                        }
                        .format();
                        assert!(output.is_ascii() && output.len() <= MAX_FORMAT_BYTES);
                        assert!(output.ends_with(']'));
                        assert!(output.contains("] [producer kind="));
                    }
                }
            }
        }
        for name in STANDARD_KEYS {
            let property = PropertyKey::String((*name).into());
            assert_eq!(
                KeyCategory::property(&property),
                KeyCategory::classify(&Value::text(name))
            );
        }
        for secret in [
            "https://private.invalid/query",
            "42",
            "true",
            "null",
            "prototype\n",
            "π",
        ] {
            assert_eq!(
                KeyCategory::property(&PropertyKey::String(secret.into())),
                KeyCategory::String
            );
        }
    }

    fn accessor_fixture(symbol: bool, inherited: bool) -> (Runtime, Value, PropertyKey) {
        let mut runtime = Runtime::new();
        let getter = runtime.execute(
            "var reads=0;function getter(){reads++;if(this.marker!==7)throw 'wrong receiver';delete owner[key];try{null.name;}catch(e){}return this.absent;}getter;",
            &mut NoIo,
        ).unwrap();
        let key = if symbol {
            let Value::Symbol(symbol) = runtime
                .execute("Symbol('private description');", &mut NoIo)
                .unwrap()
            else {
                panic!("symbol")
            };
            runtime.set_global("key", Value::Symbol(symbol.clone()));
            PropertyKey::Symbol(symbol)
        } else {
            runtime.set_global("key", Value::text("length"));
            PropertyKey::String("length".into())
        };
        let owner = runtime
            .object(Some(runtime.object_prototype), None)
            .unwrap();
        runtime.set_global("owner", Value::Object(owner));
        match &key {
            PropertyKey::String(key) => runtime.put_own(owner, key, getter, false).unwrap(),
            PropertyKey::Symbol(key) => runtime.put_symbol(owner, key, getter, false).unwrap(),
        }
        runtime.objects[owner].properties.last_mut().unwrap().getter = true;
        let receiver = if inherited {
            runtime.object(Some(owner), None).unwrap()
        } else {
            owner
        };
        runtime
            .put_own(receiver, "marker", Value::Number(7.0), true)
            .unwrap();
        (runtime, Value::Object(receiver), key)
    }

    #[test]
    fn observation_uses_the_original_single_getter_and_receiver_without_extra_work() {
        for symbol in [false, true] {
            for inherited in [false, true] {
                let (mut ordinary, receiver, key) = accessor_fixture(symbol, inherited);
                assert_eq!(
                    ordinary.get_key(&receiver, &key, &mut NoIo).unwrap(),
                    Value::Undefined
                );
                let expected_report = ordinary.allocation_report();
                let expected_fuel = ordinary.budget.fuel;
                let (mut observed, receiver, key) = accessor_fixture(symbol, inherited);
                let mut kind = ProducerKind::Expression;
                assert_eq!(
                    observed
                        .get_key_observed(&receiver, &key, &mut NoIo, Some(&mut kind))
                        .unwrap(),
                    Value::Undefined
                );
                // The getter deleted itself and performed a nested missing read;
                // neither a second traversal nor nested observations may relabel it.
                assert_eq!(kind, ProducerKind::GetterResult);
                assert_eq!(observed.get_global("reads"), Value::Number(1.0));
                assert_eq!(observed.allocation_report(), expected_report);
                assert_eq!(observed.budget.fuel, expected_fuel);
                counters(&ordinary);
                counters(&observed);
                assert_eq!(
                    observed
                        .get_key_observed(&receiver, &key, &mut NoIo, Some(&mut kind))
                        .unwrap(),
                    Value::Undefined
                );
                assert_eq!(kind, ProducerKind::MissingProperty);
                assert_eq!(observed.get_global("reads"), Value::Number(1.0));
            }
        }
    }

    #[test]
    fn failed_getters_never_publish_successful_origin_or_relabel_inner_faults() {
        for source in [
            "function fail(){null.length;}fail;",
            "function fail(){while(true){}}fail;",
        ] {
            let mut runtime = Runtime::new();
            let getter = runtime.execute(source, &mut NoIo).unwrap();
            let object = runtime
                .object(Some(runtime.object_prototype), None)
                .unwrap();
            runtime.put_own(object, "prototype", getter, false).unwrap();
            runtime.objects[object]
                .properties
                .last_mut()
                .unwrap()
                .getter = true;
            let key = PropertyKey::String("prototype".into());
            let mut kind = ProducerKind::Binding;
            let outcome =
                runtime.get_key_observed(&Value::Object(object), &key, &mut NoIo, Some(&mut kind));
            assert_eq!(kind, ProducerKind::Binding);
            let error = runtime.finish(outcome).unwrap_err();
            if source.contains("while") {
                assert_eq!(error, "JavaScript fuel exhausted");
                assert!(runtime.is_fatal());
            } else {
                assert_eq!(
                    error,
                    "Uncaught JavaScript exception: TypeError: property access on null or undefined [member operation=resolve-read base=null key=length] [producer kind=expression]"
                );
                assert!(!runtime.is_fatal());
            }
            counters(&runtime);
        }
    }

    #[test]
    fn primitive_string_out_of_range_is_not_claimed_as_a_missing_lookup() {
        let mut runtime = Runtime::new();
        assert_eq!(
            runtime
                .execute("String.prototype[9]=null;String.prototype[9];", &mut NoIo)
                .unwrap(),
            Value::Null
        );
        let key = PropertyKey::String("9".into());
        let mut kind = ProducerKind::MissingProperty;
        let fuel = runtime.budget.fuel;
        let before = runtime.allocation_report();
        assert_eq!(
            runtime
                .get_key_observed(&Value::text("x"), &key, &mut NoIo, Some(&mut kind))
                .unwrap(),
            Value::Undefined
        );
        // Preserve the old untraversed primitive-string fast path. Inherited
        // string-index compatibility is separate from diagnostic observation.
        assert_eq!(kind, ProducerKind::Expression);
        assert_eq!(Producer::property(kind, &key), Producer::simple(kind));
        assert_eq!(runtime.budget.fuel, fuel - 1);
        assert_eq!(runtime.allocation_report(), before);
        assert_eq!(
            runtime.execute("'x'[9].length;", &mut NoIo).unwrap_err(),
            "Uncaught JavaScript exception: TypeError: property access on null or undefined [member operation=resolve-read base=undefined key=length] [producer kind=expression]"
        );
        counters(&runtime);
    }
}
