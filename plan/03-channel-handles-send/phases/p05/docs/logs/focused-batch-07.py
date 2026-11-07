#!/usr/bin/env python3
import json,re,subprocess,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
start_from=sys.argv[1] if len(sys.argv)>1 else None
started=start_from is None
cohorts=[
 ('hitl-prompt','channel_reply_hitl_prompt_'),
 ('hitl-resume','channel_reply_hitl_resume_'),
 ('final-approval','channel_reply_final_approval_'),
 ('channel-reply','channel_reply_'),
 ('messaging','workflow_message_'),
 ('reply-ingress','message_reply_'),
 ('action-approval','workflow_action_approval_'),
 ('decisions','workflow_decision_'),
 ('waits','workflow_wait_'),
 ('children','workflow_child_'),
 ('delivery','adapters::persistence::delivery::tests::'),
 ('cancellation','adapters::persistence::delivery::cancellation::tests::'),
 ('samples','application::workflow::sample::'),
 ('registry','workflow_registry_'),
 ('storage-semantics','workflow_storage_'),
 ('attempt-schema','schema_constraint_attempt_stop_reason_matrix'),
]
summary=json.loads((logs/'focused-batch-06.json').read_text()) if start_from else []
for name,prefix in cohorts:
    if not started and name!=start_from: continue
    started=True
    base=['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib',prefix,'--']
    for mode,args in [('list',['--list']),('tests',['--nocapture'])]:
        label=f'{name}-{mode}-07'
        result=subprocess.run([sys.executable,str(logs/'run-check.py'),label,*base,*args],capture_output=True,text=True)
        print(json.dumps({'check':label,'exit':result.returncode}),flush=True)
        if result.returncode:
            print(result.stdout[-7000:],flush=True)
            (logs/'focused-batch-07.json').write_text(json.dumps(summary,indent=2)+'\n')
            sys.exit(result.returncode)
        text=(logs/f'{label}.log').read_text()
        if mode=='list':
            inventory=re.findall(r'^(.+): test$',text,re.M)
            if not inventory: raise RuntimeError(f'empty inventory {label}')
        else:
            results=re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored)',text,re.M)
            passed=[n for n,s in results if s=='ok']; ignored=[n for n,s in results if s=='ignored']
            if set(passed+ignored)!=set(inventory): raise RuntimeError(f'inventory mismatch {label}')
            summary.append({'cohort':name,'prefix':prefix,'selected':len(inventory),'passed':len(passed),'ignored':len(ignored),'tests':passed,'ignored_tests':ignored,'list':f'{name}-list-07.log','run':f'{name}-tests-07.log'})
            (logs/'focused-batch-07.json').write_text(json.dumps(summary,indent=2)+'\n')
            print(json.dumps({'cohort':name,'selected':len(inventory),'passed':len(passed),'ignored':len(ignored)}),flush=True)
