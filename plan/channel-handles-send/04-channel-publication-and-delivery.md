# 04 — Channel publication through the existing delivery system

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [03](03-durable-response-intents.md). Recoverable channel publication turns a committed
response intent into a canonical message and, when appropriate, an existing transport delivery.
Workflow execution and delivery success remain independently observable.

## Batch 04A — Consolidate response planning and publication

1. Reuse the source-routing and canonical content rules owned by
   [message runtime](../../src/application/workflow/message_runtime.rs) and
   [message persistence](../../src/adapters/persistence/workflow/message.rs). Extract shared
   application planning and persistence operations where ownership is currently tied to an
   explicit action invocation; do not copy reply routing into a new channel-specific branch.
2. Give automatic responses a typed publication origin and durable response identity. A terminal
   run has no active execution lease: authorize and fence publication by its immutable committed
   intent and current response-worker claim. Do not fake a live workflow fence or weaken the
   guards protecting explicit `message.send`/`message.reply` actions.
3. Publish the canonical outbound message, thread association/correlation metadata, response
   linkage, and delivery record atomically. A lost acknowledgement is resolved by observing the
   same stored publication; duplicate claimants cannot create another message or delivery.
4. Preserve the original channel sender identity, reply threading, tenant scope, audience, hop
   limits, and reply-source authorization. Honor `InAppOnly`, internal messages, quiet ingress,
   and sample restrictions before any transport attempt. Never choose an email destination as
   an implicit fallback for an unsupported transport.
5. Use the transports supported by the checked-out implementation. The scoped workflow routing
   currently distinguishes email and internal replies; adding Slack or another provider is not
   a prerequisite of this plan. Keep provider-specific rendering behind existing transport ports.
6. Recheck current send authority and the integrity of any approval evidence at publication.
   Refusal or changed policy produces an inspectable publication failure, with no success
   fabrication and no new effect under stale authority.

Focused check: `channel_reply_publication_` for email/in-app/internal routing, wrong-source and
foreign-tenant rejection, policy revocation, sample isolation, replay, partial-write rollback,
and two real claimants competing to publish the same response.

## Batch 04B — Recovery, dispatch, and lifecycle

1. Connect bounded response-intent draining to the existing supervised service lifecycle. Reuse
   queue leasing/fencing patterns and shutdown ownership; avoid detached critical tasks or an
   additional generic queue framework. Durable polling must recover lost wakeups and restarts.
2. Keep publication retries separate from delivery retries. Once canonical publication exists,
   the delivery worker owns provider calls and receipts. A failed send retries that delivery;
   it never reopens the workflow or regenerates its result.
3. Preserve the distinction between safe retry, terminal failure, and provider outcome unknown.
   Database uniqueness prevents duplicate publication, not all duplicate external effects.
   Do not automatically resend an ambiguous provider call or already accepted multipart pieces.
4. Enforce existing attempt, concurrency, tenant fairness, resource, and work-time budgets. A
   poison intent cannot starve other tenants. Bound stuck-work queries and expose unresolved
   publication and delivery outcomes using existing monitoring conventions.
5. On source deletion, revocation, cancellation, or expiry, follow existing authority/deletion
   policies and retain a diagnosable outcome. Respect frozen run ownership: edits to the channel's
   current responder do not redirect an already committed response.
6. Establish cancellation semantics at the boundary: cancellation racing terminal completion is
   settled by the workflow transaction; after success, cancelling an unsent reply is a delivery
   control operation. Neither can claim that a provider-accepted message was recalled.

Focused check: `channel_reply_delivery_` and `channel_reply_recovery_` with service reconstruction,
lease expiry, lost acknowledgements, shutdown during claims/sends, provider uncertainty,
multipart partial acceptance, safe retry, bounded poison work, and quiet-tenant progress.

## Acceptance

- A successful workflow with a failed reply stays successful and exposes the failed reply.
- An in-app-only response completes local publication without creating an external delivery.
- Explicit messaging retains its action receipts, approval requirements, and existing option to
  wait for delivery before advancing the workflow.
- There is one canonical publication/delivery pipeline and no message generated by rereading a
  newer workflow output or binding revision.
- Run focused checks plus affected existing messaging/delivery/authority suites, transport and
  sample boundary checks, and SQLx/migration checks under
  [07](07-integration-and-verification.md).
