"""Pure score controls; no owner execution, Browser, worker or network.

Run from repository root.
These inputs are authored unit fixtures, not measured upstream browser outcomes.
"""
import copy
import importlib.util
import json
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location("window_score", Path(__file__).with_name("score.py"))
score = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(score)


def accepted_report():
    rows = []
    for path, calls in score.ROSTER:
        counts = dict(total=calls, pass_=0, fail=calls, timeout=0, not_run=0, precondition_failed=0, final_only=0)
        counts["pass"] = counts.pop("pass_")
        rows.append({"test": path, "status": "FAIL", "source_declared_calls": calls,
                     "attempted": True, "owner_accepted": True, "evidence_validated": True, "cleanup_confirmed": True,
                     "raw_harness_status": "Ok", "semantic_decision": {"verdict": "FAIL", "basis": "Assertions",
                         "observed_results": calls, "final_counts": counts}})
    return {"schema_version": 2, "status": "COMPLETE", "complete": True, "baseline_review_eligible": True,
            "infrastructure_errors": [], "expectations_written": False,
            "selected_total": 16, "attempted_total": 16, "completed_total": 16,
            "manifest_sha256": score.MANIFEST, "selection": score.SELECTION, "observer_contract": score.CONTRACT,
            "source_fingerprint": {"sha256": "a" * 64}, "source_fingerprint_after": {"sha256": "a" * 64},
            "build": {"status": "COMPLETE", "binary_verified": True, "completed_within_deadline": True,
                      "process_cleanup_confirmed": True},
            "execution": {"cleanup_confirmed": True, "cancelled": False,
                          "remaining_producer_reserved_bytes": 0, "owner_failure": None}, "results": rows}


class WindowScoreTests(unittest.TestCase):
    def test_fixed_full_manifest_order_counts_and_separate_lane(self):
        manifest = json.loads(Path("tools/wpt/window/corpus.json").read_text())
        self.assertEqual(tuple((row["path"], row["source_declared_call_count"]) for row in manifest["tests"]), score.ROSTER)
        self.assertEqual(sum(count for _, count in score.ROSTER), 79)
        result = score.score_from(accepted_report())
        self.assertEqual(result["summary"]["files"]["total"], 16)
        self.assertEqual(result["summary"]["actual_final_subtests"]["fail"], 79)
        self.assertEqual(result["summary"]["actual_final_subtests"]["total"], 79)
        self.assertNotEqual(result["scope"], "static-reftest-pilot")
        self.assertEqual(result["summary"]["partial_files"], 0)

    def test_volatile_measurements_never_enter_deterministic_score(self):
        first = accepted_report()
        changed = copy.deepcopy(first)
        changed.update(started_utc="other", git_commit="b" * 40, git_dirty=True,
                       binary={"sha256": "c" * 64}, font={"sha256": "d" * 64})
        changed["execution"]["elapsed_seconds"] = 999
        for row in changed["results"]:
            row.update(origin="http://127.0.0.1:49999", owner_clocks={"elapsed_ns": 1},
                       resources={"allocations": 9}, diagnostics=["not a scoring oracle"])
        self.assertEqual(score.score_from(first), score.score_from(changed))

    def test_actual_source_change_changes_score_without_a_self_hash(self):
        first = accepted_report()
        changed = copy.deepcopy(first)
        changed["source_fingerprint"]["sha256"] = "b" * 64
        changed["source_fingerprint_after"]["sha256"] = "b" * 64
        self.assertNotEqual(score.score_from(first), score.score_from(changed))

    def test_owner_and_cleanup_failures_never_produce_score(self):
        mutations = [lambda r: r.update(schema_version=True), lambda r: r.update(status="ERROR"), lambda r: r.update(complete=False),
                     lambda r: r.update(baseline_review_eligible=False), lambda r: r.update(infrastructure_errors=["failure"]),
                     lambda r: r["execution"].update(cleanup_confirmed=False), lambda r: r["execution"].update(cancelled=True),
                     lambda r: r["execution"].update(remaining_producer_reserved_bytes=1),
                     lambda r: r["execution"].update(owner_failure="uncertain"),
                     lambda r: r["build"].update(status="ERROR"), lambda r: r["build"].update(process_cleanup_confirmed=False),
                     lambda r: r["results"][0].update(owner_accepted=False),
                     lambda r: r["results"][0].update(cleanup_confirmed=False),
                     lambda r: r["results"][0].update(owner_failure={"reason": "failed"})]
        for mutate in mutations:
            report = accepted_report()
            mutate(report)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                score.score_from(report)

    def test_frozen_identity_denominator_and_metadata_must_match(self):
        mutations = [lambda r: r["results"].pop(), lambda r: r["results"].reverse(),
                     lambda r: r["results"].__setitem__(1, copy.deepcopy(r["results"][0])),
                     lambda r: r["results"][0].update(source_declared_calls=6),
                     lambda r: r.update(manifest_sha256="0" * 64),
                     lambda r: r.update(observer_contract="other"),
                     lambda r: r["source_fingerprint_after"].update(sha256="b" * 64)]
        for mutate in mutations:
            report = accepted_report()
            mutate(report)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                score.score_from(report)

    def test_ineligible_statuses_cannot_be_repaired_by_counts_or_clean_flags(self):
        for status in ("ERROR", "CRASH", "PENDING", "UNKNOWN"):
            report = accepted_report()
            report["results"][0]["status"] = status
            report["results"][0]["semantic_decision"]["verdict"] = status
            with self.subTest(status=status), self.assertRaises(ValueError):
                score.score_from(report)

    def test_clean_genuine_timeout_keeps_actual_final_inventory(self):
        report = accepted_report()
        row = report["results"][0]
        row.update(status="TIMEOUT", raw_harness_status="Timeout")
        row["semantic_decision"].update(verdict="TIMEOUT", basis="HarnessTimeout")
        result = score.score_from(report)
        self.assertEqual(result["summary"]["files"]["TIMEOUT"], 1)
        self.assertEqual(result["results"][0]["final_counts"]["total"], 7)
        score.check_ratchet(result, score.initial_ratchet())

    def test_missing_completion_never_invents_unobserved_subtests(self):
        report = accepted_report()
        row = report["results"][0]
        row.update(status="TIMEOUT", raw_harness_status=None)
        row["semantic_decision"].update(verdict="TIMEOUT", basis="MissingCompletion", final_counts=None, observed_results=2)
        result = score.score_from(report)
        self.assertIsNone(result["results"][0]["final_counts"])
        self.assertEqual(result["results"][0]["observed_results"], 2)
        self.assertEqual(result["summary"]["actual_final_subtests"]["total"], 72)
        self.assertEqual(result["summary"]["partial_files"], 1)
        self.assertEqual(result["summary"]["source_prospective_calls"], 79)
        self.assertEqual(result["summary"]["partial_unobserved_subtests"], "unknown")
        score.check_ratchet(result, score.initial_ratchet())

    def test_unsettled_completion_retains_actual_completion_inventory(self):
        report = accepted_report()
        row = report["results"][0]
        row.update(status="TIMEOUT", raw_harness_status="Ok")
        row["semantic_decision"].update(verdict="TIMEOUT", basis="UnsettledCompletion")
        row["semantic_decision"]["final_counts"].update(fail=0)
        row["semantic_decision"]["final_counts"]["pass"] = 7
        result = score.score_from(report)
        self.assertEqual(result["results"][0]["raw_harness_status"], "Ok")
        self.assertEqual(result["results"][0]["final_counts"]["pass"], 7)
        self.assertEqual(result["summary"]["files"]["TIMEOUT"], 1)
        self.assertEqual(result["summary"]["actual_final_subtests"]["total"], 79)
        self.assertEqual(result["summary"]["partial_files"], 0)
        score.check_ratchet(result, score.initial_ratchet())

    def test_precondition_and_unsupported_remain_distinct_not_expected_passes(self):
        report = accepted_report()
        row = report["results"][0]
        row.update(status="PRECONDITION_FAILED", raw_harness_status="PreconditionFailed")
        row["semantic_decision"].update(verdict="PRECONDITION_FAILED", basis="PreconditionFailed")
        row["semantic_decision"]["final_counts"].update(fail=0, precondition_failed=7)
        result = score.score_from(report)
        self.assertEqual(result["summary"]["files"]["PRECONDITION_FAILED"], 1)
        score.check_ratchet(result, score.initial_ratchet())
        row.update(status="UNSUPPORTED", raw_harness_status=None)
        row["semantic_decision"].update(verdict="UNSUPPORTED", basis={"Unsupported": "ModuleScript"},
                                        final_counts=None, observed_results=0)
        result = score.score_from(report)
        self.assertEqual(result["summary"]["files"]["UNSUPPORTED"], 1)
        with self.assertRaises(ValueError):
            score.check_ratchet(result, score.initial_ratchet())

    def test_no_auto_ratchet_and_required_passes_cannot_regress(self):
        result = score.score_from(accepted_report())
        ratchet = score.initial_ratchet()
        before = copy.deepcopy(ratchet)
        score.check_ratchet(result, ratchet)
        self.assertEqual(ratchet, before)
        self.assertEqual(ratchet["required_passes"], [])
        self.assertEqual(ratchet["required_runnable"], list(score.TESTS))
        ratchet["required_passes"] = [score.TESTS[0]]
        with self.assertRaises(ValueError):
            score.check_ratchet(result, ratchet)
        passing = accepted_report()
        passing["results"][0]["status"] = "PASS"
        passing["results"][0]["semantic_decision"].update(verdict="PASS", basis="CleanCompletion")
        passing["results"][0]["semantic_decision"]["final_counts"].update(pass_=7)
        counts = passing["results"][0]["semantic_decision"]["final_counts"]
        counts["pass"] = counts.pop("pass_")
        counts["fail"] = 0
        score.check_ratchet(score.score_from(passing), ratchet)

    def test_strict_score_counts_scalar_types_and_extra_fields(self):
        base = score.score_from(accepted_report())
        mutations = [lambda s: s["summary"]["files"].update(FAIL=15),
                     lambda s: s["summary"]["files"].update(ERROR=False),
                     lambda s: s["results"][0]["final_counts"].update(fail=True),
                     lambda s: s["results"][0]["final_counts"].update(total=8),
                     lambda s: s["results"][0].update(raw_callback_payload={}),
                     lambda s: s.update(git_commit="a" * 40)]
        for mutate in mutations:
            candidate = copy.deepcopy(base)
            mutate(candidate)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                score.validate_score(candidate)


if __name__ == "__main__":
    unittest.main()
