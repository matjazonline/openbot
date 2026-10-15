#!/bin/bash
# B2 verification (stock 2 MiB, task fixture).
cd /home/mh/dev/openbot
G=plan/workflow-foundation/docs/logs/groupB
B=target/debug/deps/mail_agents-43f563ba6b6c24ac
SHA=78998780a0d13a8a965214747bce7db459116d22a4f13bff4c7cee0c206a2901
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
for t in $(ALLOW_MISSING_DATABASE_URL=1 $B cancellation_boundary_tests::receipt_tests --list 2>/dev/null | grep ': test$' | sed 's/: test$//'); do
  short=${t##*::}
  for n in 1 2 3; do
    log=$G/B2-isolated-$short-$n.log
    { date -u +%FT%TZ; echo "# sha256=$(sha256sum $B | cut -c1-64) RUST_MIN_STACK=2097152 --exact"; env -u ALLOW_MISSING_DATABASE_URL DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $B "$t" --exact --nocapture; echo "exit=$?"; } > $log 2>&1
    echo "isolated $short #$n: $(grep -E '^test result' $log | cut -c1-40)"
  done
done
GROUPA_SHA=$SHA GROUPA_LABEL=.B2 python3 -I $G/run-groupB.py cancellation_boundary_tests::receipt_tests cancellation_boundary_tests workflow_action_ workflow_control_
echo "verify done"
