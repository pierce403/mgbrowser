//! Host contract for isolated page execution. Use one runtime service per browser.
//!
//! The host owns process creation, isolation, accounting, cancellation and reaping.
//! Implementations must reject unavailable isolation; Chassis has no in-process fallback.
use mg_sparkle::{
    js_browser::Request,
    page_session::{SessionInput, SessionReply},
};
use std::time::Duration;

pub struct SessionUpdate {
    pub generation: u64,
    pub session_id: u64,
    pub sequence: Option<u64>,
    pub reply: Result<SessionReply, String>,
}

pub trait ScriptSession: Send {
    fn id(&self) -> u64;
    fn try_dispatch(&mut self, input: SessionInput, revision: u64) -> Result<u64, String>;
    fn try_recv(&mut self) -> Option<SessionUpdate>;
    fn cancel(&self);
    fn charge_active(&mut self, duration: Duration) -> Result<(), String>;
}

pub trait ScriptRuntime: Send + Sync {
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
