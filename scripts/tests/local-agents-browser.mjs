// Production Rust markup and scripts; HTTP authorization and persistence are tested in Rust.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const directory = process.env.CLOUDROOM_MANAGEMENT_BROWSER_FIXTURES;
assert.ok(directory, 'Run dump_cloudroom_management_browser_fixtures against owned PostgreSQL');
const fixture = name => readFileSync(`${directory}/${name}`);
const metadata = JSON.parse(fixture('metadata.json'));
const paging = JSON.parse(fixture('profiles-navigation.json'));
const root = new URL('../../', import.meta.url);
const csp = readFileSync(new URL('src/adapters/http/security.rs', root), 'utf8').match(/const CSP: &str = "([^"]+)";/)[1];

test('local profile and session forms escape native output and acknowledge accepted writes', async () => {
  const writes = [];
  const pending = new Set();
  let release;
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (request.method === 'POST') {
      let body = ''; for await (const chunk of request) body += chunk;
      writes.push({ path: url.pathname, fields: new URLSearchParams(body) });
      pending.add(response);
      await new Promise(resolve => { release = resolve; });
      pending.delete(response);
      response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture('page.html'));
      return;
    }
    if (url.pathname.startsWith('/assets/')) {
      const name = url.pathname.split('/').at(-1);
      try {
        const content = ['app.js', 'theme-init.js'].includes(name) ? fixture(name) : readFileSync(new URL(url.pathname.slice(1), root));
        response.writeHead(200, { 'content-type': name.endsWith('.css') ? 'text/css' : 'text/javascript' }).end(content);
      } catch { response.writeHead(404).end(); }
      return;
    }
    const file = url.searchParams.get('profile') === paging.profile ? 'profiles-selected.html'
      : url.searchParams.get('after_profile') === paging.cursor ? 'profiles-next.html'
      : url.searchParams.get('conversation') === paging.conversation ? 'profiles-first.html' : 'page.html';
    response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture(file));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
  try {
    const page = await browser.newPage({ viewport: { width: 1400, height: 1100 } });
    const errors = [];
    const submissions = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if (message.text().startsWith('LOCAL_FORM_STATE ')) submissions.push(JSON.parse(message.text().slice(17)));
    });
    await page.goto(`http://127.0.0.1:${server.address().port}/ui/companies/${metadata.company}/local-agents`);
    const observeSubmits = () => page.evaluate(() => document.addEventListener('submit', event => {
      const form = event.target;
      console.info('LOCAL_FORM_STATE ' + JSON.stringify({ busy: form.getAttribute('aria-busy'),
        disabled: form.querySelector('button').getAttribute('aria-disabled') }));
    }));
    await observeSubmits();
    await expect(page.locator('main img')).toHaveCount(0);
    await expect(page.getByText('Result <img src=x onerror=alert(1)>', { exact: true })).toBeVisible();
    await expect(page.getByText('Measured usage: Unavailable', { exact: true })).toBeVisible();
    await expect(page.getByText('Waiting for input or permission. Respond in Cloudroom on the machine.')).toBeVisible();
    await expect(page.getByText(/permission waits · .*uncertain server outcomes · .*reconciliation gaps/)).toBeVisible();
    await expect(page.getByText(/Oldest outstanding command: .*Last matching native success:/)).toBeVisible();
    await expect(page.getByText(/Local journal pressure is reported by the plugin status command/)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Stop entire thread', exact: true })).toBeVisible();
    await expect(page.locator('input[readonly]')).toHaveValue('thread_management');
    for (const theme of ['dark', 'light']) {
      await page.evaluate(theme => document.documentElement.setAttribute('data-theme', theme), theme);
      const field = page.locator('input[name=name]');
      await expect(field).toHaveValue('Profile "<img src=x onerror=alert(1)>');
      await field.focus();
      assert.equal(await field.evaluate(element => getComputedStyle(element).outlineOffset), '-1px');
    }
    const form = page.locator(`form[action$="/sessions"]`).filter({ has: page.locator('textarea[name=prompt]') });
    await form.locator('textarea').fill('');
    await form.getByRole('button').click();
    assert.equal(writes.length, 0, 'invalid native form does not start work');
    await expect(form).not.toHaveAttribute('aria-busy', 'true');
    await form.locator('textarea').fill('Browser explicit native task');
    await form.getByRole('button').click({ noWaitAfter: true });
    await expect.poll(() => writes.length).toBe(1);
    await expect.poll(() => submissions.length).toBe(1);
    assert.deepEqual(submissions[0], { busy: 'true', disabled: 'true' });
    assert.equal(writes[0].fields.get('prompt'), 'Browser explicit native task');
    assert.equal(writes[0].fields.get('conversation_id'), metadata.conversation);
    assert.equal(writes[0].fields.get('profile_id'), metadata.profile);
    assert.match(writes[0].fields.get('request_id'), /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    release();
    await page.waitForURL('**/sessions');
    await page.waitForLoadState('load');
    const base = `http://127.0.0.1:${server.address().port}/ui/companies/${paging.company}/local-agents`;
    await page.goto(`${base}?channel=${paging.channel}&conversation=${paging.conversation}`);
    await page.getByRole('link', { name: 'More profiles', exact: true }).click();
    assert.equal(new URL(page.url()).searchParams.get('conversation'), paging.conversation);
    await page.getByRole('link', { name: paging.name, exact: true }).click();
    assert.equal(new URL(page.url()).searchParams.get('conversation'), paging.conversation);
    await expect(form.locator('[name=profile_id]')).toHaveValue(paging.profile);
    await expect(form.locator('[name=conversation_id]')).toHaveValue(paging.conversation);
    await observeSubmits();
    await form.locator('textarea').fill('Use the selected later-page profile');
    await form.getByRole('button').click({ noWaitAfter: true });
    await expect.poll(() => writes.length).toBe(2);
    assert.equal(writes[1].fields.get('profile_id'), paging.profile);
    assert.equal(writes[1].fields.get('conversation_id'), paging.conversation);
    release();
    await page.waitForURL('**/sessions');
    await page.waitForLoadState('load');
    assert.deepEqual(errors, []);
  } finally {
    release?.();
    for (const response of pending) response.destroy();
    await browser.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});


test('an Auto agent on an Auto-only provider saves the unchanged unsupported fallback and clearing the agent restores validation',async()=>{
  const metadata=JSON.parse(fixture('profile-agent-permissions.json'));const writes=[];
  const server=createServer(async(request,response)=>{
    if(request.method==='POST'){let body='';for await(const chunk of request)body+=chunk;writes.push(new URLSearchParams(body));}
    const pathname=new URL(request.url,'http://127.0.0.1').pathname;
    if(pathname.startsWith('/assets/')){
      const name=pathname.split('/').at(-1);const content=['app.js','theme-init.js'].includes(name)?fixture(name):readFileSync(new URL(pathname.slice(1),root));
      response.writeHead(200,{'content-type':name.endsWith('.css')?'text/css':'text/javascript'}).end(content);return;
    }
    response.writeHead(200,{'content-type':'text/html','content-security-policy':csp}).end(fixture('profiles-agent-permissions.html'));
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const browser=await chromium.launch(process.env.CHROME_PATH?{executablePath:process.env.CHROME_PATH}:{});
  try {
    const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/ui/local-agents`);
    const form=page.locator('form').filter({has:page.locator('[data-local-agent-selector]')});
    const permissions=form.locator('[data-local-agent-permissions]');const agent=form.locator('[data-local-agent-selector]');
    await expect(permissions).toHaveValue('');assert.equal(await form.evaluate(form=>form.checkValidity()),false);
    await agent.fill(metadata.agent);await expect(permissions).toBeDisabled();
    assert.equal(await form.evaluate(form=>form.checkValidity()),true);
    await agent.fill('');await expect(permissions).toBeEnabled();assert.equal(await form.evaluate(form=>form.checkValidity()),false);
    await agent.fill(metadata.agent);await form.getByRole('button',{name:'Save profile',exact:true}).click();
    await expect.poll(()=>writes.length).toBe(1);assert.equal(writes[0].get('agent_id'),metadata.agent);
    assert.deepEqual(writes[0].getAll('permissions'),[metadata.fallback]);
  }finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
});
