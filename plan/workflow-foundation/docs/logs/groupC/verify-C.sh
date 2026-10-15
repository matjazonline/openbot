#!/bin/bash
# Group C runtime gates (task fixture; missing-DB allowance unset; counts vs --list).
# Sequential, nothing else running: full stock-2 MiB library + bin suites, then the CI
# stack-budget and lease-budget scripts unchanged with CI's arguments (--offline) and env.
cd /home/mh/dev/openbot
C=plan/workflow-foundation/docs/logs/groupC
LIB=target/debug/deps/mail_agents-43f563ba6b6c24ac
LIB_SHA=18d7c86ab7d56031d38450f7a7748d1198ff5f7a8a49ed978b780dc440584bb4
BIN=target/debug/deps/mail_agents-3e0cdae7a94f5271
BIN_SHA=217fdee314c474b552cadcb6fbb80f9a2dddf232df732e793ab9a2aa4fd57b7e
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
unset ALLOW_MISSING_DATABASE_URL RUST_MIN_STACK
echo "# $(date -u +%FT%TZ) $(rustc -V) HEAD=$(git rev-parse HEAD)"
GROUPA_SHA=$LIB_SHA GROUPA_LABEL=.C python3 -I $C/run-groupC.py ""
GROUPA_BINARY=$BIN GROUPA_SHA=$BIN_SHA GROUPA_LABEL=.C-bins python3 -I $C/run-groupC.py ""
echo "# $(date -u +%FT%TZ) stack-budget"
{ echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> scripts/stack-budget.sh --offline (sha256 $(sha256sum scripts/stack-budget.sh | cut -c1-64)); lib binary before $(sha256sum $LIB | cut -c1-64)";
  SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U scripts/stack-budget.sh --offline; echo "exit=$?";
  echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/stack-budget-C.log 2>&1
grep -E '^exit=|^test result|Running|running the test suite' $C/stack-budget-C.log
echo "# $(date -u +%FT%TZ) lease-budget"
{ echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> LEASE_BUDGET_RECORD=$C/lease-budget-record-C.jsonl scripts/lease-budget.sh --offline (sha256 $(sha256sum scripts/lease-budget.sh | cut -c1-64)); RUST_MIN_STACK from .cargo/config.toml as in CI";
  SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U LEASE_BUDGET_RECORD=$C/lease-budget-record-C.jsonl scripts/lease-budget.sh --offline; echo "exit=$?";
  echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/lease-budget-C.log 2>&1
grep -E '^exit=|^test result|LEASE|lease budget' $C/lease-budget-C.log
echo "verify-C done $(date -u +%FT%TZ)"
