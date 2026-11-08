import pathlib, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[6]
runner=root/'plan/vault-google/integration/channel-main/docs/logs/check.py'
def check(label,*args):
 r=subprocess.run([sys.executable,str(runner),label,*args],cwd=root)
 if r.returncode:sys.exit(r.returncode)
base=['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib']
check('inventory-01',*base,'--','--list')
for label,term in [('vault-authority-01','vault_dispatch_authority_'),('vault-bindings-01','vault_binding_'),('vault-storage-01','vault_storage_'),('vault-lifecycle-01','vault_lifecycle_'),('vault-rotation-01','vault_rotation_'),('renewal-races-01','google_refresh_race_'),('channel-completion-01','channel_reply_completion_'),('channel-e2e-01','channel_reply_e2e_'),('provisioning-01','provisioning_'),('supervision-01','production_workers_stop_on_the_shutdown_token'),('markup-01','rendered_markup_carries_no_inline_javascript')]:
 check(label,*base,term)
check('sqlx-prepare-01','env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--','--all-targets')
check('sqlx-check-01','env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--check','--','--all-targets')
check('clippy-01','cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings')
check('format-01','cargo','fmt','--all','--','--check')
check('diff-01','git','diff','HEAD','--check')
check('transport-01','bash','scripts/transport-boundary-check.sh')
check('sample-01','python3','scripts/workflow-sample-boundary.py')
check('channel-authoring-01','node','--test','scripts/tests/channel-response-authoring.mjs')
check('css-01','npm','run','check:css')
check('graft-01','graft','build')
