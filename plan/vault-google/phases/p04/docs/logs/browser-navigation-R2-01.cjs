const {chromium,expect}=require('@playwright/test');
const http=require('node:http'),fs=require('node:fs'),assert=require('node:assert/strict');
const dir=process.argv[2], metadata=JSON.parse(fs.readFileSync(dir+'/navigation.json','utf8'));
let posted=0,reached=0,mode='hold',release,entered;
const nextPost=()=>new Promise(r=>entered=r);
const server=http.createServer(async(req,res)=>{
 res.setHeader('Content-Security-Policy',metadata.csp);
 if(req.method==='POST') {
  if(req.headers.origin!==base){res.writeHead(403);return res.end('Cross-origin request denied');}
  let body='';for await(const chunk of req)body+=chunk;
  assert.equal(req.url,'/ui/profile/vault/apps/gmail/connect');posted++;
  const reconnect=new URLSearchParams(body).has('account');
  const selected=mode;
  if(entered){entered();entered=null;}
  if(selected==='hold')await new Promise(r=>release=r);
  res.setHeader('Cache-Control','no-store');res.setHeader('Referrer-Policy','no-referrer');
  res.setHeader('Content-Type','text/html');
  if(selected==='error'){res.writeHead(503);return res.end('App consent is temporarily unavailable. Try again.');}
  res.setHeader('HX-Redirect',metadata[reconnect?'reconnect':'connect']);
  return res.end(fs.readFileSync(dir+(reconnect?'/reconnect.html':'/connect.html')));
 }
 if(req.url==='/assets/htmx-2.0.4.min.js'){res.setHeader('Content-Type','application/javascript');return res.end(fs.readFileSync('assets/htmx-2.0.4.min.js'));}
 if(req.url.startsWith('/assets/app.')&&req.url.endsWith('.js')){res.setHeader('Content-Type','application/javascript');return res.end(fs.readFileSync(dir+'/app.js'));}
 if(req.url.startsWith('/assets/app.')&&req.url.endsWith('.css')){res.setHeader('Content-Type','text/css');return res.end(fs.readFileSync('assets/app.css'));}
 if(req.url.startsWith('/assets/')){res.setHeader('Content-Type','application/javascript');return res.end('');}
 res.setHeader('Content-Type','text/html');res.end(fs.readFileSync(dir+'/page.html'));
});
let base,browser;
(async()=>{
 await new Promise(r=>server.listen(0,'127.0.0.1',r));base=`http://127.0.0.1:${server.address().port}`;
 browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
 const errors=[];
 async function context(options={}){
  const c=await browser.newContext(options);
  await c.route('**/*',route=>{
   const url=new URL(route.request().url());
   if(url.origin===base)return route.continue();
   if(url.origin==='https://accounts.google.com'&&url.pathname==='/o/oauth2/v2/auth'){
    reached++;return route.fulfill({status:200,contentType:'text/html',body:'<h1>Scripted provider consent</h1>'});
   }
   return route.abort();
  });return c;
 }
 const c=await context(),page=await c.newPage();page.on('console',m=>{if(m.type()==='error'&&/Content Security Policy|form-action/i.test(m.text()))errors.push(m.text());});
 for(const label of ['Connect gmail','Reconnect gmail']){
  await page.goto(base);const button=page.getByRole('button',{name:label,exact:true});
  const observed=nextPost(),before=posted;mode='hold';await button.click();await observed;
  await expect(button).toBeDisabled();await expect(button).toHaveAttribute('aria-busy','true');
  await expect(button.locator('[data-request-spinner]')).toHaveCount(1);
  assert.equal(await page.getByRole('link',{name:'Back to profile',exact:true}).getAttribute('aria-disabled'),null);
  await button.locator('..').evaluate(form=>form.requestSubmit());
  await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
  assert.equal(posted,before+1,'duplicate accepted submission must be dropped');
  const moved=page.waitForURL('https://accounts.google.com/**');release();await moved;
  await expect(page.getByRole('heading',{name:'Scripted provider consent'})).toBeVisible();
 }
 await page.goto(base);await page.evaluate(()=>{window.failedResponse=false;document.body.addEventListener('htmx:afterRequest',e=>{if(e.detail.xhr.status===503)window.failedResponse=true;});});mode='error';const button=page.getByRole('button',{name:'Connect gmail',exact:true});await button.click();await page.waitForFunction(()=>window.failedResponse);
 await expect(button).toBeEnabled();await expect(button).not.toHaveAttribute('aria-busy','true');
 await expect(button.locator('[data-request-spinner]')).toHaveCount(0);
 mode='success';await button.click();await page.waitForURL('https://accounts.google.com/**');
 const native=await context({javaScriptEnabled:false}),plain=await native.newPage();
 for(const label of ['Connect gmail','Reconnect gmail']){
  await plain.goto(base);mode='success';await plain.getByRole('button',{name:label,exact:true}).click();
  await expect(plain.getByRole('heading',{name:'Continue to app consent'})).toBeVisible();
  await plain.getByRole('link',{name:'Continue',exact:true}).click();
  await expect(plain.getByRole('heading',{name:'Scripted provider consent'})).toBeVisible();
 }
 assert.equal(reached,5);assert.equal(posted,6);assert.deepEqual(errors,[]);
 console.log(JSON.stringify({browser:browser.version(),productionRenderedFixtures:dir,posted,reached,cspErrors:errors.length,connectAndReconnect:true,heldProgressAndDuplicateDrop:true,unrelatedNavigationAvailable:true,inPlaceFailureRetry:true,nativeFallbackBoth:true,noExternalNetwork:true},null,2));
})().catch(e=>{console.error(e);process.exitCode=1;}).finally(async()=>{if(release)release();if(browser)await browser.close();server.closeAllConnections();await new Promise(r=>server.close(r));});
