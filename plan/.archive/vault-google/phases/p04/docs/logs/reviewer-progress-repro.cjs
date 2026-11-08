const {chromium}=require('@playwright/test');const http=require('http');const fs=require('fs');
(async()=>{
 const source=fs.readFileSync('src/adapters/http/pages/mailbox.rs','utf8');
 const constants=['BUTTON_BUSY_SCRIPT','EVENT_DELEGATION_SCRIPT'];
 const script=constants.map(n=>source.split('const '+n+': &str = r##"')[1].split('"##;')[0]).join('\n');
 const csp=fs.readFileSync('src/adapters/http/security.rs','utf8').match(/const CSP: &str = "([^"]+)"/)[1];
 let posted=0;const held=[];
 const server=http.createServer((req,res)=>{res.setHeader('Content-Security-Policy',csp);if(req.method==='POST'){posted++;held.push(res);}else if(req.url==='/app.js'){res.setHeader('Content-Type','text/javascript');res.end(script);}else{res.setHeader('Content-Type','text/html');res.end('<form method="post" action="/ui/profile/vault/apps/gmail/connect"><button type="submit" class="btn btn-primary">Connect gmail</button></form><script src="/app.js"></script>');}});
 await new Promise(r=>server.listen(0,'127.0.0.1',r));let browser;
 try{browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto(`http://127.0.0.1:${server.address().port}`);const click=page.getByRole('button').click({timeout:5000}).catch(()=>{});for(let i=0;i<50&&posted===0;i++)await page.waitForTimeout(20);await page.waitForTimeout(100);const state=await page.evaluate(()=>{const b=document.querySelector('button');return {disabled:b.disabled,ariaBusy:b.getAttribute('aria-busy'),text:b.textContent,spinner:!!b.querySelector('.loading,[data-progress]')};});console.log(JSON.stringify({browser:browser.version(),posted,pending:true,button:state,errors},null,2));held.forEach(r=>r.end('done'));await click;}
 finally{held.forEach(r=>{if(!r.writableEnded)r.end('done');});if(browser)await browser.close();await new Promise(r=>server.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1;});
