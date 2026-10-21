#!/bin/bash
# 06B2b batch gates: static, fresh migration, SQLx, broad workflow/harness/thread regressions.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
run() { local name=$1; shift; "$@" > "$logs/06B2b-batch-$name.log" 2>&1; local code=$?; echo "$name exit=$code $(grep -h '^test result' "$logs/06B2b-batch-$name.log" | tail -1)"; return $code; }
status=0
run fmt cargo fmt --all --check || status=1
run diffcheck git diff --check || status=1
run check cargo check --locked --offline --all-targets || status=1
run clippy cargo clippy --locked --offline --all-targets -- -D warnings || status=1
db=workflow06_batch_20261020_test
createdb -h localhost -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$db"
run migrate cargo sqlx migrate run || status=1
run sqlx-prepare env SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets || status=1
run sqlx-check env SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets || status=1
git status --short .sqlx > "$logs/06B2b-batch-sqlx-diff.log"
export TEST_DATABASE_URL="$DATABASE_URL"
for filter in workflow_ adapters::harness::rig application::use_cases::thread adapters::persistence::task::harness_runs model_usage application::workflow; do
  name=$(echo "$filter" | tr ':' '_')
  run "tests-$name" cargo test --locked --offline --lib "$filter" -- --test-threads=4 || status=1
done
exit $status
