#!/usr/bin/env python3
from pathlib import Path
import json,os,subprocess,time
ROOT=Path(__file__).resolve().parents[4]; RECORDS=Path(__file__).resolve().parent; LOGS=RECORDS/'logs'
env=os.environ|{'PYTHONDONTWRITEBYTECODE':'1'}
checks=[('deploy-tests',['scripts/tests/deploy.sh']),('rotation-tests',['scripts/tests/credential-key-rotation.sh']),('model-prepare-tests',['python3','scripts/tests/knowledge-model-prepare.py']),('transport-tests',['scripts/tests/transport-boundary.sh']),('transport-boundary',['scripts/transport-boundary-check.sh']),('sample-boundary-tests',['python3','scripts/tests/workflow-sample-boundary.py']),('sample-boundary',['python3','scripts/workflow-sample-boundary.py']),('integration-boundary-tests',['python3','scripts/tests/integration-boundary.py']),('integration-boundary',['python3','scripts/integration-boundary.py']),('integration-docs',['python3','scripts/check-integration-docs.py']),('browser-protocols',['node','--test','scripts/tests/sse-navigation.mjs','scripts/tests/agent-runtime.mjs','scripts/tests/channel-response-authoring.mjs','scripts/tests/workflow-runs.mjs','scripts/tests/workflow-conversations.mjs','scripts/tests/workflow-samples.mjs']),('run-counts',['npm','run','test:run-counts'])]
results=[]
for name,command in checks:
 start=time.monotonic()
 with (LOGS/(name+'.log')).open('wb') as output:r=subprocess.run(command,cwd=ROOT,env=env,stdout=output,stderr=subprocess.STDOUT)
 item={'name':name,'command':command,'status':'PASS' if r.returncode==0 else 'FAIL','exit':r.returncode,'seconds':round(time.monotonic()-start,2),'log':'logs/'+name+'.log'};results.append(item);print(json.dumps(item),flush=True);(RECORDS/'static-checks.json').write_text(json.dumps(results,indent=2)+'\n')
