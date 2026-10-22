#!/usr/bin/env python3
"""Fast negative checks for the WPT adapter, without running a browser or network."""
import copy
from contextlib import contextmanager, ExitStack, redirect_stderr, redirect_stdout
import gzip
import importlib.util
import io
import json
from pathlib import Path
import struct
import tempfile
import time
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
        with self.assertRaisesRegex(ValueError, "Cannot inspect artifact"):
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
        legacy, expectations = runner.legacy_inputs()
        self.assertEqual(len(legacy["tests"]), 24)
        self.assertEqual(len(legacy["files"]), 41)
        self.assertTrue(set(entry["path"] for entry in legacy["files"]).issubset(files))
        runner.check_legacy_corpus(corpus)
        self.assertEqual(set(expectations["required_passes"]), set(expectations["tests"]))
        self.assertEqual(corpus["upstream"]["revision"], "84daed4ae966d9624bed22f43587c79283ad1fea")
        runnable = []
        for test in legacy["tests"]:
            if not runner.test_prerequisites(test, files):
                runnable.append(test["path"])
        self.assertEqual(len(runnable), 24)

    def test_inventory_bounds_are_checked_before_body_reads(self):
        for entries in ([self.manifest["files"][0]] * (runner.MAX_FILES + 1),
                        [{**self.manifest["files"][0], "bytes": runner.MAX_INPUT_BYTES + 1}],
                        [{**self.manifest["files"][0], "bytes": True}]):
            self.manifest["files"] = entries
            with patch.object(runner, "read_artifact", wraps=runner.read_artifact) as read:
                with self.assertRaisesRegex(ValueError, "before body read"):
                    self.load()
                self.assertEqual(read.call_count, 1, "only the bounded manifest may be read")

    def test_git_blob_identity_is_checked_against_unmodified_bytes(self):
        for entry in self.manifest["files"]:
            entry.update(git_blob_sha1="0" * 40, git_mode="100644")
        with self.assertRaisesRegex(ValueError, "Git blob identity"):
            self.load()

    def test_original_cohort_cannot_be_removed_rewritten_or_weakened(self):
        legacy, baseline = runner.legacy_inputs()
        for mutation in (lambda corpus: corpus["tests"].pop(),
                         lambda corpus: corpus["files"][0].update(sha256="0" * 64),
                         lambda corpus: corpus["tests"][0]["references"][0].update(relation="!=")):
            changed = copy.deepcopy(legacy)
            mutation(changed)
            with self.assertRaises(ValueError):
                runner.check_legacy_corpus(changed)
        for field in ("required_passes", "required_supported"):
            changed = copy.deepcopy(baseline)
            changed[field].pop()
            with self.assertRaisesRegex(ValueError, "weakened"):
                runner.check_legacy_expectations(changed)


class PrerequisiteTests(unittest.TestCase):
    def reasons(self, body, suffix="html", extras=None):
        name = "test." + suffix
        files = {name: body.encode()}
        files.update(extras or {})
        return "\n".join(runner.prerequisites(name, files))

    def test_plain_static_markup_and_local_css_are_admitted(self):
        self.assertEqual(self.reasons('<link rel="stylesheet" href="box.css"><div></div>',
                                     extras={"box.css": b"div { width: 20px; background: green }"}), "")

    def test_inline_html_scripts_xml_and_local_font_face_are_admitted(self):
        self.assertEqual(self.reasons("<script>void 0</script>"), "")
        self.assertEqual(self.reasons("<html/>", "xht"), "")
        self.assertEqual(self.reasons("<html/>", "xhtml"), "")
        self.assertEqual(self.reasons("<html/>", "xml"), "")
        self.assertEqual(self.reasons("div { font-family: Ahem }", "css"), "")
        self.assertEqual(self.reasons("@font-face { font-family: Custom }", "css"), "")

    def test_external_scripts_event_handlers_and_wait_protocols_are_unsupported(self):
        for body, reason in (
            ('<script src="/resources/testharness.js"></script>', "script setup"),
            ('<script type="module">void 0</script>', "script setup"),
            ('<body onload="void 0">', "event-handler"),
            ('<html class="reftest-wait">', "reftest-wait"),
            ('<html class="test-wait">', "reftest-wait"),
            ('<iframe src="child.html"></iframe>', "iframe setup"),
            ('<base href="/other/">', "base setup"),
        ):
            with self.subTest(body=body):
                self.assertIn(reason, self.reasons(body, extras={"resources/testharness.js": b"", "child.html": b""}))

    def test_svg_dynamic_setup_is_not_treated_as_static(self):
        self.assertIn("script setup", self.reasons("<svg><script>void 0</script></svg>", "svg"))
        self.assertIn("event-handler", self.reasons('<svg onload="void 0"/>', "svg"))
        self.assertIn("external references", self.reasons('<svg><use href="#shape"/></svg>', "svg"))

    def test_imports_timing_and_server_metadata_are_unsupported(self):
        for body, suffix, expected in (
            ('@import "other.css";', "css", "CSS imports"),
            ("div { animation-duration: 1s }", "css", "animation/timing"),
            ("div { transition: width 1s }", "css", "animation/timing"),
            ("{{host}}", "html", "server substitution"),
            ('<meta name="fuzzy" content="1;1">', "html", "WPT fuzzy"),
            ('<meta name="flags" content="interact">', "html", "WPT flags"),
            ('<meta name="variant" content="?mode=1">', "html", "WPT variant"),
        ):
            with self.subTest(body=body, suffix=suffix):
                self.assertIn(expected, self.reasons(body, suffix, extras={"other.css": b""}))

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
        self.assertIn("CSS imports", self.reasons('<link rel="stylesheet" href="import.css">',
                                                  extras={"import.css": b'@import "other.css";', "other.css": b""}))
        self.assertEqual(self.reasons('body { background-image: url(test.css) }', "css"), "")

    def test_comments_strings_and_script_literals_do_not_create_css_protocols(self):
        self.assertEqual(self.reasons('<style>/* @import "missing.css"; animation: x */ div{content:"transition: x; url(fake.png)"}</style><script>var s="animation: x";</script>'), "")

    def test_meta_refresh_is_source_unsupported_before_any_navigation(self):
        for html in ('<META HTTP-EQUIV="Refresh" content="0; url=https://foreign.invalid/">',
                     '<html xmlns="http://www.w3.org/1999/xhtml"><head><meta http-equiv="refresh" content="0;url=/next"/></head></html>'):
            self.assertIn("meta-refresh navigation", self.reasons(html))

    def test_reference_query_nonlocal_and_chain_stay_in_denominator(self):
        for href in ("ref.html?mode=1", "https://foreign.invalid/ref.html"):
            files = {"test.html": f'<link rel=match href="{href}">'.encode()}
            test = {"path": "test.html", "references": runner.reference_edges("test.html", files["test.html"])}
            self.assertTrue(runner.test_prerequisites(test, files))
            self.assertIsNone(test["references"][0]["path"])
        files = {"test.html": b'<link rel=match href="ref.html">', "ref.html": b'<link rel=match href="third.html">', "third.html": b'<p>ref</p>'}
        test = {"path": "test.html", "references": runner.reference_edges("test.html", files["test.html"])}
        self.assertIn("chained-reference", "\n".join(runner.test_prerequisites(test, files)))

    def test_svg_images_remain_static_but_top_level_resource_placeholders_cannot_pass(self):
        svg = b'<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>'
        files = {"test.html": b'<link rel=match href="ref.html"><img src="shape.svg">', "ref.html": b'<p>reference</p>', "shape.svg": svg}
        test = {"path": "test.html", "references": runner.reference_edges("test.html", files["test.html"])}
        self.assertEqual(runner.test_prerequisites(test, files), [])
        for suffix in ("svg", "xml", "png"):
            name = "reference." + suffix
            files[name] = svg
            test["references"] = [{"relation": "==", "href": name, "path": name}]
            self.assertIn("top-level resource-document", "\n".join(runner.test_prerequisites(test, files)))

    def test_verified_upstream_absence_is_failure_but_unknown_missing_is_error(self):
        source, href, path = sorted(runner.REVIEWED_MISSING)[0]
        files = {source: f'<link rel=stylesheet href="{href}">'.encode()}
        record = {"source": source, "href": href, "path": path,
                  "evidence": {"directory_tree_sha": runner.FLEX_TREE, "inventory_prefix": "css/css-flexbox", "absent_from_complete_tree": True}}
        missing = runner.missing_dependencies({"upstream_missing_dependencies": [record]}, files)
        with self.assertRaises(runner.UpstreamSourceFailure):
            runner.prerequisites(source, files, missing=missing)
        with self.assertRaisesRegex(ValueError, "Missing resource"):
            runner.prerequisites(source, files)
        record["evidence"]["absent_from_complete_tree"] = False
        with self.assertRaises(ValueError):
            runner.missing_dependencies({"upstream_missing_dependencies": [record]}, files)


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
            ({"content_height": runner.HEIGHT + 1}, "FAIL"),
        ):
            with self.subTest(change=change):
                self.assertEqual(runner.page_failure(valid_metadata() | change)[0], status)

    def test_every_render_diagnostic_kind_blocks_pass_without_message_exemptions(self):
        self.assertIsNone(runner.page_failure(valid_metadata()))
        for kind in ("css-parse", "css-unsupported", "image-unsupported", "layout-fallback",
                     "style-fallback", "future-informational"):
            for message in ("Known implementation gap", "Intentionally invalid CSS", ""):
                metadata = valid_metadata() | {"diagnostics": [{"kind": kind, "message": message}]}
                with self.subTest(kind=kind, message=message):
                    self.assertEqual(runner.page_failure(metadata),
                                     ("FAIL", "Rendering diagnostics violate the diagnostic-free reftest precondition"))

    def test_truncated_diagnostic_or_omission_cannot_establish_empty_evidence(self):
        metadata = valid_metadata() | {"diagnostics": [{"kind": "css-parse", "truncated": True}]}
        self.assertEqual(runner.page_failure(metadata)[0], "FAIL")
        metadata = valid_metadata() | {"diagnostics": [], "diagnostics_omitted": 1}
        self.assertEqual(runner.page_failure(metadata)[0], "FAIL")
        self.assertIsNone(runner.page_failure(valid_metadata() | {"diagnostics": [], "diagnostics_omitted": 0}))

    def test_malformed_metadata_and_field_types_are_errors(self):
        for malformed in (None, [], 1, "metadata"):
            with self.subTest(malformed=malformed):
                self.assertEqual(runner.page_failure(malformed)[0], "ERROR")
        for change in ({"diagnostics": None}, {"diagnostics": {}}, {"diagnostics": ["layout-fallback"]},
                       {"diagnostics": [{}]},
                       {"diagnostics": [{"kind": None}]}, {"resource_warnings": "none"},
                       {"content_height": "600"}, {"content_height": True},
                       {"content_height": -1}, {"non_uniform": 1}, {"schema_version": True},
                       {"scale": True}, {"diagnostics_omitted": "0"}, {"diagnostics_omitted": -1},
                       {"diagnostics_omitted": False}, {"diagnostics_omitted": None}, {"diagnostics_omitted": []},
                       {"resource_warnings": [None]}):
            with self.subTest(change=change):
                self.assertEqual(runner.page_failure(valid_metadata() | change)[0], "ERROR")
        metadata = valid_metadata()
        del metadata["diagnostics_omitted"]
        self.assertEqual(runner.page_failure(metadata)[0], "ERROR")


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
        self.prefix.with_suffix(".rgb.gz").unlink(missing_ok=True)
        metadata = valid_metadata() | {"url": self.base + self.name, "final_url": self.base + self.name} | (metadata or {})
        self.prefix.with_suffix(".json").write_text(json.dumps(metadata), encoding="utf-8")
        self.prefix.with_suffix(".rgb").write_bytes(nonuniform_pixels() if pixels is None else pixels)
        self.prefix.with_suffix(".png").write_bytes(b"\x89PNG\r\n\x1a\n" + struct.pack(">I", 13) + b"IHDR" + struct.pack(">II", runner.WIDTH, runner.HEIGHT) + bytes(9))

    def test_timeout_launch_error_and_process_exit_classification(self):
        cases = (("TIMEOUT", -9), ("ERROR", None), ("CRASH", -11), ("ERROR", 1))
        for expected, code in cases:
            with self.subTest(expected=expected, code=code), patch.object(
                    runner, "run_bounded",
                    return_value={"status": expected, "returncode": code, "reason": "mocked failure", "elapsed_ms": 1}) as process:
                self.assertEqual(self.render()["status"], expected)
                self.assertEqual(process.call_args.kwargs["timeout"], 0.25)
                self.assertEqual(set(process.call_args.kwargs["env"]), {"PATH", "LANG", "LC_ALL"})

    @patch.object(runner, "run_bounded", return_value={"status": "OK", "returncode": 0, "reason": "", "elapsed_ms": 1})
    def test_missing_bad_and_wrong_page_artifacts_are_errors(self, _process):
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts(pixels=b"short")
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts({"final_url": self.base + "different.html"})
        self.assertEqual(self.render()["status"], "ERROR")
        self.prefix.with_suffix(".json").write_text("not json", encoding="utf-8")
        self.assertEqual(self.render()["status"], "ERROR")

    @patch.object(runner, "run_bounded", return_value={"status": "OK", "returncode": 0, "reason": "", "elapsed_ms": 1})
    def test_render_evidence_preserves_page_failures(self, _process):
        self.artifacts({"resource_warnings": ["missing CSS"]})
        self.assertEqual(self.render()["status"], "FAIL")
        self.artifacts({"content_height": runner.HEIGHT + 1})
        self.assertEqual(self.render()["status"], "FAIL")
        self.artifacts({"diagnostics_omitted": 1})
        self.assertEqual(self.render()["status"], "FAIL")
        self.artifacts()
        result = self.render()
        self.assertEqual(result["status"], "OK")
        self.assertEqual(result["rgb_sha256"], runner.digest(nonuniform_pixels()))
        self.assertFalse(self.prefix.with_suffix(".rgb").exists())
        self.assertEqual(runner.read_pixels(self.output, self.prefix.name), nonuniform_pixels())

    @patch.object(runner, "run_bounded", return_value={"status": "OK", "returncode": 0, "reason": "", "elapsed_ms": 1})
    def test_valid_matching_pixels_with_any_diagnostic_remain_visible_failure(self, _process):
        diagnostics = [{"kind": "future-informational", "message": "No kind is silently exempt", "truncated": False}]
        self.artifacts({"diagnostics": diagnostics})
        result = self.render()
        self.assertEqual(result["status"], "FAIL")
        self.assertEqual(result["metadata"]["diagnostics"], diagnostics)
        self.assertEqual(result["metadata"]["diagnostics_omitted"], 0)
        self.assertEqual(runner.read_pixels(self.output, self.prefix.name), nonuniform_pixels())

    @patch.object(runner, "run_bounded", return_value={"status": "OK", "returncode": 0, "reason": "", "elapsed_ms": 1})
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

    @patch.object(runner, "run_bounded", return_value={"status": "OK", "returncode": 0, "reason": "", "elapsed_ms": 1})
    def test_actual_pixels_must_agree_with_uniformity_evidence(self, _process):
        self.artifacts(pixels=bytes(runner.RGB_BYTES))
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts({"non_uniform": False})
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts({"non_uniform": False}, pixels=bytes(runner.RGB_BYTES))
        self.assertEqual(self.render()["status"], "FAIL")

    @patch.object(runner, "run_bounded", return_value={"status": "OK", "returncode": 0, "reason": "", "elapsed_ms": 1})
    def test_missing_or_wrong_dimension_png_is_not_accepted(self, _process):
        self.artifacts()
        self.prefix.with_suffix(".png").unlink()
        self.assertEqual(self.render()["status"], "ERROR")
        self.artifacts()
        self.prefix.with_suffix(".png").write_bytes(b"not a PNG")
        self.assertEqual(self.render()["status"], "ERROR")

    @patch.object(runner, "run_bounded", return_value={"status": "ERROR", "returncode": 2, "reason": "exit 2", "elapsed_ms": 1, "completed": True})
    def test_structured_exit_two_distinguishes_admitted_failure_and_infrastructure(self, _process):
        envelope = {"schema_version": 1, "outcome": "FAIL", "kind": "admitted-execution",
                    "url": self.base + self.name, "phase": "script", "reason": "Missing DOM operation"}
        for outcome, kind, phase in (("FAIL", "admitted-execution", "script"), ("FAIL", "admitted-execution", "navigation"),
                                     ("ERROR", "infrastructure", "artifact"), ("ERROR", "infrastructure", "script-host"),
                                     ("TIMEOUT", "deadline", "navigation")):
            self.prefix.with_suffix(".json").write_text(json.dumps(envelope | {"outcome": outcome, "kind": kind, "phase": phase}))
            self.assertEqual(self.render()["status"], outcome)
        for changes in ({"kind": "infrastructure"}, {"phase": "unknown"}, {"reason": ""},
                        {"reason": "é" * 513}, {"url": "http://foreign.invalid/"}, {"schema_version": True},
                        {"outcome": "UNSUPPORTED"}, {"kind": []}, {"unexpected": True}):
            self.prefix.with_suffix(".json").write_text(json.dumps(envelope | changes))
            self.assertEqual(self.render()["status"], "ERROR")

    def test_valid_failure_envelope_cannot_mask_process_output_or_artifact_overflow(self):
        envelope = {"schema_version": 1, "outcome": "FAIL", "kind": "admitted-execution",
                    "url": self.base + self.name, "phase": "script", "reason": "Admitted operation failed"}
        self.prefix.with_suffix(".json").write_text(json.dumps(envelope))
        for reason in ("Process output exceeds byte limit", "Artifact exceeds byte limit", "Owned process cleanup failed"):
            with patch.object(runner, "run_bounded", return_value={"status": "ERROR", "returncode": 2,
                              "reason": reason, "elapsed_ms": 1, "completed": False}):
                result = self.render()
            self.assertEqual(result["status"], "ERROR")
            self.assertEqual(result["reason"], reason)

    def test_compressed_rgb_rejects_bombs_truncation_and_wrong_dimensions(self):
        path = self.prefix.with_suffix(".rgb.gz")
        for data in (gzip.compress(bytes(runner.RGB_BYTES + 1)), gzip.compress(bytes(runner.RGB_BYTES - 1)),
                     gzip.compress(bytes(runner.RGB_BYTES))[:-4]):
            path.write_bytes(data)
            with self.assertRaises((ValueError, OSError, EOFError)):
                runner.read_pixels(self.output, self.prefix.name)

    @patch.object(runner, "run_bounded", return_value={"status": "OK", "returncode": 0, "reason": "", "elapsed_ms": 1})
    def test_deadline_and_artifact_bounds_are_forwarded_to_owned_process(self, process):
        self.artifacts()
        deadline = time.monotonic() + 10
        result = runner.render_page(Path("unused"), Path("font"), self.base, self.name, self.output, global_deadline=deadline)
        self.assertEqual(result["status"], "OK")
        self.assertEqual(process.call_args.kwargs["global_deadline"], deadline)
        self.assertEqual(process.call_args.kwargs["artifact_limits"][self.prefix.with_suffix(".rgb")], runner.RGB_BYTES)


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

    def test_pending_or_unknown_outcomes_are_never_scores(self):
        for status in (runner.PENDING, "OK", None):
            report = copy.deepcopy(self.report)
            report["results"][0]["status"] = status
            with self.assertRaisesRegex(ValueError, "cannot be accepted"):
                runner.score_from(report)


class CompleteSelectionTests(unittest.TestCase):
    def setUp(self):
        prefix = "css/css-flexbox/"
        self.files = {
            prefix + "a.htm": b'<link rel="match" href="reference/ref.xhtml">',
            prefix + "b.html": b"<p>not a reftest</p>",
            prefix + "reference/ref.xhtml": b'<html xmlns="http://www.w3.org/1999/xhtml"/>',
            prefix + "nested/check.svg": b'<svg xmlns="http://www.w3.org/2000/svg" xmlns:h="http://www.w3.org/1999/xhtml"><h:link rel="mismatch" href="../reference/ref.xhtml"/></svg>',
            prefix + "box.css": b"div{display:flex}",
            "resources/extra.css": b"body{margin:0}",
        }
        entries = [{"path": name, "bytes": len(data), "sha256": runner.digest(data),
                    "git_blob_sha1": runner.hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest(),
                    "git_mode": "100644"} for name, data in sorted(self.files.items())]
        flex = sorted(name for name in self.files if name.startswith(prefix))
        self.tree = runner.git_tree_sha([(entry["path"][len(prefix):], entry["git_mode"], entry["git_blob_sha1"])
                                         for entry in entries if entry["path"].startswith(prefix)])
        candidates = [name for name in flex if not name.endswith(".css")]
        selected = [name for name in candidates if runner.reference_edges(name, self.files[name])]
        path_hash = lambda names: runner.digest(("\n".join(names) + "\n").encode())
        self.corpus = {"files": entries, "tests": [{"path": name, "references": runner.reference_edges(name, self.files[name])}
                                                   for name in selected],
                       "selection": {"id": "css-flexbox-recursive-reftests-v2", "limit": len(selected),
                                     "directory_tree_sha": self.tree, "recursive_upstream_files_count": len(flex),
                                     "recursive_upstream_bytes": sum(len(self.files[name]) for name in flex),
                                     "recursive_markup_candidates_count": len(candidates), "selected_tests_count": len(selected),
                                     "candidate_paths_sha256": path_hash(candidates), "selected_paths_sha256": path_hash(selected)}}

    def check(self, corpus=None, files=None):
        with patch.object(runner, "FLEX_TREE", self.tree):
            runner.check_complete_selection(self.corpus if corpus is None else corpus, self.files if files is None else files)

    def test_complete_tree_and_source_selected_roster_includes_htm_and_namespaced_svg(self):
        self.check()
        self.assertEqual(len(self.corpus["tests"]), 2)
        self.assertTrue(self.corpus["tests"][0]["path"].endswith(".htm"))

    def test_selection_cannot_drop_case_or_relabel_candidate_or_tree(self):
        for mutate in (lambda corpus: corpus["tests"].pop(),
                       lambda corpus: corpus["selection"].update(candidate_paths_sha256="0" * 64),
                       lambda corpus: corpus["selection"].update(recursive_markup_candidates_count=1),
                       lambda corpus: corpus["selection"].update(recursive_upstream_bytes=0),
                       lambda corpus: corpus["files"][0].update(git_blob_sha1="0" * 40)):
            corpus = copy.deepcopy(self.corpus)
            mutate(corpus)
            with self.assertRaises(ValueError):
                self.check(corpus)
        for field in ("git_blob_sha1", "git_mode"):
            corpus = copy.deepcopy(self.corpus)
            corpus["files"][-1].pop(field)
            with self.assertRaisesRegex(ValueError, "every original Git blob"):
                self.check(corpus)

    def test_git_tree_uses_git_directory_sort_order_and_rejects_conflicts(self):
        blob = "1" * 40
        directory = runner.git_tree_sha([("file", "100644", blob)])
        body = b"100644 a.c\0" + bytes.fromhex(blob) + b"40000 a\0" + bytes.fromhex(directory)
        expected = runner.hashlib.sha1(b"tree " + str(len(body)).encode() + b"\0" + body).hexdigest()
        self.assertEqual(runner.git_tree_sha([("a/file", "100644", blob), ("a.c", "100644", blob)]), expected)
        with self.assertRaises(ValueError):
            runner.git_tree_sha([("a", "100644", blob), ("a/file", "100644", blob)])


class BoundsAndEvidenceTests(unittest.TestCase):
    def test_atomic_json_overflow_keeps_prior_report_and_removes_temporary(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-report-test-") as directory:
            path = Path(directory) / "report.json"
            path.write_bytes(b"prior evidence")
            with self.assertRaisesRegex(ValueError, "exceeds"):
                runner.write_json(path, {"oversized": "x" * 50}, limit=20)
            self.assertEqual(path.read_bytes(), b"prior evidence")
            self.assertEqual(list(Path(directory).iterdir()), [path])

    def test_partial_report_has_complete_pending_denominator_and_cannot_score(self):
        corpus = {"tests": [{"path": "a.html"}, {"path": "b.html"}, {"path": "c.html"}]}
        report = {"selected_total": 3, "results": runner.pending_results(corpus)}
        report["results"][0]["status"] = "FAIL"
        runner.progress_counts(report)
        self.assertEqual(report["counts"]["total"], 3)
        self.assertEqual(report["counts"][runner.PENDING], 2)
        self.assertEqual(report["completed_total"], 1)
        with self.assertRaises(ValueError):
            runner.score_from(report)

    def test_budget_reserves_atomic_replacement_and_rejects_nonregular_entries(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-budget-test-") as directory:
            output = Path(directory)
            (output / "report.json").write_bytes(b"123456")
            with patch.object(runner, "OUTPUT_LIMIT", 10):
                runner.check_budget(output, reserve=4)
                with self.assertRaisesRegex(ValueError, "512 MiB"):
                    runner.check_budget(output, reserve=5)
            with self.assertRaisesRegex(ValueError, "deadline"):
                runner.check_budget(output, deadline=time.monotonic() - 1)
            (output / "unexpected-directory").mkdir()
            with self.assertRaisesRegex(ValueError, "nonregular"):
                runner.evidence_bytes(output)

    def test_request_audit_retains_finite_entries_and_records_overflow(self):
        requests = runner.RequestAudit(limit=2)
        self.assertTrue(requests.add({"path": "/a"}))
        self.assertTrue(requests.add({"path": "/b"}))
        self.assertFalse(requests.add({"path": "/c"}))
        self.assertEqual(len(requests), 2)
        self.assertTrue(requests.exceeded)

    def test_runtime_python_helpers_are_fingerprinted_but_tests_are_not(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-source-test-") as directory:
            root = Path(directory)
            here = root / "tools/wpt"
            here.mkdir(parents=True)
            for name in ("Cargo.toml", "Cargo.lock", "build.rs", "docs/cdp-protocol.json", "assets/mgbrowser-32.png", "examples/wpt_render.rs"):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"fixed")
            for name in ("run.py", "process_control.py", "import_corpus.py", "site.py", "test_runner.py"):
                (here / name).write_bytes(b"initial")
            with patch.multiple(runner, ROOT=root, HERE=here, __file__=str(here / "run.py")):
                before = runner.source_hash()
                (here / "test_runner.py").write_bytes(b"test change")
                self.assertEqual(runner.source_hash(), before)
                for name in ("process_control.py", "import_corpus.py", "site.py"):
                    (here / name).write_bytes(b"runtime change")
                    after = runner.source_hash()
                    self.assertNotEqual(after, before)
                    before = after

    def test_html_and_resource_mime_types_are_honest_and_no_js_lane_is_admitted(self):
        self.assertEqual(runner.MIME_TYPES[".htm"], "text/html; charset=utf-8")
        self.assertEqual(runner.MIME_TYPES[".xht"], "application/xhtml+xml")
        self.assertEqual(runner.MIME_TYPES[".xml"], "application/xml")
        self.assertEqual(runner.MIME_TYPES[".svg"], "image/svg+xml")
        self.assertNotIn(".js", runner.MIME_TYPES)


class PartialRunTests(unittest.TestCase):
    @contextmanager
    def admitted_run(self):
        """A fake adapter boundary; never executes a browser, build or server."""
        with tempfile.TemporaryDirectory(prefix="mg-wpt-main-test-") as directory:
            root = Path(directory)
            here = root / "tools/wpt"
            here.mkdir(parents=True)
            corpus = {"tests": [{"path": name, "references": [{"relation": "==", "href": "ref.html", "path": "ref.html"}]}
                                for name in ("a.html", "b.html")],
                      "upstream": {"revision": "2" * 40}, "selection": {"limit": 2}}
            (here / "corpus.json").write_text(json.dumps(corpus))
            (here / "score.json").write_bytes(b"untouched score")
            output = root / "evidence"
            files = {"a.html": b"<p>a</p>", "b.html": b"<p>b</p>", "ref.html": b"<p>ref</p>"}

            def build(*_args, **kwargs):
                kwargs["stdout_path"].write_text(json.dumps({"reason": "compiler-artifact", "target": {"name": "wpt_render", "kind": ["example"]}, "executable": str(root / "fake-adapter")}))
                return {"status": "OK"}

            @contextmanager
            def server(_files):
                yield "http://127.0.0.1:1/", runner.RequestAudit()

            with ExitStack() as stack:
                for target, value in (("ROOT", root), ("HERE", here)):
                    stack.enter_context(patch.object(runner, target, value))
                stack.enter_context(patch.object(runner, "load_corpus", return_value=(corpus, files, "0" * 64)))
                stack.enter_context(patch.object(runner, "source_hash", return_value="1" * 64))
                stack.enter_context(patch.object(runner, "digest_file", return_value="3" * 64))
                stack.enter_context(patch.object(runner, "tool_output", side_effect=lambda command, *_: "rustc 1.91.1 mock" if command[0] == "rustc" else "4" * 40))
                stack.enter_context(patch.object(runner, "run_bounded", side_effect=build))
                stack.enter_context(patch.object(runner, "serve", server))
                stack.enter_context(patch.object(runner, "check_controls", return_value={"passed": True}))
                stack.enter_context(patch.object(runner, "render_page", return_value={"status": "FAIL", "reason": "admitted mismatch", "artifact": "fake"}))
                stack.enter_context(patch.object(runner.sys, "argv", ["run.py", "--output", str(output), "--update-score"]))
                stack.enter_context(redirect_stdout(io.StringIO()))
                stack.enter_context(redirect_stderr(io.StringIO()))
                yield here, output

    def test_tool_failure_retains_every_pending_case_and_does_not_update_score(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-partial-test-") as directory:
            root = Path(directory)
            here = root / "tools/wpt"
            here.mkdir(parents=True)
            manifest = {"tests": [{"path": "a.html"}, {"path": "b.html"}]}
            (here / "corpus.json").write_text(json.dumps(manifest))
            score = here / "score.json"
            score.write_bytes(b"untouched score")
            output = root / "evidence"
            with patch.multiple(runner, ROOT=root, HERE=here), patch.object(runner, "load_corpus", return_value=(manifest, {}, "0" * 64)), \
                    patch.object(runner, "source_hash", return_value="1" * 64), \
                    patch.object(runner, "tool_output", side_effect=ValueError("tool launch failed")), \
                    patch.object(runner, "run_bounded") as process, \
                    patch.object(runner.sys, "argv", ["run.py", "--output", str(output), "--update-score"]), \
                    redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                self.assertEqual(runner.main(), 1)
                process.assert_not_called()
            report = json.loads((output / "report.json").read_bytes())
            self.assertEqual(report["selected_total"], 2)
            self.assertEqual(report["counts"]["total"], 2)
            self.assertEqual(report["counts"][runner.PENDING], 2)
            self.assertFalse(report["complete"])
            self.assertEqual([item["test"] for item in report["results"]], ["a.html", "b.html"])
            self.assertEqual(score.read_bytes(), b"untouched score")

    def test_successful_complete_fail_outcomes_are_still_accepted_and_deadline_is_forwarded(self):
        with self.admitted_run() as (here, output), patch.object(runner, "check_controls", return_value={"passed": True}) as controls:
            self.assertEqual(runner.main(), 0)
            report = json.loads((output / "report.json").read_bytes())
            score = json.loads((here / "score.json").read_bytes())
            self.assertTrue(report["complete"])
            self.assertEqual(report["counts"]["FAIL"], 2)
            self.assertEqual(report["counts"][runner.PENDING], 0)
            self.assertEqual(score["counts"]["total"], 2)
            self.assertGreater(controls.call_args.kwargs["global_deadline"], time.monotonic())

    def test_mid_loop_whole_run_deadline_keeps_finished_and_pending_cases(self):
        with self.admitted_run() as (here, output):
            budget = runner.check_budget
            calls = 0

            def expire(*args, **kwargs):
                nonlocal calls
                calls += 1
                # Two checkpoints, build admission, then first/second test.
                if calls == 5:
                    raise ValueError("Whole WPT run deadline expired")
                return budget(*args, **kwargs)

            with patch.object(runner, "check_budget", side_effect=expire):
                self.assertEqual(runner.main(), 1)
            report = json.loads((output / "report.json").read_bytes())
            self.assertEqual(report["counts"]["total"], 2)
            self.assertEqual(report["counts"]["FAIL"], 1)
            self.assertEqual(report["counts"][runner.PENDING], 1)
            self.assertFalse(report["complete"])
            self.assertEqual((here / "score.json").read_bytes(), b"untouched score")

    def test_failed_final_evidence_retention_cannot_publish_score(self):
        with self.admitted_run() as (here, _output), patch.object(runner, "finish_report", return_value=False):
            self.assertEqual(runner.main(), 1)
            self.assertEqual((here / "score.json").read_bytes(), b"untouched score")

    def test_reference_only_diagnostics_block_every_consumer_without_dropping_cases(self):
        with self.admitted_run() as (here, output):
            diagnostics = [{"kind": "css-parse", "message": "Ignored rendering requirement", "truncated": False}]

            def render(_binary, _font, _base, name, _output, **_kwargs):
                metadata = valid_metadata() | {"diagnostics": diagnostics if name == "ref.html" else []}
                failure = runner.page_failure(metadata)
                return {"status": failure[0] if failure else "OK", "reason": failure[1] if failure else "",
                        "artifact": name.replace(".", "-"), "metadata": metadata}

            with patch.object(runner, "render_page", side_effect=render) as renderer, \
                    patch.object(runner, "read_pixels") as pixels:
                self.assertEqual(runner.main(), 0)
                # The shared reference is cached once, but blocks both cases.
                self.assertEqual(renderer.call_count, 3)
                pixels.assert_not_called()
            report = json.loads((output / "report.json").read_bytes())
            score = json.loads((here / "score.json").read_bytes())
            self.assertEqual(report["selected_total"], 2)
            self.assertEqual(report["counts"]["total"], 2)
            self.assertEqual(report["counts"]["FAIL"], 2)
            self.assertEqual(report["counts"]["UNSUPPORTED"], 0)
            self.assertEqual(report["counts"][runner.PENDING], 0)
            self.assertEqual([item["test"] for item in report["results"]], ["a.html", "b.html"])
            self.assertTrue(all(item["status"] == "FAIL" and not item["comparisons"] for item in report["results"]))
            self.assertEqual(report["pages"]["ref.html"]["metadata"]["diagnostics"], diagnostics)
            self.assertTrue(all(page["status"] == "OK" for name, page in report["pages"].items() if name != "ref.html"))
            self.assertEqual(score["counts"]["total"], 2)
            self.assertEqual(score["counts"]["FAIL"], 2)


if __name__ == "__main__":
    unittest.main()
