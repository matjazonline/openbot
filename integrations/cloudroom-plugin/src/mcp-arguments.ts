import { createHash } from 'node:crypto';
import { RecoveryRequired } from './journal.js';

/** Bound before cloning, sorting or serializing data from the model. */
// Unicode mode treats a valid surrogate pair as one scalar; only lone halves match.
const invalidText=/[\u0000\uD800-\uDFFF]/u;
export function mcpArguments(input:unknown):{arguments:Record<string,unknown>;digest:string} {
  let nodes=4096,bytes=65536;
  function visit(value:unknown,depth:number):unknown {
    if(depth>32 || --nodes<0 || (bytes-=2)<0)throw new RecoveryRequired();
    if(value===null || typeof value==='boolean')return value;
    if(typeof value==='number'){if(!Number.isFinite(value))throw new RecoveryRequired();return value;}
    if(typeof value==='string'){if(invalidText.test(value) || (bytes-=Buffer.byteLength(value))<0)throw new RecoveryRequired();return value;}
    if(Array.isArray(value)) {
      if(value.length>nodes)throw new RecoveryRequired();
      return value.map(item=>visit(item,depth+1));
    }
    if(typeof value!=='object' || ![Object.prototype,null].includes(Object.getPrototypeOf(value)))throw new RecoveryRequired();
    const entries:Array<[string,unknown]>=[];
    for(const key in value) {
      if(!Object.hasOwn(value,key))continue;
      if(invalidText.test(key) || entries.length>=nodes || (bytes-=Buffer.byteLength(key)+3)<0)throw new RecoveryRequired();
      const property=Object.getOwnPropertyDescriptor(value,key);
      if(!property || !('value' in property))throw new RecoveryRequired();
      entries.push([key,property.value]);
    }
    entries.sort(([left],[right])=>left<right?-1:left>right?1:0);
    return Object.fromEntries(entries.map(([key,item])=>[key,visit(item,depth+1)]));
  }
  if(input===null || typeof input!=='object' || Array.isArray(input))throw new RecoveryRequired();
  const args=visit(input,0) as Record<string,unknown>;
  const raw=JSON.stringify(args);if(Buffer.byteLength(raw)>65536)throw new RecoveryRequired();
  return {arguments:args,digest:createHash('sha256').update(raw).digest('hex')};
}
