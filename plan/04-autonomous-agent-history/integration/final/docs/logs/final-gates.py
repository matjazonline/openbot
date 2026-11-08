from pathlib import Path
import json,subprocess,sys
l=Path(__file__).resolve().parent
resources=json.loads((l/'resources.json').read_text())
selectors=json.loads((l/'planned-final-selectors.json').read_text())['selectors']
checks=[
('final-migrate-main-01',['cargo','sqlx','migrate','run']),
('final-migrate-test-01',['env',f"DATABASE_URL=postgres://mac02@127.0.0.1:{resources['port']}/{resources['test_database']}",'cargo','sqlx','migrate','run']),
('final-css-build-01',['npm','run','build:css']),
('final-css-check-01',['npm','run','check:css']),
('final-discovery-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','--list',*selectors]),
('final-runtime-01',['env','AGENT_HISTORY_BROWSER_FIXTURES='+str(l/'browser-fixtures-01'),'scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--',*selectors,'--nocapture']),
('final-source-modules-01',['cargo','test','--locked','--offline','--test','source_modules']),
('final-sqlx-prepare-01',['cargo','sqlx','prepare','--','--all-targets']),
('final-sqlx-check-01',['cargo','sqlx','prepare','--check','--','--all-targets']),
('final-offline-01',['cargo','check','--locked','--offline','--all-targets']),
('final-clippy-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('final-fmt-01',['cargo','fmt','--all','--','--check']),
('final-diff-01',['git','diff','--check']),
('final-graft-01',['graft','build']),
]
for name,args in checks:
 assert not (l/(name+'.json')).exists(),name
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*args])
 if result.returncode:sys.exit(result.returncode)
