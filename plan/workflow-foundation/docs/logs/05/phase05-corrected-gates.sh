#!/usr/bin/env bash
# Final phase05 gate; run only after 05B acceptance.
# Every gate runs and records PASS/FAIL; a failure is diagnosed, never hidden.
set -uo pipefail
cd /Users/mac03/dev/openbot
export PATH="/opt/homebrew/opt/postgresql@18/bin:$PATH"
export TEST_DATABASE_URL=postgres://workflow0410@127.0.0.1:55450/workflow0410_test
export DATABASE_URL="$TEST_DATABASE_URL"
export CARGO_BUILD_JOBS=4
export CARGO_INCREMENTAL=0
logdir=plan/workflow-foundation/docs/logs/05
status="$logdir/phase05-corrected-gate-status.log"
fresh=postgres://workflow0410@127.0.0.1:55450/workflow0410_fresh_test
run_gate() {
  local name="$1"
  shift
  printf '%s START %s\n' "$(date -u +%FT%TZ)" "$name" >> "$status"
  if "$@" > "$logdir/phase05-corrected-gate-$name.log" 2>&1; then
    printf '%s PASS %s\n' "$(date -u +%FT%TZ)" "$name" >> "$status"
  else
    local code=$?
    printf '%s FAIL %s status=%s\n' "$(date -u +%FT%TZ)" "$name" "$code" >> "$status"
  fi
}
fresh_schema() {
  psql -h 127.0.0.1 -p 55450 -U workflow0410 -d postgres -X -q \
    -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname='workflow0410_fresh_test' AND pid<>pg_backend_pid()" \
    -c "DROP DATABASE IF EXISTS workflow0410_fresh_test" \
    -c "CREATE DATABASE workflow0410_fresh_test OWNER workflow0410" || return 1
  SQLX_OFFLINE=false DATABASE_URL="$fresh" cargo sqlx migrate run || return 1
  local normalize='/^\\(un)?restrict /d; /^-- Dumped (from|by) /d'
  pg_dump -s "$fresh" | sed -E "$normalize" > "$logdir/phase05-corrected-fresh-schema.sql" || return 1
  pg_dump -s "$TEST_DATABASE_URL" | sed -E "$normalize" > "$logdir/phase05-corrected-fixture-schema.sql" || return 1
  diff -u "$logdir/phase05-corrected-fixture-schema.sql" "$logdir/phase05-corrected-fresh-schema.sql"
}
transport() { scripts/tests/transport-boundary.sh && scripts/transport-boundary-check.sh; }
scripts_cli() { scripts/tests/credential-key-rotation.sh && scripts/tests/deploy.sh; }
: > "$status"
run_gate fmt cargo fmt --all -- --check
run_gate diff git diff --check
run_gate offline env SQLX_OFFLINE=true cargo check --locked --offline --all-targets
run_gate clippy env SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings
run_gate sqlx-prepare env SQLX_OFFLINE=false cargo sqlx prepare -- --locked --offline --all-targets
run_gate sqlx-check env SQLX_OFFLINE=false cargo sqlx prepare --check -- --locked --offline --all-targets
run_gate no-run env SQLX_OFFLINE=true cargo test --locked --offline --all-targets --no-run
run_gate full-stock-suite env RUST_MIN_STACK=2097152 SQLX_OFFLINE=true scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
run_gate lease-budget env SQLX_OFFLINE=true LEASE_BUDGET_RECORD="$logdir/phase05-corrected-lease-record.jsonl" scripts/test-network-isolation.sh bash scripts/lease-budget.sh --offline
run_gate graph graft build
cat "$status"
