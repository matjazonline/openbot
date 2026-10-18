#!/usr/bin/env bash
# Batch S equivalence proof: fresh pre-squash (HEAD 4b54485 migrations, 56 files) vs fresh
# post-squash (one init schema) databases. Full pg_dump --schema-only (owners, ACLs, comments,
# functions, triggers, views, sequences, extensions, the sqlx ledger table) normalized only for
# pg_dump's random \restrict key and version banner; plus an independent catalog fingerprint.
set -uo pipefail
cd /Users/mac03/dev/openbot
export PATH="/opt/homebrew/opt/postgresql@18/bin:$PATH"
logdir=plan/workflow-foundation/docs/logs/S
base=postgres://workflow0410@127.0.0.1:55450
pre=$base/workflow0410_presquash
post=$base/workflow0410_postsquash
psql "$base/postgres" -X -q -v ON_ERROR_STOP=1 \
  -c "DROP DATABASE IF EXISTS workflow0410_postsquash" \
  -c "CREATE DATABASE workflow0410_postsquash OWNER workflow0410" || exit 1
SQLX_OFFLINE=false DATABASE_URL="$post" cargo sqlx migrate run > "$logdir/postsquash-migrate.log" 2>&1 || exit 1
normalize='/^\\(un)?restrict /d; /^-- Dumped (from|by) /d'
pg_dump --schema-only "$pre" | sed -E "$normalize" > "$logdir/presquash-schema.sql" || exit 1
pg_dump --schema-only "$post" | sed -E "$normalize" > "$logdir/postsquash-schema.sql" || exit 1
fingerprint="
SELECT 'function '||p.oid::regprocedure||' '||md5(pg_get_functiondef(p.oid))||' '||coalesce(md5(obj_description(p.oid,'pg_proc')),'-') FROM pg_proc p WHERE p.pronamespace='public'::regnamespace AND p.prokind IN ('f','p')
UNION ALL SELECT 'trigger '||t.tgrelid::regclass||'.'||t.tgname||' '||md5(pg_get_triggerdef(t.oid))||' '||t.tgenabled::text FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid WHERE c.relnamespace='public'::regnamespace AND NOT t.tgisinternal
UNION ALL SELECT 'constraint '||conrelid::regclass||'.'||conname||' '||md5(pg_get_constraintdef(oid))||' '||condeferrable||condeferred||convalidated FROM pg_constraint WHERE connamespace='public'::regnamespace
UNION ALL SELECT 'index '||indexrelid::regclass||' '||md5(pg_get_indexdef(indexrelid)) FROM pg_index i JOIN pg_class c ON c.oid=i.indexrelid WHERE c.relnamespace='public'::regnamespace
UNION ALL SELECT 'view '||c.oid::regclass||' '||md5(pg_get_viewdef(c.oid)) FROM pg_class c WHERE c.relnamespace='public'::regnamespace AND c.relkind IN ('v','m')
UNION ALL SELECT 'relation '||c.oid::regclass||' '||c.relkind::text||' '||c.relpersistence::text||' '||coalesce(c.relacl::text,'-')||' '||coalesce(md5(obj_description(c.oid,'pg_class')),'-') FROM pg_class c WHERE c.relnamespace='public'::regnamespace
UNION ALL SELECT 'column '||a.attrelid::regclass||'.'||a.attnum||' '||a.attname||' '||format_type(a.atttypid,a.atttypmod)||' '||a.attnotnull||' '||coalesce(md5(pg_get_expr(d.adbin,d.adrelid)),'-')||' '||a.attidentity::text||a.attgenerated::text||' '||coalesce(md5(col_description(a.attrelid,a.attnum)),'-')||' '||coalesce(a.attacl::text,'-') FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum WHERE c.relnamespace='public'::regnamespace AND a.attnum>0 AND NOT a.attisdropped
UNION ALL SELECT 'sequence '||seqrelid::regclass||' '||format_type(seqtypid,NULL)||' '||seqstart||' '||seqincrement||' '||seqmax||' '||seqmin||' '||seqcache||' '||seqcycle FROM pg_sequence s JOIN pg_class c ON c.oid=s.seqrelid WHERE c.relnamespace='public'::regnamespace
UNION ALL SELECT 'extension '||extname||' '||extversion||' '||extnamespace::regnamespace FROM pg_extension
UNION ALL SELECT 'type '||t.oid::regtype||' '||t.typtype::text FROM pg_type t WHERE t.typnamespace='public'::regnamespace AND t.typtype NOT IN ('b','c') 
UNION ALL SELECT 'default-acl '||defaclrole::regrole||' '||defaclobjtype::text||' '||defaclacl::text FROM pg_default_acl
UNION ALL SELECT 'namespace '||nspname||' '||coalesce(nspacl::text,'-')||' '||coalesce(md5(obj_description(oid,'pg_namespace')),'-') FROM pg_namespace WHERE nspname NOT LIKE 'pg\\_%' AND nspname<>'information_schema'
ORDER BY 1"
psql "$pre" -X -At -v ON_ERROR_STOP=1 -c "$fingerprint" > "$logdir/presquash-fingerprint.txt" || exit 1
psql "$post" -X -At -v ON_ERROR_STOP=1 -c "$fingerprint" > "$logdir/postsquash-fingerprint.txt" || exit 1
rows="SELECT format('%s %s', c.oid::regclass, (xpath('/row/n/text()', query_to_xml(format('SELECT count(*) AS n FROM %s', c.oid::regclass), false, true, '')))[1]::text) FROM pg_class c WHERE c.relnamespace='public'::regnamespace AND c.relkind IN ('r','p') AND c.relname<>'_sqlx_migrations' ORDER BY 1"
psql "$pre" -X -At -c "$rows" | grep -v ' 0$' > "$logdir/presquash-nonempty.txt"
psql "$post" -X -At -c "$rows" | grep -v ' 0$' > "$logdir/postsquash-nonempty.txt"
psql "$pre" -X -At -c "SELECT count(*) FROM _sqlx_migrations" > "$logdir/presquash-ledger.txt"
psql "$post" -X -At -c "SELECT version, description, success FROM _sqlx_migrations" > "$logdir/postsquash-ledger.txt"
status=0
echo "== schema dump diff (pre vs post)"; diff -u "$logdir/presquash-schema.sql" "$logdir/postsquash-schema.sql" || status=1
echo "== catalog fingerprint diff"; diff -u "$logdir/presquash-fingerprint.txt" "$logdir/postsquash-fingerprint.txt" || status=1
echo "== non-empty tables (pre / post)"; cat "$logdir/presquash-nonempty.txt" "$logdir/postsquash-nonempty.txt"
echo "== ledger: pre $(cat "$logdir/presquash-ledger.txt") rows; post: $(cat "$logdir/postsquash-ledger.txt")"
wc -l "$logdir/presquash-schema.sql" "$logdir/postsquash-schema.sql" "$logdir/presquash-fingerprint.txt" "$logdir/postsquash-fingerprint.txt"
exit $status
