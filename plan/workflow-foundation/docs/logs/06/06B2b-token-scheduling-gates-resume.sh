#!/bin/bash
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all --check
git diff --check
for filter in workflow_budget_runtime_ workflow_action_uncertainty_; do
  cargo test --locked --offline --lib "$filter" -- --test-threads=4 > "$logs/06B2b-scheduling-$filter.log" 2>&1
done
cargo check --locked --offline --all-targets > "$logs/06B2b-scheduling-check.log" 2>&1
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B2b-scheduling-clippy.log" 2>&1
meta_database=workflow06_token_scheduling_20261019_test
createdb -h localhost -p 55450 -U workflow0410 "$meta_database"
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$meta_database"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$meta_database"
cargo sqlx migrate run > "$logs/06B2b-scheduling-migrate.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06B2b-scheduling-sqlx-prepare.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06B2b-scheduling-sqlx-check.log" 2>&1
