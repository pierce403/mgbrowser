//! Authored resource checks for lazy default user-function prototypes. Public
//! Runtime APIs and ordinary paid ingress only; no site input, alternate engine,
//! increased stack, changed cap or private allocation bypass.

use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;
const CALL: u64 = 265;
const RETAINED: u64 = 128 + 128 + "prototype".len() as u64;
const MATERIALIZE: u64 = 128 + 128 + "constructor".len() as u64;
const READ: u64 = CALL + 2 * "prototype".len() as u64;

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
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, LIMIT);
    report
}

fn deltas(before: AllocationReport, after: AllocationReport) -> (u64, u64) {
    assert_eq!(after.phases.bootstrap, before.phases.bootstrap);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.regex_compile, before.phases.regex_compile);
    assert_eq!(after.phases.regex_result, before.phases.regex_result);
    let code = after.phases.function_code - before.phases.function_code;
    let runtime = after.phases.runtime - before.phases.runtime;
    assert_eq!(after.accepted_bytes - before.accepted_bytes, code + runtime);
    (code, runtime)
}

fn invoke(runtime: &mut Runtime, function: Value, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(function, Value::Undefined, args, &mut NoIo)
}

fn invoke_global(runtime: &mut Runtime, name: &str) -> Result<Value, String> {
    let function = runtime.get_global(name);
    invoke(runtime, function, vec![])
}

fn leave_budget(runtime: &mut Runtime, desired: u64) {
    runtime.set_global("padding", Value::String(Vec::new()));
    let remaining = LIMIT - report(runtime).accepted_bytes;
    assert!(remaining > desired + 2);
    let units = ((remaining - desired) / 2) as usize;
    runtime.set_global("padding", Value::String(vec![b'p' as u16; units]));
    let after = report(runtime);
    assert!(after.first_rejected.is_none());
    assert!((desired..=desired + 1).contains(&(LIMIT - after.accepted_bytes)));
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
        invoke(runtime, Value::Native("Number".into()), vec![]).unwrap_err(),
        error
    );
    runtime.set_global("forbidden_ingress", Value::text("not admitted"));
    assert_eq!(runtime.get_global("forbidden_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn failure(
    runtime: &Runtime,
    error: &str,
    phase: AllocationPhase,
    request: u64,
) -> AllocationReport {
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, phase);
    assert_eq!(rejected.accepted_bytes, first.accepted_bytes);
    assert_eq!(rejected.requested_bytes, request);
    assert!(request > LIMIT - first.accepted_bytes);
    first
}

fn reader_realm() -> Runtime {
    let mut runtime = Runtime::new();
    runtime.execute(
        "var target=function(){};function read(){return target.prototype;}function unread(){return 7;}",
        &mut NoIo,
    ).unwrap();
    runtime
}

#[test]
fn fresh_anonymous_functions_retain_real_metadata_without_copying_code_or_unused_defaults() {
    assert_eq!(RETAINED, 265);
    assert_eq!(MATERIALIZE, 267);
    for body in [String::new(), "0;".repeat(1000)] {
        let mut runtime = Runtime::new();
        let initial = report(&runtime);
        // Real Function.bind adds 145 bytes; Array.reduceRight adds 156.
        assert_eq!(initial.accepted_bytes, 25_854 + 145 + 156 + 725);
        assert_eq!(initial.phases.bootstrap, initial.accepted_bytes);
        let factory = runtime
            .execute(
                &format!("(function(){{return function(){{{body}return 7;}};}});"),
                &mut NoIo,
            )
            .unwrap();
        let before = report(&runtime);
        let mut values = Vec::new();
        for _ in 0..64 {
            let value = invoke(&mut runtime, factory.clone(), vec![]).unwrap();
            assert!(matches!(value, Value::Function(_)));
            assert!(!values.contains(&value));
            values.push(value);
        }
        // Frozen eager-default observation: Runtime797 + FunctionCode128/call.
        assert_eq!(
            deltas(before, report(&runtime)),
            (64 * 128, 64 * (CALL + RETAINED))
        );
        for value in [values[0].clone(), values[63].clone()] {
            assert_eq!(
                invoke(&mut runtime, value, vec![]).unwrap(),
                Value::Number(7.0)
            );
        }
    }
}

#[test]
fn named_expression_self_scope_and_name_storage_keep_their_existing_charges() {
    let mut runtime = Runtime::new();
    let factory = runtime
        .execute(
            "(function(){return function named(){return named;};});",
            &mut NoIo,
        )
        .unwrap();
    let before = report(&runtime);
    let value = invoke(&mut runtime, factory, vec![]).unwrap();
    assert_eq!(
        deltas(before, report(&runtime)),
        (128 + 5, CALL + RETAINED + 128 + 128 + 5)
    );
    assert_eq!(invoke(&mut runtime, value.clone(), vec![]).unwrap(), value);
}

#[test]
fn actual_first_read_pays_object_and_backlink_once_and_retains_distinct_identity() {
    let mut runtime = reader_realm();
    let before = report(&runtime);
    let first = invoke_global(&mut runtime, "read").unwrap();
    assert!(matches!(first, Value::Object(_)));
    assert_eq!(deltas(before, report(&runtime)), (0, READ + MATERIALIZE));
    let before = report(&runtime);
    assert_eq!(invoke_global(&mut runtime, "read").unwrap(), first);
    assert_eq!(deltas(before, report(&runtime)), (0, READ));
    runtime.set_global("first", first.clone());
    assert_eq!(runtime.execute(
        "var other=function(){};first.constructor===target&&other.prototype!==first&&other.prototype.constructor===other;",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
}

#[test]
fn metadata_names_presence_delete_and_function_parent_do_not_force_default_values() {
    for (operation, expected) in [
        (
            "return target.name===''&&target.length===0&&typeof target==='function';",
            true,
        ),
        ("return target.hasOwnProperty('prototype');", true),
        ("return 'prototype' in target;", true),
        ("return delete target.prototype;", false),
        (
            "return Object.getPrototypeOf(target)===Function.prototype;",
            true,
        ),
        ("return Object.keys(target).length===0;", true),
        (
            "return Object.getOwnPropertyNames(target).join(',')==='length,name,prototype';",
            true,
        ),
        (
            "return Object.getOwnPropertySymbols(target).length===0;",
            true,
        ),
        ("for(var key in target){return false;}return true;", true),
    ] {
        let mut runtime = reader_realm();
        runtime
            .execute(&format!("function examine(){{{operation}}}"), &mut NoIo)
            .unwrap();
        assert_eq!(
            invoke_global(&mut runtime, "examine").unwrap(),
            Value::Bool(expected),
            "{operation}"
        );
        let before = report(&runtime);
        invoke_global(&mut runtime, "read").unwrap();
        assert_eq!(
            deltas(before, report(&runtime)),
            (0, READ + MATERIALIZE),
            "{operation}"
        );
    }
}

#[test]
fn successful_override_cancels_only_the_unused_default_and_retains_real_write_work() {
    for replacement in ["7", "null", "{}", "target"] {
        let mut runtime = reader_realm();
        runtime.execute(
            &format!("var replacement={replacement};function replace(){{target.prototype=replacement;return 7;}}"),
            &mut NoIo,
        ).unwrap();
        let before = report(&runtime);
        assert_eq!(
            invoke_global(&mut runtime, "replace").unwrap(),
            Value::Number(7.0)
        );
        assert_eq!(deltas(before, report(&runtime)), (0, READ));
        let before = report(&runtime);
        let value = invoke_global(&mut runtime, "read").unwrap();
        assert_eq!(value, runtime.get_global("replacement"));
        assert_eq!(deltas(before, report(&runtime)), (0, READ));
        assert_eq!(
            runtime
                .execute("delete target.prototype;", &mut NoIo)
                .unwrap(),
            Value::Bool(false)
        );
    }
}

fn payload_cost(length: usize, property: &str) -> (u64, u64) {
    let mut runtime = Runtime::new();
    runtime.execute(&format!(
        "var target=function(){{}};function store(value){{target.{property}=value;return 7;}}function read(){{return target.{property};}}"
    ), &mut NoIo).unwrap();
    let before = report(&runtime);
    let store = runtime.get_global("store");
    invoke(
        &mut runtime,
        store,
        vec![Value::String(vec![0xd800; length])],
    )
    .unwrap();
    let written = deltas(before, report(&runtime));
    assert_eq!(written.0, 0);
    let before = report(&runtime);
    assert_eq!(
        invoke_global(&mut runtime, "read").unwrap(),
        Value::String(vec![0xd800; length])
    );
    let read = deltas(before, report(&runtime));
    assert_eq!(read.0, 0);
    (written.1, read.1)
}

#[test]
fn prototype_replacement_and_unrelated_properties_keep_real_utf16_copy_charges() {
    for property in ["prototype", "note"] {
        let small = payload_cost(512, property);
        let large = payload_cost(1024, property);
        // Ingress, formal copy, identifier read, assignment-result copy and
        // ordinary property transfer remain charged; a later Get copies again.
        assert_eq!(large.0 - small.0, 512 * 10, "{property}");
        assert_eq!(large.1 - small.1, 512 * 2, "{property}");
    }
}

fn guarded_operation(operation: &str) -> Runtime {
    let mut runtime = Runtime::new();
    runtime.execute(&format!(
        "var target=function(){{}},entered=false,returned=false,caught=false,finalized=false,result=17;function run(){{entered=true;try{{{operation}returned=true;}}catch(e){{caught=true;}}finally{{finalized=true;}}}}"
    ), &mut NoIo).unwrap();
    runtime
}

fn interrupted(runtime: &Runtime) {
    assert_eq!(runtime.get_global("entered"), Value::Bool(true));
    assert_eq!(runtime.get_global("result"), Value::Number(17.0));
    for name in ["returned", "caught", "finalized"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
}

#[test]
fn real_function_code_bag_and_pending_property_are_admitted_before_publication() {
    for (remaining, phase, request, expected) in [
        (380, AllocationPhase::FunctionCode, 128, (0, CALL)),
        (500, AllocationPhase::Runtime, 128, (128, CALL)),
        (600, AllocationPhase::Runtime, 137, (128, CALL + 128)),
    ] {
        let mut runtime = guarded_operation("result=function(){};");
        leave_budget(&mut runtime, remaining);
        let before = report(&runtime);
        let error = invoke_global(&mut runtime, "run").unwrap_err();
        let first = failure(&runtime, &error, phase, request);
        assert_eq!(deltas(before, first), expected);
        interrupted(&runtime);
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn first_value_read_object_and_backlink_rejections_leave_no_partial_result() {
    for (remaining, request, accepted) in [(400, 128, READ), (420, 139, READ + 128)] {
        let mut runtime = guarded_operation("result=target.prototype;");
        leave_budget(&mut runtime, remaining);
        let before = report(&runtime);
        let error = invoke_global(&mut runtime, "run").unwrap_err();
        let first = failure(&runtime, &error, AllocationPhase::Runtime, request);
        assert_eq!(deltas(before, first), (0, accepted));
        interrupted(&runtime);
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn new_and_instanceof_preflight_the_real_default_before_constructor_or_result_effects() {
    for operation in ["result=new target();", "result=subject instanceof target;"] {
        for (remaining, request, accepted) in [(390, 128, CALL), (420, 139, CALL + 128)] {
            let mut runtime = Runtime::new();
            runtime.execute(&format!(
                "var constructed=false,target=function(){{constructed=true;}},subject={{}},entered=false,returned=false,caught=false,finalized=false,result=17;function run(){{entered=true;try{{{operation}returned=true;}}catch(e){{caught=true;}}finally{{finalized=true;}}}}"
            ), &mut NoIo).unwrap();
            leave_budget(&mut runtime, remaining);
            let before = report(&runtime);
            let error = invoke_global(&mut runtime, "run").unwrap_err();
            let first = failure(&runtime, &error, AllocationPhase::Runtime, request);
            assert_eq!(deltas(before, first), (0, accepted), "{operation}");
            assert_eq!(runtime.get_global("constructed"), Value::Bool(false));
            interrupted(&runtime);
            latch(&mut runtime, &error, first);
        }
    }
    // The existing primitive-LHS instanceof short circuit does not read the
    // constructor's prototype value and must not manufacture its storage.
    let mut runtime = reader_realm();
    runtime
        .execute(
            "function primitive(){return 7 instanceof target;}",
            &mut NoIo,
        )
        .unwrap();
    leave_budget(&mut runtime, 300);
    let before = report(&runtime);
    assert_eq!(
        invoke_global(&mut runtime, "primitive").unwrap(),
        Value::Bool(false)
    );
    assert_eq!(deltas(before, report(&runtime)), (0, CALL));
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn numeric_override_fits_without_default_allocation_when_the_remaining_heap_is_small() {
    let mut runtime = reader_realm();
    runtime
        .execute(
            "function replace(){target.prototype=7;return 7;}",
            &mut NoIo,
        )
        .unwrap();
    leave_budget(&mut runtime, 300);
    let before = report(&runtime);
    assert_eq!(
        invoke_global(&mut runtime, "replace").unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(deltas(before, report(&runtime)), (0, READ));
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn failed_real_override_payload_transfer_remains_fatal_before_later_effects() {
    let mut runtime = Runtime::new();
    runtime.execute(
        "var target=function(){},entered=false,returned=false,caught=false,finalized=false,result=17;function store(value){entered=true;try{target.prototype=value;returned=true;}catch(e){caught=true;}finally{finalized=true;}}",
        &mut NoIo,
    ).unwrap();
    leave_budget(&mut runtime, 9000);
    let before = report(&runtime);
    let store = runtime.get_global("store");
    let error = invoke(&mut runtime, store, vec![Value::String(vec![0xd800; 1024])]).unwrap_err();
    let first = failure(&runtime, &error, AllocationPhase::Runtime, 2048);
    // Four real payload copies/admissions succeed; ordinary property transfer
    // is the fifth and must still fail. Private owner checks pending state.
    assert_eq!(deltas(before, first), (0, 724 + 18 + 8 * 1024));
    interrupted(&runtime);
    latch(&mut runtime, &error, first);
}

#[test]
fn six_thousand_unused_defaults_fit_but_continued_creation_still_exhausts_the_same_heap() {
    let mut runtime = Runtime::new();
    let factory = runtime
        .execute("(function(){return function(){};});", &mut NoIo)
        .unwrap();
    let before = report(&runtime);
    for _ in 0..6000 {
        assert!(matches!(
            invoke(&mut runtime, factory.clone(), vec![]).unwrap(),
            Value::Function(_)
        ));
    }
    assert_eq!(
        deltas(before, report(&runtime)),
        (6000 * 128, 6000 * (CALL + RETAINED))
    );
    let mut completed = 6000;
    let error = loop {
        match invoke(&mut runtime, factory.clone(), vec![]) {
            Ok(Value::Function(_)) => completed += 1,
            Ok(value) => panic!("unexpected factory result: {value:?}"),
            Err(error) => break error,
        }
        assert!(completed < 7000);
    };
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(&runtime);
    assert!(first.first_rejected.is_some());
    latch(&mut runtime, &error, first);
}

fn array(runtime: &mut Runtime) -> Result<Value, String> {
    invoke(runtime, Value::Native("Array".into()), vec![])
}

#[test]
fn actual_object_cap_is_enforced_on_first_default_read_not_unrelated_function_use() {
    let mut calibration = reader_realm();
    let mut available = 0;
    let error = loop {
        match array(&mut calibration) {
            Ok(Value::Object(_)) => available += 1,
            Ok(value) => panic!("unexpected array: {value:?}"),
            Err(error) => break error,
        }
        assert!(available <= 10_000);
    };
    assert!(error.contains("object limit exhausted"), "{error}");
    assert!((9900..10_000).contains(&available));
    assert!(report(&calibration).first_rejected.is_none());
    let mut runtime = reader_realm();
    for _ in 0..available {
        array(&mut runtime).unwrap();
    }
    let before = report(&runtime);
    assert_eq!(
        invoke_global(&mut runtime, "unread").unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(deltas(before, report(&runtime)), (0, CALL));
    let before = report(&runtime);
    let error = invoke_global(&mut runtime, "read").unwrap_err();
    assert!(error.contains("object limit exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(deltas(before, first), (0, READ));
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn inherited_value_reads_materialize_the_owner_once_with_the_existing_depth_edge() {
    for depth in [1, 64, 65] {
        let mut runtime = Runtime::new();
        runtime.execute(&format!(
            "var target=function(){{}},child=target;for(var i=0;i<{depth};i++)child=Object.create(child);function read(){{return child.prototype;}}function owner(){{return target.prototype;}}"
        ), &mut NoIo).unwrap();
        let before = report(&runtime);
        let result = invoke_global(&mut runtime, "read");
        if depth == 65 {
            let error = result.unwrap_err();
            assert!(error.contains("prototype depth limit exhausted"), "{error}");
            let first = report(&runtime);
            assert_eq!(deltas(before, first), (0, READ));
            assert!(first.first_rejected.is_none());
            latch(&mut runtime, &error, first);
        } else {
            let inherited = result.unwrap();
            assert_eq!(deltas(before, report(&runtime)), (0, READ + MATERIALIZE));
            let before = report(&runtime);
            assert_eq!(invoke_global(&mut runtime, "owner").unwrap(), inherited);
            assert_eq!(deltas(before, report(&runtime)), (0, READ));
        }
    }
}

#[test]
fn fuel_exhaustion_before_first_value_read_does_not_create_default_storage() {
    let mut runtime = guarded_operation("while(true){}result=target.prototype;");
    let before = report(&runtime);
    let error = invoke_global(&mut runtime, "run").unwrap_err();
    assert!(error.contains("fuel exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(deltas(before, first), (0, CALL));
    assert!(first.first_rejected.is_none());
    interrupted(&runtime);
    latch(&mut runtime, &error, first);
}

#[test]
fn default_stack_recursion_keeps_sixty_four_calls_with_or_without_prototype_reads() {
    for (body, expected) in [
        ("return f();", 64 * CALL),
        ("f.prototype;return f();", 64 * READ + MATERIALIZE),
    ] {
        let mut runtime = Runtime::new();
        let function = runtime
            .execute(&format!("function f(){{{body}}}f;"), &mut NoIo)
            .unwrap();
        let before = report(&runtime);
        let error = invoke(&mut runtime, function, vec![]).unwrap_err();
        assert_eq!(error, "JavaScript call depth exhausted");
        let first = report(&runtime);
        assert_eq!(deltas(before, first), (0, expected));
        assert!(first.first_rejected.is_none());
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn argument_and_array_caps_remain_fatal_without_forcing_function_defaults() {
    let mut runtime = guarded_operation("result=target.prototype;");
    let before = report(&runtime);
    let function = runtime.get_global("run");
    let error = invoke(&mut runtime, function, vec![Value::Number(1.0); 10_001]).unwrap_err();
    assert!(error.contains("argument limit exhausted"), "{error}");
    assert_eq!(runtime.get_global("entered"), Value::Bool(false));
    assert_eq!(report(&runtime), before);
    latch(&mut runtime, &error, before);

    let mut runtime = guarded_operation("result=Array(10001);");
    let error = invoke_global(&mut runtime, "run").unwrap_err();
    assert!(error.contains("array limit exhausted"), "{error}");
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    interrupted(&runtime);
    latch(&mut runtime, &error, first);
}
