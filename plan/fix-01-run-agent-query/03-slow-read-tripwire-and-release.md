# 03 — Leave a signal for a slow run list, then release

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-agent-run-filter.md) and [02](02-query-plan-evidence-and-index.md).

## Outcome

If a run-list shape — the agent filter above all — gets slow in production, a log line says so and
carries what is needed to reopen 02's decision. A deliberately deferred optimization needs "a named
duration or working-set metric, or a threshold log" (`src/adapters/persistence/AGENTS.md`).

## Implementation

1. Time `list_runs`. Past `RUN_LIST_QUERY_WARN_THRESHOLD` (start at 500 ms, the deleted task
   board's `BOARD_QUERY_WARN_THRESHOLD`), log `Workflow run list is slow` at `warn` with
   `elapsed_ms`, `company_id`, which filters were present (`binding`, `state`, `agent` as kinds —
   identifiers only, no titles), `page_size` and `returned`. Keep the decision a pure function of
   elapsed time and threshold so it is unit-tested without sleeping.
2. Add a "Revisit when" section to `docs/performance/run-list.md`: slow under an agent filter
   with a small `returned` moves 02's ladder up a step; slow on every shape points at pool
   acquisition (`database_acquire_duration_ms`) first.
3. Document `agent_id` and its "uses, not ran" meaning on `RunsQuery` and the JSON route.

## Final verification gate

Focused first: `run_agent_filter_` and the workflow route suites
(`cargo test --locked --offline --lib routes::workflows::`) against a disposable migrated database.
Then run `scripts/check.sh` once — formatting, offline compilation, clippy, migrations, sqlx
metadata, and the database-backed suite on its own disposable database. Record commands and
results, and name any gate that could not run.
