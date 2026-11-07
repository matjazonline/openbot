#!/usr/bin/env python3
"""P02 traced contract/caller selections; this never runs the full suite."""
import json
from pathlib import Path
import subprocess
import sys
logs=Path(__file__).resolve().parent
filters=[
 ("terminal","channel_reply_terminal_"),
 ("registry","application::workflow::registry::"),
 ("publication-storage","publication::storage::tests::"),
 ("sample","application::workflow::sample::"),
 ("batch","workflow_batch_"),
 ("completion","workflow_completion_"),
 ("wait","workflow_wait_"),
 ("decision","workflow_decision_"),
 ("child-call","workflow_call_"),
 ("child-repeat","workflow_repeat_"),
 ("poison-recovery","workflow_poison_"),
 ("binding","channel_reply_binding_"),
]
if len(sys.argv)>1:
 filters=filters[[name for name,_ in filters].index(sys.argv[1]):]
results=[]
for name,selected in filters:
 base=["cargo","test","--locked","--offline","--lib",selected]
 listing=f"focused-{name}-list-04"
 subprocess.run([sys.executable,str(logs/"run-check.py"),listing,*base,"--","--list"],check=True)
 count=sum(line.endswith(": test") for line in (logs/f"{listing}.log").read_text().splitlines())
 if count==0: raise RuntimeError(f"Empty intended selection {selected}")
 executed=f"focused-{name}-tests-04"
 status=subprocess.run([sys.executable,str(logs/"run-check.py"),executed,"scripts/test-network-isolation.sh",*base]).returncode
 results.append({"filter":selected,"selected":count,"exit_code":status,"inventory":listing,"execution":executed})
 (logs/"focused-summary-04.json").write_text(json.dumps(results,indent=2)+"\n")
 if status: sys.exit(status)
