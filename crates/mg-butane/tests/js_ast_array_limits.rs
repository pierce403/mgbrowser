//! Array geometry/admission expectations frozen before implementation. Six groups
//! failed and two fatal controls passed on the old library. Distinct from the
//! semantic corpus and preserved historical geometric-capacity pins.
use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};
use mg_butane::{Expr, Program, Stmt, syntax};
use std::mem::size_of;

const LENGTHS: [usize; 9] = [0, 1, 3, 4, 5, 4096, 4097, 10_000, 10_001];
const LIMIT: u64 = 4 * 1024 * 1024;
struct NoIo;
impl Host for NoIo {
    fn get(&mut self, _: &str, _: &str) -> Result<Value, String> {
        panic!("unexpected Host Get")
    }
    fn set(&mut self, _: &str, _: &str, _: Value) -> Result<(), String> {
        panic!("unexpected Host Set")
    }
    fn call(&mut self, _: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected Host call")
    }
}
fn report(runtime: &Runtime) -> AllocationReport {
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, LIMIT);
    report
}
fn literal(length: usize, holes: bool, trailing: bool) -> String {
    if holes {
        // Hole-only arrays can finish only through ArrayStart. A final comma
        // here is a hole, not the no-extra-element trailing comma of [0,].
        format!("[{}]", ",".repeat(length))
    } else if length == 0 {
        "[]".into()
    } else {
        let mut elements = vec!["0"; length].join(",");
        if trailing {
            elements.push(',');
        }
        format!("[{elements}]")
    }
}
fn root_items(program: &Program) -> &Vec<Option<Expr>> {
    let Stmt::Expr(Expr::Array(items)) = &program.0[0] else {
        panic!("root array expected")
    };
    items
}
fn function_items(body: &[Stmt]) -> &Vec<Option<Expr>> {
    let Stmt::Return(Some(Expr::Array(items))) = &body[0] else {
        panic!("returned array expected")
    };
    items
}
fn retained_source(length: usize, holes: bool, trailing: bool) -> String {
    format!(
        "function retained(){{return {};}}",
        literal(length, holes, trailing)
    )
}
fn admitted(length: usize, holes: bool, trailing: bool) -> AllocationReport {
    let mut runtime = Runtime::new();
    let source = retained_source(length, holes, trailing);
    runtime.execute(&source, &mut NoIo).unwrap();
    let result = report(&runtime);
    assert!(!runtime.is_fatal());
    assert!(result.first_rejected.is_none());
    result
}
fn latch(runtime: &mut Runtime, error: &str, first: AllocationReport) {
    assert!(runtime.is_fatal());
    assert_eq!(
        runtime.execute("var later=1;", &mut NoIo).unwrap_err(),
        error
    );
    assert_eq!(
        runtime
            .invoke(
                Value::Native("Number".into()),
                Value::Undefined,
                vec![],
                &mut NoIo
            )
            .unwrap_err(),
        error
    );
    runtime.set_global("later", Value::Number(2.0));
    assert_eq!(runtime.get_global("later"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

#[test]
fn exact_dense_capacity_in_both_literal_closing_paths() {
    for length in LENGTHS {
        for trailing in [false, true] {
            let source = format!("{};", literal(length, false, trailing));
            let parsed = syntax::parse(&source).unwrap();
            let items = root_items(&parsed);
            assert_eq!(items.len(), length, "length={length} trailing={trailing}");
            assert_eq!(
                items.capacity(),
                length,
                "length={length} trailing={trailing}"
            );
            assert!(
                items
                    .iter()
                    .all(|item| matches!(item, Some(Expr::Number(value)) if *value == 0.0))
            );
        }
    }
}

#[test]
fn exact_sparse_capacity_preserves_every_hole_including_10001_uncalled_slots() {
    for length in LENGTHS {
        let parsed = syntax::parse(&format!("{};", literal(length, true, false))).unwrap();
        let items = root_items(&parsed);
        assert_eq!(items.len(), length);
        assert_eq!(items.capacity(), length);
        assert!(items.iter().all(Option::is_none));
    }
}

#[test]
fn nested_and_dynamic_function_parser_roots_finalize_the_same_arrays() {
    for length in LENGTHS {
        for holes in [false, true] {
            for trailing in [false, true] {
                let source = retained_source(length, holes, trailing);
                let parsed = syntax::parse(&source).unwrap();
                let Stmt::Function { body, .. } = &parsed.0[0] else {
                    panic!()
                };
                let items = function_items(body);
                assert_eq!((items.len(), items.capacity()), (length, length));
                let dynamic = syntax::parse_function(
                    "",
                    &format!("return {};", literal(length, holes, trailing)),
                )
                .unwrap();
                let Expr::Function { body, .. } = dynamic else {
                    panic!()
                };
                let items = function_items(&body);
                assert_eq!((items.len(), items.capacity()), (length, length));
                assert_eq!(
                    items.iter().filter(|item| item.is_none()).count(),
                    if holes { length } else { 0 }
                );
            }
        }
    }
}

#[test]
fn retained_slots_and_one_real_block_still_receive_full_ast_charge() {
    assert_eq!(size_of::<Option<Expr>>(), 56);
    for holes in [false, true] {
        for trailing in [false, true] {
            let empty = admitted(0, holes, trailing);
            for length in LENGTHS {
                let actual = admitted(length, holes, trailing);
                let expected = if length == 0 {
                    0
                } else {
                    (length * size_of::<Option<Expr>>() + 16) as u64
                };
                assert_eq!(
                    actual.phases.ast - empty.phases.ast,
                    expected,
                    "length={length} holes={holes} trailing={trailing}"
                );
                assert_eq!(actual.phases.bootstrap, empty.phases.bootstrap);
                assert_eq!(actual.phases.function_code, empty.phases.function_code);
                assert_eq!(actual.phases.runtime, empty.phases.runtime);
                assert_eq!(actual.phases.regex_compile, empty.phases.regex_compile);
                assert_eq!(actual.phases.regex_result, empty.phases.regex_result);
                assert_eq!(
                    actual.phases.source,
                    (128 + retained_source(length, holes, trailing).len()) as u64
                );
            }
        }
    }
}

#[test]
fn the_4096_to_4097_transition_is_one_slot_not_an_unretained_growth_buffer() {
    for holes in [false, true] {
        for trailing in [false, true] {
            let small = admitted(4096, holes, trailing);
            let large = admitted(4097, holes, trailing);
            assert_eq!(large.phases.ast - small.phases.ast, 56);
            assert_eq!(
                large.accepted_bytes - small.accepted_bytes,
                56 + large.phases.source - small.phases.source
            );
        }
    }
    for length in [1, 3, 4, 5, 4097] {
        let closed_after_element = admitted(length, false, false);
        let closed_after_comma = admitted(length, false, true);
        assert_eq!(
            closed_after_element.phases.ast,
            closed_after_comma.phases.ast
        );
        assert_eq!(
            closed_after_comma.phases.source - closed_after_element.phases.source,
            1
        );
        assert_eq!(
            closed_after_comma.accepted_bytes - closed_after_element.accepted_bytes,
            1
        );
    }
}

fn nested_source(count: usize) -> String {
    // count=5 preserves the exact existing js_ast_storage negative source.
    // count=8 is a separately specified larger negative under the same caps.
    let sparse = format!("[{}]", ",".repeat(10_000));
    format!(
        "marker=1;function untouched(){{return [{}];}}try{{marker=2;}}catch(e){{caught=true;}}finally{{finalized=true;}}",
        vec![sparse; count].join(",")
    )
}
fn initialized() -> Runtime {
    let mut runtime = Runtime::new();
    runtime
        .execute("var marker=0,caught=false,finalized=false;", &mut NoIo)
        .unwrap();
    runtime
}

#[test]
fn original_five_nested_sparse_source_now_fits_without_calling_the_large_function() {
    let mut runtime = initialized();
    let before = report(&runtime);
    let source = nested_source(5);
    runtime.execute(&source, &mut NoIo).unwrap();
    let after = report(&runtime);
    assert!(after.first_rejected.is_none());
    assert!(!runtime.is_fatal());
    assert_eq!(runtime.get_global("marker"), Value::Number(2.0));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(true));
    assert!(matches!(
        runtime.get_global("untouched"),
        Value::Function(_)
    ));
    assert_eq!(
        after.phases.source - before.phases.source,
        (source.len() + 128) as u64
    );
    assert!(after.phases.ast - before.phases.ast >= 5 * 10_000 * 56);
    assert!(after.phases.ast - before.phases.ast < 5 * 16_384 * 56);
    // Deliberately do not call untouched: five new runtime arrays would add
    // real storage and are a different workload, not this AST admission gate.
}

#[test]
fn separately_frozen_eight_nested_sparse_source_rejects_before_all_prefix_effects() {
    let source = nested_source(8);
    assert!(source.len() < 100_000);
    // This is a storage negative, not a token, grammar, or node-limit substitute.
    let parsed = syntax::parse(&source).unwrap();
    drop(parsed);
    let mut runtime = initialized();
    let before = report(&runtime);
    let error = runtime.execute(&source, &mut NoIo).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Ast);
    assert!(rejected.requested_bytes >= 8 * 10_000 * 56);
    assert_eq!(first.phases.ast, before.phases.ast);
    assert_eq!(first.phases.function_code, before.phases.function_code);
    assert_eq!(first.phases.runtime, before.phases.runtime);
    assert_eq!(
        first.phases.source - before.phases.source,
        (source.len() + 128) as u64
    );
    assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
    assert_eq!(runtime.get_global("untouched"), Value::Undefined);
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    latch(&mut runtime, &error, first);
}

#[test]
fn eight_nested_arrays_are_fatal_ast_admission_for_eval_and_function_too() {
    for method in ["eval", "Function"] {
        let mut runtime = initialized();
        let before = report(&runtime);
        let source = nested_source(8);
        let error = runtime
            .invoke(
                Value::Native(method.into()),
                Value::Undefined,
                vec![Value::text(&source)],
                &mut NoIo,
            )
            .unwrap_err();
        assert!(error.contains("allocation budget exhausted"), "{error}");
        let first = report(&runtime);
        assert_eq!(first.first_rejected.unwrap().phase, AllocationPhase::Ast);
        assert_eq!(first.phases.ast, before.phases.ast);
        assert_eq!(first.phases.function_code, before.phases.function_code);
        assert!(first.phases.source > before.phases.source);
        assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
        assert_eq!(runtime.get_global("untouched"), Value::Undefined);
        assert_eq!(runtime.get_global("caught"), Value::Bool(false));
        assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
        latch(&mut runtime, &error, first);
    }
}
