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
