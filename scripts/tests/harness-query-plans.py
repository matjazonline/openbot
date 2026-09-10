#!/usr/bin/env python3
"""Compare the exact fencing queries on synthetic history; only TEMP tables are written.

Usage: python3 scripts/tests/harness-query-plans.py postgres:///mail_agents_test
This is a reproducible scaling probe, not a production latency benchmark.
"""
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
OLD = (ROOT / "scripts/tests/fixtures/harness-fencing-before.sql").read_text()
NEW = (ROOT / "migrations/20260817000000_init_schema.sql").read_text()


def query(source, kind):
    name = "lock_task_agent_harnesses" if kind == "lock" else "guard_active_agent_harness_change"
    source = re.search(
        rf"CREATE (?:OR REPLACE )?FUNCTION public\.{name}\(\).*?\$\$;", source, re.S
    ).group()
    if kind == "lock":
        result = "SELECT" + source.split("PERFORM", 1)[1].split(";", 1)[0]
    else:
        result = "SELECT " + source.split("IF EXISTS (", 1)[1].split(
            ") THEN", 1
        )[0]
        result = "SELECT EXISTS (" + result.removeprefix("SELECT ") + ")"
    for name, expression in {
        "NEW.owner_principal_id": "md5('50000')::uuid",
        "NEW.company_id": "md5('company100')::uuid",
        "NEW.channel_id": "md5('50000')::uuid",
        "OLD.company_id": "md5('company100')::uuid",
        "OLD.id": "md5('50000')::uuid",
    }.items():
        result = result.replace(name, expression)
    return result


sql = """
BEGIN;
CREATE TEMP TABLE agents (id uuid PRIMARY KEY, company_id uuid);
CREATE TEMP TABLE principals (id uuid PRIMARY KEY, company_id uuid, agent_id uuid);
CREATE UNIQUE INDEX principals_company_agent_key ON principals(company_id, agent_id) WHERE agent_id IS NOT NULL;
CREATE TEMP TABLE channel_agents (channel_id uuid, company_id uuid, agent_id uuid, PRIMARY KEY(channel_id, agent_id));
CREATE INDEX channel_agents_agent_idx ON channel_agents(agent_id, channel_id);
CREATE TEMP TABLE background_tasks (id uuid PRIMARY KEY, company_id uuid, channel_id uuid, owner_principal_id uuid, status text, created_at timestamptz);
CREATE INDEX background_tasks_company_status_created_idx ON background_tasks(company_id, status, created_at DESC, id DESC);
CREATE INDEX background_tasks_company_channel_created_idx ON background_tasks(company_id, channel_id, created_at DESC, id DESC);
INSERT INTO agents SELECT md5(n::text)::uuid, md5('company' || (((n-1)/500)+1)::text)::uuid FROM generate_series(1,50000) AS series(n);
INSERT INTO principals SELECT id, company_id, id FROM agents;
INSERT INTO channel_agents SELECT id, company_id, id FROM agents;
INSERT INTO background_tasks
SELECT md5('task' || n::text)::uuid,
       md5('company' || (((n-1)/5000)+1)::text)::uuid,
       md5((((n-1)/10)+1)::text)::uuid, md5((((n-1)/10)+1)::text)::uuid,
       CASE WHEN n % 10 = 1 AND n < 499991 THEN 'pending' ELSE 'completed' END,
       '2026-09-01'::timestamptz
FROM generate_series(1,500000) AS series(n);
ANALYZE agents;
ANALYZE principals;
ANALYZE channel_agents;
ANALYZE background_tasks;
"""
for label, source, kind in [
    ("Before: admission locks", OLD, "lock"),
    ("Before: configuration guard", OLD, "guard"),
]:
    sql += f"\\echo {label}\nEXPLAIN (ANALYZE, BUFFERS, TIMING OFF) {query(source, kind)};\n"
for name in ["background_tasks_unsettled_owner_idx", "background_tasks_unsettled_channel_idx"]:
    # Use the baseline's exact index definitions, targeting only the temporary table.
    index = re.search(rf"CREATE INDEX {name} .*?;", NEW, re.S).group()
    sql += index.replace("ON public.background_tasks", "ON background_tasks") + "\n"
sql += "ANALYZE background_tasks;\n"
for label, kind in [
    ("After: admission locks", "lock"),
    ("After: configuration guard", "guard"),
]:
    sql += f"\\echo {label}\nEXPLAIN (ANALYZE, BUFFERS, TIMING OFF) {query(NEW, kind)};\n"
sql += """
SELECT relname, reltuples::bigint AS estimated_rows, pg_size_pretty(pg_relation_size(oid)) AS size
FROM pg_class WHERE relnamespace = pg_my_temp_schema() ORDER BY relname;
ROLLBACK;
"""
subprocess.run(
    ["psql", "-X", "-v", "ON_ERROR_STOP=1", "-d", sys.argv[1]],
    input=sql, text=True, check=True,
)
