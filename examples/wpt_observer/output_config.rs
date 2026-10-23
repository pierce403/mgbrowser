//! Pure, fixed-roster output-configuration controls. No execution or transport.
use mg_sparkle::{
    document::{self, Node},
    js_browser::{
        ClassicErrorVisibility, Request, classic_script_type, unsupported_external_attribute,
    },
    page_session::{
        LifecyclePhase, LifecycleProgress, RealmState,
        observer::{
            HarnessStatus, ObservationView, ObserverContract, ObserverExpectation,
            ObserverProgress, ObserverRecord, OptionalUtf16, SubtestStatus, TestRecord,
        },
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub type Result<T> = std::result::Result<T, String>;
pub const MANIFEST_BYTES: usize = 15_871;
pub const MANIFEST_SHA: &str = "162a583745f1fb837b342b6ffa9f386c41d344312b4a74ab9a6e3d164c638f07";
pub const MANIFEST_PATH: &str = "docs/WPT_HARNESS_OUTPUT_CONTROLS.json";
pub const HARNESS_BYTES: usize = 198_291;
pub const HARNESS_SHA: &str = "d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e";
pub const VENDOR_BYTES: usize = 1_231;
pub const VENDOR_SHA: &str = "3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae";
pub const HARNESS_ROUTE: &str = "/resources/testharness.js";
pub const VENDOR_ROUTE: &str = "/resources/testharnessreport.js";
pub const STDIO_ORIGIN: &str = "https://observer.example.test";
const IDS: [&str; 5] = [
    "H1-configured-sync-pass",
    "H2-configured-assertion-fail",
    "H3-configured-async-timer-pass",
    "H4-configured-genuine-file-timeout",
    "H5-original-profile-default-output-failure",
];
fn ensure(value: bool, message: &str) -> Result<()> {
    if value { Ok(()) } else { Err(message.into()) }
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Suite {
    #[default]
    Observer,
    OutputConfig,
}
impl Suite {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "observer" => Ok(Self::Observer),
            "output-config" => Ok(Self::OutputConfig),
            _ => Err("Unknown fixed observer suite".into()),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observer => "observer",
            Self::OutputConfig => "output-config",
        }
    }
    pub fn select(slot: &mut Option<Self>, value: &str) -> Result<()> {
        ensure(slot.is_none(), "Duplicate suite selector")?;
        *slot = Some(Self::parse(value)?);
        Ok(())
    }
}
// Only this exact-size, exact-hash manifest is admitted. Its documentary metadata
// is not configuration; semantic case/constraint objects below are closed.
#[derive(Deserialize)]
struct RawManifest {
    case_count: usize,
    cases: Vec<Case>,
    fixture_url_binding: Binding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    stdio_document_origin: String,
    stdio_harness_response_url: String,
    stdio_vendor_response_url: String,
    stdio_source_error_visibility: String,
    metadata_is_synthetic: bool,
    metadata_is_http_transport_proof: bool,
    browser_host_binding: String,
}
pub struct Manifest {
    pub cases: Vec<Case>,
}
impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure(
            bytes.len() == MANIFEST_BYTES && hash(bytes) == MANIFEST_SHA,
            "Frozen output-control manifest differs",
        )?;
        let raw: RawManifest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        ensure(
            raw.case_count == 5 && raw.cases.len() == 5,
            "Output roster differs",
        )?;
        let binding = &raw.fixture_url_binding;
        ensure(
            binding.stdio_document_origin == STDIO_ORIGIN
                && binding.stdio_harness_response_url == format!("{STDIO_ORIGIN}{HARNESS_ROUTE}")
                && binding.stdio_vendor_response_url == format!("{STDIO_ORIGIN}{VENDOR_ROUTE}")
                && binding.stdio_source_error_visibility == "same-origin"
                && binding.metadata_is_synthetic
                && !binding.metadata_is_http_transport_proof
                && !binding.browser_host_binding.is_empty(),
            "Synthetic source binding differs",
        )?;
        for (index, case) in raw.cases.iter().enumerate() {
            ensure(
                case.id == IDS[index]
                    && case.run_id == 101 + index as u64
                    && case.url_path == format!("/owned-output-profile/h{}", index + 1),
                "Output case identity/order differs",
            )?;
            case.verify_bytes()?;
            case.source_plan(&format!("{STDIO_ORIGIN}{}", case.url_path))?;
            ensure(
                case.observer_contract
                    == if index == 4 {
                        ObserverContract::PinnedWindowHarnessV1
                    } else {
                        ObserverContract::PinnedWindowHarnessOutputDisabledV1
                    },
                "Output suite contract differs",
            )?;
            case.expected.validate()?;
        }
        ensure(
            raw.cases[0].html_utf8 == raw.cases[4].html_utf8
                && raw.cases[0].body_source_utf8 == raw.cases[4].body_source_utf8,
            "Configured/original sibling inputs differ",
        )?;
        Ok(Self { cases: raw.cases })
    }
    pub fn case(&self, id: &str) -> Result<&Case> {
        self.cases
            .iter()
            .find(|case| case.id == id)
            .ok_or_else(|| "Case outside output-config suite".into())
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub run_id: u64,
    pub url_path: String,
    pub body_source_utf8: String,
    pub html_utf8: String,
    body_bytes: usize,
    body_sha256: String,
    html_bytes: usize,
    html_sha256: String,
    pub expected: Expected,
    pub observer_contract: ObserverContract,
}
impl Case {
    fn verify_bytes(&self) -> Result<()> {
        ensure(
            self.body_source_utf8.len() == self.body_bytes
                && hash(self.body_source_utf8.as_bytes()) == self.body_sha256
                && self.html_utf8.len() == self.html_bytes
                && hash(self.html_utf8.as_bytes()) == self.html_sha256,
            "Output HTML/body identity differs",
        )
    }
    pub fn source_plan(&self, url: &str) -> Result<SourcePlan> {
        self.verify_bytes()?;
        derive_plan(&self.html_utf8, &self.body_source_utf8, url)
    }
    pub fn html_sha256(&self) -> &str {
        &self.html_sha256
    }
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }
    pub fn expected_outcome(&self) -> &'static str {
        if self.expected.missing_completion {
            "MissingCompletion with retained native failure"
        } else if self.expected.verifier_expected_negative {
            "Declared genuine negative, not WPT PASS"
        } else {
            "Finished authored control, not WPT PASS"
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextKind {
    Null,
    Undefined,
    Text,
}
impl<'de> Deserialize<'de> for TextKind {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = TextKind;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a closed optional-text kind string")
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> std::result::Result<TextKind, E> {
                match value {
                    "Null" => Ok(TextKind::Null),
                    "Undefined" => Ok(TextKind::Undefined),
                    "Text" => Ok(TextKind::Text),
                    _ => Err(E::custom("Unknown text-kind constraint")),
                }
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedRow {
    id: u16,
    index: u16,
    name: String,
    status: SubtestStatus,
    #[serde(default)]
    message_kind: Option<TextKind>,
    #[serde(default)]
    message_exact: Option<String>,
    stack_kind: TextKind,
    #[serde(default)]
    stack_text_unmeasured: bool,
    #[serde(default)]
    final_only: Option<bool>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expected {
    accepted_progress: ObserverProgress,
    accepted_lifecycle: LifecyclePhase,
    initial_failure: bool,
    realm: RealmState,
    observer_fault: Option<String>,
    native_execution_failed: bool,
    pub diagnostics_exact: Vec<String>,
    result_callbacks: usize,
    completion_callbacks: usize,
    #[serde(default)]
    harness_status: Option<HarnessStatus>,
    #[serde(default)]
    harness_message_kind: Option<TextKind>,
    #[serde(default)]
    harness_stack_kind: Option<TextKind>,
    #[serde(default)]
    inventory: Vec<ExpectedRow>,
    #[serde(default)]
    captured_results: Vec<ExpectedRow>,
    #[serde(default)]
    healthy_prefix_end: Option<u32>,
    #[serde(default)]
    typed_harness_status: Option<String>,
    pub missing_completion: bool,
    #[serde(default)]
    normal_file_timer_still_pending_at_finished: Option<bool>,
    #[serde(default)]
    normal_file_timer_pending_after_delivery: Option<bool>,
    #[serde(default)]
    normal_file_timer_pending_after_actual_timer_delivery: Option<bool>,
    #[serde(default)]
    assertion_uses_genuine_later_function_timer: Option<bool>,
    #[serde(default)]
    genuine_timer_duration_ms: Option<u32>,
    #[serde(default)]
    cutoff_policy: Option<String>,
    pub verifier_expected_negative: bool,
}
impl Expected {
    fn validate(&self) -> Result<()> {
        ensure(
            self.observer_fault.is_none()
                && self.realm == RealmState::Ready
                && self.accepted_lifecycle == LifecyclePhase::LoadComplete
                && !self.initial_failure,
            "Unknown output observer/lifecycle constraint",
        )?;
        for row in self.inventory.iter().chain(&self.captured_results) {
            ensure(
                row.message_kind.is_some() != row.message_exact.is_some()
                    && row.stack_text_unmeasured == (row.stack_kind == TextKind::Text),
                "Unknown or ambiguous output text constraint",
            )?;
        }
        if self.missing_completion {
            ensure(
                self.accepted_progress == ObserverProgress::Waiting
                    && self.completion_callbacks == 0
                    && self.result_callbacks == 1
                    && self.harness_status.is_none()
                    && self.harness_message_kind.is_none()
                    && self.harness_stack_kind.is_none()
                    && self.inventory.is_empty()
                    && self.captured_results.len() == 1
                    && self.healthy_prefix_end == Some(1)
                    && self.typed_harness_status.as_deref()
                        == Some("ABSENT_NO_COMPLETION_MUST_NOT_SYNTHESIZE")
                    && self.cutoff_policy.as_deref()
                        == Some(
                            "Continue ordinary eligible pumps/validated idle until the unchanged owner admission deadline, preserving already known native failure; no fabricated harness Timeout",
                        )
                    && self.normal_file_timer_pending_after_actual_timer_delivery == Some(false),
                "Malformed missing-completion constraint",
            )?;
        } else {
            ensure(
                self.accepted_progress == ObserverProgress::Finished
                    && self.completion_callbacks == 1
                    && self.inventory.len() == 1
                    && self.captured_results.is_empty()
                    && self.harness_status.is_some()
                    && self.harness_message_kind == Some(TextKind::Null)
                    && self.harness_stack_kind == Some(TextKind::Null),
                "Malformed completed-control constraint",
            )?;
        }
        ensure(
            self.genuine_timer_duration_ms.is_none_or(|ms| ms == 10_000)
                && self
                    .assertion_uses_genuine_later_function_timer
                    .is_none_or(|value| value),
            "Unknown timer constraint",
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SourcePlan {
    pub harness_node: usize,
    pub vendor_node: usize,
    pub inline_node: usize,
}
fn active(nodes: &[Node], mut index: usize) -> bool {
    for _ in 0..=256 {
        let Some(node) = nodes.get(index) else {
            return false;
        };
        if matches!(node.tag.as_str(), "template" | "noscript") {
            return false;
        }
        if index == 0 {
            return true;
        }
        index = node.parent;
    }
    false
}
fn derive_plan(html: &str, body: &str, url: &str) -> Result<SourcePlan> {
    let page = document::parse_with_scripting(html, url, true);
    ensure(page.refresh.is_none(), "Initial refresh denied")?;
    let mut scripts = [usize::MAX; 3];
    let mut count = 0;
    let mut charset = 0;
    for (index, node) in page.nodes.iter().enumerate() {
        ensure(
            !matches!(
                node.tag.as_str(),
                "base"
                    | "link"
                    | "img"
                    | "iframe"
                    | "frame"
                    | "object"
                    | "embed"
                    | "source"
                    | "audio"
                    | "video"
                    | "style"
            ),
            "Unexpected initial resource",
        )?;
        if node.tag == "meta" {
            ensure(
                node.attributes.as_slice() == [(String::from("charset"), String::from("utf-8"))],
                "Only the pinned benign charset meta is allowed",
            )?;
            charset += 1;
        }
        if node.tag == "script" {
            ensure(
                count < 3 && active(&page.nodes, index),
                "Extra or inert output source",
            )?;
            scripts[count] = index;
            count += 1;
        }
    }
    ensure(
        count == 3 && charset == 1,
        "Three sources and one charset meta required",
    )?;
    for (index, route) in [(scripts[0], HARNESS_ROUTE), (scripts[1], VENDOR_ROUTE)] {
        let node = &page.nodes[index];
        ensure(
            node.attributes.as_slice() == [(String::from("src"), String::from(route))]
                && classic_script_type(node.attr("type").unwrap_or(""))
                && unsupported_external_attribute(node).is_none()
                && !node.has("defer"),
            "Wrong external source/order/attributes",
        )?;
    }
    let inline = &page.nodes[scripts[2]];
    ensure(
        inline.attributes.is_empty() && inline.children.len() == 1,
        "Wrong inline source shape",
    )?;
    let text = page
        .nodes
        .get(inline.children[0])
        .ok_or("Inline text missing")?;
    ensure(
        text.tag == "#text"
            && text.text.as_bytes().first() == Some(&b'\n')
            && text.text.as_bytes().get(1..) == Some(body.as_bytes()),
        "Exact framing LF/body differs",
    )?;
    Ok(SourcePlan {
        harness_node: scripts[0],
        vendor_node: scripts[1],
        inline_node: scripts[2],
    })
}
pub struct SourceInventory {
    pub plan: SourcePlan,
    pub body: String,
    pub vendor: String,
    pub vendor_url: String,
}
impl SourceInventory {
    pub fn verify(
        &self,
        request: &Request,
        html: &str,
        page_url: &str,
        harness: &str,
        harness_url: &str,
    ) -> Result<()> {
        ensure(
            request.url == page_url && request.html == html,
            "Actual output document differs",
        )?;
        ensure(
            derive_plan(&request.html, &self.body, &request.url)? == self.plan,
            "Actual output source indices differ",
        )?;
        let sources = request
            .external_scripts
            .as_ref()
            .ok_or("Missing typed output sources")?;
        ensure(
            sources.len() == 2,
            "Exactly two external output sources required",
        )?;
        for (source, node, expected, url, size, digest) in [
            (
                &sources[0],
                self.plan.harness_node,
                harness,
                harness_url,
                HARNESS_BYTES,
                HARNESS_SHA,
            ),
            (
                &sources[1],
                self.plan.vendor_node,
                self.vendor.as_str(),
                self.vendor_url.as_str(),
                VENDOR_BYTES,
                VENDOR_SHA,
            ),
        ] {
            ensure(
                source.node == node
                    && source.source.len() == size
                    && source.source == expected
                    && hash(source.source.as_bytes()) == digest,
                "Actual complete external output source differs",
            )?;
            let provenance = source
                .provenance
                .as_ref()
                .ok_or("Missing output provenance")?;
            let page = url::Url::parse(page_url).map_err(|e| e.to_string())?;
            let response = url::Url::parse(&provenance.response_url).map_err(|e| e.to_string())?;
            ensure(
                provenance.error_visibility == ClassicErrorVisibility::SameOrigin
                    && provenance.response_url == url
                    && page.origin() == response.origin()
                    && response.username().is_empty()
                    && response.password().is_none()
                    && response.query().is_none()
                    && response.fragment().is_none(),
                "Actual output response provenance differs",
            )?;
        }
        Ok(())
    }
}
fn text_matches(actual: &OptionalUtf16, kind: TextKind, exact: Option<&str>) -> bool {
    match (actual, kind) {
        (OptionalUtf16::Null, TextKind::Null) | (OptionalUtf16::Undefined, TextKind::Undefined) => {
            exact.is_none()
        }
        (OptionalUtf16::Text { units }, TextKind::Text) => {
            exact.is_none_or(|text| units.as_units().iter().copied().eq(text.encode_utf16()))
        }
        _ => false,
    }
}
fn row_matches(actual: &TestRecord, expected: &ExpectedRow) -> Result<()> {
    ensure(
        actual.id == expected.id
            && actual.index == expected.index
            && actual.status == expected.status
            && actual
                .name
                .as_units()
                .iter()
                .copied()
                .eq(expected.name.encode_utf16()),
        "Output row identity/status/name differs",
    )?;
    ensure(
        text_matches(
            &actual.message,
            expected.message_kind.unwrap_or(TextKind::Text),
            expected.message_exact.as_deref(),
        ) && text_matches(&actual.stack, expected.stack_kind, None),
        "Output row optional text differs",
    )
}
pub struct Outcome {
    pub lifecycle: Option<LifecycleProgress>,
    pub state: RealmState,
    pub diagnostics: usize,
    pub diagnostics_match: bool,
    pub missing: bool,
    pub pending: usize,
    pub initial_results: usize,
    pub initial_completions: usize,
    pub elapsed: Duration,
}
pub fn check(case: &Case, view: ObservationView<'_>, outcome: Outcome) -> Result<()> {
    let expected = &case.expected;
    expected.validate()?;
    ensure(
        view.expectation
            == (ObserverExpectation::WindowHarness {
                run_id: case.run_id,
            })
            && view.fault.is_none()
            && view.parent_failure.is_none()
            && outcome.state == expected.realm,
        "Output observer/parent/realm failed",
    )?;
    ensure(
        view.execution_failed == expected.native_execution_failed
            && outcome.lifecycle
                == Some(LifecycleProgress {
                    phase: expected.accepted_lifecycle,
                    initial_failure: expected.initial_failure,
                })
            && view.progress == Some(expected.accepted_progress)
            && outcome.missing == expected.missing_completion
            && outcome.diagnostics_match
            && outcome.diagnostics == expected.diagnostics_exact.len(),
        "Output accepted state/history differs",
    )?;
    let results: Vec<_> = view
        .records
        .iter()
        .filter_map(|record| match record {
            ObserverRecord::Result { test } => Some(test),
            _ => None,
        })
        .collect();
    let completions: Vec<_> = view
        .records
        .iter()
        .filter_map(|record| match record {
            ObserverRecord::Completion { harness, inventory } => Some((harness, inventory)),
            _ => None,
        })
        .collect();
    ensure(
        results.len() == expected.result_callbacks
            && completions.len() == expected.completion_callbacks,
        "Output callback denominator differs",
    )?;
    if expected.missing_completion {
        ensure(
            view.cursor == expected.healthy_prefix_end.unwrap()
                && completions.is_empty()
                && results.len() == expected.captured_results.len(),
            "Output missing-completion prefix differs",
        )?;
        for (actual, wanted) in results.iter().zip(&expected.captured_results) {
            row_matches(actual, wanted)?;
        }
    } else {
        let (harness, inventory) = completions[0];
        ensure(
            Some(harness.status) == expected.harness_status
                && text_matches(
                    &harness.message,
                    expected.harness_message_kind.unwrap(),
                    None,
                )
                && text_matches(&harness.stack, expected.harness_stack_kind.unwrap(), None)
                && inventory.len() == expected.inventory.len(),
            "Output harness/final inventory differs",
        )?;
        let mut result_index = 0;
        for (actual, wanted) in inventory.iter().zip(&expected.inventory) {
            row_matches(actual, wanted)?;
            let result = results.iter().find(|row| row.id == wanted.id);
            ensure(
                result.is_some() == !wanted.final_only.unwrap_or(false),
                "Output final-only/result differs",
            )?;
            if let Some(result) = result {
                row_matches(result, wanted)?;
                ensure(
                    results[result_index].id == wanted.id,
                    "Output Result order differs",
                )?;
                result_index += 1;
            }
        }
    }
    if let Some(pending) = expected.normal_file_timer_still_pending_at_finished {
        ensure(
            pending && outcome.pending > 0,
            "Normal file timer was incorrectly drained",
        )?;
    }
    if expected.normal_file_timer_pending_after_delivery == Some(false)
        || expected.normal_file_timer_pending_after_actual_timer_delivery == Some(false)
    {
        ensure(
            outcome.pending == 0,
            "Completed file timer left unexpected work",
        )?;
    }
    if expected.assertion_uses_genuine_later_function_timer == Some(true) {
        ensure(
            outcome.initial_results == 0 && !results.is_empty(),
            "Async timer Result occurred in Init",
        )?;
    }
    if let Some(ms) = expected.genuine_timer_duration_ms {
        ensure(
            outcome.initial_completions == 0
                && outcome.elapsed >= Duration::from_millis(u64::from(ms)),
            "File Timeout did not come from the genuine original timer",
        )?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use mg_sparkle::{
        js_browser::{ClassicSourceProvenance, ExternalScriptSource},
        page_session::observer::{
            HarnessRecord, ObserverExpectation, ObserverLedger, ObserverReply, Utf16,
        },
    };
    fn manifest() -> Manifest {
        Manifest::parse(include_bytes!(
            "../../docs/WPT_HARNESS_OUTPUT_CONTROLS.json"
        ))
        .unwrap()
    }
    fn record(case: &Case, status: SubtestStatus, stack: OptionalUtf16) -> TestRecord {
        let row = &case.expected.inventory[0];
        TestRecord {
            id: row.id,
            index: row.index,
            name: Utf16::from_units(row.name.encode_utf16().collect()).unwrap(),
            status,
            message: if let Some(text) = &row.message_exact {
                OptionalUtf16::Text {
                    units: Utf16::from_units(text.encode_utf16().collect()).unwrap(),
                }
            } else {
                OptionalUtf16::Null
            },
            stack,
        }
    }
    fn duplicate(row: &TestRecord) -> TestRecord {
        fn text(value: &OptionalUtf16) -> OptionalUtf16 {
            match value {
                OptionalUtf16::Null => OptionalUtf16::Null,
                OptionalUtf16::Undefined => OptionalUtf16::Undefined,
                OptionalUtf16::Text { units } => OptionalUtf16::Text {
                    units: Utf16::from_units(units.as_units().to_vec()).unwrap(),
                },
            }
        }
        TestRecord {
            id: row.id,
            index: row.index,
            status: row.status,
            name: Utf16::from_units(row.name.as_units().to_vec()).unwrap(),
            message: text(&row.message),
            stack: text(&row.stack),
        }
    }
    fn ledger(case: &Case, row: TestRecord) -> ObserverLedger {
        let mut ledger = ObserverLedger::new(ObserverExpectation::WindowHarness {
            run_id: case.run_id,
        })
        .unwrap();
        ledger
            .stage(ObserverReply::WindowHarness {
                run_id: case.run_id,
                first_ordinal: 0,
                next_ordinal: 2,
                records: vec![
                    ObserverRecord::Result {
                        test: duplicate(&row),
                    },
                    ObserverRecord::Completion {
                        harness: HarnessRecord {
                            status: HarnessStatus::Ok,
                            message: OptionalUtf16::Null,
                            stack: OptionalUtf16::Null,
                        },
                        inventory: vec![row],
                    },
                ],
                progress: ObserverProgress::Finished,
                execution_failed: false,
            })
            .unwrap()
            .commit();
        ledger
    }
    fn outcome() -> Outcome {
        Outcome {
            lifecycle: Some(LifecycleProgress {
                phase: LifecyclePhase::LoadComplete,
                initial_failure: false,
            }),
            state: RealmState::Ready,
            diagnostics: 0,
            diagnostics_match: true,
            missing: false,
            pending: 1,
            initial_results: 1,
            initial_completions: 0,
            elapsed: Duration::from_secs(1),
        }
    }
    #[test]
    fn fixed_suite_and_roster_reject_unknown_duplicate_cross_suite_and_changed_bytes() {
        let mut suite = None;
        assert_eq!(suite.unwrap_or_default(), Suite::Observer);
        Suite::select(&mut suite, "observer").unwrap();
        assert!(Suite::select(&mut suite, "output-config").is_err());
        assert!(Suite::parse("arbitrary").is_err());
        let manifest = manifest();
        assert_eq!(manifest.cases.len(), 5);
        assert!(manifest.case("01-sync-pass").is_err());
        let mut bytes = include_bytes!("../../docs/WPT_HARNESS_OUTPUT_CONTROLS.json").to_vec();
        bytes[0] = b' ';
        assert!(Manifest::parse(&bytes).is_err());
    }
    #[test]
    fn typed_text_and_semantic_constraints_never_treat_unknown_as_payload_or_null() {
        let manifest = manifest();
        let case = &manifest.cases[1];
        let stack = OptionalUtf16::Text {
            units: Utf16::from_units(vec![120]).unwrap(),
        };
        let accepted = ledger(case, record(case, SubtestStatus::Fail, stack));
        check(case, accepted.observation(), outcome()).unwrap();
        for stack in [OptionalUtf16::Null, OptionalUtf16::Undefined] {
            let wrong = ledger(case, record(case, SubtestStatus::Fail, stack));
            assert!(check(case, wrong.observation(), outcome()).is_err());
        }
        let wrong = ledger(case, record(case, SubtestStatus::Pass, OptionalUtf16::Null));
        assert!(check(case, wrong.observation(), outcome()).is_err());
        assert!(serde_json::from_str::<TextKind>("\"UNKNOWN\"").is_err());
        assert!(serde_json::from_str::<TextKind>("{\"Null\":null}").is_err());
        assert!(serde_json::from_str::<TextKind>("null").is_err());
    }
    #[test]
    fn full_document_source_plan_rejects_framing_order_attributes_inertness_and_extra_resources() {
        let manifest = manifest();
        let case = &manifest.cases[0];
        let url = format!("{STDIO_ORIGIN}{}", case.url_path);
        let plan = case.source_plan(&url).unwrap();
        assert_ne!(plan.harness_node, 1);
        for html in [
            case.html_utf8.replacen("<script>\n", "<script>", 1),
            case.html_utf8.replacen(HARNESS_ROUTE, VENDOR_ROUTE, 1),
            case.html_utf8
                .replacen("<script src", "<script defer src", 1),
            case.html_utf8
                .replacen("<script src", "<script async src", 1),
            case.html_utf8
                .replacen("<script src", "<script type=\"module\" src", 1),
            case.html_utf8
                .replacen("<head>", "<head><script>bad()</script>", 1),
            case.html_utf8
                .replacen("<head>", "<head><base href=\"/\">", 1),
            case.html_utf8
                .replacen("<head>", "<head><style>@import '/trap';</style>", 1),
            case.html_utf8.replacen(
                "<meta charset=\"utf-8\">",
                "<meta charset=\"utf-8\" http-equiv=\"Refresh\" content=\"0;url=/trap\">",
                1,
            ),
            case.html_utf8
                .replacen("<script src", "<template><script src", 1),
            case.html_utf8
                .replacen("<body>", "<body><img src=\"/trap\">", 1),
        ] {
            assert!(derive_plan(&html, &case.body_source_utf8, &url).is_err());
        }
        assert!(derive_plan(&case.html_utf8, "wrong body\n", &url).is_err());
    }
    #[test]
    fn actual_two_external_inventory_rejects_wrong_vendor_node_count_and_provenance() {
        let manifest = manifest();
        let case = &manifest.cases[0];
        let url = format!("{STDIO_ORIGIN}{}", case.url_path);
        let harness = include_str!("../../tools/wpt/upstream/resources/testharness.js");
        let vendor = include_str!("../../tools/wpt/upstream/resources/testharnessreport.js");
        let plan = case.source_plan(&url).unwrap();
        let inventory = SourceInventory {
            plan,
            body: case.body_source_utf8.clone(),
            vendor: vendor.into(),
            vendor_url: format!("{STDIO_ORIGIN}{VENDOR_ROUTE}"),
        };
        let harness_url = format!("{STDIO_ORIGIN}{HARNESS_ROUTE}");
        let make = || Request {
            url: url.clone(),
            html: case.html_utf8.clone(),
            external_scripts: Some(vec![
                ExternalScriptSource {
                    node: plan.harness_node,
                    source: harness.into(),
                    provenance: Some(ClassicSourceProvenance {
                        response_url: harness_url.clone(),
                        error_visibility: ClassicErrorVisibility::SameOrigin,
                    }),
                },
                ExternalScriptSource {
                    node: plan.vendor_node,
                    source: vendor.into(),
                    provenance: Some(ClassicSourceProvenance {
                        response_url: inventory.vendor_url.clone(),
                        error_visibility: ClassicErrorVisibility::SameOrigin,
                    }),
                },
            ]),
        };
        inventory
            .verify(&make(), &case.html_utf8, &url, harness, &harness_url)
            .unwrap();
        for change in 0..6 {
            let mut request = make();
            let sources = request.external_scripts.as_mut().unwrap();
            match change {
                0 => sources[1].source.push(' '),
                1 => sources[1].node = 1,
                2 => sources.swap(0, 1),
                3 => {
                    sources.pop();
                }
                4 => sources[1].provenance = None,
                _ => {
                    sources[1].provenance.as_mut().unwrap().error_visibility =
                        ClassicErrorVisibility::Muted
                }
            }
            assert!(
                inventory
                    .verify(&request, &case.html_utf8, &url, harness, &harness_url)
                    .is_err()
            );
        }
    }
    #[test]
    fn missing_prefix_and_final_only_timeout_are_not_empty_or_finished_shortcuts() {
        let manifest = manifest();
        let case = &manifest.cases[4];
        let row = TestRecord {
            id: 1,
            index: 0,
            name: Utf16::from_units("owned sync assertion".encode_utf16().collect()).unwrap(),
            status: SubtestStatus::Pass,
            message: OptionalUtf16::Null,
            stack: OptionalUtf16::Null,
        };
        let mut ledger = ObserverLedger::new(ObserverExpectation::WindowHarness {
            run_id: case.run_id,
        })
        .unwrap();
        ledger
            .stage(ObserverReply::WindowHarness {
                run_id: case.run_id,
                first_ordinal: 0,
                next_ordinal: 1,
                records: vec![ObserverRecord::Result { test: row }],
                progress: ObserverProgress::Waiting,
                execution_failed: true,
            })
            .unwrap()
            .commit();
        let mut proof = outcome();
        proof.missing = true;
        proof.pending = 0;
        proof.diagnostics = 1;
        check(case, ledger.observation(), proof).unwrap();
        let mut proof = outcome();
        proof.missing = true;
        proof.pending = 0;
        proof.diagnostics = 1;
        proof.diagnostics_match = false;
        assert!(check(case, ledger.observation(), proof).is_err());
        let empty = ObserverLedger::new(ObserverExpectation::WindowHarness {
            run_id: case.run_id,
        })
        .unwrap();
        let mut proof = outcome();
        proof.missing = true;
        proof.pending = 0;
        proof.diagnostics = 1;
        assert!(check(case, empty.observation(), proof).is_err());
        let mut forged = ObserverLedger::new(ObserverExpectation::WindowHarness {
            run_id: case.run_id,
        })
        .unwrap();
        forged
            .stage(ObserverReply::WindowHarness {
                run_id: case.run_id,
                first_ordinal: 0,
                next_ordinal: 1,
                records: vec![ObserverRecord::Result {
                    test: record(&manifest.cases[0], SubtestStatus::Pass, OptionalUtf16::Null),
                }],
                progress: ObserverProgress::Waiting,
                execution_failed: false,
            })
            .unwrap()
            .commit();
        let missing = || {
            let mut proof = outcome();
            proof.missing = true;
            proof.pending = 0;
            proof.diagnostics = 1;
            proof
        };
        assert!(check(case, forged.observation(), missing()).is_err());
        forged.fail_parent(mg_sparkle::page_session::observer::ObserverParentFailure::Deadline);
        assert!(
            check(case, forged.observation(), missing()).is_err(),
            "Parent MissingCompletion cannot supply native failure"
        );
        let timeout = &manifest.cases[3];
        let row = record(timeout, SubtestStatus::Timeout, OptionalUtf16::Null);
        let mut ledger = ObserverLedger::new(ObserverExpectation::WindowHarness {
            run_id: timeout.run_id,
        })
        .unwrap();
        ledger
            .stage(ObserverReply::WindowHarness {
                run_id: timeout.run_id,
                first_ordinal: 0,
                next_ordinal: 1,
                records: vec![ObserverRecord::Completion {
                    harness: HarnessRecord {
                        status: HarnessStatus::Timeout,
                        message: OptionalUtf16::Null,
                        stack: OptionalUtf16::Null,
                    },
                    inventory: vec![row],
                }],
                progress: ObserverProgress::Finished,
                execution_failed: false,
            })
            .unwrap()
            .commit();
        let mut proof = outcome();
        proof.pending = 0;
        proof.elapsed = Duration::from_secs(10);
        check(timeout, ledger.observation(), proof).unwrap();
        let mut proof = outcome();
        proof.pending = 0;
        proof.elapsed = Duration::from_secs(1);
        assert!(check(timeout, ledger.observation(), proof).is_err());
    }
}
