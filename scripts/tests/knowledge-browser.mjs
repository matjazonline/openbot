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
  let release;
  let ready = 0;
  const pending = new Set();
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    if (request.method === 'POST') {
      submissions++;
      for await (const _ of request) { /* drain the actual multipart/form body */ }
      pending.add(response);
      await new Promise(resolve => { release = resolve; ready = submissions; });
      pending.delete(response);
      response.writeHead(400, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture('knowledge.html'));
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
    response.writeHead(200, { 'content-type': 'text/html', 'content-security-policy': csp }).end(fixture('knowledge.html'));
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
    await expect(page.locator('main h2')).toHaveText('<img onerror=alert(1)>.md');
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
  } finally {
    release?.();for (const response of pending) response.destroy();await browser?.close();await new Promise(resolve => server.close(resolve));
  }
});
