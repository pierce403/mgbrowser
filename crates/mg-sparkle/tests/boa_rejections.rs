#![cfg(feature = "modern")]
//! Frozen authored Promise-notification controls, not scored upstream WPT cases.
use mg_sparkle::{
    document::{self, Document},
    js_browser::{
        ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource, Request,
        boa::BoaPageRealm,
    },
    page_session::{
        DefaultAction, InputKind, LifecyclePhase, RealmState, SessionInput, SessionReply, TaskPump,
    },
};
use std::time::Duration;

const URL: &str = "https://example.test/rejection-control";
const UNHANDLED: &str = "Unhandled Promise rejection";
const SUPPRESSED: &str =
    "Promise rejection observation suppressed by source-exposure policy; handled status unknown.";

fn start(source: &str) -> (BoaPageRealm, SessionReply) {
    // These controls require a loaded realm with notification work still queued.
    // Unlike bundled Init, genuine source-created tasks may precede DCL/load.
    let wrapped = format!("window.onload=function(){{{source}\n}};");
    let (mut realm, init) = request(&wrapped, None);
    assert_eq!(init.lifecycle.unwrap().phase, LifecyclePhase::Interactive);
    assert_eq!(
        (
            init.boa.as_ref().unwrap().jobs_admitted,
            init.boa.as_ref().unwrap().jobs_executed
        ),
        (2, 0)
    );
    let dcl = pump(&mut realm, &init);
    assert_eq!(
        dcl.lifecycle.unwrap().phase,
        LifecyclePhase::DomContentLoaded
    );
    let loaded = pump(&mut realm, &dcl);
    assert_eq!(
        loaded.lifecycle.unwrap().phase,
        LifecyclePhase::LoadComplete
    );
    (realm, loaded)
}

fn finish_load(realm: &mut BoaPageRealm, mut reply: SessionReply) -> SessionReply {
    for _ in 0..4 {
        if reply.lifecycle.unwrap().phase == LifecyclePhase::LoadComplete {
            return reply;
        }
        reply = pump(realm, &reply);
    }
    panic!("owned lifecycle did not complete in four tasks")
}

fn request(
    source: &str,
    external: Option<Option<ClassicErrorVisibility>>,
) -> (BoaPageRealm, SessionReply) {
    let html = format!(
        "<html><head><title>before</title></head><body><a id=a href=/next>Next</a><script>{source}</script>{}</body></html>",
        if external.is_some() {
            "<script id=external src='/owned.js' defer></script>"
        } else {
            ""
        },
    );
    let external_scripts = external.map(|visibility| {
        let parsed = document::parse_with_scripting(&html, URL, true);
        vec![ExternalScriptSource {
            node: parsed.query_selector(0, "#external").unwrap().unwrap(),
            source: "globalThis.externalExecuted=true;".into(),
            provenance: visibility.map(|error_visibility| ClassicSourceProvenance {
                response_url: if error_visibility == ClassicErrorVisibility::Muted {
                    "https://foreign.test/private.js".into()
                } else {
                    "https://example.test/owned.js".into()
                },
                error_visibility,
            }),
        }]
    });
    let (realm, reply) = BoaPageRealm::start(Request {
        url: URL.into(),
        html,
        external_scripts,
    });
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.accounting_valid());
    (realm.expect("retained authored realm"), reply)
}

fn projected(reply: &SessionReply) -> Document {
    document::project_nodes(reply.snapshot.as_ref().unwrap().nodes.clone(), URL).unwrap()
}

fn title(reply: &SessionReply) -> String {
    projected(reply).title
}

fn clean(reply: &SessionReply) {
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
}

fn no_work(reply: &SessionReply) {
    assert!(reply.task_wake.is_none(), "unexpected later work");
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
}

fn pump(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let wake = previous.task_wake.as_ref().expect("advertised later task");
    assert!(wake.delay_ms <= 100, "owned short-delay control only");
    if wake.delay_ms != 0 {
        std::thread::sleep(Duration::from_millis(wake.delay_ms as u64));
    }
    let reply = realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    });
    assert_eq!(reply.revision, previous.revision + 1);
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert_eq!(reply.default_action, DefaultAction::None);
    assert!(reply.acknowledgements.is_empty());
    assert!(reply.outcome.click_canceled.is_none());
    assert!(reply.outcome.submit_canceled.is_none());
    assert_eq!(reply.scripts_executed, 0);
    assert!(reply.accounting_valid());
    assert!(
        reply
            .boa
            .as_ref()
            .unwrap()
            .validate_after(previous.boa.as_ref().unwrap())
            .is_ok()
    );
    assert_eq!(
        reply.boa.as_ref().unwrap().pending_jobs > 0,
        reply.task_wake.is_some()
    );
    reply
}

fn click(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let target = projected(previous)
        .query_selector(0, "#a")
        .unwrap()
        .unwrap();
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.accounting_valid());
    reply
}

#[test]
fn notification_is_a_later_task_with_actual_typed_payload_and_escaped_identity() {
    for value in [
        "42",
        "null",
        "undefined",
        "'owned secret'",
        "Symbol('owned')",
        "({proof:42})",
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var reason={value},promise,saved,errors=0;
            window.addEventListener('error',()=>errors++);
            window.addEventListener('unhandledrejection',function(event){{
                if(arguments.length!==1||this!==window||event.target!==window||
                    event.currentTarget!==window||event.eventPhase!==2||event.type!=='unhandledrejection'||
                    event.bubbles||!event.cancelable||event.promise!==promise||event.reason!==reason||errors!==0)
                    throw 'wrong notification payload';
                saved=event;
                try{{event.promise=Promise.resolve();}}catch(e){{}}
                try{{Object.defineProperty(event,'reason',{{value:'forged'}});}}catch(e){{}}
                if(event.promise!==promise||event.reason!==reason)throw 'mutable notification payload';
                event.preventDefault();document.title='notified';
            }});
            document.getElementById('a').onclick=()=>{{
                if(saved.promise!==promise||saved.reason!==reason||saved.target!==window||
                    saved.currentTarget!==null||saved.eventPhase!==0||!saved.defaultPrevented||errors!==0)
                    throw 'escaped notification changed';
                document.title='escaped';return false;
            }};
            promise=Promise.reject(reason);
            if(saved!==undefined)throw 'notification ran synchronously';document.title='initialized';
        "#,
        ));
        clean(&init);
        assert_eq!(title(&init), "initialized");
        assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
        let notified = pump(&mut realm, &init);
        assert_eq!(title(&notified), "notified", "value {value}");
        assert_eq!(notified.errors, [UNHANDLED]);
        // Two genuine lifecycle tasks preceded this notification.
        assert_eq!(notified.boa.as_ref().unwrap().jobs_executed, 3);
        no_work(&notified);
        let escaped = click(&mut realm, &notified);
        clean(&escaped);
        assert_eq!(title(&escaped), "escaped");
        no_work(&escaped);
    }
}

#[test]
fn property_handler_has_one_argument_preserves_slot_and_false_only_cancels() {
    for (returned, canceled) in [
        ("false", true),
        ("true", false),
        ("0", false),
        ("null", false),
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var promise,reason={{proof:1}},order=[];
            window.addEventListener('unhandledrejection',event=>{{order.push('capture');return false;}},true);
            window.onunhandledrejection=()=>order.push('old');
            window.addEventListener('unhandledrejection',event=>{{
                order.push('last:'+event.defaultPrevented);document.title=order.join(',');return false;
            }});
            window.onunhandledrejection=function(event){{
                if(arguments.length!==1||this!==window||event.promise!==promise||event.reason!==reason)
                    throw 'property handler arguments';
                order.push('property');return {returned};
            }};
            promise=Promise.reject(reason);
        "#,
        ));
        clean(&init);
        let reply = pump(&mut realm, &init);
        assert_eq!(title(&reply), format!("capture,property,last:{canceled}"));
        assert_eq!(reply.errors, [UNHANDLED]);
        no_work(&reply);
    }
}

#[test]
fn arbitrary_property_getter_is_not_a_handler_and_capture_stop_skips_second_invocation() {
    let (mut realm, init) = start(
        r#"
        var order=[];
        window.onunhandledrejection=()=>{throw 'stopped property ran';};
        Object.defineProperty(window,'onunhandledrejection',{get(){throw 'arbitrary getter ran';},configurable:true});
        window.addEventListener('unhandledrejection',event=>{order.push('first');event.stopPropagation();},true);
        window.addEventListener('unhandledrejection',()=>{order.push('second');document.title=order.join(',');},true);
        window.addEventListener('unhandledrejection',()=>{throw 'noncapture after stop';});
        Promise.reject(1);
    "#,
    );
    clean(&init);
    let reply = pump(&mut realm, &init);
    assert_eq!(title(&reply), "first,second");
    assert_eq!(reply.errors, [UNHANDLED]);
}

#[test]
fn removal_and_listener_additions_recheck_existing_slots_without_changing_current_list() {
    let (mut realm, init) = start(
        r#"
        var order=[],count=0;
        function removed(){throw 'removed listener ran';}
        function added(){order.push('added');document.title=order.join(',');}
        window.addEventListener('unhandledrejection',event=>{
            count++;order.push('first'+count);window.removeEventListener('unhandledrejection',removed);
            window.addEventListener('unhandledrejection',added);
            if(count===1)Promise.reject(2);
        });
        window.addEventListener('unhandledrejection',removed);
        window.addEventListener('unhandledrejection',()=>{order.push('last');document.title=order.join(',');});
        Promise.reject(1);
    "#,
    );
    clean(&init);
    let first = pump(&mut realm, &init);
    assert_eq!(title(&first), "first1,last");
    assert_eq!(first.errors, [UNHANDLED]);
    let second = pump(&mut realm, &first);
    assert_eq!(title(&second), "first1,last,first2,last,added");
    assert_eq!(second.errors, [UNHANDLED]);
    no_work(&second);
}

#[test]
fn same_stack_and_microtask_handling_never_queue_notification_or_window_error() {
    for catch in [
        "promise.catch(()=>{});",
        "Promise.resolve().then(()=>promise.catch(()=>{}));",
    ] {
        let (_realm, init) = start(&format!(
            r#"
            window.addEventListener('error',()=>{{throw 'rejection became Window error';}});
            window.addEventListener('unhandledrejection',()=>{{throw 'handled rejection notified';}});
            window.addEventListener('rejectionhandled',()=>{{throw 'never-notified rejection handled';}});
            var promise=Promise.reject({{proof:1}});{catch}
            document.title='caught';
        "#,
        ));
        clean(&init);
        assert_eq!(title(&init), "caught");
        assert!(init.boa.as_ref().unwrap().jobs_admitted > 0);
        no_work(&init);
    }
}

#[test]
fn handling_a_frozen_candidate_before_its_pump_cancels_the_empty_batch() {
    let (mut realm, init) = start(
        r#"
        window.addEventListener('unhandledrejection',()=>{throw 'handled before delivery';});
        window.addEventListener('rejectionhandled',()=>{throw 'never notified';});
        var promise=Promise.reject(1);
        document.getElementById('a').onclick=()=>{promise.catch(()=>{});document.title='caught after freeze';return false;};
    "#,
    );
    clean(&init);
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
    let reply = click(&mut realm, &init);
    clean(&reply);
    assert_eq!(title(&reply), "caught after freeze");
    assert!(reply.boa.as_ref().unwrap().jobs_admitted > init.boa.as_ref().unwrap().jobs_admitted);
    no_work(&reply);
}

#[test]
fn handling_inside_notification_or_its_checkpoints_prevents_rejectionhandled() {
    for handling in [
        "promise.catch(()=>{});",
        "Promise.resolve().then(()=>promise.catch(()=>{}));",
        "throw reason;",
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var reason={{proof:1}},promise;
            window.onerror=(m,f,l,c,error)=>{{if(error!==reason)throw 'wrong listener error';promise.catch(()=>{{}});return true;}};
            window.addEventListener('rejectionhandled',()=>{{throw 'handled while notifying';}});
            window.addEventListener('unhandledrejection',event=>{{{handling}}});
            window.addEventListener('unhandledrejection',()=>{{document.title='delivered';}});
            promise=Promise.reject(reason);
        "#,
        ));
        clean(&init);
        let reply = pump(&mut realm, &init);
        assert_eq!(title(&reply), "delivered", "{handling}");
        assert_eq!(
            reply
                .errors
                .iter()
                .filter(|s| s.as_str() == UNHANDLED)
                .count(),
            1
        );
        assert_eq!(
            reply.errors.len(),
            if handling == "throw reason;" { 2 } else { 1 }
        );
        no_work(&reply);
    }
}

#[test]
fn late_handling_is_a_separate_noncancelable_task_with_the_original_payload() {
    let (mut realm, init) = start(
        r#"
        var reason={proof:1},promise,saved,handled=0;
        window.onunhandledrejection=event=>{saved=event;event.preventDefault();document.title='unhandled';};
        window.onrejectionhandled=function(event){
            if(arguments.length!==1||this!==window||event.promise!==promise||event.reason!==reason||
                event.type!=='rejectionhandled'||event.bubbles||event.cancelable||event.target!==window||
                event.currentTarget!==window||event.eventPhase!==2||event===saved)throw 'wrong handled payload';
            event.preventDefault();if(event.defaultPrevented)throw 'handled event canceled';
            handled++;document.title='handled:'+handled;return false;
        };
        window.addEventListener('rejectionhandled',event=>{if(event.defaultPrevented)throw 'false canceled handled event';});
        document.getElementById('a').onclick=()=>{
            promise.catch(()=>{});promise.catch(()=>{});
            if(saved.promise!==promise||saved.reason!==reason||!saved.defaultPrevented)throw 'old event changed';
            document.title='catch queued:'+handled;return false;
        };
        promise=Promise.reject(reason);
    "#,
    );
    clean(&init);
    let notified = pump(&mut realm, &init);
    assert_eq!(title(&notified), "unhandled");
    assert_eq!(notified.errors, [UNHANDLED]);
    no_work(&notified);
    let queued = click(&mut realm, &notified);
    clean(&queued);
    assert_eq!(title(&queued), "catch queued:0");
    assert_eq!(queued.boa.as_ref().unwrap().pending_jobs, 1);
    let handled = pump(&mut realm, &queued);
    clean(&handled);
    assert_eq!(title(&handled), "handled:1");
    no_work(&handled);
    assert_eq!(
        notified.errors,
        [UNHANDLED],
        "handling cannot retract earlier evidence"
    );
    let again = click(&mut realm, &handled);
    clean(&again);
    assert_eq!(title(&again), "catch queued:1");
    no_work(&again);
}

#[test]
fn then_without_rejection_handler_notifies_only_the_rejected_leaf_promise() {
    let (mut realm, init) = start(
        r#"
        var reason={proof:1},root=Promise.reject(reason),leaf=root.then();
        window.addEventListener('unhandledrejection',event=>{
            if(event.promise!==leaf||event.promise===root||event.reason!==reason)throw 'wrong chain identity';
            document.title='leaf';
        });
    "#,
    );
    clean(&init);
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 1);
    let reply = pump(&mut realm, &init);
    assert_eq!(title(&reply), "leaf");
    assert_eq!(reply.errors, [UNHANDLED]);
    no_work(&reply);
}

#[test]
fn one_pump_delivers_one_frozen_batch_and_new_rejections_wait_for_the_next_batch() {
    let (mut realm, init) = start(
        r#"
        var order=[],one=Promise.reject(1),two=Promise.reject(2);
        window.addEventListener('unhandledrejection',event=>{
            order.push(event.reason);
            if(event.promise===one){Promise.reject(3);Promise.resolve().then(()=>{Promise.reject(4);});}
            document.title=order.join(',');
        });
    "#,
    );
    clean(&init);
    assert_eq!(init.boa.as_ref().unwrap().pending_jobs, 2);
    let first = pump(&mut realm, &init);
    assert_eq!(title(&first), "1,2");
    assert_eq!(first.errors, [UNHANDLED, UNHANDLED]);
    assert_eq!(first.boa.as_ref().unwrap().pending_jobs, 2);
    let second = pump(&mut realm, &first);
    assert_eq!(title(&second), "1,2,3,4");
    assert_eq!(second.errors, [UNHANDLED, UNHANDLED]);
    no_work(&second);
}

#[test]
fn handling_a_later_frozen_member_during_first_delivery_skips_it_without_false_execution() {
    let (mut realm, init) = start(
        r#"
        var one=Promise.reject(1),two=Promise.reject(2);
        window.addEventListener('unhandledrejection',event=>{
            if(event.promise!==one)throw 'handled frozen member delivered';
            Promise.resolve().then(()=>{two.catch(()=>{});});document.title='first only';
        });
        window.addEventListener('rejectionhandled',()=>{throw 'never notified member handled';});
    "#,
    );
    clean(&init);
    let reply = pump(&mut realm, &init);
    assert_eq!(title(&reply), "first only");
    assert_eq!(reply.errors, [UNHANDLED]);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 6);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 5);
    no_work(&reply);
}

#[test]
fn dom_batches_and_late_handled_tasks_keep_fifo_without_recursive_task_drain() {
    let (mut realm, init) = start(
        r#"
        var first=Promise.reject(1),order=[];
        window.addEventListener('unhandledrejection',event=>{
            order.push('u'+event.reason);if(event.promise===first)Promise.reject(2);document.title=order.join(',');
        });
        window.addEventListener('rejectionhandled',event=>{
            if(event.promise!==first)throw 'wrong handled identity';order.push('h1');document.title=order.join(',');
        });
        document.getElementById('a').onclick=()=>{first.catch(()=>{});return false;};
    "#,
    );
    clean(&init);
    let first = pump(&mut realm, &init);
    assert_eq!(title(&first), "u1");
    let queued = click(&mut realm, &first);
    assert_eq!(title(&queued), "u1");
    let second = pump(&mut realm, &queued);
    assert_eq!(title(&second), "u1,u2");
    let third = pump(&mut realm, &second);
    assert_eq!(title(&third), "u1,u2,h1");
    no_work(&third);
}

#[test]
fn an_earlier_timer_and_its_microtasks_do_not_recursively_execute_a_rejection_batch() {
    let (mut realm, init) = start(
        r#"
        var order=[];
        setTimeout(()=>{order.push('timer');Promise.resolve().then(()=>{order.push('job');document.title=order.join(',');});},0);
        window.addEventListener('unhandledrejection',()=>{order.push('notification');document.title=order.join(',');});
        Promise.reject(1);
    "#,
    );
    clean(&init);
    let first = pump(&mut realm, &init);
    clean(&first);
    assert_eq!(title(&first), "timer,job");
    assert_eq!(first.boa.as_ref().unwrap().pending_jobs, 1);
    let second = pump(&mut realm, &first);
    assert_eq!(title(&second), "timer,job,notification");
    assert_eq!(second.errors, [UNHANDLED]);
    no_work(&second);
}

#[test]
fn hostile_reason_extraction_never_calls_getters_coercion_or_proxy_traps() {
    for reason in [
        "Object.defineProperties({},{message:{get(){traps++;throw 1;}},stack:{get(){traps++;throw 2;}},name:{get(){traps++;throw 3;}},toString:{value(){traps++;throw 4;}},[Symbol.toPrimitive]:{value(){traps++;throw 5;}}})",
        "new Proxy({},{get(){traps++;throw 1;},getPrototypeOf(){traps++;throw 2;},ownKeys(){traps++;throw 3;}})",
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var traps=0,reason={reason},promise;
            window.addEventListener('unhandledrejection',event=>{{
                if(event.reason!==reason||event.promise!==promise||traps!==0)throw 'unsafe extraction';
                document.title='zero traps';event.preventDefault();
            }});
            promise=Promise.reject(reason);
        "#,
        ));
        clean(&init);
        let reply = pump(&mut realm, &init);
        assert_eq!(title(&reply), "zero traps");
        assert_eq!(reply.errors, [UNHANDLED]);
        no_work(&reply);
    }
}

#[test]
fn ordinary_listener_throw_uses_window_error_but_a_promise_job_throw_is_a_later_rejection() {
    let (mut realm, init) = start(
        r#"
        var one=Promise.reject(1),thrown={proof:2},order=[];
        window.onerror=(m,f,l,c,error)=>{if(error!==thrown)throw 'wrong ordinary error';order.push('error');return true;};
        window.addEventListener('unhandledrejection',event=>{
            order.push('u'+event.reason);
            if(event.promise===one){Promise.resolve().then(()=>{throw 3;});throw thrown;}
        });
        window.addEventListener('unhandledrejection',()=>{order.push('last');document.title=order.join(',');});
    "#,
    );
    clean(&init);
    let first = pump(&mut realm, &init);
    assert_eq!(title(&first), "u1,error,last");
    assert_eq!(
        first
            .errors
            .iter()
            .filter(|s| s.as_str() == UNHANDLED)
            .count(),
        1
    );
    assert_eq!(first.errors.len(), 2);
    let second = pump(&mut realm, &first);
    assert_eq!(title(&second), "u1,error,last,u3,last");
    assert_eq!(second.errors, [UNHANDLED]);
    no_work(&second);
}

#[test]
fn fatal_listener_stops_later_listeners_reporters_jobs_and_tasks() {
    let (mut realm, init) = start(
        r#"
        window.onerror=()=>{document.title='fatal reporter ran';};
        window.addEventListener('unhandledrejection',()=>{
            document.title='before fatal';Promise.resolve().then(()=>document.title='fatal job ran');while(true){}
        });
        window.addEventListener('unhandledrejection',()=>{document.title='fatal later listener ran';});
        Promise.reject(1);Promise.reject(2);
    "#,
    );
    clean(&init);
    let wake = init.task_wake.unwrap();
    assert!(wake.delay_ms <= 100);
    std::thread::sleep(Duration::from_millis(wake.delay_ms as u64));
    let reply = realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    });
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(title(&reply), "before fatal");
    assert!(reply.boa.as_ref().unwrap().fatal_reason.is_some());
    no_work(&reply);
}

#[test]
fn initial_foreign_and_unknown_floors_suppress_even_immediately_caught_rejections() {
    for visibility in [Some(ClassicErrorVisibility::Muted), None] {
        for catch in [
            "",
            "promise.catch(()=>{});",
            "Promise.resolve().then(()=>promise.catch(()=>{}));",
        ] {
            let (mut realm, init) = request(
                &format!(
                    r#"
                window.addEventListener('unhandledrejection',()=>{{throw 'private event leaked';}});
                window.addEventListener('rejectionhandled',()=>{{throw 'private handled event leaked';}});
                var promise=Promise.reject({{privateProof:1}});{catch}
                document.getElementById('a').onclick=()=>{{document.title='later transaction';return false;}};
                document.title='suppressed';
            "#,
                ),
                Some(visibility),
            );
            // Initial provenance remains a real source-time control. Lifecycle
            // pumps must survive privacy purging and retain its sticky notice.
            let init = finish_load(&mut realm, init);
            assert_eq!(title(&init), "suppressed");
            assert_eq!(init.errors, [SUPPRESSED]);
            no_work(&init);
            let later = click(&mut realm, &init);
            assert_eq!(title(&later), "later transaction");
            assert_eq!(later.errors, [SUPPRESSED]);
            no_work(&later);
        }
    }
}

#[test]
fn proven_local_external_source_keeps_notification_identity_and_fresh_realms_are_not_muted() {
    for external in [Some(Some(ClassicErrorVisibility::SameOrigin)), None] {
        let (mut realm, init) = request(
            r#"
            var reason={proof:1},promise=Promise.reject(reason);
            window.addEventListener('unhandledrejection',event=>{
                if(event.reason!==reason||event.promise!==promise)throw 'local identity lost';document.title='local';
            });
        "#,
            external,
        );
        clean(&init);
        let reply = pump(&mut realm, &init);
        assert_eq!(title(&reply), "local");
        assert_eq!(reply.errors, [UNHANDLED]);
        let reply = finish_load(&mut realm, reply);
        no_work(&reply);
    }
}

#[test]
fn dynamic_compilation_suppresses_before_reject_before_checkpoint_and_before_pump() {
    for source in [
        "eval('');var promise=Promise.reject(1);",
        "var promise=Promise.reject(1);eval('');",
        "var promise=Promise.reject(1);Promise.resolve().then(()=>Function(''));",
    ] {
        let (mut realm, init) = request(
            &format!(
                r#"
            window.addEventListener('unhandledrejection',()=>{{throw 'newly private rejection leaked';}});
            {source}document.title='dynamic floor';
        "#,
            ),
            None,
        );
        let init = finish_load(&mut realm, init);
        assert_eq!(title(&init), "dynamic floor");
        assert_eq!(init.errors, [SUPPRESSED]);
        no_work(&init);
    }
    let (mut realm, init) = start(
        r#"
        window.addEventListener('unhandledrejection',()=>{throw 'queued private rejection leaked';});
        var promise=Promise.reject(1);
        document.getElementById('a').onclick=()=>{eval('');promise.catch(()=>{});document.title='floor fell';return false;};
    "#,
    );
    clean(&init);
    let reply = click(&mut realm, &init);
    assert_eq!(title(&reply), "floor fell");
    assert_eq!(reply.errors, [SUPPRESSED]);
    no_work(&reply);
}

#[test]
fn failed_function_argument_conversion_is_not_a_dynamic_compilation_attempt() {
    let (mut realm, init) = start(
        r#"
        try{Function({toString(){throw 1;}});}catch(error){}
        var reason={proof:1},promise=Promise.reject(reason);
        window.addEventListener('unhandledrejection',event=>{
            if(event.reason!==reason||event.promise!==promise)throw 'coercion falsely muted';document.title='still local';
        });
    "#,
    );
    clean(&init);
    let reply = pump(&mut realm, &init);
    assert_eq!(title(&reply), "still local");
    assert_eq!(reply.errors, [UNHANDLED]);
    no_work(&reply);
}

#[test]
fn a_floor_falling_during_delivery_suppresses_remaining_members_without_rewriting_exposed_events() {
    let (mut realm, init) = start(
        r#"
        var one=Promise.reject(1),two=Promise.reject(2),saved;
        window.addEventListener('unhandledrejection',event=>{
            if(event.promise!==one||event.reason!==1)throw 'later private member exposed';
            saved=event;eval('');document.title='first exposed';
        });
        window.addEventListener('rejectionhandled',()=>{throw 'private handled event exposed';});
        document.getElementById('a').onclick=()=>{
            one.catch(()=>{});two.catch(()=>{});
            if(saved.promise!==one||saved.reason!==1||saved.currentTarget!==null||saved.eventPhase!==0)
                throw 'already exposed payload rewritten';
            document.title='saved identity';return false;
        };
    "#,
    );
    clean(&init);
    let first = pump(&mut realm, &init);
    assert_eq!(title(&first), "first exposed");
    assert_eq!(first.errors, [UNHANDLED, SUPPRESSED]);
    no_work(&first);
    let later = click(&mut realm, &first);
    assert_eq!(title(&later), "saved identity");
    assert_eq!(later.errors, [SUPPRESSED]);
    no_work(&later);
}

#[test]
fn actual_rejection_listeners_use_existing_thirty_two_cumulative_slots() {
    let (mut realm, init) = request(
        r#"
        var calls=0;
        for(let i=0;i<32;i++)window.addEventListener('unhandledrejection',()=>{calls++;document.title='listeners:'+calls;});
        var rejected=false;try{window.addEventListener('rejectionhandled',()=>{});}catch(error){rejected=true;}
        if(!rejected)throw 'listener cap expanded';Promise.reject(1);
    "#,
        None,
    );
    clean(&init);
    let reply = pump(&mut realm, &init);
    assert_eq!(title(&reply), "listeners:32");
    assert_eq!(reply.errors, [UNHANDLED]);
    // No wrapper listener consumes one of the exact 32 tested slots.
    let reply = finish_load(&mut realm, reply);
    no_work(&reply);
}
