"""Independent negative controls for bounded parallel WPT orchestration.

Only owned synthetic processes and temporary evidence are used. No Browser,
Cargo build, upstream page, score, expectation or corpus mutation is involved.
Run with: python3 -B -m unittest discover -s tools/wpt -p test_parallel_review.py
"""
from contextlib import redirect_stderr
from http.client import IncompleteRead
import io
import os
from pathlib import Path
import signal
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

import process_control
import run as runner
from parallel import RequestLedger


@unittest.skipUnless(os.name == "posix" and hasattr(os, "WNOWAIT"),
                     "requires POSIX waitid/WNOWAIT")
class IndependentOwnerCancellationTests(unittest.TestCase):
    def assert_terminated(self, pid):
        """A zombie descendant is terminated, not reaped by this test."""
        deadline = time.monotonic() + 1
        while time.monotonic() < deadline:
            try:
                state = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[0]
            except (FileNotFoundError, ProcessLookupError):
                return
            if state == "Z":
                return
            time.sleep(0.005)
        self.fail(f"Owned synthetic process {pid} remained live")

    def test_shared_cancellation_reaches_both_active_owners_only(self):
        with patch.object(Path, "read_text", side_effect=ProcessLookupError(3, "process vanished")):
            self.assert_terminated(1)
        with patch.object(Path, "read_text", side_effect=PermissionError(13, "not observable")):
            with self.assertRaises(PermissionError):
                self.assert_terminated(1)
        source = (
            "import os,subprocess,sys,time; "
            "child=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)']); "
            "open(sys.argv[1],'w').write(str(os.getpid())+' '+str(child.pid)); "
            "time.sleep(30)"
        )
        cancelled = threading.Event()
        results = {}
        failures = []
        threads = []
        sentinel = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(30)"],
                                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                    stderr=subprocess.DEVNULL, start_new_session=True)
        try:
            with tempfile.TemporaryDirectory(prefix="mg-wpt-independent-owners-") as directory:
                root = Path(directory)

                def owner(index):
                    try:
                        results[index] = process_control.run_bounded(
                            [sys.executable, "-c", source, str(root / f"owner{index}.pid")],
                            cwd=root, env={"PATH": os.environ.get("PATH", "/usr/bin:/bin")},
                            log_path=root / f"owner{index}.log", timeout=5,
                            cancel_event=cancelled)
                    except BaseException as error:
                        failures.append(error)

                for index in range(2):
                    thread = threading.Thread(target=owner, args=(index,))
                    threads.append(thread)
                    thread.start()
                ready_deadline = time.monotonic() + 2
                identities = {}
                while time.monotonic() < ready_deadline:
                    for index in range(2):
                        path = root / f"owner{index}.pid"
                        if path.exists():
                            words = path.read_text().split()
                            if len(words) == 2:
                                identities[index] = tuple(map(int, words))
                    if len(identities) == 2 or failures:
                        break
                    time.sleep(0.005)
                cancelled.set()
                for thread in threads:
                    thread.join(timeout=2)
                self.assertFalse(failures, failures)
                self.assertEqual(len(identities), 2, "Both owners must be active before cancellation")
                self.assertTrue(all(not thread.is_alive() for thread in threads))
                self.assertEqual(set(results), {0, 1})
                self.assertNotEqual(identities[0][0], identities[1][0])
                for index, (leader, child) in identities.items():
                    self.assertEqual(results[index]["status"], "ERROR", results[index])
                    self.assertIn("cancelled", results[index]["reason"].lower())
                    self.assertIs(results[index]["completed"], False)
                    self.assertIs(results[index]["cleanup_succeeded"], True)
                    self.assertEqual(results[index]["returncode"], -signal.SIGKILL)
                    with self.assertRaises(ChildProcessError):
                        os.waitpid(leader, os.WNOHANG)
                    self.assert_terminated(child)
                self.assertIsNone(sentinel.poll(), "Cancellation touched an unrelated process group")
        finally:
            cancelled.set()
            for thread in threads:
                thread.join(timeout=6)
            if sentinel.poll() is None:
                os.killpg(sentinel.pid, signal.SIGKILL)
            sentinel.wait(timeout=2)

    def test_pre_cancelled_owner_creates_neither_log_nor_process(self):
        cancelled = threading.Event()
        cancelled.set()
        with tempfile.TemporaryDirectory(prefix="mg-wpt-independent-cancelled-") as directory:
            root = Path(directory)
            result = process_control.run_bounded(
                [sys.executable, "-c", "open('launched','w').write('bad')"],
                cwd=root, env={}, log_path=root / "page.log", cancel_event=cancelled)
            self.assertEqual(result["status"], "ERROR")
            self.assertIsNone(result["returncode"])
            self.assertIs(result["completed"], False)
            self.assertIs(result["cleanup_succeeded"], True)
            self.assertEqual(list(root.iterdir()), [])


class IndependentRequestAttributionTests(unittest.TestCase):
    def test_prebind_request_cannot_be_relabelled_as_another_page(self):
        ledger = RequestLedger(limit=10, max_origins=2)
        audit = runner.RequestAudit(require_owner=True)
        self.assertFalse(audit.add({"path": "/a.html", "status": 200}))
        with self.assertRaises(ValueError):
            audit.bind(ledger, "b.html", 1)
        with self.assertRaises(ValueError):
            audit.validate()
        self.assertEqual(ledger.snapshot(), [])
        self.assertEqual(audit.snapshot(), [{"path": "/a.html", "status": 503}])

    def test_fixture_origins_cannot_be_reused_or_exceed_inventory(self):
        ledger = RequestLedger(limit=10, max_origins=1)
        self.assertTrue(ledger.claim_origin("http://127.0.0.1:12345/"))
        self.assertFalse(ledger.claim_origin("http://127.0.0.1:12345/"))
        with self.assertRaises(ValueError):
            ledger.claim_origin("http://127.0.0.1:12346/")

    def test_request_snapshots_do_not_change_after_late_request(self):
        ledger = RequestLedger(limit=10, max_origins=2)
        audit = runner.RequestAudit(require_owner=True)
        audit.bind(ledger, "a.html", 0)
        self.assertTrue(audit.add({"path": "/a.html", "status": 200}))
        local_before = audit.snapshot()
        global_before = ledger.snapshot()
        audit.close_admission()
        self.assertFalse(audit.add({"path": "/late.css", "status": 200}))
        self.assertEqual(local_before, [{"path": "/a.html", "status": 200}])
        self.assertEqual(global_before, [{"page": "a.html", "lane": 0,
                                          "path": "/a.html", "status": 200}])
        self.assertEqual(ledger.snapshot()[-1]["page"], "a.html")
        self.assertEqual(ledger.snapshot()[-1]["status"], 503)
        with self.assertRaises(ValueError):
            ledger.validate()

    def test_both_lanes_share_one_atomic_request_cap(self):
        ledger = RequestLedger(limit=3, max_origins=2)
        barrier = threading.Barrier(2)
        accepted = []
        lock = threading.Lock()

        def lane(number):
            barrier.wait(timeout=1)
            for index in range(8):
                value = ledger.add(f"page{number}.html", number,
                                   {"path": f"/resource{index}.css", "status": 200})
                with lock:
                    accepted.append(value)

        threads = [threading.Thread(target=lane, args=(number,)) for number in range(2)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(timeout=2)
        self.assertTrue(all(not thread.is_alive() for thread in threads))
        self.assertEqual(sum(accepted), 3)
        self.assertEqual(len(ledger.snapshot()), 3)
        with self.assertRaises(ValueError):
            ledger.validate()

    def test_unexpected_method_and_non200_cannot_be_accepted(self):
        ledger = RequestLedger(limit=10, max_origins=2)
        with runner.serve({"a.html": b"a"}) as (base, audit):
            audit.bind(ledger, "a.html", 0)
            for request in [Request(base + "a.html", method="POST"), Request(base + "absent.css")]:
                with self.assertRaises(HTTPError) as caught:
                    urlopen(request, timeout=2)
                caught.exception.close()
        with self.assertRaises(ValueError):
            audit.validate()
        with self.assertRaises(ValueError):
            ledger.validate()

        self.assertTrue(any(row["status"] != 200 for row in ledger.snapshot()))

    def test_fixture_thread_start_failure_closes_bound_listener(self):
        created = []
        real_server = runner.HTTPServer

        def capture_server(*args, **kwargs):
            server = real_server(*args, **kwargs)
            created.append(server)
            return server

        try:
            with patch.object(runner, "HTTPServer", side_effect=capture_server), \
                    patch.object(threading.Thread, "start", side_effect=RuntimeError("start rejected")):
                with self.assertRaisesRegex(RuntimeError, "start rejected"):
                    with runner.serve({"a.html": b"a"}):
                        self.fail("A failed fixture thread cannot admit a page")
            self.assertEqual(len(created), 1)
            self.assertEqual(created[0].socket.fileno(), -1,
                             "Failed fixture startup leaked its bound listener")
        finally:
            for server in created:
                server.server_close()

    def test_partial_thread_start_failure_still_stops_started_fixture(self):
        created = []
        threads = []
        active = threading.Event()
        real_server = runner.HTTPServer
        real_start = threading.Thread.start

        def capture_server(*args, **kwargs):
            server = real_server(*args, **kwargs)
            server.service_actions = active.set
            real_handle = server.handle_request

            def active_handle():
                active.set()
                return real_handle()

            server.handle_request = active_handle
            created.append(server)
            return server

        def start_then_fail(thread):
            threads.append(thread)
            real_start(thread)
            if not active.wait(timeout=1):
                raise RuntimeError("Fixture did not become active")
            raise RuntimeError("Interrupted after actual thread startup")

        try:
            with patch.object(runner, "HTTPServer", side_effect=capture_server), \
                    patch.object(threading.Thread, "start", start_then_fail):
                with self.assertRaisesRegex(RuntimeError, "after actual thread startup"):
                    with runner.serve({"a.html": b"a"}):
                        self.fail("A partially failed start cannot admit a page")
            self.assertEqual(len(threads), 1)
            self.assertFalse(threads[0].is_alive(),
                             "Partially started fixture survived failed context admission")
            self.assertEqual(created[0].socket.fileno(), -1)
        finally:
            for server in created:
                server.server_close()
            for thread in threads:
                thread.join(timeout=1)
            # The original serve_forever-based regression needs shutdown;
            # the corrected explicit-stop loop has already exited above.
            if any(thread.is_alive() for thread in threads):
                for server in created:
                    server.shutdown()
                for thread in threads:
                    thread.join(timeout=2)

    def test_missing_main_document_request_is_rejected(self):
        ledger = RequestLedger(limit=10, max_origins=2)
        with runner.serve({"a.html": b"a", "other.css": b"x"}) as (base, audit):
            audit.bind(ledger, "a.html", 0)
            with urlopen(base + "other.css", timeout=2) as response:
                self.assertEqual(response.read(), b"x")
        with self.assertRaisesRegex(ValueError, "(?i)(missing|document|page)"):
            audit.validate()

    def test_incomplete_http_request_is_not_silently_discarded(self):
        ledger = RequestLedger(limit=10, max_origins=2)
        with runner.serve({"a.html": b"a"}) as (base, audit):
            audit.bind(ledger, "a.html", 0)
            with urlopen(base + "a.html", timeout=2) as response:
                self.assertEqual(response.read(), b"a")
            address = urlsplit(base)
            with socket.create_connection((address.hostname, address.port), timeout=2) as peer:
                peer.settimeout(2)
                peer.sendall(b"GET /late.css HTTP/1.1\r\n")
                # Wait for the owned server's read deadline, not arbitrary sleep.
                self.assertEqual(peer.recv(1), b"")
        with self.assertRaises(ValueError):
            audit.validate()
        with self.assertRaises(ValueError):
            ledger.validate()

    def test_response_body_write_failure_invalidates_request_evidence(self):
        ledger = RequestLedger(limit=10, max_origins=2)
        real_write = socketserver._SocketWriter.write

        def fail_owned_body(writer, data):
            if data == b"owned-fault-body":
                raise OSError("Owned synthetic response-body write failure")
            return real_write(writer, data)

        with redirect_stderr(io.StringIO()), \
                patch.object(socketserver._SocketWriter, "write", fail_owned_body):
            with runner.serve({"a.html": b"a", "bad.css": b"owned-fault-body"}) as (base, audit):
                audit.bind(ledger, "a.html", 0)
                with urlopen(base + "a.html", timeout=2) as response:
                    self.assertEqual(response.read(), b"a")
                with urlopen(base + "bad.css", timeout=2) as response:
                    with self.assertRaises(IncompleteRead):
                        response.read()
        with self.assertRaises(ValueError):
            audit.validate()
        with self.assertRaises(ValueError):
            ledger.validate()


if __name__ == "__main__":
    unittest.main()
