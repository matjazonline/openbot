// Actual key lifecycle and same-schema native PostgreSQL restore, with retained private storage.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { companyInput, runOpenbotHelper } from './openbot-helper.mjs';

export async function lifecycleAcceptance({ origin, actors, check, api, call, request, control, secrets, replacements, output, sanitized }) {
  assert.ok(process.env.DOOP_TEST_OPENBOT_BINARY, 'private lifecycle acceptance requires the explicit current native helper');
  const retired = [];
  const currentProfile = async (bot) => {
    const profile = (await call(bot, 'get_profile')).structuredContent;
    assert.equal(profile.id, bot.id);
    assert.equal(profile.managed_workspace_id, bot.workspace);
  };
  const retiredRefused = async (token) => {
    const response = await request('/mcp', { method: 'POST', token,
      body: { jsonrpc: '2.0', id: 'retired-key', method: 'tools/call', params: { name: 'get_profile', arguments: {} } } });
    assert.equal(response.status, 401, 'a previously valid, now revoked key must fail authentication');
    assert.ok(response.data.error && !response.data.result);
  };
  let rotated = false;
  await check('live Doop bot-key rotation revokes both old keys', async () => {
    for (const company of ['a', 'b']) {
      const bot = actors[`bot-${company}`];
      await currentProfile(bot);
      const minted = await api(bot, '/api/agent-keys', 'POST', { name: 'Rotated disposable MCP fixture' });
      assert.equal(minted.status, 200);
      assert.ok(minted.data.id !== bot.key && minted.data.secret !== bot.token);
      secrets.push(minted.data.secret);
      retired.push(bot.token);
      assert.equal((await api(bot, `/api/agent-keys/${bot.key}`, 'DELETE')).status, 200);
      await retiredRefused(bot.token);
      bot.token = minted.data.secret;
      bot.key = minted.data.id;
      replacements.push([bot.key, `rotated-key-${company}`]);
      await currentProfile(bot);
    }
    rotated = true;
  });
  assert.ok(rotated, 'key rotation must pass before dependent lifecycle or restore checks');
  const human = actors['human-a'];
  const canvasId = actors['bot-a'].verificationCanvas;
  const require = createRequire(join(process.env.DOOP_TEST_SOURCE, 'package.json'));
  const sharp = require('sharp');
  const bytes = await sharp({ create: { width: 2, height: 2, channels: 4,
    background: { r: 47, g: 121, b: 67, alpha: 1 } } }).png().toBuffer();
  const uploaded = await request(`/api/canvases/${canvasId}/assets`, {
    method: 'POST', cookie: human.cookie, bytes, headers: { 'Content-Type': 'image/png' },
  });
  assert.equal(uploaded.status, 200);
  const asset = new URL(uploaded.data.url, origin);
  assert.equal(asset.origin, origin);
  assert.ok(!asset.username && !asset.password && !asset.search && !asset.hash);
  const assetPath = asset.pathname;
  const beforeDisconnect = await request(assetPath, { cookie: human.cookie, raw: true });
  assert.equal(beforeDisconnect.status, 200);
  assert.deepEqual(beforeDisconnect.data, bytes);
  const verifyRemote = async () => {
    for (const company of ['a', 'b']) {
      const bot = actors[`bot-${company}`];
      const owner = actors[`human-${company}`];
      const canvas = await api(owner, `/api/canvases/${bot.verificationCanvas}`);
      assert.equal(canvas.status, 200);
      assert.equal(canvas.data.id, bot.verificationCanvas);
      assert.equal(canvas.data.workspaceId, bot.workspace);
      await currentProfile(bot);
    }
    const own = await request(assetPath, { cookie: human.cookie, raw: true });
    assert.equal(own.status, 200);
    assert.deepEqual(own.data, bytes);
    for (const cookie of [undefined, actors['human-b'].cookie]) {
      const denied = await request(assetPath, { cookie, raw: true });
      assert.ok([401, 403, 404].includes(denied.status));
      assert.ok(!denied.data.equals(bytes));
    }
  };
  let disconnected = false;
  await check('live frozen Openbot publications enforce current keys, disable and disconnect', async () => {
    const record = await runOpenbotHelper({
      testName: 'adapters::persistence::doop_live_setup_tests::lifecycle::doop_lifecycle_live_company_keys',
      marker: 'DOOP_OPENBOT_LIFECYCLE_PASS companies=2 retired_refused=2 disconnected=2',
      recordName: 'lifecycle-record', input: { base: companyInput(origin, actors), retired_tokens: retired }, output, secrets,
    });
    assert.equal(record.source_identity.fingerprint, process.env.DOOP_TEST_SOURCE_FINGERPRINT);
    assert.equal(record.companies.length, 2);
    assert.equal(record.remote_documents_deleted, false);
    for (let index = 0; index < 2; index++) {
      const item = record.companies[index];
      const bot = actors[`bot-${['a', 'b'][index]}`];
      assert.equal(item.bot_user_id, bot.id);
      assert.equal(item.workspace_id, bot.workspace);
      assert.equal(item.retired_key_refused && item.disabled_without_dispatch && item.disconnected_without_dispatch, true);
      assert.equal(item.credential_rows_after_disconnect, 0);
      assert.ok(item.initial_revision < item.retired_revision && item.retired_revision < item.restored_revision);
      assert.deepEqual(item.frozen_verification.result, {
        verified: true, bot_id: bot.id, workspace_id: bot.workspace, canvas_id: bot.verificationCanvas,
      });
    }
    sanitized(record);
    disconnected = true;
  });
  assert.ok(disconnected, 'live encrypted custody and frozen publications must pass before restore');
  await check('Openbot disconnect retains remote company documents and private assets', verifyRemote);
  await check('same-schema owned database restore retains private assets and revoked keys', async () => {
    await verifyRemote();
    await control({ kind: 'restore' });
    await verifyRemote();
    for (const token of retired) await retiredRefused(token);
  });
}
