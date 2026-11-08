from pathlib import Path
import subprocess,sys,json
l=Path(__file__).resolve().parent
for name,cmd in json.loads((l/'final-build-steps.json').read_text()):
 r=subprocess.run([sys.executable,str(l/'run-check.py'),name,*cmd])
 if r.returncode:sys.exit(r.returncode)
