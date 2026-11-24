import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';

const read = path => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');
const source = read('src/adapters/http/pages/workflow_authoring/channel_response.rs');
const form = source.match(/<form hx-post="\/ui\/workflows\/channels\/\{channel\}\/response[\s\S]*?<\/form>/)?.[0];
assert.ok(form, 'use the actual production response form');
const fields = form.replaceAll('{company}', 'company').replaceAll('{channel}', 'channel')
    .replace('{revision}', '1').replace('{options}', '<option value="">Observe only</option><option value="binding:2">Support</option>')
    .replace('{contract}', '{}').replace('{policy}', '');
const busy = read('src/adapters/http/pages/mailbox.rs').split('const BUTTON_BUSY_SCRIPT: &str = r##"')[1].split('"##;')[0];

test('response form supports keyboard navigation and submit and shows request progress without dropping form fields', async () => {
    const pending = [];
    const server = createServer((request, response) => {
        if (request.url === '/app.css') return response.writeHead(200, { 'Content-Type': 'text/css' }).end(read('assets/app.css'));
        if (request.url === '/htmx.js') return response.end(read('assets/htmx-2.0.4.min.js'));
        if (request.method === 'POST') {
            let body = '';
            request.on('data', chunk => { body += chunk; });
            request.on('end', () => pending.push({ response, body }));
            return;
        }
        response.end(`<!doctype html><html><head><link rel="stylesheet" href="/app.css"><script src="/htmx.js"></script></head><body>
            <section id="channel-workflows-card">${fields}</section><script>${busy}</script></body></html>`);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    let browser;
    try {
        browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
        const page = await browser.newPage();
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        const selection = page.getByLabel('Responding workflow');
        await selection.selectOption('binding:2');
        await selection.focus();
        await page.keyboard.press('Tab');
        await expect(selection).toHaveValue('binding:2');
        await expect(page.getByLabel('Final result mapping (JSON)')).toBeFocused();
        await page.getByLabel('Final result mapping (JSON)').fill('{"body":{"literal":"<untrusted>"}}');
        const button = page.getByRole('button', { name: 'Save response settings' });
        await button.focus();
        await page.keyboard.press('Enter');
        await expect.poll(() => pending.length).toBe(1);
        await expect(button).toBeDisabled();
        await expect(button).toHaveAttribute('aria-busy', 'true');
        await expect(button.locator('[data-request-spinner]')).toBeVisible();
        const submitted = new URLSearchParams(pending[0].body);
        assert.equal(submitted.get('expected_revision'), '1');
        assert.equal(submitted.get('responder'), 'binding:2');
        assert.equal(submitted.get('contract'), '{"body":{"literal":"<untrusted>"}}');
        pending.shift().response.writeHead(500).end('Temporary failure');
        await expect(button).toBeEnabled();
        await expect(button).not.toHaveAttribute('aria-busy');
        await expect(page.getByLabel('Final result mapping (JSON)')).toHaveValue(submitted.get('contract'));
        assert.deepEqual(errors, []);
    } finally {
        await browser?.close();
        for (const item of pending) item.response.destroy();
        server.closeAllConnections();
        await new Promise(resolve => server.close(resolve));
    }
});

const callsSource = read('src/adapters/http/pages/workflow_authoring/channel_response.rs');
const callsBlock = callsSource.match(/<section class="space-y-2 border-t border-base-300 pt-4" aria-labelledby="channel-calls-heading">[\s\S]*?<\/section>/)?.[0];
assert.ok(callsBlock, 'use the actual production "Calls from agents" block');
const copyScript = read('src/adapters/http/pages/layout.rs').match(/ {8}function copyTextFrom\(button\) \{[\s\S]*?\n {8}\}\n/)?.[0];
assert.ok(copyScript, 'use the actual production copy handler');
const deskId = '6f1c2d3e-0000-4000-8000-00000000c411';
const calls = callsBlock.replace('{key}', 'public').replace('{status}', 'Any agent in the company can call this desk.')
    .replace('{channel}', deskId).replace('{responder}', 'Support').replace('{review}', 'No');

test('calls-from-agents block offers the desk id read-only, reachable by keyboard, and copyable with or without the clipboard API', async () => {
    const server = createServer((request, response) => {
        if (request.url === '/app.css') return response.writeHead(200, { 'Content-Type': 'text/css' }).end(read('assets/app.css'));
        response.end(`<!doctype html><html><head><link rel="stylesheet" href="/app.css"></head><body>${calls}
            <script>${copyScript}
            document.addEventListener('click', event => {
                const control = event.target.closest('[data-action]');
                if (control && control.dataset.action === 'copy-text') copyTextFrom(control);
            });</script></body></html>`);
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const origin = `http://127.0.0.1:${server.address().port}`;
    let browser;
    try {
        browser = await chromium.launch(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : {});
        const context = await browser.newContext();
        await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin });
        const page = await context.newPage();
        const errors = [];
        page.on('pageerror', error => errors.push(error.message));
        await page.goto(`${origin}/`);
        const id = page.getByLabel('Desk id for the call tool');
        await expect(id).toHaveValue(deskId);
        await expect(id).toHaveAttribute('readonly', '');
        await id.focus();
        await page.keyboard.press('Tab');
        // The accessible name changes to "Copied", so hold the button by its stable attribute.
        const copy = page.locator('[data-copy-from="channel-call-id"]');
        await expect(copy).toHaveAccessibleName('Copy');
        await expect(copy).toBeFocused();
        await page.keyboard.press('Enter');
        await expect(copy).toHaveText('Copied');
        assert.equal(await page.evaluate(() => navigator.clipboard.readText()), deskId);

        // Without the clipboard API the id is selected for copying by hand.
        const bare = await browser.newPage();
        await bare.addInitScript(() => Object.defineProperty(navigator, 'clipboard', { value: undefined }));
        bare.on('pageerror', error => errors.push(error.message));
        await bare.goto(`${origin}/`);
        await bare.getByRole('button', { name: 'Copy' }).click();
        const fallback = bare.getByLabel('Desk id for the call tool');
        await expect(fallback).toBeFocused();
        assert.deepEqual(
            await fallback.evaluate(input => [input.selectionStart, input.selectionEnd]),
            [0, deskId.length],
        );
        await expect(bare.getByRole('button', { name: 'Copy' })).toHaveText('Copy');
        assert.deepEqual(errors, []);
    } finally {
        await browser?.close();
        server.closeAllConnections();
        await new Promise(resolve => server.close(resolve));
    }
});
