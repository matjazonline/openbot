// Browser journey over server-rendered Rust fixtures and the exact /assets/app.js body.
// Generate fixtures with dump_html_injection_browser_fixtures before running.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { chromium, expect } from '@playwright/test';

const directory = process.env.HTML_INJECTION_BROWSER_FIXTURES;
assert.ok(directory, 'HTML_INJECTION_BROWSER_FIXTURES is required');
const fixture = name => readFileSync(join(directory, name), 'utf8');
const root = new URL('../../', import.meta.url);
const htmx = readFileSync(new URL('assets/htmx-2.0.4.min.js', root));
const cspSource = readFileSync(new URL('src/adapters/http/security.rs', root), 'utf8');
const csp = cspSource.match(/const CSP: &str = "([^"]+)";/)?.[1];
assert.ok(csp, 'production CSP is present');
assert.ok(!csp.includes("script-src 'self' 'unsafe-inline'"));
assert.ok(!csp.includes('unsafe-eval'));

const companyId = '00000000-0000-4000-8000-000000000001';
const maliciousName = "victim<img/src=x/onerror=fetch('/exfil')>";
const maliciousEmail = 'tag<svg/onload=alert(1)>@example.com';
const team = [
  { email: 'owner@example.com', username: 'owner', role: 'owner' },
  { email: 'victim@example.com', username: maliciousName, role: 'member' },
  { email: maliciousEmail, username: 'tagged', role: 'member' },
];

test('form feedback and another member’s team autocomplete stay text under production CSP', async () => {
  const unexpectedRequests = [];
  const server = createServer(async (request, response) => {
    const pathname = new URL(request.url, 'http://127.0.0.1').pathname;
    if (pathname === '/favicon.ico' || pathname === '/assets/app.css' || pathname.startsWith('/assets/busybots-') || pathname === '/assets/theme-init.js') {
      response.writeHead(204).end();
      return;
    }
    if (pathname === '/assets/htmx-2.0.4.min.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(htmx);
      return;
    }
    if (pathname === '/assets/app.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(fixture('app.js'));
      return;
    }
    if (pathname === `/api/companies/${companyId}/team`) {
      response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ success: true, members: team }));
      return;
    }
    if (pathname === '/api/user/register' || pathname === '/api/user/login') {
      let submitted = '';
      for await (const chunk of request) submitted += chunk;
      const name = pathname.endsWith('/register')
        ? (new URLSearchParams(submitted).get('username') === 'reject' ? 'form-error.html' : 'register-success.html')
        : 'login-success.html';
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end(fixture(name));
      return;
    }
    const page = pathname === '/register' ? 'register.html' : pathname === '/login' ? 'login.html' : pathname === '/team' ? 'team.html' : null;
    if (page) {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'content-security-policy': csp }).end(fixture(page));
      return;
    }
    unexpectedRequests.push(pathname);
    response.writeHead(404).end();
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    const page = await browser.newPage();
    const pageErrors = [];
    page.on('pageerror', error => pageErrors.push(error.message));
    const base = `http://127.0.0.1:${server.address().port}`;

    await page.goto(`${base}/register`);
    await page.locator('[name=username]').fill(maliciousName);
    await page.locator('[name=email]').fill('victim@example.com');
    await page.locator('[name=password]').fill('password123');
    await page.locator('[name=confirm_password]').fill('password123');
    await page.locator('form [type=submit]').click();
    await expect(page.locator('#response-message')).toContainText(maliciousName);
    assert.equal(await page.locator('#response-message img, #response-message svg[onload]').count(), 0);

    await page.goto(`${base}/register`);
    await page.locator('[name=username]').fill('reject');
    await page.locator('[name=email]').fill('reject@example.com');
    await page.locator('[name=password]').fill('password123');
    await page.locator('[name=confirm_password]').fill('password123');
    await page.locator('form [type=submit]').click();
    await expect(page.locator('#response-message')).toContainText("<svg/onload=fetch('/exfil')>");
    assert.equal(await page.locator('#response-message svg[onload]').count(), 0);

    await page.goto(`${base}/login`);
    await page.locator('[name=email_or_username]').fill('victim@example.com');
    await page.locator('[name=password]').fill('password123');
    await page.locator('form [type=submit]').click();
    await expect(page.locator('#response-message')).toContainText(maliciousName);
    assert.equal(await page.locator('#response-message img, #response-message svg[onload]').count(), 0);

    await page.goto(`${base}/team`);
    await expect(page.locator('#viewer')).toHaveText('Viewer: owner@example.com');
    const input = page.locator('input[name=participant_emails]');
    await expect(page.locator('.team-chips-container')).toBeVisible();
    await input.fill('victim');
    const suggestion = page.locator('.team-dropdown > div').first();
    await expect(suggestion).toContainText(maliciousName);
    assert.equal(await page.locator('.team-dropdown img, .team-dropdown svg').count(), 0);
    await suggestion.dispatchEvent('mousedown');
    await expect(input).toHaveValue('victim@example.com, ');
    await expect(page.locator('.team-chips-container')).toContainText('victim@example.com');
    await input.fill('tag<svg');
    await expect(page.locator('.team-dropdown > div').first()).toContainText(maliciousEmail);
    assert.equal(await page.locator('.team-dropdown img, .team-dropdown svg').count(), 0);

    assert.deepEqual(unexpectedRequests, []);
    assert.deepEqual(pageErrors, []);
  } finally {
    await browser?.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});
