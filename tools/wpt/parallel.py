"""Two fixed source-assigned test lanes; no browser or worker policy changes.

Only the coordinator consumes completions and writes reports. A lane owns at
most one producer at a time and never holds a cache lock while producing or
waiting. A page is published once, after its producer/context has cleaned up.
"""
from contextlib import contextmanager
import queue
import threading
import time


class Cancelled(ValueError):
    pass


class SharedBudget:
    """Atomic aggregate admission, not an OS disk/RSS quota.

    Existing files, every outstanding producer reservation, and exactly one
    report-replacement reserve must fit the unchanged shared evidence ceiling.
    Counting a live producer's current files as well as its reservation is
    deliberately conservative. Reservations end only after owned cleanup.
    """
    def __init__(self, output, deadline, cancel_event, *, evidence_bytes,
                 output_limit, report_reserve):
        self.output = output
        self.deadline = deadline
        self.cancel_event = cancel_event
        self.evidence_bytes = evidence_bytes
        self.output_limit = output_limit
        self.report_reserve = report_reserve
        self.lock = threading.Lock()
        self.reserved = 0
        self.peak_reserved = 0
        self.cleanup_confirmed = True

    def _check(self, extra=0):
        if self.cancel_event.is_set():
            raise Cancelled("WPT orchestration cancelled")
        if time.monotonic() >= self.deadline:
            raise ValueError("Whole WPT run exceeded its reviewed 25-minute deadline")
        if self.evidence_bytes(self.output) + self.reserved + extra + self.report_reserve > self.output_limit:
            raise ValueError("WPT evidence exceeds the reviewed shared admission")

    def check(self):
        with self.lock:
            self._check()

    @contextmanager
    def reserve(self, amount, cleanup_confirmed=None):
        if type(amount) is not int or amount < 0:
            raise ValueError("Invalid producer reservation")
        with self.lock:
            self._check(amount)
            self.reserved += amount
            self.peak_reserved = max(self.peak_reserved, self.reserved)
        try:
            yield
        finally:
            # Caller must leave this scope after producer and fixture cleanup.
            with self.lock:
                if self.cleanup_confirmed and (cleanup_confirmed is None or cleanup_confirmed()):
                    self.reserved -= amount
                else:
                    self.cleanup_confirmed = False
                    self.cancel_event.set()

    def fail_cleanup(self):
        with self.lock:
            self.cleanup_confirmed = False
            self.cancel_event.set()


class RequestLedger:
    """One bounded audit across all unique owned fixture origins."""
    def __init__(self, limit, max_origins):
        self.limit = limit
        self.max_origins = max_origins
        self.lock = threading.Lock()
        self.rows = []
        self.origins = set()
        self.failures = []

    def claim_origin(self, origin):
        with self.lock:
            if origin in self.origins:
                return False
            if len(self.origins) >= self.max_origins:
                raise ValueError("Fixture origin inventory exceeded its admission")
            self.origins.add(origin)
            return True

    def fail(self, reason):
        with self.lock:
            if len(self.failures) < 64:
                self.failures.append(str(reason)[:1024])

    def add(self, page, lane, row):
        with self.lock:
            if len(self.rows) >= self.limit:
                if len(self.failures) < 64:
                    self.failures.append("HTTP request audit exceeded its reviewed entry bound")
                return False
            self.rows.append({"page": page, "lane": lane, **row})
            return True

    def snapshot(self):
        with self.lock:
            return [dict(row) for row in self.rows]

    def validate(self):
        with self.lock:
            if self.failures:
                raise ValueError("Fixture audit rejected: " + "; ".join(self.failures))
            if any(row["status"] != 200 for row in self.rows):
                raise ValueError("Corpus requested an absent or rejected fixture resource")


class SingleFlight:
    """One immutable completed value per page, bounded by admitted inventory."""
    def __init__(self, max_entries, cancel_event):
        self.max_entries = max_entries
        self.cancel_event = cancel_event
        self.condition = threading.Condition()
        self.entries = {}

    def get(self, name, producer):
        with self.condition:
            if self.cancel_event.is_set():
                raise Cancelled("WPT orchestration cancelled before page admission")
            entry = self.entries.get(name)
            owner = entry is None
            if owner:
                if len(self.entries) >= self.max_entries:
                    raise ValueError("Single-flight page inventory exceeded its admission")
                entry = {"done": False, "value": None}
                self.entries[name] = entry
            else:
                while not entry["done"]:
                    if self.cancel_event.is_set():
                        raise Cancelled("WPT orchestration cancelled while waiting for page")
                    self.condition.wait(0.025)
                return entry["value"]
        # An owner never asks for another page until this producer returns.
        # Therefore cross-lane reference waits cannot form a dependency cycle.
        value = None
        try:
            value = producer()
            if not isinstance(value, dict) or "status" not in value or "artifact" not in value:
                raise ValueError("Page producer returned malformed completion")
        except BaseException as error:
            value = {"status": "ERROR", "reason": "Page owner failed: " + str(error)[:4096],
                     "artifact": "", "owner_failed": True}
            self.cancel_event.set()
        finally:
            with self.condition:
                entry["value"] = value
                entry["done"] = True
                self.condition.notify_all()
        return value

    def snapshot(self):
        with self.condition:
            # Producers never mutate their value after publishing it. The
            # coordinator owns this mapping, not the live cache dictionary.
            return {name: entry["value"] for name, entry in self.entries.items() if entry["done"]}

    def wake(self):
        with self.condition:
            self.condition.notify_all()


class FixedLanes:
    """Capacity-two completions from one/two immutable index-modulo lanes."""
    def __init__(self, tests, jobs, case_function, cancel_event, cache, deadline):
        if jobs not in (1, 2):
            raise ValueError("WPT permits only one or two fixed renderer lanes")
        self.tests = tests
        self.jobs = jobs
        self.case_function = case_function
        self.cancel_event = cancel_event
        self.cache = cache
        self.deadline = deadline
        self.completions = queue.Queue(maxsize=2)
        self.threads = []
        self.exhausted = False
        self.joined = False
        self.failure = None

    def _put(self, completion):
        while not self.cancel_event.is_set():
            try:
                self.completions.put(completion, timeout=0.025)
                return
            except queue.Full:
                pass

    def _lane(self, lane):
        try:
            for index in range(lane, len(self.tests), self.jobs):
                if self.cancel_event.is_set():
                    return
                try:
                    result = self.case_function(index, self.tests[index], lane)
                except Exception as error:
                    result = {"test": self.tests[index]["path"], "status": "ERROR",
                              "reasons": [str(error)[:4096]], "comparisons": []}
                self._put((index, result))
        except BaseException as error:
            self.failure = "Renderer lane terminated unexpectedly: " + str(error)[:4096]
            self.cancel_event.set()
            self.cache.wake()

    def _join(self):
        deadline = time.monotonic() + 5
        for thread in self.threads:
            if thread.ident is not None:
                thread.join(timeout=max(0, deadline - time.monotonic()))
        self.joined = not any(thread.is_alive() for thread in self.threads)
        if not self.joined:
            raise ValueError("Renderer lane cleanup did not finish within its finite shared allowance")

    def __enter__(self):
        try:
            for lane in range(self.jobs):
                thread = threading.Thread(target=self._lane, args=(lane,), name=f"wpt-lane-{lane}")
                self.threads.append(thread)
                thread.start()
        except BaseException:
            self.cancel_event.set()
            self.cache.wake()
            self._join()
            raise
        return self

    def __iter__(self):
        remaining = len(self.tests)
        while remaining:
            if time.monotonic() >= self.deadline:
                raise ValueError("Whole WPT run exceeded its reviewed 25-minute deadline")
            if self.cancel_event.is_set():
                raise Cancelled(self.failure or "WPT orchestration cancelled")
            try:
                completion = self.completions.get(timeout=0.025)
            except queue.Empty:
                if self.cancel_event.is_set():
                    raise Cancelled(self.failure or "WPT orchestration cancelled")
                if not any(thread.is_alive() for thread in self.threads):
                    raise ValueError("Renderer lanes stopped before completing the selected roster")
                continue
            remaining -= 1
            yield completion
        self.exhausted = True

    def __exit__(self, *_exception):
        if not self.exhausted or (_exception and _exception[0] is not None):
            self.cancel_event.set()
            self.cache.wake()
        self._join()
