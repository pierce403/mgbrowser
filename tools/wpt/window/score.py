"""Deterministic Window metadata, not another semantic mapper.

Call score_from only AFTER the existing owner validates all actual artifacts,
cleanup and source stability. This module does not authenticate arbitrary JSON,
read files, import the owner, launch processes, change verdicts or write a score.
The integrating owner retains its existing absolute deadlines and evidence cap.
"""
import re


SCOPE = "upstream-window-declared-value-parsing-v1"
SELECTION = "css-flexbox-window-declared-value-parsing-v1"
UPSTREAM = "84daed4ae966d9624bed22f43587c79283ad1fea"
MANIFEST = "5223099747ed3b1669f311faa5007abb2c037bebb90aa7411b67c6470b4518d0"
CONTRACT = "PinnedWindowHarnessOutputDisabledV1"
FILES = ("PASS", "FAIL", "TIMEOUT", "PRECONDITION_FAILED", "UNSUPPORTED", "ERROR", "CRASH", "PENDING")
SUBTESTS = ("pass", "fail", "timeout", "not_run", "precondition_failed")
ROSTER = tuple(("css/css-flexbox/parsing/" + name + ".html", count) for name, count in (
    ("flex-basis-invalid", 7), ("flex-basis-valid", 8),
    ("flex-direction-invalid", 2), ("flex-direction-valid", 4),
    ("flex-flow-invalid", 2), ("flex-flow-valid", 7),
    ("flex-grow-invalid", 4), ("flex-grow-valid", 4), ("flex-invalid", 6),
    ("flex-shrink-invalid", 4), ("flex-shrink-valid", 4), ("flex-valid", 16),
    ("flex-wrap-invalid", 2), ("flex-wrap-valid", 3), ("order-invalid", 3), ("order-valid", 3)))
TESTS = tuple(path for path, _ in ROSTER)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def fields(value, expected, where):
    require(type(value) is dict and set(value) == set(expected), where + " fields differ")


def count(value, maximum=256):
    require(type(value) is int and 0 <= value <= maximum, "Invalid bounded count")
    return value


def summary(results):
    """Validate only the small score projection; raw owner checks stay upstream."""
    require(type(results) is list and len(results) == 16, "All sixteen file slots are required")
    files = dict.fromkeys(FILES, 0)
    final = dict.fromkeys(SUBTESTS, 0)
    complete = 0
    for row, (path, declared) in zip(results, ROSTER):
        fields(row, ("test", "status", "source_declared_calls", "observed_results",
                     "raw_harness_status", "final_counts"), "Score result")
        require(row["test"] == path and type(row["source_declared_calls"]) is int
                and row["source_declared_calls"] == declared, "Frozen order/source counts differ")
        status = row["status"]
        require(type(status) is str and status in FILES, "Unknown file verdict")
        require(status not in ("ERROR", "CRASH", "PENDING"), "Nonadoptable file verdict")
        files[status] += 1
        count(row["observed_results"])
        require(row["raw_harness_status"] is None or row["raw_harness_status"] in
                ("Ok", "Error", "Timeout", "PreconditionFailed"), "Unknown raw harness status")
        counts = row["final_counts"]
        if counts is None:
            # No Completion: never fabricate 79-minus-prefix NotRun or zero rows.
            continue
        fields(counts, ("total", *SUBTESTS, "final_only"), "Actual final inventory counts")
        for value in counts.values():
            count(value)
        require(counts["total"] == sum(counts[key] for key in SUBTESTS)
                and counts["final_only"] <= counts["total"], "Final count equation differs")
        complete += 1
        for key in SUBTESTS:
            final[key] += counts[key]
    files["total"] = 16
    final["total"] = sum(final.values())
    return {"files": files, "actual_final_subtests": final,
            "files_with_final_inventory": complete, "partial_files": 16 - complete,
            "source_prospective_calls": 79, "partial_unobserved_subtests": "unknown"}


def validate_score(score):
    fields(score, ("schema_version", "scope", "selection", "upstream_revision",
                   "manifest_sha256", "source_sha256", "observer_contract", "summary", "results"), "Window score")
    require(type(score["schema_version"]) is int and score["schema_version"] == 1, "Unknown score schema")
    require((score["scope"], score["selection"], score["upstream_revision"], score["manifest_sha256"],
             score["observer_contract"]) == (SCOPE, SELECTION, UPSTREAM, MANIFEST, CONTRACT), "Wrong Window source/scope")
    require(type(score["source_sha256"]) is str and re.fullmatch(r"[0-9a-f]{64}", score["source_sha256"]),
            "Invalid runtime/runner source fingerprint")
    expected = summary(score["results"])
    actual = score["summary"]
    fields(actual, expected, "Score summary")
    for key, names, maximum in (("files", (*FILES, "total"), 16),
                                 ("actual_final_subtests", (*SUBTESTS, "total"), 4096)):
        fields(actual[key], names, "Score summary." + key)
        for value in actual[key].values():
            count(value, maximum)
    for key in ("files_with_final_inventory", "partial_files"):
        count(actual[key], 16)
    require(type(actual["source_prospective_calls"]) is int
            and actual["source_prospective_calls"] == 79
            and actual["partial_unobserved_subtests"] == "unknown", "Source/partial summary differs")
    require(actual == expected, "Score summary differs from all actual file rows")
    return score


def score_from(report):
    """Small projection of an already accepted owner report, never raw callbacks."""
    require(type(report.get("schema_version")) is int and report["schema_version"] == 2,
            "Unknown owner report schema")
    require(report.get("status") == "COMPLETE" and report.get("complete") is True
            and report.get("baseline_review_eligible") is True, "Owner did not accept the complete run")
    require(report.get("infrastructure_errors") == [] and report.get("expectations_written") is False,
            "Owner failure or unexpected expectation mutation")
    require(all(type(report.get(key)) is int and report[key] == 16
                for key in ("selected_total", "attempted_total", "completed_total")), "Owner denominator differs")
    execution = report["execution"]
    require(execution["cleanup_confirmed"] is True and execution["cancelled"] is False
            and type(execution["remaining_producer_reserved_bytes"]) is int
            and execution["remaining_producer_reserved_bytes"] == 0 and execution["owner_failure"] is None,
            "Owner cleanup/reservation/failure is unresolved")
    build = report["build"]
    require(build["status"] == "COMPLETE" and all(build[key] is True for key in
            ("binary_verified", "completed_within_deadline", "process_cleanup_confirmed")), "Unaccepted build")
    source = report["source_fingerprint"]["sha256"]
    require(report["source_fingerprint_after"]["sha256"] == source
            and report["manifest_sha256"] == MANIFEST and report["selection"] == SELECTION
            and report["observer_contract"] == CONTRACT, "Evaluated input/source/mode differs")
    require(type(report["results"]) is list and len(report["results"]) == 16, "Missing file slots")
    rows = []
    for row in report["results"]:
        require(all(row.get(key) is True for key in ("attempted", "owner_accepted", "evidence_validated", "cleanup_confirmed"))
                and "owner_failure" not in row, "Unaccepted case or cleanup")
        decision = row["semantic_decision"]
        require(decision["verdict"] == row["status"], "Owner verdict projection differs")
        counts = decision["final_counts"]
        rows.append({"test": row["test"], "status": row["status"],
                     "source_declared_calls": row["source_declared_calls"],
                     "observed_results": decision["observed_results"], "raw_harness_status": row["raw_harness_status"],
                     "final_counts": None if counts is None else dict(counts)})
    result = {"schema_version": 1, "scope": SCOPE, "selection": SELECTION,
              "upstream_revision": UPSTREAM, "manifest_sha256": MANIFEST, "source_sha256": source,
              "observer_contract": CONTRACT, "summary": summary(rows), "results": rows}
    return validate_score(result)


def initial_ratchet():
    """Return manually reviewed initial data; never save or auto-strengthen it."""
    return {"schema_version": 1, "manifest_sha256": MANIFEST, "tests": list(TESTS),
            "required_passes": [], "required_runnable": list(TESTS)}


def check_ratchet(score, ratchet):
    validate_score(score)
    fields(ratchet, ("schema_version", "manifest_sha256", "tests", "required_passes", "required_runnable"), "Manual ratchet")
    require(type(ratchet["schema_version"]) is int and ratchet["schema_version"] == 1
            and ratchet["manifest_sha256"] == MANIFEST and ratchet["tests"] == list(TESTS), "Ratchet input/denominator differs")
    statuses = {row["test"]: row["status"] for row in score["results"]}
    for key in ("required_passes", "required_runnable"):
        values = ratchet[key]
        require(type(values) is list and all(type(value) is str and value in TESTS for value in values)
                and len(values) == len(set(values)) and values == sorted(values), "Malformed required set")
    require(all(statuses[path] == "PASS" for path in ratchet["required_passes"]), "Required Window pass regressed")
    require(all(statuses[path] != "UNSUPPORTED" for path in ratchet["required_runnable"]), "Previously runnable Window file became unsupported")
    # Clean owner-accepted TIMEOUT and PRECONDITION_FAILED remain non-PASS facts;
    # do not import the static lane's blanket timeout-as-infrastructure rule.
