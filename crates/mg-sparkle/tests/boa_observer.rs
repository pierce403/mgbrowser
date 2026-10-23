#![cfg(feature = "modern")]
//! Frozen authored controls, not upstream passes or hostile-page attestation.
use mg_sparkle::{
    js_browser::{
        ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource, Request,
        boa::BoaPageRealm,
    },
    page_session::{
        LifecyclePhase, LifecycleProgress, RealmState, SessionReply, TaskPump,
        observer::{ObserverExpectation, ObserverInit, ObserverLedger, ObserverProgress},
    },
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const HARNESS: &str = include_str!("../../../tools/wpt/upstream/resources/testharness.js");
const CONTROLS: &str = include_str!("../../../docs/WPT_OBSERVER_CONTROLS.json");

fn start(control: &Value) -> (BoaPageRealm, SessionReply) {
    let url = format!(
        "https://observer.example.test{}",
        control["url_path"].as_str().unwrap()
    );
    let config: ObserverInit = serde_json::from_value(json!({
        "kind":"WindowHarness", "contract":"PinnedWindowHarnessV1",
        "run_id":control["run_id"], "bootstrap_node":1
    }))
    .unwrap();
    let (realm, reply) = BoaPageRealm::start_with_observer(
        Request {
            url,
            html: control["html_utf8"].as_str().unwrap().into(),
            external_scripts: Some(vec![ExternalScriptSource {
                node: 1,
                source: HARNESS.into(),
                provenance: Some(ClassicSourceProvenance {
                    response_url: "https://observer.example.test/resources/testharness.js".into(),
                    error_visibility: ClassicErrorVisibility::SameOrigin,
                }),
            }]),
        },
        config,
    );
    assert_eq!(
        reply.state,
        RealmState::Ready,
        "{}: {:?}",
        control["id"],
        reply.errors
    );
    (realm.expect("retained authored realm"), reply)
}

struct LibraryConsumer {
    expected: ObserverExpectation,
    ledger: ObserverLedger,
    lifecycle: Option<LifecycleProgress>,
    report: Option<mg_butane::modern::Report>,
    revision: Option<u64>,
}
impl LibraryConsumer {
    fn new(control: &Value) -> Self {
        let expected = ObserverExpectation::WindowHarness {
            run_id: control["run_id"].as_u64().unwrap(),
        };
        Self {
            expected,
            ledger: ObserverLedger::new(expected).unwrap(),
            lifecycle: None,
            report: None,
            revision: None,
        }
    }
    fn accept(&mut self, reply: &mut SessionReply) -> Value {
        // Retain raw data before moving observer ownership into the SAME pure
        // consumer ledger as the real host. This library test has no process
        // allocator/transport/combined-active-charge proof and does not fake it.
        let raw = serde_json::to_value(&*reply).unwrap();
        reply.validate_observer_frame(self.expected).unwrap();
        let report = reply.boa.as_ref().unwrap();
        report.validate().unwrap();
        if let Some(old) = &self.report {
            report.validate_after(old).unwrap();
        }
        let next_lifecycle = if self.revision.is_some() {
            assert_eq!(reply.revision, self.revision.unwrap() + 1);
            assert_eq!(reply.scripts_executed, 0);
            reply
                .validate_lifecycle_after(self.lifecycle, true)
                .unwrap()
        } else {
            assert_eq!(reply.revision, 0);
            reply.validate_lifecycle_initial().unwrap()
        };
        assert!(reply.navigation.is_none());
        assert!(reply.acknowledgements.is_empty());
        mg_sparkle::document::project_nodes(
            reply.snapshot.as_ref().unwrap().nodes.clone(),
            "https://observer.example.test/owned",
        )
        .unwrap();
        // No verdict is based on raw decoding alone. All library transaction
        // checks precede stage/commit; real one-charge ownership is tested by
        // root's separate restricted-worker and Browser hosts.
        let staged = self.ledger.stage(reply.observer.take().unwrap()).unwrap();
        staged.commit();
        self.lifecycle = next_lifecycle;
        self.report = Some(report.clone());
        self.revision = Some(reply.revision);
        let observer = raw["observer"].clone();
        assert_eq!(observer["kind"], "WindowHarness");
        observer
    }
}

fn text(value: &Value) -> Option<Vec<u16>> {
    match value["kind"].as_str().unwrap() {
        "Null" | "Undefined" => None,
        "Text" => Some(
            value["units"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect(),
        ),
        _ => panic!("invalid optional text: {value}"),
    }
}

fn assert_fields(actual: &Value, expected: &Value, final_row: bool) {
    for key in ["id", "index", "status"] {
        assert_eq!(actual[key], expected[key], "{key}: {actual}");
    }
    let units_key = if final_row {
        "final_name_units"
    } else {
        "result_name_units"
    };
    let name = expected.get(units_key).cloned().unwrap_or_else(|| {
        json!(
            expected["name"]
                .as_str()
                .unwrap()
                .encode_utf16()
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(actual["name"], name);
    for key in ["message", "stack"] {
        if expected.get(key).is_some() {
            assert!(
                expected[key].is_null(),
                "add explicit non-null check for {expected}"
            );
            assert_eq!(actual[key], json!({"kind":"Null"}));
        }
        if let Some(fragment) = expected.get(format!("{key}_contains")) {
            let units = text(&actual[key]).expect("actual text");
            assert!(
                String::from_utf16(&units)
                    .unwrap()
                    .contains(fragment.as_str().unwrap())
            );
        }
    }
}

#[test]
fn complete_unchanged_harness_preserves_all_frozen_fast_control_semantics() {
    let manifest: Value = serde_json::from_str(CONTROLS).unwrap();
    assert_eq!(HARNESS.len(), 198291);
    assert_eq!(manifest["case_count"], 16);
    // Missing completion and the genuine unmodified ten-second timer are
    // separately bounded host controls, not shortened to make this test fast.
    for control in manifest["cases"].as_array().unwrap() {
        if matches!(control["run_id"].as_u64().unwrap(), 8 | 10) {
            continue;
        }
        let (mut realm, mut reply) = start(control);
        let mut consumer = LibraryConsumer::new(control);
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        let mut cursor = 0;
        let mut last_admitted = 0;
        let mut last_executed = 0;
        let mut completion_settling = false;
        for _ in 0..63 {
            assert!(
                Instant::now() < deadline,
                "{} exceeded bound",
                control["id"]
            );
            assert_eq!(
                reply.state,
                RealmState::Ready,
                "{}: {:?}",
                control["id"],
                reply.errors
            );
            let report = reply.boa.as_ref().unwrap();
            assert!(report.jobs_admitted >= last_admitted && report.jobs_admitted <= 2048);
            assert!(report.jobs_executed >= last_executed && report.pending_jobs <= 256);
            last_admitted = report.jobs_admitted;
            last_executed = report.jobs_executed;
            diagnostics.extend(reply.errors.iter().cloned());
            let observer = consumer.accept(&mut reply);
            assert_eq!(observer["run_id"], control["run_id"]);
            assert_eq!(observer["first_ordinal"], cursor);
            let delta = observer["records"].as_array().unwrap();
            cursor += delta.len();
            assert_eq!(observer["next_ordinal"], cursor);
            if delta.iter().any(|record| record["kind"] == "Completion") {
                completion_settling = observer["progress"] == "Settling";
            }
            records.extend(delta.iter().cloned());
            if observer["progress"] == "Finished" {
                break;
            }
            let wake = reply
                .task_wake
                .as_ref()
                .expect("unfinished complete control has actual work");
            assert!(
                wake.delay_ms <= 100,
                "do not drain retained default file timer"
            );
            std::thread::sleep(Duration::from_millis(u64::from(wake.delay_ms)));
            reply = realm.pump_tasks(TaskPump {
                wake_token: wake.token,
            });
        }
        let view = consumer.ledger.observation();
        assert_eq!(view.progress, Some(ObserverProgress::Finished));
        assert!(view.fault.is_none() && view.parent_failure.is_none());
        let final_observer =
            json!({"progress":"Finished", "execution_failed":view.execution_failed});
        assert_eq!(json!(view.records), json!(records));
        assert_eq!(final_observer["progress"], "Finished", "{}", control["id"]);
        assert_eq!(reply.lifecycle.unwrap().phase, LifecyclePhase::LoadComplete);
        let expected = &control["expected"];
        let failed = expected
            .get("native_execution_failed")
            .or_else(|| expected.get("native_execution_failed_after_settlement"))
            .unwrap();
        assert_eq!(
            &final_observer["execution_failed"], failed,
            "{}",
            control["id"]
        );
        if let Some(exact) = expected.get("diagnostics_exact") {
            assert_eq!(json!(diagnostics), *exact, "{}", control["id"]);
        } else {
            assert!(diagnostics.is_empty(), "{}: {diagnostics:?}", control["id"]);
        }
        let results: Vec<_> = records.iter().filter(|r| r["kind"] == "Result").collect();
        assert_eq!(
            json!(
                results
                    .iter()
                    .map(|r| r["test"]["index"].clone())
                    .collect::<Vec<_>>()
            ),
            expected["result_order_indices"]
        );
        let completions: Vec<_> = records
            .iter()
            .filter(|r| r["kind"] == "Completion")
            .collect();
        assert_eq!(completions.len(), 1);
        let completion = completions[0];
        let harness = &completion["harness"];
        let status = expected
            .get("harness_status")
            .or_else(|| expected.get("harness_status_at_native_callback"))
            .unwrap();
        assert_eq!(&harness["status"], status, "{}", control["id"]);
        for field in ["message", "stack"] {
            if let Some(value) = expected
                .get(format!("harness_{field}"))
                .or_else(|| expected.get(format!("harness_{field}_at_native_callback")))
            {
                if value.is_null() {
                    assert_eq!(harness[field], json!({"kind":"Null"}));
                } else {
                    assert_eq!(
                        text(&harness[field]).unwrap(),
                        value.as_str().unwrap().encode_utf16().collect::<Vec<_>>()
                    );
                }
            }
            if let Some(kind) = expected
                .get(format!("harness_{field}_kind"))
                .or_else(|| expected.get(format!("harness_{field}_kind_at_native_callback")))
            {
                assert_eq!(&harness[field]["kind"], kind);
            }
        }
        let inventory = completion["inventory"].as_array().unwrap();
        assert_eq!(
            inventory.len(),
            expected["inventory"].as_array().unwrap().len()
        );
        for (row, expected_row) in inventory
            .iter()
            .zip(expected["inventory"].as_array().unwrap())
        {
            assert_fields(row, expected_row, true);
            let result = results
                .iter()
                .find(|r| r["test"]["id"] == row["id"])
                .unwrap();
            assert_fields(&result["test"], expected_row, false);
        }
        if matches!(control["run_id"].as_u64().unwrap(), 13 | 15) {
            assert!(completion_settling);
        }
        if control["run_id"] == 9 {
            assert!(reply.boa.as_ref().unwrap().pending_jobs > 0 && reply.task_wake.is_some());
        }
    }
}

#[test]
fn missing_completion_stays_waiting_after_real_load_and_idle_is_not_a_result() {
    let manifest: Value = serde_json::from_str(CONTROLS).unwrap();
    let (mut realm, mut reply) = start(&manifest["cases"][7]);
    let mut consumer = LibraryConsumer::new(&manifest["cases"][7]);
    for _ in 0..8 {
        assert!(reply.errors.is_empty());
        let observer = consumer.accept(&mut reply);
        assert_eq!(observer["progress"], "Waiting");
        assert!(observer["records"].as_array().unwrap().is_empty());
        let Some(wake) = reply.task_wake.as_ref() else {
            break;
        };
        assert_eq!(wake.delay_ms, 0);
        reply = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
    }
    assert_eq!(reply.lifecycle.unwrap().phase, LifecyclePhase::LoadComplete);
    assert!(reply.task_wake.is_none());
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
    let view = consumer.ledger.observation();
    assert_eq!(view.progress, Some(ObserverProgress::Waiting));
    assert!(!view.execution_failed && view.fault.is_none() && view.parent_failure.is_none());
}
