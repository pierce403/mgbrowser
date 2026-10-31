//! Fixed-cohort transaction fence, not an alternate result ledger or sandbox.
//! Traces below are raw reply evidence. Only Browser's charged ledger is accepted.
use super::source::{SourceError, SourceInventory};
use mg_butane::modern::Report;
use mg_chassis::scripts::{ScriptRuntime, ScriptSession, SessionUpdate, SourceWireMode};
use mg_sparkle::{
    document::{self, Node},
    js_browser::Request,
    page_session::{
        DefaultAction, LifecycleProgress, RealmState, SessionInput, SessionReply, TaskPump,
        observer::{
            ObserverContract, ObserverExpectation, ObserverFault, ObserverInit, ObserverProgress,
            ObserverRecord, ObserverReply,
        },
    },
};
use serde::Serialize;
use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_TAGS: usize = 50_000;
const MAX_TAG_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TRANSACTIONS: usize = 64;
pub const MAX_DIAGNOSTICS: usize = 64;
pub const MAX_DIAGNOSTIC_BYTES: usize = 4096;
pub const MAX_DIAGNOSTIC_PREFIX_BYTES: usize = MAX_DIAGNOSTICS * MAX_DIAGNOSTIC_BYTES;
pub const MAX_RECEIPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_FAILURES: usize = 64;
type Result<T> = std::result::Result<T, HostFailure>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ConfigurationFailure {
    WireMode,
    Observer,
    Bootstrap,
    InnerExpectation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum GenerationFailure {
    Zero,
    Reused,
    Mismatch,
    InnerRejected,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum PolicyFailure {
    Navigation,
    DefaultAction,
    MetaRefresh,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum TraceFailure {
    Unavailable,
    Capacity,
    Allocation,
    DiagnosticShape,
    ResourceShape,
    Missing,
    DuplicateAcknowledgement,
    ReplyIdentity,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum TransportPhase {
    Start,
    TaskAdmission,
    Reply,
}
/// These are causes established by this wrapper, never inferred from inner text.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum HostFailure {
    Source { error: SourceError },
    Configuration { reason: ConfigurationFailure },
    Generation { reason: GenerationFailure },
    Cutoff,
    Closed,
    RepeatedStart,
    ActivationRefused,
    ReplyOverlap,
    Policy { reason: PolicyFailure },
    Projection,
    Trace { reason: TraceFailure },
    Acknowledgement,
    OpaqueTransport { phase: TransportPhase },
    Cleanup,
}
impl fmt::Display for HostFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "fixed Window host: {self:?}")
    }
}
impl std::error::Error for HostFailure {}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct FailureFact {
    pub ordinal: usize,
    pub failure: HostFailure,
}
/// Scalar discriminants only; no TestRecord, Utf16 or raw observer payload copy.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind")]
pub enum ObserverTrace {
    Missing,
    Disabled,
    WindowHarness {
        run_id: u64,
        first_ordinal: u32,
        next_ordinal: u32,
        progress: ObserverProgress,
        execution_failed: bool,
        result_callbacks: usize,
        completion_callbacks: usize,
        final_inventory_rows: usize,
    },
    ObservationFailed {
        run_id: u64,
        prefix_end: u32,
        fault: ObserverFault,
        execution_failed: bool,
    },
}
impl ObserverTrace {
    fn from_reply(reply: &SessionReply) -> Self {
        match &reply.observer {
            None => Self::Missing,
            Some(ObserverReply::Disabled) => Self::Disabled,
            Some(ObserverReply::ObservationFailed {
                run_id,
                prefix_end,
                fault,
                execution_failed,
            }) => Self::ObservationFailed {
                run_id: *run_id,
                prefix_end: *prefix_end,
                fault: *fault,
                execution_failed: *execution_failed,
            },
            Some(ObserverReply::WindowHarness {
                run_id,
                first_ordinal,
                next_ordinal,
                progress,
                execution_failed,
                records,
            }) => {
                let mut result_callbacks = 0;
                let mut completion_callbacks = 0;
                let mut final_inventory_rows = 0;
                for record in records {
                    match record {
                        ObserverRecord::Result { .. } => result_callbacks += 1,
                        ObserverRecord::Completion { inventory, .. } => {
                            completion_callbacks += 1;
                            final_inventory_rows += inventory.len();
                        }
                    }
                }
                Self::WindowHarness {
                    run_id: *run_id,
                    first_ordinal: *first_ordinal,
                    next_ordinal: *next_ordinal,
                    progress: *progress,
                    execution_failed: *execution_failed,
                    result_callbacks,
                    completion_callbacks,
                    final_inventory_rows,
                }
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RawTrace {
    pub generation: u64,
    pub session_id: u64,
    pub sequence: Option<u64>,
    pub revision: u64,
    pub state: RealmState,
    pub lifecycle: Option<LifecycleProgress>,
    pub observer: ObserverTrace,
    pub scripts_executed: usize,
    pub diagnostics: usize,
    pub rejected_initial: bool,
    /// Actual report, including its unclassified fatal string and System fields.
    pub resources: Option<Report>,
    pub wrapper_validation_ns: u64,
    pub manager_charge_ns: Option<u64>,
    /// A budget acknowledgement is NOT proof of Browser publication.
    pub manager_charge_succeeded: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub transaction: usize,
    pub text: String,
    pub original_utf8_bytes: usize,
    pub omitted_utf8_bytes: usize,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct HostSnapshot {
    pub first_failure: Option<FailureFact>,
    pub secondary_failures: Vec<FailureFact>,
    pub failures_total: usize,
    pub failures_omitted: usize,
    pub starts: usize,
    pub cancel_calls: usize,
    pub close_calls: usize,
    pub wait_idle_calls: usize,
    pub wait_idle_succeeded: bool,
    pub wait_idle_failed: bool,
    pub diagnostics_total: usize,
    pub diagnostics_omitted: usize,
    pub diagnostic_utf8_bytes_total: usize,
    pub diagnostic_utf8_bytes_retained: usize,
    pub diagnostic_utf8_bytes_omitted: usize,
    pub diagnostic_prefix_sealed: bool,
    pub diagnostic_prefix: Vec<Diagnostic>,
    pub raw_trace: Vec<RawTrace>,
}
pub struct HostState {
    state: Mutex<HostSnapshot>,
}
#[derive(Clone, Copy, Debug)]
pub struct HostSummary {
    pub first_failure: Option<HostFailure>,
    pub starts: usize,
    pub replies: usize,
    pub diagnostics_total: usize,
    pub diagnostics_omitted: usize,
    pub wait_idle_succeeded: bool,
}
impl Default for HostState {
    fn default() -> Self {
        Self::new()
    }
}
impl HostState {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(HostSnapshot::default()),
        }
    }
    pub fn snapshot(&self) -> Result<HostSnapshot> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| unavailable())
    }
    pub fn summary(&self) -> Result<HostSummary> {
        let state = self.state.lock().map_err(|_| unavailable())?;
        Ok(HostSummary {
            first_failure: state.first_failure.map(|fact| fact.failure),
            starts: state.starts,
            replies: state.raw_trace.len(),
            diagnostics_total: state.diagnostics_total,
            diagnostics_omitted: state.diagnostics_omitted,
            wait_idle_succeeded: state.wait_idle_succeeded,
        })
    }
    pub fn check(&self) -> Result<()> {
        let state = self.state.lock().map_err(|_| unavailable())?;
        state.first_failure.map_or(Ok(()), |fact| Err(fact.failure))
    }
    fn admit_transaction(&self) -> Result<()> {
        let state = self.state.lock().map_err(|_| unavailable())?;
        if let Some(fact) = state.first_failure {
            return Err(fact.failure);
        }
        if state.raw_trace.len() >= MAX_TRANSACTIONS {
            return Err(trace_failure(TraceFailure::Capacity));
        }
        Ok(())
    }
    pub fn fail(&self, failure: HostFailure) {
        if let Ok(mut state) = self.state.lock() {
            let ordinal = state.failures_total;
            state.failures_total = state.failures_total.saturating_add(1);
            let fact = FailureFact { ordinal, failure };
            if state.first_failure.is_none() {
                state.first_failure = Some(fact);
            } else if state.secondary_failures.len() < MAX_FAILURES - 1
                && state.secondary_failures.try_reserve_exact(1).is_ok()
                && state.secondary_failures.capacity() <= MAX_FAILURES - 1
            {
                state.secondary_failures.push(fact);
            } else {
                state.failures_omitted = state.failures_omitted.saturating_add(1);
            }
        }
    }
    fn started(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if state.starts != 0 {
            return Err(HostFailure::RepeatedStart);
        }
        state.starts = 1;
        Ok(())
    }
    fn note_reply(
        &self,
        reply: &SessionReply,
        generation: u64,
        session_id: u64,
        sequence: Option<u64>,
    ) -> Result<usize> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        if state.raw_trace.len() >= MAX_TRANSACTIONS {
            return Err(trace_failure(TraceFailure::Capacity));
        }
        if reply.errors.len() > MAX_DIAGNOSTICS {
            return Err(trace_failure(TraceFailure::DiagnosticShape));
        }
        // Check existing typed bounds before bounded report string copies.
        if reply.boa.as_ref().is_some_and(|report| !report.is_valid()) {
            return Err(trace_failure(TraceFailure::ResourceShape));
        }
        // This also bounds native scanning without copying any observer values.
        if let Some(ObserverReply::WindowHarness { records, .. }) = &reply.observer {
            if records.len() > 257
                || records.iter().any(|record| {
                    matches!(record, ObserverRecord::Completion { inventory, .. } if inventory.len() > 256)
                })
            {
                return Err(trace_failure(TraceFailure::Capacity));
            }
        }
        let resources = reply.boa.as_ref().map(copy_report).transpose()?;
        state
            .raw_trace
            .try_reserve_exact(1)
            .map_err(|_| trace_failure(TraceFailure::Allocation))?;
        if state.raw_trace.capacity() > MAX_TRANSACTIONS {
            return Err(trace_failure(TraceFailure::Capacity));
        }
        let transaction = state.raw_trace.len();
        let diagnostic_bytes = reply
            .errors
            .iter()
            .try_fold(0usize, |sum, error| sum.checked_add(error.len()))
            .ok_or_else(|| trace_failure(TraceFailure::Capacity))?;
        state.diagnostics_total = state
            .diagnostics_total
            .checked_add(reply.errors.len())
            .ok_or_else(|| trace_failure(TraceFailure::Capacity))?;
        state.diagnostic_utf8_bytes_total = state
            .diagnostic_utf8_bytes_total
            .checked_add(diagnostic_bytes)
            .ok_or_else(|| trace_failure(TraceFailure::Capacity))?;
        let diagnostic_result: Result<()> = (|| {
            for diagnostic in &reply.errors {
                let available = MAX_DIAGNOSTIC_PREFIX_BYTES
                    .saturating_sub(state.diagnostic_utf8_bytes_retained);
                if state.diagnostic_prefix_sealed
                    || state.diagnostic_prefix.len() >= MAX_DIAGNOSTICS
                    || available == 0
                {
                    state.diagnostic_prefix_sealed = true;
                    continue;
                }
                let mut length = diagnostic.len().min(MAX_DIAGNOSTIC_BYTES).min(available);
                while !diagnostic.is_char_boundary(length) {
                    length -= 1;
                }
                let text = copy_string(&diagnostic[..length], MAX_DIAGNOSTIC_BYTES)?;
                state
                    .diagnostic_prefix
                    .try_reserve_exact(1)
                    .map_err(|_| trace_failure(TraceFailure::Allocation))?;
                if state.diagnostic_prefix.capacity() > MAX_DIAGNOSTICS {
                    return Err(trace_failure(TraceFailure::Capacity));
                }
                let omitted = diagnostic.len() - length;
                state.diagnostic_utf8_bytes_retained += length;
                if omitted != 0 {
                    state.diagnostic_prefix_sealed = true;
                }
                state.diagnostic_prefix.push(Diagnostic {
                    transaction,
                    text,
                    original_utf8_bytes: diagnostic.len(),
                    omitted_utf8_bytes: omitted,
                });
            }
            Ok(())
        })();
        // Even a failed native copy retains exact totals and explicit omissions.
        state.diagnostics_omitted = state.diagnostics_total
            - state
                .diagnostic_prefix
                .iter()
                .filter(|entry| entry.omitted_utf8_bytes == 0)
                .count();
        state.diagnostic_utf8_bytes_omitted =
            state.diagnostic_utf8_bytes_total - state.diagnostic_utf8_bytes_retained;
        if let Err(failure) = diagnostic_result {
            state.diagnostic_prefix_sealed = true;
            return Err(failure);
        }
        state.raw_trace.push(RawTrace {
            generation,
            session_id,
            sequence,
            revision: reply.revision,
            state: reply.state,
            lifecycle: reply.lifecycle,
            observer: ObserverTrace::from_reply(reply),
            scripts_executed: reply.scripts_executed,
            diagnostics: reply.errors.len(),
            rejected_initial: reply.is_rejected_initial(),
            resources,
            wrapper_validation_ns: 0,
            manager_charge_ns: None,
            manager_charge_succeeded: false,
        });
        Ok(transaction)
    }
    fn validation(&self, index: usize, elapsed: Duration) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        let row = state
            .raw_trace
            .get_mut(index)
            .ok_or_else(|| trace_failure(TraceFailure::Missing))?;
        row.wrapper_validation_ns = nanos(elapsed);
        Ok(())
    }
    fn charge(&self, index: usize, elapsed: Duration, success: bool) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        let row = state
            .raw_trace
            .get_mut(index)
            .ok_or_else(|| trace_failure(TraceFailure::Missing))?;
        if row.manager_charge_ns.is_some() {
            return Err(trace_failure(TraceFailure::DuplicateAcknowledgement));
        }
        row.manager_charge_ns = Some(nanos(elapsed));
        row.manager_charge_succeeded = success;
        Ok(())
    }
    fn canceled(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.cancel_calls = state.cancel_calls.saturating_add(1);
        }
    }
    fn closed(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.close_calls = state.close_calls.saturating_add(1);
        }
    }
    fn waited(&self, success: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.wait_idle_calls = state.wait_idle_calls.saturating_add(1);
            state.wait_idle_failed |= !success;
            state.wait_idle_succeeded = success && !state.wait_idle_failed;
        }
    }
}
fn unavailable() -> HostFailure {
    trace_failure(TraceFailure::Unavailable)
}
fn trace_failure(reason: TraceFailure) -> HostFailure {
    HostFailure::Trace { reason }
}
fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}
fn copy_string(value: &str, bound: usize) -> Result<String> {
    if value.len() > bound {
        return Err(trace_failure(TraceFailure::Capacity));
    }
    let mut owned = String::new();
    owned
        .try_reserve_exact(value.len())
        .map_err(|_| trace_failure(TraceFailure::Allocation))?;
    if owned.capacity() > bound {
        return Err(trace_failure(TraceFailure::Capacity));
    }
    owned.push_str(value);
    Ok(owned)
}
fn copy_report(report: &Report) -> Result<Report> {
    Ok(Report {
        profile: copy_string(&report.profile, 64)?,
        opcodes_remaining: report.opcodes_remaining,
        source_bytes: report.source_bytes,
        source_admissions: report.source_admissions,
        jobs_admitted: report.jobs_admitted,
        jobs_executed: report.jobs_executed,
        pending_jobs: report.pending_jobs,
        fatal_reason: report
            .fatal_reason
            .as_deref()
            .map(|value| copy_string(value, 512))
            .transpose()?,
        worker_memory: report.worker_memory,
    })
}

#[derive(Default)]
struct StableArenaTags {
    tags: Vec<String>,
    bytes: usize,
}
struct StagedArenaTags {
    tags: Vec<String>,
    bytes: usize,
}
impl StableArenaTags {
    fn stage(&mut self, reply: &SessionReply) -> Result<StagedArenaTags> {
        let nodes = reply
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.nodes.as_slice());
        if nodes.len() > MAX_TAGS {
            return Err(HostFailure::Projection);
        }
        let appended = nodes.get(self.tags.len()..).unwrap_or_default();
        let bytes = appended
            .iter()
            .try_fold(0usize, |total, node| total.checked_add(node.tag.len()))
            .filter(|bytes| self.bytes.saturating_add(*bytes) <= MAX_TAG_BYTES)
            .ok_or(HostFailure::Projection)?;
        let mut tags = Vec::new();
        tags.try_reserve_exact(appended.len())
            .map_err(|_| HostFailure::Projection)?;
        self.tags
            .try_reserve_exact(appended.len())
            .map_err(|_| HostFailure::Projection)?;
        if tags.capacity() > MAX_TAGS || self.tags.capacity() > MAX_TAGS {
            return Err(HostFailure::Projection);
        }
        for node in appended {
            tags.push(copy_string(&node.tag, MAX_TAG_BYTES).map_err(|_| HostFailure::Projection)?);
        }
        Ok(StagedArenaTags { tags, bytes })
    }
    fn commit(&mut self, staged: StagedArenaTags) {
        // Capacity was reserved before the single inner charge; no allocation.
        self.tags.extend(staged.tags);
        self.bytes += staged.bytes;
    }
    fn validate(&self, nodes: &[Node]) -> Result<()> {
        if nodes.len() < self.tags.len()
            || self
                .tags
                .iter()
                .zip(nodes)
                .any(|(tag, node)| *tag != node.tag)
        {
            return Err(HostFailure::Projection);
        }
        Ok(())
    }
}

// Mechanically the same denial predicate as the reviewed Browser/static guards.
fn denied_policy(reply: &SessionReply) -> Option<PolicyFailure> {
    if reply.navigation.is_some() {
        return Some(PolicyFailure::Navigation);
    }
    if reply.default_action != DefaultAction::None {
        return Some(PolicyFailure::DefaultAction);
    }
    reply
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| {
            snapshot.nodes.iter().any(|node| {
                node.tag.eq_ignore_ascii_case("meta")
                    && node.attributes.iter().any(|(name, value)| {
                        name.eq_ignore_ascii_case("http-equiv")
                            && value.trim().eq_ignore_ascii_case("refresh")
                    })
            })
        })
        .then_some(PolicyFailure::MetaRefresh)
}
fn validate_policy_rejection(
    reply: &mut SessionReply,
    page_url: &str,
    previous: Option<&StableArenaTags>,
) -> Result<()> {
    let Some(reason) = denied_policy(reply) else {
        return Ok(());
    };
    // Only a denial consumes the existing arena. Normal projection stays Chassis's.
    let snapshot = reply.snapshot.take().ok_or(HostFailure::Projection)?;
    if let Some(previous) = previous {
        previous.validate(&snapshot.nodes)?;
    }
    document::project_nodes(snapshot.nodes, page_url).map_err(|_| HostFailure::Projection)?;
    Err(HostFailure::Policy { reason })
}

pub struct InventoryScripts {
    inner: Arc<dyn ScriptRuntime>,
    inventory: SourceInventory,
    observer: ObserverInit,
    pub evidence: Arc<HostState>,
    generation: AtomicU64,
    started: AtomicBool,
    closed: Arc<AtomicBool>,
    cutoff: Instant,
}
impl InventoryScripts {
    pub fn new(
        inner: Arc<dyn ScriptRuntime>,
        inventory: SourceInventory,
        observer: ObserverInit,
        evidence: Arc<HostState>,
        cutoff: Instant,
    ) -> Result<Self> {
        let check = || {
            if inner.source_wire_mode() != SourceWireMode::Modern {
                return Err(HostFailure::Configuration {
                    reason: ConfigurationFailure::WireMode,
                });
            }
            let ObserverInit::WindowHarness {
                contract: ObserverContract::PinnedWindowHarnessOutputDisabledV1,
                run_id,
                bootstrap_node,
            } = observer
            else {
                return Err(HostFailure::Configuration {
                    reason: ConfigurationFailure::Observer,
                });
            };
            if run_id == 0 {
                return Err(HostFailure::Configuration {
                    reason: ConfigurationFailure::Observer,
                });
            }
            if usize::try_from(bootstrap_node).ok() != Some(inventory.plan().harness_node) {
                return Err(HostFailure::Configuration {
                    reason: ConfigurationFailure::Bootstrap,
                });
            }
            if inner.observer_expectation() != observer.expectation() {
                return Err(HostFailure::Configuration {
                    reason: ConfigurationFailure::InnerExpectation,
                });
            }
            if Instant::now() >= cutoff {
                return Err(HostFailure::Cutoff);
            }
            evidence.check()
        };
        if let Err(failure) = check() {
            evidence.fail(failure);
            return Err(failure);
        }
        Ok(Self {
            inner,
            inventory,
            observer,
            evidence,
            generation: AtomicU64::new(0),
            started: AtomicBool::new(false),
            closed: Arc::new(AtomicBool::new(false)),
            cutoff,
        })
    }
    fn admit(&self, generation: u64) -> Result<()> {
        let failure = if self.inner.source_wire_mode() != SourceWireMode::Modern {
            Some(HostFailure::Configuration {
                reason: ConfigurationFailure::WireMode,
            })
        } else if self.inner.observer_expectation() != self.observer.expectation() {
            Some(HostFailure::Configuration {
                reason: ConfigurationFailure::InnerExpectation,
            })
        } else if self.closed.load(Ordering::Acquire) {
            Some(HostFailure::Closed)
        } else if Instant::now() >= self.cutoff {
            Some(HostFailure::Cutoff)
        } else if generation == 0 || self.generation.load(Ordering::Acquire) != generation {
            Some(HostFailure::Generation {
                reason: GenerationFailure::Mismatch,
            })
        } else {
            None
        };
        if let Some(failure) = failure {
            self.evidence.fail(failure);
            return Err(failure);
        }
        self.evidence.check()
    }
}
impl ScriptRuntime for InventoryScripts {
    fn observer_expectation(&self) -> ObserverExpectation {
        self.observer.expectation()
    }
    fn source_wire_mode(&self) -> SourceWireMode {
        SourceWireMode::Modern
    }
    fn set_generation(&self, generation: u64) -> std::result::Result<(), String> {
        let failure = if generation == 0 {
            Some(HostFailure::Generation {
                reason: GenerationFailure::Zero,
            })
        } else if self.closed.load(Ordering::Acquire) {
            Some(HostFailure::Closed)
        } else if Instant::now() >= self.cutoff {
            Some(HostFailure::Cutoff)
        } else if self
            .generation
            .compare_exchange(0, generation, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            Some(HostFailure::Generation {
                reason: GenerationFailure::Reused,
            })
        } else {
            None
        };
        if let Some(failure) = failure {
            self.evidence.fail(failure);
            return Err(failure.to_string());
        }
        self.evidence
            .check()
            .map_err(|failure| failure.to_string())?;
        self.inner.set_generation(generation).map_err(|error| {
            self.evidence.fail(HostFailure::Generation {
                reason: GenerationFailure::InnerRejected,
            });
            error
        })
    }
    fn start(
        &self,
        request: Request,
        generation: u64,
    ) -> std::result::Result<(Box<dyn ScriptSession>, SessionReply), String> {
        let validation_started = Instant::now();
        self.admit(generation)
            .map_err(|failure| failure.to_string())?;
        if let Err(error) = self.inventory.verify(&request) {
            let failure = HostFailure::Source { error };
            self.evidence.fail(failure);
            return Err(failure.to_string());
        }
        self.admit(generation)
            .map_err(|failure| failure.to_string())?;
        if self
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            self.evidence.fail(HostFailure::RepeatedStart);
            return Err(HostFailure::RepeatedStart.to_string());
        }
        self.evidence.started().map_err(|failure| {
            self.evidence.fail(failure);
            failure.to_string()
        })?;
        let source_validation = validation_started.elapsed();
        let (mut session, mut reply) = self.inner.start(request, generation).map_err(|error| {
            self.evidence.fail(HostFailure::OpaqueTransport {
                phase: TransportPhase::Start,
            });
            error
        })?;
        let started = Instant::now();
        let session_id = session.id();
        let trace = match self
            .evidence
            .note_reply(&reply, generation, session_id, None)
        {
            Ok(trace) => trace,
            Err(failure) => {
                self.evidence.fail(failure);
                let charged =
                    session.charge_active(source_validation.saturating_add(started.elapsed()));
                session.cancel();
                self.evidence.canceled();
                if charged.is_err() {
                    self.evidence.fail(HostFailure::Acknowledgement);
                }
                return Err(charged.err().unwrap_or_else(|| failure.to_string()));
            }
        };
        let mut tags = StableArenaTags::default();
        let validation = validate_policy_rejection(&mut reply, self.inventory.document_url(), None)
            .and_then(|()| tags.stage(&reply));
        let staged = match validation {
            Ok(staged) => staged,
            Err(failure) => {
                self.evidence.fail(failure);
                let elapsed = source_validation.saturating_add(started.elapsed());
                let validation = self.evidence.validation(trace, elapsed);
                let charged = session.charge_active(elapsed);
                let recorded = self.evidence.charge(trace, elapsed, charged.is_ok());
                session.cancel();
                self.evidence.canceled();
                if charged.is_err() {
                    self.evidence.fail(HostFailure::Acknowledgement);
                }
                if let Err(failure) = validation.and(recorded) {
                    self.evidence.fail(failure);
                }
                return Err(charged.err().unwrap_or_else(|| failure.to_string()));
            }
        };
        let page_url = match copy_string(self.inventory.document_url(), 512) {
            Ok(url) => url,
            Err(failure) => {
                self.evidence.fail(failure);
                let elapsed = source_validation.saturating_add(started.elapsed());
                let charged = session.charge_active(elapsed);
                let _ = self.evidence.charge(trace, elapsed, charged.is_ok());
                session.cancel();
                self.evidence.canceled();
                if charged.is_err() {
                    self.evidence.fail(HostFailure::Acknowledgement);
                }
                return Err(charged.err().unwrap_or_else(|| failure.to_string()));
            }
        };
        let mut wrapped = Box::new(NoNavigationSession {
            inner: session,
            pending_validation: Duration::ZERO,
            page_url,
            tags,
            staged_tags: Some(staged),
            acknowledgement_failed: false,
            evidence: self.evidence.clone(),
            pending_trace: Some(trace),
            generation,
            session_id,
            expected_sequence: None,
            closed: self.closed.clone(),
            canceled: AtomicBool::new(false),
            cutoff: self.cutoff,
        });
        let elapsed = source_validation.saturating_add(started.elapsed());
        if let Err(failure) = self.evidence.validation(trace, elapsed) {
            self.evidence.fail(failure);
            let charged = wrapped.inner.charge_active(elapsed);
            wrapped.inner.cancel();
            self.evidence.canceled();
            if charged.is_err() {
                self.evidence.fail(HostFailure::Acknowledgement);
            }
            return Err(charged.err().unwrap_or_else(|| failure.to_string()));
        }
        wrapped.pending_validation = source_validation.saturating_add(started.elapsed());
        Ok((wrapped, reply))
    }
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.evidence.closed();
        self.inner.close();
    }
    fn wait_idle(&self) -> std::result::Result<(), String> {
        let result = self.inner.wait_idle();
        self.evidence.waited(result.is_ok());
        if result.is_err() {
            self.evidence.fail(HostFailure::Cleanup);
        }
        result
    }
}

struct NoNavigationSession {
    inner: Box<dyn ScriptSession>,
    pending_validation: Duration,
    page_url: String,
    tags: StableArenaTags,
    staged_tags: Option<StagedArenaTags>,
    acknowledgement_failed: bool,
    evidence: Arc<HostState>,
    pending_trace: Option<usize>,
    generation: u64,
    session_id: u64,
    expected_sequence: Option<u64>,
    closed: Arc<AtomicBool>,
    canceled: AtomicBool,
    cutoff: Instant,
}
impl NoNavigationSession {
    fn stop(&mut self, failure: HostFailure) {
        self.evidence.fail(failure);
        self.acknowledgement_failed = true;
        self.staged_tags = None;
        self.pending_trace = None;
        self.pending_validation = Duration::ZERO;
        self.expected_sequence = None;
        self.inner.cancel();
        self.evidence.canceled();
    }
    fn rejected_update(
        &mut self,
        trace: Option<usize>,
        started: Instant,
        failure: HostFailure,
    ) -> String {
        self.evidence.fail(failure);
        let elapsed = started.elapsed();
        if let Some(trace) = trace {
            if let Err(failure) = self.evidence.validation(trace, elapsed) {
                self.evidence.fail(failure);
            }
        }
        let charged = self.inner.charge_active(elapsed);
        if let Some(trace) = trace {
            if let Err(failure) = self.evidence.charge(trace, elapsed, charged.is_ok()) {
                self.evidence.fail(failure);
            }
        }
        if charged.is_err() {
            self.evidence.fail(HostFailure::Acknowledgement);
        }
        self.acknowledgement_failed = true;
        self.staged_tags = None;
        self.pending_trace = None;
        self.pending_validation = Duration::ZERO;
        self.expected_sequence = None;
        self.inner.cancel();
        self.evidence.canceled();
        charged.err().unwrap_or_else(|| failure.to_string())
    }
}
impl ScriptSession for NoNavigationSession {
    fn id(&self) -> u64 {
        self.inner.id()
    }
    fn try_dispatch(&mut self, _: SessionInput, _: u64) -> std::result::Result<u64, String> {
        self.stop(HostFailure::ActivationRefused);
        Err(HostFailure::ActivationRefused.to_string())
    }
    fn try_task_pump(&mut self, pump: TaskPump, revision: u64) -> std::result::Result<u64, String> {
        let failure = if self.acknowledgement_failed
            || self.closed.load(Ordering::Acquire)
            || self.canceled.load(Ordering::Acquire)
        {
            Some(HostFailure::Closed)
        } else if Instant::now() >= self.cutoff {
            Some(HostFailure::Cutoff)
        } else if self.pending_trace.is_some() || self.expected_sequence.is_some() {
            Some(HostFailure::ReplyOverlap)
        } else {
            None
        };
        if let Some(failure) = failure {
            self.stop(failure);
            return Err(failure.to_string());
        }
        if let Err(failure) = self.evidence.admit_transaction() {
            self.stop(failure);
            return Err(failure.to_string());
        }
        match self.inner.try_task_pump(pump, revision) {
            Ok(sequence) => {
                self.expected_sequence = Some(sequence);
                Ok(sequence)
            }
            Err(error) => {
                self.stop(HostFailure::OpaqueTransport {
                    phase: TransportPhase::TaskAdmission,
                });
                Err(error)
            }
        }
    }
    fn try_recv(&mut self) -> Option<SessionUpdate> {
        if self.acknowledgement_failed
            || self.canceled.load(Ordering::Acquire)
            || self.closed.load(Ordering::Acquire)
        {
            return None;
        }
        if let Err(failure) = self.evidence.check() {
            self.stop(failure);
            return None;
        }
        if self.pending_trace.is_some() {
            return None;
        }
        if Instant::now() >= self.cutoff {
            self.stop(HostFailure::Cutoff);
            return None;
        }
        let mut update = self.inner.try_recv()?;
        let started = Instant::now();
        if update.generation != self.generation
            || update.session_id != self.session_id
            || update.sequence != self.expected_sequence
            || self.expected_sequence.is_none()
        {
            update.reply = Err(self.rejected_update(
                None,
                started,
                trace_failure(TraceFailure::ReplyIdentity),
            ));
            return Some(update);
        }
        self.expected_sequence = None;
        match &mut update.reply {
            Ok(reply) => {
                let trace = match self.evidence.note_reply(
                    reply,
                    update.generation,
                    update.session_id,
                    update.sequence,
                ) {
                    Ok(trace) => trace,
                    Err(failure) => {
                        update.reply = Err(self.rejected_update(None, started, failure));
                        return Some(update);
                    }
                };
                let validation = validate_policy_rejection(reply, &self.page_url, Some(&self.tags))
                    .and_then(|()| self.tags.stage(reply));
                match validation {
                    Err(failure) => {
                        update.reply = Err(self.rejected_update(Some(trace), started, failure))
                    }
                    Ok(staged) => {
                        self.staged_tags = Some(staged);
                        self.pending_trace = Some(trace);
                        let elapsed = started.elapsed();
                        if let Err(failure) = self.evidence.validation(trace, elapsed) {
                            update.reply = Err(self.rejected_update(Some(trace), started, failure));
                        } else {
                            self.pending_validation = started.elapsed();
                        }
                    }
                }
            }
            Err(_) => {
                // Inner transport details remain opaque; never infer budget/signal.
                self.stop(HostFailure::OpaqueTransport {
                    phase: TransportPhase::Reply,
                });
            }
        }
        Some(update)
    }
    fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
        self.inner.cancel();
        self.evidence.canceled();
    }
    fn charge_active(&mut self, duration: Duration) -> std::result::Result<(), String> {
        let started = Instant::now();
        if self.acknowledgement_failed
            || self.canceled.load(Ordering::Acquire)
            || self.closed.load(Ordering::Acquire)
        {
            self.stop(HostFailure::Closed);
            return Err(HostFailure::Closed.to_string());
        }
        let Some(trace) = self.pending_trace.take() else {
            let failure = trace_failure(TraceFailure::Missing);
            self.stop(failure);
            return Err(failure.to_string());
        };
        let validation = std::mem::take(&mut self.pending_validation);
        let staged = self.staged_tags.take();
        let prior_failure = self.evidence.check().err();
        let total = duration
            .saturating_add(validation)
            .saturating_add(started.elapsed());
        let charged = self.inner.charge_active(total);
        let recorded = self.evidence.charge(trace, total, charged.is_ok());
        if charged.is_ok() && recorded.is_ok() && prior_failure.is_none() {
            if let Some(staged) = staged {
                self.tags.commit(staged);
            }
            Ok(())
        } else {
            let failure = prior_failure
                .or_else(|| recorded.err())
                .unwrap_or(HostFailure::Acknowledgement);
            self.stop(failure);
            Err(charged.err().unwrap_or_else(|| failure.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::source::{CASES, HELPERS};
    use super::*;
    use mg_sparkle::{
        js_browser::{ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource},
        page_session::{ArenaSnapshot, EventOutcome, InputKind, LifecyclePhase},
    };

    fn inventory() -> SourceInventory {
        SourceInventory::new(
            &CASES[0],
            &format!("http://127.0.0.1:7878/{}", CASES[0].path()),
        )
        .unwrap()
    }
    fn observer(value: &SourceInventory) -> ObserverInit {
        ObserverInit::WindowHarness {
            contract: ObserverContract::PinnedWindowHarnessOutputDisabledV1,
            run_id: 11,
            bootstrap_node: value.plan().harness_node as u32,
        }
    }
    fn request(value: &SourceInventory) -> Request {
        Request {
            url: value.document_url().into(),
            html: value.case().html().into(),
            external_scripts: Some(
                value
                    .plan()
                    .external_nodes()
                    .iter()
                    .enumerate()
                    .map(|(index, node)| ExternalScriptSource {
                        node: *node,
                        source: HELPERS[index].body.into(),
                        provenance: Some(ClassicSourceProvenance {
                            response_url: value.helper_url(index).unwrap(),
                            error_visibility: ClassicErrorVisibility::SameOrigin,
                        }),
                    })
                    .collect(),
            ),
        }
    }
    fn reply() -> SessionReply {
        SessionReply {
            revision: 0,
            snapshot: Some(ArenaSnapshot {
                nodes: document::parse("<p>owned</p>", "http://127.0.0.1:7878/").nodes,
            }),
            outcome: EventOutcome::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: vec![],
            scripts_executed: 4,
            allocations: None,
            boa: None,
            state: RealmState::Ready,
            acknowledgements: vec![],
            task_wake: None,
            lifecycle: Some(LifecycleProgress {
                phase: LifecyclePhase::Interactive,
                initial_failure: false,
            }),
            observer: Some(ObserverReply::WindowHarness {
                run_id: 11,
                first_ordinal: 0,
                next_ordinal: 0,
                records: vec![],
                progress: ObserverProgress::Waiting,
                execution_failed: false,
            }),
            local_storage_ops: vec![],
            session_storage_ops: vec![],
            cookie_writes: vec![],
            history_ops: vec![],
            fetch_requests: vec![],
            scroll_request: None,
        }
    }
    #[derive(Default)]
    struct Stats {
        generations: Vec<u64>,
        starts: usize,
        charges: Vec<Duration>,
        pumps: Vec<(u64, u64)>,
        canceled: usize,
        closed: usize,
        waits: usize,
        fail_generation: bool,
        fail_charge: bool,
        fail_wait: bool,
        bad_reply_identity: bool,
        initial: Option<SessionReply>,
        later: Option<SessionReply>,
    }
    struct FakeRuntime {
        shared: Arc<Mutex<Stats>>,
        mode: SourceWireMode,
        expected: ObserverExpectation,
    }
    struct FakeSession {
        shared: Arc<Mutex<Stats>>,
        awaiting: bool,
    }
    impl ScriptRuntime for FakeRuntime {
        fn observer_expectation(&self) -> ObserverExpectation {
            self.expected
        }
        fn source_wire_mode(&self) -> SourceWireMode {
            self.mode
        }
        fn set_generation(&self, generation: u64) -> std::result::Result<(), String> {
            let mut s = self.shared.lock().unwrap();
            s.generations.push(generation);
            if s.fail_generation {
                Err("opaque generation error".into())
            } else {
                Ok(())
            }
        }
        fn start(
            &self,
            _: Request,
            _: u64,
        ) -> std::result::Result<(Box<dyn ScriptSession>, SessionReply), String> {
            let mut s = self.shared.lock().unwrap();
            s.starts += 1;
            let reply = s.initial.take().unwrap();
            Ok((
                Box::new(FakeSession {
                    shared: self.shared.clone(),
                    awaiting: true,
                }),
                reply,
            ))
        }
        fn close(&self) {
            self.shared.lock().unwrap().closed += 1;
        }
        fn wait_idle(&self) -> std::result::Result<(), String> {
            let mut s = self.shared.lock().unwrap();
            s.waits += 1;
            if s.fail_wait {
                Err("opaque wait error".into())
            } else {
                Ok(())
            }
        }
    }
    impl ScriptSession for FakeSession {
        fn id(&self) -> u64 {
            9
        }
        fn try_dispatch(&mut self, _: SessionInput, _: u64) -> std::result::Result<u64, String> {
            panic!("activation reached inner");
        }
        fn try_task_pump(
            &mut self,
            pump: TaskPump,
            revision: u64,
        ) -> std::result::Result<u64, String> {
            assert!(!self.awaiting);
            let mut state = self.shared.lock().unwrap();
            state.pumps.push((pump.wake_token, revision));
            Ok(6 + state.pumps.len() as u64)
        }
        fn try_recv(&mut self) -> Option<SessionUpdate> {
            assert!(!self.awaiting);
            let mut state = self.shared.lock().unwrap();
            let reply = state.later.take()?;
            let sequence = 6 + state.pumps.len() as u64;
            let sequence = sequence + u64::from(state.bad_reply_identity);
            drop(state);
            self.awaiting = true;
            Some(SessionUpdate {
                generation: 3,
                session_id: 9,
                sequence: Some(sequence),
                reply: Ok(reply),
                automation: None,
                continuation: mg_chassis::scripts::SessionContinuation::Live,
            })
        }
        fn cancel(&self) {
            self.shared.lock().unwrap().canceled += 1;
        }
        fn charge_active(&mut self, duration: Duration) -> std::result::Result<(), String> {
            assert!(self.awaiting, "duplicate real inner charge");
            self.awaiting = false;
            let mut s = self.shared.lock().unwrap();
            s.charges.push(duration);
            if s.fail_charge {
                Err("opaque charge error".into())
            } else {
                Ok(())
            }
        }
    }
    fn runtime(initial: SessionReply) -> (InventoryScripts, Arc<Mutex<Stats>>) {
        let shared = Arc::new(Mutex::new(Stats {
            initial: Some(initial),
            ..Stats::default()
        }));
        let value = inventory();
        let init = observer(&value);
        let inner = Arc::new(FakeRuntime {
            shared: shared.clone(),
            mode: SourceWireMode::Modern,
            expected: init.expectation(),
        });
        let guard = InventoryScripts::new(
            inner,
            value,
            init,
            Arc::new(HostState::new()),
            Instant::now() + Duration::from_secs(20),
        )
        .unwrap();
        (guard, shared)
    }
    fn first_failure(host: &InventoryScripts) -> HostFailure {
        host.evidence.summary().unwrap().first_failure.unwrap()
    }

    #[test]
    fn actual_three_source_mutations_stop_before_any_inner_start() {
        for mutation in 0..9 {
            let (host, shared) = runtime(reply());
            host.set_generation(3).unwrap();
            let mut input = request(&host.inventory);
            match mutation {
                0 => input.html.push(' '),
                1 => input.url.push_str("?wrong"),
                2 => input.external_scripts = None,
                3 => {
                    input.external_scripts.as_mut().unwrap().pop();
                }
                4 => input.external_scripts.as_mut().unwrap()[2].source.push(' '),
                5 => {
                    input.external_scripts.as_mut().unwrap()[0].node =
                        host.inventory.plan().inline_node
                }
                6 => input.external_scripts.as_mut().unwrap().swap(0, 2),
                7 => input.external_scripts.as_mut().unwrap()[1].provenance = None,
                _ => {
                    input.external_scripts.as_mut().unwrap()[2]
                        .provenance
                        .as_mut()
                        .unwrap()
                        .error_visibility = ClassicErrorVisibility::Muted
                }
            }
            assert!(host.start(input, 3).is_err());
            assert_eq!(shared.lock().unwrap().starts, 0);
            assert!(matches!(first_failure(&host), HostFailure::Source { .. }));
        }
    }
    #[test]
    fn constructor_requires_modern_exact_enabled_configuration() {
        for mutation in 0..6 {
            let value = inventory();
            let mut init = observer(&value);
            let shared = Arc::new(Mutex::new(Stats::default()));
            let mut mode = SourceWireMode::Modern;
            let mut expected = init.expectation();
            match mutation {
                0 => mode = SourceWireMode::Legacy,
                1 => init = ObserverInit::Disabled,
                2 => {
                    if let ObserverInit::WindowHarness {
                        ref mut bootstrap_node,
                        ..
                    } = init
                    {
                        *bootstrap_node += 1;
                    }
                }
                3 => expected = ObserverExpectation::WindowHarness { run_id: 12 },
                4 => {
                    if let ObserverInit::WindowHarness {
                        ref mut contract, ..
                    } = init
                    {
                        *contract = ObserverContract::PinnedWindowHarnessV1;
                    }
                }
                _ => {
                    if let ObserverInit::WindowHarness { ref mut run_id, .. } = init {
                        *run_id = 0;
                    }
                }
            }
            let inner = Arc::new(FakeRuntime {
                shared: shared.clone(),
                mode,
                expected,
            });
            assert!(
                InventoryScripts::new(
                    inner,
                    value,
                    init,
                    Arc::new(HostState::new()),
                    Instant::now() + Duration::from_secs(20)
                )
                .is_err()
            );
            assert_eq!(shared.lock().unwrap().starts, 0);
        }
    }
    #[test]
    fn first_nonzero_generation_only_and_failed_forwarding_never_starts() {
        let (host, s) = runtime(reply());
        assert!(host.set_generation(0).is_err());
        assert!(s.lock().unwrap().generations.is_empty());
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        assert!(host.set_generation(4).is_err());
        assert_eq!(s.lock().unwrap().generations, vec![3]);
        assert!(host.start(request(&host.inventory), 3).is_err());
        let (host, s) = runtime(reply());
        s.lock().unwrap().fail_generation = true;
        assert!(host.set_generation(3).is_err());
        assert!(host.start(request(&host.inventory), 3).is_err());
        assert_eq!(s.lock().unwrap().starts, 0);
    }
    #[test]
    fn wrapper_validation_and_initial_projection_share_one_real_charge() {
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        assert!(s.lock().unwrap().charges.is_empty());
        session.charge_active(Duration::from_millis(7)).unwrap();
        let snap = host.evidence.snapshot().unwrap();
        assert_eq!(snap.raw_trace.len(), 1);
        assert!(snap.raw_trace[0].wrapper_validation_ns > 0);
        assert!(snap.raw_trace[0].manager_charge_succeeded);
        assert!(s.lock().unwrap().charges[0] >= Duration::from_millis(7));
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert_eq!(s.lock().unwrap().charges.len(), 1);
        assert!(
            session
                .try_task_pump(TaskPump { wake_token: 1 }, 0)
                .is_err()
        );
    }
    #[test]
    fn policy_denials_are_typed_and_charge_once_without_fetch_authority() {
        for policy in 0..3 {
            let mut initial = reply();
            let expected = match policy {
                0 => {
                    initial.navigation = Some("http://127.0.0.1:7878/trap".into());
                    PolicyFailure::Navigation
                }
                1 => {
                    initial.snapshot = Some(ArenaSnapshot {
                        nodes: document::parse(
                            "<meta http-equiv=refresh content='0;url=/trap'>",
                            "http://127.0.0.1:7878/",
                        )
                        .nodes,
                    });
                    PolicyFailure::MetaRefresh
                }
                _ => {
                    initial.snapshot = Some(ArenaSnapshot {
                        nodes: document::parse(
                            "<a href='/trap'>trap</a>",
                            "http://127.0.0.1:7878/",
                        )
                        .nodes,
                    });
                    initial.default_action = DefaultAction::FollowLink { node: 1 };
                    PolicyFailure::DefaultAction
                }
            };
            let (host, s) = runtime(initial);
            host.set_generation(3).unwrap();
            assert!(host.start(request(&host.inventory), 3).is_err());
            assert_eq!(s.lock().unwrap().charges.len(), 1);
            assert_eq!(s.lock().unwrap().canceled, 1);
            assert_eq!(
                first_failure(&host),
                HostFailure::Policy { reason: expected }
            );
        }
    }
    #[test]
    fn malformed_denials_keep_projection_error_ahead_of_policy_failure() {
        for meta in [false, true] {
            let mut initial = reply();
            if meta {
                initial.snapshot = Some(ArenaSnapshot {
                    nodes: document::parse("<meta http-equiv=refresh>", "http://127.0.0.1:7878/")
                        .nodes,
                });
            } else {
                initial.navigation = Some("http://127.0.0.1:7878/trap".into());
            }
            initial.snapshot.as_mut().unwrap().nodes[1].parent = usize::MAX;
            let (host, s) = runtime(initial);
            host.set_generation(3).unwrap();
            assert!(host.start(request(&host.inventory), 3).is_err());
            assert_eq!(first_failure(&host), HostFailure::Projection);
            assert_eq!(s.lock().unwrap().charges.len(), 1);
        }
    }
    #[test]
    fn reportless_rejected_init_remains_admitted_not_a_missing_arena_error() {
        let mut initial = reply();
        initial.snapshot = None;
        initial.scripts_executed = 0;
        initial.state = RealmState::Closed;
        initial.lifecycle = Some(LifecycleProgress {
            phase: LifecyclePhase::Initializing,
            initial_failure: true,
        });
        initial.observer = Some(ObserverReply::rejected_init(
            ObserverExpectation::WindowHarness { run_id: 11 },
        ));
        assert!(initial.is_rejected_initial());
        let (host, s) = runtime(initial);
        host.set_generation(3).unwrap();
        let (mut session, returned) = host.start(request(&host.inventory), 3).unwrap();
        assert!(returned.is_rejected_initial());
        session.charge_active(Duration::ZERO).unwrap();
        assert!(host.evidence.check().is_ok());
        assert_eq!(s.lock().unwrap().charges.len(), 1);
    }
    #[test]
    fn failed_ack_is_sticky_and_never_forwards_next_pump() {
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        s.lock().unwrap().fail_charge = true;
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert!(
            session
                .try_task_pump(TaskPump { wake_token: 1 }, 0)
                .is_err()
        );
        assert!(session.try_recv().is_none());
        assert_eq!(s.lock().unwrap().charges.len(), 1);
        assert!(s.lock().unwrap().pumps.is_empty());
        assert_eq!(first_failure(&host), HostFailure::Acknowledgement);
        assert!(!host.evidence.snapshot().unwrap().raw_trace[0].manager_charge_succeeded);
    }
    #[test]
    fn failed_ack_drops_staged_identities_without_committing_tag_prefix() {
        let initial = reply();
        let evidence = Arc::new(HostState::new());
        let trace = evidence.note_reply(&initial, 3, 9, None).unwrap();
        let mut tags = StableArenaTags::default();
        let staged = tags.stage(&initial).unwrap();
        assert!(!staged.tags.is_empty());
        let shared = Arc::new(Mutex::new(Stats {
            fail_charge: true,
            ..Stats::default()
        }));
        let mut session = NoNavigationSession {
            inner: Box::new(FakeSession {
                shared: shared.clone(),
                awaiting: true,
            }),
            pending_validation: Duration::from_nanos(1),
            page_url: "http://127.0.0.1:7878/".into(),
            tags,
            staged_tags: Some(staged),
            acknowledgement_failed: false,
            evidence,
            pending_trace: Some(trace),
            generation: 3,
            session_id: 9,
            expected_sequence: None,
            closed: Arc::new(AtomicBool::new(false)),
            canceled: AtomicBool::new(false),
            cutoff: Instant::now() + Duration::from_secs(20),
        };
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert!(session.tags.tags.is_empty());
        assert_eq!(session.tags.bytes, 0);
        assert!(session.staged_tags.is_none());
        assert!(session.pending_trace.is_none());
        assert_eq!(shared.lock().unwrap().charges.len(), 1);
    }
    #[test]
    fn actual_task_forwarding_has_one_flight_and_one_charge_per_reply() {
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        session.charge_active(Duration::ZERO).unwrap();
        let mut later = reply();
        later.revision = 1;
        later.scripts_executed = 0;
        s.lock().unwrap().later = Some(later);
        assert_eq!(
            session
                .try_task_pump(TaskPump { wake_token: 77 }, 0)
                .unwrap(),
            7
        );
        assert!(session.try_recv().unwrap().reply.is_ok());
        assert_eq!(s.lock().unwrap().charges.len(), 1);
        session.charge_active(Duration::from_millis(3)).unwrap();
        assert_eq!(s.lock().unwrap().charges.len(), 2);
        assert_eq!(s.lock().unwrap().pumps, vec![(77, 0)]);
        assert_eq!(
            host.evidence.snapshot().unwrap().raw_trace[1].sequence,
            Some(7)
        );
    }
    #[test]
    fn forged_reply_identity_is_not_retained_or_accepted_and_charges_once() {
        let (host, shared) = runtime(reply());
        host.set_generation(3).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        session.charge_active(Duration::ZERO).unwrap();
        {
            let mut state = shared.lock().unwrap();
            state.later = Some(reply());
            state.bad_reply_identity = true;
        }
        session
            .try_task_pump(TaskPump { wake_token: 1 }, 0)
            .unwrap();
        assert!(session.try_recv().unwrap().reply.is_err());
        assert_eq!(shared.lock().unwrap().charges.len(), 2);
        assert_eq!(host.evidence.snapshot().unwrap().raw_trace.len(), 1);
        assert_eq!(
            first_failure(&host),
            trace_failure(TraceFailure::ReplyIdentity)
        );
        assert!(session.try_recv().is_none());
    }
    #[test]
    fn later_policy_denial_rechecks_only_acknowledged_stable_tag_prefix() {
        for remove in [false, true] {
            let (host, s) = runtime(reply());
            host.set_generation(3).unwrap();
            let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
            session.charge_active(Duration::ZERO).unwrap();
            let mut later = reply();
            later.revision = 1;
            later.navigation = Some("http://127.0.0.1:7878/trap".into());
            if remove {
                later.snapshot.as_mut().unwrap().nodes.truncate(1);
            } else {
                later.snapshot.as_mut().unwrap().nodes[1].tag = "retargeted".into();
            }
            s.lock().unwrap().later = Some(later);
            session
                .try_task_pump(TaskPump { wake_token: 1 }, 0)
                .unwrap();
            assert!(session.try_recv().unwrap().reply.is_err());
            assert_eq!(s.lock().unwrap().charges.len(), 2);
            assert_eq!(first_failure(&host), HostFailure::Projection);
        }
    }
    #[test]
    fn repeated_start_cutoff_activation_and_close_cannot_resume_execution() {
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        assert!(host.start(request(&host.inventory), 3).is_err());
        assert_eq!(s.lock().unwrap().starts, 1);
        session.cancel();
        assert!(
            session
                .try_task_pump(TaskPump { wake_token: 1 }, 0)
                .is_err()
        );
        let (mut host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        host.cutoff = Instant::now();
        assert!(host.start(request(&host.inventory), 3).is_err());
        assert_eq!(s.lock().unwrap().starts, 0);
        assert_eq!(first_failure(&host), HostFailure::Cutoff);
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        session.charge_active(Duration::ZERO).unwrap();
        host.close();
        host.wait_idle().unwrap();
        assert_eq!(s.lock().unwrap().closed, 1);
        assert_eq!(s.lock().unwrap().waits, 1);
        assert!(host.evidence.summary().unwrap().wait_idle_succeeded);
        assert!(
            session
                .try_task_pump(TaskPump { wake_token: 1 }, 0)
                .is_err()
        );
    }
    #[test]
    fn wrong_generation_and_activation_never_reach_inner_execution() {
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        assert!(host.start(request(&host.inventory), 4).is_err());
        assert_eq!(s.lock().unwrap().starts, 0);
        assert_eq!(
            first_failure(&host),
            HostFailure::Generation {
                reason: GenerationFailure::Mismatch
            }
        );
        let (host, s) = runtime(reply());
        host.set_generation(3).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        session.charge_active(Duration::ZERO).unwrap();
        assert!(
            session
                .try_dispatch(
                    SessionInput {
                        kind: InputKind::Click { target: 1 },
                        edits: vec![]
                    },
                    0
                )
                .is_err()
        );
        assert_eq!(first_failure(&host), HostFailure::ActivationRefused);
        assert!(s.lock().unwrap().pumps.is_empty());
        assert_eq!(s.lock().unwrap().charges.len(), 1);
        assert!(session.try_recv().is_none());
    }
    #[test]
    fn failed_explicit_wait_cannot_be_overwritten_by_a_later_clean_wait() {
        let (host, s) = runtime(reply());
        host.close();
        s.lock().unwrap().fail_wait = true;
        assert!(host.wait_idle().is_err());
        s.lock().unwrap().fail_wait = false;
        assert!(host.wait_idle().is_ok());
        let snapshot = host.evidence.snapshot().unwrap();
        assert!(snapshot.wait_idle_failed);
        assert!(!snapshot.wait_idle_succeeded);
        assert_eq!(first_failure(&host), HostFailure::Cleanup);
    }
    #[test]
    fn diagnostic_prefix_is_lossless_until_explicit_utf8_truncation_and_omissions() {
        let state = HostState::new();
        let mut first = reply();
        first.errors = vec![
            "first".into(),
            "é".repeat(3000),
            "never silently substituted".into(),
        ];
        state.note_reply(&first, 3, 9, None).unwrap();
        let snap = state.snapshot().unwrap();
        assert_eq!(snap.diagnostics_total, 3);
        assert_eq!(snap.diagnostic_prefix.len(), 2);
        assert_eq!(snap.diagnostic_prefix[0].text, "first");
        assert_eq!(snap.diagnostic_prefix[1].text, "é".repeat(2048));
        assert_eq!(snap.diagnostics_omitted, 2);
        assert!(snap.diagnostic_prefix_sealed);
        assert_eq!(
            snap.diagnostic_utf8_bytes_total,
            snap.diagnostic_utf8_bytes_retained + snap.diagnostic_utf8_bytes_omitted
        );
        let mut next = reply();
        next.errors = vec!["later".into()];
        state.note_reply(&next, 3, 9, Some(1)).unwrap();
        let snap = state.snapshot().unwrap();
        assert_eq!(snap.diagnostics_total, 4);
        assert_eq!(snap.diagnostics_omitted, 3);
        assert_eq!(snap.diagnostic_prefix.len(), 2);
    }
    #[test]
    fn trace_bound_and_serialized_receipt_remain_bounded_without_result_clones() {
        let state = HostState::new();
        let mut value = reply();
        value.errors = vec!["\0".repeat(MAX_DIAGNOSTIC_BYTES); MAX_DIAGNOSTICS];
        // Synthetic typed metadata boundary, not real-worker execution evidence.
        value.boa = Some(Report {
            profile: "boa-page-process-v1".into(),
            opcodes_remaining: 1,
            source_bytes: 1,
            source_admissions: 1,
            jobs_admitted: 0,
            jobs_executed: 0,
            pending_jobs: 0,
            fatal_reason: Some("\0".repeat(512)),
            worker_memory: None,
        });
        for sequence in 0..MAX_TRANSACTIONS {
            state
                .note_reply(&value, 3, 9, Some(sequence as u64))
                .unwrap();
        }
        assert_eq!(
            state.note_reply(&value, 3, 9, Some(65)),
            Err(trace_failure(TraceFailure::Capacity))
        );
        let snap = state.snapshot().unwrap();
        assert_eq!(snap.raw_trace.len(), 64);
        assert_eq!(snap.diagnostic_prefix.len(), 64);
        assert_eq!(
            snap.diagnostic_utf8_bytes_retained,
            MAX_DIAGNOSTIC_PREFIX_BYTES
        );
        assert_eq!(snap.diagnostics_total, 4096);
        assert_eq!(snap.diagnostics_omitted, 4032);
        assert!(
            snap.raw_trace
                .iter()
                .all(|trace| trace.resources.as_ref() == value.boa.as_ref())
        );
        assert!(
            snap.first_failure.is_none(),
            "fatal text is retained, not a host-classified cause"
        );
        assert!(serde_json::to_vec(&snap).unwrap().len() < MAX_RECEIPT_BYTES);
    }
    #[test]
    fn sixty_four_transaction_prefix_never_grants_a_sixty_fifth_command() {
        let (host, shared) = runtime(reply());
        host.set_generation(3).unwrap();
        let (mut session, _) = host.start(request(&host.inventory), 3).unwrap();
        session.charge_active(Duration::ZERO).unwrap();
        for revision in 1..64 {
            let mut later = reply();
            later.revision = revision;
            shared.lock().unwrap().later = Some(later);
            session
                .try_task_pump(
                    TaskPump {
                        wake_token: revision,
                    },
                    revision - 1,
                )
                .unwrap();
            assert!(session.try_recv().unwrap().reply.is_ok());
            session.charge_active(Duration::ZERO).unwrap();
        }
        assert_eq!(shared.lock().unwrap().charges.len(), 64);
        assert_eq!(shared.lock().unwrap().pumps.len(), 63);
        assert!(
            session
                .try_task_pump(TaskPump { wake_token: 65 }, 63)
                .is_err()
        );
        assert_eq!(shared.lock().unwrap().pumps.len(), 63);
        assert_eq!(first_failure(&host), trace_failure(TraceFailure::Capacity));
    }
}
