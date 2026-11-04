# 02 — Durable exchanges and accepted-output completion

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-contracts-and-conversation-identity.md). Persist each original input once and
make a schema-valid agent output durable at the same boundary as its final checkpoint. Preserve
the exchange independently of subsequent workflow business state and channel delivery.

## Batch 02A — Store conversations and logical exchanges

1. Extend the existing persistence model with the smallest conversation/exchange relationships
   needed for scope, order, original input, output, checkpoint linkage, and selected-history
   provenance. Reuse existing immutable input/output payloads where their ownership, lifetime,
   and indexed access are sufficient. Where a logical payload must be stored separately, identify
   its authoritative writer and transaction; never maintain two independently mutable copies.
2. Enforce tenant-qualified foreign keys and scope agreement between conversation, agent, run,
   execution, and checkpoint. Unique logical execution identity prevents duplicate exchanges;
   unique conversation order prevents duplicate positions. A repeated workflow step occurrence
   is a new activation, while another attempt at that activation retains the same exchange.
3. Persist the original input and queue position atomically before any provider request can be
   admitted. A crash before checkpoint creation leaves recoverable queued work, not a forgotten
   input. A duplicate command returns the same exchange; changed input under that identity is a
   conflict. Do not use a timestamp as the only ordering or deduplication mechanism.
4. Give each exchange `access_level public.access_level NOT NULL`, using the domain from
   [knowledge access levels 01A](../knowledge-access-levels/01-access-levels-and-member-levels.md).
   At enqueue, set it to the run's `content_level`: the input is built from run data, so it is
   already that sensitive. The level only rises later (02B). Index it with the
   recent-history access path so the level filter in 04A stays in SQL.
5. Store only bounded values, identifiers, and safe provenance. Provider secrets, credentials,
   action approval tokens, and raw request headers remain outside history. References to files
   retain their normal access checks and lifecycle; avoid embedding unbounded attachment bytes.
6. Define indexes for tenant-scoped recent history, ordered pending turns, and source-execution
   lookup. Design the deletion/retention relationships with phase 06 now: active work and frozen
   preparations cannot lose required payloads through a cascade from an unrelated run cleanup.
7. Update reads, writes, fixture builders, schema checks, and sample persistence boundaries together.
   Follow the repository's fresh-schema/reset policy; do not backfill old checkpoint blobs into
   conversations or keep legacy decoding paths.

Focused checks: `agent_history_storage_` database tests for full round trips, immutable input,
conflicting replay, repeated activations, foreign associations, database-enforced uniqueness,
bounded values, query access paths, and deletion constraints. Add `agent_history_level_` cases:
an exchange enqueued after a level-5 read is stamped 5, and the schema rejects 10. Regenerate SQLx metadata for SQL
changes and execute runtime-query tests even when no metadata entries change.

## Batch 02B — Commit output with the final checkpoint

1. Trace final-output writers from [the harness runtime](../../src/adapters/harness/rig/workflow.rs)
   into [checkpoint persistence](../../src/adapters/persistence/workflow/checkpoints.rs). Extend
   their shared transaction so validated final output and the completed exchange commit together.
   An after-return callback or detached task is not the correctness boundary.
   In that same transaction, read the run's `content_level` with the run row locked
   `FOR SHARE`, and set the exchange level to
   `GREATEST(access_level, content_level)`. Tool reads during the exchange (for example
   `search_knowledge`) raise the run level in their own observation commits. The accepted output
   is therefore stamped at least as high as everything it could have used. A failed or cancelled
   exchange is stamped the same way at terminalisation, because its stored input stays
   inspectable.
2. Record the accepted structured value exactly once. Invalid model output and repair candidates
   remain in existing diagnostic/checkpoint records; they cannot become the exchange's accepted
   answer. A successful repair completes the existing exchange, not another conversation turn.
3. Commit exchange completion and release of its active conversation position together. Phase 03
   supplies the ordering/fencing protocol. Workflow terminal success may happen later; failure
   or cancellation after accepted output must not erase or replace that completed exchange.
4. A failure or cancellation before output acceptance records a terminal exchange without an
   assistant answer and releases its turn through the same guarded owner. Preserve unknown
   provider outcomes and usage accounting honestly; a diagnostic state is not invented output.
5. Reconcile an ambiguous commit using the durable execution/exchange identity. If output is
   already committed, return it without another provider request or append. Keep stale-owner
   refusal, current authority checks, checkpoint revision checks, and existing transaction bounds.
6. Keep parent results, model-only steps, sample completions, operator recovery, and harness error
   paths exhaustive at shared matches. Only eligible agent executions own these exchanges.

Focused checks: `agent_history_completion_` with competing finalizers, rollback injection, lost
commit acknowledgement, stale fences, invalid output/repair, cancellation before and after output
acceptance, and workflow failure after accepted output. **Competing claimants:** a parallel
branch commits a level-5 observation while this exchange accepts output. The completed
exchange's level is never below the content level of any observation its checkpoint contains. Assert exact row identities and counted
model calls, not only eventual state. Run affected checkpoint, agent-driver, recovery, and usage
regressions; these shared writers justify broader focused verification here.

## Acceptance

Every provider-entering eligible call has a durable original input. Every accepted final agent
output has exactly one completed exchange. Workflow/channel outcome changes never rewrite it.
Record batch evidence under [07](07-integration-and-verification.md).
