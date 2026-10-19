#!/bin/bash
set -euo pipefail
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 SQLX_OFFLINE=true
logs=plan/workflow-foundation/docs/logs/06
cargo clippy --locked --offline --all-targets -- -D warnings > "$logs/06B1-clippy-final.log" 2>&1
echo 'PASS clippy final'
cargo check --locked --offline --all-targets > "$logs/06B1-check-audit.log" 2>&1
echo 'PASS offline all-target final'
cargo fmt --all --check
git diff --check
node_modules/.bin/tailwindcss -i assets/app.css.in -o /private/tmp/workflow06-usage-check.css --minify > "$logs/06B1-css-check.log" 2>&1
cmp assets/app.css /private/tmp/workflow06-usage-check.css
node plan/workflow-foundation/docs/logs/06/browser-check.cjs > "$logs/06B1-browser-final.log" 2>&1
echo 'PASS formatting, CSS and browser'
graft build > "$logs/06B1-graft-build.log" 2>&1
echo 'PASS graph build'
