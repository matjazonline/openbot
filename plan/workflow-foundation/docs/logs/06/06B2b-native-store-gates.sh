#!/bin/bash
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
export RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all --check
git diff --check
cargo test --locked --offline --lib adapters::persistence::task::harness_runs::tests -- --test-threads=4 > "$logs/06B2b-native-current-callers.log" 2>&1
cargo check --locked --offline --all-targets > "$logs/06B2b-native-store-check-2.log" 2>&1
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B2b-native-store-clippy.log" 2>&1
# Only this continuation's disposable metadata DB; preserve all retained evidence databases.
meta_database=workflow06_native_owner_20261019_test
dropdb -h localhost -p 55450 -U workflow0410 "$meta_database"
createdb -h localhost -p 55450 -U workflow0410 "$meta_database"
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$meta_database"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$meta_database"
cargo sqlx migrate run > "$logs/06B2b-native-store-migrate.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06B2b-native-store-sqlx-prepare.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06B2b-native-store-sqlx-check.log" 2>&1
printf 'PASS native store segment gates\n'
