import assert from 'node:assert/strict';

const TIMEOUT = 10000;

async function until(predicate, message) {
  const deadline = Date.now() + TIMEOUT;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  assert.fail(message);
}

export async function contextPage(browser, actor) {
  const context = await browser.createBrowserContext();
  if (actor) await context.setCookie(...actor.cookie.split(';').map((pair) => {
    const at = pair.indexOf('=');
    return { name: pair.slice(0, at).trim(), value: pair.slice(at + 1).trim(),
      domain: '127.0.0.1', path: '/', secure: true, httpOnly: true, sameSite: 'Lax' };
  }));
  const page = await context.newPage();
  await page.setViewport({ width: 1280, height: 900 });
  page.setDefaultTimeout(TIMEOUT);
  page.setDefaultNavigationTimeout(TIMEOUT);
  const closes = [];
  let initializations = 0;
  let overflow = false;
  await page.exposeFunction('__doopObservePrivateClose', (code) => {
    if (closes.length === 16) overflow = true;
    else closes.push(code);
  });
  await page.exposeFunction('__doopObservePrivateInit', () => { initializations += 1; });
  // Observe the real socket's close code across app navigations. No network or auth is mocked.
  await page.evaluateOnNewDocument(() => {
    const NativeSocket = window.WebSocket;
    window.WebSocket = class extends NativeSocket {
      constructor(...args) {
        super(...args);
        this.addEventListener('close', (event) => {
          window.__doopObservePrivateClose(event.code).catch(() => {});
        });
        this.addEventListener('message', (event) => {
          try {
            if (JSON.parse(event.data).type === 'init') window.__doopObservePrivateInit().catch(() => {});
          } catch {}
        });
      }
    };
  });
  return { context, page, closes, initialized: () => initializations > 0, overflow: () => overflow };
}

async function fetchProof(page, origin, path, headers = {}) {
  const url = new URL(path, origin);
  assert.equal(url.origin, origin);
  assert.ok(path.startsWith('/') && !path.startsWith('//'));
  return page.evaluate(async ({ path, headers }) => {
    const response = await fetch(path, { credentials: 'same-origin', cache: 'no-cache',
      redirect: 'error', headers, signal: AbortSignal.timeout(10000) });
    const reader = response.body?.getReader();
    const chunks = [];
    let size = 0;
    try {
      if (reader) for (;;) {
        const part = await reader.read();
        if (part.done) break;
        size += part.value.byteLength;
        if (size > 4 * 1024 * 1024) throw new Error('Browser proof response exceeded the existing relay byte bound');
        chunks.push(part.value);
      }
    } finally { await reader?.cancel(); }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const part of chunks) { bytes.set(part, offset); offset += part.byteLength; }
    const text = new TextDecoder().decode(bytes);
    let name;
    try { name = JSON.parse(text)?.name; } catch {}
    return { status: response.status, etag: response.headers.get('etag'),
      cache: response.headers.get('cache-control'), name,
      privatePayload: (bytes[0] === 137 && text.slice(1, 4) === 'PNG') || text.startsWith('%PDF-') ||
        text.includes('data-private-image') || text.startsWith('wOFF') || text.startsWith('wOF2') };
  }, { path, headers });
}

async function denied(page, origin, path, headers) {
  const proof = await fetchProof(page, origin, path, headers);
  assert.ok([401, 403, 404].includes(proof.status), `Browser private read returned ${proof.status}`);
  assert.equal(proof.privatePayload, false);
  if (path.startsWith('/a/') || path.startsWith('/i/') || path.includes('export.pdf')) {
    assert.ok(proof.cache?.includes('no-store'));
  }
}

async function observedDenial(state, canvasId, from) {
  await until(() => state.closes.slice(from).some((code) => [4401, 4403, 4404].includes(code)),
    'The actual canvas WebSocket did not refuse this browser');
  assert.equal(state.overflow(), false);
  await state.page.waitForFunction((id) => document.readyState === 'complete' &&
    (location.pathname !== `/c/${id}` || !!document.querySelector('input[type="password"]') ||
      document.body.textContent.includes('Canvas not found')), { timeout: TIMEOUT }, canvasId);
}

async function refusedOpening(state, origin, canvasId, from = state.closes.length) {
  await state.page.goto(`${origin}/c/${canvasId}`, { waitUntil: 'domcontentloaded' });
  await observedDenial(state, canvasId, from);
  for (const frame of state.page.frames()) {
    assert.equal(await frame.evaluate(() => !!document.querySelector('img[data-private-image]')).catch(() => false), false);
  }
  await denied(state.page, origin, `/api/canvases/${canvasId}`);
}

export async function currentPrivateFrame(page, fontFamily) {
  let child;
  await until(async () => {
    for (const frame of page.frames()) if (frame !== page.mainFrame() &&
      await frame.evaluate(() => !!document.querySelector('img[data-private-image]')).catch(() => false)) {
      child = frame; return true;
    }
    return false;
  }, 'The invited human did not open the actual private frame');
  await child.waitForFunction((family) => {
    const image = document.querySelector('img[data-private-image]');
    return image?.naturalWidth === 16 && [...document.fonts].some((font) => font.family.includes(family) && font.status === 'loaded');
  }, { timeout: TIMEOUT }, fontFamily);
  return child;
}

async function editTitle(invited, origin, canvas, record) {
  const inputs = await invited.page.$$('input');
  let title;
  for (const input of inputs) if (await input.evaluate((node, name) => node.value === name, canvas.name)) { title = input; break; }
  assert.ok(title, 'The actual editable canvas title control is missing');
  const renamed = invited.page.waitForResponse((response) =>
    new URL(response.url()).pathname === `/api/canvases/${canvas.id}` && response.request().method() === 'PATCH');
  renamed.catch(() => {});
  const expectedTitle = 'Human editing verified private draft';
  await title.click();
  await title.evaluate((node) => node.select());
  await title.type(expectedTitle);
  await title.press('Enter');
  const response = await renamed;
  const persisted = await fetchProof(invited.page, origin, `/api/canvases/${canvas.id}`);
  const proof = { version: 1, patchStatus: response.status(),
    enteredTitleMatches: JSON.parse(response.request().postData()).name === expectedTitle,
    persistedTitleMatches: persisted.name === expectedTitle, persistedTitleLength: persisted.name?.length };
  await record('human-edit-proof', proof);
  assert.equal(proof.patchStatus, 200);
  assert.equal(proof.enteredTitleMatches, true, 'Normal UI typing must send the intended title');
  assert.equal(proof.persistedTitleMatches, true, 'Normal UI edit must persist through the authenticated API');
}

export async function humanBrowserProof({ browser, origin, foreign, ownCanvasId, canvas, frameId, fontFamily, media, record }) {
  assert.ok(media.length > 0 && media.length <= 8);
  const member = await contextPage(browser, foreign);
  try {
    assert.equal(typeof ownCanvasId, 'string');
    await member.page.goto(origin, { waitUntil: 'domcontentloaded' });
    assert.equal((await fetchProof(member.page, origin, `/api/canvases/${ownCanvasId}`)).status, 200,
      'Foreign denial requires a working authenticated foreign account, not a missing cookie');
    await refusedOpening(member, origin, canvas.id);
    for (const path of media) await denied(member.page, origin, path);
  } finally { await member.context.close(); }
  // A temporary browser context owns HTTP keep-alive sockets too. Close it before the next
  // context so the existing 16-socket TLS admission limit remains effective.
  const signedOut = await contextPage(browser);
  try {
    await refusedOpening(signedOut, origin, canvas.id);
    await signedOut.page.waitForSelector('input[type="password"]', { visible: true });
    for (const path of media) await denied(signedOut.page, origin, path);
  } finally { await signedOut.context.close(); }
  const warmed = new Map();
  let invited;
  let child;
  let removalBoundary;
  return {
    async beforeRemoval(actor) {
      assert.equal(actor.id, foreign.id);
      invited = await contextPage(browser, foreign);
      await invited.page.goto(`${origin}/c/${canvas.id}?frame=${frameId}`, { waitUntil: 'domcontentloaded' });
      child = await currentPrivateFrame(invited.page, fontFamily);
      await until(invited.initialized, 'The invited browser must join the actual live canvas before removal');
      for (const path of media) {
        const proof = await fetchProof(invited.page, origin, path);
        assert.equal(proof.status, 200);
        warmed.set(path, proof.etag);
      }
      await editTitle(invited, origin, canvas, record);
      child = await currentPrivateFrame(invited.page, fontFamily);
      removalBoundary = invited.closes.length;
    },
    async afterRemoval(actor) {
      assert.equal(actor.id, foreign.id);
      try {
        assert.ok(child && Number.isInteger(removalBoundary));
        await observedDenial(invited, canvas.id, removalBoundary);
        await until(async () => !invited.page.frames().includes(child) ||
          !await child.evaluate(() => !!document.querySelector('img[data-private-image]')).catch(() => false),
        'The actual previously painted human frame survived membership removal');
        for (const [path, etag] of warmed) await denied(invited.page, origin, path,
          etag ? { 'If-None-Match': etag } : { 'If-Modified-Since': 'Wed, 01 Jan 2025 00:00:00 GMT' });
        await refusedOpening(invited, origin, canvas.id);
      } finally {
        await record('human-revocation-proof', { version: 1, initialized: invited?.initialized(),
          closeCodes: invited?.closes, removalBoundary, frameAttached: invited?.page.frames().includes(child) });
      }
      await invited.context.close();
      invited = undefined;
    },
    async close() { if (invited) { await invited.context.close(); invited = undefined; } },
  };
}
