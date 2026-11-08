from pathlib import Path
import subprocess,sys
l=Path(__file__).resolve().parent
checks=[
('p06-decision-fixtures-01',['env','WORKFLOW_DECISION_BROWSER_FIXTURES='+str(l/'decision-fixtures-01'),'scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','decision_browser_markup_and_closed_draft_recovery','source_message_outside_window_is_exact_and_authorized','workflow_capacity_count_prunes_retained_terminal_history','autonomous_agent_storage_action_origin_populated_collector_and_company_cascade']),
('p06-decision-browser-01',['env','CHROME_PATH=/Applications/Google Chrome.app/Contents/MacOS/Google Chrome','WORKFLOW_DECISION_BROWSER_FIXTURES='+str(l/'decision-fixtures-01'),'node','--test','scripts/tests/workflow-decisions.mjs']),
('p06-migrate-final-01',['cargo','sqlx','migrate','run']),
('p06-sqlx-prepare-01',['cargo','sqlx','prepare','--','--all-targets']),
('p06-sqlx-check-01',['cargo','sqlx','prepare','--check','--','--all-targets']),
('p06-offline-final-01',['cargo','check','--locked','--offline','--all-targets']),
('p06-clippy-final-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
]
import json
selectors=json.loads((l/'final-focused-selectors.json').read_text())
selectors=selectors[:selectors.index('--skip')]+['run_retention_','workflow_registry_','logical_seed_','workflow_capacity_count_prunes_retained_terminal_history']+selectors[selectors.index('--skip'):]
checks.append(('p06-discovery-final-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','--list',*selectors]))
for name,args in checks:
 assert not (l/(name+'.json')).exists(),name
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*args])
 if result.returncode:sys.exit(result.returncode)
