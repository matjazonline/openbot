#!/usr/bin/env python3
import hashlib,json,re
from pathlib import Path
logs=Path(__file__).resolve().parent
output=logs/'current-affected-summary-02.json';assert not output.exists()
summary=json.loads((logs/'current-affected-summary-01.json').read_text());cases=summary['cases'];cohorts=summary['cohorts']
rows4=json.loads((logs/'focused-batch-04-results.json').read_text());rows5=json.loads((logs/'focused-batch-05-results.json').read_text())
assert len(rows4)==12 and len(rows5)==6
assert all(row['exit_code']==0 for row in rows5)
assert all(row['exit_code']==0 for row in rows4 if row['name']!='delivery-final-tests-04')
extra=json.loads((logs/'delivery-final-tests-06.json').read_text());assert extra['exit_code']==0
rows=[row for row in rows4+rows5 if 'selected' in row and row['exit_code']==0]
rows.append({'name':'delivery-final-tests-06','selected':39,'receipt':str(logs/'delivery-final-tests-06.json')})
for row in rows:
 log=logs/(row['name']+'.log')
 matches=re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored)(?:[^\n]*)$',log.read_text(errors='replace').replace('\r','\n'),re.M)
 assert len(matches)==row['selected'],(row['name'],len(matches),row['selected'])
 assert all(status=='ok' for _,status in matches)
 cohorts.append({'name':row['name'],'selected':row['selected'],'receipt':row['receipt'],'log_sha256':hashlib.sha256(log.read_bytes()).hexdigest()})
 for name,status in matches: cases[name]={'status':status,'latest_passing_cohort':row['name']}
summary['counts']={'distinct_selected':len(cases),'passed':sum(c['status']=='ok' for c in cases.values()),'ignored':0,'failed':0}
summary['current_gates']='fresh main/test migrations07; actual SQLxprepare06/check05; offlinealltargetClippy05/fmt05/diff05/graft05; focused-batch04/05 plus delivery-final-tests06'
summary['reuse_disposition'] += [
 'After saved B1F1/F2 corrections, batch04 reran130reply,64messaging,39delivery,1message-retention,49knowledge,32approval and14ingress cases. Only one delivery assertion failed because early FK refusal changed native error metadata.',
 'Preserved the original named composite FK and native NOTNULL result without changing any existing assertions. Fresh migrations07 and actualSQLxprepare06; delivery-final-tests06 reran all39PASS, including exact tenant/null failure contract and competing queue claims.',
 'Batch05 reran all4new membership/competing-publication cases after the metadata-only correction and all final static/current SQLx gates. Other batch04 production behavior is unchanged by that error-metadata/null-validation ordering fix.',
 'Unchanged worker lifecycle/sample/transport/engine and non-message native retention evidence is retained from prior cohorts; new actual final teardown/retention and shared message-retention cases reran after membership locks.',
]
summary['retained_failures'] += '; predecessor B1F1/F2 exact frozen-source diagnostics confirmed and retained; B review interrupted by automated content flag, no B1PASS; membership correction delivery04/05 error-metadata failures retained and superseded by39PASS delivery06.'
summary['review_status']='Corrected artifact not yet independently accepted; fresh whole-phase independent review required'
output.write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary['counts']))
p=logs/'freeze-review.py';s=p.read_text().replace('focused-batch-01-results.json and current-affected-summary-01.json','current-affected-summary-02.json and its immutable referenced check cohorts');p.write_text(s)
