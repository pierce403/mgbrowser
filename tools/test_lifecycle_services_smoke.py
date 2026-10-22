#!/usr/bin/env python3
"""Synthetic lifecycle evidence guards; these are not browser/WPT results."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('lifecycle_services_smoke', Path(__file__).with_name('lifecycle-services-smoke.py'))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class Controls(unittest.TestCase):
    def reports(self):
        inline = smoke.FIXTURE.read_bytes().split(b'<script>', 1)[1].split(b'</script>', 1)[0]
        memory = dict(outstanding_limit_bytes=32 * 1024 * 1024, cumulative_limit_bytes=64 * 1024 * 1024,
                      outstanding_bytes=100, peak_bytes=200, cumulative_bytes=300, allocations=4)
        return [dict(profile='boa-page-process-v1', fatal_reason=None, opcodes_remaining=999900 - i,
                     source_admissions=2, source_bytes=len(inline) + len(smoke.DEFERRED.read_bytes()),
                     jobs_admitted=a, jobs_executed=e, pending_jobs=p, worker_memory=dict(memory))
                for i, (a, e, p) in enumerate(smoke.TRACE)]

    def log(self, reports):
        return '\n'.join([*('SCRIPT_BOA ' + json.dumps(report) for report in reports),
                          'SCRIPT_COMPLETE executed=2',
                          *(f'PAGE_TASK_APPLIED sequence={i} revision={i}' for i in range(1, 6)), ''])

    def probe(self):
        return dict(scope='authored queued lifecycle, not WPT/testharness completion', order=smoke.ORDER,
                    evidence=dict(events=smoke.EVENTS, later_dom_projection=True, frame_id='frame-1',
                                  dcl_timestamp=1.0, load_timestamp=2.0))

    def probe_log(self, proof):
        return 'CDP_LIFECYCLE_OK ' + json.dumps(proof) + '\n'

    def test_exact_source_deferred_lifecycle_and_later_work_are_accepted(self):
        evidence = smoke.check_worker(self.log(self.reports()), 'synthetic')
        self.assertEqual(evidence['fixture_applied_tasks'], 5)
        self.assertEqual(evidence['long_timer_left_pending'], 1)
        self.assertEqual(evidence['work'][-1], [15, 14, 1])

    def test_missing_merged_repeated_and_unfinished_steps_are_rejected(self):
        reports = self.reports()
        for changed in (reports[:3] + reports[4:], reports[:-1], reports + reports[-2:-1],
                        reports[:2] + reports[1:2] + reports[2:]):
            with self.subTest(changed=changed), self.assertRaises(RuntimeError):
                smoke.check_worker(self.log(changed), 'synthetic')

    def test_missing_extra_source_or_parent_task_is_rejected(self):
        log = self.log(self.reports())
        for changed in (log.replace('PAGE_TASK_APPLIED ', 'NOT_TASK '),
                        log + 'PAGE_TASK_APPLIED sequence=6 revision=6\n',
                        log.replace('SCRIPT_COMPLETE executed=2', 'SCRIPT_COMPLETE executed=1'),
                        log + 'SCRIPT_COMPLETE executed=2\n'):
            with self.subTest(changed=changed), self.assertRaises(RuntimeError):
                smoke.check_worker(changed, 'synthetic')

    def test_failure_families_are_not_hidden_by_later_clean_replies(self):
        for marker in ('SCRIPT_PARTIAL', 'SCRIPT_DIAGNOSTIC', 'SCRIPT_ERROR', 'SCRIPT_REJECTED',
                       'SCRIPT_SESSION_CLOSED', 'SCRIPT_SHUTDOWN_ERROR', 'LOAD_ERROR',
                       'RESOURCE_WARNING', 'RESOURCE_DIAGNOSTIC', 'JOURNEY_INCOMPLETE'):
            with self.subTest(marker=marker), self.assertRaises(RuntimeError):
                smoke.check_worker(self.log(self.reports()) + marker + ' owned failure\n', 'synthetic')

    def test_profile_fatal_boolean_and_renewed_resource_evidence_are_rejected(self):
        for field, value in (('profile', 'other'), ('fatal_reason', 'owned failure'),
                             ('pending_jobs', True), ('source_bytes', 1), ('opcodes_remaining', 1_000_001)):
            reports = self.reports(); reports[-1][field] = value
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                smoke.check_worker(self.log(reports), 'synthetic')
        for field in ('outstanding_limit_bytes', 'cumulative_limit_bytes'):
            reports = self.reports(); reports[-1]['worker_memory'][field] += 1
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                smoke.check_worker(self.log(reports), 'synthetic')
        for field in ('peak_bytes', 'cumulative_bytes', 'allocations'):
            reports = self.reports(); reports[0]['worker_memory'][field] += 1
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                smoke.check_worker(self.log(reports), 'synthetic')

    def test_exact_external_proof_is_accepted_without_general_compatibility_claim(self):
        smoke.check_probe(self.probe_log(self.probe()))

    def test_external_missing_duplicate_order_timestamps_or_later_projection_fail(self):
        proof = self.probe()
        for changed in ('', self.probe_log(proof) * 2):
            with self.assertRaises(RuntimeError): smoke.check_probe(changed)
        for field, value in (('events', smoke.EVENTS[:-1]), ('later_dom_projection', False),
                             ('dcl_timestamp', True), ('load_timestamp', 0.5), ('load_timestamp', float('inf')),
                             ('frame_id', '')):
            changed = copy.deepcopy(proof); changed['evidence'][field] = value
            with self.subTest(field=field), self.assertRaises(RuntimeError):
                smoke.check_probe(self.probe_log(changed))
        changed = copy.deepcopy(proof); changed['order'] = 'load first'
        with self.assertRaises(RuntimeError): smoke.check_probe(self.probe_log(changed))


if __name__ == '__main__': unittest.main()
