#!/usr/bin/env python3
"""Run the pinned static WPT pilot through the real Rust Browser embedding.

No upstream source rewrites, assertion shims, web access, or script execution.
This is not wptrunner/WebDriver or a full WPT conformance implementation.
"""
import argparse
from collections import Counter
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
from html.parser import HTMLParser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import threading
import time
from urllib.parse import unquote, urljoin, urlsplit

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
STATUSES = ("PASS", "FAIL", "UNSUPPORTED", "ERROR", "TIMEOUT", "CRASH")
WIDTH, HEIGHT = 800, 600
RGB_BYTES = WIDTH * HEIGHT * 3
SCOPE = "static-reftest-pilot"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def safe_path(path):
    if (not isinstance(path, str) or not path or path.startswith("/")
            or "\\" in path or "%" in path or "?" in path or "#" in path
            or any(part in ("", ".", "..") for part in path.split("/"))):
        raise ValueError(f"Noncanonical corpus path: {path!r}")
    return path


def resource_path(base, href):
    url = urlsplit(urljoin("http://wpt.invalid/" + base, href))
    if url.scheme != "http" or url.netloc != "wpt.invalid" or url.query or url.fragment:
        raise ValueError(f"Unsupported nonstatic/nonlocal resource: {href!r}")
    return safe_path(url.path.lstrip("/"))


class Markup(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.references = []
        self.resources = []
        self.reasons = []
        self.feed(text)

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        rel = (attrs.get("rel") or "").lower().split()
        if tag in ("script", "iframe", "object", "embed", "base", "video", "audio"):
            self.reasons.append(f"{tag} setup is outside the static adapter")
        if any(name.startswith("on") for name in attrs):
            self.reasons.append("event-handler setup requires scripting")
        if {"reftest-wait", "test-wait"} & set((attrs.get("class") or "").split()):
            self.reasons.append("reftest-wait protocol is not implemented")
        if tag == "meta" and (attrs.get("name") or "").lower() in ("flags", "fuzzy", "variant", "timeout"):
            if attrs.get("content"):
                self.reasons.append(f"WPT {attrs['name']} metadata requires adapter support")
        if tag == "link":
            for label, relation in (("match", "=="), ("mismatch", "!=")):
                if label in rel:
                    self.references.append((relation, attrs.get("href") or ""))
            if "stylesheet" in rel:
                self.resources.append(attrs.get("href") or "")
        if tag == "img":
            self.resources.append(attrs.get("src") or "")
            if "srcset" in attrs:
                self.reasons.append("responsive image selection is not implemented")

    handle_startendtag = handle_starttag


def load_corpus(path=HERE / "corpus.json", upstream=HERE / "upstream"):
    manifest_bytes = path.read_bytes()
    corpus = json.loads(manifest_bytes)
    if corpus.get("schema_version") != 1 or not re.fullmatch(r"[0-9a-f]{40}", corpus["upstream"]["revision"]):
        raise ValueError("Invalid pinned corpus identity")
    files = {}
    for entry in corpus["files"]:
        name = safe_path(entry["path"])
        candidate = upstream / name
        if name in files or candidate.is_symlink() or not candidate.resolve().is_relative_to(upstream.resolve()):
            raise ValueError(f"Duplicate or unsafe input: {name}")
        data = candidate.read_bytes()
        if len(data) != entry["bytes"] or digest(data) != entry["sha256"]:
            raise ValueError(f"Pinned input changed: {name}")
        files[name] = data
    actual = {str(path.relative_to(upstream)) for path in upstream.rglob("*") if path.is_file()}
    if actual != set(files):
        raise ValueError("Upstream file inventory differs from the manifest")
    if len(files) > 100 or sum(map(len, files.values())) > 10 * 1024 * 1024:
        raise ValueError("Pilot corpus exceeds its reviewed size")
    names = [test["path"] for test in corpus["tests"]]
    if names != sorted(set(names)) or len(names) != corpus["selection"]["limit"]:
        raise ValueError("Changed, duplicate or unsorted test denominator")
    for test in corpus["tests"]:
        name = safe_path(test["path"])
        parsed = Markup(files[name].decode("utf-8"))
        edges = [{"relation": relation, "href": href, "path": resource_path(name, href)}
                 for relation, href in parsed.references]
        if not edges or edges != test["references"]:
            raise ValueError(f"Original upstream reference relations changed: {name}")
        for edge in edges:
            if edge["path"] not in files:
                raise ValueError(f"Missing pinned reference: {edge['path']}")
    return corpus, files, digest(manifest_bytes)


def prerequisites(name, files, visited=None):
    """Conservative, source-derived classification; never use outcomes to skip."""
    visited = set() if visited is None else visited
    if name in visited:
        return []
    visited.add(name)
    if name not in files:
        raise ValueError(f"Missing resource in corpus: {name}")
    suffix = PurePosixPath(name).suffix.lower()
    if suffix in (".xht", ".xhtml", ".xml"):
        return [f"{name}: XML/XHTML parser unavailable"]
    if suffix not in (".html", ".css", ".svg"):
        return []
    text = files[name].decode("utf-8")
    reasons, resources = [], []
    if suffix in (".html", ".svg"):
        markup = Markup(text)
        reasons.extend(markup.reasons)
        resources.extend(markup.resources)
    for pattern, reason in (
        (r"\bAhem\b|@font-face", "Ahem/webfont selection unavailable"),
        (r"@import", "CSS imports unavailable"),
        (r"\b(?:animation|transition)(?:-[\w-]+)?\s*:", "animation/timing protocol unavailable"),
        (r"\{\{", "WPT server substitution unavailable"),
    ):
        if re.search(pattern, text, re.I):
            reasons.append(reason)
    resources.extend(match.group(2).strip() for match in re.finditer(r"url\(\s*(['\"]?)(.*?)\1\s*\)", text, re.I))
    # These pinned SVG assets are standalone shapes. Reject external SVG loads.
    if suffix == ".svg" and re.search(r"(?:href|src)\s*=", text, re.I):
        reasons.append("SVG external references require a separate review")
    for resource in resources:
        try:
            child = resource_path(name, resource)
        except ValueError as error:
            reasons.append(str(error))
        else:
            reasons.extend(prerequisites(child, files, visited))
    return [f"{name}: {reason}" for reason in reasons]


def source_hash():
    """Content-address evaluated browser/build/runner inputs, not circular HEAD."""
    paths = {ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "build.rs", Path(__file__).resolve(),
             ROOT / "docs/cdp-protocol.json", ROOT / "assets/mgbrowser-32.png"}
    for directory in (ROOT / "src", ROOT / "crates"):
        paths.update(directory.rglob("*.rs"))
        paths.update(directory.rglob("Cargo.toml"))
    paths.add(ROOT / "examples/wpt_render.rs")
    hasher = hashlib.sha256()
    for path in sorted(paths):
        name = str(path.relative_to(ROOT)).encode()
        data = path.read_bytes()
        hasher.update(len(name).to_bytes(8, "big") + name)
        hasher.update(len(data).to_bytes(8, "big") + data)
    return hasher.hexdigest()


CONTROL_PREFIX = "_mg_wpt_control/"
CONTROL_FILES = {
    CONTROL_PREFIX + "green.html": b'<!doctype html><link rel="stylesheet" href="square.css"><div></div>',
    CONTROL_PREFIX + "red.html": b'<!doctype html><style>body{margin:0;background:white}div{width:20px;height:20px;background:red}</style><div></div>',
    CONTROL_PREFIX + "square.css": b'body{margin:0;background:white}div{width:20px;height:20px;background:#00ff00}',
}


@contextmanager
def serve(files):
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            path = urlsplit(self.path)
            name = unquote(path.path).lstrip("/")
            data = files.get(name) if not path.query else None
            status = 200 if data is not None else 404
            requests.append({"path": self.path, "status": status})
            self.send_response(status)
            self.send_header("Content-Length", str(len(data or b"")))
            mime = {".html": "text/html; charset=utf-8", ".xht": "application/xhtml+xml",
                    ".css": "text/css; charset=utf-8", ".svg": "image/svg+xml",
                    ".ttf": "font/ttf"}.get(PurePosixPath(name).suffix, "application/octet-stream")
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
    differences = sum(left[i:i + 3] != right[i:i + 3] for i in range(0, RGB_BYTES, 3))
    maximum = max(abs(a - b) for a, b in zip(left, right))
    return {"equal": differences == 0, "different_pixels": differences, "max_channel_difference": maximum}


def relations_pass(comparisons):
    matches = [item["equal"] for item in comparisons if item["relation"] == "=="]
    mismatches = [not item["equal"] for item in comparisons if item["relation"] == "!="]
    return bool(comparisons) and (not matches or any(matches)) and all(mismatches)


def page_failure(metadata):
    if (not isinstance(metadata, dict) or not isinstance(metadata.get("diagnostics"), list)
            or not all(isinstance(item, dict) and isinstance(item.get("kind"), str)
                       for item in metadata["diagnostics"])
            or not isinstance(metadata.get("resource_warnings"), list)
            or type(metadata.get("content_height")) is not int
            or type(metadata.get("non_uniform")) is not bool):
        return "ERROR", "Malformed renderer metadata"
    if (metadata.get("schema_version") != 1 or metadata.get("width") != WIDTH
            or metadata.get("height") != HEIGHT or metadata.get("scale") != 1
            or metadata.get("scripts") is not False or metadata.get("browser_replay_equal") is not True):
        return "ERROR", "Invalid viewport/execution/replay evidence"
    if metadata.get("resource_warnings"):
        return "FAIL", "Resource loading failed or was incomplete"
    if metadata.get("diagnostics_omitted"):
        return "FAIL", "Truncated diagnostics cannot establish complete rendering prerequisites"
    if any(item["kind"] in ("layout-fallback", "style-fallback") for item in metadata.get("diagnostics", [])):
        return "FAIL", "Styled layout fell back to readable flow"
    if metadata.get("non_uniform") is not True:
        return "FAIL", "Uniform output requires a reviewed reference precondition; cannot count as a pilot pass"
    if metadata.get("content_height", 0) > HEIGHT:
        return "UNSUPPORTED", "Overflow/scrollbar reftest protocol is outside this pilot"
    return None


def render_page(binary, font, base, name, output, timeout=45):
    prefix = output / digest(name.encode())[:20]
    began = time.monotonic()
    environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"}
    with prefix.with_suffix(".log").open("wb") as log:
        try:
            process = subprocess.run([str(binary), base + name, str(font), str(prefix)],
                                     cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT,
                                     timeout=timeout, check=False)
        except subprocess.TimeoutExpired:
            return {"status": "TIMEOUT", "reason": f"Page exceeded {timeout}s", "artifact": prefix.name}
        except OSError as error:
            return {"status": "ERROR", "reason": str(error), "artifact": prefix.name}
    if process.returncode:
        return {"status": "CRASH" if process.returncode < 0 else "ERROR",
                "reason": f"Renderer exit {process.returncode}", "artifact": prefix.name}
    try:
        metadata = json.loads(prefix.with_suffix(".json").read_bytes())
        pixels = prefix.with_suffix(".rgb").read_bytes()
        if not isinstance(metadata, dict):
            raise ValueError("Expected a metadata object")
        if len(pixels) != RGB_BYTES or metadata["url"] != base + name or metadata["final_url"] != base + name:
            raise ValueError("Output dimensions or response URL do not match the requested page")
        non_uniform = pixels != pixels[:3] * (WIDTH * HEIGHT)
        if non_uniform != metadata.get("non_uniform"):
            raise ValueError("Pixel content disagrees with nonuniform-frame evidence")
        failure = page_failure(metadata)
        return {"status": failure[0] if failure else "OK", "reason": failure[1] if failure else "",
                "artifact": prefix.name, "rgb_sha256": digest(pixels), "metadata": metadata,
                "elapsed_ms": round((time.monotonic() - began) * 1000, 2)}
    except (OSError, ValueError, KeyError, TypeError) as error:
        return {"status": "ERROR", "reason": f"Invalid render artifacts: {error}", "artifact": prefix.name}


def check_controls(binary, font, base, output):
    green = render_page(binary, font, base, CONTROL_PREFIX + "green.html", output)
    red = render_page(binary, font, base, CONTROL_PREFIX + "red.html", output)
    if green["status"] != "OK" or red["status"] != "OK":
        raise ValueError(f"Harness rendering controls failed: {green['status']}, {red['status']}")
    a = (output / (green["artifact"] + ".rgb")).read_bytes()
    b = (output / (red["artifact"] + ".rgb")).read_bytes()
    at = lambda pixels, x, y: pixels[(y * WIDTH + x) * 3:(y * WIDTH + x) * 3 + 3]
    if (at(a, 5, 5) != b"\x00\xff\x00" or at(b, 5, 5) != b"\xff\x00\x00"
            or at(a, 30, 30) != b"\xff\xff\xff" or at(b, 30, 30) != b"\xff\xff\xff"
            or compare_pixels(a, b)["equal"] or not compare_pixels(a, a)["equal"]):
        raise ValueError("Harness controls rejected blank, wrong CSS, or inverted equality")
    return {"passed": True, "green": green, "red": red, "note": "Authored controls, excluded from WPT counts"}


def score_from(report):
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
    args.output = args.output.resolve()
    args.font = args.font.resolve()
    # New, disposable output only: never read stale screenshots or overwrite evidence.
    args.output.mkdir(parents=True, exist_ok=False)
    report = {"schema_version": 1, "scope": SCOPE, "results": [], "complete": False,
              "started_utc": datetime.now(timezone.utc).isoformat(), "infrastructure_errors": []}
    report_path = args.output / "report.json"
    try:
        corpus, files, corpus_hash = load_corpus()
        source_before = source_hash()
        # Never label a stale executable with the current source fingerprint.
        # Rebuild on every run, including documentation-only commit verification.
        build_env = dict(os.environ, RUSTUP_TOOLCHAIN="1.91.1")
        toolchain = subprocess.check_output(["rustc", "--version"], cwd=ROOT, env=build_env, text=True).strip()
        if not toolchain.startswith("rustc 1.91.1 "):
            raise ValueError("WPT requires Rust 1.91.1")
        with (args.output / "build.log").open("wb") as build_log, (args.output / "build.jsonl").open("wb") as artifacts:
            subprocess.run(["cargo", "build", "--locked", "--example", "wpt_render", "--message-format=json"], cwd=ROOT,
                           env=build_env, stdout=artifacts, stderr=build_log, check=True, timeout=1200)
        args.binary = cargo_executable((args.output / "build.jsonl").read_text(encoding="utf-8"))
        report.update(upstream_revision=corpus["upstream"]["revision"], corpus_sha256=corpus_hash,
                      source_sha256=source_before, selection=corpus["selection"], rustc=toolchain,
                      binary_sha256=digest(args.binary.read_bytes()), font_sha256=digest(args.font.read_bytes()),
                      git_commit=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                      git_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                      viewport={"width": WIDTH, "height": HEIGHT, "scale": 1},
                      limitations=["Selected static reftests only, not full WPT or wptrunner",
                                   "No testharness.js, WebDriver, script setup, downloaded fonts, XML, fuzzy matching or scrollbar protocol",
                                   "Reftest equality is not an independent proof that all reference features are implemented"])
        write_json(report_path, report)
        with serve(files | CONTROL_FILES) as (base, requests):
            report["controls"] = check_controls(args.binary, args.font, base, args.output)
            cache = {}
            for test in corpus["tests"]:
                item = {"test": test["path"], "status": "UNSUPPORTED", "reasons": [], "comparisons": []}
                pages = [test["path"]] + [edge["path"] for edge in test["references"]]
                for page in pages:
                    item["reasons"].extend(prerequisites(page, files))
                item["reasons"] = sorted(set(item["reasons"]))
                if not item["reasons"]:
                    for page in pages:
                        if page not in cache:
                            cache[page] = render_page(args.binary, args.font, base, page, args.output)
                    item["pages"] = {page: cache[page] for page in pages}
                    failed = [cache[page] for page in pages if cache[page]["status"] != "OK"]
                    if failed:
                        priority = {status: index for index, status in enumerate(("FAIL", "UNSUPPORTED", "ERROR", "TIMEOUT", "CRASH"))}
                        item["status"] = max(failed, key=lambda value: priority[value["status"]])["status"]
                        item["reasons"] = [value["reason"] for value in failed]
                    else:
                        pixels = (args.output / (cache[test["path"]]["artifact"] + ".rgb")).read_bytes()
                        for edge in test["references"]:
                            reference = (args.output / (cache[edge["path"]]["artifact"] + ".rgb")).read_bytes()
                            item["comparisons"].append(edge | compare_pixels(pixels, reference))
                        item["status"] = "PASS" if relations_pass(item["comparisons"]) else "FAIL"
                report["results"].append(item)
                report["counts"] = score_from(report)["counts"]
                report["requests"] = list(requests)
                write_json(report_path, report)
                print(f"{item['status']:11} {item['test']}", flush=True)
            if any(request["status"] != 200 for request in requests):
                raise ValueError("Corpus requested an absent resource; see request log")
        if source_before != source_hash():
            raise ValueError("Evaluated source changed during the run")
        report["complete"] = True
        score = score_from(report)
        baseline_path = HERE / "expectations.json"
        if baseline_path.exists():
            check_ratchet(score, json.loads(baseline_path.read_bytes()))
        elif args.check_score:
            raise ValueError("Missing reviewed regression baseline")
        if any(score["counts"][status] for status in ("ERROR", "TIMEOUT", "CRASH")):
            raise ValueError("Infrastructure failure, timeout or crash: score cannot be accepted")
        if args.update_score:
            write_json(HERE / "score.json", score)
        if args.check_score and score != json.loads((HERE / "score.json").read_bytes()):
            raise ValueError("Website score is stale: rerun --update-score, review outcomes, and regenerate the site")
        print(json.dumps(score["counts"], sort_keys=True))
        return 0
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        report["infrastructure_errors"].append(str(error))
        print(f"WPT gate failed: {error}", file=sys.stderr)
        return 1
    finally:
        report["finished_utc"] = datetime.now(timezone.utc).isoformat()
        write_json(report_path, report)


if __name__ == "__main__":
    sys.exit(main())
