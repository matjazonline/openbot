#!/bin/bash
# Reviewer B (06-R024): independent network-isolated all-target suite at the frozen phase06 digest
# 33cfebc7…4bf0, host 127.0.0.1, fresh disposable DB dropped by trap; then the two
# action_receipt resolution timing tests 3x in isolation on the same DB.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_review_r024_test
createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"; psql -h 127.0.0.1 -p 55450 -U workflow0410 -d postgres -Atc "SELECT count(*) FROM pg_database WHERE datname = '"'"'$db'"'"'" > "$logs/06-review-r024-dropcheck.log" 2>&1' EXIT
export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo sqlx migrate run > "$logs/06-review-r024-migrate.log" 2>&1 || { echo "migrate failed"; exit 1; }
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets > "$logs/06-review-r024-suite.log" 2>&1
code=$?
: > "$logs/06-review-r024-timing.log"
for i in 1 2 3; do
  scripts/test-network-isolation.sh cargo test --locked --offline --lib -- action_receipt_tests::resolution_tests >> "$logs/06-review-r024-timing.log" 2>&1
  echo "run $i exit $?" >> "$logs/06-review-r024-timing.log"
done
grep -h '^test result' "$logs/06-review-r024-suite.log"
grep -h '^test result\|^run ' "$logs/06-review-r024-timing.log"
exit $code
