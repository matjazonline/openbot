#!/bin/bash
# Reviewer B (06-R020) independent focused run on a fresh disposable database.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06_review_r020_test
createdb -h localhost -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$db"
cargo sqlx migrate run > "$logs/06B3-review-r020-migrate.log" 2>&1 || { echo "migrate failed"; exit 1; }
export TEST_DATABASE_URL="$DATABASE_URL"
cargo test --locked --offline --lib -- --test-threads=4 workflow_context_history workflow_classify workflow_decision_agent workflow_model_steps workflow_agent_ workflow_effect_free application::workflow::history application::workflow::agent application::workflow::registry application::workflow::publication > "$logs/06B3-review-r020-focused.log" 2>&1
code=$?
grep -h '^test result' "$logs/06B3-review-r020-focused.log"
exit $code
