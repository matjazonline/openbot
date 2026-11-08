from pathlib import Path
import subprocess,sys
l=Path(__file__).resolve().parent
checks=[
('p07-process-current-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','agent_history_e2e_process_','--','--nocapture']),
('p07-offline-03',['cargo','check','--locked','--offline','--all-targets']),
('p07-clippy-03',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('p07-fmt-03',['cargo','fmt','--all','--','--check']),
('p07-diff-03',['git','diff','--check']),
('p07-graft-03',['graft','build']),
('p07-case-evidence-02',['python3',str(l/'verify-case-evidence.py')]),
]
for name,args in checks:
 assert not (l/(name+'.json')).exists(),name
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*args])
 if result.returncode:sys.exit(result.returncode)
