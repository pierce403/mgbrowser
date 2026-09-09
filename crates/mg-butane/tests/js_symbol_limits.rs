//! Independent local Symbol identity-admission and resource-boundary tests.
//! Public handles are obtained from a Runtime, never manufactured from an ID.

use mg_butane::runtime::{AllocationPhase, AllocationReport, Host, Runtime, Value};

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

fn symbol(runtime: &mut Runtime, description: Option<Value>) -> Result<Value, String> {
    runtime.invoke(
        runtime.get_global("Symbol"),
        Value::Undefined,
        description.into_iter().collect(),
        &mut NoIo,
    )
}

fn latch(runtime: &mut Runtime, first: AllocationReport, error: &str) {
    assert_eq!(
        runtime
            .execute("var forbidden_later_effect = true;", &mut NoIo)
            .unwrap_err(),
        error
    );
    assert_eq!(
        runtime.get_global("forbidden_later_effect"),
        Value::Undefined
    );
    assert_eq!(
        runtime
            .invoke(
                runtime.get_global("Number"),
                Value::Undefined,
                vec![Value::Number(1.0)],
                &mut NoIo,
            )
            .unwrap_err(),
        error
    );
    runtime.set_global("forbidden_ingress", Value::Number(1.0));
    assert_eq!(runtime.get_global("forbidden_ingress"), Value::Undefined);
    assert_eq!(report(runtime), first);
}

fn allocation_failure(runtime: &Runtime, error: &str) -> AllocationReport {
    assert!(error.contains("allocation budget exhausted"), "{error}");
    let first = report(runtime);
    let failure = first.first_rejected.unwrap();
    assert_eq!(failure.phase, AllocationPhase::Runtime);
    assert_eq!(failure.accepted_bytes, first.accepted_bytes);
    assert!(failure.requested_bytes > LIMIT - first.accepted_bytes);
    first
}

#[test]
fn foreign_handles_keep_identity_after_the_originating_runtime_is_dropped() {
    let (first, second) = {
        let mut origin = Runtime::new();
        (
            symbol(&mut origin, Some(Value::text("same"))).unwrap(),
            symbol(&mut origin, Some(Value::text("same"))).unwrap(),
        )
    };
    assert!(matches!(first, Value::Symbol(_)));
    assert_ne!(first, second);
    let mut destination = Runtime::new();
    destination.set_global("first", first.clone());
    destination.set_global("alias", first.clone());
    destination.set_global("second", second.clone());
    assert_eq!(destination.get_global("first"), first);
    assert_eq!(destination.get_global("second"), second);
    assert_eq!(
        destination
            .execute(
                r#"
                var local = Symbol('same'), object = {};
                object[first] = 7;
                object[second] = 8;
                object[local] = 9;
                var keys = Object.getOwnPropertySymbols(object);
                first === alias && first !== second && first !== local &&
                    second !== local && object[alias] === 7 &&
                    keys.length === 3 && keys[0] === first &&
                    keys[1] === second && keys[2] === local;
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    report(&destination);
}

#[test]
fn public_values_remain_send_sync_and_symbols_survive_their_source_thread() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<Value>();

    let (first, alias, second) = std::thread::spawn(|| {
        // Runtime itself stays on its owning thread. Only the immutable public
        // primitive values escape after its local state has been destroyed.
        let mut origin = Runtime::new();
        let first = symbol(&mut origin, Some(Value::text("thread identity"))).unwrap();
        let second = symbol(&mut origin, Some(Value::text("thread identity"))).unwrap();
        (first.clone(), first, second)
    })
    .join()
    .expect("owned Symbol producer thread completes");
    assert_eq!(first, alias);
    assert_ne!(first, second);

    let mut destination = Runtime::new();
    destination.set_global("first", first.clone());
    destination.set_global("alias", alias);
    destination.set_global("second", second);
    assert_eq!(destination.get_global("first"), first);
    assert_eq!(
        destination
            .execute(
                r#"
                var object = {};
                object[first] = 7;
                object[second] = 9;
                var keys = Object.getOwnPropertySymbols(object);
                first === alias && first !== second && object[alias] === 7 &&
                    object[second] === 9 && keys.length === 2 &&
                    keys[0] === first && keys[1] === second;
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert!(report(&destination).first_rejected.is_none());
}

#[test]
fn foreign_registry_membership_is_local_to_the_destination_registry() {
    let foreign = Runtime::new()
        .execute("Symbol.for('registry key');", &mut NoIo)
        .unwrap();
    let mut destination = Runtime::new();
    destination.set_global("foreign", foreign.clone());
    assert_eq!(
        destination
            .execute(
                r#"
                var before = Symbol.keyFor(foreign);
                var local = Symbol.for('registry key');
                before === undefined && Symbol.keyFor(foreign) === undefined &&
                    local !== foreign && Symbol.keyFor(local) === 'registry key';
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(destination.get_global("foreign"), foreign);
}

#[test]
fn foreign_well_known_keys_keep_their_shared_semantic_identity() {
    let (primitive, tag) = {
        let mut origin = Runtime::new();
        (
            origin.execute("Symbol.toPrimitive;", &mut NoIo).unwrap(),
            origin.execute("Symbol.toStringTag;", &mut NoIo).unwrap(),
        )
    };
    let another_primitive = Runtime::new()
        .execute("Symbol.toPrimitive;", &mut NoIo)
        .unwrap();
    assert_eq!(primitive, another_primitive);
    let mut destination = Runtime::new();
    destination.set_global("foreignPrimitive", Value::Undefined);
    let before = report(&destination);
    destination.set_global("foreignPrimitive", primitive.clone());
    let first = report(&destination);
    assert!(first.phases.runtime > before.phases.runtime);
    destination.set_global("foreignPrimitive", primitive);
    assert_eq!(report(&destination), first);
    destination.set_global("foreignPrimitive", another_primitive);
    assert!(report(&destination).phases.runtime > first.phases.runtime);
    destination.set_global("foreignTag", tag);
    assert_eq!(
        destination
            .execute(
                r#"
                var object = {};
                object[foreignPrimitive] = function(hint) { return 9; };
                object[foreignTag] = 'Foreign key';
                foreignPrimitive === Symbol.toPrimitive &&
                    foreignTag === Symbol.toStringTag && +object === 9 &&
                    Object.prototype.toString.call(object) === '[object Foreign key]' &&
                    Object.getOwnPropertySymbols(object)[0] === Symbol.toPrimitive;
                "#,
                &mut NoIo,
            )
            .unwrap(),
        Value::Bool(true)
    );
    // The chosen admission contract charges distinct foreign records once even
    // when their well-known kinds are semantically equal across runtimes.
    assert!(report(&destination).first_rejected.is_none());
}

#[test]
fn public_invoke_admits_the_same_foreign_this_and_argument_identity() {
    let foreign = symbol(&mut Runtime::new(), Some(Value::text("foreign"))).unwrap();
    let mut destination = Runtime::new();
    let callee = destination
        .execute(
            "function compare(argument) { return argument === this.valueOf(); } compare;",
            &mut NoIo,
        )
        .unwrap();
    // Non-strict `this` is boxed inside the call; valueOf must return the same
    // primitive handle, not a symbol newly created from its description.
    assert_eq!(
        destination
            .invoke(callee, foreign.clone(), vec![foreign.clone()], &mut NoIo)
            .unwrap(),
        Value::Bool(true)
    );
    destination.set_global("again", foreign.clone());
    assert_eq!(destination.get_global("again"), foreign);
    report(&destination);
}

fn foreign_admission_cost(description_len: usize) -> (u64, u64) {
    let foreign = symbol(
        &mut Runtime::new(),
        Some(Value::String(vec![0xd800; description_len])),
    )
    .unwrap();
    let mut destination = Runtime::new();
    destination.set_global("slot", Value::Undefined);
    let before = report(&destination);
    destination.set_global("slot", foreign.clone());
    let first = report(&destination);
    destination.set_global("slot", foreign.clone());
    let second = report(&destination);
    assert_eq!(destination.get_global("slot"), foreign);
    assert!(second.first_rejected.is_none());
    assert_eq!(second.phases.source, before.phases.source);
    assert_eq!(second.phases.ast, before.phases.ast);
    (
        first.phases.runtime - before.phases.runtime,
        second.phases.runtime - first.phases.runtime,
    )
}

#[test]
fn first_foreign_admission_charges_description_but_reuse_does_not() {
    let small = foreign_admission_cost(512);
    let large = foreign_admission_cost(1024);
    assert!(large.0 - small.0 >= 512 * 2, "{small:?} -> {large:?}");
    assert_eq!(
        large.1, small.1,
        "reusing a handle must not repay its description"
    );
}

#[test]
fn failed_foreign_global_admission_preserves_the_old_value_and_latches() {
    let foreign = symbol(
        &mut Runtime::new(),
        Some(Value::String(vec![b'x' as u16; 500_000])),
    )
    .unwrap();
    let mut destination = Runtime::new();
    destination.set_global("slot", Value::Number(42.0));
    destination.set_global("padding", Value::String(vec![b'p' as u16; 1_650_000]));
    let before = report(&destination);
    assert!(before.first_rejected.is_none());
    assert!(LIMIT - before.accepted_bytes < 1_000_000);
    destination.set_global("slot", foreign);
    let error = destination
        .execute("var never = true;", &mut NoIo)
        .unwrap_err();
    assert_eq!(destination.get_global("slot"), Value::Number(42.0));
    assert_eq!(destination.get_global("never"), Value::Undefined);
    let first = allocation_failure(&destination, &error);
    assert!(first.first_rejected.unwrap().requested_bytes >= 1_000_000);
    latch(&mut destination, first, &error);
}

fn reflection_cost(description_len: usize) -> u64 {
    let mut runtime = Runtime::new();
    runtime.set_global(
        "description",
        Value::String(vec![b'x' as u16; description_len]),
    );
    let object = runtime
        .execute(
            "var object = {}; for (var i=0; i<8; i++) object[Symbol(description)] = i; object;",
            &mut NoIo,
        )
        .unwrap();
    let reflect = runtime
        .execute("Object.getOwnPropertySymbols;", &mut NoIo)
        .unwrap();
    let before = report(&runtime);
    let result = runtime
        .invoke(reflect, Value::Undefined, vec![object], &mut NoIo)
        .unwrap();
    assert!(matches!(result, Value::Object(_)));
    let after = report(&runtime);
    assert_eq!(after.phases.source, before.phases.source);
    assert_eq!(after.phases.ast, before.phases.ast);
    assert!(after.first_rejected.is_none());
    after.phases.runtime - before.phases.runtime
}

#[test]
fn reflected_handles_do_not_copy_description_payloads() {
    assert_eq!(reflection_cost(512), reflection_cost(1024));
}

#[test]
fn repeated_reflection_consumes_the_existing_budget_and_latches() {
    let mut runtime = Runtime::new();
    let object = runtime
        .execute(
            "var object = {}; for(var i=0;i<128;i++) object[Symbol()] = i; object;",
            &mut NoIo,
        )
        .unwrap();
    let reflect = runtime
        .execute("Object.getOwnPropertySymbols;", &mut NoIo)
        .unwrap();
    let mut previous = report(&runtime);
    for attempt in 0..1024 {
        match runtime.invoke(
            reflect.clone(),
            Value::Undefined,
            vec![object.clone()],
            &mut NoIo,
        ) {
            Ok(value) => {
                assert!(matches!(value, Value::Object(_)));
                let next = report(&runtime);
                assert!(next.accepted_bytes > previous.accepted_bytes);
                assert!(next.first_rejected.is_none());
                previous = next;
            }
            Err(error) => {
                assert!(attempt > 0);
                let first = allocation_failure(&runtime, &error);
                latch(&mut runtime, first, &error);
                return;
            }
        }
    }
    panic!("repeated Symbol reflection did not consume the fixed allocation budget");
}

#[test]
fn cumulative_symbol_creation_is_bounded_and_bypasses_catch_and_finally() {
    let mut runtime = Runtime::new();
    let error = runtime
        .execute(
            r#"
            var made = 0, caught = false, finalized = false, later = false;
            try { while (true) { Symbol(); made++; } }
            catch (error) { caught = true; }
            finally { finalized = true; }
            later = true;
            "#,
            &mut NoIo,
        )
        .unwrap_err();
    assert!(
        error.contains("JavaScript symbol limit exhausted"),
        "{error}"
    );
    // The fixed 10,000-entry identity table includes the two supported
    // well-known symbols. No descriptions or properties are allocated here.
    assert_eq!(runtime.get_global("made"), Value::Number(9_998.0));
    for name in ["caught", "finalized", "later"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    let first = report(&runtime);
    assert!(first.first_rejected.is_none());
    latch(&mut runtime, first, &error);
}

#[test]
fn retained_descriptions_consume_the_budget_without_catch_or_finally_recovery() {
    let mut runtime = Runtime::new();
    runtime.set_global("description", Value::String(vec![0xd800; 1024]));
    let error = runtime
        .execute(
            r#"
            var made = 0, caught = false, finalized = false, later = false;
            try { while (true) { Symbol(description); made++; } }
            catch (error) { caught = true; }
            finally { finalized = true; }
            later = true;
            "#,
            &mut NoIo,
        )
        .unwrap_err();
    match runtime.get_global("made") {
        Value::Number(made) => assert!(made > 0.0 && made < 10_000.0, "made {made}"),
        value => panic!("missing creation count: {value:?}"),
    }
    for name in ["caught", "finalized", "later"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    let first = allocation_failure(&runtime, &error);
    latch(&mut runtime, first, &error);
}

#[test]
fn a_failed_foreign_argument_admission_prevents_the_user_function_body() {
    let foreign = symbol(
        &mut Runtime::new(),
        Some(Value::String(vec![b'x' as u16; 500_000])),
    )
    .unwrap();
    let mut destination = Runtime::new();
    let callee = destination
        .execute(
            "var entered=false; function take(value){entered=true;} take;",
            &mut NoIo,
        )
        .unwrap();
    destination.set_global("padding", Value::String(vec![b'p' as u16; 1_650_000]));
    assert!(report(&destination).first_rejected.is_none());
    let error = destination
        .invoke(callee, Value::Undefined, vec![foreign], &mut NoIo)
        .unwrap_err();
    assert_eq!(destination.get_global("entered"), Value::Bool(false));
    let first = allocation_failure(&destination, &error);
    latch(&mut destination, first, &error);
}

#[test]
fn worker_style_description_builder_finishes_before_symbol_storage_exhaustion() {
    let mut runtime = Runtime::new();
    assert_eq!(
        runtime
            .execute(
                "var description=Array(10000).join('abcdefgh'); description.length;",
                &mut NoIo,
            )
            .unwrap(),
        Value::Number(79_992.0)
    );
    let built = report(&runtime);
    assert!(built.first_rejected.is_none());
    let error = runtime
        .execute(
            r#"
            var made=0, caught=false, finalized=false, later=false;
            try { for(var i=0;i<100;i++){Symbol(description);made++;} }
            catch(error) { caught=true; }
            finally { finalized=true; }
            later=true;
            "#,
            &mut NoIo,
        )
        .unwrap_err();
    let made = runtime.get_global("made");
    match made {
        Value::Number(count) => assert!(count > 0.0 && count < 100.0, "made {count}"),
        ref value => panic!("missing successful Symbol count: {value:?}"),
    }
    for name in ["caught", "finalized", "later"] {
        assert_eq!(runtime.get_global(name), Value::Bool(false), "{name}");
    }
    let first = allocation_failure(&runtime, &error);
    eprintln!("Symbol description checkpoint: built={built:?}, made={made:?}, failed={first:?}");
    assert!(first.phases.runtime > built.phases.runtime);
    latch(&mut runtime, first, &error);
}
