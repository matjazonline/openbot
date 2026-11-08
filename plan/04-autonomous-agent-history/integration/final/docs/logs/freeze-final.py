from pathlib import Path
from datetime import datetime,timezone
import json,hashlib,subprocess,os,tarfile,re
r=Path.cwd();l=r/'plan/04-autonomous-agent-history/integration/final/docs/logs'
i=json.loads((l/'producer-input.json').read_text());pre=json.loads((l/'primary-preflight-r1.json').read_text());a=json.loads((l/'assembly-r1.json').read_text());sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,d):p.write_text(json.dumps(d,indent=2)+'\n')
assert all(sha(r/n)==h for n,h in a['accepted216_hashes'].items());assert all((sha(r/n) if (r/n).exists() else None)==h for n,h in pre['preexisting_preserve'].items())
assert hashlib.sha256(subprocess.check_output(['git','ls-files','--stage','-z'])).hexdigest()==pre['staged_entries_sha256']
excludes=i['exclusions'];spec=['.',*[':(top,exclude,literal)'+p for p in excludes]];paths=set()
for cmd in [['git','diff','--name-only','HEAD','--',*spec],['git','ls-files','--others','--exclude-standard','--',*spec]]:paths.update(subprocess.check_output(cmd,text=True).splitlines())
assert paths==set(a['accepted216_hashes'])|set(pre['preexisting_preserve']),{'extra':sorted(paths-set(a['accepted216_hashes'])-set(pre['preexisting_preserve'])),'missing':sorted((set(a['accepted216_hashes'])|set(pre['preexisting_preserve']))-paths)}
required=['final-migrate-main-01','final-migrate-test-01','final-css-build-01','final-css-check-01','final-discovery-01','final-runtime-01','final-source-modules-01','final-sqlx-prepare-01','final-sqlx-check-01','final-offline-01','final-clippy-01','final-fmt-01','final-diff-01','final-graft-01','final-query-statistics-01','final-sample-boundary-01','final-sample-boundary-fixtures-01','final-browser-01','final-case-evidence-01']
receipts={n:json.loads((l/(n+'.json')).read_text()) for n in required};assert all(d['exit_code']==0 for d in receipts.values())
coverage=json.loads((l/'verification-final-inventory.json').read_text());assert coverage['status']=='PASS'
scope=l/'author-scope-final-r1.json';save(scope,{'owned_hashes':a['accepted216_hashes'],'owned_count':216,'integration_delta':{},'integration_delta_count':0,'preexisting_preserved':pre['preexisting_preserve'],'preexisting_count':77,'additional_primary_records':pre['additional_preserved_paths'],'producer_source':i['source_digest'],'assembly_receipt':str(l/'assembly-r1.json')})
archive=l/'AH-FINAL-B1-R1-owned-source.tar'
with tarfile.open(archive,'w') as t:
 for n in a['accepted216_hashes']:t.add(r/n,arcname=n)
env=os.environ.copy();env['PATH']=str(l/'tools')+os.pathsep+env['PATH'];snap=subprocess.check_output(['bash',i['skill']+'/scripts/snapshot.sh',*excludes],env=env,text=True);digest=re.search(r'^digest: (\w+)$',snap,re.M)[1];(l/'AH-FINAL-B1-R1-author-snapshot.txt').write_text(snap)
auth=r/'plan/04-autonomous-agent-history/COORDINATION.md';pin=l/'coordination-r34.md';assert sha(auth)==sha(pin)
def record(p):p=Path(p);return {'path':str(p.relative_to(l.parent)),'sha256':sha(p)}
checks=[]
for n,d in receipts.items():
 criteria=['FI2']
 if n in ['final-runtime-01','final-discovery-01']:criteria=['FI3']
 if 'css' in n or 'browser' in n:criteria=['FI4']
 if n=='final-case-evidence-01':criteria=['FI1','FI3','FI5']
 checks.append({'criteria':criteria,'command':' '.join(d['command']),'fixture':'Final primary main4ab45555 with exact accepted216 source/77 preserved inputs, freshowned PG18.6 localhost65037 finalpair, stock2MiB/networkguard, counted strict providers and actual native HTML/installedChrome','status':'PASS','files':[record(l/(n+'.json')),record(d['log'])]})
# Copy exact reusable raw evidence into final records so the version1 manifest is self-contained.
reuse=l/'reused-p07';reuse.mkdir(exist_ok=True)
import shutil
p07=r/'plan/04-autonomous-agent-history/phases/p07/docs/logs'
reused=[]
for n in ['verification-case-inventory.json','verification-final-map.md',*[run+ext for run in ['p07-final-runtime-01','p07-e2e-completion-01','p07-inheritance-current-01','p07-process-current-01','p07-protocols-01','p07-ci-yaml-02'] for ext in ['.json','.txt']]]:
 shutil.copy2(p07/n,reuse/n);reused.append(record(reuse/n))
checks.append({'criteria':['FI1','FI3','FI4','FI5'],'command':'exact source/dependency equality plus original wholephase883/protocol17/CI parse receipt audit','fixture':'Accepted07 code/schema/dependencies/CSS are byteidentical after final generation; all883 original phase cases have current original observations and31 subset rerun on primary. Preserve explicit original ambiguity/transport/browser/hostedCI limitations.','status':'PASS','files':[record(l/'verification-final-inventory.json'),*reused]})
checks.append({'criteria':['FI5'],'command':'fullruntime/stack/lease/hostedCI/liveprovider/deployment','fixture':'Usertargeted-only scope and local uncommitted integration','status':'NOT EXERCISED','reason':'Full runtime/stack/lease suites explicitly excluded by user; remoteCI/liveprovider/deployment outside authorized local completion. No such pass inferred.'})
evidence=l/'AH-FINAL-B1-R1.evidence.json';save(evidence,{'version':1,'round':'AH-FINAL-B1-R1','artifact':digest,'checks':checks})
v=subprocess.run(['python3',i['skill']+'/scripts/check-evidence.py',str(evidence),'--expect-artifact',digest],capture_output=True,text=True);(l/'AH-FINAL-B1-R1-evidence-validation.txt').write_text(v.stdout+v.stderr);assert v.returncode==0,v.stdout+v.stderr
m={'review':'AH-FINAL-B1-R1','stage':'final combined localprimary integration review','at':datetime.now(timezone.utc).isoformat(),'worktree':str(r),'branch':subprocess.check_output(['git','branch','--show-current'],text=True).strip(),'phase_start':i['phase_start'],'review_base':i['review_base'],'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_digest':digest,'exclusions':excludes,'snapshot_adapter':str(l/'tools'),'skill':i['skill'],'skill_sha256':i['skill_sha256'],'coordination_authority':str(auth),'coordination_revision':34,'coordination_sha256':sha(auth),'coordination_pin':str(pin),'notices':[*i['notices'],'AH-C008/r1'],'scope':str(scope),'scope_sha256':sha(scope),'owned_count':216,'integration_delta_count':0,'owned_archive':str(archive),'owned_archive_sha256':sha(archive),'external_paths':pre['preexisting_preserve'],'external_count':77,'original_external36':i['external_paths'],'producer_source':i['source_digest'],'producer_archive':i['owned_archive'],'producer_archive_sha256':i['owned_archive_sha256'],'producer_acceptance':str(l/'producer-acceptance.json'),'input_phase_acceptances':str(l/'accepted-phase-inputs.json'),'evidence':str(evidence),'evidence_sha256':sha(evidence),'case_inventory':str(l/'verification-final-inventory.json'),'case_inventory_sha256':sha(l/'verification-final-inventory.json'),'checks_in_progress':False,'source_status':'FROZEN finalprimary; no commits/pushes/staging; independent finalacceptance pending'}
save(l/'AH-FINAL-B1-R1-input.json',m);print(json.dumps({'source':digest,'owned':216,'preserved':77,'integration_delta':0,'archive':sha(archive),'native_cases':31,'reusable_phase_cases':883,'validator':'PASS retention/schema/hash only'},indent=2))
