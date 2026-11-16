import type { PluginAgentToolContext } from '@get-bb/plugin-sdk';
import { createHash, randomUUID } from 'node:crypto';
import { fixture, operation } from './support.js';
import { nativeFixture } from './native-fixture.js';
import type { NativeMcpConnection } from '../src/mcp-transport.js';
import { NativeBundleCache } from '../src/native-bundle.js';

export function mcpFixture(origin='https://openbot.example', verified=true, withTools=true) {
  const f=fixture({initialized:false});f.binding.origin=origin;f.journal.initialize(f.binding);
  const selection={...f.selection,provider:'claude-code' as const};
  const binding=randomUUID(),connection=randomUUID(),version=randomUUID();
  const contract=`{"binding":{"id":"${binding}","alias":"crm","source":{"kind":"mcp","connection":"${connection}","operation":"lookup","reviewed_version":"${version}","remote_name":"lookup"}},"input_schema":{"type":"object","maximum":18446744073709551615},"output_schema":true,"policy":{"capability":"mcp.call","policy_revision":1,"effect":"write","recovery":"reconcile"},"approval_required":true}`;
  const bundle={format:'openbot_native_v2',instructions:'Use the existing company CRM.',permissions:'full',skills:[],
    tools:withTools?[{binding_id:binding,alias:'crm',contract_base64:Buffer.from(contract).toString('base64')}]:[]};
  const raw=Buffer.from(JSON.stringify(bundle));
  const harness={kind:'agent' as const,agent_id:randomUUID(),revision_id:randomUUID(),digest:createHash('sha256').update(raw).digest('hex')};
  const op=operation({kind:'create',session_id:randomUUID(),selection,harness,prompt:'Use CRM'});
  const n=nativeFixture(f.journal,selection);
  if(!verified)Reflect.deleteProperty(n.bb.agents,'nativeToolOriginVersion');
  n.sdk.registerRequiredConfiguration(n.bb);n.sdk.setBundleReader(async()=>raw);
  const record=f.journal.admit({operation:op,mode:'dispatch'}).record;
  async function acknowledge() {
    await new NativeBundleCache(f.journal).load(async()=>raw,op.id,harness,'admission',new AbortController().signal);
    f.journal.beginIntent(record);
    Object.assign(record,{stage:'observing',hostId:'host_primary',threadId:'thr_owned',requestId:'creq_A',turnId:'turn_A',nativeState:'running'});
    f.journal.evidence(record,{kind:'turn',thread_id:'thr_owned',turn_id:'turn_A',state:'running',output:null,truncated:false,usage:null},'progress');
    f.journal.bindSession({sessionId:op.command.kind==='create'?op.command.session_id:'',threadId:'thr_owned',selection,harness,operationId:op.id,hostId:'host_primary'});
    f.journal.acknowledge(f.journal.sequence());
  }
  const context:PluginAgentToolContext={threadId:'thr_owned',projectId:selection.project_id,signal:new AbortController().signal,
    nativeInvocation:Object.freeze({clientRequestId:'creq_A',callId:'toolu_A'})};
  const stop=new AbortController();
  const transport:NativeMcpConnection={paired:{state:'paired',version:1,binding:f.binding,credential:'a'.repeat(64)},
    identity:{installation_id:f.binding.installation_id,credential_generation:1,connection_generation:1},signal:stop.signal};
  const receipt={call_id:randomUUID(),run_id:randomUUID(),deadline:new Date(Date.now()+14*60_000).toISOString(),
    origin:{operation_id:op.id,invocation:{thread_id:'thr_owned',turn_id:'turn_A',client_request_id:'creq_A',model_call_id:'toolu_A'}}};
  return {...f,n,selection,raw,harness,contract,op,record,acknowledge,context,transport,stop,receipt};
}
