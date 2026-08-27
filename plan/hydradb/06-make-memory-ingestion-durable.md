# Make Memory Ingestion Durable

## Goal

When a channel is configured to persist memory, make accepted application output eventually reach
HydraDB despite transient failures, restarts, or crashes after the user-visible response commits.

## Current Risk

Memory ingestion is a synchronous best-effort call after reply/outbox state is recorded. Failures
are logged and swallowed, so transient HydraDB outages permanently lose memory. A crash between the
reply commit and the provider call has the same result.

## Design

Use a durable memory-ingestion outbox:

- Create one durable row per logical memory item and target collection.
- Use the existing stable task/channel/agent-derived item ID plus collection as the idempotency key.
- Enqueue memory rows in the same transaction as the successful reply/task outcome that makes them
  eligible.
- A bounded worker claims rows globally with `FOR UPDATE SKIP LOCKED`, a generation-fenced lease,
  retry budget, backoff, and terminal state.
- Provider ingestion remains idempotent; a retry always reuses the same external item ID.
- Failed, suspended, partial, or provider-error-as-output runs enqueue nothing.
- Company deletion prevents new ingestion, terminally cancels pending ingestion for that company,
  and orders cancellation before remote cleanup.

## Persistence Shape

Prefer explicit columns over opaque JSON for queue state and ownership. Persisted content may use a
versioned payload only if decoding is fallible and bounded. Include:

- logical operation/idempotency key;
- company, channel, optional agent, provider, remote database, and collection identifiers;
- bounded user context and final answer or stable references from which they can be reconstructed;
- status, attempts, availability, lease generation/expiry, safe error, and timestamps;
- constraints coupling status to lease fields and uniqueness for the logical operation.

Document retention and deletion because the job payload duplicates message content while pending.

## Implementation Steps

1. Define a focused application port for atomically committing reply/outbox state and memory
   ingestion intents. Do not add queue methods to the already broad connection port.
2. Add an additive migration with state-machine constraints, due index, idempotency uniqueness, and
   tenant-consistent foreign keys where rows retain company/channel IDs.
3. Update inbound and scheduled success paths to construct bounded ingestion intents before the
   transactional commit.
4. Change the provider port to ingest one collection item per durable job, returning a typed result.
5. Implement a supervised ingestion worker using the lease protocol from
   `05-supervise-memory-job-leases.md`.
6. Classify retryable versus terminal provider errors and expose poison rows operationally.
7. Remove synchronous post-response ingestion after the durable worker is live.
8. Add queue depth, age, retries, terminal failures, lease loss, and throughput metrics.

## Tests

- Crash/restart after the application commit but before HydraDB invocation still results in later
  ingestion.
- Provider timeout and rate limit retry with the same external item ID.
- Duplicate application retries create one job per collection and one logical provider item.
- Reply/task commit failure creates no ingestion job; successful commit creates all expected jobs.
- Partial collection failure retries only failed collection rows.
- Two claimants cannot own one ingestion row, and stale generations cannot complete replacements.
- Poison-batch regression: failed rows back off and do not hot-spin or monopolize the next batch.
- Company deletion cancels pending ingestion before cleanup and leaves no later provider writes.

## Rollout

- Add the table and worker first while synchronous ingestion remains disabled behind a feature flag.
- Enable durable enqueue and verify queue metrics/idempotency in staging.
- Remove the synchronous path only after the worker has drained test traffic correctly.

## Acceptance Criteria

- A successful configured persistence intent is durable before the task is considered complete.
- Transient failures and restarts do not lose memory.
- Retries are idempotent per collection and cannot duplicate provider items.
- Terminal failures are visible and bounded rather than silently discarded.

