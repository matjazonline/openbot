# Run list access paths

Measured on 2026-10-30, PostgreSQL 18.6 / Apple arm64, using
`scripts/tests/run-list-query-plan.py`. The script measures the statements `list_runs` actually
issues: it runs `dump_run_list_query_shapes` (`src/adapters/persistence/workflow/reads_tests.rs`),
which writes each shape the builder (`run_list_query`) can produce and what each `$n` binds, and
checks that order against the builder. It creates temporary copies of the migrated tables,
constraints and indexes and writes no application data. Run it against a migrated database:

```sh
python3 scripts/tests/run-list-query-plan.py postgres://$(whoami)@localhost:5432/mail_agents_test
```

## Dataset

1,096,000 runs (268 MB heap) and 80,000 versions (21 MB), analyzed before measuring:

- 100 tenants, each with 40 workflows × 20 published versions. Four heavy tenants have 250,000
  runs each over two years; 96 light tenants have 1,000.
- A run belongs to one workflow and, as time passes, to that workflow's later versions, so a
  version's runs are one contiguous slice of history (about 312 runs per version).
- About 95% of runs are terminal (succeeded, failed or cancelled in thirds) and 10% are child runs.
- Agents per heavy tenant:
  - The **dense** agent is in every even version: half of all runs, 125,000, across 400 versions.
  - The **sparse** agent is in one version (workflow 0's first): 313 runs, 0.125%, all at the old end
    of history.
  - Every version also uses one of 62 others.

All figures below are for a heavy tenant, at both page sizes the code reads, each plus its
look-ahead row:

- 50 rows (`LIMIT 51`): the UI's pages.
- 200 rows (`LIMIT 201`): `MAX_LISTED`, which is the JSON route's default and the UI's refresh of a
  list that "Load more" has grown.

Each shape is measured under `plan_cache_mode = auto` and `force_generic_plan`. The generic plan is
the one that matters: sqlx prepares and caches statements per connection, and Postgres switches to
the generic plan after five executions when it looks no costlier. These are temporary-table plans on
synthetic data with local buffers: evidence about access paths, not production latency targets.

## Before: `version_id IN (SELECT …)`, no version index

[Full output](run-list-query-plan-before.txt), measured at 50-row pages only. Company and binding
pages were already index range scans of about one page. Every agent shape read the tenant's
history:

| Shape | Custom plan | Generic plan |
| --- | --- | --- |
| Dense agent, first page | 92 ms; hash join over all 250,000 tenant runs | 234 ms; walks `workflow_runs_newest` against 400 materialized versions per row |
| Sparse agent, first page | 12 ms; scans the tenant's whole `(company_id, id, workflow_id, version_id)` range | 60 ms; walks 239,520 runs of `workflow_runs_newest` to find 51 |
| Dense agent, page at depth 200 | 3,915 ms; 1.48 M buffer reads, a nested loop over 400 versions, each scanning the tenant's range | 1.1 ms; walks `workflow_runs_newest` from the cursor, quick only because half the runs match |

The planner estimated 4 matching versions where there were 400: `$n = ANY(agent_ids)` has no
element statistics.

## Options measured

The plan's ladder (`plan/fix-01-run-agent-query/02-query-plan-evidence-and-index.md`) had three
options:

- **No change** fails: the tables above.
- **`workflow_runs (company_id, version_id, created_at DESC, id DESC)`**, measured two ways
  (`plan/fix-01-run-agent-query/phases/f01/docs/logs/option2-experiment.txt`, 50-row pages):
  - With the same `IN`: the dense agent still read and sorted its whole history on every page,
    whatever the page size (58–63 ms, 127,000 buffers, in both plan modes).
  - **Adopted:** one ordered range per version, cut at the page size and then merged
    (`CROSS JOIN LATERAL … LIMIT`). Its cost is the subject of the next section.
- **A run-to-agent link table** would read one page for any agent. It is deferred: see "The
  remaining bound" below.

## After: per-version ranges on `workflow_runs_version_newest`

[Full output](run-list-query-plan.txt). Plans are the same in both plan modes; times are generic.
"Rows" are the version-index entries read.

| Shape | 50-row page | 200-row page |
| --- | --- | --- |
| Company, binding, `state` | 0.03–0.38 ms; about a page of their keyset index | 0.05–1.7 ms; the same; unchanged by this work |
| Dense agent, first page | 11.5 ms; 20,400 rows (16% of its history) | **41.3 ms; 80,400 rows (64%)** |
| Dense agent, page at depth 200 | 11.0 ms; 20,400 rows | 43.1 ms; 80,400 rows |
| Sparse agent, first page / at depth 200 | 0.27 / 0.13 ms; 51 rows | 0.43 / 0.15 ms; 201 / 113 rows |
| Dense agent + `failed` | 24.7 ms; 66,100 buffers | **42.8 ms; 128,000 buffers: its whole history** |
| Dense agent + `queued` | 30.3 ms; 124,700 buffers: its whole history | 30.7 ms; the same |

A page reads at most a page of runs from each version that uses the agent. So one read touches the
smaller of two things: (versions using the agent × page size), or the agent's whole history. At
50-row pages that is well under the agent's history here. At 200-row pages most of each ~312-run
version is read, and with any state filter all of it. Agents whose workflows are published often,
so that each version holds no more runs than a page, are read in full on every page. At the
50-row pages measured before, the read is an order of magnitude better than before. The dense
agent's first page fell from 234 ms to 11.5 ms (generic plan). Its page at depth 200 fell from
3.9 s to 11.5 ms (custom plan). But the read is **not** independent of run history.

## The remaining bound

Plan 02's ladder says that when the index still leaves a shape reading run history, the next step
is a run-to-agent link table, written as a plan of its own. It is written as a **deferred** plan:
[`plan/db_improve/run-agent-link-table.md`](../../plan/db_improve/run-agent-link-table.md). It
adds a table and a write on the admission path. The persistence rules
(`src/adapters/persistence/AGENTS.md`, "Change performance only from production evidence") hold
such a change until production evidence asks for it. The tripwire below supplies that evidence.

The index costs 135 MB for these 1,096,000 runs (about 123 MB per million). `state` is already in
`workflow_runs_attention` and `workflow_runs_open_by_channel`, so a state change is never a HOT
update. Every insert and every state change therefore writes an entry into this index, as it does
into the others. `scripts/lease-budget.sh` showed no change in the leased operations' work.

## Known remaining shape: agent + a rare state

A version's range that holds fewer matches than a page is read to its end. With a rare state such
as `queued` or `running`, every range is, so the read covers the agent's whole history. The plain
company + rare-state list shares this shape: no keyset index serves `queued` or `running`, and
`workflow_runs_attention` covers only `waiting` and `failed`. The synthetic data understates it,
because its open runs are spread through history rather than clustered recently.

Neither the link table nor anything else measured here fixes it. That would need state in a keyed
index, which every state change rewrites. The run board's open columns do not use this read.

## Revisit when

`list_runs` logs `Workflow run list is slow` at `warn` past `RUN_LIST_QUERY_WARN_THRESHOLD`
(500 ms). The line carries:

- `elapsed_ms` and `company_id`;
- whether a `binding` or `agent` filter was present, as booleans;
- `state`: the state asked for, `none`, or `other` for text that is not a run state;
- `page_size` and `page_rows`. `page_rows` is the rows the statement returned: `page_size` + 1 when
  the page filled, fewer when the matches ran out. It is not the index entries read, which are what
  the tables above count.

| Observation | Reading |
| --- | --- |
| Line never appears | These access paths are sufficient. |
| `agent=true`, `state` `none` or a terminal state, `page_rows` = `page_size` + 1, rising | The bound above: build the deferred link table. |
| `agent=true`, `state` `queued`, `running` or `waiting`, `page_rows` short | The rare-state shape; the link table does not help. |
| `agent=false`, `state` `queued` or `running` | The same rare-state shape without an agent. |
| Every shape slow | Check pool acquisition (`database_acquire_duration_ms`) and contention before the query. |

If the line is absent because the warning is not deployed, that is not evidence of health.
