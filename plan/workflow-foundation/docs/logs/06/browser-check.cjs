const { chromium } = require(process.cwd() + '/node_modules/@playwright/test');
const http = require('http');
const fs = require('fs');
const path = require('path');
const assert = require('assert/strict');
const directory = '/private/tmp/workflow06-usage-browser';
let posted;
const server = http.createServer((req, res) => {
  if (req.method === 'POST') {
    let body = '';
    req.on('data', chunk => body += chunk);
    req.on('end', () => { posted = new URLSearchParams(body); setTimeout(() => {res.writeHead(200, {'Content-Type':'text/html'});res.end('Saved fixture');}, 600); });
    return;
  }
  const name = new URL(req.url, 'http://localhost').pathname;
  const file = path.join(directory, name === '/' ? 'index.html' : name);
  if (!fs.existsSync(file)) {res.writeHead(404);res.end();return;}
  const type = file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : file.endsWith('.png') ? 'image/png' : 'text/html';
  res.writeHead(200, {'Content-Type': type, 'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; form-action 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"});
  res.end(fs.readFileSync(file));
});
(async () => {
  await new Promise(resolve => server.listen(0,'127.0.0.1',resolve));
  const browser = await chromium.launch({headless:true});
  try {
    const page = await browser.newPage();
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.goto('http://127.0.0.1:' + server.address().port);
    const form=page.locator('form[method=post]').last();
    for (const theme of ['dark','light']) {
      await page.locator('.theme-controller').evaluate((input, checked) => { if (input.checked !== checked) input.click(); }, theme==='light');
      await form.locator('input[name=input]').focus();
      const style=await form.locator('input[name=input]').evaluate(el=>{
        const c=getComputedStyle(el);return {theme:document.documentElement.dataset.theme,radius:c.borderRadius,outlineOffset:c.outlineOffset,color:c.color,background:c.backgroundColor};
      });
      assert.equal(style.theme,theme);assert.equal(style.outlineOffset,'-1px');assert.notEqual(style.color,style.background);
      console.log(JSON.stringify(style));
    }
    await form.getByRole('button',{name:'Save price'}).click();
    assert.equal(await form.getAttribute('aria-busy'),null,'invalid native form must remain usable');
    await form.locator('input[name=provider]').fill('openai');
    await form.locator('input[name=model]').fill('fixture');
    await form.locator('input[name=input]').fill('1.25');
    await form.locator('input[name=output]').fill('10');
    await form.getByRole('button',{name:'Save price'}).click({noWaitAfter:true});
    assert.equal(await form.getAttribute('aria-busy'),'true');
    assert.equal(await form.getByRole('button').first().innerText(),'Working…');
    await page.waitForTimeout(900);
    assert.equal(posted.get('action'),'save');assert.equal(posted.get('input'),'1.25');
    assert.deepEqual(errors,[]);console.log('PASS themes, CSP, native validation, pending feedback, submitted price fields');
  } finally {await browser.close();server.close();}
})().catch(e=>{console.error(e);server.close();process.exitCode=1;});
