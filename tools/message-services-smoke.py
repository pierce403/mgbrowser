#!/usr/bin/env python3
"""Verify authored queued Window messaging in supplied packaged/public bytes.

Build journey_server/cdp_journey first. Supply a binary argument or
MGBROWSER_BINARY. This owns its private display/profile/loopback processes;
it does not build, install, contact public sites, or use the current display.
This is native/external-CDP fixture evidence, not WPT or harness readiness.
"""

import argparse
from collections import Counter
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import select
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import threading
import time
import http.client


sys.dont_write_bytecode = True
REPO = Path(__file__).resolve().parents[1]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, REPO / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


services = load("message_page_services", "tools/page-services-smoke.py")
control = load("message_process_control", "tools/wpt/process_control.py")
require = services.require
FIXTURE = REPO / "tests/fixtures/script/message-events.html"
FIXTURE_SHA256 = "9b32f5cbd533f0d836cb09a9d9651331ce44b7f81588173789e66e743ced0c96"
FIXTURE_URL = "http://127.0.0.1:7878/message-events"
CHECKPOINT = ("script,after-post,script-microtask,load,load-microtask,property,"
              "property-microtask,listener,listener-microtask")
PROOF = "deep-clone-identity-noncancelable"
LOG_LIMIT = 1024 * 1024
PNG_LIMIT = 8 * 1024 * 1024
EVIDENCE_LIMIT = 64 * 1024 * 1024
ACTIVE_SECONDS = 150
COMMAND_SECONDS = 40


def evidence_size(root):
    """Bound retained artifacts, reserving one metadata limit for report.json."""
    def walk_error(error):
        raise error

    total, entries = 0, 0
    for directory, names, files in os.walk(root, followlinks=False, onerror=walk_error):
        for name in names + files:
            entries += 1
            require(entries <= 256, "Owned evidence entry bound exceeded")
            path = Path(directory) / name
            try:
                info = path.lstat()
            except FileNotFoundError:
                continue
            require(not stat.S_ISLNK(info.st_mode), "Owned evidence became a symlink")
            if stat.S_ISDIR(info.st_mode):
                continue
            require(stat.S_ISREG(info.st_mode), "Owned evidence is not a regular file")
            limit = PNG_LIMIT if path.suffix == ".png" else LOG_LIMIT
            require(info.st_size <= limit, f"Owned evidence overflow: {path}")
            total += info.st_size
            require(total <= EVIDENCE_LIMIT - LOG_LIMIT,
                    "Owned evidence exceeds 64 MiB including reserved report")
    return total


def bounded_text(path):
    return control.read_artifact(path, limit=LOG_LIMIT).decode("utf-8", errors="replace")


def live_log(path):
    """Bound an append-only owned log snapshot without requiring a quiet writer."""
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        info = os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode) and info.st_size <= LOG_LIMIT,
                f"Unsafe or oversized owned log: {path}")
        body = os.read(descriptor, LOG_LIMIT + 1)
        require(len(body) <= LOG_LIMIT, f"Owned log overflow: {path}")
        return body.decode("utf-8", errors="replace")
    finally:
        os.close(descriptor)


class OwnedProcess:
    """Keep the leader PID reserved until exact group termination, then reap it."""

    def __init__(self, process):
        self.process = process
        self.pid = process.pid
        self.closed = False
        self.lock = threading.Lock()

    def poll(self):
        with self.lock:
            if self.closed:
                return self.process.returncode
            result = os.waitid(os.P_PID, self.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            if result is None:
                return None
            return result.si_status if result.si_code == os.CLD_EXITED else -result.si_status

    def signal(self):
        with self.lock:
            if not self.closed:
                try:
                    os.killpg(self.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def stop(self):
        with self.lock:
            if not self.closed:
                try:
                    os.killpg(self.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                # Allow the owned display to remove its own socket/lock files.
                # WNOWAIT reserves the leader PID throughout; kill the group
                # even after leader exit, before the only reaping wait below.
                deadline = time.monotonic() + 1
                while time.monotonic() < deadline:
                    if os.waitid(os.P_PID, self.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None:
                        break
                    time.sleep(0.025)
                try:
                    os.killpg(self.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                self.process.wait(timeout=2)
                self.closed = True


class Run:
    """Small acceptance owner; browser/worker production limits stay unchanged.

    Polling is not an OS disk quota. It rejects over-limit owned evidence and
    cancels the exact owned groups. Only direct leaders are reaped; ordinary
    descendants are terminated, not claimed to be individually reaped.
    """

    def __init__(self, root, env):
        self.root, self.env = root, env
        self.deadline = time.monotonic() + ACTIVE_SECONDS
        self.children, self.files, self.commands = [], [], []
        self.cancel = threading.Event()
        self.monitor_stop = threading.Event()
        self.failure = None
        self.cleanup_confirmed = True
        self.handoff_failures = []
        self.deferred_signal = None
        self.monitor = threading.Thread(target=self.watch, name="message-evidence-watch")
        self.monitor.start()

    def guard(self):
        require(not self.cancel.is_set() and time.monotonic() < self.deadline,
                self.failure or "Acceptance deadline/cancellation stopped admission")

    def watch(self):
        while not self.monitor_stop.wait(0.025):
            try:
                require(time.monotonic() < self.deadline, "Acceptance active deadline exceeded")
                evidence_size(self.root)
            except BaseException as error:
                self.failure = f"{type(error).__name__}: {error}"
                self.cancel.set()
                for process in tuple(self.children):
                    process.signal()
                return

    def start(self, command, log, **kwargs):
        self.guard()
        output = (self.root / log).open("xb")
        self.files.append(output)
        # A raising Python signal handler between Popen's return and assignment
        # could otherwise abandon a launched group. Defer only these parent
        # handlers during the tiny handoff; do not mask signals in the child or
        # use preexec_fn in this threaded parent. Keep at most one pending signal.
        pending = None
        saved_int = signal.getsignal(signal.SIGINT)
        saved_term = signal.getsignal(signal.SIGTERM)

        def defer(signum, _frame):
            nonlocal pending
            if pending is None:
                pending = signum

        raw = process = None
        try:
            signal.signal(signal.SIGINT, defer)
            signal.signal(signal.SIGTERM, defer)
            try:
                raw = subprocess.Popen(
                    list(map(str, command)), cwd=REPO, env=self.env, stdin=subprocess.DEVNULL,
                    stdout=output, stderr=subprocess.STDOUT, start_new_session=True,
                    close_fds=True, **kwargs)
                process = OwnedProcess(raw)
                self.children.append(process)
            except BaseException as error:
                if raw is not None:
                    confirmed = False
                    cleanup_error = None
                    try:
                        try:
                            os.killpg(raw.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        raw.wait(timeout=2)
                        confirmed = True
                        if process is not None:
                            process.closed = True
                    except BaseException as cleanup:
                        cleanup_error = f"{type(cleanup).__name__}: {cleanup}"
                        self.cleanup_confirmed = False
                        self.cancel.set()
                    self.handoff_failures.append({"pid": raw.pid, "log": log,
                        "error": f"{type(error).__name__}: {error}"[:512],
                        "cleanup_succeeded": confirmed, "cleanup_error": cleanup_error,
                        "returncode": raw.returncode})
                raise
        finally:
            # Nested finally also restores TERM if a newly restored INT handler
            # raises during restoration. The group is tracked or already cleaned.
            try:
                signal.signal(signal.SIGINT, saved_int)
            finally:
                signal.signal(signal.SIGTERM, saved_term)
            if pending is not None:
                self.deferred_signal = pending
                raise KeyboardInterrupt(f"Deferred signal {pending}; owned process handoff secured")
        if self.cancel.is_set():
            process.signal()
            self.guard()
        return process

    def command(self, command, log, timeout=COMMAND_SECONDS):
        self.guard()
        artifacts = {self.root / "cdp-destination.png": PNG_LIMIT}
        artifacts.update({self.root / "native" / name: PNG_LIMIT for name in (
            "01-event-query.png", "02-link-canceled.png", "03-submit-canceled.png",
            "04-event-results.png", "05-event-destination.png")})
        result = control.run_bounded(list(map(str, command)), cwd=REPO, env=self.env,
                                     log_path=self.root / log, timeout=timeout,
                                     global_deadline=self.deadline, max_output_bytes=LOG_LIMIT,
                                     artifact_limits=artifacts, cancel_event=self.cancel)
        argv = list(map(str, command))
        if argv[0] == "xauth":
            argv[-1] = "<private ephemeral cookie>"
        self.commands.append({"argv": argv, "log": log, **result})
        if result.get("cleanup_succeeded") is not True:
            self.cleanup_confirmed = False
            self.cancel.set()
        require(result.get("completed") is True and result.get("cleanup_succeeded") is True
                and result.get("status") == "OK" and result.get("returncode") == 0,
                f"{log}: bounded command failed: {result}")
        self.guard()
        return bounded_text(self.root / log)

    def close(self):
        self.cancel.set()
        self.monitor_stop.set()
        self.monitor.join(timeout=1)
        failures = []
        if self.monitor.is_alive():
            failures.append("evidence watchdog did not join")
        for process in reversed(self.children):
            try:
                process.stop()
            except BaseException as error:
                failures.append(f"owned leader {process.pid}: {type(error).__name__}: {error}")
        for output in self.files:
            try:
                output.close()
            except BaseException as error:
                failures.append(f"owned log: {type(error).__name__}: {error}")
        require(self.cleanup_confirmed and not failures,
                "Unconfirmed owned cleanup: " + "; ".join(failures))


def wait_for(run, process, predicate, description):
    deadline = min(run.deadline, time.monotonic() + 10)
    while time.monotonic() < deadline:
        run.guard()
        require(process.poll() is None, f"{description}: owned leader exited")
        if predicate():
            return
        time.sleep(0.025)
    raise RuntimeError(f"Timed out waiting for {description}")


def start_display(run):
    authority = run.root / "xauthority"
    authority.touch(mode=0o600)
    cookie = secrets.token_hex(16)
    run.command(["xauth", "-f", authority, "add", ":0", ".", cookie], "xauth-initial.log", timeout=10)
    read_fd, write_fd = os.pipe()
    try:
        process = run.start(["Xvfb", "-displayfd", write_fd, "-screen", "0", "1920x1080x24",
                             "-nolisten", "tcp", "-auth", authority], "xvfb.log", pass_fds=(write_fd,))
        os.close(write_fd)
        write_fd = None
        ready, _, _ = select.select([read_fd], [], [], min(10, max(0, run.deadline - time.monotonic())))
        require(ready, "Owned Xvfb did not allocate a display")
        display = os.read(read_fd, 64).decode().strip()
        require(display.isdigit() and process.poll() is None, "Owned Xvfb failed")
    finally:
        os.close(read_fd)
        if write_fd is not None:
            os.close(write_fd)
    run.command(["xauth", "-f", authority, "add", f":{display}", ".", cookie], "xauth-client.log", timeout=10)
    run.env.update(DISPLAY=f":{display}", XAUTHORITY=str(authority))


def start_journey(run, helper):
    services.require_journey_port_free()
    server = run.start([helper], "journey-server.log")
    wait_for(run, server, lambda: services.listener_owned(server.pid, 7878), "owned journey listener")
    connection = http.client.HTTPConnection("127.0.0.1", 7878, timeout=2)
    try:
        connection.request("GET", "/")
        response = connection.getresponse()
        body = response.read(LOG_LIMIT + 1)
        require(response.status == 200 and len(body) <= LOG_LIMIT and
                body == control.read_artifact(REPO / "tests/fixtures/journey/home.html", limit=LOG_LIMIT),
                "Owned readiness bytes/status differ")
    finally:
        connection.close()
    return server


def check_messages(log, name):
    for marker in ("SCRIPT_PARTIAL", "SCRIPT_DIAGNOSTIC", "SCRIPT_ERROR", "SCRIPT_REJECTED",
                   "SCRIPT_SESSION_CLOSED", "SCRIPT_SHUTDOWN_ERROR", "LOAD_ERROR",
                   "RESOURCE_WARNING", "RESOURCE_DIAGNOSTIC", "JOURNEY_INCOMPLETE"):
        require(marker not in log, f"{name}: unexpected diagnostic/failure {marker}")
    require(sum(line.startswith("PAGE_TASK_APPLIED ") for line in log.splitlines()) == 1,
            f"{name}: expected one real parent-issued message task")
    reports = [json.loads(line.removeprefix("SCRIPT_BOA ")) for line in log.splitlines()
               if line.startswith("SCRIPT_BOA ")]
    require(len(reports) >= 2, f"{name}: missing real restricted-worker reports")
    for report in reports:
        require(type(report) is dict and "fatal_reason" in report,
                f"{name}: malformed or missing typed worker fatal state")
        integer_fields = ("opcodes_remaining", "jobs_admitted", "jobs_executed", "pending_jobs",
                          "source_admissions", "source_bytes")
        require(all(type(report.get(field)) is int for field in integer_fields),
                f"{name}: malformed worker scalar accounting")
        require(report.get("profile") == "boa-page-process-v1" and report.get("fatal_reason") is None
                and 0 < report["opcodes_remaining"] <= 1_000_000
                and 0 <= report["pending_jobs"] <= 256
                and 0 <= report["jobs_executed"] <= report["jobs_admitted"] <= 2048
                and report["pending_jobs"] <= report["jobs_admitted"] - report["jobs_executed"]
                and 0 <= report["source_admissions"] <= report["source_bytes"] <= 4 * 1024 * 1024,
                f"{name}: invalid/fatal or renewed worker accounting")
        memory = report.get("worker_memory") or {}
        fields = ("outstanding_limit_bytes", "cumulative_limit_bytes", "outstanding_bytes",
                  "peak_bytes", "cumulative_bytes", "allocations")
        require(all(type(memory.get(field)) is int for field in fields), f"{name}: missing real System counters")
        require(memory["outstanding_limit_bytes"] == 32 * 1024 * 1024
                and memory["cumulative_limit_bytes"] == 64 * 1024 * 1024
                and 0 < memory["outstanding_bytes"] <= memory["peak_bytes"] <= 32 * 1024 * 1024
                and memory["peak_bytes"] <= memory["cumulative_bytes"] <= 64 * 1024 * 1024
                and 0 < memory["allocations"] <= memory["cumulative_bytes"],
                f"{name}: invalid/changed requested-System memory bounds")
    source = control.read_artifact(FIXTURE, limit=LOG_LIMIT).split(b"<script>", 1)[1].split(b"</script>", 1)[0]
    realm = [item for item in reports if item["source_admissions"] == 1 and item["source_bytes"] == len(source)]
    require(len(realm) >= 2, f"{name}: missing exact original fixture source reports")
    require(tuple(realm[0][key] for key in ("jobs_admitted", "jobs_executed", "pending_jobs")) == (3, 2, 1),
            f"{name}: Init did not retain the real later message after its two microtasks")
    require(all(tuple(item[key] for key in ("jobs_admitted", "jobs_executed", "pending_jobs")) == (5, 5, 0)
                for item in realm[1:]), f"{name}: message/listener checkpoints did not settle exactly once")
    results = control.read_artifact(REPO / "tests/fixtures/script/event-results.html", limit=LOG_LIMIT)
    results_source = results.split(b"<script>", 1)[1].split(b"</script>", 1)[0]
    for item in reports:
        if item in realm:
            continue
        require((item["source_admissions"], item["source_bytes"]) in ((0, 0), (1, len(results_source)))
                and all(item[key] == 0 for key in ("jobs_admitted", "jobs_executed", "pending_jobs")),
                f"{name}: unexpected later source or unfinished work outside the original fixture")
    for before, after in zip(realm, realm[1:]):
        require(after["opcodes_remaining"] <= before["opcodes_remaining"] and
                all(after[key] >= before[key] for key in ("jobs_admitted", "jobs_executed")) and
                all(after["worker_memory"][key] >= before["worker_memory"][key]
                    for key in ("peak_bytes", "cumulative_bytes", "allocations")),
                f"{name}: original realm renewed cumulative counters")
    return {"reports": len(reports), "original_realm_reports": len(realm),
            "init_work": [3, 2, 1], "settled_work": [5, 5, 0], "applied_tasks": 1,
            "emitted_script_diagnostics": 0}


def png(path):
    body = control.read_artifact(path, limit=PNG_LIMIT)
    require(body.startswith(b"\x89PNG\r\n\x1a\n"), f"Missing actual PNG: {path}")


def check_client(log):
    for marker in ("CDP_EVENTS_STATE link canceled", "CDP_EVENTS_STATE first submit canceled",
                   "CDP_EVENTS_DESTINATION", "CDP_EVENTS_OK"):
        require(marker in log, f"Missing unchanged CDP retained-event acceptance: {marker}")
    marker = f"CDP_WINDOW_MESSAGE_OK {PROOF} checkpoint={CHECKPOINT}"
    require(log.splitlines().count(marker) == 1, "CDP did not verify exact actual message DOM proof/checkpoints")


def journeys(run, binary, client):
    native = run.command([binary, FIXTURE_URL, "--enable-scripts", "--smoke-events", "--exit-after-smoke",
                          "--evidence-dir", run.root / "native"], "native.log")
    require("JOURNEY_EVENTS_COMPLETE" in native, "Native retained-event journey did not finish")
    native_evidence = check_messages(native, "native")
    for name in ("01-event-query.png", "02-link-canceled.png", "03-submit-canceled.png",
                 "04-event-results.png", "05-event-destination.png"):
        png(run.root / "native" / name)
    browser = run.start([binary, "http://127.0.0.1:7878/", "--enable-scripts", "--remote-debugging-port=0"],
                        "cdp-browser.log")
    endpoint = []

    def debugger_ready():
        match = re.search(r"^CDP listening on (ws://127\.0\.0\.1:(\d+)/[^ ]+)",
                          live_log(run.root / "cdp-browser.log"), re.M)
        if not match or not services.listener_owned(browser.pid, int(match[2])):
            return False
        connection = http.client.HTTPConnection("127.0.0.1", int(match[2]), timeout=1)
        try:
            connection.request("GET", "/json/version")
            response = connection.getresponse()
            body = response.read(LOG_LIMIT + 1)
            require(len(body) <= LOG_LIMIT, "CDP discovery response overflow")
            if response.status == 200 and json.loads(body).get("webSocketDebuggerUrl") == match[1]:
                endpoint[:] = [match[1], int(match[2])]
                return True
        except (OSError, ValueError):
            return False
        finally:
            connection.close()
        return False

    wait_for(run, browser, debugger_ready, "owned browser CDP listener")
    client_log = run.command([client, endpoint[0], FIXTURE_URL, run.root / "cdp-destination.png"], "cdp-client.log")
    check_client(client_log)
    require(browser.poll() is None and services.listener_owned(browser.pid, endpoint[1]),
            "CDP browser exited or lost its listener before intentional shutdown")
    browser.stop()
    cdp_evidence = check_messages(bounded_text(run.root / "cdp-browser.log"), "CDP")
    png(run.root / "cdp-destination.png")
    return {"native": native_evidence, "external_cdp": cdp_evidence,
            "clone_event_proof": PROOF, "checkpoint": CHECKPOINT}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="?", type=Path, help="default: MGBROWSER_BINARY")
    args = parser.parse_args()
    configured = args.binary or os.environ.get("MGBROWSER_BINARY")
    if not configured:
        parser.error("supply a binary path or MGBROWSER_BINARY")
    binary = Path(configured).resolve(strict=True)
    helpers = [REPO / "target/debug/examples" / name for name in ("journey_server", "cdp_journey")]
    for path in [binary] + helpers:
        require(path.is_file() and os.access(path, os.X_OK), f"Missing executable: {path}")
    for tool in ("Xvfb", "xauth"):
        require(shutil.which(tool), f"Missing acceptance tool: {tool}")
    require(services.sha256(FIXTURE) == FIXTURE_SHA256, "Frozen fixture identity changed")
    (REPO / "tmp").mkdir(exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="message-services-smoke-", dir=REPO / "tmp"))
    print(f"MESSAGE_SERVICES_ARTIFACTS {root}", flush=True)
    private = root / "home"
    private.mkdir()
    env = dict(os.environ, HOME=str(private), XDG_CONFIG_HOME=str(private / "config"),
               XDG_DATA_HOME=str(private / "data"), XDG_CACHE_HOME=str(private / "cache"),
               XDG_STATE_HOME=str(private / "state"), XDG_RUNTIME_DIR=str(private / "runtime"),
               DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent-message-services-session",
               MGBROWSER_NO_AUTO_UPDATE="1", MESA_SHADER_CACHE_DISABLE="true")
    for key in ("DISPLAY", "XAUTHORITY", "MGBROWSER_INSTALL_DIR"):
        env.pop(key, None)
    Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
    input_paths = [binary, *helpers, FIXTURE, Path(__file__).resolve(),
                   REPO / "tools/page-services-smoke.py", REPO / "tools/wpt/process_control.py",
                   REPO / "tests/fixtures/journey/home.html", REPO / "tests/fixtures/script/event-results.html"]
    inputs = {str(path): services.sha256(path) for path in input_paths}
    report = {"schema_version": 1, "status": "ERROR", "binary": str(binary),
              "binary_sha256": inputs[str(binary)], "input_sha256": inputs,
              "scope": "authored queued self-Window clone/event and retained form; not WPT or harness readiness",
              "limits": {"active_seconds": ACTIVE_SECONDS, "command_seconds": COMMAND_SECONDS,
                         "log_bytes": LOG_LIMIT, "png_bytes": PNG_LIMIT, "evidence_bytes": EVIDENCE_LIMIT},
              "test_environment": {"MESA_SHADER_CACHE_DISABLE": env["MESA_SHADER_CACHE_DISABLE"]},
              "resource_evidence": "Exact inline input/source accounting and HTTP receipts; hidden native resource-warning count unavailable"}
    run = Run(root, env)
    report["commands"] = run.commands
    report["handoff_failures"] = run.handoff_failures
    try:
        server = start_journey(run, helpers[0])
        start_display(run)
        report["journeys"] = journeys(run, binary, helpers[1])
        require(server.poll() is None and services.listener_owned(server.pid, 7878), "Lost owned fixture listener")
        server.stop()
        lines = bounded_text(root / "journey-server.log").splitlines()
        requests = Counter(line for line in lines if re.match(r"^\d{3} ", line))
        expected = Counter({"200 OK /": 2, "200 OK /message-events": 2,
                            "200 OK /event-search": 2, "200 OK /event-destination": 2})
        require(requests == expected, f"Unexpected/missing HTTP receipt or trap request: {dict(requests)}")
        report["http_requests"] = dict(requests)
        require(all(services.sha256(Path(path)) == digest for path, digest in inputs.items()),
                "Acceptance executable/helper/fixture changed")
        run.guard()
        report.update(status="PASS", binary_unchanged=True, inputs_unchanged=True,
                      retained_events=True, trap_requests=0)
    except BaseException as error:
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        admission_error = None
        try:
            run.close()
            report["cleanup"] = {"confirmed": True, "persistent_leaders": len(run.children),
                                 "returncodes": [item.process.returncode for item in run.children],
                                 "completed_command_groups": len(run.commands),
                                 "scope": "owned groups terminated; direct leaders reaped, no claim of individual grandchild reaping"}
        except BaseException as error:
            report["status"] = "ERROR"
            report["cleanup"] = {"confirmed": False, "error": f"{type(error).__name__}: {error}"}
            raise
        finally:
            if run.deferred_signal is not None:
                report["deferred_signal"] = run.deferred_signal
            if run.failure:
                report["status"] = "ERROR"
                report["watchdog_error"] = run.failure
            if report.get("cleanup", {}).get("confirmed") is True:
                try:
                    report["evidence_bytes_before_report"] = evidence_size(root)
                except BaseException as error:
                    admission_error = error
                    report["status"] = "ERROR"
                    report["evidence_error"] = f"{type(error).__name__}: {error}"
            body = (json.dumps(report, indent=2) + "\n").encode()
            require(len(body) <= LOG_LIMIT, "Acceptance report exceeded metadata bound")
            with (root / "report.json").open("xb") as stream:
                stream.write(body)
        if admission_error is not None and "error" not in report:
            raise admission_error
        if run.failure and "error" not in report:
            raise RuntimeError(run.failure)
    print(f"MESSAGE_SERVICES_SMOKE_OK {inputs[str(binary)]} {root}", flush=True)


if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise KeyboardInterrupt("Acceptance interrupted; cleaning owned processes")
    signal.signal(signal.SIGTERM, interrupted)
    main()
