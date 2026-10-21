#!/bin/bash
# Reviewer B (06-R022) independent full lib suite at the frozen 06C digest, fresh disposable DB.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_review_r022_test
createdb -h localhost -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$db"
cargo sqlx migrate run > "$logs/06C-review-r022-migrate.log" 2>&1 || { echo "migrate failed"; exit 1; }
psql "$DATABASE_URL" -Atc "SELECT count(*) FROM information_schema.columns WHERE table_name='agents' AND column_name IN ('harness_kind','granted_tool_ids','native_tool_policy')" >> "$logs/06C-review-r022-migrate.log"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo test --locked --offline --lib -- --test-threads=4 > "$logs/06C-review-r022-suite.log" 2>&1
code=$?
grep -h '^test result' "$logs/06C-review-r022-suite.log"
exit $code
