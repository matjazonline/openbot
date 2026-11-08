import json,os,pathlib,subprocess,sys,time
root=pathlib.Path(__file__).resolve().parents[7];logs=pathlib.Path(__file__).parent
env=os.environ.copy();env.update(SQLX_OFFLINE='true',CARGO_BUILD_JOBS='4',CARGO_TARGET_DIR='/Users/mac02/.codex/worktrees/1133/openbot/target',PYTHONDONTWRITEBYTECODE='1',PATH='/tmp/vault-google-tools:'+env['PATH'])
for key in ('ALLOW_MISSING_DATABASE_URL','DATABASE_URL','TEST_DATABASE_URL'):env.pop(key,None)
label=sys.argv[1];cmd=sys.argv[2:];start=time.time()
with (logs/(label+'.log')).open('w') as out:r=subprocess.run(cmd,cwd=root,env=env,stdout=out,stderr=subprocess.STDOUT)
d={'command':cmd,'cwd':str(root),'exit':r.returncode,'seconds':round(time.time()-start,2),'fixture':'no database required; owned cache serialized','log':str(logs/(label+'.log'))};(logs/(label+'.json')).write_text(json.dumps(d,indent=2)+'\n');print(json.dumps(d));sys.exit(r.returncode)
