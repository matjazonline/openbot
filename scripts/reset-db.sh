#!/usr/bin/env bash

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

DATABASE_NAME="$(psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -Atqc 'select current_database()')"

if [[ "${1:-}" != "--yes" ]]; then
  printf "Drop and recreate database '%s'? [y/N] " "$DATABASE_NAME"
  read -r confirmation
  if [[ "$confirmation" != "y" && "$confirmation" != "Y" ]]; then
    echo "Cancelled."
    exit 0
  fi
fi

# Connect through the maintenance database while replacing the configured target.
DATABASE_BASE_URL="${DATABASE_URL%%\?*}"
DATABASE_QUERY=""
if [[ "$DATABASE_URL" == *"?"* ]]; then
  DATABASE_QUERY="?${DATABASE_URL#*\?}"
fi
ADMIN_URL="${DATABASE_BASE_URL%/*}/postgres${DATABASE_QUERY}"

dropdb --force --maintenance-db="$ADMIN_URL" "$DATABASE_NAME"
createdb --maintenance-db="$ADMIN_URL" "$DATABASE_NAME"

echo "Database '$DATABASE_NAME' recreated. Migrations will run when the server starts."
