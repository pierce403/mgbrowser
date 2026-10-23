"""Pure owner controls; never build or launch Browser/worker/network services."""
import importlib.util
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("window_report_owner", Path(__file__).with_name("run.py"))
owner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(owner)


class ShapeTests(unittest.TestCase):
    def test_all_sixteen_slots_exist_before_any_input_or_build(self):
        rows = owner.pending_results()
        self.assertEqual(len(rows), 16)
        self.assertEqual([row["test"] for row in rows], sorted(owner.CASE_PATHS))
        self.assertEqual([row["run_id"] for row in rows], list(range(1, 17)))
        self.assertTrue(all(row["status"] == "PENDING" and row["final_subtests"] is None for row in rows))

    def test_closed_json_rejects_duplicate_nonfinite_invalid_utf8_and_large_integer(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}', b'"\xff"',
                    b'{"x":123456789012345678901}', b'{"x":1e999}'):
            with self.subTest(raw=raw), self.assertRaises(owner.Invalid):
                owner.strict_json(raw, 1024)

    def test_type_exact_scalars_reject_boolean_integer_alias(self):
        with self.assertRaises(owner.Invalid):
            owner.integer(True, 1, "count")
        with self.assertRaises(owner.Invalid):
            owner.boolean(0, "flag")
        for bad in ({"x": 1, "extra": 2}, {}, [], None):
            with self.subTest(bad=bad), self.assertRaises(owner.Invalid):
                owner.fields(bad, ("x",), "object")

    def test_partials_remain_unknown_not_synthetic_not_run_rows(self):
        rows = owner.pending_results()
        rows[0]["status"] = "TIMEOUT"
        summary = owner.distributions(rows)
        self.assertEqual(summary["files"]["PENDING"], 15)
        self.assertEqual(summary["attempted_partial_files"], 1)
        self.assertEqual(summary["files_with_final_inventory"], 0)
        self.assertEqual(sum(summary["actual_final_subtests"].values()), 0)
        self.assertEqual(summary["partial_unobserved_subtests"], "unknown")
        self.assertEqual(summary["source_prospective_calls"], 79)

    def test_final_subtest_statuses_count_once_and_are_not_file_statuses(self):
        rows = owner.pending_results()
        rows[0].update(status="FAIL", final_subtests={"Pass": 1, "Fail": 1, "Timeout": 1,
                                                     "NotRun": 1, "PreconditionFailed": 1})
        summary = owner.distributions(rows)
        self.assertEqual(summary["files"]["FAIL"], 1)
        self.assertEqual(summary["actual_final_subtests"], {name: 1 for name in owner.SUBTEST_STATUSES})
        self.assertEqual(summary["files_with_final_inventory"], 1)

    def test_only_normal_zero_completed_confirmed_process_is_adoptable(self):
        good = {"status": "OK", "reason": "", "returncode": 0, "elapsed_ms": 1,
                "output_bytes": 0, "completed": True, "cleanup_succeeded": True}
        owner.verify_process(good)
        for key, value in (("status", "CRASH"), ("returncode", 1), ("completed", False),
                           ("cleanup_succeeded", False), ("cleanup_succeeded", None)):
            with self.subTest(key=key, value=value), self.assertRaises(owner.Invalid):
                owner.verify_process(dict(good, **{key: value}))
        missing = dict(good)
        del missing["cleanup_succeeded"]
        with self.assertRaises(owner.Invalid):
            owner.verify_process(missing)

    def test_fully_evidenced_semantic_error_keeps_exit1_not_normal_zero(self):
        result = {"status": "ERROR", "reason": "Process exit 1", "returncode": 1, "elapsed_ms": 2,
                  "output_bytes": 0, "completed": True, "cleanup_succeeded": True}
        owner.verify_process(result, semantic_error=True)
        with self.assertRaises(owner.Invalid):
            owner.verify_process(result)
        for key, value in (("returncode", 2), ("completed", False), ("cleanup_succeeded", False), ("returncode", False)):
            with self.subTest(key=key, value=value), self.assertRaises(owner.Invalid):
                owner.verify_process(dict(result, **{key: value}), semantic_error=True)

    def test_cargo_uses_exact_normal_example_artifact_not_hardcoded_binary(self):
        event = {"reason": "compiler-artifact", "target": {"name": "wpt_window", "kind": ["example"]},
                 "features": [], "executable": "/tmp/fixed-window-host"}
        self.assertEqual(owner.cargo_executable(json.dumps(event).encode()), Path("/tmp/fixed-window-host"))
        for changed in (dict(event, features=["legacy-test-engine"]),
                        dict(event, target={"name": "mgbrowser", "kind": ["bin"]})):
            with self.assertRaises(owner.Invalid):
                owner.cargo_executable(json.dumps(changed).encode())
        with self.assertRaises(owner.Invalid):
            owner.cargo_executable((json.dumps(event) + "\n" + json.dumps(event)).encode())

    def test_owner_clock_charges_setup_monitoring_and_artifact_validation(self):
        clock = {"started_monotonic_ns": 100, "cursor_monotonic_ns": 1_000_000_100,
                 "elapsed_ns": 1_000_000_000, "used_active_ns": 100_000_000,
                 "excluded_idle_ns": 900_000_000, "excluded_cleanup_ns": 0,
                 "whole_deadline_monotonic_ns": 25_000_000_000,
                 "admission_deadline_monotonic_ns": 22_000_000_000,
                 "report_deadline_monotonic_ns": 22_900_000_000,
                 "sums_valid": True, "admission_stopped": True, "report_boundary_checked": True}
        value = owner.validate_clock(clock, 0, 25_000_000_000, 2_000_000_000)
        self.assertEqual(value["used_active_ns"], 1_100_000_000)
        with self.assertRaises(owner.Invalid):
            owner.validate_clock(clock, 0, 25_000_000_000, 3_000_000_001)
        with self.assertRaises(owner.Invalid):
            owner.validate_clock(dict(clock, elapsed_ns=1), 0, 25_000_000_000, 2_000_000_000)
        with self.assertRaises(owner.Invalid):
            owner.validate_clock(dict(clock, started_monotonic_ns=True), 0, 25_000_000_000, 2_000_000_000)

    def test_optional_text_never_collapses_undefined_null_or_surrogates(self):
        self.assertEqual(owner.optional_text({"kind": "Undefined"}, "text"), 0)
        self.assertEqual(owner.optional_text({"kind": "Null"}, "text"), 0)
        self.assertEqual(owner.optional_text({"kind": "Text", "units": [0xD800]}, "text"), 1)
        for bad in ({"kind": "Null", "units": []}, {"kind": "Text", "units": [True]},
                    {"kind": "Text", "units": [65536]}, {"kind": "Unknown"}):
            with self.subTest(bad=bad), self.assertRaises(owner.Invalid):
                owner.optional_text(bad, "text")

    def test_actual_final_only_negative_rows_count_once_without_fake_result(self):
        rows = [{"id": index + 1, "index": index, "name": [97], "status": status,
                 "message": {"kind": "Null"}, "stack": {"kind": "Undefined"}}
                for index, status in enumerate(owner.SUBTEST_STATUSES)]
        records = [{"kind": "Completion", "harness": {"status": "Timeout", "message": {"kind": "Null"},
                    "stack": {"kind": "Undefined"}}, "inventory": rows}]
        summary = owner.record_inventory(records)
        self.assertEqual(summary["observed_results"], 0)
        self.assertEqual(summary["completion_record"], 0)
        self.assertEqual(summary["final_only"], 5)
        self.assertEqual(summary["final_counts"], {status: 1 for status in owner.SUBTEST_STATUSES})
        self.assertEqual(summary["raw_harness_status"], "Timeout")

    def test_cumulative_system_fields_are_actual_and_no_limits_are_waived(self):
        memory = {"outstanding_bytes": 20, "peak_bytes": 40, "cumulative_bytes": 80, "allocations": 2,
                  "outstanding_limit_bytes": 32 * 1024 * 1024, "cumulative_limit_bytes": 64 * 1024 * 1024}
        report = {"profile": "boa-page-process-v1", "opcodes_remaining": 999_000,
                  "source_bytes": 200_000, "source_admissions": 4, "jobs_admitted": 5,
                  "jobs_executed": 3, "pending_jobs": 1, "fatal_reason": None, "worker_memory": memory}
        owner.validate_resources(report, None)
        with self.assertRaises(owner.Invalid):
            owner.validate_resources(dict(report, jobs_admitted=2049), None)
        with self.assertRaises(owner.Invalid):
            owner.validate_resources(dict(report, worker_memory=dict(memory, outstanding_limit_bytes=64 * 1024 * 1024)), None)
        with self.assertRaises(owner.Invalid):
            owner.validate_resources(dict(report, source_bytes=199_999), report)
        with self.assertRaises(owner.Invalid):
            owner.validate_resources(dict(report, worker_memory=dict(memory, cumulative_bytes=79)), report)

    def test_host_internal_cleanup_cannot_claim_outer_group_proof(self):
        cleanup = {key: True for key in ("runtime_closed", "pool_wait_idle", "initial_producer_quiescent", "fixture_sealed", "fixture_drained",
                   "fixture_joined", "internal_confirmed", "outer_owner_required")}
        self.assertTrue(owner.internal_cleanup_confirmed({"cleanup": cleanup}))
        self.assertFalse(owner.internal_cleanup_confirmed({"cleanup": dict(cleanup, pool_wait_idle=False)}))
        with self.assertRaises(owner.Invalid):
            owner.internal_cleanup_confirmed({"cleanup": dict(cleanup, outer_owner_required=False)})

    def test_partly_retained_diagnostic_is_both_present_and_counted_omitted(self):
        snapshot = {key: 0 for key in ("failures_total", "failures_omitted", "starts", "cancel_calls", "close_calls",
                    "wait_idle_calls", "diagnostics_total", "diagnostics_omitted", "diagnostic_utf8_bytes_total",
                    "diagnostic_utf8_bytes_retained", "diagnostic_utf8_bytes_omitted")}
        snapshot.update(first_failure=None, secondary_failures=[], wait_idle_succeeded=True,
                        wait_idle_failed=False, diagnostic_prefix_sealed=True, raw_trace=[], diagnostics_total=1,
                        diagnostics_omitted=1, diagnostic_utf8_bytes_total=5000, diagnostic_utf8_bytes_retained=4096,
                        diagnostic_utf8_bytes_omitted=904, diagnostic_prefix=[{"transaction": 0, "text": "x" * 4096,
                            "original_utf8_bytes": 5000, "omitted_utf8_bytes": 904}])
        owner.validate_evidence(snapshot, [], 1)
        with self.assertRaises(owner.Invalid):
            owner.validate_evidence(dict(snapshot, diagnostics_omitted=0), [], 1)


class ReservationTests(unittest.TestCase):
    def test_unknown_outer_cleanup_reads_no_producer_artifact_and_keeps_denominator(self):
        class FakeControl:
            reads = 0

            def run_bounded(self, _command, **options):
                self.assertions = options
                return {"status": "ERROR", "reason": "Unconfirmed cleanup", "returncode": None,
                        "elapsed_ms": 1, "output_bytes": 0, "completed": False, "cleanup_succeeded": False}

            def read_artifact(self, *_args, **_options):
                self.reads += 1
                raise AssertionError("Unknown producer artifact must never be read")

        with tempfile.TemporaryDirectory() as directory:
            began = time.monotonic()
            budget = owner.Budget(Path(directory), began + owner.RUN_SECONDS)
            report = {"results": owner.pending_results(), "git_commit": "a" * 40}
            control = FakeControl()
            pin = {"bytes": 1, "sha256": "a" * 64}
            with (
                patch.object(owner, "private_env", return_value={}),
                patch.object(owner, "read_regular", return_value=(b"x", pin)),
                patch.object(owner, "read_font", return_value=(b"x", pin)),
            ):
                keep_going = owner.run_case(control, budget, report, 0, {"path": owner.CASE_PATHS[0]},
                    Path("/tmp/owned-host"), pin, Path("/tmp/font"), pin, {"bytes": 1}, set(), began, None, {}, {})
            self.assertFalse(keep_going)
            self.assertEqual(control.reads, 0)
            self.assertLessEqual(control.assertions["timeout"], 22.9)
            self.assertEqual(report["results"][0]["status"], "ERROR")
            self.assertTrue(all(row["status"] == "PENDING" for row in report["results"][1:]))
            self.assertFalse(budget.cleanup_confirmed)
            self.assertEqual(budget.reserved, owner.CASE_RESERVE)

    def test_cleanup_unknown_quarantines_slot_and_blocks_new_admission(self):
        with tempfile.TemporaryDirectory() as directory:
            budget = owner.Budget(Path(directory), float("inf"))
            budget.reserve(owner.CASE_RESERVE)
            budget.release(owner.CASE_RESERVE, False)
            self.assertFalse(budget.cleanup_confirmed)
            self.assertTrue(budget.cancel.is_set())
            self.assertEqual(budget.reserved, owner.CASE_RESERVE)
            with self.assertRaises(owner.Invalid):
                budget.reserve(1)

    def test_confirmed_cleanup_releases_reservation_not_existing_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            budget = owner.Budget(root, float("inf"))
            budget.reserve(owner.CASE_RESERVE)
            owner.write_new(root / "evidence", b"kept", 4)
            budget.release(owner.CASE_RESERVE, True)
            self.assertEqual(budget.reserved, 0)
            self.assertEqual(owner.evidence_bytes(root), 4)
            self.assertEqual((root / "evidence").read_bytes(), b"kept")

    def test_shared_request_cap_is_not_multiplied_per_case(self):
        with tempfile.TemporaryDirectory() as directory:
            budget = owner.Budget(Path(directory), float("inf"))
            budget.admit_requests(owner.REQUEST_LIMIT)
            with self.assertRaises(owner.Invalid):
                budget.admit_requests(1)
            self.assertEqual(budget.requests, owner.REQUEST_LIMIT)

    def test_atomic_report_keeps_current_bytes_and_respects_quarantine(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            budget = owner.Budget(root, float("inf"))
            budget.report({"status": "PENDING"})
            prior = (root / "report.json").read_bytes()
            budget.reserved = owner.EVIDENCE_LIMIT
            with self.assertRaises(owner.Invalid):
                budget.report({"status": "ERROR"})
            self.assertEqual((root / "report.json").read_bytes(), prior)
            self.assertFalse((root / "report.next").exists())

    def test_font_never_reads_body_before_shared_file_and_byte_admission(self):
        with tempfile.TemporaryDirectory() as directory:
            font = Path(directory) / "font"
            font.write_bytes(b"abc")
            for source in ({"unique_input_files": owner.MAX_INPUT_FILES, "unique_input_bytes": 0},
                           {"unique_input_files": 1, "unique_input_bytes": owner.INPUT_LIMIT - 2}):
                with self.subTest(source=source), patch.object(owner, "read_regular") as body_read:
                    with self.assertRaises(owner.Invalid):
                        owner.admit_font(font, source)
                    body_read.assert_not_called()

    def test_exact_font_boundary_counts_once_and_rechecks_same_length_hash_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            font = Path(directory) / "font"
            font.write_bytes(b"abc")
            admission = owner.admit_font(font, {"unique_input_files": owner.MAX_INPUT_FILES - 1,
                                               "unique_input_bytes": owner.INPUT_LIMIT - 3})
            self.assertEqual(admission["combined_unique_files"], owner.MAX_INPUT_FILES)
            self.assertEqual(admission["combined_unique_bytes"], owner.INPUT_LIMIT)
            body, pin = owner.read_font(font, admission)
            self.assertEqual(body, b"abc")
            self.assertEqual(owner.read_font(font, admission, pin)[1], pin)
            font.write_bytes(b"abd")
            with self.assertRaises(owner.Invalid):
                owner.read_font(font, admission, pin)


if __name__ == "__main__":
    unittest.main()
