//! Authored AST storage/accounting regressions. These exercise public parser
//! containers and runtime reports, not a second engine or website source.

use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};
use mg_butane::{Expr, ForInBinding, Stmt, SwitchCase, syntax};

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

fn latched(runtime: &mut Runtime, first: AllocationReport, error: &str) {
    assert_eq!(
        runtime
            .execute("var later_effect=1;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(runtime.get_global("later_effect"), Value::Undefined);
    assert_eq!(
        runtime
            .invoke(
                Value::Native("Function".into()),
                Value::Undefined,
                vec![Value::text("return 42;")],
                &mut NoIo
            )
            .unwrap_err(),
        error
    );
    assert_eq!(report(runtime), first);
}

fn array_function(length: usize, holes: bool) -> String {
    let elements = if holes {
        ",".repeat(length)
    } else {
        vec!["0"; length].join(",")
    };
    format!("function retained(){{return [{elements}];}}")
}

fn array_capacity_and_ast(length: usize, holes: bool) -> (usize, u64) {
    let source = array_function(length, holes);
    let program = syntax::parse(&source).unwrap();
    let Stmt::Function { body, .. } = &program.0[0] else {
        panic!("Expected function");
    };
    let Stmt::Return(Some(Expr::Array(items))) = &body[0] else {
        panic!("Expected array");
    };
    assert_eq!(items.len(), length);
    let capacity = items.capacity();
    drop(program);
    let mut runtime = Runtime::new();
    runtime.execute(&source, &mut NoIo).unwrap();
    (capacity, report(&runtime).phases.ast)
}

#[test]
fn twenty_thousand_statement_function_fits_and_is_really_callable() {
    let mut runtime = Runtime::new();
    let source = format!(
        "function large(){{{}return 42;}}large;",
        "0;".repeat(20_000)
    );
    let result = runtime.execute(&source, &mut NoIo);
    let admitted = report(&runtime);
    eprintln!("Authored20k function admission: {admitted:?}");
    let function = result.unwrap();
    assert!(matches!(function, Value::Function(_)));
    assert!(admitted.first_rejected.is_none());
    assert!(admitted.phases.ast >= 20_000 * std::mem::size_of::<Stmt>() as u64);
    drop(source);
    assert_eq!(
        runtime
            .invoke(function, Value::Undefined, vec![], &mut NoIo)
            .unwrap(),
        Value::Number(42.0)
    );
    let called = report(&runtime);
    assert_eq!(called.phases.ast, admitted.phases.ast);
    assert_eq!(called.phases.source, admitted.phases.source);
}

#[test]
fn sparse_array_ast_admits_real_container_capacity_before_any_call() {
    let source = format!("function sparse(){{return [{}];}}", ",".repeat(10_000));
    let program = syntax::parse(&source).unwrap();
    let Stmt::Function { body, .. } = &program.0[0] else {
        panic!("Expected function");
    };
    let Stmt::Return(Some(Expr::Array(items))) = &body[0] else {
        panic!("Expected array return");
    };
    assert_eq!(items.len(), 10_000);
    assert!(items.iter().all(Option::is_none));
    let backing_bytes = items.capacity() * std::mem::size_of::<Option<Expr>>();
    assert!(backing_bytes >= 10_000 * std::mem::size_of::<Option<Expr>>());
    drop(program);
    let mut runtime = Runtime::new();
    runtime.execute(&source, &mut NoIo).unwrap();
    let admitted = report(&runtime);
    eprintln!("Authored10k sparse AST: actual backing={backing_bytes}, report={admitted:?}");
    assert!(admitted.phases.ast >= backing_bytes as u64);
    assert!(admitted.first_rejected.is_none());
}

#[test]
fn dense_and_sparse_completed_capacity_transitions_pay_for_one_real_slot() {
    for holes in [false, true] {
        let (small_capacity, small_ast) = array_capacity_and_ast(4096, holes);
        let (large_capacity, large_ast) = array_capacity_and_ast(4097, holes);
        assert_eq!((small_capacity, large_capacity), (4096, 4097));
        let new_storage = (large_capacity - small_capacity) * std::mem::size_of::<Option<Expr>>();
        assert_eq!(new_storage, std::mem::size_of::<Option<Expr>>());
        assert_eq!(
            large_ast - small_ast,
            new_storage as u64,
            "holes={holes}: {small_ast}→{large_ast}, backing growth {new_storage}"
        );
    }
}

#[test]
fn root_block_and_case_vectors_pay_for_capacity_not_only_live_children() {
    for length in [4096, 4097] {
        for block in [false, true] {
            let statements = ";".repeat(length);
            let source = if block {
                format!("{{{statements}}}")
            } else {
                statements
            };
            let program = syntax::parse(&source).unwrap();
            let mut backing = program.0.capacity() * std::mem::size_of::<Stmt>();
            if block {
                let Stmt::Block(body) = &program.0[0] else {
                    panic!("Expected block");
                };
                assert_eq!(body.len(), length);
                backing += body.capacity() * std::mem::size_of::<Stmt>();
            }
            drop(program);
            let mut runtime = Runtime::new();
            runtime.execute(&source, &mut NoIo).unwrap();
            let allocation = report(&runtime);
            assert!(allocation.phases.ast >= backing as u64);
            assert_eq!(allocation.phases.source, (128 + source.len()) as u64);
        }
    }
    for length in [128, 129] {
        let cases = (0..length)
            .map(|n| format!("case {n}:;"))
            .collect::<String>();
        let source = format!("function retained(value){{switch(value){{{cases}}}}}");
        let program = syntax::parse(&source).unwrap();
        let Stmt::Function { body, .. } = &program.0[0] else {
            panic!("Expected function");
        };
        let Stmt::Switch { cases, .. } = &body[0] else {
            panic!("Expected switch");
        };
        assert_eq!(cases.len(), length);
        let backing = cases.capacity() * std::mem::size_of::<SwitchCase>()
            + cases
                .iter()
                .map(|case| case.body.capacity() * std::mem::size_of::<Stmt>())
                .sum::<usize>();
        drop(program);
        let mut runtime = Runtime::new();
        runtime.execute(&source, &mut NoIo).unwrap();
        assert!(report(&runtime).phases.ast >= backing as u64);
    }
}

#[test]
fn loop_heavy_code_retains_boxed_fields_and_executes_the_same_control_flow() {
    let count = 300;
    let source = format!(
        "function loops(){{{}{}return 42;}}loops;",
        "for(;false;0){}".repeat(count),
        "for(var key in null){}".repeat(count)
    );
    let mut runtime = Runtime::new();
    let function = runtime.execute(&source, &mut NoIo).unwrap();
    let admitted = report(&runtime);
    // Body statement slots, the two For expression boxes, both loop bodies,
    // and the ForIn binding are distinct retained allocations. Do not derive
    // this floor from the runtime's accounting visitor or its fixed overhead.
    let minimum = count
        * (2 * std::mem::size_of::<Stmt>()
            + 2 * std::mem::size_of::<Expr>()
            + 2 * std::mem::size_of::<Stmt>()
            + std::mem::size_of::<ForInBinding>());
    assert!(admitted.phases.ast >= minimum as u64);
    assert_eq!(
        runtime
            .invoke(function, Value::Undefined, vec![], &mut NoIo)
            .unwrap(),
        Value::Number(42.0)
    );
    let source = "var total=0;outer:for(var i=0;i<3;i++){for(var key in {x:1,y:2}){switch(key){case 'x':total++;break;default:total+=2;}}}for(;;){total++;break;}total===10;";
    assert_eq!(
        Runtime::new().execute(source, &mut NoIo).unwrap(),
        Value::Bool(true)
    );
}

fn compile(runtime: &mut Runtime, mode: usize, source: &str) -> Result<Value, String> {
    match mode {
        0 => runtime.execute(source, &mut NoIo),
        1 => runtime.invoke(
            Value::Native("eval".into()),
            Value::Undefined,
            vec![Value::text(source)],
            &mut NoIo,
        ),
        2 => runtime.invoke(
            Value::Native("Function".into()),
            Value::Undefined,
            vec![Value::text(source)],
            &mut NoIo,
        ),
        _ => unreachable!(),
    }
}

#[test]
fn separate_parse_roots_pay_again_but_shared_closures_keep_identity_and_lifetime() {
    for mode in 0..3 {
        let factory = format!(
            "function(seed){{return function(){{{}return seed;}};}}",
            "0;".repeat(1000)
        );
        let source = if mode == 2 {
            format!("return {factory};")
        } else {
            format!("({factory});")
        };
        let mut runtime = Runtime::new();
        let before = report(&runtime);
        let first = compile(&mut runtime, mode, &source).unwrap();
        let once = report(&runtime);
        let second = compile(&mut runtime, mode, &source).unwrap();
        let twice = report(&runtime);
        assert_ne!(first, second);
        assert_eq!(
            once.phases.ast - before.phases.ast,
            twice.phases.ast - once.phases.ast
        );
        assert!(twice.phases.source > once.phases.source);
        let factory = if mode == 2 {
            runtime
                .invoke(first, Value::Undefined, vec![], &mut NoIo)
                .unwrap()
        } else {
            first
        };
        drop(source);
        let parsed = report(&runtime);
        let mut closures = Vec::new();
        for seed in 0..6 {
            let closure = runtime
                .invoke(
                    factory.clone(),
                    Value::Undefined,
                    vec![Value::Number(seed as f64)],
                    &mut NoIo,
                )
                .unwrap();
            assert!(!closures.contains(&closure));
            closures.push(closure);
        }
        for (seed, closure) in closures.into_iter().enumerate() {
            assert_eq!(
                runtime
                    .invoke(closure, Value::Undefined, vec![], &mut NoIo)
                    .unwrap(),
                Value::Number(seed as f64)
            );
        }
        let called = report(&runtime);
        assert_eq!(called.phases.ast, parsed.phases.ast);
        assert_eq!(called.phases.source, parsed.phases.source);
        assert!(called.phases.function_code > parsed.phases.function_code);
    }
}

#[test]
fn repeated_sparse_parse_eval_and_function_storage_fails_cumulatively() {
    for mode in 0..3 {
        let source = if mode == 2 {
            format!("return [{}];", ",".repeat(10_000))
        } else {
            array_function(10_000, true)
        };
        let mut runtime = Runtime::new();
        let mut previous = report(&runtime);
        let mut successes = 0;
        let mut rejected = false;
        for _ in 0..8 {
            match compile(&mut runtime, mode, &source) {
                Ok(_) => {
                    successes += 1;
                    let next = report(&runtime);
                    assert!(next.phases.ast > previous.phases.ast);
                    assert!(next.phases.source > previous.phases.source);
                    previous = next;
                }
                Err(error) => {
                    assert!(error.contains("allocation budget exhausted"), "{error}");
                    let first = report(&runtime);
                    let rejection = first.first_rejected.unwrap();
                    assert_eq!(
                        rejection.phase,
                        if mode == 1 {
                            AllocationPhase::Source
                        } else {
                            AllocationPhase::Ast
                        }
                    );
                    if mode == 1 {
                        // Seven compact trees fit; the eighth eval's real
                        // UTF-8 source admission now precedes AST rejection.
                        assert_eq!(rejection.requested_bytes, source.len() as u64);
                        assert_eq!(first.phases.source, previous.phases.source);
                        assert_eq!(
                            first.phases.runtime - previous.phases.runtime,
                            (2 * source.len() + 4) as u64
                        );
                    }
                    assert_eq!(first.phases.ast, previous.phases.ast);
                    assert_eq!(first.phases.function_code, previous.phases.function_code);
                    assert_eq!(successes, 7);
                    latched(&mut runtime, first, &error);
                    rejected = true;
                    break;
                }
            }
        }
        assert!(
            rejected,
            "mode={mode} did not enforce the unchanged realm cap"
        );
    }
}

#[test]
fn oversized_sparse_ast_rejects_before_prefix_hoisting_catch_or_finally() {
    let mut runtime = Runtime::new();
    runtime
        .execute("var marker=0,caught=false,finalized=false;", &mut NoIo)
        .unwrap();
    let before = report(&runtime);
    let sparse = format!("[{}]", ",".repeat(10_000));
    let source = format!(
        "marker=1;function untouched(){{return [{}];}}try{{marker=2;}}catch(e){{caught=true;}}finally{{finalized=true;}}",
        // The original five-array source is preserved as a positive in
        // js_ast_array_limits; eight real buffers still exceed the same cap.
        vec![sparse; 8].join(",")
    );
    let error = runtime.execute(&source, &mut NoIo).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(&runtime);
    assert_eq!(first.first_rejected.unwrap().phase, AllocationPhase::Ast);
    assert_eq!(first.phases.ast, before.phases.ast);
    assert_eq!(first.phases.function_code, before.phases.function_code);
    assert_eq!(
        first.phases.source - before.phases.source,
        (source.len() + 128) as u64
    );
    assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
    assert_eq!(runtime.get_global("untouched"), Value::Undefined);
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    latched(&mut runtime, first, &error);
}

#[test]
fn source_attempts_and_structural_depth_limits_remain_independent_of_storage() {
    let mut runtime = Runtime::new();
    runtime.set_global("marker", Value::Number(0.0));
    let before = report(&runtime);
    let invalid = "marker=9;var invalid=;";
    let error = runtime.execute(invalid, &mut NoIo).unwrap_err();
    assert!(!error.contains("limit exhausted"), "{error}");
    let after = report(&runtime);
    assert_eq!(
        after.phases.source - before.phases.source,
        (invalid.len() + 128) as u64
    );
    assert_eq!(after.phases.ast, before.phases.ast);
    assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
    let grouped = format!("{}42{};", "(".repeat(64), ")".repeat(64));
    assert_eq!(
        runtime.execute(&grouped, &mut NoIo).unwrap(),
        Value::Number(42.0)
    );
    for depth in [128, 1024] {
        let mut runtime = Runtime::new();
        let source = if depth == 128 {
            format!(
                "({}0{});",
                "function(){return ".repeat(depth),
                ";}".repeat(depth)
            )
        } else {
            format!("{}0{};", "(".repeat(depth), ")".repeat(depth))
        };
        let error = runtime.execute(&source, &mut NoIo).unwrap_err();
        assert!(error.contains("limit exhausted"), "{error}");
        let first = report(&runtime);
        assert!(first.first_rejected.is_none());
        latched(&mut runtime, first, &error);
    }
}
