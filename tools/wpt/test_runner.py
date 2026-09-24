#!/usr/bin/env python3
"""Fast negative checks for the WPT adapter, without running a browser or network."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("mg_wpt_run", Path(__file__).with_name("run.py"))
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class CorpusTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mg-wpt-corpus-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.upstream = self.root / "upstream"
        self.upstream.mkdir()
        self.manifest_path = self.root / "corpus.json"
        self.files = {
            "test.html": b'<!doctype html><link rel="match" href="ref.html"><div>test</div>',
            "ref.html": b"<!doctype html><div>reference</div>",
        }
        for name, data in self.files.items():
            (self.upstream / name).write_bytes(data)
        self.manifest = {
            "schema_version": 1,
            "upstream": {"revision": "1" * 40},
            "selection": {"limit": 1},
            "files": [{"path": name, "bytes": len(data), "sha256": runner.digest(data)}
                      for name, data in sorted(self.files.items())],
            "tests": [{"path": "test.html", "references": [
                {"relation": "==", "href": "ref.html", "path": "ref.html"}]}],
        }

    def load(self):
        self.manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")
        return runner.load_corpus(self.manifest_path, self.upstream)

    def test_valid_inputs_and_manifest_digest(self):
        corpus, files, identity = self.load()
        self.assertEqual(corpus, self.manifest)
        self.assertEqual(files, self.files)
        self.assertEqual(identity, runner.digest(self.manifest_path.read_bytes()))

    def test_same_length_source_tampering_is_rejected(self):
        (self.upstream / "ref.html").write_bytes(self.files["ref.html"].replace(b"reference", b"different"))
        with self.assertRaisesRegex(ValueError, "Pinned input changed"):
            self.load()

    def test_missing_or_extra_inventory_is_rejected(self):
        extra = self.upstream / "unexpected.html"
        extra.write_bytes(b"unexpected")
        with self.assertRaisesRegex(ValueError, "inventory differs"):
            self.load()
        extra.unlink()
        (self.upstream / "ref.html").unlink()
        with self.assertRaises(FileNotFoundError):
            self.load()

    def test_duplicate_files_and_noncanonical_paths_are_rejected(self):
        self.manifest["files"].append(copy.deepcopy(self.manifest["files"][0]))
        with self.assertRaisesRegex(ValueError, "Duplicate or unsafe"):
            self.load()
        for name in ("../ref.html", "/ref.html", "a//b", "./ref.html", "a\\b", "a%2Fb", "a?x", "a#x"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                runner.safe_path(name)

    def test_original_relation_href_and_resolved_target_are_checked(self):
        edge = self.manifest["tests"][0]["references"][0]
        for key, value in (("relation", "!="), ("href", "other.html"), ("path", "test.html")):
            with self.subTest(key=key):
                original = edge[key]
                edge[key] = value
                with self.assertRaisesRegex(ValueError, "reference relations changed"):
                    self.load()
                edge[key] = original

    def test_missing_pinned_reference_is_rejected(self):
        (self.upstream / "ref.html").unlink()
        self.manifest["files"] = [item for item in self.manifest["files"] if item["path"] != "ref.html"]
        with self.assertRaisesRegex(ValueError, "Missing pinned reference"):
            self.load()

    def test_denominator_cannot_be_removed_or_duplicated(self):
        original = self.manifest["tests"][0]
        self.manifest["tests"] = []
        with self.assertRaisesRegex(ValueError, "denominator"):
            self.load()
        self.manifest["tests"] = [original, copy.deepcopy(original)]
        self.manifest["selection"]["limit"] = 2
        with self.assertRaisesRegex(ValueError, "denominator"):
            self.load()

    def test_checked_in_corpus_hashes_and_fixed_roster(self):
        corpus, files, _ = runner.load_corpus()
        self.assertEqual(len(corpus["tests"]), 24)
        self.assertEqual(len(files), 41)
        self.assertEqual(corpus["upstream"]["revision"], "84daed4ae966d9624bed22f43587c79283ad1fea")
        runnable = []
        for test in corpus["tests"]:
            pages = [test["path"]] + [edge["path"] for edge in test["references"]]
            if not any(runner.prerequisites(page, files) for page in pages):
                runnable.append(test["path"])
        self.assertEqual(len(runnable), 7)


class PrerequisiteTests(unittest.TestCase):
    def reasons(self, body, suffix="html", extras=None):
        name = "test." + suffix
        files = {name: body.encode()}
        files.update(extras or {})
        return "\n".join(runner.prerequisites(name, files))

    def test_plain_static_markup_and_local_css_are_admitted(self):
        self.assertEqual(self.reasons('<link rel="stylesheet" href="box.css"><div></div>',
                                     extras={"box.css": b"div { width: 20px; background: green }"}), "")

    def test_scripts_event_handlers_and_wait_protocols_are_unsupported(self):
        for body, reason in (
            ("<script>void 0</script>", "script setup"),
            ('<body onload="void 0">', "event-handler"),
            ('<html class="reftest-wait">', "reftest-wait"),
            ('<html class="test-wait">', "reftest-wait"),
            ('<iframe src="child.html"></iframe>', "iframe setup"),
            ('<base href="/other/">', "base setup"),
        ):
            with self.subTest(body=body):
                self.assertIn(reason, self.reasons(body))

    def test_svg_dynamic_setup_is_not_treated_as_static(self):
        self.assertIn("script setup", self.reasons("<svg><script>void 0</script></svg>", "svg"))
        self.assertIn("event-handler", self.reasons('<svg onload="void 0"/>', "svg"))
        self.assertIn("external references", self.reasons('<svg><use href="#shape"/></svg>', "svg"))

    def test_xml_fonts_imports_timing_and_server_metadata_are_unsupported(self):
        for body, suffix, expected in (
            ("<html/>", "xht", "XML/XHTML"),
            ("<html/>", "xhtml", "XML/XHTML"),
            ("<html/>", "xml", "XML/XHTML"),
            ("div { font-family: Ahem }", "css", "Ahem/webfont"),
            ("@font-face { font-family: Custom }", "css", "Ahem/webfont"),
            ('@import "other.css";', "css", "CSS imports"),
            ("div { animation-duration: 1s }", "css", "animation/timing"),
            ("div { transition: width 1s }", "css", "animation/timing"),
            ("{{host}}", "html", "server substitution"),
            ('<meta name="fuzzy" content="1;1">', "html", "WPT fuzzy"),
            ('<meta name="flags" content="interact">', "html", "WPT flags"),
            ('<meta name="variant" content="?mode=1">', "html", "WPT variant"),
        ):
            with self.subTest(body=body, suffix=suffix):
                self.assertIn(expected, self.reasons(body, suffix))

    def test_missing_local_resources_are_errors_not_silent_skips(self):
        with self.assertRaisesRegex(ValueError, "Missing resource"):
            self.reasons('<link rel="stylesheet" href="missing.css">')

    def test_external_and_nonstatic_resources_are_rejected_without_fetching(self):
        for href in ("https://example.invalid/style.css", "//example.invalid/style.css",
                     "style.css?variant=1", "style.css#fragment", "data:text/css,body{}"):
            with self.subTest(href=href):
                self.assertIn("Unsupported nonstatic/nonlocal", self.reasons(
                    '<link rel="stylesheet" href="' + href + '">'))

    def test_transitive_css_requirements_and_cycles_are_bounded(self):
        self.assertIn("Ahem/webfont", self.reasons('<link rel="stylesheet" href="font.css">',
                                                 extras={"font.css": b"body { font-family: Ahem }"}))
        self.assertEqual(self.reasons('body { background-image: url(test.css) }', "css"), "")


def valid_metadata():
    return {"schema_version": 1, "width": runner.WIDTH, "height": runner.HEIGHT,
            "scale": 1, "scripts": False, "browser_replay_equal": True,
            "resource_warnings": [], "diagnostics": [], "diagnostics_omitted": 0,
            "non_uniform": True, "content_height": runner.HEIGHT}


def nonuniform_pixels():
    return bytes(runner.RGB_BYTES - 3) + b"\x00\xff\x00"


class ComparisonTests(unittest.TestCase):
    def test_one_pixel_difference_is_not_a_match(self):
        black = bytes(runner.RGB_BYTES)
        changed = black[:-3] + b"\x01\x20\xff"
        self.assertEqual(runner.compare_pixels(black, black),
                         {"equal": True, "different_pixels": 0, "max_channel_difference": 0})
        self.assertEqual(runner.compare_pixels(black, changed),
                         {"equal": False, "different_pixels": 1, "max_channel_difference": 255})

    def test_truncated_or_wrong_dimension_pixels_are_errors(self):
        full = bytes(runner.RGB_BYTES)
        for bad in (b"", full[:-3], full + b"\0\0\0"):
            with self.subTest(length=len(bad)), self.assertRaisesRegex(ValueError, "RGB screenshot"):
                runner.compare_pixels(full, bad)

    def test_relations_use_any_match_but_all_mismatches(self):
        def passes(*pairs):
            return runner.relations_pass([{"relation": relation, "equal": equal}
                                          for relation, equal in pairs])
        self.assertFalse(passes())
        self.assertTrue(passes(("==", True)))
        self.assertFalse(passes(("==", False)))
        self.assertTrue(passes(("!=", False)))
        self.assertFalse(passes(("!=", True)))
        self.assertTrue(passes(("==", False), ("==", True), ("!=", False)))
        self.assertFalse(passes(("==", False), ("==", False), ("!=", False)))
        self.assertFalse(passes(("==", True), ("!=", False), ("!=", True)))

    def test_invalid_viewport_script_or_replay_evidence_is_error(self):
        self.assertIsNone(runner.page_failure(valid_metadata()))
        for key, value in (("schema_version", 2), ("width", 799), ("height", 599),
                           ("scale", 2), ("scripts", True), ("browser_replay_equal", False)):
            with self.subTest(key=key):
                metadata = valid_metadata()
                metadata[key] = value
                self.assertEqual(runner.page_failure(metadata)[0], "ERROR")

    def test_resource_fallback_blank_and_overflow_guards(self):
        for change, status in (
            ({"resource_warnings": ["missing stylesheet"]}, "FAIL"),
            ({"diagnostics_omitted": 1}, "FAIL"),
            ({"diagnostics": [{"kind": "layout-fallback"}]}, "FAIL"),
            ({"diagnostics": [{"kind": "style-fallback"}]}, "FAIL"),
            ({"non_uniform": False}, "FAIL"),
            ({"content_height": runner.HEIGHT + 1}, "UNSUPPORTED"),
        ):
            with self.subTest(change=change):
                self.assertEqual(runner.page_failure(valid_metadata() | change)[0], status)

    def test_malformed_metadata_and_field_types_are_errors(self):
        for malformed in (None, [], 1, "metadata"):
            with self.subTest(malformed=malformed):
                self.assertEqual(runner.page_failure(malformed)[0], "ERROR")
        for change in ({"diagnostics": None}, {"diagnostics": ["layout-fallback"]},
                       {"diagnostics": [{"kind": None}]}, {"resource_warnings": "none"},
                       {"content_height": "600"}, {"content_height": True},
                       {"non_uniform": 1}):
            with self.subTest(change=change):
                self.assertEqual(runner.page_failure(valid_metadata() | change)[0], "ERROR")


class RenderResultTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mg-wpt-render-test-")
        self.addCleanup(self.temporary.cleanup)
        self.output = Path(self.temporary.name)
        self.base = "http://127.0.0.1:8123/"
        self.name = "test.html"
        self.prefix = self.output / runner.digest(self.name.encode())[:20]

    def render(self):
        return runner.render_page(Path("not-executed"), Path("unused-font"),
                                  self.base, self.name, self.output, timeout=0.25)

    def artifacts(self, metadata=None, pixels=None):
        metadata = valid_metadata() | {"url": self.base + self.name, "final_url": self.base + self.name} | (metadata or {})
        self.prefix.with_suffix(".json").write_text(json.dumps(metadata), encoding="utf-8")
        self.prefix.with_suffix(".rgb").write_bytes(nonuniform_pixels() if pixels is None else pixels)

    def test_timeout_launch_error_and_process_exit_classification(self):
        cases = (
            (subprocess.TimeoutExpired(["mocked"], 0.25), None, "TIMEOUT"),
            (OSError("mocked launch error"), None, "ERROR"),
            (None, -11, "CRASH"),
            (None, 1, "ERROR"),
        )
        for error, code, expected in cases:
            with self.subTest(expected=expected, code=code), patch.object(
                    runner.subprocess, "run", side_effect=error,
                    return_value=SimpleNamespace(returncode=code)) as process:
                self.assertEqual(self.render()["status"], expected)
                self.assertEqual(process.call_args.kwargs["timeout"], 0.25)
                self.assertEqual(set(process.call_args.kwargs["env"]), {"PATH", "LANG", "LC_ALL"})

    @patch.object(runner.subprocess, "run", return_value=SimpleNamespace(returncode=0))
    def test_missing_bad_and_wrong_page_artifacts_are_errors(self, _process):
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts(pixels=b"short")
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts({"final_url": self.base + "different.html"})
        self.assertEqual(self.render()["status"], "ERROR")
        self.prefix.with_suffix(".json").write_text("not json", encoding="utf-8")
        self.assertEqual(self.render()["status"], "ERROR")

    @patch.object(runner.subprocess, "run", return_value=SimpleNamespace(returncode=0))
    def test_render_evidence_preserves_page_failures(self, _process):
        self.artifacts({"resource_warnings": ["missing CSS"]})
        self.assertEqual(self.render()["status"], "FAIL")
        self.artifacts({"content_height": runner.HEIGHT + 1})
        self.assertEqual(self.render()["status"], "UNSUPPORTED")
        self.artifacts({"diagnostics_omitted": 1})
        self.assertEqual(self.render()["status"], "FAIL")
        self.artifacts()
        result = self.render()
        self.assertEqual(result["status"], "OK")
        self.assertEqual(result["rgb_sha256"], runner.digest(nonuniform_pixels()))

    @patch.object(runner.subprocess, "run", return_value=SimpleNamespace(returncode=0))
    def test_scalar_or_mistyped_render_metadata_is_error(self, _process):
        self.artifacts()
        for malformed in (None, [], 1, "metadata"):
            with self.subTest(malformed=malformed):
                self.prefix.with_suffix(".json").write_text(json.dumps(malformed), encoding="utf-8")
                self.assertEqual(self.render()["status"], "ERROR")
        for change in ({"content_height": "600"}, {"diagnostics": [None]}, {"non_uniform": 1}):
            with self.subTest(change=change):
                self.artifacts(change)
                self.assertEqual(self.render()["status"], "ERROR")

    @patch.object(runner.subprocess, "run", return_value=SimpleNamespace(returncode=0))
    def test_actual_pixels_must_agree_with_uniformity_evidence(self, _process):
        self.artifacts(pixels=bytes(runner.RGB_BYTES))
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts({"non_uniform": False})
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts({"non_uniform": False}, pixels=bytes(runner.RGB_BYTES))
        self.assertEqual(self.render()["status"], "FAIL")


class ControlTests(unittest.TestCase):
    def test_authored_controls_require_css_colored_squares_and_white_background(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-control-test-") as directory:
            output = Path(directory)
            # Smaller authored frames exercise the same pixel checks quickly.
            with patch.multiple(runner, WIDTH=40, HEIGHT=40, RGB_BYTES=40 * 40 * 3):
                def square(color):
                    frame = bytearray(b"\xff\xff\xff" * (40 * 40))
                    for row in range(20):
                        offset = row * 40 * 3
                        frame[offset:offset + 20 * 3] = color * 20
                    return bytes(frame)

                results = [{"status": "OK", "artifact": "green"},
                           {"status": "OK", "artifact": "red"}]
                (output / "green.rgb").write_bytes(square(b"\x00\xff\x00"))
                (output / "red.rgb").write_bytes(square(b"\xff\x00\x00"))
                with patch.object(runner, "render_page", side_effect=results):
                    result = runner.check_controls(None, None, "http://127.0.0.1:1/", output)
                    self.assertTrue(result["passed"])

                for wrong in (b"\xff\xff\xff" * (40 * 40), square(b"\x00\x00\xff")):
                    with self.subTest(blank=wrong[:3] == b"\xff\xff\xff"):
                        (output / "green.rgb").write_bytes(wrong)
                        with patch.object(runner, "render_page", side_effect=results):
                            with self.assertRaisesRegex(ValueError, "controls rejected"):
                                runner.check_controls(None, None, "http://127.0.0.1:1/", output)

    def test_authored_controls_cannot_ignore_renderer_failure(self):
        with patch.object(runner, "render_page", side_effect=[{"status": "ERROR"}, {"status": "OK"}]):
            with self.assertRaisesRegex(ValueError, "controls failed"):
                runner.check_controls(None, None, "http://127.0.0.1:1/", Path("not-read"))


class CargoArtifactTests(unittest.TestCase):
    def artifact(self, **changes):
        return {"reason": "compiler-artifact", "target": {"name": "wpt_render", "kind": ["example"]},
                "executable": "/tmp/mg-wpt-custom-target/debug/examples/wpt_render"} | changes

    def test_uses_exact_custom_target_artifact_not_default_or_unrelated_binary(self):
        expected = self.artifact()
        unrelated = self.artifact(target={"name": "mgbrowser", "kind": ["bin"]},
                                  executable="/tmp/another-target/debug/mgbrowser")
        wrong_kind = self.artifact(target={"name": "wpt_render", "kind": ["bin"]})
        messages = "\n".join(json.dumps(event) for event in (
            unrelated, wrong_kind, expected, {"reason": "build-finished", "success": True}))
        self.assertEqual(runner.cargo_executable(messages), Path(expected["executable"]))

    def test_missing_or_ambiguous_executable_is_rejected(self):
        one = json.dumps(self.artifact())
        for messages in ("", json.dumps({"reason": "build-finished", "success": True}),
                         json.dumps(self.artifact(executable=None)), one + "\n" + one):
            with self.subTest(messages=messages), self.assertRaises(ValueError):
                runner.cargo_executable(messages)

    def test_malformed_cargo_records_are_rejected(self):
        for messages in ("not JSON", "null", "[]", json.dumps(self.artifact(target=None)),
                         json.dumps(self.artifact(executable=123))):
            with self.subTest(messages=messages), self.assertRaises(ValueError):
                runner.cargo_executable(messages)


class ScoreTests(unittest.TestCase):
    def setUp(self):
        self.report = {"upstream_revision": "1" * 40, "corpus_sha256": "2" * 64,
                       "source_sha256": "3" * 64,
                       "results": [{"test": str(index) + ".html", "status": status}
                                   for index, status in enumerate(runner.STATUSES)]}
        self.score = runner.score_from(self.report)
        self.baseline = {"corpus_sha256": self.score["corpus_sha256"],
                         "tests": [item["test"] for item in self.score["results"]],
                         "required_passes": ["0.html"],
                         "required_supported": ["0.html", "1.html"]}

    def test_all_outcomes_stay_in_total_and_unsupported_is_not_pass(self):
        self.assertEqual(self.score["counts"]["total"], 6)
        self.assertEqual(self.score["counts"]["PASS"], 1)
        for status in runner.STATUSES:
            self.assertEqual(self.score["counts"][status], 1)
        self.assertEqual(len(self.score["results"]), 6)

    def test_ratchet_allows_new_passes_but_no_lost_pass(self):
        runner.check_ratchet(self.score, self.baseline)
        self.score["results"][1]["status"] = "PASS"
        runner.check_ratchet(self.score, self.baseline)
        self.score["results"][0]["status"] = "FAIL"
        with self.assertRaisesRegex(ValueError, "WPT regression"):
            runner.check_ratchet(self.score, self.baseline)

    def test_ratchet_rejects_changed_removed_or_reordered_denominator(self):
        for mutate in (lambda score: score.update(corpus_sha256="4" * 64),
                       lambda score: score["results"].pop(),
                       lambda score: score["results"].reverse()):
            with self.subTest(mutate=mutate):
                score = copy.deepcopy(self.score)
                mutate(score)
                with self.assertRaises(ValueError):
                    runner.check_ratchet(score, self.baseline)

    def test_ratchet_rejects_reclassifying_known_failure_as_unsupported(self):
        self.score["results"][1]["status"] = "UNSUPPORTED"
        with self.assertRaisesRegex(ValueError, "Previously runnable"):
            runner.check_ratchet(self.score, self.baseline)


if __name__ == "__main__":
    unittest.main()
