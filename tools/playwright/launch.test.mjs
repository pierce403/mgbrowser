// End-to-end verification of unmodified Playwright 1.58.2 `chromium.launch({ executablePath })`
// over `--remote-debugging-pipe` + `--headless` + `--no-startup-window`, including locators,
// form submission, screenshots, `Browser.setDownloadBehavior` downloads, and `Browser.close`.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { access, readFile } from 'node:fs/promises';
import { constants as fsConstants } from 'node:fs';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const clientVersion = '1.58.2';
const query = 'playwright launch pipe café';
const downloadPayload = 'mgbrowser launch download payload\n';

async function resolveExecutable() {
  const candidates = [
    process.env.MGBROWSER_BIN,
    join(repo, 'tmp/release/mgbrowser-linux-x86_64/mgbrowser'),
    join(repo, 'target/release/mgbrowser'),
    join(repo, 'target/debug/mgbrowser'),
  ].filter(Boolean);
  for (const candidate of candidates) {
    const full = resolve(candidate);
    try {
      await access(full, fsConstants.X_OK);
      return full;
    } catch {
      // Try next candidate.
    }
  }
  throw new Error('Could not find an executable mgbrowser binary (set MGBROWSER_BIN or build release)');
}

function escapeHtml(text) {
  return text.replace(/[&<>"]/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
}

test('chromium.launch drives mgbrowser over --remote-debugging-pipe with locators, screenshot, and downloads', async () => {
  const installed = JSON.parse(await readFile(new URL('node_modules/playwright-core/package.json', import.meta.url), 'utf8'));
  assert.equal(installed.version, clientVersion);
  const executablePath = await resolveExecutable();

  const server = createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://127.0.0.1');
    if (url.pathname === '/') {
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      res.end(`<!doctype html><meta charset="utf-8"><title>Mg Launch Fixture</title>
        <form action="/result" method="get">
          <input id="query" name="q" value="">
          <button id="submit" type="submit">Go</button>
        </form>`);
      return;
    }
    if (url.pathname === '/result') {
      const received = escapeHtml(url.searchParams.get('q') ?? '');
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      res.end(`<!doctype html><meta charset="utf-8"><title>Mg Launch Result</title>
        <p id="received">${received}</p>
        <p><a id="download-link" href="/artifact.txt" download="launch-artifact.txt">Download artifact</a></p>`);
      return;
    }
    if (url.pathname === '/artifact.txt') {
      res.writeHead(200, {
        'content-type': 'text/plain; charset=utf-8',
        'content-disposition': 'attachment; filename="launch-artifact.txt"',
      });
      res.end(downloadPayload);
      return;
    }
    res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' });
    res.end('not found');
  });

  await new Promise((resolveListen, rejectListen) => {
    server.once('error', rejectListen);
    server.listen(0, '127.0.0.1', resolveListen);
  });

  const address = server.address();
  const baseUrl = `http://127.0.0.1:${address.port}/`;
  let browser;
  try {
    browser = await chromium.launch({
      executablePath,
      headless: true,
      timeout: 15000,
    });
    assert.equal(browser.contexts().length, 0);

    const page = await browser.newPage();
    page.setDefaultTimeout(5000);
    page.setDefaultNavigationTimeout(10000);

    const response = await page.goto(baseUrl, { waitUntil: 'load' });
    assert.equal(response?.status(), 200);
    assert.equal(await page.title(), 'Mg Launch Fixture');

    await page.locator('#query').fill(query);
    await Promise.all([
      page.waitForURL(u => u.pathname === '/result' && u.searchParams.get('q') === query),
      page.locator('#submit').click(),
    ]);
    assert.equal(await page.title(), 'Mg Launch Result');
    assert.equal(await page.locator('#received').textContent(), query);

    const png = await page.screenshot();
    assert.deepEqual([...png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);
    assert.ok(png.readUInt32BE(16) > 0 && png.readUInt32BE(20) > 0);

    const [download] = await Promise.all([
      page.waitForEvent('download'),
      page.locator('#download-link').click(),
    ]);
    assert.equal(download.suggestedFilename(), 'launch-artifact.txt');
    const savedPath = await download.path();
    assert.ok(savedPath, 'download.path() must resolve under allowAndName');
    assert.equal(await readFile(savedPath, 'utf8'), downloadPayload);

    await page.close();
    await browser.close();
    browser = undefined;
  } finally {
    await browser?.close().catch(() => {});
    await new Promise(resolveClose => server.close(resolveClose));
  }
});
