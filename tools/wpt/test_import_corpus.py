"""Selection/import negative controls; no network, browser or engine execution."""
import codecs
from copy import deepcopy
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("wpt_import_corpus", Path(__file__).with_name("import_corpus.py"))
importer = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = importer
SPEC.loader.exec_module(importer)


class SourceParsingTests(unittest.TestCase):
    def test_original_cohort_edges_survive_shared_extractor(self):
        import json
        frozen = Path(__file__).parent / "baselines/css-flexbox-first-24-v1.corpus.json"
        baseline = frozen if frozen.exists() else Path(__file__).with_name("corpus.json")
        raw = baseline.read_bytes()
        self.assertEqual(importer.sha256(raw), importer.LEGACY_CORPUS_SHA256)
        old = json.loads(raw)
        upstream = Path(__file__).with_name("upstream")
        for test in old["tests"]:
            with self.subTest(test=test["path"]):
                self.assertEqual(importer.reference_edges(test["path"], (upstream / test["path"]).read_bytes()),
                                 test["references"])

    def test_html_xml_namespace_and_encoding_selection(self):
        cases = [
            ("test.html", b'<link rel="match" href="ref.html">'),
            ("test.htm", b'<link rel="match" href="ref.html">'),
            ("test.html", codecs.BOM_UTF8 + b'<link rel="match" href="ref.html">'),
            ("test.html", '<link rel="match" href="ref.html">'.encode("utf-16")),
            ("test.html", b'<meta charset="windows-1252"><title>caf\xe9</title><link rel="match" href="ref.html">'),
            ("test.xht", b'<html xmlns="http://www.w3.org/1999/xhtml"><link rel="match" href="ref.html"/></html>'),
            ("test.xml", b'<?xml version="1.0" encoding="ISO-8859-1"?><h:html xmlns:h="http://www.w3.org/1999/xhtml"><h:title>caf\xe9</h:title><h:link rel="match" href="ref.html"/></h:html>'),
            ("test.svg", b'<svg xmlns="http://www.w3.org/2000/svg" xmlns:h="http://www.w3.org/1999/xhtml"><h:link rel="match" href="ref.html"/></svg>'),
        ]
        for path, data in cases:
            with self.subTest(path=path, prefix=data[:20]):
                self.assertEqual(importer.reference_edges(path, data),
                                 [{"relation": "==", "href": "ref.html", "path": "ref.html"}])

    def test_foreign_namespace_comments_and_script_strings_are_not_references(self):
        self.assertEqual(importer.reference_edges("t.xml", b'<x xmlns="urn:foreign"><link rel="match" href="wrong"/></x>'), [])
        self.assertEqual(importer.reference_edges("t.html", b'<!-- <link rel="match" href="wrong"> --><script>var s=\'<link rel="match" href="wrong">\';</script>'), [])
        self.assertEqual(importer.reference_edges("t.html", b'<div xmlns:h="urn:foreign"><h:link rel="match" href="wrong"></h:link></div><link rel="match" href="ref.html">'),
                         [{"relation": "==", "href": "ref.html", "path": "ref.html"}])
        for body in (b'<svg><link rel="match" href="wrong"/></svg>',
                     b'<math><link rel="match" href="wrong"/></math>'):
            with self.subTest(body=body):
                self.assertEqual(importer.reference_edges("t.html", body), [])
                self.assertEqual(len(importer.parse_document("t.html", body).foreign_links), 1)
        for body in (b'<svg><foreignObject><link rel="match" href="ref.html"></foreignObject></svg>',
                     b'<math><annotation-xml encoding="text/html"><link rel="match" href="ref.html"></annotation-xml></math>',
                     b'<svg><g></g></svg><link rel="match" href="ref.html">'):
            with self.subTest(body=body):
                self.assertEqual(importer.reference_edges("t.html", body),
                                 [{"relation": "==", "href": "ref.html", "path": "ref.html"}])

    def test_nonlocal_dynamic_query_and_fragment_edges_remain_selected(self):
        for href, code in [("https://other.test/ref.html", "nonlocal-reference"),
                           ("//other.test/ref.html", "nonlocal-reference"),
                           ("ref.html?variant=1", "query-reference"),
                           ("ref.html#target", "fragment-reference"),
                           ("{{hosts[]}}/ref.html", "dynamic-reference"),
                           ("ref%2fother.html", "unsafe-reference")]:
            with self.subTest(href=href):
                edge = importer.resolved_edge("dir/test.html", href, "==")
                self.assertEqual((edge["href"], edge["path"], edge["unsupported"]["code"]), (href, None, code))
                if code in ("query-reference", "fragment-reference"):
                    self.assertEqual(edge["source_path"], "dir/ref.html")
                else:
                    self.assertNotIn("source_path", edge)

    def test_css_resources_ignore_comments_and_quoted_decoys(self):
        css = '''/* url(wrong1) */ p {content:"url(wrong2)";background:url("right.png")} @import 'theme.css'; a{src:URL(../font.ttf)}'''
        self.assertEqual(importer.css_resources(css), ["right.png", "theme.css", "../font.ttf"])
        self.assertEqual(importer.css_resources(r'p{background:url(\69 mage.png)}'), ["image.png"])
        self.assertEqual(importer.css_resources(r'p{background:u\72l(image.png)} @\69mport "theme.css";'),
                         ["image.png", "theme.css"])
        info = importer.parse_document("t.html", b'<svg><link rel="stylesheet match" href="foreign.css"/></svg>')
        self.assertEqual(info.references, [])
        self.assertEqual(info.resources, ["foreign.css"])

    def test_prerequisite_fields_preserve_elements_and_css_contexts(self):
        info = importer.parse_document("t.html", b'<style>p{color:red}</style><script src="a.js" type="module"></script><p style="background:url(x.png)" onclick="x()">x</p>')
        self.assertIn(("script", {"src": "a.js", "type": "module"}), info.elements)
        self.assertEqual(info.css_texts, ["p{color:red}", "background:url(x.png)"])
        self.assertEqual(info.resources, ["a.js", "x.png"])

    def test_duplicate_html_attributes_keep_first_like_production_parser(self):
        info = importer.parse_document("t.html", b'<meta http-equiv=refresh http-equiv=never content="0;url=https://other.test/"><img src=https://other.test/image.png src=local.png><link rel=match rel=help href=first.html href=second.html>')
        self.assertIn(("meta", {"http-equiv": "refresh", "content": "0;url=https://other.test/"}), info.elements)
        self.assertEqual(info.resources, ["https://other.test/image.png"])
        self.assertEqual(info.references, [("==", "first.html")])


class InventoryAdmissionTests(unittest.TestCase):
    def blob(self, path, data):
        return importer.Blob(path, importer.git_blob_sha1(data), len(data))

    def test_inventory_size_and_symlink_reject_before_body_read(self):
        budget = importer.Budget()
        with self.assertRaisesRegex(ValueError, "before body read"):
            budget.add(importer.Blob("large.bin", "1" * 40, importer.MAX_BYTES + 1))
        self.assertEqual((budget.bytes, budget.blobs), (0, {}))
        with self.assertRaisesRegex(ValueError, "nonregular"):
            budget.add(importer.Blob("link", "1" * 40, 4, "120000"))
        for index in range(importer.MAX_FILES):
            budget.add(importer.Blob(str(index), "1" * 40, 0))
        with self.assertRaisesRegex(ValueError, "before body read"):
            budget.add(importer.Blob("overflow", "1" * 40, 0))

    def test_git_blob_same_length_tampering_and_corrupt_cache_reject(self):
        source = importer.Source()
        blob = self.blob("file.html", b"expected")
        with self.assertRaisesRegex(ValueError, "verification failed"):
            source.verify(blob, b"tampered")
        with tempfile.TemporaryDirectory() as root:
            source.configure_cache(root)
            source.verify(blob, b"expected")
            self.assertTrue(source.cached(blob))
            (Path(root) / blob.sha).write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "verification failed"):
                source.cached(blob)

    def test_cache_fifo_and_oversized_legacy_manifest_reject_without_read(self):
        with tempfile.TemporaryDirectory() as root:
            source = importer.Source()
            source.configure_cache(root)
            empty = self.blob("empty.html", b"")
            os.mkfifo(Path(root) / empty.sha)
            with self.assertRaisesRegex(ValueError, "regular-file"):
                source.cached(empty)
            path = Path(root) / "legacy.json"
            path.write_bytes(b"x" * (importer.LEGACY_CORPUS_BYTES + 1))
            with self.assertRaisesRegex(ValueError, "regular-file"):
                importer.read_regular(path, importer.LEGACY_CORPUS_BYTES)

    def test_conflicting_metadata_and_noncanonical_paths_reject(self):
        budget = importer.Budget()
        self.assertTrue(budget.add(self.blob("a.html", b"one")))
        self.assertFalse(budget.add(self.blob("a.html", b"one")))
        with self.assertRaisesRegex(ValueError, "Conflicting"):
            budget.add(self.blob("a.html", b"two"))
        for path in ("../secret", "/absolute", "a//b", "a%2fb", "a?x", "a\\b"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                budget.add(self.blob(path, b"x"))

    def test_missing_dependency_exception_rejects_wrong_source_path_and_extra_absence(self):
        records = [{"source": source, "href": href, "path": path,
                    "evidence": {"directory_tree_sha": importer.FLEX_TREE,
                                 "inventory_prefix": importer.PREFIX,
                                 "absent_from_complete_tree": True}}
                   for source, href, path in sorted(importer.REVIEWED_MISSING_DEPENDENCIES)]
        paths = {record["source"] for record in records}
        importer.validate_reviewed_absences(records, paths)
        for field, value in (("source", importer.PREFIX + "/different.html"),
                             ("path", importer.PREFIX + "/different.css"),
                             ("href", "different.css")):
            invalid = deepcopy(records)
            invalid[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                importer.validate_reviewed_absences(invalid, paths)
        for invalid in (records[:1], records + [deepcopy(records[0])]):
            with self.assertRaises(ValueError):
                importer.validate_reviewed_absences(invalid, paths)
        with self.assertRaises(ValueError):
            importer.validate_reviewed_absences(records, paths | {records[0]["path"]})
        invalid = deepcopy(records)
        invalid[0]["evidence"]["directory_tree_sha"] = "0" * 40
        with self.assertRaises(ValueError):
            importer.validate_reviewed_absences(invalid, paths)

    def test_git_tree_identity_detects_omission_bytes_modes_and_path_changes(self):
        first = self.blob(importer.PREFIX + "/a.html", b"a")
        second = self.blob(importer.PREFIX + "/nested/b.html", b"b")
        expected = importer.git_subtree_sha1([first, second])
        self.assertEqual(importer.git_subtree_sha1([second, first]), expected)
        self.assertNotEqual(importer.git_subtree_sha1([first]), expected)
        changed = importer.Blob(second.path, second.sha, second.size, "100755")
        self.assertNotEqual(importer.git_subtree_sha1([first, changed]), expected)
        changed = self.blob(second.path, b"c")
        self.assertNotEqual(importer.git_subtree_sha1([first, changed]), expected)


if __name__ == "__main__":
    unittest.main()
