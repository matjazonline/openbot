import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { createHash } from 'node:crypto';
const records = new URL('.', import.meta.url);
const artifact = '/tmp/openbot-cloudroom-p06-artifacts-r4/bb-plugin-openbot-local-0.2.0.tgz';
const source = '/tmp/openbot-cloudroom-d8b3cc3';
const directory = mkdtempSync(join(tmpdir(), 'openbot-packaged-smoke-'));
const plugin = join(directory, 'plugin'); const profile = join(directory, 'profile'); mkdirSync(plugin);
const integrity = JSON.parse(readFileSync(artifact + '.integrity.json'));
assert.equal(createHash('sha256').update(readFileSync(artifact)).digest('hex'), integrity.sha256);
execFileSync('tar', ['-xzf', artifact, '-C', plugin, '--strip-components=1']);
execFileSync('npm', ['ci', '--omit=dev'], { cwd: plugin, stdio: 'pipe', timeout: 180_000, maxBuffer: 1024 * 1024 });
const port = async () => { const server = createServer(); server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const value = server.address().port; await new Promise(done => server.close(done)); return value; };
const ports = [await port(), await port(), await port()]; const origin = `http://127.0.0.1:${ports[0]}`;
const args = [join(plugin, 'services/run.mjs'), '--source', source, '--profile', profile, '--server-port', String(ports[0]),
  '--daemon-port', String(ports[1]), '--ui-port', String(ports[2])];
const outputs = []; const steps = [];
let service;
const start = async () => {
  service = spawn(process.execPath, args, { stdio: ['ignore', 'pipe', 'pipe'] });
  const output = [];
  service.stdout.on('data', part => { output.push(part); outputs.push(part); assert(Buffer.concat(output).length < 64 * 1024); });
  service.stderr.on('data', part => outputs.push(part));
  const deadline = performance.now() + 60_000;
  while (performance.now() < deadline) {
    if (Buffer.concat(output).toString().includes('"state":"ready"')) return;
    if (service.exitCode !== null) throw new Error('packaged launcher exited: ' + Buffer.concat(output).toString());
    await new Promise(done => setTimeout(done, 100));
  }
  throw new Error('packaged launcher did not become ready');
};
const stop = async () => {
  if (!service || service.exitCode !== null) return;
  const exited = once(service, 'exit'); service.kill('SIGTERM');
  const timer = setTimeout(() => service.kill('SIGKILL'), 12_000);
  try { const [code] = await exited; assert.equal(code, 0); } finally { clearTimeout(timer); }
};
const cli = args => execFileSync(process.execPath, [resolve(source, 'gui/apps/cli/dist/index.js'), ...args], {
  cwd: resolve(source, 'gui'), env: { ...process.env, NODE_ENV: 'production', BB_DATA_DIR: profile, BB_SERVER_URL: origin },
  encoding: 'utf8', timeout: 30_000, maxBuffer: 256 * 1024 });
try {
  await start(); steps.push('packaged wrapper booted API/daemon/UI on loopback');
  const duplicate = spawn(process.execPath, args, { stdio: 'ignore' });
  assert.notEqual((await once(duplicate, 'exit'))[0], 0); steps.push('competing same-profile wrapper refused');
  cli(['plugin', 'install', `path:${plugin}`, '--yes']);
  const before = JSON.parse(cli(['plugin', 'run', 'openbot-local', 'status']));
  assert.equal(before.paired, false); assert.equal(before.connection, 'offline'); steps.push('packaged public CLI install/status');
  cli(['plugin', 'reload', 'openbot-local']);
  assert.equal(JSON.parse(cli(['plugin', 'run', 'openbot-local', 'status'])).paired, false); steps.push('public CLI reload');
  const auth = readFileSync(join(profile, 'auth.json'));
  await stop(); await start(); assert.deepEqual(readFileSync(join(profile, 'auth.json')), auth);
  assert.equal(JSON.parse(cli(['plugin', 'run', 'openbot-local', 'status'])).paired, false);
  steps.push('graceful stop/restart preserves host identity and installed plugin');
  cli(['plugin', 'remove', 'openbot-local', '--yes']); steps.push('public CLI remove');
  assert.deepEqual(readFileSync(join(profile, 'auth.json')), auth);
  await stop(); steps.push('shutdown joins launcher; host authentication retained');
  assert(outputs.every(part => !part.toString().includes('hostKey') && !part.toString().includes('joinCode')));
  writeFileSync(new URL('packaged-smoke-r2.json', records), JSON.stringify({ status: 'PASS', directory, plugin, profile, ports,
    artifact, sha256: integrity.sha256, platform: process.platform, node: process.version, steps,
    nativeInference: 'NOT EXERCISED by this lifecycle smoke', linux: 'NOT EXERCISED', output: Buffer.concat(outputs).toString() }, null, 2));
  console.log('PASS: ' + steps.join('; '));
} finally { await stop(); }
