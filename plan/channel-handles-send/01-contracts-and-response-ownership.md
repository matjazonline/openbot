# 01 — Channel reply contracts and response ownership

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and implementation order

Let a channel receive a message, invoke its responding workflow, and deliver that workflow's
result by default. Workflows retain explicit `message.send` and `message.reply` for intermediate
communication and custom conversations. A new terminal `channel.no_reply` completes successfully
while suppressing the automatic final response. Human input suspends and resumes the same run.

This directory is a proposed implementation plan. None of these documents claims that channel
automatic replies or `channel.no_reply` are implemented. Implement the numbered phases in order;
each depends on the preceding phases. Integrate all phases before the final release gate.

| Phase | Deliverable |
| --- | --- |
| [01 — Contracts and ownership](01-contracts-and-response-ownership.md) | Binding contract, designated responder, immutable response context |
| [02 — Terminal outcomes](02-terminal-outcomes-and-no-reply.md) | Typed completion disposition and terminal `channel.no_reply` |
| [03 — Durable response intents](03-durable-response-intents.md) | Atomic completion, policy approval boundary, recoverable publication work |
| [04 — Publication and delivery](04-channel-publication-and-delivery.md) | Channel response publication through the existing delivery system |
| [05 — Human input and continuations](05-human-input-and-continuations.md) | Prompt delivery, authorized response correlation, approval and continuation integration |
| [06 — Authoring and operations](06-authoring-observability-and-operations.md) | Configuration/editor support, independent status views, operator controls |
| [07 — Integration and verification](07-integration-and-verification.md) | End-to-end races/recovery, complete CI gate, operating documentation |

## Design invariants shared by every phase

- The workflow engine owns execution, waits, decisions, and continuation. Channel application
  logic owns response contracts and publication. The delivery queue owns transport attempts and
  delivery outcomes. Do not introduce another workflow engine or independently mutable overall
  success status.
- A run may produce many messages. Each message can have its own delivery or local publication
  outcome. A successful workflow can have a pending, failed, or uncertain final delivery.
- Exactly one designated binding may own automatic replies for a channel's incoming message
  association. Other triggered workflows may run and explicitly send messages, but their normal
  completion does not generate another default response. Distinct channels addressed by one
  inbound message retain separate response ownership.
- Final response intent is unique for that incoming association and its owning run. Human prompts
  are unique per wait/decision occurrence and recipient; they are not deduplicated by run alone.
- A pause is nonterminal. A human prompt may be sent while its workflow remains waiting. Sending
  a prompt never constitutes human acceptance. A correlated authorized response resumes its live
  wait once and does not also start a fresh run for that same association.
- Quiet/file-only ingress, in-app-only policy, sample isolation, current authorization, message
  bounds, and protected-action approval remain enforced for automatic replies.
- Normal completion sends the contracted result. Explicit silent completion suppresses only that
  automatic response. An earlier explicit reply does not imply silence and is not recalled by it.
- A failed/cancelled run produces no automatic business-result reply in this scope. A declared
  error-handling branch may finish successfully with a reply or silent outcome. Surface unhandled
  failures operationally; do not invent an additional configurable error-message subsystem.
- Only the designated message-triggered root run owns this automatic response. A scheduled,
  manually started, observer, or child run acquires no response authority merely by sharing a
  channel/thread. Child completion cannot silently finish or suppress its parent's response.

## Current code and ownership anchors

These are scoping anchors, not frozen line numbers. Re-query graft and trace callers before
changing each owner; some files are modules wired through explicit `#[path]` declarations.

- [Ingress commit](../../src/application/use_cases/thread/ingest/commit.rs), `CommitPlan::build`:
  accepted channel associations produce admission intents; channel settings do not currently
  publish the workflow result.
- [Message admission](../../src/application/workflow/message_admission.rs), `MessageAdmissions::process`:
  reply correlation precedes binding admission, and multiple matching bindings can start runs.
- [Binding configuration](../../src/application/workflow/binding/mod.rs), `BindingConfiguration`,
  and [binding lifecycle](../../src/application/workflow/lifecycle/bindings.rs): configuration,
  immutable revisions, resources, run authority, and message triggers.
- [Workflow definition](../../src/domain/workflow/definition.rs), `WorkflowDefinition::output_schema`:
  the workflow already owns its business-result schema.
- [Message runtime](../../src/application/workflow/message_runtime.rs), `MessageActions`, and
  [message routes](../../src/adapters/persistence/workflow/message.rs), `reply_target_on`:
  explicit messaging, source-derived destinations, approval, and delivery policy.
- [Completion commit](../../src/adapters/persistence/workflow/batch_commit.rs), `complete`:
  shared progression boundary to trace across pure, I/O, decision, wait, and child completions.
- [Decision persistence](../../src/adapters/persistence/workflow/decisions.rs), `park_on`, and
  [reply waits](../../src/adapters/persistence/workflow/reply_waits.rs), `deliver_on`:
  durable decisions/notices and guarded correlation, including bounded early-reply deferral.
- [Delivery worker](../../src/application/services/delivery_worker.rs), `DeliveryWorker`:
  independent sending, retry, lease, cancellation, and unknown-provider-outcome handling.

## Batch 01A — Define the channel contract

1. Add a small typed response contract to the channel/workflow binding boundary. The workflow's
   business output remains its own schema; the binding selects a bounded, deterministic projection
   into existing canonical subject/body content. Use the existing typed binding/value machinery;
   do not add a scripting language, model-based formatter, or arbitrary JSON-to-text fallback.
2. Normal completion of the designated responder uses that projection automatically. Missing,
   empty, oversized, or schema-invalid required response content is a validation failure, never
   an implicit request for silence. A suppressed completion skips response projection while still
   satisfying the workflow's declared business-output contract.
3. Validate supported projection/schema compatibility during configuration/activation using the
   existing schema facilities. Reject unsupported mappings explicitly; runtime value validation
   remains mandatory. Do not claim unrestricted JSON Schema implication checking.
4. Recipient, source binding, channel, thread, original message, and transport come from trusted
   ingress/routing context. The workflow's result supplies content, not authority to redirect it.
5. Define immutable response context: tenant, original message/channel association, responder
   binding revision, contract revision, initiating run, delivery policy, and acting authority.
   Continuations retain that context instead of replying to whichever message arrived last.

Focused check: implement and run `channel_reply_contract_` unit tests for projection, content
bounds, unsupported schemas, suppressed-output handling, and recipient injection attempts.

## Batch 01B — Bind one responder and capture it at admission

1. Make the responding binding an explicit channel configuration choice, validated as an active,
   eligible binding in that channel and tenant. When configuring the first ordinary responding
   workflow, make this the normal authoring path. Do not infer the responder from query order.
   A channel without a configured responder may still ingest/store messages; show that state.
2. Freeze ownership/configuration through the accepted-message admission snapshot and run
   creation. Edits cannot move an accepted response obligation onto a different workflow silently.
   A selected responder whose trigger does not match starts no response obligation. Record an
   observable admission refusal/failure when one should start but cannot.
3. Enforce selection and immutable associations at persistence, with tenant-qualified foreign
   keys and concurrency-safe updates. Race two responder assignments and prove only one current
   selection survives; reject foreign channels, stale revisions, and unauthorized configuration.
4. Preserve normal observer admission and incoming-reply correlation. A response consumed by a
   live wait must not also become a new automatic-response obligation for that association.
5. Thread the context through existing run snapshots, reads, fixtures, and sample policies. Keep
   sample data sealed: no sample binding or run can acquire a production response obligation.

Focused check: `channel_reply_binding_` database tests plus the affected existing binding,
admission, duplicate-ingress, quiet-message, and sample-boundary tests.

## Verification rules for all phases

New test prefixes in this plan are required future test names, not existing coverage claims.
For each prefix, run `cargo test --locked --offline --lib PREFIX -- --list`, confirm the selected
tests are intended and nonzero, then run `cargo test --locked --offline --lib PREFIX` with a
disposable test database configured. Record commands, results, and executed counts per batch.

Follow [07](07-integration-and-verification.md) for environment setup, SQLx cache regeneration,
existing regression selections, and the final full-suite gate. Use `own_database` for unscoped
claims/sweeps. Every new concurrency protocol needs actual competing claimants. Broaden checks
early when changing shared compiler, admission, or completion infrastructure; do not repeat the
entire suite after each batch. Explicitly report unavailable checks.

Acceptance: response ownership and schema are deterministic, authorized, immutable for an
admitted run, and represented independently of workflow business data and delivery success.
