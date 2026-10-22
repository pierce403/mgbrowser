#!/usr/bin/env python3
"""Authored packaged/public native and external-CDP queued lifecycle acceptance.

Uses the existing bounded owned process/display infrastructure. No build,
download, public website, Runtime execution or WPT/testharness result is implied.
Build journey_server, cdp_journey and lifecycle_journey before running.
"""
import argparse
from collections import Counter
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sys
import tempfile

sys.dont_write_bytecode = True
REPO = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('lifecycle_message_services', REPO / 'tools/message-services-smoke.py')
owned = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(owned)
require = owned.require
FIXTURE = REPO / 'tests/fixtures/script/lifecycle-events.html'
DEFERRED = REPO / 'tests/fixtures/script/lifecycle-deferred.js'
URL = 'http://127.0.0.1:7878/lifecycle-events'
ORDER = ('script:loading,script-job:loading,ready:interactive,ready-job:interactive,'
         'defer:interactive,defer-job:interactive,early-message:interactive,message-job:interactive,'
         'early-timer:interactive,timer-job:interactive,dcl-capture:interactive,dcl:interactive,'
         'dcl-job:interactive,dcl-bubble:interactive,ready:complete,ready-job:complete,load:complete,load-job:complete')
TRACE = [(8, 3, 5), (9, 5, 4), (10, 7, 3), (11, 9, 2), (14, 12, 2), (15, 14, 1)]
EVENTS = ['Page.frameStartedLoading', 'Page.frameNavigated', 'Page.domContentEventFired',
          'Page.loadEventFired', 'Page.frameStoppedLoading']


def check_worker(log, name):
    for marker in ('SCRIPT_PARTIAL', 'SCRIPT_DIAGNOSTIC', 'SCRIPT_ERROR', 'SCRIPT_REJECTED',
                   'SCRIPT_SESSION_CLOSED', 'SCRIPT_SHUTDOWN_ERROR', 'LOAD_ERROR',
                   'RESOURCE_WARNING', 'RESOURCE_DIAGNOSTIC', 'JOURNEY_INCOMPLETE'):
        require(marker not in log, f'{name}: unexpected diagnostic/failure {marker}')
    reports = [json.loads(line.removeprefix('SCRIPT_BOA ')) for line in log.splitlines()
               if line.startswith('SCRIPT_BOA ')]
    require(6 <= len(reports) <= 256, f'{name}: missing or excessive worker evidence')
    for report in reports:
        fields = ('opcodes_remaining', 'jobs_admitted', 'jobs_executed', 'pending_jobs',
                  'source_admissions', 'source_bytes')
        require(type(report) is dict and all(type(report.get(field)) is int for field in fields),
                f'{name}: malformed typed worker accounting')
        require(report.get('profile') == 'boa-page-process-v1' and 'fatal_reason' in report
                and report['fatal_reason'] is None and 0 < report['opcodes_remaining'] <= 1_000_000
                and 0 <= report['pending_jobs'] <= 256
                and 0 <= report['jobs_executed'] <= report['jobs_admitted'] <= 2048
                and report['pending_jobs'] <= report['jobs_admitted'] - report['jobs_executed']
                and 0 <= report['source_admissions'] <= report['source_bytes'] <= 4 * 1024 * 1024,
                f'{name}: failed or invalid unchanged worker profile')
        memory = report.get('worker_memory') or {}
        fields = ('outstanding_limit_bytes', 'cumulative_limit_bytes', 'outstanding_bytes',
                  'peak_bytes', 'cumulative_bytes', 'allocations')
        require(all(type(memory.get(field)) is int for field in fields)
                and memory['outstanding_limit_bytes'] == 32 * 1024 * 1024
                and memory['cumulative_limit_bytes'] == 64 * 1024 * 1024
                and 0 < memory['outstanding_bytes'] <= memory['peak_bytes'] <= 32 * 1024 * 1024
                and memory['peak_bytes'] <= memory['cumulative_bytes'] <= 64 * 1024 * 1024
                and 0 < memory['allocations'] <= memory['cumulative_bytes'],
                f'{name}: missing/changed System allocation bounds')
    inline = owned.control.read_artifact(FIXTURE, limit=owned.LOG_LIMIT).split(b'<script>', 1)[1].split(b'</script>', 1)[0]
    source_key = (2, len(inline) + len(owned.control.read_artifact(DEFERRED, limit=owned.LOG_LIMIT)))
    lifecycle = owned.services.check_lifecycle_tasks(log, reports, source_key, TRACE, name)
    realm = [item for item in reports if (item['source_admissions'], item['source_bytes']) == source_key]
    for before, after in zip(realm, realm[1:]):
        require(after['opcodes_remaining'] <= before['opcodes_remaining']
                and all(after[key] >= before[key] for key in ('jobs_admitted', 'jobs_executed'))
                and all(after['worker_memory'][key] >= before['worker_memory'][key]
                        for key in ('peak_bytes', 'cumulative_bytes', 'allocations')),
                f'{name}: original realm renewed cumulative resource counters')
    require(log.splitlines().count('SCRIPT_COMPLETE executed=2') == 1,
            f'{name}: original inline and deferred sources did not execute exactly once')
    return {'work': [list(item) for item in TRACE], 'long_timer_left_pending': 1, **lifecycle}


def check_probe(log):
    lines = [line.removeprefix('CDP_LIFECYCLE_OK ') for line in log.splitlines()
             if line.startswith('CDP_LIFECYCLE_OK ')]
    require(len(lines) == 1, 'Missing or repeated external lifecycle receipt')
    report = json.loads(lines[0])
    evidence = report.get('evidence') or {}
    require(report.get('scope') == 'authored queued lifecycle, not WPT/testharness completion'
            and report.get('order') == ORDER and evidence.get('events') == EVENTS
            and evidence.get('later_dom_projection') is True,
            'External client did not verify exact lifecycle/order/later projection')
    dcl, load = evidence.get('dcl_timestamp'), evidence.get('load_timestamp')
    require(type(dcl) in (int, float) and type(load) in (int, float)
            and 0 <= dcl <= load < float('inf') and isinstance(evidence.get('frame_id'), str)
            and bool(evidence['frame_id']), 'Missing finite monotonic receipt timestamps/frame')
    return report


def start_browser(run, binary, label):
    browser = run.start([binary, 'http://127.0.0.1:7878/', '--enable-scripts', '--remote-debugging-port=0'], label + '.log')
    endpoints = []
    def ready():
        match = re.search(r'^CDP listening on (ws://127\.0\.0\.1:(\d+)/[^ ]+)', owned.live_log(run.root / (label + '.log')), re.M)
        if not match or not owned.services.listener_owned(browser.pid, int(match[2])):
            return False
        connection = owned.http.client.HTTPConnection('127.0.0.1', int(match[2]), timeout=1)
        try:
            connection.request('GET', '/json/version')
            response = connection.getresponse()
            body = response.read(owned.LOG_LIMIT + 1)
            require(response.status == 200 and len(body) <= owned.LOG_LIMIT
                    and json.loads(body).get('webSocketDebuggerUrl') == match[1], 'Owned CDP discovery mismatch')
            connection.close()
            connection = owned.http.client.HTTPConnection('127.0.0.1', int(match[2]), timeout=1)
            connection.request('GET', '/json/list')
            response = connection.getresponse()
            body = response.read(owned.LOG_LIMIT + 1)
            require(response.status == 200 and len(body) <= owned.LOG_LIMIT, 'Owned page discovery overflow/status')
            pages = json.loads(body)
            expected = f'ws://127.0.0.1:{match[2]}/devtools/page/page-1'
            require(type(pages) is list and len(pages) == 1 and pages[0].get('id') == 'page-1'
                    and pages[0].get('webSocketDebuggerUrl') == expected, 'Owned page-1 identity mismatch')
            endpoints[:] = [match[1], expected, int(match[2])]
            return True
        except OSError:
            return False
        finally:
            connection.close()
    owned.wait_for(run, browser, ready, 'owned lifecycle CDP listener')
    return browser, endpoints


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    binary = parser.parse_args().binary.resolve(strict=True)
    helpers = [REPO / 'target/debug/examples' / name for name in ('journey_server', 'cdp_journey', 'lifecycle_journey')]
    for path in [binary, *helpers]:
        require(path.is_file() and os.access(path, os.X_OK), f'Missing executable: {path}')
    for tool in ('Xvfb', 'xauth'):
        require(shutil.which(tool), f'Missing acceptance tool: {tool}')
    root = Path(tempfile.mkdtemp(prefix='lifecycle-services-smoke-', dir=REPO / 'tmp'))
    print(f'LIFECYCLE_SERVICES_ARTIFACTS {root}', flush=True)
    private = root / 'home'; private.mkdir()
    env = dict(os.environ, HOME=str(private), XDG_CONFIG_HOME=str(private / 'config'),
               XDG_DATA_HOME=str(private / 'data'), XDG_CACHE_HOME=str(private / 'cache'),
               XDG_STATE_HOME=str(private / 'state'), XDG_RUNTIME_DIR=str(private / 'runtime'),
               DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent-lifecycle-services-session',
               MGBROWSER_NO_AUTO_UPDATE='1', MESA_SHADER_CACHE_DISABLE='true')
    for key in ('DISPLAY', 'XAUTHORITY', 'MGBROWSER_INSTALL_DIR'): env.pop(key, None)
    Path(env['XDG_RUNTIME_DIR']).mkdir(mode=0o700)
    paths = [binary, *helpers, FIXTURE, DEFERRED, Path(__file__).resolve(),
             REPO / 'tools/message-services-smoke.py', REPO / 'tools/page-services-smoke.py',
             REPO / 'tools/wpt/process_control.py', REPO / 'tests/fixtures/journey/home.html',
             REPO / 'tests/fixtures/script/event-results.html']
    hashes = {str(path): owned.services.sha256(path) for path in paths}
    report = {'schema_version': 1, 'status': 'ERROR', 'binary': str(binary), 'input_sha256': hashes,
              'scope': __doc__, 'resource_evidence': 'Exact authored source/work/HTTP receipts; hidden resource-warning count unavailable'}
    run = owned.Run(root, env)
    report['commands'] = run.commands
    report['handoff_failures'] = run.handoff_failures
    try:
        server = owned.start_journey(run, helpers[0]); owned.start_display(run)
        native = run.command([binary, URL, '--enable-scripts', '--smoke-events', '--exit-after-smoke', '--evidence-dir', root / 'native'], 'native.log')
        require('JOURNEY_EVENTS_COMPLETE' in native, 'Native lifecycle retained-form journey incomplete')
        report['native'] = check_worker(native, 'native')
        for name in ('01-event-query.png', '02-link-canceled.png', '03-submit-canceled.png', '04-event-results.png', '05-event-destination.png'): owned.png(root / 'native' / name)
        probe, endpoints = start_browser(run, binary, 'lifecycle-browser')
        report['external_lifecycle'] = check_probe(run.command([helpers[2], endpoints[1]], 'lifecycle-client.log'))
        require(probe.poll() is None and owned.services.listener_owned(probe.pid, endpoints[2]), 'Lost lifecycle probe browser ownership')
        probe.stop()
        report['external_lifecycle_work'] = check_worker(owned.bounded_text(root / 'lifecycle-browser.log'), 'external lifecycle')
        browser, endpoints = start_browser(run, binary, 'cdp-browser')
        client = run.command([helpers[1], endpoints[0], URL, root / 'cdp-destination.png'], 'cdp-client.log')
        for marker in ('CDP_EVENTS_STATE link canceled', 'CDP_EVENTS_STATE first submit canceled', 'CDP_EVENTS_DESTINATION', 'CDP_EVENTS_OK'):
            require(marker in client, f'Missing unchanged retained CDP guard: {marker}')
        require(client.splitlines().count(f'CDP_LIFECYCLE_FORM_OK {ORDER} later=after-load-task-and-checkpoint') == 1, 'CDP did not verify actual lifecycle-created form')
        require(browser.poll() is None and owned.services.listener_owned(browser.pid, endpoints[2]), 'Lost retained CDP browser ownership')
        browser.stop(); owned.png(root / 'cdp-destination.png')
        report['external_form'] = check_worker(owned.bounded_text(root / 'cdp-browser.log'), 'external retained form')
        require(server.poll() is None and owned.services.listener_owned(server.pid, 7878), 'Lost owned fixture listener')
        server.stop()
        requests = Counter(line for line in owned.bounded_text(root / 'journey-server.log').splitlines() if re.match(r'^\d{3} ', line))
        expected = Counter({'200 OK /': 3, '200 OK /lifecycle-events': 3, '200 OK /lifecycle-deferred.js': 3,
                            '200 OK /event-search': 2, '200 OK /event-destination': 2})
        require(requests == expected, f'Unexpected/missing HTTP or trap requests: {dict(requests)}')
        require(all(owned.services.sha256(Path(path)) == digest for path, digest in hashes.items()), 'Acceptance input/executable changed')
        run.guard(); report.update(status='PASS', http_requests=dict(requests), inputs_unchanged=True, trap_requests=0)
    except BaseException as error:
        report['error'] = f'{type(error).__name__}: {error}'
        raise
    finally:
        try:
            run.close()
            report['cleanup'] = {'confirmed': True, 'leaders': len(run.children),
                                 'returncodes': [child.process.returncode for child in run.children],
                                 'scope': 'owned groups terminated and direct leaders reaped; no individual descendant reap claim'}
        except BaseException as error:
            report.update(status='ERROR', cleanup={'confirmed': False, 'error': f'{type(error).__name__}: {error}'})
            raise
        finally:
            if run.failure: report.update(status='ERROR', watchdog_error=run.failure)
            if run.deferred_signal is not None: report['deferred_signal'] = run.deferred_signal
            if report.get('cleanup', {}).get('confirmed') is True:
                try: report['evidence_bytes_before_report'] = owned.evidence_size(root)
                except BaseException as error: report.update(status='ERROR', evidence_error=f'{type(error).__name__}: {error}')
            encoded = (json.dumps(report, indent=2) + '\n').encode()
            require(len(encoded) <= owned.LOG_LIMIT, 'Lifecycle report metadata overflow')
            with (root / 'report.json').open('xb') as stream: stream.write(encoded)
        require(report['status'] == 'PASS', 'Lifecycle acceptance or cleanup/evidence admission failed')
    print(f'LIFECYCLE_SERVICES_SMOKE_OK {hashes[str(binary)]} {root}', flush=True)


if __name__ == '__main__':
    def interrupted(_signum, _frame): raise KeyboardInterrupt('Acceptance interrupted; cleaning owned processes')
    signal.signal(signal.SIGTERM, interrupted)
    main()
