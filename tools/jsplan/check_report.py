#!/usr/bin/env python3
"""Require the reviewed, exact P0/P1 result inventory, including known gaps.

Do not regenerate expectations automatically in CI. A changed outcome, even an
improvement, requires inspection and an intentional baseline update.
"""

import argparse
import json
from pathlib import Path

import runner


def validate(report, expected):
    errors = []
    if report.get("complete") is not True or report.get("selected_lanes"):
        errors.append("Report is incomplete or lane-filtered")
    digest = runner.sha256(runner.MANIFEST.read_bytes())
    if report.get("manifest_sha256") != digest or expected.get("manifest_sha256") != digest:
        errors.append("Input manifest does not match report and reviewed expectations")
    if report.get("denominator") != expected.get("denominator"):
        errors.append("Test262 denominator changed")
    required = {}
    for engine, groups in expected["outcomes"].items():
        for classification, names in groups.items():
            if classification not in runner.CLASSES:
                errors.append(f"Invalid expected classification: {classification}")
            for name in names:
                key = (engine, name)
                if key in required:
                    errors.append(f"Duplicate expectation: {key}")
                required[key] = classification
    observed = {}
    for row in report.get("results", []):
        key = (row.get("engine"), row.get("id"))
        if key in observed:
            errors.append(f"Duplicate result: {key}")
        observed[key] = row.get("classification")
    if report.get("expected_result_count") != len(required):
        errors.append("Declared result count differs from full reviewed profile")
    for key in sorted(set(required) | set(observed)):
        if observed.get(key) != required.get(key) or key not in required or key not in observed:
            errors.append(f"{key[0]} {key[1]}: expected {required.get(key, 'absent')}, got {observed.get(key, 'missing')}")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--expectations", type=Path, default=Path(__file__).with_name("expectations.json"))
    args = parser.parse_args()
    try:
        report = json.loads(args.report.read_text())
        expected = json.loads(args.expectations.read_text())
        errors = validate(report, expected)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f"Invalid report or expectations: {error}\n")
    if errors:
        parser.exit(1, "JSPLAN baseline mismatch:\n" + "\n".join(errors) + "\n")
    print(f"JSPLAN reviewed baseline matched: {len(report['results'])} outcomes, including declared unsupported/failure cases")


if __name__ == "__main__":
    main()
