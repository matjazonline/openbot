import pathlib, subprocess, sys
root=pathlib.Path(__file__).resolve().parents[6]
runner=root/'plan/vault-google/phases/p05/docs/logs/check.py'
def check(label,*args):
 r=subprocess.run([sys.executable,str(runner),label,*args],cwd=root)
 if r.returncode:sys.exit(r.returncode)
base=['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib']
check('style-correction-02',*base,'rendered_markup_carries_no_inline_javascript')
check('vault-routes-correction-01',*base,'adapters::http::routes::ui_vault::tests')
check('main-binary-01','env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--bin','open_bots')
check('offline-correction-01','cargo','check','--locked','--offline','--all-targets')
check('clippy-correction-01','cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings')
check('format-correction-01','cargo','fmt','--all','--','--check')
check('diff-correction-01','git','diff','HEAD','--check')
check('transport-correction-01','bash','scripts/transport-boundary-check.sh')
check('graft-correction-01','graft','build')
