import json,pathlib,re,subprocess,sys
logs=pathlib.Path(__file__).parent
prefix=[sys.executable,str(logs/'check.py')]
checks=[('correction-clippy02',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings'])]
for family in ['vault_workflow_','integration_gateway_','adapters::mcp::tests::','integration_renewal_','vault_dispatch_authority_']:
    label='correction-'+family.replace(':','').rstrip('_')
    cargo=['cargo','test','--locked','--offline','--lib',family]
    checks.extend([(label+'-list02',cargo+['--','--list']),(label+'-run02',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh']+cargo)])
checks.extend([('correction-fmt02',['cargo','fmt','--all','--','--check']),('correction-diff02',['git','diff','--check']),('correction-offline02',['cargo','check','--locked','--offline','--all-targets']),('correction-sqlx02',['env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--check','--','--all-targets'])])
for label,command in checks:
    result=subprocess.run(prefix+[label]+command)
    if result.returncode:sys.exit(result.returncode)
    if '-list' in label:
        count=len(re.findall(r': test$',(logs/(label+'.log')).read_text(),re.M))
        if not count:raise RuntimeError('empty test filter '+label)
        print(label,'count',count,flush=True)
