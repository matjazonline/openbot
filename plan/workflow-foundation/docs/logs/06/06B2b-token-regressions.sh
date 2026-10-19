#!/bin/bash
set -euo pipefail
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
export RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
for filter in workflow_action_dispatch_ workflow_budget_activation_ workflow_action_uncertainty_ workflow_completion_ workflow_decision_; do
    cargo test --locked --offline --lib "$filter" -- --test-threads=4 > "$logs/06B2b-token-regression-$filter.log" 2>&1
done
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B2b-token-clippy.log" 2>&1
