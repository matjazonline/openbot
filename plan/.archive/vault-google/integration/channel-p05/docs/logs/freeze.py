import hashlib,json,os,pathlib,subprocess,tarfile
root=pathlib.Path(__file__).resolve().parents[6];logs=pathlib.Path(__file__).parent
excludes=['plan/vault-google/COORDINATION.md']+['plan/vault-google/phases/p%02d/docs'%n for n in range(1,6)]+['plan/vault-google/integration/channel-p05/docs']
env=os.environ.copy();env['PATH']='/tmp/vault-google-tools:'+env['PATH']
snapshot=subprocess.check_output(['bash',str(root/'.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh'),*excludes],cwd=root,env=env,text=True)
(logs/'snapshot-R1.txt').write_text(snapshot)
files=[line.strip() for line in snapshot.split('files:\n',1)[1].splitlines() if line.strip()]
manifest={'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'digest':snapshot.split('digest: ',1)[1].splitlines()[0],'excludes':excludes,'source_files':{f:hashlib.sha256((root/f).read_bytes()).hexdigest() for f in files}}
(logs/'manifest-R1.json').write_text(json.dumps(manifest,indent=2)+'\n')
with tarfile.open(logs/'source-R1.tar.gz','w:gz') as archive:
 for f in files: archive.add(root/f,arcname=f)
(logs/'coordination-r3.md').write_text((root/'plan/vault-google/COORDINATION.md').read_text())
print(snapshot.split('files:',1)[0])
