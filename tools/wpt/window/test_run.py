"""Pure owner controls; never build or launch Browser/worker/network services."""
import importlib.util
import json
from pathlib import Path
import tempfile
import time
from types import SimpleNamespace
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
    def test_evidence_scan_allows_only_vanished_queued_descendants(self):
        root = Path('/authored-evidence-root')
        child = root / 'rustc-temporary'
        class Entries:
            def __enter__(self):
                return self
            def __iter__(self):
                return iter([SimpleNamespace(path=str(child),
                    stat=lambda **options: SimpleNamespace(st_mode=owner.stat.S_IFDIR))])
            def __exit__(self, *args):
                return False
        def scan(path):
            if path == root:
                return Entries()
            self.assertEqual(path, child)
            raise FileNotFoundError(str(child))
        with patch.object(owner.os, 'scandir', side_effect=scan) as scanned:
            self.assertEqual(owner.evidence_bytes(root), 0)
        self.assertEqual(scanned.call_count, 2)
        for error in (PermissionError('descendant denied'), NotADirectoryError('descendant changed')):
            with self.subTest(error=type(error)), patch.object(owner.os, 'scandir', side_effect=[Entries(), error]):
                with self.assertRaises(type(error)):
                    owner.evidence_bytes(root)

    def test_evidence_scan_missing_root_and_other_errors_still_fail_closed(self):
        for error in (FileNotFoundError('root'), PermissionError('denied'), NotADirectoryError('changed')):
            with self.subTest(error=type(error)), patch.object(owner.os, 'scandir', side_effect=error):
                with self.assertRaises(type(error)):
                    owner.evidence_bytes(Path('/authored-evidence-root'))

    def test_evidence_scan_keeps_descendant_entries_bytes_and_symlink_checks(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            child = root / 'compiler-temp'
            child.mkdir()
            (child / 'data').write_bytes(b'12345')
            self.assertEqual(owner.evidence_bytes(root), 5)
            with patch.object(owner, 'MAX_EVIDENCE_ENTRIES', 1), self.assertRaises(owner.Invalid):
                owner.evidence_bytes(root)
            with patch.object(owner, 'EVIDENCE_LIMIT', 4), self.assertRaises(owner.Invalid):
                owner.evidence_bytes(root)
            (root / 'alias').symlink_to(child, target_is_directory=True)
            with self.assertRaises(owner.Invalid):
                owner.evidence_bytes(root)

    def test_ci_cold_cache_runner_context_is_step_scoped_not_job_scoped(self):
        # A focused regression for GitHub's context availability, not a YAML
        # parser or a substitute for the actual workflow validation/run.
        workflow = (owner.ROOT / ".github/workflows/pages.yml").read_text()
        pilot = workflow.split("  window-validate:\n", 1)[1].split("  validate:\n", 1)[0]
        job_header = pilot.split("    steps:\n", 1)[0]
        self.assertNotIn("${{ runner.", job_header)
        preflight = pilot.split("      - name: Require a cold Cargo cache and target\n", 1)[1]
        preflight = preflight.split("      - name: Check the complete fixed Window selection\n", 1)[0]
        self.assertIn("        env:\n          MGBROWSER_COLD_CARGO_HOME: ${{ runner.temp }}/", preflight)
        self.assertIn('test ! -e "$MGBROWSER_COLD_CARGO_HOME"', preflight)
        self.assertIn('test ! -e target', preflight)
        self.assertIn('>> "$GITHUB_ENV"', preflight)
        self.assertIn('test ! -L tmp', preflight)
        self.assertIn('mkdir -p tmp', preflight)

    def test_build_policy_is_explicit_default_off_and_never_changes_target_or_features(self):
        default = owner.parse_args(["--output", "tmp/not-created"])
        enabled = owner.parse_args(["--output", "tmp/not-created", "--allow-locked-build-network"])
        self.assertFalse(default.allow_locked_build_network)
        self.assertTrue(enabled.allow_locked_build_network)
        offline = owner.build_command()
        networked = owner.build_command(True)
        self.assertEqual(networked, [arg for arg in offline if arg != "--offline"])
        self.assertEqual(networked, ["cargo", "build", "--locked", "--release", "--example", "wpt_window", "--message-format=json"])
        with self.assertRaises(owner.Invalid):
            owner.build_command(1)

    def test_build_window_is_one_inclusive_clock_and_whole_run_can_only_narrow(self):
        start = 1_000_000_000_000
        self.assertEqual(owner.build_window(start, 2500), (start + 1_200_000_000_000,
                         start + 1_200_000_000_000 - owner.BUILD_POSTPROCESS_RESERVE_NS))
        self.assertEqual(owner.build_window(start, 1010), (1_007_000_000_000,
                         1_007_000_000_000 - owner.BUILD_POSTPROCESS_RESERVE_NS))
        with self.assertRaises(owner.Invalid):
            owner.build_window(start, 1005)

    def build_fixture(self, late_stage=None, cleanup=True, prelaunch=False, handoff_error=False):
        # Fake commands/artifacts/clock only. No Cargo, executable, Browser,
        # worker, socket, dependency access or other child is launched.
        class Clock:
            now = 1_000_000_000_000

        clock = Clock()
        result = {"status": "OK", "reason": "", "returncode": 0, "elapsed_ms": 1,
                  "output_bytes": 0, "completed": True, "cleanup_succeeded": cleanup}
        class Control:
            calls = 0
            reads = 0
            def run_bounded(self, command, **options):
                self.calls += 1
                self.command, self.options = command, options
                if handoff_error:
                    raise OSError("mock ownership handoff failure")
                if late_stage == "cargo_cleanup":
                    clock.now += 1_200_000_000_001
                return result
            def read_artifact(self, *_args, **_options):
                self.reads += 1
                if late_stage == "read":
                    clock.now += 1_200_000_000_001
                return b"mock Cargo record"
        class Budget:
            deadline = 2500
            output = Path("/tmp/never-created")
            held = 0
            def reserve(self, amount):
                self.held += amount
                if prelaunch:
                    clock.now += 1_200_000_000_000
            def release(self, amount, confirmed):
                if confirmed:
                    self.held -= amount
                if late_stage == "release":
                    clock.now += 1_200_000_000_001
            def report(self, _report):
                if late_stage == "evidence":
                    clock.now += 1_200_000_000_001
        control, budget, report = Control(), Budget(), {"build": {}}
        def identify(_raw, *, deadline_ns=None):
            if late_stage == "json":
                clock.now += 1_200_000_000_001
            return Path("/tmp/never-executed")
        def binary(_path, _limit, *, deadline_ns=None):
            if late_stage == "hash":
                clock.now += 1_200_000_000_001
            return b"mock", {"bytes": 4, "sha256": "a" * 64}
        with (patch.object(owner.time, "monotonic_ns", side_effect=lambda: clock.now),
              patch.object(owner, "write_new"), patch.object(owner, "cargo_executable", side_effect=identify),
              patch.object(owner, "read_regular", side_effect=binary), patch.object(owner.os, "access", return_value=True)):
            failure = None
            try:
                outcome = owner.build(control, budget, {}, report, allow_locked_build_network=True)
            except (owner.Invalid, OSError) as caught:
                failure, outcome = caught, None
        return control, budget, report, failure, outcome

    def test_build_includes_cleanup_json_hash_release_and_evidence_without_renewal(self):
        control, budget, report, failure, outcome = self.build_fixture()
        self.assertIsNone(failure)
        self.assertIsNotNone(outcome)
        self.assertEqual(budget.held, 0)
        self.assertTrue(report["build"]["completed_within_deadline"])
        self.assertEqual(control.options["global_deadline"], 2196.9)
        self.assertEqual(control.options["timeout"], 1196.9)
        for stage in ("cargo_cleanup", "read", "json", "hash", "release", "evidence"):
            with self.subTest(stage=stage):
                control, budget, report, failure, outcome = self.build_fixture(stage)
                self.assertIsInstance(failure, owner.Invalid)
                self.assertIsNone(outcome)
                self.assertFalse(report["build"]["completed_within_deadline"])
                self.assertEqual(budget.held, 0)

    def test_build_prelaunch_deadline_and_unknown_cleanup_never_read_producer_artifacts(self):
        control, budget, report, failure, outcome = self.build_fixture(prelaunch=True)
        self.assertIsInstance(failure, owner.Invalid)
        self.assertEqual(control.calls, 0)
        self.assertEqual(control.reads, 0)
        self.assertEqual(budget.held, 0)
        control, budget, report, failure, outcome = self.build_fixture(cleanup=False)
        self.assertIsInstance(failure, owner.Invalid)
        self.assertEqual(control.reads, 0)
        self.assertEqual(budget.held, owner.BUILD_RESERVE)
        self.assertFalse(report["build"]["process_cleanup_confirmed"])

    def test_build_handoff_exception_quarantines_without_producer_reads(self):
        control, budget, report, failure, outcome = self.build_fixture(handoff_error=True)
        self.assertIsInstance(failure, OSError)
        self.assertIsNone(outcome)
        self.assertEqual(control.reads, 0)
        self.assertEqual(budget.held, owner.BUILD_RESERVE)
        self.assertFalse(report["build"]["process_cleanup_confirmed"])

    def test_build_completion_does_not_inherit_a_callers_handled_exception(self):
        try:
            raise ValueError("caller handled exception")
        except ValueError:
            _, _, report, failure, outcome = self.build_fixture()
        self.assertIsNone(failure)
        self.assertIsNotNone(outcome)
        self.assertTrue(report["build"]["completed_within_deadline"])

    def test_cargo_expiry_mid_record_stops_before_another_json_record(self):
        event = {"reason": "compiler-artifact", "target": {"name": "wpt_window", "kind": ["example"]},
                 "features": [], "executable": "/tmp/never-executed"}
        clock = [1]
        def decode(_raw, _limit):
            clock[0] = 10
            return event
        with (patch.object(owner.time, "monotonic_ns", side_effect=lambda: clock[0]),
              patch.object(owner, "strict_json", side_effect=decode) as decode_mock):
            with self.assertRaises(owner.Invalid):
                owner.cargo_executable(b"first\nsecond", deadline_ns=10)
        self.assertEqual(decode_mock.call_count, 1)

    def test_regular_file_deadline_stops_mid_chunk_or_after_hash(self):
        # Test-owned ordinary bytes only; no executable or product source read.
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input"
            path.write_bytes(b"x" * 65537)
            clock = [1]
            actual_read, actual_sha = owner.os.read, owner.sha
            def chunk(descriptor, count):
                value = actual_read(descriptor, count)
                clock[0] = 10
                return value
            with (patch.object(owner.time, "monotonic_ns", side_effect=lambda: clock[0]),
                  patch.object(owner.os, "read", side_effect=chunk) as reads,
                  patch.object(owner, "sha") as hashes):
                with self.assertRaises(owner.Invalid):
                    owner.read_regular(path, 65537, deadline_ns=10)
            self.assertEqual(reads.call_count, 1)
            hashes.assert_not_called()
            clock[0] = 1
            def hashing(value):
                result = actual_sha(value)
                clock[0] = 10
                return result
            with (patch.object(owner.time, "monotonic_ns", side_effect=lambda: clock[0]),
                  patch.object(owner, "sha", side_effect=hashing) as hashes):
                with self.assertRaises(owner.Invalid):
                    owner.read_regular(path, 65537, deadline_ns=10)
            self.assertEqual(hashes.call_count, 1)

    def test_run_build_failure_keeps_every_case_pending_and_calls_no_case_owner(self):
        # Orchestrate only the failure branch with mocks. No product helper,
        # Cargo, Browser, worker, JS, network or child executes.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            document = {"tests": [{"path": path, "source_declared_call_count": 1} for path in owner.CASE_PATHS]}
            checker = type("Checker", (), {"check": lambda _self, _root: {}})()
            pin = {"bytes": 1, "sha256": "a" * 64}
            def tool(_control, _budget, _command, name, _environment):
                return {"rustc": "rustc 1.91.1 mock", "cargo": "cargo mock", "git-head": "a" * 40, "git-status": ""}[name]
            args = owner.parse_args(["--output", str(output)])
            with (patch.object(owner, "fresh_directory", return_value=output),
                  patch.object(owner, "load_definitions", return_value=checker),
                  patch.object(owner, "read_regular", return_value=(json.dumps(document).encode(), pin)),
                  patch.object(owner, "fingerprint", return_value={"sha256": "a" * 64}),
                  patch.object(owner, "admit_font", return_value={"bytes": 1}),
                  patch.object(owner, "read_font", return_value=(b"x", pin)),
                  patch.object(owner, "private_env", return_value={}), patch.object(owner, "tool", side_effect=tool),
                  patch.object(owner.signal, "signal"), patch("builtins.print"),
                  patch.object(owner, "build", side_effect=owner.Invalid("mock build failure")) as build_stage,
                  patch.object(owner, "run_case") as case_owner):
                code = owner.run(args, time.monotonic())
            report = json.loads((output / "report.json").read_bytes())
            self.assertEqual(code, 1)
            build_stage.assert_called_once()
            case_owner.assert_not_called()
            self.assertEqual(report["infrastructure_errors"], ["mock build failure"])
            self.assertEqual(len(report["results"]), 16)
            self.assertTrue(all(row["status"] == "PENDING" for row in report["results"]))
            self.assertFalse(report["complete"])
            self.assertFalse(report["baseline_review_eligible"])

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
