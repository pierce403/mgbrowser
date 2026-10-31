use super::super::linux::{
    INPUT_LIMIT, OUTPUT_LIMIT, OwnedChild, WALL_LIMIT, encode, install_isolation, nonblocking,
};
use super::{ChildPool, Permit, Request, SessionInput, SessionReply, SessionUpdate, TaskPump};
use mg_butane::runtime::AllocationReport;
use mg_chassis::scripts::SessionContinuation;
use mg_sparkle::page_session::automation::{
    AutomationRequest, AutomationResult, MAX_AUTOMATION_FRAME_BYTES,
};
use mg_sparkle::page_session::console::{ConsoleCommand, ConsoleResult, ConsoleWireEntry};
use mg_sparkle::page_session::observer::{ObserverInit, ObserverReply};
use mg_sparkle::page_session::platform::{PlatformCommand, PlatformInit};
use mg_sparkle::page_session::{
    ArenaSnapshot, ControlAck, DefaultAction, EventOutcome, LifecycleProgress, MAX_EDIT_BYTES,
    MAX_EDITS, MAX_EVENT_BYTES, RealmState, TaskWake,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const VERSION: u8 = 9;
fn wire_version() -> u8 {
    if super::super::legacy_test_mode() {
        1
    } else {
        VERSION
    }
}
const LIFETIME: Duration = Duration::from_secs(300);
const TRANSACTIONS: u64 = 64;
const WIRE_LIMIT: usize = 32 * 1024 * 1024;
const TICK: Duration = Duration::from_millis(2);
const IDLE_TICK: Duration = Duration::from_millis(25);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestFrame<
    I = Request,
    E = SessionInput,
    T = TaskPump,
    A = AutomationRequest,
    C = ConsoleCommand,
    P = PlatformCommand,
> {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    command: FrameCommand<I, E, T, A, C, P>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", deny_unknown_fields)]
enum FrameCommand<
    I = Request,
    E = SessionInput,
    T = TaskPump,
    A = AutomationRequest,
    C = ConsoleCommand,
    P = PlatformCommand,
> {
    Init(I),
    Event(E),
    TaskPump(T),
    Automation(A),
    Console(C),
    Platform(P),
}

// This historical command vocabulary must never deserialize Automation, even
// with a v1 header. The executable's trusted lane selects it, not the input.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", deny_unknown_fields)]
enum LegacyFrameCommand<I, E = SessionInput, T = TaskPump> {
    Init(I),
    Event(E),
    TaskPump(T),
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRequestFrame<I, E = SessionInput, T = TaskPump> {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    command: LegacyFrameCommand<I, E, T>,
}

fn is_zero_seed(seed: &[u8; 32]) -> bool {
    *seed == [0u8; 32]
}

/// Init-only trusted configuration. Neither shared Request nor later commands
/// acquire an observation field; strict v1 continues to decode its old shape.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModernInit {
    page: Request,
    observer: ObserverInit,
    #[serde(default, skip_serializing_if = "PlatformInit::is_default")]
    platform: PlatformInit,
    #[serde(default, skip_serializing_if = "is_zero_seed")]
    crypto_seed: [u8; 32],
}

#[derive(Serialize)]
struct ModernInitView<'a> {
    page: &'a Request,
    observer: &'a ObserverInit,
    #[serde(skip_serializing_if = "PlatformInit::is_default")]
    platform: &'a PlatformInit,
    #[serde(skip_serializing_if = "is_zero_seed")]
    crypto_seed: &'a [u8; 32],
}

/// The executable's trusted lane chooses the decoder, never a wire field.
/// A legacy payload keeps its original strict shape, including unknown-field
/// rejection, while conversion moves strings without a Value tree or reparse.
fn decode_request_frame_full(
    payload: &[u8],
    legacy: bool,
) -> Result<(RequestFrame, ObserverInit, PlatformInit, [u8; 32]), String> {
    if legacy {
        let frame: LegacyRequestFrame<mg_sparkle::js_browser::LegacyRequest> =
            serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        if frame.version != 1 {
            return Err("Legacy lane requires protocol v1".into());
        }
        Ok((
            RequestFrame {
                version: frame.version,
                generation: frame.generation,
                session_id: frame.session_id,
                sequence: frame.sequence,
                expected_revision: frame.expected_revision,
                command: match frame.command {
                    LegacyFrameCommand::Init(request) => FrameCommand::Init(request.into()),
                    LegacyFrameCommand::Event(input) => FrameCommand::Event(input),
                    LegacyFrameCommand::TaskPump(pump) => FrameCommand::TaskPump(pump),
                },
            },
            ObserverInit::Disabled,
            PlatformInit::default(),
            [0u8; 32],
        ))
    } else {
        let frame: RequestFrame<ModernInit> =
            serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        if frame.version != VERSION {
            return Err("Modern lane requires protocol v9".into());
        }
        let mut observer = ObserverInit::Disabled;
        let mut platform = PlatformInit::default();
        let mut crypto_seed = [0u8; 32];
        let command = match frame.command {
            FrameCommand::Init(init) => {
                init.platform.validate()?;
                if !matches!(init.observer, ObserverInit::Disabled)
                    && (!init.platform.is_default() || init.crypto_seed != [0u8; 32])
                {
                    return Err(
                        "Observed WindowHarness session forbids platform init metadata".into(),
                    );
                }
                observer = init.observer;
                platform = init.platform;
                crypto_seed = init.crypto_seed;
                FrameCommand::Init(init.page)
            }
            FrameCommand::Event(input) => FrameCommand::Event(input),
            FrameCommand::TaskPump(pump) => FrameCommand::TaskPump(pump),
            FrameCommand::Automation(request) => FrameCommand::Automation(request),
            FrameCommand::Console(command) => FrameCommand::Console(command),
            FrameCommand::Platform(command) => FrameCommand::Platform(command),
        };
        Ok((
            RequestFrame {
                version: frame.version,
                generation: frame.generation,
                session_id: frame.session_id,
                sequence: frame.sequence,
                expected_revision: frame.expected_revision,
                command,
            },
            observer,
            platform,
            crypto_seed,
        ))
    }
}

#[cfg(test)]
fn decode_request_frame(
    payload: &[u8],
    legacy: bool,
) -> Result<(RequestFrame, ObserverInit), String> {
    decode_request_frame_full(payload, legacy).map(|(frame, observer, _, _)| (frame, observer))
}

#[cfg(test)]
fn encode_request_frame(
    frame: &RequestFrame,
    legacy: bool,
    limit: usize,
) -> Result<Vec<u8>, String> {
    encode_request_frame_observed(frame, legacy, limit, &ObserverInit::Disabled)
}

#[cfg(test)]
fn encode_request_frame_observed(
    frame: &RequestFrame,
    legacy: bool,
    limit: usize,
    observer: &ObserverInit,
) -> Result<Vec<u8>, String> {
    encode_request_frame_full(
        frame,
        legacy,
        limit,
        observer,
        &PlatformInit::default(),
        &[0u8; 32],
    )
}

fn encode_request_frame_full(
    frame: &RequestFrame,
    legacy: bool,
    limit: usize,
    observer: &ObserverInit,
    platform: &PlatformInit,
    crypto_seed: &[u8; 32],
) -> Result<Vec<u8>, String> {
    if !legacy {
        let command = match &frame.command {
            FrameCommand::Init(page) => FrameCommand::Init(ModernInitView {
                page,
                observer,
                platform,
                crypto_seed,
            }),
            FrameCommand::Event(input) => FrameCommand::Event(input),
            FrameCommand::TaskPump(pump) => FrameCommand::TaskPump(pump),
            FrameCommand::Automation(request) => FrameCommand::Automation(request),
            FrameCommand::Console(command) => FrameCommand::Console(command),
            FrameCommand::Platform(command) => FrameCommand::Platform(command),
        };
        return encode(
            &RequestFrame {
                version: frame.version,
                generation: frame.generation,
                session_id: frame.session_id,
                sequence: frame.sequence,
                expected_revision: frame.expected_revision,
                command,
            },
            limit,
        );
    }
    if !matches!(observer, ObserverInit::Disabled)
        || !platform.is_default()
        || !is_zero_seed(crypto_seed)
    {
        return Err("Legacy v1 cannot enable observation".into());
    }
    // Borrow sources, edits and pumps. Counting and transmission use the same
    // view; no modern-only provenance can change old v1 bytes or renew a budget.
    let command = match &frame.command {
        FrameCommand::Init(request) => LegacyFrameCommand::Init(request.legacy_wire_view()),
        FrameCommand::Event(input) => LegacyFrameCommand::Event(input),
        FrameCommand::TaskPump(pump) => LegacyFrameCommand::TaskPump(pump),
        FrameCommand::Automation(_) => return Err("Legacy v1 cannot carry automation".into()),
        FrameCommand::Console(_) => return Err("Legacy v1 cannot carry console".into()),
        FrameCommand::Platform(_) => return Err("Legacy v1 cannot carry platform".into()),
    };
    encode(
        &LegacyRequestFrame {
            version: frame.version,
            generation: frame.generation,
            session_id: frame.session_id,
            sequence: frame.sequence,
            expected_revision: frame.expected_revision,
            command,
        },
        limit,
    )
}
fn console_slice_empty<C: AsRef<[ConsoleWireEntry]>>(entries: &C) -> bool {
    entries.as_ref().is_empty()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyFrame<R = SessionReply> {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    reply: R,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<ModernResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    console: Vec<ConsoleWireEntry>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "payload", deny_unknown_fields)]
enum ModernResult {
    Page,
    Automation(AutomationResult),
    Console(ConsoleResult),
}
impl<'de> Deserialize<'de> for ModernResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // A unit Page has no payload, including no redundant null payload.
        // Preserve the existing adjacent serialization and owned tuple API.
        #[derive(Deserialize)]
        #[serde(tag = "kind", deny_unknown_fields)]
        enum StrictResult {
            Page {},
            Automation { payload: AutomationResult },
            Console { payload: ConsoleResult },
        }
        Ok(match StrictResult::deserialize(deserializer)? {
            StrictResult::Page {} => Self::Page,
            StrictResult::Automation { payload } => Self::Automation(payload),
            StrictResult::Console { payload } => Self::Console(payload),
        })
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(serialize = "R: Serialize, M: Serialize, C: Serialize + AsRef<[ConsoleWireEntry]>"))]
struct ModernReplyFrame<R = SessionReply, M = ModernResult, C = Vec<ConsoleWireEntry>> {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    reply: R,
    result: M,
    #[serde(default, skip_serializing_if = "console_slice_empty")]
    console: C,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyReplyFrame<R> {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    reply: R,
}

/// Exactly the historical v1 reply fields. A new optional field on SessionReply
/// must not make an explicit legacy `lifecycle: null` silently acceptable.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySessionReply {
    revision: u64,
    snapshot: Option<ArenaSnapshot>,
    outcome: EventOutcome,
    default_action: DefaultAction,
    navigation: Option<String>,
    errors: Vec<String>,
    scripts_executed: usize,
    allocations: Option<AllocationReport>,
    #[serde(default)]
    boa: Option<mg_butane::modern::Report>,
    state: RealmState,
    acknowledgements: Vec<ControlAck>,
    #[serde(default)]
    task_wake: Option<TaskWake>,
}

/// Counting and transmission borrow the same old view, without cloning the
/// arena, diagnostics, accounting report or any strings to remove new fields.
#[derive(Serialize)]
struct LegacySessionReplyView<'a> {
    revision: u64,
    snapshot: &'a Option<ArenaSnapshot>,
    outcome: &'a EventOutcome,
    default_action: &'a DefaultAction,
    navigation: &'a Option<String>,
    errors: &'a [String],
    scripts_executed: usize,
    allocations: &'a Option<AllocationReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    boa: Option<&'a mg_butane::modern::Report>,
    state: RealmState,
    acknowledgements: &'a [ControlAck],
    #[serde(skip_serializing_if = "Option::is_none")]
    task_wake: Option<&'a TaskWake>,
}

fn decode_reply_frame(payload: &[u8], legacy: bool) -> Result<ReplyFrame, String> {
    if legacy {
        let frame: LegacyReplyFrame<LegacySessionReply> =
            serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        if frame.version != 1 {
            return Err("Legacy reply requires protocol v1".into());
        }
        let old = frame.reply;
        Ok(ReplyFrame {
            version: frame.version,
            generation: frame.generation,
            session_id: frame.session_id,
            sequence: frame.sequence,
            expected_revision: frame.expected_revision,
            result: None,
            console: Vec::new(),
            reply: SessionReply {
                revision: old.revision,
                snapshot: old.snapshot,
                outcome: old.outcome,
                default_action: old.default_action,
                navigation: old.navigation,
                errors: old.errors,
                scripts_executed: old.scripts_executed,
                allocations: old.allocations,
                boa: old.boa,
                state: old.state,
                acknowledgements: old.acknowledgements,
                task_wake: old.task_wake,
                lifecycle: None,
                observer: None,
                local_storage_ops: Vec::new(),
                session_storage_ops: Vec::new(),
                cookie_writes: Vec::new(),
                history_ops: Vec::new(),
                fetch_requests: Vec::new(),
                scroll_request: None,
            },
        })
    } else {
        let frame: ModernReplyFrame =
            serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        if frame.version != VERSION {
            return Err("Modern reply requires protocol v9".into());
        }
        if frame.reply.lifecycle.is_none() || frame.reply.observer.is_none() {
            return Err("Modern v9 reply requires non-null lifecycle and observer progress".into());
        }
        Ok(ReplyFrame {
            version: frame.version,
            generation: frame.generation,
            session_id: frame.session_id,
            sequence: frame.sequence,
            expected_revision: frame.expected_revision,
            reply: frame.reply,
            result: Some(frame.result),
            console: frame.console,
        })
    }
}

fn write_reply_frame(
    writer: &mut impl Write,
    frame: &ReplyFrame,
    total: &mut usize,
    legacy: bool,
) -> Result<(), String> {
    if !legacy {
        if frame.reply.lifecycle.is_none() || frame.reply.observer.is_none() {
            return Err("Modern v9 reply requires lifecycle and observer progress".into());
        }
        frame.reply.validate_platform_deltas()?;
        let result = frame
            .result
            .as_ref()
            .ok_or("Modern v9 reply requires a command result")?;
        match result {
            ModernResult::Automation(result) => result.validate()?,
            ModernResult::Console(result) => result.validate()?,
            ModernResult::Page => {}
        }
        for entry in &frame.console {
            entry.validate()?;
        }
        return write_frame(
            writer,
            &ModernReplyFrame {
                version: frame.version,
                generation: frame.generation,
                session_id: frame.session_id,
                sequence: frame.sequence,
                expected_revision: frame.expected_revision,
                reply: &frame.reply,
                result,
                console: frame.console.as_slice(),
            },
            total,
        );
    }
    let reply = &frame.reply;
    if reply.lifecycle.is_some()
        || reply.observer.is_some()
        || reply.has_platform_deltas()
        || frame.result.is_some()
        || !frame.console.is_empty()
    {
        return Err("Legacy v1 reply forbids lifecycle and observer progress".into());
    }
    write_frame(
        writer,
        &LegacyReplyFrame {
            version: frame.version,
            generation: frame.generation,
            session_id: frame.session_id,
            sequence: frame.sequence,
            expected_revision: frame.expected_revision,
            reply: LegacySessionReplyView {
                revision: reply.revision,
                snapshot: &reply.snapshot,
                outcome: &reply.outcome,
                default_action: &reply.default_action,
                navigation: &reply.navigation,
                errors: &reply.errors,
                scripts_executed: reply.scripts_executed,
                allocations: &reply.allocations,
                boa: reply.boa.as_ref(),
                state: reply.state,
                acknowledgements: &reply.acknowledgements,
                task_wake: reply.task_wake.as_ref(),
            },
        },
        total,
    )
}

#[derive(Default)]
struct Ledger {
    used: Duration,
    failure: Option<String>,
}
#[derive(Default)]
struct Shared {
    cancel: AtomicBool,
    closed: AtomicBool,
    ledger: Mutex<Ledger>,
    spawned: Mutex<Option<Instant>>,
    pid: AtomicU32,
}
impl Shared {
    fn remaining(&self) -> Result<Duration, String> {
        let ledger = self
            .ledger
            .lock()
            .map_err(|_| "Script-session accounting unavailable")?;
        if let Some(error) = &ledger.failure {
            return Err(error.clone());
        }
        WALL_LIMIT
            .checked_sub(ledger.used)
            .filter(|left| !left.is_zero())
            .ok_or_else(|| "Script session exhausted its total 2-second active wall budget".into())
    }
    fn charge(&self, duration: Duration) -> Result<(), String> {
        self.check_lifetime()?;
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| "Script-session accounting unavailable")?;
        if let Some(error) = &ledger.failure {
            return Err(error.clone());
        }
        ledger.used = ledger.used.saturating_add(duration);
        if ledger.used >= WALL_LIMIT {
            let message =
                "Script session exhausted its total 2-second active wall budget".to_owned();
            ledger.failure = Some(message.clone());
            return Err(message);
        }
        Ok(())
    }
    fn fail(&self, message: String) -> String {
        match self.ledger.lock() {
            Ok(mut ledger) => ledger.failure.get_or_insert(message).clone(),
            Err(_) => "Script-session accounting unavailable".into(),
        }
    }
    fn close(&self, message: String) -> String {
        let error = self.fail(message);
        self.closed.store(true, Ordering::Release);
        error
    }
    fn check_lifetime(&self) -> Result<(), String> {
        let spawned = self
            .spawned
            .lock()
            .map_err(|_| "Script-session lifetime unavailable")?;
        if spawned.is_some_and(|started| started.elapsed() >= LIFETIME) {
            Err("Script session exceeded its 300-second absolute lifetime".into())
        } else {
            Ok(())
        }
    }
    fn error(&self) -> Option<String> {
        match self.ledger.lock() {
            Ok(ledger) => ledger.failure.clone(),
            Err(_) => Some("Script-session accounting unavailable".into()),
        }
    }
}

enum TransactionCommand {
    Event(SessionInput),
    TaskPump(TaskPump),
    Automation(AutomationRequest),
    Console(ConsoleCommand),
    Platform(PlatformCommand),
}
enum Control {
    Transaction {
        sequence: u64,
        expected_revision: u64,
        command: TransactionCommand,
        started: Instant,
    },
    Cancel,
}

fn generate_crypto_seed() -> [u8; 32] {
    let mut seed = [0u8; 32];
    let mut filled = 0usize;
    while filled < seed.len() {
        let rc = unsafe {
            libc::syscall(
                libc::SYS_getrandom,
                seed[filled..].as_mut_ptr().cast::<libc::c_void>(),
                seed.len() - filled,
                0,
            )
        };
        if rc > 0 {
            filled += rc as usize;
        } else if rc == -1 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        } else {
            break;
        }
    }
    if seed == [0u8; 32] {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0x9e37_79b9_7f4a_7c15);
        seed[..16].copy_from_slice(&nanos.to_le_bytes());
        seed[0] |= 1;
    }
    seed
}

pub struct Session {
    generation: u64,
    id: u64,
    revision: u64,
    next_sequence: u64,
    pending: Option<u64>,
    validation_pending: bool,
    task_wake: Option<mg_sparkle::page_session::TaskWake>,
    closure_reported: bool,
    tx: Option<SyncSender<Control>>,
    rx: Receiver<SessionUpdate>,
    shared: Arc<Shared>,
    manager: Option<JoinHandle<()>>,
    pool: ChildPool,
    automation_enabled: bool,
    lifecycle: Option<LifecycleProgress>,
    pending_console: Vec<ConsoleWireEntry>,
    last_console_result: Option<ConsoleResult>,
}
impl Session {
    pub fn start(
        request: Request,
        generation: u64,
        pool: ChildPool,
    ) -> Result<(Self, SessionReply), String> {
        Self::start_with_observer(request, generation, pool, ObserverInit::Disabled)
    }
    pub fn start_with_observer(
        request: Request,
        generation: u64,
        pool: ChildPool,
        observer: ObserverInit,
    ) -> Result<(Self, SessionReply), String> {
        Self::start_with_platform(request, generation, pool, observer, PlatformInit::default())
    }
    pub fn start_with_platform(
        request: Request,
        generation: u64,
        pool: ChildPool,
        observer: ObserverInit,
        platform: PlatformInit,
    ) -> Result<(Self, SessionReply), String> {
        let started = Instant::now();
        observer.validate().map_err(|error| error.to_string())?;
        platform.validate()?;
        if super::super::legacy_test_mode()
            && (!matches!(observer, ObserverInit::Disabled) || !platform.is_default())
        {
            return Err("Legacy v1 cannot enable observation".into());
        }
        if !matches!(observer, ObserverInit::Disabled) && !platform.is_default() {
            return Err("Observed WindowHarness session forbids platform init metadata".into());
        }
        pool.current(generation)?;
        let id = NEXT_SESSION
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
            .map_err(|_| "Script-session identity exhausted")?;
        let shared = Arc::new(Shared::default());
        let (tx, commands) = mpsc::sync_channel(1);
        let (updates, rx) = mpsc::sync_channel(1);
        let manager_shared = shared.clone();
        let manager_pool = pool.clone();
        let automation_enabled =
            !super::super::legacy_test_mode() && matches!(observer, ObserverInit::Disabled);
        let crypto_seed = if automation_enabled {
            generate_crypto_seed()
        } else {
            [0u8; 32]
        };
        let manager = thread::Builder::new()
            .name("mgbrowser-page-worker".into())
            .spawn(move || {
                manage(
                    request,
                    observer,
                    platform,
                    crypto_seed,
                    generation,
                    id,
                    manager_pool,
                    manager_shared,
                    commands,
                    updates,
                    started,
                );
            })
            .map_err(|error| format!("Cannot start script-session manager: {error}"))?;
        let mut session = Self {
            generation,
            id,
            revision: 0,
            next_sequence: 1,
            pending: None,
            validation_pending: true,
            task_wake: None,
            closure_reported: false,
            tx: Some(tx),
            rx,
            shared,
            manager: Some(manager),
            pool,
            automation_enabled,
            lifecycle: None,
            pending_console: Vec::new(),
            last_console_result: None,
        };
        let left = WALL_LIMIT.saturating_sub(started.elapsed());
        let update = session.rx.recv_timeout(left).map_err(|_| {
            "Script session initialization did not finish within its active wall budget".to_owned()
        })?;
        if update.generation != generation || update.session_id != id || update.sequence != Some(0)
        {
            return Err("Invalid initial script-session completion".into());
        }
        let reply = update.reply?;
        if update.automation.is_some() || update.console_result.is_some() {
            return Err("Initial completion cannot carry an automation result".into());
        }
        session.pending_console = update.console;
        session.revision = reply.revision;
        session.lifecycle = reply.lifecycle;
        session.task_wake = reply.task_wake.clone();
        session.validation_pending = true;
        Ok((session, reply))
    }
    pub fn id(&self) -> u64 {
        self.id
    }
    /// Measured parent projection/validation work must be admitted before use.
    /// Each completion requires exactly one acknowledgement before new input.
    pub fn charge_active(&mut self, duration: Duration) -> Result<(), String> {
        if !self.validation_pending {
            return Err("No script projection awaits validation".into());
        }
        if let Err(error) = self
            .pool
            .current(self.generation)
            .and_then(|_| self.shared.charge(duration))
        {
            self.shared.fail(error.clone());
            self.cancel();
            return Err(error);
        }
        if self.shared.cancel.load(Ordering::Acquire) {
            return Err(self
                .shared
                .error()
                .unwrap_or_else(|| "Script session is closed".into()));
        }
        self.validation_pending = false;
        Ok(())
    }
    pub fn try_dispatch(
        &mut self,
        input: SessionInput,
        expected_revision: u64,
    ) -> Result<u64, String> {
        self.try_submit(TransactionCommand::Event(input), expected_revision)
    }
    pub fn try_task_pump(&mut self, pump: TaskPump, expected_revision: u64) -> Result<u64, String> {
        if super::super::legacy_test_mode() {
            return Err("The legacy test runtime does not support task pumping".into());
        }
        if self.task_wake.as_ref().map(|wake| wake.token) != Some(pump.wake_token) {
            return Err("Task pump does not match the advertised wake".into());
        }
        self.try_submit(TransactionCommand::TaskPump(pump), expected_revision)
    }
    pub fn try_automation(
        &mut self,
        request: AutomationRequest,
        expected_revision: u64,
    ) -> Result<u64, String> {
        if !self.automation_enabled {
            return Err("Automation requires a modern Disabled-observer session".into());
        }
        if !self.lifecycle.is_some_and(|progress| {
            progress.phase == mg_sparkle::page_session::LifecyclePhase::LoadComplete
        }) {
            return Err("Automation requires completed initial load".into());
        }
        self.try_submit(TransactionCommand::Automation(request), expected_revision)
    }
    pub fn try_console(
        &mut self,
        command: ConsoleCommand,
        expected_revision: u64,
    ) -> Result<u64, String> {
        if !self.automation_enabled {
            return Err("Console requires a modern Disabled-observer session".into());
        }
        if !self.lifecycle.is_some_and(|progress| {
            progress.phase == mg_sparkle::page_session::LifecyclePhase::LoadComplete
        }) {
            return Err("Console requires completed initial load".into());
        }
        self.try_submit(TransactionCommand::Console(command), expected_revision)
    }
    pub fn try_platform(
        &mut self,
        command: PlatformCommand,
        expected_revision: u64,
    ) -> Result<u64, String> {
        if !self.automation_enabled {
            return Err("Platform commands require a modern Disabled-observer session".into());
        }
        self.try_submit(TransactionCommand::Platform(command), expected_revision)
    }
    pub fn take_console(&mut self) -> (Vec<ConsoleWireEntry>, Option<ConsoleResult>) {
        (
            std::mem::take(&mut self.pending_console),
            self.last_console_result.take(),
        )
    }
    fn try_submit(
        &mut self,
        command: TransactionCommand,
        expected_revision: u64,
    ) -> Result<u64, String> {
        let started = Instant::now();
        if self.pending.is_some() || self.validation_pending {
            return Err("A script transaction or projection is already pending".into());
        }
        self.pool.current(self.generation)?;
        if self.shared.cancel.load(Ordering::Acquire) || self.shared.closed.load(Ordering::Acquire)
        {
            return Err(self
                .shared
                .error()
                .unwrap_or_else(|| "Script session is closed".into()));
        }
        if expected_revision != self.revision {
            return Err("Stale script-session revision".into());
        }
        if let Err(error) = validate_command(&command) {
            if let Err(charge_error) = self.shared.charge(started.elapsed()) {
                self.cancel();
                return Err(charge_error);
            }
            // Public automation preflight is nonmutating and consumes no slot.
            // The same validation on untrusted child wire remains terminal.
            if matches!(
                command,
                TransactionCommand::Automation(_)
                    | TransactionCommand::Console(_)
                    | TransactionCommand::Platform(_)
            ) {
                return Err(error);
            }
            self.shared.fail(error.clone());
            self.cancel();
            return Err(error);
        }
        if self.next_sequence >= TRANSACTIONS {
            let error = self
                .shared
                .fail("Script session reached its 64-transaction limit".into());
            self.cancel();
            return Err(error);
        }
        let sequence = self.next_sequence;
        let command = Control::Transaction {
            sequence,
            expected_revision,
            command,
            started,
        };
        match self
            .tx
            .as_ref()
            .ok_or("Script session is closed")?
            .try_send(command)
        {
            Ok(()) => {
                self.next_sequence += 1;
                self.pending = Some(sequence);
                self.task_wake = None;
                Ok(sequence)
            }
            Err(TrySendError::Full(_)) => Err("Script-session command queue is full".into()),
            Err(TrySendError::Disconnected(_)) => Err("Script-session manager is closed".into()),
        }
    }
    pub fn try_recv(&mut self) -> Option<SessionUpdate> {
        match self.rx.try_recv() {
            Ok(mut update) => {
                self.pending = None;
                if update.reply.is_ok()
                    && let Some(error) = self.shared.error()
                {
                    update.reply = Err(error);
                    update.automation = None;
                    update.console.clear();
                    update.console_result = None;
                    update.continuation = SessionContinuation::Terminal;
                }
                if let Ok(reply) = &update.reply {
                    self.revision = reply.revision;
                    self.lifecycle = reply.lifecycle;
                    self.validation_pending = true;
                    self.task_wake = reply.task_wake.clone();
                    self.pending_console
                        .extend(std::mem::take(&mut update.console));
                    self.last_console_result = update.console_result.take();
                } else {
                    self.validation_pending = false;
                    self.task_wake = None;
                    self.closure_reported = true;
                }
                Some(update)
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                if !self.closure_reported && self.shared.closed.load(Ordering::Acquire) {
                    self.closure_reported = true;
                    self.validation_pending = false;
                    self.task_wake = None;
                    Some(SessionUpdate {
                        generation: self.generation,
                        session_id: self.id,
                        sequence: self.pending.take(),
                        automation: None,
                        console: Vec::new(),
                        console_result: None,
                        continuation: SessionContinuation::Terminal,
                        reply: Err(self
                            .shared
                            .error()
                            .unwrap_or_else(|| "Script session is closed".into())),
                    })
                } else {
                    None
                }
            }
        }
    }
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Release);
        if let Some(tx) = &self.tx {
            let _ = tx.try_send(Control::Cancel);
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
        self.tx.take();
        if let Some(manager) = self.manager.take() {
            let _ = manager.join();
        }
    }
}

struct Worker {
    owned: OwnedChild,
    stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
    // Last field: the child's Drop guard precedes release of its permit.
    permit: Option<Permit>,
    spawned: Instant,
    wire: usize,
    generation: u64,
    pool: ChildPool,
    last_report: Option<AllocationReport>,
    last_boa: Option<mg_butane::modern::Report>,
    last_wake_token: u64,
    last_lifecycle: Option<LifecycleProgress>,
    legacy: bool,
    observer: ObserverInit,
    init_platform: Option<(PlatformInit, [u8; 32])>,
}
impl Worker {
    fn start(
        pool: ChildPool,
        generation: u64,
        observer: ObserverInit,
        platform: PlatformInit,
        crypto_seed: [u8; 32],
    ) -> Result<Self, String> {
        // Keep this process and its workers on the same build across an update.
        let mut command = Command::new("/proc/self/exe");
        command
            .arg(if super::super::legacy_test_mode() {
                "--legacy-script-session"
            } else {
                "--script-session"
            })
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (child, permit, spawned) = {
            let mut state = pool
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
            if state.owned >= 2 {
                return Err("Two owned script children are already active or retiring".into());
            }
            let spawned = Instant::now();
            let child = command
                .spawn()
                .map_err(|error| format!("Cannot start script session: {error}"))?;
            state.owned += 1;
            (child, Permit(pool.clone()), spawned)
        };
        let mut owned = OwnedChild {
            child,
            reaped: false,
        };
        let stdin = owned
            .child
            .stdin
            .take()
            .ok_or("Script-session stdin missing")?;
        let stdout = owned
            .child
            .stdout
            .take()
            .ok_or("Script-session stdout missing")?;
        nonblocking(stdin.as_raw_fd())
            .and_then(|_| nonblocking(stdout.as_raw_fd()))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            owned,
            stdin,
            stdout,
            permit: Some(permit),
            spawned,
            wire: 0,
            generation,
            pool,
            last_report: None,
            last_boa: None,
            last_wake_token: 0,
            last_lifecycle: None,
            legacy: super::super::legacy_test_mode(),
            observer,
            init_platform: Some((platform, crypto_seed)),
        })
    }
    fn check(&mut self, shared: &Shared, deadline: Option<Instant>) -> Result<(), String> {
        if shared.cancel.load(Ordering::Acquire) {
            return Err("Script session canceled".into());
        }
        self.pool.current(self.generation)?;
        if self.spawned.elapsed() >= LIFETIME {
            return Err("Script session exceeded its 300-second absolute lifetime".into());
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err("Script session exhausted its total 2-second active wall budget".into());
        }
        Ok(())
    }
    fn add_wire(&mut self, bytes: usize) -> Result<(), String> {
        add_wire(&mut self.wire, bytes)
    }
    fn idle(&mut self, shared: &Shared) -> Result<(), String> {
        self.check(shared, None)?;
        if let Some(status) = self
            .owned
            .child
            .try_wait()
            .map_err(|error| error.to_string())?
        {
            self.owned.reaped = true;
            return Err(format!("Script-session child terminated: {status}"));
        }
        let mut byte = [0];
        match self.stdout.read(&mut byte) {
            Ok(0) => Err("Script-session output closed while idle".into()),
            Ok(_) => Err("Unsolicited script-session output".into()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(())
            }
            Err(error) => Err(format!("Script-session output failed: {error}")),
        }
    }
    fn transaction(
        &mut self,
        request: &RequestFrame,
        shared: &Shared,
        started: Instant,
    ) -> Result<
        (
            SessionReply,
            Option<AutomationResult>,
            Vec<ConsoleWireEntry>,
            Option<ConsoleResult>,
        ),
        String,
    > {
        if matches!(
            request.command,
            FrameCommand::Automation(_) | FrameCommand::Console(_) | FrameCommand::Platform(_)
        ) && (self.legacy || !matches!(self.observer, ObserverInit::Disabled))
        {
            return Err("Automation requires a modern Disabled-observer session".into());
        }
        let deadline = started
            .checked_add(shared.remaining()?)
            .ok_or("Script active deadline exceeds the monotonic clock range")?;
        let limit = match request.command {
            FrameCommand::Init(_) | FrameCommand::Platform(_) => INPUT_LIMIT,
            FrameCommand::Automation(_) => MAX_AUTOMATION_FRAME_BYTES,
            _ => MAX_EVENT_BYTES,
        };
        let (init_plat, init_seed) = self
            .init_platform
            .take()
            .unwrap_or_else(|| (PlatformInit::default(), [0u8; 32]));
        let input = encode_request_frame_full(
            request,
            self.legacy,
            limit,
            &self.observer,
            &init_plat,
            &init_seed,
        )
        .map_err(|error| format!("Script-session request rejected: {error}"))?;
        if input.len().saturating_add(4) > WIRE_LIMIT.saturating_sub(self.wire) {
            return Err("Script session exceeded its 32 MiB combined wire limit".into());
        }
        let header = (input.len() as u32).to_be_bytes();
        let mut sent_header = 0;
        let mut sent = 0;
        let mut received_header = [0; 4];
        let mut header_read = 0;
        let mut output: Option<Vec<u8>> = None;
        let mut expected = 0;
        let mut buffer = [0u8; 16 * 1024];
        loop {
            self.check(shared, Some(deadline))?;
            let mut progressed = false;
            let send = if sent_header < 4 {
                &header[sent_header..]
            } else {
                &input[sent..(sent + buffer.len()).min(input.len())]
            };
            if !send.is_empty() {
                match self.stdin.write(send) {
                    Ok(0) => return Err("Script-session input stopped accepting bytes".into()),
                    Ok(bytes) => {
                        if sent_header < 4 {
                            sent_header += bytes;
                        } else {
                            sent += bytes;
                        }
                        self.add_wire(bytes)?;
                        progressed = true;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(format!("Script-session input failed: {error}")),
                }
            }
            let read = if header_read < 4 {
                self.stdout.read(&mut received_header[header_read..])
            } else {
                let bytes = output.as_ref().map_or(0, Vec::len);
                let amount = (expected - bytes).min(buffer.len());
                self.stdout.read(&mut buffer[..amount])
            };
            match read {
                Ok(0) => return Err("Truncated script-session reply".into()),
                Ok(bytes) => {
                    self.add_wire(bytes)?;
                    progressed = true;
                    if header_read < 4 {
                        header_read += bytes;
                        if header_read == 4 {
                            expected = u32::from_be_bytes(received_header) as usize;
                            check_payload(expected, OUTPUT_LIMIT, self.wire)?;
                            output = Some(Vec::with_capacity(expected));
                        }
                    } else {
                        output.as_mut().unwrap().extend_from_slice(&buffer[..bytes]);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(format!("Script-session output failed: {error}")),
            }
            if let Some(output) = &output
                && output.len() == expected
            {
                if sent_header != 4 || sent != input.len() {
                    return Err("Script-session reply preceded the complete request".into());
                }
                let frame = decode_reply_frame(output, self.legacy)
                    .map_err(|error| format!("Invalid script-session reply: {error}"))?;
                validate_reply(request, &frame)?;
                validate_observer_mode(&self.observer, &frame.reply, self.legacy)?;
                let lifecycle = if matches!(request.command, FrameCommand::Init(_)) {
                    frame.reply.validate_lifecycle_initial()?
                } else {
                    frame.reply.validate_lifecycle_after(
                        self.last_lifecycle,
                        matches!(request.command, FrameCommand::TaskPump(_)),
                    )?
                };
                if let Some(wake) = &frame.reply.task_wake {
                    if self.legacy || wake.token <= self.last_wake_token {
                        return Err("Invalid or nonmonotonic script task wake".into());
                    }
                }
                validate_cumulative(self.last_report, frame.reply.allocations)?;
                validate_engine_profile(&frame.reply, self.legacy, self.last_boa.as_ref())?;
                self.check(shared, Some(deadline))?;
                // A complete response is one frame, not the prefix of an
                // unsolicited stream. Check coalesced trailing bytes before
                // handing anything to the parent; future unsolicited bytes
                // are additionally checked while idle.
                let mut extra = [0];
                match self.stdout.read(&mut extra) {
                    Ok(0)
                        if frame.reply.state != RealmState::Ready
                            || request.sequence + 1 == TRANSACTIONS => {}
                    Ok(0) => return Err("Ready script-session output closed unexpectedly".into()),
                    Ok(_) => return Err("Extra data after script-session reply".into()),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error.to_string()),
                }
                self.last_report = frame.reply.allocations;
                self.last_boa = frame.reply.boa.clone();
                self.last_lifecycle = lifecycle;
                if let Some(wake) = &frame.reply.task_wake {
                    self.last_wake_token = wake.token;
                }
                let (automation, console_result) = match frame.result {
                    Some(ModernResult::Automation(result)) => (Some(result), None),
                    Some(ModernResult::Console(result)) => (None, Some(result)),
                    Some(ModernResult::Page) | None => (None, None),
                };
                return Ok((frame.reply, automation, frame.console, console_result));
            }
            if !progressed {
                thread::sleep(TICK);
            }
        }
    }
    fn stop(&mut self) -> bool {
        self.owned.stop();
        if !self.owned.reaped {
            // Do not admit a replacement child after an unconfirmed reap.
            if let Some(permit) = self.permit.take() {
                std::mem::forget(permit);
            }
        }
        self.owned.reaped
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn update(
    generation: u64,
    id: u64,
    sequence: Option<u64>,
    reply: Result<SessionReply, String>,
) -> SessionUpdate {
    SessionUpdate {
        generation,
        session_id: id,
        sequence,
        reply,
        automation: None,
        console: Vec::new(),
        console_result: None,
        continuation: SessionContinuation::Terminal,
    }
}
fn manage(
    request: Request,
    observer: ObserverInit,
    platform: PlatformInit,
    crypto_seed: [u8; 32],
    generation: u64,
    id: u64,
    pool: ChildPool,
    shared: Arc<Shared>,
    commands: Receiver<Control>,
    updates: SyncSender<SessionUpdate>,
    started: Instant,
) {
    let mut worker = match Worker::start(pool, generation, observer, platform, crypto_seed) {
        Ok(worker) => worker,
        Err(error) => {
            let error = shared.close(error);
            let _ = updates.try_send(update(generation, id, Some(0), Err(error)));
            return;
        }
    };
    shared.pid.store(worker.owned.child.id(), Ordering::Release);
    match shared.spawned.lock() {
        Ok(mut spawned) => *spawned = Some(worker.spawned),
        Err(_) => {
            worker.stop();
            let error = shared.close("Script-session lifetime unavailable".into());
            let _ = updates.try_send(update(generation, id, Some(0), Err(error)));
            return;
        }
    }
    let initial = RequestFrame {
        version: wire_version(),
        generation,
        session_id: id,
        sequence: 0,
        expected_revision: 0,
        command: FrameCommand::Init(request),
    };
    let result = worker.transaction(&initial, &shared, started);
    let elapsed = shared.charge(started.elapsed());
    let result = result.and_then(|reply| elapsed.map(|_| reply));
    let mut revision = match result {
        Ok((reply, automation, console, console_result)) => {
            let revision = reply.revision;
            let terminal = reply.state != RealmState::Ready;
            if terminal && !worker.stop() {
                let error = shared.close("Terminal script child reaping was not confirmed".into());
                let _ = updates.try_send(update(generation, id, Some(0), Err(error)));
                return;
            }
            let mut completion = update(generation, id, Some(0), Ok(reply));
            completion.automation = automation;
            completion.console = console;
            completion.console_result = console_result;
            completion.continuation = if terminal {
                SessionContinuation::Terminal
            } else {
                SessionContinuation::Live
            };
            if updates.try_send(completion).is_err() {
                worker.stop();
                shared.close("Script-session initial completion disconnected".into());
                return;
            }
            if terminal {
                shared.closed.store(true, Ordering::Release);
                return;
            }
            revision
        }
        Err(error) => {
            worker.stop();
            let error = shared.close(error);
            let _ = updates.try_send(update(generation, id, Some(0), Err(error)));
            return;
        }
    };
    let mut next_sequence = 1;
    loop {
        if let Err(error) = worker.idle(&shared) {
            worker.stop();
            let error = shared.close(error);
            let _ = updates.try_send(update(generation, id, None, Err(error)));
            return;
        }
        match commands
            .recv_timeout(IDLE_TICK.min(LIFETIME.saturating_sub(worker.spawned.elapsed())))
        {
            Ok(Control::Transaction {
                sequence,
                expected_revision,
                command,
                started,
            }) => {
                let result = (|| {
                    if sequence != next_sequence
                        || sequence >= TRANSACTIONS
                        || expected_revision != revision
                    {
                        return Err(
                            "Invalid script-session transaction sequence or revision".into()
                        );
                    }
                    validate_command(&command)?;
                    let request = RequestFrame {
                        version: wire_version(),
                        generation,
                        session_id: id,
                        sequence,
                        expected_revision,
                        command: match command {
                            TransactionCommand::Event(input) => FrameCommand::Event(input),
                            TransactionCommand::TaskPump(pump) => FrameCommand::TaskPump(pump),
                            TransactionCommand::Automation(request) => {
                                FrameCommand::Automation(request)
                            }
                            TransactionCommand::Console(command) => FrameCommand::Console(command),
                            TransactionCommand::Platform(command) => {
                                FrameCommand::Platform(command)
                            }
                        },
                    };
                    worker.transaction(&request, &shared, started)
                })();
                let elapsed = shared.charge(started.elapsed());
                match result.and_then(|reply| elapsed.map(|_| reply)) {
                    Ok((reply, automation, console, console_result)) => {
                        next_sequence += 1;
                        revision = reply.revision;
                        let terminal =
                            reply.state != RealmState::Ready || sequence + 1 == TRANSACTIONS;
                        if terminal && !worker.stop() {
                            let error = shared
                                .close("Terminal script child reaping was not confirmed".into());
                            let _ = updates.try_send(update(
                                generation,
                                id,
                                Some(sequence),
                                Err(error),
                            ));
                            return;
                        }
                        let mut completion = update(generation, id, Some(sequence), Ok(reply));
                        completion.automation = automation;
                        completion.console = console;
                        completion.console_result = console_result;
                        completion.continuation = if terminal {
                            SessionContinuation::Terminal
                        } else {
                            SessionContinuation::Live
                        };
                        if updates.try_send(completion).is_err() {
                            worker.stop();
                            shared.close(
                                "Script-session completion queue is full or disconnected".into(),
                            );
                            return;
                        }
                        if terminal {
                            shared.closed.store(true, Ordering::Release);
                            return;
                        }
                    }
                    Err(error) => {
                        worker.stop();
                        let error = shared.close(error);
                        let _ =
                            updates.try_send(update(generation, id, Some(sequence), Err(error)));
                        return;
                    }
                }
            }
            Ok(Control::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                worker.stop();
                let error = shared.close("Script session canceled".into());
                let _ = updates.try_send(update(generation, id, None, Err(error)));
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn validate_input(input: &SessionInput) -> Result<(), String> {
    if input.edits.len() > MAX_EDITS {
        return Err("Script event exceeds its control-edit count limit".into());
    }
    for (index, edit) in input.edits.iter().enumerate() {
        if edit.value.len() > MAX_EDIT_BYTES {
            return Err("Script event exceeds its control-value byte limit".into());
        }
        if input.edits[..index]
            .iter()
            .any(|prior| prior.node == edit.node)
        {
            return Err("Duplicate control edit in script event".into());
        }
    }
    Ok(())
}
fn validate_command(command: &TransactionCommand) -> Result<(), String> {
    match command {
        TransactionCommand::Event(input) => validate_input(input),
        TransactionCommand::TaskPump(pump) if pump.wake_token != 0 => Ok(()),
        TransactionCommand::TaskPump(_) => Err("Task wake token must be nonzero".into()),
        TransactionCommand::Automation(request) => request.validate(),
        TransactionCommand::Console(command) => command.validate(),
        TransactionCommand::Platform(command) => command.validate(),
    }
}
fn validate_reply(request: &RequestFrame, frame: &ReplyFrame) -> Result<(), String> {
    if !matches!(request.version, 1 | VERSION) {
        return Err("Unsupported script-session protocol version".into());
    }
    if matches!(
        request.command,
        FrameCommand::TaskPump(_)
            | FrameCommand::Automation(_)
            | FrameCommand::Console(_)
            | FrameCommand::Platform(_)
    ) && request.version != VERSION
    {
        return Err("Legacy script session does not support tasks or automation".into());
    }
    if frame.version != request.version
        || frame.generation != request.generation
        || frame.session_id != request.session_id
        || frame.sequence != request.sequence
        || frame.expected_revision != request.expected_revision
    {
        return Err("Script-session reply identity or sequence mismatch".into());
    }
    if frame.console.len() > mg_sparkle::page_session::console::MAX_CONSOLE_ENTRIES_PER_TX {
        return Err("Console reply exceeds entry limit".into());
    }
    for entry in &frame.console {
        entry.validate()?;
    }
    let reply = &frame.reply;
    reply.validate_platform_deltas()?;
    if request.version == 1 && reply.has_platform_deltas() {
        return Err("Legacy script session cannot return platform deltas".into());
    }
    if let FrameCommand::Platform(cmd) = &request.command {
        cmd.validate()?;
    }
    match (&request.command, &frame.result) {
        (FrameCommand::Automation(input), Some(ModernResult::Automation(result))) => {
            result.validate_for(input)?
        }
        (FrameCommand::Automation(_), _) => {
            return Err("Automation reply omitted its matching result".into());
        }
        (FrameCommand::Console(_), Some(ModernResult::Console(result))) => result.validate()?,
        (FrameCommand::Console(_), _) => {
            return Err("Console reply omitted its matching result".into());
        }
        (_, Some(ModernResult::Page)) if request.version == VERSION => {}
        (_, None) if request.version == 1 => {}
        _ => return Err("Script command/result kind mismatch".into()),
    }
    let initial = matches!(request.command, FrameCommand::Init(_));
    if (request.version == VERSION) != reply.lifecycle.is_some()
        || (request.version == VERSION) != reply.observer.is_some()
    {
        return Err("Script reply changed its trusted lifecycle or observation lane".into());
    }
    if initial {
        reply.validate_lifecycle_initial()?;
    } else if reply.allocations.is_none() && reply.boa.is_none() {
        return Err("Later script reply omitted its engine report".into());
    }
    let expected = if initial {
        0
    } else {
        request
            .expected_revision
            .checked_add(1)
            .ok_or("Script-session revision exhausted")?
    };
    let rejected_input = !initial
        && reply.state == RealmState::Closed
        && reply.revision == request.expected_revision;
    if (!rejected_input && reply.revision != expected)
        || reply.errors.len() > 64
        || reply.errors.iter().any(|error| error.len() > 4096)
        || reply.scripts_executed > 32
        || reply.acknowledgements.len() > MAX_EDITS
    {
        return Err("Invalid script-session reply metadata".into());
    }
    if reply
        .allocations
        .as_ref()
        .is_some_and(|report| !report.is_valid() || report.limit_bytes != 4 * 1024 * 1024)
    {
        return Err("Invalid script-session allocation report".into());
    }
    if reply
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.nodes.len() > 50_000)
    {
        return Err("Script-session arena exceeds its node limit".into());
    }
    if reply.boa.as_ref().is_some_and(|report| !report.is_valid())
        || (reply.boa.is_some() && reply.allocations.is_some())
    {
        return Err("Invalid or mixed script-session engine report".into());
    }
    if reply.state == RealmState::Ready && (reply.snapshot.is_none() || !reply.accounting_valid()) {
        return Err("Ready script session omitted its arena or allocation report".into());
    }
    if reply.state == RealmState::Ready
        && reply
            .allocations
            .is_some_and(|report| report.first_rejected.is_some())
    {
        return Err("Ready script session reported a latched allocation rejection".into());
    }
    if reply.state != RealmState::Ready
        && (reply.navigation.is_some()
            || reply.default_action != DefaultAction::None
            || reply.task_wake.is_some()
            || reply.has_platform_deltas())
    {
        return Err("Closed script session proposed a default or navigation".into());
    }
    if reply.state != RealmState::Ready
        && reply
            .boa
            .as_ref()
            .is_some_and(|report| report.pending_jobs != 0)
    {
        return Err("Terminal Boa script session retained pending work".into());
    }
    if reply.task_wake.as_ref().is_some_and(|wake| wake.token == 0) {
        return Err("Invalid zero script task wake token".into());
    }
    if reply.task_wake.is_some() && (request.version != VERSION || reply.boa.is_none()) {
        return Err("Legacy script session advertised a task wake".into());
    }
    if reply.state == RealmState::Ready
        && reply
            .boa
            .as_ref()
            .is_some_and(|report| (report.pending_jobs > 0) != reply.task_wake.is_some())
    {
        return Err("Inconsistent script task wake and resource report".into());
    }
    if matches!(request.command, FrameCommand::TaskPump(_))
        && (!reply.acknowledgements.is_empty()
            || reply.default_action != DefaultAction::None
            || reply.outcome.click_canceled.is_some()
            || reply.outcome.submit_canceled.is_some()
            || reply.scripts_executed != 0)
    {
        return Err("Task pump returned activation effects or startup-script counts".into());
    }
    if matches!(
        request.command,
        FrameCommand::Automation(_) | FrameCommand::Console(_) | FrameCommand::Platform(_)
    ) && (reply.default_action != DefaultAction::None
        || reply.outcome.click_canceled.is_some()
        || reply.outcome.submit_canceled.is_some()
        || reply.scripts_executed != 0)
    {
        return Err("Automation returned activation effects or startup counts".into());
    }
    if rejected_input
        && (reply.snapshot.is_some()
            || !reply.acknowledgements.is_empty()
            || reply.outcome.click_canceled.is_some()
            || reply.outcome.submit_canceled.is_some())
    {
        return Err("Rejected script event returned effects or acknowledgements".into());
    }
    let edits = match &request.command {
        FrameCommand::Event(input) => Some(input.edits.as_slice()),
        FrameCommand::Automation(input) => Some(input.edits.as_slice()),
        _ => None,
    };
    if let Some(edits) = edits {
        if !rejected_input
            && (reply.acknowledgements.len() != edits.len()
                || reply
                    .acknowledgements
                    .iter()
                    .zip(edits)
                    .any(|(ack, edit)| ack.node != edit.node || ack.version != edit.version))
        {
            return Err("Script-session edit acknowledgement mismatch".into());
        }
    } else if !reply.acknowledgements.is_empty() {
        return Err("Initial script reply acknowledged nonexistent edits".into());
    }
    Ok(())
}
fn validate_engine_profile(
    reply: &SessionReply,
    legacy: bool,
    previous: Option<&mg_butane::modern::Report>,
) -> Result<(), String> {
    // A rejected initial document may have no realm or report. Once a realm
    // exists its parent-selected engine cannot change, even on terminal replies.
    let rejected_initial = previous.is_none() && reply.is_rejected_initial();
    if !rejected_initial && (reply.boa.is_some() == legacy) {
        return Err("Script session returned the wrong engine profile".into());
    }
    if let Some(previous) = previous {
        reply
            .boa
            .as_ref()
            .ok_or("Script session lost its Boa resource report")?
            .validate_after(previous)
            .map_err(str::to_owned)?;
    }
    if let Some(report) = &reply.boa
        && report.worker_memory.is_none()
    {
        return Err("Boa worker omitted process allocation accounting".into());
    }
    Ok(())
}

fn validate_observer_mode(
    expected: &ObserverInit,
    reply: &SessionReply,
    legacy: bool,
) -> Result<(), String> {
    if legacy {
        return if matches!(expected, ObserverInit::Disabled) && reply.observer.is_none() {
            Ok(())
        } else {
            Err("Legacy v1 cannot carry observation".into())
        };
    }
    reply
        .validate_observer_frame(expected.expectation())
        .map_err(|error| error.to_string())?;
    match (expected, reply.observer.as_ref()) {
        (ObserverInit::Disabled, Some(ObserverReply::Disabled)) => Ok(()),
        (
            ObserverInit::WindowHarness { run_id, .. },
            Some(
                ObserverReply::WindowHarness { run_id: got, .. }
                | ObserverReply::ObservationFailed { run_id: got, .. },
            ),
        ) if *run_id != 0 && run_id == got => Ok(()),
        _ => Err("Script observation does not match trusted mode/run".into()),
    }
}

fn validate_cumulative(
    previous: Option<AllocationReport>,
    next: Option<AllocationReport>,
) -> Result<(), String> {
    let Some(previous) = previous else {
        return Ok(());
    };
    let next = next.ok_or("Script session lost its cumulative allocation report")?;
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
    if values(previous)
        .iter()
        .zip(values(next))
        .any(|(old, new)| new < *old)
        || previous
            .first_rejected
            .is_some_and(|old| next.first_rejected != Some(old))
    {
        return Err("Script-session cumulative allocation report moved backward".into());
    }
    Ok(())
}
fn add_wire(total: &mut usize, bytes: usize) -> Result<(), String> {
    *total = total
        .checked_add(bytes)
        .filter(|total| *total <= WIRE_LIMIT)
        .ok_or("Script session exceeded its 32 MiB combined wire limit")?;
    Ok(())
}
fn check_payload(length: usize, limit: usize, total: usize) -> Result<(), String> {
    if length == 0 || length > limit {
        return Err("Script-session frame payload exceeds its limit or is empty".into());
    }
    if length > WIRE_LIMIT.saturating_sub(total) {
        return Err("Script session exceeded its 32 MiB combined wire limit".into());
    }
    Ok(())
}
fn read_frame(
    reader: &mut impl Read,
    total: &mut usize,
    limit: usize,
) -> Result<Option<Vec<u8>>, String> {
    let mut header = [0; 4];
    let mut filled = 0;
    while filled < 4 {
        match reader.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err("Truncated script-session request header".into()),
            Ok(bytes) => {
                add_wire(total, bytes)?;
                filled += bytes;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let length = u32::from_be_bytes(header) as usize;
    check_payload(length, limit, *total)?;
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .map_err(|_| "Truncated script-session request payload")?;
    add_wire(total, length)?;
    Ok(Some(payload))
}
fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
    total: &mut usize,
) -> Result<(), String> {
    let output = encode(value, OUTPUT_LIMIT)?;
    if output.len().saturating_add(4) > WIRE_LIMIT.saturating_sub(*total) {
        return Err("Script session exceeded its 32 MiB combined wire limit".into());
    }
    writer
        .write_all(&(output.len() as u32).to_be_bytes())
        .map_err(|error| error.to_string())?;
    writer
        .write_all(&output)
        .map_err(|error| error.to_string())?;
    writer.flush().map_err(|error| error.to_string())?;
    add_wire(total, output.len() + 4)
}

/// Early executable entrypoint, before fonts/display/network are initialized.
pub fn session_entry() -> ! {
    session_entry_for(false)
}
#[cfg(feature = "legacy-test-engine")]
pub fn legacy_session_entry() -> ! {
    session_entry_for(true)
}
fn session_entry_for(legacy: bool) -> ! {
    if install_isolation().is_err() {
        std::process::exit(73);
    }
    if std::env::args().len() != 2 {
        std::process::exit(74);
    }
    if !legacy {
        crate::platform::worker_memory::activate();
    }
    let result = child_loop_for(&mut io::stdin().lock(), &mut io::stdout().lock(), legacy);
    std::process::exit(if result.is_ok() { 0 } else { 74 })
}
enum PageEngine {
    Boa(mg_sparkle::js_browser::boa::BoaPageRealm),
    #[cfg(feature = "legacy-test-engine")]
    Legacy(mg_sparkle::js_browser::PageRealm),
}
impl PageEngine {
    fn start(
        request: Request,
        legacy: bool,
        observer: ObserverInit,
        platform: PlatformInit,
        crypto_seed: [u8; 32],
    ) -> (Option<Self>, SessionReply) {
        #[cfg(feature = "legacy-test-engine")]
        if legacy {
            let (realm, reply) = mg_sparkle::js_browser::PageRealm::start(request);
            return (realm.map(Self::Legacy), reply);
        }
        let _ = legacy;
        let (realm, reply) = if crypto_seed == [0u8; 32] && platform.is_default() {
            mg_sparkle::js_browser::boa::BoaPageRealm::start_with_observer(request, observer)
        } else {
            let seed = if crypto_seed == [0u8; 32] {
                let mut s = [0x5a_u8; 32];
                s[0] = 1;
                s
            } else {
                crypto_seed
            };
            mg_sparkle::js_browser::boa::BoaPageRealm::start_with_platform(
                request, observer, platform, seed,
            )
        };
        (realm.map(Self::Boa), reply)
    }
    fn dispatch(&mut self, input: SessionInput) -> SessionReply {
        match self {
            Self::Boa(realm) => realm.dispatch(input),
            #[cfg(feature = "legacy-test-engine")]
            Self::Legacy(realm) => realm.dispatch(input),
        }
    }
    fn pump_tasks(&mut self, pump: TaskPump) -> Result<SessionReply, String> {
        match self {
            Self::Boa(realm) => Ok(realm.pump_tasks(pump)),
            #[cfg(feature = "legacy-test-engine")]
            Self::Legacy(_) => Err("The legacy test runtime does not support task pumping".into()),
        }
    }
    fn automation(
        &mut self,
        request: AutomationRequest,
    ) -> Result<(SessionReply, AutomationResult), String> {
        match self {
            Self::Boa(realm) => Ok(realm.automation(request)),
            #[cfg(feature = "legacy-test-engine")]
            Self::Legacy(_) => Err("Legacy v1 cannot execute automation".into()),
        }
    }
    fn console_command(
        &mut self,
        command: ConsoleCommand,
    ) -> Result<(SessionReply, ConsoleResult), String> {
        match self {
            Self::Boa(realm) => Ok(realm.console_command(command)),
            #[cfg(feature = "legacy-test-engine")]
            Self::Legacy(_) => Err("Legacy v1 cannot execute console".into()),
        }
    }
    fn platform_command(&mut self, command: PlatformCommand) -> Result<SessionReply, String> {
        match self {
            Self::Boa(realm) => Ok(realm.platform_command(command)),
            #[cfg(feature = "legacy-test-engine")]
            Self::Legacy(_) => Err("Legacy v1 cannot execute platform commands".into()),
        }
    }
    fn take_console_entries(&mut self) -> Vec<ConsoleWireEntry> {
        match self {
            Self::Boa(realm) => realm.take_console_entries(),
            #[cfg(feature = "legacy-test-engine")]
            Self::Legacy(_) => Vec::new(),
        }
    }
}
fn attach_memory(reply: &mut SessionReply) {
    if let Some(report) = &mut reply.boa {
        report.worker_memory = Some(crate::platform::worker_memory::report());
    }
}
fn child_loop_for(
    reader: &mut impl Read,
    writer: &mut impl Write,
    legacy: bool,
) -> Result<(), String> {
    let version = if legacy { 1 } else { VERSION };
    let mut wire = 0;
    let Some(payload) = read_frame(reader, &mut wire, INPUT_LIMIT)? else {
        return Err("Session initialization missing".into());
    };
    let (request, observer, platform, crypto_seed) = decode_request_frame_full(&payload, legacy)?;
    if request.version != version
        || request.sequence != 0
        || request.expected_revision != 0
        || request.session_id == 0
    {
        return Err("Invalid session initialization envelope".into());
    }
    let FrameCommand::Init(initial) = request.command else {
        return Err("First session command must be Init".into());
    };
    let automation_enabled = !legacy && matches!(observer, ObserverInit::Disabled);
    let (mut realm, mut reply) =
        PageEngine::start(initial, legacy, observer, platform, crypto_seed);
    let initial_console = if !legacy && matches!(observer, ObserverInit::Disabled) {
        realm
            .as_mut()
            .map(PageEngine::take_console_entries)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    attach_memory(&mut reply);
    let mut revision = reply.revision;
    let mut lifecycle = reply.lifecycle;
    let mut terminal = reply.state != RealmState::Ready;
    write_reply_frame(
        writer,
        &ReplyFrame {
            version,
            generation: request.generation,
            session_id: request.session_id,
            sequence: 0,
            expected_revision: 0,
            result: (!legacy).then_some(ModernResult::Page),
            console: initial_console,
            reply,
        },
        &mut wire,
        legacy,
    )?;
    for sequence in 1..TRANSACTIONS {
        if terminal {
            return Ok(());
        }
        let frame_limit = if automation_enabled {
            INPUT_LIMIT
        } else {
            MAX_EVENT_BYTES
        };
        let Some(payload) = read_frame(reader, &mut wire, frame_limit)? else {
            return Ok(());
        };
        let (event, _, _, _) = decode_request_frame_full(&payload, legacy)?;
        let event_limit = match event.command {
            FrameCommand::Platform(_) => INPUT_LIMIT,
            FrameCommand::Automation(_) if automation_enabled => MAX_AUTOMATION_FRAME_BYTES,
            _ => MAX_EVENT_BYTES,
        };
        if payload.len() > event_limit {
            return Err("Script-session event payload exceeds 64 KiB limit".into());
        }
        if event.version != version
            || event.generation != request.generation
            || event.session_id != request.session_id
            || event.sequence != sequence
            || event.expected_revision != revision
        {
            return Err("Invalid script-session event envelope".into());
        }
        let realm = realm.as_mut().ok_or("Script realm is unavailable")?;
        let (mut reply, result) = match event.command {
            FrameCommand::Event(input) => {
                validate_input(&input)?;
                (
                    realm.dispatch(input),
                    (!legacy).then_some(ModernResult::Page),
                )
            }
            FrameCommand::TaskPump(pump) if !legacy && pump.wake_token != 0 => {
                (realm.pump_tasks(pump)?, Some(ModernResult::Page))
            }
            FrameCommand::Automation(input)
                if automation_enabled
                    && lifecycle.is_some_and(|progress| {
                        progress.phase == mg_sparkle::page_session::LifecyclePhase::LoadComplete
                    }) =>
            {
                input.validate()?;
                let (reply, result) = realm.automation(input)?;
                (reply, Some(ModernResult::Automation(result)))
            }
            FrameCommand::Console(command)
                if automation_enabled
                    && lifecycle.is_some_and(|progress| {
                        progress.phase == mg_sparkle::page_session::LifecyclePhase::LoadComplete
                    }) =>
            {
                command.validate()?;
                let (reply, result) = realm.console_command(command)?;
                (reply, Some(ModernResult::Console(result)))
            }
            FrameCommand::Platform(command) if automation_enabled => {
                command.validate()?;
                (realm.platform_command(command)?, Some(ModernResult::Page))
            }
            _ => return Err("Repeated initialization or unsupported task command".into()),
        };
        let tx_console = if automation_enabled {
            realm.take_console_entries()
        } else {
            Vec::new()
        };
        attach_memory(&mut reply);
        revision = reply.revision;
        lifecycle = reply.lifecycle;
        terminal = reply.state != RealmState::Ready;
        write_reply_frame(
            writer,
            &ReplyFrame {
                version,
                generation: request.generation,
                session_id: request.session_id,
                sequence,
                expected_revision: event.expected_revision,
                result,
                console: tx_console,
                reply,
            },
            &mut wire,
            legacy,
        )?;
    }
    // Reaching the fixed transaction budget ends the original realm, never resets it.
    Ok(())
}

/// Actual manager/child checks. Only called in the normal browser executable;
/// unit tests below never spawn their own binary test harness as a worker.
pub fn session_selftest() -> Result<(), String> {
    use mg_sparkle::page_session::InputKind;
    fn request() -> Request {
        Request { external_scripts: None, url: "https://example.test/session-selftest".into(), html:
            "<button id=b type=button>B</button><p id=o></p><script>var calls=0;document.getElementById('b').onclick=function(){calls++;document.getElementById('o').setAttribute('data-count',String(calls));};</script>".into() }
    }
    fn wait(session: &mut Session) -> Result<SessionUpdate, String> {
        let deadline = Instant::now() + WALL_LIMIT;
        loop {
            if let Some(update) = session.try_recv() {
                return Ok(update);
            }
            if Instant::now() >= deadline {
                return Err("Session selftest completion timed out".into());
            }
            thread::sleep(TICK);
        }
    }
    fn reaped(pid: u32) -> Result<(), String> {
        let mut status = 0;
        // Exact owned child PID, after manager cleanup; never waits on another.
        let result = unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) };
        if result != -1 || io::Error::last_os_error().raw_os_error() != Some(libc::ECHILD) {
            return Err("Session selftest child was not already reaped".into());
        }
        Ok(())
    }
    fn empty(pool: &ChildPool) -> Result<(), String> {
        if pool.inner.lock().map_err(|_| "Pool unavailable")?.owned != 0 {
            return Err("Session selftest leaked a child permit".into());
        }
        Ok(())
    }
    let pool = ChildPool::new();
    pool.set_generation(1)?;
    let (mut session, initial) = Session::start(request(), 1, pool.clone())?;
    if initial.state != RealmState::Ready || !initial.errors.is_empty() {
        return Err("Session selftest startup failed".into());
    }
    let target = initial
        .snapshot
        .as_ref()
        .ok_or("Initial arena missing")?
        .nodes
        .iter()
        .position(|node| node.attr("id") == Some("b"))
        .ok_or("Selftest button missing")?;
    let event = || SessionInput {
        kind: InputKind::Click { target },
        edits: Vec::new(),
    };
    if session.try_dispatch(event(), 0).is_ok() {
        return Err("Unvalidated projection admitted new input".into());
    }
    session.charge_active(Duration::ZERO)?;
    let first = session.try_dispatch(event(), 0)?;
    if session.try_dispatch(event(), 0).is_ok() {
        return Err("Session accepted a second in-flight command".into());
    }
    let first_update = wait(&mut session)?;
    if first_update.sequence != Some(first) {
        return Err("Session sequence mismatch".into());
    }
    let first_reply = first_update.reply?;
    if !first_reply
        .snapshot
        .as_ref()
        .ok_or("Event arena missing")?
        .nodes
        .iter()
        .any(|node| node.attr("data-count") == Some("1"))
    {
        return Err("Retained handler did not execute".into());
    }
    session.charge_active(Duration::ZERO)?;
    let second = session.try_dispatch(event(), 1)?;
    let second_update = wait(&mut session)?;
    if second_update.sequence != Some(second) {
        return Err("Session sequence mismatch".into());
    }
    let second_reply = second_update.reply?;
    if !second_reply
        .snapshot
        .as_ref()
        .ok_or("Event arena missing")?
        .nodes
        .iter()
        .any(|node| node.attr("data-count") == Some("2"))
    {
        return Err("Session replayed or lost retained state".into());
    }
    session.charge_active(Duration::ZERO)?;
    let pid = session.shared.pid.load(Ordering::Acquire);
    drop(session);
    reaped(pid)?;
    empty(&pool)?;
    println!(
        "SCRIPT_SESSION_CHECK retained: two real callbacks; projection acknowledgement and single-flight enforced; drop reaped"
    );

    if !super::super::legacy_test_mode() {
        // This runs in the actual normal executable, not a Rust test-harness
        // re-exec. Reuse exactly the same manager, validation slot and ledger.
        let (mut automation, mut reply) = Session::start(
            Request {
                external_scripts: None,
                url: "https://example.test/automation-selftest".into(),
                html: "<p>bounded</p>".into(),
            },
            1,
            pool.clone(),
        )?;
        let evaluate = || AutomationRequest {
            owner: 1,
            operation: mg_sparkle::page_session::automation::Operation::Evaluate {
                source: "1".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        };
        if automation.try_automation(evaluate(), 0).is_ok() {
            return Err("Automation bypassed initial-load/validation admission".into());
        }
        automation.charge_active(Duration::ZERO)?;
        for sequence in 1..=2 {
            let wake = reply
                .task_wake
                .as_ref()
                .ok_or("Automation lifecycle wake missing")?
                .token;
            automation.try_task_pump(TaskPump { wake_token: wake }, reply.revision)?;
            let completion = wait(&mut automation)?;
            if completion.sequence != Some(sequence)
                || completion.automation.is_some()
                || completion.continuation != SessionContinuation::Live
            {
                return Err("Automation lifecycle completion metadata mismatch".into());
            }
            reply = completion.reply?;
            automation.charge_active(Duration::ZERO)?;
        }
        for sequence in 3..=63 {
            let previous = reply.boa.clone().ok_or("Automation report missing")?;
            if automation.try_automation(evaluate(), reply.revision)? != sequence {
                return Err("Automation transaction sequence changed".into());
            }
            if automation
                .try_automation(evaluate(), reply.revision)
                .is_ok()
            {
                return Err("Automation admitted concurrent work".into());
            }
            let completion = wait(&mut automation)?;
            let expected = if sequence == 63 {
                SessionContinuation::Terminal
            } else {
                SessionContinuation::Live
            };
            if completion.sequence != Some(sequence)
                || completion.continuation != expected
                || completion.automation
                    != Some(AutomationResult::Value(
                        mg_sparkle::page_session::automation::RemoteValue::Number(
                            mg_sparkle::page_session::automation::Number::Finite(1.0),
                        ),
                    ))
            {
                return Err("Automation result/continuation publication mismatch".into());
            }
            reply = completion.reply?;
            if reply.state != RealmState::Ready {
                return Err("Primitive final completion lost its Ready snapshot".into());
            }
            reply
                .boa
                .as_ref()
                .ok_or("Automation report missing")?
                .validate_after(&previous)
                .map_err(str::to_owned)?;
            if automation
                .try_automation(evaluate(), reply.revision)
                .is_ok()
            {
                return Err("Automation bypassed pending validation".into());
            }
            automation.charge_active(Duration::ZERO)?;
            if automation.charge_active(Duration::ZERO).is_ok() {
                return Err("Automation completion accepted two charges".into());
            }
        }
        if automation
            .try_automation(evaluate(), reply.revision)
            .is_ok()
        {
            return Err("Automation renewed the 64-transaction allowance".into());
        }
        let pid = automation.shared.pid.load(Ordering::Acquire);
        drop(automation);
        reaped(pid)?;
        empty(&pool)?;
        println!(
            "SCRIPT_SESSION_CHECK automation: retained actual primitive results; shared single-flight/one-charge; sequence63 terminal and child reaped"
        );

        let timed = Request {
            external_scripts: None,
            url: "https://example.test/task-selftest".into(),
            html: "<p id=o data-count=0></p><script>var n=0;var canceled=setTimeout(function(){throw Error('canceled timer ran');},0);clearTimeout(canceled);function task(){n++;document.getElementById('o').setAttribute('data-count',String(n));Promise.resolve().then(function(){document.getElementById('o').setAttribute('data-micro',String(n));});}setTimeout(task,0);setTimeout(task,0);</script>".into(),
        };
        let (mut session, mut reply) = Session::start(timed, 1, pool.clone())?;
        let mut received = Instant::now();
        if reply.state != RealmState::Ready
            || !reply.errors.is_empty()
            || reply.boa.as_ref().map(|report| report.pending_jobs) != Some(4)
            || !reply.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .nodes
                    .iter()
                    .any(|node| node.attr("data-count") == Some("0"))
            })
        {
            return Err("Task selftest startup ran or lost a timer".into());
        }
        let first_wake = reply.task_wake.clone().ok_or("Task wake missing")?;
        if session
            .try_task_pump(
                TaskPump {
                    wake_token: first_wake.token,
                },
                0,
            )
            .is_ok()
        {
            return Err("Task bypassed initial projection acknowledgement".into());
        }
        session.charge_active(Duration::ZERO)?;
        for revision in 0..2 {
            let wake = reply
                .task_wake
                .clone()
                .ok_or("Retained task wake missing")?;
            let due = received
                .checked_add(Duration::from_millis(u64::from(wake.delay_ms)))
                .ok_or("Task selftest clock overflow")?;
            while Instant::now() < due {
                thread::sleep(TICK);
            }
            if session.try_recv().is_some() {
                return Err("Idle worker emitted an unsolicited task reply".into());
            }
            let sequence = session.try_task_pump(
                TaskPump {
                    wake_token: wake.token,
                },
                revision,
            )?;
            if session.try_dispatch(event(), revision).is_ok()
                || session
                    .try_task_pump(
                        TaskPump {
                            wake_token: wake.token,
                        },
                        revision,
                    )
                    .is_ok()
            {
                return Err("Task bypassed activation/task shared backpressure".into());
            }
            let update = wait(&mut session)?;
            received = Instant::now();
            let next = update.reply?;
            if update.sequence != Some(sequence)
                || next.revision != revision + 1
                || next.state != RealmState::Ready
                || !next.errors.is_empty()
                || !next.acknowledgements.is_empty()
                || next.default_action != DefaultAction::None
                || next.outcome.click_canceled.is_some()
                || next.outcome.submit_canceled.is_some()
                || next.scripts_executed != 0
                || !next.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.nodes.iter().any(|node| {
                        node.attr("data-count") == Some((revision + 1).to_string().as_str())
                            && node.attr("data-micro") == Some((revision + 1).to_string().as_str())
                    })
                })
            {
                return Err("Task or its checkpoint returned invalid effects".into());
            }
            next.boa
                .as_ref()
                .ok_or("Task resource report missing")?
                .validate_after(reply.boa.as_ref().ok_or("Previous task report missing")?)?;
            if session
                .try_task_pump(
                    TaskPump {
                        wake_token: wake.token,
                    },
                    revision + 1,
                )
                .is_ok()
            {
                return Err("Task bypassed projection acknowledgement or replayed its wake".into());
            }
            session.charge_active(Duration::ZERO)?;
            if session.charge_active(Duration::ZERO).is_ok() {
                return Err("Task projection was acknowledged twice".into());
            }
            reply = next;
        }
        // Both source timers precede the mandatory lifecycle work. Complete
        // exactly those two milestones, not an arbitrary task drain, using
        // the same single-flight and one-charge acknowledgement boundary.
        for (revision, phase) in [
            (
                2,
                mg_sparkle::page_session::LifecyclePhase::DomContentLoaded,
            ),
            (3, mg_sparkle::page_session::LifecyclePhase::LoadComplete),
        ] {
            let wake = reply.task_wake.as_ref().ok_or("Lifecycle wake missing")?;
            let due = received
                .checked_add(Duration::from_millis(u64::from(wake.delay_ms)))
                .ok_or("Lifecycle selftest clock overflow")?;
            while Instant::now() < due {
                thread::sleep(TICK);
            }
            let sequence = session.try_task_pump(
                TaskPump {
                    wake_token: wake.token,
                },
                revision,
            )?;
            let update = wait(&mut session)?;
            received = Instant::now();
            let next = update.reply?;
            if update.sequence != Some(sequence)
                || next.revision != revision + 1
                || next.state != RealmState::Ready
                || !next.errors.is_empty()
                || !next.acknowledgements.is_empty()
                || next.default_action != DefaultAction::None
                || next.outcome.click_canceled.is_some()
                || next.outcome.submit_canceled.is_some()
                || next.scripts_executed != 0
                || next
                    .validate_lifecycle_after(reply.lifecycle, true)?
                    .map(|progress| progress.phase)
                    != Some(phase)
                || !next.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.nodes.iter().any(|node| {
                        node.attr("data-count") == Some("2") && node.attr("data-micro") == Some("2")
                    })
                })
            {
                return Err(
                    "Lifecycle selftest changed timer effects or fabricated a milestone".into(),
                );
            }
            next.boa
                .as_ref()
                .ok_or("Lifecycle resource report missing")?
                .validate_after(
                    reply
                        .boa
                        .as_ref()
                        .ok_or("Previous lifecycle report missing")?,
                )?;
            session.charge_active(Duration::ZERO)?;
            if session.charge_active(Duration::ZERO).is_ok() {
                return Err("Lifecycle projection was acknowledged twice".into());
            }
            reply = next;
        }
        if reply.task_wake.is_some()
            || reply.boa.as_ref().map(|report| report.pending_jobs) != Some(0)
        {
            return Err("Completed task session retained a phantom wake".into());
        }
        let pid = session.shared.pid.load(Ordering::Acquire);
        drop(session);
        reaped(pid)?;
        empty(&pool)?;
        println!(
            "SCRIPT_SESSION_CHECK tasks: parent-issued pumps; one task plus checkpoint; shared single-flight/projection charge; canceled timer absent; separate DCL/load; monotonic reports; drop reaped"
        );
    }

    let (first, _) = Session::start(request(), 1, pool.clone())?;
    let (second, _) = Session::start(request(), 1, pool.clone())?;
    if Session::start(request(), 1, pool.clone()).is_ok() {
        return Err("Pool admitted a third child".into());
    }
    let pids = [
        first.shared.pid.load(Ordering::Acquire),
        second.shared.pid.load(Ordering::Acquire),
    ];
    drop(first);
    drop(second);
    for pid in pids {
        reaped(pid)?;
    }
    empty(&pool)?;
    println!("SCRIPT_SESSION_CHECK pool: at most two children; permits released only after reap");

    let (mut session, _) = Session::start(request(), 1, pool.clone())?;
    let pid = session.shared.pid.load(Ordering::Acquire);
    pool.set_generation(2)?;
    if wait(&mut session)?.reply.is_ok() {
        return Err("Superseded session survived generation change".into());
    }
    reaped(pid)?;
    drop(session);
    empty(&pool)?;
    if Session::start(request(), 1, pool.clone()).is_ok() {
        return Err("Stale generation started a worker".into());
    }
    println!(
        "SCRIPT_SESSION_CHECK generation: superseded idle child reaped; stale startup rejected"
    );

    let (mut session, _) = Session::start(request(), 2, pool.clone())?;
    session.charge_active(Duration::from_secs(1))?;
    session.try_dispatch(event(), 0)?;
    let reply = wait(&mut session)?.reply?;
    if reply.revision != 1 {
        return Err("Active-budget event failed".into());
    }
    let pid = session.shared.pid.load(Ordering::Acquire);
    if session.charge_active(Duration::from_secs(1)).is_ok() {
        return Err("Parent validation renewed active wall budget".into());
    }
    if wait(&mut session)?.reply.is_ok() {
        return Err("Exhausted active budget did not close".into());
    }
    reaped(pid)?;
    drop(session);
    empty(&pool)?;
    println!(
        "SCRIPT_SESSION_CHECK active: cumulative parent-validation admission rejects renewal; child reaped"
    );

    let (mut session, _) = Session::start(request(), 2, pool.clone())?;
    session.charge_active(Duration::ZERO)?;
    let pid = session.shared.pid.load(Ordering::Acquire);
    session.try_dispatch(event(), 0)?;
    session.cancel();
    if wait(&mut session)?.reply.is_ok() {
        return Err("Canceled activation returned a usable reply".into());
    }
    reaped(pid)?;
    drop(session);
    empty(&pool)?;
    println!("SCRIPT_SESSION_CHECK cancel: pending activation canceled and child reaped");

    let (mut session, _) = Session::start(request(), 2, pool.clone())?;
    session.charge_active(Duration::ZERO)?;
    for revision in 0..63 {
        session.try_dispatch(event(), revision)?;
        let reply = wait(&mut session)?.reply?;
        if reply.revision != revision + 1 {
            return Err("Transaction count/revision mismatch".into());
        }
        session.charge_active(Duration::ZERO)?;
    }
    if session.try_dispatch(event(), 63).is_ok() {
        return Err("Session exceeded 64 total transactions".into());
    }
    let pid = session.shared.pid.load(Ordering::Acquire);
    drop(session);
    reaped(pid)?;
    empty(&pool)?;
    println!(
        "SCRIPT_SESSION_CHECK transactions: initialization plus63 events; final reply drained; child reaped"
    );
    let (first, _) = Session::start(request(), 2, pool.clone())?;
    let (second, _) = Session::start(request(), 2, pool.clone())?;
    let pids = [
        first.shared.pid.load(Ordering::Acquire),
        second.shared.pid.load(Ordering::Acquire),
    ];
    pool.close();
    pool.wait_idle()?;
    for pid in pids {
        reaped(pid)?;
    }
    if pool.set_generation(3).is_ok() || Session::start(request(), 3, pool.clone()).is_ok() {
        return Err("Closed pool reopened or admitted a late startup".into());
    }
    drop(first);
    drop(second);
    empty(&pool)?;
    println!(
        "SCRIPT_SESSION_CHECK shutdown: permanent pool close; both children reaped; late starts rejected"
    );
    println!("SCRIPT_SESSION_SELFTEST_OK");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mg_butane::runtime::{AllocationPhase, AllocationTotals, RejectedAllocation};
    use mg_sparkle::page_session::{
        ArenaSnapshot, ControlAck, ControlEdit, EventOutcome, InputKind,
    };
    use std::io::Cursor;

    fn report(runtime: u64) -> AllocationReport {
        AllocationReport {
            limit_bytes: 4 * 1024 * 1024,
            accepted_bytes: runtime,
            phases: AllocationTotals {
                runtime,
                ..AllocationTotals::default()
            },
            first_rejected: None,
        }
    }
    fn request(sequence: u64) -> RequestFrame {
        RequestFrame {
            version: 1,
            generation: 7,
            session_id: 8,
            sequence,
            expected_revision: sequence.saturating_sub(1),
            command: if sequence == 0 {
                FrameCommand::Init(Request {
                    external_scripts: None,
                    url: "https://example.test/".into(),
                    html: "<p>owned</p>".into(),
                })
            } else {
                FrameCommand::Event(SessionInput {
                    kind: InputKind::Click { target: 1 },
                    edits: Vec::new(),
                })
            },
        }
    }
    fn response(request: &RequestFrame) -> ReplyFrame {
        ReplyFrame {
            version: request.version,
            generation: request.generation,
            session_id: request.session_id,
            sequence: request.sequence,
            expected_revision: request.expected_revision,
            result: None,
            console: Vec::new(),
            reply: SessionReply {
                revision: request.sequence,
                snapshot: Some(ArenaSnapshot { nodes: Vec::new() }),
                outcome: EventOutcome::default(),
                default_action: DefaultAction::None,
                navigation: None,
                errors: Vec::new(),
                scripts_executed: 1,
                allocations: Some(report(10)),
                boa: None,
                state: RealmState::Ready,
                acknowledgements: Vec::new(),
                task_wake: None,
                lifecycle: None,
                observer: None,
                local_storage_ops: Vec::new(),
                session_storage_ops: Vec::new(),
                cookie_writes: Vec::new(),
                history_ops: Vec::new(),
                fetch_requests: Vec::new(),
                scroll_request: None,
            },
        }
    }

    fn modern_response(request: &RequestFrame) -> ReplyFrame {
        let mut frame = response(request);
        frame.reply.allocations = None;
        frame.reply.observer = Some(ObserverReply::Disabled);
        frame.result = Some(ModernResult::Page);
        frame.reply.boa = Some(mg_butane::modern::Engine::new().unwrap().report());
        let initial = matches!(request.command, FrameCommand::Init(_));
        frame.reply.lifecycle = Some(LifecycleProgress {
            phase: if initial {
                mg_sparkle::page_session::LifecyclePhase::Interactive
            } else {
                mg_sparkle::page_session::LifecyclePhase::LoadComplete
            },
            initial_failure: false,
        });
        if initial {
            // This wire fixture represents the two admitted mandatory tasks,
            // not extra allowance or two already executed callbacks.
            let report = frame.reply.boa.as_mut().unwrap();
            report.jobs_admitted = 2;
            report.pending_jobs = 2;
            frame.reply.task_wake = Some(TaskWake {
                token: 1,
                delay_ms: 0,
            });
        }
        frame
    }

    fn write_request(output: &mut Vec<u8>, request: &RequestFrame, legacy: bool) {
        let payload = encode_request_frame(request, legacy, INPUT_LIMIT).unwrap();
        output.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        output.extend_from_slice(&payload);
    }

    fn automation_request() -> RequestFrame {
        let mut frame = request(3);
        frame.version = VERSION;
        frame.command = FrameCommand::Automation(AutomationRequest {
            owner: 1,
            operation: mg_sparkle::page_session::automation::Operation::Evaluate {
                source: "1".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
            edits: vec![],
        });
        frame
    }

    #[test]
    fn modern_v8_result_is_required_non_null_strict_and_command_matched() {
        let mut page = request(0);
        page.version = VERSION;
        let request = automation_request();
        let mut frame = modern_response(&request);
        frame.reply.scripts_executed = 0;
        frame.result = Some(ModernResult::Automation(AutomationResult::Value(
            mg_sparkle::page_session::automation::RemoteValue::Null,
        )));
        validate_reply(&request, &frame).unwrap();
        let raw = serde_json::to_value(&frame).unwrap();
        for value in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({"kind":"Automation","payload":null}),
            serde_json::json!({"kind":"Page","payload":null}),
            serde_json::json!({"kind":"Page","extra":true}),
        ] {
            let mut bad = raw.clone();
            bad["result"] = value;
            assert!(
                decode_reply_frame(&serde_json::to_vec(&bad).unwrap(), false).is_err(),
                "Forbidden result shape accepted: {}",
                bad["result"]
            );
        }
        let mut missing = raw.clone();
        missing.as_object_mut().unwrap().remove("result");
        assert!(decode_reply_frame(&serde_json::to_vec(&missing).unwrap(), false).is_err());
        for duplicate in [
            r#"{"kind":"Page","kind":"Page"}"#,
            r#"{"kind":"Automation","payload":"Released","payload":"Released"}"#,
        ] {
            assert!(serde_json::from_str::<ModernResult>(duplicate).is_err());
        }
        frame.result = Some(ModernResult::Page);
        assert!(validate_reply(&request, &frame).is_err());
        frame.result = Some(ModernResult::Automation(AutomationResult::Released));
        assert!(validate_reply(&request, &frame).is_err());
        assert!(validate_reply(&page, &frame).is_err());
    }

    #[test]
    fn legacy_command_and_reply_vocabulary_never_accepts_automation_even_null() {
        let frame = automation_request();
        assert!(encode_request_frame(&frame, true, MAX_EVENT_BYTES).is_err());
        let mut raw = serde_json::to_value(&frame).unwrap();
        raw["version"] = serde_json::json!(1);
        assert!(decode_request_frame(&serde_json::to_vec(&raw).unwrap(), true).is_err());
        raw["command"]["payload"] = serde_json::Value::Null;
        assert!(decode_request_frame(&serde_json::to_vec(&raw).unwrap(), true).is_err());
        let old = response(&request(0));
        let mut raw = serde_json::to_value(&old).unwrap();
        for value in [serde_json::Value::Null, serde_json::json!({"kind":"Page"})] {
            raw["result"] = value;
            assert!(decode_reply_frame(&serde_json::to_vec(&raw).unwrap(), true).is_err());
        }
    }

    #[test]
    fn automation_edits_require_exact_acks_and_cannot_supply_activation_effects() {
        let mut request = automation_request();
        let FrameCommand::Automation(input) = &mut request.command else {
            unreachable!()
        };
        input.edits.push(ControlEdit {
            node: 4,
            version: 9,
            value: "Unicode café".into(),
        });
        let mut frame = modern_response(&request);
        frame.reply.scripts_executed = 0;
        frame.result = Some(ModernResult::Automation(AutomationResult::Rejected {
            diagnostic: "representation unavailable".into(),
        }));
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.acknowledgements.push(ControlAck {
            node: 4,
            version: 9,
        });
        validate_reply(&request, &frame).unwrap();
        frame.reply.acknowledgements[0].version = 8;
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.acknowledgements[0].version = 9;
        frame.reply.outcome.click_canceled = Some(false);
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.outcome.click_canceled = None;
        frame.reply.scripts_executed = 1;
        assert!(validate_reply(&request, &frame).is_err());
    }

    #[test]
    fn automation_uses_existing_frame_counting_and_narrower_operation_edit_limits() {
        use mg_sparkle::page_session::automation::{
            MAX_EDITS_BYTES, MAX_OPERATION_BYTES, Operation, encoded_size,
        };
        let mut frame = automation_request();
        frame.generation = u64::MAX;
        frame.session_id = u64::MAX;
        frame.expected_revision = u64::MAX;
        let FrameCommand::Automation(input) = &mut frame.command else {
            unreachable!()
        };
        let base = encoded_size(&input.operation, MAX_OPERATION_BYTES).unwrap() - 1;
        let padding = MAX_OPERATION_BYTES - base;
        input.operation = Operation::Evaluate {
            source: format!("{}{}", "\0".repeat(padding / 6), "x".repeat(padding % 6)),
            return_by_value: false,
            group: None,
            world: None,
            await_promise: false,
            user_gesture: false,
        };
        assert_eq!(
            encoded_size(&input.operation, MAX_OPERATION_BYTES).unwrap(),
            MAX_OPERATION_BYTES
        );
        input.edits = (1..=3)
            .map(|node| ControlEdit {
                node,
                version: u64::MAX,
                value: String::new(),
            })
            .collect();
        let mut left = MAX_EDITS_BYTES - encoded_size(&input.edits, MAX_EDITS_BYTES).unwrap();
        for edit in &mut input.edits {
            let n = left.min(MAX_EDIT_BYTES);
            edit.value = "x".repeat(n);
            left -= n;
        }
        assert_eq!(left, 0);
        input.validate().unwrap();
        let bytes = encode_request_frame(&frame, false, MAX_AUTOMATION_FRAME_BYTES).unwrap();
        assert!(bytes.len() <= MAX_AUTOMATION_FRAME_BYTES);
        assert_eq!(
            encode_request_frame(&frame, false, bytes.len()).unwrap(),
            bytes
        );
        assert!(encode_request_frame(&frame, false, bytes.len() - 1).is_err());
        let FrameCommand::Automation(input) = &mut frame.command else {
            unreachable!()
        };
        if let Operation::Evaluate { source, .. } = &mut input.operation {
            source.push('x');
        }
        assert!(input.validate().is_err());
        if let Operation::Evaluate { source, .. } = &mut input.operation {
            source.pop();
        }
        input.edits.last_mut().unwrap().value.push('x');
        assert!(
            input.validate().is_err(),
            "encoded edits +1 must not borrow operation headroom"
        );
        input.edits.clear();
        input.operation = Operation::Evaluate {
            source: String::new(),
            return_by_value: false,
            group: None,
            world: None,
            await_promise: false,
            user_gesture: false,
        };
        let base = encode_request_frame(&frame, false, MAX_AUTOMATION_FRAME_BYTES)
            .unwrap()
            .len();
        let FrameCommand::Automation(input) = &mut frame.command else {
            unreachable!()
        };
        let Operation::Evaluate { source, .. } = &mut input.operation else {
            unreachable!()
        };
        *source = "x".repeat(MAX_AUTOMATION_FRAME_BYTES - base);
        // Serialization-only boundary: source admission independently rejects
        // this payload; no larger executable source allowance is implied.
        assert!(input.validate().is_err());
        assert_eq!(
            encode_request_frame(&frame, false, MAX_AUTOMATION_FRAME_BYTES)
                .unwrap()
                .len(),
            MAX_AUTOMATION_FRAME_BYTES
        );
        let FrameCommand::Automation(input) = &mut frame.command else {
            unreachable!()
        };
        if let Operation::Evaluate { source, .. } = &mut input.operation {
            source.push('x');
        }
        assert!(encode_request_frame(&frame, false, MAX_AUTOMATION_FRAME_BYTES).is_err());
    }

    #[test]
    fn automation_result_section_has_an_exact_128k_boundary_before_full_reply_encoding() {
        use mg_sparkle::page_session::automation::{
            MAX_RESULT_BYTES, OwnProperty, PropertyDescriptor, RemoteValue, encoded_size,
        };
        let mut properties: Vec<_> = (1..=6)
            .map(|name| OwnProperty {
                name: vec![name],
                enumerable: true,
                configurable: true,
                descriptor: PropertyDescriptor::Data {
                    value: RemoteValue::String(if name < 6 {
                        vec![u16::MAX; 4096]
                    } else {
                        vec![]
                    }),
                    writable: true,
                },
            })
            .collect();
        let base = encoded_size(
            &AutomationResult::Properties(properties.clone()),
            MAX_RESULT_BYTES,
        )
        .unwrap();
        let units = (MAX_RESULT_BYTES - base + 1) / 6;
        assert!(units > 0 && units <= 4096);
        let PropertyDescriptor::Data {
            value: RemoteValue::String(text),
            ..
        } = &mut properties[5].descriptor
        else {
            unreachable!()
        };
        text.resize(units, u16::MAX);
        let used = encoded_size(
            &AutomationResult::Properties(properties.clone()),
            MAX_RESULT_BYTES,
        )
        .unwrap();
        let gap = MAX_RESULT_BYTES - used;
        assert!(gap <= 5);
        let first = gap.min(4);
        properties[0].name[0] = 10u16.pow(first as u32);
        properties[1].name[0] = 2 * 10u16.pow((gap - first) as u32);
        let result = AutomationResult::Properties(properties);
        assert_eq!(
            encoded_size(&result, MAX_RESULT_BYTES).unwrap(),
            MAX_RESULT_BYTES
        );
        result.validate().unwrap();
        let mut frame = modern_response(&automation_request());
        frame.reply.scripts_executed = 0;
        frame.result = Some(ModernResult::Automation(result));
        let mut bytes = Vec::new();
        write_reply_frame(&mut bytes, &frame, &mut 0, false).unwrap();
        let Some(ModernResult::Automation(AutomationResult::Properties(properties))) =
            &mut frame.result
        else {
            unreachable!()
        };
        // Preserve valid descriptor/string counts while adding one JSON digit.
        properties[2].name[0] = 30;
        let Some(ModernResult::Automation(result)) = &frame.result else {
            unreachable!()
        };
        assert_eq!(
            encoded_size(result, MAX_RESULT_BYTES + 1).unwrap(),
            MAX_RESULT_BYTES + 1
        );
        let mut bytes = Vec::new();
        let mut wire = 0;
        assert!(write_reply_frame(&mut bytes, &frame, &mut wire, false).is_err());
        assert!(bytes.is_empty());
        assert_eq!(wire, 0);
    }

    #[test]
    fn observer_v8_init_requires_exact_wrapper_and_disabled_tag() {
        let mut frame = request(0);
        frame.version = VERSION;
        let bytes = encode_request_frame(&frame, false, INPUT_LIMIT).unwrap();
        let (decoded, observer) = decode_request_frame(&bytes, false).unwrap();
        assert_eq!(decoded.version, VERSION);
        assert_eq!(observer, ObserverInit::Disabled);
        let good: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for malformed in [
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!({"kind":"Disabled","run_id":1}),
            serde_json::json!({"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":1}),
            serde_json::json!({"kind":"WindowHarness","contract":"Unknown","run_id":1,"bootstrap_node":1}),
            serde_json::json!({"kind":"WindowHarness","contract":"PinnedWindowHarnessV1","run_id":true,"bootstrap_node":1}),
            serde_json::json!({"kind":"WindowHarness","contract":null,"run_id":1,"bootstrap_node":1}),
            serde_json::json!({"kind":"WindowHarness","contract":{"PinnedWindowHarnessOutputDisabledV1":null},"run_id":1,"bootstrap_node":1}),
            serde_json::json!({"kind":"WindowHarness","contract":"PinnedWindowHarnessOutputDisabledV1","run_id":0,"bootstrap_node":1}),
            serde_json::json!({"kind":"WindowHarness","contract":"PinnedWindowHarnessOutputDisabledV1","run_id":1,"bootstrap_node":null}),
            serde_json::json!({"kind":"WindowHarness","contract":"PinnedWindowHarnessOutputDisabledV1","run_id":1,"bootstrap_node":1,"output":false}),
        ] {
            let mut invalid = good.clone();
            invalid["command"]["payload"]["observer"] = malformed;
            assert!(
                decode_request_frame(&serde_json::to_vec(&invalid).unwrap(), false).is_err(),
                "accepted malformed Init: {invalid}"
            );
        }
        for removed in ["observer", "page"] {
            let mut invalid = good.clone();
            invalid["command"]["payload"]
                .as_object_mut()
                .unwrap()
                .remove(removed);
            assert!(decode_request_frame(&serde_json::to_vec(&invalid).unwrap(), false).is_err());
            invalid["command"]["payload"][removed] = serde_json::Value::Null;
            assert!(decode_request_frame(&serde_json::to_vec(&invalid).unwrap(), false).is_err());
        }
        assert!(
            decode_request_frame(&encode(&frame, INPUT_LIMIT).unwrap(), false).is_err(),
            "old unwrapped modern Init must not execute"
        );
    }

    #[test]
    fn observer_v8_both_contracts_bind_exact_init_mode_and_run_without_new_page_reply_fields() {
        use mg_sparkle::page_session::observer::{ObserverContract, ObserverProgress};
        for contract in [
            ObserverContract::PinnedWindowHarnessV1,
            ObserverContract::PinnedWindowHarnessOutputDisabledV1,
        ] {
            let mut initial = request(0);
            initial.version = VERSION;
            let observer = ObserverInit::WindowHarness {
                contract,
                run_id: 91,
                bootstrap_node: 1,
            };
            let encoded =
                encode_request_frame_observed(&initial, false, INPUT_LIMIT, &observer).unwrap();
            let (decoded, actual) = decode_request_frame(&encoded, false).unwrap();
            assert_eq!(decoded.version, VERSION);
            assert_eq!(actual, observer);
            assert!(encode_request_frame_observed(&initial, true, INPUT_LIMIT, &observer).is_err());
            assert!(decode_request_frame(&encoded, true).is_err());
            let text = String::from_utf8(encoded.clone()).unwrap();
            for duplicate in [
                text.replace(
                    "\"observer\":",
                    "\"observer\":{\"kind\":\"Disabled\"},\"observer\":",
                ),
                text.replace(
                    "\"page\":",
                    "\"page\":{\"url\":\"https://example.test/\",\"html\":\"owned\"},\"page\":",
                ),
                text.replace(
                    "\"contract\":",
                    "\"contract\":\"PinnedWindowHarnessV1\",\"contract\":",
                ),
            ] {
                assert!(decode_request_frame(duplicate.as_bytes(), false).is_err());
            }
            let mut raw = serde_json::to_value(&initial).unwrap();
            raw["command"]["payload"]["observer"] = serde_json::to_value(observer).unwrap();
            assert!(decode_request_frame(&serde_json::to_vec(&raw).unwrap(), true).is_err());

            let mut frame = modern_response(&initial);
            frame.reply.observer = Some(ObserverReply::WindowHarness {
                run_id: 91,
                first_ordinal: 0,
                next_ordinal: 0,
                records: Vec::new(),
                progress: ObserverProgress::Waiting,
                execution_failed: false,
            });
            validate_reply(&initial, &frame).unwrap();
            validate_observer_mode(&observer, &frame.reply, false).unwrap();
            assert!(validate_observer_mode(&ObserverInit::Disabled, &frame.reply, false).is_err());
            assert!(validate_observer_mode(&observer, &frame.reply, true).is_err());
            let wrong_run = ObserverInit::WindowHarness {
                contract,
                run_id: 92,
                bootstrap_node: 1,
            };
            assert!(validate_observer_mode(&wrong_run, &frame.reply, false).is_err());
            let encoded_reply = encode(&frame, OUTPUT_LIMIT).unwrap();
            let decoded_reply = decode_reply_frame(&encoded_reply, false).unwrap();
            validate_observer_mode(&observer, &decoded_reply.reply, false).unwrap();
            let mut forbidden = serde_json::to_value(&frame).unwrap();
            forbidden["reply"]["contract"] = serde_json::to_value(contract).unwrap();
            assert!(decode_reply_frame(&serde_json::to_vec(&forbidden).unwrap(), false).is_err());
            for observed in [
                serde_json::to_value(frame.reply.observer.as_ref().unwrap()).unwrap(),
                serde_json::json!({
                    "kind":"ObservationFailed","run_id":91,"prefix_end":0,
                    "fault":"MalformedCapture","execution_failed":false
                }),
            ] {
                let mut forbidden = serde_json::to_value(&frame).unwrap();
                forbidden["reply"]["observer"] = observed;
                assert!(
                    decode_reply_frame(&serde_json::to_vec(&forbidden).unwrap(), false).is_ok()
                );
                forbidden["reply"]["observer"]["contract"] =
                    serde_json::to_value(contract).unwrap();
                assert!(
                    decode_reply_frame(&serde_json::to_vec(&forbidden).unwrap(), false).is_err()
                );
            }
            frame.reply.observer = Some(ObserverReply::Disabled);
            assert!(validate_observer_mode(&observer, &frame.reply, false).is_err());

            for command in [
                FrameCommand::Event(SessionInput {
                    kind: InputKind::Click { target: 1 },
                    edits: Vec::new(),
                }),
                FrameCommand::TaskPump(TaskPump { wake_token: 1 }),
            ] {
                let mut later = request(1);
                later.version = VERSION;
                later.command = command;
                let bytes = encode_request_frame(&later, false, MAX_EVENT_BYTES).unwrap();
                let mut malformed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                malformed["command"]["payload"]["observer"] =
                    serde_json::to_value(observer).unwrap();
                assert!(
                    decode_request_frame(&serde_json::to_vec(&malformed).unwrap(), false).is_err()
                );
            }
        }
    }

    #[test]
    fn observer_v8_maximum_metadata_fits_old_reserve_without_new_allowance() {
        use mg_sparkle::page_session::observer::ObserverContract;
        let mut lengths = Vec::new();
        for contract in [
            ObserverContract::PinnedWindowHarnessV1,
            ObserverContract::PinnedWindowHarnessOutputDisabledV1,
        ] {
            let mut frame = request(0);
            frame.version = VERSION;
            frame.generation = u64::MAX;
            frame.session_id = u64::MAX;
            frame.sequence = u64::MAX;
            frame.expected_revision = u64::MAX;
            let observer = ObserverInit::WindowHarness {
                contract,
                run_id: u64::MAX,
                bootstrap_node: u32::MAX,
            };
            let FrameCommand::Init(page) = &mut frame.command else {
                unreachable!()
            };
            page.url.clear();
            page.html.clear();
            let base = encode(page, INPUT_LIMIT).unwrap().len();
            let metadata = encode_request_frame_observed(&frame, false, INPUT_LIMIT, &observer)
                .unwrap()
                .len()
                - base;
            lengths.push(metadata);
            assert!(
                metadata <= 1024,
                "metadata uses {metadata} bytes of existing reserve"
            );
            let FrameCommand::Init(page) = &mut frame.command else {
                unreachable!()
            };
            page.html = "x".repeat(mg_sparkle::js_browser::MAX_TYPED_REQUEST_BYTES - base);
            let full =
                encode_request_frame_observed(&frame, false, INPUT_LIMIT, &observer).unwrap();
            assert_eq!(
                full.len(),
                mg_sparkle::js_browser::MAX_TYPED_REQUEST_BYTES + metadata
            );
            assert!(full.len() <= INPUT_LIMIT);
            assert_eq!(
                encode_request_frame_observed(&frame, false, full.len(), &observer).unwrap(),
                full
            );
            assert!(
                encode_request_frame_observed(&frame, false, full.len() - 1, &observer).is_err()
            );

            // Serialization-only envelope boundary, not a claim that this
            // larger page passes the independent typed Request admission.
            let FrameCommand::Init(page) = &mut frame.command else {
                unreachable!()
            };
            page.html = "x".repeat(INPUT_LIMIT - base - metadata);
            let exact =
                encode_request_frame_observed(&frame, false, INPUT_LIMIT, &observer).unwrap();
            assert_eq!(exact.len(), INPUT_LIMIT);
            let FrameCommand::Init(page) = &mut frame.command else {
                unreachable!()
            };
            page.html.push('x');
            assert!(encode_request_frame_observed(&frame, false, INPUT_LIMIT, &observer).is_err());
        }
        assert!(
            lengths[1] > lengths[0],
            "new longest contract must be measured"
        );
    }

    #[test]
    fn observer_modern_reply_required_and_legacy_presence_forbidden_even_null() {
        let mut request = request(0);
        request.version = VERSION;
        let frame = modern_response(&request);
        let good = serde_json::to_value(&frame).unwrap();
        for value in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({"kind":"Disabled","extra":0}),
        ] {
            let mut invalid = good.clone();
            invalid["reply"]["observer"] = value;
            assert!(decode_reply_frame(&serde_json::to_vec(&invalid).unwrap(), false).is_err());
        }
        let mut missing = good;
        missing["reply"].as_object_mut().unwrap().remove("observer");
        assert!(decode_reply_frame(&serde_json::to_vec(&missing).unwrap(), false).is_err());
        request.version = 1;
        let legacy = serde_json::to_value(response(&request)).unwrap();
        for value in [
            serde_json::Value::Null,
            serde_json::json!({"kind":"Disabled"}),
        ] {
            let mut invalid = legacy.clone();
            invalid["reply"]["observer"] = value;
            assert!(decode_reply_frame(&serde_json::to_vec(&invalid).unwrap(), true).is_err());
        }
    }

    #[test]
    fn lifecycle_terminal_boa_reply_cannot_retain_pending_jobs() {
        let mut request = request(0);
        request.version = VERSION;
        for state in [RealmState::Closed, RealmState::Fatal] {
            let mut frame = modern_response(&request);
            frame.reply.state = state;
            frame.reply.task_wake = None;
            frame.reply.lifecycle.as_mut().unwrap().initial_failure = true;
            assert!(frame.reply.boa.as_ref().unwrap().is_valid());
            assert!(validate_reply(&request, &frame).is_err(), "{state:?}");
            frame.reply.boa.as_mut().unwrap().pending_jobs = 0;
            validate_reply(&request, &frame).unwrap();
        }
        let mut rejected = modern_response(&request);
        rejected.reply.boa = None;
        rejected.reply.snapshot = None;
        rejected.reply.scripts_executed = 0;
        rejected.reply.state = RealmState::Closed;
        rejected.reply.task_wake = None;
        rejected.reply.lifecycle = Some(LifecycleProgress {
            phase: mg_sparkle::page_session::LifecyclePhase::Initializing,
            initial_failure: true,
        });
        assert!(rejected.reply.is_rejected_initial());
        assert!(!rejected.reply.accounting_valid());
        validate_reply(&request, &rejected).unwrap();
    }

    #[test]
    fn lifecycle_modern_decoder_requires_non_null_strict_progress() {
        let mut request = request(0);
        request.version = VERSION;
        let valid = modern_response(&request);
        let encoded = encode(&valid, OUTPUT_LIMIT).unwrap();
        let decoded = decode_reply_frame(&encoded, false).unwrap();
        validate_reply(&request, &decoded).unwrap();
        for value in [
            serde_json::Value::Null,
            serde_json::json!({}),
            serde_json::json!({"phase":"Interactive","initial_failure":null}),
            serde_json::json!({"phase":"Interactive","initial_failure":0}),
            serde_json::json!({"phase":"Interactive","initial_failure":false,"extra":1}),
            serde_json::json!({"phase":"Unknown","initial_failure":false}),
        ] {
            let mut invalid = serde_json::to_value(&valid).unwrap();
            invalid["reply"]["lifecycle"] = value;
            assert!(decode_reply_frame(&serde_json::to_vec(&invalid).unwrap(), false).is_err());
        }
        let mut missing = serde_json::to_value(&valid).unwrap();
        missing["reply"]
            .as_object_mut()
            .unwrap()
            .remove("lifecycle");
        assert!(decode_reply_frame(&serde_json::to_vec(&missing).unwrap(), false).is_err());
        let mut old = request;
        old.version = 4;
        let mut reply = modern_response(&old);
        reply.version = 4;
        assert!(validate_reply(&old, &reply).is_err());
    }

    #[test]
    fn lifecycle_legacy_decoder_rejects_presence_even_null_and_encoding_is_borrowed_old_shape() {
        let request = request(0);
        let mut frame = response(&request);
        let expected = encode(&frame, OUTPUT_LIMIT).unwrap();
        for value in [
            serde_json::Value::Null,
            serde_json::json!({"phase":"Initializing","initial_failure":true}),
        ] {
            let mut invalid = serde_json::to_value(&frame).unwrap();
            invalid["reply"]["lifecycle"] = value;
            assert!(decode_reply_frame(&serde_json::to_vec(&invalid).unwrap(), true).is_err());
        }
        let mut output = Vec::new();
        let mut written = 0;
        write_reply_frame(&mut output, &frame, &mut written, true).unwrap();
        assert_eq!(written, expected.len() + 4);
        assert_eq!(&output[..4], &(expected.len() as u32).to_be_bytes());
        assert_eq!(&output[4..], expected);
        let decoded = decode_reply_frame(&output[4..], true).unwrap();
        assert!(decoded.reply.lifecycle.is_none());
        validate_reply(&request, &decoded).unwrap();
        // Wrong-lane internal metadata is not silently stripped or published.
        frame.reply.lifecycle = Some(LifecycleProgress {
            phase: mg_sparkle::page_session::LifecyclePhase::Interactive,
            initial_failure: false,
        });
        let mut forbidden = Vec::new();
        let mut written = 0;
        assert!(write_reply_frame(&mut forbidden, &frame, &mut written, true).is_err());
        assert!(forbidden.is_empty());
        assert_eq!(written, 0);
    }

    #[test]
    fn framed_messages_round_trip_and_preserve_next_frame_boundary() {
        let value = serde_json::json!({"text":"owned\nUTF-8 café"});
        let mut bytes = Vec::new();
        let mut written = 0;
        write_frame(&mut bytes, &value, &mut written).unwrap();
        write_frame(&mut bytes, &value, &mut written).unwrap();
        assert_eq!(written, bytes.len());
        let mut reader = Cursor::new(bytes);
        let mut read = 0;
        for _ in 0..2 {
            let payload = read_frame(&mut reader, &mut read, INPUT_LIMIT)
                .unwrap()
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&payload).unwrap(),
                value
            );
        }
        assert!(
            read_frame(&mut reader, &mut read, INPUT_LIMIT)
                .unwrap()
                .is_none()
        );
        assert_eq!(read, written);
    }
    #[test]
    fn header_limits_reject_before_request_payload_read_or_allocation() {
        for (length, limit) in [
            (0, INPUT_LIMIT),
            (INPUT_LIMIT + 1, INPUT_LIMIT),
            (MAX_EVENT_BYTES + 1, MAX_EVENT_BYTES),
        ] {
            let mut input = Cursor::new((length as u32).to_be_bytes());
            let mut total = 0;
            assert!(read_frame(&mut input, &mut total, limit).is_err());
            assert_eq!(input.position(), 4);
            assert_eq!(total, 4);
        }
        let mut header = Cursor::new(10u32.to_be_bytes());
        let mut total = WIRE_LIMIT - 8;
        assert!(read_frame(&mut header, &mut total, INPUT_LIMIT).is_err());
        assert_eq!(total, WIRE_LIMIT - 4);
    }
    #[test]
    fn truncated_frames_and_combined_wire_budget_fail_closed() {
        for bytes in [vec![0], vec![0, 0, 0, 2, b'{']] {
            assert!(read_frame(&mut Cursor::new(bytes), &mut 0, INPUT_LIMIT).is_err());
        }
        let mut total = WIRE_LIMIT - 1;
        add_wire(&mut total, 1).unwrap();
        assert!(add_wire(&mut total, 1).is_err());
        assert_eq!(total, WIRE_LIMIT);
        assert!(check_payload(usize::MAX, OUTPUT_LIMIT, 0).is_err());
        let mut output = Vec::new();
        let mut total = WIRE_LIMIT - 4;
        assert!(write_frame(&mut output, &"owned", &mut total).is_err());
        assert!(output.is_empty());
    }
    #[test]
    fn strict_envelopes_reject_unknown_fields_commands_and_identity_mismatches() {
        let request = request(0);
        let mut json = serde_json::to_value(&request).unwrap();
        json["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<RequestFrame>(json).is_err());
        let mut json = serde_json::to_value(&request).unwrap();
        json["command"]["kind"] = serde_json::json!("Evaluate");
        assert!(serde_json::from_value::<RequestFrame>(json).is_err());
        for field in [
            "version",
            "generation",
            "session_id",
            "sequence",
            "expected_revision",
        ] {
            let mut frame = serde_json::to_value(response(&request)).unwrap();
            frame[field] = serde_json::json!(99);
            let frame: ReplyFrame = serde_json::from_value(frame).unwrap();
            assert!(validate_reply(&request, &frame).is_err(), "{field}");
        }
    }
    #[test]
    fn observer_modern_v8_rejects_v1_through_v7_before_execution() {
        assert_eq!(VERSION, 9);
        for version in 1..=8 {
            for wrapped in [false, true] {
                let mut initial = request(0);
                initial.version = version;
                let FrameCommand::Init(page) = &mut initial.command else {
                    unreachable!()
                };
                page.html = "<script>throw 'MUST_NOT_RUN'</script>".into();
                let mut input = Vec::new();
                if wrapped {
                    write_request(&mut input, &initial, false);
                } else {
                    write_frame(&mut input, &initial, &mut 0).unwrap();
                }
                let mut output = Vec::new();
                assert!(child_loop_for(&mut Cursor::new(input), &mut output, false).is_err());
                assert!(
                    output.is_empty(),
                    "old modern frames must execute no source: version={version}, wrapped={wrapped}"
                );
            }
        }
    }
    #[test]
    fn observer_modern_v8_accepts_absent_source_metadata_in_required_wrapper() {
        let mut initial = request(0);
        initial.version = VERSION;
        let mut input = Vec::new();
        write_request(&mut input, &initial, false);
        let mut output = Vec::new();
        child_loop_for(&mut Cursor::new(input), &mut output, false).unwrap();
        let bytes = read_frame(&mut Cursor::new(output), &mut 0, OUTPUT_LIMIT)
            .unwrap()
            .unwrap();
        let reply: ReplyFrame = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(reply.version, VERSION);
        assert_eq!(reply.reply.state, RealmState::Ready);
    }
    #[test]
    fn observer_modern_v8_rejects_later_v1_through_v7_events_and_pumps_without_replies() {
        for (task, old_version) in
            (1..=8).flat_map(|version| [false, true].map(|task| (task, version)))
        {
            let mut initial = request(0);
            initial.version = VERSION;
            let FrameCommand::Init(page) = &mut initial.command else {
                unreachable!()
            };
            page.html = "<p id=o></p><script>window.onclick=()=>{document.getElementById('o').textContent='RAN';};setTimeout(()=>{document.getElementById('o').textContent='RAN';},0);</script>".into();
            let mut later = request(1);
            later.version = old_version;
            if task {
                later.command = FrameCommand::TaskPump(TaskPump { wake_token: 1 });
            }
            let mut input = Vec::new();
            write_request(&mut input, &initial, false);
            write_frame(&mut input, &later, &mut 0).unwrap();
            let mut output = Vec::new();
            assert!(child_loop_for(&mut Cursor::new(input), &mut output, false).is_err());
            let mut cursor = Cursor::new(output);
            let first = read_frame(&mut cursor, &mut 0, OUTPUT_LIMIT)
                .unwrap()
                .unwrap();
            let first: ReplyFrame = serde_json::from_slice(&first).unwrap();
            assert_eq!(first.version, VERSION);
            assert_eq!(first.reply.state, RealmState::Ready);
            // One source timer and both mandatory lifecycle tasks remain;
            // rejected old-version commands execute none of those callbacks.
            assert_eq!(first.reply.boa.unwrap().pending_jobs, 3);
            assert!(
                read_frame(&mut cursor, &mut 0, OUTPUT_LIMIT)
                    .unwrap()
                    .is_none(),
                "old-version commands must not execute or emit another reply"
            );
        }
    }
    #[test]
    fn provenance_legacy_init_rejects_new_metadata_including_null_before_execution() {
        for provenance in [
            serde_json::Value::Null,
            serde_json::json!({
                "response_url":"https://example.test/script.js",
                "error_visibility":"same-origin"
            }),
        ] {
            let mut initial = serde_json::to_value(request(0)).unwrap();
            initial["command"]["payload"]["html"] =
                serde_json::json!("<script src='/script.js'></script>");
            initial["command"]["payload"]["external_scripts"] = serde_json::json!([{
                "node":1,"source":"throw 'MUST_NOT_RUN'","provenance":provenance
            }]);
            let mut input = Vec::new();
            write_frame(&mut input, &initial, &mut 0).unwrap();
            let mut output = Vec::new();
            assert!(child_loop_for(&mut Cursor::new(input), &mut output, true).is_err());
            assert!(
                output.is_empty(),
                "legacy provenance must reject before Init"
            );
        }
    }
    #[test]
    fn provenance_selected_legacy_frame_encoding_keeps_old_bytes_and_strict_ingress() {
        let mut value = serde_json::to_value(request(0)).unwrap();
        value["command"]["payload"]["html"] = serde_json::json!("<script src='/s.js'></script>");
        value["command"]["payload"]["external_scripts"] = serde_json::json!([{
            "node":1,"source":"'owned \\ café'","provenance":{
                "response_url":"https://example.test/s.js?escaped=\"",
                "error_visibility":"same-origin"
            }
        }]);
        let frame: RequestFrame = serde_json::from_value(value.clone()).unwrap();
        let modern = encode_request_frame(&frame, false, INPUT_LIMIT).unwrap();
        assert!(decode_request_frame(&modern, true).is_err());
        value["command"]["payload"]["external_scripts"][0]
            .as_object_mut()
            .unwrap()
            .remove("provenance");
        // Derive the frozen old field order from the original frame shape,
        // not Value's sorted object order.
        let old: RequestFrame = serde_json::from_value(value).unwrap();
        let expected = encode(&old, INPUT_LIMIT).unwrap();
        let actual = encode_request_frame(&frame, true, INPUT_LIMIT).unwrap();
        assert_eq!(actual, expected);
        let golden = r#"{"version":1,"generation":7,"session_id":8,"sequence":0,"expected_revision":0,"command":{"kind":"Init","payload":{"url":"https://example.test/","html":"<script src='/s.js'></script>","external_scripts":[{"node":1,"source":"'owned \\ café'"}]}}}"#;
        assert_eq!(
            actual,
            golden.as_bytes(),
            "frozen old external-source field order/escaping"
        );
        assert_eq!(
            encode_request_frame(&frame, true, actual.len()).unwrap(),
            actual
        );
        assert!(encode_request_frame(&frame, true, actual.len() - 1).is_err());
        assert!(encode_request_frame(&frame, false, actual.len()).is_err());
        let (decoded, observer) = decode_request_frame(&actual, true).unwrap();
        assert!(matches!(observer, ObserverInit::Disabled));
        let FrameCommand::Init(legacy_request) = decoded.command else {
            unreachable!()
        };
        let source = &legacy_request.external_scripts.unwrap()[0];
        assert!(source.provenance.is_none());
        assert_eq!(source.source, "'owned \\ café'");
        for sources in [None, Some(Vec::new())] {
            let mut frame = request(0);
            let FrameCommand::Init(initial) = &mut frame.command else {
                unreachable!()
            };
            initial.external_scripts = sources;
            assert_eq!(
                encode_request_frame(&frame, true, INPUT_LIMIT).unwrap(),
                encode(&frame, INPUT_LIMIT).unwrap()
            );
        }
    }
    #[test]
    fn terminal_revisions_and_acknowledgements_distinguish_admitted_from_rejected_input() {
        let mut request = request(1);
        let FrameCommand::Event(input) = &mut request.command else {
            unreachable!()
        };
        input.edits.push(ControlEdit {
            node: 2,
            version: 3,
            value: "typed".into(),
        });
        let mut frame = response(&request);
        frame.reply.state = RealmState::Closed;
        frame.reply.snapshot = None;
        frame.reply.revision = 0;
        validate_reply(&request, &frame).unwrap();
        frame.reply.acknowledgements.push(ControlAck {
            node: 2,
            version: 3,
        });
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.revision = 1;
        validate_reply(&request, &frame).unwrap();
        frame.reply.acknowledgements[0].version = 4;
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.acknowledgements[0].version = 3;
        frame.reply.navigation = Some("https://example.test/ignored".into());
        assert!(validate_reply(&request, &frame).is_err());
    }
    #[test]
    fn diagnostics_keep_existing_prefix_capacity_without_unbounded_output() {
        let request = request(1);
        let mut frame = response(&request);
        frame
            .reply
            .errors
            .push(format!("click handler: {}", "x".repeat(512)));
        validate_reply(&request, &frame).unwrap();
        frame.reply.errors[0] = "x".repeat(4097);
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.errors = vec!["small".into(); 65];
        assert!(validate_reply(&request, &frame).is_err());
    }
    #[test]
    fn task_frames_require_modern_consistent_wakes_and_no_activation_effects() {
        let mut request = request(1);
        request.version = VERSION;
        request.command = FrameCommand::TaskPump(TaskPump { wake_token: 7 });
        let mut frame = modern_response(&request);
        frame.reply.scripts_executed = 0;
        validate_reply(&request, &frame).unwrap();
        for change in [
            "ack",
            "default",
            "click",
            "submit",
            "scripts",
            "missing-wake",
            "empty-jobs",
            "zero",
            "closed",
            "legacy",
            "overflow",
        ] {
            let mut changed_request: RequestFrame =
                serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
            let mut changed: ReplyFrame =
                serde_json::from_value(serde_json::to_value(&frame).unwrap()).unwrap();
            match change {
                "ack" => changed.reply.acknowledgements.push(ControlAck {
                    node: 1,
                    version: 1,
                }),
                "default" => changed.reply.default_action = DefaultAction::FollowLink { node: 1 },
                "click" => changed.reply.outcome.click_canceled = Some(false),
                "submit" => changed.reply.outcome.submit_canceled = Some(false),
                "scripts" => changed.reply.scripts_executed = 1,
                "missing-wake" => {
                    let report = changed.reply.boa.as_mut().unwrap();
                    report.pending_jobs = 1;
                    report.jobs_admitted = 1;
                }
                "empty-jobs" => {
                    changed.reply.task_wake = Some(mg_sparkle::page_session::TaskWake {
                        token: 8,
                        delay_ms: 0,
                    })
                }
                "zero" | "closed" | "legacy" => {
                    let report = changed.reply.boa.as_mut().unwrap();
                    report.pending_jobs = 1;
                    report.jobs_admitted = 1;
                    changed.reply.task_wake = Some(mg_sparkle::page_session::TaskWake {
                        token: if change == "zero" { 0 } else { 8 },
                        delay_ms: 0,
                    });
                    if change == "closed" {
                        changed.reply.state = RealmState::Closed;
                    }
                    if change == "legacy" {
                        changed_request.version = 1;
                        changed.version = 1;
                    }
                }
                "overflow" => {
                    changed_request.expected_revision = u64::MAX;
                    changed.expected_revision = u64::MAX;
                }
                _ => unreachable!(),
            }
            assert!(
                validate_reply(&changed_request, &changed).is_err(),
                "{change}"
            );
        }
        frame.reply.task_wake = Some(mg_sparkle::page_session::TaskWake {
            token: 8,
            delay_ms: u32::MAX,
        });
        let report = frame.reply.boa.as_mut().unwrap();
        report.pending_jobs = 1;
        report.jobs_admitted = 1;
        validate_reply(&request, &frame).unwrap();
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["command"]["kind"], "TaskPump");
        assert!(encoded["command"]["payload"].get("edits").is_none());
    }
    #[test]
    fn every_allocation_phase_is_monotonic_and_first_rejection_cannot_reset() {
        let old = report(100);
        validate_cumulative(Some(old), Some(report(101))).unwrap();
        assert!(validate_cumulative(Some(old), Some(report(99))).is_err());
        let mut moved = report(100);
        moved.phases.runtime -= 1;
        moved.phases.source += 1;
        assert!(moved.is_valid());
        assert!(validate_cumulative(Some(old), Some(moved)).is_err());
        assert!(validate_cumulative(Some(old), None).is_err());
        let mut rejected = old;
        rejected.first_rejected = Some(RejectedAllocation {
            phase: AllocationPhase::Runtime,
            accepted_bytes: 100,
            requested_bytes: 4 * 1024 * 1024,
            limit_bytes: 4 * 1024 * 1024,
        });
        assert!(rejected.is_valid());
        validate_cumulative(Some(rejected), Some(rejected)).unwrap();
        assert!(validate_cumulative(Some(rejected), Some(old)).is_err());
    }
    #[test]
    fn boa_profile_requires_process_accounting_and_cannot_fall_back_or_reset() {
        use mg_butane::modern::{WORKER_CUMULATIVE_LIMIT, WORKER_OUTSTANDING_LIMIT, WorkerMemory};
        let mut request = request(0);
        request.version = VERSION;
        let mut frame = response(&request);
        assert!(validate_engine_profile(&frame.reply, false, None).is_err());
        frame = modern_response(&request);
        assert!(validate_engine_profile(&frame.reply, false, None).is_err());
        frame.reply.boa.as_mut().unwrap().worker_memory = Some(WorkerMemory {
            outstanding_bytes: 100,
            peak_bytes: 100,
            cumulative_bytes: 200,
            allocations: 2,
            outstanding_limit_bytes: WORKER_OUTSTANDING_LIMIT,
            cumulative_limit_bytes: WORKER_CUMULATIVE_LIMIT,
        });
        validate_reply(&request, &frame).unwrap();
        validate_engine_profile(&frame.reply, false, None).unwrap();
        assert!(validate_engine_profile(&frame.reply, true, None).is_err());
        let previous = frame.reply.boa.clone().unwrap();
        frame
            .reply
            .boa
            .as_mut()
            .unwrap()
            .worker_memory
            .as_mut()
            .unwrap()
            .cumulative_bytes = 199;
        assert!(validate_engine_profile(&frame.reply, false, Some(&previous)).is_err());
        frame.reply.boa = Some(previous.clone());
        frame.reply.allocations = Some(report(10));
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.allocations = None;
        frame.reply.boa.as_mut().unwrap().fatal_reason = Some("latched limit".into());
        assert!(validate_reply(&request, &frame).is_err());
        frame.reply.state = RealmState::Closed;
        frame.reply.snapshot = None;
        frame.reply.boa = None;
        frame.reply.scripts_executed = 0;
        frame.reply.task_wake = None;
        frame.reply.lifecycle = Some(LifecycleProgress {
            phase: mg_sparkle::page_session::LifecyclePhase::Initializing,
            initial_failure: true,
        });
        validate_engine_profile(&frame.reply, false, None).unwrap();
        assert!(validate_engine_profile(&frame.reply, false, Some(&previous)).is_err());
        frame.reply.allocations = Some(report(10));
        assert!(validate_engine_profile(&frame.reply, false, None).is_err());
    }
    #[test]
    fn active_accounting_and_lifetime_are_nonrenewable_but_terminal_projection_can_be_charged() {
        let shared = Shared::default();
        shared.charge(Duration::from_millis(750)).unwrap();
        shared.closed.store(true, Ordering::Release); // Already reaped terminal reply.
        shared.charge(Duration::from_millis(750)).unwrap();
        assert_eq!(shared.remaining().unwrap(), Duration::from_millis(500));
        assert!(shared.charge(Duration::from_millis(500)).is_err());
        assert!(shared.charge(Duration::ZERO).is_err());
        let shared = Shared::default();
        *shared.spawned.lock().unwrap() = Some(Instant::now() - LIFETIME);
        assert!(shared.charge(Duration::ZERO).is_err());
        assert_eq!(shared.ledger.lock().unwrap().used, Duration::ZERO);
    }
    #[test]
    fn failure_latching_is_separate_from_post_reap_closure() {
        let shared = Shared::default();
        assert_eq!(shared.fail("first".into()), "first");
        assert!(!shared.closed.load(Ordering::Acquire));
        assert_eq!(shared.close("later".into()), "first");
        assert!(shared.closed.load(Ordering::Acquire));
        assert_eq!(shared.error().as_deref(), Some("first"));
    }
    #[test]
    fn edit_preflight_and_permanent_pool_fence_do_not_need_a_child() {
        let mut input = SessionInput {
            kind: InputKind::Click { target: 1 },
            edits: vec![ControlEdit {
                node: 1,
                version: 0,
                value: "x".repeat(MAX_EDIT_BYTES),
            }],
        };
        validate_input(&input).unwrap();
        input.edits[0].value.push('x');
        assert!(validate_input(&input).is_err());
        input.edits[0].value.clear();
        input.edits.push(input.edits[0].clone());
        assert!(validate_input(&input).is_err());
        let pool = ChildPool::new();
        pool.set_generation(8).unwrap();
        assert!(pool.current(7).is_err());
        pool.current(8).unwrap();
        pool.close();
        pool.wait_idle().unwrap();
        assert!(pool.current(8).is_err());
        assert!(pool.set_generation(9).is_err());
        fn send<T: Send>() {}
        send::<Session>();
        send::<ChildPool>();
    }
}
