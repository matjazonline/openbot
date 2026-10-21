#!/bin/bash
# Static/schema/SQLx refresh for the native checkpoint-update delta (HEAD 76fe99c).
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all --check
git diff --check
cargo check --locked --offline --all-targets > "$logs/06B2b-native-updates-check.log" 2>&1
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B2b-native-updates-clippy.log" 2>&1
meta_database=workflow06_native_updates_20261019_test
createdb -h localhost -p 55450 -U workflow0410 "$meta_database"
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$meta_database"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$meta_database"
cargo sqlx migrate run > "$logs/06B2b-native-updates-migrate.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06B2b-native-updates-sqlx-prepare.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06B2b-native-updates-sqlx-check.log" 2>&1
git status --short .sqlx
echo GATES-PASS
