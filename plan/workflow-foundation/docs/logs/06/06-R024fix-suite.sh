#!/bin/bash
# 06-R024 corrections: one network-isolated full library suite on a fresh disposable database
# (dropped by the trap), then the node frontend tests.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_r024fix_suite_20261021_test
createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo sqlx migrate run > /dev/null 2>&1 || { echo "migrate failed"; exit 1; }
scripts/test-network-isolation.sh cargo test --locked --offline --lib -- --test-threads=6 \
    > "$logs/06-R024fix-suite.log" 2>&1
code=$?
echo "suite exit $code"
grep -h '^test result' "$logs/06-R024fix-suite.log"
grep -h -A40 '^failures:$' "$logs/06-R024fix-suite.log" | grep -E '^    [a-zA-Z_:0-9]+$' | sort -u
node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs \
    > "$logs/06-R024fix-node.log" 2>&1
ncode=$?
echo "node exit $ncode"
grep -E '^# (tests|pass|fail)' "$logs/06-R024fix-node.log"
exit $(( code | ncode ))
