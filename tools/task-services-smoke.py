#!/usr/bin/env python3
"""Verify real later timer tasks in supplied packaged/public mgbrowser bytes.

Uses the existing owned display/server/native-input/external-CDP infrastructure.
Authored fixture only: not WPT, Playwright or live-site compatibility evidence.
Build journey_server and cdp_journey first, then supply an absolute binary path.
"""

import argparse
from collections import Counter
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import sys
import tempfile


REPO = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("page_services", REPO / "tools/page-services-smoke.py")
services = importlib.util.module_from_spec(SPEC)
sys.dont_write_bytecode = True
SPEC.loader.exec_module(services)
require = services.require


def check_tasks(log, name):
    require("PAGE_TASK_APPLIED " in log, f"{name}: no accepted parent-issued timer task")
    require("SCRIPT_PARTIAL" not in log and "SCRIPT_DIAGNOSTIC" not in log and
            "SCRIPT_ERROR" not in log and "SCRIPT_REJECTED" not in log,
            f"{name}: browser reported a script error")
    reports = [json.loads(line.removeprefix("SCRIPT_BOA "))
               for line in log.splitlines() if line.startswith("SCRIPT_BOA ")]
    require(len(reports) >= 2, f"{name}: missing startup and task worker reports")
    require(all(report.get("profile") == "boa-page-process-v1" and
                report.get("fatal_reason") is None and
                0 < report.get("opcodes_remaining", 1_000_001) <= 1_000_000 and
                0 <= report.get("pending_jobs", 257) <= 256 and
                0 <= report.get("jobs_executed", 2049) <= report.get("jobs_admitted", -1) <= 2048 and
                report["pending_jobs"] <= report["jobs_admitted"] - report["jobs_executed"] and
                0 <= report.get("source_admissions", -1) <= report.get("source_bytes", -1) <= 4 * 1024 * 1024 and
                (report.get("worker_memory") or {}).get("outstanding_limit_bytes") == 32 * 1024 * 1024 and
                (report.get("worker_memory") or {}).get("cumulative_limit_bytes") == 64 * 1024 * 1024
                for report in reports), f"{name}: invalid worker accounting or changed limits")
    for report in reports:
        memory = report["worker_memory"]
        require(0 < memory["outstanding_bytes"] <= memory["peak_bytes"] <= 32 * 1024 * 1024 and
                memory["peak_bytes"] <= memory["cumulative_bytes"] <= 64 * 1024 * 1024 and
                0 < memory["allocations"] <= memory["cumulative_bytes"],
                f"{name}: inconsistent real worker memory counters")
    require(any(report.get("pending_jobs", 0) > 0 for report in reports),
            f"{name}: startup did not retain later work")
    require(any(report.get("jobs_executed", 0) >= 4 and report.get("pending_jobs") == 0
                for report in reports), f"{name}: task/microtasks did not finish")
    source = (REPO / "tests/fixtures/script/task-events.html").read_text().split("<script>", 1)[1].split("</script>", 1)[0]
    realm = [report for report in reports if report["source_admissions"] == 1 and
             report["source_bytes"] == len(source.encode())]
    services.check_lifecycle_tasks(log, reports, (1, len(source.encode())),
                                   [(3, 1, 2), (3, 2, 1), (6, 4, 1), (7, 6, 0)], name)
    require(len(realm) >= 2, f"{name}: missing original fixture realm's task report")
    require(realm[0]["pending_jobs"] > 0 and realm[-1]["pending_jobs"] == 0,
            f"{name}: original fixture task did not settle")
    for previous, current in zip(realm, realm[1:]):
        require(current["opcodes_remaining"] <= previous["opcodes_remaining"] and
                current["jobs_admitted"] >= previous["jobs_admitted"] and
                current["jobs_executed"] >= previous["jobs_executed"] and
                all(current["worker_memory"][field] >= previous["worker_memory"][field]
                    for field in ("peak_bytes", "cumulative_bytes", "allocations")),
                f"{name}: original fixture realm renewed its work/memory counters")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    helpers = [REPO / "target/debug/examples" / name for name in ("journey_server", "cdp_journey")]
    for path in [binary] + helpers:
        require(path.is_file() and os.access(path, os.X_OK), f"Missing executable: {path}")
    for tool in ("Xvfb", "xauth"):
        require(shutil.which(tool), f"Missing acceptance tool: {tool}")
    (REPO / "tmp").mkdir(exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix="task-services-smoke-", dir=REPO / "tmp"))
    print(f"TASK_SERVICES_ARTIFACTS {root}", flush=True)
    private = root / "home"
    private.mkdir()
    env = dict(os.environ, HOME=str(private), XDG_CONFIG_HOME=str(private / "config"),
               XDG_DATA_HOME=str(private / "data"), XDG_CACHE_HOME=str(private / "cache"),
               XDG_STATE_HOME=str(private / "state"), XDG_RUNTIME_DIR=str(private / "runtime"),
               DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent-task-services-session",
               MGBROWSER_NO_AUTO_UPDATE="1")
    for key in ("DISPLAY", "XAUTHORITY", "MGBROWSER_INSTALL_DIR"):
        env.pop(key, None)
    Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
    run = services.Run(root, env)
    initial_hash = services.sha256(binary)
    report = {"schema_version": 1, "status": "ERROR", "binary": str(binary),
              "binary_sha256": initial_hash, "commands": run.commands,
              "fixture_sha256": services.sha256(REPO / "tests/fixtures/script/task-events.html"),
              "scope": "authored real timer tasks and retained events; not WPT or live-site compatibility"}
    try:
        server = services.start_journey(run, helpers[0])
        services.start_display(run)
        services.journeys(run, binary, helpers[1],
                          fixture="http://127.0.0.1:7878/script-task-events", worker_check=check_tasks)
        require("CDP_TASK_ORDER_OK " in (root / "cdp-client.log").read_text(),
                "External CDP did not verify actual task-order proof")
        require(server.poll() is None and services.listener_owned(server.pid, 7878),
                "Lost ownership of fixture service")
        server_log = (root / "journey-server.log").read_text(errors="replace")
        require("/event-trap" not in server_log, "Canceled action reached trap endpoint")
        counts = Counter(server_log.splitlines())
        for path in ("/script-task-events", "/event-search", "/event-destination"):
            require(counts[f"200 OK {path}"] == 2, f"Expected two successful requests for {path}")
        require(services.sha256(binary) == initial_hash, "Supplied binary changed during acceptance")
        report.update(status="PASS", native=True, external_cdp=True, task_order=True,
                      retained_events=True, trap_requests=0, binary_unchanged=True)
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
    print(f"TASK_SERVICES_SMOKE_OK {initial_hash} {root}", flush=True)


if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise KeyboardInterrupt("Acceptance interrupted; cleaning owned processes")
    signal.signal(signal.SIGTERM, interrupted)
    main()
