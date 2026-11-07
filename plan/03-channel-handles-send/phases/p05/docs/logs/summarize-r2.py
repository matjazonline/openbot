import json,re
from pathlib import Path
logs=Path(__file__).resolve().parent
prior=json.loads((logs/'current-affected-summary-01.json').read_text())
names=set(prior['tests']);new=[]
for check in ['hitl-tests-10','release-tests-04','completion-probe-tests-01','upstream-action-tests-02','release-knowledge-tests-02','upstream-message-agent-tests-01','upstream-knowledge-tests-01','release-final-entry-tests-01','release-native-delivery-tests-01']:
 receipt=json.loads((logs/(check+'.json')).read_text());assert receipt['exit_code']==0,check
 output=(logs/(check+'.log')).read_text();results=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',output)
 passed=re.findall(r'^test (\S+) \.\.\. ok$',output,re.M)
 assert len(results)==1 and int(results[0][0])>0 and results[0][1:] == ('0','0'),(check,results)
 assert len(passed)==int(results[0][0]),(check,len(passed),results)
 names.update(passed);new.append({'check':check,'selected':len(passed),'passed':len(passed),'failed':0,'ignored':0,'tests':passed})
static=json.loads((logs/'correction-checks-03.json').read_text());assert len(static)==12 and all(c['exit']==0 for c in static)
current={'distinct_passed':len(names),'failed':0,'ignored':0,'full_suite':False,'tests':sorted(names),'prior_summary':'current-affected-summary-01.json','current_cohorts':new,'evidence_reuse':'Unchanged p05 core/registry/decision/wait/child/sample behavior retains R1 evidence. Current35 HITL,current channel-reply family, native delivery, knowledge release and upstream-changed caller fixtures rerun after F1. Two inherited completion probe prefixes corrected to the exact current query; both actual competing-owner cases rerun. Partial broad action output is deliberately not counted. Source semantics outside this correction and upstream bridge are unchanged.','static_checks':static,'fresh_migrations':['migrations-main-05.json','migrations-test-05.json'],'actual_sqlx_prepare':'sqlx-prepare-04.json','sqlx_check':'sqlx-check-04.json','retained_failed_or_empty_checks':['release-before-tests-01.json','release-tests-02.json','release-tests-03.json','hitl-tests-09.json','release-knowledge-tests-01.json','upstream-action-tests-01.json'],'empty_selection_replaced':{'release-knowledge-tests-01':'release-knowledge-tests-02'},'partial_cohort_disposition':'upstream-action-stop.json and completion-probe-correction.json; zero full-cohort acceptance claim','current_head':'f6e133058c69e28c173850657b134b83ac7c6224'}
(logs/'current-affected-summary-02.json').write_text(json.dumps(current,indent=2)+'\n');print(json.dumps({'distinct_passed':len(names),'cohorts':[(c['check'],c['passed']) for c in new]}))
