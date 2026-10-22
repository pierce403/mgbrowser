#![cfg(feature = "modern")]
//! Frozen authored messaging admission/privacy/root controls, not upstream WPT.
//! Public realm/input/task APIs only. Collection follows real checkpoints; no
//! test-only ClearKeptObjects or replacement Context repairs ownership.
use mg_butane::modern::{CUMULATIVE_JOB_LIMIT, PENDING_JOB_LIMIT, PROFILE, Report};
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

const URL: &str = "https://example.test/message-resources";
const GAP: &str =
    "Promise rejection observation suppressed by source-exposure policy; handled status unknown.";
const UNHANDLED: &str = "Unhandled Promise rejection";

fn request(source: &str) -> Request {
    Request {
        url: URL.into(),
        external_scripts: None,
        html: format!(
            "<html><head><title>before</title></head><body><a id=a href=/event-trap>Owned input</a><script>{source}</script></body></html>"
        ),
    }
}
fn start(source: &str) -> (BoaPageRealm, SessionReply) {
    // Capacity/clone workloads start during genuine load so both mandatory
    // permits are already begun. Keep their inclusive accounting visible.
    let (mut realm, init) = raw_start(&format!("window.onload=function(){{{source}\n}};"));
    assert_eq!(
        (report(&init).jobs_admitted, report(&init).jobs_executed),
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
fn raw_start(source: &str) -> (BoaPageRealm, SessionReply) {
    let (realm, reply) = BoaPageRealm::start(request(source));
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    report(&reply);
    (realm.expect("owned retained realm"), reply)
}
fn finish_load(realm: &mut BoaPageRealm, mut reply: SessionReply) -> SessionReply {
    for _ in 0..4 {
        if reply.lifecycle.unwrap().phase == LifecyclePhase::LoadComplete {
            return reply;
        }
        reply = pump(realm, &reply);
        assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    }
    panic!("owned lifecycle did not complete")
}
fn external(
    source: &str,
    visibility: Option<ClassicErrorVisibility>,
) -> (BoaPageRealm, SessionReply) {
    let mut input = request(source);
    input.html = input.html.replace(
        "</body>",
        "<script id=external src='/owned.js' defer></script></body>",
    );
    let parsed = document::parse_with_scripting(&input.html, URL, true);
    input.external_scripts = Some(vec![ExternalScriptSource {
        node: parsed.query_selector(0, "#external").unwrap().unwrap(),
        source: "void 0;".into(),
        provenance: visibility.map(|error_visibility| ClassicSourceProvenance {
            response_url: if error_visibility == ClassicErrorVisibility::Muted {
                "https://foreign.test/private.js".into()
            } else {
                "https://example.test/owned.js".into()
            },
            error_visibility,
        }),
    }]);
    let (realm, reply) = BoaPageRealm::start(input);
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    report(&reply);
    (realm.expect("owned external-source realm"), reply)
}
fn report(reply: &SessionReply) -> &Report {
    let report = reply.boa.as_ref().expect("actual modern accounting");
    assert_eq!(report.profile, PROFILE);
    assert!(reply.accounting_valid(), "{report:?}");
    report
}
fn projected(reply: &SessionReply) -> Document {
    document::project_nodes(reply.snapshot.as_ref().unwrap().nodes.clone(), URL).unwrap()
}
fn title(reply: &SessionReply) -> String {
    projected(reply).title
}
fn no_work(reply: &SessionReply) {
    assert_eq!(report(reply).pending_jobs, 0);
    assert!(reply.task_wake.is_none());
}
fn pump(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let wake = previous.task_wake.as_ref().expect("advertised owned task");
    assert!(wake.delay_ms <= 100, "only bounded short-delay controls");
    if wake.delay_ms != 0 {
        std::thread::sleep(Duration::from_millis(u64::from(wake.delay_ms)));
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
    report(&reply).validate_after(report(previous)).unwrap();
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
    assert_eq!(reply.revision, previous.revision + 1);
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    report(&reply).validate_after(report(previous)).unwrap();
    reply
}

#[test]
fn messages_timers_and_promise_jobs_share_the_exact_pending_cap() {
    for source in [
        "for(let i=0;i<128;i++)setTimeout(()=>{},100000);for(let i=0;i<127;i++)Promise.resolve().then(()=>{});postMessage(1,'/');postMessage(2,'/');document.title='must not publish';",
        "for(let i=0;i<128;i++)postMessage(i,'/');for(let i=0;i<127;i++)setTimeout(()=>{},100000);Promise.resolve().then(()=>{});postMessage(1,'/');document.title='must not publish';",
    ] {
        let (_realm, reply) = BoaPageRealm::start(request(source));
        assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
        assert_eq!(report(&reply).jobs_admitted, PENDING_JOB_LIMIT);
        assert_eq!(report(&reply).jobs_executed, 0);
        assert!(
            report(&reply)
                .fatal_reason
                .as_ref()
                .unwrap()
                .contains("shared job/task")
        );
        no_work(&reply);
        assert!(reply.navigation.is_none());
        assert_eq!(title(&reply), "before");
    }
}

#[test]
fn exact_capacity_delivery_frees_one_pending_slot_before_callback_admission() {
    let (mut realm, init) = start(
        r#"
        var calls=0;
        window.onmessage=()=>{calls++;if(calls===1)postMessage('later','/');document.title='delivered '+calls;};
        for(let i=0;i<128;i++)setTimeout(()=>{},100000);
        for(let i=0;i<127;i++)Promise.resolve().then(()=>{});
        postMessage('first','/');
    "#,
    );
    assert_eq!(report(&init).jobs_admitted, PENDING_JOB_LIMIT + 2);
    assert_eq!(report(&init).jobs_executed, 129);
    assert_eq!(report(&init).pending_jobs, 129);
    let first = pump(&mut realm, &init);
    assert_eq!(first.state, RealmState::Ready, "{:?}", first.errors);
    assert!(first.errors.is_empty());
    assert_eq!(report(&first).jobs_admitted, 259);
    assert_eq!(report(&first).jobs_executed, 130);
    assert_eq!(report(&first).pending_jobs, 129);
    assert_eq!(title(&first), "delivered 1");
}

#[test]
fn full_pending_capacity_delivery_begins_its_permit_before_callback_posts_another_message() {
    let (mut realm, init) = start(
        r#"
        window.onmessage=()=>{postMessage('later','/');document.title='full-capacity callback';};
        for(let i=0;i<255;i++)setTimeout(()=>{},100000);
        postMessage('first','/');
    "#,
    );
    assert_eq!(report(&init).jobs_admitted, PENDING_JOB_LIMIT + 2);
    assert_eq!(report(&init).jobs_executed, 2);
    assert_eq!(report(&init).pending_jobs, PENDING_JOB_LIMIT);
    let first = pump(&mut realm, &init);
    assert_eq!(first.state, RealmState::Ready, "{:?}", first.errors);
    assert!(first.errors.is_empty(), "{:?}", first.errors);
    assert_eq!(report(&first).jobs_admitted, 259);
    assert_eq!(report(&first).jobs_executed, 3);
    assert_eq!(report(&first).pending_jobs, PENDING_JOB_LIMIT);
    assert_eq!(title(&first), "full-capacity callback");
    assert!(first.task_wake.is_some());
}

#[test]
fn failed_clone_releases_pending_but_does_not_refund_lifetime_work() {
    let (mut realm, init) = start(
        r#"
        // Two lifecycle admissions + 2,045 failed clones + the final message
        // hit the exact unchanged 2,048 lifetime cap before the next attempt.
        for(let i=0;i<2045;i++){
            try{postMessage(Symbol('owned'),'/');throw 'invalid clone accepted';}
            catch(error){if(!(error instanceof DOMException)||error.name!=='DataCloneError')throw error;}
        }
        window.onmessage=()=>{document.title='last admitted';};postMessage('last','/');
        document.getElementById('a').onclick=event=>{
            event.preventDefault();try{postMessage('too many','/');}catch(error){document.title='caught fatal';}
            document.title='must not continue';
        };
    "#,
    );
    assert_eq!(report(&init).jobs_admitted, CUMULATIVE_JOB_LIMIT);
    assert_eq!(report(&init).jobs_executed, 2);
    assert_eq!(report(&init).pending_jobs, 1);
    let delivered = pump(&mut realm, &init);
    assert_eq!(delivered.state, RealmState::Ready, "{:?}", delivered.errors);
    assert!(delivered.errors.is_empty());
    assert_eq!(title(&delivered), "last admitted");
    assert_eq!(report(&delivered).jobs_executed, 3);
    no_work(&delivered);
    let target = projected(&delivered)
        .query_selector(0, "#a")
        .unwrap()
        .unwrap();
    let fatal = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    assert_eq!(fatal.state, RealmState::Fatal, "{:?}", fatal.errors);
    assert_eq!(report(&fatal).jobs_admitted, CUMULATIVE_JOB_LIMIT);
    no_work(&fatal);
    assert_eq!(title(&fatal), "last admitted");
}

#[test]
fn preclone_argument_syntax_and_transfer_failures_spend_no_clone_permit() {
    let (_realm, reply) = start(
        r#"
        for(let i=0;i<256;i++){
            try{postMessage(1,'not an absolute origin');}catch(error){if(error.name!=='SyntaxError')throw error;}
            try{postMessage(1,'/',[new ArrayBuffer(1)]);}catch(error){if(error.name!=='NotSupportedError')throw error;}
            try{postMessage(1,{get targetOrigin(){throw 42;}});}catch(error){if(error!==42)throw error;}
        }
        document.title='preclone failures';
    "#,
    );
    assert_eq!(title(&reply), "preclone failures");
    assert_eq!(report(&reply).jobs_admitted, 2);
    assert_eq!(report(&reply).jobs_executed, 2);
    no_work(&reply);
}

#[test]
fn message_delivery_preserves_the_exact_cumulative_two_hundred_fifty_six_event_cap() {
    // A direct library control isolates the unchanged event-record allowance.
    // It does not change/bypass the real parent's separate 64-transaction cap.
    let (mut realm, init) = raw_start(
        r#"
        var calls=0;
        window.onerror=()=>{document.title='event-limit reporter must not run';};
        window.onmessage=()=>{
            calls++;document.title='delivered '+calls;if(calls<257)postMessage(calls+1,'/');
        };
        postMessage(1,'/');
    "#,
    );
    let mut reply = init;
    for executed in 1..=256 {
        reply = pump(&mut realm, &reply);
        if executed == 1 {
            // Keep this exact 256-message/event-record control source-time:
            // no wrapper load listener consumes an invisible extra event.
            // The two unobserved lifecycle tasks run before message #2.
            assert_eq!(report(&reply).jobs_executed, 1);
            assert_eq!(report(&reply).pending_jobs, 3);
            reply = finish_load(&mut realm, reply);
        }
        assert_eq!(
            reply.state,
            RealmState::Ready,
            "message {executed}: {:?}",
            reply.errors
        );
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(title(&reply), format!("delivered {executed}"));
        assert_eq!(report(&reply).jobs_admitted, executed + 3);
        assert_eq!(report(&reply).jobs_executed, executed + 2);
        assert_eq!(report(&reply).pending_jobs, 1);
    }
    let fatal = pump(&mut realm, &reply);
    assert_eq!(fatal.state, RealmState::Fatal, "{:?}", fatal.errors);
    assert_eq!(report(&fatal).jobs_admitted, 259);
    assert_eq!(report(&fatal).jobs_executed, 259);
    assert!(report(&fatal).fatal_reason.is_some());
    assert_eq!(title(&fatal), "delivered 256");
    assert!(fatal.navigation.is_none());
    no_work(&fatal);
}

#[test]
fn oversized_retained_graph_is_rejected_under_existing_host_storage_bound() {
    let (_realm, reply) = BoaPageRealm::start(request(
        r#"
        var large='x';for(let i=0;i<22;i++)large+=large;
        window.onerror=()=>{document.title='resource reporter must not run';};
        try{postMessage(large,'/');}catch(error){document.title='caught storage fatal';}
        postMessage('must not queue','/');document.title='must not continue';
    "#,
    ));
    assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
    assert_eq!(report(&reply).jobs_admitted, 1);
    assert_eq!(report(&reply).jobs_executed, 0);
    assert!(report(&reply).fatal_reason.is_some());
    assert_eq!(title(&reply), "before");
    no_work(&reply);
}

#[test]
fn oversized_owned_string_key_is_fatal_before_unadmitted_utf16_key_copy() {
    let (_realm, reply) = BoaPageRealm::start(request(
        r#"
        var large='k';for(let i=0;i<22;i++)large+=large;
        const source={};source[large]=1;
        window.onerror=()=>{document.title='key-storage reporter must not run';};
        try{postMessage(source,'/');}catch(error){document.title='caught key-storage fatal';}
        postMessage('must not queue','/');document.title='must not continue';
    "#,
    ));
    assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
    assert_eq!(report(&reply).jobs_admitted, 1);
    assert_eq!(report(&reply).jobs_executed, 0);
    assert!(report(&reply).fatal_reason.is_some());
    assert_eq!(title(&reply), "before");
    assert!(reply.navigation.is_none());
    no_work(&reply);
}

#[test]
fn source_identity_memo_is_released_after_serialization_but_queued_snapshot_survives_gc() {
    let (mut realm, init) = start(
        r#"
        var weakSource,weakChild;
        (()=>{const child={proof:42},source={child,alias:child};source.self=source;
            weakSource=new WeakRef(source);weakChild=new WeakRef(child);postMessage(source,'/');})();
        window.onmessage=event=>{
            if(weakSource.deref()!==undefined||weakChild.deref()!==undefined)throw 'clone memo rooted sender';
            if(event.data.child.proof!==42||event.data.child!==event.data.alias||event.data.self!==event.data)
                throw 'queued graph lost';document.title='source released snapshot retained';
        };
    "#,
    );
    boa_gc::force_collect();
    let reply = pump(&mut realm, &init);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(title(&reply), "source released snapshot retained");
    no_work(&reply);
}

#[test]
fn failed_getter_releases_partial_clone_source_memo_and_keeps_only_inner_message() {
    let (mut realm, init) = start(
        r#"
        var weakSource,weakChild;
        (()=>{const child={proof:42},source={child,get broken(){postMessage('inner','/');throw 7;}};
            weakSource=new WeakRef(source);weakChild=new WeakRef(child);
            try{postMessage(source,'/');}catch(error){if(error!==7)throw error;}})();
        window.onmessage=event=>{
            if(event.data!=='inner'||weakSource.deref()!==undefined||weakChild.deref()!==undefined)
                throw 'partial clone retained';document.title='partial clone released';
        };
    "#,
    );
    assert_eq!(report(&init).jobs_admitted, 4);
    assert_eq!(report(&init).pending_jobs, 1);
    boa_gc::force_collect();
    let reply = pump(&mut realm, &init);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(title(&reply), "partial clone released");
    no_work(&reply);
}

#[test]
fn escaped_message_payload_is_traced_until_its_event_is_released() {
    let (mut realm, init) = start(
        r#"
        var saved,weakEvent,weakData,stage=0;
        window.onmessage=event=>{saved=event;weakEvent=new WeakRef(event);weakData=new WeakRef(event.data);};
        postMessage({proof:42},'/');
        document.getElementById('a').onclick=event=>{
            event.preventDefault();
            if(stage===0){
                if(saved!==weakEvent.deref()||saved.data!==weakData.deref()||saved.data.proof!==42||
                    saved.currentTarget!==null||saved.eventPhase!==0)throw 'escaped message lost';
                saved=null;stage=1;document.title='escaped retained';
            }else{
                if(weakEvent.deref()!==undefined||weakData.deref()!==undefined)throw 'hidden message roots';
                document.title='escaped released';
            }
        };
    "#,
    );
    let delivered = pump(&mut realm, &init);
    assert!(delivered.errors.is_empty());
    no_work(&delivered);
    boa_gc::force_collect();
    let escaped = click(&mut realm, &delivered);
    assert!(escaped.errors.is_empty());
    assert_eq!(title(&escaped), "escaped retained");
    boa_gc::force_collect();
    let released = click(&mut realm, &escaped);
    assert!(released.errors.is_empty());
    assert_eq!(title(&released), "escaped released");
}

#[test]
fn floor_fall_with_only_messages_does_not_invent_rejection_observation_gap() {
    let (mut realm, init) = raw_start(
        r#"
        window.onmessage=event=>{if(event.origin!==location.origin||event.data.proof!==42)throw 'message muted';document.title='message survives floor';};
        postMessage({proof:42},'/');eval('void 0');
    "#,
    );
    assert_eq!(report(&init).jobs_admitted, 3);
    assert_eq!(report(&init).pending_jobs, 3);
    let reply = pump(&mut realm, &init);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(title(&reply), "message survives floor");
    let reply = finish_load(&mut realm, reply);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    no_work(&reply);
}

#[test]
fn rejection_privacy_purge_preserves_message_task_origin_and_exact_remaining_work() {
    // This case intentionally expects the sticky observation notice; the
    // diagnostic-free start() helper is not an appropriate admission assertion.
    let (realm, init) = BoaPageRealm::start(request(
        r#"
        window.onunhandledrejection=()=>{throw 'muted notice delivered';};
        window.onmessage=event=>{if(event.origin!=='https://example.test'||event.data!=='owned')throw 'message changed';document.title='message after purge';};
        postMessage('owned','/');Promise.reject({private:42});eval('void 0');
    "#,
    ));
    assert_eq!(init.state, RealmState::Ready);
    report(&init);
    let mut realm = realm.expect("owned retained message realm with expected privacy notice");
    assert_eq!(init.errors, [GAP]);
    assert_eq!(report(&init).jobs_admitted, 4);
    assert_eq!(report(&init).pending_jobs, 3);
    let reply = pump(&mut realm, &init);
    assert_eq!(reply.errors, [GAP]);
    assert_eq!(report(&reply).jobs_executed, 1);
    assert_eq!(title(&reply), "message after purge");
    let reply = finish_load(&mut realm, reply);
    assert_eq!(reply.errors, [GAP]);
    assert_eq!(report(&reply).jobs_executed, 3);
    no_work(&reply);
}

#[test]
fn initial_foreign_or_unknown_floor_does_not_suppress_legitimate_message_payload() {
    for visibility in [None, Some(ClassicErrorVisibility::Muted)] {
        let (mut realm, init) = external(
            r#"
            window.onmessage=event=>{
                if(event.origin!=='https://example.test'||event.source!==window||event.data.proof!==42)
                    throw 'foreign script origin substituted';document.title='document message';
            };
            postMessage({proof:42},'/');
        "#,
            visibility,
        );
        assert!(init.errors.is_empty(), "{:?}", init.errors);
        assert_eq!(report(&init).pending_jobs, 3);
        let reply = pump(&mut realm, &init);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        assert_eq!(title(&reply), "document message");
        let reply = finish_load(&mut realm, reply);
        assert!(reply.errors.is_empty(), "{:?}", reply.errors);
        no_work(&reply);
    }
}

#[test]
fn notification_handler_posted_message_survives_active_rejection_floor_purge() {
    let (mut realm, init) = start(
        r#"
        window.onunhandledrejection=event=>{event.preventDefault();postMessage({proof:42},'/');eval('void 0');};
        window.onmessage=event=>{if(event.data.proof!==42)throw 'message payload purged';document.title='message after notice';};
        Promise.reject({owned:42});
    "#,
    );
    let notice = pump(&mut realm, &init);
    assert_eq!(notice.state, RealmState::Ready, "{:?}", notice.errors);
    assert_eq!(notice.errors, [UNHANDLED, GAP]);
    assert_eq!(report(&notice).jobs_admitted, 4);
    assert_eq!(report(&notice).jobs_executed, 3);
    assert_eq!(report(&notice).pending_jobs, 1);
    let message = pump(&mut realm, &notice);
    assert_eq!(message.errors, [GAP]);
    assert_eq!(title(&message), "message after notice");
    no_work(&message);
}

#[test]
fn invalid_input_closes_all_message_timer_and_rejection_work_without_replay() {
    let (mut realm, init) = start(
        "postMessage({proof:1},'/');Promise.reject({proof:2});setTimeout(()=>{throw 'must not run';},100000);",
    );
    assert_eq!(report(&init).jobs_admitted, 5);
    assert_eq!(report(&init).pending_jobs, 3);
    let rejected = realm.dispatch(SessionInput {
        kind: InputKind::Click { target: usize::MAX },
        edits: vec![],
    });
    assert_eq!(rejected.state, RealmState::Closed);
    assert!(rejected.snapshot.is_none());
    assert_eq!(report(&rejected).jobs_admitted, 5);
    assert_eq!(report(&rejected).jobs_executed, 2);
    no_work(&rejected);
    let replay = realm.pump_tasks(TaskPump {
        wake_token: init.task_wake.unwrap().token,
    });
    assert_eq!(replay.state, RealmState::Closed);
    no_work(&replay);
    drop(realm);
    boa_gc::force_collect();
}

#[test]
fn fatal_message_callback_stops_later_listeners_jobs_messages_and_timers() {
    let (mut realm, init) = start(
        r#"
        window.onmessage=()=>{while(true){}};
        window.addEventListener('message',()=>{document.title='must not run';});
        window.onerror=()=>{document.title='fatal reporter must not run';};
        postMessage({proof:1},'/');postMessage({proof:2},'/');setTimeout(()=>{document.title='timer must not run';},100000);
    "#,
    );
    assert_eq!(report(&init).jobs_admitted, 5);
    let fatal = pump(&mut realm, &init);
    assert_eq!(fatal.state, RealmState::Fatal, "{:?}", fatal.errors);
    assert_eq!(report(&fatal).jobs_admitted, 5);
    assert_eq!(report(&fatal).jobs_executed, 3);
    assert!(report(&fatal).fatal_reason.is_some());
    assert_eq!(title(&fatal), "before");
    assert!(fatal.navigation.is_none());
    no_work(&fatal);
    // Pinned Boa may retain fatal VM arguments until actual Context teardown;
    // do not assert artificial immediate whole-VM collection or clear kept roots.
    drop(realm);
    boa_gc::force_collect();
}

#[test]
fn hostile_origin_conversion_cannot_catch_fatal_or_retain_an_inner_message() {
    let (_realm, reply) = BoaPageRealm::start(request(
        r#"
        try{postMessage('outer',{targetOrigin:{toString(){postMessage('inner','/');while(true){}}}});}
        catch(error){document.title='caught fatal';}
        document.title='must not continue';
    "#,
    ));
    assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
    assert_eq!(report(&reply).jobs_admitted, 1);
    assert_eq!(report(&reply).jobs_executed, 0);
    no_work(&reply);
    assert_eq!(title(&reply), "before");
}

#[test]
fn hostile_transfer_iterator_cannot_admit_outer_work_or_run_later_target_conversion_after_fatal() {
    let (_realm, reply) = BoaPageRealm::start(request(
        r#"
        const transfer={ [Symbol.iterator](){return {next(){postMessage('inner','/');while(true){}}};}};
        try{postMessage({get value(){document.title='clone must not run';return 1;}},{
            get transfer(){return transfer;},get targetOrigin(){document.title='target must not run';return '/';}
        });}catch(error){document.title='caught fatal';}
        document.title='must not continue';
    "#,
    ));
    assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
    assert_eq!(report(&reply).jobs_admitted, 1);
    assert_eq!(report(&reply).jobs_executed, 0);
    no_work(&reply);
    assert_eq!(title(&reply), "before");
}

#[test]
fn fatal_iterator_close_during_object_item_conversion_overrides_catchable_typeerror() {
    let (_realm, reply) = BoaPageRealm::start(request(
        r#"
        const transfer={ [Symbol.iterator](){return {
            next(){return {done:false,value:1};},return(){postMessage('inner','/');while(true){}}
        };}};
        try{postMessage('outer',{transfer,get targetOrigin(){document.title='target must not run';return '/';}});}
        catch(error){document.title='caught conversion error';}
        document.title='must not continue';
    "#,
    ));
    assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
    assert_eq!(report(&reply).jobs_admitted, 1);
    assert_eq!(report(&reply).jobs_executed, 0);
    no_work(&reply);
    assert_eq!(title(&reply), "before");
}

#[test]
fn clone_getter_cannot_catch_fatal_or_publish_queued_inner_and_outer_graphs() {
    let (_realm, reply) = BoaPageRealm::start(request(
        r#"
        try{postMessage({get value(){postMessage('inner','/');while(true){}}},'/');}
        catch(error){document.title='caught fatal';}
        document.title='must not continue';
    "#,
    ));
    assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
    assert_eq!(report(&reply).jobs_admitted, 2);
    assert_eq!(report(&reply).jobs_executed, 0);
    no_work(&reply);
    assert_eq!(title(&reply), "before");
}
