#!/usr/bin/env python3
"""Verify authored physical justification in one supplied packaged/public binary.

This owns one private display/profile and the exact loopback journey service.
Build the pinned journey_server and physical_justify_smoke examples beforehand.
No build, installer, updater, public website or WPT outcome is part of this tool.
All required executable/version/commit/font identities come from its caller.
"""

import argparse
from collections import Counter
import hashlib
import http.client
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import sys
import tempfile
import time


sys.dont_write_bytecode = True
REPO = Path(__file__).resolve().parents[1]
URL = "http://127.0.0.1:7878/physical-justify"
LOG_LIMIT = 1024 * 1024
PNG_LIMIT = 8 * 1024 * 1024
EXECUTABLE_LIMIT = 64 * 1024 * 1024
FONT_LIMIT = 16 * 1024 * 1024
CLIENT_SECONDS = 30
PNG_NAMES = ("native-fixture.png", "native-destination.png",
             "cdp-fixture.png", "cdp-destination.png")
FAILURE_PNG = "failure-native.png"
ROUTES = ("/physical-justify", "/physical-justify-native-hit",
          "/physical-justify", "/physical-justify-cdp-hit")
TITLES = ("Physical justification fixture", "Physical native hit",
          "Physical justification fixture", "Physical CDP hit")
# The three imported owner files are admitted before executing any of them.
# These are the reviewed original authored input bytes, never discovered or
# automatically adopted from the current checkout during a measurement.
SOURCE_PINS = {
    "tools/message-services-smoke.py": "367314ba089e4c9e03b6794782c600b4239d759a9b06516914cd115363eddedd",
    "tools/page-services-smoke.py": "7560bdcea6187df67a524669a89fe4c80aff5f2f7628cb7583b00a18f68ded04",
    "tools/wpt/process_control.py": "eaec5e966841e547cfc92ccd785bf356e519aa16b9dc19e26ac40d4aae120c7d",
    "examples/physical_justify_smoke.rs": "ef9d67368a5ce2940ca0012566478ee94dc71e4ac36fa48ca44110cb1d7378e6",
    "examples/journey_server.rs": "1d421a6d0f8cbe00d7d9ad90e585cb585fba3a459bed994dfff6722749faa29b",
    "tests/fixtures/layout/physical-justify.html": "6e5d85d50a30f7852a1f2fa8b7d01b835213c8abf3edc0392c8f029f0bea3795",
    "tests/fixtures/journey/home.html": "34e7877af1c3d645c131d96a3baed20edfce2998395ae1e7bc46c8674e509cb0",
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def pinned_path(path, limit, executable=False):
    """Admit a regular nonsymlink final component before reading any body."""
    path = Path(path).absolute()
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode)
            and 0 < info.st_size <= limit, f"Unsafe or oversized input: {path}")
    require(not executable or os.access(path, os.X_OK), f"Not executable: {path}")
    return path.resolve(strict=True)


def digest(path, limit):
    path = pinned_path(path, limit)
    before = path.lstat()
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        opened = os.fstat(descriptor)
        require(stat.S_ISREG(opened.st_mode) and opened.st_size == before.st_size
                and (opened.st_dev, opened.st_ino) == (before.st_dev, before.st_ino),
                f"Input changed before hashing: {path}")
        value = hashlib.sha256()
        count = 0
        while count <= before.st_size:
            block = os.read(descriptor, min(64 * 1024, before.st_size + 1 - count))
            if not block:
                break
            count += len(block)
            value.update(block)
        after = os.fstat(descriptor)
        require(count == before.st_size and after.st_size == before.st_size
                and after.st_mtime_ns == opened.st_mtime_ns,
                f"Input changed during hashing: {path}")
        return value.hexdigest()
    finally:
        os.close(descriptor)


def source_inputs():
    inputs = {}
    for relative, expected in SOURCE_PINS.items():
        require(re.fullmatch(r"[0-9a-f]{64}", expected), f"Unfrozen source pin: {relative}")
        path = pinned_path(REPO / relative, LOG_LIMIT)
        actual = digest(path, LOG_LIMIT)
        require(actual == expected, f"Frozen source changed: {relative}")
        inputs[str(path)] = {"sha256": actual, "limit": LOG_LIMIT}
    own = pinned_path(__file__, LOG_LIMIT)
    inputs[str(own)] = {"sha256": digest(own, LOG_LIMIT), "limit": LOG_LIMIT}
    return inputs


def load_owned():
    spec = importlib.util.spec_from_file_location(
        "physical_message_services", REPO / "tools/message-services-smoke.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--expected-sha", required=True)
    parser.add_argument("--expected-version", required=True)
    parser.add_argument("--expected-commit", required=True,
                        help="exact 12- or 40-hex build commit, with -dirty if present")
    parser.add_argument("--client", type=Path, required=True)
    parser.add_argument("--client-sha", required=True)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--server-sha", required=True)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--font-sha", required=True)
    args = parser.parse_args()
    for key in ("expected_sha", "client_sha", "server_sha", "font_sha"):
        require(re.fullmatch(r"[0-9a-f]{64}", getattr(args, key)), f"Invalid {key}")
    require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.expected_version),
            "Expected version must be an explicit release version")
    require(re.fullmatch(r"(?:[0-9a-f]{12}|[0-9a-f]{40})(?:-dirty)?", args.expected_commit),
            "Expected commit must be exact 12/40 hex with the actual dirty suffix")
    return args


def register_input(inputs, path, expected, limit, executable=False):
    path = pinned_path(path, limit, executable)
    actual = digest(path, limit)
    require(actual == expected, f"Explicit input identity mismatch: {path}")
    inputs[str(path)] = {"sha256": actual, "limit": limit}
    return path


def unchanged(inputs):
    for path, admission in inputs.items():
        require(digest(Path(path), admission["limit"]) == admission["sha256"],
                f"Acceptance input changed: {path}")


def private_environment(root, font):
    private = root / "home"
    private.mkdir(mode=0o700)
    env = {"PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
           "HOME": str(private), "DBUS_SESSION_BUS_ADDRESS":
           "unix:path=" + str(private / "no-session"),
           "MGBROWSER_NO_AUTO_UPDATE": "1", "MESA_SHADER_CACHE_DISABLE": "true",
           "MGBROWSER_FONT": str(font)}
    for key, child in (("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                       ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state"),
                       ("XDG_RUNTIME_DIR", "runtime"), ("TMPDIR", "tmp")):
        target = private / child
        target.mkdir(mode=0o700)
        env[key] = str(target)
    return env


def expected_about_commit(commit):
    return commit[:12] + ("-dirty" if commit.endswith("-dirty") else "")


def check_identity(run, binary, args, label):
    version = run.command([binary, "--version"], label + "-version.log")
    require(version.strip() == f"mgbrowser {args.expected_version}",
            "Supplied executable version differs from its explicit identity")
    about = run.command([binary, "--about"], label + "-about.log")
    lines = about.splitlines()
    require(len(lines) == 4 and lines[0] == f"mgbrowser {args.expected_version}"
            and lines[1].startswith("Compiled: ") and lines[1].endswith(" GMT")
            and lines[2] == "Commit: " + expected_about_commit(args.expected_commit)
            and lines[3] == "Experimental Preview : Apache-2.0",
            "Supplied executable About does not match its exact build identity")
    return {"version": version.strip(), "about": lines}


def start_browser(owned, run, binary):
    browser = run.start([binary, URL, "--enable-scripts", "--remote-debugging-port=0"],
                        "browser.log")
    endpoint = []

    def discover():
        text = owned.live_log(run.root / "browser.log")
        windows = re.findall(r"^WINDOW id=([1-9][0-9]*) pid=([1-9][0-9]*)$", text, re.M)
        require(len(windows) <= 1, "Unexpected extra native browser window")
        match = re.search(r"^CDP listening on (ws://127\.0\.0\.1:([0-9]+)/devtools/browser/browser-1) \(experimental subset; local clients control this page\)$", text, re.M)
        if not match or not windows:
            return False
        require(int(windows[0][1]) == browser.pid, "Native window is not owned by the supplied browser")
        port = int(match[2])
        require(0 < port < 65536 and port != 7878, "Invalid/conflicting owned CDP port")
        if not owned.services.listener_owned(browser.pid, port):
            return False
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
        try:
            connection.request("GET", "/json/version")
            response = connection.getresponse()
            body = response.read(LOG_LIMIT + 1)
            require(response.status == 200 and len(body) <= LOG_LIMIT
                    and json.loads(body).get("webSocketDebuggerUrl") == match[1],
                    "Owned CDP discovery identity mismatch")
            connection.close()
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
            connection.request("GET", "/json/list")
            response = connection.getresponse()
            body = response.read(LOG_LIMIT + 1)
            require(response.status == 200 and len(body) <= LOG_LIMIT,
                    "Owned page discovery status/size mismatch")
            pages = json.loads(body)
            page_endpoint = f"ws://127.0.0.1:{port}/devtools/page/page-1"
            require(type(pages) is list and len(pages) == 1 and type(pages[0]) is dict
                    and pages[0].get("id") == "page-1"
                    and pages[0].get("webSocketDebuggerUrl") == page_endpoint,
                    "Owned browser does not have exactly the expected page-1")
            endpoint[:] = [page_endpoint, port, windows[0][0]]
            return True
        except OSError:
            return False
        finally:
            connection.close()

    owned.wait_for(run, browser, discover, "owned single-window/page CDP discovery")
    return browser, endpoint


def check_script_free_lifecycle(log):
    """Require four real realms, not an inferred blanket extra-work allowance."""
    for marker in ("SCRIPT_PARTIAL", "SCRIPT_DIAGNOSTIC", "SCRIPT_ERROR", "SCRIPT_REJECTED",
                   "SCRIPT_SESSION_CLOSED", "SCRIPT_SHUTDOWN_ERROR", "LOAD_ERROR",
                   "RESOURCE_WARNING", "RESOURCE_DIAGNOSTIC", "JOURNEY_INCOMPLETE"):
        require(marker not in log, f"Unexpected emitted failure: {marker}")
    lines = log.splitlines()
    require(sum(line.startswith("SCRIPT_BOA ") for line in lines) == 14
            and sum(line.startswith("PAGE_TASK_APPLIED ") for line in lines) == 8
            and sum(line.startswith("PAGE_EVENT_APPLIED ") for line in lines) == 2
            and [line for line in lines if line.startswith("SCRIPT_COMPLETE ")]
            == ["SCRIPT_COMPLETE executed=0"] * 14,
            "Unexpected global script/work acknowledgment prefix")
    sections = re.split(r"(?m)^NAVIGATE GET ", log)[1:]
    require(len(sections) == 4, "Expected exactly four actual navigations")
    rows = []
    for index, (section, route, count) in enumerate(zip(sections, ROUTES, (4, 3, 4, 3))):
        lines = section.splitlines()
        full_url = "http://127.0.0.1:7878" + route
        require(lines[0] == full_url, "Actual navigation order/target differs")
        loaded = [line for line in lines if line.startswith("LOADED ")]
        require(len(loaded) == 1 and re.fullmatch(
                    re.escape(f'LOADED {full_url} HTTP 200 title="{TITLES[index]}" ')
                    + r"items=[0-9]+ forms=0", loaded[0]),
                "Actual navigation has no exact successful load")
        tasks = [line for line in lines if line.startswith("PAGE_TASK_APPLIED ")]
        require(tasks == ["PAGE_TASK_APPLIED sequence=1 revision=1",
                          "PAGE_TASK_APPLIED sequence=2 revision=2"],
                "Actual realm did not apply exactly its mandatory lifecycle pair")
        events = [line for line in lines if line.startswith("PAGE_EVENT_APPLIED ")]
        expected_event = ["PAGE_EVENT_APPLIED sequence=3 revision=3 click_canceled=Some(false) submit_canceled=None"]
        require(events == (expected_event if index in (0, 2) else []),
                "Actual click/default-action acknowledgment differs")
        reports = [json.loads(line.removeprefix("SCRIPT_BOA ")) for line in lines
                   if line.startswith("SCRIPT_BOA ")]
        require(len(reports) == count, "Actual realm worker-report count differs")
        expected_work = [(2, 0, 2), (2, 1, 1), (2, 2, 0)] + ([(2, 2, 0)] if count == 4 else [])
        for report, work in zip(reports, expected_work):
            require(type(report) is dict and report.get("profile") == "boa-page-process-v1"
                    and "fatal_reason" in report and report["fatal_reason"] is None,
                    "Missing/nonmodern/fatal actual worker report")
            fields = ("source_admissions", "source_bytes", "opcodes_remaining",
                      "jobs_admitted", "jobs_executed", "pending_jobs")
            require(all(type(report.get(key)) is int for key in fields)
                    and report["source_admissions"] == report["source_bytes"] == 0
                    and 0 < report["opcodes_remaining"] <= 1_000_000
                    and tuple(report[key] for key in fields[3:]) == work,
                    "Actual script-free lifecycle work/source bounds differ")
            memory = report.get("worker_memory")
            memory_fields = ("outstanding_limit_bytes", "cumulative_limit_bytes", "outstanding_bytes",
                             "peak_bytes", "cumulative_bytes", "allocations")
            require(type(memory) is dict and all(type(memory.get(key)) is int for key in memory_fields)
                    and memory["outstanding_limit_bytes"] == 32 * 1024 * 1024
                    and memory["cumulative_limit_bytes"] == 64 * 1024 * 1024
                    and 0 < memory["outstanding_bytes"] <= memory["peak_bytes"] <= 32 * 1024 * 1024
                    and memory["peak_bytes"] <= memory["cumulative_bytes"] <= 64 * 1024 * 1024
                    and 0 < memory["allocations"] <= memory["cumulative_bytes"],
                    "Actual worker System counters/profile bounds differ")
        for before, after in zip(reports, reports[1:]):
            require(after["opcodes_remaining"] <= before["opcodes_remaining"]
                    and all(after["worker_memory"][key] >= before["worker_memory"][key]
                            for key in ("peak_bytes", "cumulative_bytes", "allocations")),
                    "A retained realm renewed cumulative worker resources")
        rows.append({"url": full_url, "loaded": loaded[0], "work": [list(item) for item in expected_work],
                     "task_acknowledgments": tasks, "event_acknowledgments": events,
                     "reports": reports})
    return {"realms": rows, "reports": 14, "lifecycle_tasks": 8,
            "native_and_cdp_click_acknowledgments": 2, "script_source_bytes": 0}


def check_http(log):
    requests = Counter(line for line in log.splitlines() if re.match(r"^\d{3} ", line))
    expected = Counter({"200 OK /": 1, "200 OK /physical-justify": 2,
                        "200 OK /physical-justify-native-hit": 1,
                        "200 OK /physical-justify-cdp-hit": 1})
    require(requests == expected, f"Unexpected/missing exact five HTTP receipts: {dict(requests)}")
    return dict(requests)


def main():
    args = parse_args()
    inputs = source_inputs()
    binary = register_input(inputs, args.binary, args.expected_sha, EXECUTABLE_LIMIT, True)
    client = register_input(inputs, args.client, args.client_sha, EXECUTABLE_LIMIT, True)
    server_exe = register_input(inputs, args.server, args.server_sha, EXECUTABLE_LIMIT, True)
    font = register_input(inputs, args.font, args.font_sha, FONT_LIMIT)
    for tool in ("Xvfb", "xauth"):
        resolved = shutil.which(tool, path="/usr/bin:/bin")
        require(resolved is not None, f"Missing private-display acceptance tool: {tool}")
        path = pinned_path(resolved, EXECUTABLE_LIMIT, True)
        inputs[str(path)] = {"sha256": digest(path, EXECUTABLE_LIMIT), "limit": EXECUTABLE_LIMIT}
    owned = load_owned()
    require(owned.ACTIVE_SECONDS == 150 and owned.COMMAND_SECONDS == 40
            and owned.LOG_LIMIT == LOG_LIMIT and owned.PNG_LIMIT == PNG_LIMIT
            and owned.EVIDENCE_LIMIT == 64 * 1024 * 1024,
            "Imported owner bounds differ from reviewed physical proof")

    class PhysicalRun(owned.Run):
        def command(self, command, log, timeout=40):
            self.guard()
            # Every new client PNG is admitted during its producer command,
            # not merely admitted by the later whole-root watchdog/readback.
            artifacts = {self.root / "frames" / name: PNG_LIMIT
                         for name in (*PNG_NAMES, FAILURE_PNG)}
            result = owned.control.run_bounded(
                list(map(str, command)), cwd=REPO, env=self.env, log_path=self.root / log,
                timeout=timeout, global_deadline=self.deadline, max_output_bytes=LOG_LIMIT,
                artifact_limits=artifacts, cancel_event=self.cancel)
            argv = list(map(str, command))
            if Path(argv[0]).name == "xauth":
                argv[-1] = "<private ephemeral cookie>"
            self.commands.append({"argv": argv, "log": log, **result})
            if result.get("cleanup_succeeded") is not True:
                self.cleanup_confirmed = False
                self.cancel.set()
            require(result.get("status") == "OK" and result.get("returncode") == 0
                    and result.get("completed") is True and result.get("cleanup_succeeded") is True,
                    f"{log}: bounded command failed: {result}")
            self.guard()
            return owned.bounded_text(self.root / log)

    temporary = REPO / "tmp"
    temporary.mkdir(exist_ok=True)
    require(temporary.is_dir() and not temporary.is_symlink(), "Repository tmp must be a nonsymlink directory")
    root = Path(tempfile.mkdtemp(prefix="physical-justify-smoke-", dir=temporary))
    print(f"PHYSICAL_JUSTIFY_ARTIFACTS {root}", flush=True)
    env = private_environment(root, font)
    run = PhysicalRun(root, env)
    report = {"schema_version": 1, "status": "ERROR", "scope":
              "authored physical-justify native X11 and external-CDP fixture; not WPT, Playwright or live-site compatibility",
              "binary": str(binary), "binary_sha256": args.expected_sha,
              "expected_version": args.expected_version, "expected_commit": args.expected_commit,
              "input_sha256": inputs, "commands": run.commands, "handoff_failures": run.handoff_failures,
              "limits": {"active_seconds": 150, "command_seconds": 40, "client_seconds": CLIENT_SECONDS,
                         "log_bytes": LOG_LIMIT, "png_bytes": PNG_LIMIT,
                         "evidence_bytes": 64 * 1024 * 1024, "evidence_entries": 256},
              "resource_evidence": "Real script-free lifecycle/System receipts retained; hidden native rendering diagnostics are not exhaustively exposed",
              "cleanup": {"confirmed": False}}
    error = None
    try:
        report["identity_before"] = check_identity(run, binary, args, "before")
        server = owned.start_journey(run, server_exe)
        owned.start_display(run)
        browser, endpoint = start_browser(owned, run, binary)
        report["owned_browser"] = {"pid": browser.pid, "window": int(endpoint[2]), "cdp_port": endpoint[1]}
        client_log = run.command(
            [client, endpoint[0], URL, endpoint[2], root / "browser.log", root / "frames"],
            "client.log", timeout=CLIENT_SECONDS)
        require(client_log.splitlines().count("PHYSICAL_JUSTIFY_SMOKE_OK") == 1,
                "Client did not complete its exact literal native/CDP pixel/hit proof")
        require(browser.poll() is None and owned.services.listener_owned(browser.pid, endpoint[1]),
                "Browser exited or lost its CDP listener before intentional shutdown")
        browser.stop()
        report["lifecycle"] = check_script_free_lifecycle(owned.bounded_text(root / "browser.log"))
        for name in PNG_NAMES:
            owned.png(root / "frames" / name)
        require(sorted(path.name for path in (root / "frames").iterdir()) == sorted(PNG_NAMES),
                "Client artifact roster differs from its exact four PNGs")
        require(server.poll() is None and owned.services.listener_owned(server.pid, 7878),
                "Lost ownership of exact journey fixture listener")
        server.stop()
        report["http_requests"] = check_http(owned.bounded_text(root / "journey-server.log"))
        report["identity_after"] = check_identity(run, binary, args, "after")
        unchanged(inputs)
        run.guard()
        report["measurement_complete"] = True
    except BaseException as failure:
        error = failure
        report["error"] = f"{type(failure).__name__}: {failure}"
    finally:
        try:
            run.close()
            report["cleanup"] = {"confirmed": True, "watchdog_joined": not run.monitor.is_alive(),
                                 "persistent_leaders": len(run.children),
                                 "returncodes": [item.process.returncode for item in run.children],
                                 "completed_command_groups": len(run.commands),
                                 "scope": "owned groups terminated before direct leaders reaped; no independent descendant census or individual grandchild reap claim"}
        except BaseException as failure:
            report["cleanup"] = {"confirmed": False, "error": f"{type(failure).__name__}: {failure}"}
            if error is None:
                error = failure
        try:
            require(run.failure is None and not run.monitor.is_alive(), run.failure or "Watchdog did not join")
            require(run.deferred_signal is None, "Deferred signal interrupted acceptance")
            require(time.monotonic() < run.deadline, "Acceptance exceeded its original active deadline")
            unchanged(inputs)
            report["inputs_unchanged"] = True
            failure_png = root / "frames" / FAILURE_PNG
            if failure_png.exists() or failure_png.is_symlink():
                require(error is not None, "Failure-only frame is not permitted in a positive proof")
                require(report["cleanup"]["confirmed"] is True,
                        "Cannot inspect producer failure evidence with uncertain cleanup")
                body = owned.control.read_artifact(failure_png, limit=PNG_LIMIT)
                require(body.startswith(b"\x89PNG\r\n\x1a\n"), "Invalid retained failure PNG")
                report["failure_evidence"] = {
                    "native_frame": str(failure_png), "bytes": len(body),
                    "sha256": hashlib.sha256(body).hexdigest(),
                    "scope": "optional last actual lifecycle/title-correlated frame and bounded client-log probes; original client error/timeout remains ERROR, not an independent pixel-regression verdict"}
            report["evidence_bytes_before_report"] = owned.evidence_size(root)
        except BaseException as failure:
            report["final_error"] = f"{type(failure).__name__}: {failure}"
            if error is None:
                error = failure
        report["status"] = "PASS" if error is None and report.get("measurement_complete") is True and report["cleanup"]["confirmed"] else "ERROR"
        body = (json.dumps(report, indent=2) + "\n").encode()
        require(len(body) <= LOG_LIMIT, "Physical proof report exceeded 1 MiB")
        with (root / "report.json").open("xb") as output:
            output.write(body)
    if error is not None:
        raise error
    try:
        require(report["status"] == "PASS" and run.failure is None
                and not run.monitor.is_alive() and run.deferred_signal is None
                and time.monotonic() < run.deadline,
                "Final publication deadline/certainty check failed")
        print(f"PHYSICAL_JUSTIFY_SMOKE_OK {args.expected_sha} {root}", flush=True)
        require(run.failure is None and time.monotonic() < run.deadline,
                "Final stdout publication exceeded its original deadline")
    except BaseException as failure:
        # A late interrupted/failed stdout is not an accepted receipt even if
        # all measurements and cleanup had already finished. Rewrite only this
        # owned report, retaining the genuine measurement/command records.
        report["status"] = "ERROR"
        report["publication_error"] = f"{type(failure).__name__}: {failure}"
        body = (json.dumps(report, indent=2) + "\n").encode()
        require(len(body) <= LOG_LIMIT, "Failed-publication report exceeded 1 MiB")
        with (root / "report.json").open("wb") as output:
            output.write(body)
        raise


if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise KeyboardInterrupt("Physical proof interrupted; cleaning exact owned groups")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    main()
