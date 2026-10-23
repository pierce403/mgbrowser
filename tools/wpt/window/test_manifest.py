"""Owned data-only fixtures; no browser, worker, network or JavaScript runs."""
from copy import deepcopy
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).absolute().parent
ROOT = HERE.parents[2]
SPEC = importlib.util.spec_from_file_location("window_source_manifest", HERE / "check_manifest.py")
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)


class ManifestTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.manifest_raw = (ROOT / checker.MANIFEST).read_bytes()
        cls.parent_raw = (ROOT / checker.PARENT).read_bytes()
        cls.document = checker.strict_json(cls.manifest_raw)
        cls.parent = checker.parent_index(checker.strict_json(cls.parent_raw))
        cls.selected, cls.domain, cls.plan = checker.validate_manifest(cls.document, cls.parent)
        cls.original = {row["path"]: (ROOT / checker.UPSTREAM / row["path"]).read_bytes()
                        for row in cls.plan}

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mg-window-source-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.write(checker.MANIFEST, self.manifest_raw)
        self.write(checker.PARENT, self.parent_raw)
        for path, body in self.original.items():
            self.write(f"{checker.UPSTREAM}/{path}", body)

    def write(self, path, body):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(body)
        return target

    def changed_manifest(self, change):
        """Admit only a test-owned hash to reach deeper checks; no tool mode exists."""
        document = deepcopy(self.document)
        change(document)
        raw = (json.dumps(document, ensure_ascii=True) + "\n").encode()
        self.write(checker.MANIFEST, raw)
        with patch.object(checker, "MANIFEST_SHA", checker.sha256(raw)):
            return checker.check(self.root)

    def source(self, index=0, change=lambda raw: raw, test_change=lambda test: None):
        test = deepcopy(self.document["tests"][index])
        test_change(test)
        return checker.validate_source(test["path"], change(self.original[test["path"]]), test)

    def test_complete_fixed_source_check_succeeds_without_runtime(self):
        report = checker.check(self.root)
        self.assertEqual(report["status"], "SOURCE_VALIDATED")
        self.assertEqual((report["input_files"], report["input_bytes"], report["domain_html_files"],
                          report["selected_files"], report["source_valid_calls"],
                          report["source_invalid_calls"], report["source_declared_calls"]),
                         (20, 222805, 27, 16, 49, 30, 79))
        self.assertIs(report["runtime_executed"], False)
        self.assertIs(report["mg_node_ids_assigned"], False)
        self.assertLessEqual(report["total_read_bytes"], checker.MAX_BYTES)

    def test_manifest_and_parent_bytes_remain_independently_pinned(self):
        for path in (checker.MANIFEST, checker.PARENT):
            with self.subTest(path=path):
                original = (self.root / path).read_bytes()
                self.write(path, b" " + original[1:])
                with self.assertRaisesRegex(checker.Invalid, "SHA-256"):
                    checker.check(self.root)
                self.write(path, original)

    def test_strict_json_duplicates_numbers_utf8_and_trailing_data(self):
        bad = [b'{"a":1,"a":2}', b'{"x":{"a":1,"a":1}}', b'{"x":NaN}',
               b'{"x":Infinity}', b'{"x":1e9999}', b'{"x":1.0}',
               b'{"x":123456789012345678901}', b'{"x":"\xff"}', b'{}{}']
        for raw in bad:
            with self.subTest(raw=raw), self.assertRaises(checker.Invalid):
                checker.strict_json(raw)

    def test_closed_fields_boolean_integer_and_unknown_modes_reject(self):
        changes = [lambda d: d.update(extra=True), lambda d: d.pop("tests"),
                   lambda d: d.update(schema_version=True),
                   lambda d: d["selection"].update(selected_test_file_count=True),
                   lambda d: d["selection"].update(source_counts_are_not_observed_results=1),
                   lambda d: d["source_protocol"].update(observer_contract="arbitrary"),
                   lambda d: d["source_protocol"].update(url="https://other.test/"),
                   lambda d: d["tests"][0]["inline_text"].update(leading_lf_count=True),
                   lambda d: d["full_source_domain_roster"][0].update(selected=0)]
        for index, change in enumerate(changes):
            with self.subTest(index=index), self.assertRaises(checker.Invalid):
                self.changed_manifest(change)

    def test_inventory_selection_order_duplicates_missing_and_extra_reject(self):
        changes = [lambda d: d["files"].reverse(),
                   lambda d: d["files"].append(deepcopy(d["files"][0])),
                   lambda d: d["files"].pop(), lambda d: d["tests"].reverse(),
                   lambda d: d["tests"].append(deepcopy(d["tests"][0])),
                   lambda d: d["full_source_domain_roster"].pop(),
                   lambda d: d["files"][0].update(bytes=True),
                   lambda d: d["files"][0].update(sha256="0" * 64)]
        for index, change in enumerate(changes):
            with self.subTest(index=index), self.assertRaises(checker.Invalid):
                self.changed_manifest(change)

    def test_inline_hash_names_count_order_and_utf16_alias_reject(self):
        changes = [lambda t: t["inline_text"].update(sha256="0" * 64),
                   lambda t: t["inline_text"].update(utf8_bytes=1),
                   lambda t: t.update(source_declared_call_count=True),
                   lambda t: t["source_expected_raw_test_names"].reverse(),
                   lambda t: t["source_expected_raw_test_names"].__setitem__(0, "wrong"),
                   lambda t: t["source_expected_raw_test_names"].__setitem__(0, "\ud800"),
                   lambda t: t["source_expected_raw_test_names"].append("invented"),
                   lambda t: t.update(mg_node_id=1)]
        for index, change in enumerate(changes):
            with self.subTest(index=index), self.assertRaises(checker.Invalid):
                self.changed_manifest(lambda d: change(d["tests"][0]))

    def test_source_same_length_tampering_and_truncation_reject(self):
        path = self.selected[0]
        raw = self.original[path]
        for changed in (b"!" + raw[1:], raw[:-1]):
            self.write(f"{checker.UPSTREAM}/{path}", changed)
            with self.subTest(size=len(changed)), self.assertRaises(checker.Invalid):
                checker.check(self.root)

    def test_complete_helper_and_unselected_domain_bytes_are_checked(self):
        paths = ["resources/testharness.js", "resources/testharnessreport.js",
                 "css/support/parsing-testcommon.js", "LICENSE.md",
                 next(path for path in self.domain if path not in self.selected)]
        for path in paths:
            with self.subTest(path=path):
                raw = self.original[path]
                self.write(f"{checker.UPSTREAM}/{path}", b"!" + raw[1:])
                with self.assertRaisesRegex(checker.Invalid, "SHA-256"):
                    checker.check(self.root)
                self.write(f"{checker.UPSTREAM}/{path}", raw)

    def test_actual_domain_missing_extra_and_nonhtml_entry_reject(self):
        directory = f"{checker.UPSTREAM}/{checker.DIRECTORY}"
        for extra in ("unexpected-valid.html", "unexpected.js"):
            target = self.write(f"{directory}/{extra}", b"")
            with self.subTest(extra=extra), self.assertRaises(checker.Invalid):
                checker.check(self.root)
            target.unlink()
        (self.root / checker.UPSTREAM / self.domain[0]).unlink()
        with self.assertRaises(checker.Invalid):
            checker.check(self.root)

    def test_parent_inventory_missing_and_duplicate_paths_reject(self):
        for change in (lambda p: p["files"].append(deepcopy(p["files"][0])),
                       lambda p: p["files"][0].update(path="../outside"),
                       lambda p: p["files"][0].update(bytes=True)):
            parent = checker.strict_json(self.parent_raw)
            change(parent)
            with self.assertRaises(checker.Invalid):
                checker.parent_index(parent)
        (self.root / checker.PARENT).unlink()
        with self.assertRaises(OSError):
            checker.check(self.root)

    def test_noncanonical_paths_and_route_aliases_reject(self):
        for path in ("../outside", "/absolute", "a//b", "a/./b", "a/../b", "a%2fb",
                     "a?x", "a#x", "a\\b", "https:other", "a\0b", "a b", "caf\xe9"):
            with self.subTest(path=path), self.assertRaises(checker.Invalid):
                checker.canonical(path)
        for route in ("//resources/testharness.js", "/resources/../resources/testharness.js",
                      "/resources/testharness.js?x", "/resources/testharness.js#x",
                      "/resources/testharness%2ejs", "https://other.test/testharness.js"):
            with self.subTest(route=route), self.assertRaises(checker.Invalid):
                self.source(change=lambda raw: raw.replace(b"/resources/testharness.js", route.encode()))

    def test_source_protocol_metadata_and_attribute_negatives(self):
        replacements = [(b'<script src=', b'<script defer src='),
                        (b'<script src=', b'<script type="module" src='),
                        (b'<script src=', b'<script integrity="x" src='),
                        (b'<meta charset="utf-8">', b'<meta http-equiv="refresh" content="0;url=/trap">'),
                        (b'rel="help"', b'rel="stylesheet"'),
                        (b'<body>', b'<body onload="trap()">'),
                        (b'<body>', b'<body><img src="/trap">'),
                        (b'<script>\n', b'<script>\n\n'),
                        (b'<script>\n', b'<script>'),
                        (b'</script>\n</body>', b'</script><script>trap()</script>\n</body>')]
        for before, after in replacements:
            with self.subTest(after=after), self.assertRaises(checker.Invalid):
                self.source(change=lambda raw: raw.replace(before, after, 1))
        with self.assertRaises(checker.Invalid):
            self.source(change=lambda raw: raw.replace(b'<script src=', b'<script src="/trap" src=', 1))

    def test_original_inline_css_and_target_markup_are_not_optional(self):
        index = self.selected.index(f"{checker.DIRECTORY}/flex-valid.html")
        for before, after in ((b"width: 10px", b"width: 11px"),
                              (b'id="target"', b'id="other"'),
                              (b"<style>", b"<style>@import '/trap';")):
            with self.subTest(after=after), self.assertRaises(checker.Invalid):
                self.source(index, lambda raw: raw.replace(before, after, 1))

    def test_literal_reader_one_name_per_call_and_comments_not_calls(self):
        text = ('/* test_valid_value("flex", "ignored"); */\n'
                '// test_valid_value("flex", "also ignored");\n'
                'test_valid_value("flex", "x", ["alternative1", "alternative2"]);\n'
                'test_valid_value("flex", "test_valid_value(placeholder)");\n')
        self.assertEqual(checker.literal_calls(text, "flex", "valid"), [
            'e.style[\'flex\'] = "x" should set the property value',
            'e.style[\'flex\'] = "test_valid_value(placeholder)" should set the property value'])
        bad = ['test_valid_value("flex", trap());', 'test_valid_value("flex", "x", {"x":1});',
               'test_valid_value("flex", "x", ["x", 1]);', 'test_valid_value("flex", "x", "x", {});',
               'test_valid_value("other", "x");', 'test_invalid_value("flex", "x");',
               'test_valid_value("flex", "\\uD800");', '/* unterminated',
               'eval("anything");']
        for text in bad:
            with self.subTest(text=text), self.assertRaises(checker.Invalid):
                checker.literal_calls(text, "flex", "valid")

    def test_unsafe_manifest_paths_reject_before_input_body(self):
        for path in ("../outside", "/outside", "a%2fb"):
            with self.subTest(path=path), self.assertRaises(checker.Invalid):
                self.changed_manifest(lambda d: d["files"][0].update(path=path))
        with checker.Reader(self.root) as reader:
            with patch.object(checker.os, "open", side_effect=AssertionError("must not open")):
                with self.assertRaises(checker.Invalid):
                    reader.read("../outside", 1)

    def test_symlink_leaf_directory_and_repository_aliases_reject(self):
        leaf = self.root / checker.UPSTREAM / "resources/testharness.js"
        saved = leaf.with_name("saved.js")
        leaf.rename(saved)
        leaf.symlink_to(saved)
        with self.assertRaises((checker.Invalid, OSError)):
            checker.check(self.root)
        leaf.unlink()
        saved.rename(leaf)
        directory = self.root / checker.UPSTREAM / "resources"
        saved_directory = directory.with_name("saved-resources")
        directory.rename(saved_directory)
        directory.symlink_to(saved_directory, target_is_directory=True)
        with self.assertRaises((checker.Invalid, OSError)):
            checker.check(self.root)
        with tempfile.TemporaryDirectory(prefix="mg-window-alias-") as temporary:
            alias = Path(temporary) / "alias"
            alias.symlink_to(self.root, target_is_directory=True)
            with self.assertRaises(OSError):
                checker.Reader(alias)

    def test_nonregular_and_oversized_files_fail_before_body_read(self):
        for path, size, limit in ((checker.MANIFEST, checker.MAX_MANIFEST + 1, checker.MAX_MANIFEST),
                                  (checker.PARENT, checker.MAX_METADATA + 1, checker.MAX_METADATA)):
            target = self.root / path
            with target.open("wb") as stream:
                stream.truncate(size)
            with checker.Reader(self.root) as reader:
                with patch.object(checker.os, "read", side_effect=AssertionError("oversized body read")):
                    with self.assertRaisesRegex(checker.Invalid, "before body read"):
                        reader.read(path, limit, metadata=True)
        fifo = self.root / "fifo"
        os.mkfifo(fifo)
        with checker.Reader(self.root) as reader:
            with patch.object(checker.os, "read", side_effect=AssertionError("FIFO body read")):
                with self.assertRaisesRegex(checker.Invalid, "before body read"):
                    reader.read("fifo", 1)

    def test_shared_file_byte_metadata_and_plan_budgets_are_admitted(self):
        self.write("one", b"1234")
        self.write("two", b"5678")
        for limit, flags in (("MAX_BYTES", {}), ("MAX_METADATA", {"metadata": True})):
            with patch.object(checker, limit, 7), checker.Reader(self.root) as reader:
                self.assertEqual(reader.read("one", 4, **flags), b"1234")
                with patch.object(checker.os, "read", side_effect=AssertionError("over-budget body read")):
                    with self.assertRaisesRegex(checker.Invalid, "before body read"):
                        reader.read("two", 4, **flags)
        with patch.object(checker, "MAX_FILES", 1), checker.Reader(self.root) as reader:
            reader.read("one", 4)
            with self.assertRaisesRegex(checker.Invalid, "before body read"):
                reader.read("two", 4)
        with checker.Reader(self.root) as reader:
            reader.read("one", 4)
            with self.assertRaisesRegex(checker.Invalid, "before body read"):
                checker.admit_plan([{"bytes": checker.MAX_BYTES}], reader)
            with self.assertRaises(checker.Invalid):
                checker.admit_plan([{"bytes": 0}] * checker.MAX_FILES, reader)

    def test_file_mutation_during_read_is_not_accepted(self):
        self.write("one", b"1234")
        real_read = checker.os.read
        def changing_read(descriptor, size):
            result = real_read(descriptor, size)
            self.write("one", b"12345")
            return result
        with checker.Reader(self.root) as reader, patch.object(checker.os, "read", changing_read):
            with self.assertRaisesRegex(checker.Invalid, "changed during"):
                reader.read("one", 4)

    def test_main_has_no_override_mode_and_only_bounded_json_stdout(self):
        with patch.object(checker, "check", side_effect=AssertionError("unexpected read")) as check:
            for arguments in (["--manifest", "other"], ["--source", "other"], ["https://other.test/"]):
                with io.StringIO() as output, patch("sys.stdout", output):
                    self.assertEqual(checker.main(arguments), 1)
                    self.assertEqual(json.loads(output.getvalue())["status"], "ERROR")
            check.assert_not_called()
        with patch.object(checker, "check", return_value={"status": "SOURCE_VALIDATED", "runtime_executed": False}):
            with io.StringIO() as output, patch("sys.stdout", output):
                self.assertEqual(checker.main([]), 0)
                self.assertIs(json.loads(output.getvalue())["runtime_executed"], False)
        with patch.object(checker, "check", side_effect=checker.Invalid("x" * 10000)):
            with io.StringIO() as output, patch("sys.stdout", output):
                self.assertEqual(checker.main([]), 1)
                self.assertEqual(len(json.loads(output.getvalue())["reason"]), 512)

    def test_import_is_inert_and_unavailable_safe_reading_fails_closed(self):
        with patch.object(checker.os, "open", side_effect=AssertionError("import input read")):
            fresh = importlib.util.module_from_spec(SPEC)
            SPEC.loader.exec_module(fresh)
        with patch.object(checker.os, "name", "unsupported"):
            with self.assertRaisesRegex(checker.Invalid, "unavailable"):
                checker.Reader(self.root)


if __name__ == "__main__":
    unittest.main()
