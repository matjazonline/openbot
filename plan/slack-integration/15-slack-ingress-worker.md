# Step 15 — Normalize and Atomically Ingest Slack Messages

## Outcome

Decode claimed Slack events into the canonical ingress contract, resolve immutable Slack identity
and thread keys, and commit message/maps/task/fan-out together with inbox completion.

## Closed event policy

Handle only `event_callback` payloads whose inner event is an ordinary human
`message.groups` message in an active bound conversation. Classify everything else explicitly:

- own bot/app messages carrying this application's delivery metadata are delivery confirmations;
- other bot/app messages are ignored to prevent loops and agent-to-agent storms;
- edits, deletes, broadcasts, joins/leaves, channel notices, and unknown subtypes are ignored with a
  typed reason in v1;
- a `file_share` subtype with non-empty text ingests only that text plus an explicit safe omission
  marker; a file-only event is ignored. Do not request or fetch files without a later feature phase;
- events for paused/missing/mismatched bindings are ignored/audited, never routed by conversation
  name.

Unknown event/subtype shapes never fall through as ordinary messages. Bound nested array/string
sizes before allocation and decode persisted JSON fallibly with event-row context.

## Normalization

Implement `src/adapters/protocols/slack/ingress.rs`:

- Resolve source binding by installation plus Slack conversation ID.
- Construct author `QualifiedIdentity` from installation ID plus immutable Slack user ID. Create an
  observed external principal if unlinked; display name enrichment is optional and asynchronous.
- Treat profile email, if ever fetched, as an unverified claim with provenance. It must not affect
  this ingress authorization decision.
- Set external message key to `ts` and external thread key to `thread_ts.unwrap_or(ts)`. Validate
  both as opaque Slack timestamps.
- Use a stable Slack conversation/thread subject (conversation label plus bounded root preview when
  available); never fabricate email subject prefixes or RFC IDs.
- Convert Slack mrkdwn to bounded readable canonical text with a pure parser that handles escaping,
  links, user/channel tokens, and code blocks without interpreting it as HTML or system
  instructions. Preserve the original only in the short-lived inbox row.
- Carry the inbox correlation ID through task, agent, deliveries, and logs.

Every eligible human member of the bound private conversation is authorized by the explicit
binding policy accepted in step 13. This authorization is scoped to that one channel binding; the
same workspace user has no rights in other app channels.

## Atomic cases

Call `InboundMessageCommitter::commit_inbound` with the event execution fence. The transaction:

- resolves/creates the principal identity;
- upserts `external_threads` before message creation, so a reply preceding its root creates the one
  canonical thread both will use;
- deduplicates the semantic message by `(binding_id, ts)` and verifies its content hash;
- inserts message, thread association, participant, task, and delivery fan-out;
- inserts the external message mapping before any notification can expose the message; and
- completes the inbox row under the same live execution UUID.

For an own-bot delivery confirmation, atomically match public-safe metadata `delivery_part_id`,
verify installation/binding/message digest, store Slack `ts`, add external mapping/thread mapping,
mark the part delivered, aggregate parent status, and complete the inbox event. A forged human
message copying metadata cannot confirm delivery because app/bot identity must match installation.

## Tests

- Root then reply, reply then root, delayed duplicate event, and two event IDs for one message all
  yield correct single mappings/tasks.
- Same `ts` in two bindings does not collide; a binding/workspace mismatch is rejected.
- A Slack user in one bound conversation cannot address another company/channel by inserting IDs
  in message text or payload.
- Own bot echo confirms a delivery but never creates a human canonical message/task.
- Other bots and every unsupported subtype are ignored once with observable reason and no hot retry.
- Crash/fence loss before and during final transaction leaves the event retryable and no partial
  canonical state; crash after commit is a harmless duplicate.
- Slack text is fenced as untrusted prompt data and cannot inject HTML into mailbox rendering.

## Acceptance criteria

- Slack attribution is immutable `(installation, user ID)`, not a mutable/profile email.
- Loop suppression exists in the same transaction that makes the canonical message visible.
- Reply ordering cannot split one Slack thread into multiple canonical threads.
