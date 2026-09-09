//! Parent-owned, bounded sessions; the legacy EOF-delimited worker is separate.
use mg_sparkle::{
    js_browser::Request,
    page_session::{SessionInput, SessionReply},
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct ChildPool {
    inner: Arc<Mutex<PoolState>>,
}
#[derive(Default)]
struct PoolState {
    owned: usize,
    generation: Option<u64>,
    closed: bool,
}
impl ChildPool {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set_generation(&self, generation: u64) -> Result<(), String> {
        let mut state = self
            .inner
            .lock()
            .map_err(|_| "Script-child pool unavailable")?;
        if state.closed {
            return Err("Script-child pool is permanently closed".into());
        }
        state.generation = Some(generation);
        Ok(())
    }
    fn current(&self, generation: u64) -> Result<(), String> {
        let state = self
            .inner
            .lock()
            .map_err(|_| "Script-child pool unavailable")?;
        if state.closed {
            return Err("Script-child pool is permanently closed".into());
        }
        if state
            .generation
            .is_some_and(|current| current != generation)
        {
            return Err("Script session belongs to a superseded document".into());
        }
        Ok(())
    }
    /// Permanently fence pending network completions and wake manager polling.
    pub fn close(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.closed = true;
    }
    /// Shutdown cleanup has its own finite wait; it grants no execution credit.
    pub fn wait_idle(&self) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let count = self
                .inner
                .lock()
                .map_err(|_| "Script-child pool unavailable")?
                .owned;
            if count == 0 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err("Script-child pool cleanup did not complete within two seconds".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}
struct Permit(ChildPool);
impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = self
            .0
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.owned = state.owned.saturating_sub(1);
    }
}

#[derive(Debug)]
pub struct SessionUpdate {
    pub generation: u64,
    pub session_id: u64,
    /// None denotes an idle closure, not an activation/default-action reply.
    pub sequence: Option<u64>,
    pub reply: Result<SessionReply, String>,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "script_worker_session_platform.rs"]
mod platform;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use platform::{Session, session_entry, session_selftest};

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub struct Session;
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
impl Session {
    pub fn start(_: Request, _: u64, _: ChildPool) -> Result<(Self, SessionReply), String> {
        Err("Script-worker isolation is only implemented for Linux x86_64".into())
    }
    pub fn id(&self) -> u64 {
        0
    }
    pub fn try_dispatch(&mut self, _: SessionInput, _: u64) -> Result<u64, String> {
        Err("Script-worker isolation is only implemented for Linux x86_64".into())
    }
    pub fn try_recv(&mut self) -> Option<SessionUpdate> {
        None
    }
    pub fn cancel(&self) {}
    pub fn charge_active(&mut self, _: std::time::Duration) -> Result<(), String> {
        Err("Script-worker isolation is only implemented for Linux x86_64".into())
    }
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn session_entry() -> ! {
    std::process::exit(78)
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn session_selftest() -> Result<(), String> {
    Err("Script-session isolation selftest requires Linux x86_64".into())
}
