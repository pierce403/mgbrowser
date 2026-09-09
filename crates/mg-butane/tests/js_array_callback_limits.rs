//! Independently authored resource/Host gates for docs/ARRAY_CALLBACKS.md.
//! No website input, alternate evaluator, old-test edits, or relaxed limits.
use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const LIMIT: u64 = 4 * 1024 * 1024;
const METHODS: [&str; 7] = [
    "forEach",
    "map",
    "filter",
    "some",
    "every",
    "reduce",
    "reduceRight",
];

#[derive(Clone, Debug, PartialEq)]
enum Event {
    Length,
    Has(usize),
    Get(usize),
    Call(usize),
}
enum Outcome {
    Constant(Value),
    First,
    Mutate,
    Throw,
}
struct Collection {
    length: Value,
    rows: Vec<Option<Value>>,
    trace: Vec<Event>,
    values: Vec<Value>,
    receivers: Vec<Value>,
    reduction: bool,
    outcome: Outcome,
    presence_error: Option<usize>,
    get_error: Option<usize>,
}
impl Collection {
    fn new(length: Value, rows: Vec<Option<Value>>) -> Self {
        Self {
            length,
            rows,
            trace: vec![],
            values: vec![],
            receivers: vec![],
            reduction: false,
            outcome: Outcome::Constant(Value::Bool(true)),
            presence_error: None,
            get_error: None,
        }
    }
    fn calls(&self) -> usize {
        self.trace
            .iter()
            .filter(|e| matches!(e, Event::Call(_)))
            .count()
    }
}
impl Host for Collection {
    fn has_indexed_property(&mut self, object: &str, index: usize) -> Result<bool, String> {
        assert_eq!(object, "collection");
        self.trace.push(Event::Has(index));
        if self.presence_error == Some(index) {
            return Err("presence denied".into());
        }
        Ok(self.rows.get(index).is_some_and(Option::is_some))
    }
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!(object, "collection");
        if key == "length" {
            self.trace.push(Event::Length);
            return Ok(self.length.clone());
        }
        let index: usize = key.parse().expect("only canonical indexed Host reads");
        assert_eq!(key, index.to_string());
        self.trace.push(Event::Get(index));
        if self.get_error == Some(index) {
            return Err("get denied".into());
        }
        Ok(self
            .rows
            .get(index)
            .and_then(Clone::clone)
            .expect("Get must follow present HasProperty"))
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host setter")
    }
    fn call(&mut self, name: &str, this: Value, mut args: Vec<Value>) -> Result<Value, String> {
        assert_eq!(name, "host.callback");
        assert_eq!(args.len(), if self.reduction { 4 } else { 3 });
        let index_slot = if self.reduction { 2 } else { 1 };
        let Value::Number(index) = args[index_slot] else {
            panic!("callback index must be numeric")
        };
        assert_eq!(args[index_slot + 1], Value::Host("collection".into()));
        self.trace.push(Event::Call(index as usize));
        self.receivers.push(this);
        // Test observations are outside realm accounting. Do not clone large
        // payloads here: tests needing exact values use small scalar inputs.
        if !matches!(args[0], Value::String(_)) {
            self.values.push(args[0].clone());
        }
        match &self.outcome {
            Outcome::Constant(value) => Ok(value.clone()),
            Outcome::First => Ok(args.remove(0)),
            Outcome::Throw => Err("callback denied".into()),
            Outcome::Mutate => {
                if index == 0.0 {
                    self.rows = vec![
                        Some(Value::Number(99.0)),
                        None,
                        Some(Value::Number(9.0)),
                        Some(Value::Number(100.0)),
                    ];
                    self.length = Value::Number(50.0);
                }
                Ok(Value::Bool(true))
            }
        }
    }
}

fn report(runtime: &Runtime) -> AllocationReport {
    let r = runtime.allocation_report();
    assert!(r.is_valid(), "{r:?}");
    assert_eq!(r.limit_bytes, LIMIT);
    r
}
fn call(
    runtime: &mut Runtime,
    host: &mut Collection,
    method: &str,
    callback: Value,
    extra: Option<Value>,
) -> Result<Value, String> {
    host.reduction = method == "reduce" || method == "reduceRight";
    let mut args = vec![callback];
    if let Some(extra) = extra {
        args.push(extra);
    }
    runtime.invoke(
        Value::Native(format!("Array.{method}")),
        Value::Host("collection".into()),
        args,
        host,
    )
}
fn callback() -> Value {
    Value::Native("host.callback".into())
}
fn runtime_delta(before: AllocationReport, after: AllocationReport) -> u64 {
    let mut p = before.phases;
    p.runtime = after.phases.runtime;
    assert_eq!(p, after.phases);
    assert!(before.first_rejected.is_none() && after.first_rejected.is_none());
    assert_eq!(
        after.accepted_bytes - before.accepted_bytes,
        after.phases.runtime - before.phases.runtime
    );
    after.accepted_bytes - before.accepted_bytes
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
fn latch(runtime: &mut Runtime, error: &str) {
    assert!(error.starts_with("JavaScript "), "{error}");
    assert!(!error.contains("[member ") && !error.contains("[producer "));
    let first = report(runtime);
    let mut host = Collection::new(Value::Number(0.0), vec![]);
    assert_eq!(
        runtime
            .execute("var forbidden=true;", &mut host)
            .unwrap_err(),
        error
    );
    assert_eq!(
        call(runtime, &mut host, "forEach", callback(), None).unwrap_err(),
        error
    );
    runtime.set_global("forbidden", Value::Bool(true));
    assert_eq!(runtime.get_global("forbidden"), Value::Undefined);
    assert_eq!(report(runtime), first);
    assert!(host.trace.is_empty());
}

#[test]
fn new_property_has_only_its_measured_bootstrap_allowance() {
    let r = report(&Runtime::new());
    // Old frozen language bootstrap25,999; real new property128 + key11 + value17.
    assert_eq!(r.phases.bootstrap, 25_999 + 128 + 11 + 17 + 725);
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
}

#[test]
fn all_methods_admit_converted_10000_but_reject_10001_before_any_index() {
    for method in METHODS {
        let mut runtime = Runtime::new();
        let mut host = Collection::new(Value::Number(10_000.0), vec![]);
        call(
            &mut runtime,
            &mut host,
            method,
            callback(),
            Some(Value::Number(7.0)),
        )
        .unwrap();
        assert_eq!(
            host.trace
                .iter()
                .filter(|e| matches!(e, Event::Has(_)))
                .count(),
            10_000
        );
        assert_eq!(host.calls(), 0);
        assert!(!host.trace.iter().any(|e| matches!(e, Event::Get(_))));
        let mut runtime = Runtime::new();
        let mut host = Collection::new(Value::Number(10_001.0), vec![]);
        let error = call(
            &mut runtime,
            &mut host,
            method,
            callback(),
            Some(Value::Number(7.0)),
        )
        .unwrap_err();
        assert!(error.contains("array limit exhausted"), "{method}: {error}");
        assert_eq!(host.trace, [Event::Length]);
        latch(&mut runtime, &error);
    }
}

#[test]
fn length_conversion_wraps_before_cap_instead_of_clamping() {
    for (length, expected) in [
        (f64::NAN, 0),
        (f64::INFINITY, 0),
        (f64::NEG_INFINITY, 0),
        (-0.9, 0),
        (4_294_967_296.0, 0),
        (4_294_967_297.9, 1),
        (4_294_977_296.0, 10_000),
    ] {
        let mut runtime = Runtime::new();
        let mut host = Collection::new(Value::Number(length), vec![]);
        call(&mut runtime, &mut host, "forEach", callback(), None).unwrap();
        assert_eq!(host.trace.len(), expected + 1, "length={length}");
        assert_eq!(host.calls(), 0);
    }
    let mut runtime = Runtime::new();
    let mut host = Collection::new(Value::Number(-1.0), vec![]);
    let error = call(&mut runtime, &mut host, "forEach", callback(), None).unwrap_err();
    assert!(error.contains("array limit exhausted"));
    assert_eq!(host.trace, [Event::Length]);
    latch(&mut runtime, &error);
}

#[test]
fn invalid_callback_is_catchable_before_length_cap_and_still_checked_when_empty() {
    for method in METHODS {
        for length in [0.0, 10_001.0] {
            let mut runtime = Runtime::new();
            let mut host = Collection::new(Value::Number(length), vec![]);
            let error =
                call(&mut runtime, &mut host, method, Value::Number(3.0), None).unwrap_err();
            assert!(error.contains("TypeError") && !error.contains("limit exhausted"));
            assert_eq!(host.trace, [Event::Length]);
            assert!(report(&runtime).first_rejected.is_none());
            host.length = Value::Number(0.0);
            assert_eq!(
                call(&mut runtime, &mut host, "forEach", callback(), None).unwrap(),
                Value::Undefined
            );
        }
    }
}

#[test]
fn fallible_length_conversion_runs_once_before_callback_validation() {
    for throws in [false, true] {
        let mut runtime = Runtime::new();
        let mut host = Collection::new(Value::Undefined, vec![]);
        let effect = if throws {
            "throw 'length conversion denied';"
        } else {
            "return 4294967297;"
        };
        host.length = runtime
            .execute(
                &format!("var conversions=0;({{valueOf:function(){{conversions++;{effect}}}}});"),
                &mut host,
            )
            .unwrap();
        let error = call(&mut runtime, &mut host, "forEach", Value::Number(0.0), None).unwrap_err();
        if throws {
            assert_eq!(
                error,
                "Uncaught JavaScript exception: length conversion denied"
            );
        } else {
            assert!(error.contains("TypeError"));
        }
        assert_eq!(runtime.get_global("conversions"), Value::Number(1.0));
        assert_eq!(host.trace, [Event::Length]);
        assert!(report(&runtime).first_rejected.is_none());
    }
}

struct NoIndexed {
    length: Value,
    reads: usize,
}
impl Host for NoIndexed {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        assert_eq!((object, key), ("opaque", "length"));
        self.reads += 1;
        Ok(self.length.clone())
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected set")
    }
    fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unsupported indexed Host must not reach callback")
    }
}

#[test]
fn default_unsupported_host_does_not_probe_empty_or_missing_length() {
    for method in METHODS {
        for length in [
            Value::Undefined,
            Value::Number(0.0),
            Value::Number(f64::INFINITY),
        ] {
            let mut runtime = Runtime::new();
            let mut host = NoIndexed { length, reads: 0 };
            runtime
                .invoke(
                    Value::Native(format!("Array.{method}")),
                    Value::Host("opaque".into()),
                    vec![callback(), Value::Number(7.0)],
                    &mut host,
                )
                .unwrap();
            assert_eq!(host.reads, 1);
        }
        let mut runtime = Runtime::new();
        let mut host = NoIndexed {
            length: Value::Number(1.0),
            reads: 0,
        };
        let error = runtime
            .invoke(
                Value::Native(format!("Array.{method}")),
                Value::Host("opaque".into()),
                vec![callback(), Value::Number(7.0)],
                &mut host,
            )
            .unwrap_err();
        assert_eq!(
            error,
            "Uncaught JavaScript exception: Indexed property inspection is not implemented by this host"
        );
        assert_eq!(host.reads, 1);
        assert_eq!(
            runtime.execute("42;", &mut host).unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn host_has_and_get_distinguish_missing_from_present_undefined_in_order() {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(
        Value::Number(3.0),
        vec![None, Some(Value::Undefined), Some(Value::Number(3.0))],
    );
    call(&mut runtime, &mut host, "forEach", callback(), None).unwrap();
    assert_eq!(
        host.trace,
        [
            Event::Length,
            Event::Has(0),
            Event::Has(1),
            Event::Get(1),
            Event::Call(1),
            Event::Has(2),
            Event::Get(2),
            Event::Call(2)
        ]
    );
    assert_eq!(host.values, [Value::Undefined, Value::Number(3.0)]);
    assert_eq!(host.receivers, [Value::Undefined, Value::Undefined]);
}

#[test]
fn reduce_right_uses_descending_presence_and_exact_four_argument_calls() {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(
        Value::Number(3.0),
        vec![Some(Value::Number(1.0)), None, Some(Value::Number(3.0))],
    );
    host.outcome = Outcome::First;
    assert_eq!(
        call(
            &mut runtime,
            &mut host,
            "reduceRight",
            callback(),
            Some(Value::Number(7.0))
        )
        .unwrap(),
        Value::Number(7.0)
    );
    assert_eq!(
        host.trace,
        [
            Event::Length,
            Event::Has(2),
            Event::Get(2),
            Event::Call(2),
            Event::Has(1),
            Event::Has(0),
            Event::Get(0),
            Event::Call(0)
        ]
    );
    assert_eq!(host.receivers, [Value::Undefined, Value::Undefined]);
}

#[test]
fn host_mutations_are_live_but_length_is_captured_and_filter_does_not_reread() {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(
        Value::Number(3.0),
        vec![Some(Value::Number(1.0)), Some(Value::Number(2.0)), None],
    );
    host.outcome = Outcome::Mutate;
    let result = call(&mut runtime, &mut host, "filter", callback(), None).unwrap();
    assert_eq!(
        host.trace,
        [
            Event::Length,
            Event::Has(0),
            Event::Get(0),
            Event::Call(0),
            Event::Has(1),
            Event::Has(2),
            Event::Get(2),
            Event::Call(2)
        ]
    );
    runtime.set_global("selected", result);
    assert_eq!(
        runtime
            .execute(
                "selected.length===2&&selected[0]===1&&selected[1]===9;",
                &mut host
            )
            .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn presence_get_and_callback_errors_stop_later_work_without_fatal_latching() {
    for stage in ["presence", "get", "callback"] {
        let mut runtime = Runtime::new();
        let mut host = Collection::new(
            Value::Number(3.0),
            vec![
                Some(Value::Number(1.0)),
                Some(Value::Number(2.0)),
                Some(Value::Number(3.0)),
            ],
        );
        if stage == "presence" {
            host.presence_error = Some(1);
        } else if stage == "get" {
            host.get_error = Some(1);
        } else {
            host.outcome = Outcome::Throw;
        }
        let error = call(&mut runtime, &mut host, "map", callback(), None).unwrap_err();
        assert_eq!(
            error,
            format!("Uncaught JavaScript exception: {stage} denied")
        );
        assert!(
            !host
                .trace
                .iter()
                .any(|e| matches!(e, Event::Has(2) | Event::Get(2) | Event::Call(2)))
        );
        assert_eq!(host.calls(), 1);
        assert!(report(&runtime).first_rejected.is_none());
        assert_eq!(
            runtime.execute("42;", &mut host).unwrap(),
            Value::Number(42.0)
        );
    }
}

fn payload_cost(method: &str, length: usize, echo: bool) -> u64 {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(
        Value::Number(1.0),
        vec![Some(Value::String(vec![b'x' as u16; length]))],
    );
    host.outcome = if echo {
        Outcome::First
    } else {
        Outcome::Constant(Value::Bool(true))
    };
    let before = report(&runtime);
    call(&mut runtime, &mut host, method, callback(), None).unwrap();
    assert_eq!(host.calls(), 1);
    runtime_delta(before, report(&runtime))
}

#[test]
fn payload_slopes_preserve_host_ingress_filter_copy_and_return_ingress_only() {
    for (method, echo, bytes) in [
        ("forEach", false, 2),
        ("map", false, 2),
        ("some", false, 2),
        ("every", false, 2),
        ("filter", false, 4),
        ("map", true, 4),
    ] {
        assert_eq!(
            payload_cost(method, 1024, echo) - payload_cost(method, 512, echo),
            512 * bytes,
            "{method}"
        );
    }
}

fn this_cost(length: usize, count: usize) -> u64 {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(
        Value::Number(count as f64),
        vec![Some(Value::Number(1.0)); count],
    );
    let before = report(&runtime);
    call(
        &mut runtime,
        &mut host,
        "forEach",
        callback(),
        Some(Value::String(vec![b't' as u16; length])),
    )
    .unwrap();
    assert_eq!(host.calls(), count);
    assert!(
        host.receivers
            .iter()
            .all(|value| matches!(value,Value::String(units) if units.len()==length))
    );
    runtime_delta(before, report(&runtime))
}

#[test]
fn supplied_this_arg_pays_ingress_and_one_real_copy_per_callback() {
    for count in [0, 1, 3] {
        assert_eq!(
            this_cost(1024, count) - this_cost(512, count),
            512 * 2 * (1 + count as u64)
        );
    }
}

#[test]
fn three_and_four_argument_slots_are_admitted_before_callback_effects() {
    for (method, slots) in [
        ("forEach", 192),
        ("some", 192),
        ("every", 192),
        ("reduce", 256),
        ("reduceRight", 256),
    ] {
        let mut runtime = Runtime::new();
        let mut host = Collection::new(Value::Number(1.0), vec![Some(Value::Number(1.0))]);
        leave(&mut runtime, 200);
        let error = call(
            &mut runtime,
            &mut host,
            method,
            callback(),
            Some(Value::Number(0.0)),
        )
        .unwrap_err();
        let rejected = report(&runtime).first_rejected.unwrap();
        assert_eq!(rejected.phase, AllocationPhase::Runtime);
        assert_eq!(rejected.requested_bytes, slots, "{method}");
        assert_eq!(host.trace, [Event::Length, Event::Has(0), Event::Get(0)]);
        assert_eq!(host.calls(), 0);
        latch(&mut runtime, &error);
    }
}

#[test]
fn map_full_result_preflight_precedes_all_indexed_effects() {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(Value::Number(10_000.0), vec![]);
    leave(&mut runtime, 500_000);
    let error = call(&mut runtime, &mut host, "map", callback(), None).unwrap_err();
    let rejected = report(&runtime).first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes, 640_000);
    assert_eq!(host.trace, [Event::Length]);
    latch(&mut runtime, &error);
}

#[test]
fn filter_growth_failure_preserves_callback_effect_but_returns_no_partial_result() {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(
        Value::Number(2.0),
        vec![Some(Value::Number(1.0)), Some(Value::Number(2.0))],
    );
    leave(&mut runtime, 400);
    let error = call(&mut runtime, &mut host, "filter", callback(), None).unwrap_err();
    let rejected = report(&runtime).first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes, 64);
    assert_eq!(
        host.trace,
        [Event::Length, Event::Has(0), Event::Get(0), Event::Call(0)]
    );
    assert_eq!(host.calls(), 1);
    latch(&mut runtime, &error);
}

#[test]
fn callback_abrupt_completion_preserves_public_assignment_and_original_caught_value() {
    let mut runtime = Runtime::new();
    runtime.set_global("collection", Value::Host("collection".into()));
    runtime.set_global("callback", callback());
    let mut host = Collection::new(
        Value::Number(2.0),
        vec![Some(Value::Number(1.0)), Some(Value::Number(2.0))],
    );
    host.outcome = Outcome::Throw;
    assert_eq!(runtime.execute("var result='sentinel',caught=false,finished=false;try{result=Array.prototype.map.call(collection,callback);}catch(e){caught=e==='callback denied';}finally{finished=true;}result==='sentinel'&&caught&&finished;",&mut host).unwrap(),Value::Bool(true));
    assert_eq!(host.calls(), 1);
    assert!(!host.trace.contains(&Event::Has(1)));
}

#[test]
fn repeated_callbacks_exhaust_cumulative_fuel_without_catch_finally_or_later_work() {
    let mut runtime = Runtime::new();
    let mut host = Collection::new(Value::Number(0.0), vec![]);
    let error=runtime.execute("var rounds=0,caught=false,finished=false;function callback(){rounds++;if(rounds===17){while(true){}}}try{for(var i=0;i<20;i++){[0].forEach(callback);}}catch(e){caught=true;}finally{finished=true;}",&mut host).unwrap_err();
    assert_eq!(error, "JavaScript fuel exhausted");
    assert_eq!(runtime.get_global("rounds"), Value::Number(17.0));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finished"), Value::Bool(false));
    assert!(host.trace.is_empty());
    latch(&mut runtime, &error);
}

#[test]
fn bound_prefix_aggregation_preserves_the_existing_argument_cap() {
    for (method, prefix) in [("forEach", 9998), ("reduce", 9997)] {
        let mut runtime = Runtime::new();
        let mut host = Collection::new(Value::Number(1.0), vec![Some(Value::Number(1.0))]);
        let mut args = vec![Value::Undefined];
        args.extend(std::iter::repeat_n(Value::Number(1.0), prefix));
        let bound = runtime
            .invoke(
                Value::Native("Function.bind".into()),
                callback(),
                args,
                &mut host,
            )
            .unwrap();
        assert!(host.trace.is_empty());
        let error = call(
            &mut runtime,
            &mut host,
            method,
            bound,
            Some(Value::Number(0.0)),
        )
        .unwrap_err();
        assert!(error.contains("argument limit exhausted"), "{error}");
        assert_eq!(host.trace, [Event::Length, Event::Has(0), Event::Get(0)]);
        assert!(report(&runtime).first_rejected.is_none());
        latch(&mut runtime, &error);
    }
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn callback_reentry_is_bounded_on_an_owned_default_stack() {
    const FLAG: &str = "MGBROWSER_ARRAY_CALLBACK_RESOURCE_CHILD";
    if std::env::var_os(FLAG).is_some() {
        for (method, args) in [
            ("forEach", "recurse"),
            ("map", "recurse"),
            ("reduce", "recurse,0"),
        ] {
            let source = format!(
                "var caught=false,finished=false;function recurse(){{[0].{method}({args});}}try{{recurse();}}catch(e){{caught=true;}}finally{{finished=true;}}"
            );
            let mut runtime = Runtime::new();
            let mut host = Collection::new(Value::Number(0.0), vec![]);
            let error = runtime.execute(&source, &mut host).unwrap_err();
            assert!(
                error.contains("depth") && error.contains("exhausted"),
                "{method}: {error}"
            );
            assert_eq!(runtime.get_global("caught"), Value::Bool(false));
            assert_eq!(runtime.get_global("finished"), Value::Bool(false));
            latch(&mut runtime, &error);
        }
        return;
    }
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("callback_reentry_is_bounded_on_an_owned_default_stack")
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
            assert!(status.success(), "default-stack child failed: {status}");
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "bounded callback child timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
