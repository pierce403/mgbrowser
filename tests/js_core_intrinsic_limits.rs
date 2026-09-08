//! Independent resource gates for docs/CORE_INTRINSICS.md. Authored inputs,
//! public Runtime/Host APIs, unchanged caps, and no alternate evaluator.
use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const LIMIT: u64 = 4 * 1024 * 1024;
const EMPTY_CALL: u64 = 265;
const OBJECT: u64 = 128;

#[derive(Default)]
struct Probe {
    events: Vec<&'static str>,
    source: Option<Value>,
    fail_number: bool,
}
impl Host for Probe {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!((object, key), ("input", "valueOf"));
        self.events.push("get-number");
        Ok(Value::Native("host.number".into()))
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host setter")
    }
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        assert!(args.is_empty());
        match name {
            "host.number" => {
                assert_eq!(this, Value::Host("input".into()));
                self.events.push("number");
                if self.fail_number {
                    Err("authored conversion denied".into())
                } else {
                    Ok(Value::Number(9.0))
                }
            }
            "host.source" => {
                assert_eq!(this, Value::Undefined);
                self.events.push("source");
                Ok(self.source.take().expect("one owned Host output"))
            }
            _ => panic!("unexpected Host call: {name}"),
        }
    }
}

fn report(runtime: &Runtime) -> AllocationReport {
    let r = runtime.allocation_report();
    assert!(r.is_valid(), "{r:?}");
    assert_eq!(r.limit_bytes, LIMIT);
    r
}
fn runtime_delta(before: AllocationReport, after: AllocationReport) -> u64 {
    let mut phases = before.phases;
    phases.runtime = after.phases.runtime;
    assert_eq!(phases, after.phases);
    assert_eq!(
        after.accepted_bytes - before.accepted_bytes,
        after.phases.runtime - before.phases.runtime
    );
    after.phases.runtime - before.phases.runtime
}
fn invoke(runtime: &mut Runtime, target: Value, host: &mut Probe) -> Result<Value, String> {
    runtime.invoke(target, Value::Undefined, vec![], host)
}
fn runner(runtime: &mut Runtime, body: &str) -> Value {
    runtime
        .execute(&format!("(function(){{{body}}});"), &mut Probe::default())
        .unwrap()
}
fn leave(runtime: &mut Runtime, remaining: u64) {
    runtime.set_global("padding", Value::String(vec![]));
    let available = LIMIT - report(runtime).accepted_bytes;
    assert!(available > remaining + 2);
    runtime.set_global(
        "padding",
        Value::String(vec![b'p' as u16; ((available - remaining) / 2) as usize]),
    );
    assert!((remaining..=remaining + 1).contains(&(LIMIT - report(runtime).accepted_bytes)));
    assert!(report(runtime).first_rejected.is_none());
}
fn latch(runtime: &mut Runtime, error: &str, host: &mut Probe) {
    let first = report(runtime);
    let events = host.events.clone();
    assert_eq!(
        runtime.execute("var forbidden=true;", host).unwrap_err(),
        error
    );
    assert_eq!(runtime.get_global("forbidden"), Value::Undefined);
    assert_eq!(
        runtime
            .invoke(
                Value::Native("host.number".into()),
                Value::Host("input".into()),
                vec![],
                host
            )
            .unwrap_err(),
        error
    );
    runtime.set_global("forbidden_ingress", Value::Bool(true));
    assert_eq!(runtime.get_global("forbidden_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
    assert_eq!(host.events, events);
}
fn rejected(runtime: &Runtime, error: &str, bytes: u64) {
    assert!(
        error.starts_with("JavaScript allocation budget exhausted"),
        "{error}"
    );
    let r = report(runtime);
    let first = r.first_rejected.unwrap();
    assert_eq!(first.phase, AllocationPhase::Runtime);
    assert_eq!(first.requested_bytes, bytes);
    assert_eq!(first.accepted_bytes, r.accepted_bytes);
    assert!(bytes > LIMIT - r.accepted_bytes);
}
fn bind(runtime: &mut Runtime, name: &str, receiver: Value, prefix: Vec<Value>) -> Value {
    let mut args = vec![receiver];
    args.extend(prefix);
    runtime
        .invoke(
            Value::Native("Function.bind".into()),
            Value::Native(name.into()),
            args,
            &mut Probe::default(),
        )
        .unwrap()
}
fn unbox(runtime: &mut Runtime, name: &str, object: Value) -> Value {
    runtime
        .invoke(
            Value::Native(format!("{name}.valueOf")),
            object,
            vec![],
            &mut Probe::default(),
        )
        .unwrap()
}

#[test]
fn bootstrap_adds_only_five_real_backlinks_and_no_primitive_payload_buffers() {
    let r = report(&Runtime::new());
    let names = ["Object", "Array", "String", "Number", "Boolean"];
    let added: u64 = names
        .iter()
        .map(|name| OBJECT + 11 + name.len() as u64)
        .sum();
    assert_eq!(added, 725);
    assert_eq!(r.phases.bootstrap, 26_155 + added);
    assert_eq!(r.accepted_bytes, r.phases.bootstrap);
    assert_eq!(
        [
            r.phases.source,
            r.phases.ast,
            r.phases.function_code,
            r.phases.runtime,
            r.phases.regex_compile,
            r.phases.regex_result
        ],
        [0; 6]
    );
    assert!(r.first_rejected.is_none());
}

#[test]
fn each_scalar_construction_adds_exactly_one_128_byte_instance() {
    for name in ["Number", "Boolean"] {
        for argument in ["", "undefined", "null", "false", "-0", "Infinity"] {
            let mut ordinary = Runtime::new();
            let call = runner(&mut ordinary, &format!("return {name}({argument});"));
            let before = report(&ordinary);
            invoke(&mut ordinary, call, &mut Probe::default()).unwrap();
            let ordinary_cost = runtime_delta(before, report(&ordinary));

            let mut constructed = Runtime::new();
            let call = runner(&mut constructed, &format!("return new {name}({argument});"));
            let before = report(&constructed);
            let first = invoke(&mut constructed, call.clone(), &mut Probe::default()).unwrap();
            assert!(matches!(first, Value::Object(_)));
            assert_eq!(
                runtime_delta(before, report(&constructed)),
                ordinary_cost + OBJECT
            );
            let second = invoke(&mut constructed, call, &mut Probe::default()).unwrap();
            assert_ne!(first, second);
        }
    }
}

#[test]
fn repeated_intrinsic_unboxing_does_not_materialize_new_instances() {
    for (name, expected) in [
        ("String", Value::String(vec![])),
        ("Number", Value::Number(0.0)),
        ("Boolean", Value::Bool(false)),
    ] {
        let mut runtime = Runtime::new();
        let prototype = runtime
            .execute(&format!("{name}.prototype;"), &mut Probe::default())
            .unwrap();
        let before = report(&runtime);
        for _ in 0..8 {
            assert_eq!(unbox(&mut runtime, name, prototype.clone()), expected);
        }
        assert_eq!(
            runtime_delta(before, report(&runtime)),
            8 * format!("{name}.valueOf").len() as u64
        );
    }
}

#[test]
fn inherited_payload_is_not_a_brand_and_rejection_does_not_invoke_hooks() {
    for name in ["String", "Number", "Boolean"] {
        let mut runtime = Runtime::new();
        runtime.set_global("poison", Value::Native("host.number".into()));
        let object = runtime.execute(&format!(
            "var child=Object.create({name}.prototype);child.valueOf=poison;child.toString=poison;child;"),
            &mut Probe::default()).unwrap();
        let mut host = Probe::default();
        let before = report(&runtime);
        let method = format!("{name}.valueOf");
        let error = runtime
            .invoke(Value::Native(method.clone()), object, vec![], &mut host)
            .unwrap_err();
        assert!(error.contains("TypeError"), "{error}");
        assert!(host.events.is_empty());
        assert_eq!(runtime_delta(before, report(&runtime)), method.len() as u64);
        assert!(report(&runtime).first_rejected.is_none());
        assert_eq!(
            runtime.execute("42;", &mut host).unwrap(),
            Value::Number(42.0)
        );
    }
}

fn host_string_cost(name: &str, count: usize, extra: bool) -> u64 {
    let mut runtime = Runtime::new();
    runtime.set_global("source", Value::Native("host.source".into()));
    let arguments = if extra { "0,source()" } else { "source()" };
    let call = runner(&mut runtime, &format!("return new {name}({arguments});"));
    let mut host = Probe {
        source: Some(Value::String(vec![b' ' as u16; count])),
        ..Probe::default()
    };
    let before = report(&runtime);
    let object = invoke(&mut runtime, call, &mut host).unwrap();
    assert!(matches!(object, Value::Object(_)));
    assert_eq!(host.events, ["source"]);
    runtime_delta(before, report(&runtime))
}

#[test]
fn consumed_host_strings_and_ignored_extra_values_retain_only_real_ingress() {
    for name in ["Number", "Boolean"] {
        for extra in [false, true] {
            assert_eq!(
                host_string_cost(name, 1024, extra) - host_string_cost(name, 16, extra),
                2 * (1024 - 16)
            );
        }
        // 2.2 MiB of real incoming UTF-16 fits; a second synthetic payload copy
        // would exceed the unchanged cap. Number consumes valid whitespace.
        assert!(host_string_cost(name, 1_100_000, false) >= 2_200_000);
    }
}

fn native_string_cost(name: &str, count: usize) -> u64 {
    let mut runtime = Runtime::new();
    let before = report(&runtime);
    runtime
        .invoke(
            Value::Native(name.into()),
            Value::Undefined,
            vec![Value::String(vec![b' ' as u16; count])],
            &mut Probe::default(),
        )
        .unwrap();
    runtime_delta(before, report(&runtime))
}

#[test]
fn ordinary_constructor_calls_and_existing_boxing_keep_their_real_copy_slopes() {
    for (name, slope) in [("Number", 4), ("Boolean", 4), ("String", 4), ("Object", 6)] {
        assert_eq!(
            native_string_cost(name, 1024) - native_string_cost(name, 16),
            slope * (1024 - 16),
            "{name}"
        );
    }
}

#[test]
fn actual_first_copy_rejection_in_ordinary_number_and_boolean_calls_still_latches() {
    for name in ["Number", "Boolean"] {
        let mut runtime = Runtime::new();
        let mut host = Probe::default();
        let error = runtime
            .invoke(
                Value::Native(name.into()),
                Value::Undefined,
                vec![Value::String(vec![b' ' as u16; 1_100_000])],
                &mut host,
            )
            .unwrap_err();
        rejected(&runtime, &error, 2_200_000);
        assert_eq!(
            report(&runtime).phases.runtime,
            name.len() as u64 + 2_200_000
        );
        latch(&mut runtime, &error, &mut host);
    }
}

#[test]
fn number_converts_once_while_boolean_never_asks_host_for_a_primitive() {
    for name in ["Number", "Boolean"] {
        let mut runtime = Runtime::new();
        runtime.set_global("value", Value::Host("input".into()));
        let call = runner(&mut runtime, &format!("return new {name}(value);"));
        let mut host = Probe::default();
        let object = invoke(&mut runtime, call, &mut host).unwrap();
        let expected = if name == "Number" {
            Value::Number(9.0)
        } else {
            Value::Bool(true)
        };
        assert_eq!(unbox(&mut runtime, name, object), expected);
        if name == "Number" {
            assert_eq!(host.events, ["get-number", "number"]);
        } else {
            assert!(host.events.is_empty());
        }
    }
}

fn guarded_number(runtime: &mut Runtime) -> Value {
    runtime.set_global("value", Value::Host("input".into()));
    runtime.execute("var result=17,completed=0,caught=false,finished=false;function run(){try{result=new Number(value);completed++;}catch(e){caught=e;}finally{finished=true;}}run;",
        &mut Probe::default()).unwrap()
}

#[test]
fn ordinary_conversion_failure_is_catchable_preserves_value_and_allows_later_work() {
    let mut runtime = Runtime::new();
    let call = guarded_number(&mut runtime);
    let mut host = Probe {
        fail_number: true,
        ..Probe::default()
    };
    invoke(&mut runtime, call.clone(), &mut host).unwrap();
    assert_eq!(host.events, ["get-number", "number"]);
    assert_eq!(
        runtime.get_global("caught"),
        Value::text("authored conversion denied")
    );
    assert_eq!(runtime.get_global("result"), Value::Number(17.0));
    assert_eq!(runtime.get_global("completed"), Value::Number(0.0));
    assert_eq!(runtime.get_global("finished"), Value::Bool(true));
    assert!(report(&runtime).first_rejected.is_none());
    host.fail_number = false;
    invoke(&mut runtime, call, &mut host).unwrap();
    assert_eq!(runtime.get_global("completed"), Value::Number(1.0));
    assert!(matches!(runtime.get_global("result"), Value::Object(_)));
}

#[test]
fn instance_metadata_failure_follows_conversion_but_never_publishes_a_result() {
    let mut runtime = Runtime::new();
    let call = guarded_number(&mut runtime);
    let mut host = Probe::default();
    let before = report(&runtime);
    invoke(&mut runtime, call.clone(), &mut host).unwrap();
    let cost = runtime_delta(before, report(&runtime));
    assert!(cost >= EMPTY_CALL + OBJECT);
    for (key, value) in [
        ("result", Value::Number(17.0)),
        ("completed", Value::Number(0.0)),
        ("caught", Value::Bool(false)),
        ("finished", Value::Bool(false)),
    ] {
        runtime.set_global(key, value);
    }
    host.events.clear();
    // Paired successful control measures only incidental existing traversal;
    // the explicit expected rejected allocation remains the128-byte instance.
    leave(&mut runtime, cost - 2);
    let before = report(&runtime);
    let error = invoke(&mut runtime, call, &mut host).unwrap_err();
    rejected(&runtime, &error, OBJECT);
    assert_eq!(runtime_delta(before, report(&runtime)), cost - OBJECT);
    assert_eq!(host.events, ["get-number", "number"]);
    assert_eq!(runtime.get_global("result"), Value::Number(17.0));
    assert_eq!(runtime.get_global("completed"), Value::Number(0.0));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finished"), Value::Bool(false));
    latch(&mut runtime, &error, &mut host);
}

fn bound_cost(name: &str, receiver: Value, prefix: Vec<Value>) -> u64 {
    let mut runtime = Runtime::new();
    let bound = bind(&mut runtime, name, receiver, prefix);
    runtime.set_global("Bound", bound);
    let call = runner(&mut runtime, "return new Bound();");
    let before = report(&runtime);
    let object = invoke(&mut runtime, call, &mut Probe::default()).unwrap();
    assert!(matches!(object, Value::Object(_)));
    runtime_delta(before, report(&runtime))
}

#[test]
fn bound_construction_keeps_exact_forwarded_slots_and_single_instance_metadata() {
    for name in ["Number", "Boolean"] {
        for count in [0, 1, 2, 257] {
            assert_eq!(
                bound_cost(name, Value::Null, vec![Value::Number(1.0); count]),
                EMPTY_CALL + OBJECT + name.len() as u64 + 64 * count as u64
            );
        }
    }
}

#[test]
fn bound_prefix_real_copy_is_paid_but_ignored_receiver_is_not_copied() {
    for name in ["Number", "Boolean"] {
        let prefix_cost = |count| {
            bound_cost(
                name,
                Value::Null,
                vec![Value::String(vec![b' ' as u16; count])],
            )
        };
        assert_eq!(prefix_cost(1024) - prefix_cost(16), 2 * (1024 - 16));
        let receiver_cost = |count| {
            bound_cost(
                name,
                Value::String(vec![b'x' as u16; count]),
                vec![Value::Number(1.0)],
            )
        };
        assert_eq!(receiver_cost(1024), receiver_cost(16));
    }
}

#[test]
fn bound_aggregate_slot_failure_precedes_conversion_and_instance_allocation() {
    for name in ["Number", "Boolean"] {
        let mut runtime = Runtime::new();
        let mut prefix = vec![Value::Number(1.0); 257];
        prefix[0] = Value::Host("input".into());
        let bound = bind(&mut runtime, name, Value::Null, prefix);
        runtime.set_global("Bound", bound);
        let call = runner(&mut runtime, "return new Bound();");
        leave(&mut runtime, 1000);
        let mut host = Probe::default();
        let error = invoke(&mut runtime, call, &mut host).unwrap_err();
        rejected(&runtime, &error, 257 * 64);
        assert!(host.events.is_empty());
        latch(&mut runtime, &error, &mut host);
    }
}

#[test]
fn cumulative_retained_prefix_copies_preserve_prior_instances_and_latch() {
    for name in ["Number", "Boolean"] {
        let mut runtime = Runtime::new();
        let bound = bind(
            &mut runtime,
            name,
            Value::Null,
            vec![Value::String(vec![b' ' as u16; 400_000])],
        );
        runtime.set_global("Bound", bound);
        let call = runtime.execute("var result=17,completed=0,caught=false,finished=false;function run(){try{result=new Bound();completed++;}catch(e){caught=true;}finally{finished=true;}}run;",
            &mut Probe::default()).unwrap();
        let mut host = Probe::default();
        let mut previous = Value::Number(17.0);
        let mut successes = 0;
        let error = loop {
            runtime.set_global("finished", Value::Bool(false));
            match invoke(&mut runtime, call.clone(), &mut host) {
                Ok(_) => {
                    successes += 1;
                    previous = runtime.get_global("result");
                    assert!(matches!(previous, Value::Object(_)));
                    assert!(successes < 8, "cumulative copies must remain bounded");
                }
                Err(error) => break error,
            }
        };
        assert!(successes > 0);
        rejected(&runtime, &error, 800_000);
        assert_eq!(runtime.get_global("result"), previous);
        assert_eq!(
            runtime.get_global("completed"),
            Value::Number(successes as f64)
        );
        assert_eq!(runtime.get_global("caught"), Value::Bool(false));
        assert_eq!(runtime.get_global("finished"), Value::Bool(false));
        latch(&mut runtime, &error, &mut host);
    }
}

#[test]
fn retained_scalar_instances_still_hit_the_existing_object_cap() {
    for name in ["Number", "Boolean"] {
        let mut runtime = Runtime::new();
        let call = runner(&mut runtime, &format!("return new {name}();"));
        let mut host = Probe::default();
        let mut successes = 0;
        let error = loop {
            match invoke(&mut runtime, call.clone(), &mut host) {
                Ok(Value::Object(_)) => {
                    successes += 1;
                    assert!(successes <= 10_000);
                }
                Ok(other) => panic!("not a distinct object: {other:?}"),
                Err(error) => break error,
            }
        };
        assert!(successes > 9000);
        assert_eq!(error, "JavaScript object limit exhausted");
        assert!(report(&runtime).first_rejected.is_none());
        latch(&mut runtime, &error, &mut host);
    }
}

#[test]
fn conversion_fuel_failure_preserves_effects_and_bypasses_handlers() {
    let mut runtime = Runtime::new();
    let error = runtime.execute("var entered=false,result=17,caught=false,finished=false;var value={valueOf:function(){entered=true;while(true){}return 1;}};try{result=new Number(value);}catch(e){caught=true;}finally{finished=true;}",
        &mut Probe::default()).unwrap_err();
    assert_eq!(error, "JavaScript fuel exhausted");
    assert_eq!(runtime.get_global("entered"), Value::Bool(true));
    assert_eq!(runtime.get_global("result"), Value::Number(17.0));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finished"), Value::Bool(false));
    assert!(report(&runtime).first_rejected.is_none());
    latch(&mut runtime, &error, &mut Probe::default());
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn primitive_constructor_reentry_is_bounded_on_an_owned_default_stack() {
    const FLAG: &str = "MGBROWSER_CORE_INTRINSIC_RESOURCE_CHILD";
    if std::env::var_os(FLAG).is_some() {
        for setup in [
            "var input={valueOf:function(){return new Number(input);}};function run(){new Number(input);}",
            "var input={};input[Symbol.toPrimitive]=function(){return new Number(input);};function run(){new Number(input);}",
            "function run(){new Boolean();return run();}",
        ] {
            let mut runtime = Runtime::new();
            let source = format!(
                "var caught=false,finished=false;{setup}try{{run();}}catch(e){{caught=true;}}finally{{finished=true;}}"
            );
            let mut host = Probe::default();
            let error = runtime.execute(&source, &mut host).unwrap_err();
            assert!(
                error.contains("depth") && error.contains("exhausted"),
                "{error}"
            );
            assert_eq!(runtime.get_global("caught"), Value::Bool(false));
            assert_eq!(runtime.get_global("finished"), Value::Bool(false));
            latch(&mut runtime, &error, &mut host);
        }
        return;
    }
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("primitive_constructor_reentry_is_bounded_on_an_owned_default_stack")
            .arg("--nocapture")
            .env(FLAG, "1")
            .env_remove("RUST_MIN_STACK")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "owned default-stack child: {status}");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "owned child timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
