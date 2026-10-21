#!/bin/bash
# Reviewer B (06-R024): characterize the two lease/dispatch-family failures of
# 06-review-r024-suite.log (run overlapped B's Playwright/CSS/node checks): each 3x in isolation,
# then one full all-target suite with no concurrent B activity. Fresh disposable DB, 127.0.0.1.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_review_r024b_test
createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"; psql -h 127.0.0.1 -p 55450 -U workflow0410 -d postgres -Atc "SELECT count(*) FROM pg_database WHERE datname LIKE '"'"'workflow06%'"'"'" > "$logs/06-review-r024b-dropcheck.log" 2>&1' EXIT
export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
export TEST_DATABASE_URL="$DATABASE_URL"
cargo sqlx migrate run > /dev/null 2>&1 || { echo "migrate failed"; exit 1; }
: > "$logs/06-review-r024b-focused.log"
for i in 1 2 3; do
  scripts/test-network-isolation.sh cargo test --locked --offline --lib -- --exact \
    adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_reconciliation_receipt_tests::audit_tests::workflow_action_audit_sql_orphan_immutability_and_other_kind_policy \
    adapters::persistence::workflow::tests::admission_tests::lease_tests::action_dispatch_tests::action_receipt_tests::uncertainty_tests::workflow_action_uncertainty_pending_failures_consult_expired_action_proof \
    >> "$logs/06-review-r024b-focused.log" 2>&1
  echo "run $i exit $?" >> "$logs/06-review-r024b-focused.log"
done
uptime >> "$logs/06-review-r024b-focused.log"
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets > "$logs/06-review-r024b-suite.log" 2>&1
code=$?
uptime >> "$logs/06-review-r024b-suite.log"
grep -h '^test result\|^run ' "$logs/06-review-r024b-focused.log"
grep -h '^test result' "$logs/06-review-r024b-suite.log"
exit $code
