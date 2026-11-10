// Actual Openbot workflows and ordinary human consumers of their real private Doop documents.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { companyInput, runOpenbotHelper } from './openbot-helper.mjs';
import { createBrowserOwner } from './browser-owner.mjs';
import { downloadCanvasPdf } from './browser-export.mjs';
import { contextPage, currentPrivateFrame, humanBrowserProof } from './private-browser.mjs';

export function assertDocumentReadback({ item, bot, canvas, frame }) {
  assert.equal(canvas.canvas.id, item.canvas_id);
  assert.equal(canvas.workspace_id, bot.workspace);
  assert.ok(canvas.frames.some((candidate) => candidate.id === item.frame_id));
  assert.equal(frame.id, item.frame_id);
  assert.equal(typeof frame.html, 'string');
  assert.ok(frame.html.includes(item.expected_targeted_edit), 'fresh bot read must contain the accepted replacement');
  assert.ok(!frame.html.includes(item.expected_old_text), 'fresh bot read must no longer contain the replaced text');
}

async function browserImageProof({ page, child, sharp, color, output, name }) {
  assert.equal(child.parentFrame(), page.mainFrame());
  const element = await child.frameElement();
  const parent = await element.evaluate((node) => {
    const box = node.getBoundingClientRect();
    return { x: box.x, y: box.y, width: box.width, height: box.height,
      clientWidth: node.clientWidth, clientHeight: node.clientHeight, borderX: node.clientLeft, borderY: node.clientTop };
  });
  assert.ok(parent.clientWidth > 0 && parent.clientHeight > 0);
  assert.equal(parent.borderX, 0); assert.equal(parent.borderY, 0);
  const local = await child.$eval('img[data-private-image]', (node) => {
    const box = node.getBoundingClientRect();
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  });
  const screenshot = Buffer.from(await page.screenshot());
  assert.ok(screenshot.length <= 1024 * 1024);
  await writeFile(join(output, name + '.png'), screenshot, { mode: 0o600 });
  const pixels = await sharp(screenshot).removeAlpha().raw().toBuffer({ resolveWithObject: true });
  const x = Math.floor((parent.x + local.x * parent.width / parent.clientWidth) * pixels.info.width / page.viewport().width);
  const y = Math.floor((parent.y + local.y * parent.height / parent.clientHeight) * pixels.info.height / page.viewport().height);
  assert.ok(x >= 0 && x < pixels.info.width && y >= 0 && y < pixels.info.height);
  const at = (y * pixels.info.width + x) * pixels.info.channels;
  const observed = [...pixels.data.subarray(at, at + 3)];
  await writeFile(join(output, name + '.json'), JSON.stringify({ expected: color, observed, x, y,
    parent, local, view_action: 'ordinary Zoom to fit; actual viewport pixels at scaled frame image center' }), { mode: 0o600 });
  assert.deepEqual(observed, color);
}

export async function e2eAcceptance({ origin, actors, check, call, request, api, textData, secrets, output, source }) {
  const require = createRequire(join(source, 'package.json'));
  const sharp = require('sharp');
  const { PDFDocument, PDFDict, PDFName } = require('pdf-lib');
  const mupdf = await import(pathToFileURL(require.resolve('mupdf')).href);
  const browserOwner = createBrowserOwner(require('puppeteer-core'));
  const data = (result) => { assert.notEqual(result.isError, true); return result.structuredContent ?? textData(result); };
  let requests = 0;
  let record;
  const raw = async (path, options = {}) => {
    assert.ok(++requests <= 96, 'private acceptance request budget exceeded');
    return request(path, options);
  };
  const canonical = (value) => {
    const url = new URL(value, origin);
    assert.equal(url.origin, origin);
    assert.ok(!url.username && !url.password && !url.search && !url.hash);
    return url.pathname;
  };
  await check('live two-company Openbot document workflows and current approvals', async () => {
    const result = await runOpenbotHelper({
      testName: 'adapters::persistence::workflow::tests::mcp_tests::doop_setup_tests::document_tests::live_tests::doop_e2e_live_company_documents',
      marker: 'DOOP_OPENBOT_E2E_PASS companies=2 documents=2 creates=3 unknown=1',
      recordName: 'e2e-record', input: companyInput(origin, actors), output, secrets,
    });
    assert.equal(result.version, 1);
    assert.equal(result.origin, origin);
    assert.equal(result.source.fingerprint, process.env.DOOP_TEST_SOURCE_FINGERPRINT);
    assert.equal(result.source.revision, process.env.DOOP_TEST_SOURCE_REVISION);
    assert.equal(result.companies.length, 2);
    assert.equal(result.worker_groups, 2);
    assert.equal(result.snapshot_guard, 'enabled before and after');
    assert.equal(result.paid_live_inference, 'NOT EXERCISED');
    for (const [index, item] of result.companies.entries()) {
      const bot = actors[`bot-${['a', 'b'][index]}`];
      assert.equal(item.bot_user_id, bot.id);
      assert.equal(item.workspace_id, bot.workspace);
      assert.equal(item.canvas_url, `${origin}/c/${item.canvas_id}`);
      assert.equal(item.model_exchanges, 5);
      assert.equal(item.status, 'draft');
      assert.equal(item.visual_review, 'not_performed');
      assert.equal(item.current_audience_delivery, true);
      assert.equal(typeof item.frame_id, 'string');
      assert.equal(typeof item.expected_targeted_edit, 'string');
      assert.equal(typeof item.expected_old_text, 'string');
      assert.ok(item.expected_targeted_edit.length > 0 && item.expected_old_text.length > 0);
      assert.equal(item.post_edit_verification, 'fresh bot read required before human enrichment');
    }
    assert.notEqual(result.companies[0].canvas_id, result.companies[1].canvas_id);
    assert.notEqual(result.companies[0].frame_id, result.companies[1].frame_id);
    record = result;
  });
  assert.ok(record, 'current native two-company workflow must pass before browser consumers');
  await check('live worker restart reuses known creates and parks a lost receipt', async () => {
    assert.equal(record.known_create_worker_restart, true);
    assert.equal(record.lost_receipt.outcome, 'unknown');
    assert.equal(record.lost_receipt.manual_reconciliation, true);
    assert.equal(record.lost_receipt.blind_replay, false);
    assert.equal(record.lost_receipt.document_notifications, 0);
    assert.equal(record.lost_receipt.independent_no_effect_workflow, 'succeeded');
    const partial = await api(actors['human-a'], `/api/canvases/${record.lost_receipt.canvas_id}`);
    assert.equal(partial.status, 200);
    assert.equal(partial.data.id, record.lost_receipt.canvas_id);
    assert.equal(partial.data.workspaceId, actors['bot-a'].workspace);
    assert.equal(partial.data.frames.length, 0);
    assert.ok(record.companies.every((item) => item.canvas_id !== partial.data.id));
  });
  const originals = [];
  const freshReadbacks = [];
  await check('live document edits persist and bot keys reopen and export', async () => {
    for (const [index, item] of record.companies.entries()) {
      const bot = actors[`bot-${['a', 'b'][index]}`];
      const canvas = data(await call(bot, 'get_canvas', { canvas_id: item.canvas_id }));
      const frame = data(await call(bot, 'get_frame', { frame_id: item.frame_id }));
      assertDocumentReadback({ item, bot, canvas, frame });
      const exported = await call(bot, 'get_frame_screenshot', { frame_id: item.frame_id });
      assert.notEqual(exported.isError, true);
      const image = exported.content.find((part) => part.type === 'image');
      assert.ok(image);
      const info = textData(exported);
      assert.equal(info.width, 800); assert.equal(info.height, 1000);
      assert.equal(info.format, 'png'); assert.equal(info.scale, 1);
      const bytes = Buffer.from(image.data, 'base64');
      assert.ok(bytes.length > 0 && bytes.length <= 12288);
      const actual = await sharp(bytes).metadata();
      assert.ok(['png', 'jpeg'].includes(actual.format));
      assert.equal(image.mimeType, `image/${actual.format}`);
      assert.equal(info.inline.mime_type, image.mimeType);
      assert.equal(actual.width, info.inline.width); assert.equal(actual.height, info.inline.height);
      assert.equal(info.inline.full_resolution, actual.width === info.width && actual.height === info.height);
      assert.ok(actual.width <= info.width && actual.height <= info.height);
      const extension = actual.format === 'jpeg' ? 'jpg' : 'png';
      await writeFile(join(output, `e2e-bot-${index + 1}.${extension}`), bytes, { mode: 0o600 });
      originals.push(frame.html);
      freshReadbacks.push({ canvas_id: item.canvas_id, frame_id: item.frame_id,
        html_sha256: createHash('sha256').update(frame.html).digest('hex'), replacement_present: true, old_text_absent: true,
        export_mime_type: image.mimeType, inline_full_resolution: info.inline.full_resolution });
    }
  });
  assert.equal(originals.length, 2, 'both actual draft readbacks and bot exports must pass');
  await writeFile(join(output, 'e2e-persisted-edits.json'), JSON.stringify({ version: 1,
    proof: 'fresh current-key bot MCP reads before any human enrichment', companies: freshReadbacks }), { mode: 0o600 });
  const fontBytes = await readFile(join(source, 'tests/fixtures/fonts/nunito-sans-bold-italic.woff2'));
  const enriched = [];
  for (const [index, item] of record.companies.entries()) {
    const human = actors[`human-${['a', 'b'][index]}`];
    const color = index ? [26, 48, 210] : [13, 165, 67];
    const image = await sharp({ create: { width: 16, height: 16, channels: 3,
      background: { r: color[0], g: color[1], b: color[2] } } }).png().toBuffer();
    const upload = await raw(`/api/canvases/${item.canvas_id}/assets`, { method: 'POST', cookie: human.cookie,
      bytes: image, headers: { 'Content-Type': 'image/png' } });
    assert.equal(upload.status, 200);
    const font = await raw(`/api/fonts?canvas=${item.canvas_id}`, { method: 'POST', cookie: human.cookie,
      bytes: fontBytes, headers: { 'Content-Type': 'application/octet-stream', 'X-File-Name': 'nunito-sans-bold-italic.woff2' } });
    assert.equal(font.status, 200);
    const files = [[canonical(upload.data.url), image], [canonical(font.data.url), fontBytes]];
    assert.ok(originals[index].includes('</body>'));
    const html = originals[index].replace('</body>', `<style>html,body{margin:0;background:white}h1{position:absolute;top:80px;margin:0}.type{position:absolute;left:80px;top:0;font:italic 700 24px ${JSON.stringify(font.data.family)},serif}</style><img data-private-image width="48" height="48" src="${upload.data.url}"><span class="type">Private WWW</span></body>`);
    assert.equal((await api(human, `/api/frames/${item.frame_id}`, 'PATCH', { html })).status, 200);
    for (const [path, expected] of files) {
      const current = await raw(path, { cookie: human.cookie, raw: true });
      assert.equal(current.status, 200);
      assert.deepEqual(current.data, expected);
    }
    const current = await api(human, `/api/canvases/${item.canvas_id}`);
    assert.equal(current.status, 200);
    enriched.push({ item, human, color, font: font.data, files, canvas: current.data,
      frame: current.data.frames.find((frame) => frame.id === item.frame_id) });
  }
  await writeFile(join(output, 'e2e-media-scope.json'), JSON.stringify({ version: 1,
    producer: 'normal authorized human enrichment after verified bot targeted edits',
    model_visual_review: 'not_performed', canvases: enriched.map(({ item }) => item.canvas_id) }), { mode: 0o600 });
  const browser = await browserOwner.launch();
  await check('live ordinary humans reopen their generated private documents', async () => {
    for (const [index, { item, human, color, font }] of enriched.entries()) {
      const state = await contextPage(browser, human);
      try {
        await state.page.goto(`${item.canvas_url}?frame=${item.frame_id}`, { waitUntil: 'domcontentloaded' });
        for (let opening = 0; opening < 2; opening++) {
          if (opening) await state.page.reload({ waitUntil: 'domcontentloaded' });
          const child = await currentPrivateFrame(state.page, font.family);
          assert.ok(await child.evaluate((text) => document.body.textContent.includes(text), item.expected_targeted_edit));
          // Fit the tall generated page using the ordinary canvas shortcut before
          // inspecting its visible pixels inside the editor's transformed stage.
          await state.page.keyboard.down('Shift');
          await state.page.keyboard.press('Digit1');
          await state.page.keyboard.up('Shift');
          await state.page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
          await browserImageProof({ page: state.page, child, sharp, color, output,
            name: `e2e-human-image-${index + 1}-${opening}` });
        }
      } finally { await state.context.close(); }
    }
  });
  await check('live human PDF controls export generated private documents', async () => {
    for (const [index, { item, human, canvas, frame, font }] of enriched.entries()) {
      const state = await contextPage(browser, human);
      try {
        await state.page.goto(`${item.canvas_url}?frame=${item.frame_id}`, { waitUntil: 'domcontentloaded' });
        await currentPrivateFrame(state.page, font.family);
        const bytes = await downloadCanvasPdf({ browser, page: state.page, origin, canvas,
          frames: [frame], directory: `ui-downloads-e2e-${index + 1}` });
        const pdf = await PDFDocument.load(bytes);
        assert.deepEqual(pdf.getPages().map((page) => [page.getWidth(), page.getHeight()]), [[600, 750]]);
        assert.ok(pdf.context.enumerateIndirectObjects().some(([, value]) => value instanceof PDFDict &&
          /Nunito/i.test(String(value.get(PDFName.of('FontName')))) &&
          (value.has(PDFName.of('FontFile2')) || value.has(PDFName.of('FontFile3')))));
        const rendered = mupdf.Document.openDocument(bytes, 'application/pdf');
        assert.equal(rendered.countPages(), 1);
        const text = rendered.loadPage(0).toStructuredText().asText();
        assert.ok(text.includes('Private WWW') && text.includes(item.expected_targeted_edit));
        await writeFile(join(output, `e2e-human-${index + 1}.pdf`), bytes, { mode: 0o600 });
      } finally { await state.context.close(); }
    }
  });
  await check('live generated document IDs and private media deny foreign and signed-out access', async () => {
    for (const [index, { item, canvas, font, files }] of enriched.entries()) {
      const other = ['b', 'a'][index];
      const bot = actors[`bot-${other}`];
      assert.equal((await call(bot, 'get_canvas', { canvas_id: item.canvas_id })).isError, true);
      assert.equal((await call(bot, 'get_frame', { frame_id: item.frame_id })).isError, true);
      const proof = await humanBrowserProof({ browser, origin, foreign: actors[`human-${other}`],
        ownCanvasId: record.companies[1 - index].canvas_id, canvas, frameId: item.frame_id,
        fontFamily: font.family, record: async () => {},
        media: [...files.map(([path]) => path), `/i/${item.frame_id}.png`,
          `/api/canvases/${item.canvas_id}/export.pdf?frames=${item.frame_id}`] });
      await proof.close();
      for (const [path, expected] of files) for (const cookie of [undefined, actors[`human-${other}`].cookie]) {
        const denied = await raw(path, { cookie, raw: true });
        assert.ok([401, 403, 404].includes(denied.status));
        assert.ok(!denied.data.equals(expected));
      }
    }
  });
  if (process.env.DOOP_TEST_PRIVATE_BUCKET_ORIGIN) await check('actual private bucket origin rejects unsigned known files', async () => {
    for (const { files } of enriched) for (const [path] of files) {
      assert.ok(++requests <= 96);
      const result = await fetch(process.env.DOOP_TEST_PRIVATE_BUCKET_ORIGIN + '/' + path.slice(3),
        { redirect: 'error', signal: AbortSignal.timeout(10000) });
      assert.equal(result.status, 403); await result.arrayBuffer();
    }
  });
  return { close: () => browserOwner.close(), get requests() { return requests; } };
}
