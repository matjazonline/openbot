# Private and Customer-Visible Message Boundaries

## Expected result

Every persisted message has an explicit audience boundary, and every thread association has an
explicit entry kind. Internal notes, delegated findings, tool results, and system events cannot be
delivered externally. Drafts are separate versioned artifacts and do not become canonical messages
until somebody or an approved policy publishes them.

This design keeps three independent concepts independent:

- `MessageRole` says who authored content: human, agent, or system.
- `MessageDisposition::{Answer, FileOnly}` says what ingress should do.
- `MessageAudience` and `ThreadEntryKind` say where stored content may appear and how it is used.

## Domain and persistence decisions

- Add `MessageAudience::{ExternalConversation, InternalOnly, LegacyUnclassified}` to canonical
  messages. It is a content-level maximum audience and may never be widened in place.
- Add `ThreadEntryKind::{Conversation, Note, Delegation, SystemEvent}` to `thread_messages`. The
  association kind controls rendering and prompt labelling; channel and thread ACLs continue to
  decide who may read the association.
- Attachments inherit their canonical message's audience. A derived artifact records its source
  identifiers and cannot be made less restrictive by copying metadata.
- Add the minimal `ResponseDraft` foundation used by manual handoff and later expanded by the
  review plan: company, channel, thread, optional task, immutable version, author principal, body,
  recipient snapshot, status, and timestamps. Draft content is not inserted into `messages`.
- Publishing creates a new `ExternalConversation` message from one exact draft version and queues
  its delivery in the same logical transaction. The draft and its history remain immutable.

## Migration and enforcement

- Backfill external inbound messages only when the author is an external principal, and external
  outbound messages only when a durable delivery proves they were published to an external
  destination. Backfill known team-authored notes, internal agent traffic, and system events as
  `InternalOnly`.
- Use `LegacyUnclassified` when existing data does not prove either classification. It remains
  readable under existing channel ACLs but is never directly deliverable.
- Replace references to the removed `is_context_only` persistence concept with the current
  `MessageDisposition::FileOnly` ingress behavior. Replace "outbox" terminology with canonical
  messages, `message_deliveries`, delivery parts, and external mappings.
- Require the external delivery composer/enqueue boundary to accept only an
  `ExternalConversation` message produced by an explicit publish operation. Internal channel
  delivery remains a separate permitted path for `InternalOnly` content.
- Keep private source material available to the agent only inside clearly labelled untrusted and
  internal-only prompt sections. Private evidence may inform newly drafted wording, but the source
  itself is never promoted or quoted automatically.
- Apply the boundary to exports, APIs, mailbox projections, mirrors, outreach, pipeline output,
  summaries, attachments, and future transports. The mailbox must visibly distinguish customer
  conversation, internal notes, drafts, delegation, and system events.

## Dependencies and sequencing

Implement after task ownership is stable. Human notes and manual handoff consume this audience and
draft foundation; delegation, review, operational work items, and notifications consume it later.

## Test and acceptance plan

- Database constraints reject unknown audience/kind values and cross-tenant associations.
- Every direct reply, mirror, outreach, pipeline, approval, and human-send path proves an
  `InternalOnly` or `LegacyUnclassified` message cannot enter external delivery.
- Tests cover private content influencing a new draft without copying the source verbatim,
  attachment inheritance, forwarding, quoted history, multi-channel association, and legacy rows.
- Publishing the same draft version twice creates one logical external message/delivery; publishing
  a stale or superseded version fails.
- Existing quiet/FileOnly ingestion still files the message without running an agent, while the
  stored audience is determined independently.
- A user can identify each entry's audience and kind without inspecting transport headers or
  provider metadata.

