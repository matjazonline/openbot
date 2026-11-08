#!/usr/bin/env python3
import json
from pathlib import Path
import re
import subprocess
import sys
logs=Path(__file__).resolve().parent
checks=[('sqlx-check-03',['cargo','sqlx','prepare','--check','--','--all-targets'])]
for name,scope in [
 ('publication-final-tests-03','completion_tests::publication_tests'),
]:
 checks.append((name,['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib',scope,'--','--nocapture']))
checks += [
 ('clippy-final-03',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('fmt-final-03',['cargo','fmt','--all','--','--check']),
 ('diff-final-03',['git','diff','--check']),
 ('graft-final-03',['graft','build'])]
results=[]
for name,command in checks:
 print('CHECK',name,flush=True)
 exit_code=subprocess.call([sys.executable,str(logs/'run-check.py'),name,*command])
 receipt=json.loads((logs/(name+'.json')).read_text())
 result={'name':name,'exit_code':exit_code,'seconds':receipt['seconds'],'receipt':str(logs/(name+'.json'))}
 if 'test' in command:
  output=(logs/(name+'.log')).read_text(errors='replace')
  selected=re.search(r'running (\d+) tests?',output)
  result['selected']=int(selected.group(1)) if selected else 0
  if not result['selected']:
   result['inventory_error']='No nonzero selected inventory'
   if not exit_code: result['exit_code']=1
 results.append(result)
 (logs/'focused-batch-03-results.json').write_text(json.dumps(results,indent=2)+'\n')
 print('RESULT',json.dumps(result),flush=True)
sys.exit(1 if any(r['exit_code'] for r in results) else 0)
