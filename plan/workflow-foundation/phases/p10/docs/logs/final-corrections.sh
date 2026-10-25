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
run_gate corrected-fmt cargo fmt --all -- --check
run_gate corrected-whitespace git diff --check
run_gate corrected-shutdown scripts/test-network-isolation.sh cargo test --locked --offline --lib production_workers_stop_on_the_shutdown_token
run_gate binary-tests scripts/test-network-isolation.sh cargo test --locked --offline --bin mail_agents
run_gate stack scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
run_gate lease scripts/test-network-isolation.sh scripts/lease-budget.sh --offline
export WORKFLOW_DECISION_BROWSER_FIXTURES=/private/tmp/workflow10-decision-browser
run_gate decision-fixtures cargo test --locked --offline --lib -- decision_browser_markup_and_closed_draft_recovery source_message_outside_window_is_exact_and_authorized
run_gate decision-browser env CHROME_PATH='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' node --test scripts/tests/workflow-decisions.mjs
