# Step 10 — Add the Generic Durable Inbound Inbox

## Outcome

Provide the queue Slack and future fast-ack webhooks need before adding a Slack route. The HTTP
adapter authenticates and stores a bounded event; a supervised worker owns parsing and canonical
ingress afterward.

## Migration

Create `migrations/20260902040000_add_inbound_events.sql` with `inbound_events`:

- ID, transport, installation ID, external event key, received correlation ID, bounded raw payload
  (`BYTEA` preferred when signature fidelity matters), content type/hash, safe header facts, status,
  attempt/max-attempt counts, `available_at`, last error class/detail, execution UUID, owner/lease,
  received/processed/created/updated times.
- Statuses: `pending`, `processing`, `retryable`, `completed`, `ignored`, `dead_letter`.
- `UNIQUE (transport, external_event_key)`; for providers whose event keys are not global, include
  installation in the documented unique key. Slack `event_id` is the delivery dedup key, while
  binding-qualified timestamp remains the semantic message key.
- Composite installation/company FKs and a check that installed transports require an installation.
- Full lease-coherence and non-negative/capped attempt constraints mirroring the delivery queue.
- A maximum payload size check matching the HTTP boundary, plus retention-friendly processed-time
  index. Do not index or log the body.

## Ports and worker

- `InboundEventInbox::store_authenticated` inserts/deduplicates without parsing provider content
  beyond the authenticated routing envelope needed to identify an installation.
- `InboundEventQueue` exposes claim, renew, complete, ignore, retry, dead-letter, reap, and census.
  Correctness methods have no no-op defaults.
- `InboundEventDecoder`, registered by `TransportKind`, turns one claimed bounded payload into an
  `InboundEnvelope` or a typed ignore/terminal/retryable classification.
- `InboundEventWorker` claims globally with a bounded pool and explicit per-company/installation
  fairness, renews leases while decode/commit runs, cancels the real future on lease loss, and
  awaits cancellation before another execution proceeds.
- Completion is not a second commit: pass the live event fence into `commit_inbound` so event state,
  message/maps/task/deliveries transition atomically. Ignored non-message events can complete in a
  small fenced transaction.
- Reconcile on startup and a bounded timer; notifications only shorten idle latency.

## Retention and observability

- Retain completed/ignored raw payloads only for a documented short incident window; delete in
  bounded batches after all canonical references are durable. Retain dead letters longer with
  operator-visible counts.
- Metrics distinguish received, duplicate, auth-rejected (at HTTP), claimed, retried, lease-lost,
  ignored-by-reason, dead-lettered, processing latency, and age of oldest ready row.
- Logs contain installation/event/correlation/execution IDs and state transition, never raw body,
  tokens, signature header, message text, or profile email.

## Database and lifecycle tests

- Two simultaneous stores of one external event yield one row and a duplicate-success result.
- Two simultaneous claimants yield disjoint sets.
- Every stale-fence transition affects zero rows.
- Lease loss during canonical commit rolls back all canonical effects and cancels active work.
- Crashes before claim, after claim, during decode, before canonical commit, and after commit are
  recoverable without duplicate canonical messages.
- A poison batch is not immediately reclaimed and eventually becomes observable dead letter.
- Shutdown between iterations and during active work leaves no orphaned future.

## Acceptance criteria

- A future webhook adapter can authenticate, store, acknowledge, and return without executing
  business ingress inline.
- No process crash after successful storage can permanently lose an acknowledged event.
- Queue ownership and canonical ingestion share one execution fence and one final transaction.

