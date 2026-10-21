#!/bin/bash
# D5 focused suites on a fresh disposable database (retained DB predates the init edits).
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_d5_focused_20261020_test
createdb -h localhost -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$db"
cargo sqlx migrate run > "$logs/06B2b-d5-migrate.log" 2>&1 || { echo "migrate failed"; exit 1; }
export TEST_DATABASE_URL="$DATABASE_URL"
status=0
for filter in application::workflow::compiler application::workflow::registry application::workflow::publication application::use_cases::agent application::use_cases::skill adapters::persistence::skill adapters::persistence::agent workflow_harness_ workflow_agent_ workflow_budget_; do
  name=$(echo "$filter" | tr ':' '_')
  cargo test --locked --offline --lib "$filter" -- --test-threads=4 > "$logs/06B2b-d5-$name.log" 2>&1
  code=$?
  echo "$filter exit=$code $(grep -h '^test result' "$logs/06B2b-d5-$name.log" | tail -1)"
  [ $code -ne 0 ] && status=1
done
exit $status
