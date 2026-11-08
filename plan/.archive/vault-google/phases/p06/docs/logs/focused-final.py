import json, os, pathlib, re, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[6]
logs=pathlib.Path(__file__).parent
families=['integration_gateway_','vault_workflow_http_','vault_workflow_mcp_','vault_authoring_','workflow_action_approval_','integration_contract_','vault_dispatch_authority_','google_refresh_','integration_renewal_','google_connection_lifecycle_','integration_connect_','google_vault_callback_','workflow_http_','vault_binding_','sample_direct_workflow_tool_child_and_message_have_zero_production_effects']
records=[]
for family in families:
    label='final-'+family.rstrip('_')
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
    (logs/'focused-final-results.json').write_text(json.dumps(records,indent=2)+'\n')
    if result.returncode: sys.exit(result.returncode)
