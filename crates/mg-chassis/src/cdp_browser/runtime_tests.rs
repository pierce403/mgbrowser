//! Predeclared broker controls: real worker behavior is tested separately.
use super::*;
use mg_sparkle::page_session::automation::OwnProperty;
use std::collections::VecDeque;

struct MockHost {
    identity: Option<(u64, u64)>,
    idle: bool,
    reject: bool,
    requests: Vec<(AutomationRequest, u64)>,
    completions: VecDeque<AutomationCompletion>,
    canceled: bool,
}
impl Default for MockHost {
    fn default() -> Self {
        Self {
            identity: Some((7, 11)),
            idle: true,
            reject: false,
            requests: Vec::new(),
            completions: VecDeque::new(),
            canceled: false,
        }
    }
}
impl Host for MockHost {
    fn identity(&self) -> Option<(u64, u64)> {
        self.identity
    }
    fn idle(&self) -> bool {
        self.idle
    }
    fn dispatch(&mut self, request: AutomationRequest, token: u64) -> Result<(), String> {
        if self.reject {
            return Err("Existing worker allowance exhausted".into());
        }
        assert!(self.idle);
        assert!(request.edits.is_empty());
        request.validate().unwrap();
        self.requests.push((request, token));
        self.idle = false;
        Ok(())
    }
    fn completion(&mut self) -> Option<AutomationCompletion> {
        self.completions.pop_front()
    }
    fn cancel(&mut self, _: &str) {
        self.canceled = true;
        self.identity = None;
    }
    fn origin(&self) -> String {
        "https://runtime.test".into()
    }
}
impl MockHost {
    fn finish(&mut self, result: AutomationResult) {
        let token = self.requests.last().unwrap().1;
        self.completions.push_back(AutomationCompletion {
            token,
            result: Ok(result),
            continuation: SessionContinuation::Live,
        });
        self.idle = true;
    }
}
fn route(client: u64) -> Route {
    Route {
        client,
        session: None,
    }
}
fn command(
    broker: &mut Broker,
    host: &mut MockHost,
    route: &Route,
    method: &str,
    params: Value,
) -> Vec<Delivery> {
    broker.command("page-1", host, route, json!(1), method, &params)
}
fn value_reply(deliveries: &[Delivery]) -> &Value {
    deliveries
        .iter()
        .find_map(|d| match d {
            Delivery::Reply {
                result: Ok(value), ..
            } => Some(value),
            _ => None,
        })
        .unwrap()
}
fn error(deliveries: &[Delivery]) -> &(i32, String) {
    deliveries
        .iter()
        .find_map(|d| match d {
            Delivery::Reply {
                result: Err(error), ..
            } => Some(error),
            _ => None,
        })
        .unwrap()
}
fn methods(deliveries: &[Delivery]) -> Vec<&str> {
    deliveries
        .iter()
        .map(|d| match d {
            Delivery::Reply { .. } => "reply",
            Delivery::Event { method, .. } => *method,
        })
        .collect()
}

#[test]
fn enable_reply_precedes_actual_context_and_absent_realm_has_no_context() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    let output = command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.enable",
        json!({}),
    );
    assert_eq!(
        methods(&output),
        ["reply", "Runtime.executionContextCreated"]
    );
    let Delivery::Event { params, .. } = &output[1] else {
        panic!()
    };
    assert_eq!(params["context"]["id"], 1);
    assert_eq!(params["context"]["origin"], "https://runtime.test");
    assert_eq!(params["context"]["auxData"]["frameId"], "frame-1");
    host.identity = None;
    let output = broker.poll("page-1", &mut host);
    assert_eq!(methods(&output), ["Runtime.executionContextDestroyed"]);
    let output = command(
        &mut broker,
        &mut host,
        &route(2),
        "Runtime.enable",
        json!({}),
    );
    assert_eq!(methods(&output), ["reply"]);
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(2),
            "Runtime.evaluate",
            json!({"expression":"1"})
        ))
        .0,
        -32000
    );
    assert!(host.requests.is_empty());
}

#[test]
fn arbitrary_source_is_worker_dispatched_and_only_completion_publishes() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    let source = "globalThis.notARecognizedExpression = (() => 37)();";
    assert!(
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":source})
        )
        .is_empty()
    );
    let Operation::Evaluate { source: actual, .. } = &host.requests[0].0.operation else {
        panic!()
    };
    assert_eq!(actual, source);
    assert_ne!(host.requests[0].0.owner, 0);
    assert!(broker.poll("page-1", &mut host).is_empty());
    host.finish(AutomationResult::Value(RemoteValue::Number(
        Number::Finite(37.),
    )));
    assert_eq!(
        value_reply(&broker.poll("page-1", &mut host))["result"]["value"],
        37.
    );
}

#[test]
fn identity_survives_projection_move_and_navigation_destroys_then_creates() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.enable",
        json!({}),
    );
    assert!(broker.poll("page-1", &mut host).is_empty());
    host.identity = Some((8, 12));
    let output = broker.poll("page-1", &mut host);
    assert_eq!(
        methods(&output),
        [
            "Runtime.executionContextDestroyed",
            "Runtime.executionContextCreated"
        ]
    );
    let Delivery::Event { params, .. } = &output[1] else {
        panic!()
    };
    assert_eq!(params["context"]["id"], 2);
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"1", "contextId":1})
        ))
        .0,
        -32000
    );
    assert!(host.requests.is_empty());
}

#[test]
fn terminal_completion_reply_precedes_context_destruction() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.enable",
        json!({}),
    );
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"6"}),
    );
    let token = host.requests[0].1;
    host.identity = None;
    host.completions.push_back(AutomationCompletion {
        token,
        result: Ok(AutomationResult::Value(RemoteValue::Number(
            Number::Finite(6.),
        ))),
        continuation: SessionContinuation::Terminal,
    });
    let output = broker.poll("page-1", &mut host);
    assert_eq!(
        methods(&output),
        ["reply", "Runtime.executionContextDestroyed"]
    );
    assert_eq!(value_reply(&output)["result"]["value"], 6.);
    assert!(broker.contexts.is_empty());
}

#[test]
fn handles_bind_target_context_client_and_flattened_route() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"({})"}),
    );
    host.finish(AutomationResult::Value(RemoteValue::Object {
        object: 1,
        kind: ObjectKind::Object,
    }));
    let output = broker.poll("page-1", &mut host);
    let object = value_reply(&output)["result"]["objectId"]
        .as_str()
        .unwrap()
        .to_owned();
    for foreign in [
        route(2),
        Route {
            client: 1,
            session: Some("session-1".into()),
        },
    ] {
        assert_eq!(
            error(&command(
                &mut broker,
                &mut host,
                &foreign,
                "Runtime.getProperties",
                json!({"objectId":object,"ownProperties":true})
            ))
            .0,
            -32000
        );
    }
    assert_eq!(host.requests.len(), 1);
    let output = broker.command(
        "page-2",
        &mut host,
        &route(1),
        json!(2),
        "Runtime.releaseObject",
        &json!({"objectId":object}),
    );
    assert_eq!(error(&output).0, -32000);
    assert_eq!(host.requests.len(), 1);
    assert!(
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.getProperties",
            json!({"objectId":object,"ownProperties":true})
        )
        .is_empty()
    );
    assert!(matches!(
        host.requests[1].0.operation,
        Operation::GetOwnProperties { object: 1 }
    ));
}

#[test]
fn unsupported_semantic_options_do_not_admit_worker_or_owner() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    for name in [
        "awaitPromise",
        "userGesture",
        "silent",
        "generatePreview",
        "includeCommandLineAPI",
        "throwOnSideEffect",
        "replMode",
    ] {
        let mut params = json!({"expression":"1"});
        params[name] = json!(true);
        assert_eq!(
            error(&command(
                &mut broker,
                &mut host,
                &route(1),
                "Runtime.evaluate",
                params
            ))
            .0,
            -32602
        );
    }
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"1","timeout":100})
        ))
        .0,
        -32602
    );
    assert!(host.requests.is_empty());
    assert!(broker.contexts["page-1"].owners.is_empty());
}

#[test]
fn calls_preserve_receiver_arguments_and_inspection_requires_own_properties() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"({})","objectGroup":"group"}),
    );
    host.finish(AutomationResult::Value(RemoteValue::Object {
        object: 1,
        kind: ObjectKind::Object,
    }));
    let output = broker.poll("page-1", &mut host);
    let id = value_reply(&output)["result"]["objectId"].clone();
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.getProperties",
            json!({"objectId":id})
        ))
        .0,
        -32602
    );
    assert!(command(&mut broker, &mut host, &route(1), "Runtime.callFunctionOn", json!({"functionDeclaration":"function(a,b,c){return a === this}","objectId":id,"arguments":[{"objectId":id},{"unserializableValue":"-0"},{"unserializableValue":"19n"}]})).is_empty());
    let Operation::CallFunction {
        receiver,
        arguments,
        group,
        ..
    } = &host.requests[1].0.operation
    else {
        panic!()
    };
    assert_eq!(*receiver, Some(1));
    assert_eq!(
        arguments,
        &[
            Argument::Object(1),
            Argument::Number(Number::NegativeZero),
            Argument::BigInt("19".into())
        ]
    );
    assert_eq!(*group, None, "Worker inherits receiver's actual group");
    host.finish(AutomationResult::Value(RemoteValue::Boolean(true)));
    broker.poll("page-1", &mut host);
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.getProperties",
        json!({"objectId":id,"ownProperties":true}),
    );
    host.finish(AutomationResult::Properties(vec![OwnProperty {
        name: "value".encode_utf16().collect(),
        enumerable: false,
        configurable: true,
        descriptor: PropertyDescriptor::Accessor {
            get: Some(RemoteValue::Object {
                object: 2,
                kind: ObjectKind::Function,
            }),
            set: None,
        },
    }]));
    let output = broker.poll("page-1", &mut host);
    let descriptor = &value_reply(&output)["result"][0];
    assert_eq!(descriptor["enumerable"], false);
    assert_eq!(descriptor["configurable"], true);
    assert_eq!(descriptor["isOwn"], true);
    assert_eq!(descriptor["get"]["type"], "function");
    assert!(descriptor.get("set").is_none());
    assert!(descriptor.get("value").is_none());
}

#[test]
fn revocation_consumes_completion_suppresses_result_and_defers_busy_cleanup() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"globalThis.effect=1"}),
    );
    broker.revoke(&route(1));
    assert!(broker.poll("page-1", &mut host).is_empty());
    assert!(!host.canceled);
    host.finish(AutomationResult::Value(RemoteValue::Object {
        object: 1,
        kind: ObjectKind::Object,
    }));
    assert!(broker.poll("page-1", &mut host).is_empty());
    assert!(host.completions.is_empty());
    let request = &host.requests[1].0;
    assert_eq!(request.owner, 0);
    assert!(matches!(&request.operation, Operation::ReleaseOwners { owners } if owners == &[1]));
    assert_eq!(broker.contexts["page-1"].owners.len(), 1);
    host.finish(AutomationResult::Released);
    assert!(broker.poll("page-1", &mut host).is_empty());
    assert!(broker.contexts["page-1"].owners.is_empty());
}

#[test]
fn pending_revoked_owners_count_live_and_cleanup_failure_cancels_same_worker() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    for client in 1..=MAX_OWNERS as u64 {
        command(
            &mut broker,
            &mut host,
            &route(client),
            "Runtime.evaluate",
            json!({"expression":"0"}),
        );
        host.finish(AutomationResult::Value(RemoteValue::Undefined));
        broker.poll("page-1", &mut host);
    }
    host.idle = false; // A genuine non-automation page task owns the slot.
    for client in 1..=MAX_OWNERS as u64 {
        broker.revoke(&route(client));
    }
    broker.poll("page-1", &mut host);
    assert!(!host.canceled);
    assert_eq!(broker.contexts["page-1"].owners.len(), MAX_OWNERS);
    host.idle = true;
    host.reject = true;
    let output = broker.poll("page-1", &mut host);
    assert!(output.is_empty());
    assert!(host.canceled);
    assert!(broker.contexts.is_empty());
}

#[test]
fn owner_lifetime_token_and_context_id_exhaustion_fail_closed() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    broker.poll("page-1", &mut host);
    broker.contexts.get_mut("page-1").unwrap().issued_owner = MAX_OWNER_IDENTITIES as u64;
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"1"})
        ))
        .0,
        -32000
    );
    broker.contexts.get_mut("page-1").unwrap().issued_owner = 0;
    broker.next_token = u64::MAX;
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"1"})
        ))
        .0,
        -32000
    );
    assert!(host.requests.is_empty());
    let mut broker = Broker::default();
    broker.next_context = i32::MAX as u32;
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"1"})
        ))
        .0,
        -32000
    );
    assert!(broker.contexts.is_empty());
}

#[test]
fn scalar_and_tree_admission_is_lossless_and_bounded() {
    assert_eq!(
        remote(1, 1, &RemoteValue::Number(Number::NegativeZero)).unwrap()["unserializableValue"],
        "-0"
    );
    assert_eq!(
        remote(1, 1, &RemoteValue::Number(Number::NaN)).unwrap()["unserializableValue"],
        "NaN"
    );
    assert_eq!(
        remote(1, 1, &RemoteValue::BigInt("-9".into())).unwrap()["unserializableValue"],
        "-9n"
    );
    assert!(remote(1, 1, &RemoteValue::String(vec![0xd800])).is_err());
    assert!(
        remote(
            1,
            1,
            &RemoteValue::ByValue(SerializedValue::Object(vec![(
                vec![0xd800],
                SerializedValue::Null
            )]))
        )
        .is_err()
    );
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"x".repeat(MAX_SOURCE_BYTES+1)})
        ))
        .0,
        -32602
    );
    assert_eq!(
        error(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.callFunctionOn",
            json!({"functionDeclaration":"function(){}","arguments":[{"value":[-0.0]}]})
        ))
        .0,
        -32602
    );
    assert!(host.requests.is_empty());
}

#[test]
fn missing_or_mismatched_completion_token_cancels_without_publication() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.enable",
        json!({}),
    );
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"1"}),
    );
    host.completions.push_back(AutomationCompletion {
        token: 999,
        result: Ok(AutomationResult::Value(RemoteValue::Undefined)),
        continuation: SessionContinuation::Live,
    });
    let output = broker.poll("page-1", &mut host);
    assert_eq!(error(&output).0, -32000);
    assert_eq!(
        methods(&output),
        ["reply", "Runtime.executionContextDestroyed"]
    );
    assert!(host.canceled);
}

#[test]
fn disabling_notifications_preserves_owner_and_explicit_release_dispatches() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.enable",
        json!({}),
    );
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"({})","objectGroup":"g"}),
    );
    host.finish(AutomationResult::Value(RemoteValue::Object {
        object: 1,
        kind: ObjectKind::Object,
    }));
    let output = broker.poll("page-1", &mut host);
    let id = value_reply(&output)["result"]["objectId"].clone();
    assert_eq!(
        methods(&command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.disable",
            json!({})
        )),
        ["reply"]
    );
    assert!(
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.releaseObject",
            json!({"objectId":id})
        )
        .is_empty()
    );
    assert_eq!(host.requests[1].0.owner, 1);
    assert!(matches!(
        host.requests[1].0.operation,
        Operation::ReleaseObject { object: 1 }
    ));
    host.finish(AutomationResult::Released);
    assert_eq!(value_reply(&broker.poll("page-1", &mut host)), &json!({}));
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.releaseObjectGroup",
        json!({"objectGroup":"g"}),
    );
    assert_eq!(host.requests[2].0.owner, 1);
    assert!(
        matches!(&host.requests[2].0.operation,Operation::ReleaseGroup { group } if group == "g")
    );
    host.finish(AutomationResult::Released);
    broker.poll("page-1", &mut host);
    host.identity = None;
    assert!(broker.poll("page-1", &mut host).is_empty());
}

#[test]
fn closed_target_fails_pending_before_context_destroy_and_stale_ids_never_retarget() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.enable",
        json!({}),
    );
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"({})"}),
    );
    let output = broker.retain_targets(&[]);
    assert_eq!(
        methods(&output),
        ["reply", "Runtime.executionContextDestroyed"]
    );
    assert!(error(&output).1.contains("closed"));
    assert!(broker.contexts.is_empty());
    assert!(broker.enabled.is_empty());
}

#[test]
fn terminal_handle_result_and_unrepresentable_worker_result_fail_closed() {
    for result in [
        AutomationResult::Value(RemoteValue::Object {
            object: 1,
            kind: ObjectKind::Object,
        }),
        AutomationResult::Value(RemoteValue::String(vec![0xd800])),
    ] {
        let terminal = matches!(result, AutomationResult::Value(RemoteValue::Object { .. }));
        let mut broker = Broker::default();
        let mut host = MockHost::default();
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.enable",
            json!({}),
        );
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"owned arbitrary result"}),
        );
        host.finish(result);
        if terminal {
            host.completions.back_mut().unwrap().continuation = SessionContinuation::Terminal;
        }
        let output = broker.poll("page-1", &mut host);
        assert_eq!(
            methods(&output),
            ["reply", "Runtime.executionContextDestroyed"]
        );
        assert_eq!(error(&output).0, -32000);
        assert!(host.canceled);
        assert!(broker.contexts.is_empty());
    }
}

#[test]
fn argument_shape_source_group_and_encoded_admissions_do_not_dispatch() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    for params in [
        json!({"functionDeclaration":"function(){}","arguments":[{"value":1,"objectId":"mg:1:1:1"}]}),
        json!({"functionDeclaration":"function(){}","arguments":[{"unserializableValue":"01n"}]}),
        json!({"functionDeclaration":"function(){}","arguments":[{"unserializableValue":"-0n"}]}),
        json!({"functionDeclaration":"function(){}","arguments":vec![json!({});MAX_ARGUMENTS+1]}),
        json!({"functionDeclaration":"function(){}","arguments":[{"value":"a".repeat(MAX_STRING_UNITS+1)}]}),
        json!({"functionDeclaration":"function(){}","arguments":[{"value":"a".repeat(MAX_ARGUMENT_BYTES+1)}]}),
        json!({"functionDeclaration":"function(){}","objectGroup":"g".repeat(MAX_GROUP_BYTES+1)}),
        json!({"functionDeclaration":"function(){}","objectId":"mg:1:1:1","executionContextId":1}),
    ] {
        assert_eq!(
            error(&command(
                &mut broker,
                &mut host,
                &route(1),
                "Runtime.callFunctionOn",
                params
            ))
            .0,
            -32602
        );
    }
    assert!(host.requests.is_empty());
    assert!(broker.contexts["page-1"].owners.is_empty());
}

#[test]
fn cached_live_handles_cannot_escape_after_navigation_or_block() {
    for kind in 0..3 {
        for new_identity in [None, Some((8, 12))] {
            let mut broker = Broker::default();
            let mut host = MockHost::default();
            command(
                &mut broker,
                &mut host,
                &route(1),
                "Runtime.enable",
                json!({}),
            );
            command(
                &mut broker,
                &mut host,
                &route(1),
                "Runtime.evaluate",
                json!({"expression":"({})"}),
            );
            if kind == 1 {
                host.finish(AutomationResult::Value(RemoteValue::Object {
                    object: 1,
                    kind: ObjectKind::Object,
                }));
                let output = broker.poll("page-1", &mut host);
                let id = value_reply(&output)["result"]["objectId"].clone();
                command(
                    &mut broker,
                    &mut host,
                    &route(1),
                    "Runtime.getProperties",
                    json!({"objectId":id,"ownProperties":true}),
                );
            }
            let object = RemoteValue::Object {
                object: 2,
                kind: ObjectKind::Object,
            };
            host.finish(match kind {
                0 => AutomationResult::Value(object),
                1 => AutomationResult::Properties(vec![OwnProperty {
                    name: vec![120],
                    enumerable: true,
                    configurable: true,
                    descriptor: PropertyDescriptor::Data {
                        value: object,
                        writable: true,
                    },
                }]),
                _ => AutomationResult::Exception {
                    diagnostic: "Native exception".into(),
                    exception: Some(object),
                },
            });
            // The completion was actually cached as Live before navigation or
            // blocking destroyed its worker; a new realm must never be canceled.
            host.identity = new_identity;
            let output = broker.poll("page-1", &mut host);
            assert_eq!(error(&output).0, -32000);
            let expected = if new_identity.is_some() {
                vec![
                    "reply",
                    "Runtime.executionContextDestroyed",
                    "Runtime.executionContextCreated",
                ]
            } else {
                vec!["reply", "Runtime.executionContextDestroyed"]
            };
            assert_eq!(methods(&output), expected);
            assert!(!host.canceled);
            assert!(host.completions.is_empty());
            if new_identity.is_some() {
                assert_eq!(broker.contexts["page-1"].identity, (8, 12));
                assert!(broker.contexts["page-1"].owners.is_empty());
            }
        }
    }
}

#[test]
fn completed_nonhandle_data_remains_available_before_old_context_destruction() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.enable",
        json!({}),
    );
    command(
        &mut broker,
        &mut host,
        &route(1),
        "Runtime.evaluate",
        json!({"expression":"37"}),
    );
    host.finish(AutomationResult::Value(RemoteValue::Number(
        Number::Finite(37.),
    )));
    host.identity = Some((8, 12));
    let output = broker.poll("page-1", &mut host);
    assert_eq!(
        methods(&output),
        [
            "reply",
            "Runtime.executionContextDestroyed",
            "Runtime.executionContextCreated"
        ]
    );
    assert_eq!(value_reply(&output)["result"]["value"], 37.);
    assert!(!host.canceled);
}

#[test]
fn caught_exception_ids_are_real_monotonic_and_unknown_locations_are_omitted() {
    let mut broker = Broker::default();
    let mut host = MockHost::default();
    for expected in 1..=2 {
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"throw 7"}),
        );
        host.finish(AutomationResult::Exception {
            diagnostic: "Thrown value".into(),
            exception: Some(RemoteValue::Number(Number::Finite(7.))),
        });
        let output = broker.poll("page-1", &mut host);
        let details = &value_reply(&output)["exceptionDetails"];
        assert_eq!(details["exceptionId"], expected);
        assert_eq!(details["exception"]["value"], 7.);
        for unknown in [
            "lineNumber",
            "columnNumber",
            "url",
            "scriptId",
            "stackTrace",
        ] {
            assert!(details.get(unknown).is_none());
        }
    }
}

#[test]
fn unavailable_exception_value_is_not_fabricated_as_thrown_undefined() {
    for actual in [None, Some(RemoteValue::Undefined)] {
        let mut broker = Broker::default();
        let mut host = MockHost::default();
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"arbitrary throw source"}),
        );
        host.finish(AutomationResult::Exception {
            diagnostic: "Native bounded exception".into(),
            exception: actual.clone(),
        });
        let output = broker.poll("page-1", &mut host);
        let reply = value_reply(&output);
        assert_eq!(
            reply["exceptionDetails"]["text"],
            "Native bounded exception"
        );
        if actual.is_some() {
            assert_eq!(reply["result"], json!({"type":"undefined"}));
            assert_eq!(
                reply["exceptionDetails"]["exception"],
                json!({"type":"undefined"})
            );
        } else {
            assert!(reply.get("result").is_none());
            assert!(reply["exceptionDetails"].get("exception").is_none());
        }
    }
}

#[test]
fn cached_failed_cleanup_never_cancels_a_replacement_worker() {
    for native_failure in [false, true] {
        let mut broker = Broker::default();
        let mut host = MockHost::default();
        command(
            &mut broker,
            &mut host,
            &route(1),
            "Runtime.evaluate",
            json!({"expression":"1"}),
        );
        host.finish(AutomationResult::Value(RemoteValue::Undefined));
        broker.poll("page-1", &mut host);
        command(
            &mut broker,
            &mut host,
            &route(2),
            "Runtime.enable",
            json!({}),
        );
        broker.revoke(&route(1));
        broker.poll("page-1", &mut host);
        assert!(matches!(
            host.requests[1].0.operation,
            Operation::ReleaseOwners { .. }
        ));
        host.finish(AutomationResult::Rejected {
            diagnostic: "Old worker cleanup failed".into(),
        });
        if native_failure {
            host.completions.back_mut().unwrap().result = Err("Old worker cleanup failed".into());
        }
        host.identity = Some((8, 12));
        let output = broker.poll("page-1", &mut host);
        assert_eq!(
            methods(&output),
            [
                "Runtime.executionContextDestroyed",
                "Runtime.executionContextCreated"
            ]
        );
        assert!(!host.canceled);
        assert!(host.completions.is_empty());
        assert_eq!(broker.contexts["page-1"].identity, (8, 12));
        assert!(broker.contexts["page-1"].owners.is_empty());
        assert_eq!(
            host.requests.len(),
            2,
            "No new worker or cleanup credit was created"
        );
    }
}
