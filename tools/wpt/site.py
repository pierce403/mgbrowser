#!/usr/bin/env python3
"""Render the public WPT score only from a validated deterministic summary."""

import argparse
from collections import Counter
import html
import json
from pathlib import Path
import re
import sys


STATES = ("PASS", "FAIL", "UNSUPPORTED", "ERROR", "TIMEOUT", "CRASH")
START = "<!-- wpt-score:start -->"
END = "<!-- wpt-score:end -->"


def validate_score(score):
    """Reject malformed, inconsistent or incomplete counts rather than hiding them."""
    if not isinstance(score, dict):
        raise ValueError("WPT score must be a JSON object")
    if type(score.get("schema_version")) is not int or score["schema_version"] != 1:
        raise ValueError("WPT score requires schema_version 1")
    if score.get("scope") != "static-reftest-pilot":
        raise ValueError("WPT score has an unknown scope")
    for field, length in (
        ("upstream_revision", 40),
        ("corpus_sha256", 64),
        ("source_sha256", 64),
    ):
        value = score.get(field)
        if not isinstance(value, str) or not re.fullmatch(rf"[0-9a-f]{{{length}}}", value):
            raise ValueError(f"WPT score has an invalid {field}")
    counts = score.get("counts")
    if not isinstance(counts, dict) or set(counts) != set(STATES) | {"total"}:
        raise ValueError("WPT score must contain every status count and total")
    if any(type(value) is not int or value < 0 for value in counts.values()):
        raise ValueError("WPT score counts must be nonnegative integers")
    if counts["total"] == 0 or sum(counts[state] for state in STATES) != counts["total"]:
        raise ValueError("WPT score status counts do not equal a positive total")
    results = score.get("results")
    if not isinstance(results, list) or len(results) != counts["total"]:
        raise ValueError("WPT result list length does not equal total")
    observed = Counter()
    seen = set()
    for result in results:
        if not isinstance(result, dict):
            raise ValueError("Each WPT result must be an object")
        test = result.get("test")
        if not isinstance(test, str) or not test or any(ord(char) < 32 for char in test):
            raise ValueError("Each WPT result needs a nonempty printable test ID")
        if test in seen:
            raise ValueError(f"Duplicate WPT test ID: {test}")
        seen.add(test)
        status = result.get("status")
        if status not in STATES:
            raise ValueError(f"Unknown WPT result status for {test}")
        observed[status] += 1
    if any(observed[state] != counts[state] for state in STATES):
        raise ValueError("WPT result statuses do not agree with summary counts")
    return score


def render(score):
    score = validate_score(score)
    counts = score["counts"]
    upstream = html.escape(score["upstream_revision"], quote=True)
    source = html.escape(score["source_sha256"], quote=True)
    corpus = html.escape(score["corpus_sha256"], quote=True)
    return (
        '\n<p class="wpt-score"><strong>'
        f'{counts["PASS"]} / {counts["total"]} selected tests passing'
        '</strong></p>\n'
        f'<p>Fail: {counts["FAIL"]} · Unsupported: {counts["UNSUPPORTED"]} · '
        f'Errors: {counts["ERROR"]} · Timeouts: {counts["TIMEOUT"]} · '
        f'Crashes: {counts["CRASH"]}.</p>\n'
        '<p class="muted">A pinned static-reftest pilot using the real Rust browser '
        'navigation, resource and rendering path. This is a selected subset, not '
        'the full Web Platform Tests suite or JavaScript testharness coverage. '
        'These are current-source measurements, not a score for the installed release.</p>\n'
        '<p>Pages reruns this selection and checks the recorded score before '
        'publishing a commit. Unsupported cases stay in the denominator.</p>\n'
        '<p><a href="https://github.com/pierce403/mgbrowser/blob/main/docs/WPT.md">'
        'Method and limitations ↗</a> · <a href="/wpt-results.json">'
        'Full results and tested commit ↗</a></p>\n'
        '<details><summary>Reproducibility fingerprints</summary>\n'
        '<p>Upstream WPT: <a href="https://github.com/web-platform-tests/wpt/commit/'
        f'{upstream}"><code class="wpt-fingerprint">{upstream}</code></a></p>\n'
        f'<p>Corpus SHA-256: <code class="wpt-fingerprint">{corpus}</code></p>\n'
        f'<p>Browser source SHA-256: <code class="wpt-fingerprint">{source}</code></p>\n'
        '</details>\n'
    )


def replace_region(document, generated):
    if document.count(START) != 1 or document.count(END) != 1:
        raise ValueError("Expected exactly one WPT score marker pair in index.html")
    start = document.index(START) + len(START)
    end = document.index(END)
    if start > end:
        raise ValueError("Reversed WPT score markers")
    return document[:start] + generated + document[end:]


def synchronize(score_path, html_path, check=False):
    score = json.loads(score_path.read_text(encoding="utf-8"))
    original = html_path.read_text(encoding="utf-8")
    updated = replace_region(original, render(score))
    if updated != original:
        if check:
            raise ValueError("Website WPT score is stale; run tmp/site and commit index.html")
        html_path.write_text(updated, encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        synchronize(Path("tools/wpt/score.json"), Path("index.html"), args.check)
    except (OSError, ValueError) as error:
        print(f"WPT website score: {error}", file=sys.stderr)
        return 1
    print("Website WPT score is current." if args.check else "Website WPT score synchronized.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
