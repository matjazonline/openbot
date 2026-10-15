#!/bin/bash
# Isolated reruns (--exact, stock 2 MiB, task fixture) of the 7 tests that failed in the full-library run.
cd /home/mh/dev/openbot
C=plan/workflow-foundation/docs/logs/groupC
B=target/debug/deps/mail_agents-43f563ba6b6c24ac
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
unset ALLOW_MISSING_DATABASE_URL RUST_MIN_STACK
for t in agent_work_stays_out_until_a_human_decision_is_required the_queue_ages_the_generation_rather_than_the_row \
         claiming_anchors_the_next_run_on_the_slot_not_the_clock extending_an_expired_outreach_serializes_with_the_timeout_sweep \
         workflow_wait_competing_park_signal_resume_and_lost_ack \
         workflow_action_dispatch_delayed_first_poll_checks_monotonic_window workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline; do
  full=$(ALLOW_MISSING_DATABASE_URL=1 $B "$t" --list 2>/dev/null | grep ': test$' | sed 's/: test$//')
  for n in 1 2 3; do
    log=$C/C-isolated-$t-$n.log
    { echo "# $(date -u +%FT%TZ) sha256=$(sha256sum $B | cut -c1-64) RUST_MIN_STACK=2097152 load=$(cut -d' ' -f1-3 /proc/loadavg) --exact $full"; env DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $B "$full" --exact; echo "exit=$?"; } > $log 2>&1
    echo "$t-$n: $(grep -E '^test result' $log | cut -c1-45) $(grep -E 'panicked|left:|right:' $log | head -3 | tr '\n' ' ' | cut -c1-200)"
  done
done
GROUPA_SHA=18d7c86ab7d56031d38450f7a7748d1198ff5f7a8a49ed978b780dc440584bb4 GROUPA_LABEL=.C-rerun python3 -I $C/run-groupC.py workflow_action_dispatch_ workflow_wait_
