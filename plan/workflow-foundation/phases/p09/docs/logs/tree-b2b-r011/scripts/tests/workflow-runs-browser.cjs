// p09 B2a browser pass: the runs workspace on localhost:3001 (phase database workflow09 only).
// A fresh local test owner and company; the throwaway password stays in this scratch dir.
const { chromium } = require('playwright-core');
const fs = require('fs');
const crypto = require('crypto');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const upstream = new URL(process.env.WORKFLOW_TEST_URL || 'http://localhost:3001');
const OUT = process.env.OUT || fs.mkdtempSync(path.join(os.tmpdir(), 'workflow-runs-browser-'));
let BASE;
const streams = new Set();
const proxy = http.createServer((request, response) => {
  const target = http.request(new URL(request.url, upstream), { method: request.method, headers: request.headers }, incoming => {
    response.writeHead(incoming.statusCode, incoming.headers);
    if ((incoming.headers['content-type'] || '').startsWith('text/event-stream')) {
      const stream = { incoming, response };
      streams.add(stream);
      response.on('close', () => { streams.delete(stream); incoming.destroy(); });
    }
    incoming.pipe(response);
  });
  target.on('error', () => response.destroy());
  request.pipe(target);
});
const SHOTS = OUT + '/shots';
fs.mkdirSync(SHOTS, { recursive: true });
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok: !!ok, detail: String(detail).slice(0, 400) });
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? ' — ' + String(detail).replace(/\s+/g, ' ').slice(0, 220) : ''}`);
};

(async () => {
  await new Promise(resolve => proxy.listen(0, '127.0.0.1', resolve));
  BASE = `http://127.0.0.1:${proxy.address().port}`;
  const browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({ viewport: { width: 1400, height: 950 } });
  // Every request htmx aborts, across navigations.
  const aborts = [];
  await context.exposeBinding('recordAbort', (_, entry) => aborts.push(entry));
  await context.addInitScript(() => {
    document.addEventListener('htmx:sendAbort', (e) => window.recordAbort(`${e.detail.requestConfig?.verb} ${e.detail.requestConfig?.path}`));
  });
  const page = await context.newPage();
  const errors = [];
  const disconnectErrors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', async m => {
    if (m.type() !== 'error' || m.text().startsWith('Failed to load resource')) return;
    // HTMX logs the native EventSource error object for each intentionally severed socket.
    if (m.text() === 'Event' && await m.args()[0]?.evaluate(value => value instanceof Event && value.target instanceof EventSource)) {
      disconnectErrors.push(m.text());
    } else errors.push(m.text());
  });
  page.on('response', (r) => r.status() >= 400 && errors.push(`${r.status()} ${r.url()}`));
  let answer = 'dismiss';
  const dialogs = [];
  page.on('dialog', async (d) => {
    dialogs.push(d.message());
    if (answer === 'accept') await d.accept();
    else await d.dismiss();
  });
  const settle = async () => {
    await page.waitForLoadState('domcontentloaded');
    await page.waitForTimeout(200);
    await page.waitForFunction(() => !document.querySelector('.htmx-request'), null, { timeout: 15000 }).catch(() => {});
    await page.waitForTimeout(250);
  };
  const shot = async (name) => page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: false });
  const theme = async (value) => {
    const state = await page.evaluate((v) => {
      const toggle = document.getElementById('theme-toggle');
      if (!toggle) return 'no toggle';
      toggle.checked = v === 'light';
      toggle.dispatchEvent(new Event('change', { bubbles: true }));
      return `${document.documentElement.getAttribute('data-theme')}`;
    }, value);
    await page.waitForTimeout(150);
    return state;
  };

  // A local test owner and company.
  const user = 'b2a' + crypto.randomBytes(3).toString('hex');
  const password = crypto.randomBytes(18).toString('base64url');
  fs.writeFileSync(OUT + '/password.' + user, password, { mode: 0o600 });
  await page.goto(BASE + '/register');
  await page.fill('[name=username]', user);
  await page.fill('[name=email]', user + '@example.test');
  await page.fill('[name=password]', password);
  if (await page.locator('[name=confirm_password]').count()) await page.fill('[name=confirm_password]', password);
  await Promise.all([page.waitForLoadState('networkidle'), page.locator('form [type=submit]').first().click()]);
  if (page.url().includes('/login') || page.url().endsWith('/register')) {
    await page.goto(BASE + '/login');
    await page.fill('[name=email_or_username]', user);
    await page.fill('[name=password]', password);
    await Promise.all([page.waitForLoadState('networkidle'), page.locator('form [type=submit]').first().click()]);
  }
  await page.goto(BASE + '/ui/companies');
  const created = await page.evaluate(async (slug) => {
    const body = new URLSearchParams({ name: 'Gamma Operations', slug });
    return (await fetch('/ui/companies', { method: 'POST', body, headers: { 'HX-Request': 'true' } })).status;
  }, 'gamma-' + user);
  await page.goto(BASE + '/ui/companies');
  const ids = [...new Set((await page.content()).match(/company_id=([0-9a-f-]{36})/g) || [])];
  check('setup: one company for the fresh owner', created < 400 && ids.length === 1, `${created} ${ids.length}`);
  const C = ids[0].split('=')[1];

  const api = (method, path, body) =>
    page.evaluate(async ({ method, path, body }) => {
      const r = await fetch(path, { method, headers: { 'Content-Type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
      const text = await r.text();
      let json = null;
      try { json = JSON.parse(text); } catch (_) {}
      return { status: r.status, json, text: text.slice(0, 300) };
    }, { method, path, body });
  const P = `/api/companies/${C}`;
  const publish = async (title, source) => {
    const workflow = source.workflow_id;
    const saved = await api('PUT', `${P}/workflow-definitions/${workflow}`, { expected_revision: null, title, source: JSON.stringify(source) });
    const published = await api('POST', `${P}/workflow-definitions/${workflow}/publish`, { expected_revision: saved.json?.revision, command_key: crypto.randomUUID() });
    const binding = crypto.randomUUID();
    const bound = await api('PUT', `${P}/workflow-bindings/${binding}`, { expected_state_revision: null, association: { kind: 'company' }, version_id: published.json?.version_id });
    const active = await api('POST', `${P}/workflow-bindings/${binding}/activate`, { expected_state_revision: bound.json?.state_revision });
    check(`setup: ${title} published, bound and active`, saved.status === 200 && published.status < 300 && bound.status === 200 && active.status === 200,
      `${saved.status} ${published.status} ${published.text} ${bound.status} ${bound.text} ${active.status}`);
    return binding;
  };
  const step = (type, values) => ({ type, with: Object.fromEntries(Object.entries(values).map(([k, v]) => [k, { literal: v }])) });
  const base = (steps) => ({ format_version: 1, workflow_id: crypto.randomUUID(), input_schema: true, parameter_schema: true, output_schema: true,
    resources: [], entry: 'start', steps, limits: { max_steps: 100, max_context_bytes: 65536 } });
  const schema = { type: 'string', maxLength: 128 };
  const deadline = new Date(Date.now() + 40 * 60 * 1000).toISOString().replace(/\.\d+Z$/, 'Z');
  const hold = await publish('Hold for review', base({
    start: { ...step('data.map', { value: 'checked', output_schema: schema }), routes: { success: 'hold' } },
    hold: { ...step('wait.timer', { deadline }), routes: { success: '$end' } },
  }));
  const quick = await publish('Quick map', base({ start: { ...step('data.map', { value: 'hello', output_schema: schema }), routes: { success: '$end' } } }));
  const start = (binding, input = {}) => api('POST', `${P}/workflow-bindings/${binding}/runs`, { input, command_key: crypto.randomUUID() });
  const startedRuns = [];
  for (const binding of [quick, quick, quick, hold, hold]) startedRuns.push(await start(binding, { note: '<img src=x onerror=alert(1)>' }));
  check('setup: five runs started', startedRuns.every((r) => r.status < 300), startedRuns.map((r) => r.status + ' ' + r.text).join(' | '));
  const waitingRun = startedRuns[3].json?.run_id;

  // The workspace, with the stream connected.
  const entries = () => page.evaluate(() => document.querySelectorAll('#run-list ul.menu > li > a').length);
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}`);
  await settle();
  await page.waitForTimeout(1500);
  check('workspace lists the five runs', (await entries()) === 5, await entries());
  await theme('light');
  await shot('01-workspace-light');

  // A run started elsewhere shows up without a reload (sse:runs wakes the list).
  const marker = await page.evaluate(() => (window.__b2aMarker = Math.random()));
  await start(quick);
  const live = await page.waitForFunction(() => document.querySelectorAll('#run-list ul.menu > li > a').length === 6, null, { timeout: 15000 }).then(() => true).catch(() => false);
  check('a new run appears through the stream, without a navigation', live && (await page.evaluate(() => window.__b2aMarker)) === marker, await entries());

  // Overlapping wake-ups: many changes at once; one list and one pane remain, and the last read wins.
  await page.evaluate(async ({ P, quick }) => {
    await Promise.all([...Array(6)].map(() => fetch(`${P}/workflow-bindings/${quick}/runs`, { method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ input: {}, command_key: crypto.randomUUID() }) })));
  }, { P, quick });
  const overlapped = await page.waitForFunction(() => document.querySelectorAll('#run-list ul.menu > li > a').length === 12, null, { timeout: 20000 }).then(() => true).catch(() => false);
  await page.waitForTimeout(2500);
  const regions = await page.evaluate(() => [document.querySelectorAll('#run-list').length, document.querySelectorAll('#run-pane').length,
    document.querySelectorAll('[sse-connect]').length]);
  check('overlapping wake-ups settle on the current list (12 runs), one list, one pane, one stream owner',
    overlapped && regions.join() === '1,1,1', `${await entries()} ${regions}`);
  const ids12 = await page.evaluate(() => [...document.querySelectorAll('#run-list ul.menu > li > a')].map((a) => a.getAttribute('hx-get')));
  check('no run listed twice', new Set(ids12).size === ids12.length, ids12.length);

  // Opening a run: the pane, the pushed URL, the graph, and a lazy step through the keyboard.
  await page.click(`#run-list a[hx-get^="/ui/workflow-runs/${waitingRun}"]`);
  await settle();
  check('opening a run pushes its URL', page.url().includes(`run_id=${waitingRun}`), page.url());
  const pane = await page.evaluate(() => document.getElementById('run-pane')?.innerText || '');
  check('the waiting run shows its state, controls and steps', /waiting/.test(pane) && pane.includes('Cancel run') && pane.includes('Steps'), pane.slice(0, 300));
  check('the run graph is drawn', await page.evaluate(() => !!document.querySelector('#run-pane svg')));
  const hostile = await page.evaluate(() => [document.getElementById('run-pane')?.textContent.includes('<img src=x onerror=alert(1)>'), !!document.querySelector('#run-pane img[src="x"]')]);
  check('hostile input is shown as text, never as markup', hostile[0] && !hostile[1] && dialogs.length === 0, `${hostile} ${dialogs.join()}`);
  const summary = page.locator('#run-pane details > summary').filter({ hasText: 'start' }).first();
  await summary.focus();
  await page.keyboard.press('Enter');
  await settle();
  const stepBody = await page.evaluate(() => [...document.querySelectorAll('#run-pane details[open]')].map((d) => d.innerText).join(' '));
  check('Enter on a step opens it and loads what it froze and produced', stepBody.includes('Inputs (frozen when the step started)') && stepBody.includes('checked'), stepBody.slice(0, 300));
  await shot('02-waiting-run-step-light');

  // A change made elsewhere: the body reloads in place, the finished step stays open and loaded.
  const other = startedRuns[4].json?.run_id;
  await page.click(`#run-list a[hx-get^="/ui/workflow-runs/${other}"]`);
  await settle();
  await page.locator('#run-pane details > summary').filter({ hasText: 'start' }).first().click();
  await settle();
  const otherRevision = (await api('GET', `${P}/workflow-runs/${other}`)).json?.revision;
  const elsewhere = await api('POST', `${P}/workflow-runs/${other}/cancel`, { expected_revision: otherRevision, command_key: crypto.randomUUID() });
  const reloaded = await page.waitForFunction(() => /cancelled/.test(document.querySelector('#run-body h2')?.parentElement?.innerText || ''), null, { timeout: 15000 }).then(() => true).catch(() => false);
  const kept = await page.evaluate(() => [...document.querySelectorAll('#run-pane details[open]')].map((d) => d.innerText).join(' '));
  check('a change elsewhere reloads the body; the open finished step stays open and loaded',
    elsewhere.status === 200 && reloaded && kept.includes('Inputs (frozen when the step started)') && page.url().includes(`run_id=${other}`), `${elsewhere.status} ${reloaded} ${kept.slice(0, 120)}`);
  await page.click(`#run-list a[hx-get^="/ui/workflow-runs/${waitingRun}"]`);
  await settle();

  // Cancel through its confirm: the run's own answer, then rerun to a new run.
  answer = 'accept';
  await page.click('#run-pane button:has-text("Cancel run")');
  await settle();
  const cancelled = await page.evaluate(() => document.getElementById('run-pane')?.innerText || '');
  check('cancel asks first, naming what stays', dialogs.some((d) => d.startsWith('Cancel this run?') && d.includes('No effect of this run has been accepted yet.')), dialogs.join(' | '));
  check('cancel answers on the pane and the run is cancelled', cancelled.includes('Cancelled (revision') && /cancelled/.test(cancelled) && cancelled.includes('Rerun'), cancelled.slice(0, 300));
  await page.waitForTimeout(2500);
  const stays = await page.evaluate(() => document.getElementById('run-pane')?.innerText || '');
  check("the answer outlasts the run's own wake-up", stays.includes('Cancelled (revision') && /cancelled/.test(stays), stays.slice(0, 200));
  const stopped = await page.evaluate(() => [document.getElementById('run-pane')?.textContent.includes('stopped, run cancelled'),
    [...document.querySelectorAll('#run-pane svg [style*="dasharray"]')].length]);
  check('the parked step of the cancelled run reads as stopped, in the list and on the graph', stopped[0] && stopped[1] === 1, JSON.stringify(stopped));
  await shot('03-cancelled-light');
  await page.click('#run-pane button:has-text("Rerun")');
  await settle();
  const rerun = await page.evaluate(() => document.getElementById('run-pane')?.innerText || '');
  const rerunUrl = page.url();
  check('rerun moves the pane and the URL to the new run', /Started run \w+ as a rerun of/.test(rerun) && rerunUrl.includes('run_id=') && !rerunUrl.includes(waitingRun), `${rerun.slice(0, 200)} ${rerunUrl}`);
  const labelled = await page.waitForFunction(() => document.getElementById('run-list')?.innerText.includes('rerun of'), null, { timeout: 15000 }).then(() => true).catch(() => false);
  check('the list labels the rerun after its wake-up', labelled);
  await page.goBack();
  await settle();
  check('Back returns to the cancelled run', page.url().includes(`run_id=${waitingRun}`) && (await page.evaluate(() => document.getElementById('run-pane')?.innerText || '')).includes('Rerun'), page.url());
  answer = 'dismiss';

  // A deep link renders the run on the server.
  await page.goto(rerunUrl);
  await settle();
  check('a deep link opens that run', (await page.evaluate(() => document.getElementById('run-pane')?.innerText || '')).includes('Cancel run'));

  // The state filter, then Load more past 50 runs.
  await page.selectOption('#run-list select[name=state]', 'succeeded');
  await settle();
  const states = await page.evaluate(() => [...document.querySelectorAll('#run-list ul.menu > li > a')].map((a) => a.innerText));
  check('the state filter keeps only succeeded runs', states.length >= 9 && states.every((t) => t.includes('succeeded')), `${states.length}`);
  await page.selectOption('#run-list select[name=state]', '');
  await settle();
  await page.evaluate(async ({ P, quick }) => {
    for (let i = 0; i < 45; i++) {
      await fetch(`${P}/workflow-bindings/${quick}/runs`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ input: { i }, command_key: crypto.randomUUID() }) });
    }
  }, { P, quick });
  await page.waitForTimeout(4000);
  await settle();
  const before = await entries();
  const more = page.locator('#run-list-more button');
  check('a full first page offers Load more', before === 50 && (await more.count()) === 1, before);
  await more.click();
  await settle();
  const after = await page.evaluate(() => [...document.querySelectorAll('#run-list ul.menu > li > a')].map((a) => a.getAttribute('hx-get')));
  check('Load more appends the rest without repeats', after.length > 50 && new Set(after).size === after.length, after.length);

  // F025: a wake-up keeps what Load more added and where the list was scrolled.
  await page.evaluate(() => { const list = document.getElementById('run-list-entries'); list.scrollTop = list.scrollHeight; });
  const scrolled = await page.evaluate(() => document.getElementById('run-list-entries').scrollTop);
  await start(quick);
  await page.waitForTimeout(3000);
  await settle();
  const keptList = await page.evaluate(() => [document.querySelectorAll('#run-list ul.menu > li > a').length, document.getElementById('run-list-entries').scrollTop]);
  check('a wake-up after Load more keeps the rows read and the scroll position', keptList[0] >= after.length && scrolled > 0 && keptList[1] > 0, `${after.length} -> ${keptList[0]}, scroll ${scrolled} -> ${keptList[1]}`);
  // F025: opening another run moves the highlight there, and only there.
  const twoLinks = await page.evaluate(() => [...document.querySelectorAll('#run-list ul.menu > li > a')].slice(0, 2).map((a) => a.getAttribute('hx-get')));
  for (const href of twoLinks) {
    await page.click(`#run-list a[hx-get="${href}"]`);
    await settle();
    await page.waitForTimeout(800);
  }
  const active = await page.evaluate(() => [...document.querySelectorAll('#run-list a.menu-active')].map((a) => a.getAttribute('hx-get')));
  check('the list highlights the run open in the pane, and only it', active.length === 1 && active[0] === twoLinks[1], JSON.stringify(active));

  // F023: command keys survive a reconnect, and a lost answer is safe to repeat.
  const finished = (await api('GET', `${P}/workflow-runs?binding_id=${quick}&limit=5`)).json;
  const finishedRuns = (finished?.runs || finished?.items || []).filter((r) => r.state === 'succeeded').map((r) => r.run_id);
  const reconnect = async () => {
    await page.evaluate(() => { const body = document.getElementById('run-body'); if (body) body.dataset.before = '1'; });
    // Break the actual HTTP stream: Playwright's offline mode can leave an established SSE
    // socket alive, which does not exercise reconnect at all.
    if (!streams.size) throw new Error('No SSE socket to disconnect');
    for (const stream of [...streams]) { stream.incoming.destroy(); stream.response.destroy(); }
    return page.waitForFunction(() => { const body = document.getElementById('run-body'); return body && !body.dataset.before; }, null, { timeout: 30000 }).then(() => true).catch(() => false);
  };
  const keyOf = () => page.evaluate(() => document.querySelector('#run-body form[hx-post*="/rerun?"] input[name=command_key]')?.value);
  const rerunCount = async (run) => ((await api('GET', `${P}/workflow-runs?limit=200`)).json?.runs || (await api('GET', `${P}/workflow-runs?limit=200`)).json?.items || []).filter((r) => r.rerun_of === run || r.rerun_of_run_id === run).length;
  check('setup: three finished runs to rerun', finishedRuns.length >= 3, JSON.stringify(finished).slice(0, 200));
  const [keep, lost, lostThenReconnect] = finishedRuns;
  // A reconnect alone redraws the body with the same keys.
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}&run_id=${keep}`);
  await settle();
  const keyBefore = await keyOf();
  const reconnected = await reconnect();
  const keyAfter = await keyOf();
  check('a reconnect redraws the body (resync) with the same command key', reconnected && keyBefore && keyBefore === keyAfter, `${reconnected} ${keyBefore} ${keyAfter}`);
  // An answer lost after the server committed: the same form again replays it.
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}&run_id=${lost}`);
  await settle();
  await page.route('**/rerun?**', async (route) => { await route.fetch(); await route.abort('connectionreset'); });
  await page.click('#run-pane button:has-text("Rerun")');
  await page.waitForTimeout(1500);
  await page.unroute('**/rerun?**');
  check('a lost answer leaves the page as it was', page.url().includes(`run_id=${lost}`) && (await page.evaluate(() => document.getElementById('run-pane').innerText)).includes('Rerun'));
  await page.click('#run-pane button:has-text("Rerun")');
  await settle();
  const replayed = await page.evaluate(() => document.getElementById('run-pane').innerText);
  check('pressing it again replays the committed rerun instead of starting another', /Already started run \w+ as a rerun of/.test(replayed) && (await rerunCount(lost)) === 1, `${replayed.slice(0, 120)} ${await rerunCount(lost)}`);
  // An answer lost, then the connection drops and comes back: the redrawn page shows the rerun.
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}&run_id=${lostThenReconnect}`);
  await settle();
  await page.route('**/rerun?**', async (route) => { await route.fetch(); await route.abort('connectionreset'); });
  await page.click('#run-pane button:has-text("Rerun")');
  await page.waitForTimeout(1500);
  await page.unroute('**/rerun?**');
  const redrawn = await reconnect();
  const shownAfter = await page.evaluate(() => document.getElementById('run-pane').innerText);
  check('after a lost answer and reconnect the page shows the committed rerun and an explicit separate-intent link', redrawn && shownAfter.includes('Rerun as') && shownAfter.includes('Prepare another rerun') && (await rerunCount(lostThenReconnect)) === 1, `${redrawn} ${shownAfter.slice(0, 200)}`);

  await page.click('#run-pane button:has-text("Rerun")');
  await settle();
  check('a replay after reconnect still opens the original rerun', (await rerunCount(lostThenReconnect)) === 1);
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}&run_id=${lostThenReconnect}`);
  await settle();
  await page.click('#run-pane a:has-text("Prepare another rerun")');
  await settle();
  const secondKey = await keyOf();
  await page.route('**/rerun?**', async route => { await route.fetch(); await route.abort('connectionreset'); });
  await page.click('#run-pane button:has-text("Rerun")');
  await page.waitForTimeout(1500);
  await page.unroute('**/rerun?**');
  const secondReconnect = await reconnect();
  check('a lost second rerun and reconnect retain its key', secondReconnect && secondKey === await keyOf());
  await page.click('#run-pane button:has-text("Rerun")');
  await settle();
  check('repeating the second rerun after reconnect creates no third run', (await rerunCount(lostThenReconnect)) === 2);

  // F022: on a waiting run, the open unfinished step stays open across a resync.
  const waitingNow = startedRuns[3].json?.run_id ? (await api('POST', `${P}/workflow-bindings/${hold}/runs`, { input: {}, command_key: crypto.randomUUID() })).json?.run_id : null;
  await page.waitForTimeout(3000);
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}&run_id=${waitingNow}`);
  await settle();
  check('a waiting run takes no tick', !(await page.content()).includes('sse:tick'));
  await page.locator('#run-pane details > summary').filter({ hasText: 'hold' }).first().click();
  await settle();
  const resynced = await reconnect();
  const stillOpen = await page.evaluate(() => [...document.querySelectorAll('#run-pane details[open]')].some((d) => d.innerText.includes('hold') && d.innerText.includes('Inputs (frozen')));
  check('an open unfinished step stays open across a resync while the run has not moved', resynced && stillOpen, `${resynced} ${stillOpen}`);

  // Keyboard: Tab reaches a run link with a visible focus ring, and Enter opens it.
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}`);
  await settle();
  let reached = null;
  for (let i = 0; i < 80 && !reached; i++) {
    await page.keyboard.press('Tab');
    reached = await page.evaluate(() => {
      const el = document.activeElement;
      if (!el || !el.closest('#run-list ul.menu') || el.tagName !== 'A') return null;
      const s = getComputedStyle(el);
      return { outline: `${s.outlineStyle} ${s.outlineWidth}`, shadow: s.boxShadow, href: el.getAttribute('hx-get') };
    });
  }
  check('Tab reaches a run link with a visible focus indicator', reached && (reached.outline.split(' ')[0] !== 'none' || reached.shadow !== 'none'), JSON.stringify(reached));
  await page.keyboard.press('Enter');
  await settle();
  check('Enter on a run link opens its pane', !!reached && (await page.evaluate(() => !!document.querySelector('#run-pane h2, #run-pane h3'))) && page.url().includes('run_id='), page.url());

  // Both themes: contrast of the workspace with a run and one step open.
  const contrast = () =>
    page.evaluate(() => {
      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 1;
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      const rgb = (s) => { ctx.clearRect(0, 0, 1, 1); ctx.fillStyle = '#000'; ctx.fillStyle = s; ctx.fillRect(0, 0, 1, 1); const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data; return [r, g, b, a / 255]; };
      const lum = ([r, g, b]) => { const f = (c) => ((c /= 255) <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b); };
      const background = (el) => { while (el) { const v = rgb(getComputedStyle(el).backgroundColor); if (v[3] > 0.5) return v; el = el.parentElement; } return [255, 255, 255]; };
      const worst = [];
      for (const el of document.querySelectorAll('main *')) {
        if (![...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim())) continue;
        const style = getComputedStyle(el);
        if (style.visibility === 'hidden' || style.display === 'none' || el.getClientRects().length === 0) continue;
        const fg = rgb(style.color);
        let alpha = fg[3];
        for (let node = el; node; node = node.parentElement) alpha *= Number(getComputedStyle(node).opacity);
        if (alpha < 0.05) continue;
        const bg = background(el);
        const mixed = fg.slice(0, 3).map((c, i) => c * alpha + bg[i] * (1 - alpha));
        const [a, b] = [lum(mixed), lum(bg)].sort((x, y) => y - x);
        worst.push({ ratio: Math.round(((a + 0.05) / (b + 0.05)) * 100) / 100, text: el.textContent.trim().slice(0, 40), cls: el.className.toString().slice(0, 60) });
      }
      worst.sort((x, y) => x.ratio - y.ratio);
      return worst.slice(0, 5);
    });
  for (const mode of ['light', 'dark']) {
    await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}&run_id=${waitingRun}`);
    await settle();
    const applied = await theme(mode);
    await page.locator('#run-pane details > summary').filter({ hasText: 'start' }).first().click();
    await settle();
    await page.waitForTimeout(300);
    await shot(`theme-${mode}-run`);
    const low = await contrast();
    check(`${mode}: theme applied and no text under 3:1`, applied === mode && low.every((entry) => entry.ratio >= 3), `${applied} ${JSON.stringify(low.slice(0, 3))}`);
  }

  // The bindings tab's "runs" link filters the workspace by binding.
  await page.goto(`${BASE}/ui/workflow-runs?company_id=${C}&binding_id=${hold}`);
  await settle();
  const filtered = await page.evaluate(() => [document.getElementById('run-list')?.innerText || '', document.querySelectorAll('#run-list ul.menu > li > a').length]);
  check('a binding filter shows only that binding and a way back', filtered[0].includes('only') && filtered[0].includes('all runs') && filtered[1] === (await api('GET', `${P}/workflow-runs?binding_id=${hold}&limit=200`)).json.items.length, `${filtered[1]}`);

  check(`every request htmx aborted (${aborts.length}) was a superseded read (GET), never a write`, aborts.every((a) => a.startsWith('GET ') || a.startsWith('get ')), aborts.join(' | '));
  check('the four severed SSE sockets report native EventSource errors', disconnectErrors.length === 4);
  check('all three intentionally lost answers report sendError', errors.filter(e => e === 'htmx:sendError').length === 3);
  const unexpected = errors.filter((e) => !/^htmx:(sendAbort|afterRequest|sendError|sseError)$/.test(e));
  check('no unexpected page errors', unexpected.length === 0, unexpected.join(' | '));
  fs.writeFileSync(OUT + '/results.json', JSON.stringify(results, null, 2));
  console.log(`${results.filter((r) => r.ok).length}/${results.length} pass`);
  await browser.close();
  proxy.close();
  if (results.some(result => !result.ok)) process.exitCode = 1;
})().catch((e) => {
  console.log('SCRIPT ERROR', e.message);
  fs.writeFileSync(OUT + '/results.json', JSON.stringify(results, null, 2));
  process.exit(1);
});
