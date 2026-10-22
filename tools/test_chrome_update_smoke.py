"""Synthetic log checks using the actual native restart destination predicate.

No browser, display, update server or native input is started by these tests.
"""
from pathlib import Path
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/wpt'))
from process_control import run_bounded


class RestartDestinationGuard(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        source = (ROOT / 'tools/chrome-update-smoke.sh').read_text()
        begin = '# BEGIN restart destination guard: also exercised by synthetic checker tests.\n'
        end = '# END restart destination guard.\n'
        if source.count(begin) != 1 or source.count(end) != 1:
            raise RuntimeError('Actual native destination guard boundaries changed')
        cls.guard = source.split(begin, 1)[1].split(end, 1)[0]
        cls.destination = ('LOADED http://127.0.0.1:7878/destination HTTP 200 '
                           'title="Local journey completed" items=10 forms=0\n')
        cls.trailer = ('SCRIPT_BOA {"jobs_admitted":2,"jobs_executed":1,"pending_jobs":1}\n'
                       'SCRIPT_COMPLETE executed=0\nPAGE_TASK_APPLIED sequence=1 revision=1\n'
                       'SCRIPT_BOA {"jobs_admitted":2,"jobs_executed":2,"pending_jobs":0}\n'
                       'SCRIPT_COMPLETE executed=0\nPAGE_TASK_APPLIED sequence=2 revision=2\n')

    def check_guard(self, body, before, expected):
        with tempfile.TemporaryDirectory(prefix='mg-chrome-guard-') as temporary:
            directory = Path(temporary)
            log = directory / 'browser.log'
            log.write_text(body)
            command = ['bash', '-c', self.guard + '\nrestart_destination_loaded "$1" "$2"\n',
                       'synthetic-restart-guard', str(log), str(before)]
            result = run_bounded(command, cwd=ROOT, env={'PATH': '/usr/bin:/bin'},
                                 log_path=directory / 'guard.log', timeout=2,
                                 global_deadline=time.monotonic() + 3,
                                 max_output_bytes=16 * 1024)
            self.assertTrue(result['completed'], result)
            self.assertTrue(result['cleanup_succeeded'], result)
            # The supervisor deliberately calls an ordinary nonzero predicate
            # exit ERROR; completed/cleanup still distinguish it from a timeout
            # or uncertain process teardown.
            self.assertEqual(result['status'], 'OK' if expected else 'ERROR', result)
            self.assertEqual(result['returncode'], 0 if expected else 1, result)

    def test_lifecycle_diagnostics_after_new_destination_do_not_hide_it(self):
        self.check_guard(self.destination + self.destination + self.trailer, 1, True)

    def test_old_destination_does_not_override_wrong_last_loaded_url(self):
        wrong = self.destination.replace('/destination HTTP', '/search HTTP')
        self.check_guard(self.destination + wrong + self.trailer, 1, False)

    def test_destination_must_be_a_new_postrestart_load(self):
        self.check_guard(self.destination + self.trailer, 1, False)

    def test_failed_destination_or_wrong_title_is_not_success(self):
        for wrong in [self.destination.replace('HTTP 200', 'HTTP 404'),
                      self.destination.replace('Local journey completed', 'Wrong document')]:
            with self.subTest(record=wrong):
                self.check_guard(self.destination + wrong + self.trailer, 1, False)

    def test_no_loaded_record_or_invalid_saved_count_is_not_success(self):
        self.check_guard(self.trailer, 0, False)
        self.check_guard(self.destination, 'invalid', False)


if __name__ == '__main__':
    unittest.main()
