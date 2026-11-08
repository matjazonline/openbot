#!/usr/bin/env python3
"""Serial focused gates; intentionally no full runtime, lease or stack suite."""
from pathlib import Path
import json
import subprocess
import sys

logs=Path(__file__).resolve().parent
runner=logs/'run-check.py'
filters=['agent_history_','workflow_admit_','workflow_agent_run_','autonomous_agent_admission_',
         'activation_tests::','rerun_tests::','manual::tests','channel_reply_e2e_',
         'autonomous_agent_samples_','ui::runs::']
checks=[
 ('sqlx-check-lifecycle-01',['cargo','sqlx','prepare','--check','--','--all-targets']),
 ('history-and-affected-list-04',['cargo','test','--locked','--offline','--lib','--','--list',*filters]),
 ('history-and-affected-runtime-06',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--',*filters]),
 ('offline-all-targets-lifecycle-01',['cargo','check','--locked','--offline','--all-targets']),
 ('identity-clippy-02',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('identity-fmt-check-01',['cargo','fmt','--all','--','--check']),
 ('graph-implementation-03',['graft','build']),
 ('identity-diff-check-01',['git','diff','--check']),
]
completed=[]
for name,command in checks:
    result=subprocess.run([sys.executable,str(runner),name,*command])
    completed.append({'check':name,'exit_code':result.returncode})
    (logs/'focused-final-gate-progress.json').write_text(json.dumps(completed,indent=2)+'\n')
    if result.returncode:sys.exit(result.returncode)
    if name=='history-and-affected-list-04':
        names=[line[:-6] for line in (logs/(name+'.log')).read_text().splitlines() if line.endswith(': test')]
        counts={term:sum(term in item for item in names) for term in filters}
        assert len(names)==165 and all(counts.values()),(len(names),counts)
        prefixes=['agent_history_identity_','agent_history_level_','agent_history_contract_']
        receipt={'unique_selected':len(names),'filters':counts,'history_prefixes':{term:sum(term in item for item in names) for term in prefixes},'names':names,'full_suite':False}
        (logs/'focused-selection-06.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('All serial focused gates passed',flush=True)
