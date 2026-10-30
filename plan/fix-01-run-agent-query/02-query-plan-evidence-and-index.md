# 02 — Measure the run list before indexing it

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Depends on [01](01-agent-run-filter.md).

## Outcome

Recorded `EXPLAIN (ANALYZE, BUFFERS)` evidence for every run-list shape, and an index only where
that evidence shows a shape reading the tenant's run history — which is never pruned — to fill
one page (`src/adapters/persistence/AGENTS.md`, "Change performance only from production
evidence"). Later plans add their shapes to the same script.

## Implementation

1. Add `scripts/tests/run-list-query-plan.py`, modelled on the deleted
   `task-counts-query-plan.py` (`git show 5d4aab1^:scripts/tests/task-counts-query-plan.py`):
   `TEMP` tables `LIKE public.workflow_runs` / `public.workflow_versions INCLUDING ALL`, the
   shipped SQL constants extracted from `reads.rs`, no application rows written, `ANALYZE` before
   measuring.
2. Synthetic data that exercises the bad cases: 100 tenants; four heavy tenants with 250,000
   retained runs each over two years; 40 workflows × 20 published versions; 64 agents, including
   one **dense** agent (in half the versions) and one **sparse** agent (one version, about 0.1% of
   runs); about 10% child runs; about 95% terminal states. Fill `bundle`, `input`, `params` and
   `resources` with minimal values that satisfy the copied CHECKs.
3. Measure: company first page; binding first page; dense- and sparse-agent first pages; agent +
   state; agent at the deepest page the UI reaches (`MAX_LISTED` = 200,
   `src/application/workflow/reads.rs:15`). Capture heap and index sizes.
4. Decide by access path, not local latency:
   - **No change** when every shape stops after about one page of matching index entries.
   - **`workflow_runs (company_id, version_id, created_at DESC, id DESC)`** when the sparse agent
     walks the company's history. `version_id IN (...)` over several versions does not come back
     in `created_at` order from this index, so re-measure the dense agent: it may now sort its
     whole history instead.
   - **A display-only run→agent link table** written at admission, keyed
     `(company_id, agent_id, created_at DESC, run_id DESC)`, only when both options above leave a
     shape reading unbounded history. That is a schema change of its own: stop and write its plan
     rather than growing this one.
5. An index goes into the init schema in place — no follow-up migration. Reset with
   `./scripts/reset-db.sh --all` and confirm with `psql "$DATABASE_URL" -c '\d workflow_runs'`.
6. Record dataset, plans and decision in `docs/performance/run-list.md`, full output in
   `docs/performance/run-list-query-plan.txt`, laid out like the deleted
   `docs/performance/task-counts.md`. Say that synthetic temp-table plans are access-path evidence,
   not latency targets.

## Focused checks and acceptance

`python3 scripts/tests/run-list-query-plan.py postgres://$(whoami)@localhost:5432/mail_agents_test`
completes and its output is committed. If the schema changed: reset, run
`cargo sqlx prepare --check -- --all-targets`, and rerun the `run_agent_filter_` tests from 01.
Record commands and results.
