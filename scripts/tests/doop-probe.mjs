// Verifier sensitivity tests: controlled responses, not live Doop acceptance.
import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { runOpenbotHelper } from '../doop-test/openbot-helper.mjs';
import { createBrowserOwner } from '../doop-test/browser-owner.mjs';
import { assertDocumentReadback } from '../doop-test/e2e-probe.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

function currentDocumentReadback() {
  return { item: { canvas_id: 'generated-a', frame_id: 'generated-frame-a',
    expected_old_text: 'Old content', expected_targeted_edit: 'Current replacement' },
    bot: { workspace: 'workspace-a' },
    canvas: { canvas: { id: 'generated-a' }, workspace_id: 'workspace-a', frames: [{ id: 'generated-frame-a' }] },
    frame: { id: 'generated-frame-a', html: '<h1>Current replacement</h1>' } };
}

test('document fresh readback accepts the actual expected replacement and parent identities', () => {
  assertDocumentReadback(currentDocumentReadback());
});
for (const [name, mutate] of [
  ['a stale saved pre-edit snapshot', (value) => { value.frame.html = '<h1>Old content</h1>'; }],
  ['a partial replacement retaining old text', (value) => { value.frame.html = 'Old content and Current replacement'; }],
  ['a foreign frame identity', (value) => { value.frame.id = 'foreign-frame'; }],
  ['a foreign canvas identity', (value) => { value.canvas.canvas.id = 'foreign-canvas'; }],
  ['a foreign workspace identity', (value) => { value.canvas.workspace_id = 'workspace-b'; }],
  ['a frame missing from its parent', (value) => { value.canvas.frames = []; }],
  ['a missing HTML observation', (value) => { delete value.frame.html; }],
]) test('document fresh readback rejects ' + name, () => {
  const value = currentDocumentReadback();
  mutate(value);
  assert.throws(() => assertDocumentReadback(value));
});

async function browserOwnerFixture(action) {
  const temporary = mkdtempSync(join(tmpdir(), 'openbot-browser-owner-'));
  const names = ['DOOP_TEST_PRIVATE_RUNTIME', 'DOOP_TEST_PRIVATE_OWNER', 'DOOP_TEST_UI_CHROME', 'DOOP_TEST_TLS_SPKI'];
  const previous = Object.fromEntries(names.map((name) => [name, process.env[name]]));
  Object.assign(process.env, { DOOP_TEST_PRIVATE_RUNTIME: temporary, DOOP_TEST_PRIVATE_OWNER: 'a'.repeat(32),
    DOOP_TEST_UI_CHROME: '/explicit/fixture/chrome', DOOP_TEST_TLS_SPKI: 'fixture-SPKI' });
  try { await action(temporary); }
  finally {
    for (const [name, value] of Object.entries(previous)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(temporary, { recursive: true, force: true });
  }
}

test('competing browser launches create one process with the existing profile and sandbox', async () => {
  await browserOwnerFixture(async (directory) => {
    let launches = 0;
    let closes = 0;
    const owner = createBrowserOwner({ async launch(options) {
      launches++;
      assert.equal(options.userDataDir, join(directory, 'ui-profile'));
      assert.deepEqual(options.args, ['--ignore-certificate-errors-spki-list=fixture-SPKI']);
      assert.equal(options.executablePath, '/explicit/fixture/chrome');
      const pending = JSON.parse(readFileSync(join(directory, 'browser-process.json')));
      assert.deepEqual(pending, { version: 1, phase: 'pending', pid: null, owner: 'a'.repeat(32) });
      return { process: () => ({ pid: 424242 }), async close() { closes++; } };
    } });
    const first = owner.launch();
    await assert.rejects(owner.launch(), /one browser launch/);
    await first;
    assert.equal(launches, 1);
    assert.equal(JSON.parse(readFileSync(join(directory, 'browser-process.json'))).phase, 'created');
    await Promise.all([owner.close(), owner.close()]);
    assert.equal(closes, 1);
    assert.equal(JSON.parse(readFileSync(join(directory, 'browser-process.json'))).phase, 'removed');
  });
});

test('failed browser shutdown retains its created receipt for authoritative recovery', async () => {
  await browserOwnerFixture(async (directory) => {
    const owner = createBrowserOwner({ async launch() {
      return { process: () => ({ pid: 424242 }), async close() { throw new Error('fixture close failure'); } };
    } });
    await owner.launch();
    await assert.rejects(owner.close(), /fixture close failure/);
    const receipt = JSON.parse(readFileSync(join(directory, 'browser-process.json')));
    assert.equal(receipt.phase, 'created');
    assert.equal(receipt.pid, 424242);
  });
});

test('browser with no positive process identity cannot claim confirmed removal', async () => {
  await browserOwnerFixture(async (directory) => {
    const owner = createBrowserOwner({ async launch() {
      return { process: () => undefined, async close() {} };
    } });
    await assert.rejects(owner.launch());
    await assert.rejects(owner.close(), /identity remains unconfirmed/);
    assert.equal(JSON.parse(readFileSync(join(directory, 'browser-process.json'))).phase, 'pending');
  });
});

test('lifecycle helper rejects an old executable that exits successfully without its exact marker', async () => {
  const temporary = mkdtempSync(join(tmpdir(), 'openbot-lifecycle-verifier-'));
  const previous = process.env.DOOP_TEST_OPENBOT_BINARY;
  try {
    const helper = join(temporary, 'old-helper.mjs');
    writeFileSync(helper, '#!/usr/bin/env node\nprocess.stdin.resume(); process.stdin.on("end",()=>console.log("running 0 tests; 0 passed"));\n');
    chmodSync(helper, 0o700);
    process.env.DOOP_TEST_OPENBOT_BINARY = helper;
    await assert.rejects(runOpenbotHelper({
      testName: 'adapters::persistence::doop_live_setup_tests::lifecycle::doop_lifecycle_live_company_keys',
      marker: 'DOOP_OPENBOT_LIFECYCLE_PASS companies=2 retired_refused=2 disconnected=2',
      recordName: 'lifecycle-record', input: { base: {}, retired_tokens: [] }, output: temporary, secrets: [],
    }), /must actually run/);
  } finally {
    if (previous === undefined) delete process.env.DOOP_TEST_OPENBOT_BINARY;
    else process.env.DOOP_TEST_OPENBOT_BINARY = previous;
    rmSync(temporary, { recursive: true, force: true });
  }
});

test('shared native launcher refuses oversized stdin before starting any executable', async () => {
  await assert.rejects(runOpenbotHelper({ input: { data: 'x'.repeat(65536) } }), /64 KiB/);
});

test('e2e helper rejects an old executable with zero selected document workflow tests', async () => {
  const temporary = mkdtempSync(join(tmpdir(), 'openbot-e2e-zero-match-'));
  const previous = process.env.DOOP_TEST_OPENBOT_BINARY;
  try {
    const helper = join(temporary, 'old-helper.mjs');
    writeFileSync(helper, '#!/usr/bin/env node\nprocess.stdin.resume(); process.stdin.on("end",()=>console.log("running 0 tests; 0 passed"));\n');
    chmodSync(helper, 0o700);
    process.env.DOOP_TEST_OPENBOT_BINARY = helper;
    await assert.rejects(runOpenbotHelper({
      testName: 'adapters::persistence::workflow::tests::mcp_tests::doop_setup_tests::document_tests::live_tests::doop_e2e_live_company_documents',
      marker: 'DOOP_OPENBOT_E2E_PASS companies=2 documents=2 creates=3 unknown=1',
      recordName: 'e2e-record', input: {}, output: temporary, secrets: [],
    }), /must actually run/);
  } finally {
    if (previous === undefined) delete process.env.DOOP_TEST_OPENBOT_BINARY;
    else process.env.DOOP_TEST_OPENBOT_BINARY = previous;
    rmSync(temporary, { recursive: true, force: true });
  }
});

test('native helper diagnostics redact retired and replacement keys', async () => {
  const temporary = mkdtempSync(join(tmpdir(), 'openbot-lifecycle-redaction-'));
  const previous = process.env.DOOP_TEST_OPENBOT_BINARY;
  try {
    const helper = join(temporary, 'failing-helper.mjs');
    writeFileSync(helper, '#!/usr/bin/env node\nprocess.stdin.resume(); process.stdin.on("end",()=>{console.log("retired-fixture-secret replacement-fixture-secret");process.exitCode=1;});\n');
    chmodSync(helper, 0o700);
    process.env.DOOP_TEST_OPENBOT_BINARY = helper;
    await assert.rejects(runOpenbotHelper({ testName: 'controlled', marker: 'never', recordName: 'lifecycle-record',
      input: {}, output: temporary, secrets: ['retired-fixture-secret', 'replacement-fixture-secret'] }));
    const diagnostic = readFileSync(join(temporary, 'lifecycle-record-helper-failure.txt'), 'utf8');
    assert.ok(!diagnostic.includes('fixture-secret'));
    assert.equal(diagnostic.trim(), '[redacted] [redacted]');
  } finally {
    if (previous === undefined) delete process.env.DOOP_TEST_OPENBOT_BINARY;
    else process.env.DOOP_TEST_OPENBOT_BINARY = previous;
    rmSync(temporary, { recursive: true, force: true });
  }
});
const preload = `
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
const root = process.env.PROBE_FIXTURE_ROOT;
const mutation = process.env.PROBE_MUTATION;
const html = '<!doctype html><html><body><h1>Synthetic contract</h1></body></html>';
const fixture = (name) => JSON.parse(readFileSync(join(root, name + '.json'), 'utf8'));
const text = (value) => ({ content: [{ type: 'text', text: JSON.stringify(value) }] });
globalThis.fetch = async (url, options) => {
  const path = new URL(url).pathname;
  const input = options.body ? JSON.parse(options.body) : {};
  const headers = { 'Content-Type': 'application/json' };
  let body;
  if (path === '/api/auth/sign-up/email') {
    const label = input.name.slice(5);
    body = { user: { id: 'user-' + label }, token: 'session-' + label };
    headers['Set-Cookie'] = 'session=' + label + '; HttpOnly';
  } else if (path === '/api/canvases') {
    body = options.method === 'POST' ? { id: 'verification-' + input.workspaceId } : [];
  } else if (path === '/api/workspaces') {
    body = { id: options.headers.Cookie.endsWith('a') ? 'workspace-a' : 'workspace-b' };
  } else if (path.endsWith('/members')) {
    body = { ok: true };
  } else if (path === '/api/agent-keys') {
    const label = options.headers.Cookie.split('=')[1];
    body = { id: 'agent-' + label, secret: 'token-' + label };
  } else if (path === '/mcp') {
    let result;
    if (input.method === 'initialize') result = fixture('initialize');
    else if (input.method === 'tools/list') result = fixture('tools');
    else {
      const name = input.params.name;
      if (name === 'get_canvas' && options.headers.Authorization.endsWith('bot-b')) {
        result = { isError: true, content: [{ type: 'text', text: 'foreign canvas denied' }] };
      } else if (mutation === name + '-error') {
        result = { isError: true, content: [{ type: 'text', text: 'controlled read failure' }] };
      } else if (name === 'get_guide') result = { content: [{ type: 'text', text: 'fixture guide' }] };
      else if (name === 'get_profile') result = fixture('profile');
      else if (name === 'list_canvases') result = text([{ id: 'canvas-a', workspace_id: mutation === 'wrong-workspace' ? 'workspace-b' : 'workspace-a' }]);
      else if (name === 'create_canvas') result = text({ id: 'canvas-a', name: 'Synthetic contract', url: '/c/canvas-a' });
      else if (name === 'create_frame') result = text({ ok: true, frame: { id: 'frame-a' } });
      else if (name === 'set_frame_html') result = text({ ok: true, frame: { id: 'frame-a' } });
      else if (name === 'get_frame') result = text({ id: mutation === 'wrong-frame' ? 'frame-b' : 'frame-a', html: mutation === 'unpersisted-edit' ? '' : html });
      else if (name === 'get_canvas') result = text({ id: mutation === 'wrong-canvas' ? 'canvas-b' : 'canvas-a', frames: [{ id: 'frame-a' }] });
      else if (name === 'get_frame_screenshot') result = fixture('screenshot');
      else if (name === 'export_frame') result = fixture('pdf');
      else throw new Error('unexpected controlled operation');
    }
    body = { jsonrpc: '2.0', id: input.id, result };
  } else throw new Error('unexpected controlled request path');
  return new Response(JSON.stringify(body), { status: 200, headers });
};
`;

for (const mutation of [
  'none', 'list_canvases-error', 'get_frame-error', 'get_canvas-error',
  'wrong-workspace', 'unpersisted-edit', 'wrong-frame', 'wrong-canvas',
  'helper-zero-matches', 'helper-nonzero', 'helper-unavailable',
]) {
  test(`compatibility report ${mutation === 'none' ? 'accepts the controlled positive' : 'rejects ' + mutation}`, () => {
    const temporary = mkdtempSync(join(tmpdir(), 'openbot-probe-verifier-'));
    try {
      const module = join(temporary, 'controlled-fetch.mjs');
      const output = join(temporary, 'results');
      mkdirSync(output);
      writeFileSync(module, preload);
      const helper = join(temporary, 'helper.mjs');
      if (mutation.startsWith('helper-') && mutation !== 'helper-unavailable') {
        writeFileSync(helper, '#!/usr/bin/env node\nprocess.stdin.resume(); process.stdin.on("end",()=>{console.log("running 0 tests; 0 passed");process.exitCode=' + (mutation === 'helper-nonzero' ? '1' : '0') + ';});\n');
        chmodSync(helper, 0o700);
      }
      const result = spawnSync(process.execPath, [
        '--import', module, join(root, 'scripts/doop-test/probe.mjs'),
        'http://127.0.0.1:4400', output, 'contract',
      ], {
        encoding: 'utf8', timeout: 10000, input: 'ready\n',
        env: { ...process.env, PROBE_MUTATION: mutation, PROBE_FIXTURE_ROOT: join(root, 'src/adapters/mcp/fixtures/doop'),
          DOOP_TEST_OPENBOT_BINARY: mutation.startsWith('helper-') ? helper : undefined,
          DOOP_TEST_OPENBOT_DATABASE: 'synthetic-fixture', DOOP_TEST_SOURCE_FINGERPRINT: 'a'.repeat(64) },
      });
      assert.equal(result.signal, null, 'the probe must finish by its assertions, not a timeout/signal');
      assert.equal(result.status, mutation === 'none' ? 0 : 1, result.stdout + result.stderr);
      const report = JSON.parse(readFileSync(join(output, 'report.json'), 'utf8'));
      assert.equal(report.checks.some((entry) => entry.status === 'FAIL'), mutation !== 'none');
      if (mutation.startsWith('helper-')) {
        assert.ok(report.checks.some((entry) => entry.name === 'live Openbot company connections and encrypted custody' && entry.status === 'FAIL'),
          'the helper failure path must actually be reached');
      }
    } finally {
      rmSync(temporary, { recursive: true, force: true });
    }
  });
}
