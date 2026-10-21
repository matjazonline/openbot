#!/bin/bash
# Reviewer B (06-R024): every distinct test that failed in any phase06 final-gate or B full run
# (06-final-suite-rerun1-localhost, 06-review-r024-suite, 06-review-r024b-suite; 17 tests), run
# together 3x by exact name on a fresh disposable DB at 127.0.0.1. Host note: an unrelated
# project's cargo test was running concurrently (external load).
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
names=$1
db=workflow06_review_r024c_test
createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"; psql -h 127.0.0.1 -p 55450 -U workflow0410 -d postgres -Atc "SELECT count(*) FROM pg_database WHERE datname LIKE '"'"'workflow06%'"'"'" > "$logs/06-review-r024c-dropcheck.log" 2>&1' EXIT
export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo sqlx migrate run > /dev/null 2>&1 || { echo "migrate failed"; exit 1; }
: > "$logs/06-review-r024c-focused.log"
for i in 1 2 3; do
  uptime >> "$logs/06-review-r024c-focused.log"
  scripts/test-network-isolation.sh cargo test --locked --offline --lib -- --exact $(cat "$names") >> "$logs/06-review-r024c-focused.log" 2>&1
  echo "run $i exit $?" >> "$logs/06-review-r024c-focused.log"
done
grep -h '^test result\|^run ' "$logs/06-review-r024c-focused.log"
