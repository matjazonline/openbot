#!/bin/bash
# Batch C1 targeted verification (stock 2 MiB, task fixture; runner requires passed == --list - ignored).
cd /home/mh/dev/openbot
C=plan/workflow-foundation/docs/logs/groupC
B=target/debug/deps/mail_agents-43f563ba6b6c24ac
SHA=8fd635ae795e0f8e60c78b4746a5ae1a13635d99e7dd4878564fccaa86d4a4f4
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
unset ALLOW_MISSING_DATABASE_URL RUST_MIN_STACK
[ "$(sha256sum $B | cut -c1-64)" = "$SHA" ] || { echo "binary digest mismatch"; exit 1; }
echo "# start $(date -u +%FT%TZ) load=$(cut -d' ' -f1-3 /proc/loadavg)"
for t in agent_work_stays_out_until_a_human_decision_is_required the_queue_ages_the_generation_rather_than_the_row \
         claiming_anchors_the_next_run_on_the_slot_not_the_clock extending_an_expired_outreach_serializes_with_the_timeout_sweep \
         workflow_wait_competing_park_signal_resume_and_lost_ack workflow_action_dispatch_delayed_first_poll_checks_monotonic_window \
         workflow_action_dispatch_expired_entry_oracle_rejects_polled_provider workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline; do
  full=$(ALLOW_MISSING_DATABASE_URL=1 $B "$t" --list 2>/dev/null | grep ': test$' | sed 's/: test$//')
  for n in 1 2 3; do
    log=$C/C1-isolated-$t-$n.log
    { echo "# $(date -u +%FT%TZ) sha256=$(sha256sum $B | cut -c1-64) RUST_MIN_STACK=2097152 load=$(cut -d' ' -f1-3 /proc/loadavg) --exact $full"; env DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $B "$full" --exact --nocapture; echo "exit=$?"; } > $log 2>&1
    echo "$t-$n: $(grep -E '^test result' $log | cut -c1-45) $(grep -hE '^pending case' $log | cut -c1-140 | tr '\n' ' ')"
  done
done
GROUPA_SHA=$SHA GROUPA_LABEL=.C1-targeted python3 -I $C/run-groupC.py \
  persistence::attention:: persistence::task:: persistence::schedule:: workflow_wait_ workflow_action_dispatch_ \
  workflow_control_ workflow_poll workflow_maintenance_ workflow_poison_ workflow_budget_activation_
echo "# end $(date -u +%FT%TZ) load=$(cut -d' ' -f1-3 /proc/loadavg) exit=$?"
