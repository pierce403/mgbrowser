#!/usr/bin/env python3
"""Bounded P0/P1 research runner. It does not execute browser page scripts.

The configured executable must isolate itself before reading JSON input. This
parent additionally enforces wall/output limits and launches with an empty env.
No automatic downloads, package installs, engine fallbacks or pass normalization.
"""

import argparse
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import selectors
import signal
import subprocess
import tarfile
import tempfile
import time
from types import SimpleNamespace
import urllib.request


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name("inputs.json")
FIXTURES = ROOT / "tests/fixtures/jsplan"
REQUEST_CAP = 2 * 1024 * 1024
OUTPUT_CAP = 4 * 1024 * 1024
STDERR_CAP = 64 * 1024
DEADLINE = 2.0
REAP_GRACE = 0.10
CLEANUP_DEADLINE = 1.0
CLASSES = ("pass", "assertion-failure", "unsupported", "exception", "crash",
           "timeout", "termination", "harness-failure", "infrastructure-error")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def load_manifest():
    return json.loads(MANIFEST.read_text())


def verified_read(path, digest):
    data = path.read_bytes()
    if sha256(data) != digest:
        raise ValueError(f"SHA-256 mismatch: {path}")
    return data


def corpus_fingerprint(root):
    """Hash exact bytes and relative paths of all execution inputs, not mtimes."""
    paths = sorted([*root.joinpath("test").rglob("*"), *root.joinpath("harness").rglob("*"),
                    root / "LICENSE", root / "INTERPRETING.md"])
    digest = hashlib.sha256()
    for path in paths:
        if path.is_symlink():
            raise ValueError(f"Symlink in frozen corpus: {path}")
        if path.is_file():
            digest.update(str(path.relative_to(root)).encode() + b"\0")
            digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def baseline_fingerprint():
    root = ROOT / "crates/mg-butane"
    paths = sorted([root / "Cargo.toml", *root.joinpath("src").rglob("*.rs")])
    digest = hashlib.sha256()
    for path in paths:
        digest.update(str(path.relative_to(root)).encode() + b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def fetch(cache):
    """Explicit network setup: only immutable, hash-verified archive inputs."""
    cache.mkdir(parents=True, exist_ok=True)
    manifest = load_manifest()
    for name in ("test262", "vue"):
        entry = manifest[name]
        archive = cache / (name + ".tar.gz")
        if not archive.exists():
            with urllib.request.urlopen(entry["url"], timeout=30) as response:
                data = response.read(32 * 1024 * 1024 + 1)
            if len(data) > 32 * 1024 * 1024 or sha256(data) != entry["sha256"]:
                raise ValueError(f"Archive size/hash rejection: {name}")
            archive.write_bytes(data)
        verified_read(archive, entry["sha256"])
        # Never use extractall on downloaded paths. Reject links/devices and
        # bound both file count and decoded bytes before any member extraction.
        with tarfile.open(archive, "r:gz") as tar:
            members = tar.getmembers()
            if len(members) > 100000 or sum(m.size for m in members) > 256 * 1024 * 1024:
                raise ValueError("Archive expansion limit")
            expected_root = entry.get("root", "package")
            for member in members:
                path = Path(member.name)
                if (path.is_absolute() or ".." in path.parts or not path.parts
                        or path.parts[0] != expected_root
                        or not (member.isfile() or member.isdir())):
                    raise ValueError(f"Unsafe archive member: {member.name}")
            for member in members:
                destination = cache / member.name
                if any(path.is_symlink() for path in (destination, *destination.parents)):
                    raise ValueError(f"Symlink in extraction destination: {destination}")
                if member.isdir():
                    destination.mkdir(parents=True, exist_ok=True)
                else:
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    with tar.extractfile(member) as source:
                        destination.write_bytes(source.read())
        print(f"Verified {name}: {entry['sha256']}")
    verified_read(cache / manifest["vue"]["bundle"], manifest["vue"]["bundle_sha256"])


def metadata(source):
    """Parse only Test262 execution metadata, never arbitrary YAML objects.

    This is intentionally not a general YAML parser. Scalar names, flow lists
    and indented block lists cover this pinned corpus. Unsupported encodings
    fail closed, rather than silently dropping a flag or include.
    """
    match = re.search(r"/\*---(.*?)---\*/", source, re.S)
    if not match:
        raise ValueError("Missing Test262 frontmatter")
    front = match.group(1)
    result = {"flags": [], "features": [], "includes": []}
    for key in result:
        field = re.search(r"^" + key + r":([^\n]*)(\n(?:[ \t]+[^\n]*\n|\n)*)?", front + "\n", re.M)
        if not field:
            continue
        value = (field.group(1) + (field.group(2) or "")).strip()
        if value.startswith("[") and value.endswith("]"):
            entries = value[1:-1].split(",") if value[1:-1].strip() else []
        elif value.startswith("-"):
            entries = [line.strip()[1:].strip() for line in value.splitlines() if line.strip()]
        else:
            raise ValueError(f"Unsupported {key} metadata: {value!r}")
        parsed = []
        for entry in entries:
            entry = entry.strip().strip("\"'")
            if not re.fullmatch(r"[A-Za-z0-9_./@+-]+", entry):
                raise ValueError(f"Unsupported {key} entry: {entry!r}")
            parsed.append(entry)
        result[key] = parsed
    negative = re.search(r"^negative:\s*\n((?:[ \t]+[^\n]*\n)+)", front + "\n", re.M)
    if negative:
        pairs = dict(re.findall(r"^\s+(phase|type):\s*([A-Za-z][A-Za-z0-9]*)\s*$", negative.group(1), re.M))
        if set(pairs) != {"phase", "type"} or pairs["phase"] not in {"parse", "resolution", "runtime"}:
            raise ValueError("Unsupported negative metadata")
        result["negative"] = pairs
    elif re.search(r"^negative:", front, re.M):
        raise ValueError("Unsupported negative metadata encoding")
    return result


def variants(meta):
    flags = set(meta["flags"])
    mode = flags & {"onlyStrict", "noStrict", "module", "raw"}
    if len(mode) > 1 and mode != {"raw", "module"}:
        raise ValueError(f"Conflicting Test262 modes: {sorted(mode)}")
    if "module" in mode:
        return ["module-raw" if "raw" in mode else "module"]
    if "raw" in mode:
        return ["raw"]
    if "onlyStrict" in mode:
        return ["strict"]
    if "noStrict" in mode:
        return ["sloppy"]
    return ["sloppy", "strict"]


def request(source, **extra):
    value = dict(protocol=1, action="evaluate", goal="script", source=source,
                 includes=[], modules={}, drain_jobs=True, **{"async": False})
    value.update(extra)
    return value


def test262_cases(cache):
    spec = load_manifest()["test262"]
    root = cache / spec["root"]
    if not root.is_dir():
        raise ValueError("Missing corpus: run the explicit fetch subcommand first")
    if corpus_fingerprint(root) != spec["corpus_sha256"]:
        raise ValueError("Extracted Test262 corpus changed; fetch the frozen inputs again")
    selected = set(spec["files"])
    for family in spec["families"]:
        selected.update(str(path.relative_to(root / "test")) for path in (root / "test" / family).rglob("*.js")
                        if "_FIXTURE" not in path.name)
    totals = dict(files=0, variants=0, fixtures=0, metadata_errors=0)
    for path in (root / "test").rglob("*.js"):
        if "_FIXTURE" in path.name:
            totals["fixtures"] += 1
            continue
        totals["files"] += 1
        try:
            totals["variants"] += len(variants(metadata(path.read_text())))
        except ValueError:
            totals["metadata_errors"] += 1
    if totals["metadata_errors"]:
        # Never publish a falsely exact full-suite variant denominator.
        totals["variants"] = None
    cases = []
    for relative in sorted(selected):
        source = (root / "test" / relative).read_bytes().decode("utf-8")
        meta = metadata(source)
        unknown = set(meta["flags"]) - {"raw", "onlyStrict", "noStrict", "module", "async", "generated"}
        unsupported = f"unsupported flags: {sorted(unknown)}" if unknown else None
        if "$262" in source or "agent" in meta["features"]:
            unsupported = "Test262 host/agent capability not implemented in narrow probe"
        for mode in variants(meta):
            includes = []
            if mode not in {"raw", "module-raw"}:
                names = ["assert.js", "sta.js"]
                if "async" in meta["flags"]:
                    names.append("doneprintHandle.js")
                names += meta["includes"]
                for name in names:
                    if Path(name).name != name:
                        raise ValueError(f"Unsafe harness name: {name}")
                    includes.append({"name": name, "source": (root / "harness" / name).read_bytes().decode("utf-8")})
            code = ('"use strict";\n' if mode == "strict" else "") + source
            negative = meta.get("negative")
            case = dict(id=f"test262/{relative}#{mode}", lane="test262", metadata=meta,
                        request=request(code, goal="module" if mode.startswith("module") else "script",
                                        action="parse" if negative and negative["phase"] == "parse" else "evaluate",
                                        includes=includes, **{"async": "async" in meta["flags"]}),
                        expected=negative, unsupported=unsupported)
            cases.append(case)
    totals.update(selected_files=len(selected), selected_variants=len(cases),
                  excluded_files=totals["files"] - len(selected))
    return cases, totals


def authored_cases(cache):
    hashes = load_manifest()["authored"]
    cases = [dict(id="authored/" + name, lane="authored", expected_value=True,
                  request=request(verified_read(FIXTURES / name, hashes[name]).decode()))
             for name in ("es5-observables.js", "modern-observables.js")]
    vue = load_manifest()["vue"]
    bundle = verified_read(cache / vue["bundle"], vue["bundle_sha256"]).decode()
    cases.append(dict(id="vue/reactivity-3.5.42", lane="vue-no-dom", expected_value=True,
                      request=request(verified_read(FIXTURES / "vue-reactivity.js", hashes["vue-reactivity.js"]).decode(),
                                      includes=[dict(name="reactivity.global.prod.js", source=bundle)],
                                      **{"async": True})))
    return cases


def probe_cases():
    return [dict(id="probe/" + name, lane="probe", expected_value=True,
                 request=request("", action="probe", probe=name))
            for name in ("isolation", "host-root-reentry", "promise-order", "supplied-module",
                         "fatal-interrupt", "gc-cycles")]


def spawn(executable, usage_path):
    """GNU time measures the Rust child after the supervisor's exec boundary.

    wait4 on a direct Python child includes the inherited corpus-inventory RSS,
    even with posix_spawn on Linux. Keep that number separately, not as engine RSS.
    """
    input_read, input_write = os.pipe()
    output_read, output_write = os.pipe()
    error_read, error_write = os.pipe()
    descriptors = (input_read, input_write, output_read, output_write, error_read, error_write)
    actions = [(os.POSIX_SPAWN_DUP2, input_read, 0), (os.POSIX_SPAWN_DUP2, output_write, 1),
               (os.POSIX_SPAWN_DUP2, error_write, 2)]
    actions += [(os.POSIX_SPAWN_CLOSE, descriptor) for descriptor in descriptors]
    try:
        arguments = ["/usr/bin/time", "-q", "-f", "%M %U %S %x", "-o", str(usage_path), str(executable)]
        pid = os.posix_spawn(arguments[0], arguments, {}, file_actions=actions, setsid=True)
    except BaseException:
        for descriptor in descriptors:
            os.close(descriptor)
        raise
    for descriptor in (input_read, output_write, error_write):
        os.close(descriptor)
    return SimpleNamespace(pid=pid, stdin=os.fdopen(input_write, "wb", buffering=0),
                           stdout=os.fdopen(output_read, "rb", buffering=0),
                           stderr=os.fdopen(error_read, "rb", buffering=0), returncode=None)


def execute(executable, payload, deadline=DEADLINE, output_cap=OUTPUT_CAP, stderr_cap=STDERR_CAP):
    """A fresh process per variant, nonblocking bounded pipes, per-child wait4 RSS."""
    encoded = json.dumps(payload, ensure_ascii=True).encode()
    if len(encoded) > REQUEST_CAP:
        return dict(classification="unsupported", reason="request admission cap")
    started = time.monotonic()
    usage_file = tempfile.NamedTemporaryFile(prefix="mg-jsplan-usage-", delete=False)
    usage_path = Path(usage_file.name)
    usage_file.close()
    try:
        process = spawn(executable, usage_path)
    except BaseException:
        usage_path.unlink()
        raise
    streams = {"stdout": bytearray(), "stderr": bytearray()}
    selector = selectors.DefaultSelector()
    for name in ("stdin", "stdout", "stderr"):
        stream = getattr(process, name)
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_WRITE if name == "stdin" else selectors.EVENT_READ, name)
    sent = 0
    forced = None
    forced_at = None
    escalated = False
    status = None
    usage = None
    def reap():
        nonlocal status, usage
        if status is None:
            pid, child_status, child_usage = os.wait4(process.pid, os.WNOHANG)
            if pid:
                status, usage = child_status, child_usage
                process.returncode = os.waitstatus_to_exitcode(status)

    def kill_owned_group():
        # This direct child's PID cannot be reused until we reap it. Never use
        # its numeric process-group identity after wait4 has consumed its status.
        if status is not None:
            return
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def stop_child():
        # Give the trusted time wrapper an opportunity to reap its sole child
        # and write metrics. The confined Rust child cannot create descendants.
        if status is not None:
            return
        children_path = Path(f"/proc/{process.pid}/task/{process.pid}/children")
        try:
            for child in children_path.read_text().split():
                try:
                    pidfd = os.pidfd_open(int(child))
                except ProcessLookupError:
                    continue
                try:
                    # Opening the pidfd pins identity; rechecking parenthood
                    # afterward excludes PID reuse between the first read/open.
                    if child in children_path.read_text().split():
                        signal.pidfd_send_signal(pidfd, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                finally:
                    os.close(pidfd)
            # A stopped trusted supervisor still needs to reap its killed child.
            os.kill(process.pid, signal.SIGCONT)
        except (FileNotFoundError, ProcessLookupError):
            pass

    def force(reason):
        nonlocal forced, forced_at
        if forced is None:
            forced, forced_at = reason, time.monotonic()
            stop_child()

    try:
        while selector.get_map() or status is None:
            now = time.monotonic()
            if now - started >= deadline:
                force("timeout")
            if forced_at is not None:
                if now - forced_at >= REAP_GRACE and not escalated:
                    kill_owned_group()
                    escalated = True
                if now - forced_at >= CLEANUP_DEADLINE:
                    break
            for key, _events in selector.select(0.01):
                name = key.data
                if name == "stdin":
                    try:
                        sent += os.write(key.fd, encoded[sent:sent + 65536])
                    except BrokenPipeError:
                        sent = len(encoded)
                    if sent == len(encoded):
                        selector.unregister(key.fileobj)
                        key.fileobj.close()
                    continue
                chunk = os.read(key.fd, 65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    key.fileobj.close()
                    continue
                cap = output_cap if name == "stdout" else stderr_cap
                if len(streams[name]) + len(chunk) > cap and forced is None:
                    force("termination")
                streams[name].extend(chunk[:max(0, cap - len(streams[name]))])
            reap()
    finally:
        selector.close()
        if status is None:
            kill_owned_group()
            cleanup_end = (forced_at or time.monotonic()) + CLEANUP_DEADLINE
            while status is None and time.monotonic() < cleanup_end:
                reap()
                if status is None:
                    time.sleep(0.01)
        for name in ("stdin", "stdout", "stderr"):
            getattr(process, name).close()
    if status is None:
        usage_path.unlink()
        return dict(classification="infrastructure-error", reason="owned supervisor did not reap within bounded cleanup",
                    wall_seconds=time.monotonic() - started, reaped=False,
                    stdout=streams["stdout"].decode(errors="replace"), stderr=streams["stderr"].decode(errors="replace"))
    result = dict(wall_seconds=time.monotonic() - started, supervisor_peak_rss_bytes=usage.ru_maxrss * 1024,
                  supervisor_user_cpu_seconds=usage.ru_utime, supervisor_system_cpu_seconds=usage.ru_stime,
                  returncode=process.returncode, stdout=streams["stdout"].decode(errors="replace"),
                  stderr=streams["stderr"].decode(errors="replace"), reaped=True, cleanup_escalated=escalated)
    try:
        values = usage_path.read_text().split()
        if len(values) != 4:
            raise ValueError("missing GNU time child metrics")
        peak_kib, user_seconds, system_seconds, child_exit = values
        child_exit = int(child_exit)
        result.update(peak_rss_bytes=int(peak_kib) * 1024, user_cpu_seconds=float(user_seconds),
                      system_cpu_seconds=float(system_seconds), measurement="GNU time: child wait4 after supervisor exec",
                      supervisor_returncode=process.returncode, measured_child_exit_status=child_exit)
        # GNU time reports %x=0 for a signaled child and returns 128+signal;
        # ordinary exit 143 instead has %x=143, preserving this distinction.
        if child_exit == 0 and 128 < process.returncode < 193:
            result["returncode"] = -(process.returncode - 128)
            result["signal"] = process.returncode - 128
    except (OSError, ValueError) as error:
        result.update(measurement_error=str(error))
        if forced is None:
            result.update(classification="infrastructure-error", reason=str(error))
    finally:
        usage_path.unlink()
    if forced:
        result.update(classification=forced, reason="parent deadline" if forced == "timeout" else "parent output cap")
    elif "classification" in result:
        return result
    elif result["returncode"] < 0:
        result.update(classification="crash", reason=f"signal {-result['returncode']}")
    elif result["returncode"]:
        result.update(classification="termination", reason="nonzero process exit")
    else:
        try:
            response = json.loads(result["stdout"])
            if not isinstance(response, dict) or response.get("protocol") != 1 or response.get("outcome") not in {"ok", "exception", "unsupported", "termination"}:
                raise ValueError("invalid response envelope")
            if response.get("phase") not in {"harness", "parse", "early", "resolution", "runtime"}:
                raise ValueError("missing or invalid response phase")
            if response["outcome"] == "ok":
                expected_phase = "parse" if payload["action"] == "parse" else "runtime"
                if response["phase"] != expected_phase:
                    raise ValueError("successful response has wrong execution phase")
            result["response"] = response
        except (ValueError, TypeError) as error:
            result.update(classification="infrastructure-error", reason=f"invalid probe JSON: {error}")
    return result


def classify(case, result):
    if "classification" in result:
        return result["classification"]
    response = result["response"]
    outcome = response["outcome"]
    if outcome in {"unsupported", "termination"}:
        return outcome
    if response.get("phase") == "harness" and outcome != "ok":
        return "harness-failure"
    expected = case.get("expected")
    if expected:
        phase = response.get("phase")
        if phase == "early":
            phase = "parse"
        if outcome == "exception" and phase == expected["phase"] and response.get("error_type") == expected["type"]:
            return "pass"
        return "assertion-failure"
    if outcome == "exception":
        return "assertion-failure" if response.get("error_type") == "Test262Error" else "exception"
    if case["request"].get("async"):
        if response.get("done") == "missing":
            result["reason"] = "missing asynchronous completion after bounded job checkpoint"
            return "timeout"
        if response.get("done") != "ok":
            return "assertion-failure"
    if "expected_value" in case and response.get("value") is not case["expected_value"]:
        return "assertion-failure"
    return "pass"


def environment(executable):
    def command(*args):
        try:
            return subprocess.check_output(args, cwd=ROOT, text=True, stderr=subprocess.STDOUT).strip()
        except (OSError, subprocess.CalledProcessError):
            return "unavailable"
    return dict(platform=platform.platform(), machine=platform.machine(),
                cpu=next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                          if line.startswith("model name")), "unknown"), python=platform.python_version(),
                commit=command("git", "rev-parse", "HEAD"), worktree=command("git", "status", "--porcelain"),
                rustc=command("rustc", "--version"), executable=str(executable),
                executable_sha256=sha256(executable.read_bytes()),
                thermal_conditions="uncontrolled; correctness probe, not performance comparison")


def run(args):
    executable = args.executable.resolve()
    if platform.system() != "Linux":
        raise ValueError("This runner's wait4 RSS units and confinement contract require Linux")
    if not hasattr(os, "pidfd_open") or not hasattr(signal, "pidfd_send_signal"):
        raise ValueError("Safe child cleanup requires Python/Linux pidfd support")
    if baseline_fingerprint() != load_manifest()["baseline_source_sha256"]:
        raise ValueError("Original Butane source changed; declare and review a new baseline")
    cases, denominator = test262_cases(args.cache)
    cases = probe_cases() + authored_cases(args.cache) + cases
    if args.lane:
        cases = [case for case in cases if case["lane"] in args.lane]
    report = dict(schema_version=1, started_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                  purpose="P0/P1 selected correctness baseline; not full Test262 or browser framework support",
                  pins=load_manifest(), manifest_sha256=sha256(MANIFEST.read_bytes()), environment=environment(executable),
                  limits=dict(wall_seconds=DEADLINE, request_bytes=REQUEST_CAP, stdout_bytes=OUTPUT_CAP, stderr_bytes=STDERR_CAP),
                  denominator=denominator, results=[], summary={}, complete=False,
                  selected_lanes=args.lane, engines=args.engine, expected_result_count=len(cases) * len(args.engine),
                  declared_build_profile=args.build_profile, baseline_source_sha256=baseline_fingerprint())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for engine in args.engine:
        for case in cases:
            payload = dict(case["request"], engine=engine)
            if case.get("unsupported"):
                result = dict(classification="unsupported", reason=case["unsupported"])
            else:
                result = execute(executable, payload)
                result["classification"] = classify(case, result)
            result.update(id=case["id"], lane=case["lane"], engine=engine,
                          input_sha256=sha256(json.dumps(payload, sort_keys=True).encode()))
            report["results"].append(result)
            # Partial reports remain useful when a human interrupts a long run.
            report["summary"] = {name: dict(collections.Counter(row["classification"] for row in report["results"] if row["engine"] == name))
                                 for name in args.engine}
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(f"{engine} {result['classification']}: {case['id']}", flush=True)
    print(json.dumps(report["summary"], sort_keys=True))
    report["complete"] = True
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    return 0 if not any(row["classification"] == "infrastructure-error" for row in report["results"]) else 2


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(dest="command", required=True)
    setup = subs.add_parser("fetch", help="explicitly download and verify pinned external fixtures")
    setup.add_argument("--cache", type=Path, default=ROOT / "tmp/jsplan-inputs")
    probe = subs.add_parser("run", help="record honest results, including unsupported/failure outcomes")
    probe.add_argument("--cache", type=Path, default=ROOT / "tmp/jsplan-inputs")
    probe.add_argument("--executable", type=Path, required=True)
    probe.add_argument("--engine", choices=("baseline", "boa"), action="append", required=True)
    probe.add_argument("--lane", choices=("probe", "authored", "vue-no-dom", "test262"), action="append")
    probe.add_argument("--output", type=Path, required=True)
    probe.add_argument("--build-profile", choices=("release", "debug", "unspecified"), default="unspecified",
                       help="record the caller's build declaration; executable hash remains the exact identity")
    args = parser.parse_args()
    try:
        if args.command == "fetch":
            fetch(args.cache)
            return 0
        return run(args)
    except (OSError, ValueError) as error:
        parser.exit(2, f"JSPLAN runner: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
