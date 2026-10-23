//! Additive preservation controls for docs/LINK_DEFAULTS.md.
//! Public library dispatch is not native hit testing or parent URL-policy proof.
#![cfg(feature = "modern")]

use mg_sparkle::{
    document::{self, Document},
    js_browser::{Request, boa::BoaPageRealm},
    page_session::{
        DefaultAction, InputKind, LifecyclePhase, RealmState, SessionInput, SessionReply, TaskPump,
    },
};

const URL: &str = "https://example.test/owned/document.html";
const BASE: &str = "<base id='base' href='/before/'>";

fn projected(reply: &SessionReply) -> Document {
    document::project_nodes(
        reply
            .snapshot
            .as_ref()
            .expect("real snapshot")
            .nodes
            .clone(),
        URL,
    )
    .expect("valid public arena projection")
}

fn node(reply: &SessionReply, selector: &str) -> usize {
    projected(reply)
        .query_selector(0, selector)
        .unwrap()
        .expect("connected authored node")
}

fn text(reply: &SessionReply, selector: &str) -> String {
    let doc = projected(reply);
    let mut pending = vec![doc.query_selector(0, selector).unwrap().unwrap()];
    let mut result = String::new();
    while let Some(id) = pending.pop() {
        if doc.nodes[id].tag == "#text" {
            result.push_str(&doc.nodes[id].text);
        }
        pending.extend(doc.nodes[id].children.iter().rev().copied());
    }
    result
}

fn work(reply: &SessionReply) -> (usize, usize, usize) {
    let report = reply.boa.as_ref().expect("actual modern report");
    assert!(reply.accounting_valid(), "{report:?}");
    (
        report.jobs_admitted,
        report.jobs_executed,
        report.pending_jobs,
    )
}

fn cumulative(previous: &SessionReply, reply: &SessionReply) {
    let before = previous.boa.as_ref().unwrap();
    let after = reply.boa.as_ref().unwrap();
    assert!(reply.accounting_valid(), "{after:?}");
    after.validate_after(before).unwrap();
    assert_eq!(after.source_admissions, before.source_admissions);
    assert_eq!(after.source_bytes, before.source_bytes);
    assert_eq!(reply.scripts_executed, 0);
    assert!(reply.acknowledgements.is_empty());
    assert_eq!(reply.lifecycle.unwrap().phase, LifecyclePhase::LoadComplete);
    assert!(!reply.lifecycle.unwrap().initial_failure);
}

fn ready(reply: &SessionReply) {
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.boa.as_ref().unwrap().fatal_reason.is_none());
    assert_eq!(reply.outcome.submit_canceled, None);
}

fn loaded(head: &str, anchor: &str, source: &str) -> (BoaPageRealm, SessionReply) {
    let source = format!(
        "var link=document.getElementById('link'),state=document.getElementById('state'),\n\
         left=document.getElementById('left'),right=document.getElementById('right'),\n\
         base=document.getElementById('base');\n{source}\ndocument.title='ready';"
    );
    let (realm, initial) = BoaPageRealm::start(Request {
        url: URL.into(),
        external_scripts: None,
        html: format!(
            "<!doctype html><html><head><title>before</title>{head}</head><body>\
             <div id='left'>{anchor}</div><div id='right'></div>\
             <a id='other' href='/unrelated'>Unrelated</a><p id='state'>before</p>\
             <script>{source}</script></body></html>"
        ),
    });
    ready(&initial);
    assert_eq!(initial.revision, 0);
    assert_eq!(initial.scripts_executed, 1);
    assert_eq!(initial.boa.as_ref().unwrap().source_admissions, 1);
    assert_eq!(initial.boa.as_ref().unwrap().source_bytes, source.len());
    assert_eq!(
        initial.lifecycle.unwrap().phase,
        LifecyclePhase::Interactive
    );
    assert!(!initial.lifecycle.unwrap().initial_failure);
    assert_eq!(initial.default_action, DefaultAction::None);
    assert!(initial.navigation.is_none());
    assert_eq!(work(&initial), (2, 0, 2));
    assert_eq!(projected(&initial).title, "ready");
    let mut realm = realm.expect("retained modern realm");
    let mut previous = initial;
    for (phase, expected_work) in [
        (LifecyclePhase::DomContentLoaded, (2, 1, 1)),
        (LifecyclePhase::LoadComplete, (2, 2, 0)),
    ] {
        let wake = previous
            .task_wake
            .as_ref()
            .expect("mandatory lifecycle task");
        assert_eq!(wake.delay_ms, 0);
        let reply = realm.pump_tasks(TaskPump {
            wake_token: wake.token,
        });
        ready(&reply);
        assert_eq!(reply.revision, previous.revision + 1);
        assert_eq!(reply.scripts_executed, 0);
        assert!(reply.acknowledgements.is_empty());
        assert_eq!(reply.outcome.click_canceled, None);
        assert_eq!(reply.default_action, DefaultAction::None);
        assert!(reply.navigation.is_none());
        assert_eq!(reply.lifecycle.unwrap().phase, phase);
        assert!(!reply.lifecycle.unwrap().initial_failure);
        assert_eq!(work(&reply), expected_work);
        let report = reply.boa.as_ref().unwrap();
        report
            .validate_after(previous.boa.as_ref().unwrap())
            .unwrap();
        assert_eq!(report.source_admissions, 1);
        assert_eq!(report.source_bytes, source.len());
        previous = reply;
    }
    assert!(previous.task_wake.is_none());
    (realm, previous)
}

fn link_page(body: &str, source: &str) -> (BoaPageRealm, SessionReply) {
    loaded(
        BASE,
        &format!("<a id='link' href='next'>{body}</a>"),
        source,
    )
}

fn click(realm: &mut BoaPageRealm, previous: &SessionReply, target: usize) -> SessionReply {
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    assert_eq!(reply.revision, previous.revision + 1);
    cumulative(previous, &reply);
    reply
}

fn follows(reply: &SessionReply, anchor: usize, expected_work: (usize, usize, usize)) {
    ready(reply);
    assert_eq!(reply.outcome.click_canceled, Some(false));
    assert_eq!(
        reply.default_action,
        DefaultAction::FollowLink { node: anchor }
    );
    assert!(reply.navigation.is_none());
    assert_eq!(work(reply), expected_work);
}

#[test]
fn empty_whitespace_nontext_and_text_anchors_keep_exact_arena_identity() {
    for body in [
        "",
        " \n\t ",
        "<span id='inside' style='display:block;width:20px;height:20px'></span>",
        "Visible text",
        "<span id='inside'>Nested text</span>",
    ] {
        let (mut realm, loaded) = link_page(body, "");
        let target = node(&loaded, "#link");
        let unrelated = node(&loaded, "#other");
        let reply = click(&mut realm, &loaded, target);
        follows(&reply, target, (2, 2, 0));
        assert_ne!(target, unrelated);
        assert_eq!(node(&reply, "#link"), target);
        assert_eq!(projected(&reply).nodes[target].attr("href"), Some("next"));
        assert!(reply.task_wake.is_none());
    }
}

#[test]
fn nontext_descendant_target_activates_its_original_anchor_not_another_link() {
    let (mut realm, loaded) = link_page(
        "<span id='inside' style='display:block;width:20px;height:20px'></span>",
        r#"
        var inside=document.getElementById('inside');
        link.onclick=function(event){
            if(this!==link || event.currentTarget!==link || event.target!==inside)
                throw 'wrong retained target';
            link.href='/descendant-selected';state.textContent='same anchor';
        };
        "#,
    );
    let anchor = node(&loaded, "#link");
    let target = node(&loaded, "#inside");
    let reply = click(&mut realm, &loaded, target);
    follows(&reply, anchor, (2, 2, 0));
    assert_ne!(anchor, target);
    assert_eq!(text(&reply, "#state"), "same anchor");
    assert_eq!(
        projected(&reply).nodes[anchor].attr("href"),
        Some("/descendant-selected")
    );
}

#[test]
fn present_empty_trimmed_relative_and_absolute_hrefs_are_not_absent() {
    for href in [
        "",
        "next",
        " \t/trimmed\n ",
        "https://destination.test/path",
    ] {
        let (mut realm, loaded) = loaded(BASE, &format!("<a id='link' href='{href}'></a>"), "");
        let target = node(&loaded, "#link");
        let reply = click(&mut realm, &loaded, target);
        follows(&reply, target, (2, 2, 0));
        assert_eq!(projected(&reply).nodes[target].attr("href"), Some(href));
    }
}

#[test]
fn direct_handler_changes_href_and_base_before_default_selection() {
    let (mut realm, loaded) = link_page(
        "",
        r#"
        link.onclick=function(){
            base.href='/direct-base/';link.href='selected?proof=direct';
            state.textContent='direct';
        };
        "#,
    );
    let target = node(&loaded, "#link");
    let reply = click(&mut realm, &loaded, target);
    follows(&reply, target, (2, 2, 0));
    let doc = projected(&reply);
    assert_eq!(doc.base_url, "https://example.test/direct-base/");
    assert_eq!(
        doc.nodes[target].attr("href"),
        Some("selected?proof=direct")
    );
    assert_eq!(text(&reply, "#state"), "direct");
}

#[test]
fn promise_checkpoint_changes_href_and_base_before_default_selection() {
    let (mut realm, loaded) = link_page(
        "",
        r#"
        link.onclick=function(){
            Promise.resolve().then(function(){
                base.href='/promise-base/';link.href='selected?proof=promise';
                state.textContent='promise';
            });
        };
        "#,
    );
    let target = node(&loaded, "#link");
    let reply = click(&mut realm, &loaded, target);
    follows(&reply, target, (3, 3, 0));
    let doc = projected(&reply);
    assert_eq!(doc.base_url, "https://example.test/promise-base/");
    assert_eq!(
        doc.nodes[target].attr("href"),
        Some("selected?proof=promise")
    );
    assert_eq!(text(&reply, "#state"), "promise");
    assert!(reply.task_wake.is_none());
}

#[test]
fn base_reorder_removal_and_detachment_use_connected_post_handler_tree_order() {
    for (mutation, expected_base) in [
        (
            "base.parentNode.appendChild(base);",
            "https://example.test/second/",
        ),
        (
            "base.parentNode.removeChild(base);",
            "https://example.test/second/",
        ),
        (
            "base.removeAttribute('href');",
            "https://example.test/second/",
        ),
        (
            "base.parentNode.removeChild(base);var b=document.getElementById('second');b.parentNode.removeChild(b);",
            URL,
        ),
    ] {
        let (mut realm, loaded) = loaded(
            "<base id='base' href='/before/'><base id='second' href='/second/'>",
            "<a id='link' href='selected'></a>",
            &format!("link.onclick=function(){{{mutation}state.textContent='base changed';}};"),
        );
        let target = node(&loaded, "#link");
        let reply = click(&mut realm, &loaded, target);
        follows(&reply, target, (2, 2, 0));
        assert_eq!(projected(&reply).base_url, expected_base);
        assert_eq!(
            projected(&reply).nodes[target].attr("href"),
            Some("selected")
        );
        assert_eq!(text(&reply, "#state"), "base changed");
    }
}

#[test]
fn text_removal_hiding_move_and_detach_reinsert_preserve_connected_anchor() {
    for mutation in [
        "link.textContent='';",
        "link.setAttribute('hidden','');",
        "link.style.display='none';",
        "right.appendChild(link);",
        "left.removeChild(link);right.appendChild(link);",
    ] {
        let (mut realm, loaded) = link_page(
            "Text before handler",
            &format!(
                "link.onclick=function(){{{mutation}link.href='/connected-selected';state.textContent='committed';}};"
            ),
        );
        let target = node(&loaded, "#link");
        let reply = click(&mut realm, &loaded, target);
        follows(&reply, target, (2, 2, 0));
        assert_eq!(node(&reply, "#link"), target);
        assert_eq!(
            projected(&reply).nodes[target].attr("href"),
            Some("/connected-selected")
        );
        assert_eq!(text(&reply, "#state"), "committed");
    }
}

#[test]
fn permanent_detachment_commits_valid_effects_but_suppresses_default() {
    let (mut realm, loaded) = link_page(
        "Text",
        "link.onclick=function(){left.removeChild(link);link.href='/detached';state.textContent='committed';};",
    );
    let target = node(&loaded, "#link");
    let reply = click(&mut realm, &loaded, target);
    ready(&reply);
    assert_eq!(reply.outcome.click_canceled, Some(false));
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.navigation.is_none());
    assert_eq!(work(&reply), (2, 2, 0));
    let doc = projected(&reply);
    assert_eq!(doc.query_selector(0, "#link").unwrap(), None);
    assert_eq!(doc.nodes[target].attr("href"), Some("/detached"));
    assert_eq!(text(&reply, "#state"), "committed");
}

#[test]
fn direct_and_promise_href_removal_suppress_without_erasing_dom_effects() {
    for (source, expected_work) in [
        (
            "link.onclick=function(){link.removeAttribute('href');state.textContent='committed';};",
            (2, 2, 0),
        ),
        (
            "link.onclick=function(){Promise.resolve().then(function(){link.removeAttribute('href');state.textContent='committed';});};",
            (3, 3, 0),
        ),
    ] {
        let (mut realm, loaded) = link_page("", source);
        let target = node(&loaded, "#link");
        let reply = click(&mut realm, &loaded, target);
        ready(&reply);
        assert_eq!(reply.outcome.click_canceled, Some(false));
        assert_eq!(reply.default_action, DefaultAction::None);
        assert!(reply.navigation.is_none());
        assert_eq!(work(&reply), expected_work);
        assert_eq!(node(&reply, "#link"), target);
        assert_eq!(projected(&reply).nodes[target].attr("href"), None);
        assert_eq!(text(&reply, "#state"), "committed");
    }
}

#[test]
fn same_html_id_replacement_does_not_inherit_original_activation_identity() {
    let (mut realm, loaded) = link_page(
        "Original",
        r#"
        link.onclick=function(){
            var replacement=document.createElement('a');
            replacement.id='link';replacement.href='/replacement';
            replacement.textContent='Replacement';right.appendChild(replacement);
            left.removeChild(link);state.textContent='replaced';
        };
        "#,
    );
    let original = node(&loaded, "#link");
    let reply = click(&mut realm, &loaded, original);
    ready(&reply);
    assert_eq!(reply.outcome.click_canceled, Some(false));
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.navigation.is_none());
    assert_eq!(work(&reply), (2, 2, 0));
    let replacement = node(&reply, "#link");
    assert_ne!(replacement, original);
    assert_eq!(projected(&reply).nodes[original].attr("href"), Some("next"));
    assert_eq!(
        projected(&reply).nodes[replacement].attr("href"),
        Some("/replacement")
    );
    assert_eq!(text(&reply, "#state"), "replaced");
    let second = click(&mut realm, &reply, replacement);
    follows(&second, replacement, (2, 2, 0));
}

#[test]
fn cancellation_and_property_false_commit_edits_but_listener_false_does_not_cancel() {
    for (registration, canceled) in [
        (
            "link.addEventListener('click',function(event){link.href='/edited';state.textContent='committed';event.preventDefault();});",
            true,
        ),
        (
            "link.onclick=function(){link.href='/edited';state.textContent='committed';return false;};",
            true,
        ),
        (
            "link.addEventListener('click',function(){link.href='/edited';state.textContent='committed';return false;});",
            false,
        ),
    ] {
        let (mut realm, loaded) = link_page("", registration);
        let target = node(&loaded, "#link");
        let reply = click(&mut realm, &loaded, target);
        ready(&reply);
        assert_eq!(reply.outcome.click_canceled, Some(canceled));
        assert_eq!(
            reply.default_action,
            if canceled {
                DefaultAction::None
            } else {
                DefaultAction::FollowLink { node: target }
            }
        );
        assert!(reply.navigation.is_none());
        assert_eq!(work(&reply), (2, 2, 0));
        assert_eq!(
            projected(&reply).nodes[target].attr("href"),
            Some("/edited")
        );
        assert_eq!(text(&reply, "#state"), "committed");
    }
}

#[test]
fn ordinary_throw_retains_diagnostic_and_later_listener_before_default() {
    let (mut realm, loaded) = link_page(
        "",
        r#"
        window.onerror=function(){return true;};
        link.onclick=function(){link.href='/ordinary-selected';state.textContent='first';throw 'owned';};
        link.addEventListener('click',function(){state.textContent+=':continued';});
        "#,
    );
    let target = node(&loaded, "#link");
    let reply = click(&mut realm, &loaded, target);
    assert_eq!(reply.state, RealmState::Ready);
    assert_eq!(
        reply.errors,
        ["Event handler: ThrownValue: JavaScript threw a value (not coerced by the host)"]
    );
    assert_eq!(reply.outcome.click_canceled, Some(false));
    assert_eq!(
        reply.default_action,
        DefaultAction::FollowLink { node: target }
    );
    assert!(reply.navigation.is_none());
    assert!(reply.boa.as_ref().unwrap().fatal_reason.is_none());
    assert_eq!(work(&reply), (2, 2, 0));
    assert_eq!(
        projected(&reply).nodes[target].attr("href"),
        Some("/ordinary-selected")
    );
    assert_eq!(text(&reply, "#state"), "first:continued");
}

#[test]
fn fatal_handler_discards_defaults_and_script_navigation_without_js_reporter() {
    let (mut realm, loaded) = link_page(
        "",
        r#"
        window.onerror=function(){state.textContent='wrong reporter';return true;};
        link.onclick=function(){
            link.href='/fatal-link';state.textContent='before fatal';
            location.href='/fatal-script';while(true){}
        };
        link.addEventListener('click',function(){state.textContent='wrong later listener';});
        "#,
    );
    let target = node(&loaded, "#link");
    let reply = click(&mut realm, &loaded, target);
    assert_eq!(reply.state, RealmState::Fatal);
    assert!(!reply.errors.is_empty());
    assert!(reply.boa.as_ref().unwrap().fatal_reason.is_some());
    assert_eq!(reply.boa.as_ref().unwrap().opcodes_remaining, 0);
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.navigation.is_none());
    assert!(reply.task_wake.is_none());
    assert_eq!(work(&reply), (2, 2, 0));
    if reply.snapshot.is_some() {
        assert_eq!(text(&reply, "#state"), "before fatal");
    }
    let next = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    assert_eq!(next.state, RealmState::Fatal);
    assert_eq!(next.revision, reply.revision);
    assert_eq!(next.default_action, DefaultAction::None);
    assert!(next.navigation.is_none());
    assert!(next.task_wake.is_none());
    cumulative(&reply, &next);
    assert_eq!(
        next.boa.as_ref().unwrap().fatal_reason,
        reply.boa.as_ref().unwrap().fatal_reason
    );
}

#[test]
fn direct_and_promise_script_navigation_suppress_link_default() {
    for (source, expected_navigation, expected_work) in [
        (
            "link.onclick=function(){link.href='ftp://unused.test/link';state.textContent='direct';location.href='/explicit?proof=direct';};",
            "https://example.test/explicit?proof=direct",
            (2, 2, 0),
        ),
        (
            "link.onclick=function(){Promise.resolve().then(function(){link.href='https://user:pass@unused.test/link';state.textContent='promise';location.href='/explicit?proof=promise';});};",
            "https://example.test/explicit?proof=promise",
            (3, 3, 0),
        ),
    ] {
        let (mut realm, loaded) = link_page("", source);
        let target = node(&loaded, "#link");
        let reply = click(&mut realm, &loaded, target);
        ready(&reply);
        assert_eq!(reply.outcome.click_canceled, Some(false));
        assert_eq!(reply.default_action, DefaultAction::None);
        assert_eq!(reply.navigation.as_deref(), Some(expected_navigation));
        assert_eq!(work(&reply), expected_work);
        assert!(reply.task_wake.is_none());
        assert_eq!(node(&reply, "#link"), target);
    }
}

#[test]
fn later_timer_does_not_delay_or_rewrite_current_link_default() {
    let (mut realm, loaded) = link_page(
        "",
        r#"
        link.onclick=function(){
            link.href='/now';state.textContent='now';
            setTimeout(function(){link.href='/late';state.textContent='late';},60000);
        };
        "#,
    );
    let target = node(&loaded, "#link");
    let reply = click(&mut realm, &loaded, target);
    follows(&reply, target, (3, 2, 1));
    assert_eq!(projected(&reply).nodes[target].attr("href"), Some("/now"));
    assert_eq!(text(&reply, "#state"), "now");
    let wake = reply.task_wake.as_ref().expect("unexecuted later timer");
    assert!(wake.delay_ms > 0 && wake.delay_ms <= 60000);
    // Intentionally do not pump or sleep: the accepted click default is current.
}
