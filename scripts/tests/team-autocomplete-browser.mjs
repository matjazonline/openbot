// Run with APP_JS_OUT=<temporary file> after the ignored
// dump_application_javascript Rust test writes the exact /assets/app.js body.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const script = readFileSync(process.env.APP_JS_OUT, 'utf8');
const companyId = '00000000-0000-4000-8000-000000000001';
const maliciousName = "victim<img/src=x/onerror=fetch('/exfil')>";
const maliciousEmail = 'tag<svg/onload=alert(1)>@example.com';
const members = [
  { email: 'victim@example.com', username: maliciousName, role: 'member' },
  { email: maliciousEmail, username: 'second', role: 'member' },
];

test('served team autocomplete renders account strings as text and keeps selection working', async () => {
  const unexpectedRequests = [];
  const server = createServer((request, response) => {
    if (request.url === '/favicon.ico') {
      response.writeHead(204).end();
      return;
    }
    if (request.url === '/assets/app.js') {
      response.writeHead(200, { 'content-type': 'text/javascript; charset=utf-8' });
      response.end(script);
      return;
    }
    if (request.url === `/api/companies/${companyId}/team`) {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ success: true, members }));
      return;
    }
    if (request.url !== '/') {
      unexpectedRequests.push(request.url);
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, {
      'content-type': 'text/html; charset=utf-8',
      'content-security-policy': "default-src 'self'; script-src 'self'; connect-src 'self'; img-src 'self' data:; style-src 'self'",
    });
    response.end(`<!doctype html><html><head><script defer src="/assets/app.js"></script></head><body><label>Participants<input name="participant_emails" data-company-id="${companyId}"></label></body></html>`);
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    const page = await browser.newPage();
    const pageErrors = [];
    page.on('pageerror', error => pageErrors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    const input = page.locator('input[name="participant_emails"]');
    await expect(page.locator('.team-chips-container')).toBeVisible();

    await input.fill('victim');
    const first = page.locator('.team-dropdown > div').first();
    await expect(first).toBeVisible();
    await expect(first).toContainText(maliciousName);
    assert.equal(await page.locator('.team-dropdown img, .team-dropdown svg').count(), 0);
    await first.dispatchEvent('mousedown');
    await expect(input).toHaveValue('victim@example.com, ');
    await expect(page.locator('.team-chips-container')).toContainText('victim@example.com');

    await input.fill('tag<svg');
    const second = page.locator('.team-dropdown > div').first();
    await expect(second).toBeVisible();
    await expect(second).toContainText(maliciousEmail);
    assert.equal(await page.locator('.team-dropdown svg, .team-dropdown img').count(), 0);
    assert.deepEqual(unexpectedRequests, []);
    assert.deepEqual(pageErrors, []);
  } finally {
    await browser?.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});
