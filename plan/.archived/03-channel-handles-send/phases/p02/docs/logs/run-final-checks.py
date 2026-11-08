#!/usr/bin/env python3
import subprocess
import sys
from pathlib import Path
logs=Path(__file__).resolve().parent
checks=[
 ("final-terminal-business-tests-01",["scripts/test-network-isolation.sh","cargo","test","--locked","--offline","--lib","channel_reply_terminal_preserves_business_value_and_replays_after_restart"]),
 ("final-terminal-invalid-tests-01",["scripts/test-network-isolation.sh","cargo","test","--locked","--offline","--lib","channel_reply_terminal_invalid_output_fails_or_takes_ordinary_error_route"]),
 ("final-sqlx-prepare-01",["cargo","sqlx","prepare","--","--locked","--offline","--all-targets"]),
 ("final-sqlx-check-01",["cargo","sqlx","prepare","--check","--","--locked","--offline","--all-targets"]),
 ("final-clippy-01",["cargo","clippy","--locked","--offline","--all-targets","--","-D","warnings"]),
 ("final-fmt-01",["cargo","fmt","--all","--","--check"]),
 ("final-diff-check-01",["git","diff","--check"]),
 ("final-graft-01",["graft","build"]),
]
for name,command in checks:
 result=subprocess.run([sys.executable,str(logs/"run-check.py"),name,*command])
 if result.returncode: sys.exit(result.returncode)
