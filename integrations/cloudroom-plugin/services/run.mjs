// Pinned source launcher: one unprivileged profile, loopback listeners, owned children.
import assert from 'node:assert/strict';
import { readFileSync, existsSync, realpathSync } from 'node:fs';
import { resolve, isAbsolute } from 'node:path';
import { parseArgs } from 'node:util';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { prepareSource } from '../compatibility/check-source.mjs';
import { startOwned, stopOwned, ready } from './processes.mjs';
import { acquireProfile } from './profile.mjs';

const { values } = parseArgs({ options: { source: { type: 'string' }, profile: { type: 'string' },
  'server-port': { type: 'string', default: '4310' }, 'daemon-port': { type: 'string', default: '4311' },
  'ui-port': { type: 'string', default: '4312' } }, allowPositionals: false });
const children = []; const stop = new AbortController(); let release;
let stage = 'configuration';
const shutdown = () => stop.abort();
process.on('SIGTERM', shutdown); process.on('SIGINT', shutdown);
const status = state => process.stdout.write(JSON.stringify({ component: 'cloudroom-local', stage, state }) + '\n');

async function bootstrap(cwd, env) {
  // Short-lived join material stays in bounded memory and the daemon environment.
  const child = spawn(process.execPath, ['apps/cli/dist/index.js', 'machine', 'join-code', '--json'],
    { cwd, env, detached: true, stdio: ['ignore', 'pipe', 'ignore'] });
  const owned = { child, exited: false, done: undefined };
  const chunks = []; let bytes = 0;
  const result = new Promise((resolveResult, reject) => {
    child.stdout.on('data', part => { bytes += part.length; if (bytes > 64 * 1024) { child.kill('SIGKILL'); reject(new Error('bootstrap response bound')); } else chunks.push(part); });
    child.once('error', reject);
    child.once('exit', code => { owned.exited = true; code === 0 ? resolveResult(Buffer.concat(chunks).toString('utf8')) : reject(new Error('host bootstrap failed')); });
  });
  owned.done = result.then(() => {}, () => {}); children.push(owned);
  const cancel = () => { try { process.kill(-child.pid, 'SIGKILL'); } catch {} };
  stop.signal.addEventListener('abort', cancel, { once: true });
  const timer = setTimeout(cancel, 10_000);
  try {
    const claim = JSON.parse(await result); stop.signal.throwIfAborted();
    assert(typeof claim.hostId === 'string' && claim.hostId.length <= 128);
    assert(typeof claim.joinCode === 'string' && claim.joinCode.length <= 4096);
    return { BB_HOST_ID: claim.hostId, BB_HOST_ENROLL_KEY: claim.joinCode };
  } finally { clearTimeout(timer); stop.signal.removeEventListener('abort', cancel); }
}

try {
  assert(process.getuid && process.getuid() !== 0, 'run as an unprivileged company account');
  assert(values.source && isAbsolute(values.source) && values.profile && isAbsolute(values.profile), 'explicit absolute source and profile paths required');
  const source = realpathSync(values.source); const profile = resolve(values.profile); const cwd = resolve(source, 'gui');
  const manifest = JSON.parse(readFileSync(new URL('../compatibility/manifest.json', import.meta.url)));
  assert.equal(process.version, `v${manifest.runtime.node}`);
  stage = 'source'; const identity = prepareSource(source);
  const prepared = JSON.parse(readFileSync(resolve(source, '.openbot-runtime.json')));
  assert.equal(prepared.artifactDigest, identity.artifactDigest); assert.equal(prepared.node, process.version);
  const ports = ['server-port', 'daemon-port', 'ui-port'].map(key => {
    assert(/^\d+$/.test(values[key])); const port = Number(values[key]); assert(port >= 1024 && port <= 65535); return port;
  });
  assert.equal(new Set(ports).size, 3, 'distinct unprivileged ports required');
  stage = 'profile'; process.umask(0o077); release = acquireProfile(profile);
  stage = 'ports';
  for (const port of ports) await new Promise((done, reject) => {
    const probe = createServer(); probe.once('error', reject);
    probe.listen(port, '127.0.0.1', () => probe.close(done));
  });
  const [serverPort, daemonPort, uiPort] = ports; const origin = `http://127.0.0.1:${serverPort}`;
  // Remove inherited Cloudroom overrides, including bootstrap/remote-account keys.
  const inherited = Object.fromEntries(Object.entries(process.env).filter(([key]) => !/^(BB_|ROOM_)/.test(key)));
  const env = { ...inherited, NODE_ENV: 'production', BB_DATA_DIR: profile, BB_SERVER_BIND_HOST: '127.0.0.1',
    BB_SERVER_URL: origin, ROOM_SERVER_URL: origin, BB_SERVER_PORT: String(serverPort),
    BB_HOST_DAEMON_PORT: String(daemonPort), ROOM_HOST_DAEMON_PORT: String(daemonPort),
    BB_HOST_DAEMON_AUTO_UPDATE: 'false', BB_APP_URL: `http://127.0.0.1:${uiPort}`, BB_DEV_APP_HOST: '127.0.0.1', BB_DEV_APP_PORT: String(uiPort) };
  const start = (entry, extra = {}, args = [], directory = cwd) => startOwned(children, process.execPath,
    ['--conditions=source', '--import', 'tsx', entry, ...args], { cwd: directory, env: { ...env, ...extra } }, stop);
  stage = 'server'; const server = start('apps/server/src/index.ts');
  await ready(`${origin}/api/v1/system/version`, server, stop.signal);
  stage = 'bootstrap'; const bootstrapEnv = existsSync(resolve(profile, 'auth.json')) ? {} : await bootstrap(cwd, env);
  stage = 'host';
  const host = start('apps/host-daemon/src/index.ts', bootstrapEnv);
  await ready(`http://127.0.0.1:${daemonPort}/status`, host, stop.signal);
  stage = 'ui'; const ui = start('node_modules/vite/bin/vite.js', {}, ['preview', '--strictPort', '--config', 'vite.dev.config.ts', '--configLoader', 'runner',
    '--port', String(uiPort), '--host', '127.0.0.1'], resolve(cwd, 'apps/app'));
  await ready(`http://127.0.0.1:${uiPort}`, ui, stop.signal);
  stage = 'services'; status('ready');
  if (!stop.signal.aborted) await new Promise(done => stop.signal.addEventListener('abort', done, { once: true }));
  if (stop.signal.reason instanceof Error && stop.signal.reason.name !== 'AbortError') throw stop.signal.reason;
} catch {
  process.exitCode = 1; status('failed');
} finally {
  stop.abort();
  try { await stopOwned(children); } catch { process.exitCode = 1; status('shutdown_failed'); }
  release?.();
  process.removeListener('SIGTERM', shutdown); process.removeListener('SIGINT', shutdown);
  status('stopped');
}
