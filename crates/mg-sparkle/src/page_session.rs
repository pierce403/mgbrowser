//! Typed, bounded interaction between the parent and one retained page realm.
//! Transport identities and authority are supplied by the parent, never script.
use crate::document::Node;
use mg_butane::runtime::AllocationReport;
use serde::{Deserialize, Serialize};

pub mod automation;
pub mod console;
pub mod observer;
pub mod platform;

pub use platform::{
    HistoryOp, MAX_COOKIE_WRITE_BYTES, MAX_COOKIE_WRITES_PER_TX, MAX_FETCH_REQUESTS_PER_TX,
    MAX_HISTORY_OPS_PER_TX, MAX_STORAGE_OPS_PER_TX, MAX_STORAGE_QUOTA_BYTES, PlatformCommand,
    PlatformInit, ScriptFetchRequest, ScriptFetchResponse, ScrollRequest, StorageOp, ViewportState,
};

pub const MAX_EDITS: usize = 128;
pub const MAX_EDIT_BYTES: usize = 8191;
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
pub const MAX_EVENTS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEdit {
    pub node: usize,
    pub version: u64,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAck {
    pub node: usize,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInput {
    pub kind: InputKind,
    pub edits: Vec<ControlEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum InputKind {
    Click {
        target: usize,
    },
    Submit {
        form: usize,
        submitter: Option<usize>,
    },
}

/// A parent-issued request to run one advertised, eligible task. No JS handles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskPump {
    pub wake_token: u64,
}

/// Relative monotonic wake hint. Receipt plus delay can be late, never early.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskWake {
    pub token: u64,
    pub delay_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum DefaultAction {
    None,
    FollowLink {
        node: usize,
    },
    SubmitForm {
        form: usize,
        submitter: Option<usize>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RealmState {
    Ready,
    Fatal,
    Closed,
}

/// Completed snapshot milestones, independent of mutable DOM readiness and
/// whether the retained realm can still execute work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LifecyclePhase {
    Initializing,
    Interactive,
    DomContentLoaded,
    LoadComplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleProgress {
    pub phase: LifecyclePhase,
    pub initial_failure: bool,
}

/// Bounded cumulative accounting only; never retains a document or JS value.
#[derive(Debug, Clone)]
pub struct AccountingSnapshot {
    allocations: Option<AllocationReport>,
    #[cfg(feature = "modern")]
    boa: Option<mg_butane::modern::Report>,
}

impl AccountingSnapshot {
    pub fn is_valid_after(&self, previous: &Self) -> bool {
        #[cfg(feature = "modern")]
        match (&self.boa, &previous.boa) {
            (Some(next), Some(old)) => {
                return self.allocations.is_none()
                    && previous.allocations.is_none()
                    && next.validate_after(old).is_ok();
            }
            (None, None) => {}
            _ => return false,
        }
        let (Some(old), Some(next)) = (previous.allocations, self.allocations) else {
            return false;
        };
        let values = |report: AllocationReport| {
            let phases = report.phases;
            [
                report.accepted_bytes,
                phases.bootstrap,
                phases.source,
                phases.ast,
                phases.function_code,
                phases.runtime,
                phases.regex_compile,
                phases.regex_result,
            ]
        };
        old.is_valid()
            && next.is_valid()
            && old.limit_bytes == 4 * 1024 * 1024
            && next.limit_bytes == old.limit_bytes
            && values(old)
                .iter()
                .zip(values(next))
                .all(|(old, new)| new >= *old)
            && old
                .first_rejected
                .is_none_or(|old| next.first_rejected == Some(old))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventOutcome {
    pub click_canceled: Option<bool>,
    pub submit_canceled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArenaSnapshot {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionReply {
    pub revision: u64,
    pub snapshot: Option<ArenaSnapshot>,
    pub outcome: EventOutcome,
    pub default_action: DefaultAction,
    pub navigation: Option<String>,
    pub errors: Vec<String>,
    pub scripts_executed: usize,
    pub allocations: Option<AllocationReport>,
    /// Explicit Boa profile, never relabeled as original interpreter allocations.
    #[cfg(feature = "modern")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boa: Option<mg_butane::modern::Report>,
    pub state: RealmState,
    pub acknowledgements: Vec<ControlAck>,
    /// Modern parent task scheduling only; absent in the legacy protocol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_wake: Option<TaskWake>,
    /// Required by modern v5, absent from the strict legacy v1 wire shape.
    /// The trusted transport must distinguish absence from an explicit null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<LifecycleProgress>,
    /// Trusted modern v6 observation. Legacy and one-shot lanes cannot opt in;
    /// the transport requires presence even when modern observation is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observer: Option<observer::ObserverReply>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub local_storage_ops: Vec<StorageOp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_storage_ops: Vec<StorageOp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cookie_writes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history_ops: Vec<HistoryOp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fetch_requests: Vec<ScriptFetchRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll_request: Option<ScrollRequest>,
}

impl SessionReply {
    /// Cross-check observation against its outer transaction before staging.
    /// This does not accept cumulative identities or publish a result: consumers
    /// must also use ObserverLedger and commit only after their active charge.
    pub fn validate_observer_frame(
        &self,
        expected: observer::ObserverExpectation,
    ) -> Result<(), observer::ObserverValidationError> {
        use observer::{
            ObserverExpectation as E, ObserverFault as F, ObserverProgress as P,
            ObserverReply as R, ObserverValidationError as V,
        };
        expected.validate()?;
        self.observer
            .as_ref()
            .ok_or(V::Mode)?
            .validate_expectation(expected)?;
        #[cfg(feature = "modern")]
        let fatal_engine = self
            .boa
            .as_ref()
            .is_some_and(|report| report.fatal_reason.is_some());
        #[cfg(not(feature = "modern"))]
        let fatal_engine = false;
        match (expected, self.observer.as_ref()) {
            (E::Disabled, Some(R::Disabled)) => Ok(()),
            (
                E::WindowHarness { run_id },
                Some(R::WindowHarness {
                    run_id: got,
                    progress,
                    ..
                }),
            ) if run_id == *got => {
                if self.is_rejected_initial()
                    || self.state != RealmState::Ready
                    || self.navigation.is_some()
                    || (*progress == P::Settling && self.task_wake.is_none())
                {
                    return Err(V::Frame);
                }
                Ok(())
            }
            (
                E::WindowHarness { run_id },
                Some(R::ObservationFailed {
                    run_id: got,
                    prefix_end,
                    fault,
                    execution_failed,
                }),
            ) if run_id == *got => {
                if self.is_rejected_initial() {
                    if *prefix_end != 0 || *fault != F::RejectedInit || !execution_failed {
                        return Err(V::Frame);
                    }
                } else if *fault == F::RejectedInit {
                    return Err(V::Frame);
                }
                if matches!(
                    fault,
                    F::StorageAdmission | F::AuthorityUnavailable | F::InvariantFailure
                ) && (self.state != RealmState::Fatal || !fatal_engine)
                {
                    return Err(V::Frame);
                }
                if *fault == F::RealmTerminated && self.state == RealmState::Ready {
                    return Err(V::Frame);
                }
                if (self.state != RealmState::Ready
                    || self.navigation.is_some()
                    || matches!(
                        fault,
                        F::PrivacyLost
                            | F::AuthorityUnavailable
                            | F::RealmTerminated
                            | F::NavigationProposed
                            | F::InvariantFailure
                    ))
                    && !execution_failed
                {
                    return Err(V::Frame);
                }
                Ok(())
            }
            _ => Err(V::Mode),
        }
    }

    pub fn accounting_snapshot(&self) -> AccountingSnapshot {
        AccountingSnapshot {
            allocations: self.allocations,
            #[cfg(feature = "modern")]
            boa: self.boa.clone(),
        }
    }

    pub fn accounting_valid_after(&self, previous: &AccountingSnapshot) -> bool {
        self.accounting_valid() && self.accounting_snapshot().is_valid_after(previous)
    }

    /// A valid pre-realm rejection can retain readable source fallback, but
    /// cannot stand in for a report-less later reply. Trusted wire selection
    /// still requires modern lifecycle presence or strict legacy absence.
    pub fn is_rejected_initial(&self) -> bool {
        #[cfg(feature = "modern")]
        let has_boa = self.boa.is_some();
        #[cfg(not(feature = "modern"))]
        let has_boa = false;
        self.revision == 0
            && self.state == RealmState::Closed
            && self.snapshot.is_none()
            && self.allocations.is_none()
            && !has_boa
            && self.scripts_executed == 0
            && self.outcome.click_canceled.is_none()
            && self.outcome.submit_canceled.is_none()
            && self.default_action == DefaultAction::None
            && self.navigation.is_none()
            && self.acknowledgements.is_empty()
            && self.task_wake.is_none()
            && !self.has_platform_deltas()
            && self.lifecycle.is_none_or(|progress| {
                progress.phase == LifecyclePhase::Initializing && progress.initial_failure
            })
    }

    pub fn has_platform_deltas(&self) -> bool {
        !self.local_storage_ops.is_empty()
            || !self.session_storage_ops.is_empty()
            || !self.cookie_writes.is_empty()
            || !self.history_ops.is_empty()
            || !self.fetch_requests.is_empty()
            || self.scroll_request.is_some()
    }

    pub fn clear_platform_deltas(&mut self) {
        self.local_storage_ops.clear();
        self.session_storage_ops.clear();
        self.cookie_writes.clear();
        self.history_ops.clear();
        self.fetch_requests.clear();
        self.scroll_request = None;
    }

    pub fn validate_platform_deltas(&self) -> Result<(), String> {
        if self.local_storage_ops.len() > MAX_STORAGE_OPS_PER_TX
            || self.session_storage_ops.len() > MAX_STORAGE_OPS_PER_TX
            || self.cookie_writes.len() > MAX_COOKIE_WRITES_PER_TX
            || self.history_ops.len() > MAX_HISTORY_OPS_PER_TX
            || self.fetch_requests.len() > MAX_FETCH_REQUESTS_PER_TX
        {
            return Err("Script reply platform delta count exceeds limit".into());
        }
        for op in &self.local_storage_ops {
            op.validate()?;
        }
        for op in &self.session_storage_ops {
            op.validate()?;
        }
        for cookie in &self.cookie_writes {
            if cookie.len() > MAX_COOKIE_WRITE_BYTES {
                return Err("Script cookie write exceeds 4 KiB limit".into());
            }
        }
        for op in &self.history_ops {
            op.validate()?;
        }
        for req in &self.fetch_requests {
            req.validate()?;
        }
        if let Some(scroll) = &self.scroll_request {
            scroll.validate()?;
        }
        Ok(())
    }

    fn lifecycle_profile(&self) -> Result<Option<LifecycleProgress>, String> {
        #[cfg(feature = "modern")]
        let has_boa = self.boa.is_some();
        #[cfg(not(feature = "modern"))]
        let has_boa = false;
        match self.lifecycle {
            None if has_boa => Err("Modern reply omitted lifecycle progress".into()),
            None => Ok(None), // The transport separately selects the strict legacy shape.
            Some(_) if self.allocations.is_some() => {
                Err("Legacy accounting cannot advertise modern lifecycle progress".into())
            }
            Some(progress) if !has_boa => {
                if self.is_rejected_initial() {
                    Ok(Some(progress))
                } else {
                    Err("Lifecycle progress omitted its modern engine report".into())
                }
            }
            Some(progress) => Ok(Some(progress)),
        }
    }

    fn lifecycle_wait_valid(&self, progress: LifecycleProgress) -> Result<(), String> {
        if self.state == RealmState::Ready && self.navigation.is_none() {
            if progress.phase == LifecyclePhase::Initializing {
                return Err("Ready initialization made no lifecycle progress or navigation".into());
            }
            if progress.phase != LifecyclePhase::LoadComplete && self.task_wake.is_none() {
                return Err("Incomplete lifecycle omitted its task wake".into());
            }
        }
        Ok(())
    }

    /// Validate an Init observation. A trusted modern transport must also
    /// require presence; None is reserved for its explicitly selected v1 lane.
    pub fn validate_lifecycle_initial(&self) -> Result<Option<LifecycleProgress>, String> {
        let Some(progress) = self.lifecycle_profile()? else {
            return Ok(None);
        };
        if progress.phase > LifecyclePhase::Interactive {
            return Err("Init cannot complete a queued lifecycle task".into());
        }
        if (!self.errors.is_empty() || self.state != RealmState::Ready) && !progress.initial_failure
        {
            return Err("Initial lifecycle failure was not recorded".into());
        }
        self.lifecycle_wait_valid(progress)?;
        Ok(Some(progress))
    }

    /// Validate a cumulative observation, without claiming to identify the
    /// worker-local selected task behind an opaque TaskPump token.
    pub fn validate_lifecycle_after(
        &self,
        previous: Option<LifecycleProgress>,
        is_task_pump: bool,
    ) -> Result<Option<LifecycleProgress>, String> {
        let next = self.lifecycle_profile()?;
        let (Some(previous), Some(next)) = (previous, next) else {
            return if previous == next {
                Ok(None)
            } else {
                Err("Script session changed lifecycle protocol lanes".into())
            };
        };
        if next.phase < previous.phase {
            return Err("Lifecycle phase moved backward".into());
        }
        if next.phase != previous.phase
            && (!is_task_pump
                || self.state != RealmState::Ready
                || self.navigation.is_some()
                || !matches!(
                    (previous.phase, next.phase),
                    (
                        LifecyclePhase::Interactive,
                        LifecyclePhase::DomContentLoaded
                    ) | (
                        LifecyclePhase::DomContentLoaded,
                        LifecyclePhase::LoadComplete
                    )
                ))
        {
            return Err("Invalid lifecycle phase advancement".into());
        }
        if previous.initial_failure && !next.initial_failure {
            return Err("Initial lifecycle failure was reset".into());
        }
        if previous.phase == LifecyclePhase::LoadComplete {
            if next.initial_failure != previous.initial_failure {
                return Err("Completed initialization history changed".into());
            }
        } else if (!self.errors.is_empty() || self.state != RealmState::Ready)
            && !next.initial_failure
        {
            return Err("Initial lifecycle failure was not recorded".into());
        }
        self.lifecycle_wait_valid(next)?;
        Ok(Some(next))
    }

    /// One explicit accounting profile: never accept mixed or missing reports.
    pub fn accounting_valid(&self) -> bool {
        #[cfg(feature = "modern")]
        if let Some(report) = &self.boa {
            return self.allocations.is_none()
                && report.is_valid()
                && (self.state == RealmState::Ready || report.pending_jobs == 0)
                && (self.state != RealmState::Ready
                    || (report.fatal_reason.is_none() && report.opcodes_remaining > 0));
        }
        self.allocations.is_some_and(|report| {
            report.is_valid()
                && report.limit_bytes == 4 * 1024 * 1024
                && (self.state != RealmState::Ready || report.first_rejected.is_none())
        })
    }
}

#[cfg(all(test, feature = "modern"))]
mod lifecycle_tests {
    use super::*;

    fn reply(phase: LifecyclePhase) -> SessionReply {
        SessionReply {
            revision: 0,
            snapshot: Some(ArenaSnapshot { nodes: Vec::new() }),
            outcome: EventOutcome::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: Vec::new(),
            scripts_executed: 0,
            allocations: None,
            boa: Some(mg_butane::modern::Engine::new().unwrap().report()),
            state: RealmState::Ready,
            acknowledgements: Vec::new(),
            task_wake: (phase != LifecyclePhase::LoadComplete).then_some(TaskWake {
                token: 1,
                delay_ms: 0,
            }),
            lifecycle: Some(LifecycleProgress {
                phase,
                initial_failure: false,
            }),
            observer: Some(observer::ObserverReply::Disabled),
            local_storage_ops: Vec::new(),
            session_storage_ops: Vec::new(),
            cookie_writes: Vec::new(),
            history_ops: Vec::new(),
            fetch_requests: Vec::new(),
            scroll_request: None,
        }
    }

    #[test]
    fn lifecycle_object_requires_exact_typed_fields() {
        for malformed in [
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!({"phase":"Interactive"}),
            serde_json::json!({"phase":"Interactive","initial_failure":null}),
            serde_json::json!({"phase":"Interactive","initial_failure":0}),
            serde_json::json!({"phase":"Interactive","initial_failure":"false"}),
            serde_json::json!({"phase":"Complete","initial_failure":false}),
            serde_json::json!({"phase":"Interactive","initial_failure":false,"extra":0}),
        ] {
            assert!(serde_json::from_value::<LifecycleProgress>(malformed).is_err());
        }
        let progress = LifecycleProgress {
            phase: LifecyclePhase::Interactive,
            initial_failure: false,
        };
        assert_eq!(
            serde_json::from_slice::<LifecycleProgress>(&serde_json::to_vec(&progress).unwrap())
                .unwrap(),
            progress
        );
    }

    #[test]
    fn observer_outer_frame_preserves_fatal_policy_and_reportless_rejection() {
        use observer::{ObserverExpectation as E, ObserverFault as F, ObserverReply as R};
        let expected = E::WindowHarness { run_id: 1 };
        for fault in [
            F::StorageAdmission,
            F::AuthorityUnavailable,
            F::InvariantFailure,
        ] {
            let mut frame = reply(LifecyclePhase::LoadComplete);
            frame.observer = Some(R::ObservationFailed {
                run_id: 1,
                prefix_end: 0,
                fault,
                execution_failed: true,
            });
            assert!(
                frame.validate_observer_frame(expected).is_err(),
                "{fault:?} cannot be Ready"
            );
            frame.state = RealmState::Fatal;
            assert!(
                frame.validate_observer_frame(expected).is_err(),
                "{fault:?} needs an actual fatal report"
            );
            frame.boa.as_mut().unwrap().fatal_reason = Some("Owned fatal policy failure".into());
            frame.validate_observer_frame(expected).unwrap();
        }
        let mut narrow = reply(LifecyclePhase::LoadComplete);
        narrow.observer = Some(R::ObservationFailed {
            run_id: 1,
            prefix_end: 0,
            fault: F::MalformedCapture,
            execution_failed: false,
        });
        narrow.validate_observer_frame(expected).unwrap();
        let mut rejected = reply(LifecyclePhase::Initializing);
        rejected.state = RealmState::Closed;
        rejected.snapshot = None;
        rejected.boa = None;
        rejected.task_wake = None;
        rejected.lifecycle.as_mut().unwrap().initial_failure = true;
        rejected.observer = Some(R::rejected_init(expected));
        assert!(rejected.is_rejected_initial());
        rejected.validate_observer_frame(expected).unwrap();
        rejected.observer = Some(R::ObservationFailed {
            run_id: 1,
            prefix_end: 0,
            fault: F::InvalidConfiguration,
            execution_failed: true,
        });
        assert!(rejected.validate_observer_frame(expected).is_err());
    }

    #[test]
    fn init_requires_interactive_wake_but_preserves_early_navigation() {
        let mut value = reply(LifecyclePhase::Interactive);
        value.validate_lifecycle_initial().unwrap();
        value.task_wake = None;
        assert!(value.validate_lifecycle_initial().is_err());
        value.navigation = Some("https://example.test/next".into());
        value.lifecycle.as_mut().unwrap().phase = LifecyclePhase::Initializing;
        value.validate_lifecycle_initial().unwrap();
        value.navigation = None;
        assert!(value.validate_lifecycle_initial().is_err());
        for phase in [
            LifecyclePhase::DomContentLoaded,
            LifecyclePhase::LoadComplete,
        ] {
            assert!(reply(phase).validate_lifecycle_initial().is_err());
        }
        let mut value = reply(LifecyclePhase::Interactive);
        value.lifecycle = None;
        assert!(value.validate_lifecycle_initial().is_err());
    }

    #[test]
    fn only_ready_pumps_can_advance_one_lifecycle_phase() {
        let interactive = reply(LifecyclePhase::Interactive).lifecycle;
        let dcl = reply(LifecyclePhase::DomContentLoaded).lifecycle;
        let complete = reply(LifecyclePhase::LoadComplete).lifecycle;
        reply(LifecyclePhase::DomContentLoaded)
            .validate_lifecycle_after(interactive, true)
            .unwrap();
        reply(LifecyclePhase::LoadComplete)
            .validate_lifecycle_after(dcl, true)
            .unwrap();
        reply(LifecyclePhase::Interactive)
            .validate_lifecycle_after(interactive, false)
            .unwrap();
        assert!(
            reply(LifecyclePhase::LoadComplete)
                .validate_lifecycle_after(interactive, true)
                .is_err()
        );
        assert!(
            reply(LifecyclePhase::Interactive)
                .validate_lifecycle_after(dcl, true)
                .is_err()
        );
        assert!(
            reply(LifecyclePhase::DomContentLoaded)
                .validate_lifecycle_after(interactive, false)
                .is_err()
        );
        let mut terminal = reply(LifecyclePhase::DomContentLoaded);
        terminal.state = RealmState::Fatal;
        terminal.task_wake = None;
        terminal.lifecycle.as_mut().unwrap().initial_failure = true;
        assert!(
            terminal
                .validate_lifecycle_after(interactive, true)
                .is_err()
        );
        terminal.lifecycle.as_mut().unwrap().phase = LifecyclePhase::Interactive;
        terminal
            .validate_lifecycle_after(interactive, true)
            .unwrap();
        let mut navigating = reply(LifecyclePhase::DomContentLoaded);
        navigating.navigation = Some("https://example.test/next".into());
        assert!(
            navigating
                .validate_lifecycle_after(interactive, true)
                .is_err()
        );
        navigating.lifecycle.as_mut().unwrap().phase = LifecyclePhase::Interactive;
        navigating
            .validate_lifecycle_after(interactive, true)
            .unwrap();
        assert!(
            reply(LifecyclePhase::Interactive)
                .validate_lifecycle_after(complete, true)
                .is_err()
        );
    }

    #[test]
    fn ordinary_errors_latch_including_the_load_transition_and_cannot_reset() {
        let mut initial = reply(LifecyclePhase::Interactive);
        initial.errors.push("ordinary initial error".into());
        assert!(initial.validate_lifecycle_initial().is_err());
        initial.lifecycle.as_mut().unwrap().initial_failure = true;
        initial.validate_lifecycle_initial().unwrap();
        let prior = reply(LifecyclePhase::DomContentLoaded).lifecycle;
        let mut load = reply(LifecyclePhase::LoadComplete);
        load.errors.push("load callback error".into());
        assert!(load.validate_lifecycle_after(prior, true).is_err());
        load.lifecycle.as_mut().unwrap().initial_failure = true;
        load.validate_lifecycle_after(prior, true).unwrap();
        let failed = load.lifecycle;
        let mut clean = reply(LifecyclePhase::LoadComplete);
        assert!(clean.validate_lifecycle_after(failed, true).is_err());
        clean.lifecycle.as_mut().unwrap().initial_failure = true;
        clean.validate_lifecycle_after(failed, true).unwrap();
    }

    #[test]
    fn post_load_work_cannot_rewrite_initial_history_or_require_queue_drain() {
        let previous = reply(LifecyclePhase::LoadComplete).lifecycle;
        let mut later = reply(LifecyclePhase::LoadComplete);
        later.errors.push("unrelated later timer error".into());
        later.task_wake = Some(TaskWake {
            token: 9,
            delay_ms: 60_000,
        });
        later.validate_lifecycle_after(previous, true).unwrap();
        later.lifecycle.as_mut().unwrap().initial_failure = true;
        assert!(later.validate_lifecycle_after(previous, true).is_err());
    }

    #[test]
    fn reportless_rejection_is_an_exact_initial_only_exception() {
        let mut rejected = reply(LifecyclePhase::Initializing);
        rejected.boa = None;
        rejected.snapshot = None;
        rejected.state = RealmState::Closed;
        rejected.task_wake = None;
        rejected.lifecycle.as_mut().unwrap().initial_failure = true;
        rejected
            .errors
            .push("rejected before creating realm".into());
        assert!(rejected.is_rejected_initial());
        assert!(
            !rejected.accounting_valid(),
            "pre-realm rejection is not ordinary engine accounting"
        );
        rejected.validate_lifecycle_initial().unwrap();
        assert!(
            rejected
                .validate_lifecycle_after(reply(LifecyclePhase::Interactive).lifecycle, true)
                .is_err()
        );
        for field in [
            "revision",
            "scripts",
            "outcome",
            "navigation",
            "ack",
            "wake",
            "phase",
        ] {
            let mut changed: SessionReply =
                serde_json::from_value(serde_json::to_value(&rejected).unwrap()).unwrap();
            match field {
                "revision" => changed.revision = 1,
                "scripts" => changed.scripts_executed = 1,
                "outcome" => changed.outcome.click_canceled = Some(false),
                "navigation" => changed.navigation = Some("https://example.test/".into()),
                "ack" => changed.acknowledgements.push(ControlAck {
                    node: 0,
                    version: 1,
                }),
                "wake" => {
                    changed.task_wake = Some(TaskWake {
                        token: 1,
                        delay_ms: 0,
                    })
                }
                "phase" => changed.lifecycle.as_mut().unwrap().phase = LifecyclePhase::Interactive,
                _ => unreachable!(),
            }
            assert!(!changed.is_rejected_initial(), "{field}");
            assert!(changed.validate_lifecycle_initial().is_err(), "{field}");
        }
        rejected.lifecycle = None;
        assert!(rejected.is_rejected_initial());
        assert_eq!(rejected.validate_lifecycle_initial().unwrap(), None);
        assert!(
            rejected
                .validate_lifecycle_after(reply(LifecyclePhase::Interactive).lifecycle, true)
                .is_err()
        );
    }

    #[test]
    fn terminal_boa_accounting_cannot_retain_pending_work() {
        let mut value = reply(LifecyclePhase::DomContentLoaded);
        let report = value.boa.as_mut().unwrap();
        report.jobs_admitted = 1;
        report.pending_jobs = 1;
        assert!(report.is_valid());
        assert!(value.accounting_valid(), "Ready work is genuinely retained");
        for state in [RealmState::Closed, RealmState::Fatal] {
            value.state = state;
            assert!(
                !value.accounting_valid(),
                "terminal {state:?} cannot retain a permit/job"
            );
        }
        value.boa.as_mut().unwrap().pending_jobs = 0;
        assert!(
            value.accounting_valid(),
            "terminal cancellation preserves lifetime admission without pending work"
        );
    }

    #[test]
    fn accounting_snapshots_preserve_profiles_and_every_legacy_phase() {
        use mg_butane::runtime::{AllocationPhase, AllocationTotals, RejectedAllocation};
        let mut modern = reply(LifecyclePhase::LoadComplete);
        let initial = modern.accounting_snapshot();
        modern.boa.as_mut().unwrap().opcodes_remaining -= 1;
        assert!(modern.accounting_valid_after(&initial));
        let advanced = modern.accounting_snapshot();
        assert!(!initial.is_valid_after(&advanced));

        let mut legacy = reply(LifecyclePhase::LoadComplete);
        legacy.boa = None;
        legacy.lifecycle = None;
        legacy.allocations = Some(AllocationReport {
            limit_bytes: 4 * 1024 * 1024,
            accepted_bytes: 10,
            phases: AllocationTotals {
                runtime: 10,
                ..AllocationTotals::default()
            },
            first_rejected: None,
        });
        assert!(!legacy.accounting_valid_after(&initial));
        let before = legacy.accounting_snapshot();
        legacy.allocations.as_mut().unwrap().accepted_bytes += 1;
        legacy.allocations.as_mut().unwrap().phases.runtime += 1;
        assert!(legacy.accounting_valid_after(&before));
        let after = legacy.accounting_snapshot();
        assert!(!before.is_valid_after(&after));
        legacy.allocations.as_mut().unwrap().phases.runtime -= 1;
        legacy.allocations.as_mut().unwrap().phases.source += 1;
        assert!(legacy.accounting_valid());
        assert!(!legacy.accounting_valid_after(&after));
        legacy.allocations.as_mut().unwrap().first_rejected = Some(RejectedAllocation {
            phase: AllocationPhase::Runtime,
            accepted_bytes: 11,
            requested_bytes: 4 * 1024 * 1024,
            limit_bytes: 4 * 1024 * 1024,
        });
        legacy.state = RealmState::Fatal;
        let rejected = legacy.accounting_snapshot();
        assert!(rejected.is_valid_after(&rejected));
        legacy.allocations.as_mut().unwrap().first_rejected = None;
        assert!(!legacy.accounting_snapshot().is_valid_after(&rejected));
        legacy.allocations = None;
        assert!(!legacy.accounting_snapshot().is_valid_after(&rejected));
    }
}
