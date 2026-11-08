import hashlib,json,os,pathlib,subprocess,time,uuid
root=pathlib.Path(__file__).resolve().parents[6];logs=pathlib.Path(__file__).parent
receipt=logs/'baseline-overflow-01.json';resource=json.loads((logs/'resources.json').read_text())
assert not receipt.exists()
while not (logs/'final-full-01.json').exists():time.sleep(1)
base='97e5d60dc538e26c7cb976a8d63772eed80baff2'
parent=pathlib.Path('/private/tmp')/('vault-vf2-baseline-'+uuid.uuid4().hex[:12]);worktree=parent/'worktree'
database='vault_vf2_1133_'+uuid.uuid4().hex[:12];url='postgresql://localhost/'+database
r={'baseline':base,'worktree':str(worktree),'database':database,'database_url':url,'cargo_target':resource['cargo_target'],'pid':os.getpid(),'ownership':'created exclusively by this phase for one baseline reproduction','command':['bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','workflow_control_revision_overflow_refuses_without_partial_writes','--','--nocapture']}
(logs/'baseline-resource-01.json').write_text(json.dumps(r,indent=2)+'\n')
started=time.time();created=False;checked_out=False
try:
 parent.mkdir()
 with (logs/'baseline-setup-01.log').open('w') as out:
  subprocess.run(['git','worktree','add','--detach',str(worktree),base],cwd=root,check=True,stdout=out,stderr=subprocess.STDOUT);checked_out=True
  subprocess.run(['psql','postgresql://localhost/postgres','-X','-v','ON_ERROR_STOP=1','-c',f'CREATE DATABASE "{database}"'],check=True,stdout=out,stderr=subprocess.STDOUT);created=True
 r['actual_head']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=worktree,text=True).strip();assert r['actual_head']==base
 r['baseline_status_before']=subprocess.check_output(['git','status','--porcelain'],cwd=worktree,text=True);assert not r['baseline_status_before']
 rel='src/adapters/persistence/workflow/control_revision_tests.rs'
 r['test_sha256']=hashlib.sha256((worktree/rel).read_bytes()).hexdigest();assert (worktree/rel).read_bytes()==(root/rel).read_bytes()
 env=os.environ.copy();env.update(DATABASE_URL=url,TEST_DATABASE_URL=url,SQLX_OFFLINE='true',CARGO_TARGET_DIR=resource['cargo_target'],CARGO_BUILD_JOBS='4',RUST_MIN_STACK='2097152',RUST_TEST_THREADS='1')
 env.pop('ALLOW_MISSING_DATABASE_URL',None)
 with (logs/'baseline-overflow-01.log').open('w') as out:
  result=subprocess.run(r['command'],cwd=worktree,env=env,stdout=out,stderr=subprocess.STDOUT)
 r['exit']=result.returncode;output=(logs/'baseline-overflow-01.log').read_text()
 r['same_failure_reproduced']=result.returncode!=0 and 'running 1 test' in output and 'pending trigger events' in output and '55006' in output and 'control_revision_tests.rs:85' in output
 r['baseline_status_after']=subprocess.check_output(['git','status','--porcelain'],cwd=worktree,text=True)
except Exception as error:
 r['setup_error']=str(error);r['same_failure_reproduced']=False
finally:
 with (logs/'baseline-cleanup-01.log').open('w') as out:
  if created:
   drop=subprocess.run(['psql','postgresql://localhost/postgres','-X','-v','ON_ERROR_STOP=1','-c',f'DROP DATABASE "{database}" WITH (FORCE)'],stdout=out,stderr=subprocess.STDOUT);r['database_cleanup_exit']=drop.returncode
  if checked_out:
   clean=subprocess.check_output(['git','status','--porcelain'],cwd=worktree,text=True)
   if not clean:
    remove=subprocess.run(['git','worktree','remove',str(worktree)],cwd=root,stdout=out,stderr=subprocess.STDOUT);r['worktree_cleanup_exit']=remove.returncode
   else:r['worktree_preserved_dirty']=clean
 r['seconds']=round(time.time()-started,2);receipt.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r),flush=True)
