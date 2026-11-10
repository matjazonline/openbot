import { spawn, execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { once } from 'node:events';
const directory = '/var/folders/n2/5y7x75r92zl0ll90154hxdzr0000gn/T/openbot-packaged-smoke-WAYvD9';
const source = '/tmp/openbot-cloudroom-d8b3cc3'; const ports = [51771,51772,51773];
const child = spawn(process.execPath, [directory+'/plugin/services/run.mjs', '--source', source, '--profile', directory+'/profile',
  '--server-port',String(ports[0]),'--daemon-port',String(ports[1]),'--ui-port',String(ports[2])], { stdio:['ignore','pipe','pipe'] });
let output=''; child.stdout.on('data',part=>{ output+=part; });
const timer=setTimeout(()=>child.kill('SIGTERM'),45_000);
const cli=args=>JSON.parse(execFileSync(process.execPath,[source+'/gui/apps/cli/dist/index.js',...args],{cwd:source+'/gui',
  env:{...process.env,NODE_ENV:'production',BB_DATA_DIR:directory+'/profile',ROOM_SERVER_URL:`http://127.0.0.1:${ports[0]}`,ROOM_HOST_DAEMON_PORT:String(ports[1])},encoding:'utf8',timeout:10_000,maxBuffer:512*1024}));
try{
  while(!output.includes('"state":"ready"')){if(child.exitCode!==null)throw new Error(output);await new Promise(done=>setTimeout(done,100));}
  const threads=cli(['thread','list','--json']); const results=[];
  for(const thread of threads){
    const list=cli(['thread','log',thread.id,'--json','--limit','100']);
    results.push({id:thread.id,status:thread.status,events:list.map(event=>({seq:event.seq,type:event.type,scope:event.scope,
      keys:Object.keys(event.data??{}),errorCode:event.data?.error?.code,itemType:event.data?.item?.type,
      terminalStatus:event.data?.status,markerPresent:JSON.stringify(event.data).includes('OPENBOT_PACKAGED_NATIVE_LIFECYCLE_20261108')}))});
  }
  writeFileSync(new URL('native-inspect-r1.json',import.meta.url),JSON.stringify(results,null,2));console.log(JSON.stringify(results));
}finally{clearTimeout(timer);const exited=once(child,'exit');child.kill('SIGTERM');await exited;}
