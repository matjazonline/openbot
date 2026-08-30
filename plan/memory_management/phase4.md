# Phase 4 — The durable worker and the lifecycle hooks

Read [`general_plan_instructions.md`](general_plan_instructions.md) first. This phase needs phases
1–3 landed.

**Goal:** queued jobs actually run. A document reaches *In memory*, a deleted one leaves the
provider, and the three events that would otherwise silently strand documents — switching provider,
deleting an agent, deleting a company — are handled.

This is the phase where correctness lives. `src/application/AGENTS.md` and the persistence
`AGENTS.md` are unusually specific about queues; re-read both sections on leases before starting.

---

## 1. A third loop in `MemoryWorker` — `src/application/services/memory_worker.rs`

`MemoryWorker::run` (`:65`) currently drives a provisioning loop and a cleanup loop. Add a document
loop beside them, joined the same way, reusing without modification:

- `supervise_memory_job_lease` (`memory_job_lease.rs`) — renews the lease while a provider call is in
  flight, and **cancels the work on lease loss**;
- `retry_at` / `next_poll_at` (`memory_job_schedule.rs`) — exponential backoff capped at 300s, and
  2–5s jitter;
- `LEASE_SECONDS = 180`, `MAX_PROVIDER_FAILURE_ATTEMPTS = 8`.

### One claim, bounded work

```
claim_document_job(lease_token, now + LEASE_SECONDS)
  -> resolve the binding for job.company_id
  -> match job.operation { Ingest => …, Forget => … }
```

**Resolve the provider from the job row, never from a literal.** The job carries `provider`,
`remote_database_id` and `collection`; use them. `src/adapters/persistence/AGENTS.md`, "Carry the
discriminator; never re-assert it as a literal", documents three real bugs from exactly this — the
wrong row updated, a no-op misread as a lost lease, teardown silently skipped — all of which were
tautologies while the enum had one variant.

Also re-check the binding at claim time rather than trusting the row: a company may have switched
provider since the job was enqueued. If the *current* binding names a different provider, the job is
stale — complete it and let the re-enqueue from §2 do the work.

**Ingest**

1. Load the document by `job.document_id`; a missing row means it was deleted mid-flight — complete
   the job and move on.
2. `chunk_document_text(&document.extracted_text)` — chunking is deterministic, so the cursor stays
   valid across restarts. (It is cheaper than storing a row per chunk and it cannot drift.)
3. Set `ingest_status = 'ingesting'` on the first chunk of the first attempt.
4. From `next_chunk_index`, send at most `MEMORY_DOCUMENT_CHUNKS_PER_LEASE` chunks, calling
   `advance_document_cursor` after **each** one. Then either finish or `reschedule_document_job` for
   immediate re-claim, so one 128-chunk document cannot monopolise the worker and one tenant's
   burst cannot starve another's.
5. When the cursor reaches `chunk_count`: mark the document `ready` with `ingested_provider` and
   `ingested_at`, and complete the job.

**Forget**

1. Rebuild every id with `document_chunk_id(job.document_id, 0..job.chunk_count)` — no document row
   needed, which is why the job carries `chunk_count` and `collection`.
2. `forget_items(remote_database_id, collection, &ids)`, batching the same way.
3. Complete the job and delete its row.

### Error classification — the part that is easy to get wrong

| Outcome | Action |
|---|---|
| `NotReady` (binding went not-ready mid-flight) | `reschedule_document_job` — **no failure attempt consumed**, no `ingest_status` change |
| other `retryable()`: `RateLimited`, `Timeout`, `Unavailable` | `retry_document_job(terminal: false)` — one attempt, backoff |
| `RejectedItem`, `RequestTooLarge`, `InvalidIdentifier`, `MalformedResponse` | `retry_document_job(terminal: true)` — document `failed`, safe error stored |
| `failure_attempts` reaches `MAX_PROVIDER_FAILURE_ATTEMPTS` | terminal, document `failed` |
| lease lost mid-call | `supervise_memory_job_lease` cancels; **do not** commit anything after |

`last_error` is operator-safe text only: a `MemoryProviderError`'s `Display`, never a response body.
The document's `ingest_error` is what phase 5 renders, so keep it short and human.

An execution error is job state, never content — nothing here writes to the document's text.

### Metrics

`memory_document_ingest_total{outcome}`, `memory_document_forget_total{outcome}`,
`memory_document_ingest_duration_ms{provider}`, plus queue depth and oldest-pending age from the
maintenance sweep. `src/application/AGENTS.md`: the metrics must distinguish retries, terminal
failures, lease loss and stuck work — in-process counters alone are not observability, so follow
whatever `runtime_metric_samples` already does for the memory provider panel.

### Stack

The worker → provider chain is the one `src/AGENTS.md` warns about. `Box::pin` the call descending
into the provider **with a comment saying why**, extract the classification `match` into a
non-`async fn` (it needs no `await` and therefore costs no frame), and check
`./scripts/stack-frames.sh` before and after rather than reasoning about it.

---

## 2. Three lifecycle hooks — the ones that are easy to miss

### Switching provider silently loses every document

`select_provider` (`src/adapters/persistence/memory.rs:83`) already deletes the abandoned connection
and enqueues its cleanup inside one transaction. **In that same transaction**, for the company's
documents:

- set `ingest_status = 'pending'`, clear `ingest_error`, `ingested_provider`, `ingested_at`;
- upsert one `ingest` job per document against the **new** provider and its `remote_database_id`,
  resetting `next_chunk_index` to 0.

Without this, switching provider leaves every document marked *In memory* while the new provider
holds nothing — the UI lying is worse than the documents being gone.

`disable_provider` does the same reset and **deletes** the document jobs; the connection-delete
trigger already tears down the whole remote database.

Cap: a company at `MAX_MEMORY_DOCUMENTS_PER_SCOPE` per scope enqueues a bounded number of jobs, so
this cannot become an unbounded write.

### Deleting an agent strands its chunks

`AgentPersistence::delete` (`src/adapters/persistence/agent.rs:236`) is a bare `sqlx::query!`. Make
it a transaction that, **before** the delete cascades the rows away, enqueues a `forget` job per
document of that agent — carrying `collection`, `chunk_count`, `provider` and `remote_database_id`,
which is exactly why `memory_document_jobs.document_id` has no foreign key.

Keep its existing 23503/23514 → `AppError::Conflict` mapping and the library-agent delete guard
trigger untouched. This file uses compile-time macros, so **`cargo sqlx prepare` is required** after
this edit.

### Deleting a company

`delete_company_with_cleanup` (`src/adapters/persistence/company.rs:88`) already locks memory jobs
`FOR UPDATE`, orphans lifecycles and enqueues `memory_cleanup_jobs` before deleting the company. Add,
**before** the remote cleanup is enqueued: delete pending document jobs and bump
`operation_generation` on leased ones so a stale worker's completion no-ops.

Ordering matters — `src/adapters/persistence/AGENTS.md` requires cancellation to be ordered before
remote cleanup. The whole-database cleanup then removes the chunks.

Objects left in the private bucket are orphaned. That matches how email attachments already behave;
**say so in the module doc** rather than leaving it to be discovered.

---

## 3. Tests

Inline `#[cfg(test)] mod tests`, or a `#[path = "…_tests.rs"]` sibling past ~500 lines. Use the
`start_paused = true` virtual-time pattern the existing lease tests use, so a 180-second lease is not
180 real seconds. Every DB-backed test follows the shared-database rules.

Worker:

- `a_document_reaches_ready_after_every_chunk_is_accepted`.
- `a_crash_between_chunks_resumes_from_the_cursor` — advance to chunk 5, drop the lease, re-claim,
  assert chunks 0–4 are not re-sent.
- `a_long_document_yields_the_worker_after_its_chunk_budget` — assert the reschedule, and that a
  second job interleaves.
- `a_stale_worker_cannot_complete_the_replacement_workers_execution` — the generation fence. Two
  competing claimants, not sequential mocks (`AGENTS.md` root, on concurrency protocols).
- `a_not_ready_binding_reschedules_without_consuming_an_attempt` — the distinction the retry budget
  depends on.
- `a_rejected_item_fails_the_document_terminally_with_a_safe_error` — assert the provider's response
  text appears nowhere in `ingest_error`.
- `a_forget_job_rebuilds_every_chunk_id_from_the_row_alone` — delete the document row first, then
  run the job.
- `a_job_whose_company_has_since_switched_provider_is_completed_not_run`.
- **`a_poison_batch_is_not_reclaimed_on_the_next_iteration`** — fill one batch with failures, run two
  consecutive iterations without advancing time, prove the second does not reclaim the same rows.
  `src/application/AGENTS.md` requires this for every retryable worker queue.

Lifecycle:

- `switching_providers_requeues_every_document_against_the_new_provider` — extend the existing
  `switching_providers_retires_the_previous_connection_and_enqueues_its_cleanup`, and follow its
  cleanup discipline at the tail.
- `disabling_memory_clears_document_jobs_and_resets_status`.
- `deleting_an_agent_enqueues_forget_jobs_before_the_cascade`.
- `deleting_a_company_cancels_document_jobs_before_remote_cleanup`.

**Residue is the trap here.** Every one of these creates globally claimable rows, and
`claim_document_job` is table-wide by design. Complete the row, delete it, or push `available_at`
past the horizon before the guard drops — a left-behind pending job is claimed by whichever worker
test runs next, whose registry may not hold that provider, and that test *hangs* rather than fails.
This is not hypothetical: it is written up at the tail of the existing switching-providers test.

---

## Done when

- `DATABASE_URL=… cargo test` green **four consecutive runs** — one green run does not disprove
  residue.
- `./scripts/stack-budget.sh` passes at the stock 2 MiB. If it fails, shrink the chain before
  raising `STACK_BUDGET_KIB`, and record the reason either way.
- `DATABASE_URL=… cargo sqlx prepare -- --all-targets` committed (`agent.rs` is a macro file).
- A real document uploaded through a test or a stub reaches `ready`, and deleting it drains a forget
  job — verified against a live provider if one is reachable.
