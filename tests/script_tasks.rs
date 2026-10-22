//! Owned modern-v4 process controls; no display, fixture server or parent JS.
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
        self.send_version(4, sequence, revision, kind, payload);
    }
    fn send_version(
        &mut self,
        version: u8,
        sequence: u64,
        revision: u64,
        kind: &str,
        payload: Value,
    ) {
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
    fn receive(&mut self, sequence: u64, revision: u64) -> Value {
        let len = u32::from_be_bytes(self.exact(4).try_into().unwrap()) as usize;
        assert!(len > 0);
        let frame: Value = serde_json::from_slice(&self.exact(len)).unwrap();
        for (field, value) in [
            ("version", 4),
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

#[test]
fn real_worker_timer_error_retains_native_event_across_following_input_transaction() {
    let mut peer = Peer::start();
    let initial = peer.initialize("var value={},saved,order=[];window.onerror=function(message,file,line,column,error){if(arguments.length!==5||this!==window||error!==value||file!==''||line!==0||column!==0)throw 'bad onerror';order.push('property');Promise.resolve().then(()=>order.push('property-job'));return true;};window.addEventListener('error',event=>{if(event.error!==value||event.target!==window||event.currentTarget!==window||event.eventPhase!==2||!event.defaultPrevented||event.bubbles||!event.cancelable)throw 'bad event';saved=event;order.push('listener');Promise.resolve().then(()=>{order.push('listener-job');document.getElementById('o').setAttribute('data-order',order.join(','));});});document.getElementById('o').onclick=event=>{if(saved.error!==value||saved.currentTarget!==null||saved.eventPhase!==0||!saved.defaultPrevented)throw 'lost event';event.preventDefault();document.getElementById('o').setAttribute('data-retained','yes');};setTimeout(()=>{throw value;},60);");
    assert_eq!(initial["reply"]["state"], "Ready");
    assert_eq!(initial["reply"]["errors"], json!([]));
    assert_eq!(report(&initial).pending_jobs, 1);
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let fired = peer.pump(1, 0, token);
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
    report(&fired).validate_after(&report(&initial)).unwrap();
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
        2,
        1,
        "Event",
        json!({"kind":{"kind":"Click","target":target},"edits":[]}),
    );
    let clicked = peer.receive(2, 1);
    assert_eq!(clicked["reply"]["errors"], json!([]));
    assert_eq!(clicked["reply"]["state"], "Ready");
    assert_eq!(clicked["reply"]["outcome"]["click_canceled"], true);
    assert_eq!(attribute(&clicked, "data-retained").as_deref(), Some("yes"));
    assert_eq!(report(&clicked).pending_jobs, 0);
    report(&clicked).validate_after(&report(&fired)).unwrap();
    peer.finish();
}

#[test]
fn real_worker_rejects_v3_init_and_later_commands_without_reply_or_execution() {
    for kind in ["Init", "Event", "TaskPump"] {
        let mut peer = Peer::start();
        let (sequence, payload) = match kind {
            "Init" => (
                0,
                json!({"url":"https://example.test/tasks","html":"<script>throw 'MUST_NOT_RUN'</script>"}),
            ),
            later => {
                let initial = peer.initialize("setTimeout(()=>{throw 'MUST_NOT_RUN';},100000);");
                assert_eq!(initial["reply"]["state"], "Ready");
                assert_eq!(report(&initial).pending_jobs, 1);
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
        peer.send_version(3, sequence, 0, kind, payload);
        loop {
            if let Some(status) = peer.child.try_wait().unwrap() {
                assert!(
                    !status.success(),
                    "old modern version must fail closed: {kind}"
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
    assert_eq!(report(&initial).jobs_admitted, 3);
    assert_eq!(report(&initial).jobs_executed, 1);
    assert_eq!(report(&initial).pending_jobs, 2);
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
    assert_eq!(report(&notified).jobs_admitted, 5);
    assert_eq!(report(&notified).jobs_executed, 5);
    assert_eq!(report(&notified).pending_jobs, 0);
    assert!(notified["reply"]["task_wake"].is_null());
    report(&notified).validate_after(&report(&initial)).unwrap();
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
    // reaction and timer consume work, under the unchanged shared report.
    assert_eq!(report(&initial).jobs_admitted, 2);
    assert_eq!(report(&initial).jobs_executed, 1);
    assert_eq!(report(&initial).pending_jobs, 1);
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let later = peer.pump(1, 0, token);
    no_activation(&later);
    assert_eq!(later["reply"]["state"], "Ready");
    assert_eq!(later["reply"]["errors"], json!([NOTICE]));
    assert_eq!(attribute(&later, "data-later").as_deref(), Some("yes"));
    assert_eq!(report(&later).jobs_admitted, 2);
    assert_eq!(report(&later).jobs_executed, 2);
    assert_eq!(report(&later).pending_jobs, 0);
    assert!(later["reply"]["task_wake"].is_null());
    report(&later).validate_after(&report(&initial)).unwrap();
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
    assert_eq!(report(&initial).pending_jobs, 2);
    let (token, delay) = wake(&initial);
    peer.wait_due(delay);
    let fatal = peer.pump(1, 0, token);
    no_activation(&fatal);
    assert_eq!(fatal["reply"]["state"], "Fatal");
    assert_eq!(report(&fatal).jobs_admitted, 3);
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
