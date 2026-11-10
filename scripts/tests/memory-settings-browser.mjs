import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

// Phase 05 E8: the company memory section's per-agent capture forms in a real browser, served with
// the production CSP, htmx and application scripts (fixtures from dump_memory_settings_browser_fixtures).
const root = new URL('../../', import.meta.url);
const dir = process.env.MEMORY_SETTINGS_BROWSER_FIXTURES;
const fixture = name => readFileSync(`${dir}/${name}`);
const cspSource = readFileSync(new URL('src/adapters/http/security.rs', root), 'utf8');
const csp = cspSource.match(/const CSP: &str = "([^"]+)";/)[1];
const asset = name => readFileSync(new URL(`assets/${name}`, root));

test('capture forms are labelled, refuse a blank reason, post the policy and swap in the saved section', async () => {
  const posts = [];
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (request.method === 'POST') {
      let body = '';
      for await (const chunk of request) body += chunk;
      posts.push({ path: url.pathname, form: Object.fromEntries(new URLSearchParams(body)) });
      response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture('saved.html'));
      return;
    }
    if (url.pathname === '/assets/app.js' || url.pathname === '/assets/theme-init.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(fixture(url.pathname.endsWith('app.js') ? 'app.js' : 'theme-init.js'));
      return;
    }
    if (url.pathname === '/assets/app.css') { response.writeHead(200, { 'content-type': 'text/css' }).end(asset('app.css')); return; }
    if (url.pathname === '/assets/htmx-2.0.4.min.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(asset('htmx-2.0.4.min.js'));
      return;
    }
    if (url.pathname.startsWith('/assets/') || url.pathname === '/favicon.ico') { response.writeHead(204).end(); return; }
    response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture('memory.html'));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch();
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/companies/settings`);
    const capture = page.locator('#company-memory-capture');
    await expect(capture).toBeVisible();
    // The agent name is text, never markup.
    await expect(capture.locator('legend')).toHaveText('Support <agent>');
    assert.equal(await capture.locator('agent').count(), 0);
    await expect(capture).toContainText('lowest-reader level');
    await expect(capture).toContainText('never reach private memory');
    // Every visible field is labelled.
    const unlabelled = await capture.evaluate(node =>
      [...node.querySelectorAll('input:not([type=hidden])')].filter(input => input.labels.length === 0).length);
    assert.equal(unlabelled, 0);
    const threads = capture.locator('form', { hasText: 'Threads: capture is off' });
    // A blank reason is refused by the browser before anything is posted.
    await threads.getByRole('button', { name: 'Turn on' }).click();
    assert.equal(posts.length, 0);
    await threads.getByLabel('Reason').fill('Thread pilot');
    await threads.getByRole('button', { name: 'Turn on' }).click();
    await expect(page.locator('#company-memory [role=status]').first()).toContainText('Capture settings saved. Thread capture');
    await expect(page.locator('#company-memory-capture')).toContainText('Threads: capture is on (revision 1)');
    assert.equal(posts.length, 1);
    assert.match(posts[0].path, /^\/ui\/companies\/[0-9a-f-]+\/memory\/capture$/);
    assert.deepEqual(
      { kind: posts[0].form.kind, enabled: posts[0].form.enabled, revision: posts[0].form.expected_revision, reason: posts[0].form.reason, allowance: posts[0].form.allowance_tokens, output: posts[0].form.max_output_tokens, expiry: posts[0].form.expiry_hours },
      { kind: 'threads', enabled: 'true', revision: '0', reason: 'Thread pilot', allowance: '32768', output: '2048', expiry: '24' },
    );
    // Both themes keep the section readable (text and field colours differ from the surface).
    const backgrounds = new Set();
    for (const theme of ['light', 'dark']) {
      // The saved choice, as the theme toggle records it; app.js keeps `data-theme` in step with it.
      await page.evaluate(value => localStorage.setItem('ui_theme', value), theme);
      await page.reload();
      await page.waitForFunction(value => document.documentElement.getAttribute('data-theme') === value, theme);
      const styles = await page.evaluate(() => {
        // The surface colour is on <html> (`bg-base-100`); text inherits it in the section.
        const surface = getComputedStyle(document.documentElement);
        const text = getComputedStyle(document.querySelector('#company-memory-capture p'));
        const input = getComputedStyle(document.querySelector('#company-memory-capture input:not([type=hidden])'));
        return { foreground: text.color, background: surface.backgroundColor, field: input.color, fieldBackground: input.backgroundColor };
      });
      assert.notEqual(styles.foreground, styles.background, theme);
      assert.notEqual(styles.field, styles.fieldBackground, theme);
      backgrounds.add(styles.background);
      await page.locator('#company-memory').screenshot({ path: `${dir}/memory-${theme}.png` });
    }
    assert.equal(backgrounds.size, 2, 'the two themes differ');
    assert.deepEqual(errors, []);
  } finally {
    await browser?.close();
    await new Promise(resolve => server.close(resolve));
  }
});
