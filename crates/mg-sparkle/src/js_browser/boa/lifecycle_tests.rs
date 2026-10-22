//! Private fault/terminal controls complement the public lifecycle boundary cases.
use super::*;

fn fresh() -> BoaPageRealm {
    let html = "<title>before</title><p>Owned native setup</p>";
    let mut realm = BoaPageRealm::new(
        BrowserHost {
            document: document::parse_with_scripting(html, "https://example.test/owned", true),
            url: url::Url::parse("https://example.test/owned").unwrap(),
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
    realm.set_ready("interactive", &mut vec![]);
    realm
}
fn snapshot(realm: &mut BoaPageRealm) -> SessionReply {
    let mut reply = BoaPageRealm::empty_reply();
    realm.snapshot(&mut reply);
    reply
}
fn next(realm: &mut BoaPageRealm) -> SessionReply {
    let wake = realm.state.borrow().armed_wake.unwrap();
    realm.pump_tasks(TaskPump {
        wake_token: wake.token,
    })
}
fn empty_terminal(realm: &BoaPageRealm, reply: &SessionReply) {
    assert_ne!(reply.state, RealmState::Ready);
    assert_eq!(reply.boa.as_ref().unwrap().pending_jobs, 0);
    assert!(reply.task_wake.is_none());
    let data = realm.state.borrow();
    assert!(data.dom_tasks.is_empty() && data.timers.is_empty() && data.rejections.is_empty());
    assert!(data.outstanding_rejections.is_none() && data.armed_wake.is_none());
}

#[test]
fn storage_failure_after_two_admissions_never_publishes_half_a_lifecycle() {
    let mut realm = fresh();
    realm.state.borrow_mut().host.allocated = super::super::MAX_DOM_BYTES;
    realm.queue_lifecycle(&mut vec![]);
    let reply = snapshot(&mut realm);
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 2);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 0);
    assert_eq!(realm.lifecycle.phase, LifecyclePhase::Interactive);
    empty_terminal(&realm, &reply);
}

#[test]
fn both_checked_handle_overflows_are_terminal_without_partial_queue_publication() {
    for handle in [u64::MAX, u64::MAX - 1] {
        let mut realm = fresh();
        realm.state.borrow_mut().next_handle = handle;
        realm.queue_lifecycle(&mut vec![]);
        let reply = snapshot(&mut realm);
        assert_eq!(reply.state, RealmState::Fatal);
        assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 2);
        assert_eq!(realm.state.borrow().next_handle, handle);
        empty_terminal(&realm, &reply);
    }
}

#[test]
fn second_permit_failure_invalidates_prior_work_and_cancels_the_first_outside_borrows() {
    let mut realm = fresh();
    let permits: Vec<_> = (0..255)
        .map(|_| modern::admit_host_task(realm.engine.context_mut()).unwrap())
        .collect();
    realm.queue_lifecycle(&mut vec![]);
    let reply = snapshot(&mut realm);
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 256);
    empty_terminal(&realm, &reply);
    drop(permits);
    assert_eq!(realm.engine.stats().pending_jobs, 0);
}

#[test]
fn event_record_exhaustion_consumes_only_selected_permit_and_never_advances_progress() {
    let mut realm = fresh();
    realm
        .engine
        .evaluate(
            "document.addEventListener('DOMContentLoaded',()=>document.title='must not run');",
        )
        .unwrap();
    realm.checkpoint(&mut vec![]);
    for _ in 0..MAX_EVENTS {
        realm
            .state
            .borrow_mut()
            .host
            .new_event(Kind::Click, Target::Node(0), None)
            .unwrap();
    }
    realm.queue_lifecycle(&mut vec![]);
    snapshot(&mut realm);
    let reply = next(&mut realm);
    assert_eq!(reply.state, RealmState::Closed);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 1);
    assert_eq!(realm.lifecycle.phase, LifecyclePhase::Interactive);
    assert!(realm.lifecycle.initial_failure);
    empty_terminal(&realm, &reply);
}

#[test]
fn serialization_rejection_cannot_publish_an_already_dispatched_milestone() {
    let mut realm = fresh();
    realm.queue_lifecycle(&mut vec![]);
    snapshot(&mut realm);
    // Private malformed projection, never supplied by page code or the parent.
    realm.state.borrow_mut().host.document.nodes[0]
        .children
        .push(0);
    let reply = next(&mut realm);
    assert_eq!(reply.state, RealmState::Closed);
    assert_eq!(realm.lifecycle.phase, LifecyclePhase::Interactive);
    empty_terminal(&realm, &reply);
}

#[test]
fn duplicate_queue_request_is_an_invariant_failure_not_four_mandatory_tasks() {
    let mut realm = fresh();
    realm.queue_lifecycle(&mut vec![]);
    realm.queue_lifecycle(&mut vec![]);
    let reply = snapshot(&mut realm);
    assert_eq!(reply.state, RealmState::Fatal);
    // The second attempt cannot obtain uncounted or refunded work.
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 4);
    empty_terminal(&realm, &reply);
}

#[test]
fn fatal_before_admission_keeps_existing_first_failure_and_spends_no_lifecycle_credit() {
    let mut realm = fresh();
    realm.engine.latch_failure("owned prior fatal");
    realm.queue_lifecycle(&mut vec![]);
    let reply = snapshot(&mut realm);
    assert_eq!(reply.state, RealmState::Fatal);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 0);
    assert!(
        reply
            .boa
            .as_ref()
            .unwrap()
            .fatal_reason
            .as_ref()
            .unwrap()
            .contains("owned prior fatal")
    );
    empty_terminal(&realm, &reply);
}

#[test]
fn closed_error_reporting_cancels_a_previously_queued_microtask_without_advancing_lifecycle() {
    let mut realm = fresh();
    realm.engine.evaluate("window.addEventListener('error',()=>document.title='wrong reporter');document.addEventListener('DOMContentLoaded',()=>{Promise.resolve().then(()=>document.title='must not run');throw 1;});").unwrap();
    realm.checkpoint(&mut vec![]);
    for _ in 0..MAX_EVENTS - 1 {
        realm
            .state
            .borrow_mut()
            .host
            .new_event(Kind::Click, Target::Node(0), None)
            .unwrap();
    }
    realm.queue_lifecycle(&mut vec![]);
    snapshot(&mut realm);
    let reply = next(&mut realm);
    assert_eq!(reply.state, RealmState::Closed);
    assert_eq!(realm.lifecycle.phase, LifecyclePhase::Interactive);
    assert!(
        reply
            .errors
            .iter()
            .any(|error| error.contains("Event record limit"))
    );
    empty_terminal(&realm, &reply);
    assert!(realm.closed_work_latched);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_admitted, 3);
    assert_eq!(reply.boa.as_ref().unwrap().jobs_executed, 1);
    // The original host closure remains Closed even though its pending VM job
    // was canceled through the existing fatal-latch cleanup mechanism.
    let rejected = realm.pump_tasks(TaskPump { wake_token: 0 });
    assert_eq!(rejected.state, RealmState::Closed);
    assert_eq!(realm.lifecycle.phase, LifecyclePhase::Interactive);
    empty_terminal(&realm, &rejected);
    let html = realm.state.borrow().host.serialize(0, true).unwrap();
    assert!(html.contains("<title>before</title>"));
    assert!(!html.contains("must not run") && !html.contains("wrong reporter"));
}
