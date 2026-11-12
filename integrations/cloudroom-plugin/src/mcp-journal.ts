import { createHash } from 'node:crypto';
import type { PluginAgentToolContext } from '@get-bb/plugin-sdk';
import { z } from 'zod';
import { Journal, JournalCapacity, RecoveryRequired, type CommandRecord, type SessionMapping } from './journal.js';
import { external, uuid, LIMITS, isEffect, observationSequence } from './protocol.js';
import type { McpResponse } from './mcp-transport.js';

export const MCP_INTENT_LIMITS=Object.freeze({records:4096,bytes:8*1024*1024});
const hash=z.string().regex(/^[0-9a-f]{64}$/);
const intentSchema=z.object({installation:uuid,thread:external,request:external,modelCall:external,
  operation:uuid,session:uuid,harness:hash,turn:external,alias:external,
  argumentDigest:hash,deadline:z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
  receipt:z.object({call:uuid,run:uuid,deadline:z.string().max(64)}).strict().nullable(),resolved:z.boolean(),
  attempted:z.boolean(),rejected:z.boolean()}).strict();
export type McpIntent=z.infer<typeof intentSchema>;
export type McpSource={record:CommandRecord;mapping:SessionMapping};
type Row={state:string;resolved:number;bytes:number;reserve:number;operation_id:string};
const digest=(value:string)=>createHash('sha256').update(value).digest('hex');
const key=(intent:Pick<McpIntent,'installation'|'thread'|'request'|'modelCall'>)=>digest(JSON.stringify([intent.installation,intent.thread,intent.request,intent.modelCall]));

/** Never infer an invocation owner from the mapping's mutable operationId. */
export function nativeMcpSource(journal:Journal,context:PluginAgentToolContext):McpSource {
  const invocation=context.nativeInvocation;
  if(!invocation || !Object.isFrozen(invocation)) throw new RecoveryRequired();
  external.parse(invocation.clientRequestId);external.parse(invocation.callId);
  const mapping=journal.thread(context.threadId);
  if(!mapping || mapping.harness.kind!=='agent' || mapping.selection.provider!=='claude-code'
      || mapping.selection.project_id!==context.projectId || journal.recoveryRequired()) throw new RecoveryRequired();
  const matches=journal.db.prepare(`SELECT id FROM commands WHERE json_extract(state,'$.threadId')=?
      AND json_extract(state,'$.requestId')=? LIMIT 2`).all(context.threadId,invocation.clientRequestId) as Array<{id:string}>;
  if(matches.length!==1) throw new RecoveryRequired();
  const record=journal.get(matches[0]!.id);
  if(!record || !isEffect(record.operation.command) || record.operation.command.session_id!==mapping.sessionId
      || record.correlationConflict || record.sdkCalls!==1 || record.hostId!==mapping.hostId
      || !record.turnId || record.threadId!==context.threadId || !record.lastObservation) throw new RecoveryRequired();
  if(record.operation.command.kind==='create' && (JSON.stringify(record.operation.command.harness)!==JSON.stringify(mapping.harness)
      || JSON.stringify(record.operation.command.selection)!==JSON.stringify(mapping.selection))) throw new RecoveryRequired();
  const observation=record.lastObservation;
  if(observation.kind!=='agent' || observation.observation.evidence.kind!=='turn'
      || observation.observation.evidence.thread_id!==record.threadId || observation.observation.evidence.turn_id!==record.turnId
      || journal.acknowledged()<observationSequence(observation)) throw new RecoveryRequired();
  return {record,mapping};
}

export class McpIntentJournal {
  constructor(readonly journal:Journal,readonly limits:{records:number;bytes:number}=MCP_INTENT_LIMITS) {
    if(limits.records<1 || limits.records>MCP_INTENT_LIMITS.records || limits.bytes<1 || limits.bytes>MCP_INTENT_LIMITS.bytes) throw new JournalCapacity();
  }
  private read(intent:Pick<McpIntent,'installation'|'thread'|'request'|'modelCall'>):McpIntent|undefined {
    const row=this.journal.db.prepare('SELECT state,resolved,bytes,reserve,operation_id FROM mcp_intents WHERE key=?').get(key(intent)) as Row|undefined;
    if(!row)return undefined;
    const state=intentSchema.parse(JSON.parse(row.state));
    if(state.operation!==row.operation_id || Number(state.resolved)!==row.resolved || Buffer.byteLength(row.state)!==row.bytes
        || row.reserve!==((state.receipt || state.resolved)?0:256) || key(state)!==key(intent)
        || (state.rejected && (!state.attempted || !state.resolved || state.receipt!==null))
        || (state.resolved && state.receipt===null && !state.rejected)) throw new RecoveryRequired();
    return state;
  }
  intent(source:McpSource,context:PluginAgentToolContext,alias:string,argumentDigest:string):McpIntent {
    const {record,mapping}=source;const invocation=context.nativeInvocation;
    if(!invocation || mapping.harness.kind!=='agent' || !record.turnId) throw new RecoveryRequired();
    return this.journal.db.transaction(()=>{
      const candidate=intentSchema.parse({installation:this.journal.binding().installation_id,thread:context.threadId,
        request:invocation.clientRequestId,modelCall:invocation.callId,operation:record.operation.id,session:mapping.sessionId,
        harness:mapping.harness.kind==='agent'?mapping.harness.digest:'',turn:record.turnId,alias,argumentDigest,
        deadline:Math.min(Date.parse(record.operation.deadline),Date.now()+15*60*1000),receipt:null,resolved:false,attempted:false,rejected:false});
      const saved=this.read(candidate);
      if(saved) {
        for(const field of ['installation','thread','request','modelCall','operation','session','harness','turn','alias','argumentDigest'] as const)
          if(saved[field]!==candidate[field]) throw new RecoveryRequired();
        return saved;
      }
      if(record.stage!=='observing' || !['running','waiting_for_input'].includes(record.nativeState??'') || candidate.deadline<=Date.now()) throw new RecoveryRequired();
      this.write(candidate,true);return candidate;
    })();
  }
  attempt(intent:McpIntent):boolean {
    return this.journal.db.transaction(()=>{
      const current=this.read(intent);if(!current || current.rejected || current.receipt)throw new RecoveryRequired();
      this.write({...current,attempted:true},false);
      return !current.attempted;
    })();
  }
  reject(intent:McpIntent):void {
    this.journal.db.transaction(()=>{
      const current=this.read(intent);if(!current || !current.attempted || current.receipt)throw new RecoveryRequired();
      this.write({...current,rejected:true,resolved:true},false);
    })();
  }
  receipt(intent:McpIntent,call:string,run:string,deadline:string,resolved:boolean):McpIntent {
    return this.journal.db.transaction(()=>{
      const current=this.read(intent);if(!current || current.rejected)throw new RecoveryRequired();
      const receipt=intentSchema.shape.receipt.unwrap().parse({call,run,deadline});
      if(!Number.isFinite(Date.parse(deadline)) || (current.receipt && JSON.stringify(current.receipt)!==JSON.stringify(receipt))
          || (current.resolved && !resolved)) throw new RecoveryRequired();
      const next=intentSchema.parse({...current,receipt,deadline:Math.min(current.deadline,Date.parse(deadline)),resolved});
      this.write(next,false);return next;
    })();
  }
  observe(response:McpResponse,current:McpSource):McpIntent|undefined {
    const {invocation}=response.origin;
    const intent=this.read({installation:this.journal.binding().installation_id,thread:invocation.thread_id,
      request:invocation.client_request_id,modelCall:invocation.model_call_id});
    if(!intent)return undefined; // A status reference never creates an effect intent.
    if(intent.operation!==response.origin.operation_id || intent.turn!==invocation.turn_id
        || intent.session!==current.mapping.sessionId || current.mapping.harness.kind!=='agent'
        || intent.harness!==current.mapping.harness.digest)throw new RecoveryRequired();
    return this.receipt(intent,response.call_id,response.run_id,response.deadline,!['pending','reconciliation_required'].includes(response.kind));
  }
  private write(intent:McpIntent,fresh:boolean):void {
    const state=JSON.stringify(intent);const bytes=Buffer.byteLength(state);const reserve=(intent.receipt || intent.resolved)?0:256;
    const budget=this.journal.db.prepare(`SELECT count(*) AS records,COALESCE(sum(bytes+reserve),0) AS used,
      COALESCE((SELECT sum(bytes) FROM outbox),0)+COALESCE((SELECT sum(reserve) FROM commands),0) AS other FROM mcp_intents`).get() as {records:number;used:number;other:number};
    const previous=this.journal.db.prepare('SELECT bytes+reserve AS cost FROM mcp_intents WHERE key=?').get(key(intent)) as {cost:number}|undefined;
    const cost=budget.used-(previous?.cost??0)+bytes+reserve;
    if(bytes>8192 || budget.records+Number(fresh)>this.limits.records || cost>this.limits.bytes
        || budget.other+cost>LIMITS.outbox) throw new JournalCapacity();
    if(fresh)this.journal.db.prepare('INSERT INTO mcp_intents(key,operation_id,state,bytes,resolved,reserve) VALUES(?,?,?,?,?,?)')
      .run(key(intent),intent.operation,state,bytes,Number(intent.resolved),reserve);
    else this.journal.db.prepare('UPDATE mcp_intents SET state=?,bytes=?,resolved=?,reserve=? WHERE key=?').run(state,bytes,Number(intent.resolved),reserve,key(intent));
  }
}
