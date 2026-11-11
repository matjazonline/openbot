// Real Rust markup, shipped JS/CSS and HTMX. Rust route tests cover persistence and authorization.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFileSync } from 'node:fs';
import { chromium, expect } from '@playwright/test';

const directory = process.env.SKILL_BUNDLE_BROWSER_FIXTURES;
assert.ok(directory, 'Run dump_skill_bundle_browser_fixtures first');
const fixture = name => readFileSync(`${directory}/${name}`);
const root = new URL('../../', import.meta.url);
const csp = readFileSync(new URL('src/adapters/http/security.rs', root), 'utf8').match(/const CSP: &str = "([^"]+)";/)[1];

test('skill ZIP uploads retain multipart bytes and show progress, success and failure in both themes', async () => {
  const writes = [];
  const pending = new Set();
  let release;
  let outcome = 'success.html';
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (request.method === 'POST') {
      const chunks = []; for await (const chunk of request) chunks.push(chunk);
      writes.push({ path: url.pathname, body: Buffer.concat(chunks), type: request.headers['content-type'] });
      pending.add(response);
      await new Promise(resolve => { release = resolve; });
      pending.delete(response);
      response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture(outcome));
      return;
    }
    if (url.pathname.startsWith('/assets/')) {
      const name = url.pathname.split('/').at(-1);
      try {
        const bytes = ['app.js', 'theme-init.js'].includes(name) ? fixture(name) : readFileSync(new URL(url.pathname.slice(1), root));
        response.writeHead(200, { 'content-type': name.endsWith('.css') ? 'text/css' : 'text/javascript' }).end(bytes);
      } catch { response.writeHead(404).end(); }
      return;
    }
    response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp })
      .end(fixture(url.pathname.endsWith('/bundle/files') ? 'files.html' : 'page.html'));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
  try {
    const page = await browser.newPage();
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    for (const theme of ['dark', 'light']) {
      await page.goto(`http://127.0.0.1:${server.address().port}/skills`);
      await page.evaluate(theme => document.documentElement.setAttribute('data-theme', theme), theme);
      const form = () => page.locator('form[hx-encoding="multipart/form-data"]');
      const button = () => form().locator('button[type=submit]');
      const before = writes.length;
      await button().click();
      assert.equal(writes.length, before, 'invalid native form starts no request');
      await expect(form()).not.toHaveClass(/htmx-request/);
      await expect(page.getByText('scripts/check.py', { exact: false }).last()).toBeVisible();
      for (const result of ['success.html', 'failure.html']) {
        outcome = result;
        await form().locator('input[type=file]').setInputFiles({ name: 'skill.zip', mimeType: 'application/zip', buffer: fixture('skill.zip') });
        await button().click();
        await expect.poll(() => writes.length).toBe(before + (result === 'success.html' ? 1 : 2));
        await expect(form()).toHaveClass(/htmx-request/);
        await expect(button()).toBeDisabled();
        await expect(page.locator('#skill-editor button[type=submit]')).toBeDisabled();
        await expect(button()).toContainText('Importing');
        await expect(button().getByText('Importing…', { exact: true })).toBeVisible();
        const submitted = writes.at(-1);
        assert.match(submitted.type, /^multipart\/form-data; boundary=/);
        assert.ok(submitted.body.includes(fixture('skill.zip')), 'exact ZIP bytes reach multipart body');
        assert.ok(submitted.body.includes(Buffer.from('name="bundle"; filename="skill.zip"')));
        release();
        await expect(page.getByText(result === 'success.html' ? 'Skill directory imported.' : 'Invalid skill ZIP. Choose another file.', { exact: true })).toBeVisible();
        await expect(form()).not.toHaveClass(/htmx-request/);
        await expect(button()).toBeEnabled();
        await expect(page.locator('#skill-editor button[type=submit]')).toBeEnabled();
      }
    }
    assert.deepEqual(errors, []);
  } finally {
    release?.(); for (const response of pending) response.destroy();
    await browser.close(); await new Promise(resolve => server.close(resolve));
  }
});
