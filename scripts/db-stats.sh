#!/usr/bin/env bash
# Read-only PostgreSQL statistics snapshot. Output can contain normalized production SQL and is
# operationally sensitive; review and sanitize it before saving or sharing it.

set -euo pipefail

mode=fly
case "${1:-}" in
  "") ;;
  --local) mode=local ;;
  -h|--help)
    echo "Usage: $0 [--local]"
    echo "  --local  connect with DATABASE_URL; default uses Fly SSH"
    exit 0
    ;;
  *)
    echo "Unknown argument: $1" >&2
    exit 64
    ;;
esac

read -r -d '' snapshot_sql <<'SQL' || true
\pset pager off
\set ON_ERROR_STOP on
\echo '== Capture context =='
SELECT CURRENT_TIMESTAMP AS captured_at,
       version() AS postgres_version,
       current_database() AS database_name;
SELECT name, setting, unit
  FROM pg_settings
 WHERE name IN (
           'shared_preload_libraries',
           'compute_query_id',
           'pg_stat_statements.track',
           'pg_stat_statements.track_utility',
           'log_min_duration_statement',
           'log_parameter_max_length',
           'log_parameter_max_length_on_error'
       )
 ORDER BY name;
SELECT statistics.stats_reset AS database_stats_reset,
       statements.stats_reset AS statement_stats_reset,
       statements.dealloc
  FROM pg_stat_database AS statistics
 CROSS JOIN pg_stat_statements_info AS statements
 WHERE statistics.datname = current_database();

\echo '== Top statements by total execution time =='
SELECT queryid,
       SUM(calls)::bigint AS calls,
       SUM(rows)::bigint AS rows,
       ROUND(SUM(total_exec_time)::numeric, 2) AS total_ms,
       ROUND((SUM(total_exec_time) / NULLIF(SUM(calls), 0))::numeric, 2) AS mean_ms,
       ROUND(MAX(max_exec_time)::numeric, 2) AS max_ms,
       SUM(shared_blks_read)::bigint AS shared_reads,
       SUM(shared_blks_hit)::bigint AS shared_hits,
       SUM(temp_blks_written)::bigint AS temp_writes,
       SUM(wal_bytes)::bigint AS wal_bytes,
       MIN(LEFT(query, 4096)) AS normalized_query
  FROM pg_stat_statements
 WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())
 GROUP BY queryid
 ORDER BY SUM(total_exec_time) DESC, queryid ASC
 LIMIT 20;

\echo '== Top statements by weighted mean execution time (5+ calls) =='
SELECT queryid,
       SUM(calls)::bigint AS calls,
       ROUND((SUM(total_exec_time) / NULLIF(SUM(calls), 0))::numeric, 2) AS mean_ms,
       ROUND(MAX(max_exec_time)::numeric, 2) AS max_ms,
       ROUND(SUM(total_exec_time)::numeric, 2) AS total_ms,
       SUM(rows)::bigint AS rows,
       SUM(shared_blks_read)::bigint AS shared_reads,
       SUM(shared_blks_hit)::bigint AS shared_hits,
       SUM(temp_blks_written)::bigint AS temp_writes,
       SUM(wal_bytes)::bigint AS wal_bytes,
       MIN(LEFT(query, 4096)) AS normalized_query
  FROM pg_stat_statements
 WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())
 GROUP BY queryid
HAVING SUM(calls) >= 5
 ORDER BY SUM(total_exec_time) / NULLIF(SUM(calls), 0) DESC, queryid ASC
 LIMIT 20;

\echo '== Top statements by maximum execution time (5+ calls) =='
SELECT queryid,
       SUM(calls)::bigint AS calls,
       ROUND(MAX(max_exec_time)::numeric, 2) AS max_ms,
       ROUND((SUM(total_exec_time) / NULLIF(SUM(calls), 0))::numeric, 2) AS mean_ms,
       ROUND(SUM(total_exec_time)::numeric, 2) AS total_ms,
       MIN(LEFT(query, 4096)) AS normalized_query
  FROM pg_stat_statements
 WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())
 GROUP BY queryid
HAVING SUM(calls) >= 5
 ORDER BY MAX(max_exec_time) DESC, queryid ASC
 LIMIT 20;

\echo '== User table statistics =='
SELECT relname, seq_scan, idx_scan, n_live_tup, n_dead_tup, n_mod_since_analyze,
       last_vacuum, last_autovacuum, last_analyze, last_autoanalyze
  FROM pg_stat_user_tables
 ORDER BY relname;

\echo '== User index statistics =='
SELECT relname, indexrelname, idx_scan, idx_tup_read, idx_tup_fetch, last_idx_scan
  FROM pg_stat_user_indexes
 ORDER BY relname, indexrelname;

\echo '== Relation sizes =='
SELECT relation.relname,
       pg_size_pretty(pg_relation_size(relation.oid)) AS heap,
       pg_size_pretty(pg_indexes_size(relation.oid)) AS indexes,
       pg_size_pretty(CASE WHEN relation.reltoastrelid = 0 THEN 0 ELSE pg_total_relation_size(relation.reltoastrelid) END) AS toast,
       pg_size_pretty(pg_total_relation_size(relation.oid)) AS total
  FROM pg_class AS relation
  JOIN pg_namespace AS namespace ON namespace.oid = relation.relnamespace
 WHERE namespace.nspname = 'public'
   AND relation.relkind IN ('r', 'm')
 ORDER BY pg_total_relation_size(relation.oid) DESC, relation.relname ASC;
SQL

if [[ "$mode" == local ]]; then
  if [[ -z "${DATABASE_URL:-}" ]]; then
    echo "DATABASE_URL is required with --local" >&2
    exit 64
  fi
  printf '%s\n' "$snapshot_sql" | psql "$DATABASE_URL" -X -v ON_ERROR_STOP=1 -P pager=off
  exit 0
fi

if ! command -v fly >/dev/null 2>&1; then
  echo "flyctl is required for the default Fly mode" >&2
  exit 64
fi

fly_app="${DATABASE_FLY_APP:-mail-agents-db}"
database_user="${POSTGRES_USER:-mail_agents}"
database_name="${POSTGRES_DB:-mail_agents}"
encoded_sql="$(printf '%s\n' "$snapshot_sql" | base64 | tr -d '\n')"
remote_command="printf '%s' '$encoded_sql' | base64 -d | psql -U '$database_user' -d '$database_name' -X -v ON_ERROR_STOP=1 -P pager=off"
fly ssh console --app "$fly_app" --command "$remote_command"
