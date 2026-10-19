#!/bin/bash
set -euo pipefail
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true RUST_MIN_STACK=2097152
export TEST_DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
export MODEL_USAGE_BROWSER_FIXTURE_DIR=/private/tmp/workflow06-usage-browser
logs=plan/workflow-foundation/docs/logs/06
for filter in model_usage harness_runs adapters::harness::rig application::workflow::publication application::workflow::compiler adapters::http::pages::tests; do
    file=${filter//:/_}
    cargo test --locked --offline --lib "$filter" > "$logs/06B1-focused-$file.log" 2>&1
    echo "PASS $filter"
done
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B1-clippy-2.log" 2>&1
echo 'PASS clippy'
cargo check --locked --offline --all-targets > "$logs/06B1-check-final.log" 2>&1
echo 'PASS offline check'
cargo fmt --all --check
git diff --check
echo 'PASS fmt and diff'
