#!/usr/bin/env python3
"""Owned v0.9.10 retained-worker observations, not WPT or future-lifecycle acceptance."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import select
import signal
import stat
import struct
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY_SHA = '0a4b00e3ce18eae1e6b84766dc8ada6772d75738df3ea681ec0f508d52f76836'
BINARY_BYTES = 23_708_856
LIMIT = 4 * 1024 * 1024
INPUT_LIMIT = 8192
RECEIPT_LIMIT = 64 * 1024
CHILD_SECONDS, RUN_SECONDS, CLEANUP_SECONDS = 4, 25, 2
URL = 'https://example.test/owned-lifecycle-baseline'
SOURCES = {
    'empty': '',
    'ordered': """
        var order=['script:'+document.readyState];
        Promise.resolve().then(()=>order.push('script-job:'+document.readyState));
        document.addEventListener('readystatechange',()=>{
            order.push('ready:'+document.readyState);
            Promise.resolve().then(()=>order.push('ready-job:'+document.readyState));
        });
        document.addEventListener('DOMContentLoaded',()=>{
            order.push('dcl:'+document.readyState);
            Promise.resolve().then(()=>order.push('dcl-job:'+document.readyState));
        });
        window.addEventListener('load',()=>{
            order.push('load:'+document.readyState);
            Promise.resolve().then(()=>{
                order.push('load-job:'+document.readyState); document.title=order.join('|');
            });
        });
        setTimeout(()=>{order.push('timer:'+document.readyState);document.title=order.join('|');},0);
        window.addEventListener('message',()=>{
            order.push('message:'+document.readyState);document.title=order.join('|');
        });
        postMessage('owned-before-lifecycle','*');
    """,
    'capture-stop': """
        var order=[];
        document.addEventListener('DOMContentLoaded',event=>{
            order.push('capture');event.stopPropagation();document.title=order.join('|');
        },true);
        document.addEventListener('DOMContentLoaded',()=>{
            order.push('target-after-stop');document.title=order.join('|');
        });
    """,
    'pending-256': 'for(let i=0;i<256;i++)setTimeout(()=>{},60000);',
    'canceled-2048': 'for(let i=0;i<2048;i++){let id=setTimeout(()=>{},60000);clearTimeout(id);}',
}
ORDER = ('script:loading|script-job:loading|ready:interactive|ready-job:interactive|'
         'dcl:interactive|dcl-job:interactive|ready:complete|ready-job:complete|'
         'load:complete|load-job:complete')


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def supported_platform():
    require(sys.platform == 'linux' and platform.machine() == 'x86_64'
            and all(hasattr(os, name) for name in ('waitid', 'WNOWAIT', 'WEXITED', 'WNOHANG')),
            'Requires Linux x86_64 and waitid/WNOWAIT owned-group cleanup')


def no_symlinks(path):
    for item in (path, *path.parents):
        require(not item.is_symlink(), 'Symlink paths are not admitted')


def binary_identity(path):
    no_symlinks(path)
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_size == BINARY_BYTES,
            'Binary must be a regular nonsymlink v0.9.10 executable of the exact size')
    require(os.access(path, os.X_OK), 'Binary is not executable')
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        opened = os.fstat(descriptor)
        require(stat.S_ISREG(opened.st_mode)
                and (opened.st_dev, opened.st_ino, opened.st_size)
                == (before.st_dev, before.st_ino, BINARY_BYTES), 'Binary changed before hashing')
        digest, received = hashlib.sha256(), 0
        while received <= BINARY_BYTES:
            chunk = os.read(descriptor, min(65536, BINARY_BYTES + 1 - received))
            if not chunk:
                break
            digest.update(chunk)
            received += len(chunk)
        after = os.fstat(descriptor)
        require(received == BINARY_BYTES and after.st_size == opened.st_size
                and after.st_mtime_ns == opened.st_mtime_ns, 'Binary changed while hashing')
        require(digest.hexdigest() == BINARY_SHA, 'Binary SHA-256 is not the frozen public v0.9.10')
        return (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns)
    finally:
        os.close(descriptor)


def output_path(value):
    path = Path(os.path.abspath(value))
    no_symlinks(path)
    root = ROOT / 'tmp'
    require(path != root and path.is_relative_to(root), 'Output must be a new directory strictly under repo tmp')
    require(path.parent.is_dir(), 'Output parent must already be a directory')
    require(not path.exists(), 'Output already exists; never replace evidence')
    return path


def encode_frame(sequence, revision, kind, payload):
    frame = {'version': 4, 'generation': 31, 'session_id': 47, 'sequence': sequence,
             'expected_revision': revision, 'command': {'kind': kind, 'payload': payload}}
    body = json.dumps(frame, separators=(',', ':'), allow_nan=False).encode()
    require(len(body) < INPUT_LIMIT, 'Parent frame exceeds the fixed input admission')
    return frame, struct.pack('>I', len(body)) + body


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'Duplicate JSON field')
        result[key] = value
    return result


def decode_response(raw):
    require(0 < len(raw) <= LIMIT, 'Invalid/oversized worker frame')
    def finite_float(value):
        number = float(value)
        require(math.isfinite(number), 'Nonfinite JSON')
        return number
    return json.loads(raw, object_pairs_hook=unique_object,
                      parse_float=finite_float,
                      parse_constant=lambda value: require(False, 'Nonfinite JSON'))


class Worker:
    """One owned session; never poll/reap its leader before killing its group."""
    def __init__(self, deadline, interrupted=lambda: False):
        self.deadline, self.interrupted = deadline, interrupted
        self.child = None
        self.cleanup_confirmed = True
        self.eof_observed = False

    def check(self):
        require(not self.interrupted(), 'Probe interrupted')
        require(time.monotonic() < self.deadline, 'Owned worker observation deadline')

    def start(self, command):
        self.check()
        # Probe handlers only record signals, so ownership assignment cannot be
        # interrupted between Popen return and this retained child reference.
        self.child = subprocess.Popen(command, env={}, stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                      start_new_session=True, close_fds=True)
        self.cleanup_confirmed = False
        for pipe in (self.child.stdin, self.child.stdout):
            os.set_blocking(pipe.fileno(), False)

    def wait(self, pipe, write=False):
        self.check()
        readable, writable, _ = select.select([] if write else [pipe], [pipe] if write else [], [],
                                               min(0.025, self.deadline - time.monotonic()))
        return bool(writable if write else readable)

    def exact(self, count):
        require(type(count) is int and 0 < count <= LIMIT, 'Invalid/oversized worker frame')
        value = bytearray()
        while len(value) < count:
            if not self.wait(self.child.stdout):
                continue
            part = os.read(self.child.stdout.fileno(), min(count - len(value), 65536))
            require(bool(part), 'Truncated worker response')
            value.extend(part)
        return bytes(value)

    def exchange(self, framed):
        require(len(framed) < INPUT_LIMIT + 4, 'Oversized parent input')
        offset = 0
        while offset < len(framed):
            if self.wait(self.child.stdin, True):
                count = os.write(self.child.stdin.fileno(), framed[offset:])
                require(count > 0, 'Worker input made no progress')
                offset += count
        size, = struct.unpack('>I', self.exact(4))
        return self.exact(size)

    def finish_eof(self):
        self.child.stdin.close()
        while True:
            self.check()
            if os.waitid(os.P_PID, self.child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None:
                break
            if self.wait(self.child.stdout):
                require(not os.read(self.child.stdout.fileno(), 1), 'Unsolicited worker output')
                select.select([], [], [], min(0.001, max(0, self.deadline - time.monotonic())))
        self.eof_observed = True

    def close(self):
        if self.child is None:
            return None
        deadline = time.monotonic() + CLEANUP_SECONDS
        try:
            try:
                os.killpg(self.child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            code = self.child.wait(timeout=max(0, deadline - time.monotonic()))
            while True:
                try:
                    extra = os.read(self.child.stdout.fileno(), 1)
                    break
                except BlockingIOError:
                    require(time.monotonic() < deadline, 'Worker pipe cleanup deadline')
                    select.select([self.child.stdout], [], [], min(0.025, deadline - time.monotonic()))
            self.cleanup_confirmed = True
            # Ordinary descendants are terminated, not claimed individually reaped.
            require(not extra, 'Unsolicited worker output')
            return code
        finally:
            for pipe in (self.child.stdin, self.child.stdout):
                if not pipe.closed:
                    pipe.close()


def summarize(response, frame):
    require(set(response) == {'version', 'generation', 'session_id', 'sequence', 'expected_revision', 'reply'},
            'Unexpected worker envelope fields')
    for key in ('version', 'generation', 'session_id', 'sequence', 'expected_revision'):
        require(type(response[key]) is int and response[key] == frame[key], 'Worker frame identity mismatch')
    reply = response['reply']
    require(reply['state'] == 'Ready' and reply['errors'] == [], 'Worker did not return diagnostic-free Ready')
    require(reply['revision'] == frame['sequence'] and type(reply['revision']) is int, 'Worker revision mismatch')
    require(reply['navigation'] is None and reply['default_action'] == {'kind': 'None'}
            and reply['acknowledgements'] == []
            and reply['outcome'] == {'click_canceled': None, 'submit_canceled': None}, 'Unexpected worker authority')
    require(type(reply['scripts_executed']) is int
            and reply['scripts_executed'] == (1 if frame['sequence'] == 0 else 0), 'Unexpected startup count')
    stats = reply['boa']
    require(stats['profile'] == 'boa-page-process-v1' and stats['fatal_reason'] is None
            and reply['allocations'] is None, 'Unexpected worker accounting profile/fatal failure')
    for key, maximum in (('jobs_admitted', 2048), ('jobs_executed', 2048), ('pending_jobs', 256),
                         ('opcodes_remaining', 1_000_000)):
        require(type(stats[key]) is int and 0 <= stats[key] <= maximum, 'Invalid bounded work counter')
    require(stats['opcodes_remaining'] > 0
            and stats['jobs_executed'] + stats['pending_jobs'] <= stats['jobs_admitted'], 'Invalid work ledger')
    require(all(type(stats[key]) is int and 0 < stats[key] <= 4 * 1024 * 1024
                for key in ('source_admissions', 'source_bytes')), 'Invalid source counters')
    memory = stats['worker_memory']
    require(set(memory) == {'outstanding_bytes', 'peak_bytes', 'cumulative_bytes', 'allocations',
                            'outstanding_limit_bytes', 'cumulative_limit_bytes'}, 'Unexpected memory report fields')
    require(type(memory['outstanding_limit_bytes']) is int and type(memory['cumulative_limit_bytes']) is int
            and memory['outstanding_limit_bytes'] == 32 * 1024 * 1024
            and memory['cumulative_limit_bytes'] == 64 * 1024 * 1024, 'Changed worker allocation caps')
    require(all(type(memory[key]) is int for key in ('outstanding_bytes', 'peak_bytes', 'cumulative_bytes', 'allocations'))
            and 0 < memory['outstanding_bytes'] <= memory['peak_bytes'] <= 32 * 1024 * 1024
            and memory['peak_bytes'] <= memory['cumulative_bytes'] <= 64 * 1024 * 1024
            and memory['allocations'] > 0, 'Invalid worker memory counters')
    nodes = reply['snapshot']['nodes']
    title_node = next((i for i, node in enumerate(nodes) if node['tag'] == 'title'), None)
    title = ''.join(node['text'] for node in nodes if node['parent'] == title_node) if title_node is not None else ''
    require(len(title) <= 2048, 'Oversized summary title')
    wake = reply.get('task_wake')
    require((stats['pending_jobs'] > 0) == (wake is not None), 'Missing or unexplained wake')
    if wake is not None:
        require(set(wake) == {'token', 'delay_ms'} and type(wake['token']) is int and wake['token'] > 0
                and type(wake['delay_ms']) is int and 0 <= wake['delay_ms'] <= 60000, 'Invalid wake')
    return {'sequence': frame['sequence'], 'revision': reply['revision'], 'title': title,
            'state': reply['state'], 'errors': [], 'jobs_admitted': stats['jobs_admitted'],
            'jobs_executed': stats['jobs_executed'], 'pending_jobs': stats['pending_jobs'],
            'opcodes_remaining': stats['opcodes_remaining'], 'worker_memory': memory,
            'source_admissions': stats['source_admissions'], 'source_bytes': stats['source_bytes'],
            'task_wake': wake, 'fatal_reason': None}


def check_case(name, frames):
    expected = {'empty': [(0, 0, 0, 'before')],
                'ordered': [(7, 5, 2, ORDER), (7, 6, 1, ORDER + '|timer:complete'),
                            (7, 7, 0, ORDER + '|timer:complete|message:complete')],
                'capture-stop': [(0, 0, 0, 'capture|target-after-stop')],
                'pending-256': [(256, 0, 256, 'before')], 'canceled-2048': [(2048, 0, 0, 'before')]}
    require([(item['jobs_admitted'], item['jobs_executed'], item['pending_jobs'], item['title'])
             for item in frames] == expected[name], 'Frozen old lifecycle observation changed: ' + name)


def run_probe(binary, output, identity):
    began = time.monotonic()
    report = {'scope': __doc__, 'binary_sha256': BINARY_SHA, 'binary_bytes': BINARY_BYTES,
              'cases': {}, 'status': 'RUNNING', 'cleanup_confirmed': True, 'wpt_pass_claimed': False,
              'queued_lifecycle_acceptance_claimed': False,
              'input_limit_exclusive': INPUT_LIMIT, 'output_limit_per_frame': LIMIT,
              'child_seconds': CHILD_SECONDS, 'run_seconds': RUN_SECONDS, 'cleanup_seconds': CLEANUP_SECONDS}
    pending_signal = []
    previous = {number: signal.getsignal(number) for number in (signal.SIGINT, signal.SIGTERM)}
    def interrupted(number, _frame):
        if not pending_signal:
            pending_signal.append(number)
    try:
        with Path(__file__).open('rb') as stream:
            helper = stream.read(128 * 1024 + 1)
        require(len(helper) <= 128 * 1024, 'Helper exceeds its own input admission')
        report['helper_sha256'] = hashlib.sha256(helper).hexdigest()
        for number in previous:
            signal.signal(number, interrupted)
        for name, source in SOURCES.items():
            require(not pending_signal and time.monotonic() < began + RUN_SECONDS, 'Probe deadline/interruption')
            require(binary_identity(binary) == identity, 'Executable changed between controls')
            case = {'source_sha256': hashlib.sha256(source.encode()).hexdigest(), 'frames': [],
                    'cleanup_confirmed': True}
            report['cases'][name] = case
            worker = Worker(min(began + RUN_SECONDS, time.monotonic() + CHILD_SECONDS), lambda: bool(pending_signal))
            try:
                worker.start([str(binary), '--script-session'])
                for sequence in range(3 if name == 'ordered' else 1):
                    if sequence == 0:
                        payload = {'url': URL, 'html': '<title>before</title><p>Owned baseline only</p><script>' + source + '</script>'}
                    else:
                        wake = case['frames'][-1]['task_wake']
                        require(wake is not None and wake['delay_ms'] == 0, 'Expected immediate owned task')
                        payload = {'wake_token': wake['token']}
                    frame, framed = encode_frame(sequence, max(0, sequence - 1), 'Init' if sequence == 0 else 'TaskPump', payload)
                    raw = worker.exchange(framed)
                    path = output / f'{name}-{sequence}.json'
                    with path.open('xb') as stream:
                        stream.write(raw)
                    case['last_raw_response'] = path.name
                    response = decode_response(raw)
                    summary = summarize(response, frame)
                    if case['frames']:
                        before = case['frames'][-1]
                        require(summary['opcodes_remaining'] <= before['opcodes_remaining']
                                and all(summary[key] == before[key] for key in ('source_admissions', 'source_bytes'))
                                and all(summary['worker_memory'][key] >= before['worker_memory'][key]
                                        for key in ('peak_bytes', 'cumulative_bytes', 'allocations')),
                                'Worker counters renewed across transactions')
                    summary['raw_response_sha256'] = hashlib.sha256(raw).hexdigest()
                    case['frames'].append(summary)
                check_case(name, case['frames'])
                worker.finish_eof()
            finally:
                try:
                    case['returncode'] = worker.close()
                finally:
                    case['ordinary_eof_exit_observed'] = worker.eof_observed
                    case['cleanup_confirmed'] = worker.cleanup_confirmed
                    report['cleanup_confirmed'] = all(item['cleanup_confirmed'] for item in report['cases'].values())
            require(case['returncode'] == 0 and case['ordinary_eof_exit_observed'], 'Worker did not exit zero on ordinary EOF')
        require(binary_identity(binary) == identity and not pending_signal, 'Executable changed or probe interrupted')
        report['status'] = 'BASELINE_OBSERVED_NOT_QUEUED_LIFECYCLE_ACCEPTANCE'
    except BaseException as error:
        report.update(status='ERROR', error=(type(error).__name__ + ': ' + str(error))[:1024])
    finally:
        try:
            report['elapsed_seconds'] = round(time.monotonic() - began, 3)
            encoded = json.dumps(report, indent=2, allow_nan=False).encode() + b'\n'
            if len(encoded) > RECEIPT_LIMIT:
                report.update(status='ERROR', error='Receipt exceeded its fixed admission; raw frames retained',
                              cases={name: {'cleanup_confirmed': case['cleanup_confirmed'],
                                            'last_raw_response': case.get('last_raw_response')}
                                     for name, case in report['cases'].items()})
                encoded = json.dumps(report, indent=2, allow_nan=False).encode() + b'\n'
            require(len(encoded) <= RECEIPT_LIMIT, 'Receipt exceeds its fixed admission')
            with (output / 'receipt.json').open('xb') as stream:
                stream.write(encoded)
        finally:
            for number, handler in previous.items():
                signal.signal(number, handler)
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True)
    args = parser.parse_args(argv)
    try:
        supported_platform()
        binary = args.binary.absolute()
        identity = binary_identity(binary)
        output = output_path(args.output)
        output.mkdir(mode=0o700)
        report = run_probe(binary, output, identity)
        print(json.dumps({'status': report['status'], 'cleanup_confirmed': report['cleanup_confirmed'],
                          'receipt': str(output / 'receipt.json')}))
        return 0 if report['status'] == 'BASELINE_OBSERVED_NOT_QUEUED_LIFECYCLE_ACCEPTANCE' else 1
    except (OSError, RuntimeError, ValueError) as error:
        print('lifecycle-baseline: ' + str(error), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
