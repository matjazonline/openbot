from pathlib import Path
import json,subprocess,sys
l=Path(__file__).resolve().parent
selectors=json.loads((l/'final-focused-selectors.json').read_text());i=selectors.index('--skip')
selectors=selectors[:i]+['run_retention_','workflow_registry_','logical_seed_','workflow_capacity_count_prunes_retained_terminal_history']+selectors[i:]
checks=[
('p06-source-modules-r2',['cargo','test','--locked','--offline','--test','source_modules']),
('p06-migrate-validate-r2',['cargo','sqlx','migrate','run']),
('p06-sqlx-prepare-r2',['cargo','sqlx','prepare','--','--all-targets']),
('p06-sqlx-check-r2',['cargo','sqlx','prepare','--check','--','--all-targets']),
('p06-offline-r2',['cargo','check','--locked','--offline','--all-targets']),
('p06-clippy-r2',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('p06-discovery-final-02',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','--list',*selectors]),
]
for name,args in checks:
 assert not (l/(name+'.json')).exists(),name
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*args])
 if result.returncode:sys.exit(result.returncode)
