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
        return [dict(common, opcodes_remaining=999900 - i, jobs_admitted=admitted,
                     jobs_executed=executed, pending_jobs=pending, worker_memory=dict(memory))
                for i, (admitted, executed, pending) in enumerate(
                    [(3, 1, 2), (3, 2, 1), (5, 4, 1), (7, 7, 0)])]

    def log(self, reports):
        tasks = sum(a.get("source_admissions") == b.get("source_admissions")
                    and a.get("source_bytes") == b.get("source_bytes")
                    and a.get("jobs_executed", 0) < b.get("jobs_executed", 0)
                    for a, b in zip(reports, reports[1:]))
        return "\n".join([*("SCRIPT_BOA " + json.dumps(report) for report in reports),
                          "SCRIPT_COMPLETE executed=1",
                          *(f"PAGE_TASK_APPLIED sequence={i} revision={i}" for i in range(1, tasks + 1)), ""])

    def client_log(self):
        return "\n".join(["CDP_EVENTS_STATE link canceled", "CDP_EVENTS_STATE first submit canceled",
                          "CDP_EVENTS_DESTINATION", "CDP_EVENTS_OK",
                          f"CDP_WINDOW_MESSAGE_OK {smoke.PROOF} checkpoint={smoke.CHECKPOINT}", ""])

    def test_frozen_fixture_and_checkpoint_inputs_are_explicit(self):
        self.assertEqual(smoke.FIXTURE_SHA256,
                         "5f5a7276865dbd635f87f8e2dc175e3e12f9fd7fb4d27d71d149fa5dccf3580d")
        self.assertEqual(smoke.services.sha256(smoke.FIXTURE), smoke.FIXTURE_SHA256)
        self.assertEqual(smoke.FIXTURE_URL, "http://127.0.0.1:7878/message-events")
        self.assertEqual(smoke.PROOF, "deep-clone-identity-noncancelable")
        self.assertEqual(smoke.CHECKPOINT, "script,script-microtask,load,after-post,load-microtask,property,"
                         "property-microtask,listener,listener-microtask")

    def test_exact_nonfatal_queued_and_settled_worker_evidence_is_accepted(self):
        evidence = smoke.check_messages(self.log(self.reports()), "synthetic guard")
        self.assertEqual(evidence["init_work"], [3, 1, 2])
        self.assertEqual(evidence["load_work"], [5, 4, 1])
        self.assertEqual(evidence["settled_work"], [7, 7, 0])
        self.assertEqual(evidence["applied_tasks"], 3)
        self.assertEqual(evidence["fixture_applied_tasks"], 3)
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
        for admitted, executed, pending in ((3, 3, 0), (2, 2, 0), (3, 2, 1), (3, 2, 0)):
            reports = self.reports()
            before, after = reports[0], reports[-1]
            before.update(jobs_admitted=admitted, jobs_executed=executed, pending_jobs=pending)
            with self.subTest(work=(admitted, executed, pending)):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log(reports), "synthetic guard")

    def test_final_message_work_must_settle_exactly_without_extra_tasks(self):
        for admitted, executed, pending in ((7, 6, 1), (8, 8, 0), (6, 6, 0), (7, 7, 1)):
            reports = self.reports()
            before, after = reports[0], reports[-1]
            after.update(jobs_admitted=admitted, jobs_executed=executed, pending_jobs=pending)
            with self.subTest(work=(admitted, executed, pending)):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log(reports), "synthetic guard")

    def test_fatal_changed_profile_missing_scalar_and_boolean_accounting_are_rejected(self):
        for change in ("fatal", "missing-fatal", "profile", "source", "missing-counter", "boolean"):
            reports = self.reports()
            before, after = reports[0], reports[-1]
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
                    smoke.check_messages(self.log(reports), "synthetic guard")

    def test_original_realm_opcode_and_memory_counters_cannot_renew(self):
        for field in ("opcodes_remaining", "peak_bytes", "cumulative_bytes", "allocations"):
            reports = self.reports()
            before, after = reports[0], reports[-1]
            if field == "opcodes_remaining":
                after[field] = before[field] + 1
            else:
                # Each individual report remains internally valid, so only the
                # cross-transaction monotonicity check can reject this renewal.
                before["worker_memory"][field] += 1
            with self.subTest(field=field):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log(reports), "synthetic guard")

    def test_unchanged_worker_caps_are_required_not_merely_positive_numbers(self):
        for field in ("outstanding_limit_bytes", "cumulative_limit_bytes"):
            reports = self.reports()
            before, after = reports[0], reports[-1]
            after["worker_memory"][field] += 1
            with self.subTest(field=field):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log(reports), "synthetic guard")

    def test_other_page_reports_cannot_hide_unknown_sources_or_pending_work(self):
        reports = self.reports()
        after = reports[-1]
        empty = dict(after, source_admissions=0, source_bytes=0,
                     jobs_admitted=2, jobs_executed=0, pending_jobs=2)
        other = [empty, dict(empty, jobs_executed=1, pending_jobs=1), dict(empty, jobs_executed=2, pending_jobs=0)]
        smoke.check_messages(self.log([*reports, *other]), "synthetic guard")
        for changed in (dict(empty, source_admissions=1, source_bytes=1),
                        dict(empty, jobs_admitted=3, pending_jobs=3),
                        dict(empty, jobs_admitted=3, jobs_executed=1)):
            with self.subTest(changed=changed):
                with self.assertRaises(RuntimeError):
                    smoke.check_messages(self.log([*reports, changed]), "synthetic guard")

    def test_exact_cdp_message_proof_preserves_original_retained_event_markers(self):
        smoke.check_client(self.client_log())

    def test_dcl_and_load_transitions_cannot_be_missing_merged_or_repeated(self):
        reports = self.reports()
        for changed in (reports[:1] + reports[2:], reports[:2] + reports[3:],
                        reports[:2] + reports[1:2] + reports[2:]):
            with self.subTest(work=[(item['jobs_admitted'], item['jobs_executed'], item['pending_jobs'])
                                    for item in changed]), self.assertRaises(RuntimeError):
                smoke.check_messages(self.log(changed), 'synthetic lifecycle guard')

    def test_only_the_initial_empty_homepage_can_be_a_superseded_prefix(self):
        reports = self.reports()
        empty = dict(reports[0], source_admissions=0, source_bytes=0,
                     jobs_admitted=2, jobs_executed=0, pending_jobs=2)
        dcl = dict(empty, jobs_executed=1, pending_jobs=1)
        for prefix in ([empty], [empty, dcl]):
            smoke.check_messages(self.log([*prefix, *reports]), 'synthetic supersession guard')
        for changed in ([*reports, empty], [*reports, empty, dcl],
                        [dict(empty, source_admissions=1, source_bytes=1), *reports]):
            with self.subTest(changed=changed), self.assertRaises(RuntimeError):
                smoke.check_messages(self.log(changed), 'synthetic supersession guard')

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
