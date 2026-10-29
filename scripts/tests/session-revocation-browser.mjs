// Actual Rust profile markup/responses with the shipped HTMX and a real browser cookie jar.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const directory = process.env.SESSION_REVOCATION_BROWSER_FIXTURES;
assert.ok(directory, 'Generate fixtures with the ignored dump_session_revocation_browser_fixtures Rust test');
const profile = readFileSync(`${directory}/profile.html`, 'utf8');
const fixture = name => JSON.parse(readFileSync(`${directory}/${name}.json`, 'utf8'));
const responses = [fixture('failed'), fixture('success')];
const htmx = readFileSync(new URL('../../assets/htmx-2.0.4.min.js', import.meta.url));

test('failed password change preserves the browser; a committed change clears its cookie and navigates to login', async () => {
  let writes = 0;
  const cookies = [];
  const server = createServer(async (request, response) => {
    if (request.url === '/htmx.js') { response.writeHead(200, { 'content-type': 'text/javascript' }).end(htmx); return; }
    if (request.url === '/favicon.ico') { response.writeHead(204).end(); return; }
    if (request.url === '/login') { response.writeHead(200, { 'content-type': 'text/html' }).end('<h1>Sign in again</h1>'); return; }
    if (request.method === 'PUT' && request.url === '/ui/profile/password') {
      for await (const _ of request) { /* drain the form */ }
      cookies.push(request.headers.cookie ?? '');
      const result = responses[writes++];
      response.writeHead(result.status, result.headers.flat()).end(result.body);
      return;
    }
    if (request.url === '/ui/profile') {
      response.writeHead(200, { 'content-type': 'text/html', 'set-cookie': 'session=browser-test-cookie; Path=/; HttpOnly; SameSite=Lax' });
      response.end(`<!doctype html><html><head><script defer src="/htmx.js"></script></head><body>${profile}</body></html>`);
      return;
    }
    response.writeHead(404).end();
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    const page = await browser.newPage();
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    const base = `http://127.0.0.1:${server.address().port}`;
    await page.goto(`${base}/ui/profile`);
    const form = page.locator('form[hx-put="/ui/profile/password"]');
    const fill = async current => {
      await form.locator('[name="current_password"]').fill(current);
      await form.locator('[name="new_password"]').fill('new-password');
      await form.locator('[name="confirm_password"]').fill('new-password');
      await form.locator('button[type="submit"]').click();
    };
    await fill('wrong-password');
    await expect(page.getByText('That is not your current password.', { exact: true })).toBeVisible();
    assert.equal(page.url(), `${base}/ui/profile`);
    assert.ok((await page.context().cookies()).some(cookie => cookie.name === 'session'));
    await fill('old-password');
    await expect(page).toHaveURL(`${base}/login`);
    await expect(page.getByRole('heading', { name: 'Sign in again' })).toBeVisible();
    assert.equal((await page.context().cookies()).some(cookie => cookie.name === 'session'), false);
    assert.equal(writes, 2);
    assert.ok(cookies.every(cookie => cookie.includes('session=browser-test-cookie')));
    assert.deepEqual(errors, []);
  } finally {
    await browser?.close(); server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});
