#!/bin/bash
# Phase06 final full verification gate (mirrors CI): static checks, fresh migration + SQLx,
# network-isolated all-target suite, stock-stack budget, lease budget. Each database suite gets
# its own fresh disposable database on the retained cluster; existing databases are untouched.
set -uo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
logs=plan/workflow-foundation/docs/logs/06
out="$logs/06-final-gate.log"
: > "$out"
step() { echo "== $1: $2" | tee -a "$out"; }
dbs=()
cleanup() { for db in "${dbs[@]}"; do dropdb -h 127.0.0.1 -p 55450 -U workflow0410 --if-exists "$db"; done; }
trap cleanup EXIT
fresh() {
    local db="workflow06_final_$1_20261020_test"
    createdb -h 127.0.0.1 -p 55450 -U workflow0410 "$db" || return 1
    dbs+=("$db")
    export DATABASE_URL="postgres://workflow0410@127.0.0.1:55450/$db"
    export TEST_DATABASE_URL="$DATABASE_URL"
    cargo sqlx migrate run >> "$logs/06-final-migrate.log" 2>&1
}
cargo fmt --all --check && step fmt PASS || { step fmt FAIL; exit 1; }
git diff --check && step diff-check PASS || { step diff-check FAIL; exit 1; }
cargo check --locked --offline --all-targets > "$logs/06-final-check.log" 2>&1 && step check PASS || { step check FAIL; exit 1; }
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06-final-clippy.log" 2>&1 && step clippy PASS || { step clippy FAIL; exit 1; }
: > "$logs/06-final-migrate.log"
fresh meta && step migrate PASS || { step migrate FAIL; exit 1; }
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06-final-sqlx-check.log" 2>&1 && step sqlx-check PASS || { step sqlx-check FAIL; exit 1; }
cargo test --locked --offline --all-targets --no-run > "$logs/06-final-build-tests.log" 2>&1 && step build-tests PASS || { step build-tests FAIL; exit 1; }
fresh suite || { step suite-db FAIL; exit 1; }
scripts/test-network-isolation.sh cargo test --locked --offline --all-targets > "$logs/06-final-suite.log" 2>&1; code=$?
step suite "exit $code; $(grep -h '^test result' "$logs/06-final-suite.log" | tr '\n' ' ')"
fresh stack || { step stack-db FAIL; exit 1; }
scripts/test-network-isolation.sh scripts/stack-budget.sh --offline > "$logs/06-final-stack-budget.log" 2>&1; scode=$?
step stack-budget "exit $scode; $(grep -h '^test result' "$logs/06-final-stack-budget.log" | tr '\n' ' ')"
fresh lease || { step lease-db FAIL; exit 1; }
scripts/test-network-isolation.sh scripts/lease-budget.sh --offline > "$logs/06-final-lease-budget.log" 2>&1; lcode=$?
step lease-budget "exit $lcode; $(tail -3 "$logs/06-final-lease-budget.log" | tr '\n' ' ')"
[[ $code -eq 0 && $scode -eq 0 && $lcode -eq 0 ]] && step final PASS || step final FAIL
