import pathlib, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[6]
runner=root/'plan/vault-google/integration/channel-main/docs/logs/check.py'
def check(label,*args):
 r=subprocess.run([sys.executable,str(runner),label,*args],cwd=root)
 if r.returncode:sys.exit(r.returncode)
base=['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib']
check('sample-01','python3','scripts/workflow-sample-boundary.py')
check('channel-authoring-01','node','--test','scripts/tests/channel-response-authoring.mjs')
check('css-01','npm','run','check:css')
check('graft-01','graft','build')
