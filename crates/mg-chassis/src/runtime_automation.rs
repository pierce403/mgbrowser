//! Parent-side admission/publication for the existing worker transaction slot.
//! Only the CDP broker issues tokens/owners; no JavaScript values live here.
use super::*;
use mg_sparkle::page_session::{
    automation::{MAX_EDITS_BYTES, Operation, PropertyDescriptor, RemoteValue},
    observer::ObserverExpectation,
};
use scripts::SessionContinuation;

pub(crate) struct AutomationCompletion {
    pub token: u64,
    pub result: Result<AutomationResult, String>,
    pub continuation: SessionContinuation,
}

/// Every nested handle matters, including exceptions and accessor functions.
/// The final transaction may return data, but cannot publish a dead VM's roots.
pub(super) fn has_handles(result: &AutomationResult) -> bool {
    fn object(value: &RemoteValue) -> bool {
        matches!(value, RemoteValue::Object { .. })
    }
    match result {
        AutomationResult::Value(value) => object(value),
        AutomationResult::Exception { exception, .. } => exception.as_ref().is_some_and(object),
        AutomationResult::Properties(properties) => {
            properties
                .iter()
                .any(|property| match &property.descriptor {
                    PropertyDescriptor::Data { value, .. } => object(value),
                    PropertyDescriptor::Accessor { get, set } => {
                        get.as_ref().is_some_and(object) || set.as_ref().is_some_and(object)
                    }
                })
        }
        AutomationResult::Released | AutomationResult::Rejected { .. } => false,
    }
}

impl Browser {
    pub(crate) fn runtime_identity(&self) -> Option<(u64, u64)> {
        if !self.cdp_enabled
            || (!self.scripts_enabled && !self.automation_only_session)
            || self.loading
            || self.fetching
            || self.script_blocked
            || self.child_pool.source_wire_mode() != scripts::SourceWireMode::Modern
            || self.child_pool.observer_expectation() != ObserverExpectation::Disabled
            || !self
                .lifecycle
                .is_some_and(|p| p.phase == LifecyclePhase::LoadComplete)
        {
            return None;
        }
        self.page_session
            .as_ref()
            .map(|session| (self.generation, session.id()))
    }

    pub(crate) fn dispatch_automation(
        &mut self,
        mut request: AutomationRequest,
        token: u64,
    ) -> Result<(), String> {
        if token == 0 || !request.edits.is_empty() {
            return Err("Invalid Runtime broker request".into());
        }
        let (generation, session_id) = self
            .runtime_identity()
            .ok_or("Runtime requires an enabled, loaded, usable page script realm")?;
        if self.pending_transaction.is_some()
            || self.automation_waiting.is_some()
            || self.automation_completion.is_some()
        {
            return Err("A page transaction or unconsumed Runtime result is pending".into());
        }
        request.validate()?;
        // Root release never runs JavaScript or synchronizes controls. Preserve
        // all native edits for the next operation that actually observes them.
        if !matches!(
            request.operation,
            Operation::ReleaseObject { .. }
                | Operation::ReleaseGroup { .. }
                | Operation::ReleaseOwners { .. }
        ) {
            let mut selected = Vec::new();
            let mut bytes = 0usize;
            for (&node, &version) in &self.edit_versions {
                if let Some(value) = self
                    .values
                    .get(&node)
                    .filter(|_| self.editable_value_item(node).is_some())
                {
                    bytes = bytes.saturating_add(value.len());
                    if selected.len() == 128
                        || bytes > MAX_EDITS_BYTES
                        || value.len() > MAX_EDIT_BYTES
                    {
                        return Err("Runtime control-edit admission exceeded".into());
                    }
                    selected.push((node, version, value));
                }
            }
            request.edits = selected
                .into_iter()
                .map(|(node, version, value)| ControlEdit {
                    node,
                    version,
                    value: value.clone(),
                })
                .collect();
            request.edits.sort_by_key(|edit| edit.node);
            request.validate()?;
        }
        let session = self
            .page_session
            .as_mut()
            .ok_or("Runtime worker is missing")?;
        let sequence = match session.try_automation(request.clone(), self.page_revision) {
            Ok(sequence) => sequence,
            Err(error) => {
                self.block_session(&error);
                return Err(error);
            }
        };
        self.automation_session = Some((generation, session_id));
        self.pending_transaction = Some(PendingTransaction::Automation {
            identity: TransactionIdentity {
                generation,
                session_id,
                sequence,
                revision: self.page_revision,
            },
            request,
        });
        self.automation_waiting = Some(token);
        self.task_wake = None;
        self.status = "Running DevTools Runtime transaction".into();
        self.dirty = true;
        Ok(())
    }

    /// Cleanup defers behind genuine page work; a busy realm is not expired.
    /// The broker must still handle admission failure after this check because
    /// the existing host deadline/budget may expire before queue submission.
    pub(crate) fn runtime_automation_idle(&self) -> bool {
        self.runtime_identity().is_some()
            && self.pending_transaction.is_none()
            && self.automation_waiting.is_none()
            && self.automation_completion.is_none()
    }

    pub(crate) fn take_automation_completion(&mut self) -> Option<AutomationCompletion> {
        self.automation_completion.take()
    }

    pub(super) fn finish_automation(
        &mut self,
        result: Result<AutomationResult, String>,
        continuation: SessionContinuation,
    ) {
        if continuation == SessionContinuation::Terminal {
            self.automation_session = None;
        }
        if let Some(token) = self.automation_waiting.take() {
            // Admission forbids overwriting an unconsumed result. A violation
            // is a host invariant failure, not permission to discard a handle.
            assert!(self.automation_completion.is_none());
            self.automation_completion = Some(AutomationCompletion {
                token,
                result,
                continuation,
            });
        } else if continuation == SessionContinuation::Terminal
            && let Some(completion) = &mut self.automation_completion
            && completion.result.as_ref().is_ok_and(has_handles)
        {
            // A completed reply can wait for the broker while navigation or
            // closure revokes its old realm. Keep real already-charged data,
            // but never hand out roots after their owning context is retired.
            // Do not rewrite the manager's original continuation/reap evidence.
            completion.result = Err(
                "Runtime context was revoked before its object handles could be delivered".into(),
            );
        }
    }

    pub(crate) fn cancel_automation_session(&mut self, reason: &str) {
        self.block_session(reason);
    }

    /// Permission loss cannot strand roots when the broker loses its host.
    /// Compare the actual worker, not the permission-filtered Runtime identity,
    /// and never cancel a replacement while retiring old automation ownership.
    pub(super) fn revoke_automation_permission(&mut self, reason: &str) {
        if let Some(identity) = self.automation_session.take()
            && self
                .page_session
                .as_ref()
                .is_some_and(|session| (self.generation, session.id()) == identity)
        {
            self.block_session(reason);
        }
    }
}

#[cfg(all(test, feature = "chrome"))]
mod tests {
    use super::*;
    use crate::scripts::{ScriptSession, SessionUpdate};
    use mg_sparkle::page_session::{ArenaSnapshot, ControlAck, EventOutcome};
    use std::{collections::VecDeque, sync::Mutex};

    #[derive(Default)]
    struct State {
        session_id: u64,
        requests: Vec<(AutomationRequest, u64)>,
        updates: VecDeque<SessionUpdate>,
        validation_pending: bool,
        charges: usize,
        fail_charge: bool,
        canceled: bool,
    }
    struct Worker(Arc<Mutex<State>>);
    impl ScriptSession for Worker {
        fn id(&self) -> u64 {
            let id = self.0.lock().unwrap().session_id;
            if id == 0 { 17 } else { id }
        }
        fn try_dispatch(&mut self, _: SessionInput, _: u64) -> Result<u64, String> {
            Err("Automation-only fixture".into())
        }
        fn try_automation(
            &mut self,
            request: AutomationRequest,
            revision: u64,
        ) -> Result<u64, String> {
            let mut state = self.0.lock().unwrap();
            assert!(!state.validation_pending);
            state.requests.push((request, revision));
            Ok(revision + 1)
        }
        fn try_recv(&mut self) -> Option<SessionUpdate> {
            let mut state = self.0.lock().unwrap();
            let update = state.updates.pop_front()?;
            state.validation_pending = update.reply.is_ok();
            Some(update)
        }
        fn cancel(&self) {
            self.0.lock().unwrap().canceled = true;
        }
        fn charge_active(&mut self, _: Duration) -> Result<(), String> {
            let mut state = self.0.lock().unwrap();
            assert!(
                state.validation_pending,
                "Exactly one accepted host frame charge"
            );
            state.validation_pending = false;
            state.charges += 1;
            if state.fail_charge {
                Err("Owned charge failure".into())
            } else {
                Ok(())
            }
        }
    }
    fn fixture() -> (Browser, Arc<Mutex<State>>, usize) {
        let mut app = test_app();
        app.page_url = "https://example.test/runtime".into();
        app.document = document::parse(
            "<title>Runtime</title><form><input id=value name=value value=old></form>",
            &app.page_url,
        );
        let node = app.document.query_selector(0, "#value").unwrap().unwrap();
        app.generation = 5;
        app.cdp_enabled = true;
        app.scripts_enabled = true;
        app.lifecycle = Some(LifecycleProgress {
            phase: LifecyclePhase::LoadComplete,
            initial_failure: false,
        });
        app.observer_ledger = Some(ObserverLedger::new(ObserverExpectation::Disabled).unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        app.page_session = Some(Box::new(Worker(state.clone())));
        (app, state, node)
    }
    fn request() -> AutomationRequest {
        AutomationRequest {
            owner: 3,
            edits: vec![],
            operation: Operation::Evaluate {
                source: "globalThis.arbitraryState = 11".into(),
                return_by_value: false,
                group: None,
                world: None,
                await_promise: false,
                user_gesture: false,
            },
        }
    }
    fn update(app: &Browser, result: AutomationResult, terminal: bool) -> SessionUpdate {
        let edits = match app.pending_transaction.as_ref().unwrap() {
            PendingTransaction::Automation { request, .. } => request
                .edits
                .iter()
                .map(|edit| ControlAck {
                    node: edit.node,
                    version: edit.version,
                })
                .collect(),
            _ => panic!("Expected actual automation pending variant"),
        };
        SessionUpdate {
            generation: 5,
            session_id: 17,
            sequence: Some(app.page_revision + 1),
            automation: Some(result),
            continuation: if terminal {
                SessionContinuation::Terminal
            } else {
                SessionContinuation::Live
            },
            reply: Ok(SessionReply {
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
                boa: Some(mg_butane::modern::Engine::new().unwrap().report()),
                state: RealmState::Ready,
                acknowledgements: edits,
                task_wake: None,
                lifecycle: app.lifecycle,
                observer: Some(mg_sparkle::page_session::observer::ObserverReply::Disabled),
                local_storage_ops: vec![],
                session_storage_ops: vec![],
                cookie_writes: vec![],
                history_ops: vec![],
                fetch_requests: vec![],
                scroll_request: None,
            }),
        }
    }
    fn value() -> AutomationResult {
        AutomationResult::Value(RemoteValue::Boolean(true))
    }
    fn set_value(update: &mut SessionUpdate, node: usize, value: &str) {
        let node = &mut update
            .reply
            .as_mut()
            .unwrap()
            .snapshot
            .as_mut()
            .unwrap()
            .nodes[node];
        node.attributes
            .iter_mut()
            .find(|(name, _)| name == "value")
            .unwrap()
            .1 = value.into();
    }

    #[test]
    fn runtime_single_flight_one_charge_and_real_edit_versions() {
        let (mut app, state, node) = fixture();
        let identity = app.runtime_identity();
        assert_eq!(
            identity,
            Some((5, 17)),
            "Disabled observer ledger is ordinary browsing"
        );
        assert!(app.runtime_automation_idle());
        app.values.insert(node, "before".into());
        app.edit_versions.insert(node, 9);
        app.dispatch_automation(request(), 2).unwrap();
        assert!(
            !app.runtime_automation_idle(),
            "Busy work defers cleanup, not context identity"
        );
        assert!(app.take_automation_completion().is_none());
        assert_eq!(state.lock().unwrap().requests[0].0.edits[0].version, 9);
        assert!(app.dispatch_automation(request(), 3).is_err());
        assert!(
            app.dispatch_page(InputKind::Click { target: node })
                .is_err()
        );
        app.values.insert(node, "newer café".into());
        app.edit_versions.insert(node, 10);
        let mut response = update(&app, value(), false);
        set_value(&mut response, node, "worker change");
        state.lock().unwrap().updates.push_back(response);
        app.poll_page_session();
        assert_eq!(state.lock().unwrap().charges, 1);
        assert_eq!(app.page_revision, 1);
        assert_eq!(
            app.document.nodes[node].attr("value"),
            Some("worker change")
        );
        assert_eq!(app.values[&node], "newer café");
        assert_eq!(app.edit_versions[&node], 10);
        assert_eq!(
            app.runtime_identity(),
            identity,
            "Projection is not realm replacement"
        );
        assert!(
            !app.runtime_automation_idle(),
            "Unread result is part of the single-flight slot"
        );
        assert!(
            app.dispatch_automation(request(), 3).is_err(),
            "Never overwrite an unread completion"
        );
        let completion = app.take_automation_completion().unwrap();
        assert_eq!(completion.token, 2);
        assert_eq!(completion.result.unwrap(), value());
        assert_eq!(completion.continuation, SessionContinuation::Live);
        assert!(app.runtime_automation_idle());
        app.poll_page_session();
        assert_eq!(state.lock().unwrap().charges, 1);
        assert!(app.take_automation_completion().is_none());
    }

    #[test]
    fn runtime_validation_or_charge_failure_never_publishes_result_or_dom() {
        for field in [
            "charge",
            "generation",
            "session",
            "sequence",
            "revision",
            "ack",
            "graph",
            "result",
            "activation",
            "lifecycle",
            "missing-result",
        ] {
            let (mut app, state, node) = fixture();
            app.values.insert(node, "native".into());
            app.edit_versions.insert(node, 7);
            app.dispatch_automation(request(), 9).unwrap();
            let mut response = update(&app, value(), false);
            set_value(&mut response, node, "must not publish");
            match field {
                "charge" => state.lock().unwrap().fail_charge = true,
                "generation" => response.generation += 1,
                "session" => response.session_id += 1,
                "sequence" => response.sequence = Some(9),
                "revision" => response.reply.as_mut().unwrap().revision += 1,
                "ack" => response.reply.as_mut().unwrap().acknowledgements.clear(),
                "graph" => response
                    .reply
                    .as_mut()
                    .unwrap()
                    .snapshot
                    .as_mut()
                    .unwrap()
                    .nodes[0]
                    .children
                    .push(usize::MAX),
                "result" => response.automation = Some(AutomationResult::Released),
                "activation" => {
                    response.reply.as_mut().unwrap().outcome.click_canceled = Some(false)
                }
                "lifecycle" => {
                    response
                        .reply
                        .as_mut()
                        .unwrap()
                        .lifecycle
                        .as_mut()
                        .unwrap()
                        .initial_failure = true
                }
                "missing-result" => response.automation = None,
                _ => unreachable!(),
            }
            state.lock().unwrap().updates.push_back(response);
            app.poll_page_session();
            assert_eq!(state.lock().unwrap().charges, 1, "{field}");
            assert!(state.lock().unwrap().canceled, "{field}");
            assert!(app.script_blocked, "{field}");
            assert_eq!(app.page_revision, 0, "{field}");
            assert_eq!(
                app.document.nodes[node].attr("value"),
                Some("old"),
                "{field}"
            );
            assert_eq!(app.values[&node], "native", "{field}");
            assert_eq!(app.edit_versions[&node], 7, "{field}");
            let completion = app.take_automation_completion().unwrap();
            assert_eq!(completion.token, 9);
            assert!(completion.result.is_err(), "{field}");
            assert_eq!(completion.continuation, SessionContinuation::Terminal);
            assert!(app.runtime_identity().is_none());
        }
    }

    #[test]
    fn runtime_terminal_ready_commits_data_not_dead_handles() {
        use mg_sparkle::page_session::automation::{ObjectKind, OwnProperty};
        let object = RemoteValue::Object {
            object: 1,
            kind: ObjectKind::Object,
        };
        for result in [
            value(),
            AutomationResult::Value(RemoteValue::Object {
                object: 1,
                kind: mg_sparkle::page_session::automation::ObjectKind::Object,
            }),
            AutomationResult::Exception {
                diagnostic: "Real caught exception".into(),
                exception: Some(object.clone()),
            },
            AutomationResult::Properties(vec![OwnProperty {
                name: vec![120],
                enumerable: false,
                configurable: true,
                descriptor: PropertyDescriptor::Data {
                    value: object,
                    writable: false,
                },
            }]),
            AutomationResult::Properties(vec![OwnProperty {
                name: vec![120],
                enumerable: true,
                configurable: false,
                descriptor: PropertyDescriptor::Accessor {
                    get: Some(RemoteValue::Object {
                        object: 2,
                        kind: ObjectKind::Function,
                    }),
                    set: None,
                },
            }]),
            AutomationResult::Properties(vec![OwnProperty {
                name: vec![120],
                enumerable: true,
                configurable: true,
                descriptor: PropertyDescriptor::Data {
                    value: RemoteValue::Null,
                    writable: true,
                },
            }]),
        ] {
            let (mut app, state, node) = fixture();
            let mut command = request();
            if matches!(result, AutomationResult::Properties(_)) {
                command.operation = Operation::GetOwnProperties { object: 1 };
            }
            app.dispatch_automation(command, 7).unwrap();
            let mut response = update(&app, result.clone(), true);
            // A final Ready completion may advertise genuinely retained work;
            // trusted terminal status must suppress that otherwise-valid wake.
            let reply = response.reply.as_mut().unwrap();
            reply.task_wake = Some(TaskWake {
                token: 1,
                delay_ms: 100,
            });
            let report = reply.boa.as_mut().unwrap();
            report.jobs_admitted = 1;
            report.pending_jobs = 1;
            set_value(&mut response, node, "real final effect");
            state.lock().unwrap().updates.push_back(response);
            app.poll_page_session();
            assert_eq!(state.lock().unwrap().charges, 1);
            assert_eq!(
                app.document.nodes[node].attr("value"),
                Some("real final effect")
            );
            assert_eq!(app.page_revision, 1);
            let completion = app.take_automation_completion().unwrap();
            assert_eq!(completion.result.is_err(), has_handles(&result));
            assert_eq!(completion.continuation, SessionContinuation::Terminal);
            assert!(app.runtime_identity().is_none());
            assert!(app.task_wake.is_none());
            assert!(app.dispatch_automation(request(), 8).is_err());
        }
    }

    #[test]
    fn runtime_admission_is_not_scripting_permission_or_cleanup_credit() {
        for field in [
            "disabled",
            "debugging",
            "loading",
            "fetching",
            "blocked",
            "absent",
            "initial",
        ] {
            let (mut app, state, _) = fixture();
            match field {
                "disabled" => app.scripts_enabled = false,
                "debugging" => app.cdp_enabled = false,
                "loading" => app.loading = true,
                "fetching" => app.fetching = true,
                "blocked" => app.script_blocked = true,
                "absent" => app.page_session = None,
                "initial" => app.lifecycle = None,
                _ => unreachable!(),
            }
            assert!(app.runtime_identity().is_none(), "{field}");
            assert!(app.dispatch_automation(request(), 1).is_err(), "{field}");
            assert!(state.lock().unwrap().requests.is_empty(), "{field}");
        }
        let (mut app, state, node) = fixture();
        let mut invalid = request();
        invalid.owner = 0;
        assert!(app.dispatch_automation(invalid, 1).is_err());
        let mut invalid = request();
        invalid.edits.push(ControlEdit {
            node,
            version: 1,
            value: "forged".into(),
        });
        assert!(app.dispatch_automation(invalid, 1).is_err());
        assert!(app.dispatch_automation(request(), 0).is_err());
        let mut invalid = request();
        invalid.operation = Operation::Evaluate {
            source: " ".repeat(mg_sparkle::page_session::automation::MAX_SOURCE_BYTES + 1),
            return_by_value: false,
            group: None,
            world: None,
            await_promise: false,
            user_gesture: false,
        };
        assert!(app.dispatch_automation(invalid, 1).is_err());
        assert!(state.lock().unwrap().requests.is_empty());
        assert!(!state.lock().unwrap().canceled);
        app.values.insert(node, "retained".into());
        app.edit_versions.insert(node, 3);
        let cleanup = AutomationRequest {
            owner: 0,
            edits: vec![],
            operation: Operation::ReleaseOwners { owners: vec![3] },
        };
        app.dispatch_automation(cleanup, 4).unwrap();
        assert!(state.lock().unwrap().requests[0].0.edits.is_empty());
        state
            .lock()
            .unwrap()
            .updates
            .push_back(update(&app, AutomationResult::Released, false));
        app.poll_page_session();
        assert_eq!(state.lock().unwrap().charges, 1);
        assert_eq!(app.values[&node], "retained");
        assert_eq!(app.edit_versions[&node], 3);
        assert_eq!(
            app.take_automation_completion().unwrap().result.unwrap(),
            AutomationResult::Released
        );
    }

    #[test]
    fn runtime_cached_completion_cannot_publish_handles_after_context_revocation() {
        for handles in [false, true] {
            for navigation in [false, true] {
                let (mut app, state, _) = fixture();
                app.dispatch_automation(request(), 5).unwrap();
                let result = if handles {
                    AutomationResult::Value(RemoteValue::Object {
                        object: 1,
                        kind: mg_sparkle::page_session::automation::ObjectKind::Object,
                    })
                } else {
                    value()
                };
                state
                    .lock()
                    .unwrap()
                    .updates
                    .push_back(update(&app, result, false));
                app.poll_page_session();
                assert_eq!(state.lock().unwrap().charges, 1);
                assert!(app.automation_waiting.is_none());
                if navigation {
                    // Exercise the exact helper used at accepted navigation,
                    // without starting an unrelated network thread in this test.
                    app.generation += 1;
                    app.finish_automation(
                        Err("Navigation retired old realm".into()),
                        SessionContinuation::Terminal,
                    );
                } else {
                    app.block_session("Old worker closed before broker delivery");
                }
                let completion = app.take_automation_completion().unwrap();
                assert_eq!(completion.token, 5);
                assert_eq!(completion.result.is_err(), handles);
                assert_eq!(
                    completion.continuation,
                    SessionContinuation::Live,
                    "Do not invent post-reap manager metadata"
                );
                assert_eq!(
                    state.lock().unwrap().charges,
                    1,
                    "No second projection charge"
                );
            }
        }
    }

    #[test]
    fn runtime_observer_and_legacy_lanes_never_gain_automation_authority() {
        struct Lane(ObserverExpectation, scripts::SourceWireMode);
        impl scripts::ScriptRuntime for Lane {
            fn observer_expectation(&self) -> ObserverExpectation {
                self.0
            }
            fn source_wire_mode(&self) -> scripts::SourceWireMode {
                self.1
            }
            fn set_generation(&self, _: u64) -> Result<(), String> {
                Ok(())
            }
            fn start(
                &self,
                _: mg_sparkle::js_browser::Request,
                _: u64,
            ) -> Result<(Box<dyn scripts::ScriptSession>, SessionReply), String> {
                Err("No child in parent-only lane control".into())
            }
            fn close(&self) {}
            fn wait_idle(&self) -> Result<(), String> {
                Ok(())
            }
        }
        for (expectation, mode) in [
            (
                ObserverExpectation::WindowHarness { run_id: 1 },
                scripts::SourceWireMode::Modern,
            ),
            (
                ObserverExpectation::Disabled,
                scripts::SourceWireMode::Legacy,
            ),
        ] {
            let (mut app, state, _) = fixture();
            app.child_pool = Arc::new(Lane(expectation, mode));
            assert!(app.runtime_identity().is_none());
            assert!(!app.runtime_automation_idle());
            assert!(app.dispatch_automation(request(), 1).is_err());
            assert!(state.lock().unwrap().requests.is_empty());
            assert_eq!(state.lock().unwrap().charges, 0);
        }
    }

    #[test]
    fn runtime_ordinary_exception_and_representation_rejection_commit_real_effects_once() {
        for result in [
            AutomationResult::Exception {
                diagnostic: "Native caught exception".into(),
                exception: None,
            },
            AutomationResult::Rejected {
                diagnostic: "Unsupported representation".into(),
            },
        ] {
            let (mut app, state, node) = fixture();
            app.values.insert(node, "native edit".into());
            app.edit_versions.insert(node, 3);
            app.dispatch_automation(request(), 2).unwrap();
            let mut response = update(&app, result.clone(), false);
            set_value(&mut response, node, "effect preceding exception/rejection");
            state.lock().unwrap().updates.push_back(response);
            app.poll_page_session();
            assert_eq!(state.lock().unwrap().charges, 1);
            assert_eq!(app.page_revision, 1);
            assert_eq!(
                app.document.nodes[node].attr("value"),
                Some("effect preceding exception/rejection")
            );
            assert!(!app.values.contains_key(&node));
            assert!(!app.edit_versions.contains_key(&node));
            assert_eq!(
                app.take_automation_completion().unwrap().result.unwrap(),
                result
            );
            assert_eq!(app.runtime_identity(), Some((5, 17)));
            assert!(!state.lock().unwrap().canceled);
        }
    }

    #[test]
    fn runtime_permission_revocation_retires_only_the_automation_used_worker() {
        for scripts in [false, true] {
            for stage in ["pending", "handles", "data", "delivered"] {
                let (mut app, state, _) = fixture();
                app.dispatch_automation(request(), 5).unwrap();
                if stage != "pending" {
                    let result = if stage == "data" {
                        value()
                    } else {
                        AutomationResult::Value(RemoteValue::Object {
                            object: 1,
                            kind: mg_sparkle::page_session::automation::ObjectKind::Object,
                        })
                    };
                    state
                        .lock()
                        .unwrap()
                        .updates
                        .push_back(update(&app, result, false));
                    app.poll_page_session();
                }
                if stage == "delivered" {
                    app.take_automation_completion().unwrap().result.unwrap();
                }
                if scripts {
                    app.set_scripts_enabled(false);
                } else {
                    app.set_debugging(false);
                }
                assert!(state.lock().unwrap().canceled, "{scripts}/{stage}");
                assert!(app.script_blocked, "{scripts}/{stage}");
                assert!(app.pending_transaction.is_none());
                assert!(app.runtime_identity().is_none());
                let completion = app.take_automation_completion();
                match stage {
                    "pending" => {
                        let completion = completion.unwrap();
                        assert!(completion.result.is_err());
                        assert_eq!(completion.continuation, SessionContinuation::Terminal);
                    }
                    "handles" => {
                        let completion = completion.unwrap();
                        assert!(completion.result.is_err());
                        assert_eq!(completion.continuation, SessionContinuation::Live);
                    }
                    "data" => assert_eq!(completion.unwrap().result.unwrap(), value()),
                    "delivered" => assert!(completion.is_none()),
                    _ => unreachable!(),
                }
                if scripts {
                    app.set_scripts_enabled(false);
                    app.set_scripts_enabled(true);
                } else {
                    app.set_debugging(false);
                    app.set_debugging(true);
                }
                assert!(app.runtime_identity().is_none(), "Re-enable is not restart");
                assert!(app.dispatch_automation(request(), 6).is_err());
                let state = state.lock().unwrap();
                assert_eq!(state.requests.len(), 1);
                assert_eq!(state.charges, usize::from(stage != "pending"));
            }
            for rejected in [false, true] {
                let (mut app, state, _) = fixture();
                if rejected {
                    let mut invalid = request();
                    invalid.owner = 0;
                    assert!(app.dispatch_automation(invalid, 5).is_err());
                }
                if scripts {
                    app.set_scripts_enabled(false);
                } else {
                    app.set_debugging(false);
                }
                assert!(!state.lock().unwrap().canceled);
                assert!(!app.script_blocked);
                assert!(state.lock().unwrap().requests.is_empty());
            }
            for generation_changed in [false, true] {
                let (mut app, old_state, _) = fixture();
                app.dispatch_automation(request(), 5).unwrap();
                // Deliberately retain any old automation marker to test the
                // identity fence independently of navigation's normal cleanup.
                app.pending_transaction = None;
                app.automation_waiting = None;
                if generation_changed {
                    app.generation += 1;
                }
                let replacement = Arc::new(Mutex::new(State {
                    session_id: if generation_changed { 17 } else { 18 },
                    ..State::default()
                }));
                app.page_session = Some(Box::new(Worker(replacement.clone())));
                if scripts {
                    app.set_scripts_enabled(false);
                } else {
                    app.set_debugging(false);
                }
                assert!(!replacement.lock().unwrap().canceled);
                assert!(!app.script_blocked);
                assert_eq!(old_state.lock().unwrap().requests.len(), 1);
                assert!(replacement.lock().unwrap().requests.is_empty());
                assert_eq!(replacement.lock().unwrap().charges, 0);
            }
        }
    }
}
