#!/bin/bash
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all --check
 git diff --check
SQLX_OFFLINE=true cargo check --locked --offline --all-targets > "$logs/06B2b-admission-check.log" 2>&1
SQLX_OFFLINE=true cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B2b-admission-clippy.log" 2>&1
meta_database=workflow06_admission_20261019_test
createdb -h localhost -p 55450 -U workflow0410 "$meta_database"
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$meta_database"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$meta_database"
cargo sqlx migrate run > "$logs/06B2b-admission-migrate.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06B2b-admission-sqlx-prepare.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06B2b-admission-sqlx-check.log" 2>&1
printf 'PASS admission static/migration/SQLx gates\n'
