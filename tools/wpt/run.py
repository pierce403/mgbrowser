#!/usr/bin/env python3
"""Run the pinned static WPT pilot through the real Rust Browser embedding.

No upstream source rewrites, assertion shims, or arbitrary web access.
This is not wptrunner/WebDriver or a full WPT conformance implementation.
"""
import argparse
from collections import Counter
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import gzip
import io
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time
from urllib.parse import quote, unquote, urljoin, urlsplit

from import_corpus import (decode_source, parse_document, reference_edges,
                           resolved_edge, resource_edges)
from process_control import (LOG_LIMIT, METADATA_LIMIT, PNG_LIMIT, read_artifact,
                             run_bounded)

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
STATUSES = ("PASS", "FAIL", "UNSUPPORTED", "ERROR", "TIMEOUT", "CRASH")
WIDTH, HEIGHT = 800, 600
RGB_BYTES = WIDTH * HEIGHT * 3
SCOPE = "static-reftest-pilot"
MAX_FILES = 4096
MAX_INPUT_BYTES = 10 * 1024 * 1024
RUN_TIMEOUT = 25 * 60
BUILD_TIMEOUT = 1200
BUILD_OUTPUT_LIMIT = 32 * 1024 * 1024
OUTPUT_LIMIT = 512 * 1024 * 1024
REPORT_LIMIT = 64 * 1024 * 1024
REQUEST_LIMIT = 20_000
COMPRESSED_RGB_LIMIT = RGB_BYTES + 64 * 1024
PAGE_RESERVATION = LOG_LIMIT + METADATA_LIMIT + PNG_LIMIT + RGB_BYTES + COMPRESSED_RGB_LIMIT
CHECKPOINT_SECONDS = 5
PENDING = "PENDING"
FAILURE_PHASES = {
    ("FAIL", "admitted-execution"): {"navigation", "script"},
    ("TIMEOUT", "deadline"): {"navigation"},
    ("ERROR", "infrastructure"): {"configuration", "font", "artifact", "replay", "cleanup", "script-host"},
}
LEGACY_CORPUS_SHA256 = "a7c74dc8f01eebd8f14ad8851e4d8c3bad3d2ac97764abd2e370602e40b1bbdd"
LEGACY_EXPECTATIONS_SHA256 = "01fe95ec8e4b6c070ffed32f52afe90d98a854c3fc18e6e4458d1e65ab3fc82a"
LEGACY_PREFIX = "css-flexbox-first-24-v1"
FLEX_TREE = "9ad051e4d7244fb68fc346148daa8e369ebb4844"
REVIEWED_MISSING = {
    ("css/css-flexbox/percentage-descendant-of-anonymous-flex-item.html", "resources/flexbox.css", "css/css-flexbox/resources/flexbox.css"),
    ("css/css-flexbox/reference/flexbox-with-multi-column-property-ref.html", "resources/flexbox.css", "css/css-flexbox/reference/resources/flexbox.css"),
}


class UpstreamSourceFailure(ValueError):
    """A reviewed dependency is absent from the complete original pinned tree."""


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value, limit=REPORT_LIMIT):
    """Atomic, streaming bounded reports; never allocate an unbounded JSON string."""
    path = Path(path)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="wb", dir=path.parent, prefix="." + path.name + "-", delete=False) as stream:
            temporary = Path(stream.name)
            written = 0
            for text in json.JSONEncoder(indent=2, sort_keys=True, ensure_ascii=True).iterencode(value):
                data = text.encode("utf-8")
                if len(data) > limit - written:
                    raise ValueError(f"JSON report exceeds {limit} byte limit")
                stream.write(data)
                written += len(data)
            if written >= limit:
                raise ValueError(f"JSON report exceeds {limit} byte limit")
            stream.write(b"\n")
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def evidence_bytes(output):
    total = 0
    for path in output.iterdir():
        value = path.lstat()
        if not stat.S_ISREG(value.st_mode):
            raise ValueError(f"Unexpected nonregular evidence artifact: {path}")
        total += value.st_size
    return total


def check_budget(output, deadline=None, reserve=0):
    if deadline is not None and time.monotonic() >= deadline:
        raise ValueError("Whole WPT run exceeded its reviewed 25-minute deadline")
    # Reserve report replacement and the next producer's admitted artifacts.
    # This is runner admission/polling, not a hostile-writer filesystem quota.
    if evidence_bytes(output) + reserve > OUTPUT_LIMIT:
        raise ValueError("WPT evidence exceeds the reviewed 512 MiB admission")


def digest_file(path, deadline):
    before = Path(path).lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError("Fingerprint input is not a regular file: " + str(path))
    hasher = hashlib.sha256()
    with Path(path).open("rb") as stream:
        opened = os.fstat(stream.fileno())
        if (opened.st_dev, opened.st_ino, opened.st_size) != (before.st_dev, before.st_ino, before.st_size):
            raise ValueError("Fingerprint input changed before reading")
        remaining = before.st_size
        while remaining:
            if time.monotonic() >= deadline:
                raise ValueError("Whole WPT run deadline expired while fingerprinting")
            data = stream.read(min(64 * 1024, remaining))
            if not data:
                raise ValueError("Fingerprint input was truncated")
            hasher.update(data)
            remaining -= len(data)
        after = os.fstat(stream.fileno())
        if stream.read(1) or after.st_size != before.st_size or after.st_mtime_ns != opened.st_mtime_ns:
            raise ValueError("Fingerprint input changed while reading")
    return hasher.hexdigest()


def tool_output(command, label, output, environment, deadline):
    check_budget(output, deadline, reserve=LOG_LIMIT + REPORT_LIMIT)
    stdout = output / (label + ".out")
    result = run_bounded(command, cwd=ROOT, env=environment, log_path=output / (label + ".log"),
                         stdout_path=stdout, timeout=10, global_deadline=deadline)
    if result["status"] != "OK":
        raise ValueError(label + " failed: " + result["reason"])
    return read_artifact(stdout, limit=LOG_LIMIT).decode("utf-8")


def pending_results(corpus):
    if not isinstance(corpus, dict):
        raise ValueError("Invalid selected WPT manifest")
    tests = corpus.get("tests")
    if not isinstance(tests, list) or len(tests) > MAX_FILES:
        raise ValueError("Invalid or oversized selected WPT roster")
    if any(not isinstance(test, dict) or "path" not in test for test in tests):
        raise ValueError("Malformed selected WPT roster entry")
    names = [safe_path(test["path"]) for test in tests]
    if names != sorted(set(names)):
        raise ValueError("Selected WPT denominator must be unique and sorted")
    return [{"test": name, "status": PENDING, "reasons": [], "comparisons": []} for name in names]


def progress_counts(report):
    counts = dict.fromkeys((*STATUSES, PENDING), 0)
    for item in report["results"]:
        if item.get("status") not in counts:
            raise ValueError("Unknown outcome in partial report")
        counts[item["status"]] += 1
    counts["total"] = report["selected_total"]
    report["counts"] = counts
    report["completed_total"] = counts["total"] - counts[PENDING]


def checkpoint(path, report, output, deadline):
    progress_counts(report)
    check_budget(output, deadline, reserve=REPORT_LIMIT)
    write_json(path, report)


def finish_report(path, report):
    """Final bounded evidence remains honest even if detailed JSON exceeds cap."""
    try:
        progress_counts(report)
        check_budget(path.parent, reserve=REPORT_LIMIT)
        write_json(path, report)
        return True
    except (OSError, ValueError, TypeError) as error:
        report["infrastructure_errors"].append("Final report rejected: " + str(error)[:4096])
        minimal = {key: value for key, value in report.items() if key not in {"pages", "controls", "requests", "results"}}
        minimal["details_omitted_due_to_evidence_limit"] = True
        minimal["results"] = [{"test": item["test"], "status": item["status"]} for item in report["results"]]
        try:
            write_json(path, minimal)
        except (OSError, ValueError, TypeError) as final_error:
            print(f"Cannot retain bounded final WPT report: {final_error}", file=sys.stderr)
        return False


def safe_path(path):
    if (not isinstance(path, str) or not path or path.startswith("/")
            or "\\" in path or "%" in path or "?" in path or "#" in path
            or any(ord(character) < 32 for character in path)
            or any(part in ("", ".", "..") for part in path.split("/"))):
        raise ValueError(f"Noncanonical corpus path: {path!r}")
    return path


def resource_path(base, href):
    url = urlsplit(urljoin("http://wpt.invalid/" + base, href))
    if url.scheme != "http" or url.netloc != "wpt.invalid" or url.query or url.fragment:
        raise ValueError(f"Unsupported nonstatic/nonlocal resource: {href!r}")
    return safe_path(url.path.lstrip("/"))


def uses_inline_scripts(name, files):
    suffix = PurePosixPath(name).suffix.lower()
    if suffix not in {".html", ".htm"}:
        return False
    return any(tag == "script" and "src" not in attrs
               and (attrs.get("type") or "").strip().lower() in ("", "text/javascript", "application/javascript", "text/ecmascript", "application/ecmascript", "application/x-javascript")
               for tag, attrs in parse_document(name, files[name]).elements)


def load_corpus(path=HERE / "corpus.json", upstream=HERE / "upstream", global_deadline=None):
    manifest_bytes = read_artifact(path, limit=MAX_INPUT_BYTES)
    corpus = json.loads(manifest_bytes)
    if (not isinstance(corpus, dict) or type(corpus.get("schema_version")) is not int
            or corpus["schema_version"] != 1 or not isinstance(corpus.get("upstream"), dict)
            or not isinstance(corpus["upstream"].get("revision"), str)
            or not re.fullmatch(r"[0-9a-f]{40}", corpus["upstream"]["revision"])):
        raise ValueError("Invalid pinned corpus identity")
    files = {}
    entries = corpus["files"]
    if not isinstance(entries, list) or len(entries) > MAX_FILES:
        raise ValueError("Corpus exceeds reviewed 4096-file admission before body read")
    declared_bytes = 0
    for entry in entries:
        if not isinstance(entry, dict):
            raise ValueError("Malformed inventory entry before body read")
        length = entry.get("bytes")
        if type(length) is not int or length < 0 or length > MAX_INPUT_BYTES - declared_bytes:
            raise ValueError("Corpus exceeds reviewed 10 MiB admission before body read")
        if not isinstance(entry.get("sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
            raise ValueError("Invalid pinned input digest")
        declared_bytes += length
    for entry in corpus["files"]:
        if global_deadline is not None and time.monotonic() >= global_deadline:
            raise ValueError("Whole WPT run deadline expired while validating inputs")
        name = safe_path(entry["path"])
        candidate = upstream / name
        if name in files or candidate.is_symlink() or not candidate.resolve().is_relative_to(upstream.resolve()):
            raise ValueError(f"Duplicate or unsafe input: {name}")
        data = read_artifact(candidate, limit=entry["bytes"], exact=entry["bytes"])
        if len(data) != entry["bytes"] or digest(data) != entry["sha256"]:
            raise ValueError(f"Pinned input changed: {name}")
        if "git_blob_sha1" in entry or "git_mode" in entry:
            blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
            if entry.get("git_blob_sha1") != blob or entry.get("git_mode") not in ("100644", "100755"):
                raise ValueError(f"Pinned Git blob identity changed: {name}")
        files[name] = data
    actual = set()
    for candidate in upstream.rglob("*"):
        if global_deadline is not None and time.monotonic() >= global_deadline:
            raise ValueError("Whole WPT run deadline expired while validating inventory")
        mode = candidate.lstat().st_mode
        if stat.S_ISREG(mode):
            actual.add(str(candidate.relative_to(upstream)))
        elif not stat.S_ISDIR(mode):
            raise ValueError("Nonregular or symlink corpus inventory entry")
        if len(actual) > MAX_FILES:
            raise ValueError("Upstream file inventory exceeds its reviewed count")
    if actual != set(files):
        raise ValueError("Upstream file inventory differs from the manifest")
    names = [item["test"] for item in pending_results(corpus)]
    if (not isinstance(corpus.get("selection"), dict)
            or type(corpus["selection"].get("limit")) is not int
            or not names or len(names) != corpus["selection"]["limit"]):
        raise ValueError("Changed, duplicate or unsorted test denominator")
    for test in corpus["tests"]:
        name = safe_path(test["path"])
        edges = reference_edges(name, files[name])
        if not edges or edges != test["references"]:
            raise ValueError(f"Original upstream reference relations changed: {name}")
        for edge in edges:
            if edge["path"] is not None and edge["path"] not in files:
                raise ValueError(f"Missing pinned reference: {edge['path']}")
    if corpus["selection"].get("id") == "css-flexbox-recursive-reftests-v2":
        check_complete_selection(corpus, files, global_deadline)
    if path.resolve() == (HERE / "corpus.json").resolve():
        check_legacy_corpus(corpus)
    missing_dependencies(corpus, files)
    return corpus, files, digest(manifest_bytes)


def missing_dependencies(corpus, files):
    records = corpus.get("upstream_missing_dependencies", [])
    if not isinstance(records, list):
        raise ValueError("Malformed upstream missing-dependency evidence")
    paths = {}
    for record in records:
        if not isinstance(record, dict):
            raise ValueError("Malformed upstream missing-dependency evidence")
        identity = (record.get("source"), record.get("href"), record.get("path"))
        if (set(record) != {"source", "href", "path", "evidence"}
                or identity not in REVIEWED_MISSING or record["path"] in paths
                or record.get("evidence") != {"directory_tree_sha": FLEX_TREE,
                                             "inventory_prefix": "css/css-flexbox",
                                             "absent_from_complete_tree": True}
                or record["evidence"].get("absent_from_complete_tree") is not True
                or record["path"] in files or record["source"] not in files
                or not any(edge.get("path") == record["path"] and edge.get("href") == record["href"]
                           for edge in resource_edges(record["source"], files[record["source"]]))):
            raise ValueError("Unreviewed or inconsistent upstream missing dependency")
        paths[record["path"]] = record
    return paths


def git_tree_sha(entries):
    tree = {}
    for path, mode, blob in entries:
        node = tree
        parts = path.split("/")
        for part in parts[:-1]:
            node = node.setdefault(part, {})
            if not isinstance(node, dict):
                raise ValueError("Conflicting Git tree file/directory")
        if parts[-1] in node:
            raise ValueError("Duplicate/conflicting Git tree entry")
        node[parts[-1]] = (mode, blob)

    def encode(node):
        records = []
        for name, value in node.items():
            directory = isinstance(value, dict)
            mode, identity = ("40000", encode(value)) if directory else value
            records.append(((name + ("/" if directory else "")).encode("utf-8"), mode.encode() + b" " + name.encode("utf-8") + b"\0" + bytes.fromhex(identity)))
        data = b"".join(value for _, value in sorted(records))
        return hashlib.sha1(b"tree " + str(len(data)).encode() + b"\0" + data).hexdigest()

    return encode(tree)


def check_complete_selection(corpus, files, global_deadline=None):
    prefix = "css/css-flexbox/"
    flex = sorted(name for name in files if name.startswith(prefix))
    candidates = [name for name in flex if PurePosixPath(name).suffix.lower() in {".html", ".htm", ".xht", ".xhtml", ".xml", ".svg"}]
    selected = []
    for name in candidates:
        if global_deadline is not None and time.monotonic() >= global_deadline:
            raise ValueError("Whole WPT run deadline expired while reconstructing selection")
        if reference_edges(name, files[name]):
            selected.append(name)
    selection = corpus["selection"]
    path_hash = lambda names: digest(("\n".join(names) + "\n").encode("utf-8"))
    if (selection.get("directory_tree_sha") != FLEX_TREE
            or selection.get("recursive_upstream_files_count") != len(flex)
            or selection.get("recursive_upstream_bytes") != sum(len(files[name]) for name in flex)
            or selection.get("recursive_markup_candidates_count") != len(candidates)
            or selection.get("selected_tests_count") != len(selected)
            or selection.get("candidate_paths_sha256") != path_hash(candidates)
            or selection.get("selected_paths_sha256") != path_hash(selected)
            or selected != [test["path"] for test in corpus["tests"]]):
        raise ValueError("Complete recursive WPT candidate/selected roster differs from pinned source")
    entries = {entry["path"]: entry for entry in corpus["files"]}
    if any("git_blob_sha1" not in entry or "git_mode" not in entry for entry in entries.values()):
        raise ValueError("Expanded inventory requires every original Git blob identity/mode")
    if git_tree_sha([(name[len(prefix):], entries[name]["git_mode"], entries[name]["git_blob_sha1"]) for name in flex]) != FLEX_TREE:
        raise ValueError("Complete recursive input tree does not match pinned upstream Git tree")


def legacy_inputs():
    directory = HERE / "baselines"
    data = read_artifact(directory / (LEGACY_PREFIX + ".corpus.json"), limit=MAX_INPUT_BYTES)
    if digest(data) != LEGACY_CORPUS_SHA256:
        raise ValueError("Immutable original WPT corpus snapshot changed")
    corpus = json.loads(data)
    baseline_data = read_artifact(directory / (LEGACY_PREFIX + ".expectations.json"), limit=METADATA_LIMIT)
    if digest(baseline_data) != LEGACY_EXPECTATIONS_SHA256:
        raise ValueError("Immutable original WPT expectation snapshot changed")
    baseline = json.loads(baseline_data)
    names = [test["path"] for test in corpus["tests"]]
    if (len(names) != 24 or len(corpus["files"]) != 41
            or baseline.get("corpus_sha256") != LEGACY_CORPUS_SHA256
            or baseline.get("tests") != names
            or set(baseline.get("required_passes", [])) != set(names)
            or set(baseline.get("required_supported", [])) != set(names)):
        raise ValueError("Immutable original 24 passing/runnable requirements changed")
    return corpus, baseline


def check_legacy_corpus(corpus):
    legacy, _ = legacy_inputs()
    if corpus["upstream"]["revision"] != legacy["upstream"]["revision"]:
        raise ValueError("Expanded WPT corpus changed the original upstream revision")
    files = {entry["path"]: entry for entry in corpus["files"]}
    tests = {test["path"]: test for test in corpus["tests"]}
    for entry in legacy["files"]:
        current = files.get(entry["path"], {})
        if any(current.get(field) != entry[field] for field in ("path", "bytes", "sha256")):
            raise ValueError("Expanded corpus changed original pinned input: " + entry["path"])
    for test in legacy["tests"]:
        if tests.get(test["path"], {}).get("references") != test["references"]:
            raise ValueError("Expanded corpus removed/changed original WPT test: " + test["path"])


def check_legacy_expectations(baseline):
    _, legacy = legacy_inputs()
    for field in ("required_passes", "required_supported"):
        if not set(legacy[field]).issubset(baseline.get(field, [])):
            raise ValueError("Expanded expectations weakened original " + field)


def css_protocol_text(text):
    """Exclude CSS comments/strings when identifying execution requirements."""
    return re.sub(r"/\*.*?(?:\*/|$)|\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'", " ", text, flags=re.S)


def prerequisites(name, files, visited=None, missing=None):
    """Conservative, source-derived classification; never use outcomes to skip."""
    visited = set() if visited is None else visited
    if name in visited:
        return []
    visited.add(name)
    if name not in files:
        if name in (missing or {}):
            record = missing[name]
            raise UpstreamSourceFailure(f"Original pinned upstream dependency is absent: {record['source']} -> {record['href']} ({name})")
        raise ValueError(f"Missing resource in corpus: {name}")
    suffix = PurePosixPath(name).suffix.lower()
    if suffix not in (".html", ".htm", ".xht", ".xhtml", ".xml", ".css", ".svg"):
        return []
    text = decode_source(name, files[name])
    info = parse_document(name, files[name])
    reasons = list(info.errors)
    try:
        files[name].decode("utf-8")
    except UnicodeDecodeError:
        reasons.append("source encoding is outside the UTF-8 static adapter")
    for tag, attrs in info.elements:
        if tag == "script":
            kind = (attrs.get("type") or "").strip().lower()
            if suffix not in {".html", ".htm"} or "src" in attrs or kind == "module":
                reasons.append("script setup is outside the static adapter")
        elif tag in ("iframe", "object", "embed", "base", "video", "audio"):
            reasons.append(f"{tag} setup is outside the static adapter")
        if any(attribute.startswith("on") for attribute in attrs):
            reasons.append("event-handler setup requires scripting")
        if {"reftest-wait", "test-wait"} & set((attrs.get("class") or "").split()):
            reasons.append("reftest-wait protocol is not implemented")
        if tag == "meta":
            if (attrs.get("http-equiv") or "").strip().lower() == "refresh":
                reasons.append("meta-refresh navigation protocol is outside the static adapter")
            if (attrs.get("name") or "").lower() in ("flags", "fuzzy", "variant", "timeout") and attrs.get("content"):
                reasons.append(f"WPT {attrs['name']} metadata requires adapter support")
        if tag == "img" and "srcset" in attrs:
            reasons.append("responsive image selection is not implemented")
    for css in info.css_texts:
        active = css_protocol_text(css)
        if re.search(r"@import\b", active, re.I):
            reasons.append("CSS imports unavailable")
        if re.search(r"\b(?:animation|transition)(?:-[\w-]+)?\s*:", active, re.I):
            reasons.append("animation/timing protocol unavailable")
    if "{{" in text or ".sub." in PurePosixPath(name).name:
        reasons.append("WPT server substitution unavailable")
    # These pinned SVG assets are standalone shapes. Reject external SVG loads.
    if suffix == ".svg" and re.search(r"(?:href|src)\s*=", text, re.I):
        reasons.append("SVG external references require a separate review")
    for edge in resource_edges(name, files[name]):
        if edge["path"] is None:
            reasons.append("Unsupported nonstatic/nonlocal resource: " + edge["unsupported"]["reason"])
        else:
            reasons.extend(prerequisites(edge["path"], files, visited, missing))
    return [f"{name}: {reason}" for reason in reasons]


def test_prerequisites(test, files, missing=None):
    reasons = prerequisites(test["path"], files, missing=missing)
    if PurePosixPath(test["path"]).suffix.lower() not in {".html", ".htm", ".xht", ".xhtml"}:
        reasons.append(f"{test['path']}: top-level resource-document rendering is outside the adapter")
    for edge in test["references"]:
        if edge["path"] is None:
            reasons.append(f"{test['path']}: {edge['unsupported']['code']}: {edge['unsupported']['reason']}")
        else:
            page = edge["path"]
            if PurePosixPath(page).suffix.lower() not in {".html", ".htm", ".xht", ".xhtml"}:
                reasons.append(f"{page}: top-level resource-document rendering is outside the adapter")
            if reference_edges(page, files[page]):
                reasons.append(f"{page}: chained-reference protocol is not implemented")
            reasons.extend(prerequisites(page, files, missing=missing))
    return sorted(set(reasons))


def source_hash(global_deadline=None):
    """Content-address evaluated browser/build/runner inputs, not circular HEAD."""
    paths = {ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "build.rs", Path(__file__).resolve(),
             ROOT / "docs/cdp-protocol.json", ROOT / "assets/mgbrowser-32.png"}
    for directory in (ROOT / "src", ROOT / "crates"):
        paths.update(directory.rglob("*.rs"))
        paths.update(directory.rglob("Cargo.toml"))
    paths.add(ROOT / "examples/wpt_render.rs")
    paths.update(path for path in HERE.glob("*.py") if not path.name.startswith("test_"))
    hasher = hashlib.sha256()
    for path in sorted(paths):
        if global_deadline is not None and time.monotonic() >= global_deadline:
            raise ValueError("Whole WPT run deadline expired while fingerprinting source")
        name = str(path.relative_to(ROOT)).encode()
        data = read_artifact(path, limit=MAX_INPUT_BYTES)
        hasher.update(len(name).to_bytes(8, "big") + name)
        hasher.update(len(data).to_bytes(8, "big") + data)
    return hasher.hexdigest()


CONTROL_PREFIX = "_mg_wpt_control/"
CONTROL_FILES = {
    CONTROL_PREFIX + "green.html": b'<!doctype html><link rel="stylesheet" href="square.css"><div></div>',
    CONTROL_PREFIX + "red.html": b'<!doctype html><style>body{margin:0;background:white}div{width:20px;height:20px;background:red}</style><div></div>',
    CONTROL_PREFIX + "square.css": b'body{margin:0;background:white}div{width:20px;height:20px;background:#00ff00}',
}


MIME_TYPES = {
    ".html": "text/html; charset=utf-8", ".htm": "text/html; charset=utf-8", ".xht": "application/xhtml+xml",
    ".xhtml": "application/xhtml+xml", ".xml": "application/xml",
    ".css": "text/css; charset=utf-8", ".svg": "image/svg+xml",
    ".ttf": "font/ttf", ".otf": "font/otf", ".png": "image/png",
    ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif",
}


class RequestAudit(list):
    def __init__(self, limit=REQUEST_LIMIT):
        super().__init__()
        self.limit = limit
        self.exceeded = False
        self.lock = threading.Lock()

    def add(self, request):
        with self.lock:
            if len(self) >= self.limit:
                self.exceeded = True
                return False
            self.append(request)
            return True


@contextmanager
def serve(files):
    requests = RequestAudit()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            path = urlsplit(self.path)
            name = unquote(path.path).lstrip("/")
            data = files.get(name) if not path.query else None
            status = 200 if data is not None else 404
            # Request targets are separately bounded by BaseHTTPRequestHandler;
            # cap retained target text as well as the total number of entries.
            if len(self.path.encode("utf-8")) > 8192:
                requests.exceeded = True
                status, data = 414, None
            if not requests.add({"path": self.path[:8192], "status": status}):
                status, data = 503, None
            self.send_response(status)
            self.send_header("Content-Length", str(len(data or b"")))
            mime = MIME_TYPES.get(PurePosixPath(name).suffix.lower(), "application/octet-stream")
            self.send_header("Content-Type", mime)
            self.send_header("Connection", "close")
            self.end_headers()
            if data:
                self.wfile.write(data)

        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}/", requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


def compare_pixels(left, right):
    if len(left) != RGB_BYTES or len(right) != RGB_BYTES:
        raise ValueError("Missing/truncated/wrong-dimension RGB screenshot")
    if left == right:
        return {"equal": True, "different_pixels": 0, "max_channel_difference": 0}
    differences = sum(left[i:i + 3] != right[i:i + 3] for i in range(0, RGB_BYTES, 3))
    maximum = max(abs(a - b) for a, b in zip(left, right))
    return {"equal": differences == 0, "different_pixels": differences, "max_channel_difference": maximum}


def relations_pass(comparisons):
    matches = [item["equal"] for item in comparisons if item["relation"] == "=="]
    mismatches = [not item["equal"] for item in comparisons if item["relation"] == "!="]
    return bool(comparisons) and (not matches or any(matches)) and all(mismatches)


def page_failure(metadata, scripts=False):
    if (not isinstance(metadata, dict) or not isinstance(metadata.get("diagnostics"), list)
            or not all(isinstance(item, dict) and isinstance(item.get("kind"), str)
                       for item in metadata["diagnostics"])
            or not isinstance(metadata.get("resource_warnings"), list)
            or not all(isinstance(warning, str) for warning in metadata["resource_warnings"])
            or type(metadata.get("content_height")) is not int or metadata["content_height"] < 0
            or type(metadata.get("diagnostics_omitted")) is not int or metadata["diagnostics_omitted"] < 0
            or type(metadata.get("non_uniform")) is not bool):
        return "ERROR", "Malformed renderer metadata"
    if (any(type(metadata.get(field)) is not int for field in ("schema_version", "width", "height", "scale"))
            or metadata.get("schema_version") != 1 or metadata.get("width") != WIDTH
            or metadata.get("height") != HEIGHT or metadata.get("scale") != 1
            or metadata.get("scripts") is not scripts or metadata.get("browser_replay_equal") is not True):
        return "ERROR", "Invalid viewport/execution/replay evidence"
    if metadata.get("resource_warnings"):
        return "FAIL", "Resource loading failed or was incomplete"
    if metadata.get("diagnostics_omitted"):
        return "FAIL", "Truncated diagnostics cannot establish complete rendering prerequisites"
    # Equality cannot establish a pass when both pages omitted the same
    # feature. This conservative evidence precondition also rejects parser
    # diagnostics from intentionally invalid CSS; it is not a CSS verdict.
    # No kind/message exemptions: future diagnostic kinds require review.
    if metadata["diagnostics"]:
        return "FAIL", "Rendering diagnostics violate the diagnostic-free reftest precondition"
    if metadata.get("non_uniform") is not True:
        return "FAIL", "Uniform output requires a reviewed reference precondition; cannot count as a pilot pass"
    if metadata.get("content_height", 0) > HEIGHT:
        return "FAIL", "Admitted page exceeds the fixed viewport rendering precondition"
    return None


def read_pixels(output, artifact):
    prefix = output / artifact
    compressed = prefix.with_suffix(".rgb.gz")
    if compressed.exists():
        data = read_artifact(compressed, limit=COMPRESSED_RGB_LIMIT)
        with gzip.GzipFile(fileobj=io.BytesIO(data), mode="rb") as stream:
            pixels = stream.read(RGB_BYTES + 1)
        if len(pixels) != RGB_BYTES:
            raise ValueError("Missing/truncated/wrong-dimension compressed RGB screenshot")
        return pixels
    return read_artifact(prefix.with_suffix(".rgb"), limit=RGB_BYTES, exact=RGB_BYTES)


def compress_pixels(prefix, pixels):
    data = gzip.compress(pixels, compresslevel=6, mtime=0)
    if len(data) > COMPRESSED_RGB_LIMIT:
        raise ValueError("Compressed RGB exceeds its reviewed byte admission")
    with prefix.with_suffix(".rgb.gz").open("xb") as stream:
        stream.write(data)
    if read_pixels(prefix.parent, prefix.name) != pixels:
        raise ValueError("Lossless RGB compression round trip failed")
    prefix.with_suffix(".rgb").unlink()


def adapter_failure(value, expected_url):
    if (not isinstance(value, dict)
            or set(value) != {"schema_version", "outcome", "kind", "url", "phase", "reason"}
            or type(value.get("schema_version")) is not int or value["schema_version"] != 1
            or value.get("url") != expected_url
            or not isinstance(value.get("outcome"), str) or not isinstance(value.get("kind"), str)
            or not isinstance(value.get("reason"), str) or not value["reason"].strip()
            or len(value["reason"].encode("utf-8")) > 1024
            or value.get("phase") not in FAILURE_PHASES.get((value.get("outcome"), value.get("kind")), set())):
        raise ValueError("Malformed structured adapter failure")
    return value["outcome"], value["reason"]


def render_page(binary, font, base, name, output, timeout=45, scripts=False, global_deadline=None):
    prefix = output / digest(name.encode())[:20]
    began = time.monotonic()
    environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"}
    expected_url = base + quote(name, safe="/")
    command = [str(binary), expected_url, str(font), str(prefix)]
    if scripts:
        command.append("--enable-scripts")
    try:
        check_budget(output, global_deadline, reserve=PAGE_RESERVATION + REPORT_LIMIT)
        process = run_bounded(command, cwd=ROOT, env=environment, log_path=prefix.with_suffix(".log"),
                              timeout=timeout, global_deadline=global_deadline,
                              artifact_limits={prefix.with_suffix(".json"): METADATA_LIMIT,
                                               prefix.with_suffix(".rgb"): RGB_BYTES,
                                               prefix.with_suffix(".png"): PNG_LIMIT})
        if process["status"] != "OK":
            if (process.get("returncode") == 2 and process["status"] == "ERROR"
                    and process.get("completed") is True):
                metadata = json.loads(read_artifact(prefix.with_suffix(".json"), limit=METADATA_LIMIT))
                status, reason = adapter_failure(metadata, expected_url)
                return {"status": status, "reason": reason, "artifact": prefix.name,
                        "failure": metadata, "elapsed_ms": process["elapsed_ms"]}
            return {"status": process["status"], "reason": process["reason"], "artifact": prefix.name,
                    "elapsed_ms": process.get("elapsed_ms", round((time.monotonic() - began) * 1000, 2))}
        metadata = json.loads(read_artifact(prefix.with_suffix(".json"), limit=METADATA_LIMIT))
        pixels = read_artifact(prefix.with_suffix(".rgb"), limit=RGB_BYTES, exact=RGB_BYTES)
        png = read_artifact(prefix.with_suffix(".png"), limit=PNG_LIMIT)
        if (len(png) < 33 or png[:8] != b"\x89PNG\r\n\x1a\n" or png[12:16] != b"IHDR"
                or struct.unpack(">II", png[16:24]) != (WIDTH, HEIGHT)):
            raise ValueError("Missing/malformed/wrong-dimension PNG screenshot")
        if not isinstance(metadata, dict):
            raise ValueError("Expected a metadata object")
        if metadata["url"] != expected_url or metadata["final_url"] != expected_url:
            raise ValueError("Output dimensions or response URL do not match the requested page")
        non_uniform = pixels != pixels[:3] * (WIDTH * HEIGHT)
        if non_uniform != metadata.get("non_uniform"):
            raise ValueError("Pixel content disagrees with nonuniform-frame evidence")
        failure = page_failure(metadata, scripts=scripts)
        compress_pixels(prefix, pixels)
        check_budget(output, global_deadline, reserve=REPORT_LIMIT)
        return {"status": failure[0] if failure else "OK", "reason": failure[1] if failure else "",
                "artifact": prefix.name, "rgb_sha256": digest(pixels), "metadata": metadata,
                "elapsed_ms": round((time.monotonic() - began) * 1000, 2)}
    except (OSError, ValueError, KeyError, TypeError, EOFError) as error:
        return {"status": "ERROR", "reason": f"Invalid render artifacts: {error}", "artifact": prefix.name}


def check_controls(binary, font, base, output, global_deadline=None):
    green = render_page(binary, font, base, CONTROL_PREFIX + "green.html", output, global_deadline=global_deadline)
    red = render_page(binary, font, base, CONTROL_PREFIX + "red.html", output, global_deadline=global_deadline)
    if green["status"] != "OK" or red["status"] != "OK":
        raise ValueError(f"Harness rendering controls failed: {green['status']}, {red['status']}")
    a = read_pixels(output, green["artifact"])
    b = read_pixels(output, red["artifact"])
    at = lambda pixels, x, y: pixels[(y * WIDTH + x) * 3:(y * WIDTH + x) * 3 + 3]
    if (at(a, 5, 5) != b"\x00\xff\x00" or at(b, 5, 5) != b"\xff\x00\x00"
            or at(a, 30, 30) != b"\xff\xff\xff" or at(b, 30, 30) != b"\xff\xff\xff"
            or compare_pixels(a, b)["equal"] or not compare_pixels(a, a)["equal"]):
        raise ValueError("Harness controls rejected blank, wrong CSS, or inverted equality")
    return {"passed": True, "green": green, "red": red, "note": "Authored controls, excluded from WPT counts"}


def score_from(report):
    if any(item.get("status") not in STATUSES for item in report["results"]):
        raise ValueError("Pending/nonterminal outcomes cannot be accepted as a WPT score")
    counts = {status: 0 for status in STATUSES}
    counts.update(Counter(item["status"] for item in report["results"]))
    counts["total"] = len(report["results"])
    return {"schema_version": 1, "scope": SCOPE, "upstream_revision": report["upstream_revision"],
            "corpus_sha256": report["corpus_sha256"], "source_sha256": report["source_sha256"],
            "counts": counts, "results": [{"test": item["test"], "status": item["status"]} for item in report["results"]]}


def check_ratchet(score, baseline):
    if baseline["corpus_sha256"] != score["corpus_sha256"]:
        raise ValueError("Corpus changed: requires a separately reviewed baseline")
    if baseline["tests"] != [item["test"] for item in score["results"]]:
        raise ValueError("Test denominator changed")
    passed = {item["test"] for item in score["results"] if item["status"] == "PASS"}
    lost = set(baseline["required_passes"]) - passed
    if lost:
        raise ValueError("WPT regression: " + ", ".join(sorted(lost)))
    supported = {item["test"] for item in score["results"] if item["status"] != "UNSUPPORTED"}
    if set(baseline["required_supported"]) - supported:
        raise ValueError("Previously runnable tests became unsupported")


def cargo_executable(messages):
    """Use this build's artifact, including custom Cargo target directories."""
    candidates = []
    for line in messages.splitlines():
        event = json.loads(line)
        if not isinstance(event, dict):
            raise ValueError("Malformed Cargo artifact record")
        target = event.get("target", {})
        if not isinstance(target, dict) or (event.get("executable") is not None
                                           and not isinstance(event["executable"], str)):
            raise ValueError("Malformed Cargo target/executable record")
        if (event.get("reason") == "compiler-artifact" and target.get("name") == "wpt_render"
                and target.get("kind") == ["example"] and event.get("executable")):
            candidates.append(Path(event["executable"]).resolve())
    if len(candidates) != 1:
        raise ValueError("Cargo did not identify exactly one freshly built WPT executable")
    return candidates[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "tmp/wpt")
    parser.add_argument("--font", type=Path, default=Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"))
    actions = parser.add_mutually_exclusive_group()
    actions.add_argument("--update-score", action="store_true")
    actions.add_argument("--check-score", action="store_true")
    args = parser.parse_args()
    began = time.monotonic()
    deadline = began + RUN_TIMEOUT
    args.output = args.output.resolve()
    args.font = args.font.resolve()
    # New, disposable output only: never read stale screenshots or overwrite evidence.
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"schema_version": 2, "scope": SCOPE, "results": [], "pages": {}, "selected_total": 0,
              "complete": False, "started_utc": datetime.now(timezone.utc).isoformat(), "infrastructure_errors": [],
              "limits": {"whole_run_seconds": RUN_TIMEOUT, "build_seconds": BUILD_TIMEOUT,
                         "page_seconds": 45, "evidence_bytes": OUTPUT_LIMIT, "report_bytes": REPORT_LIMIT,
                         "http_requests": REQUEST_LIMIT, "log_bytes": LOG_LIMIT,
                         "metadata_bytes": METADATA_LIMIT, "png_bytes": PNG_LIMIT,
                         "rgb_bytes": RGB_BYTES, "build_output_bytes": BUILD_OUTPUT_LIMIT}}
    report_path = args.output / "report.json"
    exit_code = 1
    score = None
    try:
        # Retain the declared complete denominator even if input validation,
        # build, controls or a later budget fails before the first test runs.
        manifest = json.loads(read_artifact(HERE / "corpus.json", limit=MAX_INPUT_BYTES))
        report["results"] = pending_results(manifest)
        report["selected_total"] = len(report["results"])
        checkpoint(report_path, report, args.output, deadline)
        corpus, files, corpus_hash = load_corpus(global_deadline=deadline)
        missing = missing_dependencies(corpus, files)
        source_before = source_hash(deadline)
        # Never label a stale executable with the current source fingerprint.
        # Rebuild on every run, including documentation-only commit verification.
        build_env = dict(os.environ, RUSTUP_TOOLCHAIN="1.91.1")
        toolchain = tool_output(["rustc", "--version"], "toolchain", args.output, build_env, deadline).strip()
        if not toolchain.startswith("rustc 1.91.1 "):
            raise ValueError("WPT requires Rust 1.91.1")
        check_budget(args.output, deadline, reserve=BUILD_OUTPUT_LIMIT + REPORT_LIMIT)
        build = run_bounded(["cargo", "build", "--locked", "--example", "wpt_render", "--message-format=json"],
                            cwd=ROOT, env=build_env, log_path=args.output / "build.log",
                            stdout_path=args.output / "build.jsonl", timeout=BUILD_TIMEOUT,
                            global_deadline=deadline, max_output_bytes=BUILD_OUTPUT_LIMIT)
        if build["status"] != "OK":
            raise ValueError("WPT adapter build failed: " + build["status"] + ": " + build["reason"])
        args.binary = cargo_executable(read_artifact(args.output / "build.jsonl", limit=BUILD_OUTPUT_LIMIT).decode("utf-8"))
        report.update(upstream_revision=corpus["upstream"]["revision"], corpus_sha256=corpus_hash,
                      source_sha256=source_before, selection=corpus["selection"], rustc=toolchain,
                      binary_sha256=digest_file(args.binary, deadline), font_sha256=digest_file(args.font, deadline),
                      git_commit=tool_output(["git", "rev-parse", "HEAD"], "git-commit", args.output, build_env, deadline).strip(),
                      git_dirty=bool(tool_output(["git", "status", "--porcelain"], "git-status", args.output, build_env, deadline)),
                      viewport={"width": WIDTH, "height": HEIGHT, "scale": 1},
                      limitations=["Selected reftests only, not full WPT or wptrunner",
                                   "No testharness.js, WebDriver, external scripts, fuzzy matching or scrollbar protocol",
                                   "Reftest equality is not an independent proof that all reference features are implemented"])
        checkpoint(report_path, report, args.output, deadline)
        last_checkpoint = time.monotonic()
        with serve(files | CONTROL_FILES) as (base, requests):
            report["requests"] = requests
            report["controls"] = check_controls(args.binary, args.font, base, args.output, global_deadline=deadline)
            cache = {}
            report["pages"] = cache
            for index, test in enumerate(corpus["tests"]):
                check_budget(args.output, deadline, reserve=REPORT_LIMIT)
                item = {"test": test["path"], "status": "UNSUPPORTED", "reasons": [], "comparisons": []}
                try:
                    item["reasons"] = test_prerequisites(test, files, missing)
                    pages = [test["path"]] + [edge["path"] for edge in test["references"] if edge["path"] is not None]
                    if not item["reasons"]:
                        for page in pages:
                            if page not in cache:
                                cache[page] = render_page(args.binary, args.font, base, page, args.output,
                                                          scripts=uses_inline_scripts(page, files), global_deadline=deadline)
                            if requests.exceeded:
                                raise ValueError("HTTP request audit exceeded its reviewed entry/target bounds")
                        # Normalize page evidence once rather than duplicating
                        # all diagnostics in every test sharing a reference.
                        item["pages"] = {page: {"artifact": cache[page]["artifact"], "status": cache[page]["status"]} for page in pages}
                        failed = [cache[page] for page in pages if cache[page]["status"] != "OK"]
                        if failed:
                            priority = {status: order for order, status in enumerate(("FAIL", "UNSUPPORTED", "ERROR", "TIMEOUT", "CRASH"))}
                            item["status"] = max(failed, key=lambda value: priority[value["status"]])["status"]
                            item["reasons"] = [value["reason"] for value in failed]
                        else:
                            pixels = read_pixels(args.output, cache[test["path"]]["artifact"])
                            for edge in test["references"]:
                                reference = read_pixels(args.output, cache[edge["path"]]["artifact"])
                                item["comparisons"].append(edge | compare_pixels(pixels, reference))
                            item["status"] = "PASS" if relations_pass(item["comparisons"]) else "FAIL"
                except UpstreamSourceFailure as error:
                    item["status"], item["reasons"] = "FAIL", [str(error)]
                except (OSError, ValueError, KeyError, TypeError, LookupError) as error:
                    item["status"], item["reasons"] = "ERROR", [str(error)[:4096]]
                report["results"][index] = item
                if time.monotonic() - last_checkpoint >= CHECKPOINT_SECONDS:
                    checkpoint(report_path, report, args.output, deadline)
                    last_checkpoint = time.monotonic()
                print(f"{item['status']:11} {item['test']}", flush=True)
            if requests.exceeded:
                raise ValueError("HTTP request audit exceeded its reviewed entry/target bounds")
            if any(request["status"] != 200 for request in requests):
                raise ValueError("Corpus requested an absent resource; see request log")
        if source_before != source_hash(deadline):
            raise ValueError("Evaluated source changed during the run")
        if report["git_commit"] != tool_output(["git", "rev-parse", "HEAD"], "git-finish", args.output, build_env, deadline).strip():
            raise ValueError("Tested Git commit changed during the run")
        report["complete"] = True
        score = score_from(report)
        baseline_path = HERE / "expectations.json"
        if baseline_path.exists():
            baseline = json.loads(read_artifact(baseline_path, limit=MAX_INPUT_BYTES))
            check_legacy_expectations(baseline)
            check_ratchet(score, baseline)
        elif args.check_score:
            raise ValueError("Missing reviewed regression baseline")
        if any(score["counts"][status] for status in ("ERROR", "TIMEOUT", "CRASH")):
            raise ValueError("Infrastructure failure, timeout or crash: score cannot be accepted")
        if args.check_score and score != json.loads(read_artifact(HERE / "score.json", limit=REPORT_LIMIT)):
            raise ValueError("Website score is stale: rerun --update-score, review outcomes, and regenerate the site")
        print(json.dumps(score["counts"], sort_keys=True))
        exit_code = 0
    except (OSError, ValueError, KeyError, TypeError, LookupError, subprocess.SubprocessError) as error:
        report["infrastructure_errors"].append(str(error)[:4096])
        print(f"WPT gate failed: {str(error)[:4096]}", file=sys.stderr)
    except KeyboardInterrupt:
        report["infrastructure_errors"].append("WPT run interrupted before acceptance")
    finally:
        report["finished_utc"] = datetime.now(timezone.utc).isoformat()
        report["elapsed_seconds"] = round(time.monotonic() - began, 3)
        if not finish_report(report_path, report):
            exit_code = 1
    # Never publish an updated score unless its complete bounded evidence was
    # retained successfully. Failed/PENDING runs leave the tracked score alone.
    if exit_code == 0 and args.update_score:
        try:
            write_json(HERE / "score.json", score)
        except (OSError, ValueError, TypeError) as error:
            report["infrastructure_errors"].append("Cannot update score: " + str(error)[:4096])
            finish_report(report_path, report)
            return 1
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
