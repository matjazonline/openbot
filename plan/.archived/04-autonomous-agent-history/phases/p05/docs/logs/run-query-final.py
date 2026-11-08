from pathlib import Path
import subprocess,sys,json
l=Path(__file__).resolve().parent
for name,cmd in json.loads((l/'final-query-steps.json').read_text()):
 result=subprocess.run([sys.executable,str(l/'run-check.py'),name,*cmd])
 if result.returncode:sys.exit(result.returncode)
