#!/usr/bin/env python3
"""Synthesized checkers and owned local process negatives, not browser results."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "message_smoke", Path(__file__).with_name("message-services-smoke.py"))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def cleanup_spawned(processes):
    # Test safety only: assertions inspect the production cleanup result first.
    # A broken regression must not leave even its four-second fixture alive.
    for process in processes:
        if process.returncode is None:
            try:
                smoke.os.killpg(process.pid, smoke.signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait(timeout=2)


class Controls(unittest.TestCase):
    def reports(self):
        source = smoke.FIXTURE.read_bytes().split(b"<script>", 1)[1].split(b"</script>", 1)[0]
        memory = dict(outstanding_bytes=1000, peak_bytes=2000, cumulative_bytes=3000,
                      allocations=20, outstanding_limit_bytes=32 * 1024 * 1024,
                      cumulative_limit_bytes=64 * 1024 * 1024)
        common = dict(profile="boa-page-process-v1", fatal_reason=None,
                      source_admissions=1, source_bytes=len(source))
        return [dict(common, opcodes_remaining=999900, jobs_admitted=3, jobs_executed=2,
                     pending_jobs=1, worker_memory=dict(memory)),
                dict(common, opcodes_remaining=999800, jobs_admitted=5, jobs_executed=5,
                     pending_jobs=0, worker_memory=dict(memory))]

    def log(self, reports):
        return "\n".join([*("SCRIPT_BOA " + json.dumps(report) for report in reports),
                          "SCRIPT_COMPLETE executed=1", "PAGE_TASK_APPLIED sequence=1 revision=1", ""])

    def client_log(self):
        return "\n".join(["CDP_EVENTS_STATE link canceled", "CDP_EVENTS_STATE first submit canceled",
                          "CDP_EVENTS_DESTINATION", "CDP_EVENTS_OK",
                          f"CDP_WINDOW_MESSAGE_OK {smoke.PROOF} checkpoint={smoke.CHECKPOINT}", ""])

    def test_frozen_fixture_and_checkpoint_inputs_are_explicit(self):
        self.assertEqual(smoke.FIXTURE_SHA256,
                         "9b32f5cbd533f0d836cb09a9d9651331ce44b7f81588173789e66e743ced0c96")
        self.assertEqual(smoke.services.sha256(smoke.FIXTURE), smoke.FIXTURE_SHA256)
        self.assertEqual(smoke.FIXTURE_URL, "http://127.0.0.1:7878/message-events")
        self.assertEqual(smoke.PROOF, "deep-clone-identity-noncancelable")
        self.assertEqual(smoke.CHECKPOINT, "script,after-post,script-microtask,load,load-microtask,property,"
                         "property-microtask,listener,listener-microtask")

    def test_exact_nonfatal_queued_and_settled_worker_evidence_is_accepted(self):
        evidence = smoke.check_messages(self.log(self.reports()), "synthetic guard")
        self.assertEqual(evidence["init_work"], [3, 2, 1])
        self.assertEqual(evidence["settled_work"], [5, 5, 0])
        self.assertEqual(evidence["applied_tasks"], 1)
        self.assertEqual(evidence["emitted_script_diagnostics"], 0)

    def test_missing_or_extra_parent_task_and_worker_evidence_is_rejected(self):
        valid = self.log(self.reports())
        for changed in (valid.replace("PAGE_TASK_APPLIED ", "NOT_A_TASK "),
                        valid + "PAGE_TASK_APPLIED sequence=2 revision=2\n",
                        self.log(self.reports()[:1]),
                        valid.replace("SCRIPT_BOA ", "NOT_A_WORKER ")):
            with self.subTest(changed=changed):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(changed, "synthetic guard")

    def test_every_emitted_failure_family_invalidates_clean_message_acceptance(self):
        for marker in ("SCRIPT_PARTIAL", "SCRIPT_DIAGNOSTIC", "SCRIPT_ERROR", "SCRIPT_REJECTED",
                       "SCRIPT_SESSION_CLOSED", "SCRIPT_SHUTDOWN_ERROR", "LOAD_ERROR",
                       "RESOURCE_WARNING", "RESOURCE_DIAGNOSTIC", "JOURNEY_INCOMPLETE"):
            with self.subTest(marker=marker):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log(self.reports()) + marker + " owned failure\n", "synthetic guard")

    def test_init_cannot_hide_early_delivery_or_missing_pending_admission(self):
        for admitted, executed, pending in ((3, 3, 0), (2, 2, 0), (3, 1, 2), (3, 2, 0)):
            before, after = self.reports()
            before.update(jobs_admitted=admitted, jobs_executed=executed, pending_jobs=pending)
            with self.subTest(work=(admitted, executed, pending)):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log([before, after]), "synthetic guard")

    def test_final_message_work_must_settle_exactly_without_extra_tasks(self):
        for admitted, executed, pending in ((5, 4, 1), (6, 6, 0), (4, 4, 0), (5, 5, 1)):
            before, after = self.reports()
            after.update(jobs_admitted=admitted, jobs_executed=executed, pending_jobs=pending)
            with self.subTest(work=(admitted, executed, pending)):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log([before, after]), "synthetic guard")

    def test_fatal_changed_profile_missing_scalar_and_boolean_accounting_are_rejected(self):
        for change in ("fatal", "missing-fatal", "profile", "source", "missing-counter", "boolean"):
            before, after = self.reports()
            if change == "fatal":
                after["fatal_reason"] = "synthetic failure"
            elif change == "missing-fatal":
                del after["fatal_reason"]
            elif change == "profile":
                after["profile"] = "unreviewed-profile"
            elif change == "source":
                after["source_bytes"] += 1
            elif change == "missing-counter":
                del after["jobs_admitted"]
            else:
                after["pending_jobs"] = False
            with self.subTest(change=change):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log([before, after]), "synthetic guard")

    def test_original_realm_opcode_and_memory_counters_cannot_renew(self):
        for field in ("opcodes_remaining", "peak_bytes", "cumulative_bytes", "allocations"):
            before, after = self.reports()
            if field == "opcodes_remaining":
                after[field] = before[field] + 1
            else:
                # Each individual report remains internally valid, so only the
                # cross-transaction monotonicity check can reject this renewal.
                before["worker_memory"][field] += 1
            with self.subTest(field=field):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log([before, after]), "synthetic guard")

    def test_unchanged_worker_caps_are_required_not_merely_positive_numbers(self):
        for field in ("outstanding_limit_bytes", "cumulative_limit_bytes"):
            before, after = self.reports()
            after["worker_memory"][field] += 1
            with self.subTest(field=field):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log([before, after]), "synthetic guard")

    def test_other_page_reports_cannot_hide_unknown_sources_or_pending_work(self):
        before, after = self.reports()
        empty = dict(after, source_admissions=0, source_bytes=0,
                     jobs_admitted=0, jobs_executed=0, pending_jobs=0)
        smoke.check_messages(self.log([before, after, empty]), "synthetic guard")
        for changed in (dict(empty, source_admissions=1, source_bytes=1),
                        dict(empty, jobs_admitted=1, pending_jobs=1),
                        dict(empty, jobs_admitted=1, jobs_executed=1)):
            with self.subTest(changed=changed):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log([before, after, changed]), "synthetic guard")

    def test_exact_cdp_message_proof_preserves_original_retained_event_markers(self):
        smoke.check_client(self.client_log())

    def test_missing_wrong_duplicated_proof_or_retained_event_markers_fail(self):
        valid = self.client_log()
        proof = f"CDP_WINDOW_MESSAGE_OK {smoke.PROOF} checkpoint={smoke.CHECKPOINT}"
        for changed in (valid.replace(proof, ""), valid + proof + "\n",
                        valid.replace(smoke.PROOF, "unproven clone"),
                        valid.replace("property-microtask,listener", "listener,property-microtask"),
                        valid.replace("CDP_EVENTS_STATE link canceled", ""),
                        valid.replace("CDP_EVENTS_STATE first submit canceled", ""),
                        valid.replace("CDP_EVENTS_DESTINATION", ""), valid.replace("CDP_EVENTS_OK", "")):
            with self.subTest(changed=changed):
                with self.assertRaises(RuntimeError):
                    smoke.check_client(changed)

    def test_evidence_total_reserves_report_capacity_without_raising_the_total_cap(self):
        files = [f"owned-{i}.png" for i in range(7)]
        info = SimpleNamespace(st_mode=smoke.stat.S_IFREG, st_size=smoke.PNG_LIMIT)
        with patch.object(smoke.os, "walk", return_value=[("/owned", [], files)]), \
                patch.object(smoke.Path, "lstat", return_value=info):
            self.assertEqual(smoke.evidence_size(Path("/owned")), 7 * smoke.PNG_LIMIT)
        files.append("owned-7.png")
        with patch.object(smoke.os, "walk", return_value=[("/owned", [], files)]), \
                patch.object(smoke.Path, "lstat", return_value=info):
            with self.assertRaisesRegex(RuntimeError, "reserved report"):
                smoke.evidence_size(Path("/owned"))

    def test_final_evidence_admission_rejects_oversize_and_nonregular_artifacts(self):
        for mode, size in ((smoke.stat.S_IFREG, smoke.PNG_LIMIT + 1),
                           (smoke.stat.S_IFLNK, 1), (smoke.stat.S_IFIFO, 1)):
            info = SimpleNamespace(st_mode=mode, st_size=size)
            with self.subTest(mode=mode, size=size), \
                    patch.object(smoke.os, "walk", return_value=[("/owned", [], ["owned.png"])]), \
                    patch.object(smoke.Path, "lstat", return_value=info):
                with self.assertRaises(RuntimeError):
                    smoke.evidence_size(Path("/owned"))

    def test_postspawn_owner_failure_kills_exact_group_and_reaps_known_leader(self):
        real_popen = smoke.subprocess.Popen
        spawned = []

        def launch(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            spawned.append(process)
            return process

        with tempfile.TemporaryDirectory(prefix="message-handoff-negative-", dir=smoke.REPO / "tmp") as directory:
            root = Path(directory)
            run = smoke.Run(root, {"PATH": "/usr/bin:/bin", "HOME": str(root)})
            try:
                with patch.object(smoke.subprocess, "Popen", side_effect=launch), \
                        patch.object(smoke, "OwnedProcess", side_effect=RuntimeError("owned postspawn failure")):
                    with self.assertRaisesRegex(RuntimeError, "owned postspawn failure"):
                        run.start(["/usr/bin/python3", "-c", "import time;time.sleep(4)"], "child.log")
                self.assertEqual(len(spawned), 1)
                self.assertIsNotNone(spawned[0].returncode)
                self.assertEqual(run.children, [])
                self.assertTrue(run.cleanup_confirmed)
                self.assertTrue(run.handoff_failures[0]["cleanup_succeeded"])
                with self.assertRaises(ProcessLookupError):
                    smoke.os.killpg(spawned[0].pid, 0)
            finally:
                try:
                    run.close()
                finally:
                    cleanup_spawned(spawned)

    def test_interrupt_after_popen_return_is_raised_only_after_owner_is_registered(self):
        real_popen = smoke.subprocess.Popen
        spawned = []
        saved = [smoke.signal.getsignal(item) for item in (smoke.signal.SIGINT, smoke.signal.SIGTERM)]

        def interrupted_launch(*args, **kwargs):
            process = real_popen(*args, **kwargs)
            spawned.append(process)
            smoke.signal.raise_signal(smoke.signal.SIGINT)
            return process

        with tempfile.TemporaryDirectory(prefix="message-handoff-interrupt-", dir=smoke.REPO / "tmp") as directory:
            root = Path(directory)
            run = smoke.Run(root, {"PATH": "/usr/bin:/bin", "HOME": str(root)})
            try:
                with patch.object(smoke.subprocess, "Popen", side_effect=interrupted_launch):
                    with self.assertRaisesRegex(KeyboardInterrupt, "handoff secured"):
                        run.start(["/usr/bin/python3", "-c", "import time;time.sleep(4)"], "child.log")
                self.assertEqual(len(run.children), 1)
                self.assertEqual(run.deferred_signal, smoke.signal.SIGINT)
                self.assertEqual(saved, [smoke.signal.getsignal(item)
                                        for item in (smoke.signal.SIGINT, smoke.signal.SIGTERM)])
            finally:
                try:
                    run.close()
                finally:
                    cleanup_spawned(spawned)
            self.assertIsNotNone(run.children[0].process.returncode)
            with self.assertRaises(ProcessLookupError):
                smoke.os.killpg(run.children[0].pid, 0)


if __name__ == "__main__":
    unittest.main()
