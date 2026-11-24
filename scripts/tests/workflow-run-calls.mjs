// The run page's channel-call section in a browser: the exact Rust renderings
// (calls_tests.rs) under the shipped stylesheet, at phone width.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { chromium, expect } from '@playwright/test';

const directory = process.env.WORKFLOW_CALLS_BROWSER_FIXTURES;
assert.ok(directory, 'Generate markup with WORKFLOW_CALLS_BROWSER_FIXTURES=<dir> cargo test --lib channel_call_views_section_draws_only_authorized_references');
const fixture = name => readFileSync(`${directory}/${name}.html`, 'utf8');
const css = readFileSync(new URL('../../assets/app.css', import.meta.url), 'utf8');
const document = section => `<!doctype html><html><head><meta name="viewport" content="width=device-width">
    <style>${css}</style></head><body><main class="p-4">${section}</main></body></html>`;

test('a call lists its phase, review link and deadline, and a restricted viewer gets labels only', async () => {
    const browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
    try {
        const page = await browser.newPage({ viewport: { width: 390, height: 800 } });
        await page.setContent(document(fixture('authorized')));
        await expect(page.getByRole('heading', { name: 'Desk calls' })).toBeVisible();
        const call = page.locator('[data-call]');
        await expect(call.locator('[data-call-phase]')).toHaveText('Awaiting final review');
        await expect(call.getByRole('link', { name: 'review' })).toHaveAttribute('href', /^\/ui\/workflow-decisions\//);
        await expect(call.getByText('past deadline')).toBeVisible();
        await expect(call.getByText('<b>finance</b>')).toBeVisible();
        await expect(page.getByText('Only the newest 50 calls are listed.')).toBeVisible();
        assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), true,
            'no horizontal scroll at phone width');
        await page.setContent(document(fixture('restricted')));
        const restricted = page.locator('[data-call]');
        await expect(restricted.getByText('a desk', { exact: false })).toBeVisible();
        assert.deepEqual(await restricted.getByRole('link').allTextContents(), ['step'], 'only the step link remains');
    } finally {
        await browser.close();
    }
});
