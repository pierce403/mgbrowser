//! Actual owned worker tests. The test harness itself remains unconfined.
//! These cases prove the named experimental profile, not hostile-web safety or
//! comprehensive cooperative parser/builtin/RegExp/GC resource accounting.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use mg_butane::modern::{CUMULATIVE_JOB_LIMIT, Report};
use mg_sparkle::js_browser::{Reply, Request};
use std::{
    io::{self, Read, Write},
    os::{fd::AsRawFd, unix::process::ExitStatusExt},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const PIPE_CAP: usize = 64 * 1024;
const FAILURE: &str = "Boa worker requested-allocation limit reached\n";

struct OwnedChild {
    child: Child,
    reaped: bool,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn nonblocking(fd: i32) {
    // SAFETY: the child pipe descriptor remains owned/open throughout this call.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
}

fn drain(reader: &mut impl Read, bytes: &mut Vec<u8>) {
    let mut chunk = [0; 4096];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                assert!(
                    n <= PIPE_CAP.saturating_sub(bytes.len()),
                    "worker test output exceeded 64 KiB"
                );
                bytes.extend_from_slice(&chunk[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => panic!("read owned child pipe: {error}"),
        }
    }
}

fn run(args: &[&str], input: &[u8]) -> (ExitStatus, String, String) {
    // Admission happens before spawn. The request fits below PIPE_BUF; output
    // is drained while waiting so a full pipe cannot masquerade as a CPU hang.
    assert!(input.len() < 4096);
    let mut owned = OwnedChild {
        child: Command::new(env!("CARGO_BIN_EXE_mgbrowser"))
            .args(args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start owned Boa test worker"),
        reaped: false,
    };
    let mut stdin = owned.child.stdin.take().unwrap();
    let mut stdout = owned.child.stdout.take().unwrap();
    let mut stderr = owned.child.stderr.take().unwrap();
    nonblocking(stdout.as_raw_fd());
    nonblocking(stderr.as_raw_fd());
    stdin.write_all(input).expect("write tiny test request");
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let status = loop {
        drain(&mut stdout, &mut output);
        drain(&mut stderr, &mut errors);
        if let Some(status) = owned.child.try_wait().unwrap() {
            owned.reaped = true;
            drain(&mut stdout, &mut output);
            drain(&mut stderr, &mut errors);
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "owned Boa worker exceeded test deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    (
        status,
        String::from_utf8(output).unwrap(),
        String::from_utf8(errors).unwrap(),
    )
}

fn page(source: &str) -> (ExitStatus, String, String) {
    let request = Request {
        url: "https://example.test/boa-owned-fixture".into(),
        html: format!(
            "<!doctype html><title>Owned Boa fixture</title><p id=status>readable fallback</p><script>{source}</script>"
        ),
    };
    run(&["--script-worker"], &serde_json::to_vec(&request).unwrap())
}

fn reply(outcome: (ExitStatus, String, String)) -> Reply {
    let (status, stdout, stderr) = outcome;
    assert!(status.success(), "worker {status}: {stdout}\n{stderr}");
    assert!(stderr.is_empty(), "unexpected worker stderr: {stderr}");
    let reply: Reply = serde_json::from_str(&stdout).expect("actual worker reply");
    let report = reply.boa.as_ref().expect("Boa, not the original backend");
    report.validate().unwrap();
    assert!(
        reply.allocations.is_none(),
        "never relabel original logical-allocation accounting as Boa memory"
    );
    let memory = report
        .worker_memory
        .expect("actual worker requested-allocation report");
    assert!(memory.is_valid());
    assert!(memory.allocations > 0 && memory.outstanding_bytes > 0);
    reply
}

fn fatal(reply: &Reply) -> &Report {
    assert!(
        !reply.errors.is_empty(),
        "fatal rejection needs readable diagnostics"
    );
    assert!(
        reply.navigation.is_none(),
        "fatal execution must not navigate"
    );
    let report = reply.boa.as_ref().unwrap();
    assert!(
        report.fatal_reason.is_some(),
        "not a cooperative failure: {report:?}"
    );
    assert_eq!(report.pending_jobs, 0);
    assert!(reply.html.contains("readable fallback"));
    report
}

#[test]
fn modern_page_and_promise_dom_effects_have_real_worker_memory_evidence() {
    let reply = reply(page(
        "class Box { constructor(n) { this.n = n; } } const values = new Map([['answer', new Box(42)]]); Promise.resolve().then(() => { document.getElementById('status').textContent = 'Boa page answer ' + values.get('answer').n; });",
    ));
    assert!(reply.applied);
    assert!(reply.errors.is_empty(), "{:?}", reply.errors);
    assert_eq!(reply.scripts_executed, 1);
    assert!(reply.html.contains("Boa page answer 42"));
    let report = reply.boa.unwrap();
    assert!(report.fatal_reason.is_none());
    assert_eq!(report.pending_jobs, 0);
    assert!(report.jobs_admitted > 0 && report.jobs_executed > 0);
    println!(
        "BOA_WORKER_CALIBRATION {}",
        serde_json::to_string(&report).unwrap()
    );
}

#[test]
fn frozen_modern_and_retained_event_startups_fit_the_worker_profile() {
    for (name, html, scripts) in [
        ("script-boa", include_str!("fixtures/script/boa.html"), 2),
        (
            "script-events",
            include_str!("fixtures/script/events.html"),
            1,
        ),
    ] {
        let request = Request {
            url: format!("https://example.test/{name}"),
            html: html.into(),
        };
        let reply = reply(run(
            &["--script-worker"],
            &serde_json::to_vec(&request).unwrap(),
        ));
        assert!(reply.applied, "{name}: {:?}", reply.errors);
        assert!(reply.errors.is_empty(), "{name}: {:?}", reply.errors);
        assert_eq!(reply.scripts_executed, scripts);
        let report = reply.boa.unwrap();
        assert!(report.fatal_reason.is_none());
        assert_eq!(report.pending_jobs, 0);
        if name == "script-boa" {
            assert!(reply.html.contains("Boa JavaScript page ready"));
            assert!(reply.html.contains("Boa Promise checkpoint: 42"));
        }
        println!(
            "BOA_WORKER_CALIBRATION {name} {}",
            serde_json::to_string(&report).unwrap()
        );
    }
}

#[test]
fn actual_worker_opcode_failure_is_uncatchable_and_preserves_readable_page() {
    let reply = reply(page(
        "try { for (;;) {} } catch (e) { document.getElementById('status').textContent = 'caught unexpectedly'; } location.href = '/must-not-navigate';",
    ));
    let report = fatal(&reply);
    assert_eq!(report.opcodes_remaining, 0);
    assert!(!reply.html.contains(">caught unexpectedly<"));
}

#[test]
fn actual_worker_endless_promise_chain_is_cooperatively_terminated() {
    let reply = reply(page(
        "function again() { Promise.resolve().then(again); } again();",
    ));
    let report = fatal(&reply);
    assert_eq!(report.jobs_admitted, CUMULATIVE_JOB_LIMIT);
    assert!(report.opcodes_remaining > 0);
    assert!(report.fatal_reason.as_ref().unwrap().contains("job"));
}

#[test]
fn actual_worker_dynamic_source_admission_is_fatal() {
    let reply = reply(page(
        "try { new Function(' '.repeat(524289))(); } catch (e) { document.getElementById('status').textContent = 'caught unexpectedly'; }",
    ));
    assert!(
        fatal(&reply)
            .fatal_reason
            .as_ref()
            .unwrap()
            .contains("source")
    );
}

#[test]
fn native_string_allocation_is_stopped_by_worker_admission_not_gc_threshold() {
    let (status, stdout, stderr) = page(
        "const oversized = 'x'.repeat(40 * 1024 * 1024); document.getElementById('status').textContent = 'must not publish';",
    );
    assert_eq!(
        status.code(),
        Some(75),
        "worker {status}: {stdout}\n{stderr}"
    );
    assert!(stdout.is_empty());
    assert_eq!(stderr, FAILURE);
}

#[test]
fn adversarial_regex_is_contained_without_claiming_cooperative_regex_metering() {
    let (status, stdout, stderr) = page(
        "const value = 'a'.repeat(48) + '!'; /^(a+)+\\1$/.test(value); document.getElementById('status').textContent = 'must not finish';",
    );
    // RegExp native work has no cooperative counter in the pinned engine. The
    // unchanged one-CPU-second kernel limit is the explicit final boundary.
    assert!(
        matches!(status.signal(), Some(libc::SIGKILL | libc::SIGXCPU)),
        "worker {status}: {stdout}\n{stderr}"
    );
    assert!(stdout.is_empty(), "no partially accepted DOM response");
}

#[test]
fn allocator_owned_process_live_and_cumulative_limits_are_fatal() {
    for probe in ["allocation-live", "allocation-cumulative"] {
        let (status, stdout, stderr) = run(&["--script-worker", &format!("--probe={probe}")], b"");
        assert_eq!(
            status.code(),
            Some(75),
            "{probe}: {status}: {stdout}\n{stderr}"
        );
        assert!(stdout.is_empty());
        assert_eq!(stderr, FAILURE);
    }
}

#[test]
fn allocator_owned_process_alignment_realloc_and_peak_accounting() {
    let (status, stdout, stderr) = run(&["--script-worker", "--probe=allocation-alignment"], b"");
    assert!(status.success(), "{status}: {stdout}\n{stderr}");
    assert!(stderr.is_empty());
    // The probe asserts exact byte preservation, live balance and cumulative
    // monotonicity in the restricted process before emitting this sentinel.
    assert_eq!(stdout.trim(), "allocation-alignment:passed");
}
