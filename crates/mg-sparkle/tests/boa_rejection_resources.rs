#![cfg(feature = "modern")]
//! Frozen authored rejection accounting/privacy controls, not upstream WPT tests.
//! Only existing public realm/task/input APIs are used. GC checks rely on normal
//! page checkpoints and never repair roots with test-only ClearKeptObjects.

use mg_butane::modern::{CUMULATIVE_JOB_LIMIT, PENDING_JOB_LIMIT, PROFILE, Report};
use mg_sparkle::{
    document::{self, Document},
    js_browser::{
        ClassicErrorVisibility, ClassicSourceProvenance, ExternalScriptSource, Request,
        boa::BoaPageRealm,
    },
    page_session::{DefaultAction, InputKind, RealmState, SessionInput, SessionReply, TaskPump},
};
use std::time::Duration;

const URL: &str = "https://example.test/rejection-resources";
const NOTICE: &str = "Unhandled Promise rejection";
const GAP: &str =
    "Promise rejection observation suppressed by source-exposure policy; handled status unknown.";

fn request(sources: &[&str]) -> Request {
    let mut html = String::from(
        "<html><head><title>before</title></head><body><a id=a href=/trap>Owned input</a>",
    );
    for source in sources {
        html.push_str("<script>");
        html.push_str(source);
        html.push_str("</script>");
    }
    html.push_str("</body></html>");
    Request {
        url: URL.into(),
        html,
        external_scripts: None,
    }
}

fn start(source: &str) -> (BoaPageRealm, SessionReply) {
    let (realm, reply) = BoaPageRealm::start(request(&[source]));
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    report(&reply);
    (realm.expect("owned retained realm"), reply)
}

fn external(
    source: &str,
    visibility: Option<ClassicErrorVisibility>,
) -> (BoaPageRealm, SessionReply) {
    let mut input = request(&[source]);
    input.html = input.html.replace(
        "</body>",
        "<script id=external src='/owned.js' defer></script></body>",
    );
    let parsed = document::parse_with_scripting(&input.html, URL, true);
    let node = parsed.query_selector(0, "#external").unwrap().unwrap();
    input.external_scripts = Some(vec![ExternalScriptSource {
        node,
        source: "void 0;".into(),
        provenance: visibility.map(|error_visibility| ClassicSourceProvenance {
            response_url: if error_visibility == ClassicErrorVisibility::Muted {
                "https://foreign.test/owned.js".into()
            } else {
                "https://example.test/owned.js".into()
            },
            error_visibility,
        }),
    }]);
    let (realm, reply) = BoaPageRealm::start(input);
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    report(&reply);
    (realm.expect("owned retained external realm"), reply)
}

fn report(reply: &SessionReply) -> &Report {
    let report = reply.boa.as_ref().expect("real Boa accounting");
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

fn click(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let target = projected(previous)
        .query_selector(0, "#a")
        .unwrap()
        .unwrap();
    let reply = realm.dispatch(SessionInput {
        kind: InputKind::Click { target },
        edits: vec![],
    });
    report(&reply).validate_after(report(previous)).unwrap();
    reply
}

fn pump(realm: &mut BoaPageRealm, previous: &SessionReply) -> SessionReply {
    let wake = previous.task_wake.as_ref().expect("advertised owned task");
    assert!(wake.delay_ms <= 100, "only bounded short-delay owned tasks");
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

fn gap(reply: &SessionReply) {
    assert_eq!(reply.errors, [GAP], "one persistent data-only gap notice");
    assert_eq!(report(reply).pending_jobs, 0);
    assert!(reply.task_wake.is_none());
    assert_eq!(reply.state, RealmState::Ready);
}

#[test]
fn mixed_rejections_timers_and_promise_jobs_share_the_exact_pending_cap() {
    for source in [
        "for(let i=0;i<128;i++)setTimeout(()=>{},100000);for(let i=0;i<127;i++)Promise.resolve().then(()=>{});Promise.reject(1);Promise.reject(2);document.title='must not publish';",
        "for(let i=0;i<128;i++)Promise.reject(i);for(let i=0;i<127;i++)setTimeout(()=>{},100000);Promise.resolve().then(()=>{});setTimeout(()=>{},100000);document.title='must not publish';",
    ] {
        let (_realm, reply) = BoaPageRealm::start(request(&[source]));
        assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
        let stats = report(&reply);
        assert_eq!(stats.jobs_admitted, PENDING_JOB_LIMIT);
        assert_eq!(stats.jobs_executed, 0);
        assert_eq!(stats.pending_jobs, 0);
        assert!(
            stats
                .fatal_reason
                .as_ref()
                .unwrap()
                .contains("shared job/task")
        );
        assert!(reply.task_wake.is_none());
        assert!(reply.navigation.is_none());
        assert_eq!(title(&reply), "before");
    }
}

#[test]
fn freezing_an_exact_capacity_batch_does_not_spend_an_extra_work_slot() {
    let (mut realm, init) = start(
        "for(let i=0;i<128;i++)setTimeout(()=>{},100000);for(let i=0;i<127;i++)Promise.resolve().then(()=>{});Promise.reject('owned');",
    );
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    assert_eq!(report(&init).jobs_admitted, PENDING_JOB_LIMIT);
    assert_eq!(report(&init).jobs_executed, 127);
    assert_eq!(report(&init).pending_jobs, 129);
    let notified = pump(&mut realm, &init);
    assert_eq!(notified.state, RealmState::Ready);
    assert_eq!(notified.errors, [NOTICE]);
    assert_eq!(report(&notified).jobs_admitted, PENDING_JOB_LIMIT);
    assert_eq!(report(&notified).jobs_executed, 128);
    assert_eq!(report(&notified).pending_jobs, 128);
    assert!(notified.task_wake.as_ref().unwrap().delay_ms > 100);
    let closed = realm.dispatch(SessionInput {
        kind: InputKind::Click { target: usize::MAX },
        edits: vec![],
    });
    assert_eq!(closed.state, RealmState::Closed);
    assert_eq!(report(&closed).pending_jobs, 0);
    assert!(closed.task_wake.is_none());
}

#[test]
fn immediate_handles_cancel_pending_but_spend_exactly_2048_lifetime_admissions() {
    // Each of 16 ordinary source checkpoints drains 64 reaction jobs. Every
    // reject/catch pair also spends one canceled notice admission, not a refund.
    let source = "for(let i=0;i<64;i++){const p=Promise.reject(i);p.catch(()=>{});}";
    let sources = vec![source; 16];
    let (_realm, reply) = BoaPageRealm::start(request(&sources));
    assert_eq!(reply.state, RealmState::Ready, "{:?}", reply.errors);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 16);
    assert_eq!(report(&reply).source_admissions, 16);
    assert_eq!(report(&reply).jobs_admitted, CUMULATIVE_JOB_LIMIT);
    assert_eq!(report(&reply).jobs_executed, 1024);
    assert_eq!(report(&reply).pending_jobs, 0);
    assert!(reply.task_wake.is_none());
}

#[test]
fn canceled_candidate_work_cannot_renew_the_lifetime_budget() {
    let source = "for(let i=0;i<64;i++){const p=Promise.reject(i);p.catch(()=>{});}";
    let mut sources = vec![source; 16];
    sources.push("Promise.reject('past the lifetime cap');document.title='must not publish';");
    let (_realm, reply) = BoaPageRealm::start(request(&sources));
    assert_eq!(reply.state, RealmState::Fatal, "{:?}", reply.errors);
    assert_eq!(report(&reply).jobs_admitted, CUMULATIVE_JOB_LIMIT);
    assert_eq!(report(&reply).jobs_executed, 1024);
    assert_eq!(report(&reply).pending_jobs, 0);
    assert!(reply.task_wake.is_none());
    assert_eq!(title(&reply), "before");
}

#[test]
fn handling_a_frozen_candidate_cancels_only_its_pending_reservation() {
    let (mut realm, init) = start(
        r#"
        var p=Promise.reject({owned:true});
        document.getElementById('a').onclick=event=>{
            event.preventDefault();p.catch(()=>{document.title='caught before task';});p=null;
        };
    "#,
    );
    assert!(init.errors.is_empty());
    assert_eq!(report(&init).jobs_admitted, 1);
    assert_eq!(report(&init).pending_jobs, 1);
    assert!(init.task_wake.is_some());
    let handled = click(&mut realm, &init);
    assert_eq!(handled.state, RealmState::Ready, "{:?}", handled.errors);
    assert!(handled.errors.is_empty(), "{:?}", handled.errors);
    assert_eq!(title(&handled), "caught before task");
    assert_eq!(report(&handled).jobs_admitted, 2);
    assert_eq!(report(&handled).jobs_executed, 1);
    assert_eq!(report(&handled).pending_jobs, 0);
    assert!(
        handled.task_wake.is_none(),
        "canceled empty batch has no task"
    );
}

#[test]
fn genuinely_late_handling_admits_its_own_later_notification_work() {
    let (mut realm, init) = start(
        r#"
        var p=Promise.reject({owned:true});
        window.addEventListener('unhandledrejection',event=>{event.preventDefault();document.title='unhandled';});
        window.addEventListener('rejectionhandled',event=>{
            if(event.promise!==p)throw 'wrong handled promise';document.title='handled task';
        });
        document.getElementById('a').onclick=event=>{
            event.preventDefault();p.catch(()=>{document.title='reaction';});
        };
    "#,
    );
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    let notified = pump(&mut realm, &init);
    assert_eq!(notified.errors, [NOTICE]);
    assert_eq!(report(&notified).jobs_admitted, 1);
    assert_eq!(report(&notified).jobs_executed, 1);
    assert_eq!(report(&notified).pending_jobs, 0);
    let handled = click(&mut realm, &notified);
    assert_eq!(title(&handled), "reaction");
    assert_eq!(report(&handled).jobs_admitted, 3);
    assert_eq!(report(&handled).jobs_executed, 2);
    assert_eq!(report(&handled).pending_jobs, 1);
    let later = pump(&mut realm, &handled);
    assert_eq!(title(&later), "handled task");
    assert!(later.errors.is_empty(), "{:?}", later.errors);
    assert_eq!(report(&later).jobs_admitted, 3);
    assert_eq!(report(&later).jobs_executed, 3);
    assert_eq!(report(&later).pending_jobs, 0);
    assert!(later.task_wake.is_none());
}

#[test]
fn initial_unknown_or_foreign_floor_suppresses_payloads_without_task_admission() {
    for visibility in [None, Some(ClassicErrorVisibility::Muted)] {
        let (mut realm, init) = external(
            r#"
            var calls=0,traps=0;
            window.addEventListener('unhandledrejection',()=>{calls++;});
            window.addEventListener('rejectionhandled',()=>{calls++;});
            window.addEventListener('error',()=>{calls++;});
            const secret=new Proxy({},{get(){traps++;throw 'private getter';}});
            Promise.reject(secret).catch(()=>{});
            window.onload=()=>{document.title='muted:'+calls+':'+traps;};
            document.getElementById('a').onclick=event=>{event.preventDefault();document.title='later:'+calls+':'+traps;};
        "#,
            visibility,
        );
        gap(&init);
        assert_eq!(title(&init), "muted:0:0");
        // Only the actual catch reaction, never a suppressed-notice permit.
        assert_eq!(report(&init).jobs_admitted, 1);
        assert_eq!(report(&init).jobs_executed, 1);
        let later = click(&mut realm, &init);
        gap(&later);
        assert_eq!(title(&later), "later:0:0");
        assert_eq!(report(&later).jobs_admitted, 1);
    }
}

#[test]
fn same_origin_proof_does_not_blanket_suppress_local_notifications() {
    let (mut realm, init) = external(
        "window.addEventListener('unhandledrejection',()=>{document.title='proven local';});Promise.reject({owned:true});",
        Some(ClassicErrorVisibility::SameOrigin),
    );
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    assert_eq!(report(&init).pending_jobs, 1);
    let notified = pump(&mut realm, &init);
    assert_eq!(notified.errors, [NOTICE]);
    assert_eq!(title(&notified), "proven local");
    assert!(notified.task_wake.is_none());
}

#[test]
fn successful_or_failed_dynamic_compilation_suppresses_before_retention_and_stays_partial() {
    for dynamic in [
        "eval('1')",
        "try{eval(')')}catch(e){}",
        "Function('return 1')()",
        "try{Function(')')}catch(e){}",
    ] {
        let (mut realm, init) = start(&format!(
            r#"
            var calls=0,traps=0;
            window.addEventListener('unhandledrejection',()=>calls++);
            window.addEventListener('rejectionhandled',()=>calls++);
            {dynamic};
            Promise.reject(new Proxy({{}},{{get(){{traps++;throw 'getter';}}}})).catch(()=>{{}});
            window.onload=()=>{{document.title='dynamic:'+calls+':'+traps;}};
            document.getElementById('a').onclick=event=>{{event.preventDefault();document.title='later:'+calls+':'+traps;}};
        "#
        ));
        gap(&init);
        assert_eq!(title(&init), "dynamic:0:0", "{dynamic}");
        assert_eq!(report(&init).jobs_admitted, 1, "{dynamic}");
        let later = click(&mut realm, &init);
        gap(&later);
        assert_eq!(title(&later), "later:0:0", "{dynamic}");
    }
    // Observation loss belongs to one realm, not a process-global privacy flag.
    let (mut fresh, init) = start("Promise.reject('fresh local');");
    assert!(init.errors.is_empty());
    let notified = pump(&mut fresh, &init);
    assert_eq!(notified.errors, [NOTICE]);
}

#[test]
fn a_floor_change_before_checkpoint_cancels_the_old_candidate_without_refund() {
    let (_realm, init) = start(
        "Promise.reject({owned:true});try{eval(')')}catch(e){}document.title='floor changed';",
    );
    gap(&init);
    assert_eq!(title(&init), "floor changed");
    assert_eq!(report(&init).jobs_admitted, 1);
    assert_eq!(report(&init).jobs_executed, 0);
}

#[test]
fn a_floor_change_after_freezing_disarms_the_batch_and_persists_the_gap() {
    let (mut realm, init) = start(
        r#"
        Promise.reject({owned:true});
        document.getElementById('a').onclick=event=>{event.preventDefault();eval('1');document.title='floor fell before pump';};
    "#,
    );
    assert!(init.errors.is_empty());
    assert_eq!(report(&init).pending_jobs, 1);
    assert!(init.task_wake.is_some());
    let muted = click(&mut realm, &init);
    gap(&muted);
    assert_eq!(title(&muted), "floor fell before pump");
    assert_eq!(report(&muted).jobs_admitted, 1);
    assert_eq!(report(&muted).jobs_executed, 0);
    let later = click(&mut realm, &muted);
    gap(&later);
    assert_eq!(report(&later).jobs_admitted, 1);
}

#[test]
fn suppression_floods_keep_one_notice_and_no_candidate_work_or_wake() {
    let (mut realm, init) = external(
        r#"
        var calls=0;
        window.addEventListener('unhandledrejection',()=>calls++);
        window.addEventListener('rejectionhandled',()=>calls++);
        for(let i=0;i<4096;i++)Promise.reject(i);
        document.getElementById('a').onclick=event=>{
            event.preventDefault();for(let i=0;i<4096;i++)Promise.reject(i);document.title='suppressed:'+calls;
        };
    "#,
        None,
    );
    gap(&init);
    assert_eq!(report(&init).jobs_admitted, 0);
    let later = click(&mut realm, &init);
    gap(&later);
    assert_eq!(report(&later).jobs_admitted, 0);
    assert_eq!(report(&later).jobs_executed, 0);
    assert_eq!(title(&later), "suppressed:0");
}

#[test]
fn suppressed_unknown_values_have_no_hidden_strong_roots() {
    let (mut realm, init) = external(
        r#"
        var weakPromise,weakReason;
        (()=>{const reason={proof:42},promise=Promise.reject(reason);weakPromise=new WeakRef(promise);weakReason=new WeakRef(reason);})();
        document.getElementById('a').onclick=event=>{
            event.preventDefault();
            if(weakPromise.deref()!==undefined||weakReason.deref()!==undefined)throw 'suppressed values retained';
            document.title='suppressed roots released';
        };
    "#,
        None,
    );
    gap(&init);
    assert_eq!(report(&init).jobs_admitted, 0);
    boa_gc::force_collect();
    let checked = click(&mut realm, &init);
    gap(&checked);
    assert_eq!(title(&checked), "suppressed roots released");
}

#[test]
fn pending_roots_survive_gc_but_notified_membership_is_genuinely_weak() {
    let (mut realm, init) = start(
        r#"
        var weakPromise,weakReason,stage=0;
        window.addEventListener('unhandledrejection',event=>{
            if(event.promise!==weakPromise.deref()||event.reason!==weakReason.deref())throw 'pending identity lost';
            stage=1;document.title='delivered';
        });
        (()=>{const reason={proof:42},promise=Promise.reject(reason);weakPromise=new WeakRef(promise);weakReason=new WeakRef(reason);})();
        document.getElementById('a').onclick=event=>{
            event.preventDefault();
            if(stage===0){
                if(weakPromise.deref()===undefined||weakReason.deref()===undefined)throw 'pending roots lost';
                document.title='pending roots retained';
            }else{
                if(weakPromise.deref()!==undefined||weakReason.deref()!==undefined)throw 'notified membership rooted values';
                document.title='notified roots released';
            }
        };
    "#,
    );
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    assert_eq!(report(&init).pending_jobs, 1);
    boa_gc::force_collect();
    let pending = click(&mut realm, &init);
    assert!(pending.errors.is_empty(), "{:?}", pending.errors);
    assert_eq!(title(&pending), "pending roots retained");
    let notified = pump(&mut realm, &pending);
    assert_eq!(notified.errors, [NOTICE]);
    assert_eq!(title(&notified), "delivered");
    assert_eq!(report(&notified).pending_jobs, 0);
    assert!(notified.task_wake.is_none());
    boa_gc::force_collect();
    let released = click(&mut realm, &notified);
    assert!(released.errors.is_empty(), "{:?}", released.errors);
    assert_eq!(title(&released), "notified roots released");
}

#[test]
fn escaped_event_payloads_retain_values_only_until_the_event_becomes_unreachable() {
    let (mut realm, init) = start(
        r#"
        var saved,weakEvent,weakPromise,weakReason,stage=0;
        window.addEventListener('unhandledrejection',event=>{saved=event;weakEvent=new WeakRef(event);});
        (()=>{const reason={proof:42},promise=Promise.reject(reason);weakPromise=new WeakRef(promise);weakReason=new WeakRef(reason);})();
        document.getElementById('a').onclick=event=>{
            event.preventDefault();
            if(stage===0){
                if(saved!==weakEvent.deref()||saved.promise!==weakPromise.deref()||saved.reason!==weakReason.deref())throw 'escaped roots lost';
                saved=null;stage=1;document.title='escaped payload retained';
            }else{
                if(weakEvent.deref()!==undefined||weakPromise.deref()!==undefined||weakReason.deref()!==undefined)throw 'unreachable event retained roots';
                document.title='escaped payload released';
            }
        };
    "#,
    );
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    let notified = pump(&mut realm, &init);
    assert_eq!(notified.errors, [NOTICE]);
    boa_gc::force_collect();
    let escaped = click(&mut realm, &notified);
    assert!(escaped.errors.is_empty(), "{:?}", escaped.errors);
    assert_eq!(title(&escaped), "escaped payload retained");
    boa_gc::force_collect();
    let released = click(&mut realm, &escaped);
    assert!(released.errors.is_empty(), "{:?}", released.errors);
    assert_eq!(title(&released), "escaped payload released");
}

#[test]
fn handling_before_delivery_releases_candidate_roots_after_the_real_checkpoint() {
    let (mut realm, init) = start(
        r#"
        var hold,weakPromise,weakReason,stage=0;
        (()=>{const reason={proof:42};hold=Promise.reject(reason);weakPromise=new WeakRef(hold);weakReason=new WeakRef(reason);})();
        document.getElementById('a').onclick=event=>{
            event.preventDefault();
            if(stage===0){hold.catch(()=>{});hold=null;stage=1;document.title='handled';}
            else{
                if(weakPromise.deref()!==undefined||weakReason.deref()!==undefined)throw 'handled roots retained';
                document.title='handled roots released';
            }
        };
    "#,
    );
    assert!(init.errors.is_empty());
    assert_eq!(report(&init).pending_jobs, 1);
    let handled = click(&mut realm, &init);
    assert!(handled.errors.is_empty(), "{:?}", handled.errors);
    assert_eq!(title(&handled), "handled");
    assert_eq!(report(&handled).jobs_admitted, 2);
    assert_eq!(report(&handled).jobs_executed, 1);
    assert_eq!(report(&handled).pending_jobs, 0);
    assert!(handled.task_wake.is_none());
    boa_gc::force_collect();
    let released = click(&mut realm, &handled);
    assert!(released.errors.is_empty(), "{:?}", released.errors);
    assert_eq!(title(&released), "handled roots released");
}

#[test]
fn invalid_input_releases_frozen_candidates_and_existing_timer_work_together() {
    let (mut realm, init) = start(
        "Promise.reject({proof:1});Promise.reject({proof:2});setTimeout(()=>{throw 'must not run';},100000);",
    );
    assert_eq!(report(&init).jobs_admitted, 3);
    assert_eq!(report(&init).pending_jobs, 3);
    let rejected = realm.dispatch(SessionInput {
        kind: InputKind::Click { target: usize::MAX },
        edits: vec![],
    });
    assert_eq!(rejected.state, RealmState::Closed);
    assert!(rejected.snapshot.is_none());
    assert_eq!(report(&rejected).jobs_admitted, 3);
    assert_eq!(report(&rejected).jobs_executed, 0);
    assert_eq!(report(&rejected).pending_jobs, 0);
    assert!(rejected.task_wake.is_none());
    let replay = realm.pump_tasks(TaskPump {
        wake_token: init.task_wake.unwrap().token,
    });
    assert_eq!(replay.state, RealmState::Closed);
    assert_eq!(report(&replay).pending_jobs, 0);
    assert!(replay.task_wake.is_none());
    drop(realm);
    boa_gc::force_collect();
}

#[test]
fn fatal_notification_discards_the_rest_of_its_batch_and_existing_timer_work() {
    let (mut realm, init) = start(
        r#"
        window.addEventListener('unhandledrejection',()=>{while(true){}});
        window.addEventListener('unhandledrejection',()=>{document.title='must not continue';});
        Promise.reject({proof:1});Promise.reject({proof:2});
        setTimeout(()=>{document.title='must not run';},100000);
    "#,
    );
    assert!(init.errors.is_empty(), "{:?}", init.errors);
    assert_eq!(report(&init).jobs_admitted, 3);
    assert_eq!(report(&init).pending_jobs, 3);
    let fatal = pump(&mut realm, &init);
    assert_eq!(fatal.state, RealmState::Fatal, "{:?}", fatal.errors);
    assert_eq!(report(&fatal).jobs_admitted, 3);
    assert_eq!(report(&fatal).jobs_executed, 1);
    assert_eq!(report(&fatal).pending_jobs, 0);
    assert!(report(&fatal).fatal_reason.is_some());
    assert!(fatal.task_wake.is_none());
    assert!(fatal.navigation.is_none());
    assert_eq!(title(&fatal), "before");
    drop(realm);
    boa_gc::force_collect();
}

#[test]
fn losing_outstanding_handled_observation_is_sticky_without_rewriting_escaped_payloads() {
    // Added after the original 18-test baseline: the initial coherent engine
    // dropped weak membership on a floor change without recording observation
    // loss. Cover both Handle inside that callback and Handle after its purge.
    for immediate in [false, true] {
        let (mut realm, init) = start(&format!(
            r#"
            var promise,reason={{proof:42}},saved,handled=0,stage=0;
            window.onunhandledrejection=event=>{{saved=event;event.preventDefault();document.title='local notice';}};
            window.onrejectionhandled=()=>{{handled++;}};
            document.getElementById('a').onclick=event=>{{
                event.preventDefault();
                if(saved.promise!==promise||saved.reason!==reason||saved.target!==window||
                    saved.currentTarget!==null||saved.eventPhase!==0||!saved.defaultPrevented)
                    throw 'exposed event changed after muting';
                if(stage===0){{
                    eval('1');stage=1;
                    if({immediate})promise.catch(()=>{{}});
                    document.title='floor fell:'+handled;
                }}else if(stage===1){{
                    if(!{immediate})promise.catch(()=>{{}});
                    stage=2;document.title='late catch:'+handled;
                }}else{{document.title='gap persists:'+handled;}}
            }};
            promise=Promise.reject(reason);
        "#
        ));
        assert!(init.errors.is_empty(), "{:?}", init.errors);
        let notified = pump(&mut realm, &init);
        assert_eq!(notified.errors, [NOTICE]);
        assert_eq!(title(&notified), "local notice");
        assert_eq!(report(&notified).jobs_admitted, 1);
        assert_eq!(report(&notified).jobs_executed, 1);
        assert_eq!(report(&notified).pending_jobs, 0);

        let muted = click(&mut realm, &notified);
        gap(&muted);
        assert_eq!(title(&muted), "floor fell:0");
        assert_eq!(report(&muted).jobs_admitted, if immediate { 2 } else { 1 });
        let caught = click(&mut realm, &muted);
        gap(&caught);
        assert_eq!(title(&caught), "late catch:0");
        assert_eq!(report(&caught).jobs_admitted, 2);
        assert_eq!(report(&caught).jobs_executed, 2);
        let later = click(&mut realm, &caught);
        gap(&later);
        assert_eq!(title(&later), "gap persists:0");
        assert_eq!(report(&later).jobs_admitted, 2);
        assert_eq!(notified.errors, [NOTICE], "old evidence remains immutable");
    }
}
