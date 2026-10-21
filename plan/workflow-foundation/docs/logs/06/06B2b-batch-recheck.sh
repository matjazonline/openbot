#!/bin/bash
# Recheck after clippy fixes and owner-guard constraint parity.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all --check > "$logs/06B2b-recheck-fmt.log" 2>&1 || { echo fmt failed; exit 1; }
db=workflow06_recheck_20261020_test
createdb -h localhost -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$db"
cargo sqlx migrate run > "$logs/06B2b-recheck-migrate.log" 2>&1 || { echo migrate failed; exit 1; }
export TEST_DATABASE_URL="$DATABASE_URL"
cargo test --locked --offline --lib -- --test-threads=4 workflow_job_ workflow_agent_ workflow_harness_ adapters::harness::rig > "$logs/06B2b-recheck-tests.log" 2>&1
code=$?
grep -h '^test result' "$logs/06B2b-recheck-tests.log"
exit $code
