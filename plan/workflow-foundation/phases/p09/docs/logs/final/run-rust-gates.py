"""Run the final Rust/database gates against a phase-owned, uniquely named test database."""
from pathlib import Path
import json, os, subprocess, time, uuid
root=Path.cwd(); logs=root/'plan/workflow-foundation/phases/p09/docs/logs/final'
env=dict(os.environ,SQLX_OFFLINE='true',CARGO_INCREMENTAL='0',DATABASE_URL='postgres://workflow09@127.0.0.1:55490/workflow09')
database='workflow09_final_'+uuid.uuid4().hex[:10]
env['TEST_DATABASE_URL']='postgres://workflow09@127.0.0.1:55490/'+database
psql='/opt/homebrew/opt/postgresql@18/bin/psql'
subprocess.run([psql,env['DATABASE_URL'],'-X','-v','ON_ERROR_STOP=1','-c',f'CREATE DATABASE "{database}"'],check=True)
(logs/'database.json').write_text(json.dumps({'database':database,'url':env['TEST_DATABASE_URL']},indent=2)+'\n')
checks=[
 ('offline-compilation',['cargo','check','--locked','--offline','--all-targets']),
 ('clippy',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('compile-tests',['cargo','test','--locked','--offline','--all-targets','--no-run']),
 ('database-suite',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--all-targets']),
 ('stack-budget',['scripts/test-network-isolation.sh','scripts/stack-budget.sh','--offline']),
 ('lease-budget',['scripts/test-network-isolation.sh','scripts/lease-budget.sh','--offline']),
]
results=[]
for name,command in checks:
 start=time.monotonic();print('START',name,flush=True)
 with (logs/(name+'.log')).open('w') as output:
  process=subprocess.run(command,env=env,stdout=output,stderr=subprocess.STDOUT)
 results.append({'check':name,'command':command,'exit_code':process.returncode,'seconds':round(time.monotonic()-start,2)})
 (logs/'rust-results.json').write_text(json.dumps(results,indent=2)+'\n')
 print('DONE',name,process.returncode,flush=True)
 if process.returncode:
  print('STOP: investigate before broader checks',flush=True);break
# Retain this one owned database for migration inspection and any focused failure recheck.
raise SystemExit(any(result['exit_code'] for result in results))
