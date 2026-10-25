// Shipped HTMX/SSE: late admission attribution reconciles without replacing conversation state.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const assets = Object.fromEntries(['htmx-2.0.4.min.js', 'htmx-ext-sse-2.2.3.js'].map(name =>
    [`/${name}`, readFileSync(new URL(`../../assets/${name}`, import.meta.url), 'utf8')]));

test('late attribution and reconnect keep both replies, composer and stream owner', async () => {
    let admitted = false;
    let connections = 0;
    const streams = new Set();
    const activity = () => admitted ? '<ul><li>run first: working</li><li>run second: awaiting review</li></ul>' +
        '<span id="message-runs-question" hx-swap-oob="innerHTML">Started run first, run second</span>' : '';
    const send = response => response.write(`event: activity\ndata: ${activity()}\n\n`);
    const server = createServer((req, res) => {
        if (assets[req.url]) { res.setHeader('Content-Type', 'text/javascript'); return res.end(assets[req.url]); }
        if (req.url === '/events') {
            res.writeHead(200, {'Content-Type':'text/event-stream'});
            res.write('retry: 100\n\n'); connections++; streams.add(res);
            res.on('close', () => streams.delete(res)); send(res); return;
        }
        res.setHeader('Content-Type', 'text/html');
        res.end(`<!doctype html><html><head><script src="/htmx-2.0.4.min.js"></script>
            <script src="/htmx-ext-sse-2.2.3.js"></script></head><body>
            <section id="detail-pane" hx-ext="sse" sse-connect="/events">
            <div id="message-scroll" style="height:160px;overflow:auto">
                <div id="question">Question <span id="message-runs-question"></span></div>
                <div style="height:500px"></div><div id="reply-first">First reply: Written by run first</div>
                <div id="reply-second">Second reply: Written by run second</div>
            </div><div id="thread-activity" sse-swap="activity" hx-target="this" hx-swap="innerHTML"></div>
            <textarea aria-label="Reply"></textarea></section></body></html>`);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const browser = await chromium.launch(process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH} : {});
    const page = await browser.newPage();
    const errors = []; page.on('pageerror', e => errors.push(e.message));
    try {
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await expect.poll(() => connections).toBe(1);
        await page.getByRole('textbox', {name:'Reply'}).fill('Unsent response');
        await page.locator('#message-scroll').evaluate(el => { el.scrollTop = 100; });
        await page.locator('#question').evaluate(el => { el.dataset.retained = 'yes'; });
        admitted = true; for (const stream of streams) send(stream);
        await expect(page.locator('#message-runs-question')).toHaveText('Started run first, run second');
        assert.equal(connections, 1);
        for (const stream of streams) stream.destroy();
        await expect.poll(() => connections).toBe(2);
        await expect(page.locator('#thread-activity')).toContainText('run second: awaiting review');
        await expect(page.getByRole('textbox', {name:'Reply'})).toHaveValue('Unsent response');
        await expect(page.locator('#question')).toHaveAttribute('data-retained', 'yes');
        assert.equal(await page.locator('#message-scroll').evaluate(el => el.scrollTop), 100);
        await expect(page.locator('#reply-first')).toHaveCount(1);
        await expect(page.locator('#reply-second')).toHaveCount(1);
        assert.deepEqual(errors, []);
    } finally {
        await browser.close(); for (const stream of streams) stream.destroy();
        server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
    }
});
