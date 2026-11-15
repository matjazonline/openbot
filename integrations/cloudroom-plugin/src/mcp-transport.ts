import { z } from 'zod';
import type { Paired } from './enrollment.js';
import { RecoveryRequired } from './journal.js';
import { PROTOCOL_VERSION, uuid, external, type Identity } from './protocol.js';
import type { McpSource } from './mcp-journal.js';

export type NativeMcpConnection={paired:Paired;identity:Identity;signal:AbortSignal};
const MAX_RESPONSE=2*1024*1024;
const base=z.object({call_id:uuid,run_id:uuid,deadline:z.string().max(64).refine(value=>Number.isFinite(Date.parse(value))),
  origin:z.object({operation_id:uuid,invocation:z.object({thread_id:external,turn_id:external,client_request_id:external,model_call_id:external}).strict()}).strict()});
export const mcpResponseSchema=z.discriminatedUnion('kind',[
  base.extend({kind:z.literal('pending')}).strict(),
  base.extend({kind:z.literal('completed'),result:z.unknown()}).strict(),
  base.extend({kind:z.literal('tool_error'),result:z.unknown()}).strict(),
  base.extend({kind:z.literal('failed'),code:z.string().regex(/^[a-z_]{1,64}$/)}).strict(),
  base.extend({kind:z.enum(['cancelled','reconciliation_required','collected'])}).strict(),
]);
export type McpResponse=z.infer<typeof mcpResponseSchema>;
export class McpHttpRefusal extends Error {
  constructor(readonly status:number,readonly notAdmitted=false){super(`Openbot refused this MCP request (${status})`);}
}
export type McpAction={kind:'call';alias:string;arguments:Record<string,unknown>}|{kind:'status';call_id:string}|{kind:'cancel_invocation'};
export function mcpInvocation(source:McpSource) {
  const {record}=source;
  if(!record.threadId || !record.turnId || !record.requestId)throw new RecoveryRequired();
  return {thread_id:record.threadId,turn_id:record.turnId,client_request_id:record.requestId};
}

export async function nativeMcpRequest(connection:NativeMcpConnection,source:McpSource,modelCall:string,
  action:McpAction,signal:AbortSignal,remaining=15_000):Promise<McpResponse> {
  const {paired,identity}=connection;
  const url=new URL(`/v1/local-machines/operations/${encodeURIComponent(source.record.operation.id)}/mcp`,paired.binding.origin);
  if(url.protocol!=='https:' || url.origin!==paired.binding.origin)throw new RecoveryRequired();
  const body=JSON.stringify({invocation:{...mcpInvocation(source),model_call_id:modelCall},action});
  if(Buffer.byteLength(body)>131072)throw new RecoveryRequired();
  const response=await fetch(url,{method:'POST',redirect:'error',body,
    signal:AbortSignal.any([signal,connection.signal,AbortSignal.timeout(Math.max(1,Math.min(15_000,remaining)))]),headers:{
      authorization:`Bearer ${paired.credential}`,'content-type':'application/json',
      'x-openbot-protocol-version':String(PROTOCOL_VERSION),'x-openbot-installation-id':identity.installation_id,
      'x-openbot-credential-generation':String(identity.credential_generation),'x-openbot-connection-generation':String(identity.connection_generation),
    }});
  if(response.status!==200 || !response.body){await response.body?.cancel();throw new McpHttpRefusal(response.status,
    response.status===400 && response.headers.get('x-openbot-mcp-not-admitted')==='invalid_arguments');}
  const length=response.headers.get('content-length');
  if(length!==null && (!/^\d+$/.test(length) || Number(length)>MAX_RESPONSE)){await response.body.cancel();throw new RecoveryRequired();}
  const reader=response.body.getReader();const chunks:Uint8Array[]=[];let bytes=0;
  try {
    for(;;){signal.throwIfAborted();const {done,value}=await reader.read();if(done)break;bytes+=value.byteLength;if(bytes>MAX_RESPONSE)throw new RecoveryRequired();chunks.push(value);}
    return mcpResponseSchema.parse(JSON.parse(new TextDecoder('utf8',{fatal:true}).decode(Buffer.concat(chunks))));
  }finally{await reader.cancel().catch(()=>{});reader.releaseLock();}
}
