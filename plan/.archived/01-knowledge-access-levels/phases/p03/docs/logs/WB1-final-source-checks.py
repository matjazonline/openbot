from pathlib import Path
import os,json,subprocess,time
p=Path('plan/01-knowledge-access-levels/phases/p03/docs/logs')
env=dict(os.environ,CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55503/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55503/open_bots_test')
cmd=['cargo','test','--locked','--offline','--lib','--no-run','--message-format=json']
with (p/'WB1-final-source-native-build.log').open('w') as f:r=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT)
assert r.returncode==0
artifacts=[]
for line in (p/'WB1-final-source-native-build.log').read_text().splitlines():
 try:j=json.loads(line)
 except ValueError:continue
 if j.get('reason')=='compiler-artifact' and j.get('executable') and j.get('target',{}).get('kind')==['lib']:artifacts.append(j)
assert len(artifacts)==1
exe=artifacts[0]['executable'];results=[{'name':'build','command':cmd,'exit_code':r.returncode,'artifact_fresh':artifacts[0]['fresh']}]
commands=[('route-tests',[exe,'adapters::http::routes::connection_levels::tests']),('clippy',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),('fmt',['cargo','fmt','--','--check']),('diff',['git','diff','--check'])]
for name,cmd in commands:
 start=time.monotonic()
 with (p/f'WB1-final-source-{name}.log').open('w') as f:r=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT)
 results.append({'name':name,'command':cmd,'exit_code':r.returncode,'elapsed_seconds':round(time.monotonic()-start,2)})
 (p/'WB1-final-source-result.json').write_text(json.dumps(results,indent=2)+'\n');print(name,r.returncode,flush=True)
raise SystemExit(0 if all(r['exit_code']==0 for r in results) else 1)
