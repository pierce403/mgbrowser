//! Concrete Boa embedding policy for the experimental, process-contained page lane.
//!
//! These cooperative opcode/source/job checks are not complete parser, native
//! builtin, regular-expression or GC work accounting. The platform must still
//! provide worker allocation limits, CPU/address-space limits and a cumulative
//! parent deadline. Constructing this facade alone does not provide isolation.
//! The original interpreter and its independently tested limits are unchanged.

pub use boa_engine;
pub use boa_gc;

use boa_engine::{
    Context, JsError, JsResult, JsString, JsValue, Script, Source,
    builtins::promise::{OperationType, Promise},
    context::HostHooks,
    error::{EngineError, JsNativeErrorKind},
    job::{Job, JobExecutor},
    module::IdleModuleLoader,
    object::JsObject,
    realm::Realm,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::VecDeque,
    fmt,
    rc::{Rc, Weak},
};

/// The profile is deliberately versioned separately from ECMAScript support.
pub const PROFILE: &str = "boa-page-process-v1";
pub const OPCODE_LIMIT: usize = 1_000_000;
pub const SOURCE_LIMIT: usize = 1024 * 1024;
pub const CUMULATIVE_SOURCE_LIMIT: usize = 4 * 1024 * 1024;
pub const PENDING_JOB_LIMIT: usize = 256;
pub const CUMULATIVE_JOB_LIMIT: usize = 2_048;
pub const RECURSION_LIMIT: usize = 64;
pub const STACK_LIMIT: usize = 65_536;
pub const DIAGNOSTIC_LIMIT: usize = 512;
pub const WORKER_OUTSTANDING_LIMIT: u64 = 32 * 1024 * 1024;
pub const WORKER_CUMULATIVE_LIMIT: u64 = 64 * 1024 * 1024;

const SOURCE_FAILURE: &str = "Boa source admission budget exhausted";
const JOB_FAILURE: &str = "Boa shared job/task budget exhausted";
const INVALID_PERMIT: &str = "Invalid Boa host work permit";
const OPCODE_FAILURE: &str = "Boa cumulative opcode budget exhausted";
const ENGINE_FAILURE: &str = "Boa engine runtime limit reached";
const UNSUPPORTED_JOB: &str = "Boa job type is outside the page profile";

/// Data-only diagnostics: formatting never calls JavaScript or reads properties.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub fatal: bool,
    pub kind: String,
    pub message: String,
}

impl Error {
    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        self.fatal
    }

    fn fatal(reason: &str) -> Self {
        Self {
            fatal: true,
            kind: "EngineLimit".into(),
            message: reason.chars().take(DIAGNOSTIC_LIMIT).collect(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for Error {}

/// A worker-local failure retaining the genuine exception, without coercion.
///
/// This is deliberately not serializable. Fatal policy/engine failures never
/// expose a dispatchable exception. The data-only public methods discard this
/// local value and retain their existing safe diagnostic contract.
#[derive(Debug)]
pub struct ExecutionFailure {
    pub diagnostic: Error,
    pub exception: Option<JsError>,
}

impl ExecutionFailure {
    fn diagnostic(diagnostic: Error) -> Self {
        Self {
            diagnostic,
            exception: None,
        }
    }
}

/// A non-capturing worker-local bridge. It must not execute JavaScript, retain
/// values in untraced storage, or hold native borrows across re-entry. A binding
/// retaining a rejection candidate must first admit shared host work.
pub type RejectionObserver = fn(&JsObject<Promise>, OperationType, &mut Context);

#[derive(Clone, Copy)]
struct RejectionHook(RejectionObserver);

/// Cumulative, monotonic engine-policy counters. These are not GC heap/RSS data.
/// Job fields include Promise/native jobs and admitted host tasks, not separate
/// allowances for each kind of work. Cancellation never refunds lifetime work.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceStats {
    pub profile: String,
    pub opcodes_remaining: usize,
    pub source_bytes: usize,
    pub source_admissions: usize,
    pub jobs_admitted: usize,
    pub jobs_executed: usize,
    pub pending_jobs: usize,
    pub fatal_reason: Option<String>,
    pub worker_memory: Option<WorkerMemory>,
}

pub type Report = ResourceStats;

/// Platform allocator accounting across the entire restricted worker. Includes
/// input, DOM, engine and protocol storage plus allocator header/alignment costs;
/// this is neither Boa GC live heap nor operating-system resident-set size.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerMemory {
    pub outstanding_bytes: u64,
    pub peak_bytes: u64,
    pub cumulative_bytes: u64,
    pub allocations: u64,
    pub outstanding_limit_bytes: u64,
    pub cumulative_limit_bytes: u64,
}

impl WorkerMemory {
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.outstanding_limit_bytes == WORKER_OUTSTANDING_LIMIT
            && self.cumulative_limit_bytes == WORKER_CUMULATIVE_LIMIT
            && self.outstanding_bytes <= self.peak_bytes
            && self.peak_bytes <= self.outstanding_limit_bytes
            && self.peak_bytes <= self.cumulative_bytes
            && self.cumulative_bytes <= self.cumulative_limit_bytes
            && self.allocations <= self.cumulative_bytes
    }

    #[must_use]
    pub const fn is_valid_after(&self, previous: &Self) -> bool {
        self.is_valid()
            && previous.is_valid()
            && self.peak_bytes >= previous.peak_bytes
            && self.cumulative_bytes >= previous.cumulative_bytes
            && self.allocations >= previous.allocations
    }
}

impl ResourceStats {
    /// Validate untrusted worker counters before accepting a projection.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.profile != PROFILE
            || self.opcodes_remaining > OPCODE_LIMIT
            || self.source_bytes > CUMULATIVE_SOURCE_LIMIT
            || self.source_admissions > self.source_bytes
            || self.jobs_admitted > CUMULATIVE_JOB_LIMIT
            || self.jobs_executed > self.jobs_admitted
            || self.pending_jobs > PENDING_JOB_LIMIT
            || self.pending_jobs > self.jobs_admitted - self.jobs_executed
            || (self.opcodes_remaining == 0 && self.fatal_reason.is_none())
            || (self.fatal_reason.is_some() && self.pending_jobs != 0)
            || self.worker_memory.is_some_and(|memory| !memory.is_valid())
            || self
                .fatal_reason
                .as_ref()
                .is_some_and(|reason| reason.is_empty() || reason.len() > DIAGNOSTIC_LIMIT)
        {
            return Err("Invalid Boa resource report");
        }
        Ok(())
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// A retained page must not renew counters or recover from fatal termination.
    pub fn validate_after(&self, previous: &Self) -> Result<(), &'static str> {
        self.validate()?;
        previous.validate()?;
        if self.opcodes_remaining > previous.opcodes_remaining
            || self.source_bytes < previous.source_bytes
            || self.source_admissions < previous.source_admissions
            || self.jobs_admitted < previous.jobs_admitted
            || self.jobs_executed < previous.jobs_executed
            || previous
                .fatal_reason
                .as_ref()
                .is_some_and(|reason| self.fatal_reason.as_ref() != Some(reason))
            || match (self.worker_memory, previous.worker_memory) {
                (Some(current), Some(previous)) => !current.is_valid_after(&previous),
                (None, Some(_)) => true,
                _ => false,
            }
        {
            return Err("Boa resource counters were renewed");
        }
        Ok(())
    }
}

#[derive(Default)]
struct State {
    source_bytes: usize,
    source_admissions: usize,
    jobs_admitted: usize,
    jobs_executed: usize,
    pending_work: usize,
    epoch: u64,
    fatal: Option<&'static str>,
}

impl State {
    fn latch(&mut self, reason: &'static str) {
        if self.fatal.is_none() {
            self.fatal = Some(reason);
            // There is no recovery or renewed epoch. Old permits become inert
            // before their queues/roots are dropped at the owning boundary.
            self.epoch += 1;
            self.pending_work = 0;
        }
    }

    fn admit_source(&mut self, bytes: usize) -> JsResult<()> {
        if self.fatal.is_some() {
            return Err(policy_error());
        }
        // Empty evals also cost admission; no counter can grow without a bound.
        let charged = bytes.max(1);
        if bytes > SOURCE_LIMIT
            || charged > CUMULATIVE_SOURCE_LIMIT.saturating_sub(self.source_bytes)
            || self.source_admissions >= CUMULATIVE_SOURCE_LIMIT
        {
            self.latch(SOURCE_FAILURE);
            return Err(policy_error());
        }
        self.source_bytes += charged;
        self.source_admissions += 1;
        Ok(())
    }
}

/// One reservation from the shared pending/lifetime work allowance.
///
/// No JavaScript or GC value lives here, and the weak policy reference cannot
/// keep an Engine alive. Bindings may ignore this scalar field when tracing a
/// record, but must trace the record's callbacks, arguments and rejection values.
/// Drop/cancel releases pending storage only; lifetime admission is never renewed.
#[derive(Debug)]
pub struct WorkPermit {
    state: Weak<RefCell<State>>,
    epoch: u64,
    armed: bool,
}

impl WorkPermit {
    /// Cancel without executing. No cumulative counter is refunded.
    pub fn cancel(self) {
        drop(self);
    }
}

impl Drop for WorkPermit {
    fn drop(&mut self) {
        if self.armed {
            self.armed = false;
            if let Some(state) = self.state.upgrade() {
                let mut state = state.borrow_mut();
                if state.epoch == self.epoch {
                    state.pending_work -= 1;
                }
            }
        }
    }
}

fn admit_work(state: &Rc<RefCell<State>>) -> JsResult<WorkPermit> {
    let epoch = {
        let mut state = state.borrow_mut();
        if state.fatal.is_some() {
            return Err(policy_error());
        }
        if state.pending_work >= PENDING_JOB_LIMIT || state.jobs_admitted >= CUMULATIVE_JOB_LIMIT {
            state.latch(JOB_FAILURE);
            return Err(policy_error());
        }
        state.pending_work += 1;
        state.jobs_admitted += 1;
        state.epoch
    };
    Ok(WorkPermit {
        state: Rc::downgrade(state),
        epoch,
        armed: true,
    })
}

#[derive(Clone)]
struct Policy(Rc<RefCell<State>>);

impl HostHooks for Policy {
    fn promise_rejection_tracker(
        &self,
        promise: &JsObject<Promise>,
        operation: OperationType,
        context: &mut Context,
    ) {
        // Copy the scalar bridge out before any binding accesses context data.
        // The hook must complete normally; a binding may latch policy failure,
        // but may not dispatch callbacks from this hook.
        let observer = context.get_data::<RejectionHook>().copied();
        if let Some(RejectionHook(observer)) = observer {
            observer(promise, operation, context);
        }
    }

    fn ensure_can_compile_strings(
        &self,
        _realm: Realm,
        parameters: &[JsString],
        body: &JsString,
        _direct: bool,
        _context: &mut Context,
    ) -> JsResult<()> {
        // Dynamic sources already exist as UTF-16. Charge their storage without
        // an allocation or user-controlled coercion. Function fragments share
        // one admission and include the comma separators inserted by the engine.
        let units = parameters.iter().fold(body.len(), |sum, parameter| {
            sum.saturating_add(parameter.len()).saturating_add(1)
        });
        self.0.borrow_mut().admit_source(units.saturating_mul(2))
    }

    fn local_timezone_offset_seconds(&self, _unix_time_seconds: i64) -> i32 {
        // No timezone files or environment queries in this host profile.
        0
    }

    fn max_buffer_size(&self, _context: &mut Context) -> u64 {
        // Bytes, despite the upstream method's misleading "bits" doc sentence.
        // Process allocation admission remains authoritative for all buffers.
        4 * 1024 * 1024
    }
}

fn policy_error() -> JsError {
    // Reuse upstream's uncatchable variant. The independent first-failure latch
    // gives the precise source/job reason instead of pretending opcode exhaustion.
    EngineError::NoInstructionsRemain.into()
}

/// Check shared policy at the start of every DOM/native callback before effects.
pub fn check_context(context: &mut Context) -> JsResult<()> {
    let Some(policy) = context.get_data::<Policy>().cloned() else {
        return Err(policy_error());
    };
    let mut state = policy.0.borrow_mut();
    if context.instructions_remaining() == 0 {
        state.latch(OPCODE_FAILURE);
    }
    if state.fatal.is_some() {
        return Err(policy_error());
    }
    Ok(())
}

/// Latch a host-defined fatal rejection without a catchable JavaScript error.
pub fn latch_context(context: &mut Context, reason: &'static str) -> JsError {
    if let Some(policy) = context.get_data::<Policy>() {
        policy.0.borrow_mut().latch(reason);
    }
    policy_error()
}

/// Admit a host task BEFORE retaining its callback, arguments or other GC roots.
pub fn admit_host_task(context: &mut Context) -> JsResult<WorkPermit> {
    check_context(context)?;
    let policy = context
        .get_data::<Policy>()
        .cloned()
        .ok_or_else(policy_error)?;
    admit_work(&policy.0)
}

/// Consume a reservation before callback execution, freeing its pending slot.
///
/// This does not invoke code or perform a microtask checkpoint. Both remain the
/// binding's explicit guarded operations. Reusing, crossing or replaying a
/// permit latches the receiving policy; no foreign policy counter is changed.
pub fn begin_host_task(context: &mut Context, permit: &mut WorkPermit) -> JsResult<()> {
    check_context(context)?;
    let policy = context
        .get_data::<Policy>()
        .cloned()
        .ok_or_else(policy_error)?;
    let mut state = policy.0.borrow_mut();
    if !permit.armed
        || !Weak::ptr_eq(&permit.state, &Rc::downgrade(&policy.0))
        || permit.epoch != state.epoch
    {
        state.latch(INVALID_PERMIT);
        return Err(policy_error());
    }
    permit.armed = false;
    state.pending_work -= 1;
    state.jobs_executed += 1;
    Ok(())
}

struct Jobs {
    state: Rc<RefCell<State>>,
    queue: RefCell<VecDeque<(Job, WorkPermit)>>,
}

impl Jobs {
    fn clear(&self) {
        // End the queue borrow before permits release their policy reservations.
        let queued = std::mem::take(&mut *self.queue.borrow_mut());
        drop(queued);
    }
}

impl JobExecutor for Jobs {
    fn enqueue_job(self: Rc<Self>, job: Job, _context: &mut Context) {
        if self.state.borrow().fatal.is_some() {
            return;
        }
        // FinalizationRegistry cleanup callbacks are optional in ECMAScript.
        // The page profile deliberately does not schedule them or async/timer
        // executors; generic native and Promise jobs are synchronous only.
        if matches!(job, Job::FinalizationRegistryCleanupJob(_)) {
            return;
        }
        if !matches!(job, Job::PromiseJob(_) | Job::GenericJob(_)) {
            self.state.borrow_mut().latch(UNSUPPORTED_JOB);
            return;
        }
        let Ok(permit) = admit_work(&self.state) else {
            return;
        };
        self.queue.borrow_mut().push_back((job, permit));
    }

    fn run_jobs(self: Rc<Self>, context: &mut Context) -> JsResult<()> {
        loop {
            if let Err(error) = check_context(context) {
                self.clear();
                return Err(error);
            }
            // Release all RefCell borrows before executing arbitrary callbacks.
            let Some((job, mut permit)) = self.queue.borrow_mut().pop_front() else {
                return Ok(());
            };
            begin_host_task(context, &mut permit)?;
            let result = match job {
                Job::PromiseJob(job) => job.call(context),
                Job::GenericJob(job) => job.call(context),
                _ => unreachable!("only synchronous jobs admitted"),
            };
            if let Err(error) = result {
                if error.as_engine().is_some() {
                    self.state.borrow_mut().latch(ENGINE_FAILURE);
                }
                self.clear();
                return Err(error);
            }
        }
    }
}

/// One thread-owned realm and policy. Navigation must drop the entire instance.
///
/// Binding setup may use `context_mut`; execute source/functions through the
/// guarded methods. Host callbacks must call [`check_context`] before effects.
pub struct Engine {
    context: Context,
    state: Rc<RefCell<State>>,
    jobs: Rc<Jobs>,
}

impl Engine {
    pub fn new() -> Result<Self, Error> {
        let state = Rc::new(RefCell::new(State::default()));
        let policy = Policy(state.clone());
        let jobs = Rc::new(Jobs {
            state: state.clone(),
            queue: RefCell::new(VecDeque::new()),
        });
        let mut context = Context::builder()
            .instructions_remaining(OPCODE_LIMIT)
            .host_hooks(Rc::new(policy.clone()))
            .job_executor(jobs.clone())
            .module_loader(Rc::new(IdleModuleLoader))
            .can_block(false)
            .build()
            .map_err(|error| safe_error(&error))?;
        context.insert_data(policy);
        context
            .runtime_limits_mut()
            .set_recursion_limit(RECURSION_LIMIT);
        context
            .runtime_limits_mut()
            .set_stack_size_limit(STACK_LIMIT);
        context
            .runtime_limits_mut()
            .set_loop_iteration_limit(OPCODE_LIMIT as u64);
        context.runtime_limits_mut().set_backtrace_limit(16);
        Ok(Self {
            context,
            state,
            jobs,
        })
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Install a worker-local, non-capturing rejection observer. The bridge runs
    /// synchronously in Boa's hook, not as a task or JavaScript callback.
    pub fn set_rejection_observer(&mut self, observer: RejectionObserver) -> Result<(), Error> {
        self.check()?;
        self.context.insert_data(RejectionHook(observer));
        Ok(())
    }

    #[must_use]
    pub fn is_fatal(&self) -> bool {
        self.state.borrow().fatal.is_some() || self.context.instructions_remaining() == 0
    }

    pub fn check(&mut self) -> Result<(), Error> {
        let result = check_context(&mut self.context);
        self.finish(result)
    }

    pub fn latch_failure(&mut self, reason: &'static str) -> Error {
        self.state.borrow_mut().latch(reason);
        self.jobs.clear();
        self.context.clear_kept_objects();
        Error::fatal(self.state.borrow().fatal.unwrap_or(ENGINE_FAILURE))
    }

    /// Evaluate one classic source; callers choose the HTML job checkpoint.
    pub fn evaluate(&mut self, source: &str) -> Result<JsValue, Error> {
        self.evaluate_raw(source)
            .map_err(|failure| failure.diagnostic)
    }

    /// Guarded evaluation retaining any ordinary exception inside this worker.
    pub fn evaluate_raw(&mut self, source: &str) -> Result<JsValue, ExecutionFailure> {
        self.check().map_err(ExecutionFailure::diagnostic)?;
        let admitted = self.state.borrow_mut().admit_source(source.len());
        self.finish_raw(admitted)?;
        let parsed = Script::parse(Source::from_bytes(source), None, &mut self.context);
        let script = self.finish_raw(parsed)?;
        let result = script.evaluate(&mut self.context);
        self.finish_raw(result)
    }

    /// Invoke a retained listener/function without renewing any resource budget.
    pub fn call(
        &mut self,
        function: &JsObject,
        this: &JsValue,
        arguments: &[JsValue],
    ) -> Result<JsValue, Error> {
        self.call_raw(function, this, arguments)
            .map_err(|failure| failure.diagnostic)
    }

    /// Guarded callback invocation preserving the genuine exception, without
    /// admitting a task or running a checkpoint implicitly.
    pub fn call_raw(
        &mut self,
        function: &JsObject,
        this: &JsValue,
        arguments: &[JsValue],
    ) -> Result<JsValue, ExecutionFailure> {
        self.check().map_err(ExecutionFailure::diagnostic)?;
        let result = function.call(this, arguments, &mut self.context);
        self.finish_raw(result)
    }

    /// Read the actual realm global, not a script-reassignable `globalThis`
    /// alias. Accessors can execute JS, so this shares all fatal guards.
    pub fn get_global(&mut self, name: &str) -> Result<JsValue, Error> {
        self.get_global_raw(name)
            .map_err(|failure| failure.diagnostic)
    }

    /// Guarded actual-global read retaining a thrown getter value worker-locally.
    pub fn get_global_raw(&mut self, name: &str) -> Result<JsValue, ExecutionFailure> {
        self.check().map_err(ExecutionFailure::diagnostic)?;
        let result = self
            .context
            .global_object()
            .get(JsString::from(name), &mut self.context);
        self.finish_raw(result)
    }

    /// Drain the FIFO checkpoint, including newly queued jobs, within one budget.
    pub fn checkpoint(&mut self) -> Result<(), Error> {
        self.checkpoint_raw().map_err(|failure| failure.diagnostic)
    }

    /// Drain only the microtask queue, retaining any ordinary failure locally.
    /// Host tasks remain separate and must be externally selected and begun.
    pub fn checkpoint_raw(&mut self) -> Result<(), ExecutionFailure> {
        if let Err(error) = self.check() {
            // No JavaScript stack is active at this host boundary, including
            // when a preceding script already terminated the realm.
            self.context.clear_kept_objects();
            return Err(ExecutionFailure::diagnostic(error));
        }
        let result = self.context.run_jobs();
        // Boa delegates run_jobs to our executor but does not perform the HTML
        // checkpoint's ClearKeptObjects step. Keep WeakRef dereferences alive
        // through the complete checkpoint, never indefinitely across inputs.
        self.context.clear_kept_objects();
        self.finish_raw(result)
    }

    fn finish<T>(&mut self, result: JsResult<T>) -> Result<T, Error> {
        self.finish_raw(result)
            .map_err(|failure| failure.diagnostic)
    }

    fn finish_raw<T>(&mut self, result: JsResult<T>) -> Result<T, ExecutionFailure> {
        if self.context.instructions_remaining() == 0 {
            self.state.borrow_mut().latch(OPCODE_FAILURE);
        }
        if result
            .as_ref()
            .is_err_and(|error| error.as_engine().is_some())
        {
            self.state.borrow_mut().latch(ENGINE_FAILURE);
        }
        // A temporary Ref must not span queue clearing: permit Drop accesses
        // scalar policy state. Copy the reason out before releasing any roots.
        let fatal = self.state.borrow().fatal;
        if let Some(reason) = fatal {
            self.jobs.clear();
            self.context.clear_kept_objects();
            return Err(ExecutionFailure::diagnostic(Error::fatal(reason)));
        }
        result.map_err(|error| ExecutionFailure {
            diagnostic: safe_error(&error),
            exception: Some(error),
        })
    }

    #[must_use]
    pub fn stats(&self) -> Report {
        let state = self.state.borrow();
        Report {
            profile: PROFILE.into(),
            opcodes_remaining: self.context.instructions_remaining(),
            source_bytes: state.source_bytes,
            source_admissions: state.source_admissions,
            jobs_admitted: state.jobs_admitted,
            jobs_executed: state.jobs_executed,
            pending_jobs: state.pending_work,
            fatal_reason: state.fatal.map(str::to_owned),
            worker_memory: None,
        }
    }

    #[must_use]
    pub fn report(&self) -> Report {
        self.stats()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Jobs capture GC roots. Release those before the owning Context dies.
        self.jobs.clear();
    }
}

/// Never uses JsError::try_native, coercion, .name/.message access or JS getters.
pub fn safe_error(error: &JsError) -> Error {
    if error.as_engine().is_some() {
        return Error::fatal(ENGINE_FAILURE);
    }
    if let Some(native) = error.as_native() {
        let kind = match native.kind() {
            JsNativeErrorKind::Error => "Error",
            JsNativeErrorKind::Eval => "EvalError",
            JsNativeErrorKind::Type => "TypeError",
            JsNativeErrorKind::Range => "RangeError",
            JsNativeErrorKind::Reference => "ReferenceError",
            JsNativeErrorKind::Syntax => "SyntaxError",
            JsNativeErrorKind::Uri => "URIError",
            JsNativeErrorKind::Aggregate(_) => "AggregateError",
            _ => "UnknownError",
        };
        return Error {
            fatal: false,
            kind: kind.into(),
            message: native.message().chars().take(DIAGNOSTIC_LIMIT).collect(),
        };
    }
    Error {
        fatal: false,
        kind: "ThrownValue".into(),
        message: "JavaScript threw a value (not coerced by the host)".into(),
    }
}

#[cfg(test)]
mod permit_tests {
    use super::*;

    #[test]
    fn a_permit_does_not_keep_the_policy_alive_after_engine_drop() {
        let mut engine = Engine::new().unwrap();
        let permit = admit_host_task(engine.context_mut()).unwrap();
        let weak = permit.state.clone();
        assert!(weak.upgrade().is_some());
        drop(engine);
        assert!(weak.upgrade().is_none());
        permit.cancel();
    }

    #[test]
    fn expired_epoch_cannot_execute_or_decrement_a_fatal_ledger() {
        let mut engine = Engine::new().unwrap();
        let mut permit = admit_host_task(engine.context_mut()).unwrap();
        let original_epoch = permit.epoch;
        engine.latch_failure("owned test fatal");
        assert_ne!(engine.state.borrow().epoch, original_epoch);
        assert!(begin_host_task(engine.context_mut(), &mut permit).is_err());
        drop(permit);
        assert_eq!(engine.stats().pending_jobs, 0);
        assert_eq!(engine.stats().jobs_admitted, 1);
        assert_eq!(engine.stats().jobs_executed, 0);
    }
}
