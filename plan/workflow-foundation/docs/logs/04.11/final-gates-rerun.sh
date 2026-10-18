#!/usr/bin/env bash
# 04.11 final-gate rerun after diagnosis: the fixture now carries CI's pg_stat_statements
# settings (logs/04.11/fixture-ci-stat-settings.log); the full suite and stack budget rerun.
set -uo pipefail
cd /Users/mac03/dev/openbot
export PATH="/opt/homebrew/opt/postgresql@18/bin:$PATH"
export TEST_DATABASE_URL=postgres://workflow0410@127.0.0.1:55450/workflow0410_test
export DATABASE_URL="$TEST_DATABASE_URL"
export CARGO_BUILD_JOBS=4
logdir=plan/workflow-foundation/docs/logs/04.11
status="$logdir/final-gate-rerun-status.log"
run_gate() {
  local name="$1"
  shift
  printf '%s START %s\n' "$(date -u +%FT%TZ)" "$name" >> "$status"
  if "$@" > "$logdir/final-gate-$name.log" 2>&1; then
    printf '%s PASS %s\n' "$(date -u +%FT%TZ)" "$name" >> "$status"
  else
    local code=$?
    printf '%s FAIL %s status=%s\n' "$(date -u +%FT%TZ)" "$name" "$code" >> "$status"
  fi
}
ci_settings() {
  test "$(psql "$DATABASE_URL" -X -Atqc "SHOW pg_stat_statements.track")" = "top" &&
  test "$(psql "$DATABASE_URL" -X -Atqc "SHOW pg_stat_statements.track_utility")" = "off" &&
  test "$(psql "$DATABASE_URL" -X -Atqc "SHOW log_parameter_max_length")" = "0" &&
  test "$(psql "$DATABASE_URL" -X -Atqc "SHOW log_parameter_max_length_on_error")" = "0"
}
: > "$status"
run_gate ci-stat-settings ci_settings
run_gate full-suite-rerun env SQLX_OFFLINE=true scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
run_gate stack-budget-rerun env SQLX_OFFLINE=true scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
cat "$status"
