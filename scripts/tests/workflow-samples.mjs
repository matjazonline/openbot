import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';
const read=path=>readFileSync(new URL(`../../${path}`,import.meta.url),'utf8');
const script=read('src/adapters/http/pages/workflow_authoring/editor.rs').split('const WORKFLOW_EDITOR_SCRIPT: &str = r##"')[1].split('"##;')[0];
const pane=`<section id="workflow-pane"><form id="workflow-sample-form" hx-post="/sample" hx-target="#workflow-pane" hx-swap="outerHTML" hx-disabled-elt="find button"><textarea name="input">{}</textarea><button>Run sample</button><p role="status" data-sample-status></p></form><a hx-get="/other" hx-target="#workflow-pane" hx-swap="outerHTML" hx-sync="#workflow-pane:replace">Other tab</a></section>`;
test('sample progress, failed request recovery and late navigation response use the requesting form',async()=>{
    const pending=[];
    const server=createServer((request,response)=>{
        if(request.url==='/htmx.js')return response.end(read('assets/htmx-2.0.4.min.js'));
        if(request.url==='/editor.js')return response.end(script);
        if(request.url==='/sample'){pending.push(response);return;}
        if(request.url==='/other')return response.end('<section id="workflow-pane">Other view</section>');
        response.end(`<!doctype html><html><head><script src="/htmx.js"></script><script defer src="/editor.js"></script></head><body>${pane}</body></html>`);
    });
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    const browser=await chromium.launch(process.env.CHROME_PATH?{executablePath:process.env.CHROME_PATH}:{});
    const page=await browser.newPage();const errors=[];page.on('pageerror',error=>errors.push(error.message));
    try {
        await page.goto(`http://127.0.0.1:${server.address().port}/`);
        await page.locator('[name=input]').fill('Unfinished input');
        await page.getByRole('button',{name:'Run sample'}).click();
        await expect.poll(()=>pending.length).toBe(1);
        await expect(page.locator('#workflow-sample-form')).toHaveAttribute('aria-busy','true');
        await expect(page.getByRole('button')).toBeDisabled();
        pending.shift().writeHead(500).end('Failed');
        await expect(page.getByRole('status')).toContainText('Your input is still here');
        await expect(page.getByRole('button')).toBeEnabled();
        await expect(page.locator('[name=input]')).toHaveValue('Unfinished input');
        await page.getByRole('button').click();await expect.poll(()=>pending.length).toBe(1);
        await page.getByText('Other tab',{exact:true}).click();await expect(page.locator('#workflow-pane')).toHaveText('Other view');
        pending.shift().end('<section id="workflow-pane">Late sample</section>');
        await page.waitForTimeout(150);await expect(page.locator('#workflow-pane')).toHaveText('Other view');
        assert.deepEqual(errors,[]);
    }finally{
        await browser.close();for(const response of pending)response.destroy();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
    }
});

test('actual sample textarea template preserves leading newlines for every raw JSON field',async()=>{
    const source=read('src/adapters/http/pages/workflow_authoring/sample.rs');
    const template=source.match(/<textarea name="\{name\}"[^>]*>[\s\S]*?\{raw\}<\/textarea>/)?.[0];
    assert.ok(template,'test must use the production textarea template');
    const browser=await chromium.launch(process.env.CHROME_PATH?{executablePath:process.env.CHROME_PATH}:{});
    try {
        const page=await browser.newPage();
        for(const raw of ['\n{invalid JSON','\n\n{}','\n[1, 2]','{}']) {
            const html=['input','params','scripts'].map(name=>template.replace('{name}',name).replace('{raw}',raw)).join('');
            for(let cycle=0;cycle<2;cycle++) {
                await page.setContent(html);
                for(const name of ['input','params','scripts'])await expect(page.locator(`[name=${name}]`)).toHaveValue(raw);
            }
        }
    }finally{await browser.close();}
});
