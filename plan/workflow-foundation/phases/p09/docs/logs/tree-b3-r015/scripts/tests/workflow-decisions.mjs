// Actual Rust-rendered decision forms with the shipped HTMX, CSS and request guard.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const directory = process.env.WORKFLOW_DECISION_BROWSER_FIXTURES;
assert.ok(directory, 'Generate markup with WORKFLOW_DECISION_BROWSER_FIXTURES=<dir> cargo test --lib decision_browser_markup_and_closed_draft_recovery');
const fixtures = Object.fromEntries(['waiting','comment','closed'].map(name => [name,readFileSync(`${directory}/${name}.html`,'utf8')]));
const read = path => readFileSync(new URL(`../../${path}`, import.meta.url),'utf8');
const guard = read('src/adapters/http/pages/workflow_runs/mod.rs').split('const WORKFLOW_RUNS_SCRIPT: &str = r#"')[1].split('"#;')[0];

test('comment preserves the unfinished exact decision; queued submission settles without stale replacement', async () => {
    const pending = [];
    const received = [];
    const server = createServer(async (request,response) => {
        const url = request.url.split('?')[0];
        if (url.endsWith('.js')) {
            response.setHeader('Content-Type','text/javascript');
            return response.end(url.includes('htmx-') ? read('assets/htmx-2.0.4.min.js') : url.includes('theme') ? '' : guard);
        }
        if (url.endsWith('.css')) { response.setHeader('Content-Type','text/css'); return response.end(read('assets/app.css')); }
        if (request.method === 'POST') {
            const chunks=[];for await (const chunk of request) chunks.push(chunk);
            const body=new URLSearchParams(Buffer.concat(chunks).toString());
            received.push({url,body});pending.push(response);return;
        }
        response.setHeader('Content-Type','text/html');
        if (url.startsWith('/source-')) return response.end(readFileSync(`${directory}${url}.html`,'utf8'));
        response.end(fixtures.waiting);
    });
    await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
    const browser=await chromium.launch(process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH}:{});
    const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
    try {
        const colors=[];
        for (const theme of ['dark','light']) {
            await page.goto(`http://127.0.0.1:${server.address().port}/`);
            await page.evaluate(theme => document.documentElement.setAttribute('data-theme',theme),theme);
            await page.locator('[name="data:body"]').fill('Unfinished draft');
            await page.locator('[name=feedback]').fill('Keep this feedback');
            await page.locator('[name="data:body"]').focus();await page.keyboard.press('Tab');
            await expect(page.getByText('Data schema',{exact:true})).toBeFocused();
            await page.keyboard.press('Tab');
            await expect(page.locator('[name=feedback]')).toBeFocused();
            colors.push(await page.locator('[name=feedback]').evaluate(el=>getComputedStyle(el).color));
            const key=await page.locator('form[hx-preserve] [name=command_key]').inputValue();
            await page.getByRole('textbox',{name:'Comment',exact:true}).fill('Discussion only');
            await page.getByRole('button',{name:'Add comment',exact:true}).click();
            await expect.poll(()=>pending.length).toBe(1);
            await expect(page.getByRole('button',{name:'Submit decision: accept',exact:true})).toBeDisabled();
            pending.shift().end(fixtures.comment);
            await expect(page.getByRole('status').filter({hasText:'Comment posted'})).toBeVisible();
            await expect(page.locator('[name="data:body"]')).toHaveValue('Unfinished draft');
            await expect(page.locator('[name=feedback]')).toHaveValue('Keep this feedback');
            await expect(page.locator('form[hx-preserve] [name=command_key]')).toHaveValue(key);
            await expect(page.getByRole('button',{name:'Submit decision: accept',exact:true})).toBeEnabled();
            await page.getByRole('button',{name:'Submit decision: accept',exact:true}).focus();await page.keyboard.press('Enter');
            await expect.poll(()=>pending.length).toBe(1);
            assert.equal(received.at(-1).body.get('data:body'),'Unfinished draft');
            assert.equal(received.at(-1).body.get('choice'),'accept');
            pending.shift().end(fixtures.closed);
            await expect(page.getByText('Decision submitted',{exact:true})).toBeVisible();
            await expect(page.locator('#workflow-decision form')).toHaveCount(0);
            await expect(page.locator('#decision-shell')).toHaveCount(1);
            await expect(page.locator('#decision-draft-recovery')).toHaveCount(0);
            await expect(page.locator('#--htmx-preserve-pantry--')).toHaveCount(0);
        }
        // Another reviewer closes this decision while the comment is in flight. Recover
        // even edits typed after the request began, without suggesting the comment submitted them.
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await page.locator('[name="data:body"]').fill('Unsent body');
        await page.getByRole('textbox',{name:'Comment',exact:true}).fill('Posting comment');
        await page.getByRole('button',{name:'Add comment',exact:true}).click();
        await expect.poll(()=>pending.length).toBe(1);
        await page.locator('[name="data:body"]').fill('Later edit <script>');
        await page.locator('[name=feedback]').fill('Latest feedback');
        pending.shift().end(fixtures.closed.replace('Decision submitted','Comment posted'));
        await expect(page.locator('#decision-draft-recovery')).toContainText('Later edit <script>');
        await expect(page.locator('#decision-draft-recovery')).toContainText('Latest feedback');
        await expect(page.locator('#decision-draft-recovery')).toContainText('Posting a comment did not submit this input.');
        await expect(page.locator('#decision-draft-recovery script')).toHaveCount(0);
        await expect(page.locator('#workflow-decision form')).toHaveCount(0);
        // Programmatically overlap requests, as can happen before a disabled state is painted.
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await page.locator('[name="data:body"]').fill('Queued draft');
        await page.getByRole('textbox',{name:'Comment',exact:true}).fill('Concurrent comment');
        await page.getByRole('button',{name:'Add comment',exact:true}).click();
        await expect.poll(()=>pending.length).toBe(1);
        await page.getByRole('button',{name:'Submit decision: accept',exact:true}).evaluate(button=>{
            button.disabled=false;button.click();
        });
        await page.waitForTimeout(100);
        assert.equal(pending.length,1,'the second write waits for the comment');
        pending.shift().end(fixtures.comment);
        await expect.poll(()=>pending.length).toBe(1);
        assert.equal(received.at(-1).body.get('data:body'),'Queued draft');
        assert.equal(received.at(-1).body.get('choice'),'accept');
        pending.shift().end(fixtures.closed);
        await expect(page.getByText('Decision submitted',{exact:true})).toBeVisible();
        await expect(page.locator('#workflow-decision form')).toHaveCount(0);
        assert.notEqual(colors[0],colors[1],'real text colors follow both themes');
        for (const [name,body] of [['old','Source message 209 <script>'],['retained','Source message 5 <script>'],['note','Source internal note']]) {
            await page.goto(`http://127.0.0.1:${server.address().port}/source-${name}`);
            await expect(page.locator('#source-message')).toBeFocused();
            await expect(page.locator('#source-message')).toContainText(body);
            await expect(page.locator('#source-message')).toBeInViewport();
            await expect(page.locator('#message-scroll')).toHaveCount(0);
            await expect(page.locator('#source-message [hx-target], #source-message button, #source-message form')).toHaveCount(0);
            await page.getByRole('link',{name:'Open latest conversation'}).click();
            await expect(page).toHaveURL(/\/ui\?company_id=/);
        }
        assert.deepEqual(errors,[]);
    } finally {
        await browser.close();for(const response of pending) response.destroy();
        server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
    }
});
