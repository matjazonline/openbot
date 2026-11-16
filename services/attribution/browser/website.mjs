import assert from "node:assert/strict";
import {pathToFileURL} from "node:url";
import https from "node:https";
import {mkdtempSync,readFileSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {execFileSync} from "node:child_process";
import {randomUUID} from "node:crypto";
const modulePath=process.env.ATTRIBUTION_PLAYWRIGHT_MODULE;
const {chromium}=modulePath ? await import(pathToFileURL(modulePath).href) : await import("playwright");
const service=process.env.ATTRIBUTION_BROWSER_ORIGIN, installation=process.env.ATTRIBUTION_BROWSER_INSTALLATION;
const key=process.env.ATTRIBUTION_BROWSER_MERCHANT_KEY, codeA=process.env.ATTRIBUTION_BROWSER_LINK_A, codeB=process.env.ATTRIBUTION_BROWSER_LINK_B, linkB=process.env.ATTRIBUTION_BROWSER_LINK_ID_B;
const carts=new Map(), emitted=[], deliveries=[];
let browser,merchant,directory,page,stage="launch";
const state=async()=> (await fetch(`${service}/fixture/website-state`)).json();
async function until(check,message) {const end=Date.now()+10000;while(Date.now()<end){if(await check())return;await new Promise(resolve=>setTimeout(resolve,25));}throw new Error(message);}
const desktop="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36";
async function bytes(request) {let data="";for await(const chunk of request){data+=chunk;if(data.length>16384)throw new Error("fixture body bound");}return data;}
function html() {
  return `<!doctype html><html><head><meta name="referrer" content="unsafe-url"><title>Merchant API fixture</title></head><body><h1>Product</h1>
<button id="grant">Grant tracking consent</button><button id="withdraw">Withdraw tracking consent</button><button id="checkout">Create checkout</button><button id="watch">Observe placement</button><pre id="result" role="status"></pre>
<div style="height:2200px"></div><div id="placement" style="height:200px;width:300px">Placement</div>
<script src="${service}/sdk/1.0.0.js" integrity="${process.env.ATTRIBUTION_BROWSER_SDK_INTEGRITY}" crossorigin="anonymous" referrerpolicy="no-referrer"></script><script>
window.tracker=OpenbotAttribution.create(${JSON.stringify(installation)});
const saveCart=()=>fetch('/cart',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(tracker.handoff())});
document.getElementById('grant').onclick=()=>tracker.setConsent(true);
document.getElementById('withdraw').onclick=async()=>{tracker.setConsent(false);await saveCart();};
document.getElementById('watch').onclick=()=>tracker.observeVisible(document.getElementById('placement'),${JSON.stringify(linkB)});
document.getElementById('checkout').onclick=async()=>{await saveCart();const response=await fetch('/checkout',{method:'POST'});document.getElementById('result').textContent=JSON.stringify(await response.json());};
</script></body></html>`;
}
try {
  directory=mkdtempSync(join(tmpdir(),"as-website-"));
  execFileSync("openssl",["req","-x509","-newkey","rsa:2048","-nodes","-keyout",join(directory,"key.pem"),"-out",join(directory,"cert.pem"),"-days","1","-subj","/CN=shop.merchant.com"],{stdio:"ignore"});
  merchant=https.createServer({key:readFileSync(join(directory,"key.pem")),cert:readFileSync(join(directory,"cert.pem"))},async(request,response)=>{
    try {
      const url=new URL(request.url,"https://shop.merchant.com");
      let cartId=request.headers.cookie?.split(";").map(s=>s.trim()).find(s=>s.startsWith("fixture_cart="))?.slice(13);
      if(!cartId || !carts.has(cartId) || url.pathname==="/new-cart") {
        cartId=randomUUID();carts.set(cartId,{consent:false,ids:[],frozen:null});response.setHeader("Set-Cookie",`fixture_cart=${cartId}; Secure; HttpOnly; SameSite=Lax; Path=/`);
      }
      const cart=carts.get(cartId);
      if(url.pathname==="/new-cart") {response.writeHead(204);response.end();return;}
      if(request.method==="POST" && url.pathname==="/cart") {
        const input=JSON.parse(await bytes(request));
        if(Object.keys(input).sort().join(",")!=="candidate_click_ids,consent" || typeof input.consent!=="boolean" || !Array.isArray(input.candidate_click_ids) || input.candidate_click_ids.length>32 || input.candidate_click_ids.some(id=>typeof id!=="string" || !/^[0-9a-f-]{36}$/.test(id))) {response.writeHead(400);response.end();return;}
        cart.consent=input.consent;cart.ids=input.consent ? [...new Set([...cart.ids,...input.candidate_click_ids])].slice(-32) : [];
        response.writeHead(204);response.end();return;
      }
      if(request.method==="POST" && url.pathname==="/checkout") {
        if(await bytes(request)) {response.writeHead(400);response.end();return;}
        if(!cart.frozen) {
          cart.request ??= {checkout_id:randomUUID(),order_created_at:new Date().toISOString(),consent:cart.consent,candidate_click_ids:[...cart.ids]};
          if(!cart.inflight) {
            const payload=cart.request;deliveries.push(payload);
            cart.inflight=(async()=>{
              const result=await fetch(`${service}/v1/checkouts`,{method:"POST",headers:{Authorization:`Bearer ${key}`,"Content-Type":"application/json"},body:JSON.stringify(payload)});
              if(!result.ok) throw new Error(`merchant API checkout status ${result.status}`);
              cart.frozen=await result.json();
            })().finally(()=>{cart.inflight=null;});
          }
          await cart.inflight;
        }
        response.writeHead(200,{"Content-Type":"application/json"});response.end(JSON.stringify(cart.frozen));return;
      }
      if(request.method==="POST" && ["/payment","/refund/partial","/refund/remaining"].includes(url.pathname)) {
        if(await bytes(request) || !cart.frozen) {response.writeHead(400);response.end();return;}
        // Test merchant owns these facts; the browser cannot submit trusted money,
        // identities, timestamps or credentials.
        const now=new Date().toISOString();
        cart.payment ??= {kind:"paid",event_id:randomUUID(),order_id:randomUUID(),occurred_at:now,order_created_at:cart.request.order_created_at,paid_at:now,checkout_id:cart.request.checkout_id,currency:"EUR",lines:[{line_id:"x",product_id:"PX",quantity:2,merchandise_amount:2400},{line_id:"y",product_id:"PY",quantity:1,merchandise_amount:3500}],merchandise_amount:5900,tax_amount:1180,shipping_amount:500};
        let payload=cart.payment;
        if(url.pathname!=="/payment") {
          const partial=url.pathname.endsWith("/partial");
          cart.refunds ??= {};
          cart.refunds[url.pathname] ??= {kind:"refund",event_id:randomUUID(),order_id:cart.payment.order_id,refund_id:randomUUID(),occurred_at:now,currency:"EUR",lines:partial ? [{line_id:"x",returned_quantity:1,merchandise_amount:1200}] : [{line_id:"x",returned_quantity:1,merchandise_amount:1200},{line_id:"y",returned_quantity:1,merchandise_amount:3500}],tax_amount:partial ? 240 : 940,shipping_amount:partial ? 0 : 500};
          payload=cart.refunds[url.pathname];
        }
        const result=await fetch(`${service}/v1/conversions`,{method:"POST",headers:{Authorization:`Bearer ${key}`,"Content-Type":"application/json"},body:JSON.stringify(payload)});
        response.writeHead(result.status,{"Content-Type":"application/json"});response.end(await result.text());return;
      }
      response.writeHead(200,{"Content-Type":"text/html"});response.end(html());
    } catch {response.writeHead(500);response.end("fixture request failed");}
  });
  await new Promise(resolve=>merchant.listen(0,"127.0.0.1",resolve));
  browser=await chromium.launch({headless:true,args:[`--host-resolver-rules=MAP shop.merchant.com 127.0.0.1:${merchant.address().port}`,"--no-proxy-server"]});
  const context=await browser.newContext({ignoreHTTPSErrors:true,userAgent:desktop});
  context.on("request",request=>{if(request.url()===`${service}/v1/events`) emitted.push(request.allHeaders().then(headers=>({url:request.url(),headers,body:JSON.parse(request.postData())})));});
  page=await context.newPage();
  page.on("pageerror",error=>console.log("merchant fixture page error:",error.message));
  const open=async(target)=>{await page.goto(target);await page.waitForFunction(()=>Boolean(window.tracker));};
  const grant=async()=>{await page.getByRole("button",{name:"Grant tracking consent",exact:true}).click();await page.waitForFunction(()=>window.tracker.handoff().consent);};
  const handoff=()=>page.evaluate(()=>window.tracker.handoff());
  const checkout=async()=>{
    await page.locator("#result").evaluate(node=>node.textContent="");await page.getByRole("button",{name:"Create checkout",exact:true}).click();
    await page.waitForFunction(()=>document.getElementById("result").textContent.startsWith("{"));return JSON.parse(await page.locator("#result").textContent());
  };
  stage="no optional tracking before explicit consent; URL removal";
  await open(`${service}/r/${codeA}`);
  assert.equal(new URL(page.url()).searchParams.has("ob_click"),false);
  assert.equal((await state()).observations,0);assert.equal(emitted.length,0);
  assert.equal(await page.evaluate(()=>localStorage.length),0);
  assert.deepEqual(await handoff(),{consent:false,candidate_click_ids:[]});
  stage="consented Ad A then Ad B navigation and bounded retained history";
  await grant();const a=(await handoff()).candidate_click_ids[0];assert.ok(a);
  await until(async()=> (await state()).observations===1,"Ad A landing commit");
  await open(`${service}/r/${codeB}`);assert.equal((await state()).observations,1);
  assert.deepEqual(new URL(page.url()).searchParams.getAll("keep"),["1","2"]);assert.equal(new URL(page.url()).hash,"#detail");
  await grant();const history=(await handoff()).candidate_click_ids;const b=history.find(id=>id!==a);assert.ok(b);assert.equal(history.length,2);
  await until(async()=> (await state()).observations===2,"Ad B landing commit");
  stage="same observation ID across response-loss retry";
  let first,retried=false;
  await context.route(`${service}/v1/events`,async route=>{
    const body=JSON.parse(route.request().postData());
    if(body.kind!=="landing_page_load")return route.continue();
    if(!first){first=body;const response=await route.fetch();assert.equal(response.status(),204);return route.abort("failed");}
    assert.deepEqual(body,first);retried=true;return route.continue();
  });
  await open("https://shop.merchant.com/return?synthetic_email=do-not-collect%40example.invalid");await grant();
  await until(()=>retried,"same event retry after committed response loss");await context.unroute(`${service}/v1/events`);
  assert.equal((await state()).observations,3);
  stage="viewport threshold/duration distinct from landing loads";
  await page.getByRole("button",{name:"Observe placement",exact:true}).click();
  await page.evaluate(()=>scrollTo(0,document.getElementById("placement").offsetTop-innerHeight+80));await page.waitForTimeout(1200);assert.equal((await state()).visible,0,"40 percent is below the required threshold");
  await page.evaluate(()=>{Object.defineProperty(document,"visibilityState",{value:"hidden",configurable:true});document.dispatchEvent(new Event("visibilitychange"));});
  await page.locator("#placement").scrollIntoViewIfNeeded();await page.waitForTimeout(1200);assert.equal((await state()).visible,0,"hidden-document branch must not count a visible placement");
  await page.evaluate(()=>{delete document.visibilityState;document.dispatchEvent(new Event("visibilitychange"));});
  await page.locator("#placement").scrollIntoViewIfNeeded();await page.waitForTimeout(400);await page.evaluate(()=>scrollTo(0,0));await page.waitForTimeout(750);
  assert.equal((await state()).visible,0);
  await page.locator("#placement").scrollIntoViewIfNeeded();await page.waitForTimeout(750);assert.equal((await state()).visible,0,"less than 1000ms must not count");await until(async()=> (await state()).visible===1,"continuous visible placement commit");await page.evaluate(()=>scrollTo(0,0));
  stage="server cart chooses Ad B and freezes checkout";
  const simultaneous=await page.evaluate(async()=>{
    await fetch('/cart',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(tracker.handoff())});
    return Promise.all([0,1].map(()=>fetch('/checkout',{method:'POST'}).then(r=>r.json())));
  });
  assert.deepEqual(simultaneous[0],simultaneous[1]);const frozen=simultaneous[0];assert.deepEqual(await checkout(),frozen);assert.equal(frozen.click_id,b);assert.equal(frozen.reason,"attributed");assert.ok(!frozen.checkout_id.includes("forged"));
  const attack=await page.evaluate(()=>fetch('/checkout',{method:'POST',body:JSON.stringify({checkout_id:'forged'})}).then(r=>r.status));assert.equal(attack,400);
  await open(`${service}/r/${codeA}`);await grant();assert.deepEqual(await checkout(),frozen);
  stage="merchant payment and partial/full refunds preserve Ad B and exact retries";
  for(const path of ["/payment","/refund/partial","/refund/remaining"]) {
    const results=await page.evaluate(async path=>{
      const send=()=>fetch(path,{method:"POST"}).then(async r=>({status:r.status,body:await r.json()}));
      return [await send(),await send()];
    },path);
    assert.equal(results[0].status,200);assert.equal(results[0].body.state,"accepted");assert.deepEqual(results[0],results[1]);
    assert.ok(results[0].body.order_id);
  }
  assert.equal(await page.evaluate(()=>fetch('/payment',{method:'POST',body:JSON.stringify({merchandise_amount:1})}).then(r=>r.status)),400);
  stage="multiple tabs merge candidates and withdrawal clears both tabs";
  await page.evaluate(()=>fetch('/new-cart',{method:'POST'}));
  const beforeSecond=(await handoff()).candidate_click_ids;
  const second=await context.newPage();await second.goto(`${service}/r/${codeB}`);await second.waitForFunction(()=>Boolean(window.tracker));await second.getByRole("button",{name:"Grant tracking consent",exact:true}).click();
  const secondIds=await second.evaluate(()=>tracker.handoff().candidate_click_ids);

  const merged=await handoff();assert.ok(secondIds.every(id=>merged.candidate_click_ids.includes(id)));
  const fresh=secondIds.filter(id=>!beforeSecond.includes(id));assert.equal(fresh.length,1);
  const newCheckout=await checkout();assert.equal(newCheckout.reason,"attributed");assert.equal(newCheckout.click_id,fresh[0]);
  stage="later ineligible candidates do not replace earlier eligible history";
  await page.evaluate(()=>fetch('/new-cart',{method:'POST'}));
  const bot=await fetch(`${service}/r/${codeA}`,{redirect:"manual",headers:{"User-Agent":"PreviewBot"}});assert.equal(bot.status,302);
  const botUrl=new URL(bot.headers.get("location"));const badIds=[botUrl.searchParams.get("ob_click"),process.env.ATTRIBUTION_BROWSER_FOREIGN_CLICK,process.env.ATTRIBUTION_BROWSER_EXPIRED_CLICK,process.env.ATTRIBUTION_BROWSER_FUTURE_CLICK];
  for(const id of badIds) {await open(`https://shop.merchant.com/candidate?ob_click=${id}`);await grant();assert.ok((await handoff()).candidate_click_ids.includes(id));}
  const despiteInvalid=await checkout();assert.equal(despiteInvalid.click_id,fresh[0]);assert.equal(despiteInvalid.reason,"attributed");
  await second.getByRole("button",{name:"Withdraw tracking consent",exact:true}).click();await page.waitForFunction(()=>tracker.handoff().consent===false);
  assert.deepEqual(await handoff(),{consent:false,candidate_click_ids:[]});assert.equal(await page.evaluate(()=>Object.keys(localStorage).filter(k=>k.includes(":click:")).length),0);
  await second.close();await page.evaluate(()=>fetch('/new-cart',{method:'POST'}));assert.equal((await checkout()).reason,"no_consent");
  stage="ordinary unreferenced visit produces no landing event";
  await open("https://shop.merchant.com/plain");const before=(await state()).observations;await grant();await page.waitForTimeout(150);assert.equal((await state()).observations,before);assert.equal((await handoff()).candidate_click_ids.length,0);
  stage="blocked storage still permits an in-memory consented handoff";
  const blocked=await browser.newContext({ignoreHTTPSErrors:true,userAgent:desktop});
  await blocked.addInitScript(()=>Object.defineProperty(window,"localStorage",{get(){throw new DOMException("blocked","SecurityError");}}));
  const blockedPage=await blocked.newPage();await blockedPage.goto(`${service}/r/${codeA}`);await blockedPage.waitForFunction(()=>Boolean(window.tracker));
  await blockedPage.getByRole("button",{name:"Grant tracking consent",exact:true}).click();assert.equal((await blockedPage.evaluate(()=>tracker.handoff())).candidate_click_ids.length,1);
  await blockedPage.getByRole("button",{name:"Create checkout",exact:true}).click();await blockedPage.waitForFunction(()=>document.getElementById("result").textContent.startsWith("{"));assert.equal(JSON.parse(await blockedPage.locator("#result").textContent()).reason,"attributed");await blocked.close();
  stage="candidate expiry and bounded storage";
  const storagePage=await context.newPage();await storagePage.goto("https://shop.merchant.com/storage");await storagePage.waitForFunction(()=>Boolean(window.tracker));
  await storagePage.evaluate(({installation,a})=>localStorage.setItem(`ob-attribution:1:${installation}:click:${a}`,JSON.stringify({id:a,captured_at:Date.now()-31*24*60*60*1000})),{installation,a});
  await storagePage.getByRole("button",{name:"Grant tracking consent",exact:true}).click();assert.equal((await storagePage.evaluate(()=>tracker.handoff())).candidate_click_ids.length,0);
  const futureKey=await storagePage.evaluate(installation=>{const id=crypto.randomUUID(),key=`ob-attribution:1:${installation}:click:${id}`;localStorage.setItem(key,JSON.stringify({id,captured_at:Date.now()+86400000}));return key;},installation);
  assert.equal((await storagePage.evaluate(()=>tracker.handoff())).candidate_click_ids.length,0,"future timestamps remain invalid");
  assert.equal(await storagePage.evaluate(key=>localStorage.getItem(key),futureKey),null);
  await storagePage.evaluate(installation=>{for(let i=0;i<40;i++){const id=crypto.randomUUID();localStorage.setItem(`ob-attribution:1:${installation}:click:${id}`,JSON.stringify({id,captured_at:Date.now()}));}},installation);
  assert.equal((await storagePage.evaluate(()=>tracker.handoff())).candidate_click_ids.length,32);
  assert.equal(await storagePage.evaluate(()=>Object.keys(localStorage).filter(k=>k.includes(":click:")).length),32);
  await storagePage.getByRole("button",{name:"Withdraw tracking consent",exact:true}).click();await storagePage.close();
  for (const beforeRead of [true,false]) {
  stage=beforeRead ? "competing tabs preserve a click written before storage validation" : "competing tabs preserve a click written during cleanup";
  const raceContext=await browser.newContext({ignoreHTTPSErrors:true,userAgent:desktop});
  const tabA=await raceContext.newPage(),tabB=await raceContext.newPage();
  await tabA.goto(`${service}/r/${codeA}`);await tabA.waitForFunction(()=>Boolean(window.tracker));
  await tabA.evaluate(()=>tracker.setConsent(true));
  const raceA=(await tabA.evaluate(()=>tracker.handoff())).candidate_click_ids[0];
  await tabB.goto(`${service}/r/${codeB}`);await tabB.waitForFunction(()=>Boolean(window.tracker));
  const marker=beforeRead ? "if (Number(store.getItem(withdrawal))>grantedAt)" : "if(!memory.has(key.slice(prefix.length)))";
  const boundaryLine=readFileSync("assets/sdk-1.0.0.js","utf8").split("\n").findIndex(line=>line.includes(marker));
  assert.ok(boundaryLine>=0,"locate the production storage boundary");
  const cdp=await raceContext.newCDPSession(tabA);await cdp.send("Debugger.enable");
  const breakpoint=await cdp.send("Debugger.setBreakpointByUrl",{url:`${service}/sdk/1.0.0.js`,lineNumber:boundaryLine,columnNumber:6});
  const paused=new Promise(resolve=>cdp.once("Debugger.paused",resolve));
  const reading=tabA.evaluate(()=>tracker.handoff());await paused;
  if(beforeRead) await new Promise(resolve=>setTimeout(resolve,30));
  await tabB.evaluate(()=>tracker.setConsent(true));
  const raceB=(await tabB.evaluate(()=>tracker.handoff())).candidate_click_ids.find(id=>id!==raceA);assert.ok(raceB);
  await cdp.send("Debugger.removeBreakpoint",{breakpointId:breakpoint.breakpointId});await cdp.send("Debugger.resume");await reading;
  assert.ok(await tabB.evaluate(({installation,raceB})=>localStorage.getItem(`ob-attribution:1:${installation}:click:${raceB}`),{installation,raceB}),"cleanup must preserve a concurrently added candidate");
  await tabB.goto("https://shop.merchant.com/race-return");await tabB.waitForFunction(()=>Boolean(window.tracker));await tabB.evaluate(()=>tracker.setConsent(true));
  const durable=(await tabB.evaluate(()=>tracker.handoff())).candidate_click_ids;assert.ok(durable.includes(raceA));assert.ok(durable.includes(raceB),"return visit reads the persisted newer click");
  await raceContext.close();
  }
  stage="withdrawal cancels queued retries";
  const abortContext=await browser.newContext({ignoreHTTPSErrors:true,userAgent:desktop});let attempts=0;
  await abortContext.route(`${service}/v1/events`,route=>{if(route.request().method()!=="POST")return route.continue();attempts++;return route.fulfill({status:503,headers:{"Access-Control-Allow-Origin":"https://shop.merchant.com","Retry-After":"1"},body:""});});
  const abortPage=await abortContext.newPage();await abortPage.goto(`${service}/r/${codeA}`);await abortPage.waitForFunction(()=>Boolean(window.tracker));await abortPage.getByRole("button",{name:"Grant tracking consent",exact:true}).click();await until(()=>attempts===1,"first failed attempt");
  await abortPage.getByRole("button",{name:"Withdraw tracking consent",exact:true}).click();await abortPage.waitForTimeout(1300);assert.equal(attempts,1);await abortContext.close();
  stage="privacy and credentials remain on merchant server";
  assert.ok(!html().includes(key));assert.ok(deliveries.length>=4);
  for(const request of await Promise.all(emitted)) {
    assert.equal(request.headers.authorization,undefined);assert.equal(request.headers.cookie,undefined);assert.equal(request.headers.referer ?? "", "", "no referrer information may leave the merchant page");
    assert.equal(Object.keys(request.body).sort().join(","),"click_id,consent,event_id,installation_id,kind,link_id,observed_at");
    assert.ok(!JSON.stringify(request).includes(key));assert.ok(!JSON.stringify(request).includes("do-not-collect"));
  }
  console.log(`PASS: website SDK and merchant cart journey: consent, AdA/AdB, URL removal, return history, exact retry, viewport duration, immutable server checkout, multiple tabs, competing before-read and before-cleanup writes persist after navigation, withdrawal, no-reference visit, blocked storage, invalid candidate fallbacks, history expiry/bound, cancelled retries, privacy (${emitted.length} observed browser attempts)`);
} catch(error) {
  console.error(`FAIL: website browser stage ${stage}: ${String(error.message).replaceAll(key,"[redacted]")}`);process.exitCode=1;
} finally {
  await browser?.close();if(merchant)await new Promise(resolve=>merchant.close(resolve));if(directory)rmSync(directory,{recursive:true,force:true});
}
