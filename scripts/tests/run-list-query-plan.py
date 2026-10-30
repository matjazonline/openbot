#!/usr/bin/env python3
"""Measure the shipped run-list statements on temporary tables with the migrated columns and indexes.

Usage: python3 scripts/tests/run-list-query-plan.py postgres:///mail_agents_test [--shapes FILE] [--out FILE]

The statements come from the code, not from a copy: without --shapes the script runs
`cargo test --lib dump_run_list_query_shapes -- --ignored`, which writes every shape `list_runs`
can issue and what each of its parameters binds. Only TEMP tables are written; no triggers fire and
no application rows are read or modified. The database must be migrated: the temporary tables are
`LIKE public.<table> INCLUDING ALL`, so they carry the shipped constraints and indexes.
"""
import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile

root = pathlib.Path(__file__).resolve().parents[2]

SEED = r"""
\pset pager off
SET client_min_messages = warning;
CREATE TEMP TABLE workflow_versions (LIKE public.workflow_versions INCLUDING ALL);
CREATE TEMP TABLE workflow_runs (LIKE public.workflow_runs INCLUDING ALL);
-- 100 tenants x 40 workflows x 20 published versions. Agent k of tenant t is md5('agent'||t||'-'||k).
-- Agent 0 is dense (every even version of every workflow); agent 63 is sparse (workflow 0's first
-- version only); every version also uses one of agents 1..62.
INSERT INTO workflow_versions
    (company_id, workflow_id, id, draft_revision, command_key, actor_id, title, description, source,
     bundle, content_hash, created_at, agent_ids)
SELECT md5('company' || t)::uuid, md5('workflow' || t || '-' || w)::uuid,
       md5('version' || t || '-' || w || '-' || v)::uuid, v + 1, 'publish-' || w || '-' || v,
       md5('actor' || t)::uuid, 'Plan fixture', '', '{}', '\x00'::bytea, repeat('0', 64),
       now() - interval '2 years' + v * interval '36 days',
       array_remove(ARRAY[
           CASE WHEN v % 2 = 0 THEN md5('agent' || t || '-0')::uuid END,
           CASE WHEN w = 0 AND v = 0 THEN md5('agent' || t || '-63')::uuid END,
           md5('agent' || t || '-' || (1 + (w * 20 + v) % 62))::uuid], NULL)
FROM generate_series(1, 100) AS tenants(t), generate_series(0, 39) AS workflows(w),
     generate_series(0, 19) AS versions(v);
-- Four heavy tenants with 250,000 runs each and 96 light tenants with 1,000, spread over two
-- years. Run n of a tenant with N runs belongs to workflow n % 40 and, as time passes, to that
-- workflow's later versions (version 20n/N), so the sparse agent's runs are the oldest. About 95%
-- are terminal and 10% are child runs.
INSERT INTO workflow_runs
    (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id,
     correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline,
     created_at, state, waiting_reason, parent_run_id, parent_execution_id)
SELECT company, md5('run' || t || '-' || n)::uuid, md5('binding' || t || '-' || w)::uuid, 1,
       md5('workflow' || t || '-' || w)::uuid, md5('version' || t || '-' || w || '-' || v)::uuid,
       md5('actor' || t)::uuid, md5('trigger' || t || '-' || n)::uuid,
       md5('correlation' || t || '-' || n)::uuid, '\x00'::bytea, '{}', '{}', '{}', 100, 65536,
       created + interval '1 day', created, state,
       CASE WHEN state = 'waiting' THEN 'decision' END,
       CASE WHEN n % 10 = 5 THEN md5('run' || t || '-' || (n - 1))::uuid END,
       CASE WHEN n % 10 = 5 THEN md5('execution' || t || '-' || n)::uuid END
FROM (SELECT t, n, n % 40 AS w, 20 * n / total AS v, md5('company' || t)::uuid AS company,
             now() - interval '2 years' + (n::double precision / total) * interval '2 years' AS created,
             CASE WHEN n % 20 = 0 THEN (ARRAY['queued', 'running', 'waiting'])[1 + (n / 20) % 3]
                  ELSE (ARRAY['succeeded', 'failed', 'cancelled'])[1 + n % 3] END AS state
      FROM (SELECT t, CASE WHEN t <= 4 THEN 250000 ELSE 1000 END AS total
            FROM generate_series(1, 100) AS tenants(t)) AS tenant,
           generate_series(0, total - 1) AS runs(n)) AS seed;
ANALYZE workflow_versions;
ANALYZE workflow_runs;
\echo '== Table and index sizes (temporary clones)'
SELECT class.relname, class.reltuples::bigint AS rows, pg_size_pretty(pg_relation_size(class.oid)) AS size
  FROM pg_class AS class
 WHERE class.relpersistence = 't' AND class.relkind IN ('r', 'i')
 ORDER BY class.relkind DESC, class.relname;
\echo '== Shipped indexes on public.workflow_runs and public.workflow_versions'
SELECT indexdef FROM pg_indexes
 WHERE schemaname = 'public' AND tablename IN ('workflow_runs', 'workflow_versions') ORDER BY indexdef;
"""

# Fixed values: tenant 1 is heavy; binding of workflow 0; dense agent 0, sparse agent 63.
VALUES = {
    "company": "md5('company1')::uuid",
    "binding": "md5('binding1-0')::uuid",
}
# Both page sizes the code reads, each plus its look-ahead row: the UI's 50-row pages
# (`LIST_PAGE`), and 200 (`MAX_LISTED`), the JSON route's default and the UI's refresh of a list
# that "Load more" has grown.
LIMITS = {"page 50": "51", "page 200": "201"}
AGENTS = {"dense": "md5('agent1-0')::uuid", "sparse": "md5('agent1-63')::uuid"}
# A common terminal state (a third of runs) and a rare open one (under 2%), which no keyset index
# serves: a range that holds fewer matches than a page is read to its end.
STATES = {"failed": "'failed'", "queued": "'queued'"}
CURSOR_DEPTH = 200


def dump_shapes() -> list:
    with tempfile.TemporaryDirectory() as directory:
        path = os.path.join(directory, "shapes.json")
        env = dict(os.environ, RUN_LIST_QUERY_SHAPES=path, SQLX_OFFLINE="true",
                   ALLOW_MISSING_DATABASE_URL="1")
        subprocess.run(
            ["cargo", "test", "--locked", "--offline", "--lib", "dump_run_list_query_shapes",
             "--", "--ignored"],
            cwd=root, env=env, check=True, stdout=subprocess.DEVNULL)
        return json.loads(pathlib.Path(path).read_text())


def cursor_lookup(agent: str, name: str) -> str:
    """psql that stores the (created_at, id) of the agent list's row at CURSOR_DEPTH in :name_*."""
    return (
        "SELECT created_at, id FROM workflow_runs "
        f"WHERE company_id = {VALUES['company']} AND version_id IN (SELECT version.id FROM "
        f"workflow_versions AS version WHERE version.company_id = {VALUES['company']} AND "
        f"{agent} = ANY(version.agent_ids)) ORDER BY created_at DESC, id DESC "
        f"OFFSET {CURSOR_DEPTH - 1} LIMIT 1 \\gset {name}_\n")


def measurements(shapes: list) -> str:
    sql = []
    for mode in ("auto", "force_generic_plan"):
        sql.append(f"SET plan_cache_mode = {mode};\n")
        for shape in shapes:
            name = shape["name"]
            sql.append(f"DEALLOCATE ALL;\nPREPARE {name} AS {shape['sql']};\n")
            agents = AGENTS if "agent" in shape["parameters"] else {"": None}
            states = STATES if "state" in shape["parameters"] else {"": None}
            for label, agent in agents.items():
                for state_label, state in states.items():
                    for limit_label, limit in LIMITS.items():
                        values = dict(VALUES, agent=agent, state=state, limit=limit)
                        if "cursor_created" in shape["parameters"]:
                            sql.append(cursor_lookup(agent, f"cursor_{label}"))
                            values["cursor_created"] = f":'cursor_{label}_created_at'::timestamptz"
                            values["cursor_id"] = f":'cursor_{label}_id'::uuid"
                        arguments = ", ".join(values[role] for role in shape["parameters"])
                        title = " ".join(
                            part for part in (name, label, state_label, limit_label) if part)
                        sql.append(f"\\echo '== {title} (plan_cache_mode = {mode})'\n")
                        sql.append(f"EXPLAIN (ANALYZE, BUFFERS) EXECUTE {name}({arguments});\n")
    return "".join(sql)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("database_url")
    parser.add_argument("--shapes", help="a dump_run_list_query_shapes file; default: dump now")
    parser.add_argument("--out", help="also write the output here")
    args = parser.parse_args()
    shapes = json.loads(pathlib.Path(args.shapes).read_text()) if args.shapes else dump_shapes()
    script = SEED + measurements(shapes)
    result = subprocess.run(["psql", args.database_url, "-X", "-q", "-v", "ON_ERROR_STOP=1"],
                            input=script, text=True, capture_output=True)
    output = result.stdout + result.stderr
    if args.out:
        pathlib.Path(args.out).write_text(output)
    sys.stdout.write(output)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
