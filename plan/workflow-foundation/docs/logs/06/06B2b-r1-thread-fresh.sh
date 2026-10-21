#!/bin/bash
# Thread suite against a fresh disposable database (retained workflow0410_test predates the init edit).
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_r1_thread_20261019_test
createdb -h localhost -p 55450 -U workflow0410 "$db"
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$db"
cargo sqlx migrate run > "$logs/06B2b-r1-thread-fresh-migrate.log" 2>&1
TEST_DATABASE_URL="$DATABASE_URL" cargo test --locked --offline --lib application::use_cases::thread -- --test-threads=4 > "$logs/06B2b-r1-thread-fresh.log" 2>&1
grep '^test result' "$logs/06B2b-r1-thread-fresh.log"
