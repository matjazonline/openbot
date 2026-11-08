import json, os, pathlib, subprocess, sys, time
root=pathlib.Path(__file__).resolve().parents[6]
logs=pathlib.Path(__file__).parent
resource=json.loads((logs/'resources.json').read_text())
env=os.environ.copy();env.update(DATABASE_URL=resource['database_url'],TEST_DATABASE_URL=resource['test_database_url'],SQLX_OFFLINE='true',CARGO_BUILD_JOBS='4',CARGO_TARGET_DIR=resource['cargo_target'],PATH='/tmp/vault-google-tools:'+env['PATH'])
env.pop('ALLOW_MISSING_DATABASE_URL',None)
label=sys.argv[1]; command=sys.argv[2:];start=time.time()
with (logs/(label+'.log')).open('w') as out:
 result=subprocess.run(command,cwd=root,env=env,stdout=out,stderr=subprocess.STDOUT)
record={'command':command,'cwd':str(root),'database':resource['database'],'exit':result.returncode,'seconds':round(time.time()-start,2),'log':str(logs/(label+'.log'))}
(logs/(label+'.json')).write_text(json.dumps(record,indent=2)+'\n');print(json.dumps(record));sys.exit(result.returncode)
