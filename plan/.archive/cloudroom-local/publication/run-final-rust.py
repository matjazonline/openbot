#!/usr/bin/env python3
from pathlib import Path
import json,os,subprocess,time
ROOT=Path(__file__).resolve().parents[4];RECORDS=Path(__file__).resolve().parent;LOGS=RECORDS/'logs'
env=os.environ|{'CARGO_TARGET_DIR':'/Users/mac03/dev/openbot/target','TEST_DATABASE_URL':'postgres://mac03@127.0.0.1:56390/cloudroom_publication_test','DATABASE_URL':'postgres://mac03@127.0.0.1:56390/cloudroom_publication_test','SQLX_OFFLINE':'true','PYTHONDONTWRITEBYTECODE':'1'}
checks=[('full-suite',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--all-targets']),('stack-budget',['scripts/test-network-isolation.sh','scripts/stack-budget.sh','--offline']),('lease-budget',['scripts/test-network-isolation.sh','scripts/lease-budget.sh','--offline'])]
results=[]
for name,command in checks:
 print('START '+name,flush=True);start=time.monotonic()
 with (LOGS/(name+'.log')).open('wb') as output:r=subprocess.run(command,cwd=ROOT,env=env,stdout=output,stderr=subprocess.STDOUT)
 item={'name':name,'command':command,'fixture':'owned UTF8 PostgreSQL18.6 port56390','status':'PASS' if r.returncode==0 else 'FAIL','exit':r.returncode,'seconds':round(time.monotonic()-start,2),'log':'logs/'+name+'.log'};results.append(item);(RECORDS/'final-rust-checks.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(item),flush=True)
