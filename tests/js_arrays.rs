//! Independently authored array ownership/accounting regressions. No page code,
//! alternate engine, larger stack or changed realm limit is used here.

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
    let p = report.phases;
    assert_eq!(
        p.bootstrap
            + p.source
            + p.ast
            + p.function_code
            + p.runtime
            + p.regex_compile
            + p.regex_result,
        report.accepted_bytes
    );
    report
}

fn yes(source: &str) {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime.execute(source, &mut NoIo).unwrap(),
        Value::Bool(true),
        "{source}"
    );
    report(&runtime);
}

fn invoke(
    runtime: &mut Runtime,
    name: &str,
    this: Value,
    args: Vec<Value>,
) -> Result<Value, String> {
    runtime.invoke(Value::Native(name.into()), this, args, &mut NoIo)
}

fn latched(runtime: &mut Runtime, error: &str, first: AllocationReport) {
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
    assert_eq!(report(runtime), first);
    assert_eq!(
        invoke(
            runtime,
            "Number",
            Value::Undefined,
            vec![Value::Number(42.0)]
        )
        .unwrap_err(),
        error
    );
    assert_eq!(report(runtime), first);
}

#[test]
fn six_maximum_hole_arrays_succeed_under_the_unchanged_realm_budget() {
    let mut runtime = Runtime::new();
    let result = runtime.execute("var finalArray;for(var i=0;i<6;i++){finalArray=Array(10000);}finalArray.length===10000&&!(0 in finalArray)&&!(9999 in finalArray);", &mut NoIo);
    let allocation = report(&runtime);
    eprintln!("Authored six-array observation: {allocation:?}");
    assert_eq!(result.unwrap(), Value::Bool(true));
    assert!(allocation.phases.runtime >= 6 * 10_000 * 64);
    assert!(allocation.first_rejected.is_none());
}

#[test]
fn repeated_dense_and_hole_literals_pay_new_storage_once_per_evaluation() {
    for (literal, length, has_first) in [
        (format!("[{}]", ",".repeat(10_000)), 10_000, false),
        (format!("[{}0]", "0,".repeat(4999)), 5000, true),
    ] {
        let mut runtime = Runtime::new();
        let source = format!(
            "var last;for(var i=0;i<6;i++)last={literal};last.length==={length}&&(0 in last)==={has_first};"
        );
        let result = runtime.execute(&source, &mut NoIo);
        eprintln!("Authored literal length {length}: {:?}", report(&runtime));
        assert_eq!(result.unwrap(), Value::Bool(true));
    }
}

#[test]
fn array_length_constructor_and_element_constructor_remain_distinct() {
    yes(
        "var holes=Array(3),element=Array('3'),items=Array(1,2,3),empty=Array(0);holes.length===3&&!(0 in holes)&&element.length===1&&element[0]==='3'&&items.length===3&&items[0]===1&&items[2]===3&&empty.length===0;",
    );
    yes(
        "var caught=0;try{Array(-1);}catch(e){caught++;}try{Array(1.5);}catch(e){caught++;}try{Array(NaN);}catch(e){caught++;}caught===3;",
    );
}

#[test]
fn arguments_are_unmapped_snapshots_with_independent_lifetimes() {
    yes(
        "function save(value){var kept=arguments;value='parameter';kept[0]='snapshot';return function(){return value+':'+kept[0]+':'+(kept.callee===save);};}var first=save('first'),second=save('second');first!==second&&first()==='parameter:snapshot:true'&&second()==='parameter:snapshot:true';",
    );
    yes(
        "function save(){return arguments;}var a=save('one'),b=save('two');a[0]='changed';a!==b&&a[0]==='changed'&&b[0]==='two'&&a.callee===save&&b.callee===save;",
    );
}

#[test]
fn duplicate_shadowing_extra_arguments_and_object_identity_are_preserved() {
    yes(
        "function absent(first,missing){return first===1&&typeof missing==='undefined'&&arguments.length===1&&!(1 in arguments);}absent(1);",
    );
    yes(
        "function duplicate(a,a){a=3;arguments[0]=4;return a===3&&arguments[0]===4&&arguments[1]===2;}function shadow(arguments,extra){return arguments===7&&extra===8;}function extra(a){return arguments.length===3&&arguments[2]===9;}duplicate(1,2)&&shadow(7,8)&&extra(1,2,9);",
    );
    yes(
        "var shared={value:1};function change(value){var same=value===arguments[0];value.value=7;var observed=arguments[0].value;arguments[0]={value:9};return same&&observed===7&&value===shared&&value.value===7&&arguments[0].value===9;}change(shared)&&shared.value===7;",
    );
}

#[test]
fn argument_evaluation_order_and_utf16_payloads_survive_snapshot_moves() {
    yes(
        "var order='';function item(tag,value){order+=tag;return value;}function keep(a,b){return order==='ab'&&a==='\\uD800😀'&&b===7&&arguments[0]===a&&arguments[0].length===3&&arguments[1]===7;}keep(item('a','\\uD800😀'),item('b',7));",
    );
    yes(
        "var trace='';var separator={toString:function(){trace+='s';return '|';}};var first={toString:function(){trace+='a';return 'A';}};var second={toString:function(){trace+='b';return 'B';}};var output=[first,null,second].join(separator);output==='A||B'&&trace==='sab';",
    );
}

#[test]
fn slice_retains_holes_and_keeps_result_mutations_independent() {
    yes(
        "var original=[,'\\uD800😀',,7];var copy=original.slice(0,4);var preserved=copy.length===4&&!(0 in copy)&&!(2 in copy)&&copy[1]===original[1];copy[1]='changed';copy[3]=8;preserved&&original[1]==='\\uD800😀'&&original[3]===7;",
    );
}

#[test]
fn split_and_regex_results_preserve_values_holes_and_phase_attribution() {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime
            .execute(
                "var parts='a|\\uD800😀|b'.split('|');parts.length===3&&parts[1]==='\\uD800😀';",
                &mut NoIo
            )
            .unwrap(),
        Value::Bool(true)
    );
    let plain = report(&runtime);
    assert_eq!(plain.phases.regex_compile, 0);
    assert_eq!(plain.phases.regex_result, 0);
    assert_eq!(runtime.execute("var match=/(a)?(b)/.exec('b');var pieces='ab'.split(/(x)?b/);var all='aba'.match(/a/g);match.length===3&&(1 in match)&&typeof match[1]==='undefined'&&match[2]==='b'&&pieces.length===3&&(1 in pieces)&&typeof pieces[1]==='undefined'&&all.length===2&&all[0]==='a'&&all[1]==='a';", &mut NoIo).unwrap(), Value::Bool(true));
    let regex = report(&runtime);
    assert!(regex.phases.regex_compile > plain.phases.regex_compile);
    assert!(regex.phases.regex_result > plain.phases.regex_result);
}

#[test]
fn array_and_arguments_length_caps_remain_fatal_and_latched() {
    for source in [
        "try{Array(10001);}catch(e){true;}",
        "function take(){return arguments.length;}try{take.apply(null,Array(10001));}catch(e){true;}",
    ] {
        let mut runtime = Runtime::new();
        let error = runtime.execute(source, &mut NoIo).unwrap_err();
        assert!(error.contains("array limit exhausted"), "{error}");
        let first = report(&runtime);
        latched(&mut runtime, &error, first);
    }
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("function take(){return arguments.length;}take;", &mut NoIo)
        .unwrap();
    let error = runtime
        .invoke(
            function,
            Value::Undefined,
            vec![Value::Number(0.0); 10_001],
            &mut NoIo,
        )
        .unwrap_err();
    assert!(error.contains("argument limit exhausted"), "{error}");
    let first = report(&runtime);
    latched(&mut runtime, &error, first);
}

fn numeric_array_cost(length: usize, elements: bool) -> u64 {
    let mut runtime = Runtime::new();
    let before = report(&runtime);
    let args = if elements {
        vec![Value::Number(7.0); length]
    } else {
        vec![Value::Number(length as f64)]
    };
    let value = invoke(&mut runtime, "Array", Value::Undefined, args).unwrap();
    assert!(matches!(value, Value::Object(_)));
    let after = report(&runtime);
    assert_eq!(after.phases.source, 0);
    assert_eq!(after.phases.ast, 0);
    after.phases.runtime - before.phases.runtime
}

#[test]
fn numeric_and_element_constructors_admit_their_own_slots_after_public_ingress() {
    for elements in [false, true] {
        assert_eq!(
            numeric_array_cost(32, elements) - numeric_array_cost(16, elements),
            16 * 64
        );
    }
    assert_eq!(
        numeric_array_cost(10_000, false),
        numeric_array_cost(10_000, true)
    );
    assert_eq!(
        numeric_array_cost(10_000, false) - numeric_array_cost(0, false),
        10_000 * 64
    );
}

fn snapshot_slot_cost(length: usize, from_script: bool) -> u64 {
    let mut runtime = Runtime::new();
    let function = runtime
        .execute("function take(){return arguments;}take;", &mut NoIo)
        .unwrap();
    let before = report(&runtime);
    let result = if from_script {
        runtime
            .execute(&format!("take({}0);", "0,".repeat(length - 1)), &mut NoIo)
            .unwrap()
    } else {
        runtime
            .invoke(
                function,
                Value::Undefined,
                vec![Value::Number(0.0); length],
                &mut NoIo,
            )
            .unwrap()
    };
    assert!(matches!(result, Value::Object(_)));
    let after = report(&runtime);
    if !from_script {
        assert_eq!(after.phases.source, before.phases.source);
        assert_eq!(after.phases.ast, before.phases.ast);
    }
    after.phases.runtime - before.phases.runtime
}

#[test]
fn input_argument_vectors_do_not_prepay_separate_snapshot_storage() {
    // Public ingress admits payloads but not a new Option<Value> vector. A JS
    // call additionally allocates its own argument-expression value vector.
    assert_eq!(
        snapshot_slot_cost(32, false) - snapshot_slot_cost(16, false),
        16 * 64
    );
    assert_eq!(
        snapshot_slot_cost(32, true) - snapshot_slot_cost(16, true),
        16 * 128
    );
}

fn snapshot_string_cost(length: usize, formal: bool) -> u64 {
    let mut runtime = Runtime::new();
    let source = if formal {
        "function take(value){return arguments;}take;"
    } else {
        "function take(){return arguments;}take;"
    };
    let function = runtime.execute(source, &mut NoIo).unwrap();
    let before = report(&runtime);
    let snapshot = runtime
        .invoke(
            function,
            Value::Undefined,
            vec![Value::String(vec![0xd800; length])],
            &mut NoIo,
        )
        .unwrap();
    assert!(matches!(snapshot, Value::Object(_)));
    let after = report(&runtime);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    // Keep the returned snapshot beyond the original call without reading and
    // copying its large string again inside the allocation measurement.
    runtime.set_global("snapshot", snapshot);
    assert_eq!(
        runtime.execute("snapshot.length===1;", &mut NoIo).unwrap(),
        Value::Bool(true)
    );
    report(&runtime);
    after.phases.runtime - before.phases.runtime
}

#[test]
fn snapshot_moves_owned_payloads_but_formal_parameter_copies_stay_charged() {
    assert_eq!(
        snapshot_string_cost(1024, false) - snapshot_string_cost(512, false),
        512 * 2
    );
    assert_eq!(
        snapshot_string_cost(1024, true) - snapshot_string_cost(512, true),
        // Narrow parameter-binding policy: ingress and the actual independent
        // copy remain paid; moving that copy into its binding is not paid again.
        512 * 4
    );
    assert!(snapshot_string_cost(1_500_000, false) >= 3_000_000);
    assert!(snapshot_string_cost(900_000, true) >= 3_600_000);
}

fn slice_cost(count: usize, units: usize) -> u64 {
    let mut runtime = Runtime::new();
    let array = invoke(
        &mut runtime,
        "Array",
        Value::Undefined,
        (0..count)
            .map(|_| Value::String(vec![b'x' as u16; units]))
            .collect(),
    )
    .unwrap();
    let before = report(&runtime);
    let result = invoke(&mut runtime, "Array.slice", array, vec![]).unwrap();
    assert!(matches!(result, Value::Object(_)));
    let after = report(&runtime);
    assert_eq!(after.phases.regex_compile, 0);
    assert_eq!(after.phases.regex_result, 0);
    after.phases.runtime - before.phases.runtime
}

#[test]
fn slice_prepays_new_slots_while_retaining_genuine_string_clone_charges() {
    assert_eq!(slice_cost(4, 8) - slice_cost(2, 8), 2 * (64 + 16));
    assert_eq!(slice_cost(2, 1024) - slice_cost(2, 512), 2 * 512 * 2);
}

fn one_split_cost(length: usize, regex: bool) -> (u64, u64) {
    let mut runtime = Runtime::new();
    let args = if regex {
        vec![
            invoke(
                &mut runtime,
                "RegExp",
                Value::Undefined,
                vec![Value::text("x")],
            )
            .unwrap(),
        ]
    } else {
        vec![]
    };
    let before = report(&runtime);
    let array = invoke(
        &mut runtime,
        "String.split",
        Value::String(vec![b'a' as u16; length]),
        args,
    )
    .unwrap();
    assert!(matches!(array, Value::Object(_)));
    let after = report(&runtime);
    assert_eq!(after.phases.regex_compile, before.phases.regex_compile);
    (
        after.phases.runtime - before.phases.runtime,
        after.phases.regex_result - before.phases.regex_result,
    )
}

#[test]
fn split_result_payloads_are_paid_once_and_keep_plain_versus_regex_phases() {
    // Result counts/capacities remain one while payload length changes. Growing
    // builders may retain unused geometric credits; those are not refunded.
    let small = one_split_cost(512, false);
    let large = one_split_cost(1024, false);
    assert_eq!(large.0 - small.0, 512 * 4);
    assert_eq!((small.1, large.1), (0, 0));
    let small = one_split_cost(512, true);
    let large = one_split_cost(1024, true);
    assert_eq!(large.0 - small.0, 512 * 2);
    assert_eq!(large.1 - small.1, 512 * 2);
    assert!(one_split_cost(900_000, false).0 >= 3_600_000);
}

#[test]
fn a_seventh_maximum_array_fails_before_effects_and_preserves_the_first_charge() {
    let mut runtime = Runtime::new();
    let error = runtime.execute("var made=0,caught=false,finalized=false;try{for(var i=0;i<7;i++){Array(10000);made++;}}catch(e){caught=true;}finally{finalized=true;}",&mut NoIo).unwrap_err();
    assert!(error.contains("allocation budget exhausted"), "{error}");
    assert_eq!(runtime.get_global("made"), Value::Number(6.0));
    assert_eq!(runtime.get_global("caught"), Value::Bool(false));
    assert_eq!(runtime.get_global("finalized"), Value::Bool(false));
    let first = report(&runtime);
    let rejected = first.first_rejected.unwrap();
    assert_eq!(rejected.phase, AllocationPhase::Runtime);
    assert_eq!(rejected.requested_bytes, 10_000 * 64);
    assert_eq!(rejected.accepted_bytes, first.accepted_bytes);
    assert!(rejected.accepted_bytes + rejected.requested_bytes > LIMIT);
    latched(&mut runtime, &error, first);
}

#[test]
fn growing_split_capacity_honors_explicit_limits_and_the_ten_thousand_slot_cap() {
    for count in [3, 4, 5, 257, 10_000] {
        let mut runtime = Runtime::new();
        let result = invoke(
            &mut runtime,
            "String.split",
            Value::String(vec![b'a' as u16; count]),
            vec![Value::text("")],
        )
        .unwrap();
        let allocation = report(&runtime);
        let paid_capacity = count.next_power_of_two().min(10_000);
        assert!(allocation.phases.runtime >= paid_capacity as u64 * 64);
        runtime.set_global("parts", result);
        assert_eq!(
            runtime
                .execute(
                    &format!(
                        "parts.length==={count}&&parts[0]==='a'&&parts[{}]==='a';",
                        count - 1
                    ),
                    &mut NoIo
                )
                .unwrap(),
            Value::Bool(true)
        );
    }
    let mut runtime = Runtime::new();
    let result = invoke(
        &mut runtime,
        "String.split",
        Value::String(vec![b'a' as u16; 10_001]),
        vec![Value::text(""), Value::Number(10_000.0)],
    )
    .unwrap();
    runtime.set_global("parts", result);
    assert_eq!(
        runtime.execute("parts.length===10000;", &mut NoIo).unwrap(),
        Value::Bool(true)
    );
    report(&runtime);

    let mut runtime = Runtime::new();
    let error = invoke(
        &mut runtime,
        "String.split",
        Value::String(vec![b'a' as u16; 10_001]),
        vec![Value::text("")],
    )
    .unwrap_err();
    assert!(error.contains("array limit exhausted"), "{error}");
    let first = report(&runtime);
    latched(&mut runtime, &error, first);
}

#[test]
fn key_result_arrays_keep_enumerability_independence_and_runtime_attribution() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.execute("var original=[,2];var keys=Object.keys(original),names=Object.getOwnPropertyNames(original);var valid=keys.length===1&&keys[0]==='1'&&names.join(',').indexOf('length')>=0;keys[0]='changed';valid&&original[1]===2&&!(0 in original);",&mut NoIo).unwrap(),Value::Bool(true));
    let allocation = report(&runtime);
    assert!(allocation.phases.runtime > 0);
    assert_eq!(allocation.phases.regex_compile, 0);
    assert_eq!(allocation.phases.regex_result, 0);
}
