#!/bin/bash
# D-014 final C2 gates on the final binary. Prefer a quiet host (1-min load < 4, no foreign
# cargo/rustc; poll every 10 min, at most ~1 h, then run anyway and record the load). Load and
# foreign processes are sampled every 15 s. A failure while foreign load was seen is rerun once
# when quiet; a failure on a quiet host stops. stack-budget.sh is the full-suite gate (S3, R-016).
cd /home/mh/dev/openbot
C=plan/workflow-foundation/docs/logs/groupC
LIB=target/debug/deps/mail_agents-43f563ba6b6c24ac
LIB_SHA=44ada14f77c70b30889bd0b1bea51a8047664e9b944b7faa8dea323dac13afff
BIN=target/debug/deps/mail_agents-3e0cdae7a94f5271
BIN_SHA=b5b4ea2719cc280fe52134bfa0c362e561457b00a9d73d620db0341aff79a134
U=postgres://postgres:workflow048@127.0.0.1:55448/workflow048_test
unset ALLOW_MISSING_DATABASE_URL RUST_MIN_STACK
LOAD=$C/load-C2-final.log
foreign() {
  local n=0 pid cwd
  for pid in $(pgrep -f '(cargo|rustc|clippy-driver)' 2>/dev/null); do
    cwd=$(readlink /proc/$pid/cwd 2>/dev/null) || continue
    case "$cwd" in /home/mh/dev/openbot|/home/mh/dev/openbot/*) ;; *) n=$((n+1)) ;; esac
  done
  echo $n
}
note() { echo "$(date -u +%FT%TZ) $1 load=$(cut -d' ' -f1-3 /proc/loadavg) foreign_cargo_rustc=$(foreign)" | tee -a $LOAD; }
quiet_wait() {
  local polls=0
  while :; do
    if awk -v l="$(cut -d' ' -f1 /proc/loadavg)" 'BEGIN{exit !(l < 4)}' && [ "$(foreign)" -eq 0 ]; then note "quiet($1)"; return 0; fi
    polls=$((polls+1))
    if [ $polls -gt 6 ]; then note "not-quiet-after-1h-running-anyway($1)"; return 1; fi
    note "waiting-for-quiet-host($1)"; sleep 600
  done
}
exact_run() { # test label repeats
  local t=$1 label=$2 n full log
  full=$(ALLOW_MISSING_DATABASE_URL=1 $LIB "$t" --list 2>/dev/null | grep ': test$' | sed 's/: test$//')
  for n in $(seq 1 $3); do
    log=$C/$label-isolated-$t-$n.log
    { echo "# $(date -u +%FT%TZ) sha256=$(sha256sum $LIB | cut -c1-64) RUST_MIN_STACK=2097152 load=$(cut -d' ' -f1-3 /proc/loadavg) --exact $full"; env DATABASE_URL=$U TEST_DATABASE_URL=$U RUST_MIN_STACK=2097152 $LIB "$full" --exact --nocapture; echo "exit=$?"; } > $log 2>&1
    grep -q '^exit=0' $log || failed=1
    echo "$label $t-$n: $(grep -E '^test result' $log | cut -c1-45) $(grep -hE '^stop race|^pending case RunDeadline|rejected:' $log | cut -c1-120 | tr '\n' ' ')"
  done
}
[ "$(sha256sum $LIB | cut -c1-64)" = "$LIB_SHA" ] || { echo "binary digest mismatch"; exit 1; }
echo "# $(date -u +%FT%TZ) $(rustc -V) HEAD=$(git rev-parse HEAD)"
for attempt in 1 2; do
  A=f$attempt
  quiet_wait $A; quiet=$?
  SAMPLES=$C/load-samples-C2-$A.log; : > $SAMPLES
  ( while :; do echo "$(date -u +%FT%TZ) load=$(cut -d' ' -f1-3 /proc/loadavg) foreign=$(foreign)" >> $SAMPLES; sleep 15; done ) &
  sampler=$!
  failed=0
  note start-targeted-$A
  exact_run stop_task_serializes_with_final_dispatch_without_duplicate_customer_reply C2-$A 5
  exact_run workflow_action_dispatch_pending_actual_provider_dropped_on_loss_cancel_deadline C2-$A 3
  exact_run workflow_action_dispatch_pending_run_deadline_oracle_rejects_lease_limited_entry C2-$A 3
  GROUPA_SHA=$LIB_SHA GROUPA_LABEL=.C2-$A-targeted python3 -I $C/run-groupC.py persistence::task:: workflow_action_dispatch_ || failed=1
  note end-targeted-$A
  note start-stack-budget-$A
  { echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> scripts/stack-budget.sh --offline (sha256 $(sha256sum scripts/stack-budget.sh | cut -c1-64)); lib binary before $(sha256sum $LIB | cut -c1-64)";
    SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U scripts/stack-budget.sh --offline; echo "exit=$?";
    echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/stack-budget-C2-$A.log 2>&1
  grep -q '^exit=0' $C/stack-budget-C2-$A.log || failed=1
  note end-stack-budget-$A
  grep -E '^exit=|^test result' $C/stack-budget-C2-$A.log
  GROUPA_BINARY=$BIN GROUPA_SHA=$BIN_SHA GROUPA_LABEL=.C2-$A-bins python3 -I $C/run-groupC.py "" || failed=1
  note start-lease-budget-$A
  { echo "# $(date -u +%FT%TZ) SQLX_OFFLINE=true DATABASE_URL=TEST_DATABASE_URL=<task> LEASE_BUDGET_RECORD=$C/lease-budget-record-C2-$A.jsonl scripts/lease-budget.sh --offline (sha256 $(sha256sum scripts/lease-budget.sh | cut -c1-64)); RUST_MIN_STACK from .cargo/config.toml as in CI";
    SQLX_OFFLINE=true DATABASE_URL=$U TEST_DATABASE_URL=$U LEASE_BUDGET_RECORD=$C/lease-budget-record-C2-$A.jsonl scripts/lease-budget.sh --offline; echo "exit=$?";
    echo "# lib binary after $(sha256sum $LIB | cut -c1-64)"; } > $C/lease-budget-C2-$A.log 2>&1
  grep -q '^exit=0' $C/lease-budget-C2-$A.log || failed=1
  note end-lease-budget-$A
  grep -E '^exit=|^test result|LEASE' $C/lease-budget-C2-$A.log
  { echo "# $(date -u +%FT%TZ) $(cargo sqlx --version) DATABASE_URL=<task> SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets"; DATABASE_URL=$U SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets 2>&1; echo "exit=$?"; } > $C/sqlx-prepare-check-C2-$A.log 2>&1
  grep -q '^exit=0' $C/sqlx-prepare-check-C2-$A.log || failed=1
  kill $sampler 2>/dev/null; wait $sampler 2>/dev/null
  foreign_seen=$(awk -F'foreign=' '$2+0>0' $SAMPLES | wc -l)
  note "attempt-$A-done failed=$failed foreign_samples=$foreign_seen started_quiet=$([ $quiet -eq 0 ] && echo yes || echo no)"
  if [ $failed -eq 0 ]; then echo "RESULT pass attempt=$A"; exit 0; fi
  if [ $foreign_seen -eq 0 ]; then echo "RESULT fail-on-quiet-host attempt=$A"; exit 1; fi
  note "attempt-$A-load-invalidated"
done
echo "RESULT fail-after-one-rerun"; exit 3
