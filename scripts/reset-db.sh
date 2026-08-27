#!/usr/bin/env bash
#
# Drop and recreate the local databases.
#
#   ./scripts/reset-db.sh              development database only, with a prompt
#   ./scripts/reset-db.sh --yes        the same, unattended
#   ./scripts/reset-db.sh --all        development database and its _test sibling
#
# The development schema is rebuilt here because `sqlx::query!` checks the live database while
# compiling, before the server can run its embedded migrations. The test database is rebuilt by
# the first `cargo test` run that reaches it, through the migration macro in test support.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! -f "$ROOT_DIR/.env" ]]; then
  echo "Missing $ROOT_DIR/.env" >&2
  exit 1
fi

set -a
source "$ROOT_DIR/.env"
set +a

if [[ -z "${DATABASE_URL:-}" ]]; then
  echo "DATABASE_URL is not set" >&2
  exit 1
fi

ASSUME_YES=false
INCLUDE_TEST=false
for argument in "$@"; do
  case "$argument" in
    --yes) ASSUME_YES=true ;;
    --all) INCLUDE_TEST=true ;;
    *)
      echo "Unknown argument: $argument" >&2
      echo "Usage: $0 [--yes] [--all]" >&2
      exit 1
      ;;
  esac
done

DATABASE_NAME="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atqc 'select current_database()')"
DATABASE_USER="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atqc 'select current_user')"

# libpq (used by psql/dropdb) defaults a missing URL username to the operating-system user. SQLx
# instead parses it as `anonymous`, so give its migration URL the same explicit user that libpq
# resolved. Keep URLs that already contain credentials unchanged.
MIGRATION_DATABASE_URL="$DATABASE_URL"
DATABASE_AUTHORITY="${DATABASE_URL#*://}"
DATABASE_AUTHORITY="${DATABASE_AUTHORITY%%/*}"
if [[ "$DATABASE_AUTHORITY" != *"@"* ]]; then
  DATABASE_SCHEME="${DATABASE_URL%%://*}"
  DATABASE_LOCATION="${DATABASE_URL#*://}"
  MIGRATION_DATABASE_URL="${DATABASE_SCHEME}://${DATABASE_USER}@${DATABASE_LOCATION}"
fi

# Connect through the maintenance database while replacing the configured target.
DATABASE_BASE_URL="${DATABASE_URL%%\?*}"
DATABASE_QUERY=""
if [[ "$DATABASE_URL" == *"?"* ]]; then
  DATABASE_QUERY="?${DATABASE_URL#*\?}"
fi
ADMIN_URL="${DATABASE_BASE_URL%/*}/postgres${DATABASE_QUERY}"

TARGETS=("$DATABASE_NAME")
if [[ "$INCLUDE_TEST" == true ]]; then
  # Must match `with_test_database_name` in src/adapters/persistence/test_support.rs, including the
  # TEST_DATABASE_URL escape hatch — resetting a database the tests do not use helps nobody.
  if [[ -n "${TEST_DATABASE_URL:-}" ]]; then
    # Read the name out of the URL rather than asking the server for it: the whole point of a
    # reset is that the database may be missing or unusable, which is when a connection fails.
    TEST_DATABASE_BASE="${TEST_DATABASE_URL%%\?*}"
    TEST_DATABASE_NAME="${TEST_DATABASE_BASE##*/}"
  else
    TEST_DATABASE_NAME="${DATABASE_NAME}_test"
  fi
  if [[ -z "$TEST_DATABASE_NAME" ]]; then
    echo "Could not determine the test database name" >&2
    exit 1
  fi
  TARGETS+=("$TEST_DATABASE_NAME")
fi

if [[ "$ASSUME_YES" != true ]]; then
  printf "Drop and recreate %s? [y/N] " "$(printf "'%s' " "${TARGETS[@]}")"
  read -r confirmation
  if [[ "$confirmation" != "y" && "$confirmation" != "Y" ]]; then
    echo "Cancelled."
    exit 0
  fi
fi

for target in "${TARGETS[@]}"; do
  # --force closes open pool connections; a running `cargo run` holds several and will need a
  # restart afterwards to rebuild its pool and re-run migrations.
  dropdb --force --if-exists --maintenance-db="$ADMIN_URL" "$target"
  createdb --maintenance-db="$ADMIN_URL" "$target"
  echo "Database '$target' recreated."
done

echo "Applying development database migrations..."
(cd "$ROOT_DIR" && cargo sqlx migrate run --database-url "$MIGRATION_DATABASE_URL")

echo
echo "Reset complete. The development database is ready for cargo run."
if [[ "$INCLUDE_TEST" == true ]]; then
  echo "The test schema will be rebuilt by the first cargo test run that reaches it."
fi
