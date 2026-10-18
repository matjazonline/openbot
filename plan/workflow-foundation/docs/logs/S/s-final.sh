#!/usr/bin/env bash
# Batch S final run on HEAD feefe3e plus the user-approved AGENTS/startup-log edits:
# the network-isolated full suite, then (only if it passes) the stack budget.
# Load and memory_pressure free % are recorded at the start and end of each run.
set -uo pipefail
cd /Users/mac03/dev/openbot
export PATH="/opt/homebrew/opt/postgresql@18/bin:$PATH"
export TEST_DATABASE_URL=postgres://workflow0410@127.0.0.1:55450/workflow0410_test
export DATABASE_URL="$TEST_DATABASE_URL"
export CARGO_BUILD_JOBS=4
export SQLX_OFFLINE=true
logdir=plan/workflow-foundation/docs/logs/S
status="$logdir/s-final-status.log"
host() {
  printf '%s %s load=%s %s swap=%s\n' "$(date -u +%FT%TZ)" "$1" "$(sysctl -n vm.loadavg)" \
    "$(memory_pressure | tail -1)" "$(sysctl -n vm.swapusage)" >> "$status"
}
: > "$status"
host "START full-suite"
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets \
  > "$logdir/s-final-full-suite.log" 2>&1
suite=$?
host "END full-suite exit=$suite"
if [ "$suite" -eq 0 ]; then
  host "START stack-budget"
  scripts/test-network-isolation.sh scripts/stack-budget.sh --offline \
    > "$logdir/s-final-stack-budget.log" 2>&1
  stack=$?
  host "END stack-budget exit=$stack"
fi
cat "$status"
