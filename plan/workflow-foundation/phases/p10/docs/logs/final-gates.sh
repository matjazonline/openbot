#!/usr/bin/env bash
set -euo pipefail
cd /Users/mac02/dev/openbot
export SQLX_OFFLINE=true CARGO_INCREMENTAL=0
export DATABASE_URL=postgres://workflow10@127.0.0.1:55500/workflow10
export TEST_DATABASE_URL=postgres://workflow10@127.0.0.1:55500/workflow10_test
logs=plan/workflow-foundation/phases/p10/docs/logs
run_gate() {
    local name="$1"
    shift
    echo "START $name $(date -u +%FT%TZ)"
    if "$@" > "$logs/final-$name.log" 2>&1; then
        echo "PASS $name $(date -u +%FT%TZ)"
    else
        echo "FAIL $name $(date -u +%FT%TZ)"
        tail -60 "$logs/final-$name.log"
        return 1
    fi
}
run_gate fmt cargo fmt --all -- --check
run_gate whitespace git diff --check
run_gate css npm run check:css
run_gate check cargo check --locked --offline --all-targets
run_gate clippy cargo clippy --locked --offline --all-targets -- -D warnings
run_gate migrate env SQLX_OFFLINE=false cargo sqlx migrate run
run_gate sqlx-prepare env SQLX_OFFLINE=false cargo sqlx prepare -- --locked --offline --all-targets
run_gate sqlx-check env SQLX_OFFLINE=false cargo sqlx prepare --check -- --locked --offline --all-targets
run_gate compile-tests cargo test --locked --offline --all-targets --no-run
run_gate tests scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
run_gate stack scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
run_gate lease scripts/test-network-isolation.sh scripts/lease-budget.sh --offline
export WORKFLOW_DECISION_BROWSER_FIXTURES=/private/tmp/workflow10-decision-browser
run_gate decision-fixtures cargo test --locked --offline --lib -- decision_browser_markup_and_closed_draft_recovery source_message_outside_window_is_exact_and_authorized
run_gate decision-browser env CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' node --test scripts/tests/workflow-decisions.mjs
