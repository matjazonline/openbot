# Actionable Collaboration Notifications

## Expected result

People receive a quiet, permission-safe alert when responsibility or a meaningful business state
changes. Notifications point to authoritative operational work; they do not own assignment,
deadline, or resolution state. Reading an alert never marks the underlying action complete.

V1 supports in-app notifications and a narrow set of immediate emails. Digests, chat/webhook
delivery, business-hours scheduling, and escalation chains are deferred until timezone and service
policy exist.

## Event and record model

- Define notification events from durable domain transitions, not worker heartbeats, retries, SSE,
  or low-level writes. Persist an identifier-only event in the same transaction as the source
  transition, then project recipient records idempotently.
- Give each actionable notification an identity based on company, recipient, source kind/id, action
  kind, and source generation/version. A newer generation resolves or withdraws the obsolete one
  instead of creating competing actions.
- Notification records store recipient principal, event/action kind, source identifiers,
  active/resolved/withdrawn state, independent `read_at`, safe presentation metadata, and
  timestamps. Content and authorization are re-resolved when opened.
- Extend the minimal `user_notification_preferences` introduced by task ownership rather than
  creating a competing table or preference namespace.
- Reuse `message_deliveries` and delivery parts for email attempts, retry, and provider outcomes.
  Notification records must not implement a second delivery-attempt queue.

## V1 routing policy

- Always create in-app records for actionable responsibility changes the recipient is authorized to
  see. Ordinary informational transitions stay in source timelines and do not create notifications.
- Send immediate email only for assignment/transfer to the user, response-review assignment,
  delegation timeout requiring a decision, and permanent task or delivery failure assigned to the
  user. Self-assignment and self-triggered actions do not email.
- Respect per-user email preferences for those event families. In-app actionable records remain
  available so opting out of email cannot hide owned work.
- Multiple delegated replies update the source progress and at most one active notification; they
  do not send one email per reply.
- Unassigned channel-team work appears in the shared work queue but sends no personal notification
  until a principal is assigned. V1 does not fan one unassigned item out to every teammate.
- Quiet notes, FileOnly ingestion, ordinary context additions, worker retries, routine internal
  agent traffic, and successful background completion do not notify by default.

## Security and lifecycle

- Resolve recipients from current task ownership, explicit reviewer/handoff responsibility, or the
  authorized channel team queue. Company role alone does not grant access to a restricted channel.
- Email contains only a safe event label, company/channel label when authorized, and a secure deep
  link. It never includes note text, handoff instructions, draft content, evidence excerpts,
  customer bodies, or provider errors.
- Re-check authorization on every in-app read and deep link. Removing a user, revoking channel
  access, transferring ownership, replacing a handoff generation, or resolving the source
  withdraws obsolete active records.
- `read_at` is presentation state; active/resolved/withdrawn is source-derived business state.

## Monitoring and test plan

- Track event projection lag, active-notification age, enqueue failure, delivery result, withdrawal,
  and notification-to-source-action time using bounded labels.
- Test transactional event creation, projector retry, duplicate events, ownership/reviewer changes,
  disabled users, preference changes, self-actions, withdrawal, and cross-company isolation.
- Prove each source generation creates at most one active notification per responsible user and at
  most one logical email delivery for an enabled immediate-email event.
- Verify quiet/internal content never creates notification noise or leaks through stored metadata,
  email, list rendering, or deep-link error messages.
- Test reconnect and SSE lag as reconciliation paths, and verify notification read/dismiss cannot
  resolve an operational work item.

## Dependencies and sequencing

Implement after ownership, audience/drafts, notes/manual handoff, delegation status and controls,
review, and the operational work queue have stable transitions. Event taxonomy can be designed
earlier, but delivery must consume those authoritative sources rather than pre-empt them.
