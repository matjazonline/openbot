# Run list and run board access paths

Measured on 2026-10-30, re-measured with the top-level shapes for `fix-02-run-child-duplicates`,
and on 2026-10-31 with the run board's statements and the dataset additions for
`fix-03-run-board-finished-columns`, and again that day with the workflow filter's shapes for
`new-board-view` 02 (the full output files are from that last run; tables above it keep the numbers
of the run that measured them). PostgreSQL 18.6 / Apple arm64, using
`scripts/tests/run-list-query-plan.py`. The script measures the statements `list_runs`,
`run_board_counts` and `run_board_cards` actually issue: it runs `dump_run_list_query_shapes`
(`src/adapters/persistence/workflow/reads_tests.rs`), which writes each shape the builders
(`run_list_query`, `board_counts_query`, `board_cards_query`) can produce and what each `$n` binds,
and checks that order against the builders. It creates temporary copies of the migrated tables,
constraints and indexes and writes no application data. Run it against a migrated database:

```sh
python3 scripts/tests/run-list-query-plan.py postgres://$(whoami)@localhost:5432/mail_agents_test \
  --out docs/performance/run-list-query-plan.txt \
  --board-out docs/performance/run-board-query-plan.txt
```

## Dataset

1,096,000 runs (289 MB heap) and 80,000 versions (21 MB), analyzed before measuring:

- 100 tenants, each with 40 workflows × 20 published versions. Four heavy tenants have 250,000
  runs each over two years; 96 light tenants have 1,000.
- A run belongs to one workflow and, as time passes, to that workflow's later versions, so a
  version's runs are one contiguous slice of history (about 312 runs per version).
- About 95% of runs are terminal (succeeded, failed or cancelled in thirds), each finished ten
  minutes after it was created. 10% are child runs, all through history.
- Workflows 0–31 of a tenant are bound to its channels 0–31; workflows 32–39 to the company (no
  channel).
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

The plan's ladder (`plan/.archives/fix-01-run-agent-query/02-query-plan-evidence-and-index.md`) had three
options:

- **No change** fails: the tables above.
- **`workflow_runs (company_id, version_id, created_at DESC, id DESC)`**, measured two ways
  (`plan/.archives/fix-01-run-agent-query/phases/f01/docs/logs/option2-experiment.txt`, 50-row pages):
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
| Company, binding, `state` | 0.03–0.43 ms; about a page of their keyset index | 0.06–1.8 ms; the same; unchanged by this work |
| Dense agent, first page | 11.5 ms; 20,400 rows (16% of its history) | **43.2 ms; 80,400 rows (64%)** |
| Dense agent, page at depth 200 | 11.7 ms; 20,400 rows | 43.8 ms; 80,400 rows |
| Sparse agent, first page / at depth 200 | 0.27 / 0.13 ms; 51 rows | 0.47 / 0.16 ms; 201 / 113 rows |
| Dense agent + `failed` | 26.3 ms; 66,100 buffers | **44.5 ms; 127,900 buffers: its whole history** |
| Dense agent + `queued` | 31.5 ms; 124,700 buffers: its whole history | 33.1 ms; the same |

A page reads at most a page of runs from each version that uses the agent. So one read touches the
smaller of two things: (versions using the agent × page size), or the agent's whole history. At
50-row pages that is well under the agent's history here. At 200-row pages most of each ~312-run
version is read, and with any state filter all of it. Agents whose workflows are published often,
so that each version holds no more runs than a page, are read in full on every page. At the
50-row pages measured before, the read is an order of magnitude better than before. The dense
agent's first page fell from 234 ms to 11.5 ms (generic plan). Its page at depth 200 fell from
3.9 s to 11.7 ms (custom plan). But the read is **not** independent of run history.

## Top-level runs only

`RunLineage::TopLevel` (plan `fix-02-run-child-duplicates`) adds `parent_run_id IS NULL` to each
range, so a page stays full and the cursor never passes a skipped root. About 10% of these runs are
children; every run of workflows 5, 15, 25 and 35 is one, so 40 of the dense agent's 400 versions
hold only children.

With an agent, each version's range comes from `workflow_runs_version_top_level`, a partial copy of
`workflow_runs_version_newest` restricted to `WHERE parent_run_id IS NULL`. A version whose runs are
all children (a workflow only ever called as a child) is then an empty range. On the full index,
it would be read to its end on every page, a cost that grows with that workflow's history. The
predicate is a literal, so the generic plan uses the partial index too. Generic plan:

| Shape | 50-row page | 200-row page |
| --- | --- | --- |
| Company, top-level | 0.03 ms; 5 buffers, the same index range as without the filter | 0.08 ms; 13 buffers |
| Dense agent, top-level | 10.4 ms; 18,360 rows from 360 versions, none from the 40 child-only ones; 19,600 buffers | 39.3 ms; 72,360 rows; 74,600 buffers |
| Sparse agent, top-level | 0.29 ms; 89 buffers | 0.48 ms; 240 buffers |

The last section of the output adds 100,000 child runs to one child-only version of the dense agent
(workflow 5, version 18), then measures the same top-level first page twice:

| Index on the version ranges | 50-row page, generic plan |
| --- | --- |
| `workflow_runs_version_top_level` | 13.8 ms; 19,620 buffers, unchanged by the new children |
| `workflow_runs_version_newest` only (partial index dropped) | 43.0 ms; 37,849 buffers: all 100,000 children read and discarded |

Before the partial index, the plain top-level dense page read 32,300 buffers (17.5 ms). Each child
run was read from `workflow_runs_version_newest` and discarded on the heap.

The partial index holds one entry per top-level run: 122 MB for these 986,400 top-level runs. Each
entry is written on insert and again on every state change (state changes are never HOT; see
below).

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

## Workflow filter (`new-board-view` 02)

The run list and the board both take a workflow scope (`RunFilter::workflow`,
`RunBoardScope::workflow`): `AND workflow_id = $n` on the list, and the same filter on the board's
materialized working set, like a binding scope. Measured on the heavy tenant for workflow 0 (a
fortieth of its runs, 6,250, every one open) and for a workflow of the tenant with no runs, the
sparsest scope. Before is the migrated schema with `workflow_runs_workflow_newest` dropped; after
is the shipped schema. Times are `force_generic_plan` at 50-row pages, buffers are the whole
statement's:

| Shape | Before | After |
| --- | --- | --- |
| Workflow, a fortieth | 23.6 ms; 8,844 buffers. Walks the tenant's whole `(company_id, id, workflow_id)` unique index, then reads and sorts the workflow's history | 0.11 ms; 54. A page of `workflow_runs_workflow_newest` |
| Workflow, no runs | 7.5 ms; 2,594. The same unique-index walk, for nothing | 0.01 ms; 4 |
| Workflow + `queued` | 20.1 ms; 8,844 | 0.28 ms; 155 |
| Workflow + `failed` (matches nothing in workflow 0) | 19.6 ms; 8,844 | 10.5 ms; 6,268. The workflow's range read to its end |
| No runs + any state | 6.9–7.1 ms; 2,594 | 0.004–0.012 ms |

`auto` plans agree, except that before, the first page of the dense workflow stopped early on
`workflow_runs_newest` (0.36 ms) and every larger page or sparser scope fell into the full walk.
Before, every workflow-filtered page read the whole tenant, whatever the workflow's size. That is
the access-path failure fix-01 measured for the sparse agent, so the index is added, as fix-01 added
`workflow_runs_version_newest`: `workflow_runs_workflow_newest (company_id, workflow_id,
created_at DESC, id DESC)`, 143 MB for these 1,096,078 runs, the size of the binding index beside
it. Every insert writes it, and so does every state change: `state` is in other indexes, so a state
change is never a HOT update and writes an entry into every index, this one included (as for
`workflow_runs_version_newest`, above). `workflow_id` itself never changes.

What remains is the "agent + a rare state" shape below in its workflow form: a workflow with a
state that matches few of its runs reads the workflow's range to its end, bounded by that
workflow's history rather than the company's.

The board needs nothing new: its workflow scope filters the materialized working set as a binding
scope does, and costs the same (16.6 ms counts, 29.5 ms cards in the week with the shipped finished
index; "Run board" below).

## Run board

The run board (plan `fix-03-run-board-finished-columns`) shows one column per state over top-level
runs: every open run, and runs finished inside a window (a day, a week or 30 days). It makes two
reads with an authorization step between them (`src/adapters/persistence/workflow/board_reads.rs`):

- **Counts**, by state and channel: the open runs (`state IN ('queued', 'running', 'waiting')`)
  `UNION ALL` the runs with `finished_at >= cutoff`, both `parent_run_id IS NULL`.
- **Cards**, once the route knows which counted channels the viewer may see: one subquery per state
  with the literal state, the visible channels, and `LIMIT 51` (`BOARD_COLUMN_CAP` + 1). Open
  columns are oldest first; finished columns most recently finished first.

[Full output](run-board-query-plan.txt). Every statement was measured in both plan modes:

- under three finished-run indexes: none, (A) `(company_id, finished_at DESC, id DESC)` and (B)
  `(company_id, state, finished_at DESC, id DESC)`, both `WHERE finished_at IS NOT NULL AND
  parent_run_id IS NULL`;
- for the heavy tenant in each window, and the light tenant in the 30-day window;
- with the cards seeing every channel and the company's runs, or 8 of the 32 channels and not the
  company's runs;
- for two bindings and three agents in the default week. In this dataset, workflow 0's binding
  holds only open runs and workflow 1's only finished ones (about 60 in a week). The dense and
  sparse agents have no runs in any window, because their versions end before it. The "recent"
  agent is in the last versions of workflows 1 and 32, so it has finished runs in every window.

Heavy tenant, week window; each cell is time, buffers (largest plan node) and rows removed by a
filter:

| Read | No index, generic | (A), generic | (A), auto | **(B), generic** | (B), auto |
| --- | --- | --- | --- | --- | --- |
| Counts | 61.8 ms; 19,217; 223,228 | 18.9 ms; 8,524; 0 | 19.1 ms; 8,524; 0 | **18.1 ms; 8,540; 0** | 18.7 ms; 8,540; 0 |
| Counts, 30-day window | 63.9 ms; 19,217; 216,439 | 21.4 ms; 8,842; 0 | 21.6 ms; 8,842; 0 | **21.3 ms; 8,871; 0** | 21.6 ms; 8,871; 0 |
| Cards, all visible | 235.7 ms; 55,135; 748,228 | 29.2 ms; 12,574; 303 | 19.9 ms; 8,453; 303 | **29.1 ms; 12,574; 0** | 18.9 ms; 8,453; 0 |
| Cards, 8 of 32 channels | 159.7 ms; 48,869; 299,688 | 14.6 ms; 6,408; 2,458 | 13.7 ms; 6,530; 5,004 | **14.3 ms; 6,397; 719** | 13.6 ms; 6,508; 1,460 |
| Finished binding: counts | 64.0 ms; 19,217; 237,448 | 16.3 ms; 8,524; 14,220 | 17.1 ms; 8,524; 14,220 | **17.5 ms; 8,540; 14,220** | 16.5 ms; 8,540; 14,220 |
| Finished binding: cards | 228.6 ms; 55,135; 762,448 | 232.0 ms; 55,135; 762,448 | 30.9 ms; 12,802; 17,764 | **31.9 ms; 12,780; 14,220** | 30.4 ms; 12,780; 14,220 |
| Dense agent: counts | 62.4 ms; 19,253; 223,628 | 19.9 ms; 8,560; 400 | 19.6 ms; 8,559; 400 | **19.5 ms; 8,576; 400** | 19.2 ms; 8,575; 400 |
| Dense agent: cards | 243.8 ms; 112,223; 748,232 | 244.0 ms; 112,223; 748,232 | 34.9 ms; 19,995; 4,747 | **43.0 ms; 69,868; 4** | 34.9 ms; 19,973; 1,203 |
| Recent agent: cards, 8 of 32 | 163.5 ms; 75,117; 299,694 | 162.5 ms; 75,117; 299,694 | 170.2 ms; 75,117; 299,694 | **20.0 ms; 32,762; 1,466** | 20.0 ms; 32,762; 1,466 |

The light tenant's board reads under 0.3 ms and 80 buffers with (B) in every case.

- **No index:** every read covers the tenant's history; the window cannot prune it.
- **(A):** the company's board reads only the window. Scoped cards read history under generic
  plans, and under auto plans for the recent agent with 8 of 32 channels visible; the other scoped
  card reads stay in the window under auto plans. Without `state` in the key, the planner can
  prefer a bitmap over the company's channel key, which reads every run of those channels.
- **(B), adopted** as `workflow_runs_finished_by_state`. The gate's rule was to take the candidate
  under which, in both plan modes, the counts read only the open runs and the window, and each
  finished column stops at 51 matching rows. (B) meets it, and (A) does not under generic plans,
  which are the ones a busy connection runs. The stop at 51 holds for unscoped columns, which read
  their first 51 index entries. A scoped column reads its whole working set and filters it (below).
  The index is 117 MB for these 934,800 top-level finished runs. Like the other state-keyed
  indexes, it is written on every state change into or out of a terminal state, which is never HOT.

**A scope filters a materialized working set.** With the binding or agent filter in the same
`WHERE` as the working set, the planner drove the read from `workflow_runs_binding_newest` or from
each of the agent's versions. It then read the scope's whole history and filtered it by state and
window. In generic plans, with (B) in place, the dense agent's counts read 230,378 buffers over its
400 versions and its cards 408,108
([evidence](../../plan/.archives/fix-03-run-board-finished-columns/phases/f03/docs/logs/B1-unfenced-board.md)).
Moving the working set into an `OFFSET 0` subquery kept the scope out of its scan, but not out of
a rescan. With one version estimated where the recent agent has 13, the planner looped over the
versions and re-ran the working set for each: 111,055 buffers
([evidence](../../plan/.archives/fix-03-run-board-finished-columns/phases/f03/docs/logs/B1-offset0-rescan.md)).

So both statements read the working set as `WITH … AS MATERIALIZED`, which runs at most once, and
apply the scope to its rows:

- Every read is then bounded by the company's working set, whatever a scope's history.
- A scope with little history pays more than it would have: the sparse agent's counts read 702
  buffers unfenced, 8,576 now.
- A scoped column reads its whole working set: every open run of its state, or the window's runs
  of its state (about 590 a week here). It then sorts the matches; it never stops at 51.
- The agent cards' extra buffers are cache hits on the versions key, one probe per working-set row.
  The dense agent's generic cards read 12,677 pages; the other 57,191 buffers are hits.

**What the working set costs here.** About 8,450 of the ~8,540 buffers a heavy board's counts read
are its open runs. 5% of this dataset's runs are open and spread through two years, 12,500 per
heavy tenant, scattered over the heap. Open columns and the open half of the counts read every
open run through `workflow_runs_open_by_channel` and check `parent_run_id` on the heap. In
production, open runs are the work in progress, not history.

## Revisit when

`list_runs` logs `Workflow run list is slow` at `warn` past `RUN_LIST_QUERY_WARN_THRESHOLD`
(500 ms). The line carries:

- `elapsed_ms` and `company_id`;
- whether a `binding`, `workflow` or `agent` filter was present, as booleans;
- `state`: the state asked for, `none`, or `other` for text that is not a run state;
- `lineage`: `any`, or `top_level` when child runs are left out;
- `page_size` and `page_rows`. `page_rows` is the rows the statement returned: `page_size` + 1 when
  the page filled, fewer when the matches ran out. It is not the index entries read, which are what
  the tables above count.

| Observation | Reading |
| --- | --- |
| Line never appears | These access paths are sufficient. |
| `agent=true`, `state` `none` or a terminal state, `page_rows` = `page_size` + 1, rising | The bound above: build the deferred link table. |
| `agent=true`, `state` `queued`, `running` or `waiting`, `page_rows` short | The rare-state shape; the link table does not help. |
| `agent=false`, `state` `queued` or `running` | The same rare-state shape without an agent. |
| `workflow=true`, `state` `none`, rising | `workflow_runs_workflow_newest` should read a page ("Workflow filter" above). Check the plan still uses it. |
| `workflow=true`, `state` rare | The rare-state shape within one workflow's range. |
| `lineage=top_level`, `agent=false`, rising | Child runs crowd the company's range. Each is skipped by a heap check, because no keyset index carries `parent_run_id`. Measure their share of runs. If children have become most of the range, give the index the read uses a top-level partial twin (`WHERE parent_run_id IS NULL`), measured as above. |
| `lineage=top_level`, `agent=true` | Read it as the agent rows above. Child runs are not in the ranges the read takes (`workflow_runs_version_top_level`), so children no longer explain a slow line. If the plan has stopped using that index, re-measure: the child-only history section shows the cost without it. |
| Every shape slow | Check pool acquisition (`database_acquire_duration_ms`) and contention before the query. |

If the line is absent because the warning is not deployed, that is not evidence of health.

The board logs `Workflow run board is slow` at `warn` when its reads together pass
`RUN_BOARD_QUERY_WARN_THRESHOLD` (500 ms, `src/adapters/http/routes/workflows/board.rs`). The line
carries:

- `elapsed_ms`, and each phase's share: `counts_ms`, `visibility_ms`, `cards_ms`, `rollup_ms`;
- `company_id`; whether a `binding`, `workflow` or `agent` scope was given, as booleans; `window`;
- `eligible_runs`: every counted run, hidden channels included;
- `channels_checked`: the distinct associations authorized;
- `cards_read`, look-ahead cards included, and `cards_shown`.

| Observation | Reading |
| --- | --- |
| `eligible_runs` small, `elapsed_ms` high | Not the working set. Check pool acquisition (`database_acquire_duration_ms`) and contention, then `visibility_ms`. |
| `counts_ms` leads, `eligible_runs` high in every window | Open runs: the counts and open columns read every one. Look for runs stuck open before indexing anything. |
| `counts_ms` leads, `eligible_runs` high and rising, `window=month` worst | The finished window itself, a working-set problem. Write a deferred rollup plan for the board (plan `fix-03-run-board-finished-columns` 03, step 3): the live query is its specification and test oracle, measured before anything is built. |
| `cards_ms` leads, `binding`, `workflow` or `agent` true | Scoped columns read the company's working set of their state and filter it (materialized, above). If the scope is a small share of a large window, measure a scoped finished index, e.g. `(company_id, binding_id, state, finished_at DESC)`, as above. |
| `cards_ms` leads, no scope | Unscoped columns should stop at 51 entries. Check that the plan still uses `workflow_runs_finished_by_state`; a viewer who sees few channels reads past hidden runs. |
| `visibility_ms` leads, `channels_checked` high | One authorization per channel, as in the list: batch the visibility read. |
| `rollup_ms` leads | The child rollup over at most 300 shown cards; measure children per parent. |
