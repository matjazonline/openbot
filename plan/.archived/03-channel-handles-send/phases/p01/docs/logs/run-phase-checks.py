#!/usr/bin/env python3
"""Final p01 checks, each with separate durable command/output receipts."""
from pathlib import Path
import subprocess
import sys

logs = Path(__file__).resolve().parent

def run(name, *command):
    subprocess.run([sys.executable, str(logs / "run-check.py"), name, *command], check=True)

for label, selected, expected in [
    ("contract", "channel_reply_contract_", 8),
    ("binding", "channel_reply_binding_", 17),
]:
    command = ["cargo", "test", "--locked", "--offline", "--lib", selected]
    run(f"author-{label}-list-01", *command, "--", "--list")
    count = sum(line.endswith(": test") for line in
                (logs / f"author-{label}-list-01.log").read_text().splitlines())
    if count != expected:
        raise RuntimeError(f"Unexpected focused inventory for {selected}: {count}, expected {expected}")
    run(f"author-{label}-tests-01", "scripts/test-network-isolation.sh", *command)
subprocess.run([sys.executable, str(logs / "run-regressions.py")], check=True)
run("author-sqlx-prepare-01", "cargo", "sqlx", "prepare", "--", "--all-targets")
run("author-sqlx-check-01", "cargo", "sqlx", "prepare", "--check", "--", "--all-targets")
run("author-fmt-final-01", "cargo", "fmt", "--all", "--", "--check")
run("author-diff-final-01", "git", "diff", "--check")
