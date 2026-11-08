from pathlib import Path
import json,subprocess,sys
l=Path(__file__).resolve().parent
selectors=json.loads((l/'final-focused-selectors.json').read_text())
checks=[
('p07-inheritance-current-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','inherited_listener_rejects_a_reflecting_listener_without_disclosing_its_secret']),
('p07-discovery-02',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','--list',*selectors]),
('p07-sqlx-check-02',['cargo','sqlx','prepare','--check','--','--all-targets']),
('p07-offline-02',['cargo','check','--locked','--offline','--all-targets']),
('p07-clippy-02',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('p07-fmt-02',['cargo','fmt','--all','--','--check']),
('p07-diff-02',['git','diff','--check']),
('p07-graft-02',['graft','build']),
('p07-case-evidence-01',['python3',str(l/'verify-case-evidence.py')]),
]
for name,args in checks:
 assert not (l/(name+'.json')).exists(),name
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*args])
 if result.returncode:sys.exit(result.returncode)
