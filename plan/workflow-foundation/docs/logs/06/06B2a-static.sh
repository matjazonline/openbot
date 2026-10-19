#!/bin/bash
set -euo pipefail
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
logs=plan/workflow-foundation/docs/logs/06
cargo fmt --all -- --check > "$logs/06B2a-fmt.log" 2>&1
cargo check --locked --offline --all-targets > "$logs/06B2a-check-final.log" 2>&1
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B2a-clippy.log" 2>&1
git diff --check > "$logs/06B2a-diff-check.log" 2>&1
graft build > "$logs/06B2a-graft.log" 2>&1
