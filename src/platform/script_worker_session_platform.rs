use super::super::linux::{
    INPUT_LIMIT, OUTPUT_LIMIT, OwnedChild, WALL_LIMIT, encode, install_isolation, nonblocking,
};
use super::{ChildPool, Permit, Request, SessionInput, SessionReply, SessionUpdate};
use mg_butane::runtime::AllocationReport;
use mg_sparkle::page_session::{
    DefaultAction, MAX_EDIT_BYTES, MAX_EDITS, MAX_EVENT_BYTES, RealmState,
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

const VERSION: u8 = 1;
const LIFETIME: Duration = Duration::from_secs(300);
const TRANSACTIONS: u64 = 64;
const WIRE_LIMIT: usize = 32 * 1024 * 1024;
const TICK: Duration = Duration::from_millis(2);
const IDLE_TICK: Duration = Duration::from_millis(25);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestFrame {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    command: FrameCommand,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", deny_unknown_fields)]
enum FrameCommand {
    Init(Request),
    Event(SessionInput),
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplyFrame {
    version: u8,
    generation: u64,
    session_id: u64,
    sequence: u64,
    expected_revision: u64,
    reply: SessionReply,
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

enum Control {
    Event {
        sequence: u64,
        expected_revision: u64,
        input: SessionInput,
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
        if let Err(error) = validate_input(&input) {
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
        let command = Control::Event {
            sequence,
            expected_revision,
            input,
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
                } else {
                    self.validation_pending = false;
                    self.closure_reported = true;
                }
                Some(update)
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                if !self.closure_reported && self.shared.closed.load(Ordering::Acquire) {
                    self.closure_reported = true;
                    self.validation_pending = false;
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
}
impl Worker {
    fn start(pool: ChildPool, generation: u64) -> Result<Self, String> {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut command = Command::new(executable);
        command
            .arg("--script-session")
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
        let deadline = started + shared.remaining()?;
        let limit = if matches!(request.command, FrameCommand::Init(_)) {
            INPUT_LIMIT
        } else {
            MAX_EVENT_BYTES
        };
        let input = encode(request, limit)
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
                let frame: ReplyFrame = serde_json::from_slice(output)
                    .map_err(|error| format!("Invalid script-session reply: {error}"))?;
                validate_reply(request, &frame)?;
                validate_cumulative(self.last_report, frame.reply.allocations)?;
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
        version: VERSION,
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
            Ok(Control::Event {
                sequence,
                expected_revision,
                input,
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
                    validate_input(&input)?;
                    let request = RequestFrame {
                        version: VERSION,
                        generation,
                        session_id: id,
                        sequence,
                        expected_revision,
                        command: FrameCommand::Event(input),
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
fn validate_reply(request: &RequestFrame, frame: &ReplyFrame) -> Result<(), String> {
    if frame.version != VERSION
        || frame.generation != request.generation
        || frame.session_id != request.session_id
        || frame.sequence != request.sequence
        || frame.expected_revision != request.expected_revision
    {
        return Err("Script-session reply identity or sequence mismatch".into());
    }
    let reply = &frame.reply;
    let initial = matches!(request.command, FrameCommand::Init(_));
    let expected = if initial {
        0
    } else {
        request.expected_revision + 1
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
    if reply.state == RealmState::Ready && (reply.snapshot.is_none() || reply.allocations.is_none())
    {
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
        && (reply.navigation.is_some() || reply.default_action != DefaultAction::None)
    {
        return Err("Closed script session proposed a default or navigation".into());
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
    if install_isolation().is_err() {
        std::process::exit(73);
    }
    if std::env::args().len() != 2 {
        std::process::exit(74);
    }
    let result = child_loop(&mut io::stdin().lock(), &mut io::stdout().lock());
    std::process::exit(if result.is_ok() { 0 } else { 74 })
}
fn child_loop(reader: &mut impl Read, writer: &mut impl Write) -> Result<(), String> {
    let mut wire = 0;
    let Some(payload) = read_frame(reader, &mut wire, INPUT_LIMIT)? else {
        return Err("Session initialization missing".into());
    };
    let request: RequestFrame =
        serde_json::from_slice(&payload).map_err(|error| error.to_string())?;
    if request.version != VERSION
        || request.sequence != 0
        || request.expected_revision != 0
        || request.session_id == 0
    {
        return Err("Invalid session initialization envelope".into());
    }
    let FrameCommand::Init(initial) = request.command else {
        return Err("First session command must be Init".into());
    };
    let (mut realm, reply) = mg_sparkle::js_browser::PageRealm::start(initial);
    let mut revision = reply.revision;
    let mut terminal = reply.state != RealmState::Ready;
    write_frame(
        writer,
        &ReplyFrame {
            version: VERSION,
            generation: request.generation,
            session_id: request.session_id,
            sequence: 0,
            expected_revision: 0,
            reply,
        },
        &mut wire,
    )?;
    for sequence in 1..TRANSACTIONS {
        if terminal {
            return Ok(());
        }
        let Some(payload) = read_frame(reader, &mut wire, MAX_EVENT_BYTES)? else {
            return Ok(());
        };
        let event: RequestFrame =
            serde_json::from_slice(&payload).map_err(|error| error.to_string())?;
        if event.version != VERSION
            || event.generation != request.generation
            || event.session_id != request.session_id
            || event.sequence != sequence
            || event.expected_revision != revision
        {
            return Err("Invalid script-session event envelope".into());
        }
        let FrameCommand::Event(input) = event.command else {
            return Err("Repeated session initialization".into());
        };
        validate_input(&input)?;
        let reply = realm
            .as_mut()
            .ok_or("Script realm is unavailable")?
            .dispatch(input);
        revision = reply.revision;
        terminal = reply.state != RealmState::Ready;
        write_frame(
            writer,
            &ReplyFrame {
                version: VERSION,
                generation: request.generation,
                session_id: request.session_id,
                sequence,
                expected_revision: event.expected_revision,
                reply,
            },
            &mut wire,
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
        Request { url: "https://example.test/session-selftest".into(), html:
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
                state: RealmState::Ready,
                acknowledgements: Vec::new(),
            },
        }
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
