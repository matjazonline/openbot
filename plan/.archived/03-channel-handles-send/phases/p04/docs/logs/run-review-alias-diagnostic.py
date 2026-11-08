#!/usr/bin/env python3
import datetime,difflib,hashlib,json,os,subprocess,tarfile,time
from pathlib import Path
root=Path.cwd(); logs=Path(__file__).resolve().parent
m=json.loads((logs/'CHS-P04-B1-R1-manifest.json').read_text());resource=json.loads((logs/'review-part-resources.json').read_text());worktree=Path(resource['worktree'])
name='src/adapters/persistence/workflow/channel_reply_publication_integrity_tests.rs'
with tarfile.open(logs/m['source_archive']) as archive: before=archive.extractfile(name).read().decode()
first='\n'+(logs/'CHS-P04-B1-R1-part-reparent-diagnostic.rs').read_text()
file=worktree/name
assert file.read_text()==before+first,'Only exact prior diagnostic may differ'
for path,want in m['source_files'].items():
 if path==name: continue
 f=worktree/path; actual=hashlib.sha256(f.read_bytes()).hexdigest() if f.exists() else None
 assert actual==want,path
addition='\n'+(logs/'CHS-P04-B1-R1-delivery-alias-diagnostic.rs').read_text()
file.write_text(before+first+addition)
(logs/'review-alias-diagnostic.diff').write_text(''.join(difflib.unified_diff(before.splitlines(True),(before+first+addition).splitlines(True),fromfile='a/'+name,tofile='b/'+name)))
env=os.environ.copy();env.pop('ALLOW_MISSING_DATABASE_URL',None)
settings={'DATABASE_URL':'postgres://mac02@127.0.0.1:65037/openbot_chs','TEST_DATABASE_URL':'postgres://mac02@127.0.0.1:65037/openbot_chs_test','SQLX_OFFLINE':'true','CARGO_INCREMENTAL':'1','CARGO_BUILD_JOBS':'4','RUST_MIN_STACK':'2097152','CARGO_TARGET_DIR':str(root/'target')};env.update(settings)
command=['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','reviewer_final_delivery_alias_must_not_bypass_revocation','--','--nocapture']
start=datetime.datetime.now(datetime.timezone.utc).isoformat();clock=time.monotonic()
with (logs/'review-alias-tests-01.log').open('w') as stream:
 result=subprocess.run(command,cwd=worktree,env=env,stdout=stream,stderr=subprocess.STDOUT)
receipt={**resource,'diagnostic_patch':'review-alias-diagnostic.diff','command':command,'environment':settings,'started':start,'seconds':round(time.monotonic()-clock,3),'exit_code':result.returncode,'log':'review-alias-tests-01.log'}
(logs/'review-alias-tests-01.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt));print('\n'.join((logs/'review-alias-tests-01.log').read_text().splitlines()[-35:]))
