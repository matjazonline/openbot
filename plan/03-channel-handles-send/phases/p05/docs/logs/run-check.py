#!/usr/bin/env python3
"""Run one recorded check with the plan-owned local PostgreSQL fixture."""
import datetime
import json
import os
from pathlib import Path
import subprocess
import sys
import time

logs = Path(__file__).resolve().parent
resources = json.loads((logs / "resources.json").read_text())
name, *command = sys.argv[1:]
environment = os.environ.copy()
environment.pop("ALLOW_MISSING_DATABASE_URL", None)
settings = {
    "DATABASE_URL": f"postgres://mac02@127.0.0.1:{resources['port']}/{resources['database']}",
    "TEST_DATABASE_URL": f"postgres://mac02@127.0.0.1:{resources['port']}/{resources['test_database']}",
    "SQLX_OFFLINE": "false" if "sqlx" in command else "true",
    "CARGO_INCREMENTAL": "1",
    "CARGO_BUILD_JOBS": "4",
    "RUST_MIN_STACK": "2097152",
}
environment.update(settings)
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
before = time.monotonic()
output = logs / f"{name}.log"
with output.open("w") as stream:
    result = subprocess.run(command, cwd=logs.parents[5], env=environment,
                            stdout=stream, stderr=subprocess.STDOUT)
receipt = {
    "command": command, "cwd": str(logs.parents[5]), "environment": settings,
    "started": started, "seconds": round(time.monotonic() - before, 3),
    "exit_code": result.returncode, "log": str(output),
}
(logs / f"{name}.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps(receipt))
print("\n".join(output.read_text(errors="replace").replace("\r", "\n").splitlines()[-25:]))
sys.exit(result.returncode)
