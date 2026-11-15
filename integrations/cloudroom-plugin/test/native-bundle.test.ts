import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { parse as parseYaml } from 'yaml';
import { NativeBundleCache, hashNativeTree, type NativeReference } from '../src/native-bundle.js';
import { JournalCapacity, RecoveryRequired } from '../src/journal.js';
import { fixture, operation } from './support.js';
import { nativeFixture } from './native-fixture.js';
import { Dispatcher } from '../src/dispatcher.js';
import { commandDigest } from '../src/protocol.js';

function bundle() {
  const body='Use the script, preserving this body.\r\nBODY_TAIL\r\n';
  const value={format:'openbot_native_v2',instructions:'Complete native instructions. '.repeat(300)+'INSTRUCTION_TAIL',permissions:'full',skills:[{
    name:'original-skill',entry:'---\nname: original-skill\ndescription: Native regression\nlicense: MIT\n---\n'+body,
    files:[{path:'scripts/run.sh',content_base64:Buffer.from('#!/bin/sh\ncat "$(dirname "$0")/../.bb-registry-skill.json"\n').toString('base64'),executable:true},
      {path:'.bb-registry-skill.json',content_base64:Buffer.from([0,255,3,128]).toString('base64'),executable:false}],
  }],tools:[]};
  const raw=Buffer.from(JSON.stringify(value));const reference:NativeReference={kind:'agent',agent_id:randomUUID(),revision_id:randomUUID(),digest:createHash('sha256').update(raw).digest('hex')};
  return {raw,reference,body,value};
}
const context=()=>({signal:new AbortController().signal,canDispatch:()=>true});
function nativeContext(f:ReturnType<typeof fixture>,n:ReturnType<typeof nativeFixture>) {
  return {thread:{id:n.state.thread.id,title:null,parentThreadId:null,sourceThreadId:null},project:{id:f.selection.project_id,kind:'standard' as const,name:'Fixture',gitRemoteUrl:null},
    environment:{id:'env_owned',name:null,path:f.directory,branchName:null,workspaceProvisionType:null},host:{id:n.state.hostId,name:'Fixture'},
    provider:{id:f.selection.provider,model:f.selection.model,capabilities:{supportsNativeUserQuestion:false}},origin:{kind:null,pluginId:n.bb.pluginId},signal:new AbortController().signal};
}

test('native immutable cache preserves namespaced YAML, exact assets, modes and tree hashes, and checks server authority on every hit',async()=>{
  const f=fixture(); const b=bundle(); const op=operation({kind:'create',session_id:randomUUID(),selection:f.selection,harness:b.reference,prompt:'native fixture'});
  f.journal.admit({operation:op,mode:'dispatch'});const cache=new NativeBundleCache(f.journal);let reads=0;
  try {
    const reader=async()=>{reads++;return b.raw;};
    const loaded=await cache.load(reader,op.id,b.reference,'admission',context().signal);
    assert(loaded.configuration.instructions.includes('INSTRUCTION_TAIL'));assert(loaded.configuration.instructions.length>4096);
    const descriptor=loaded.configuration.skillDirectories[0]!;const markdown=await fs.readFile(path.join(descriptor.rootPath,'SKILL.md'),'utf8');
    assert(markdown.endsWith('\n'+b.body));const header=/^---\n([\s\S]*?)\n---/.exec(markdown)![1]!;const metadata=parseYaml(header);
    assert.equal(metadata.license,'MIT');assert.match(metadata.name,/^ob-original-skill-[0-9a-f]{32}$/);
    const binary=await fs.readFile(path.join(descriptor.rootPath,'.bb-registry-skill.json'));assert.deepEqual(binary,Buffer.from([0,255,3,128]));
    assert.equal((await fs.stat(path.join(descriptor.rootPath,'scripts/run.sh'))).mode&0o777,0o755);
    assert.equal((await fs.stat(descriptor.rootPath)).mode&0o777,0o700);
    const files=await Promise.all(['SKILL.md','scripts/run.sh','.bb-registry-skill.json'].map(async relative=>({path:relative,bytes:await fs.readFile(path.join(descriptor.rootPath,relative)),mode:(await fs.stat(path.join(descriptor.rootPath,relative))).mode&0o777})));
    assert.equal(hashNativeTree(files),descriptor.expectedTreeHash);
    await cache.load(reader,op.id,b.reference,'admission',context().signal);assert.equal(reads,2);
    await assert.rejects(cache.load(async()=>{throw new RecoveryRequired();},op.id,b.reference,'admission',context().signal),RecoveryRequired);
    await fs.writeFile(path.join(descriptor.rootPath,'unexpected'),'extra');
    await assert.rejects(cache.load(reader,op.id,b.reference,'admission',context().signal),RecoveryRequired);
    assert.equal(await fs.readFile(path.join(descriptor.rootPath,'unexpected'),'utf8'),'extra');
  }finally{f.close();}
});

for(const corruption of ['bytes','mode','symlink','missing'] as const) test(`native cache ${corruption} refuses without reconstructing a mapped bundle`,async()=>{
  const f=fixture();const b=bundle();const session=randomUUID();const op=operation({kind:'create',session_id:session,selection:f.selection,harness:b.reference,prompt:'fixture'});
  f.journal.admit({operation:op,mode:'dispatch'});const cache=new NativeBundleCache(f.journal);
  try{
    const loaded=await cache.load(async()=>b.raw,op.id,b.reference,'admission',context().signal);const root=loaded.configuration.skillDirectories[0]!.rootPath;const asset=path.join(root,'scripts/run.sh');
    f.journal.bindSession({sessionId:session,threadId:'thread_positive',selection:f.selection,harness:b.reference,operationId:op.id,hostId:'host_fixture'});
    if(corruption==='bytes')await fs.writeFile(asset,'changed');
    if(corruption==='mode')await fs.chmod(asset,0o644);
    if(corruption==='symlink'){await fs.unlink(asset);await fs.symlink(path.join(root,'SKILL.md'),asset);}
    if(corruption==='missing')await fs.rm(path.dirname(path.dirname(root)),{recursive:true});
    await assert.rejects(cache.load(async()=>b.raw,op.id,b.reference,'required_configuration',context().signal),RecoveryRequired);
  }finally{f.close();}
});

test('native cache capacity refuses before writes and the download/materialization slot is bounded',async()=>{
  const f=fixture();const b=bundle();const op=operation({kind:'create',session_id:randomUUID(),selection:f.selection,harness:b.reference,prompt:'fixture'});f.journal.admit({operation:op,mode:'dispatch'});
  try{
    const cache=new NativeBundleCache(f.journal,{bytes:1,bundles:128,concurrent:1});
    await assert.rejects(cache.load(async()=>b.raw,op.id,b.reference,'admission',context().signal),JournalCapacity);
    assert.deepEqual(await fs.readdir(path.join(f.directory,'openbot-native-bundles')),[]);
    let release!:(value:Buffer)=>void;const held=new Promise<Buffer>(resolve=>{release=resolve;});
    const first=cache.load(async()=>held,op.id,b.reference,'admission',context().signal);
    await assert.rejects(cache.load(async()=>b.raw,op.id,b.reference,'admission',context().signal),JournalCapacity);
    release(b.raw);await assert.rejects(first,JournalCapacity);
  }finally{f.close();}
});

test('configured spawn holds before positive mapping, durably binds its receipt, then rechecks and supplies full required configuration',async()=>{
  const f=fixture();const b=bundle();const n=nativeFixture(f.journal,f.selection);n.sdk.registerRequiredConfiguration(n.bb);n.sdk.setBundleReader(async()=>b.raw);
  const command={kind:'create' as const,session_id:randomUUID(),selection:f.selection,harness:b.reference,prompt:'native fixture'};const op=operation(command);
  const spawn=n.bb.sdk.threads.spawn;
  n.bb.sdk.threads.spawn=async args=>{
    assert.equal(args.requiredAgentConfiguration,true);
    assert.equal(n.state.dispatchHook!({thread:n.state.thread,requiredAgentConfigurationOwner:n.bb.pluginId}).action,'wait');
    await assert.rejects(Promise.resolve().then(()=>n.state.requiredConfiguration!(nativeContext(f,n))),RecoveryRequired);
    return spawn(args);
  };
  try{
    await new Dispatcher(f.journal,n.sdk).process({operation:op,mode:'dispatch'},context());
    const mapping=f.journal.session(command.session_id)!;assert.deepEqual(mapping.harness,b.reference);assert.equal(mapping.hostId,n.state.hostId);assert.equal(mapping.operationId,op.id);
    assert.equal(n.state.rechecks,1);assert.equal(n.state.dispatchHook!({thread:n.state.thread,requiredAgentConfigurationOwner:n.bb.pluginId}).action,'proceed');
    const configured=await n.state.requiredConfiguration!(nativeContext(f,n));assert(configured.instructions.startsWith(b.value.instructions));assert(configured.instructions.includes('INSTRUCTION_TAIL'));assert.equal(configured.skillDirectories.length,1);
    assert.equal(n.state.dispatchHook!({thread:{id:'ordinary'},requiredAgentConfigurationOwner:null}).action,'proceed');
    await assert.rejects(Promise.resolve().then(()=>n.state.requiredConfiguration!({...nativeContext(f,n),thread:{...nativeContext(f,n).thread,id:'ordinary'}})),RecoveryRequired);
    assert.equal(n.calls.length,1);
  }finally{f.close();}
});

test('configured lost spawn receipt never recovers authority from editable metadata or repeats a spawn',async()=>{
  const f=fixture();const b=bundle();const n=nativeFixture(f.journal,f.selection);n.sdk.registerRequiredConfiguration(n.bb);n.sdk.setBundleReader(async()=>b.raw);n.state.lostResponse=true;
  const command={kind:'create' as const,session_id:randomUUID(),selection:f.selection,harness:b.reference,prompt:'lost fixture'};const op=operation(command);const dispatcher=new Dispatcher(f.journal,n.sdk);
  try{
    await dispatcher.process({operation:op,mode:'dispatch'},context());
    assert.equal(f.journal.get(op.id)!.stage,'uncertain');assert.equal(f.journal.session(command.session_id),undefined);
    await dispatcher.process({operation:op,mode:'reconcile'},context());assert.equal(n.calls.length,1);assert.equal(f.journal.session(command.session_id),undefined);
    await assert.rejects(Promise.resolve().then(()=>n.state.requiredConfiguration!(nativeContext(f,n))),RecoveryRequired);
    assert.equal(n.state.dispatchHook!({thread:n.state.thread,requiredAgentConfigurationOwner:n.bb.pluginId}).action,'wait');
  }finally{f.close();}
});

test('native cache cancellation joins owned writes without publishing a partial bundle',async()=>{
  const f=fixture();const b=bundle();const op=operation({kind:'create',session_id:randomUUID(),selection:f.selection,harness:b.reference,prompt:'cancel'});f.journal.admit({operation:op,mode:'dispatch'});
  const stop=new AbortController();const cache=new NativeBundleCache(f.journal);let release!:(value:Buffer)=>void;
  const body=new Promise<Buffer>(resolve=>{release=resolve;});const pending=cache.load(async()=>body,op.id,b.reference,'admission',stop.signal);const rejected=assert.rejects(pending);
  try{stop.abort();release(b.raw);await rejected;await assert.rejects(fs.stat(path.join(f.directory,'openbot-native-bundles')),/ENOENT/);}
  finally{f.close();}
});

test('native cache keeps command and session references through acknowledgement and retires only the confirmed unchanged mapping',async()=>{
  const f=fixture();const b=bundle();const session=randomUUID();const op=operation({kind:'create',session_id:session,selection:f.selection,harness:b.reference,prompt:'retention'});op.deadline=new Date(Date.now()-1000).toISOString();op.payload_digest=commandDigest(op.command,op.deadline);
  const {record}=f.journal.admit({operation:op,mode:'dispatch'});const cache=new NativeBundleCache(f.journal);const signal=context().signal;
  try{
    const loaded=await cache.load(async()=>b.raw,op.id,b.reference,'admission',signal);const root=loaded.configuration.skillDirectories[0]!.rootPath;
    f.journal.bindSession({sessionId:session,threadId:'thread_positive_retention',selection:f.selection,harness:b.reference,operationId:op.id,hostId:'host_fixture'});
    f.journal.beginIntent(record);record.threadId='thread_positive_retention';record.hostId='host_fixture';f.journal.submission(record,f.selection,'accepted');
    record.stage='terminal';f.journal.evidence(record,{kind:'turn',thread_id:record.threadId,turn_id:'turn_fixture',state:'completed',output:'retained',truncated:false,usage:null},'terminal');
    const old=Date.now()-100*86_400_000;
    f.journal.db.prepare('UPDATE commands SET terminal_at=?').run(old);f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(old);
    await cache.cleanup(signal);assert((await fs.stat(root)).isDirectory());assert.equal(f.journal.compactAcknowledged(),0);assert.deepEqual(f.journal.mappingCandidates(),[]);
    f.journal.acknowledge(f.journal.sequence());f.journal.db.prepare('UPDATE commands SET acknowledged_at=?').run(old);
    assert.equal(f.journal.compactAcknowledged(),1);await cache.cleanup(signal);assert((await fs.stat(root)).isDirectory());
    const candidates=f.journal.mappingCandidates();assert.equal(candidates.length,1);
    f.journal.referenceOperation(session,op.id);assert.equal(f.journal.retireMappings(candidates,[session]),0);
    f.journal.db.prepare('UPDATE sessions SET last_used_at=?').run(old);
    assert.equal(f.journal.retireMappings(f.journal.mappingCandidates(),[session]),1);
    await cache.cleanup(signal);await assert.rejects(fs.stat(root),/ENOENT/);
  }finally{f.close();}
});


test('native cache abort during materialization removes its owned temporary directory before returning',async()=>{
  const f=fixture();const b=bundle();const value={...b.value,skills:Array.from({length:16},(_,index)=>({
    name:`skill-${index}`,entry:`---\nname: skill-${index}\ndescription: Cancellable materialization\n---\nBody`,
    files:Array.from({length:64},(_,file)=>({path:`assets/file-${file}`,content_base64:'eA==',executable:false})),
  }))};
  const raw=Buffer.from(JSON.stringify(value));const reference={...b.reference,digest:createHash('sha256').update(raw).digest('hex')};
  const op=operation({kind:'create',session_id:randomUUID(),selection:f.selection,harness:reference,prompt:'cancel writes'});f.journal.admit({operation:op,mode:'dispatch'});
  const cache=new NativeBundleCache(f.journal);const stop=new AbortController();const pending=cache.load(async()=>raw,op.id,reference,'admission',stop.signal);const rejected=assert.rejects(pending);
  try{
    const root=path.join(f.directory,'openbot-native-bundles');let temporary=false;const until=Date.now()+5000;
    while(!temporary && Date.now()<until){temporary=await fs.readdir(root).then(entries=>entries.some(name=>name.startsWith('tmp-')),()=>false);if(!temporary)await delay(1);}
    assert(temporary,'materialization must start before cancellation');stop.abort();await rejected;
    assert.deepEqual(await fs.readdir(root),[]);
  }finally{stop.abort();await pending.catch(()=>{});f.close();}
});

test('native cache inventory rejects unexpected bytes in another bundle before admitting new materialization',async()=>{
  const f=fixture();const first=bundle();const second=bundle();second.value.instructions+=' another immutable bundle';second.raw=Buffer.from(JSON.stringify(second.value));second.reference.digest=createHash('sha256').update(second.raw).digest('hex');
  const cache=new NativeBundleCache(f.journal);const signal=context().signal;
  try{
    const loaded=await cache.load(async()=>first.raw,randomUUID(),first.reference,'admission',signal);
    await fs.writeFile(path.join(loaded.configuration.skillDirectories[0]!.rootPath,'unexpected'),'unaccounted bytes');
    await assert.rejects(cache.load(async()=>second.raw,randomUUID(),second.reference,'admission',signal),RecoveryRequired);
    await assert.rejects(fs.stat(path.join(f.directory,'openbot-native-bundles',second.reference.digest)),/ENOENT/);
  }finally{f.close();}
});


test('native handoff rejects case-colliding directory components before writes and preserves identical repeated parents',async()=>{
  const f=fixture();const b=bundle();const cache=new NativeBundleCache(f.journal);const signal=context().signal;
  try {
    for(const names of [["Scripts/one.txt","scripts/two.txt"],["a/Files/one","a/files/two"],["scripts/one.txt","scripts/two.txt"]]) {
      b.value.skills[0]!.files=names.map(name=>({path:name,content_base64:'eA==',executable:false}));
      const raw=Buffer.from(JSON.stringify(b.value));const reference={...b.reference,digest:createHash('sha256').update(raw).digest('hex')};
      if(names[0]==='scripts/one.txt') {
        const loaded=await cache.load(async()=>raw,randomUUID(),reference,'admission',signal);
        for(const name of names)assert.deepEqual(await fs.readFile(path.join(loaded.configuration.skillDirectories[0]!.rootPath,name)),Buffer.from('x'));
      }else {
        await assert.rejects(cache.load(async()=>raw,randomUUID(),reference,'admission',signal),RecoveryRequired);
        await assert.rejects(fs.stat(path.join(f.directory,'openbot-native-bundles',reference.digest)),/ENOENT/);
      }
    }
  }finally{f.close();}
});

test('MCP contracts retain exact schema bytes and reject substituted binding/source identities before cache writes',async()=>{
  const f=fixture();
  try {
    const item=bundle();const binding=randomUUID();const connection=randomUUID();const version=randomUUID();
    const contract=`{"binding":{"id":"${binding}","alias":"crm","source":{"kind":"mcp","connection":"${connection}","operation":"lookup","reviewed_version":"${version}","remote_name":"lookup"}},"input_schema":{"type":"object","maximum":18446744073709551615},"output_schema":true,"policy":{"capability":"mcp.call","policy_revision":1,"effect":"write","recovery":"reconcile"},"approval_required":true}`;
    const withTools={...item.value,tools:[{binding_id:binding,alias:'crm',contract_base64:Buffer.from(contract).toString('base64')}]};
    const raw=Buffer.from(JSON.stringify(withTools));const reference={...item.reference,digest:createHash('sha256').update(raw).digest('hex')};
    const loaded=await new NativeBundleCache(f.journal).load(async()=>raw,randomUUID(),reference,'admission',new AbortController().signal);
    const {nativeMcpContract}=await import('../src/native-bundle.js');
    assert.equal(nativeMcpContract(loaded.bundle.tools[0]!),contract);
    for(const change of [
      {...withTools,format:'openbot_native_v1'},
      {...withTools,tools:[{...withTools.tools[0]!,binding_id:randomUUID()}]},
      {...withTools,tools:[{...withTools.tools[0]!,contract_base64:Buffer.from(contract.replace('"kind":"mcp"','"kind":"http"')).toString('base64')}]},
      {...withTools,tools:[withTools.tools[0]!,withTools.tools[0]!]},
    ]) {
      const changed=Buffer.from(JSON.stringify(change));const target={...reference,digest:createHash('sha256').update(changed).digest('hex')};
      await assert.rejects(new NativeBundleCache(f.journal).load(async()=>changed,randomUUID(),target,'admission',new AbortController().signal));
    }
  } finally {f.close();}
});
