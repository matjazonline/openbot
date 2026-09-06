# General Improvements Roadmap

## Business objective

Improve shared-inbox operational throughput without weakening tenant isolation, delivery safety, or
the transport-neutral architecture. A teammate should be able to answer:

1. Who owns this work?
2. What is it waiting for?
3. Who must act next, and by when?
4. Which content is private versus customer-visible?
5. Has the intended response been reviewed and durably queued?

Technical task state, unread notification state, and business responsibility remain separate.

## Delivery sequence

1. Complete and stabilize the `task_ownership_and_transfer_1.md` implementation plan. Ownership
   fencing is the prerequisite for every later human/agent responsibility change.
2. Implement [message audience and draft boundaries](message_visibility_2.md).
3. Implement [first-class internal notes](human_quiet_notes_3.md), then update the companion
   [manual-handoff workflow](../../message-confirmation-ideas/manual-handoff.md) to consume notes,
   drafts, ownership, and audience rather than defining alternatives.
4. Add the read-only [delegation status](delegation_status_4.md).
5. Add versioned [delegation controls](delegation_controls_5.md).
6. Add [response provenance and review](answer_provenance_and_review_6.md).
7. Add the shared
   [operational work queue and business metrics](operational_work_queue_and_metrics_7.md).
8. Add [actionable notifications](actionable_notifications_8.md) as the final projection over
   stable source transitions.

Do not implement a later plan by introducing a temporary competing state model. If an earlier
contract is not available, either implement its minimal foundation first or defer the dependent
surface.

## Shared contracts

- Canonical messages own content and maximum audience; thread associations own entry kind and
  remain subject to channel/thread ACLs.
- Drafts are immutable versioned artifacts. Publishing creates a new externally classified
  canonical message and one logical delivery.
- Source workflows own lifecycle and responsibility. `AttentionItem` and notification records are
  projections, not replacement state machines.
- Every mutation is tenant-scoped, authorization-checked, version-fenced, idempotent, and audited
  with typed actor/reason data.
- PostgreSQL/SSE events carry identifiers only and act as wake-ups; readers re-query current state.
- External providers are not exactly once. The system guarantees one logical delivery creation,
  preserves stable idempotency, and surfaces outcome-unknown rather than blindly retrying.
- Durable task payloads contain identifiers and delivery facts only. Workflow state is stored in
  normalized rows and reloaded by workers.
- Add indexes only from representative query plans and production-shaped evidence.

## Release gates

- Use additive migrations and staged rollouts. Do not rewrite a migration applied to any persistent
  environment.
- Keep existing automatic-send, quiet-message, and outreach-timeout behavior as migration defaults
  unless a plan explicitly changes it.
- Before enabling each mutation surface, pass formatting, offline compilation, migrations, SQLx
  preparation, database-backed tests, relevant competing-claimant tests, and the stock-stack budget
  when worker supervision changes.
- Require cross-company and restricted-channel authorization tests for every new list, detail,
  download, deep-link, and mutation route.
- Measure unassigned work, oldest actionable age, time-to-claim, delegation wait, review turnaround,
  time-to-logical-external-response, and permanent delivery failures. Use these results to choose
  later SLA, business-hours, digest, and escalation policy rather than inventing defaults.
