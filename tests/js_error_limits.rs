//! Independently authored Error-family resource checks. No website input,
//! alternate evaluator, enlarged stack or relaxed runtime limit is used.

use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;
const FAMILIES: [&str; 6] = [
    "Error",
    "TypeError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "URIError",
];

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("unexpected host get: {object}.{key}");
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("unexpected host set: {object}.{key}");
    }
    fn call(&mut self, name: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected host call: {name}");
    }
}

fn report(runtime: &Runtime) -> AllocationReport {
    let result = runtime.allocation_report();
    assert!(result.is_valid(), "{result:?}");
    assert_eq!(result.limit_bytes, LIMIT);
    result
}

fn runtime_only(before: AllocationReport, after: AllocationReport) -> u64 {
    assert_eq!(after.phases.bootstrap, before.phases.bootstrap);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.function_code, before.phases.function_code);
    assert_eq!(after.phases.regex_compile, before.phases.regex_compile);
    assert_eq!(after.phases.regex_result, before.phases.regex_result);
    let delta = after.phases.runtime - before.phases.runtime;
    assert_eq!(after.accepted_bytes - before.accepted_bytes, delta);
    delta
}

fn latch(runtime: &mut Runtime, error: &str, first: AllocationReport) {
    assert_eq!(
        runtime
            .execute("var forbidden_later=true;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(runtime.get_global("forbidden_later"), Value::Undefined);
    assert_eq!(
        runtime
            .invoke(
                Value::Native("Error".into()),
                Value::Undefined,
                vec![],
                &mut NoIo
            )
            .unwrap_err(),
        error
    );
    runtime.set_global("forbidden_ingress", Value::text("not admitted"));
    assert_eq!(runtime.get_global("forbidden_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn allocation_failure(runtime: &Runtime, error: &str, request: u64) -> AllocationReport {
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes, request);
    assert_eq!(rejected.accepted_bytes, first.accepted_bytes);
    assert!(rejected.requested_bytes > LIMIT - first.accepted_bytes);
    first
}

fn leave_budget(runtime: &mut Runtime, desired: u64) {
    // Ordinary public string ingress consumes the budget. Allow a KiB for its
    // existing property admission rather than assuming an internal layout.
    let remaining = LIMIT - report(runtime).accepted_bytes;
    assert!(remaining > desired + 1024);
    let units = ((remaining - desired - 1024) / 2) as usize;
    runtime.set_global("padding", Value::String(vec![b'p' as u16; units]));
    let after = report(runtime);
    assert!(after.first_rejected.is_none());
    assert!((desired..=desired + 1024).contains(&(LIMIT - after.accepted_bytes)));
}

fn construct(runtime: &mut Runtime, family: &str, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(
        Value::Native(family.into()),
        Value::Undefined,
        args,
        &mut NoIo,
    )
}

fn stringify(runtime: &mut Runtime, receiver: Value) -> Result<Value, String> {
    runtime.invoke(
        Value::Native("Error.toString".into()),
        receiver,
        vec![],
        &mut NoIo,
    )
}

fn prototype(runtime: &mut Runtime, object: Value) -> Value {
    runtime
        .invoke(
            Value::Native("Object.getPrototypeOf".into()),
            Value::Undefined,
            vec![object],
            &mut NoIo,
        )
        .unwrap()
}

#[test]
fn bootstrap_is_deterministic_exclusive_and_keeps_the_fixed_limit() {
    let first = report(&Runtime::new());
    assert_eq!(first, report(&Runtime::new()));
    assert!(first.accepted_bytes > 0);
    assert_eq!(first.accepted_bytes, first.phases.bootstrap);
    assert_eq!(first.phases.runtime, 0);
    assert_eq!(first.phases.source, 0);
    assert_eq!(first.phases.ast, 0);
    assert_eq!(first.phases.function_code, 0);
    assert_eq!(first.phases.regex_compile, 0);
    assert_eq!(first.phases.regex_result, 0);
    assert!(first.first_rejected.is_none());
}

#[test]
fn each_family_reuses_its_prototype_without_per_instance_bootstrap_storage() {
    let mut runtime = Runtime::new();
    let mut prototypes = Vec::new();
    for family in FAMILIES {
        // Warm constructor-specific lazy bookkeeping before comparing instances.
        construct(&mut runtime, family, vec![]).unwrap();
        let before = report(&runtime);
        let first = construct(&mut runtime, family, vec![]).unwrap();
        let middle = report(&runtime);
        let second = construct(&mut runtime, family, vec![]).unwrap();
        let after = report(&runtime);
        assert_eq!(
            runtime_only(before, middle),
            runtime_only(middle, after),
            "{family}"
        );
        assert_ne!(first, second);
        let owner = prototype(&mut runtime, first);
        assert_eq!(owner, prototype(&mut runtime, second));
        assert!(!prototypes.contains(&owner));
        prototypes.push(owner);
    }
    assert!(report(&runtime).first_rejected.is_none());
}

fn constructor_cost(family: &str, length: usize) -> u64 {
    let mut runtime = Runtime::new();
    let before = report(&runtime);
    let result = construct(
        &mut runtime,
        family,
        vec![Value::String(vec![0xd800; length])],
    )
    .unwrap();
    assert!(matches!(result, Value::Object(_)));
    runtime_only(before, report(&runtime))
}

#[test]
fn constructor_ingress_first_clone_and_retained_message_costs_stay_unchanged() {
    for family in FAMILIES {
        assert_eq!(
            constructor_cost(family, 1024) - constructor_cost(family, 512),
            512 * 6,
            "{family}"
        );
    }
}

#[test]
fn constructor_real_first_copy_and_retained_message_admission_both_preflight() {
    for length in [1_100_000, 900_000] {
        let mut runtime = Runtime::new();
        let error = construct(
            &mut runtime,
            "Error",
            vec![Value::String(vec![0xd800; length])],
        )
        .unwrap_err();
        let first = allocation_failure(&runtime, &error, (length * 2) as u64);
        let admitted_copies = if length == 1_100_000 { 1 } else { 2 };
        let admitted_payload = (length * 2 * admitted_copies) as u64;
        assert!((admitted_payload..admitted_payload + 1024).contains(&first.phases.runtime));
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn reading_a_retained_message_still_requires_a_real_copy_before_completion() {
    let mut runtime = Runtime::new();
    let object = construct(
        &mut runtime,
        "Error",
        vec![Value::String(vec![0xdfff; 100_000])],
    )
    .unwrap();
    runtime.set_global("item", object);
    let reader = runtime.execute("var entered=false,returned=false,caught=false,finalized=false;function read(){entered=true;try{var result=item.message;returned=true;return result;}catch(e){caught=true;}finally{finalized=true;}}read;", &mut NoIo).unwrap();
    leave_budget(&mut runtime, 100_000);
    let error = runtime
        .invoke(reader, Value::Undefined, vec![], &mut NoIo)
        .unwrap_err();
    assert_eq!(runtime.get_global("entered"), Value::Bool(true));
    for name in ["returned", "caught", "finalized"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    let first = allocation_failure(&runtime, &error, 200_000);
    latch(&mut runtime, &error, first);
}

fn stringification_cost(length: usize, empty_name: bool) -> u64 {
    let mut runtime = Runtime::new();
    let object = construct(
        &mut runtime,
        "Error",
        vec![Value::String(vec![0xd800; length])],
    )
    .unwrap();
    if empty_name {
        runtime.set_global("item", object.clone());
        runtime.execute("item.name='';", &mut NoIo).unwrap();
    }
    let before = report(&runtime);
    let Value::String(result) = stringify(&mut runtime, object).unwrap() else {
        panic!("expected string")
    };
    let delta = runtime_only(before, report(&runtime));
    assert_eq!(result.len(), length + if empty_name { 0 } else { 7 });
    assert_eq!(&result[result.len() - length..], vec![0xd800; length]);
    delta
}

#[test]
fn active_stringification_charges_true_reads_and_only_a_needed_join() {
    assert_eq!(
        stringification_cost(1024, false) - stringification_cost(512, false),
        512 * 4
    );
    assert_eq!(
        stringification_cost(1024, true) - stringification_cost(512, true),
        512 * 2
    );
}

#[test]
fn joined_error_output_is_preflighted_after_successful_field_reads() {
    let mut runtime = Runtime::new();
    let length = 8192;
    let object = construct(
        &mut runtime,
        "Error",
        vec![Value::String(vec![b'm' as u16; length])],
    )
    .unwrap();
    // The 16 KiB message read fits; its new name-colon-message output does not.
    leave_budget(&mut runtime, (length * 3) as u64);
    let before = report(&runtime);
    let error = stringify(&mut runtime, object).unwrap_err();
    let first = allocation_failure(&runtime, &error, ((length + 7) * 2) as u64);
    assert!(runtime_only(before, first) >= (length * 2) as u64);
    latch(&mut runtime, &error, first);
}

struct LongNameHost {
    reads: Vec<String>,
    length: usize,
}
impl Host for LongNameHost {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!(object, "receiver");
        self.reads.push(key.into());
        match key {
            "name" => Ok(Value::String(vec![b'n' as u16; self.length])),
            "message" => panic!("message read after fatal name ingress"),
            _ => panic!("unexpected host field: {key}"),
        }
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected host set")
    }
    fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected host call")
    }
}

#[test]
fn host_field_ingress_is_admitted_before_the_next_field_is_observed() {
    let mut runtime = Runtime::new();
    let mut host = LongNameHost {
        reads: vec![],
        length: 8192,
    };
    leave_budget(&mut runtime, 8192);
    let error = runtime
        .invoke(
            Value::Native("Error.toString".into()),
            Value::Host("receiver".into()),
            vec![],
            &mut host,
        )
        .unwrap_err();
    assert_eq!(host.reads, ["name"]);
    let first = allocation_failure(&runtime, &error, 16384);
    latch(&mut runtime, &error, first);
}

fn abrupt_stringification(source: &str, expected: &str) {
    let mut runtime = Runtime::new();
    let error = runtime.execute(source, &mut NoIo).unwrap_err();
    assert!(error.contains(expected), "{error}");
    assert_eq!(runtime.get_global("entered"), Value::Bool(true));
    for name in ["message_seen", "caught", "finalized", "after"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn name_coercion_fuel_exhaustion_bypasses_message_catch_and_finally() {
    abrupt_stringification(
        "var entered=false,message_seen=false,caught=false,finalized=false,after=false;var receiver={name:{toString:function(){entered=true;while(true){}}},message:{toString:function(){message_seen=true;return 'bad';}}};try{Error.prototype.toString.call(receiver);after=true;}catch(e){caught=true;}finally{finalized=true;}",
        "fuel exhausted",
    );
}

#[test]
fn recursive_name_coercion_is_bounded_on_the_default_stack() {
    abrupt_stringification(
        "var entered=false,message_seen=false,caught=false,finalized=false,after=false;var receiver={name:{toString:function(){entered=true;return Error.prototype.toString.call(receiver);}},message:{toString:function(){message_seen=true;return 'bad';}}};try{Error.prototype.toString.call(receiver);after=true;}catch(e){caught=true;}finally{finalized=true;}",
        "call depth exhausted",
    );
}

#[test]
fn active_error_field_lookup_retains_the_existing_prototype_depth_limit() {
    abrupt_stringification(
        "var entered=false,message_seen=false,caught=false,finalized=false,after=false;var receiver={name:'deep',message:'value'};for(var i=0;i<70;i++)receiver=Object.create(receiver);try{entered=true;Error.prototype.toString.call(receiver);after=true;}catch(e){caught=true;}finally{finalized=true;}",
        "prototype depth limit exhausted",
    );
}

#[test]
fn no_message_instances_reach_the_existing_object_cap_not_an_added_budget() {
    let mut runtime = Runtime::new();
    let mut count = 0;
    let error = loop {
        assert!(count < 10_000, "object cap was not enforced");
        match construct(&mut runtime, "Error", vec![]) {
            Ok(value) => {
                assert!(matches!(value, Value::Object(_)));
                count += 1;
            }
            Err(error) => break error,
        }
    };
    assert!(
        (9900..10_000).contains(&count),
        "unexpected early failure after {count}: {error}"
    );
    assert!(error.contains("object limit exhausted"), "{error}");
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn uncaught_error_diagnostics_never_invoke_name_message_or_tostring_hooks() {
    let mut runtime = Runtime::new();
    let thrower = runtime.execute("var touched=0;var item=Error('safe');item.name={toString:function(){touched++;while(true){}}};item.message={toString:function(){touched++;throw 99;}};item.toString=function(){touched++;while(true){}};function fail(){throw item;}fail;", &mut NoIo).unwrap();
    let error = runtime
        .invoke(thrower, Value::Undefined, vec![], &mut NoIo)
        .unwrap_err();
    assert!(
        error.starts_with("Uncaught JavaScript exception: "),
        "{error}"
    );
    assert!(error.contains("[object]"), "{error}");
    assert_eq!(runtime.get_global("touched"), Value::Number(0.0));
    assert!(report(&runtime).first_rejected.is_none());
    assert_eq!(
        runtime.execute("touched===0;", &mut NoIo).unwrap(),
        Value::Bool(true)
    );
    runtime.set_global("never_host", Value::Host("never read this host".into()));
    let error = runtime
        .execute(
            "item.name=never_host;item.message=never_host;throw item;",
            &mut NoIo,
        )
        .unwrap_err();
    assert_eq!(error, "Uncaught JavaScript exception: [host]: [host]");
    assert_eq!(runtime.get_global("touched"), Value::Number(0.0));
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn final_diagnostic_is_bounded_and_adds_no_realm_charge_for_a_large_message() {
    let mut runtime = Runtime::new();
    let object = construct(
        &mut runtime,
        "Error",
        vec![Value::String(vec![0xd800; 100_000])],
    )
    .unwrap();
    runtime.set_global("item", object);
    runtime
        .execute(
            "function give(){return item;}function fail(){throw item;}",
            &mut NoIo,
        )
        .unwrap();
    let before = report(&runtime);
    runtime
        .invoke(
            runtime.get_global("give"),
            Value::Undefined,
            vec![],
            &mut NoIo,
        )
        .unwrap();
    let middle = report(&runtime);
    let error = runtime
        .invoke(
            runtime.get_global("fail"),
            Value::Undefined,
            vec![],
            &mut NoIo,
        )
        .unwrap_err();
    let after = report(&runtime);
    assert_eq!(runtime_only(before, middle), runtime_only(middle, after));
    assert!(
        error.starts_with("Uncaught JavaScript exception: Error: "),
        "{error}"
    );
    assert!(error.ends_with("... [truncated]"));
    assert!(error.encode_utf16().count() <= 4096);
    assert!(error.len() <= 4096 * 3);
    assert!(after.first_rejected.is_none());
    assert_eq!(
        runtime.execute("42;", &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn ordinary_error_diagnostic_near_the_budget_does_not_become_a_fatal_copy() {
    let mut runtime = Runtime::new();
    let object = construct(
        &mut runtime,
        "TypeError",
        vec![Value::String(vec![b'm' as u16; 100_000])],
    )
    .unwrap();
    runtime.set_global("item", object);
    let thrower = runtime
        .execute("function fail(){throw item;}fail;", &mut NoIo)
        .unwrap();
    leave_budget(&mut runtime, 4096);
    let error = runtime
        .invoke(thrower, Value::Undefined, vec![], &mut NoIo)
        .unwrap_err();
    assert!(
        error.starts_with("Uncaught JavaScript exception: TypeError: "),
        "{error}"
    );
    assert!(error.encode_utf16().count() <= 4096);
    assert!(report(&runtime).first_rejected.is_none());
    assert_eq!(
        runtime.execute("42;", &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
}
