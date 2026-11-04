# 03 — Durable response intents and the completion boundary

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [02](02-terminal-outcomes-and-no-reply.md). A responding workflow cannot commit success
and then lose its final response to a crash. Save its validated terminal result and response
intent, or its explicit suppression decision, in the same database transaction. Publication and
transport sending remain recoverable work outside that transaction.

## Batch 03A — Define the durable relationship

1. Add the smallest persistence representation that links response ownership to the original
   message/channel association, run, terminal execution, frozen contract, and canonical response
   content. Reuse existing message/delivery identities when publication occurs. Do not duplicate
   transport status, provider receipts, or the workflow state machine in this record.
2. Enforce one final response decision per owned incoming association/run using database
   uniqueness and tenant-qualified associations. Replays return the stored record; conflicting
   content or reused keys are rejected. Distinguish a deliberate suppressed result from pending
   publication and from a publication failure.
3. Store the frozen projected content, destination context, authority/policy evidence, and an
   integrity identity. After success, delivery recovery must never rerun the model, re-evaluate
   the result against a new binding, or substitute newly edited content.
4. Make pending publication enumerable and claimable with bounded work, attempts, leases, and
   terminal diagnostics using the existing durable-worker conventions. Wakeups accelerate work;
   the database remains sufficient for recovery if every wakeup is lost.
5. Keep queue state only for work that this record owns: publishing a response into canonical
   messages and the delivery queue. Once published, delivery status is read from the existing
   queue. A local in-app response has a publication receipt without a fabricated external send.
6. Define deletion/retention alongside existing company/channel/thread/run lifecycles, preserving
   live obligations and audit links. Do not introduce unbounded history or orphan delivery rows.

Focused check: `channel_reply_intent_schema_` for tenant/association constraints, uniqueness,
round trips, suppressed versus publishable records, and deletion/retention behavior.

## Batch 03B — Finalize with policy and approval intact

1. Extend the shared terminal completion path after validating the workflow output and channel
   projection. Normal completion atomically commits the result and reply intent; silent completion
   atomically commits the result and suppression. Rollback leaves neither terminal success nor
   its response decision committed.
2. Automatic sending is a messaging effect. Reuse the current authority and protected-action
   rules for the exact content, original source, sender, destination, and run actor. Do not let
   moving publication out of a `message.reply` step bypass approvals or grants.
3. When final response approval is required, freeze the proposed response and park the existing
   terminal execution through the current durable approval machinery before marking the run
   succeeded. Introduce only the typed origin/evidence needed for a channel final response; do
   not synthesize a new user-authored workflow step or a second approval engine.
4. On approval, resume the same run and finalize the exact approved content. Editing it invalidates
   prior approval evidence. Rejection/timeout follows the existing explicit failure/error route;
   it never falls through to an unapproved send. Phase 05 completes the notice/continuation
   integration and its contention cases.
5. After terminal success, revoked authority or a changed policy can block publication/delivery
   with a visible outcome. Do not reopen a succeeded workflow to request fresh approval, rerun
   business effects, or silently reuse stale approval. An authorized replacement communication
   is a new explicit action through the existing control path.
6. Trace every terminal-success writer, including child/human completion and recovery. Put the
   invariant in their shared transaction owner, not in a worker callback that only sees some
   completions. Sample completion uses its sealed sample policy and cannot create production work.
7. Use durable operation identity when a transaction result is uncertain. Observe/reconcile the
   saved completion before retrying, retaining existing fencing, transaction deadlines, lock
   order, continuation accounting, and action-budget checks.

Focused check: `channel_reply_completion_` with competing finalizers, commit/rollback injection,
lost commit acknowledgement, stale fences, cancellation versus completion, suppressed completion,
approval-required parking, invalid result rejection, and sample completion.

## Acceptance

- Every committed responder success has one durable final response decision.
- Invalid contracted output cannot be reported as successful completion with an invented reply.
- Approval can suspend finalization, but completed workflow state never depends on a later
  provider acknowledgement. Delivery failure does not roll back business success.
- No post-commit in-memory task is the sole owner of publication work.
- Run focused checks and affected existing completion/approval/recovery tests under
  [07](07-integration-and-verification.md). Shared completion changes justify broader existing
  regression checks here, before the final full-suite gate.
