import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { once } from 'node:events';
import { createServer } from 'node:net';
import { createHash } from 'node:crypto';
const records = new URL('.', import.meta.url);
const artifact = '/tmp/openbot-cloudroom-p06-artifacts-final-r2/bb-plugin-openbot-local-0.2.0.tgz';
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
  cwd: resolve(source, 'gui'), env: { ...process.env, NODE_ENV: 'production', BB_DATA_DIR: profile, ROOM_SERVER_URL: origin, ROOM_HOST_DAEMON_PORT: String(ports[1]) },
  encoding: 'utf8', timeout: 30_000, maxBuffer: 256 * 1024 });
let thread;
const jsonCli = args => JSON.parse(cli(args));
const events = () => jsonCli(['thread', 'log', thread, '--json', '--limit', '100']);
const until = async predicate => {
  const deadline = performance.now() + 60_000;
  while (performance.now() < deadline) {
    const value = events();
    assert(Array.isArray(value), 'expected bounded native event array');
    if (predicate(value)) return value;
    await new Promise(done => setTimeout(done, 250));
  }
  throw new Error('native lifecycle smoke condition timed out');
};
writeFileSync(new URL('packaged-native-lifecycle-r3-resource.json', records), JSON.stringify({directory,profile,plugin,ports,artifact}));
try {
  await start(); steps.push('packaged server/host/UI boot');
  cli(['plugin', 'install', `path:${plugin}`, '--yes']);
  const workspace = join(directory, 'workspace'); mkdirSync(workspace);
  const marker = 'OPENBOT_PACKAGED_NATIVE_LIFECYCLE_20261108';
  writeFileSync(join(workspace, 'smoke-marker.txt'), marker + '\n');
  execFileSync('git', ['init', '--quiet', workspace]);
  const host = JSON.parse(readFileSync(join(profile, 'auth.json'))).hostId;
  const project = jsonCli(['project', 'create', '--name', 'Disposable packaged lifecycle smoke', '--root', workspace, '--machine', host, '--json']);
  assert(project.id);
  const created = jsonCli(['thread', 'spawn', '--project', project.id, '--machine', 'local', '--provider', 'codex', '--model', 'gpt-6-luna', '--reasoning-level', 'low',
    '--title', 'Disposable packaged lifecycle smoke', '--prompt', `Disposable local file smoke: use your shell tool once to run exactly cat ${workspace}/smoke-marker.txt and reply with the file's exact contents. This is a new test-owned project file. Do not edit any file, use other commands, access network or start subagents.`, '--json']);
  thread = created.id; assert(/^thr_[a-z0-9]+$/.test(thread));
  const first = await until(list => list.some(event => event.type === 'turn/completed') && JSON.stringify(list).includes(marker));
  steps.push('native Codex task reads only disposable project file and matches its accepted turn');
  const turnIds = [...new Set(first.filter(event => event.scope?.kind === 'turn').map(event => event.scope.turnId))];
  assert.equal(turnIds.length, 1);
  cli(['thread', 'tell', thread, 'Disposable service shutdown smoke: use your shell tool once to run exactly sleep 20, then reply DONE. Do not read or write files, access network, or start subagents.', '--model', 'gpt-6-luna', '--reasoning-level', 'low', '--mode', 'auto', '--json']);
  const active = await until(list => list.some(event => event.type === 'item/started' && event.data?.item?.type === 'commandExecution' && !turnIds.includes(event.scope?.turnId) && !list.some(done => done.type === 'turn/completed' && done.scope?.turnId === event.scope?.turnId)));
  const beforeRequests = active.filter(event => event.type === 'client/turn/requested').length;
  assert.equal(beforeRequests, 2);
  await stop(); steps.push('bounded packaged wrapper shutdown while native shell work is active');
  await start(); const after = events();
  assert.equal(after.filter(event => event.type === 'client/turn/requested').length, beforeRequests);
  assert.deepEqual([...new Set(after.filter(event => event.scope?.kind === 'turn').map(event => event.scope.turnId))].slice(0, 1), turnIds);
  steps.push('restart preserves thread/turn history with zero new native submissions');
  cli(['thread', 'stop', thread]); cli(['thread', 'archive', thread]);
  cli(['plugin', 'remove', 'openbot-local']);
  await stop(); assert.equal(readFileSync(join(workspace, 'smoke-marker.txt'), 'utf8'), marker + '\n');
  steps.push('explicit broad stop/archive and plugin removal preserve workspace');
  const output = Buffer.concat(outputs).toString(); assert(!output.includes(marker)); assert(!output.includes('joinCode')); assert(!output.includes('hostKey'));
  writeFileSync(new URL('packaged-native-lifecycle-r3.json', records), JSON.stringify({ status: 'PASS', directory, profile, plugin, workspace, ports, artifact,
    sha256: integrity.sha256, platform: process.platform, node: process.version, projectId: project.id, threadId: thread, initialTurnId: turnIds[0],
    submissionCountBeforeRestart: beforeRequests, submissionCountAfterRestart: after.filter(event => event.type === 'client/turn/requested').length,
    stoppedByServiceShutdown: 'NOT INFERRED; explicit thread-wide stop requested after restart',
    productionPluginNativeInference: 'NOT EXERCISED; this is a public CLI provider task for launcher lifecycle, not paired connector acceptance',
    steps, output, linux: 'NOT EXERCISED' }, null, 2));
  console.log('PASS: ' + steps.join('; '));
} finally {
  if (thread && service?.exitCode === null) { try { cli(['thread', 'stop', thread]); } catch {} }
  await stop();
}
