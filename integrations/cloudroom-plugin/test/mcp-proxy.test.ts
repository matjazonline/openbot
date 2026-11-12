import test from 'node:test';
import assert from 'node:assert/strict';
import { setTimeout as delay } from 'node:timers/promises';
import { NativeMcpProxy } from '../src/mcp-proxy.js';
import { Dispatcher } from '../src/dispatcher.js';
import { nativeMcpRequest } from '../src/mcp-transport.js';
import { nativeMcpSource } from '../src/mcp-journal.js';
import { mcpFixture } from './mcp-fixture.js';
import { server,waitFor } from './connection-support.js';
import { operation } from './support.js';
import { nativeFixture } from './native-fixture.js';

test('MCP native preflight refuses an older core before any provider mutation; instructions-only still launch',async()=>{
  const f=mcpFixture(undefined,false);
  try {
    await new Dispatcher(f.journal,f.n.sdk).process({operation:f.op,mode:'dispatch'},{signal:f.stop.signal,canDispatch:()=>true});
    assert.equal(f.n.calls.length,0);assert.equal(f.journal.get(f.op.id)!.stage,'terminal');
    assert.match(JSON.stringify(f.journal.get(f.op.id)!.lastObservation),/provider_unavailable/);
  }finally{f.close();}
  const instructions=mcpFixture(undefined,false,false);
  try {
    await new Dispatcher(instructions.journal,instructions.n.sdk).process({operation:instructions.op,mode:'dispatch'},{signal:instructions.stop.signal,canDispatch:()=>true});
    assert.equal(instructions.n.calls.length,1);assert.equal(instructions.n.calls[0]!.kind,'spawn');
  }finally{instructions.close();}
});

test('registered MCP proxy lists and describes the exact frozen contract without creating effect intents',async()=>{
  const f=mcpFixture();await f.acknowledge();
  const proxy=new NativeMcpProxy(f.journal,f.n.sdk,{current:()=>f.transport,latest:()=>f.transport});
  try {
    proxy.register(f.n.bb);assert.equal(f.n.state.tools.length,1);assert.equal(f.n.state.tools[0]!.name,'openbot_company_mcp');
    assert.equal(await proxy.execute({kind:'describe',alias:'crm'},f.context),f.contract);
    assert.match(String(await proxy.execute({kind:'list'},f.context)),/crm/);
    const ordinary=await proxy.execute({kind:'list'},{...f.context,threadId:'ordinary'});
    assert.equal(typeof ordinary==='object' && ordinary.isError,true);
    assert.equal((f.journal.db.prepare('SELECT count(*) AS n FROM mcp_intents').get() as {n:number}).n,0);
    assert.equal(f.n.calls.length,0);
  }finally{f.close();}
});

test('TLS proxy journals before POST, retries a lost response with the same owner, rotates credentials and holds through pending approval',async()=>{
  const s=await server();const f=mcpFixture(s.origin);await f.acknowledge();
  let current=f.transport,calls=0,polls=0,approved=false,settled=false;
  const failures:unknown[]=[];const requests:any[]=[];
  s.https.on('request',async(req,res)=>{
    try {
      const chunks:Buffer[]=[];for await(const chunk of req)chunks.push(chunk);
      const body=JSON.parse(Buffer.concat(chunks).toString());requests.push(body);
      assert.equal(req.url,`/v1/local-machines/operations/${f.op.id}/mcp`);
      assert.deepEqual(body.invocation,f.receipt.origin.invocation);
      assert.equal(req.headers['x-openbot-installation-id'],f.binding.installation_id);
      assert.equal(req.headers['x-openbot-protocol-version'],'2');
      assert.equal((f.journal.db.prepare('SELECT count(*) AS n FROM mcp_intents').get() as {n:number}).n,1,'intent must be durable before admission');
      if(body.action.kind==='call') {
        calls++;assert.deepEqual(body.action.arguments,{query:'exact',nativeInvocation:{clientRequestId:'forged',callId:'forged'}});
        if(calls===1){assert.equal(req.headers.authorization,`Bearer ${'a'.repeat(64)}`);current={...current,paired:{...current.paired,credential:'b'.repeat(64)},identity:{...current.identity,credential_generation:2,connection_generation:2}};req.socket.destroy();return;}
      }else {polls++;assert.equal(body.action.call_id,f.receipt.call_id);}
      assert.equal(req.headers.authorization,`Bearer ${'b'.repeat(64)}`);
      assert.equal(req.headers['x-openbot-credential-generation'],'2');assert.equal(req.headers['x-openbot-connection-generation'],'2');
      res.setHeader('content-type','application/json');
      res.end(JSON.stringify({...f.receipt,kind:approved?'completed':'pending',...(approved?{result:{content:[{type:'text',text:'crm result'},{type:'image',data:'AQI=',mimeType:'image/png'}],structuredContent:{exact:true},_meta:{fixture:true}}}:{})}));
    }catch(error){failures.push(error);res.writeHead(500).end();}
  });
  const proxy=new NativeMcpProxy(f.journal,f.n.sdk,{current:()=>current,latest:()=>current});
  const pending=proxy.execute({kind:'call',alias:'crm',arguments:{query:'exact',nativeInvocation:{clientRequestId:'forged',callId:'forged'}}},f.context).finally(()=>{settled=true;});
  try {
    await waitFor(()=>polls>=1);await delay(15_100);assert.equal(settled,false,'approval must outlive one HTTP budget without detaching native tool');
    assert.equal(calls,2);assert(polls>1);approved=true;
    const result=await pending;assert.equal(typeof result,'object');assert(result && typeof result==='object' && !result.isError);
    assert.deepEqual(result.content.slice(0,2),[{type:'text',text:'crm result'},{type:'image',data:'AQI=',mimeType:'image/png'}]);
    assert.match(JSON.stringify(result),/structuredContent/);assert.match(JSON.stringify(result),/workflow-runs/);
    assert.equal((f.journal.db.prepare('SELECT resolved FROM mcp_intents').get() as {resolved:number}).resolved,1);
    assert.deepEqual(failures,[]);assert(requests.every(request=>request.invocation.client_request_id==='creq_A'));
  }finally{f.stop.abort();await pending.catch(()=>{});f.close();await s.close();}
});

test('native abort cancels the immutable invocation even when the admission response was lost',async()=>{
  const s=await server();const f=mcpFixture(s.origin);await f.acknowledge();const actions:string[]=[];
  s.https.on('request',async(req,res)=>{
    const chunks:Buffer[]=[];for await(const chunk of req)chunks.push(chunk);const body=JSON.parse(Buffer.concat(chunks).toString());actions.push(body.action.kind);
    if(body.action.kind==='call'){req.socket.destroy();return;}
    res.end(JSON.stringify({...f.receipt,kind:'cancelled'}));
  });
  const abort=new AbortController();const proxy=new NativeMcpProxy(f.journal,f.n.sdk,{current:()=>f.transport,latest:()=>f.transport});
  const pending=proxy.execute({kind:'call',alias:'crm',arguments:{}},{...f.context,signal:abort.signal});const rejected=assert.rejects(pending);
  try {
    await waitFor(()=>actions.length===1);abort.abort();await rejected;
    assert.deepEqual(actions,['call','cancel_invocation']);
    const row=f.journal.db.prepare('SELECT state,resolved FROM mcp_intents').get() as {state:string;resolved:number};
    assert.equal(row.resolved,1,'authenticated terminal cancellation closes the durable original intent');
    assert.equal(JSON.parse(row.state).receipt.call,f.receipt.call_id,'the cancellation reply recovers the lost admission receipt');
  }finally{f.close();await s.close();}
});

test('only a proven first-attempt admission rejection retires after restart; ambiguous errors retain the intent',async t=>{
  for(const mode of ['definite','generic','lost'] as const) {
    const s=await server();const f=mcpFixture(s.origin);await f.acknowledge();let calls=0,cancels=0;
    s.https.on('request',async(req,res)=>{
      const chunks:Buffer[]=[];for await(const chunk of req)chunks.push(chunk);
      const body=JSON.parse(Buffer.concat(chunks).toString());
      if(body.action.kind==='cancel_invocation'){cancels++;res.writeHead(404).end();return;}
      calls++;
      if(mode==='lost' && calls===1){req.socket.destroy();return;}
      if(mode!=='generic')res.setHeader('x-openbot-mcp-not-admitted','invalid_arguments');
      res.writeHead(400).end();
    });
    try {
      const input={kind:'call',alias:'crm',arguments:{}};
      const first=new NativeMcpProxy(f.journal,f.n.sdk,{current:()=>f.transport,latest:()=>f.transport});
      const result=await first.execute(input,f.context);
      assert(result && typeof result==='object' && result.isError);
      const beforeRestart=calls;
      f.journal.db.close();const journal=f.open();const n=nativeFixture(journal,f.selection);
      n.sdk.registerRequiredConfiguration(n.bb);n.sdk.setBundleReader(async()=>f.raw);
      const restarted=new NativeMcpProxy(journal,n.sdk,{current:()=>f.transport,latest:()=>f.transport});
      await restarted.execute(input,f.context);
      const row=journal.db.prepare('SELECT state,resolved,reserve FROM mcp_intents').get() as {state:string;resolved:number;reserve:number};
      assert.equal(row.resolved,Number(mode==='definite'));assert.equal(JSON.parse(row.state).rejected,mode==='definite');
      if(mode==='definite'){assert.equal(calls,beforeRestart,'durable rejection forbids another effect admission');assert.equal(cancels,0);assert.equal(row.reserve,0);}
      else {assert(calls>beforeRestart);assert(cancels>0);assert.equal(row.reserve,256);}
      const record=journal.get(f.op.id)!;record.stage='terminal';record.nativeState='completed';
      journal.evidence(record,{kind:'turn',thread_id:'thr_owned',turn_id:'turn_A',state:'completed',output:'done',truncated:false,usage:null},'terminal');
      journal.acknowledge(journal.sequence());
      const future=Date.now()+100*86_400_000;t.mock.method(Date,'now',()=>future);
      assert.equal(journal.compactAcknowledged(),Number(mode==='definite'));
      assert.equal((journal.db.prepare('SELECT count(*) AS n FROM mcp_intents').get() as {n:number}).n,Number(mode!=='definite'));
    }finally{t.mock.restoreAll();f.close();await s.close();}
  }
});

test('a positively received first rejection is durable even if native cancellation races its response cleanup',async t=>{
  const f=mcpFixture();await f.acknowledge();const abort=new AbortController();let posts=0;
  t.mock.method(globalThis,'fetch',async()=>{
    posts++;
    return new Response(new ReadableStream({cancel(){abort.abort();}}),{
      status:400,headers:{'x-openbot-mcp-not-admitted':'invalid_arguments'},
    });
  });
  try {
    const proxy=new NativeMcpProxy(f.journal,f.n.sdk,{current:()=>f.transport,latest:()=>f.transport});
    await assert.rejects(proxy.execute({kind:'call',alias:'crm',arguments:{}},{...f.context,signal:abort.signal}));
    const row=f.journal.db.prepare('SELECT state,resolved FROM mcp_intents').get() as {state:string;resolved:number};
    assert.equal(row.resolved,1);assert.equal(JSON.parse(row.state).rejected,true);assert.equal(posts,1);
  }finally{t.mock.restoreAll();f.close();}
});

test('unknown outcome survives a physical plugin journal restart; status from B resolves A without another admission',async()=>{
  const s=await server();const f=mcpFixture(s.origin);await f.acknowledge();const requests:any[]=[];let reconciled=false;
  s.https.on('request',async(req,res)=>{
    const chunks:Buffer[]=[];for await(const chunk of req)chunks.push(chunk);
    requests.push({url:req.url,body:JSON.parse(Buffer.concat(chunks).toString())});
    res.end(JSON.stringify({...f.receipt,kind:reconciled?'completed':'reconciliation_required',
      ...(reconciled?{result:{content:[{type:'text',text:'historically reconciled A'}]}}:{})}));
  });
  try {
    const first=new NativeMcpProxy(f.journal,f.n.sdk,{current:()=>f.transport,latest:()=>f.transport});
    const unknown=await first.execute({kind:'call',alias:'crm',arguments:{query:'A'}},f.context);
    assert(unknown && typeof unknown==='object' && unknown.isError);
    assert.equal((f.journal.db.prepare('SELECT resolved FROM mcp_intents').get() as {resolved:number}).resolved,0);
    f.journal.db.close();const journal=f.open();const n=nativeFixture(journal,f.selection);
    n.sdk.registerRequiredConfiguration(n.bb);n.sdk.setBundleReader(async()=>f.raw);
    const mapping=journal.thread('thr_owned')!;
    const op=operation({kind:'send',session_id:mapping.sessionId,thread_id:'thr_owned',prompt:'Read A'});
    const record=journal.admit({operation:op,mode:'dispatch'}).record;journal.beginIntent(record);
    Object.assign(record,{stage:'observing',hostId:mapping.hostId,threadId:'thr_owned',requestId:'creq_B',turnId:'turn_B',nativeState:'running'});
    journal.evidence(record,{kind:'turn',thread_id:'thr_owned',turn_id:'turn_B',state:'running',output:null,truncated:false,usage:null},'progress');
    journal.referenceOperation(mapping.sessionId,op.id);journal.acknowledge(journal.sequence());reconciled=true;
    const restarted=new NativeMcpProxy(journal,n.sdk,{current:()=>f.transport,latest:()=>f.transport});
    const result=await restarted.execute({kind:'status',call_id:f.receipt.call_id},{...f.context,nativeInvocation:Object.freeze({clientRequestId:'creq_B',callId:'toolu_status_B'})});
    assert(result && typeof result==='object' && !result.isError);assert.match(JSON.stringify(result),/historically reconciled A/);
    assert.deepEqual(requests.map(request=>request.body.action.kind),['call','status']);
    assert.equal(requests[1].url,`/v1/local-machines/operations/${op.id}/mcp`);
    assert.equal(requests[1].body.invocation.client_request_id,'creq_B');
    assert.equal((journal.db.prepare('SELECT count(*) AS n FROM mcp_intents').get() as {n:number}).n,1);
    assert.equal((journal.db.prepare('SELECT resolved FROM mcp_intents').get() as {resolved:number}).resolved,1);
  }finally{f.close();await s.close();}
});

test('MCP HTTPS transport refuses oversized replies, redirects, invalid metadata and an individual request deadline',async()=>{
  const s=await server();const f=mcpFixture(s.origin);await f.acknowledge();let mode='size';let foreign=0;
  s.https.on('request',(req,res)=>{
    req.resume();
    if(req.url==='/foreign'){foreign++;res.end('{}');return;}
    if(mode==='size'){res.setHeader('content-length',String(2*1024*1024+1));res.end();}
    else if(mode==='redirect')res.writeHead(307,{location:s.origin+'/foreign'}).end();
    else if(mode==='metadata')res.end(JSON.stringify({...f.receipt,kind:'pending',origin:{...f.receipt.origin,invocation:{...f.receipt.origin.invocation,model_call_id:''}}}));
    else {res.writeHead(200);res.write('{');}
  });
  const source=nativeMcpSource(f.journal,f.context);const request=()=>nativeMcpRequest(f.transport,source,'toolu_A',{kind:'status',call_id:f.receipt.call_id},f.stop.signal,50);
  try {for(mode of ['size','redirect','metadata','timeout'])await assert.rejects(request());assert.equal(foreign,0);}
  finally {f.stop.abort();f.close();s.https.closeAllConnections();await s.close();}
});
