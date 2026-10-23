//! Owned modern-v7 process controls; no display, fixture server or parent JS.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use mg_butane::modern::Report;
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

struct Peer {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    deadline: Instant,
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Peer {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mgbrowser"))
            .arg("--script-session")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        for fd in [input.as_raw_fd(), output.as_raw_fd()] {
            // SAFETY: live pipe descriptors remain owned by this Peer.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            assert!(flags >= 0);
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
                0
            );
        }
        Self {
            child,
            input: Some(input),
            output,
            deadline: Instant::now() + Duration::from_secs(5),
        }
    }
    fn tick(&self) {
        assert!(Instant::now() < self.deadline, "owned task worker deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    fn send(&mut self, sequence: u64, revision: u64, kind: &str, payload: Value) {
        self.send_version(7, sequence, revision, kind, payload);
    }
    fn send_version(
        &mut self,
        version: u8,
        sequence: u64,
        revision: u64,
        kind: &str,
        payload: Value,
    ) {
        let payload = if matches!(version, 6 | 7) && kind == "Init" {
            // Version 6 keeps its genuine old wrapper in the rejection control.
            json!({"page":payload,"observer":{"kind":"Disabled"}})
        } else {
            payload
        };
        self.send_wire(version, sequence, revision, kind, payload);
    }
    fn send_observed_init(&mut self, page: Value, observer: Value) {
        self.send_wire(7, 0, 0, "Init", json!({"page":page,"observer":observer}));
    }
    fn send_wire(&mut self, version: u8, sequence: u64, revision: u64, kind: &str, payload: Value) {
        let payload = serde_json::to_vec(&json!({
            "version":version,"generation":31,"session_id":47,"sequence":sequence,
            "expected_revision":revision,"command":{"kind":kind,"payload":payload}
        }))
        .unwrap();
        assert!(payload.len() < 8192);
        let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
        bytes.extend(payload);
        let mut offset = 0;
        while offset < bytes.len() {
            match self
                .input
                .as_mut()
                .unwrap()
                .write(&bytes[offset..(offset + 3).min(bytes.len())])
            {
                Ok(0) => panic!("task input closed"),
                Ok(n) => offset += n,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.tick()
                }
                Err(error) => panic!("task input: {error}"),
            }
        }
    }
    fn exact(&mut self, count: usize) -> Vec<u8> {
        assert!(count <= 4 * 1024 * 1024);
        let mut bytes = vec![0; count];
        let mut offset = 0;
        while offset < count {
            match self.output.read(&mut bytes[offset..]) {
                Ok(0) => panic!("EOF before task frame"),
                Ok(n) => offset += n,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.tick()
                }
                Err(error) => panic!("task output: {error}"),
            }
        }
        bytes
    }
    fn receive_frame(&mut self, sequence: u64, revision: u64) -> Value {
        let len = u32::from_be_bytes(self.exact(4).try_into().unwrap()) as usize;
        assert!(len > 0);
        let frame: Value = serde_json::from_slice(&self.exact(len)).unwrap();
        for (field, value) in [
            ("version", 7),
            ("generation", 31),
            ("session_id", 47),
            ("sequence", sequence),
            ("expected_revision", revision),
        ] {
            assert_eq!(frame[field], value, "{field}");
        }
        serde_json::from_value::<mg_sparkle::page_session::LifecycleProgress>(
            frame["reply"]["lifecycle"].clone(),
        )
        .expect("required typed modern lifecycle progress");
        frame
    }
    fn receive(&mut self, sequence: u64, revision: u64) -> Value {
        let frame = self.receive_frame(sequence, revision);
        assert!(frame["reply"]["allocations"].is_null());
        let report = report(&frame);
        report.validate().unwrap();
        let memory = report.worker_memory.unwrap();
        assert!(memory.is_valid() && memory.allocations > 0 && memory.outstanding_bytes > 0);
        frame
    }
    fn initialize(&mut self, script: &str) -> Value {
        self.send(0, 0, "Init", json!({"url":"https://example.test/tasks","html":format!("<p id=o></p><script>{script}</script>")}));
        self.receive(0, 0)
    }
    fn pump(&mut self, sequence: u64, revision: u64, token: u64) -> Value {
        self.send(sequence, revision, "TaskPump", json!({"wake_token":token}));
        self.receive(sequence, revision)
    }
    /// Only the explicitly expected remaining mandatory milestones, never an
    /// arbitrary timer/message drain. Call after earlier fixture tasks finish.
    fn complete_lifecycle(&mut self, mut previous: Value) -> Value {
        use mg_sparkle::page_session::{LifecyclePhase, LifecycleProgress, SessionReply};
        let progress: LifecycleProgress =
            serde_json::from_value(previous["reply"]["lifecycle"].clone()).unwrap();
        let phases: &[LifecyclePhase] = match progress.phase {
            LifecyclePhase::Interactive => &[
                LifecyclePhase::DomContentLoaded,
                LifecyclePhase::LoadComplete,
            ],
            LifecyclePhase::DomContentLoaded => &[LifecyclePhase::LoadComplete],
            LifecyclePhase::LoadComplete => &[],
            LifecyclePhase::Initializing => panic!("fixture did not finish source initialization"),
        };
        for phase in phases {
            let sequence = previous["sequence"].as_u64().unwrap() + 1;
            let revision = previous["reply"]["revision"].as_u64().unwrap();
            let (token, delay) = wake(&previous);
            self.wait_due(delay);
            let next = self.pump(sequence, revision, token);
            no_activation(&next);
            assert_eq!(next["reply"]["state"], "Ready");
            let old_reply: SessionReply =
                serde_json::from_value(previous["reply"].clone()).unwrap();
            let next_reply: SessionReply = serde_json::from_value(next["reply"].clone()).unwrap();
            let observed = next_reply
                .validate_lifecycle_after(old_reply.lifecycle, true)
                .unwrap()
                .unwrap();
            assert_eq!(
                observed.phase, *phase,
                "unexpected non-lifecycle work in fixed milestone control"
            );
            report(&next).validate_after(&report(&previous)).unwrap();
            previous = next;
        }
        previous
    }
    fn idle(&mut self) {
        let mut byte = [0];
        match self.output.read(&mut byte) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            other => panic!("idle child emitted unsolicited output: {other:?}"),
        }
    }
    fn wait_due(&self, delay: Duration) {
        assert!(
            delay <= Duration::from_millis(100),
            "small fixture timer delay"
        );
        let due = Instant::now()
            .checked_add(delay + Duration::from_millis(2))
            .unwrap();
        assert!(due < self.deadline);
        while Instant::now() < due {
            self.tick();
        }
    }
    fn finish(&mut self) {
        self.input.take();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "owned task child {status}");
                return;
            }
            self.tick();
        }
    }
}
fn report(frame: &Value) -> Report {
    serde_json::from_value(frame["reply"]["boa"].clone()).unwrap()
}
fn wake(frame: &Value) -> (u64, Duration) {
    let wake = &frame["reply"]["task_wake"];
    let token = wake["token"].as_u64().unwrap();
    assert_ne!(token, 0);
    (
        token,
        Duration::from_millis(wake["delay_ms"].as_u64().unwrap()),
    )
}
fn attribute(frame: &Value, name: &str) -> Option<String> {
    frame["reply"]["snapshot"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|node| {
            let attrs = node["attributes"].as_array()?;
            if !attrs.iter().any(|pair| pair == &json!(["id", "o"])) {
                return None;
            }
            attrs
                .iter()
                .find_map(|pair| (pair[0] == name).then(|| pair[1].as_str().unwrap().to_string()))
        })
}
fn no_activation(frame: &Value) {
    assert_eq!(frame["reply"]["scripts_executed"], 0);
    assert_eq!(frame["reply"]["acknowledgements"], json!([]));
    assert_eq!(frame["reply"]["default_action"], json!({"kind":"None"}));
    assert!(frame["reply"]["outcome"]["click_canceled"].is_null());
    assert!(frame["reply"]["outcome"]["submit_canceled"].is_null());
}

// Additive retained CSSOM process control, not an upstream WPT result.
// Reuses that file's exact modern-v7 Peer, five-second whole harness,
// <8192-byte input-frame guard, attribute/no_activation/report helpers, and
// owned child cleanup. No display, fixture server, new profile, or API.

#[test]
fn real_worker_cssom_retains_declarations_across_lifecycle_and_real_click() {
    let mut peer = Peer::start();
    let initial = peer.initialize(
        r#"
        var ownedNode = document.getElementById('o');
        var ownedStyle = ownedNode.style;
        ownedStyle.cssText = 'width: 12px; height: 1px';
        if (ownedStyle.getPropertyValue('width') !== '12px' ||
            ownedStyle.height !== '1px') throw 'initial CSSOM parse failed';
        ownedNode.setAttribute('data-cssom-phase', 'init');
        ownedNode.setAttribute('data-cssom-value', ownedStyle.width);
        ownedNode.onclick = function(event) {
            event.preventDefault();
            if (ownedStyle.getPropertyValue('width') !== '12px' ||
                ownedNode.style.getPropertyValue('height') !== '1px')
                throw 'retained declarations lost';
            ownedStyle.width = '27px';
            ownedStyle.height = '3px';
            if (ownedStyle.getPropertyValue('width') !== '27px' ||
                ownedNode.style.height !== '3px') throw 'later CSSOM mutation failed';
            ownedNode.setAttribute('data-cssom-phase', 'click');
            ownedNode.setAttribute('data-cssom-value', ownedStyle.getPropertyValue('width'));
        };
        "#,
    );
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["scripts_executed"], 1);
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(
        initial["reply"]["lifecycle"],
        json!({"phase":"Interactive","initial_failure":false})
    );
    assert_eq!(
        attribute(&initial, "data-cssom-phase").as_deref(),
        Some("init")
    );
    assert_eq!(
        attribute(&initial, "data-cssom-value").as_deref(),
        Some("12px")
    );
    let initial_report = report(&initial);
    assert_eq!(
        (
            initial_report.jobs_admitted,
            initial_report.jobs_executed,
            initial_report.pending_jobs
        ),
        (2, 0, 2)
    );
    assert!(initial_report.fatal_reason.is_none());
    peer.idle();

    // This helper pumps only the two distinct required lifecycle milestones;
    // it validates each phase and each cumulative report before returning.
    // The authored source creates no timer, Promise job, or message task.
    let loaded = peer.complete_lifecycle(initial);
    assert_eq!(loaded["sequence"], 2);
    assert_eq!(loaded["reply"]["revision"], 2);
    assert_eq!(loaded["reply"]["errors"], json!([]));
    assert_eq!(
        loaded["reply"]["lifecycle"],
        json!({"phase":"LoadComplete","initial_failure":false})
    );
    no_activation(&loaded);
    assert_eq!(
        attribute(&loaded, "data-cssom-phase").as_deref(),
        Some("init")
    );
    assert_eq!(
        attribute(&loaded, "data-cssom-value").as_deref(),
        Some("12px")
    );
    assert!(loaded["reply"]["task_wake"].is_null());
    let loaded_report = report(&loaded);
    loaded_report.validate_after(&initial_report).unwrap();
    assert_eq!(
        (
            loaded_report.jobs_admitted,
            loaded_report.jobs_executed,
            loaded_report.pending_jobs
        ),
        (2, 2, 0)
    );
    assert_eq!(loaded_report.source_bytes, initial_report.source_bytes);
    assert_eq!(
        loaded_report.source_admissions,
        initial_report.source_admissions
    );
    peer.idle();

    // Select the real current projected node, not an assumed parser ordinal.
    let target = loaded["reply"]["snapshot"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|node| {
            node["attributes"]
                .as_array()
                .is_some_and(|attrs| attrs.iter().any(|pair| pair == &json!(["id", "o"])))
        })
        .unwrap();
    peer.send(
        3,
        2,
        "Event",
        json!({"kind":{"kind":"Click","target":target},"edits":[]}),
    );
    let clicked = peer.receive(3, 2);
    assert_eq!(clicked["reply"]["revision"], 3);
    assert_eq!(clicked["reply"]["state"], "Ready");
    assert_eq!(clicked["reply"]["errors"], json!([]));
    assert_eq!(clicked["reply"]["scripts_executed"], 0);
    assert_eq!(clicked["reply"]["acknowledgements"], json!([]));
    assert_eq!(clicked["reply"]["default_action"], json!({"kind":"None"}));
    assert_eq!(clicked["reply"]["outcome"]["click_canceled"], true);
    assert!(clicked["reply"]["outcome"]["submit_canceled"].is_null());
    assert_eq!(clicked["reply"]["lifecycle"], loaded["reply"]["lifecycle"]);
    assert_eq!(
        attribute(&clicked, "data-cssom-phase").as_deref(),
        Some("click")
    );
    assert_eq!(
        attribute(&clicked, "data-cssom-value").as_deref(),
        Some("27px")
    );
    let current_style = attribute(&clicked, "style").unwrap();
    assert!(current_style.contains("width: 27px;"));
    assert!(current_style.contains("height: 3px;"));
    assert!(clicked["reply"]["task_wake"].is_null());

    let clicked_report = report(&clicked);
    clicked_report.validate_after(&loaded_report).unwrap();
    assert!(clicked_report.fatal_reason.is_none());
    assert_eq!(
        (
            clicked_report.jobs_admitted,
            clicked_report.jobs_executed,
            clicked_report.pending_jobs
        ),
        (2, 2, 0),
        "synchronous CSSOM must not admit or execute a host task"
    );
    assert_eq!(clicked_report.source_bytes, initial_report.source_bytes);
    assert_eq!(
        clicked_report.source_admissions,
        initial_report.source_admissions
    );
    assert!(clicked_report.opcodes_remaining < loaded_report.opcodes_remaining);
    let initial_memory = initial_report.worker_memory.unwrap();
    let loaded_memory = loaded_report.worker_memory.unwrap();
    let clicked_memory = clicked_report.worker_memory.unwrap();
    assert!(loaded_memory.is_valid_after(&initial_memory));
    assert!(clicked_memory.is_valid_after(&loaded_memory));
    assert!(clicked_memory.cumulative_bytes > loaded_memory.cumulative_bytes);
    assert!(clicked_memory.allocations > loaded_memory.allocations);
    for memory in [initial_memory, loaded_memory, clicked_memory] {
        assert_eq!(
            memory.outstanding_limit_bytes,
            mg_butane::modern::WORKER_OUTSTANDING_LIMIT
        );
        assert_eq!(
            memory.cumulative_limit_bytes,
            mg_butane::modern::WORKER_CUMULATIVE_LIMIT
        );
    }
    // Outstanding bytes may decrease when native/GC storage is released;
    // validate_after proves peak/cumulative/allocation counters did not reset.
    // These are requested System bytes, never GC live heap or RSS claims.
    println!(
        "BOA_CSSOM_RETAINED {}",
        serde_json::to_string(&[&initial_report, &loaded_report, &clicked_report]).unwrap()
    );
    peer.idle();
    peer.finish();
}

#[test]
fn real_worker_cssom_methods_retain_identity_priority_and_removal_across_real_click() {
    let mut peer = Peer::start();
    let initial = peer.initialize(
        r#"
        var methodNode=document.getElementById('o'), methodStyle=methodNode.style;
        var methodSet=methodStyle.setProperty, methodRemove=methodStyle.removeProperty;
        var methodPriority=methodStyle.getPropertyPriority;
        if(typeof methodSet!=='function' || typeof methodRemove!=='function' ||
           typeof methodPriority!=='function') throw 'missing CSSOM editing methods';
        methodSet.call(methodStyle,'width','12px','IMPORTANT');
        methodSet.call(methodStyle,'margin','2px 4px','important');
        if(methodStyle.getPropertyValue('width')!=='12px' ||
           methodPriority.call(methodStyle,'width')!=='important' ||
           methodPriority.call(methodStyle,'margin')!=='important') throw 'initial method declarations failed';
        methodNode.setAttribute('data-method-phase','init');
        methodNode.onclick=function(event) {
            event.preventDefault();
            if(methodNode.style!==methodStyle || methodStyle.setProperty!==methodSet ||
               methodStyle.removeProperty!==methodRemove ||
               methodStyle.getPropertyPriority!==methodPriority) throw 'method identity changed';
            if(methodRemove.call(methodStyle,'width')!=='12px' ||
               methodStyle.getPropertyValue('width')!=='') throw 'retained width removal failed';
            methodSet.call(methodStyle,'width','27px','important');
            methodSet.call(methodStyle,'height','3px',undefined);
            if(methodRemove.call(methodStyle,'margin')!=='2px 4px' ||
               methodStyle.getPropertyValue('margin-left')!=='' ||
               methodStyle.getPropertyValue('width')!=='27px' ||
               methodPriority.call(methodStyle,'width')!=='important' ||
               methodPriority.call(methodStyle,'height')!=='') throw 'later declaration methods failed';
            methodNode.setAttribute('data-method-phase','click');
        };
        "#,
    );
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(initial["reply"]["scripts_executed"], 1);
    assert_eq!(initial["reply"]["revision"], 0);
    assert_eq!(
        initial["reply"]["lifecycle"],
        json!({"phase":"Interactive","initial_failure":false})
    );
    // Init really executed its one source; no_activation is a later-task
    // helper which also requires scripts_executed=0 and is not valid here.
    assert_eq!(initial["reply"]["acknowledgements"], json!([]));
    assert_eq!(initial["reply"]["default_action"], json!({"kind":"None"}));
    assert!(initial["reply"]["outcome"]["click_canceled"].is_null());
    assert!(initial["reply"]["outcome"]["submit_canceled"].is_null());
    assert_eq!(
        attribute(&initial, "data-method-phase").as_deref(),
        Some("init")
    );
    let first = report(&initial);
    assert_eq!(
        (first.jobs_admitted, first.jobs_executed, first.pending_jobs),
        (2, 0, 2)
    );
    assert!(first.fatal_reason.is_none());
    peer.idle();

    let loaded = peer.complete_lifecycle(initial);
    assert_eq!(loaded["sequence"], 2);
    assert_eq!(loaded["reply"]["revision"], 2);
    assert_eq!(loaded["reply"]["errors"], json!([]));
    assert_eq!(
        attribute(&loaded, "data-method-phase").as_deref(),
        Some("init")
    );
    assert!(loaded["reply"]["task_wake"].is_null());
    no_activation(&loaded);
    let before_click = report(&loaded);
    before_click.validate_after(&first).unwrap();
    assert_eq!(
        (
            before_click.jobs_admitted,
            before_click.jobs_executed,
            before_click.pending_jobs
        ),
        (2, 2, 0)
    );
    peer.idle();

    let target = loaded["reply"]["snapshot"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|node| {
            node["attributes"]
                .as_array()
                .is_some_and(|attributes| attributes.iter().any(|pair| pair == &json!(["id", "o"])))
        })
        .unwrap();
    peer.send(
        3,
        2,
        "Event",
        json!({"kind":{"kind":"Click","target":target},"edits":[]}),
    );
    let clicked = peer.receive(3, 2);
    assert_eq!(clicked["reply"]["state"], "Ready");
    assert_eq!(clicked["reply"]["revision"], 3);
    assert_eq!(clicked["reply"]["errors"], json!([]));
    assert_eq!(clicked["reply"]["scripts_executed"], 0);
    assert_eq!(clicked["reply"]["acknowledgements"], json!([]));
    assert_eq!(clicked["reply"]["default_action"], json!({"kind":"None"}));
    assert_eq!(clicked["reply"]["outcome"]["click_canceled"], true);
    assert!(clicked["reply"]["outcome"]["submit_canceled"].is_null());
    assert_eq!(clicked["reply"]["lifecycle"], loaded["reply"]["lifecycle"]);
    assert!(clicked["reply"]["task_wake"].is_null());
    assert_eq!(
        attribute(&clicked, "data-method-phase").as_deref(),
        Some("click")
    );
    let style = attribute(&clicked, "style").unwrap();
    assert!(style.contains("width: 27px !important;"), "{style}");
    assert!(style.contains("height: 3px;"), "{style}");
    assert!(!style.contains("margin"), "{style}");
    let after_click = report(&clicked);
    after_click.validate_after(&before_click).unwrap();
    assert!(after_click.fatal_reason.is_none());
    assert_eq!(
        (
            after_click.jobs_admitted,
            after_click.jobs_executed,
            after_click.pending_jobs
        ),
        (2, 2, 0)
    );
    assert_eq!(after_click.source_bytes, first.source_bytes);
    assert_eq!(after_click.source_admissions, first.source_admissions);
    assert!(after_click.opcodes_remaining < before_click.opcodes_remaining);
    let initial_memory = first.worker_memory.unwrap();
    let loaded_memory = before_click.worker_memory.unwrap();
    let clicked_memory = after_click.worker_memory.unwrap();
    assert!(loaded_memory.is_valid_after(&initial_memory));
    assert!(clicked_memory.is_valid_after(&loaded_memory));
    assert!(clicked_memory.cumulative_bytes > loaded_memory.cumulative_bytes);
    for memory in [initial_memory, loaded_memory, clicked_memory] {
        assert_eq!(
            memory.outstanding_limit_bytes,
            mg_butane::modern::WORKER_OUTSTANDING_LIMIT
        );
        assert_eq!(
            memory.cumulative_limit_bytes,
            mg_butane::modern::WORKER_CUMULATIVE_LIMIT
        );
    }
    println!(
        "BOA_CSSOM_METHODS_RETAINED {}",
        serde_json::to_string(&[first, before_click, after_click]).unwrap()
    );
    peer.idle();
    peer.finish();
}

#[test]
fn real_worker_v7_output_configuration_is_init_only_and_keeps_old_contract_off() {
    use mg_sparkle::page_session::observer::{ObserverContract, ObserverInit};
    // Synthetic ordinary functions isolate the public process/configuration
    // seam. This is not a full-harness result or a new startup work golden.
    let bootstrap = r#"
        globalThis.setupCalls = 0;
        globalThis.add_result_callback = function(callback) { globalThis.ownedResult = callback; };
        globalThis.add_completion_callback = function(callback) { globalThis.ownedCompletion = callback; };
        globalThis.setup = function(properties) {
            "use strict";
            var descriptor = Object.getOwnPropertyDescriptor(properties, "output");
            if (this !== globalThis || arguments.length !== 1 ||
                Object.getPrototypeOf(properties) !== Object.prototype ||
                Reflect.ownKeys(properties).length !== 1 || !descriptor ||
                descriptor.value !== false || descriptor.get !== undefined ||
                descriptor.set !== undefined || descriptor.enumerable !== true ||
                descriptor.writable !== true || descriptor.configurable !== true) {
                throw Error("invalid trusted output properties");
            }
            globalThis.setupCalls++;
            document.getElementById("o").setAttribute("data-setup-calls",String(globalThis.setupCalls));
        };
    "#;
    let html = "<script src='/bootstrap.js'></script><p id=o></p><script>document.getElementById('o').setAttribute('data-setup-calls',String(globalThis.setupCalls));</script>";
    let url = "https://example.test/tasks";
    let document = mg_sparkle::document::parse_with_scripting(html, url, true);
    let bootstrap_node = document
        .nodes
        .iter()
        .position(|node| node.tag == "script" && node.attr("src") == Some("/bootstrap.js"))
        .unwrap();
    for (contract, expected_calls) in [
        (ObserverContract::PinnedWindowHarnessV1, "0"),
        (ObserverContract::PinnedWindowHarnessOutputDisabledV1, "1"),
    ] {
        let mut peer = Peer::start();
        let observer = ObserverInit::WindowHarness {
            contract,
            run_id: 81,
            bootstrap_node: u32::try_from(bootstrap_node).unwrap(),
        };
        peer.send_observed_init(
            json!({
                "url":url,"html":html,
                "external_scripts":[{
                    "node":bootstrap_node,"source":bootstrap,
                    "provenance":{
                        "response_url":"https://example.test/bootstrap.js",
                        "error_visibility":"same-origin"
                    }
                }]
            }),
            serde_json::to_value(observer).unwrap(),
        );
        let initial = peer.receive(0, 0);
        assert_eq!(initial["reply"]["state"], "Ready");
        assert_eq!(initial["reply"]["errors"], json!([]));
        assert_eq!(initial["reply"]["scripts_executed"], 2);
        assert_eq!(
            attribute(&initial, "data-setup-calls").as_deref(),
            Some(expected_calls)
        );
        assert_eq!(
            initial["reply"]["observer"],
            json!({
                "kind":"WindowHarness","run_id":81,"first_ordinal":0,"next_ordinal":0,
                "records":[],"progress":"Waiting","execution_failed":false
            })
        );
        assert_eq!(
            (
                report(&initial).source_admissions,
                report(&initial).jobs_admitted,
                report(&initial).jobs_executed,
                report(&initial).pending_jobs
            ),
            (2, 2, 0, 2),
            "synchronous configuration is not a source or a separate task allowance"
        );
        let loaded = peer.complete_lifecycle(initial);
        assert_eq!(loaded["reply"]["errors"], json!([]));
        assert_eq!(
            attribute(&loaded, "data-setup-calls").as_deref(),
            Some(expected_calls)
        );
        assert_eq!(loaded["reply"]["observer"]["progress"], "Waiting");
        assert_eq!(loaded["reply"]["observer"]["execution_failed"], false);
        assert_eq!(report(&loaded).pending_jobs, 0);
        assert_eq!(report(&loaded).jobs_admitted, 2);
        assert_eq!(report(&loaded).jobs_executed, 2);
        peer.finish();
    }
}

#[test]
fn real_worker_empty_initialization_and_lifecycle_are_three_distinct_replies() {
    let mut peer = Peer::start();
    let initial = peer.initialize("");
    assert_eq!(
        initial["reply"]["lifecycle"],
        json!({"phase":"Interactive","initial_failure":false})
    );
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(
        (
            report(&initial).jobs_admitted,
            report(&initial).jobs_executed,
            report(&initial).pending_jobs
        ),
        (2, 0, 2)
    );
    peer.idle();
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let dcl = peer.pump(1, 0, token);
    no_activation(&dcl);
    assert_eq!(
        dcl["reply"]["lifecycle"],
        json!({"phase":"DomContentLoaded","initial_failure":false})
    );
    assert_eq!(dcl["reply"]["errors"], json!([]));
    assert_eq!(
        (
            report(&dcl).jobs_admitted,
            report(&dcl).jobs_executed,
            report(&dcl).pending_jobs
        ),
        (2, 1, 1)
    );
    assert!(wake(&dcl).0 > token);
    report(&dcl).validate_after(&report(&initial)).unwrap();
    peer.idle();
    let loaded = peer.complete_lifecycle(dcl);
    assert_eq!(
        loaded["reply"]["lifecycle"],
        json!({"phase":"LoadComplete","initial_failure":false})
    );
    assert_eq!(
        (
            report(&loaded).jobs_admitted,
            report(&loaded).jobs_executed,
            report(&loaded).pending_jobs
        ),
        (2, 2, 0)
    );
    assert_eq!(loaded["reply"]["errors"], json!([]));
    assert!(loaded["reply"]["task_wake"].is_null());
    peer.finish();
}

#[test]
fn real_worker_initial_errors_stay_latched_through_clean_milestones_and_later_work() {
    for stage in ["inline", "dcl", "load"] {
        let mut peer = Peer::start();
        let source = format!(
            r#"
            function fail(){{throw 'owned-{stage}-failure';}}
            setTimeout(()=>document.getElementById('o').setAttribute('data-later','yes'),60);
            {}
        "#,
            match stage {
                "inline" => "fail();",
                "dcl" => "document.addEventListener('DOMContentLoaded',fail);",
                "load" => "window.onload=fail;",
                _ => unreachable!(),
            }
        );
        let initial = peer.initialize(&source);
        assert_eq!(
            initial["reply"]["lifecycle"]["initial_failure"],
            stage == "inline"
        );
        let mut loaded = initial;
        for (sequence, phase) in [(1, "DomContentLoaded"), (2, "LoadComplete")] {
            let (token, delay) = wake(&loaded);
            peer.wait_due(delay);
            let next = peer.pump(sequence, sequence - 1, token);
            no_activation(&next);
            assert_eq!(next["reply"]["lifecycle"]["phase"], phase);
            let expected_error =
                (stage == "dcl" && sequence == 1) || (stage == "load" && sequence == 2);
            assert_eq!(
                next["reply"]["errors"].as_array().unwrap().len(),
                usize::from(expected_error),
                "{stage} {phase}"
            );
            if expected_error {
                assert!(
                    next["reply"]["errors"][0]
                        .as_str()
                        .unwrap()
                        .contains("JavaScript threw a value (not coerced by the host)")
                );
            }
            report(&next).validate_after(&report(&loaded)).unwrap();
            loaded = next;
        }
        assert_eq!(
            loaded["reply"]["lifecycle"],
            json!({"phase":"LoadComplete","initial_failure":true}),
            "{stage}"
        );
        assert_eq!(report(&loaded).pending_jobs, 1);
        let (token, delay) = wake(&loaded);
        peer.wait_due(delay);
        let later = peer.pump(3, 2, token);
        assert_eq!(later["reply"]["errors"], json!([]), "{stage}");
        assert_eq!(
            later["reply"]["lifecycle"], loaded["reply"]["lifecycle"],
            "{stage}"
        );
        assert_eq!(attribute(&later, "data-later").as_deref(), Some("yes"));
        assert_eq!(report(&later).pending_jobs, 0);
        report(&later).validate_after(&report(&loaded)).unwrap();
        peer.finish();
    }
}

#[test]
fn real_worker_window_error_identity_survives_genuine_queued_load() {
    let mut peer = Peer::start();
    // Retained counterpart of the original one-shot Window-error control:
    // property/listener identity and no coercion are preserved across load.
    let initial = peer.initialize("var traps=0,secret={},saved;for(const key of ['message','name','stack'])Object.defineProperty(secret,key,{get(){traps++;throw 'trap';}});secret[Symbol.toPrimitive]=()=>{traps++;throw 'trap';};window.onerror=function(message,file,line,column,error){if(arguments.length!==5||this!==window||error!==secret||file!==''||line!==0||column!==0||traps)throw 'bad property handler';return true;};window.addEventListener('error',function(event){if(event.error!==secret||event.target!==window||event.currentTarget!==window||event.eventPhase!==2||event.bubbles||!event.cancelable||!event.defaultPrevented||traps)throw 'bad native event';saved=event;});window.onload=()=>{if(saved.currentTarget!==null||saved.eventPhase!==0||saved.error!==secret||traps)throw 'bad escaped event';document.getElementById('o').setAttribute('data-proof','owned error identity');};throw secret;");
    assert_eq!(initial["reply"]["errors"].as_array().unwrap().len(), 1);
    assert!(
        initial["reply"]["errors"][0]
            .as_str()
            .unwrap()
            .contains("JavaScript threw a value (not coerced by the host)")
    );
    assert!(attribute(&initial, "data-proof").is_none());
    assert_eq!(report(&initial).pending_jobs, 2);
    let loaded = peer.complete_lifecycle(initial);
    assert_eq!(loaded["reply"]["errors"], json!([]));
    assert_eq!(
        attribute(&loaded, "data-proof").as_deref(),
        Some("owned error identity")
    );
    assert_eq!(
        loaded["reply"]["lifecycle"],
        json!({"phase":"LoadComplete","initial_failure":true})
    );
    assert_eq!(report(&loaded).pending_jobs, 0);
    peer.finish();
}

#[test]
fn real_worker_early_navigation_and_prerealm_rejection_never_manufacture_load() {
    let mut peer = Peer::start();
    let initial = peer.initialize("location.href='/next';");
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["navigation"], "https://example.test/next");
    assert_eq!(
        initial["reply"]["lifecycle"],
        json!({"phase":"Initializing","initial_failure":false})
    );
    assert_eq!(
        (
            report(&initial).jobs_admitted,
            report(&initial).pending_jobs
        ),
        (0, 0)
    );
    assert!(initial["reply"]["task_wake"].is_null());
    peer.finish();

    let mut peer = Peer::start();
    let html = "<p id=o>fallback</p><script>throw 'MUST_NOT_RUN';</script><script id=external src=/owned.js></script>";
    let document =
        mg_sparkle::document::parse_with_scripting(html, "https://example.test/tasks", true);
    peer.send(0, 0, "Init", json!({"url":"https://example.test/tasks","html":html,"external_scripts":[{
        "node":document.query_selector(0,"#external").unwrap().unwrap(),"source":"throw 'MUST_NOT_RUN'","provenance":{
            "response_url":"https://foreign.test/owned.js","error_visibility":"same-origin"
        }
    }]}));
    let rejected = peer.receive_frame(0, 0);
    let reply: mg_sparkle::page_session::SessionReply =
        serde_json::from_value(rejected["reply"].clone()).unwrap();
    assert!(reply.is_rejected_initial());
    assert_eq!(
        rejected["reply"]["lifecycle"],
        json!({"phase":"Initializing","initial_failure":true})
    );
    assert!(rejected["reply"]["boa"].is_null());
    assert_eq!(rejected["reply"]["errors"].as_array().unwrap().len(), 1);
    assert!(
        rejected["reply"]["errors"][0]
            .as_str()
            .unwrap()
            .contains("contradictory classic source provenance")
    );
    assert!(peer.input.is_some());
    loop {
        if let Some(status) = peer.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        peer.tick();
    }
    let mut byte = [0];
    assert_eq!(peer.output.read(&mut byte).unwrap(), 0);
}

#[test]
fn real_worker_delivers_exactly_one_cloned_message_per_parent_pump() {
    let mut peer = Peer::start();
    let initial = peer.initialize(r#"
        var order=['init'],original={value:42},saved;
        original.self=original;
        function show(){document.getElementById('o').setAttribute('data-order',order.join(','));}
        onmessage=function(event){
            if(event.origin!=='https://example.test'||event.source!==window||event.cancelable||event.bubbles||
                event.data===original||event.data.self!==event.data||event.data.value!==42)
                throw 'invalid native cloned message';
            saved=event;order.push('message');show();
            Promise.resolve().then(()=>{order.push('checkpoint');show();});return false;
        };
        addEventListener('message',event=>{
            if(event!==saved||event.defaultPrevented||order[order.length-1]!=='checkpoint')throw 'missing message checkpoint';
            order.push('listener');show();
        });
        postMessage(original,'/');postMessage(original,'*');original.value=99;
        Promise.resolve().then(()=>{order.push('init-job');show();});
    "#);
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(
        attribute(&initial, "data-order").as_deref(),
        Some("init,init-job")
    );
    assert_eq!(
        (
            report(&initial).jobs_admitted,
            report(&initial).jobs_executed,
            report(&initial).pending_jobs
        ),
        (5, 1, 4) // Two messages, one reaction and two queued lifecycle permits.
    );
    let mut previous = initial;
    for sequence in 1..=2 {
        let (token, delay) = wake(&previous);
        peer.wait_due(delay);
        peer.idle();
        let current = peer.pump(sequence, sequence - 1, token);
        no_activation(&current);
        assert_eq!(current["reply"]["state"], "Ready");
        assert_eq!(current["reply"]["errors"], json!([]));
        let suffix = ",message,checkpoint,listener".repeat(sequence as usize);
        assert_eq!(
            attribute(&current, "data-order"),
            Some(format!("init,init-job{suffix}"))
        );
        assert_eq!(
            (
                report(&current).jobs_admitted,
                report(&current).jobs_executed,
                report(&current).pending_jobs
            ),
            (
                5 + sequence as usize,
                1 + 2 * sequence as usize,
                4 - sequence as usize
            )
        );
        report(&current).validate_after(&report(&previous)).unwrap();
        previous = current;
    }
    let completed = peer.complete_lifecycle(previous);
    assert_eq!(report(&completed).jobs_admitted, 7);
    assert_eq!(report(&completed).jobs_executed, 7);
    assert_eq!(report(&completed).pending_jobs, 0);
    assert_eq!(completed["reply"]["errors"], json!([]));
    assert!(completed["reply"]["task_wake"].is_null());
    peer.idle();
    peer.finish();
}

#[test]
fn real_worker_message_pump_replay_is_terminal_without_running_the_next_message() {
    let mut peer = Peer::start();
    let initial = peer.initialize("onmessage=e=>document.getElementById('o').setAttribute('data-order',e.data);postMessage('first','/');postMessage('second','/');");
    assert_eq!(initial["reply"]["state"], "Ready");
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let first = peer.pump(1, 0, token);
    assert_eq!(first["reply"]["state"], "Ready");
    assert_eq!(first["reply"]["errors"], json!([]));
    assert_eq!(attribute(&first, "data-order").as_deref(), Some("first"));
    assert_eq!(report(&first).pending_jobs, 3); // Second message plus both lifecycle tasks.
    let rejected = peer.pump(2, 1, token);
    no_activation(&rejected);
    assert_eq!(rejected["reply"]["state"], "Closed");
    assert_eq!(report(&rejected).jobs_admitted, 4);
    assert_eq!(report(&rejected).jobs_executed, 1);
    assert_eq!(report(&rejected).pending_jobs, 0);
    assert_eq!(
        rejected["reply"]["lifecycle"],
        json!({"phase":"Interactive","initial_failure":true})
    );
    assert!(rejected["reply"]["task_wake"].is_null());
    assert!(
        rejected["reply"]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error.as_str().unwrap().contains("replayed task wake"))
    );
    report(&rejected).validate_after(&report(&first)).unwrap();
    peer.finish();
}

#[test]
fn real_worker_message_queue_cannot_bypass_shared_pending_admission() {
    let mut peer = Peer::start();
    let initial = peer.initialize("onerror=()=>document.getElementById('o').setAttribute('data-reporter','wrong');for(let i=0;i<257;i++)postMessage({proof:i},'/');document.getElementById('o').setAttribute('data-after','wrong');");
    assert_eq!(initial["reply"]["state"], "Fatal");
    assert_eq!(report(&initial).jobs_admitted, 256);
    assert_eq!(report(&initial).jobs_executed, 0);
    assert_eq!(report(&initial).pending_jobs, 0);
    assert!(report(&initial).fatal_reason.is_some());
    assert!(attribute(&initial, "data-reporter").is_none());
    assert!(attribute(&initial, "data-after").is_none());
    assert!(initial["reply"]["task_wake"].is_null());
    assert!(
        initial["reply"]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|error| error.as_str().unwrap().contains("EngineLimit:"))
    );
    peer.finish();
}

#[test]
fn real_worker_message_pumps_share_the_unchanged_sixty_four_transaction_budget() {
    let mut peer = Peer::start();
    let mut reply = peer.initialize("var count=0;onmessage=()=>{count++;document.getElementById('o').setAttribute('data-count',''+count);postMessage(count,'/');};postMessage(0,'/');");
    assert_eq!(reply["reply"]["state"], "Ready");
    assert_eq!(reply["reply"]["errors"], json!([]));
    assert_eq!(
        (
            report(&reply).jobs_admitted,
            report(&reply).jobs_executed,
            report(&reply).pending_jobs
        ),
        (3, 0, 3) // Recursive message plus DCL/load; all share the same budget.
    );
    let initial = report(&reply);
    for sequence in 1..64 {
        let (token, delay) = wake(&reply);
        peer.wait_due(delay);
        let next = peer.pump(sequence, sequence - 1, token);
        no_activation(&next);
        assert_eq!(next["reply"]["state"], "Ready");
        assert_eq!(next["reply"]["errors"], json!([]));
        assert_eq!(next["reply"]["revision"], sequence);
        let messages = sequence.saturating_sub(2).max(1);
        assert_eq!(attribute(&next, "data-count"), Some(messages.to_string()));
        assert_eq!(
            (
                report(&next).jobs_admitted,
                report(&next).jobs_executed,
                report(&next).pending_jobs
            ),
            (
                messages as usize + 3,
                sequence as usize,
                if sequence < 3 {
                    4 - sequence as usize
                } else {
                    1
                }
            )
        );
        report(&next).validate_after(&report(&reply)).unwrap();
        reply = next;
    }
    assert!(report(&reply).opcodes_remaining < initial.opcodes_remaining);
    assert_eq!(attribute(&reply, "data-count").as_deref(), Some("61"));
    assert_eq!(reply["reply"]["lifecycle"]["phase"], "LoadComplete");
    // Init + 63 pumps (two lifecycle, 61 messages) ends the worker even though a real message remains queued.
    // Do not close parent stdin or renew the realm to manufacture termination.
    assert!(peer.input.is_some());
    loop {
        if let Some(status) = peer.child.try_wait().unwrap() {
            assert!(status.success(), "message transaction-bound child {status}");
            break;
        }
        peer.tick();
    }
}

#[test]
fn real_worker_fatal_message_delivery_exits_without_next_callbacks_or_closing_parent_input() {
    let mut peer = Peer::start();
    let initial = peer.initialize("onerror=()=>document.getElementById('o').setAttribute('data-reporter','wrong');onmessage=()=>{document.getElementById('o').setAttribute('data-entered','yes');Promise.resolve().then(()=>document.getElementById('o').setAttribute('data-job','wrong'));while(true){}};addEventListener('message',()=>document.getElementById('o').setAttribute('data-listener','wrong'));postMessage(1,'/');postMessage(2,'/');");
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(report(&initial).pending_jobs, 4); // Messages and mandatory lifecycle.
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let fatal = peer.pump(1, 0, token);
    no_activation(&fatal);
    assert_eq!(fatal["reply"]["state"], "Fatal");
    assert_eq!(report(&fatal).jobs_admitted, 5); // Two messages, two lifecycle, one stopped reaction.
    assert_eq!(report(&fatal).jobs_executed, 1);
    assert_eq!(report(&fatal).pending_jobs, 0);
    assert!(report(&fatal).fatal_reason.is_some());
    assert!(fatal["reply"]["task_wake"].is_null());
    assert_eq!(attribute(&fatal, "data-entered").as_deref(), Some("yes"));
    for field in ["data-reporter", "data-job", "data-listener"] {
        assert!(attribute(&fatal, field).is_none(), "{field}");
    }
    report(&fatal).validate_after(&report(&initial)).unwrap();
    assert!(peer.input.is_some());
    loop {
        if let Some(status) = peer.child.try_wait().unwrap() {
            assert!(status.success(), "terminal message child {status}");
            break;
        }
        peer.tick();
    }
    let mut byte = [0];
    assert_eq!(peer.output.read(&mut byte).unwrap(), 0);
}

#[test]
fn real_worker_only_runs_one_due_timer_and_its_checkpoint_per_parent_pump() {
    let mut peer = Peer::start();
    let initial = peer.initialize("var order=['init'];function show(){document.getElementById('o').setAttribute('data-order',order.join(','));}Promise.resolve().then(function(){order.push('micro');show();});var gone=setTimeout(function(){order.push('canceled');show();},0);clearTimeout(gone);setTimeout(function(){order.push('first');show();Promise.resolve().then(function(){order.push('checkpoint');show();});},0);setTimeout(function(){order.push('second');show();},0);");
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(
        attribute(&initial, "data-order").as_deref(),
        Some("init,micro")
    );
    assert_eq!(report(&initial).pending_jobs, 4); // Two timers and both mandatory milestones.
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    peer.idle();
    let first = peer.pump(1, 0, token);
    no_activation(&first);
    assert_eq!(
        attribute(&first, "data-order").as_deref(),
        Some("init,micro,first,checkpoint")
    );
    assert_eq!(report(&first).pending_jobs, 3);
    report(&first).validate_after(&report(&initial)).unwrap();
    let (next, delay) = wake(&first);
    assert!(next > token);
    peer.wait_due(delay);
    let second = peer.pump(2, 1, next);
    no_activation(&second);
    assert_eq!(
        attribute(&second, "data-order").as_deref(),
        Some("init,micro,first,checkpoint,second")
    );
    assert_eq!(report(&second).pending_jobs, 2);
    report(&second).validate_after(&report(&first)).unwrap();
    let completed = peer.complete_lifecycle(second);
    assert_eq!(report(&completed).pending_jobs, 0);
    assert!(completed["reply"]["task_wake"].is_null());
    peer.finish();
}

#[test]
fn real_worker_rejects_early_foreign_and_replayed_tokens_without_effects() {
    for invalid in ["early", "foreign", "replayed"] {
        let mut peer = Peer::start();
        let initial = peer.initialize(if invalid == "early" {
            "setTimeout(function(){document.getElementById('o').setAttribute('data-ran','yes');},10000);"
        } else {
            "setTimeout(function(){document.getElementById('o').setAttribute('data-ran','first');},0);setTimeout(function(){document.getElementById('o').setAttribute('data-ran','second');},0);"
        });
        // Complete only the two mandatory milestones before testing the long
        // timer's genuine not-yet-due token. Immediate timer controls retain
        // their original position ahead of lifecycle tasks.
        let initial = if invalid == "early" {
            peer.complete_lifecycle(initial)
        } else {
            initial
        };
        let sequence = initial["sequence"].as_u64().unwrap() + 1;
        let revision = initial["reply"]["revision"].as_u64().unwrap();
        let (token, delay) = wake(&initial);
        if invalid == "early" {
            assert!(delay > Duration::from_secs(1));
        }
        let (sequence, revision, bad_token) = if invalid == "replayed" {
            peer.wait_due(delay);
            let first = peer.pump(sequence, revision, token);
            assert_eq!(attribute(&first, "data-ran").as_deref(), Some("first"));
            assert!(wake(&first).0 > token);
            (sequence + 1, revision + 1, token)
        } else {
            (
                sequence,
                revision,
                if invalid == "foreign" {
                    token + 1
                } else {
                    token
                },
            )
        };
        let rejected = peer.pump(sequence, revision, bad_token);
        no_activation(&rejected);
        assert_eq!(rejected["reply"]["state"], "Closed", "{invalid}");
        assert_eq!(rejected["reply"]["revision"], revision, "{invalid}");
        assert!(rejected["reply"]["snapshot"].is_null(), "{invalid}");
        assert!(rejected["reply"]["task_wake"].is_null(), "{invalid}");
        assert!(rejected["reply"]["navigation"].is_null(), "{invalid}");
        assert_eq!(report(&rejected).pending_jobs, 0, "{invalid}");
        peer.finish();
    }
}

#[test]
fn real_worker_timer_pumps_share_the_fixed_64_transaction_budget() {
    let mut peer = Peer::start();
    let mut reply = peer.initialize("var n=0;function again(){document.getElementById('o').setAttribute('data-count',String(++n));setTimeout(again,0);}setTimeout(again,0);");
    let first = report(&reply);
    for sequence in 1..64 {
        let (token, delay) = wake(&reply);
        peer.wait_due(delay);
        let next = peer.pump(sequence, sequence - 1, token);
        no_activation(&next);
        assert_eq!(next["reply"]["revision"], sequence);
        // The first timer precedes DCL/load; its successors follow the two
        // already queued lifecycle tasks and share their transaction budget.
        let timers = sequence.saturating_sub(2).max(1);
        assert_eq!(attribute(&next, "data-count"), Some(timers.to_string()));
        report(&next).validate_after(&report(&reply)).unwrap();
        reply = next;
    }
    assert!(report(&reply).opcodes_remaining < first.opcodes_remaining);
    assert_eq!(report(&reply).jobs_executed, 63);
    assert_eq!(attribute(&reply, "data-count").as_deref(), Some("61"));
    assert_eq!(reply["reply"]["lifecycle"]["phase"], "LoadComplete");
    // The child closes after the original Init + 63-command limit, even with
    // a retained recursive timeout. It cannot accept a 65th frame or renew the realm.
    loop {
        if let Some(status) = peer.child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        peer.tick();
    }
}

#[test]
fn real_worker_timer_error_retains_native_event_across_following_input_transaction() {
    let mut peer = Peer::start();
    let initial = peer.initialize("var value={},saved,order=[];window.onerror=function(message,file,line,column,error){if(arguments.length!==5||this!==window||error!==value||file!==''||line!==0||column!==0)throw 'bad onerror';order.push('property');Promise.resolve().then(()=>order.push('property-job'));return true;};window.addEventListener('error',event=>{if(event.error!==value||event.target!==window||event.currentTarget!==window||event.eventPhase!==2||!event.defaultPrevented||event.bubbles||!event.cancelable)throw 'bad event';saved=event;order.push('listener');Promise.resolve().then(()=>{order.push('listener-job');document.getElementById('o').setAttribute('data-order',order.join(','));});});document.getElementById('o').onclick=event=>{if(saved.error!==value||saved.currentTarget!==null||saved.eventPhase!==0||!saved.defaultPrevented)throw 'lost event';event.preventDefault();document.getElementById('o').setAttribute('data-retained','yes');};setTimeout(()=>{throw value;},60);");
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(report(&initial).pending_jobs, 3);
    let loaded = peer.complete_lifecycle(initial.clone());
    assert_eq!(report(&loaded).pending_jobs, 1);
    let (token, delay) = wake(&loaded);
    peer.wait_due(delay);
    let fired = peer.pump(3, 2, token);
    no_activation(&fired);
    assert_eq!(fired["reply"]["state"], "Ready");
    assert_eq!(fired["reply"]["errors"].as_array().unwrap().len(), 1);
    assert!(
        fired["reply"]["errors"][0]
            .as_str()
            .unwrap()
            .starts_with("Timer callback:")
    );
    assert_eq!(
        attribute(&fired, "data-order").as_deref(),
        Some("property,property-job,listener,listener-job")
    );
    assert_eq!(report(&fired).pending_jobs, 0);
    assert!(fired["reply"]["task_wake"].is_null());
    report(&fired).validate_after(&report(&loaded)).unwrap();
    assert_eq!(fired["reply"]["lifecycle"]["initial_failure"], false);
    let target = fired["reply"]["snapshot"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|node| {
            node["attributes"]
                .as_array()
                .is_some_and(|attrs| attrs.iter().any(|pair| pair == &json!(["id", "o"])))
        })
        .unwrap();
    peer.send(
        4,
        3,
        "Event",
        json!({"kind":{"kind":"Click","target":target},"edits":[]}),
    );
    let clicked = peer.receive(4, 3);
    assert_eq!(clicked["reply"]["errors"], json!([]));
    assert_eq!(clicked["reply"]["state"], "Ready");
    assert_eq!(clicked["reply"]["outcome"]["click_canceled"], true);
    assert_eq!(attribute(&clicked, "data-retained").as_deref(), Some("yes"));
    assert_eq!(report(&clicked).pending_jobs, 0);
    report(&clicked).validate_after(&report(&fired)).unwrap();
    peer.finish();
}

#[test]
fn real_worker_rejects_v1_through_v6_init_and_later_commands_without_reply_or_execution() {
    for (version, kind) in
        (1..=6).flat_map(|version| ["Init", "Event", "TaskPump"].map(|kind| (version, kind)))
    {
        let mut peer = Peer::start();
        let (sequence, payload) = match kind {
            "Init" => (
                0,
                json!({"url":"https://example.test/tasks","html":"<script>throw 'MUST_NOT_RUN'</script>"}),
            ),
            later => {
                let initial = peer.initialize("setTimeout(()=>{throw 'MUST_NOT_RUN';},100000);");
                assert_eq!(initial["reply"]["state"], "Ready");
                assert_eq!(report(&initial).pending_jobs, 3); // Timer plus mandatory lifecycle.
                (
                    1,
                    if later == "Event" {
                        json!({"kind":{"kind":"Click","target":0},"edits":[]})
                    } else {
                        json!({"wake_token":wake(&initial).0})
                    },
                )
            }
        };
        peer.send_version(version, sequence, 0, kind, payload);
        loop {
            if let Some(status) = peer.child.try_wait().unwrap() {
                assert!(
                    !status.success(),
                    "old modern version must fail closed: {version} {kind}"
                );
                break;
            }
            peer.tick();
        }
        let mut byte = [0];
        assert_eq!(
            peer.output.read(&mut byte).unwrap(),
            0,
            "no reply for {kind}"
        );
    }
}

#[test]
fn real_worker_runs_one_frozen_rejection_batch_and_its_listener_checkpoints() {
    let mut peer = Peer::start();
    let initial = peer.initialize(
        r#"
        var order=['script'],a={proof:'a'},b={proof:'b'},pa,pb;
        function show(){document.getElementById('o').setAttribute('data-order',order.join(','));}
        window.addEventListener('error',()=>document.getElementById('o').setAttribute('data-window-error','wrong'));
        window.addEventListener('unhandledrejection',function(event){
            const id=event.promise===pa?'a':event.promise===pb?'b':'wrong';
            if(id==='wrong'||event.reason!==(id==='a'?a:b)||this!==window||
               event.target!==window||event.currentTarget!==window||event.eventPhase!==2||
               event.bubbles||!event.cancelable||event.defaultPrevented)throw 'invalid rejection payload';
            if(id==='b'&&order.join(',')!=='script,init-job,u-a,m-a')throw 'missing first listener checkpoint';
            event.preventDefault();order.push('u-'+id);
            Promise.resolve().then(()=>{order.push('m-'+id);show();});
        });
        pa=Promise.reject(a);pb=Promise.reject(b);
        Promise.resolve().then(()=>{order.push('init-job');show();});
    "#,
    );
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(
        attribute(&initial, "data-order").as_deref(),
        Some("script,init-job")
    );
    assert_eq!(report(&initial).jobs_admitted, 5); // Two candidates, a reaction and lifecycle.
    assert_eq!(report(&initial).jobs_executed, 1);
    assert_eq!(report(&initial).pending_jobs, 4);
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    peer.idle();
    let notified = peer.pump(1, 0, token);
    no_activation(&notified);
    assert_eq!(notified["reply"]["state"], "Ready");
    assert_eq!(
        notified["reply"]["errors"],
        json!(["Unhandled Promise rejection", "Unhandled Promise rejection"])
    );
    assert_eq!(
        attribute(&notified, "data-order").as_deref(),
        Some("script,init-job,u-a,m-a,u-b,m-b")
    );
    assert!(attribute(&notified, "data-window-error").is_none());
    assert_eq!(report(&notified).jobs_admitted, 7);
    assert_eq!(report(&notified).jobs_executed, 5);
    assert_eq!(report(&notified).pending_jobs, 2);
    report(&notified).validate_after(&report(&initial)).unwrap();
    let completed = peer.complete_lifecycle(notified);
    assert_eq!(report(&completed).jobs_admitted, 7);
    assert_eq!(report(&completed).jobs_executed, 7);
    assert_eq!(report(&completed).pending_jobs, 0);
    assert_eq!(completed["reply"]["lifecycle"]["initial_failure"], true);
    assert!(completed["reply"]["task_wake"].is_null());
    peer.idle();
    peer.finish();
}

#[test]
fn real_worker_synchronous_rejections_cannot_bypass_shared_pending_admission() {
    let mut peer = Peer::start();
    let initial = peer.initialize(
        r#"
        window.onerror=()=>document.getElementById('o').setAttribute('data-reporter','wrong');
        for(let i=0;i<257;i++)Promise.reject(i);
        document.getElementById('o').setAttribute('data-after','wrong');
    "#,
    );
    assert_eq!(initial["reply"]["state"], "Fatal");
    assert_eq!(report(&initial).jobs_admitted, 256);
    assert_eq!(report(&initial).jobs_executed, 0);
    assert_eq!(report(&initial).pending_jobs, 0);
    assert!(report(&initial).fatal_reason.is_some());
    assert!(initial["reply"]["task_wake"].is_null());
    assert!(attribute(&initial, "data-reporter").is_none());
    assert!(attribute(&initial, "data-after").is_none());
    let errors = initial["reply"]["errors"].as_array().unwrap();
    assert!(!errors.is_empty());
    assert!(
        errors
            .iter()
            .all(|error| error.as_str().unwrap().contains("EngineLimit:")),
        "{errors:?}"
    );
    peer.finish();
}

#[test]
fn real_worker_muted_rejection_observation_gap_survives_catch_and_later_reply() {
    const NOTICE: &str = "Promise rejection observation suppressed by source-exposure policy; handled status unknown.";
    let mut peer = Peer::start();
    let initial = peer.initialize(
        r#"
        try{eval(')')}catch(error){}
        Promise.reject({proof:'private'}).catch(()=>document.getElementById('o').setAttribute('data-caught','yes'));
        setTimeout(()=>document.getElementById('o').setAttribute('data-later','yes'),0);
    "#,
    );
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([NOTICE]));
    assert_eq!(attribute(&initial, "data-caught").as_deref(), Some("yes"));
    // Suppressed observation retains no rejection permit. Only the real catch
    // reaction, timer and two mandatory lifecycle tasks consume shared work.
    assert_eq!(report(&initial).jobs_admitted, 4);
    assert_eq!(report(&initial).jobs_executed, 1);
    assert_eq!(report(&initial).pending_jobs, 3);
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let later = peer.pump(1, 0, token);
    no_activation(&later);
    assert_eq!(later["reply"]["state"], "Ready");
    assert_eq!(later["reply"]["errors"], json!([NOTICE]));
    assert_eq!(attribute(&later, "data-later").as_deref(), Some("yes"));
    assert_eq!(report(&later).jobs_admitted, 4);
    assert_eq!(report(&later).jobs_executed, 2);
    assert_eq!(report(&later).pending_jobs, 2);
    report(&later).validate_after(&report(&initial)).unwrap();
    let completed = peer.complete_lifecycle(later);
    assert_eq!(completed["reply"]["errors"], json!([NOTICE]));
    assert_eq!(report(&completed).jobs_admitted, 4);
    assert_eq!(report(&completed).jobs_executed, 4);
    assert_eq!(report(&completed).pending_jobs, 0);
    assert!(completed["reply"]["task_wake"].is_null());
    peer.finish();
}

#[test]
fn real_worker_fatal_rejection_delivery_exits_with_parent_input_still_open() {
    let mut peer = Peer::start();
    let initial = peer.initialize(
        r#"
        window.onerror=()=>document.getElementById('o').setAttribute('data-reporter','wrong');
        window.addEventListener('unhandledrejection',event=>{
            document.getElementById('o').setAttribute('data-entered','yes');
            Promise.resolve().then(()=>document.getElementById('o').setAttribute('data-job','wrong'));
            while(true){}
        });
        window.addEventListener('unhandledrejection',()=>document.getElementById('o').setAttribute('data-listener','wrong'));
        (()=>{const reason=document.createElement('div');Promise.reject(reason);})();
        Promise.reject('second candidate');
    "#,
    );
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(report(&initial).pending_jobs, 4);
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let fatal = peer.pump(1, 0, token);
    no_activation(&fatal);
    assert_eq!(fatal["reply"]["state"], "Fatal");
    assert_eq!(report(&fatal).jobs_admitted, 5); // Candidates, lifecycle, then stopped reaction.
    assert_eq!(report(&fatal).jobs_executed, 1);
    assert_eq!(report(&fatal).pending_jobs, 0);
    assert!(report(&fatal).fatal_reason.is_some());
    assert!(fatal["reply"]["task_wake"].is_null());
    assert_eq!(attribute(&fatal, "data-entered").as_deref(), Some("yes"));
    for field in ["data-reporter", "data-job", "data-listener"] {
        assert!(attribute(&fatal, field).is_none(), "{field}");
    }
    let errors = fatal["reply"]["errors"].as_array().unwrap();
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.as_str() == Some("Unhandled Promise rejection"))
            .count(),
        1
    );
    assert!(
        errors
            .iter()
            .any(|error| error.as_str().unwrap().contains("EngineLimit:"))
    );
    report(&fatal).validate_after(&report(&initial)).unwrap();
    // Do not close stdin to manufacture EOF cleanup: a terminal reply itself
    // must make the real restricted child drop its realm and exit successfully.
    assert!(peer.input.is_some());
    loop {
        if let Some(status) = peer.child.try_wait().unwrap() {
            assert!(status.success(), "terminal task child {status}");
            break;
        }
        peer.tick();
    }
    let mut byte = [0];
    assert_eq!(peer.output.read(&mut byte).unwrap(), 0);
}
