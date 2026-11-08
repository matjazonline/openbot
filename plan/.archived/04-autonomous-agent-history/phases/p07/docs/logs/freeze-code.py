from pathlib import Path
from datetime import datetime,timezone
import os,json,hashlib,subprocess,tarfile,re,difflib
r=Path.cwd();l=r/'plan/04-autonomous-agent-history/phases/p07/docs/logs'
producer=json.loads((l/'producer-input.json').read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,d):p.write_text(json.dumps(d,indent=2)+'\n')
for n,h in producer['external_paths'].items():assert (sha(r/n) if (r/n).exists() else None)==h,n
required=['p07-final-runtime-01','p07-e2e-completion-01','p07-discovery-02','p07-source-modules-01','p07-migrate-validate-01','p07-sqlx-prepare-01','p07-sqlx-check-01','p07-sqlx-check-02','p07-inheritance-current-01','p07-process-current-01','p07-offline-03','p07-clippy-03','p07-fmt-03','p07-diff-03','p07-graft-03','p07-browser-01','p07-protocols-01','p07-query-statistics-02','p07-sample-boundary-01','p07-sample-boundary-fixtures-01','p07-css-check-02','p07-ci-yaml-02','p07-case-evidence-02']
receipts={n:json.loads((l/(n+'.json')).read_text()) for n in required}
assert all(d['exit_code']==0 for d in receipts.values())
coverage=json.loads((l/'verification-case-inventory.json').read_text());assert coverage['status']=='PASS'
excludes=producer['exclusions'];spec=['.',*[':(top,exclude,literal)'+p for p in excludes]];paths=set()
for c in [['git','diff','--name-only','HEAD','--',*spec],['git','ls-files','--others','--exclude-standard','--',*spec]]:paths.update(subprocess.check_output(c,text=True).splitlines())
owned={n:sha(r/n) if (r/n).exists() else None for n in sorted(paths-set(producer['external_paths']))};assert all(owned.values())
old=json.loads(Path(producer['scope']).read_text())['owned_hashes'];assert set(old)<=set(owned)
delta={n:h for n,h in owned.items() if old.get(n)!=h}
scope=l/'author-scope-code-r1.json';save(scope,{'owned_hashes':owned,'owned_count':len(owned),'phase_delta':delta,'phase_delta_count':len(delta),'producer_source':producer['source_digest']})
archive=l/'AH-P07-B1-R1-owned-source.tar'
with tarfile.open(archive,'w') as t:
 for n in owned:t.add(r/n,arcname=n)
with tarfile.open(producer['owned_archive'],'r:*') as t:
 diff=[]
 for n in delta:
  before=t.extractfile(n).read().decode().splitlines(keepends=True) if n in old else []
  diff.extend(difflib.unified_diff(before,(r/n).read_text().splitlines(keepends=True),fromfile='a/'+n,tofile='b/'+n))
(l/'AH-P07-B1-R1-producer-delta.patch').write_text(''.join(diff))
skill='/private/tmp/openbot-ah-p05-workflow-skill';env=os.environ.copy();env['PATH']=str(l/'tools')+os.pathsep+env['PATH']
snapshot=subprocess.check_output(['bash',skill+'/scripts/snapshot.sh',*excludes],text=True,env=env)
digest=re.search(r'^digest: (\w+)$',snapshot,re.M)[1];(l/'AH-P07-B1-R1-author-snapshot.txt').write_text(snapshot)
auth=Path('/Users/mac02/dev/openbot/plan/04-autonomous-agent-history/COORDINATION.md');pin=l/'coordination-r32.md';assert sha(auth)==sha(pin)==sha(r/'plan/04-autonomous-agent-history/COORDINATION.md')
def record(p):p=Path(p);return {'path':str(p.relative_to(l.parent)),'sha256':sha(p)}
checks=[]
for n,d in receipts.items():
 criteria=['AC13'] if n not in ['p07-final-runtime-01','p07-e2e-completion-01'] else ['AC'+str(i) for i in range(1,14)]
 checks.append({'criteria':criteria,'command':' '.join(d['command']),'fixture':'P07 fresh owned PG18.6 loopback65037; stock2MiB/network guard; counted strict scripted providers; installed Chrome; exact per-case map and shared-contract impact in verification-final-map.md','status':'PASS','files':[record(l/(n+'.json')),record(d['log'])]})
checks.append({'criteria':['AC'+str(i) for i in range(1,14)],'command':'audit actual current discovery against per-case successful observations','fixture':'all accepted866 names retained; native883 current latestPASS, helper excluded standalone but invoked/killed by6 required frontier cases; no old runtime reuse','status':'PASS','files':[record(l/'verification-case-inventory.json'),record(l/'verification-final-map.md')]})
for n,reason in [('p07-process-frontiers-01','URL validator incorrectly parsed valid fixture trailing slash; replaced with real Url parser'),('p07-process-frontiers-02','child claim marker unavailable before inherited fixture registry was implemented; stderr visibility fixed'),('p07-process-frontiers-03','real child process ordinary endpoint guard correctly refused unregistered parent; one-use mutually authenticated cfgtest inheritance implemented'),('p07-process-frontiers-04','author fixture used invalid5ms poll; kept native50ms minimum'),('p07-process-frontiers-05','native durable Frozen is queued+active pointer before checkpoint; exact native frontier assertion fixed'),('p07-knowledge-journey-01','compile-only invalid observation field on Deployment'),('p07-knowledge-journey-02','author read query referenced nonexistent input column; native execution frozen_inputs.context is actual digest source'),('p07-css-check-01','generated CSS stale; actual build refreshed and check02 PASS'),('p07-query-statistics-01','overstrict spelling on versus auto; actual nonnull queryid is activation requirement and probe02 proves it without shared-server configuration changes'),('p07-ci-yaml-01','local Ruby version lacks filter_map; compatible map.compact parse02 PASS')]:
 d=json.loads((l/(n+'.json')).read_text());assert d['exit_code']!=0
 checks.append({'criteria':['AC13'],'command':' '.join(d['command']),'fixture':reason,'status':'FAIL','files':[record(l/(n+'.json')),record(d['log'])]})
checks.append({'criteria':['AC13'],'command':'GitHub hosted workflow execution and actionlint','fixture':'new owned CI workflow parsed; actual native/browser/query-stat commands exercised locally','status':'NOT EXERCISED','reason':'Remote CI was not triggered from this uncommitted branch; local actionlint executable unavailable. Neither substitutes for or blocks the required independently reviewed local checks.'})
evidence=l/'AH-P07-B1-R1.evidence.json';save(evidence,{'version':1,'round':'AH-P07-B1-R1','artifact':digest,'checks':checks})
v=subprocess.run(['python3',skill+'/scripts/check-evidence.py',str(evidence),'--expect-artifact',digest],capture_output=True,text=True);(l/'AH-P07-B1-R1-evidence-validation.txt').write_text(v.stdout+v.stderr);assert v.returncode==0,v.stdout+v.stderr
m={'review':'AH-P07-B1-R1','stage':'whole07A/B code review','at':datetime.now(timezone.utc).isoformat(),'worktree':str(r),'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip(),'phase_start':producer['phase_start'],'review_base':producer['review_base'],'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_digest':digest,'exclusions':excludes,'snapshot_adapter':str(l/'tools'),'skill':skill,'skill_sha256':sha(Path(skill)/'SKILL.md'),'coordination_authority':str(auth),'coordination_revision':32,'coordination_sha256':sha(auth),'coordination_pin':str(pin),'notices':[*['AH-I00'+str(n)+'/r1' for n in range(1,8)],'AH-C001/r1','AH-C002/r5','AH-C003/r2','AH-C004/r3','AH-C005/r4','AH-C006/r2','AH-C007/r4'],'scope':str(scope),'scope_sha256':sha(scope),'owned_count':len(owned),'phase_delta_count':len(delta),'owned_archive':str(archive),'owned_archive_sha256':sha(archive),'external_paths':producer['external_paths'],'producer_source':producer['source_digest'],'producer_archive':producer['owned_archive'],'producer_archive_sha256':producer['owned_archive_sha256'],'producer_acceptance':str(l/'producer-acceptance.json'),'evidence':str(evidence),'evidence_sha256':sha(evidence),'case_inventory':str(l/'verification-case-inventory.json'),'case_inventory_sha256':sha(l/'verification-case-inventory.json'),'checks_in_progress':False,'source_status':'FROZEN for whole07review; targeted-only; no final primaryintegration acceptance'}
save(l/'AH-P07-B1-R1-input.json',m);print(json.dumps({'source':digest,'owned':len(owned),'phase_delta':len(delta),'archive':sha(archive),'cases':coverage['selected'],'validator':'PASS retention/schema/hash only'},indent=2))
