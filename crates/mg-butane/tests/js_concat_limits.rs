//! Authored concat resource/ownership checks. No page input, alternate engine,
//! unsafe allocation probe, increased stack or relaxed realm limit is used.

use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;

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

fn invoke(
    runtime: &mut Runtime,
    name: &str,
    this: Value,
    args: Vec<Value>,
) -> Result<Value, String> {
    runtime.invoke(Value::Native(name.into()), this, args, &mut NoIo)
}

fn holes(runtime: &mut Runtime, length: usize) -> Value {
    invoke(
        runtime,
        "Array",
        Value::Undefined,
        vec![Value::Number(length as f64)],
    )
    .unwrap()
}

fn concat(runtime: &mut Runtime, this: Value, args: Vec<Value>) -> Result<Value, String> {
    invoke(runtime, "Array.concat", this, args)
}

fn leave_budget(runtime: &mut Runtime, desired: u64) {
    // Pre-create a normal global property, then use its ordinary charged public
    // ingress to consume the budget. No private budget access or paid-value flag.
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
        invoke(runtime, "Number", Value::Undefined, vec![]).unwrap_err(),
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
    assert!(request > LIMIT - first.accepted_bytes);
    first
}

fn capacity(length: usize) -> usize {
    if length == 0 {
        0
    } else {
        length.next_power_of_two().min(10_000)
    }
}

fn scalar_slot_cost(length: usize) -> u64 {
    let mut runtime = Runtime::new();
    let source = holes(&mut runtime, 0);
    let before = report(&runtime);
    let result = concat(&mut runtime, source, vec![Value::Number(7.0); length]).unwrap();
    let cost = runtime_only(before, report(&runtime));
    runtime.set_global("result", result);
    assert_eq!(
        runtime.execute("result.length;", &mut NoIo).unwrap(),
        Value::Number(length as f64)
    );
    cost
}

#[test]
fn numeric_operands_prepay_geometric_slots_and_exactly_one_result_metadata() {
    let empty = scalar_slot_cost(0);
    // The direct public native name is genuinely incoming UTF-8 storage.
    assert_eq!(empty, 128 + "Array.concat".len() as u64);
    for length in [1, 2, 3, 4, 5, 257, 10_000] {
        assert_eq!(
            scalar_slot_cost(length) - empty,
            (capacity(length) * 64) as u64,
            "{length}"
        );
    }
}

fn scanned_slot_cost(length: usize, dense: bool) -> u64 {
    let mut runtime = Runtime::new();
    let source = if dense {
        // Avoid the single-numeric-argument length constructor case.
        assert!(length > 1);
        invoke(
            &mut runtime,
            "Array",
            Value::Undefined,
            vec![Value::Number(7.0); length],
        )
        .unwrap()
    } else {
        holes(&mut runtime, length)
    };
    let before = report(&runtime);
    let result = concat(&mut runtime, source, vec![]).unwrap();
    let cost = runtime_only(before, report(&runtime));
    runtime.set_global("result", result);
    assert_eq!(
        runtime.execute("result.length;", &mut NoIo).unwrap(),
        Value::Number(length as f64)
    );
    assert_eq!(
        runtime
            .execute("(0 in result)&&(result.length-1 in result);", &mut NoIo)
            .unwrap(),
        Value::Bool(dense)
    );
    cost
}

#[test]
fn holes_and_dense_numeric_sources_pay_the_same_capacity_without_payload_clones() {
    for length in [3, 4, 5, 257, 10_000] {
        let expected = 128 + "Array.concat".len() as u64 + (capacity(length) * 64) as u64;
        assert_eq!(scanned_slot_cost(length, false), expected, "holes {length}");
        assert_eq!(scanned_slot_cost(length, true), expected, "dense {length}");
    }
}

fn opaque_argument_cost(length: usize, kind: &str) -> u64 {
    let mut runtime = Runtime::new();
    let source = holes(&mut runtime, 0);
    let value = match kind {
        "string" => Value::String(vec![0xd800; length]),
        "native" => Value::Native("x".repeat(length)),
        "host" => Value::Host("x".repeat(length)),
        _ => unreachable!(),
    };
    let before = report(&runtime);
    let result = concat(&mut runtime, source, vec![value]).unwrap();
    assert!(matches!(result, Value::Object(_)));
    runtime_only(before, report(&runtime))
}

#[test]
fn owned_nonarray_arguments_move_after_real_public_ingress_without_extra_copies() {
    for (kind, bytes) in [("string", 2), ("native", 1), ("host", 1)] {
        assert_eq!(
            opaque_argument_cost(1024, kind) - opaque_argument_cost(512, kind),
            512 * bytes,
            "{kind}"
        );
    }
    let mut runtime = Runtime::new();
    let source = holes(&mut runtime, 0);
    let result = concat(
        &mut runtime,
        source,
        vec![Value::String(vec![0xdfff; 1_500_000])],
    )
    .unwrap();
    runtime.set_global("result", result);
    // Check the container length without reading its element: result[0] would
    // still make a charged copy of the large string before any length lookup.
    assert_eq!(
        runtime.execute("result.length===1;", &mut NoIo).unwrap(),
        Value::Bool(true)
    );
    assert!(report(&runtime).first_rejected.is_none());
}

fn boxed_receiver_cost(length: usize) -> u64 {
    let mut runtime = Runtime::new();
    let before = report(&runtime);
    let result = concat(&mut runtime, Value::String(vec![0xd800; length]), vec![]).unwrap();
    assert!(matches!(result, Value::Object(_)));
    runtime_only(before, report(&runtime))
}

#[test]
fn primitive_receiver_boxing_keeps_its_existing_transfer_charge() {
    assert_eq!(
        boxed_receiver_cost(1024) - boxed_receiver_cost(512),
        512 * 4
    );
    assert!(boxed_receiver_cost(900_000) >= 3_600_000);
}

fn element_copy_cost(length: usize, inherited: bool) -> u64 {
    let mut runtime = Runtime::new();
    let source = if inherited {
        runtime.set_global("payload", Value::String(vec![0xd800; length]));
        runtime
            .execute("Array.prototype[0]=payload;", &mut NoIo)
            .unwrap();
        holes(&mut runtime, 1)
    } else {
        invoke(
            &mut runtime,
            "Array",
            Value::Undefined,
            vec![Value::String(vec![0xd800; length])],
        )
        .unwrap()
    };
    let before = report(&runtime);
    let result = concat(&mut runtime, source, vec![]).unwrap();
    assert!(matches!(result, Value::Object(_)));
    runtime_only(before, report(&runtime))
}

#[test]
fn own_and_inherited_array_element_gets_still_pay_genuine_payload_copies() {
    for inherited in [false, true] {
        assert_eq!(
            element_copy_cost(1024, inherited) - element_copy_cost(512, inherited),
            512 * 2,
            "inherited={inherited}"
        );
    }
}

fn arguments_concat_cost(length: usize) -> u64 {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("function save(){return arguments;}save;", &mut NoIo)
        .unwrap();
    let arguments = runtime
        .invoke(
            function,
            Value::Undefined,
            vec![Value::String(vec![0xd800; length])],
            &mut NoIo,
        )
        .unwrap();
    let before = report(&runtime);
    let result = concat(&mut runtime, arguments.clone(), vec![]).unwrap();
    let cost = runtime_only(before, report(&runtime));
    runtime.set_global("saved", arguments);
    runtime.set_global("result", result);
    assert_eq!(runtime.execute("result.length===1&&result[0]===saved&&!Array.isArray(saved)&&Object.getPrototypeOf(saved)===Object.prototype;", &mut NoIo).unwrap(), Value::Bool(true));
    cost
}

#[test]
fn arguments_keep_indexed_storage_but_concat_neither_spreads_nor_clones_it() {
    assert_eq!(arguments_concat_cost(1024), arguments_concat_cost(512));
    assert_eq!(
        arguments_concat_cost(500_000),
        128 + 64 + "Array.concat".len() as u64
    );
}

#[test]
fn result_metadata_fails_before_result_slots_are_allocated() {
    let mut runtime = Runtime::new();
    let source = holes(&mut runtime, 0);
    leave_budget(&mut runtime, 64);
    let error = concat(&mut runtime, source, vec![]).unwrap_err();
    let first = allocation_failure(&runtime, &error, 128);
    latch(&mut runtime, &error, first);
}

#[test]
fn first_slot_admission_precedes_a_possible_large_element_get() {
    let mut runtime = Runtime::new();
    let source = invoke(
        &mut runtime,
        "Array",
        Value::Undefined,
        vec![Value::String(vec![0xd800; 8192])],
    )
    .unwrap();
    leave_budget(&mut runtime, 128 + 48);
    let before = report(&runtime);
    let error = concat(&mut runtime, source, vec![]).unwrap_err();
    let first = allocation_failure(&runtime, &error, 64);
    assert_eq!(
        runtime_only(before, first),
        128 + "Array.concat".len() as u64
    );
    latch(&mut runtime, &error, first);
}

#[test]
fn geometric_growth_rejects_before_the_next_large_element_get() {
    let mut runtime = Runtime::new();
    let source = invoke(
        &mut runtime,
        "Array",
        Value::Undefined,
        vec![
            Value::Number(1.0),
            Value::Number(2.0),
            Value::String(vec![0xd800; 8192]),
        ],
    )
    .unwrap();
    leave_budget(&mut runtime, 128 + 128 + 64);
    let before = report(&runtime);
    let error = concat(&mut runtime, source, vec![]).unwrap_err();
    let first = allocation_failure(&runtime, &error, 128);
    assert_eq!(
        runtime_only(before, first),
        128 + 2 * 64 + "Array.concat".len() as u64
    );
    latch(&mut runtime, &error, first);
}

#[test]
fn element_copy_can_still_fail_after_metadata_and_slot_admission() {
    let mut runtime = Runtime::new();
    let source = invoke(
        &mut runtime,
        "Array",
        Value::Undefined,
        vec![Value::String(vec![0xd800; 8192])],
    )
    .unwrap();
    leave_budget(&mut runtime, 8192);
    let before = report(&runtime);
    let error = concat(&mut runtime, source, vec![]).unwrap_err();
    let first = allocation_failure(&runtime, &error, 16384);
    assert_eq!(
        runtime_only(before, first),
        128 + 64 + "Array.concat".len() as u64
    );
    latch(&mut runtime, &error, first);
}

#[test]
fn array_and_argument_caps_are_fatal_without_a_partial_published_result() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.execute("var source=Array(9999);var good=source.concat(7);good.length===10000&&!(9998 in good)&&good[9999]===7;", &mut NoIo).unwrap(), Value::Bool(true));
    let mut runtime = Runtime::new();
    let error = runtime.execute("var source=Array(10000),marker={},result=marker,caught=false,finalized=false,after=false;try{result=source.concat(7);after=true;}catch(e){caught=true;}finally{finalized=true;}", &mut NoIo).unwrap_err();
    assert!(error.contains("array limit exhausted"), "{error}");
    assert_eq!(runtime.get_global("result"), runtime.get_global("marker"));
    for name in ["caught", "finalized", "after"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false));
    }
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);

    let mut runtime = Runtime::new();
    let source = holes(&mut runtime, 0);
    let error = concat(&mut runtime, source, vec![Value::Number(0.0); 10_001]).unwrap_err();
    assert!(error.contains("argument limit exhausted"), "{error}");
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn repeated_results_exhaust_the_same_cumulative_budget_without_partial_assignment() {
    let mut runtime = Runtime::new();
    let runner = runtime.execute("var source=Array(1000),result=null,last=null,done=0,caught=false,finalized=false;function run(){try{for(var i=0;i<4;i++){last=result;result=source.concat();done++;}}catch(e){caught=true;}finally{finalized=true;}}run;", &mut NoIo).unwrap();
    // The original repeated 10k-hole draft correctly reached fuel first. Use
    // ordinary public ingress after parsing to isolate cumulative allocation:
    // three 1024-credit outputs fit, and the fourth cannot finish.
    leave_budget(&mut runtime, 200_000);
    let error = runtime
        .invoke(runner, Value::Undefined, vec![], &mut NoIo)
        .unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    assert_eq!(runtime.get_global("done"), Value::Number(3.0));
    assert_eq!(runtime.get_global("result"), runtime.get_global("last"));
    assert!(matches!(runtime.get_global("result"), Value::Object(_)));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes % 64, 0);
    latch(&mut runtime, &error, first);
}

#[test]
fn inherited_property_scans_consume_fuel_before_the_allocation_budget() {
    let mut runtime = Runtime::new();
    let error = runtime.execute("var source=Array(3000),ready=false,caught=false,finalized=false,after=false,result=null;for(var i=0;i<400;i++)Array.prototype['marker'+i]=i;ready=true;try{result=source.concat();after=true;}catch(e){caught=true;}finally{finalized=true;}", &mut NoIo).unwrap_err();
    assert!(error.contains("fuel exhausted"), "{error}");
    assert_eq!(runtime.get_global("ready"), Value::Bool(true));
    assert_eq!(runtime.get_global("result"), Value::Null);
    for name in ["caught", "finalized", "after"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false));
    }
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn nonarray_receivers_are_opaque_even_with_a_deep_prototype_chain() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.execute("var value={length:1000000000};for(var i=0;i<70;i++)value=Object.create(value);var result=Array.prototype.concat.call(value);result.length===1&&result[0]===value;", &mut NoIo).unwrap(), Value::Bool(true));
    let receiver = Value::Host("opaque receiver".into());
    let result = concat(
        &mut runtime,
        receiver,
        vec![Value::Host("opaque argument".into())],
    )
    .unwrap();
    runtime.set_global("result", result);
    assert_eq!(
        runtime.execute("result.length===2;", &mut NoIo).unwrap(),
        Value::Bool(true)
    );
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn empty_concat_results_still_consume_the_existing_object_cap() {
    let mut runtime = Runtime::new();
    let source = holes(&mut runtime, 0);
    let mut count = 0;
    let error = loop {
        assert!(count < 10_000, "object cap was not enforced");
        match concat(&mut runtime, source.clone(), vec![]) {
            Ok(value) => {
                assert!(matches!(value, Value::Object(_)));
                count += 1;
            }
            Err(error) => break error,
        }
    };
    assert!(
        (9900..10_000).contains(&count),
        "early failure after {count}: {error}"
    );
    assert!(error.contains("object limit exhausted"), "{error}");
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}
