//! Immutable four-resource fixture for the separately frozen Window cohort.
//!
//! This is an owned test endpoint, not an arbitrary HTTP server or a confinement
//! boundary. `delivered` means that this server wrote the complete pinned body
//! and successfully flushed its socket. SourceInventory independently verifies
//! what Browser actually received. A successful seal proves the accepted-socket
//! prefix was drained through WouldBlock, the listener closed, and the only
//! server thread joined. It does not enumerate future packets or prove peer ACKs.
//!
//! No relative cleanup allowance is created here. The caller supplies the same
//! absolute file deadlines and must reject an overrun. Explicit seal/join is
//! required before trusting the audit. Drop signals cancellation and joins the
//! already-owned thread; if OS scheduling prevents timely destruction, the outer
//! owned-host deadline is the terminal boundary, never a clean fixture result.

use super::source::{CaseSpec, HELPERS};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{self, Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const MAX_ATTEMPTS: usize = 32;
pub const MAX_AUDIT_BYTES: usize = 1024 * 1024;
const MAX_FAULTS: usize = 64;
const MAX_HEADER_BYTES: usize = 8192;
const MAX_HEADER_FIELDS: usize = 64;
const MAX_RAW_PREFIX: usize = 2048;
const MAX_TARGET_BYTES: usize = 512;
const MAX_METHOD_BYTES: usize = 16;
const POLL: Duration = Duration::from_micros(250);
const WHOLE_LIMIT: Duration = Duration::from_secs(25);
const FINAL_RESERVE: Duration = Duration::from_secs(3);
// Each variable request string is ASCII and independently bounded. This generous
// JSON upper bound includes escaping, field names, enum tags and all scalar data.
const AUDIT_STRUCTURAL_UPPER_BOUND: usize = 4096 + MAX_ATTEMPTS * 16_384 + MAX_FAULTS * 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum IoStage {
    Bind,
    ListenerMode,
    LocalAddress,
    Spawn,
    Accept,
    StreamMode,
    Read,
    Peek,
    Write,
    Flush,
    Shutdown,
}

/// Closed startup/ownership errors. Raw OS codes are diagnostic, not classifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FixtureError {
    UnknownCase,
    InputBytes,
    InvalidDeadlines,
    ForbiddenPorts,
    ReusedPort,
    LocalAddress,
    Io {
        stage: IoStage,
        raw_os_error: Option<i32>,
    },
    AlreadySealed,
    JoinDeadline,
    ThreadPanicked,
    AuditBounds,
}
impl fmt::Display for FixtureError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "immutable Window fixture: {self:?}")
    }
}
impl std::error::Error for FixtureError {}
pub type Result<T> = std::result::Result<T, FixtureError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Route {
    Document,
    Harness,
    Vendor,
    Parsing,
}
impl Route {
    fn index(self) -> usize {
        match self {
            Self::Document => 0,
            Self::Harness => 1,
            Self::Vendor => 2,
            Self::Parsing => 3,
        }
    }
    fn mime(self) -> &'static str {
        match self {
            Self::Document => "text/html; charset=utf-8",
            _ => "application/javascript; charset=utf-8",
        }
    }
    fn body(self, case: &'static CaseSpec) -> &'static [u8] {
        match self {
            Self::Document => case.html().as_bytes(),
            _ => HELPERS[self.index() - 1].body.as_bytes(),
        }
    }
    fn hash(self, case: &'static CaseSpec) -> &'static str {
        match self {
            Self::Document => case.sha256(),
            _ => HELPERS[self.index() - 1].sha256,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FaultKind {
    AttemptLimit,
    Accept,
    StreamMode,
    Read,
    Peek,
    Write,
    Flush,
    Shutdown,
    HeaderLimit,
    HeaderIncomplete,
    HeaderSyntax,
    HeaderCount,
    RequestLine,
    Method,
    Target,
    Version,
    Host,
    Body,
    DuplicateRoute,
    LateAttempt,
    SealedInFlight,
    Deadline,
    DrainUnconfirmed,
    AuditBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Fault {
    pub kind: FaultKind,
    pub attempt: Option<u8>,
    pub raw_os_error: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct Attempt {
    pub ordinal: u8,
    pub accepted_elapsed_ns: u64,
    pub completed_elapsed_ns: u64,
    pub accepted_after_seal: bool,
    pub seal_seen_in_flight: bool,
    pub request_bytes_seen: usize,
    pub header_complete: bool,
    pub raw_prefix_hex: String,
    pub raw_prefix_omitted_bytes: usize,
    pub method: Option<String>,
    pub target: Option<String>,
    pub route: Option<Route>,
    pub response_status: Option<u16>,
    pub mime: Option<&'static str>,
    pub expected_body_bytes: usize,
    pub expected_body_sha256: Option<&'static str>,
    pub header_bytes_written: usize,
    pub body_bytes_written: usize,
    pub flushed: bool,
    pub delivered: bool,
    pub fault: Option<FaultKind>,
}
impl Attempt {
    fn new(ordinal: usize, elapsed: u64, late: bool) -> Self {
        Self {
            ordinal: ordinal as u8,
            accepted_elapsed_ns: elapsed,
            completed_elapsed_ns: elapsed,
            accepted_after_seal: late,
            seal_seen_in_flight: false,
            request_bytes_seen: 0,
            header_complete: false,
            raw_prefix_hex: String::new(),
            raw_prefix_omitted_bytes: 0,
            method: None,
            target: None,
            route: None,
            response_status: None,
            mime: None,
            expected_body_bytes: 0,
            expected_body_sha256: None,
            header_bytes_written: 0,
            body_bytes_written: 0,
            flushed: false,
            delivered: false,
            fault: None,
        }
    }
    fn record_request(&mut self, bytes: &[u8]) {
        self.request_bytes_seen = bytes.len();
        let prefix = &bytes[..bytes.len().min(MAX_RAW_PREFIX)];
        self.raw_prefix_hex = hex(prefix);
        self.raw_prefix_omitted_bytes = bytes.len() - prefix.len();
        if let Some(line_end) = bytes.windows(2).position(|value| value == b"\r\n") {
            let mut words = bytes[..line_end].split(|byte| *byte == b' ');
            self.method = words
                .next()
                .and_then(|part| ascii_text(part, MAX_METHOD_BYTES));
            self.target = words
                .next()
                .and_then(|part| ascii_text(part, MAX_TARGET_BYTES));
        }
    }
    fn prepare_response(&mut self, route: Route, case: &'static CaseSpec) {
        self.route = Some(route);
        self.response_status = Some(200);
        self.mime = Some(route.mime());
        self.expected_body_bytes = route.body(case).len();
        self.expected_body_sha256 = Some(route.hash(case));
    }
    fn valid_delivery(&self) -> bool {
        !self.accepted_after_seal
            && !self.seal_seen_in_flight
            && self.fault.is_none()
            && self.header_complete
            && self.route.is_some()
            && self.response_status == Some(200)
            && self.mime == self.route.map(Route::mime)
            && self.expected_body_sha256.is_some()
            && self.header_bytes_written
                == self
                    .route
                    .map(|route| response_header(route, self.expected_body_bytes).len())
                    .unwrap_or(0)
            && self.body_bytes_written == self.expected_body_bytes
            && self.flushed
            && self.delivered
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Seal {
    pub requested_by_host: bool,
    pub admission_reached: bool,
    pub in_flight_at_seal: bool,
    pub accepted_after_seal: usize,
    pub drain_would_block: bool,
    pub listener_closed: bool,
    pub thread_finished: bool,
    pub thread_joined: bool,
    pub join_before_deadline: bool,
}

/// Immutable only after the owned server thread has joined. No Browser verdict.
#[derive(Debug, Serialize)]
pub struct FixtureAudit {
    pub schema_version: u8,
    pub scope: &'static str,
    pub case: &'static str,
    pub origin: String,
    pub document_url: String,
    pub port: u16,
    pub attempts_total: usize,
    pub attempts: Vec<Attempt>,
    pub faults: Vec<Fault>,
    pub omitted_faults: u64,
    pub seal: Seal,
    pub cleanup_confirmed: bool,
    pub clean_attributed_prefix: bool,
    pub complete_four_gets: bool,
}
impl FixtureAudit {
    fn new(case: &'static CaseSpec, origin: String, document_url: String, port: u16) -> Self {
        Self {
            schema_version: 1,
            scope: "Pinned immutable four-route accepted-socket audit; not peer receipt or Browser verdict",
            case: case.path(),
            origin,
            document_url,
            port,
            attempts_total: 0,
            attempts: Vec::with_capacity(MAX_ATTEMPTS),
            faults: Vec::with_capacity(MAX_FAULTS),
            omitted_faults: 0,
            seal: Seal::default(),
            cleanup_confirmed: false,
            clean_attributed_prefix: false,
            complete_four_gets: false,
        }
    }
    pub fn complete_four_gets(&self) -> bool {
        self.complete_four_gets
    }
    pub fn clean_attributed_prefix(&self) -> bool {
        self.clean_attributed_prefix
    }
    pub fn cleanup_confirmed(&self) -> bool {
        self.cleanup_confirmed
    }
    pub fn attempts_total(&self) -> usize {
        self.attempts_total
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn document_url(&self) -> &str {
        &self.document_url
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn first_fault(&self) -> Option<&Fault> {
        self.faults.first()
    }

    fn add_fault(&mut self, kind: FaultKind, attempt: Option<usize>, raw: Option<i32>) {
        if self.faults.len() < MAX_FAULTS {
            self.faults.push(Fault {
                kind,
                attempt: attempt.map(|index| index as u8),
                raw_os_error: raw,
            });
        } else {
            self.omitted_faults = self.omitted_faults.saturating_add(1);
        }
    }
    fn begin(&mut self, elapsed: u64, late: bool) -> Option<usize> {
        if self.attempts.len() == MAX_ATTEMPTS {
            self.add_fault(FaultKind::AttemptLimit, None, None);
            return None;
        }
        let index = self.attempts.len();
        self.attempts.push(Attempt::new(index, elapsed, late));
        self.attempts_total += 1;
        if late {
            self.seal.accepted_after_seal += 1;
        }
        Some(index)
    }
    fn fail_attempt(&mut self, index: usize, kind: FaultKind, error: Option<&io::Error>) {
        if self.attempts[index].fault.is_none() {
            self.attempts[index].fault = Some(kind);
        }
        if kind == FaultKind::SealedInFlight {
            self.attempts[index].seal_seen_in_flight = true;
            self.seal.in_flight_at_seal = true;
        }
        self.add_fault(kind, Some(index), error.and_then(io::Error::raw_os_error));
    }
    fn finish(&mut self) -> Result<()> {
        if !self.within_bounds() {
            return Err(FixtureError::AuditBounds);
        }
        self.cleanup_confirmed = self.seal.drain_would_block
            && self.seal.listener_closed
            && self.seal.thread_finished
            && self.seal.thread_joined
            && self.seal.join_before_deadline;
        let mut seen = [false; 4];
        let unique = self.attempts.iter().all(|attempt| {
            let Some(route) = attempt.route else {
                return false;
            };
            let fresh = !seen[route.index()];
            seen[route.index()] = true;
            fresh && attempt.valid_delivery()
        });
        self.clean_attributed_prefix = self.cleanup_confirmed
            && self.faults.is_empty()
            && self.omitted_faults == 0
            && unique
            && self.attempts_total == self.attempts.len();
        self.complete_four_gets = self.clean_attributed_prefix
            && self.attempts_total == 4
            && seen.iter().all(|value| *value);
        Ok(())
    }
    fn within_bounds(&self) -> bool {
        AUDIT_STRUCTURAL_UPPER_BOUND <= MAX_AUDIT_BYTES
            && self.origin.len() <= 32
            && self.document_url.len() <= 192
            && self.case.len() <= 128
            && self.attempts.len() <= MAX_ATTEMPTS
            && self.faults.len() <= MAX_FAULTS
            && self.attempts_total == self.attempts.len()
            && self.attempts.iter().all(|attempt| {
                attempt.raw_prefix_hex.len() <= MAX_RAW_PREFIX * 2
                    && attempt
                        .raw_prefix_hex
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
                    && attempt
                        .method
                        .as_ref()
                        .is_none_or(|value| bounded_ascii(value, MAX_METHOD_BYTES))
                    && attempt
                        .target
                        .as_ref()
                        .is_none_or(|value| bounded_ascii(value, MAX_TARGET_BYTES))
                    && attempt.request_bytes_seen <= MAX_HEADER_BYTES + 1
            })
    }
}

struct Control {
    epoch: Instant,
    admission: Instant,
    whole: Instant,
    seal: AtomicBool,
    deadline_ns: AtomicU64,
}
impl Control {
    fn elapsed(&self, now: Instant) -> u64 {
        nanos(now.saturating_duration_since(self.epoch))
    }
    fn deadline(&self) -> Instant {
        self.epoch + Duration::from_nanos(self.deadline_ns.load(Ordering::Acquire))
    }
    fn narrow_and_seal(&self, deadline: Instant) {
        self.deadline_ns
            .fetch_min(self.elapsed(deadline.min(self.whole)), Ordering::AcqRel);
        self.seal.store(true, Ordering::Release);
    }
    fn expired(&self, now: Instant) -> bool {
        now >= self.deadline().min(self.whole)
    }
    fn sealing(&self, now: Instant) -> bool {
        self.seal.load(Ordering::Acquire) || now >= self.admission
    }
    fn io_permission(&self, now: Instant) -> std::result::Result<(), FaultKind> {
        if self.expired(now) {
            Err(FaultKind::Deadline)
        } else if self.sealing(now) {
            Err(FaultKind::SealedInFlight)
        } else {
            Ok(())
        }
    }
    fn pause(&self) {
        let remaining = self
            .deadline()
            .min(self.admission)
            .saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            thread::sleep(remaining.min(POLL));
        }
    }
}

/// The listener and every accepted connection belong to exactly one thread.
pub struct Fixture {
    origin: String,
    document_url: String,
    port: u16,
    control: Arc<Control>,
    thread: Option<JoinHandle<FixtureAudit>>,
    seal_started: bool,
}
impl Fixture {
    pub fn start(
        case: &'static CaseSpec,
        forbidden_ports: &[u16],
        absolute_admission: Instant,
        absolute_whole: Instant,
    ) -> Result<Self> {
        let now = Instant::now();
        validate_deadlines(now, absolute_admission, absolute_whole)?;
        validate_ports(forbidden_ports)?;
        verify_inputs(case)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| io_error(IoStage::Bind, error))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| io_error(IoStage::ListenerMode, error))?;
        let address = listener
            .local_addr()
            .map_err(|error| io_error(IoStage::LocalAddress, error))?;
        let SocketAddr::V4(address) = address else {
            return Err(FixtureError::LocalAddress);
        };
        if *address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
            return Err(FixtureError::LocalAddress);
        }
        let port = address.port();
        validate_chosen_port(forbidden_ports, port)?;
        let origin = format!("http://127.0.0.1:{port}");
        let document_url = format!("{origin}/{}", case.path());
        let authority = format!("127.0.0.1:{port}");
        let control = Arc::new(Control {
            epoch: now,
            admission: absolute_admission,
            whole: absolute_whole,
            seal: AtomicBool::new(false),
            deadline_ns: AtomicU64::new(nanos(absolute_whole.duration_since(now))),
        });
        let audit = FixtureAudit::new(case, origin.clone(), document_url.clone(), port);
        if !audit.within_bounds() || Instant::now() >= absolute_admission {
            return Err(FixtureError::InvalidDeadlines);
        }
        let thread_control = Arc::clone(&control);
        let handle = thread::Builder::new()
            .name("wpt-window-fixture".to_owned())
            .spawn(move || serve(listener, case, authority, audit, thread_control))
            .map_err(|error| io_error(IoStage::Spawn, error))?;
        Ok(Self {
            origin,
            document_url,
            port,
            control,
            thread: Some(handle),
            seal_started: false,
        })
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn document_url(&self) -> &str {
        &self.document_url
    }
    pub fn port(&self) -> u16 {
        self.port
    }

    /// One explicit seal. On error no final audit exists and the handle remains
    /// owned until Drop. The caller must stop, not reuse the port or admit a case.
    pub fn seal_and_join(&mut self, deadline: Instant) -> Result<FixtureAudit> {
        if self.seal_started {
            return Err(FixtureError::AlreadySealed);
        }
        self.seal_started = true;
        let deadline = deadline.min(self.control.whole);
        self.control.narrow_and_seal(deadline);
        loop {
            if Instant::now() >= deadline {
                return Err(FixtureError::JoinDeadline);
            }
            let Some(handle) = self.thread.as_ref() else {
                return Err(FixtureError::AlreadySealed);
            };
            if handle.is_finished() {
                break;
            }
            thread::sleep(deadline.saturating_duration_since(Instant::now()).min(POLL));
        }
        let handle = self.thread.take().ok_or(FixtureError::AlreadySealed)?;
        let mut audit = handle.join().map_err(|_| FixtureError::ThreadPanicked)?;
        if Instant::now() >= deadline {
            return Err(FixtureError::JoinDeadline);
        }
        audit.seal.thread_joined = true;
        audit.seal.join_before_deadline = true;
        audit.finish()?;
        if Instant::now() >= deadline {
            return Err(FixtureError::JoinDeadline);
        }
        Ok(audit)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            self.control.narrow_and_seal(Instant::now());
            // No detached handler and no renewed grace. A stuck OS thread means
            // the enclosing host exceeds its owner deadline and cannot be used.
            let _ = handle.join();
        }
    }
}

fn io_error(stage: IoStage, error: io::Error) -> FixtureError {
    FixtureError::Io {
        stage,
        raw_os_error: error.raw_os_error(),
    }
}
fn nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 15) as usize] as char);
    }
    text
}
fn bounded_ascii(value: &str, limit: usize) -> bool {
    value.len() <= limit && value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}
fn ascii_text(value: &[u8], limit: usize) -> Option<String> {
    if value.len() > limit || !value.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return None;
    }
    String::from_utf8(value.to_vec()).ok()
}
fn validate_ports(ports: &[u16]) -> Result<()> {
    if ports.len() > 16
        || ports.iter().any(|port| *port == 0)
        || ports.windows(2).any(|pair| pair[0] >= pair[1])
    {
        Err(FixtureError::ForbiddenPorts)
    } else {
        Ok(())
    }
}
fn validate_chosen_port(forbidden_ports: &[u16], chosen: u16) -> Result<()> {
    if chosen == 0 {
        Err(FixtureError::LocalAddress)
    } else if forbidden_ports.binary_search(&chosen).is_ok() {
        Err(FixtureError::ReusedPort)
    } else {
        Ok(())
    }
}
fn validate_deadlines(now: Instant, admission: Instant, whole: Instant) -> Result<()> {
    if now >= admission
        || admission >= whole
        || whole.saturating_duration_since(now) > WHOLE_LIMIT
        || whole.saturating_duration_since(admission) < FINAL_RESERVE
    {
        Err(FixtureError::InvalidDeadlines)
    } else {
        Ok(())
    }
}
fn verify_inputs(case: &'static CaseSpec) -> Result<()> {
    let actual = CaseSpec::by_path(case.path()).map_err(|_| FixtureError::UnknownCase)?;
    if !std::ptr::eq(actual, case) {
        return Err(FixtureError::UnknownCase);
    }
    if case.path().len() > 128
        || case.html().len() != case.bytes()
        || hex(&Sha256::digest(case.html().as_bytes())) != case.sha256()
        || HELPERS.iter().any(|helper| {
            helper.body.len() != helper.bytes
                || hex(&Sha256::digest(helper.body.as_bytes())) != helper.sha256
        })
    {
        return Err(FixtureError::InputBytes);
    }
    Ok(())
}

/// No generic URL normalization: only four exact origin-form paths exist.
fn parse_request(
    bytes: &[u8],
    authority: &str,
    document_path: &str,
) -> std::result::Result<Route, FaultKind> {
    if bytes.len() > MAX_HEADER_BYTES {
        return Err(FaultKind::HeaderLimit);
    }
    let end = bytes
        .windows(4)
        .position(|value| value == b"\r\n\r\n")
        .ok_or(FaultKind::HeaderIncomplete)?
        + 4;
    if end != bytes.len() {
        return Err(FaultKind::Body);
    }
    if bytes.iter().any(|byte| *byte == 0 || *byte >= 0x80) {
        return Err(FaultKind::HeaderSyntax);
    }
    let text = std::str::from_utf8(&bytes[..end - 2]).map_err(|_| FaultKind::HeaderSyntax)?;
    let mut lines = text.split("\r\n");
    let request = lines.next().ok_or(FaultKind::RequestLine)?;
    let words: Vec<_> = request.split(' ').collect();
    if words.len() != 3 || words.iter().any(|word| word.is_empty()) {
        return Err(FaultKind::RequestLine);
    }
    if words[0] != "GET" {
        return Err(FaultKind::Method);
    }
    if !matches!(words[2], "HTTP/1.0" | "HTTP/1.1") {
        return Err(FaultKind::Version);
    }
    let route = if words[1] == document_path {
        Route::Document
    } else if words[1] == HELPERS[0].route {
        Route::Harness
    } else if words[1] == HELPERS[1].route {
        Route::Vendor
    } else if words[1] == HELPERS[2].route {
        Route::Parsing
    } else {
        return Err(FaultKind::Target);
    };
    let mut host = false;
    let mut content_length = false;
    let mut count = 0;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        count += 1;
        if count > MAX_HEADER_FIELDS {
            return Err(FaultKind::HeaderCount);
        }
        let (name, value) = line.split_once(':').ok_or(FaultKind::HeaderSyntax)?;
        if name.is_empty()
            || !name.bytes().all(header_token)
            || value
                .bytes()
                .any(|byte| !matches!(byte, b'\t' | 0x20..=0x7e))
        {
            return Err(FaultKind::HeaderSyntax);
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("host") {
            if host || value != authority {
                return Err(FaultKind::Host);
            }
            host = true;
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(FaultKind::Body);
        } else if name.eq_ignore_ascii_case("content-length") {
            if content_length || value != "0" {
                return Err(FaultKind::Body);
            }
            content_length = true;
        }
    }
    if !host {
        return Err(FaultKind::Host);
    }
    Ok(route)
}
fn header_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}
fn response_header(route: Route, body_bytes: usize) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {body_bytes}\r\nConnection: close\r\n\r\n",
        route.mime()
    )
}

/// Shared with fake-I/O controls: partial writes and failed flush never deliver.
fn write_response<W: Write>(
    writer: &mut W,
    header: &[u8],
    body: &[u8],
    attempt: &mut Attempt,
    mut permission: impl FnMut() -> std::result::Result<(), FaultKind>,
    mut pause: impl FnMut(),
) -> std::result::Result<(), (FaultKind, Option<io::Error>)> {
    for (bytes, header_part) in [(header, true), (body, false)] {
        let mut written = 0;
        while written < bytes.len() {
            permission().map_err(|kind| (kind, None))?;
            match writer.write(&bytes[written..]) {
                Ok(0) => return Err((FaultKind::Write, None)),
                Ok(count) if count <= bytes.len() - written => {
                    written += count;
                    if header_part {
                        attempt.header_bytes_written += count;
                    } else {
                        attempt.body_bytes_written += count;
                    }
                }
                Ok(_) => return Err((FaultKind::Write, None)),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => pause(),
                Err(error) => return Err((FaultKind::Write, Some(error))),
            }
        }
    }
    loop {
        permission().map_err(|kind| (kind, None))?;
        match writer.flush() {
            Ok(()) => {
                attempt.flushed = true;
                attempt.delivered = true;
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => pause(),
            Err(error) => return Err((FaultKind::Flush, Some(error))),
        }
    }
}

fn serve(
    listener: TcpListener,
    case: &'static CaseSpec,
    authority: String,
    mut audit: FixtureAudit,
    control: Arc<Control>,
) -> FixtureAudit {
    let document_path = format!("/{}", case.path());
    let mut seen = [false; 4];
    loop {
        let now = Instant::now();
        if control.expired(now) {
            audit.add_fault(FaultKind::Deadline, None, None);
            audit.add_fault(FaultKind::DrainUnconfirmed, None, None);
            break;
        }
        let sealing = control.sealing(now);
        audit.seal.requested_by_host |= control.seal.load(Ordering::Acquire);
        audit.seal.admission_reached |= now >= control.admission;
        if audit.attempts.len() == MAX_ATTEMPTS {
            audit.add_fault(FaultKind::AttemptLimit, None, None);
            audit.add_fault(FaultKind::DrainUnconfirmed, None, None);
            break;
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                // The bounded row exists before any request read or response.
                let index = audit
                    .begin(control.elapsed(Instant::now()), sealing)
                    .expect("prechecked bounded audit");
                if !peer.ip().is_loopback() {
                    audit.fail_attempt(index, FaultKind::Host, None);
                } else if let Err(error) = stream.set_nonblocking(true) {
                    audit.fail_attempt(index, FaultKind::StreamMode, Some(&error));
                } else if sealing {
                    let mut bytes = [0; MAX_HEADER_BYTES + 1];
                    match stream.read(&mut bytes) {
                        Ok(count) => audit.attempts[index].record_request(&bytes[..count]),
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                            ) => {}
                        Err(error) => {
                            audit.add_fault(FaultKind::Read, Some(index), error.raw_os_error())
                        }
                    }
                    audit.fail_attempt(index, FaultKind::LateAttempt, None);
                } else {
                    connection(
                        &mut stream,
                        case,
                        &authority,
                        &document_path,
                        &mut seen,
                        index,
                        &mut audit,
                        &control,
                    );
                }
                if let Err(error) = stream.shutdown(Shutdown::Both) {
                    // NotConnected commonly means the peer already closed. All
                    // other failures make ownership evidence conservative.
                    if error.kind() != io::ErrorKind::NotConnected {
                        audit.fail_attempt(index, FaultKind::Shutdown, Some(&error));
                    }
                }
                drop(stream);
                audit.attempts[index].completed_elapsed_ns = control.elapsed(Instant::now());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if sealing {
                    audit.seal.drain_would_block = true;
                    break;
                }
                control.pause();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                audit.add_fault(FaultKind::Accept, None, error.raw_os_error());
                audit.add_fault(FaultKind::DrainUnconfirmed, None, None);
                break;
            }
        }
    }
    drop(listener);
    audit.seal.listener_closed = true;
    audit.seal.thread_finished = true;
    audit
}

fn connection(
    stream: &mut TcpStream,
    case: &'static CaseSpec,
    authority: &str,
    document_path: &str,
    seen: &mut [bool; 4],
    index: usize,
    audit: &mut FixtureAudit,
    control: &Control,
) {
    let mut bytes = Vec::with_capacity(MAX_HEADER_BYTES + 1);
    let read_result = loop {
        if let Err(kind) = control.io_permission(Instant::now()) {
            break Err((kind, None));
        }
        let mut chunk = [0; 1024];
        let remaining = (MAX_HEADER_BYTES + 1 - bytes.len()).min(chunk.len());
        if remaining == 0 {
            break Err((FaultKind::HeaderLimit, None));
        }
        match stream.read(&mut chunk[..remaining]) {
            Ok(0) => break Err((FaultKind::HeaderIncomplete, None)),
            Ok(count) => {
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.windows(4).any(|value| value == b"\r\n\r\n") {
                    break Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => control.pause(),
            Err(error) => break Err((FaultKind::Read, Some(error))),
        }
    };
    audit.attempts[index].record_request(&bytes);
    if let Err((kind, error)) = read_result {
        audit.fail_attempt(index, kind, error.as_ref());
        return;
    }
    audit.attempts[index].header_complete = true;
    let route = match parse_request(&bytes, authority, document_path) {
        Ok(route) => route,
        Err(kind) => {
            audit.fail_attempt(index, kind, None);
            return;
        }
    };
    audit.attempts[index].route = Some(route);
    let mut extra = [0; 1];
    match stream.peek(&mut extra) {
        Ok(count) if count != 0 => {
            audit.fail_attempt(index, FaultKind::Body, None);
            return;
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(error) => {
            audit.fail_attempt(index, FaultKind::Peek, Some(&error));
            return;
        }
    }
    if seen[route.index()] {
        audit.fail_attempt(index, FaultKind::DuplicateRoute, None);
        return;
    }
    seen[route.index()] = true;
    audit.attempts[index].prepare_response(route, case);
    let header = response_header(route, route.body(case).len());
    let result = write_response(
        stream,
        header.as_bytes(),
        route.body(case),
        &mut audit.attempts[index],
        || control.io_permission(Instant::now()),
        || control.pause(),
    );
    if let Err((kind, error)) = result {
        audit.fail_attempt(index, kind, error.as_ref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn case() -> &'static CaseSpec {
        CaseSpec::by_path("css/css-flexbox/parsing/flex-basis-invalid.html").unwrap()
    }
    fn request(path: &str, fields: &str) -> Vec<u8> {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n{fields}\r\n").into_bytes()
    }
    fn audit() -> FixtureAudit {
        FixtureAudit::new(
            case(),
            "http://127.0.0.1:12345".into(),
            format!("http://127.0.0.1:12345/{}", case().path()),
            12345,
        )
    }
    fn joined(audit: &mut FixtureAudit) {
        audit.seal.drain_would_block = true;
        audit.seal.listener_closed = true;
        audit.seal.thread_finished = true;
        audit.seal.thread_joined = true;
        audit.seal.join_before_deadline = true;
    }
    fn delivery(audit: &mut FixtureAudit, route: Route) {
        let index = audit.begin(0, false).unwrap();
        let attempt = &mut audit.attempts[index];
        attempt.header_complete = true;
        attempt.prepare_response(route, case());
        attempt.header_bytes_written = response_header(route, route.body(case()).len()).len();
        attempt.body_bytes_written = attempt.expected_body_bytes;
        attempt.flushed = true;
        attempt.delivered = true;
    }

    #[test]
    fn canonical_forbidden_ports_and_existing_absolute_reserve() {
        assert_eq!(validate_ports(&[]), Ok(()));
        assert_eq!(validate_ports(&[1, 12345, 65535]), Ok(()));
        for ports in [vec![0], vec![2, 2], vec![3, 2], (1..=17).collect()] {
            assert_eq!(validate_ports(&ports), Err(FixtureError::ForbiddenPorts));
        }
        assert_eq!(
            validate_chosen_port(&[12, 12345], 12345),
            Err(FixtureError::ReusedPort)
        );
        assert_eq!(validate_chosen_port(&[12, 12345], 12346), Ok(()));
        assert_eq!(
            validate_chosen_port(&[], 0),
            Err(FixtureError::LocalAddress)
        );
        let now = Instant::now();
        assert_eq!(
            validate_deadlines(
                now,
                now + Duration::from_secs(22),
                now + Duration::from_secs(25)
            ),
            Ok(())
        );
        assert_eq!(
            validate_deadlines(
                now,
                now + Duration::from_secs(22),
                now + Duration::from_secs(24)
            ),
            Err(FixtureError::InvalidDeadlines)
        );
        assert_eq!(
            validate_deadlines(
                now,
                now + Duration::from_secs(23),
                now + Duration::from_secs(26)
            ),
            Err(FixtureError::InvalidDeadlines)
        );
        assert_eq!(
            validate_deadlines(now, now, now + Duration::from_secs(25)),
            Err(FixtureError::InvalidDeadlines)
        );
    }

    #[test]
    fn exact_four_routes_host_and_javascript_mime() {
        let document = format!("/{}", case().path());
        for (path, route) in [
            (&document[..], Route::Document),
            (HELPERS[0].route, Route::Harness),
            (HELPERS[1].route, Route::Vendor),
            (HELPERS[2].route, Route::Parsing),
        ] {
            assert_eq!(
                parse_request(
                    &request(path, "Content-Length: 0\r\n"),
                    "127.0.0.1:12345",
                    &document
                ),
                Ok(route)
            );
            let header = response_header(route, route.body(case()).len());
            assert!(header.contains(&format!("Content-Type: {}\r\n", route.mime())));
            assert!(header.contains(&format!("Content-Length: {}\r\n", route.body(case()).len())));
            assert!(header.ends_with("Connection: close\r\n\r\n"));
        }
    }

    #[test]
    fn no_alias_query_foreign_host_method_or_body() {
        let document = format!("/{}", case().path());
        for path in [
            "/resources/testharness.js?x",
            "/resources/./testharness.js",
            "/resources/%74estharness.js",
            "http://127.0.0.1:12345/resources/testharness.js",
            "/unknown",
        ] {
            assert_eq!(
                parse_request(&request(path, ""), "127.0.0.1:12345", &document),
                Err(FaultKind::Target)
            );
        }
        for fields in [
            "Host: 127.0.0.1:12345\r\n",
            "Content-Length: 1\r\n",
            "Content-Length: 0\r\nContent-Length: 0\r\n",
            "Transfer-Encoding: chunked\r\n",
            "Content-Length: 00\r\n",
        ] {
            assert!(
                parse_request(
                    &request(HELPERS[0].route, fields),
                    "127.0.0.1:12345",
                    &document
                )
                .is_err()
            );
        }
        let wrong = b"GET /resources/testharness.js HTTP/1.1\r\nHost: localhost:12345\r\n\r\n";
        assert_eq!(
            parse_request(wrong, "127.0.0.1:12345", &document),
            Err(FaultKind::Host)
        );
        let post = b"POST /resources/testharness.js HTTP/1.1\r\nHost: 127.0.0.1:12345\r\n\r\n";
        assert_eq!(
            parse_request(post, "127.0.0.1:12345", &document),
            Err(FaultKind::Method)
        );
        let mut pipelined = request(HELPERS[0].route, "");
        pipelined.extend_from_slice(b"GET / HTTP/1.1\r\n\r\n");
        assert_eq!(
            parse_request(&pipelined, "127.0.0.1:12345", &document),
            Err(FaultKind::Body)
        );
    }

    #[test]
    fn malformed_headers_and_limits_are_not_silent_prefixes() {
        let document = format!("/{}", case().path());
        for raw in [
            b"GET /resources/testharness.js HTTP/1.1\nHost: x\n\n".as_slice(),
            b"GET  /resources/testharness.js HTTP/1.1\r\nHost: x\r\n\r\n",
            b"GET /resources/testharness.js HTTP/1.1\r\nHost : x\r\n\r\n",
            b"GET /resources/testharness.js HTTP/1.1\r\n\r\n",
        ] {
            assert!(parse_request(raw, "127.0.0.1:12345", &document).is_err());
        }
        assert_eq!(
            parse_request(
                &vec![b'x'; MAX_HEADER_BYTES + 1],
                "127.0.0.1:12345",
                &document
            ),
            Err(FaultKind::HeaderLimit)
        );
        let fields = "X: y\r\n".repeat(MAX_HEADER_FIELDS);
        assert_eq!(
            parse_request(
                &request(HELPERS[0].route, &fields),
                "127.0.0.1:12345",
                &document
            ),
            Err(FaultKind::HeaderCount)
        );
    }

    struct FakeWriter {
        bytes: Vec<u8>,
        chunk: usize,
        fail_at: Option<usize>,
        flush_error: bool,
        blocked_once: bool,
    }
    impl Write for FakeWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.blocked_once {
                self.blocked_once = false;
                return Err(io::ErrorKind::WouldBlock.into());
            }
            if self.fail_at.is_some_and(|limit| self.bytes.len() >= limit) {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let count = bytes.len().min(self.chunk).min(
                self.fail_at
                    .unwrap_or(usize::MAX)
                    .saturating_sub(self.bytes.len()),
            );
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.flush_error {
                Err(io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(())
            }
        }
    }
    fn writer() -> FakeWriter {
        FakeWriter {
            bytes: Vec::new(),
            chunk: 3,
            fail_at: None,
            flush_error: false,
            blocked_once: true,
        }
    }

    #[test]
    fn fake_io_requires_full_body_and_flush_not_headers() {
        let mut output = writer();
        let mut attempt = Attempt::new(0, 0, false);
        attempt.header_complete = true;
        attempt.prepare_response(Route::Document, case());
        let header = response_header(Route::Document, case().bytes());
        let pauses = Cell::new(0);
        write_response(
            &mut output,
            header.as_bytes(),
            case().html().as_bytes(),
            &mut attempt,
            || Ok(()),
            || pauses.set(pauses.get() + 1),
        )
        .unwrap();
        assert_eq!(pauses.get(), 1);
        assert_eq!(
            output.bytes,
            [header.as_bytes(), case().html().as_bytes()].concat()
        );
        assert!(attempt.valid_delivery());
        for (fail_at, flush_error) in [(Some(header.len()), false), (None, true)] {
            let mut output = writer();
            output.fail_at = fail_at;
            output.flush_error = flush_error;
            let mut attempt = Attempt::new(0, 0, false);
            attempt.prepare_response(Route::Document, case());
            assert!(
                write_response(
                    &mut output,
                    header.as_bytes(),
                    case().html().as_bytes(),
                    &mut attempt,
                    || Ok(()),
                    || {}
                )
                .is_err()
            );
            assert!(!attempt.delivered);
            assert!(!attempt.valid_delivery());
        }
    }

    #[test]
    fn fake_clock_seal_and_deadline_stop_pending_io_without_new_allowance() {
        let now = Instant::now();
        let control = Control {
            epoch: now,
            admission: now + Duration::from_secs(22),
            whole: now + Duration::from_secs(25),
            seal: AtomicBool::new(false),
            deadline_ns: AtomicU64::new(25_000_000_000),
        };
        assert_eq!(control.io_permission(now + Duration::from_secs(21)), Ok(()));
        assert_eq!(
            control.io_permission(now + Duration::from_secs(22)),
            Err(FaultKind::SealedInFlight)
        );
        control.narrow_and_seal(now + Duration::from_millis(22_900));
        assert_eq!(
            control.io_permission(now + Duration::from_secs(20)),
            Err(FaultKind::SealedInFlight)
        );
        assert_eq!(
            control.io_permission(now + Duration::from_millis(22_900)),
            Err(FaultKind::Deadline)
        );
        control.narrow_and_seal(now + Duration::from_secs(25));
        assert_eq!(control.deadline(), now + Duration::from_millis(22_900));
        let checks = Cell::new(0);
        let mut output = writer();
        output.blocked_once = false;
        let mut attempt = Attempt::new(0, 0, false);
        let result = write_response(
            &mut output,
            b"header",
            b"body",
            &mut attempt,
            || {
                checks.set(checks.get() + 1);
                if checks.get() == 2 {
                    Err(FaultKind::SealedInFlight)
                } else {
                    Ok(())
                }
            },
            || {},
        );
        assert!(matches!(result, Err((FaultKind::SealedInFlight, None))));
        assert_eq!(output.bytes, b"hea");
        assert!(!attempt.delivered);
    }

    #[test]
    fn immutable_audit_distinguishes_four_deliveries_and_clean_negative_prefix() {
        let mut full = audit();
        for route in [
            Route::Document,
            Route::Harness,
            Route::Vendor,
            Route::Parsing,
        ] {
            delivery(&mut full, route);
        }
        joined(&mut full);
        full.finish().unwrap();
        assert!(full.complete_four_gets());
        assert!(full.clean_attributed_prefix());
        assert!(full.cleanup_confirmed());
        let mut prefix = audit();
        delivery(&mut prefix, Route::Document);
        joined(&mut prefix);
        prefix.finish().unwrap();
        assert!(!prefix.complete_four_gets());
        assert!(prefix.clean_attributed_prefix());
        // Only the host can pair this audited prefix with a known semantic cause.
        for mutation in 0..5 {
            let mut bad = audit();
            delivery(&mut bad, Route::Document);
            joined(&mut bad);
            match mutation {
                0 => delivery(&mut bad, Route::Document),
                1 => {
                    bad.attempts[0].delivered = false;
                }
                2 => {
                    bad.attempts[0].accepted_after_seal = true;
                }
                3 => {
                    bad.seal.drain_would_block = false;
                }
                _ => bad.add_fault(FaultKind::Accept, None, None),
            }
            bad.finish().unwrap();
            assert!(!bad.clean_attributed_prefix());
            assert!(!bad.complete_four_gets());
        }
    }

    #[test]
    fn attempts_fault_prefix_and_serialized_audit_stay_bounded() {
        let mut value = audit();
        for index in 0..MAX_ATTEMPTS {
            let row = value.begin(index as u64, false).unwrap();
            let request = vec![b'"'; MAX_HEADER_BYTES + 1];
            value.attempts[row].record_request(&request);
            value.attempts[row].method = Some("\\".repeat(MAX_METHOD_BYTES));
            value.attempts[row].target = Some("\\".repeat(MAX_TARGET_BYTES));
        }
        assert!(value.begin(0, false).is_none());
        for _ in 0..MAX_FAULTS + 5 {
            value.add_fault(FaultKind::Body, Some(0), None);
        }
        assert_eq!(value.attempts_total(), MAX_ATTEMPTS);
        assert_eq!(value.faults.len(), MAX_FAULTS);
        assert!(value.omitted_faults > 0);
        joined(&mut value);
        value.finish().unwrap();
        assert!(!value.clean_attributed_prefix());
        let encoded = serde_json::to_vec(&value).unwrap();
        assert!(encoded.len() <= AUDIT_STRUCTURAL_UPPER_BOUND);
        assert!(encoded.len() < MAX_AUDIT_BYTES);
        assert_eq!(value.attempts[0].raw_prefix_hex.len(), MAX_RAW_PREFIX * 2);
        assert_eq!(
            value.attempts[0].raw_prefix_omitted_bytes,
            MAX_HEADER_BYTES + 1 - MAX_RAW_PREFIX
        );
    }
}
