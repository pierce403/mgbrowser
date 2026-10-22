"""Focused bounded orchestration controls; no browser, build or corpus run."""
from contextlib import contextmanager, redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

import run as runner
from parallel import Cancelled, FixedLanes, SharedBudget, SingleFlight


class BudgetTests(unittest.TestCase):
    def budget(self, cancel=None, size=lambda _: 0, limit=150):
        return SharedBudget(Path("unused"), time.monotonic() + 5, cancel or threading.Event(),
                            evidence_bytes=size, output_limit=limit, report_reserve=10)

    def test_two_slot_reservation_is_atomic_and_report_reserve_not_multiplied(self):
        budget = self.budget(size=lambda _: 40)
        with budget.reserve(100):
            self.assertEqual(budget.reserved, 100)
            budget.check()
            with self.assertRaisesRegex(ValueError, "shared admission"):
                with budget.reserve(1):
                    self.fail("An over-admitted producer started")
        self.assertEqual(budget.reserved, 0)
        self.assertEqual(budget.peak_reserved, 100)

    def test_simultaneous_reservations_share_one_ceiling(self):
        budget = self.budget(limit=100)
        start = threading.Barrier(2)
        attempted = threading.Barrier(2)
        accepted = []
        errors = []

        def owner():
            try:
                start.wait(timeout=1)
                try:
                    with budget.reserve(60):
                        accepted.append(True)
                        attempted.wait(timeout=1)
                except ValueError:
                    accepted.append(False)
                    attempted.wait(timeout=1)
            except BaseException as error:
                errors.append(error)

        threads = [threading.Thread(target=owner) for _ in range(2)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(timeout=2)
        self.assertTrue(all(not thread.is_alive() for thread in threads))
        self.assertEqual(errors, [])
        self.assertEqual(sorted(accepted), [False, True])
        self.assertEqual(budget.peak_reserved, 60)
        self.assertEqual(budget.reserved, 0)

    def test_unknown_cleanup_keeps_reservations_and_stops_new_admission(self):
        for failed_owner in (False, True):
            budget = self.budget()
            with budget.reserve(60, cleanup_confirmed=lambda: failed_owner):
                if failed_owner:
                    budget.fail_cleanup()
            self.assertFalse(budget.cleanup_confirmed)
            self.assertEqual(budget.reserved, 60)
            self.assertTrue(budget.cancel_event.is_set())
            with self.assertRaises(Cancelled):
                with budget.reserve(1):
                    self.fail("A new producer followed unknown cleanup")

    def test_shared_deadline_and_cancel_are_checked_before_admission(self):
        cancelled = threading.Event()
        budget = self.budget(cancelled)
        cancelled.set()
        with self.assertRaises(Cancelled):
            with budget.reserve(1):
                self.fail()
        budget = self.budget()
        budget.deadline = time.monotonic() - 1
        with self.assertRaisesRegex(ValueError, "deadline"):
            budget.check()


class CacheTests(unittest.TestCase):
    def test_shared_reference_single_flight_publishes_once_without_locking_owner(self):
        cancel = threading.Event()
        cache = SingleFlight(2, cancel)
        entered = threading.Event()
        release = threading.Event()
        calls = []
        results = []

        def producer():
            calls.append(True)
            entered.set()
            release.wait(timeout=1)
            return {"status": "OK", "artifact": "shared"}

        first = threading.Thread(target=lambda: results.append(cache.get("ref", producer)))
        second = threading.Thread(target=lambda: results.append(cache.get("ref", producer)))
        first.start()
        self.assertTrue(entered.wait(timeout=1))
        second.start()
        # Snapshot is available while the producer is active, but incomplete
        # entries are not visible as accepted page evidence.
        self.assertEqual(cache.snapshot(), {})
        release.set()
        first.join(timeout=1)
        second.join(timeout=1)
        self.assertFalse(first.is_alive() or second.is_alive())
        self.assertEqual(len(calls), 1)
        self.assertIs(results[0], results[1])
        before = cache.snapshot()
        cache.get("other", lambda: {"status": "FAIL", "artifact": "other", "reason": "guard"})
        self.assertEqual(set(before), {"ref"})
        self.assertEqual(set(cache.snapshot()), {"ref", "other"})
        with self.assertRaises(ValueError):
            cache.get("third", producer)

    def test_owner_exception_fulfils_waiters_and_cancels_further_work(self):
        cancel = threading.Event()
        cache = SingleFlight(2, cancel)
        entered = threading.Event()
        release = threading.Event()
        values = []
        errors = []

        def fail():
            entered.set()
            release.wait(timeout=1)
            raise RuntimeError("owned failure")

        def get():
            try:
                values.append(cache.get("ref", fail))
            except Cancelled:
                values.append("cancelled")
            except BaseException as error:
                errors.append(error)

        owner = threading.Thread(target=get)
        owner.start()
        self.assertTrue(entered.wait(timeout=1))
        waiter = threading.Thread(target=get)
        waiter.start()
        release.set()
        owner.join(timeout=1)
        waiter.join(timeout=1)
        self.assertFalse(owner.is_alive() or waiter.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(len(values), 2)
        self.assertTrue(all(value == "cancelled" or value["status"] == "ERROR" for value in values))
        self.assertTrue(cancel.is_set())
        self.assertTrue(cache.snapshot()["ref"]["owner_failed"])
        self.assertEqual(cache.snapshot()["ref"]["status"], "ERROR")
        with self.assertRaises(Cancelled):
            cache.get("new", lambda: self.fail("New owner started"))

    def test_cancellation_wakes_a_waiter_without_releasing_cache_owner(self):
        cancel = threading.Event()
        cache = SingleFlight(2, cancel)
        entered = threading.Event()
        owner_done = threading.Event()
        waiter_done = threading.Event()

        def produce():
            entered.set()
            cancel.wait(timeout=1)
            owner_done.set()
            return {"status": "ERROR", "artifact": "cancelled"}

        owner = threading.Thread(target=lambda: cache.get("ref", produce))
        owner.start()
        self.assertTrue(entered.wait(timeout=1))

        def waiter():
            try:
                cache.get("ref", produce)
            except Cancelled:
                pass
            waiter_done.set()

        waiting = threading.Thread(target=waiter)
        waiting.start()
        cancel.set()
        cache.wake()
        owner.join(timeout=1)
        waiting.join(timeout=1)
        self.assertTrue(owner_done.is_set() and waiter_done.is_set())


class LaneTests(unittest.TestCase):
    def roster(self, count=10):
        return [{"path": f"{index:02}.html"} for index in range(count)]

    def pool(self, function, count=10, jobs=2, deadline=None):
        cancel = threading.Event()
        cache = SingleFlight(20, cancel)
        return FixedLanes(self.roster(count), jobs, function, cancel, cache, deadline or time.monotonic() + 5)

    def test_fixed_source_assignment_bounded_queue_and_coordinator_order(self):
        observed = []
        pool = self.pool(lambda index, test, lane: observed.append((index, lane, threading.current_thread().name)) or test)
        with pool:
            completions = list(pool)
        self.assertTrue(pool.joined)
        self.assertFalse(pool.cancel_event.is_set())
        self.assertEqual(pool.completions.maxsize, 2)
        self.assertEqual(sorted(index for index, _ in completions), list(range(10)))
        self.assertTrue(all(lane == index % 2 and name == f"wpt-lane-{lane}" for index, lane, name in observed))
        for lane in (0, 1):
            self.assertEqual([index for index, actual, _ in observed if actual == lane], list(range(lane, 10, 2)))

    def test_normal_early_break_cancels_full_queue_and_joins(self):
        pool = self.pool(lambda _index, test, _lane: test, count=100)
        began = time.monotonic()
        with pool:
            for _completion in pool:
                break
        self.assertTrue(pool.cancel_event.is_set())
        self.assertTrue(pool.joined)
        self.assertTrue(all(not thread.is_alive() for thread in pool.threads))
        self.assertLess(time.monotonic() - began, 1)

    def test_partial_thread_start_failure_cancels_and_joins_all_started_owners(self):
        pool = self.pool(lambda _index, test, _lane: test, count=100)
        start = threading.Thread.start
        calls = 0

        def partial(thread):
            nonlocal calls
            calls += 1
            start(thread)
            if calls == 2:
                raise RuntimeError("second startup interrupted")

        with patch.object(threading.Thread, "start", partial):
            with self.assertRaisesRegex(RuntimeError, "startup interrupted"):
                with pool:
                    self.fail("A partially started pool admitted coordination")
        self.assertTrue(pool.cancel_event.is_set())
        self.assertTrue(pool.joined)
        self.assertTrue(all(not thread.is_alive() for thread in pool.threads))

    def test_global_deadline_cancels_active_owners_before_returning(self):
        pool = None

        def active(_index, test, _lane):
            pool.cancel_event.wait(timeout=1)
            return test

        pool = self.pool(active, deadline=time.monotonic() + 0.05)
        with self.assertRaisesRegex(ValueError, "deadline"):
            with pool:
                list(pool)
        self.assertTrue(pool.joined)
        self.assertTrue(pool.cancel_event.is_set())

    def test_unexpected_lane_termination_cancels_other_lane_and_cannot_complete(self):
        def broken(index, test, _lane):
            if index == 0:
                raise SystemExit("owned synthetic termination")
            return test

        pool = self.pool(broken)
        with self.assertRaisesRegex(Cancelled, "terminated unexpectedly"):
            with pool:
                list(pool)
        self.assertTrue(pool.joined)


class RunnerIntegrationTests(unittest.TestCase):
    def test_synthetic_one_and_two_lanes_keep_cases_pixels_and_per_page_requests(self):
        cases = [{"path": name, "references": [{"relation": relation, "href": "ref.html", "path": "ref.html"}]}
                 for name, relation in (("a.html", "=="), ("b.html", "=="), ("c.html", "!="))]
        files = {case["path"]: f'<link rel="match" href="ref.html">'.encode() for case in cases}
        files["c.html"] = b'<link rel="mismatch" href="ref.html">'
        files["ref.html"] = b"<p>reference</p>"
        green = bytes(runner.RGB_BYTES - 3) + b"\x00\xff\x00"
        blue = bytes(runner.RGB_BYTES - 3) + b"\x00\x00\xff"
        pixels = {"a.html": green, "b.html": blue, "c.html": blue, "ref.html": green}
        observed = []
        for jobs in (1, 2):
            with tempfile.TemporaryDirectory(prefix="mg-wpt-lane-model-") as directory:
                output = Path(directory)
                report = {"results": runner.pending_results({"tests": cases}), "selected_total": 3, "execution": {"jobs": jobs}}
                cancel = threading.Event()
                ids = iter(range(1000, 1100))
                lock = threading.Lock()
                calls = []

                @contextmanager
                def server(_files):
                    with lock:
                        port = next(ids)
                    audit = runner.RequestAudit(require_owner=True)
                    yield f"http://127.0.0.1:{port}/", audit
                    audit.add({"path": "/" + audit.page, "status": 200})
                    audit.close_admission()

                def render(_binary, _font, _base, page, _output, **_kwargs):
                    with lock:
                        calls.append(page)
                    return {"status": "OK", "artifact": page, "rgb_sha256": runner.digest(pixels[page])}

                with patch.object(runner, "serve", server), patch.object(runner, "render_page", side_effect=render), \
                        patch.object(runner, "check_controls", return_value={"passed": True}), \
                        patch.object(runner, "read_pixels", side_effect=lambda _output, artifact: pixels[artifact]), \
                        redirect_stdout(io.StringIO()):
                    runner.run_cases({"tests": cases}, files, {}, None, None, output, time.monotonic() + 5,
                                     jobs, report, output / "report.json", cancel)
                self.assertEqual(sorted(calls), ["a.html", "b.html", "c.html", "ref.html"])
                self.assertEqual([row["status"] for row in report["results"]], ["PASS", "FAIL", "PASS"])
                self.assertTrue(report["execution"]["cleanup_confirmed"])
                self.assertEqual(report["execution"]["remaining_producer_reserved_bytes"], 0)
                self.assertEqual(report["execution"]["peak_producer_reserved_bytes"], jobs * runner.PAGE_RESERVATION)
                observed.append((report["results"], {name: (value["rgb_sha256"], value["requests"])
                                                     for name, value in report["pages"].items()}))
        self.assertEqual(observed[0], observed[1])

    def test_fixture_cleanup_uncertainty_retains_slots_after_lane_threads_join(self):
        cases = [{"path": "a.html", "references": [{"relation": "==", "href": "ref.html", "path": "ref.html"}]}]
        files = {"a.html": b'<link rel="match" href="ref.html">', "ref.html": b"<p>ref</p>"}

        @contextmanager
        def failed_fixture(_files):
            audit = runner.RequestAudit(require_owner=True)
            yield "http://127.0.0.1:1000/", audit
            raise runner.FixtureCleanupError("synthetic unconfirmed fixture join")

        with tempfile.TemporaryDirectory(prefix="mg-wpt-fixture-uncertain-") as directory:
            output = Path(directory)
            report = {"results": runner.pending_results({"tests": cases}), "selected_total": 1, "execution": {"jobs": 2}}
            cancel = threading.Event()
            with patch.object(runner, "serve", failed_fixture), \
                    patch.object(runner, "render_page", return_value={"status": "OK", "artifact": "not-read"}), \
                    patch.object(runner, "check_controls", return_value={"passed": True}), \
                    patch.object(runner.RequestLedger, "snapshot", side_effect=AssertionError("Unconfirmed fixture audit was read")), \
                    patch.object(runner.SingleFlight, "snapshot", side_effect=AssertionError("Unconfirmed cache was read")), redirect_stdout(io.StringIO()):
                with self.assertRaises(Cancelled):
                    runner.run_cases({"tests": cases}, files, {}, None, None, output, time.monotonic() + 5,
                                     2, report, output / "report.json", cancel)
            self.assertTrue(cancel.is_set())
            self.assertFalse(report["execution"]["cleanup_confirmed"])
            self.assertEqual(report["execution"]["remaining_producer_reserved_bytes"], 2 * runner.PAGE_RESERVATION)
            self.assertNotIn("pages", report)
            self.assertNotIn("requests", report)

    def test_periodic_snapshot_fences_cleanup_failure_after_budget_check(self):
        cases = [{"path": "a.html", "references": []}, {"path": "b.html", "references": []}]
        original_check = SharedBudget.check
        injected = []

        def raced_check(budget):
            original_check(budget)
            if (threading.current_thread() is threading.main_thread()
                    and budget.reserved and not injected):
                # Reproduce another lane failing immediately after admission,
                # before the coordinator's periodic snapshot starts.
                injected.append(True)
                budget.fail_cleanup()

        with tempfile.TemporaryDirectory(prefix="mg-wpt-periodic-quarantine-") as directory:
            output = Path(directory)
            report = {"results": runner.pending_results({"tests": cases}), "selected_total": 2,
                      "execution": {"jobs": 2}}
            cancel = threading.Event()
            with patch.object(runner, "test_prerequisites", return_value=["synthetic unsupported protocol"]), \
                    patch.object(runner, "check_controls", return_value={"passed": True}), \
                    patch.object(runner, "CHECKPOINT_SECONDS", 0), \
                    patch.object(SharedBudget, "check", raced_check), \
                    patch.object(runner.RequestLedger, "snapshot", side_effect=AssertionError("Unconfirmed periodic audit read")), \
                    patch.object(runner.SingleFlight, "snapshot", side_effect=AssertionError("Unconfirmed periodic cache read")), \
                    redirect_stdout(io.StringIO()):
                with self.assertRaises(Cancelled):
                    runner.run_cases({"tests": cases}, {}, {}, None, None, output,
                                     time.monotonic() + 5, 2, report, output / "report.json", cancel)
            self.assertEqual(injected, [True])
            self.assertFalse(report["execution"]["cleanup_confirmed"])
            self.assertEqual(report["execution"]["remaining_producer_reserved_bytes"], 2 * runner.PAGE_RESERVATION)
            self.assertNotIn("pages", report)
            self.assertNotIn("requests", report)

    def test_tool_probe_requires_typed_cleanup_confirmation(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-tool-cleanup-") as directory:
            output = Path(directory)
            for cleanup in (None, False, "true"):
                value = {"status": "OK", "reason": "unconfirmed", "cleanup_succeeded": cleanup}
                with patch.object(runner, "run_bounded", return_value=value):
                    with self.assertRaises(ValueError):
                        runner.tool_output(["not-run"], "probe", output, {}, time.monotonic() + 5)

    def test_control_external_css_is_required_even_for_correct_pixels(self):
        with patch.object(runner, "render_page", side_effect=[{"status": "OK", "artifact": "green", "requests": []},
                                                             {"status": "OK", "artifact": "red", "requests": []}]), \
                patch.object(runner, "read_pixels") as pixels:
            with self.assertRaisesRegex(ValueError, "external CSS request"):
                runner.check_controls(None, None, "http://127.0.0.1:1/", Path("not-read"))
            pixels.assert_not_called()

    def test_budget_scan_tolerates_only_a_disappeared_owned_temporary(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-budget-scan-") as directory:
            output = Path(directory)
            removed = output / "compressed-away.rgb"
            retained = output / "page.rgb.gz"
            retained.write_bytes(b"rgb")
            with patch.object(Path, "iterdir", return_value=iter([removed, retained])):
                self.assertEqual(runner.evidence_bytes(output), 3)
            invalid = output / "directory"
            invalid.mkdir()
            with self.assertRaisesRegex(ValueError, "nonregular"):
                runner.evidence_bytes(output)

    def test_unconfirmed_or_missing_cleanup_cancels_and_retains_reservation(self):
        for cleanup in (False, None):
            cancel = threading.Event()
            with tempfile.TemporaryDirectory(prefix="mg-wpt-cleanup-contract-") as directory:
                output = Path(directory)
                budget = SharedBudget(output, time.monotonic() + 5, cancel,
                                      evidence_bytes=runner.evidence_bytes, output_limit=runner.OUTPUT_LIMIT,
                                      report_reserve=runner.REPORT_LIMIT)
                value = {"status": "OK", "returncode": 0, "reason": "", "cleanup_succeeded": cleanup}
                with budget.reserve(runner.PAGE_RESERVATION), patch.object(runner, "run_bounded", return_value=value):
                    result = runner.render_page(Path("not-executed"), Path("font"), "http://127.0.0.1:1/",
                                                "page.html", output, cancel_event=cancel, budget=budget)
                self.assertEqual(result["status"], "ERROR")
                self.assertFalse(result["cleanup_succeeded"])
                self.assertTrue(cancel.is_set())
                self.assertEqual(budget.reserved, runner.PAGE_RESERVATION)

    def test_report_fallback_does_not_exceed_remaining_shared_evidence(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-fallback-bound-") as directory:
            output = Path(directory)
            path = output / "report.json"
            path.write_bytes(b"prior report")
            (output / "other").write_bytes(b"x" * 100)
            report = {"results": [{"test": "a.html", "status": runner.PENDING}], "selected_total": 1,
                      "infrastructure_errors": [], "pages": {"huge": "x" * 500}}
            before = runner.evidence_bytes(output)
            with patch.object(runner, "OUTPUT_LIMIT", before), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                self.assertFalse(runner.finish_report(path, report))
            self.assertEqual(path.read_bytes(), b"prior report")
            self.assertEqual(runner.evidence_bytes(output), before)

    def test_final_report_cannot_spend_quarantined_producer_slots(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-report-quarantine-") as directory:
            output = Path(directory)
            path = output / "report.json"
            path.write_bytes(b"prior report")
            report = {"results": [{"test": "a.html", "status": runner.PENDING}], "selected_total": 1,
                      "execution": {"cleanup_confirmed": False, "remaining_producer_reserved_bytes": 1000},
                      "infrastructure_errors": []}
            # There is ample raw disk admission for a small report, but all of
            # it is quarantined until unknown producer cleanup is confirmed.
            limit = runner.evidence_bytes(output) + 1000
            with patch.object(runner, "OUTPUT_LIMIT", limit), patch.object(runner, "REPORT_LIMIT", 100), \
                    patch.object(runner, "write_json", wraps=runner.write_json) as write, redirect_stderr(io.StringIO()):
                self.assertFalse(runner.finish_report(path, report))
                write.assert_not_called()
            self.assertEqual(path.read_bytes(), b"prior report")

    def test_final_report_replacement_reserves_quarantined_slots_as_well(self):
        with tempfile.TemporaryDirectory(prefix="mg-wpt-report-admission-") as directory:
            path = Path(directory) / "report.json"
            report = {"results": [{"test": "a.html", "status": runner.PENDING}], "selected_total": 1,
                      "execution": {"cleanup_confirmed": False, "remaining_producer_reserved_bytes": 1234},
                      "infrastructure_errors": []}
            with patch.object(runner, "check_budget") as budget:
                self.assertTrue(runner.finish_report(path, report))
            self.assertEqual(budget.call_args.kwargs["reserve"], runner.REPORT_LIMIT + 1234)


if __name__ == "__main__":
    unittest.main()
