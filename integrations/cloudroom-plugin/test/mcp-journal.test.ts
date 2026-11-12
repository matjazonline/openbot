import test from 'node:test';
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import type { PluginAgentToolContext } from '@get-bb/plugin-sdk';
import { McpIntentJournal, nativeMcpSource } from '../src/mcp-journal.js';
import { JournalCapacity, RecoveryRequired, type Journal } from '../src/journal.js';
import { fixture, operation, send } from './support.js';

function owned(f:ReturnType<typeof fixture>,request='creq_A',journal:Journal=f.journal) {
  const command=send();const mapping=journal.thread('thr_owned');if(mapping)command.session_id=mapping.sessionId;
  const op=operation(command);const record=journal.admit({operation:op,mode:'dispatch'}).record;
  journal.beginIntent(record);
  Object.assign(record,{stage:'observing',hostId:'host_primary',threadId:'thr_owned',requestId:request,turnId:`turn_${request}`,nativeState:'running'});
  journal.evidence(record,{kind:'turn',thread_id:'thr_owned',turn_id:record.turnId!,state:'running',output:null,truncated:false,usage:null},'progress');
  journal.bindSession({sessionId:command.session_id,threadId:'thr_owned',selection:{...f.selection,provider:'claude-code'},
    harness:{kind:'agent',agent_id:randomUUID(),revision_id:randomUUID(),digest:'d'.repeat(64)},operationId:op.id,hostId:'host_primary'});
  journal.acknowledge(journal.sequence());
  const context:PluginAgentToolContext={threadId:'thr_owned',projectId:f.selection.project_id,signal:new AbortController().signal,
    nativeInvocation:Object.freeze({clientRequestId:request,callId:'toolu_A'})};
  return {op,record,context};
}

test('an explicit A descriptor keeps A ownership after B replaces the mapping pointer and cannot borrow a counter',()=>{
  const f=fixture();try {
    const a=owned(f);const prior=f.journal.thread('thr_owned')!;
    const bCommand=send();bCommand.session_id=prior.sessionId;
    const b=f.journal.admit({operation:operation(bCommand),mode:'dispatch'}).record;
    f.journal.referenceOperation(prior.sessionId,b.operation.id);
    const source=nativeMcpSource(f.journal,a.context);
    assert.equal(source.record.operation.id,a.op.id);
    assert.equal(source.mapping.operationId,b.operation.id);
    const intents=new McpIntentJournal(f.journal);
    const first=intents.intent(source,a.context,'crm.lookup','a'.repeat(64));
    assert.equal(first.operation,a.op.id);
    assert.throws(()=>intents.intent(source,a.context,'crm.lookup','b'.repeat(64)),RecoveryRequired);
    assert.throws(()=>nativeMcpSource(f.journal,{...a.context,nativeInvocation:undefined}),RecoveryRequired);
    assert.throws(()=>nativeMcpSource(f.journal,{...a.context,nativeInvocation:Object.freeze({clientRequestId:'call-1',callId:'call-1'})}),RecoveryRequired);
    assert.throws(()=>nativeMcpSource(f.journal,{...a.context,projectId:'foreign'}),RecoveryRequired);
    assert.throws(()=>nativeMcpSource(f.journal,{...a.context,nativeInvocation:{...a.context.nativeInvocation!}}),RecoveryRequired);
  }finally{f.close();}
});

test('exact acknowledged identity is required before creating an intent and duplicate request mappings refuse',()=>{
  const f=fixture();try {
    const a=owned(f);f.journal.db.prepare('UPDATE journal_meta SET acknowledged=acknowledged-1').run();
    assert.throws(()=>nativeMcpSource(f.journal,a.context),RecoveryRequired);
    f.journal.acknowledge(f.journal.sequence());
    const duplicate=f.journal.admit({operation:operation(send()),mode:'dispatch'}).record;
    f.journal.beginIntent(duplicate);Object.assign(duplicate,{threadId:'thr_owned',requestId:'creq_A'});f.journal.save(duplicate);
    assert.throws(()=>nativeMcpSource(f.journal,a.context),RecoveryRequired);
    assert.equal((f.journal.db.prepare('SELECT count(*) AS n FROM mcp_intents').get() as {n:number}).n,0);
  }finally{f.close();}
});

test('physical SQLite reopen preserves immutable invocation, receipt and original absolute deadline',()=>{
  const f=fixture();try {
    const a=owned(f);let intents=new McpIntentJournal(f.journal);
    const first=intents.intent(nativeMcpSource(f.journal,a.context),a.context,'crm.lookup','a'.repeat(64));
    const call=randomUUID(),run=randomUUID(),deadline=new Date(Date.now()+60_000).toISOString();
    const saved=intents.receipt(first,call,run,deadline,false);
    f.journal.db.close();const reopened=f.open();intents=new McpIntentJournal(reopened);
    const replay=intents.intent(nativeMcpSource(reopened,a.context),a.context,'crm.lookup','a'.repeat(64));
    assert.deepEqual(replay,saved);assert(replay.deadline<=first.deadline);
    assert.throws(()=>intents.receipt(replay,randomUUID(),run,deadline,false),RecoveryRequired);
    assert.throws(()=>intents.receipt(replay,call,run,new Date(Date.now()+120_000).toISOString(),false),RecoveryRequired);
  }finally{f.close();}
});

test('record and byte bounds refuse before a second admission while saved identity remains recoverable',()=>{
  for(const limits of [{records:1,bytes:8*1024*1024},{records:4096,bytes:1000}]) {
    const f=fixture();try {
      const a=owned(f);const intents=new McpIntentJournal(f.journal,limits);
      const source=nativeMcpSource(f.journal,a.context);
      const first=intents.intent(source,a.context,'crm.lookup','a'.repeat(64));
      const second={...a.context,nativeInvocation:Object.freeze({...a.context.nativeInvocation!,callId:'toolu_B'})};
      assert.throws(()=>intents.intent(source,second,'crm.lookup','b'.repeat(64)),JournalCapacity);
      assert.deepEqual(intents.intent(source,a.context,'crm.lookup','a'.repeat(64)),first);
    }finally{f.close();}
  }
});

test('unresolved effects pin exact source through collection; resolved collection refuses a later old callback',t=>{
  const f=fixture();try {
    const a=owned(f);const intents=new McpIntentJournal(f.journal);
    let intent=intents.intent(nativeMcpSource(f.journal,a.context),a.context,'crm.lookup','a'.repeat(64));
    const call=randomUUID(),run=randomUUID(),deadline=new Date(Date.now()+60_000).toISOString();
    intent=intents.receipt(intent,call,run,deadline,false);
    a.record.stage='terminal';a.record.nativeState='completed';
    f.journal.evidence(a.record,{kind:'turn',thread_id:'thr_owned',turn_id:a.record.turnId!,state:'completed',output:'done',truncated:false,usage:null},'terminal');
    f.journal.acknowledge(f.journal.sequence());
    const future=Date.now()+100*86_400_000;t.mock.method(Date,'now',()=>future);
    assert.equal(f.journal.compactAcknowledged(),0);
    intents.receipt(intent,call,run,deadline,true);
    assert.equal(f.journal.compactAcknowledged(),1);
    assert.equal((f.journal.db.prepare('SELECT count(*) AS n FROM mcp_intents').get() as {n:number}).n,0);
    assert.throws(()=>nativeMcpSource(f.journal,a.context),RecoveryRequired);
  }finally{t.mock.restoreAll();f.close();}
});
