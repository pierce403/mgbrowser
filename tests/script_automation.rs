//! Actual modern-v8 restricted-child controls; no display, network or parent JS.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use mg_sparkle::page_session::{
    LifecyclePhase, SessionReply,
    automation::{AutomationRequest, AutomationResult, Number, Operation, RemoteValue},
};
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
    fn start(mode: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mgbrowser"))
            .arg(mode)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        for fd in [input.as_raw_fd(), output.as_raw_fd()] {
            // SAFETY: this Peer retains both live pipe descriptors.
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
        assert!(
            Instant::now() < self.deadline,
            "owned automation child deadline"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    fn send(&mut self, version: u8, sequence: u64, revision: u64, kind: &str, payload: Value) {
        let payload = serde_json::to_vec(&json!({"version":version,"generation":31,
            "session_id":47,"sequence":sequence,"expected_revision":revision,
            "command":{"kind":kind,"payload":payload}}))
        .unwrap();
        assert!(payload.len() <= 2 * 1024 * 1024);
        let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
        frame.extend(payload);
        let mut offset = 0;
        while offset < frame.len() {
            match self
                .input
                .as_mut()
                .unwrap()
                .write(&frame[offset..(offset + 4096).min(frame.len())])
            {
                Ok(0) => panic!("automation input closed"),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.tick()
                }
                Err(e) => panic!("automation write: {e}"),
            }
        }
    }
    fn exact(&mut self, count: usize) -> Vec<u8> {
        assert!(count <= 4 * 1024 * 1024);
        let mut data = vec![0; count];
        let mut offset = 0;
        while offset < count {
            match self.output.read(&mut data[offset..]) {
                Ok(0) => panic!("truncated automation frame"),
                Ok(n) => offset += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.tick()
                }
                Err(e) => panic!("automation read: {e}"),
            }
        }
        data
    }
    fn receive(&mut self, sequence: u64, revision: u64) -> (Value, SessionReply) {
        let count = u32::from_be_bytes(self.exact(4).try_into().unwrap()) as usize;
        assert!(count > 0);
        let frame: Value = serde_json::from_slice(&self.exact(count)).unwrap();
        for (field, expected) in [
            ("version", 8),
            ("generation", 31),
            ("session_id", 47),
            ("sequence", sequence),
            ("expected_revision", revision),
        ] {
            assert_eq!(frame[field].as_u64(), Some(expected), "{field}");
        }
        assert!(
            frame.get("continuation").is_none(),
            "continuation is trusted manager metadata, not worker authority"
        );
        let reply: SessionReply = serde_json::from_value(frame["reply"].clone()).unwrap();
        assert!(reply.accounting_valid());
        (frame, reply)
    }
    fn load(&mut self, html: &str) -> SessionReply {
        self.send(
            8,
            0,
            0,
            "Init",
            json!({"page":{"url":"https://example.test/automation","html":html},
            "observer":{"kind":"Disabled"}}),
        );
        let (frame, mut reply) = self.receive(0, 0);
        assert_eq!(frame["result"], json!({"kind":"Page"}));
        assert_eq!(reply.lifecycle.unwrap().phase, LifecyclePhase::Interactive);
        for sequence in 1..=2 {
            let wake = reply.task_wake.as_ref().unwrap().token;
            self.send(
                8,
                sequence,
                reply.revision,
                "TaskPump",
                json!({"wake_token":wake}),
            );
            let (frame, next) = self.receive(sequence, reply.revision);
            assert_eq!(frame["result"], json!({"kind":"Page"}));
            cumulative(&reply, &next);
            reply = next;
        }
        assert_eq!(reply.lifecycle.unwrap().phase, LifecyclePhase::LoadComplete);
        reply
    }
    fn automation(
        &mut self,
        sequence: u64,
        previous: &SessionReply,
        operation: Operation,
    ) -> (SessionReply, AutomationResult) {
        let request = AutomationRequest {
            owner: 1,
            operation,
            edits: vec![],
        };
        self.send(
            8,
            sequence,
            previous.revision,
            "Automation",
            serde_json::to_value(&request).unwrap(),
        );
        let (frame, reply) = self.receive(sequence, previous.revision);
        assert_eq!(frame["result"]["kind"], "Automation");
        let result: AutomationResult =
            serde_json::from_value(frame["result"]["payload"].clone()).unwrap();
        result.validate_for(&request).unwrap();
        cumulative(previous, &reply);
        assert_eq!(reply.lifecycle, previous.lifecycle);
        assert_eq!(reply.scripts_executed, 0);
        assert!(reply.outcome.click_canceled.is_none() && reply.outcome.submit_canceled.is_none());
        assert_eq!(
            reply.default_action,
            mg_sparkle::page_session::DefaultAction::None
        );
        (reply, result)
    }
    fn finish(&mut self, expected: i32) {
        self.input.take();
        self.wait_exit(expected);
    }
    fn wait_exit(&mut self, expected: i32) {
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(expected));
                break;
            }
            self.tick();
        }
        let mut byte = [0];
        assert_eq!(
            self.output.read(&mut byte).unwrap(),
            0,
            "no unsolicited/trailing output"
        );
    }
}
fn evaluate(source: &str) -> Operation {
    Operation::Evaluate {
        source: source.into(),
        return_by_value: false,
        group: None,
    }
}
fn cumulative(before: &SessionReply, after: &SessionReply) {
    let old = before.boa.as_ref().unwrap();
    let new = after.boa.as_ref().unwrap();
    new.validate_after(old).unwrap();
    let memory = new.worker_memory.as_ref().unwrap();
    assert_eq!(memory.outstanding_limit_bytes, 32 * 1024 * 1024);
    assert_eq!(memory.cumulative_limit_bytes, 64 * 1024 * 1024);
    assert_eq!(
        new.jobs_admitted, 2,
        "synchronous automation adds no lifecycle/task permits"
    );
    assert!(new.pending_jobs <= 2);
}

#[test]
fn retained_automation_calls_properties_release_and_dom_share_the_real_child() {
    let mut p = Peer::start("--script-session");
    let initial = p.load("<title>before</title><p id=o>Original</p>");
    let (a, result) = p.automation(3, &initial, evaluate("globalThis.saved={n:7};saved"));
    let AutomationResult::Value(RemoteValue::Object { object, .. }) = result else {
        panic!("actual object root required")
    };
    let call = Operation::CallFunction {
        source: "function(x){document.title='changed';return this===x && this.n===7;}".into(),
        receiver: Some(object),
        arguments: vec![mg_sparkle::page_session::automation::Argument::Object(
            object,
        )],
        return_by_value: false,
        group: None,
    };
    let (b, result) = p.automation(4, &a, call);
    assert_eq!(result, AutomationResult::Value(RemoteValue::Boolean(true)));
    let (c, result) = p.automation(5, &b, Operation::GetOwnProperties { object });
    let AutomationResult::Properties(properties) = result else {
        panic!("complete own descriptors required")
    };
    assert_eq!(properties.len(), 1);
    assert_eq!(properties[0].name, "n".encode_utf16().collect::<Vec<_>>());
    let (d, result) = p.automation(6, &c, Operation::ReleaseObject { object });
    assert_eq!(result, AutomationResult::Released);
    let (e, result) = p.automation(7, &d, Operation::ReleaseObject { object });
    assert_eq!(
        result,
        AutomationResult::Released,
        "same-owner duplicate release is idempotent"
    );
    let (_, result) = p.automation(8, &e, evaluate("document.title"));
    assert_eq!(
        result,
        AutomationResult::Value(RemoteValue::String("changed".encode_utf16().collect()))
    );
    p.finish(0);
}

#[test]
fn caught_exception_and_representation_rejection_preserve_preceding_effects() {
    let mut p = Peer::start("--script-session");
    let initial = p.load("<script>var reported=0;window.onerror=()=>{reported++;};</script>");
    let (a, result) = p.automation(
        3,
        &initial,
        evaluate("document.title='before-throw';throw 7"),
    );
    assert!(
        matches!(result,AutomationResult::Exception{exception:Some(RemoteValue::Number(Number::Finite(value))),..} if value==7.0)
    );
    let (b, result) = p.automation(4, &a, evaluate("document.title+':'+reported"));
    assert_eq!(
        result,
        AutomationResult::Value(RemoteValue::String(
            "before-throw:0".encode_utf16().collect()
        ))
    );
    let (c, result) = p.automation(
        5,
        &b,
        evaluate("document.title='before-symbol';Symbol('unsupported')"),
    );
    assert!(matches!(result, AutomationResult::Rejected { .. }));
    let (_, result) = p.automation(6, &c, evaluate("document.title"));
    assert_eq!(
        result,
        AutomationResult::Value(RemoteValue::String(
            "before-symbol".encode_utf16().collect()
        ))
    );
    p.finish(0);
}

#[test]
fn final_sequence_63_preserves_completed_primitive_then_reaps_without_renewal() {
    let mut p = Peer::start("--script-session");
    let mut reply = p.load("<p>bounded</p>");
    for sequence in 3..=63 {
        let (next, result) = p.automation(sequence, &reply, evaluate("1"));
        assert_eq!(
            result,
            AutomationResult::Value(RemoteValue::Number(Number::Finite(1.0)))
        );
        assert_eq!(next.state, mg_sparkle::page_session::RealmState::Ready);
        reply = next;
    }
    // Keep stdin open: quota exhaustion itself must stop the child, not EOF.
    assert!(p.input.is_some());
    p.wait_exit(0);
}

#[test]
fn old_modern_versions_and_later_automation_fail_closed() {
    for version in 1..=7 {
        let mut p = Peer::start("--script-session");
        p.send(version,0,0,"Init",json!({"page":{"url":"https://example.test/","html":"<script>throw 'MUST_NOT_RUN'</script>"},"observer":{"kind":"Disabled"}}));
        p.finish(74);
    }
    let mut p = Peer::start("--script-session");
    let reply = p.load("<p>owned</p>");
    p.send(
        7,
        3,
        reply.revision,
        "Automation",
        serde_json::to_value(AutomationRequest {
            owner: 1,
            operation: evaluate("throw 'MUST_NOT_RUN'"),
            edits: vec![],
        })
        .unwrap(),
    );
    p.finish(74);
}

#[test]
fn invalid_automation_fields_are_not_reinterpreted_as_an_event() {
    for bad in [
        json!({"owner":1,"operation":{"Evaluate":{"source":"1","return_by_value":false,"group":null}},"edits":[],"extra":true}),
        json!({"owner":1,"operation":{"Evaluate":{"source":"1","return_by_value":false,"group":null,"extra":true}},"edits":[]}),
    ] {
        let mut p = Peer::start("--script-session");
        let reply = p.load("<p>owned</p>");
        p.send(8, 3, reply.revision, "Automation", bad);
        p.finish(74);
    }
}

#[cfg(feature = "legacy-test-engine")]
#[test]
fn strict_legacy_v1_rejects_automation_and_keeps_its_historical_reply_shape() {
    let mut p = Peer::start("--legacy-script-session");
    p.send(
        1,
        0,
        0,
        "Init",
        json!({"url":"https://example.test/","html":"<p>legacy</p>"}),
    );
    let count = u32::from_be_bytes(p.exact(4).try_into().unwrap()) as usize;
    let frame: Value = serde_json::from_slice(&p.exact(count)).unwrap();
    assert_eq!(frame["version"], 1);
    assert!(frame.get("result").is_none());
    assert!(frame["reply"].get("lifecycle").is_none());
    assert!(frame["reply"].get("observer").is_none());
    p.send(
        1,
        1,
        0,
        "Automation",
        serde_json::to_value(AutomationRequest {
            owner: 1,
            operation: evaluate("throw 'MUST_NOT_RUN'"),
            edits: vec![],
        })
        .unwrap(),
    );
    p.finish(74);
}
