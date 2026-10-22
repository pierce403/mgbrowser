use super::super::linux::{
    INPUT_LIMIT, OUTPUT_LIMIT, OwnedChild, WALL_LIMIT, encode, install_isolation, nonblocking,
};
use super::{ChildPool, Permit, Request, SessionInput, SessionReply, SessionUpdate, TaskPump};
use mg_butane::runtime::AllocationReport;
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

const VERSION: u8 = 5;
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
struct RequestFrame<I = Request, E = SessionInput, T = TaskPump> {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    command: FrameCommand<I, E, T>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", deny_unknown_fields)]
enum FrameCommand<I = Request, E = SessionInput, T = TaskPump> {
    Init(I),
    Event(E),
    TaskPump(T),
}

/// The executable's trusted lane chooses the decoder, never a wire field.
/// A legacy payload keeps its original strict shape, including unknown-field
/// rejection, while conversion moves strings without a Value tree or reparse.
fn decode_request_frame(payload: &[u8], legacy: bool) -> Result<RequestFrame, String> {
    if legacy {
        let frame: RequestFrame<mg_sparkle::js_browser::LegacyRequest> =
            serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        Ok(RequestFrame {
            version: frame.version,
            generation: frame.generation,
            session_id: frame.session_id,
            sequence: frame.sequence,
            expected_revision: frame.expected_revision,
            command: match frame.command {
                FrameCommand::Init(request) => FrameCommand::Init(request.into()),
                FrameCommand::Event(input) => FrameCommand::Event(input),
                FrameCommand::TaskPump(pump) => FrameCommand::TaskPump(pump),
            },
        })
    } else {
        serde_json::from_slice(payload).map_err(|error| error.to_string())
    }
}

fn encode_request_frame(
    frame: &RequestFrame,
    legacy: bool,
    limit: usize,
) -> Result<Vec<u8>, String> {
    if !legacy {
        return encode(frame, limit);
    }
    // Borrow sources, edits and pumps. Counting and transmission use the same
    // view; no modern-only provenance can change old v1 bytes or renew a budget.
    let command = match &frame.command {
        FrameCommand::Init(request) => FrameCommand::Init(request.legacy_wire_view()),
        FrameCommand::Event(input) => FrameCommand::Event(input),
        FrameCommand::TaskPump(pump) => FrameCommand::TaskPump(pump),
    };
    encode(
        &RequestFrame {
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
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyFrame<R = SessionReply> {
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
        let frame: ReplyFrame<LegacySessionReply> =
            serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        let old = frame.reply;
        Ok(ReplyFrame {
            version: frame.version,
            generation: frame.generation,
            session_id: frame.session_id,
            sequence: frame.sequence,
            expected_revision: frame.expected_revision,
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
            },
        })
    } else {
        let frame: ReplyFrame =
            serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        if frame.reply.lifecycle.is_none() {
            return Err("Modern v5 reply requires non-null lifecycle progress".into());
        }
        Ok(frame)
    }
}

fn write_reply_frame(
    writer: &mut impl Write,
    frame: &ReplyFrame,
    total: &mut usize,
    legacy: bool,
) -> Result<(), String> {
    if !legacy {
        if frame.reply.lifecycle.is_none() {
            return Err("Modern v5 reply requires lifecycle progress".into());
        }
        return write_frame(writer, frame, total);
    }
    let reply = &frame.reply;
    if reply.lifecycle.is_some() {
        return Err("Legacy v1 reply forbids lifecycle progress".into());
    }
    write_frame(
        writer,
        &ReplyFrame {
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
}
impl Session {
    pub fn start(
        request: Request,
        generation: u64,
        pool: ChildPool,
    ) -> Result<(Self, SessionReply), String> {
        let started = Instant::now();
        pool.current(generation)?;
        let id = NEXT_SESSION
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
            .map_err(|_| "Script-session identity exhausted")?;
        let shared = Arc::new(Shared::default());
        let (tx, commands) = mpsc::sync_channel(1);
        let (updates, rx) = mpsc::sync_channel(1);
        let manager_shared = shared.clone();
        let manager_pool = pool.clone();
        let manager = thread::Builder::new()
            .name("mgbrowser-page-worker".into())
            .spawn(move || {
                manage(
                    request,
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
        session.revision = reply.revision;
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
            let _ = self.shared.charge(started.elapsed());
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
                }
                if let Ok(reply) = &update.reply {
                    self.revision = reply.revision;
                    self.validation_pending = true;
                    self.task_wake = reply.task_wake.clone();
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
}
impl Worker {
    fn start(pool: ChildPool, generation: u64) -> Result<Self, String> {
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
    ) -> Result<SessionReply, String> {
        let deadline = started
            .checked_add(shared.remaining()?)
            .ok_or("Script active deadline exceeds the monotonic clock range")?;
        let limit = if matches!(request.command, FrameCommand::Init(_)) {
            INPUT_LIMIT
        } else {
            MAX_EVENT_BYTES
        };
        let input = encode_request_frame(request, self.legacy, limit)
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
                return Ok(frame.reply);
            }
            if !progressed {
                thread::sleep(TICK);
            }
        }
    }
    fn stop(&mut self) {
        self.owned.stop();
        if !self.owned.reaped {
            // Do not admit a replacement child after an unconfirmed reap.
            if let Some(permit) = self.permit.take() {
                std::mem::forget(permit);
            }
        }
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
    }
}
fn manage(
    request: Request,
    generation: u64,
    id: u64,
    pool: ChildPool,
    shared: Arc<Shared>,
    commands: Receiver<Control>,
    updates: SyncSender<SessionUpdate>,
    started: Instant,
) {
    let mut worker = match Worker::start(pool, generation) {
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
        Ok(reply) => {
            let revision = reply.revision;
            let terminal = reply.state != RealmState::Ready;
            if terminal {
                worker.stop();
            }
            if updates
                .try_send(update(generation, id, Some(0), Ok(reply)))
                .is_err()
            {
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
                        },
                    };
                    worker.transaction(&request, &shared, started)
                })();
                let elapsed = shared.charge(started.elapsed());
                match result.and_then(|reply| elapsed.map(|_| reply)) {
                    Ok(reply) => {
                        next_sequence += 1;
                        revision = reply.revision;
                        let terminal =
                            reply.state != RealmState::Ready || sequence + 1 == TRANSACTIONS;
                        if terminal {
                            worker.stop();
                        }
                        if updates
                            .try_send(update(generation, id, Some(sequence), Ok(reply)))
                            .is_err()
                        {
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
    }
}
fn validate_reply(request: &RequestFrame, frame: &ReplyFrame) -> Result<(), String> {
    if !matches!(request.version, 1 | VERSION) {
        return Err("Unsupported script-session protocol version".into());
    }
    if matches!(request.command, FrameCommand::TaskPump(_)) && request.version != VERSION {
        return Err("Legacy script session does not support task pumping".into());
    }
    if frame.version != request.version
        || frame.generation != request.generation
        || frame.session_id != request.session_id
        || frame.sequence != request.sequence
        || frame.expected_revision != request.expected_revision
    {
        return Err("Script-session reply identity or sequence mismatch".into());
    }
    let reply = &frame.reply;
    let initial = matches!(request.command, FrameCommand::Init(_));
    if (request.version == VERSION) != reply.lifecycle.is_some() {
        return Err("Script reply changed its trusted lifecycle protocol lane".into());
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
            || reply.task_wake.is_some())
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
    if rejected_input
        && (reply.snapshot.is_some()
            || !reply.acknowledgements.is_empty()
            || reply.outcome.click_canceled.is_some()
            || reply.outcome.submit_canceled.is_some())
    {
        return Err("Rejected script event returned effects or acknowledgements".into());
    }
    if let FrameCommand::Event(input) = &request.command {
        if !rejected_input
            && (reply.acknowledgements.len() != input.edits.len()
                || reply
                    .acknowledgements
                    .iter()
                    .zip(&input.edits)
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
    fn start(request: Request, legacy: bool) -> (Option<Self>, SessionReply) {
        #[cfg(feature = "legacy-test-engine")]
        if legacy {
            let (realm, reply) = mg_sparkle::js_browser::PageRealm::start(request);
            return (realm.map(Self::Legacy), reply);
        }
        let _ = legacy;
        let (realm, reply) = mg_sparkle::js_browser::boa::BoaPageRealm::start(request);
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
    let request = decode_request_frame(&payload, legacy)?;
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
    let (mut realm, mut reply) = PageEngine::start(initial, legacy);
    attach_memory(&mut reply);
    let mut revision = reply.revision;
    let mut terminal = reply.state != RealmState::Ready;
    write_reply_frame(
        writer,
        &ReplyFrame {
            version,
            generation: request.generation,
            session_id: request.session_id,
            sequence: 0,
            expected_revision: 0,
            reply,
        },
        &mut wire,
        legacy,
    )?;
    for sequence in 1..TRANSACTIONS {
        if terminal {
            return Ok(());
        }
        let Some(payload) = read_frame(reader, &mut wire, MAX_EVENT_BYTES)? else {
            return Ok(());
        };
        let event = decode_request_frame(&payload, legacy)?;
        if event.version != version
            || event.generation != request.generation
            || event.session_id != request.session_id
            || event.sequence != sequence
            || event.expected_revision != revision
        {
            return Err("Invalid script-session event envelope".into());
        }
        let realm = realm.as_mut().ok_or("Script realm is unavailable")?;
        let mut reply = match event.command {
            FrameCommand::Event(input) => {
                validate_input(&input)?;
                realm.dispatch(input)
            }
            FrameCommand::TaskPump(pump) if !legacy && pump.wake_token != 0 => {
                realm.pump_tasks(pump)?
            }
            _ => return Err("Repeated initialization or unsupported task command".into()),
        };
        attach_memory(&mut reply);
        revision = reply.revision;
        terminal = reply.state != RealmState::Ready;
        write_reply_frame(
            writer,
            &ReplyFrame {
                version,
                generation: request.generation,
                session_id: request.session_id,
                sequence,
                expected_revision: event.expected_revision,
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
            },
        }
    }

    fn modern_response(request: &RequestFrame) -> ReplyFrame {
        let mut frame = response(request);
        frame.reply.allocations = None;
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
    fn lifecycle_modern_v5_rejects_v3_and_v4_before_execution() {
        assert_eq!(VERSION, 5);
        for version in [3, 4] {
            let mut initial = request(0);
            initial.version = version;
            let mut input = Vec::new();
            write_frame(&mut input, &initial, &mut 0).unwrap();
            let mut output = Vec::new();
            assert!(child_loop_for(&mut Cursor::new(input), &mut output, false).is_err());
            assert!(
                output.is_empty(),
                "old modern frames must execute no source"
            );
        }
    }
    #[test]
    fn lifecycle_modern_v5_accepts_old_absent_source_metadata_shape() {
        let mut initial = request(0);
        initial.version = VERSION;
        let mut input = Vec::new();
        write_frame(&mut input, &initial, &mut 0).unwrap();
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
    fn lifecycle_modern_v5_rejects_later_v3_v4_events_and_pumps_without_replies() {
        for (task, old_version) in [(false, 3), (true, 3), (false, 4), (true, 4)] {
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
            write_frame(&mut input, &initial, &mut 0).unwrap();
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
        let decoded = decode_request_frame(&actual, true).unwrap();
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
