#!/usr/bin/env python3
from pathlib import Path
import subprocess,sys
logs=Path(__file__).resolve().parent
for name,selected in [
 ("correction-budget-before-03","channel_reply_terminal_contract_preflight_charges_all_schema_copies"),
 ("correction-exhaustion-before-02","channel_reply_terminal_exhausted_pending_is_effect_free_failure"),
]:
 subprocess.run([sys.executable,str(logs/"run-check.py"),name,"scripts/test-network-isolation.sh","cargo","test","--locked","--offline","--lib",selected,"--","--nocapture"])
