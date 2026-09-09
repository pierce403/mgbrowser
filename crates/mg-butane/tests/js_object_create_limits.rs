//! Independent Object.create-only descriptor resource controls.
//! Authored inputs; unchanged caps; no public descriptor-definition/reflection API.
use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const LIMIT: u64 = 4 * 1024 * 1024;
struct NoIo;
impl Host for NoIo {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        panic!("unexpected Host.get")
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host.set")
    }
    fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected Host.call")
    }
}
fn report(runtime: &Runtime) -> AllocationReport {
    let result = runtime.allocation_report();
    assert!(result.is_valid(), "{result:?}");
    assert_eq!(result.limit_bytes, LIMIT);
    result
}
fn create(runtime: &mut Runtime, arguments: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(
        Value::Native("Object.create".into()),
        Value::Undefined,
        arguments,
        &mut NoIo,
    )
}
fn runtime_delta(before: AllocationReport, after: AllocationReport) -> u64 {
    let mut phases = before.phases;
    phases.runtime = after.phases.runtime;
    assert_eq!(phases, after.phases);
    assert!(before.first_rejected.is_none() && after.first_rejected.is_none());
    let result = after.phases.runtime - before.phases.runtime;
    assert_eq!(after.accepted_bytes - before.accepted_bytes, result);
    result
}
fn latch(runtime: &mut Runtime, error: &str, first: AllocationReport) {
    assert!(runtime.is_fatal());
    assert_eq!(
        runtime.execute("var forbidden=1;", &mut NoIo).unwrap_err(),
        error
    );
    assert_eq!(create(runtime, vec![Value::Null]).unwrap_err(), error);
    runtime.set_global("forbidden", Value::Number(2.0));
    assert_eq!(runtime.get_global("forbidden"), Value::Undefined);
    assert_eq!(report(runtime), first);
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
}

#[test]
fn bootstrap_and_legacy_omitted_or_undefined_map_costs_stay_unchanged() {
    for args in [vec![Value::Null], vec![Value::Null, Value::Undefined]] {
        let mut runtime = Runtime::new();
        let before = report(&runtime);
        assert_eq!(before.accepted_bytes, 26_880);
        assert_eq!(before.phases.bootstrap, 26_880);
        assert!(matches!(
            create(&mut runtime, args).unwrap(),
            Value::Object(_)
        ));
        assert_eq!(
            runtime_delta(before, report(&runtime)),
            128 + "Object.create".len() as u64
        );
    }
}

#[derive(Default)]
struct Adapter {
    calls: usize,
    writes: Vec<Value>,
}
impl Host for Adapter {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        panic!("adapter uses configured getter")
    }
    fn set(&mut self, object: &str, key: &str, value: Value) -> Result<(), String> {
        assert_eq!((object, key), ("fixture", "bridge"));
        self.writes.push(value);
        Ok(())
    }
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.adapter.get");
        assert_eq!(this, Value::Object(0));
        assert!(args.is_empty());
        self.calls += 1;
        Ok(Value::Number(7.0))
    }
}
#[test]
fn existing_host_global_adapter_keeps_direct_setter_child_shadow_and_passive_inspection() {
    let mut runtime = Runtime::new();
    let getter = Value::Native("host.adapter.get".into());
    runtime.set_global_accessor("bridge", "fixture", "bridge", getter.clone());
    let mut host = Adapter::default();
    let before = report(&runtime);
    assert_eq!(runtime.get_global("bridge"), getter);
    assert_eq!(report(&runtime), before);
    assert_eq!(runtime.execute("bridge=9;var child=Object.create(globalThis);child.bridge=11;bridge===7 && child.bridge===11 && child.hasOwnProperty('bridge');", &mut host).unwrap(), Value::Bool(true));
    assert_eq!(host.calls, 1);
    assert_eq!(host.writes, vec![Value::Number(9.0)]);
    assert_eq!(runtime.get_global("bridge"), getter);
    assert_eq!(host.calls, 1);
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn only_callable_setter_pairs_add_the_real_64_byte_box_and_16_byte_allowance() {
    let cost = |setter: bool| {
        let mut runtime = Runtime::new();
        runtime.execute("var calls=0;function get(){calls++;return 1;}function set(v){calls++;}var absent={p:{get:get,set:undefined}},present={p:{get:get,set:set}};", &mut NoIo).unwrap();
        let map = runtime.get_global(if setter { "present" } else { "absent" });
        let before = report(&runtime);
        let result = create(&mut runtime, vec![Value::Null, map]).unwrap();
        assert!(matches!(result, Value::Object(_)));
        assert_eq!(runtime.get_global("calls"), Value::Number(0.0));
        runtime_delta(before, report(&runtime))
    };
    assert_eq!(cost(true) - cost(false), 80);
}

fn descriptor_cost(length: usize, long_key: bool) -> u64 {
    let mut runtime = Runtime::new();
    runtime.set_global("payload", Value::String(vec![b'x' as u16; length]));
    let source = if long_key {
        "var map={};map[payload]={value:1};map;"
    } else {
        "var map={p:{value:payload}};map;"
    };
    let map = runtime.execute(source, &mut NoIo).unwrap();
    let before = report(&runtime);
    create(&mut runtime, vec![Value::Null, map]).unwrap();
    runtime_delta(before, report(&runtime))
}
#[test]
fn copied_snapshot_keys_and_descriptor_get_payloads_pay_for_their_real_buffers_once() {
    assert_eq!(
        descriptor_cost(512, true) - descriptor_cost(128, true),
        512 - 128
    );
    assert_eq!(
        descriptor_cost(512, false) - descriptor_cost(128, false),
        2 * (512 - 128)
    );
}

fn dense_map(runtime: &mut Runtime, length: usize) -> Value {
    // A null-prototype empty record avoids incidental inherited field lookups.
    let descriptor = create(runtime, vec![Value::Null]).unwrap();
    runtime
        .invoke(
            Value::Native("Array".into()),
            Value::Undefined,
            vec![descriptor; length],
            &mut NoIo,
        )
        .unwrap()
}
#[test]
fn full_snapshot_boundary_counts_the_non_enumerable_array_length_key() {
    let mut runtime = Runtime::new();
    let map = dense_map(&mut runtime, 9_999);
    let result = create(&mut runtime, vec![Value::Null, map]).unwrap();
    runtime.set_global("result", result);
    assert_eq!(
        runtime
            .execute(
                "'0' in result && '9998' in result && !('length' in result);",
                &mut NoIo
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert!(report(&runtime).first_rejected.is_none());

    let mut runtime = Runtime::new();
    let map = dense_map(&mut runtime, 10_000);
    let error = create(&mut runtime, vec![Value::Null, map]).unwrap_err();
    assert!(error.contains("limit exhausted"), "{error}");
    let first = report(&runtime);
    assert!(
        first.first_rejected.is_none(),
        "key-count cap must not become a heap failure"
    );
    latch(&mut runtime, &error, first);
}

#[test]
fn snapshot_and_exact_staging_preflight_precede_the_first_observable_map_get() {
    for remaining in [150, 250] {
        let mut runtime = Runtime::new();
        let map = runtime.execute("var seen=0;Object.create(null,{p:{get:function(){seen++;return {value:1};},enumerable:true}});", &mut NoIo).unwrap();
        leave(&mut runtime, remaining);
        let error = create(&mut runtime, vec![Value::Null, map]).unwrap_err();
        assert!(error.starts_with("JavaScript allocation budget exhausted"));
        assert_eq!(runtime.get_global("seen"), Value::Number(0.0));
        let first = report(&runtime);
        assert_eq!(
            first.first_rejected.unwrap().phase,
            AllocationPhase::Runtime
        );
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn ordinary_descriptor_getter_error_keeps_prior_effects_without_publishing_a_result() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.execute("var effects=0,result='old',caught=false,sentinel={};var map=Object.create(null,{first:{get:function(){effects++;return {value:1};},enumerable:true},second:{get:function(){throw sentinel;},enumerable:true}});try{result=Object.create(null,map);}catch(e){caught=e===sentinel;}effects===1 && caught && result==='old';", &mut NoIo).unwrap(), Value::Bool(true));
    assert!(!runtime.is_fatal());
    assert!(report(&runtime).first_rejected.is_none());
    assert_eq!(
        runtime.execute("1+2;", &mut NoIo).unwrap(),
        Value::Number(3.0)
    );
}

#[test]
fn a_real_large_descriptor_value_read_still_fails_before_any_adoption() {
    let mut runtime = Runtime::new();
    runtime.set_global("payload", Value::String(vec![b'x' as u16; 600_000]));
    let map = runtime
        .execute("({p:{value:payload}});", &mut NoIo)
        .unwrap();
    assert!(report(&runtime).first_rejected.is_none());
    let error = create(&mut runtime, vec![Value::Null, map]).unwrap_err();
    assert!(error.starts_with("JavaScript allocation budget exhausted"));
    let first = report(&runtime);
    let failure = first.first_rejected.unwrap();
    assert_eq!(failure.phase, AllocationPhase::Runtime);
    assert_eq!(failure.requested_bytes, 1_200_000);
    latch(&mut runtime, &error, first);
}

#[test]
fn repeated_discarded_descriptor_results_keep_cumulative_storage_and_first_failure() {
    let mut runtime = Runtime::new();
    let map = dense_map(&mut runtime, 128);
    // Real public ingress leaves a finite heap window, so this checks repeated
    // allocation failure rather than accidentally exhausting traversal fuel.
    leave(&mut runtime, 200_000);
    let mut previous = report(&runtime);
    let mut successful = 0;
    for _ in 0..256 {
        match create(&mut runtime, vec![Value::Null, map.clone()]) {
            Ok(result) => {
                assert!(matches!(result, Value::Object(_)));
                let next = report(&runtime);
                assert!(next.phases.runtime > previous.phases.runtime);
                assert_eq!(next.phases.ast, previous.phases.ast);
                assert_eq!(next.phases.source, previous.phases.source);
                previous = next;
                successful += 1;
            }
            Err(error) => {
                assert!(successful >= 2);
                assert!(
                    error.starts_with("JavaScript allocation budget exhausted"),
                    "{error}"
                );
                let first = report(&runtime);
                assert!(first.first_rejected.is_some());
                assert!(first.accepted_bytes >= previous.accepted_bytes);
                latch(&mut runtime, &error, first);
                return;
            }
        }
    }
    panic!("cumulative descriptor storage unexpectedly escaped the fixed cap");
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn accessor_reentry_stays_fatal_and_latched_on_an_owned_default_stack() {
    const CHILD: &str = "MGBROWSER_OBJECT_CREATE_RESOURCE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        for (descriptor, operation) in [
            ("get:function(){entered++;return object.p;}", "object.p;"),
            ("set:function(v){entered++;object.p=v;}", "object.p=1;"),
        ] {
            let mut runtime = Runtime::new();
            let source = format!(
                "var entered=0,caught=0,cleaned=0;var object=Object.create(null,{{p:{{{descriptor}}}}});try{{{operation}}}catch(e){{caught=1;}}finally{{cleaned=1;}}"
            );
            let error = runtime.execute(&source, &mut NoIo).unwrap_err();
            assert!(
                error.contains("call depth exhausted")
                    || error.contains("evaluation depth limit exhausted"),
                "{error}"
            );
            assert!(matches!(runtime.get_global("entered"), Value::Number(value) if value > 0.0));
            assert_eq!(runtime.get_global("caught"), Value::Number(0.0));
            assert_eq!(runtime.get_global("cleaned"), Value::Number(0.0));
            let first = report(&runtime);
            latch(&mut runtime, &error, first);
        }
        return;
    }
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "accessor_reentry_stays_fatal_and_latched_on_an_owned_default_stack",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove("RUST_MIN_STACK")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "owned accessor test timed out");
        std::thread::sleep(Duration::from_millis(5));
    };
    fn output(reader: impl Read) -> String {
        let mut bytes = Vec::new();
        reader.take(16_385).read_to_end(&mut bytes).unwrap();
        assert!(bytes.len() <= 16_384);
        String::from_utf8_lossy(&bytes).into_owned()
    }
    let stdout = output(child.0.stdout.take().unwrap());
    let stderr = output(child.0.stderr.take().unwrap());
    assert!(
        status.success(),
        "owned accessor child {status}: {stdout}\n{stderr}"
    );
}
