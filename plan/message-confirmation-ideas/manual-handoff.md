# Manual Handoff for Outside Replies

## Summary

Add a durable shared-mailbox handoff workflow instead of per-user unread state.

When an ordinary outside reply reaches an existing thread, the channel's effective policy either
runs the agent immediately or files the message and creates a **Needs instruction** work item.
Opening the thread or reading a notification never clears the handoff; an explicit team action does.

This plan consumes the general-improvements audience, internal-note, response-draft, task-ownership,
and operational-attention contracts rather than defining competing versions of them.

## Configuration and policy

- Add `ExternalReplyHandling::{Automatic, ManualHandoff}`.
- Store `external_reply_handling` on companies and a nullable channel override:
  - `NULL`: inherit the company policy live;
  - `Automatic`: run the normal answering path;
  - `ManualHandoff`: hold eligible replies for the team.
- Preserve `Automatic` for existing and new companies. Expose company default and channel
  inheritance in settings and JSON representations.
- Treat a sender as outside when its resolved `CompanyMembership` is `None`; allowlisted customers
  remain outside.
- Hold only messages that continue an existing thread, would otherwise make that channel answer,
  are not explicitly `FileOnly`, and are not correlated outreach/quorum replies.
- New outside conversations and outreach replies retain current automatic behavior. For
  multi-channel input, hold only manual channels while automatic channel targets proceed normally.

## Handoff state and responsibility

- Add a tenant-scoped `thread_handoffs` row for the current handoff of a thread: generation UUID,
  source message, state, optional responsible principal, business priority/due time, monotonic
  version, and timestamps.
- Append immutable handoff events for creation, generation replacement, claim/reassignment,
  priority/due changes, drafting, draft readiness, publication, dismissal, and failure recovery.
- Use `needs_instruction`, `drafting`, `replying`, and `draft_ready` as active states. Database
  constraints enforce their valid references.
- Default a new handoff to the channel team queue with `Normal` priority and no business due time.
  Authorized teammates may claim/reassign it or set priority/due time through versioned,
  idempotent, audited commands.
- Persist the inbound external message, its `ExternalConversation` classification, handoff
  transition, task targets, and outreach transitions in the existing atomic inbound commit.
- A later outside reply creates a new handoff generation. An old task, draft, send, or dismiss
  operation cannot mutate the newer generation.
- Project `needs_instruction` and `draft_ready` into the shared `AttentionItem` read model.
  `drafting` and `replying` remain visible non-actionable progress states.

## Generate, publish, and dismiss

- A normal team reply with `Answer` disposition releases the matching generation:
  - `InAppOnly` enters `drafting`;
  - `Send` enters `replying` and follows ordinary external publication.
  A quiet/FileOnly internal note never changes handoff state.
- **Generate draft** writes a team-authored `InternalOnly` instruction through the shared note use
  case and creates one fenced agent task. Completion writes a `ResponseDraft`, not an unsent
  canonical message, and moves the matching generation to `draft_ready`.
- Do not add handoff state or content to `InboundTaskPayloadV1`. Add an immutable,
  tenant-scoped handoff-run correlation keyed by task ID and handoff generation; workers reload it
  from persistence. This preserves stale-generation fencing without snapshotting entities in a
  durable payload.
- A draft-ready handoff offers:
  - **Send draft**: publish the exact current draft version;
  - **Edit and send**: create a new immutable version and publish it under the applicable review
    policy;
  - **Dismiss**: resolve the generation and append an auditable `InternalOnly` system event.
- Send and dismiss lock the handoff generation/version and use a stable command UUID. Publication
  creates one new `ExternalConversation` message and one logical delivery; retries cannot enqueue
  another.
- Resolve the handoff when the logical external delivery is durably enqueued. A later permanent or
  outcome-unknown delivery failure becomes its own operational attention item rather than silently
  reopening the completed editorial decision.
- Terminal draft-task failure or explicit stop returns only the matching `drafting` generation to
  `needs_instruction`; retryable failures keep it in progress.

## Mailbox updates

- Show durable **Needs instruction** and **Draft ready** badges on thread rows and a prominent
  banner in the open thread, including responsible party, age, priority, and due/overdue state.
- Counts come from the operational attention projection, not notification unread state. Merely
  opening the thread does not change them.
- Emit identifier-only PostgreSQL/SSE wake-ups after committed material changes. Subscribe before
  querying and re-query on initial connect, reconnect, or lag.
- Notification email/push behavior belongs to the final actionable-notifications plan.

## Test plan

- Verify company inheritance, both channel overrides, migration defaults, and JSON/form round trips.
- Cover team versus outside sender, new versus existing thread, quiet reply, passive CC, outreach
  reply, and mixed multi-channel routing.
- Database-test that held input and handoff commit together and no answering task exists until an
  explicit release/generate action.
- Add competing tests for duplicate inbound delivery, two Generate draft actions, outside reply
  versus team instruction, stale task completion versus a newer generation, edit versus send, and
  send versus dismiss.
- Verify task correlation is loaded from durable rows rather than payload snapshots and that stale
  workers cannot write a draft or clear a newer handoff.
- Verify exact draft-version publication, logical-send idempotency, outbound threading, delivery
  failure attention, restricted-channel authorization, and cross-company IDs.
- Verify initial rendering, live reconciliation, counts, responsibility changes, and that opening or
  reading a notification never clears attention.
- Run formatting checks, offline compilation, migrations, SQLx preparation, the database-backed
  suite, competing-claimant cases, and the stock-stack budget for worker-path changes.
