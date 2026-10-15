#!/bin/bash
# B1 verification: isolated --exact runs, repeated B1 filter, affected filters; stock 2 MiB.
cd /home/mh/dev/openbot
G=plan/workflow-foundation/docs/logs/groupB
B=target/debug/deps/mail_agents-43f563ba6b6c24ac
SHA=596408e77e589d68078de72a0e061d77465032bbd5fb0787f73f0d10db638a72
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
for t in $(ALLOW_MISSING_DATABASE_URL=1 $B cancellation_boundary_tests --list 2>/dev/null | grep ': test$' | sed 's/: test$//'); do
  short=${t##*::}
  { date -u +%FT%TZ; echo "# sha256=$(sha256sum $B | cut -c1-64) RUST_MIN_STACK=2097152 --exact"; env -u ALLOW_MISSING_DATABASE_URL DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $B "$t" --exact --nocapture; echo "exit=$?"; } > $G/B1-isolated-$short.log 2>&1
  echo "isolated $short: $(grep -E '^test result' $G/B1-isolated-$short.log | cut -c1-40)"
done
for n in 1 2 3; do GROUPA_SHA=$SHA GROUPA_LABEL=.B1-repeat$n python3 -I $G/run-groupB.py cancellation_boundary_tests; done
GROUPA_SHA=$SHA GROUPA_LABEL=.B1-full python3 -I $G/run-groupB.py workflow_action_ prior_bounds_ workflow_control_
echo "verify done"
