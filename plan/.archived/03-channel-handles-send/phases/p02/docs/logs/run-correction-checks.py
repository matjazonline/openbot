#!/usr/bin/env python3
import json,subprocess,sys
from pathlib import Path
logs=Path(__file__).resolve().parent
filters=[("terminal","channel_reply_terminal_"),("failure","application::workflow::actions::failure::tests::"),("registry","application::workflow::registry::"),("recovery","workflow_poison_")]
rows=[]
for name,selected in filters:
 base=["cargo","test","--locked","--offline","--lib",selected]
 listing=f"correction-{name}-list-01"
 subprocess.run([sys.executable,str(logs/"run-check.py"),listing,*base,"--","--list"],check=True)
 count=sum(line.endswith(": test") for line in (logs/f"{listing}.log").read_text().splitlines())
 if count==0:raise RuntimeError(f"Empty correction selection {selected}")
 executed=f"correction-{name}-tests-01"
 status=subprocess.run([sys.executable,str(logs/"run-check.py"),executed,"scripts/test-network-isolation.sh",*base]).returncode
 rows.append({"filter":selected,"selected":count,"exit_code":status,"inventory":listing,"execution":executed})
 (logs/"author-correction-checks.json").write_text(json.dumps(rows,indent=2)+"\n")
 if status:sys.exit(status)
for name,command in [
 ("correction-clippy-01",["cargo","clippy","--locked","--offline","--all-targets","--","-D","warnings"]),
 ("correction-fmt-01",["cargo","fmt","--all","--","--check"]),
 ("correction-diff-01",["git","diff","--check"]),
 ("correction-graft-01",["graft","build"]),
]:
 result=subprocess.run([sys.executable,str(logs/"run-check.py"),name,*command])
 if result.returncode:sys.exit(result.returncode)
