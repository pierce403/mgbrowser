#!/usr/bin/env python3
"""Synthetic evidence-checker controls, not browser or WPT results."""
import importlib.util
import json
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("rejection_smoke", Path(__file__).with_name("rejection-services-smoke.py"))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class Controls(unittest.TestCase):
    def reports(self):
        source = smoke.FIXTURE.read_text().split("<script>", 1)[1].split("</script>", 1)[0]
        memory = dict(outstanding_bytes=1000, peak_bytes=2000, cumulative_bytes=3000,
                      allocations=20, outstanding_limit_bytes=32 * 1024 * 1024,
                      cumulative_limit_bytes=64 * 1024 * 1024)
        common = dict(profile="boa-page-process-v1", fatal_reason=None,
                      source_admissions=1, source_bytes=len(source.encode()))
        return [dict(common, opcodes_remaining=999900 - i, jobs_admitted=admitted,
                     jobs_executed=executed, pending_jobs=pending, worker_memory=dict(memory))
                for i, (admitted, executed, pending) in enumerate(
                    [(3, 1, 2), (3, 2, 1), (4, 3, 1), (6, 6, 0)])]

    def log(self, reports):
        return "\n".join([*("SCRIPT_BOA " + json.dumps(report) for report in reports),
                          "PAGE_TASK_APPLIED sequence=1 revision=1",
                          "PAGE_TASK_APPLIED sequence=2 revision=2",
                          "PAGE_TASK_APPLIED sequence=3 revision=3",
                          'SCRIPT_DIAGNOSTIC index=1 message="Unhandled Promise rejection"',
                          'SCRIPT_PARTIAL executed=0 errors=1 first="Unhandled Promise rejection"', ""])

    def test_exact_retained_nonfatal_evidence_is_accepted(self):
        smoke.check_rejections(self.log(self.reports()), "synthetic guard")

    def test_missing_extra_or_erased_error_and_task_evidence_is_rejected(self):
        valid = self.log(self.reports())
        for changed in (valid.replace("SCRIPT_DIAGNOSTIC ", "NOT_A_DIAGNOSTIC "),
                        valid.replace("SCRIPT_PARTIAL ", "NOT_PARTIAL "),
                        valid.replace("PAGE_TASK_APPLIED ", "NOT_A_TASK "),
                        valid + 'SCRIPT_DIAGNOSTIC index=1 message="Unhandled Promise rejection"\n',
                        valid + 'PAGE_TASK_APPLIED sequence=2 revision=2\n',
                        valid + 'SCRIPT_REJECTED wrong\n',
                        valid.replace("Unhandled Promise rejection", "some other failure")):
            with self.subTest(changed=changed):
                with self.assertRaises(RuntimeError):
                    smoke.check_rejections(changed, "synthetic guard")

    def test_pending_renewed_changed_or_fatal_accounting_is_rejected(self):
        for change in ("early", "pending", "extra", "source", "opcodes", "memory", "cap", "fatal"):
            reports = self.reports()
            before, after = reports[0], reports[-1]
            if change == "early":
                before.update(jobs_executed=3, pending_jobs=0)
            elif change == "pending":
                after["pending_jobs"] = 1
            elif change == "extra":
                after.update(jobs_admitted=7, jobs_executed=7)
            elif change == "source":
                after["source_bytes"] += 1
            elif change == "opcodes":
                after["opcodes_remaining"] = before["opcodes_remaining"] + 1
            elif change == "memory":
                after["worker_memory"]["cumulative_bytes"] -= 1
            elif change == "cap":
                after["worker_memory"]["outstanding_limit_bytes"] += 1
            elif change == "fatal":
                after["fatal_reason"] = "synthetic failure"
            with self.subTest(change=change):
                with self.assertRaises(RuntimeError):
                    smoke.check_rejections(self.log(reports), "synthetic guard")


if __name__ == "__main__":
    unittest.main()
