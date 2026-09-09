//! Independent tests for the narrow copied-parameter binding boundary. Inputs
//! are authored here; no page source, alternate engine or enlarged stack is used.

use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("Unexpected host read: {object}.{key}");
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("Unexpected host write: {object}.{key}");
    }
    fn call(&mut self, name: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("Unexpected host call: {name}");
    }
}

fn report(runtime: &Runtime) -> AllocationReport {
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, LIMIT);
    assert!(report.accepted_bytes <= LIMIT);
    report
}

fn latch(runtime: &mut Runtime, first: AllocationReport, error: &str) {
    assert_eq!(
        runtime
            .execute("var later_effect=1;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(runtime.get_global("later_effect"), Value::Undefined);
    assert_eq!(report(runtime), first);
    assert_eq!(
        runtime
            .invoke(
                Value::Native("Number".into()),
                Value::Undefined,
                vec![],
                &mut NoIo
            )
            .unwrap_err(),
        error
    );
    runtime.set_global("later_ingress", Value::text("not admitted"));
    assert_eq!(runtime.get_global("later_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn allocation_failure(runtime: &Runtime, error: &str, request: u64) -> AllocationReport {
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.accepted_bytes, first.accepted_bytes);
    assert_eq!(rejected.requested_bytes, request);
    assert!(rejected.accepted_bytes + rejected.requested_bytes > LIMIT);
    first
}

#[test]
fn nine_hundred_thousand_utf16_units_fit_a_real_formal_parameter() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("function take(value){return arguments;}take;", &mut NoIo)
        .unwrap();
    let result = runtime.invoke(
        function,
        Value::Undefined,
        vec![Value::String(vec![0xd800; 900_000])],
        &mut NoIo,
    );
    let allocation = report(&runtime);
    eprintln!("Authored 900k formal observation: {allocation:?}");
    let snapshot = result.unwrap();
    assert!(matches!(snapshot, Value::Object(_)));
    assert!(allocation.phases.runtime >= 3_600_000);
    assert!(allocation.first_rejected.is_none());
    runtime.set_global("snapshot", snapshot);
    // Looking at the container does not clone its large contained string.
    assert_eq!(
        runtime.execute("snapshot.length===1;", &mut NoIo).unwrap(),
        Value::Bool(true)
    );
    report(&runtime);
}

fn formal_cost(length: usize, source: &str, count: usize) -> u64 {
    let mut runtime = Runtime::new();
    let function = runtime.execute(source, &mut NoIo).unwrap();
    let before = report(&runtime);
    assert_eq!(
        runtime
            .invoke(
                function,
                Value::Undefined,
                (0..count)
                    .map(|_| Value::String(vec![0xdfff; length]))
                    .collect(),
                &mut NoIo,
            )
            .unwrap(),
        Value::Number(7.0)
    );
    let after = report(&runtime);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.function_code, before.phases.function_code);
    after.phases.runtime - before.phases.runtime
}

#[test]
fn duplicate_and_arguments_named_parameters_keep_actual_copy_costs() {
    for (source, count, bytes_per_unit) in [
        ("function take(value,value){return 7;}take;", 2, 8),
        ("function take(value,value){return 7;}take;", 1, 4),
        ("function take(arguments){return 7;}take;", 1, 4),
        ("function take(value,missing){return 7;}take;", 1, 4),
    ] {
        assert_eq!(
            formal_cost(1024, source, count) - formal_cost(512, source, count),
            512 * bytes_per_unit,
            "{source}"
        );
    }
}

#[test]
fn parameters_keep_identity_scope_and_utf16_after_the_call_returns() {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime.execute(
            "var object={value:1};function keep(text,item){var saved=arguments;text+='!';item.value=2;return function(){return text==='\\uD800😀!'&&saved[0]==='\\uD800😀'&&text.length===4&&saved[0].length===3&&item===object&&saved[1]===object&&item.value===2;};}var first=keep('\\uD800😀',object),second=keep('\\uD800😀',object);first!==second;",
            &mut NoIo,
        ).unwrap(),
        Value::Bool(true)
    );
    // The parse and originating call have ended; each closure still owns its
    // environment while object arguments preserve reference identity.
    assert_eq!(
        runtime.execute("first()&&second();", &mut NoIo).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        runtime.execute(
            "function duplicate(value,value){return typeof value==='undefined'&&arguments[0]==='kept'&&arguments.length===1;}function shadow(arguments,missing){return arguments==='\\uDFFF'&&typeof missing==='undefined';}duplicate('kept')&&shadow('\\uDFFF');",
            &mut NoIo,
        ).unwrap(),
        Value::Bool(true)
    );
    report(&runtime);
}

#[test]
fn failed_real_parameter_copy_prevents_body_effects_and_latches() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute(
            "var entered=false;function take(value){entered=true;return 1;}take;",
            &mut NoIo,
        )
        .unwrap();
    let error = runtime
        .invoke(
            function,
            Value::Undefined,
            vec![Value::String(vec![0xd800; 1_100_000])],
            &mut NoIo,
        )
        .unwrap_err();
    assert_eq!(runtime.get_global("entered"), Value::Bool(false));
    let first = allocation_failure(&runtime, &error, 2_200_000);
    // Public ingress was admitted; the independent formal copy was not.
    assert!(first.phases.runtime >= 2_200_000);
    assert!(first.phases.runtime < 2_300_000);
    latch(&mut runtime, first, &error);
}

#[test]
fn reading_an_admitted_parameter_still_copies_and_can_fail_fatally() {
    let mut runtime = Runtime::new();
    let function = runtime.execute("var entered=false,copied=false,caught=false,finalized=false;function take(value){entered=true;try{var actualCopy=value;copied=true;}catch(e){caught=true;}finally{finalized=true;}}take;", &mut NoIo).unwrap();
    let error = runtime
        .invoke(
            function,
            Value::Undefined,
            vec![Value::String(vec![0xd800; 900_000])],
            &mut NoIo,
        )
        .unwrap_err();
    assert_eq!(runtime.get_global("entered"), Value::Bool(true));
    for name in ["copied", "caught", "finalized"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    let first = allocation_failure(&runtime, &error, 1_800_000);
    assert!(first.phases.runtime >= 3_600_000);
    latch(&mut runtime, first, &error);
}

#[test]
fn repeated_calls_keep_cumulative_parameter_copy_charges() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute(
            "var calls=0;function take(value){calls++;return arguments;}take;",
            &mut NoIo,
        )
        .unwrap();
    let snapshot = runtime
        .invoke(
            function.clone(),
            Value::Undefined,
            vec![Value::String(vec![0xdc00; 600_000])],
            &mut NoIo,
        )
        .unwrap();
    let first_call = report(&runtime);
    assert!(first_call.first_rejected.is_none());
    assert!(matches!(snapshot, Value::Object(_)));
    let error = runtime
        .invoke(
            function,
            Value::Undefined,
            vec![Value::String(vec![0xdc00; 600_000])],
            &mut NoIo,
        )
        .unwrap_err();
    assert_eq!(runtime.get_global("calls"), Value::Number(1.0));
    let first = allocation_failure(&runtime, &error, 1_200_000);
    assert!(first.accepted_bytes > first_call.accepted_bytes);
    latch(&mut runtime, first, &error);
}

fn global_cost(length: usize) -> (u64, u64, u64) {
    let mut runtime = Runtime::new();
    runtime.set_global("held", Value::Undefined);
    let before = report(&runtime);
    runtime.set_global("held", Value::String(vec![0xdfff; length]));
    let ingress = report(&runtime);
    assert_eq!(
        runtime.execute("held.length;", &mut NoIo).unwrap(),
        Value::Number(length as f64)
    );
    let read = report(&runtime);
    let function = runtime
        .execute("function copy(){var local=held;return 7;}copy;", &mut NoIo)
        .unwrap();
    let before_local = report(&runtime);
    assert_eq!(
        runtime
            .invoke(function, Value::Undefined, vec![], &mut NoIo)
            .unwrap(),
        Value::Number(7.0)
    );
    let after_local = report(&runtime);
    (
        ingress.phases.runtime - before.phases.runtime,
        read.phases.runtime - ingress.phases.runtime,
        after_local.phases.runtime - before_local.phases.runtime,
    )
}

#[test]
fn global_ingress_reads_and_ordinary_local_binding_charges_are_unchanged() {
    let small = global_cost(512);
    let large = global_cost(1024);
    assert_eq!(large.0 - small.0, 512 * 2);
    assert_eq!(large.1 - small.1, 512 * 2);
    // The special parameter boundary must not exempt generic local assignment:
    // the global read makes a real copy, then ordinary binding admission pays.
    assert_eq!(large.2 - small.2, 512 * 4);
}

#[test]
fn oversized_global_ingress_is_rejected_without_replacing_the_old_value() {
    let mut runtime = Runtime::new();
    runtime.set_global("held", Value::Number(7.0));
    runtime.set_global("held", Value::String(vec![0xd800; 2_100_000]));
    assert_eq!(runtime.get_global("held"), Value::Number(7.0));
    let error = runtime
        .execute("var later_effect=1;", &mut NoIo)
        .unwrap_err();
    let first = allocation_failure(&runtime, &error, 4_200_000);
    latch(&mut runtime, first, &error);
}

struct HostError {
    units: usize,
    calls: usize,
}
impl Host for HostError {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("Unexpected host read: {object}.{key}");
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("Unexpected host write: {object}.{key}");
    }
    fn call(&mut self, name: &str, _: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.fail");
        assert!(args.is_empty());
        self.calls += 1;
        Err("x".repeat(self.units))
    }
}

fn caught_host_error_cost(length: usize) -> u64 {
    let mut runtime = Runtime::new();
    runtime.set_global("fail", Value::Native("host.fail".into()));
    let before = report(&runtime);
    let mut host = HostError {
        units: length,
        calls: 0,
    };
    assert_eq!(
        runtime
            .execute(
                "var caught=false;try{fail();}catch(e){caught=true;}caught;",
                &mut host
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(host.calls, 1);
    let after = report(&runtime);
    assert!(after.first_rejected.is_none());
    after.phases.runtime - before.phases.runtime
}

#[test]
fn caught_host_errors_keep_their_generic_binding_payload_admission() {
    // Host exceptions are newly created UTF-16 strings, not copied/admitted
    // parameter values. Do not read e here: that would add another real copy.
    assert_eq!(
        caught_host_error_cost(1024) - caught_host_error_cost(512),
        512 * 2
    );
}

#[test]
fn oversized_host_error_cannot_enter_catch_or_finally_and_latches() {
    let mut runtime = Runtime::new();
    runtime.set_global("fail", Value::Native("host.fail".into()));
    let mut host = HostError {
        units: 2_100_000,
        calls: 0,
    };
    let error = runtime.execute("var caught=false,finalized=false;try{fail();}catch(e){caught=true;}finally{finalized=true;}", &mut host).unwrap_err();
    assert_eq!(host.calls, 1);
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    let first = allocation_failure(&runtime, &error, 4_200_000);
    latch(&mut runtime, first, &error);
}
