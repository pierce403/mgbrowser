//! Independent resource/privacy checks for nullish member-fault context.
//! Authored sources only; no website input, alternate engine, changed limits,
//! enlarged test stack or public diagnostic-state API.

use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;
const ORIGINAL: &str = "TypeError: property access on null or undefined";
const CAUGHT: &str =
    "var rounds=0;for(var i=0;i<128;i++){try{null.length;}catch(e){rounds++;}}rounds;";
const FINALLY: &str = "var prior=0;try{null.length;}finally{prior=7;}";
const FUEL: &str = "var rounds=0,ticks=0;for(var i=0;i<128;i++){try{null.length;}catch(e){rounds++;}}while(true){ticks++;}";

struct NoIo;
impl Host for NoIo {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        panic!("diagnostic unexpectedly called Host.get")
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("diagnostic unexpectedly called Host.set")
    }
    fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("diagnostic unexpectedly called Host.call")
    }
}

fn report(runtime: &Runtime) -> AllocationReport {
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, LIMIT);
    report
}

fn phase_tuple(report: AllocationReport) -> [u64; 7] {
    let phases = report.phases;
    [
        phases.bootstrap,
        phases.source,
        phases.ast,
        phases.function_code,
        phases.runtime,
        phases.regex_compile,
        phases.regex_result,
    ]
}

fn exact_report(runtime: &Runtime, accepted: u64, phases: [u64; 7]) {
    let report = report(runtime);
    assert_eq!(report.accepted_bytes, accepted);
    assert_eq!(phase_tuple(report), phases);
    assert!(report.first_rejected.is_none());
}

fn runtime_delta(before: AllocationReport, after: AllocationReport) -> u64 {
    let mut expected = before.phases;
    expected.runtime = after.phases.runtime;
    assert_eq!(expected, after.phases);
    assert!(before.first_rejected.is_none() && after.first_rejected.is_none());
    let delta = after.phases.runtime - before.phases.runtime;
    assert_eq!(after.accepted_bytes - before.accepted_bytes, delta);
    delta
}

// Retain every old exact member block, source, phase and fuel assertion. The
// adopted additive suffix has an independently specified immediate producer.
fn annotated(error: &str, operation: &str, base: &str, key: &str, producer: &str) {
    assert_eq!(
        error,
        format!(
            "Uncaught JavaScript exception: {ORIGINAL} [member operation={operation} base={base} key={key}] [producer kind={producer}]"
        )
    );
    assert!(error.is_ascii());
    assert!(error.len() <= 256, "diagnostic exceeded fixed byte bound");
}

fn function(runtime: &mut Runtime, body: &str) -> Value {
    runtime
        .execute(&format!("(function(k){{{body}}});"), &mut NoIo)
        .unwrap()
}

fn invoke(runtime: &mut Runtime, function: Value, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(function, Value::Undefined, args, &mut NoIo)
}

fn leave_budget(runtime: &mut Runtime, remaining: u64) {
    runtime.set_global("padding", Value::String(vec![]));
    let available = LIMIT - report(runtime).accepted_bytes;
    assert!(available > remaining + 2);
    runtime.set_global(
        "padding",
        Value::String(vec![b'p' as u16; ((available - remaining) / 2) as usize]),
    );
    let report = report(runtime);
    assert!(report.first_rejected.is_none());
    assert!((remaining..=remaining + 1).contains(&(LIMIT - report.accepted_bytes)));
}

fn latch(runtime: &mut Runtime, error: &str, first: AllocationReport) {
    assert!(!error.contains("[member "), "fatal was annotated: {error}");
    assert_eq!(
        runtime
            .execute("var forbidden=true;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(runtime.get_global("forbidden"), Value::Undefined);
    assert_eq!(
        invoke(runtime, Value::Native("Number".into()), vec![]).unwrap_err(),
        error
    );
    runtime.set_global("forbidden", Value::Bool(true));
    assert_eq!(runtime.get_global("forbidden"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

#[test]
fn bootstrap_and_diagnostic_metadata_add_no_realm_storage() {
    let runtime = Runtime::new();
    exact_report(&runtime, 25_999 + 156, [25_999 + 156, 0, 0, 0, 0, 0, 0]);
    assert_eq!(report(&runtime), report(&Runtime::new()));
}

#[test]
fn fixed_whitelist_and_equal_length_redactions_have_identical_admitted_costs() {
    // This is the adopted public vocabulary, not names read from production.
    let keys = [
        "prototype",
        "constructor",
        "length",
        "name",
        "message",
        "call",
        "apply",
        "bind",
        "toString",
        "valueOf",
        "forEach",
        "map",
        "filter",
        "some",
        "every",
        "reduce",
        "push",
        "pop",
        "shift",
        "unshift",
        "slice",
        "join",
        "concat",
        "indexOf",
        "includes",
        "reverse",
        "appendChild",
        "removeChild",
        "insertBefore",
        "remove",
        "addEventListener",
        "removeEventListener",
        "querySelector",
        "querySelectorAll",
        "getElementById",
        "getElementsByTagName",
        "getElementsByClassName",
        "createElement",
        "createTextNode",
        "setAttribute",
        "getAttribute",
        "hasAttribute",
        "removeAttribute",
        "textContent",
        "innerHTML",
        "innerText",
        "style",
        "classList",
        "className",
        "id",
        "parentNode",
        "parentElement",
        "firstChild",
        "lastChild",
        "nextSibling",
        "previousSibling",
        "ownerDocument",
        "documentElement",
        "head",
        "body",
        "children",
        "childNodes",
        "forms",
        "elements",
        "document",
        "navigator",
        "location",
        "href",
        "search",
        "cookie",
        "userAgent",
        "getComputedStyle",
        "onload",
        "onclick",
        "submit",
        "focus",
    ];
    for key in keys {
        let mut results = Vec::new();
        for (units, label) in [
            (key.encode_utf16().collect(), key),
            (vec![b'x' as u16; key.len()], "<string>"),
        ] {
            let mut runtime = Runtime::new();
            let reader = function(&mut runtime, "null[k];");
            let before = report(&runtime);
            let error = invoke(&mut runtime, reader, vec![Value::String(units)]).unwrap_err();
            annotated(&error, "resolve-read", "null", label, "expression");
            results.push((runtime_delta(before, report(&runtime)), report(&runtime)));
        }
        assert_eq!(results[0], results[1], "whitelist scan charged for {key}");
    }
}

#[test]
fn all_origin_operations_keep_large_arbitrary_keys_out_of_bounded_output() {
    for (body, operation) in [
        ("null[k];", "resolve-read"),
        ("null[k]=7;", "resolve-write-target"),
        ("null[k]+=7;", "resolve-compound-target"),
        ("null[k]++;", "resolve-update-target"),
        ("delete null[k];", "resolve-delete-target"),
        ("null[k]();", "resolve-call-target"),
        ("for(null[k] in {x:1}){}", "resolve-for-in-target"),
    ] {
        let mut runtime = Runtime::new();
        let target = function(&mut runtime, body);
        let units =
            "private-authored-key\r\n [member forged] https://fixture.invalid/".repeat(1000);
        let error = invoke(&mut runtime, target, vec![Value::text(&units)]).unwrap_err();
        annotated(&error, operation, "null", "<string>", "expression");
        assert!(!error.contains("private-authored") && !error.contains("fixture.invalid"));
        assert!(report(&runtime).first_rejected.is_none());
    }
}

fn string_cost(length: usize, fail: bool) -> u64 {
    let mut runtime = Runtime::new();
    let reader = function(&mut runtime, if fail { "null[k];" } else { "return k;" });
    let before = report(&runtime);
    let outcome = invoke(
        &mut runtime,
        reader,
        vec![Value::String(vec![0xd800; length])],
    );
    if fail {
        annotated(
            &outcome.unwrap_err(),
            "resolve-read",
            "null",
            "<string>",
            "expression",
        );
    } else {
        assert_eq!(outcome.unwrap(), Value::String(vec![0xd800; length]));
    }
    runtime_delta(before, report(&runtime))
}

#[test]
fn huge_utf16_keys_pay_only_existing_ingress_formal_and_identifier_copies() {
    for length in [0, 512, 1024, 600_000] {
        assert_eq!(string_cost(length, true), string_cost(length, false));
    }
    // Public ingress, independent formal copy, then the actual identifier read.
    assert_eq!(string_cost(1024, true) - string_cost(512, true), 512 * 6);
}

fn opaque_cost(length: usize, native: bool, fail: bool) -> u64 {
    let mut runtime = Runtime::new();
    let reader = function(&mut runtime, if fail { "null[k];" } else { "return k;" });
    let before = report(&runtime);
    let name = "z".repeat(length);
    let key = if native {
        Value::Native(name)
    } else {
        Value::Host(name)
    };
    let outcome = invoke(&mut runtime, reader, vec![key.clone()]);
    if fail {
        annotated(
            &outcome.unwrap_err(),
            "resolve-read",
            "null",
            if native { "<native>" } else { "<host>" },
            "expression",
        );
    } else {
        assert_eq!(outcome.unwrap(), key);
    }
    runtime_delta(before, report(&runtime))
}

#[test]
fn long_native_and_host_keys_are_not_formatted_invoked_or_copied_for_context() {
    for native in [false, true] {
        for length in [512, 1024, 400_000] {
            assert_eq!(
                opaque_cost(length, native, true),
                opaque_cost(length, native, false)
            );
        }
        assert_eq!(
            opaque_cost(1024, native, true) - opaque_cost(512, native, true),
            512 * 3
        );
    }
}

#[test]
fn scalar_symbol_and_object_categories_never_request_key_coercion() {
    let mut runtime = Runtime::new();
    let reader = function(&mut runtime, "undefined[k];");
    let symbol = runtime
        .invoke(
            Value::Native("Symbol".into()),
            Value::Undefined,
            vec![Value::String(vec![0xdfff; 100_000])],
            &mut NoIo,
        )
        .unwrap();
    let object = runtime.execute(
        "var coerced=0;var key={toString:function(){coerced++;throw 1;},valueOf:function(){coerced++;throw 2;}};key[Symbol.toPrimitive]=function(){coerced++;throw 3;};key;",
        &mut NoIo,
    ).unwrap();
    let callable = function(&mut runtime, "throw 4;");
    for (key, category) in [
        (Value::Number(f64::NAN), "<number>"),
        (Value::Number(f64::INFINITY), "<number>"),
        (Value::Number(-123_456.0), "<number>"),
        (Value::Bool(true), "<boolean>"),
        (Value::Null, "<null>"),
        (Value::Undefined, "<undefined>"),
        (symbol, "<symbol>"),
        (object, "<object>"),
        (callable, "<function>"),
    ] {
        let error = invoke(&mut runtime, reader.clone(), vec![key]).unwrap_err();
        annotated(&error, "resolve-read", "undefined", category, "binding");
        assert_eq!(runtime.get_global("coerced"), Value::Number(0.0));
    }
}

#[test]
fn repeated_uncaught_diagnostics_do_not_accumulate_realm_context_or_taint_success() {
    let mut runtime = Runtime::new();
    let reader = function(&mut runtime, "null[k];");
    let successful = function(&mut runtime, "return 42;");
    let mut previous_cost = None;
    for _ in 0..64 {
        let before = report(&runtime);
        let error = invoke(&mut runtime, reader.clone(), vec![Value::Null]).unwrap_err();
        annotated(&error, "resolve-read", "null", "<null>", "expression");
        let cost = runtime_delta(before, report(&runtime));
        if let Some(expected) = previous_cost {
            assert_eq!(cost, expected);
        }
        previous_cost = Some(cost);
        assert_eq!(
            invoke(&mut runtime, successful.clone(), vec![]).unwrap(),
            Value::Number(42.0)
        );
    }
    assert_eq!(
        runtime.execute("throw 'later';", &mut NoIo).unwrap_err(),
        "Uncaught JavaScript exception: later"
    );
    assert!(report(&runtime).first_rejected.is_none());
}

// These exact sources/totals were frozen before production changes against the
// published library: tmp/diagnostic-resource-baseline-probe.rs, source SHA256
// 2b073c087d143aba56518f1f6828e0023dfcb9c5e59dfb4444cb77c46ef60333.
// The original public-only probe passed once in91ms, including no heap-first
// failure before the fuel checkpoint. The later real reduceRight property adds
// only 156 measured Bootstrap bytes; sources, other phases and fuel ticks remain.
#[test]
fn repeated_caught_faults_preserve_the_frozen_exclusive_phase_totals() {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime.execute(CAUGHT, &mut NoIo).unwrap(),
        Value::Number(128.0)
    );
    exact_report(
        &runtime,
        76_200 + 156,
        [25_999 + 156, 208, 3195, 0, 46_798, 0, 0],
    );
    let error = runtime
        .execute(
            "var saved;try{null.length;}catch(e){saved=e;}throw saved;",
            &mut NoIo,
        )
        .unwrap_err();
    assert_eq!(runtime.get_global("saved"), Value::text(ORIGINAL));
    assert_eq!(error, format!("Uncaught JavaScript exception: {ORIGINAL}"));
}

#[test]
fn normal_finally_keeps_context_but_preserves_the_frozen_phase_totals() {
    let mut runtime = Runtime::new();
    annotated(
        &runtime.execute(FINALLY, &mut NoIo).unwrap_err(),
        "resolve-read",
        "null",
        "length",
        "expression",
    );
    assert_eq!(runtime.get_global("prior"), Value::Number(7.0));
    exact_report(
        &runtime,
        28_270 + 156,
        [25_999 + 156, 174, 1915, 0, 182, 0, 0],
    );
}

#[test]
fn fixed_caught_faults_do_not_move_the_frozen_fuel_exhaustion_checkpoint() {
    let mut runtime = Runtime::new();
    let error = runtime.execute(FUEL, &mut NoIo).unwrap_err();
    assert_eq!(error, "JavaScript fuel exhausted");
    assert_eq!(runtime.get_global("rounds"), Value::Number(128.0));
    assert_eq!(runtime.get_global("ticks"), Value::Number(21_462.0));
    exact_report(
        &runtime,
        76_934 + 156,
        [25_999 + 156, 230, 3737, 0, 46_968, 0, 0],
    );
    let first = report(&runtime);
    latch(&mut runtime, &error, first);
}

#[test]
fn public_empty_invoke_preserves_both_frozen_reports_and_the_265_byte_call() {
    let mut runtime = Runtime::new();
    let target = runtime
        .execute("(function(){null[void 0];});", &mut NoIo)
        .unwrap();
    exact_report(
        &runtime,
        27_264 + 156,
        [25_999 + 156, 156, 716, 128, 265, 0, 0],
    );
    let before = report(&runtime);
    annotated(
        &invoke(&mut runtime, target, vec![]).unwrap_err(),
        "resolve-read",
        "null",
        "<undefined>",
        "expression",
    );
    exact_report(
        &runtime,
        27_529 + 156,
        [25_999 + 156, 156, 716, 128, 530, 0, 0],
    );
    assert_eq!(runtime_delta(before, report(&runtime)), 265);
}

#[test]
fn annotation_can_escape_near_the_heap_cap_without_any_new_admission() {
    let mut runtime = Runtime::new();
    let target = runtime
        .execute("(function(){null[void 0];});", &mut NoIo)
        .unwrap();
    leave_budget(&mut runtime, 265);
    let before = report(&runtime);
    annotated(
        &invoke(&mut runtime, target, vec![]).unwrap_err(),
        "resolve-read",
        "null",
        "<undefined>",
        "expression",
    );
    assert_eq!(runtime_delta(before, report(&runtime)), 265);
    assert!(LIMIT - report(&runtime).accepted_bytes <= 1);
    let error = runtime.execute("42;", &mut NoIo).unwrap_err();
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Source);
    assert_eq!(rejected.requested_bytes, 131);
    latch(&mut runtime, &error, first);
}

#[test]
fn real_key_copy_failure_stays_fatal_before_annotation_catch_or_finally() {
    let mut runtime = Runtime::new();
    runtime
        .execute("var caught=false,finished=false;", &mut NoIo)
        .unwrap();
    let target = function(
        &mut runtime,
        "try{null[k];}catch(e){caught=true;}finally{finished=true;}",
    );
    let error = invoke(
        &mut runtime,
        target,
        vec![Value::String(vec![0xd800; 800_000])],
    )
    .unwrap_err();
    assert!(error.starts_with("JavaScript allocation budget exhausted"));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finished"), Value::Bool(false));
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes, 1_600_000);
    latch(&mut runtime, &error, first);
}

#[test]
fn fatal_finally_override_discards_pending_context_and_cannot_resume_later() {
    let mut runtime = Runtime::new();
    let error = runtime.execute(
        "var caught=false,finished=false;try{try{null.length;}finally{while(true){}}}catch(e){caught=true;}finally{finished=true;}",
        &mut NoIo,
    ).unwrap_err();
    assert_eq!(error, "JavaScript fuel exhausted");
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finished"), Value::Bool(false));
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn pending_fault_and_mixed_expression_recursion_stay_bounded_on_default_stack() {
    const CHILD: &str = "MGBROWSER_DIAGNOSTIC_RESOURCE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        for (pending, unary) in [(1, 8), (1, 32), (1, 96), (16, 8)] {
            let mut body = format!("return {}recurse();", "+ ".repeat(unary));
            for _ in 0..pending {
                body = format!("try{{null.length;}}finally{{{body}}}");
            }
            let source = format!(
                "var caught=false,finished=false;function recurse(){{{body}}}try{{recurse();}}catch(e){{caught=true;}}finally{{finished=true;}}"
            );
            let mut runtime = Runtime::new();
            let error = runtime.execute(&source, &mut NoIo).unwrap_err();
            assert!(
                error.contains("evaluation depth limit exhausted"),
                "pending={pending} unary={unary}: {error}"
            );
            assert_eq!(runtime.get_global("caught"), Value::Bool(false));
            assert_eq!(runtime.get_global("finished"), Value::Bool(false));
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
                "pending_fault_and_mixed_expression_recursion_stay_bounded_on_default_stack",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove("RUST_MIN_STACK")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start only the owned diagnostic resource child"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "owned diagnostic child failed: {status}");
            break;
        }
        assert!(Instant::now() < deadline, "owned diagnostic child deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
