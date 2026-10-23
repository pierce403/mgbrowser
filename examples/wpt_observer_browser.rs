//! Non-scoring Browser acceptance: original16 by default, or fixed output-config5.
//! The immutable owned server is a prerequisite, not arbitrary-network isolation.
//! This composes the real Browser, resource broker, restricted worker and ledger.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_observer_browser/guard.rs"]
mod guard;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_observer/output_config.rs"]
mod output_config;

#[global_allocator]
static ALLOCATOR: mg_browser::platform::worker_memory::WorkerAllocator =
    mg_browser::platform::worker_memory::WorkerAllocator;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use super::guard::{self, HostState, Inventory, InventoryScripts};
    use super::output_config::{self, Suite};
    use mg_browser::platform;
    use mg_chassis::{
        Browser, ScriptWorkStatus,
        scripts::{ScriptRuntime, SourceWireMode},
    };
    use mg_sparkle::{
        document,
        page_session::{
            LifecyclePhase, LifecycleProgress, RealmState,
            observer::{
                HarnessStatus, NameReconciliation, ObservationView, ObserverContract, ObserverInit,
                ObserverProgress, ObserverRecord, OptionalUtf16, SubtestStatus, TestRecord,
            },
        },
        paint::Fonts,
    };
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    const ROOT: &str = env!("CARGO_MANIFEST_DIR");
    const MANIFEST_SHA: &str = "0e82815d53f382904ae5786c5da82d1b700872b7eb73adcdc63767a0f9da7317";
    const ARTIFACT_LIMIT: usize = 4 * 1024 * 1024;
    type Result<T> = std::result::Result<T, String>;
    type AnyResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    static CANCELED: AtomicBool = AtomicBool::new(false);

    fn ensure(value: bool, error: &str) -> Result<()> {
        if value { Ok(()) } else { Err(error.into()) }
    }
    fn hash(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
    fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        ensure(
            metadata.is_file() && metadata.len() <= limit as u64,
            "Input is not a bounded regular file",
        )?;
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        ensure(bytes.len() <= limit, "Input grew past bound")?;
        Ok(bytes)
    }
    fn pinned(path: &Path, size: usize, digest: &str) -> Result<Vec<u8>> {
        let bytes = read_regular(path, size)?;
        ensure(
            bytes.len() == size && hash(&bytes) == digest,
            "Pinned input identity differs",
        )?;
        Ok(bytes)
    }
    struct LimitedFile {
        file: File,
        written: usize,
    }
    impl Write for LimitedFile {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > ARTIFACT_LIMIT.saturating_sub(self.written) {
                return Err(std::io::Error::other("Artifact exceeds 4 MiB"));
            }
            let count = self.file.write(bytes)?;
            self.written += count;
            Ok(count)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.file.flush()
        }
    }
    fn artifact(path: &Path, value: &impl Serialize) -> Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut writer = LimitedFile { file, written: 0 };
        serde_json::to_writer_pretty(&mut writer, value).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    }

    extern "C" fn canceled(_: libc::c_int) {
        CANCELED.store(true, Ordering::Relaxed);
    }
    struct Signals([libc::sighandler_t; 2]);
    impl Signals {
        fn install() -> Result<Self> {
            let int = unsafe { libc::signal(libc::SIGINT, canceled as libc::sighandler_t) };
            if int == libc::SIG_ERR {
                return Err("Cannot install INT handler".into());
            }
            let term = unsafe { libc::signal(libc::SIGTERM, canceled as libc::sighandler_t) };
            if term == libc::SIG_ERR {
                unsafe {
                    libc::signal(libc::SIGINT, int);
                }
                return Err("Cannot install TERM handler".into());
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

    /// Supplemental stricter host wall accounting; never another manager allowance.
    struct Clock {
        started: Instant,
        cursor: Instant,
        used: Duration,
        cutoff: Instant,
        whole: Instant,
        owner_whole_ns: u64,
        admission_ns: u64,
        process_ns: u64,
    }
    impl Clock {
        fn new(started: Instant, deadline_ns: u64) -> Result<Self> {
            let before = Instant::now();
            let now_ns = monotonic_ns()?;
            let local_remaining =
                (started + Duration::from_secs(25)).saturating_duration_since(before);
            let owner_remaining = Duration::from_nanos(
                deadline_ns
                    .checked_sub(now_ns)
                    .ok_or("Owner deadline expired")?,
            );
            let remaining = local_remaining.min(owner_remaining);
            ensure(
                remaining > Duration::from_secs(3),
                "Insufficient shared owner deadline",
            )?;
            let owner_whole_ns = now_ns
                .checked_add(remaining.as_nanos().min(u64::MAX as u128) as u64)
                .ok_or("Owner deadline overflow")?;
            let admission_ns = owner_whole_ns
                .checked_sub(3_000_000_000)
                .ok_or("Admission deadline underflow")?;
            let process_ns = owner_whole_ns
                .checked_sub(2_100_000_000)
                .ok_or("Process deadline underflow")?;
            Ok(Self {
                started,
                cursor: started,
                used: Duration::ZERO,
                cutoff: before + remaining - Duration::from_secs(3),
                whole: before + remaining - Duration::from_millis(2100),
                owner_whole_ns,
                admission_ns,
                process_ns,
            })
        }
        fn account_at(&mut self, now: Instant) -> Result<()> {
            let elapsed = now
                .checked_duration_since(self.cursor)
                .ok_or("Host clock moved backwards")?;
            self.used = self.used.saturating_add(elapsed);
            self.cursor = now;
            ensure(
                !CANCELED.load(Ordering::Relaxed)
                    && now < self.whole
                    && self.used < Duration::from_secs(2),
                "Host cancellation/absolute/cumulative active guard failed",
            )
        }
        fn account(&mut self) -> Result<()> {
            self.account_at(Instant::now())
        }
        fn exclude(&mut self, start: Instant, end: Instant) -> Result<()> {
            ensure(end >= start, "Excluded interval moved backwards")?;
            let result = self.account_at(start);
            self.cursor = end;
            result?;
            ensure(
                !CANCELED.load(Ordering::Relaxed)
                    && end < self.whole
                    && self.used < Duration::from_secs(2),
                "Excluded interval exceeded whole/active boundary",
            )
        }
        fn sleep(&mut self, idle: bool) -> Result<()> {
            self.account()?;
            if idle {
                let start = Instant::now();
                std::thread::sleep(Duration::from_millis(2));
                self.exclude(start, Instant::now())
            } else {
                std::thread::sleep(Duration::from_millis(2));
                self.account()
            }
        }
        fn cleanup(&mut self, start: Instant, end: Instant) -> Result<()> {
            let result = self.exclude(start, end);
            ensure(
                end.duration_since(start) <= Duration::from_secs(2),
                "Cleanup exceeded its nonrenewed two seconds",
            )?;
            result
        }
    }
    fn monotonic_ns() -> Result<u64> {
        let mut value = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) } != 0
            || value.tv_sec < 0
            || !(0..1_000_000_000).contains(&value.tv_nsec)
        {
            return Err("CLOCK_MONOTONIC unavailable".into());
        }
        (value.tv_sec as u64)
            .checked_mul(1_000_000_000)
            .and_then(|ns| ns.checked_add(value.tv_nsec as u64))
            .ok_or("Monotonic timestamp overflow".into())
    }

    fn origin(value: &str) -> Result<url::Url> {
        let parsed = url::Url::parse(value).map_err(|_| "Invalid fixture origin")?;
        let authority = value
            .strip_prefix("http://")
            .and_then(|s| s.split('/').next())
            .ok_or("Expected HTTP loopback")?;
        let port = authority
            .strip_prefix("127.0.0.1:")
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|p| *p != 0)
            .ok_or("Explicit nonzero fixture port required")?;
        ensure(
            parsed.scheme() == "http"
                && parsed.host_str() == Some("127.0.0.1")
                && parsed.port_or_known_default() == Some(port)
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.path() == "/",
            "Expected exact credential-free loopback origin",
        )?;
        Ok(parsed)
    }

    #[derive(Deserialize)]
    struct Manifest {
        case_count: usize,
        cases: Vec<Case>,
        fixture_url_binding: Binding,
    }
    #[derive(Deserialize)]
    struct Binding {
        stdio_document_origin: String,
        stdio_source_response_url: String,
        stdio_source_error_visibility: String,
    }
    #[derive(Deserialize)]
    struct Case {
        id: String,
        run_id: u64,
        url_path: String,
        html_utf8: String,
        expected: Expected,
    }
    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum Text {
        Null(()),
        Text(String),
    }
    fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
        d: D,
    ) -> std::result::Result<Option<T>, D::Error> {
        T::deserialize(d).map(Some)
    }
    #[derive(Deserialize)]
    struct Expected {
        verifier_accepts_expected_negative: bool,
        browser_outcome: String,
        result_order_indices: Vec<u16>,
        completion_count: usize,
        #[serde(default, alias = "harness_status_at_native_callback")]
        harness_status: Option<HarnessStatus>,
        #[serde(
            default,
            alias = "harness_message_at_native_callback",
            deserialize_with = "present"
        )]
        harness_message: Option<Text>,
        #[serde(
            default,
            alias = "harness_stack_at_native_callback",
            deserialize_with = "present"
        )]
        harness_stack: Option<Text>,
        #[serde(default, alias = "harness_stack_kind_at_native_callback")]
        harness_stack_kind: Option<String>,
        #[serde(default)]
        inventory: Vec<ExpectedTest>,
        #[serde(
            default,
            alias = "native_execution_failed_after_settlement",
            alias = "native_execution_failed_before_parent_deadline"
        )]
        native_execution_failed: Option<bool>,
        #[serde(default, alias = "diagnostic_count_before_parent_deadline")]
        diagnostic_count: Option<usize>,
        #[serde(default)]
        diagnostics_exact: Option<Vec<String>>,
    }
    #[derive(Deserialize)]
    struct ExpectedTest {
        id: u16,
        index: u16,
        result_present: bool,
        status: SubtestStatus,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        result_name_units: Option<Vec<u16>>,
        #[serde(default)]
        final_name_units: Option<Vec<u16>>,
        #[serde(default)]
        name_reconciliation: Option<String>,
        #[serde(default, deserialize_with = "present")]
        message: Option<Text>,
        #[serde(default, deserialize_with = "present")]
        stack: Option<Text>,
        #[serde(default)]
        message_contains: Option<String>,
    }

    fn matches_text(actual: &OptionalUtf16, expected: &Text) -> bool {
        match (actual, expected) {
            (OptionalUtf16::Null, Text::Null(())) => true,
            (OptionalUtf16::Text { units }, Text::Text(text)) => {
                units.as_units().iter().copied().eq(text.encode_utf16())
            }
            _ => false,
        }
    }
    fn row(actual: &TestRecord, expected: &ExpectedTest, result: bool) -> Result<()> {
        ensure(
            actual.id == expected.id
                && actual.index == expected.index
                && actual.status == expected.status,
            "Expected row identity/status differs",
        )?;
        if let Some(name) = &expected.name {
            ensure(
                actual
                    .name
                    .as_units()
                    .iter()
                    .copied()
                    .eq(name.encode_utf16()),
                "Expected row name differs",
            )?;
        }
        if let Some(units) = if result {
            &expected.result_name_units
        } else {
            &expected.final_name_units
        } {
            ensure(
                actual.name.as_units() == units,
                "Exact exceptional UTF16 name differs",
            )?;
        }
        for (actual, expected) in [
            (&actual.message, &expected.message),
            (&actual.stack, &expected.stack),
        ] {
            if let Some(expected) = expected {
                ensure(
                    matches_text(actual, expected),
                    "Exact optional text differs",
                )?;
            }
        }
        if let Some(contains) = &expected.message_contains {
            let OptionalUtf16::Text { units } = &actual.message else {
                return Err("Expected real diagnostic text".into());
            };
            let needle: Vec<u16> = contains.encode_utf16().collect();
            ensure(
                !needle.is_empty() && units.as_units().windows(needle.len()).any(|v| v == needle),
                "Expected assertion message absent",
            )?;
        }
        Ok(())
    }
    fn check(
        case: &Case,
        view: ObservationView<'_>,
        lifecycle: Option<LifecycleProgress>,
        state: RealmState,
        diagnostics: usize,
        diagnostic_match: bool,
        missing: bool,
        pending: usize,
    ) -> Result<()> {
        let expected = &case.expected;
        ensure(
            view.fault.is_none() && view.parent_failure.is_none() && state == RealmState::Ready,
            "Observer/parent/realm failed, not an intended semantic negative",
        )?;
        ensure(
            view.execution_failed == expected.native_execution_failed.unwrap_or(false),
            "Native failure history differs",
        )?;
        ensure(
            diagnostic_match
                && diagnostics
                    == expected
                        .diagnostics_exact
                        .as_ref()
                        .map_or(expected.diagnostic_count.unwrap_or(0), Vec::len),
            "Diagnostic inventory differs",
        )?;
        let results: Vec<&TestRecord> = view
            .records
            .iter()
            .filter_map(|r| match r {
                ObserverRecord::Result { test } => Some(test),
                _ => None,
            })
            .collect();
        ensure(
            results
                .iter()
                .map(|r| r.index)
                .eq(expected.result_order_indices.iter().copied()),
            "Result arrival order differs",
        )?;
        let completions: Vec<_> = view
            .records
            .iter()
            .filter_map(|r| match r {
                ObserverRecord::Completion { harness, inventory } => Some((harness, inventory)),
                _ => None,
            })
            .collect();
        ensure(
            completions.len() == expected.completion_count,
            "Completion count differs",
        )?;
        if missing {
            return ensure(
                case.id == "08-missing-completion"
                    && view.progress == Some(ObserverProgress::Waiting)
                    && view.records.is_empty()
                    && lifecycle
                        == Some(LifecycleProgress {
                            phase: LifecyclePhase::LoadComplete,
                            initial_failure: false,
                        })
                    && pending == 0,
                "Wrong missing-completion prefix",
            );
        }
        ensure(
            view.progress == Some(ObserverProgress::Finished),
            "Completion was not causally settled",
        )?;
        let (harness, inventory) = completions.first().ok_or("Missing reconciled completion")?;
        ensure(
            Some(harness.status) == expected.harness_status,
            "Harness status differs",
        )?;
        if harness.status == HarnessStatus::Ok || !expected.verifier_accepts_expected_negative {
            ensure(
                lifecycle
                    == Some(LifecycleProgress {
                        phase: LifecyclePhase::LoadComplete,
                        initial_failure: false,
                    }),
                "Positive/ordinary Ok did not reach clean actual load",
            )?;
        }
        if let Some(text) = &expected.harness_message {
            ensure(
                matches_text(&harness.message, text),
                "Harness message differs",
            )?;
        }
        if let Some(text) = &expected.harness_stack {
            ensure(matches_text(&harness.stack, text), "Harness stack differs")?;
        }
        if let Some(kind) = &expected.harness_stack_kind {
            ensure(
                kind == "Undefined" && harness.stack == OptionalUtf16::Undefined,
                "Own undefined stack was normalized",
            )?;
        }
        ensure(
            inventory.len() == expected.inventory.len(),
            "Final inventory denominator differs",
        )?;
        for (actual, expected) in inventory.iter().zip(&expected.inventory) {
            row(actual, expected, false)?;
            let result = results.iter().find(|r| r.id == expected.id);
            ensure(
                result.is_some() == expected.result_present,
                "Final-only/result presence differs",
            )?;
            if let Some(result) = result {
                row(result, expected, true)?;
            }
            if let Some(label) = &expected.name_reconciliation {
                let kind = if label == "pinned sanitizer" {
                    NameReconciliation::PinnedSanitized
                } else if label == "unchanged" {
                    NameReconciliation::Unchanged
                } else {
                    return Err("Unknown frozen reconciliation label".into());
                };
                ensure(
                    view.reconciliations.iter().any(|r| {
                        r.id == expected.id && r.index == expected.index && r.kind == kind
                    }),
                    "Reconciliation proof differs",
                )?;
            }
        }
        if case.id == "09-default-file-timer-retained" {
            ensure(pending > 0, "Default file timer was incorrectly drained")?;
        }
        Ok(())
    }

    #[derive(Serialize)]
    struct PublishedTrace {
        cursor: u32,
        progress: Option<ObserverProgress>,
        lifecycle: Option<LifecycleProgress>,
        execution_failed: bool,
        charged_reply_count: usize,
    }
    #[derive(Serialize)]
    struct Reconciliation {
        id: u16,
        index: u16,
        kind: &'static str,
    }
    #[derive(Serialize)]
    struct Accepted<'a> {
        scope: &'static str,
        run_id: u64,
        cursor: u32,
        progress: Option<ObserverProgress>,
        execution_failed: bool,
        fault: Option<String>,
        parent_failure: Option<String>,
        copied_units: usize,
        retained_capacity_bytes: usize,
        records: &'a [ObserverRecord],
        reconciliations: Vec<Reconciliation>,
    }
    #[derive(Default)]
    struct Cutoff {
        cursor: u32,
        progress: Option<ObserverProgress>,
        execution_failed: bool,
        fault: Option<String>,
        parent_failure: Option<String>,
    }
    #[derive(Serialize)]
    struct Receipt<'a> {
        suite: &'static str,
        vendor_sha256: Option<&'static str>,
        source_plan: Option<output_config::SourcePlan>,
        source_metadata_scope: &'static str,
        observer_contract: ObserverContract,
        control_html_sha256: Option<&'a str>,
        control_body_sha256: Option<&'a str>,
        scope: &'static str,
        case: &'a str,
        expected_negative: bool,
        expected_outcome: &'a str,
        verifier_expectation_met: bool,
        error: Option<String>,
        cleanup_confirmed: bool,
        cleanup_wait_idle_ok: bool,
        whole_deadline_monotonic_ns: u64,
        admission_deadline_monotonic_ns: u64,
        process_deadline_monotonic_ns: u64,
        host_active_seconds: f64,
        elapsed_seconds: f64,
        missing_completion: bool,
        manifest_sha256: &'static str,
        harness_sha256: &'static str,
        font_sha256: &'a str,
        origin: &'a str,
        url: &'a str,
        wpt_passes_added: u8,
        accepted_file: &'static str,
        accepted_cursor: u32,
        accepted_progress: Option<ObserverProgress>,
        accepted_execution_failed: bool,
        accepted_fault: Option<String>,
        parent_failure: Option<String>,
        host_evidence: Option<guard::Snapshot>,
        published_trace: &'a [PublishedTrace],
        build_commit: &'static str,
        host_source_sha256: String,
        guard_source_sha256: String,
        cleanup_scope: &'static str,
        fixture_trust: &'static str,
    }
    fn preflight(case: &Case, url: &str) -> Result<()> {
        let document = document::parse_with_scripting(&case.html_utf8, url, true);
        ensure(
            document
                .nodes
                .iter()
                .filter(|node| node.tag == "script")
                .count()
                == 2,
            "Pinned control source roster differs",
        )?;
        ensure(
            !document.nodes.iter().any(|node| {
                matches!(
                    node.tag.as_str(),
                    "meta"
                        | "link"
                        | "img"
                        | "iframe"
                        | "object"
                        | "embed"
                        | "base"
                        | "source"
                        | "audio"
                        | "video"
                )
            }),
            "Pinned initial resources/navigation must be preflighted before fetching",
        )?;
        ensure(
            case.html_utf8
                .starts_with("<script src=\"/resources/testharness.js\"></script>"),
            "Pinned bootstrap must be first source",
        )?;
        Ok(())
    }
    fn write_accepted(browser: &Browser, run_id: u64, output: &Path) -> Result<Cutoff> {
        let ledger = browser
            .script_observation()
            .ok_or("Browser has no accepted observer ledger")?;
        let view = ledger.observation();
        let reconciliations = view
            .reconciliations
            .iter()
            .map(|row| Reconciliation {
                id: row.id,
                index: row.index,
                kind: match row.kind {
                    NameReconciliation::Unchanged => "Unchanged",
                    NameReconciliation::PinnedSanitized => "PinnedSanitized",
                    NameReconciliation::FinalOnly => "FinalOnly",
                },
            })
            .collect::<Vec<_>>();
        let fault = view.fault.map(|v| format!("{v:?}"));
        let parent_failure = view.parent_failure.map(|v| format!("{v:?}"));
        let value = Accepted {
            scope: "Only parent-validated, charged Browser observations; not page metadata",
            run_id,
            cursor: view.cursor,
            progress: view.progress,
            execution_failed: view.execution_failed,
            fault,
            parent_failure,
            copied_units: view.copied_units,
            retained_capacity_bytes: view.retained_capacity_bytes,
            records: view.records,
            reconciliations,
        };
        artifact(&output.join("accepted.json"), &value)?;
        Ok(Cutoff {
            cursor: view.cursor,
            progress: view.progress,
            execution_failed: view.execution_failed,
            fault: value.fault,
            parent_failure: value.parent_failure,
        })
    }
    fn require_first_completion(
        case: &Case,
        view: &ObservationView<'_>,
        observed: &mut bool,
    ) -> Result<()> {
        if !*observed
            && view
                .records
                .iter()
                .any(|record| matches!(record, ObserverRecord::Completion { .. }))
        {
            if matches!(
                case.id.as_str(),
                "13-completion-then-rejection" | "15-queued-handled-callback-error"
            ) {
                ensure(
                    view.progress == Some(ObserverProgress::Settling),
                    "First accepted Completion must be Settling",
                )?;
            }
            *observed = true;
        }
        Ok(())
    }
    pub fn run() -> AnyResult<()> {
        let started = Instant::now();
        let _signals = Signals::install()?;
        let mut args = std::env::args().skip(1);
        let (
            mut origin_arg,
            mut case_arg,
            mut font_path,
            mut font_sha,
            mut output,
            mut deadline_ns,
        ) = (None, None, None, None, None, None);
        let mut suite = None;
        while let Some(key) = args.next() {
            let value = args.next().ok_or("Each argument requires one value")?;
            match key.as_str() {
                "--origin" => {
                    ensure(origin_arg.is_none(), "Duplicate origin")?;
                    origin_arg = Some(value);
                }
                "--case" => {
                    ensure(case_arg.is_none(), "Duplicate case")?;
                    case_arg = Some(value);
                }
                "--suite" => Suite::select(&mut suite, &value)?,
                "--font" => {
                    ensure(font_path.is_none(), "Duplicate font")?;
                    font_path = Some(PathBuf::from(value));
                }
                "--font-sha" => {
                    ensure(font_sha.is_none(), "Duplicate font hash")?;
                    font_sha = Some(value);
                }
                "--output" => {
                    ensure(output.is_none(), "Duplicate output")?;
                    output = Some(PathBuf::from(value));
                }
                "--deadline-monotonic-ns" => {
                    ensure(deadline_ns.is_none(), "Duplicate deadline")?;
                    deadline_ns = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| "Invalid monotonic deadline")?,
                    );
                }
                _ => return Err("Unknown host argument".into()),
            }
        }
        let mut clock = Clock::new(
            started,
            deadline_ns.ok_or("--deadline-monotonic-ns is required")?,
        )?;
        let suite = suite.unwrap_or_default();
        let fixture = origin(&origin_arg.ok_or("--origin is required")?)?;
        let selected = case_arg.ok_or("--case is required")?;
        let font_path = font_path.ok_or("--font is required")?;
        let font_sha = font_sha.ok_or("--font-sha is required")?;
        ensure(
            font_sha.len() == 64
                && font_sha
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "Explicit lowercase font SHA256 required",
        )?;
        let output = output.ok_or("--output is required")?;
        let root = Path::new(ROOT);
        ensure(
            output
                .parent()
                .ok_or("Output has no parent")?
                .canonicalize()?
                == root.join("tmp").canonicalize()?,
            "Output must be a new direct ignored repository tmp directory",
        )?;
        fs::create_dir(&output)?;
        let manifest = pinned(
            &root.join("docs/WPT_OBSERVER_CONTROLS.json"),
            27229,
            MANIFEST_SHA,
        )?;
        let manifest: Manifest = serde_json::from_slice(&manifest)?;
        ensure(
            manifest.case_count == 16
                && manifest.cases.len() == 16
                && manifest
                    .cases
                    .iter()
                    .enumerate()
                    .all(|(index, case)| case.run_id == index as u64 + 1),
            "Frozen full control roster differs",
        )?;
        ensure(
            manifest.fixture_url_binding.stdio_document_origin == "https://observer.example.test"
                && manifest.fixture_url_binding.stdio_source_response_url
                    == "https://observer.example.test/resources/testharness.js"
                && manifest.fixture_url_binding.stdio_source_error_visibility == "same-origin",
            "Pinned synthetic source binding differs",
        )?;
        let output_manifest = if suite == Suite::OutputConfig {
            Some(output_config::Manifest::parse(&pinned(
                &root.join(output_config::MANIFEST_PATH),
                output_config::MANIFEST_BYTES,
                output_config::MANIFEST_SHA,
            )?)?)
        } else {
            None
        };
        let output_case = output_manifest
            .as_ref()
            .map(|manifest| manifest.case(&selected))
            .transpose()?;
        let case = if output_case.is_some() {
            &manifest.cases[0]
        } else {
            manifest
                .cases
                .iter()
                .find(|case| case.id == selected)
                .ok_or("Unknown frozen case")?
        };
        let case_id = output_case.map_or(case.id.as_str(), |case| case.id.as_str());
        let run_id = output_case.map_or(case.run_id, |case| case.run_id);
        let expected_negative = output_case
            .map_or(case.expected.verifier_accepts_expected_negative, |case| {
                case.expected.verifier_expected_negative
            });
        let expected_outcome = output_case.map_or(case.expected.browser_outcome.as_str(), |case| {
            case.expected_outcome()
        });
        let requested = fixture
            .join(output_case.map_or(case.url_path.as_str(), |case| case.url_path.as_str()))?;
        let requested_string = requested.to_string();
        let source_plan = if let Some(case) = output_case {
            Some(case.source_plan(&requested_string)?)
        } else {
            preflight(case, &requested_string)?;
            None
        };
        let expected_sources = if output_case.is_some() { 3 } else { 2 };
        let harness = String::from_utf8(pinned(
            &root.join("tools/wpt/upstream/resources/testharness.js"),
            198291,
            guard::HARNESS_SHA,
        )?)?;
        let font = read_regular(&font_path, 32 * 1024 * 1024)?;
        ensure(hash(&font) == font_sha, "Font identity differs")?;
        let fonts = Fonts::from_bytes(font)?;
        let output_inventory = if let Some(case) = output_case {
            Some(output_config::SourceInventory {
                plan: source_plan.unwrap(),
                body: case.body_source_utf8.clone(),
                vendor: String::from_utf8(pinned(
                    &root.join("tools/wpt/upstream/resources/testharnessreport.js"),
                    output_config::VENDOR_BYTES,
                    output_config::VENDOR_SHA,
                )?)?,
                vendor_url: fixture.join(output_config::VENDOR_ROUTE)?.to_string(),
            })
        } else {
            None
        };
        clock.account()?;
        let observer = ObserverInit::WindowHarness {
            contract: output_case.map_or(ObserverContract::PinnedWindowHarnessV1, |case| {
                case.observer_contract
            }),
            run_id,
            bootstrap_node: source_plan.map_or(1, |plan| plan.harness_node as u32),
        };
        let raw = Arc::new(platform::LinuxScripts::with_observer(observer));
        ensure(
            raw.source_wire_mode() == SourceWireMode::Modern,
            "Candidate host cannot select legacy mode",
        )?;
        let evidence = Arc::new(HostState::new(output_case.map_or_else(
            || case.expected.diagnostics_exact.clone().unwrap_or_default(),
            |case| case.expected.diagnostics_exact.clone(),
        )));
        let scripts: Arc<dyn ScriptRuntime> = Arc::new(InventoryScripts::new(
            raw,
            Inventory {
                url: requested_string.clone(),
                html: output_case
                    .map_or_else(|| case.html_utf8.clone(), |case| case.html_utf8.clone()),
                harness,
                harness_url: fixture.join("/resources/testharness.js")?.to_string(),
                output: output_inventory,
                observer,
                cutoff: clock.cutoff,
            },
            evidence.clone(),
        ));
        let mut browser = Browser::new(fonts, scripts.clone());
        browser.set_chrome(false);
        browser.set_scripts_enabled(true);
        browser.resize(800, 600);
        let mut published_trace = Vec::new();
        let mut seen_replies = 0;
        let mut completion_observed = false;
        let mut missing = false;
        let outcome: Result<()> = (|| {
            clock.account()?;
            browser.navigate(requested_string.clone(), None, false);
            loop {
                clock.account()?;
                if Instant::now() >= clock.cutoff {
                    missing = true;
                    break;
                }
                browser.poll();
                evidence.check()?;
                if let Some(ledger) = browser.script_observation() {
                    let view = ledger.observation();
                    ensure(
                        view.fault.is_none() && view.parent_failure.is_none(),
                        "Browser observation failed",
                    )?;
                    require_first_completion(case, &view, &mut completion_observed)?;
                    let summary = evidence.summary()?;
                    if view.progress.is_some() && summary.replies != seen_replies {
                        ensure(
                            summary.replies <= 64,
                            "Published transaction trace exceeds bound",
                        )?;
                        published_trace
                            .try_reserve_exact(1)
                            .map_err(|_| "Cannot reserve published trace")?;
                        ensure(
                            published_trace.capacity() <= 64,
                            "Published trace capacity exceeds bound",
                        )?;
                        published_trace.push(PublishedTrace {
                            cursor: view.cursor,
                            progress: view.progress,
                            lifecycle: browser.document_lifecycle(),
                            execution_failed: view.execution_failed,
                            charged_reply_count: summary.replies,
                        });
                        seen_replies = summary.replies;
                    }
                    if view.progress == Some(ObserverProgress::Finished) {
                        break;
                    }
                }
                ensure(
                    browser.initial_script_outcome().broker_rejected_sources == 0,
                    "Real source broker rejected an input",
                )?;
                ensure(
                    browser.page_url() == requested_string || browser.is_loading(),
                    "Browser changed final URL",
                )?;
                let idle = !browser.is_loading()
                    && matches!(
                        browser.script_work_status(),
                        ScriptWorkStatus::Idle | ScriptWorkStatus::Scheduled(_)
                    );
                clock.sleep(idle)?;
            }
            clock.account()?;
            let summary = evidence.summary()?;
            let last = summary.last.ok_or("No actual worker reply")?;
            ensure(
                summary.starts == 1 && last.manager_charge_succeeded,
                "Missing real startup or single charge",
            )?;
            let measured = evidence.snapshot()?;
            ensure(
                measured
                    .charged_trace
                    .iter()
                    .enumerate()
                    .all(|(index, row)| {
                        row.manager_charge_succeeded
                            && row.manager_charge_ns.is_some()
                            && row.scripts_executed == if index == 0 { expected_sources } else { 0 }
                            && row.source_admissions == Some(expected_sources)
                    }),
                "Actual source/acknowledgement inventory differs",
            )?;
            let ledger = browser
                .script_observation()
                .ok_or("No Browser observation")?;
            let view = ledger.observation();
            ensure(
                view.expectation == observer.expectation(),
                "Browser observer run changed",
            )?;
            ensure(
                browser.last_load_succeeded() && browser.page_url() == requested_string,
                "Actual Browser document load/URL differs",
            )?;
            if let Some(case) = output_case {
                ensure(
                    browser.initial_script_outcome().broker_rejected_sources == 0,
                    "Output source broker rejected an input",
                )?;
                let initial = measured
                    .charged_trace
                    .first()
                    .ok_or("No initial output transaction")?;
                output_config::check(
                    case,
                    view,
                    output_config::Outcome {
                        lifecycle: browser.document_lifecycle(),
                        state: last.state,
                        diagnostics: summary.diagnostics,
                        diagnostics_match: summary.diagnostics_match,
                        missing,
                        pending: last.pending_jobs.ok_or("No work report")?,
                        initial_results: initial.result_callbacks,
                        initial_completions: initial.completion_callbacks,
                        elapsed: started.elapsed(),
                    },
                )?;
            } else {
                check(
                    case,
                    view,
                    browser.document_lifecycle(),
                    last.state,
                    summary.diagnostics,
                    summary.diagnostics_match,
                    missing,
                    last.pending_jobs.ok_or("No work report")?,
                )?;
            }
            Ok(())
        })();
        let accepted = write_accepted(&browser, run_id, &output);
        let host_evidence = evidence.snapshot();
        let artifact_charge = clock.account();
        let cleanup_started = Instant::now();
        scripts.close();
        let cleanup = scripts.wait_idle();
        drop(browser);
        let cleanup_clock = clock.cleanup(cleanup_started, Instant::now());
        let cleanup_confirmed = cleanup.is_ok() && cleanup_clock.is_ok();
        let mut error = outcome
            .err()
            .or_else(|| accepted.as_ref().err().cloned())
            .or_else(|| host_evidence.as_ref().err().cloned())
            .or_else(|| artifact_charge.err())
            .or_else(|| cleanup.as_ref().err().cloned())
            .or_else(|| cleanup_clock.err());
        let cutoff = accepted.unwrap_or_default();
        if !cleanup_confirmed && error.is_none() {
            error = Some("Runtime cleanup unconfirmed".into());
        }
        let receipt = Receipt {
            suite: suite.as_str(),
            vendor_sha256: output_case.map(|_| output_config::VENDOR_SHA),
            source_plan,
            source_metadata_scope: "Real Browser source broker and accepted ledger; exact HTTP inventory/owned fixture audited separately",
            observer_contract: output_case
                .map_or(ObserverContract::PinnedWindowHarnessV1, |case| {
                    case.observer_contract
                }),
            control_html_sha256: output_case.map(|case| case.html_sha256()),
            control_body_sha256: output_case.map(|case| case.body_sha256()),
            scope: "Candidate real Browser + resource broker + manager acceptance; not native UI/CDP or WPT score",
            case: case_id,
            expected_negative,
            expected_outcome,
            verifier_expectation_met: error.is_none(),
            error,
            cleanup_confirmed,
            cleanup_wait_idle_ok: cleanup.is_ok(),
            whole_deadline_monotonic_ns: clock.owner_whole_ns,
            admission_deadline_monotonic_ns: clock.admission_ns,
            process_deadline_monotonic_ns: clock.process_ns,
            host_active_seconds: clock.used.as_secs_f64(),
            elapsed_seconds: clock.started.elapsed().as_secs_f64(),
            missing_completion: missing,
            manifest_sha256: if output_case.is_some() {
                output_config::MANIFEST_SHA
            } else {
                MANIFEST_SHA
            },
            harness_sha256: guard::HARNESS_SHA,
            font_sha256: &font_sha,
            origin: fixture.as_str(),
            url: &requested_string,
            wpt_passes_added: 0,
            accepted_file: "accepted.json",
            accepted_cursor: cutoff.cursor,
            accepted_progress: cutoff.progress,
            accepted_execution_failed: cutoff.execution_failed,
            accepted_fault: cutoff.fault,
            parent_failure: cutoff.parent_failure,
            host_evidence: host_evidence.ok(),
            published_trace: &published_trace,
            build_commit: env!("MGBROWSER_REVISION"),
            host_source_sha256: hash(include_bytes!("wpt_observer_browser.rs")),
            guard_source_sha256: hash(include_bytes!("wpt_observer_browser/guard.rs")),
            cleanup_scope: "Explicit closed LinuxScripts pool wait_idle; outer owned fixture/group audit is separate",
            fixture_trust: "Immutable preflighted route-only owned server; not arbitrary-server network confinement",
        };
        artifact(&output.join("receipt.json"), &receipt)?;
        clock.account()?;
        ensure(
            receipt.verifier_expectation_met && cleanup_confirmed,
            "Browser control expectation/cleanup failed",
        )?;
        println!(
            "BROWSER_CONTROL_EXPECTATION_MET {} active_seconds_total={} elapsed_seconds={}",
            case_id,
            clock.used.as_secs_f64(),
            clock.started.elapsed().as_secs_f64()
        );
        clock.account()?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use mg_sparkle::page_session::observer::{
            HarnessRecord, ObserverExpectation, ObserverLedger, ObserverReply, Utf16,
        };

        fn manifest() -> Manifest {
            let bytes = include_bytes!("../docs/WPT_OBSERVER_CONTROLS.json");
            assert_eq!(bytes.len(), 27229);
            assert_eq!(hash(bytes), MANIFEST_SHA);
            serde_json::from_slice(bytes).unwrap()
        }
        fn row(message_undefined: bool) -> TestRecord {
            TestRecord {
                id: 1,
                index: 0,
                name: Utf16::from_units("sync pass".encode_utf16().collect()).unwrap(),
                status: SubtestStatus::Pass,
                message: if message_undefined {
                    OptionalUtf16::Undefined
                } else {
                    OptionalUtf16::Null
                },
                stack: OptionalUtf16::Null,
            }
        }
        fn ledger(
            run_id: u64,
            progress: ObserverProgress,
            message_undefined: bool,
        ) -> ObserverLedger {
            let mut ledger =
                ObserverLedger::new(ObserverExpectation::WindowHarness { run_id }).unwrap();
            ledger
                .stage(ObserverReply::WindowHarness {
                    run_id,
                    first_ordinal: 0,
                    next_ordinal: 2,
                    records: vec![
                        ObserverRecord::Result {
                            test: row(message_undefined),
                        },
                        ObserverRecord::Completion {
                            harness: HarnessRecord {
                                status: HarnessStatus::Ok,
                                message: OptionalUtf16::Null,
                                stack: OptionalUtf16::Null,
                            },
                            inventory: vec![row(message_undefined)],
                        },
                    ],
                    progress,
                    execution_failed: false,
                })
                .unwrap()
                .commit();
            ledger
        }
        fn loaded() -> Option<LifecycleProgress> {
            Some(LifecycleProgress {
                phase: LifecyclePhase::LoadComplete,
                initial_failure: false,
            })
        }
        #[test]
        fn all16_source_identities_remain_frozen_and_are_preflighted() {
            let manifest = manifest();
            assert_eq!(manifest.case_count, 16);
            assert_eq!(manifest.cases.len(), 16);
            for (index, case) in manifest.cases.iter().enumerate() {
                assert_eq!(case.run_id, index as u64 + 1);
                preflight(case, &format!("http://127.0.0.1:7878{}", case.url_path)).unwrap();
            }
            assert_eq!(manifest.cases[7].id, "08-missing-completion");
            assert_eq!(manifest.cases[15].id, "16-out-of-order-results");
        }
        #[test]
        fn closed_matcher_rejects_wrong_status_inventory_name_and_optional_value() {
            let mut manifest = manifest();
            let case = &mut manifest.cases[0];
            let accepted = ledger(1, ObserverProgress::Finished, false);
            check(
                case,
                accepted.observation(),
                loaded(),
                RealmState::Ready,
                0,
                true,
                false,
                0,
            )
            .unwrap();
            case.expected.inventory[0].status = SubtestStatus::Fail;
            assert!(
                check(
                    case,
                    accepted.observation(),
                    loaded(),
                    RealmState::Ready,
                    0,
                    true,
                    false,
                    0
                )
                .is_err()
            );
            case.expected.inventory[0].status = SubtestStatus::Pass;
            case.expected.inventory[0].name = Some("wrong".into());
            assert!(
                check(
                    case,
                    accepted.observation(),
                    loaded(),
                    RealmState::Ready,
                    0,
                    true,
                    false,
                    0
                )
                .is_err()
            );
            case.expected.inventory[0].name = Some("sync pass".into());
            let undefined = ledger(1, ObserverProgress::Finished, true);
            assert!(
                check(
                    case,
                    undefined.observation(),
                    loaded(),
                    RealmState::Ready,
                    0,
                    true,
                    false,
                    0
                )
                .is_err()
            );
            case.expected.inventory.clear();
            assert!(
                check(
                    case,
                    accepted.observation(),
                    loaded(),
                    RealmState::Ready,
                    0,
                    true,
                    false,
                    0
                )
                .is_err()
            );
        }
        #[test]
        fn first_accepted_completion_requires_frozen_settling_for13_and15_only() {
            let manifest = manifest();
            for index in [12, 14] {
                let case = &manifest.cases[index];
                let finished = ledger(case.run_id, ObserverProgress::Finished, false);
                assert!(
                    require_first_completion(case, &finished.observation(), &mut false).is_err()
                );
                let settling = ledger(case.run_id, ObserverProgress::Settling, false);
                let mut observed = false;
                require_first_completion(case, &settling.observation(), &mut observed).unwrap();
                assert!(observed);
                require_first_completion(case, &finished.observation(), &mut observed).unwrap();
            }
            let ordinary = ledger(1, ObserverProgress::Finished, false);
            require_first_completion(&manifest.cases[0], &ordinary.observation(), &mut false)
                .unwrap();
        }
        #[test]
        fn positive_needs_actual_load_and_zero_diagnostics_not_merely_finished() {
            let manifest = manifest();
            let case = &manifest.cases[0];
            let accepted = ledger(1, ObserverProgress::Finished, false);
            assert!(
                check(
                    case,
                    accepted.observation(),
                    None,
                    RealmState::Ready,
                    0,
                    true,
                    false,
                    0
                )
                .is_err()
            );
            assert!(
                check(
                    case,
                    accepted.observation(),
                    loaded(),
                    RealmState::Ready,
                    1,
                    false,
                    false,
                    0
                )
                .is_err()
            );
            assert!(
                check(
                    case,
                    accepted.observation(),
                    loaded(),
                    RealmState::Closed,
                    0,
                    true,
                    false,
                    0
                )
                .is_err()
            );
        }
        #[test]
        fn monotonic_accounting_excludes_only_measured_idle_not_bookkeeping() {
            let base = Instant::now();
            let mut clock = Clock {
                started: base,
                cursor: base,
                used: Duration::ZERO,
                cutoff: base + Duration::from_secs(22),
                whole: base + Duration::from_millis(22900),
                owner_whole_ns: 25_000_000_000,
                admission_ns: 22_000_000_000,
                process_ns: 22_900_000_000,
            };
            clock.account_at(base + Duration::from_millis(10)).unwrap();
            clock
                .exclude(
                    base + Duration::from_millis(20),
                    base + Duration::from_millis(100),
                )
                .unwrap();
            clock.account_at(base + Duration::from_millis(110)).unwrap();
            assert_eq!(clock.used, Duration::from_millis(30));
            assert!(
                clock
                    .account_at(base + Duration::from_millis(2200))
                    .is_err()
            );
        }
        #[test]
        fn owner_deadline_only_narrows_and_keeps22_229_25_reservations() {
            let base = Instant::now();
            let owner = monotonic_ns().unwrap() + 25_000_000_000;
            let clock = Clock::new(base, owner).unwrap();
            assert!(clock.owner_whole_ns <= owner);
            assert_eq!(clock.admission_ns + 3_000_000_000, clock.owner_whole_ns);
            assert_eq!(clock.process_ns + 2_100_000_000, clock.owner_whole_ns);
            assert!(clock.whole <= base + Duration::from_millis(22900));
            assert!(Clock::new(base, 1).is_err());
        }
        #[test]
        fn fixture_origin_rejects_arbitrary_navigation_and_credentials() {
            assert!(origin("http://127.0.0.1:7878").is_ok());
            for value in [
                "https://127.0.0.1:7878",
                "http://localhost:7878",
                "http://127.0.0.1",
                "http://127.0.0.1:0",
                "http://user@127.0.0.1:7878",
                "http://127.0.0.1:7878/path",
                "http://127.0.0.1:7878/?x=1",
            ] {
                assert!(origin(value).is_err(), "{value}");
            }
        }
    }
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--script-worker") => mg_browser::platform::script_worker::worker_entry(),
        Some("--script-session") => mg_browser::platform::script_worker::session_entry(),
        _ => {}
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if let Err(error) = linux::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        eprintln!("Browser observer acceptance requires Linux x86_64");
        std::process::exit(1);
    }
}
