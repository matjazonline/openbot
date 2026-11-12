from pathlib import Path
import importlib.util,json,os,subprocess
logs=Path(__file__).resolve().parent
run=Path((logs/'B1-R3-resource-root.txt').read_text().strip())
node=json.loads((logs/'B1-R3-node22.txt').read_text().splitlines()[-1])
spec=importlib.util.spec_from_file_location('doop_gate','scripts/doop-test.py')
runner=importlib.util.module_from_spec(spec);spec.loader.exec_module(runner)
setup=run/'doop';source,manifest,identity=runner.select_source(setup,None,None,True)
home=run/'doop-service-gate-home';(home/'tmp').mkdir(mode=0o700,parents=True)
environment=runner.clean_environment(home)
environment['PATH']=str(setup/'tools/node_modules/.bin')+os.pathsep+str(Path(node['executable']).parent)+os.pathsep+environment['PATH']
environment['CHROME_PATH']='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
environment['DOOP_TEST_DATABASE_URL']='postgres://postgres:p05fixture@127.0.0.1:55435/openbot_p05_doop'
# Readiness is bounded and scoped to this phase-owned container.
docker_env=os.environ.copy();docker_env.pop('DOCKER_CONTEXT',None);docker_env.update(json.loads((logs/'B1-R3-docker-environment.json').read_text()))
subprocess.run(['docker','exec','openbot-p05-r3-doop-pg','pg_isready','-U','postgres','-d','openbot_p05_doop'],env=docker_env,check=True)
bun=str(setup/'tools/node_modules/.bin/bun')
commands=[('install',[bun,'install','--frozen-lockfile','--ignore-scripts'])]
commands += [(name,[bun,'run',name]+(['--','--maxWorkers=2'] if name=='test' else [])) for name in ['format:check','typecheck','lint','doc-lint','test','build']]
results=[]
for name,command in commands:
    output=logs/('B1-R3-doop-service-'+name.replace(':','-')+'.txt')
    with output.open('x') as stream:
        print('Command: '+' '.join(command),file=stream,flush=True)
        result=subprocess.run(command,cwd=source,env=environment,stdout=stream,stderr=subprocess.STDOUT,timeout=1200)
    item=dict(name=name,command=command,status='PASS' if result.returncode==0 else 'FAIL',exit_code=result.returncode,log=output.name)
    results.append(item);(logs/'B1-R3-doop-service-results.json').write_text(json.dumps(results,indent=2)+'\n')
    print(name+': '+item['status'],flush=True)
runner.reviewed_source(source,identity['fingerprint'],manifest)
print('Managed source fingerprint unchanged:',identity['fingerprint'],flush=True)
raise SystemExit(int(any(item['status']!='PASS' for item in results)))
