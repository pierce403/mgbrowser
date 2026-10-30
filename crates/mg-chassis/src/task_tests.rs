//! Independent parent coordination controls; no browser process or network.
use super::*;
use crate::scripts::{ScriptSession, SessionUpdate};
use mg_sparkle::page_session::{ArenaSnapshot, ControlAck, EventOutcome};
use std::{collections::VecDeque, sync::Mutex};

#[derive(Default)]
struct State {
    pumps: Vec<(u64, u64)>,
    updates: VecDeque<SessionUpdate>,
    charges: usize,
    queued: bool,
    validation_pending: bool,
    canceled: bool,
    fail_charge: bool,
}
struct FakeSession(Arc<Mutex<State>>, u64);
impl ScriptSession for FakeSession {
    fn id(&self) -> u64 {
        self.1
    }
    fn try_dispatch(&mut self, _: SessionInput, _: u64) -> Result<u64, String> {
        let state = self.0.lock().unwrap();
        if state.queued || state.validation_pending {
            Err("backpressure".into())
        } else {
            Err("This fixture accepts tasks only".into())
        }
    }
    fn try_task_pump(&mut self, pump: TaskPump, revision: u64) -> Result<u64, String> {
        let mut state = self.0.lock().unwrap();
        if state.queued || state.validation_pending {
            return Err("backpressure".into());
        }
        state.queued = true;
        state.pumps.push((pump.wake_token, revision));
        Ok(revision + 1)
    }
    fn try_recv(&mut self) -> Option<SessionUpdate> {
        let mut state = self.0.lock().unwrap();
        let update = state.updates.pop_front()?;
        state.queued = false;
        state.validation_pending = update.reply.is_ok();
        Some(update)
    }
    fn cancel(&self) {
        self.0.lock().unwrap().canceled = true;
    }
    fn charge_active(&mut self, _: Duration) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        if !state.validation_pending {
            return Err("No projection acknowledgement due".into());
        }
        state.validation_pending = false;
        state.charges += 1;
        if state.fail_charge {
            Err("Owned projection charge rejection".into())
        } else {
            Ok(())
        }
    }
}
fn fixture() -> (Browser, Arc<Mutex<State>>, usize) {
    let mut app = test_app();
    app.page_url = "https://example.test/tasks".into();
    app.address = app.page_url.clone();
    app.document = document::parse(
        "<title>Retained</title><form><input id=value name=value value=old></form><p>Readable</p>",
        &app.page_url,
    );
    app.generation = 42;
    app.script_blocked = false;
    app.observer_ledger = Some(ObserverLedger::new(Default::default()).unwrap());
    // These pre-existing controls exercise later work on an already loaded
    // document, not initialization or its mandatory lifecycle admissions.
    app.lifecycle = Some(LifecycleProgress {
        phase: LifecyclePhase::LoadComplete,
        initial_failure: false,
    });
    let node = app.document.query_selector(0, "#value").unwrap().unwrap();
    assert!(
        app.input_item(node).is_some(),
        "fixture must contain a native-editable control"
    );
    let state = Arc::new(Mutex::new(State::default()));
    app.page_session = Some(Box::new(FakeSession(state.clone(), 91)));
    app.replace_task_wake(Some(ScheduledTaskWake {
        token: 1,
        at: Instant::now(),
    }));
    (app, state, node)
}
fn reply(app: &Browser, wake: Option<TaskWake>) -> SessionReply {
    let mut report = mg_butane::modern::Engine::new().unwrap().report();
    report.pending_jobs = usize::from(wake.is_some());
    report.jobs_admitted = report.pending_jobs;
    SessionReply {
        revision: app.page_revision + 1,
        snapshot: Some(ArenaSnapshot {
            nodes: app.document.nodes.clone(),
        }),
        outcome: EventOutcome::default(),
        default_action: DefaultAction::None,
        navigation: None,
        errors: vec![],
        scripts_executed: 0,
        allocations: None,
        boa: Some(report),
        state: RealmState::Ready,
        acknowledgements: vec![],
        task_wake: wake,
        lifecycle: app.lifecycle,
        observer: Some(mg_sparkle::page_session::observer::ObserverReply::Disabled),
    }
}
fn complete(app: &Browser, state: &Arc<Mutex<State>>, reply: SessionReply) {
    state.lock().unwrap().updates.push_back(SessionUpdate {
        generation: app.generation,
        session_id: 91,
        sequence: Some(app.page_revision + 1),
        reply: Ok(reply),
        automation: None,
        continuation: scripts::SessionContinuation::Live,
    });
}

#[test]
fn task_due_time_backpressure_and_one_projection_preserve_newer_native_edits() {
    let (mut app, state, node) = fixture();
    let now = Instant::now();
    app.task_wake.as_mut().unwrap().at = now + Duration::from_secs(1);
    app.poll_task_wake(now);
    assert!(state.lock().unwrap().pumps.is_empty());
    app.values.insert(node, "before pump".into());
    app.edit_versions.insert(node, 7);
    app.poll_task_wake(now + Duration::from_secs(1));
    assert_eq!(state.lock().unwrap().pumps, [(1, 0)]);
    assert_eq!(app.script_work_status(), ScriptWorkStatus::Pending);
    assert!(!app.is_loading());
    assert!(
        app.dispatch_page(InputKind::Click { target: node })
            .is_err()
    );
    app.values.insert(node, "newer café".into());
    app.edit_versions.insert(node, 8);
    let response = reply(
        &app,
        Some(TaskWake {
            token: 2,
            delay_ms: 40,
        }),
    );
    complete(&app, &state, response);
    let before = Instant::now();
    app.poll_page_session();
    let after = Instant::now();
    assert_eq!(state.lock().unwrap().charges, 1);
    assert!(!state.lock().unwrap().validation_pending);
    assert_eq!(app.page_revision, 1);
    assert_eq!(app.values[&node], "newer café");
    assert_eq!(app.edit_versions[&node], 8);
    assert!(app.pending_transaction.is_none());
    let wake = app.task_wake.unwrap();
    assert_eq!(wake.token, 2);
    assert!(wake.at >= before + Duration::from_millis(40));
    assert!(wake.at <= after + Duration::from_millis(40));
    app.poll_page_session();
    assert_eq!(
        state.lock().unwrap().charges,
        1,
        "No double projection acknowledgement"
    );
}

#[test]
fn observer_publication_is_atomic_with_projection_and_one_active_charge() {
    use mg_sparkle::page_session::observer::*;
    struct ObserverRuntime;
    impl scripts::ScriptRuntime for ObserverRuntime {
        fn observer_expectation(&self) -> ObserverExpectation {
            ObserverExpectation::WindowHarness { run_id: 1 }
        }
        fn set_generation(&self, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn start(
            &self,
            _: mg_sparkle::js_browser::Request,
            _: u64,
        ) -> Result<(Box<dyn ScriptSession>, SessionReply), String> {
            panic!("This pure parent control must not start a worker")
        }
        fn close(&self) {}
        fn wait_idle(&self) -> Result<(), String> {
            Ok(())
        }
    }
    for condition in ["accept", "finished", "projection", "charge", "wrong-run"] {
        let (mut app, state, _) = fixture();
        app.child_pool = Arc::new(ObserverRuntime);
        app.observer_ledger =
            Some(ObserverLedger::new(app.child_pool.observer_expectation()).unwrap());
        app.poll_task_wake(Instant::now());
        let mut response = reply(&app, None);
        response.observer = Some(ObserverReply::WindowHarness {
            run_id: if condition == "wrong-run" { 2 } else { 1 },
            first_ordinal: 0,
            next_ordinal: 1,
            records: vec![ObserverRecord::Result {
                test: TestRecord {
                    id: 1,
                    index: 0,
                    name: Utf16::from_units(vec![0x61]).unwrap(),
                    status: SubtestStatus::Pass,
                    message: OptionalUtf16::Null,
                    stack: OptionalUtf16::Null,
                },
            }],
            progress: ObserverProgress::Waiting,
            execution_failed: false,
        });
        if condition == "finished" {
            let Some(ObserverReply::WindowHarness {
                records,
                next_ordinal,
                progress,
                ..
            }) = &mut response.observer
            else {
                unreachable!()
            };
            records.push(ObserverRecord::Completion {
                harness: HarnessRecord {
                    status: HarnessStatus::Ok,
                    message: OptionalUtf16::Null,
                    stack: OptionalUtf16::Null,
                },
                inventory: vec![TestRecord {
                    id: 1,
                    index: 0,
                    name: Utf16::from_units(vec![0x61]).unwrap(),
                    status: SubtestStatus::Pass,
                    message: OptionalUtf16::Null,
                    stack: OptionalUtf16::Null,
                }],
            });
            *next_ordinal = 2;
            *progress = ObserverProgress::Finished;
            response.boa.as_mut().unwrap().jobs_admitted = 1;
            response.boa.as_mut().unwrap().pending_jobs = 1;
            response.task_wake = Some(TaskWake {
                token: 2,
                delay_ms: 60_000,
            });
        }
        if condition == "projection" {
            response.snapshot.as_mut().unwrap().nodes[0]
                .children
                .push(usize::MAX);
        }
        state.lock().unwrap().fail_charge = condition == "charge";
        complete(&app, &state, response);
        assert_eq!(app.script_observation().unwrap().observation().cursor, 0);
        app.poll_page_session();
        let observation = app.script_observation().unwrap().observation();
        let accepted = matches!(condition, "accept" | "finished");
        let records = if condition == "finished" {
            2
        } else {
            usize::from(accepted)
        };
        assert_eq!(observation.cursor, records as u32, "{condition}");
        assert_eq!(observation.records.len(), records, "{condition}");
        assert_eq!(app.page_revision, u64::from(accepted), "{condition}");
        assert_eq!(state.lock().unwrap().charges, 1, "{condition}");
        assert_eq!(
            state.lock().unwrap().canceled,
            !accepted || condition == "finished",
            "{condition}"
        );
        assert_eq!(
            app.script_blocked,
            !accepted || condition == "finished",
            "{condition}"
        );
        if condition == "finished" {
            assert!(
                app.task_wake.is_none(),
                "do not pump an unrelated future timer after accepted cutoff"
            );
            app.poll_task_wake(Instant::now() + Duration::from_secs(100));
            assert_eq!(state.lock().unwrap().pumps.len(), 1);
        }
    }
}

#[test]
fn task_reply_cannot_acknowledge_controls_or_produce_activation_effects() {
    for change in [
        "ack",
        "default",
        "click",
        "submit",
        "scripts",
        "zero-wake",
        "reused-wake",
        "missing-wake",
        "empty-jobs",
        "legacy-wake",
        "fatal",
        "terminal-wake",
        "navigation",
    ] {
        let (mut app, state, node) = fixture();
        app.values.insert(node, "retained".into());
        app.edit_versions.insert(node, 9);
        let old_epoch = app.dom_epoch;
        app.poll_task_wake(Instant::now());
        let mut response = reply(&app, None);
        match change {
            "ack" => response
                .acknowledgements
                .push(ControlAck { node, version: 9 }),
            "default" => response.default_action = DefaultAction::FollowLink { node },
            "click" => response.outcome.click_canceled = Some(false),
            "submit" => response.outcome.submit_canceled = Some(false),
            "scripts" => response.scripts_executed = 1,
            "zero-wake" | "reused-wake" | "terminal-wake" => {
                let report = response.boa.as_mut().unwrap();
                report.pending_jobs = 1;
                report.jobs_admitted = 1;
                response.task_wake = Some(TaskWake {
                    token: if change == "zero-wake" {
                        0
                    } else if change == "reused-wake" {
                        1
                    } else {
                        2
                    },
                    delay_ms: 0,
                });
                if change == "terminal-wake" {
                    response.state = RealmState::Closed;
                }
            }
            "missing-wake" => {
                let report = response.boa.as_mut().unwrap();
                report.pending_jobs = 1;
                report.jobs_admitted = 1;
            }
            "empty-jobs" => {
                response.task_wake = Some(TaskWake {
                    token: 2,
                    delay_ms: 0,
                })
            }
            "legacy-wake" => {
                response.boa = None;
                response.allocations = Some(mg_butane::runtime::Runtime::new().allocation_report());
                response.task_wake = Some(TaskWake {
                    token: 2,
                    delay_ms: 0,
                });
            }
            "fatal" => response.boa.as_mut().unwrap().fatal_reason = Some("latched failure".into()),
            "navigation" => response.navigation = Some("javascript:trap()".into()),
            _ => unreachable!(),
        }
        complete(&app, &state, response);
        app.poll_page_session();
        assert!(app.script_blocked, "{change}");
        assert!(state.lock().unwrap().canceled, "{change}");
        assert_eq!(state.lock().unwrap().charges, 1, "{change}");
        assert_eq!(app.dom_epoch, old_epoch, "{change}");
        assert_eq!(app.page_revision, 0, "{change}");
        assert_eq!(app.values[&node], "retained", "{change}");
        assert_eq!(app.edit_versions[&node], 9, "{change}");
        assert!(app.task_wake.is_none(), "{change}");
    }
}

#[test]
fn poll_advances_each_unpainted_background_page_and_none_disarms_wake() {
    let (mut first, a, _) = fixture();
    let (mut second, b, _) = fixture();
    first.poll();
    second.poll();
    assert_eq!(a.lock().unwrap().pumps, [(1, 0)]);
    assert_eq!(b.lock().unwrap().pumps, [(1, 0)]);
    complete(&first, &a, reply(&first, None));
    complete(&second, &b, reply(&second, None));
    first.poll();
    second.poll();
    for app in [&first, &second] {
        assert_eq!(app.page_revision, 1);
        assert_eq!(app.script_work_status(), ScriptWorkStatus::Idle);
        assert!(!app.is_loading());
    }
    first.poll();
    second.poll();
    assert_eq!(a.lock().unwrap().charges, 1);
    assert_eq!(b.lock().unwrap().charges, 1);
}

#[test]
fn native_event_smoke_waits_for_advertised_work_but_still_fails_a_missing_form() {
    let (mut app, _, _) = fixture();
    app.smoke = Some("owned task fixture".into());
    app.smoke_events = true;
    app.smoke_since = Instant::now() - Duration::from_secs(1);
    app.last_load_ok = true;
    let canvas = Canvas::new(1, 1, 0xffffff);
    app.smoke_event_step(&canvas);
    assert!(
        !app.smoke_failed,
        "A real later task may create the control"
    );
    assert_eq!(app.event_smoke_stage, 0);
    app.task_wake = None;
    app.smoke_event_step(&canvas);
    assert!(
        app.smoke_failed,
        "No pending work must retain the old failure"
    );
    assert!(app.status.contains("no real query control"));
}

#[test]
fn stale_task_completion_never_publishes_a_projection() {
    for field in ["generation", "session", "sequence", "revision"] {
        let (mut app, state, _) = fixture();
        app.poll_task_wake(Instant::now());
        let mut update = SessionUpdate {
            generation: 42,
            session_id: 91,
            sequence: Some(1),
            reply: Ok(reply(&app, None)),
            automation: None,
            continuation: scripts::SessionContinuation::Live,
        };
        match field {
            "generation" => update.generation += 1,
            "session" => update.session_id += 1,
            "sequence" => update.sequence = Some(2),
            "revision" => update.reply.as_mut().unwrap().revision = 2,
            _ => unreachable!(),
        }
        state.lock().unwrap().updates.push_back(update);
        app.poll_page_session();
        assert!(app.script_blocked, "{field}");
        assert!(app.task_wake.is_none(), "{field}");
        assert_eq!(app.page_revision, 0, "{field}");
        assert_eq!(state.lock().unwrap().charges, 1, "{field}");
        assert!(!state.lock().unwrap().validation_pending, "{field}");
        app.poll_page_session();
        app.poll_task_wake(Instant::now() + Duration::from_secs(1));
        assert_eq!(
            state.lock().unwrap().charges,
            1,
            "No duplicate charge: {field}"
        );
        assert_eq!(state.lock().unwrap().pumps.len(), 1, "No retry: {field}");
    }
}

#[test]
fn unsolicited_success_is_charged_once_but_error_closures_are_not() {
    for success in [true, false] {
        let (mut app, state, _) = fixture();
        state.lock().unwrap().updates.push_back(SessionUpdate {
            generation: 42,
            session_id: 91,
            sequence: None,
            reply: if success {
                Ok(reply(&app, None))
            } else {
                Err("Expired".into())
            },
            automation: None,
            continuation: scripts::SessionContinuation::Live,
        });
        app.poll_page_session();
        assert!(app.script_blocked);
        assert_eq!(state.lock().unwrap().charges, usize::from(success));
        assert_eq!(app.page_revision, 0);
        assert!(app.task_wake.is_none());
        app.poll_page_session();
        assert_eq!(state.lock().unwrap().charges, usize::from(success));
    }
}

#[test]
fn blocking_session_disarms_idle_timer_without_synthetic_activation() {
    let (mut app, state, _) = fixture();
    app.block_session("owned cancellation");
    app.poll_task_wake(Instant::now() + Duration::from_secs(60));
    assert_eq!(app.script_work_status(), ScriptWorkStatus::Blocked);
    assert!(state.lock().unwrap().pumps.is_empty());
    assert!(state.lock().unwrap().canceled);
}

#[test]
fn task_zero_startup_count_preserves_page_metric_until_navigation_reset() {
    let (mut app, state, _) = fixture();
    let mut initial = reply(&app, None);
    initial.scripts_executed = 3;
    app.script_summary(&initial);
    assert_eq!(app.performance_profile().scripts_executed, 3);
    app.poll_task_wake(Instant::now());
    complete(&app, &state, reply(&app, None));
    app.poll_page_session();
    assert_eq!(app.performance_profile().scripts_executed, 3);
    // This is the same metric reset called before navigation admission; avoid
    // starting an unrelated real transport merely to exercise a counter reset.
    app.devtools.new_navigation();
    assert_eq!(app.performance_profile().scripts_executed, 0);
}

#[test]
fn stale_initial_success_charges_only_its_owned_session_then_cancels() {
    let (mut app, current, _) = fixture();
    let stale = Arc::new(Mutex::new(State {
        validation_pending: true,
        ..State::default()
    }));
    let mut initial = reply(
        &app,
        Some(TaskWake {
            token: 999,
            delay_ms: 0,
        }),
    );
    initial.revision = 0;
    app.loading = true;
    app.fetching = true;
    app.inflight = 1;
    app.tx
        .send(Loaded {
            generation: app.generation - 1,
            document: None,
            result: Ok(net::Response {
                resource_same_origin_chain: false,
                url: url::Url::parse("https://example.test/stale").unwrap(),
                status: 200,
                content_type: "text/html".into(),
                content_disposition: None,
                body: b"<title>Stale</title>".to_vec(),
            }),
            script: Some(Ok(ScriptLoad {
                session: Some(Box::new(FakeSession(stale.clone(), 92))),
                reply: initial,
            })),
            fetch_html_ms: 0.0,
            parse_html_ms: 0.0,
            resource_profile: Default::default(),
            script_fetch_profile: Default::default(),
            script_worker_ms: 0.0,
        })
        .unwrap();
    app.poll();
    assert_eq!(stale.lock().unwrap().charges, 1);
    assert!(!stale.lock().unwrap().validation_pending);
    assert!(stale.lock().unwrap().canceled);
    assert_eq!(current.lock().unwrap().charges, 0);
    assert!(!current.lock().unwrap().canceled);
    assert!(current.lock().unwrap().pumps.is_empty());
    assert!(app.loading);
    assert_eq!(app.document.title, "Retained");
    assert_eq!(app.last_task_wake_token, 1);
    assert_eq!(app.task_wake.unwrap().token, 1);
    assert_eq!(app.page_session.as_ref().unwrap().id(), 91);
    app.poll();
    assert_eq!(stale.lock().unwrap().charges, 1);
}

fn lifecycle_reply(
    app: &Browser,
    phase: LifecyclePhase,
    failure: bool,
    executed: usize,
    pending: usize,
    wake: Option<TaskWake>,
) -> SessionReply {
    let mut reply = reply(app, wake);
    let report = reply.boa.as_mut().unwrap();
    report.jobs_admitted = executed + pending;
    report.jobs_executed = executed;
    report.pending_jobs = pending;
    reply.lifecycle = Some(LifecycleProgress {
        phase,
        initial_failure: failure,
    });
    reply
}

fn committed_lifecycle(
    errors: &[&str],
    broker_rejections: usize,
) -> (Browser, Arc<Mutex<State>>, usize) {
    let (mut app, state, node) = fixture();
    app.page_session = None;
    app.scripts_enabled = true;
    app.loading = true;
    app.fetching = true;
    app.inflight = 1;
    app.lifecycle = None;
    app.cdp_enabled = true;
    state.lock().unwrap().validation_pending = true;
    let mut initial = lifecycle_reply(
        &app,
        LifecyclePhase::Interactive,
        !errors.is_empty(),
        0,
        2,
        Some(TaskWake {
            token: 1,
            delay_ms: 0,
        }),
    );
    initial.revision = 0;
    initial.scripts_executed = 3;
    initial.errors = errors.iter().map(|value| (*value).into()).collect();
    app.tx
        .send(Loaded {
            generation: app.generation,
            document: None,
            result: Ok(net::Response {
                resource_same_origin_chain: false,
                url: url::Url::parse(&app.page_url).unwrap(),
                status: 200,
                content_type: "text/html".into(),
                content_disposition: None,
                body: b"<title>Retained</title>".to_vec(),
            }),
            script: Some(Ok(ScriptLoad {
                session: Some(Box::new(FakeSession(state.clone(), 91))),
                reply: initial,
            })),
            fetch_html_ms: 0.0,
            parse_html_ms: 0.0,
            resource_profile: Default::default(),
            script_fetch_profile: resources::ScriptFetchProfile {
                rejected_sources: broker_rejections,
                ..Default::default()
            },
            script_worker_ms: 0.0,
        })
        .unwrap();
    app.poll();
    (app, state, node)
}

#[test]
fn committed_initial_lifecycle_pumps_while_loading_and_charges_once_per_reply() {
    let (mut app, state, _) = committed_lifecycle(&[], 0);
    assert!(app.is_loading());
    assert!(!app.fetching);
    assert_eq!(
        app.initial_script_outcome().status,
        InitialScriptStatus::Pending
    );
    assert_eq!(state.lock().unwrap().charges, 1);
    assert_eq!(state.lock().unwrap().pumps, [(1, 0)]);
    assert!(matches!(
        app.cdp_events.as_slice(),
        [cdp_browser::Event::Committed { .. }]
    ));
    assert!(app.dispatch_page(InputKind::Click { target: 0 }).is_err());

    let dcl = lifecycle_reply(
        &app,
        LifecyclePhase::DomContentLoaded,
        false,
        1,
        1,
        Some(TaskWake {
            token: 2,
            delay_ms: 0,
        }),
    );
    complete(&app, &state, dcl);
    app.poll();
    assert!(app.is_loading());
    assert_eq!(state.lock().unwrap().charges, 2);
    assert_eq!(state.lock().unwrap().pumps, [(1, 0), (2, 1)]);
    let load = lifecycle_reply(&app, LifecyclePhase::LoadComplete, false, 2, 0, None);
    complete(&app, &state, load);
    app.poll();
    assert!(!app.is_loading());
    assert_eq!(
        app.initial_script_outcome().status,
        InitialScriptStatus::Complete
    );
    assert_eq!(app.initial_script_outcome().scripts_executed, 3);
    assert_eq!(state.lock().unwrap().charges, 3);
    let kinds: Vec<_> = app
        .cdp_events
        .iter()
        .map(|event| match event {
            cdp_browser::Event::Committed { .. } => "commit",
            cdp_browser::Event::DocumentUpdated => "dom-update",
            cdp_browser::Event::DomContentLoaded { .. } => "dcl",
            cdp_browser::Event::LoadComplete { .. } => "load",
            cdp_browser::Event::Finished { .. } => "stop",
            cdp_browser::Event::Started => "start",
        })
        .collect();
    assert_eq!(
        kinds,
        ["commit", "dom-update", "dcl", "dom-update", "load", "stop"]
    );
    app.poll();
    assert_eq!(state.lock().unwrap().charges, 3);
    assert_eq!(
        app.cdp_events.len(),
        6,
        "Milestones are not repeated by idle polls"
    );
}

#[test]
fn initial_failures_survive_clean_lifecycle_and_transient_status() {
    for (errors, broker) in [(vec!["SyntaxError: owned source"], 0), (vec![], 1)] {
        let (mut app, state, _) = committed_lifecycle(&errors, broker);
        let failed = !errors.is_empty(); // Broker failures are parent-only evidence.
        let dcl = lifecycle_reply(
            &app,
            LifecyclePhase::DomContentLoaded,
            failed,
            1,
            1,
            Some(TaskWake {
                token: 2,
                delay_ms: 0,
            }),
        );
        complete(&app, &state, dcl);
        app.poll();
        let load = lifecycle_reply(&app, LifecyclePhase::LoadComplete, failed, 2, 0, None);
        complete(&app, &state, load);
        app.poll();
        assert!(!app.is_loading());
        let outcome = app.initial_script_outcome();
        assert_eq!(outcome.status, InitialScriptStatus::Partial);
        assert_eq!(outcome.failure, Some(InitialScriptFailure::Script));
        assert_eq!(outcome.scripts_executed, 3);
        assert_eq!(outcome.error_count, errors.len());
        assert_eq!(outcome.broker_rejected_sources, broker);
        assert_eq!(outcome.diagnostics, errors);
        assert!(
            app.status().contains("0 diagnostics"),
            "Typed failures outlive clean task status"
        );
    }
}

#[test]
fn load_completion_does_not_wait_for_long_or_new_zero_delay_task() {
    for delay_ms in [0, 60_000] {
        let (mut app, state, _) = committed_lifecycle(&[], 0);
        let dcl = lifecycle_reply(
            &app,
            LifecyclePhase::DomContentLoaded,
            false,
            1,
            1,
            Some(TaskWake {
                token: 2,
                delay_ms: 0,
            }),
        );
        complete(&app, &state, dcl);
        app.poll();
        let load = lifecycle_reply(
            &app,
            LifecyclePhase::LoadComplete,
            false,
            2,
            1,
            Some(TaskWake { token: 3, delay_ms }),
        );
        complete(&app, &state, load);
        app.poll();
        assert!(!app.is_loading());
        assert_eq!(
            app.initial_script_outcome().status,
            InitialScriptStatus::Complete
        );
        assert!(matches!(
            app.script_work_status(),
            ScriptWorkStatus::Scheduled(_)
        ));
        assert_eq!(
            state.lock().unwrap().pumps.len(),
            2,
            "Load is observable before starting later work"
        );
        if delay_ms == 0 {
            let frozen = app.initial_script_outcome().clone();
            app.poll();
            assert_eq!(state.lock().unwrap().pumps.len(), 3);
            let mut later = lifecycle_reply(&app, LifecyclePhase::LoadComplete, false, 3, 0, None);
            later.errors.push("An unrelated later task failed".into());
            complete(&app, &state, later);
            app.poll();
            assert_eq!(app.initial_script_outcome(), &frozen);
            assert_eq!(
                app.cdp_events
                    .iter()
                    .filter(|event| matches!(
                        event,
                        cdp_browser::Event::DomContentLoaded { .. }
                            | cdp_browser::Event::LoadComplete { .. }
                    ))
                    .count(),
                2
            );
        }
    }
}

#[test]
fn invalid_lifecycle_and_opaque_failures_stop_without_fake_completion() {
    for kind in [
        "skip",
        "missing",
        "reset-failure",
        "navigation-advance",
        "opaque",
    ] {
        let (mut app, state, _) = committed_lifecycle(&["Owned startup error"], 0);
        let before = app.dom_epoch;
        if kind == "opaque" {
            state.lock().unwrap().updates.push_back(SessionUpdate {
                generation: app.generation,
                session_id: 91,
                sequence: Some(1),
                reply: Err("Opaque worker transport failure".into()),
                automation: None,
                continuation: scripts::SessionContinuation::Live,
            });
        } else {
            let mut invalid = lifecycle_reply(
                &app,
                LifecyclePhase::DomContentLoaded,
                true,
                1,
                1,
                Some(TaskWake {
                    token: 2,
                    delay_ms: 0,
                }),
            );
            match kind {
                "skip" => invalid.lifecycle.as_mut().unwrap().phase = LifecyclePhase::LoadComplete,
                "missing" => invalid.lifecycle = None,
                "reset-failure" => invalid.lifecycle.as_mut().unwrap().initial_failure = false,
                "navigation-advance" => {
                    invalid.navigation = Some("https://example.test/next".into())
                }
                _ => unreachable!(),
            }
            complete(&app, &state, invalid);
        }
        app.poll();
        assert!(!app.is_loading(), "{kind}");
        assert_eq!(app.dom_epoch, before, "{kind}");
        assert_eq!(
            app.document_lifecycle().unwrap().phase,
            LifecyclePhase::Interactive
        );
        assert_eq!(
            app.initial_script_outcome().status,
            InitialScriptStatus::Failed
        );
        assert_eq!(
            app.initial_script_outcome().failure,
            Some(InitialScriptFailure::Infrastructure)
        );
        assert_eq!(app.initial_script_outcome().error_count, 1);
        assert!(app.cdp_events.iter().all(|event| !matches!(
            event,
            cdp_browser::Event::DomContentLoaded { .. } | cdp_browser::Event::LoadComplete { .. }
        )));
        assert_eq!(
            state.lock().unwrap().charges,
            if kind == "opaque" { 1 } else { 2 }
        );
        assert!(state.lock().unwrap().canceled);
    }
}

#[test]
fn accepted_fatal_prefix_is_script_failure_not_transport_error_or_load_success() {
    let (mut app, state, _) = committed_lifecycle(&[], 0);
    let epoch = app.dom_epoch;
    let mut fatal = lifecycle_reply(&app, LifecyclePhase::Interactive, true, 1, 0, None);
    fatal.boa.as_mut().unwrap().jobs_admitted = 2; // Canceled load is not refunded.
    fatal.boa.as_mut().unwrap().fatal_reason = Some("Owned terminal task".into());
    fatal.state = RealmState::Fatal;
    fatal.errors.push("Owned terminal task".into());
    complete(&app, &state, fatal);
    app.poll();
    assert!(!app.is_loading());
    assert_eq!(app.dom_epoch, epoch);
    assert_eq!(
        app.initial_script_outcome().status,
        InitialScriptStatus::Failed
    );
    assert_eq!(
        app.initial_script_outcome().failure,
        Some(InitialScriptFailure::Script)
    );
    assert_eq!(app.initial_script_outcome().error_count, 1);
    assert_eq!(
        app.document_lifecycle().unwrap().phase,
        LifecyclePhase::Interactive
    );
    assert_eq!(state.lock().unwrap().charges, 2);
    assert!(app.cdp_events.iter().all(|event| !matches!(
        event,
        cdp_browser::Event::DomContentLoaded { .. } | cdp_browser::Event::LoadComplete { .. }
    )));
}

#[test]
fn ordinary_error_on_load_transition_latches_before_completion() {
    let (mut app, state, _) = committed_lifecycle(&[], 0);
    let dcl = lifecycle_reply(
        &app,
        LifecyclePhase::DomContentLoaded,
        false,
        1,
        1,
        Some(TaskWake {
            token: 2,
            delay_ms: 0,
        }),
    );
    complete(&app, &state, dcl);
    app.poll();
    let mut load = lifecycle_reply(&app, LifecyclePhase::LoadComplete, true, 2, 0, None);
    load.errors.push("Owned load handler error".into());
    complete(&app, &state, load);
    app.poll();
    assert!(!app.is_loading());
    assert_eq!(
        app.initial_script_outcome().status,
        InitialScriptStatus::Partial
    );
    assert_eq!(
        app.initial_script_outcome().failure,
        Some(InitialScriptFailure::Script)
    );
    assert_eq!(app.initial_script_outcome().error_count, 1);
    assert_eq!(
        app.document_lifecycle().unwrap().phase,
        LifecyclePhase::LoadComplete
    );
    assert_eq!(
        app.cdp_events
            .iter()
            .filter(|event| matches!(event, cdp_browser::Event::LoadComplete { .. }))
            .count(),
        1
    );
}

#[test]
fn cumulative_accounting_is_validated_before_publishing_load() {
    let (mut app, state, _) = committed_lifecycle(&[], 0);
    let dcl = lifecycle_reply(
        &app,
        LifecyclePhase::DomContentLoaded,
        false,
        1,
        1,
        Some(TaskWake {
            token: 2,
            delay_ms: 0,
        }),
    );
    complete(&app, &state, dcl);
    app.poll();
    let epoch = app.dom_epoch;
    let invalid = lifecycle_reply(&app, LifecyclePhase::LoadComplete, false, 1, 0, None);
    assert!(
        invalid.accounting_valid(),
        "Individually valid is not cumulatively valid"
    );
    complete(&app, &state, invalid);
    app.poll();
    assert!(!app.is_loading());
    assert_eq!(app.dom_epoch, epoch);
    assert_eq!(
        app.document_lifecycle().unwrap().phase,
        LifecyclePhase::DomContentLoaded
    );
    assert_eq!(
        app.initial_script_outcome().failure,
        Some(InitialScriptFailure::Infrastructure)
    );
    assert_eq!(state.lock().unwrap().charges, 3);
    assert!(
        app.cdp_events
            .iter()
            .all(|event| !matches!(event, cdp_browser::Event::LoadComplete { .. }))
    );
}

#[test]
fn hidden_committed_pages_pump_initial_lifecycle_without_painting() {
    let (mut first, a, _) = committed_lifecycle(&[], 0);
    let (mut second, b, _) = committed_lifecycle(&[], 0);
    for (app, state) in [(&mut first, &a), (&mut second, &b)] {
        let dcl = lifecycle_reply(
            app,
            LifecyclePhase::DomContentLoaded,
            false,
            1,
            1,
            Some(TaskWake {
                token: 2,
                delay_ms: 0,
            }),
        );
        complete(app, state, dcl);
        app.poll();
        let load = lifecycle_reply(app, LifecyclePhase::LoadComplete, false, 2, 0, None);
        complete(app, state, load);
        app.poll();
        assert!(!app.is_loading());
        assert_eq!(
            app.initial_script_outcome().status,
            InitialScriptStatus::Complete
        );
        assert_eq!(state.lock().unwrap().charges, 3);
        assert_eq!(app.performance_profile().frames_painted, 0);
    }
}

struct RejectedGeneration;
impl scripts::ScriptRuntime for RejectedGeneration {
    fn set_generation(&self, _: u64) -> Result<(), String> {
        Err("Owned child-generation admission failure".into())
    }
    fn start(
        &self,
        _: mg_sparkle::js_browser::Request,
        _: u64,
    ) -> Result<(Box<dyn ScriptSession>, SessionReply), String> {
        panic!("Generation failure must not start a worker or fetch job")
    }
    fn close(&self) {}
    fn wait_idle(&self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn generation_admission_failure_and_supersession_stop_each_navigation_once() {
    let (mut app, state, _) = committed_lifecycle(&[], 0);
    let previous = app.generation;
    app.child_pool = Arc::new(RejectedGeneration);
    app.navigate("https://must-not-fetch.invalid/".into(), None, false);
    assert!(!app.is_loading());
    assert!(!app.fetching);
    assert_eq!(app.inflight, 0);
    assert_eq!(app.generation, previous + 1);
    assert!(state.lock().unwrap().canceled);
    assert!(app.document_lifecycle().is_none());
    assert_eq!(
        app.initial_script_outcome().status,
        InitialScriptStatus::Failed
    );
    assert_eq!(
        app.initial_script_outcome().failure,
        Some(InitialScriptFailure::Infrastructure)
    );
    let stops: Vec<_> = app
        .cdp_events
        .iter()
        .filter_map(|event| {
            if let cdp_browser::Event::Finished { generation, error } = event {
                Some((*generation, error.as_deref().unwrap()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(stops.len(), 2);
    assert_eq!(stops[0].0, previous);
    assert!(stops[0].1.contains("superseded"));
    assert_eq!(stops[1].0, previous + 1);
    assert!(stops[1].1.contains("child-generation"));
    assert!(app.cdp_events.iter().all(|event| !matches!(
        event,
        cdp_browser::Event::DomContentLoaded { .. } | cdp_browser::Event::LoadComplete { .. }
    )));
    app.poll();
    assert_eq!(
        app.cdp_events
            .iter()
            .filter(|event| matches!(event, cdp_browser::Event::Finished { .. }))
            .count(),
        2
    );
}

// These additive controls exercise parent receipt handling, not worker dispatch.
// FakeSession deliberately cannot dispatch clicks: seed the already-owned
// Activation identity and feed its reply through the normal acceptance path.
mod link_default_receipts {
    use super::*;
    use mg_sparkle::page_session::observer::*;

    const PAGE: &str = "http://127.0.0.1:9/original";
    const EXPLICIT: &str = "http://127.0.0.1:9/explicit";

    struct ReceiptRuntime(ObserverExpectation);
    impl scripts::ScriptRuntime for ReceiptRuntime {
        fn observer_expectation(&self) -> ObserverExpectation {
            self.0
        }
        fn set_generation(&self, _: u64) -> Result<(), String> {
            panic!("Receipt-only controls must not enter navigation admission")
        }
        fn start(
            &self,
            _: mg_sparkle::js_browser::Request,
            _: u64,
        ) -> Result<(Box<dyn ScriptSession>, SessionReply), String> {
            panic!("Receipt-only controls must not start a worker")
        }
        fn close(&self) {}
        fn wait_idle(&self) -> Result<(), String> {
            Ok(())
        }
    }

    fn activation() -> (Browser, Arc<Mutex<State>>, usize, usize) {
        let (mut app, state, _) = fixture();
        app.page_url = PAGE.into();
        app.address = PAGE.into();
        app.document = document::parse(
            "<head><title>Retained</title><base id=base href='/base/'></head>\
             <body><form><input id=value name=value value=old></form>\
             <a id=link href='/accepted'>Link</a><a id=other href='/other'>Other</a>\
             <section id=destination></section></body>",
            PAGE,
        );
        let node = app.document.query_selector(0, "#value").unwrap().unwrap();
        let link = app.document.query_selector(0, "#link").unwrap().unwrap();
        assert!(app.input_item(node).is_some());
        app.values.insert(node, "native edit seven".into());
        app.edit_versions.insert(node, 7);
        app.focus = Focus::Input(node);
        app.select_all = true;
        app.dom_epoch = 23;
        app.cdp_enabled = true;
        app.task_wake = None;
        app.child_pool = Arc::new(ReceiptRuntime(ObserverExpectation::WindowHarness {
            run_id: 1,
        }));
        app.observer_ledger =
            Some(ObserverLedger::new(app.child_pool.observer_expectation()).unwrap());
        app.pending_transaction = Some(PendingTransaction::Activation {
            identity: TransactionIdentity {
                generation: 42,
                session_id: 91,
                sequence: 1,
                revision: 0,
            },
            input: SessionInput {
                kind: InputKind::Click { target: link },
                edits: vec![ControlEdit {
                    node,
                    version: 7,
                    value: "native edit seven".into(),
                }],
            },
        });
        (app, state, node, link)
    }

    fn candidate(app: &Browser, control: usize, link: usize) -> SessionReply {
        let mut response = reply(app, None);
        let title = app.document.query_selector(0, "title").unwrap().unwrap();
        let title_text = app.document.nodes[title].children[0];
        let nodes = &mut response.snapshot.as_mut().unwrap().nodes;
        nodes[title_text].text = "Candidate".into();
        nodes[control]
            .attributes
            .iter_mut()
            .find(|(key, _)| key == "value")
            .unwrap()
            .1 = "worker edit seven".into();
        response.acknowledgements = vec![ControlAck {
            node: control,
            version: 7,
        }];
        response.outcome.click_canceled = Some(false);
        response.default_action = DefaultAction::FollowLink { node: link };
        response.observer = Some(ObserverReply::WindowHarness {
            run_id: 1,
            first_ordinal: 0,
            next_ordinal: 1,
            records: vec![ObserverRecord::Result {
                test: TestRecord {
                    id: 1,
                    index: 0,
                    name: Utf16::from_units(vec![0x6c, 0x69, 0x6e, 0x6b]).unwrap(),
                    status: SubtestStatus::Pass,
                    message: OptionalUtf16::Null,
                    stack: OptionalUtf16::Null,
                },
            }],
            progress: ObserverProgress::Waiting,
            execution_failed: false,
        });
        assert!(response.accounting_valid());
        response
    }

    fn attribute(nodes: &mut [document::Node], node: usize, name: &str, value: &str) {
        let pair = nodes[node]
            .attributes
            .iter_mut()
            .find(|(key, _)| key == name)
            .unwrap();
        pair.1 = value.into();
    }

    fn detach(nodes: &mut [document::Node], node: usize) {
        let parent = nodes[node].parent;
        nodes[parent].children.retain(|child| *child != node);
        nodes[node].parent = node;
    }

    fn disabled_observer(app: &mut Browser, response: &mut SessionReply) {
        // Ordinary browsing allows explicit navigation. Trusted WindowHarness
        // intentionally rejects it; do not use that incompatible mode here.
        app.child_pool = Arc::new(ReceiptRuntime(ObserverExpectation::Disabled));
        app.observer_ledger = Some(ObserverLedger::new(ObserverExpectation::Disabled).unwrap());
        response.observer = Some(ObserverReply::Disabled);
    }

    fn retained(
        app: &mut Browser,
        state: &Arc<Mutex<State>>,
        node: usize,
        before: &[document::Node],
        failure: ObserverParentFailure,
        charges: usize,
        copied_units: usize,
    ) {
        assert_eq!(app.document.nodes, before);
        assert_eq!(app.document.title, "Retained");
        assert_eq!(app.values[&node], "native edit seven");
        assert_eq!(app.edit_versions[&node], 7);
        assert!(app.focus == Focus::Input(node));
        assert!(app.select_all);
        assert_eq!(app.page_revision, 0);
        assert_eq!(app.dom_epoch, 23);
        assert_eq!(app.page_url, PAGE);
        assert_eq!(app.address, PAGE);
        assert_eq!(app.generation, 42);
        assert_eq!(app.inflight, 0);
        assert!(!app.loading && !app.fetching);
        assert!(app.cdp_events.is_empty());
        let view = app.script_observation().unwrap().observation();
        assert_eq!(view.cursor, 0);
        assert!(view.records.is_empty());
        assert!(view.reconciliations.is_empty());
        assert_eq!(view.copied_units, copied_units);
        assert_eq!(view.progress, None);
        assert_eq!(view.fault, None);
        assert!(!view.execution_failed);
        assert_eq!(view.parent_failure, Some(failure));
        assert!(app.script_blocked);
        assert!(app.pending_transaction.is_none());
        assert!(app.task_wake.is_none());
        assert!(state.lock().unwrap().canceled);
        assert_eq!(state.lock().unwrap().charges, charges);
        assert!(!state.lock().unwrap().validation_pending);
        assert!(state.lock().unwrap().pumps.is_empty());
        app.poll_page_session();
        assert_eq!(state.lock().unwrap().charges, charges);
        assert_eq!(app.document.nodes, before);
        assert_eq!(app.dom_epoch, 23);
        assert!(app.cdp_events.is_empty());
    }

    #[test]
    fn eligibility_is_independent_of_text_projection() {
        for body in ["Link", "", " \n\t ", "<span></span>", "<img alt=''>"] {
            let document =
                document::parse(&format!("<a id=link href=' /selected '> {body}</a>"), PAGE);
            let link = document.query_selector(0, "#link").unwrap().unwrap();
            let has_text = document
                .items
                .iter()
                .zip(&document.item_nodes)
                .any(|(item, node)| {
                    *node == link && matches!(item, Item::Text { href: Some(_), .. })
                });
            assert_eq!(has_text, body == "Link", "fixture text premise: {body:?}");
            assert!(
                Browser::validate_default(
                    &document,
                    &InputKind::Click { target: link },
                    &DefaultAction::FollowLink { node: link },
                )
                .is_ok(),
                "present own href on connected clicked anchor: {body:?}"
            );
        }
    }

    #[test]
    fn eligibility_uses_the_same_final_connected_anchor_after_handler_changes() {
        for change in ["text-removed", "hidden", "reparented", "detach-reinsert"] {
            let (app, _, _, link) = activation();
            let mut nodes = app.document.nodes.clone();
            match change {
                "text-removed" => {
                    for child in nodes[link].children.clone() {
                        detach(&mut nodes, child);
                    }
                }
                "hidden" => nodes[link]
                    .attributes
                    .push(("hidden".into(), String::new())),
                "reparented" | "detach-reinsert" => {
                    let original = nodes[link].parent;
                    let destination = if change == "reparented" {
                        app.document
                            .query_selector(0, "#destination")
                            .unwrap()
                            .unwrap()
                    } else {
                        original
                    };
                    detach(&mut nodes, link);
                    nodes[destination].children.push(link);
                    nodes[link].parent = destination;
                }
                _ => unreachable!(),
            }
            attribute(&mut nodes, link, "href", "");
            let document = document::project_nodes(nodes, PAGE).unwrap();
            assert!(Browser::connected(&document, link));
            assert_eq!(document.nodes[link].attr("href"), Some(""));
            assert!(
                Browser::validate_default(
                    &document,
                    &InputKind::Click { target: link },
                    &DefaultAction::FollowLink { node: link },
                )
                .is_ok(),
                "{change}"
            );
        }
    }

    #[test]
    fn relationship_validation_rejects_unrelated_or_missing_anchor_authority() {
        for change in [
            "other-anchor",
            "text-node",
            "non-anchor",
            "detached",
            "missing-href",
            "invalid-href",
            "javascript",
            "replacement-id",
            "out-of-range",
            "submit-input",
        ] {
            let (app, _, control, link) = activation();
            let mut nodes = app.document.nodes.clone();
            let mut default_node = link;
            let mut input = InputKind::Click { target: link };
            match change {
                "other-anchor" => {
                    default_node = app.document.query_selector(0, "#other").unwrap().unwrap()
                }
                "text-node" => default_node = nodes[link].children[0],
                "non-anchor" => {
                    default_node = control;
                    input = InputKind::Click { target: control };
                    nodes[control]
                        .attributes
                        .push(("href".into(), "/selected".into()));
                }
                "detached" => detach(&mut nodes, link),
                "missing-href" => nodes[link].attributes.retain(|(key, _)| key != "href"),
                "invalid-href" => attribute(&mut nodes, link, "href", "http://["),
                "javascript" => attribute(&mut nodes, link, "href", "javascript:trap()"),
                "replacement-id" => {
                    let parent = nodes[link].parent;
                    detach(&mut nodes, link);
                    default_node = nodes.len();
                    nodes.push(document::Node {
                        tag: "a".into(),
                        attributes: vec![
                            ("id".into(), "link".into()),
                            ("href".into(), "/selected".into()),
                        ],
                        text: String::new(),
                        parent,
                        children: vec![],
                    });
                    nodes[parent].children.push(default_node);
                }
                "out-of-range" => default_node = nodes.len(),
                "submit-input" => {
                    input = InputKind::Submit {
                        form: app.document.form_nodes[0],
                        submitter: None,
                    }
                }
                _ => unreachable!(),
            }
            let document = document::project_nodes(nodes, PAGE).unwrap();
            assert!(
                Browser::validate_default(
                    &document,
                    &input,
                    &DefaultAction::FollowLink { node: default_node }
                )
                .is_err(),
                "{change}"
            );
        }
    }

    #[test]
    fn selected_forbidden_link_is_rejected_before_dom_ack_epoch_or_observer_commit() {
        for target in [
            "ftp",
            "username",
            "password",
            "data",
            "over-limit",
            "utf8-expansion",
        ] {
            let (mut app, state, control, link) = activation();
            let before = app.document.nodes.clone();
            let mut response = candidate(&app, control, link);
            let nodes = &mut response.snapshot.as_mut().unwrap().nodes;
            let href = match target {
                "ftp" => "ftp://127.0.0.1/file".into(),
                "username" => "http://user@127.0.0.1:9/file".into(),
                "password" => "http://:secret@127.0.0.1:9/file".into(),
                "data" => "data:text/plain,not-http".into(),
                "over-limit" => {
                    let base = app.document.query_selector(0, "#base").unwrap().unwrap();
                    // Admitted raw base is exactly 16,384 bytes; joining the
                    // one-byte relative href makes a 16,385-byte selected URL.
                    let value = format!("http://127.0.0.1:9/{}/", "a".repeat(16_364));
                    assert_eq!(value.len(), 16_384);
                    attribute(nodes, base, "href", &value);
                    "b".into()
                }
                "utf8-expansion" => format!("http://127.0.0.1:9/{}", "é".repeat(3_000)),
                _ => unreachable!(),
            };
            assert!(href.len() <= 16_384);
            attribute(nodes, link, "href", &href);
            let projected = document::project_nodes(nodes.clone(), PAGE).unwrap();
            // This text-bearing control is eligible even on old production:
            // the targeted failure is the selected policy check after commit.
            assert!(
                Browser::validate_default(
                    &projected,
                    &InputKind::Click { target: link },
                    &response.default_action
                )
                .is_ok()
            );
            let resolved = projected
                .items
                .iter()
                .zip(&projected.item_nodes)
                .find_map(|(item, node)| {
                    if *node == link {
                        if let Item::Text {
                            href: Some(url), ..
                        } = item
                        {
                            return Some(url);
                        }
                    }
                    None
                })
                .unwrap();
            if target == "over-limit" {
                assert_eq!(resolved.len(), 16_385);
            }
            if target == "utf8-expansion" {
                assert_eq!(resolved.len(), 18_019);
            }
            assert!(Browser::checked_navigation(resolved).is_err(), "{target}");
            complete(&app, &state, response);
            app.poll_page_session();
            retained(
                &mut app,
                &state,
                control,
                &before,
                ObserverParentFailure::Validation,
                1,
                0,
            );
        }
    }

    #[test]
    fn failed_single_charge_retains_link_projection_and_observer_rows() {
        for empty in [false, true] {
            let (mut app, state, control, link) = activation();
            let before = app.document.nodes.clone();
            let mut response = candidate(&app, control, link);
            if empty {
                let nodes = &mut response.snapshot.as_mut().unwrap().nodes;
                for child in nodes[link].children.clone() {
                    detach(nodes, child);
                }
            }
            state.lock().unwrap().fail_charge = true;
            complete(&app, &state, response);
            app.poll_page_session();
            retained(
                &mut app,
                &state,
                control,
                &before,
                ObserverParentFailure::ActiveCharge,
                1,
                // Staging copies the four UTF-16 name units even when the
                // charge fails. No cumulative admission is refunded.
                4,
            );
        }
    }

    #[test]
    fn ready_none_commits_cancellation_detachment_and_href_removal_without_navigation() {
        for suppression in ["canceled", "detached", "href-removed"] {
            for newer_edit in [false, true] {
                let (mut app, state, control, link) = activation();
                let mut response = candidate(&app, control, link);
                response.default_action = DefaultAction::None;
                match suppression {
                    "canceled" => response.outcome.click_canceled = Some(true),
                    "detached" => detach(&mut response.snapshot.as_mut().unwrap().nodes, link),
                    "href-removed" => response.snapshot.as_mut().unwrap().nodes[link]
                        .attributes
                        .retain(|(key, _)| key != "href"),
                    _ => unreachable!(),
                }
                let accepted_nodes = response.snapshot.as_ref().unwrap().nodes.clone();
                if newer_edit {
                    app.values.insert(control, "native edit eight".into());
                    app.edit_versions.insert(control, 8);
                }
                complete(&app, &state, response);
                app.poll_page_session();
                assert_eq!(app.document.nodes, accepted_nodes);
                assert_eq!(app.document.title, "Candidate");
                if newer_edit {
                    assert_eq!(app.values[&control], "native edit eight");
                    assert_eq!(app.edit_versions[&control], 8);
                } else {
                    assert!(!app.values.contains_key(&control));
                    assert!(!app.edit_versions.contains_key(&control));
                }
                assert_eq!(
                    app.document.nodes[control].attr("value"),
                    Some("worker edit seven")
                );
                assert!(app.focus == Focus::Input(control));
                assert!(app.select_all);
                assert_eq!(app.page_revision, 1);
                assert_eq!(app.dom_epoch, 24);
                assert_eq!(app.page_url, PAGE);
                assert_eq!(app.address, PAGE);
                assert_eq!(app.generation, 42);
                assert_eq!(app.inflight, 0);
                assert!(!app.loading && !app.fetching && !app.script_blocked);
                assert!(matches!(
                    app.cdp_events.as_slice(),
                    [cdp_browser::Event::DocumentUpdated]
                ));
                let view = app.script_observation().unwrap().observation();
                assert_eq!(view.cursor, 1);
                assert_eq!(view.records.len(), 1);
                assert_eq!(view.parent_failure, None);
                assert_eq!(view.progress, Some(ObserverProgress::Waiting));
                assert_eq!(state.lock().unwrap().charges, 1);
                assert!(!state.lock().unwrap().canceled);
                assert!(!state.lock().unwrap().validation_pending);
                assert!(app.pending_transaction.is_none());
                app.poll_page_session();
                assert_eq!(state.lock().unwrap().charges, 1);
                assert_eq!(app.dom_epoch, 24);
                assert_eq!(app.cdp_events.len(), 1);
            }
        }
    }

    #[test]
    fn activation_envelope_rejections_preserve_all_parent_state() {
        for field in [
            "generation",
            "session",
            "sequence",
            "revision",
            "missing-ack",
            "wrong-ack",
            "graph",
            "tag",
            "arena",
        ] {
            let (mut app, state, control, link) = activation();
            let before = app.document.nodes.clone();
            let mut response = candidate(&app, control, link);
            response.default_action = DefaultAction::None;
            match field {
                "revision" => response.revision = 2,
                "missing-ack" => response.acknowledgements.clear(),
                "wrong-ack" => response.acknowledgements[0].version = 8,
                "graph" => response.snapshot.as_mut().unwrap().nodes[0]
                    .children
                    .push(usize::MAX),
                "tag" => response.snapshot.as_mut().unwrap().nodes[link].tag = "span".into(),
                "arena" => {
                    response.snapshot.as_mut().unwrap().nodes.pop();
                }
                _ => {}
            }
            let mut update = SessionUpdate {
                generation: 42,
                session_id: 91,
                sequence: Some(1),
                reply: Ok(response),
                automation: None,
                continuation: scripts::SessionContinuation::Live,
            };
            match field {
                "generation" => update.generation = 43,
                "session" => update.session_id = 92,
                "sequence" => update.sequence = Some(2),
                _ => {}
            }
            state.lock().unwrap().updates.push_back(update);
            app.poll_page_session();
            retained(
                &mut app,
                &state,
                control,
                &before,
                ObserverParentFailure::Validation,
                1,
                0,
            );
        }
    }

    #[test]
    fn canceled_or_terminal_follow_link_never_reaches_default_selection() {
        for condition in ["click-canceled", "submit-canceled", "fatal", "closed"] {
            let (app, _, control, link) = activation();
            let mut response = candidate(&app, control, link);
            match condition {
                "click-canceled" => response.outcome.click_canceled = Some(true),
                "submit-canceled" => response.outcome.submit_canceled = Some(true),
                "fatal" => response.state = RealmState::Fatal,
                "closed" => response.state = RealmState::Closed,
                _ => unreachable!(),
            }
            assert!(response.accounting_valid());
            assert!(
                Browser::project_reply(&mut response, PAGE, Some(&app.document)).is_err(),
                "{condition}"
            );
        }
    }

    #[test]
    fn explicit_navigation_preserves_eligible_suppressed_defaults_and_optional_outcome() {
        for empty in [false, true] {
            for canceled in [None, Some(false)] {
                for target in [
                    "ftp://127.0.0.1/file".to_string(),
                    "http://user@127.0.0.1:9/file".into(),
                    format!("http://127.0.0.1:9/{}", "é".repeat(3_000)),
                ] {
                    let (mut app, state, control, link) = activation();
                    // The existing automatic-hop limit intentionally stops the
                    // explicit destination before navigate_inner. This proves
                    // precedence/acceptance, not a successful network request.
                    app.refresh_count = 4;
                    let mut response = candidate(&app, control, link);
                    disabled_observer(&mut app, &mut response);
                    response.navigation = Some(EXPLICIT.into());
                    response.outcome.click_canceled = canceled;
                    let nodes = &mut response.snapshot.as_mut().unwrap().nodes;
                    attribute(nodes, link, "href", &target);
                    if empty {
                        for child in nodes[link].children.clone() {
                            detach(nodes, child);
                        }
                    }
                    let accepted_nodes = nodes.clone();
                    complete(&app, &state, response);
                    app.poll_page_session();
                    assert_eq!(app.document.nodes, accepted_nodes);
                    assert_eq!(app.document.title, "Candidate");
                    assert_eq!(app.page_revision, 1);
                    assert_eq!(app.dom_epoch, 24);
                    assert!(!app.edit_versions.contains_key(&control));
                    assert!(!app.values.contains_key(&control));
                    assert!(app.focus == Focus::Input(control));
                    assert_eq!(app.generation, 42);
                    assert_eq!(app.page_url, PAGE);
                    assert_eq!(app.address, PAGE);
                    assert_eq!(app.inflight, 0);
                    assert!(!app.loading && !app.fetching);
                    assert_eq!(app.refresh_count, 4);
                    assert!(app.status.contains("Automatic navigation limit reached"));
                    assert!(app.script_blocked);
                    assert!(matches!(
                        app.cdp_events.as_slice(),
                        [cdp_browser::Event::DocumentUpdated]
                    ));
                    let view = app.script_observation().unwrap().observation();
                    assert_eq!(view.cursor, 0);
                    assert!(view.records.is_empty());
                    assert_eq!(view.copied_units, 0);
                    assert_eq!(view.progress, None);
                    assert_eq!(view.parent_failure, None);
                    assert_eq!(state.lock().unwrap().charges, 1);
                    assert!(state.lock().unwrap().canceled);
                    app.poll_page_session();
                    assert_eq!(state.lock().unwrap().charges, 1);
                }
            }
        }
    }

    #[test]
    fn explicit_navigation_does_not_excuse_forged_defaults_or_invalid_explicit_urls() {
        for invalid in [
            "unrelated",
            "detached",
            "missing-href",
            "invalid-href",
            "javascript",
            "explicit-ftp",
            "explicit-credentials",
        ] {
            let (mut app, state, control, link) = activation();
            let before = app.document.nodes.clone();
            app.refresh_count = 4;
            let mut response = candidate(&app, control, link);
            disabled_observer(&mut app, &mut response);
            response.navigation = Some(EXPLICIT.into());
            let nodes = &mut response.snapshot.as_mut().unwrap().nodes;
            match invalid {
                "unrelated" => {
                    response.default_action = DefaultAction::FollowLink {
                        node: app.document.query_selector(0, "#other").unwrap().unwrap(),
                    }
                }
                "detached" => detach(nodes, link),
                "missing-href" => nodes[link].attributes.retain(|(key, _)| key != "href"),
                "invalid-href" => attribute(nodes, link, "href", "http://["),
                "javascript" => attribute(nodes, link, "href", "javascript:trap()"),
                "explicit-ftp" => response.navigation = Some("ftp://127.0.0.1/file".into()),
                "explicit-credentials" => {
                    response.navigation = Some("http://user@127.0.0.1:9/file".into())
                }
                _ => unreachable!(),
            }
            complete(&app, &state, response);
            app.poll_page_session();
            retained(
                &mut app,
                &state,
                control,
                &before,
                ObserverParentFailure::Validation,
                1,
                0,
            );
        }
    }

    #[test]
    fn activation_transport_error_has_no_projection_charge_or_commit() {
        let (mut app, state, control, _) = activation();
        let before = app.document.nodes.clone();
        state.lock().unwrap().updates.push_back(SessionUpdate {
            generation: 42,
            session_id: 91,
            sequence: Some(1),
            reply: Err("Owned activation transport failure".into()),
            automation: None,
            continuation: scripts::SessionContinuation::Live,
        });
        app.poll_page_session();
        retained(
            &mut app,
            &state,
            control,
            &before,
            ObserverParentFailure::Transport,
            0,
            0,
        );
    }
}
mod staged_link_destination_literals {
    use super::*;

    const PAGE: &str = "https://links.invalid/root/page";
    const PREFIX: &str = "http://127.0.0.1:9/";

    fn attr(nodes: &mut [document::Node], id: usize, key: &str, value: &str) {
        nodes[id]
            .attributes
            .iter_mut()
            .find(|(name, _)| name == key)
            .unwrap()
            .1 = value.into();
    }

    fn page(href: &str, base: Option<&str>) -> (Document, usize) {
        let mut doc = document::parse(
            "<head><base id=base href=''></head><body>\
             <a id=link href=''><span id=child></span></a>\
             <span id=non-anchor href='/selected'></span></body>",
            PAGE,
        );
        let link = doc.query_selector(0, "#link").unwrap().unwrap();
        let base_id = doc.query_selector(0, "#base").unwrap().unwrap();
        attr(&mut doc.nodes, link, "href", href);
        if let Some(base) = base {
            attr(&mut doc.nodes, base_id, "href", base);
        } else {
            doc.nodes[base_id]
                .attributes
                .retain(|(key, _)| key != "href");
        }
        (document::project_nodes(doc.nodes, PAGE).unwrap(), link)
    }

    fn resolved(doc: &Document, link: usize) -> String {
        let staged = Browser::validate_default(
            doc,
            &InputKind::Click { target: link },
            &DefaultAction::FollowLink { node: link },
        )
        .unwrap();
        match staged {
            StagedDefault::FollowLink(url) => url,
            _ => panic!("Expected owned exact-anchor destination"),
        }
    }

    #[test]
    fn exact_own_href_trim_empty_and_identity_literals() {
        for (href, expected) in [
            (
                " \t../next?q=1#frag \r\n",
                "https://links.invalid/next?q=1#frag",
            ),
            ("", PAGE),
            ("./é x", "https://links.invalid/root/%C3%A9%20x"),
            (
                "https://other.invalid/absolute",
                "https://other.invalid/absolute",
            ),
        ] {
            let (doc, link) = page(href, None);
            assert_eq!(doc.anchor_href(link).as_deref(), Some(expected));
            assert_eq!(resolved(&doc, link), expected);
            assert_eq!(Browser::checked_navigation(expected).unwrap(), expected);
            for selector in ["#child", "#non-anchor"] {
                let id = doc.query_selector(0, selector).unwrap().unwrap();
                assert_eq!(doc.anchor_href(id), None);
                assert!(
                    Browser::validate_default(
                        &doc,
                        &InputKind::Click { target: id },
                        &DefaultAction::FollowLink { node: id },
                    )
                    .is_err()
                );
            }
            assert_eq!(doc.anchor_href(doc.nodes.len()), None);
        }
        let (mut doc, link) = page("", None);
        doc.nodes[link].attributes.retain(|(key, _)| key != "href");
        let doc = document::project_nodes(doc.nodes, PAGE).unwrap();
        assert_eq!(doc.anchor_href(link), None);
        assert!(
            Browser::validate_default(
                &doc,
                &InputKind::Click { target: link },
                &DefaultAction::FollowLink { node: link },
            )
            .is_err()
        );
    }

    #[test]
    fn exact_destination_uses_reprojected_connected_base_order() {
        for change in ["original", "reverse", "remove-href", "detach-both"] {
            let mut doc = document::parse(
                "<head><base id=first href='/one/'><base id=second href='/two/'></head>\
                 <body><a id=link href='chosen'></a></body>",
                PAGE,
            );
            let first = doc.query_selector(0, "#first").unwrap().unwrap();
            let second = doc.query_selector(0, "#second").unwrap().unwrap();
            let link = doc.query_selector(0, "#link").unwrap().unwrap();
            let parent = doc.nodes[first].parent;
            let expected = match change {
                "original" => "https://links.invalid/one/chosen",
                "reverse" => {
                    doc.nodes[parent].children.reverse();
                    "https://links.invalid/two/chosen"
                }
                "remove-href" => {
                    doc.nodes[first].attributes.retain(|(key, _)| key != "href");
                    "https://links.invalid/two/chosen"
                }
                "detach-both" => {
                    doc.nodes[parent]
                        .children
                        .retain(|id| *id != first && *id != second);
                    doc.nodes[first].parent = first;
                    doc.nodes[second].parent = second;
                    "https://links.invalid/root/chosen"
                }
                _ => unreachable!(),
            };
            let doc = document::project_nodes(doc.nodes, PAGE).unwrap();
            assert_eq!(resolved(&doc, link), expected, "{change}");
        }
    }

    #[test]
    fn selected_target_boundary_is_serialized_16384_not_raw_reference_length() {
        assert_eq!(PREFIX.len(), 19);
        let exact = format!("{PREFIX}{}", "a".repeat(16_365));
        assert_eq!(exact.len(), 16_384);
        let (doc, link) = page(&exact, None);
        assert_eq!(resolved(&doc, link), exact);
        assert_eq!(Browser::checked_navigation(&exact).unwrap(), exact);

        let base = format!("{PREFIX}{}/", "a".repeat(16_364));
        assert_eq!(base.len(), 16_384);
        let expected = format!("{base}b");
        assert_eq!(expected.len(), 16_385);
        let (doc, link) = page("b", Some(&base));
        assert_eq!(resolved(&doc, link), expected);
        assert!(Browser::checked_navigation(&expected).is_err());
    }

    #[test]
    fn admitted_long_trim_and_expanded_base_do_not_reject_short_destination() {
        let raw_href = format!("{}/ok", " ".repeat(16_381));
        assert_eq!(raw_href.len(), 16_384);
        let (doc, link) = page(&raw_href, Some(PREFIX));
        let expected = "http://127.0.0.1:9/ok";
        assert_eq!(resolved(&doc, link), expected);
        assert_eq!(Browser::checked_navigation(expected).unwrap(), expected);

        let raw_base = format!("{PREFIX}{}/", "é".repeat(3_000));
        assert_eq!(raw_base.len(), 6_020);
        let expected_base = format!("{PREFIX}{}/", "%C3%A9".repeat(3_000));
        assert_eq!(expected_base.len(), 18_020);
        let expected = "https://other.invalid/ok";
        let (doc, link) = page(expected, Some(&raw_base));
        assert_eq!(doc.base_url, expected_base);
        assert_eq!(resolved(&doc, link), expected);
        assert_eq!(Browser::checked_navigation(expected).unwrap(), expected);
    }

    #[test]
    fn utf8_expansion_is_returned_then_rejected_only_by_selected_url_policy() {
        let raw = format!("{PREFIX}{}", "é".repeat(3_000));
        assert_eq!(raw.len(), 6_019);
        let expected = format!("{PREFIX}{}", "%C3%A9".repeat(3_000));
        assert_eq!(expected.len(), 18_019);
        let (doc, link) = page(&raw, None);
        assert_eq!(doc.anchor_href(link).as_deref(), Some(expected.as_str()));
        assert_eq!(resolved(&doc, link), expected);
        assert!(Browser::checked_navigation(&expected).is_err());
    }
}
