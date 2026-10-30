# Deferred — run-to-agent link table for the run list's agent filter

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

**Status: deferred.** Build it only when the trigger below is met and measured.

## Context

The run list's agent filter (plan `fix-01-run-agent-query`) works in two steps:

1. It reads the versions whose `agent_ids` display index holds the agent.
2. For each of them it reads one range of `workflow_runs_version_newest`, cut at the page size, then
   merges the ranges (`run_list_query`, `src/adapters/persistence/workflow/reads.rs`).

A page therefore reads the smaller of (versions × page size) and the agent's whole history.
`docs/performance/run-list.md` measured it: a dense agent's 200-row page read 80,400 runs, 64% of
its history. Agents whose workflows are published often are read in full.

That plan's ladder calls for this plan once the version index still leaves a shape reading run
history. It is deferred under `src/adapters/persistence/AGENTS.md` ("Change performance only from
production evidence"), because it adds a table and an admission-path write to save reads nobody has
yet shown to be slow.

## Build it when

`Workflow run list is slow` (`warn_if_run_list_is_slow`, past 500 ms) appears repeatedly with:

- `agent=true`;
- `state` either `none` or a terminal state (`succeeded`, `failed`, `cancelled`);
- a full page (`page_rows` = `page_size` + 1);

for companies whose agents' versions keep growing. Rule out pool acquisition
(`database_acquire_duration_ms`) first. A line for `queued`, `running` or `waiting` with a short
page is not a reason: see "Not solved by this".

## Design sketch

- **Table.** `workflow_run_agents (company_id, agent_id, run_id, created_at)` in the init schema
  while pre-release:
  - one row per run and agent of its version's `agent_ids`;
  - `created_at` copied from the run, which never changes;
  - a composite foreign key `(company_id, run_id) → workflow_runs(company_id, id)`;
  - no foreign key to `agents`: like `agent_ids`, it is a display index and outlives a deleted
    agent;
  - an index `(company_id, agent_id, created_at DESC, run_id DESC)`.
- **Write.** In the same transaction as the run row (`insert_run`,
  `src/adapters/persistence/workflow/admission_write.rs`), from the version's frozen agents. Nothing
  else writes it, and no execution, admission or authorization path reads it.
- **Read.** The agent shape walks that index for one page from the cursor, joining
  `workflow_runs` for binding and state.

## Verification

Per-batch focused checks, against a disposable migrated database
(`SQLX_OFFLINE=true TEST_DATABASE_URL=… cargo test --locked --offline --lib <filter>`):

- **Schema and write.** After editing the init schema, run `./scripts/reset-db.sh --all` and
  `psql -c '\d workflow_run_agents'`. Add `run_agent_links_*` tests for admission writing one row per
  frozen agent, and for a cross-tenant row being rejected by the composite key.
- **Read.** The current per-version read is the new read's specification and test oracle. Add
  `run_agent_links_*` tests asserting equal pages, in order, across agents, filters and cursors on
  generated data, before switching. Then run the existing `run_agent_filter_` and
  `routes::workflows::` suites.
- **Evidence.** Re-run `scripts/tests/run-list-query-plan.py` before and after, and update
  `docs/performance/run-list.md`. Re-run `scripts/lease-budget.sh --offline` for the admission write.

Final gate: `scripts/check.sh` once the whole change is in. Record commands and results, and name
any gate that could not run.

## Not solved by this

Agent + rare state: the link walk still reads the agent's history to find the few `queued` or
`running` runs. Company + rare state has the same problem. Fixing it needs state in a keyed index,
which every state change rewrites, and is out of scope here.
