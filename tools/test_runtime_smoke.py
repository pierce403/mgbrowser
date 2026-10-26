#!/usr/bin/env python3
"""Pure synthetic transcript guards: no browser, worker, owner or WPT result."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
# Import this module once, under its normal non-main name. Do not call main,
# load_common, source_bytes or any nested owner: only the two pure checkers run.
spec = importlib.util.spec_from_file_location(
    'runtime_smoke_guards', Path(__file__).with_name('runtime-smoke.py'))
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def wire(kind, message, route='direct'):
    return {'protocol': kind, 'route': route, 'message': message}


def synthetic_batch():
    """Declared checker input, not records captured from a browser."""
    pending = {'code': -32000, 'message': 'Browser transaction pending'}
    rows = [wire('receive', {'method': 'Runtime.executionContextCreated',
                            'params': {'context': {'id': 7}}})]
    requests = [
        {'id': 20, 'method': 'Runtime.callFunctionOn', 'params': {'executionContextId': 7,
            'functionDeclaration': smoke.LOOP_SOURCE, 'arguments': [], 'returnByValue': False}},
        {'id': 21, 'method': 'Runtime.evaluate', 'params': {'expression': '1', 'contextId': 7}},
        {'id': 22, 'method': 'Input.insertText', 'params': {'text': '-newer'}},
        {'id': 23, 'method': 'Runtime.evaluate', 'params': {'expression': '1', 'contextId': 7}},
    ]
    rows.extend(wire('send', request) for request in requests)
    for request_id, position in ((21, 'before-input'), (23, 'after-input')):
        if request_id == 23:
            rows.append(wire('receive', {'id': 22, 'result': {}}))
        rows.append(wire('receive', {'id': request_id, 'error': dict(pending)}))
        rows.append({'phase': 'pending-barrier', 'position': position, 'request_id': request_id,
                     'call_id': 20, 'error': dict(pending), 'fixed_iterations': 10000,
                     'ordering_method': smoke.ORDERING_METHOD})
    rows.extend([
        wire('receive', {'id': 20, 'result': {'result': {'type': 'number', 'value': 10000.0}}}),
        wire('send', {'id': 24, 'method': 'Runtime.evaluate', 'params': {
            'expression': "document.getElementById('query').value", 'contextId': 7, 'returnByValue': False}}),
        wire('receive', {'id': 24, 'result': {'result': {'type': 'string', 'value': 'native-edit-newer'}}}),
        {'phase': 'newer-edit', 'ordering_method': smoke.ORDERING_METHOD},
    ])
    return rows


def synthetic_protocol():
    """Minimal full-checker envelope; invented values prove no native behavior."""
    batch = synthetic_batch()
    rows = batch[:1]
    # The checker requires fourteen initial successful compiled sources and one
    # destination source. These twelve synthetic pairs plus Call/read supply14.
    for request_id in range(1, 13):
        rows.extend([
            wire('send', {'id': request_id, 'method': 'Runtime.evaluate',
                          'params': {'expression': str(request_id), 'contextId': 7}}),
            wire('receive', {'id': request_id, 'result': {
                'result': {'type': 'number', 'value': float(request_id)}}}),
        ])
    rows.extend([{'phase': 'contexts'}, {'phase': 'native-input'}, {'phase': 'closures-handles'},
                 {'phase': 'paint', 'label': 'blue'}, {'phase': 'exception-effects'},
                 {'phase': 'routing-release', 'navigation_live_handle_retained': True}])
    rows.extend(batch[1:])
    rows.extend([
        {'phase': 'paint', 'label': 'brown'},
        wire('send', {'id': 25, 'method': 'Page.navigate', 'params': {'url': smoke.DESTINATION}}),
        wire('receive', {'id': 25, 'result': {}}),
        wire('send', {'id': 26, 'method': 'Runtime.evaluate',
                      'params': {'expression': '42', 'contextId': 8}}),
        wire('receive', {'id': 26, 'result': {'result': {'type': 'number', 'value': 42.0}}}),
        {'phase': 'navigation'},
        {'phase': 'runtime-proof', 'groups': 8, 'images': 6, 'fixed_loop_iterations': 10000,
         'target_moves': False, 'ordering_method': smoke.ORDERING_METHOD,
         'newer_field_glyphs_automatically_verified': False,
         'newer_field_images': ['native-brown.png', 'cdp-brown.png'], 'native_input': True,
         'native_and_cdp_paint': True, 'real_worker_closure': True,
         'measured_newer_edit': True, 'direct_and_flattened': True},
    ])
    return rows


def transcript(rows):
    return '\n'.join([*(json.dumps(row) for row in rows), 'RUNTIME_SMOKE_OK', ''])


class Controls(unittest.TestCase):
    def reject_batch(self, rows):
        with self.assertRaises(RuntimeError):
            smoke.check_ordering_batch(rows)

    def test_exact_four_frame_checker_example_is_accepted(self):
        self.assertEqual(smoke.check_ordering_batch(synthetic_batch()), {
            'method': 'four-frame-single-flush', 'route': 'direct',
            'ids': [20, 21, 22, 23], 'response_ids': [21, 22, 23, 20]})

    def test_full_protocol_checker_requires_and_returns_the_ordering_witness(self):
        result = smoke.check_protocol(transcript(synthetic_protocol()))
        self.assertEqual(result['ordering'], smoke.check_ordering_batch(synthetic_batch()))
        self.assertEqual([len(rows) for rows in result['compiled_sources']], [14, 1])
        self.assertEqual(result['commands'], 19)

    def test_old_sequential_after_probe_success_is_not_a_batch_witness(self):
        rows = synthetic_batch()
        # Model A's failure: receive the first busy reply before sending Input,
        # then a successful after-probe, rather than four sends before reads.
        after_success = wire('receive', {'id': 23, 'result': {'result': {'type': 'number', 'value': 1.0}}})
        sequential = rows[:3] + rows[5:7] + rows[3:4] + rows[7:8] + rows[4:5] + [after_success] + rows[10:]
        self.reject_batch(sequential)

    def test_early_call_reordered_duplicate_or_missing_batch_replies_are_rejected(self):
        original = synthetic_batch()
        for first, second in ((5, 10), (7, 10), (5, 8), (7, 8)):
            changed = copy.deepcopy(original)
            changed[first], changed[second] = changed[second], changed[first]
            with self.subTest(swapped=(first, second)):
                self.reject_batch(changed)
        for changed in (original[:8] + original[9:], original[:11] + original[10:11] + original[11:]):
            with self.subTest(missing_or_duplicate=True):
                self.reject_batch(changed)

    def test_wrong_ids_context_route_or_foreign_session_are_rejected(self):
        for change in ('id-gap', 'id-bool', 'id-overflow', 'context-bool', 'context-overflow',
                       'context-duplicate', 'send-route', 'reply-route', 'send-session', 'reply-session'):
            rows = synthetic_batch()
            if change == 'id-gap': rows[2]['message']['id'] = 29
            elif change == 'id-bool': rows[1]['message']['id'] = True
            elif change == 'id-overflow': rows[1]['message']['id'] = 254
            elif change == 'context-bool': rows[0]['message']['params']['context']['id'] = True
            elif change == 'context-overflow': rows[0]['message']['params']['context']['id'] = 2**31
            elif change == 'context-duplicate': rows.insert(0, copy.deepcopy(rows[0]))
            elif change == 'send-route': rows[3]['route'] = 'flat'
            elif change == 'reply-route': rows[8]['route'] = 'flat'
            elif change == 'send-session': rows[2]['message']['sessionId'] = 'foreign'
            else: rows[8]['message']['sessionId'] = 'foreign'
            with self.subTest(change=change):
                self.reject_batch(rows)

    def test_changed_method_source_input_or_semantic_options_are_rejected(self):
        for index, key, value in ((1, 'method', 'Runtime.evaluate'), (3, 'method', 'Input.dispatchKeyEvent'),
                                  (1, 'functionDeclaration', smoke.LOOP_SOURCE.replace('10000', '10001')),
                                  (1, 'executionContextId', 8), (1, 'arguments', [1]),
                                  (1, 'returnByValue', True), (2, 'expression', '2'),
                                  (2, 'contextId', 8), (3, 'text', '-other')):
            rows = synthetic_batch()
            target = rows[index]['message'] if key == 'method' else rows[index]['message']['params']
            target[key] = value
            with self.subTest(index=index, key=key):
                self.reject_batch(rows)

    def test_barrier_must_match_its_immediately_preceding_pending_rejection(self):
        for change in ('missing', 'duplicate', 'wrong-code', 'nonpending', 'phase-code', 'phase-id',
                       'call-id', 'label', 'iterations', 'ordering-method', 'not-adjacent'):
            rows = synthetic_batch()
            if change == 'missing': del rows[9]
            elif change == 'duplicate': rows.insert(10, copy.deepcopy(rows[9]))
            elif change == 'wrong-code': rows[8]['message']['error']['code'] = -32601
            elif change == 'nonpending': rows[8]['message']['error']['message'] = 'No usable context'
            elif change == 'phase-code': rows[9]['error']['code'] = -32601
            elif change == 'phase-id': rows[9]['request_id'] = 21
            elif change == 'call-id': rows[9]['call_id'] = 19
            elif change == 'label': rows[9]['position'] = 'before-input'
            elif change == 'iterations': rows[9]['fixed_iterations'] = 10001
            elif change == 'ordering-method': rows[9]['ordering_method'] = 'sequential'
            else: rows.insert(9, {'synthetic-note': 'not a barrier'})
            with self.subTest(change=change):
                self.reject_batch(rows)

    def test_input_ack_and_finite_call_result_must_be_exact(self):
        for change in ('input-error', 'input-extra', 'call-error', 'call-value', 'call-type', 'call-extra'):
            rows = synthetic_batch()
            if change == 'input-error': rows[7]['message'] = {'id': 22, 'error': {'code': -32000, 'message': 'pending'}}
            elif change == 'input-extra': rows[7]['message']['result']['extra'] = True
            elif change == 'call-error': rows[10]['message'] = {'id': 20, 'error': {'code': -32000, 'message': 'fatal'}}
            elif change == 'call-value': rows[10]['message']['result']['result']['value'] = 9999.0
            elif change == 'call-type': rows[10]['message']['result']['result']['type'] = 'string'
            else: rows[10]['message']['result']['result']['objectId'] = 'invented'
            with self.subTest(change=change):
                self.reject_batch(rows)

    def test_newer_value_read_and_phase_are_bound_after_call_completion(self):
        for change in ('early-read', 'read-id', 'read-source', 'read-context', 'read-route',
                       'value-route', 'value-session', 'wrong-value', 'phase-order', 'phase-method'):
            rows = synthetic_batch()
            if change == 'early-read': rows[10], rows[11] = rows[11], rows[10]
            elif change == 'read-id': rows[11]['message']['id'] = 25
            elif change == 'read-source': rows[11]['message']['params']['expression'] = "'native-edit-newer'"
            elif change == 'read-context': rows[11]['message']['params']['contextId'] = 8
            elif change == 'read-route': rows[11]['route'] = 'flat'
            elif change == 'value-route': rows[12]['route'] = 'flat'
            elif change == 'value-session': rows[12]['message']['sessionId'] = 'foreign'
            elif change == 'wrong-value': rows[12]['message']['result']['result']['value'] = 'stale-worker-newer'
            elif change == 'phase-order': rows[12], rows[13] = rows[13], rows[12]
            else: rows[13]['ordering_method'] = 'sequential'
            with self.subTest(change=change):
                self.reject_batch(rows)

    def test_full_protocol_cannot_bypass_batch_with_summary_or_phase_claims(self):
        for change in ('after-probe-success', 'early-call', 'missing-barrier', 'summary-method', 'missing-marker'):
            rows = synthetic_protocol()
            if change == 'after-probe-success':
                next(row for row in rows if row.get('protocol') == 'receive'
                     and row['message'].get('id') == 23)['message'] = {
                         'id': 23, 'result': {'result': {'type': 'number', 'value': 1.0}}}
            elif change == 'early-call':
                call = next(index for index, row in enumerate(rows) if row.get('protocol') == 'receive'
                            and row['message'].get('id') == 20)
                busy = next(index for index, row in enumerate(rows) if row.get('protocol') == 'receive'
                            and row['message'].get('id') == 21)
                rows[call], rows[busy] = rows[busy], rows[call]
            elif change == 'missing-barrier':
                rows = [row for row in rows if row.get('position') != 'after-input']
            elif change == 'summary-method': rows[-1]['ordering_method'] = 'sequential'
            text = transcript(rows)
            if change == 'missing-marker': text = text.replace('RUNTIME_SMOKE_OK\n', '')
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                smoke.check_protocol(text)


# One definition-only, non-main import. Runtime execution waits until root binds
# its client pin; never call main/load_common/source_bytes or owner I/O here.
moves_spec = importlib.util.spec_from_file_location(
    'runtime_moves_identity_guards', Path(__file__).with_name('runtime-moves-smoke.py'))
moves = importlib.util.module_from_spec(moves_spec)
moves_spec.loader.exec_module(moves)


class MoveIdentityControls(unittest.TestCase):
    """Invented identity metadata only; no native/Runtime/move result is proved."""

    @staticmethod
    def info(target, url, attached):
        return {'targetId': target, 'type': 'page', 'url': url, 'attached': attached}

    def inventory(self, target='page-42'):
        return {'targetInfos': [self.info('page-1', moves.URL, True),
                                self.info(target, 'about:blank', False)]}

    def test_canonical_positive_u64_targets_include_nonconsecutive_ids(self):
        for number in (1, 4, 42, 2**64-1):
            with self.subTest(number=number):
                self.assertEqual(moves.target_number(f'page-{number}'), number)
        for target in (None, True, 4, 4.0, [], {}, b'page-4', '', 'page-', 'page-0',
                       'page-00', 'page-04', 'page-+4', 'page--4', 'page-４', 'page-٤',
                       ' page-4', 'page-4 ', 'page-4\n', 'page-4/x', 'frame-4',
                       'PAGE-4', f'page-{2**64}', 'page-' + '9'*21):
            with self.subTest(target=target), self.assertRaises(RuntimeError):
                moves.target_number(target)

    def test_target_inventory_requires_unique_page_ids_and_exact_initial_a(self):
        pair = self.inventory()
        for rows in (pair['targetInfos'][:1], pair['targetInfos'], list(reversed(pair['targetInfos'])),
                     self.inventory(f'page-{2**64-1}')['targetInfos']):
            with self.subTest(targets=[row['targetId'] for row in rows]):
                self.assertEqual(moves.target_inventory({'targetInfos': rows}),
                                 {row['targetId']: row for row in rows})
        bad = [None, [], {}, {'targetInfos': None}, {'targetInfos': []},
               {'targetInfos': [None]}, {'targetInfos': [pair['targetInfos'][1]]},
               {'targetInfos': [*pair['targetInfos'], self.info('page-43', 'about:blank', False)]},
               {'targetInfos': [pair['targetInfos'][0], copy.deepcopy(pair['targetInfos'][0])]},
               {'targetInfos': [pair['targetInfos'][1], copy.deepcopy(pair['targetInfos'][1])]}]
        for field, value in (('type', 'worker'), ('targetId', 'page-042'),
                             ('targetId', None), ('url', moves.URL_B)):
            changed = copy.deepcopy(pair)
            changed['targetInfos'][0][field] = value
            bad.append(changed)
        for field, value in (('type', 'iframe'), ('targetId', 'page-0'),
                             ('targetId', 'page-１')):
            changed = copy.deepcopy(pair)
            changed['targetInfos'][1][field] = value
            bad.append(changed)
        for result in bad:
            with self.subTest(result=result), self.assertRaises(RuntimeError):
                moves.target_inventory(result)

    def test_native_tab_record_is_single_canonical_and_distinct_from_tab_counts(self):
        self.assertEqual(moves.opened_tab('TABS window=99 groups=1 count=1\n'
                                         'TAB opened=4 pane=2\n'
                                         'TABS window=99 groups=1 count=2\n'), 4)
        self.assertEqual(moves.opened_tab(f'TAB opened={2**64-1} pane={2**64-1}\n'), 2**64-1)
        bad = ['', 'TABS window=99 groups=1 count=2\n', 'TAB invalid\n',
               'TAB opened=4 pane=2\nTAB opened=4 pane=2\n',
               'TAB opened=4 pane=2\nTAB opened=42 pane=2\n',
               'TAB opened=4 pane=2 trailing\n']
        for tab, pane in (('0', '2'), ('04', '2'), ('+4', '2'), ('４', '2'),
                          (str(2**64), '2'), ('4', '0'), ('4', '02'), ('4', '-2'),
                          ('4', '٢'), ('4', str(2**64))):
            bad.append(f'TAB opened={tab} pane={pane}\n')
        for log in bad:
            with self.subTest(log=log), self.assertRaises(RuntimeError):
                moves.opened_tab(log)

    def test_blank_new_target_binding_uses_actual_native_id_and_never_a_fallback(self):
        for number in (4, 42, 2**64-1):
            result = self.inventory(f'page-{number}')
            native = moves.opened_tab(f'TAB opened={number} pane=2\n')
            with self.subTest(number=number):
                self.assertEqual(moves.bind_new_target(result, native), f'page-{number}')
                self.assertIsNone(moves.bind_new_target(result, None))
        only_a = {'targetInfos': self.inventory()['targetInfos'][:1]}
        self.assertIsNone(moves.bind_new_target(only_a, None))
        self.assertIsNone(moves.bind_new_target(only_a, 42))
        pair = self.inventory()
        for native in (1, 2, 4, 43, 0, -1, True, 42.0, '42', 2**64):
            with self.subTest(native=native), self.assertRaises(RuntimeError):
                moves.bind_new_target(pair, native)
        for field, value in (('url', moves.URL_B), ('url', moves.URL), ('url', 'about:blank?other'),
                             ('attached', True), ('attached', 0), ('attached', None)):
            changed = copy.deepcopy(pair)
            changed['targetInfos'][1][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(RuntimeError):
                moves.bind_new_target(changed, 42)
        changed = copy.deepcopy(pair)
        del changed['targetInfos'][1]['attached']
        with self.assertRaises(RuntimeError):
            moves.bind_new_target(changed, 42)


class MoveTabCountControls(unittest.TestCase):
    """Synthetic global tab-count phases, not a native move acceptance result."""

    @staticmethod
    def log():
        return ('TABS window=101 groups=1 count=1\n'
                'TAB opened=42 pane=7\n'
                'TABS window=101 groups=1 count=2\n'
                'TABS window=101 groups=1 count=2\n'
                'TABS window=509 groups=1 count=2\n'
                'TABS window=101 groups=1 count=2\n'
                'TABS window=101 groups=1 count=1\n')

    def test_global_count_tracks_one_two_one_across_both_owned_windows(self):
        log = self.log()
        expected = [('101', '1', '1'), ('101', '1', '2'), ('101', '1', '2'),
                    ('509', '1', '2'), ('101', '1', '2'), ('101', '1', '1')]
        self.assertEqual(moves.check_tab_counts(log, 101, 509), expected)
        # Repeated geometry synchronization does not add tabs or transitions.
        repeated = ('TABS window=101 groups=1 count=1\n' + log
                    + 'TABS window=101 groups=1 count=1\n')
        self.assertEqual(moves.check_tab_counts(repeated, 101, 509),
                         [('101', '1', '1'), *expected, ('101', '1', '1')])
        self.assertEqual(moves.check_tab_counts(log.replace('101', '701').replace('509', '23'), 701, 23),
                         [('701', '1', '1'), ('701', '1', '2'), ('701', '1', '2'),
                          ('23', '1', '2'), ('701', '1', '2'), ('701', '1', '1')])

    def test_bad_owned_geometry_global_count_order_or_opening_is_rejected(self):
        log = self.log()
        bad = [
            log.replace('window=509', 'window=510'),
            log.replace('window=509 groups=1', 'window=509 groups=2'),
            log.replace('window=509 groups=1 count=2', 'window=509 groups=1 count=3'),
            log.replace('window=509 groups=1 count=2', 'window=509 groups=1 count=1'),
            log.replace('TAB opened=42 pane=7\n', ''),
            log.replace('TAB opened=42 pane=7\n', 'TAB opened=42 pane=7\nTAB opened=43 pane=7\n'),
            log.replace('TAB opened=42 pane=7', 'TAB opened=042 pane=7'),
            log.replace('TAB opened=42 pane=7\n', '', 1).replace(
                'TABS window=509', 'TAB opened=42 pane=7\nTABS window=509', 1),
            log.replace('TABS window=509 groups=1 count=2\n', ''),
            log.replace('TABS window=101 groups=1 count=1\n', '', 1),
            log.rsplit('TABS window=101 groups=1 count=1\n', 1)[0],
            log + 'TABS window=101 groups=1 count=2\n',
            log.replace('TABS window=509', 'TABS window=101 groups=1 count=1\nTABS window=509', 1),
        ]
        for malformed in ('TABS window=0509 groups=1 count=2', 'TABS window=509 groups=01 count=2',
                          'TABS window=509 groups=1 count=02', 'TABS window=509 groups=1 count=0',
                          'TABS window=509 groups=1 count=-2', 'TABS window=509 groups=1 count=２',
                          'TABS window=509 groups=1 count=2 trailing', 'TABS malformed', 'TABS'):
            bad.append(log.replace('TABS window=509 groups=1 count=2', malformed))
        for changed in bad:
            with self.subTest(log=changed), self.assertRaises(RuntimeError):
                moves.check_tab_counts(changed, 101, 509)


if __name__ == '__main__':
    unittest.main()
