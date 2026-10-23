"""Pure score-gate I/O controls: authored data/temp files, no owner run or child.

No Rust, Browser, worker, JS, fixture server or network executes. These tests
exercise metadata APIs only, not an accepted upstream browser measurement.
"""
from contextlib import contextmanager, redirect_stderr
import copy
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch


def definitions(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


owner = definitions("window_score_owner", "run.py")
inputs = definitions("window_score_authored_inputs", "test_score.py")
checker = definitions("window_score_input_reader", "check_manifest.py")
score = inputs.score


def pin(root, name):
    raw = (root / name).read_bytes()
    return {"path": name, "bytes": len(raw), "sha256": owner.sha(raw)}


@contextmanager
def gate_case(action="check", *, stale=False):
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory).resolve()
        here = root / "tools/wpt/window"
        here.mkdir(parents=True)
        output = root / "tmp/authored-score-gate"
        output.mkdir(parents=True)
        (root / owner.SCORE_HELPER_PATH).write_bytes(Path(__file__).with_name("score.py").read_bytes())
        ratchet = score.initial_ratchet()
        (root / owner.RATCHET_PATH).write_bytes(owner.encoded(ratchet, owner.METADATA_LIMIT))
        report = inputs.accepted_report()
        for row in report["results"]:
            counts = row["semantic_decision"]["final_counts"]
            row["final_subtests"] = {status: counts[key] for status, key in zip(owner.SUBTEST_STATUSES, score.SUBTESTS)}
        report["distributions"] = owner.distributions(report["results"])
        report["score_written"] = False
        report["score_gate"] = {"mode": action, "status": "PENDING", "error": None}
        source = [pin(root, owner.SCORE_HELPER_PATH), pin(root, owner.RATCHET_PATH)]
        report["source_fingerprint"].update(files=source, unique_input_files=2,
            unique_input_bytes=sum(item["bytes"] for item in source), metadata_bytes=source[1]["bytes"])
        report["input_admission"] = {"combined_unique_files": 3,
            "combined_unique_bytes": sum(item["bytes"] for item in source) + 42}
        recorded = score.score_from(report)
        if stale:
            recorded["source_sha256"] = "b" * 64
        (root / owner.CURRENT_SCORE_PATH).write_bytes(owner.encoded(recorded, owner.METADATA_LIMIT))
        args = owner.parse_args(["--output", str(output), "--" + action + "-score"])
        budget = owner.Budget(output, time.monotonic() + 10)
        # Model the caller's required retained complete evidence, without run().
        budget.report(report)
        with patch.object(owner, "ROOT", root), patch.object(owner, "HERE", here):
            yield root, output, report, args, budget


class ScoreIntegrationTests(unittest.TestCase):
    def test_default_is_report_only_and_modes_are_mutually_exclusive(self):
        defaults = owner.parse_args(["--output", "tmp/not-created"])
        self.assertFalse(defaults.update_score)
        self.assertFalse(defaults.check_score)
        for mode in ("update", "check"):
            args = owner.parse_args(["--output", "tmp/not-created", "--" + mode + "-score"])
            self.assertEqual((args.update_score, args.check_score), (mode == "update", mode == "check"))
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as failure:
            owner.parse_args(["--output", "tmp/not-created", "--update-score", "--check-score"])
        self.assertEqual(failure.exception.code, 2)

    def test_only_generated_score_is_excluded_while_ratchet_helper_and_source_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            here = root / "tools/wpt/window"
            for relative in ("src", "crates", "examples/wpt_window", "tools/wpt/window"):
                (root / relative).mkdir(parents=True, exist_ok=True)
            for relative in (owner.CURRENT_SCORE_PATH, owner.RATCHET_PATH, owner.SCORE_HELPER_PATH,
                             "src/authored.rs", "tools/wpt/window/other.json"):
                (root / relative).write_bytes(b"fixture\n")
            with patch.object(owner, "ROOT", root), patch.object(owner, "HERE", here):
                names = owner.source_paths()
                self.assertNotIn(owner.CURRENT_SCORE_PATH, names)
                self.assertIn(owner.RATCHET_PATH, names)
                self.assertIn(owner.SCORE_HELPER_PATH, names)
                self.assertIn("tools/wpt/window/other.json", names)
                for name in {*names, owner.MANIFEST_PATH, "tools/wpt/corpus.json"}:
                    path = root / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    if not path.exists():
                        path.write_bytes(b"fixture\n")
                # Reader/fingerprint only: no manifest parser, JS or owner runs.
                document = {"full_source_domain_roster": [], "files": []}
                snapshot = lambda: owner.fingerprint(checker, document, time.monotonic() + 5)
                before = snapshot()
                self.assertEqual(before["metadata_bytes"], 3 * len(b"fixture\n"))
                (root / owner.CURRENT_SCORE_PATH).write_bytes(b"changed generated bytes\n")
                self.assertEqual(snapshot(), before)
                for name in (owner.RATCHET_PATH, owner.SCORE_HELPER_PATH, "src/authored.rs"):
                    (root / name).write_bytes(b"changed evaluated bytes\n")
                    after = snapshot()
                    self.assertNotEqual(after["sha256"], before["sha256"])
                    before = after
                (root / owner.CURRENT_SCORE_PATH).unlink()
                (root / owner.CURRENT_SCORE_PATH).symlink_to(root / owner.RATCHET_PATH)
                with self.assertRaises(owner.Invalid):
                    owner.source_paths()

    def test_generated_score_admission_shares_exact_file_input_and_metadata_limits(self):
        report = {"input_admission": {"combined_unique_files": owner.MAX_INPUT_FILES - 1,
                    "combined_unique_bytes": owner.INPUT_LIMIT - 7},
                  "source_fingerprint": {"metadata_bytes": owner.METADATA_LIMIT - 7}}
        admitted = owner.score_input_admission(report, 7)
        self.assertEqual(admitted["combined_unique_files"], owner.MAX_INPUT_FILES)
        self.assertEqual(admitted["combined_unique_bytes"], owner.INPUT_LIMIT)
        self.assertEqual(admitted["metadata_bytes"], owner.METADATA_LIMIT)
        for length in (0, True, 8, owner.METADATA_LIMIT + 1):
            with self.subTest(length=length), self.assertRaises(owner.Invalid):
                owner.score_input_admission(report, length)
        report["input_admission"]["combined_unique_files"] = owner.MAX_INPUT_FILES
        with self.assertRaises(owner.Invalid):
            owner.score_input_admission(report, 1)

    def test_check_passes_without_writing_score_or_ratchet(self):
        with gate_case() as (root, output, report, args, budget):
            old_score = (root / owner.CURRENT_SCORE_PATH).read_bytes()
            old_ratchet = (root / owner.RATCHET_PATH).read_bytes()
            with patch.object(owner, "load_definitions", wraps=owner.load_definitions) as loaded:
                owner.run_score_gate(args, report, budget)
            self.assertEqual(report["score_gate"]["status"], "PASSED")
            self.assertFalse(report["score_written"])
            self.assertFalse(report["expectations_written"])
            self.assertEqual((root / owner.CURRENT_SCORE_PATH).read_bytes(), old_score)
            self.assertEqual((root / owner.RATCHET_PATH).read_bytes(), old_ratchet)
            self.assertFalse((output / "current-score.next").exists())
            self.assertEqual(budget.reserved, 0)
            call = loaded.call_args
            self.assertEqual(call.kwargs["deadline_ns"], int(budget.deadline * 1_000_000_000))
            self.assertEqual(call.kwargs["expected"], report["source_fingerprint"]["files"][0]["sha256"])
            self.assertEqual(call.kwargs["limit"], report["source_fingerprint"]["files"][0]["bytes"])

    def test_stale_score_preserves_actual_sixteen_file_seventy_nine_outcomes(self):
        with gate_case(stale=True) as (root, _output, report, args, budget):
            before = copy.deepcopy(report)
            old_score = (root / owner.CURRENT_SCORE_PATH).read_bytes()
            old_ratchet = (root / owner.RATCHET_PATH).read_bytes()
            owner.run_score_gate(args, report, budget)
            self.assertEqual(report["score_gate"]["status"], "FAILED")
            self.assertIn("stale", report["score_gate"]["error"])
            for key in ("status", "complete", "baseline_review_eligible", "results", "distributions", "infrastructure_errors"):
                self.assertEqual(report[key], before[key])
            self.assertEqual(len(report["results"]), 16)
            self.assertEqual(report["distributions"]["actual_final_subtests"]["Fail"], 79)
            self.assertFalse(report["score_written"])
            self.assertEqual((root / owner.CURRENT_SCORE_PATH).read_bytes(), old_score)
            self.assertEqual((root / owner.RATCHET_PATH).read_bytes(), old_ratchet)
            self.assertEqual(budget.reserved, 0)

    def test_bad_measurement_uncertain_cleanup_and_ratchet_cannot_write(self):
        for failure in ("ineligible", "infrastructure", "cleanup", "ratchet"):
            with self.subTest(failure=failure), gate_case("update") as (root, output, report, args, budget):
                if failure == "ineligible":
                    report["baseline_review_eligible"] = False
                elif failure == "infrastructure":
                    report["infrastructure_errors"] = ["authored failure"]
                elif failure == "cleanup":
                    budget.cleanup_confirmed = False
                else:
                    manual = score.initial_ratchet()
                    manual["required_passes"] = [score.TESTS[0]]
                    (root / owner.RATCHET_PATH).write_bytes(owner.encoded(manual, owner.METADATA_LIMIT))
                    report["source_fingerprint"]["files"][1] = pin(root, owner.RATCHET_PATH)
                old_score = (root / owner.CURRENT_SCORE_PATH).read_bytes()
                old_ratchet = (root / owner.RATCHET_PATH).read_bytes()
                owner.run_score_gate(args, report, budget)
                self.assertEqual(report["score_gate"]["status"], "NOT_ELIGIBLE" if failure == "ineligible" else "FAILED")
                self.assertFalse(report["score_written"])
                self.assertEqual((root / owner.CURRENT_SCORE_PATH).read_bytes(), old_score)
                self.assertEqual((root / owner.RATCHET_PATH).read_bytes(), old_ratchet)
                self.assertFalse((output / "current-score.next").exists())

    def test_excluded_score_is_admitted_before_body_read(self):
        with gate_case() as (root, _output, report, args, budget):
            report["input_admission"]["combined_unique_bytes"] = owner.INPUT_LIMIT
            with patch.object(owner, "read_regular", wraps=owner.read_regular) as read:
                owner.run_score_gate(args, report, budget)
            self.assertEqual(report["score_gate"]["status"], "FAILED")
            self.assertFalse(any(call.args[0] == root / owner.CURRENT_SCORE_PATH for call in read.call_args_list))
            self.assertFalse(report["score_written"])

    def test_changed_source_helper_is_refused_before_import(self):
        with gate_case("update") as (root, output, report, args, budget):
            old_score = (root / owner.CURRENT_SCORE_PATH).read_bytes()
            (root / owner.SCORE_HELPER_PATH).write_text("raise AssertionError('MUTATED_MODULE_EXECUTED')\n")
            owner.run_score_gate(args, report, budget)
            self.assertEqual(report["score_gate"]["status"], "FAILED")
            self.assertNotIn("MUTATED_MODULE_EXECUTED", report["score_gate"]["error"])
            self.assertEqual((root / owner.CURRENT_SCORE_PATH).read_bytes(), old_score)
            self.assertFalse((output / "current-score.next").exists())

    def test_update_is_one_atomic_replace_after_retained_complete_evidence(self):
        with gate_case("update", stale=True) as (root, output, report, args, budget):
            current = root / owner.CURRENT_SCORE_PATH
            old_score = current.read_bytes()
            old_ratchet = (root / owner.RATCHET_PATH).read_bytes()
            replace = owner.os.replace
            events = []
            def commit(source, destination):
                self.assertEqual(destination, current)
                retained = json.loads((output / "report.json").read_bytes())
                self.assertTrue(retained["baseline_review_eligible"])
                self.assertEqual(retained["status"], "COMPLETE")
                self.assertEqual(len(retained["results"]), 16)
                self.assertFalse(retained["score_written"])
                self.assertEqual(current.read_bytes(), old_score)
                events.append((source, destination))
                return replace(source, destination)
            with patch.object(owner.os, "replace", side_effect=commit):
                owner.run_score_gate(args, report, budget)
            self.assertEqual(len(events), 1)
            self.assertEqual(report["score_gate"]["status"], "PASSED")
            self.assertTrue(report["score_written"])
            self.assertEqual(json.loads(current.read_bytes()), score.score_from(report))
            self.assertEqual((root / owner.RATCHET_PATH).read_bytes(), old_ratchet)
            self.assertFalse((output / "current-score.next").exists())
            self.assertEqual(budget.reserved, 0)
            budget.report(report)
            self.assertTrue(json.loads((output / "report.json").read_bytes())["score_written"])

    def test_precommit_deadline_or_replace_failure_keeps_old_score_and_bounded_temp(self):
        for failure in ("expired", "final_guard", "replace"):
            with self.subTest(failure=failure), gate_case("update", stale=True) as (root, output, report, args, budget):
                current = root / owner.CURRENT_SCORE_PATH
                old_score = current.read_bytes()
                if failure == "expired":
                    budget.deadline = time.monotonic() - 1
                elif failure == "final_guard":
                    budget.deadline = time.monotonic() + 1
                if failure == "replace":
                    with patch.object(owner.os, "replace", side_effect=OSError("authored replacement failure")):
                        owner.run_score_gate(args, report, budget)
                else:
                    owner.run_score_gate(args, report, budget)
                self.assertEqual(report["score_gate"]["status"], "FAILED")
                self.assertFalse(report["score_written"])
                self.assertEqual(current.read_bytes(), old_score)
                self.assertEqual(budget.reserved, 0)
                temporary = output / "current-score.next"
                self.assertEqual(temporary.exists(), failure != "expired")
                if temporary.exists():
                    self.assertLessEqual(temporary.stat().st_size, owner.METADATA_LIMIT)

    def test_postcommit_failure_never_claims_rollback_or_changes_measurement(self):
        with gate_case("update", stale=True) as (root, output, report, args, budget):
            results = copy.deepcopy(report["results"])
            check = owner.check_deadline
            def late(deadline, where):
                if where == "Window score gate completion":
                    raise owner.Invalid("authored postcommit failure")
                return check(deadline, where)
            with patch.object(owner, "check_deadline", side_effect=late):
                owner.run_score_gate(args, report, budget)
            self.assertEqual(report["score_gate"]["status"], "FAILED")
            self.assertTrue(report["score_written"])
            self.assertEqual(json.loads((root / owner.CURRENT_SCORE_PATH).read_bytes()), score.score_from(report))
            self.assertEqual(report["results"], results)
            self.assertEqual(report["status"], "COMPLETE")
            self.assertFalse((output / "current-score.next").exists())
            self.assertEqual(budget.reserved, 0)
            # Two files, not an atomic rollback promise: old retained evidence
            # can lag the successful score replacement if its next write fails.
            with patch.object(budget, "report", side_effect=OSError("authored final report failure")):
                with self.assertRaises(OSError):
                    budget.report(report)
            self.assertFalse(json.loads((output / "report.json").read_bytes())["score_written"])


class FinalPublicationTests(unittest.TestCase):
    def test_final_guard_checks_live_state_and_exact_deadline(self):
        with tempfile.TemporaryDirectory() as directory:
            deadline = time.monotonic() + 10
            for state in ("clean", "interrupted", "cancelled", "cleanup", "reserved", "failure", "deadline"):
                with self.subTest(state=state):
                    budget = owner.Budget(Path(directory), deadline)
                    if state == "interrupted":
                        budget.interruption = "authored late interruption"
                    elif state == "cancelled":
                        budget.cancel.set()
                    elif state == "cleanup":
                        budget.cleanup_confirmed = False
                    elif state == "reserved":
                        budget.reserved = 1
                    elif state == "failure":
                        budget.failure = "authored earlier unresolved failure"
                    now = deadline if state == "deadline" else deadline - 1
                    with patch.object(owner.time, "monotonic", return_value=now):
                        if state == "clean":
                            owner.final_publication_guard(budget, deadline)
                        else:
                            with self.assertRaises(owner.Invalid):
                                owner.final_publication_guard(budget, deadline)

    def mocked_run(self, interruption_at):
        # Fully mocked successful orchestration, not an actual owner/product
        # measurement. Real I/O is limited to test-owned report JSON. Every
        # source/tool/build/case boundary is replaced before run() is entered.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            authored_rows = inputs.accepted_report()["results"]
            for index, row in enumerate(authored_rows, 1):
                row["run_id"] = index
                row["final_subtests"] = {status: row["semantic_decision"]["final_counts"][key]
                    for status, key in zip(owner.SUBTEST_STATUSES, score.SUBTESTS)}
            document = {"tests": [{"path": path, "source_declared_call_count": calls}
                                   for path, calls in score.ROSTER]}
            source_checker = type("Checker", (), {"check": lambda _self, _root: {}})()
            binary_pin = {"bytes": 1, "sha256": "a" * 64}
            signals, printed, budgets, fired = [], [], [], []
            original_report = owner.Budget.report

            def retain(budget, report):
                budgets[:] = [budget]
                original_report(budget, report)
                if (interruption_at == "report" and not fired
                        and report["score_gate"]["status"] == "PASSED"):
                    fired.append("report")
                    budget.interruption = "authored interruption during final report"

            def emit(value):
                printed.append(json.loads(value))
                if interruption_at == "stdout" and not fired:
                    fired.append("stdout")
                    budgets[0].interruption = "authored interruption during final stdout"

            def accepted_case(_control, _budget, report, index, *_args):
                report["results"][index] = copy.deepcopy(authored_rows[index])
                return True

            def passed_gate(_args, report, _budget):
                self.assertTrue(report["baseline_review_eligible"])
                report["score_gate"].update(status="PASSED", error=None)
                # Model a completed atomic replacement. Late publication
                # failure must preserve this fact, never claim rollback.
                report["score_written"] = True

            def tool(_control, _budget, _command, name, _environment):
                return {"rustc": "rustc 1.91.1 authored", "cargo": "cargo authored",
                        "git-head": "a" * 40, "git-status": ""}[name]

            def signal(signum, handler):
                signals.append((signum, handler))
                return "authored prior handler"

            args = owner.parse_args(["--output", str(output), "--update-score"])
            with (patch.object(owner, "fresh_directory", return_value=output),
                  patch.object(owner, "load_definitions", return_value=source_checker),
                  patch.object(owner, "read_regular", return_value=(json.dumps(document).encode(), binary_pin)),
                  patch.object(owner, "fingerprint", return_value={"sha256": "a" * 64}),
                  patch.object(owner, "admit_font", return_value={"bytes": 1}),
                  patch.object(owner, "read_font", return_value=(b"x", binary_pin)),
                  patch.object(owner, "private_env", return_value={}),
                  patch.object(owner, "tool", side_effect=tool),
                  patch.object(owner, "build", return_value=(Path("/tmp/authored-never-executed"), binary_pin)),
                  patch.object(owner, "run_case", side_effect=accepted_case) as cases,
                  patch.object(owner, "run_score_gate", side_effect=passed_gate) as gate,
                  patch.object(owner.Budget, "report", new=retain),
                  patch.object(owner.signal, "signal", side_effect=signal),
                  patch("builtins.print", side_effect=emit)):
                code = owner.run(args, time.monotonic())
            report = json.loads((output / "report.json").read_bytes())
            self.assertEqual(cases.call_count, 16)
            gate.assert_called_once()
            self.assertEqual(report["results"], authored_rows)
            self.assertEqual(report["distributions"], owner.distributions(authored_rows))
            self.assertEqual(report["distributions"]["actual_final_subtests"]["Fail"], 79)
            self.assertTrue(report["complete"])
            self.assertTrue(report["score_written"])
            self.assertFalse(report["expectations_written"])
            self.assertEqual(len(signals), 4)
            self.assertEqual([handler for _, handler in signals[2:]], ["authored prior handler"] * 2)
            self.assertEqual([event for event in fired], [] if interruption_at is None else [interruption_at])
            if interruption_at is None:
                self.assertEqual(code, 0)
                self.assertTrue(report["baseline_review_eligible"])
                self.assertEqual(report["status"], "COMPLETE")
                self.assertEqual(report["score_gate"]["status"], "PASSED")
                self.assertIsNone(report["score_gate"]["error"])
                self.assertEqual(report["infrastructure_errors"], [])
                self.assertTrue(printed[-1]["baseline_review_eligible"])
            else:
                self.assertEqual(code, 1)
                self.assertFalse(report["baseline_review_eligible"])
                self.assertEqual(report["status"], "COMPLETE_NONADOPTABLE")
                self.assertEqual(report["score_gate"]["status"], "FAILED")
                self.assertIn("interrupt", report["score_gate"]["error"].lower())

    def test_final_report_and_stdout_interruptions_cannot_publish_success(self):
        for interruption_at in (None, "report", "stdout"):
            with self.subTest(interruption_at=interruption_at):
                self.mocked_run(interruption_at)


if __name__ == "__main__":
    unittest.main()
