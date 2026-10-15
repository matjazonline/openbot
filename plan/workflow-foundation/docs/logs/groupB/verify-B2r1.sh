#!/bin/bash
# F-005 correction verification (stock 2 MiB, task fixture).
cd /home/mh/dev/openbot
G=plan/workflow-foundation/docs/logs/groupB
B=target/debug/deps/mail_agents-43f563ba6b6c24ac
SHA=b36b7b85efce7d17c9a8223fcf45e17828a7071de36d97c7a1b388eae9255a1b
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
exact() { ALLOW_MISSING_DATABASE_URL=1 $B "$1" --list 2>/dev/null | grep ': test$' | sed 's/: test$//'; }
run() { local t=$1 log=$2; { date -u +%FT%TZ; echo "# sha256=$(sha256sum $B | cut -c1-64) RUST_MIN_STACK=2097152 --exact"; env -u ALLOW_MISSING_DATABASE_URL DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $B "$(exact $t)" --exact --nocapture; echo "exit=$?"; } > $log 2>&1; echo "$(basename $log): $(grep -E '^test result' $log | cut -c1-40) | $(grep -hE '^receipt-first late|^late-release control rejected' $log | cut -c1-200 | tr '\n' ' ')"; }
for n in 1 2 3; do run workflow_action_cancellation_receipt_race_receipt_first $G/B2r1-isolated-receipt-first-$n.log; done
for n in 1 2 3; do run workflow_action_cancellation_receipt_race_late_release_fails_ordinary_oracle $G/B2r1-isolated-late-release-$n.log; done
run workflow_action_cancellation_receipt_ordinary_oracle_discriminates_attempt_paths $G/B2r1-isolated-oracle.log
GROUPA_SHA=$SHA GROUPA_LABEL=.B2r1 python3 -I $G/run-groupB.py cancellation_boundary_tests workflow_action_
echo "verify done"
