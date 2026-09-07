//! Authored typed-prototype resource and admission checks. No website source,
//! alternate engine, unsafe allocation probe or enlarged thread stack is used.

use mg_deps::js::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

const LIMIT: u64 = 4 * 1024 * 1024;

struct NoIo;

impl Host for NoIo {
    fn get(&mut self, object: &str, key: &str) -> Result<Value, String> {
        panic!("unexpected host get: {object}.{key}");
    }
    fn set(&mut self, object: &str, key: &str, _: Value) -> Result<(), String> {
        panic!("unexpected host set: {object}.{key}");
    }
    fn call(&mut self, name: &str, _: Value, _: Vec<Value>) -> Result<Value, String> {
        panic!("unexpected host call: {name}");
    }
}

fn report(runtime: &Runtime) -> AllocationReport {
    let report = runtime.allocation_report();
    assert!(report.is_valid(), "{report:?}");
    assert_eq!(report.limit_bytes, LIMIT);
    assert!(report.accepted_bytes <= LIMIT);
    report
}

fn latch(runtime: &mut Runtime, error: &str, first: AllocationReport) {
    assert_eq!(
        runtime
            .execute("var forbidden_later=true;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(runtime.get_global("forbidden_later"), Value::Undefined);
    assert_eq!(
        runtime
            .invoke(
                runtime.get_global("Number"),
                Value::Undefined,
                vec![],
                &mut NoIo
            )
            .unwrap_err(),
        error
    );
    runtime.set_global(
        "forbidden_ingress",
        Value::Native("unused new native".into()),
    );
    assert_eq!(runtime.get_global("forbidden_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn allocation_failure(runtime: &Runtime, error: &str) -> AllocationReport {
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let report = report(runtime);
    let failure = report.first_rejected.unwrap();
    assert_eq!(failure.phase, AllocationPhase::Runtime);
    assert!(failure.requested_bytes > LIMIT - report.accepted_bytes);
    report
}

fn create(runtime: &mut Runtime, prototype: Value) -> Result<Value, String> {
    runtime.invoke(
        Value::Native("Object.create".into()),
        Value::Undefined,
        vec![prototype],
        &mut NoIo,
    )
}

fn prototype(runtime: &mut Runtime, object: Value) -> Result<Value, String> {
    runtime.invoke(
        Value::Native("Object.getPrototypeOf".into()),
        Value::Undefined,
        vec![object],
        &mut NoIo,
    )
}

fn warm_identity_helpers(runtime: &mut Runtime) {
    let object = create(runtime, Value::Null).unwrap();
    assert_eq!(prototype(runtime, object).unwrap(), Value::Null);
}

fn leave_budget(runtime: &mut Runtime, desired_remaining: u64) {
    // Keep a full KiB of slack for the existing global property's metadata.
    // This is an ordinary charged public string ingress, not a budget bypass.
    let remaining = LIMIT - report(runtime).accepted_bytes;
    assert!(remaining > desired_remaining + 1024);
    let units = ((remaining - desired_remaining - 1024) / 2) as usize;
    runtime.set_global("padding", Value::String(vec![b'p' as u16; units]));
    let after = report(runtime);
    assert!(after.first_rejected.is_none());
    assert!(LIMIT - after.accepted_bytes >= desired_remaining);
    assert!(LIMIT - after.accepted_bytes <= desired_remaining + 1024);
}

#[test]
fn public_native_names_reuse_identity_without_enabling_unknown_calls() {
    let mut runtime = Runtime::new();
    let name = "independently authored unknown native";
    let first = create(&mut runtime, Value::Native(name.into())).unwrap();
    let second = create(&mut runtime, Value::Native(name.into())).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        prototype(&mut runtime, first.clone()).unwrap(),
        Value::Native(name.into())
    );
    assert_eq!(
        prototype(&mut runtime, second.clone()).unwrap(),
        Value::Native(name.into())
    );
    runtime.set_global("first", first);
    runtime.set_global("second", second);
    assert_eq!(runtime.execute(
        "var owner=Object.getPrototypeOf(first);owner.visible=7;var key=Symbol('key');owner[key]=9;Object.getPrototypeOf(first)===Object.getPrototypeOf(second)&&second.visible===7&&second[key]===9;",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
    let error = runtime
        .invoke(
            Value::Native(name.into()),
            Value::Undefined,
            vec![],
            &mut NoIo,
        )
        .unwrap_err();
    assert!(error.contains("Unsupported JavaScript behavior"), "{error}");
    assert_eq!(
        runtime
            .execute("first.visible+second.visible;", &mut NoIo)
            .unwrap(),
        Value::Number(14.0)
    );
    assert!(report(&runtime).first_rejected.is_none());
}

fn native_identity_cost(length: usize) -> (u64, u64) {
    let mut runtime = Runtime::new();
    warm_identity_helpers(&mut runtime);
    let name = "x".repeat(length);
    let before = report(&runtime);
    create(&mut runtime, Value::Native(name.clone())).unwrap();
    let first = report(&runtime);
    create(&mut runtime, Value::Native(name)).unwrap();
    let second = report(&runtime);
    assert!(second.first_rejected.is_none());
    assert_eq!(second.phases.source, before.phases.source);
    assert_eq!(second.phases.ast, before.phases.ast);
    (
        first.phases.runtime - before.phases.runtime,
        second.phases.runtime - first.phases.runtime,
    )
}

#[test]
fn native_identity_retains_one_utf8_name_beyond_existing_ingress_and_copy_costs() {
    let small = native_identity_cost(512);
    let large = native_identity_cost(1024);
    assert!(
        small.0 > small.1 && large.0 > large.1,
        "{small:?}, {large:?}"
    );
    assert_eq!((large.0 - large.1) - (small.0 - small.1), 512);
    // Every public invocation still pays its actual name ingress/copies; reuse
    // eliminates only the separately retained native identity/name allocation.
    assert!(large.1 - small.1 >= 512);
}

fn returned_native_name_cost(length: usize) -> u64 {
    let mut runtime = Runtime::new();
    warm_identity_helpers(&mut runtime);
    let name = "x".repeat(length);
    let child = create(&mut runtime, Value::Native(name.clone())).unwrap();
    let before = report(&runtime);
    assert_eq!(prototype(&mut runtime, child).unwrap(), Value::Native(name));
    let after = report(&runtime);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    after.phases.runtime - before.phases.runtime
}

#[test]
fn get_prototype_of_charges_the_genuine_returned_native_name_copy() {
    assert_eq!(
        returned_native_name_cost(1024) - returned_native_name_cost(512),
        512
    );
}

#[test]
fn native_name_storage_rejects_before_new_identity_retention_when_budget_is_short() {
    let mut runtime = Runtime::new();
    warm_identity_helpers(&mut runtime);
    let length = 8192;
    // Existing argument ingress and native-dispatch first-argument copy fit;
    // the additional retained native name does not. No object is returned.
    leave_budget(&mut runtime, (length * 2 + length / 2) as u64);
    let error = create(&mut runtime, Value::Native("x".repeat(length))).unwrap_err();
    let first = allocation_failure(&runtime, &error);
    assert_eq!(
        first.first_rejected.unwrap().requested_bytes,
        (length + 64) as u64
    );
    latch(&mut runtime, &error, first);
}

#[test]
fn oversized_public_native_ingress_preserves_existing_state_and_latches() {
    let mut runtime = Runtime::new();
    runtime.set_global("slot", Value::Number(42.0));
    runtime.set_global("slot", Value::Native("x".repeat(LIMIT as usize + 1)));
    let error = runtime.execute("var never=true;", &mut NoIo).unwrap_err();
    assert_eq!(runtime.get_global("slot"), Value::Number(42.0));
    assert_eq!(runtime.get_global("never"), Value::Undefined);
    let first = allocation_failure(&runtime, &error);
    assert_eq!(first.first_rejected.unwrap().requested_bytes, LIMIT + 1);
    latch(&mut runtime, &error, first);
}

#[test]
fn returning_an_existing_long_native_prototype_preflights_its_real_copy() {
    let mut runtime = Runtime::new();
    warm_identity_helpers(&mut runtime);
    let length = 120_000;
    let child = create(&mut runtime, Value::Native("x".repeat(length))).unwrap();
    leave_budget(&mut runtime, (length / 2) as u64);
    let error = prototype(&mut runtime, child).unwrap_err();
    let first = allocation_failure(&runtime, &error);
    assert_eq!(first.first_rejected.unwrap().requested_bytes, length as u64);
    latch(&mut runtime, &error, first);
}

#[test]
fn long_native_virtual_name_reads_are_charged_and_bounded() {
    let mut runtime = Runtime::new();
    let length = 120_000;
    let child = create(&mut runtime, Value::Native("x".repeat(length))).unwrap();
    runtime.set_global("child", child);
    assert_eq!(
        runtime.execute("child.name.length;", &mut NoIo).unwrap(),
        Value::Number(length as f64)
    );
    leave_budget(&mut runtime, (length / 2) as u64);
    let error = runtime
        .execute("var not_returned=child.name;", &mut NoIo)
        .unwrap_err();
    let first = allocation_failure(&runtime, &error);
    assert!(first.first_rejected.unwrap().requested_bytes >= length as u64);
    assert_eq!(runtime.get_global("not_returned"), Value::Undefined);
    latch(&mut runtime, &error, first);
}

#[test]
fn prototype_identity_retains_the_function_and_its_closed_over_environment() {
    let mut runtime = Runtime::new();
    assert_eq!(runtime.execute(
        "function build(){var secret=17;function Parent(){return secret;}Parent.read=function(){return secret;};return Object.create(Parent);}var child=build();build=0;child.read();",
        &mut NoIo,
    ).unwrap(), Value::Number(17.0));
    let parent = runtime
        .execute("Object.getPrototypeOf(child);", &mut NoIo)
        .unwrap();
    assert!(matches!(parent, Value::Function(_)));
    assert_eq!(
        runtime
            .invoke(parent.clone(), Value::Undefined, vec![], &mut NoIo)
            .unwrap(),
        Value::Number(17.0)
    );
    assert_eq!(runtime.execute("var alias=eval('Object.getPrototypeOf(child)');alias===Object.getPrototypeOf(child)&&child.read()===17;", &mut NoIo).unwrap(), Value::Bool(true));
    assert_eq!(runtime.get_global("alias"), parent);
    assert!(report(&runtime).first_rejected.is_none());
}

#[test]
fn shallow_typed_prototype_chains_remain_usable() {
    for seed in ["Base", "Array"] {
        let mut runtime = Runtime::new();
        let source = format!(
            "function Base(){{}}var seed={seed};seed.present=17;var key=Symbol('marker');seed[key]=19;var tail=seed;for(var i=0;i<48;i++){{tail=Object.create(tail);}}tail.present===17&&tail[key]===19&&'present' in tail&&key in tail;"
        );
        assert_eq!(
            runtime.execute(&source, &mut NoIo).unwrap(),
            Value::Bool(true),
            "{seed}"
        );
        assert!(report(&runtime).first_rejected.is_none());
    }
}

#[test]
fn long_typed_prototype_traversals_fail_without_partial_completion_or_handlers() {
    for seed in ["Base", "Array"] {
        for operation in [
            "tail.present;visited=true;",
            "tail.present=0;visited=true;",
            "tail[key];visited=true;",
            "tail[key]=0;visited=true;",
            "'present' in tail;visited=true;",
            "key in tail;visited=true;",
            "for(var name in tail){visited=true;}",
            "tail instanceof C;visited=true;",
        ] {
            let mut runtime = Runtime::new();
            runtime.execute(&format!(
                "function Base(){{}}function C(){{}}var seed={seed};C.prototype=seed;seed.present=17;var key=Symbol('marker');seed[key]=19;var tail=seed;for(var i=0;i<70;i++){{tail=Object.create(tail);}}"
            ), &mut NoIo).unwrap();
            let error = runtime.execute(&format!(
                "var visited=false,caught=false,finalized=false,later=false;try{{{operation}}}catch(e){{caught=true;}}finally{{finalized=true;}}later=true;"
            ), &mut NoIo).unwrap_err();
            assert!(
                error.contains("prototype depth limit"),
                "{seed}: {operation}: {error}"
            );
            for flag in ["visited", "caught", "finalized", "later"] {
                assert_eq!(
                    runtime.get_global(flag),
                    Value::Bool(false),
                    "{seed}: {operation}: {flag}"
                );
            }
            let first = report(&runtime);
            assert!(first.first_rejected.is_none());
            latch(&mut runtime, &error, first);
        }
    }
}

#[test]
fn repeated_typed_reads_exhaust_shared_fuel_without_catch_or_finally_recovery() {
    for seed in ["Base", "Array"] {
        let mut runtime = Runtime::new();
        let error = runtime.execute(&format!(
            "function Base(){{}}var seed={seed};seed.present=17;var tail=Object.create(seed);var caught=false,finalized=false,later=false;try{{while(true){{tail.present;}}}}catch(e){{caught=true;}}finally{{finalized=true;}}later=true;"
        ), &mut NoIo).unwrap_err();
        assert!(
            error.contains("JavaScript fuel exhausted"),
            "{seed}: {error}"
        );
        for flag in ["caught", "finalized", "later"] {
            assert_eq!(
                runtime.get_global(flag),
                Value::Bool(false),
                "{seed}: {flag}"
            );
        }
        let first = report(&runtime);
        assert!(first.first_rejected.is_none());
        latch(&mut runtime, &error, first);
    }
}

#[test]
fn stable_native_identity_reuse_still_consumes_the_existing_object_limit() {
    let mut runtime = Runtime::new();
    create(&mut runtime, Value::Native("Array".into())).unwrap();
    for count in 0..10_001 {
        match create(&mut runtime, Value::Native("Array".into())) {
            Ok(value) => assert!(matches!(value, Value::Object(_))),
            Err(error) => {
                assert!(
                    error.contains("JavaScript object limit exhausted"),
                    "after {count}: {error}"
                );
                assert!(count > 9000 && count < 10_000, "made {count}");
                let first = report(&runtime);
                assert!(first.first_rejected.is_none());
                latch(&mut runtime, &error, first);
                return;
            }
        }
    }
    panic!("typed native prototypes bypassed the existing object cap");
}

fn ordinary_chain(edges: usize) -> Runtime {
    let mut runtime = Runtime::new();
    runtime.execute(&format!(
        "var base=Object.create(null);base.present=17;var key=Symbol('present'),missingKey=Symbol('missing');base[key]=19;var tail=base;for(var i=0;i<{edges};i++){{tail=Object.create(tail);}}"
    ), &mut NoIo).unwrap();
    runtime
}

fn ordinary_depth_failure(edges: usize, operation: &str) {
    let mut runtime = ordinary_chain(edges);
    let error = runtime.execute(operation, &mut NoIo).unwrap_err();
    assert!(
        error.contains("prototype depth limit"),
        "{edges}: {operation}: {error}"
    );
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, &error, first);
}

#[test]
fn historical_string_reads_keep_the_root_plus_sixty_four_ancestors_boundary() {
    // These are deliberately preserved pre-increment policy differences, not
    // an assertion that ECMAScript imposes a uniform prototype depth limit.
    assert_eq!(
        ordinary_chain(64)
            .execute("tail.present;", &mut NoIo)
            .unwrap(),
        Value::Number(17.0)
    );
    assert_eq!(
        ordinary_chain(63)
            .execute("tail.absent;", &mut NoIo)
            .unwrap(),
        Value::Undefined
    );
    ordinary_depth_failure(64, "tail.absent;");
    ordinary_depth_failure(65, "tail.present;");
}

#[test]
fn historical_symbol_and_in_reads_keep_the_sixty_four_owners_boundary() {
    assert_eq!(
        ordinary_chain(63)
            .execute("tail[key]===19&&'present' in tail&&key in tail;", &mut NoIo,)
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        ordinary_chain(62)
            .execute(
                "tail[missingKey]===undefined&&!('absent' in tail)&&!(missingKey in tail);",
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    for operation in [
        "tail[missingKey];",
        "'absent' in tail;",
        "missingKey in tail;",
    ] {
        ordinary_depth_failure(63, operation);
    }
    for operation in ["tail[key];", "'present' in tail;", "key in tail;"] {
        ordinary_depth_failure(64, operation);
    }
}

#[test]
fn historical_terminal_none_write_and_enumeration_policies_remain_distinct() {
    assert_eq!(
        ordinary_chain(63)
            .execute(
                "tail.absent=23;tail[key]=29;tail.absent===23&&tail[key]===29&&base[key]===19;",
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        ordinary_chain(62)
            .execute("tail[missingKey]=23;tail[missingKey];", &mut NoIo,)
            .unwrap(),
        Value::Number(23.0)
    );
    ordinary_depth_failure(63, "tail[missingKey]=23;");
    ordinary_depth_failure(64, "tail.absent=23;");
    assert_eq!(
        ordinary_chain(63)
            .execute(
                "var seen='';for(var name in tail){seen+=name;}seen;",
                &mut NoIo,
            )
            .unwrap(),
        Value::text("present")
    );
    ordinary_depth_failure(64, "for(var name in tail){}");
}

#[test]
fn own_only_inspection_and_deletion_do_not_walk_an_overdeep_prototype_chain() {
    assert_eq!(ordinary_chain(70).execute(
        "Object.keys(tail).length===0&&Object.getOwnPropertyNames(tail).length===0&&Object.getOwnPropertySymbols(tail).length===0&&!Object.prototype.hasOwnProperty.call(tail,'present')&&!Object.prototype.hasOwnProperty.call(tail,key)&&(delete tail.present)&&(delete tail[key]);",
        &mut NoIo,
    ).unwrap(), Value::Bool(true));
}

#[test]
fn historical_instanceof_checks_keep_the_sixty_four_ancestors_boundary() {
    assert_eq!(
        ordinary_chain(64)
            .execute(
                "function C(){}C.prototype=base;tail instanceof C;",
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        ordinary_chain(63)
            .execute("function C(){}tail instanceof C;", &mut NoIo,)
            .unwrap(),
        Value::Bool(false)
    );
    ordinary_depth_failure(65, "function C(){}C.prototype=base;tail instanceof C;");
    ordinary_depth_failure(64, "function C(){}tail instanceof C;");
}
