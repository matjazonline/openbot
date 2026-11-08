import pathlib,subprocess,sys
logs=pathlib.Path(__file__).parent
checks=[('baseline-migrate01',['env','SQLX_OFFLINE=false','cargo','sqlx','migrate','run']),('baseline-contracts01',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','integration_contract_']),('baseline-authority01',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','vault_dispatch_authority_']),('baseline-renewal-races01',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','google_refresh_race_'])]
for label,args in checks:
 result=subprocess.run([sys.executable,str(logs/'check.py'),label,*args])
 if result.returncode:sys.exit(result.returncode)
