import json,subprocess,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
checks=[
 ('hitl-list-08',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','channel_reply_hitl_','--','--list']),
 ('hitl-tests-08',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','channel_reply_hitl_','--','--nocapture']),
 ('message-entry-tests-08',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','workflow_message_delivery_duplicate_callback_adds_no_event','--','--nocapture']),
 ('clippy-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('sqlx-prepare-03',['cargo','sqlx','prepare','--','--all-targets']),
 ('sqlx-check-03',['cargo','sqlx','prepare','--check','--','--all-targets']),
 ('fmt-check-01',['cargo','fmt','--all','--','--check']),
 ('diff-check-01',['git','diff','--check']),
 ('graft-build-01',['graft','build']),
]
summary=[]
for name,command in checks:
 result=subprocess.run([sys.executable,str(logs/'run-check.py'),name,*command],capture_output=True,text=True)
 summary.append({'check':name,'exit':result.returncode})
 (logs/'final-checks-01.json').write_text(json.dumps(summary,indent=2)+'\n')
 print(json.dumps(summary[-1]),flush=True)
 if result.returncode:
  print(result.stdout[-9000:],flush=True);sys.exit(result.returncode)
