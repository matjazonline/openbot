#!/usr/bin/env python3
import json
from pathlib import Path
import subprocess
import sys
logs=Path(__file__).resolve().parent
checks=[
 ('final-sqlx-check-01',['cargo','sqlx','prepare','--check','--','--locked','--offline','--all-targets']),
 ('final-clippy-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('final-feature-tests-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','channel_reply_','--','--nocapture']),
 ('final-fmt-01',['cargo','fmt','--all','--','--check']),
 ('final-diff-01',['git','diff','--check']),
 ('final-graft-01',['graft','build']),
]
receipts=[]
for identity,command in checks:
 result=subprocess.run([sys.executable,str(logs/'run-check.py'),identity,*command])
 receipts.append(identity+'.json')
 (logs/'author-final-checks.json').write_text(json.dumps({'checks':receipts,'full_suite':False},indent=2)+'\n')
 if result.returncode:raise SystemExit(result.returncode)
