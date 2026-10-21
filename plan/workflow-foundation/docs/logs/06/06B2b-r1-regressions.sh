#!/bin/bash
# Regressions for the shared Rig driver binding refactor and native store authority check.
set -uo pipefail
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
logs=plan/workflow-foundation/docs/logs/06
status=0
for filter in adapters::harness::rig application::use_cases::thread adapters::persistence::task::harness_runs workflow_harness_ model_usage; do
  name=$(echo "$filter" | tr ':' '_')
  cargo test --locked --offline --lib "$filter" -- --test-threads=4 > "$logs/06B2b-r1-$name.log" 2>&1
  code=$?
  echo "$filter exit=$code $(grep -h '^test result' "$logs/06B2b-r1-$name.log" | tail -1)"
  [ $code -ne 0 ] && status=1
done
exit $status
