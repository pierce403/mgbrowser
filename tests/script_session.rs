//! Independent framed-protocol checks against actual isolated executable children.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use serde_json::{Value, json};
use std::{
    io::{self, Read, Write},
    os::fd::AsRawFd,
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Peer {
    child: Owned,
    input: Option<ChildStdin>,
    output: ChildStdout,
    deadline: Instant,
}
fn nonblocking(fd: i32) {
    // SAFETY: the test owns this live pipe descriptor; fcntl receives no pointer.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0);
    assert!(unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0);
}
impl Peer {
    fn start() -> Self {
        let mut child = Owned(
            Command::new(env!("CARGO_BIN_EXE_mgbrowser"))
                .arg("--script-session")
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let input = child.0.stdin.take().unwrap();
        let output = child.0.stdout.take().unwrap();
        nonblocking(input.as_raw_fd());
        nonblocking(output.as_raw_fd());
        Self {
            child,
            input: Some(input),
            output,
            deadline: Instant::now() + Duration::from_secs(4),
        }
    }
    fn tick(&self) {
        assert!(Instant::now() < self.deadline, "owned session deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    fn send(&mut self, bytes: &[u8], chunk: usize) {
        let mut offset = 0;
        while offset < bytes.len() {
            match self
                .input
                .as_mut()
                .unwrap()
                .write(&bytes[offset..(offset + chunk).min(bytes.len())])
            {
                Ok(0) => panic!("session input closed"),
                Ok(n) => offset += n,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.tick()
                }
                Err(error) => panic!("session input: {error}"),
            }
        }
    }
    fn exact(&mut self, count: usize) -> Vec<u8> {
        assert!(count <= 4 * 1024 * 1024);
        let mut bytes = vec![0; count];
        let mut offset = 0;
        while offset < count {
            match self.output.read(&mut bytes[offset..]) {
                Ok(0) => panic!("EOF before complete session reply"),
                Ok(n) => offset += n,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) =>
                {
                    self.tick()
                }
                Err(error) => panic!("session output: {error}"),
            }
        }
        bytes
    }
    fn receive(&mut self, sequence: u64, previous: u64) -> Value {
        let header: [u8; 4] = self.exact(4).try_into().unwrap();
        let len = u32::from_be_bytes(header) as usize;
        assert!(len > 0 && len <= 4 * 1024 * 1024);
        let reply: Value = serde_json::from_slice(&self.exact(len)).unwrap();
        for (key, expected) in [
            ("version", 1),
            ("generation", 91),
            ("session_id", 73),
            ("sequence", sequence),
            ("expected_revision", previous),
        ] {
            assert_eq!(reply[key].as_u64(), Some(expected), "{key}");
        }
        reply
    }
    fn wait(&mut self) -> ExitStatus {
        loop {
            if let Some(status) = self.child.0.try_wait().unwrap() {
                return status;
            }
            self.tick();
        }
    }
    fn initialize(&mut self, html: &str) -> Value {
        let message = frame(
            json!({"version":1,"generation":91,"session_id":73,"sequence":0,"expected_revision":0,
            "command":{"kind":"Init","payload":{"url":"http://127.0.0.1:7878/owned","html":html}}}),
        );
        self.send(&message, 3);
        self.receive(0, 0)
    }
}
fn frame(value: Value) -> Vec<u8> {
    let payload = serde_json::to_vec(&value).unwrap();
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend(payload);
    bytes
}
fn event(sequence: u64, previous: u64, target: usize) -> Value {
    json!({"version":1,"generation":91,"session_id":73,"sequence":sequence,"expected_revision":previous,
        "command":{"kind":"Event","payload":{"kind":{"kind":"Click","target":target},"edits":[]}}})
}
fn node(reply: &Value, id: &str) -> usize {
    reply
        .pointer("/reply/snapshot/nodes")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .position(|node| {
            node["attributes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|pair| pair == &json!(["id", id]))
        })
        .unwrap()
}

#[test]
fn split_frames_retain_closure_state_and_clean_idle_eof() {
    let mut peer = Peer::start();
    let initial = peer.initialize("<html><body><a id='link' href='/trap'>Link</a><script>var n=0;document.getElementById('link').onclick=function(e){n++;this.setAttribute('data-count',String(n));e.preventDefault();};</script></body></html>");
    assert_eq!(initial["reply"]["state"], "Ready");
    let target = node(&initial, "link");
    for sequence in 1..=2 {
        peer.send(&frame(event(sequence, sequence - 1, target)), 1);
        let reply = peer.receive(sequence, sequence - 1);
        assert_eq!(reply["reply"]["revision"], sequence);
        assert_eq!(reply["reply"]["outcome"]["click_canceled"], true);
        assert_eq!(reply["reply"]["default_action"], json!({"kind":"None"}));
        assert!(
            reply["reply"]["snapshot"]["nodes"][target]["attributes"]
                .as_array()
                .unwrap()
                .contains(&json!(["data-count", sequence.to_string()]))
        );
    }
    peer.input.take();
    assert!(peer.wait().success());
}

#[test]
fn repeated_init_and_mismatched_identity_or_revision_close_the_child() {
    for change in [
        "version",
        "generation",
        "session_id",
        "sequence",
        "expected_revision",
        "command",
        "unknown",
    ] {
        let mut peer = Peer::start();
        let initial =
            peer.initialize("<html><body><a id='link' href='/trap'>Link</a></body></html>");
        let mut request = event(1, 0, node(&initial, "link"));
        match change {
            "command" => {
                request["command"] =
                    json!({"kind":"Init","payload":{"url":"http://127.0.0.1/","html":""}})
            }
            "unknown" => request["extra_authority"] = json!(true),
            field => request[field] = json!(999),
        }
        peer.send(&frame(request), 4096);
        assert_eq!(peer.wait().code(), Some(74), "{change}");
    }
}

#[test]
fn oversized_event_header_is_rejected_without_waiting_for_its_body() {
    let mut peer = Peer::start();
    peer.initialize("<html><body>Local fixture</body></html>");
    peer.send(&(65_537u32).to_be_bytes(), 1);
    // Keep stdin open: rejection must occur at header admission, not EOF.
    assert_eq!(peer.wait().code(), Some(74));
}

#[test]
fn truncated_headers_payloads_and_zero_length_are_errors() {
    for bytes in [vec![0, 0], vec![0, 0, 0, 5, b'{'], vec![0, 0, 0, 0]] {
        let mut peer = Peer::start();
        peer.send(&bytes, 4096);
        peer.input.take();
        assert_eq!(peer.wait().code(), Some(74));
    }
}

#[test]
fn fatal_reply_is_complete_before_actual_child_exit() {
    let mut peer = Peer::start();
    let reply = peer.initialize("<html><body>Fallback<script>while(true){}</script></body></html>");
    assert_eq!(reply["reply"]["state"], "Fatal");
    assert!(reply["reply"]["allocations"].is_object());
    assert!(reply["reply"]["navigation"].is_null());
    assert!(peer.wait().success());
}

#[test]
fn final_transaction_replies_then_exits_without_a_new_realm() {
    let mut peer = Peer::start();
    let initial = peer.initialize("<html><body><div id='target'>Local</div></body></html>");
    let target = node(&initial, "target");
    for sequence in 1..64 {
        peer.send(&frame(event(sequence, sequence - 1, target)), 4096);
        let reply = peer.receive(sequence, sequence - 1);
        assert_eq!(reply["reply"]["revision"], sequence);
        assert_eq!(reply["reply"]["state"], "Ready");
    }
    assert!(peer.wait().success());
}

#[test]
fn real_parent_session_manager_selftest() {
    let mut child = Owned(
        Command::new(env!("CARGO_BIN_EXE_mgbrowser"))
            .arg("--script-session-selftest")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "manager selftest deadline");
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut stdout = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .take(8192)
        .read_to_string(&mut stdout)
        .unwrap();
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .take(8192)
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(status.success(), "{status}: {stdout}\n{stderr}");
    assert!(
        stdout
            .lines()
            .any(|line| line == "SCRIPT_SESSION_SELFTEST_OK"),
        "{stdout}"
    );
}
