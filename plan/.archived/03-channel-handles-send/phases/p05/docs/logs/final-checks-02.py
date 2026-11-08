import json,subprocess,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
checks=[
 ('clippy-02',['cargo','clippy','--locked','--offline','--all-targets','--','-D','warnings']),
 ('sqlx-prepare-03',['cargo','sqlx','prepare','--','--all-targets']),
 ('sqlx-check-03',['cargo','sqlx','prepare','--check','--','--all-targets']),
 ('fmt-check-02',['cargo','fmt','--all','--','--check']),
 ('diff-check-02',['git','diff','--check']),
 ('graft-build-02',['graft','build']),
]
summary=[]
for name,command in checks:
 result=subprocess.run([sys.executable,str(logs/'run-check.py'),name,*command],capture_output=True,text=True)
 summary.append({'check':name,'exit':result.returncode})
 (logs/'final-checks-02.json').write_text(json.dumps(summary,indent=2)+'\n')
 print(json.dumps(summary[-1]),flush=True)
 if result.returncode:
  print(result.stdout[-9000:],flush=True);sys.exit(result.returncode)
