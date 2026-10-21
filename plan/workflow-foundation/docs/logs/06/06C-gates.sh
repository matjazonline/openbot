#!/bin/bash
# 06C static/schema/SQLx gates (legacy catalogue + ai-agents removal). Disposable DB only;
# the retained cluster databases are never touched.
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all --check
git diff --check
cargo check --locked --offline --all-targets > "$logs/06C-check.log" 2>&1
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06C-clippy.log" 2>&1
meta_database=workflow06c_meta_20261020_test
createdb -h localhost -p 55450 -U workflow0410 "$meta_database"
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$meta_database"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$meta_database"
cargo sqlx migrate run > "$logs/06C-migrate.log" 2>&1
psql "$DATABASE_URL" -Atc "SELECT count(*) FROM information_schema.columns WHERE table_name='agents' AND column_name IN ('harness_kind','granted_tool_ids','native_tool_policy')" >> "$logs/06C-migrate.log"
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06C-sqlx-prepare.log" 2>&1
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06C-sqlx-check.log" 2>&1
git status --short .sqlx
echo GATES-PASS
