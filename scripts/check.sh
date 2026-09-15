#!/usr/bin/env bash
#
# Run every CI gate locally, in the order .github/workflows/ci.yml runs them.
#
#   ./scripts/check.sh              every gate, then a pass/fail summary
#   ./scripts/check.sh --fail-fast  stop at the first failing gate
#
# Gates keep running after a failure so one pass reports everything; a compile error therefore
# shows up again in each later cargo gate.
#
# DATABASE_URL defaults to the local development database with an explicit user, which sqlx-cli
# requires. Migrations apply to that database, because `cargo sqlx prepare --check` compares the
# query cache against it -- the same migrations the server applies at startup.
#
# The test suites do not use the shared `_test` sibling. Test support truncates every table when a
# test binary first connects, so a `cargo test` started from another checkout or session during
# this run would wipe fixtures out from under it and deadlock with it. They get a database created
# for this run instead, passed as TEST_DATABASE_URL and dropped on exit.
#
# Two CI steps are left out. `npm ci` would replace node_modules; run it yourself if tailwindcss is
# missing. The pg_stat_statements setup reconfigures CI's disposable Postgres with ALTER SYSTEM,
# which is not something to do to a local server, and no test depends on it.

set -uo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

FAIL_FAST=false
for argument in "$@"; do
  case "$argument" in
    --fail-fast) FAIL_FAST=true ;;
    *)
      echo "Unknown argument: $argument" >&2
      echo "Usage: $0 [--fail-fast]" >&2
      exit 2
      ;;
  esac
done

export DATABASE_URL="${DATABASE_URL:-postgres://$(whoami)@localhost:5432/mail_agents}"
# As in CI: compile from the checked-in .sqlx cache, except where a gate needs the live schema.
export SQLX_OFFLINE=true

# Rename the database in DATABASE_URL, leaving any query string such as `?sslmode=` in place.
database_url_base="${DATABASE_URL%%\?*}"
database_url_query=""
if [[ "$DATABASE_URL" == *\?* ]]; then
  database_url_query="?${DATABASE_URL#*\?}"
fi
check_database="${database_url_base##*/}_check_$$"
export TEST_DATABASE_URL="${database_url_base%/*}/${check_database}${database_url_query}"

drop_check_database() {
  psql "$DATABASE_URL" -X -q -c "DROP DATABASE IF EXISTS \"$check_database\" WITH (FORCE)" \
    >/dev/null 2>&1
}
trap drop_check_database EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

names=()
results=()
failed=false

gate() {
  local name="$1"
  shift
  names+=("$name")
  if [[ "$failed" == true && "$FAIL_FAST" == true ]]; then
    results+=("SKIP")
    return
  fi
  printf '\n==> %s\n' "$name"
  local started=$SECONDS
  if "$@"; then
    results+=("PASS $((SECONDS - started))s")
  else
    results+=("FAIL $((SECONDS - started))s")
    failed=true
  fi
}

# CI rebuilds into the tree and asks git whether anything changed. Building aside and comparing
# with the working copy gives the same answer without failing on a regenerated file that simply
# has not been committed yet.
generated_css_is_current() {
  local built status
  built="$(mktemp)"
  node_modules/.bin/tailwindcss -i assets/app.css.in -o "$built" --minify &&
    cmp -s "$built" assets/app.css
  status=$?
  rm -f "$built"
  if [[ $status -ne 0 ]]; then
    echo "assets/app.css does not match its sources; run: npm run build:css" >&2
  fi
  return "$status"
}

deployment_script_tests() {
  scripts/tests/deploy.sh && scripts/tests/credential-key-rotation.sh
}

transport_boundary() {
  scripts/tests/transport-boundary.sh && scripts/transport-boundary-check.sh
}

create_check_database() {
  psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$check_database\"" &&
    echo "tests run against $check_database"
}

gate "Generated CSS is current" generated_css_is_current
gate "SSE navigation lifecycle" \
  node --test scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs
gate "Deployment script tests" deployment_script_tests
gate "Formatting" cargo fmt --all -- --check
gate "Patch whitespace" git diff HEAD --check
gate "Transport abstraction boundary" transport_boundary
gate "Fetch locked Rust dependencies" cargo fetch --locked
gate "Offline compilation" cargo check --locked --offline --all-targets
gate "Clippy" cargo clippy --locked --all-targets -- -D warnings
gate "Migrations" env SQLX_OFFLINE=false cargo sqlx migrate run
gate "SQLx metadata matches migrated schema" \
  env SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
gate "Compile tests" cargo test --locked --offline --all-targets --no-run
gate "Test database for this run" create_check_database
gate "Database-backed test suite (isolated network)" \
  scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
gate "Stack budget" scripts/test-network-isolation.sh scripts/stack-budget.sh --offline

printf '\n==> Summary\n'
for index in "${!names[@]}"; do
  printf '%-10s %s\n' "${results[$index]}" "${names[$index]}"
done

if [[ "$failed" == true ]]; then
  exit 1
fi
