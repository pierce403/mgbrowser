#!/usr/bin/env python3
"""Small negative controls for native acceptance evidence/cleanup, not WPT."""
import importlib.util
import errno
import json
import os
from pathlib import Path
import socket
import sys
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
path = Path(__file__).with_name("task-services-smoke.py")
spec = importlib.util.spec_from_file_location("task_services", path)
task = importlib.util.module_from_spec(spec)
spec.loader.exec_module(task)


class Controls(unittest.TestCase):
    def reports(self):
        source = (task.REPO / "tests/fixtures/script/task-events.html").read_text().split("<script>", 1)[1].split("</script>", 1)[0]
        common = {"profile": "boa-page-process-v1", "fatal_reason": None,
                  "source_admissions": 1, "source_bytes": len(source.encode())}
        memory = {"outstanding_bytes": 1000, "peak_bytes": 2000,
                  "cumulative_bytes": 3000, "allocations": 10,
                  "outstanding_limit_bytes": 32 * 1024 * 1024,
                  "cumulative_limit_bytes": 64 * 1024 * 1024}
        return [dict(common, opcodes_remaining=999900 - i, jobs_admitted=a,
                     jobs_executed=e, pending_jobs=p, worker_memory=dict(memory))
                for i, (a, e, p) in enumerate([(3, 1, 2), (3, 2, 1), (6, 4, 1), (7, 6, 0)])]

    def log(self, reports):
        return "\n".join([*("SCRIPT_BOA " + json.dumps(report) for report in reports),
                           *(f"PAGE_TASK_APPLIED sequence={i} revision={i}" for i in range(1, 4)), ""])

    def test_consistent_cumulative_fixture_evidence_is_accepted(self):
        task.check_tasks(self.log(self.reports()), "synthetic guard control")

    def test_invalid_or_renewed_resource_evidence_never_passes(self):
        for change in ("pending", "source", "memory", "opcodes", "admitted", "allocations"):
            with self.subTest(change=change):
                reports = self.reports()
                before, after = reports[0], reports[-1]
                if change == "pending":
                    before["pending_jobs"] = 4
                elif change == "source":
                    before["source_bytes"] = 4 * 1024 * 1024 + 1
                elif change == "memory":
                    after["worker_memory"]["cumulative_bytes"] = 2001
                elif change == "opcodes":
                    after["opcodes_remaining"] = before["opcodes_remaining"] + 1
                elif change == "admitted":
                    before["jobs_admitted"] = after["jobs_admitted"] + 1
                elif change == "allocations":
                    after["worker_memory"]["allocations"] = 1
                with self.assertRaises(RuntimeError):
                    task.check_tasks(self.log(reports), "synthetic guard control")

    def test_missing_task_or_explicit_diagnostic_never_passes(self):
        valid = self.log(self.reports())
        for log in (valid.replace("PAGE_TASK_APPLIED ", "NOT_A_TASK "),
                    valid + 'SCRIPT_DIAGNOSTIC index=1 message="failed"\n'):
            with self.assertRaises(RuntimeError):
                task.check_tasks(log, "synthetic guard control")

    def test_cleanup_attempts_all_owned_children_and_logs_after_one_failure(self):
        calls = []

        class Process:
            def __init__(self, pid):
                self.pid = pid

        class Log:
            def close(self):
                calls.append("log")

        def stop(process):
            calls.append(process.pid)
            if process.pid == 2:
                raise OSError("synthetic cleanup failure")

        run = task.services.Run(None, {})
        run.children = [Process(1), Process(2)]
        run.files = [Log()]
        with patch.object(task.services, "stop", stop):
            with self.assertRaisesRegex(RuntimeError, "cleanup failed"):
                run.close()
        self.assertEqual(calls, [2, 1, "log"])

    def test_journey_probe_reuses_a_real_closed_connection_in_time_wait(self):
        with socket.socket() as listener, socket.socket() as client:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
            listener.listen(1)
            listener.settimeout(2)
            client.settimeout(2)
            client.connect(("127.0.0.1", port))
            accepted, _ = listener.accept()
            accepted.close()  # The server side actively closes and enters TIME_WAIT.
            self.assertEqual(client.recv(1), b"")
        deadline = time.monotonic() + 2
        while True:
            time_wait = any(
                fields[1] == f"0100007F:{port:04X}" and fields[3] == "06"
                for line in Path("/proc/net/tcp").read_text().splitlines()[1:]
                if len(fields := line.split()) >= 4
            )
            if time_wait or time.monotonic() >= deadline:
                break
            time.sleep(0.001)
        self.assertTrue(time_wait, "Control must create a real server-side TIME_WAIT socket")
        with socket.socket() as nonreusable:
            with self.assertRaises(OSError) as rejected:
                nonreusable.bind(("127.0.0.1", port))
            self.assertEqual(rejected.exception.errno, errno.EADDRINUSE)
        task.services.require_journey_port_free(port)
        with socket.socket() as replacement:
            replacement.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            replacement.bind(("127.0.0.1", port))
            replacement.listen(1)
            self.assertTrue(task.services.listener_owned(os.getpid(), port))

    def test_journey_probe_rejects_a_real_active_listener_without_reusing_it(self):
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
            listener.listen(1)
            with self.assertRaisesRegex(RuntimeError, "occupied; refusing to reuse") as rejected:
                task.services.require_journey_port_free(port)
            self.assertEqual(rejected.exception.__cause__.errno, errno.EADDRINUSE)
            self.assertTrue(task.services.listener_owned(os.getpid(), port))


if __name__ == "__main__":
    unittest.main()
