#![cfg(feature = "modern")]

use mg_butane::modern::{
    CUMULATIVE_JOB_LIMIT, CUMULATIVE_SOURCE_LIMIT, Engine, OPCODE_LIMIT, PENDING_JOB_LIMIT,
    SOURCE_LIMIT, WORKER_CUMULATIVE_LIMIT, WORKER_OUTSTANDING_LIMIT, WorkPermit, WorkerMemory,
    admit_host_task, begin_host_task,
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

#[test]
fn promise_and_host_tasks_share_one_pending_limit_in_both_directions() {
    for host_count in [0, 1, 128, 255, 256] {
        let mut engine = Engine::new().unwrap();
        let permits: Vec<_> = (0..host_count)
            .map(|_| admit_host_task(engine.context_mut()).unwrap())
            .collect();
        let promises = PENDING_JOB_LIMIT - host_count;
        engine
            .evaluate(&format!(
                "for(let i=0;i<{promises};i++) Promise.resolve().then(()=>0);"
            ))
            .unwrap();
        assert_eq!(engine.stats().pending_jobs, PENDING_JOB_LIMIT);
        assert_eq!(engine.stats().jobs_admitted, PENDING_JOB_LIMIT);
        if host_count % 2 == 0 {
            assert!(admit_host_task(engine.context_mut()).is_err());
        } else {
            assert!(engine.evaluate("Promise.resolve().then(()=>0);").is_err());
        }
        assert!(engine.check().unwrap_err().is_fatal());
        assert_eq!(engine.stats().pending_jobs, 0);
        assert_eq!(engine.stats().jobs_admitted, PENDING_JOB_LIMIT);
        assert_eq!(engine.stats().jobs_executed, 0);
        // Every remaining permit has an expired epoch. Dropping it must not
        // double-release the already-cleared pending ledger.
        drop(permits);
        engine.stats().validate().unwrap();
    }
}

#[test]
fn cancelled_host_admissions_and_promises_share_the_lifetime_limit() {
    let mut engine = Engine::new().unwrap();
    for _ in 0..(CUMULATIVE_JOB_LIMIT / 2) {
        admit_host_task(engine.context_mut()).unwrap().cancel();
    }
    let canceled = engine.stats();
    assert_eq!(canceled.pending_jobs, 0);
    assert_eq!(canceled.jobs_admitted, CUMULATIVE_JOB_LIMIT / 2);
    assert_eq!(canceled.jobs_executed, 0);
    for _ in 0..4 {
        engine
            .evaluate("for(let i=0;i<256;i++) Promise.resolve().then(()=>0);")
            .unwrap();
        engine.checkpoint().unwrap();
    }
    let exhausted = engine.stats();
    assert_eq!(exhausted.jobs_admitted, CUMULATIVE_JOB_LIMIT);
    assert_eq!(exhausted.jobs_executed, CUMULATIVE_JOB_LIMIT / 2);
    assert_eq!(exhausted.pending_jobs, 0);
    exhausted.validate_after(&canceled).unwrap();
    assert!(admit_host_task(engine.context_mut()).is_err());
    assert!(engine.check().unwrap_err().is_fatal());
    assert_eq!(engine.stats().jobs_admitted, CUMULATIVE_JOB_LIMIT);
    assert_eq!(engine.stats().jobs_executed, exhausted.jobs_executed);
    assert_eq!(engine.stats().source_bytes, exhausted.source_bytes);
    assert_eq!(
        engine.stats().opcodes_remaining,
        exhausted.opcodes_remaining
    );
}

#[test]
fn beginning_host_work_frees_one_slot_without_running_a_callback_or_checkpoint() {
    let mut engine = Engine::new().unwrap();
    let mut permits: Vec<_> = (0..PENDING_JOB_LIMIT)
        .map(|_| admit_host_task(engine.context_mut()).unwrap())
        .collect();
    let callback = engine
        .evaluate("var order='sync'; (()=>{order+=',task';Promise.resolve().then(()=>order+=',microtask')})")
        .unwrap()
        .as_object()
        .unwrap();
    begin_host_task(engine.context_mut(), &mut permits[0]).unwrap();
    assert_eq!(engine.stats().pending_jobs, PENDING_JOB_LIMIT - 1);
    assert_eq!(engine.stats().jobs_executed, 1);
    assert_eq!(
        engine
            .get_global("order")
            .unwrap()
            .as_string()
            .unwrap()
            .to_std_string_escaped(),
        "sync"
    );
    engine
        .call_raw(&callback, &JsValue::undefined(), &[])
        .unwrap();
    assert_eq!(engine.stats().pending_jobs, PENDING_JOB_LIMIT);
    assert_eq!(engine.stats().jobs_executed, 1);
    assert_eq!(
        engine
            .get_global("order")
            .unwrap()
            .as_string()
            .unwrap()
            .to_std_string_escaped(),
        "sync,task"
    );
    engine.checkpoint_raw().unwrap();
    assert_eq!(engine.stats().pending_jobs, PENDING_JOB_LIMIT - 1);
    assert_eq!(engine.stats().jobs_executed, 2);
    assert_eq!(
        engine
            .get_global("order")
            .unwrap()
            .as_string()
            .unwrap()
            .to_std_string_escaped(),
        "sync,task,microtask"
    );
    drop(permits);
    assert_eq!(engine.stats().pending_jobs, 0);
    assert_eq!(engine.stats().jobs_admitted, PENDING_JOB_LIMIT + 1);
    engine.stats().validate().unwrap();
}

#[test]
fn foreign_and_double_used_permits_fail_without_counter_refunds() {
    let mut first = Engine::new().unwrap();
    let mut foreign = admit_host_task(first.context_mut()).unwrap();
    let first_before = first.stats();
    let mut second = Engine::new().unwrap();
    let own = admit_host_task(second.context_mut()).unwrap();
    assert!(begin_host_task(second.context_mut(), &mut foreign).is_err());
    assert!(second.check().unwrap_err().is_fatal());
    assert_eq!(first.stats(), first_before);
    drop(own);
    foreign.cancel();
    assert_eq!(first.stats().pending_jobs, 0);
    assert_eq!(first.stats().jobs_admitted, 1);
    assert_eq!(second.stats().pending_jobs, 0);
    assert_eq!(second.stats().jobs_executed, 0);

    let mut engine = Engine::new().unwrap();
    let mut permit = admit_host_task(engine.context_mut()).unwrap();
    begin_host_task(engine.context_mut(), &mut permit).unwrap();
    assert!(begin_host_task(engine.context_mut(), &mut permit).is_err());
    assert!(engine.check().unwrap_err().is_fatal());
    drop(permit);
    assert_eq!(engine.stats().jobs_admitted, 1);
    assert_eq!(engine.stats().jobs_executed, 1);
    assert_eq!(engine.stats().pending_jobs, 0);
    engine.stats().validate().unwrap();
}

#[test]
fn raw_execution_retains_actual_values_without_hostile_property_access() {
    let mut engine = Engine::new().unwrap();
    engine.evaluate("var effects=0;var marker=new Proxy({},{get(){effects++;throw 1},getPrototypeOf(){effects++;throw 2}}); function fail(){throw marker} Object.defineProperty(this,'failedGetter',{get:fail});").unwrap();
    let marker = engine.get_global("marker").unwrap();
    let callback = engine.get_global("fail").unwrap().as_object().unwrap();
    let failures = [
        engine.evaluate_raw("throw marker;").unwrap_err(),
        engine
            .call_raw(&callback, &JsValue::undefined(), &[])
            .unwrap_err(),
        engine.get_global_raw("failedGetter").unwrap_err(),
    ];
    for failure in failures {
        assert!(!failure.diagnostic.is_fatal());
        assert_eq!(failure.diagnostic.kind, "ThrownValue");
        assert_eq!(failure.exception.unwrap().as_opaque(), Some(&marker));
    }
    assert_eq!(engine.get_global("effects").unwrap().as_number(), Some(0.0));
    let primitive = engine.evaluate_raw("throw 42;").unwrap_err();
    assert_eq!(
        primitive
            .exception
            .unwrap()
            .as_opaque()
            .unwrap()
            .as_number(),
        Some(42.0)
    );
    let native = engine.evaluate_raw("missingBinding;").unwrap_err();
    assert_eq!(native.diagnostic.kind, "ReferenceError");
    assert!(native.exception.unwrap().as_native().is_some());
    let fatal = engine.evaluate_raw("for(;;){}").unwrap_err();
    assert!(fatal.diagnostic.is_fatal());
    assert!(fatal.exception.is_none());
    let later = engine.get_global_raw("marker").unwrap_err();
    assert!(later.diagnostic.is_fatal());
    assert!(later.exception.is_none());
}

#[test]
fn raw_checkpoint_preserves_an_ordinary_native_job_exception_and_cancels_queue() {
    use mg_butane::modern::boa_engine::{JsError, job::GenericJob};
    let mut engine = Engine::new().unwrap();
    let mut permit = admit_host_task(engine.context_mut()).unwrap();
    let realm = engine.context().realm().clone();
    engine
        .context_mut()
        .enqueue_job(GenericJob::new(|_| Err(JsError::from_opaque(42.into())), realm).into());
    engine
        .evaluate("var effect=0;Promise.resolve().then(()=>effect++);")
        .unwrap();
    let failure = engine.checkpoint_raw().unwrap_err();
    assert!(!failure.diagnostic.is_fatal());
    assert_eq!(
        failure.exception.unwrap().as_opaque().unwrap().as_number(),
        Some(42.0)
    );
    assert_eq!(engine.get_global("effect").unwrap().as_number(), Some(0.0));
    assert_eq!(engine.stats().pending_jobs, 1); // Only the separate host task remains.
    assert_eq!(engine.stats().jobs_admitted, 3);
    assert_eq!(engine.stats().jobs_executed, 1);
    begin_host_task(engine.context_mut(), &mut permit).unwrap();
    assert_eq!(engine.stats().pending_jobs, 0);
    assert_eq!(engine.stats().jobs_executed, 2);
    engine.stats().validate().unwrap();
}

#[test]
fn fatal_boundary_releases_native_job_roots_and_expires_host_permits() {
    use mg_butane::modern::{
        boa_engine::job::GenericJob,
        boa_gc::{Finalize, Gc, Trace},
    };
    #[derive(Trace)]
    struct Probe(#[unsafe_ignore_trace] Rc<Cell<usize>>);
    impl Finalize for Probe {
        fn finalize(&self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let finalized = Rc::new(Cell::new(0));
    let mut engine = Engine::new().unwrap();
    let permit = admit_host_task(engine.context_mut()).unwrap();
    let probe = Gc::new(Probe(finalized.clone()));
    let realm = engine.context().realm().clone();
    engine.context_mut().enqueue_job(
        GenericJob::new(
            move |_| {
                let _keep_root = &probe;
                Ok(JsValue::undefined())
            },
            realm,
        )
        .into(),
    );
    boa_gc::force_collect();
    assert_eq!(finalized.get(), 0);
    assert_eq!(engine.stats().pending_jobs, 2);
    engine.latch_failure("owned root-release test");
    boa_gc::force_collect();
    assert_eq!(finalized.get(), 1);
    permit.cancel();
    assert_eq!(engine.stats().pending_jobs, 0);
    assert_eq!(engine.stats().jobs_admitted, 2);
    assert_eq!(engine.stats().jobs_executed, 0);
    engine.stats().validate().unwrap();
}

mod rejection_bridge {
    use super::*;
    use mg_butane::modern::{
        boa_engine::{
            Context,
            builtins::promise::{OperationType, Promise},
            object::{JsObject, builtins::JsPromise},
        },
        boa_gc::{Finalize, Gc, GcRefCell, Trace},
    };

    #[derive(Trace, Finalize)]
    struct Candidate {
        promise: JsObject<Promise>,
        reason: JsValue,
        #[unsafe_ignore_trace]
        _permit: WorkPermit,
    }

    #[derive(Default, Trace, Finalize)]
    struct Candidates {
        entries: Vec<Candidate>,
        rejected: usize,
        handled: usize,
    }

    fn observer(promise: &JsObject<Promise>, operation: OperationType, context: &mut Context) {
        if check_context(context).is_err() {
            return;
        }
        let state = context
            .get_data::<Gc<GcRefCell<Candidates>>>()
            .unwrap()
            .clone();
        match operation {
            OperationType::Reject => {
                let Ok(permit) = admit_host_task(context) else {
                    return;
                };
                let reason = JsPromise::from(promise.clone())
                    .state()
                    .as_rejected()
                    .unwrap()
                    .clone();
                let mut state = state.borrow_mut();
                state.rejected += 1;
                state.entries.push(Candidate {
                    promise: promise.clone(),
                    reason,
                    _permit: permit,
                });
            }
            OperationType::Handle => {
                let mut state = state.borrow_mut();
                state.handled += 1;
                state
                    .entries
                    .retain(|candidate| candidate.promise != *promise);
            }
        }
    }

    #[test]
    fn rejection_hook_preserves_native_identity_and_values_without_reentry() {
        let mut engine = Engine::new().unwrap();
        let candidates = Gc::new(GcRefCell::new(Candidates::default()));
        engine.context_mut().insert_data(candidates.clone());
        engine.set_rejection_observer(observer).unwrap();
        engine.evaluate("var effects=0;var reason=new Proxy({},{get(){effects++;throw 1}});var rejected=Promise.reject(reason);").unwrap();
        let reason = engine.get_global("reason").unwrap();
        assert_eq!(candidates.borrow().entries[0].reason, reason);
        assert_eq!(candidates.borrow().rejected, 1);
        assert_eq!(engine.stats().jobs_admitted, 1);
        assert_eq!(engine.stats().pending_jobs, 1);
        engine.checkpoint().unwrap();
        boa_gc::force_collect();
        assert_eq!(candidates.borrow().entries[0].reason, reason);
        assert_eq!(engine.get_global("effects").unwrap().as_number(), Some(0.0));

        engine.evaluate("rejected.catch(()=>0);").unwrap();
        assert_eq!(candidates.borrow().handled, 1);
        assert!(candidates.borrow().entries.is_empty());
        assert_eq!(engine.stats().jobs_admitted, 2); // Canceled notice + Promise reaction.
        assert_eq!(engine.stats().pending_jobs, 1);
        engine.checkpoint().unwrap();
        assert_eq!(engine.stats().jobs_admitted, 2);
        assert_eq!(engine.stats().jobs_executed, 1);
        assert_eq!(engine.stats().pending_jobs, 0);
        engine.stats().validate().unwrap();
    }

    #[test]
    fn synchronous_rejections_cannot_bypass_shared_pending_admission() {
        let mut engine = Engine::new().unwrap();
        let candidates = Gc::new(GcRefCell::new(Candidates::default()));
        engine.context_mut().insert_data(candidates.clone());
        engine.set_rejection_observer(observer).unwrap();
        let failure = engine
            .evaluate_raw("for(let i=0;i<257;i++) Promise.reject(i);")
            .unwrap_err();
        assert!(failure.diagnostic.is_fatal());
        assert!(failure.exception.is_none());
        assert_eq!(candidates.borrow().entries.len(), PENDING_JOB_LIMIT);
        assert_eq!(engine.stats().jobs_admitted, PENDING_JOB_LIMIT);
        assert_eq!(engine.stats().jobs_executed, 0);
        assert_eq!(engine.stats().pending_jobs, 0);
        // The embedding layer owns and clears its traced values at fatal exit.
        candidates.borrow_mut().entries.clear();
        assert_eq!(engine.stats().pending_jobs, 0);
        engine.stats().validate().unwrap();
        assert!(
            engine
                .set_rejection_observer(observer)
                .unwrap_err()
                .is_fatal()
        );
    }
}
