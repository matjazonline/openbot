// Real HTTP/SSE, shipped HTMX, and a browser: no mocked EventSource or swap callbacks.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const read = path => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');
const countsScript = read('src/adapters/http/pages/task_counts.rs')
    .split('const TASK_COUNTS_SCRIPT: &str = r#"')[1].split('"#;')[0];
const navigationScript = read('src/adapters/http/pages/mailbox.rs')
    .split('const EVENT_DELEGATION_SCRIPT: &str = r##"')[1].split('document.addEventListener')[0];
const assets = {
    '/htmx.js': read('assets/htmx-2.0.4.min.js'),
    '/sse.js': read('assets/htmx-ext-sse-2.2.3.js'),
    '/counts.js': countsScript,
    '/navigation.js': navigationScript,
    '/app.css': read('assets/app.css'),
};
let browser;
before(async () => {
    browser = await chromium.launch({
        ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {}),
    });
});
after(async () => { await browser?.close(); });

const slot = key => `<span data-task-counts="${key}"></span>`;
const pane = (kind, id) => `<section id="${kind}-pane" data-selected="${id}">${slot(`${kind}:${id}`)}</section>`;
const menu = (kind, oob = '') => `<ul id="${kind}-menu" ${oob}>${['a', 'b'].map(id => `<li>
    <button id="${kind}-${id}" hx-get="/pane/${kind}/${id}" hx-target="#${kind}-pane"
        hx-swap="outerHTML" hx-sync="#${kind}-pane:replace">${id}${slot(`${kind}:${id}`)}</button>
</li>`).join('')}</ul>`;
const snapshot = entries => Object.entries({ company: 'No open tasks', ...entries }).map(([key, text]) =>
    `<template data-task-counts-key="${key}">${text}</template>`).join('');

async function harness(kind) {
    const streams = new Set();
    const pending = new Map();
    let connections = 0;
    const server = createServer((request, response) => {
        const path = new URL(request.url, 'http://localhost').pathname;
        if (assets[path]) {
            response.setHeader('Content-Type', path.endsWith('.css') ? 'text/css' : 'text/javascript');
            return response.end(assets[path]);
        }
        if (path === '/ui/task-counts/events') {
            response.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
            response.write('retry: 100\n\n');
            streams.add(response);
            connections++;
            response.on('close', () => streams.delete(response));
            return;
        }
        if (path.startsWith('/pane/')) {
            pending.set(path, response);
            return;
        }
        response.setHeader('Content-Type', 'text/html');
        if (path.startsWith('/list/')) return response.end(menu(path.split('/')[2], 'hx-swap-oob="outerHTML"'));
        if (path === '/away') return response.end('<!doctype html><p>Elsewhere</p>');
        response.end(`<!doctype html><html data-theme="light"><head>
            <link rel="stylesheet" href="/app.css">
            <script src="/htmx.js"></script><script src="/sse.js"></script>
            <script defer src="/counts.js"></script><script defer src="/navigation.js"></script>
            </head><body><aside>Open tasks ${slot('company')}
            <div hidden data-task-counts-source hx-ext="sse" sse-connect="/ui/task-counts/events?company_id=test"
                sse-swap="task-counts" hx-swap="innerHTML"></div>${menu(kind)}</aside>${pane(kind, 'a')}
            <button id="list" hx-get="/list/${kind}" hx-swap="none">Refresh list</button>
            <a id="away" href="/away">Away</a></body></html>`);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/ui/${kind}s`);
    await expect.poll(() => streams.size).toBe(1);
    return {
        page, streams, pending, connections: () => connections,
        async send(entries) {
            for (const response of streams) response.write(`event: task-counts\ndata: ${snapshot(entries)}\n\n`);
            await expect(page.locator('[data-task-counts="company"]')).toHaveText(entries.company ?? 'No open tasks');
        },
        async finish(id) {
            const key = `/pane/${kind}/${id}`;
            await expect.poll(() => pending.has(key)).toBe(true);
            pending.get(key).end(pane(kind, id));
            pending.delete(key);
        },
        async close() {
            await page.close();
            for (const response of streams) response.end();
            for (const response of pending.values()) response.end();
            server.closeAllConnections();
            await new Promise(resolve => server.close(resolve));
            assert.deepEqual(errors, []);
        },
    };
}

for (const kind of ['agent', 'channel']) {
    test(`${kind}: snapshot before pane, OOB list, competing reads, reconnect and navigation`, async () => {
        const h = await harness(kind);
        try {
            await expect(h.page.locator('[data-task-counts="company"]')).toBeEmpty();
            await h.send({ company: '3 pending', [`${kind}:a`]: '1 pending', [`${kind}:b`]: '2 pending' });
            await h.page.click(`#${kind}-b`);
            await h.send({ company: '4 pending', [`${kind}:a`]: '1 pending', [`${kind}:b`]: '3 pending' });
            await h.finish('b');
            await expect(h.page.locator(`#${kind}-pane [data-task-counts]`)).toHaveText('3 pending');
            await h.page.click('#list');
            await expect(h.page.locator(`#${kind}-menu [data-task-counts="${kind}:a"]`)).toHaveText('1 pending');
            assert.equal(h.connections(), 1);
            // Complete the newest response first. The aborted older read must not replace it.
            await h.page.click(`#${kind}-a`);
            await expect.poll(() => h.pending.has(`/pane/${kind}/a`)).toBe(true);
            await h.page.click(`#${kind}-b`);
            await h.finish('b');
            await h.finish('a');
            await expect(h.page.locator(`#${kind}-pane`)).toHaveAttribute('data-selected', 'b');
            await expect(h.page.locator(`#${kind}-pane [data-task-counts]`)).toHaveText('3 pending');
            await h.send({});
            await expect(h.page.locator(`#${kind}-pane [data-task-counts]`)).toBeEmpty();
            await expect(h.page.locator(`#${kind}-menu [data-task-counts="${kind}:a"]`)).toBeEmpty();
            for (const response of h.streams) response.end();
            await expect.poll(h.connections, { timeout: 15_000 }).toBe(2);
            await expect.poll(() => h.streams.size).toBe(1);
            await h.send({ company: '4 active', [`${kind}:b`]: '4 active' });
            await expect(h.page.locator(`#${kind}-pane [data-task-counts]`)).toHaveText('4 active');
            await h.page.click('#away');
            await expect.poll(() => h.streams.size).toBe(0);
        } finally { await h.close(); }
    });
    test(`${kind}: pane before first snapshot stays uninitialized and then hydrates`, async () => {
        const h = await harness(kind);
        try {
            await h.page.click(`#${kind}-b`);
            await h.finish('b');
            await expect(h.page.locator(`#${kind}-pane`)).toHaveAttribute('data-selected', 'b');
            await expect(h.page.locator(`#${kind}-pane [data-task-counts]`)).toBeEmpty();
            await h.send({ company: '2 waiting', [`${kind}:b`]: '2 waiting' });
            await expect(h.page.locator(`#${kind}-pane [data-task-counts]`)).toHaveText('2 waiting');
            assert.equal(h.connections(), 1);
        } finally { await h.close(); }
    });
}
