from pathlib import Path
import datetime,difflib,hashlib,json,os,subprocess,tarfile

root=Path.cwd(); logs=root/'plan/04-autonomous-agent-history/phases/p02/docs/logs'
setup=json.loads((logs/'setup.json').read_text()); producer=json.loads(Path(setup['producer_scope']).read_text())
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
for name,want in setup['external_paths'].items():
 p=root/name;got=sha(p) if p.exists() else None
 assert got==want,(name,got,want)
coord=Path('/Users/mac02/dev/openbot/plan/04-autonomous-agent-history/COORDINATION.md')
assert coord.read_bytes()==(logs/'coordination-r5.md').read_bytes()
changed=subprocess.check_output(['git','diff','--name-only','HEAD','-z']).decode().split('\0')
untracked=subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z']).decode().split('\0')
owned=sorted(n for n in set(changed+untracked) if n and n not in setup['external_paths'] and not any(n==e or n.startswith(e+'/') for e in setup['exclusions']))
hashes={name:sha(root/name) if (root/name).exists() else None for name in owned}
assert set(producer['owned_hashes']).issubset(hashes)
delta=[name for name in owned if hashes[name]!=producer['owned_hashes'].get(name)]
patch=[]
with tarfile.open(setup['producer_archive']) as archive:
 for name in delta:
  if name in producer['owned_hashes']:
   before=archive.extractfile(name).read().decode()
  else:
   r=subprocess.run(['git','show','HEAD:'+name],capture_output=True)
   before=r.stdout.decode() if r.returncode==0 else ''
  after=(root/name).read_text() if (root/name).exists() else ''
  patch.extend(difflib.unified_diff(before.splitlines(True),after.splitlines(True),fromfile='accepted-p01/'+name,tofile='p02/'+name))
(logs/'AH-P02-B1-R2.diff').write_text(''.join(patch))
archive=logs/'AH-P02-B1-R2-owned-source.tar.gz'
with tarfile.open(archive,'w:gz') as stream:
 for name in owned:
  if (root/name).exists():stream.add(root/name,arcname=name)
checks={}
for name in ['migrations-main-findings-02','migrations-test-findings-02','findings-affected-runtime-04','findings-history-runtime-03','sqlx-prepare-findings-02','sqlx-check-findings-02','offline-all-targets-findings-02','clippy-findings-02','fmt-findings-02','diff-findings-02','graft-findings-02']:
 p=logs/(name+'.json');receipt=json.loads(p.read_text());assert receipt['exit_code']==0,name
 checks[name]={'receipt':str(p),'sha256':sha(p),'log':receipt['log'],'log_sha256':sha(receipt['log'])}
scoped={'base':setup['review_base'],'external_count':len(setup['external_paths']),'owned_count':len(owned),'owned_hashes':hashes,'phase02_delta_paths':delta,'exclusions':setup['exclusions'],'purpose':'P01 accepted dependency plus complete P02 source; external 36 inputs preserved, included in snapshot but not claimed as authored.'}
(logs/'author-scope-findings-02.json').write_text(json.dumps(scoped,indent=2)+'\n')
skill=Path(setup['skill']);env=os.environ.copy();env['PATH']=str(logs/'tools')+os.pathsep+env['PATH']
snapshot=subprocess.check_output(['bash',str(skill/'scripts/snapshot.sh'),*setup['exclusions']],cwd=root,env=env).decode()
(logs/'AH-P02-B1-R2-snapshot.txt').write_text(snapshot)
digest=next(line.split(':',1)[1].strip() for line in snapshot.splitlines() if line.startswith('digest:'))
review={'review':'AH-P02-B1-R2','stage':'correction recheck of complete F1/F2 batch','at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'worktree':str(root),'branch':setup['branch'],'phase_start':setup['start_commit'],'review_base':setup['review_base'],'head':subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'source_digest':digest,'snapshot':str(logs/'AH-P02-B1-R2-snapshot.txt'),'exclusions':setup['exclusions'],'skill':str(skill),'snapshot_path_adapter':str(logs/'tools'),'coordination_authority':str(coord),'coordination_revision':5,'coordination_sha256':sha(coord),'coordination_pin':str(logs/'coordination-r5.md'),'notices':['AH-I001/r1','AH-I002/r1','AH-I003/r1','AH-I004/r1','AH-I005/r1','AH-I006/r1','AH-C001/r1','AH-C002/r4','AH-C003/r1'],'producer_source':setup['producer_source'],'producer_archive':setup['producer_archive'],'producer_archive_sha256':sha(setup['producer_archive']),'producer_scope':setup['producer_scope'],'author_scope':str(logs/'author-scope-findings-02.json'),'author_scope_sha256':sha(logs/'author-scope-findings-02.json'),'owned_count':len(owned),'phase02_delta_count':len(delta),'phase02_delta':str(logs/'AH-P02-B1-R2.diff'),'phase02_delta_sha256':sha(logs/'AH-P02-B1-R2.diff'),'source_archive':str(archive),'source_archive_sha256':sha(archive),'external_paths':setup['external_paths'],'architecture_decision':'AH-P02-D1/r2','architecture_decision_sha256':sha(logs/'AH-P02-D1-r2.md'),'architecture_acceptance':str(logs/'AH-P02-architecture-acceptance.json'),'targeted_evidence':str(logs/'AH-P02-targeted-runtime-evidence-r2.json'),'checks':checks,'verification_scope':'Targeted only. Actual current 58 history cases include genuine deletion/retry contention and real applied/aborted COMMIT ambiguity; affected cohort and prior unchanged retention evidence pinned separately. No full suite or live provider claim. Final acknowledgement test uses a counted scripted loopback provider.'}
prior=json.loads((logs/'AH-P02-B1-R1-input.json').read_text())
prior_scope=json.loads(Path(prior['author_scope']).read_text())
correction_paths=[name for name in owned if hashes[name]!=prior_scope['owned_hashes'].get(name)]
correction=[]
with tarfile.open(prior['source_archive']) as stream:
 for name in correction_paths:
  before=stream.extractfile(name).read().decode() if name in prior_scope['owned_hashes'] else ''
  after=(root/name).read_text() if (root/name).exists() else ''
  correction.extend(difflib.unified_diff(before.splitlines(True),after.splitlines(True),fromfile='review-r1/'+name,tofile='review-r2/'+name))
correction_file=logs/'AH-P02-B1-R1-to-R2.diff';correction_file.write_text(''.join(correction))
review.update({'recheck_of':str(logs/'AH-P02-B1-R1-input.json'),'prior_source_digest':prior['source_digest'],'prior_source_archive':prior['source_archive'],'prior_source_archive_sha256':prior['source_archive_sha256'],'finding_ids':['AH-P02-B1-F1','AH-P02-B1-F2'],'correction_paths':correction_paths,'correction_diff':str(correction_file),'correction_diff_sha256':sha(correction_file),'correction_record':str(logs/'AH-P02-B1-corrections.md')})
(logs/'AH-P02-B1-R2-input.json').write_text(json.dumps(review,indent=2)+'\n')
print(json.dumps({k:review[k] for k in ['source_digest','owned_count','phase02_delta_count','source_archive_sha256']},indent=2))
