#!/usr/bin/env bash
# The service's full gate (implementation-decisions D10). Run from any checkout or extracted copy.
#   MEMORY_TEST_DATABASE_URL  superuser maintenance URL on a disposable/local server (required)
#   MEMORY_PSQL               psql binary (default: psql)
#   MEMORY_SQLX               sqlx-cli 0.8.6 as a cargo subcommand (default: cargo sqlx; `prepare`
#                             refuses to run unless invoked through cargo)
# It never reads DATABASE_URL or .env files and never migrates an existing database: migrations
# and the SQLx metadata check run against a memverify_* database it creates and drops.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if [[ -z "${MEMORY_TEST_DATABASE_URL:-}" ]]; then
  echo "verify: MEMORY_TEST_DATABASE_URL is required (DATABASE_URL is never used)" >&2
  exit 2
fi
psql="${MEMORY_PSQL:-psql}"
read -r -a sqlx <<<"${MEMORY_SQLX:-cargo sqlx}"
version="$("${sqlx[@]}" --version 2>/dev/null || true)"
[[ "$version" == *" 0.8.6"* ]] || { echo "verify: sqlx-cli 0.8.6 is required (found: ${version:-none})" >&2; exit 2; }

suffix="$$_$(od -An -N4 -tx4 /dev/urandom | tr -d ' ')"
db="memverify_${suffix}"; role="memverify_${suffix}_owner"
password="$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')"
cleanup() {
  "$psql" "$MEMORY_TEST_DATABASE_URL" -X -q -c "DROP DATABASE IF EXISTS \"$db\" WITH (FORCE)" >/dev/null 2>&1 || true
  "$psql" "$MEMORY_TEST_DATABASE_URL" -X -q -c "DROP ROLE IF EXISTS \"$role\"" >/dev/null 2>&1 || true
}
trap cleanup EXIT
# Split scheme://[userinfo@]host[:port][/database][?query]; the database path is optional.
base="${MEMORY_TEST_DATABASE_URL%%\?*}"; query=""
[[ "$MEMORY_TEST_DATABASE_URL" == *\?* ]] && query="?${MEMORY_TEST_DATABASE_URL#*\?}"
scheme="${base%%://*}"; authority="${base#*://}"; authority="${authority%%/*}"
hostpart="${authority##*@}"
owner_url="postgres://${role}:${password}@${hostpart}/${db}${query}"
superuser_db_url="${scheme}://${authority}/${db}${query}"
"$psql" "$MEMORY_TEST_DATABASE_URL" -X -q -v ON_ERROR_STOP=1 -v role="$role" -v db="$db" -v pw="$password" <<'SQL'
CREATE ROLE :"role" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD :'pw';
CREATE DATABASE :"db" OWNER :"role";
REVOKE CONNECT ON DATABASE :"db" FROM PUBLIC;
GRANT CONNECT ON DATABASE :"db" TO :"role";
SQL
"$psql" "$superuser_db_url" -X -q -v ON_ERROR_STOP=1 -c "CREATE EXTENSION vector"

step() { printf '\n==> %s\n' "$1"; shift; "$@"; }
step "Formatting" cargo fmt --all -- --check
if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then step "Patch whitespace" git diff --check -- .; fi
step "Boundary (with poisoned-parent probe)" scripts/boundary-check.sh --probe
step "Fetch locked dependencies" cargo fetch --locked
step "Offline compilation" cargo check --locked --offline --all-targets
step "Clippy" cargo clippy --locked --offline --all-targets -- -D warnings
step "Migrations as the owner role" env SQLX_OFFLINE=false "${sqlx[@]}" migrate run --no-dotenv --database-url "$owner_url"
step "SQLx metadata matches the schema" env SQLX_OFFLINE=false DATABASE_URL="$owner_url" "${sqlx[@]}" prepare --check --no-dotenv --database-url "$owner_url" -- --all-targets
step "Model manifest" python3 -I scripts/prepare-model.py --check-manifest
# The prepared model is required: the real-model acceptance check fails, never skips, without it.
step "Model artifacts (sizes and SHA-256)" python3 -I scripts/prepare-model.py --verify
step "Compile tests" cargo test --locked --offline --all-targets --no-run
step "Test suite (network isolated)" scripts/test-network-isolation.sh cargo test --locked --offline --all-targets
echo "verify: all gates passed"
