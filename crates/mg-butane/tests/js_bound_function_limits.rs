//! Independent bound-function admission tests. Authored sources, public Runtime
//! APIs and ordinary paid ingress only; no page input or increased realm/stack cap.

use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;
const RECORD: u64 = 160;
const SLOT: u64 = 64;
const BIND_NAME: u64 = "Function.bind".len() as u64;

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

#[derive(Default)]
struct ProbeHost {
    calls: usize,
    fail_once: bool,
    receiver: Option<Value>,
    arguments: Vec<Value>,
}
impl Host for ProbeHost {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        panic!("binding/forwarding must not ask Host for metadata");
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host write");
    }
    fn call(&mut self, name: &str, receiver: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.probe");
        self.calls += 1;
        self.receiver = Some(receiver);
        self.arguments = args;
        if std::mem::take(&mut self.fail_once) {
            Err("authored ordinary Host error".into())
        } else {
            Ok(Value::Number(self.arguments.len() as f64))
        }
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

fn target(runtime: &mut Runtime) -> Value {
    runtime
        .execute("(function(){return 7;});", &mut NoIo)
        .unwrap()
}

fn bind_args(runtime: &mut Runtime, target: Value, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(
        Value::Native("Function.bind".into()),
        target,
        args,
        &mut NoIo,
    )
}

fn bind(runtime: &mut Runtime, target: Value, receiver: Value, prefix: Vec<Value>) -> Value {
    let mut args = Vec::with_capacity(prefix.len() + 1);
    args.push(receiver);
    args.extend(prefix);
    bind_args(runtime, target, args).unwrap()
}

fn call(runtime: &mut Runtime, function: Value, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(function, Value::Undefined, args, &mut NoIo)
}

fn chain(runtime: &mut Runtime, mut function: Value, count: usize) -> Value {
    for _ in 0..count {
        function = bind(runtime, function, Value::Null, vec![]);
    }
    function
}

fn leave_budget(runtime: &mut Runtime, desired: u64) {
    runtime.set_global("padding", Value::String(Vec::new()));
    let remaining = LIMIT - report(runtime).accepted_bytes;
    assert!(remaining > desired + 2);
    runtime.set_global(
        "padding",
        Value::String(vec![b'p' as u16; ((remaining - desired) / 2) as usize]),
    );
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
    assert_eq!(rejected.requested_bytes, request);
    assert_eq!(rejected.accepted_bytes, first.accepted_bytes);
    assert!(request > LIMIT - first.accepted_bytes);
    first
}

#[test]
fn bootstrap_and_bound_metadata_slots_are_admitted_without_synthetic_code() {
    for count in [0, 1, 2, 257, 9999] {
        let mut runtime = Runtime::new();
        let initial = report(&runtime);
        assert_eq!(initial.phases.bootstrap, 25_854 + 145 + 156 + 725); // reduceRight metadata
        assert_eq!(initial.accepted_bytes, initial.phases.bootstrap);
        let target = target(&mut runtime);
        let before = report(&runtime);
        let bound = bind(
            &mut runtime,
            target,
            Value::Null,
            vec![Value::Number(7.0); count],
        );
        assert!(matches!(bound, Value::Function(_)));
        assert_eq!(
            deltas(before, report(&runtime)),
            (RECORD, 128 + BIND_NAME + SLOT * count as u64)
        );
    }
}

#[test]
fn public_spare_vector_capacity_does_not_change_logical_retained_slot_count() {
    for capacity in [2, 10_000] {
        let mut runtime = Runtime::new();
        let target = target(&mut runtime);
        let mut args = Vec::with_capacity(capacity);
        args.push(Value::Null);
        args.push(Value::Number(7.0));
        let before = report(&runtime);
        let bound = bind_args(&mut runtime, target, args).unwrap();
        assert_eq!(
            deltas(before, report(&runtime)),
            (RECORD, 128 + BIND_NAME + SLOT)
        );
        assert_eq!(
            call(&mut runtime, bound, vec![]).unwrap(),
            Value::Number(7.0)
        );
    }
    // This public cost check complements the private exact-slice/pointer test;
    // it is not by itself an RSS or buffer-retention measurement.
}

fn payload_cost(length: usize, receiver: bool) -> (u64, u64) {
    let mut runtime = Runtime::new();
    let target = target(&mut runtime);
    let value = Value::String(vec![0xd800; length]);
    let (this, prefix) = if receiver {
        (value, vec![])
    } else {
        (Value::Null, vec![value])
    };
    let before = report(&runtime);
    let bound = bind(&mut runtime, target, this, prefix);
    let creation = deltas(before, report(&runtime));
    assert_eq!(creation.0, RECORD);
    let before = report(&runtime);
    assert_eq!(
        call(&mut runtime, bound, vec![]).unwrap(),
        Value::Number(7.0)
    );
    let invoked = deltas(before, report(&runtime));
    assert_eq!(invoked.0, 0);
    (creation.1, invoked.1)
}

#[test]
fn owned_receiver_and_prefix_move_at_bind_but_real_forwarding_and_boxing_copies_remain() {
    for (receiver, invocation_slope) in [(false, 2), (true, 4)] {
        let small = payload_cost(512, receiver);
        let large = payload_cost(1024, receiver);
        assert_eq!(large.0 - small.0, 512 * 2);
        // Ordinary non-strict targets retain their additional receiver boxing
        // transfer charge; binding is not an exemption from that policy.
        assert_eq!(large.1 - small.1, 512 * invocation_slope);
    }
}

#[test]
fn opaque_host_forwarding_preserves_object_symbol_and_utf16_identity_without_coercion() {
    let mut runtime = Runtime::new();
    let object = runtime.execute("({value:7});", &mut NoIo).unwrap();
    let symbol = runtime.execute("Symbol('retained');", &mut NoIo).unwrap();
    let text = Value::String(vec![0xd800, 0xdfff]);
    let bound = bind(
        &mut runtime,
        Value::Native("host.probe".into()),
        object.clone(),
        vec![symbol.clone(), text.clone(), object.clone()],
    );
    let mut host = ProbeHost::default();
    for _ in 0..2 {
        assert_eq!(
            runtime
                .invoke(bound.clone(), Value::Null, vec![], &mut host)
                .unwrap(),
            Value::Number(3.0)
        );
        assert_eq!(host.receiver, Some(object.clone()));
        assert_eq!(
            host.arguments,
            vec![symbol.clone(), text.clone(), object.clone()]
        );
    }
    assert_eq!(host.calls, 2);
}

fn outer_receiver_call_cost(length: usize) -> u64 {
    let mut runtime = Runtime::new();
    let inner = bind(
        &mut runtime,
        Value::Native("host.probe".into()),
        Value::Null,
        vec![],
    );
    let outer = bind(
        &mut runtime,
        inner,
        Value::String(vec![0xd800; length]),
        vec![],
    );
    let before = report(&runtime);
    let mut host = ProbeHost::default();
    runtime
        .invoke(outer, Value::Undefined, vec![], &mut host)
        .unwrap();
    assert_eq!(host.receiver, Some(Value::Null));
    let delta = deltas(before, report(&runtime));
    assert_eq!(delta.0, 0);
    delta.1
}

#[test]
fn ignored_outer_bound_receivers_are_not_copied_again_during_forwarding() {
    assert_eq!(
        outer_receiver_call_cost(512),
        outer_receiver_call_cost(900_000)
    );
}

#[test]
fn binding_record_bag_and_exact_slots_reject_before_publishing() {
    for (remaining, phase, request, expected) in [
        (160, AllocationPhase::FunctionCode, RECORD, (0, BIND_NAME)),
        (200, AllocationPhase::Runtime, 128, (RECORD, BIND_NAME)),
        (
            330,
            AllocationPhase::Runtime,
            SLOT,
            (RECORD, BIND_NAME + 128),
        ),
    ] {
        let mut runtime = Runtime::new();
        let target = target(&mut runtime);
        runtime.set_global("earlier", Value::Bool(true));
        leave_budget(&mut runtime, remaining);
        let before = report(&runtime);
        let error =
            bind_args(&mut runtime, target, vec![Value::Null, Value::Number(7.0)]).unwrap_err();
        let first = failure(&runtime, &error, phase, request);
        assert_eq!(deltas(before, first), expected);
        assert_eq!(runtime.get_global("earlier"), Value::Bool(true));
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn forwarding_slots_precede_retained_payload_copies_and_host_effects() {
    for (remaining, request, accepted) in [(50, SLOT, 0), (100, 2048, SLOT)] {
        let mut runtime = Runtime::new();
        let bound = bind(
            &mut runtime,
            Value::Native("host.probe".into()),
            Value::Null,
            vec![Value::String(vec![0xd800; 1024])],
        );
        leave_budget(&mut runtime, remaining);
        let before = report(&runtime);
        let mut host = ProbeHost::default();
        let error = runtime
            .invoke(bound, Value::Undefined, vec![], &mut host)
            .unwrap_err();
        let first = failure(&runtime, &error, AllocationPhase::Runtime, request);
        assert_eq!(deltas(before, first), (0, accepted));
        assert_eq!(host.calls, 0);
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn aggregate_limit_is_checked_before_single_output_storage_or_retained_copies() {
    let mut runtime = Runtime::new();
    let inner = bind(
        &mut runtime,
        Value::Native("host.probe".into()),
        Value::Null,
        vec![Value::Number(1.0); 9999],
    );
    let bound = bind(&mut runtime, inner, Value::Null, vec![Value::Number(2.0)]);
    let mut host = ProbeHost::default();
    assert_eq!(
        runtime
            .invoke(bound.clone(), Value::Undefined, vec![], &mut host)
            .unwrap(),
        Value::Number(10_000.0)
    );
    assert_eq!(host.arguments[0], Value::Number(1.0));
    assert_eq!(host.arguments[9999], Value::Number(2.0));
    let before = report(&runtime);
    let error = runtime
        .invoke(bound, Value::Undefined, vec![Value::Number(3.0)], &mut host)
        .unwrap_err();
    assert!(error.contains("argument limit exhausted"), "{error}");
    assert_eq!(host.calls, 1);
    assert_eq!(report(&runtime), before);
    latch(&mut runtime, &error, before);
}

#[test]
fn aggregate_overflow_preserves_prior_script_effects_and_skips_handlers_and_assignment() {
    let mut runtime = Runtime::new();
    let target = runtime
        .execute("(function(){called=true;return 7;});", &mut NoIo)
        .unwrap();
    let inner = bind(
        &mut runtime,
        target,
        Value::Null,
        vec![Value::Number(1.0); 9999],
    );
    let bound = bind(&mut runtime, inner, Value::Null, vec![Value::Number(2.0)]);
    runtime.set_global("bound", bound);
    let error = runtime.execute(
        "var prior=true,called=false,result=17,caught=false,finalized=false;try{result=bound(3);}catch(e){caught=true;}finally{finalized=true;}",
        &mut NoIo,
    ).unwrap_err();
    assert!(error.contains("argument limit exhausted"), "{error}");
    for name in ["called", "caught", "finalized"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false));
    }
    assert_eq!(runtime.get_global("prior"), Value::Bool(true));
    assert_eq!(runtime.get_global("result"), Value::Number(17.0));
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn retained_large_values_are_recharged_for_each_native_ordinary_and_host_forward() {
    for kind in ["ordinary", "native", "host"] {
        let mut runtime = Runtime::new();
        runtime.set_global("visits", Value::Number(0.0));
        let value = Value::String(vec![0xd800; 900_000]);
        let bound = match kind {
            "ordinary" => {
                let target = runtime
                    .execute("(function(){visits++;return 7;});", &mut NoIo)
                    .unwrap();
                bind(&mut runtime, target, Value::Null, vec![value])
            }
            "native" => bind(
                &mut runtime,
                Value::Native("String.valueOf".into()),
                value,
                vec![],
            ),
            _ => bind(
                &mut runtime,
                Value::Native("host.probe".into()),
                Value::Null,
                vec![value],
            ),
        };
        let mut host = ProbeHost::default();
        let first_value = runtime
            .invoke(bound.clone(), Value::Undefined, vec![], &mut host)
            .unwrap();
        match kind {
            "ordinary" => assert_eq!(first_value, Value::Number(7.0)),
            "native" => {
                assert!(matches!(first_value, Value::String(ref units) if units.len()==900_000))
            }
            _ => assert_eq!(first_value, Value::Number(1.0)),
        }
        let error = runtime
            .invoke(bound, Value::Undefined, vec![], &mut host)
            .unwrap_err();
        let first = failure(&runtime, &error, AllocationPhase::Runtime, 1_800_000);
        assert_eq!(host.calls, usize::from(kind == "host"));
        assert_eq!(
            runtime.get_global("visits"),
            Value::Number(if kind == "ordinary" { 1.0 } else { 0.0 })
        );
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn conceptual_calls_include_each_bound_wrapper_and_unwind_after_ordinary_host_errors() {
    for depth in [63, 64] {
        let mut runtime = Runtime::new();
        let function = target(&mut runtime);
        let bound = chain(&mut runtime, function, depth);
        let result = call(&mut runtime, bound, vec![]);
        if depth == 63 {
            assert_eq!(result.unwrap(), Value::Number(7.0));
        } else {
            let error = result.unwrap_err();
            assert_eq!(error, "JavaScript call depth exhausted");
            let first = report(&runtime);
            assert!(first.first_rejected.is_none());
            latch(&mut runtime, &error, first);
        }
    }
    let mut runtime = Runtime::new();
    let bound = chain(&mut runtime, Value::Native("host.probe".into()), 63);
    let mut host = ProbeHost {
        fail_once: true,
        ..ProbeHost::default()
    };
    assert!(
        runtime
            .invoke(bound.clone(), Value::Undefined, vec![], &mut host)
            .unwrap_err()
            .contains("authored ordinary Host error")
    );
    assert_eq!(
        runtime
            .invoke(bound, Value::Undefined, vec![], &mut host)
            .unwrap(),
        Value::Number(0.0)
    );
    assert_eq!(host.calls, 2);
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn instance_delegation_has_its_separate_sixty_four_bound_target_cap() {
    for depth in [64, 65] {
        let mut runtime = Runtime::new();
        let function = runtime
            .execute("function C(){}var subject=new C();C;", &mut NoIo)
            .unwrap();
        let bound = chain(&mut runtime, function, depth);
        runtime.set_global("bound", bound);
        let result = runtime.execute("subject instanceof bound;", &mut NoIo);
        if depth == 64 {
            assert_eq!(result.unwrap(), Value::Bool(true));
        } else {
            let error = result.unwrap_err();
            assert!(
                error.contains("bound") && (error.contains("limit") || error.contains("depth")),
                "{error}"
            );
            let first = report(&runtime);
            assert!(first.first_rejected.is_none());
            latch(&mut runtime, &error, first);
        }
    }
}

#[test]
fn repeated_forwarding_still_exhausts_fuel_before_target_or_handler_recovery() {
    let mut runtime = Runtime::new();
    let bound = bind(
        &mut runtime,
        Value::Native("Number.isFinite".into()),
        Value::Null,
        vec![Value::Number(1.0)],
    );
    runtime.set_global("bound", bound);
    let error = runtime.execute(
        "var completed=0,caught=false,finalized=false;try{while(true){bound();completed++;}}catch(e){caught=true;}finally{finalized=true;}",
        &mut NoIo,
    ).unwrap_err();
    assert!(error.contains("fuel exhausted"), "{error}");
    assert!(matches!(runtime.get_global("completed"), Value::Number(count) if count>0.0));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn object_cap_blocks_new_bound_bags_without_refunding_the_admitted_record() {
    let mut runtime = Runtime::new();
    let target = target(&mut runtime);
    let mut count = 0;
    let error = loop {
        match bind_args(&mut runtime, target.clone(), vec![Value::Null]) {
            Ok(Value::Function(_)) => count += 1,
            Ok(value) => panic!("unexpected binding result: {value:?}"),
            Err(error) => break error,
        }
        assert!(count < 10_000);
    };
    assert!(error.contains("object limit exhausted"), "{error}");
    assert!((9900..10_000).contains(&count));
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
    // One actual property bag per function makes the object cap dominate the
    // public function cap. Private paid metadata rows isolate the latter guard.
}

#[test]
fn bind_and_forward_argument_caps_do_not_become_partial_or_catchable_success() {
    let mut runtime = Runtime::new();
    let target = target(&mut runtime);
    let before = report(&runtime);
    let error = bind_args(&mut runtime, target, vec![Value::Null; 10_001]).unwrap_err();
    assert!(error.contains("argument limit exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(deltas(before, first), (0, BIND_NAME));
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);

    let mut runtime = Runtime::new();
    let bound = bind(
        &mut runtime,
        Value::Native("Array".into()),
        Value::Null,
        vec![Value::Number(10_001.0)],
    );
    let error = call(&mut runtime, bound, vec![]).unwrap_err();
    assert!(error.contains("array limit exhausted"), "{error}");
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn combined_expression_and_bound_reentry_remains_bounded_on_default_stack() {
    const CHILD: &str = "MGBROWSER_BOUND_RESOURCE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        for depth in [8, 32, 96] {
            let mut runtime = Runtime::new();
            let source = format!(
                "var caught=false,finalized=false;function recurse(){{return {}bound();}}var bound=recurse.bind(null);try{{bound();}}catch(e){{caught=true;}}finally{{finalized=true;}}",
                "+ ".repeat(depth)
            );
            let error = runtime.execute(&source, &mut NoIo).unwrap_err();
            assert!(error.contains("evaluation depth"), "depth {depth}: {error}");
            assert_eq!(runtime.get_global("caught"), Value::Bool(false));
            assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
            let first = report(&runtime);
            assert!(first.first_rejected.is_none());
            latch(&mut runtime, &error, first);
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
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "combined_expression_and_bound_reentry_remains_bounded_on_default_stack",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove("RUST_MIN_STACK")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start only the owned default-stack resource test child"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(
                status.success(),
                "owned bound-resource child failed: {status}"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "owned bound-resource child deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
