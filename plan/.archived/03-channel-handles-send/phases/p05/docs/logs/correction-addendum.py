import json,subprocess,sys,re
from pathlib import Path
logs=Path(__file__).resolve().parent
command=['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','knowledge_run_level_','--','--nocapture']
r=subprocess.run([sys.executable,str(logs/'run-check.py'),'release-knowledge-tests-02',*command]);sys.exit(r.returncode)
