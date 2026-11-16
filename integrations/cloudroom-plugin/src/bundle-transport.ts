import { createHash } from 'node:crypto';
import type { Paired } from './enrollment.js';
import type { Identity } from './protocol.js';
import { PROTOCOL_VERSION } from './protocol.js';
import { MAX_NATIVE_BUNDLE_BYTES } from './native-bundle.js';
import { RecoveryRequired } from './journal.js';

export async function fetchNativeBundle(paired:Paired,identity:Identity,operation:string,signal:AbortSignal):Promise<Buffer> {
  const url=new URL(`/v1/local-machines/operations/${encodeURIComponent(operation)}/bundle`,paired.binding.origin);
  if(url.protocol!=='https:' || url.origin!==paired.binding.origin) throw new RecoveryRequired();
  const response=await fetch(url,{method:'GET',redirect:'error',signal:AbortSignal.any([signal,AbortSignal.timeout(5000)]),headers:{
    authorization:`Bearer ${paired.credential}`,'x-openbot-protocol-version':String(PROTOCOL_VERSION),
    'x-openbot-installation-id':identity.installation_id,'x-openbot-credential-generation':String(identity.credential_generation),
    'x-openbot-connection-generation':String(identity.connection_generation),
  }});
  if(response.status!==200 || !response.body) {await response.body?.cancel();throw new RecoveryRequired();}
  const length=response.headers.get('content-length');
  if(length!==null && (!/^\d+$/.test(length) || Number(length)>MAX_NATIVE_BUNDLE_BYTES)) {await response.body.cancel();throw new RecoveryRequired();}
  const reader=response.body.getReader();const chunks:Uint8Array[]=[];let size=0;
  try {
    for(;;){signal.throwIfAborted();const {done,value}=await reader.read();if(done)break;size+=value.byteLength;if(size>MAX_NATIVE_BUNDLE_BYTES)throw new RecoveryRequired();chunks.push(value);}
    const bytes=Buffer.concat(chunks);const digest=createHash('sha256').update(bytes).digest('hex');
    if(response.headers.get('etag')!==`"${digest}"`)throw new RecoveryRequired();return bytes;
  }finally{await reader.cancel().catch(()=>{});reader.releaseLock();}
}
