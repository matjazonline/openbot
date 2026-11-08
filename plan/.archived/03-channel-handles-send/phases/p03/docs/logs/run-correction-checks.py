#!/usr/bin/env python3
import json,subprocess,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
runner=logs/'run-check.py'
checks=[
 ('correction-retention-list-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','run_retention_','--','--list']),
 ('correction-retention-tests-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','run_retention_','--','--nocapture']),
 ('correction-sqlx-prepare-01',['cargo','sqlx','prepare','--','--locked','--offline','--all-targets']),
 ('correction-sqlx-check-01',['cargo','sqlx','prepare','--check','--','--locked','--offline','--all-targets']),
 ('correction-clippy-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('correction-fmt-01',['cargo','fmt','--all','--','--check']),
 ('correction-diff-01',['git','diff','--check']),
 ('correction-graft-01',['graft','build']),
]
results=[]
for name,command in checks:
 result=subprocess.run([sys.executable,str(runner),name,*command],cwd=logs.parents[5])
 receipt=json.loads((logs/(name+'.json')).read_text());results.append(receipt)
 (logs/'author-correction-checks.json').write_text(json.dumps(results,indent=2)+'\n')
 if result.returncode:sys.exit(result.returncode)
 if name=='correction-retention-list-01':
  count=sum(line.endswith(': test') for line in (logs/(name+'.log')).read_text().splitlines())
  assert count>0,'empty retention selection'
  (logs/'correction-retention-inventory.json').write_text(json.dumps({'selected':count,'list_log':name+'.log'},indent=2)+'\n')
