#!/usr/bin/env python3
"""Explicit p01 caller/interface regression selections; never a full-suite runner."""
import json
from pathlib import Path
import subprocess
import sys

logs = Path(__file__).resolve().parent
filters = [
    ("message-intents", "message_intent_"),
    ("message-replies", "message_reply_"),
    ("bindings", "workflow_binding_sql_"),
    ("admission-snapshots", "workflow_admission_binding_"),
    ("schedule-binding", "schedule_binding_must_fit_the_schedule_channel"),
    ("schedule-reconfiguration", "binding_reconfiguration_is_refused_by_an_incompatible_schedule"),
    ("provisioning", "builder_provisions_an_assistant_for_a_signed_in_admin"),
    ("onboarding", "adapters::persistence::onboarding::tests::"),
    ("binding-triggers", "binding_message_trigger_"),
    ("binding-actor", "binding_run_as_"),
]
results = []
for name, selected in filters:
    base = ["cargo", "test", "--locked", "--offline", "--lib", selected]
    listing = f"regression-{name}-list-01"
    subprocess.run([sys.executable, str(logs / "run-check.py"), listing, *base, "--", "--list"], check=True)
    count = sum(line.endswith(": test") for line in (logs / f"{listing}.log").read_text().splitlines())
    if count == 0:
        raise RuntimeError(f"Empty intended regression selection: {selected}")
    executed = f"regression-{name}-tests-01"
    status = subprocess.run([sys.executable, str(logs / "run-check.py"), executed,
                             "scripts/test-network-isolation.sh", *base]).returncode
    results.append({"filter": selected, "selected": count, "exit_code": status,
                    "inventory": listing, "execution": executed})
    (logs / "regressions-summary.json").write_text(json.dumps(results, indent=2) + "\n")
    if status:
        sys.exit(status)
