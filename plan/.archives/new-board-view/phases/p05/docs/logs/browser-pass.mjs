// Throwaway served-page browser pass for new-board-view 05 (not committed). Usage:
//   CHROME_PATH=... node scripts/tests/.nbv-pass.mjs <scratch-dir>
import { chromium, expect } from '@playwright/test';
import { readFileSync, writeFileSync } from 'node:fs';

const dir = process.argv[2];
const seed = JSON.parse(readFileSync(`${dir}/seed.json`, 'utf8'));
const base = 'http://localhost:3001';
const c = seed.company;
const board = `${base}/ui/workflow-runs?company_id=${c}&view=board`;
const round = Number(process.env.NBV_ROUND || 0);
const results = [];
// A check returns its detail, or `{ notExercised: why }` when the pass could not reach it.
const check = async (name, fn) => {
    try {
        const detail = await fn();
        if (detail && detail.notExercised) results.push({ name, status: 'NOT EXERCISED', detail: detail.notExercised });
        else results.push({ name, status: 'PASS', detail: detail ?? '' });
    } catch (error) {
        results.push({ name, status: 'FAIL', detail: String(error.message || error).split('\n').slice(0, 4).join(' | ') });
    }
};
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH });
const session = async (user, viewport = { width: 1400, height: 900 }) => {
    const context = await browser.newContext({ storageState: `${dir}/${user}.json`, viewport });
    const page = await context.newPage();
    page.errors = [];
    page.on('pageerror', error => page.errors.push(error.message));
    // The run controls ask with hx-confirm; the reader says yes.
    page.on('dialog', dialog => dialog.accept());
    return { context, page };
};
// Change a toolbar control and wait for the board region it reads.
const choose = async (page, selector, value) => {
    const answered = page.waitForResponse(response => response.url().includes('/ui/workflow-runs?') && response.url().includes('part=columns'));
    await page.locator(selector).selectOption(value);
    await answered;
    await page.waitForTimeout(200);
};
const totals = page => page.$$eval('[data-state]', sections => sections.map(s => [s.dataset.state, Number(s.querySelector('[data-total]').textContent)]));

const owner = await session('owner');
const p = owner.page;
let ownerTotals;
await check('board: six columns in state order with totals', async () => {
    await p.goto(board);
    ownerTotals = await totals(p);
    const states = ownerTotals.map(([state]) => state);
    if (states.join() !== 'queued,running,waiting,succeeded,failed,cancelled') throw new Error(states.join());
    return JSON.stringify(ownerTotals);
});
await check('board: cut failed column says "Showing 50 of N" and opens the list with state', async () => {
    const cut = p.locator('[data-state="failed"] [data-cut]');
    const text = await cut.textContent();
    if (!/Showing 50 of 57/.test(text)) throw new Error(text);
    await cut.locator('a').click();
    await p.waitForURL(/state=failed/);
    if (!(await p.locator('#run-list').count())) throw new Error('no list');
    const selected = await p.locator('select[name=state]').inputValue();
    if (selected !== 'failed') throw new Error(`list state ${selected}`);
    return p.url();
});
await check('toggle: List ↔ Board keeps the scope', async () => {
    await p.goto(`${base}/ui/workflow-runs?company_id=${c}&workflow_id=${seed.mapper.workflow}`);
    await p.locator('nav[aria-label="Runs view"] >> text=Board').click();
    await p.waitForURL(/view=board/);
    if (!p.url().includes(`workflow_id=${seed.mapper.workflow}`)) throw new Error(p.url());
    await p.locator('nav[aria-label="Runs view"] >> text=List').click();
    await p.waitForURL(url => !String(url).includes('view=board'));
    if (!p.url().includes(`workflow_id=${seed.mapper.workflow}`)) throw new Error(p.url());
    return 'kept workflow_id both ways';
});
await check('filters: workflow, agent and window each narrow the board', async () => {
    await p.goto(board);
    await choose(p, '#run-board-filters select[name=workflow_id]', seed.assistant.workflow);
    const byWorkflow = await totals(p);
    const sum = rows => rows.reduce((total, [, n]) => total + n, 0);
    if (sum(byWorkflow) !== new Set(seed.assistantRuns).size) throw new Error(`workflow ${JSON.stringify(byWorkflow)}`);
    await choose(p, '#run-board-filters select[name=workflow_id]', '');
    await choose(p, '#run-board-filters select[name=agent_id]', seed.agents.assistant);
    const byAgent = await totals(p);
    if (sum(byAgent) !== new Set(seed.assistantRuns).size) throw new Error(`agent ${JSON.stringify(byAgent)}`);
    await choose(p, '#run-board-filters select[name=agent_id]', '');
    await choose(p, '#run-board-window', 'day');
    await expect(p.locator('[data-state="failed"] p').first()).toContainText('the last 24 hours');
    return `workflow ${sum(byWorkflow)}, agent ${sum(byAgent)}, window day`;
});
await check('cards: parent rollup, escaped hostile title', async () => {
    await p.goto(board);
    const parent = p.locator(`[data-run="${seed.parentRun}"]`);
    const text = await parent.textContent();
    if (!/1 child run · 1 open/.test(text)) throw new Error(text);
    if (await p.locator(`[data-run="${seed.childRun}"]`).count()) throw new Error('child has its own card');
    const hostile = p.locator(`[data-run="${seed.hostileRun}"] span.font-semibold`);
    const title = await hostile.textContent();
    if (!title.includes('<img src=x onerror=alert(1)>')) throw new Error(`title rendered as ${title}`);
    if (await p.locator('img[src="x"]').count()) throw new Error('hostile markup was parsed');
    return text.replace(/\s+/g, ' ').trim();
});
await check('rail: Runs lit on the board, other rail links unchanged', async () => {
    await p.goto(board);
    const links = await p.$$eval('#app-rail a[data-action="navigate-workspace"]', anchors =>
        anchors.map(a => [a.getAttribute('title'), a.getAttribute('href'), a.getAttribute('aria-current')]));
    const lit = links.filter(([, , current]) => current === 'page').map(([title]) => title);
    if (lit.join() !== 'Runs') throw new Error(`lit ${lit}`);
    const expected = { Mailbox: '/ui', Channels: '/ui/channels', Agents: '/ui/agents', Workflows: '/ui/workflows',
        Schedules: '/ui/schedules', Work: '/ui/work', Deliveries: '/ui/deliveries', Dashboard: '/ui/dashboard', Companies: '/ui/companies' };
    for (const [title, href] of links) {
        if (title === 'Runs') {
            if (href !== `/ui/workflow-runs?view=board&company_id=${c}`) throw new Error(`Runs ${href}`);
        } else if (expected[title] && href !== `${expected[title]}?company_id=${c}`) {
            throw new Error(`${title} ${href}`);
        }
    }
    await p.goto(`${base}/ui/workflow-runs?company_id=${c}`);
    const listLit = await p.$eval('#app-rail a[aria-current="page"]', a => a.getAttribute('title'));
    if (listLit !== 'Runs') throw new Error(`list lit ${listLit}`);
    return links.map(([title]) => title).join(', ');
});
await check('agent pane: "Runs board" opens the single-agent board', async () => {
    await p.goto(`${base}/ui/agents?company_id=${c}&agent_id=${seed.agents.assistant}`);
    const link = p.locator('#agent-workflow-usage a', { hasText: 'Runs board' });
    await link.waitFor({ timeout: 10000 });
    await link.click();
    await p.waitForURL(/view=board/);
    if (!p.url().includes(`agent_id=${seed.agents.assistant}`)) throw new Error(p.url());
    const chosen = await p.locator('#run-board-filters select[name=agent_id]').inputValue();
    if (chosen !== seed.agents.assistant) throw new Error(`select ${chosen}`);
    return p.url();
});
await check('live: cancel from the pane moves the card without a reload', async () => {
    await p.goto(board);
    const run = seed.running[round];
    await p.locator(`[data-run="${run}"]`).click();
    await expect(p.locator('#run-selected')).toHaveValue(run);
    await expect(p.locator(`[data-run="${run}"]`)).toHaveAttribute('aria-current', 'true');
    const before = await p.evaluate(() => performance.getEntriesByType('navigation').length);
    await p.locator('#run-pane button', { hasText: /Cancel run/ }).first().click();
    await expect(p.locator(`[data-state="cancelled"] [data-run="${run}"]`)).toHaveCount(1, { timeout: 10000 });
    const after = await p.evaluate(() => performance.getEntriesByType('navigation').length);
    if (after !== before) throw new Error('the page reloaded');
    return 'moved to cancelled';
});
await check('live: retry from the pane moves a failed card out of failed', async () => {
    await p.goto(board);
    const run = seed.assistantRuns[round];
    await p.locator(`[data-run="${run}"]`).click();
    await expect(p.locator('#run-selected')).toHaveValue(run);
    const retry = p.locator('#run-pane button', { hasText: /Retry/ });
    if (!(await retry.count())) return { notExercised: 'no retry offered for this failure' };
    const posted = p.waitForResponse(response => response.url().includes('/retry'));
    await retry.first().click();
    const answer = await posted;
    await p.waitForTimeout(500);
    const banner = (await p.locator('#run-pane [role=status], #run-pane .alert').allTextContents()).join(' ').replace(/\s+/g, ' ').trim();
    const moved = await p.locator(`[data-state="failed"] [data-run="${run}"]`).count() === 0;
    const where = await p.$$eval(`[data-run="${run}"]`, cards => cards.map(card => card.closest('[data-state]').dataset.state));
    if (answer.status() >= 400) throw new Error(`retry answered ${answer.status()}: ${banner}`);
    const detail = `retry answered ${answer.status()}; banner "${banner}"; card now in ${where.join() || 'no column'}`;
    return moved ? detail : { notExercised: `the engine refused the retry, so no card moved: ${detail}` };
});
await check('dashboard: step-job panel and board link', async () => {
    await p.goto(`${base}/ui/dashboard?company_id=${c}`);
    const link = p.locator('a', { hasText: 'Runs by state' });
    await link.waitFor({ timeout: 10000 });
    if (!(await p.getByText('Workflow step jobs').count())) throw new Error('label');
    const href = await link.getAttribute('href');
    const jobs = await p.locator('a[title="Open run"]').count();
    return `board link ${href}; outstanding job rows linking a run: ${jobs}`;
});
await check('dashboard: an outstanding job opens its run', async () => {
    await p.goto(`${base}/ui/dashboard?company_id=${c}`);
    const job = p.locator('a[title="Open run"]').first();
    await p.waitForTimeout(1500);
    if (!(await job.count())) return { notExercised: 'no workflow job was outstanding during the pass (the workers drain the queue at once)' };
    await job.click();
    await p.waitForURL(/run_id=/);
    return p.url();
});
await check('themes: board tokens resolve in dark and light', async () => {
    await p.goto(board);
    const read = () => p.evaluate(() => {
        const column = getComputedStyle(document.querySelector('[data-state="queued"]'));
        const card = getComputedStyle(document.querySelector('[data-run]'));
        return { theme: document.documentElement.getAttribute('data-theme'), column: column.backgroundColor,
            card: card.backgroundColor, border: card.borderTopColor, text: card.color };
    });
    const first = await read();
    await p.locator('label:has(#theme-toggle)').first().click();
    await p.waitForTimeout(300);
    const second = await read();
    if (first.theme === second.theme) throw new Error(`theme did not change: ${JSON.stringify(first)}`);
    for (const key of ['column', 'card', 'text']) {
        if (first[key] === second[key]) throw new Error(`${key} same in both themes: ${first[key]}`);
    }
    for (const value of [first, second]) {
        if (value.card === 'rgba(0, 0, 0, 0)' || value.column === 'rgba(0, 0, 0, 0)') throw new Error(`transparent ${JSON.stringify(value)}`);
    }
    await p.locator('label:has(#theme-toggle)').first().click();
    return `${JSON.stringify(first)} / ${JSON.stringify(second)}`;
});
await check('owner pages raised no script errors', async () => {
    if (p.errors.length) throw new Error(p.errors.join(' | '));
});
await owner.context.close();

const admin = await session('admin');
await check('admin: runs on a channel they cannot see are left out', async () => {
    await admin.page.goto(board);
    const adminTotals = Object.fromEntries(await totals(admin.page));
    const ownerSucceeded = Object.fromEntries(ownerTotals).succeeded;
    for (const run of seed.hidden) {
        if (await admin.page.locator(`[data-run="${run}"]`).count()) throw new Error(`hidden run ${run} shown`);
    }
    if (adminTotals.succeeded !== ownerSucceeded - seed.hidden.length) throw new Error(`succeeded ${adminTotals.succeeded} vs owner ${ownerSucceeded}`);
    return `succeeded ${adminTotals.succeeded} (owner ${ownerSucceeded})`;
});
await admin.context.close();

const phone = await session('owner', { width: 390, height: 844 });
await check('phone: a card opens the pane full width, back returns to the board', async () => {
    const page = phone.page;
    await page.goto(board);
    await page.locator(`[data-run="${seed.reviewRuns[0]}"]`).click();
    await expect(page.locator('#run-selected')).toHaveValue(seed.reviewRuns[0]);
    const pane = await page.$eval('#run-pane', el => [getComputedStyle(el).display, Math.round(el.getBoundingClientRect().width)]);
    const boardShown = await page.$eval('#run-board', el => getComputedStyle(el).display);
    if (pane[0] === 'none' || pane[1] < 380 || boardShown !== 'none') throw new Error(`pane ${pane}, board ${boardShown}`);
    await page.locator('.ui-mobile-back').first().click();
    const back = await page.$eval('#run-board', el => [getComputedStyle(el).display, Math.round(el.getBoundingClientRect().width)]);
    if (back[0] === 'none' || back[1] < 380) throw new Error(`board after back ${back}`);
    // The failed column is off to the right: bring it into view, then wheel over it until its last
    // card shows. The page itself must not grow past the viewport.
    const doc = await page.evaluate(() => document.documentElement.scrollHeight);
    if (doc > 844) throw new Error(`page grew to ${doc}px`);
    const column = page.locator('[data-state="failed"]');
    await column.scrollIntoViewIfNeeded();
    const list = column.locator('ul');
    const box = await list.boundingBox();
    const last = column.locator('li').last();
    for (let i = 0; i < 40 && !(await last.isVisible() && await last.evaluate(el => {
        const r = el.getBoundingClientRect(); return r.bottom <= window.innerHeight && r.top >= 0;
    })); i++) {
        await page.mouse.move(box.x + box.width / 2, box.y + Math.min(box.height / 2, 200));
        await page.mouse.wheel(0, 600);
        await page.waitForTimeout(50);
    }
    const lastBox = await last.boundingBox();
    if (!lastBox || lastBox.y + lastBox.height > 844 || lastBox.y < 0) throw new Error(`last card at ${JSON.stringify(lastBox)}`);
    const overflow = `page ${doc}px; last of 50 failed cards reached at y=${Math.round(lastBox.y)}`;
    return `pane ${pane[1]}px; board ${back[1]}px after back; ${overflow}`;
});
await phone.context.close();
await browser.close();

writeFileSync(`${dir}/pass-results-round-${round}.json`, JSON.stringify({ round, results }, null, 2));
console.log(`round ${round}`);
for (const result of results) console.log(`${result.status}  ${result.name} — ${result.detail}`);
