# Delegation and Waiting Status

## Expected result

From one view, a user can answer who owns a task, who or what it is waiting on, what has arrived,
what happens next, and when a decision becomes due. The view uses business language, remains useful
for nested work, and does not expose provider or restricted-channel details.

The status is derived from authoritative task, outreach, target, canonical-message,
generic-delivery, approval, and ownership records. It is not a second workflow state machine.

## Read model

- Add a transport-neutral `CollaborationSummary` containing task/correlation identity, current
  owner, business status, progress, expiry, `next_action`, nested children, `as_of`, and a
  `truncated` flag.
- Represent targets as an internal principal/channel reference or an external qualified identity.
  Do not expose an email-only target shape through the application/API boundary.
- Use stable business statuses:
  - outreach: `Waiting`, `ReadyToResume`, `NeedsDecision`, `Completed`, `Cancelled`, or `Failed`;
  - target: `Preparing`, `Sending`, `Waiting`, `Responded`, `NeedsDecision`, `Failed`,
    `Cancelled`, `Superseded`, or `Expired`;
  - next action: actor/queue responsible, allowed action kind, due time, and authorized deep link.
- Derive delivery-facing target states from `message_deliveries` and delivery parts, including
  outcome-unknown as `NeedsDecision`; do not leak leases, provider payloads, credentials, or raw
  errors.
- Correlate a response with the exact target/request that caused it. A nested delegate never
  becomes owner of the originating task merely because it owns a child task.

## Authorization and bounded reads

- Apply viewer authorization at every referenced channel/thread. If a user may see the parent but
  not a nested internal channel, render `Internal specialist` plus safe status instead of its name,
  messages, or handoff details.
- Return at most five nested levels and 100 nodes. Set `truncated=true` and provide a scoped detail
  link when more data exists; never silently omit it.
- V1 is read-only. Deadline, cancellation, partial-result, and reassignment mutations belong to the
  following controls plan.
- HTTP reads return one current database-derived snapshot with `as_of`. SSE events are
  identifier-only wake-ups; reconnect and lag always re-query current state.
- Instrument query duration and working-set size first. Add or change indexes only after recording
  representative table statistics and `EXPLAIN (ANALYZE, BUFFERS)` evidence.

## UI and monitoring

- Show compact waiting-on, progress, due/overdue, owner, and next-action indicators in mailbox and
  task views. Detailed timelines load separately from mailbox-list summaries.
- Emit bounded metrics/log classifications for stuck waits, unmatched replies, inconsistent source
  states, delivery failure, outcome unknown, and overdue delegation. Keep tenant, message, and
  target identifiers out of metric labels.

## Test and acceptance plan

- Cover ordinary, nested, partial-quorum, timeout, duplicate-response, failed-delivery,
  outcome-unknown, cancelled, reassigned, and late-reply histories.
- Verify status remains correct after retries and concurrent transitions and that `as_of` snapshots
  reconcile after missed or coalesced SSE events.
- Test restricted nested channels, external identities, disabled principals, cross-company IDs,
  depth/node truncation, and provider-error redaction.
- Record query plans for representative mailbox-list and detail cardinalities; enforce bounded reads
  and visible truncation before adding any supporting index.
