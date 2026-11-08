#!/usr/bin/env python3
"""Reproduce exactly the two observed onboarding failures at the untouched base."""
import datetime
import json
import os
from pathlib import Path
import subprocess
import time

logs = Path(__file__).resolve().parent
resources = json.loads((logs / "baseline-resources.json").read_text())
environment = os.environ.copy()
environment.pop("ALLOW_MISSING_DATABASE_URL", None)
settings = {
    "DATABASE_URL": f"postgres://mac02@127.0.0.1:{resources['port']}/{resources['database']}",
    "TEST_DATABASE_URL": f"postgres://mac02@127.0.0.1:{resources['port']}/{resources['test_database']}",
    "CARGO_TARGET_DIR": resources["target"], "SQLX_OFFLINE": "true",
    "CARGO_INCREMENTAL": "0", "CARGO_BUILD_JOBS": "4", "RUST_MIN_STACK": "2097152",
}
environment.update(settings)
prefix = "adapters::persistence::onboarding::tests::"
tests = ["onboarding_same_key_reuses_results_and_new_key_after_deletion_is_fresh",
         "onboarding_new_owner_with_new_key_does_not_replay_the_previous_owners_command"]
for index, test in enumerate(tests, 1):
    command = ["scripts/test-network-isolation.sh", "cargo", "test", "--locked", "--offline",
               "--lib", prefix + test, "--", "--exact"]
    before = time.monotonic()
    name = f"baseline-onboarding-{index:02d}"
    with (logs / f"{name}.log").open("w") as output:
        result = subprocess.run(command, cwd=resources["worktree"], env=environment,
                                stdout=output, stderr=subprocess.STDOUT)
    receipt = {"command": command, "cwd": resources["worktree"], "base": resources["base"],
               "environment": settings, "exit_code": result.returncode,
               "seconds": round(time.monotonic() - before, 3),
               "finished": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    (logs / f"{name}.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt), flush=True)
