//! Pure file disposition for the frozen cohort in docs/WPT_WINDOW_PARSING.md.
//!
//! The caller supplies only Browser's accepted ledger view, never a raw frame.
//! This is not a second ledger, an expectation updater, or an evidence verifier.
//! The owner retains raw records, chronological/secondary causes, requests and
//! cleanup receipts separately. Unattempted files remain PENDING in that owner.
use mg_sparkle::page_session::{
    LifecyclePhase, LifecycleProgress, RealmState,
    observer::{
        HarnessStatus, MAX_FIELD_UNITS, MAX_TESTS, NameReconciliation, ObservationView,
        ObserverExpectation, ObserverFault, ObserverParentFailure, ObserverProgress,
        ObserverRecord, SubtestStatus,
    },
};
use serde::Serialize;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Pass,
    Fail,
    Timeout,
    PreconditionFailed,
    Unsupported,
    Error,
    Crash,
}

/// Typed layers, not diagnostic-string classifications.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum Infrastructure {
    Inventory,
    Authority,
    Configuration,
    Projection,
    ActiveCharge,
    Wire,
    Launch,
    Artifact,
    Transport,
    ObservationIntegrity,
    MissingObservation,
    DeliveryProof,
    InitialOutcome,
    IncompleteEvidence,
    ContradictoryFacts,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum Uncertainty {
    Cleanup,
    Interrupted,
    Superseded,
}

/// Only an unimplemented source protocol rejected before any JavaScript.
/// Missing CSSOM/grammar after execution is deliberately not representable.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum Prerequisite {
    ModuleScript,
    NonWindowGlobal,
    CrossWindowProtocol,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum FailureCause {
    ResourceLimit,
    ObservationLimit,
    ObservationLost,
    PolicyDenied,
    ResourceAdmission,
    NativeExecution,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum Basis {
    CleanCompletion,
    Assertions,
    DeclaredInventoryIncomplete,
    DeclaredInventoryMismatch,
    HarnessError,
    HarnessTimeout,
    PreconditionFailed,
    Failure(FailureCause),
    MissingCompletion,
    UnsettledCompletion,
    Unsupported(Prerequisite),
    Infrastructure(Infrastructure),
    Uncertainty(Uncertainty),
    WorkerSignal(u8),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum Stop {
    AcceptedCutoff,
    AdmissionDeadline,
    BeforeJavaScript,
    Terminal,
    Interrupted,
    Superseded,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum Cleanup {
    /// The host proves pool close/wait_idle and fixture seal/drain/join. Its
    /// live process cannot prove its own outer group/direct-leader termination:
    /// that separate owner proof is mandatory before adopting this decision.
    Confirmed,
    Unknown,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum Delivery {
    /// All four immutable GET bodies, MIME, provenance and attribution verified.
    CompleteFourGets,
    /// Fully attributed, sealed/drained negative prefix with a known reason for
    /// incompleteness; never invented deliveries. Unknown or partial fixture
    /// evidence is Unverified, not an audited negative prefix.
    AuditedPrefix,
    Unverified,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum InitialEvidence {
    Unknown,
    Pending,
    Clean,
    ScriptFailed,
    InfrastructureFailed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum NativeHistory {
    Unknown,
    Clean,
    Failed,
}

/// Independent proof of the actual event causing a transport error. A prior
/// failure alone does not explain a later opaque transport failure.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum TransportConsequence {
    None,
    ValidatedPolicyDenied,
    ValidatedResourceAdmission,
}
impl TransportConsequence {
    fn cause(self) -> Option<FailureCause> {
        match self {
            Self::None => None,
            Self::ValidatedPolicyDenied => Some(FailureCause::PolicyDenied),
            Self::ValidatedResourceAdmission => Some(FailureCause::ResourceAdmission),
        }
    }
}

/// Construct only from authoritative actual worker signal/ownership evidence
/// held by a surviving owner. Neither Transport nor an owner cleanup kill is
/// such proof. This scalar records the signal, not a guessed fatal subtype.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub struct WorkerSignal {
    pub signal: u8,
}

/// Bounded, data-only facts to be established by the future fixed owner.
/// No field may be derived from diagnostic/fatal strings or page-controlled data.
#[derive(Debug, Copy, Clone, Serialize)]
pub struct HostFacts {
    #[serde(serialize_with = "serialize_expectation")]
    pub expected: ObserverExpectation,
    /// Source/manifest/configuration/identity, clocks and cumulative accounting
    /// premises verified. This does not substitute for the separate GET proof.
    pub premises_verified: bool,
    pub infrastructure: Option<Infrastructure>,
    pub cleanup: Cleanup,
    pub stop: Stop,
    pub delivery: Delivery,
    pub lifecycle: Option<LifecycleProgress>,
    pub realm: Option<RealmState>,
    pub initial: InitialEvidence,
    /// Independent sticky history, including failures after initial load froze.
    pub native_history: NativeHistory,
    pub pending_work: Option<u16>,
    /// Earliest validated owner cause, with all secondary facts retained outside
    /// this mapper. For the SAME event choose a concrete cause before generic
    /// NativeExecution; simultaneous concrete causes use the frozen order:
    /// ResourceLimit, ObservationLimit, ObservationLost, PolicyDenied,
    /// ResourceAdmission. A later deadline must not replace this field.
    pub first_established_failure: Option<FailureCause>,
    pub transport_consequence: TransportConsequence,
    /// Proof that THIS initial Infrastructure failure is the same validated
    /// guard denial/admission event causing the initial parent Transport. A
    /// prior cause or later transport is insufficient. Preserve raw initial
    /// evidence unchanged; nonempty accepted publication contradicts this proof.
    pub initial_transport_consequence: TransportConsequence,
    pub worker_signal: Option<WorkerSignal>,
    pub unsupported_before_js: Option<Prerequisite>,
    /// Exact validated reportless RejectedInit exception, not broken config.
    pub rejected_init_validated: bool,
}

fn serialize_expectation<S: serde::Serializer>(
    value: &ObserverExpectation,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    #[serde(tag = "kind")]
    enum Expected {
        Disabled,
        WindowHarness { run_id: u64 },
    }
    match value {
        ObserverExpectation::Disabled => Expected::Disabled,
        ObserverExpectation::WindowHarness { run_id } => {
            Expected::WindowHarness { run_id: *run_id }
        }
    }
    .serialize(serializer)
}

/// Exact source-derived creation order. Not runtime Result arrival order and not
/// a set: duplicate/colliding names must not collapse distinct native identities.
pub struct DeclaredTests<'a> {
    pub count: usize,
    pub names: &'a [String],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub enum InventoryMatch {
    Unknown,
    Exact,
    Incomplete { actual: usize, expected: usize },
    Mismatch { first_index: usize },
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub total: usize,
    pub pass: usize,
    pub fail: usize,
    pub timeout: usize,
    pub not_run: usize,
    pub precondition_failed: usize,
    pub final_only: usize,
}

/// Indices refer to the same accepted view retained by the owner. Counts are of
/// the final inventory ONCE; None means no Completion, not zero unseen tests.
/// Still requires the independent outer owner's process-cleanup/adoption gate.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub verdict: Verdict,
    pub basis: Basis,
    pub completion_record: Option<usize>,
    pub observed_results: usize,
    pub final_counts: Option<Counts>,
    pub inventory: InventoryMatch,
}

impl Decision {
    fn with(mut self, verdict: Verdict, basis: Basis) -> Self {
        self.verdict = verdict;
        self.basis = basis;
        self
    }
    fn error(self, layer: Infrastructure) -> Self {
        self.with(Verdict::Error, Basis::Infrastructure(layer))
    }
    fn failure(self, cause: FailureCause) -> Self {
        self.with(Verdict::Fail, Basis::Failure(cause))
    }
}

fn exact_initial_consequence(accepted: Option<&ObservationView<'_>>, host: &HostFacts) -> bool {
    let Some(cause) = host.initial_transport_consequence.cause() else {
        return false;
    };
    host.initial == InitialEvidence::InfrastructureFailed
        && host.stop == Stop::Terminal
        && host.transport_consequence == host.initial_transport_consequence
        && host.first_established_failure == Some(cause)
        && host.realm.is_none()
        && host.pending_work.is_none()
        && host
            .lifecycle
            .is_none_or(|value| value.phase == LifecyclePhase::Initializing)
        && !host.rejected_init_validated
        && host.unsupported_before_js.is_none()
        && accepted.is_some_and(|view| {
            view.expectation == host.expected
                && view.parent_failure == Some(ObserverParentFailure::Transport)
                && view.cursor == 0
                && view.progress.is_none()
                && view.fault.is_none()
                && !view.execution_failed
                && view.records.is_empty()
                && view.reconciliations.is_empty()
                && view.copied_units == 0
        })
}

/// No I/O, allocation, string coercion, staging, or mutation. All raw statuses,
/// text/reconciliation and secondary evidence remain in the caller's ledger.
pub fn classify(
    accepted: Option<ObservationView<'_>>,
    declared: DeclaredTests<'_>,
    host: &HostFacts,
) -> Decision {
    let mut decision = Decision {
        verdict: Verdict::Error,
        basis: Basis::Infrastructure(Infrastructure::MissingObservation),
        completion_record: None,
        observed_results: 0,
        final_counts: None,
        inventory: InventoryMatch::Unknown,
    };
    let declared_valid = declared.count != 0
        && declared.count <= MAX_TESTS
        && declared.names.len() == declared.count
        && declared.names.iter().all(|name| {
            !name.is_empty()
                && name.encode_utf16().take(MAX_FIELD_UNITS + 1).count() <= MAX_FIELD_UNITS
        });
    // This is a shape check, not a second identity/reconciliation validator.
    let mut harness = None;
    let mut repeated_completion = false;
    if let Some(view) = &accepted {
        for (record_index, record) in view.records.iter().enumerate() {
            match record {
                ObserverRecord::Result { .. } => decision.observed_results += 1,
                ObserverRecord::Completion {
                    harness: raw,
                    inventory,
                } => {
                    repeated_completion |= decision.completion_record.is_some();
                    decision.completion_record = Some(record_index);
                    harness = Some(raw.status);
                    let mut counts = Counts {
                        total: inventory.len(),
                        ..Counts::default()
                    };
                    for row in inventory {
                        match row.status {
                            SubtestStatus::Pass => counts.pass += 1,
                            SubtestStatus::Fail => counts.fail += 1,
                            SubtestStatus::Timeout => counts.timeout += 1,
                            SubtestStatus::NotRun => counts.not_run += 1,
                            SubtestStatus::PreconditionFailed => counts.precondition_failed += 1,
                        }
                    }
                    counts.final_only = view
                        .reconciliations
                        .iter()
                        .filter(|link| link.kind == NameReconciliation::FinalOnly)
                        .count();
                    decision.final_counts = Some(counts);
                    if declared_valid {
                        let name_difference =
                            inventory
                                .iter()
                                .zip(declared.names)
                                .position(|(row, expected)| {
                                    !row.name
                                        .as_units()
                                        .iter()
                                        .copied()
                                        .eq(expected.encode_utf16())
                                });
                        decision.inventory = if let Some(first_index) = name_difference {
                            InventoryMatch::Mismatch { first_index }
                        } else if inventory.len() < declared.count {
                            InventoryMatch::Incomplete {
                                actual: inventory.len(),
                                expected: declared.count,
                            }
                        } else if inventory.len() > declared.count {
                            InventoryMatch::Mismatch {
                                first_index: declared.count,
                            }
                        } else {
                            InventoryMatch::Exact
                        };
                    }
                }
            }
        }
    }

    // Uncertainty and integrity are never excused by a real semantic failure.
    if host.cleanup == Cleanup::Unknown {
        return decision.with(Verdict::Error, Basis::Uncertainty(Uncertainty::Cleanup));
    }
    match host.stop {
        Stop::Interrupted => {
            return decision.with(Verdict::Error, Basis::Uncertainty(Uncertainty::Interrupted));
        }
        Stop::Superseded => {
            return decision.with(Verdict::Error, Basis::Uncertainty(Uncertainty::Superseded));
        }
        _ => {}
    }
    if let Some(layer) = host.infrastructure {
        return decision.error(layer);
    }
    if !declared_valid
        || !matches!(host.expected, ObserverExpectation::WindowHarness { run_id } if run_id != 0)
    {
        return decision.error(Infrastructure::Configuration);
    }
    if !host.premises_verified {
        return decision.error(Infrastructure::Authority);
    }
    if repeated_completion {
        return decision.error(Infrastructure::ObservationIntegrity);
    }
    if host.pending_work.is_some_and(|count| count > 256)
        || (host.realm.is_some_and(|state| state != RealmState::Ready)
            && host.pending_work.is_some_and(|count| count != 0))
        || host
            .worker_signal
            .is_some_and(|proof| !(1..=64).contains(&proof.signal))
    {
        return decision.error(Infrastructure::ContradictoryFacts);
    }
    if host.initial_transport_consequence != TransportConsequence::None {
        if !exact_initial_consequence(accepted.as_ref(), host) {
            return decision.error(Infrastructure::ContradictoryFacts);
        }
    } else if host.initial == InitialEvidence::InfrastructureFailed {
        return decision.error(Infrastructure::InitialOutcome);
    }
    if host.stop == Stop::BeforeJavaScript
        && (host.initial == InitialEvidence::Clean
            || host
                .lifecycle
                .is_some_and(|progress| progress.phase != LifecyclePhase::Initializing)
            || host.pending_work.is_some_and(|count| count != 0)
            || accepted
                .as_ref()
                .is_some_and(|view| view.cursor != 0 || view.progress.is_some()))
    {
        return decision.error(Infrastructure::ContradictoryFacts);
    }
    if host.rejected_init_validated
        && accepted
            .as_ref()
            .is_none_or(|view| view.fault != Some(ObserverFault::RejectedInit))
    {
        return decision.error(Infrastructure::ContradictoryFacts);
    }
    let concrete = host.first_established_failure;
    let transport_cause = host.transport_consequence.cause();
    let policy = concrete == Some(FailureCause::PolicyDenied)
        || transport_cause == Some(FailureCause::PolicyDenied);
    let resource_admission = concrete == Some(FailureCause::ResourceAdmission)
        || transport_cause == Some(FailureCause::ResourceAdmission);
    let mut observed_cause = None;
    if let Some(view) = &accepted {
        if view.expectation != host.expected {
            return decision.error(Infrastructure::Authority);
        }
        match view.parent_failure {
            Some(ObserverParentFailure::Validation) => {
                return decision.error(Infrastructure::ObservationIntegrity);
            }
            Some(ObserverParentFailure::Projection) => {
                return decision.error(Infrastructure::Projection);
            }
            Some(ObserverParentFailure::ActiveCharge) => {
                return decision.error(Infrastructure::ActiveCharge);
            }
            Some(ObserverParentFailure::Cleanup) => {
                return decision.with(Verdict::Error, Basis::Uncertainty(Uncertainty::Cleanup));
            }
            Some(ObserverParentFailure::Transport)
                if transport_cause.is_none() && host.worker_signal.is_none() =>
            {
                return decision.error(Infrastructure::Transport);
            }
            Some(ObserverParentFailure::BrokerPolicy) if !policy && !resource_admission => {
                return decision.error(Infrastructure::Authority);
            }
            Some(ObserverParentFailure::Deadline) if host.stop != Stop::AdmissionDeadline => {
                return decision.error(Infrastructure::ContradictoryFacts);
            }
            _ => {}
        }
        match view.fault {
            Some(ObserverFault::InvalidConfiguration) => {
                return decision.error(Infrastructure::Configuration);
            }
            Some(ObserverFault::AuthorityUnavailable) => {
                return decision.error(Infrastructure::Authority);
            }
            Some(
                ObserverFault::MalformedCapture
                | ObserverFault::DuplicateResult
                | ObserverFault::DuplicateCompletion
                | ObserverFault::ResultAfterCompletion
                | ObserverFault::InvariantFailure,
            ) => return decision.error(Infrastructure::ObservationIntegrity),
            Some(ObserverFault::NavigationProposed) if !policy => {
                return decision.error(Infrastructure::Authority);
            }
            Some(ObserverFault::RejectedInit) => {
                if !host.rejected_init_validated
                    || host.stop != Stop::BeforeJavaScript
                    || view.cursor != 0
                    || !view.records.is_empty()
                    || !view.execution_failed
                {
                    return decision.error(Infrastructure::Configuration);
                }
                observed_cause = Some(FailureCause::NativeExecution);
            }
            Some(ObserverFault::CaptureOverflow) => {
                observed_cause = Some(FailureCause::ObservationLimit)
            }
            Some(ObserverFault::PrivacyLost) => {
                observed_cause = Some(FailureCause::ObservationLost)
            }
            Some(ObserverFault::NavigationProposed) => {
                observed_cause = Some(FailureCause::PolicyDenied)
            }
            // Accepted StorageAdmission/RealmTerminated are not proof of a
            // resource subtype. Their sticky native failure still counts.
            _ => {}
        }
        if observed_cause.is_none() && view.execution_failed {
            observed_cause = Some(FailureCause::NativeExecution);
        }
    }
    if host.unsupported_before_js.is_some()
        && (host.stop != Stop::BeforeJavaScript
            || concrete.is_some()
            || observed_cause.is_some()
            || transport_cause.is_some()
            || host.worker_signal.is_some()
            || host.native_history == NativeHistory::Failed
            || host.initial == InitialEvidence::ScriptFailed
            || accepted
                .as_ref()
                .is_some_and(|view| view.cursor != 0 || view.progress.is_some()))
    {
        return decision.error(Infrastructure::ContradictoryFacts);
    }
    if host.delivery == Delivery::Unverified {
        return decision.error(Infrastructure::DeliveryProof);
    }
    // Signal proof is explicit, never inferred from Transport or a string.
    if let Some(proof) = host.worker_signal {
        return decision.with(Verdict::Crash, Basis::WorkerSignal(proof.signal));
    }
    if let Some(prerequisite) = host.unsupported_before_js {
        return decision.with(Verdict::Unsupported, Basis::Unsupported(prerequisite));
    }
    if accepted.is_none()
        && !(host.stop == Stop::BeforeJavaScript
            && (concrete.is_some() || transport_cause.is_some()))
    {
        return decision.error(Infrastructure::MissingObservation);
    }
    if let Some(cause) = concrete.or(observed_cause).or(transport_cause) {
        return decision.failure(cause);
    }
    if host.native_history == NativeHistory::Failed
        || host.initial == InitialEvidence::ScriptFailed
        || host
            .lifecycle
            .is_some_and(|progress| progress.initial_failure)
    {
        return decision.failure(FailureCause::NativeExecution);
    }
    let Some(view) = accepted else {
        return decision.error(Infrastructure::MissingObservation);
    };
    if host.realm.is_some_and(|state| state != RealmState::Ready) || view.fault.is_some() {
        return decision.error(Infrastructure::IncompleteEvidence);
    }
    if view.progress != Some(ObserverProgress::Finished) {
        // Only healthy, audited normal waiting at the actual admission cutoff
        // is a semantic timeout. A killed host is not MissingCompletion.
        if host.stop != Stop::AdmissionDeadline
            || host.realm != Some(RealmState::Ready)
            || host.native_history != NativeHistory::Clean
        {
            return decision.error(Infrastructure::IncompleteEvidence);
        }
        return match view.progress {
            Some(ObserverProgress::Waiting) => {
                decision.with(Verdict::Timeout, Basis::MissingCompletion)
            }
            Some(ObserverProgress::Settling) => {
                decision.with(Verdict::Timeout, Basis::UnsettledCompletion)
            }
            _ => decision.error(Infrastructure::MissingObservation),
        };
    }
    if host.stop != Stop::AcceptedCutoff {
        return decision.error(Infrastructure::ContradictoryFacts);
    }
    let (Some(raw), Some(counts)) = (harness, decision.final_counts) else {
        return decision.error(Infrastructure::ObservationIntegrity);
    };
    match raw {
        HarnessStatus::Error => return decision.with(Verdict::Fail, Basis::HarnessError),
        HarnessStatus::Timeout => return decision.with(Verdict::Timeout, Basis::HarnessTimeout),
        _ => {}
    }
    if counts.fail + counts.timeout + counts.not_run != 0 {
        return decision.with(Verdict::Fail, Basis::Assertions);
    }
    if raw == HarnessStatus::PreconditionFailed {
        return decision.with(Verdict::PreconditionFailed, Basis::PreconditionFailed);
    }
    match decision.inventory {
        InventoryMatch::Incomplete { .. } => {
            return decision.with(Verdict::Fail, Basis::DeclaredInventoryIncomplete);
        }
        InventoryMatch::Mismatch { .. } => {
            return decision.with(Verdict::Fail, Basis::DeclaredInventoryMismatch);
        }
        InventoryMatch::Unknown => return decision.error(Infrastructure::ObservationIntegrity),
        InventoryMatch::Exact => {}
    }
    if counts.precondition_failed != 0 {
        return decision.with(Verdict::PreconditionFailed, Basis::PreconditionFailed);
    }
    if host.delivery != Delivery::CompleteFourGets
        || host.realm != Some(RealmState::Ready)
        || host.lifecycle
            != Some(LifecycleProgress {
                phase: LifecyclePhase::LoadComplete,
                initial_failure: false,
            })
        || host.initial != InitialEvidence::Clean
        || host.native_history != NativeHistory::Clean
        || host.pending_work.is_none()
    {
        return decision.error(Infrastructure::IncompleteEvidence);
    }
    decision.with(Verdict::Pass, Basis::CleanCompletion)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mg_sparkle::page_session::observer::{
        HarnessRecord, ObserverLedger, ObserverReply, OptionalUtf16, TestRecord, Utf16,
    };

    const EXPECTED: ObserverExpectation = ObserverExpectation::WindowHarness { run_id: 7 };

    fn facts() -> HostFacts {
        HostFacts {
            expected: EXPECTED,
            premises_verified: true,
            infrastructure: None,
            cleanup: Cleanup::Confirmed,
            stop: Stop::AcceptedCutoff,
            delivery: Delivery::CompleteFourGets,
            lifecycle: Some(LifecycleProgress {
                phase: LifecyclePhase::LoadComplete,
                initial_failure: false,
            }),
            realm: Some(RealmState::Ready),
            initial: InitialEvidence::Clean,
            native_history: NativeHistory::Clean,
            pending_work: Some(1), // The normal harness timer may remain.
            first_established_failure: None,
            transport_consequence: TransportConsequence::None,
            initial_transport_consequence: TransportConsequence::None,
            worker_signal: None,
            unsupported_before_js: None,
            rejected_init_validated: false,
        }
    }

    fn before_js() -> HostFacts {
        let mut host = facts();
        host.stop = Stop::BeforeJavaScript;
        host.delivery = Delivery::AuditedPrefix;
        host.lifecycle = None;
        host.realm = None;
        host.initial = InitialEvidence::Unknown;
        host.pending_work = None;
        host
    }

    fn row(id: u16, index: u16, name: &str, status: SubtestStatus) -> TestRecord {
        TestRecord {
            id,
            index,
            name: Utf16::from_units(name.encode_utf16().collect()).unwrap(),
            status,
            message: OptionalUtf16::Null,
            stack: OptionalUtf16::Undefined,
        }
    }

    fn result(test: TestRecord) -> ObserverRecord {
        ObserverRecord::Result { test }
    }

    fn completion(status: HarnessStatus, inventory: Vec<TestRecord>) -> ObserverRecord {
        ObserverRecord::Completion {
            harness: HarnessRecord {
                status,
                message: OptionalUtf16::Null,
                stack: OptionalUtf16::Undefined,
            },
            inventory,
        }
    }

    fn append(
        ledger: &mut ObserverLedger,
        records: Vec<ObserverRecord>,
        progress: ObserverProgress,
        failed: bool,
    ) {
        let first_ordinal = ledger.observation().cursor;
        let next_ordinal = first_ordinal + records.len() as u32;
        ledger
            .stage(ObserverReply::WindowHarness {
                run_id: 7,
                first_ordinal,
                next_ordinal,
                records,
                progress,
                execution_failed: failed,
            })
            .unwrap()
            .commit();
    }

    fn finished(
        status: HarnessStatus,
        names: &[&str],
        statuses: &[SubtestStatus],
    ) -> ObserverLedger {
        assert_eq!(names.len(), statuses.len());
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        let mut records = Vec::with_capacity(names.len() + 1);
        for (index, (name, status)) in names.iter().zip(statuses).enumerate() {
            records.push(result(row(index as u16 + 1, index as u16, name, *status)));
        }
        let inventory = names
            .iter()
            .zip(statuses)
            .enumerate()
            .map(|(index, (name, status))| row(index as u16 + 1, index as u16, name, *status))
            .collect();
        records.push(completion(status, inventory));
        append(&mut ledger, records, ObserverProgress::Finished, false);
        ledger
    }

    fn decide(ledger: Option<&ObserverLedger>, names: &[&str], host: &HostFacts) -> Decision {
        let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
        classify(
            ledger.map(ObserverLedger::observation),
            DeclaredTests {
                count: names.len(),
                names: &names,
            },
            host,
        )
    }

    fn fault(fault: ObserverFault) -> ObserverLedger {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        ledger
            .stage(ObserverReply::ObservationFailed {
                run_id: 7,
                prefix_end: 0,
                fault,
                execution_failed: true,
            })
            .unwrap()
            .commit();
        ledger
    }

    #[test]
    fn pass_counts_final_inventory_once_and_allows_retained_timer() {
        let ledger = finished(HarnessStatus::Ok, &["a", "b"], &[SubtestStatus::Pass; 2]);
        let verdict = decide(Some(&ledger), &["a", "b"], &facts());
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (Verdict::Pass, Basis::CleanCompletion)
        );
        assert_eq!(verdict.observed_results, 2);
        assert_eq!(verdict.completion_record, Some(2));
        assert_eq!(
            verdict.final_counts.unwrap(),
            Counts {
                total: 2,
                pass: 2,
                ..Counts::default()
            }
        );
        assert!(
            matches!(&ledger.observation().records[0], ObserverRecord::Result { test } if test.stack == OptionalUtf16::Undefined)
        );
    }

    #[test]
    fn each_nonpass_row_and_optional_feature_remains_nonpass() {
        for status in [
            SubtestStatus::Fail,
            SubtestStatus::Timeout,
            SubtestStatus::NotRun,
            SubtestStatus::PreconditionFailed,
        ] {
            let ledger = finished(HarnessStatus::Ok, &["a"], &[status]);
            let verdict = decide(Some(&ledger), &["a"], &facts());
            let expected = if status == SubtestStatus::PreconditionFailed {
                (Verdict::PreconditionFailed, Basis::PreconditionFailed)
            } else {
                (Verdict::Fail, Basis::Assertions)
            };
            assert_eq!((verdict.verdict, verdict.basis), expected);
            assert_eq!(verdict.final_counts.unwrap().total, 1);
        }
    }

    #[test]
    fn zero_missing_extra_and_wrong_names_are_semantic_inventory_failures() {
        for (actual, declared, basis) in [
            (vec![], vec!["a"], Basis::DeclaredInventoryIncomplete),
            (
                vec!["a"],
                vec!["a", "b"],
                Basis::DeclaredInventoryIncomplete,
            ),
            (vec!["a", "b"], vec!["a"], Basis::DeclaredInventoryMismatch),
            (
                vec!["b", "a"],
                vec!["a", "b"],
                Basis::DeclaredInventoryMismatch,
            ),
        ] {
            let ledger = finished(
                HarnessStatus::Ok,
                &actual,
                &vec![SubtestStatus::Pass; actual.len()],
            );
            let verdict = decide(Some(&ledger), &declared, &facts());
            assert_eq!((verdict.verdict, verdict.basis), (Verdict::Fail, basis));
            assert_eq!(verdict.final_counts.unwrap().total, actual.len());
        }
    }

    #[test]
    fn negative_harness_statuses_and_final_only_rows_are_genuine_not_synthesized() {
        for harness in [
            HarnessStatus::Error,
            HarnessStatus::Timeout,
            HarnessStatus::PreconditionFailed,
        ] {
            let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
            let statuses = [
                SubtestStatus::Pass,
                SubtestStatus::Fail,
                SubtestStatus::Timeout,
                SubtestStatus::NotRun,
                SubtestStatus::PreconditionFailed,
            ];
            let inventory = statuses
                .iter()
                .enumerate()
                .map(|(i, status)| row(i as u16 + 1, i as u16, "same", *status))
                .collect();
            append(
                &mut ledger,
                vec![completion(harness, inventory)],
                ObserverProgress::Finished,
                false,
            );
            let verdict = decide(Some(&ledger), &["same"; 5], &facts());
            let expected = match harness {
                HarnessStatus::Error => (Verdict::Fail, Basis::HarnessError),
                HarnessStatus::Timeout => (Verdict::Timeout, Basis::HarnessTimeout),
                _ => (Verdict::Fail, Basis::Assertions),
            };
            assert_eq!((verdict.verdict, verdict.basis), expected);
            assert_eq!(verdict.observed_results, 0);
            assert_eq!(
                verdict.final_counts.unwrap(),
                Counts {
                    total: 5,
                    pass: 1,
                    fail: 1,
                    timeout: 1,
                    not_run: 1,
                    precondition_failed: 1,
                    final_only: 5
                }
            );
        }
    }

    #[test]
    fn harness_precondition_without_stronger_failure_is_not_pass_or_unsupported() {
        let ledger = finished(
            HarnessStatus::PreconditionFailed,
            &["a"],
            &[SubtestStatus::Pass],
        );
        let verdict = decide(Some(&ledger), &["a"], &facts());
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (Verdict::PreconditionFailed, Basis::PreconditionFailed)
        );
    }

    #[test]
    fn ok_inventory_failure_is_stronger_than_optional_row_but_not_global_precondition() {
        let ledger = finished(
            HarnessStatus::Ok,
            &["a"],
            &[SubtestStatus::PreconditionFailed],
        );
        let verdict = decide(Some(&ledger), &["a", "b"], &facts());
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (Verdict::Fail, Basis::DeclaredInventoryIncomplete)
        );
        assert_eq!(verdict.final_counts.unwrap().precondition_failed, 1);
        let global = finished(HarnessStatus::PreconditionFailed, &[], &[]);
        let verdict = decide(Some(&global), &["a"], &facts());
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (Verdict::PreconditionFailed, Basis::PreconditionFailed)
        );
        assert_eq!(verdict.final_counts.unwrap().total, 0);
        assert_eq!(
            verdict.inventory,
            InventoryMatch::Incomplete {
                actual: 0,
                expected: 1
            }
        );
    }

    #[test]
    fn final_creation_order_is_not_result_arrival_order_or_native_id_order() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        append(
            &mut ledger,
            vec![
                result(row(1, 1, "b", SubtestStatus::Pass)),
                result(row(2, 0, "a", SubtestStatus::Pass)),
                completion(
                    HarnessStatus::Ok,
                    vec![
                        row(2, 0, "a", SubtestStatus::Pass),
                        row(1, 1, "b", SubtestStatus::Pass),
                    ],
                ),
            ],
            ObserverProgress::Finished,
            false,
        );
        let verdict = decide(Some(&ledger), &["a", "b"], &facts());
        assert_eq!(verdict.verdict, Verdict::Pass);
        assert_eq!(verdict.final_counts.unwrap().total, 2);
    }

    #[test]
    fn pinned_sanitizer_collision_does_not_deduplicate_native_rows() {
        let mut raw = row(1, 0, "unused", SubtestStatus::Pass);
        raw.name = Utf16::from_units(vec![0xd800]).unwrap();
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        append(
            &mut ledger,
            vec![
                result(raw),
                result(row(2, 1, "U+d800", SubtestStatus::Pass)),
                completion(
                    HarnessStatus::Ok,
                    vec![
                        row(1, 0, "U+d800", SubtestStatus::Pass),
                        row(2, 1, "U+d800", SubtestStatus::Pass),
                    ],
                ),
            ],
            ObserverProgress::Finished,
            false,
        );
        let verdict = decide(Some(&ledger), &["U+d800", "U+d800"], &facts());
        assert_eq!(verdict.verdict, Verdict::Pass);
        assert_eq!(verdict.final_counts.unwrap().total, 2);
        assert_eq!(
            ledger.observation().reconciliations[0].kind,
            NameReconciliation::PinnedSanitized
        );
    }

    #[test]
    fn partial_prefix_has_unknown_final_total_and_distinct_missing_timeout() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        append(
            &mut ledger,
            vec![result(row(1, 0, "a", SubtestStatus::Pass))],
            ObserverProgress::Waiting,
            false,
        );
        let mut host = facts();
        host.stop = Stop::AdmissionDeadline;
        ledger.fail_parent(ObserverParentFailure::Deadline);
        let verdict = decide(Some(&ledger), &["a", "b"], &host);
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (Verdict::Timeout, Basis::MissingCompletion)
        );
        assert_eq!(verdict.observed_results, 1);
        assert_eq!(verdict.final_counts, None);
        assert_eq!(verdict.inventory, InventoryMatch::Unknown);
    }

    #[test]
    fn provisional_completion_is_unsettled_not_harness_timeout_or_pass() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        append(
            &mut ledger,
            vec![
                result(row(1, 0, "a", SubtestStatus::Pass)),
                completion(HarnessStatus::Ok, vec![row(1, 0, "a", SubtestStatus::Pass)]),
            ],
            ObserverProgress::Settling,
            false,
        );
        let mut host = facts();
        host.stop = Stop::AdmissionDeadline;
        let verdict = decide(Some(&ledger), &["a"], &host);
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (Verdict::Timeout, Basis::UnsettledCompletion)
        );
        assert_eq!(verdict.final_counts.unwrap().pass, 1);
    }

    #[test]
    fn native_failure_is_sticky_even_with_clean_harness_and_after_load() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        append(
            &mut ledger,
            vec![
                result(row(1, 0, "a", SubtestStatus::Pass)),
                completion(HarnessStatus::Ok, vec![row(1, 0, "a", SubtestStatus::Pass)]),
            ],
            ObserverProgress::Finished,
            true,
        );
        assert_eq!(
            decide(Some(&ledger), &["a"], &facts()).basis,
            Basis::Failure(FailureCause::NativeExecution)
        );
        let clean = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Pass]);
        let mut host = facts();
        host.native_history = NativeHistory::Failed;
        assert_eq!(
            decide(Some(&clean), &["a"], &host).basis,
            Basis::Failure(FailureCause::NativeExecution)
        );
    }

    #[test]
    fn accepted_terminal_faults_do_not_guess_resource_subtype() {
        for raw in [
            ObserverFault::StorageAdmission,
            ObserverFault::RealmTerminated,
        ] {
            let ledger = fault(raw);
            let mut host = facts();
            host.stop = Stop::Terminal;
            host.realm = Some(RealmState::Fatal);
            host.pending_work = Some(0);
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Failure(FailureCause::NativeExecution)
            );
            host.first_established_failure = Some(FailureCause::ResourceLimit);
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Failure(FailureCause::ResourceLimit)
            );
        }
    }

    #[test]
    fn observation_and_validated_policy_faults_remain_failures_before_deadline() {
        for (raw, cause) in [
            (
                ObserverFault::CaptureOverflow,
                FailureCause::ObservationLimit,
            ),
            (ObserverFault::PrivacyLost, FailureCause::ObservationLost),
            (
                ObserverFault::NavigationProposed,
                FailureCause::PolicyDenied,
            ),
        ] {
            let mut ledger = fault(raw);
            let mut host = facts();
            host.stop = Stop::AdmissionDeadline;
            if raw == ObserverFault::NavigationProposed {
                host.first_established_failure = Some(cause);
            }
            ledger.fail_parent(ObserverParentFailure::Deadline);
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Failure(cause)
            );
        }
    }

    #[test]
    fn earliest_typed_owner_cause_wins_and_is_not_reconstructed_from_flags() {
        let ledger = fault(ObserverFault::PrivacyLost);
        for cause in [
            FailureCause::ResourceLimit,
            FailureCause::ObservationLimit,
            FailureCause::ObservationLost,
            FailureCause::PolicyDenied,
            FailureCause::ResourceAdmission,
            FailureCause::NativeExecution,
        ] {
            let mut host = facts();
            host.stop = Stop::AdmissionDeadline;
            host.first_established_failure = Some(cause);
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Failure(cause)
            );
        }
    }

    #[test]
    fn malformed_faults_are_integrity_errors_even_with_known_native_failure() {
        for raw in [
            ObserverFault::InvalidConfiguration,
            ObserverFault::MalformedCapture,
            ObserverFault::DuplicateResult,
            ObserverFault::DuplicateCompletion,
            ObserverFault::ResultAfterCompletion,
            ObserverFault::AuthorityUnavailable,
            ObserverFault::InvariantFailure,
            ObserverFault::NavigationProposed,
        ] {
            let ledger = fault(raw);
            let mut host = facts();
            host.first_established_failure = Some(FailureCause::NativeExecution);
            assert_eq!(decide(Some(&ledger), &["a"], &host).verdict, Verdict::Error);
        }
    }

    #[test]
    fn malformed_staging_is_rejected_not_presented_as_accepted_result() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        append(
            &mut ledger,
            vec![result(row(1, 0, "a", SubtestStatus::Pass))],
            ObserverProgress::Waiting,
            false,
        );
        assert!(
            ledger
                .stage(ObserverReply::WindowHarness {
                    run_id: 7,
                    first_ordinal: 1,
                    next_ordinal: 2,
                    records: vec![result(row(1, 0, "a", SubtestStatus::Pass))],
                    progress: ObserverProgress::Waiting,
                    execution_failed: false
                })
                .is_err()
        );
        assert_eq!(ledger.observation().records.len(), 1);
        let verdict = decide(Some(&ledger), &["a"], &facts());
        assert_eq!(
            verdict.basis,
            Basis::Infrastructure(Infrastructure::ObservationIntegrity)
        );
        assert_eq!(verdict.final_counts, None);
    }

    #[test]
    fn parent_transport_is_opaque_unless_owner_has_authoritative_cause() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        ledger.fail_parent(ObserverParentFailure::Transport);
        assert_eq!(
            decide(Some(&ledger), &["a"], &facts()).basis,
            Basis::Infrastructure(Infrastructure::Transport)
        );
        let mut host = facts();
        host.stop = Stop::Terminal;
        host.delivery = Delivery::AuditedPrefix;
        for earlier in [
            FailureCause::NativeExecution,
            FailureCause::ResourceLimit,
            FailureCause::PolicyDenied,
            FailureCause::ResourceAdmission,
        ] {
            host.first_established_failure = Some(earlier);
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Infrastructure(Infrastructure::Transport)
            );
        }
        host.first_established_failure = Some(FailureCause::PolicyDenied);
        host.transport_consequence = TransportConsequence::ValidatedPolicyDenied;
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Failure(FailureCause::PolicyDenied)
        );
        host.first_established_failure = Some(FailureCause::ResourceAdmission);
        host.transport_consequence = TransportConsequence::ValidatedResourceAdmission;
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Failure(FailureCause::ResourceAdmission)
        );
        host.first_established_failure = Some(FailureCause::NativeExecution);
        host.transport_consequence = TransportConsequence::ValidatedPolicyDenied;
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Failure(FailureCause::NativeExecution)
        );
    }

    fn initial_consequence(consequence: TransportConsequence) -> HostFacts {
        let mut host = facts();
        host.stop = Stop::Terminal;
        host.delivery = Delivery::AuditedPrefix;
        host.lifecycle = Some(LifecycleProgress {
            phase: LifecyclePhase::Initializing,
            initial_failure: false,
        });
        host.initial = InitialEvidence::InfrastructureFailed;
        host.realm = None;
        host.pending_work = None;
        host.native_history = NativeHistory::Unknown;
        host.first_established_failure = consequence.cause();
        host.transport_consequence = consequence;
        host.initial_transport_consequence = consequence;
        host
    }

    #[test]
    fn initial_same_event_denial_preserves_raw_infrastructure_and_empty_ledger() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        ledger.fail_parent(ObserverParentFailure::Transport);
        for consequence in [
            TransportConsequence::ValidatedPolicyDenied,
            TransportConsequence::ValidatedResourceAdmission,
        ] {
            let mut host = initial_consequence(consequence);
            host.initial_transport_consequence = TransportConsequence::None;
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Infrastructure(Infrastructure::InitialOutcome)
            );
            host.initial_transport_consequence = consequence;
            let decision = decide(Some(&ledger), &["a"], &host);
            assert_eq!(
                (decision.verdict, decision.basis),
                (Verdict::Fail, Basis::Failure(consequence.cause().unwrap()))
            );
            assert_eq!(decision.observed_results, 0);
            assert_eq!(decision.final_counts, None);
            assert_eq!(host.initial, InitialEvidence::InfrastructureFailed);
            assert_eq!(
                ledger.observation().parent_failure,
                Some(ObserverParentFailure::Transport)
            );
            assert!(ledger.observation().records.is_empty());
        }
    }

    #[test]
    fn initial_consequence_requires_exact_matching_initial_event_not_prior_or_later_cause() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        ledger.fail_parent(ObserverParentFailure::Transport);
        let exact = initial_consequence(TransportConsequence::ValidatedPolicyDenied);
        let mut variants = [exact; 10];
        variants[0].initial = InitialEvidence::Clean;
        variants[1].stop = Stop::AdmissionDeadline;
        variants[2].transport_consequence = TransportConsequence::None;
        variants[3].first_established_failure = Some(FailureCause::NativeExecution);
        variants[4].realm = Some(RealmState::Ready);
        variants[5].pending_work = Some(0);
        variants[6].lifecycle.as_mut().unwrap().phase = LifecyclePhase::LoadComplete;
        variants[7].rejected_init_validated = true;
        variants[8].unsupported_before_js = Some(Prerequisite::ModuleScript);
        variants[9].initial_transport_consequence =
            TransportConsequence::ValidatedResourceAdmission;
        for host in variants {
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Infrastructure(Infrastructure::ContradictoryFacts)
            );
        }
    }

    #[test]
    fn initial_consequence_cannot_excuse_publication_faults_or_missing_authority() {
        let host = initial_consequence(TransportConsequence::ValidatedPolicyDenied);
        assert_eq!(decide(None, &["a"], &host).verdict, Verdict::Error);
        let mut waiting = ObserverLedger::new(EXPECTED).unwrap();
        append(&mut waiting, vec![], ObserverProgress::Waiting, false);
        waiting.fail_parent(ObserverParentFailure::Transport);
        let mut completed = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Pass]);
        completed.fail_parent(ObserverParentFailure::Transport);
        for ledger in [&waiting, &completed] {
            assert_eq!(decide(Some(ledger), &["a"], &host).verdict, Verdict::Error);
        }
        for failure in [
            ObserverParentFailure::Projection,
            ObserverParentFailure::ActiveCharge,
            ObserverParentFailure::Validation,
            ObserverParentFailure::Cleanup,
        ] {
            let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
            ledger.fail_parent(failure);
            assert_eq!(decide(Some(&ledger), &["a"], &host).verdict, Verdict::Error);
        }
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        ledger.fail_parent(ObserverParentFailure::Transport);
        let mut unknown = host;
        unknown.cleanup = Cleanup::Unknown;
        assert_eq!(
            decide(Some(&ledger), &["a"], &unknown).basis,
            Basis::Uncertainty(Uncertainty::Cleanup)
        );
        unknown = host;
        unknown.infrastructure = Some(Infrastructure::Projection);
        assert_eq!(
            decide(Some(&ledger), &["a"], &unknown).basis,
            Basis::Infrastructure(Infrastructure::Projection)
        );
    }

    #[test]
    fn serialized_dispositions_are_closed_and_do_not_reinterpret_decisions() {
        for (value, expected) in [
            (Verdict::Pass, "PASS"),
            (Verdict::Fail, "FAIL"),
            (Verdict::Timeout, "TIMEOUT"),
            (Verdict::PreconditionFailed, "PRECONDITION_FAILED"),
            (Verdict::Unsupported, "UNSUPPORTED"),
            (Verdict::Error, "ERROR"),
            (Verdict::Crash, "CRASH"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        let ledger = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Pass]);
        let decision = decide(Some(&ledger), &["a"], &facts());
        let serialized = serde_json::to_value(decision).unwrap();
        assert_eq!(serialized["verdict"], "PASS");
        assert_eq!(serialized["basis"], "CleanCompletion");
        assert_eq!(serialized["final_counts"]["pass"], 1);
        let serialized = serde_json::to_value(facts()).unwrap();
        assert_eq!(
            serialized["expected"],
            serde_json::json!({"kind":"WindowHarness","run_id":7})
        );
        assert_eq!(serialized["initial"], "Clean");
    }

    #[test]
    fn projection_charge_validation_and_cleanup_cannot_be_excused_by_policy() {
        for failure in [
            ObserverParentFailure::Validation,
            ObserverParentFailure::Projection,
            ObserverParentFailure::ActiveCharge,
            ObserverParentFailure::Cleanup,
        ] {
            let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
            ledger.fail_parent(failure);
            let mut host = facts();
            host.first_established_failure = Some(FailureCause::PolicyDenied);
            host.transport_consequence = TransportConsequence::ValidatedPolicyDenied;
            assert_eq!(decide(Some(&ledger), &["a"], &host).verdict, Verdict::Error);
        }
    }

    #[test]
    fn unknown_cleanup_interrupt_supersession_and_bad_inputs_prevent_adoption() {
        let ledger = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Pass]);
        for (cleanup, stop, basis) in [
            (
                Cleanup::Unknown,
                Stop::AcceptedCutoff,
                Basis::Uncertainty(Uncertainty::Cleanup),
            ),
            (
                Cleanup::Confirmed,
                Stop::Interrupted,
                Basis::Uncertainty(Uncertainty::Interrupted),
            ),
            (
                Cleanup::Confirmed,
                Stop::Superseded,
                Basis::Uncertainty(Uncertainty::Superseded),
            ),
        ] {
            let mut host = facts();
            host.cleanup = cleanup;
            host.stop = stop;
            assert_eq!(decide(Some(&ledger), &["a"], &host).basis, basis);
        }
        for layer in [
            Infrastructure::Inventory,
            Infrastructure::Authority,
            Infrastructure::Configuration,
            Infrastructure::Projection,
            Infrastructure::ActiveCharge,
            Infrastructure::Wire,
            Infrastructure::Launch,
            Infrastructure::Artifact,
        ] {
            let mut host = facts();
            host.infrastructure = Some(layer);
            host.first_established_failure = Some(FailureCause::ResourceLimit);
            assert_eq!(
                decide(Some(&ledger), &["a"], &host).basis,
                Basis::Infrastructure(layer)
            );
        }
    }

    #[test]
    fn signal_requires_explicit_proof_and_confirmed_cleanup() {
        let mut ledger = ObserverLedger::new(EXPECTED).unwrap();
        ledger.fail_parent(ObserverParentFailure::Transport);
        let mut host = facts();
        host.stop = Stop::Terminal;
        host.worker_signal = Some(WorkerSignal { signal: 11 });
        let verdict = decide(Some(&ledger), &["a"], &host);
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (Verdict::Crash, Basis::WorkerSignal(11))
        );
        host.cleanup = Cleanup::Unknown;
        assert_eq!(decide(Some(&ledger), &["a"], &host).verdict, Verdict::Error);
        host.cleanup = Cleanup::Confirmed;
        host.worker_signal = Some(WorkerSignal { signal: 0 });
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Infrastructure(Infrastructure::ContradictoryFacts)
        );
    }

    #[test]
    fn unsupported_is_only_pre_js_source_protocol_not_optional_or_runtime_api() {
        let mut host = before_js();
        host.unsupported_before_js = Some(Prerequisite::NonWindowGlobal);
        assert_eq!(
            decide(None, &["a"], &host).basis,
            Basis::Unsupported(Prerequisite::NonWindowGlobal)
        );
        let ledger = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Fail]);
        host.stop = Stop::AcceptedCutoff;
        assert_eq!(decide(Some(&ledger), &["a"], &host).verdict, Verdict::Error);
    }

    #[test]
    fn rejected_init_requires_exact_validated_pre_realm_exception() {
        let ledger = fault(ObserverFault::RejectedInit);
        let mut host = before_js();
        host.realm = Some(RealmState::Closed);
        host.pending_work = Some(0);
        assert_eq!(decide(Some(&ledger), &["a"], &host).verdict, Verdict::Error);
        host.rejected_init_validated = true;
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Failure(FailureCause::NativeExecution)
        );
    }

    #[test]
    fn missing_ledger_is_not_a_fabricated_empty_completion() {
        assert_eq!(
            decide(None, &["a"], &facts()).basis,
            Basis::Infrastructure(Infrastructure::MissingObservation)
        );
        let mut host = before_js();
        host.first_established_failure = Some(FailureCause::ResourceAdmission);
        assert_eq!(
            decide(None, &["a"], &host).basis,
            Basis::Failure(FailureCause::ResourceAdmission)
        );
    }

    #[test]
    fn pass_requires_complete_noncontradictory_owner_premises() {
        let ledger = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Pass]);
        let mut variants = [facts(); 13];
        variants[0].premises_verified = false;
        variants[1].delivery = Delivery::AuditedPrefix;
        variants[2].delivery = Delivery::Unverified;
        variants[3].lifecycle = None;
        variants[4].lifecycle = Some(LifecycleProgress {
            phase: LifecyclePhase::DomContentLoaded,
            initial_failure: false,
        });
        variants[5].realm = None;
        variants[6].initial = InitialEvidence::Unknown;
        variants[7].initial = InitialEvidence::Pending;
        variants[8].initial = InitialEvidence::InfrastructureFailed;
        variants[9].native_history = NativeHistory::Unknown;
        variants[10].pending_work = None;
        variants[11].pending_work = Some(257);
        variants[12].expected = ObserverExpectation::WindowHarness { run_id: 8 };
        for host in variants {
            assert_eq!(decide(Some(&ledger), &["a"], &host).verdict, Verdict::Error);
        }
        let mut host = facts();
        host.lifecycle.as_mut().unwrap().initial_failure = true;
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Failure(FailureCause::NativeExecution)
        );
        host = facts();
        host.initial = InitialEvidence::ScriptFailed;
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Failure(FailureCause::NativeExecution)
        );
    }

    #[test]
    fn source_count_shape_and_delivery_authority_are_not_browser_failures() {
        let ledger = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Pass]);
        let names = vec!["a".to_owned()];
        assert_eq!(
            classify(
                Some(ledger.observation()),
                DeclaredTests {
                    count: 2,
                    names: &names
                },
                &facts()
            )
            .basis,
            Basis::Infrastructure(Infrastructure::Configuration)
        );
        let mut host = facts();
        host.delivery = Delivery::Unverified;
        host.first_established_failure = Some(FailureCause::NativeExecution);
        assert_eq!(
            decide(Some(&ledger), &["a"], &host).basis,
            Basis::Infrastructure(Infrastructure::DeliveryProof)
        );
    }

    #[test]
    fn empty_source_declared_name_is_configuration_error() {
        let ledger = finished(HarnessStatus::Ok, &["a"], &[SubtestStatus::Pass]);
        let verdict = decide(Some(&ledger), &[""], &facts());
        assert_eq!(
            (verdict.verdict, verdict.basis),
            (
                Verdict::Error,
                Basis::Infrastructure(Infrastructure::Configuration)
            )
        );
    }

    #[test]
    fn pre_js_proof_cannot_coexist_with_completed_lifecycle_or_runtime_work() {
        let mut host = before_js();
        host.unsupported_before_js = Some(Prerequisite::ModuleScript);
        host.lifecycle = facts().lifecycle;
        assert_eq!(
            decide(None, &["a"], &host).basis,
            Basis::Infrastructure(Infrastructure::ContradictoryFacts)
        );
        host = before_js();
        host.rejected_init_validated = true;
        assert_eq!(
            decide(None, &["a"], &host).basis,
            Basis::Infrastructure(Infrastructure::ContradictoryFacts)
        );
    }
}
