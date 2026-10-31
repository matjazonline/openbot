# 03 — Durable runtime and persistence

## Outcome and dependencies

Depends on phases 1–2. Execute validated workflows durably with independent message runs,
atomic progression, bounded workers, and recovery. Use scripted handlers first.

## Admission and independent contexts

Create normalized records for workflows/versions, bindings/revisions, runs, step executions,
waits, and audit events. Reuse/refactor existing jobs and attempts for execution ownership.
Use foreign keys and scoped uniqueness constraints to enforce identity relationships.

Admission atomically records the trigger identity, selected immutable bundle, input snapshot,
binding revision, limits, and first runnable job. Repeated delivery of the same source event to
the same logical binding rejoins its run, even if the binding was updated after first admission.
Different messages create different runs; never deduplicate merely by conversation or content.

Capture a committed conversation-history boundary with the input. `context.load` must never
read later messages into that run implicitly. Ordinary messages arriving during human review
start their own runs immediately. A matched decision/event response resumes the exact waiting
execution instead of being misidentified as a new ordinary message.

There is no conversation execution lock. Runs may finish and reply out of order. Their inputs,
step outputs, human decisions, agent checkpoints, and reply associations remain independent.
Explicit shared-memory operations can observe or change shared knowledge; they do not merge contexts.

## Execution and commits

1. Queue payloads contain IDs, not serialized contexts. Resolve and freeze step inputs on first
   activation; persist logical execution identity independently of worker attempt identity.
2. Execute pure mapping/routing steps in bounded transactional batches. Record each step's inputs,
   outputs, and route. When the batch budget is consumed, enqueue continuation rather than spinning.
3. Lease I/O work with generation fences and heartbeats. Keep external I/O outside transactions.
   On lease loss or cancellation, cancel the actual future, not just its outer task handle.
4. Complete a step, update run state, append audit events, and create the successor job in one
   transaction. A unique logical activation prevents duplicate successor creation.
5. Parking commits the wait and notification intent together, releasing worker capacity. Resumption
   atomically consumes a matching event, records the result, and schedules the continuation.
6. Adopt a consistent run-first transition lock order, followed by owned executions/waits/actions.
   Child completion schedules a parent wakeup instead of locking two run trees in opposite order.
7. Poll PostgreSQL as the correctness mechanism. Notifications/SSE only reduce latency and prompt
   rereads; lost wakeups cannot strand committed runnable work.

## Failure and recovery

Use classified failures and bounded retry/backoff. Lease expiry and abnormal worker death consume
attempts. Completed results are reused, never rerun to rebuild context. A stale worker cannot
commit a result or create work after losing its lease.

All runs have a deadline; every wait has a deadline and sweeper transition. Enforce step, model,
repetition, total activation, and root budget limits. Bound context and output sizes; retain large
artifacts through scoped storage references rather than copying them into every record.

Cancellation stops future scheduling and attempts to cancel current work. Preserve receipts for
external effects already accepted. An unknown effect outcome requires phase 4 reconciliation.
An explicit retry command resumes safely from the failed execution; changed inputs or definitions
require a new run. Retrying cannot reset action identities or replenish root budgets silently.

## Acceptance

- Competing workers cannot own/advance the same execution; test real concurrent claimants.
- Inject crashes before and after activation, result commit, parking, and successor creation.
- Duplicate ingress/jobs/events create no extra run, successor, or decision consumption.
- Two messages in one thread run concurrently; one can wait while the other sends its own reply.
- Test history cutoff, stale lease writes, cancellation/completion races, and restart recovery.
- A poison batch is not reclaimed on the next poll without time advancing.
- Enforce per-company fairness and global concurrency under sustained mixed-tenant load.
