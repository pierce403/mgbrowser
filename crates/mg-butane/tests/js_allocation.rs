//! Independently authored allocation-report checks. These exercise logical
//! accounting, not allocator RSS, using no website source or alternate engine.
//! Exact function-body copy costs are deliberately not contractual assertions:
//! a later ownership improvement must be able to remove a copy it truly avoids.

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

fn phases(report: &AllocationReport) -> [u64; 7] {
    let phases = &report.phases;
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

fn valid(report: &AllocationReport) {
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, LIMIT);
    assert!(report.accepted_bytes <= LIMIT);
    assert_eq!(
        phases(report).into_iter().sum::<u64>(),
        report.accepted_bytes
    );
    if let Some(rejected) = &report.first_rejected {
        assert_eq!(rejected.accepted_bytes, report.accepted_bytes);
        assert_eq!(rejected.limit_bytes, LIMIT);
        assert!(rejected.requested_bytes > LIMIT - report.accepted_bytes);
    }
}

fn monotonic(before: &AllocationReport, after: &AllocationReport) {
    valid(before);
    valid(after);
    assert!(after.accepted_bytes >= before.accepted_bytes);
    for (old, new) in phases(before).into_iter().zip(phases(after)) {
        assert!(new >= old, "{before:?} -> {after:?}");
    }
    assert_eq!(before.phases.bootstrap, after.phases.bootstrap);
}

fn invoke(runtime: &mut Runtime, name: &str, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(
        Value::Native(name.into()),
        Value::Undefined,
        args,
        &mut NoIo,
    )
}

fn assert_latched(runtime: &mut Runtime, error: &str, report: AllocationReport) {
    valid(&report);
    assert!(report.first_rejected.is_some());
    assert_eq!(
        runtime
            .execute("var unexpected_later_effect=1;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(
        runtime.get_global("unexpected_later_effect"),
        Value::Undefined
    );
    assert_eq!(runtime.allocation_report(), report);
    assert_eq!(
        invoke(runtime, "Number", vec![Value::Number(42.0)]).unwrap_err(),
        error
    );
    assert_eq!(runtime.allocation_report(), report);
}

fn reject_runtime_argument(runtime: &mut Runtime) -> String {
    // The argument alone exceeds the unchanged budget. No script or native
    // callback executes, and constructing this local test value needs ~4 MiB.
    let argument = Value::String(vec![b'x' as u16; LIMIT as usize / 2 + 1]);
    let error = invoke(runtime, "Number", vec![argument]).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    error
}

#[test]
fn bootstrap_has_fixed_exclusive_accounting_and_a_small_roundtrippable_report() {
    let runtime = Runtime::new();
    let report = runtime.allocation_report();
    valid(&report);
    assert!(report.accepted_bytes > 0);
    assert_eq!(report.phases.bootstrap, report.accepted_bytes);
    assert_eq!(phases(&report)[1..], [0; 6]);
    assert!(report.first_rejected.is_none());
    assert_eq!(report, runtime.allocation_report());
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.len() < 2048);
    assert_eq!(
        serde_json::from_str::<AllocationReport>(&json).unwrap(),
        report
    );
    assert_eq!(Runtime::new().allocation_report(), report);
}

#[test]
fn successful_phase_totals_are_monotonic_across_scripts_and_report_snapshots() {
    let mut runtime = Runtime::new();
    let initial = runtime.allocation_report();
    let mut previous = initial;
    for source in [
        "1;",
        "var values=[1,2];values.push(3);",
        "function read(){return values[2];}read();",
        "var pattern=/a+/g;pattern.exec('aaa');",
    ] {
        runtime.execute(source, &mut NoIo).unwrap();
        let next = runtime.allocation_report();
        monotonic(&previous, &next);
        assert!(next.accepted_bytes > previous.accepted_bytes);
        assert!(next.phases.source > previous.phases.source);
        assert!(next.phases.ast > previous.phases.ast);
        assert!(next.first_rejected.is_none());
        previous = next;
    }
    assert_eq!(initial, Runtime::new().allocation_report());
}

#[test]
fn comment_only_sources_charge_source_but_no_ast_function_or_runtime_storage() {
    let mut runtime = Runtime::new();
    let before = runtime.allocation_report();
    let source = format!("/*{}*/", "local comment ".repeat(2000));
    assert_eq!(
        runtime.execute(&source, &mut NoIo).unwrap(),
        Value::Undefined
    );
    let after = runtime.allocation_report();
    monotonic(&before, &after);
    assert!(after.phases.source >= source.len() as u64);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.function_code, before.phases.function_code);
    assert_eq!(after.phases.runtime, before.phases.runtime);
    assert_eq!(after.phases.regex_compile, 0);
    assert_eq!(after.phases.regex_result, 0);
}

#[test]
fn ordinary_syntax_failure_charges_attempt_not_successful_ast_or_prefix_effects() {
    let mut runtime = Runtime::new();
    runtime.execute("var marker=0;", &mut NoIo).unwrap();
    let before = runtime.allocation_report();
    let source = "marker=9;function never_constructed(){return 1;}var broken=;";
    let error = runtime.execute(source, &mut NoIo).unwrap_err();
    assert!(error.contains("SyntaxError"), "{error}");
    let after = runtime.allocation_report();
    monotonic(&before, &after);
    assert!(after.phases.source > before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.function_code, before.phases.function_code);
    assert!(after.first_rejected.is_none());
    assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
    assert_eq!(runtime.get_global("never_constructed"), Value::Undefined);
    assert_eq!(
        runtime.execute("marker+42;", &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
    monotonic(&after, &runtime.allocation_report());
}

#[test]
fn large_straight_line_program_attributes_successful_ast_without_function_code() {
    let mut runtime = Runtime::new();
    let source = format!("{}42;", "0;".repeat(4000));
    assert_eq!(
        runtime.execute(&source, &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
    let report = runtime.allocation_report();
    valid(&report);
    assert!(report.phases.source >= source.len() as u64);
    assert!(report.phases.ast > report.phases.source);
    assert_eq!(report.phases.function_code, 0);
    assert_eq!(report.phases.regex_compile, 0);
    assert_eq!(report.phases.regex_result, 0);
    assert!(report.first_rejected.is_none());
}

#[test]
fn function_creation_and_invocation_have_distinct_phase_attribution() {
    let mut runtime = Runtime::new();
    let source = format!(
        "function authored(){{{}return 42;}}authored;",
        "0;".repeat(1000)
    );
    let function = runtime.execute(&source, &mut NoIo).unwrap();
    assert!(matches!(function, Value::Function(_)));
    let compiled = runtime.allocation_report();
    valid(&compiled);
    assert!(compiled.phases.source > 0);
    assert!(compiled.phases.ast > 0);
    assert!(compiled.phases.function_code > 0);
    for _ in 0..2 {
        assert_eq!(
            runtime
                .invoke(function.clone(), Value::Undefined, vec![], &mut NoIo)
                .unwrap(),
            Value::Number(42.0)
        );
    }
    let invoked = runtime.allocation_report();
    monotonic(&compiled, &invoked);
    assert_eq!(invoked.phases.source, compiled.phases.source);
    assert_eq!(invoked.phases.ast, compiled.phases.ast);
    assert_eq!(invoked.phases.function_code, compiled.phases.function_code);
    assert!(invoked.phases.runtime > compiled.phases.runtime);
    // Do not assert a function-code/AST ratio: a genuine sharing or ownership
    // change may avoid a body copy while retaining the same observable code.
}

#[test]
fn large_uncalled_body_succeeds_with_shared_code_under_the_unchanged_cap() {
    let mut runtime = Runtime::new();
    runtime.execute("var marker=0;", &mut NoIo).unwrap();
    let source = format!(
        "marker=9;function retained(){{{}return 42;}}retained;",
        "0;".repeat(10_000)
    );
    let function = runtime.execute(&source, &mut NoIo).unwrap();
    let report = runtime.allocation_report();
    valid(&report);
    assert!(report.phases.source > 0);
    assert!(report.phases.ast > 0);
    assert!(matches!(function, Value::Function(_)));
    assert!(report.first_rejected.is_none());
    assert_eq!(runtime.get_global("marker"), Value::Number(9.0));
    // The parse has returned and its temporary Program has been disposed.
    // Retained code must remain callable without another source/AST charge.
    drop(source);
    assert_eq!(
        runtime
            .invoke(function, Value::Undefined, vec![], &mut NoIo)
            .unwrap(),
        Value::Number(42.0)
    );
    let after = runtime.allocation_report();
    valid(&after);
    assert_eq!(after.phases.source, report.phases.source);
    assert_eq!(after.phases.ast, report.phases.ast);
    assert_eq!(after.phases.function_code, report.phases.function_code);
}

#[test]
fn dynamic_function_compilation_without_an_outer_script_has_source_ast_and_code() {
    let mut runtime = Runtime::new();
    let function = invoke(
        &mut runtime,
        "Function",
        vec![Value::text("x"), Value::text("return x+1;")],
    )
    .unwrap();
    let report = runtime.allocation_report();
    valid(&report);
    assert!(report.phases.source > 0);
    assert!(report.phases.ast > 0);
    assert!(report.phases.function_code > 0);
    assert!(report.phases.runtime > 0);
    assert_eq!(report.phases.regex_compile, 0);
    assert_eq!(report.phases.regex_result, 0);
    assert_eq!(
        runtime
            .invoke(
                function,
                Value::Undefined,
                vec![Value::Number(41.0)],
                &mut NoIo
            )
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn regex_compilation_and_match_results_have_separate_exclusive_phases() {
    let mut runtime = Runtime::new();
    let pattern = invoke(
        &mut runtime,
        "RegExp",
        vec![Value::text("(a)(b+)"), Value::text("g")],
    )
    .unwrap();
    let compiled = runtime.allocation_report();
    valid(&compiled);
    assert!(compiled.phases.regex_compile > 0);
    assert_eq!(compiled.phases.source, 0);
    assert_eq!(compiled.phases.ast, 0);
    assert_eq!(compiled.phases.function_code, 0);
    let matched = runtime
        .invoke(
            Value::Native("RegExp.exec".into()),
            pattern,
            vec![Value::text("abbb")],
            &mut NoIo,
        )
        .unwrap();
    assert!(matches!(matched, Value::Object(_)));
    let after = runtime.allocation_report();
    monotonic(&compiled, &after);
    assert_eq!(after.phases.regex_compile, compiled.phases.regex_compile);
    assert!(after.phases.regex_result > compiled.phases.regex_result);
    assert_eq!(after.phases.source, 0);
    assert_eq!(after.phases.ast, 0);
    assert_eq!(after.phases.function_code, 0);
}

#[test]
fn invalid_regex_attempt_is_charged_without_script_ast_or_successful_match() {
    let mut runtime = Runtime::new();
    let before = runtime.allocation_report();
    let error = invoke(&mut runtime, "RegExp", vec![Value::text("(")]).unwrap_err();
    assert!(error.contains("SyntaxError"), "{error}");
    let after = runtime.allocation_report();
    monotonic(&before, &after);
    assert!(after.phases.regex_compile > 0);
    assert_eq!(after.phases.source, 0);
    assert_eq!(after.phases.ast, 0);
    assert_eq!(after.phases.function_code, 0);
    assert_eq!(after.phases.regex_result, 0);
    assert!(after.first_rejected.is_none());
    assert_eq!(
        invoke(&mut runtime, "Number", vec![Value::Number(42.0)]).unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn first_source_rejection_is_not_added_to_accepted_totals_or_replaced_later() {
    let mut runtime = Runtime::new();
    let source = format!("/*{}*/", "x".repeat(100_000));
    let mut previous = runtime.allocation_report();
    for _ in 0..64 {
        match runtime.execute(&source, &mut NoIo) {
            Ok(Value::Undefined) => {
                let after = runtime.allocation_report();
                monotonic(&previous, &after);
                assert!(after.phases.source > previous.phases.source);
                previous = after;
            }
            Err(error) => {
                assert!(error.contains("allocation budget exhausted"), "{error}");
                let after = runtime.allocation_report();
                valid(&after);
                assert_eq!(after.accepted_bytes, previous.accepted_bytes);
                assert_eq!(after.phases, previous.phases);
                assert_eq!(after.first_rejected.unwrap().phase, AllocationPhase::Source);
                assert_latched(&mut runtime, &error, after);
                return;
            }
            Ok(other) => panic!("Unexpected comment result: {other:?}"),
        }
    }
    panic!("Repeated source attempts did not enforce the unchanged four-MiB cap");
}

#[test]
fn oversized_successful_ast_rejects_before_prefix_effects_and_hoisting() {
    let mut runtime = Runtime::new();
    runtime.execute("var marker=0;", &mut NoIo).unwrap();
    let before = runtime.allocation_report();
    // Fewer than 100,000 source tokens/nodes and shallow AST depth. The logical
    // AST reservation, not parser nesting or fuel, is the intended boundary.
    // Selective boxing lets the old 24k input fit. The 40k input's 65,536-slot
    // root capacity exceeds the same cap under retained-storage accounting.
    let source = format!(
        "marker=9;function untouched(){{return 1;}}{}",
        "0;".repeat(40_000)
    );
    let error = runtime.execute(&source, &mut NoIo).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let after = runtime.allocation_report();
    monotonic(&before, &after);
    assert!(after.phases.source > before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.function_code, before.phases.function_code);
    assert_eq!(after.first_rejected.unwrap().phase, AllocationPhase::Ast);
    assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
    assert_eq!(runtime.get_global("untouched"), Value::Undefined);
    assert_latched(&mut runtime, &error, after);
}

#[test]
fn rejected_invoke_argument_preserves_totals_and_first_runtime_charge() {
    let mut runtime = Runtime::new();
    let before = runtime.allocation_report();
    let error = reject_runtime_argument(&mut runtime);
    let after = runtime.allocation_report();
    valid(&after);
    assert_eq!(after.accepted_bytes, before.accepted_bytes);
    assert_eq!(after.phases, before.phases);
    assert_eq!(
        after.first_rejected.unwrap().phase,
        AllocationPhase::Runtime
    );
    assert_latched(&mut runtime, &error, after);
}

#[test]
fn runtime_array_exhaustion_bypasses_catch_finally_and_later_scripts() {
    let mut runtime = Runtime::new();
    let error = runtime.execute(
        "var caught=false,finalized=false;try{var arrays=[];while(true){arrays.push(new Array(512));}}catch(e){caught=true;}finally{finalized=true;}",
        &mut NoIo,
    ).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    let after = runtime.allocation_report();
    valid(&after);
    assert!(after.phases.runtime > 0);
    assert_eq!(
        after.first_rejected.unwrap().phase,
        AllocationPhase::Runtime
    );
    assert_latched(&mut runtime, &error, after);
}

#[test]
fn repeated_invalid_function_bodies_charge_attempts_not_successful_code() {
    let mut runtime = Runtime::new();
    let body = format!("/*{}*/return;)", "x".repeat(8192));
    let mut previous = runtime.allocation_report();
    let mut ordinary_failures = 0;
    for _ in 0..512 {
        let error = invoke(&mut runtime, "Function", vec![Value::text(&body)]).unwrap_err();
        let after = runtime.allocation_report();
        monotonic(&previous, &after);
        assert_eq!(after.phases.ast, 0);
        assert_eq!(after.phases.function_code, 0);
        if error.contains("allocation budget exhausted") {
            assert!(ordinary_failures > 1);
            assert_latched(&mut runtime, &error, after);
            return;
        }
        assert!(error.contains("SyntaxError"), "{error}");
        assert!(after.phases.source > previous.phases.source);
        assert!(after.first_rejected.is_none());
        ordinary_failures += 1;
        previous = after;
    }
    panic!("Malformed dynamic compilation reset its cumulative allocation budget");
}

#[test]
fn serialized_reports_have_only_fixed_metadata_not_source_names_or_values() {
    let mut runtime = Runtime::new();
    runtime.execute(
        "var private_identifier_7391='https://private.invalid/secret-query-9246';var private_pattern_1628=/secret-query-(9246)/;private_pattern_1628.exec(private_identifier_7391);",
        &mut NoIo,
    ).unwrap();
    for failed in [false, true] {
        if failed {
            reject_runtime_argument(&mut runtime);
        }
        let report = runtime.allocation_report();
        valid(&report);
        let json = serde_json::to_string(&report).unwrap();
        assert!(
            json.len() < 2048,
            "The report must not contain an event history"
        );
        for secret in [
            "private_identifier_7391",
            "private_pattern_1628",
            "private.invalid",
            "secret-query",
        ] {
            assert!(
                !json.contains(secret),
                "Source data leaked into report: {secret}"
            );
        }
        assert_eq!(
            serde_json::from_str::<AllocationReport>(&json).unwrap(),
            report
        );
        let object = serde_json::to_value(report).unwrap();
        let object = object.as_object().unwrap();
        assert_eq!(object.len(), 4);
        for key in object.keys() {
            assert!(
                ["limit_bytes", "accepted_bytes", "phases", "first_rejected"]
                    .contains(&key.as_str())
            );
        }
    }
}

#[test]
fn report_validation_rejects_wrong_caps_overflows_and_inconsistent_rejections() {
    let mut runtime = Runtime::new();
    reject_runtime_argument(&mut runtime);
    let report = runtime.allocation_report();
    valid(&report);
    let mut wrong = report;
    wrong.limit_bytes += 1;
    assert!(!wrong.is_valid());
    let mut wrong = report;
    wrong.accepted_bytes += 1;
    assert!(!wrong.is_valid());
    let mut wrong = report;
    wrong.phases.source = u64::MAX;
    assert!(!wrong.is_valid());
    let mut wrong = report;
    wrong.accepted_bytes = LIMIT + 1;
    assert!(!wrong.is_valid());
    let mut wrong = report;
    wrong.first_rejected.as_mut().unwrap().accepted_bytes += 1;
    assert!(!wrong.is_valid());
    let mut wrong = report;
    wrong.first_rejected.as_mut().unwrap().limit_bytes += 1;
    assert!(!wrong.is_valid());
    let mut wrong = report;
    wrong.first_rejected.as_mut().unwrap().requested_bytes = LIMIT - report.accepted_bytes;
    assert!(!wrong.is_valid());
}

#[test]
fn nested_factory_instances_charge_constant_code_metadata_not_repeated_bodies() {
    let mut growth = Vec::new();
    for body_size in [1, 3000] {
        let mut runtime = Runtime::new();
        let source = format!(
            "function factory(seed){{return function retained(step){{{}return seed+step;}};}}factory;",
            "0;".repeat(body_size)
        );
        let factory = runtime.execute(&source, &mut NoIo).unwrap();
        drop(source);
        let parsed = runtime.allocation_report();
        let mut closures = Vec::new();
        let mut increments = Vec::new();
        for index in 0..8 {
            let before = runtime.allocation_report();
            let closure = runtime
                .invoke(
                    factory.clone(),
                    Value::Undefined,
                    vec![Value::Number(40.0 + index as f64)],
                    &mut NoIo,
                )
                .unwrap();
            assert!(matches!(closure, Value::Function(_)));
            assert!(
                !closures.contains(&closure),
                "Every evaluation creates a fresh function identity"
            );
            closures.push(closure);
            let after = runtime.allocation_report();
            monotonic(&before, &after);
            assert_eq!(after.phases.source, parsed.phases.source);
            assert_eq!(after.phases.ast, parsed.phases.ast);
            let added = after.phases.function_code - before.phases.function_code;
            assert!(
                added > 0 && added < 1024,
                "Closure metadata must not copy its large body: {added}"
            );
            increments.push(added);
        }
        assert!(increments.iter().all(|added| *added == increments[0]));
        growth.push(increments);
        for (index, closure) in closures.into_iter().enumerate() {
            assert_eq!(
                runtime
                    .invoke(
                        closure,
                        Value::Undefined,
                        vec![Value::Number(2.0)],
                        &mut NoIo
                    )
                    .unwrap(),
                Value::Number(42.0 + index as f64)
            );
        }
        valid(&runtime.allocation_report());
    }
    assert_eq!(
        growth[0], growth[1],
        "Per-instance code metadata is independent of shared body size"
    );
}

#[test]
fn shared_nested_code_keeps_captures_properties_prototypes_and_private_self_names() {
    let mut runtime = Runtime::new();
    let source = format!(
        "function factory(seed){{return function PrivateSelf(step){{{}if(step===0)return PrivateSelf;seed+=step;return seed;}};}}var one=factory(1),two=factory(10);factory=undefined;",
        "0;".repeat(1500)
    );
    runtime.execute(&source, &mut NoIo).unwrap();
    drop(source);
    assert_eq!(runtime.execute(
        "one.tag='first';one.prototype.tag='prototype';one!==two&&one.prototype!==two.prototype&&one.prototype.constructor===one&&two.prototype.constructor===two&&typeof two.tag==='undefined'&&typeof two.prototype.tag==='undefined'&&one(2)===3&&two(3)===13&&one(0)===one&&two(0)===two&&one(4)===7&&typeof PrivateSelf==='undefined';",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
    valid(&runtime.allocation_report());
}

#[test]
fn shared_function_slices_survive_eval_and_function_constructor_parse_lifetimes() {
    for constructor in [false, true] {
        let mut runtime = Runtime::new();
        let body = format!(
            "return function retained(step){{{}return seed+step;}};",
            "0;".repeat(3000)
        );
        let factory = if constructor {
            invoke(
                &mut runtime,
                "Function",
                vec![Value::text("seed"), Value::text(&body)],
            )
            .unwrap()
        } else {
            invoke(
                &mut runtime,
                "eval",
                vec![Value::text(&format!("(function(seed){{{body}}});"))],
            )
            .unwrap()
        };
        drop(body);
        let compiled = runtime.allocation_report();
        assert!(compiled.phases.source > 0 && compiled.phases.ast > 0);
        for seed in [40.0, 80.0] {
            let closure = runtime
                .invoke(
                    factory.clone(),
                    Value::Undefined,
                    vec![Value::Number(seed)],
                    &mut NoIo,
                )
                .unwrap();
            assert_eq!(
                runtime
                    .invoke(
                        closure,
                        Value::Undefined,
                        vec![Value::Number(2.0)],
                        &mut NoIo
                    )
                    .unwrap(),
                Value::Number(seed + 2.0)
            );
        }
        let after = runtime.allocation_report();
        monotonic(&compiled, &after);
        assert_eq!(after.phases.source, compiled.phases.source);
        assert_eq!(after.phases.ast, compiled.phases.ast);
    }
}

fn prepaid_string_case(kind: &str, length: usize) -> (Value, u64) {
    let mut runtime = Runtime::new();
    let input = || Value::String(vec![b'a' as u16; length]);
    let (name, this, args) = match kind {
        "String" => ("String", Value::Undefined, vec![input()]),
        "valueOf" => ("String.valueOf", input(), vec![]),
        "concat" => ("String.concat", input(), vec![input()]),
        "substring" => (
            "String.substring",
            input(),
            vec![Value::Number(0.0), Value::Number((length / 2) as f64)],
        ),
        "upper" => ("String.toUpperCase", input(), vec![]),
        "join" => {
            let array = invoke(&mut runtime, "Array", vec![input(), input()]).unwrap();
            ("Array.join", array, vec![Value::text("|")])
        }
        _ => panic!("Unknown authored string case"),
    };
    let before = runtime.allocation_report();
    let value = runtime
        .invoke(Value::Native(name.into()), this, args, &mut NoIo)
        .unwrap();
    let after = runtime.allocation_report();
    monotonic(&before, &after);
    assert_eq!(after.phases.source, 0);
    assert_eq!(after.phases.ast, 0);
    assert_eq!(after.phases.function_code, 0);
    assert_eq!(after.phases.regex_compile, 0);
    assert_eq!(after.phases.regex_result, 0);
    (value, after.phases.runtime - before.phases.runtime)
}

#[test]
fn prepaid_string_output_growth_charges_ingress_and_true_copies_once() {
    // The native String constructor still actually clones its used argument,
    // so its slope includes that copy. valueOf moves a prepaid receiver. concat
    // and join copy into growing output; substring/uppercase allocate outputs.
    for (kind, bytes_per_unit) in [
        ("String", 4),
        ("valueOf", 2),
        ("concat", 6),
        ("substring", 3),
        ("upper", 4),
        ("join", 8),
    ] {
        let (_, small) = prepaid_string_case(kind, 512);
        let (_, large) = prepaid_string_case(kind, 1024);
        assert_eq!(
            large - small,
            512 * bytes_per_unit,
            "{kind}: prepaid output or genuine copy accounting changed"
        );
    }
}

#[test]
fn prepaid_large_string_results_succeed_without_raising_the_realm_cap() {
    for (kind, length) in [
        ("String", 900_000),
        ("valueOf", 1_500_000),
        ("concat", 650_000),
        ("substring", 1_200_000),
        ("upper", 800_000),
        ("join", 200_000),
    ] {
        let (value, _) = prepaid_string_case(kind, length);
        let Value::String(units) = value else {
            panic!("Expected string from {kind}")
        };
        let expected_length = match kind {
            "concat" => length * 2,
            "substring" => length / 2,
            "join" => length * 2 + 1,
            _ => length,
        };
        assert_eq!(units.len(), expected_length, "{kind}");
        if kind == "join" {
            assert_eq!(units[length], b'|' as u16);
            assert!(
                units[..length]
                    .iter()
                    .chain(&units[length + 1..])
                    .all(|unit| *unit == b'a' as u16)
            );
        } else {
            let expected = if kind == "upper" { b'A' } else { b'a' } as u16;
            assert!(units.iter().all(|unit| *unit == expected), "{kind}");
        }
    }
}

#[test]
fn prepaid_string_paths_preserve_coercion_order_utf16_and_ignored_arguments() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.execute(
        "var trace='';function part(tag,value){return {toString:function(){trace+=tag;return value;}};}var concat=String.prototype.concat.call(part('r','ab'),part('a','CD'),part('b','ef'));var order1=trace;trace='';var slice=String.prototype.substring.call(part('r','abcd'),{valueOf:function(){trace+='s';return 1;}},{valueOf:function(){trace+='e';return 3;}});var order2=trace;trace='';var joined=[part('a','A'),null,part('b','B')].join(part('s','|'));var order3=trace;trace='';var converted=String(part('c','value'));concat==='abCDef'&&order1==='rab'&&slice==='bc'&&order2==='rse'&&joined==='A||B'&&order3==='sab'&&converted==='value'&&trace==='c';",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
    assert_eq!(runtime.execute(
        "var text='a\\uD800ß';String(text)===text&&text.valueOf()===text&&text.concat('z')==='a\\uD800ßz'&&text.substring(1,2)==='\\uD800'&&text.toUpperCase()==='A\\uD800SS'&&[text,null,text].join('|')==='a\\uD800ß||a\\uD800ß';",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
    valid(&runtime.allocation_report());

    // Ignored arguments still pay host ingress, but must not acquire an unused
    // deep copy in either native dispatch or the valueOf method wrapper.
    let mut runtime = Runtime::new();
    let before = runtime.allocation_report();
    assert_eq!(
        runtime
            .invoke(
                Value::Native("String.valueOf".into()),
                Value::text("ok"),
                vec![Value::String(vec![b'x' as u16; 1_500_000])],
                &mut NoIo
            )
            .unwrap(),
        Value::text("ok")
    );
    let after = runtime.allocation_report();
    monotonic(&before, &after);
    assert!(after.phases.runtime - before.phases.runtime >= 3_000_000);
    assert!(after.first_rejected.is_none());
}

#[test]
fn remaining_string_ingress_and_used_argument_copies_keep_cumulative_limits_fatal() {
    let mut runtime = Runtime::new();
    let mut previous = runtime.allocation_report();
    let mut successful = 0;
    for _ in 0..32 {
        match invoke(
            &mut runtime,
            "String",
            vec![Value::String(vec![b'x' as u16; 65_536])],
        ) {
            Ok(Value::String(units)) => {
                assert_eq!(units.len(), 65_536);
                successful += 1;
                let after = runtime.allocation_report();
                monotonic(&previous, &after);
                assert!(after.phases.runtime > previous.phases.runtime);
                previous = after;
            }
            Err(error) => {
                assert!(successful > 1);
                assert!(error.contains("allocation budget exhausted"), "{error}");
                let report = runtime.allocation_report();
                assert_eq!(
                    report.first_rejected.unwrap().phase,
                    AllocationPhase::Runtime
                );
                assert_latched(&mut runtime, &error, report);
                return;
            }
            Ok(other) => panic!("Expected String result, got {other:?}"),
        }
    }
    panic!("String ownership changes bypassed cumulative allocation enforcement");
}
