#!/usr/bin/env python3
"""Frozen-input/tool guards and tiny owned Python process controls, not browser results."""
import ast
import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import signal
import struct
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('lifecycle_baseline', Path(__file__).with_name('lifecycle-baseline.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class Controls(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='mg-lifecycle-unit-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'tmp').mkdir()
        self.binary = self.root / 'owned-binary'
        self.binary.write_bytes(b'owned')
        self.binary.chmod(0o700)
        for name, value in (('ROOT', self.root), ('BINARY_BYTES', 5),
                            ('BINARY_SHA', hashlib.sha256(b'owned').hexdigest())):
            patcher = patch.object(probe, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)

    def response(self):
        frame, _ = probe.encode_frame(0, 0, 'Init', {})
        memory = dict(outstanding_bytes=100, peak_bytes=200, cumulative_bytes=300, allocations=4,
                      outstanding_limit_bytes=32 * 1024 * 1024, cumulative_limit_bytes=64 * 1024 * 1024)
        reply = dict(revision=0, state='Ready', errors=[], navigation=None,
                     default_action={'kind': 'None'}, acknowledgements=[],
                     outcome={'click_canceled': None, 'submit_canceled': None}, scripts_executed=1,
                     allocations=None, boa=dict(profile='boa-page-process-v1', fatal_reason=None,
                     jobs_admitted=0, jobs_executed=0, pending_jobs=0, opcodes_remaining=999999,
                     source_admissions=1, source_bytes=1, worker_memory=memory),
                     snapshot={'nodes': [dict(tag='title', text='', parent=None),
                                         dict(tag='#text', text='before', parent=0)]})
        return frame, {key: value for key, value in frame.items() if key != 'command'} | {'reply': reply}

    def cli(self, *extra):
        with patch.object(probe.subprocess, 'Popen') as launch, patch.object(probe.sys, 'stderr', io.StringIO()):
            result = probe.main(['--binary', str(self.binary), '--output', str(self.root / 'tmp/out'), *extra])
        launch.assert_not_called()
        return result

    def test_five_sources_are_byte_identical_to_the_reviewed_old_control_inputs(self):
        expected = {'empty': 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
                    'ordered': 'fc3a6adf9d0dfc4fca50ee867b166fa475765cc45222203263e5fe9e03d09631',
                    'capture-stop': 'bcdcd0a13514948057a83dc2bfafe69c0cfa56474c1cac800d669c6b5ada443f',
                    'pending-256': '10cf85d4b9347dd77174a24243f0c3d95e56a90bee200684a48b6c17c6b70f48',
                    'canceled-2048': '6812e48c7ff1cbf0edbf56c5d6d100b25a97359a695cc44c73a3a2295acac06e'}
        self.assertEqual({key: hashlib.sha256(value.encode()).hexdigest()
                          for key, value in probe.SOURCES.items()}, expected)
        self.assertEqual(probe.URL, 'https://example.test/owned-lifecycle-baseline')
        self.assertEqual((probe.CHILD_SECONDS, probe.RUN_SECONDS, probe.CLEANUP_SECONDS), (4, 25, 2))
        self.assertFalse(any(isinstance(node, ast.Assert) for node in ast.walk(
            ast.parse(Path(probe.__file__).read_text()))))

    def test_required_and_unknown_cli_arguments_fail_without_launch(self):
        for arguments in ([], ['--binary', str(self.binary)], ['--output', 'tmp/new'],
                          ['--binary', str(self.binary), '--output', 'tmp/new', '--source', '1']):
            with self.subTest(arguments=arguments), patch.object(probe.subprocess, 'Popen') as launch, \
                    patch.object(probe.sys, 'stderr', io.StringIO()):
                with self.assertRaises(SystemExit):
                    probe.main(arguments)
                launch.assert_not_called()

    def test_unsupported_platform_is_rejected_before_launch(self):
        for system, machine in (('darwin', 'x86_64'), ('linux', 'aarch64')):
            with self.subTest(platform=(system, machine)), patch.object(probe.sys, 'platform', system), \
                    patch.object(probe.platform, 'machine', return_value=machine):
                self.assertEqual(self.cli(), 2)

    def test_missing_waitid_cleanup_capability_is_rejected_before_launch(self):
        with patch.object(probe.os, 'WNOWAIT', create=True):
            del probe.os.WNOWAIT
            self.assertEqual(self.cli(), 2)

    def test_missing_wrong_sized_and_wrong_hash_binaries_are_rejected(self):
        for body in (None, b'', b'longer', b'other'):
            with self.subTest(body=body):
                if self.binary.exists():
                    self.binary.unlink()
                if body is not None:
                    self.binary.write_bytes(body)
                    self.binary.chmod(0o700)
                self.assertEqual(self.cli(), 2)
                self.assertFalse((self.root / 'tmp/out').exists())

    def test_nonexecutable_symlink_directory_and_fifo_binaries_are_rejected(self):
        target = self.root / 'target'
        self.binary.rename(target)
        variants = ('nonexecutable', 'symlink', 'directory', 'fifo')
        for kind in variants:
            with self.subTest(kind=kind):
                if kind == 'nonexecutable':
                    self.binary.write_bytes(b'owned')
                elif kind == 'symlink':
                    self.binary.symlink_to(target)
                elif kind == 'directory':
                    self.binary.mkdir()
                else:
                    os.mkfifo(self.binary)
                self.assertEqual(self.cli(), 2)
                self.binary.rmdir() if kind == 'directory' else self.binary.unlink()

    def test_existing_escaping_root_and_symlink_outputs_are_rejected(self):
        existing = self.root / 'tmp/existing'
        existing.mkdir()
        link = self.root / 'tmp/link'
        link.symlink_to(existing, target_is_directory=True)
        for output in (existing, self.root / 'tmp', self.root / 'outside', link / 'new'):
            with self.subTest(output=output), patch.object(probe.subprocess, 'Popen') as launch, \
                    patch.object(probe.sys, 'stderr', io.StringIO()):
                self.assertEqual(probe.main(['--binary', str(self.binary), '--output', str(output)]), 2)
                launch.assert_not_called()

    def test_parent_frames_keep_frozen_identity_and_strict_input_bound(self):
        frame, raw = probe.encode_frame(0, 0, 'Init', {'url': probe.URL, 'html': 'owned'})
        self.assertEqual(struct.unpack('>I', raw[:4])[0], len(raw) - 4)
        self.assertEqual(frame['generation'], 31)
        self.assertEqual(frame['session_id'], 47)
        self.assertEqual(frame['version'], 4)
        with self.assertRaisesRegex(RuntimeError, 'input admission'):
            probe.encode_frame(0, 0, 'Init', {'html': 'x' * probe.INPUT_LIMIT})

    def test_oversized_response_is_rejected_before_pipe_wait_or_body_read(self):
        worker = probe.Worker(time.monotonic() + 1)
        with patch.object(worker, 'wait') as wait, patch.object(probe.os, 'read') as read:
            for size in (0, probe.LIMIT + 1, True):
                with self.assertRaisesRegex(RuntimeError, 'Invalid/oversized'):
                    worker.exact(size)
            wait.assert_not_called()
            read.assert_not_called()

    def test_duplicate_and_nonfinite_json_and_wrong_frame_identity_are_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'Duplicate'):
            probe.decode_response(b'{"version":4,"version":4}')
        for raw in (b'{"value":NaN}', b'{"value":Infinity}', b'{"value":1e9999}', b'{"value":-1e9999}'):
            with self.assertRaisesRegex(RuntimeError, 'Nonfinite'):
                probe.decode_response(raw)
        frame, response = self.response()
        for value in (True, 3, 5):
            changed = copy.deepcopy(response)
            changed['version'] = value
            with self.assertRaisesRegex(RuntimeError, 'identity'):
                probe.summarize(changed, frame)

    def test_summary_accepts_only_original_nonfatal_diagnostic_free_profile(self):
        frame, response = self.response()
        self.assertEqual(probe.summarize(response, frame)['title'], 'before')
        for key, value in (('errors', ['owned failure']), ('state', 'Fatal'), ('navigation', '/wrong'),
                           ('scripts_executed', 0)):
            changed = copy.deepcopy(response)
            changed['reply'][key] = value
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                probe.summarize(changed, frame)
        for key, value in (('jobs_admitted', 2049), ('pending_jobs', True), ('opcodes_remaining', 0),
                           ('fatal_reason', 'owned failure'), ('profile', 'wrong')):
            changed = copy.deepcopy(response)
            changed['reply']['boa'][key] = value
            with self.subTest(key=key), self.assertRaises(RuntimeError):
                probe.summarize(changed, frame)

    def test_missing_wakes_changed_memory_caps_and_oversized_summaries_are_rejected(self):
        frame, response = self.response()
        changes = [lambda reply: reply['boa'].update(jobs_admitted=1, pending_jobs=1),
                   lambda reply: reply['boa']['worker_memory'].update(outstanding_limit_bytes=1),
                   lambda reply: reply['snapshot']['nodes'][1].update(text='x' * 2049)]
        for change in changes:
            changed = copy.deepcopy(response)
            change(changed['reply'])
            with self.assertRaises(RuntimeError):
                probe.summarize(changed, frame)

    def test_wrong_old_observations_do_not_become_queued_lifecycle_acceptance(self):
        frame, response = self.response()
        summary = probe.summarize(response, frame)
        probe.check_case('empty', [summary])
        with self.assertRaisesRegex(RuntimeError, 'old lifecycle observation changed'):
            probe.check_case('capture-stop', [summary])
        with self.assertRaisesRegex(RuntimeError, 'old lifecycle observation changed'):
            probe.check_case('empty', [summary, summary])

    def test_runtime_launch_failure_retains_bounded_error_receipt_without_another_launch(self):
        output = self.root / 'tmp/failure'
        with patch.object(probe.subprocess, 'Popen', side_effect=OSError('owned launch failure')) as launch, \
                patch.object(probe.sys, 'stdout', io.StringIO()):
            self.assertEqual(probe.main(['--binary', str(self.binary), '--output', str(output)]), 1)
        self.assertEqual(launch.call_count, 1)
        raw = (output / 'receipt.json').read_bytes()
        self.assertLessEqual(len(raw), probe.RECEIPT_LIMIT)
        report = json.loads(raw)
        self.assertEqual(report['status'], 'ERROR')
        self.assertTrue(report['cleanup_confirmed'])
        self.assertFalse(report['wpt_pass_claimed'])
        self.assertIn('owned launch failure', report['error'])

    def test_summary_overflow_retains_a_small_error_receipt_and_preserves_no_pass_claim(self):
        output = self.root / 'tmp/receipt-bound'
        with patch.object(probe.Worker, 'start', side_effect=RuntimeError('x' * 10000)), \
                patch.object(probe, 'RECEIPT_LIMIT', 1600), patch.object(probe.sys, 'stdout', io.StringIO()):
            self.assertEqual(probe.main(['--binary', str(self.binary), '--output', str(output)]), 1)
        raw = (output / 'receipt.json').read_bytes()
        self.assertLessEqual(len(raw), 1600)
        self.assertEqual(json.loads(raw)['status'], 'ERROR')
        self.assertEqual(json.loads(raw)['error'], 'Receipt exceeded its fixed admission; raw frames retained')
        self.assertFalse(json.loads(raw)['queued_lifecycle_acceptance_claimed'])

    def test_ordinary_eof_is_observed_without_reap_then_group_kill_precedes_wait(self):
        worker = probe.Worker(time.monotonic() + 1)
        try:
            worker.start([sys.executable, '-c', 'import sys;sys.stdin.buffer.read()'])
            worker.finish_eof()
            self.assertTrue(worker.eof_observed)
            self.assertIsNone(worker.child.returncode)
            events = []
            real_kill, real_wait = os.killpg, worker.child.wait
            def kill(*args):
                events.append('kill-group')
                return real_kill(*args)
            def wait(*args, **kwargs):
                events.append('reap-leader')
                return real_wait(*args, **kwargs)
            with patch.object(probe.os, 'killpg', side_effect=kill), patch.object(worker.child, 'wait', side_effect=wait):
                self.assertEqual(worker.close(), 0)
            self.assertEqual(events, ['kill-group', 'reap-leader'])
            self.assertTrue(worker.cleanup_confirmed)
        finally:
            self.cleanup_worker(worker)

    def test_timeout_kills_owned_leader_and_ordinary_descendant_then_reaps_only_leader(self):
        worker = probe.Worker(time.monotonic() + 4)
        descendant = None
        try:
            source = ('import os,struct,subprocess,sys,time;'
                      'p=subprocess.Popen([sys.executable,"-c","import time;time.sleep(3)"]);'
                      'os.write(1,struct.pack(">I",p.pid));time.sleep(3)')
            worker.start([sys.executable, '-c', source])
            descendant, = struct.unpack('>I', worker.exact(4))
            # Admit startup independently; expire only after the PID frame
            # proves the owned leader and its ordinary child are running.
            worker.deadline = time.monotonic() - 1
            with self.assertRaisesRegex(RuntimeError, 'deadline'):
                worker.finish_eof()
            self.assertFalse(worker.eof_observed)
            self.assertEqual(worker.close(), -signal.SIGKILL)
            self.assertTrue(worker.cleanup_confirmed)
            self.assertIsNotNone(worker.child.returncode)
            self.assert_terminated(descendant)
        finally:
            self.cleanup_worker(worker)

    def test_explicit_cleanup_failure_is_not_claimed_confirmed(self):
        worker = probe.Worker(time.monotonic() + 1)
        try:
            worker.start([sys.executable, '-c', 'import time;time.sleep(3)'])
            with patch.object(worker.child, 'wait', side_effect=subprocess_timeout()):
                with self.assertRaises(Exception):
                    worker.close()
            self.assertFalse(worker.cleanup_confirmed)
        finally:
            self.cleanup_worker(worker)

    def assert_terminated(self, pid):
        deadline = time.monotonic() + 1
        while True:
            try:
                state = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0]
            except (FileNotFoundError, ProcessLookupError):
                return
            if state == 'Z':  # Terminated grandchild, not claimed individually reaped.
                return
            self.assertLess(time.monotonic(), deadline, 'Owned descendant still running')
            time.sleep(0.005)

    def cleanup_worker(self, worker):
        # Safety fallback only after the test inspects production cleanup results.
        child = worker.child
        if child is not None and child.returncode is None:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=2)
        if child is not None:
            for pipe in (child.stdin, child.stdout):
                if not pipe.closed:
                    pipe.close()


def subprocess_timeout():
    return probe.subprocess.TimeoutExpired('owned fixture', 0)


if __name__ == '__main__':
    unittest.main()
