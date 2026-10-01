"""The public score must retain the complete selected denominator."""

from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("wpt_site", Path(__file__).with_name("site.py"))
wpt_site = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wpt_site)


def sample():
    return {
        "schema_version": 1,
        "scope": "static-reftest-pilot",
        "upstream_revision": "a" * 40,
        "corpus_sha256": "b" * 64,
        "source_sha256": "c" * 64,
        "counts": {**dict.fromkeys(wpt_site.STATES, 1), "total": 6},
        "results": [
            {"test": f"/css/test-{index}.html", "status": state}
            for index, state in enumerate(wpt_site.STATES)
        ],
    }


class ScoreTests(unittest.TestCase):
    def test_accepts_consistent_counts_and_shows_every_outcome(self):
        score = sample()
        self.assertEqual(wpt_site.validate_score(score), score)
        output = wpt_site.render(score)
        for text in (
            "1 / 6 selected tests passing", "Fail: 1", "Unsupported: 1",
            "Errors: 1", "Timeouts: 1", "Crashes: 1", "/wpt-results.json",
            "not the full Web Platform Tests suite", "not a score for the installed release",
            score["upstream_revision"], score["source_sha256"], score["corpus_sha256"],
        ):
            self.assertIn(text, output)

    def test_rejects_invalid_or_missing_fingerprint_and_schema(self):
        for field, bad_values in {
            "schema_version": [True, "1", 2, None],
            "scope": ["full-wpt", None],
            "upstream_revision": ["main", "a" * 39, None],
            "corpus_sha256": ["b" * 63, "g" * 64, None],
            "source_sha256": ["<script>", None],
        }.items():
            for value in bad_values:
                with self.subTest(field=field, value=value):
                    score = sample()
                    score[field] = value
                    with self.assertRaises(ValueError):
                        wpt_site.validate_score(score)
            score = sample()
            del score[field]
            with self.assertRaises(ValueError):
                wpt_site.validate_score(score)

    def test_rejects_missing_extra_and_noninteger_counts(self):
        for state in (*wpt_site.STATES, "total"):
            score = sample()
            del score["counts"][state]
            with self.assertRaises(ValueError):
                wpt_site.validate_score(score)
        for value in (-1, True, 1.0, "1", None):
            score = sample()
            score["counts"]["PASS"] = value
            with self.assertRaises(ValueError):
                wpt_site.validate_score(score)
        score = sample()
        score["counts"]["SKIP"] = 0
        with self.assertRaises(ValueError):
            wpt_site.validate_score(score)

    def test_rejects_inconsistent_denominators_and_result_states(self):
        changes = []
        score = sample()
        score["counts"]["total"] = 7
        changes.append(score)
        score = sample()
        score["results"].pop()
        changes.append(score)
        score = sample()
        score["results"][0]["status"] = "FAIL"
        changes.append(score)
        score = sample()
        score["results"][0]["status"] = "SKIP"
        changes.append(score)
        score = sample()
        score["counts"] = {**dict.fromkeys(wpt_site.STATES, 0), "total": 0}
        score["results"] = []
        changes.append(score)
        for score in changes:
            with self.subTest(score=score), self.assertRaises(ValueError):
                wpt_site.validate_score(score)

    def test_rejects_duplicate_or_missing_test_ids(self):
        for value in ("", None, "test\n.html", sample()["results"][1]["test"]):
            score = sample()
            score["results"][0]["test"] = value
            with self.assertRaises(ValueError):
                wpt_site.validate_score(score)
        score = sample()
        del score["results"][0]["test"]
        with self.assertRaises(ValueError):
            wpt_site.validate_score(score)

    def test_rejects_wrong_container_types(self):
        for score in (None, [], "score"):
            with self.assertRaises(ValueError):
                wpt_site.validate_score(score)
        for key, value in (("counts", []), ("results", {}), ("results", [None] * 6)):
            score = sample()
            score[key] = value
            with self.assertRaises(ValueError):
                wpt_site.validate_score(score)

    def test_marker_integrity_and_other_html_preservation(self):
        before = "<header>installer unchanged</header>" + wpt_site.START
        after = wpt_site.END + "<footer>unchanged</footer>"
        self.assertEqual(wpt_site.replace_region(before + "old" + after, "new"), before + "new" + after)
        for document in ("", wpt_site.START, wpt_site.END + wpt_site.START,
                         wpt_site.START + wpt_site.START + wpt_site.END):
            with self.assertRaises(ValueError):
                wpt_site.replace_region(document, "new")

    def test_generate_then_check_and_reject_stale_score_without_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            score_path, html_path = root / "score.json", root / "index.html"
            score = sample()
            score_path.write_text(json.dumps(score), encoding="utf-8")
            html_path.write_text(wpt_site.START + "old" + wpt_site.END, encoding="utf-8")
            with self.assertRaises(ValueError):
                wpt_site.synchronize(score_path, html_path, check=True)
            wpt_site.synchronize(score_path, html_path)
            wpt_site.synchronize(score_path, html_path, check=True)
            original = html_path.read_bytes()
            changed = deepcopy(score)
            changed["source_sha256"] = "d" * 64
            score_path.write_text(json.dumps(changed), encoding="utf-8")
            with self.assertRaises(ValueError):
                wpt_site.synchronize(score_path, html_path, check=True)
            self.assertEqual(original, html_path.read_bytes())


if __name__ == "__main__":
    unittest.main()
