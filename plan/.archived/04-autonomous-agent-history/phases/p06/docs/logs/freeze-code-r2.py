from pathlib import Path
from datetime import datetime,timezone
import os,json,hashlib,subprocess,tarfile,re,difflib
r=Path.cwd();l=r/'plan/04-autonomous-agent-history/phases/p06/docs/logs';producer=json.loads((l/'producer-input.json').read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,d):p.write_text(json.dumps(d,indent=2)+'\n')
for n,h in producer['external_paths'].items():assert (sha(r/n) if (r/n).exists() else None)==h,n
required=['p06-corrections-02','p06-pressure-recheck-01','p06-source-modules-r2','p06-migrate-validate-r2','p06-sqlx-prepare-r2','p06-sqlx-check-r2','p06-offline-r2','p06-clippy-r2','p06-discovery-final-02','p06-fmt-r2','p06-diff-r2','p06-graft-r2','p06-case-evidence-r2-02','p06-browser-03','p06-protocols-02','p06-decision-browser-01']
receipts={n:json.loads((l/(n+'.json')).read_text()) for n in required};assert all(d['exit_code']==0 for d in receipts.values())
coverage=json.loads((l/'verification-case-inventory-r2.json').read_text());assert coverage['status']=='PASS'
excludes=producer['exclusions'];spec=['.',*[':(top,exclude,literal)'+p for p in excludes]];paths=set()
for c in [['git','diff','--name-only','HEAD','--',*spec],['git','ls-files','--others','--exclude-standard','--',*spec]]:paths.update(subprocess.check_output(c,text=True).splitlines())
owned={n:sha(r/n) if (r/n).exists() else None for n in sorted(paths-set(producer['external_paths']))};assert all(owned.values())
old=json.loads(Path(producer['scope']).read_text())['owned_hashes'];assert set(old)<=set(owned)
delta={n:h for n,h in owned.items() if old.get(n)!=h};scope=l/'author-scope-code-r2.json';save(scope,{'owned_hashes':owned,'owned_count':len(owned),'phase_delta':delta,'phase_delta_count':len(delta),'producer_source':producer['source_digest']})
archive=l/'AH-P06-B1-R2-owned-source.tar'
with tarfile.open(archive,'w') as t:
 for n in owned:t.add(r/n,arcname=n)
with tarfile.open(producer['owned_archive'],'r:*') as t:
 diff=[]
 for n in delta:
  before=t.extractfile(n).read().decode().splitlines(keepends=True) if n in old else []
  diff.extend(difflib.unified_diff(before,(r/n).read_text().splitlines(keepends=True),fromfile='a/'+n,tofile='b/'+n))
(l/'AH-P06-B1-R2-producer-delta.patch').write_text(''.join(diff))
skill='/private/tmp/openbot-ah-p05-workflow-skill';env=os.environ.copy();env['PATH']=str(l/'tools')+os.pathsep+env['PATH'];snapshot=subprocess.check_output(['bash',skill+'/scripts/snapshot.sh',*excludes],text=True,env=env);digest=re.search(r'^digest: (\w+)$',snapshot,re.M)[1];(l/'AH-P06-B1-R2-author-snapshot.txt').write_text(snapshot)
auth=Path('/Users/mac02/dev/openbot/plan/04-autonomous-agent-history/COORDINATION.md');pin=l/'coordination-r29.md';assert sha(auth)==sha(pin)==sha(r/'plan/04-autonomous-agent-history/COORDINATION.md')
def record(p):p=Path(p);return {'path':str(p.relative_to(l.parent)),'sha256':sha(p)}
checks=[]
criteria={'p06-corrections-02':['AC3','AC4','AC6','AC7','AH-P06-B1-F1','AH-P06-B1-F2'],'p06-pressure-recheck-01':['AC6','AC7'],'p06-final-inspector-01':['AC1','AC2','AC3','AC4','AC5','AC6','AC7'],'p06-browser-03':['AC1','AC2','AC3','AC4'],'p06-protocols-02':['AC1','AC3','AC8'],'p06-decision-fixtures-01':['AC2','AC3','AC6','AC7','AC8'],'p06-decision-browser-01':['AC2','AC3','AC8'],'p06-case-evidence-01':['AC1','AC2','AC3','AC4','AC5','AC6','AC7','AC8']}
for n,d in receipts.items():
 checks.append({'criteria':criteria.get(n,['AC8']),'command':' '.join(d['command']),'fixture':'P06 owned PG18.6 loopback65037; stock2MiB; scripted providers; explicit installed Chrome for browser gates','status':'PASS','files':[record(l/(n+'.json')),record(d['log'])]})
checks.append({'criteria':['AC1','AC2','AC3','AC4','AC5','AC6','AC7','AC8'],'command':'python3 plan/04-autonomous-agent-history/phases/p06/docs/logs/verify-case-evidence.py','fixture':'current discovery and latest successful per-case observations; exact R1/R2 source/fixture/env impact dispositions in current inventory','status':'PASS','files':[record(l/'verification-case-inventory-r2.json'),*[record(l/(n+e)) for n in ['p06-final-history-01','p06-final-focused-runtime-01','p06-final-inspector-01','p06-decision-fixtures-01','p06-corrections-02','p06-correction-affected-runtime-01','p06-pressure-recheck-01'] for e in ['.json','.txt']]]})
for n,reason in [('p06-final-history-01','sole positive member grant fixture required provenance; current inspector/focused cases corrected'),('p06-final-focused-runtime-01','sole source collector fixture upgraded for retained-history pin/native expiry; decision-fixtures01 correction'),('p06-protocols-01','missing explicit Chrome and decision fixtures; protocols02 and generated decision fixtures/browser correction'),('p06-correction-affected-runtime-01','186PASS/2FAIL: cancellation pressure fixtures now use completed own jobs or explicitly elapsed original retry window; all original thresholds/assertions preserved and pressure-recheck01 PASS2'),('p06-corrections-01','compile-only test imports, corrected before actual five-case PASS02')]:
 d=json.loads((l/(n+'.json')).read_text());assert d['exit_code']!=0
 checks.append({'criteria':['AC8'],'command':' '.join(d['command']),'fixture':reason,'status':'FAIL','files':[record(l/(n+'.json')),record(d['log'])]})
evidence=l/'AH-P06-B1-R2.evidence.json';save(evidence,{'version':1,'round':'AH-P06-B1-R2','artifact':digest,'checks':checks})
v=subprocess.run(['python3',skill+'/scripts/check-evidence.py',str(evidence),'--expect-artifact',digest],capture_output=True,text=True);(l/'AH-P06-B1-R2-evidence-validation.txt').write_text(v.stdout+v.stderr);assert v.returncode==0,v.stdout+v.stderr
m={'review':'AH-P06-B1-R2','stage':'consolidated F1/F2 code correction recheck; original whole61-path coverage reused','at':datetime.now(timezone.utc).isoformat(),'worktree':str(r),'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip(),'phase_start':producer['phase_start'],'review_base':producer['review_base'],'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_digest':digest,'exclusions':excludes,'snapshot_adapter':str(l/'tools'),'skill':skill,'skill_sha256':sha(Path(skill)/'SKILL.md'),'decision':str(l/'AH-P06-D1-r3.md'),'decision_revision':'AH-P06-D1/r3','decision_sha256':sha(l/'AH-P06-D1-r3.md'),'coordination_authority':str(auth),'coordination_revision':29,'coordination_sha256':sha(auth),'coordination_pin':str(pin),'notices':[*['AH-I00'+str(n)+'/r1' for n in range(1,8)],'AH-C001/r1','AH-C002/r5','AH-C003/r2','AH-C004/r3','AH-C005/r4','AH-C006/r2','AH-C007/r3'],'scope':str(scope),'scope_sha256':sha(scope),'owned_count':len(owned),'phase_delta_count':len(delta),'owned_archive':str(archive),'owned_archive_sha256':sha(archive),'external_paths':producer['external_paths'],'producer_source':producer['source_digest'],'producer_archive':producer['owned_archive'],'producer_archive_sha256':producer['owned_archive_sha256'],'producer_acceptance':str(l/'producer-acceptance.json'),'evidence':str(evidence),'evidence_sha256':sha(evidence),'case_inventory':str(l/'verification-case-inventory-r2.json'),'case_inventory_sha256':sha(l/'verification-case-inventory-r2.json'),'checks_in_progress':False,'source_status':'FROZEN for whole06review; targeted-only; failed commands retained and explicit per-case correction/reuse'}
save(l/'AH-P06-B1-R2-input.json',m);print(json.dumps({'source':digest,'owned':len(owned),'phase_delta':len(delta),'archive':sha(archive),'cases':coverage['selected'],'validator':'PASS retention/schema/hash only'},indent=2))
