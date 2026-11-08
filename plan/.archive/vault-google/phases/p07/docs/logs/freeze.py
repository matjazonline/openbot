import subprocess,tarfile,hashlib,json,sys,difflib,shutil,stat,os
from pathlib import Path
root=Path.cwd();logs=Path(__file__).parent;round=sys.argv[1]
ex=['plan/vault-google/COORDINATION.md','plan/vault-google/phases/p06/docs','plan/vault-google/phases/p07/docs']
env=os.environ.copy();env['PATH']='/tmp/vault-google-tools:'+env['PATH']
skill=root/'.agents/skills/implement-astra-reviewed-2-2'
r=subprocess.run(['bash',str(skill/'scripts/snapshot.sh'),*ex],env=env,text=True,capture_output=True,check=True)
(logs/('snapshot-'+round+'.log')).write_text(r.stdout)
digest=r.stdout.split('digest: ')[1].splitlines()[0];head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
paths=set(subprocess.check_output(['git','diff','HEAD','--name-only','-z']).decode().split('\0'))|set(subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z']).decode().split('\0'));paths={p for p in paths if p and not any(p==e or p.startswith(e+'/') for e in ex)}
with tarfile.open(root/'plan/vault-google/phases/p06/docs/logs/source-B1-R2.tar.gz') as t:old={m.name:t.extractfile(m).read() for m in t.getmembers() if m.isfile()}
changes=[];patch=[];sha=lambda x:hashlib.sha256(x).hexdigest() if x is not None else None
for p in sorted(paths|old.keys()):
 before=old.get(p)
 if before is None:
  q=subprocess.run(['git','show',head+':'+p],capture_output=True);before=q.stdout if q.returncode==0 else None
 after=(root/p).read_bytes() if (root/p).is_file() else None
 if before!=after:
  changes.append({'path':p,'before_sha256':sha(before),'after_sha256':sha(after),'old_lines':len((before or b'').splitlines()),'new_lines':len((after or b'').splitlines())})
  patch.extend(difflib.unified_diff((before or b'').decode().splitlines(True),(after or b'').decode().splitlines(True),fromfile='accepted06/'+p,tofile='p07/'+p))
with tarfile.open(logs/('source-'+round+'.tar.gz'),'w:gz') as t:
 for p in sorted(paths):
  if (root/p).exists():t.add(root/p,arcname=p,recursive=False)
coord=root/'plan/vault-google/COORDINATION.md';pinned=logs/('coordination-'+round+'-rev23.md');shutil.copyfile(coord,pinned)
req=root/'plan/vault-google/07-integration-verification-and-operations.md'
manifest={'phase':'p07','round':round,'start':head,'review_base':head,'head':head,'digest':digest,'exclusions':ex,'coordination_revision':23,'coordination_sha256':sha(coord.read_bytes()),'coordination_pin':str(pinned.relative_to(root)),'requirements_sha256':sha(req.read_bytes()),'decision':'VG-P07-D1/r1','decision_sha256':'241766fb8efd2af70cbdc5c8d8a5e7dcb23dcd4db63e7db3c1e52b4937049484','accepted06_digest':'d9d84e7e35bc0a1d603f6f57bf2ac6ba5a9c95a133611bba00b432fe31533b71','accepted06_archive':'plan/vault-google/phases/p06/docs/logs/source-B1-R2.tar.gz','whole_source_paths':len(paths),'phase07_paths':len(changes),'delta':changes}
(logs/('delta-'+round+'.patch')).write_text(''.join(patch));(logs/('manifest-'+round+'.json')).write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps({k:v for k,v in manifest.items() if k!='delta'},indent=2))
