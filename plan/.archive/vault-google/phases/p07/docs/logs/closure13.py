import pathlib,subprocess,sys,json
logs=pathlib.Path(__file__).parent
checks=[
('closure13-fmt',['cargo','fmt','--all','--','--check']),
('closure13-diff',['git','diff','--check']),
('closure13-clippy',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('closure13-zero',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=1','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','vault_google_e2e_direct_nested_model_and_sealed_sample_never_resolve_private_credentials']),
('closure13-reviewed',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=1','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','vault_google_e2e_reviewed_supplied_mime_resumes_after_expiry_with_same_approval']),
('closure13-boundary-fixtures',['python3','scripts/tests/integration-boundary.py']),
('closure13-boundary',['python3','scripts/integration-boundary.py']),
('closure13-docs',['python3','scripts/check-integration-docs.py'])]
failed=[]
for label,cmd in checks:
 r=subprocess.run([sys.executable,str(logs/'check.py'),label,*cmd])
 if r.returncode:failed.append((label,r.returncode))
(logs/'closure13-summary.json').write_text(json.dumps({'failures':failed},indent=2)+'\n')
sys.exit(bool(failed))
