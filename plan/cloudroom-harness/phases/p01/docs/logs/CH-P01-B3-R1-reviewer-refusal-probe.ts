import { NativeMcpProxy } from '/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree/integrations/cloudroom-plugin/src/mcp-proxy.ts';
import { mcpFixture } from '/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree/integrations/cloudroom-plugin/test/mcp-fixture.ts';
(async()=>{
 const f=mcpFixture();const originalFetch=globalThis.fetch;const originalNow=Date.now;
 try {
  await f.acknowledge();const actions:string[]=[];
  globalThis.fetch=async(_url,options)=>{const kind=JSON.parse(String(options?.body)).action.kind;actions.push(kind);return new Response('{}',{status:kind==='call'?400:404});};
  const proxy=new NativeMcpProxy(f.journal,f.n.sdk,{current:()=>f.transport,latest:()=>f.transport});
  const result=await proxy.execute({kind:'call',alias:'crm',arguments:{}},f.context);
  f.record.stage='terminal';f.record.nativeState='completed';
  f.journal.evidence(f.record,{kind:'turn',thread_id:'thr_owned',turn_id:'turn_A',state:'completed',output:'done',truncated:false,usage:null},'terminal');
  f.journal.acknowledge(f.journal.sequence());
  const future=Date.now()+100*86_400_000;Date.now=()=>future;
  const removed=f.journal.compactAcknowledged();
  console.log(JSON.stringify({case:'Definitive admission HTTP 400, cancel lookup HTTP 404, source terminal/acked/100 days old',actions,result,removed,intent:f.journal.db.prepare('SELECT state,resolved FROM mcp_intents').get(),commands:f.journal.db.prepare('SELECT count(*) AS n FROM commands').get()},null,2));
 }finally{globalThis.fetch=originalFetch;Date.now=originalNow;f.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
