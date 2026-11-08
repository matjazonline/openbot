#!/usr/bin/env python3
"""Focused combined correction verification for B1-F1/F2."""
from pathlib import Path
import subprocess
import sys

logs = Path(__file__).resolve().parent
attempt = sys.argv[1] if len(sys.argv) > 1 else "01"

def run(name, *command):
    subprocess.run([sys.executable, str(logs / "run-check.py"), name, *command], check=True)

for label, selected, expected in [
    ("binding", "channel_reply_binding_", 19),
    ("intents", "message_intent_", 14),
    ("actor", "binding_run_as_", 1),
]:
    command = ["cargo", "test", "--locked", "--offline", "--lib", selected]
    name = f"correction-{label}-list-{attempt}"
    run(name, *command, "--", "--list")
    count = sum(line.endswith(": test") for line in (logs / f"{name}.log").read_text().splitlines())
    if count != expected:
        raise RuntimeError(f"Unexpected correction inventory {selected}: {count}")
    run(f"correction-{label}-tests-{attempt}", "scripts/test-network-isolation.sh", *command)
run(f"correction-sqlx-prepare-{attempt}", "cargo", "sqlx", "prepare", "--", "--locked", "--offline", "--all-targets")
run(f"correction-sqlx-check-{attempt}", "cargo", "sqlx", "prepare", "--check", "--", "--locked", "--offline", "--all-targets")
run(f"correction-clippy-offline-{attempt}", "cargo", "clippy", "--locked", "--offline", "--all-targets", "--", "-D", "warnings")
run(f"correction-fmt-{attempt}", "cargo", "fmt", "--all", "--", "--check")
run(f"correction-diff-{attempt}", "git", "diff", "--check")
