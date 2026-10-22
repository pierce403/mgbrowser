//! Sticky, bounded initial-script evidence, independent of transient UI status.
use mg_sparkle::page_session::{LifecyclePhase, RealmState, SessionReply};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialScriptStatus {
    NotRequested,
    Pending,
    Complete,
    Partial,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialScriptFailure {
    Script,
    Infrastructure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialScriptOutcome {
    pub status: InitialScriptStatus,
    pub failure: Option<InitialScriptFailure>,
    pub scripts_executed: usize,
    pub error_count: usize,
    pub broker_rejected_sources: usize,
    /// At most 64 accepted diagnostics, each at most 4096 UTF-8 bytes.
    pub diagnostics: Vec<String>,
}

impl InitialScriptOutcome {
    pub(super) fn new(requested: bool) -> Self {
        Self {
            status: if requested {
                InitialScriptStatus::Pending
            } else {
                InitialScriptStatus::NotRequested
            },
            failure: None,
            scripts_executed: 0,
            error_count: 0,
            broker_rejected_sources: 0,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn broker_rejections(&mut self, count: usize) {
        self.broker_rejected_sources = count;
        if count != 0 {
            self.failure = Some(InitialScriptFailure::Script);
        }
    }

    /// Call only after complete reply validation and the host's single charge.
    pub(super) fn accept(&mut self, reply: &SessionReply, initial: bool) {
        if self.status != InitialScriptStatus::Pending {
            return;
        }
        if initial {
            self.scripts_executed = reply.scripts_executed;
        }
        self.error_count = self.error_count.saturating_add(reply.errors.len());
        for error in &reply.errors {
            if self.diagnostics.len() == 64 {
                break;
            }
            // Already validated at admission. Do not retain unbounded host text.
            self.diagnostics.push(error.clone());
        }
        let failed = !reply.errors.is_empty()
            || reply
                .lifecycle
                .is_some_and(|progress| progress.initial_failure);
        if failed && self.failure.is_none() {
            self.failure = Some(InitialScriptFailure::Script);
        }
        if reply.state != RealmState::Ready {
            self.fail(
                InitialScriptFailure::Script,
                "Initial page realm became fatal or closed",
            );
        } else if reply
            .lifecycle
            .is_none_or(|progress| progress.phase == LifecyclePhase::LoadComplete)
        {
            self.status = if self.failure.is_some() {
                InitialScriptStatus::Partial
            } else {
                InitialScriptStatus::Complete
            };
        }
    }

    pub(super) fn fail(&mut self, kind: InitialScriptFailure, reason: &str) {
        if self.status != InitialScriptStatus::Pending {
            return;
        }
        self.status = InitialScriptStatus::Failed;
        if self.failure != Some(InitialScriptFailure::Infrastructure) {
            self.failure = Some(kind);
        }
        if self.diagnostics.len() < 64 {
            let mut end = reason.len().min(4096);
            while !reason.is_char_boundary(end) {
                end -= 1;
            }
            self.diagnostics.push(reason[..end].to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_diagnostic_is_bounded_at_utf8_boundary_and_terminal_outcome_is_sticky() {
        let mut outcome = InitialScriptOutcome::new(true);
        outcome.broker_rejections(2);
        outcome.fail(InitialScriptFailure::Infrastructure, &"é".repeat(4096));
        assert_eq!(outcome.status, InitialScriptStatus::Failed);
        assert_eq!(outcome.failure, Some(InitialScriptFailure::Infrastructure));
        assert_eq!(outcome.broker_rejected_sources, 2);
        assert_eq!(outcome.diagnostics[0].len(), 4096);
        let accepted = outcome.clone();
        outcome.fail(
            InitialScriptFailure::Script,
            "A later task must not rewrite initial history",
        );
        assert_eq!(outcome, accepted);
    }
}
