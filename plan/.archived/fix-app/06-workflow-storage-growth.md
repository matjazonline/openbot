# 06 — Workflow storage growth

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Part of [fix-app](README.md). Starts after [03](03-workflow-database-guards.md) merges. Retention
deletes terminal runs, which is safe only once 03 guards the edges that leave a terminal state.
The witness changes here also sit beside 03's new `BEFORE UPDATE` trigger on `workflow_runs`.

## Outcome

Workflow tables grow with the history someone needs, not with every state change:
- witness rows live only as long as their transaction;
- each distinct bundle is stored once;
- terminal history is removed after a decided period.

## Batches

### 1 — Witnesses do not outlive their transaction

**Today:**
- `workflow_action_capture_state_witness` (`init_schema.sql:1954-1964`) inserts a row into
  `workflow_action_state_witnesses` on every run state or `waiting_reason` change.
- The claim retirement and schedule witnesses (`:1925`, `:1946`) do the same on job updates.
- Every reader matches `transaction_id = pg_current_xact_id()` (`:145`, `:1888`, `:1992`, `:2023`).
  So once its transaction commits, a row is never read again, yet nothing deletes it.
- The table has no primary key (`:4734-4740`).
- No production `ActionEvidenceVerifier` exists; every implementation is in a test or test-support
  file. The machinery is therefore paid for on every run but cannot be exercised in production. The
  verifier itself is tracked in `plan/todo.md`.

**Change:**
- Sweep committed witnesses in the existing workflow maintenance loop, in bounded batches. A row
  whose transaction has committed is garbage by construction.
- Rows from transactions still in flight are invisible to the sweeper, so it cannot race a reader.
- Check the append-only delete guards on these tables first, and allow this sweep through them.
- Consider instead whether the capture can be skipped entirely when no reconciliation guard can run
  in the transaction. Only take that route if the condition is local and provable.

**Tests, `witness_sweep_*` (use `own_database`, since this asserts table contents):**
- After a run moves through several states and the transactions commit, one sweep leaves no rows.
- A reconciliation guard test that needs a same-transaction witness still passes. Rerun the
  `action_reconciliation_` suites.

### 2 — Each bundle is stored once

**Today:**
- Admission writes `store_bundle(row.binding.bundle())` into every `workflow_runs.bundle`
  (`src/adapters/persistence/workflow/admission_write.rs:103-116`), up to 4 MiB per run.
- The bundle is the published set of versions, a root plus its children (`init_schema.sql:628-632`
  reads `versions` and `root`). It is therefore not the same bytes as one `workflow_versions.bundle`,
  but it is identical for every run of the same binding revision.

**Change:**
- Measure first, per `src/adapters/persistence/AGENTS.md`: record the distinct-to-total byte ratio
  of `workflow_runs.bundle` on the development database, together with the query used.
- Then content-address: a `workflow_bundles(company_id, digest, bundle)` table, with runs
  referencing `(company_id, bundle_digest)`.
- Every reader of `workflow_runs.bundle` joins through the digest. Find them with
  `graft grep "bundle"` under `src/adapters/persistence/workflow/`.
- 03 removes the budget trigger's read of the bundle, so this batch does not have to carry it.

**Tests, `bundle_store_*`:**
- Two runs of one binding revision share one stored bundle.
- A run reads back exactly the bundle it was admitted with.
- Rerun the admission and recovery suites.

### 3 — Retention for terminal runs

**Decision before starting:** how long terminal history is kept. Proposed:
- Delete a top-level run and its whole tree 90 days after its `finished_at`.
- Never delete a run that an open decision, a live child, or an unsettled reconciliation still
  references.

**Today:**
- Nothing deletes workflow runs, run events, waits, decisions or `model_usage_calls`. The only path
  is the company-delete cascade (`src/adapters/persistence/company.rs:96`).
- The `background_tasks` / `task_attempts` gap is already recorded in
  `plan/db_improve/05-deferred-until-traffic.md` §4a. Pick it up here only if the same sweep covers
  it naturally.

**Change:**
- A bounded sweep in the maintenance loop deletes eligible run trees. `workflow_action_fact_immutable`
  (`init_schema.sql:2323`) already allows cascade deletes, so the sweep runs through it.
- **`model_usage_calls` feeds usage and cost reporting.** Decide whether its rows go with the run,
  or are rolled up before the run is deleted. [08](08-schema-cleanup.md) makes their run ids
  `NOT NULL`, which rules out detaching them.
- Count deleted trees and record the oldest remaining `finished_at`, so the sweep's progress is
  observable.

**Tests, `run_retention_*` (`own_database`):**
- An old terminal tree is deleted completely.
- A run inside the window, a non-terminal run, and a terminal run with an open decision are kept.
- A full batch is followed by a second iteration that makes progress rather than re-selecting the
  same rows.

## Focused checks

```sh
createdb mail_agents_fix_app_06
SQLX_OFFLINE=true TEST_DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_fix_app_06" \
  cargo test --locked --offline --lib witness_sweep_
# repeat with action_reconciliation_, bundle_store_, workflow::admission, run_retention_
dropdb mail_agents_fix_app_06
```

Run `./scripts/reset-db.sh --all` after every init-schema edit. Confirm a nonzero test count for
each filter.

## Final gate

Run `./scripts/reset-db.sh --all`, then `scripts/check.sh` once. Record the commands and results,
and name any gate that could not run.
