//! Actual modern language execution mutates the existing Rust DOM projection.
#![cfg(feature = "modern")]
use mg_sparkle::{
    document,
    js_browser::{
        Request,
        boa::{BoaPageRealm, execute},
    },
    page_session::*,
};

fn page(script: &str) -> Request {
    Request {
        url: "https://example.test/owned".into(),
        html: format!(
            "<html><head><title>Before</title></head><body><p id=out>Before</p><a id=link href=/trap>Next</a><script>{script}</script></body></html>"
        ),
    }
}
fn projected(reply: &SessionReply) -> document::Document {
    document::project_nodes(
        reply.snapshot.as_ref().unwrap().nodes.clone(),
        "https://example.test/owned",
    )
    .unwrap()
}

#[test]
fn native_journey_fixture_creates_real_form_through_boa() {
    let reply = execute(Request {
        url: "http://127.0.0.1:7878/script-boa".into(),
        html: include_str!("../../../tests/fixtures/script/boa.html").into(),
    });
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 2);
    let doc = document::parse(&reply.html, "http://127.0.0.1:7878/script-boa");
    assert_eq!(doc.title, "Boa JavaScript page ready");
    assert_eq!(doc.forms[0].action, "http://127.0.0.1:7878/search");
    assert!(
        doc.forms[0]
            .fields
            .iter()
            .any(|(name, value)| name == "source" && value == "fixture")
    );
}

#[test]
fn modern_syntax_promises_and_shared_script_state_create_real_controls() {
    let mut input = page(
        r#"
        class Counter { constructor() { this.value = 40n; } next() { return ++this.value; } }
        const counter = new Counter();
        const { word } = { word: 'Rust & café' };
        const values = new Map([['query', word]]);
        const make = tag => document.createElement(tag);
        const form = make('form'); form.action = '/search';
        const q = make('input'); q.name = 'q'; q.value = `${values.get('query')}:${counter.next()}`;
        form.appendChild(q); document.body.appendChild(form);
        Promise.resolve().then(() => { document.title = 'Boa ready'; q.value += ':promise'; });
    "#,
    );
    input.html.push_str(
        "<script>document.getElementById('out').textContent = `${counter.next()}`;</script>",
    );
    let reply = execute(input);
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 2);
    assert!(
        reply.allocations.is_none(),
        "must not fabricate original evaluator accounting"
    );
    assert!(reply.boa.as_ref().unwrap().is_valid());
    let doc = document::parse(&reply.html, "https://example.test/owned");
    assert_eq!(doc.title, "Boa ready");
    assert_eq!(doc.forms[0].action, "https://example.test/search");
    let query = doc.query_selector(0, "input").unwrap().unwrap();
    assert_eq!(doc.nodes[query].attr("name"), Some("q"));
    assert_eq!(
        doc.nodes[query].attr("value"),
        Some("Rust & café:41:promise")
    );
    assert!(reply.html.contains(">42</p>"));
}

#[test]
fn promises_from_later_handlers_update_dom_before_default_navigation() {
    let (realm, initial) = BoaPageRealm::start(page(
        r#"
        let clicks = 0;
        const link = document.getElementById('link');
        link.onclick = event => {
            clicks++;
            Promise.resolve().then(() => {
                link.href = `/destination?clicks=${clicks}`;
                document.title = `Clicked ${clicks}`;
            });
        };
    "#,
    ));
    assert!(initial.errors.is_empty(), "{:?}", initial.errors);
    let mut realm = realm.unwrap();
    let target = projected(&initial)
        .query_selector(0, "#link")
        .unwrap()
        .unwrap();
    for count in 1..=2 {
        let reply = realm.dispatch(SessionInput {
            kind: InputKind::Click { target },
            edits: Vec::new(),
        });
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(
            reply.default_action,
            DefaultAction::FollowLink { node: target }
        );
        assert_eq!(projected(&reply).title, format!("Clicked {count}"));
        assert_eq!(
            projected(&reply).nodes[target].attr("href"),
            Some(format!("/destination?clicks={count}").as_str())
        );
    }
}

#[test]
fn callback_coercion_reenters_the_same_dom_without_a_borrow_panic() {
    let reply = execute(page(
        r#"
        const out = document.getElementById('out');
        out.textContent = {
            toString() { out.setAttribute('data-proof', { toString() { return 'nested'; } }); return 'outer'; }
        };
        if (out !== document.querySelector('#out')) throw 'identity';
        if (document.body.parentNode.parentNode !== document) throw 'document identity';
        const snapshot = document.querySelectorAll('p');
        if (Array.prototype.map.call(snapshot, node => node)[0] !== out) throw 'collection identity';
    "#,
    ));
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.html.contains("data-proof=\"nested\">outer</p>"));
}

#[test]
fn dom_string_conversion_replaces_lone_surrogates_and_ignores_unused_arguments() {
    let reply = execute(page(
        r#"
        const poison = { toString() { throw 'unused conversion'; } };
        const out = document.getElementById('out', poison);
        out.setAttribute('proof', '\ud800', poison);
        out.textContent = '\udc00';
        let rejected = false;
        try { out.textContent = Symbol('must reject'); } catch (error) { rejected = true; }
        if (!rejected) throw 'Symbol accepted';
    "#,
    ));
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.html.contains("proof=\"�\">�</p>"));
}

#[test]
fn startup_uses_actual_global_and_guards_an_onload_getter() {
    let reply = execute(page(
        r#"
        globalThis = {};
        Object.defineProperty(window, 'onload', { get() {
            document.getElementById('out').setAttribute('proof', 'getter');
            return () => document.title = 'actual global load';
        }});
    "#,
    ));
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.html.contains("<title>actual global load</title>"));
    assert!(reply.html.contains("proof=\"getter\""));
}

#[test]
fn inherited_accessors_keep_wrapper_receiver_and_cannot_expose_private_target() {
    let reply = execute(page(
        r#"
        const symbol = Symbol('receiver');
        const reflectGet = Reflect.get;
        Object.defineProperty(Object.prototype, 'domReceiver', {
            configurable: true,
            get() { this.setAttribute('proof', 'reentered'); return this; }
        });
        Object.defineProperty(Object.prototype, symbol, {
            configurable: true, get() { return this; }
        });
        Reflect.get = () => { throw 'mutable Reflect.get must not run'; };
        const body = document.body;
        if (body.domReceiver !== body || body[symbol] !== body) throw 'private target exposed';
        const alternate = { marker: 42 };
        if (reflectGet(body, symbol, alternate) !== alternate) throw 'explicit receiver lost';
        let rejected = false;
        try { Object.defineProperty(body.domReceiver, 'escape', { value: true }); }
        catch (error) { rejected = true; }
        if (!rejected) throw 'private target bypassed wrapper mutation policy';
        delete Object.prototype.domReceiver;
        delete Object.prototype[symbol];
        document.title = 'wrapper receivers retained';
    "#,
    ));
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(
        reply
            .html
            .contains("<title>wrapper receivers retained</title>")
    );
    assert!(reply.html.contains("<body proof=\"reentered\">"));
}

#[test]
fn unsupported_loading_and_network_capabilities_are_explicit() {
    let mut input = page(
        "if (typeof fetch !== 'undefined') throw 'unexpected network authority'; document.title='inline';",
    );
    input.html.push_str("<script src='/external.js'></script><script type=module>document.title='module ran';</script>");
    let reply = execute(input);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(reply.errors.len(), 2);
    assert!(reply.errors.iter().any(|e| e.contains("External script")));
    assert!(reply.errors.iter().any(|e| e.contains("Module scripts")));
    assert!(reply.html.contains("<title>inline</title>"));
}

#[test]
fn fatal_opcode_exhaustion_stops_later_scripts_and_navigation() {
    let mut input = page("try { while(true) {} } catch(error) { location='/caught'; }");
    input
        .html
        .push_str("<script>location='/later';document.title='must not run';</script>");
    let (realm, reply) = BoaPageRealm::start(input);
    assert_eq!(reply.state, RealmState::Fatal);
    assert!(reply.navigation.is_none());
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(projected(&reply).title, "Before");
    assert!(reply.boa.unwrap().fatal_reason.is_some());
    assert!(realm.is_some(), "readable partial document is retained");
}

#[test]
fn source_and_projection_rejection_keep_original_fallback() {
    let mut input = page("location='/next';");
    input.html.insert_str(0, &"\"".repeat(400_000));
    let original = input.html.clone();
    let reply = execute(input);
    assert!(!reply.applied);
    assert_eq!(reply.html, original);
    assert!(reply.navigation.is_none());
    assert!(
        reply
            .errors
            .iter()
            .any(|e| e.contains("Serialized DOM byte limit"))
    );
}
