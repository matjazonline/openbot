#!/bin/bash
set -euo pipefail
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
export RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
for filter in rig_execution_tests structured_response_tests model_usage; do
  cargo test --locked --offline --lib "$filter" > "$logs/06B1-correction-$filter.log" 2>&1
  echo "PASS $filter"
done
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B1-correction-clippy.log" 2>&1
echo 'PASS clippy'
cargo check --locked --offline --all-targets > "$logs/06B1-correction-check.log" 2>&1
echo 'PASS offline check'
DATABASE_URL="$TEST_DATABASE_URL" SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06B1-correction-sqlx.log" 2>&1
DATABASE_URL="$TEST_DATABASE_URL" SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06B1-correction-sqlx-check.log" 2>&1
echo 'PASS sqlx'
cargo fmt --all --check
git diff --check
graft build > "$logs/06B1-correction-graft.log" 2>&1
echo 'PASS format/diff/graph'
