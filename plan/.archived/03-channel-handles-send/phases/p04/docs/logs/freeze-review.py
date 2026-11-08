#!/usr/bin/env python3
"""Freeze the entire source tree and p04 delta; preserve external deletions explicitly."""
import difflib,hashlib,json,os,subprocess,sys,tarfile
from pathlib import Path
root=Path.cwd();logs=Path(__file__).resolve().parent
previous=root/'plan/03-channel-handles-send/phases/p03/docs/logs/CHS-P03-B1-R2-manifest.json'
base=json.loads(previous.read_text());exclusions=base['exclusions']
identity=sys.argv[1]
assert not (logs/(identity+'-manifest.json')).exists(),'Never overwrite a reviewed identity'
environment=os.environ.copy()
environment['PATH']=str(root/'plan/03-channel-handles-send/phases/p01/docs/logs/tools')+os.pathsep+environment['PATH']
snapshot=subprocess.check_output(['bash',str(root/'.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh'),*exclusions],env=environment,text=True)
(logs/(identity+'-snapshot.txt')).write_text(snapshot)
digest=next(line.split()[1] for line in snapshot.splitlines() if line.startswith('digest:'))
paths=[line.strip() for line in snapshot.splitlines() if line.startswith('  ')]
sha=lambda data:hashlib.sha256(data).hexdigest()
with tarfile.open(previous.parent/base['source_archive']) as archive:
    saved={member.name:archive.extractfile(member).read() for member in archive.getmembers() if member.isfile()}
def original(path):
    if path in saved:return saved[path]
    result=subprocess.run(['git','show',base['head']+':'+path],capture_output=True)
    return result.stdout if result.returncode==0 else None
current={path:(root/path).read_bytes() if (root/path).exists() else None for path in paths}
changed=[];delta=[]
for path in paths:
    old=original(path);new=current[path]
    if old!=new:
        changed.append(path)
        delta.extend(difflib.unified_diff((old or b'').decode().splitlines(keepends=True),(new or b'').decode().splitlines(keepends=True),fromfile='a/'+path,tofile='b/'+path))
(logs/(identity+'-p04.diff')).write_text(''.join(delta))
archive_path=logs/(identity+'-source.tar.gz')
with tarfile.open(archive_path,'w:gz') as archive:
    for path in paths:
        if current[path] is not None:archive.add(root/path,arcname=path,recursive=False)
coord=root/'plan/03-channel-handles-send/COORDINATION.md'
pinned=logs/'coordination-r11.md'
assert coord.read_bytes()==pinned.read_bytes(),'Reconcile coordination before freezing'
external=['plan/local_machine_tunnel_plan.md']
assert current.get(external[0],'unexpected') is None,'Reconcile preserved external deletion'
manifest={
    'phase':'p04','batch':'CHS-P04-B1','start':base['head'],'review_base':base['head'],
    'head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'digest':digest,
    'exclusions':exclusions,'source_files':{path:sha(data) if data is not None else None for path,data in current.items()},
    'deleted_files':[path for path,data in current.items() if data is None],
    'preserved_external_changes':external,'p04_changed_files':[path for path in changed if path not in external],
    'preserved_prerequisite_manifest':str(previous),'previous_digest':base['digest'],
    'source_archive':archive_path.name,'source_archive_sha256':sha(archive_path.read_bytes()),
    'p04_delta':identity+'-p04.diff','architecture':'CHS-P04-D1/r2','architecture_pass':'CHS-P04-AG1-R2',
    'coordination_revision':11,'coordination_pin':pinned.name,'coordination_sha256':sha(coord.read_bytes()),
    'notice_revisions':['I001-I003/r1','I004/r2','I005-I006/r1','C001/r1','C002/r1','C003/r2','C004/r2'],
    'checks':'current-affected-summary-02.json and its immutable referenced check cohorts','full_suite':False,
}
if (logs/'CHS-P04-B1-R1-manifest.json').exists():
    first=json.loads((logs/'CHS-P04-B1-R1-manifest.json').read_text())
    with tarfile.open(logs/first['source_archive']) as archive:
        first_source={member.name:archive.extractfile(member).read() for member in archive.getmembers() if member.isfile()}
    correction=[];correction_files=[]
    for path in sorted(set(paths)|set(first['source_files'])):
        old=None if first['source_files'].get(path,'new') is None else first_source.get(path,original(path))
        new=(root/path).read_bytes() if (root/path).exists() else None
        if old!=new:
            correction_files.append(path)
            correction.extend(difflib.unified_diff((old or b'').decode().splitlines(keepends=True),(new or b'').decode().splitlines(keepends=True),fromfile='a/'+path,tofile='b/'+path))
    name='CHS-P04-B1-R1-to-'+identity.rsplit('-',1)[-1]+'.diff'
    (logs/name).write_text(''.join(correction))
    manifest.update(previous_review_digest=first['digest'],correction_files=correction_files,correction_delta=name)
(logs/(identity+'-manifest.json')).write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'identity':identity,'digest':digest,'source_files':len(paths),'phase_files':len(manifest['p04_changed_files']),'deleted_files':manifest['deleted_files']}))
