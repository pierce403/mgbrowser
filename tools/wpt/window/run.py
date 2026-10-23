#!/usr/bin/env python3
"""Report-only owner for the fixed upstream Window parsing cohort.

This is separate from tools/wpt/run.py, both authored observer suites and every
tracked score/expectation. The Rust host owns its Browser, immutable fixture and
final typed mapper. Python verifies ownership/bindings; it does not reinterpret
callbacks, native messages, host signals or expected-negative controls.

Import is definition-only. Execution requires the separately reviewed host and
owner; no executable override, selection filter, retry or score-adoption mode.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import signal
import stat
import sys
import threading
import time


ROOT = Path(__file__).absolute().parents[3]
HERE = ROOT / "tools/wpt/window"
MANIFEST_PATH = "tools/wpt/window/corpus.json"
MANIFEST_SHA = "5223099747ed3b1669f311faa5007abb2c037bebb90aa7411b67c6470b4518d0"
CHECKER_PATH = "tools/wpt/window/check_manifest.py"
CONTROL_PATH = "tools/wpt/process_control.py"
PREFIX = "css/css-flexbox/parsing/"
CASE_PATHS = tuple(sorted(PREFIX + prop + suffix + ".html"
                          for prop in ("flex", "flex-basis", "flex-direction", "flex-flow",
                                       "flex-grow", "flex-shrink", "flex-wrap", "order")
                          for suffix in ("-valid", "-invalid")))
SCOPE = "Local real-Browser upstream Window parsing report; not static WPT, native UI or a published score"
CONTRACT = "PinnedWindowHarnessOutputDisabledV1"
STATUSES = ("PASS", "FAIL", "TIMEOUT", "PRECONDITION_FAILED", "UNSUPPORTED", "ERROR", "CRASH", "PENDING")
SUBTEST_STATUSES = ("Pass", "Fail", "Timeout", "NotRun", "PreconditionFailed")
RUN_SECONDS = 25 * 60
BUILD_SECONDS = 1200
RUN_FINAL_RESERVE = 3.0
CASE_NS = 25_000_000_000
ADMISSION_RESERVE_NS = 3_000_000_000
PROCESS_RESERVE_NS = 2_100_000_000
ACTIVE_NS = 2_000_000_000
LOG_LIMIT = 1024 * 1024
ARTIFACT_LIMIT = 4 * 1024 * 1024
AUDIT_LIMIT = 1024 * 1024
MANIFEST_LIMIT = 128 * 1024
METADATA_LIMIT = 4 * 1024 * 1024
INPUT_LIMIT = 10 * 1024 * 1024
MAX_INPUT_FILES = 4096
BUILD_OUTPUT_LIMIT = 32 * 1024 * 1024
BUILD_RESERVE = BUILD_OUTPUT_LIMIT + LOG_LIMIT
CASE_RESERVE = 10 * 1024 * 1024
REPORT_LIMIT = 64 * 1024 * 1024
EVIDENCE_LIMIT = 512 * 1024 * 1024
MAX_EVIDENCE_ENTRIES = 4096
REQUEST_LIMIT = 20_000
BINARY_LIMIT = 64 * 1024 * 1024
HASH = re.compile(r"[0-9a-f]{64}\Z")
HELPER_PINS = (
    ("Harness", "/resources/testharness.js", 198291, "d2399236c2a09c429804ff2299ad6629e17e2b53f17a74dd341e936adb11ae3e"),
    ("Vendor", "/resources/testharnessreport.js", 1231, "3777176fc736b0a78cf7617b9613b220f635daae7536ec7ebb99c61dbb525aae"),
    ("Parsing", "/css/support/parsing-testcommon.js", 7476, "ac482e1e1908e9b98f8f1c4b2c457ec77ae5ad2baa074205c4b0f7812f4024b5"),
)
HTTP_FAULTS = {"AttemptLimit", "Accept", "StreamMode", "Read", "Peek", "Write", "Flush", "Shutdown",
               "HeaderLimit", "HeaderIncomplete", "HeaderSyntax", "HeaderCount", "RequestLine", "Method",
               "Target", "Version", "Host", "Body", "DuplicateRoute", "LateAttempt", "SealedInFlight",
               "Deadline", "DrainUnconfirmed", "AuditBounds"}
OBSERVER_FAULTS = {"RejectedInit", "InvalidConfiguration", "MalformedCapture", "DuplicateResult", "DuplicateCompletion",
                   "ResultAfterCompletion", "CaptureOverflow", "StorageAdmission", "AuthorityUnavailable", "PrivacyLost",
                   "RealmTerminated", "NavigationProposed", "InvariantFailure"}
PARENT_FAILURES = {"Validation", "Projection", "ActiveCharge", "Transport", "BrokerPolicy", "Deadline", "Cleanup"}
FAILURE_CAUSES = {"ResourceLimit", "ObservationLimit", "ObservationLost", "PolicyDenied", "ResourceAdmission", "NativeExecution"}
INFRASTRUCTURE = {"Inventory", "Authority", "Configuration", "Projection", "ActiveCharge", "Wire", "Launch", "Artifact",
                  "Transport", "ObservationIntegrity", "MissingObservation", "DeliveryProof", "InitialOutcome",
                  "IncompleteEvidence", "ContradictoryFacts"}
U64 = (1 << 64) - 1


class Invalid(ValueError):
    """Owner evidence/admission failure, never a CSS assertion disposition."""


def require(condition, reason):
    if not condition:
        raise Invalid(reason)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def fields(value, names, where):
    require(type(value) is dict and set(value) == set(names),
            f"{where}: missing/unknown field or wrong object type")


def integer(value, maximum, where, minimum=0):
    require(type(value) is int and minimum <= value <= maximum, f"{where}: invalid integer")


def boolean(value, where):
    require(type(value) is bool, f"{where}: invalid boolean")


def digest(value, where):
    require(type(value) is str and HASH.fullmatch(value), f"{where}: invalid SHA-256")


def strict_json(raw, limit):
    require(type(raw) is bytes and len(raw) <= limit, "JSON byte admission failed")

    def pairs(items):
        value = {}
        for name, item in items:
            require(name not in value, "Duplicate JSON field")
            value[name] = item
        return value

    def number(text):
        require(len(text) <= 20, "JSON integer exceeds scalar admission")
        return int(text)

    def floating(text):
        value = float(text)
        require(math.isfinite(value), "Nonfinite JSON number")
        return value

    def constant(_text):
        raise Invalid("Nonfinite JSON constant")

    try:
        return json.loads(raw.decode("utf-8"), object_pairs_hook=pairs,
                          parse_int=number, parse_float=floating, parse_constant=constant)
    except (UnicodeError, json.JSONDecodeError, RecursionError) as error:
        raise Invalid("Malformed bounded UTF-8 JSON") from error


def safe_path(path):
    path = Path(os.path.abspath(path))
    require(not any(part.is_symlink() for part in (path, *path.parents)), "Symlink path rejected")
    return path


def stamp(value):
    return (value.st_dev, value.st_ino, value.st_mode, value.st_size,
            value.st_mtime_ns, value.st_ctime_ns)


def read_regular(path, limit, *, expected=None):
    path = safe_path(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and 0 <= before.st_size <= limit,
            "Regular file size/type admission failed before read: " + str(path))
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        opened = os.fstat(descriptor)
        require(stamp(opened) == stamp(before), "Input replaced before read")
        pieces, received = [], 0
        while received <= before.st_size:
            piece = os.read(descriptor, min(65536, before.st_size + 1 - received))
            if not piece:
                break
            pieces.append(piece)
            received += len(piece)
        require(received == before.st_size and stamp(os.fstat(descriptor)) == stamp(opened),
                "Input changed during bounded read")
        data = b"".join(pieces)
        require(expected is None or sha(data) == expected, "Pinned input SHA differs")
        return data, {"bytes": len(data), "sha256": sha(data), "identity": list(stamp(opened))}
    finally:
        os.close(descriptor)


def admit_font(path, source):
    """Admit its unique file/bytes BEFORE the first font body read.

    A conservative extra slot is intentional even if the caller names a file
    already in the source table. The generated executable alone is separate.
    """
    count, used = source["unique_input_files"], source["unique_input_bytes"]
    integer(count, MAX_INPUT_FILES, "source unique files")
    integer(used, INPUT_LIMIT, "source unique bytes")
    require(count + 1 <= MAX_INPUT_FILES, "Font exceeds shared unique input-file admission")
    remaining = INPUT_LIMIT - used
    path = safe_path(path)
    value = path.lstat()
    require(stat.S_ISREG(value.st_mode) and 0 < value.st_size <= remaining,
            "Font exceeds remaining shared input bytes BEFORE body read")
    return {"path": str(path), "bytes": value.st_size, "identity": list(stamp(value)),
            "source_unique_files": count, "source_unique_bytes": used,
            "remaining_bytes_before_font": remaining, "combined_unique_files": count + 1,
            "combined_unique_bytes": used + value.st_size}


def read_font(path, admission, expected=None):
    """Re-read the SAME admitted exact-length/hash/inode, not a renewed budget."""
    require(str(safe_path(path)) == admission["path"], "Font path changed after admission")
    data, pin = read_regular(path, admission["bytes"], expected=None if expected is None else expected["sha256"])
    require(pin["bytes"] == admission["bytes"] and pin["identity"] == admission["identity"],
            "Font length/identity changed after pre-read admission")
    require(expected is None or pin == expected, "Previously pinned font changed")
    return data, pin


def load_definitions(path, name):
    """Only the two fixed reviewed definition-only runtime helpers are loaded."""
    require(path in (CHECKER_PATH, CONTROL_PATH), "Unreviewed helper import")
    before, pin = read_regular(ROOT / path, METADATA_LIMIT)
    specification = importlib.util.spec_from_file_location(name, ROOT / path)
    require(specification is not None and specification.loader is not None, "Helper loader unavailable")
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    after, current = read_regular(ROOT / path, METADATA_LIMIT)
    require(before == after and pin == current, "Helper changed while loading definitions")
    return module


def fresh_directory(path):
    path = safe_path(path)
    require(path.parent == safe_path(ROOT / "tmp") and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", path.name),
            "Output must be one new direct child of ignored tmp")
    path.mkdir(mode=0o700, exist_ok=False)
    return path


def write_new(path, data, limit):
    require(type(data) is bytes and len(data) <= limit, "Output byte admission failed")
    with open(path, "xb") as destination:
        destination.write(data)


def encoded(value, limit):
    try:
        data = (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True,
                           allow_nan=False) + "\n").encode("ascii")
    except (ValueError, RecursionError) as error:
        raise Invalid("Cannot encode bounded owner evidence") from error
    require(len(data) <= limit, "Encoded evidence exceeds its byte admission")
    return data


def evidence_bytes(output):
    """Admission/polling, not an instantaneous hostile-writer disk quota."""
    total, count = 0, 0
    pending = [output]
    while pending:
        directory = pending.pop()
        with os.scandir(directory) as entries:
            for entry in entries:
                count += 1
                require(count <= MAX_EVIDENCE_ENTRIES, "Evidence entry admission exceeded")
                try:
                    value = entry.stat(follow_symlinks=False)
                except FileNotFoundError:
                    continue
                if stat.S_ISDIR(value.st_mode):
                    pending.append(Path(entry.path))
                else:
                    require(stat.S_ISREG(value.st_mode), "Evidence contains symlink/nonregular entry")
                    total += value.st_size
                    require(total <= EVIDENCE_LIMIT, "Shared evidence bytes exceeded")
    return total


class Budget:
    """One serial atomic reservation ledger; failed cleanup never releases it."""
    def __init__(self, output, deadline):
        self.output, self.deadline = output, deadline
        self.lock = threading.Lock()
        self.reserved = self.peak_reserved = 0
        self.cleanup_confirmed = True
        self.requests = 0
        self.cancel = threading.Event()
        self.interruption = None
        self.failure = None

    def stop(self, reason):
        with self.lock:
            if self.failure is None:
                self.failure = str(reason)[:1024]
            self.cancel.set()

    def is_set(self):
        if self.interruption is not None:
            self.stop(self.interruption)
        if self.cancel.is_set():
            return True
        try:
            require(time.monotonic() < self.deadline - RUN_FINAL_RESERVE, "Whole-run admission deadline")
            require(evidence_bytes(self.output) + self.reserved + REPORT_LIMIT <= EVIDENCE_LIMIT,
                    "Shared evidence/reservation admission exceeded")
        except (OSError, Invalid) as error:
            self.stop(error)
        return self.cancel.is_set()

    def reserve(self, amount):
        with self.lock:
            require(not self.cancel.is_set() and self.interruption is None and self.cleanup_confirmed,
                    "New admission after stop/unknown cleanup")
            require(evidence_bytes(self.output) + self.reserved + amount + REPORT_LIMIT <= EVIDENCE_LIMIT,
                    "Producer and atomic-report reservations exceed shared evidence")
            self.reserved += amount
            self.peak_reserved = max(self.peak_reserved, self.reserved)

    def release(self, amount, confirmed):
        with self.lock:
            if not self.cleanup_confirmed or confirmed is not True:
                self.cleanup_confirmed = False
                self.cancel.set()
                if self.failure is None:
                    self.failure = "Owned cleanup unconfirmed; producer reservation quarantined"
            else:
                require(self.reserved >= amount, "Reservation accounting underflow")
                self.reserved -= amount

    def admit_requests(self, count):
        integer(count, REQUEST_LIMIT, "Actual HTTP request count")
        with self.lock:
            require(self.requests + count <= REQUEST_LIMIT, "Shared20,000 request cap exceeded")
            self.requests += count

    def report(self, value):
        data = encoded(value, REPORT_LIMIT)
        with self.lock:
            require(evidence_bytes(self.output) + self.reserved + len(data) <= EVIDENCE_LIMIT,
                    "Atomic final report would spend held/quarantined evidence")
            target = self.output / "report.next"
            write_new(target, data, REPORT_LIMIT)
            os.replace(target, self.output / "report.json")


def pending_results():
    return [{"test": path, "status": "PENDING", "run_id": index + 1,
             "source_declared_calls": None, "final_subtests": None,
             "partial_unobserved_total": "unknown"} for index, path in enumerate(CASE_PATHS)]


def distributions(results):
    files = {name: 0 for name in STATUSES}
    final = {name: 0 for name in SUBTEST_STATUSES}
    complete_files = partial_files = 0
    for row in results:
        require(row["status"] in files, "Unknown owner file status")
        files[row["status"]] += 1
        counts = row["final_subtests"]
        if counts is None:
            partial_files += row["status"] != "PENDING"
        else:
            complete_files += 1
            for key in SUBTEST_STATUSES:
                final[key] += counts[key]
    return {"files": files, "actual_final_subtests": final,
            "files_with_final_inventory": complete_files, "attempted_partial_files": partial_files,
            "source_prospective_calls": 79, "partial_unobserved_subtests": "unknown"}


def private_env(root, *, build=False):
    root.mkdir(mode=0o700, exist_ok=False)
    names = {"HOME": "home", "XDG_CONFIG_HOME": "config", "XDG_CACHE_HOME": "cache",
             "XDG_DATA_HOME": "data", "XDG_STATE_HOME": "state", "XDG_RUNTIME_DIR": "runtime",
             "TMPDIR": "tmp"}
    environment = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "LANG": "C.UTF-8",
                   "LC_ALL": "C.UTF-8", "MGBROWSER_NO_AUTO_UPDATE": "1",
                   "PYTHONDONTWRITEBYTECODE": "1"}
    for variable, leaf in names.items():
        directory = root / leaf
        directory.mkdir(mode=0o700)
        environment[variable] = str(directory)
    if build:
        # Read existing toolchain/dependency caches, never a real browser profile
        # or caller RUSTFLAGS/features. --offline/--locked prevent dependency drift.
        original_home = os.environ.get("HOME")
        require(original_home is not None, "Configured Rust cache root is unavailable")
        environment.update(RUSTUP_TOOLCHAIN="1.91.1",
                           RUSTUP_HOME=os.environ.get("RUSTUP_HOME", original_home + "/.rustup"),
                           CARGO_HOME=os.environ.get("CARGO_HOME", original_home + "/.cargo"))
    return environment


def source_paths():
    fixed = {"Cargo.toml", "Cargo.lock", "build.rs", "docs/cdp-protocol.json",
             "assets/mgbrowser-32.png", "examples/wpt_window.rs", CONTROL_PATH,
             "docs/WPT_WINDOW_PARSING.md", "docs/WPT_WINDOW_RUNNER.md",
             "docs/WPT_OBSERVER.md", "docs/WPT_HARNESS_OUTPUT.md",
             "docs/BOA.md", "docs/PAGE_SESSIONS.md", "docs/DOCUMENT_LIFECYCLE.md"}
    for directory in (ROOT / "src", ROOT / "crates", ROOT / "examples/wpt_window", HERE):
        require(directory.is_dir() and not directory.is_symlink(), "Missing/aliased fingerprint source directory")
        pending = [directory]
        while pending:
            current = pending.pop()
            with os.scandir(current) as entries:
                for entry in entries:
                    if entry.name in ("target", "__pycache__", ".git"):
                        continue
                    value = entry.stat(follow_symlinks=False)
                    require(not stat.S_ISLNK(value.st_mode), "Fingerprint source symlink")
                    if stat.S_ISDIR(value.st_mode):
                        pending.append(Path(entry.path))
                    elif (entry.name.endswith((".rs", ".py", ".json"))
                          or entry.name in ("Cargo.toml", "Cargo.lock")):
                        require(stat.S_ISREG(value.st_mode), "Fingerprint nonregular source")
                        fixed.add(str(Path(entry.path).relative_to(ROOT)))
                    require(len(fixed) <= MAX_INPUT_FILES, "Fingerprint file-count admission")
    return sorted(fixed)


def fingerprint(checker, document, deadline):
    """All current runtime/helper/core build inputs, including nested new files.

    The fixed source checker verifies all31 original files plus two metadata
    files. This snapshot admits that SAME unique set and runtime sources under
    one4096/10MiB input budget before their reads. The font then shares its
    remaining input bytes/file slot; only the generated binary is separate.
    Repeated snapshots do not expand the unique admitted input inventory.
    """
    original = {row["path"]: row for row in document["full_source_domain_roster"]}
    original.update({row["path"]: row for row in document["files"]})
    names = set(source_paths()) | {"tools/wpt/corpus.json", MANIFEST_PATH}
    names.update("tools/wpt/upstream/" + path for path in original)
    hasher, rows = hashlib.sha256(), []
    with checker.Reader(ROOT) as reader:
        for name in sorted(names):
            require(time.monotonic() < deadline, "Run deadline during source fingerprint")
            expected = original.get(name.removeprefix("tools/wpt/upstream/")) if name.startswith("tools/wpt/upstream/") else None
            metadata = name in (MANIFEST_PATH, "tools/wpt/corpus.json")
            raw = reader.read(name, MANIFEST_LIMIT if name == MANIFEST_PATH else METADATA_LIMIT if metadata else INPUT_LIMIT,
                              expected_size=expected["bytes"] if expected else None,
                              expected_sha=expected["sha256"] if expected else None,
                              metadata=metadata)
            path_bytes = name.encode("utf-8")
            hasher.update(len(path_bytes).to_bytes(8, "big") + path_bytes)
            hasher.update(len(raw).to_bytes(8, "big") + raw)
            rows.append({"path": name, "bytes": len(raw), "sha256": sha(raw)})
        return {"sha256": hasher.hexdigest(), "files": rows,
                "unique_input_files": reader.count, "unique_input_bytes": reader.total,
                "metadata_bytes": reader.metadata}


def ordinary(result):
    return (type(result) is dict and result.get("status") == "OK"
            and result.get("returncode") == 0 and result.get("completed") is True
            and result.get("cleanup_succeeded") is True)


def tool(control, budget, command, name, environment, timeout=20.0, limit=LOG_LIMIT):
    require(not budget.is_set(), "Tool admission stopped")
    budget.reserve(limit + 4096)
    result = {"cleanup_succeeded": False}
    try:
        result = control.run_bounded(command, cwd=ROOT, env=environment,
                                     log_path=budget.output / (name + ".log"), timeout=timeout,
                                     global_deadline=budget.deadline - RUN_FINAL_RESERVE,
                                     max_output_bytes=limit, cancel_event=budget)
        write_new(budget.output / (name + "-process.json"), encoded(result, 4096), 4096)
        require(ordinary(result), "Owned tool failed: " + name)
        return control.read_artifact(budget.output / (name + ".log"), limit=limit).decode("utf-8")
    finally:
        budget.release(limit + 4096, result.get("cleanup_succeeded") is True)


def cargo_executable(raw):
    require(len(raw) <= BUILD_OUTPUT_LIMIT, "Cargo output admission")
    candidates = []
    for line in raw.splitlines():
        event = strict_json(line, BUILD_OUTPUT_LIMIT)
        require(type(event) is dict, "Malformed Cargo record")
        target = event.get("target", {})
        require(type(target) is dict, "Malformed Cargo target")
        if event.get("reason") == "compiler-artifact" and target.get("name") == "wpt_window" and target.get("kind") == ["example"]:
            require(event.get("features") == [], "Host built with unexpected features (including legacy)")
            executable = event.get("executable")
            if executable is not None:
                require(type(executable) is str, "Malformed Cargo executable")
                candidates.append(safe_path(executable))
    require(len(candidates) == 1, "Build did not identify one wpt_window executable")
    return candidates[0]


def build(control, budget, environment):
    budget.reserve(BUILD_RESERVE)
    result = {"cleanup_succeeded": False}
    try:
        result = control.run_bounded(["cargo", "build", "--locked", "--offline", "--release",
                                     "--example", "wpt_window", "--message-format=json"],
                                    cwd=ROOT, env=environment, log_path=budget.output / "build.log",
                                    stdout_path=budget.output / "build.jsonl", timeout=BUILD_SECONDS,
                                    global_deadline=budget.deadline - RUN_FINAL_RESERVE,
                                    max_output_bytes=BUILD_OUTPUT_LIMIT, cancel_event=budget)
        write_new(budget.output / "build-process.json", encoded(result, 4096), 4096)
        require(ordinary(result), "Normal fixed Window host build failed")
        return cargo_executable(control.read_artifact(budget.output / "build.jsonl", limit=BUILD_OUTPUT_LIMIT))
    finally:
        budget.release(BUILD_RESERVE, result.get("cleanup_succeeded") is True)


def verify_process(result, *, semantic_error=False):
    fields(result, ("status", "reason", "returncode", "elapsed_ms", "output_bytes", "completed", "cleanup_succeeded"), "process")
    boolean(result["completed"], "process.completed")
    boolean(result["cleanup_succeeded"], "process.cleanup_succeeded")
    integer(result["returncode"], 255, "process ordinary returncode")
    integer(result["output_bytes"], LOG_LIMIT, "process output_bytes")
    text(result["reason"], 4096, "process reason")
    require(type(result["elapsed_ms"]) in (int, float) and math.isfinite(result["elapsed_ms"])
            and 0 <= result["elapsed_ms"] <= 25_000, "process elapsed_ms")
    if semantic_error:
        require(result["status"] == "ERROR" and result["returncode"] == 1
                and result["completed"] is True and result["cleanup_succeeded"] is True,
                "SemanticERROR requires actual normal exit1/cleanup, not adoption success")
    else:
        require(ordinary(result), "Host did not finish normally with confirmed owned-group/direct-leader cleanup")


def validate_clock(clock, case_started_ns, case_deadline_ns, owner_now_ns):
    """Count ALL owner wall except the host's reviewed exact idle/cleanup totals.

    In particular Python setup, launch, process monitoring, transfer, artifact
    validation and post-host bookkeeping receive no unmeasured free allowance.
    Adding host.used_active_ns again would double count the same wall interval.
    """
    fields(clock, ("whole_deadline_monotonic_ns", "admission_deadline_monotonic_ns", "report_deadline_monotonic_ns",
                   "started_monotonic_ns", "cursor_monotonic_ns", "elapsed_ns", "used_active_ns",
                   "excluded_idle_ns", "excluded_cleanup_ns", "sums_valid", "admission_stopped", "report_boundary_checked"), "host.clocks")
    for key, value in clock.items():
        if key in ("sums_valid", "admission_stopped", "report_boundary_checked"):
            require(value is True, "Unverified host clock boundary: " + key)
        else:
            integer(value, U64, "host.clocks." + key)
    require(clock["whole_deadline_monotonic_ns"] == case_deadline_ns
            and clock["admission_deadline_monotonic_ns"] == case_deadline_ns - ADMISSION_RESERVE_NS
            and clock["report_deadline_monotonic_ns"] == case_deadline_ns - PROCESS_RESERVE_NS,
            "Host renewed/changed absolute case deadlines")
    start, cursor = clock["started_monotonic_ns"], clock["cursor_monotonic_ns"]
    require(case_started_ns <= start <= cursor <= owner_now_ns <= case_deadline_ns,
            "Host clock lies outside the SAME absolute owner case deadline")
    require(clock["elapsed_ns"] == cursor - start
            == clock["used_active_ns"] + clock["excluded_idle_ns"] + clock["excluded_cleanup_ns"],
            "Host active/idle/cleanup accounting does not exactly reconcile")
    require(clock["used_active_ns"] <= ACTIVE_NS and clock["excluded_cleanup_ns"] <= ACTIVE_NS,
            "Host active/cleanup allowance exceeded")
    elapsed = owner_now_ns - case_started_ns
    active = elapsed - clock["excluded_idle_ns"] - clock["excluded_cleanup_ns"]
    require(0 <= active <= ACTIVE_NS and elapsed <= CASE_NS,
            "Combined owner case active/whole allowance exceeded")
    return {"started_monotonic_ns": case_started_ns, "cursor_monotonic_ns": owner_now_ns,
            "deadline_monotonic_ns": case_deadline_ns, "elapsed_ns": elapsed,
            "used_active_ns": active, "excluded_idle_ns": clock["excluded_idle_ns"],
            "excluded_cleanup_ns": clock["excluded_cleanup_ns"]}


def text(value, maximum, where):
    require(type(value) is str and len(value.encode("utf-8")) <= maximum, where + ": bounded UTF-8 text required")


def choice(value, options, where, *, optional=False):
    require((optional and value is None) or (type(value) is str and value in options), where + ": unknown enum")


def lifecycle(value):
    if value is not None:
        fields(value, ("phase", "initial_failure"), "lifecycle")
        choice(value["phase"], ("Initializing", "Interactive", "DomContentLoaded", "LoadComplete"), "lifecycle.phase")
        boolean(value["initial_failure"], "lifecycle.initial_failure")


def validate_receipt_header(receipt, test, run_id, binary_pin, font_pin, deadline, head):
    fields(receipt, ("schema_version", "scope", "case", "run_id", "manifest_sha256", "contract", "binary", "font", "source",
                     "origin", "url", "port", "deadline_monotonic_ns", "clocks", "semantic_decision",
                     "requires_outer_owner_acceptance", "artifacts", "cleanup", "host_facts", "raw_initial", "host_evidence",
                     "published_trace", "source_plan", "error", "source_declared_call_count", "source_expected_raw_test_names"), "host receipt")
    require(type(receipt["schema_version"]) is int and receipt["schema_version"] == 1
            and receipt["scope"] == "Fixed upstream Window Browser semantic decision; requires independent outer-owner acceptance"
            and receipt["case"] == test["path"] and type(receipt["run_id"]) is int and receipt["run_id"] == run_id
            and receipt["manifest_sha256"] == MANIFEST_SHA and receipt["contract"] == CONTRACT
            and receipt["requires_outer_owner_acceptance"] is True,
            "Host receipt schema/source/run/contract/outer-acceptance binding differs")
    integer(receipt["deadline_monotonic_ns"], U64, "host deadline", 1)
    require(receipt["deadline_monotonic_ns"] == deadline, "Host used a different absolute deadline")
    for name, expected in (("binary", binary_pin), ("font", font_pin)):
        fields(receipt[name], ("bytes", "sha256"), "host." + name)
        integer(receipt[name]["bytes"], BINARY_LIMIT if name == "binary" else font_pin["bytes"], "host." + name + ".bytes", 1)
        digest(receipt[name]["sha256"], "host." + name + ".sha256")
        require(receipt[name] == {key: expected[key] for key in ("bytes", "sha256")}, "Host " + name + " differs from actual built input")
    source = receipt["source"]
    fields(source, ("upstream", "html_sha256", "entry_sha256", "host_sha256", "source_module_sha256", "guard_sha256",
                    "verdict_sha256", "fixture_sha256", "build_revision", "compiled", "package_version"), "host.source")
    require(source["upstream"] == "84daed4ae966d9624bed22f43587c79283ad1fea" and source["html_sha256"] == test["raw_html"]["sha256"],
            "Wrong pinned upstream/document identity")
    for field, path in (("entry_sha256", "examples/wpt_window.rs"), ("host_sha256", "examples/wpt_window/host.rs"),
                        ("source_module_sha256", "examples/wpt_window/source.rs"), ("guard_sha256", "examples/wpt_window/guard.rs"),
                        ("verdict_sha256", "examples/wpt_window/verdict.rs"), ("fixture_sha256", "examples/wpt_window/fixture.rs")):
        digest(source[field], "host.source." + field)
        require(source[field] == read_regular(ROOT / path, INPUT_LIMIT)[1]["sha256"], "Built host captured different source: " + path)
    require(type(source["build_revision"]) is str and re.fullmatch(re.escape(head[:12]) + r"(?:-dirty)?", source["build_revision"]),
            "Host build revision differs from measured source HEAD")
    text(source["compiled"], 128, "host compile timestamp")
    cargo = read_regular(ROOT / "Cargo.toml", METADATA_LIMIT)[0].decode("utf-8")
    version = re.search(r'(?m)^\[workspace\.package\]\nversion = "([^"\n]+)"$', cargo)
    require(version is not None and source["package_version"] == version.group(1), "Host package version differs")
    integer(receipt["source_declared_call_count"], 256, "host declared calls", 1)
    require(receipt["source_declared_call_count"] == test["source_declared_call_count"]
            and receipt["source_expected_raw_test_names"] == test["source_expected_raw_test_names"], "Host source-declared inventory differs")
    integer(receipt["port"], 65535, "host.port", 1)
    require(receipt["origin"] == f"http://127.0.0.1:{receipt['port']}" and receipt["url"] == receipt["origin"] + "/" + test["path"],
            "Host source/owned-origin binding differs")


def internal_cleanup_confirmed(receipt):
    cleanup = receipt["cleanup"]
    fields(cleanup, ("runtime_closed", "pool_wait_idle", "initial_producer_quiescent", "fixture_sealed", "fixture_drained", "fixture_joined",
                     "internal_confirmed", "outer_owner_required"), "host.cleanup")
    for key, value in cleanup.items():
        boolean(value, "host.cleanup." + key)
    require(cleanup["outer_owner_required"] is True, "Host incorrectly claims outer-owner proof")
    return all(cleanup[key] for key in ("runtime_closed", "pool_wait_idle", "initial_producer_quiescent", "fixture_sealed", "fixture_drained",
                                       "fixture_joined", "internal_confirmed"))


def validate_accepted(accepted, run_id):
    fields(accepted, ("scope", "run_id", "missing", "cursor", "progress", "execution_failed", "fault", "parent_failure",
                     "copied_units", "retained_capacity_bytes", "records", "reconciliations"), "accepted ledger")
    require(accepted["scope"] == "Only Browser parent-validated, once-charged accepted ledger; not raw wrapper replies"
            and type(accepted["run_id"]) is int and accepted["run_id"] == run_id, "Accepted scope/run binding")
    boolean(accepted["missing"], "accepted.missing")
    if accepted["missing"]:
        require(all(accepted[name] is None for name in ("cursor", "progress", "execution_failed", "fault", "parent_failure",
                        "copied_units", "retained_capacity_bytes", "records", "reconciliations")), "Missing ledger invented fields")
        return {"final_counts": None, "observed_results": 0, "raw_harness_status": None, "final_only": None}
    integer(accepted["cursor"], 257, "accepted.cursor")
    choice(accepted["progress"], ("Waiting", "Settling", "Finished"), "accepted.progress", optional=True)
    boolean(accepted["execution_failed"], "accepted.execution_failed")
    choice(accepted["fault"], OBSERVER_FAULTS, "accepted.fault", optional=True)
    choice(accepted["parent_failure"], PARENT_FAILURES, "accepted.parent_failure", optional=True)
    integer(accepted["copied_units"], 131072, "accepted.copied_units")
    integer(accepted["retained_capacity_bytes"], 4 * 1024 * 1024, "accepted.retained_capacity_bytes")
    summary = record_inventory(accepted["records"])
    require(accepted["cursor"] == len(accepted["records"])
            and summary["actual_payload_units"] <= accepted["copied_units"], "Accepted cursor/copy evidence differs")
    links = accepted["reconciliations"]
    require(type(links) is list and len(links) <= 256, "Accepted reconciliation bound")
    identities = set()
    for row in links:
        fields(row, ("id", "index", "kind"), "accepted.reconciliation")
        integer(row["id"], 256, "reconciliation.id", 1)
        integer(row["index"], 255, "reconciliation.index")
        choice(row["kind"], ("Unchanged", "PinnedSanitized", "FinalOnly"), "reconciliation.kind")
        require(row["id"] not in identities, "Duplicate reconciliation identity")
        identities.add(row["id"])
    if summary["final_counts"] is not None:
        require(len(links) == sum(summary["final_counts"].values()), "Final reconciliation cardinality differs")
    return summary


def validate_decision(decision, summary):
    fields(decision, ("verdict", "basis", "completion_record", "observed_results", "final_counts", "inventory"), "semantic_decision")
    choice(decision["verdict"], STATUSES[:-1], "semantic_decision.verdict")
    basis = decision["basis"]
    if type(basis) is str:
        choice(basis, ("CleanCompletion", "Assertions", "DeclaredInventoryIncomplete", "DeclaredInventoryMismatch", "HarnessError",
                       "HarnessTimeout", "PreconditionFailed", "MissingCompletion", "UnsettledCompletion"), "semantic basis")
    else:
        require(type(basis) is dict and len(basis) == 1, "Malformed tagged semantic basis")
        kind, value = next(iter(basis.items()))
        options = {"Failure": FAILURE_CAUSES, "Unsupported": {"ModuleScript", "NonWindowGlobal", "CrossWindowProtocol"},
                   "Infrastructure": INFRASTRUCTURE, "Uncertainty": {"Cleanup", "Interrupted", "Superseded"}}
        if kind == "WorkerSignal":
            integer(value, 127, "semantic worker signal", 1)
        else:
            require(kind in options, "Unknown semantic basis tag")
            choice(value, options[kind], "semantic basis payload")
    integer(decision["observed_results"], 256, "semantic observed_results")
    require(decision["observed_results"] == summary["observed_results"], "Semantic/accepted Result count mismatch")
    if decision["completion_record"] is not None:
        integer(decision["completion_record"], 256, "semantic completion index")
    require(decision["completion_record"] == summary.get("completion_record"), "Semantic Completion pointer differs")
    counts = decision["final_counts"]
    if counts is None:
        require(summary["final_counts"] is None, "Semantic counts lost actual Completion")
    else:
        fields(counts, ("total", "pass", "fail", "timeout", "not_run", "precondition_failed", "final_only"), "semantic final_counts")
        for key, value in counts.items():
            integer(value, 256, "semantic counts." + key)
        names = ("pass", "fail", "timeout", "not_run", "precondition_failed")
        require(summary["final_counts"] == {status: counts[key] for status, key in zip(SUBTEST_STATUSES, names)}
                and counts["total"] == sum(counts[key] for key in names) and counts["final_only"] == summary["final_only"],
                "Semantic final counts differ from actual final rows")
    inventory = decision["inventory"]
    if type(inventory) is str:
        choice(inventory, ("Unknown", "Exact"), "semantic inventory")
    else:
        require(type(inventory) is dict and len(inventory) == 1, "Malformed semantic inventory")
        kind, value = next(iter(inventory.items()))
        require(kind in ("Incomplete", "Mismatch"), "Unknown semantic inventory kind")
        fields(value, ("actual", "expected") if kind == "Incomplete" else ("first_index",), "semantic inventory payload")
        for key, scalar in value.items():
            integer(scalar, 256, "semantic inventory." + key)


def utf16(value, where):
    require(type(value) is list and len(value) <= 4096, where + ": UTF-16 field limit")
    for unit in value:
        integer(unit, 65535, where + ".unit")
    return len(value)


def optional_text(value, where):
    require(type(value) is dict, where + ": optional text is not an object")
    kind = value.get("kind")
    if kind in ("Null", "Undefined"):
        fields(value, ("kind",), where)
        return 0
    fields(value, ("kind", "units"), where)
    require(kind == "Text", where + ": unknown optional-text tag")
    return utf16(value["units"], where + ".units")


def test_record(value):
    fields(value, ("id", "index", "name", "status", "message", "stack"), "accepted.test")
    integer(value["id"], 256, "accepted.test.id", 1)
    integer(value["index"], 255, "accepted.test.index")
    require(value["status"] in SUBTEST_STATUSES, "Unknown actual subtest status")
    return (utf16(value["name"], "accepted.test.name")
            + optional_text(value["message"], "accepted.test.message")
            + optional_text(value["stack"], "accepted.test.stack"))


def record_inventory(records):
    """Validate data shape and count actual final rows ONCE; never derive verdict.

    Result prefixes remain evidence, not synthesized final NotRun rows. Names
    are lossless UTF-16 and are not dictionary keys or deduplication identities.
    """
    require(type(records) is list and len(records) <= 257, "Accepted record-count limit")
    results, inventory, harness, completion_record = {}, None, None, None
    units = 0
    for ordinal, record in enumerate(records):
        require(type(record) is dict, "Accepted record object required")
        if record.get("kind") == "Result":
            fields(record, ("kind", "test"), "accepted.Result")
            value = record["test"]
            units += test_record(value)
            require(value["id"] not in results and all(row["index"] != value["index"] for row in results.values()),
                    "Duplicate actual Result identity/index")
            results[value["id"]] = value
        else:
            fields(record, ("kind", "harness", "inventory"), "accepted.Completion")
            require(record["kind"] == "Completion" and inventory is None, "Unknown/duplicate Completion")
            inventory, harness = record["inventory"], record["harness"]
            completion_record = ordinal
            fields(harness, ("status", "message", "stack"), "accepted.harness")
            require(harness["status"] in ("Ok", "Error", "Timeout", "PreconditionFailed"), "Unknown raw harness status")
            units += optional_text(harness["message"], "accepted.harness.message")
            units += optional_text(harness["stack"], "accepted.harness.stack")
            require(type(inventory) is list and len(inventory) <= 256, "Final inventory bound")
            identities = set()
            for index, value in enumerate(inventory):
                units += test_record(value)
                require(value["index"] == index and value["id"] not in identities, "Non-dense/duplicate final inventory")
                identities.add(value["id"])
            require(set(results) <= identities, "Final inventory lost earlier Result identities")
    require(units <= 131072, "Accepted actual UTF-16 payload exceeds cumulative copy bound")
    counts = None if inventory is None else {name: 0 for name in SUBTEST_STATUSES}
    if inventory is not None:
        for row in inventory:
            counts[row["status"]] += 1
    return {"final_counts": counts, "observed_results": len(results),
            "raw_harness_status": None if harness is None else harness["status"],
            "actual_payload_units": units,
            "completion_record": completion_record,
            "final_only": None if inventory is None else len(inventory) - len(results)}


def validate_audit(audit, test, origin, ports):
    fields(audit, ("schema_version", "scope", "case", "origin", "document_url", "port", "attempts_total",
                   "attempts", "faults", "omitted_faults", "seal", "cleanup_confirmed",
                   "clean_attributed_prefix", "complete_four_gets"), "HTTP audit")
    require(type(audit["schema_version"]) is int and audit["schema_version"] == 1, "HTTP audit version")
    require(audit["scope"] == "Pinned immutable four-route accepted-socket audit; not peer receipt or Browser verdict",
            "HTTP audit scope differs")
    integer(audit["port"], 65535, "HTTP port", 1)
    require(audit["port"] not in ports and origin == audit["origin"] == f"http://127.0.0.1:{audit['port']}"
            and audit["case"] == test["path"] and audit["document_url"] == origin + "/" + test["path"],
            "HTTP origin/source binding differs or origin was reused")
    for name in ("cleanup_confirmed", "clean_attributed_prefix", "complete_four_gets"):
        boolean(audit[name], "HTTP." + name)
    seal = audit["seal"]
    fields(seal, ("requested_by_host", "admission_reached", "in_flight_at_seal", "accepted_after_seal",
                  "drain_would_block", "listener_closed", "thread_finished", "thread_joined", "join_before_deadline"), "HTTP seal")
    for key, value in seal.items():
        if key == "accepted_after_seal":
            integer(value, 32, "HTTP seal.accepted_after_seal")
        else:
            boolean(value, "HTTP seal." + key)
    require(audit["cleanup_confirmed"] is True and all(seal[key] is True for key in
            ("drain_would_block", "listener_closed", "thread_finished", "thread_joined", "join_before_deadline")),
            "HTTP fixture drain/listener/thread cleanup proof missing")
    require(type(audit["attempts"]) is list and len(audit["attempts"]) <= 32, "HTTP attempt bound")
    integer(audit["attempts_total"], 32, "HTTP attempts_total")
    require(audit["attempts_total"] == len(audit["attempts"]), "Omitted HTTP attempts")
    require(type(audit["faults"]) is list and len(audit["faults"]) <= 64, "HTTP faults bound")
    integer(audit["omitted_faults"], (1 << 64) - 1, "HTTP omitted_faults")
    for fault in audit["faults"]:
        fields(fault, ("kind", "attempt", "raw_os_error"), "HTTP fault")
        require(fault["kind"] in HTTP_FAULTS, "Unknown typed HTTP fault")
        if fault["attempt"] is not None:
            integer(fault["attempt"], 31, "HTTP fault.attempt")
            require(fault["attempt"] < len(audit["attempts"]), "HTTP fault references absent attempt")
        if fault["raw_os_error"] is not None:
            integer(fault["raw_os_error"], (1 << 31) - 1, "HTTP OS code", -(1 << 31))
    routes = {kind: (path, count, digest_) for kind, path, count, digest_ in HELPER_PINS}
    routes["Document"] = ("/" + test["path"], test["raw_html"]["bytes"], test["raw_html"]["sha256"])
    delivered_routes = []
    for ordinal, attempt in enumerate(audit["attempts"]):
        fields(attempt, ("ordinal", "accepted_elapsed_ns", "completed_elapsed_ns", "accepted_after_seal",
                         "seal_seen_in_flight", "request_bytes_seen", "header_complete", "raw_prefix_hex",
                         "raw_prefix_omitted_bytes", "method", "target", "route", "response_status", "mime",
                         "expected_body_bytes", "expected_body_sha256", "header_bytes_written", "body_bytes_written",
                         "flushed", "delivered", "fault"), "HTTP attempt")
        integer(attempt["ordinal"], 31, "HTTP ordinal")
        require(attempt["ordinal"] == ordinal, "HTTP arrival order differs")
        for key in ("accepted_elapsed_ns", "completed_elapsed_ns"):
            integer(attempt[key], CASE_NS, "HTTP." + key)
        require(attempt["accepted_elapsed_ns"] <= attempt["completed_elapsed_ns"], "HTTP negative duration")
        for key in ("accepted_after_seal", "seal_seen_in_flight", "header_complete", "flushed", "delivered"):
            boolean(attempt[key], "HTTP." + key)
        integer(attempt["request_bytes_seen"], 8193, "HTTP request_bytes_seen")
        prefix = attempt["raw_prefix_hex"]
        require(type(prefix) is str and len(prefix) <= 4096 and len(prefix) % 2 == 0
                and re.fullmatch(r"[0-9a-f]*", prefix), "HTTP raw prefix bound/encoding")
        integer(attempt["raw_prefix_omitted_bytes"], 8193, "HTTP omitted prefix")
        require(len(prefix) // 2 + attempt["raw_prefix_omitted_bytes"] == attempt["request_bytes_seen"],
                "HTTP prefix loss accounting differs")
        for key, maximum in (("method", 16), ("target", 512)):
            value = attempt[key]
            require(value is None or (type(value) is str and len(value) <= maximum
                    and all(32 <= ord(character) <= 126 for character in value)), "HTTP bounded ASCII " + key)
        require(attempt["route"] is None or attempt["route"] in routes, "Unknown HTTP route")
        require(attempt["fault"] is None or attempt["fault"] in HTTP_FAULTS, "Unknown HTTP attempt fault")
        for key in ("expected_body_bytes", "header_bytes_written", "body_bytes_written"):
            integer(attempt[key], 1024 * 1024, "HTTP." + key)
        if attempt["expected_body_sha256"] is not None:
            digest(attempt["expected_body_sha256"], "HTTP expected body SHA")
        require(attempt["response_status"] is None or (type(attempt["response_status"]) is int
                and attempt["response_status"] == 200), "HTTP response status")
        require(attempt["mime"] in (None, "text/html; charset=utf-8", "application/javascript; charset=utf-8"), "HTTP MIME")
        if attempt["delivered"]:
            require(attempt["route"] in routes, "Delivered unknown route")
            target, size, expected_sha = routes[attempt["route"]]
            mime = "text/html; charset=utf-8" if attempt["route"] == "Document" else "application/javascript; charset=utf-8"
            header = f"HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
            require(attempt["method"] == "GET" and attempt["target"] == target and attempt["response_status"] == 200
                    and attempt["mime"] == mime and attempt["expected_body_bytes"] == size
                    and attempt["expected_body_sha256"] == expected_sha and attempt["body_bytes_written"] == size
                    and attempt["header_bytes_written"] == len(header.encode("ascii")) and attempt["flushed"] is True
                    and attempt["header_complete"] is True and attempt["fault"] is None
                    and not attempt["accepted_after_seal"] and not attempt["seal_seen_in_flight"],
                    "HTTP delivered flag lacks complete original-body/MIME/write/flush proof")
            delivered_routes.append(attempt["route"])
    clean = (not audit["faults"] and audit["omitted_faults"] == 0
             and len(delivered_routes) == len(audit["attempts"])
             and len(set(delivered_routes)) == len(delivered_routes))
    require(audit["clean_attributed_prefix"] == clean
            and audit["complete_four_gets"] == (clean and set(delivered_routes) == set(routes)),
            "HTTP complete/prefix flag inconsistent with immutable actual audit")
    return {"port": audit["port"], "http_records": audit["attempts_total"]}


def validate_resources(value, previous):
    fields(value, ("profile", "opcodes_remaining", "source_bytes", "source_admissions", "jobs_admitted", "jobs_executed",
                   "pending_jobs", "fatal_reason", "worker_memory"), "actual modern report")
    require(value["profile"] == "boa-page-process-v1", "Worker profile changed")
    for key, maximum in (("opcodes_remaining", 1_000_000), ("source_bytes", 4 * 1024 * 1024),
                         ("source_admissions", 4 * 1024 * 1024), ("jobs_admitted", 2048),
                         ("jobs_executed", 2048), ("pending_jobs", 256)):
        integer(value[key], maximum, "actual report." + key)
    require(value["source_admissions"] <= value["source_bytes"] and value["jobs_executed"] <= value["jobs_admitted"]
            and value["pending_jobs"] <= value["jobs_admitted"] - value["jobs_executed"], "Actual shared source/work counters inconsistent")
    if value["fatal_reason"] is not None:
        text(value["fatal_reason"], 512, "fatal diagnostic")
        require(value["fatal_reason"] and value["pending_jobs"] == 0, "Fatal pending work or empty reason")
    require(value["opcodes_remaining"] != 0 or value["fatal_reason"] is not None, "Zero opcodes without fatal latch")
    memory = value["worker_memory"]
    fields(memory, ("outstanding_bytes", "peak_bytes", "cumulative_bytes", "allocations",
                    "outstanding_limit_bytes", "cumulative_limit_bytes"), "actual requested-System report")
    for key, scalar in memory.items():
        integer(scalar, U64, "actual memory." + key)
    require(memory["outstanding_limit_bytes"] == 32 * 1024 * 1024
            and memory["cumulative_limit_bytes"] == 64 * 1024 * 1024
            and memory["outstanding_bytes"] <= memory["peak_bytes"] <= memory["outstanding_limit_bytes"]
            and memory["peak_bytes"] <= memory["cumulative_bytes"] <= memory["cumulative_limit_bytes"]
            and memory["allocations"] <= memory["cumulative_bytes"], "Actual requested-System limit/accounting changed")
    if previous is not None:
        require(value["opcodes_remaining"] <= previous["opcodes_remaining"]
                and all(value[key] >= previous[key] for key in ("source_bytes", "source_admissions", "jobs_admitted", "jobs_executed"))
                and (previous["fatal_reason"] is None or value["fatal_reason"] == previous["fatal_reason"])
                and all(memory[key] >= previous["worker_memory"][key] for key in ("peak_bytes", "cumulative_bytes", "allocations")),
                "Actual cumulative worker report renewed counters")


def validate_host_failure(value):
    require(type(value) is dict and type(value.get("kind")) is str, "Typed host failure object required")
    kind = value["kind"]
    payload = {"Source": "error", "Configuration": "reason", "Generation": "reason", "Policy": "reason",
               "Trace": "reason", "OpaqueTransport": "phase"}
    if kind not in payload:
        fields(value, ("kind",), "host failure")
        choice(kind, ("Cutoff", "Closed", "RepeatedStart", "ActivationRefused", "ReplyOverlap", "Projection", "Acknowledgement", "Cleanup"), "host failure")
        return
    fields(value, ("kind", payload[kind]), "host failure")
    item = value[payload[kind]]
    options = {"Configuration": ("WireMode", "Observer", "Bootstrap", "InnerExpectation"),
               "Generation": ("Zero", "Reused", "Mismatch", "InnerRejected"),
               "Policy": ("Navigation", "DefaultAction", "MetaRefresh"),
               "Trace": ("Unavailable", "Capacity", "Allocation", "DiagnosticShape", "ResourceShape", "Missing", "DuplicateAcknowledgement", "ReplyIdentity"),
               "OpaqueTransport": ("Start", "TaskAdmission", "Reply")}
    if kind != "Source":
        choice(item, options[kind], "host failure payload")
    elif type(item) is str:
        choice(item, ("UnknownCase", "DocumentUrl", "EmbeddedCase", "DocumentBytes", "SourceShape", "InlineSource", "SourceIdentity", "HelperCount"), "source failure")
    else:
        require(type(item) is dict and len(item) == 1, "Source indexed failure")
        tag, indexed = next(iter(item.items()))
        choice(tag, ("EmbeddedHelper", "HelperNode", "HelperBytes", "HelperProvenance"), "Source indexed failure kind")
        fields(indexed, ("index",), "source failure index")
        integer(indexed["index"], 2, "source helper failure index")


def validate_evidence(snapshot, published, run_id):
    require(type(published) is list and len(published) <= 64, "Published acceptance trace bound")
    if snapshot is None:
        require(not published, "Published replies lack host evidence")
        return
    scalar_names = ("failures_total", "failures_omitted", "starts", "cancel_calls", "close_calls", "wait_idle_calls",
                    "diagnostics_total", "diagnostics_omitted", "diagnostic_utf8_bytes_total", "diagnostic_utf8_bytes_retained", "diagnostic_utf8_bytes_omitted")
    fields(snapshot, (*scalar_names, "first_failure", "secondary_failures", "wait_idle_succeeded", "wait_idle_failed",
                      "diagnostic_prefix_sealed", "diagnostic_prefix", "raw_trace"), "host evidence")
    for key in scalar_names:
        integer(snapshot[key], U64, "host evidence." + key)
    for key in ("wait_idle_succeeded", "wait_idle_failed", "diagnostic_prefix_sealed"):
        boolean(snapshot[key], "host evidence." + key)
    failures = snapshot["secondary_failures"]
    require(type(failures) is list and len(failures) <= 64, "Secondary typed failure bound")
    all_failures = ([snapshot["first_failure"]] if snapshot["first_failure"] is not None else []) + failures
    for value in all_failures:
        fields(value, ("ordinal", "failure"), "typed failure fact")
        integer(value["ordinal"], U64, "typed failure ordinal")
        validate_host_failure(value["failure"])
    require(len(all_failures) + snapshot["failures_omitted"] == snapshot["failures_total"], "Typed failure omission accounting")
    diagnostics = snapshot["diagnostic_prefix"]
    require(type(diagnostics) is list and len(diagnostics) <= 64, "Diagnostic prefix bound")
    retained = 0
    for value in diagnostics:
        fields(value, ("transaction", "text", "original_utf8_bytes", "omitted_utf8_bytes"), "host diagnostic")
        integer(value["transaction"], 64, "diagnostic transaction")
        text(value["text"], 4096, "diagnostic prefix text")
        integer(value["original_utf8_bytes"], U64, "diagnostic original bytes")
        integer(value["omitted_utf8_bytes"], U64, "diagnostic omitted bytes")
        require(len(value["text"].encode("utf-8")) + value["omitted_utf8_bytes"] == value["original_utf8_bytes"], "Diagnostic truncation accounting")
        retained += len(value["text"].encode("utf-8"))
    fully_retained = sum(value["omitted_utf8_bytes"] == 0 for value in diagnostics)
    require(fully_retained + snapshot["diagnostics_omitted"] == snapshot["diagnostics_total"]
            and retained == snapshot["diagnostic_utf8_bytes_retained"]
            and retained + snapshot["diagnostic_utf8_bytes_omitted"] == snapshot["diagnostic_utf8_bytes_total"], "Diagnostic cumulative omission accounting")
    traces = snapshot["raw_trace"]
    require(type(traces) is list and len(traces) <= 64, "Raw scalar/resource trace bound (not raw wire)")
    previous = None
    for trace in traces:
        fields(trace, ("generation", "session_id", "sequence", "revision", "state", "lifecycle", "observer", "scripts_executed",
                       "diagnostics", "rejected_initial", "resources", "wrapper_validation_ns", "manager_charge_ns", "manager_charge_succeeded"), "raw trace")
        for key in ("generation", "session_id", "revision", "scripts_executed", "diagnostics", "wrapper_validation_ns"):
            integer(trace[key], U64, "raw trace." + key)
        for key in ("sequence", "manager_charge_ns"):
            if trace[key] is not None:
                integer(trace[key], U64, "raw trace." + key)
        for key in ("rejected_initial", "manager_charge_succeeded"):
            boolean(trace[key], "raw trace." + key)
        choice(trace["state"], ("Ready", "Fatal", "Closed"), "raw realm state")
        lifecycle(trace["lifecycle"])
        observer = trace["observer"]
        require(type(observer) is dict, "Raw scalar observer shape")
        kind = observer.get("kind")
        if kind in ("Missing", "Disabled"):
            fields(observer, ("kind",), "raw observer")
        else:
            names = (("kind", "run_id", "first_ordinal", "next_ordinal", "progress", "execution_failed",
                      "result_callbacks", "completion_callbacks", "final_inventory_rows") if kind == "WindowHarness"
                     else ("kind", "run_id", "prefix_end", "fault", "execution_failed"))
            fields(observer, names, "raw observer")
            require(kind in ("WindowHarness", "ObservationFailed") and type(observer["run_id"]) is int
                    and observer["run_id"] == run_id, "Raw observer mode/run")
            boolean(observer["execution_failed"], "raw observer execution_failed")
            if kind == "WindowHarness":
                for key in ("first_ordinal", "next_ordinal", "result_callbacks", "completion_callbacks", "final_inventory_rows"):
                    integer(observer[key], 257, "raw observer." + key)
                choice(observer["progress"], ("Waiting", "Settling", "Finished"), "raw observer progress")
            else:
                integer(observer["prefix_end"], 257, "raw observer prefix_end")
                choice(observer["fault"], OBSERVER_FAULTS, "raw observer fault")
        resource = trace["resources"]
        if resource is None:
            require(trace["rejected_initial"], "Non-rejected reply omitted actual resources")
        else:
            validate_resources(resource, previous)
            require(trace["state"] == "Ready" or resource["pending_jobs"] == 0, "Terminal pending jobs")
            previous = resource
    last_index = -1
    for value in published:
        fields(value, ("raw_trace_index", "revision", "cursor", "progress", "fault", "execution_failed", "lifecycle", "state", "pending_work"), "published trace")
        integer(value["raw_trace_index"], 63, "published raw index")
        require(last_index < value["raw_trace_index"] < len(traces), "Published trace index/order")
        last_index = value["raw_trace_index"]
        raw = traces[last_index]
        require(raw["manager_charge_succeeded"] is True and raw["manager_charge_ns"] is not None,
                "Published evidence lacks successful one-charge acknowledgement")
        integer(value["revision"], U64, "published revision")
        integer(value["cursor"], 257, "published cursor")
        choice(value["progress"], ("Waiting", "Settling", "Finished"), "published progress", optional=True)
        choice(value["fault"], OBSERVER_FAULTS, "published fault", optional=True)
        boolean(value["execution_failed"], "published execution_failed")
        lifecycle(value["lifecycle"])
        choice(value["state"], ("Ready", "Fatal", "Closed"), "published state")
        if value["pending_work"] is not None:
            integer(value["pending_work"], 256, "published pending_work")
        require(value["revision"] == raw["revision"] and value["state"] == raw["state"], "Published/raw identity mismatch")


def validate_host(receipt, accepted, audit, test, run_id, ports, *, accepted_raw, audit_raw):
    require(type(receipt["semantic_decision"]) is dict, "Semantic Decision object required")
    semantic_error = receipt["semantic_decision"].get("verdict") == "ERROR"
    if receipt["error"] is not None:
        fields(receipt["error"], ("kind", "detail"), "typed host error")
        choice(receipt["error"]["kind"], ("Arguments", "Input", "Manifest", "Font", "Clock", "Active", "Interrupted",
                                          "Fixture", "Runtime", "Artifact", "Correlation"), "host error kind")
        text(receipt["error"]["detail"], 2048, "host error detail")
        require(semantic_error, "Host error contradicts nonERROR disposition")
    fields(receipt["artifacts"], ("accepted", "http"), "host artifacts")
    for key, name, body in (("accepted", "accepted.json", accepted_raw), ("http", "http-audit.json", audit_raw)):
        identity = receipt["artifacts"][key]
        fields(identity, ("file", "bytes", "sha256"), "artifact identity")
        require(identity == {"file": name, "bytes": len(body), "sha256": sha(body)}, "Host artifact byte/hash binding differs")
    summary = validate_accepted(accepted, run_id)
    validate_decision(receipt["semantic_decision"], summary)
    validate_evidence(receipt["host_evidence"], receipt["published_trace"], run_id)
    if receipt["published_trace"]:
        last = receipt["published_trace"][-1]
        require(not accepted["missing"] and all(accepted[key] == last[key]
                for key in ("cursor", "progress", "fault", "execution_failed")),
                "Final accepted ledger differs from published acceptance evidence")
    facts = receipt["host_facts"]
    fields(facts, ("expected", "premises_verified", "infrastructure", "cleanup", "stop", "delivery", "lifecycle", "realm", "initial",
                   "native_history", "pending_work", "first_established_failure", "transport_consequence", "initial_transport_consequence",
                   "worker_signal", "unsupported_before_js", "rejected_init_validated"), "host facts")
    require(facts["expected"] == {"kind": "WindowHarness", "run_id": run_id} and facts["premises_verified"] is True
            and facts["cleanup"] == "Confirmed", "Host premises/expectation/internal cleanup differ")
    choice(facts["infrastructure"], INFRASTRUCTURE, "host infrastructure", optional=True)
    require(facts["infrastructure"] not in ("Inventory", "Authority", "Configuration")
            and (semantic_error or facts["infrastructure"] is None), "Unsafe host source/authority/configuration premise")
    basis = receipt["semantic_decision"]["basis"]
    require(not (type(basis) is dict and (basis.get("Infrastructure") in ("Inventory", "Authority", "Configuration")
                or "Uncertainty" in basis)), "Unsafe/uncertain semantic premise cannot continue")
    choice(facts["stop"], ("AcceptedCutoff", "AdmissionDeadline", "BeforeJavaScript", "Terminal", "Interrupted", "Superseded"), "host stop")
    choice(facts["delivery"], ("CompleteFourGets", "AuditedPrefix", "Unverified"), "host delivery")
    lifecycle(facts["lifecycle"])
    choice(facts["realm"], ("Ready", "Fatal", "Closed"), "host realm", optional=True)
    choice(facts["initial"], ("Unknown", "Pending", "Clean", "ScriptFailed", "InfrastructureFailed"), "host initial")
    choice(facts["native_history"], ("Unknown", "Clean", "Failed"), "host native history")
    choice(facts["first_established_failure"], FAILURE_CAUSES, "host first failure", optional=True)
    for key in ("transport_consequence", "initial_transport_consequence"):
        choice(facts[key], ("None", "ValidatedPolicyDenied", "ValidatedResourceAdmission"), "host." + key)
    choice(facts["unsupported_before_js"], ("ModuleScript", "NonWindowGlobal", "CrossWindowProtocol"), "host prerequisite", optional=True)
    boolean(facts["rejected_init_validated"], "host rejected Init")
    if facts["pending_work"] is not None:
        integer(facts["pending_work"], 256, "host pending_work")
    if facts["worker_signal"] is not None:
        fields(facts["worker_signal"], ("signal",), "actual worker signal evidence")
        integer(facts["worker_signal"]["signal"], 127, "actual worker signal", 1)
    if receipt["published_trace"]:
        last = receipt["published_trace"][-1]
        require(facts["realm"] == last["state"] and facts["pending_work"] == last["pending_work"]
                and facts["lifecycle"] == last["lifecycle"], "Host facts differ from final accepted publication")
    raw_initial = receipt["raw_initial"]
    if raw_initial is not None:
        fields(raw_initial, ("status", "failure", "scripts_executed", "error_count", "broker_rejected_sources", "diagnostics"), "raw initial outcome")
        choice(raw_initial["status"], ("NotRequested", "Pending", "Complete", "Partial", "Failed"), "raw initial status")
        choice(raw_initial["failure"], ("Script", "Infrastructure"), "raw initial failure", optional=True)
        for key in ("scripts_executed", "error_count", "broker_rejected_sources"):
            integer(raw_initial[key], U64, "raw initial." + key)
        require(type(raw_initial["diagnostics"]) is list and len(raw_initial["diagnostics"]) <= 64, "Raw initial diagnostic bound")
        for diagnostic in raw_initial["diagnostics"]:
            text(diagnostic, 4096, "raw initial diagnostic")
    plan = receipt["source_plan"]
    if plan is not None:
        fields(plan, ("harness_node", "vendor_node", "parsing_node", "inline_node"), "actual Mg source plan")
        for value in plan.values():
            integer(value, 49999, "actual Mg source node")
        require(len(set(plan.values())) == 4, "Actual source node identities overlap")
    proof = validate_audit(audit, test, receipt["origin"], ports)
    require(proof["port"] == receipt["port"], "Receipt/audit port differs")
    if facts["delivery"] == "CompleteFourGets":
        require(audit["complete_four_gets"] is True, "Host complete-delivery premise lacks actual audit")
    elif facts["delivery"] == "AuditedPrefix":
        require(audit["clean_attributed_prefix"] is True, "Host negative-prefix premise lacks actual audit")
    # This is binding verification, not a Python semantic mapper. The unchanged
    # Decision is authoritative only AFTER these independent owner predicates.
    require(receipt["semantic_decision"]["verdict"] != "CRASH", "No authoritative worker-signal evidence is provided by this host")
    return {**proof, **summary}


def artifact_references(directory, artifacts):
    return {name: {"path": str((directory / name).relative_to(directory.parent)),
                   "bytes": len(raw), "sha256": sha(raw)}
            for name, raw in artifacts.items()}


def owner_error(row, reason):
    # Never turn a host wait status or diagnostic string into a worker crash or
    # CSS assertion. The typed internal decision is retained, but not adopted.
    if row.get("semantic_decision") is not None:
        row["unadopted_semantic_decision"] = row.pop("semantic_decision")
    row.update(status="ERROR", owner_accepted=False, evidence_validated=False,
               owner_failure={"layer": "OwnerInfrastructure", "reason": str(reason)[:1024]})


def refresh(report, budget, began):
    report["distributions"] = distributions(report["results"])
    report["selected_total"] = 16
    report["completed_total"] = sum(row.get("evidence_validated") is True for row in report["results"])
    report["attempted_total"] = sum(row.get("attempted") is True for row in report["results"])
    report["execution"] = {"serial": True, "cleanup_confirmed": budget.cleanup_confirmed,
                           "remaining_producer_reserved_bytes": budget.reserved,
                           "peak_producer_reserved_bytes": budget.peak_reserved,
                           "http_records": budget.requests,
                           "elapsed_seconds": time.monotonic() - began,
                           "cancelled": budget.cancel.is_set(), "owner_failure": budget.failure}


def run_case(control, budget, report, index, test, binary, binary_pin, font, font_pin, font_admission, ports, began,
             checker, document, source_before):
    case_started = time.monotonic_ns()
    row = report["results"][index]
    budget.reserve(CASE_RESERVE)
    confirmed, attempted = True, False
    case_deadline = case_started + CASE_NS
    process_deadline = case_deadline - PROCESS_RESERVE_NS
    directory = budget.output / f"case-{index + 1:02d}"
    row.update(attempted=True, owner_accepted=False, deadline_monotonic_ns=case_deadline)
    try:
        require(time.monotonic() + 25 + RUN_FINAL_RESERVE <= budget.deadline,
                "Insufficient whole-run reservation for a fresh case")
        require(not directory.exists() and not directory.is_symlink(), "Case output reuse")
        environment = private_env(budget.output / f"profile-{index + 1:02d}")
        require(read_regular(binary, BINARY_LIMIT)[1] == binary_pin, "Host executable changed before launch")
        read_font(font, font_admission, font_pin)
        command = [str(binary), "--case", test["path"], "--font", str(font), "--font-sha", font_pin["sha256"],
                   "--font-bytes", str(font_admission["bytes"]),
                   "--output", str(directory), "--deadline-monotonic-ns", str(case_deadline),
                   "--forbidden-ports", ",".join(str(port) for port in sorted(ports))]
        now = time.monotonic()
        require(now < process_deadline / 1e9 and not budget.is_set(), "Case admission expired")
        # Once launch may occur, direct-group cleanup alone cannot prove the
        # host's pool close/wait_idle and fixture join. Keep the slot until BOTH.
        attempted, confirmed = True, False
        process = control.run_bounded(command, cwd=ROOT, env=environment,
                                      log_path=budget.output / f"case-{index + 1:02d}.log",
                                      timeout=process_deadline / 1e9 - now,
                                      global_deadline=process_deadline / 1e9,
                                      max_output_bytes=LOG_LIMIT, cancel_event=budget,
                                      artifact_limits={directory / "receipt.json": ARTIFACT_LIMIT,
                                                       directory / "accepted.json": ARTIFACT_LIMIT,
                                                       directory / "http-audit.json": AUDIT_LIMIT})
        row["process"] = process
        require(process.get("cleanup_succeeded") is True,
                "Outer owned-group/direct-leader cleanup is unconfirmed; artifacts quarantined")
        require(process.get("completed") is True,
                "Host did not complete ordinarily; killed-host artifacts cannot prove inner cleanup")
        # A normal nonzero host may leave a bounded INTERNAL decision. It is
        # only secondary evidence unless every outer predicate below succeeds.
        receipt_raw = control.read_artifact(directory / "receipt.json", limit=ARTIFACT_LIMIT)
        receipt = strict_json(receipt_raw, ARTIFACT_LIMIT)
        row["artifacts"] = artifact_references(directory, {"receipt.json": receipt_raw})
        if type(receipt) is dict and "semantic_decision" in receipt:
            row["unadopted_semantic_decision"] = receipt["semantic_decision"]
        validate_receipt_header(receipt, test, index + 1, binary_pin, font_pin, case_deadline, report["git_commit"])
        require(internal_cleanup_confirmed(receipt), "Host pool/fixture cleanup is unconfirmed; reservation quarantined")
        semantic_error = type(receipt["semantic_decision"]) is dict and receipt["semantic_decision"].get("verdict") == "ERROR"
        verify_process(process, semantic_error=semantic_error)
        accepted_raw = control.read_artifact(directory / "accepted.json", limit=ARTIFACT_LIMIT)
        audit_raw = control.read_artifact(directory / "http-audit.json", limit=AUDIT_LIMIT)
        accepted = strict_json(accepted_raw, ARTIFACT_LIMIT)
        audit = strict_json(audit_raw, AUDIT_LIMIT)
        summary = validate_host(receipt, accepted, audit, test, index + 1, ports,
                                accepted_raw=accepted_raw, audit_raw=audit_raw)
        require({path.name for path in directory.iterdir()} == {"accepted.json", "receipt.json", "http-audit.json"},
                "Unexpected host evidence (raw wire/screenshots are not admitted)")
        require(read_regular(binary, BINARY_LIMIT)[1] == binary_pin, "Host executable changed after execution")
        read_font(font, font_admission, font_pin)
        require(fingerprint(checker, document, case_deadline / 1e9) == source_before,
                "Evaluated source/input fingerprint changed before further case admission")
        # Do not release on the receipt's cleanup booleans alone: its exact
        # source, accepted payload AND immutable fixture audit must agree first.
        confirmed = True
        require(evidence_bytes(directory) + (budget.output / f"case-{index + 1:02d}.log").stat().st_size <= CASE_RESERVE,
                "Case evidence exceeded its10MiB reserved payload")
        budget.admit_requests(summary["http_records"])
        ports.add(summary["port"])
        if not semantic_error:
            row.pop("unadopted_semantic_decision", None)
            row["semantic_decision"] = receipt["semantic_decision"]
        row.update(status=receipt["semantic_decision"]["verdict"], owner_accepted=not semantic_error,
                   evidence_validated=True, final_subtests=summary["final_counts"],
                   raw_harness_status=summary["raw_harness_status"], origin=receipt["origin"],
                   host_clocks=receipt["clocks"], http_records=summary["http_records"],
                   artifacts=artifact_references(directory, {"receipt.json": receipt_raw,
                               "accepted.json": accepted_raw, "http-audit.json": audit_raw}))
        # Include final aggregate serialization/atomic replacement in the SAME
        # measured case, not just the earlier host clock snapshot.
        refresh(report, budget, began)
        budget.report(report)
        row["owner_clocks"] = validate_clock(receipt["clocks"], case_started, case_deadline, time.monotonic_ns())
    except (Exception, KeyboardInterrupt) as error:
        owner_error(row, error)
        budget.stop(error)
    finally:
        budget.release(CASE_RESERVE, confirmed if attempted else True)
        row["cleanup_confirmed"] = confirmed if attempted else True
        if row.get("evidence_validated") is True:
            try:
                row["owner_clocks"] = validate_clock(receipt["clocks"], case_started, case_deadline, time.monotonic_ns())
            except (Exception, KeyboardInterrupt) as error:
                owner_error(row, error)
                budget.stop(error)
    return row.get("evidence_validated", False) and "owner_failure" not in row


def run(args, began):
    deadline = began + RUN_SECONDS
    output = fresh_directory(args.output)
    budget = Budget(output, deadline)
    report = {"schema_version": 1, "status": "RUNNING", "scope": SCOPE,
              "started_utc": datetime.now(timezone.utc).isoformat(), "complete": False,
              "baseline_review_eligible": False, "score_written": False, "expectations_written": False,
              "selection": "css-flexbox-window-declared-value-parsing-v1",
              "manifest_sha256": MANIFEST_SHA, "observer_contract": CONTRACT,
              "results": pending_results(), "infrastructure_errors": [],
              "limits": {"whole_run_seconds": RUN_SECONDS, "build_seconds": BUILD_SECONDS,
                         "whole_case_ns": CASE_NS, "admission_ns": CASE_NS - ADMISSION_RESERVE_NS,
                         "host_report_ns": CASE_NS - PROCESS_RESERVE_NS, "active_ns": ACTIVE_NS,
                         "input_files": MAX_INPUT_FILES, "input_bytes": INPUT_LIMIT,
                         "metadata_bytes": METADATA_LIMIT, "manifest_bytes": MANIFEST_LIMIT,
                         "evidence_bytes": EVIDENCE_LIMIT, "case_reserved_bytes": CASE_RESERVE,
                         "atomic_report_reserved_bytes": REPORT_LIMIT, "report_bytes": REPORT_LIMIT,
                         "http_records": REQUEST_LIMIT, "raw_wire_persisted": False}}
    refresh(report, budget, began)
    budget.report(report)
    previous_signals = {}

    def interrupted(signum, _frame):
        # A nonraising handler cannot interrupt Popen's ownership handoff. The
        # reviewed process utility observes cancellation and kills before reap.
        # Only scalar assignment here: acquiring Budget/Event locks in a signal
        # handler could deadlock if the signal interrupted the same lock holder.
        budget.interruption = "Owner interrupted by signal " + str(signum)

    try:
        require(sys.platform == "linux" and os.uname().machine == "x86_64", "Restricted Linux x86_64 host required")
        sys.dont_write_bytecode = True
        for signum in (signal.SIGINT, signal.SIGTERM):
            previous_signals[signum] = signal.signal(signum, interrupted)
        checker = load_definitions(CHECKER_PATH, "mg_window_source_checker")
        control = load_definitions(CONTROL_PATH, "mg_window_process_control")
        report["source_validation"] = checker.check(ROOT)
        manifest_raw, _ = read_regular(ROOT / MANIFEST_PATH, MANIFEST_LIMIT, expected=MANIFEST_SHA)
        document = strict_json(manifest_raw, MANIFEST_LIMIT)
        require(tuple(test["path"] for test in document["tests"]) == CASE_PATHS, "Selected ordered denominator differs")
        for row, test in zip(report["results"], document["tests"]):
            row["source_declared_calls"] = test["source_declared_call_count"]
        source_before = fingerprint(checker, document, deadline - RUN_FINAL_RESERVE)
        report["source_fingerprint"] = source_before
        font = safe_path(args.font)
        font_admission = admit_font(font, source_before)
        report["input_admission"] = font_admission
        _, font_pin = read_font(font, font_admission)
        report["font"] = {"path": str(font), **font_pin}
        build_environment = private_env(output / "build-profile", build=True)
        toolchain = tool(control, budget, ["rustc", "--version"], "rustc", build_environment).strip()
        require(toolchain.startswith("rustc 1.91.1 "), "Pinned Rust1.91.1 is required")
        report["toolchain"] = toolchain
        report["cargo"] = tool(control, budget, ["cargo", "--version"], "cargo", build_environment).strip()
        report["git_commit"] = tool(control, budget, ["git", "rev-parse", "HEAD"], "git-head", build_environment).strip()
        report["git_dirty"] = bool(tool(control, budget, ["git", "status", "--porcelain"], "git-status", build_environment))
        require(re.fullmatch(r"[0-9a-f]{40}", report["git_commit"]), "Invalid tested Git identity")
        binary = build(control, budget, build_environment)
        _, binary_pin = read_regular(binary, BINARY_LIMIT)
        require(os.access(binary, os.X_OK), "Built host is not executable")
        report["binary"] = {"path": str(binary), **binary_pin}
        ports = set()
        for index, test in enumerate(document["tests"]):
            if budget.is_set() or time.monotonic() + 25 + RUN_FINAL_RESERVE > deadline:
                report["infrastructure_errors"].append("Insufficient reserved run time or stopped admission; remaining files PENDING")
                break
            if not run_case(control, budget, report, index, test, binary, binary_pin, font, font_pin, font_admission, ports, began,
                            checker, document, source_before):
                break
        if budget.cleanup_confirmed:
            source_after = fingerprint(checker, document, deadline)
            report["source_fingerprint_after"] = {"sha256": source_after["sha256"]}
            require(source_after == source_before, "Evaluated source/input fingerprint changed during run")
            require(read_regular(binary, BINARY_LIMIT)[1] == binary_pin, "Built host changed during run")
            read_font(font, font_admission, font_pin)
        else:
            report["infrastructure_errors"].append("Unknown cleanup; no producer artifact/fingerprint resnapshot")
    except (Exception, KeyboardInterrupt) as error:
        budget.stop(error)
        report["infrastructure_errors"].append(str(error)[:1024])
    finally:
        refresh(report, budget, began)
        report["complete"] = (report["completed_total"] == 16 and budget.cleanup_confirmed
                              and "source_fingerprint_after" in report)
        report["baseline_review_eligible"] = (report["complete"] and budget.cleanup_confirmed
                    and budget.reserved == 0 and not budget.cancel.is_set()
                    and budget.interruption is None
                    and not report["infrastructure_errors"]
                    and all(row["owner_accepted"] for row in report["results"])
                    and not any(row["status"] in ("ERROR", "CRASH", "PENDING") for row in report["results"]))
        report["status"] = ("COMPLETE" if report["baseline_review_eligible"] else "COMPLETE_NONADOPTABLE" if report["complete"]
                            else "ERROR" if budget.cancel.is_set() else "INCOMPLETE")
        try:
            require(time.monotonic() < deadline, "Whole25-minute owner deadline exceeded")
            budget.report(report)
            # Final serialization and stdout do not grant any renewed deadline.
            require(time.monotonic() <= deadline, "Final report exceeded whole-run deadline")
            print(json.dumps({"status": report["status"], "complete": report["complete"],
                              "baseline_review_eligible": report["baseline_review_eligible"],
                              "distributions": report["distributions"], "report": str(output / "report.json")}, sort_keys=True))
            require(time.monotonic() <= deadline, "Final output exceeded whole-run deadline")
        finally:
            for signum, prior in previous_signals.items():
                signal.signal(signum, prior)
    return 0 if report["baseline_review_eligible"] else 1


def main(argv=None):
    # No source checker, Rust, child, profile or file is touched by module import.
    began = time.monotonic()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--font", type=Path, default=Path("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"))
    args = parser.parse_args(argv)
    return run(args, began)


if __name__ == "__main__":
    raise SystemExit(main())
