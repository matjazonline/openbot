import json, os, pathlib, re, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[6]
logs=pathlib.Path(__file__).parent
families=['workflow_action_dispatch_','workflow_action_replay_','workflow_action_receipt_','workflow_action_cancellation_']
records=[]
for family in families:
    label='native-'+family.rstrip('_')
    prefix=[sys.executable,str(logs/'check.py')]
    cargo=['cargo','test','--locked','--offline','--lib',family]
    result=subprocess.run(prefix+[label+'-list']+cargo+['--','--list'],cwd=root)
    if result.returncode: sys.exit(result.returncode)
    listing=(logs/(label+'-list.log')).read_text()
    count=len(re.findall(r': test$', listing,re.M))
    if not count: raise RuntimeError('empty family '+family)
    command=['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4']
    if family=='vault_authoring_': command+=['VAULT_AUTHORING_BROWSER_FIXTURES='+str(logs/'browser-fixtures')]
    command+=['bash','scripts/test-network-isolation.sh']+cargo
    result=subprocess.run(prefix+[label+'-run']+command,cwd=root)
    record={'family':family,'listed':count,'exit':result.returncode};records.append(record)
    (logs/'focused-native-results.json').write_text(json.dumps(records,indent=2)+'\n')
    if result.returncode: sys.exit(result.returncode)
