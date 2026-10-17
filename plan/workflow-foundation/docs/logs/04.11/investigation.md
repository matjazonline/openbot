# 04.11 exploration (NOT an approved architecture decision)

No backward compatibility: reset the database when schemas/payloads do not apply;
no backfill, compatibility shim or staged rollout.

User confirmed users and channels; reject groups for now. Original requirements:
04-actions-http-and-delivery.md provider-neutral messaging and Acceptance;
EXPANSION.md04.11 and cross-point preservation. No04.11 source edits yet.

Existing seams inspected through graft and exact spans:
- registry/actions.rs message.send(content,destinations[kind,id]), message.reply
  (explicit source_message,content); schemas.rs result message_id/status accepted.
  content is subject/body, no attachment or provider-specific input.
- DeliveryComposer compose.rs resolves live channel binding, freezes provider parts,
  stable key from logical source+destination; predicted email Message-ID already exists.
- thread::insert_message_on and associate_message_on allow canonical write and
  multi-thread associations inside caller transaction. MessageWrite requires a thread;
  workflow_runs carries optional channel_id/thread_id and actor/correlation.
- delivery::enqueue::insert_delivery_on shares caller transaction, freezes parts,
  absorbs same binding+idempotency key. task_id is legacy-only; workflow links must
  be separate tenant-scoped relations, not populate the legacy task field.
- delivery/queue.rs owns claims, begin_part, complete_part, fail/release/reap;
  part Sending marks possible provider entry; Delivered means provider acceptance.
  Prepared/Retryable only can enter; unknown never resends; siblings retained.
- Potential gap to verify: complete_part SQL lacks a part.status='sending'
  predicate; while a multi-part parent retains its lease a duplicate callback could
  overwrite an accepted part. Add a meaningful duplicate-callback test before fix.
- delivery/cancellation.rs already separates unsent from sending/unknown and
  retains late receipts. Extend its responsible helper to workflow-linked IDs;
  avoid duplicating state machine.
- workflow/waits.rs uses reason effect already in SQL but current resume handles
  event/timer only. workflow_waits unique(company,run,execution) assumes one wait per
  execution. Direct model calls may eventually suspend multiple times per execution;
  phase06 owner integration must be explicit, not silently complete an agent step
  with a message result.
- WorkflowHandler::execute returns Result<Value,WorkflowFailure>; supervise_io
  validates still-live lease before completion. Atomic park closes the lease, so an
  explicit suspended outcome is needed rather than abusing errors/lease-loss.
  Can retain existing result alias for existing supervised test futures and introduce
  outer handler outcome conversion; inspect callers before deciding.
- Shared ActionService prepare_step/prepare_tool already owns immutable invocation,
  digest, model-call mapping. PostgresActionDispatch::authorize_on owns company-first,
  run/execution/job/attempt fence and current frozen authority. Reuse this boundary.
- Dispatch SQL allows local/remote only and deferred local guard requires atomic
  receipt plus still-processing fence. Queued delivery cannot claim accepted local
  receipt. A delivery-specific dispatch kind/guard is a candidate; must carry original
  commit-time lease expiry if atomic park clears the job fence. No decision accepted.
- Publication snapshots are required by validate_run_authority; message built-ins
  need trusted explicit snapshots/current authority, not caller supplied tool policy.
  Existing publication authority routes slotless tools to HTTP registration only.

Architecture still to resolve together:
1. Scoped destination routing: channel publication and user email; require explicit
   run/source channel for email (never pick first channel). Exact source UUID,
   same-tenant/thread checks, reject private-source exfiltration, frozen rendering.
2. Shared invocation authority and atomic canonical/linkage/queue/wait transaction,
   commit-time fence proof, ambiguous acknowledgement and restart observations.
3. Provider receipt event collection and action acceptance. Do not create a second
   delivery queue. Avoid parent→run locks conflicting with cancellation run→parent.
4. Direct-tool pending result and later agent-resumption seam; never substitute a
   message output for agent.run output. Keep phase06/08 obligations honest.
5. Cancellation/revocation/deadline before actual begin_part; late receipts preserved.
6. Failed/unknown/mixed destinations remain explicit; no retry of accepted siblings.
7. Focused competing-claimant, atomic rollback, exact reply threading, duplicate
   callback, crash-after-send, mixed destination, cancelled late acceptance tests;
   combined original phase04/full Rust+DB/migration/SQLx/static/stock/lease final gate.

Caller tracing before edits (2026-10-17, while D1 review runs):
- supervise_io → WorkflowWorker execute_io/process and completion/control/lease/
  recovery/supervise tests. WorkflowLeases implemented by PostgresPersistence and
  supervise_tests Port; generic/qualified impls still require exhaustive search.
- CANDIDATES_SQL → polling::poll_work, fair_polling::discover/company_query. Fair
  discovery and diagnostics must share the new receipt-event eligibility.
- cancel_task_deliveries_on → task::withdraw_pending_work_on → stop_task and channel
  agent removal. Preserve all legacy cancellation semantics; affected suites include
  delivery cancellation, task release, channel assignment and workflow job projection.
- complete_part → delivery_worker::send_part; real delivery tests/cancellation tests
  and delivery_worker mock implementation. Callback API itself need not change.
- supported_publication → publication, binding selection, admission and replay.
- wait_commit::resume_on → resume_wait/sweep_waits; preserve event/timer paths.
- marker_owned_effect_kind → recovery and pending recovery → leases/completion/
  budgets and reconciliation histories. Message kinds may join only when actual
  composition proves every effect goes through the message marker boundary.
- messages.subject has a2048-byte SQL limit; content() currently uses generic
  MAX_TEXT. Runtime/compiler must align with the canonical/provider subject bound.
- Member removal preserves/demotes principal (remove_member at company_invite.rs
 355–384); avoid FK to mutable company_members for historical recipient identity.
