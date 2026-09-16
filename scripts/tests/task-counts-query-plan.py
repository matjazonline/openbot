#!/usr/bin/env python3
"""Measure the shipped aggregate on temporary tables with the migrated columns and indexes.
Usage: python3 scripts/tests/task-counts-query-plan.py postgres:///mail_agents_test
Only TEMP tables are written; no triggers or production rows are modified.
"""
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(__file__).resolve().parents[2]
source = (root / 'src/adapters/persistence/task/counts.rs').read_text()
query = re.search(r'COUNTS_QUERY: &str = r#"(.*?)"#;', source, re.S).group(1)
sql = r"""
\pset pager off
\timing on
CREATE TEMP TABLE principals (LIKE public.principals INCLUDING ALL);
CREATE TEMP TABLE background_tasks (LIKE public.background_tasks INCLUDING ALL);
-- 100 tenants, 64 agent principals and a person each: realistic tenant-scoped join filtering.
INSERT INTO principals (id, company_id, kind, agent_id, user_id, display_label)
SELECT md5('principal' || tenant || '-' || person)::uuid, md5('company' || tenant)::uuid,
       CASE WHEN person = 64 THEN 'person' ELSE 'agent' END,
       CASE WHEN person < 64 THEN md5('agent' || tenant || '-' || person)::uuid END,
       CASE WHEN person = 64 THEN md5('user' || tenant)::uuid END, 'Plan fixture'
FROM generate_series(1,100) AS companies(tenant) CROSS JOIN generate_series(0,64) AS owners(person);
-- Four tenants with 100,000 terminal tasks and 4,000 open tasks each; 32 channels per tenant.
-- The owner distribution is 25% unassigned, 25% human, 50% agent across 64 agents.
INSERT INTO background_tasks
    (id, company_id, channel_id, correlation_id, task_type, status, owner_principal_id,
     owner_principal_kind, run_at, worker_id, execution_generation, locked_at, lock_expires_at,
     created_at, updated_at)
SELECT md5('task' || tenant || '-' || n)::uuid, md5('company' || tenant)::uuid,
       md5('channel' || tenant || '-' || (n % 32))::uuid, gen_random_uuid(), 'agent_run',
       CASE WHEN n <= 100000 THEN (ARRAY['completed','failed','dead_letter','stopped'])[1 + n % 4]
            ELSE (ARRAY['pending','processing','pending_approval','waiting_for_third_party_reply'])[1 + n % 4] END,
       CASE WHEN (n / 32) % 4 = 0 THEN NULL
            WHEN (n / 32) % 4 = 1 THEN md5('principal' || tenant || '-64')::uuid
            ELSE md5('principal' || tenant || '-' || ((n / 128) % 64))::uuid END,
       CASE WHEN (n / 32) % 4 = 0 THEN NULL WHEN (n / 32) % 4 = 1 THEN 'person' ELSE 'agent' END,
       CURRENT_TIMESTAMP + INTERVAL '1 year',
       CASE WHEN n > 100000 AND n % 4 = 1 THEN md5('worker')::uuid END,
       CASE WHEN n > 100000 AND n % 4 = 1 THEN md5('generation')::uuid END,
       CASE WHEN n > 100000 AND n % 4 = 1 THEN CURRENT_TIMESTAMP END,
       CASE WHEN n > 100000 AND n % 4 = 1 THEN CURRENT_TIMESTAMP + INTERVAL '1 year' END,
       CURRENT_TIMESTAMP - INTERVAL '1 year', CURRENT_TIMESTAMP - INTERVAL '1 year'
FROM generate_series(1,4) AS companies(tenant) CROSS JOIN generate_series(1,104000) AS tasks(n);
ANALYZE background_tasks;
ANALYZE principals;
SELECT version();
SELECT relname, reltuples, relpages, pg_size_pretty(pg_relation_size(oid)) AS relation_size
FROM pg_class WHERE oid IN ('background_tasks'::regclass, 'principals'::regclass)
   OR oid IN (SELECT indexrelid FROM pg_index WHERE indrelid IN ('background_tasks'::regclass, 'principals'::regclass))
ORDER BY relname;
SELECT attname, n_distinct, most_common_freqs FROM pg_stats
WHERE tablename = 'background_tasks' AND schemaname LIKE 'pg_temp_%'
AND attname IN ('company_id','status','channel_id','owner_principal_kind');
"""
query = query.replace('$1', "md5('company1')::uuid").replace('$2',
    "ARRAY['pending','processing','pending_approval','waiting_for_third_party_reply']::text[]")
for channels in (32, 8):
    channel_ids = ','.join(f"md5('channel1-{n}')::uuid" for n in range(channels))
    sql += f"\n\\echo Visible channels: {channels}\nEXPLAIN (ANALYZE, BUFFERS) " + query.replace('$3', f'ARRAY[{channel_ids}]') + ';\n'
subprocess.run(['psql', sys.argv[1], '-X', '-v', 'ON_ERROR_STOP=1'], input=sql, text=True, check=True)
