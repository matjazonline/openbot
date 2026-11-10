// Explicitly opted-in disposable acceptance of the packaged production plugin.
// Pairing/auth material remains in a private config file and request bodies.
import assert from 'node:assert/strict';
import { spawn, execFileSync, type ChildProcess } from 'node:child_process';
import { readFileSync, statSync, mkdirSync, writeFileSync, mkdtempSync, existsSync, rmSync } from 'node:fs';
import { homedir } from 'node:os';
import Database from 'better-sqlite3';
import { resolve, join } from 'node:path';
import { createServer } from 'node:net';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import { createInterface } from 'node:readline';
import { readBoundedJson } from '../src/enrollment.js';
import { loggedOutProviderEnvironment } from './native-provider-environment.js';

assert.equal(process.env.OPENBOT_CLOUDROOM_NATIVE_SMOKE, '1', 'native acceptance requires explicit opt-in');
const inputFile = process.argv[2]!; assert(statSync(inputFile).size < 16 * 1024);
const cfg = JSON.parse(readFileSync(inputFile, 'utf8')) as {
  directory: string; source: string; artifact: string; workspace: string; origin: string;
  pairingCode: string; provider: string; opencodeBin?: string; evidence: string;
};
assert(['codex', 'claude-code', 'acp-opencode'].includes(cfg.provider));
const integrity = JSON.parse(readFileSync(cfg.artifact + '.integrity.json', 'utf8'));
assert.equal(createHash('sha256').update(readFileSync(cfg.artifact)).digest('hex'), integrity.sha256);
process.umask(0o077);
const plugin = join(cfg.directory, 'plugin'); const profile = join(cfg.directory, 'profile');
mkdirSync(plugin); execFileSync('tar', ['-xzf', cfg.artifact, '-C', plugin, '--strip-components=1']);
execFileSync('npm', ['ci', '--omit=dev'], { cwd: plugin, stdio: 'pipe', timeout: 180_000, maxBuffer: 1024 * 1024 });
const port = async () => { const listener = createServer(); listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
  const value = (listener.address() as { port: number }).port; await new Promise<void>(done => listener.close(() => done())); return value; };
const ports = [await port(), await port(), await port()]; const origin = `http://127.0.0.1:${ports[0]}`;
const uiOrigin = `http://127.0.0.1:${ports[2]}`;
let env: NodeJS.ProcessEnv = { ...process.env, NODE_ENV: 'production', BB_DATA_DIR: profile, ROOM_SERVER_URL: origin,
  ROOM_HOST_DAEMON_PORT: String(ports[1]), PATH: cfg.opencodeBin ? `${cfg.opencodeBin}:${process.env.PATH}` : process.env.PATH };
if (cfg.provider === 'acp-opencode') {
  assert.match(execFileSync('opencode', ['--version'], { env, encoding: 'utf8', timeout: 5000 }), /1\.19\.5/);
  assert(!env.OPENCODE_CONFIG_CONTENT, 'native acceptance must not replace an existing inline provider configuration');
  env.OPENCODE_CONFIG_CONTENT = JSON.stringify({ permission: { edit: 'ask' } });
}
const cli = (args: string[]) => execFileSync(process.execPath, [resolve(cfg.source, 'gui/apps/cli/dist/index.js'), ...args], {
  cwd: resolve(cfg.source, 'gui'), env, encoding: 'utf8', timeout: 60_000, maxBuffer: 2 * 1024 * 1024,
});
const jsonCli = (args: string[]) => JSON.parse(cli(args));
const emit = (value: unknown) => process.stdout.write(JSON.stringify(value) + '\n');
let service: ChildProcess | undefined; let output = ''; let stage = 'boot';
async function boot() {
  output = '';
  service = spawn(process.execPath, [join(plugin, 'services/run.mjs'), '--source', cfg.source, '--profile', profile,
    '--server-port', String(ports[0]), '--daemon-port', String(ports[1]), '--ui-port', String(ports[2])], { env, stdio: ['ignore', 'pipe', 'ignore'] });
  service.stdout!.on('data', part => { output += part; if (Buffer.byteLength(output) > 64 * 1024) service!.kill('SIGTERM'); });
  const deadline = performance.now() + 45_000;
  while (!output.includes('"state":"ready"')) {
    assert(service.exitCode === null && performance.now() < deadline, 'packaged native service boot failed');
    await new Promise(done => setTimeout(done, 100));
  }
}
async function stop() {
  if (!service || service.exitCode !== null) return;
  const exited = once(service, 'exit'); service.kill('SIGTERM');
  const timer = setTimeout(() => service?.kill('SIGKILL'), 12_000);
  try { const [code] = await exited; assert.equal(code, 0); } finally { clearTimeout(timer); }
}
const compatibility = new URL('../compatibility/', import.meta.url);
const { localCallerHeaders } = await import(new URL('local-caller.mjs', compatibility).href);
const { proveUi } = await import(new URL('prove-ui.mjs', compatibility).href);
let headers: Record<string, string>; let thread: string | undefined; let ownedProject: string | undefined;
let permissionDirectory: string | undefined; let permissionTarget: string | undefined;
async function settings(values: Record<string, unknown>) {
  const response = await fetch(`${origin}/api/v1/plugins/openbot-local/settings`, { method: 'PUT', redirect: 'error',
    headers: { ...headers, 'content-type': 'application/json' }, body: JSON.stringify({ values }), signal: AbortSignal.timeout(10_000) });
  assert(response.ok, 'public plugin settings refused'); await readBoundedJson(response, 256 * 1024);
}
async function waitStatus(ready: (status: any) => boolean) {
  const deadline = performance.now() + 60_000;
  while (performance.now() < deadline) {
    const status = jsonCli(['plugin', 'run', 'openbot-local', 'status']);
    if (status.paired && status.connection === 'connected' && ready(status)) return status;
    await new Promise(done => setTimeout(done, 250));
  }
  throw new Error('paired production connector did not become ready');
}
const connected = () => waitStatus(status => status.providers.some((item: {provider:string;status:string}) => item.provider === cfg.provider && item.status === 'available'));
const lines = createInterface({ input: process.stdin });
const deadline = setTimeout(() => lines.close(), 5 * 60_000);
function readEvents(id: string): any[] {
  const events: any[] = []; let after = 0;
  for (let page = 0; page < 4; page++) {
    const batch = jsonCli(['thread', 'log', id, '--json', '--limit', '100', '--after-seq', String(after)]);
    assert(Array.isArray(batch) && batch.length <= 100);
    for (const event of batch) { assert(Number.isSafeInteger(event.seq) && event.seq > after); after = event.seq; events.push(event); }
    if (batch.length < 100) return events;
  }
  throw new Error('native acceptance history exceeded four bounded pages');
}
async function humanTerminal(marker: string) {
  assert(thread); const deadline = performance.now() + 120_000;
  // A native permission denial may interrupt the turn and pause its queue.
  // The human explicitly sends the existing queued message; never resubmit it.
  const queued = jsonCli(['thread', 'queue', 'list', thread, '--json']);
  assert(Array.isArray(queued) && queued.length <= 32);
  const own = queued.filter(item => JSON.stringify(item.content).includes(marker));
  assert(own.length <= 1);
  if (own.length === 1) {
    assert(typeof own[0].id === 'string');
    const lastTurn = readEvents(thread).filter(event => event.type === 'turn/completed').at(-1);
    // ACP can retain an active thread after the denied turn is interrupted.
    // The human explicitly stops that thread before resuming its existing queue.
    if (lastTurn?.data.status === 'interrupted') cli(['thread', 'stop', thread]);
    jsonCli(['thread', 'queue', 'send', thread, own[0].id, '--json']);
  }
  while (performance.now() < deadline) {
    const events = readEvents(thread);
    const requests = events.filter(event => event.type === 'client/turn/requested' && JSON.stringify(event.data.input).includes(marker));
    assert(requests.length <= 1, 'independent human input must not be resubmitted');
    const request = requests[0];
    const accepted = request && events.find(event => event.type === 'turn/input/accepted' && event.data.clientRequestId === request.data.requestId);
    if (accepted && events.some(event => event.type === 'turn/completed' && event.scope.turnId === accepted.scope.turnId)) {
      assert(events.some(event => event.type === 'item/completed' && event.scope.turnId === accepted.scope.turnId
        && event.data.item.type === 'agentMessage' && event.data.item.text.includes(marker)));
      return accepted.scope.turnId;
    }
    await new Promise(done => setTimeout(done, 100));
  }
  throw new Error('independent human turn did not finish');
}
function transcript(operation: string) {
  assert(thread && /^[0-9a-f-]{36}$/.test(operation));
  const events = readEvents(thread);
  assert(Array.isArray(events) && events.length <= 400);
  const request = events.find((event: any) => event.type === 'client/turn/requested'
    && JSON.stringify(event.data.input).includes(`Openbot operation ${operation}`));
  const accepted = request && events.find((event: any) => event.type === 'turn/input/accepted' && event.data.clientRequestId === request.data.requestId);
  return { events, turn: accepted?.scope.turnId };
}
async function terminal(operation: string, marker: string) {
  const deadline = performance.now() + 120_000;
  while (performance.now() < deadline) {
    const { events, turn } = transcript(operation);
    const complete = turn && events.find((event: any) => event.type === 'turn/completed' && event.scope.turnId === turn);
    if (complete) {
      assert.equal(complete.data.status, 'completed');
      assert(events.some((event: any) => event.type === 'item/completed' && event.scope.turnId === turn
        && event.data.item.type === 'agentMessage' && event.data.item.text.includes(marker)));
      return turn;
    }
    await new Promise(done => setTimeout(done, 100));
  }
  throw new Error('native output did not become durable');
}
async function executing(operation: string) {
  assert(thread && /^[0-9a-f-]{36}$/.test(operation));
  const deadline = performance.now() + 30_000;
  while (performance.now() < deadline) {
    const { events, turn } = transcript(operation);
    if (turn && events.some((event: any) => event.type === 'item/started' && event.data.item.type === 'commandExecution'
        && event.scope.kind === 'turn' && event.scope.turnId === turn)) return turn;
    await new Promise(done => setTimeout(done, 100));
  }
  throw new Error('native stop task did not execute its bounded shell command');
}
function diagnostics() {
  // Read only the plugin-owned database, never Cloudroom's internal tables.
  const db = new Database(join(profile, 'plugins/openbot-local/data.db'), { readonly: true, fileMustExist: true });
  try {
    const commands = db.prepare(`SELECT id,json_extract(state,'$.operation.command.kind') AS kind,
      json_extract(state,'$.stage') AS stage,json_extract(state,'$.sdkCalls') AS sdkCalls,
      json_extract(state,'$.threadId') AS thread,json_extract(state,'$.requestId') AS request,
      json_extract(state,'$.turnId') AS turn,json_extract(state,'$.queueId') AS queue,
      json_extract(state,'$.cursor') AS cursor,json_extract(state,'$.correlationConflict') AS correlationConflict,
      json_extract(state,'$.nativeState') AS nativeState,
      COALESCE(json_extract(state,'$.lastObservation.observation.sequence'),json_extract(state,'$.lastObservation.sequence')) AS sequence,
      json_extract(state,'$.lastObservation.observation.evidence.kind') AS evidence
      FROM commands ORDER BY rowid LIMIT 16`).all();
    const events = thread ? readEvents(thread) : [];
    const threads = jsonCli(['thread', 'list', '--json']);
    assert(Array.isArray(threads) && threads.length <= 8);
    const nativeThreads = threads.map(item => ({ id: item.id, status: item.status,
      events: readEvents(item.id).map(event => ({ type: event.type, status: event.data.status })) }));
    writeFileSync(join(cfg.evidence, 'diagnostics.json'), JSON.stringify({ stage, commands, nativeThreads, permissionTargetCreated: permissionTarget ? existsSync(permissionTarget) : null, events: events.map((event: any) => ({
      seq: event.seq, type: event.type, scope: event.scope, request: event.data.requestId, clientRequest: event.data.clientRequestId,
      source: event.data.source, status: event.data.status, itemType: event.data.item?.type,
      ...(cfg.provider === 'acp-opencode' && stage === 'permission_prompt' && event.data.item?.type === 'agentMessage'
        ? { fixtureAgentText: String(event.data.item.text).slice(0, 1000) } : {}),
      operation: JSON.stringify(event.data.input ?? '').match(/Openbot operation ([0-9a-f-]{36})/)?.[1],
    })) }, null, 2));
  } finally { db.close(); }
}
try {
  await boot(); stage = 'install'; cli(['plugin', 'install', `path:${plugin}`, '--yes']);
  headers = await localCallerHeaders(origin, profile);
  const host = JSON.parse(readFileSync(join(profile, 'auth.json'), 'utf8')).hostId;
  const project = jsonCli(['project', 'create', '--name', `Disposable Openbot ${cfg.provider} acceptance`, '--root', cfg.workspace, '--machine', host, '--json']);
  assert(/^proj_[a-z0-9]+$/.test(project.id));
  ownedProject = project.id;
  stage = 'catalog';
  // Warm provider discovery before the connector's bounded read. This is separate
  // from successful authentication/inference, which only the later task proves.
  for (const provider of ['codex', 'claude-code', 'acp-opencode']) {
    try { jsonCli(['provider', 'models', provider, '--machine', host, '--json']); } catch { /* actual selected-provider proof remains mandatory */ }
  }
  stage = 'pair_settings';
  await settings({ openbotOrigin: cfg.origin, machineName: `Disposable ${cfg.provider} acceptance`, selectedProjects: project.id, pairingCode: cfg.pairingCode });
  stage = 'pair_reload'; cli(['plugin', 'reload', 'openbot-local']); stage = 'pair_connected'; await connected();
  emit({ event: 'ready', project: project.id, profile, ports });
  for await (const line of lines) {
    assert(Buffer.byteLength(line) <= 4096); const input = JSON.parse(line) as { command: string; thread?: string; text?: string; operation?: string; permissionMode?: string };
    if (input.thread !== undefined) { assert(/^thr_[a-z0-9]+$/.test(input.thread)); thread = input.thread; }
    if (input.command === 'restart') {
      stage = 'restart'; await stop(); await boot(); headers = await localCallerHeaders(origin, profile); await connected(); emit({ event: 'restarted' });
    } else if (input.command === 'shutdown') {
      stage = 'shutdown'; await stop(); emit({ event: 'shutdown' });
    } else if (input.command === 'boot') {
      stage = 'boot'; await boot(); headers = await localCallerHeaders(origin, profile); await connected(); emit({ event: 'booted' });
    } else if (input.command === 'terminal') {
      stage = 'terminal'; assert(input.operation && input.text); emit({ event: 'terminal', turn: await terminal(input.operation, input.text) });
    } else if (input.command === 'ui') {
      stage = 'ui'; assert(thread && input.text && input.text.length <= 256);
      const evidence = join(cfg.evidence, `ui-${Date.now()}`); mkdirSync(evidence, { recursive: true });
      await proveUi({ origin: uiOrigin, backendOrigin: origin, profile, projectId: project.id, threadId: thread, expectedText: input.text, evidence });
      emit({ event: 'ui', thread, project: project.id, evidence });
    } else if (input.command === 'executing') {
      stage = 'executing'; assert(input.operation); emit({ event: 'executing', turn: await executing(input.operation) });
    } else if (input.command === 'human') {
      stage = 'human'; assert(thread && input.text && input.text.length <= 256);
      assert(input.permissionMode === undefined || input.permissionMode === 'accept-edits');
      const receipt = jsonCli(['thread', 'tell', thread, `Reply exactly ${input.text}. Do not use tools, change files, access network, or start subagents.`, '--mode', 'queue', '--json',
        ...(input.permissionMode ? ['--permission-mode', input.permissionMode] : [])]);
      emit({ event: 'human', independentlyOriginated: true, queued: receipt.delivery === 'queued' });
    } else if (input.command === 'human_terminal') {
      stage = 'human_terminal'; assert(input.text); emit({ event: 'human_terminal', turn: await humanTerminal(input.text) });
    } else if (input.command === 'permission_prompt') {
      stage = 'permission_prompt'; assert(input.text && input.text.length <= 256);
      if (cfg.provider === 'codex') {
        emit({ event: 'permission_prompt', prompt: `Disposable permission test: call exec_command exactly once with sandbox_permissions set to require_escalated and a justification asking the human to allow this disposable local request. Do not attempt the command without requesting elevated permission first. The command is exactly curl --max-time 5 --silent ${origin}/api/v1/system/version. If denied, do not retry or work around it. Finally reply exactly ${input.text}. Do not read/change files, use other URLs, or start subagents.` });
      } else {
        assert(!permissionTarget);
        if (cfg.provider === 'claude-code') {
          permissionDirectory = mkdtempSync(join(homedir(), '.openbot-p07-permission-'));
          permissionTarget = join(permissionDirectory, 'smoke.txt');
        } else permissionTarget = join(cfg.workspace, 'smoke-permission.txt');
        assert(!existsSync(permissionTarget));
        const tool = cfg.provider === 'claude-code' ? 'Write' : 'available file-creation (write or apply_patch)';
        const policy = cfg.provider === 'claude-code' ? 'The fresh test-owned target is outside the project.' : 'The fresh test-owned target is in the project, whose native edit policy explicitly requires human approval.';
        emit({ event: 'permission_prompt', prompt: `Authorized disposable permission test: invoke your ${tool} tool exactly once to write the single word smoke to exactly ${permissionTarget}. ${policy} Invoke the tool directly; let its native permission callback ask the human. Do not ask in chat instead. If denied, do not retry or work around the boundary. Finally reply exactly ${input.text}. Do not access any other paths, use network, or start subagents.` });
      }
    } else if (input.command === 'deny') {
      stage = 'deny'; assert(thread);
      try {
        execFileSync(process.execPath, [resolve(new URL('prove-native-ui.mjs', compatibility).pathname), uiOrigin, origin, profile, project.id, thread, 'deny', cfg.evidence],
          { env, stdio: 'pipe', timeout: 75_000, maxBuffer: 1024 * 1024 });
      } catch (error) {
        // This controlled UI probe reports locator/assertion errors, never request headers.
        const detail = (error as { stderr?: Buffer }).stderr?.toString('utf8') ?? 'UI probe timeout';
        writeFileSync(join(cfg.evidence, 'deny-probe-failure.txt'), detail.slice(0, 4096), { mode: 0o600 });
        throw error;
      }
      assert(!permissionTarget || !existsSync(permissionTarget), 'denied permission must not create the test-owned file');
      emit({ event: 'deny', actualUi: true, deniedFileAbsent: true });
    } else if (input.command === 'logged_out') {
      stage = 'logged_out'; await stop(); const isolated = loggedOutProviderEnvironment(cfg.provider, env, cfg.directory); env = isolated.env;
      await boot(); headers = await localCallerHeaders(origin, profile); const status = await waitStatus(() => true);
      emit({ event: 'logged_out', ...isolated.proof, health: status.providers.find((item: any) => item.provider === cfg.provider)?.status });
    } else if (input.command === 'journal') {
      stage = 'journal'; assert(input.operation && /^[0-9a-f-]{36}$/.test(input.operation));
      const db = new Database(join(profile, 'plugins/openbot-local/data.db'), { readonly: true, fileMustExist: true });
      try {
        const row = db.prepare('SELECT state FROM commands WHERE id=?').get(input.operation) as {state:string} | undefined;
        assert(!row || Buffer.byteLength(row.state) <= 2 * 1024 * 1024); const record = row && JSON.parse(row.state);
        emit({ event: 'journal', present: Boolean(record), sdkCalls: record?.sdkCalls ?? 0, stage: record?.stage, thread: record?.threadId, turn: record?.turnId });
      } finally { db.close(); }
    } else if (input.command === 'snapshot') {
      stage = 'snapshot'; assert(thread);
      const events = readEvents(thread); assert(Array.isArray(events) && events.length <= 400);
      const native = jsonCli(['thread', 'show', thread, '--json']).thread;
      assert(native.id === thread && Object.hasOwn(native, 'archivedAt'));
      emit({ event: 'snapshot', thread, archived: native.archivedAt !== null, requests: events.filter(event => event.type === 'client/turn/requested').length,
        turns: [...new Set(events.filter(event => event.scope?.kind === 'turn').map(event => event.scope.turnId))],
        terminals: events.filter(event => event.type === 'turn/completed').map(event => ({ turn: event.scope.turnId, status: event.data.status })) });
    } else if (input.command === 'close') break;
    else throw new Error('unknown native acceptance command');
  }
} catch (error) {
  const failure = error as { name?: string; code?: string; status?: number; signal?: string };
  writeFileSync(join(cfg.evidence, 'failure-stage.json'), JSON.stringify({ stage, name: failure.name,
    code: failure.code, exitStatus: failure.status, signal: failure.signal }), { mode: 0o600 });
  emit({ event: 'failure', stage }); process.exitCode = 1;
} finally {
  clearTimeout(deadline); lines.close();
  if (service?.exitCode === null) {
    try { diagnostics(); } catch { /* no raw SDK objects or credentials in diagnostics */ }
    if (ownedProject) {
      try {
        const own = jsonCli(['thread', 'list', '--json']).filter((item: {projectId:string}) => item.projectId === ownedProject);
        assert(own.length <= 8);
        for (const item of own) cli(['thread', 'stop', item.id]);
      } catch {}
    }
    try { cli(['plugin', 'remove', 'openbot-local']); } catch {}
  }
  await stop();
  if (permissionDirectory) rmSync(permissionDirectory, { recursive: true, force: true });
}
