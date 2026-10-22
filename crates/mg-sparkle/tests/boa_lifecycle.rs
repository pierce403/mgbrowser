#![cfg(feature = "modern")]
//! Frozen authored snapshot-lifecycle controls, not upstream WPT/testharness results.
use mg_sparkle::{
    document,
    js_browser::{ExternalScriptSource, Request, boa::BoaPageRealm},
    page_session::{DefaultAction, LifecyclePhase as Phase, RealmState, SessionReply, TaskPump},
};

const URL: &str = "https://example.test/owned-lifecycle";
fn start(source: &str) -> (BoaPageRealm, SessionReply) {
    let (realm, reply) = BoaPageRealm::start(Request {
        url: URL.into(),
        external_scripts: None,
        html: format!("<title>before</title><p>Owned control</p><script>{source}</script>"),
    });
    (realm.expect("readable owned realm"), reply)
}
fn title(reply: &SessionReply) -> String {
    document::project_nodes(reply.snapshot.as_ref().unwrap().nodes.clone(), URL)
        .unwrap()
        .title
}
fn phase(reply: &SessionReply) -> Phase {
    reply.lifecycle.expect("modern progress").phase
}
fn pump(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let wake = previous
        .task_wake
        .as_ref()
        .expect("mandatory work advertised");
    assert_eq!(wake.delay_ms, 0, "only immediate tasks in owned control");
    let reply = realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    });
    assert_eq!(reply.revision, previous.revision + 1);
    assert_eq!(reply.scripts_executed, 0);
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.acknowledgements.is_empty());
    assert!(reply.outcome.click_canceled.is_none() && reply.outcome.submit_canceled.is_none());
    reply
}
fn work(reply: &SessionReply) -> (usize, usize, usize) {
    let report = reply.boa.as_ref().unwrap();
    (
        report.jobs_admitted,
        report.jobs_executed,
        report.pending_jobs,
    )
}

#[test]
fn unobserved_lifecycle_still_spends_two_real_permits_in_two_later_pumps() {
    let (mut realm, init) = start("");
    assert_eq!(init.state, RealmState::Ready);
    assert!(init.errors.is_empty());
    assert_eq!(phase(&init), Phase::Interactive);
    assert_eq!(work(&init), (2, 0, 2));
    let dcl = pump(&mut realm, &init);
    assert_eq!(phase(&dcl), Phase::DomContentLoaded);
    assert_eq!(work(&dcl), (2, 1, 1));
    let load = pump(&mut realm, &dcl);
    assert_eq!(phase(&load), Phase::LoadComplete);
    assert_eq!(work(&load), (2, 2, 0));
    assert!(!load.lifecycle.unwrap().initial_failure && load.task_wake.is_none());
}

#[test]
fn deferred_readiness_and_real_callback_checkpoints_are_separate_from_queued_events() {
    let html = r#"<title>before</title><script>
        var order=['script:'+document.readyState];
        document.onreadystatechange=()=>{order.push('ready:'+document.readyState);
            Promise.resolve().then(()=>order.push('ready-job:'+document.readyState));};
        window.addEventListener('DOMContentLoaded',e=>{if(e.eventPhase!==1||e.target!==document)throw 'capture';order.push('capture');},true);
        document.addEventListener('DOMContentLoaded',e=>{
            if(e.eventPhase!==2||!e.bubbles||e.cancelable||e.currentTarget!==document)throw 'DCL metadata';
            order.push('dcl:'+document.readyState);Promise.resolve().then(()=>order.push('dcl-job'));});
        window.addEventListener('DOMContentLoaded',()=>{order.push('bubble');document.title=order.join('|');});
        document.addEventListener('load',()=>{throw 'load dispatched at document';});
        window.onload=function(e){if(this!==window||e.target!==document||e.currentTarget!==window||
            e.eventPhase!==2||e.bubbles||e.cancelable)throw 'load metadata';
            e.preventDefault();if(e.defaultPrevented)throw 'load canceled';
            order.push('load:'+document.readyState);Promise.resolve().then(()=>document.title=order.join('|')+'|load-job');return false;};
    </script><script id=defer src='/owned.js' defer></script>"#;
    let parsed = document::parse_with_scripting(html, URL, true);
    let node = parsed.query_selector(0, "#defer").unwrap().unwrap();
    let (realm, init) = BoaPageRealm::start(Request {
        url: URL.into(),
        html: html.into(),
        external_scripts: Some(vec![ExternalScriptSource {
            node,
            source: "order.push('defer:'+document.readyState);document.title=order.join('|');"
                .into(),
            provenance: None,
        }]),
    });
    let mut realm = realm.unwrap();
    assert_eq!(phase(&init), Phase::Interactive);
    assert_eq!(
        title(&init),
        "script:loading|ready:interactive|ready-job:interactive|defer:interactive"
    );
    let dcl = pump(&mut realm, &init);
    assert_eq!(phase(&dcl), Phase::DomContentLoaded);
    assert_eq!(
        title(&dcl),
        "script:loading|ready:interactive|ready-job:interactive|defer:interactive|capture|dcl:interactive|dcl-job|bubble"
    );
    let load = pump(&mut realm, &dcl);
    assert!(load.errors.is_empty(), "{:?}", load.errors);
    assert_eq!(phase(&load), Phase::LoadComplete);
    assert_eq!(
        title(&load),
        "script:loading|ready:interactive|ready-job:interactive|defer:interactive|capture|dcl:interactive|dcl-job|bubble|ready:complete|ready-job:complete|load:complete|load-job"
    );
}

#[test]
fn source_tasks_keep_their_order_and_dcl_created_work_cannot_overtake_queued_load() {
    let (mut realm, init) = start(
        r#"
        var order=[];window.onmessage=e=>{order.push(e.data+':'+document.readyState);document.title=order.join('|');};
        postMessage('early','/');
        document.addEventListener('DOMContentLoaded',()=>{order.push('dcl');postMessage('nested','/');document.title=order.join('|');});
        window.onload=()=>{order.push('load');document.title=order.join('|');};
    "#,
    );
    let early = pump(&mut realm, &init);
    assert_eq!(phase(&early), Phase::Interactive);
    assert_eq!(title(&early), "early:interactive");
    let dcl = pump(&mut realm, &early);
    assert_eq!(phase(&dcl), Phase::DomContentLoaded);
    assert_eq!(title(&dcl), "early:interactive|dcl");
    let load = pump(&mut realm, &dcl);
    assert_eq!(phase(&load), Phase::LoadComplete);
    assert_eq!(title(&load), "early:interactive|dcl|load");
    let nested = pump(&mut realm, &load);
    assert_eq!(title(&nested), "early:interactive|dcl|load|nested:complete");
    assert_eq!(work(&nested), (4, 4, 0));
}

#[test]
fn capture_stop_skips_later_invocations_but_not_other_listeners_in_the_same_invocation() {
    for (method, expected) in [
        ("stopPropagation", "first,second"),
        ("stopImmediatePropagation", "first"),
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var seen=[];document.addEventListener('DOMContentLoaded',e=>{{seen.push('first');e.{method}();document.title=seen.join(',');}},true);
            document.addEventListener('DOMContentLoaded',()=>{{seen.push('second');document.title=seen.join(',');}},true);
            document.addEventListener('DOMContentLoaded',()=>{{throw 'noncapture after capture stop';}});
            window.addEventListener('DOMContentLoaded',()=>{{throw 'bubble after capture stop';}});
        "#
        ));
        let dcl = pump(&mut realm, &init);
        assert!(dcl.errors.is_empty(), "{:?}", dcl.errors);
        assert_eq!(title(&dcl), expected);
        assert_eq!(phase(&dcl), Phase::DomContentLoaded);
    }
}

#[test]
fn pending_and_lifetime_boundaries_include_both_mandatory_admissions_without_extra_credit() {
    for (source, admitted, ready) in [
        (
            "for(let i=0;i<254;i++)setTimeout(()=>{},60000);".to_owned(),
            256,
            true,
        ),
        (
            "for(let i=0;i<255;i++)setTimeout(()=>{},60000);".to_owned(),
            256,
            false,
        ),
        (
            "for(let i=0;i<2046;i++){let id=setTimeout(()=>{},60000);clearTimeout(id);}".to_owned(),
            2048,
            true,
        ),
        (
            "for(let i=0;i<2047;i++){let id=setTimeout(()=>{},60000);clearTimeout(id);}".to_owned(),
            2048,
            false,
        ),
        (
            "for(let i=0;i<2048;i++){let id=setTimeout(()=>{},60000);clearTimeout(id);}".to_owned(),
            2048,
            false,
        ),
    ] {
        let (_realm, reply) = start(&source);
        assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, admitted);
        if ready {
            assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
            assert_eq!(phase(&reply), Phase::Interactive);
            assert_eq!(
                reply.boa.as_ref().unwrap().pending_jobs,
                if source.contains("254") { 256 } else { 2 }
            );
            assert!(reply.task_wake.is_some());
        } else {
            assert_eq!(reply.state, RealmState::Fatal);
            assert!(reply.lifecycle.unwrap().initial_failure);
            assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
            assert!(reply.task_wake.is_none());
        }
    }
}

#[test]
fn source_already_fatal_is_not_admitted_two_more_times() {
    let (_realm, reply) = start("while(true){}");
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(work(&reply), (0, 0, 0));
    assert_eq!(phase(&reply), Phase::Initializing);
    assert!(reply.task_wake.is_none() && reply.lifecycle.unwrap().initial_failure);
}

#[test]
fn ordinary_init_and_lifecycle_errors_are_sticky_but_later_clean_replies_do_not_repeat_diagnostics()
{
    for source in [
        "throw 1;",
        "document.addEventListener('DOMContentLoaded',()=>{throw 2;});",
        "window.onload=()=>{throw 3;};",
        "document.onreadystatechange=()=>{if(document.readyState==='complete')throw 4;};",
    ] {
        let (mut realm, init) = start(source);
        let initial_errors = init.errors.clone();
        let dcl = pump(&mut realm, &init);
        let dcl_errors = dcl.errors.clone();
        let load = pump(&mut realm, &dcl);
        assert_eq!(phase(&load), Phase::LoadComplete);
        assert!(load.lifecycle.unwrap().initial_failure);
        assert_eq!(init.errors, initial_errors);
        assert_eq!(dcl.errors, dcl_errors);
        assert_eq!(
            initial_errors.len() + dcl_errors.len() + load.errors.len(),
            1
        );
    }
}

#[test]
fn fatal_lifecycle_keeps_the_last_completed_phase_and_clears_later_work() {
    for (source, load_fatal) in [
        (
            "document.addEventListener('DOMContentLoaded',()=>{while(true){}});",
            false,
        ),
        ("window.onload=()=>{while(true){}};", true),
    ] {
        let (mut realm, init) = start(source);
        let dcl = pump(&mut realm, &init);
        let terminal = if load_fatal {
            pump(&mut realm, &dcl)
        } else {
            dcl
        };
        assert_eq!(terminal.state, RealmState::Fatal);
        assert_eq!(
            phase(&terminal),
            if load_fatal {
                Phase::DomContentLoaded
            } else {
                Phase::Interactive
            }
        );
        assert!(terminal.lifecycle.unwrap().initial_failure && terminal.task_wake.is_none());
        assert_eq!(terminal.boa.as_ref().unwrap().pending_jobs, 0);
    }
}

#[test]
fn proposed_navigation_cancels_lifecycle_without_fabricating_completion() {
    let (_realm, early) = start("location.href='/next';");
    assert_eq!(phase(&early), Phase::Initializing);
    assert!(early.navigation.is_some() && early.task_wake.is_none());
    assert_eq!(work(&early), (0, 0, 0));
    for (source, from_load) in [
        (
            "document.addEventListener('DOMContentLoaded',()=>location.href='/next');",
            false,
        ),
        ("window.onload=()=>location.href='/next';", true),
    ] {
        let (mut realm, init) = start(source);
        let dcl = pump(&mut realm, &init);
        let next = if from_load {
            pump(&mut realm, &dcl)
        } else {
            dcl
        };
        assert!(next.navigation.is_some() && next.task_wake.is_none());
        assert_eq!(
            phase(&next),
            if from_load {
                Phase::DomContentLoaded
            } else {
                Phase::Interactive
            }
        );
        assert_eq!(next.boa.as_ref().unwrap().pending_jobs, 0);
    }
}

#[test]
fn lifecycle_survives_dynamic_privacy_purge_without_a_fake_rejection_gap() {
    let (mut realm, init) = start(
        "eval('');document.addEventListener('DOMContentLoaded',()=>document.title='dcl');window.onload=()=>document.title='load';",
    );
    assert!(init.errors.is_empty());
    let dcl = pump(&mut realm, &init);
    assert_eq!(title(&dcl), "dcl");
    let load = pump(&mut realm, &dcl);
    assert_eq!(title(&load), "load");
    assert_eq!(phase(&load), Phase::LoadComplete);
    assert!(!load.lifecycle.unwrap().initial_failure && load.errors.is_empty());
}

#[test]
fn load_completion_does_not_drain_long_timers_or_its_own_later_zero_delay_task() {
    let (mut realm, init) = start(
        "setTimeout(()=>{throw 'long timer ran';},60000);window.onload=()=>setTimeout(()=>document.title='after-load',0);",
    );
    let dcl = pump(&mut realm, &init);
    let load = pump(&mut realm, &dcl);
    assert_eq!(phase(&load), Phase::LoadComplete);
    assert_eq!(title(&load), "before");
    assert_eq!(work(&load), (4, 2, 2));
    let task = pump(&mut realm, &load);
    assert_eq!(title(&task), "after-load");
    assert_eq!(task.boa.as_ref().unwrap().pending_jobs, 1);
    assert!(task.task_wake.as_ref().unwrap().delay_ms > 0);
}

#[test]
fn a_post_load_error_cannot_rewrite_clean_initial_history() {
    let (mut realm, init) = start("window.onload=()=>setTimeout(()=>{throw 9;},0);");
    let dcl = pump(&mut realm, &init);
    let load = pump(&mut realm, &dcl);
    assert!(!load.lifecycle.unwrap().initial_failure);
    let task = pump(&mut realm, &load);
    assert_eq!(task.errors.len(), 1);
    assert!(!task.lifecycle.unwrap().initial_failure);
}

#[test]
fn rejected_early_transaction_keeps_interactive_history_and_cancels_both_permits() {
    let (mut realm, init) = start("");
    let rejected = realm.pump_tasks(TaskPump { wake_token: 0 });
    assert_eq!(rejected.state, RealmState::Closed);
    assert_eq!(rejected.revision, init.revision);
    assert_eq!(phase(&rejected), Phase::Interactive);
    assert!(rejected.lifecycle.unwrap().initial_failure);
    assert_eq!(work(&rejected), (2, 0, 0));
    assert!(rejected.task_wake.is_none() && rejected.snapshot.is_none());
}

#[test]
fn pre_realm_rejection_is_explicit_initializing_failure_without_fake_accounting() {
    let (realm, reply) = BoaPageRealm::start(Request {
        url: "file:///must-not-open".into(),
        html: "<p>Owned rejection</p>".into(),
        external_scripts: None,
    });
    assert!(realm.is_none() && reply.boa.is_none());
    assert_eq!(reply.state, RealmState::Closed);
    assert_eq!(phase(&reply), Phase::Initializing);
    assert!(reply.lifecycle.unwrap().initial_failure && !reply.errors.is_empty());
    assert!(reply.task_wake.is_none());
}
