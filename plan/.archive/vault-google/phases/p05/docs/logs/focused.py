import pathlib, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[6]
runner=root/'plan/vault-google/phases/p05/docs/logs/check.py'
def check(label,*args):
 r=subprocess.run([sys.executable,str(runner),label,*args],cwd=root)
 if r.returncode:sys.exit(r.returncode)
base=['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib']
check('test-inventory-01',*base,'--','--list')
for label,term in [('contracts-01','integration_contract_'),('connect-regression-01','integrations::connect_tests::'),('connect-routes-01','ui_vault_connect::tests'),('http-regression-01','workflow_http_'),('mcp-regression-01','workflow_mcp_'),('approval-regression-01','workflow_action_approval_'),('rotation-regression-01','adapters::persistence::credential_rotation::tests')]:
 check(label,*base,term)
check('sqlx-prepare-01','env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--','--all-targets')
check('sqlx-check-01','env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--check','--','--all-targets')
check('offline-check-01','cargo','check','--locked','--offline','--all-targets')
check('clippy-01','cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings')
check('format-01','cargo','fmt','--all','--','--check')
check('diff-01','git','diff','HEAD','--check')
check('css-01','npm','run','check:css')
check('transport-fixtures-01','bash','scripts/tests/transport-boundary.sh')
check('transport-boundary-01','bash','scripts/transport-boundary-check.sh')
check('sample-boundary-01','python3','scripts/workflow-sample-boundary.py')
check('graft-build-01','graft','build')
