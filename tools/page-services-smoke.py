#!/usr/bin/env python3
"""Accept page services against supplied packaged/public bytes using owned fixtures.

From the repository root, after building journey_server and cdp_journey:
    python3 tools/page-services-smoke.py /absolute/path/to/mgbrowser

Requires Linux /proc, Python's standard library, Xvfb and xauth. This does not
build anything, install a browser, contact public sites or use a current display.
Logs, native/CDP PNGs and report.json remain in a new ignored tmp/ directory.
The fixed-port journey fixture must be free; unrelated services are never reused.
"""

import argparse
from collections import Counter
import hashlib
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import secrets
import select
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time


REPO = Path(__file__).resolve().parents[1]
FIXTURE = "http://127.0.0.1:7878/script-external-events"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def listener_owned(pid, port):
    """Check that this process owns the exact IPv4 loopback listening socket."""
    inodes = set()
    for line in Path("/proc/net/tcp").read_text().splitlines()[1:]:
        fields = line.split()
        if fields[1] == f"0100007F:{port:04X}" and fields[3] == "0A":
            inodes.add(fields[9])
    try:
        for entry in Path(f"/proc/{pid}/fd").iterdir():
            try:
                target = os.readlink(entry)
            except (FileNotFoundError, PermissionError):
                continue
            if target.startswith("socket:[") and target[8:-1] in inodes:
                return True
    except (FileNotFoundError, PermissionError):
        pass
    return False


def wait_for(process, predicate, description, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        require(process.poll() is None, f"{description}: owned process exited")
        if predicate():
            return
        time.sleep(0.05)
    raise RuntimeError(f"Timed out waiting for {description}")


def stop(process):
    # Every child starts a fresh session; include its owned script workers.
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=3)


class Run:
    def __init__(self, root, env):
        self.root = root
        self.env = env
        self.children = []
        self.files = []
        self.commands = []

    def start(self, command, log, env=None, **kwargs):
        output = (self.root / log).open("wb")
        self.files.append(output)
        process = subprocess.Popen(
            list(map(str, command)), cwd=REPO, env=env or self.env,
            stdout=output, stderr=subprocess.STDOUT, start_new_session=True,
            **kwargs,
        )
        self.children.append(process)
        return process

    def command(self, command, log, timeout=40, env=None, expected_exit=0):
        process = self.start(command, log, env=env)
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            stop(process)
            raise RuntimeError(f"{log}: command exceeded {timeout} seconds") from None
        self.commands.append({"argv": list(map(str, command)), "log": log,
                              "returncode": code})
        require(code == expected_exit,
                f"{log}: exit status {code}, expected {expected_exit}")
        return (self.root / log).read_text(errors="replace")

    def close(self):
        failures = []
        for process in reversed(self.children):
            try:
                stop(process)
            except BaseException as error:
                failures.append(f"owned process {process.pid}: {type(error).__name__}: {error}")
        for output in self.files:
            try:
                output.close()
            except BaseException as error:
                failures.append(f"acceptance log: {type(error).__name__}: {error}")
        if failures:
            raise RuntimeError("Acceptance cleanup failed: " + "; ".join(failures))


def start_display(run):
    authority = run.root / "xauthority"
    authority.touch(mode=0o600)
    cookie = secrets.token_hex(16)
    # The server consumes the cookie independent of the display-number key;
    # add the selected number for clients after Xvfb has allocated it.
    subprocess.run(["xauth", "-f", authority, "add", ":0", ".", cookie],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    read_fd, write_fd = os.pipe()
    try:
        process = run.start(
            ["Xvfb", "-displayfd", write_fd, "-screen", "0", "1920x1080x24",
             "-nolisten", "tcp", "-auth", authority], "xvfb.log",
            pass_fds=(write_fd,),
        )
        os.close(write_fd)
        write_fd = None
        ready, _, _ = select.select([read_fd], [], [], 10)
        require(ready, "Xvfb did not allocate an owned display")
        display = os.read(read_fd, 64).decode().strip()
        require(display.isdigit() and process.poll() is None,
                "Xvfb failed to allocate an owned display")
    finally:
        os.close(read_fd)
        if write_fd is not None:
            os.close(write_fd)
    subprocess.run(["xauth", "-f", authority, "add", f":{display}", ".", cookie],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    run.env.update(DISPLAY=f":{display}", XAUTHORITY=str(authority))


def require_journey_port_free(port=7878):
    with socket.socket() as probe:
        # Match the fixture listener's reusable-address bind: a previous owned
        # run may leave TIME_WAIT connections, but an active listener still
        # prevents this bind. Never enable SO_REUSEPORT or reuse another server.
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            probe.bind(("127.0.0.1", port))
        except OSError as error:
            raise RuntimeError(f"127.0.0.1:{port} is occupied; refusing to reuse it") from error


def start_journey(run, helper):
    require_journey_port_free()
    process = run.start([helper], "journey-server.log")
    wait_for(process, lambda: listener_owned(process.pid, 7878),
             "owned journey_server loopback bind")
    connection = http.client.HTTPConnection("127.0.0.1", 7878, timeout=2)
    try:
        connection.request("GET", "/")
        response = connection.getresponse()
        body = response.read()
        require(response.status == 200, "Owned journey readiness request failed")
        require(body == (REPO / "tests/fixtures/journey/home.html").read_bytes(),
                "Owned journey readiness bytes differ from the authored fixture")
    finally:
        connection.close()
    require(process.poll() is None and listener_owned(process.pid, 7878),
            "Lost ownership of the journey listener during readiness")
    return process


def check_worker(log, name):
    require("SCRIPT_COMPLETE executed=3" in log,
            f"{name}: external and surrounding inline scripts did not complete")
    require("SCRIPT_BOA " in log, f"{name}: missing real Boa worker report")
    require("SCRIPT_PARTIAL" not in log and "SCRIPT_DIAGNOSTIC" not in log,
            f"{name}: script execution reported an error")
    workers = [json.loads(line.removeprefix("SCRIPT_BOA "))
               for line in log.splitlines() if line.startswith("SCRIPT_BOA ")]
    require(any(worker.get("profile") == "boa-page-process-v1" and
                worker.get("source_admissions", 0) >= 3 and
                worker.get("source_bytes", 0) > 0 and
                0 < worker.get("opcodes_remaining", 1_000_000) < 1_000_000 and
                worker.get("fatal_reason") is None and
                (worker.get("worker_memory") or {}).get("cumulative_bytes", 0) > 0
                for worker in workers),
            f"{name}: missing nonfatal restricted-worker execution/accounting proof")


def journeys(run, binary, client, fixture=FIXTURE, worker_check=check_worker):
    native_dir = run.root / "native"
    native = run.command(
        [binary, fixture, "--enable-scripts", "--smoke-events", "--exit-after-smoke",
         "--evidence-dir", native_dir], "native.log",
    )
    require("JOURNEY_EVENTS_COMPLETE" in native and "JOURNEY_INCOMPLETE" not in native,
            "Native retained-event journey did not finish")
    worker_check(native, "native")
    for name in ("01-event-query.png", "02-link-canceled.png", "03-submit-canceled.png",
                 "04-event-results.png", "05-event-destination.png"):
        require((native_dir / name).read_bytes().startswith(b"\x89PNG\r\n\x1a\n"),
                f"Native journey did not save {name}")

    browser = run.start(
        [binary, "http://127.0.0.1:7878/", "--enable-scripts", "--remote-debugging-port=0"],
        "cdp-browser.log",
    )
    endpoint = []

    def debugger_ready():
        text = (run.root / "cdp-browser.log").read_text(errors="replace")
        match = re.search(r"^CDP listening on (ws://127\.0\.0\.1:(\d+)/[^ ]+)", text, re.M)
        if not match or not listener_owned(browser.pid, int(match[2])):
            return False
        connection = http.client.HTTPConnection("127.0.0.1", int(match[2]), timeout=1)
        try:
            connection.request("GET", "/json/version")
            response = connection.getresponse()
            metadata = json.loads(response.read())
            if response.status == 200 and metadata.get("webSocketDebuggerUrl") == match[1]:
                endpoint[:] = [match[1]]
                return True
        except (OSError, ValueError):
            pass
        finally:
            connection.close()
        return False

    wait_for(browser, debugger_ready, "owned browser CDP listener")
    client_log = run.command(
        [client, endpoint[0], fixture, run.root / "cdp-destination.png"], "cdp-client.log",
    )
    for marker in ("CDP_EVENTS_STATE link canceled", "CDP_EVENTS_STATE first submit canceled",
                   "CDP_EVENTS_DESTINATION", "CDP_EVENTS_OK"):
        require(marker in client_log, f"Missing unchanged CDP acceptance: {marker}")
    worker_check((run.root / "cdp-browser.log").read_text(errors="replace"), "CDP")
    require((run.root / "cdp-destination.png").read_bytes().startswith(b"\x89PNG\r\n\x1a\n"),
            "CDP did not save an actual PNG")
    stop(browser)


class ProfileServer(ThreadingHTTPServer):
    daemon_threads = True
    block_on_close = False


def profiles(run, binary):
    requests = []
    short = b"<style>p{color:#123456}</style><p>Authored short profile fixture</p>"
    tall = ("<style>p{margin:8px;color:#123456}</style>" +
            "<p>Authored scrolling profile fixture</p>" * 60).encode()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            if self.path == "/malformed":
                self.connection.sendall(
                    b"HTTP/1.1 200 OK\r\nContent-Length: invalid\r\nConnection: close\r\n\r\nbroken"
                )
                self.close_connection = True
                return
            body = {"/short": short, "/tall": tall}.get(self.path)
            self.send_response(200 if body else 404)
            body = body or b"Unknown authored fixture"
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    server = ProfileServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env = dict(run.env)
    env.pop("DISPLAY", None)
    fields = ("Performance Dashboard", "Page Load Breakdown", "Render & Frame Cache Pipeline",
              "Last completed Browser paint:", "Composition/geometry:", "Chrome/Inspector:",
              "Boa JS worker:", "Requested worker allocation:", "Frame cache hits:",
              "Style cache hits:", "Retained snapshot payload:",
              "Shaping lookups (Fonts lifetime):", "Shaping retained:")
    try:
        for path, expected in (
            ("short", "Scroll attempt [no movement: page fits viewport; frame cache hit]"),
            ("tall", "Scroll attempt [position changed; style cache hit]"),
        ):
            log = run.command(
                [binary, "--profile", f"http://127.0.0.1:{server.server_port}/{path}",
                 "--disable-scripts"], f"profile-{path}.log", env=env,
            )
            for label in fields + (expected, "UI redraw [frame cache hit]",
                                  "Repaint Benchmark (complete Browser paints, including composition/chrome):"):
                require(label in log, f"{path} profile is missing {label}")
        failed = run.command(
            [binary, "--profile", f"http://127.0.0.1:{server.server_port}/malformed",
             "--disable-scripts"], "profile-malformed.log", env=env, expected_exit=1,
        )
        require("Profile navigation failed" in failed and
                "Navigation: unsuccessful response or load failure" in failed,
                "Malformed response did not preserve navigation failure diagnostics")
        require("Repaint Benchmark" not in failed,
                "Failed navigation advertised a successful repaint benchmark")
        require(Counter(requests) == Counter({"/short": 1, "/tall": 1, "/malformed": 1}),
                f"Unexpected profile fixture requests: {requests}")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=3)
        (run.root / "profile-requests.json").write_text(json.dumps(requests, indent=2) + "\n")
    return {"requests": requests, "short_noop_scroll": True, "tall_real_scroll": True,
            "malformed_response_nonzero": True, "headless": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("binary", type=Path, help="packaged or public-installed mgbrowser executable")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    helpers = [REPO / "target/debug/examples" / name for name in ("journey_server", "cdp_journey")]
    for path in [binary] + helpers:
        require(path.is_file() and os.access(path, os.X_OK), f"Missing executable: {path}")
    for command in ("Xvfb", "xauth"):
        require(shutil.which(command), f"Required acceptance tool is missing: {command}")
    (REPO / "tmp").mkdir(exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="page-services-smoke-", dir=REPO / "tmp"))
    print(f"PAGE_SERVICES_ARTIFACTS {root}", flush=True)
    private = root / "home"
    private.mkdir()
    env = dict(os.environ, HOME=str(private), XDG_CONFIG_HOME=str(private / "config"),
               XDG_DATA_HOME=str(private / "data"), XDG_CACHE_HOME=str(private / "cache"),
               XDG_STATE_HOME=str(private / "state"), XDG_RUNTIME_DIR=str(private / "runtime"),
               DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent-page-services-session",
               MGBROWSER_NO_AUTO_UPDATE="1")
    for key in ("DISPLAY", "XAUTHORITY", "MGBROWSER_INSTALL_DIR"):
        env.pop(key, None)
    Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
    run = Run(root, env)
    initial_hash = sha256(binary)
    report = {"schema_version": 1, "status": "ERROR", "binary": str(binary),
              "binary_sha256": initial_hash, "commands": run.commands,
              "scope": "authored page services; not WPT, Playwright or live-site compatibility"}
    try:
        server = start_journey(run, helpers[0])
        start_display(run)
        journeys(run, binary, helpers[1])
        require(server.poll() is None and listener_owned(server.pid, 7878),
                "Journey server ownership changed during acceptance")
        server_log = (root / "journey-server.log").read_text(errors="replace")
        require("/event-trap" not in server_log, "Canceled action reached the trap endpoint")
        counts = Counter(server_log.splitlines())
        for path in ("/external-events.js", "/event-search", "/event-destination"):
            require(counts[f"200 OK {path}"] == 2,
                    f"Expected two actual successful requests for {path}: {dict(counts)}")
        report["journeys"] = {"native": True, "external_cdp": True, "trap_requests": 0,
                               "external_js_fetches": 2, "accepted_searches": 2,
                               "handler_destinations": 2, "worker_scripts_per_fixture": 3}
        report["profiles"] = profiles(run, binary)
        require(sha256(binary) == initial_hash, "Supplied binary changed during acceptance")
        report["binary_unchanged"] = True
        report["status"] = "PASS"
    except BaseException as error:
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        try:
            run.close()
        except BaseException as error:
            report["status"] = "ERROR"
            report["cleanup_error"] = f"{type(error).__name__}: {error}"
            raise
        finally:
            (root / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"PAGE_SERVICES_SMOKE_OK {initial_hash} {root}", flush=True)


if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise KeyboardInterrupt("Acceptance interrupted; cleaning owned processes")
    signal.signal(signal.SIGTERM, interrupted)
    main()
