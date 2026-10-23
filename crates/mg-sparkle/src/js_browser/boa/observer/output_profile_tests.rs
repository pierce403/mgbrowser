//! Private installer controls, not substitutes for the five frozen full-harness
//! controls. Synthetic functions below exercise only the native trust boundary.
use super::*;
use crate::js_browser::{ClassicSourceProvenance, ExternalScriptSource};
use modern::boa_engine::job::GenericJob;

const OUTPUT: ObserverContract = ObserverContract::PinnedWindowHarnessOutputDisabledV1;
const ORIGINAL: ObserverContract = ObserverContract::PinnedWindowHarnessV1;
const FUNCTIONS: &str = "window.calls=0;window.setupCalls=0;window.late=false;window.add_result_callback=function(cb){calls++;window.result=cb};window.add_completion_callback=function(cb){calls++;window.complete=cb};window.setup=function(options){setupCalls++}";

fn fresh(enabled: bool) -> BoaPageRealm {
    let html = "<title>before</title><p>Owned private installer control</p>";
    let mut realm = BoaPageRealm::new(
        BrowserHost {
            document: document::parse_with_scripting(
                html,
                "https://observer.example.test/private",
                true,
            ),
            url: url::Url::parse("https://observer.example.test/private").unwrap(),
            navigation: None,
            allocated: html.len(),
            collections: vec![],
            listeners: vec![],
            events: vec![],
            ready: "loading",
        },
        true,
    )
    .unwrap();
    realm.set_ready("interactive", &mut Vec::new());
    if enabled {
        enable(
            &mut realm,
            ObserverInit::WindowHarness {
                contract: OUTPUT,
                run_id: 7,
                bootstrap_node: 1,
            },
        );
    }
    assert_eq!(realm.engine.stats().jobs_admitted, 0);
    realm
}

fn evaluate(realm: &mut BoaPageRealm, source: &str) -> JsValue {
    let _ingress = Ingress::enter(realm.state.clone());
    realm.engine.evaluate_raw(source).unwrap()
}

fn fixture(extra: &str) -> BoaPageRealm {
    let mut realm = fresh(true);
    evaluate(&mut realm, FUNCTIONS);
    evaluate(&mut realm, extra);
    realm
}

fn value(realm: &BoaPageRealm, name: &str) -> JsValue {
    own_data(
        &realm.engine.context().global_object(),
        JsString::from(name).into(),
    )
    .unwrap()
}

fn count(realm: &BoaPageRealm, name: &str) -> f64 {
    value(realm, name).as_number().unwrap()
}

fn installed(realm: &BoaPageRealm) -> bool {
    realm.state.borrow().observer.as_ref().unwrap().installed
}

fn fault(realm: &BoaPageRealm) -> Option<ObserverFault> {
    realm.state.borrow().observer.as_ref().unwrap().fault
}

fn snapshot(realm: &mut BoaPageRealm) -> SessionReply {
    let mut reply = BoaPageRealm::empty_reply();
    realm.snapshot(&mut reply);
    reply
}

fn next(realm: &mut BoaPageRealm) -> SessionReply {
    let wake = realm.state.borrow().armed_wake.unwrap();
    realm.state.borrow_mut().test_now = Some(wake.deadline);
    realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    })
}

fn stopped(realm: &BoaPageRealm) {
    assert!(realm.closed);
    assert!(!installed(realm));
    assert_eq!(realm.engine.stats().pending_jobs, 0);
    let data = realm.state.borrow();
    let capture = data.observer.as_ref().unwrap();
    assert!(data.timers.is_empty() && data.dom_tasks.is_empty() && data.rejections.is_empty());
    assert!(data.armed_wake.is_none() && data.outstanding_rejections.is_none());
    assert!(capture.pending.is_empty() && capture.identities.is_empty() && capture.ids.is_none());
    assert_eq!(capture.emitted, 0);
}

fn spend_to(realm: &mut BoaPageRealm, target: usize) {
    while realm.engine.stats().jobs_admitted < target {
        modern::admit_host_task(realm.engine.context_mut())
            .unwrap()
            .cancel();
    }
    assert_eq!(realm.engine.stats().jobs_admitted, target);
}

#[test]
fn original_profile_and_disabled_never_read_setup_or_add_work() {
    for enabled in [false, true] {
        let mut realm = fresh(enabled);
        evaluate(&mut realm, FUNCTIONS);
        evaluate(
            &mut realm,
            "window.hits=0;Object.defineProperty(window,'setup',{get(){hits++;throw 1},configurable:true})",
        );
        assert!(install(&mut realm, &mut Vec::new(), ORIGINAL));
        assert_eq!(count(&realm, "hits"), 0.0);
        assert_eq!(count(&realm, "calls"), if enabled { 2.0 } else { 0.0 });
        assert_eq!(realm.engine.stats().jobs_admitted, 0);
    }
}

#[test]
fn setup_requires_own_data_ordinary_function_before_any_registration() {
    for extra in [
        "window.hits=0;delete window.setup",
        "window.hits=0;delete window.setup;Object.setPrototypeOf(window,{setup:function(){hits++}})",
        "window.hits=0;Object.defineProperty(window,'setup',{get(){hits++;throw 1},configurable:true})",
        "window.hits=0;window.setup=new Proxy(function(){hits++},{get(){hits++},apply(){hits++;throw 1}})",
        "window.hits=0;let r=Proxy.revocable(function(){hits++},{get(){hits++},apply(){hits++}});window.setup=r.proxy;r.revoke()",
        "window.hits=0;window.setup=(function(){hits++}).bind(null)",
        "window.hits=0;window.setup=Object",
        "window.hits=0;window.setup={call(){hits++}}",
    ] {
        let mut realm = fixture(extra);
        assert!(!install(&mut realm, &mut Vec::new(), OUTPUT), "{extra}");
        assert_eq!(fault(&realm), Some(ObserverFault::MalformedCapture));
        assert_eq!(count(&realm, "hits"), 0.0);
        assert_eq!(count(&realm, "calls"), 0.0);
        stopped(&realm);
    }
}

#[test]
fn captured_setup_uses_intrinsic_object_exact_data_flags_and_actual_global_once() {
    let mut realm = fixture(
        r#"
        window.proto=Object.prototype;window.describe=Object.getOwnPropertyDescriptor;
        window.protoOf=Object.getPrototypeOf;window.keys=Object.keys;window.hits=0;
        Object.defineProperty(Object.prototype,'output',{get(){window.hits++;return 9},set(){window.hits++},configurable:true});
        window.setup=function(options){
          window.setupCalls++;window.optionsWeak=new WeakRef(options);
          let d=describe(options,'output');
          window.proof=this===window&&arguments.length===1&&protoOf(options)===proto&&
            keys(options).length===1&&d.value===false&&d.writable&&d.enumerable&&d.configurable;
        };
        window.setupWeak=new WeakRef(setup);
        window.add_result_callback=function(cb){
          window.calls++;window.result=cb;
          Object.defineProperty(window,'setup',{get(){window.hits++;throw 1},configurable:true});
          Object.defineProperty(window,'add_completion_callback',{get(){window.hits++;throw 1},configurable:true});
          window.Object=function(){window.hits++;throw 1};window.globalThis=0;
        };
    "#,
    );
    let mut errors = Vec::new();
    assert!(install(&mut realm, &mut errors, OUTPUT), "{errors:?}");
    assert!(installed(&realm) && errors.is_empty());
    assert_eq!(value(&realm, "proof").as_boolean(), Some(true));
    assert_eq!(count(&realm, "calls"), 2.0);
    assert_eq!(count(&realm, "setupCalls"), 1.0);
    assert_eq!(count(&realm, "hits"), 0.0);
    assert_eq!(realm.engine.stats().jobs_admitted, 0);
    // Normal production checkpoint already cleared kept objects; no test-only
    // ClearKeptObjects repairs options or the captured old setup identity.
    modern::boa_gc::force_collect();
    assert_eq!(
        evaluate(
            &mut realm,
            "window.optionsWeak.deref()===undefined&&window.setupWeak.deref()===undefined"
        )
        .as_boolean(),
        Some(true)
    );
    assert!(!install(&mut realm, &mut Vec::new(), OUTPUT));
    assert_eq!(count(&realm, "setupCalls"), 1.0);
    assert_eq!(count(&realm, "hits"), 0.0);
    assert_eq!(fault(&realm), Some(ObserverFault::InvalidConfiguration));
}

#[test]
fn immediate_and_checkpoint_capture_faults_keep_current_stack_effects_but_stop_install() {
    for contract in [ORIGINAL, OUTPUT] {
        for queued in [false, true] {
            let source = if queued {
                "window.add_completion_callback=function(cb){calls++;Promise.resolve().then(()=>{result({index:0,status:0,name:'early',message:null,stack:null});window.tail=true})}"
            } else {
                "window.add_result_callback=function(cb){calls++;cb({index:0,status:0,name:'early',message:null,stack:null});window.tail=true}"
            };
            let mut realm = fixture(source);
            assert!(!install(&mut realm, &mut Vec::new(), contract));
            assert_eq!(fault(&realm), Some(ObserverFault::InvalidConfiguration));
            assert_eq!(value(&realm, "tail").as_boolean(), Some(true));
            assert_eq!(count(&realm, "calls"), if queued { 2.0 } else { 1.0 });
            assert_eq!(
                count(&realm, "setupCalls"),
                if queued && contract == OUTPUT {
                    1.0
                } else {
                    0.0
                }
            );
            stopped(&realm);
            let reply = snapshot(&mut realm);
            assert!(matches!(
                reply.observer,
                Some(ObserverReply::ObservationFailed {
                    fault: ObserverFault::InvalidConfiguration,
                    prefix_end: 0,
                    ..
                })
            ));
        }
    }
}

#[test]
fn native_reentry_uses_real_ingress_depth_and_cannot_register_or_recover() {
    let mut realm = fixture("");
    let ingress = Ingress::enter(realm.state.clone());
    assert!(!install(&mut realm, &mut Vec::new(), OUTPUT));
    drop(ingress);
    assert_eq!(fault(&realm), Some(ObserverFault::InvariantFailure));
    assert!(realm.engine.is_fatal());
    assert_eq!(count(&realm, "calls"), 0.0);
    assert_eq!(count(&realm, "setupCalls"), 0.0);
    stopped(&realm);
}

#[test]
fn actual_start_with_queued_install_fault_never_executes_next_source() {
    // Clearly synthetic bootstrap, only to test the actual start-loop fence.
    // Full unchanged-harness readiness remains the separate frozen H1-H5 lane.
    let html = "<title>before</title><script src='/owned.js'></script><script>document.title='must not run';</script>";
    let url = "https://observer.example.test/private";
    let document = document::parse_with_scripting(html, url, true);
    let node = document
        .nodes
        .iter()
        .position(|node| node.tag == "script" && node.attr("src") == Some("/owned.js"))
        .unwrap();
    let source = "window.add_result_callback=function(cb){window.result=cb};window.add_completion_callback=function(){Promise.resolve().then(()=>result({index:0,status:0,name:'early',message:null,stack:null}))};window.setup=function(){}";
    let (realm, reply) = BoaPageRealm::start_with_observer(
        Request {
            url: url.into(),
            html: html.into(),
            external_scripts: Some(vec![ExternalScriptSource {
                node,
                source: source.into(),
                provenance: Some(ClassicSourceProvenance {
                    response_url: "https://observer.example.test/owned.js".into(),
                    error_visibility: ClassicErrorVisibility::SameOrigin,
                }),
            }]),
        },
        ObserverInit::WindowHarness {
            contract: OUTPUT,
            run_id: 7,
            bootstrap_node: node as u32,
        },
    );
    let realm = realm.unwrap();
    assert_eq!(reply.state, RealmState::Closed);
    assert_eq!(reply.scripts_executed, 1);
    assert_eq!(
        document::project_nodes(
            realm.state.borrow().host.document.nodes.clone(),
            "https://observer.example.test/private",
        )
        .unwrap()
        .title,
        "before"
    );
    assert!(
        !realm
            .state
            .borrow()
            .host
            .serialize(0, true)
            .unwrap()
            .contains("<title>must not run</title>")
    );
    assert_eq!(fault(&realm), Some(ObserverFault::InvalidConfiguration));
    stopped(&realm);
}

#[test]
fn setup_throw_keeps_bounded_configuration_prefix_and_cancels_pending_microtask() {
    let mut realm = fixture(
        "window.setup=function(){setupCalls++;Promise.resolve().then(()=>window.late=true);throw 17}",
    );
    let mut errors = Vec::new();
    assert!(!install(&mut realm, &mut errors, OUTPUT));
    assert_eq!(
        errors,
        ["Observer configuration: ThrownValue: JavaScript threw a value (not coerced by the host)"]
    );
    assert_eq!(fault(&realm), Some(ObserverFault::InvalidConfiguration));
    assert_eq!(count(&realm, "setupCalls"), 1.0);
    assert_eq!(value(&realm, "late").as_boolean(), Some(false));
    assert_eq!(realm.engine.stats().jobs_admitted, 1);
    assert_eq!(realm.engine.stats().jobs_executed, 0);
    stopped(&realm);
}

#[test]
fn foreign_or_unknown_bootstrap_cannot_upgrade_exposure_with_dom_or_global_metadata() {
    for provenance in [
        None,
        Some(ClassicSourceProvenance {
            response_url: "https://foreign.example.test/redirected.js".into(),
            error_visibility: ClassicErrorVisibility::Muted,
        }),
    ] {
        let url = "https://observer.example.test/private";
        let html = "<base href='https://observer.example.test/'><title>before</title><script src='/owned.js'></script><script>document.title='must not run';</script>";
        let document = document::parse_with_scripting(html, url, true);
        let node = document
            .nodes
            .iter()
            .position(|node| node.tag == "script" && node.attr("src") == Some("/owned.js"))
            .unwrap();
        let source = format!(
            "{FUNCTIONS};window.error_exposure_allowed=true;window.source_provenance={{error_visibility:'same-origin'}};"
        );
        let (realm, reply) = BoaPageRealm::start_with_observer(
            Request {
                url: url.into(),
                html: html.into(),
                external_scripts: Some(vec![ExternalScriptSource {
                    node,
                    source,
                    provenance,
                }]),
            },
            ObserverInit::WindowHarness {
                contract: OUTPUT,
                run_id: 7,
                bootstrap_node: node as u32,
            },
        );
        let realm = realm.unwrap();
        assert_eq!(reply.state, RealmState::Closed);
        assert_eq!(reply.scripts_executed, 1);
        assert_eq!(fault(&realm), Some(ObserverFault::PrivacyLost));
        assert_eq!(count(&realm, "calls"), 0.0);
        assert_eq!(count(&realm, "setupCalls"), 0.0);
        assert!(
            !realm
                .state
                .borrow()
                .host
                .serialize(0, true)
                .unwrap()
                .contains("<title>must not run</title>")
        );
        stopped(&realm);
    }
}

#[test]
fn native_checkpoint_failure_keeps_original_prefix_and_cancels_later_job() {
    let mut realm = fixture("");
    let late = Rc::new(Cell::new(false));
    let observed = late.clone();
    let js_realm = realm.engine.context().realm().clone();
    realm.engine.context_mut().enqueue_job(
        GenericJob::new(|_| Err(JsError::from_opaque(42.into())), js_realm.clone()).into(),
    );
    realm.engine.context_mut().enqueue_job(
        GenericJob::new(
            move |_| {
                observed.set(true);
                Ok(JsValue::undefined())
            },
            js_realm,
        )
        .into(),
    );
    let mut errors = Vec::new();
    assert!(!install(&mut realm, &mut errors, OUTPUT));
    assert_eq!(
        errors,
        ["Promise checkpoint: ThrownValue: JavaScript threw a value (not coerced by the host)"]
    );
    assert_eq!(count(&realm, "setupCalls"), 1.0);
    assert!(!late.get());
    assert_eq!(realm.engine.stats().jobs_admitted, 2);
    assert_eq!(realm.engine.stats().jobs_executed, 1);
    stopped(&realm);
}

#[test]
fn navigation_privacy_and_missing_authority_stop_before_the_next_registration() {
    for (effect, expected) in [
        (
            "window.location.href='/denied'",
            ObserverFault::NavigationProposed,
        ),
        ("eval('1')", ObserverFault::PrivacyLost),
    ] {
        let mut realm = fixture(&format!(
            "window.add_result_callback=function(cb){{calls++;{effect}}}"
        ));
        assert!(!install(&mut realm, &mut Vec::new(), OUTPUT));
        assert_eq!(fault(&realm), Some(expected));
        assert_eq!(count(&realm, "calls"), 1.0);
        assert_eq!(count(&realm, "setupCalls"), 0.0);
        stopped(&realm);
    }
    let mut realm = fixture("");
    let removed = realm.engine.context_mut().remove_data::<Shared>().unwrap();
    assert!(!install(&mut realm, &mut Vec::new(), OUTPUT));
    assert_eq!(fault(&realm), Some(ObserverFault::AuthorityUnavailable));
    assert!(realm.engine.is_fatal());
    assert_eq!(count(&realm, "calls"), 0.0);
    stopped(&realm);
    drop(removed);
}

#[test]
fn genuine_host_capacity_failure_prevents_configuration_without_counter_assignment() {
    let mut realm = fresh(false);
    evaluate(&mut realm, FUNCTIONS);
    let mut retained = Vec::<u8>::new();
    {
        let mut data = realm.state.borrow_mut();
        let remaining = super::super::super::MAX_DOM_BYTES - data.host.allocated;
        reserve_storage(&mut data.host, &mut retained, remaining).unwrap();
        retained.resize(remaining, 0);
    }
    enable(
        &mut realm,
        ObserverInit::WindowHarness {
            contract: OUTPUT,
            run_id: 7,
            bootstrap_node: 1,
        },
    );
    assert!(realm.engine.is_fatal());
    assert!(!install(&mut realm, &mut Vec::new(), OUTPUT));
    assert_eq!(fault(&realm), Some(ObserverFault::StorageAdmission));
    assert_eq!(count(&realm, "setupCalls"), 0.0);
    assert_eq!(realm.engine.stats().jobs_admitted, 0);
    stopped(&realm);
    drop(retained);
}

#[test]
fn lifetime_2045_fits_rearm_and_two_tasks_but_2046_keeps_partial_2048_prefix() {
    for before in [2045, 2046] {
        let mut realm = fixture(
            "window.original=setTimeout(function(){},10000);window.setup=function(options){setupCalls++;clearTimeout(original);window.original=setTimeout(function(){},10000)}",
        );
        spend_to(&mut realm, before);
        assert_eq!(realm.engine.stats().pending_jobs, 1);
        assert!(install(&mut realm, &mut Vec::new(), OUTPUT));
        assert_eq!(realm.engine.stats().jobs_admitted, before + 1);
        assert_eq!(realm.engine.stats().pending_jobs, 1);
        let mut errors = Vec::new();
        realm.queue_lifecycle(&mut errors);
        let reply = snapshot(&mut realm);
        assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 2048);
        assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 0);
        assert_eq!(realm.lifecycle.phase, LifecyclePhase::Interactive);
        if before == 2045 {
            assert_eq!(reply.state, RealmState::Ready);
            assert!(errors.is_empty());
            assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 3);
            assert_eq!(realm.state.borrow().dom_tasks.len(), 2);
        } else {
            assert_eq!(reply.state, RealmState::Fatal);
            assert!(!errors.is_empty());
            assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
            assert!(realm.state.borrow().dom_tasks.is_empty());
        }
    }
}

#[test]
fn full_256_pending_rearm_succeeds_without_credit_but_lifecycle_cannot_fit() {
    let mut realm = fixture(
        "window.original=setTimeout(function(){},10000);for(let i=0;i<255;i++)setTimeout(function(){},60000);window.setup=function(options){setupCalls++;clearTimeout(original);window.original=setTimeout(function(){},10000)}",
    );
    assert_eq!(realm.engine.stats().jobs_admitted, 256);
    assert_eq!(realm.engine.stats().pending_jobs, 256);
    assert!(install(&mut realm, &mut Vec::new(), OUTPUT));
    assert_eq!(realm.engine.stats().jobs_admitted, 257);
    assert_eq!(realm.engine.stats().pending_jobs, 256);
    realm.queue_lifecycle(&mut Vec::new());
    let reply = snapshot(&mut realm);
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 257);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 0);
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
    assert_eq!(realm.lifecycle.phase, LifecyclePhase::Interactive);
    assert!(realm.state.borrow().dom_tasks.is_empty());
}

#[test]
fn promise_rejection_from_setup_is_a_real_later_obligation_not_checkpoint_error() {
    for caught in [false, true] {
        let mut realm = fixture(if caught {
            "window.setup=function(){setupCalls++;Promise.reject('owned').catch(()=>{})}"
        } else {
            "window.setup=function(){setupCalls++;Promise.reject('owned')}"
        });
        let mut errors = Vec::new();
        assert!(install(&mut realm, &mut errors, OUTPUT), "{errors:?}");
        assert!(errors.is_empty() && fault(&realm).is_none() && installed(&realm));
        assert_eq!(count(&realm, "setupCalls"), 1.0);
        if caught {
            assert_eq!(realm.engine.stats().pending_jobs, 0);
            assert!(realm.state.borrow().rejections.is_empty());
            assert!(realm.state.borrow().dom_tasks.is_empty());
        } else {
            assert_eq!(realm.engine.stats().jobs_admitted, 1);
            assert_eq!(realm.engine.stats().pending_jobs, 1);
            let before = snapshot(&mut realm);
            assert!(before.errors.is_empty());
            let delivered = next(&mut realm);
            assert_eq!(delivered.errors, [UNHANDLED_REJECTION]);
            assert_eq!(realm.engine.stats().jobs_executed, 1);
            assert!(
                realm
                    .state
                    .borrow()
                    .observer
                    .as_ref()
                    .unwrap()
                    .execution_failed
            );
            assert!(fault(&realm).is_none());
        }
    }
}

#[test]
fn html_forgery_cannot_enable_setup_and_bad_bootstrap_rejects_before_execution() {
    let html = "<!-- observer=output:false --><script>window.setupCalls=0;window.setup=function(){setupCalls++};window.observer={contract:'PinnedWindowHarnessOutputDisabledV1'};</script>";
    let request = || Request {
        url: "https://observer.example.test/private".into(),
        html: html.into(),
        external_scripts: None,
    };
    let (realm, reply) = BoaPageRealm::start(request());
    assert!(matches!(reply.observer, Some(ObserverReply::Disabled)));
    assert_eq!(count(&realm.unwrap(), "setupCalls"), 0.0);
    let (realm, reply) = BoaPageRealm::start_with_observer(
        request(),
        ObserverInit::WindowHarness {
            contract: OUTPUT,
            run_id: 7,
            bootstrap_node: 1,
        },
    );
    assert!(realm.is_none());
    assert_eq!(reply.scripts_executed, 0);
    assert!(reply.boa.is_none() && reply.snapshot.is_none());
    assert!(matches!(
        reply.observer,
        Some(ObserverReply::ObservationFailed {
            fault: ObserverFault::RejectedInit,
            prefix_end: 0,
            execution_failed: true,
            ..
        })
    ));
}

#[test]
fn fatal_configuration_clears_binding_work_and_reclaims_vm_payload_after_realm_drop() {
    #[derive(Trace, JsData)]
    struct Probe(#[unsafe_ignore_trace] Rc<Cell<usize>>);
    impl Finalize for Probe {
        fn finalize(&self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let finalized = Rc::new(Cell::new(0));
    let mut realm = fixture(
        "window.reporter=0;window.onerror=function(){reporter++};window.setup=function(options){setupCalls++;let p=window.probe;window.probe=null;Promise.resolve().then(()=>p);for(;;){}}",
    );
    let probe = JsObject::from_proto_and_data(None, Probe(finalized.clone()));
    realm
        .engine
        .context()
        .global_object()
        .create_data_property_or_throw(js_string!("probe"), probe, realm.engine.context_mut())
        .unwrap();
    assert!(!install(&mut realm, &mut Vec::new(), OUTPUT));
    assert!(realm.engine.is_fatal());
    assert_eq!(count(&realm, "reporter"), 0.0);
    assert_eq!(count(&realm, "setupCalls"), 1.0);
    stopped(&realm);
    // Pinned Boa may retain fatal VM frame values until Context destruction.
    // Require immediate BINDING cleanup above, and real teardown for VM roots.
    drop(realm);
    modern::boa_gc::force_collect();
    assert_eq!(finalized.get(), 1);

    let mut prior = fixture("");
    prior.engine.latch_failure("owned earlier first fatal");
    assert!(!install(&mut prior, &mut Vec::new(), OUTPUT));
    assert_eq!(count(&prior, "calls"), 0.0);
    assert_eq!(count(&prior, "setupCalls"), 0.0);
    assert_eq!(
        prior.engine.stats().fatal_reason.as_deref(),
        Some("owned earlier first fatal")
    );
    stopped(&prior);
}

#[test]
fn new_profile_prepared_result_cannot_commit_after_terminal_failure() {
    let mut realm = fixture("");
    assert!(install(&mut realm, &mut Vec::new(), OUTPUT));
    evaluate(
        &mut realm,
        "result({index:0,status:0,name:'owned',message:null,stack:null})",
    );
    realm.checkpoint(&mut Vec::new());
    let mut reply = BoaPageRealm::empty_reply();
    reply.state = RealmState::Ready;
    let prepared = prepare_reply(&mut realm, &mut reply);
    realm
        .engine
        .latch_failure("owned failure before observer publication");
    let failed = commit_reply(&mut realm, prepared);
    assert!(matches!(
        failed,
        ObserverReply::ObservationFailed {
            fault: ObserverFault::RealmTerminated,
            prefix_end: 0,
            execution_failed: true,
            ..
        }
    ));
    assert_eq!(realm.engine.stats().pending_jobs, 0);
    let data = realm.state.borrow();
    let capture = data.observer.as_ref().unwrap();
    assert_eq!(capture.emitted, 0);
    assert!(capture.pending.is_empty() && capture.identities.is_empty() && capture.ids.is_none());
    // This tests worker preparation/commit only, not parent projection or the
    // combined active-charge transaction owned by the separate parent tests.
}
