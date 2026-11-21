import assert from "node:assert/strict";
import https from "node:https";
import {readFileSync} from "node:fs";
import {pathToFileURL} from "node:url";
import {randomUUID} from "node:crypto";
const modulePath=process.env.ATTRIBUTION_PLAYWRIGHT_MODULE;
const {chromium}=modulePath?await import(pathToFileURL(modulePath).href):await import("playwright");
const origin=process.env.ATTRIBUTION_PUBLIC_ORIGIN, openbot=process.env.ATTRIBUTION_FIXTURE_OPENBOT_ORIGIN;
const company=process.env.ATTRIBUTION_FIXTURE_COMPANY, ca=readFileSync(process.env.ATTRIBUTION_FIXTURE_CA);
let stage="launch",browser,page,cookies,csrf;
function request(path,{method="GET",body,headers={}}={}) {
  const bytes=body===undefined?undefined:JSON.stringify(body);
  return new Promise((resolve,reject)=>{
    const req=https.request(new URL(path,origin),{method,ca,lookup:(_host,options,done)=>options?.all?done(null,[{address:"127.0.0.1",family:4}]):done(null,"127.0.0.1",4),headers:{...(bytes?{"content-type":"application/json","content-length":Buffer.byteLength(bytes)}:{}),...headers}},res=>{
      let data="";res.on("data",part=>{data+=part;if(data.length>2000000)req.destroy();});
      res.on("end",()=>{let value;try{value=JSON.parse(data);}catch{}resolve({status:res.statusCode,headers:res.headers,value,text:data});});
    });req.setTimeout(15000,()=>req.destroy());req.on("error",reject);req.end(bytes);
  });
}
async function control(action,other={}) {const r=await fetch(process.env.ATTRIBUTION_FIXTURE_CONTROL,{method:"POST",body:JSON.stringify({action,...other})});assert.equal(r.status,200);return r.json();}
async function user(path,method="GET",body) {return request(path,{method,body,headers:{cookie:cookies,"x-company-id":company,origin,...(csrf?{"x-csrf-token":csrf}:{}),...(method!=="GET"?{"idempotency-key":randomUUID(),"idempotency-created-at":new Date().toISOString()}:{})}});}
async function create(path,body) {const r=await user(path,"POST",body);assert.ok([200,201].includes(r.status));return r.value;}
try {
 browser=await chromium.launch({headless:true,args:["--host-resolver-rules=MAP *.fixture.test 127.0.0.1","--no-proxy-server",`--ignore-certificate-errors-spki-list=${process.env.ATTRIBUTION_FIXTURE_CERT_PIN}`]});
 const context=await browser.newContext();page=await context.newPage();
 stage="real Openbot password and PKCE exchange over HTTPS";
 await page.goto(origin+"/auth/login");stage="Openbot login location";await page.waitForURL(openbot+"/login");
 await page.locator('input[name="email_or_username"]').fill(process.env.ATTRIBUTION_FIXTURE_USERNAME);
 await page.locator('input[name="password"]').fill(process.env.ATTRIBUTION_FIXTURE_PASSWORD);
 await page.locator("form").filter({has:page.locator('input[name="password"]')}).locator('button[type="submit"]').click();
 stage="Openbot login completion";await page.waitForURL(origin+"/");
 cookies=(await context.cookies(origin)).map(c=>`${c.name}=${c.value}`).join("; ");
 stage="standalone HTTPS session read";const session=await user("/v1/session");assert.equal(session.status,200);assert.equal(session.value.user_id,process.env.ATTRIBUTION_FIXTURE_USER_ID);csrf=session.value.csrf_token;
 assert.ok((await context.cookies(origin)).every(c=>c.secure&&c.httpOnly));
 assert.ok(!(await context.cookies(origin)).some(c=>c.name==="session"));
 await page.goto(origin+"/?company_id="+company);await page.getByRole("heading",{name:"Topology fixture",exact:true}).waitFor();
 stage="authorized merchant setup";
 const store=await create("/v1/stores",{name:"Topology shop",timezone:"UTC"});
 const domain=await create("/v1/domains",{store_id:store.id,host:"shop.merchant.com"});
 // Public authoritative DNS is outside this local topology. The existing real
 // DNS fixture covers proof ownership; this fixture seeds only its approved state.
 await control("verify-fixture-domain",{id:domain.domain.id});
 const campaign=await create("/v1/campaigns",{store_id:store.id,name:"Topology campaign"});
 const ad=await create("/v1/ads",{store_id:store.id,campaign_id:campaign.id,name:"Topology ad",advertised_product_id:"product-x"});
 const link=await create("/v1/links",{store_id:store.id,ad_id:ad.id,destination:"https://shop.merchant.com/product",placement:"topology"});
 const key=await create("/v1/credentials",{store_id:store.id,label:"Topology backend",scopes:["checkouts:write","conversions:write","analytics:read"],expires_at:null});
 const merchant=(path,body)=>request(path,{method:body===undefined?"GET":"POST",body,headers:{authorization:"Bearer "+key.secret}});
 stage="durable redirect checkout purchase refund and independent report";
 const click=await request("/r/"+link.code);assert.equal(click.status,302);
 const clickId=new URL(click.headers.location).searchParams.get("ob_click");assert.match(clickId,/^[0-9a-f-]{36}$/);
 const created=(await control("commerce-clock")).now;const checkout={checkout_id:"fixture-checkout",order_created_at:created,consent:true,candidate_click_ids:[clickId]};
 const saved=await merchant("/v1/checkouts",checkout);assert.equal(saved.status,200);assert.equal(saved.value.click_id,clickId);
 const paidTime=(await control("commerce-clock")).now;
 const paid={kind:"paid",event_id:"fixture-payment",order_id:"fixture-order",occurred_at:paidTime,order_created_at:created,paid_at:paidTime,checkout_id:checkout.checkout_id,currency:"EUR",lines:[{line_id:"line-x",product_id:"product-x",quantity:2,merchandise_amount:2400}],merchandise_amount:2400,tax_amount:480,shipping_amount:500};
 const payment=await merchant("/v1/conversions",paid);assert.equal(payment.status,200);assert.equal(payment.value.state,"accepted");
 assert.equal((await merchant("/v1/conversions",paid)).value.id,payment.value.id);
 const refund={kind:"refund",event_id:"fixture-refund",order_id:"fixture-order",refund_id:"refund-x",occurred_at:(await control("commerce-clock")).now,currency:"EUR",lines:[{line_id:"line-x",returned_quantity:1,merchandise_amount:1200}],tax_amount:240,shipping_amount:0};
 assert.equal((await merchant("/v1/conversions",refund)).value.state,"accepted");
 const filter=new URLSearchParams({start:new Date(Date.now()-86400000).toISOString(),end:new Date(Date.now()+86400000).toISOString(),timezone:"UTC"});
 const report=await merchant("/v1/analytics?"+filter);assert.equal(report.status,200);const totals=report.value.groups[0].currencies[0];
 assert.equal(totals.paid_orders,1);assert.equal(totals.refunds,1);assert.equal(totals.net_merchandise_amount,1200);
 assert.equal(report.value.groups[0].attributed_paid_orders,1);
 await page.goto(origin+"/reports?company_id="+company);assert.deepEqual(await page.locator("table").filter({has:page.getByRole("columnheader",{name:"Currency",exact:true})}).locator("tbody tr").first().locator("th,td").allTextContents(),["EUR","1","2","1","1","24.00","12.00","12.00","4.80","2.40","5.00","0.00"]);
 stage="authority outage and durable freshness expiry";
 await control("authority-outage");assert.equal((await user("/v1/session")).status,503);
 assert.equal((await merchant("/v1/checkouts",checkout)).status,200,"warm status permits machine retry within bound");
 await control("expire-status");assert.equal((await merchant("/v1/checkouts",checkout)).status,503);
 assert.equal((await request("/r/"+link.code)).status,503);
 stage="cold restart remains unavailable until authority reconciliation";
 await control("cold-restart");assert.equal((await merchant("/v1/checkouts",checkout)).status,503);
 await control("authority-recover");
 let recovered;
 for(let i=0;i<20;i++){recovered=await merchant("/v1/checkouts",checkout);if(recovered.status===200)break;await new Promise(r=>setTimeout(r,500));}
 assert.equal(recovered.status,200);assert.equal((await user("/v1/session")).status,200);
 assert.equal((await merchant("/v1/analytics?"+filter)).value.groups[0].currencies[0].net_merchandise_amount,1200);
 console.log("PASS actual HTTPS Openbot SSO, independent cookies, scoped merchant setup, durable redirect, consent checkout, purchase replay, refund, API/dashboard ledger, warm outage, forced durable freshness expiry, cold restart, reconciled recovery");
} catch(error) {
 console.error("Topology browser failed at fixed stage: "+stage+" error="+error.name+" path="+(page?new URL(page.url()).pathname:"none"));
 if(page && new URL(page.url()).pathname==="/login") console.error("Login fixture message: "+(await page.locator("body").innerText()).slice(0,1200));process.exitCode=1;
} finally {await browser?.close();}
