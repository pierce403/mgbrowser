#![cfg(feature = "modern")]

use mg_butane::modern::{
    CUMULATIVE_JOB_LIMIT, CUMULATIVE_SOURCE_LIMIT, Engine, OPCODE_LIMIT, PENDING_JOB_LIMIT,
    SOURCE_LIMIT, WORKER_CUMULATIVE_LIMIT, WORKER_OUTSTANDING_LIMIT, WorkerMemory,
    boa_engine::{JsValue, NativeFunction, Source, js_string},
    boa_gc, check_context,
};
use std::{cell::Cell, rc::Rc};

#[test]
fn modern_language_and_cumulative_script_accounting() {
    let mut engine = Engine::new().unwrap();
    let first = engine.stats();
    assert_eq!(first.opcodes_remaining, OPCODE_LIMIT);
    assert_eq!(first.source_bytes, 0);
    assert_eq!(first.worker_memory, None);
    let source = "let total = 0; for (let n of [1,2,3]) total += n; total;";
    assert_eq!(engine.evaluate(source).unwrap().as_number(), Some(6.0));
    let second = engine.stats();
    assert_eq!(second.source_bytes, source.len());
    assert_eq!(second.source_admissions, 1);
    assert!(second.opcodes_remaining < first.opcodes_remaining);
    second.validate_after(&first).unwrap();
    assert_eq!(
        engine.evaluate("total += 4;").unwrap().as_number(),
        Some(10.0)
    );
    let third = engine.stats();
    third.validate_after(&second).unwrap();
    assert!(third.opcodes_remaining < second.opcodes_remaining);
    assert_eq!(third.source_admissions, 2);
}

#[test]
fn native_reentry_and_forced_collection_share_the_same_opcode_budget() {
    let mut engine = Engine::new().unwrap();
    engine
        .context_mut()
        .register_global_builtin_callable(
            js_string!("reenter"),
            0,
            NativeFunction::from_fn_ptr(|_, _, context| {
                check_context(context)?;
                boa_gc::force_collect();
                context.eval(Source::from_bytes("counter += 40;"))
            }),
        )
        .unwrap();
    let before = engine.stats().opcodes_remaining;
    assert_eq!(
        engine
            .evaluate("let counter = 2; reenter(); counter;")
            .unwrap()
            .as_number(),
        Some(42.0)
    );
    assert!(engine.stats().opcodes_remaining < before);
    engine.evaluate("reenter();").unwrap();
    assert_eq!(engine.evaluate("counter;").unwrap().as_number(), Some(82.0));
}

#[test]
fn promise_checkpoint_is_fifo_and_does_not_run_before_checkpoint() {
    let mut engine = Engine::new().unwrap();
    engine.evaluate("var order = 'sync'; Promise.resolve().then(() => {order += ',first'; Promise.resolve().then(() => order += ',nested');}); Promise.resolve().then(() => order += ',second');").unwrap();
    assert_eq!(
        engine
            .evaluate("order;")
            .unwrap()
            .as_string()
            .unwrap()
            .to_std_string_escaped(),
        "sync"
    );
    assert_eq!(engine.stats().pending_jobs, 2);
    engine.checkpoint().unwrap();
    assert_eq!(
        engine
            .evaluate("order;")
            .unwrap()
            .as_string()
            .unwrap()
            .to_std_string_escaped(),
        "sync,first,second,nested"
    );
    assert_eq!(engine.stats().jobs_admitted, 3);
    assert_eq!(engine.stats().jobs_executed, 3);
    assert_eq!(engine.stats().pending_jobs, 0);
    engine.stats().validate().unwrap();
}

#[test]
fn checkpoint_releases_weakref_kept_objects_only_after_jobs_finish() {
    let mut engine = Engine::new().unwrap();
    engine.evaluate("var observed = 0; var weak = (() => { const target = {n: 42}; return new WeakRef(target); })(); Promise.resolve().then(() => { observed = weak.deref().n; });").unwrap();
    // A host-forced collection during the current job must not invalidate the
    // WeakRef constructor's kept target before its microtask checkpoint.
    boa_gc::force_collect();
    engine.checkpoint().unwrap();
    assert_eq!(
        engine.get_global("observed").unwrap().as_number(),
        Some(42.0)
    );
    boa_gc::force_collect();
    assert_eq!(
        engine
            .evaluate("weak.deref() === undefined;")
            .unwrap()
            .as_boolean(),
        Some(true)
    );
}

#[test]
fn real_global_reads_ignore_reassigned_alias_and_guard_hostile_getters() {
    let mut engine = Engine::new().unwrap();
    engine
        .evaluate("var answer = 42; globalThis = { answer: 99 }; ")
        .unwrap();
    assert_eq!(engine.get_global("answer").unwrap().as_number(), Some(42.0));
    engine
        .evaluate("Object.defineProperty(this, 'endless', {get() { while (true) {} }});")
        .unwrap();
    assert!(engine.get_global("endless").unwrap_err().is_fatal());
    assert!(engine.is_fatal());
    assert!(engine.get_global("answer").unwrap_err().is_fatal());
}

#[test]
fn endless_promise_jobs_terminate_cooperatively_and_latch() {
    let mut engine = Engine::new().unwrap();
    engine
        .evaluate("function again() { Promise.resolve().then(again); } again();")
        .unwrap();
    let failure = engine.checkpoint().unwrap_err();
    assert!(failure.is_fatal());
    assert!(engine.is_fatal());
    assert_eq!(engine.stats().jobs_admitted, CUMULATIVE_JOB_LIMIT);
    assert!(engine.stats().opcodes_remaining > 0);
    assert!(failure.message.contains("job"));
    assert_eq!(engine.stats().pending_jobs, 0);
    let failed = engine.stats();
    assert!(engine.evaluate("42;").unwrap_err().is_fatal());
    assert!(engine.checkpoint().unwrap_err().is_fatal());
    assert_eq!(engine.stats(), failed);
}

#[test]
fn queue_overflow_latches_before_later_host_effects() {
    let mut engine = Engine::new().unwrap();
    let effects = Rc::new(Cell::new(0usize));
    engine.context_mut().insert_data(effects.clone());
    engine
        .context_mut()
        .register_global_builtin_callable(
            js_string!("effect"),
            0,
            NativeFunction::from_fn_ptr(|_, _, context| {
                check_context(context)?;
                let effects = context.get_data::<Rc<Cell<usize>>>().unwrap();
                effects.set(effects.get() + 1);
                Ok(JsValue::undefined())
            }),
        )
        .unwrap();
    let failure = engine.evaluate("try { for (let n = 0; n < 257; n++) Promise.resolve().then(() => effect()); effect(); } catch (e) { effect(); }").unwrap_err();
    assert!(failure.is_fatal());
    assert_eq!(effects.get(), 0);
    assert_eq!(engine.stats().jobs_admitted, PENDING_JOB_LIMIT);
    assert_eq!(engine.stats().jobs_executed, 0);
    assert_eq!(engine.stats().pending_jobs, 0);
}

#[test]
fn opcode_limit_is_not_catchable_and_never_renewed() {
    let mut engine = Engine::new().unwrap();
    let failure = engine
        .evaluate("var caught = false; try { for (;;) {} } catch (e) { caught = true; }")
        .unwrap_err();
    assert!(failure.is_fatal());
    assert_eq!(engine.stats().opcodes_remaining, 0);
    assert_eq!(
        engine
            .context_mut()
            .global_object()
            .get(js_string!("caught"), engine.context_mut())
            .unwrap()
            .as_boolean(),
        Some(false)
    );
    assert!(engine.evaluate("caught = true;").unwrap_err().is_fatal());
}

#[test]
fn static_source_admission_is_cumulative_and_failure_is_fatal() {
    let mut engine = Engine::new().unwrap();
    let source = " ".repeat(SOURCE_LIMIT);
    for _ in 0..(CUMULATIVE_SOURCE_LIMIT / SOURCE_LIMIT) {
        engine.evaluate(&source).unwrap();
    }
    assert_eq!(engine.stats().source_bytes, CUMULATIVE_SOURCE_LIMIT);
    assert!(engine.evaluate("1;").unwrap_err().is_fatal());
    assert_eq!(engine.stats().source_bytes, CUMULATIVE_SOURCE_LIMIT);
}

#[test]
fn dynamic_eval_and_function_fragments_share_source_admission() {
    let mut engine = Engine::new().unwrap();
    let source = "eval('21 + 21'); new Function('a', 'return a + 1')(41);";
    assert_eq!(engine.evaluate(source).unwrap().as_number(), Some(42.0));
    assert_eq!(engine.stats().source_admissions, 3);
    assert!(engine.stats().source_bytes > source.len());
    // Dynamic UTF-16 units are charged without allocating a UTF-8 copy.
    let failure = engine
        .evaluate(
            "var caught = false; try { eval(' '.repeat(524289)); } catch (e) { caught = true; }",
        )
        .unwrap_err();
    assert!(failure.is_fatal());
    assert!(failure.message.contains("source"));
    assert_eq!(
        engine
            .context_mut()
            .global_object()
            .get(js_string!("caught"), engine.context_mut())
            .unwrap()
            .as_boolean(),
        Some(false)
    );
}

#[test]
fn diagnostics_do_not_invoke_getters_proxies_or_coercion() {
    let mut engine = Engine::new().unwrap();
    engine.evaluate("var sideEffects = 0;").unwrap();
    let error = engine.evaluate("throw new Proxy({get name() {sideEffects++;}, get message() {sideEffects++;}, toString() {sideEffects++;}}, {get() { sideEffects++; throw 1; }, getPrototypeOf() {sideEffects++; throw 2;}});").unwrap_err();
    assert!(!error.is_fatal());
    assert_eq!(error.kind, "ThrownValue");
    assert!(error.to_string().len() < 1024);
    assert_eq!(
        engine.evaluate("sideEffects;").unwrap().as_number(),
        Some(0.0)
    );
    assert!(!engine.is_fatal());
}

#[test]
fn ordinary_exceptions_are_not_fatal_and_reports_reject_renewal() {
    let mut engine = Engine::new().unwrap();
    assert!(!engine.evaluate("notDeclared;").unwrap_err().is_fatal());
    assert_eq!(engine.evaluate("42;").unwrap().as_number(), Some(42.0));
    let current = engine.stats();
    let mut forged = current.clone();
    forged.opcodes_remaining += 1;
    assert!(forged.validate_after(&current).is_err());
    forged = current.clone();
    forged.jobs_executed = 1;
    assert!(forged.validate().is_err());
    forged = current.clone();
    forged.source_admissions = forged.source_bytes + 1;
    assert!(forged.validate().is_err());
    forged = current.clone();
    forged.opcodes_remaining = 0;
    assert!(forged.fatal_reason.is_none());
    assert!(forged.validate().is_err());
    forged = current.clone();
    forged.fatal_reason = Some("terminated".into());
    forged.jobs_admitted = 1;
    forged.pending_jobs = 1;
    assert!(forged.validate().is_err());
    let memory = WorkerMemory {
        outstanding_bytes: 16,
        peak_bytes: 32,
        cumulative_bytes: 64,
        allocations: 1,
        outstanding_limit_bytes: WORKER_OUTSTANDING_LIMIT,
        cumulative_limit_bytes: WORKER_CUMULATIVE_LIMIT,
    };
    assert!(memory.is_valid());
    let mut report = current.clone();
    report.worker_memory = Some(memory);
    assert!(report.is_valid());
    assert!(current.validate_after(&report).is_err());
    report.worker_memory.as_mut().unwrap().peak_bytes = WORKER_OUTSTANDING_LIMIT + 1;
    assert!(!report.is_valid());
}
