#!/usr/bin/env python3
"""Run the traced affected interfaces, with nonempty inventories and separate receipts."""
import json
from pathlib import Path
import subprocess
import sys

logs=Path(__file__).resolve().parent
selections=[
 ('feature','channel_reply_'),
 ('batch','workflow_batch_'),
 ('completion','workflow_completion_'),
 ('wait','workflow_wait_'),
 ('child-call','workflow_call_'),
 ('repeat','workflow_repeat_'),
 ('decision','workflow_decision_'),
 ('pending-recovery','workflow_poison_'),
 ('protected-action','workflow_action_approval_'),
 ('messaging','workflow_message_'),
 ('message-admission','message_intent_'),
 ('release','knowledge_run_level_'),
 ('sample','application::workflow::sample::'),
 ('autonomous-agent','autonomous_agent_'),
]
summary=[]
for name,prefix in selections:
    row={'name':name,'prefix':prefix}
    for mode in ['list','tests']:
        identity=f'affected-{name}-{mode}-01'
        command=['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib',prefix,'--','--list' if mode=='list' else '--nocapture']
        result=subprocess.run([sys.executable,str(logs/'run-check.py'),identity,*command])
        row[f'{mode}_receipt']=identity+'.json'
        row[f'{mode}_exit']=result.returncode
        if mode=='list':
            row['selected']=[line.removesuffix(': test') for line in (logs/(identity+'.log')).read_text().splitlines() if line.endswith(': test')]
            if result.returncode or not row['selected']:
                summary.append(row)
                (logs/'affected-summary-01.json').write_text(json.dumps(summary,indent=2)+'\n')
                raise SystemExit('A focused selection did not compile or selected zero tests: '+prefix)
    summary.append(row)
    (logs/'affected-summary-01.json').write_text(json.dumps(summary,indent=2)+'\n')
    print('Focused scope finished:',name,'selected',len(row['selected']),'exit',row['tests_exit'],flush=True)
failed=[row['name'] for row in summary if row['tests_exit']]
print('Distinct selected tests:',len({test for row in summary for test in row['selected']}),flush=True)
print('Failed scopes:',failed,flush=True)
raise SystemExit(bool(failed))
