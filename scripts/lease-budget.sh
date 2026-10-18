#!/usr/bin/env bash
#
# Fail when a workflow persistence operation's code-owned work approaches its attempt bound.
#
# The lease, renewal, release, action dispatch and receipt, budget and completion
# transactions each run under a client bound derived from the lease
# (LeasePolicy::attempt_bound, at most 2 s) with a read-only resolution after an ambiguous
# COMMIT. Raising those bounds removed the signal that they were being approached, so this
# re-runs the lease, action and budget suites uncontended
# (--test-threads=1) with the test-only phase recorder on, and gates the WORK phase: the
# statements after the operation's leading locks (run, execution, job; capacity for a
# renewal) up to COMMIT, which our code owns. The body before those locks is mostly waiting
# on competing transactions in the same test, so it is reported as the lock phase and never
# gated, like acquire, COMMIT and resolution outcomes, which measure host IO: a permanently
# red gate on a slow disk would only teach people to raise the threshold.
#
# Calibrated 2026-10-10 on arm64/macOS (debug, external APFS volume): see the summary this
# prints. With the action and budget suites added the highest share was budget.reserve at
# 357 of 1000 ms (refusals inside the lease suites, not in budget_runtime), then lease.renew
# at 310 of 2000; every action.* operation stayed under 20 ms. When it fails, shrink the operation (fewer statements, smaller locked scope)
# before raising LEASE_BODY_BUDGET_PCT, and record the reason here either way. Treat a first
# failure on new hardware as calibration, not regression.

set -uo pipefail

BUDGET_PCT="${LEASE_BODY_BUDGET_PCT:-50}"
# The lease suites proper (claims, renewals, releases, completions and recoveries), then the
# action dispatch/receipt, budget and message publish/receipt suites that run on the same
# converted bound. 04.11 added the message suites and their operations; no threshold changed.
# Deliberate-delay message tests are named workflow_message_race_ and stay out of FILTERS.
read -r -a FILTERS <<<"${LEASE_BUDGET_FILTERS:-lease_tests::workflow_io lease_tests::completion_tests lease_tests::recovery_tests lease_tests::capacity_tests workflow_action_dispatch_ workflow_action_receipt_ workflow_budget_runtime_ workflow_message_publish_ workflow_message_receipt_}"
# Every converted write and read must be measured: a filter that drifts away from an
# operation fails here instead of silently dropping its gate.
REQUIRED_OPS="${LEASE_BUDGET_REQUIRED_OPS:-lease.claim lease.renew lease.release completion action.local action.reserve_remote action.enter_remote action.commit_remote action.return_authorized budget.reserve message.publish message.consume}"
# LEASE_BUDGET_RECORD keeps the per-event JSONL for diagnosis; otherwise it is temporary.
if [[ -n "${LEASE_BUDGET_RECORD:-}" ]]; then
    record="$LEASE_BUDGET_RECORD"
    : >"$record"
else
    record=$(mktemp -t lease-budget.XXXXXX)
    trap 'rm -f "$record"' EXIT
fi

echo "lease budget: work phase must stay under ${BUDGET_PCT}% of the attempt bound"
WORKFLOW_PERSISTENCE_RECORD="$record" cargo test --locked --lib "$@" -- "${FILTERS[@]}" --test-threads=1
status=$?
if [[ $status -ne 0 ]]; then
    echo "lease budget: the measured suite failed (status $status)" >&2
    exit $status
fi

python3 -I - "$record" "$BUDGET_PCT" "$REQUIRED_OPS" <<'PY'
import json, sys
from collections import defaultdict
path, pct, required = sys.argv[1], float(sys.argv[2]), sys.argv[3].split()
ops = defaultdict(lambda: {"n": 0, "work": 0, "lock": 0, "body": 0, "bound": 0, "acquire": 0, "commit": 0, "outcomes": defaultdict(int)})
injected = 0
for line in open(path):
    e = json.loads(line)
    if e.get("injected"):
        injected += 1
        continue
    o = ops[e["op"]]
    o["n"] += 1
    o["outcomes"][e["outcome"]] += 1
    o["bound"] = max(o["bound"], e["attempt_bound_ms"])
    for key, field in (("work", "work_ms"), ("lock", "lock_ms"), ("body", "body_ms"), ("acquire", "acquire_ms"), ("commit", "commit_ms")):
        o[key] = max(o[key], e.get(field) or 0)
if not ops:
    sys.exit("lease budget: the recorder captured no operations; is the filter right?")
failed = []
print(f"{'op':28} {'n':>5} {'max work ms':>12} {'bound ms':>9} {'max lock':>9} {'max body':>9} {'max acquire':>12} {'max commit':>11}  outcomes")
for op, o in sorted(ops.items()):
    print(f"{op:28} {o['n']:>5} {o['work']:>12} {o['bound']:>9} {o['lock']:>9} {o['body']:>9} {o['acquire']:>12} {o['commit']:>11}  {dict(o['outcomes'])}")
    if o["work"] > o["bound"] * pct / 100:
        failed.append(op)
print(f"injected (excluded) events: {injected}")
missing = [op for op in required if op not in ops]
if missing:
    sys.exit(f"lease budget: no measured events for {', '.join(missing)}; is the filter right?")
if failed:
    sys.exit(f"LEASE WORK BUDGET EXCEEDED for {', '.join(failed)}: shrink the operation, or raise "
             "LEASE_BODY_BUDGET_PCT in scripts/lease-budget.sh with a recorded reason")
PY
