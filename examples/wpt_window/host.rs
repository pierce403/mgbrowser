//! One fresh Browser, accepted ledger and owned immutable fixture per invocation.
//! The outer owner MUST separately prove normal leader/group cleanup and adopt
//! the unchanged semantic decision; a live host cannot attest its own teardown.
use super::{
    fixture::Fixture,
    guard::{HostFailure, HostSnapshot, HostState, InventoryScripts, ObserverTrace, RawTrace},
    source::{self, CaseSpec, SourceInventory, SourcePlan},
    verdict::{
        self, Cleanup, DeclaredTests, Delivery, FailureCause, HostFacts, Infrastructure,
        InitialEvidence, NativeHistory, Stop, TransportConsequence, Verdict,
    },
};
use mg_browser::platform::LinuxScripts;
use mg_chassis::{
    Browser, InitialScriptFailure, InitialScriptOutcome, InitialScriptStatus, ScriptWorkStatus,
    scripts::ScriptRuntime,
};
use mg_sparkle::{
    page_session::{
        LifecycleProgress, RealmState,
        observer::{
            ObservationView, ObserverContract, ObserverExpectation, ObserverFault,
            ObserverParentFailure, ObserverProgress, ObserverRecord, ReconciledName,
        },
    },
    paint::Fonts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const MAX_ARTIFACT: usize = 4 * 1024 * 1024;
const MAX_HTTP_ARTIFACT: usize = 1024 * 1024;
const MAX_INPUT: usize = 10 * 1024 * 1024;
const MAX_BINARY: u64 = 64 * 1024 * 1024;
const MANIFEST_BYTES: usize = include_bytes!("../../tools/wpt/window/corpus.json").len();
const SCOPE: &str =
    "Fixed upstream Window Browser semantic decision; requires independent outer-owner acceptance";
static CANCELED: AtomicBool = AtomicBool::new(false);
type Result<T> = std::result::Result<T, HostError>;

#[derive(Debug, Copy, Clone, Serialize)]
enum ErrorKind {
    Arguments,
    Input,
    Manifest,
    Font,
    Clock,
    Active,
    Interrupted,
    Fixture,
    Runtime,
    Artifact,
    Correlation,
}
#[derive(Debug, Serialize)]
pub struct HostError {
    kind: ErrorKind,
    detail: String,
}
impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}
impl std::error::Error for HostError {}
fn error(kind: ErrorKind, detail: impl std::fmt::Display) -> HostError {
    let text = detail.to_string();
    let mut end = text.len().min(2048);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    HostError {
        kind,
        detail: text[..end].into(),
    }
}
fn require(ok: bool, kind: ErrorKind, detail: &'static str) -> Result<()> {
    if ok { Ok(()) } else { Err(error(kind, detail)) }
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn lower_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

struct Args {
    case: &'static CaseSpec,
    run_id: u64,
    font: PathBuf,
    font_sha: String,
    font_bytes: usize,
    output: PathBuf,
    deadline_ns: u64,
    forbidden_ports: Vec<u16>,
}
fn ports(value: &str) -> Result<Vec<u16>> {
    require(
        value.len() <= 95,
        ErrorKind::Arguments,
        "Forbidden-port list exceeds bound",
    )?;
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let mut ports = Vec::new();
    for part in value.split(',') {
        require(
            ports.len() < 16 && !part.is_empty(),
            ErrorKind::Arguments,
            "Invalid forbidden-port count",
        )?;
        let port: u16 = part
            .parse()
            .map_err(|_| error(ErrorKind::Arguments, "Invalid forbidden port"))?;
        require(
            port != 0 && part == port.to_string() && ports.last().is_none_or(|last| *last < port),
            ErrorKind::Arguments,
            "Forbidden ports must be canonical, unique and increasing",
        )?;
        ports.push(port);
    }
    Ok(ports)
}
fn parse_args(arguments: impl IntoIterator<Item = String>) -> Result<Args> {
    let mut values = arguments.into_iter();
    let (mut case, mut font, mut font_sha, mut font_bytes, mut output, mut deadline, mut forbidden) =
        (None, None, None, None, None, None, None);
    let mut count = 0;
    while let Some(key) = values.next() {
        count += 1;
        require(count <= 7, ErrorKind::Arguments, "Too many host arguments")?;
        let value = values
            .next()
            .ok_or_else(|| error(ErrorKind::Arguments, "Every flag requires a value"))?;
        require(
            value.len() <= 4096,
            ErrorKind::Arguments,
            "Argument value exceeds bound",
        )?;
        let slot = match key.as_str() {
            "--case" => &mut case,
            "--font" => &mut font,
            "--font-sha" => &mut font_sha,
            "--font-bytes" => &mut font_bytes,
            "--output" => &mut output,
            "--deadline-monotonic-ns" => &mut deadline,
            "--forbidden-ports" => &mut forbidden,
            _ => return Err(error(ErrorKind::Arguments, "Unknown host flag")),
        };
        require(slot.is_none(), ErrorKind::Arguments, "Duplicate host flag")?;
        *slot = Some(value);
    }
    let selected = case.ok_or_else(|| error(ErrorKind::Arguments, "Missing --case"))?;
    let case = CaseSpec::by_path(&selected).map_err(|e| error(ErrorKind::Arguments, e))?;
    let ordinal = source::CASES
        .iter()
        .position(|candidate| candidate.path() == case.path())
        .ok_or_else(|| error(ErrorKind::Arguments, "Case absent from fixed order"))?;
    let font_sha = font_sha.ok_or_else(|| error(ErrorKind::Arguments, "Missing --font-sha"))?;
    require(
        lower_sha(&font_sha),
        ErrorKind::Arguments,
        "Font SHA must be lowercase hex64",
    )?;
    let font_bytes_text =
        font_bytes.ok_or_else(|| error(ErrorKind::Arguments, "Missing --font-bytes"))?;
    let font_bytes: usize = font_bytes_text
        .parse()
        .map_err(|_| error(ErrorKind::Arguments, "Invalid admitted font length"))?;
    require(
        font_bytes != 0 && font_bytes <= MAX_INPUT && font_bytes_text == font_bytes.to_string(),
        ErrorKind::Arguments,
        "Font length must be positive bounded canonical decimal",
    )?;
    let deadline_text =
        deadline.ok_or_else(|| error(ErrorKind::Arguments, "Missing absolute deadline"))?;
    let deadline_ns: u64 = deadline_text
        .parse()
        .map_err(|_| error(ErrorKind::Arguments, "Invalid absolute deadline"))?;
    require(
        deadline_ns != 0 && deadline_text == deadline_ns.to_string(),
        ErrorKind::Arguments,
        "Noncanonical absolute deadline",
    )?;
    Ok(Args {
        case,
        run_id: ordinal as u64 + 1,
        font: font
            .ok_or_else(|| error(ErrorKind::Arguments, "Missing --font"))?
            .into(),
        font_sha,
        font_bytes,
        output: output
            .ok_or_else(|| error(ErrorKind::Arguments, "Missing --output"))?
            .into(),
        deadline_ns,
        forbidden_ports: ports(forbidden.as_deref().unwrap_or(""))?,
    })
}
fn font_read_limit(font_bytes: usize, input_bytes: usize) -> Result<usize> {
    let remaining = MAX_INPUT
        .checked_sub(input_bytes)
        .ok_or_else(|| error(ErrorKind::Font, "Shared input admission already exceeded"))?;
    require(
        font_bytes != 0 && font_bytes <= remaining,
        ErrorKind::Font,
        "Admitted font does not fit the shared input remainder",
    )?;
    Ok(font_bytes.min(remaining))
}
fn prepare_output(path: &Path) -> Result<()> {
    let root = Path::new(ROOT).join("tmp");
    require(
        path.is_absolute()
            && path.starts_with(&root)
            && path != root
            && path
                .components()
                .all(|component| matches!(component, Component::RootDir | Component::Normal(_))),
        ErrorKind::Arguments,
        "Output must be normalized below repository tmp",
    )?;
    let parent = path
        .parent()
        .ok_or_else(|| error(ErrorKind::Arguments, "Output has no parent"))?;
    require(
        parent
            .canonicalize()
            .map_err(|e| error(ErrorKind::Arguments, e))?
            == parent,
        ErrorKind::Arguments,
        "Output parent must already exist without aliases",
    )?;
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    require(
        !name.is_empty()
            && name.len() <= 128
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        ErrorKind::Arguments,
        "Unsafe output basename",
    )?;
    fs::create_dir(path).map_err(|e| error(ErrorKind::Artifact, e))
}
fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>> {
    require(
        path.is_absolute()
            && path
                .canonicalize()
                .map_err(|e| error(ErrorKind::Input, e))?
                == path,
        ErrorKind::Input,
        "Input path aliases/symlinks are not admitted",
    )?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| error(ErrorKind::Input, e))?;
    let before = file.metadata().map_err(|e| error(ErrorKind::Input, e))?;
    require(
        before.is_file() && before.len() <= limit as u64,
        ErrorKind::Input,
        "Input is not bounded regular data",
    )?;
    let mut body = Vec::new();
    body.try_reserve_exact(before.len() as usize)
        .map_err(|e| error(ErrorKind::Input, e))?;
    Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|e| error(ErrorKind::Input, e))?;
    let after = file.metadata().map_err(|e| error(ErrorKind::Input, e))?;
    require(
        body.len() as u64 == before.len()
            && after.len() == before.len()
            && before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec(),
        ErrorKind::Input,
        "Input changed during bounded read",
    )?;
    Ok(body)
}
fn pinned(path: &Path, bytes: usize, digest: &str) -> Result<Vec<u8>> {
    let body = read_regular(path, bytes)?;
    require(
        body.len() == bytes && hash(&body) == digest,
        ErrorKind::Input,
        "Pinned input identity differs",
    )?;
    Ok(body)
}
#[derive(Deserialize)]
struct Manifest {
    schema_version: u8,
    input_file_count: usize,
    input_bytes: usize,
    files: Vec<InputSpec>,
    tests: Vec<DeclaredCase>,
}
#[derive(Deserialize)]
struct InputSpec {
    path: String,
    bytes: usize,
    sha256: String,
}
#[derive(Deserialize)]
struct DeclaredCase {
    path: String,
    source_declared_call_count: usize,
    source_expected_raw_test_names: Vec<String>,
}
fn manifest(selected: &str) -> Result<(DeclaredCase, usize)> {
    let root = Path::new(ROOT);
    let body = pinned(
        &root.join(source::MANIFEST_PATH),
        MANIFEST_BYTES,
        source::MANIFEST_SHA,
    )?;
    // A byte-pinned immutable object, not a generic untrusted manifest mode.
    let parsed: Manifest =
        serde_json::from_slice(&body).map_err(|e| error(ErrorKind::Manifest, e))?;
    require(
        parsed.schema_version == 1
            && parsed.input_file_count == 20
            && parsed.files.len() == 20
            && parsed.input_bytes == 222805
            && parsed.tests.len() == 16,
        ErrorKind::Manifest,
        "Fixed manifest roster differs",
    )?;
    let mut total = body.len();
    for input in &parsed.files {
        require(
            !input.path.is_empty()
                && !input.path.starts_with('/')
                && input
                    .path
                    .split('/')
                    .all(|p| !p.is_empty() && p != "." && p != "..")
                && input.path.is_ascii()
                && !input
                    .path
                    .bytes()
                    .any(|b| matches!(b, b'\\' | b'%' | b':' | b'?' | b'#' | 0))
                && lower_sha(&input.sha256),
            ErrorKind::Manifest,
            "Unsafe fixed input metadata",
        )?;
        total = total
            .checked_add(input.bytes)
            .filter(|total| *total <= MAX_INPUT)
            .ok_or_else(|| error(ErrorKind::Manifest, "Aggregate input admission exceeded"))?;
        pinned(
            &root.join("tools/wpt/upstream").join(&input.path),
            input.bytes,
            &input.sha256,
        )?;
    }
    require(
        parsed
            .tests
            .iter()
            .zip(source::CASES.iter())
            .all(|(a, b)| a.path == b.path()),
        ErrorKind::Manifest,
        "Manifest creation order differs",
    )?;
    let case = parsed
        .tests
        .into_iter()
        .find(|case| case.path == selected)
        .ok_or_else(|| error(ErrorKind::Manifest, "Missing declared case"))?;
    require(
        (1..=256).contains(&case.source_declared_call_count)
            && case.source_expected_raw_test_names.len() == case.source_declared_call_count
            && case
                .source_expected_raw_test_names
                .iter()
                .all(|name| !name.is_empty() && name.encode_utf16().take(4097).count() <= 4096),
        ErrorKind::Manifest,
        "Invalid fixed declared tests",
    )?;
    Ok((case, total))
}

#[derive(Debug, Copy, Clone, Serialize)]
struct FileIdentity {
    bytes: u64,
    #[serde(rename = "sha256", serialize_with = "serialize_digest")]
    digest: [u8; 32],
}
fn serialize_digest<S: serde::Serializer>(
    value: &[u8; 32],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.iter().map(|b| format!("{b:02x}")).collect::<String>())
}
fn binary_identity() -> Result<FileIdentity> {
    // Deliberately follow ONLY the kernel's current executable inode, as workers do.
    let mut file = File::open("/proc/self/exe").map_err(|e| error(ErrorKind::Input, e))?;
    let before = file.metadata().map_err(|e| error(ErrorKind::Input, e))?;
    require(
        before.is_file() && before.len() > 0 && before.len() <= MAX_BINARY,
        ErrorKind::Input,
        "Unbounded current executable",
    )?;
    let mut digest = Sha256::new();
    let mut seen = 0u64;
    let mut chunk = [0u8; 65536];
    loop {
        let size = file
            .read(&mut chunk)
            .map_err(|e| error(ErrorKind::Input, e))?;
        if size == 0 {
            break;
        }
        seen = seen
            .checked_add(size as u64)
            .filter(|seen| *seen <= MAX_BINARY)
            .ok_or_else(|| error(ErrorKind::Input, "Executable grew"))?;
        digest.update(&chunk[..size]);
    }
    let after = file.metadata().map_err(|e| error(ErrorKind::Input, e))?;
    require(
        seen == before.len()
            && after.len() == before.len()
            && after.ino() == before.ino()
            && after.dev() == before.dev()
            && after.mtime() == before.mtime()
            && after.mtime_nsec() == before.mtime_nsec(),
        ErrorKind::Input,
        "Executable changed during hash",
    )?;
    Ok(FileIdentity {
        bytes: seen,
        digest: digest.finalize().into(),
    })
}
struct LimitedFile {
    file: File,
    written: usize,
    limit: usize,
}
impl Write for LimitedFile {
    fn write(&mut self, body: &[u8]) -> std::io::Result<usize> {
        if body.len() > self.limit.saturating_sub(self.written) {
            return Err(std::io::Error::other("Artifact byte bound exceeded"));
        }
        let written = self.file.write(body)?;
        self.written += written;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
#[derive(Debug, Serialize)]
struct ArtifactIdentity {
    file: &'static str,
    bytes: usize,
    sha256: String,
}
fn artifact(
    output: &Path,
    name: &'static str,
    limit: usize,
    value: &impl Serialize,
) -> Result<ArtifactIdentity> {
    let path = output.join(name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| error(ErrorKind::Artifact, e))?;
    let mut writer = LimitedFile {
        file,
        written: 0,
        limit,
    };
    serde_json::to_writer_pretty(&mut writer, value).map_err(|e| error(ErrorKind::Artifact, e))?;
    writer.flush().map_err(|e| error(ErrorKind::Artifact, e))?;
    let body = read_regular(&path, limit)?;
    Ok(ArtifactIdentity {
        file: name,
        bytes: body.len(),
        sha256: hash(&body),
    })
}
fn early_failure(output: &Path, case: &str, run_id: u64, failure: &HostError) -> Result<()> {
    // No fixture/worker was admitted. This bounded ERROR envelope is retained,
    // but intentionally cannot pass the owner's healthy full-receipt schema.
    artifact(
        output,
        "receipt.json",
        MAX_ARTIFACT,
        &serde_json::json!({"schema_version":1,"scope":SCOPE,"case":case,
        "run_id":run_id,"owner_error":failure,"requires_outer_owner_acceptance":true,"internal_cleanup_required":false}),
    )?;
    Ok(())
}

fn monotonic_ns() -> Result<u64> {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    require(
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) } == 0
            && value.tv_sec >= 0
            && (0..1_000_000_000).contains(&value.tv_nsec),
        ErrorKind::Clock,
        "CLOCK_MONOTONIC unavailable",
    )?;
    (value.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(value.tv_nsec as u64))
        .ok_or_else(|| error(ErrorKind::Clock, "Monotonic clock overflow"))
}
struct Clock {
    started_ns: u64,
    cursor_ns: u64,
    used_active_ns: u64,
    excluded_idle_ns: u64,
    excluded_cleanup_ns: u64,
    admission: Instant,
    report: Instant,
    whole: Instant,
    whole_ns: u64,
    admission_ns: u64,
    report_ns: u64,
}
impl Clock {
    fn new(started: Instant, absolute: u64) -> Result<Self> {
        let before = Instant::now();
        let now = monotonic_ns()?;
        // Backdate conservatively by all elapsed setup, including this sample's
        // native bookkeeping. The outer owner counts any earlier process work.
        let started_ns = now
            .checked_sub(
                u64::try_from(started.elapsed().as_nanos())
                    .map_err(|_| error(ErrorKind::Clock, "Startup clock overflow"))?,
            )
            .ok_or_else(|| error(ErrorKind::Clock, "Startup clock underflow"))?;
        let whole_ns = absolute.min(
            started_ns
                .checked_add(25_000_000_000)
                .ok_or_else(|| error(ErrorKind::Clock, "Deadline overflow"))?,
        );
        let remaining = Duration::from_nanos(
            whole_ns
                .checked_sub(now)
                .ok_or_else(|| error(ErrorKind::Clock, "Owner deadline expired"))?,
        );
        require(
            remaining > Duration::from_secs(3),
            ErrorKind::Clock,
            "Insufficient owner time",
        )?;
        Ok(Self {
            started_ns,
            cursor_ns: started_ns,
            used_active_ns: 0,
            excluded_idle_ns: 0,
            excluded_cleanup_ns: 0,
            whole: before + remaining,
            admission: before + remaining - Duration::from_secs(3),
            report: before + remaining - Duration::from_millis(2100),
            whole_ns,
            admission_ns: whole_ns - 3_000_000_000,
            report_ns: whole_ns - 2_100_000_000,
        })
    }
    fn account_at(&mut self, now: u64) -> Result<()> {
        let elapsed = now
            .checked_sub(self.cursor_ns)
            .ok_or_else(|| error(ErrorKind::Clock, "Host clock moved backwards"))?;
        self.used_active_ns = self
            .used_active_ns
            .checked_add(elapsed)
            .ok_or_else(|| error(ErrorKind::Clock, "Active clock overflow"))?;
        self.cursor_ns = now;
        require(
            !CANCELED.load(Ordering::Acquire),
            ErrorKind::Interrupted,
            "Owned host interrupted",
        )?;
        require(
            now < self.report_ns,
            ErrorKind::Clock,
            "Host report deadline exceeded",
        )?;
        require(
            self.used_active_ns < 2_000_000_000,
            ErrorKind::Active,
            "Cumulative host active limit exceeded",
        )
    }
    fn account(&mut self) -> Result<()> {
        self.account_at(monotonic_ns()?)
    }
    fn exclude(&mut self, start: u64, end: u64, cleanup: bool) -> Result<()> {
        require(
            end >= start,
            ErrorKind::Clock,
            "Excluded interval moved backwards",
        )?;
        let previous = self.account_at(start);
        let excluded = if cleanup {
            &mut self.excluded_cleanup_ns
        } else {
            &mut self.excluded_idle_ns
        };
        *excluded = excluded
            .checked_add(end - start)
            .ok_or_else(|| error(ErrorKind::Clock, "Excluded clock overflow"))?;
        self.cursor_ns = end;
        previous?;
        require(
            end < self.report_ns && !CANCELED.load(Ordering::Acquire),
            ErrorKind::Clock,
            "Excluded interval exceeded report deadline",
        )
    }
    fn sleep(&mut self, idle: bool) -> Result<()> {
        self.account()?;
        let start = monotonic_ns()?;
        std::thread::sleep(Duration::from_millis(2));
        if idle {
            self.exclude(start, monotonic_ns()?, false)
        } else {
            self.account()
        }
    }
    fn cleanup(&mut self, start: u64, end: u64) -> Result<()> {
        require(
            end.checked_sub(start)
                .is_some_and(|elapsed| elapsed <= 2_000_000_000),
            ErrorKind::Clock,
            "Internal cleanup exceeded two seconds",
        )?;
        self.exclude(start, end, true)
    }
    fn valid_sums(&self) -> bool {
        self.used_active_ns
            .checked_add(self.excluded_idle_ns)
            .and_then(|sum| sum.checked_add(self.excluded_cleanup_ns))
            == self.cursor_ns.checked_sub(self.started_ns)
            && self.excluded_cleanup_ns <= 2_000_000_000
    }
    fn elapsed_ns(&self) -> u64 {
        self.cursor_ns.saturating_sub(self.started_ns)
    }
}
extern "C" fn cancel(_: libc::c_int) {
    CANCELED.store(true, Ordering::Release);
}
struct Signals([libc::sighandler_t; 2]);
impl Signals {
    fn install() -> Result<Self> {
        let int = unsafe { libc::signal(libc::SIGINT, cancel as libc::sighandler_t) };
        require(
            int != libc::SIG_ERR,
            ErrorKind::Runtime,
            "Cannot install INT handler",
        )?;
        let term = unsafe { libc::signal(libc::SIGTERM, cancel as libc::sighandler_t) };
        if term == libc::SIG_ERR {
            unsafe {
                libc::signal(libc::SIGINT, int);
            };
            return Err(error(ErrorKind::Runtime, "Cannot install TERM handler"));
        }
        Ok(Self([int, term]))
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.0[0]);
            libc::signal(libc::SIGTERM, self.0[1]);
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize)]
struct Published {
    raw_trace_index: usize,
    revision: u64,
    cursor: u32,
    progress: Option<ObserverProgress>,
    fault: Option<ObserverFault>,
    execution_failed: bool,
    lifecycle: Option<LifecycleProgress>,
    state: RealmState,
    pending_work: Option<u16>,
}
#[derive(Default)]
struct Acceptance {
    seen_raw: usize,
    last: Option<Published>,
    published: Vec<Published>,
    first_failure: Option<FailureCause>,
}
fn trace_matches(view: &ObservationView<'_>, trace: &RawTrace) -> bool {
    match trace.observer {
        ObserverTrace::WindowHarness {
            run_id,
            next_ordinal,
            progress,
            execution_failed,
            ..
        } => {
            view.expectation == (ObserverExpectation::WindowHarness { run_id })
                && view.cursor == next_ordinal
                && view.progress == Some(progress)
                && view.fault.is_none()
                && view.execution_failed == execution_failed
        }
        ObserverTrace::ObservationFailed {
            run_id,
            prefix_end,
            fault,
            execution_failed,
        } => {
            view.expectation == (ObserverExpectation::WindowHarness { run_id })
                && view.cursor == prefix_end
                && view.fault == Some(fault)
                && view.execution_failed == execution_failed
        }
        _ => false,
    }
}
fn raw_integrity(trace: &RawTrace) -> Option<Infrastructure> {
    // These are typed observation failures, not native diagnostics. Even a
    // simultaneous valid policy denial must not hide this integrity evidence.
    match trace.observer {
        ObserverTrace::ObservationFailed {
            fault: ObserverFault::InvalidConfiguration,
            ..
        } => Some(Infrastructure::Configuration),
        ObserverTrace::ObservationFailed {
            fault: ObserverFault::AuthorityUnavailable,
            ..
        } => Some(Infrastructure::Authority),
        ObserverTrace::ObservationFailed {
            fault:
                ObserverFault::MalformedCapture
                | ObserverFault::DuplicateResult
                | ObserverFault::DuplicateCompletion
                | ObserverFault::ResultAfterCompletion
                | ObserverFault::InvariantFailure,
            ..
        } => Some(Infrastructure::ObservationIntegrity),
        _ => None,
    }
}
impl Acceptance {
    fn observe_polled(
        &mut self,
        is_loading: bool,
        view: Option<ObservationView<'_>>,
        life: Option<LifecycleProgress>,
        raw: &HostSnapshot,
    ) -> Result<()> {
        // Init runs on the navigation producer. Its raw trace can appear after
        // poll returned but before Loaded reaches Browser's one-charge/commit
        // path. Do not consume or publish that still-unacknowledged Init.
        if is_loading
            && self.seen_raw == 0
            && raw.first_failure.is_none()
            && let [initial] = raw.raw_trace.as_slice()
            && initial.sequence.is_none()
            && initial.revision == 0
            && initial.manager_charge_ns.is_none()
            && !initial.manager_charge_succeeded
        {
            return Ok(());
        }
        self.observe(view, life, raw)
    }

    fn observe(
        &mut self,
        view: Option<ObservationView<'_>>,
        life: Option<LifecycleProgress>,
        raw: &HostSnapshot,
    ) -> Result<()> {
        if raw.raw_trace.len() == self.seen_raw {
            return Ok(());
        }
        require(
            raw.raw_trace.len() == self.seen_raw + 1 && raw.raw_trace.len() <= 64,
            ErrorKind::Correlation,
            "Unexpected raw transaction gap",
        )?;
        let index = self.seen_raw;
        self.seen_raw = raw.raw_trace.len();
        let view = view.ok_or_else(|| {
            error(
                ErrorKind::Correlation,
                "Reply without accepted Browser ledger",
            )
        })?;
        // A refusal is not publication. Preserve earlier accepted prefix; mapper
        // independently consumes parent failure plus the typed wrapper cause.
        if view.parent_failure.is_some() || raw.first_failure.is_some() {
            return Ok(());
        }
        let trace = &raw.raw_trace[index];
        require(
            trace.manager_charge_succeeded
                && trace.manager_charge_ns.is_some()
                && trace_matches(&view, trace),
            ErrorKind::Correlation,
            "Charge/run/cursor/progress/fault correlation failed",
        )?;
        if let ObserverTrace::WindowHarness {
            first_ordinal,
            result_callbacks,
            completion_callbacks,
            final_inventory_rows,
            ..
        } = trace.observer
        {
            let previous = self.last.map_or(0, |last| last.cursor);
            require(
                first_ordinal == previous && view.cursor >= previous,
                ErrorKind::Correlation,
                "Accepted ordinal prefix did not correlate",
            )?;
            let delta = view
                .records
                .get(previous as usize..)
                .ok_or_else(|| error(ErrorKind::Correlation, "Missing accepted record prefix"))?;
            let results = delta
                .iter()
                .filter(|row| matches!(row, ObserverRecord::Result { .. }))
                .count();
            let completions = delta
                .iter()
                .filter(|row| matches!(row, ObserverRecord::Completion { .. }))
                .count();
            let inventory = delta
                .iter()
                .map(|row| match row {
                    ObserverRecord::Completion { inventory, .. } => inventory.len(),
                    _ => 0,
                })
                .sum::<usize>();
            require(
                results == result_callbacks
                    && completions == completion_callbacks
                    && inventory == final_inventory_rows,
                ErrorKind::Correlation,
                "Accepted delta counts did not correlate",
            )?;
        }
        require(
            trace.state != RealmState::Ready || trace.lifecycle == life,
            ErrorKind::Correlation,
            "Accepted Ready lifecycle did not correlate",
        )?;
        let pending_work = match &trace.resources {
            None => None,
            Some(report) => Some(
                u16::try_from(report.pending_jobs)
                    .ok()
                    .filter(|pending| *pending <= 256)
                    .ok_or_else(|| {
                        error(ErrorKind::Correlation, "Unbounded accepted pending work")
                    })?,
            ),
        };
        require(
            trace.rejected_initial || trace.resources.is_some(),
            ErrorKind::Correlation,
            "Accepted reply missing modern report",
        )?;
        let published = Published {
            raw_trace_index: index,
            revision: trace.revision,
            cursor: view.cursor,
            progress: view.progress,
            fault: view.fault,
            execution_failed: view.execution_failed,
            lifecycle: life,
            state: trace.state,
            pending_work,
        };
        if self.first_failure.is_none() {
            self.first_failure = match view.fault {
                Some(ObserverFault::CaptureOverflow) => Some(FailureCause::ObservationLimit),
                Some(ObserverFault::PrivacyLost) => Some(FailureCause::ObservationLost),
                _ if view.execution_failed => Some(FailureCause::NativeExecution),
                _ => None,
            };
        }
        self.published
            .try_reserve_exact(1)
            .map_err(|e| error(ErrorKind::Correlation, e))?;
        require(
            self.published.capacity() <= 64,
            ErrorKind::Correlation,
            "Published trace capacity exceeded",
        )?;
        self.published.push(published);
        self.last = Some(published);
        Ok(())
    }
}

#[derive(Serialize)]
struct RawInitial<'a> {
    status: &'static str,
    failure: Option<&'static str>,
    scripts_executed: usize,
    error_count: usize,
    broker_rejected_sources: usize,
    diagnostics: &'a [String],
}
fn raw_initial(value: &InitialScriptOutcome) -> RawInitial<'_> {
    RawInitial {
        status: match value.status {
            InitialScriptStatus::NotRequested => "NotRequested",
            InitialScriptStatus::Pending => "Pending",
            InitialScriptStatus::Complete => "Complete",
            InitialScriptStatus::Partial => "Partial",
            InitialScriptStatus::Failed => "Failed",
        },
        failure: value.failure.map(|failure| match failure {
            InitialScriptFailure::Script => "Script",
            InitialScriptFailure::Infrastructure => "Infrastructure",
        }),
        scripts_executed: value.scripts_executed,
        error_count: value.error_count,
        broker_rejected_sources: value.broker_rejected_sources,
        diagnostics: &value.diagnostics,
    }
}
fn initial_evidence(value: Option<&InitialScriptOutcome>) -> InitialEvidence {
    match value {
        None => InitialEvidence::Unknown,
        Some(value) if value.failure == Some(InitialScriptFailure::Infrastructure) => {
            InitialEvidence::InfrastructureFailed
        }
        Some(value) if value.failure == Some(InitialScriptFailure::Script) => {
            InitialEvidence::ScriptFailed
        }
        Some(value)
            if value.status == InitialScriptStatus::Complete
                && value.error_count == 0
                && value.broker_rejected_sources == 0 =>
        {
            InitialEvidence::Clean
        }
        Some(value) if value.status == InitialScriptStatus::Pending => InitialEvidence::Pending,
        _ => InitialEvidence::Unknown,
    }
}
fn accepted_native_history(execution_failed: Option<bool>) -> NativeHistory {
    match execution_failed {
        None => NativeHistory::Unknown,
        Some(true) => NativeHistory::Failed,
        Some(false) => NativeHistory::Clean,
    }
}
fn infrastructure(failure: HostFailure) -> Option<Infrastructure> {
    use super::guard::{TraceFailure, TransportPhase};
    match failure {
        HostFailure::Source { .. } => Some(Infrastructure::Inventory),
        HostFailure::Configuration { .. } => Some(Infrastructure::Configuration),
        HostFailure::Generation { .. }
        | HostFailure::RepeatedStart
        | HostFailure::ActivationRefused
        | HostFailure::ReplyOverlap => Some(Infrastructure::Authority),
        HostFailure::Projection => Some(Infrastructure::Projection),
        HostFailure::Trace {
            reason:
                TraceFailure::ReplyIdentity
                | TraceFailure::DiagnosticShape
                | TraceFailure::ResourceShape,
        } => Some(Infrastructure::ObservationIntegrity),
        HostFailure::Trace { .. } => Some(Infrastructure::Artifact),
        HostFailure::Acknowledgement => Some(Infrastructure::ActiveCharge),
        HostFailure::OpaqueTransport {
            phase: TransportPhase::Start | TransportPhase::TaskAdmission | TransportPhase::Reply,
        } => Some(Infrastructure::Transport),
        HostFailure::Closed => Some(Infrastructure::Authority),
        HostFailure::Cleanup | HostFailure::Policy { .. } | HostFailure::Cutoff => None,
    }
}

#[derive(Serialize)]
struct Accepted<'a> {
    scope: &'static str,
    run_id: u64,
    missing: bool,
    cursor: Option<u32>,
    progress: Option<ObserverProgress>,
    execution_failed: Option<bool>,
    fault: Option<ObserverFault>,
    parent_failure: Option<&'static str>,
    copied_units: Option<usize>,
    retained_capacity_bytes: Option<usize>,
    records: Option<&'a [ObserverRecord]>,
    reconciliations: Option<Reconciliations<'a>>,
}
struct Reconciliations<'a>(&'a [ReconciledName]);
impl Serialize for Reconciliations<'_> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        #[derive(Serialize)]
        struct Link {
            id: u16,
            index: u16,
            kind: &'static str,
        }
        let mut rows = serializer.serialize_seq(Some(self.0.len()))?;
        for link in self.0 {
            rows.serialize_element(&Link {
                id: link.id,
                index: link.index,
                kind: match link.kind {
                    mg_sparkle::page_session::observer::NameReconciliation::Unchanged => {
                        "Unchanged"
                    }
                    mg_sparkle::page_session::observer::NameReconciliation::PinnedSanitized => {
                        "PinnedSanitized"
                    }
                    mg_sparkle::page_session::observer::NameReconciliation::FinalOnly => {
                        "FinalOnly"
                    }
                },
            })?;
        }
        rows.end()
    }
}
fn parent_label(failure: ObserverParentFailure) -> &'static str {
    match failure {
        ObserverParentFailure::Validation => "Validation",
        ObserverParentFailure::Projection => "Projection",
        ObserverParentFailure::ActiveCharge => "ActiveCharge",
        ObserverParentFailure::Transport => "Transport",
        ObserverParentFailure::BrokerPolicy => "BrokerPolicy",
        ObserverParentFailure::Deadline => "Deadline",
        ObserverParentFailure::Cleanup => "Cleanup",
    }
}
fn accepted<'a>(browser: Option<&'a Browser>, run_id: u64) -> Accepted<'a> {
    let view = browser
        .and_then(Browser::script_observation)
        .map(|ledger| ledger.observation());
    accepted_view(view, run_id)
}
fn accepted_view(view: Option<ObservationView<'_>>, run_id: u64) -> Accepted<'_> {
    Accepted {
        scope: "Only Browser parent-validated, once-charged accepted ledger; not raw wrapper replies",
        run_id,
        missing: view.is_none(),
        cursor: view.as_ref().map(|v| v.cursor),
        progress: view.as_ref().and_then(|v| v.progress),
        execution_failed: view.as_ref().map(|v| v.execution_failed),
        fault: view.as_ref().and_then(|v| v.fault),
        parent_failure: view
            .as_ref()
            .and_then(|v| v.parent_failure)
            .map(parent_label),
        copied_units: view.as_ref().map(|v| v.copied_units),
        retained_capacity_bytes: view.as_ref().map(|v| v.retained_capacity_bytes),
        records: view.as_ref().map(|v| v.records),
        reconciliations: view.as_ref().map(|v| Reconciliations(v.reconciliations)),
    }
}
#[derive(Serialize)]
struct CleanupEvidence {
    runtime_closed: bool,
    pool_wait_idle: bool,
    fixture_sealed: bool,
    fixture_drained: bool,
    fixture_joined: bool,
    initial_producer_quiescent: bool,
    internal_confirmed: bool,
    outer_owner_required: bool,
}
#[derive(Serialize)]
struct Clocks {
    whole_deadline_monotonic_ns: u64,
    admission_deadline_monotonic_ns: u64,
    report_deadline_monotonic_ns: u64,
    started_monotonic_ns: u64,
    cursor_monotonic_ns: u64,
    elapsed_ns: u64,
    used_active_ns: u64,
    excluded_idle_ns: u64,
    excluded_cleanup_ns: u64,
    sums_valid: bool,
    admission_stopped: bool,
    report_boundary_checked: bool,
}
#[derive(Serialize)]
struct SourceIdentity {
    upstream: &'static str,
    html_sha256: &'static str,
    entry_sha256: String,
    host_sha256: String,
    source_module_sha256: String,
    guard_sha256: String,
    verdict_sha256: String,
    fixture_sha256: String,
    build_revision: &'static str,
    compiled: &'static str,
    package_version: &'static str,
}
#[derive(Serialize)]
struct Artifacts {
    accepted: Option<ArtifactIdentity>,
    http: Option<ArtifactIdentity>,
}
#[derive(Serialize)]
struct Receipt<'a> {
    schema_version: u8,
    scope: &'static str,
    case: &'a str,
    run_id: u64,
    manifest_sha256: &'static str,
    contract: ObserverContract,
    binary: FileIdentity,
    font: FileIdentity,
    source: SourceIdentity,
    origin: &'a str,
    url: &'a str,
    port: u16,
    deadline_monotonic_ns: u64,
    clocks: Clocks,
    semantic_decision: verdict::Decision,
    requires_outer_owner_acceptance: bool,
    artifacts: Artifacts,
    cleanup: CleanupEvidence,
    host_facts: &'a HostFacts,
    raw_initial: Option<RawInitial<'a>>,
    host_evidence: Option<&'a HostSnapshot>,
    published_trace: &'a [Published],
    source_plan: Option<SourcePlan>,
    error: Option<&'a HostError>,
    source_declared_call_count: usize,
    source_expected_raw_test_names: &'a [String],
}

pub fn run() -> Result<bool> {
    let started = Instant::now();
    let _signals = Signals::install()?;
    let args = parse_args(std::env::args().skip(1))?;
    let mut clock = Clock::new(started, args.deadline_ns)?;
    prepare_output(&args.output)?;
    let prepared = (|| {
        let (declared, input_bytes) = manifest(args.case.path())?;
        let font = read_regular(&args.font, font_read_limit(args.font_bytes, input_bytes)?)?;
        require(
            font.len() == args.font_bytes && hash(&font) == args.font_sha,
            ErrorKind::Font,
            "Font length or SHA pin differs",
        )?;
        let font_identity = FileIdentity {
            bytes: font.len() as u64,
            digest: Sha256::digest(&font).into(),
        };
        let fonts = Fonts::from_bytes(font).map_err(|e| error(ErrorKind::Font, e))?;
        let binary = binary_identity()?;
        clock.account()?;
        Ok((declared, fonts, font_identity, binary))
    })();
    let (declared, fonts, font_identity, binary) = match prepared {
        Ok(v) => v,
        Err(failure) => {
            early_failure(&args.output, args.case.path(), args.run_id, &failure)?;
            return Ok(false);
        }
    };
    let mut fixture = match Fixture::start(
        args.case,
        &args.forbidden_ports,
        clock.admission,
        clock.whole,
    ) {
        Ok(fixture) => fixture,
        Err(e) => {
            early_failure(
                &args.output,
                args.case.path(),
                args.run_id,
                &error(ErrorKind::Fixture, e),
            )?;
            return Ok(false);
        }
    };
    let origin = fixture.origin().to_owned();
    let url = fixture.document_url().to_owned();
    let port = fixture.port();
    let evidence = Arc::new(HostState::new());
    let mut runtime: Option<Arc<InventoryScripts>> = None;
    let mut browser: Option<Browser> = None;
    let mut source_plan = None;
    let mut acceptance = Acceptance::default();
    let mut stop = Stop::Terminal;
    let outcome: Result<()> = (|| {
        let inventory =
            SourceInventory::new(args.case, &url).map_err(|e| error(ErrorKind::Input, e))?;
        let plan = inventory.plan();
        source_plan = Some(plan);
        let bootstrap_node = u32::try_from(plan.harness_node)
            .map_err(|_| error(ErrorKind::Input, "Bootstrap ID overflow"))?;
        let init = mg_sparkle::page_session::observer::ObserverInit::WindowHarness {
            contract: ObserverContract::PinnedWindowHarnessOutputDisabledV1,
            run_id: args.run_id,
            bootstrap_node,
        };
        let scripts = Arc::new(
            InventoryScripts::new(
                Arc::new(LinuxScripts::with_observer(init)),
                inventory,
                init,
                evidence.clone(),
                clock.admission,
            )
            .map_err(|e| error(ErrorKind::Runtime, e))?,
        );
        runtime = Some(scripts.clone());
        let mut page = Browser::new(fonts, scripts);
        page.set_chrome(false);
        page.set_scripts_enabled(true);
        page.resize(800, 600);
        page.navigate(url.clone(), None, false);
        browser = Some(page);
        let page = browser.as_mut().unwrap();
        loop {
            clock.account()?;
            if Instant::now() >= clock.admission {
                stop = Stop::AdmissionDeadline;
                break;
            }
            page.poll();
            let summary = evidence
                .summary()
                .map_err(|e| error(ErrorKind::Runtime, e))?;
            if summary.replies != acceptance.seen_raw {
                let snapshot = evidence
                    .snapshot()
                    .map_err(|e| error(ErrorKind::Runtime, e))?;
                acceptance.observe_polled(
                    page.is_loading(),
                    page.script_observation().map(|l| l.observation()),
                    page.document_lifecycle(),
                    &snapshot,
                )?;
            }
            let observed = page.script_observation().map(|l| l.observation());
            if summary.first_failure.is_some()
                || observed
                    .as_ref()
                    .is_some_and(|v| v.fault.is_some() || v.parent_failure.is_some())
            {
                stop = if summary.first_failure == Some(HostFailure::Cutoff) {
                    Stop::AdmissionDeadline
                } else {
                    Stop::Terminal
                };
                break;
            }
            if observed
                .as_ref()
                .is_some_and(|v| v.progress == Some(ObserverProgress::Finished))
            {
                stop = Stop::AcceptedCutoff;
                break;
            }
            if page.initial_script_outcome().status == InitialScriptStatus::Failed {
                stop = Stop::Terminal;
                break;
            }
            let idle = !page.is_loading()
                && matches!(
                    page.script_work_status(),
                    ScriptWorkStatus::Idle | ScriptWorkStatus::Scheduled(_)
                );
            clock.sleep(idle)?;
        }
        clock.account()
    })();
    // Always stop command admission, retain Browser/ledger, and finish internal
    // cleanup BEFORE audit/classification. Never a free arbitrary task drain.
    let cleanup_started = monotonic_ns();
    let runtime_closed = runtime.as_ref().is_some();
    let pool_wait_idle = if let Some(runtime) = &runtime {
        runtime.close();
        runtime.wait_idle().is_ok()
    } else {
        true
    };
    let cleanup_clock = if runtime_closed {
        cleanup_started.and_then(|start| clock.cleanup(start, monotonic_ns()?))
    } else {
        clock.account()
    };
    // Fixture stop/join/audit is charged active, not silently included in the
    // excluded pool-cleanup interval (which would waive native bookkeeping).
    let fixture_result = fixture.seal_and_join(clock.report);
    let fixture_ok = fixture_result
        .as_ref()
        .is_ok_and(|audit| audit.cleanup_confirmed());
    // A pending navigation producer may still call the wrapper after pool close.
    // Do not clone its detailed evidence or adopt a cutoff while it can mutate.
    // No additional poll/drain is allowed here; outer owner cleanup remains.
    let initial_producer_quiescent = browser.as_ref().is_none_or(|page| !page.is_loading());
    let internal_cleanup =
        pool_wait_idle && fixture_ok && cleanup_clock.is_ok() && initial_producer_quiescent;
    let audit = fixture_result.as_ref().ok();
    let mut failure = outcome.err().or_else(|| cleanup_clock.err());
    if fixture_result.is_err() && failure.is_none() {
        failure = Some(error(ErrorKind::Fixture, "Fixture cleanup/audit failed"));
    }
    if !pool_wait_idle && failure.is_none() {
        failure = Some(error(
            ErrorKind::Runtime,
            "Explicit runtime wait_idle failed",
        ));
    }
    if !initial_producer_quiescent && failure.is_none() {
        failure = Some(error(
            ErrorKind::Runtime,
            "Initial navigation producer did not quiesce",
        ));
    }
    let snapshot = if internal_cleanup {
        match evidence.snapshot() {
            Ok(v) => Some(v),
            Err(e) => {
                failure.get_or_insert_with(|| error(ErrorKind::Runtime, e));
                None
            }
        }
    } else {
        None
    };
    let mut facts = HostFacts {
        expected: ObserverExpectation::WindowHarness {
            run_id: args.run_id,
        },
        premises_verified: true,
        infrastructure: failure.as_ref().map(|f| match f.kind {
            ErrorKind::Active => Infrastructure::ActiveCharge,
            ErrorKind::Correlation => Infrastructure::ObservationIntegrity,
            ErrorKind::Input | ErrorKind::Manifest | ErrorKind::Font => Infrastructure::Inventory,
            ErrorKind::Artifact => Infrastructure::Artifact,
            _ => Infrastructure::IncompleteEvidence,
        }),
        cleanup: if internal_cleanup {
            Cleanup::Confirmed
        } else {
            Cleanup::Unknown
        },
        stop,
        delivery: Delivery::Unverified,
        lifecycle: browser.as_ref().and_then(Browser::document_lifecycle),
        realm: acceptance.last.map(|last| last.state),
        initial: initial_evidence(browser.as_ref().map(Browser::initial_script_outcome)),
        native_history: accepted_native_history(acceptance.last.map(|last| last.execution_failed)),
        pending_work: acceptance.last.and_then(|last| last.pending_work),
        first_established_failure: acceptance.first_failure,
        transport_consequence: TransportConsequence::None,
        initial_transport_consequence: TransportConsequence::None,
        worker_signal: None,
        unsupported_before_js: None,
        rejected_init_validated: false,
    };
    if CANCELED.load(Ordering::Acquire) {
        facts.stop = Stop::Interrupted;
    }
    if let Some(snapshot) = &snapshot {
        for fact in snapshot
            .first_failure
            .iter()
            .chain(snapshot.secondary_failures.iter())
        {
            if let Some(layer) = infrastructure(fact.failure) {
                facts.infrastructure.get_or_insert(layer);
            }
        }
        for trace in &snapshot.raw_trace {
            if let Some(layer) = raw_integrity(trace) {
                facts.infrastructure.get_or_insert(layer);
            }
        }
        if let Some(HostFailure::Policy { .. }) = snapshot.first_failure.map(|f| f.failure) {
            facts
                .first_established_failure
                .get_or_insert(FailureCause::PolicyDenied);
            facts.transport_consequence = TransportConsequence::ValidatedPolicyDenied;
            if acceptance.last.is_none()
                && snapshot.starts == 1
                && snapshot.raw_trace.len() == 1
                && snapshot.raw_trace[0].manager_charge_succeeded
                && snapshot.raw_trace[0].manager_charge_ns.is_some()
            {
                facts.initial_transport_consequence = TransportConsequence::ValidatedPolicyDenied;
            }
        }
        // Charge trace is not accepted. Only a correlated accepted RejectedInit
        // enables the narrow reportless exception, never arbitrary config Err.
        facts.rejected_init_validated = acceptance
            .last
            .is_some_and(|last| last.fault == Some(ObserverFault::RejectedInit));
        if facts.rejected_init_validated {
            facts.stop = Stop::BeforeJavaScript;
        }
    }
    if let Some(audit) = audit {
        if audit.port() != port || audit.origin() != origin || audit.document_url() != url {
            facts.infrastructure = Some(Infrastructure::Authority);
        }
        facts.delivery = if audit.complete_four_gets() {
            Delivery::CompleteFourGets
        } else if audit.clean_attributed_prefix() && facts.first_established_failure.is_some() {
            Delivery::AuditedPrefix
        } else {
            Delivery::Unverified
        };
    }
    let http = match audit {
        Some(audit) => artifact(&args.output, "http-audit.json", MAX_HTTP_ARTIFACT, audit),
        None => artifact(
            &args.output,
            "http-audit.json",
            MAX_HTTP_ARTIFACT,
            &serde_json::json!({"cleanup_confirmed":false,"audit":null}),
        ),
    };
    let http = match http {
        Ok(v) => Some(v),
        Err(e) => {
            facts.infrastructure = Some(Infrastructure::Artifact);
            failure.get_or_insert(e);
            None
        }
    };
    let accepted_artifact = if internal_cleanup {
        artifact(
            &args.output,
            "accepted.json",
            MAX_ARTIFACT,
            &accepted(browser.as_ref(), args.run_id),
        )
    } else {
        artifact(
            &args.output,
            "accepted.json",
            MAX_ARTIFACT,
            &accepted(None, args.run_id),
        )
    };
    let accepted_artifact = match accepted_artifact {
        Ok(v) => Some(v),
        Err(e) => {
            facts.infrastructure = Some(Infrastructure::Artifact);
            failure.get_or_insert(e);
            None
        }
    };
    if let Err(e) = clock.account() {
        facts.infrastructure = Some(Infrastructure::IncompleteEvidence);
        failure.get_or_insert(e);
    }
    if !clock.valid_sums() {
        facts.infrastructure = Some(Infrastructure::ContradictoryFacts);
        failure.get_or_insert_with(|| error(ErrorKind::Clock, "Clock sum invariant failed"));
    }
    let decision = verdict::classify(
        if internal_cleanup {
            browser
                .as_ref()
                .and_then(Browser::script_observation)
                .map(|l| l.observation())
        } else {
            None
        },
        DeclaredTests {
            count: declared.source_declared_call_count,
            names: &declared.source_expected_raw_test_names,
        },
        &facts,
    );
    let measured = !matches!(decision.verdict, Verdict::Error | Verdict::Crash)
        && internal_cleanup
        && failure.is_none();
    let receipt = Receipt {
        schema_version: 1,
        scope: SCOPE,
        case: args.case.path(),
        run_id: args.run_id,
        manifest_sha256: source::MANIFEST_SHA,
        contract: ObserverContract::PinnedWindowHarnessOutputDisabledV1,
        binary,
        font: font_identity,
        source: SourceIdentity {
            upstream: source::REVISION,
            html_sha256: args.case.sha256(),
            entry_sha256: hash(include_bytes!("../wpt_window.rs")),
            host_sha256: hash(include_bytes!("host.rs")),
            source_module_sha256: hash(include_bytes!("source.rs")),
            guard_sha256: hash(include_bytes!("guard.rs")),
            verdict_sha256: hash(include_bytes!("verdict.rs")),
            fixture_sha256: hash(include_bytes!("fixture.rs")),
            build_revision: env!("MGBROWSER_REVISION"),
            compiled: env!("MGBROWSER_COMPILED"),
            package_version: env!("CARGO_PKG_VERSION"),
        },
        origin: &origin,
        url: &url,
        port,
        deadline_monotonic_ns: args.deadline_ns,
        clocks: Clocks {
            whole_deadline_monotonic_ns: clock.whole_ns,
            admission_deadline_monotonic_ns: clock.admission_ns,
            report_deadline_monotonic_ns: clock.report_ns,
            started_monotonic_ns: clock.started_ns,
            cursor_monotonic_ns: clock.cursor_ns,
            elapsed_ns: clock.elapsed_ns(),
            used_active_ns: clock.used_active_ns,
            excluded_idle_ns: clock.excluded_idle_ns,
            excluded_cleanup_ns: clock.excluded_cleanup_ns,
            sums_valid: clock.valid_sums(),
            admission_stopped: true,
            report_boundary_checked: Instant::now() < clock.report,
        },
        semantic_decision: decision,
        requires_outer_owner_acceptance: true,
        artifacts: Artifacts {
            accepted: accepted_artifact,
            http,
        },
        cleanup: CleanupEvidence {
            runtime_closed,
            pool_wait_idle,
            fixture_sealed: fixture_ok,
            fixture_drained: fixture_ok,
            fixture_joined: fixture_ok,
            initial_producer_quiescent,
            internal_confirmed: internal_cleanup,
            outer_owner_required: true,
        },
        host_facts: &facts,
        raw_initial: browser
            .as_ref()
            .map(|b| raw_initial(b.initial_script_outcome())),
        host_evidence: snapshot.as_ref(),
        published_trace: &acceptance.published,
        source_plan,
        error: failure.as_ref(),
        source_declared_call_count: declared.source_declared_call_count,
        source_expected_raw_test_names: &declared.source_expected_raw_test_names,
    };
    artifact(&args.output, "receipt.json", MAX_ARTIFACT, &receipt)?;
    clock.account()?;
    Ok(measured)
}

#[cfg(test)]
mod tests {
    // Pure host composition tests: no Browser construction, parser, worker,
    // socket, process launch, JavaScript or font loading. The real shared ledger
    // supplies every accepted view; no malformed raw view is declared accepted.
    use super::*;
    use mg_sparkle::page_session::{
        LifecyclePhase,
        observer::{
            HarnessRecord, HarnessStatus, ObserverLedger, ObserverReply, OptionalUtf16,
            SubtestStatus, TestRecord, Utf16,
        },
    };

    const RUN: u64 = 1;
    fn life() -> LifecycleProgress {
        LifecycleProgress {
            phase: LifecyclePhase::LoadComplete,
            initial_failure: false,
        }
    }
    fn arguments() -> Vec<String> {
        vec![
            "--case",
            source::CASES[0].path(),
            "--font",
            "/tmp/font.ttf",
            "--font-sha",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--output",
            "/tmp/not-created",
            "--deadline-monotonic-ns",
            "100000000000",
            "--font-bytes",
            "759720",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }
    fn ledger() -> ObserverLedger {
        ObserverLedger::new(ObserverExpectation::WindowHarness { run_id: RUN }).unwrap()
    }
    fn row() -> TestRecord {
        TestRecord {
            id: 1,
            index: 0,
            name: Utf16::from_units(vec![0xd800]).unwrap(),
            status: SubtestStatus::Pass,
            message: OptionalUtf16::Null,
            stack: OptionalUtf16::Undefined,
        }
    }
    fn finish(ledger: &mut ObserverLedger, failed: bool) {
        ledger
            .stage(ObserverReply::WindowHarness {
                run_id: RUN,
                first_ordinal: 0,
                next_ordinal: 2,
                records: vec![
                    ObserverRecord::Result { test: row() },
                    ObserverRecord::Completion {
                        harness: HarnessRecord {
                            status: HarnessStatus::Ok,
                            message: OptionalUtf16::Null,
                            stack: OptionalUtf16::Undefined,
                        },
                        inventory: vec![row()],
                    },
                ],
                progress: ObserverProgress::Finished,
                execution_failed: failed,
            })
            .unwrap()
            .commit();
    }
    fn report() -> mg_butane::modern::Report {
        mg_butane::modern::Report {
            profile: mg_butane::modern::PROFILE.into(),
            opcodes_remaining: 1,
            source_bytes: 1,
            source_admissions: 1,
            jobs_admitted: 3,
            jobs_executed: 2,
            pending_jobs: 1,
            fatal_reason: None,
            worker_memory: None,
        }
    }
    fn trace() -> RawTrace {
        RawTrace {
            generation: 1,
            session_id: 1,
            sequence: None,
            revision: 0,
            state: RealmState::Ready,
            lifecycle: Some(life()),
            observer: ObserverTrace::WindowHarness {
                run_id: RUN,
                first_ordinal: 0,
                next_ordinal: 2,
                progress: ObserverProgress::Finished,
                execution_failed: false,
                result_callbacks: 1,
                completion_callbacks: 1,
                final_inventory_rows: 1,
            },
            scripts_executed: 4,
            diagnostics: 0,
            rejected_initial: false,
            resources: Some(report()),
            wrapper_validation_ns: 1,
            manager_charge_ns: Some(2),
            manager_charge_succeeded: true,
        }
    }
    fn snapshot(trace: RawTrace) -> HostSnapshot {
        HostSnapshot {
            raw_trace: vec![trace],
            ..HostSnapshot::default()
        }
    }
    fn clock() -> Clock {
        let now = Instant::now();
        Clock {
            started_ns: 100,
            cursor_ns: 100,
            used_active_ns: 0,
            excluded_idle_ns: 0,
            excluded_cleanup_ns: 0,
            admission: now + Duration::from_secs(22),
            report: now + Duration::from_secs(23),
            whole: now + Duration::from_secs(25),
            whole_ns: 25_000_000_100,
            admission_ns: 22_000_000_100,
            report_ns: 22_900_000_100,
        }
    }

    #[test]
    fn fixed_cli_derives_run_and_rejects_extra_authority() {
        let parsed = parse_args(arguments()).unwrap();
        assert_eq!(parsed.run_id, 1);
        assert_eq!(parsed.font_bytes, 759720);
        assert!(parsed.forbidden_ports.is_empty());
        for flag in ["--run-id", "--origin", "--source", "--suite"] {
            let mut input = arguments();
            input.extend([flag.into(), "1".into()]);
            assert!(parse_args(input).is_err());
        }
        let mut duplicate = arguments();
        duplicate.extend(["--case".into(), source::CASES[0].path().into()]);
        assert!(parse_args(duplicate).is_err());
        let mut absent = arguments();
        absent[1] = "css/css-flexbox/parsing/not-frozen-valid.html".into();
        assert!(parse_args(absent).is_err());
        let mut alias = arguments();
        alias[1] = format!("./{}", source::CASES[0].path());
        assert!(parse_args(alias).is_err());
        let mut noncanonical = arguments();
        noncanonical[9] = "010".into();
        assert!(parse_args(noncanonical).is_err());
    }
    #[test]
    fn exact_font_length_is_required_before_shared_input_body_admission() {
        let mut absent = arguments();
        absent.truncate(10);
        assert!(parse_args(absent).is_err());
        for invalid in ["0", "01", "+1", "10.0", "10485761"] {
            let mut input = arguments();
            input[11] = invalid.into();
            assert!(parse_args(input).is_err(), "{invalid}");
        }
        assert_eq!(font_read_limit(1, MAX_INPUT - 1).unwrap(), 1);
        assert_eq!(font_read_limit(759720, 222805).unwrap(), 759720);
        assert!(font_read_limit(2, MAX_INPUT - 1).is_err());
        assert!(font_read_limit(1, MAX_INPUT).is_err());
        assert!(font_read_limit(1, MAX_INPUT + 1).is_err());
        assert!(font_read_limit(0, 0).is_err());
    }
    #[test]
    fn port_reservation_is_canonical_unique_and_bounded() {
        assert_eq!(ports("1,1024,65535").unwrap(), vec![1, 1024, 65535]);
        for bad in ["0", "01", "2,1", "1,1", "1,", "65536", " 1"] {
            assert!(ports(bad).is_err(), "{bad}");
        }
        let sixteen = (1..=16)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(ports(&sixteen).unwrap().len(), 16);
        assert!(ports(&(sixteen + ",17")).is_err());
    }
    #[test]
    fn clock_exactly_separates_actual_idle_cleanup_and_active() {
        let mut c = clock();
        c.account_at(200).unwrap();
        c.exclude(250, 1_000, false).unwrap();
        c.cleanup(1_100, 1_500).unwrap();
        c.account_at(1_600).unwrap();
        assert_eq!(c.used_active_ns, 350);
        assert_eq!(c.excluded_idle_ns, 750);
        assert_eq!(c.excluded_cleanup_ns, 400);
        assert_eq!(c.elapsed_ns(), 1_500);
        assert!(c.valid_sums());
        c.excluded_idle_ns += 1;
        assert!(!c.valid_sums());
    }
    #[test]
    fn clock_never_renews_deadlines_or_two_second_active_allowance() {
        let mut c = clock();
        let report = c.report_ns;
        assert!(c.account_at(2_000_000_100).is_err());
        assert_eq!(c.report_ns, report);
        let mut c = clock();
        assert!(c.cleanup(100, 2_000_000_101).is_err());
        assert_eq!(c.cursor_ns, 100);
        let mut c = clock();
        assert!(c.exclude(200, 100, false).is_err());
        let mut c = clock();
        assert!(c.account_at(c.report_ns).is_err());
    }
    #[test]
    fn accepted_finished_correlates_without_requiring_pending_zero() {
        let mut ledger = ledger();
        finish(&mut ledger, false);
        let mut accepted = Acceptance::default();
        accepted
            .observe(Some(ledger.observation()), Some(life()), &snapshot(trace()))
            .unwrap();
        let published = accepted.last.unwrap();
        assert_eq!(published.pending_work, Some(1));
        assert_eq!(published.cursor, 2);
        assert_eq!(accepted.published.len(), 1);
        assert_eq!(accepted.first_failure, None);
    }
    #[test]
    fn pending_initial_raw_trace_waits_for_browser_publication_exactly_once() {
        let mut ledger = ledger();
        let interactive = LifecycleProgress {
            phase: LifecyclePhase::Interactive,
            initial_failure: false,
        };
        let mut initial = trace();
        initial.lifecycle = Some(interactive);
        initial.observer = ObserverTrace::WindowHarness {
            run_id: RUN,
            first_ordinal: 0,
            next_ordinal: 1,
            progress: ObserverProgress::Waiting,
            execution_failed: false,
            result_callbacks: 1,
            completion_callbacks: 0,
            final_inventory_rows: 0,
        };
        initial.manager_charge_ns = None;
        initial.manager_charge_succeeded = false;
        let mut raw = snapshot(initial);
        let mut accepted = Acceptance::default();
        for _ in 0..2 {
            accepted
                .observe_polled(true, Some(ledger.observation()), None, &raw)
                .unwrap();
            assert_eq!(accepted.seen_raw, 0);
            assert!(accepted.last.is_none());
            assert!(accepted.published.is_empty());
            assert_eq!(accepted.first_failure, None);
            assert_eq!(ledger.observation().cursor, 0);
        }
        // The normal Browser poll performs the one charge and commits the real
        // shared ledger. The earlier raw trace did not publish either fact.
        ledger
            .stage(ObserverReply::WindowHarness {
                run_id: RUN,
                first_ordinal: 0,
                next_ordinal: 1,
                records: vec![ObserverRecord::Result { test: row() }],
                progress: ObserverProgress::Waiting,
                execution_failed: false,
            })
            .unwrap()
            .commit();
        raw.raw_trace[0].manager_charge_ns = Some(2);
        raw.raw_trace[0].manager_charge_succeeded = true;
        for _ in 0..2 {
            accepted
                .observe_polled(true, Some(ledger.observation()), Some(interactive), &raw)
                .unwrap();
        }
        assert_eq!(accepted.seen_raw, 1);
        assert_eq!(accepted.last.unwrap().cursor, 1);
        assert_eq!(accepted.published.len(), 1);
        assert_eq!(accepted.published[0].raw_trace_index, 0);
    }
    #[test]
    fn polled_loading_does_not_waive_acknowledged_mismatch_or_quiescent_charge() {
        let mut ledger = ledger();
        finish(&mut ledger, false);
        let mut wrong_cursor = trace();
        if let ObserverTrace::WindowHarness { next_ordinal, .. } = &mut wrong_cursor.observer {
            *next_ordinal = 1;
        }
        let mut failed_charge = trace();
        failed_charge.manager_charge_succeeded = false;
        let mut uncharged = trace();
        uncharged.manager_charge_ns = None;
        uncharged.manager_charge_succeeded = false;
        let mut noninitial_sequence = uncharged.clone();
        noninitial_sequence.sequence = Some(1);
        let mut noninitial_revision = uncharged.clone();
        noninitial_revision.revision = 1;
        for (loading, candidate) in [
            (true, wrong_cursor),
            (true, failed_charge),
            (false, uncharged),
            (true, noninitial_sequence),
            (true, noninitial_revision),
        ] {
            let mut accepted = Acceptance::default();
            assert!(
                accepted
                    .observe_polled(
                        loading,
                        Some(ledger.observation()),
                        Some(life()),
                        &snapshot(candidate)
                    )
                    .is_err()
            );
            assert!(accepted.published.is_empty());
        }
        let mut gap = snapshot(trace());
        gap.raw_trace.push(trace());
        let mut accepted = Acceptance::default();
        assert!(
            accepted
                .observe_polled(true, Some(ledger.observation()), Some(life()), &gap)
                .is_err()
        );
        assert!(accepted.published.is_empty());
    }
    #[test]
    fn charge_alone_cannot_publish_missing_or_parent_rejected_ledger() {
        let mut accepted = Acceptance::default();
        assert!(
            accepted
                .observe(None, Some(life()), &snapshot(trace()))
                .is_err()
        );
        let mut ledger = ledger();
        ledger.fail_parent(ObserverParentFailure::Validation);
        let mut accepted = Acceptance::default();
        accepted
            .observe(Some(ledger.observation()), None, &snapshot(trace()))
            .unwrap();
        assert!(accepted.last.is_none());
        assert!(accepted.published.is_empty());
        assert_eq!(ledger.observation().cursor, 0);
    }
    #[test]
    fn correlation_rejects_wrong_run_cursor_progress_fault_charge_and_lifecycle() {
        let mut ledger = ledger();
        finish(&mut ledger, false);
        let mut variants = Vec::new();
        let mut t = trace();
        if let ObserverTrace::WindowHarness { run_id, .. } = &mut t.observer {
            *run_id = 2;
        }
        variants.push(t);
        let mut t = trace();
        if let ObserverTrace::WindowHarness { next_ordinal, .. } = &mut t.observer {
            *next_ordinal = 1;
        }
        variants.push(t);
        let mut t = trace();
        if let ObserverTrace::WindowHarness { progress, .. } = &mut t.observer {
            *progress = ObserverProgress::Settling;
        }
        variants.push(t);
        let mut t = trace();
        t.observer = ObserverTrace::ObservationFailed {
            run_id: RUN,
            prefix_end: 2,
            fault: ObserverFault::PrivacyLost,
            execution_failed: false,
        };
        variants.push(t);
        let mut t = trace();
        t.manager_charge_succeeded = false;
        variants.push(t);
        let mut t = trace();
        t.manager_charge_ns = None;
        variants.push(t);
        let mut t = trace();
        t.lifecycle = None;
        variants.push(t);
        let mut t = trace();
        if let ObserverTrace::WindowHarness { first_ordinal, .. } = &mut t.observer {
            *first_ordinal = 1;
        }
        variants.push(t);
        let mut t = trace();
        if let ObserverTrace::WindowHarness {
            result_callbacks, ..
        } = &mut t.observer
        {
            *result_callbacks = 0;
        }
        variants.push(t);
        for trace in variants {
            let mut accepted = Acceptance::default();
            assert!(
                accepted
                    .observe(Some(ledger.observation()), Some(life()), &snapshot(trace))
                    .is_err()
            );
            assert!(accepted.published.is_empty());
        }
    }
    #[test]
    fn raw_transaction_gap_is_not_accepted_history() {
        let mut ledger = ledger();
        finish(&mut ledger, false);
        let mut raw = snapshot(trace());
        raw.raw_trace.push(trace());
        let mut accepted = Acceptance::default();
        assert!(
            accepted
                .observe(Some(ledger.observation()), Some(life()), &raw)
                .is_err()
        );
    }
    #[test]
    fn native_failure_prefix_survives_later_validated_policy_refusal() {
        let mut ledger = ledger();
        finish(&mut ledger, true);
        let mut first = trace();
        if let ObserverTrace::WindowHarness {
            execution_failed, ..
        } = &mut first.observer
        {
            *execution_failed = true;
        }
        let mut raw = snapshot(first);
        let mut accepted = Acceptance::default();
        accepted
            .observe(Some(ledger.observation()), Some(life()), &raw)
            .unwrap();
        raw.raw_trace.push(trace());
        raw.first_failure = Some(super::super::guard::FailureFact {
            ordinal: 1,
            failure: HostFailure::Policy {
                reason: super::super::guard::PolicyFailure::Navigation,
            },
        });
        ledger.fail_parent(ObserverParentFailure::Transport);
        accepted
            .observe(Some(ledger.observation()), Some(life()), &raw)
            .unwrap();
        assert_eq!(accepted.first_failure, Some(FailureCause::NativeExecution));
        assert_eq!(accepted.published.len(), 1);
    }
    #[test]
    fn reportless_rejected_init_is_not_invented_ready() {
        let mut ledger = ledger();
        ledger
            .stage(ObserverReply::rejected_init(
                ObserverExpectation::WindowHarness { run_id: RUN },
            ))
            .unwrap()
            .commit();
        let mut t = trace();
        t.observer = ObserverTrace::ObservationFailed {
            run_id: RUN,
            prefix_end: 0,
            fault: ObserverFault::RejectedInit,
            execution_failed: true,
        };
        t.state = RealmState::Closed;
        t.lifecycle = Some(LifecycleProgress {
            phase: LifecyclePhase::Initializing,
            initial_failure: true,
        });
        t.resources = None;
        t.rejected_initial = true;
        let mut accepted = Acceptance::default();
        accepted
            .observe(Some(ledger.observation()), None, &snapshot(t))
            .unwrap();
        let last = accepted.last.unwrap();
        assert_eq!(last.state, RealmState::Closed);
        assert_eq!(last.pending_work, None);
        assert_eq!(last.lifecycle, None);
    }
    #[test]
    fn typed_raw_capture_integrity_cannot_be_hidden_by_policy_or_native_flag() {
        for fault in [
            ObserverFault::MalformedCapture,
            ObserverFault::DuplicateResult,
            ObserverFault::DuplicateCompletion,
            ObserverFault::ResultAfterCompletion,
            ObserverFault::InvariantFailure,
            ObserverFault::AuthorityUnavailable,
            ObserverFault::InvalidConfiguration,
        ] {
            let mut t = trace();
            t.observer = ObserverTrace::ObservationFailed {
                run_id: RUN,
                prefix_end: 0,
                fault,
                execution_failed: true,
            };
            assert!(raw_integrity(&t).is_some());
        }
        for fault in [
            ObserverFault::StorageAdmission,
            ObserverFault::RealmTerminated,
            ObserverFault::CaptureOverflow,
            ObserverFault::PrivacyLost,
            ObserverFault::RejectedInit,
            ObserverFault::NavigationProposed,
        ] {
            let mut t = trace();
            t.observer = ObserverTrace::ObservationFailed {
                run_id: RUN,
                prefix_end: 0,
                fault,
                execution_failed: true,
            };
            assert!(raw_integrity(&t).is_none());
        }
    }
    #[test]
    fn borrowed_accepted_artifact_preserves_utf16_optional_and_final_links() {
        let mut ledger = ledger();
        finish(&mut ledger, false);
        let value = serde_json::to_value(accepted_view(Some(ledger.observation()), RUN)).unwrap();
        assert_eq!(value["missing"], false);
        assert_eq!(
            value["records"][0]["test"]["name"],
            serde_json::json!([55296])
        );
        assert_eq!(value["records"][0]["test"]["message"]["kind"], "Null");
        assert_eq!(value["records"][0]["test"]["stack"]["kind"], "Undefined");
        assert_eq!(value["reconciliations"][0]["kind"], "Unchanged");
        let missing = serde_json::to_value(accepted_view(None, RUN)).unwrap();
        assert_eq!(missing["missing"], true);
        assert!(missing["records"].is_null());
        assert!(missing["progress"].is_null());
    }
    #[test]
    fn raw_initial_infrastructure_is_retained_not_normalized() {
        let original = InitialScriptOutcome {
            status: InitialScriptStatus::Failed,
            failure: Some(InitialScriptFailure::Infrastructure),
            scripts_executed: 0,
            error_count: 1,
            broker_rejected_sources: 0,
            diagnostics: vec!["opaque transport".into()],
        };
        assert_eq!(
            initial_evidence(Some(&original)),
            InitialEvidence::InfrastructureFailed
        );
        let value = serde_json::to_value(raw_initial(&original)).unwrap();
        assert_eq!(value["failure"], "Infrastructure");
        assert_eq!(value["status"], "Failed");
        assert_eq!(value["diagnostics"][0], "opaque transport");
    }
    #[test]
    fn missing_helpers_stay_inventory_without_an_authoritative_admission_witness() {
        // The production counter also includes network, MIME, UTF-8 and wire
        // rejection. Even a clean fixture prefix cannot refine its cause.
        for rejected in [0, 1, 3] {
            let original = InitialScriptOutcome {
                status: InitialScriptStatus::Failed,
                failure: Some(InitialScriptFailure::Infrastructure),
                scripts_executed: 0,
                error_count: 1,
                broker_rejected_sources: rejected,
                diagnostics: vec![],
            };
            assert_eq!(
                initial_evidence(Some(&original)),
                InitialEvidence::InfrastructureFailed
            );
            assert_eq!(
                infrastructure(HostFailure::Source {
                    error: source::SourceError::HelperCount
                }),
                Some(Infrastructure::Inventory)
            );
        }
    }
    #[test]
    fn concrete_first_failure_preserves_secondary_native_history_and_unknown_absence() {
        let mut ledger = ledger();
        ledger
            .stage(ObserverReply::ObservationFailed {
                run_id: RUN,
                prefix_end: 0,
                fault: ObserverFault::CaptureOverflow,
                execution_failed: true,
            })
            .unwrap()
            .commit();
        let view = ledger.observation();
        let first_cause = Some(FailureCause::ObservationLimit);
        assert_eq!(
            accepted_native_history(Some(view.execution_failed)),
            NativeHistory::Failed
        );
        assert_eq!(first_cause, Some(FailureCause::ObservationLimit));
        assert_eq!(accepted_native_history(Some(false)), NativeHistory::Clean);
        assert_eq!(accepted_native_history(None), NativeHistory::Unknown);
    }
}
