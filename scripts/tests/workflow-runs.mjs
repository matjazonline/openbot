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

// The board's region as `run_board` renders it (`board_view_tests.rs` pins these attributes): a
// reload leaf on the toolbar's replace group that reads whatever the toolbar's form holds, a
// filter form that replaces the region, and a card that opens the pane. The server answers what
// it is asked, so the columns name the window and agent the request carried.
const options = (values, chosen) => values.map(value =>
    `<option value="${value}"${value === chosen ? ' selected' : ''}>${value || 'any'}</option>`).join('');
const boardRegion = (window, agent = '', version = 0) => `<div id="run-board">
    <span hidden data-board-wake hx-get="/board" hx-vals='{"reload": "columns"}' hx-trigger="sse:runs, sse:resync, sse:tick, run-selected from:body, board-wake"
        hx-target="#run-board-columns" hx-select="#run-board-columns > section" hx-swap="innerHTML"
        hx-sync="#run-board:replace" hx-include="#run-selected, #run-board-filters"></span>
    <form id="run-board-filters" hx-get="/board" hx-target="#run-board" hx-swap="outerHTML" hx-trigger="change"
        hx-sync="#run-board:replace" hx-include="#run-selected" data-busy="#run-board">
        <input type="hidden" name="part" value="columns">
        <select name="agent_id">${options(['', 'a1'], agent)}</select>
        <select id="run-board-window" name="window">${options(['day', 'week', 'month'], window)}</select>
    </form>
    <div id="run-board-columns"><section><p id="columns-window">${window}</p><p id="columns-agent">${agent}</p><p id="columns-version">${version}</p>
        <a id="card" href="/?run_id=r1" hx-get="/pane" hx-target="#run-pane" hx-swap="outerHTML" hx-sync="#run-pane:replace"
            hx-push-url="/?run_id=r1" data-busy="#run-pane">Card</a></section></div></div>`;

/** `sse`: the page owns a run event stream; `live`: the server answers every board read with the
 * `state.version` it saw when the read arrived, after `delay` ms, recording when each read arrived
 * and whether it was aborted. */
async function boardHarness({ sse = false, live = false, delay = 0 } = {}) {
    const reads = [];
    const panes = [];
    const streams = new Set();
    const state = { version: 0 };
    const server = createServer((request, response) => {
        const url = new URL(request.url, 'http://board');
        if (assets[url.pathname]) {
            response.setHeader('Content-Type', 'text/javascript');
            return response.end(assets[url.pathname]);
        }
        if (url.pathname === '/events') {
            response.writeHead(200, { 'Content-Type': 'text/event-stream' });
            response.write('retry: 100\n\nevent: resync\ndata: {}\n\n');
            streams.add(response);
            response.on('close', () => streams.delete(response));
            return;
        }
        response.setHeader('Content-Type', 'text/html');
        if (url.pathname === '/board') {
            const read = { window: url.searchParams.get('window'), agent: url.searchParams.get('agent_id') || '',
                part: url.searchParams.getAll('part'), reload: url.searchParams.get('reload'),
                at: Date.now(), response, aborted: false };
            request.on('close', () => { read.aborted = !response.writableEnded; });
            reads.push(read);
            if (live) {
                const seen = state.version;
                setTimeout(() => {
                    if (!read.aborted && !response.writableEnded) response.end(boardRegion(read.window, read.agent, seen));
                }, delay);
            }
            return;
        }
        if (url.pathname === '/pane') { panes.push(response); return; }
        const owner = sse ? ' hx-ext="sse" sse-connect="/events"' : '';
        response.end(`<!doctype html><html><head><script src="/htmx.js"></script><script src="/sse.js"></script><script defer src="/runs.js"></script></head>
            <body><main${owner}>${boardRegion('week')}<div id="run-pane"></div></main></body></html>`);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.waitForFunction(() => window.htmx);
    return { page, reads, state,
        /** Send `name` on every open stream. */
        wake(name = 'runs') {
            for (const response of streams) response.write(`event: ${name}\ndata: {}\n\n`);
        },
        /** Answer the oldest held read that is still open with the region for its own window. */
        async answer(read, version = 0) {
            if (!read.aborted && !read.response.writableEnded) read.response.end(boardRegion(read.window, read.agent, version));
            await page.waitForTimeout(150);
        },
        /** Answer the held pane read, telling the board another run is open. */
        async openPane() {
            await expect.poll(() => panes.length).toBeGreaterThan(0);
            const response = panes.shift();
            response.setHeader('HX-Trigger-After-Settle', 'run-selected');
            response.end('<section id="run-pane"><input type="hidden" id="run-selected" name="run_id" value="r1"><p id="pane">r1</p></section>');
        },
        async close() {
            await browser.close();
            for (const read of reads) read.response.destroy();
            for (const response of [...panes, ...streams]) response.destroy();
            server.closeAllConnections();
            await new Promise(resolve => server.close(resolve));
            assert.deepEqual(errors, []);
        },
    };
}

test('board: two window reads completing out of order show the later choice', async () => {
    const h = await boardHarness();
    try {
        await h.page.locator('#run-board-window').selectOption('day');
        await expect.poll(() => h.reads.length).toBe(1);
        await expect(h.page.locator('#run-board')).toHaveAttribute('aria-busy', 'true');
        await h.page.locator('#run-board-window').selectOption('month');
        await expect.poll(() => h.reads.length).toBe(2);
        await h.answer(h.reads[1]);
        await h.answer(h.reads[0]);
        await expect(h.page.locator('#columns-window')).toHaveText('month');
        await expect(h.page.locator('#run-board-window')).toHaveValue('month');
        await expect(h.page.locator('[aria-busy]')).toHaveCount(0);
    } finally { await h.close(); }
});

test('board: opening a run while a filter read is in flight keeps the chosen filters', async () => {
    const h = await boardHarness();
    try {
        await h.page.locator('[name=agent_id]').selectOption('a1');
        await expect.poll(() => h.reads.length).toBe(1);
        await h.answer(h.reads[0]);
        await h.page.locator('#run-board-window').selectOption('day');
        await expect.poll(() => h.reads.length).toBe(2);
        await h.page.locator('#card').click();
        await expect(h.page.locator('#card')).toHaveAttribute('aria-busy', 'true');
        await h.openPane();
        // The pane's settle wakes the leaf; it waits for the filter read rather than cancel it,
        // then reads what the toolbar holds now, once.
        await h.page.waitForTimeout(300);
        assert.equal(h.reads.length, 2, 'the filter read is not cancelled');
        await h.answer(h.reads[1]);
        await expect.poll(() => h.reads.length).toBe(3);
        assert.deepEqual([h.reads[2].window, h.reads[2].agent, h.reads[2].part], ['day', 'a1', ['columns']]);
        for (const read of [...h.reads]) await h.answer(read);
        await expect(h.page.locator('#columns-window')).toHaveText('day');
        await expect(h.page.locator('#columns-agent')).toHaveText('a1');
        await expect(h.page.locator('#run-board-window')).toHaveValue('day');
        await expect(h.page.locator('#pane')).toHaveText('r1');
        await expect(h.page.locator('[aria-busy]')).toHaveCount(0);
        // Back restores the snapshot taken before the pane swapped in, with no busy mark in it.
        await h.page.goBack();
        await expect(h.page.locator('#run-board')).toHaveCount(1);
        await expect(h.page.locator('[aria-busy]')).toHaveCount(0);
    } finally { await h.close(); }
});

// The list's region as `run_list` renders it (`board_view_tests.rs` pins the leaf): a reload leaf
// and a filter form on one replace group, the leaf reading the form as it is now.
const listRegion = workflow => `<div id="run-list">
    <span hidden hx-get="/list?part=entries" hx-trigger="run-selected from:body"
        hx-target="#run-list-entries" hx-swap="innerHTML" hx-sync="#run-list:replace"
        hx-include="#run-list-shown, #run-selected, #run-list-filters"></span>
    <input type="hidden" id="run-list-shown" name="shown" value="1">
    <form id="run-list-filters" hx-get="/list" hx-target="#run-list" hx-swap="outerHTML" hx-trigger="change"
        hx-sync="#run-list:replace" hx-include="#run-selected">
        <select name="workflow_id">${options(['', 'w1'], workflow)}</select>
    </form>
    <ul id="run-list-entries"><li id="entries-workflow">${workflow}</li></ul></div>`;

test('list: a wake-up during a filter read reads the new filter instead of undoing it', async () => {
    const reads = [];
    const server = createServer((request, response) => {
        const url = new URL(request.url, 'http://list');
        if (assets[url.pathname]) {
            response.setHeader('Content-Type', 'text/javascript');
            return response.end(assets[url.pathname]);
        }
        response.setHeader('Content-Type', 'text/html');
        if (url.pathname === '/list') {
            reads.push({ workflow: url.searchParams.get('workflow_id') || '',
                entries: url.searchParams.get('part') === 'entries', response });
            return;
        }
        response.end(`<!doctype html><html><head><script src="/htmx.js"></script><script defer src="/runs.js"></script></head>
            <body><main>${listRegion('')}</main></body></html>`);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const answer = async read => {
        if (!read.response.writableEnded) {
            read.response.end(read.entries ? `<li id="entries-workflow">${read.workflow}</li>` : listRegion(read.workflow));
        }
        await page.waitForTimeout(150);
    };
    try {
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await page.waitForFunction(() => window.htmx);
        await page.locator('[name=workflow_id]').selectOption('w1');
        await expect.poll(() => reads.length).toBe(1);
        // A run changes while the filter read is held: the wake-up replaces it.
        await page.evaluate(() => htmx.trigger(document.body, 'run-selected'));
        await expect.poll(() => reads.length).toBe(2);
        assert.deepEqual([reads[1].entries, reads[1].workflow], [true, 'w1']);
        for (const read of [...reads]) await answer(read);
        await expect(page.locator('#entries-workflow')).toHaveText('w1');
        await expect(page.locator('[name=workflow_id]')).toHaveValue('w1');
        // And the next wake-up keeps it.
        await page.evaluate(() => htmx.trigger(document.body, 'run-selected'));
        await expect.poll(() => reads.length).toBe(3);
        assert.equal(reads[2].workflow, 'w1');
        await answer(reads[2]);
        await expect(page.locator('#entries-workflow')).toHaveText('w1');
        assert.deepEqual(errors, []);
    } finally {
        await browser.close();
        for (const read of reads) read.response.destroy();
        server.closeAllConnections();
        await new Promise(resolve => server.close(resolve));
    }
});

test('board: a filter change while a wake-up read is in flight wins', async () => {
    const h = await boardHarness({ sse: true });
    try {
        // The stream's first `resync` wakes the leaf; its read is held.
        await expect.poll(() => h.reads.length).toBe(1);
        assert.equal(h.reads[0].reload, 'columns');
        await h.page.locator('#run-board-window').selectOption('day');
        await expect.poll(() => h.reads.length).toBe(2);
        h.state.version = 1;
        await h.answer(h.reads[1]);
        // The wake-up's late answer, for the old window, must not land on the new board.
        if (!h.reads[0].aborted) h.reads[0].response.end(boardRegion('week', '', 0));
        await h.page.waitForTimeout(200);
        await expect(h.page.locator('#columns-window')).toHaveText('day');
        await expect(h.page.locator('#run-board-window')).toHaveValue('day');
    } finally { await h.close(); }
});

test('board: a burst of run events reads at most once per two seconds and shows the final state', async () => {
    const h = await boardHarness({ sse: true, live: true });
    try {
        await expect.poll(() => h.reads.length).toBe(1);
        await h.page.waitForTimeout(2100);
        const start = Date.now();
        // Thirty changes in 1.5 s: one read at once, one after the burst.
        for (let version = 1; version <= 30; version++) {
            h.state.version = version;
            h.wake('runs');
            await h.page.waitForTimeout(50);
        }
        await expect(h.page.locator('#columns-version')).toHaveText('30', { timeout: 5000 });
        await h.page.waitForTimeout(2500);
        const burst = h.reads.filter(read => read.at >= start);
        assert.ok(burst.length <= 2, `${burst.length} reads for one burst`);
        assert.ok(burst.at(-1).at >= start + 1500, 'the last read follows the last event');
    } finally { await h.close(); }
});

test('board: constant change still reads every two seconds and never starves', async () => {
    const h = await boardHarness({ sse: true, live: true });
    try {
        await expect.poll(() => h.reads.length).toBe(1);
        await h.page.waitForTimeout(2100);
        const start = Date.now();
        for (let version = 1; version <= 60; version++) {
            h.state.version = version;
            h.wake(version % 2 ? 'runs' : 'tick');
            await h.page.waitForTimeout(100);
        }
        await h.page.waitForTimeout(2500);
        await expect(h.page.locator('#columns-version')).toHaveText('60');
        const reads = h.reads.filter(read => read.at >= start).map(read => read.at);
        // About six seconds of change and the trailing read: at least one read per interval.
        assert.ok(reads.length >= 3 && reads.length <= 5, `${reads.length} reads`);
        for (let i = 1; i < reads.length; i++) {
            assert.ok(reads[i] - reads[i - 1] >= 1900, `reads ${reads[i] - reads[i - 1]} ms apart`);
        }
    } finally { await h.close(); }
});

// The shell's phone-pane handler as `mailbox.rs` ships it: from `setRailOpen` to the end of the
// `htmx:beforeSwap` listener that moves the reader to a pane opened in `.ui-pane-detail`.
const shellPane = (() => {
    const mailbox = read('src/adapters/http/pages/mailbox.rs');
    const start = mailbox.indexOf('function setRailOpen(open)');
    const end = mailbox.indexOf('});', mailbox.indexOf("setMobilePane(empty ? 'list' : 'detail');")) + 3;
    return mailbox.slice(start, end);
})();

test('phone: a stream-driven or polled pane reload leaves the reader on the list; opening a run still moves them', async () => {
    const streams = new Set();
    let bodies = 0;
    let polls = 0;
    const server = createServer((request, response) => {
        const url = new URL(request.url, 'http://phone');
        if (url.pathname === '/shell.js') {
            response.setHeader('Content-Type', 'text/javascript');
            return response.end(shellPane);
        }
        if (assets[url.pathname]) {
            response.setHeader('Content-Type', 'text/javascript');
            return response.end(assets[url.pathname]);
        }
        if (url.pathname === '/events') {
            response.writeHead(200, { 'Content-Type': 'text/event-stream' });
            response.write('retry: 100\n\n');
            streams.add(response);
            response.on('close', () => streams.delete(response));
            return;
        }
        response.setHeader('Content-Type', 'text/html');
        if (url.pathname === '/body') { bodies++; return response.end(`<p id="body">body ${bodies}</p>`); }
        if (url.pathname === '/poll') {
            polls++;
            return response.end('<span hidden hx-get="/poll" hx-trigger="every 300ms" hx-target="#poll" hx-swap="innerHTML"></span>');
        }
        if (url.pathname === '/pane') return response.end(paneSection('r2'));
        response.end(`<!doctype html><html><head><script src="/htmx.js"></script><script src="/sse.js"></script></head>
            <body><main hx-ext="sse" sse-connect="/events">
            <div class="ui-pane-list"><a id="open" hx-get="/pane" hx-target="#run-pane" hx-swap="outerHTML">Open</a></div>
            ${paneSection('r1')}</main><script src="/shell.js"></script></body></html>`);
    });
    const paneSection = run => `<section id="run-pane" class="ui-pane-detail"><div id="run-body">
        <span hidden hx-get="/body" hx-trigger="sse:tick" hx-target="#run-body" hx-swap="innerHTML"></span><p>${run}</p></div>
        <div id="poll"><span hidden hx-get="/poll" hx-trigger="every 300ms" hx-target="#poll" hx-swap="innerHTML"></span></div></section>`;
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    const page = await browser.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    try {
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await expect.poll(() => streams.size).toBe(1);
        // A run is open (the deep link), and the reader goes back to the list.
        await expect.poll(() => page.evaluate(() => document.body.dataset.pane)).toBe('detail');
        await page.evaluate(() => { document.body.dataset.pane = 'list'; });
        for (const response of streams) response.write('event: tick\ndata: {}\n\n');
        await expect.poll(() => bodies).toBe(1);
        const polled = polls;
        await expect.poll(() => polls).toBeGreaterThan(polled + 1);
        assert.equal(await page.evaluate(() => document.body.dataset.pane), 'list', 'a wake-up or a poll is not navigation');
        // The reader opening a run is.
        await page.locator('#open').click();
        await expect.poll(() => page.evaluate(() => document.body.dataset.pane)).toBe('detail');
        assert.deepEqual(errors, []);
    } finally {
        await browser.close();
        for (const response of streams) response.destroy();
        server.closeAllConnections();
        await new Promise(resolve => server.close(resolve));
    }
});

test('board: reads slower than the interval never overlap and keep landing under constant change', async () => {
    const h = await boardHarness({ sse: true, live: true, delay: 2500 });
    try {
        await expect(h.page.locator('#columns-version')).toHaveText('0', { timeout: 5000 });
        const start = Date.now();
        let landed = 0;
        for (let version = 1; version <= 80; version++) {
            h.state.version = version;
            h.wake('runs');
            await h.page.waitForTimeout(100);
            if (Number(await h.page.locator('#columns-version').textContent()) > 0) landed++;
        }
        assert.ok(landed > 0, 'the board moved while runs kept changing');
        await expect(h.page.locator('#columns-version')).toHaveText('80', { timeout: 10000 });
        const reads = h.reads.filter(read => read.at >= start);
        assert.deepEqual(reads.filter(read => read.aborted).length, 0, 'no read cancelled another');
        for (let i = 1; i < reads.length; i++) {
            // Each read starts only after the previous one settled (2.5 s) and the interval passed.
            assert.ok(reads[i].at - reads[i - 1].at >= 4400, `reads ${reads[i].at - reads[i - 1].at} ms apart`);
        }
    } finally { await h.close(); }
});

test('board: a run change during a filter read is read again after it, on the new leaf', async () => {
    const h = await boardHarness({ sse: true });
    try {
        await expect.poll(() => h.reads.length).toBe(1);
        await h.answer(h.reads[0]);
        await h.page.waitForTimeout(2100);
        await h.page.locator('#run-board-window').selectOption('day');
        await expect.poll(() => h.reads.length).toBe(2);
        // The run changes while the filter read is out; the read had already taken its snapshot.
        h.wake('runs');
        await h.page.waitForTimeout(100);
        assert.equal(h.reads.length, 2, 'held while the board is being read');
        await h.answer(h.reads[1], 0);
        await expect.poll(() => h.reads.length, { timeout: 4000 }).toBe(3);
        assert.equal(h.reads[2].window, 'day');
        await h.answer(h.reads[2], 1);
        await expect(h.page.locator('#columns-version')).toHaveText('1');
        await expect(h.page.locator('#columns-window')).toHaveText('day');
    } finally { await h.close(); }
});

test('board: two quick filter changes do not leave the coalescer waiting for ever', async () => {
    const h = await boardHarness({ sse: true });
    try {
        await expect.poll(() => h.reads.length).toBe(1);
        await h.answer(h.reads[0]);
        await h.page.locator('#run-board-window').selectOption('day');
        await expect.poll(() => h.reads.length).toBe(2);
        await h.page.locator('#run-board-window').selectOption('month');
        await expect.poll(() => h.reads.length).toBe(3);
        await h.answer(h.reads[2]);
        await h.page.waitForTimeout(2100);
        h.wake('runs');
        await expect.poll(() => h.reads.length, { timeout: 4000 }).toBe(4);
        assert.equal(h.reads[3].window, 'month');
    } finally { await h.close(); }
});
