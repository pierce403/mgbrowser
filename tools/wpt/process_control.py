"""Bound owned WPT tool processes and validate artifacts before reading them.

These are runner controls, not changes to browser/worker resource policy or an
OS disk quota. Artifact polling detects over-limit files; read_artifact rejects
them before reading. Process groups contain the reviewed commands and their
ordinary descendants, not programs deliberately escaping into another session.
Only the direct leader is reaped here. Terminated grandchildren may remain as
zombies until their actual parent or the operating system's reaper collects them.
"""
import math
import os
from pathlib import Path
import selectors
import signal
import stat
import subprocess
import time


LOG_LIMIT = 1024 * 1024
METADATA_LIMIT = 1024 * 1024
PNG_LIMIT = 8 * 1024 * 1024
RGB_BYTES = 800 * 600 * 3
POLL_INTERVAL = 0.025
CLEANUP_TIMEOUT = 2.0


class ArtifactError(ValueError):
    """An artifact is missing, unsafe, changed, or outside its byte admission."""


class _ProcessCancelled(Exception):
    """The owning runner requested cancellation, not a child process crash."""


def _artifact_stat(path, limit, exact=None):
    if type(limit) is not int or limit < 0:
        raise ArtifactError("Artifact byte limit must be a nonnegative integer")
    if exact is not None and (type(exact) is not int or not 0 <= exact <= limit):
        raise ArtifactError("Artifact exact length must fit its byte limit")
    try:
        value = Path(path).lstat()
    except OSError as error:
        raise ArtifactError(f"Cannot inspect artifact {path}: {error}") from error
    if not stat.S_ISREG(value.st_mode):
        raise ArtifactError(f"Artifact is not a regular nonsymlink file: {path}")
    if value.st_size > limit:
        raise ArtifactError(f"Artifact exceeds {limit} byte limit: {path}")
    if exact is not None and value.st_size != exact:
        raise ArtifactError(f"Artifact has {value.st_size} bytes; expected exactly {exact}: {path}")
    return value


def read_artifact(path, *, limit, exact=None):
    """Read only a regular nonsymlink artifact admitted by size first.

    The descriptor is checked again after opening and after the bounded read.
    An oversized input is rejected before any body read or proportional buffer
    allocation. Call after the owned producer has been terminated and reaped.
    """
    before = _artifact_stat(path, limit, exact)
    flags = os.O_RDONLY | os.O_NONBLOCK | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise ArtifactError(f"Cannot open artifact {path}: {error}") from error
    try:
        opened = os.fstat(descriptor)
        if (not stat.S_ISREG(opened.st_mode)
                or (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino)
                or opened.st_size != before.st_size):
            raise ArtifactError(f"Artifact changed before reading: {path}")
        chunks = []
        received = 0
        while received <= before.st_size:
            chunk = os.read(descriptor, min(64 * 1024, before.st_size + 1 - received))
            if not chunk:
                break
            chunks.append(chunk)
            received += len(chunk)
        after = os.fstat(descriptor)
        if (received != before.st_size or after.st_size != before.st_size
                or after.st_mtime_ns != opened.st_mtime_ns):
            raise ArtifactError(f"Artifact changed while reading: {path}")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def _leader_exited(process):
    # WNOWAIT keeps the leader PID reserved until group cleanup. Calling poll()
    # first would reap it and introduce a PID/group-reuse race before killpg().
    return os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None


def _stop_group(process):
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    return process.wait(timeout=CLEANUP_TIMEOUT)


def run_bounded(command, *, cwd, env, log_path, stdout_path=None,
                timeout=45.0, global_deadline=None, max_output_bytes=LOG_LIMIT,
                artifact_limits=None, cancel_event=None):
    """Run one reviewed command without a shell in an owned process group.

    timeout is a local duration; global_deadline is an absolute monotonic
    deadline and can only shorten it. The default is the unchanged 45-second
    page deadline. stdout_path splits Cargo JSONL from stderr log_path; otherwise
    both streams are merged. max_output_bytes bounds their combined bytes.
    Optional cancel_event has threading.Event-style is_set(). Cancellation is
    checked before log/process admission and at each bounded polling iteration;
    it returns ERROR/completed=False after the same owned-group cleanup.

    Returns status OK/ERROR/TIMEOUT/CRASH, reason, returncode, elapsed_ms,
    output_bytes, completed and cleanup_succeeded. completed is true only for an ordinary leader
    exit after successful output/artifact admission and cleanup. It lets callers
    distinguish a structured nonzero exit from an admission error whose leader
    happened to exit with the same code. TIMEOUT/output-limit errors retain their cause after cleanup;
    a command killed by a signal on its own is CRASH. The complete owned group
    is killed before reaping the leader even after an ordinary successful exit.
    cleanup_succeeded is true only if no process was launched or owned-group
    termination and leader wait returned successfully. A cleanup attempt alone
    is not confirmation; callers must stop new admissions when it is false.
    """
    began = time.monotonic()
    process = None
    cleaned = False
    cleanup_succeeded = True
    completed = False
    output_bytes = 0
    status, reason, returncode = "ERROR", "Process was not started", None
    selector = selectors.DefaultSelector()
    streams = []
    logs = []

    def check_cancellation():
        if cancel_event is not None and cancel_event.is_set():
            raise _ProcessCancelled("Process cancelled before launch" if process is None
                                    else "Process cancelled")

    try:
        if (isinstance(command, (str, bytes)) or not command
                or not math.isfinite(timeout) or timeout <= 0
                or (global_deadline is not None and not math.isfinite(global_deadline))
                or type(max_output_bytes) is not int or max_output_bytes < 0
                or (cancel_event is not None
                    and not callable(getattr(cancel_event, "is_set", None)))):
            raise ValueError("Invalid command, deadline or output admission")
        if not hasattr(os, "WNOWAIT"):
            raise ValueError("Owned process-group cleanup requires waitid/WNOWAIT")
        deadline = min(began + timeout, global_deadline) if global_deadline is not None else began + timeout
        check_cancellation()
        # Exclusive creation prevents stale log reuse and symlink following.
        log = open(log_path, "xb")
        logs.append(log)
        separate_stdout = stdout_path is not None
        if separate_stdout:
            if Path(stdout_path).resolve() == Path(log_path).resolve():
                raise ValueError("stdout_path must differ from log_path")
            output = open(stdout_path, "xb")
            logs.append(output)
        else:
            output = log
        if time.monotonic() >= deadline:
            status, reason = "TIMEOUT", "Process deadline expired before launch"
        else:
            check_cancellation()
            process = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE if separate_stdout else subprocess.STDOUT,
                                       start_new_session=True, close_fds=True)
            cleanup_succeeded = False
            for stream, destination in [(process.stdout, output), (process.stderr, log)]:
                if stream is not None:
                    streams.append(stream)
                    os.set_blocking(stream.fileno(), False)
                    selector.register(stream, selectors.EVENT_READ, destination)
            status, reason = "OK", ""
            while True:
                check_cancellation()
                now = time.monotonic()
                if now >= deadline:
                    status, reason = "TIMEOUT", "Process exceeded its deadline"
                    break
                for path, limit in (artifact_limits or {}).items():
                    if Path(path).exists() or Path(path).is_symlink():
                        _artifact_stat(path, limit)
                if not cleaned and _leader_exited(process):
                    # Descendants holding output pipes must not outlive a
                    # successful leader or turn its completed run into a hang.
                    cleaned = True
                    returncode = _stop_group(process)
                    cleanup_succeeded = True
                if cleaned and not selector.get_map():
                    break
                for key, _ in selector.select(min(POLL_INTERVAL, deadline - now)):
                    chunk = os.read(key.fd, min(64 * 1024, max_output_bytes - output_bytes + 1))
                    if not chunk:
                        selector.unregister(key.fileobj)
                        key.fileobj.close()
                        continue
                    remaining = max_output_bytes - output_bytes
                    if len(chunk) > remaining:
                        # Persist only admitted bytes, never an over-limit log.
                        key.data.write(chunk[:remaining])
                        output_bytes += remaining
                        status, reason = "ERROR", f"Process output exceeds {max_output_bytes} byte limit"
                        break
                    key.data.write(chunk)
                    output_bytes += len(chunk)
                if status != "OK":
                    break
            if not cleaned:
                cleaned = True
                returncode = _stop_group(process)
                cleanup_succeeded = True
            if status == "OK":
                check_cancellation()
                completed = returncode is not None and returncode >= 0
                if returncode != 0:
                    status = "CRASH" if returncode < 0 else "ERROR"
                    reason = f"Process exit {returncode}"
    except _ProcessCancelled as error:
        completed = False
        status, reason = "ERROR", str(error)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        completed = False
        status, reason = "ERROR", str(error)
    finally:
        if process is not None and not cleaned:
            try:
                cleaned = True
                returncode = _stop_group(process)
                cleanup_succeeded = True
            except (OSError, subprocess.SubprocessError) as error:
                completed = False
                status, reason = "ERROR", f"Owned process cleanup failed: {error}"
        selector.close()
        for stream in streams:
            if not stream.closed:
                stream.close()
        for log in logs:
            try:
                log.close()
            except OSError as error:
                completed = False
                status, reason = "ERROR", f"Cannot finish process log: {error}"
    return {"status": status, "reason": reason, "returncode": returncode,
            "elapsed_ms": round((time.monotonic() - began) * 1000, 2),
            "output_bytes": output_bytes, "completed": completed,
            "cleanup_succeeded": cleanup_succeeded}
