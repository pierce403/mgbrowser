//! Independent storage/resource controls for canonical static operator spellings.
//! Authored before candidate execution; no website input or changed resource cap.
//! Exact operator savings belong to the separate old/new allocator measurement.

use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};
use mg_deps::js::{Expr, Stmt, syntax};

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
    let value = runtime.allocation_report();
    assert!(value.is_valid(), "{value:?}");
    assert_eq!(value.limit_bytes, LIMIT);
    value
}

fn native(runtime: &mut Runtime, name: &str, arguments: Vec<Value>) -> Result<Value, String> {
    runtime.invoke(
        Value::Native(name.into()),
        Value::Undefined,
        arguments,
        &mut NoIo,
    )
}

fn latch(runtime: &mut Runtime, first: AllocationReport, error: &str) {
    assert_eq!(
        runtime.execute("var later=1;", &mut NoIo).unwrap_err(),
        error
    );
    assert_eq!(native(runtime, "Number", vec![]).unwrap_err(), error);
    runtime.set_global("later", Value::Number(2.0));
    assert_eq!(runtime.get_global("later"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn unchanged_code(before: AllocationReport, after: AllocationReport) {
    assert_eq!(after.phases.bootstrap, before.phases.bootstrap);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(after.phases.function_code, before.phases.function_code);
    assert!(after.first_rejected.is_none());
}

#[test]
fn static_spellings_do_not_change_the_measured_bootstrap_or_caps() {
    let value = report(&Runtime::new());
    assert_eq!(value.accepted_bytes, 26_880);
    assert_eq!(value.phases.bootstrap, 26_880);
    assert_eq!(
        value.phases.source
            + value.phases.ast
            + value.phases.function_code
            + value.phases.runtime
            + value.phases.regex_compile
            + value.phases.regex_result,
        0
    );
    assert!(value.first_rejected.is_none());
}

#[test]
fn generated_14500_binary_statements_compile_and_really_execute() {
    // Same generation count and shallow operator workload as the frozen actual
    // worker form, with an ordinary realm checkpoint instead of DOM capability.
    let mut runtime = Runtime::new();
    let value = runtime.execute(
        "var reached=0;Function(Array(7251).join('0+0;0+0;')+'reached=42;')();reached;",
        &mut NoIo,
    );
    eprintln!(
        "Authored14,500 operator dynamic Function: {:?}",
        report(&runtime)
    );
    assert_eq!(value.unwrap(), Value::Number(42.0));
    assert_eq!(runtime.get_global("reached"), Value::Number(42.0));
    assert!(report(&runtime).first_rejected.is_none());
}

fn compile(runtime: &mut Runtime, mode: usize, source: &str) -> Value {
    match mode {
        0 => runtime.execute(source, &mut NoIo).unwrap(),
        1 => native(runtime, "eval", vec![Value::text(source)]).unwrap(),
        2 => native(runtime, "Function", vec![Value::text(source)]).unwrap(),
        _ => unreachable!(),
    }
}

#[test]
fn separate_execute_eval_and_function_parses_each_pay_for_retained_storage() {
    for mode in 0..3 {
        let body = format!("{}return 42;", "0+0;".repeat(256));
        let source = if mode == 2 {
            body
        } else {
            format!("(function(){{{body}}});")
        };
        let mut runtime = Runtime::new();
        let before = report(&runtime);
        let first = compile(&mut runtime, mode, &source);
        let once = report(&runtime);
        let second = compile(&mut runtime, mode, &source);
        let twice = report(&runtime);
        assert_ne!(first, second);
        assert!(once.phases.ast > before.phases.ast);
        assert_eq!(
            once.phases.ast - before.phases.ast,
            twice.phases.ast - once.phases.ast
        );
        assert_eq!(
            once.phases.source - before.phases.source,
            twice.phases.source - once.phases.source
        );
        assert_eq!(
            once.phases.function_code - before.phases.function_code,
            twice.phases.function_code - once.phases.function_code
        );
        drop(source);
        for function in [first, second] {
            assert_eq!(
                runtime
                    .invoke(function, Value::Undefined, vec![], &mut NoIo)
                    .unwrap(),
                Value::Number(42.0)
            );
        }
        unchanged_code(twice, report(&runtime));
    }
}

#[test]
fn retained_operator_code_shares_storage_but_not_closure_state() {
    let source = format!(
        "(function(seed){{return function(delta){{{}seed+=delta;return seed;}};}});",
        "0+0;".repeat(512),
    );
    let mut runtime = Runtime::new();
    let factory = runtime.execute(&source, &mut NoIo).unwrap();
    drop(source);
    let parsed = report(&runtime);
    let left = runtime
        .invoke(
            factory.clone(),
            Value::Undefined,
            vec![Value::Number(10.0)],
            &mut NoIo,
        )
        .unwrap();
    let right = runtime
        .invoke(
            factory,
            Value::Undefined,
            vec![Value::Number(100.0)],
            &mut NoIo,
        )
        .unwrap();
    assert_ne!(left, right);
    let created = report(&runtime);
    assert_eq!(created.phases.ast, parsed.phases.ast);
    assert_eq!(created.phases.source, parsed.phases.source);
    // Each anonymous child retains its existing fresh Code allowance, not a
    // new admission of the already shared operator-rich function body.
    assert_eq!(
        created.phases.function_code - parsed.phases.function_code,
        2 * 128
    );
    for (function, increment, expected) in [
        (left.clone(), 2.0, 12.0),
        (right.clone(), 3.0, 103.0),
        (left, 4.0, 16.0),
        (right, 5.0, 108.0),
    ] {
        assert_eq!(
            runtime
                .invoke(
                    function,
                    Value::Undefined,
                    vec![Value::Number(increment)],
                    &mut NoIo
                )
                .unwrap(),
            Value::Number(expected)
        );
    }
    unchanged_code(created, report(&runtime));
}

fn owned_buffer_case(kind: usize, length: usize) -> (u64, u64) {
    let text = "x".repeat(length);
    let source = match kind {
        0 => format!("function kept({text}){{return {text};}}"),
        1 => format!("function kept(){{return {{{text}:0}};}}"),
        2 => format!("function kept(){{return '{text}';}}"),
        3 => format!("function kept(){{return /{text}/g;}}"),
        _ => unreachable!(),
    };
    let parsed = syntax::parse(&source).unwrap();
    let Stmt::Function { params, body, .. } = &parsed.0[0] else {
        panic!("function expected")
    };
    let Stmt::Return(Some(value)) = &body[0] else {
        panic!("return expected")
    };
    let owned = match (kind, value) {
        (0, Expr::Ident(name)) => params[0].capacity() + name.capacity(),
        (1, Expr::Object(values)) => values[0].0.capacity(),
        (2, Expr::String(units)) => 2 * units.capacity(),
        (3, Expr::RegExp { pattern, flags }) => 2 * pattern.capacity() + flags.capacity(),
        _ => panic!("unexpected owned field"),
    };
    drop(parsed);
    let mut runtime = Runtime::new();
    runtime.execute(&source, &mut NoIo).unwrap();
    (owned as u64, report(&runtime).phases.ast)
}

#[test]
fn names_properties_utf16_literals_and_regex_buffers_remain_owned_and_charged() {
    for kind in 0..4 {
        let (small_buffer, small_ast) = owned_buffer_case(kind, 129);
        let (large_buffer, large_ast) = owned_buffer_case(kind, 513);
        assert!(large_buffer > small_buffer);
        assert_eq!(
            large_ast - small_ast,
            large_buffer - small_buffer,
            "owned field kind={kind}"
        );
    }
}

#[test]
fn dynamic_source_ingress_and_real_utf8_buffer_charges_are_not_operator_credits() {
    let measure = |length| {
        let source = format!("/*{}*/return 1+2;", "x".repeat(length));
        let mut runtime = Runtime::new();
        let function = native(&mut runtime, "Function", vec![Value::text(&source)]).unwrap();
        let compiled = report(&runtime);
        assert_eq!(
            runtime
                .invoke(function, Value::Undefined, vec![], &mut NoIo)
                .unwrap(),
            Value::Number(3.0)
        );
        unchanged_code(compiled, report(&runtime));
        compiled
    };
    let small = measure(512);
    let large = measure(1024);
    assert_eq!(large.phases.runtime - small.phases.runtime, 2 * 512);
    assert_eq!(large.phases.source - small.phases.source, 512);
    assert_eq!(large.phases.ast, small.phases.ast);
    assert_eq!(large.phases.function_code, small.phases.function_code);
    assert_eq!(large.accepted_bytes - small.accepted_bytes, 3 * 512);
}

#[test]
fn a_real_large_identifier_read_still_rejects_and_bypasses_finally() {
    let mut runtime = Runtime::new();
    let function = runtime.execute(
        "var started=0,cleaned=0;(function(value){started=1;try{return value;}finally{cleaned=1;}});",
        &mut NoIo,
    ).unwrap();
    let error = runtime
        .invoke(
            function,
            Value::Undefined,
            vec![Value::String(vec![b'x' as u16; 900_000])],
            &mut NoIo,
        )
        .unwrap_err();
    assert!(error.starts_with("JavaScript allocation budget exhausted"));
    assert_eq!(runtime.get_global("started"), Value::Number(1.0));
    assert_eq!(runtime.get_global("cleaned"), Value::Number(0.0));
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes, 1_800_000);
    latch(&mut runtime, first, &error);
}

#[test]
fn real_expression_boxes_still_exhaust_ast_admission_before_function_body_effects() {
    let mut runtime = Runtime::new();
    runtime.set_global("reached", Value::Number(0.0));
    let body = format!("{}reached=1;", "0+0;".repeat(20_000));
    let error = native(&mut runtime, "Function", vec![Value::text(&body)]).unwrap_err();
    assert!(error.starts_with("JavaScript allocation budget exhausted"));
    assert_eq!(runtime.get_global("reached"), Value::Number(0.0));
    let first = report(&runtime);
    assert_eq!(first.phases.ast, 0);
    assert_eq!(first.phases.function_code, 0);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Ast);
    assert!(rejected.requested_bytes > LIMIT);
    latch(&mut runtime, first, &error);
}

#[test]
fn repeated_discarded_function_values_do_not_refund_real_compilation_storage() {
    let body = format!("{}return 7;", "0+0;".repeat(512));
    let mut runtime = Runtime::new();
    let mut previous = report(&runtime);
    let mut successful = 0;
    for _ in 0..128 {
        match native(&mut runtime, "Function", vec![Value::text(&body)]) {
            Ok(function) => {
                assert!(matches!(function, Value::Function(_)));
                let next = report(&runtime);
                assert!(next.phases.ast > previous.phases.ast);
                assert!(next.phases.source > previous.phases.source);
                assert!(next.phases.function_code > previous.phases.function_code);
                assert!(next.accepted_bytes > previous.accepted_bytes);
                previous = next;
                successful += 1;
                drop(function);
            }
            Err(error) => {
                assert!(
                    successful >= 2,
                    "no repeated successful compilation checkpoint"
                );
                assert!(
                    error.starts_with("JavaScript allocation budget exhausted"),
                    "{error}"
                );
                let first = report(&runtime);
                assert!(first.first_rejected.is_some());
                assert!(first.accepted_bytes >= previous.accepted_bytes);
                latch(&mut runtime, first, &error);
                return;
            }
        }
    }
    panic!("real retained compilation storage unexpectedly escaped the fixed cap");
}

#[test]
fn sparse_operator_free_containers_keep_capacity_and_holes() {
    let source = format!("(function(){{return [{}];}});", ",".repeat(10_000));
    let parsed = syntax::parse(&source).unwrap();
    let Stmt::Expr(Expr::Function { body, .. }) = &parsed.0[0] else {
        panic!("function expected")
    };
    let Stmt::Return(Some(Expr::Array(items))) = &body[0] else {
        panic!("array expected")
    };
    assert_eq!(items.len(), 10_000);
    assert!(items.iter().all(Option::is_none));
    let backing = items.capacity() * std::mem::size_of::<Option<Expr>>();
    drop(parsed);
    let mut runtime = Runtime::new();
    let function = runtime.execute(&source, &mut NoIo).unwrap();
    let compiled = report(&runtime);
    assert!(compiled.phases.ast >= backing as u64);
    let array = runtime
        .invoke(function, Value::Undefined, vec![], &mut NoIo)
        .unwrap();
    unchanged_code(compiled, report(&runtime));
    runtime.set_global("holes", array);
    assert_eq!(
        runtime
            .execute(
                "holes.length===10000 && !(0 in holes) && !(9999 in holes);",
                &mut NoIo
            )
            .unwrap(),
        Value::Bool(true)
    );
}
