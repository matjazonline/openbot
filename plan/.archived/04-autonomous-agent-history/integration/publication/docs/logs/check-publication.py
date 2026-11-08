from pathlib import Path
import json,subprocess,sys
l=Path(__file__).resolve().parent;res=json.loads((l/'resources.json').read_text());selectors=json.loads((l/'selectors.json').read_text())
checks=[
('pub-migrate-main-01',['cargo','sqlx','migrate','run']),
('pub-migrate-test-01',['env',f"DATABASE_URL=postgres://mac02@127.0.0.1:{res['port']}/{res['test_database']}",'cargo','sqlx','migrate','run']),
('pub-css-build-01',['npm','run','build:css']),
('pub-css-check-01',['npm','run','check:css']),
('pub-offline-01',['cargo','check','--locked','--offline','--all-targets']),
('pub-discovery-01',['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--','--list',*selectors]),
('pub-runtime-01',['env','AGENT_HISTORY_BROWSER_FIXTURES='+str(l/'browser-fixtures-01'),'scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','--',*selectors,'--nocapture']),
('pub-sqlx-prepare-01',['cargo','sqlx','prepare','--','--all-targets']),
('pub-sqlx-check-01',['cargo','sqlx','prepare','--check','--','--all-targets']),
('pub-clippy-01',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
('pub-fmt-01',['cargo','fmt','--all','--','--check']),
('pub-diff-01',['git','diff','HEAD','--check','--','.',':(glob,exclude)plan/**/docs/**']),
('pub-sample-01',['python3','scripts/workflow-sample-boundary.py']),
('pub-graft-01',['graft','build']),
]
for name,args in checks:
 assert not (l/(name+'.json')).exists(),name
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*args])
 if result.returncode:sys.exit(result.returncode)
