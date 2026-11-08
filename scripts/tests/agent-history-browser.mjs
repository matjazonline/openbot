import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { chromium, expect } from '@playwright/test';
const read=path=>readFileSync(new URL(`../../${path}`,import.meta.url),'utf8');
assert.ok(process.env.AGENT_HISTORY_BROWSER_FIXTURES,'generate actual native history HTML fixtures first');
const fixture=name=>readFileSync(`${process.env.AGENT_HISTORY_BROWSER_FIXTURES}/${name}`,'utf8');
const facts=JSON.parse(fixture('facts.json'));
function constant(path,name) {
    const source=read(path);const match=source.match(new RegExp(`const ${name}: &str = r(#+)"([\\s\\S]*?)"\\1;`));
    assert.ok(match,`actual ${name}`);return match[2];
}
const busy=constant('src/adapters/http/pages/mailbox.rs','BUTTON_BUSY_SCRIPT');
const errors=constant('src/adapters/http/pages/mailbox.rs','REQUEST_ERROR_SCRIPT');
const toast=constant('src/adapters/http/pages/mailbox.rs','REQUEST_ERROR_TOAST');
async function harness(theme) {
    const pending=[];const browserErrors=[];
    const server=createServer((request,response)=>{
        if(request.url==='/htmx.js')return response.end(read('assets/htmx-2.0.4.min.js'));
        if(request.url==='/app.css')return response.end(read('assets/app.css'));
        if(request.url==='/progress.js')return response.end(busy+errors);
        if(request.url.includes('/history?')){pending.push({url:request.url,response});return;}
        response.end(`<!doctype html><html data-theme="${theme}"><head><link rel="stylesheet" href="/app.css"><script src="/htmx.js"></script></head><body>${toast}${fixture('loader.html')}<script src="/progress.js"></script></body></html>`);
    });
    await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
    const browser=await chromium.launch(process.env.CHROME_PATH?{executablePath:process.env.CHROME_PATH}:{});
    const page=await browser.newPage({viewport:{width:1100,height:850}});
    page.on('pageerror',error=>browserErrors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    return {page,pending,browserErrors,async close(){await browser.close();for(const entry of pending)entry.response.destroy();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}};
}
async function take(h) {await expect.poll(()=>h.pending.length).toBeGreaterThan(0);return h.pending.shift();}
async function settled(h) {await expect(h.page.locator('.htmx-request, .htmx-settling, .htmx-swapping, .htmx-added')).toHaveCount(0);}
async function keyboard(button) {await button.focus();await button.press('Enter');}
async function load(h) {
    await keyboard(h.page.locator('summary').first());
    const request=await take(h);request.response.end(fixture('newest.html'));
    await expect(h.page.locator('[data-history-order]')).toHaveCount(16);
    await expect(h.page.locator('[data-history-order]').first()).toHaveAttribute('data-history-order',String(facts.latest_order));
    await settled(h);
}
for(const theme of ['dark','light'])test(`actual history HTML: ${theme} keyboard pagination reaches last exchange, detail is escaped, reads replace and failures recover`,async()=>{
    const h=await harness(theme);const {page}=h;
    try {
        await load(h);
        await keyboard(page.getByRole('button',{name:'Inspect exchange 18',exact:true}));
        (await take(h)).response.end(fixture('current-before-expiry.html'));
        let current=page.locator(`#${facts.region}-detail`);
        await expect(current).toContainText('Exchange 18 · Produced');await settled(h);
        assert.equal(await page.locator('[data-history-order="2"]').count(),0,'selected source really is outside this page');
        await keyboard(current.getByRole('button',{name:'Inspect exchange 2',exact:true}));
        (await take(h)).response.end(fixture('selected-outside-page.html'));
        await expect(current).toContainText('Exchange 2 · Produced');await settled(h);
        let older=page.getByRole('button',{name:'Older exchanges'});
        await keyboard(older);const late=await take(h);
        await expect(older).toHaveAttribute('aria-busy','true');
        await expect(older).toBeDisabled();await expect(older.locator('[data-request-spinner]')).toHaveCount(1);
        // Another accepted read to the same history region supersedes the old page request.
        await keyboard(page.getByRole('button',{name:'Latest exchanges'}));
        const latest=await take(h);latest.response.end(fixture('newest.html'));
        late.response.end(fixture('older.html'));
        await settled(h);
        await expect(page.locator('[data-history-order]')).toHaveCount(16);
        await expect(page.locator('[data-history-order]').first()).toHaveAttribute('data-history-order','18');
        await keyboard(page.getByRole('button',{name:'Older exchanges'}));
        (await take(h)).response.end(fixture('older.html'));
        await expect(page.locator('[data-history-order]')).toHaveCount(2);
        await settled(h);
        await expect(page.locator('[data-history-order]').last()).toHaveAttribute('data-history-order',String(facts.oldest_order));
        const inspect=page.getByRole('button',{name:`Inspect exchange ${facts.oldest_order}`,exact:true});
        await keyboard(inspect);const detail=await take(h);
        assert.ok(detail.url.includes('exchange='),'the actual exact-exchange route is used');
        await expect(inspect).toHaveAttribute('aria-busy','true');detail.response.end(fixture('first.html'));
        const article=page.locator(`#${facts.region}-detail`);
        await expect(article).toContainText('Exchange 1 · Produced');
        await settled(h);
        await keyboard(article.locator('summary').filter({hasText:'Original input'}));
        await expect(article.locator('pre').first()).toBeVisible();
        await expect(article.locator('pre').first()).toContainText('</script><script>history-secret</script>');
        assert.equal(await page.locator('script').count(),2,'stored markup never creates executable nodes');
        await expect(article).toContainText('Workflow: cancelled');
        // Failed GETs keep the current view and restore the initiating control.
        await keyboard(inspect);(await take(h)).response.writeHead(500).end('History changed; reload the inspection');
        await expect(inspect).toBeEnabled();await expect(inspect).not.toHaveAttribute('aria-busy','true');
        await expect(page.locator('#request-error-toast')).toBeVisible();
        await expect(page.locator('#request-error-message')).toContainText('History changed; reload');
        await expect(article).toContainText('Exchange 1 · Produced');
        assert.deepEqual(h.browserErrors,[]);
    }finally{await h.close();}
});
