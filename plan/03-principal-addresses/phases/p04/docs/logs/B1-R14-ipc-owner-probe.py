import json, os, subprocess, tempfile
from pathlib import Path
root=Path.cwd()
script=root/'scripts/doop-test/browser-ipc-check.mjs'
results=[]
for kind in ('receipt-failure','stalled-close'):
 with tempfile.TemporaryDirectory(prefix='pa03-b14-ipc-') as temporary:
  fixture=Path(temporary);source=fixture/'source';module=source/'node_modules/puppeteer-core';module.mkdir(parents=True)
  (source/'package.json').write_text('{}')
  (module/'package.json').write_text('{"main":"index.cjs"}')
  (module/'index.cjs').write_text("""const fs=require('node:fs');const path=require('node:path');
exports.launch=async()=>{
 const runtime=process.env.DOOP_TEST_PRIVATE_RUNTIME;
 fs.writeFileSync(path.join(runtime,'launch-returned'),'yes');
 if(process.env.REVIEW_KIND==='receipt-failure'){
  fs.unlinkSync(path.join(runtime,'browser-process.json'));
  fs.mkdirSync(path.join(runtime,'browser-process.json'));
 }
 return {process:()=>({pid:process.pid}),newPage:async()=>({goto:async()=>{}}),close:async()=>{
  fs.writeFileSync(path.join(runtime,'close-entered'),'yes');
  if(process.env.REVIEW_KIND==='stalled-close')await new Promise(()=>setInterval(()=>{},1000));
 }};
};
""")
  output=fixture/'output';output.mkdir()
  env=dict(os.environ,REVIEW_KIND=kind,CHROME_PATH='/mock/only')
  child=subprocess.Popen(['node',str(script),str(source),str(output)],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  stopped=False
  try: stdout,stderr=child.communicate(timeout=2)
  except subprocess.TimeoutExpired:
   stopped=True;child.terminate();stdout,stderr=child.communicate(timeout=5)
  runtime=next(output.glob('browser-ipc-*'))
  item={'case':kind,'launch_returned':(runtime/'launch-returned').exists(),'close_entered':(runtime/'close-entered').exists(),'harness_had_to_terminate':stopped,'exit_status':child.returncode,'PASS_report_written':(output/'browser-ipc-check.json').exists(),'runtime_retained':runtime.exists(),'native_browser_used':False}
  if kind=='receipt-failure':
   assert child.returncode!=0 and item['launch_returned'] and not item['close_entered'];assert b'EISDIR' in stderr or b'ENOTDIR' in stderr
  else:assert stopped and item['close_entered']
  results.append(item)
print(json.dumps(results,indent=2))
