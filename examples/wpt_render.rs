//! WPT reftest adapter over the real Chassis navigation, resources, paint and
//! restricted Linux script-worker path.
//!
//! This is not WebDriver or a general testharness executor. The caller serves a
//! reviewed pinned selection on loopback and enforces the outer timeout.
//! Run: wpt_render http://127.0.0.1:PORT/test.html FONT.ttf OUTPUT_PREFIX [--enable-scripts]
use mg_browser::platform::{self, script_worker};
use mg_chassis::{
    Browser, InitialScriptFailure, InitialScriptOutcome, InitialScriptStatus,
    scripts::{DisabledScripts, ScriptRuntime, ScriptSession, SessionUpdate},
};
use mg_sparkle::{
    document::{self, Node},
    js_browser::Request,
    page_session::{DefaultAction, SessionInput, SessionReply, TaskPump},
    paint::{Canvas, Fonts},
    render::{self, Controls, Viewport},
};
use serde_json::json;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use url::Url;

#[global_allocator]
static ALLOCATOR: platform::worker_memory::WorkerAllocator =
    platform::worker_memory::WorkerAllocator;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;
const LOAD_TIMEOUT: Duration = Duration::from_secs(35);
const FAILURE_REASON_BYTES: usize = 1024;
// Mirror the existing document projection bounds, not a new worker profile.
const MAX_ARENA_TAGS: usize = 50_000;
const MAX_ARENA_TAG_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
struct AdapterFailure {
    outcome: &'static str,
    kind: &'static str,
    phase: &'static str,
    reason: String,
}

impl AdapterFailure {
    fn admitted(phase: &'static str, reason: impl Into<String>) -> Self {
        Self {
            outcome: "FAIL",
            kind: "admitted-execution",
            phase,
            reason: reason.into(),
        }
    }
    fn infrastructure(phase: &'static str, reason: impl Into<String>) -> Self {
        Self {
            outcome: "ERROR",
            kind: "infrastructure",
            phase,
            reason: reason.into(),
        }
    }
    fn deadline(reason: impl Into<String>) -> Self {
        Self {
            outcome: "TIMEOUT",
            kind: "deadline",
            phase: "navigation",
            reason: reason.into(),
        }
    }
    fn report(&self, url: &str) -> serde_json::Value {
        let mut reason = self.reason.clone();
        if reason.is_empty() {
            reason = "Adapter failed without a diagnostic".into();
        }
        if reason.len() > FAILURE_REASON_BYTES {
            let mut end = FAILURE_REASON_BYTES - "[truncated]".len();
            while !reason.is_char_boundary(end) {
                end -= 1;
            }
            reason.truncate(end);
            reason.push_str("[truncated]");
        }
        json!({"schema_version": 1, "outcome": self.outcome, "kind": self.kind,
               "url": url, "phase": self.phase, "reason": reason})
    }
}

// Test-host policy only, not production navigation/network isolation. Source
// preflight independently rejects original meta
// refresh markup; otherwise a rejected worker's source fallback could refresh.
// Never forward script-proposed navigation or a refresh-producing projection to
// Chassis, which can schedule the resulting network request in the same poll.
struct NoNavigationScripts {
    inner: Arc<dyn ScriptRuntime>,
    host_failures: Arc<ScriptHostFailures>,
}
struct NoNavigationSession {
    inner: Box<dyn ScriptSession>,
    pending_validation: Duration,
    page_url: String,
    tags: StableArenaTags,
    staged_tags: Option<StagedArenaTags>,
    acknowledgement_failed: bool,
    host_failures: Arc<ScriptHostFailures>,
}
#[derive(Default)]
struct StableArenaTags {
    tags: Vec<String>,
    bytes: usize,
}
struct StagedArenaTags {
    tags: Vec<String>,
    bytes: usize,
}
impl StableArenaTags {
    fn stage(&mut self, reply: &SessionReply) -> Result<StagedArenaTags, String> {
        let nodes = reply
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| snapshot.nodes.as_slice());
        if nodes.len() > MAX_ARENA_TAGS {
            return Err("Script snapshot tag metadata node limit exceeded".into());
        }
        // Do not replace prior identities before Chassis validates them. Only
        // append new tags after a successful acknowledgement. A parent rejecting
        // an ordinary snapshot charges it, then cancels the session; it cannot
        // become the predecessor of another accepted snapshot.
        let appended = nodes.get(self.tags.len()..).unwrap_or_default();
        let bytes = appended
            .iter()
            .try_fold(0usize, |bytes, node| bytes.checked_add(node.tag.len()));
        let bytes = bytes
            .filter(|bytes| self.bytes.saturating_add(*bytes) <= MAX_ARENA_TAG_BYTES)
            .ok_or("Script snapshot tag metadata byte limit exceeded")?;
        let mut tags = Vec::new();
        tags.try_reserve_exact(appended.len())
            .map_err(|_| "Cannot reserve script snapshot tag metadata")?;
        self.tags
            .try_reserve_exact(appended.len())
            .map_err(|_| "Cannot reserve stable script snapshot tag metadata")?;
        for node in appended {
            let mut tag = String::new();
            tag.try_reserve_exact(node.tag.len())
                .map_err(|_| "Cannot reserve script snapshot tag bytes")?;
            tag.push_str(&node.tag);
            tags.push(tag);
        }
        Ok(StagedArenaTags { tags, bytes })
    }
    fn commit(&mut self, staged: StagedArenaTags) {
        // stage reserved this capacity before the timed acknowledgement.
        self.tags.extend(staged.tags);
        self.bytes += staged.bytes;
    }
    fn validate(&self, nodes: &[Node]) -> Result<(), String> {
        if nodes.len() < self.tags.len()
            || self
                .tags
                .iter()
                .zip(nodes)
                .any(|(tag, node)| *tag != node.tag)
        {
            return Err("Script snapshot reused or removed stable arena identities".into());
        }
        Ok(())
    }
}
enum ScriptReplyRejection {
    Policy(String),
    Infrastructure(String),
}
impl ScriptReplyRejection {
    fn reason(&self) -> &str {
        match self {
            Self::Policy(reason) | Self::Infrastructure(reason) => reason,
        }
    }
}
#[derive(Default)]
struct ScriptHostFailures {
    reason: Mutex<Option<String>>,
    admitted_reason: Mutex<Option<String>>,
}
impl ScriptHostFailures {
    fn observe<T>(&self, result: Result<T, String>) -> Result<T, String> {
        if let Err(error) = &result
            && let Ok(mut reason) = self.reason.lock()
        {
            reason.get_or_insert_with(|| error.clone());
        }
        result
    }
    fn check(&self) -> Result<(), AdapterFailure> {
        let reason = self.reason.lock().map_err(|_| {
            AdapterFailure::infrastructure(
                "script-host",
                "Script-host failure observer unavailable",
            )
        })?;
        if let Some(reason) = reason.as_ref() {
            return Err(AdapterFailure::infrastructure(
                "script-host",
                reason.clone(),
            ));
        }
        let admitted = self.admitted_reason.lock().map_err(|_| {
            AdapterFailure::infrastructure("script-host", "Script policy observer unavailable")
        })?;
        match admitted.as_ref() {
            Some(reason) => Err(AdapterFailure::admitted("script", reason.clone())),
            None => Ok(()),
        }
    }
    fn reject_policy(&self, reason: &str) -> Result<(), String> {
        let mut admitted = self.admitted_reason.lock().map_err(|_| {
            let error = "Script policy observer unavailable".to_string();
            let _ = self.observe::<()>(Err(error.clone()));
            error
        })?;
        admitted.get_or_insert_with(|| reason.to_owned());
        Ok(())
    }
    fn record_rejection(&self, rejection: &ScriptReplyRejection) -> Result<(), String> {
        match rejection {
            ScriptReplyRejection::Policy(reason) => self.reject_policy(reason),
            ScriptReplyRejection::Infrastructure(reason) => {
                let _ = self.observe::<()>(Err(reason.clone()));
                Ok(())
            }
        }
    }
}
fn admit_script_reply(reply: &SessionReply) -> Result<(), String> {
    if reply.navigation.is_some() || reply.default_action != DefaultAction::None {
        return Err("WPT adapter does not support script navigation or navigation defaults".into());
    }
    if reply.snapshot.as_ref().is_some_and(|snapshot| {
        snapshot.nodes.iter().any(|node| {
            node.tag.eq_ignore_ascii_case("meta")
                && node.attributes.iter().any(|(name, value)| {
                    name.eq_ignore_ascii_case("http-equiv")
                        && value.trim().eq_ignore_ascii_case("refresh")
                })
        })
    }) {
        return Err("WPT adapter does not support meta refresh in script projections".into());
    }
    Ok(())
}
fn validate_policy_rejection(
    reply: &mut SessionReply,
    page_url: &str,
    previous: Option<&StableArenaTags>,
) -> Result<(), ScriptReplyRejection> {
    let Err(policy) = admit_script_reply(reply) else {
        return Ok(());
    };
    // A denied reply never reaches Chassis. Validate its owned arena here so
    // malformed projections cannot masquerade as ordinary policy FAILs. This
    // is not a second protocol-envelope validator; the actual Linux transport
    // still validates its trusted report, accounting and lifecycle fields.
    let validation = (|| {
        let snapshot = reply
            .snapshot
            .take()
            .ok_or("Script session has no accepted snapshot")?;
        if let Some(previous) = previous {
            previous.validate(&snapshot.nodes)?;
        }
        document::project_nodes(snapshot.nodes, page_url).map(|_| ())
    })();
    match validation {
        Ok(()) => Err(ScriptReplyRejection::Policy(policy)),
        Err(error) => Err(ScriptReplyRejection::Infrastructure(error)),
    }
}
impl ScriptRuntime for NoNavigationScripts {
    fn set_generation(&self, generation: u64) -> Result<(), String> {
        self.host_failures
            .observe(self.inner.set_generation(generation))
    }
    fn start(
        &self,
        request: Request,
        generation: u64,
    ) -> Result<(Box<dyn ScriptSession>, SessionReply), String> {
        let page_url = request.url.clone();
        let (mut session, mut reply) = self
            .host_failures
            .observe(self.inner.start(request, generation))?;
        let started = Instant::now();
        let mut tags = StableArenaTags::default();
        let validation = validate_policy_rejection(&mut reply, &page_url, None).and_then(|()| {
            tags.stage(&reply)
                .map_err(ScriptReplyRejection::Infrastructure)
        });
        let elapsed = started.elapsed();
        let staged_tags = match validation {
            Ok(staged) => staged,
            Err(rejection) => {
                // Rejected replies never reach Chassis, so acknowledge their
                // validation here before cancellation. Opaque host errors remain
                // infrastructure errors rather than being inferred from text.
                let charged = self.host_failures.observe(session.charge_active(elapsed));
                session.cancel();
                charged?;
                self.host_failures.record_rejection(&rejection)?;
                return Err(rejection.reason().into());
            }
        };
        Ok((
            Box::new(NoNavigationSession {
                inner: session,
                pending_validation: elapsed,
                page_url,
                tags,
                staged_tags: Some(staged_tags),
                acknowledgement_failed: false,
                host_failures: self.host_failures.clone(),
            }),
            reply,
        ))
    }
    fn close(&self) {
        self.inner.close();
    }
    fn wait_idle(&self) -> Result<(), String> {
        self.inner.wait_idle()
    }
}
impl ScriptSession for NoNavigationSession {
    fn id(&self) -> u64 {
        self.inner.id()
    }
    fn try_dispatch(&mut self, input: SessionInput, revision: u64) -> Result<u64, String> {
        if self.acknowledgement_failed {
            return Err("Script projection acknowledgement failed".into());
        }
        self.host_failures
            .observe(self.inner.try_dispatch(input, revision))
    }
    fn try_task_pump(&mut self, pump: TaskPump, revision: u64) -> Result<u64, String> {
        if self.acknowledgement_failed {
            return Err("Script projection acknowledgement failed".into());
        }
        // Preserve the production host API. Replies still pass through the
        // same navigation/default guard and exactly-one combined charge below.
        self.host_failures
            .observe(self.inner.try_task_pump(pump, revision))
    }
    fn try_recv(&mut self) -> Option<SessionUpdate> {
        if self.acknowledgement_failed {
            return None;
        }
        let mut update = self.inner.try_recv()?;
        if let Ok(reply) = &mut update.reply {
            let started = Instant::now();
            let validation = validate_policy_rejection(reply, &self.page_url, Some(&self.tags))
                .and_then(|()| {
                    self.tags
                        .stage(reply)
                        .map_err(ScriptReplyRejection::Infrastructure)
                });
            let elapsed = started.elapsed();
            match validation {
                Err(rejection) => {
                    let charged = self
                        .host_failures
                        .observe(self.inner.charge_active(elapsed));
                    self.inner.cancel();
                    self.acknowledgement_failed = true;
                    self.staged_tags = None;
                    let recorded = if charged.is_ok() {
                        self.host_failures.record_rejection(&rejection)
                    } else {
                        Ok(())
                    };
                    update.reply = Err(charged
                        .err()
                        .or_else(|| recorded.err())
                        .unwrap_or_else(|| rejection.reason().into()));
                }
                Ok(staged) => {
                    self.pending_validation = self.pending_validation.saturating_add(elapsed);
                    self.staged_tags = Some(staged);
                }
            }
        } else if let Err(error) = &update.reply {
            let _ = self.host_failures.observe::<()>(Err(error.clone()));
        }
        Some(update)
    }
    fn cancel(&self) {
        self.inner.cancel();
    }
    fn charge_active(&mut self, duration: Duration) -> Result<(), String> {
        if self.acknowledgement_failed {
            return Err("Script projection acknowledgement failed".into());
        }
        // One completion permits exactly one acknowledgement. Include this
        // adapter's validation in Chassis's acknowledgement, never consume the
        // inner session's validation_pending flag before Chassis can use it.
        let validation = std::mem::take(&mut self.pending_validation);
        let staged = self.staged_tags.take();
        let charged = self.host_failures.observe(
            self.inner
                .charge_active(duration.saturating_add(validation)),
        );
        if let Some(staged) = staged {
            if charged.is_ok() {
                self.tags.commit(staged);
            } else {
                self.acknowledgement_failed = true;
                self.inner.cancel();
            }
        }
        charged
    }
}

fn local_url(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| format!("Invalid test URL: {error}"))?;
    let authority = value
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .ok_or("Test URL must use http://127.0.0.1 with an explicit port")?;
    let port = authority
        .strip_prefix("127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or("Test URL must use http://127.0.0.1 with an explicit nonzero port")?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(port)
    {
        return Err("Test URL must be loopback HTTP without credentials, query or fragment".into());
    }
    Ok(url)
}

fn artifact_path(prefix: &Path, suffix: &str) -> PathBuf {
    let mut path: OsString = prefix.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

fn check_final_url(requested: &Url, value: &str) -> Result<(), String> {
    let final_url = Url::parse(value).map_err(|error| format!("Invalid final URL: {error}"))?;
    // URL serialization removes an explicit default :80 port. Compare the
    // parsed origin rather than rejecting that legitimate normalization.
    if requested.origin() != final_url.origin()
        || !final_url.username().is_empty()
        || final_url.password().is_some()
        || final_url.query().is_some()
        || final_url.fragment().is_some()
    {
        return Err("Test navigation changed origin or added credentials/query/fragment".into());
    }
    Ok(())
}

fn rgb_bytes(canvas: &Canvas) -> Result<Vec<u8>, String> {
    if canvas.width != WIDTH
        || canvas.height != HEIGHT
        || canvas.pixels.len() != WIDTH as usize * HEIGHT as usize
        || canvas.scale() != 1.0
    {
        return Err("Expected an exact 800x600 viewport at scale 1".into());
    }
    let mut rgb = Vec::with_capacity(canvas.pixels.len() * 3);
    for &pixel in &canvas.pixels {
        rgb.extend_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]);
    }
    Ok(rgb)
}

fn check_initial_script(outcome: &InitialScriptOutcome) -> Result<(), AdapterFailure> {
    let reason = format!(
        "Initial scripts: {:?}; executed={}; errors={}; broker-rejected={}; {}",
        outcome.status,
        outcome.scripts_executed,
        outcome.error_count,
        outcome.broker_rejected_sources,
        outcome
            .diagnostics
            .first()
            .map(String::as_str)
            .unwrap_or("")
    );
    if outcome.failure == Some(InitialScriptFailure::Infrastructure) {
        return Err(AdapterFailure::infrastructure("script-host", reason));
    }
    match outcome.status {
        InitialScriptStatus::Complete if outcome.failure.is_none() => Ok(()),
        InitialScriptStatus::Partial | InitialScriptStatus::Failed => {
            Err(AdapterFailure::admitted("script", reason))
        }
        _ => Err(AdapterFailure::infrastructure("script-host", reason)),
    }
}

fn render_page(
    url: &str,
    font_path: &Path,
    prefix: &Path,
    enable_scripts: bool,
) -> Result<(), AdapterFailure> {
    let requested =
        local_url(url).map_err(|error| AdapterFailure::infrastructure("configuration", error))?;
    let font = fs::read(font_path).map_err(|error| {
        AdapterFailure::infrastructure("font", format!("Cannot read font: {error}"))
    })?;
    let host_failures = Arc::new(ScriptHostFailures::default());
    let scripts: Arc<dyn ScriptRuntime> = if enable_scripts {
        Arc::new(NoNavigationScripts {
            inner: Arc::new(platform::LinuxScripts::default()),
            host_failures: host_failures.clone(),
        })
    } else {
        Arc::new(DisabledScripts)
    };
    let mut browser = Browser::new(
        Fonts::from_bytes(font.clone())
            .map_err(|error| AdapterFailure::infrastructure("font", error))?,
        scripts.clone(),
    );
    browser.set_chrome(false);
    browser.set_scripts_enabled(enable_scripts);
    browser.resize(WIDTH, HEIGHT);
    browser.navigate(requested.to_string(), None, false);
    let deadline = Instant::now() + LOAD_TIMEOUT;
    while browser.is_loading() {
        if Instant::now() >= deadline {
            return Err(AdapterFailure::deadline(format!(
                "Navigation timed out: {}",
                browser.status()
            )));
        }
        browser.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    host_failures.check()?;
    if !browser.last_load_succeeded() {
        return Err(AdapterFailure::admitted(
            "navigation",
            format!("Navigation failed: {}", browser.status()),
        ));
    }
    // This is the typed initial-load result, never a transient task status,
    // harness completion signal or permission to wait for arbitrary timers.
    if enable_scripts {
        check_initial_script(browser.initial_script_outcome())?;
    }
    check_final_url(&requested, browser.page_url())
        .map_err(|error| AdapterFailure::admitted("navigation", error))?;
    let canvas = browser.paint();
    let rgb =
        rgb_bytes(&canvas).map_err(|error| AdapterFailure::infrastructure("artifact", error))?;

    // Browser keeps paint diagnostics private. Render its actual loaded DOM
    // again with identical inputs, then require byte-identical output before
    // associating the second pass's diagnostics with Browser::paint's pixels.
    let frame = render::render(
        browser.document(),
        &mut Fonts::from_bytes(font)
            .map_err(|error| AdapterFailure::infrastructure("font", error))?,
        Viewport {
            width: WIDTH,
            height: HEIGHT,
            scroll: 0,
        },
        &Controls::default(),
    );
    if canvas.width != frame.canvas.width
        || canvas.height != frame.canvas.height
        || canvas.pixels != frame.canvas.pixels
    {
        return Err(AdapterFailure::infrastructure(
            "replay",
            "Diagnostic replay differs from the actual Browser::paint output",
        ));
    }
    let diagnostics: Vec<_> = frame
        .diagnostics
        .entries
        .iter()
        .map(|diagnostic| {
            json!({
                "kind": diagnostic.kind,
                "message": diagnostic.message,
                "source": diagnostic.source,
                "node": diagnostic.node,
                "line": diagnostic.line,
                "column": diagnostic.column,
                "truncated": diagnostic.truncated,
            })
        })
        .collect();
    let final_url = browser.page_url().to_owned();
    let resource_warnings = browser.document().resource_warnings.clone();
    let stylesheets = browser.document().stylesheets.len();
    let resources = browser.document().resources.len();
    let status = browser.status().to_owned();
    drop(browser);
    scripts
        .wait_idle()
        .map_err(|error| AdapterFailure::infrastructure("cleanup", error))?;
    let report = json!({
        "schema_version": 1,
        "url": url,
        "final_url": final_url,
        "width": WIDTH,
        "height": HEIGHT,
        "scale": 1,
        "non_uniform": canvas.pixels.iter().any(|pixel| *pixel != canvas.pixels[0]),
        "content_height": frame.content_height,
        "diagnostics": diagnostics,
        "diagnostics_omitted": frame.diagnostics.omitted,
        "resource_warnings": resource_warnings,
        "stylesheets": stylesheets,
        "resources": resources,
        "status": status,
        "scripts": enable_scripts,
        "browser_replay_equal": true,
    });
    let png_path = artifact_path(prefix, ".png");
    canvas
        .save_png(
            png_path.to_str().ok_or_else(|| {
                AdapterFailure::infrastructure("artifact", "PNG path is not UTF-8")
            })?,
        )
        .map_err(|error| AdapterFailure::infrastructure("artifact", error))?;
    fs::write(artifact_path(prefix, ".rgb"), rgb).map_err(|error| {
        AdapterFailure::infrastructure("artifact", format!("Cannot write RGB artifact: {error}"))
    })?;
    fs::write(
        artifact_path(prefix, ".json"),
        serde_json::to_vec_pretty(&report)
            .map_err(|error| AdapterFailure::infrastructure("artifact", error.to_string()))?,
    )
    .map_err(|error| {
        AdapterFailure::infrastructure("artifact", format!("Cannot write report: {error}"))
    })?;
    println!("WPT_RENDER_OK {} {}x{}", final_url, WIDTH, HEIGHT);
    Ok(())
}

fn main() -> Result<(), String> {
    // Worker dispatch must precede fonts, networking and browser setup.
    match std::env::args().nth(1).as_deref() {
        Some("--script-worker") => script_worker::worker_entry(),
        Some("--script-session") => script_worker::session_entry(),
        _ => {}
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    let enable_scripts = match args.len() {
        3 => false,
        4 if args[3] == "--enable-scripts" => true,
        _ => {
            return Err(
                "Usage: wpt_render http://127.0.0.1:PORT/test.html FONT.ttf OUTPUT_PREFIX [--enable-scripts]"
                    .into(),
            );
        }
    };
    if args[2].is_empty() {
        return Err(
            "Usage: wpt_render http://127.0.0.1:PORT/test.html FONT.ttf OUTPUT_PREFIX [--enable-scripts]"
                .into(),
        );
    }
    if let Err(failure) = render_page(
        &args[0],
        Path::new(&args[1]),
        Path::new(&args[2]),
        enable_scripts,
    ) {
        let failure_report = failure.report(&args[0]);
        let report =
            serde_json::to_vec_pretty(&failure_report).map_err(|error| error.to_string())?;
        fs::write(artifact_path(Path::new(&args[2]), ".json"), report)
            .map_err(|error| format!("Cannot write structured adapter failure: {error}"))?;
        eprintln!(
            "WPT_RENDER_{} phase={} {}",
            failure.outcome,
            failure.phase,
            failure_report["reason"]
                .as_str()
                .unwrap_or("Adapter failure")
        );
        std::process::exit(2);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mg_sparkle::page_session::{ArenaSnapshot, EventOutcome, RealmState};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FakeSession {
        canceled: Arc<AtomicBool>,
        charges: Arc<Mutex<Vec<Duration>>>,
        validation_pending: bool,
        fail_charge: Option<usize>,
        update: Option<SessionUpdate>,
    }
    impl ScriptSession for FakeSession {
        fn id(&self) -> u64 {
            99
        }
        fn try_dispatch(&mut self, _: SessionInput, _: u64) -> Result<u64, String> {
            assert!(
                !self.validation_pending,
                "Fixture projection is still pending"
            );
            Ok(1)
        }
        fn try_task_pump(&mut self, pump: TaskPump, revision: u64) -> Result<u64, String> {
            if self.validation_pending {
                return Err("Fixture projection is still pending".into());
            }
            assert_eq!(pump.wake_token, 7);
            assert_eq!(revision, 0);
            Ok(1)
        }
        fn try_recv(&mut self) -> Option<SessionUpdate> {
            assert!(
                !self.validation_pending,
                "Fixture projection is still pending"
            );
            let update = self.update.take()?;
            self.validation_pending = update.reply.is_ok();
            Some(update)
        }
        fn cancel(&self) {
            self.canceled.store(true, Ordering::SeqCst);
        }
        fn charge_active(&mut self, duration: Duration) -> Result<(), String> {
            if !self.validation_pending {
                return Err("No fixture projection awaits validation".into());
            }
            self.validation_pending = false;
            let mut charges = self.charges.lock().unwrap();
            charges.push(duration);
            if self.fail_charge == Some(charges.len()) {
                return Err("Fixture active charge failed".into());
            }
            Ok(())
        }
    }
    struct FakeScripts {
        startup: Mutex<Option<Result<(Box<dyn ScriptSession>, SessionReply), String>>>,
    }
    impl ScriptRuntime for FakeScripts {
        fn set_generation(&self, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn start(
            &self,
            _: Request,
            _: u64,
        ) -> Result<(Box<dyn ScriptSession>, SessionReply), String> {
            self.startup
                .lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Err("Fixture already started".into()))
        }
        fn close(&self) {}
        fn wait_idle(&self) -> Result<(), String> {
            Ok(())
        }
    }
    fn reply(html: &str) -> SessionReply {
        SessionReply {
            revision: 0,
            snapshot: Some(ArenaSnapshot {
                nodes: mg_sparkle::document::parse_with_scripting(
                    html,
                    "http://127.0.0.1:8000/test.html",
                    true,
                )
                .nodes,
            }),
            outcome: EventOutcome::default(),
            default_action: DefaultAction::None,
            navigation: None,
            errors: Vec::new(),
            scripts_executed: 0,
            allocations: None,
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
        }
    }
    fn guarded(
        reply: SessionReply,
        later: Option<SessionReply>,
    ) -> (
        NoNavigationScripts,
        Arc<AtomicBool>,
        Arc<Mutex<Vec<Duration>>>,
    ) {
        guarded_charge(reply, later, None)
    }
    fn guarded_charge(
        reply: SessionReply,
        later: Option<SessionReply>,
        fail_charge: Option<usize>,
    ) -> (
        NoNavigationScripts,
        Arc<AtomicBool>,
        Arc<Mutex<Vec<Duration>>>,
    ) {
        let canceled = Arc::new(AtomicBool::new(false));
        let charges = Arc::new(Mutex::new(Vec::new()));
        let session = FakeSession {
            canceled: canceled.clone(),
            charges: charges.clone(),
            validation_pending: true,
            fail_charge,
            update: later.map(|reply| SessionUpdate {
                generation: 3,
                session_id: 99,
                sequence: Some(1),
                reply: Ok(reply),
                automation: None,
                continuation: mg_chassis::scripts::SessionContinuation::Live,
            }),
        };
        (
            NoNavigationScripts {
                inner: Arc::new(FakeScripts {
                    startup: Mutex::new(Some(Ok((Box::new(session), reply)))),
                }),
                host_failures: Arc::new(ScriptHostFailures::default()),
            },
            canceled,
            charges,
        )
    }
    fn request() -> Request {
        Request {
            url: "http://127.0.0.1:8000/test.html".into(),
            html: "<p>owned</p>".into(),
            external_scripts: None,
        }
    }

    #[test]
    fn failure_envelopes_separate_admitted_failure_infrastructure_and_timeout() {
        for (failure, outcome, kind) in [
            (
                AdapterFailure::admitted("script", "SyntaxError"),
                "FAIL",
                "admitted-execution",
            ),
            (
                AdapterFailure::admitted("navigation", "HTTP 404"),
                "FAIL",
                "admitted-execution",
            ),
            (
                AdapterFailure::infrastructure("font", "Missing font"),
                "ERROR",
                "infrastructure",
            ),
            (
                AdapterFailure::infrastructure("replay", "Pixels differ"),
                "ERROR",
                "infrastructure",
            ),
            (
                AdapterFailure::infrastructure("script-host", "Opaque launch failure"),
                "ERROR",
                "infrastructure",
            ),
            (
                AdapterFailure::deadline("35-second navigation deadline"),
                "TIMEOUT",
                "deadline",
            ),
        ] {
            let report = failure.report("http://127.0.0.1:8000/test.html");
            assert_eq!(report.as_object().unwrap().len(), 6);
            assert_eq!(report["schema_version"], 1);
            assert_eq!(report["outcome"], outcome);
            assert_eq!(report["kind"], kind);
            assert_eq!(report["url"], "http://127.0.0.1:8000/test.html");
            assert!(!report["reason"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn failure_diagnostics_are_nonempty_and_bounded_at_utf8_boundaries() {
        let report = AdapterFailure::admitted("script", "é".repeat(2000)).report("owned");
        let reason = report["reason"].as_str().unwrap();
        assert!(reason.len() <= FAILURE_REASON_BYTES);
        assert!(reason.ends_with("[truncated]"));
        assert!(
            !AdapterFailure::infrastructure("artifact", "").report("owned")["reason"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_host_cancels_initial_navigation_before_chassis_can_observe_it() {
        let mut initial = reply("<p>owned</p>");
        initial.navigation = Some("https://must-not-fetch.invalid/".into());
        let (runtime, canceled, charges) = guarded(initial, None);
        assert!(matches!(runtime.start(request(), 3), Err(error) if error.contains("navigation")));
        assert!(canceled.load(Ordering::SeqCst));
        assert_eq!(charges.lock().unwrap().len(), 1);
        assert_eq!(runtime.host_failures.check().unwrap_err().outcome, "FAIL");
    }

    #[test]
    fn test_host_cancels_projected_meta_refresh_before_parent_projection() {
        for value in ["REFRESH", " refresh "] {
            let (runtime, canceled, _) = guarded(
                reply(&format!(
                    "<meta http-equiv='{value}' content='0;url=https://must-not-fetch.invalid/'>"
                )),
                None,
            );
            assert!(
                matches!(runtime.start(request(), 3), Err(error) if error.contains("meta refresh"))
            );
            assert!(canceled.load(Ordering::SeqCst));
        }
    }

    #[test]
    fn test_host_rejects_later_navigation_refresh_and_navigation_defaults() {
        let html = "<a href=https://must-not-fetch.invalid/>link</a>";
        let mut navigation = reply(html);
        navigation.navigation = Some("https://must-not-fetch.invalid/".into());
        // A later refresh must append a node without replacing existing arena
        // identities, otherwise it is a projection ERROR before policy FAIL.
        let mut refresh = reply(html);
        let nodes = &mut refresh.snapshot.as_mut().unwrap().nodes;
        let meta = nodes.len();
        nodes.push(Node {
            tag: "meta".into(),
            attributes: vec![("http-equiv".into(), "refresh".into())],
            text: String::new(),
            parent: meta,
            children: vec![],
        });
        let mut default = reply(html);
        default.default_action = DefaultAction::FollowLink { node: 1 };
        for later in [navigation, refresh, default] {
            let (runtime, canceled, charges) = guarded(reply(html), Some(later));
            let (mut session, _) = runtime.start(request(), 3).unwrap();
            session.charge_active(Duration::from_millis(4)).unwrap();
            let update = session.try_recv().unwrap();
            assert!(update.reply.is_err());
            assert_eq!(update.session_id, 99);
            assert_eq!(update.generation, 3);
            assert_eq!(update.sequence, Some(1));
            assert!(canceled.load(Ordering::SeqCst));
            assert_eq!(charges.lock().unwrap().len(), 2);
            assert_eq!(runtime.host_failures.check().unwrap_err().outcome, "FAIL");
        }
    }

    #[test]
    fn denied_initial_navigation_or_refresh_does_not_mask_invalid_projection() {
        for refresh in [false, true] {
            for fault in ["root", "edge", "missing"] {
                let mut initial = if refresh {
                    reply(
                        "<meta http-equiv=refresh content='0;url=https://must-not-fetch.invalid/'>",
                    )
                } else {
                    let mut initial = reply("<p>owned</p>");
                    initial.navigation = Some("https://must-not-fetch.invalid/".into());
                    initial
                };
                match fault {
                    "root" => initial.snapshot.as_mut().unwrap().nodes[0].tag = "bad-root".into(),
                    "edge" => initial.snapshot.as_mut().unwrap().nodes[0]
                        .children
                        .push(usize::MAX),
                    "missing" => {
                        initial.snapshot = None;
                        // Without an arena the refresh cannot itself be found;
                        // retain an independent navigation-policy denial.
                        initial.navigation = Some("https://must-not-fetch.invalid/".into());
                    }
                    _ => unreachable!(),
                }
                let (runtime, canceled, charges) = guarded(initial, None);
                assert!(runtime.start(request(), 3).is_err(), "{refresh}/{fault}");
                assert!(canceled.load(Ordering::SeqCst));
                assert_eq!(charges.lock().unwrap().len(), 1);
                let failure = runtime.host_failures.check().unwrap_err();
                assert_eq!(failure.outcome, "ERROR", "{refresh}/{fault}");
                assert_eq!(failure.kind, "infrastructure");
                assert!(
                    runtime
                        .host_failures
                        .admitted_reason
                        .lock()
                        .unwrap()
                        .is_none()
                );
            }
        }
    }

    #[test]
    fn denied_later_projection_keeps_exact_acknowledged_arena_identities() {
        for refresh in [false, true] {
            for fault in ["removed", "retagged", "edge"] {
                // The initial extra detached node is valid and must keep its
                // identity even though it contributes no painted content.
                let mut initial = reply("<p>owned</p>");
                let nodes = &mut initial.snapshot.as_mut().unwrap().nodes;
                let detached = nodes.len();
                nodes.push(Node {
                    tag: "aside".into(),
                    attributes: vec![],
                    text: String::new(),
                    parent: detached,
                    children: vec![],
                });
                let mut later = reply("<p>owned</p>");
                let nodes = &mut later.snapshot.as_mut().unwrap().nodes;
                if fault != "removed" {
                    nodes.push(Node {
                        tag: if fault == "retagged" {
                            "section"
                        } else {
                            "aside"
                        }
                        .into(),
                        attributes: vec![],
                        text: String::new(),
                        parent: detached,
                        children: vec![],
                    });
                }
                if fault == "edge" {
                    nodes[0].children.push(usize::MAX);
                }
                if refresh {
                    // A policy-relevant detached meta is sufficient: the
                    // existing guard rejects any arena refresh element.
                    let meta = nodes.len();
                    nodes.push(Node {
                        tag: "meta".into(),
                        attributes: vec![("http-equiv".into(), "refresh".into())],
                        text: String::new(),
                        parent: meta,
                        children: vec![],
                    });
                } else {
                    later.navigation = Some("https://must-not-fetch.invalid/".into());
                }
                let (runtime, canceled, charges) = guarded(initial, Some(later));
                let (mut session, _) = runtime.start(request(), 3).unwrap();
                session.charge_active(Duration::ZERO).unwrap();
                let update = session.try_recv().unwrap();
                assert!(update.reply.is_err(), "{refresh}/{fault}");
                assert_eq!(
                    (update.generation, update.session_id, update.sequence),
                    (3, 99, Some(1))
                );
                assert!(canceled.load(Ordering::SeqCst));
                assert_eq!(charges.lock().unwrap().len(), 2);
                let failure = runtime.host_failures.check().unwrap_err();
                assert_eq!(failure.outcome, "ERROR", "{refresh}/{fault}");
                assert_eq!(failure.kind, "infrastructure");
                assert!(
                    runtime
                        .host_failures
                        .admitted_reason
                        .lock()
                        .unwrap()
                        .is_none()
                );
                assert!(session.try_recv().is_none());
                assert!(session.charge_active(Duration::ZERO).is_err());
            }
        }
    }

    #[test]
    fn denied_projection_charge_failure_is_infrastructure_and_never_retried() {
        for startup in [false, true] {
            let mut denied = reply("<p>owned</p>");
            denied.navigation = Some("https://must-not-fetch.invalid/".into());
            let (runtime, canceled, charges) = if startup {
                guarded_charge(denied, None, Some(1))
            } else {
                guarded_charge(reply("<p>owned</p>"), Some(denied), Some(2))
            };
            if startup {
                assert!(
                    matches!(runtime.start(request(), 3), Err(error) if error == "Fixture active charge failed")
                );
            } else {
                let (mut session, _) = runtime.start(request(), 3).unwrap();
                session.charge_active(Duration::ZERO).unwrap();
                assert!(
                    matches!(session.try_recv().unwrap().reply, Err(error) if error == "Fixture active charge failed")
                );
                assert!(session.try_recv().is_none());
                assert!(session.charge_active(Duration::ZERO).is_err());
                assert!(
                    session
                        .try_task_pump(TaskPump { wake_token: 7 }, 0)
                        .is_err()
                );
            }
            assert!(canceled.load(Ordering::SeqCst));
            assert_eq!(charges.lock().unwrap().len(), if startup { 1 } else { 2 });
            assert_eq!(runtime.host_failures.check().unwrap_err().outcome, "ERROR");
            assert!(
                runtime
                    .host_failures
                    .admitted_reason
                    .lock()
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[test]
    fn tag_metadata_is_bounded_append_only_and_committed_only_on_acknowledgement() {
        let mut tags = StableArenaTags::default();
        let initial = reply("<p>owned</p>");
        let staged = tags.stage(&initial).unwrap();
        assert!(tags.tags.is_empty());
        assert_eq!(tags.bytes, 0);
        let count = staged.tags.len();
        let bytes = staged.bytes;
        tags.commit(staged);
        assert_eq!(tags.tags.len(), count);
        assert_eq!(tags.bytes, bytes);

        let mut appended = reply("<p>owned</p>");
        let nodes = &mut appended.snapshot.as_mut().unwrap().nodes;
        let detached = nodes.len();
        nodes.push(Node {
            tag: "aside".into(),
            attributes: vec![],
            text: String::new(),
            parent: detached,
            children: vec![],
        });
        let staged = tags.stage(&appended).unwrap();
        assert_eq!(staged.tags, ["aside"]);
        assert_eq!(tags.tags.len(), count);
        drop(staged); // Failed acknowledgements must not adopt this identity.
        assert_eq!(tags.tags.len(), count);
        assert_eq!(tags.bytes, bytes);
        let staged = tags.stage(&initial).unwrap();
        assert!(staged.tags.is_empty());
        assert_eq!(staged.bytes, 0);
        drop(staged);
        assert_eq!(tags.tags.len(), count);
        assert_eq!(tags.bytes, bytes);

        let mut oversized = reply("<p>owned</p>");
        oversized.snapshot.as_mut().unwrap().nodes.resize(
            MAX_ARENA_TAGS + 1,
            Node {
                tag: "p".into(),
                attributes: vec![],
                text: String::new(),
                parent: 0,
                children: vec![],
            },
        );
        assert!(tags.stage(&oversized).is_err());
        let mut oversized = reply("<p>owned</p>");
        oversized.snapshot.as_mut().unwrap().nodes.push(Node {
            tag: "x".repeat(MAX_ARENA_TAG_BYTES),
            attributes: vec![],
            text: String::new(),
            parent: 0,
            children: vec![],
        });
        assert!(tags.stage(&oversized).is_err());
        assert_eq!(tags.tags.len(), count);
        assert_eq!(tags.bytes, bytes);

        let staged = tags.stage(&appended).unwrap();
        tags.commit(staged);
        assert_eq!(tags.tags.len(), count + 1);
        assert_eq!(tags.bytes, bytes + "aside".len());
        assert!(
            tags.validate(&initial.snapshot.as_ref().unwrap().nodes)
                .is_err()
        );
        assert!(
            tags.validate(&appended.snapshot.as_ref().unwrap().nodes)
                .is_ok()
        );

        for fail_charge in [1, 2] {
            let initial = reply("<p>owned</p>");
            let later = reply("<p>owned</p><aside>new</aside>");
            let (runtime, canceled, charges) =
                guarded_charge(initial, Some(later), Some(fail_charge));
            let (mut session, _) = runtime.start(request(), 3).unwrap();
            if fail_charge == 2 {
                session.charge_active(Duration::ZERO).unwrap();
                assert!(session.try_recv().unwrap().reply.is_ok());
            }
            assert!(session.charge_active(Duration::ZERO).is_err());
            assert!(canceled.load(Ordering::SeqCst));
            assert!(session.try_recv().is_none());
            assert!(session.charge_active(Duration::ZERO).is_err());
            assert!(
                session
                    .try_task_pump(TaskPump { wake_token: 7 }, 0)
                    .is_err()
            );
            assert_eq!(charges.lock().unwrap().len(), fail_charge);
            assert_eq!(runtime.host_failures.check().unwrap_err().outcome, "ERROR");
        }
    }

    #[test]
    fn reportless_prerealm_closed_rejection_passes_through_without_projection() {
        use mg_sparkle::page_session::{LifecyclePhase, LifecycleProgress};
        for modern in [false, true] {
            let mut rejected = reply("<p>source fallback</p>");
            rejected.snapshot = None;
            rejected.state = RealmState::Closed;
            rejected
                .errors
                .push("Source rejected before realm creation".into());
            rejected.lifecycle = modern.then_some(LifecycleProgress {
                phase: LifecyclePhase::Initializing,
                initial_failure: true,
            });
            assert!(rejected.is_rejected_initial());
            assert!(rejected.validate_lifecycle_initial().is_ok());
            let (runtime, canceled, charges) = guarded(rejected, None);
            let (mut session, forwarded) = runtime.start(request(), 3).unwrap();
            assert!(forwarded.is_rejected_initial());
            assert!(forwarded.snapshot.is_none());
            assert_eq!(forwarded.errors, ["Source rejected before realm creation"]);
            assert_eq!(forwarded.lifecycle.is_some(), modern);
            assert!(runtime.host_failures.check().is_ok());
            assert!(charges.lock().unwrap().is_empty());
            session.charge_active(Duration::ZERO).unwrap();
            assert_eq!(charges.lock().unwrap().len(), 1);
            assert!(!canceled.load(Ordering::SeqCst));
            session.cancel(); // Chassis owns cancellation of the Closed result.
            assert!(canceled.load(Ordering::SeqCst));
            assert!(runtime.host_failures.check().is_ok());
        }
    }

    #[test]
    fn test_host_preserves_non_navigation_snapshots_and_lifecycle_ownership() {
        let (runtime, canceled, charges) =
            guarded(reply("<p>owned</p>"), Some(reply("<p>later</p>")));
        let (mut session, startup) = runtime.start(request(), 3).unwrap();
        assert!(charges.lock().unwrap().is_empty());
        session.charge_active(Duration::from_millis(4)).unwrap();
        assert_eq!(session.id(), 99);
        assert!(
            startup
                .snapshot
                .unwrap()
                .nodes
                .iter()
                .any(|node| node.text == "owned")
        );
        assert!(
            session
                .try_recv()
                .unwrap()
                .reply
                .unwrap()
                .snapshot
                .unwrap()
                .nodes
                .iter()
                .any(|node| node.text == "later")
        );
        session.charge_active(Duration::from_millis(7)).unwrap();
        assert!(!canceled.load(Ordering::SeqCst));
        let charged = charges.lock().unwrap();
        assert_eq!(charged.len(), 2);
        assert!(charged[0] >= Duration::from_millis(4));
        assert!(charged[1] >= Duration::from_millis(7));
        drop(charged);
        assert!(runtime.host_failures.check().is_ok());
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert_eq!(charges.lock().unwrap().len(), 2);
        session.cancel();
        assert!(canceled.load(Ordering::SeqCst));
    }

    #[test]
    fn test_host_forwards_task_pumps_without_consuming_a_projection_charge() {
        let (runtime, canceled, charges) =
            guarded(reply("<p>owned</p>"), Some(reply("<p>task</p>")));
        let (mut session, _) = runtime.start(request(), 3).unwrap();
        assert!(charges.lock().unwrap().is_empty());
        session.charge_active(Duration::ZERO).unwrap();
        assert_eq!(
            session
                .try_task_pump(TaskPump { wake_token: 7 }, 0)
                .unwrap(),
            1
        );
        assert_eq!(charges.lock().unwrap().len(), 1);
        assert!(session.try_recv().unwrap().reply.is_ok());
        assert_eq!(charges.lock().unwrap().len(), 1);
        session.charge_active(Duration::ZERO).unwrap();
        assert_eq!(charges.lock().unwrap().len(), 2);
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert!(!canceled.load(Ordering::SeqCst));
    }

    #[test]
    fn test_host_combines_validation_with_one_chassis_acknowledgement() {
        let charges = Arc::new(Mutex::new(Vec::new()));
        let mut session = NoNavigationSession {
            inner: Box::new(FakeSession {
                canceled: Arc::new(AtomicBool::new(false)),
                charges: charges.clone(),
                validation_pending: true,
                fail_charge: None,
                update: None,
            }),
            pending_validation: Duration::from_millis(3),
            page_url: request().url,
            tags: StableArenaTags::default(),
            staged_tags: None,
            acknowledgement_failed: false,
            host_failures: Arc::new(ScriptHostFailures::default()),
        };
        session.charge_active(Duration::from_millis(5)).unwrap();
        assert_eq!(*charges.lock().unwrap(), [Duration::from_millis(8)]);
        assert!(session.charge_active(Duration::ZERO).is_err());
        assert_eq!(charges.lock().unwrap().len(), 1);
    }

    #[test]
    fn opaque_host_errors_are_infrastructure_but_well_formed_script_errors_are_not() {
        let host_failures = Arc::new(ScriptHostFailures::default());
        let runtime = NoNavigationScripts {
            inner: Arc::new(FakeScripts {
                startup: Mutex::new(Some(Err("Opaque script host failure".into()))),
            }),
            host_failures: host_failures.clone(),
        };
        assert!(runtime.start(request(), 3).is_err());
        let failure = host_failures.check().unwrap_err();
        assert_eq!(failure.outcome, "ERROR");
        assert_eq!(failure.kind, "infrastructure");
        assert_eq!(failure.phase, "script-host");

        let mut initial = reply("<p>owned</p>");
        initial.errors.push("SyntaxError: owned fixture".into());
        let (runtime, _, _) = guarded(initial, None);
        let (mut session, reply) = runtime.start(request(), 3).unwrap();
        assert_eq!(reply.errors, ["SyntaxError: owned fixture"]);
        session.charge_active(Duration::ZERO).unwrap();
        assert!(runtime.host_failures.check().is_ok());
    }

    #[test]
    fn typed_initial_outcome_rejects_partial_pending_and_opaque_failure_without_status_text() {
        let clean = InitialScriptOutcome {
            status: InitialScriptStatus::Complete,
            failure: None,
            scripts_executed: 3,
            error_count: 0,
            broker_rejected_sources: 0,
            diagnostics: vec![],
        };
        assert!(check_initial_script(&clean).is_ok());
        for status in [InitialScriptStatus::Partial, InitialScriptStatus::Failed] {
            for (script_errors, broker_errors) in [(1, 0), (0, 1)] {
                let mut outcome = clean.clone();
                outcome.status = status;
                outcome.failure = Some(InitialScriptFailure::Script);
                outcome.error_count = script_errors;
                outcome.broker_rejected_sources = broker_errors;
                let failure = check_initial_script(&outcome).unwrap_err();
                assert_eq!(failure.outcome, "FAIL");
                assert_eq!(failure.kind, "admitted-execution");
            }
        }
        for status in [
            InitialScriptStatus::Failed,
            InitialScriptStatus::Pending,
            InitialScriptStatus::NotRequested,
        ] {
            let mut outcome = clean.clone();
            outcome.status = status;
            if status == InitialScriptStatus::Failed {
                outcome.failure = Some(InitialScriptFailure::Infrastructure);
            }
            let failure = check_initial_script(&outcome).unwrap_err();
            assert_eq!(failure.outcome, "ERROR");
            assert_eq!(failure.kind, "infrastructure");
        }
    }

    #[test]
    fn permits_only_explicit_port_loopback_http() {
        for url in [
            "http://127.0.0.1:8000/test.html",
            "http://127.0.0.1:80/test.html",
            "http://127.0.0.1:65535/",
        ] {
            assert!(local_url(url).is_ok(), "{url}");
        }
        for url in [
            "https://127.0.0.1:8000/test.html",
            "http://localhost:8000/test.html",
            "http://127.0.0.2:8000/test.html",
            "http://[::1]:8000/test.html",
            "http://127.0.0.1/test.html",
            "http://127.0.0.1:0/test.html",
            "http://127.0.0.1:65536/test.html",
            "http://user@127.0.0.1:8000/test.html",
            "http://user:password@127.0.0.1:8000/test.html",
            "http://127.0.0.1:8000/test.html?variant=1",
            "http://127.0.0.1:8000/test.html#fragment",
            "http://2130706433:8000/test.html",
            "file:///tmp/test.html",
        ] {
            assert!(local_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn artifact_suffix_does_not_replace_prefix_extension() {
        assert_eq!(
            artifact_path(Path::new("results/test.ref"), ".json"),
            PathBuf::from("results/test.ref.json")
        );
    }

    #[test]
    fn final_url_allows_default_port_normalization_but_not_origin_changes() {
        let requested = local_url("http://127.0.0.1:80/test.html").unwrap();
        assert!(check_final_url(&requested, "http://127.0.0.1/test.html").is_ok());
        for value in [
            "http://127.0.0.1:8000/test.html",
            "http://example.com/test.html",
            "http://user@127.0.0.1/test.html",
            "http://127.0.0.1/test.html?variant=1",
            "http://127.0.0.1/test.html#fragment",
        ] {
            assert!(check_final_url(&requested, value).is_err(), "{value}");
        }
    }

    #[test]
    fn rgb_conversion_preserves_channels_and_exact_length() {
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 0x12_34_56);
        canvas.pixels[1] = 0xfe_dc_ba;
        let rgb = rgb_bytes(&canvas).unwrap();
        assert_eq!(rgb.len(), WIDTH as usize * HEIGHT as usize * 3);
        assert_eq!(&rgb[..6], &[0x12, 0x34, 0x56, 0xfe, 0xdc, 0xba]);
    }

    #[test]
    fn rejects_wrong_surface_size_and_truncated_pixels() {
        assert!(rgb_bytes(&Canvas::new(WIDTH - 1, HEIGHT, 0)).is_err());
        let mut canvas = Canvas::new(WIDTH, HEIGHT, 0);
        canvas.pixels.pop();
        assert!(rgb_bytes(&canvas).is_err());
        assert!(rgb_bytes(&Canvas::new_scaled(WIDTH, HEIGHT, 0, 1.25)).is_err());
    }
}
