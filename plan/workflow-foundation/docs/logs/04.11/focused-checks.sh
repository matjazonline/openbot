#!/bin/zsh
# 04.11 focused checks per D2 §8.1 (areas A-F, H), D3 §K and D4 §S. Real PostgreSQL fixture.
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export TEST_DATABASE_URL=postgres://workflow0410@127.0.0.1:55450/workflow0410_test
export DATABASE_URL=$TEST_DATABASE_URL SQLX_OFFLINE=true CARGO_BUILD_JOBS=4
cd /Users/mac03/dev/openbot
logs=plan/workflow-foundation/docs/logs/04.11
statusfile=$logs/focused-status.log
: > $statusfile
run() {
    local area=$1; shift
    cargo test --locked --offline --lib -- "$@" > $logs/focused-$area.log 2>&1
    local rc=$?
    echo "$area rc=$rc $(grep -E '^test result' $logs/focused-$area.log | tail -1)" >> $statusfile
}
run A registry:: publication:: workflow_authority workflow_binding message_registry_
run B workflow_message_publish_ workflow_message_sql_ workflow_action_dispatch_
run C workflow_message_receipt_ workflow_message_wait_ wait_ polling fair supervise
run D workflow_message_recovery_ workflow_action recovery uncertainty action_reconciliation
run E delivery:: workflow_message_delivery_ cancellation task_release channel_assignment
run F thread:: workflow_message_thread_ find_outbound_reply_after latest_thread_rfc
run H workflow_message_reconcile_ workflow_message_race_ action_reconciliation mcp_reconciliation
cat $statusfile
