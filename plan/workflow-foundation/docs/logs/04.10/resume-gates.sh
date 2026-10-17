#!/usr/bin/env bash
set -euo pipefail
cd /Users/mac03/dev/openbot
export PATH="/opt/homebrew/opt/postgresql@18/bin:$PATH"
export TEST_DATABASE_URL=postgres://workflow0410@127.0.0.1:55450/workflow0410_test
export DATABASE_URL="$TEST_DATABASE_URL"
export CARGO_BUILD_JOBS=2
logdir=plan/workflow-foundation/docs/logs/04.10
run_gate() {
  local name="$1"
  shift
  printf '%s START %s\n' "$(date -u +%FT%TZ)" "$name" >> "$logdir/resume-gate-status.log"
  if "$@" > "$logdir/resume-gate-$name.log" 2>&1; then
    printf '%s PASS %s\n' "$(date -u +%FT%TZ)" "$name" >> "$logdir/resume-gate-status.log"
  else
    local status=$?
    printf '%s FAIL %s status=%s\n' "$(date -u +%FT%TZ)" "$name" "$status" >> "$logdir/resume-gate-status.log"
    return "$status"
  fi
}
run_gate stock env SQLX_OFFLINE=true scripts/stack-budget.sh --offline -- workflow_action workflow_mcp mcp
run_gate sqlx-prepare env SQLX_OFFLINE=false cargo sqlx prepare -- --locked --offline --all-targets
run_gate sqlx-check env SQLX_OFFLINE=false cargo sqlx prepare --check -- --locked --offline --all-targets
run_gate offline env SQLX_OFFLINE=true cargo check --locked --offline --all-targets
run_gate clippy env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
run_gate fmt cargo fmt --all -- --check
run_gate diff git diff --check
run_gate transport scripts/transport-boundary-check.sh
run_gate transport-regression scripts/tests/transport-boundary.sh
run_gate credential-cli scripts/tests/credential-key-rotation.sh
run_gate graph graft build
