# 01 — Let run reads leave out child runs

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Second of the run board plans, after
[fix-01-run-agent-query](../fix-01-run-agent-query/01-agent-run-filter.md); see
[new-board-view](../new-board-view/README.md) for the order.

## Outcome

A run read can be limited to top-level runs, so a parent and the child it started are not shown
as two separate pieces of work. The board always uses it; the list keeps showing every run.

## Context

- A child run is a `workflow_runs` row with `parent_run_id` set (CHECK `workflow_parent_shape`);
  a rerun is always top-level (CHECK `workflow_rerun_shape`).
- A parent version's `agent_ids` includes its pinned children's agents, so under
  fix-01-run-agent-query's filter both parent and child match.
- Children are already reachable from the parent: its pane lists them and wakes on their events
  (`src/adapters/http/pages/workflow_runs/detail.rs:186`), and their controls go through the
  parent (`detail.rs:316`). The list labels them "child of …"
  (`src/adapters/http/pages/workflow_runs/mod.rs:143`).

## Implementation

1. Add a named enum to `RunFilter`, not a bool (`src/AGENTS.md`, "No flag parameters") — for
   example `RunLineage::{Any, TopLevel}` — defaulting to `Any`, so the list and the JSON route
   return what they return today.
2. `TopLevel` appends `AND parent_run_id IS NULL` in SQL. Filtering after the read would leave
   short pages and a cursor past rows nobody saw.
3. The JSON runs route accepts `lineage=top_level`.
4. Record the decision on the enum: the board always reads `TopLevel`; the list stays `Any`
   because it is the forensic feed and already labels children.
5. Add the `TopLevel` and `TopLevel` + agent shapes to `scripts/tests/run-list-query-plan.py` and
   record them in `docs/performance/run-list.md`. Children are a small fraction, so no index is
   expected — record that rather than assume it.

## Focused checks and acceptance

DB-backed tests named `run_top_level_*`:

- a parent and its child where only the child's workflow uses the agent: `TopLevel` + agent
  returns the parent once, `Any` returns both;
- a rerun is returned by `TopLevel`;
- with children interleaved, `TopLevel` pages are full and the cursor continues correctly;
- the default leaves existing callers unchanged.

```sh
createdb mail_agents_run_top
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_run_top" \
  cargo test --locked --offline --lib run_top_level_
dropdb mail_agents_run_top
```

Confirm a nonzero test count and record the result.
