import pathlib,subprocess,sys,json
logs=pathlib.Path(__file__).parent
checks=[('fmt',['cargo','fmt','--all','--','--check']),('offline',['cargo','check','--locked','--offline','--all-targets']),('clippy',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),('reachability',['env','RUST_MIN_STACK=2097152','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--test','source_modules']),('integration-mutations',['python3','scripts/tests/integration-boundary.py']),('integration-boundary',['python3','scripts/integration-boundary.py']),('docs',['python3','scripts/check-integration-docs.py']),('evidence-fixtures',['python3','scripts/tests/workflow-evidence.py'])]
failed=[]
for label,cmd in checks:
 r=subprocess.run([sys.executable,str(logs/'check.py'),label,*cmd])
 if r.returncode:failed.append((label,r.returncode))
(logs/'checks-summary.json').write_text(json.dumps({'failures':failed},indent=2)+'\n');sys.exit(bool(failed))
