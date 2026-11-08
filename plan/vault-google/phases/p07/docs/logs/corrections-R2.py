import pathlib,subprocess,sys,json,re
logs=pathlib.Path(__file__).parent
listed=(logs/'correction-service-list-R2.log').read_text();count=len(re.findall(r': test$',listed,re.M));assert count==23,count
checks=[
('correction-service-run-R2',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=1','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','integration_service_contract_']),
('correction-custody-run-R2',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=1','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','vault_storage_host_consumers_do_not_read_module_tables']),
('correction-offline-R2',['cargo','check','--locked','--offline','--all-targets']),
('correction-clippy-R2',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('correction-fmt-R2',['cargo','fmt','--all','--','--check']),
('correction-diff-R2',['git','diff','--check']),
('correction-boundary-mutations-R2',['python3','scripts/tests/integration-boundary.py']),
('correction-boundary-R2',['python3','scripts/integration-boundary.py']),
('correction-docs-R2',['python3','scripts/check-integration-docs.py'])]
failed=[]
for label,cmd in checks:
 r=subprocess.run([sys.executable,str(logs/'check.py'),label,*cmd])
 if r.returncode:failed.append((label,r.returncode))
(logs/'correction-results-R2.json').write_text(json.dumps({'service_count':count,'failures':failed},indent=2)+'\n');sys.exit(bool(failed))
