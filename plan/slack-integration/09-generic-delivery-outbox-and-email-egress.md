# Step 9 — Replace the Email Outbox with Generic Deliveries

## Outcome

Create one crash-aware delivery state machine for email, Slack, and future transports, and implement
email on it before writing Slack egress. A delivery is a durable attempt to expose one canonical
message through one destination binding; parts record provider-side results.

## Migration

Define the delivery tables directly in the rewritten clean-reset migration set.

### `message_deliveries`

Include `id`, company/channel/message/destination-binding IDs, optional task ID, optional
`depends_on_delivery_id`, correlation ID, transport, purpose, stable idempotency key, status,
attempt/max-attempt counts, `available_at`, last error class/detail, execution UUID, owner, lock
times, delivered/created/updated times.

- Statuses: `pending`, `sending`, `retryable`, `delivered`, `outcome_unknown`, `dead_letter`.
- Only `sending` rows hold a complete live lease. Every other status has no lease fields.
- `UNIQUE (destination_binding_id, idempotency_key)` deduplicates logical delivery creation.
- Claim SQL excludes a row until its dependency is delivered; a terminal dependency moves its
  descendants to dead letter with a typed causal reason rather than leaving them pending forever.
- Composite FKs prove message, channel, binding, and optional task belong to the company.
- Purpose is checked (`reply`, `mirror`, `outreach`, `notification`); transport is carried from the
  binding, never reasserted from a string literal in later queries.

### `message_delivery_parts`

Include company/delivery ID, zero-based `part_index`, stable `part_key`, versioned bounded payload,
status, provider message key, content digest, attempt metadata, and timestamps.

- `UNIQUE (delivery_id, part_index)` and `UNIQUE (delivery_id, part_key)`.
- Part state distinguishes prepared, sending, delivered, outcome unknown, retryable, and dead.
- Parts do not own independent leases. The parent delivery owns one execution lease, and every part
  transition joins/updates through that live execution UUID. This avoids two competing ownership
  state machines for one provider call.
- A parent is delivered only when every part is delivered; any unknown part keeps the parent
  `outcome_unknown`; terminal poison makes the parent `dead_letter`.
- Payload JSON is transport-rendered but versioned, bounded, and decoded fallibly. Secrets and raw
  authorization headers are forbidden.

## Application/persistence ports

- Define `DeliveryQueue` next to the delivery worker: atomic bounded claim using one
  `UPDATE ... FROM (SELECT ... FOR UPDATE SKIP LOCKED)` ordered by availability/ID.
- Claims mint a new execution UUID. Renew, begin part, complete part, retry, unknown, dead-letter,
  and parent completion all require that fence and a live lease.
- Reaping an expired lease consumes an attempt, applies bounded exponential backoff with jitter,
  and eventually dead-letters. Poison payloads go terminal immediately.
- Add workspace/binding scheduling hooks without embedding Slack logic in SQL. Global claim remains
  global; application scheduling enforces fair bounded global/per-company/per-installation capacity.
- Keep `LISTEN/NOTIFY` as a wake-up and periodic reconciliation as correctness.

## Email implementation

- Implement `EmailRenderer`: canonical message + email destination/context -> exactly one
  `OutboundEmail` part with a stable RFC Message-ID derived from `part_key`.
- Implement `EmailSender` around `OutboundDispatcher`/`MailTransport` returning typed provider
  outcomes. Preserve TLS/timeouts and never build a new SMTP transport per message.
- Change agent replies, schedules, approvals, confirmations, bounces, stop notices, and outreach to
  enqueue generic deliveries in the transaction that creates their durable source state.
- Split account-confirmation mail into a generic notification delivery only if its current flow can
  preserve the same atomic registration semantics; otherwise document it as a bounded separate
  queue client and do not pretend it is migrated.
- Rename UI/domain outbox concepts from email to delivery and render transport/purpose/status.

## Concurrency/failure tests

- Two claimants never own the same delivery or part.
- A stale execution cannot renew, complete, retry, or overwrite the replacement execution.
- Crash before send reclaims with an attempt; crash after a definite provider success but before
  commit becomes `outcome_unknown`, never an automatic resend.
- One poison row does not hot-spin or monopolize the next full batch.
- Partial multi-part state aggregates correctly even though email currently has one part.
- Atomic agent reply + canonical task payload + delivery tests pass with `message_deliveries`.
- Shutdown during send cancels/awaits work and leaves either a short recoverable lease or an
  explicit unknown result.

## Acceptance criteria

- All email delivery is selected and leased through the generic tables/ports.
- Every state transition is constrained in SQL and fenced in its `WHERE` clause.
- No provider-specific worker writes directly to canonical message/task state after sending.
