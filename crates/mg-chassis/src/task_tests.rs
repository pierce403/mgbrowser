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
    app.inflight = 1;
    app.tx
        .send(Loaded {
            generation: app.generation - 1,
            document: None,
            result: Ok(net::Response {
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
