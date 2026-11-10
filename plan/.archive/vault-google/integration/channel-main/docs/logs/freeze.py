import difflib,hashlib,json,os,pathlib,subprocess,tarfile
root=pathlib.Path(__file__).resolve().parents[6];logs=pathlib.Path(__file__).parent;r=json.loads((logs/'resources.json').read_text())
env=os.environ.copy();env['PATH']='/tmp/vault-google-tools:'+env['PATH']
snapshot=subprocess.check_output(['bash',str(root/'.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh'),*r['excludes']],cwd=root,env=env,text=True)
assert not (logs/'manifest-R1.json').exists()
files=[line.strip() for line in snapshot.split('files:\n',1)[1].splitlines() if line.strip()]
m={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'digest':snapshot.split('digest: ',1)[1].splitlines()[0],'excludes':r['excludes'],'source_files':{f:hashlib.sha256((root/f).read_bytes()).hexdigest() for f in files}}
assert m['head']==r['pulled_main']
(logs/'snapshot-R1.txt').write_text(snapshot);(logs/'manifest-R1.json').write_text(json.dumps(m,indent=2)+'\n')
with tarfile.open(logs/'source-R1.tar.gz','w:gz') as a:
 for f in files:a.add(root/f,arcname=f)
(logs/'coordination-R1-r12.md').write_text((root/'plan/vault-google/COORDINATION.md').read_text())
old=json.loads((logs/'vault-baseline-manifest.json').read_text())
with tarfile.open(root/'plan/vault-google/phases/p05/docs/logs/source-R2.tar.gz') as a:prior={f:a.extractfile(f).read() for f in old['source_files']}
changed=[]
with (logs/'vault-05-to-current.diff').open('w') as out:
 for f in sorted(set(prior)|set(m['source_files'])):
  before=prior.get(f,b'');after=(root/f).read_bytes() if (root/f).is_file() else b''
  if before!=after:
   changed.append(f);out.writelines(difflib.unified_diff(before.decode().splitlines(True),after.decode().splitlines(True),fromfile='accepted-vault05/'+f,tofile='combined-main/'+f))
upstream=subprocess.check_output(['git','diff','--name-only',r['pre_pull_head'],r['pulled_main']],cwd=root,text=True).splitlines()
(logs/'integration-delta-R1.json').write_text(json.dumps({'prior_head':r['pre_pull_head'],'pulled_head':r['pulled_main'],'current_digest':m['digest'],'vault_source_changed_paths':changed,'upstream_committed_changed_paths':upstream,'scope':'Review preserved vault overlay plus its interoperability with committed main. This five-path supplemental diff is not the entire upstream commit; reuse accepted channel03 evidence and inspect affected new callers.'},indent=2)+'\n')
print(json.dumps({'head':m['head'],'digest':m['digest'],'vault_changed_paths':changed,'source_files':len(files)}))
