import json,subprocess,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
checks=[('deletion-fmt-check-01',['cargo','fmt','--all','--','--check']),('deletion-diff-check-01',['git','diff','--check']),('deletion-reference-check-01',[sys.executable,str(logs/'deletion-reference-check.py')]),('deletion-graft-build-01',['graft','build'])]
summary=[]
for name,command in checks:
 r=subprocess.run([sys.executable,str(logs/'run-check.py'),name,*command],capture_output=True,text=True);summary.append({'check':name,'exit':r.returncode});(logs/'deletion-checks-01.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary[-1]),flush=True)
 if r.returncode:print(r.stdout[-3000:]);sys.exit(r.returncode)
