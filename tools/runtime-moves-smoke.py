#!/usr/bin/env python3
"""Separate supplied-binary Runtime native move/root proof; not WPT or Playwright."""

# Root must read/freeze the new client before imports, output or process launch.
SOURCE_PINS = {
    "examples/runtime_moves_smoke.rs": "96ae42511a87d4125ac3a121d79af806ed28987ba93d46be0ccff745a3f9cf1f",
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
URL = "http://127.0.0.1:7878/script-events?runtime-move=A"
URL_B = "http://127.0.0.1:7878/script-events?runtime-move=B"
LOG_LIMIT = 1024 * 1024
PNG_LIMIT = 8 * 1024 * 1024
PNG_NAMES = tuple(f"{kind}-{phase}.png" for phase in ("A-detached", "B-detached", "B-redocked", "A-survivor")
                  for kind in ("native", "cdp"))
FAILURE_PNG = "failure-native.png"


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


# Authored before execution, not inferred from observed reports.
ROSTER = (
    ("A", 3, "direct-A", "create-A", "CREATE_A", "A-direct", "A"),
    ("A", 4, "direct-A", "A-baseline", "INCREMENT", None, 101),
    ("A", 5, "flattened", "A-flat-baseline", "MARKER", None, "A"),
    ("B", 3, "direct-B", "create-B", "CREATE_B", "B-direct", "B_direct"),
    ("B", 4, "flattened", "create-B-flat", "CREATE_B_FLAT", "B-flat", "B_flat"),
    ("B", 5, "direct-B", "B-baseline", "INCREMENT", None, 201),
    ("B", 6, "flattened", "B-flat-baseline", "INCREMENT", None, 301),
    ("A", 6, "direct-A", "A-detached", "INCREMENT", None, 102),
    ("B", 7, "direct-B", "B-detached", "INCREMENT", None, 202),
    ("B", 8, "flattened", "B-flat-detached", "INCREMENT", None, 302),
    ("A", 7, "direct-A", "A-redocked", "INCREMENT", None, 103),
    ("B", 9, "direct-B", "B-redocked", "INCREMENT", None, 203),
    ("B", 10, "flattened", "B-flat-redocked", "INCREMENT", None, 303),
    ("A", 8, "direct-A", "A-survivor", "INCREMENT", None, 104),
    ("A", 9, "flattened", "A-flat-survivor", "MARKER", None, "A"),
)
NEGATIVES = ("foreign-owner-before", "foreign-context-before", "foreign-owner-after",
             "foreign-context-after", "closed-session", "closed-target", "closed-root-foreign")
WORK_PREFIXES = ("SCRIPT_BOA ", "SCRIPT_COMPLETE ", "PAGE_TASK_APPLIED ", "PAGE_EVENT_APPLIED ",
                 "PAGE_AUTOMATION_APPLIED ")


def target_number(target):
    require(type(target) is str and re.fullmatch(r"page-[1-9][0-9]{0,19}", target),
            "Noncanonical page target")
    number = int(target[5:])
    require(0 < number <= 2**64-1 and target == f"page-{number}", "Target exceeds canonical u64")
    return number


def target_inventory(result):
    require(type(result) is dict and type(result.get("targetInfos")) is list
            and 1 <= len(result["targetInfos"]) <= 2, "Unexpected target inventory")
    targets = {}
    for info in result["targetInfos"]:
        require(type(info) is dict, "Nonobject target metadata")
        target = info.get("targetId")
        target_number(target)
        require(target not in targets and info.get("type") == "page", "Duplicate/foreign target")
        targets[target] = info
    require("page-1" in targets and targets["page-1"].get("url") == URL, "Initial target/URL changed")
    return targets


def opened_tab(log):
    lines = [line for line in log.splitlines() if line.startswith("TAB ")]
    require(len(lines) == 1, "Expected exactly one actual native tab creation")
    match = re.fullmatch(r"TAB opened=([1-9][0-9]{0,19}) pane=([1-9][0-9]{0,19})", lines[0])
    require(match is not None, "Malformed native tab identity")
    target_number("page-" + match[2])
    return target_number("page-" + match[1])


def bind_new_target(result, native_tab):
    require(native_tab is None or (type(native_tab) is int and 0 < native_tab <= 2**64-1),
            "Invalid native tab number")
    targets = target_inventory(result)
    if len(targets) == 1:
        return None
    target = next(target for target in targets if target != "page-1")
    info = targets[target]
    require(info.get("url") == "about:blank" and info.get("attached") is False,
            "New target was not blank/unattached")
    if native_tab is None:
        return None
    require(target_number(target) == native_tab, "Discovered target differs from actual native tab")
    return target


def check_protocol(text):
    require(text.splitlines().count("RUNTIME_MOVES_SMOKE_OK") == 1, "Missing final move proof marker")
    records = [json.loads(line) for line in text.splitlines() if line.startswith("{")]
    require(len(records) <= 4096, "Client JSONL count exceeded")
    phases = [row for row in records if row.get("phase")]
    summaries = [row for row in phases if row["phase"] == "runtime-move-proof"]
    require(len(summaries) == 1, "Missing/duplicate move summary")
    summary = summaries[0]
    require(summary.get("target_moves") is True and summary.get("images") == 8
            and summary.get("real_worker_roots") == 3 and summary.get("automation_transactions") == 15
            and summary.get("worker_reports") == 21 and summary.get("lifecycle_tasks") == 4
            and summary.get("page_events") == 0, "Wrong literal move proof summary")
    structural = [row["phase"] for row in phases if row["phase"] != "log-interval"]
    require(structural == ["context-A", "context-B", "roots", "native-drag", "paint", "paint",
            "native-drag", "paint", "direct-closed", "closed-endpoint", "paint", "closed-B",
            "runtime-move-proof"], "Move phase order/inventory differs")
    b_phases = [row for row in phases if row["phase"] == "context-B"]
    require(len(b_phases) == 1, "Missing unique B context phase")
    b_target = b_phases[0]["target"]
    require(target_number(b_target) != 1, "B aliases initial A target")
    targets = {"A": "page-1", "B": b_target}
    frames = {role: f"frame-{target_number(target)}" for role, target in targets.items()}
    contexts, sessions, context_positions = {}, {}, {}
    for role, frame in frames.items():
        rows = [row for row in phases if row["phase"] == "context-" + role]
        require(len(rows) == 1, "Wrong context phase count")
        context = rows[0]["context"]
        identity = context.get("id")
        require(type(identity) is int and 0 < identity <= 2**31-1 and context == {
            "id": identity, "origin": "http://127.0.0.1:7878", "name": "",
            "uniqueId": f"mg-context-{identity}", "auxData": {"isDefault": True, "type": "default", "frameId": frame}},
            "Actual context/default/origin/frame shape differs")
        require(rows[0]["target"] == targets[role], "Wrong phase target")
        session = rows[0]["session"]
        require(type(session) is str and 0 < len(session) <= 128, "Missing bounded flattened session")
        contexts[role], sessions[role] = context, session
        context_positions[role] = next(pos for pos, row in enumerate(records)
                                       if row.get("phase") == "context-" + role)
    require(contexts["A"]["id"] != contexts["B"]["id"] and sessions["A"] != sessions["B"],
            "Distinct realms or routes alias")
    roots_row = [row for row in phases if row["phase"] == "roots"]
    require(len(roots_row) == 1, "Root inventory missing")
    roots = {name: roots_row[0][name] for name in ("A", "B_direct", "B_flat")}
    require(len(set(roots.values())) == 3, "Root identity aliases")
    owner_ids = {}
    for name, root in roots.items():
        match = re.fullmatch(r"mg:([1-9][0-9]*):([1-9][0-9]*):([1-9][0-9]*)", root)
        role = "A" if name == "A" else "B"
        require(match is not None and int(match[1]) == contexts[role]["id"]
                and 0 < int(match[2]) <= 64 and 0 < int(match[3]) <= 512, "Root context/owner/id shape differs")
        owner_ids[name] = int(match[2])
    require(owner_ids["B_direct"] != owner_ids["B_flat"], "B route owners alias")
    sources = {}
    client_source = source_bytes(REPO / "examples/runtime_moves_smoke.rs").decode()
    for name in ("CREATE_A", "CREATE_B", "CREATE_B_FLAT", "INCREMENT", "MARKER"):
        matches = re.findall(r'^const ' + name + r': &str = ("[^\n]*");$', client_source, re.M)
        require(len(matches) == 1, "Frozen source constant missing/ambiguous: " + name)
        sources[name] = json.loads(matches[0])
    sent, replies, events, positions = {}, {}, [], {}
    for position, row in enumerate(records):
        if row.get("protocol") not in ("send", "receive"):
            continue
        route, message = row["route"], row["message"]
        require(route in ("direct-A", "direct-B", "flattened") and type(message) is dict, "Foreign protocol route")
        if row["protocol"] == "send":
            key = (route, message["id"])
            require(type(message["id"]) is int and 1 <= message["id"] <= 256 and key not in sent,
                    "Invalid/repeated request identity")
            require(message["method"] in ("Runtime.enable", "Runtime.evaluate", "Runtime.callFunctionOn",
                "Page.enable", "Page.navigate", "Page.getFrameTree", "Page.captureScreenshot",
                "Target.attachToTarget", "Target.getTargets"), "Unfrozen command")
            require(len(json.dumps(message, separators=(",", ":")).encode()) <= 64*1024, "Request bound exceeded")
            sent[key] = message
            positions[key] = [position]
        elif "id" in message:
            key = (route, message["id"])
            require(key in sent and key not in replies and ("error" in message) != ("result" in message),
                    "Unmatched/repeated/ambiguous response")
            require(message.get("sessionId") == sent[key].get("sessionId"), "Response session identity differs")
            replies[key] = message
            positions[key].append(position)
        else:
            require(type(message.get("method")) is str, "Non-event frame")
            events.append((position, route, message))
    require(sent.keys() == replies.keys(), "Missing response")
    received = [row for row in records if row.get("protocol") == "receive"]
    require(type(summary.get("shared_messages")) is int and summary["shared_messages"] == len(received)
            and 0 < summary["shared_messages"] <= 4096
            and type(summary.get("shared_reply_bytes")) is int
            and 0 < summary["shared_reply_bytes"] <= 24*1024*1024, "Shared actual receive accounting differs")
    for route in ("direct-A", "direct-B", "flattened"):
        ids = [key[1] for key in sent if key[0] == route]
        require(ids == list(range(1, len(ids)+1)), "Request IDs skipped/reused")
    attachments = [(key, request) for key, request in sent.items()
                   if request["method"] == "Target.attachToTarget" and "result" in replies[key]]
    require(len(attachments) == 2, "Wrong successful target attachment roster")
    attached_events = [(pos, route, event) for pos, route, event in events
                       if event["method"] == "Target.attachedToTarget"]
    require(len(attached_events) == 2, "Wrong actual attachment event roster")
    attach_keys = {}
    for role, (key, request) in zip(("A", "B"), attachments):
        require(key[0] == "flattened" and request.get("sessionId") is None
                and request["params"] == {"targetId": targets[role], "flatten": True}
                and replies[key]["result"] == {"sessionId": sessions[role]},
                "Actual attachment request/response selected wrong target/route")
        matching = [(pos, route, event) for pos, route, event in attached_events
                    if event.get("params", {}).get("sessionId") == sessions[role]]
        require(len(matching) == 1, "Missing/duplicate actual attachment event")
        pos, route, event = matching[0]
        info = event["params"].get("targetInfo", {})
        require(route == "flattened" and event.get("sessionId") is None
                and positions[key][0] < pos < positions[key][1] < context_positions[role]
                and event["params"].get("waitingForDebugger") is False
                and info.get("targetId") == targets[role] and info.get("type") == "page"
                and info.get("url") == (URL if role == "A" else "about:blank")
                and info.get("attached") is True, "Actual attachment event identity/order differs")
        attach_keys[role] = key
    destroyed_positions = [pos for pos, route, event in events
                           if route == "flattened" and event["method"] == "Runtime.executionContextDestroyed"]
    require(len(destroyed_positions) == 1, "Missing unique flattened destruction")
    b_attach = positions[attach_keys["B"]][0]
    inventories = [(key, request) for key, request in sent.items() if request["method"] == "Target.getTargets"]
    before_b, live_b, closed_b = [], [], []
    for key, request in inventories:
        require(key[0] == "flattened" and request.get("sessionId") is None and request["params"] == {},
                "Target discovery used an unexpected route/parameters")
        inventory = target_inventory(replies[key]["result"])
        require(set(inventory) <= set(targets.values()) and inventory["page-1"].get("attached") is True,
                "Target inventory rebound or lost A's actual attachment")
        if positions[key][1] < b_attach:
            bound = bind_new_target(replies[key]["result"], target_number(b_target))
            require(bound is None or bound == b_target, "Pre-attach discovery bound a foreign B")
            before_b.append(key)
        elif positions[key][1] < destroyed_positions[0]:
            require(set(inventory) == set(targets.values()) and inventory[b_target].get("url") == URL_B
                    and inventory[b_target].get("attached") is True, "Move changed live target/URL roster")
            live_b.append(key)
        else:
            require(set(inventory) == {"page-1"}, "Closed target survived or was replaced")
            closed_b.append(key)
    require(before_b and len(live_b) >= 3 and closed_b, "Missing initial/move/closure target barriers")
    require(positions[before_b[0]][1] < positions[attach_keys["A"]][0]
            and set(target_inventory(replies[before_b[0]]["result"])) == {"page-1"},
            "Initial A-only roster missing before attachment")
    new_target_inventory = replies[before_b[-1]]["result"]
    require(bind_new_target(new_target_inventory, target_number(b_target)) == b_target,
            "B attachment lacks its last actual blank-target discovery")
    intervals = []
    previous = -1
    for position, row in enumerate(records):
        if row.get("phase") == "log-interval":
            item = dict(row)
            item["record_position"] = position
            item["previous_interval_position"] = previous
            previous = position
            intervals.append(item)
    autos = [row for row in intervals if row["kind"] == "automation"]
    require(len(autos) == len(ROSTER), "Wrong accepted command count")
    compiled = {"A": [], "B": []}
    accepted_keys = set()
    for row, (role, sequence, route, label, source_name, group, expected) in zip(autos, ROSTER):
        require((row["realm"], row["sequence"], row["label"]) == (role, sequence, label)
                and row["command"]["route"] == route, "Frozen accepted roster differs")
        key = (route, row["command"]["id"])
        require(key in sent and key not in accepted_keys and row["previous_interval_position"] < positions[key][0]
                < positions[key][1] < row["record_position"], "Accepted log interval not bound to its actual reply")
        accepted_keys.add(key)
        request, response = sent[key], replies[key]
        require("error" not in response, "Accepted operation returned protocol error")
        require(request.get("sessionId") == (sessions[role] if route == "flattened" else None), "Operation selected wrong realm")
        source = sources[source_name]
        if source_name == "INCREMENT":
            root_name = "A" if role == "A" else ("B_flat" if route == "flattened" else "B_direct")
            root = roots[root_name]
            require(request["method"] == "Runtime.callFunctionOn" and request["params"] == {
                "functionDeclaration": source, "objectId": root, "arguments": [{"objectId": root}], "returnByValue": True},
                "Retained receiver/argument alias source differs")
            require(response["result"] == {"result": {"type": "object", "subtype": "array", "value": [role, expected]}},
                    "Literal retained closure count/realm result differs")
            require(type(response["result"]["result"]["value"][1]) is float,
                    "Finite Runtime numbers must use the actual f64 JSON encoding")
            body = "(" + source + "\n)"
        else:
            params = {"expression": source, "contextId": contexts[role]["id"], "returnByValue": False}
            if group is not None:
                params["objectGroup"] = group
            require(request["method"] == "Runtime.evaluate" and request["params"] == params, "Exact evaluation source/context/group differs")
            result = {"type": "string", "value": role} if source_name == "MARKER" else {"type": "object", "objectId": roots[expected]}
            require(response["result"] == {"result": result}, "Actual root/scalar result differs")
            body = source
        compiled[role].append({"route": route, "id": key[1], "sequence": sequence, "bytes": len(body.encode()),
                               "sha256": hashlib.sha256(body.encode()).hexdigest()})
    negatives = [row for row in intervals if row["kind"] == "quiet" and row.get("command") is not None]
    require([row["label"] for row in negatives] == list(NEGATIVES), "Negative controls changed")
    rejected_keys = set()
    for row in negatives:
        key = (row["command"]["route"], row["command"]["id"])
        require(key in sent and key not in rejected_keys and key not in accepted_keys
                and row["previous_interval_position"] < positions[key][0] < positions[key][1] < row["record_position"],
                "Negative interval lacks its actual response")
        rejected_keys.add(key)
        require(replies[key].get("error", {}).get("code") == -32000 and "result" not in replies[key],
                "Negative was not a specific protocol rejection")
        label = row["label"]
        if label == "closed-target":
            params = {"targetId": targets["B"], "flatten": True}
            require(key[0] == "flattened" and sent[key].get("sessionId") is None
                    and sent[key]["method"] == "Target.attachToTarget" and sent[key]["params"] == params, "Wrong closed target negative")
        else:
            root = roots["A" if label.startswith("foreign-context") else "B_flat" if label == "closed-session" else "B_direct"]
            route = "flattened" if label.startswith("foreign-owner") or label == "closed-session" else "direct-A" if label == "closed-root-foreign" else "direct-B"
            require(key[0] == route and sent[key].get("sessionId") == (sessions["B"] if route == "flattened" else None)
                    and sent[key]["method"] == "Runtime.callFunctionOn" and sent[key]["params"] == {
                        "functionDeclaration": sources["INCREMENT"], "objectId": root,
                        "arguments": [{"objectId": root}], "returnByValue": True}, "Wrong foreign/stale handle negative")
    require({key for key in replies if "error" in replies[key]} == rejected_keys, "Extra unaccounted protocol error")
    require({key for key in sent if sent[key]["method"] in ("Runtime.evaluate", "Runtime.callFunctionOn")}
            == accepted_keys | {key for key in rejected_keys if sent[key]["method"] == "Runtime.callFunctionOn"},
            "Unaccounted Runtime source command")
    enables = [(key, request) for key, request in sent.items() if request["method"] == "Runtime.enable"]
    require(len(enables) == 4 and {(key[0], request.get("sessionId")) for key, request in enables}
            == {("direct-A", None), ("direct-B", None), ("flattened", sessions["A"]), ("flattened", sessions["B"])},
            "Wrong per-route Runtime enable roster")
    for key, request in enables:
        role = "A" if key[0] == "direct-A" or request.get("sessionId") == sessions["A"] else "B"
        require(request["params"] == {} and replies[key]["result"] == {}, "Enable reply/parameters differ")
        matching = [(pos, event) for pos, route, event in events if route == key[0]
            and event.get("sessionId") == request.get("sessionId") and event["method"] == "Runtime.executionContextCreated"]
        require(len(matching) == 1 and positions[key][1] < matching[0][0] < context_positions[role]
                and matching[0][1]["params"] == {"context": contexts[role]}, "Actual route context event not after enable acknowledgment")
    runtime_events = [(route, event) for _, route, event in events if event["method"].startswith("Runtime.")]
    require(len(runtime_events) in (5, 6), "Unexpected Runtime lifecycle event")
    destroyed = [(pos, route, event) for pos, route, event in events if event["method"] == "Runtime.executionContextDestroyed"]
    require(len(destroyed) in (1, 2), "Wrong destroyed context count")
    for _, route, event in destroyed:
        require(route in ("direct-B", "flattened") and event.get("sessionId") == (sessions["B"] if route == "flattened" else None)
                and event["params"] == {"executionContextId": contexts["B"]["id"],
                    "executionContextUniqueId": contexts["B"]["uniqueId"]}, "Survivor/foreign context destroyed")
    flat_destroy = [pos for pos, route, _ in destroyed if route == "flattened"]
    detach = [(pos, route, event) for pos, route, event in events if event["method"] == "Target.detachedFromTarget"]
    require(len(flat_destroy) == 1 and len(detach) == 1 and flat_destroy[0] < detach[0][0]
            and detach[0][1] == "flattened" and detach[0][2].get("sessionId") is None
            and detach[0][2]["params"] == {"sessionId": sessions["B"]},
            "Context destruction/flattened detach ordering differs")
    nav = [request for request in sent.values() if request["method"] == "Page.navigate"]
    require(len(nav) == 1 and nav[0] == {"id": nav[0]["id"], "method": "Page.navigate",
            "sessionId": sessions["B"], "params": {"url": URL_B}}, "Only one actual B navigation is permitted")
    for key, request in sent.items():
        if request["method"] == "Page.getFrameTree":
            require(request.get("sessionId") in sessions.values() if key[0] == "flattened"
                    else request.get("sessionId") is None, "Frame barrier used an unbound route")
            role = "A" if key[0] == "direct-A" or request.get("sessionId") == sessions["A"] else "B"
            frame = replies[key]["result"]["frameTree"]["frame"]
            require(request["params"] == {} and frame["id"] == frames[role]
                    and frame["url"] == (URL if role == "A" else URL_B), "Actual frame URL barrier differs")
    page_events = [(route, event) for _, route, event in events if event["method"].startswith("Page.")]
    require([event["method"] for _, event in page_events] == ["Page.frameStartedLoading", "Page.frameNavigated",
            "Page.domContentEventFired", "Page.loadEventFired", "Page.frameStoppedLoading"]
            and all(route == "flattened" and event.get("sessionId") == sessions["B"] for route, event in page_events)
            and page_events[1][1]["params"]["frame"]["url"] == URL_B
            and page_events[1][1]["params"]["frame"]["id"] == frames["B"], "Extra/missing Page lifecycle across moves")
    paints = [row for row in phases if row["phase"] == "paint"]
    require([row["label"] for row in paints] == ["A-detached", "B-detached", "B-redocked", "A-survivor"], "Wrong PNG phase roster")
    for row in paints:
        color = [0x12, 0x34, 0x56] if row["label"].startswith("A") else [0x65, 0x43, 0x21]
        require(row["color"] == color and row["native_page_top"] == 98 and row["scale"] == 1
                and row["native_size"] == [800, 634] and row["cdp_size"] == [800, 507], "Literal move paint geometry differs")
    require([row for row in phases if row["phase"] == "direct-closed"]
            == [{"phase": "direct-closed", "route": "direct-B"}], "Wrong direct socket closure")
    require([row for row in phases if row["phase"] == "closed-endpoint"]
            == [{"phase": "closed-endpoint", "target": targets["B"], "status": 404}], "Wrong closed endpoint observation")
    return {"phases": phases, "intervals": intervals, "commands": len(sent), "compiled_sources": compiled,
            "contexts": contexts, "sessions": sessions, "targets": targets, "frames": frames,
            "new_target_inventory": new_target_inventory, "roots": roots, "summary": summary}


def check_tab_counts(log, original, moved):
    """TABS count is global workspace size; groups is per native window."""
    require(type(original) is int and type(moved) is int
            and 0 < original != moved > 0, "Invalid owned window identities")
    opened_tab(log)
    lines = log.splitlines()
    opened_position = next(position for position, line in enumerate(lines) if line.startswith("TAB "))
    tabs = []
    for position, line in enumerate(lines):
        if not line.startswith("TABS"):
            continue
        match = re.fullmatch(r"TABS window=([1-9][0-9]*) groups=([1-9][0-9]*) count=([1-9][0-9]*)", line)
        require(match is not None, "Malformed native tab count record")
        window, groups, count = map(int, match.groups())
        require(window in (original, moved) and groups == 1 and count in (1, 2)
                and (window != moved or count == 2), "Wrong native tab geometry state")
        tabs.append((position, window, count))
    require(tabs and tabs[0][1:] == (original, 1) and tabs[-1][1:] == (original, 1),
            "Wrong first/final native tab count")
    counts = []
    for _, _, count in tabs:
        if not counts or counts[-1] != count:
            counts.append(count)
    require(counts == [1, 2, 1], "Wrong global workspace tab count order")
    first_two = next(position for position, _, count in tabs if count == 2)
    moved_positions = [position for position, window, _ in tabs if window == moved]
    require(opened_position < first_two and moved_positions
            and max(moved_positions) < tabs[-1][0], "Native tab count phases differ from opening/moves")
    return [(str(window), "1", str(count)) for _, window, count in tabs]


def check_resources(log, protocol, pid):
    for marker in ("SCRIPT_PARTIAL", "SCRIPT_DIAGNOSTIC", "SCRIPT_ERROR", "SCRIPT_REJECTED",
                   "SCRIPT_SESSION_CLOSED", "SCRIPT_SHUTDOWN_ERROR", "LOAD_ERROR", "RESOURCE_WARNING",
                   "RESOURCE_DIAGNOSTIC", "JOURNEY_INCOMPLETE", "SCRIPT_ALLOCATION"):
        require(marker not in log, "Unexpected native/script/resource failure: " + marker)
    require(re.findall(r"^NAVIGATE GET (.*)$", log, re.M) == [URL, URL_B], "Wrong native navigation inventory")
    windows = re.findall(r"^WINDOW id=([1-9][0-9]*) pid=([1-9][0-9]*)$", log, re.M)
    require(len(windows) == 2 and len({window for window, _ in windows}) == 2
            and all(int(owner) == pid for _, owner in windows), "Wrong native WINDOW/PID roster")
    original, moved = [int(window) for window, _ in windows]
    require([row for row in protocol["phases"] if row["phase"] == "closed-B"] == [{
        "phase": "closed-B", "destroyed_context": protocol["contexts"]["B"]["id"],
        "surviving_context": protocol["contexts"]["A"]["id"], "original_window": original, "removed_window": moved}],
        "Closed target/native/context identities differ")
    paints = [row for row in protocol["phases"] if row["phase"] == "paint"]
    require([row["window"] for row in paints] == [original, moved, original, original],
            "Actual paint window identity differs across moves")
    require(bind_new_target(protocol["new_target_inventory"], opened_tab(log)) == protocol["targets"]["B"],
            "Actual native tab differs from frozen discovery/attachment/context identity")
    tabs = check_tab_counts(log, original, moved)
    drags = [row for row in protocol["phases"] if row["phase"] == "native-drag"]
    require(drags == [{"phase": "native-drag", "label": "detach", "window": original, "from": [290, 56], "to": [950, 180]},
                      {"phase": "native-drag", "label": "redock", "window": moved, "from": [1020, 56], "to": [270, 56]}],
            "Actual native drag coordinates/window changed")
    body = log.encode()
    require(body.endswith(b"\n"), "Final retained browser log has a partial line")
    cursor = 0
    realms = {"A": [], "B": []}
    boot = []
    for interval in protocol["intervals"]:
        start, end = interval["start"], interval["end"]
        require(type(start) is int and type(end) is int and start == cursor and start <= end <= len(body)
                and (start == 0 or body[start-1:start] == b"\n") and (end == 0 or body[end-1:end] == b"\n"),
                "Log intervals are not complete contiguous actual byte ranges")
        lines = body[start:end].decode().splitlines()
        rows = [json.loads(line[11:]) for line in lines if line.startswith("SCRIPT_BOA ")]
        tasks = [line for line in lines if line.startswith("PAGE_TASK_APPLIED ")]
        auto = [line for line in lines if line.startswith("PAGE_AUTOMATION_APPLIED ")]
        complete = [line for line in lines if line.startswith("SCRIPT_COMPLETE ")]
        require(not any(line.startswith("PAGE_EVENT_APPLIED ") for line in lines), "Unexpected page activation")
        kind, role = interval["kind"], interval["realm"]
        if kind == "bootstrap":
            require(role in realms and not realms[role] and len(rows) == 3 and not auto
                    and tasks == ["PAGE_TASK_APPLIED sequence=1 revision=1", "PAGE_TASK_APPLIED sequence=2 revision=2"]
                    and complete == ["SCRIPT_COMPLETE executed=1", "SCRIPT_COMPLETE executed=0", "SCRIPT_COMPLETE executed=0"],
                    "Actual bootstrap range differs")
            boot.append(role)
            realms[role].extend(rows)
        elif kind == "automation":
            sequence = interval["sequence"]
            require(role in realms and sequence == len(realms[role]) and len(rows) == 1 and not tasks
                    and auto == [f"PAGE_AUTOMATION_APPLIED sequence={sequence} revision={sequence} terminal=false"]
                    and complete == ["SCRIPT_COMPLETE executed=0"], "Actual accepted operation range differs")
            realms[role].extend(rows)
        else:
            require(kind == "quiet" and role is None and interval["sequence"] is None
                    and not any(line.startswith(WORK_PREFIXES) for line in lines), "Unattributed actual worker work")
        cursor = end
    require(boot == ["A", "B"] and not any(line.startswith(WORK_PREFIXES) for line in body[cursor:].decode().splitlines()),
            "Unattributed final worker work or bootstrap order")
    require([len(realms[role]) for role in ("A", "B")] == [10, 11], "Wrong complete per-realm report roster")
    source = source_bytes(REPO / "tests/fixtures/script/events.html").split(b"<script>", 1)[1].split(b"</script>", 1)[0]
    for role, rows in realms.items():
        source_count, source_size = 1, len(source)
        compiled = protocol["compiled_sources"][role]
        require(len(compiled) == (7 if role == "A" else 8), "Wrong source command roster")
        for position, row in enumerate(rows):
            if position >= 3:
                item = compiled[position-3]
                require(item["sequence"] == position, "Compilation/sequence order differs")
                source_count += 1
                source_size += item["bytes"]
            fields = ("opcodes_remaining", "jobs_admitted", "jobs_executed", "pending_jobs", "source_bytes", "source_admissions")
            require(type(row) is dict and all(type(row.get(key)) is int for key in fields)
                    and row.get("profile") == "boa-page-process-v1" and row.get("fatal_reason", "missing") is None
                    and 0 < row["opcodes_remaining"] <= 1_000_000
                    and row["jobs_admitted"] == 2 and row["jobs_executed"] == min(position, 2)
                    and row["pending_jobs"] == max(2-position, 0)
                    and row["source_admissions"] == source_count and row["source_bytes"] == source_size,
                    "Wrong/fatal/renewed per-realm work or source accounting")
            memory = row.get("worker_memory") or {}
            keys = ("outstanding_limit_bytes", "cumulative_limit_bytes", "outstanding_bytes", "peak_bytes", "cumulative_bytes", "allocations")
            require(all(type(memory.get(key)) is int for key in keys)
                    and memory["outstanding_limit_bytes"] == 32*1024*1024
                    and memory["cumulative_limit_bytes"] == 64*1024*1024
                    and 0 <= memory["outstanding_bytes"] <= memory["peak_bytes"] <= memory["outstanding_limit_bytes"]
                    and memory["peak_bytes"] <= memory["cumulative_bytes"] <= memory["cumulative_limit_bytes"], "Invalid requested-System report")
            if position:
                before = rows[position-1]
                require(row["opcodes_remaining"] <= before["opcodes_remaining"]
                        and all(memory[key] >= before["worker_memory"][key] for key in ("peak_bytes", "cumulative_bytes", "allocations")),
                        "Per-realm counters renewed during movement")
    return {"A": realms["A"], "B": realms["B"], "attribution": "contiguous byte intervals and actual routed reply/sequence roster",
            "final_quiet_bytes": len(body)-cursor, "native_windows": windows, "native_tab_states": tabs}


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

    class RuntimeMovesRun(owned.Run):
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
    root = Path(tempfile.mkdtemp(prefix="runtime-moves-smoke-", dir=temporary))
    print("RUNTIME_MOVES_ARTIFACTS " + str(root), flush=True)
    env = common.private_environment(root, font)
    preferences = Path(env["XDG_CONFIG_HOME"]) / "mgbrowser/settings.json"
    inputs[str(preferences)] = {"sha256": common.digest(preferences, LOG_LIMIT), "limit": LOG_LIMIT}
    run = RuntimeMovesRun(root, env)
    report = {"schema_version": 1, "status": "ERROR", "scope": "authored supplied-binary Runtime native detach/redock/closure and actual retained-root; not WPT, Playwright, full Runtime or independent hardware input",
        "binary": str(binary), "binary_sha256": args.expected_sha, "expected_version": args.expected_version,
        "expected_commit": args.expected_commit, "input_sha256": inputs, "commands": run.commands,
        "handoff_failures": run.handoff_failures, "target_moves": True,
        "manual_visual_review_required": "Inspect all eight actual native/CDP move images; no independent physical-input claim",
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
        client_log = run.command([client, endpoint[0], endpoint[2], browser.pid, root / "browser.log", root / "frames"], "client.log", timeout=30)
        report["protocol"] = check_protocol(client_log)
        exit_deadline = min(run.deadline, time.monotonic() + 10)
        while browser.poll() is None and time.monotonic() < exit_deadline:
            run.guard()
            time.sleep(0.025)
        require(browser.poll() == 0, "Last native close did not produce normal browser exit0")
        browser.stop()  # WNOWAIT kept the leader identity until owned group stop/reap.
        require(browser.process.returncode == 0, "Normal native close status changed during reap")
        browser_log = owned.bounded_text(root / "browser.log")
        require(re.findall(r"^DISPLAY_SCALE system=[0-9]+% effective=([0-9]+)%$", browser_log, re.M) == ["100"], "Unexpected native scale")
        report["resource_realms"] = check_resources(browser_log, report["protocol"], browser.pid)
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
        require(requests == Counter({"200 OK /": 1, "200 OK /script-events": 2}), "Exact source/destination/trap HTTP inventory differs")
        report["http_requests"] = dict(requests)
        report["http_log_scope"] = "Shared server logs paths only; client/frame/native navigation independently bind exact runtime-move=A/B queries"
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
        print("RUNTIME_MOVES_SMOKE_PASS " + str(root), flush=True)
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
