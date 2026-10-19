#!/bin/bash
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
export RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all --check
git diff --check
cargo test --locked --offline --lib workflow_harness_ -- --test-threads=4 > "$logs/06B2b-model-owner-tests-final.log" 2>&1
cargo check --locked --offline --all-targets > "$logs/06B2b-model-owner-check.log" 2>&1
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B2b-model-owner-clippy.log" 2>&1
# Create a new task-only metadata database; creation must fail if already present.
meta_database=workflow06_model_owner_20261019_test
createdb -h localhost -p 55450 -U workflow0410 "$meta_database"
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$meta_database"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$meta_database"
cargo sqlx migrate run > "$logs/06B2b-model-owner-migrate.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06B2b-model-owner-sqlx-prepare.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06B2b-model-owner-sqlx-check.log" 2>&1
printf 'PASS model ownership segment gates\n'
