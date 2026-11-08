import json,subprocess,sys
from pathlib import Path
p=Path(__file__).resolve().parent
filters=json.loads((p/"focused-filters.json").read_text())
name=sys.argv[1]
command=["scripts/test-network-isolation.sh","cargo","test","--locked","--offline","--lib","--"]
if "list" in name: command += ["--list"]
command += filters+["--skip","dump_autonomous_workflow_agent_picker_browser_fixtures"]
sys.exit(subprocess.call([sys.executable,str(p/"run-check.py"),name,*command]))
