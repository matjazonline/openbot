#!/bin/bash
# 06-R024 corrections (06-F030/F031/F032): static gates, fresh migration, SQLx check, and focused
# database-backed tests for every changed area. Each database is a fresh disposable database on
# the retained cluster, created here and dropped by the trap; existing databases are untouched.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
logs=plan/workflow-foundation/docs/logs/06
out="$logs/06-R024fix-gates.log"
: > "$out"
step() { echo "== $1: $2" | tee -a "$out"; }
dbs=()
cleanup() { for db in "${dbs[@]}"; do dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"; done; }
trap cleanup EXIT
fresh() {
    local db="workflow06_r024fix_$1_20261021_test"
    createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || return 1
    dbs+=("$db")
    export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
    export TEST_DATABASE_URL="$DATABASE_URL"
    cargo sqlx migrate run >> "$logs/06-R024fix-migrate.log" 2>&1
}
cargo fmt --all --check && step fmt PASS || { step fmt FAIL; exit 1; }
git diff --check && step diff-check PASS || { step diff-check FAIL; exit 1; }
cargo check --locked --offline --all-targets > "$logs/06-R024fix-check.log" 2>&1 && step check PASS || { step check FAIL; exit 1; }
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06-R024fix-clippy.log" 2>&1 && step clippy PASS || { step clippy FAIL; exit 1; }
: > "$logs/06-R024fix-migrate.log"
fresh meta && step migrate PASS || { step migrate FAIL; exit 1; }
psql -X -At -d "$DATABASE_URL" -c "SELECT count(*) FROM information_schema.columns WHERE table_name = 'companies' AND column_name = 'enable_llm_spam_guardrail'" > "$logs/06-R024fix-column.txt" 2>&1
step guardrail-column-count "$(cat "$logs/06-R024fix-column.txt")"
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06-R024fix-sqlx-check.log" 2>&1 && step sqlx-check PASS || { step sqlx-check FAIL; exit 1; }
fresh focused || { step focused-db FAIL; exit 1; }
scripts/test-network-isolation.sh cargo test --locked --offline --lib -- --test-threads=6 \
    application::services::agent_runner \
    application::services::spam_scanner \
    application::services::task_worker \
    application::use_cases::agent \
    application::use_cases::company \
    application::use_cases::channel \
    application::use_cases::thread::dispatch \
    application::use_cases::thread::inter_channel_tests \
    application::use_cases::thread::external_reply_tests \
    application::use_cases::thread::tests \
    adapters::harness::rig::classifier \
    adapters::http::pages \
    adapters::http::routes::company \
    adapters::http::routes::ui_companies \
    adapters::http::routes::agent \
    adapters::persistence::company \
    adapters::smtp::server \
    infra::config \
    > "$logs/06-R024fix-focused.log" 2>&1
code=$?
step focused "exit $code; $(grep -h '^test result' "$logs/06-R024fix-focused.log" | tr '\n' ' ')"
grep -h -- '--- FAILED\|^    [a-z_:]* *$' "$logs/06-R024fix-focused.log" | head -20 | tee -a "$out"
[[ $code -eq 0 ]] && step final PASS || step final FAIL
exit $code
