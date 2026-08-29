#!/usr/bin/env bash
#
# Fail if the test suite stops fitting in a stock thread stack.
#
# `.cargo/config.toml` raises RUST_MIN_STACK to 16 MiB so that a test driving the
# task-worker -> dispatch -> agent-runner chain fails on its assertions rather than on the guard
# page. That fixes the tests and removes the alarm: CI would otherwise tolerate roughly twelvefold
# growth in silence. This chain has grown silently before -- an agent run that completed on
# 2026-08-26 aborted the process on 2026-08-29 -- so the suite is run once more here at the
# platform default, where growth shows up as a red build instead of as a crash months later.
#
# Measured 2026-08-29 on arm64/macOS, debug: the suite needs ~1.35 MiB. The deepest path is
# `AgentRunner::execute` into the provider's reqwest/rustls stack, not our own code. 2 MiB leaves
# about 45%.
#
# When this fails, `scripts/stack-frames.sh` shows which frames grew (arm64/macOS only). Prefer
# shrinking the chain -- extract a synchronous helper, drop an `async fn` level, `Box::pin` a seam
# -- over raising the budget. If you do raise it, record why in this comment.

set -uo pipefail

BUDGET_KIB="${STACK_BUDGET_KIB:-2048}"

# Exported, so it wins over `.cargo/config.toml`: cargo's `[env]` does not override a variable
# that is already set unless it is declared with `force = true`.
export RUST_MIN_STACK=$((BUDGET_KIB * 1024))

echo "running the test suite with a ${BUDGET_KIB} KiB thread stack"

output=$(cargo test --locked --lib "$@" 2>&1)
status=$?
echo "$output"

if [[ $status -eq 0 ]]; then
    exit 0
fi

if grep -q "has overflowed its stack" <<<"$output"; then
    culprit=$(grep -o "thread '[^']*' ([0-9]*) has overflowed its stack" <<<"$output" | head -1)
    cat >&2 <<EOF

=======================================================================
STACK BUDGET EXCEEDED

  $culprit

The await chain no longer fits in ${BUDGET_KIB} KiB, the stack a thread gets by default. It still
passes under the 16 MiB that .cargo/config.toml sets, so nothing else in CI would have told you.

In an unoptimized build each 'async fn' poll frame materialises the child future it constructs, so
a level of nesting costs real stack. Run scripts/stack-frames.sh to see which frames grew, then
either shrink the chain or raise STACK_BUDGET_KIB in scripts/stack-budget.sh with a reason.
=======================================================================
EOF
    exit 1
fi

echo "tests failed for a reason unrelated to the stack budget" >&2
exit $status
