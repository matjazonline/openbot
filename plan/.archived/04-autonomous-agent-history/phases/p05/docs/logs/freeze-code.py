import os,json,hashlib,subprocess,tarfile,re
from pathlib import Path
from datetime import datetime,timezone
r=Path.cwd();l=r/'plan/04-autonomous-agent-history/phases/p05/docs/logs'
producer=json.loads((l/'producer-input.json').read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,d):p.write_text(json.dumps(d,indent=2)+'\n')
for name,h in producer['external_paths'].items():
 p=r/name;assert (sha(p) if p.exists() else None)==h,name
required=['knowledge-cc-final02','cc-owner-final01','fmt-check-final02','diff-check-final02','source-modules-final01',
 'sqlx-prepare-final02','sqlx-check-final02','offline-final02','clippy-final02',
 'final-focused-list-02','sample-structural01','sample-boundary-fixtures-final01',
 'sample-node-final02','migrations-main-correction01','migrations-test-correction01','graft-build-final03']
receipts={name:json.loads((l/(name+'.json')).read_text()) for name in required}
assert all(v['exit_code']==0 for v in receipts.values())
coverage=json.loads((l/'verification-case-inventory.json').read_text());assert coverage['status']=='PASS'
excludes=producer['exclusions'];spec=['.',*[':(top,exclude,literal)'+p for p in excludes]];paths=set()
for cmd in [['git','diff','--name-only','HEAD','--',*spec],['git','ls-files','--others','--exclude-standard','--',*spec]]:
 paths.update(subprocess.check_output(cmd,text=True).splitlines())
owned={p:sha(r/p) if (r/p).exists() else None for p in sorted(paths-set(producer['external_paths']))}
assert all(v is not None for v in owned.values())
old=json.loads(Path(producer['scope']).read_text())['owned_hashes']
delta={p:h for p,h in owned.items() if old.get(p)!=h}
scope=l/'author-scope-code-r1.json';save(scope,{'owned_hashes':owned,'owned_count':len(owned),'phase_delta':delta,'phase_delta_count':len(delta),'producer_source':producer['source_digest']})
archive=l/'AH-P05-B1-R1-owned-source.tar'
with tarfile.open(archive,'w') as tar:
 for name in owned:tar.add(r/name,arcname=name)
assert subprocess.run(['git','check-ignore',str(archive)],stdout=subprocess.DEVNULL).returncode==1
skill='/private/tmp/openbot-ah-p05-workflow-skill';env=os.environ.copy();env['PATH']=str(l/'tools')+os.pathsep+env['PATH']
snapshot=subprocess.check_output(['bash',skill+'/scripts/snapshot.sh',*excludes],text=True,env=env)
(l/'AH-P05-B1-R1-author-snapshot.txt').write_text(snapshot);digest=re.search(r'^digest: (\w+)$',snapshot,re.M)[1]
check_digest=re.search(r'^digest: (\w+)$',(l/'current-final-check-source-r3.txt').read_text(),re.M)[1]
assert digest==check_digest,(digest,check_digest)
auth=Path('/Users/mac02/dev/openbot/plan/04-autonomous-agent-history/COORDINATION.md');pin=l/'coordination-r22.md'
assert sha(auth)==sha(pin)==sha(r/'plan/04-autonomous-agent-history/COORDINATION.md')
def file_record(p):
 p=Path(p);return {'path':str(p.relative_to(l.parent)),'sha256':sha(p)}
evidence=l/'AH-P05-B1-R1.evidence.json';checks=[]
for name,data in receipts.items():
 checks.append({'criteria':['AC1–AC8' if name=='final-focused-list-02' else name],
  'command':' '.join(data['command']),'fixture':'owned loopback PG18.6/P05; stock2MiB; scripted providers; Chrome explicit where named',
  'status':'PASS','files':[file_record(l/(name+'.json')),file_record(data['log'])]})
checks.append({'criteria':['AC1','AC2','AC3','AC4','AC5','AC6','AC8'],'command':'python3 plan/04-autonomous-agent-history/phases/p05/docs/logs/verify-case-evidence.py',
 'fixture':'same P05 owned PG/schema; current selected names and retained latest successful case observations; see reuse dispositions',
 'status':'PASS','files':[file_record(l/'verification-case-inventory.json'),file_record(l/'case-evidence-final01.txt'),
  *[file_record(l/(name+ext)) for name in ['final-focused-runtime-02','final-affected-recheck01','knowledge-cc-final02','cc-owner-final01'] for ext in ['.json','.txt']]]})
save(evidence,{'version':1,'round':'AH-P05-B1-R1','artifact':digest,'checks':checks})
validation=subprocess.run(['python3',skill+'/scripts/check-evidence.py',str(evidence),'--expect-artifact',digest],capture_output=True,text=True)
(l/'AH-P05-B1-R1-evidence-validation.txt').write_text(validation.stdout+validation.stderr);assert validation.returncode==0,validation.stdout+validation.stderr
manifest={'review':'AH-P05-B1-R1','stage':'whole05A/B code review; no code acceptance yet','at':datetime.now(timezone.utc).isoformat(),
 'worktree':str(r),'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip(),
 'phase_start':producer['phase_start'],'review_base':producer['review_base'],'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
 'source_digest':digest,'exclusions':excludes,'snapshot_adapter':str(l/'tools'),'skill':skill,'skill_sha256':sha(Path(skill)/'SKILL.md'),
 'decision':str(l/'AH-P05-D1-r1.md'),'decision_revision':'AH-P05-D1/r1','decision_sha256':sha(l/'AH-P05-D1-r1.md'),
 'coordination_authority':str(auth),'coordination_revision':22,'coordination_sha256':sha(auth),'coordination_pin':str(pin),
 'notices':[ *['AH-I00'+str(n)+'/r1' for n in range(1,8)],'AH-C001/r1','AH-C002/r5','AH-C003/r2','AH-C004/r3','AH-C005/r4','AH-C006/r1 architecture active'],
 'scope':str(scope),'scope_sha256':sha(scope),'owned_count':len(owned),'phase_delta_count':len(delta),
 'owned_archive':str(archive),'owned_archive_sha256':sha(archive),'external_paths':producer['external_paths'],
 'producer_source':producer['source_digest'],'producer_archive':producer['owned_archive'],'producer_archive_sha256':producer['owned_archive_sha256'],
 'producer_acceptance':str(r/'plan/04-autonomous-agent-history/phases/p04/docs/logs/AH-P04-code-acceptance.json'),
 'evidence':str(evidence),'evidence_sha256':sha(evidence),'case_inventory':str(l/'verification-case-inventory.json'),'case_inventory_sha256':sha(l/'verification-case-inventory.json'),
 'checks_in_progress':False,'source_status':'FROZEN for whole P05 review; targeted-only, explicit receipt reuse/failure dispositions'}
save(l/'AH-P05-B1-R1-input.json',manifest)
print(json.dumps({'digest':digest,'owned':len(owned),'phase_delta':len(delta),'archive_sha256':sha(archive),'case_passes':coverage['selected'],'evidence_validator':'PASS retention/schema/hash only'},indent=2))
