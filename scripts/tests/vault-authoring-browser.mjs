// Actual authenticated route markup, the shipped JavaScript/HTMX, and a real browser.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';
const directory = process.env.VAULT_AUTHORING_BROWSER_FIXTURES;
assert.ok(directory, 'generate fixtures with vault_authoring_picker_is_owner_scoped_and_only_exposes_approved_metadata');
const fixture = name => readFileSync(`${directory}/${name}`);
const asset = name => readFileSync(new URL(`../../assets/${name}`, import.meta.url));

for (const theme of ['dark', 'light']) test(`vault ${theme}: native picker progress, escaped private metadata and recoverable consent`, async () => {
  const pending = [], errors = [], submits = [];
  let writes = 0;
  const server = createServer(async (request, response) => {
    const path = new URL(request.url, 'http://localhost').pathname;
    if (path.startsWith('/assets/')) {
      if (path.endsWith('.css')) return response.writeHead(200, {'content-type':'text/css'}).end(asset('app.css'));
      if (path.includes('htmx-2.0.4')) return response.writeHead(200, {'content-type':'text/javascript'}).end(asset('htmx-2.0.4.min.js'));
      if (path.startsWith('/assets/app.') && path.endsWith('.js')) return response.writeHead(200, {'content-type':'text/javascript'}).end(fixture('app.js'));
      return response.writeHead(200, {'content-type':'text/javascript'}).end('');
    }
    if (request.method === 'POST') {
      let body = ''; for await (const chunk of request) body += chunk;
      writes++; pending.push({response, body, path}); return;
    }
    if (path === '/done') return response.end('<h1>Consent recorded</h1>');
    response.writeHead(200, {'content-type':'text/html'}).end(fixture(path === '/consent' ? 'consent.html' : 'source.html'));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  let browser;
  try {
    browser = await chromium.launch(process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH} : {});
    const page = await browser.newPage(); page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => { if(message.text().startsWith('vault-submit:')) submits.push(JSON.parse(message.text().slice(13))); });
    await page.route('**/*', route => route.request().url().startsWith(base) ? route.continue() : route.abort());
    await page.goto(base); await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    assert.deepEqual(errors, [], 'shipped scripts initialize before submitting');
    assert.equal(await page.evaluate(() => typeof markSubmitBusy), 'function');
    const form = page.locator('form[data-submit="busy-once"]');
    const choose = form.getByRole('button', {name:'Choose private account'});
    // Read pending native navigation inside the real submit event. Playwright's DOM
    // queries otherwise wait for navigation to finish before inspecting the old page.
    await page.evaluate(() => document.addEventListener('submit', event => {
      const form = event.target;
      if (!form.matches('form[data-submit="busy-once"]') || event.defaultPrevented) return;
      const button = event.submitter;
      const duplicate = new SubmitEvent('submit', {bubbles:true, cancelable:true, submitter:button});
      const duplicateAccepted = form.dispatchEvent(duplicate);
      console.log('vault-submit:' + JSON.stringify({busy:form.getAttribute('aria-busy'),
        disabled:button.getAttribute('aria-disabled'), label:button.textContent,
        progress:getComputedStyle(form.querySelector('[data-progress]')).display,
        duplicateAccepted}));
    }));
    await choose.focus(); await choose.press('Enter', {noWaitAfter:true});
    await expect.poll(() => pending.length).toBe(1);
    assert.equal(submits.length, 1);
    assert.equal(submits[0].busy, 'true'); assert.equal(submits[0].disabled, 'true');
    assert.ok(submits[0].label.includes('Requesting confirmation'));
    assert.notEqual(submits[0].progress, 'none'); assert.equal(submits[0].duplicateAccepted, false);
    assert.equal(writes, 1);
    const first = pending.shift(); assert.ok(first.body.includes('expected_revision=') && first.body.includes('app='));
    first.response.writeHead(303, {location:'/consent'}).end();
    await expect(page).toHaveURL(`${base}/consent`);
    await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    const select = page.locator('select[name="account_revision"]');
    await expect(select.locator('option')).toHaveText(['<private label>']);
    assert.equal(await page.locator('private').count(), 0, 'stored label remains text');
    assert.ok(!(await page.content()).includes('sentinel-never-in-browser'));
    const confirm = page.getByRole('button', {name:'Confirm connection use'});
    await confirm.focus(); await confirm.press('Enter');
    await expect.poll(() => pending.length).toBe(1);
    await expect(confirm).toBeDisabled(); await expect(confirm).toHaveAttribute('aria-busy', 'true');
    await expect(confirm.locator('[data-request-spinner]')).toBeVisible();
    pending.shift().response.writeHead(422, {'content-type':'text/plain'}).end('Connection changed; reload and review again.');
    await expect(confirm).toBeEnabled(); await expect(confirm).not.toHaveAttribute('aria-busy', 'true');
    await expect(page.locator('#request-error-message')).toContainText('Connection changed');
    await confirm.press('Enter'); await expect.poll(() => pending.length).toBe(1);
    pending.shift().response.writeHead(200, {'hx-redirect':'/done'}).end();
    await expect(page).toHaveURL(`${base}/done`);
    assert.equal(writes, 3); assert.deepEqual(errors, []);
  } finally {
    await browser?.close(); for (const item of pending) item.response.destroy();
    server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
  }
});

for (const theme of ['dark', 'light']) test(`vault ${theme}: owner secrets start blank and reconnect failures remain recoverable`, async () => {
  const pending = [], errors = [];
  const server = createServer(async (request, response) => {
    const path = new URL(request.url, 'http://localhost').pathname;
    if (path.startsWith('/assets/')) {
      if (path.endsWith('.css')) return response.writeHead(200, {'content-type':'text/css'}).end(asset('app.css'));
      if (path.includes('htmx-2.0.4')) return response.writeHead(200, {'content-type':'text/javascript'}).end(asset('htmx-2.0.4.min.js'));
      if (path.startsWith('/assets/app.') && path.endsWith('.js')) return response.writeHead(200, {'content-type':'text/javascript'}).end(fixture('app.js'));
      return response.writeHead(200, {'content-type':'text/javascript'}).end('');
    }
    if (request.method === 'POST') {
      let body = ''; for await (const chunk of request) body += chunk;
      pending.push({response, body, path}); return;
    }
    response.writeHead(200, {'content-type':'text/html'}).end(fixture(path === '/reconnect' ? 'reconnect.html' : 'owner.html'));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  let browser;
  try {
    browser = await chromium.launch(process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH} : {});
    const page = await browser.newPage(); page.on('pageerror', error => errors.push(error.message));
    await page.route('**/*', route => route.request().url().startsWith(base) ? route.continue() : route.abort());
    await page.goto(base); await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    const secrets = page.locator('textarea[name="value"]');
    assert.equal(await secrets.count(), 2);
    assert.deepEqual(await secrets.evaluateAll(nodes => nodes.map(node => node.value)), ['', '']);
    await expect(page.getByRole('heading', {name:'<owner label>'})).toBeVisible();
    assert.equal(await page.locator('owner').count(), 0);
    assert.ok(!(await page.content()).includes('owner-form-secret-sentinel'));
    const create = page.locator('form[hx-post="/ui/profile/vault"]');
    await create.locator('[name="key"]').fill('browser-new');
    await create.locator('[name="label"]').fill('New private account');
    await create.locator('[name="value"]').fill('browser-entered-secret');
    const save = create.getByRole('button', {name:'Save credential'});
    await save.focus(); await save.press('Enter');
    await expect.poll(() => pending.length).toBe(1);
    const submitted = pending.shift();
    const fields = new URLSearchParams(submitted.body);
    assert.equal(fields.get('value'), 'browser-entered-secret');
    assert.equal(fields.has('owner'), false);
    submitted.response.writeHead(422, {'content-type':'text/plain'}).end('Please review the current account form.');
    await expect(save).toBeEnabled();
    await expect(page.locator('#request-error-message')).toContainText('review the current');
    await page.goto(`${base}/reconnect`); await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
    await expect(page.getByText('Reconnect required', {exact:true})).toBeVisible();
    const reconnect = page.locator('form').filter({has:page.getByRole('button', {name:'Reconnect gmail', exact:true})});
    assert.ok(await reconnect.locator('[name="account"]').inputValue());
    assert.equal(await reconnect.locator('[name="authorization"]').inputValue(), '2');
    const retry = reconnect.getByRole('button', {name:'Reconnect gmail', exact:true});
    await retry.focus(); await retry.press('Enter');
    await expect.poll(() => pending.length).toBe(1);
    await expect(retry).toBeDisabled();
    const attempt = pending.shift(); assert.equal(attempt.path, '/ui/profile/vault/apps/gmail/connect');
    assert.equal(new URLSearchParams(attempt.body).get('authorization'), '2');
    attempt.response.writeHead(422, {'content-type':'text/plain'}).end('Account changed; reload before reconnecting.');
    await expect(retry).toBeEnabled();
    await expect(page.locator('#request-error-message')).toContainText('Account changed');
    assert.deepEqual(errors, []);
  } finally {
    await browser?.close(); for (const item of pending) item.response.destroy();
    server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
  }
});
