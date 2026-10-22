//! Parent source admission is independent of page-controlled HTML comments.
#![cfg(feature = "modern")]
use mg_sparkle::{
    document,
    js_browser::{
        ExternalScriptSource, MAX_EXTERNAL_SOURCE, MAX_SOURCE, Request,
        boa::{BoaPageRealm, execute},
    },
    page_session::{InputKind, SessionInput},
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
    let snapshot = initial.snapshot.unwrap();
    let document = document::project_nodes(snapshot.nodes, URL).unwrap();
    assert!(
        document
            .nodes
            .iter()
            .any(|node| node.text == "classic:deferred:ready")
    );
    let target = document.query_selector(0, "#link").unwrap().unwrap();
    let reply = realm.unwrap().dispatch(SessionInput {
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
