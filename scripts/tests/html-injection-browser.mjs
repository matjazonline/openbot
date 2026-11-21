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
const sse = readFileSync(new URL('assets/htmx-ext-sse-2.2.3.js', root));
const css = readFileSync(new URL('assets/app.css', root));
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

const deskPages = ['desk-classic-create', 'desk-classic-edit', 'desk-modern-create', 'desk-modern-edit'];
const bindingRequest = fixture('desk-modern-edit.html').match(/hx-get="([^" ]*\/bindings\?[^" ]*)"/)?.[1];
assert.ok(bindingRequest, 'the modern edit pane has its real lazy binding request');
const bindingPath = new URL(bindingRequest, 'http://127.0.0.1').pathname;

async function verifyDeskAccessForms(page, base, context) {
  for (const variant of deskPages) {
    await page.goto(`${base}/${variant}`);
    const form = page.locator('form:has(input[name="public"])');
    await expect(form).toHaveCount(1);
    const owner = form.locator('select[name="department_id"]');
    const publicInput = form.locator('input[name="public"]');
    const confirmation = form.locator('input[name="confirm_spam_disabled"]');
    const addresses = form.locator('input[name="participant_emails"]');
    assert.equal(await owner.getAttribute('required'), '');
    const initialOwner = await owner.inputValue();
    assert.equal(await owner.evaluate(select => select.selectedOptions[0].textContent.trim()),
      variant.endsWith('edit') ? 'Dispatch' : 'Company root');
    assert.equal(await publicInput.isChecked(), variant.endsWith('edit'));
    assert.equal(await confirmation.isDisabled(), !variant.endsWith('edit'));
    await expect(addresses).toHaveValue(variant.endsWith('edit') ? 'listed@example.test' : '');
    await form.getByLabel('Desk Name', { exact: true }).fill('Changed name');
    await form.locator('[name="slug"]').fill('changed-desk');
    await publicInput.check();
    await expect(confirmation).toBeEnabled();
    await confirmation.check();
    await addresses.fill('changed@example.test');
    await expect(confirmation).toBeChecked();
    const data = await form.evaluate(element => Object.fromEntries(new FormData(element)));
    assert.equal(data.public, 'true');
    assert.equal(data.confirm_spam_disabled, 'true');
    assert.equal(data.participant_emails, 'changed@example.test');
    assert.equal(data.department_id, initialOwner);
    for (const theme of ['light', 'dark']) {
      await page.evaluate(value => {
        document.documentElement.setAttribute('data-theme', value);
        const toggle = document.querySelector('input.theme-controller');
        if (toggle) toggle.checked = value === 'light';
      }, theme);
      await expect(owner).toBeVisible();
      await expect(publicInput).toBeVisible();
      for (const field of [owner, addresses]) {
        await field.focus();
        const computed = await field.evaluate(element => {
          const style = getComputedStyle(element);
          return { radius: style.borderRadius, outlineOffset: style.outlineOffset };
        });
        const expected = variant.startsWith('desk-modern')
          ? { radius: '8px', outlineOffset: '-1px' }
          : { radius: '4px', outlineOffset: '2px' };
        assert.deepEqual(computed, expected, `${variant}/${theme}`);
      }
    }
    await publicInput.uncheck();
    await expect(confirmation).toBeDisabled();
    await expect(confirmation).not.toBeChecked();
    const closed = await form.evaluate(element => Object.fromEntries(new FormData(element)));
    assert.ok(!Object.hasOwn(closed, 'public'));
    assert.ok(!Object.hasOwn(closed, 'confirm_spam_disabled'));
    assert.equal(closed.department_id, initialOwner);
    assert.equal(closed.participant_emails, 'changed@example.test');
    const otherOwner = await owner.locator('option').evaluateAll(options =>
      options.find(option => !option.selected).value);
    await owner.selectOption(otherOwner);
    assert.equal(await form.evaluate(element => new FormData(element).get('department_id')), otherOwner);
    await form.evaluate(element => element.reset());
    await expect(owner).toHaveValue(initialOwner);
    assert.equal(await publicInput.isChecked(), variant.endsWith('edit'));
    assert.equal(await confirmation.isDisabled(), !variant.endsWith('edit'));
    context.diagnostic(`${variant}: native controls, Public preservation/off/reset, owner, spam interlock and both-theme computed styles passed`);
  }
}

test('form feedback and another member’s team autocomplete stay text under production CSP', async context => {
  const unexpectedRequests = [];
  const server = createServer(async (request, response) => {
    const pathname = new URL(request.url, 'http://127.0.0.1').pathname;
    if (pathname === '/assets/app.css') {
      response.writeHead(200, { 'content-type': 'text/css' }).end(css);
      return;
    }
    if (pathname === '/favicon.ico' || pathname.startsWith('/assets/busybots-') || pathname === '/assets/theme-init.js') {
      response.writeHead(204).end();
      return;
    }
    if (pathname === '/assets/htmx-2.0.4.min.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(htmx);
      return;
    }
    if (pathname === '/assets/htmx-ext-sse-2.2.3.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(sse);
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
    if (pathname === bindingPath) {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' }).end('<p>Binding pane fixture</p>');
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
    const page = pathname === '/register' ? 'register.html' : pathname === '/login' ? 'login.html' : pathname === '/team' ? 'team.html' : pathname === '/restricted' ? 'restricted.html' : deskPages.includes(pathname.slice(1)) ? `${pathname.slice(1)}.html` : null;
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

    await page.goto(`${base}/restricted`);
    const restricted = page.locator('[data-restricted-message]');
    await expect(restricted).toContainText('Restricted message');
    await expect(restricted.locator('.chat-header')).toContainText("writer<img/src=x/onerror=fetch('/exfil')>");
    assert.equal(await restricted.locator('img, svg, a, textarea, [download]').count(), 0);
    const markup = await page.content();
    for (const secret of ['private-subject-canary', 'private-body-canary', 'private-attachment-canary']) {
      assert.ok(!markup.includes(secret), `restricted markup leaked ${secret}`);
    }
    for (const theme of ['light', 'dark']) {
      await page.evaluate(value => document.documentElement.setAttribute('data-theme', value), theme);
      await expect(restricted.locator('.chat-bubble')).toBeVisible();
      const style = await restricted.locator('.chat-bubble').evaluate(element => {
        const value = getComputedStyle(element);
        return { opacity: value.opacity, color: value.color, background: value.backgroundColor };
      });
      assert.equal(style.opacity, '0.7');
      assert.notEqual(style.color, style.background);
    }

    await verifyDeskAccessForms(page, base, context);

    assert.deepEqual(unexpectedRequests, []);
    assert.deepEqual(pageErrors, []);
  } finally {
    await browser?.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});
