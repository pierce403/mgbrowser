"""Owned synthetic process/artifact failures; no browser, network or page JS."""
import importlib.util
import errno
import os
from pathlib import Path
import signal
import stat
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("mg_wpt_process_control", Path(__file__).with_name("process_control.py"))
process_control = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(process_control)


@unittest.skipUnless(os.name == "posix" and hasattr(os, "WNOWAIT"), "requires POSIX waitid/WNOWAIT")
class ProcessControlTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mg-wpt-owned-process-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def run_owned(self, source, **options):
        return process_control.run_bounded([sys.executable, "-c", source], cwd=self.root,
                                           env={"PATH": os.environ.get("PATH", "/usr/bin:/bin")},
                                           log_path=self.root / "page.log", **options)

    def assert_descendant_terminated(self, pid):
        deadline = time.monotonic() + 1.0
        while time.monotonic() < deadline:
            try:
                state = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[0]
            except (FileNotFoundError, ProcessLookupError):
                return "reaped"
            if state == "Z":
                # Grandchildren are not ours to waitpid(). Their Z state proves
                # termination, not successful reaping by this utility.
                return "terminated-zombie-awaiting-parent"
            time.sleep(0.005)
        self.fail(f"Owned descendant {pid} is still running")

    def test_descendant_observation_accepts_only_process_disappearance(self):
        for error in (FileNotFoundError(), ProcessLookupError()):
            with self.subTest(error=type(error).__name__), \
                    patch.object(Path, "read_text", side_effect=error):
                self.assertEqual(self.assert_descendant_terminated(123456), "reaped")
        for error in (PermissionError(errno.EACCES, "Synthetic denied observation"),
                      OSError(errno.EIO, "Synthetic observation I/O failure")):
            with self.subTest(error=type(error).__name__), \
                    patch.object(Path, "read_text", side_effect=error):
                with self.assertRaises(type(error)):
                    self.assert_descendant_terminated(123456)

    def cancel_when_created(self, path, cancel_event):
        """Cancel only after the owned fixture has admitted its PID record."""
        observed = threading.Event()

        def cancel():
            deadline = time.monotonic() + 2
            while time.monotonic() < deadline:
                if path.exists() and path.read_text().strip():
                    observed.set()
                    break
                time.sleep(0.005)
            cancel_event.set()

        thread = threading.Thread(target=cancel)
        thread.start()
        self.addCleanup(thread.join, 3)
        return thread, observed

    def test_success_keeps_combined_output_and_exit_status(self):
        result = self.run_owned("import os; os.write(1,b'out\\n'); os.write(2,b'err\\n')")
        self.assertEqual(result["status"], "OK")
        self.assertEqual(result["returncode"], 0)
        self.assertIs(result["completed"], True)
        self.assertIs(result["cleanup_succeeded"], True)
        self.assertEqual(result["output_bytes"], 8)
        self.assertEqual((self.root / "page.log").read_bytes(), b"out\nerr\n")

    def test_unset_cancellation_keeps_ordinary_completion(self):
        result = self.run_owned("print('ordinary')", cancel_event=threading.Event())
        self.assertEqual(result["status"], "OK")
        self.assertEqual(result["returncode"], 0)
        self.assertIs(result["completed"], True)
        self.assertIs(result["cleanup_succeeded"], True)
        self.assertEqual((self.root / "page.log").read_bytes(), b"ordinary\n")

    def test_cancellation_before_admission_creates_no_log_or_child(self):
        cancel_event = threading.Event()
        cancel_event.set()
        with patch.object(process_control.subprocess, "Popen", side_effect=AssertionError("must not launch")):
            result = self.run_owned("raise SystemExit(0)", cancel_event=cancel_event,
                                    stdout_path=self.root / "stdout.log")
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["reason"], "Process cancelled before launch")
        self.assertIsNone(result["returncode"])
        self.assertIs(result["completed"], False)
        self.assertIs(result["cleanup_succeeded"], True)
        self.assertEqual(result["output_bytes"], 0)
        self.assertFalse((self.root / "page.log").exists())
        self.assertFalse((self.root / "stdout.log").exists())

    def test_cancellation_during_log_admission_still_prevents_launch(self):
        cancel_event = threading.Event()
        real_open = open

        def open_and_cancel(*args, **kwargs):
            log = real_open(*args, **kwargs)
            cancel_event.set()
            return log

        with patch.object(process_control, "open", open_and_cancel, create=True), \
                patch.object(process_control.subprocess, "Popen", side_effect=AssertionError("must not launch")):
            result = self.run_owned("raise SystemExit(0)", cancel_event=cancel_event)
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["reason"], "Process cancelled before launch")
        self.assertIsNone(result["returncode"])
        self.assertIs(result["completed"], False)
        self.assertEqual((self.root / "page.log").read_bytes(), b"")

    def test_active_cancellation_kills_owned_group_before_reaping_leader(self):
        cancel_event = threading.Event()
        thread, observed = self.cancel_when_created(self.root / "leader.pid", cancel_event)
        operations = []
        real_killpg = process_control.os.killpg
        real_wait = process_control.subprocess.Popen.wait

        def kill_group(pid, sig):
            operations.append(("kill", pid, sig))
            return real_killpg(pid, sig)

        def wait_leader(process, *args, **kwargs):
            self.assertIn(("kill", process.pid, signal.SIGKILL), operations)
            operations.append(("wait", process.pid))
            return real_wait(process, *args, **kwargs)

        with patch.object(process_control.os, "killpg", kill_group), \
                patch.object(process_control.subprocess.Popen, "wait", wait_leader):
            result = self.run_owned("import os,time; open('leader.pid','w').write(str(os.getpid())); time.sleep(30)",
                                    cancel_event=cancel_event, timeout=3)
        thread.join(3)
        self.assertFalse(thread.is_alive())
        self.assertTrue(observed.is_set(), "Owned leader never became ready")
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["reason"], "Process cancelled")
        self.assertEqual(result["returncode"], -signal.SIGKILL)
        self.assertIs(result["completed"], False)
        self.assertIs(result["cleanup_succeeded"], True)
        self.assertLess(result["elapsed_ms"], 1500)
        pid = int((self.root / "leader.pid").read_text())
        self.assertEqual(operations, [("kill", pid, signal.SIGKILL), ("wait", pid)])
        with self.assertRaises(ChildProcessError):
            os.waitpid(pid, os.WNOHANG)
        with self.assertRaises(ProcessLookupError):
            real_killpg(pid, 0)

    def test_active_cancellation_terminates_owned_descendant(self):
        cancel_event = threading.Event()
        thread, observed = self.cancel_when_created(self.root / "descendant.pid", cancel_event)
        result = self.run_owned("import os,subprocess,sys,time; open('leader.pid','w').write(str(os.getpid())); p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); open('descendant.pid','w').write(str(p.pid)); time.sleep(30)",
                                cancel_event=cancel_event, timeout=3)
        thread.join(3)
        self.assertFalse(thread.is_alive())
        self.assertTrue(observed.is_set(), "Owned descendant never became ready")
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["reason"], "Process cancelled")
        self.assertEqual(result["returncode"], -signal.SIGKILL)
        self.assertIs(result["completed"], False)
        self.assertIs(result["cleanup_succeeded"], True)
        self.assertLess(result["elapsed_ms"], 1500)
        with self.assertRaises(ChildProcessError):
            os.waitpid(int((self.root / "leader.pid").read_text()), os.WNOHANG)
        self.assert_descendant_terminated(int((self.root / "descendant.pid").read_text()))

    def test_cancellation_during_cleanup_is_not_normal_nonzero_completion(self):
        cancel_event = threading.Event()
        real_stop_group = process_control._stop_group

        def stop_and_cancel(process):
            returncode = real_stop_group(process)
            cancel_event.set()
            return returncode

        with patch.object(process_control, "_stop_group", stop_and_cancel):
            result = self.run_owned("raise SystemExit(2)", cancel_event=cancel_event)
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["reason"], "Process cancelled")
        self.assertEqual(result["returncode"], 2)
        self.assertIs(result["completed"], False)
        self.assertIs(result["cleanup_succeeded"], True)

    def test_cleanup_failure_is_not_confirmed_by_attempted_cleanup(self):
        real_stop_group = process_control._stop_group
        owned = []

        def stop_without_confirmation(process):
            owned.append(process)
            # Terminate/reap this synthetic fixture so the negative control
            # itself cannot leak a process. Then simulate the helper failing to
            # return confirmation, which must invalidate future admission.
            real_stop_group(process)
            raise OSError("Synthetic owned-cleanup confirmation failure")

        with patch.object(process_control, "_stop_group", stop_without_confirmation):
            result = self.run_owned("import time; time.sleep(30)", timeout=0.1)
        self.assertEqual(result["status"], "ERROR")
        self.assertIn("cleanup", result["reason"])
        self.assertIs(result["completed"], False)
        self.assertIs(result["cleanup_succeeded"], False)
        self.assertEqual(len(owned), 1)
        with self.assertRaises(ChildProcessError):
            os.waitpid(owned[0].pid, os.WNOHANG)
        with self.assertRaises(ProcessLookupError):
            os.killpg(owned[0].pid, 0)

    def test_split_cargo_stdout_and_stderr_share_output_limit(self):
        result = self.run_owned("import os; os.write(1,b'{\"artifact\":1}\\n'); os.write(2,b'building\\n')",
                                stdout_path=self.root / "build.jsonl", max_output_bytes=24)
        self.assertEqual(result["status"], "OK")
        self.assertEqual((self.root / "build.jsonl").read_bytes(), b'{"artifact":1}\n')
        self.assertEqual((self.root / "page.log").read_bytes(), b"building\n")

    def test_launch_failure_is_error_and_creates_no_child(self):
        result = process_control.run_bounded([str(self.root / "absent")], cwd=self.root,
                                             env={}, log_path=self.root / "page.log")
        self.assertEqual(result["status"], "ERROR")
        self.assertIsNone(result["returncode"])
        self.assertIs(result["completed"], False)
        self.assertIs(result["cleanup_succeeded"], True)

    def test_unexpected_nonzero_exit_is_error(self):
        result = self.run_owned("raise SystemExit(2)")
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["returncode"], 2)
        self.assertIs(result["completed"], True)

    def test_independent_signal_preserves_crash_status(self):
        result = self.run_owned("import os, signal; os.kill(os.getpid(), signal.SIGTERM)")
        self.assertEqual(result["status"], "CRASH")
        self.assertEqual(result["returncode"], -signal.SIGTERM)
        self.assertIs(result["completed"], False)

    def test_local_timeout_kills_group_and_reaps_leader(self):
        result = self.run_owned("import os,time; open('leader.pid','w').write(str(os.getpid())); time.sleep(30)", timeout=0.15)
        self.assertEqual(result["status"], "TIMEOUT")
        self.assertEqual(result["returncode"], -signal.SIGKILL)
        self.assertIs(result["completed"], False)
        self.assertLess(result["elapsed_ms"], 1500)
        pid = int((self.root / "leader.pid").read_text())
        with self.assertRaises(ChildProcessError):
            os.waitpid(pid, os.WNOHANG)

    def test_global_deadline_only_shortens_page_deadline(self):
        result = self.run_owned("import time; time.sleep(30)", timeout=45,
                                global_deadline=time.monotonic() + 0.1)
        self.assertEqual(result["status"], "TIMEOUT")
        self.assertLess(result["elapsed_ms"], 1500)

    def test_expired_global_deadline_does_not_launch_command(self):
        result = self.run_owned("open('must-not-exist','w').write('launched')",
                                global_deadline=time.monotonic() - 1)
        self.assertEqual(result["status"], "TIMEOUT")
        self.assertIsNone(result["returncode"])
        self.assertFalse((self.root / "must-not-exist").exists())

    def test_log_overflow_is_error_after_kill_not_a_crash(self):
        result = self.run_owned("import os,time; os.write(1,b'x'*8192); time.sleep(30)",
                                max_output_bytes=1024)
        self.assertEqual(result["status"], "ERROR")
        self.assertIn("output exceeds 1024", result["reason"])
        self.assertEqual(result["returncode"], -signal.SIGKILL)
        self.assertEqual((self.root / "page.log").stat().st_size, 1024)
        self.assertIs(result["completed"], False)
        self.assertIs(result["cleanup_succeeded"], True)

    def test_output_overflow_after_leader_exit_two_is_not_normal_completion(self):
        real_exited = process_control._leader_exited

        def wait_for_fixture_exit(process):
            # A small payload fits the pipe before capture. Make the completed
            # leader observable first, so its exit2 cannot mask log overflow.
            deadline = time.monotonic() + 1
            while time.monotonic() < deadline:
                if real_exited(process):
                    return True
                time.sleep(0.005)
            self.fail("Owned fixture did not exit")

        with patch.object(process_control, "_leader_exited", wait_for_fixture_exit):
            result = self.run_owned("import os; os.write(1,b'x'*128); os._exit(2)",
                                    max_output_bytes=16)
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual(result["returncode"], 2)
        self.assertIn("output exceeds 16", result["reason"])
        self.assertIs(result["completed"], False)
        self.assertEqual((self.root / "page.log").stat().st_size, 16)

    def test_artifact_overflow_with_exit_two_is_not_normal_completion(self):
        artifact = self.root / "large.json"
        result = self.run_owned("import os; open('large.json','wb').write(b'x'*128); os._exit(2)",
                                artifact_limits={artifact: 16})
        self.assertEqual(result["status"], "ERROR")
        self.assertIn("Artifact exceeds 16", result["reason"])
        self.assertIs(result["completed"], False)

    def test_timeout_terminates_descendants_sharing_the_owned_group(self):
        result = self.run_owned("import subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); open('descendant.pid','w').write(str(p.pid)); time.sleep(30)", timeout=0.2)
        self.assertEqual(result["status"], "TIMEOUT")
        self.assert_descendant_terminated(int((self.root / "descendant.pid").read_text()))

    def test_output_overflow_also_terminates_owned_descendants(self):
        result = self.run_owned("import os,subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); open('descendant.pid','w').write(str(p.pid)); os.write(1,b'x'*8192); time.sleep(30)", max_output_bytes=256)
        self.assertEqual(result["status"], "ERROR")
        self.assertIn("output exceeds 256", result["reason"])
        self.assertEqual((self.root / "page.log").stat().st_size, 256)
        self.assert_descendant_terminated(int((self.root / "descendant.pid").read_text()))

    def test_successful_leader_cannot_leave_a_pipe_holding_descendant(self):
        result = self.run_owned("import subprocess,sys; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); open('descendant.pid','w').write(str(p.pid))", timeout=1)
        self.assertEqual(result["status"], "OK", result)
        self.assertLess(result["elapsed_ms"], 900)
        self.assert_descendant_terminated(int((self.root / "descendant.pid").read_text()))

    def test_detected_artifact_overflow_terminates_producer(self):
        artifact = self.root / "large.json"
        result = self.run_owned("import time; open('large.json','wb').write(b'x'*4096); time.sleep(30)",
                                artifact_limits={artifact: 1024})
        self.assertEqual(result["status"], "ERROR")
        self.assertIn("Artifact exceeds 1024", result["reason"])
        self.assertEqual(result["returncode"], -signal.SIGKILL)

    def test_existing_log_cannot_be_reused_or_overwritten(self):
        (self.root / "page.log").write_bytes(b"original")
        result = self.run_owned("raise SystemExit(0)")
        self.assertEqual(result["status"], "ERROR")
        self.assertEqual((self.root / "page.log").read_bytes(), b"original")


class ArtifactAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mg-wpt-artifact-admission-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_regular_admitted_file_is_read_exactly(self):
        path = self.root / "metadata.json"
        path.write_bytes(b"{\"ok\":true}")
        self.assertEqual(process_control.read_artifact(path, limit=100), b'{"ok":true}')

    def test_metadata_png_and_rgb_limits_are_checked_before_any_read(self):
        for name, limit, exact in [("metadata.json", process_control.METADATA_LIMIT, None),
                                   ("page.png", process_control.PNG_LIMIT, None),
                                   ("page.rgb", process_control.RGB_BYTES, process_control.RGB_BYTES)]:
            with self.subTest(name=name):
                path = self.root / name
                with path.open("wb") as output:
                    output.truncate(limit + 1)
                with patch.object(process_control.os, "read", side_effect=AssertionError("must reject before body read")):
                    with self.assertRaisesRegex(process_control.ArtifactError, "exceeds"):
                        process_control.read_artifact(path, limit=limit, exact=exact)

    def test_rgb_requires_exact_1440000_bytes_before_read(self):
        self.assertEqual(process_control.RGB_BYTES, 1_440_000)
        path = self.root / "page.rgb"
        path.write_bytes(b"short")
        with patch.object(process_control.os, "read", side_effect=AssertionError("must reject before body read")):
            with self.assertRaisesRegex(process_control.ArtifactError, "exactly 1440000"):
                process_control.read_artifact(path, limit=process_control.RGB_BYTES, exact=process_control.RGB_BYTES)

    def test_symlinks_and_nonregular_files_are_rejected(self):
        regular = self.root / "regular"
        regular.write_bytes(b"data")
        link = self.root / "link"
        link.symlink_to(regular)
        fifo = self.root / "fifo"
        os.mkfifo(fifo)
        for path in (link, fifo, self.root):
            with self.subTest(path=path), self.assertRaisesRegex(process_control.ArtifactError, "not a regular"):
                process_control.read_artifact(path, limit=100)

    def test_opened_file_identity_is_checked_before_read(self):
        path = self.root / "metadata"
        path.write_bytes(b"data")
        with patch.object(process_control.os, "fstat", return_value=type("Changed", (), {"st_mode": stat.S_IFREG, "st_dev": -1, "st_ino": -1})()):
            with patch.object(process_control.os, "read", side_effect=AssertionError("must reject before body read")):
                with self.assertRaisesRegex(process_control.ArtifactError, "changed before"):
                    process_control.read_artifact(path, limit=100)


if __name__ == "__main__":
    unittest.main()
