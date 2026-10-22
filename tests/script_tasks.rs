//! Owned modern-v3 process controls; no display, fixture server or parent JS.
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
        let payload = serde_json::to_vec(&json!({
            "version":3,"generation":31,"session_id":47,"sequence":sequence,
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
    fn receive(&mut self, sequence: u64, revision: u64) -> Value {
        let len = u32::from_be_bytes(self.exact(4).try_into().unwrap()) as usize;
        assert!(len > 0);
        let frame: Value = serde_json::from_slice(&self.exact(len)).unwrap();
        for (field, value) in [
            ("version", 3),
            ("generation", 31),
            ("session_id", 47),
            ("sequence", sequence),
            ("expected_revision", revision),
        ] {
            assert_eq!(frame[field], value, "{field}");
        }
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

#[test]
fn real_worker_only_runs_one_due_timer_and_its_checkpoint_per_parent_pump() {
    let mut peer = Peer::start();
    let initial = peer.initialize("var order=['init'];function show(){document.getElementById('o').setAttribute('data-order',order.join(','));}Promise.resolve().then(function(){order.push('micro');show();});var gone=setTimeout(function(){order.push('canceled');show();},0);clearTimeout(gone);setTimeout(function(){order.push('first');show();Promise.resolve().then(function(){order.push('checkpoint');show();});},0);setTimeout(function(){order.push('second');show();},0);");
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(
        attribute(&initial, "data-order").as_deref(),
        Some("init,micro")
    );
    assert_eq!(report(&initial).pending_jobs, 2);
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    peer.idle();
    let first = peer.pump(1, 0, token);
    no_activation(&first);
    assert_eq!(
        attribute(&first, "data-order").as_deref(),
        Some("init,micro,first,checkpoint")
    );
    assert_eq!(report(&first).pending_jobs, 1);
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
    assert!(second["reply"]["task_wake"].is_null());
    assert_eq!(report(&second).pending_jobs, 0);
    report(&second).validate_after(&report(&first)).unwrap();
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
        let (token, delay) = wake(&initial);
        let (sequence, revision, bad_token) = if invalid == "replayed" {
            peer.wait_due(delay);
            let first = peer.pump(1, 0, token);
            assert_eq!(attribute(&first, "data-ran").as_deref(), Some("first"));
            assert!(wake(&first).0 > token);
            (2, 1, token)
        } else {
            (
                1,
                0,
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
        assert_eq!(attribute(&next, "data-count"), Some(sequence.to_string()));
        report(&next).validate_after(&report(&reply)).unwrap();
        reply = next;
    }
    assert!(report(&reply).opcodes_remaining < first.opcodes_remaining);
    assert!(report(&reply).jobs_executed >= 63);
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
