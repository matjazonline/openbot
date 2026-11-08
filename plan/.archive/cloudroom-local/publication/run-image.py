#!/usr/bin/env python3
from pathlib import Path
import hashlib, json, os, shutil, subprocess, tempfile, time
ROOT=Path(__file__).resolve().parents[4]; RECORDS=Path(__file__).resolve().parent; LOGS=RECORDS/'logs'
resource=Path(tempfile.mkdtemp(prefix='obpub-',dir='/tmp')).resolve(); context=resource/'context'; context.mkdir()
fixed={'Cargo.toml','Cargo.lock','Dockerfile','.dockerignore','scripts/prepare-knowledge-model.py','scripts/check-knowledge-image.py','fly.toml'}
paths=subprocess.check_output(['git','ls-files','-z'],cwd=ROOT).decode().split('\0'); inputs=[]
for name in paths:
 if name in fixed or name.startswith(('.sqlx/','migrations/','assets/','src/')):
  p=ROOT/name
  if p.is_file():
   dest=context/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest);inputs.append({'path':name,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()})
env=os.environ.copy();env['LIMA_HOME']=str(resource/'i')
image='openbot-cloudroom-publication:verified'
info={'owner':'root/cloudroom-publication','directory':str(resource),'limaHome':env['LIMA_HOME'],'name':'img','image':image,'context':str(context),'cpus':2,'memoryGiB':6,'mountWritable':False,'nativeLinuxProviderAcceptance':'NOT EXERCISED/user waived'}
(RECORDS/'image-resource.json').write_text(json.dumps(info,indent=2)+'\n');(RECORDS/'image-inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
commands=[('image-start',['limactl','start','--tty=false','--name=img','--vm-type=vz','--arch=aarch64','--cpus=2','--memory=6','--disk=12','--mount-only='+str(context),'--set','.mounts[0].writable=false | .ssh.forwardAgent=false | .ssh.loadDotSSHPubKeys=false','--set','.images = [{"location": "https://cloud-images.ubuntu.com/releases/resolute/release-20260927/ubuntu-26.04-server-cloudimg-arm64.img", "arch": "aarch64", "digest": "sha256:63a93bd5a8d76e33b15ceb5daa3657bd79be804748051ab178e643b0f5da22e7"}]','--timeout=10m','template:docker']),('image-build',['limactl','shell','img','--','docker','build','--progress=plain','--tag',image,str(context)]),('image-check',['limactl','shell','img','--','python3',str(context/'scripts/check-knowledge-image.py'),'--image',image])]
results=[]
for name,command in commands:
 print('START '+name,flush=True);start=time.monotonic()
 with (LOGS/(name+'.log')).open('wb') as output:r=subprocess.run(command,cwd=ROOT,env=env,stdout=output,stderr=subprocess.STDOUT)
 entry={'name':name,'command':command,'status':'PASS' if r.returncode==0 else 'FAIL','exit':r.returncode,'seconds':round(time.monotonic()-start,2),'log':'logs/'+name+'.log'};results.append(entry);(RECORDS/'image-checks.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(entry),flush=True)
 if r.returncode:
  print((LOGS/(name+'.log')).read_text(errors='replace')[-3000:],flush=True);break
