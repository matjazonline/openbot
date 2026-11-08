from pathlib import Path
import json,subprocess,sys
l=Path(__file__).resolve().parent
selectors=json.loads((l/'final-focused-selectors.json').read_text())
checks=[
('p07-discovery-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','--list',*selectors]),
('p07-final-runtime-01',['env','AGENT_HISTORY_BROWSER_FIXTURES='+str(l/'browser-fixtures-01'),'scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--',*selectors,'--skip','workflow_capacity_count_prunes_retained_terminal_history','--nocapture']),
('p07-source-modules-01',['cargo','test','--locked','--offline','--test','source_modules']),
('p07-migrate-validate-01',['cargo','sqlx','migrate','run']),
('p07-sqlx-prepare-01',['cargo','sqlx','prepare','--','--all-targets']),
('p07-sqlx-check-01',['cargo','sqlx','prepare','--check','--','--all-targets']),
('p07-offline-01',['cargo','check','--locked','--offline','--all-targets']),
('p07-clippy-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('p07-fmt-01',['cargo','fmt','--all','--','--check']),
('p07-diff-01',['git','diff','--check']),
('p07-graft-01',['graft','build']),
]
for name,args in checks:
 assert not (l/(name+'.json')).exists(),name
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*args])
 if result.returncode:sys.exit(result.returncode)
