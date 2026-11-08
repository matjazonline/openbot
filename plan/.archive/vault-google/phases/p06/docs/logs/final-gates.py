import pathlib,subprocess,sys
logs=pathlib.Path(__file__).parent
checks=[
('format-final01',['cargo','fmt','--all','--','--check']),
('diff-final01',['git','diff','--check']),
('offline-final02',['cargo','check','--locked','--offline','--all-targets']),
('clippy-final02',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('migrate-final01',['env','SQLX_OFFLINE=false','cargo','sqlx','migrate','run']),
('sqlx-check-final01',['env','SQLX_OFFLINE=false','cargo','sqlx','prepare','--check','--','--all-targets']),
('authoring-list-final01',['cargo','test','--locked','--offline','--lib','vault_authoring_','--','--list']),
('authoring-run-final01',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','vault_authoring_']),
('renewal-borrow-final01',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','integration_renewal_'])]
for name,cmd in checks:
 result=subprocess.run([sys.executable,str(logs/'check.py'),name]+cmd)
 if result.returncode:sys.exit(result.returncode)
