#!/usr/bin/env python3
"""Authored supplied-binary Runtime/native/CDP proof; not WPT or Playwright."""

# Root must read/freeze the new client before imports, output or process launch.
SOURCE_PINS = {
    "examples/runtime_smoke.rs": "9b285ad21ecde1d44926cb6a205e770e6437e53d7831f28cb9f01d219af22aff",
    "examples/journey_server.rs": "eeee15c2d2d8f69a4a2eaee7f02ced00e800c96359eb061a01f31b8396613639",
    "tests/fixtures/script/events.html": "d3d5e6806c67a7270998a83e9b622ae0fc64c62405327061f16f1e9b22354c9e",
    "tests/fixtures/journey/home.html": "34e7877af1c3d645c131d96a3baed20edfce2998395ae1e7bc46c8674e509cb0",
    "tools/cssom-methods-smoke.py": "def29cb882e9987e91854db2ce9ef8abc56b223ecf48d0cc06a8bb6838c3bfdf",
    "tools/message-services-smoke.py": "367314ba089e4c9e03b6794782c600b4239d759a9b06516914cd115363eddedd",
    "tools/page-services-smoke.py": "7560bdcea6187df67a524669a89fe4c80aff5f2f7628cb7583b00a18f68ded04",
    "tools/wpt/process_control.py": "eaec5e966841e547cfc92ccd785bf356e519aa16b9dc19e26ac40d4aae120c7d",
}
for relative, pin in SOURCE_PINS.items():
    if len(pin) != 64 or any(c not in "0123456789abcdef" for c in pin):
        raise RuntimeError("Unfrozen Runtime proof input: " + relative)

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
URL = "http://127.0.0.1:7878/script-events"
DESTINATION = "http://127.0.0.1:7878/event-destination?proof=clicked"
LOG_LIMIT = 1024 * 1024
PNG_LIMIT = 8 * 1024 * 1024
PNG_NAMES = ("native-blue.png", "cdp-blue.png", "native-brown.png", "cdp-brown.png",
             "native-destination.png", "cdp-destination.png")
FAILURE_PNG = "failure-native.png"
LOOP_SOURCE = "function(){document.getElementById('query').value='stale-worker';let n=0;for(let i=0;i<10000;i++){n++;}return n;}"
ORDERING_METHOD = "four-frame-single-flush"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def source_bytes(path):
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and not stat.S_ISLNK(before.st_mode)
            and 0 < before.st_size <= LOG_LIMIT, "Unsafe source input: " + str(path))
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    try:
        opened = os.fstat(descriptor)
        require((opened.st_dev, opened.st_ino, opened.st_size)
                == (before.st_dev, before.st_ino, before.st_size), "Source changed before read")
        body = os.read(descriptor, LOG_LIMIT + 1)
        after = os.fstat(descriptor)
        require(len(body) == before.st_size and after.st_size == before.st_size
                and after.st_mtime_ns == opened.st_mtime_ns, "Source changed during read")
        return body
    finally:
        os.close(descriptor)


def load_common():
    for relative, expected in SOURCE_PINS.items():
        require(hashlib.sha256(source_bytes(REPO / relative)).hexdigest() == expected,
                "Frozen source changed: " + relative)
    spec = importlib.util.spec_from_file_location("runtime_common", REPO / "tools/cssom-methods-smoke.py")
    common = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(common)
    return common


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("binary", "client", "server", "font"):
        parser.add_argument("--" + name, type=Path, required=True)
    for name in ("expected-sha", "expected-version", "expected-commit", "client-sha", "server-sha", "font-sha"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    for name in ("expected_sha", "client_sha", "server_sha", "font_sha"):
        require(re.fullmatch(r"[0-9a-f]{64}", getattr(args, name)), "Invalid explicit " + name)
    require(re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", args.expected_version), "Explicit version required")
    require(re.fullmatch(r"(?:[0-9a-f]{12}|[0-9a-f]{40})(?:-dirty)?", args.expected_commit),
            "Explicit exact build commit required")
    return args


def start_browser(owned, run, binary):
    browser = run.start([binary, URL, "--enable-scripts", "--remote-debugging-port=0"], "browser.log")
    endpoint = []

    def discover():
        text = owned.live_log(run.root / "browser.log")
        windows = re.findall(r"^WINDOW id=([1-9][0-9]*) pid=([1-9][0-9]*)$", text, re.M)
        require(len(windows) <= 1, "Unexpected extra native window")
        found = re.search(r"^CDP listening on (ws://127\.0\.0\.1:([0-9]+)/devtools/browser/browser-1) \(experimental subset; local clients control this page\)$", text, re.M)
        if not found or not windows:
            return False
        require(int(windows[0][1]) == browser.pid, "Window PID differs from supplied browser")
        port = int(found[2])
        require(0 < port < 65536 and port != 7878, "Invalid owned CDP port")
        if not owned.services.listener_owned(browser.pid, port):
            return False
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
        try:
            connection.request("GET", "/json/version")
            response = connection.getresponse()
            body = response.read(LOG_LIMIT + 1)
            require(response.status == 200 and len(body) <= LOG_LIMIT
                    and json.loads(body).get("webSocketDebuggerUrl") == found[1], "Owned CDP identity differs")
            connection.close()
            connection = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
            connection.request("GET", "/json/list")
            response = connection.getresponse()
            body = response.read(LOG_LIMIT + 1)
            require(response.status == 200 and len(body) <= LOG_LIMIT, "Owned page discovery failed")
            pages = json.loads(body)
            page = f"ws://127.0.0.1:{port}/devtools/page/page-1"
            require(type(pages) is list and len(pages) == 1 and pages[0].get("id") == "page-1"
                    and pages[0].get("webSocketDebuggerUrl") == page, "Expected exactly actual page-1")
            endpoint[:] = [page, port, windows[0][0]]
            return True
        except OSError:
            return False
        finally:
            connection.close()

    owned.wait_for(run, browser, discover, "owned native/loopback discovery")
    return browser, endpoint


def check_ordering_batch(records):
    starts = [index for index, row in enumerate(records)
              if row.get("protocol") == "send" and row.get("message", {}).get("method") == "Runtime.callFunctionOn"
              and row["message"].get("params", {}).get("functionDeclaration") == LOOP_SOURCE]
    require(len(starts) == 1, "Expected exactly one fixed unchanged-loop ordering batch")
    start = starts[0]
    batch = records[start:start+4]
    require(len(batch) == 4 and all(row.get("protocol") == "send" and row.get("route") == "direct"
            for row in batch), "Four direct sends must be consecutive before any batch receive")
    contexts = [row["message"]["params"]["context"]["id"] for row in records[:start]
                if row.get("protocol") == "receive" and row.get("route") == "direct"
                and row.get("message", {}).get("method") == "Runtime.executionContextCreated"]
    require(len(contexts) == 1 and type(contexts[0]) is int and 0 < contexts[0] <= 2**31-1,
            "Ordering batch lacks the unique actual direct execution context")
    context = contexts[0]
    ids = [row["message"].get("id") for row in batch]
    require(all(type(item) is int for item in ids) and 0 < ids[0] <= 253
            and ids == list(range(ids[0], ids[0]+4)), "Batch IDs must be four fresh continuous bounded IDs")
    expected = [
        {"id": ids[0], "method": "Runtime.callFunctionOn", "params": {"executionContextId": context,
            "functionDeclaration": LOOP_SOURCE, "arguments": [], "returnByValue": False}},
        {"id": ids[1], "method": "Runtime.evaluate", "params": {"expression": "1", "contextId": context}},
        {"id": ids[2], "method": "Input.insertText", "params": {"text": "-newer"}},
        {"id": ids[3], "method": "Runtime.evaluate", "params": {"expression": "1", "contextId": context}},
    ]
    require([row["message"] for row in batch] == expected, "Batch exact methods/params/no-session binding differs")
    require(sum(len(json.dumps(message, separators=(",", ":"), ensure_ascii=False).encode())
                for message in expected) + 4*14 < 64*1024, "Serialized four-frame aggregate exceeds original64KiB")
    received = [(index, row["message"]) for index, row in enumerate(records)
                if row.get("protocol") == "receive" and row.get("route") == "direct"
                and row.get("message", {}).get("id") in ids]
    require(len(received) == 4 and all(index > start+3 and "sessionId" not in message
            for index, message in received) and [message["id"] for _, message in received]
            == [ids[1], ids[2], ids[3], ids[0]], "Four unique actual replies must order busy/input/busy before Call")
    barriers = [(index, row) for index, row in enumerate(records) if row.get("phase") == "pending-barrier"]
    require(len(barriers) == 2, "Both exact measured barrier phases required")
    for ordinal, reply_index, label in ((0, 0, "before-input"), (1, 2, "after-input")):
        index, message = received[reply_index]
        require(set(message) == {"id", "error"} and type(message["error"].get("code")) is int
                and message["error"]["code"] == -32000
                and type(message["error"].get("message")) is str and "pending" in message["error"]["message"],
                "Actual batch barrier must be the pending protocol rejection")
        require(barriers[ordinal] == (index+1, {"phase": "pending-barrier", "position": label,
                "request_id": message["id"], "call_id": ids[0], "error": message["error"],
                "fixed_iterations": 10000, "ordering_method": ORDERING_METHOD}),
                "Barrier phase must immediately follow its actual rejection")
    require(received[1][1] == {"id": ids[2], "result": {}}, "Input ACK must be exact and precede Call reply")
    require(received[3][1] == {"id": ids[0], "result": {"result": {"type": "number", "value": 10000.0}}},
            "Final batch reply must be the actual unchanged finite Call result")
    following = [(index, row["message"]) for index, row in enumerate(records)
                 if index > received[3][0] and row.get("protocol") == "send"]
    require(following and following[0][1] == {"id": ids[0]+4, "method": "Runtime.evaluate", "params": {
            "expression": "document.getElementById('query').value", "contextId": context, "returnByValue": False}}
            and records[following[0][0]]["route"] == "direct", "Actual retained-value read must follow the batch")
    value_replies = [(index, row["message"]) for index, row in enumerate(records)
                     if row.get("protocol") == "receive" and row.get("route") == "direct"
                     and row.get("message", {}).get("id") == ids[0]+4]
    require(len(value_replies) == 1 and value_replies[0][0] > following[0][0]
            and value_replies[0][1] == {"id": ids[0]+4, "result": {"result": {
                "type": "string", "value": "native-edit-newer"}}}, "Actual newer value result differs")
    newer = [(index, row) for index, row in enumerate(records) if row.get("phase") == "newer-edit"]
    require(len(newer) == 1 and newer[0][0] == value_replies[0][0]+1
            and newer[0][1].get("ordering_method") == ORDERING_METHOD,
            "Newer-edit phase must immediately follow its bound actual value result")
    return {"method": ORDERING_METHOD, "route": "direct", "ids": ids,
            "response_ids": [message["id"] for _, message in received]}


def check_protocol(text):
    require(text.splitlines().count("RUNTIME_SMOKE_OK") == 1, "Missing final literal Runtime proof marker")
    records = [json.loads(line) for line in text.splitlines() if line.startswith("{")]
    require(len(records) <= 4096, "Client JSONL count exceeded")
    phases = [row for row in records if row.get("phase")]
    summaries = [row for row in phases if row["phase"] == "runtime-proof"]
    require(len(summaries) == 1, "Missing/duplicate proof summary")
    summary = summaries[0]
    require(summary.get("groups") == 8 and summary.get("images") == 6
            and summary.get("fixed_loop_iterations") == 10000 and summary.get("target_moves") is False
            and summary.get("ordering_method") == ORDERING_METHOD
            and summary.get("newer_field_glyphs_automatically_verified") is False
            and summary.get("newer_field_images") == ["native-brown.png", "cdp-brown.png"]
            and all(summary.get(key) is True for key in ("native_input", "native_and_cdp_paint",
                    "real_worker_closure", "measured_newer_edit", "direct_and_flattened")),
            "Incomplete core Runtime proof or substituted target-move claim")
    require([row["position"] for row in phases if row["phase"] == "pending-barrier"]
            == ["before-input", "after-input"], "Concurrent edit lacked both measured barriers")
    require([row["label"] for row in phases if row["phase"] == "paint"] == ["blue", "brown"],
            "Missing literal native/CDP paint phases")
    require([row.get("navigation_live_handle_retained") for row in phases if row["phase"] == "routing-release"]
            == [True], "Navigation must retain a live handle distinct from release negatives")
    require([row["phase"] for row in phases] == ["contexts", "native-input", "closures-handles", "paint",
            "exception-effects", "routing-release", "pending-barrier", "pending-barrier",
            "newer-edit", "paint", "navigation", "runtime-proof"], "Client phase inventory/order differs")
    ordering = check_ordering_batch(records)
    sent, replies, compiled = {}, {}, [[], []]
    navigation = 0
    for row in records:
        if row.get("protocol") == "send":
            message = row["message"]
            key = row["route"], message["id"]
            require(key not in sent, "Duplicate client command identity")
            if message["method"] == "Page.navigate":
                navigation += 1
                require(message["params"] == {"url": DESTINATION}, "Wrong explicit selected navigation")
            sent[key] = (message, navigation)
        elif row.get("protocol") == "receive" and "id" in row["message"]:
            key = row["route"], row["message"]["id"]
            require(key in sent and key not in replies, "Unmatched/duplicate retained response")
            message = row["message"]
            replies[key] = message
            request, section = sent[key]
            require(request.get("sessionId") == message.get("sessionId"), "Flattened response binding changed")
            if "result" in message and request["method"] in ("Runtime.evaluate", "Runtime.callFunctionOn"):
                params = request["params"]
                source = params.get("expression", params.get("functionDeclaration"))
                require(type(source) is str and 0 < len(source.encode()) <= 16 * 1024, "Source admission differs")
                body = source if request["method"] == "Runtime.evaluate" else "(" + source + "\n)"
                compiled[section].append({"route": key[0], "id": key[1], "bytes": len(body.encode()),
                                          "sha256": hashlib.sha256(body.encode()).hexdigest()})
    require(navigation == 1 and sent.keys() == replies.keys(), "Missing response or navigation identity")
    require([len(rows) for rows in compiled] == [14, 1], "Actual successful source/compile inventory differs")
    return {"phases": phases, "commands": len(sent), "compiled_sources": compiled, "summary": summary,
            "ordering": ordering}


def check_resources(log, compiled):
    for marker in ("SCRIPT_PARTIAL", "SCRIPT_DIAGNOSTIC", "SCRIPT_ERROR", "SCRIPT_REJECTED",
                   "SCRIPT_SESSION_CLOSED", "SCRIPT_SHUTDOWN_ERROR", "LOAD_ERROR", "RESOURCE_WARNING",
                   "RESOURCE_DIAGNOSTIC", "JOURNEY_INCOMPLETE"):
        require(marker not in log, "Unexpected native/script/resource failure: " + marker)
    chunks = re.split(r"^NAVIGATE GET ", log, flags=re.M)
    require(len(chunks) == 3 and [chunk.splitlines()[0] for chunk in chunks[1:]] == [URL, DESTINATION],
            "Wrong native navigation inventory")
    source = source_bytes(REPO / "tests/fixtures/script/events.html").split(b"<script>", 1)[1].split(b"</script>", 1)[0]
    result = []
    for index, chunk in enumerate(chunks[1:]):
        rows = [json.loads(line[11:]) for line in chunk.splitlines() if line.startswith("SCRIPT_BOA ")]
        tasks = [line for line in chunk.splitlines() if line.startswith("PAGE_TASK_APPLIED ")]
        events = [line for line in chunk.splitlines() if line.startswith("PAGE_EVENT_APPLIED ")]
        auto = [line for line in chunk.splitlines() if line.startswith("PAGE_AUTOMATION_APPLIED ")]
        require(tasks == ["PAGE_TASK_APPLIED sequence=1 revision=1", "PAGE_TASK_APPLIED sequence=2 revision=2"],
                "Missing/repeated actual mandatory lifecycle tasks")
        expected_events = ["PAGE_EVENT_APPLIED sequence=3 revision=3 click_canceled=Some(false) submit_canceled=None"] if index == 0 else []
        require(events == expected_events, "Native input activation inventory differs")
        require(len(auto) == 20 if index == 0 else 1 <= len(auto) <= 2,
                "Unexpected Automation/optional final route-cleanup transaction inventory")
        first = 4 if index == 0 else 3
        require(auto == [f"PAGE_AUTOMATION_APPLIED sequence={seq} revision={seq} terminal=false"
                         for seq in range(first, first + len(auto))], "Automation sequence/revision/continuation differs")
        require(len(rows) == 3 + len(events) + len(auto), "Worker report/accepted completion inventory differs")
        completes = [line for line in chunk.splitlines() if line.startswith("SCRIPT_COMPLETE ")]
        require(completes == (["SCRIPT_COMPLETE executed=1"] if index == 0 else ["SCRIPT_COMPLETE executed=0"])
                + ["SCRIPT_COMPLETE executed=0"] * (len(rows)-1), "Worker startup/later execution counts differ")
        for position, row in enumerate(rows):
            fields = ("opcodes_remaining", "jobs_admitted", "jobs_executed", "pending_jobs", "source_bytes", "source_admissions")
            require(type(row) is dict and all(type(row.get(key)) is int for key in fields)
                    and row.get("profile") == "boa-page-process-v1" and row.get("fatal_reason", "missing") is None
                    and 0 < row["opcodes_remaining"] <= 1_000_000
                    and row["jobs_admitted"] == 2 and row["jobs_executed"] == min(position, 2)
                    and row["pending_jobs"] == max(2-position, 0), "Wrong/fatal or renewed lifecycle/work accounting")
            memory = row.get("worker_memory") or {}
            keys = ("outstanding_limit_bytes", "cumulative_limit_bytes", "outstanding_bytes", "peak_bytes", "cumulative_bytes", "allocations")
            require(all(type(memory.get(key)) is int for key in keys)
                    and memory["outstanding_limit_bytes"] == 32*1024*1024
                    and memory["cumulative_limit_bytes"] == 64*1024*1024
                    and 0 <= memory["outstanding_bytes"] <= memory["peak_bytes"] <= memory["outstanding_limit_bytes"]
                    and memory["peak_bytes"] <= memory["cumulative_bytes"] <= memory["cumulative_limit_bytes"],
                    "Missing/invalid actual requested-System report")
            if position:
                before = rows[position-1]
                require(row["opcodes_remaining"] <= before["opcodes_remaining"]
                        and row["source_admissions"] >= before["source_admissions"]
                        and row["source_bytes"] >= before["source_bytes"]
                        and all(memory[key] >= before["worker_memory"][key] for key in ("peak_bytes", "cumulative_bytes", "allocations")),
                        "Cumulative realm accounting moved backward")
        require(all(row["source_admissions"] == (1 if index == 0 else 0)
                    and row["source_bytes"] == (len(source) if index == 0 else 0) for row in rows[:3]),
                "Initial source admission/bytes differ from the original fixture")
        require(rows[-1]["source_admissions"] == (1 if index == 0 else 0) + len(compiled[index])
                and rows[-1]["source_bytes"] == (len(source) if index == 0 else 0)
                + sum(item["bytes"] for item in compiled[index]), "Runtime compilation/source bytes not bound to actual sent source")
        result.append({"url": (URL, DESTINATION)[index], "reports": rows,
                       "tasks": tasks, "events": events, "automation": auto,
                       "work": [[row["jobs_admitted"], row["jobs_executed"], row["pending_jobs"]] for row in rows]})
    return result


def main():
    common = load_common()
    args = arguments()
    inputs = {}
    for relative, pin in SOURCE_PINS.items():
        common.register_input(inputs, REPO / relative, pin, LOG_LIMIT)
    own = common.pinned_path(__file__, LOG_LIMIT)
    inputs[str(own)] = {"sha256": common.digest(own, LOG_LIMIT), "limit": LOG_LIMIT}
    binary = common.register_input(inputs, args.binary, args.expected_sha, 64*1024*1024, True)
    client = common.register_input(inputs, args.client, args.client_sha, 64*1024*1024, True)
    server_exe = common.register_input(inputs, args.server, args.server_sha, 64*1024*1024, True)
    font = common.register_input(inputs, args.font, args.font_sha, 16*1024*1024)
    for name in ("Xvfb", "xauth"):
        path = shutil.which(name, path="/usr/bin:/bin")
        require(path is not None, "Missing private-display tool: " + name)
        path = common.pinned_path(path, 64*1024*1024, True)
        inputs[str(path)] = {"sha256": common.digest(path, 64*1024*1024), "limit": 64*1024*1024}
    owned = common.load_owned()
    require(owned.ACTIVE_SECONDS == 150 and owned.COMMAND_SECONDS == 40 and owned.LOG_LIMIT == LOG_LIMIT
            and owned.PNG_LIMIT == PNG_LIMIT and owned.EVIDENCE_LIMIT == 64*1024*1024, "Imported owner bounds differ")

    class RuntimeRun(owned.Run):
        def command(self, command, log, timeout=40):
            self.guard()
            artifacts = {self.root / "frames" / name: PNG_LIMIT for name in (*PNG_NAMES, FAILURE_PNG)}
            result = owned.control.run_bounded(list(map(str, command)), cwd=REPO, env=self.env,
                log_path=self.root / log, timeout=timeout, global_deadline=self.deadline,
                max_output_bytes=LOG_LIMIT, artifact_limits=artifacts, cancel_event=self.cancel)
            argv = list(map(str, command))
            if Path(argv[0]).name == "xauth":
                argv[-1] = "<private ephemeral cookie>"
            self.commands.append({"argv": argv, "log": log, **result})
            if result.get("cleanup_succeeded") is not True:
                self.cleanup_confirmed = False
                self.cancel.set()
            require(result.get("completed") is True and result.get("cleanup_succeeded") is True
                    and result.get("status") == "OK" and result.get("returncode") == 0,
                    f"{log}: bounded command failed: {result}")
            self.guard()
            return owned.bounded_text(self.root / log)

    temporary = REPO / "tmp"
    temporary.mkdir(exist_ok=True)
    require(temporary.is_dir() and not temporary.is_symlink(), "Unsafe repository tmp")
    root = Path(tempfile.mkdtemp(prefix="runtime-smoke-", dir=temporary))
    print("RUNTIME_ARTIFACTS " + str(root), flush=True)
    env = common.private_environment(root, font)
    preferences = Path(env["XDG_CONFIG_HOME"]) / "mgbrowser/settings.json"
    inputs[str(preferences)] = {"sha256": common.digest(preferences, LOG_LIMIT), "limit": LOG_LIMIT}
    run = RuntimeRun(root, env)
    report = {"schema_version": 1, "status": "ERROR", "scope": "authored supplied-binary Runtime/native X11/external-CDP core; not WPT, Playwright, full Runtime or independent hardware input",
        "binary": str(binary), "binary_sha256": args.expected_sha, "expected_version": args.expected_version,
        "expected_commit": args.expected_commit, "input_sha256": inputs, "commands": run.commands,
        "handoff_failures": run.handoff_failures, "target_moves": False,
        "manual_visual_review_required": "Read actual newer field glyphs in native-brown.png and cdp-brown.png; automatic literals cover panel colors, not the field string",
        "limits": {"active_seconds": 150, "command_seconds": 40, "client_work_seconds": 28, "client_whole_seconds": 30,
            "log_bytes": LOG_LIMIT, "png_bytes": PNG_LIMIT, "evidence_bytes": 64*1024*1024, "evidence_entries": 256},
        "cleanup": {"confirmed": False}}
    error = None
    try:
        report["identity_before"] = common.check_identity(run, binary, args, "before")
        server = owned.start_journey(run, server_exe)
        owned.start_display(run)
        browser, endpoint = start_browser(owned, run, binary)
        report["owned_browser"] = {"pid": browser.pid, "window": int(endpoint[2]), "cdp_port": endpoint[1]}
        client_log = run.command([client, endpoint[0], endpoint[2], root / "browser.log", root / "frames"], "client.log", timeout=30)
        report["protocol"] = check_protocol(client_log)
        require(browser.poll() is None and owned.services.listener_owned(browser.pid, endpoint[1]), "Browser lost ownership before shutdown")
        browser.stop()
        browser_log = owned.bounded_text(root / "browser.log")
        require(re.findall(r"^DISPLAY_SCALE system=[0-9]+% effective=([0-9]+)%$", browser_log, re.M) == ["100"], "Unexpected native scale")
        report["resource_realms"] = check_resources(browser_log, report["protocol"]["compiled_sources"])
        report["images"] = []
        for name in PNG_NAMES:
            path = root / "frames" / name
            owned.png(path)
            body = owned.control.read_artifact(path, limit=PNG_LIMIT)
            report["images"].append({"path": str(path), "bytes": len(body), "sha256": hashlib.sha256(body).hexdigest()})
        require(sorted(path.name for path in (root / "frames").iterdir()) == sorted(PNG_NAMES), "Positive image roster differs")
        require(server.poll() is None and owned.services.listener_owned(server.pid, 7878), "Journey listener ownership lost")
        server.stop()
        requests = Counter(line for line in owned.bounded_text(root / "journey-server.log").splitlines() if re.match(r"^\d{3} ", line))
        require(requests == Counter({"200 OK /": 1, "200 OK /script-events": 1,
                "200 OK /event-destination": 1}), "Exact source/destination/trap HTTP inventory differs")
        report["http_requests"] = dict(requests)
        report["http_log_scope"] = "Shared journey server logs paths; client navigation/frame/context receipts independently bind exact proof=clicked query"
        report["identity_after"] = common.check_identity(run, binary, args, "after")
        common.unchanged(inputs)
        run.guard()
        report["measurement_complete"] = True
    except BaseException as failure:
        error = failure
        report["error"] = f"{type(failure).__name__}: {failure}"
    finally:
        try:
            run.close()
            report["cleanup"] = {"confirmed": True, "watchdog_joined": not run.monitor.is_alive(),
                "persistent_leaders": len(run.children), "returncodes": [item.process.returncode for item in run.children],
                "completed_command_groups": len(run.commands),
                "scope": "owned groups stopped before direct leaders reaped; no independent descendant census or individual grandchild reap proof"}
        except BaseException as failure:
            report["cleanup"] = {"confirmed": False, "error": f"{type(failure).__name__}: {failure}"}
            if error is None:
                error = failure
        try:
            require(run.failure is None and not run.monitor.is_alive(), run.failure or "Watchdog did not join")
            require(run.deferred_signal is None and time.monotonic() < run.deadline, "Late signal/deadline failure")
            common.unchanged(inputs)
            report["inputs_unchanged"] = True
            failure_png = root / "frames" / FAILURE_PNG
            if failure_png.exists() or failure_png.is_symlink():
                require(error is not None and report["cleanup"]["confirmed"] is True, "Unsafe/unexpected failure-only artifact")
                body = owned.control.read_artifact(failure_png, limit=PNG_LIMIT)
                require(body.startswith(b"\x89PNG\r\n\x1a\n"), "Failure PNG header differs")
                report["failure_evidence"] = {"path": str(failure_png), "bytes": len(body), "sha256": hashlib.sha256(body).hexdigest(),
                    "scope": "last observed mismatching native frame; original error retained, not independent regression classification"}
            report["evidence_bytes_before_report"] = owned.evidence_size(root)
        except BaseException as failure:
            report["final_error"] = f"{type(failure).__name__}: {failure}"
            if error is None:
                error = failure
        report["status"] = "PASS" if error is None and report.get("measurement_complete") is True and report["cleanup"]["confirmed"] else "ERROR"
        body = (json.dumps(report, indent=2) + "\n").encode()
        require(len(body) <= LOG_LIMIT, "Runtime receipt exceeds1MiB")
        with (root / "report.json").open("xb") as output:
            output.write(body)
        owned.evidence_size(root)
        if error is not None:
            raise error
    try:
        require(report["status"] == "PASS" and run.failure is None and not run.monitor.is_alive()
                and run.deferred_signal is None and time.monotonic() < run.deadline,
                "Final publication deadline/certainty check failed")
        print("RUNTIME_SMOKE_PASS " + str(root), flush=True)
        require(run.failure is None and time.monotonic() < run.deadline,
                "Final stdout publication exceeded its original deadline")
    except BaseException as failure:
        report["status"] = "ERROR"
        report["publication_error"] = f"{type(failure).__name__}: {failure}"
        body = (json.dumps(report, indent=2) + "\n").encode()
        require(len(body) <= LOG_LIMIT, "Failed-publication report exceeds1MiB")
        with (root / "report.json").open("wb") as output:
            output.write(body)
        raise


if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise KeyboardInterrupt("Runtime proof interrupted; cleaning exact owned groups")
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGTERM, interrupted)
    main()
