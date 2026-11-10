// Actual final-image acceptance. Every action uses normal sessions or MCP keys.
import assert from 'node:assert/strict';
import { contextPage, currentPrivateFrame, humanBrowserProof } from './private-browser.mjs';
import { createBrowserOwner } from './browser-owner.mjs';
import { downloadCanvasPdf } from './browser-export.mjs';
import { createRequire } from 'node:module';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { request as httpsRequest } from 'node:https';

export async function privateAcceptance({ origin, actors, check, call, secrets, output, source }) {
  const require = createRequire(join(source, 'package.json'));
  const sharp = require('sharp');
  const puppeteer = require('puppeteer-core');
  const browserOwner = createBrowserOwner(puppeteer);
  const { PDFDocument, PDFDict, PDFName } = require('pdf-lib');
  const mupdf = await import(pathToFileURL(require.resolve('mupdf')).href);
  let requests = 0;
  const owner = actors['bot-a'];
  const human = actors['human-a'];
  const foreign = actors['human-b'];
  let browser;
  let page;
  let displayedChild;
  let humanProof;
  const runtime = process.env.DOOP_TEST_PRIVATE_RUNTIME;
  const raw = async (path, { actor, token, method = 'GET', json, bytes, headers = {}, base = origin, redirect = 'error' } = {}) => {
    assert.ok(++requests <= 96, 'private acceptance request budget exceeded');
    assert.ok(path.startsWith('/') && !path.startsWith('//'));
    const response = await fetch(base + path, {
      method, redirect, signal: AbortSignal.timeout(10000),
      headers: { Origin: origin, ...headers, ...(actor ? { Cookie: actor.cookie } : {}),
        ...(token ? { Authorization: `Bearer ${token}` } : {}), ...(json === undefined ? {} : { 'Content-Type': 'application/json' }) },
      body: json === undefined ? bytes : JSON.stringify(json),
    });
    const chunks = [];
    let length = 0;
    if (response.body) {
      const reader = response.body.getReader();
      try {
        while (true) {
          const next = await reader.read();
          if (next.done) break;
          length += next.value.byteLength;
          assert.ok(length <= 1024 * 1024, 'private fixture response exceeds its fixed bound');
          chunks.push(next.value);
        }
      } finally { await reader.cancel(); }
    }
    return { response, bytes: Buffer.concat(chunks), status: response.status };
  };
  const json = async (path, options = {}) => {
    const result = await raw(path, options);
    assert.equal(result.status, 200);
    return JSON.parse(result.bytes.toString());
  };
  const canonicalPath = (value) => {
    const url = new URL(value, origin);
    assert.equal(url.origin, origin);
    assert.ok(!url.username && !url.password && !url.search);
    return url.pathname;
  };
  const denied = (result, statuses = [401, 404]) => {
    assert.ok(statuses.includes(result.status), 'private request must deny before releasing bytes');
    assert.ok(result.bytes.length <= 4096);
    assert.ok(!result.bytes.includes(Buffer.from('89504e470d0a1a0a', 'hex')));
    assert.equal(result.response.headers.get('content-range'), null);
    assert.equal(result.response.headers.get('etag'), null);
    assert.ok(result.response.headers.get('cache-control')?.includes('no-store'));
  };
  const capture = async (name, value) => {
    const text = JSON.stringify(value, null, 2) + '\n';
    for (const secret of secrets) assert.ok(!text.includes(secret), 'credential appeared in a result capture');
    await writeFile(join(output, `private-${name}.json`), text, { mode: 0o600 });
  };
  const canvas = await json('/api/canvases', { actor: owner, method: 'POST',
    json: { name: 'Private image deployment specimen', workspaceId: owner.workspace } });
  assert.equal(typeof canvas.id, 'string');
  const colors = [[13, 165, 67], [26, 48, 210]];
  const images = [];
  const urls = [];
  for (const color of colors) {
    const bytes = await sharp({ create: { width: 16, height: 16, channels: 4,
      background: { r: color[0], g: color[1], b: color[2], alpha: 1 } } }).png().toBuffer();
    const uploaded = await json(`/api/canvases/${canvas.id}/assets`, { actor: owner, method: 'POST', bytes,
      headers: { 'Content-Type': 'image/png' } });
    canonicalPath(uploaded.url);
    images.push(bytes); urls.push(uploaded.url);
  }
  const fontBytes = await readFile(join(source, 'tests/fixtures/fonts/nunito-sans-bold-italic.woff2'));
  const font = await json(`/api/fonts?canvas=${canvas.id}`, { actor: owner, method: 'POST', bytes: fontBytes,
    headers: { 'Content-Type': 'application/octet-stream', 'X-File-Name': 'nunito-sans-bold-italic.woff2' } });
  canonicalPath(font.url);
  const frames = [];
  for (let index = 0; index < 2; index++) {
    frames.push(await json(`/api/canvases/${canvas.id}/frames`, { actor: owner, method: 'POST', json: {
      name: `Private page ${index + 1}`, x: index * 400, y: 0, width: index ? 200 : 320, height: index ? 120 : 240,
      html: `<style>html,body{margin:0;background:white}.type{position:absolute;left:80px;top:0;color:black;font:italic 700 24px ${JSON.stringify(font.family)},serif}</style><img ${index ? '' : 'data-private-image'} width="48" height="48" src="${urls[index]}"><span class="type">Private WWW</span>`,
    } }));
  }
  const files = [[canonicalPath(urls[0]), images[0]], [canonicalPath(font.url), fontBytes]];
  await check('private image byte and metadata isolation', async () => {
    for (const [path, expected] of files) {
      const own = await raw(path, { actor: human });
      assert.equal(own.status, 200); assert.deepEqual(own.bytes, expected);
      assert.ok(own.response.headers.get('cache-control')?.includes('no-store'));
      const head = await raw(path, { actor: human, method: 'HEAD' });
      assert.equal(head.status, 200); assert.equal(head.bytes.length, 0);
      const variants = [{}, { method: 'HEAD' }, { download: true }, { headers: { Range: 'bytes=0-7' } },
        { headers: { 'If-None-Match': own.response.headers.get('etag') ?? '"known-private-file"' } }];
      for (const variant of variants) {
        const requested = path + (variant.download ? '?download' : '');
        for (const authority of [{}, { actor: foreign }, { token: owner.token }])
          denied(await raw(requested, { ...variant, ...authority }));
      }
    }
  });
  await check('alternate loopback backend cannot bypass private image authorization or warm caches', async () => {
    const current = JSON.parse(await readFile(join(runtime, 'backend.json'), 'utf8'));
    assert.deepEqual(Object.keys(current).sort(), ['generation', 'origin', 'version']);
    assert.equal(current.version, 1); assert.ok(Number.isInteger(current.generation) && current.generation >= 0);
    const backend = new URL(current.origin);
    assert.equal(backend.protocol, 'http:'); assert.equal(backend.hostname, '127.0.0.1');
    assert.ok(!backend.username && !backend.password && backend.pathname === '/' && !backend.search && !backend.hash);
    const path = canonicalPath(urls[0]);
    const request = async (authority) => {
      try { return await raw(path, { ...authority, base: backend.origin, redirect: 'manual' }); }
      catch (error) {
        await capture('backend-connection', { backend: backend.origin, error: error.name, code: error.cause?.code ?? null });
        throw error;
      }
    };
    const own = await request({ actor: human });
    const redirected = [301, 302, 307, 308].includes(own.status);
    const canonicalRedirect = (result) => {
      assert.equal(result.status, own.status);
      assert.equal(result.response.headers.get('location'), origin + path);
      assert.ok(result.bytes.length <= 4096 && !result.bytes.includes(images[0]));
    };
    if (redirected) canonicalRedirect(own);
    else {
      assert.equal(own.status, 200); assert.deepEqual(own.bytes, images[0]);
      assert.ok(own.response.headers.get('cache-control')?.includes('no-store'));
    }
    for (const authority of [{}, { actor: foreign }, { token: owner.token }]) {
      const result = await request(authority);
      if (redirected) { canonicalRedirect(result); denied(await raw(path, authority)); }
      else denied(result);
    }
    await capture('backend-policy', { backend: backend.origin, generation: current.generation,
      redirectsToCanonicalOrigin: redirected, status: own.status });
  });
  await check('actual HTTPS held upload denies publication after its parent deletion completes', async () => {
    const parent = await json('/api/canvases', { actor: owner, method: 'POST',
      json: { name: 'Held upload deletion specimen', workspaceId: owner.workspace } });
    assert.ok(++requests <= 96);
    let upload;
    let complete;
    const response = new Promise((resolve, reject) => {
      upload = httpsRequest(`${origin}/api/canvases/${parent.id}/assets`, {
        method: 'POST', headers: { Origin: origin, Cookie: human.cookie, 'Content-Type': 'image/png', 'Content-Length': images[0].length },
        signal: AbortSignal.timeout(10000),
      }, (incoming) => {
        const chunks = []; let size = 0;
        incoming.on('error', reject);
        incoming.on('data', (bytes) => {
          size += bytes.length;
          if (size > 4096) { incoming.destroy(); reject(new Error('Held upload denial exceeds its fixed bound')); }
          else chunks.push(bytes);
        });
        incoming.on('end', () => resolve({ status: incoming.statusCode, bytes: Buffer.concat(chunks) }));
      });
      upload.on('error', reject);
      complete = () => upload.end(images[0].subarray(8));
    });
    response.catch(() => {}); // Observe an early socket failure while deletion is awaited.
    // The server receives headers and an incomplete body. Deletion must finish
    // before the remaining bytes can reach the normal upload publication path.
    // This proves the late-publication point, not cancellation of earlier reads.
    try {
      upload.flushHeaders(); upload.write(images[0].subarray(0, 8));
      const removed = await raw(`/api/canvases/${parent.id}`, { actor: human, method: 'DELETE' });
      assert.equal(removed.status, 200);
      complete();
      const refused = await response;
      assert.equal(refused.status, 404);
      assert.ok(!refused.bytes.includes(Buffer.from('89504e470d0a1a0a', 'hex')));
    } finally {
      upload.destroy(); await response.catch(() => {});
    }
  });
  await check('actual image materialization binds a draft to its current parent and preserves canonical source', async () => {
    const path = `/api/canvases/${canvas.id}/private-materialization`;
    const context = { kind: 'frame', id: frames[0].id };
    const prepared = await json(path, { actor: human, method: 'POST', json: { source: frames[0].html, syntax: 'html', context } });
    assert.equal(prepared.privateMedia, true);
    assert.ok(prepared.html.includes(images[0].toString('base64')) && prepared.html.includes(fontBytes.toString('base64')));
    for (const secret of secrets) assert.ok(!prepared.html.includes(secret));
    const other = await json('/api/canvases', { actor: owner, method: 'POST',
      json: { name: 'Other authorized private parent', workspaceId: owner.workspace } });
    const uploaded = await json(`/api/canvases/${other.id}/assets`, { actor: owner, method: 'POST', bytes: images[1],
      headers: { 'Content-Type': 'image/png' } });
    assert.equal((await raw(canonicalPath(uploaded.url), { actor: human })).status, 200);
    for (const source of [`<img src="${uploaded.url}">`, `<style>body{background:url('${uploaded.url}')}</style>`,
      `<svg><image href="${uploaded.url}"/></svg>`]) {
      const refused = await raw(path, { actor: human, method: 'POST', json: { source, syntax: 'html', context } });
      assert.equal(refused.status, 404); assert.equal(refused.bytes.length, 0);
    }
    const current = await call(owner, 'get_frame', { frame_id: frames[0].id });
    const saved = JSON.parse(current.content.find((item) => item.type === 'text').text);
    assert.ok(saved.html.includes(urls[0]) && saved.html.includes(font.url));
    assert.ok(!saved.html.includes('doop-resource=') && !saved.html.includes('data:image/'));
  });
  await check('actual image screenshot and multipage PDF contain private pixels and the real embedded font', async () => {
    const screenshot = await raw(`/i/${frames[0].id}.png`, { actor: human });
    assert.equal(screenshot.status, 200);
    const decoded = await sharp(screenshot.bytes).removeAlpha().raw().toBuffer({ resolveWithObject: true });
    const offset = (24 * decoded.info.width + 24) * decoded.info.channels;
    assert.deepEqual([...decoded.data.subarray(offset, offset + 3)], colors[0]);
    const path = `/api/canvases/${canvas.id}/export.pdf?frames=${frames.map((frame) => frame.id).join(',')}`;
    const response = await raw(path, { actor: human }); assert.equal(response.status, 200);
    const pdf = await PDFDocument.load(response.bytes);
    assert.deepEqual(pdf.getPages().map((page) => [page.getWidth(), page.getHeight()]), [[240, 180], [150, 90]]);
    assert.ok(pdf.context.enumerateIndirectObjects().some(([, value]) => value instanceof PDFDict &&
      /Nunito/i.test(String(value.get(PDFName.of('FontName')))) &&
      (value.has(PDFName.of('FontFile2')) || value.has(PDFName.of('FontFile3')))));
    const rendered = mupdf.Document.openDocument(response.bytes, 'application/pdf');
    try {
      assert.equal(rendered.countPages(), 2);
      for (let index = 0; index < 2; index++) {
        const page = rendered.loadPage(index);
        try {
          const pixmap = page.toPixmap([4 / 3, 0, 0, 4 / 3, 0, 0], mupdf.ColorSpace.DeviceRGB, false);
          try {
            const pixels = pixmap.getPixels(); const stride = pixmap.getStride(); const components = pixmap.getNumberOfComponents();
            const at = 24 * stride + 24 * components;
            assert.deepEqual([...pixels.subarray(at, at + 3)], colors[index]);
            let ink = 0;
            for (let y = 0; y < 40; y++) for (let x = 80; x < 190; x++) {
              const at = y * stride + x * components;
              if (pixels[at] < 160 && pixels[at + 1] < 160 && pixels[at + 2] < 160) ink++;
            }
            assert.ok(ink > 100, 'private PDF glyphs are not painted');
          } finally { pixmap.destroy(); }
        } finally { page.destroy(); }
      }
    } finally { rendered.destroy(); }
    // API error JSON can have an ETag for that generic error body. It must be
    // indistinguishable from an absent parent, never metadata for the private PDF.
    for (const authority of [{}, { actor: foreign }]) {
      const refused = await raw(path, authority);
      const absent = await raw('/api/canvases/absent0000/export.pdf?frames=absent0000', authority);
      assert.ok([401, 404].includes(refused.status));
      assert.equal(refused.status, absent.status); assert.deepEqual(refused.bytes, absent.bytes);
      assert.ok(refused.bytes.length <= 4096 && !refused.bytes.includes(Buffer.from('%PDF-')));
      for (const name of ['etag', 'last-modified', 'content-length', 'content-disposition'])
        assert.equal(refused.response.headers.get(name), absent.response.headers.get(name));
      assert.ok(refused.response.headers.get('cache-control')?.includes('no-store'));
    }
    await writeFile(join(output, 'private-visual.pdf'), response.bytes, { mode: 0o600 });
  });
  await check('actual MCP dense previews and full downloads retain private current-session authority', async () => {
    const calls = {};
    const pixels = Buffer.alloc(320 * 240 * 3);
    let seed = 0x12345678;
    for (let at = 0; at < pixels.length; at++) {
      seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
      pixels[at] = seed & 0xff;
    }
    const denseBytes = await sharp(pixels, { raw: { width: 320, height: 240, channels: 3 } }).png().toBuffer();
    const uploaded = await json(`/api/canvases/${canvas.id}/assets`, { actor: owner, method: 'POST', bytes: denseBytes,
      headers: { 'Content-Type': 'image/png' } });
    canonicalPath(uploaded.url);
    const dense = await json(`/api/canvases/${canvas.id}/frames`, { actor: owner, method: 'POST', json: {
      name: 'Dense private export', x: 0, y: 400, width: 320, height: 240,
      html: `<style>html,body{margin:0}</style><img width="320" height="240" src="${uploaded.url}">`,
    } });
    const download = (value, pathname, search) => {
      const url = new URL(value);
      assert.equal(url.origin, origin);
      assert.ok(!url.username && !url.password && !url.hash);
      assert.equal(url.pathname, pathname); assert.equal(url.search, search);
      return pathname + search;
    };
    const metadata = (result) => {
      assert.notEqual(result.isError, true);
      assert.ok(Buffer.byteLength(JSON.stringify(result)) <= 65536, 'MCP receipt exceeds the existing workflow limit');
      const bounded = (value) => {
        if (typeof value === 'string') assert.ok(Buffer.byteLength(value) <= 16384, 'MCP field exceeds the existing workflow limit');
        else if (value && typeof value === 'object') for (const entry of Object.values(value)) bounded(entry);
      };
      bounded(result);
      return result.content.filter((block) => block.type === 'text').map((block) => {
        try { return JSON.parse(block.text); } catch { return null; }
      }).find((value) => typeof value?.download_url === 'string');
    };
    for (const format of ['screenshot', 'png', 'jpg']) {
      const tool = format === 'screenshot' ? 'get_frame_screenshot' : 'export_frame';
      const args = { frame_id: dense.id, ...(format === 'screenshot' ? {} : { format }) };
      calls[format] = { tool, arguments: args };
      const result = await call(owner, tool, args);
      const info = metadata(result); assert.ok(info);
      const image = result.content.find((block) => block.type === 'image'); assert.ok(image);
      const bytes = Buffer.from(image.data, 'base64'); assert.ok(bytes.length > 0 && bytes.length <= 12288);
      const actual = await sharp(bytes).metadata();
      assert.equal(image.mimeType, 'image/jpeg'); assert.equal(info.inline.mime_type, image.mimeType);
      assert.equal(info.inline.full_resolution, false);
      assert.equal(actual.width, info.inline.width); assert.equal(actual.height, info.inline.height);
      assert.ok((await sharp(bytes).stats()).channels.some((channel) => channel.stdev > 1));
      const scale = format === 'screenshot' ? 1 : 2;
      assert.equal(info.width, 320 * scale); assert.equal(info.height, 240 * scale);
      const ext = format === 'jpg' ? 'jpg' : 'png';
      const path = download(info.download_url, `/i/${dense.id}.${ext}`, `?scale=${scale}${ext === 'jpg' ? '&quality=90' : ''}&download`);
      const full = await raw(path, { actor: human }); assert.equal(full.status, 200);
      const dimensions = await sharp(full.bytes).metadata();
      assert.equal(dimensions.width, info.width); assert.equal(dimensions.height, info.height);
      assert.equal(dimensions.format, ext === 'jpg' ? 'jpeg' : ext);
      for (const authority of [{}, { actor: foreign }, { token: owner.token }]) denied(await raw(path, authority));
      await capture(format, result);
    }
    const small = await json(`/api/canvases/${canvas.id}/frames`, { actor: owner, method: 'POST', json: {
      name: 'Small graphic PDF', x: 400, y: 400, width: 32, height: 24,
      html: '<style>html,body{margin:0;background:#0da543}</style>',
    } });
    for (const [name, selected] of [['pdf-small', [small]], ['pdf', [dense, frames[1]]]]) {
      const args = { frame_id: selected[0].id, format: 'pdf',
        ...(selected.length > 1 ? { frame_ids: selected.slice(1).map((frame) => frame.id) } : {}) };
      calls[name] = { tool: 'export_frame', arguments: args };
      const result = await call(owner, 'export_frame', args);
      const info = metadata(result); assert.ok(info);
      const embedded = name === 'pdf-small';
      assert.equal(info.inline_embedded, embedded); assert.equal(info.pages, selected.length);
      const resources = result.content.filter((block) => block.type === 'resource');
      assert.equal(resources.length, embedded ? 1 : 0);
      const path = download(info.download_url, `/api/canvases/${canvas.id}/export.pdf`, `?frames=${selected.map((frame) => frame.id).join(',')}`);
      const full = await raw(path, { actor: human }); assert.equal(full.status, 200);
      assert.equal(full.bytes.subarray(0, 5).toString(), '%PDF-');
      const pdf = await PDFDocument.load(full.bytes);
      assert.deepEqual(pdf.getPages().map((page) => [page.getWidth(), page.getHeight()]),
        selected.map((frame) => [frame.width * 0.75, frame.height * 0.75]));
      if (embedded) {
        assert.equal(resources[0].resource.mimeType, 'application/pdf');
        const bytes = Buffer.from(resources[0].resource.blob, 'base64');
        assert.ok(bytes.length > 0 && bytes.length <= 12288);
        const inlinePdf = await PDFDocument.load(bytes); assert.equal(inlinePdf.getPageCount(), 1);
      } else assert.ok(full.bytes.length > 12288);
      for (const authority of [{}, { actor: foreign }, { token: owner.token }]) {
        const refused = await raw(path, authority);
        assert.ok([401, 404].includes(refused.status));
        assert.ok(!refused.bytes.includes(Buffer.from('%PDF-')));
        assert.ok(refused.response.headers.get('cache-control')?.includes('no-store'));
      }
      await capture(name, result);
    }
    await capture('export-calls', calls);
  });
  await check('actual normal browser paints private image and font in an opaque editor frame', async () => {
    browser = await browserOwner.launch();
    const context = await browser.createBrowserContext(); page = await context.newPage();
    await page.setViewport({ width: 1280, height: 900 });
    // Import only normal signup session cookies into this isolated browser jar.
    // Neither the renderer's profile nor arbitrary requests receive these headers.
    await context.setCookie(...human.cookie.split(';').map((pair) => {
      const at = pair.indexOf('='); return { name: pair.slice(0, at).trim(), value: pair.slice(at + 1).trim(),
        domain: '127.0.0.1', path: '/', secure: true, httpOnly: true, sameSite: 'Lax' };
    }));
    const childPrivate = [];
    page.on('request', (request) => {
      if (request.frame() !== page.mainFrame() && new URL(request.url()).pathname.startsWith('/a/')) childPrivate.push(request.url());
    });
    await page.goto(`${origin}/c/${canvas.id}?frame=${frames[0].id}`, { waitUntil: 'domcontentloaded' });
    let child;
    const deadline = Date.now() + 10000;
    while (!child && Date.now() < deadline) {
      for (const candidate of page.frames()) {
        if (candidate === page.mainFrame()) continue;
        if (await candidate.evaluate(() => !!document.querySelector('img[data-private-image]')).catch(() => false)) {
          child = candidate; break;
        }
      }
      if (!child) await new Promise((resolve) => setTimeout(resolve, 50));
    }
    assert.ok(child, 'actual private editor frame was not mounted');
    displayedChild = child;
    await child.waitForFunction((family) => {
      const img = document.querySelector('img[data-private-image]');
      return img?.naturalWidth === 16 && [...document.fonts].some((font) => font.family.includes(family) && font.status === 'loaded');
    }, { timeout: 10000 }, font.family);
    assert.equal(await child.evaluate(() => { try { return document.cookie; } catch (error) { return error.name; } }), 'SecurityError');
    const element = await child.frameElement();
    assert.equal(await element.evaluate((node) => node.getAttribute('sandbox')), 'allow-scripts');
    const img = await child.$('img[data-private-image]');
    const pixels = await sharp(Buffer.from(await img.screenshot())).removeAlpha().raw().toBuffer({ resolveWithObject: true });
    await writeFile(join(output, 'private-browser.png'), await page.screenshot(), { mode: 0o600 });
    const at = (Math.floor(pixels.info.height / 2) * pixels.info.width + Math.floor(pixels.info.width / 2)) * pixels.info.channels;
    assert.deepEqual([...pixels.data.subarray(at, at + 3)], colors[0]);
    assert.equal(childPrivate.length, 0);
  });
  await check('actual browser PDF export control downloads selected private pages with embedded fonts', async () => {
    assert.ok(page, 'the actual normal editor browser is required');
    const bytes = await downloadCanvasPdf({ browser, page, origin, canvas, frames, directory: 'ui-downloads' });
    const pdf = await PDFDocument.load(bytes);
    assert.deepEqual(pdf.getPages().map((page) => [page.getWidth(), page.getHeight()]), [[240, 180], [150, 90]]);
    assert.ok(pdf.context.enumerateIndirectObjects().some(([, value]) => value instanceof PDFDict &&
      /Nunito/i.test(String(value.get(PDFName.of('FontName')))) &&
      (value.has(PDFName.of('FontFile2')) || value.has(PDFName.of('FontFile3')))));
    await writeFile(join(output, 'private-browser-export.pdf'), bytes, { mode: 0o600 });
  });
  await check('actual foreign and signed-out human browsers cannot open the private canvas', async () => {
    assert.ok(browser);
    // Every temporary human context is sequential, including the original exporting owner.
    // Reopen and paint the owner's normal session immediately before the deletion challenge.
    if (page) { await page.browserContext().close(); page = undefined; displayedChild = undefined; }
    humanProof = await humanBrowserProof({ browser, origin, foreign, ownCanvasId: actors['bot-b'].verificationCanvas,
        canvas, frameId: frames[0].id, fontFamily: font.family, record: capture,
      media: [canonicalPath(urls[0]), canonicalPath(font.url), `/i/${frames[0].id}.png`,
        `/api/canvases/${canvas.id}/export.pdf?frames=${frames.map((frame) => frame.id).join(',')}`] });
  });
  await check('an explicitly invited human opens and edits a private canvas in the normal browser', async () => {
    assert.ok(humanProof);
    await json(`/api/workspaces/${human.workspace}/members`, { actor: human, method: 'POST', json: { email: foreign.email } });
    try { await humanProof.beforeRemoval(foreign); }
    finally { await json(`/api/workspaces/${human.workspace}/members/${foreign.id}`, { actor: human, method: 'DELETE' }); }
  });
  await check('human membership removal clears the warm editor and denies browser revalidation', async () => {
    assert.ok(humanProof);
    try { await humanProof.afterRemoval(foreign); } finally { await humanProof.close(); }
  });
  if (process.env.DOOP_TEST_PRIVATE_BUCKET_ORIGIN) await check('actual private bucket origin rejects unsigned known files', async () => {
    for (const url of [urls[0], font.url]) {
      assert.ok(++requests <= 96);
      const result = await fetch(process.env.DOOP_TEST_PRIVATE_BUCKET_ORIGIN + '/' + canonicalPath(url).slice(3),
        { redirect: 'error', signal: AbortSignal.timeout(10000) });
      assert.equal(result.status, 403); await result.arrayBuffer();
    }
  });
  return {
    async afterRevocation() {
      await check('known private media and export URLs deny after membership and key revocation', async () => {
        for (const [path] of files) denied(await raw(path, { actor: owner }));
        denied(await raw(`/i/${frames[0].id}.png`, { actor: owner }));
      });
      await check('owner deletion invalidates warm private bytes and the actual open editor', async () => {
        ({ page } = await contextPage(browser, human));
        await page.goto(`${origin}/c/${canvas.id}?frame=${frames[0].id}`, { waitUntil: 'domcontentloaded' });
        // Challenge the actual currently painted frame, after the legitimate human rename.
        displayedChild = await currentPrivateFrame(page, font.family);
        const removed = await raw(`/api/canvases/${canvas.id}`, { actor: human, method: 'DELETE' });
        assert.equal(removed.status, 200);
        for (const [path] of files) denied(await raw(path, { actor: human }));
        if (page) {
          assert.ok(displayedChild, 'deletion must challenge the actual previously painted private frame');
          const deadline = Date.now() + 10000;
          let painted = true;
          while (painted && Date.now() < deadline) {
            painted = page.frames().includes(displayedChild) && await displayedChild.evaluate(
              () => !!document.querySelector('img[data-private-image]')).catch(() => false);
            if (painted) await new Promise((resolve) => setTimeout(resolve, 50));
          }
          assert.equal(painted, false, 'the actual painted private editor survived parent deletion');
        }
      });
    },
    async close() {
      try { await humanProof?.close(); } finally { await browserOwner.close(); }
    },
    get requests() { return requests; },
    capture,
  };
}
