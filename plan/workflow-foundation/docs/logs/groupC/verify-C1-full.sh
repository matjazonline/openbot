#!/bin/bash
# Batch C1 final gates: full stock-2 MiB library + bin suites, then CI stack-budget and lease-budget
# (unchanged scripts, CI args --offline). Root/user confirmed a quiet host (bizi44 stopped), so
# no load wait; load and any foreign cargo/rustc are recorded at the start and end of every run.
cd /home/mh/dev/openbot
C=plan/workflow-foundation/docs/logs/groupC
LIB=target/debug/deps/mail_agents-43f563ba6b6c24ac
LIB_SHA=8fd635ae795e0f8e60c78b4746a5ae1a13635d99e7dd4878564fccaa86d4a4f4
BIN=target/debug/deps/mail_agents-3e0cdae7a94f5271
BIN_SHA=b5b4ea2719cc280fe52134bfa0c362e561457b00a9d73d620db0341aff79a134
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
unset ALLOW_MISSING_DATABASE_URL RUST_MIN_STACK
LOAD=$C/load-C1.log
foreign() { ps -eo args | grep -E '(cargo|rustc|mail_agents)' | grep -v -E 'grep|/home/mh/dev/openbot|verify-C1' | grep -c . ; }
note() { echo "$(date -u +%FT%TZ) $1 load=$(cut -d' ' -f1-3 /proc/loadavg) foreign_cargo_rustc=$(foreign)" | tee -a $LOAD; }
quiet() { :; }
echo "# $(date -u +%FT%TZ) $(rustc -V) HEAD=$(git rev-parse HEAD)"
quiet full-lib; note start-full-lib
GROUPA_SHA=$LIB_SHA GROUPA_LABEL=.C1 python3 -I $C/run-groupC.py ""
note end-full-lib
GROUPA_BINARY=$BIN GROUPA_SHA=$BIN_SHA GROUPA_LABEL=.C1-bins python3 -I $C/run-groupC.py ""
quiet stack-budget; note start-stack-budget
{ echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> scripts/stack-budget.sh --offline (sha256 $(sha256sum scripts/stack-budget.sh | cut -c1-64)); lib binary before $(sha256sum $LIB | cut -c1-64)";
  SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U scripts/stack-budget.sh --offline; echo "exit=$?";
  echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/stack-budget-C1.log 2>&1
note end-stack-budget
grep -E '^exit=|^test result' $C/stack-budget-C1.log
quiet lease-budget; note start-lease-budget
{ echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> LEASE_BUDGET_RECORD=$C/lease-budget-record-C1.jsonl scripts/lease-budget.sh --offline (sha256 $(sha256sum scripts/lease-budget.sh | cut -c1-64)); RUST_MIN_STACK from .cargo/config.toml as in CI";
  SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U LEASE_BUDGET_RECORD=$C/lease-budget-record-C1.jsonl scripts/lease-budget.sh --offline; echo "exit=$?";
  echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/lease-budget-C1.log 2>&1
note end-lease-budget
grep -E '^exit=|^test result|LEASE' $C/lease-budget-C1.log
echo "verify-C1-full done $(date -u +%FT%TZ)"
