#!/bin/bash
set -euo pipefail
export PATH=/opt/homebrew/opt/postgresql@18/bin:$PATH
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
export DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_test
logs=plan/workflow-foundation/docs/logs/06
dropdb -h localhost -p 55450 -U workflow0410 workflow0410_fresh_test
createdb -h localhost -p 55450 -U workflow0410 workflow0410_fresh_test
DATABASE_URL=postgres://workflow0410@localhost:55450/workflow0410_fresh_test cargo sqlx migrate run > "$logs/06B2a-fresh-migrate.log" 2>&1
for database in workflow0410_test workflow0410_fresh_test; do
    pg_dump -h localhost -p 55450 -U workflow0410 --schema-only --no-owner --no-privileges "$database" | sed '/^\\restrict /d; /^\\unrestrict /d' > "$logs/06B2a-$database-schema.sql"
done
cmp "$logs/06B2a-workflow0410_test-schema.sql" "$logs/06B2a-workflow0410_fresh_test-schema.sql"
echo 'PASS fresh schema equality'
SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets > "$logs/06B2a-sqlx-prepare.log" 2>&1
echo 'PASS SQLx prepare'
SQLX_OFFLINE=false cargo sqlx prepare --check -- --all-targets > "$logs/06B2a-sqlx-check.log" 2>&1
echo 'PASS SQLx check'
