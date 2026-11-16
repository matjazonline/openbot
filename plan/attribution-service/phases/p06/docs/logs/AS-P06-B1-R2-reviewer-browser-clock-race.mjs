import {chromium} from '/tmp/as-p03-browser/node_modules/playwright/index.mjs';
import http from 'node:http';
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
const sdk=fs.readFileSync('services/attribution/assets/sdk-1.0.0.js','utf8');
const installation=randomUUID(), a=randomUUID(), b=randomUUID();
const server=http.createServer((req,res)=>{
 if(req.url==='/sdk/1.0.0.js'){res.writeHead(200,{'content-type':'text/javascript'});res.end(sdk);return;}
 if(req.url==='/v1/events'){res.writeHead(204);res.end();return;}
 res.writeHead(200,{'content-type':'text/html'});res.end(`<script src='/sdk/1.0.0.js'></script><script>window.tracker=OpenbotAttribution.create('${installation}')</script>`);
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
let browser;
try {
 const origin=`http://127.0.0.1:${server.address().port}`;
 browser=await chromium.launch({headless:true});const context=await browser.newContext();
 const tabA=await context.newPage(),tabB=await context.newPage();
 await tabA.goto(`${origin}/?ob_click=${a}`);await tabA.evaluate(()=>tracker.setConsent(true));
 await tabB.goto(`${origin}/?ob_click=${b}`);
 const cdp=await context.newCDPSession(tabA);await cdp.send('Debugger.enable');
 const bp=await cdp.send('Debugger.setBreakpointByUrl',{url:origin+'/sdk/1.0.0.js',lineNumber:29,columnNumber:6});
 const paused=new Promise(r=>cdp.once('Debugger.paused',r));
 const handoff=tabA.evaluate(()=>tracker.handoff());await paused;
 console.log('Tab A paused after capturing now, before reading shared storage.');
 await new Promise(resolve=>setTimeout(resolve,30));
 await tabB.evaluate(()=>tracker.setConsent(true));
 assert.ok((await tabB.evaluate(()=>tracker.handoff())).candidate_click_ids.includes(b));
 console.log('Tab B granted consent and persisted its distinct click.');
 await cdp.send('Debugger.removeBreakpoint',{breakpointId:bp.breakpointId});await cdp.send('Debugger.resume');await handoff;
 const persisted=await tabB.evaluate(({installation,b})=>localStorage.getItem(`ob-attribution:1:${installation}:click:${b}`),{installation,b});
 console.log('B localStorage entry after A resumes:',persisted);assert.equal(persisted,null);
 await tabB.goto(origin+'/return');await new Promise(resolve=>setTimeout(resolve,30));
 await tabB.evaluate(()=>tracker.setConsent(true));
 const returned=await tabB.evaluate(()=>tracker.handoff());
 console.log('Return visit retains B:',returned.candidate_click_ids.includes(b));assert.equal(returned.candidate_click_ids.includes(b),false);
 console.log('REPRODUCED in two real Chromium tabs using the unmodified production SDK; only 2 candidates, neither expired. CDP fixes the interleaving; service response is a 204 stand-in.');
} finally {await browser?.close();await new Promise(r=>server.close(r));}
