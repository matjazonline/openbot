# 03 — Conversation turn ordering and recovery

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-durable-exchanges-and-completion.md). Serialize logical agent exchanges within
one conversation so the next exchange can observe the preceding completed answer. Other
conversations remain independently runnable. Durable order is assigned when an agent activation
joins the stream; this does not claim network-arrival order across separately scheduled workflows.

## Batch 03A — Acquire a turn without blocking a worker

1. Add conversation-turn eligibility to the existing job/admission/claim path. Use the exchange's
   durable order and one active owner per conversation. Resolve identity and enqueue the original
   input once; choose history only after the turn becomes eligible, not when it first queues.
2. Reuse existing durable waits and scheduler readiness for blocked turns. Do not hold a database
   transaction, connection, process mutex, or workflow worker slot across a provider call or while
   another exchange owns the turn. Do not introduce a polling loop per conversation or another
   generic queue framework.
3. Specify and document lock order among conversation, execution, checkpoint, job owners, and
   the `workflow_runs` row that 02B and 04A read `FOR SHARE` for the content level.
   Use fencing and compare-and-update transitions consistent with the existing workflow lease.
   A stale worker cannot select different history, publish output, or release another owner's turn.
4. Preserve logical turn ownership across retries and nonterminal approval/tool/human waits.
   Lease expiry transfers execution ownership of that same exchange; it does not let the next
   queued exchange pass an unfinished one. Expiry/cancellation deadlines provide a bounded exit.
5. Terminal output, terminal failure, and cancellation release the position and make successors
   eligible in a guarded transaction. Wakeups only accelerate work: bounded durable recovery must
   find eligible successors even if a process crashes before emitting a wakeup.
6. Bound pending turns per conversation and tenant, readiness queries, claim work, and wait age.
   Preserve run deadlines and distinguish waiting for a turn from a provider failure; waiting
   must not consume model tokens or manufacture failed provider attempts. Overload receives a
   controlled refusal rather than an unbounded queue.

Focused checks: `agent_history_ordering_` using real competing claimants for the same stream,
independent streams progressing concurrently, queue overload, cancellation of queued work, and
two queued calls observing prior committed output in durable order. Assert that waiting work
does not enter the provider and that no worker holds a connection while awaiting another turn.

## Batch 03B — Recover waits, ownership, and interrupted completions

1. Exercise restart after enqueue, after turn acquisition, after frozen preparation, during a
   provider call, and after committed output before scheduler wakeup. Recover the same exchange,
   selected history, and accepted result through existing checkpoint/effect reconciliation.
2. Wire workflow/job terminalization and deadline handling to settle any unfinished exchange.
   Include cancellation before checkpoint creation and failures that bypass the normal model
   return path. Never leave a dead queue head blocking a conversation indefinitely.
3. Preserve existing unknown-provider-outcome and action replay rules. Turn recovery is not
   permission to repeat a tool effect or an ambiguous model request blindly. Reuse existing
   failure accounting and operator reconciliation paths; do not hide uncertainty as success.
4. Keep a parked human/approval wait resumable while it owns the conversation turn. Incoming
   responses must reach the existing authorized wait-correlation path before being queued as new
   agent work. Phase 05 proves this with channel admission and repeated human rounds.
5. Enforce private child branches from phase 01 at child admission. A parent awaiting a child
   must not hold the only position in a conversation that the child is then required to acquire.
   Reject prohibited reuse before dispatch; do not silently release the parent and reload its
   context later. Nested branches retain existing depth/resource bounds.
6. Put recovery work under existing supervised lifecycle and shutdown ownership. Reuse existing
   bounded scans and durable readiness machinery wherever possible; no detached critical tasks.

Focused checks: `agent_history_recovery_` with actual ownership contention, lease replacement,
lost wakeups, cancellation versus completion, deadline versus resume, stale completion attempts,
and reconstruction from each durable boundary. Include parent/child same-agent deadlock refusal
and an independently progressing conversation. Use `own_database` for unscoped claims/sweeps.

## Acceptance

At most one unfinished exchange is active in a conversation. A recoverable wait retains its
position; terminal work cannot strand successors. Recovery neither changes selected history nor
duplicates model/tool effects. Run affected workflow lease, deadline, cancellation, approval,
child, and delivery-wait regressions under [07](07-integration-and-verification.md).
