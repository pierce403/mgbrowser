"""Validate two accepted source-bound reports and stage only fixed public files.

Producer jobs must succeed. This is neither a result mapper nor a raw artifact
verifier, and it never starts a Browser, worker, network request or score update.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import time


ROOT = Path(__file__).absolute().parents[3]
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "tools/wpt"))
import run as static_runner
from process_control import read_artifact
from window import score as window_score

PUBLIC_LIMIT = 25 * 1024 * 1024
METADATA_LIMIT = 4 * 1024 * 1024
TOTAL_LIMIT = 128 * 1024 * 1024
STAGE_SECONDS = 120
STATIC = ("index.html", "CNAME", ".nojekyll", "install.sh",
          "assets/mgbrowser.svg", "assets/mgbrowser-32.png", "assets/mgbrowser-256.png")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"),
                       ensure_ascii=True, allow_nan=False) + "\n").encode("ascii")


def strict_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, "Duplicate JSON key")
            result[key] = value
        return result

    def floating(text):
        value = float(text)
        require(math.isfinite(value), "Nonfinite JSON number")
        return value

    def constant(_text):
        raise ValueError("Nonfinite JSON constant")

    return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs,
                      parse_float=floating, parse_constant=constant)


def validate(static_report, window_report, static_score, current_window_score,
             ratchet, commit, current_window_raw):
    """Pure acceptance gate; do not change verdicts, inventories or scores."""
    require(type(commit) is str and re.fullmatch(r"[0-9a-f]{40}", commit), "Invalid exact commit")
    for report in (static_report, window_report):
        require(report["git_commit"] == commit and report["git_dirty"] is False,
                "Both reports must describe the same clean exact checkout")
        require(report["complete"] is True and report["infrastructure_errors"] == [], "Incomplete owner evidence")
        require(report["execution"]["cleanup_confirmed"] is True
                and type(report["execution"]["remaining_producer_reserved_bytes"]) is int
                and report["execution"]["remaining_producer_reserved_bytes"] == 0, "Unresolved owner cleanup")
    require(type(static_report["schema_version"]) is int and static_report["schema_version"] == 2
            and static_report["scope"] == "static-reftest-pilot", "Wrong static report lane")
    require(all(type(static_report[key]) is int and static_report[key] == 1011
                for key in ("selected_total", "completed_total"))
            and type(static_report["results"]) is list and len(static_report["results"]) == 1011,
            "Static denominator changed")
    require(all(type(row["test"]) is str and row["test"]
                and not any(ord(char) < 32 for char in row["test"]) for row in static_report["results"])
            and len({row["test"] for row in static_report["results"]}) == 1011, "Duplicate/invalid static test identity")
    require(all(type(static_report[key]) is str and re.fullmatch(r"[0-9a-f]{64}", static_report[key])
                for key in ("source_sha256", "corpus_sha256")), "Invalid static source fingerprint")
    require(type(static_report["upstream_revision"]) is str
            and re.fullmatch(r"[0-9a-f]{40}", static_report["upstream_revision"]), "Invalid static upstream identity")
    expected_static = static_runner.score_from(static_report)
    require(expected_static == static_score, "Static report differs from tracked score")
    counts = static_report["counts"]
    require(type(counts) is dict and all(type(value) is int and value >= 0 for value in counts.values())
            and counts == dict(expected_static["counts"], PENDING=0), "Static displayed counts differ from actual results")
    require(all(counts[key] == 0 for key in ("ERROR", "TIMEOUT", "CRASH", "PENDING")),
            "Static infrastructure/timeout/crash rule changed")
    candidate = window_score.score_from(window_report)
    window_score.validate_score(current_window_score)
    window_score.check_ratchet(candidate, ratchet)
    require(candidate == current_window_score, "Window report differs from tracked score/source")
    require(type(current_window_raw) is bytes and len(current_window_raw) <= METADATA_LIMIT
            and strict_json(current_window_raw) == current_window_score, "Tracked Window raw/object binding differs")
    gate = window_report["score_gate"]
    require(gate["mode"] == "check" and gate["status"] == "PASSED" and gate["error"] is None
            and window_report["score_written"] is False, "Fresh Window check-score did not pass")
    require(gate["candidate_sha256"] == hashlib.sha256(encoded(candidate)).hexdigest(), "Window candidate binding differs")
    source_input = gate["current_score_input"]
    require(source_input["path"] == "tools/wpt/window/current-score.json"
            and type(source_input["bytes"]) is int and source_input["bytes"] == len(current_window_raw)
            and source_input["sha256"] == hashlib.sha256(current_window_raw).hexdigest(), "Tracked Window score bytes differ")


def before(deadline):
    require(math.isfinite(deadline) and time.monotonic() < deadline, "Bounded staging deadline exceeded")


def read(path, limit, deadline):
    before(deadline)
    require(not any(parent.is_symlink() for parent in (path, *path.parents)), "Staging symlink rejected")
    value = read_artifact(path, limit=limit)
    before(deadline)
    return value


def report_bytes(directory, deadline):
    before(deadline)
    require(directory.is_dir() and not directory.is_symlink(), "Missing report artifact")
    # The producer artifact contains ONE report, not the raw evidence tree.
    # Stop on the second entry instead of materializing an arbitrary inventory.
    with os.scandir(directory) as entries:
        first = next(entries, None)
        require(first is not None and first.name == "report.json"
                and next(entries, None) is None, "Report artifact contains other/missing files")
    return read(directory / "report.json", PUBLIC_LIMIT, deadline)


def stage(root, commit, *, deadline=None):
    """Fixed filesystem operation; root is injectable for owned pure fixtures."""
    deadline = time.monotonic() + STAGE_SECONDS if deadline is None else deadline
    before(deadline)
    static_raw = report_bytes(root / "tmp/static-report", deadline)
    window_raw = report_bytes(root / "tmp/window-report", deadline)
    static_score_raw = read(root / "tools/wpt/score.json", METADATA_LIMIT, deadline)
    current_window_raw = read(root / "tools/wpt/window/current-score.json", METADATA_LIMIT, deadline)
    ratchet_raw = read(root / "tools/wpt/window/ratchet.json", METADATA_LIMIT, deadline)
    validate(strict_json(static_raw), strict_json(window_raw), strict_json(static_score_raw),
             strict_json(current_window_raw), strict_json(ratchet_raw), commit, current_window_raw)
    before(deadline)
    output = root / "tmp/public"
    require(output.parent.is_dir() and not output.parent.is_symlink()
            and not output.exists() and not output.is_symlink(), "Public output must be fresh")
    bodies = [("wpt-results.json", static_raw), ("wpt-window-results.json", window_raw)]
    used = len(static_raw) + len(window_raw)
    require(used <= TOTAL_LIMIT, "Public reports exceed existing verifier bound")
    for name in STATIC:
        body = read(root / name, min(PUBLIC_LIMIT, TOTAL_LIMIT - used), deadline)
        used += len(body)
        bodies.append((name, body))
    before(deadline)
    output.mkdir()
    for name, body in bodies:
        before(deadline)
        target = output / name
        target.parent.mkdir(parents=True, exist_ok=True)
        with target.open("xb") as stream:
            stream.write(body)
        before(deadline)
    return {"status": "STAGED", "commit": commit, "files": len(bodies),
            "static_selected_total": 1011, "window_selected_total": 16,
            "scope": "Existing accepted reports only; no raw artifacts, new results or installed-release claim"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    deadline = time.monotonic() + STAGE_SECONDS
    result = stage(ROOT, args.commit, deadline=deadline)
    print(json.dumps(result), flush=True)
    before(deadline)


if __name__ == "__main__":
    main()
