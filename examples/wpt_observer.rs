//! Non-scoring owned Window-harness worker controls, one frozen case per run.
//!
//! This test tool accepts an explicit packaged/public binary, not a production
//! runtime override. It runs no JS in the parent and proves no HTTP/native/CDP
//! behavior. Acceptance enumerates the complete selected fixed suite: old16 by
//! default, or the five separate output-configuration controls.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_observer/output_config.rs"]
mod output_config;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "wpt_observer/transport.rs"]
mod transport;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod linux {
    use super::output_config::{self, Suite};
    use super::transport::{self, Budget, Result, Worker};
    use mg_sparkle::{
        document::{self, Document},
        js_browser::{
            ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource, Request,
        },
        page_session::{
            self, AccountingSnapshot, DefaultAction, LifecyclePhase, LifecycleProgress, RealmState,
            SessionReply, TaskPump,
            observer::{
                HarnessStatus, NameReconciliation, ObservationView, ObserverContract,
                ObserverExpectation, ObserverInit, ObserverLedger, ObserverParentFailure,
                ObserverProgress, ObserverRecord, OptionalUtf16, SubtestStatus, TestRecord,
            },
        },
    };
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    const ROOT: &str = env!("CARGO_MANIFEST_DIR");
    const MANIFEST_SHA: &str = "0e82815d53f382904ae5786c5da82d1b700872b7eb73adcdc63767a0f9da7317";
    const HARNESS_SHA: &str = "d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e";
    const RECEIPT: usize = 128 * 1024;
    type AnyResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    fn ensure(value: bool, error: &str) -> Result<()> {
        if value { Ok(()) } else { Err(error.into()) }
    }
    fn hash(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
    fn body(path: &Path, length: usize, digest: &str) -> Result<Vec<u8>> {
        let file = transport::regular(path, length)?;
        let before = file.metadata().map_err(|e| e.to_string())?;
        ensure(before.len() == length as u64, "Input size differs")?;
        let mut bytes = Vec::new();
        file.take(length as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        ensure(
            bytes.len() == length && hash(&bytes) == digest,
            "Pinned input hash differs",
        )?;
        Ok(bytes)
    }
    fn binary_hash(path: &Path) -> Result<String> {
        let mut file = transport::regular(path, 64 * 1024 * 1024)?;
        let mut digest = Sha256::new();
        let mut buffer = [0; 16384];
        let mut bytes = 0usize;
        loop {
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count)
                .filter(|n| *n <= 64 * 1024 * 1024)
                .ok_or("Binary read bound")?;
            digest.update(&buffer[..count]);
        }
        Ok(digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
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

    #[derive(Serialize)]
    #[serde(deny_unknown_fields)]
    struct ModernInit {
        page: Request,
        observer: ObserverInit,
    }
    #[derive(Serialize)]
    #[serde(tag = "kind", content = "payload", deny_unknown_fields)]
    enum Command {
        Init(ModernInit),
        TaskPump(TaskPump),
    }
    #[derive(Serialize)]
    #[serde(deny_unknown_fields)]
    struct Frame {
        version: u8,
        generation: u64,
        session_id: u64,
        sequence: u64,
        expected_revision: u64,
        command: Command,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        version: u8,
        generation: u64,
        session_id: u64,
        sequence: u64,
        expected_revision: u64,
        reply: SessionReply,
    }
    struct Limited(Vec<u8>, usize);
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.1.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("Encoding bound"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
        let mut output = Limited(Vec::new(), limit);
        serde_json::to_writer(&mut output, value).map_err(|e| e.to_string())?;
        Ok(output.0)
    }
    fn create(path: &Path) -> Result<File> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())
    }

    fn metadata(
        frame: &Frame,
        response: &Reply,
        previous: Option<&AccountingSnapshot>,
        lifecycle: Option<LifecycleProgress>,
        last_wake: u64,
        expected_sources: usize,
    ) -> Result<()> {
        ensure(
            response.version == 7
                && response.generation == frame.generation
                && response.session_id == frame.session_id
                && response.sequence == frame.sequence
                && response.expected_revision == frame.expected_revision,
            "Reply identity/version differs",
        )?;
        let r = &response.reply;
        let initial = frame.sequence == 0;
        ensure(
            r.lifecycle.is_some() && r.observer.is_some(),
            "Required modern evidence missing",
        )?;
        ensure(
            r.revision
                == if initial {
                    0
                } else {
                    frame
                        .expected_revision
                        .checked_add(1)
                        .ok_or("Revision overflow")?
                },
            "Revision does not match completed control transaction",
        )?;
        ensure(
            r.errors.len() <= 64
                && r.errors.iter().all(|e| e.len() <= 4096)
                && r.scripts_executed == if initial { expected_sources } else { 0 },
            "Invalid metadata/startup source execution",
        )?;
        ensure(
            r.navigation.is_none()
                && r.default_action == DefaultAction::None
                && r.acknowledgements.is_empty()
                && r.outcome.click_canceled.is_none()
                && r.outcome.submit_canceled.is_none(),
            "Control worker returned navigation/activation effects",
        )?;
        ensure(
            r.accounting_valid() && previous.is_none_or(|old| r.accounting_valid_after(old)),
            "Invalid or nonmonotonic accounting",
        )?;
        let report = r.boa.as_ref().ok_or("Modern report missing")?;
        ensure(
            r.allocations.is_none()
                && report
                    .worker_memory
                    .is_some_and(|m| m.is_valid() && m.allocations > 0 && m.outstanding_bytes > 0)
                && report.source_admissions == expected_sources,
            "Wrong profile/process/source admission",
        )?;
        if initial {
            r.validate_lifecycle_initial()?;
        } else {
            r.validate_lifecycle_after(lifecycle, true)?;
        }
        if let Some(wake) = &r.task_wake {
            ensure(
                wake.token > last_wake && r.state == RealmState::Ready,
                "Wake token nonmonotonic/terminal",
            )?;
        }
        ensure(
            (report.pending_jobs > 0) == r.task_wake.is_some(),
            "Pending-work/wake mismatch",
        )?;
        Ok(())
    }
    fn project(reply: &mut SessionReply, url: &str, old: Option<&Document>) -> Result<Document> {
        let snapshot = reply.snapshot.take().ok_or("Missing projection")?;
        ensure(
            old.is_none_or(|old| {
                snapshot.nodes.len() >= old.nodes.len()
                    && old
                        .nodes
                        .iter()
                        .zip(&snapshot.nodes)
                        .all(|(a, b)| a.tag == b.tag)
            }),
            "Stable arena IDs changed",
        )?;
        let document = document::project_nodes(snapshot.nodes, url)?;
        ensure(
            document.refresh.is_none()
                && !document.nodes.iter().any(|n| {
                    n.tag == "meta"
                        && n.attr("http-equiv")
                            .is_some_and(|v| v.trim().eq_ignore_ascii_case("refresh"))
                }),
            "Projected refresh denied before any fetch",
        )?;
        Ok(document)
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
    fn completion_progress(
        case: &str,
        observer: &page_session::observer::ObserverReply,
    ) -> Result<()> {
        if matches!(
            case,
            "13-completion-then-rejection" | "15-queued-handled-callback-error"
        ) && let page_session::observer::ObserverReply::WindowHarness {
            records, progress, ..
        } = observer
            && records
                .iter()
                .any(|record| matches!(record, ObserverRecord::Completion { .. }))
        {
            // Frozen before execution: these controls have an actual pending
            // rejection/Handled obligation at their first Completion capture.
            ensure(
                *progress == ObserverProgress::Settling,
                "Frozen first Completion must be Settling",
            )?;
        }
        Ok(())
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
    struct Receipt<'a> {
        suite: &'static str,
        vendor_sha256: Option<&'static str>,
        source_metadata_scope: &'static str,
        source_plan: Option<output_config::SourcePlan>,
        observer_contract: ObserverContract,
        control_html_sha256: Option<&'a str>,
        control_body_sha256: Option<&'a str>,
        missing_completion: bool,
        native_execution_failed_before_missing: Option<bool>,
        scope: &'static str,
        case: &'a str,
        expected_negative: bool,
        expected_outcome: &'a str,
        expectation_met: bool,
        error: Option<String>,
        binary: &'a Path,
        binary_sha256: &'a str,
        manifest_sha256: &'static str,
        harness_sha256: &'static str,
        cleanup_confirmed: bool,
        pid: Option<u32>,
        transactions: u64,
        wire_bytes: usize,
        active_seconds_before_finalization: f64,
        whole_seconds_before_receipt: f64,
        cutoff: &'static str,
        progress: Option<ObserverProgress>,
        cursor: u32,
        execution_failed: bool,
        fault: Option<String>,
        parent_failure: Option<String>,
        diagnostics: usize,
        stderr_bytes: usize,
        copied_units: usize,
        retained_observer_capacity_bytes: usize,
        wpt_passes_added: u8,
        cleanup_scope: &'static str,
        reports: &'a [mg_butane::modern::Report],
        host_driver_source_sha256: String,
        host_transport_source_sha256: String,
    }

    pub fn run() -> AnyResult<()> {
        let started = Instant::now();
        let mut args = std::env::args().skip(1);
        let mut binary = None;
        let mut expected_sha = None;
        let mut output = None;
        let mut selected = None;
        let mut suite = None;
        while let Some(key) = args.next() {
            let value = args.next().ok_or("Each argument needs a value")?;
            match key.as_str() {
                "--binary" => binary = Some(PathBuf::from(value)),
                "--expected-sha" => expected_sha = Some(value),
                "--output" => output = Some(PathBuf::from(value)),
                "--case" => selected = Some(value),
                "--suite" => Suite::select(&mut suite, &value)?,
                _ => return Err("Unknown argument".into()),
            }
        }
        let binary = binary.ok_or("--binary is required")?;
        let suite = suite.unwrap_or_default();
        let expected_sha = expected_sha.ok_or("--expected-sha is required")?;
        ensure(
            expected_sha.len() == 64
                && expected_sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Explicit SHA256 required",
        )?;
        let output = output.ok_or("--output is required")?;
        let selected =
            selected.ok_or("--case is required; full acceptance separately enumerates all16")?;
        let root = Path::new(ROOT);
        ensure(
            output
                .parent()
                .ok_or("Output has no parent")?
                .canonicalize()?
                == root.join("tmp").canonicalize()?,
            "Output must be a new direct ignored tmp child",
        )?;
        fs::create_dir(&output)?;
        let _signals = transport::Signals::install()?;
        let mut budget = Budget::new(started);
        let manifest_bytes = body(
            &root.join("docs/WPT_OBSERVER_CONTROLS.json"),
            27229,
            MANIFEST_SHA,
        )?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
        ensure(
            manifest.case_count == 16 && manifest.cases.len() == 16,
            "Frozen full roster differs",
        )?;
        let output_manifest = if suite == Suite::OutputConfig {
            Some(output_config::Manifest::parse(&body(
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
                .find(|v| v.id == selected)
                .ok_or("Case not in frozen roster")?
        };
        let case_id = output_case.map_or(case.id.as_str(), |case| case.id.as_str());
        let run_id = output_case.map_or(case.run_id, |case| case.run_id);
        let case_html = output_case.map_or(case.html_utf8.as_str(), |case| case.html_utf8.as_str());
        let expected_diagnostics = output_case
            .map_or(case.expected.diagnostics_exact.as_ref(), |case| {
                Some(&case.expected.diagnostics_exact)
            });
        let expected_negative = output_case
            .map_or(case.expected.verifier_accepts_expected_negative, |case| {
                case.expected.verifier_expected_negative
            });
        let expected_outcome = output_case.map_or(case.expected.browser_outcome.as_str(), |case| {
            case.expected_outcome()
        });
        ensure(
            binary_hash(&binary)? == expected_sha,
            "Supplied binary SHA differs",
        )?;
        let harness = String::from_utf8(body(
            &root.join("tools/wpt/upstream/resources/testharness.js"),
            198291,
            HARNESS_SHA,
        )?)?;
        ensure(
            manifest.fixture_url_binding.stdio_source_error_visibility == "same-origin",
            "Wrong synthetic source metadata",
        )?;
        let url = if let Some(case) = output_case {
            format!("{}{}", output_config::STDIO_ORIGIN, case.url_path)
        } else {
            manifest.fixture_url_binding.stdio_document_origin.clone() + &case.url_path
        };
        let source_plan = output_case.map(|case| case.source_plan(&url)).transpose()?;
        let vendor = if output_case.is_some() {
            Some(String::from_utf8(body(
                &root.join("tools/wpt/upstream/resources/testharnessreport.js"),
                output_config::VENDOR_BYTES,
                output_config::VENDOR_SHA,
            )?)?)
        } else {
            None
        };
        let expected_sources = if output_case.is_some() { 3 } else { 2 };
        let expectation = ObserverExpectation::WindowHarness { run_id };
        let observer = ObserverInit::WindowHarness {
            contract: output_case.map_or(ObserverContract::PinnedWindowHarnessV1, |case| {
                case.observer_contract
            }),
            run_id,
            bootstrap_node: source_plan.map_or(1, |plan| plan.harness_node as u32),
        };
        let mut sources = vec![ExternalScriptSource {
            node: source_plan.map_or(1, |plan| plan.harness_node),
            source: harness,
            provenance: Some(ClassicSourceProvenance {
                response_url: if output_case.is_some() {
                    format!(
                        "{}{}",
                        output_config::STDIO_ORIGIN,
                        output_config::HARNESS_ROUTE
                    )
                } else {
                    manifest
                        .fixture_url_binding
                        .stdio_source_response_url
                        .clone()
                },
                error_visibility: ClassicErrorVisibility::SameOrigin,
            }),
        }];
        if let Some(vendor) = vendor {
            sources.push(ExternalScriptSource {
                node: source_plan.unwrap().vendor_node,
                source: vendor,
                provenance: Some(ClassicSourceProvenance {
                    response_url: format!(
                        "{}{}",
                        output_config::STDIO_ORIGIN,
                        output_config::VENDOR_ROUTE
                    ),
                    error_visibility: ClassicErrorVisibility::SameOrigin,
                }),
            });
        }
        let page = Request {
            url: url.clone(),
            html: case_html.into(),
            external_scripts: Some(sources),
        };
        if let Some(case) = output_case {
            let sources = page
                .external_scripts
                .as_ref()
                .ok_or("Constructed output sources missing")?;
            let inventory = output_config::SourceInventory {
                plan: source_plan.unwrap(),
                body: case.body_source_utf8.clone(),
                vendor: sources[1].source.clone(),
                vendor_url: format!(
                    "{}{}",
                    output_config::STDIO_ORIGIN,
                    output_config::VENDOR_ROUTE
                ),
            };
            inventory.verify(
                &page,
                &case.html_utf8,
                &url,
                &sources[0].source,
                &format!(
                    "{}{}",
                    output_config::STDIO_ORIGIN,
                    output_config::HARNESS_ROUTE
                ),
            )?;
        }
        let mut frame = Frame {
            version: 7,
            generation: 1,
            session_id: run_id,
            sequence: 0,
            expected_revision: 0,
            command: Command::Init(ModernInit { page, observer }),
        };
        let mut ledger = ObserverLedger::new(expectation)?;
        let mut worker = None;
        let mut lifecycle = None;
        let mut accounting = None;
        let mut document: Option<Document> = None;
        let mut state = RealmState::Closed;
        let mut last_wake = 0;
        let mut pending = 0;
        let mut diagnostics = 0;
        let mut diagnostic_match = true;
        let mut transactions = 0;
        let mut missing = false;
        let mut cutoff = "failure";
        let mut parent_failure = ObserverParentFailure::Validation;
        let mut reports = Vec::new();
        let mut initial_results = 0;
        let mut initial_completions = 0;
        let mut native_execution_failed_before_missing = None;
        let mut startup_cleanup_confirmed = true;
        let mut startup_pid = None;
        let outcome: Result<()> = (|| {
            budget.deadline()?;
            worker = Some(Worker::start(&binary, budget.whole).map_err(|failure| {
                startup_cleanup_confirmed = failure.cleanup_confirmed;
                startup_pid = failure.pid;
                failure.message
            })?);
            loop {
                let deadline = budget.deadline()?;
                ensure(
                    transactions < transport::TRANSACTIONS,
                    "64-transaction allowance exhausted",
                )?;
                let input = encode(
                    &frame,
                    if frame.sequence == 0 {
                        transport::INPUT
                    } else {
                        page_session::MAX_EVENT_BYTES
                    },
                )?;
                let sent_path = output.join(format!("{:02}-request.frame", frame.sequence));
                let received_path = output.join(format!("{:02}-reply.frame", frame.sequence));
                let mut sent = create(&sent_path)?;
                let mut received = create(&received_path)?;
                parent_failure = ObserverParentFailure::Transport;
                let raw = worker.as_mut().unwrap().exchange(
                    &input,
                    if frame.sequence == 0 {
                        transport::INPUT
                    } else {
                        page_session::MAX_EVENT_BYTES
                    },
                    deadline,
                    &mut sent,
                    &mut received,
                )?;
                parent_failure = ObserverParentFailure::Validation;
                let mut response: Reply =
                    serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
                metadata(
                    &frame,
                    &response,
                    accounting.as_ref(),
                    lifecycle,
                    last_wake,
                    expected_sources,
                )?;
                response
                    .reply
                    .validate_observer_frame(expectation)
                    .map_err(|e| e.to_string())?;
                completion_progress(
                    case_id,
                    response.reply.observer.as_ref().ok_or("Observer missing")?,
                )?;
                worker
                    .as_mut()
                    .unwrap()
                    .tail(response.reply.state != RealmState::Ready)?;
                parent_failure = ObserverParentFailure::Projection;
                let staged_document = project(&mut response.reply, &url, document.as_ref())?;
                let next_accounting = response.reply.accounting_snapshot();
                let next_lifecycle = response.reply.lifecycle;
                let report = response
                    .reply
                    .boa
                    .as_ref()
                    .ok_or("No actual worker report")?;
                pending = report.pending_jobs;
                reports.try_reserve_exact(1).map_err(|e| e.to_string())?;
                ensure(
                    reports.capacity() <= transport::TRANSACTIONS as usize,
                    "Report retention bound",
                )?;
                reports.push(report.clone()); // Bounded scalar accounting only, never DOM/JS history.
                for error in &response.reply.errors {
                    diagnostic_match &=
                        expected_diagnostics.and_then(|v| v.get(diagnostics)) == Some(error);
                    diagnostics += 1;
                }
                let wake = response.reply.task_wake.take();
                let initial_counts = if frame.sequence == 0 {
                    match response.reply.observer.as_ref() {
                        Some(page_session::observer::ObserverReply::WindowHarness {
                            records,
                            ..
                        }) => (
                            records
                                .iter()
                                .filter(|record| matches!(record, ObserverRecord::Result { .. }))
                                .count(),
                            records
                                .iter()
                                .filter(|record| {
                                    matches!(record, ObserverRecord::Completion { .. })
                                })
                                .count(),
                        ),
                        _ => (0, 0),
                    }
                } else {
                    (initial_results, initial_completions)
                };
                let next_state = response.reply.state;
                let next_revision = response.reply.revision;
                let next_progress = match response.reply.observer.as_ref() {
                    Some(page_session::observer::ObserverReply::WindowHarness {
                        progress, ..
                    }) => Some(*progress),
                    _ => None,
                };
                ensure(
                    worker.as_ref().unwrap().stderr.is_empty(),
                    "Unexpected worker stderr",
                )?;
                parent_failure = ObserverParentFailure::Validation;
                let staged = ledger
                    .stage(response.reply.observer.take().ok_or("Observer missing")?)
                    .map_err(|e| e.to_string())?;
                // No publish/state movement until the complete ONE active charge.
                parent_failure = ObserverParentFailure::ActiveCharge;
                budget.charge()?;
                staged.commit();
                (initial_results, initial_completions) = initial_counts;
                accounting = Some(next_accounting);
                lifecycle = next_lifecycle;
                document = Some(staged_document);
                state = next_state;
                transactions += 1;
                parent_failure = ObserverParentFailure::Validation;
                if next_progress == Some(ObserverProgress::Finished) {
                    cutoff = "Finished accepted checkpoint";
                    break;
                }
                ensure(
                    ledger.observation().fault.is_none() && state == RealmState::Ready,
                    "Observer/realm stopped before cutoff",
                )?;
                let due = wake
                    .as_ref()
                    .map(|w| Instant::now() + Duration::from_millis(u64::from(w.delay_ms)));
                // Ready/Waiting with no in-flight command and no wake is also
                // validated parent idle, just like the manager. It may expire
                // only as MissingCompletion, never a synthesized harness Timeout.
                loop {
                    parent_failure = ObserverParentFailure::Transport;
                    worker.as_mut().unwrap().idle()?;
                    parent_failure = ObserverParentFailure::ActiveCharge;
                    budget.account_nonadmitting()?;
                    if Instant::now() >= budget.cutoff {
                        missing = true;
                        cutoff = "MissingCompletion at23s";
                        break;
                    }
                    if due.is_some_and(|when| Instant::now() >= when) {
                        break;
                    }
                    budget.sleep_idle(Duration::from_millis(2))?;
                }
                if missing {
                    break;
                }
                let wake = wake.ok_or("No eligible wake")?;
                last_wake = wake.token;
                frame = Frame {
                    version: 7,
                    generation: 1,
                    session_id: run_id,
                    sequence: frame.sequence.checked_add(1).ok_or("Sequence overflow")?,
                    expected_revision: next_revision,
                    command: Command::TaskPump(TaskPump {
                        wake_token: wake.token,
                    }),
                };
            }
            parent_failure = ObserverParentFailure::Validation;
            if let Some(case) = output_case {
                if case.expected.missing_completion {
                    native_execution_failed_before_missing =
                        Some(ledger.observation().execution_failed);
                }
                return output_config::check(
                    case,
                    ledger.observation(),
                    output_config::Outcome {
                        lifecycle,
                        state,
                        diagnostics,
                        diagnostics_match: diagnostic_match,
                        missing,
                        pending,
                        initial_results,
                        initial_completions,
                        elapsed: started.elapsed(),
                    },
                );
            }
            check(
                case,
                ledger.observation(),
                lifecycle,
                state,
                diagnostics,
                diagnostic_match,
                missing,
                pending,
            )
        })();
        if outcome.is_err() {
            ledger.fail_parent(parent_failure);
        }
        // Consume all failed-exchange/verdict/bookkeeping time not already
        // charged. This is not a second completion acknowledgment/admission.
        let verdict_charge = budget.account_nonadmitting();
        if verdict_charge.is_err() {
            ledger.fail_parent(ObserverParentFailure::ActiveCharge);
        }
        if missing && outcome.is_ok() {
            ledger.fail_parent(ObserverParentFailure::Deadline);
        }
        let cleanup_started = Instant::now();
        let cleanup = worker.as_mut().map_or(Ok(()), Worker::stop);
        let cleanup_clock = budget.exclude_owned_cleanup(cleanup_started, Instant::now());
        if cleanup.is_err() {
            ledger.fail_parent(ObserverParentFailure::Cleanup);
        }
        if cleanup_clock.is_err() {
            ledger.fail_parent(ObserverParentFailure::ActiveCharge);
        }
        let stderr_capture = worker.as_mut().map_or(Ok(()), Worker::drain_stderr);
        let unchanged = binary_hash(&binary)
            .map(|v| v == expected_sha)
            .unwrap_or(false);
        let view = ledger.observation();
        let cleanup_confirmed = startup_cleanup_confirmed
            && worker
                .as_ref()
                .map_or(cleanup.is_ok(), |w| w.cleanup_confirmed);
        let error = outcome
            .err()
            .or_else(|| verdict_charge.err())
            .or_else(|| cleanup.err())
            .or_else(|| cleanup_clock.err())
            .or_else(|| stderr_capture.err())
            .or_else(|| (!unchanged).then(|| "Binary changed during control".into()))
            .or_else(|| {
                worker
                    .as_ref()
                    .is_some_and(|w| !w.stderr.is_empty())
                    .then(|| "Unexpected worker stderr at teardown".into())
            });
        let receipt = Receipt {
            suite: suite.as_str(),
            vendor_sha256: output_case.map(|_| output_config::VENDOR_SHA),
            source_metadata_scope: "Synthetic same-origin typed sources: no HTTP, MIME or resource-broker proof",
            source_plan,
            observer_contract: output_case
                .map_or(ObserverContract::PinnedWindowHarnessV1, |case| {
                    case.observer_contract
                }),
            control_html_sha256: output_case.map(|case| case.html_sha256()),
            control_body_sha256: output_case.map(|case| case.body_sha256()),
            missing_completion: missing,
            native_execution_failed_before_missing,
            scope: "Owned restricted-worker control, not HTTP/native/CDP or WPT score",
            case: case_id,
            expected_negative,
            expected_outcome,
            expectation_met: error.is_none(),
            error,
            binary: &binary,
            binary_sha256: &expected_sha,
            manifest_sha256: if output_case.is_some() {
                output_config::MANIFEST_SHA
            } else {
                MANIFEST_SHA
            },
            harness_sha256: HARNESS_SHA,
            cleanup_confirmed,
            pid: worker.as_ref().map(Worker::pid).or(startup_pid),
            transactions,
            wire_bytes: worker.as_ref().map_or(0, |w| w.wire),
            active_seconds_before_finalization: budget.used.as_secs_f64(),
            whole_seconds_before_receipt: budget.started.elapsed().as_secs_f64(),
            cutoff,
            progress: view.progress,
            cursor: view.cursor,
            execution_failed: view.execution_failed,
            fault: view.fault.map(|v| format!("{v:?}")),
            parent_failure: view.parent_failure.map(|v| format!("{v:?}")),
            diagnostics,
            stderr_bytes: worker.as_ref().map_or(0, |w| w.stderr.len()),
            copied_units: view.copied_units,
            retained_observer_capacity_bytes: view.retained_capacity_bytes,
            wpt_passes_added: 0,
            cleanup_scope: "Exact owned group termination and direct leader reap; not individual descendant reaping",
            reports: &reports,
            host_driver_source_sha256: hash(include_bytes!("wpt_observer.rs")),
            host_transport_source_sha256: hash(include_bytes!("wpt_observer/transport.rs")),
        };
        create(&output.join("stderr.bin"))?
            .write_all(worker.as_ref().map_or(&[], |w| w.stderr.as_slice()))?;
        create(&output.join("receipt.json"))?.write_all(&encode(&receipt, RECEIPT)?)?;
        // The receipt is a pre-final-encoding timing snapshot. The command must
        // ALSO exit successfully; finalization failure never admits its receipt.
        budget.account_nonadmitting()?;
        println!(
            "{} {} active_seconds_total={} whole_seconds={}",
            if receipt.expectation_met {
                "CONTROL_EXPECTATION_MET"
            } else {
                "CONTROL_FAILED"
            },
            case_id,
            budget.used.as_secs_f64(),
            budget.started.elapsed().as_secs_f64()
        );
        ensure(
            receipt.expectation_met && cleanup_confirmed,
            "Frozen control expectation or cleanup failed",
        )?;
        budget.final_guard()?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn frozen_manifest_reuses_all16_exact_sources_and_expectations() {
            let bytes = body(
                &Path::new(ROOT).join("docs/WPT_OBSERVER_CONTROLS.json"),
                27229,
                MANIFEST_SHA,
            )
            .unwrap();
            let manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(manifest.case_count, 16);
            assert_eq!(manifest.cases.len(), 16);
            assert_eq!(manifest.cases[0].id, "01-sync-pass");
            assert_eq!(manifest.cases[15].id, "16-out-of-order-results");
            assert_eq!(manifest.cases[7].expected.completion_count, 0);
            assert!(
                manifest
                    .cases
                    .iter()
                    .enumerate()
                    .all(|(i, c)| c.run_id == i as u64 + 1)
            );
            assert!(manifest.cases.iter().all(|c| {
                c.html_utf8
                    .starts_with("<script src=\"/resources/testharness.js\"></script>")
            }));
        }

        #[test]
        fn expected_null_is_not_missing_or_undefined() {
            #[derive(Deserialize)]
            struct Field {
                #[serde(default, deserialize_with = "present")]
                value: Option<Text>,
            }
            let absent: Field = serde_json::from_str("{}").unwrap();
            let null: Field = serde_json::from_str("{\"value\":null}").unwrap();
            assert!(absent.value.is_none());
            assert!(matches!(null.value, Some(Text::Null(()))));
            assert!(!matches_text(&OptionalUtf16::Undefined, &Text::Null(())));
            assert!(!matches_text(
                &OptionalUtf16::Null,
                &Text::Text(String::new())
            ));
        }

        #[test]
        fn strict_reply_wrapper_rejects_unknown_and_duplicate_identity_fields() {
            let original = r#"{"version":7,"generation":1,"session_id":1,"sequence":0,"expected_revision":0,"reply":{"revision":0,"snapshot":null,"outcome":{"click_canceled":null,"submit_canceled":null},"default_action":{"kind":"None"},"navigation":null,"errors":[],"scripts_executed":0,"allocations":null,"boa":null,"state":"Closed","acknowledgements":[],"lifecycle":{"phase":"Initializing","initial_failure":true},"observer":{"kind":"ObservationFailed","run_id":1,"prefix_end":0,"fault":"RejectedInit","execution_failed":true}}}"#;
            assert!(serde_json::from_str::<Reply>(original).is_ok());
            assert!(
                serde_json::from_str::<Reply>(&original.replacen(
                    "\"version\":7",
                    "\"version\":7,\"unknown\":0",
                    1
                ))
                .is_err()
            );
            assert!(
                serde_json::from_str::<Reply>(&original.replacen(
                    "\"version\":7",
                    "\"version\":7,\"version\":7",
                    1
                ))
                .is_err()
            );
            let response: Reply = serde_json::from_str(original).unwrap();
            let frame = Frame {
                version: 7,
                generation: 1,
                session_id: 1,
                sequence: 0,
                expected_revision: 0,
                command: Command::TaskPump(TaskPump { wake_token: 1 }),
            };
            assert!(
                metadata(&frame, &response, None, None, 0, 2).is_err(),
                "Reportless rejection is not a completed positive"
            );
        }

        #[test]
        fn bounded_encoding_has_no_unlimited_temporary_json_tree() {
            assert!(encode(&"abcd", 5).is_err());
            assert_eq!(encode(&"abc", 5).unwrap(), b"\"abc\"");
        }

        #[test]
        fn frozen_causal_controls_require_settling_on_first_completion_frame() {
            use page_session::observer::{HarnessRecord, ObserverReply};
            fn incoming(progress: ObserverProgress) -> ObserverReply {
                ObserverReply::WindowHarness {
                    run_id: 13,
                    first_ordinal: 0,
                    next_ordinal: 1,
                    records: vec![ObserverRecord::Completion {
                        harness: HarnessRecord {
                            status: HarnessStatus::Ok,
                            message: OptionalUtf16::Null,
                            stack: OptionalUtf16::Null,
                        },
                        inventory: Vec::new(),
                    }],
                    progress,
                    execution_failed: false,
                }
            }
            for case in [
                "13-completion-then-rejection",
                "15-queued-handled-callback-error",
            ] {
                assert!(completion_progress(case, &incoming(ObserverProgress::Finished)).is_err());
                assert!(completion_progress(case, &incoming(ObserverProgress::Waiting)).is_err());
                assert!(completion_progress(case, &incoming(ObserverProgress::Settling)).is_ok());
            }
            assert!(
                completion_progress("01-sync-pass", &incoming(ObserverProgress::Finished)).is_ok()
            );
        }
    }
}

fn main() {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if let Err(error) = linux::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        eprintln!("Owned observer controls require Linux x86_64");
        std::process::exit(1);
    }
}
