"""Pure authored renderer and IO controls, not measured WPT outcomes."""
from copy import deepcopy
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import stat
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location("window_site", Path(__file__).with_name("site.py"))
site = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(site)
api = site.score_api
SYNC_SPEC = importlib.util.spec_from_file_location("window_site_sync", Path(__file__).with_name("site_sync.py"))
sync = importlib.util.module_from_spec(SYNC_SPEC)
SYNC_SPEC.loader.exec_module(sync)


def sample():
    rows = [{"test": path, "status": "FAIL", "source_declared_calls": calls,
             "observed_results": calls, "raw_harness_status": "Ok",
             "final_counts": {"total": calls, "pass": 0, "fail": calls,
                              "timeout": 0, "not_run": 0, "precondition_failed": 0,
                              "final_only": 0}}
            for path, calls in api.ROSTER]
    return {"schema_version": 1, "scope": api.SCOPE, "selection": api.SELECTION,
            "upstream_revision": api.UPSTREAM, "manifest_sha256": api.MANIFEST,
            "source_sha256": "a" * 64, "observer_contract": api.CONTRACT,
            "summary": api.summary(rows), "results": rows}


class WindowSiteTests(unittest.TestCase):
    def test_file_final_and_source_denominators_are_separate_and_nonmutating(self):
        score = sample()
        before = deepcopy(score)
        output = site.render(score)
        for text in ("0 / 16 selected Window files passing", "Fail: 16", "Pass: 0 / 79 observed final rows",
                     "Source-declared calls: 79; this is not a result denominator",
                     "Final inventories observed for 16 / 16 files", "Every selected file has an observed final inventory",
                     "not a score for the installed release", "not added to these totals",
                     "not the full WPT", "not public URLs", "/wpt-window-results.json",
                     score["upstream_revision"], score["manifest_sha256"], score["source_sha256"]):
            self.assertIn(text, output)
        self.assertNotIn("317", output)
        self.assertNotIn('href="case-', output)
        self.assertNotIn('href="tmp/', output)
        self.assertNotIn("unobserved subtest total: 0", output)
        self.assertEqual(score, before)

    def test_distinct_nonpass_states_and_raw_final_status_distribution(self):
        score = sample()
        first = score["results"][0]
        first.update(status="TIMEOUT", raw_harness_status="Timeout")
        first["final_counts"].update({"pass": 1, "fail": 2, "timeout": 1,
                                      "not_run": 1, "precondition_failed": 2})
        score["results"][1].update(status="PRECONDITION_FAILED", raw_harness_status="PreconditionFailed")
        score["results"][1]["final_counts"].update(fail=0, precondition_failed=8)
        score["results"][2].update(status="UNSUPPORTED", raw_harness_status=None,
                                    observed_results=0, final_counts=None)
        score["summary"] = api.summary(score["results"])
        output = site.render(score)
        for text in ("File outcomes: Fail: 13 · Timeouts: 1 · Precondition failed: 1 · Unsupported: 1",
                     "Errors: 0 · Crashes: 0 · Pending: 0", "Pass: 1 / 77 observed final rows",
                     "Fail: 64 · Timeouts: 1 · Not run: 1 · Precondition failed: 10"):
            self.assertIn(text, output)

    def test_genuine_positive_counts_are_not_hardcoded_or_merged_with_static(self):
        score = sample()
        score["results"][0]["status"] = "PASS"
        score["results"][0]["final_counts"].update({"pass": 7, "fail": 0})
        score["summary"] = api.summary(score["results"])
        output = site.render(score)
        self.assertIn("1 / 16 selected Window files passing", output)
        self.assertIn("Pass: 7 / 79 observed final rows", output)
        self.assertIn("Fail: 15", output)

    def test_partial_prefix_is_not_subtracted_from_prospective_total(self):
        score = sample()
        score["results"][0].update(status="TIMEOUT", raw_harness_status=None,
                                    observed_results=2, final_counts=None)
        score["summary"] = api.summary(score["results"])
        output = site.render(score)
        for text in ("Pass: 0 / 72 observed final rows", "Final inventories observed for 15 / 16 files",
                     "1 file lacks a final inventory; unobserved subtest total: unknown",
                     "Source-declared calls: 79", "Result prefixes without Completion are not counted as final inventory"):
            self.assertIn(text, output)
        self.assertNotIn("74 observed final rows", output)
        self.assertNotIn("7 unobserved", output)

    def test_no_final_inventory_does_not_fabricate_zero_of_79_results(self):
        score = sample()
        for row in score["results"]:
            row.update(status="TIMEOUT", raw_harness_status=None, observed_results=0, final_counts=None)
        score["summary"] = api.summary(score["results"])
        output = site.render(score)
        self.assertIn("Pass: 0 / 0 observed final rows", output)
        self.assertIn("16 files lack a final inventory; unobserved subtest total: unknown", output)
        self.assertNotIn("Pass: 0 / 79", output)

    def test_renderer_uses_score_validator_and_rejects_nonadoptable_metadata(self):
        changes = [lambda s: s.update(schema_version=True),
                   lambda s: s.update(scope="static-reftest-pilot"),
                   lambda s: s.update(source_sha256='<script>alert(1)</script>'),
                   lambda s: s.update(git_commit="b" * 40),
                   lambda s: s["summary"]["files"].update(FAIL=15),
                   lambda s: s["summary"]["actual_final_subtests"].update(fail=True)]
        for state in ("ERROR", "CRASH", "PENDING"):
            changes.append(lambda s, state=state: s["results"][0].update(status=state))
        for change in changes:
            score = sample()
            change(score)
            with self.subTest(change=change), self.assertRaises(ValueError):
                site.render(score)

    def test_window_region_never_replaces_static_region_and_is_idempotent(self):
        static = '<!-- wpt-score:start -->static 317 / 1011<!-- wpt-score:end -->'
        original = static + "\nbefore" + site.START + "old" + site.END + "after"
        generated = site.render(sample())
        result = site.replace_region(original, generated)
        self.assertEqual(result, static + "\nbefore" + site.START + generated + site.END + "after")
        self.assertEqual(site.replace_region(result, generated), result)

    def test_missing_duplicate_or_reversed_window_markers_reject(self):
        for document in ("", site.START, site.END, site.END + site.START,
                         site.START + site.START + site.END, site.START + site.END + site.END,
                         '<!-- wpt-score:start --><!-- wpt-score:end -->'):
            with self.subTest(document=document), self.assertRaises(ValueError):
                site.replace_region(document, "new")


class WindowSyncTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.score = self.root / "current-score.json"
        self.index = self.root / "index.html"
        self.score.write_text(json.dumps(sample()), encoding="utf-8")
        self.prefix = 'café\r\n<!-- wpt-score:start -->static unchanged<!-- wpt-score:end -->\r\n'
        self.suffix = '\r\n<footer>unchanged</footer>\r\n'
        self.original = (self.prefix + site.START + "old" + site.END + self.suffix).encode("utf-8")
        self.index.write_bytes(self.original)

    def test_write_only_window_region_preserves_other_bytes_mode_and_is_idempotent(self):
        self.index.chmod(0o640)
        self.assertTrue(sync.synchronize(self.score, self.index))
        expected = (self.prefix + site.START + site.render(sample()) + site.END + self.suffix).encode("utf-8")
        self.assertEqual(self.index.read_bytes(), expected)
        self.assertEqual(stat.S_IMODE(self.index.stat().st_mode), 0o640)
        identity = sync._stamp(self.index.stat())
        self.assertFalse(sync.synchronize(self.score, self.index))
        self.assertFalse(sync.synchronize(self.score, self.index, check=True))
        self.assertEqual(sync._stamp(self.index.stat()), identity)
        self.assertEqual(set(self.root.iterdir()), {self.score, self.index})

    def test_stale_check_never_writes_or_creates_scratch(self):
        with mock.patch.object(sync.tempfile, "mkstemp") as create:
            with self.assertRaisesRegex(ValueError, "stale"):
                sync.synchronize(self.score, self.index, check=True)
            create.assert_not_called()
        self.assertEqual(self.index.read_bytes(), self.original)

    def test_duplicate_nonfinite_overflow_invalid_utf8_and_json_leave_index_unchanged(self):
        values = [(b'{"x":1,"x":2}', "Duplicate"),
                  (b'{"x":NaN}', "Nonfinite"), (b'{"x":Infinity}', "Nonfinite"),
                  (b'{"x":-Infinity}', "Nonfinite"), (b'{"x":1e9999}', "Nonfinite"),
                  (b'\xff', None), (b'{', None)]
        for raw, message in values:
            self.score.write_bytes(raw)
            with self.subTest(raw=raw):
                if message:
                    with self.assertRaisesRegex(ValueError, message):
                        sync.synchronize(self.score, self.index)
                else:
                    with self.assertRaises(ValueError):
                        sync.synchronize(self.score, self.index)
            self.assertEqual(self.index.read_bytes(), self.original)

    def test_oversized_score_and_index_reject_before_body_open(self):
        for path in (self.score, self.index):
            with self.subTest(path=path):
                with path.open("wb") as output:
                    output.truncate(sync.LIMIT + 1)
                with mock.patch.object(sync.os, "open") as opened:
                    with self.assertRaisesRegex(ValueError, "bounded regular"):
                        sync._read_regular(path)
                    opened.assert_not_called()

    def test_symlink_file_parent_and_nonregular_inputs_reject_before_open(self):
        linked = self.root / "linked.json"
        linked.symlink_to(self.score)
        directory = self.root / "directory"
        directory.mkdir()
        parent = self.root / "alias"
        parent.symlink_to(self.root, target_is_directory=True)
        for path in (linked, directory, parent / self.score.name):
            with self.subTest(path=path), mock.patch.object(sync.os, "open") as opened:
                with self.assertRaises(ValueError):
                    sync._read_regular(path)
                opened.assert_not_called()
        self.index.unlink()
        self.index.symlink_to(self.score)
        with self.assertRaises(ValueError):
            sync.synchronize(self.score, self.index)

    def test_output_expansion_is_bounded_before_scratch_creation(self):
        with mock.patch.object(sync, "LIMIT", 1), mock.patch.object(sync.tempfile, "mkstemp") as create:
            with self.assertRaisesRegex(ValueError, "exceeds its byte limit"):
                sync.synchronize(self.score, self.index)
            create.assert_not_called()
        self.assertEqual(self.index.read_bytes(), self.original)

    def test_atomic_replace_failure_preserves_original_and_removes_only_own_scratch(self):
        with mock.patch.object(sync.os, "replace", side_effect=OSError("authored replace failure")):
            with self.assertRaises(OSError):
                sync.synchronize(self.score, self.index)
        self.assertEqual(self.index.read_bytes(), self.original)
        self.assertEqual(set(self.root.iterdir()), {self.score, self.index})

    def test_changed_input_before_commit_never_overwrites_new_content(self):
        for name in ("score", "index"):
            self.score.write_text(json.dumps(sample()), encoding="utf-8")
            self.index.write_bytes(self.original)
            path = getattr(self, name)
            def changed(value):
                path.write_bytes(b"authored concurrent content")
                return site.render(value)
            with self.subTest(path=path), mock.patch.object(sync.renderer, "render", side_effect=changed):
                with self.assertRaisesRegex(ValueError, "changed before publication"):
                    sync.synchronize(self.score, self.index)
            self.assertEqual(path.read_bytes(), b"authored concurrent content")
            if path == self.score:
                self.assertEqual(self.index.read_bytes(), self.original)

    def test_cli_is_fixed_path_check_only_and_reports_errors(self):
        with mock.patch.object(sync, "synchronize", return_value=False) as apply, redirect_stdout(io.StringIO()):
            self.assertEqual(sync.main([]), 0)
            apply.assert_called_once_with(sync.SCORE, sync.INDEX, check=False)
        with mock.patch.object(sync, "synchronize", return_value=False) as apply, redirect_stdout(io.StringIO()):
            self.assertEqual(sync.main(["--check"]), 0)
            apply.assert_called_once_with(sync.SCORE, sync.INDEX, check=True)
        with mock.patch.object(sync, "synchronize") as apply, redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                sync.main(["--score", str(self.score)])
            apply.assert_not_called()
        with mock.patch.object(sync, "synchronize", side_effect=ValueError("stale")), redirect_stderr(io.StringIO()) as errors:
            self.assertEqual(sync.main(["--check"]), 1)
            self.assertIn("stale", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
