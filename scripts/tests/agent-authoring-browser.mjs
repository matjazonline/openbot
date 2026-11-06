// Actual Rust authoring markup/responses, shipped HTMX, production CSP and a real browser.
// Native route tests independently exercise the submitted commands and their database effects.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const directory = process.env.AGENT_AUTHORING_BROWSER_FIXTURES;
assert.ok(directory, 'Run dump_autonomous_agent_authoring_browser_fixtures against the owned test database');
const fixture = name => readFileSync(`${directory}/${name}`);
const metadata = JSON.parse(fixture('metadata.json'));
const late = JSON.parse(fixture('late-save.json'));
const root = new URL('../../', import.meta.url);
const csp = readFileSync(new URL('src/adapters/http/security.rs', root), 'utf8').match(/const CSP: &str = "([^"]+)";/)[1];

test('agent selectors preserve unsaved input, publication retries and navigation after an accepted write', async () => {
  const queries = [];
  const writes = [];
  const waiting = new Set();
  let releaseCatalog;
  let holdCatalog = true;
  let releaseWrite;
  const html = (response, file, headers = {}) => response.writeHead(200, {
    'content-type': 'text/html', 'content-security-policy': csp, ...headers,
  }).end(fixture(file));
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (url.pathname === '/assets/app.js' || url.pathname === '/assets/theme-init.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(fixture(url.pathname.split('/').at(-1))); return;
    }
    if (url.pathname.startsWith('/assets/')) {
      try {
        response.writeHead(200, { 'content-type': url.pathname.endsWith('.css') ? 'text/css' : 'text/javascript' })
          .end(readFileSync(new URL(url.pathname.slice(1), root)));
      } catch { response.writeHead(204).end(); }
      return;
    }
    if (request.method === 'PUT' || request.method === 'POST') {
      let body = ''; for await (const chunk of request) body += chunk;
      writes.push({ path: url.pathname, body: new URLSearchParams(body) });
      if (url.pathname.endsWith('/publish')) { html(response, 'publication.html'); return; }
      const saves = writes.filter(item => !item.path.endsWith('/publish'));
      if (saves.length === 1) { html(response, 'stale-pane.html'); return; }
      waiting.add(response);
      await new Promise(resolve => { releaseWrite = resolve; });
      waiting.delete(response);
      response.writeHead(200, { 'content-type': 'text/html', ...Object.fromEntries(late.headers) }).end(late.body);
      return;
    }
    if (url.pathname === `/ui/agents/${metadata.other}/tool-catalog`) { response.writeHead(204).end(); return; }
    if (url.pathname.endsWith('/tool-catalog')) {
      queries.push(url);
      const kind = url.searchParams.get('kind') || 'builtin';
      if (kind === 'http' && holdCatalog) {
        holdCatalog = false;
        waiting.add(response);
        await new Promise(resolve => { releaseCatalog = resolve; });
        waiting.delete(response);
      }
      html(response, kind === 'http' ? (url.searchParams.get('connection') ? 'http-selected.html' : 'http.html') : `${kind}.html`);
      return;
    }
    if (url.pathname === `/ui/agents/${metadata.other}`) { html(response, 'other-pane.html'); return; }
    if (url.pathname === `/ui/agents/${metadata.agent}`) { html(response, 'fresh-pane.html'); return; }
    if (url.pathname === '/ui/agents') { html(response, 'page.html'); return; }
    response.writeHead(204).end();
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  let page;
  const errors = [];
  try {
    browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    page = await browser.newPage({ viewport: { width: 1400, height: 1100 } });
    page.setDefaultTimeout(10000);
    page.on('pageerror', error => errors.push(error.message));
    const base = `http://127.0.0.1:${server.address().port}`;
    await page.goto(`${base}/ui/agents?company_id=${metadata.company}&agent_id=${metadata.agent}`);
    await expect(page.locator('#agent-pane img[src=x]')).toHaveCount(0);
    const editor = () => page.locator('#agent-pane > div form[hx-put]').first();
    const catalog = () => page.locator(`#agent-tool-catalog-${metadata.agent}`);
    const ready = () => expect(page.locator('#agent-pane.htmx-added, #agent-pane.htmx-settling, #agent-pane .htmx-added, #agent-pane .htmx-settling')).toHaveCount(0);
    await page.evaluate(() => document.addEventListener('htmx:beforeRequest', event => {
      const form = event.detail.elt;
      if (form.matches('form[hx-put^="/ui/agents/"]')) window.pendingAgentEditor = form;
    }, true));
    await expect(catalog().locator('[name=kind]')).toHaveValue('builtin');
    await ready();
    await editor().locator('[name=name]').fill('Draft <browser>');
    await editor().locator('[name=system_prompt]').fill('browser unsaved prompt marker');
    await catalog().locator('[name=kind]').selectOption('http');
    await expect.poll(() => typeof releaseCatalog).toBe('function');
    await catalog().locator('[name=kind]').selectOption('workflow');
    await expect(catalog().locator('[name=kind] option[value=workflow]')).toHaveAttribute('selected', '');
    await ready();
    releaseCatalog();
    await expect(catalog().locator('[name=connection]')).toHaveCount(0);
    await expect(editor().locator('[name=name]')).toHaveValue('Draft <browser>');
    await catalog().locator('[name=kind]').selectOption('http');
    await expect(catalog().locator('[name=connection]')).toBeVisible();
    await ready();
    await catalog().locator('[name=connection]').selectOption(metadata.connection);
    await expect(catalog().locator('[data-agent-tool-add]')).toBeVisible();
    await ready();
    await catalog().locator('[data-agent-tool-add]').click();
    await expect(editor().locator('[data-agent-tool-row]')).toContainText(metadata.connection);
    await editor().locator('[data-agent-tool-remove]').click();
    await catalog().locator('[name=kind]').selectOption('builtin');
    await expect(catalog().locator('[name=kind] option[value=builtin]')).toHaveAttribute('selected', '');
    await ready();
    await catalog().locator('[data-alias="message.reply"]').focus();
    await page.keyboard.press('Enter');
    await expect(editor().locator('[data-agent-tool-alias]')).toBeFocused();
    await editor().locator('[data-agent-tool-alias]').fill('respond');
    await editor().getByRole('button', { name: 'Move skill zeta later' }).focus();
    await page.keyboard.press('Enter');
    await expect(editor().locator('[name=skill_ids]')).toHaveValue(`${metadata.alpha},${metadata.zeta}`);
    const tools = JSON.parse(await editor().locator('[name=tool_assignments]').inputValue());
    assert.deepEqual(tools, [{ binding: null, alias: 'respond', choice: { kind: 'builtin', tool: 'message_reply' } }]);
    const panel = page.locator(`#agent-publication-${metadata.agent}`);
    const key = await panel.locator('[name=key]').inputValue();
    await panel.getByRole('button', { name: 'Publish saved draft', exact: true }).click();
    await expect(panel).toContainText('original request key');
    await expect(panel.locator('[name=key]')).toHaveValue(key);
    await expect(panel.locator('[name=expected_draft_revision]')).toHaveValue(String(metadata.initial_revision));
    await expect(editor().locator('[name=name]')).toHaveValue('Draft <browser>');
    await expect(editor().locator('[name=system_prompt]')).toHaveValue('browser unsaved prompt marker');
    await editor().getByRole('button', { name: 'Save Changes', exact: true }).click();
    await expect(page.locator('#agent-pane')).toContainText('Reload it before saving');
    await ready();
    await expect(editor().locator('[name=name]')).toHaveValue('Draft <browser>');
    await expect(editor().locator('[name=expected_revision]')).toHaveValue(String(metadata.initial_revision));
    const savedBody = writes.find(item => !item.path.endsWith('/publish')).body;
    assert.deepEqual(JSON.parse(savedBody.get('tool_assignments')), tools);
    assert.equal(savedBody.get('skill_ids'), `${metadata.alpha},${metadata.zeta}`);
    for (const url of queries) {
      for (const privateField of ['name', 'system_prompt', 'avatar_url', 'tool_assignments', 'skill_ids']) {
        assert.equal(url.searchParams.has(privateField), false, `${privateField} leaked into catalog URL ${url}`);
      }
    }
    // A saved response already exists server-side. Delay its delivery, navigate, then deliver its
    // pane, push URL and out-of-band sidebar together: none may overwrite the later selection.
    await editor().getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(editor().locator('[name=name]')).toHaveValue('Concurrent winner');
    await ready();
    await editor().locator('[name=name]').fill('Draft <browser>');
    await editor().locator('[name=system_prompt]').fill('browser unsaved prompt marker');
    await expect(catalog().locator('[data-alias="message.reply"]')).toBeVisible();
    await ready();
    await catalog().locator('[data-alias="message.reply"]').click();
    await editor().locator('[data-agent-tool-alias]').fill('respond');
    await editor().locator(`input[type=checkbox][value="${metadata.alpha}"]`).check();
    await editor().locator(`input[type=checkbox][value="${metadata.zeta}"]`).check();
    await editor().getByRole('button', { name: 'Move skill alpha earlier' }).click();
    await expect(editor().locator('[name=skill_ids]')).toHaveValue(`${metadata.alpha},${metadata.zeta}`);
    await editor().getByRole('button', { name: 'Save Changes', exact: true }).click();
    await expect.poll(() => typeof releaseWrite).toBe('function');
    const lateBody = writes.at(-1).body;
    assert.deepEqual(JSON.parse(lateBody.get('tool_assignments')), tools);
    assert.equal(lateBody.get('skill_ids'), `${metadata.alpha},${metadata.zeta}`);
    assert.equal(lateBody.get('expected_revision'), String(Number(metadata.initial_revision) + 1));
    await expect(page.locator('#agent-pane')).toHaveAttribute('aria-busy', 'true');
    await expect.poll(() => page.evaluate(() => window.pendingAgentEditor.querySelector('button[type=submit]').disabled)).toBe(true);
    await page.locator(`#agent-menu a[hx-get^="/ui/agents/${metadata.other}?"]`).click();
    await expect(page.locator('#agent-pane h2')).toHaveText('Navigator');
    await ready();
    const settled = page.waitForResponse(response => response.request().method() === 'PUT');
    releaseWrite(); await settled;
    await expect(page.locator('#agent-pane h2')).toHaveText('Navigator');
    await expect(page).toHaveURL(new RegExp(`agent_id=${metadata.other}`));
    assert.equal(await page.locator('#agent-pane img[src=x]').count(), 0);
    assert.deepEqual(errors, []);
  } catch (error) {
    if (page) {
      const failure = `${directory}/failure-${Date.now()}`;
      writeFileSync(`${failure}.html`, await page.content());
      writeFileSync(`${failure}.json`, JSON.stringify({ errors, queries: queries.map(url => url.href), writes: writes.map(write => write.path) }, null, 2));
      await page.screenshot({ path: `${failure}.png` });
      console.error(`Browser failure artifacts: ${failure}`);
    }
    throw error;
  } finally {
    releaseCatalog?.(); releaseWrite?.(); for (const response of waiting) response.destroy();
    await browser?.close(); server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});
