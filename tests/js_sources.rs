//! Independently authored dynamic-source ownership checks. No site source,
//! alternate engine, enlarged stack, or changed allocation cap is used.

use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

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

fn construct(runtime: &mut Runtime, args: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(
        Value::Native("Function".into()),
        Value::Undefined,
        args,
        &mut NoIo,
    )
}

fn parameter_comment(count: usize, comment: &str, name: &str) -> String {
    format!("/*{}*/ {name}", comment.repeat(count))
}

fn latched(runtime: &mut Runtime, first: AllocationReport, error: &str) {
    assert_eq!(
        runtime
            .execute("var later_effect=1;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(runtime.get_global("later_effect"), Value::Undefined);
    assert_eq!(
        construct(runtime, vec![Value::text("return 42;")]).unwrap_err(),
        error
    );
    runtime.set_global("later_ingress", Value::text("not admitted"));
    assert_eq!(runtime.get_global("later_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn compile_report(args: Vec<Value>) -> AllocationReport {
    let mut runtime = Runtime::new();
    assert!(matches!(
        construct(&mut runtime, args).unwrap(),
        Value::Function(_)
    ));
    let allocation = report(&runtime);
    assert!(allocation.first_rejected.is_none());
    allocation
}

fn comment_cost(count: usize, character: &str, fragments: usize) -> AllocationReport {
    let mut args: Vec<Value> = (0..fragments)
        .map(|_| Value::text(&parameter_comment(count, character, "value")))
        .collect();
    args.push(Value::text("return 42;"));
    compile_report(args)
}

#[test]
fn a_large_single_parameter_fragment_compiles_and_remains_callable() {
    let mut runtime = Runtime::new();
    let parameter = parameter_comment(900_000, "x", "value");
    assert_eq!(parameter.len(), 900_010);
    let result = construct(
        &mut runtime,
        vec![Value::text(&parameter), Value::text("return value;")],
    );
    let allocation = report(&runtime);
    eprintln!("Authored 900k parameter-source observation: {allocation:?}");
    let function = result.unwrap();
    assert!(matches!(function, Value::Function(_)));
    assert!(allocation.first_rejected.is_none());
    drop(parameter);
    assert_eq!(
        runtime
            .invoke(
                function,
                Value::Undefined,
                vec![Value::Number(42.0)],
                &mut NoIo
            )
            .unwrap(),
        Value::Number(42.0)
    );
    let called = report(&runtime);
    assert_eq!(called.phases.source, allocation.phases.source);
    assert_eq!(called.phases.ast, allocation.phases.ast);
}

#[test]
fn sole_fragment_moves_but_ingress_and_actual_utf8_conversion_stay_charged() {
    for (character, runtime_per_char, source_per_char) in [("x", 2, 1), ("é", 2, 2), ("😀", 4, 4)]
    {
        let small = comment_cost(512, character, 1);
        let large = comment_cost(1024, character, 1);
        assert_eq!(
            large.phases.runtime - small.phases.runtime,
            512 * runtime_per_char
        );
        assert_eq!(
            large.phases.source - small.phases.source,
            512 * source_per_char
        );
        assert_eq!(large.phases.ast, small.phases.ast);
        assert_eq!(large.phases.function_code, small.phases.function_code);
        assert_eq!(
            large.accepted_bytes - small.accepted_bytes,
            512 * (runtime_per_char + source_per_char)
        );
    }
}

#[test]
fn multiple_parameter_fragments_still_pay_the_real_comma_join_copy() {
    for fragments in [2, 3] {
        let small = comment_cost(512, "x", fragments);
        let large = comment_cost(1024, "x", fragments);
        assert_eq!(
            large.phases.runtime - small.phases.runtime,
            512 * 2 * fragments as u64
        );
        assert_eq!(
            large.phases.source - small.phases.source,
            512 * 3 * fragments as u64
        );
        assert_eq!(large.phases.ast, small.phases.ast);
        assert_eq!(large.phases.function_code, small.phases.function_code);
    }
    let parameters = ["/* first */ a", "/* second */ b"];
    let body = "return a+b;";
    let allocation = compile_report(vec![
        Value::text(parameters[0]),
        Value::text(parameters[1]),
        Value::text(body),
    ]);
    let joined = parameters.join(",");
    // Three converted-argument slots, a newly allocated UTF-16 join, real UTF-8
    // parameter/body buffers, then the unchanged fixed parser-attempt charge.
    assert_eq!(
        allocation.phases.source,
        3 * 32 + 2 * joined.len() as u64 + joined.len() as u64 + body.len() as u64 + 128
    );
    let mut runtime = Runtime::new();
    let function = construct(
        &mut runtime,
        vec![
            Value::text(parameters[0]),
            Value::text(parameters[1]),
            Value::text(body),
        ],
    )
    .unwrap();
    assert_eq!(
        runtime
            .invoke(
                function,
                Value::Undefined,
                vec![Value::Number(20.0), Value::Number(22.0)],
                &mut NoIo
            )
            .unwrap(),
        Value::Number(42.0)
    );
}

#[test]
fn zero_parameters_and_body_only_source_keep_their_existing_costs() {
    let empty = compile_report(vec![]);
    let body_empty = compile_report(vec![Value::text("")]);
    assert_eq!(empty.phases.source, 128);
    assert_eq!(body_empty.phases.source, 32 + 128);
    assert_eq!(empty.phases.ast, body_empty.phases.ast);
    assert_eq!(empty.phases.runtime, body_empty.phases.runtime);
    let body = |count| {
        compile_report(vec![Value::text(&format!(
            "/*{}*/return 42;",
            "x".repeat(count)
        ))])
    };
    let small = body(512);
    let large = body(1024);
    assert_eq!(large.phases.runtime - small.phases.runtime, 512 * 2);
    assert_eq!(large.phases.source - small.phases.source, 512);
    assert_eq!(large.phases.ast, small.phases.ast);
    let mut runtime = Runtime::new();
    let function = construct(&mut runtime, vec![]).unwrap();
    assert_eq!(
        runtime
            .invoke(function, Value::Undefined, vec![], &mut NoIo)
            .unwrap(),
        Value::Undefined
    );
}

#[test]
fn conversions_finish_in_order_before_any_parameter_or_body_grammar_check() {
    for source in [
        "var order='',built=false,marker=0,caught=false;var p={toString:function(){order+='p';return 'bad)';}},b={toString:function(){order+='b';return 'marker=9;return 42;';}};try{Function(p,b);built=true;}catch(e){caught=true;}order==='pb'&&caught&&!built&&marker===0;",
        "var order='',built=false,caught=0;var p={toString:function(){order+='p';return 'bad)';}},b={toString:function(){order+='b';throw 37;}};try{Function(p,b);built=true;}catch(e){caught=e;}order==='pb'&&caught===37&&!built;",
        "var order='',built=false,caught=0;var p={toString:function(){order+='p';throw 23;}},b={toString:function(){order+='b';return 'return 42;';}};try{Function(p,b);built=true;}catch(e){caught=e;}order==='p'&&caught===23&&!built;",
        "var order='',built=false,caught=false;var p={toString:function(){order+='p';return 'bad)';}},q={toString:function(){order+='q';return 'b';}},b={toString:function(){order+='b';return 'return 42;';}};try{Function(p,q,b);built=true;}catch(e){caught=true;}order==='pqb'&&caught&&!built;",
    ] {
        let mut runtime = Runtime::new();
        assert_eq!(
            runtime.execute(source, &mut NoIo).unwrap(),
            Value::Bool(true),
            "{source}"
        );
        assert!(report(&runtime).first_rejected.is_none());
    }
}

#[test]
fn comments_escaped_identifiers_and_utf16_body_values_survive_fragment_ownership() {
    for parameter in [
        "/*é😀*/ value",
        "value// trailing comment",
        "\\u0076alue",
        "/*before*/ value /*after*/",
    ] {
        let mut runtime = Runtime::new();
        let function = construct(
            &mut runtime,
            vec![Value::text(parameter), Value::text("return value;")],
        )
        .unwrap();
        assert_eq!(
            runtime
                .invoke(
                    function,
                    Value::Undefined,
                    vec![Value::Number(42.0)],
                    &mut NoIo
                )
                .unwrap(),
            Value::Number(42.0)
        );
        report(&runtime);
    }
    let mut runtime = Runtime::new();
    let function = construct(
        &mut runtime,
        vec![
            Value::text("/*😀*/ value"),
            Value::text("return '\\uD800😀';"),
        ],
    )
    .unwrap();
    assert_eq!(
        runtime
            .invoke(function, Value::Undefined, vec![], &mut NoIo)
            .unwrap(),
        Value::String(vec![0xd800, 0xd83d, 0xde00])
    );
    report(&runtime);
}

#[test]
fn separate_parameter_and_body_grammar_rejects_before_compiled_effects() {
    for (parameter, body) in [
        ("a) { marker=99; } function inserted(", "return 42;"),
        ("/*", "*/ marker=99;return 42;"),
        ("value,", "marker=99;return 42;"),
        ("class", "marker=99;return 42;"),
        ("value", "marker=99;var broken=;"),
        ("value", "'use strict';marker=99;return 42;"),
        ("value", "marker=99;break;"),
    ] {
        let mut runtime = Runtime::new();
        runtime.set_global("marker", Value::Number(0.0));
        let before = report(&runtime);
        let error = construct(
            &mut runtime,
            vec![Value::text(parameter), Value::text(body)],
        )
        .unwrap_err();
        assert!(error.contains("SyntaxError"), "{error}");
        assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
        let after = report(&runtime);
        assert!(after.phases.source > before.phases.source);
        assert_eq!(after.phases.ast, before.phases.ast);
        assert_eq!(after.phases.function_code, before.phases.function_code);
        assert!(after.first_rejected.is_none());
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn raw_unpaired_source_units_reject_without_replacement_or_fatal_latching() {
    for args in [
        vec![Value::String(vec![0xd800]), Value::text("return 42;")],
        vec![Value::text("value"), Value::String(vec![0xdc00])],
        vec![
            Value::String(vec![0xd800]),
            Value::text("other"),
            Value::text("return 42;"),
        ],
    ] {
        let mut runtime = Runtime::new();
        let error = construct(&mut runtime, args).unwrap_err();
        assert!(error.contains("Unpaired UTF-16"), "{error}");
        let rejected = report(&runtime);
        assert_eq!(rejected.phases.ast, 0);
        assert_eq!(rejected.phases.function_code, 0);
        assert!(rejected.first_rejected.is_none());
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn repeated_successful_compilation_keeps_source_costs_and_first_failure() {
    let mut runtime = Runtime::new();
    let parameter = parameter_comment(700_000, "x", "value");
    let args = || vec![Value::text(&parameter), Value::text("return value;")];
    assert!(matches!(
        construct(&mut runtime, args()).unwrap(),
        Value::Function(_)
    ));
    let first_compile = report(&runtime);
    assert!(first_compile.phases.ast > 0 && first_compile.phases.function_code > 0);
    let error = construct(&mut runtime, args()).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let failed = report(&runtime);
    let charge = failed.first_rejected.unwrap();
    assert_eq!(charge.phase, AllocationPhase::Source);
    assert_eq!(charge.requested_bytes, parameter.len() as u64);
    assert!(failed.accepted_bytes > first_compile.accepted_bytes);
    assert_eq!(failed.phases.ast, first_compile.phases.ast);
    assert_eq!(
        failed.phases.function_code,
        first_compile.phases.function_code
    );
    latched(&mut runtime, failed, &error);
}

#[test]
fn malformed_compilation_attempts_still_charge_source_until_the_realm_latches() {
    let mut runtime = Runtime::new();
    let parameter = parameter_comment(100_000, "x", "value)");
    let mut previous = report(&runtime);
    for attempt in 0..20 {
        let error = construct(
            &mut runtime,
            vec![Value::text(&parameter), Value::text("return 42;")],
        )
        .unwrap_err();
        let after = report(&runtime);
        assert_eq!(after.phases.ast, 0);
        assert_eq!(after.phases.function_code, 0);
        if after.first_rejected.is_some() {
            assert!(attempt > 0);
            assert!(error.contains("allocation budget exhausted"), "{error}");
            latched(&mut runtime, after, &error);
            return;
        }
        assert!(error.contains("SyntaxError"), "{error}");
        assert!(after.phases.source > previous.phases.source);
        assert!(after.accepted_bytes > previous.accepted_bytes);
        previous = after;
    }
    panic!("Repeated invalid construction did not exhaust the unchanged realm cap");
}

#[test]
fn genuine_utf8_source_allocation_failure_bypasses_catch_finally_and_latches() {
    let mut runtime = Runtime::new();
    let parameter = parameter_comment(900_000, "x", "value");
    runtime.set_global("parameter", Value::text(&parameter));
    let error=runtime.execute("var caught=false,finalized=false,constructed=false;try{Function(parameter,'return 42;');constructed=true;}catch(e){caught=true;}finally{finalized=true;}",&mut NoIo).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    for name in ["caught", "finalized", "constructed"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Source);
    assert_eq!(rejected.requested_bytes, parameter.len() as u64);
    // The global payload and its real identifier-read copy stay paid. The
    // actual UTF-8 buffer cannot be admitted, even though no join is allocated.
    assert!(first.phases.runtime >= 3_600_000);
    latched(&mut runtime, first, &error);
}

#[test]
fn source_byte_cap_remains_fatal_after_real_utf8_admission() {
    let mut runtime = Runtime::new();
    let parameter = parameter_comment(1_048_576, "x", "value");
    let error = construct(
        &mut runtime,
        vec![Value::text(&parameter), Value::text("return 42;")],
    )
    .unwrap_err();
    assert!(error.contains("parser limit exhausted"), "{error}");
    assert!(error.contains("Source exceeds one MiB"), "{error}");
    let first = report(&runtime);
    assert!(first.phases.source >= parameter.len() as u64);
    assert_eq!(first.phases.ast, 0);
    assert_eq!(first.phases.function_code, 0);
    assert!(
        first.first_rejected.is_none(),
        "The parser cap, not the heap cap, should reject this case"
    );
    latched(&mut runtime, first, &error);
}
