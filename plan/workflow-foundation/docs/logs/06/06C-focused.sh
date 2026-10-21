#!/bin/bash
# 06C focused rerun of the first full-suite failures and their modules (disposable DB).
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logs=plan/workflow-foundation/docs/logs/06
db=workflow06c_focused_20261020_test
createdb -h localhost -p 55450 -U workflow0410 "$db" || exit 1
trap 'dropdb -h localhost -p 55450 -U workflow0410 "$db"' EXIT
export DATABASE_URL="postgres://workflow0410@localhost:55450/$db"
cargo sqlx migrate run > "$logs/06C-focused-migrate.log" 2>&1 || { echo "migrate failed"; exit 1; }
export TEST_DATABASE_URL="$DATABASE_URL"
cargo test --locked --offline --lib -- --test-threads=4 adapters::http::routes::agent adapters::persistence::skill adapters::persistence::task::claim_tests application::services::agent_runner application::services::task_worker application::transport::dependency_tests application::use_cases::agent application::use_cases::thread::inter_channel_tests adapters::persistence::agent_config_tests > "$logs/06C-focused.log" 2>&1
code=$?
grep -h '^test result' "$logs/06C-focused.log"
grep -h 'FAILED' "$logs/06C-focused.log" | head -20
exit $code
