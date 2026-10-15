#!/bin/bash
# C1 correction (F-007) verification: targeted runs, then the final full gates on the same binary.
# Load and foreign cargo/rustc processes are recorded at every run boundary.
cd /home/mh/dev/openbot
C=plan/workflow-foundation/docs/logs/groupC
LIB=target/debug/deps/mail_agents-43f563ba6b6c24ac
LIB_SHA=94a422dc15bc09486aeb99872f5ecf140982df93183eec696e0db072a0c8fd5e
BIN=target/debug/deps/mail_agents-3e0cdae7a94f5271
BIN_SHA=b5b4ea2719cc280fe52134bfa0c362e561457b00a9d73d620db0341aff79a134
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
unset ALLOW_MISSING_DATABASE_URL RUST_MIN_STACK
[ "$(sha256sum $LIB | cut -c1-64)" = "$LIB_SHA" ] || { echo "binary digest mismatch"; exit 1; }
LOAD=$C/load-C2.log
foreign() { ps -eo args | grep -E '(cargo|rustc|mail_agents)' | grep -v -E 'grep|/home/mh/dev/openbot|verify-C2' | grep -c . ; }
note() { echo "$(date -u +%FT%TZ) $1 load=$(cut -d' ' -f1-3 /proc/loadavg) foreign_cargo_rustc=$(foreign)" | tee -a $LOAD; }
echo "# $(date -u +%FT%TZ) $(rustc -V) HEAD=$(git rev-parse HEAD)"
note start-targeted
for t in workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline workflow_action_dispatch_pending_run_deadline_oracle_rejects_lease_limited_entry; do
  full=$(ALLOW_MISSING_DATABASE_URL=1 $LIB "$t" --list 2>/dev/null | grep ': test$' | sed 's/: test$//')
  for n in 1 2 3; do
    log=$C/C2-isolated-$t-$n.log
    { echo "# $(date -u +%FT%TZ) sha256=$(sha256sum $LIB | cut -c1-64) RUST_MIN_STACK=2097152 load=$(cut -d' ' -f1-3 /proc/loadavg) --exact $full"; env DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $LIB "$full" --exact --nocapture; echo "exit=$?"; } > $log 2>&1
    echo "$t-$n: $(grep -E '^test result' $log | cut -c1-45) $(grep -hE '^pending case RunDeadline|rejected:' $log | cut -c1-200 | tr '\n' ' ')"
  done
done
GROUPA_SHA=$LIB_SHA GROUPA_LABEL=.C2-targeted python3 -I $C/run-groupC.py workflow_action_dispatch_
note end-targeted
note start-full-lib
GROUPA_SHA=$LIB_SHA GROUPA_LABEL=.C2 python3 -I $C/run-groupC.py ""
note end-full-lib
GROUPA_BINARY=$BIN GROUPA_SHA=$BIN_SHA GROUPA_LABEL=.C2-bins python3 -I $C/run-groupC.py ""
note start-stack-budget
{ echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> scripts/stack-budget.sh --offline (sha256 $(sha256sum scripts/stack-budget.sh | cut -c1-64)); lib binary before $(sha256sum $LIB | cut -c1-64)";
  SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U scripts/stack-budget.sh --offline; echo "exit=$?";
  echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/stack-budget-C2.log 2>&1
note end-stack-budget
grep -E '^exit=|^test result' $C/stack-budget-C2.log
note start-lease-budget
{ echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> LEASE_BUDGET_RECORD=$C/lease-budget-record-C2.jsonl scripts/lease-budget.sh --offline (sha256 $(sha256sum scripts/lease-budget.sh | cut -c1-64)); RUST_MIN_STACK from .cargo/config.toml as in CI";
  SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U LEASE_BUDGET_RECORD=$C/lease-budget-record-C2.jsonl scripts/lease-budget.sh --offline; echo "exit=$?";
  echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/lease-budget-C2.log 2>&1
note end-lease-budget
grep -E '^exit=|^test result|LEASE' $C/lease-budget-C2.log
{ echo "# $(date -u +%FT%TZ) $(cargo sqlx --version) DATABASE_URL=<task> SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets"; DATABASE_URL=$U SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets 2>&1; echo "exit=$?"; } > $C/sqlx-prepare-check-C2.log 2>&1
grep -E '^exit=' $C/sqlx-prepare-check-C2.log
echo "verify-C2 done $(date -u +%FT%TZ)"
