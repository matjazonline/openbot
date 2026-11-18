#!/usr/bin/env bash
# Gate implementations sourced by check.sh. Each group fails at its first failed prerequisite.
# Subshells keep service directories, stack settings and credentials out of sibling gates.

check_plugin() (
  set -e
  npm --prefix integrations/cloudroom-plugin run typecheck
  npm --prefix integrations/cloudroom-plugin test
  npm --prefix integrations/cloudroom-plugin run test:guards
)

check_plugin_package() (
  set -e
  local package_dir="$CHECK_WORK/plugin"
  mkdir -p "$package_dir"
  npm --prefix integrations/cloudroom-plugin run package -- "$package_dir"
  tar -xzf "$package_dir"/*.tgz -C "$package_dir" --strip-components=1
  npm --prefix "$package_dir" ci --omit=dev
)

check_query_statistics() (
  set -e
  # Only configure the invocation-owned database. Server settings are read, never changed.
  psql "$DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -c 'CREATE EXTENSION IF NOT EXISTS pg_stat_statements'
  local setting expected actual
  for setting in pg_stat_statements.track pg_stat_statements.track_utility log_parameter_max_length log_parameter_max_length_on_error; do
    case "$setting" in
      pg_stat_statements.track) expected=top ;;
      pg_stat_statements.track_utility) expected=off ;;
      *) expected=0 ;;
    esac
    actual="$(psql "$DATABASE_URL" -X -Atqc "SHOW $setting")"
    [[ "$actual" == "$expected" ]] || { echo "required PostgreSQL setting: $setting=$expected (see README.md)" >&2; exit 1; }
  done
  psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -c 'SELECT 1 AS openbot_check_query_probe'
  psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -Atqc \
    "SELECT queryid FROM pg_stat_statements WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database()) AND query LIKE '%openbot_check_query_probe%' LIMIT 1" \
    | grep -Eq '^-?[0-9]+$'
)

check_conformance_inventory() (
  set -e
  local family
  for family in integration_portability_ integration_service_contract_ vault_google_e2e_; do
    cargo test --locked --offline --lib "$family" -- --skip doop --skip Doop --list > "$CHECK_WORK/conformance-list"
    grep -Eq ': test$' "$CHECK_WORK/conformance-list"
  done
)

check_browser_fixtures() (
  set -e
  local fixture
  local fixtures=(
    dump_html_injection_browser_fixtures dump_knowledge_browser_fixtures
    dump_memory_settings_browser_fixtures dump_autonomous_agent_authoring_browser_fixtures
    dump_cloudroom_management_browser_fixtures dump_skill_bundle_browser_fixtures
    dump_autonomous_workflow_agent_picker_browser_fixtures dump_session_revocation_browser_fixtures
  )
  cargo test --locked --offline --lib -- --skip doop --skip Doop --ignored --list "${fixtures[@]}" > "$CHECK_WORK/browser-fixtures-list"
  for fixture in "${fixtures[@]}"; do
    grep -Eq "::${fixture}: test$" "$CHECK_WORK/browser-fixtures-list"
  done
  scripts/test-network-isolation.sh cargo test --locked --offline --lib -- --skip doop --skip Doop --ignored "${fixtures[@]}"
)

check_knowledge_image() (
  set -e
  docker build --tag "$check_knowledge_image" .
  python3 scripts/check-knowledge-image.py --image "$check_knowledge_image"
)

check_memory_backup() (
  set -e
  if [[ -z "${MEMORY_PG_BIN:-}" ]]; then
    local dump
    dump="$(command -v pg_dump)"
    export MEMORY_PG_BIN="$(dirname "$dump")"
  fi
  services/memory/scripts/backup-restore-check.sh
)

check_memory_image() (
  set -e
  docker build --tag "$check_memory_image" services/memory
  python3 services/memory/scripts/container-smoke.py --image "$check_memory_image"
)

check_attribution_service() (
  set -e
  export CARGO_TARGET_DIR="$ROOT_DIR/services/attribution/target"
  export RUST_MIN_STACK=2097152
  cd services/attribution
  cargo fetch --locked
  python3 scripts/extracted-build.py
)

check_attribution_browser() (
  set -e
  export ATTRIBUTION_TEST_BINARY="$ROOT_DIR/services/attribution/target/debug/attribution-service"
  (
    cd services/attribution
    export CARGO_TARGET_DIR="$ROOT_DIR/services/attribution/target"
    export RUST_MIN_STACK=2097152
    cargo build --locked --offline --bin attribution-service
    node --check assets/sdk-1.0.0.js
    node --check browser/website.mjs
    cargo test --locked --offline --lib -- --skip doop --skip Doop --ignored --list \
      attribution_setup_browser_ attribution_checkout_handoff_browser_ > "$CHECK_WORK/attribution-browser-list"
    grep -Eq '::attribution_setup_browser_.*: test$' "$CHECK_WORK/attribution-browser-list"
    grep -Eq '::attribution_checkout_handoff_browser_.*: test$' "$CHECK_WORK/attribution-browser-list"
    cargo test --locked --offline --lib -- --skip doop --skip Doop --ignored --nocapture \
      attribution_setup_browser_ attribution_checkout_handoff_browser_
  )
  # OpenBot uses its own build directory and stack budget.
  cargo test --locked --offline --lib attribution_sso_browser_ -- --skip doop --skip Doop --ignored --list > "$CHECK_WORK/attribution-sso-list"
  grep -Eq '::attribution_sso_browser_.*: test$' "$CHECK_WORK/attribution-sso-list"
  cargo test --locked --offline --lib attribution_sso_browser_ -- --skip doop --skip Doop --ignored --nocapture
)

check_attribution_image() (
  set -e
  docker build --tag "$check_attribution_image" services/attribution
  ATTRIBUTION_TEST_IMAGE="$check_attribution_image" python3 services/attribution/scripts/container-smoke.py
)
