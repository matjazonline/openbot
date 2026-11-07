#!/usr/bin/env python3
import hashlib,json,re,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
identity=sys.argv[1] if len(sys.argv)>1 else '01'
output=logs/('current-affected-summary-'+identity+'.json')
assert not output.exists(),'Preserve immutable review evidence'
cohorts=[]; cases={}
for number,total in [('01',16),('02',9),('03',6)]:
 rows=json.loads((logs/('focused-batch-'+number+'-results.json')).read_text())
 assert len(rows)==total,('incomplete verification',number,len(rows))
 if number!='01': assert all(row['exit_code']==0 for row in rows),('failed current gate',number)
 for row in rows:
  if 'selected' not in row: continue
  assert row['exit_code']==0,row
  log=logs/(row['name']+'.log')
  matches=re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored)(?:[^\n]*)$',log.read_text(errors='replace').replace('\r','\n'),re.M)
  assert len(matches)==row['selected'],('inventory mismatch',row['name'],len(matches),row['selected'])
  assert all(status!='FAILED' for _,status in matches),row['name']
  cohorts.append({'name':row['name'],'selected':row['selected'],'receipt':row['receipt'],'log_sha256':hashlib.sha256(log.read_bytes()).hexdigest()})
  for name,status in matches: cases[name]={'status':status,'latest_passing_cohort':row['name']}
counts={status:sum(case['status']==status for case in cases.values()) for status in ['ok','ignored','FAILED']}
summary={
 'phase':'p04','full_suite':False,'scope':'Only changed reply publication/delivery and affected shared interfaces',
 'counts':{'distinct_selected':len(cases),'passed':counts['ok'],'ignored':counts['ignored'],'failed':counts['FAILED']},
 'cohorts':cohorts,'cases':cases,
 'current_gates':'focused-batch-03-results.json; fresh main/test migrations04; actual sqlx-prepare03 and check03',
 'reuse_disposition':[
  'Batch02 reruns the final-link retention correction against125reply,64messaging,39delivery and1message-retention tests; prior failed diagnostic is replaced by its passing permanent regression.',
  'Batch03 reruns all28publication/delivery/recovery tests after final-only server lock bounds, including real blocked binding-lock/no-entry/release proof. Explicit/nonworkflow branches return before the setter.',
  'Earlier worker shutdown, sample, knowledge, engine/pool, ingress, transport and protected-approval scopes are unaffected by these later parent-delete/NULL-retention and final-only server-bound corrections.',
  'Existing native retention21 cases (including100001member boundary) have no final-link deletion path; previous PASS retained. New actual final pending/unknown/resolved retention and whole-company teardown controls rerun in the publication cohort.',
 ],
 'retained_failures':'All receipts kept: author test fixture/type mistakes, corrected exact retry classification,3Clippy auto-deref issues fixed, and real author-detected unknown-delivery/owner deletion bypass demonstrated then fixed.',
 'unavailable_checks':[],
 'environment':'Owned PostgreSQL18.6 loopback65037; mandatory runtime networkguard; locked offline Cargo runtime; stock2MiB; serialized jobs4 incremental target',
}
output.write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary['counts']))
