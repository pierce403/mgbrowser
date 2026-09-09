//! Authored array-finalization semantics/resource corpus. All 21 groups passed
//! against the frozen pre-finalization library before the documented repeated-
//! eval Source-phase migration. No website or alternate engine input;
//! geometry-specific admission gates are in js_ast_array_limits.
use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};
use mg_butane::{Expr, Stmt, syntax};
use std::rc::Rc;

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
fn yes(source: &str) {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime.execute(source, &mut NoIo).unwrap(),
        Value::Bool(true),
        "{source}"
    );
    assert!(report(&runtime).first_rejected.is_none());
    assert!(!runtime.is_fatal());
}
fn invoke(runtime: &mut Runtime, value: Value) -> Result<Value, String> {
    runtime.invoke(value, Value::Undefined, vec![], &mut NoIo)
}
fn latch(runtime: &mut Runtime, error: &str, first: AllocationReport) {
    assert!(runtime.is_fatal());
    assert_eq!(
        runtime.execute("var later=1;", &mut NoIo).unwrap_err(),
        error
    );
    assert_eq!(
        invoke(runtime, Value::Native("Number".into())).unwrap_err(),
        error
    );
    runtime.set_global("later", Value::Number(2.0));
    assert_eq!(runtime.get_global("later"), Value::Undefined);
    assert_eq!(report(runtime), first);
}
fn outer_array(program: &mg_butane::Program) -> &[Option<Expr>] {
    let Stmt::Expr(Expr::Array(items)) = &program.0[0] else {
        panic!("array expression expected")
    };
    items
}

#[test]
fn both_closing_paths_preserve_holes_values_and_trailing_length() {
    for (source, present) in [
        ("[];", vec![]),
        ("[,];", vec![false]),
        ("[,,];", vec![false, false]),
        ("[1];", vec![true]),
        ("[1,];", vec![true]),
        ("[1,,];", vec![true, false]),
        ("[,1];", vec![false, true]),
        ("[,1,];", vec![false, true]),
        ("[,1,,];", vec![false, true, false]),
    ] {
        let parsed = syntax::parse(source).unwrap();
        let items = outer_array(&parsed);
        assert_eq!(
            items.iter().map(Option::is_some).collect::<Vec<_>>(),
            present,
            "{source}"
        );
        yes(&format!(
            "var value={source}value.length==={};",
            present.len()
        ));
    }
}

#[test]
fn undefined_is_own_but_holes_can_read_inherited_values() {
    yes(
        "Array.prototype[0]=9;var hole=[,],present=[undefined];hole[0]===9&&present[0]===undefined&&hole.length===1&&present.length===1&&!Object.prototype.hasOwnProperty.call(hole,'0')&&Object.prototype.hasOwnProperty.call(present,'0');",
    );
    yes(
        "var value=[,undefined,,3,];Object.keys(value).join(',')==='1,3'&&value.length===4&&!(0 in value)&&(1 in value)&&!(2 in value);",
    );
}

#[test]
fn element_expressions_run_left_to_right_once_and_preserve_identity() {
    yes(
        "var trace='',object={},token=Symbol('array');function take(tag,value){trace+=tag;return value;}var value=[take('a',object),,take('b',token),take('c',object),];trace==='abc'&&value.length===4&&value[0]===object&&value[2]===token&&value[3]===object&&!(1 in value);",
    );
}

#[test]
fn numeric_bits_and_utf16_units_are_not_coerced_by_literal_storage() {
    yes(
        "var value=[-0,NaN,Infinity,'\\uD800\\u0000\\uDFFF','😀'];1/value[0]===-Infinity&&value[1]!==value[1]&&value[2]===Infinity&&value[3].length===3&&value[3].charCodeAt(0)===55296&&value[3].charCodeAt(1)===0&&value[3].charCodeAt(2)===57343&&value[4].length===2;",
    );
}

#[test]
fn abrupt_element_stops_later_elements_without_replacing_assignment_target() {
    yes(
        "var trace='',saved='old',caught=0;function stop(){trace+='b';throw 17;}try{saved=[(trace+='a',1),stop(),(trace+='c',3)];}catch(error){caught=error;}trace==='ab'&&saved==='old'&&caught===17;",
    );
}

#[test]
fn getter_element_receivers_and_compound_side_effects_remain_ordered() {
    yes(
        "var trace='',parent=Object.create(null,{value:{get:function(){trace+='g';return this.marker;}}}),child=Object.create(parent);child.marker=4;var n=2,value=[child.value,n++,++n];trace==='g'&&value[0]===4&&value[1]===2&&value[2]===4&&n===4;",
    );
}

#[test]
fn repeated_literal_evaluation_creates_distinct_mutable_nested_arrays() {
    yes(
        "function make(){return [[1,,3],{field:4},];}var a=make(),b=make();a[0][0]=9;a[1].field=8;a.push(7);a!==b&&a[0]!==b[0]&&a[1]!==b[1]&&b.length===2&&b[0][0]===1&&!(1 in b[0])&&b[1].field===4;",
    );
}

#[test]
fn escaping_closures_keep_their_own_array_and_captured_environment() {
    yes(
        "function factory(seed){var kept=[seed,,function(){return seed;}];return function(){return kept;};}var f=factory(7),g=factory(8),a=f(),b=g();a===f()&&a!==b&&a[0]===7&&b[0]===8&&a[2]()===7&&b[2]()===8&&!(1 in a);",
    );
}

#[test]
fn ordinary_direct_indirect_eval_and_function_keep_scope_and_array_shape() {
    yes(
        "var marker=10;function local(){var marker=3;var a=eval('[marker,,]'),b=(0,eval)('[marker,,]'),c=Function('return [marker,,];')();return a[0]===3&&b[0]===10&&c[0]===10&&a.length===2&&b.length===2&&c.length===2&&!(1 in a);}local();",
    );
}

#[test]
fn separately_owned_ast_clone_survives_source_and_original_drop() {
    let source = "['\\uD800x',,[1,,2],function(){return 7;}];".to_owned();
    let parsed = syntax::parse(&source).unwrap();
    let cloned = parsed.clone();
    assert_eq!(parsed, cloned);
    let original = outer_array(&parsed);
    let copy = outer_array(&cloned);
    let (Some(Expr::String(a)), Some(Expr::String(b))) = (&original[0], &copy[0]) else {
        panic!()
    };
    assert_ne!(a.as_ptr(), b.as_ptr());
    let (Some(Expr::Array(a)), Some(Expr::Array(b))) = (&original[2], &copy[2]) else {
        panic!()
    };
    assert_ne!(a.as_ptr(), b.as_ptr());
    let (Some(Expr::Function { body: a, .. }), Some(Expr::Function { body: b, .. })) =
        (&original[3], &copy[3])
    else {
        panic!()
    };
    assert!(Rc::ptr_eq(a, b));
    drop(parsed);
    drop(source);
    let Some(Expr::String(units)) = &outer_array(&cloned)[0] else {
        panic!()
    };
    assert_eq!(units, &[0xd800, b'x' as u16]);
    drop(cloned);
}

#[test]
fn shared_function_body_and_runtime_function_outlive_their_input_buffers() {
    let source = "function retained(){return ['\\uDFFF',,7,];}retained;".to_owned();
    let parsed = syntax::parse(&source).unwrap();
    let cloned = parsed.clone();
    let (
        Stmt::Function {
            params: a, body: b, ..
        },
        Stmt::Function {
            params: c, body: d, ..
        },
    ) = (&parsed.0[0], &cloned.0[0])
    else {
        panic!()
    };
    assert!(Rc::ptr_eq(a, c) && Rc::ptr_eq(b, d));
    let mut runtime = Runtime::new();
    let function = runtime.execute(&source, &mut NoIo).unwrap();
    drop(source);
    drop(parsed);
    drop(cloned);
    let compiled = report(&runtime);
    let result = invoke(&mut runtime, function).unwrap();
    assert_eq!(report(&runtime).phases.ast, compiled.phases.ast);
    assert_eq!(report(&runtime).phases.source, compiled.phases.source);
    runtime.set_global("answer", result);
    assert_eq!(runtime.execute("answer.length===3&&answer[0].charCodeAt(0)===57343&&!(1 in answer)&&answer[2]===7;", &mut NoIo).unwrap(), Value::Bool(true));
}

#[test]
fn array_context_preserves_noin_sequences_regex_division_and_asi() {
    yes(
        "var count=0;for(var a=['x' in {x:1}];a.length;a.pop()){if(a[0])count++;}var values=[(1,2),/x/.test('x'),12/3,];count===1&&values[0]===2&&values[1]===true&&values[2]===4&&values.length===3;",
    );
    yes(
        "function empty(){return\n[1,2];}var value=[/*a*/1,//b\n2,];empty()===undefined&&value.length===2&&value[1]===2;",
    );
}

#[test]
fn malformed_arrays_never_publish_prefix_or_hoisted_declarations() {
    for ending in ["[1,;", "[1 2];", "[1,,", "[,)", "[function(){return 1;}];]"] {
        let source = format!("marker=1;function hidden(){{}}var value={ending}");
        let first = syntax::parse(&source).unwrap_err();
        assert_eq!(syntax::parse(&source).unwrap_err(), first);
        let mut runtime = Runtime::new();
        runtime.set_global("marker", Value::Number(0.0));
        let error = runtime.execute(&source, &mut NoIo).unwrap_err();
        assert!(error.contains("SyntaxError"), "{error}");
        assert!(!runtime.is_fatal());
        assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
        assert_eq!(runtime.get_global("hidden"), Value::Undefined);
        assert_eq!(report(&runtime).phases.ast, 0);
        assert_eq!(
            runtime.execute("40+2;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn sixty_four_nested_arrays_parse_clone_drop_and_overdepth_stays_fatal() {
    let source = format!("{}0{};", "[".repeat(64), "]".repeat(64));
    let parsed = syntax::parse(&source).unwrap();
    let cloned = parsed.clone();
    assert_eq!(parsed, cloned);
    drop(parsed);
    drop(cloned);
    for mode in 0..3 {
        let source = format!("{}0{};", "[".repeat(129), "]".repeat(129));
        let mut runtime = Runtime::new();
        let error = match mode {
            0 => runtime.execute(&source, &mut NoIo),
            1 => runtime.invoke(
                Value::Native("eval".into()),
                Value::Undefined,
                vec![Value::text(&source)],
                &mut NoIo,
            ),
            _ => runtime.invoke(
                Value::Native("Function".into()),
                Value::Undefined,
                vec![Value::text(&source)],
                &mut NoIo,
            ),
        }
        .unwrap_err();
        assert!(
            error.contains("nesting limit exceeded") || error.contains("depth limit exceeded"),
            "{error}"
        );
        let first = report(&runtime);
        assert_eq!(first.phases.ast, 0);
        assert!(first.first_rejected.is_none());
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn uncalled_over_runtime_limit_literal_parses_but_call_preflight_has_no_effects() {
    let source = format!(
        "function make(){{return [marker=1,{}];}}make;",
        ",".repeat(10_000)
    );
    let parsed = syntax::parse(&source).unwrap();
    let Stmt::Function { body, .. } = &parsed.0[0] else {
        panic!()
    };
    let Stmt::Return(Some(Expr::Array(items))) = &body[0] else {
        panic!()
    };
    assert_eq!(items.len(), 10_001);
    let mut runtime = Runtime::new();
    runtime.set_global("marker", Value::Number(0.0));
    let function = runtime.execute(&source, &mut NoIo).unwrap();
    let before = report(&runtime);
    let error = invoke(&mut runtime, function).unwrap_err();
    assert!(error.contains("array limit exhausted"), "{error}");
    assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
    let first = report(&runtime);
    assert_eq!(first.phases.ast, before.phases.ast);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn original_six_sparse_evaluation_runtime_negative_remains_unchanged() {
    // Exact existing js_arrays source and failure controls, not a new capacity pin.
    let literal = format!("[{}]", ",".repeat(10_000));
    let source = format!(
        "var last;for(var i=0;i<6;i++)last={literal};last.length===10000&&(0 in last)===false;"
    );
    let mut runtime = Runtime::new();
    let error = runtime.execute(&source, &mut NoIo).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    assert_eq!(runtime.get_global("i"), Value::Number(5.0));
    let first = report(&runtime);
    assert_eq!(
        first.first_rejected.unwrap().phase,
        AllocationPhase::Runtime
    );
    assert_eq!(first.first_rejected.unwrap().requested_bytes, 640_000);
    assert!(first.phases.ast >= 10_000 * std::mem::size_of::<Option<Expr>>() as u64);
    latch(&mut runtime, &error, first);
}

#[test]
fn separately_parsed_sparse_trees_remain_cumulative_in_all_three_roots() {
    for mode in 0..3 {
        let body = format!("return [{}];", ",".repeat(10_000));
        let source = if mode == 2 {
            body
        } else {
            format!("function retained(){{{body}}}")
        };
        let mut runtime = Runtime::new();
        let mut previous = report(&runtime);
        let mut failed = false;
        for attempt in 0..8 {
            let result = match mode {
                0 => runtime.execute(&source, &mut NoIo),
                1 => runtime.invoke(
                    Value::Native("eval".into()),
                    Value::Undefined,
                    vec![Value::text(&source)],
                    &mut NoIo,
                ),
                _ => runtime.invoke(
                    Value::Native("Function".into()),
                    Value::Undefined,
                    vec![Value::text(&source)],
                    &mut NoIo,
                ),
            };
            match result {
                Ok(_) => {
                    let next = report(&runtime);
                    assert!(next.phases.ast > previous.phases.ast);
                    assert!(next.phases.source > previous.phases.source);
                    previous = next;
                }
                Err(error) => {
                    assert!(error.contains("allocation budget exhausted"), "{error}");
                    let first = report(&runtime);
                    assert_eq!(attempt, 7);
                    let rejected = first.first_rejected.unwrap();
                    assert_eq!(
                        rejected.phase,
                        if mode == 1 {
                            AllocationPhase::Source
                        } else {
                            AllocationPhase::Ast
                        }
                    );
                    if mode == 1 {
                        // Explicitly reviewed change from the old fifth-tree
                        // AST failure: UTF-8 admission rejects on attempt eight.
                        assert_eq!(rejected.requested_bytes, source.len() as u64);
                        assert_eq!(first.phases.source, previous.phases.source);
                        assert_eq!(
                            first.phases.runtime - previous.phases.runtime,
                            (2 * source.len() + 4) as u64
                        );
                    }
                    assert_eq!(first.phases.ast, previous.phases.ast);
                    assert_eq!(first.phases.function_code, previous.phases.function_code);
                    latch(&mut runtime, &error, first);
                    failed = true;
                    break;
                }
            }
        }
        assert!(failed, "mode {mode} escaped the fixed cumulative cap");
    }
}

#[test]
fn malformed_dynamic_arrays_charge_attempts_without_admitting_partial_ast() {
    for method in ["eval", "Function"] {
        let mut runtime = Runtime::new();
        let mut prior = report(&runtime);
        let mut source_delta = None;
        for _ in 0..16 {
            let error = runtime
                .invoke(
                    Value::Native(method.into()),
                    Value::Undefined,
                    vec![Value::text("var partial=[1,2,3];var broken=[4,;")],
                    &mut NoIo,
                )
                .unwrap_err();
            assert!(error.contains("SyntaxError"), "{error}");
            assert!(!runtime.is_fatal());
            let next = report(&runtime);
            assert_eq!(next.phases.ast, 0);
            assert!(next.phases.source > prior.phases.source);
            let delta = next.phases.source - prior.phases.source;
            assert_eq!(*source_delta.get_or_insert(delta), delta);
            assert_eq!(runtime.get_global("partial"), Value::Undefined);
            prior = next;
        }
        assert_eq!(
            runtime.execute("42;", &mut NoIo).unwrap(),
            Value::Number(42.0)
        );
    }
}

#[test]
fn unrelated_frozen_fuel_and_full_phase_checkpoint_does_not_change() {
    // Existing diagnostic control has no nonempty array literal to compact.
    let source = "var rounds=0,ticks=0;for(var i=0;i<128;i++){try{null.length;}catch(e){rounds++;}}while(true){ticks++;}";
    let mut runtime = Runtime::new();
    let error = runtime.execute(source, &mut NoIo).unwrap_err();
    assert_eq!(error, "JavaScript fuel exhausted");
    assert_eq!(runtime.get_global("rounds"), Value::Number(128.0));
    assert_eq!(runtime.get_global("ticks"), Value::Number(21_462.0));
    let first = report(&runtime);
    assert_eq!(first.accepted_bytes, 76_934 + 156 + 725 - 71);
    let p = first.phases;
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
        [26_880, 230, 3737 - 71, 0, 46_968, 0, 0]
    );
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn native_array_slot_cost_and_bootstrap_are_not_ast_finalization_targets() {
    let mut costs = Vec::new();
    for length in [0, 10_000] {
        let mut runtime = Runtime::new();
        let before = report(&runtime);
        assert_eq!(before.accepted_bytes, 26_880);
        runtime
            .invoke(
                Value::Native("Array".into()),
                Value::Undefined,
                vec![Value::Number(length as f64)],
                &mut NoIo,
            )
            .unwrap();
        let after = report(&runtime);
        assert_eq!(after.phases.ast, 0);
        assert_eq!(after.phases.source, 0);
        costs.push(after.phases.runtime - before.phases.runtime);
    }
    assert_eq!(costs[1] - costs[0], 640_000);
}

#[test]
fn source_and_token_limits_still_precede_prefix_effects_and_ast_admission() {
    for source in [
        " ".repeat(1024 * 1024 + 1),
        format!("marker=1;{}", ";".repeat(100_001)),
    ] {
        let mut runtime = Runtime::new();
        runtime.set_global("marker", Value::Number(0.0));
        let error = runtime.execute(&source, &mut NoIo).unwrap_err();
        assert!(
            error.contains("Source exceeds one MiB")
                || error.contains("Token limit exceeded")
                || error.contains("AST node limit exceeded"),
            "{error}"
        );
        assert_eq!(runtime.get_global("marker"), Value::Number(0.0));
        let first = report(&runtime);
        assert_eq!(first.phases.ast, 0);
        assert!(first.first_rejected.is_none());
        latch(&mut runtime, &error, first);
    }
}
