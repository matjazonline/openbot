const { chromium } = require('playwright');
const fs = require('fs');
(async()=>{
 const browser=await chromium.launch({headless:true,executablePath:"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"});
 try {
  const page=await browser.newPage();
  const src=fs.readFileSync('src/adapters/http/pages/vault.rs','utf8');
  const form=src.match(/<form hx-delete=[\s\S]*?<\/form>/)[0].replaceAll('{id}','11111111-1111-1111-1111-111111111111').replaceAll('{revision}','1');
  let resolve;
  const request=new Promise(r=>resolve=r);
  await page.route('http://vault-review.test/**', async route=>{
   const r=route.request();
   if(r.method()==='DELETE') {
    resolve({method:r.method(),url:r.url(),body:r.postData(),contentType:r.headers()['content-type']||null});
    await route.fulfill({status:415,body:'Expected form body'});
   } else await route.fulfill({contentType:'text/html',body:'<html><body><div id="vault-content">'+form+'</div><script>'+fs.readFileSync('assets/htmx-2.0.4.min.js','utf8')+'</script></body></html>'});
  });
  await page.goto('http://vault-review.test/');
  await page.getByRole('button',{name:'Remove account'}).click();
  const result=await Promise.race([request,new Promise((_,reject)=>setTimeout(()=>reject(new Error('No DELETE request')),5000))]);
  console.log(JSON.stringify(result));
  if(!result.url.includes('expected_revision=1')||result.body!==null)throw new Error('Unexpected request encoding');
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
