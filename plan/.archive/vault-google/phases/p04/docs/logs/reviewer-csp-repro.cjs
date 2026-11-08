const {chromium}=require('@playwright/test');
const http=require('http');const fs=require('fs');
(async()=>{
 const csp=fs.readFileSync('src/adapters/http/security.rs','utf8').match(/const CSP: &str = "([^"]+)"/)[1];
 let posted=0,reached=0;
 const target=http.createServer((req,res)=>{reached++;res.end('provider consent reached');});
 await new Promise(r=>target.listen(0,'127.0.0.1',r));
 const targetUrl=`http://127.0.0.1:${target.address().port}/consent`;
 const server=http.createServer((req,res)=>{res.setHeader('Content-Security-Policy',csp);if(req.method==='POST'){posted++;res.writeHead(303,{Location:targetUrl,'Referrer-Policy':'no-referrer','Cache-Control':'no-store'});res.end();}else{res.setHeader('Content-Type','text/html');res.end('<form method="post" action="/ui/profile/vault/apps/gmail/connect"><button type="submit">Connect gmail</button></form>');}});
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 let browser;
 try{browser=await chromium.launch({headless:true,executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});const page=await browser.newPage();const errors=[];page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});await page.goto(`http://127.0.0.1:${server.address().port}`);await page.getByRole('button').click();await page.waitForTimeout(500);console.log(JSON.stringify({browser:browser.version(),csp,posted,reached,url:page.url(),errors},null,2));}
 finally{if(browser)await browser.close();await new Promise(r=>server.close(r));await new Promise(r=>target.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1;});
