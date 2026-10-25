// Browser regression for overlapping run reads/writes, using shipped HTMX and the run guard.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const read = path => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');
const script = read('src/adapters/http/pages/workflow_runs/mod.rs')
    .split('const WORKFLOW_RUNS_SCRIPT: &str = r#"')[1].split('"#;')[0];
const assets = { '/htmx.js': read('assets/htmx-2.0.4.min.js'), '/runs.js': script,
    '/sse.js': read('assets/htmx-ext-sse-2.2.3.js') };
const body = (state, child = 'queued') => `<div id="run-body"><span hidden hx-get="/body" hx-trigger="sse:resync, sse:run-child"
    hx-target="#run-body" hx-swap="outerHTML" hx-sync="#run-body:replace"></span>
    <p id="state">${state}</p><p id="child-state">${child}</p><details id="run-step-finished" hx-preserve="true"><summary>Finished step</summary><p>Output</p></details>
    <details id="run-reconcile" hx-preserve="true"><summary>Record what happened</summary><textarea name="note"></textarea></details>
    <form hx-post="/cancel" hx-target="#run-pane" hx-swap="outerHTML"><button>Cancel</button></form></div>`;
const pane = state => `<section id="run-pane">${body(state)}</section>`;

async function harness() {
    const pending = [];
    const streams = new Set();
    let connections = 0;
    const server = createServer((request, response) => {
        if (assets[request.url]) {
            response.setHeader('Content-Type', 'text/javascript');
            return response.end(assets[request.url]);
        }
        if (request.url === '/events') {
            response.writeHead(200, { 'Content-Type': 'text/event-stream' });
            response.write('retry: 100\n\nevent: resync\ndata: {}\n\n');
            connections++;
            streams.add(response);
            response.on('close', () => streams.delete(response));
            return;
        }
        if (request.url === '/body') { pending.push(response); return; }
        response.setHeader('Content-Type', 'text/html');
        if (request.url === '/cancel') return response.end(pane('cancelled'));
        if (request.url === '/other') return response.end(pane('other run'));
        response.end(`<!doctype html><html><head><script src="/htmx.js"></script>
            <script src="/sse.js"></script><script defer src="/runs.js"></script></head><body>
            <main hx-ext="sse" sse-connect="/events">${pane('waiting')}
            <a id="other" hx-get="/other" hx-target="#run-pane" hx-swap="outerHTML" hx-sync="#run-pane:replace">Other run</a>
            </main></body></html>`);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await expect.poll(() => pending.length).toBe(1);
    return { page, pending, streams, connections: () => connections,
        async respond(state, child) {
            await expect.poll(() => pending.length).toBeGreaterThan(0);
            const response = pending.shift();
            response.end(body(state, child));
            await page.waitForTimeout(150);
        },
        async close() {
            await browser.close();
            for (const response of [...streams, ...pending]) response.destroy();
            server.closeAllConnections();
            await new Promise(resolve => server.close(resolve));
            assert.deepEqual(errors, []);
        },
    };
}

test('a late body read cannot remove preserved steps after a cancel or navigation', async () => {
    for (const action of ['Cancel', 'Other run']) {
        const h = await harness();
        try {
            // The initial resync GET is held while another response replaces its entire pane.
            await h.page.getByText(action, { exact: true }).click();
            const state = action === 'Cancel' ? 'cancelled' : 'other run';
            await expect(h.page.locator('#state')).toHaveText(state);
            await h.respond('stale waiting');
            await expect(h.page.locator('#state')).toHaveText(state);
            await expect(h.page.locator('#run-step-finished')).toHaveCount(1);
            await expect(h.page.locator('#run-reconcile')).toHaveCount(1);
            assert.equal(h.connections(), 1, 'the event-stream owner remains mounted');
            await expect(h.page.locator('#--htmx-preserve-pantry--')).toHaveCount(0);
        } finally { await h.close(); }
    }
});

test('an actual SSE disconnect reconciles and preserves an unfinished note and disclosure', async () => {
    const h = await harness();
    try {
        await h.respond('waiting');
        await h.page.getByText('Record what happened', { exact: true }).click();
        await h.page.locator('[name=note]').fill('Vendor has not confirmed the result.');
        for (const response of h.streams) response.destroy();
        await expect.poll(h.connections).toBe(2);
        await h.respond('waiting after reconnect');
        await expect(h.page.locator('#state')).toHaveText('waiting after reconnect');
        await expect(h.page.locator('[name=note]')).toHaveValue('Vendor has not confirmed the result.');
        await expect(h.page.locator('#run-reconcile')).toHaveAttribute('open', '');
    } finally { await h.close(); }
});


test('a waiting parent refreshes a child badge on the child wakeup without reconnecting', async () => {
    const h = await harness();
    try {
        await h.respond('waiting for child');
        await expect(h.page.locator('#child-state')).toHaveText('queued');
        for (const response of h.streams) response.write('event: run-child\ndata: child\n\n');
        await h.respond('waiting for child', 'waiting for a decision');
        await expect(h.page.locator('#state')).toHaveText('waiting for child');
        await expect(h.page.locator('#child-state')).toHaveText('waiting for a decision');
        assert.equal(h.connections(), 1);
    } finally { await h.close(); }
});
