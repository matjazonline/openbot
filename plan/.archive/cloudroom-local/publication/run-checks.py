#!/usr/bin/env python3
"""Retain publication verification output; never reuse mutable phase evidence."""
from pathlib import Path
import json, os, subprocess, time
ROOT = Path(__file__).resolve().parents[4]
RECORDS = Path(__file__).resolve().parent
LOGS = RECORDS / 'logs'
LOGS.mkdir(exist_ok=True)
env = os.environ.copy()
env.update(CARGO_TARGET_DIR='/Users/mac03/dev/openbot/target', DATABASE_URL='postgres://mac03@127.0.0.1:56389/cloudroom_publication_test', TEST_DATABASE_URL='postgres://mac03@127.0.0.1:56389/cloudroom_publication_test', SQLX_OFFLINE='true', PYTHONDONTWRITEBYTECODE='1')
checks = [
 ('format', ['cargo','fmt','--all','--','--check'], {}),
 ('whitespace', ['git','diff','--check'], {}),
 ('offline', ['cargo','check','--locked','--offline','--all-targets'], {}),
 ('clippy', ['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'], {}),
 ('sqlx', ['cargo','sqlx','prepare','--check','--','--all-targets'], {'SQLX_OFFLINE':'false'}),
 ('compile-tests', ['cargo','test','--locked','--offline','--all-targets','--no-run'], {}),
 ('inventory', ['cargo','test','--locked','--offline','--all-targets','--','--list'], {}),
 ('full-suite', ['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--all-targets'], {}),
 ('stack-budget', ['scripts/test-network-isolation.sh','scripts/stack-budget.sh','--offline'], {}),
 ('lease-budget', ['scripts/test-network-isolation.sh','scripts/lease-budget.sh','--offline'], {}),
]
results=[]
for name, command, override in checks:
 start=time.monotonic(); print('START '+name,flush=True)
 with (LOGS/(name+'.log')).open('wb') as output:
  result=subprocess.run(command,cwd=ROOT,env=env|override,stdout=output,stderr=subprocess.STDOUT)
 item={'name':name,'command':command,'status':'PASS' if result.returncode==0 else 'FAIL','exit':result.returncode,'seconds':round(time.monotonic()-start,2),'log':'logs/'+name+'.log'}
 results.append(item);(RECORDS/'rust-checks.json').write_text(json.dumps(results,indent=2)+'\n');print(json.dumps(item),flush=True)
 if result.returncode:
  print((LOGS/(name+'.log')).read_text(errors='replace')[-9000:],flush=True)
  break
