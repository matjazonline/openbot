from pathlib import Path
import os,subprocess,json
p=Path('plan/fix-app/phases/p08/docs/logs')
env=dict(os.environ,CARGO_INCREMENTAL='0',SQLX_OFFLINE='true',DATABASE_URL='postgres://mac02@127.0.0.1:55408/open_bots',TEST_DATABASE_URL='postgres://mac02@127.0.0.1:55408/open_bots_test')
commands={
 'clippy':['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'],
 'main':['cargo','test','--locked','--offline','--bin','open_bots'],
 'fmt':['cargo','fmt','--','--check'],
 'whitespace':['git','diff','--check','77edd70862a88e2189587fda50fb2b0b913082b6'],
 'resources':['python3',str(p/'verify-step6-resources.py')],
}
results={}
for name,command in commands.items():
 with (p/f'final-author-{name}.log').open('w') as f:r=subprocess.run(command,env=env,stdout=f,stderr=subprocess.STDOUT)
 results[name]=dict(command=command,exit_code=r.returncode);(p/'final-author-checks.json').write_text(json.dumps(results,indent=2)+'\n');print(name,r.returncode,flush=True)
assert all(v['exit_code']==0 for v in results.values()),results
