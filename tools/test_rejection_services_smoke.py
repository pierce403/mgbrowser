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
        return [dict(common, opcodes_remaining=999900, jobs_admitted=2, jobs_executed=1,
                     pending_jobs=1, worker_memory=dict(memory)),
                dict(common, opcodes_remaining=999800, jobs_admitted=4, jobs_executed=4,
                     pending_jobs=0, worker_memory=dict(memory))]

    def log(self, reports):
        return "\n".join([*("SCRIPT_BOA " + json.dumps(report) for report in reports),
                          "PAGE_TASK_APPLIED sequence=1 revision=1",
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
            before, after = self.reports()
            if change == "early":
                before.update(jobs_executed=2, pending_jobs=0)
            elif change == "pending":
                after["pending_jobs"] = 1
            elif change == "extra":
                after.update(jobs_admitted=5, jobs_executed=5)
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
                    smoke.check_rejections(self.log([before, after]), "synthetic guard")


if __name__ == "__main__":
    unittest.main()
