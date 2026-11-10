#!/usr/bin/env bash
# The backup check (p06 A4-3): run the ignored DB-backed test that dumps a seeded database with
# pg_dump -Fc, restores it into a fresh database with pg_restore, runs after-restore and checks
# that content survives and nothing is replayed.
#   MEMORY_TEST_DATABASE_URL  superuser maintenance URL on a disposable/local server (required)
#   MEMORY_PG_BIN             directory holding pg_dump, pg_restore and psql whose major version
#                             matches the server's (required)
# It never reads DATABASE_URL or .env files, and it refuses a run that selects no test.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
: "${MEMORY_TEST_DATABASE_URL:?backup-restore-check: MEMORY_TEST_DATABASE_URL is required}"
: "${MEMORY_PG_BIN:?backup-restore-check: MEMORY_PG_BIN (the pg_dump directory) is required}"
for tool in pg_dump pg_restore psql; do
  [[ -x "$MEMORY_PG_BIN/$tool" ]] || { echo "backup-restore-check: $MEMORY_PG_BIN/$tool is missing" >&2; exit 2; }
done
server="$("$MEMORY_PG_BIN/psql" "$MEMORY_TEST_DATABASE_URL" -X -tA -c 'SHOW server_version_num')"
server_major=$((server / 10000))
client_major="$("$MEMORY_PG_BIN/pg_dump" --version | sed -E 's/.* ([0-9]+)(\.[0-9]+)*.*/\1/')"
if [[ "$client_major" != "$server_major" ]]; then
  echo "backup-restore-check: pg_dump $client_major does not match server $server_major" >&2
  exit 2
fi
test=adapters::postgres::operations_tests::memory_operations_backup_restore_keeps_content_and_never_replays
out="$(cargo test --locked --offline --lib -- --ignored --exact "$test" 2>&1)" || { echo "$out"; exit 1; }
echo "$out" | tail -3
grep -q 'test result: ok. 1 passed' <<<"$out" || { echo "backup-restore-check: the test did not run" >&2; exit 1; }
