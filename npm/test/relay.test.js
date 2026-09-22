'use strict';

const test = require('node:test');
const assert = require('node:assert');

const { target, exeName, archiveName, expectedHash } = require('../bin/relay.js');

test('each supported platform maps to a release target', () => {
  assert.strictEqual(target('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.strictEqual(target('linux', 'x64'), 'x86_64-unknown-linux-gnu');
  assert.strictEqual(target('win32', 'x64'), 'x86_64-pc-windows-msvc');
  assert.strictEqual(target('linux', 'arm64'), undefined);
  assert.strictEqual(exeName('win32'), 'relay.exe');
});

test('archive names match what the release workflow publishes', () => {
  assert.strictEqual(archiveName('0.1.0', 'aarch64-apple-darwin'), 'relay-v0.1.0-aarch64-apple-darwin.tar.gz');
  assert.strictEqual(archiveName('0.1.0', 'x86_64-pc-windows-msvc'), 'relay-v0.1.0-x86_64-pc-windows-msvc.zip');
});

test('the expected hash comes from the matching SHA256SUMS line', () => {
  const sums = 'aaa  relay-v0.1.0-x86_64-apple-darwin.tar.gz\nbbb  relay-v0.1.0-aarch64-apple-darwin.tar.gz\n';
  assert.strictEqual(expectedHash(sums, 'relay-v0.1.0-aarch64-apple-darwin.tar.gz'), 'bbb');
  assert.strictEqual(expectedHash(sums, 'relay-v0.1.0-x86_64-pc-windows-msvc.zip'), undefined);
});
