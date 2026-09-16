#!/usr/bin/env python3
"""The fake executable below tests transport only, never engine semantics."""

import json
import os
from pathlib import Path
import signal
import tempfile
import unittest
from unittest.mock import patch

import runner
import check_report


class MetadataTests(unittest.TestCase):
    def parse(self, body):
        return runner.metadata("/*---\n" + body + "\n---*/\n")

    def test_default_and_strict_variants(self):
        self.assertEqual(runner.variants(self.parse("description: sample")), ["sloppy", "strict"])
        self.assertEqual(runner.variants(self.parse("flags: [onlyStrict]")), ["strict"])
        self.assertEqual(runner.variants(self.parse("flags: [noStrict]")), ["sloppy"])

    def test_raw_modules_do_not_get_wrappers(self):
        self.assertEqual(runner.variants(self.parse("flags: [module, raw]")), ["module-raw"])
        self.assertEqual(runner.variants(self.parse("flags: [raw]")), ["raw"])

    def test_multiline_includes_and_negative(self):
        meta = self.parse("includes:\n  - compareArray.js\n  - propertyHelper.js\nflags: [async]\nnegative:\n  phase: runtime\n  type: Test262Error")
        self.assertEqual(meta["includes"], ["compareArray.js", "propertyHelper.js"])
        self.assertEqual(meta["negative"], dict(phase="runtime", type="Test262Error"))

    def test_invalid_metadata_rejects(self):
        for body in ("flags: {unknown: true}", "negative: wrong", "flags: [onlyStrict, noStrict]"):
            with self.assertRaises(ValueError):
                runner.variants(self.parse(body))


class ClassificationTests(unittest.TestCase):
    def case(self, **extra):
        return dict(request=runner.request("true"), **extra)

    def classify(self, case, **response):
        return runner.classify(case, dict(response=dict(protocol=1, **response)))

    def test_runtime_negative_does_not_accept_parse_error(self):
        case = self.case(expected=dict(phase="runtime", type="SyntaxError"))
        self.assertEqual(self.classify(case, outcome="exception", phase="parse", error_type="SyntaxError"), "assertion-failure")
        self.assertEqual(self.classify(case, outcome="exception", phase="runtime", error_type="SyntaxError"), "pass")

    def test_parse_negative_accepts_early_but_not_untyped_error(self):
        case = self.case(expected=dict(phase="parse", type="SyntaxError"))
        self.assertEqual(self.classify(case, outcome="exception", phase="early", error_type="SyntaxError"), "pass")
        self.assertEqual(self.classify(case, outcome="exception", phase="parse"), "assertion-failure")

    def test_failure_types_stay_distinct(self):
        case = self.case()
        self.assertEqual(self.classify(case, outcome="unsupported"), "unsupported")
        self.assertEqual(self.classify(case, outcome="termination"), "termination")
        self.assertEqual(self.classify(case, outcome="exception", phase="harness"), "harness-failure")
        self.assertEqual(self.classify(case, outcome="exception", phase="runtime", error_type="Test262Error"), "assertion-failure")
        self.assertEqual(self.classify(case, outcome="exception", phase="runtime", error_type="ReferenceError"), "exception")

    def test_expected_true_and_async_completion_are_required(self):
        case = self.case(expected_value=True)
        self.assertEqual(self.classify(case, outcome="ok", value=1), "assertion-failure")
        self.assertEqual(self.classify(case, outcome="ok", value=True), "pass")
        case["request"]["async"] = True
        self.assertEqual(self.classify(case, outcome="ok", value=True, done="missing"), "timeout")
        self.assertEqual(self.classify(case, outcome="ok", value=True, done="error"), "assertion-failure")
        self.assertEqual(self.classify(case, outcome="ok", value=True, done="ok"), "pass")


class TransportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="mg-jsplan-test-")
        self.executable = Path(self.temp.name) / "fake-probe"

    def tearDown(self):
        self.temp.cleanup()

    def fake(self, body, **limits):
        self.executable.write_text("#!/usr/bin/python3\nimport json, os, signal, sys, time\n" + body + "\n")
        self.executable.chmod(0o700)
        return runner.execute(self.executable, runner.request("true"), **limits)

    def test_result_and_per_child_measurements(self):
        result = self.fake("request=json.load(sys.stdin)\nprint(json.dumps(dict(protocol=1,outcome='ok',phase='runtime',value=True)))")
        self.assertEqual(result["response"]["outcome"], "ok")
        self.assertGreater(result["peak_rss_bytes"], 0)
        self.assertGreater(result["wall_seconds"], 0)
        self.assertEqual(result["returncode"], 0)

    def test_deadline_kills_and_reaps(self):
        result = self.fake("time.sleep(10)", deadline=0.05)
        self.assertEqual(result["classification"], "timeout")
        self.assertEqual(result["returncode"], -signal.SIGKILL)
        self.assertLess(result["wall_seconds"], 1)
        self.assertTrue(result["reaped"])

    def test_stopped_supervisor_and_child_are_reaped_within_bound(self):
        result = self.fake("os.kill(os.getppid(),signal.SIGSTOP)\nos.kill(os.getpid(),signal.SIGSTOP)\ntime.sleep(10)", deadline=0.05)
        self.assertEqual(result["classification"], "timeout")
        self.assertTrue(result["reaped"])
        self.assertLess(result["wall_seconds"], 1)

    def test_failed_initial_signal_escalates_within_bound(self):
        # Simulate the initial graceful cleanup not completing. The owned
        # process group must still be killed and its supervisor reaped.
        with patch("runner.signal.pidfd_send_signal", return_value=None):
            result = self.fake("time.sleep(10)", deadline=0.05)
        self.assertEqual(result["classification"], "timeout")
        self.assertTrue(result["cleanup_escalated"])
        self.assertTrue(result["reaped"])
        self.assertLess(result["wall_seconds"], 1)

    def test_stdout_and_stderr_caps(self):
        for stream, key in (("stdout", "output_cap"), ("stderr", "stderr_cap")):
            result = self.fake(f"sys.{stream}.write('x'*5000)\nsys.{stream}.flush()\ntime.sleep(1)", **{key: 100})
            self.assertEqual(result["classification"], "termination")
            self.assertEqual(len(result[stream]), 100)

    def test_signal_and_nonzero_exit_are_not_passes(self):
        result = self.fake("os.kill(os.getpid(), signal.SIGTERM)")
        self.assertEqual(result["classification"], "crash")
        result = self.fake("sys.exit(7)")
        self.assertEqual(result["classification"], "termination")
        result = self.fake("sys.exit(143)")
        self.assertEqual(result["classification"], "termination")
        self.assertEqual(result["returncode"], 143)

    def test_bad_json_is_infrastructure_error(self):
        self.assertEqual(self.fake("print('bad')")["classification"], "infrastructure-error")
        self.assertEqual(self.fake("print('{}')")["classification"], "infrastructure-error")
        self.assertEqual(self.fake("print(json.dumps(dict(protocol=1,outcome='ok')))")["classification"], "infrastructure-error")
        self.assertEqual(self.fake("print(json.dumps(dict(protocol=1,outcome='ok',phase='parse')))")["classification"], "infrastructure-error")


class ReportTests(unittest.TestCase):
    def test_exact_inventory_rejects_missing_duplicate_changed_and_partial(self):
        digest = runner.sha256(runner.MANIFEST.read_bytes())
        expected = dict(manifest_sha256=digest, denominator={}, outcomes={"boa": {"pass": ["test"], "unsupported": ["gap"]}})
        rows = [dict(engine="boa", id="test", classification="pass"), dict(engine="boa", id="gap", classification="unsupported")]
        report = dict(complete=True, selected_lanes=None, manifest_sha256=digest, denominator={},
                      expected_result_count=2, results=rows)
        self.assertEqual(check_report.validate(report, expected), [])
        self.assertTrue(check_report.validate(dict(report, results=rows[:1]), expected))
        self.assertTrue(check_report.validate(dict(report, results=rows + rows[:1]), expected))
        self.assertTrue(check_report.validate(dict(report, complete=False), expected))
        self.assertTrue(check_report.validate(dict(report, selected_lanes=["test262"]), expected))
        self.assertTrue(check_report.validate(dict(report, manifest_sha256="bad"), expected))
        rows[1]["classification"] = "pass"
        self.assertTrue(check_report.validate(report, expected))


class IntegrityTests(unittest.TestCase):
    def test_changed_source_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.js"
            path.write_text("true;")
            digest = runner.sha256(path.read_bytes())
            self.assertEqual(runner.verified_read(path, digest), b"true;")
            path.write_text("false;")
            with self.assertRaises(ValueError):
                runner.verified_read(path, digest)

    def test_corpus_digest_covers_added_paths_and_rejects_links(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "test").mkdir()
            before = runner.corpus_fingerprint(root)
            (root / "test/new.js").write_text("true;")
            self.assertNotEqual(before, runner.corpus_fingerprint(root))
            (root / "test/link.js").symlink_to("new.js")
            with self.assertRaises(ValueError):
                runner.corpus_fingerprint(root)


if __name__ == "__main__":
    unittest.main()
