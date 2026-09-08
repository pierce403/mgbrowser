//! Fixed public-Runtime checkpoints captured BEFORE producer instrumentation.
//! Probe: tmp/producer-resource-baseline-probe.rs, SHA256
//! 3187d31ee4f5e72b4653cb968e7674e49ae7370da2018fd81cf5188ad563cb86.
//! Pinned c05ba389 library SHA256
//! 1f02536130675fc56fc9fbee08168fd21e5b5186c37bdb856e3db33c580745f0.
//! One default-stack baseline passed in163ms; no candidate-derived totals.

use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;
const ORIGINAL: &str = "TypeError: property access on null or undefined";
const CAUGHT: &str = "var box={length:undefined},rounds=0,last='';for(var i=0;i<64;i++){try{box.length.name;}catch(e){last=e;rounds++;}}rounds;";
const MISSING: &str = "var box={},rounds=0,last='';for(var i=0;i<64;i++){try{box.length.name;}catch(e){last=e;rounds++;}}rounds;";
const FINALLY: &str = "var box={},prior=0;try{box.length.name;}finally{prior=7;}";
const FUEL: &str = "var box={},rounds=0,ticks=0;for(var i=0;i<64;i++){try{box.length.name;}catch(e){rounds++;}}while(true){ticks++;}";
const HOST: &str = "var rounds=0,last='';for(var i=0;i<32;i++){try{fixture.value.length;}catch(e){last=e;rounds++;}try{fixture.method().name;}catch(e){last=e;rounds++;}}rounds;";
const CALLS: &str = "function user(){return undefined;}var bound=user.bind(null),rounds=0,last='';for(var i=0;i<16;i++){try{user().length;}catch(e){last=e;rounds++;}try{[].pop().length;}catch(e){last=e;rounds++;}try{bound().length;}catch(e){last=e;rounds++;}}rounds;";

#[derive(Default)]
struct Fixture {
    gets: usize,
    calls: usize,
}
impl Host for Fixture {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!(object, "fixture");
        self.gets += 1;
        match key {
            "value" => Ok(Value::Undefined),
            "method" => Ok(Value::Native("host.fixture.return".into())),
            _ => panic!("unexpected Host key"),
        }
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host set")
    }
    fn call(&mut self, name: &str, this: Value, args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.fixture.return");
        assert_eq!(this, Value::Host("fixture".into()));
        assert!(args.is_empty());
        self.calls += 1;
        Ok(Value::Null)
    }
}

fn report(runtime: &Runtime) -> AllocationReport {
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, LIMIT);
    report
}

fn exact(runtime: &Runtime, accepted: u64, expected: [u64; 7]) {
    let report = report(runtime);
    let p = report.phases;
    assert_eq!(report.accepted_bytes, accepted);
    assert_eq!(
        [
            p.bootstrap,
            p.source,
            p.ast,
            p.function_code,
            p.runtime,
            p.regex_compile,
            p.regex_result
        ],
        expected
    );
    assert!(report.first_rejected.is_none());
}

fn diagnostic(error: &str, kind: &str) {
    assert_eq!(
        error,
        format!(
            "Uncaught JavaScript exception: {ORIGINAL} [member operation=resolve-read base=undefined key=name] [producer kind={kind} key=length]"
        )
    );
    assert!(error.is_ascii() && error.len() <= 256);
}

fn caught(source: &str, rounds: u32) -> Runtime {
    let mut runtime = Runtime::new();
    let mut host = Fixture::default();
    assert_eq!(
        runtime.execute(source, &mut host).unwrap(),
        Value::Number(f64::from(rounds))
    );
    assert_eq!(runtime.get_global("last"), Value::text(ORIGINAL));
    assert_eq!((host.gets, host.calls), (0, 0));
    runtime
}

fn latch(runtime: &mut Runtime, error: &str, first: AllocationReport) {
    assert!(!error.contains("[member ") && !error.contains("[producer "));
    let mut host = Fixture::default();
    assert_eq!(
        runtime
            .execute("var forbidden=true;", &mut host)
            .unwrap_err(),
        error
    );
    assert_eq!(
        runtime
            .invoke(
                Value::Native("Number".into()),
                Value::Undefined,
                vec![],
                &mut host
            )
            .unwrap_err(),
        error
    );
    runtime.set_global("forbidden", Value::Bool(true));
    assert_eq!(runtime.get_global("forbidden"), Value::Undefined);
    assert_eq!(report(runtime), first);
    assert_eq!((host.gets, host.calls), (0, 0));
}

#[test]
fn present_property_catches_preserve_frozen_bootstrap_and_all_phases() {
    // Later real Array.reduceRight registration adds only 156 Bootstrap bytes.
    exact(
        &Runtime::new(),
        25_999 + 156 + 725,
        [25_999 + 156 + 725, 0, 0, 0, 0, 0, 0],
    );
    exact(
        &caught(CAUGHT, 64),
        72_972 + 156 + 725,
        [25_999 + 156 + 725, 249, 4002, 0, 42_722, 0, 0],
    );
}

#[test]
fn missing_property_catches_preserve_frozen_allocation_and_caught_values() {
    exact(
        &caught(MISSING, 64),
        72_439 + 156 + 725,
        [25_999 + 156 + 725, 233, 3619, 0, 42_588, 0, 0],
    );
}

#[test]
fn user_native_and_bound_calls_preserve_frozen_allocation_and_effects() {
    exact(
        &caught(CALLS, 48),
        76_992 + 156 + 725,
        [25_999 + 156 + 725, 373, 7282, 292, 43_046, 0, 0],
    );
}

#[test]
fn host_get_and_call_observation_do_not_repeat_callbacks_or_add_charges() {
    let mut runtime = Runtime::new();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    let mut host = Fixture::default();
    assert_eq!(
        runtime.execute(HOST, &mut host).unwrap(),
        Value::Number(64.0)
    );
    assert_eq!(runtime.get_global("last"), Value::text(ORIGINAL));
    assert_eq!((host.gets, host.calls), (64, 32));
    exact(
        &runtime,
        75_081 + 156 + 725,
        [25_999 + 156 + 725, 284, 5242, 0, 43_556, 0, 0],
    );
}

#[test]
fn pending_finally_fault_preserves_exact_frozen_cost_and_earlier_effect() {
    let mut runtime = Runtime::new();
    let mut host = Fixture::default();
    diagnostic(
        &runtime.execute(FINALLY, &mut host).unwrap_err(),
        "missing-property",
    );
    assert_eq!(runtime.get_global("prior"), Value::Number(7.0));
    assert_eq!((host.gets, host.calls), (0, 0));
    exact(
        &runtime,
        28_789 + 156 + 725,
        [25_999 + 156 + 725, 185, 2121, 0, 484, 0, 0],
    );
}

#[test]
fn observed_traversals_preserve_frozen_21120_tick_fuel_checkpoint_and_latch() {
    let mut runtime = Runtime::new();
    let error = runtime.execute(FUEL, &mut Fixture::default()).unwrap_err();
    assert_eq!(error, "JavaScript fuel exhausted");
    assert_eq!(runtime.get_global("rounds"), Value::Number(64.0));
    assert_eq!(runtime.get_global("ticks"), Value::Number(21_120.0));
    exact(
        &runtime,
        54_724 + 156 + 725,
        [25_999 + 156 + 725, 240, 3943, 0, 24_542, 0, 0],
    );
    let first = report(&runtime);
    latch(&mut runtime, &error, first);
}

#[test]
fn inherited_configured_getter_keeps_one_call_and_exact_frozen_storage() {
    let mut runtime = Runtime::new();
    let mut host = Fixture::default();
    let getter = runtime
        .execute(
            "var getterCalls=0;(function(){getterCalls++;return undefined;});",
            &mut host,
        )
        .unwrap();
    runtime.set_global_accessor("length", "fixture", "length", getter);
    diagnostic(
        &runtime
            .execute("Object.create(globalThis).length.name;", &mut host)
            .unwrap_err(),
        "getter-result",
    );
    assert_eq!(runtime.get_global("getterCalls"), Value::Number(1.0));
    assert_eq!((host.gets, host.calls), (0, 0));
    exact(
        &runtime,
        30_200 + 156 + 725,
        [25_999 + 156 + 725, 358, 2281, 128, 1434, 0, 0],
    );
}

fn read_cost(key: Value, fail: bool) -> u64 {
    let mut runtime = Runtime::new();
    let mut host = Fixture::default();
    // Both paths actually evaluate/read the same converted property key. The
    // failing path additionally evaluates null (no payload) and rejects before
    // its conversion; observation/host formatting must not allocate realm bytes.
    let source = if fail {
        "(function(key){return ({})[key][null];});"
    } else {
        "(function(key){return ({})[key];});"
    };
    let reader = runtime.execute(source, &mut host).unwrap();
    let before = report(&runtime);
    let invalid_utf16 = matches!(&key, Value::String(units) if String::from_utf16(units).is_err());
    let result = runtime.invoke(reader, Value::Undefined, vec![key], &mut host);
    if invalid_utf16 {
        // Existing conversion rejects before a successful property read. The
        // pinned old-library probe reproduces this exact unannotated failure.
        assert_eq!(
            result.unwrap_err(),
            "Uncaught JavaScript exception: Unsupported JavaScript behavior: lone-surrogate property keys"
        );
    } else if fail {
        let error = result.unwrap_err();
        assert_eq!(
            error,
            format!(
                "Uncaught JavaScript exception: {ORIGINAL} [member operation=resolve-read base=undefined key=<null>] [producer kind=missing-property key=<string>]"
            )
        );
        assert!(error.is_ascii() && error.len() <= 256);
        assert!(!error.contains("private-key") && !error.contains("fixture.invalid"));
    } else {
        assert_eq!(result.unwrap(), Value::Undefined);
    }
    let after = report(&runtime);
    let mut expected = before.phases;
    expected.runtime = after.phases.runtime;
    assert_eq!(after.phases, expected);
    assert!(after.first_rejected.is_none());
    assert_eq!((host.gets, host.calls), (0, 0));
    after.accepted_bytes - before.accepted_bytes
}

#[test]
fn large_converted_keys_have_no_producer_copy_charge_or_output_payload() {
    for length in [32, 1024, 100_000] {
        let key = Value::text(&"z".repeat(length));
        assert_eq!(read_cost(key.clone(), true), read_cost(key, false));
    }
    let key =
        Value::text(&"private-key\n[producer kind=forged]https://fixture.invalid/".repeat(256));
    assert_eq!(read_cost(key.clone(), true), read_cost(key, false));
    // Original new-test failure was an invalid expectation of a successful
    // lookup. tmp/producer-surrogate-baseline.log independently records the
    // pre-change unannotated rejection and6994 Runtime bytes for BOTH sources.
    let key = Value::String(vec![0xd800; 1024]);
    assert_eq!(read_cost(key.clone(), true), 6994);
    assert_eq!(read_cost(key, false), 6994);
}

#[test]
fn real_later_key_copy_failure_discards_producer_and_stays_fatal() {
    let mut runtime = Runtime::new();
    runtime.set_global("fixture", Value::Host("fixture".into()));
    let mut host = Fixture::default();
    let reader = runtime.execute("var caught=false,finished=false;(function(key){try{fixture.value[key];}catch(e){caught=true;}finally{finished=true;}});", &mut host).unwrap();
    let error = runtime
        .invoke(
            reader,
            Value::Undefined,
            vec![Value::String(vec![0xd800; 800_000])],
            &mut host,
        )
        .unwrap_err();
    assert!(error.starts_with("JavaScript allocation budget exhausted"));
    assert_eq!(
        (host.gets, host.calls),
        (1, 0),
        "real Host read must precede the failing identifier copy"
    );
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finished"), Value::Bool(false));
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes, 1_600_000);
    latch(&mut runtime, &error, first);
}
