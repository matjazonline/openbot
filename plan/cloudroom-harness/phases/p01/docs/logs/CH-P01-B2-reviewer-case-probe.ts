import { createHash, randomUUID } from 'node:crypto';
import { NativeBundleCache } from '/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree/integrations/cloudroom-plugin/src/native-bundle.ts';
import { fixture, operation } from '/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree/integrations/cloudroom-plugin/test/support.ts';
async function main() {
 const f=fixture();
 const value={format:'openbot_native_v1',instructions:'Use both assets.',permissions:'full',skills:[{name:'case-proof',entry:'---\nname: case-proof\ndescription: Case directory proof\n---\nRead both files.',files:[{path:'Scripts/one.txt',content_base64:'YQ==',executable:false},{path:'scripts/two.txt',content_base64:'Yg==',executable:false}]}]};
 const raw=Buffer.from(JSON.stringify(value)); const reference={kind:'agent' as const,agent_id:randomUUID(),revision_id:randomUUID(),digest:createHash('sha256').update(raw).digest('hex')};
 const op=operation({kind:'create',session_id:randomUUID(),selection:f.selection,harness:reference,prompt:'case proof'});f.journal.admit({operation:op,mode:'dispatch'});
 try {const result=await new NativeBundleCache(f.journal).load(async()=>raw,op.id,reference,'admission',new AbortController().signal);console.log('CACHE ACCEPTED',result.configuration.skillDirectories);}
 catch(error) {console.log('CACHE REFUSED',error.constructor.name,error.message);}
 finally {f.close();}
}
main().catch(error=>{console.error(error);process.exitCode=1;});
