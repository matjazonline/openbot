// Native workflow markup and source-span responses, shipped scripts/HTMX and production CSP.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const directory = process.env.WORKFLOW_AGENT_BROWSER_FIXTURES;
assert.ok(directory, 'Export dump_autonomous_workflow_agent_picker_browser_fixtures first');
const fixture = name => readFileSync(`${directory}/${name}`);
const metadata = JSON.parse(fixture('metadata.json'));
const root = new URL('../../', import.meta.url);
const csp = readFileSync(new URL('src/adapters/http/security.rs', root), 'utf8').match(/const CSP: &str = "([^"]+)";/)[1];

test('workflow agent choices preserve source and ignore transformations after editing or navigation', async () => {
  const requests = [];
  const waiting = new Set();
  let release;
  let hold = true;
  const html = (response, name) => response.writeHead(200, {
    'content-type': 'text/html', 'content-security-policy': csp,
  }).end(fixture(name));
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
    if (request.method === 'POST') {
      let text = ''; for await (const chunk of request) text += chunk;
      const body = new URLSearchParams(text);
      requests.push({ path: url.pathname, body, url });
      if (url.pathname === `/ui/workflows/${metadata.other}/agent-picker`) { response.writeHead(204).end(); return; }
      if (url.pathname.endsWith('/agent-picker')) {
        assert.ok([metadata.source, metadata.unsaved].includes(body.get('source')));
        html(response, body.get('agent') === metadata.agent ? 'selected-picker.html' : 'initial-picker.html'); return;
      }
      assert.equal(url.pathname, `/ui/workflows/${metadata.workflow}/agent-reference`);
      assert.equal(body.get('source'), metadata.unsaved);
      assert.equal(body.get('step'), 'start');
      assert.equal(body.get('agent'), metadata.agent);
      if (hold) {
        waiting.add(response);
        await new Promise(resolve => { release = resolve; });
        waiting.delete(response);
      }
      html(response, 'applied.html'); return;
    }
    if (url.pathname === `/ui/workflows/${metadata.other}`) { html(response, 'other-pane.html'); return; }
    if (url.pathname === '/ui/workflows') { html(response, 'page.html'); return; }
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
    page.on('dialog', dialog => dialog.accept());
    const base = `http://127.0.0.1:${server.address().port}`;
    await page.goto(`${base}/ui/workflows?company_id=${metadata.company}&workflow_id=${metadata.workflow}`);
    const picker = () => page.locator('#workflow-agent-picker');
    const source = () => page.locator('#workflow-source');
    const ready = () => expect(page.locator('#workflow-pane .htmx-added, #workflow-pane .htmx-settling')).toHaveCount(0);
    await expect(picker().locator('select[name=agent]')).toBeVisible();
    await ready();
    await source().fill(metadata.unsaved);
    await picker().locator('select[name=agent]').selectOption(metadata.agent);
    await expect(picker()).toContainText(metadata.revision);
    await expect(picker()).toContainText('message.reply');
    await expect(picker()).toContainText(/current/i);
    await ready();
    await expect(source()).toHaveValue(metadata.unsaved);
    await picker().getByRole('button', { name: 'Use selected agent in source' }).click();
    await expect.poll(() => typeof release).toBe('function');
    const edited = `${metadata.unsaved}# edited during selection\n`;
    await source().fill(edited);
    release();
    await expect(page.locator('#workflow-agent-reference-result')).toContainText('The source changed');
    await expect(source()).toHaveValue(edited);

    hold = false;
    await source().fill(metadata.unsaved);
    await picker().getByRole('button', { name: 'Refresh choices', exact: true }).click();
    await ready();
    await picker().getByRole('button', { name: 'Use selected agent in source' }).click();
    await expect(source()).toHaveValue(metadata.applied);
    await expect(source()).toBeFocused();
    await expect(page.locator('form[data-workflow-editor]')).toHaveAttribute('data-dirty', '');
    assert.ok(metadata.applied.includes('# browser unsaved comment'));

    await source().fill(metadata.unsaved);
    hold = true;
    release = undefined;
    await picker().getByRole('button', { name: 'Use selected agent in source' }).click();
    await expect.poll(() => typeof release).toBe('function');
    await page.locator(`a[hx-get^="/ui/workflows/${metadata.other}?"]`).first().click();
    await expect(source()).toHaveValue(metadata.other_source);
    release();
    await ready();
    await expect(source()).toHaveValue(metadata.other_source);
    assert.equal(requests.filter(item => !item.path.endsWith('/agent-picker') && !item.path.endsWith('/agent-reference')).length, 0,
      'selection never saves or publishes the workflow');
    for (const request of requests) {
      assert.equal(request.url.searchParams.has('source'), false, 'unsaved source stays in POST bodies');
    }
    assert.deepEqual(errors, []);
  } catch (error) {
    if (page) {
      writeFileSync(`${directory}/failure.html`, await page.content());
      writeFileSync(`${directory}/failure.json`, JSON.stringify({ errors, requests: requests.map(item => ({ path: item.path, body: Object.fromEntries(item.body) })) }, null, 2));
      await page.screenshot({ path: `${directory}/failure.png` });
    }
    throw error;
  } finally {
    release?.();
    for (const response of waiting) response.destroy();
    await browser?.close();
    await new Promise(resolve => server.close(resolve));
  }
});
