#!/usr/bin/env python3
import datetime,difflib,hashlib,json,os,subprocess,tarfile,tempfile,time
from pathlib import Path
root=Path.cwd(); logs=Path(__file__).resolve().parent
m=json.loads((logs/'CHS-P04-B1-R1-manifest.json').read_text())
container=Path(tempfile.mkdtemp(prefix='openbot-chs-p04-review-part-')); worktree=container/'worktree'
setup=subprocess.run(['git','worktree','add','--detach',str(worktree),m['head']],cwd=root,text=True,capture_output=True,check=True)
(logs/'review-part-setup.log').write_text(setup.stdout+setup.stderr)
with tarfile.open(logs/m['source_archive']) as archive:
 assert all(member.isfile() and not Path(member.name).is_absolute() and '..' not in Path(member.name).parts for member in archive.getmembers())
 archive.extractall(worktree)
for name in m['deleted_files']:
 f=worktree/name
 if f.exists(): f.unlink()
for name,want in m['source_files'].items():
 f=worktree/name; actual=hashlib.sha256(f.read_bytes()).hexdigest() if f.exists() else None
 assert actual==want,name
env=os.environ.copy();env.pop('ALLOW_MISSING_DATABASE_URL',None)
env['PATH']=str(root/'plan/03-channel-handles-send/phases/p01/docs/logs/tools')+os.pathsep+env['PATH']
snapshot=subprocess.check_output(['bash',str(root/'.agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh'),*m['exclusions']],cwd=worktree,env=env,text=True)
assert 'digest: '+m['digest'] in snapshot
(logs/'review-part-before-diagnostic-snapshot.txt').write_text(snapshot)
file=worktree/'src/adapters/persistence/workflow/channel_reply_publication_integrity_tests.rs';before=file.read_text()
addition='\n'+(logs/'CHS-P04-B1-R1-part-reparent-diagnostic.rs').read_text()
file.write_text(before+addition)
(logs/'review-part-diagnostic.diff').write_text(''.join(difflib.unified_diff(before.splitlines(True),(before+addition).splitlines(True),fromfile='a/'+str(file.relative_to(worktree)),tofile='b/'+str(file.relative_to(worktree)))))
settings={'DATABASE_URL':'postgres://mac02@127.0.0.1:65037/openbot_chs','TEST_DATABASE_URL':'postgres://mac02@127.0.0.1:65037/openbot_chs_test','SQLX_OFFLINE':'true','CARGO_INCREMENTAL':'1','CARGO_BUILD_JOBS':'4','RUST_MIN_STACK':'2097152','CARGO_TARGET_DIR':str(root/'target')};env.update(settings)
command=['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','reviewer_final_part_reparent_must_be_rejected','--','--nocapture']
resource={'worktree':str(worktree),'container':str(container),'preserve_until_review_complete':True,'source_digest':m['digest'],'diagnostic_patch':'review-part-diagnostic.diff','database_pair':'openbot_chs/openbot_chs_test','target':str(root/'target')}
(logs/'review-part-resources.json').write_text(json.dumps(resource,indent=2)+'\n')
start=datetime.datetime.now(datetime.timezone.utc).isoformat();clock=time.monotonic()
with (logs/'review-part-tests-01.log').open('w') as stream:
 result=subprocess.run(command,cwd=worktree,env=env,stdout=stream,stderr=subprocess.STDOUT)
receipt={**resource,'command':command,'environment':settings,'started':start,'seconds':round(time.monotonic()-clock,3),'exit_code':result.returncode,'log':'review-part-tests-01.log'}
(logs/'review-part-tests-01.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt));print('\n'.join((logs/'review-part-tests-01.log').read_text().splitlines()[-35:]))
