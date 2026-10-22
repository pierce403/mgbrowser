#!/usr/bin/env python3
"""Check real later Promise rejection delivery in supplied release bytes.

Authored native/external-CDP retained-form acceptance, not upstream WPT,
Playwright or live-site compatibility. Existing diagnostic-free and Window-error
checkers are unchanged. Build journey_server/cdp_journey before running.
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
SPEC = importlib.util.spec_from_file_location("rejection_services", REPO / "tools/page-services-smoke.py")
services = importlib.util.module_from_spec(SPEC)
sys.dont_write_bytecode = True
SPEC.loader.exec_module(services)
require = services.require
FIXTURE = REPO / "tests/fixtures/script/rejection-events.html"
DIAGNOSTIC = "Unhandled Promise rejection"


def check_rejections(log, name):
    diagnostics = [line for line in log.splitlines() if line.startswith("SCRIPT_DIAGNOSTIC ")]
    require(diagnostics == ["SCRIPT_DIAGNOSTIC index=1 message=" + json.dumps(DIAGNOSTIC)],
            f"{name}: missing, extra or erased genuine rejection diagnostic")
    partials = [line for line in log.splitlines() if line.startswith("SCRIPT_PARTIAL ")]
    require(partials == ["SCRIPT_PARTIAL executed=0 errors=1 first=" + json.dumps(DIAGNOSTIC)],
            f"{name}: expected one explicitly partial rejection transaction")
    require("SCRIPT_ERROR" not in log and "SCRIPT_REJECTED" not in log,
            f"{name}: another script failure was reported")
    reports = [json.loads(line.removeprefix("SCRIPT_BOA "))
               for line in log.splitlines() if line.startswith("SCRIPT_BOA ")]
    require(len(reports) >= 4, f"{name}: missing separate startup/DCL/load/rejection reports")
    for report in reports:
        require(report.get("profile") == "boa-page-process-v1" and
                report.get("fatal_reason") is None and
                0 < report.get("opcodes_remaining", 1_000_001) <= 1_000_000 and
                0 <= report.get("pending_jobs", 257) <= 256 and
                0 <= report.get("jobs_executed", 2049) <= report.get("jobs_admitted", -1) <= 2048 and
                report["pending_jobs"] <= report["jobs_admitted"] - report["jobs_executed"] and
                0 <= report.get("source_admissions", -1) <= report.get("source_bytes", -1) <= 4 * 1024 * 1024,
                f"{name}: invalid or renewed worker accounting")
        memory = report.get("worker_memory") or {}
        require(memory.get("outstanding_limit_bytes") == 32 * 1024 * 1024 and
                memory.get("cumulative_limit_bytes") == 64 * 1024 * 1024 and
                0 < memory.get("outstanding_bytes", 0) <= memory.get("peak_bytes", 0) <= 32 * 1024 * 1024 and
                memory["peak_bytes"] <= memory.get("cumulative_bytes", 0) <= 64 * 1024 * 1024 and
                0 < memory.get("allocations", 0) <= memory["cumulative_bytes"],
                f"{name}: invalid or changed requested-System memory bounds")
    source = FIXTURE.read_text().split("<script>", 1)[1].split("</script>", 1)[0]
    realm = [report for report in reports if report["source_admissions"] == 1 and
             report["source_bytes"] == len(source.encode())]
    services.check_lifecycle_tasks(log, reports, (1, len(source.encode())),
                                   [(3, 1, 2), (3, 2, 1), (4, 3, 1), (6, 6, 0)], name)
    for previous, current in zip(realm, realm[1:]):
        require(current["opcodes_remaining"] <= previous["opcodes_remaining"] and
                current["jobs_admitted"] >= previous["jobs_admitted"] and
                current["jobs_executed"] >= previous["jobs_executed"] and
                all(current["worker_memory"][field] >= previous["worker_memory"][field]
                    for field in ("peak_bytes", "cumulative_bytes", "allocations")),
                f"{name}: original realm renewed cumulative counters")


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
    root = Path(tempfile.mkdtemp(prefix="rejection-services-smoke-", dir=REPO / "tmp"))
    print(f"REJECTION_SERVICES_ARTIFACTS {root}", flush=True)
    private = root / "home"
    private.mkdir()
    env = dict(os.environ, HOME=str(private), XDG_CONFIG_HOME=str(private / "config"),
               XDG_DATA_HOME=str(private / "data"), XDG_CACHE_HOME=str(private / "cache"),
               XDG_STATE_HOME=str(private / "state"), XDG_RUNTIME_DIR=str(private / "runtime"),
               DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent-rejection-services-session",
               MGBROWSER_NO_AUTO_UPDATE="1")
    for key in ("DISPLAY", "XAUTHORITY", "MGBROWSER_INSTALL_DIR"):
        env.pop(key, None)
    Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
    run = services.Run(root, env)
    initial_hash = services.sha256(binary)
    report = {"schema_version": 1, "status": "ERROR", "binary": str(binary),
              "binary_sha256": initial_hash, "commands": run.commands,
              "fixture_sha256": services.sha256(FIXTURE),
              "scope": "authored later rejection and retained form; not WPT or live-site compatibility"}
    try:
        server = services.start_journey(run, helpers[0])
        services.start_display(run)
        services.journeys(run, binary, helpers[1],
                          fixture="http://127.0.0.1:7878/script-rejection-events", worker_check=check_rejections)
        marker = "CDP_PROMISE_REJECTION_OK typed-identity-canceled checkpoint="
        require(marker in (root / "cdp-client.log").read_text(),
                "External CDP did not verify actual rejection identity/checkpoints")
        require(server.poll() is None and services.listener_owned(server.pid, 7878),
                "Lost ownership of fixture service")
        server_log = (root / "journey-server.log").read_text(errors="replace")
        require("/event-trap" not in server_log, "Canceled action reached trap endpoint")
        counts = Counter(server_log.splitlines())
        for path in ("/script-rejection-events", "/event-search", "/event-destination"):
            require(counts[f"200 OK {path}"] == 2, f"Expected two successful requests for {path}")
        require(services.sha256(binary) == initial_hash, "Supplied binary changed during acceptance")
        report.update(status="PASS", native=True, external_cdp=True, rejection_identity=True,
                      retained_diagnostics_per_journey=1, checkpoint=True, retained_events=True,
                      trap_requests=0, binary_unchanged=True)
    except BaseException as error:
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        try:
            run.close()
            report["cleanup"] = {"confirmed": True, "owned_processes": len(run.children),
                                 "returncodes": [process.returncode for process in run.children]}
        except BaseException as error:
            report["status"] = "ERROR"
            report["cleanup"] = {"confirmed": False, "error": f"{type(error).__name__}: {error}"}
            raise
        finally:
            (root / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"REJECTION_SERVICES_SMOKE_OK {initial_hash} {root}", flush=True)


if __name__ == "__main__":
    def interrupted(_signum, _frame):
        raise KeyboardInterrupt("Acceptance interrupted; cleaning owned processes")
    signal.signal(signal.SIGTERM, interrupted)
    main()
