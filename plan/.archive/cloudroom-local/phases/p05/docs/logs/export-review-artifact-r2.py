from pathlib import Path
import hashlib, json, subprocess, tempfile
root=Path.cwd()
records=root/'plan/cloudroom-local/phases/p05/docs'
previous=root/'plan/cloudroom-local/phases/p04/docs/logs'
identity=json.loads((previous/'p04-r2-source-identity.json').read_text())
previous_patch=previous/identity['patch']
assert hashlib.sha256(previous_patch.read_bytes()).hexdigest()==identity['patchSha256']
excludes=identity['exclusions']
spec=['.']+[f':(top,exclude,literal){path}' for path in excludes]
changed=subprocess.check_output(['git','diff','HEAD','--name-only','-z','--',*spec]).split(b'\0')
untracked=subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z','--',*spec]).split(b'\0')
files=sorted({p.decode() for p in changed+untracked if p})
full=records/'logs/p05-r2-source.patch'
with full.open('wb') as stream:
 subprocess.run(['git','diff','HEAD','--binary','--',*spec],stdout=stream,check=True)
 for raw in sorted(p for p in untracked if p):
  result=subprocess.run(['git','diff','--no-index','--binary','--','/dev/null',raw.decode()],stdout=stream)
  assert result.returncode in (0,1)
base=Path(tempfile.mkdtemp(prefix='openbot-cloudroom-p05-review-base-'))
archive=subprocess.Popen(['git','archive','HEAD'],stdout=subprocess.PIPE)
subprocess.run(['tar','-x','-C',str(base)],stdin=archive.stdout,check=True)
archive.stdout.close()
assert archive.wait()==0
subprocess.run(['git','apply',str(previous_patch)],cwd=base,check=True)
delta=records/'logs/p05-r2-delta.patch'
with delta.open('wb') as stream:
 for path in files:
  old=base/path
  current=root/path
  result=subprocess.run(['git','diff','--no-index','--binary','--',str(old) if old.exists() else '/dev/null',str(current) if current.exists() else '/dev/null'],stdout=subprocess.PIPE)
  assert result.returncode in (0,1)
  content=result.stdout.replace(f'a{base}/'.encode(),b'a/').replace(f'b{root}/'.encode(),b'b/')
  stream.write(content)
result={'version':1,'reviewBaseCommit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
 'prerequisite':identity['artifact'],'prerequisitePatchSha256':identity['patchSha256'],'prerequisiteTree':str(base),
 'exclusions':excludes,'fullPatch':full.name,'fullPatchSha256':hashlib.sha256(full.read_bytes()).hexdigest(),
 'deltaPatch':delta.name,'deltaPatchSha256':hashlib.sha256(delta.read_bytes()).hexdigest()}
(records/'logs/p05-r2-source-identity.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
