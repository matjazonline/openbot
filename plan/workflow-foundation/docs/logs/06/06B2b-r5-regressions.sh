#!/bin/bash
# R5 delivery-wait continuation regressions (affected SQL guards, waits, decisions, actions).
set -uo pipefail
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
logs=plan/workflow-foundation/docs/logs/06
status=0
for filter in workflow_message workflow_wait_ workflow_decision_ workflow_action_ workflow_completion_ workflow_batch_ workflow_poison_ workflow_harness_ workflow_agent_; do
  cargo test --locked --offline --lib "$filter" -- --test-threads=4 > "$logs/06B2b-r5-$filter.log" 2>&1
  code=$?
  echo "$filter exit=$code $(grep -h '^test result' "$logs/06B2b-r5-$filter.log" | tail -1)"
  [ $code -ne 0 ] && status=1
done
exit $status
