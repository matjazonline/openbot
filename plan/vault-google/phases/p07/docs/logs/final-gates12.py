import pathlib,re,subprocess,sys,json
logs=pathlib.Path(__file__).parent
failures=[]
def run(label,args):
 code=subprocess.run([sys.executable,str(logs/'check.py'),label]+args).returncode
 if code:failures.append((label,code))
 return code
checks=[('final-fmt',['cargo','fmt','--all','--','--check']),('final-diff',['git','diff','--check']),('final-offline',['cargo','check','--locked','--offline','--all-targets']),('final-clippy12',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),('final-migrations',['env','SQLX_OFFLINE=false','cargo','sqlx','migrate','run']),('final-prepare',['env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--','--all-targets']),('final-prepare-check',['env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--check','--','--all-targets'])]
for label,args in checks:run(label,args)
families=['integration_renewal_','google_refresh_','google_connection_lifecycle_','integration_gateway_','vault_workflow_http_','vault_workflow_mcp_','vault_rotation_','vault_storage_','vault_management_','vault_binding_','vault_dispatch_authority_']
for index,family in enumerate(families):
 label=f'final-existing12-{index+1}'
 cargo=['cargo','test','--locked','--offline','--lib',family]
 if run(label+'-list',cargo+['--','--list']):continue
 count=len(re.findall(r': test$',(logs/(label+'-list.log')).read_text(),re.M));print(family,count,flush=True)
 if not count:failures.append((family,'empty'));continue
 run(label+'-run',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh']+cargo)
for label,args in [('final-transport-fixtures',['bash','scripts/tests/transport-boundary.sh']),('final-transport',['bash','scripts/transport-boundary-check.sh']),('final-sample-fixtures',['python3','scripts/tests/workflow-sample-boundary.py']),('final-sample',['python3','scripts/workflow-sample-boundary.py'])]:run(label,args)
(logs/'final-gates12-summary.json').write_text(json.dumps({'failures':failures},indent=2)+'\n')
print('failures',failures,flush=True)
sys.exit(bool(failures))
