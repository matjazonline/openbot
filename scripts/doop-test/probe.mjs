// Real local Doop HTTP/MCP probes. Persistent outputs never contain credentials.
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createInterface } from 'node:readline';
import { companyInput, runOpenbotHelper } from './openbot-helper.mjs';

const [origin, output, suite] = process.argv.slice(2);
const target = new URL(origin);
const privateDeployment = process.env.DOOP_TEST_PRIVATE_DEPLOYMENT;
function validateOrigin() {
  const privateMode = ['disk', 'bucket'].includes(privateDeployment);
  assert.ok(['contract', 'all', 'e2e'].includes(suite), 'unknown probe family');
  if (suite === 'e2e') assert.ok(privateMode && process.env.DOOP_TEST_OPENBOT_BINARY,
    'e2e requires the explicit private deployment and native Openbot helper');
  assert.ok((privateDeployment === undefined || privateMode) && target.hostname === '127.0.0.1' &&
    target.protocol === (privateMode ? 'https:' : 'http:') && target.origin === origin &&
    !target.username && !target.password && !target.search && !target.hash &&
    (!privateMode || (process.env.NODE_EXTRA_CA_CERTS && process.env.DOOP_TEST_TLS_SPKI)),
    'The local probe origin or explicit TLS fixture is invalid');
}
const checks = [];
const secrets = [];
const replacements = [];
let requests = 0;
let nextId = 0;
const controls = createInterface({ input: process.stdin, crlfDelay: Infinity });
const acknowledgements = controls[Symbol.asyncIterator]();

async function control(message) {
  console.log('DOOP_TEST_CONTROL ' + JSON.stringify(message));
  let timeout;
  try {
    const reply = await Promise.race([
      acknowledgements.next(),
      new Promise((_, reject) => { timeout = setTimeout(() => reject(new Error('local restart acknowledgement timeout')), 95000); }),
    ]);
    assert.equal(reply.value, 'ready');
    assert.equal(reply.done, false);
  } finally { clearTimeout(timeout); }
}

async function openbotSetup(actors) {
  if (!process.env.DOOP_TEST_OPENBOT_BINARY) {
    checks.push({ name: 'live Openbot company connections and encrypted custody', status: 'NOT EXERCISED', reason: 'explicit compiled helper and owned PostgreSQL not selected' });
    return;
  }
  await check('live Openbot company connections and encrypted custody', async () => {
    const record = await runOpenbotHelper({
      testName: 'adapters::persistence::doop_live_setup_tests::doop_live_company_setup',
      marker: 'DOOP_OPENBOT_SETUP_PASS companies=2 tool_calls=4', recordName: 'setup-record',
      input: companyInput(origin, actors), output, secrets,
    });
      assert.equal(record.mappings.length, 2);
      assert.equal(record.mcp_tool_calls, 4);
      assert.equal(record.catalog_discoveries, 6);
      for (let index = 0; index < 2; index += 1) {
        const bot = actors[`bot-${['a', 'b'][index]}`];
        assert.deepEqual(record.mappings[index].verification.result, {
          verified: true, bot_id: bot.id, workspace_id: bot.workspace, canvas_id: bot.verificationCanvas,
        });
      }
      assert.equal(record.source_identity.fingerprint, process.env.DOOP_TEST_SOURCE_FINGERPRINT);
      sanitized(record);

  });
}

function failure(error) {
  return {
    reason: error.name,
    location: error.stack?.split('\n').find((line) => /(?:probe|private-browser)\.mjs:\d+:\d+/.test(line))?.trim(),
    locations: error.stack?.split('\n').filter((line) => /(?:probe|private-browser)\.mjs:\d+:\d+/.test(line)).slice(0, 3).map((line) => line.trim()),
    ...(typeof error.actual === 'number' ? { actualStatus: error.actual } : {}),
    ...(typeof error.expected === 'number' ? { expectedStatus: error.expected } : {}),
    ...(typeof error.actual === 'boolean' ? { actual: error.actual } : {}),
  };
}

function assertDenied(response, allowed = [403, 404]) {
  if (!allowed.includes(response.status)) {
    const error = new Error('authorization denial required');
    error.actual = response.status;
    error.expected = 403;
    throw error;
  }
}

async function request(path, { method = 'GET', body, bytes: requestBytes, headers = {}, raw = false, cookie, token } = {}) {
  assert.ok(path.startsWith('/') && !path.startsWith('//'));
  assert.ok(++requests <= 160, 'request budget exceeded');
  const response = await fetch(origin + path, {
    method, redirect: 'error', signal: AbortSignal.timeout(10000),
    headers: {
      Accept: 'application/json, text/event-stream', Origin: origin, ...headers,
      ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...(cookie ? { Cookie: cookie } : {}),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      'MCP-Protocol-Version': '2025-03-26',
    },
    body: body === undefined ? requestBytes : JSON.stringify(body),
  });
  const chunks = [];
  let bytes = 0;
  const reader = response.body.getReader();
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      if (bytes > 1024 * 1024) throw new Error('local response exceeds Openbot discovery bound');
      chunks.push(value);
    }
  } finally {
    await reader.cancel();
  }
  const responseBytes = Buffer.concat(chunks);
  const text = raw ? '' : responseBytes.toString('utf8');
  const data = raw ? responseBytes : text.startsWith('event:') || text.startsWith('data:')
    ? text.split('\n').filter((line) => line.startsWith('data:')).map((line) => JSON.parse(line.slice(5)))[0]
    : (text ? JSON.parse(text) : null);
  return { status: response.status, data, cookies: response.headers.getSetCookie(), retryAfter: response.headers.get('x-retry-after') ?? response.headers.get('retry-after') };
}

async function check(name, action) {
  try {
    await action();
    checks.push({ name, status: 'PASS' });
    console.log(`PASS ${name}`);
  } catch (error) {
    // Avoid serializing assertion objects or provider bodies, which can carry secrets.
    checks.push({ name, status: 'FAIL', ...failure(error) });
    console.log(`FAIL ${name}`);
  }
}

async function account(label) {
  const password = randomBytes(24).toString('hex');
  secrets.push(password);
  const email = `doop-test-${label.toLowerCase()}@example.test`;
  const signup = () => request('/api/auth/sign-up/email', {
    method: 'POST', body: { name: `Test ${label}`, email, password },
  });
  let response = await signup();
  if (response.status === 429) {
    // Better Auth rejects before signup effects; only this explicit rejection
    // permits one paced retry. Timeouts/unknown write outcomes are never retried.
    const seconds = Number(response.retryAfter);
    assert.ok(Number.isInteger(seconds) && seconds >= 1 && seconds <= 10);
    await new Promise((resolve) => setTimeout(resolve, (seconds + 1) * 1000));
    response = await signup();
    assert.equal(response.status, 200);
    checks.push({ name: 'production signup limit honored before the fourth disposable account', status: 'PASS' });
  }
  assert.equal(response.status, 200, 'normal signup failed');
  const cookie = response.cookies.map((value) => value.split(';')[0]).join('; ');
  assert.ok(cookie);
  secrets.push(cookie, ...response.cookies);
  if (response.data.token) secrets.push(response.data.token);
  replacements.push([response.data.user.id, `user-${label}`]);
  return { label, email, cookie, id: response.data.user.id };
}

async function api(actor, path, method = 'GET', body) {
  return request(path, { method, body, cookie: actor.cookie });
}

async function mcp(actor, method, params = {}) {
  const response = await request('/mcp', {
    method: 'POST', token: actor.token,
    body: { jsonrpc: '2.0', id: nextId++, method, params },
  });
  assert.equal(response.status, 200, 'authenticated MCP request failed');
  assert.ok(!response.data.error, 'MCP protocol error');
  return response.data.result;
}

async function call(actor, name, args = {}) {
  return mcp(actor, 'tools/call', { name, arguments: args });
}

function textData(result) {
  assert.ok(!result.isError, 'Doop returned a tool error');
  return JSON.parse(result.content.find((block) => block.type === 'text').text);
}

function sanitized(value) {
  let encoded = JSON.stringify(value);
  for (const [actual, replacement] of replacements) encoded = encoded.split(actual).join(replacement);
  encoded = encoded.split(origin).join('http://127.0.0.1:4400');
  for (const secret of secrets) if (secret && encoded.includes(secret)) throw new Error('secret in fixture output');
  if (/dpk_[\w-]{20,}/.test(encoded)) throw new Error('agent key in fixture output');
  return JSON.parse(encoded);
}

async function save(name, value) {
  await writeFile(join(output, name), JSON.stringify(sanitized(value), null, 2) + '\n');
}

async function provision() {
  const actors = {};
  for (const label of ['human-a', 'bot-a', 'human-b', 'bot-b']) actors[label] = await account(label);
  for (const company of ['a', 'b']) {
    const human = actors[`human-${company}`];
    const bot = actors[`bot-${company}`];
    const workspace = await api(human, '/api/workspaces', 'POST', { name: `Company ${company.toUpperCase()} synthetic` });
    assert.equal(workspace.status, 200);
    human.workspace = bot.workspace = workspace.data.id;
    replacements.push([workspace.data.id, `workspace-${company}`]);
    const member = await api(human, `/api/workspaces/${bot.workspace}/members`, 'POST', { email: bot.email });
    assert.equal(member.status, 200);
    if (process.env.DOOP_TEST_OPENBOT_BINARY) {
      const verification = await api(human, '/api/canvases', 'POST', { name: 'Human setup verification', workspaceId: bot.workspace });
      assert.equal(verification.status, 200);
      assert.equal(typeof verification.data.id, 'string');
      bot.verificationCanvas = verification.data.id;
      replacements.push([verification.data.id, `verification-canvas-${company}`]);
    }
    const key = await api(bot, '/api/agent-keys', 'POST', { name: 'Disposable MCP fixture' });
    assert.equal(key.status, 200);
    bot.token = key.data.secret;
    bot.key = key.data.id;
    secrets.push(bot.token);
    replacements.push([bot.key, `key-${company}`]);
    const welcome = await api(bot, '/api/canvases');
    assert.equal(welcome.status, 200);
    assert.ok(Array.isArray(welcome.data));
    for (const canvas of welcome.data) {
      if (canvas.ownerId === bot.id && !canvas.workspaceId) {
        assert.equal((await api(bot, `/api/canvases/${canvas.id}`, 'DELETE')).status, 200);
      }
    }
  }
  return actors;
}

async function contracts(actors) {
  const bot = actors['bot-a'];
  const other = actors['bot-b'];
  const initialize = await mcp(bot, 'initialize', {
    protocolVersion: '2025-03-26', capabilities: {}, clientInfo: { name: 'openbot-doop-contract', version: '1' },
  });
  assert.equal(initialize.serverInfo.name, 'doop-canvas');
  await save('initialize.json', initialize);
  const catalog = await mcp(bot, 'tools/list');
  assert.ok(catalog.tools.length > 0 && catalog.tools.length <= 100);
  if (suite === 'all') {
    for (const tool of catalog.tools) {
      assert.equal(tool.inputSchema.$schema, 'https://json-schema.org/draft/2020-12/schema');
      if (tool.outputSchema) assert.equal(tool.outputSchema.$schema, 'https://json-schema.org/draft/2020-12/schema');
    }
    for (const name of ['create_canvas', 'get_canvas']) {
      assert.ok(catalog.tools.find((tool) => tool.name === name)?.outputSchema);
    }
  }
  for (const name of ['get_guide', 'get_profile', 'list_canvases', 'create_canvas', 'get_canvas', 'create_frame', 'get_frame', 'set_frame_html', 'get_frame_screenshot', 'export_frame']) {
    assert.ok(catalog.tools.some((tool) => tool.name === name), `missing ${name}`);
  }
  await save('tools.json', catalog);
  const guide = await call(bot, 'get_guide', { topic: 'doop-instructions' });
  assert.ok(!guide.isError && guide.content.some((block) => block.type === 'text'));
  await save('guide.json', guide);
  const profile = await call(bot, 'get_profile');
  assert.equal(profile.structuredContent.id, bot.id);
  if (suite === 'all') assert.equal(profile.structuredContent.managed_workspace_id, bot.workspace);
  await save('profile.json', profile);
  const canvases = await call(bot, 'list_canvases');
  assert.ok(Array.isArray(textData(canvases)));
  await save('list-canvases.json', canvases);
  const created = await call(bot, 'create_canvas', { name: 'Synthetic contract', workspace_id: bot.workspace });
  const canvas = textData(created);
  assert.equal(typeof canvas.id, 'string');
  assert.ok(canvas.id.length > 0 && canvas.id.length <= 128);
  if (suite === 'all') {
    assert.equal(created.structuredContent.id, canvas.id);
    assert.equal(created.structuredContent.workspace_id, bot.workspace);
  }
  bot.canvas = canvas.id;
  replacements.push([canvas.id, 'canvas-a']);
  await save('create-canvas.json', created);
  const frame = textData(await call(bot, 'create_frame', {
    canvas_id: canvas.id, name: 'Synthetic frame', html: '', width: 480, height: 320, x: 0, y: 0, agent_name: 'Fixture',
  })).frame;
  assert.equal(typeof frame.id, 'string');
  assert.ok(frame.id.length > 0 && frame.id.length <= 128);
  bot.frame = frame.id;
  replacements.push([frame.id, 'frame-a']);
  const html = '<!doctype html><html><body><h1>Synthetic contract</h1></body></html>';
  const edited = await call(bot, 'set_frame_html', {
    frame_id: frame.id, html, agent_name: 'Fixture',
  });
  assert.equal(textData(edited).ok, true);
  assert.equal(textData(edited).frame.id, frame.id);
  const readFrame = await call(bot, 'get_frame', { frame_id: frame.id });
  const savedFrame = textData(readFrame);
  assert.equal(savedFrame.id, frame.id);
  assert.equal(savedFrame.html, html, 'edited content must be readable after acknowledgement');
  await save('get-frame.json', readFrame);
  const readCanvas = await call(bot, 'get_canvas', { canvas_id: canvas.id });
  const savedCanvas = textData(readCanvas);
  assert.equal(savedCanvas.id, canvas.id);
  assert.ok(savedCanvas.frames.some((entry) => entry.id === frame.id));
  if (suite === 'all') {
    assert.equal(readCanvas.structuredContent.canvas.id, canvas.id);
    assert.equal(readCanvas.structuredContent.workspace_id, bot.workspace);
  }
  await save('get-canvas.json', readCanvas);
  // Independently cross-check placement through the actual listing owner.
  const placed = textData(await call(bot, 'list_canvases'));
  assert.ok(Array.isArray(placed));
  assert.equal(placed.find((entry) => entry.id === canvas.id)?.workspace_id, bot.workspace);
  const denied = await call(other, 'get_canvas', { canvas_id: canvas.id });
  assert.equal(denied.isError, true);
  await save('foreign-canvas-error.json', denied);
  checks.push({ name: 'authenticated MCP discovery, profile, fixed-workspace creation, frame write/read and foreign-ID denial', status: 'PASS' });
  await check('real Chromium screenshot returns an inline PNG image', async () => {
    const screenshot = await call(bot, 'get_frame_screenshot', { frame_id: frame.id, scale: 1 });
    assert.ok(!screenshot.isError);
    const image = screenshot.content.find((block) => block.type === 'image');
    assert.equal(image.mimeType, 'image/png');
    assert.equal(Buffer.from(image.data, 'base64').subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
    await save('screenshot.json', screenshot);
  });
  await check('real Chromium export returns an embedded PDF resource', async () => {
    const pdf = await call(bot, 'export_frame', { frame_id: frame.id, format: 'pdf' });
    assert.ok(!pdf.isError);
    const resource = pdf.content.find((block) => block.type === 'resource').resource;
    assert.equal(resource.mimeType, 'application/pdf');
    assert.equal(Buffer.from(resource.blob, 'base64').subarray(0, 5).toString(), '%PDF-');
    await save('pdf.json', pdf);
  });
}

async function prepareIsolation(actors) {
  const bot = actors['bot-a'];
  const human = actors['human-a'];
  const canvas = await api(bot, '/api/canvases', 'POST', { name: 'Pre-policy sync fixture', workspaceId: bot.workspace });
  assert.equal(canvas.status, 200);
  bot.syncCanvas = canvas.data.id;
  bot.retainedSyncKeys = [];
  for (const actor of [bot, human]) {
    const key = await api(actor, `/api/canvases/${bot.syncCanvas}/sync-keys`, 'POST', { name: 'Disposable retained key' });
    assert.equal(key.status, 200);
    assert.match(key.data.secret, /^dk_/);
    secrets.push(key.data.secret);
    bot.retainedSyncKeys.push(key.data.secret);
    assert.equal((await request(`/ingest/${key.data.secret}`, { method: 'POST', body: {
      page: `/positive-${actor.label}`, html: '<h1>Valid pre-policy snapshot</h1>',
    } })).status, 200);
    assert.equal((await request(`/ingest/${key.data.secret}`, { method: 'POST', body: {
      page: `/positive-${actor.label}-target`, html: '<h1>Valid pre-policy snapshot</h1>',
      edges: [{ from: `/positive-${actor.label}`, to: `/positive-${actor.label}-target` }],
    } })).status, 200);
  }
  const listed = await api(bot, `/api/canvases/${bot.syncCanvas}/sync-keys`);
  assert.equal(listed.status, 200);
  assert.ok(listed.data.some((key) => key.secret === bot.retainedSyncKeys[1]), 'bot must actually retain the human-minted key');
  bot.syncBefore = (await api(human, `/api/canvases/${bot.syncCanvas}`)).data.frames;
  bot.flowBefore = (await api(human, `/api/canvases/${bot.syncCanvas}/sync-flow`)).data;
  assert.equal(bot.syncBefore.length, 4);
  assert.equal(bot.flowBefore.edges.length, 2);
  const metadata = JSON.parse(await readFile(process.env.DOOP_TEST_SEED_METADATA, 'utf8'));
  assert.ok(metadata.ownerlessCanvasIds.length > 0);
  bot.seed = metadata.ownerlessCanvasIds[0];
  checks.push({ name: 'valid self-minted and listed human sync keys work before policy activation', status: 'PASS' });
}

async function retainedSyncDenied(actors) {
  const bot = actors['bot-a'];
  const human = actors['human-a'];
  for (const [index, secret] of bot.retainedSyncKeys.entries()) {
    const page = index === 0 ? '/positive-bot-a' : '/positive-human-a';
    assertDenied(await request(`/ingest/${secret}`, { method: 'POST', body: {
      page: '/forbidden-retained', html: '<h1>Forbidden sync sentinel</h1>',
      links: [{ to: '/positive-human-a', x: 1, y: 1, w: 10, h: 10 }],
      edges: [{ from: page, to: page + '-target' }],
    } }));
  }
  const canvas = await api(human, `/api/canvases/${bot.syncCanvas}`);
  const flow = await api(human, `/api/canvases/${bot.syncCanvas}/sync-flow`);
  assert.equal(canvas.status, 200);
  assert.equal(flow.status, 200);
  assert.deepEqual(canvas.data.frames, bot.syncBefore, 'denied ingest must not alter frames');
  assert.deepEqual(flow.data, bot.flowBefore, 'denied ingest must not alter edges');
}

async function until(predicate) {
  const deadline = Date.now() + 5000;
  while (!predicate() && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 20));
  assert.ok(predicate(), 'WebSocket observation timeout');
}

async function roomClient(actor, canvasId, mustJoin = true) {
  const { default: WebSocket } = await import(pathToFileURL(process.env.DOOP_TEST_WS_MODULE));
  const endpoint = new URL('/ws', origin);
  endpoint.protocol = target.protocol === 'https:' ? 'wss:' : 'ws:';
  const socket = new WebSocket(endpoint, { headers: { Cookie: actor.cookie, Origin: origin } });
  const state = { socket, messages: [], closed: false, error: false };
  socket.on('error', (error) => {
    state.error = true;
    state.errorCode = typeof error.code === 'string' && /^[A-Z_0-9]{1,64}$/.test(error.code) ? error.code : 'UNKNOWN';
  });
  socket.on('close', () => { state.closed = true; });
  socket.on('message', (raw) => {
    if (state.messages.length >= 128 || raw.length > 1024 * 1024) { socket.terminate(); state.error = true; return; }
    state.messages.push(String(raw));
  });
  await until(() => socket.readyState === WebSocket.OPEN || state.closed || state.error);
  if (state.error) await save('room-connection-error.json', { actor: actor.id, code: state.errorCode, protocol: endpoint.protocol });
  assert.equal(state.error, false);
  assert.equal(socket.readyState, WebSocket.OPEN);
  socket.send(JSON.stringify({ type: 'join', canvasId, clientId: randomBytes(8).toString('hex'), name: 'Synthetic peer', kind: 'user' }));
  await until(() => state.messages.some((raw) => JSON.parse(raw).type === 'init') || state.closed || state.error);
  assert.equal(state.error, false);
  assert.equal(state.messages.some((raw) => JSON.parse(raw).type === 'init'), mustJoin);
  return state;
}

async function liveRevocation(actors) {
  const bot = actors['bot-a'];
  const human = actors['human-a'];
  const removedHuman = actors['human-b'];
  assert.equal((await api(human, `/api/workspaces/${bot.workspace}/members`, 'POST', { email: removedHuman.email })).status, 200);
  const joined = [];
  try {
    for (const actor of [human, bot, removedHuman]) joined.push(await roomClient(actor, bot.canvas));
    const [authorized, revokedBot, revokedHuman] = joined;
    for (const [actor, revoked] of [[bot, revokedBot], [removedHuman, revokedHuman]]) {
      assert.equal((await api(human, `/api/workspaces/${bot.workspace}/members/${actor.id}`, 'DELETE')).status, 200);
      const sentinel = `after-removal-${randomBytes(12).toString('hex')}`;
      await callHumanEdit(human, bot.frame, sentinel);
      await until(() => authorized.messages.some((raw) => raw.includes(sentinel)) && revoked.closed);
      assert.ok(!revoked.messages.some((raw) => raw.includes(sentinel)), 'revoked peer received post-removal content');
      const reconnect = await roomClient(actor, bot.canvas, false);
      joined.push(reconnect);
      assert.equal(reconnect.closed, true);
    }
  } finally { for (const peer of joined) peer.socket.terminate(); }
}

async function callHumanEdit(human, frameId, sentinel) {
  const response = await api(human, `/api/frames/${frameId}`, 'PATCH', { html: `<h1>${sentinel}</h1>` });
  assert.equal(response.status, 200);
}

async function isolation(actors) {
  const human = actors['human-a'];
  const bot = actors['bot-a'];
  const foreign = actors['human-b'];
  await check('both bots can work in their own company and foreign humans and frames are denied', async () => {
    const other = actors['bot-b'];
    const canvas = textData(await call(other, 'create_canvas', { name: 'Company B positive', workspace_id: other.workspace }));
    assert.equal(textData(await call(other, 'get_canvas', { canvas_id: canvas.id })).id, canvas.id);
    assert.equal((await call(other, 'get_frame', { frame_id: bot.frame })).isError, true);
    assertDenied(await api(foreign, `/api/canvases/${bot.canvas}`));
    assertDenied(await api(bot, `/api/canvases/${bot.canvas}/members`, 'POST', { email: foreign.email }));
    const copy = await api(bot, `/api/canvases/${bot.canvas}/duplicate`, 'POST');
    assert.equal(copy.status, 200);
    assert.equal(copy.data.workspaceId, bot.workspace);
  });
  await check('ordinary unmanaged design sync remains available after policy activation', async () => {
    const canvas = await api(foreign, '/api/canvases', 'POST', { name: 'Unmanaged sync positive' });
    assert.equal(canvas.status, 200);
    const key = await api(foreign, `/api/canvases/${canvas.data.id}/sync-keys`, 'POST', { name: 'Unmanaged key' });
    assert.equal(key.status, 200);
    secrets.push(key.data.secret);
    assert.equal((await request(`/ingest/${key.data.secret}`, { method: 'POST', body: { page: '/ordinary', html: '<h1>Ordinary sync positive</h1>' } })).status, 200);
    const saved = await api(foreign, `/api/canvases/${canvas.data.id}`);
    assert.equal(saved.data.frames.length, 1);
    assert.ok(saved.data.frames[0].html.includes('Ordinary sync positive'));
  });
  await check('applied company policy denies sync mint/list and retained keys without frame or edge effects', async () => {
    assertDenied(await api(human, `/api/canvases/${bot.syncCanvas}/sync-keys`));
    assertDenied(await api(human, `/api/canvases/${bot.syncCanvas}/sync-keys`, 'POST', { name: 'Forbidden' }));
    await retainedSyncDenied(actors);
  });
  await check('managed bot cannot claim a valid ownerless personal canvas; ordinary human can', async () => {
    assertDenied(await api(bot, `/api/canvases/${bot.seed}/claim`, 'POST'));
    assert.equal((await api(foreign, `/api/canvases/${bot.seed}/claim`, 'POST')).status, 200);
    assert.equal((await api(foreign, `/api/canvases/${bot.seed}`)).data.ownerId, foreign.id);
  });
  await check('managed bot cannot create another workspace or be promoted', async () => {
    assertDenied(await api(bot, '/api/workspaces', 'POST', { name: 'Forbidden bot workspace' }));
    assertDenied(await api(human, `/api/workspaces/${bot.workspace}/members/${bot.id}`, 'PATCH', { role: 'admin' }), [400, 403, 404]);
  });
  await check('private company canvas cannot enter Community or be deleted through workspace detachment', async () => {
    assertDenied(await api(bot, `/api/canvases/${bot.canvas}/publish`, 'PUT', { description: 'Synthetic private fixture', category: 'other' }));
    assertDenied(await api(human, `/api/workspaces/${bot.workspace}`, 'DELETE'));
    assert.equal((await api(human, `/api/canvases/${bot.canvas}`)).data.workspaceId, bot.workspace);
  });
  await check('bot cannot create a canvas in a foreign workspace', async () => {
    const result = await call(bot, 'create_canvas', { name: 'Forbidden foreign', workspace_id: foreign.workspace });
    assert.equal(Boolean(result.isError), true);
  });
  await check('managed bot cannot create an unrelated personal canvas', async () => {
    const result = await call(bot, 'create_canvas', { name: 'Forbidden personal' });
    assert.equal(result.isError, true);
  });
  await check('managed canvas owner cannot enable public link access', async () => {
    const result = await api(bot, `/api/canvases/${bot.canvas}`, 'PATCH', { linkAccess: 'edit' });
    try { assertDenied(result); }
    finally { await api(bot, `/api/canvases/${bot.canvas}`, 'PATCH', { linkAccess: 'none' }); }
  });
  await check('foreign company cannot invite the managed bot into its workspace', async () => {
    const result = await api(foreign, `/api/workspaces/${foreign.workspace}/members`, 'POST', { email: bot.email });
    try { assertDenied(result, [400, 403, 404]); }
    finally {
      if (result.status === 200) await api(foreign, `/api/workspaces/${foreign.workspace}/members/${bot.id}`, 'DELETE');
    }
  });
  await check('managed canvas cannot move into an unrelated personal space', async () => {
    const result = await api(bot, `/api/canvases/${bot.canvas}/workspace`, 'PUT', { workspaceId: null });
    try { assertDenied(result); }
    finally {
      if (result.status === 200) await api(bot, `/api/canvases/${bot.canvas}/workspace`, 'PUT', { workspaceId: bot.workspace });
    }
  });
  await check('foreign canvas owner cannot directly invite the managed bot', async () => {
    const canvas = await api(foreign, '/api/canvases', 'POST', { name: 'Foreign direct invitation fixture', workspaceId: foreign.workspace });
    assert.equal(canvas.status, 200);
    const result = await api(foreign, `/api/canvases/${canvas.data.id}/members`, 'POST', { email: bot.email });
    try { assertDenied(result, [400, 403, 404]); }
    finally {
      if (result.status === 200) await api(foreign, `/api/canvases/${canvas.data.id}/members/${bot.id}`, 'DELETE');
    }
  });
  await check('competing foreign invitations and personal creates cannot widen the bot audience', async () => {
    const results = await Promise.all([
      api(foreign, `/api/workspaces/${foreign.workspace}/members`, 'POST', { email: bot.email }),
      api(foreign, `/api/workspaces/${foreign.workspace}/members`, 'POST', { email: bot.email }),
      api(bot, '/api/canvases', 'POST', { name: 'Competing forbidden personal one' }),
      api(bot, '/api/canvases', 'POST', { name: 'Competing forbidden personal two' }),
    ]);
    for (const result of results) assertDenied(result, [400, 403, 404]);
    const listing = textData(await call(bot, 'list_canvases'));
    assert.ok(listing.every((canvas) => canvas.workspace_id === bot.workspace));
  });
  await check('joined bot and human lose live delivery on removal while authorized peer receives each sentinel', () => liveRevocation(actors));
  await check('revoked owner loses read/list/frame/duplicate access and workspace creation', async () => {
    const result = await call(bot, 'create_canvas', { name: 'Revoked workspace', workspace_id: human.workspace });
    assert.equal(result.isError, true);
    assert.equal((await call(bot, 'get_canvas', { canvas_id: bot.canvas })).isError, true);
    assert.equal((await call(bot, 'get_frame', { frame_id: bot.frame })).isError, true);
    assert.ok(!textData(await call(bot, 'list_canvases')).some((canvas) => canvas.id === bot.canvas));
    assertDenied(await api(bot, `/api/canvases/${bot.canvas}/duplicate`, 'POST'));
    await retainedSyncDenied(actors);
  });
  await check('restart preserves binding, revocation, claim denial and retained-key denial', async () => {
    await control({ kind: 'restart' });
    assert.equal((await call(bot, 'get_profile')).structuredContent.managed_workspace_id, bot.workspace);
    assert.equal((await call(bot, 'get_canvas', { canvas_id: bot.canvas })).isError, true);
    assert.equal((await call(bot, 'create_canvas', { name: 'Still revoked', workspace_id: bot.workspace })).isError, true);
    assert.equal((await api(foreign, `/api/canvases/${bot.seed}`)).data.ownerId, foreign.id);
    await retainedSyncDenied(actors);
  });
  await check('revoked bot key denies the next MCP request', async () => {
    assert.equal((await api(bot, `/api/agent-keys/${bot.key}`, 'DELETE')).status, 200);
    const result = await request('/mcp', {
      method: 'POST', token: bot.token,
      body: { jsonrpc: '2.0', id: nextId++, method: 'tools/call', params: { name: 'get_profile', arguments: {} } },
    });
    assert.equal(result.status, 401);
  });
}

let privateProbe;
try {
  validateOrigin();
  const actors = await provision();
  if (suite === 'all') await prepareIsolation(actors);
  await control({ kind: 'configure', bindings: ['a', 'b'].map((company) => ({
    workspaceId: actors[`bot-${company}`].workspace, botUserId: actors[`bot-${company}`].id,
  })) });
  if (suite === 'all') {
    const bot = actors['bot-a'];
    const current = await api(actors['human-a'], `/api/canvases/${bot.syncCanvas}`);
    assert.equal(current.status, 200);
    assert.equal(current.data.frames.length, bot.syncBefore.length);
    assert.ok(current.data.frames.every((frame) => frame.html.includes('Valid pre-policy snapshot')));
    // Hydration may supply canonical optional fields; freeze the persisted form.
    bot.syncBefore = current.data.frames;
    bot.flowBefore = (await api(actors['human-a'], `/api/canvases/${bot.syncCanvas}/sync-flow`)).data;
  }
  checks.push({ name: 'normal signup, separate human/bot accounts, workspaces and key minting', status: 'PASS' });
  if (suite === 'e2e') {
    const { e2eAcceptance } = await import('./e2e-probe.mjs');
    privateProbe = await e2eAcceptance({ origin, actors, check, call, request, api, textData, secrets, output,
      source: process.env.DOOP_TEST_SOURCE });
  } else {
    await openbotSetup(actors);
    if (privateDeployment) {
      const { lifecycleAcceptance } = await import('./lifecycle-probe.mjs');
      await lifecycleAcceptance({ origin, actors, check, api, call, request, control, secrets, replacements, output, sanitized });
    }
    await contracts(actors);
    if (privateDeployment) {
      const { privateAcceptance } = await import('./private-probe.mjs');
      privateProbe = await privateAcceptance({ origin, actors, check, call, secrets, output, source: process.env.DOOP_TEST_SOURCE });
    }
    if (suite === 'all') await isolation(actors);
    await privateProbe?.afterRevocation();
  }
} catch (error) {
  checks.push({ name: 'local-service setup/contract probe', status: 'FAIL', ...failure(error) });
  console.error('FAIL local-service setup/contract probe');
} finally {
  if (privateProbe) await check('private acceptance browser cleanup', () => privateProbe.close());
  controls.close();
  process.stdin.destroy();
  await save('report.json', { version: 1, suite, requests, privateRequests: privateProbe?.requests, checks });
  process.exitCode = checks.some((entry) => entry.status === 'FAIL') ? 1 : 0;
}
