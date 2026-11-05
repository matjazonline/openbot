from pathlib import Path
import os,subprocess,json,time
p=Path('plan/01-knowledge-access-levels/phases/p03/docs/logs')
base=dict(os.environ,CARGO_INCREMENTAL='0',DATABASE_URL='postgres://mac02@127.0.0.1:55503/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55503/open_bots_test')
commands=[('fmt',['cargo','fmt','--','--check'],'true'),('diff',['git','diff','--check'],'true'),('sqlx-check',['cargo','sqlx','prepare','--check','--','--all-targets'],'false'),('offline',['cargo','check','--locked','--offline','--all-targets'],'true'),('clippy',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'],'true')]
results=[]
for name,cmd,offline in commands:
 start=time.monotonic()
 with (p/f'WB1-{name}.log').open('w') as f:result=subprocess.run(cmd,env=dict(base,SQLX_OFFLINE=offline),stdout=f,stderr=subprocess.STDOUT)
 results.append({'name':name,'command':cmd,'sqlx_offline':offline,'exit_code':result.returncode,'elapsed_seconds':round(time.monotonic()-start,2)})
 (p/'WB1-compiler-checks-result.json').write_text(json.dumps(results,indent=2)+'\n');print(name,result.returncode,flush=True)
raise SystemExit(0 if all(r['exit_code']==0 for r in results) else 1)
