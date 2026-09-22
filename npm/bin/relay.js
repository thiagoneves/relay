#!/usr/bin/env node
// Thin launcher so `npx -y @thiagoneves/relay claude` works on a fresh
// machine. It finds the native relay binary for this platform (downloading
// the matching release once, checked against its published SHA-256) and
// runs it with the same arguments. `relay claude|codex|gemini` and `relay
// setup` copy it to ~/.local/bin; hooks point there, never to npx.
'use strict';

const { spawnSync, execFileSync } = require('node:child_process');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const pkg = require('../package.json');

const REPO = 'thiagoneves/relay';

/** Release target triple per `platform arch`, as the release workflow names them. */
const TARGETS = {
  'darwin arm64': 'aarch64-apple-darwin',
  'darwin x64': 'x86_64-apple-darwin',
  'linux x64': 'x86_64-unknown-linux-gnu',
  'win32 x64': 'x86_64-pc-windows-msvc',
};

function target(platform = process.platform, arch = process.arch) {
  return TARGETS[`${platform} ${arch}`];
}

function exeName(platform = process.platform) {
  return platform === 'win32' ? 'relay.exe' : 'relay';
}

function archiveName(version, triple) {
  const ext = triple.includes('windows') ? 'zip' : 'tar.gz';
  return `relay-v${version}-${triple}.${ext}`;
}

/** The hash SHA256SUMS lists for `name`, or undefined. */
function expectedHash(sums, name) {
  for (const line of sums.split('\n')) {
    const [hash, file] = line.trim().split(/\s+\*?/);
    if (file === name) return hash;
  }
  return undefined;
}

function cacheRoot() {
  if (process.platform === 'win32' && process.env.LOCALAPPDATA) {
    return path.join(process.env.LOCALAPPDATA, 'relay', 'cache');
  }
  return path.join(process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache'), 'relay');
}

function versionOf(bin) {
  try {
    return execFileSync(bin, ['--version'], { encoding: 'utf8' }).trim().split(/\s+/).pop();
  } catch {
    return undefined;
  }
}

async function download(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return Buffer.from(await res.arrayBuffer());
}

/** Download, verify and unpack this version's binary into `dir`. */
async function install(dir, version, triple) {
  const base = `https://github.com/${REPO}/releases/download/v${version}/`;
  const name = archiveName(version, triple);
  const expected = expectedHash((await download(base + 'SHA256SUMS')).toString('utf8'), name);
  if (!expected) throw new Error(`SHA256SUMS of v${version} does not list ${name}`);
  const archive = await download(base + name);
  const actual = crypto.createHash('sha256').update(archive).digest('hex');
  if (actual !== expected) throw new Error(`${name} does not match its published SHA-256`);

  const work = fs.mkdtempSync(path.join(os.tmpdir(), 'relay-'));
  try {
    const file = path.join(work, name);
    fs.writeFileSync(file, archive);
    // tar ships with macOS, Linux and Windows 10+, and reads zip there too.
    execFileSync('tar', ['-xf', file, '-C', work]);
    const unpacked = path.join(work, name.replace(/\.(tar\.gz|zip)$/, ''), exeName());
    fs.mkdirSync(dir, { recursive: true });
    fs.copyFileSync(unpacked, path.join(dir, exeName()));
    fs.chmodSync(path.join(dir, exeName()), 0o755);
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

async function resolveBinary() {
  if (process.env.RELAY_BINARY) return process.env.RELAY_BINARY;
  const installed = path.join(os.homedir(), '.local', 'bin', exeName());
  if (fs.existsSync(installed) && versionOf(installed) === pkg.version) return installed;
  const cached = path.join(cacheRoot(), pkg.version, exeName());
  if (fs.existsSync(cached)) return cached;
  const triple = target();
  if (!triple) {
    throw new Error(
      `no prebuilt relay for ${process.platform} ${process.arch}; build it from source: https://github.com/${REPO}`,
    );
  }
  await install(path.dirname(cached), pkg.version, triple);
  return cached;
}

async function main() {
  let bin;
  try {
    bin = await resolveBinary();
  } catch (e) {
    process.stderr.write(`✗ relay could not be installed: ${e.message}\n`);
    process.stderr.write(`→ Check your connection, or set RELAY_BINARY to a relay you built.\n`);
    process.exit(1);
  }
  const run = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' });
  if (run.error) {
    process.stderr.write(`✗ could not run ${bin}: ${run.error.message}\n`);
    process.exit(1);
  }
  process.exit(run.status ?? 1);
}

if (require.main === module) main();

module.exports = { target, exeName, archiveName, expectedHash };
