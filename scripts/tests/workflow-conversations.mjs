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

const historyScript = readFileSync(new URL('../../src/adapters/http/pages/thread_activity.rs', import.meta.url), 'utf8')
    .split('const THREAD_HISTORY_SCRIPT: &str = r#"')[1].split('"#;')[0];

test('a reconnect backlog keeps an explicit 200-message live window and the composer', async () => {
    const streams = new Set();
    let connections = 0;
    const bubble = id => `<div class="chat" data-message-id="${id}"><p>Message ${id}</p><span id="message-runs-${id}"></span></div>`;
    const server = createServer((req, res) => {
        if (assets[req.url]) { res.setHeader('Content-Type','text/javascript'); return res.end(assets[req.url]); }
        if (req.url === '/history.js') { res.setHeader('Content-Type','text/javascript'); return res.end(historyScript); }
        if (req.url === '/events') {
            res.writeHead(200, {'Content-Type':'text/event-stream'}); res.write('retry: 100\n\n');
            connections++; streams.add(res); res.on('close', () => streams.delete(res));
            if (connections > 1) {
                // Admission happened while disconnected: question 2 is still mounted but outside
                // the database window. Its run is visible; the backlog then advances the DOM window.
                res.write('event: activity\ndata: <p>run old: queued</p>\n\n');
                // The database now has more than 200 messages beyond the disconnected cursor.
                // Each historical batch's own eligibility supplies its links before it is appended.
                for (let id = 202; id <= 411; id++) {
                    const content = bubble(id).replace(`id="message-runs-${id}">`,
                        `id="message-runs-${id}">Written by <a href="/run/${id}">run ${id}</a>`);
                    res.write(`id: ${id}\nevent: message\ndata: ${content}\n\n`);
                }
            }
            return;
        }
        res.setHeader('Content-Type','text/html');
        res.end(`<!doctype html><html><head><script src="/htmx-2.0.4.min.js"></script>
            <script src="/htmx-ext-sse-2.2.3.js"></script><script src="/history.js"></script></head><body>
            <section id="detail-pane" hx-ext="sse" sse-connect="/events">
            <p>Showing up to 200 newest messages. Older messages leave this live view.</p>
            <div id="message-scroll" data-history-limit="200" sse-swap="message" hx-swap="beforeend" style="height:200px;overflow:auto">
                ${Array.from({length:200}, (_, i) => bubble(i + 1)).join('')}
            </div><div id="thread-activity" sse-swap="activity" hx-swap="innerHTML"></div>
            <textarea aria-label="Reply"></textarea></section></body></html>`);
    });
    await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
    const browser = await chromium.launch(process.env.CHROME_PATH ? {executablePath:process.env.CHROME_PATH} : {});
    const page = await browser.newPage(); const errors = []; page.on('pageerror', e => errors.push(e.message));
    try {
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await expect.poll(() => connections).toBe(1);
        await page.getByRole('textbox',{name:'Reply'}).fill('Keep this unfinished response');
        await page.locator('#detail-pane').evaluate(el => {el.dataset.retained='yes';});
        await page.locator('#message-scroll').evaluate(el => { el.scrollTop = 1000; });
        const retained = page.locator('[data-message-id="101"]');
        const before = await retained.evaluate(el => el.getBoundingClientRect().top);
        for (const stream of streams) stream.write(`id: 201\nevent: message\ndata: ${bubble(201)}\n\n`);
        await expect(page.locator('[data-message-id="201"]')).toHaveCount(1);
        assert.ok(Math.abs(await retained.evaluate(el => el.getBoundingClientRect().top) - before) < 1,
            'trimming an earlier bubble retains the reading position');
        // While disconnected question 2 remains mounted outside the newest DB window.
        for (const stream of streams) stream.destroy();
        await expect.poll(() => connections).toBe(2);
        await expect(page.locator('.chat[data-message-id]')).toHaveCount(200);
        await expect(page.locator('.chat').first()).toHaveAttribute('data-message-id','212');
        await expect(page.locator('.chat').last()).toHaveAttribute('data-message-id','411');
        await expect(page.locator('#message-scroll a')).toHaveCount(200);
        // A delayed admission for the old question never resurrects it outside the bounded window.
        for (const stream of streams) stream.write('event: activity\ndata: <p>run old: queued</p><span id="message-runs-2" hx-swap-oob="innerHTML">Started run old</span>\n\n');
        await expect(page.locator('#thread-activity')).toContainText('run old: queued');
        await expect(page.locator('[data-message-id="2"]')).toHaveCount(0);
        await expect(page.locator('#detail-pane')).toHaveAttribute('data-retained','yes');
        await expect(page.getByRole('textbox',{name:'Reply'})).toHaveValue('Keep this unfinished response');
        assert.equal(connections,2); assert.deepEqual(errors,[]);
    } finally {
        await browser.close(); for (const stream of streams) stream.destroy();
        server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
    }
});
