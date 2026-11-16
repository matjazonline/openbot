#!/usr/bin/env bash
#
# Run the required root, browser, plugin, independent-service and integration gates locally.
# Prerequisites and intentionally manual checks are documented in README.md (Running Checks).
#
#   ./scripts/check.sh              every gate, then a pass/fail summary
#   ./scripts/check.sh --fail-fast  stop at the first failing gate
#
# Gates keep running after a failure so one pass reports everything; a compile error therefore
# shows up again in each later cargo gate.
#
# DATABASE_URL supplies maintenance access. Migrations and tests use a fresh owned database.
# The test suites do not use the shared `_test` sibling. Test support truncates every table when a
# test binary first connects, so a `cargo test` started from another checkout or session during
# this run would wipe fixtures out from under it and deadlock with it. They get a database created
# for this run instead, passed as TEST_DATABASE_URL and dropped on exit.
#
# Dependency installation and PostgreSQL server configuration remain explicit prerequisites:
# this runner neither replaces node_modules nor reconfigures/restarts a local database server.

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

check_admin_url="${DATABASE_URL:-postgres://$(whoami)@localhost:5432/postgres}"
# As in CI: compile from the checked-in .sqlx cache, except where a gate needs the live schema.
export SQLX_OFFLINE=true

# Rename the database in DATABASE_URL, leaving any query string such as `?sslmode=` in place.
database_url_base="${check_admin_url%%\?*}"
database_url_query=""
if [[ "$check_admin_url" == *\?* ]]; then
  database_url_query="?${check_admin_url#*\?}"
fi
check_suffix="$(python3 -c 'import uuid; print(uuid.uuid4().hex)')" || exit 1
check_database="openbot_check_$check_suffix"
export TEST_DATABASE_URL="${database_url_base%/*}/${check_database}${database_url_query}"
export DATABASE_URL="$TEST_DATABASE_URL"
check_database_created=false
check_work="$(mktemp -d "${TMPDIR:-/tmp}/openbot-check.XXXXXX")" || exit 1
export CHECK_WORK="$check_work"
export MEMORY_TEST_DATABASE_URL="${MEMORY_TEST_DATABASE_URL:-${database_url_base%/*}/postgres${database_url_query}}"
export MEMORY_INTEGRATION_ADMIN_URL="${MEMORY_INTEGRATION_ADMIN_URL:-$MEMORY_TEST_DATABASE_URL}"
export ATTRIBUTION_TEST_ADMIN_URL="${ATTRIBUTION_TEST_ADMIN_URL:-${database_url_base%/*}/postgres${database_url_query}}"
export HTML_INJECTION_BROWSER_FIXTURES="$check_work/html"
export APP_JS_OUT="$HTML_INJECTION_BROWSER_FIXTURES/app.js"
export KNOWLEDGE_BROWSER_FIXTURES="$check_work/knowledge"
export MEMORY_SETTINGS_BROWSER_FIXTURES="$check_work/memory-settings"
export AGENT_AUTHORING_BROWSER_FIXTURES="$check_work/agent-authoring"
export CLOUDROOM_MANAGEMENT_BROWSER_FIXTURES="$check_work/local-agents"
export SKILL_BUNDLE_BROWSER_FIXTURES="$check_work/skill-bundle"
export WORKFLOW_AGENT_BROWSER_FIXTURES="$check_work/workflow-agent"
export SESSION_REVOCATION_BROWSER_FIXTURES="$check_work/session"
export VAULT_AUTHORING_BROWSER_FIXTURES="$check_work/vault"
export WORKFLOW_DECISION_BROWSER_FIXTURES="$check_work/decision"
export AGENT_HISTORY_BROWSER_FIXTURES="$check_work/history"
check_knowledge_image="openbot-check-knowledge:$check_suffix"
check_memory_image="openbot-check-memory:$check_suffix"
check_attribution_image="openbot-check-attribution:$check_suffix"

cleanup_check() {
  if [[ "$check_database_created" == true ]]; then
    psql "$check_admin_url" -X -q -v ON_ERROR_STOP=1 -c "DROP DATABASE \"$check_database\" WITH (FORCE)" \
      >/dev/null 2>&1 || echo "warning: could not drop owned database $check_database" >&2
  fi
  rm -rf "$check_work"
  if command -v docker >/dev/null 2>&1; then
    docker image rm "$check_knowledge_image" "$check_memory_image" "$check_attribution_image" >/dev/null 2>&1 || true
  fi
}
trap cleanup_check EXIT
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
  # Do not put a shell function in an `if`: Bash would disable its `set -e`,
  # allowing a later successful command to hide an earlier failed prerequisite.
  "$@"
  local status=$?
  if [[ $status -eq 0 ]]; then
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
  psql "$check_admin_url" -X -q -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$check_database\"" || return
  check_database_created=true
  echo "migrations and tests run against $check_database"
}

source scripts/check-support.sh

gate "Runner test coverage" env PYTHONDONTWRITEBYTECODE=1 python3 scripts/tests/check-runner.py
gate "Local-agent plugin tests and guards" check_plugin
gate "Reproducible plugin package and runtime install" check_plugin_package
gate "Generated CSS is current" generated_css_is_current
gate "Frontend protocols" node --test \
  scripts/tests/sse-navigation.mjs scripts/tests/agent-runtime.mjs scripts/tests/channel-response-authoring.mjs \
  scripts/tests/workflow-runs.mjs scripts/tests/workflow-conversations.mjs scripts/tests/workflow-samples.mjs \
  scripts/tests/run-counts.mjs
gate "Deployment script tests" deployment_script_tests
gate "Knowledge artifact preparation tests" python3 scripts/tests/knowledge-model-prepare.py
gate "Doop runner boundaries" python3 scripts/tests/doop-test.py
gate "Doop probe protocols" node --test scripts/tests/doop-probe.mjs
gate "Formatting" cargo fmt --all -- --check
gate "Patch whitespace" git diff HEAD --check
gate "Transport abstraction boundary" transport_boundary
gate "Workflow sample boundary tests" python3 scripts/tests/workflow-sample-boundary.py
gate "Workflow sample boundary" python3 scripts/workflow-sample-boundary.py
gate "Integration boundary tests" python3 scripts/tests/integration-boundary.py
gate "Integration boundary" python3 scripts/integration-boundary.py
gate "Integration documentation" python3 scripts/check-integration-docs.py
gate "Workflow evidence validator fixtures" \
  env PYTHONDONTWRITEBYTECODE=1 python3 scripts/tests/workflow-evidence.py
gate "Retained workflow evidence" \
  env PYTHONDONTWRITEBYTECODE=1 python3 scripts/check-workflow-evidence.py --available-manifests
gate "Fetch locked Rust dependencies" cargo fetch --locked
gate "Offline compilation" cargo check --locked --offline --all-targets
gate "Clippy" cargo clippy --locked --all-targets -- -D warnings
gate "Test database for this run" create_check_database
gate "Migrations" env SQLX_OFFLINE=false cargo sqlx migrate run
gate "SQLx metadata matches migrated schema" \
  env SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets
gate "Compile tests" cargo test --locked --offline --all-targets --no-run
gate "Query statistics prerequisites" check_query_statistics
gate "Integration conformance families are registered" check_conformance_inventory
# source_modules and the non-ignored browser fixture producers run here once.
gate "Database-backed test suite (isolated network)" \
  scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
gate "Ignored browser fixture producers" check_browser_fixtures
gate "Browser protocols using native markup" node --test \
  scripts/tests/team-autocomplete-browser.mjs scripts/tests/html-injection-browser.mjs \
  scripts/tests/knowledge-browser.mjs scripts/tests/memory-settings-browser.mjs \
  scripts/tests/agent-authoring-browser.mjs scripts/tests/local-agents-browser.mjs \
  scripts/tests/skill-bundle-browser.mjs scripts/tests/workflow-agent-picker-browser.mjs \
  scripts/tests/session-revocation-browser.mjs scripts/tests/vault-authoring-browser.mjs \
  scripts/tests/workflow-decisions.mjs scripts/tests/agent-history-browser.mjs
gate "Stack budget" scripts/test-network-isolation.sh scripts/stack-budget.sh --offline
gate "Lease budget" scripts/test-network-isolation.sh scripts/lease-budget.sh --offline
gate "Knowledge production image" check_knowledge_image
gate "Memory model preparation" python3 -I services/memory/scripts/prepare-model.py --download
gate "Memory extracted service suite" services/memory/scripts/extracted-build.sh
gate "Memory backup and restore" check_memory_backup
gate "Memory standalone journey" env MEMORY_JOURNEY_PROVIDER=stand-in services/memory/scripts/standalone-journey.sh
gate "OpenBot and memory integration" scripts/memory-integration.sh
gate "Memory production image" check_memory_image
# The service is developed independently and may not be present on this branch yet.
# Its CI declaration also requires the gates, so missing declared source fails visibly.
if [[ -d services/attribution || -f .github/workflows/attribution-service.yml ]]; then
  gate "Attribution extracted service and contracts" check_attribution_service
  gate "Attribution browser integration" check_attribution_browser
  gate "Attribution production image" check_attribution_image
fi
gate "Managed Doop service and private integration" python3 scripts/check-doop.py --output "$check_work/doop"

printf '\n==> Summary\n'
for index in "${!names[@]}"; do
  printf '%-10s %s\n' "${results[$index]}" "${names[$index]}"
done

if [[ "$failed" == true ]]; then
  exit 1
fi
