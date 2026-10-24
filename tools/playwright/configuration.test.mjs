import assert from 'node:assert/strict';
import test from 'node:test';
import { browserConfiguration } from './configuration.mjs';

test('the ordinary-client fixture explicitly disables page scripts and updates', () => {
  const fixture = 'http://127.0.0.1:43210/';
  const configuration = browserConfiguration(fixture);
  assert.deepEqual(configuration.args, [
    '--remote-debugging-port=0', '--no-auto-update', '--disable-scripts', fixture,
  ]);
  assert.equal(configuration.scriptsEnabled, false);
  assert.equal(configuration.args.includes('--enable-scripts'), false);
});

test('each browser gets independent arguments with its actual fixture URL', () => {
  const first = browserConfiguration('http://127.0.0.1:43210/');
  first.args.pop();
  const second = browserConfiguration('http://127.0.0.1:43211/');
  assert.equal(second.args.at(-1), 'http://127.0.0.1:43211/');
  assert.equal(second.args.length, 4);
});
