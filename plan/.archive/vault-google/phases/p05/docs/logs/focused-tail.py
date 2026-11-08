import pathlib, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[6]
runner=root/'plan/vault-google/phases/p05/docs/logs/check.py'
def check(label,*args):
 r=subprocess.run([sys.executable,str(runner),label,*args],cwd=root)
 if r.returncode:sys.exit(r.returncode)
base=['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib']
check('renewal-scope-regression-01',*base,'google_refresh_omitted_refresh_is_preserved_and_cache_updates_do_not_change_consent')
check('format-01','cargo','fmt','--all','--','--check')
check('diff-01','git','diff','HEAD','--check')
check('css-01','npm','run','check:css')
check('transport-fixtures-01','bash','scripts/tests/transport-boundary.sh')
check('transport-boundary-01','bash','scripts/transport-boundary-check.sh')
check('sample-boundary-01','python3','scripts/workflow-sample-boundary.py')
check('graft-build-01','graft','build')
