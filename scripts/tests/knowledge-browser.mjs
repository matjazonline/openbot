import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const root = new URL('../../', import.meta.url);
const fixture = name => readFileSync(`${process.env.KNOWLEDGE_BROWSER_FIXTURES}/${name}`);
const cspSource = readFileSync(new URL('src/adapters/http/security.rs', root), 'utf8');
const csp = cspSource.match(/const CSP: &str = "([^"]+)";/)[1];
const asset = name => readFileSync(new URL(`assets/${name}`, root));

test('library native forms validate before showing progress, prevent duplicate writes, and keep both themes readable', async () => {
  let submissions = 0;
  const acceptedBodies = [];
  let release;
  let ready = 0;
  const pending = new Set();
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (request.method === 'POST') {
      submissions++;
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      acceptedBodies.push(Buffer.concat(chunks).toString('utf8'));
      pending.add(response);
      await new Promise(resolve => { release = resolve; ready = submissions; });
      pending.delete(response);
      response.writeHead(400, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture(url.pathname.endsWith('/move') ? 'knowledge-move.html' : 'knowledge.html'));
      return;
    }
    if (url.pathname === '/assets/app.js' || url.pathname === '/assets/theme-init.js') {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(fixture(url.pathname.endsWith('app.js') ? 'app.js' : 'theme-init.js'));return;
    }
    if (url.pathname === '/assets/app.css') { response.writeHead(200, { 'content-type': 'text/css' }).end(asset('app.css'));return; }
    if (['/assets/htmx-2.0.4.min.js', '/assets/htmx-ext-sse-2.2.3.js'].includes(url.pathname)) {
      response.writeHead(200, { 'content-type': 'text/javascript' }).end(asset(url.pathname.split('/').at(-1)));return;
    }
    if (url.pathname.startsWith('/assets/') || url.pathname === '/favicon.ico') { response.writeHead(204).end();return; }
    response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture(url.pathname.endsWith('/move') ? 'knowledge-move.html' : 'knowledge.html'));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch();
    const page = await browser.newPage();
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    const snapshots=[];
    await page.exposeFunction('recordKnowledgeSubmit',snapshot=>snapshots.push(snapshot));
    const watchSubmit=()=>page.evaluate(()=>document.addEventListener('submit',event=>{
      const form=event.target.closest('form[data-submit="busy-once"]');if(!form)return;
      window.recordKnowledgeSubmit({busy:form.getAttribute('aria-busy'),label:form.querySelector('[data-label]').textContent,progressVisible:getComputedStyle(form.querySelector('[data-progress]')).display!=='none',duplicatePrevented:event.defaultPrevented});
    }));
    await page.goto(`http://127.0.0.1:${server.address().port}/ui/knowledge`);
    await expect(page.getByRole('heading', { name: 'Root', level: 2, exact: true })).toBeVisible();
    await expect(page.getByRole('heading', { name: '<img onerror=alert(1)>.md', level: 3, exact: true })).toBeVisible();
    assert.equal(await page.locator('main img').count(), 0);
    await watchSubmit();
    const upload = page.locator('form[enctype="multipart/form-data"]');
    await upload.getByRole('button', { name: 'Upload and index' }).click();
    assert.equal(submissions, 0);
    assert.equal(snapshots.length,0);
    assert.equal(await upload.getAttribute('aria-busy'), null);
    for (const theme of ['light', 'dark']) {
      await page.evaluate(theme => document.documentElement.setAttribute('data-theme', theme), theme);
      const styles = await page.locator('#knowledge-files').evaluate(input => {
        const style = getComputedStyle(input);
        return { radius: style.borderRadius, outline: style.outlineOffset, background: style.backgroundColor, foreground: style.color, colorScheme: getComputedStyle(document.documentElement).colorScheme };
      });
      assert.notEqual(styles.foreground, styles.background, theme);
      assert.notEqual(styles.radius, '0px', theme);
      assert.equal(styles.outline, '-1px', theme);
    }
    await page.locator('#knowledge-files').setInputFiles({ name: '0_notes.md', mimeType: 'text/markdown', buffer: Buffer.from('public notes') });
    const department = upload.getByLabel('Upload department', { exact: true });
    await department.selectOption('');
    await upload.getByRole('button', { name: 'Upload and index' }).click();
    assert.equal(submissions, 0);
    assert.equal(snapshots.length, 0);
    assert.equal(await upload.getAttribute('aria-busy'), null);
    await department.selectOption('00000000-0000-0000-0000-000000000004');
    assert.deepEqual(errors,[]);
    const navigation = page.waitForNavigation().catch(error=>error);
    await upload.getByRole('button', { name: 'Upload and index' }).click({ noWaitAfter: true });
    // Native navigation is pending: observe the actual accepted submit synchronously in-page.
    // Locator assertions wait for navigation and cannot inspect that interval.
    await expect.poll(()=>snapshots.length).toBe(1);
    assert.deepEqual(snapshots[0],{busy:'true',label:'Indexing…',progressVisible:true,duplicatePrevented:false});
    await page.evaluate(()=>document.querySelector('form[enctype="multipart/form-data"]').requestSubmit());
    await expect.poll(()=>snapshots.length).toBe(2);assert.equal(snapshots[1].duplicatePrevented,true);
    await expect.poll(() => ready).toBe(1);
    assert.ok(acceptedBodies[0].includes('name="unit_id"'));
    assert.ok(acceptedBodies[0].indexOf('name="unit_id"') < acceptedBodies[0].indexOf('name="files"'));
    assert.ok(acceptedBodies[0].includes('00000000-0000-0000-0000-000000000004'));
    assert.equal(submissions, 1);
    release();
    const navigationResult=await navigation;if(navigationResult instanceof Error) throw navigationResult;
    await expect(page.locator('form[enctype="multipart/form-data"]')).not.toHaveAttribute('aria-busy', 'true');
    await watchSubmit();
    const deletion = page.locator('form[action*="/delete"]');
    const deleted = page.waitForNavigation().catch(error=>error);
    await deletion.getByRole('button', { name: /Delete/ }).click({ noWaitAfter: true });
    await expect.poll(()=>snapshots.length).toBe(3);
    assert.deepEqual(snapshots[2],{busy:'true',label:'Deleting…',progressVisible:true,duplicatePrevented:false});
    await expect.poll(() => ready).toBe(2);
    release();const deletionResult=await deleted;if(deletionResult instanceof Error) throw deletionResult;
    assert.equal(submissions, 2);
    await page.getByRole('link', { name: 'Move document', exact: true }).click();
    await watchSubmit();
    const moving = page.locator('form[action*="/move"]');
    await moving.getByLabel('Move all levels to', { exact: true }).selectOption('00000000-0000-0000-0000-000000000005');
    const moved = page.waitForNavigation();
    await moving.getByRole('button', { name: 'Move document', exact: true }).click({ noWaitAfter: true });
    await expect.poll(() => snapshots.length).toBe(4);
    assert.deepEqual(snapshots[3], { busy: 'true', label: 'Moving…', progressVisible: true, duplicatePrevented: false });
    await expect.poll(() => ready).toBe(3);
    assert.ok(acceptedBodies[2].includes('target_unit_id=00000000-0000-0000-0000-000000000005'));
    release(); await moved;
    await expect(page.locator('form[action*="/move"]')).not.toHaveAttribute('aria-busy', 'true');
    await page.getByRole('link', { name: 'Back to library', exact: true }).click();
    await watchSubmit();
    const searching = page.locator('form[action*="/search"]');
    const question = '😀'.repeat(1000);
    await searching.getByLabel('Search readable knowledge', { exact: true }).fill(question);
    const searched = page.waitForNavigation();
    await searching.getByRole('button', { name: 'Search knowledge', exact: true }).click({ noWaitAfter: true });
    await expect.poll(() => snapshots.length).toBe(5);
    assert.deepEqual(snapshots[4], { busy: 'true', label: 'Searching…', progressVisible: true, duplicatePrevented: false });
    await expect.poll(() => ready).toBe(4);
    assert.equal(new URLSearchParams(acceptedBodies[3]).get('query'), question);
    assert.ok(Buffer.byteLength(acceptedBodies[3]) > 4096 && Buffer.byteLength(acceptedBodies[3]) < 16384);
    release(); await searched;
    await expect(page.locator('form[action*="/search"]')).not.toHaveAttribute('aria-busy', 'true');
    assert.equal(submissions, 4);
    assert.deepEqual(errors, []);
  } finally {
    release?.();for (const response of pending) response.destroy();await browser?.close();await new Promise(resolve => server.close(resolve));
  }
});
