#!/usr/bin/env bash
set -euo pipefail
cd /Users/mac03/dev/openbot
export PATH="/opt/homebrew/opt/postgresql@18/bin:$PATH"
export TEST_DATABASE_URL=postgres://workflow049@127.0.0.1:55449/workflow049_test
export DATABASE_URL="$TEST_DATABASE_URL"
export CARGO_BUILD_JOBS=2
logdir=plan/workflow-foundation/docs/logs/04.9
run_gate() {
  local name="$1"
  shift
  printf '%s START %s\n' "$(date -u +%FT%TZ)" "$name" >> "$logdir/crash-gate-status.log"
  if "$@" > "$logdir/crash-$name.log" 2>&1; then
    printf '%s PASS %s\n' "$(date -u +%FT%TZ)" "$name" >> "$logdir/crash-gate-status.log"
  else
    local status=$?
    printf '%s FAIL %s status=%s\n' "$(date -u +%FT%TZ)" "$name" "$status" >> "$logdir/crash-gate-status.log"
    return "$status"
  fi
}
run_gate migrations cargo sqlx migrate run
run_gate schema psql "$DATABASE_URL" -X -c '\d workflow_http_connections' -c '\d workflow_http_credentials' -c '\d workflow_http_operations'
run_gate http env SQLX_OFFLINE=true cargo test --locked --offline --lib workflow_http_
run_gate credentials env SQLX_OFFLINE=true cargo test --locked --offline --lib credential_rotation
run_gate sqlx-prepare env SQLX_OFFLINE=false cargo sqlx prepare -- --locked --offline --all-targets
run_gate sqlx-check env SQLX_OFFLINE=false cargo sqlx prepare --check -- --locked --offline --all-targets
run_gate offline env SQLX_OFFLINE=true cargo check --locked --offline --all-targets
run_gate clippy env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
run_gate fmt cargo fmt --all -- --check
run_gate diff git diff --check
run_gate transport scripts/transport-boundary-check.sh
run_gate transport-regression scripts/tests/transport-boundary.sh
run_gate credential-cli scripts/tests/credential-key-rotation.sh
run_gate full-tests env SQLX_OFFLINE=true scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
run_gate stack env SQLX_OFFLINE=true scripts/stack-budget.sh --offline
run_gate lease env SQLX_OFFLINE=true LEASE_BUDGET_RECORD="$logdir/crash-lease.jsonl" scripts/lease-budget.sh --offline
run_gate graph graft build
