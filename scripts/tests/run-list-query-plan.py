#!/usr/bin/env python3
"""Measure the shipped run-list and run-board statements on temporary tables with the migrated columns and indexes.

Usage: python3 scripts/tests/run-list-query-plan.py postgres:///mail_agents_test [--shapes FILE] [--out FILE]
           [--board-out FILE]

The statements come from the code, not from a copy: without --shapes the script runs
`cargo test --lib dump_run_list_query_shapes -- --ignored`, which writes every shape `list_runs`,
`run_board_counts` and `run_board_cards` can issue and what each of its parameters binds. The board
section goes to --board-out when given, the rest to --out. Only TEMP tables are written; no triggers fire and
no application rows are read or modified. The database must be migrated: the temporary tables are
`LIKE public.<table> INCLUDING ALL`, so they carry the shipped constraints and indexes.
"""
import argparse
import itertools
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
-- are terminal, finished ten minutes after they were created, and 10% are child runs, all through
-- history, so every board window holds children. Workflows 0..31 are bound to channels 0..31 of
-- their tenant (md5('channel' || t || '-' || w)); workflows 32..39 to the company.
INSERT INTO workflow_runs
    (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id,
     correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline,
     created_at, state, waiting_reason, parent_run_id, parent_execution_id, channel_id, finished_at)
SELECT company, md5('run' || t || '-' || n)::uuid, md5('binding' || t || '-' || w)::uuid, 1,
       md5('workflow' || t || '-' || w)::uuid, md5('version' || t || '-' || w || '-' || v)::uuid,
       md5('actor' || t)::uuid, md5('trigger' || t || '-' || n)::uuid,
       md5('correlation' || t || '-' || n)::uuid, '\x00'::bytea, '{}', '{}', '{}', 100, 65536,
       created + interval '1 day', created, state,
       CASE WHEN state = 'waiting' THEN 'decision' END,
       CASE WHEN n % 10 = 5 THEN md5('run' || t || '-' || (n - 1))::uuid END,
       CASE WHEN n % 10 = 5 THEN md5('execution' || t || '-' || n)::uuid END,
       CASE WHEN w < 32 THEN md5('channel' || t || '-' || w)::uuid END,
       CASE WHEN state IN ('succeeded', 'failed', 'cancelled')
            THEN created + interval '10 minutes' END
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

# Fixed values: tenant 1 is heavy; binding of workflow 0; workflow 0 (a fortieth of the tenant's
# runs, every one of them open, so its `failed` list matches nothing and reads to the end); dense
# agent 0, sparse agent 63.
VALUES = {
    "company": "md5('company1')::uuid",
    "binding": "md5('binding1-0')::uuid",
    "workflow": "md5('workflow1-0')::uuid",
}
# Both page sizes the code reads, each plus its look-ahead row: the UI's 50-row pages
# (`LIST_PAGE`), and 200 (`MAX_LISTED`), the JSON route's default and the UI's refresh of a list
# that "Load more" has grown.
LIMITS = {"page 50": "51", "page 200": "201"}
AGENTS = {"dense": "md5('agent1-0')::uuid", "sparse": "md5('agent1-63')::uuid"}
# A common terminal state (a third of runs) and a rare open one (under 2%), which no keyset index
# serves: a range that holds fewer matches than a page is read to its end.
STATES = {"failed": "'failed'", "queued": "'queued'"}
# The workflow filter: workflow 0 (a fortieth of the tenant's runs), and a workflow of the tenant
# with no runs at all, the sparsest scope: without its own index it reads the company's history.
WORKFLOWS = {"a fortieth": VALUES["workflow"], "no runs": "md5('workflow1-99')::uuid"}
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
        for shape in (shape for shape in shapes if shape["family"] == "list"):
            name = shape["name"]
            sql.append(f"DEALLOCATE ALL;\nPREPARE {name} AS {shape['sql']};\n")
            agents = AGENTS if "agent" in shape["parameters"] else {"": None}
            workflows = WORKFLOWS if "workflow" in shape["parameters"] else {"": None}
            states = STATES if "state" in shape["parameters"] else {"": None}
            for (label, agent), (workflow_label, workflow), (state_label, state), (
                    limit_label, limit) in itertools.product(
                        agents.items(), workflows.items(), states.items(), LIMITS.items()):
                values = dict(VALUES, agent=agent, state=state, limit=limit)
                if workflow:
                    values["workflow"] = workflow
                if "cursor_created" in shape["parameters"]:
                    sql.append(cursor_lookup(agent, f"cursor_{label}"))
                    values["cursor_created"] = f":'cursor_{label}_created_at'::timestamptz"
                    values["cursor_id"] = f":'cursor_{label}_id'::uuid"
                arguments = ", ".join(values[role] for role in shape["parameters"])
                title = " ".join(part for part in
                                 (name, label, workflow_label, state_label, limit_label) if part)
                sql.append(f"\\echo '== {title} (plan_cache_mode = {mode})'\n")
                sql.append(f"EXPLAIN (ANALYZE, BUFFERS) EXECUTE {name}({arguments});\n")
    return "".join(sql)


# The board's windows, back from the time of the read.
WINDOWS = {"day": "1 day", "week": "7 days", "month": "30 days"}
# A heavy tenant, and a light one (1,000 runs over two years).
BOARD_COMPANIES = {"heavy": "md5('company1')::uuid", "light": "md5('company5')::uuid"}
# The scoped boards, in the week window. Workflow 0's binding holds only open runs (run n % 40 = 0
# is always in an open state) and workflow 1's only finished ones. The dense and sparse agents have
# no runs in any window: their versions end before it. Agent 40 is in the last versions of
# workflows 1 and 32, so it has finished runs in every window.
BOARD_BINDINGS = {"open-only": VALUES["binding"], "finished": "md5('binding1-1')::uuid"}
BOARD_AGENTS = dict(AGENTS, recent="md5('agent1-40')::uuid")
# The workflow-scoped boards: the same two workflows as the bindings above, by workflow id.
BOARD_WORKFLOWS = {"open-only": VALUES["workflow"], "finished": "md5('workflow1-1')::uuid"}
# The cards' visible channels, as psql variables set by VISIBLE: every channel and the company's
# runs, or 8 of the 32 channels and not the company's runs, so each column reads past hidden runs.
VISIBLE = r"""
SELECT ARRAY(SELECT md5('channel' || t || '-' || c)::uuid FROM generate_series(0, 31) AS c)::text
       AS all_channels,
       ARRAY(SELECT md5('channel' || t || '-' || c)::uuid FROM generate_series(0, 7) AS c)::text
       AS some_channels
  FROM (VALUES (1)) AS tenant(t) \gset heavy_
SELECT ARRAY(SELECT md5('channel' || t || '-' || c)::uuid FROM generate_series(0, 31) AS c)::text
       AS all_channels,
       ARRAY(SELECT md5('channel' || t || '-' || c)::uuid FROM generate_series(0, 7) AS c)::text
       AS some_channels
  FROM (VALUES (5)) AS tenant(t) \gset light_
"""
VISIBILITY = {
    "all visible": ("all_channels", "true"),
    "8 of 32 visible, no company runs": ("some_channels", "false"),
}
# The finished-run index candidates. Each variant first drops every temporary index on
# `finished_at`, the shipped one's clone included, so "none" measures the board without one.
FINISHED_INDEXES = {
    "no finished index": None,
    "(A) (company_id, finished_at DESC, id DESC) top-level":
        "CREATE INDEX board_finished_a ON workflow_runs (company_id, finished_at DESC, id DESC) "
        "WHERE finished_at IS NOT NULL AND parent_run_id IS NULL;",
    "(B) (company_id, state, finished_at DESC, id DESC) top-level":
        "CREATE INDEX board_finished_b ON workflow_runs "
        "(company_id, state, finished_at DESC, id DESC) "
        "WHERE finished_at IS NOT NULL AND parent_run_id IS NULL;",
}
DROP_FINISHED_INDEXES = r"""
SELECT format('DROP INDEX %I.%I', schemaname, indexname) FROM pg_indexes
 WHERE schemaname LIKE 'pg_temp%' AND tablename = 'workflow_runs'
   AND indexdef LIKE '%finished_at%' \gexec
"""
BOARD_START, BOARD_END = "#### run board", "#### end of run board"


def board_cases(shape: dict) -> list:
    """(title, values) for each read of `shape`: the company scope of the heavy tenant in every
    window, the light tenant's month, and the binding, workflow and agent scopes in the default
    week."""
    family, name = shape["family"], shape["name"]
    visibilities = VISIBILITY if family == "board_cards" else {"": None}
    cases = []

    def add(label: str, tenant: str, window: str, **values: str) -> None:
        for visibility, channels in visibilities.items():
            row = dict(VALUES, company=BOARD_COMPANIES[tenant],
                       cutoff=f"now() - interval '{WINDOWS[window]}'", **values)
            if channels:
                row["channels"] = f":'{tenant}_{channels[0]}'::uuid[]"
                row["company_visible"] = channels[1]
            title = " ".join(part for part in (name, label, tenant, window, visibility) if part)
            cases.append((title, row))

    if name in ("board_counts", "board_cards"):
        for window in WINDOWS:
            add("", "heavy", window)
        add("", "light", "month")
    elif "agent" in shape["parameters"]:
        for label, agent in BOARD_AGENTS.items():
            add(label, "heavy", "week", agent=agent)
    elif "workflow" in shape["parameters"]:
        for label, workflow in BOARD_WORKFLOWS.items():
            add(label, "heavy", "week", workflow=workflow)
    else:
        for label, binding in BOARD_BINDINGS.items():
            add(label, "heavy", "week", binding=binding)
    return cases


def board_measurements(shapes: list) -> str:
    """Every board shape under each finished-index candidate, in both plan modes."""
    # Plans only: no header, separator or row count around each of the many plans.
    sql = [f"\\echo '{BOARD_START}'\n\\pset tuples_only on\n", VISIBLE]
    boards = [shape for shape in shapes if shape["family"].startswith("board")]
    for variant, ddl in FINISHED_INDEXES.items():
        sql.append(DROP_FINISHED_INDEXES)
        if ddl:
            sql.append(ddl + "\n")
        for mode in ("auto", "force_generic_plan"):
            sql.append(f"SET plan_cache_mode = {mode};\n")
            for shape in boards:
                name = shape["name"]
                sql.append(f"DEALLOCATE ALL;\nPREPARE {name} AS {shape['sql']};\n")
                for title, values in board_cases(shape):
                    arguments = ", ".join(values[role] for role in shape["parameters"])
                    sql.append(f"\\echo '== {title} [{variant}] (plan_cache_mode = {mode})'\n")
                    sql.append(f"EXPLAIN (ANALYZE, BUFFERS) EXECUTE {name}({arguments});\n")
    sql.append(f"\\pset tuples_only off\n\\echo '{BOARD_END}'\n")
    return "".join(sql)


# A workflow only ever called as a child, with a long history: every run of workflow 5 is a child
# (run n is a child when n % 10 = 5), and its version 18 uses the dense agent. Run last, after the
# main measurements, because it changes the data.
CHILD_HISTORY = r"""
INSERT INTO workflow_runs
    (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id,
     correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline,
     created_at, state, parent_run_id, parent_execution_id, channel_id, finished_at)
SELECT md5('company1')::uuid, md5('extra-run1-' || n)::uuid, md5('binding1-5')::uuid, 1,
       md5('workflow1-5')::uuid, md5('version1-5-18')::uuid, md5('actor1')::uuid,
       md5('extra-trigger1-' || n)::uuid, md5('extra-correlation1-' || n)::uuid, '\x00'::bytea,
       '{}', '{}', '{}', 100, 65536, created + interval '1 day', created, 'succeeded',
       md5('run1-4')::uuid, md5('extra-execution1-' || n)::uuid, md5('channel1-5')::uuid,
       created + interval '10 minutes'
FROM (SELECT n, now() - interval '30 days' + n * interval '1 second' AS created
      FROM generate_series(1, 100000) AS extra(n)) AS seed;
ANALYZE workflow_runs;
SET plan_cache_mode = force_generic_plan;
"""
# The temporary clone of `workflow_runs_version_top_level`, dropped to show what it saves.
DROP_TOP_LEVEL_INDEX = r"""
SELECT format('DROP INDEX %I.%I', schemaname, indexname) FROM pg_indexes
 WHERE schemaname LIKE 'pg_temp%' AND tablename = 'workflow_runs'
   AND indexdef LIKE '%WHERE (parent_run_id IS NULL)%' \gexec
"""


def child_history(shapes: list) -> str:
    """The dense agent's top-level first page after 100,000 child runs land on one of its
    versions, with and without `workflow_runs_version_top_level`."""
    shape = next(shape for shape in shapes if shape["name"] == "agent_top_level")
    values = dict(VALUES, agent=AGENTS["dense"], limit=LIMITS["page 50"])
    arguments = ", ".join(values[role] for role in shape["parameters"])
    measure = (f"DEALLOCATE ALL;\nPREPARE agent_top_level AS {shape['sql']};\n"
               f"EXPLAIN (ANALYZE, BUFFERS) EXECUTE agent_top_level({arguments});\n")
    return (CHILD_HISTORY
            + "\\echo '== child-only history: agent_top_level dense page 50"
              " (plan_cache_mode = force_generic_plan)'\n" + measure
            + DROP_TOP_LEVEL_INDEX
            + "\\echo '== child-only history without workflow_runs_version_top_level:"
              " agent_top_level dense page 50 (plan_cache_mode = force_generic_plan)'\n" + measure)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("database_url")
    parser.add_argument("--shapes", help="a dump_run_list_query_shapes file; default: dump now")
    parser.add_argument("--out", help="also write the output here")
    parser.add_argument("--board-out", help="write the run-board section here, not to --out")
    args = parser.parse_args()
    shapes = json.loads(pathlib.Path(args.shapes).read_text()) if args.shapes else dump_shapes()
    # The board runs before the child-only history, which changes the data.
    script = SEED + measurements(shapes) + board_measurements(shapes) + child_history(shapes)
    result = subprocess.run(["psql", args.database_url, "-X", "-q", "-v", "ON_ERROR_STOP=1"],
                            input=script, text=True, capture_output=True)
    # psql pads its column headers; strip the padding so the committed evidence passes
    # `git diff --check`.
    output = "".join(line.rstrip() + "\n" for line in (result.stdout + result.stderr).splitlines())
    if args.board_out and BOARD_START in output:
        before, rest = output.split(BOARD_START + "\n", 1)
        board, after = rest.split(BOARD_END + "\n", 1) if BOARD_END in rest else (rest, "")
        pathlib.Path(args.board_out).write_text(board)
        output = before + after
    if args.out:
        pathlib.Path(args.out).write_text(output)
    sys.stdout.write(output)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
