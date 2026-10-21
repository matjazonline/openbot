#!/bin/bash
# Reviewer B (06-R025): 06-R024 correction recheck at digest 0d8bba5d…9b00. Fresh disposable DB at
# 127.0.0.1 (dropped by trap): fresh migration + retired-column probe, clippy -D warnings, then the
# network-isolated full lib suite at --test-threads=6 (the author's setting) and the bin target.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_review_r025_test
createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"; psql -h 127.0.0.1 -p 55450 -U workflow0410 -d postgres -Atc "SELECT count(*) FROM pg_database WHERE datname LIKE '"'"'workflow06%'"'"'" > "$logs/06-review-r025-dropcheck.log" 2>&1' EXIT
export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo sqlx migrate run > "$logs/06-review-r025-migrate.log" 2>&1 || { echo "migrate failed"; exit 1; }
psql "$DATABASE_URL" -Atc "SELECT 'guardrail_columns=' || count(*) FROM information_schema.columns WHERE table_name='companies' AND column_name LIKE '%guardrail%'" >> "$logs/06-review-r025-migrate.log"
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06-review-r025-clippy.log" 2>&1; echo "clippy exit $?" >> "$logs/06-review-r025-clippy.log"
scripts/test-network-isolation.sh cargo test --locked --offline --lib -- --test-threads=6 > "$logs/06-review-r025-suite.log" 2>&1
code=$?
scripts/test-network-isolation.sh cargo test --locked --offline --bins >> "$logs/06-review-r025-suite.log" 2>&1 || code=$?
uptime >> "$logs/06-review-r025-suite.log"
tail -2 "$logs/06-review-r025-migrate.log"; tail -1 "$logs/06-review-r025-clippy.log"
grep -h '^test result' "$logs/06-review-r025-suite.log"
exit $code
