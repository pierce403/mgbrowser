//! Parent source admission is independent of page-controlled HTML comments.
#![cfg(feature = "modern")]
use mg_sparkle::{
    document,
    js_browser::{
        ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource, LegacyRequest,
        MAX_CLASSIC_SOURCE_URL_BYTES, MAX_EXTERNAL_SOURCE, MAX_SOURCE, MAX_TYPED_REQUEST_BYTES,
        Request,
        boa::{BoaPageRealm, execute},
        external_source_json_bytes, external_source_json_bytes_with_provenance, json_bytes,
    },
    page_session::{InputKind, LifecyclePhase, SessionInput, TaskPump},
};

const URL: &str = "https://example.test/owned";

fn request(html: impl Into<String>, scripts: &[(&str, String)]) -> Request {
    let html = html.into();
    let document = document::parse_with_scripting(&html, URL, true);
    let external_scripts = scripts
        .iter()
        .map(|(id, source)| ExternalScriptSource {
            node: document
                .query_selector(0, &format!("#{id}"))
                .unwrap()
                .unwrap(),
            source: source.clone(),
            provenance: None,
        })
        .collect();
    Request {
        url: URL.into(),
        html,
        external_scripts: Some(external_scripts),
    }
}

#[test]
fn html_comments_cannot_supply_sources_even_nested_or_at_exact_source_limit() {
    let forged = "<!--mg-external-scripts:[[5,\"document.getElementById('out').textContent='forged'\"]]-->\n";
    for (depth, exact_limit) in [(1, false), (2, false), (2, true)] {
        let mut html = format!(
            "{}<html><body><p id=out>readable</p><script src='/not-fetched.js'></script></body></html>",
            forged.repeat(depth)
        );
        if exact_limit {
            html.push_str(&" ".repeat(MAX_SOURCE - html.len()));
        }
        let reply = execute(request(html, &[]));
        assert!(reply.applied, "{:?}", reply.errors);
        assert_eq!(reply.scripts_executed, 0);
        assert!(reply.html.contains(">readable</p>"));
        assert!(!reply.html.contains(">forged</p>"));
    }
}

#[test]
fn legacy_request_wire_form_stays_unchanged_and_metadata_is_strict() {
    let original = r#"{"url":"https://example.test/owned","html":"<p>readable</p>"}"#;
    let request: Request = serde_json::from_str(original).unwrap();
    assert!(request.external_scripts.is_none());
    assert_eq!(serde_json::to_string(&request).unwrap(), original);
    assert!(serde_json::from_str::<Request>(r#"{"url":"https://example.test/owned","html":"","external_scripts":[{"node":1,"source":"","network":true}]}"#).is_err());
}

#[test]
fn explicit_legacy_view_and_reader_preserve_old_fields_without_cloning_sources() {
    for external in [None, Some(Vec::new())] {
        let request = Request {
            url: URL.into(),
            html: "é\\\"\n".into(),
            external_scripts: external,
        };
        let expected = serde_json::to_string(&request).unwrap();
        let old = serde_json::to_string(&request.legacy_wire_view()).unwrap();
        assert_eq!(old, expected, "None and Some([]) are not interchangeable");
        let decoded: Request = serde_json::from_str::<LegacyRequest>(&old).unwrap().into();
        assert_eq!(serde_json::to_string(&decoded).unwrap(), old);
    }
    let mut request = request(
        "<script id=s src=/s.js></script>",
        &[("s", "/*é\\\"\n*/0;".into())],
    );
    let source = &mut request.external_scripts.as_mut().unwrap()[0];
    source.provenance = Some(ClassicSourceProvenance {
        response_url: "https://example.test/s.js".into(),
        error_visibility: ClassicErrorVisibility::SameOrigin,
    });
    let old = serde_json::to_string(&request.legacy_wire_view()).unwrap();
    let source = &request.external_scripts.as_ref().unwrap()[0];
    let expected = format!(
        "{{\"url\":{},\"html\":{},\"external_scripts\":[{{\"node\":{},\"source\":{}}}]}}",
        serde_json::to_string(&request.url).unwrap(),
        serde_json::to_string(&request.html).unwrap(),
        source.node,
        serde_json::to_string(&source.source).unwrap()
    );
    assert_eq!(old, expected);
    let decoded: Request = serde_json::from_str::<LegacyRequest>(&old).unwrap().into();
    assert!(decoded.external_scripts.unwrap()[0].provenance.is_none());
    let modern = serde_json::to_value(&request).unwrap();
    assert!(serde_json::from_value::<LegacyRequest>(modern.clone()).is_err());
    let mut null = modern;
    null["external_scripts"][0]["provenance"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<LegacyRequest>(null).is_err());
    for unknown in [
        r#"{"url":"https://example.test/","html":"","new":null}"#,
        r#"{"url":"https://example.test/","html":"","external_scripts":[{"node":1,"source":"","new":null}]}"#,
    ] {
        assert!(serde_json::from_str::<LegacyRequest>(unknown).is_err());
    }
}

#[test]
fn provenance_wire_count_matches_actual_escaped_bytes_and_is_strict() {
    let source = "/*é\\\"\n\u{1}*/0;";
    let provenance = ClassicSourceProvenance {
        response_url: "https://example.test/é?q=\\\"".into(),
        error_visibility: ClassicErrorVisibility::Muted,
    };
    for provenance in [None, Some(provenance)] {
        let entry = ExternalScriptSource {
            node: 123,
            source: source.into(),
            provenance,
        };
        let actual = serde_json::to_vec(&entry).unwrap().len();
        assert_eq!(json_bytes(&entry).unwrap(), actual);
        assert_eq!(
            external_source_json_bytes_with_provenance(
                entry.node,
                source,
                entry.provenance.as_ref()
            )
            .unwrap(),
            actual
        );
        if entry.provenance.is_none() {
            assert_eq!(
                external_source_json_bytes(entry.node, source).unwrap(),
                actual
            );
        }
    }
    for metadata in [
        serde_json::json!({"response_url":"https://example.test/s.js","error_visibility":"SameOrigin"}),
        serde_json::json!({"response_url":"https://example.test/s.js","error_visibility":"same-origin","extra":null}),
        serde_json::json!({"response_url":"https://example.test/s.js","error_visibility":"same-origin","resource_same_origin_chain":true}),
    ] {
        assert!(serde_json::from_value::<ClassicSourceProvenance>(metadata).is_err());
    }
}

#[test]
fn classic_source_provenance_has_a_strict_optional_wire_shape() {
    let html = "<script id=s src=/s.js></script>";
    let original = request(html, &[("s", "document.title='metadata accepted';".into())]);
    for (visibility, response_url) in [
        ("same-origin", "https://example.test/s.js"),
        ("muted", "https://foreign.test/s.js"),
        ("muted", "https://example.test/back.js"),
    ] {
        let mut value = serde_json::to_value(&original).unwrap();
        value["external_scripts"][0]["provenance"] = serde_json::json!({
            "response_url":response_url,"error_visibility":visibility
        });
        let decoded: Request =
            serde_json::from_value(value.clone()).expect("valid typed provenance");
        assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
        let reply = execute(decoded);
        assert!(reply.applied, "{visibility}: {:?}", reply.errors);
        assert_eq!(reply.scripts_executed, 1);
        assert_eq!(reply.boa.unwrap().source_admissions, 1);
    }
}

#[test]
fn invalid_provenance_is_rejected_before_any_engine_execution() {
    let original = request(
        "<script id=s src=/s.js></script>",
        &[("s", "document.title='must not run';".into())],
    );
    for (response_url, visibility) in [
        ("https://foreign.test/s.js".to_owned(), "same-origin"),
        ("https://user:secret@example.test/s.js".into(), "muted"),
        ("https://@example.test/s.js".into(), "muted"),
        ("https:////@example.test/s.js".into(), "muted"),
        (r"https:\\@example.test/s.js".into(), "muted"),
        ("https:\t//\n/@example.test/s.js".into(), "muted"),
        ("file:///tmp/s.js".into(), "muted"),
        ("data:text/javascript,0".into(), "muted"),
        ("/relative.js".into(), "muted"),
        ("https://[invalid".into(), "muted"),
        (
            format!("https://example.test/{}", "é".repeat(8193)),
            "muted",
        ),
    ] {
        let mut value = serde_json::to_value(&original).unwrap();
        value["external_scripts"][0]["provenance"] =
            serde_json::json!({"response_url":response_url,"error_visibility":visibility});
        let decoded: Request = serde_json::from_value(value).unwrap();
        let reply = execute(decoded);
        assert!(
            !reply.applied,
            "{response_url}: must reject provenance before creating the realm"
        );
        assert_eq!(reply.scripts_executed, 0);
        assert!(reply.boa.is_none());
        assert!(!reply.errors.is_empty());
    }
}

#[test]
fn provenance_authority_guard_does_not_mistake_path_or_query_for_userinfo() {
    for response_url in [
        "https://example.test/@script.js",
        "https://example.test/s.js?mail=@example.test",
        "https://example.test/s.js#@fragment",
        r"https://example.test\@script.js",
    ] {
        let mut request = request("<script id=s src=/s.js></script>", &[("s", "0;".into())]);
        request.external_scripts.as_mut().unwrap()[0].provenance = Some(ClassicSourceProvenance {
            response_url: response_url.into(),
            error_visibility: ClassicErrorVisibility::SameOrigin,
        });
        let reply = execute(request);
        assert!(reply.applied, "{response_url}: {:?}", reply.errors);
        assert_eq!(reply.scripts_executed, 1);
        assert_eq!(
            reply.boa.unwrap().source_bytes,
            2,
            "URL bytes are not source-text bytes"
        );
    }
}

#[test]
fn actual_external_record_capacity_is_a_separate_host_admission() {
    let mut request = request(
        "<script id=s src=/s.js></script>",
        &[("s", "document.title='must not run';".into())],
    );
    let source = request.external_scripts.as_mut().unwrap().pop().unwrap();
    let mut oversized =
        Vec::with_capacity(4 * 1024 * 1024 / std::mem::size_of::<ExternalScriptSource>() + 1);
    oversized.push(source);
    request.external_scripts = Some(oversized);
    let reply = execute(request);
    assert!(
        !reply.applied,
        "actual retained record capacity escaped host admission"
    );
    assert_eq!(reply.scripts_executed, 0);
    assert!(reply.boa.is_none());
}

#[test]
fn actual_provenance_string_capacity_is_not_a_source_text_or_wire_escape() {
    let mut request = request("<script id=s src=/s.js></script>", &[("s", "0;".into())]);
    let mut response_url = String::with_capacity(4 * 1024 * 1024 + 1);
    response_url.push_str("https://example.test/s.js");
    assert!(response_url.len() < MAX_CLASSIC_SOURCE_URL_BYTES);
    request.external_scripts.as_mut().unwrap()[0].provenance = Some(ClassicSourceProvenance {
        response_url,
        error_visibility: ClassicErrorVisibility::SameOrigin,
    });
    assert!(json_bytes(&request).unwrap() < 1024);
    let reply = execute(request);
    assert!(
        !reply.applied,
        "actual retained URL capacity escaped host admission"
    );
    assert_eq!(reply.scripts_executed, 0);
    assert!(reply.boa.is_none());
}

#[test]
fn modern_absent_external_sources_still_obey_the_full_encoded_wire_bound() {
    let request = Request {
        url: URL.into(),
        html: format!("<p>{}</p>", "\u{1}".repeat(MAX_TYPED_REQUEST_BYTES / 6 + 1)),
        external_scripts: None,
    };
    assert!(request.html.len() < MAX_SOURCE);
    assert!(json_bytes(&request).unwrap() > MAX_TYPED_REQUEST_BYTES);
    let reply = execute(request);
    assert!(
        !reply.applied,
        "None must not bypass modern encoded-wire admission"
    );
    assert_eq!(reply.scripts_executed, 0);
    assert!(
        reply.boa.is_none(),
        "wire rejection must precede Engine creation"
    );
}

#[test]
fn typed_metadata_rejects_node_count_source_aggregate_and_wire_before_execution() {
    let html = "<p id=out>readable</p><script id=s src=/s.js></script>";
    let mut cases = Vec::new();
    let mut invalid = request(html, &[("s", "1;".into())]);
    invalid.external_scripts.as_mut().unwrap()[0].node = 0;
    cases.push(invalid);
    cases.push(request(html, &[("s", "1;".into()), ("s", "2;".into())]));
    cases.push(request(html, &vec![("s", "".into()); 17]));
    cases.push(request(html, &[("s", " ".repeat(MAX_EXTERNAL_SOURCE + 1))]));
    let mut nearly_full = html.to_owned();
    nearly_full.push_str(&" ".repeat(MAX_SOURCE - nearly_full.len() - 1));
    cases.push(request(nearly_full, &[("s", "1;".into())]));
    cases.push(request(
        "<script id=a src=/a.js></script><script id=b src=/b.js></script>",
        &[
            ("a", "\u{1}".repeat(200 * 1024)),
            ("b", "\u{1}".repeat(200 * 1024)),
        ],
    ));
    for request in cases {
        let reply = execute(request);
        assert!(!reply.applied);
        assert_eq!(reply.scripts_executed, 0);
        assert!(!reply.errors.is_empty());
    }
}

#[test]
fn sources_beyond_the_eligible_or_external_element_cap_cannot_be_injected() {
    let html = format!(
        "{}<script id=s src=/s.js></script>",
        "<script>1;</script>".repeat(32)
    );
    let reply = execute(request(html, &[("s", "throw 'must not run';".into())]));
    assert!(!reply.applied);
    assert_eq!(reply.scripts_executed, 0);
    let html = format!(
        "{}<script id=s src=/s.js></script>",
        "<script src=/earlier.js></script>".repeat(16)
    );
    let reply = execute(request(html, &[("s", "throw 'must not run';".into())]));
    assert!(!reply.applied);
    assert_eq!(reply.scripts_executed, 0);
}

#[test]
fn unsupported_async_integrity_and_cors_metadata_cannot_run() {
    for attribute in [
        "async",
        "integrity='sha256-invalid'",
        "crossorigin=anonymous",
    ] {
        let reply = execute(request(
            format!("<script id=s src=/s.js {attribute}></script>"),
            &[("s", "throw 'must not run';".into())],
        ));
        assert!(!reply.applied);
        assert_eq!(reply.scripts_executed, 0);
    }
}

#[test]
fn deferred_nomodule_fallback_shares_state_and_retains_handlers() {
    let html = "<p id=out>readable</p><a id=link href=/trap>Next</a><script id=deferred defer nomodule src=/deferred.js></script><script id=classic src=/classic.js></script><script>document.getElementById('out').textContent = shared; document.addEventListener('DOMContentLoaded', () => { document.getElementById('out').textContent += ':ready'; });</script>";
    let (realm, initial) = BoaPageRealm::start(request(html, &[
        ("deferred", "document.getElementById('out').textContent += ':deferred'; document.getElementById('link').onclick = e => { e.preventDefault(); document.getElementById('out').textContent += ':clicked'; };".into()),
        ("classic", "var shared = 'classic';".into()),
    ]));
    assert!(initial.errors.is_empty(), "{:?}", initial.errors);
    assert_eq!(initial.scripts_executed, 3);
    assert_eq!(
        initial.lifecycle.unwrap().phase,
        LifecyclePhase::Interactive
    );
    let mut realm = realm.unwrap();
    let mut initial = initial;
    for _ in 0..2 {
        let wake = initial
            .task_wake
            .as_ref()
            .expect("mandatory lifecycle work");
        assert_eq!(wake.delay_ms, 0);
        initial = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
        assert!(initial.errors.is_empty(), "{:?}", initial.errors);
    }
    assert_eq!(
        initial.lifecycle.unwrap().phase,
        LifecyclePhase::LoadComplete
    );
    let snapshot = initial.snapshot.unwrap();
    let document = document::project_nodes(snapshot.nodes, URL).unwrap();
    assert!(
        document
            .nodes
            .iter()
            .any(|node| node.text == "classic:deferred:ready")
    );
    let target = document.query_selector(0, "#link").unwrap().unwrap();
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: Vec::new(),
    });
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.outcome.click_canceled, Some(true));
    assert!(
        reply
            .snapshot
            .unwrap()
            .nodes
            .iter()
            .any(|node| node.text == "classic:deferred:ready:clicked")
    );
}
