//! Independently authored acceptance of the retained realm's public typed API.
use mg_deps::{
    document::{self, Document},
    js_browser::{PageRealm, Request},
    page_session::{ControlEdit, DefaultAction, InputKind, RealmState, SessionInput, SessionReply},
};

const URL: &str = "http://127.0.0.1:7878/script-events";

fn start(html: &str) -> (PageRealm, SessionReply) {
    let (realm, reply) = PageRealm::start(Request {
        url: URL.into(),
        html: html.into(),
    });
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.revision, 0);
    (realm.expect("a retained realm"), reply)
}
fn page(script: &str) -> (PageRealm, SessionReply) {
    start(&format!(
        "<html><body><p id='state'></p><div id='outer'><form id='form' action='/submit'><input id='q' name='q' value='original'><button id='button' name='submit' value='yes'>Go</button></form></div><a id='link' href='/trap'>Link</a><script>{script}</script></body></html>"
    ))
}
fn projection(reply: &SessionReply) -> Document {
    document::project_nodes(
        reply.snapshot.as_ref().expect("snapshot").nodes.clone(),
        URL,
    )
    .unwrap()
}
fn node(reply: &SessionReply, selector: &str) -> usize {
    projection(reply)
        .query_selector(0, selector)
        .unwrap()
        .unwrap()
}
fn text(reply: &SessionReply, selector: &str) -> String {
    let doc = projection(reply);
    let mut pending = vec![doc.query_selector(0, selector).unwrap().unwrap()];
    let mut output = String::new();
    while let Some(id) = pending.pop() {
        if doc.nodes[id].tag == "#text" {
            output.push_str(&doc.nodes[id].text);
        }
        pending.extend(doc.nodes[id].children.iter().rev().copied());
    }
    output
}
fn click(realm: &mut PageRealm, target: usize) -> SessionReply {
    realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    })
}

#[test]
fn listener_receiver_normalization_keeps_window_target_in_later_dispatch() {
    let (mut realm, init) = page(
        r#"
        var add=addEventListener,remove=removeEventListener,count=0;
        function gone(){throw 'removed callback ran';}
        add('click',gone);remove('click',gone);
        function handle(event){
            if(this!==window||event.currentTarget!==window)throw 'wrong Window receiver';
            count++;event.preventDefault();
            document.getElementById('state').textContent='window:'+count;
        }
        add('click',handle);
        document.addEventListener.call(null,'click',handle);
        window.removeEventListener.call(undefined,'click',handle);
        document.addEventListener.call(undefined,'click',handle);
    "#,
    );
    let link = node(&init, "#link");
    let reply = click(&mut realm, link);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.state, RealmState::Ready);
    assert_eq!(reply.outcome.click_canceled, Some(true));
    assert_eq!(reply.default_action, DefaultAction::None);
    assert_eq!(text(&reply, "#state"), "window:1");
}

#[test]
fn frozen_fixture_requires_real_later_handlers_and_retains_original_node_ids() {
    let (mut realm, init) = start(include_str!("fixtures/script/events.html"));
    let query = node(&init, "#query");
    let button = node(&init, "#submit");
    let form = node(&init, "#search");
    let link = node(&init, "#cancel");
    let first = realm.dispatch(SessionInput {
        kind: InputKind::Click { target: link },
        edits: vec![ControlEdit {
            node: query,
            version: 7,
            value: "Rust & café".into(),
        }],
    });
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert_eq!(first.revision, 1);
    assert_eq!(first.outcome.click_canceled, Some(true));
    assert_eq!(first.default_action, DefaultAction::None);
    assert_eq!(text(&first, "#state"), "link canceled");
    assert_eq!(first.acknowledgements.len(), 1);
    assert_eq!(
        (
            first.acknowledgements[0].node,
            first.acknowledgements[0].version
        ),
        (query, 7)
    );
    let canceled = click(&mut realm, button);
    assert!(canceled.errors.is_empty(), "{:?}", canceled.errors);
    assert_eq!(canceled.outcome.click_canceled, Some(false));
    assert_eq!(canceled.outcome.submit_canceled, Some(true));
    assert_eq!(canceled.default_action, DefaultAction::None);
    assert_eq!(text(&canceled, "#state"), "first submit canceled");
    assert_eq!(
        node(&canceled, "#query"),
        query,
        "moving input must preserve ID"
    );
    let submitted = realm.dispatch(SessionInput {
        kind: InputKind::Submit {
            form,
            submitter: Some(button),
        },
        edits: vec![ControlEdit {
            node: query,
            version: 8,
            value: "Rust & café again".into(),
        }],
    });
    assert_eq!(submitted.revision, 3);
    assert!(submitted.errors.is_empty(), "{:?}", submitted.errors);
    assert_eq!(
        submitted.default_action,
        DefaultAction::SubmitForm {
            form,
            submitter: Some(button)
        }
    );
    let doc = projection(&submitted);
    assert_eq!(doc.forms[0].action, "http://127.0.0.1:7878/event-search");
    assert!(
        doc.forms[0]
            .fields
            .contains(&("proof".into(), "retained-1-2".into()))
    );
    assert_eq!(doc.nodes[query].attr("value"), Some("Rust & café again"));
    assert!(
        submitted.allocations.unwrap().accepted_bytes > init.allocations.unwrap().accepted_bytes
    );

    let (mut result_realm, init) = start(include_str!("fixtures/script/event-results.html"));
    let result = node(&init, "#result");
    let clicked = click(&mut result_realm, result);
    assert_eq!(
        clicked.default_action,
        DefaultAction::FollowLink { node: result }
    );
    assert_eq!(
        projection(&clicked).nodes[result].attr("href"),
        Some("/event-destination?proof=clicked")
    );
}

#[test]
fn capture_target_bubble_receivers_and_submitter_are_real() {
    let (mut realm, init) = page(
        r#"
var out='', s=document.getElementById('state'), b=document.getElementById('button'), f=document.getElementById('form');
function add(tag, receiver) { return function(e) {
 if(this!==receiver || e.currentTarget!==receiver || e.target!==b || e.type!=='click')throw 'identity';
 out+=tag+e.eventPhase; s.textContent=out;
}; }
window.addEventListener('click',add('w',window),true);
document.addEventListener('click',add('d',document),true);
f.addEventListener('click',add('f',f),true);
b.addEventListener('click',add('a',b),true);
b.addEventListener('click',add('b',b));
f.addEventListener('click',add('F',f));
document.addEventListener('click',add('D',document));
window.addEventListener('click',add('W',window));
f.onsubmit=function(e){if(e.target!==f || e.submitter!==b || !e.bubbles || !e.cancelable)throw 'submit identity';out+='S';s.textContent=out;e.preventDefault();};
"#,
    );
    let reply = click(&mut realm, node(&init, "#button"));
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(text(&reply, "#state"), "w1d1f1a2b2F3D3W3S");
    assert_eq!(reply.default_action, DefaultAction::None);
}

#[test]
fn removal_and_addition_recheck_listener_ids_without_replaying_startup() {
    let (mut realm, init) = page(
        r#"
var b=document.getElementById('link'),s=document.getElementById('state'),out='';
function second(){out+='B';s.textContent=out;}
function third(){out+='C';s.textContent=out;}
b.addEventListener('click',function(){out+='A';s.textContent=out;b.removeEventListener('click',second);b.addEventListener('click',third);});
b.addEventListener('click',second);b.addEventListener('click',second);
"#,
    );
    let target = node(&init, "#link");
    let first = click(&mut realm, target);
    assert_eq!(text(&first, "#state"), "A");
    let second = click(&mut realm, target);
    assert!(second.errors.is_empty(), "{:?}", second.errors);
    assert_eq!(text(&second, "#state"), "AAC");
}

#[test]
fn property_replacement_order_false_return_and_escaped_event_identity() {
    let (mut realm, init) = page(
        r#"
var b=document.getElementById('link'),s=document.getElementById('state'),out='',old=null;
b.addEventListener('click',function(e){
 if(old!==null && (old===e || old.currentTarget!==null || old.eventPhase!==0 || !old.defaultPrevented || old.target!==b))throw 'escaped event';
 out+='A';old=e;return false;
});
b.onclick=function(){out+='wrong';};
b.addEventListener('click',function(){out+='C';s.textContent=out;});
function replacement(){out+='B';return false;}
b.onclick=replacement;if(b.onclick!==replacement)throw 'property getter';
"#,
    );
    let target = node(&init, "#link");
    for expected in ["ABC", "ABCABC"] {
        let reply = click(&mut realm, target);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(text(&reply, "#state"), expected);
        assert_eq!(reply.outcome.click_canceled, Some(true));
        assert_eq!(reply.default_action, DefaultAction::None);
    }
}

#[test]
fn listener_false_does_not_cancel_and_script_navigation_overrides_default() {
    let (mut realm, init) = page(
        "document.getElementById('link').addEventListener('click',function(){return false;});",
    );
    let target = node(&init, "#link");
    let reply = click(&mut realm, target);
    assert_eq!(reply.outcome.click_canceled, Some(false));
    assert_eq!(
        reply.default_action,
        DefaultAction::FollowLink { node: target }
    );
    let (mut realm, init) = page(
        "document.getElementById('link').onclick=function(){location.href='/script-destination';};",
    );
    let reply = click(&mut realm, node(&init, "#link"));
    assert_eq!(
        reply.navigation.as_deref(),
        Some("http://127.0.0.1:7878/script-destination")
    );
    assert_eq!(reply.default_action, DefaultAction::None);
}

#[test]
fn stop_flags_are_distinct_and_do_not_implicitly_cancel_default() {
    for (method, expected) in [("stopPropagation", "AB"), ("stopImmediatePropagation", "A")] {
        let (mut realm, init) = page(&format!(
            r#"
var b=document.getElementById('link'),s=document.getElementById('state');
b.addEventListener('click',function(e){{s.textContent+='A';e.{method}();}});
b.addEventListener('click',function(){{s.textContent+='B';}});
document.addEventListener('click',function(){{s.textContent+='wrong';}});
"#
        ));
        let target = node(&init, "#link");
        let reply = click(&mut realm, target);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(text(&reply, "#state"), expected);
        assert_eq!(
            reply.default_action,
            DefaultAction::FollowLink { node: target }
        );
    }
}

#[test]
fn canceled_submit_button_click_never_delivers_submit() {
    let (mut realm, init) = page(
        r#"
document.getElementById('button').onclick=function(){return false;};
document.getElementById('form').onsubmit=function(){document.getElementById('state').textContent='wrong';};
"#,
    );
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Submit {
            form: node(&init, "#form"),
            submitter: Some(node(&init, "#button")),
        },
        edits: vec![],
    });
    assert_eq!(reply.outcome.click_canceled, Some(true));
    assert_eq!(reply.outcome.submit_canceled, None);
    assert_eq!(reply.default_action, DefaultAction::None);
    assert_eq!(text(&reply, "#state"), "");
}

#[test]
fn ordinary_callback_errors_continue_but_fuel_exhaustion_is_terminal() {
    let (mut realm, init) = page(
        r#"
var b=document.getElementById('link');b.addEventListener('click',function(){throw 'ordinary';});
b.addEventListener('click',function(){document.getElementById('state').textContent='continued';});
"#,
    );
    let target = node(&init, "#link");
    let reply = click(&mut realm, target);
    assert_eq!(reply.state, RealmState::Ready);
    assert_eq!(reply.errors.len(), 1);
    assert_eq!(text(&reply, "#state"), "continued");
    assert_eq!(
        reply.default_action,
        DefaultAction::FollowLink { node: target }
    );
    let (mut realm, init) = page(
        r#"
var b=document.getElementById('link');b.addEventListener('click',function(){while(true){}});
b.addEventListener('click',function(){document.getElementById('state').textContent='wrong';});
"#,
    );
    let target = node(&init, "#link");
    let reply = click(&mut realm, target);
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.navigation.is_none());
    if reply.snapshot.is_some() {
        assert_eq!(text(&reply, "#state"), "");
    }
    let next = click(&mut realm, target);
    assert_ne!(next.state, RealmState::Ready);
    assert_eq!(next.default_action, DefaultAction::None);
}

#[test]
fn invalid_edit_batch_is_rejected_atomically_and_cannot_enable_defaults() {
    let (mut realm, init) = page("");
    let query = node(&init, "#q");
    let link = node(&init, "#link");
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target: link },
        edits: vec![
            ControlEdit {
                node: query,
                version: 1,
                value: "would change".into(),
            },
            ControlEdit {
                node: usize::MAX,
                version: 2,
                value: "invalid".into(),
            },
        ],
    });
    assert_eq!(reply.state, RealmState::Closed);
    assert_eq!(reply.revision, 0);
    assert!(reply.snapshot.is_none());
    assert!(reply.acknowledgements.is_empty());
    assert_eq!(reply.default_action, DefaultAction::None);
    assert_ne!(click(&mut realm, link).state, RealmState::Ready);
}

#[test]
fn global_handler_assignment_getter_and_bound_callback_keep_identity() {
    let (mut realm, init) = page(
        r#"
var s=document.getElementById('state'),receiver={label:'bound'};
function listener(e){s.textContent=this.label;if(e.currentTarget!==window)throw 'target';return false;}
var bound=listener.bind(receiver);onclick=bound;if(window.onclick!==bound)throw 'global getter';
"#,
    );
    let reply = click(&mut realm, node(&init, "#link"));
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(text(&reply, "#state"), "bound");
    assert_eq!(reply.outcome.click_canceled, Some(true));
}

#[test]
fn separate_activations_consume_one_cumulative_fuel_budget() {
    let (mut realm, init) = page(
        r#"
var b=document.getElementById('link');
b.onclick=function(){var n=0;while(n<40000){n++;}document.getElementById('state').textContent='completed';return false;};
"#,
    );
    let target = node(&init, "#link");
    let first = click(&mut realm, target);
    assert_eq!(first.state, RealmState::Ready);
    let mut previous = first.allocations.unwrap().accepted_bytes;
    let mut exhausted = false;
    for _ in 0..7 {
        let reply = click(&mut realm, target);
        let accepted = reply.allocations.unwrap().accepted_bytes;
        assert!(
            accepted >= previous,
            "input must not renew allocation accounting"
        );
        previous = accepted;
        if reply.state == RealmState::Fatal {
            assert!(reply.errors.iter().any(|error| error.contains("fuel")));
            assert_eq!(reply.default_action, DefaultAction::None);
            exhausted = true;
            break;
        }
    }
    assert!(exhausted, "later input must not renew evaluator fuel");
}

#[test]
fn event_records_have_a_cumulative_lifetime_ceiling() {
    let (mut realm, init) = page("");
    let target = node(&init, "#outer");
    for revision in 1..=256 {
        let reply = click(&mut realm, target);
        assert_eq!(
            reply.state,
            RealmState::Ready,
            "event {revision}: {:?}",
            reply.errors
        );
        assert_eq!(reply.revision, revision);
    }
    let reply = click(&mut realm, target);
    assert_eq!(reply.state, RealmState::Closed);
    assert!(
        reply
            .errors
            .iter()
            .any(|error| error.contains("Event record limit"))
    );
    assert_eq!(reply.default_action, DefaultAction::None);
    assert_ne!(click(&mut realm, target).state, RealmState::Ready);
}

#[test]
fn typed_input_rejects_aggregate_oversize_not_just_each_field() {
    let mut html = String::from("<html><body><a id='link' href='/trap'>Link</a>");
    for index in 0..10 {
        html.push_str(&format!("<input id='q{index}' name='q{index}'>"));
    }
    html.push_str("</body></html>");
    let (mut realm, init) = start(&html);
    let edits = (0..10)
        .map(|index| ControlEdit {
            node: node(&init, &format!("#q{index}")),
            version: 1,
            value: "é".repeat(4000),
        })
        .collect();
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click {
            target: node(&init, "#link"),
        },
        edits,
    });
    assert_eq!(reply.state, RealmState::Closed);
    assert_eq!(reply.revision, 0);
    assert!(reply.snapshot.is_none());
    assert!(reply.acknowledgements.is_empty());
}

#[test]
fn fatal_initialization_cannot_propose_navigation() {
    let (_, reply) = PageRealm::start(Request { url: URL.into(), html:
        "<html><body><p>Readable fallback</p><script>location.href='/must-not-navigate';while(true){}</script></body></html>".into() });
    assert_eq!(reply.state, RealmState::Fatal);
    assert!(reply.navigation.is_none());
    assert_eq!(reply.default_action, DefaultAction::None);
}

#[test]
fn capture_identity_and_option_rejection_are_not_silent_success() {
    let (mut realm, init) = page(
        r#"
var link=document.getElementById('link'),s=document.getElementById('state'),caught=false;
function handler(e){s.textContent+='x';}
link.addEventListener('click',handler,true);
link.addEventListener('click',handler,false);
link.removeEventListener('click',handler,true);
try{link.addEventListener('click',handler,{capture:true});}catch(error){caught=true;}
if(!caught)throw 'Option objects must not be silently ignored';
"#,
    );
    let reply = click(&mut realm, node(&init, "#link"));
    assert_eq!(text(&reply, "#state"), "x");
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
}

#[test]
fn listener_slots_do_not_renew_after_removal() {
    let (mut realm, init) = page(
        r#"
var link=document.getElementById('link'),s=document.getElementById('state'),i=0,caught=false;
function handler(){}
while(i<32){link.addEventListener('click',handler);link.removeEventListener('click',handler);i++;}
try{link.addEventListener('click',handler);}catch(error){caught=true;}
if(!caught)throw 'Removed listeners renewed the registration budget';s.textContent='bounded';
"#,
    );
    let reply = click(&mut realm, node(&init, "#link"));
    assert_eq!(text(&reply, "#state"), "bounded");
}

#[test]
fn moving_target_during_dispatch_does_not_change_fixed_ancestor_path() {
    let (mut realm, init) = page(
        r#"
var button=document.getElementById('button'),s=document.getElementById('state'),form=document.getElementById('form');
form.addEventListener('click',function(){s.textContent+='capture';document.body.appendChild(button);},true);
form.addEventListener('click',function(){s.textContent+='bubble';});
"#,
    );
    let button = node(&init, "#button");
    let reply = click(&mut realm, button);
    assert_eq!(text(&reply, "#state"), "capturebubble");
    assert_eq!(node(&reply, "#button"), button);
    assert_eq!(
        reply.default_action,
        DefaultAction::None,
        "moved button has no form owner"
    );
}
