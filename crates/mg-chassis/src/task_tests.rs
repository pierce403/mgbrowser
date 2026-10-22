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
        Ok(())
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
    }
}
fn complete(app: &Browser, state: &Arc<Mutex<State>>, reply: SessionReply) {
    state.lock().unwrap().updates.push_back(SessionUpdate {
        generation: app.generation,
        session_id: 91,
        sequence: Some(app.page_revision + 1),
        reply: Ok(reply),
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
