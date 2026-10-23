"""Owned pure staging fixtures; never run owners, Browser, workers or network."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("window_public_stage", Path(__file__).with_name("stage.py"))
stage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(stage)
COMMIT = "a" * 40


def bind_window(report):
    score = stage.window_score.score_from(report)
    raw = stage.encoded(score)
    report["score_gate"] = {"mode": "check", "status": "PASSED", "error": None,
        "candidate_sha256": hashlib.sha256(raw).hexdigest(),
        "current_score_input": {"path": "tools/wpt/window/current-score.json",
                                "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}}
    return score, raw


def fixture():
    results = [{"test": f"authored-fixture/{index:04d}.html", "status": status}
               for index, status in enumerate(["PASS"] * 317 + ["FAIL"] * 632 + ["UNSUPPORTED"] * 62)]
    static_report = {"schema_version": 2, "scope": "static-reftest-pilot", "git_commit": COMMIT,
        "git_dirty": False, "complete": True, "infrastructure_errors": [],
        "selected_total": 1011, "completed_total": 1011, "results": results,
        "source_sha256": "b" * 64, "corpus_sha256": "c" * 64, "upstream_revision": "d" * 40,
        "execution": {"cleanup_confirmed": True, "remaining_producer_reserved_bytes": 0}}
    static_score = stage.static_runner.score_from(static_report)
    static_report["counts"] = dict(static_score["counts"], PENDING=0)
    rows = []
    for path, calls in stage.window_score.ROSTER:
        counts = {"total": calls, "pass": 0, "fail": calls, "timeout": 0, "not_run": 0,
                  "precondition_failed": 0, "final_only": 0}
        rows.append({"test": path, "status": "FAIL", "source_declared_calls": calls,
            "attempted": True, "owner_accepted": True, "evidence_validated": True,
            "cleanup_confirmed": True, "raw_harness_status": "Ok",
            "semantic_decision": {"verdict": "FAIL", "basis": "Assertions", "observed_results": calls,
                                  "final_counts": counts}})
    window_report = {"schema_version": 2, "status": "COMPLETE", "complete": True,
        "git_commit": COMMIT, "git_dirty": False, "baseline_review_eligible": True,
        "infrastructure_errors": [], "expectations_written": False, "score_written": False,
        "selected_total": 16, "attempted_total": 16, "completed_total": 16,
        "manifest_sha256": stage.window_score.MANIFEST, "selection": stage.window_score.SELECTION,
        "observer_contract": stage.window_score.CONTRACT,
        "source_fingerprint": {"sha256": "e" * 64}, "source_fingerprint_after": {"sha256": "e" * 64},
        "build": {"status": "COMPLETE", "binary_verified": True,
                  "completed_within_deadline": True, "process_cleanup_confirmed": True},
        "execution": {"cleanup_confirmed": True, "cancelled": False,
                      "remaining_producer_reserved_bytes": 0, "owner_failure": None}, "results": rows}
    current, raw = bind_window(window_report)
    return [static_report, window_report, static_score, current, stage.window_score.initial_ratchet(), COMMIT, raw]


def filesystem(root):
    values = fixture()
    for name, raw in (("tmp/static-report/report.json", stage.encoded(values[0])),
                      ("tmp/window-report/report.json", stage.encoded(values[1])),
                      ("tools/wpt/score.json", stage.encoded(values[2])),
                      ("tools/wpt/window/current-score.json", values[6]),
                      ("tools/wpt/window/ratchet.json", stage.encoded(values[4]))):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
    for name in stage.STATIC:
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(("Owned fixture asset: " + name).encode())
    return values


class ValidationTests(unittest.TestCase):
    def test_clean_two_lane_projection_and_ratchet_are_not_mutated(self):
        values = fixture()
        before = copy.deepcopy(values)
        stage.validate(*values)
        self.assertEqual(values, before)
        self.assertEqual(values[3]["summary"]["source_prospective_calls"], 79)

    def test_exact_clean_sha_complete_and_cleanup_required_for_both_lanes(self):
        changes = [lambda r: r.update(git_commit="f" * 40), lambda r: r.update(git_dirty=True),
                   lambda r: r.update(git_dirty=0), lambda r: r.update(complete=False),
                   lambda r: r.update(infrastructure_errors=["failed"]),
                   lambda r: r["execution"].update(cleanup_confirmed=False),
                   lambda r: r["execution"].update(remaining_producer_reserved_bytes=1),
                   lambda r: r["execution"].update(remaining_producer_reserved_bytes=False)]
        for lane in (0, 1):
            for change in changes:
                values = fixture()
                change(values[lane])
                with self.subTest(lane=lane, change=change), self.assertRaises(ValueError):
                    stage.validate(*values)
        for commit in ("short", "A" * 40, None):
            values = fixture()
            values[5] = commit
            with self.subTest(commit=commit), self.assertRaises(ValueError):
                stage.validate(*values)

    def test_static_counts_denominator_and_order_remain_bound_to_tracked_score(self):
        changes = [lambda r: r.update(scope="other"), lambda r: r.update(schema_version=True),
                   lambda r: r.update(selected_total=1010), lambda r: r["results"].reverse(),
                   lambda r: r["results"].pop(), lambda r: r["counts"].update(PASS=0),
                   lambda r: r["counts"].update(ERROR=False),
                   lambda r: r.update(source_sha256="f" * 64),
                   lambda r: r.update(corpus_sha256="z" * 64)]
        for change in changes:
            values = fixture()
            change(values[0])
            with self.subTest(change=change), self.assertRaises(ValueError):
                stage.validate(*values)

    def test_static_terminal_failure_or_duplicate_cannot_be_hidden_in_recorded_score(self):
        for status in ("ERROR", "TIMEOUT", "CRASH"):
            values = fixture()
            values[0]["results"][0]["status"] = status
            values[2] = stage.static_runner.score_from(values[0])
            values[0]["counts"] = dict(values[2]["counts"], PENDING=0)
            with self.subTest(status=status), self.assertRaises(ValueError):
                stage.validate(*values)
        values = fixture()
        values[0]["results"][1]["test"] = values[0]["results"][0]["test"]
        values[2] = stage.static_runner.score_from(values[0])
        with self.assertRaises(ValueError):
            stage.validate(*values)
        for bad in ("", "bad\npath", None):
            values = fixture()
            values[0]["results"][0]["test"] = bad
            values[2] = stage.static_runner.score_from(values[0])
            with self.subTest(test=bad), self.assertRaises(ValueError):
                stage.validate(*values)

    def test_window_check_mode_and_both_score_hash_bindings_are_required(self):
        changes = [lambda r: r["score_gate"].update(mode="update"),
                   lambda r: r["score_gate"].update(mode="report-only"),
                   lambda r: r["score_gate"].update(status="FAILED"),
                   lambda r: r["score_gate"].update(error="stale"), lambda r: r.update(score_written=True),
                   lambda r: r["score_gate"].update(candidate_sha256="0" * 64),
                   lambda r: r["score_gate"]["current_score_input"].update(path="other"),
                   lambda r: r["score_gate"]["current_score_input"].update(bytes=True),
                   lambda r: r["score_gate"]["current_score_input"].update(sha256="0" * 64)]
        for change in changes:
            values = fixture()
            change(values[1])
            with self.subTest(change=change), self.assertRaises(ValueError):
                stage.validate(*values)

    def test_window_source_after_build_eligibility_and_raw_object_binding(self):
        changes = [lambda r: r.update(baseline_review_eligible=False),
                   lambda r: r["build"].update(process_cleanup_confirmed=False),
                   lambda r: r["source_fingerprint_after"].update(sha256="0" * 64),
                   lambda r: r["results"][0].update(owner_accepted=False)]
        for change in changes:
            values = fixture()
            change(values[1])
            with self.subTest(change=change), self.assertRaises(ValueError):
                stage.validate(*values)
        values = fixture()
        values[6] = stage.encoded(dict(values[3], source_sha256="0" * 64))
        with self.assertRaises(ValueError):
            stage.validate(*values)

    def test_genuine_window_timeout_and_partial_unknown_totals_remain_allowed(self):
        for partial in (False, True):
            values = fixture()
            row = values[1]["results"][0]
            row.update(status="TIMEOUT", raw_harness_status=None if partial else "Timeout")
            row["semantic_decision"].update(verdict="TIMEOUT", basis="MissingCompletion" if partial else "HarnessTimeout")
            if partial:
                row["semantic_decision"].update(final_counts=None, observed_results=2)
            values[3], values[6] = bind_window(values[1])
            stage.validate(*values)
            self.assertEqual(values[3]["summary"]["actual_final_subtests"]["total"], 72 if partial else 79)
            self.assertEqual(values[3]["summary"]["partial_unobserved_subtests"], "unknown")

    def test_manual_ratchet_never_weakens_to_allow_regression(self):
        values = fixture()
        values[4]["required_passes"] = [stage.window_score.TESTS[0]]
        with self.assertRaises(ValueError):
            stage.validate(*values)

    def test_duplicate_nonfinite_overflowfloat_and_invalid_utf8_json_reject(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}', b'{"x":1e999}', b'"\xff"'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                stage.strict_json(raw)


class FilesystemTests(unittest.TestCase):
    def test_only_static_assets_and_both_unmodified_reports_are_staged(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            filesystem(root)
            raw = root / "tmp/raw-evidence/profile/private"
            raw.parent.mkdir(parents=True)
            raw.write_text("never public")
            result = stage.stage(root, COMMIT)
            output = root / "tmp/public"
            names = {str(path.relative_to(output)) for path in output.rglob("*") if path.is_file()}
            self.assertEqual(names, set(stage.STATIC) | {"wpt-results.json", "wpt-window-results.json"})
            self.assertEqual(result["files"], 9)
            self.assertEqual((output / "wpt-results.json").read_bytes(), (root / "tmp/static-report/report.json").read_bytes())
            self.assertEqual((output / "wpt-window-results.json").read_bytes(), (root / "tmp/window-report/report.json").read_bytes())

    def test_raw_file_or_directory_inside_report_artifact_is_rejected(self):
        for directory in (False, True):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                filesystem(root)
                extra = root / "tmp/window-report/raw"
                extra.mkdir() if directory else extra.write_bytes(b"raw")
                with self.assertRaises(ValueError):
                    stage.stage(root, COMMIT)
                self.assertFalse((root / "tmp/public").exists())

    def test_report_exact_25mib_admitted_and_plus_one_rejected(self):
        self.assertEqual(stage.PUBLIC_LIMIT, 25 * 1024 * 1024)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "report.json"
            with path.open("wb") as stream:
                stream.truncate(stage.PUBLIC_LIMIT)
            self.assertEqual(len(stage.report_bytes(root, time.monotonic() + 10)), stage.PUBLIC_LIMIT)
            with path.open("r+b") as stream:
                stream.truncate(stage.PUBLIC_LIMIT + 1)
            with self.assertRaises(ValueError):
                stage.report_bytes(root, time.monotonic() + 10)

    def test_symlink_report_or_asset_rejected_and_existing_output_never_overwritten(self):
        for target in ("tmp/window-report/report.json", "assets/mgbrowser.svg"):
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                filesystem(root)
                path = root / target
                original = path.with_suffix(".owned-original")
                path.rename(original)
                path.symlink_to(original)
                with self.subTest(target=target), self.assertRaises(ValueError):
                    stage.stage(root, COMMIT)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            filesystem(root)
            output = root / "tmp/public"
            output.mkdir()
            kept = output / "kept"
            kept.write_text("retained")
            with self.assertRaises(ValueError):
                stage.stage(root, COMMIT)
            self.assertEqual(kept.read_text(), "retained")

    def test_total_budget_and_same_deadline_fail_before_publication(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            filesystem(root)
            with patch.object(stage, "TOTAL_LIMIT", 1), self.assertRaises(ValueError):
                stage.stage(root, COMMIT)
            self.assertFalse((root / "tmp/public").exists())
            with self.assertRaises(ValueError):
                stage.stage(root, COMMIT, deadline=0)
            with patch.object(stage.time, "monotonic", side_effect=[1, 3]), self.assertRaises(ValueError):
                stage.read(root / "index.html", stage.PUBLIC_LIMIT, 2)


if __name__ == "__main__":
    unittest.main()
