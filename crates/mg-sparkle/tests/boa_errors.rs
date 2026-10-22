#![cfg(feature = "modern")]
//! Frozen authored Window-error controls, independent of the scored WPT corpus.
use mg_sparkle::{
    document::{self, Document},
    js_browser::{
        ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource, Request,
        boa::BoaPageRealm,
    },
    page_session::{InputKind, LifecyclePhase, RealmState, SessionInput, SessionReply, TaskPump},
};
use std::time::Duration;

const URL: &str = "https://example.test/error-control";
fn start(source: &str) -> (BoaPageRealm, SessionReply) {
    let (realm, reply) = BoaPageRealm::start(Request {
        url: URL.into(),
        external_scripts: None,
        html: format!(
            "<html><head><title>before</title></head><body><a id=a href=/next>Next</a><script>{source}</script></body></html>"
        ),
    });
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    finish_initial(realm.expect("retained authored realm"), reply)
}
fn finish_initial(
    mut realm: BoaPageRealm,
    mut reply: SessionReply,
) -> (BoaPageRealm, SessionReply) {
    // Historical error-service assertions inspect the whole initial load. Keep
    // every transaction's diagnostic, rather than accepting the last clean one.
    // Per-transaction immutability and failure bits have independent lifecycle controls.
    let mut errors = Vec::new();
    for _ in 0..63 {
        if reply.state != RealmState::Ready
            || reply.navigation.is_some()
            || reply.lifecycle.unwrap().phase == LifecyclePhase::LoadComplete
        {
            break;
        }
        errors.append(&mut reply.errors);
        reply = pump(&mut realm, &reply);
    }
    errors.append(&mut reply.errors);
    reply.errors = errors;
    if reply.state == RealmState::Ready && reply.navigation.is_none() {
        assert_eq!(
            reply.lifecycle.unwrap().phase,
            LifecyclePhase::LoadComplete,
            "finite initial-load wait"
        );
    }
    (realm, reply)
}
fn projected(reply: &SessionReply) -> Document {
    document::project_nodes(reply.snapshot.as_ref().unwrap().nodes.clone(), URL).unwrap()
}
fn title(reply: &SessionReply) -> String {
    projected(reply).title
}
fn click(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let target = projected(previous)
        .query_selector(0, "#a")
        .unwrap()
        .unwrap();
    realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    })
}
fn pump(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let wake = previous.task_wake.as_ref().expect("owned timer advertised");
    assert!(wake.delay_ms <= 100);
    std::thread::sleep(Duration::from_millis(wake.delay_ms as u64));
    realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    })
}
fn external(
    source: &str,
    visibility: Option<ClassicErrorVisibility>,
) -> (BoaPageRealm, SessionReply) {
    let html = format!(
        "<html><head><title>before</title></head><body><a id=a href=/next>Next</a><script>{source}</script><script id=foreign src='/foreign.js' defer></script></body></html>"
    );
    let parsed = document::parse_with_scripting(&html, URL, true);
    let node = parsed.query_selector(0, "#foreign").unwrap().unwrap();
    let response_url = if visibility == Some(ClassicErrorVisibility::Muted) {
        "https://foreign.test/secret.js"
    } else {
        "https://example.test/owned.js"
    };
    let (realm, reply) = BoaPageRealm::start(Request {
        url: URL.into(),
        html,
        external_scripts: Some(vec![ExternalScriptSource {
            node,
            source: "globalThis.foreignThrow=()=>{throw secret;};".into(),
            provenance: visibility.map(|error_visibility| ClassicSourceProvenance {
                response_url: response_url.into(),
                error_visibility,
            }),
        }]),
    });
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    finish_initial(realm.unwrap(), reply)
}

#[test]
fn primitive_and_object_identity_use_a_real_cancelable_window_event() {
    for value in [
        "42",
        "null",
        "undefined",
        "'owned'",
        "Symbol('owned')",
        "({proof:42})",
    ] {
        let (_realm, reply) = start(&format!(
            r#"
            var value={value},saved;
            window.addEventListener('error',function(event){{
                if(this!==window||event.target!==window||event.currentTarget!==window||event.eventPhase!==2||
                    event.type!=='error'||event.bubbles||!event.cancelable||event.error!==value||
                    event.filename!==''||event.lineno!==0||event.colno!==0||typeof event.message!=='string'||
                    Array.from(event.message).length>512)throw 'wrong ErrorEvent payload';
                var original=event.message;
                try{{event.error={{fake:true}};}}catch(e){{}}
                try{{Object.defineProperty(event,'message',{{value:'fake'}});}}catch(e){{}}
                if(event.error!==value||event.message!==original||!('error' in event))throw 'mutable payload';
                saved=event;event.preventDefault();document.title='identity';
            }});
            window.onload=()=>{{if(saved.currentTarget!==null||saved.eventPhase!==0||!saved.defaultPrevented)throw 'escaped event state';document.title+=':escaped';}};
            throw value;
        "#
        ));
        assert_eq!(
            title(&reply),
            "identity:escaped",
            "value {value}: {:?}",
            reply.errors
        );
        assert_eq!(reply.errors.len(), 1, "{:?}", reply.errors);
    }
}

#[test]
fn thrown_object_extraction_never_runs_getters_or_coercion() {
    let (_realm, reply) = start(
        r#"
        var traps=0,secret={};
        for(const key of ['message','stack','name'])Object.defineProperty(secret,key,{get(){traps++;throw 'getter';}});
        secret.toString=()=>{traps++;throw 'toString';};
        secret[Symbol.toPrimitive]=()=>{traps++;throw 'primitive';};
        window.onerror=function(message,file,line,column,error){
            if(arguments.length!==5||this!==window||error!==secret||traps!==0||file!==''||line!==0||column!==0)throw 'unsafe extraction';
            document.title='no traps';return true;
        };
        throw secret;
    "#,
    );
    assert_eq!(title(&reply), "no traps");
    assert_eq!(
        reply.errors.len(),
        1,
        "cancellation cannot erase diagnostics: {:?}",
        reply.errors
    );
    assert!(!reply.errors[0].contains("getter"));
}

#[test]
fn onerror_uses_five_arguments_true_only_and_preserves_slot_order() {
    for (returned, canceled) in [
        ("true", true),
        ("false", false),
        ("1", false),
        ("'true'", false),
    ] {
        let (_realm, reply) = start(&format!(
            r#"
            var value={{proof:1}},order=[];
            window.addEventListener('error',event=>{{order.push('capture');return true;}},true);
            window.onerror=function(message,file,line,column,error){{if(arguments.length!==5||error!==value)throw 'five args';order.push('old');}};
            window.addEventListener('error',event=>{{order.push('last:'+event.defaultPrevented);document.title=order.join(',');return true;}});
            window.onerror=function(message,file,line,column,error){{if(arguments.length!==5||error!==value)throw 'five args';order.push('property');return {returned};}};
            throw value;
        "#
        ));
        assert_eq!(
            title(&reply),
            format!("capture,property,last:{canceled}"),
            "{:?}",
            reply.errors
        );
        assert_eq!(reply.errors.len(), 1);
    }
}

#[test]
fn handler_slot_does_not_invoke_an_arbitrary_global_onerror_getter() {
    let (_realm, reply) = start(
        r#"
        window.onerror=()=>{document.title='stored handler';return true;};
        Object.defineProperty(window,'onerror',{get(){throw 'arbitrary getter ran';},configurable:true});
        throw 1;
    "#,
    );
    assert_eq!(title(&reply), "stored handler");
    assert_eq!(reply.errors.len(), 1);
}

#[test]
fn error_listener_removal_and_stop_flags_keep_single_target_dispatch_semantics() {
    for (stop, expected) in [
        ("stopPropagation", "capture,first,property,last"),
        ("stopImmediatePropagation", "capture,first"),
    ] {
        let (_realm, reply) = start(&format!(
            r#"
            var order=[],saved;
            function removed(){{throw 'removed listener ran';}}
            window.addEventListener('error',()=>order.push('capture'),true);
            window.addEventListener('error',event=>{{
                saved=event;order.push('first');window.removeEventListener('error',removed);event.{stop}();
            }});
            window.addEventListener('error',removed);
            window.onerror=()=>order.push('property');
            window.addEventListener('error',()=>order.push('last'));
            window.onload=()=>{{
                if(saved.target!==window||saved.currentTarget!==null||saved.eventPhase!==0||saved.defaultPrevented)throw 'post-dispatch state';
                document.title=order.join(',');
            }};
            throw 1;
        "#
        ));
        assert_eq!(title(&reply), expected, "{stop}: {:?}", reply.errors);
        assert_eq!(reply.errors.len(), 1, "{:?}", reply.errors);
    }
}

#[test]
fn capture_stop_suppresses_the_later_at_target_noncapture_invocation() {
    for (stop, expected) in [
        ("stopPropagation", "first,second"),
        ("stopImmediatePropagation", "first"),
    ] {
        let (_realm, reply) = start(&format!(
            r#"
            var order=[],saved;
            window.addEventListener('error',event=>{{
                saved=event;order.push('first');event.{stop}();
            }},true);
            window.addEventListener('error',()=>order.push('second'),true);
            window.onerror=()=>order.push('property');
            window.addEventListener('error',()=>order.push('noncapture'));
            window.onload=()=>{{
                if(saved.target!==window||saved.currentTarget!==null||saved.eventPhase!==0||saved.defaultPrevented)throw 'post-dispatch state';
                document.title=order.join(',');
            }};
            throw 1;
        "#
        ));
        assert_eq!(title(&reply), expected, "{stop}: {:?}", reply.errors);
        assert_eq!(reply.errors.len(), 1, "{:?}", reply.errors);
    }
}

#[test]
fn startup_retained_click_and_timer_failures_report_once_and_continue_listeners() {
    let (mut realm, init) = start(
        r#"
        var order=[];window.addEventListener('error',event=>{order.push('error');document.title=order.join(',');});
        document.addEventListener('DOMContentLoaded',()=>{order.push('dcl');throw 1;});
        document.addEventListener('DOMContentLoaded',()=>order.push('dcl-after'));
        window.onload=()=>{order.push('load');throw 2;};
        window.addEventListener('load',()=>{order.push('load-after');document.title=order.join(',');});
        document.getElementById('a').onclick=event=>{event.preventDefault();order.push('click');throw 3;};
        document.getElementById('a').addEventListener('click',()=>{order.push('click-after');document.title=order.join(',');});
        window.addEventListener('load',()=>setTimeout(()=>{order.push('timer');Promise.resolve().then(()=>{order.push('job');document.title=order.join(',');});throw 4;},0));
    "#,
    );
    assert_eq!(title(&init), "dcl,error,dcl-after,load,error,load-after");
    assert_eq!(init.errors.len(), 2, "{:?}", init.errors);
    let clicked = click(&mut realm, &init);
    assert_eq!(clicked.errors.len(), 1);
    assert_eq!(clicked.outcome.click_canceled, Some(true));
    assert_eq!(
        title(&clicked),
        "dcl,error,dcl-after,load,error,load-after,click,error,click-after"
    );
    let fired = pump(&mut realm, &clicked);
    assert_eq!(fired.errors.len(), 1);
    assert_eq!(
        title(&fired),
        "dcl,error,dcl-after,load,error,load-after,click,error,click-after,timer,error,job"
    );
    assert!(fired.task_wake.is_none());
}

#[test]
fn recursive_reporter_failure_keeps_both_diagnostics_without_recursive_dispatch() {
    let (_realm, reply) = start(
        r#"
        var called=0;
        window.addEventListener('error',()=>{called++;throw {nested:true};});
        window.addEventListener('error',event=>{if(called!==1)throw 'recursive reporting';document.title='guarded';event.preventDefault();});
        throw {original:true};
    "#,
    );
    assert_eq!(title(&reply), "guarded");
    assert_eq!(reply.errors.len(), 2, "{:?}", reply.errors);
}

#[test]
fn all_admitted_metadata_sets_the_floor_before_the_first_inline_source() {
    for visibility in [
        None,
        Some(ClassicErrorVisibility::Muted),
        Some(ClassicErrorVisibility::SameOrigin),
    ] {
        let exposed = visibility == Some(ClassicErrorVisibility::SameOrigin);
        let (_realm, reply) = external(
            &format!(
                r#"
            var secret={{secret:'never expose foreign data'}},count=0;
            window.addEventListener('error',event=>{{
                count++;if({exposed} ? event.error!==secret : event.error!==null||event.message!=='Script error.')throw 'floor leaked';
                if(event.filename!==''||event.lineno!==0||event.colno!==0)throw 'fabricated location';
                document.title='floor:'+count;
            }});
            window.onload=()=>{{document.getElementById('foreign').setAttribute('src','/local.js');foreignThrow();}};
            throw secret;
        "#
            ),
            visibility,
        );
        assert_eq!(title(&reply), "floor:2", "{:?}", reply.errors);
        assert_eq!(reply.errors.len(), 2);
    }
}

#[test]
fn foreign_and_unknown_external_syntax_errors_expose_only_the_opaque_payload() {
    let html = r#"<html><head><title>before</title></head><body><script>
        window.addEventListener('error',event=>{
            if(event.message!=='Script error.'||event.error!==null||event.filename!==''||
                event.lineno!==0||event.colno!==0)throw 'external syntax exposed';
            document.title='opaque syntax';
        });
    </script><script id=foreign src='/bad.js' defer></script></body></html>"#;
    let parsed = document::parse_with_scripting(html, URL, true);
    let node = parsed.query_selector(0, "#foreign").unwrap().unwrap();
    for visibility in [None, Some(ClassicErrorVisibility::Muted)] {
        let (_realm, reply) = BoaPageRealm::start(Request {
            url: URL.into(),
            html: html.into(),
            external_scripts: Some(vec![ExternalScriptSource {
                node,
                source: "function FOREIGN_SECRET(".into(),
                provenance: visibility.map(|error_visibility| ClassicSourceProvenance {
                    response_url: "https://foreign.test/bad.js".into(),
                    error_visibility,
                }),
            }]),
        });
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
        assert_eq!(title(&reply), "opaque syntax", "{:?}", reply.errors);
        assert_eq!(reply.scripts_executed, 1);
        assert_eq!(reply.errors.len(), 1, "{:?}", reply.errors);
        assert!(reply.errors[0].starts_with("External script 2: SyntaxError:"));
    }
}

#[test]
fn forged_dom_provenance_base_and_window_aliases_cannot_upgrade_the_floor() {
    let html = r#"<html><head><base id=base href='https://foreign.test/'><title>before</title></head><body><script>
        var secret={proof:'must stay opaque'};
        window.onerror=(message,file,line,column,error)=>{
            if(message!=='Script error.'||error!==null||file!==''||line!==0||column!==0)throw 'forged authority';
            document.title='forgery stayed muted';
        };
        const script=document.getElementById('foreign');
        script.setAttribute('src','/local.js');
        script.setAttribute('provenance','{"response_url":"https://example.test/local.js","error_visibility":"same-origin"}');
        script.setAttribute('data-error-visibility','same-origin');
        document.getElementById('base').setAttribute('href','https://example.test/');
        setTimeout(()=>{throw secret;},0);
        window={provenance:{response_url:'https://example.test/local.js',error_visibility:'same-origin'}};
        self=window;parent=window;globalThis=window;
    </script><script id=foreign src='original.js' defer></script></body></html>"#;
    let parsed = document::parse_with_scripting(html, URL, true);
    assert_eq!(parsed.base_url, "https://foreign.test/");
    let node = parsed.query_selector(0, "#foreign").unwrap().unwrap();
    for visibility in [None, Some(ClassicErrorVisibility::Muted)] {
        let (realm, init) = BoaPageRealm::start(Request {
            url: URL.into(),
            html: html.into(),
            external_scripts: Some(vec![ExternalScriptSource {
                node,
                source: "0;".into(),
                provenance: visibility.map(|error_visibility| ClassicSourceProvenance {
                    response_url: "https://foreign.test/original.js".into(),
                    error_visibility,
                }),
            }]),
        });
        assert_eq!(init.state, RealmState::Ready, "{:?}", init.errors);
        assert!(init.errors.is_empty(), "{:?}", init.errors);
        let document = projected(&init);
        assert_eq!(document.nodes[node].attr("src"), Some("/local.js"));
        assert!(
            document.nodes[node]
                .attr("provenance")
                .unwrap()
                .contains("same-origin")
        );
        let base = document.query_selector(0, "#base").unwrap().unwrap();
        assert_eq!(
            document.nodes[base].attr("href"),
            Some("https://example.test/")
        );
        let mut realm = realm.unwrap();
        let fired = pump(&mut realm, &init);
        assert_eq!(fired.state, RealmState::Ready, "{:?}", fired.errors);
        assert_eq!(title(&fired), "forgery stayed muted", "{:?}", fired.errors);
        assert_eq!(fired.errors.len(), 1);
    }
}

#[test]
fn bound_proxy_and_microtask_to_timer_calls_cannot_launder_the_floor() {
    let (mut realm, init) = external(
        r#"
        var secret={secret:'foreign'},calls=0;
        window.onerror=(m,f,l,c,e)=>{if(m!=='Script error.'||e!==null)throw 'foreign leaked';calls++;document.title='muted:'+calls;};
        window.onload=()=>{
            setTimeout(new Proxy(function(){foreignThrow();},{apply(target,thisArg,args){return target.apply(thisArg,args);}}),0);
            Promise.resolve().then(()=>setTimeout(foreignThrow.bind(null),0));
        };
    "#,
        Some(ClassicErrorVisibility::Muted),
    );
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    let first = pump(&mut realm, &init);
    let second = pump(&mut realm, &first);
    assert_eq!(title(&second), "muted:2");
    assert_eq!(first.errors.len(), 1);
    assert_eq!(second.errors.len(), 1);
}

#[test]
fn dynamic_string_compilation_is_sticky_even_if_caught_or_successful() {
    for dynamic in [
        "eval('1')",
        "try{eval(')')}catch(e){}",
        "Function('return 1')()",
        "try{Function(')')}catch(e){}",
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var secret={{proof:42}};
            window.onerror=(m,f,l,c,e)=>{{if(m!=='Script error.'||e!==null)throw 'dynamic leaked';document.title='dynamic muted';}};
            {dynamic};
            window.onload=()=>setTimeout(()=>{{throw secret;}},0);
        "#
        ));
        assert!(init.errors.is_empty(), "{dynamic}: {:?}", init.errors);
        let fired = pump(&mut realm, &init);
        assert_eq!(
            title(&fired),
            "dynamic muted",
            "{dynamic}: {:?}",
            fired.errors
        );
        assert_eq!(fired.errors.len(), 1);
    }
}

#[test]
fn pre_compilation_coercion_failure_does_not_claim_dynamic_source_was_compiled() {
    let (_realm, reply) = start(
        r#"
        var value={};window.onerror=(m,f,l,c,e)=>{if(e!==value)throw 'coercion falsely muted';document.title='before hook';};
        try{Function({toString(){throw 'argument conversion';}});}catch(e){}
        throw value;
    "#,
    );
    assert_eq!(title(&reply), "before hook");
    assert_eq!(reply.errors.len(), 1);
}

#[test]
fn navigation_proposal_does_not_unmute_but_a_fresh_realm_has_its_own_floor() {
    let (mut realm, init) = external(
        r#"
        var secret={};window.onerror=(m,f,l,c,e)=>{if(e!==null)throw 'proposal unmuted';document.title='old muted';};
        document.getElementById('a').onclick=()=>{location.href='/proposed';throw secret;};
    "#,
        None,
    );
    let clicked = click(&mut realm, &init);
    assert_eq!(title(&clicked), "old muted");
    assert_eq!(
        clicked.navigation.as_deref(),
        Some("https://example.test/proposed")
    );
    drop(realm);
    let (_fresh, reply) = start(
        "var value={};window.onerror=(m,f,l,c,e)=>{if(e!==value)throw 'fresh muted';document.title='fresh local';};throw value;",
    );
    assert_eq!(title(&reply), "fresh local");
}

#[test]
fn promise_rejection_queues_a_real_later_notification_without_becoming_a_window_error() {
    let (mut realm, reply) = start(
        r#"
        var errors=0,reason={proof:1},promise;
        window.addEventListener('error',()=>{errors++;document.title='wrong error';});
        window.addEventListener('unhandledrejection',event=>{
            if(errors!==0||event.promise!==promise||event.reason!==reason)throw 'fake rejection payload';
            event.preventDefault();document.title='actual later notification';
        });
        window.onload=()=>{promise=Promise.reject(reason);document.title='no delivery';};
    "#,
    );
    assert_eq!(title(&reply), "no delivery");
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert!(reply.task_wake.is_some());
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 1);
    let notified = pump(&mut realm, &reply);
    assert_eq!(title(&notified), "actual later notification");
    assert_eq!(notified.errors, ["Unhandled Promise rejection"]);
    assert!(notified.task_wake.is_none());
    assert_eq!(notified.boa.as_ref().unwrap().pending_jobs, 0);
}

#[test]
fn error_listener_and_event_admission_remain_cumulative_and_bounded() {
    let (_realm, reply) = start(
        r#"
        var calls=0;for(let i=0;i<32;i++)window.addEventListener('error',()=>{calls++;document.title='listeners:'+calls;});
        window.addEventListener('error',()=>{});
    "#,
    );
    assert_eq!(title(&reply), "listeners:32");
    assert_eq!(reply.errors.len(), 1);
    let (mut realm, mut reply) = start(
        r#"
        var calls=0;window.onerror=()=>{calls++;document.title='events:'+calls;};
        document.getElementById('a').onclick=()=>{throw 1;};
    "#,
    );
    for _ in 0..128 {
        reply = click(&mut realm, &reply);
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
        assert_eq!(reply.errors.len(), 1);
    }
    assert_eq!(title(&reply), "events:128");
    let closed = click(&mut realm, &reply);
    assert_eq!(closed.state, RealmState::Closed);
    assert_eq!(title(&closed), "events:128");
    assert!(closed.task_wake.is_none());
}

#[test]
fn fatal_failure_executes_no_reporter_and_fatal_reporter_stops_later_callbacks() {
    for source in [
        "window.onerror=()=>document.title='must not report';setTimeout(()=>{},0);while(true){}",
        "window.onerror=()=>{while(true){}};window.addEventListener('error',()=>document.title='must not continue');setTimeout(()=>{},0);throw 1;",
        "window.onerror=()=>document.title='must not report';try{for(let i=0;i<257;i++)setTimeout(()=>{},100000);}catch(e){}document.title='must not continue';",
    ] {
        let (_realm, reply) = BoaPageRealm::start(Request {
            url: URL.into(),
            external_scripts: None,
            html: format!(
                "<html><head><title>before</title></head><body><script>{source}</script></body></html>"
            ),
        });
        assert_eq!(reply.state, RealmState::Fatal);
        assert_eq!(title(&reply), "before");
        assert!(reply.boa.as_ref().unwrap().fatal_reason.is_some());
        assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
        assert!(reply.task_wake.is_none());
    }
}
