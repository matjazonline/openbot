#!/bin/bash
# Characterize the single final-suite failure (connection-level SSLRequest protocol error in
# own_database) by rerunning its module 3x under the same network isolation on a fresh DB.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_final_flake_20261020_test
createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo sqlx migrate run > /dev/null 2>&1 || exit 1
: > "$logs/06-final-flake-check.log"
for i in 1 2 3; do
  scripts/test-network-isolation.sh cargo test --locked --offline --lib -- message_tests::agent_tests >> "$logs/06-final-flake-check.log" 2>&1
  echo "run $i exit $?" >> "$logs/06-final-flake-check.log"
done
grep -h '^test result\|^run ' "$logs/06-final-flake-check.log"
