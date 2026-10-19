#!/usr/bin/env bash
set -uo pipefail
cd /Users/mac03/dev/openbot
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
logdir=plan/workflow-foundation/docs/logs/06
status="$logdir/06a-gate-status.log"
run_gate() {
  local name="$1"
  shift
  printf '%s START %s\n' "$(date -u +%FT%TZ)" "$name" >> "$status"
  if "$@" > "$logdir/06a-$name.log" 2>&1; then
    printf '%s PASS %s\n' "$(date -u +%FT%TZ)" "$name" >> "$status"
  else
    local code=$?
    printf '%s FAIL %s status=%s\n' "$(date -u +%FT%TZ)" "$name" "$code" >> "$status"
    return "$code"
  fi
}
: > "$status"
run_gate fmt cargo fmt --all -- --check || exit $?
run_gate diff git diff --check || exit $?
run_gate publication cargo test --locked --offline --lib application::workflow::publication || exit $?
run_gate compiler cargo test --locked --offline --lib application::workflow::compiler || exit $?
run_gate registry cargo test --locked --offline --lib application::workflow::registry || exit $?
run_gate offline cargo check --locked --offline --all-targets || exit $?
run_gate clippy cargo clippy --locked --offline --all-targets -- -D warnings || exit $?
