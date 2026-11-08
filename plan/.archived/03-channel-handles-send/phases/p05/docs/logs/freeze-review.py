#!/usr/bin/env python3
import difflib,hashlib,json,os,subprocess,tarfile
from pathlib import Path
root=Path(__file__).resolve().parents[6]
logs=Path(__file__).resolve().parent
os.chdir(root)
previous=root/'plan/03-channel-handles-send/phases/p04/docs/logs/CHS-P04-B1-R2-manifest.json'
base=json.loads(previous.read_text()); excludes=base['exclusions']
env=os.environ.copy();env['PATH']=str(root/'plan/03-channel-handles-send/phases/p01/docs/logs/tools')+os.pathsep+env['PATH']
snapshot=subprocess.check_output(['bash',str(root/'.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh'),*excludes],env=env,text=True)
(logs/'CHS-P05-B1-R1-snapshot.txt').write_text(snapshot)
head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(); digest=snapshot.split('digest: ',1)[1].splitlines()[0].strip()
paths=[p[2:] for p in snapshot.split('files:\n',1)[1].splitlines()]
files={p:hashlib.sha256(Path(p).read_bytes()).hexdigest() if Path(p).is_file() else None for p in paths}
changed=sorted(p for p in set(files)|set(base['source_files']) if files.get(p)!=base['source_files'].get(p))
archive=logs/'CHS-P05-B1-R1-source.tar.gz'
with tarfile.open(archive,'w:gz') as tar:
 for p in paths:
  if Path(p).exists():tar.add(p,arcname=p,recursive=False)
prior_archive=root/'plan/03-channel-handles-send/phases/p04/docs/logs/CHS-P04-B1-R2-source.tar.gz'
patch=[]
with tarfile.open(prior_archive) as prior:
 names=set(prior.getnames())
 for p in changed:
  old=prior.extractfile(p).read().decode().splitlines(keepends=True) if p in names else []
  new=Path(p).read_text().splitlines(keepends=True) if Path(p).is_file() else []
  patch.extend(difflib.unified_diff(old,new,fromfile='a/'+p if old else '/dev/null',tofile='b/'+p if new else '/dev/null'))
(logs/'CHS-P05-B1-R1-p05.diff').write_text(''.join(patch))
coord=root/'plan/03-channel-handles-send/COORDINATION.md';pin=logs/'coordination-r14.md'
assert coord.read_bytes()==pin.read_bytes()
m={'phase':'p05','batch':'B1','start':base['head'],'review_base':base['head'],'head':head,'digest':digest,'exclusions':excludes,
 'source_files':files,'deleted_files':[p for p,h in files.items() if h is None],'preserved_external_changes':['plan/local_machine_tunnel_plan.md'],
 'p05_changed_files':changed,'preserved_prerequisite_manifest':str(previous),'previous_digest':base['digest'],
 'source_archive':str(archive),'source_archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),
 'p05_delta':str(logs/'CHS-P05-B1-R1-p05.diff'),'architecture':'CHS-P05-D1/r1','architecture_pass':'CHS-P05-AG1-R1',
 'decision_sha256':hashlib.sha256((logs/'CHS-P05-D1-r1.md').read_bytes()).hexdigest(),
 'coordination_revision':14,'coordination_pin':str(pin),'coordination_sha256':hashlib.sha256(pin.read_bytes()).hexdigest(),
 'notice_revisions':['I001-I003/r1','I004/r2','I005-I006/r1','C001/r1','C002/r1','C003/r2','C004/r2','C005/r1'],
 'checks':str(logs/'current-affected-summary-01.json'),'full_suite':False}
(logs/'CHS-P05-B1-R1-manifest.json').write_text(json.dumps(m,indent=2)+'\n')
print(json.dumps({'digest':digest,'cumulative_paths':len(paths),'p05_paths':len(changed),'archive_sha256':m['source_archive_sha256']}))
