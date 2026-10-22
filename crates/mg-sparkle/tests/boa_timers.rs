#![cfg(feature = "modern")]
//! Owned non-scoring timer controls. Page code runs only in this caller-owned fixture realm.
use mg_sparkle::{
    document::{self, Document},
    js_browser::{Request, boa::BoaPageRealm},
    page_session::{DefaultAction, RealmState, SessionReply, TaskPump},
};
use std::time::Duration;

const URL: &str = "https://example.test/timer-control";
fn start(source: &str) -> (BoaPageRealm, SessionReply) {
    let (realm, reply) = BoaPageRealm::start(Request {
        external_scripts: None,
        url: URL.into(),
        html: format!(
            "<html><head><title>before</title></head><body><p id=out></p><script>{source}</script></body></html>"
        ),
    });
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    (realm.unwrap(), reply)
}
fn projected(reply: &SessionReply) -> Document {
    document::project_nodes(reply.snapshot.as_ref().unwrap().nodes.clone(), URL).unwrap()
}
fn pump(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let wake = previous.task_wake.as_ref().expect("advertised task");
    assert!(wake.delay_ms <= 100, "owned short-delay control only");
    if wake.delay_ms != 0 {
        std::thread::sleep(Duration::from_millis(wake.delay_ms as u64));
    }
    let reply = realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    });
    assert_eq!(reply.revision, previous.revision + 1);
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.acknowledgements.is_empty());
    assert!(reply.outcome.click_canceled.is_none());
    assert!(reply.outcome.submit_canceled.is_none());
    assert_eq!(reply.scripts_executed, 0);
    reply
}

#[test]
fn zero_delay_runs_only_after_init_load_and_microtasks() {
    let (mut realm, init) = start(
        r#"
        var order=['script'];
        Promise.resolve().then(()=>order.push('script-job'));
        setTimeout(function(arg){
            if(this!==window||arg.proof!==42)throw 'wrong callback/argument';
            order.push('timer');Promise.resolve().then(()=>document.title=order.join(','));
        },0,{proof:42});
        window.onload=()=>{order.push('load');Promise.resolve().then(()=>order.push('load-job'));};
    "#,
    );
    assert_eq!(projected(&init).title, "before");
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
    let reply = pump(&mut realm, &init);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(
        projected(&reply).title,
        "script,script-job,load,load-job,timer"
    );
    assert!(reply.task_wake.is_none());
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
}

#[test]
fn exact_frozen_native_task_fixture_creates_real_controls_after_a_later_task() {
    let (realm, init) = BoaPageRealm::start(Request {
        external_scripts: None,
        url: URL.into(),
        html: include_str!("../../../tests/fixtures/script/task-events.html").into(),
    });
    assert_eq!(init.state, RealmState::Ready, "{:?}", init.errors);
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    assert!(projected(&init).forms.is_empty());
    let mut realm = realm.unwrap();
    let reply = pump(&mut realm, &init);
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    let document = projected(&reply);
    assert_eq!(document.title, "Timer task controls ready");
    assert_eq!(document.forms.len(), 1);
    assert!(document.query_selector(0, "#cancel").unwrap().is_some());
    assert!(reply.task_wake.is_none());
}

#[test]
fn single_window_top_is_a_readonly_nonconfigurable_accessor() {
    let (_realm, init) = start(
        r#"
        const descriptor=Object.getOwnPropertyDescriptor(window,'top');
        if(typeof descriptor.get!=='function'||descriptor.set!==undefined||
            descriptor.configurable||!descriptor.enumerable||'value' in descriptor)throw 'top descriptor';
        top=null;
        if(top!==window||Reflect.defineProperty(window,'top',{value:null}))throw 'top replaced';
        document.title='single Window';
    "#,
    );
    assert_eq!(projected(&init).title, "single Window");
}

#[test]
fn readiness_precedes_deferred_sources_and_lifecycle_uses_real_paths_and_slots() {
    let html = r#"<html><head><title>before</title></head><body><p id=out></p><script>
        var order=['normal:'+document.readyState],escaped;
        document.onreadystatechange=function(event){
            if(event.target!==document||event.currentTarget!==document||event.bubbles||event.cancelable)throw 'readiness metadata';
            event.preventDefault();if(event.defaultPrevented)throw 'noncancelable event canceled';
            order.push('rs:'+document.readyState);
        };
        window.addEventListener('DOMContentLoaded',event=>{
            if(event.eventPhase!==1||event.currentTarget!==window||event.target!==document)throw 'capture';
            order.push('capture');
        },true);
        document.addEventListener('DOMContentLoaded',event=>{
            escaped=event;if(event.eventPhase!==2||!event.bubbles||event.cancelable)throw 'target';order.push('document');
        });
        window.addEventListener('DOMContentLoaded',event=>{if(event.eventPhase!==3)throw 'bubble';order.push('bubble');});
        document.addEventListener('load',()=>{throw 'Window load reached document';});
        window.onload=function(event){
            if(this!==window||window!==self||window!==top||window!==parent||opener!==null||
                event.target!==document||event.currentTarget!==window||event.bubbles||event.cancelable||
                escaped.currentTarget!==null||escaped.eventPhase!==0)throw 'load metadata';
            order.push('property');return false;
        };
        window.addEventListener('load',event=>{if(event.defaultPrevented)throw 'load canceled';order.push('load');document.title=order.join(',');});
    </script><script id=deferred src='/owned.js' defer></script></body></html>"#;
    let parsed = document::parse_with_scripting(html, URL, true);
    let node = parsed.query_selector(0, "#deferred").unwrap().unwrap();
    let (_realm, reply) = BoaPageRealm::start(Request {
        url: URL.into(),
        html: html.into(),
        external_scripts: Some(vec![mg_sparkle::js_browser::ExternalScriptSource {
            node,
            source: "order.push('defer:'+document.readyState);".into(),
            provenance: None,
        }]),
    });
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(
        projected(&reply).title,
        "normal:loading,rs:interactive,defer:interactive,capture,document,bubble,rs:complete,property,load"
    );
    assert!(reply.task_wake.is_none());
}

#[test]
fn cancellation_and_reentrant_delay_conversion_preserve_order_and_admissions() {
    let (mut realm, init) = start(
        r#"
        var order=[];
        var old=setTimeout(()=>{throw 'canceled callback ran';},0);
        setTimeout(()=>{order.push('timer');document.title=order.join(',');}, {valueOf(){
            clearTimeout(old);Promise.resolve().then(()=>order.push('job'));
            order.push('convert');return -1;
        }});
        order.push('after');
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 3);
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
    let reply = pump(&mut realm, &init);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(projected(&reply).title, "convert,after,job,timer");
    assert!(reply.task_wake.is_none());
}

#[test]
fn conversion_failure_retains_no_timer_and_fatal_conversion_cannot_be_caught() {
    let (_realm, init) = start(
        r#"
        var rejected=false;
        try{setTimeout(()=>document.title='must not run',Symbol('delay'));}catch(e){rejected=true;}
        if(!rejected)throw 'Symbol delay accepted';
    "#,
    );
    assert_eq!(init.boa.as_ref().unwrap().jobs_admitted, 0);
    assert!(init.task_wake.is_none());
    let (_realm, reply) = BoaPageRealm::start(Request { external_scripts: None, url: URL.into(),
        html: "<p>readable</p><script>try{setTimeout(()=>document.title='outer',{valueOf(){setTimeout(()=>document.title='inner',0);while(true){}}});}catch(e){document.title='caught fatal';}</script>".into() });
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 1);
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
    assert!(reply.task_wake.is_none());
    assert!(!projected(&reply).title.contains("caught fatal"));
}

#[test]
fn delay_conversion_is_webidl_int32_and_long_delays_are_not_clamped_to_lifetime() {
    let (mut realm, init) = start("setTimeout(()=>document.title='wrapped',4294967296);");
    let reply = pump(&mut realm, &init);
    assert_eq!(projected(&reply).title, "wrapped");
    let (_realm, init) = start("setTimeout(()=>{throw 'must remain delayed';},2147483647);");
    assert!(init.task_wake.as_ref().unwrap().delay_ms > 300_000);
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
}

#[test]
fn early_zero_stale_and_replayed_tokens_never_execute_callbacks() {
    for token in [0, 9999] {
        let (mut realm, init) = start("setTimeout(()=>document.title='must not run',100000);");
        let rejected = realm.pump_tasks(TaskPump { wake_token: token });
        assert_eq!(rejected.state, RealmState::Closed);
        assert_eq!(rejected.revision, init.revision);
        assert!(rejected.snapshot.is_none());
        assert!(rejected.task_wake.is_none());
        assert_eq!(rejected.boa.as_ref().unwrap().pending_jobs, 0);
    }
    let (mut realm, init) = start("setTimeout(()=>document.title='must not run',100000);");
    let early = realm.pump_tasks(TaskPump {
        wake_token: init.task_wake.unwrap().token,
    });
    assert_eq!(early.state, RealmState::Closed);
    assert!(early.snapshot.is_none());

    let (mut realm, init) =
        start("setTimeout(()=>{document.title='one';setTimeout(()=>document.title='two',0);},0);");
    let first = pump(&mut realm, &init);
    assert_eq!(projected(&first).title, "one");
    assert!(first.task_wake.as_ref().unwrap().token > init.task_wake.as_ref().unwrap().token);
    let replay = realm.pump_tasks(TaskPump {
        wake_token: init.task_wake.unwrap().token,
    });
    assert_eq!(replay.state, RealmState::Closed);
    assert_eq!(replay.revision, 1);
    assert!(replay.snapshot.is_none());
}

#[test]
fn ordinary_timer_error_is_reported_and_checkpoint_and_later_task_survive() {
    let (mut realm, init) = start(
        r#"
        setTimeout(()=>{Promise.resolve().then(()=>document.title='job after error');throw 'owned error';},0);
        setTimeout(()=>document.title='later task',0);
    "#,
    );
    let first = pump(&mut realm, &init);
    assert_eq!(first.state, RealmState::Ready);
    assert!(
        first
            .errors
            .iter()
            .any(|error| error.contains("Timer callback"))
    );
    assert_eq!(projected(&first).title, "job after error");
    let last = pump(&mut realm, &first);
    assert!(last.errors.is_empty(), "{:?}", last.errors);
    assert_eq!(projected(&last).title, "later task");
    assert!(last.task_wake.is_none());
}

#[test]
fn function_only_scope_and_receivers_fail_honestly() {
    let (_realm, init) = start(
        r#"
        var rejected=0;
        try{setTimeout('document.title="bad"',0);}catch(e){rejected++;}
        try{setTimeout.call(document,()=>{},0);}catch(e){rejected++;}
        try{window.addEventListener('message',()=>{});}catch(e){rejected++;}
        if(rejected!==3||typeof setInterval!=='undefined'||typeof clearInterval!=='undefined')throw 'fake scope';
        var id=setTimeout.call(null,()=>{},100000);clearTimeout.call(undefined,id);
    "#,
    );
    assert!(init.task_wake.is_none());
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 0);
}

#[test]
fn mixed_pending_and_cancelled_lifetime_work_keep_the_existing_limits() {
    let (realm, pending) = BoaPageRealm::start(Request { external_scripts: None, url: URL.into(),
        html: "<p>readable</p><script>function f(){};for(let i=0;i<256;i++)setTimeout(f,0);try{Promise.resolve().then(f);}catch(e){};document.title='must not run';</script>".into() });
    assert_eq!(pending.state, RealmState::Fatal);
    assert_eq!(pending.boa.as_ref().unwrap().jobs_admitted, 256);
    assert_eq!(pending.boa.as_ref().unwrap().pending_jobs, 0);
    assert!(pending.task_wake.is_none());
    drop(realm);
    let (_realm, lifetime) = BoaPageRealm::start(Request { external_scripts: None, url: URL.into(),
        html: "<p>readable</p><script>function f(){};for(let i=0;i<2049;i++){let id=setTimeout(f,0);clearTimeout(id);}</script>".into() });
    assert_eq!(lifetime.state, RealmState::Fatal);
    assert_eq!(lifetime.boa.as_ref().unwrap().jobs_admitted, 2048);
    assert_eq!(lifetime.boa.as_ref().unwrap().pending_jobs, 0);
    assert!(lifetime.task_wake.is_none());
}

#[test]
fn timer_navigation_is_only_an_explicit_proposal_and_fatal_work_disarms_the_rest() {
    let (mut realm, init) = start("setTimeout(()=>location.href='/next',0);");
    let reply = pump(&mut realm, &init);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.navigation.as_ref().unwrap().ends_with("/next"));
    let (mut realm, init) =
        start("setTimeout(()=>{while(true){}},0);setTimeout(()=>document.title='must not run',0);");
    let reply = pump(&mut realm, &init);
    assert_eq!(reply.state, RealmState::Fatal);
    assert!(reply.task_wake.is_none());
    assert!(reply.navigation.is_none());
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
}
