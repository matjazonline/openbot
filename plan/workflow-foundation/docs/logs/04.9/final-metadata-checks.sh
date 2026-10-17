#!/usr/bin/env bash
set -euo pipefail
cd /home/mh/dev/openbot
export TEST_DATABASE_URL=postgres://postgres:workflow049@127.0.0.1:55449/workflow049_test
export DATABASE_URL="$TEST_DATABASE_URL"
export CARGO_BUILD_JOBS=1
logdir=plan/workflow-foundation/docs/logs/04.9
run_gate() {
  local name="$1"
  shift
  printf '%s START %s\n' "$(date -u +%FT%TZ)" "$name" >> "$logdir/resumed-gate-status.log"
  if "$@" > "$logdir/resumed-$name.log" 2>&1; then
    printf '%s PASS %s\n' "$(date -u +%FT%TZ)" "$name" >> "$logdir/resumed-gate-status.log"
  else
    local status=$?
    printf '%s FAIL %s status=%s\n' "$(date -u +%FT%TZ)" "$name" "$status" >> "$logdir/resumed-gate-status.log"
    return "$status"
  fi
}
run_gate final-sqlx-prepare env SQLX_OFFLINE=false cargo sqlx prepare -- --locked --offline --all-targets
run_gate final-sqlx-check env SQLX_OFFLINE=false cargo sqlx prepare --check -- --locked --offline --all-targets
run_gate final-clippy env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
run_gate final-static scripts/transport-boundary-check.sh
