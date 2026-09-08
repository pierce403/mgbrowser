//! Independent admission checks for only the zero-length arguments snapshot.
//! Authored local programs, ordinary public ingress, unchanged realm limits and
//! default test stacks; no worker/page input or private allocation bypass.

use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;
const UNREAD_CALL: u64 = 128 + 128 + "arguments".len() as u64;
const MATERIALIZE: u64 = 128 + 128 + "callee".len() as u64;

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

fn runtime_only(before: AllocationReport, after: AllocationReport) -> u64 {
    assert_eq!(after.phases.bootstrap, before.phases.bootstrap);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.function_code, before.phases.function_code);
    assert_eq!(after.phases.regex_compile, before.phases.regex_compile);
    assert_eq!(after.phases.regex_result, before.phases.regex_result);
    let bytes = after.phases.runtime - before.phases.runtime;
    assert_eq!(after.accepted_bytes - before.accepted_bytes, bytes);
    bytes
}

fn function(runtime: &mut Runtime, parameters: &str, body: &str) -> Value {
    runtime
        .execute(&format!("function f({parameters}){{{body}}}f;"), &mut NoIo)
        .unwrap()
}

fn call(runtime: &mut Runtime, function: Value, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(function, Value::Undefined, args, &mut NoIo)
}

fn call_cost(parameters: &str, body: &str, args: Vec<Value>) -> (Value, u64) {
    let mut runtime = Runtime::new();
    let function = function(&mut runtime, parameters, body);
    let before = report(&runtime);
    let value = call(&mut runtime, function, args).unwrap();
    (value, runtime_only(before, report(&runtime)))
}

fn leave_budget(runtime: &mut Runtime, desired: u64) {
    // Existing public global-property ingress pays its real UTF-16 payload.
    // The empty property metadata is admitted before calculating the padding.
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
        call(runtime, Value::Native("Number".into()), vec![]).unwrap_err(),
        error
    );
    runtime.set_global("forbidden_ingress", Value::text("not admitted"));
    assert_eq!(runtime.get_global("forbidden_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn allocation_failure(runtime: &Runtime, error: &str, request: u64) -> AllocationReport {
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(runtime);
    let failure = first.first_rejected.unwrap();
    assert_eq!(failure.phase, AllocationPhase::Runtime);
    assert_eq!(failure.accepted_bytes, first.accepted_bytes);
    assert_eq!(failure.requested_bytes, request);
    assert!(request > LIMIT - first.accepted_bytes);
    first
}

#[test]
fn bootstrap_and_public_unread_calls_keep_only_real_environment_and_binding_storage() {
    let mut runtime = Runtime::new();
    let initial = report(&runtime);
    // Real Function.bind adds 145 bytes; Array.reduceRight adds 156.
    assert_eq!(initial.accepted_bytes, 25_854 + 145 + 156 + 725);
    assert_eq!(initial.phases.bootstrap, initial.accepted_bytes);
    let function = function(&mut runtime, "", "return 7;");
    let before = report(&runtime);
    for _ in 0..64 {
        assert_eq!(
            call(&mut runtime, function.clone(), vec![]).unwrap(),
            Value::Number(7.0)
        );
    }
    // The frozen pre-change observation is 64 * 527, not a changed cap.
    assert_eq!(runtime_only(before, report(&runtime)), 64 * UNREAD_CALL);
    assert_eq!(UNREAD_CALL, 265);
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn first_read_pays_existing_snapshot_once_and_repeated_reads_do_not_recreate_it() {
    let (snapshot, first) = call_cost("", "return arguments;", vec![]);
    assert!(matches!(snapshot, Value::Object(_)));
    assert_eq!(first, UNREAD_CALL + MATERIALIZE);
    let (same, repeated) = call_cost("", "return arguments===arguments;", vec![]);
    assert_eq!(same, Value::Bool(true));
    assert_eq!(repeated, first);
    assert_eq!(MATERIALIZE, 262);
}

#[test]
fn replacement_delete_and_uninitialized_var_do_not_force_an_unread_snapshot() {
    for (body, expected) in [
        ("arguments=9;return arguments;", Value::Number(9.0)),
        ("return delete arguments;", Value::Bool(false)),
        ("var arguments;return 7;", Value::Number(7.0)),
        ("var arguments=9;return arguments;", Value::Number(9.0)),
    ] {
        let (value, cost) = call_cost("", body, vec![]);
        assert_eq!(value, expected, "{body}");
        assert_eq!(cost, UNREAD_CALL, "{body}");
    }
    let (snapshot, cost) = call_cost("", "var arguments;return arguments;", vec![]);
    assert!(matches!(snapshot, Value::Object(_)));
    assert_eq!(cost, UNREAD_CALL + MATERIALIZE);
}

#[test]
fn typeof_is_an_actual_binding_read_and_retains_its_real_output_payload_charge() {
    let (value, cost) = call_cost("", "return typeof arguments;", vec![]);
    assert_eq!(value, Value::text("object"));
    assert_eq!(cost, UNREAD_CALL + MATERIALIZE + 2 * "object".len() as u64);
}

#[test]
fn formal_arguments_control_and_nonempty_numeric_snapshots_keep_existing_costs() {
    let (value, cost) = call_cost("arguments", "return arguments;", vec![]);
    assert_eq!(value, Value::Undefined);
    assert_eq!(cost, UNREAD_CALL);
    let (value, cost) = call_cost("arguments", "return arguments;", vec![Value::Number(19.0)]);
    assert_eq!(value, Value::Number(19.0));
    assert_eq!(cost, UNREAD_CALL);
    for length in [1, 2, 257, 10_000] {
        let (value, cost) = call_cost("", "return 7;", vec![Value::Number(3.0); length]);
        assert_eq!(value, Value::Number(7.0));
        assert_eq!(cost, 527 + 64 * length as u64, "{length}");
    }
}

#[test]
fn nonempty_payload_ingress_and_real_parameter_copies_are_not_deferred() {
    for (parameters, bytes_per_unit) in [("", 2), ("value", 4), ("arguments", 4)] {
        let (_, short) = call_cost(
            parameters,
            "return 7;",
            vec![Value::String(vec![0xd800; 512])],
        );
        let (_, long) = call_cost(
            parameters,
            "return 7;",
            vec![Value::String(vec![0xd800; 1024])],
        );
        assert_eq!(long - short, 512 * bytes_per_unit, "{parameters}");
    }
}

fn guarded_reader(runtime: &mut Runtime) -> Value {
    runtime
        .execute(
            "var entered=false,returned=false,caught=false,finalized=false,result=17;function read(){entered=true;try{result=arguments;returned=true;}catch(e){caught=true;}finally{finalized=true;}}read;",
            &mut NoIo,
        )
        .unwrap()
}

fn interrupted_read(runtime: &Runtime) {
    assert_eq!(runtime.get_global("entered"), Value::Bool(true));
    assert_eq!(runtime.get_global("result"), Value::Number(17.0));
    for name in ["returned", "caught", "finalized"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
}

#[test]
fn first_read_object_and_callee_admission_fail_at_the_actual_read_and_latch() {
    for (remaining, request, accepted) in [(390, 128, 265), (400, 134, 393)] {
        let mut runtime = Runtime::new();
        let read = guarded_reader(&mut runtime);
        leave_budget(&mut runtime, remaining);
        let before = report(&runtime);
        let error = call(&mut runtime, read, vec![]).unwrap_err();
        let first = allocation_failure(&runtime, &error, request);
        assert_eq!(runtime_only(before, first), accepted);
        interrupted_read(&runtime);
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn unread_and_replaced_calls_fit_when_the_snapshot_itself_cannot_be_admitted() {
    for body in ["return 7;", "arguments=7;return arguments;"] {
        let mut runtime = Runtime::new();
        let function = function(&mut runtime, "", body);
        leave_budget(&mut runtime, 300);
        let before = report(&runtime);
        assert_eq!(
            call(&mut runtime, function, vec![]).unwrap(),
            Value::Number(7.0)
        );
        assert_eq!(runtime_only(before, report(&runtime)), UNREAD_CALL);
        assert!(report(&runtime).first_rejected.is_none());
    }
}

#[test]
fn local_binding_metadata_is_still_preflighted_before_the_body() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("var entered=false;function f(){entered=true;}f;", &mut NoIo)
        .unwrap();
    leave_budget(&mut runtime, 260);
    let before = report(&runtime);
    let error = call(&mut runtime, function, vec![]).unwrap_err();
    let first = allocation_failure(&runtime, &error, 137);
    assert_eq!(runtime_only(before, first), 128);
    assert_eq!(runtime.get_global("entered"), Value::Bool(false));
    latch(&mut runtime, &error, first);
}

#[test]
fn eight_thousand_unread_empty_calls_fit_but_observed_snapshots_still_exhaust_heap() {
    let mut runtime = Runtime::new();
    let unread = function(&mut runtime, "", "return 7;");
    let before = report(&runtime);
    for _ in 0..8000 {
        assert_eq!(
            call(&mut runtime, unread.clone(), vec![]).unwrap(),
            Value::Number(7.0)
        );
    }
    assert_eq!(runtime_only(before, report(&runtime)), 8000 * UNREAD_CALL);
    assert!(report(&runtime).first_rejected.is_none());

    let mut runtime = Runtime::new();
    let read = function(&mut runtime, "", "return arguments;");
    let mut completed = 0;
    let error = loop {
        match call(&mut runtime, read.clone(), vec![]) {
            Ok(Value::Object(_)) => completed += 1,
            Ok(value) => panic!("unexpected snapshot: {value:?}"),
            Err(error) => break error,
        }
        assert!(
            completed < 8000,
            "observed snapshots must still pay full storage"
        );
    };
    assert!(completed > 7000);
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(
        first.first_rejected.unwrap().phase,
        AllocationPhase::Runtime
    );
    latch(&mut runtime, &error, first);
}

fn object_cap_realm() -> (Runtime, Value, Value) {
    let mut runtime = Runtime::new();
    let read = guarded_reader(&mut runtime);
    let unread = function(&mut runtime, "", "return 7;");
    (runtime, read, unread)
}

fn empty_array(runtime: &mut Runtime) -> Result<Value, String> {
    call(runtime, Value::Native("Array".into()), vec![])
}

#[test]
fn object_cap_applies_on_first_read_not_on_an_unread_empty_call() {
    // Calibrate only in a disposable, identically initialized realm. The realm
    // under test never catches or resets a prior fatal resource failure.
    let (mut calibration, _, _) = object_cap_realm();
    let mut available = 0;
    let error = loop {
        match empty_array(&mut calibration) {
            Ok(Value::Object(_)) => available += 1,
            Ok(value) => panic!("unexpected array: {value:?}"),
            Err(error) => break error,
        }
        assert!(available <= 10_000);
    };
    assert!(error.contains("object limit exhausted"), "{error}");
    assert!((9900..10_000).contains(&available));
    assert!(report(&calibration).first_rejected.is_none());

    let (mut runtime, read, unread) = object_cap_realm();
    for _ in 0..available {
        empty_array(&mut runtime).unwrap();
    }
    let before = report(&runtime);
    assert_eq!(
        call(&mut runtime, unread, vec![]).unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(runtime_only(before, report(&runtime)), UNREAD_CALL);
    let before = report(&runtime);
    let error = call(&mut runtime, read, vec![]).unwrap_err();
    assert!(error.contains("object limit exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(runtime_only(before, first), UNREAD_CALL);
    assert!(first.first_rejected.is_none());
    interrupted_read(&runtime);
    latch(&mut runtime, &error, first);
}

#[test]
fn fuel_failure_before_first_read_never_charges_a_nonexistent_snapshot() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute(
            "var entered=false,result=17,caught=false,finalized=false;function f(){entered=true;try{while(true){}result=arguments;}catch(e){caught=true;}finally{finalized=true;}}f;",
            &mut NoIo,
        )
        .unwrap();
    let before = report(&runtime);
    let error = call(&mut runtime, function, vec![]).unwrap_err();
    assert!(error.contains("fuel exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(runtime_only(before, first), UNREAD_CALL);
    assert!(first.first_rejected.is_none());
    assert_eq!(runtime.get_global("entered"), Value::Bool(true));
    assert_eq!(runtime.get_global("result"), Value::Number(17.0));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    latch(&mut runtime, &error, first);
}

#[test]
fn direct_recursion_preserves_the_existing_sixty_four_call_limit_on_default_stack() {
    for (body, per_call) in [
        ("return f();", UNREAD_CALL),
        ("arguments;return f();", UNREAD_CALL + MATERIALIZE),
    ] {
        let mut runtime = Runtime::new();
        let function = function(&mut runtime, "", body);
        let before = report(&runtime);
        let error = call(&mut runtime, function, vec![]).unwrap_err();
        assert_eq!(error, "JavaScript call depth exhausted");
        let first = report(&runtime);
        assert_eq!(runtime_only(before, first), 64 * per_call, "{body}");
        assert!(first.first_rejected.is_none());
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn nonempty_argument_length_cap_still_fails_before_body_or_snapshot_admission() {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("var entered=false;function f(){entered=true;}f;", &mut NoIo)
        .unwrap();
    let before = report(&runtime);
    let error = call(&mut runtime, function, vec![Value::Number(1.0); 10_001]).unwrap_err();
    assert!(error.contains("argument limit exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(first, before);
    assert_eq!(runtime.get_global("entered"), Value::Bool(false));
    latch(&mut runtime, &error, first);
}
