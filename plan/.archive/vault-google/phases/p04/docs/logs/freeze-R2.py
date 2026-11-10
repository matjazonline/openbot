import difflib,hashlib,json,os,pathlib,subprocess,tarfile
root=pathlib.Path(__file__).resolve().parents[6];logs=pathlib.Path(__file__).parent
old_manifest=json.loads((logs/'manifest-R1.json').read_text());excludes=old_manifest['excludes']
env=os.environ.copy();env['PATH']='/tmp/vault-google-tools:'+env['PATH']
assert not (logs/'manifest-R2.json').exists()
snapshot=subprocess.check_output(['bash',str(root/'.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh'),*excludes],cwd=root,env=env,text=True)
(logs/'snapshot-R2.txt').write_text(snapshot)
files=[line.strip() for line in snapshot.split('files:\n',1)[1].splitlines() if line.strip()]
manifest={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'digest':snapshot.split('digest: ',1)[1].splitlines()[0],'excludes':excludes,'source_files':{f:hashlib.sha256((root/f).read_bytes()).hexdigest() for f in files},'correction_base_digest':old_manifest['digest']}
(logs/'manifest-R2.json').write_text(json.dumps(manifest,indent=2)+'\n')
with tarfile.open(logs/'source-R2.tar.gz','w:gz') as archive:
 for f in files:archive.add(root/f,arcname=f)
(logs/'coordination-R2-r6.md').write_text((root/'plan/vault-google/COORDINATION.md').read_text())
with tarfile.open(logs/'source-R1.tar.gz') as archive:
 prior={name:archive.extractfile(name).read() for name in old_manifest['source_files']}
changes=[];diff=[]
for name in sorted(set(prior)|set(manifest['source_files'])):
 old=prior.get(name)
 if old is None:
  p=subprocess.run(['git','show',manifest['head']+':'+name],cwd=root,capture_output=True);old=p.stdout if p.returncode==0 else b''
 new=(root/name).read_bytes() if (root/name).exists() else b''
 if old!=new:
  changes.append(name)
  diff.extend(difflib.unified_diff(old.decode().splitlines(True),new.decode().splitlines(True),fromfile='R1/'+name,tofile='R2/'+name))
(logs/'correction-R2.diff').write_text(''.join(diff))
(logs/'delta-R2.json').write_text(json.dumps({'baseline_digest':old_manifest['digest'],'current_digest':manifest['digest'],'changed_files':changes},indent=2)+'\n')
print(snapshot.split('files:',1)[0]);print('correction files',len(changes))
