// An ordinary pinned Playwright client against a real owned mgbrowser window.
// No Chromium download, injected protocol shim, expected-failure pass or live site.
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { createWriteStream } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const self = fileURLToPath(import.meta.url);
const repo = fileURLToPath(new URL('../../', import.meta.url));
const clientVersion = '1.58.2';
const query = 'rust browser café';

async function client(endpoint, fixture, directory) {
  // The parent enables pw:protocol before this unmodified package is imported.
  const { chromium } = await import('playwright-core');
  const installed = JSON.parse(await readFile(new URL('node_modules/playwright-core/package.json', import.meta.url)));
  assert.equal(installed.version, clientVersion);
  const report = { clientVersion, endpoint, steps: [], events: [], passed: false };
  const event = (kind, text) => {
    if (report.events.length < 128) report.events.push({ kind, text: String(text).slice(0, 2048) });
  };
  let browser;
  let stage = 'connect';
  try {
    browser = await chromium.connectOverCDP(endpoint, { timeout: 10000 });
    report.steps.push(stage);
    stage = 'existing context/page discovery';
    assert.equal(browser.contexts().length, 1);
    const context = browser.contexts()[0];
    assert.equal(context.pages().length, 1);
    const original = context.pages()[0];
    assert.equal(await original.title(), 'Mg Playwright fixture');
    report.steps.push(stage);
    stage = 'create a distinct stable page';
    const page = await context.newPage();
    assert.equal(context.pages().length, 2);
    assert.notEqual(page, original);
    page.setDefaultTimeout(5000);
    page.setDefaultNavigationTimeout(10000);
    page.on('console', message => event(`console.${message.type()}`, message.text()));
    page.on('pageerror', error => event('pageerror', error.message));
    page.on('requestfailed', request => event('requestfailed', `${request.url()}: ${request.failure()?.errorText}`));
    page.on('response', response => event('response', `${response.status()} ${response.url()}`));
    report.steps.push(stage);

    stage = 'navigate/title';
    const response = await page.goto(fixture, { waitUntil: 'load' });
    assert.equal(response?.status(), 200);
    assert.equal(await page.title(), 'Mg Playwright fixture');
    report.steps.push(stage);
    stage = 'locator fill/click and actual form navigation';
    await page.locator('#query').fill(query);
    await Promise.all([
      page.waitForURL(url => url.pathname === '/result' && url.searchParams.get('q') === query),
      page.locator('#submit').click(),
    ]);
    assert.equal(await page.title(), 'Mg Playwright result');
    assert.equal(await page.locator('#received').textContent(), query);
    report.steps.push(stage);
    stage = 'viewport screenshot';
    const png = await page.screenshot({ path: join(directory, 'page.png') });
    assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
    assert.ok(png.readUInt32BE(16) > 0 && png.readUInt32BE(20) > 0);
    report.steps.push(stage);
    stage = 'missing CSS response diagnostics';
    await page.goto(`${fixture}diagnostics`, { waitUntil: 'load' });
    assert.ok(report.events.some(e => e.kind === 'response' && e.text === `404 ${fixture}missing.css`), 'missing.css HTTP404 must be observable by Playwright');
    report.steps.push(stage);
    stage = 'unsupported protocol error';
    const cdp = await context.newCDPSession(page);
    await assert.rejects(cdp.send('MgAcceptance.notImplemented'), /not found|not implemented|unknown|unsupported|-32601/i);
    await cdp.detach();
    report.steps.push(stage);
    stage = 'close only the new page';
    await page.close();
    assert.deepEqual(context.pages(), [original]);
    assert.equal(await original.title(), 'Mg Playwright fixture');
    assert.equal(new URL(original.url()).pathname, '/');
    assert.equal(original.isClosed(), false);
    report.steps.push(stage);
    report.passed = true;
  } catch (error) {
    report.failedStage = stage;
    report.error = String(error?.stack || error);
    console.error(`PLAYWRIGHT_UNSUPPORTED at ${stage}: ${error.message}`);
    process.exitCode = 1;
  } finally {
    await browser?.close().catch(() => {});
    await writeFile(join(directory, 'result.json'), `${JSON.stringify(report, null, 2)}\n`);
  }
}

async function until(predicate, label, timeout = 10000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const value = predicate();
    if (value) return value;
    await delay(25);
  }
  throw new Error(`Timed out: ${label}`);
}

async function run(binary) {
  const installed = JSON.parse(await readFile(new URL('node_modules/playwright-core/package.json', import.meta.url)));
  assert.equal(installed.version, clientVersion, 'Run npm ci in tools/playwright to install the pinned client');
  const executable = resolve(binary);
  const version = execFileSync(executable, ['--version'], { encoding: 'utf8', timeout: 5000 }).trim();
  await mkdir(join(repo, 'tmp'), { recursive: true });
  const directory = await mkdtemp(join(repo, 'tmp', 'playwright-acceptance-'));
  console.log(`Evidence: ${directory}\nBinary: ${version}`);
  const children = [];
  const logs = [];
  let interrupted = false;
  const stopChildren = () => { interrupted = true; for (const child of children) if (child.exitCode === null) child.kill('SIGTERM'); };
  process.once('SIGINT', stopChildren);
  process.once('SIGTERM', stopChildren);
  const start = (command, args, options, name) => {
    const child = spawn(command, args, options);
    child.on('error', error => { child.spawnError = error; });
    children.push(child);
    const log = createWriteStream(join(directory, `${name}.log`));
    logs.push(log);
    child.stdout?.pipe(log, { end: false });
    child.stderr?.pipe(log, { end: false });
    return child;
  };
  const requests = [];
  const server = createServer((request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    let status = 200;
    let html = '';
    if (url.pathname === '/') html = '<!doctype html><title>Mg Playwright fixture</title><h1>Local automation fixture</h1><form action="/result"><label for="query">Query</label><input id="query" name="q"><button id="submit">Go</button></form>';
    else if (url.pathname === '/result') {
      const value = url.searchParams.get('q');
      status = value === query ? 200 : 400;
      html = `<title>Mg Playwright result</title><p id="received">${value === query ? query : 'Invalid query'}</p>`;
    } else if (url.pathname === '/diagnostics') html = '<title>Mg diagnostic fixture</title><link rel="stylesheet" href="/missing.css"><p>Missing CSS is intentional</p>';
    else { status = 404; html = 'Intentional missing fixture resource'; }
    if (requests.length < 256) requests.push({ method: request.method, path: request.url, status });
    response.writeHead(status, { 'Content-Type': url.pathname.endsWith('.css') ? 'text/css; charset=utf-8' : 'text/html; charset=utf-8' });
    response.end(html);
  });
  try {
    await new Promise((accept, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', accept); });
    const fixture = `http://127.0.0.1:${server.address().port}/`;
    const profileHome = join(directory, 'home');
    const runtime = join(directory, 'runtime');
    await mkdir(profileHome, { recursive: true });
    await mkdir(runtime, { mode: 0o700 });
    const xvfb = start('Xvfb', ['-displayfd', '3', '-screen', '0', '1280x900x24', '-nolisten', 'tcp'], { stdio: ['ignore', 'pipe', 'pipe', 'pipe'] }, 'xvfb');
    let display = '';
    xvfb.stdio[3].on('data', chunk => { display += chunk; });
    await until(() => { if (xvfb.spawnError) throw xvfb.spawnError; return /^\d+\n/.test(display); }, 'owned Xvfb display');
    const env = { ...process.env, HOME: profileHome, XDG_CONFIG_HOME: join(profileHome, 'config'), XDG_DATA_HOME: join(profileHome, 'data'), XDG_CACHE_HOME: join(profileHome, 'cache'), XDG_RUNTIME_DIR: runtime, DISPLAY: `:${display.trim()}`, DBUS_SESSION_BUS_ADDRESS: `unix:path=${runtime}/no-session-bus`, MGBROWSER_NO_AUTO_UPDATE: '1' };
    delete env.XAUTHORITY;
    delete env.WAYLAND_DISPLAY;
    const native = start(executable, ['--remote-debugging-port=0', '--no-auto-update', fixture], { env, stdio: ['ignore', 'pipe', 'pipe'] }, 'browser');
    let browserLog = '';
    native.stderr.on('data', chunk => { browserLog = (browserLog + chunk).slice(-1048576); });
    const websocket = await until(() => {
      if (native.spawnError) throw native.spawnError;
      if (native.exitCode !== null) throw new Error(`mgbrowser exited ${native.exitCode}`);
      const found = browserLog.match(/CDP listening on (ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/[^\s]+)/);
      return found && /LOADED .*HTTP 200/.test(browserLog) && found[1];
    }, 'native page load and CDP endpoint');
    const base = websocket.replace('ws:', 'http:').split('/devtools/')[0];
    const results = [];
    for (const [name, endpoint] of [['http', base], ['websocket', websocket]]) {
      if (interrupted) throw new Error('Interrupted');
      const output = join(directory, name);
      await mkdir(output);
      const probe = start(process.execPath, [self, '--client', endpoint, fixture, output], { env: { ...process.env, DEBUG: 'pw:protocol' }, stdio: ['ignore', 'pipe', 'pipe'] }, `${name}-protocol`);
      await until(() => {
        if (probe.spawnError) throw probe.spawnError;
        if (probe.signalCode !== null) throw new Error(`${name} client terminated by ${probe.signalCode}`);
        return probe.exitCode !== null;
      }, `${name} acceptance`, 60000);
      const result = JSON.parse(await readFile(join(output, 'result.json'), 'utf8'));
      results.push(result);
      console.log(`${name}: ${result.passed ? 'PASS' : `FAIL at ${result.failedStage}: ${result.error.split('\n')[0]}`}`);
    }
    const summary = { version, clientVersion, scriptsEnabled: false, fixture, results };
    await writeFile(join(directory, 'summary.json'), `${JSON.stringify(summary, null, 2)}\n`);
    if (!results.every(result => result.passed)) throw new Error('Playwright acceptance failed; see exact protocol logs. Unsupported behavior is not a passing test.');
    console.log('PLAYWRIGHT_ACCEPTANCE_OK');
  } finally {
    stopChildren();
    await delay(150);
    for (const child of children) if (child.exitCode === null) child.kill('SIGKILL');
    await Promise.all(children.map(child => child.spawnError || child.exitCode !== null || child.signalCode !== null ? undefined : new Promise(done => child.once('exit', done))));
    for (const log of logs) await new Promise(done => log.end(done));
    server.closeAllConnections();
    await new Promise(done => server.close(done));
    await writeFile(join(directory, 'requests.json'), `${JSON.stringify(requests, null, 2)}\n`);
    process.removeListener('SIGINT', stopChildren);
    process.removeListener('SIGTERM', stopChildren);
  }
}

try {
  if (process.argv[2] === '--client') await client(...process.argv.slice(3));
  else {
    assert.equal(process.argv.length, 3, 'Usage: node tools/playwright/acceptance.mjs PATH_TO_MGBROWSER');
    await run(process.argv[2]);
  }
} catch (error) {
  console.error(error.stack || error);
  process.exitCode = 1;
}
