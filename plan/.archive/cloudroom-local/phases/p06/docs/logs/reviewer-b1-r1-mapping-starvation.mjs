import { createRequire } from 'node:module';
import { resolve } from 'node:path';
const plugin = resolve('integrations/cloudroom-plugin');
const require = createRequire(resolve(plugin, 'package.json'));
const { build } = require('esbuild');
const code = `
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {fixture,operation} from './test/support.ts';
const f=fixture();
try {
 const old=Date.now()-100*86400000;
 const retained=new Set();
 f.journal.db.transaction(()=>{
   for(let i=0;i<4096;i++) {
     const id=randomUUID();
     f.journal.bindSession(id,'thr_mapping_'+i,f.selection);
     f.journal.db.prepare('UPDATE sessions SET last_used_at=? WHERE id=?').run(old+i,id);
     if(i<100)retained.add(id);
   }
 })();
 const queries=[];
 // Model a current-fence server which still owns the first 100 sessions but has
 // permanently retired all other IDs. Each reply is complete for its request.
 for(let round=0;round<10;round++) {
   const page=f.journal.mappingCandidates();queries.push(page.map(x=>x.id));
   f.journal.retireMappings(page,page.filter(x=>!retained.has(x.id)).map(x=>x.id));
 }
 assert(queries.every(page=>page.length===100&&page.every(id=>retained.has(id))));
 const create=operation({kind:'create',session_id:randomUUID(),selection:f.selection,prompt:'new task'});
 assert.equal(f.journal.diagnostics().mappings,4096);
 assert.equal(f.journal.canAdmit(create),false);
 console.log(JSON.stringify({status:'REPRODUCED',rounds:queries.length,repeatedRetainedIds:100,
   absentMappingsNeverRequested:3996,mappings:f.journal.diagnostics().mappings,newCreateAdmitted:f.journal.canAdmit(create)}));
} finally {f.close();}
`;
const result = await build({stdin:{contents:code,resolveDir:plugin,sourcefile:'reviewer-starvation.ts',loader:'ts'},
  write:false,bundle:true,platform:'node',format:'cjs',packages:'external'});
const module={exports:{}};
new Function('require','module','exports',result.outputFiles[0].text)(require,module,module.exports);
