#!/usr/bin/env python3
"""Check the fixed source corpus only; see docs/WPT_WINDOW_PARSING.md.

No JavaScript, browser, product helper, network, output file or Mg parser is run.
The HTML reader extracts source facts only and never supplies Mg node IDs.
Importing this module performs no input reads. The CLI has no override modes.
"""
import hashlib
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import re
import stat
import sys

MANIFEST = "tools/wpt/window/corpus.json"
MANIFEST_SHA = "5223099747ed3b1669f311faa5007abb2c037bebb90aa7411b67c6470b4518d0"
PARENT = "tools/wpt/corpus.json"
PARENT_SHA = "d8c4bd6aacd93d964c10302e63b9c42576d785a055848772f9f71b2436f8075d"
UPSTREAM = "tools/wpt/upstream"
DIRECTORY = "css/css-flexbox/parsing"
REVISION = "84daed4ae966d9624bed22f43587c79283ad1fea"
DOMAIN_SHA = "12a14d357324d52bc83c4a313185c8bf4ebcd1cf4887f7159d8b4afb96121247"
SELECTED_SHA = "1d0958276e9ffddccb10561dc7611ab4d35fab02cc239737ed888ee6aed5581a"
STYLE_SHA = "39a2bf8324b868550c116c816cc3735ce0507475720bb1228869c92526affd7c"
MAX_FILES = 4096
MAX_BYTES = 10 * 1024 * 1024
MAX_METADATA = 4 * 1024 * 1024
MAX_MANIFEST = 128 * 1024
ROUTES = ["/resources/testharness.js", "/resources/testharnessreport.js",
          "/css/support/parsing-testcommon.js"]
COMMON = ["LICENSE.md", *(route[1:] for route in ROUTES)]


class Invalid(ValueError):
    """Source integrity or admission failed, not a browser test result."""


def require(condition, reason):
    if not condition:
        raise Invalid(reason)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def canonical(path):
    require(type(path) is str and 0 < len(path) <= 512 and path.isascii(),
            "Noncanonical path type/length")
    require(not path.startswith("/") and not any(c in path for c in "\\%?#:\0")
            and not any(ord(c) <= 32 or ord(c) == 127 for c in path)
            and all(part not in ("", ".", "..") for part in path.split("/")),
            "Noncanonical relative path")
    return path


def same(actual, expected, where):
    """Closed shapes and exact scalar types; True must never equal integer 1."""
    require(type(actual) is type(expected), f"{where}: wrong type")
    if type(expected) is dict:
        require(set(actual) == set(expected), f"{where}: unknown/missing fields")
        for key, value in expected.items():
            same(actual[key], value, f"{where}.{key}")
    elif type(expected) is list:
        require(len(actual) == len(expected), f"{where}: wrong array length")
        for index, value in enumerate(expected):
            same(actual[index], value, f"{where}[{index}]")
    else:
        require(actual == expected, f"{where}: value differs")


def fields(value, names, where):
    require(type(value) is dict and set(value) == set(names),
            f"{where}: unknown/missing fields or wrong object type")


def integer(value, maximum, where, minimum=0):
    require(type(value) is int and minimum <= value <= maximum,
            f"{where}: invalid integer")


def digest(value, where):
    require(type(value) is str and re.fullmatch(r"[0-9a-f]{64}", value),
            f"{where}: invalid SHA-256")


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate JSON field")
        result[key] = value
    return result


def _not_number(_value):
    raise Invalid("JSON floating/nonfinite numbers are not source metadata")


def _json_int(value):
    require(len(value) <= 20, "JSON integer is too large")
    return int(value)


def strict_json(raw):
    require(type(raw) is bytes and len(raw) <= MAX_METADATA, "JSON metadata limit")
    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=_pairs,
                          parse_int=_json_int, parse_float=_not_number,
                          parse_constant=_not_number)
    except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise Invalid("Malformed UTF-8 JSON metadata") from error


class Reader:
    """Descriptor-relative, no-follow admission before every bounded body read."""
    def __init__(self, root):
        require(os.name == "posix" and hasattr(os, "O_NOFOLLOW")
                and hasattr(os, "O_DIRECTORY") and os.open in os.supports_dir_fd,
                "Safe descriptor-relative reading is unavailable")
        root = Path(root)
        require(root.is_absolute() and ".." not in root.parts,
                "Repository root must be an absolute nonalias path")
        self.fd = os.open("/", os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        self.total = self.metadata = self.count = 0
        try:
            for part in root.parts[1:]:
                following = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                    dir_fd=self.fd)
                os.close(self.fd)
                self.fd = following
        except BaseException:
            os.close(self.fd)
            self.fd = None
            raise

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        os.close(self.fd)
        self.fd = None

    def directory(self, path):
        canonical(path)
        descriptor = os.dup(self.fd)
        try:
            for part in path.split("/"):
                following = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                    dir_fd=descriptor)
                os.close(descriptor)
                descriptor = following
            return descriptor
        except BaseException:
            os.close(descriptor)
            raise

    def read(self, path, limit, *, expected_size=None, expected_sha=None, metadata=False):
        canonical(path)
        parent, _, name = path.rpartition("/")
        directory = self.directory(parent) if parent else os.dup(self.fd)
        descriptor = None
        try:
            descriptor = os.open(name, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW,
                                 dir_fd=directory)
            before = os.fstat(descriptor)
            require(stat.S_ISREG(before.st_mode) and 0 <= before.st_size <= limit,
                    "Regular-file type/size admission failed before body read")
            require(expected_size is None or before.st_size == expected_size,
                    "Exact file size differs before body read")
            require(self.count < MAX_FILES and self.total + before.st_size <= MAX_BYTES,
                    "Total input budget exceeded before body read")
            require(not metadata or self.metadata + before.st_size <= MAX_METADATA,
                    "Metadata budget exceeded before body read")
            self.count += 1
            self.total += before.st_size
            self.metadata += before.st_size if metadata else 0
            parts = []
            remaining = before.st_size
            while remaining:
                block = os.read(descriptor, min(65536, remaining))
                require(block, "File truncated during body read")
                parts.append(block)
                remaining -= len(block)
            after = os.fstat(descriptor)
            stamp = lambda item: (item.st_dev, item.st_ino, item.st_size,
                                  item.st_mtime_ns, item.st_ctime_ns)
            require(stamp(before) == stamp(after), "File changed during body read")
            raw = b"".join(parts)
            require(expected_sha is None or sha256(raw) == expected_sha,
                    "Pinned file SHA-256 differs")
            return raw
        finally:
            if descriptor is not None:
                os.close(descriptor)
            os.close(directory)

    def domain(self):
        descriptor = self.directory(f"{UPSTREAM}/{DIRECTORY}")
        names = []
        try:
            with os.scandir(descriptor) as entries:
                for entry in entries:
                    require(len(names) < MAX_FILES, "Directory entry budget exceeded")
                    canonical(entry.name)
                    require(stat.S_ISREG(entry.stat(follow_symlinks=False).st_mode),
                            "Source domain contains a symlink/nonregular entry")
                    names.append(f"{DIRECTORY}/{entry.name}")
        finally:
            os.close(descriptor)
        return sorted(names)


def parent_index(parent):
    require(type(parent) is dict, "Parent inventory is not an object")
    same(parent.get("schema_version"), 1, "parent.schema_version")
    require(type(parent.get("upstream")) is dict, "Parent upstream missing")
    same(parent["upstream"].get("revision"), REVISION, "parent.revision")
    rows = parent.get("files")
    require(type(rows) is list and len(rows) <= MAX_FILES, "Parent file count")
    result = {}
    for row in rows:
        fields(row, ("path", "bytes", "git_blob_sha1", "git_mode", "sha256"), "parent.file")
        path = canonical(row["path"])
        integer(row["bytes"], MAX_BYTES, "parent.file.bytes")
        digest(row["sha256"], "parent.file.sha256")
        require(type(row["git_blob_sha1"]) is str
                and re.fullmatch(r"[0-9a-f]{40}", row["git_blob_sha1"]), "Parent Git blob")
        same(row["git_mode"], "100644", "parent.file.mode")
        require(path not in result, "Duplicate parent path")
        result[path] = {key: row[key] for key in ("path", "bytes", "sha256")}
    return result


def path_digest(paths):
    return sha256(("\n".join(paths) + "\n").encode("utf-8"))


def validate_manifest(document, parent):
    """Pure closed metadata checks; no arbitrary source or URL is admitted."""
    fields(document, ("schema_version", "status", "upstream", "selection",
                      "input_file_count", "input_bytes", "files",
                      "full_source_domain_roster", "source_protocol", "tests"), "manifest")
    same(document["schema_version"], 1, "schema_version")
    same(document["status"], "SOURCE_FROZEN_BEFORE_EXECUTION", "status")
    same(document["upstream"], {
        "repository": "https://github.com/web-platform-tests/wpt", "revision": REVISION,
        "local_root": UPSTREAM, "existing_inventory": PARENT,
        "existing_inventory_sha256": PARENT_SHA}, "upstream")
    domain = sorted(path for path in parent
                    if re.fullmatch(r"css/css-flexbox/parsing/[^/]+\.html", path))
    selected = [path for path in domain if re.search(r"-(valid|invalid)\.html$", path)]
    require(len(domain) == 27 and len(selected) == 16
            and path_digest(domain) == DOMAIN_SHA and path_digest(selected) == SELECTED_SHA,
            "Pinned source-domain/selection differs")
    same(document["selection"], {
        "id": "css-flexbox-window-declared-value-parsing-v1", "directory": DIRECTORY,
        "rule": "All direct-child .html basenames ending -valid.html or -invalid.html, sorted by exact relative path; no outcome/capability filter.",
        "full_direct_html_domain_count": 27, "selected_test_file_count": 16,
        "full_domain_paths_sha256_newline_terminated_utf8": DOMAIN_SHA,
        "selected_paths_sha256_newline_terminated_utf8": SELECTED_SHA,
        "source_valid_call_count": 49, "source_invalid_call_count": 30,
        "source_declared_call_count": 79, "source_counts_are_not_observed_results": True}, "selection")
    inputs = sorted(selected + COMMON)
    same(document["input_file_count"], 20, "input_file_count")
    same(document["input_bytes"], 222805, "input_bytes")
    same(document["files"], [parent[path] for path in inputs], "files")
    same(document["full_source_domain_roster"],
         [{**parent[path], "selected": path in selected} for path in domain], "domain_roster")
    same(document["source_protocol"], {
        "external_original_attributes": [[["src", route]] for route in ROUTES],
        "inline_original_attributes": [], "all_original_external_text_bodies_empty": True,
        "source_count_per_test": 4,
        "mg_node_ids": "Unbound: derive and validate through actual Mg parser/source admission; source ordinals are not node IDs.",
        "preserve_original_metadata": "All original charset/assert meta and inert author/help links; no original byte removed or rewritten.",
        "preserve_original_inline_style": "Only flex-valid.html: preserve its exact original style and container/target markup.",
        "name_derivation": "Original complete parsing-testcommon.js lines17-50, literal property/value arguments and JSON.stringify(value); all selected values are simple ASCII string literals. These expected raw names are source facts, not observed callback delivery.",
        "complete_helpers_and_license": "All original files in files are retained unmodified. No source/helper/body text is duplicated here.",
        "observer_contract": "PinnedWindowHarnessOutputDisabledV1", "normal_harness_timer_ms": 10000,
        "contains_runtime_results": False,
        "unexpected_resource_or_navigation_policy": "See docs/WPT_WINDOW_PARSING.md; no live-web fallback, authored setup/source injection, output-error waiver or larger resource profile is authorized."}, "source_protocol")
    tests = document["tests"]
    require(type(tests) is list and len(tests) == 16, "Test denominator differs")
    text_total = 0
    for path, test in zip(selected, tests):
        keys = {"path", "raw_html", "source_href_order", "inline_text",
                "source_declared_call_kind", "source_declared_call_count", "source_expected_raw_test_names"}
        if path == f"{DIRECTORY}/flex-valid.html":
            keys.add("original_inline_style")
        fields(test, keys, "test")
        canonical(test["path"])
        same(test["path"], path, "test.path")
        same(test["raw_html"], {key: parent[path][key] for key in ("bytes", "sha256")}, "test.raw_html")
        same(test["source_href_order"], [*ROUTES, None], "test.source_href_order")
        fields(test["inline_text"], ("utf8_bytes", "sha256", "leading_lf_count", "trailing_lf_count"), "inline_text")
        integer(test["inline_text"]["utf8_bytes"], MAX_BYTES, "inline bytes", 1)
        digest(test["inline_text"]["sha256"], "inline SHA")
        same(test["inline_text"]["leading_lf_count"], 1, "leading LF")
        same(test["inline_text"]["trailing_lf_count"], 1, "trailing LF")
        kind = "invalid" if path.endswith("-invalid.html") else "valid"
        same(test["source_declared_call_kind"], kind, "call kind")
        count = test["source_declared_call_count"]
        integer(count, 256, "call count", 1)
        names = test["source_expected_raw_test_names"]
        require(type(names) is list and len(names) == count, "Name/call cardinality differs")
        for name in names:
            require(type(name) is str and name.isascii() and 0 < len(name) <= 4096,
                    "Invalid source name")
            text_total += len(name)
        require(len(set(names)) == count and text_total <= 131072, "Duplicate/excessive source names")
        if "original_inline_style" in test:
            same(test["original_inline_style"], {"utf8_bytes": 107, "sha256": STYLE_SHA,
                 "preserve_container_target_markup": True}, "inline style")
    plan = [parent[path] for path in sorted(set(domain + COMMON))]
    return selected, domain, plan


class SourceHTML(HTMLParser):
    """Pinned-source metadata only, NOT Mg's HTML parser or a DOM-ID oracle."""
    def __init__(self, path):
        super().__init__(convert_charrefs=False)
        self.path = path
        self.scripts, self.styles, self.meta, self.links, self.divs = [], [], [], [], []
        self.current = None
        self.counts = {}
        self.doctypes = 0

    def handle_decl(self, declaration):
        same(declaration, "DOCTYPE html", "doctype")
        self.doctypes += 1

    def handle_startendtag(self, _tag, _attrs):
        raise Invalid("Unreviewed self-closing source tag")

    def handle_starttag(self, tag, attributes):
        require(tag in {"html", "head", "body", "title", "meta", "link", "script", "style", "div"},
                "Unreviewed source/resource tag")
        require(len(attributes) == len({name for name, _ in attributes})
                and not any(name.startswith("on") for name, _ in attributes), "Duplicate/handler attribute")
        self.counts[tag] = self.counts.get(tag, 0) + 1
        if tag == "script":
            index = len(self.scripts)
            require(index < 4 and self.current is None, "Extra/nested source")
            expected = [("src", ROUTES[index])] if index < 3 else []
            same(attributes, expected, "original script attributes")
            same(self.get_starttag_text(), f'<script src="{ROUTES[index]}">' if index < 3 else "<script>",
                 "original script opening tag")
            self.current = (tag, [])
        elif tag == "style":
            require(self.current is None and not attributes, "Unreviewed style source")
            self.current = (tag, [])
        elif tag == "meta":
            self.meta.append(attributes)
        elif tag == "link":
            self.links.append(attributes)
        elif tag == "div":
            self.divs.append(attributes)
        else:
            require(not attributes, "Unreviewed element attributes")

    def handle_data(self, text):
        if self.current is not None:
            self.current[1].append(text)

    def handle_entityref(self, name):
        self.handle_data(f"&{name};")

    def handle_charref(self, name):
        self.handle_data(f"&#{name};")

    def handle_endtag(self, tag):
        if tag in ("script", "style"):
            require(self.current is not None and self.current[0] == tag, "Unmatched source close")
            text = "".join(self.current[1])
            (self.scripts if tag == "script" else self.styles).append(text)
            self.current = None


def literal_calls(text, property_name, kind):
    """Read this frozen literal-only call subset as data, never evaluate JS."""
    decoder = json.JSONDecoder(parse_constant=_not_number, parse_float=_not_number)
    index = 0
    names = []

    def space():
        nonlocal index
        while index < len(text):
            if text[index].isspace():
                index += 1
            elif text.startswith("//", index):
                newline = text.find("\n", index + 2)
                index = len(text) if newline < 0 else newline + 1
            elif text.startswith("/*", index):
                close = text.find("*/", index + 2)
                require(close >= 0, "Unterminated literal-source comment")
                index = close + 2
            else:
                break

    def token(value):
        nonlocal index
        space()
        require(text.startswith(value, index), "Unreviewed literal-call syntax")
        index += len(value)

    def data():
        nonlocal index
        space()
        try:
            value, index = decoder.raw_decode(text, index)
        except (json.JSONDecodeError, RecursionError) as error:
            raise Invalid("Invalid literal JSON argument") from error
        return value

    def ascii_string(value):
        require(type(value) is str and value.isascii()
                and all(32 <= ord(c) < 127 for c in value), "Non-simple ASCII string argument")
        return value

    space()
    while index < len(text):
        token(f"test_{kind}_value")
        token("(")
        same(ascii_string(data()), property_name, "literal property")
        token(",")
        value = ascii_string(data())
        space()
        if index < len(text) and text[index] == ",":
            index += 1
            alternative = data()
            if type(alternative) is list:
                require(alternative, "Empty serialization alternatives")
                for item in alternative:
                    ascii_string(item)
            else:
                ascii_string(alternative)
        token(")")
        token(";")
        suffix = " should set the property value" if kind == "valid" else " should not set the property value"
        names.append("e.style['" + property_name + "'] = " + json.dumps(value) + suffix)
        require(len(names) <= 256, "Literal call count exceeds observer bound")
        space()
    return names


def validate_source(path, raw, test):
    text = raw.decode("utf-8")
    require("\r" not in text and "\0" not in text, "Unreviewed source encoding/framing")
    parser = SourceHTML(path)
    parser.feed(text)
    parser.close()
    require(parser.current is None and parser.doctypes == 1
            and all(parser.counts.get(tag) == 1 for tag in ("html", "head", "body", "title"))
            and len(parser.scripts) == 4, "Original HTML/source shape differs")
    same(parser.scripts[:3], ["", "", ""], "external script text")
    require(len(parser.meta) == 2 and parser.meta[0] == [("charset", "utf-8")]
            and len(parser.meta[1]) == 2 and parser.meta[1][0] == ("name", "assert")
            and parser.meta[1][1][0] == "content" and parser.meta[1][1][1], "Original inert meta differs")
    property_name = Path(path).name.removesuffix("-invalid.html").removesuffix("-valid.html")
    same(parser.links, [[("rel", "author"), ("title", "Eric Willigers"),
                        ("href", "mailto:ericwilligers@chromium.org")],
                       [("rel", "help"), ("href", "https://drafts.csswg.org/css-flexbox/#propdef-" + property_name)]],
         "original inert links")
    if property_name == "flex" and path.endswith("/flex-valid.html"):
        require(len(parser.styles) == 1, "Original inline style missing")
        same(parser.divs, [[("id", "container")], [("id", "target")]], "original container/target")
        same({"utf8_bytes": len(parser.styles[0].encode()), "sha256": sha256(parser.styles[0].encode()),
              "preserve_container_target_markup": True}, test["original_inline_style"], "original inline style")
    else:
        same(parser.styles, [], "unexpected inline style")
        same(parser.divs, [], "unexpected target markup")
    inline = parser.scripts[3]
    same({"utf8_bytes": len(inline.encode()), "sha256": sha256(inline.encode()),
          "leading_lf_count": len(inline) - len(inline.lstrip("\n")),
          "trailing_lf_count": len(inline) - len(inline.rstrip("\n"))}, test["inline_text"], "inline text/framing")
    names = literal_calls(inline, property_name, test["source_declared_call_kind"])
    same(names, test["source_expected_raw_test_names"], "literal declared names/order")
    same(len(names), test["source_declared_call_count"], "literal declared count")


def admit_plan(rows, reader):
    require(len(rows) + reader.count <= MAX_FILES, "Planned input file budget")
    require(reader.total + sum(row["bytes"] for row in rows) <= MAX_BYTES,
            "Planned input bytes exceed budget before body read")


def check(root):
    with Reader(root) as reader:
        raw = reader.read(MANIFEST, MAX_MANIFEST, expected_sha=MANIFEST_SHA, metadata=True)
        document = strict_json(raw)
        parent_raw = reader.read(PARENT, MAX_METADATA, expected_sha=PARENT_SHA, metadata=True)
        parent = parent_index(strict_json(parent_raw))
        selected, domain, plan = validate_manifest(document, parent)
        same(reader.domain(), domain, "actual full source directory")
        admit_plan(plan, reader)
        bodies = {row["path"]: reader.read(f"{UPSTREAM}/{row['path']}", MAX_BYTES,
                  expected_size=row["bytes"], expected_sha=row["sha256"]) for row in plan}
        for path, test in zip(selected, document["tests"]):
            validate_source(path, bodies[path], test)
        return {"status": "SOURCE_VALIDATED", "manifest_sha256": sha256(raw),
                "upstream_revision": REVISION, "input_files": 20, "input_bytes": 222805,
                "domain_html_files": 27, "selected_files": 16,
                "source_valid_calls": 49, "source_invalid_calls": 30,
                "source_declared_calls": 79, "runtime_executed": False,
                "mg_node_ids_assigned": False, "total_read_bytes": reader.total}


def main(argv=None):
    arguments = sys.argv[1:] if argv is None else argv
    try:
        require(not arguments, "This fixed source checker takes no arguments")
        result = check(Path(__file__).absolute().parents[3])
    except (Invalid, OSError, UnicodeError, RecursionError) as error:
        print(json.dumps({"status": "ERROR", "reason": str(error)[:512],
                          "runtime_executed": False}, sort_keys=True))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
