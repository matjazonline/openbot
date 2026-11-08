#!/usr/bin/env python3
import json
from pathlib import Path
import re
import subprocess
import sys
logs=Path(__file__).resolve().parent
checks=[('sqlx-check-02',['cargo','sqlx','prepare','--check','--','--all-targets'])]
for name,scope in [
 ('feature-final-tests-02','channel_reply_'),
 ('messaging-final-tests-02','workflow_message_'),
 ('delivery-final-tests-02','adapters::persistence::delivery::'),
 ('message-retention-final-tests-02','run_retention_pending_message_consumption_keeps_terminal_tree'),
]:
 checks.append((name,['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib',scope,'--','--nocapture']))
checks += [
 ('clippy-final-02',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('fmt-final-02',['cargo','fmt','--all','--','--check']),
 ('diff-final-02',['git','diff','--check']),
 ('graft-final-02',['graft','build'])]
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
 (logs/'focused-batch-02-results.json').write_text(json.dumps(results,indent=2)+'\n')
 print('RESULT',json.dumps(result),flush=True)
sys.exit(1 if any(r['exit_code'] for r in results) else 0)
