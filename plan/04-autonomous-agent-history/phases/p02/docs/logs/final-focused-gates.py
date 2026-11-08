"""Sequential, recorded P02 gates; stop at the first failure. No full-suite command."""
from pathlib import Path
import subprocess
import sys

runner=Path(__file__).with_name('run-check.py')
checks=[
 ('checkpoint-style-07',['cargo','fmt','--all']),
 ('history-list-11',['cargo','test','--locked','--offline','--lib','agent_history_','--','--list']),
 ('history-runtime-11',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','agent_history_']),
 ('sqlx-prepare-exchanges-01',['cargo','sqlx','prepare','--','--locked','--offline','--all-targets']),
 ('sqlx-check-exchanges-01',['cargo','sqlx','prepare','--check','--','--locked','--offline','--all-targets']),
 ('offline-all-targets-exchanges-01',['cargo','check','--locked','--offline','--all-targets']),
 ('clippy-exchanges-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('fmt-exchanges-01',['cargo','fmt','--all','--','--check']),
 ('diff-exchanges-01',['git','diff','--check']),
 ('graft-exchanges-01',['graft','build']),
]
for name,command in checks:
 result=subprocess.run([sys.executable,str(runner),name,*command])
 if result.returncode:
  sys.exit(result.returncode)
