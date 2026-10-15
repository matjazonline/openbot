#!/bin/bash
# Group B3 verification (stock 2 MiB, task fixture; runner requires passed == --list).
cd /home/mh/dev/openbot
G=plan/workflow-foundation/docs/logs/groupB
B=target/debug/deps/mail_agents-43f563ba6b6c24ac
SHA=18d7c86ab7d56031d38450f7a7748d1198ff5f7a8a49ed978b780dc440584bb4
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
[ "$(sha256sum $B | cut -c1-64)" = "$SHA" ] || { echo "binary digest mismatch"; exit 1; }
exact() { ALLOW_MISSING_DATABASE_URL=1 $B "$1" --list 2>/dev/null | grep ': test$' | sed 's/: test$//'; }
run() { local t=$1 log=$2; { date -u +%FT%TZ; echo "# sha256=$(sha256sum $B | cut -c1-64) RUST_MIN_STACK=2097152 --exact $(exact $t)"; env -u ALLOW_MISSING_DATABASE_URL DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $B "$(exact $t)" --exact --nocapture; echo "exit=$?"; } > $log 2>&1; echo "$(basename $log): $(grep -E '^test result' $log | cut -c1-40) | $(grep -hE '^completion race|rejected:' $log | cut -c1-160 | tr '\n' ' ')"; }
for t in workflow_action_cancellation_completion_race_cancel_first \
         workflow_action_cancellation_completion_race_completion_first \
         workflow_action_cancellation_completion_race_resolved_commit_fails_ordinary_oracle \
         workflow_action_cancellation_completion_ordinary_oracle_discriminates_attempt_paths \
         workflow_action_cancellation_replay_contract_cannot_override_terminal \
         workflow_action_cancellation_applied_then_hung_history_reconciles_after_cancel; do
  for n in 1 2 3; do run $t $G/B3-isolated-$t-$n.log; done
done
GROUPA_SHA=$SHA GROUPA_LABEL=.B3 python3 -I $G/run-groupB.py \
  workflow_action_cancellation_ cancellation_boundary_tests \
  workflow_action_reconciliation_control_serialization_ workflow_action_reconciliation_claim_cancellation_ \
  workflow_action_reconciliation_late_truth_ workflow_action_reconciliation_revision_ \
  workflow_action_uncertainty_ workflow_action_receipt_ workflow_lease_resolution_completion_ \
  prior_bounds_ workflow_control_ workflow_action_
echo "verify done exit=$?"
