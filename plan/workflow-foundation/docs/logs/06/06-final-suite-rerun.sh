#!/bin/bash
# Final gate: rerun of the network-isolated all-target suite after one connection-level failure
# (06-final-suite.log). Fresh disposable DB, dropped by trap.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_final_suite2_20261020_test
createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo sqlx migrate run > /dev/null 2>&1 || exit 1
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets > "$logs/06-final-suite-rerun.log" 2>&1
code=$?
grep -h '^test result' "$logs/06-final-suite-rerun.log"
exit $code
