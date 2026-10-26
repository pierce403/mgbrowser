//! Host contract for isolated page execution. Use one runtime service per browser.
//!
//! The host owns process creation, isolation, accounting, cancellation and reaping.
//! Implementations must reject unavailable isolation; Chassis has no in-process fallback.
pub use mg_sparkle::js_browser::SourceWireMode;
use mg_sparkle::{
    js_browser::Request,
    page_session::{
        SessionInput, SessionReply, TaskPump,
        automation::{AutomationRequest, AutomationResult},
    },
};
use std::time::Duration;

/// Host-derived worker liveness after accepting this completion. A Ready page
/// reply alone cannot promise a live realm: the last permitted transaction may
/// have completed just before the manager stopped and reaped its child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionContinuation {
    Live,
    Terminal,
}

pub struct SessionUpdate {
    pub generation: u64,
    pub session_id: u64,
    pub sequence: Option<u64>,
    pub reply: Result<SessionReply, String>,
    pub automation: Option<AutomationResult>,
    pub continuation: SessionContinuation,
}

pub trait ScriptSession: Send {
    fn id(&self) -> u64;
    fn try_dispatch(&mut self, input: SessionInput, revision: u64) -> Result<u64, String>;
    /// Tasks are not activations and carry no control edits or default actions.
    fn try_task_pump(&mut self, _: TaskPump, _: u64) -> Result<u64, String> {
        Err("This script runtime does not support task pumping".into())
    }
    /// Automation must share the runtime's ordinary single-flight slot and
    /// cumulative budgets; a host without this service fails explicitly.
    fn try_automation(&mut self, _: AutomationRequest, _: u64) -> Result<u64, String> {
        Err("This script runtime does not support Runtime automation".into())
    }
    fn try_recv(&mut self) -> Option<SessionUpdate>;
    fn cancel(&self);
    fn charge_active(&mut self, duration: Duration) -> Result<(), String>;
}

pub trait ScriptRuntime: Send + Sync {
    /// Host-only expectation. Ordinary browser services remain Disabled; a
    /// configured control host must separately validate its complete sources.
    fn observer_expectation(&self) -> mg_sparkle::page_session::observer::ObserverExpectation {
        mg_sparkle::page_session::observer::ObserverExpectation::Disabled
    }
    /// Trusted host-selected initial source shape. This grants no fetch or
    /// execution authority; it preserves old admission in the explicit legacy
    /// test lane. Page HTML and protocol input cannot select the mode.
    fn source_wire_mode(&self) -> SourceWireMode {
        SourceWireMode::Modern
    }
    /// Fence requests belonging to superseded documents before starting a job.
    fn set_generation(&self, generation: u64) -> Result<(), String>;
    fn start(
        &self,
        request: Request,
        generation: u64,
    ) -> Result<(Box<dyn ScriptSession>, SessionReply), String>;
    /// Permanently fence pending jobs before browser shutdown.
    fn close(&self);
    /// Wait a bounded time for all owned children to be reaped.
    fn wait_idle(&self) -> Result<(), String>;
}

/// A host without a script worker can still browse and render static documents.
#[derive(Default)]
pub struct DisabledScripts;
impl ScriptRuntime for DisabledScripts {
    fn set_generation(&self, _: u64) -> Result<(), String> {
        Ok(())
    }
    fn start(&self, _: Request, _: u64) -> Result<(Box<dyn ScriptSession>, SessionReply), String> {
        Err("This host has no isolated script runtime".into())
    }
    fn close(&self) {}
    fn wait_idle(&self) -> Result<(), String> {
        Ok(())
    }
}
