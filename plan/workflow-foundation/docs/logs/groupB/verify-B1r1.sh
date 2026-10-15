#!/bin/bash
# F-004 correction verification (stock 2 MiB, task fixture).
cd /home/mh/dev/openbot
G=plan/workflow-foundation/docs/logs/groupB
B=target/debug/deps/mail_agents-43f563ba6b6c24ac
SHA=de27a83b2cbfffc16c3928c75dab6620828599a2f30f59385b56d17e71dd123a
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
exact() { ALLOW_MISSING_DATABASE_URL=1 $B "$1" --list 2>/dev/null | grep ': test$' | sed 's/: test$//'; }
run() { local t=$1 log=$2; { date -u +%FT%TZ; echo "# sha256=$(sha256sum $B | cut -c1-64) RUST_MIN_STACK=2097152 --exact"; env -u ALLOW_MISSING_DATABASE_URL DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $B "$(exact $t)" --exact --nocapture; echo "exit=$?"; } > $log 2>&1; echo "$t $(basename $log): $(grep -E '^test result' $log | cut -c1-40)"; }
for n in 1 2 3; do run workflow_action_cancellation_pending_provider_dropped_by_ownership_supervision $G/B1r1-isolated-pending-$n.log; done
run workflow_action_cancellation_pending_oracle_rejects_deadline_only_interruption $G/B1r1-isolated-negative-control.log
run workflow_action_cancellation_pending_oracle_discriminates_interruption_causes $G/B1r1-isolated-oracle.log
GROUPA_SHA=$SHA GROUPA_LABEL=.B1r1 python3 -I $G/run-groupB.py cancellation_boundary_tests workflow_action_
echo "verify done"
