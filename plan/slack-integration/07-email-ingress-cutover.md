# Step 7 — Refactor Email Ingress and Commit It Atomically

## Outcome

Make SMTP, SendGrid, simulation, and trusted inter-channel email-shaped traffic enter the same
canonical ingress use case without turning the canonical command back into `ParsedEmail`. Preserve
SMTP's synchronous rejection behavior while making every accepted message/task/mapping/fan-out one
transaction.

## Email boundary

- Move MIME/header parsing from `src/application/services/email_parser.rs` to
  `src/adapters/protocols/email/parser.rs`; the application layer no longer owns or imports an
  email-format parser.
- `EmailIngressAdapter` validates and produces `InboundEnvelope` plus `EmailIngressFacts`:
  qualified sender/recipient identities, `ChannelSelector`s, RFC Message-ID, In-Reply-To,
  References, Thread-Index, auth verdicts, spam score, auto-reply/forward markers, raw body
  extensions, and bounded attachment metadata.
- Preserve `AuthVerdict` as the trusted SMTP/SendGrid boundary result. Do not accept authentication
  verdicts from message headers and do not give non-email transports fake `Pass` values.
- Parse `.quiet`, `+noagent`, hop count, trace, and correlation headers into typed options once.
- Reject message/header/recipient/attachment limits before persisting more than the documented
  bound. Split preflight from attachment persistence: guards, routing, ACLs, and all bounds run
  before any attachment upload; only an accepted commit plan uploads bytes. Align the relevant
  SMTP and SendGrid limits.

## Canonical application flow

Refactor `src/application/use_cases/thread/ingest.rs` into visible phases:

1. Pure ingress guard: auto-reply, hop, loop, and typed email-auth rejection.
2. Address resolution: selectors to tenant-scoped channels/bindings.
3. Principal/ACL decision: propagate directory errors; never collapse them into “unauthorized.”
4. Thread resolution: exact `external_threads`, then ordered candidate `external_messages`, then
   Thread-Index policy, then create.
5. Pure participant/third-party policy and thread-limit checks.
6. Build one `InboundCommitRequest` per logical accepted message with all channel associations,
   task data, mappings, and delivery directives.

Use named phase outcome structs. Keep I/O phases async without introducing forwarding-only async
functions; make policy phases synchronous and unit-testable.

## Purpose-specific transaction

Implement `InboundMessageCommitter` in the Postgres adapter. In one transaction it must:

- lock or create qualified identities/principals;
- resolve/upsert external thread mappings before message materialization;
- insert/deduplicate the canonical message and email extension, verifying content hash on conflict;
- associate the message with all resolved threads and update their participants/timestamps;
- insert each binding-qualified external message mapping;
- create or reuse `background_tasks` by canonical source message ID;
- create the immediate delivery rows available at this step boundary. An email-only deployment may
  legitimately produce an empty fan-out here; step 9 adds the durable generic delivery tables and
  step 10 adds claimed inbound events. This step does not claim either later capability exists;
- mark a claimed inbound event complete when one exists after step 10 (email direct ingress has no
  claim at this step boundary); and
- rely on transaction-bound database notification only after these facts all agree.

An authorization or validation rejection writes none of these rows. A duplicate returns the
original canonical/task IDs and does not enqueue new deliveries.

## Entry-point changes

- `src/adapters/smtp/server.rs`: retain hard synchronous 5xx rejections and call canonical ingress
  before returning 250. A transient database failure is a 4xx, not an accepted/lost message.
- `src/adapters/http/routes/webhooks/sendgrid.rs`: authenticate the raw request before parsing and
  call the same adapter/use case. Keep an explicit body limit and provider request correlation.
  Bounce work must be owned by a supervised request/task handle; once step 9 exists it is queued
  through the generic delivery worker rather than detached with `tokio::spawn`.
- `src/adapters/http/routes/channel.rs` simulation/mailbox: use `TrustedApplication` policy facts
  and the signed-in principal; never fabricate DMARC pass.
- Inter-channel delivery: address a canonical channel/binding and preserve the same correlation
  chain. Do not serialize an `OutboundEmail` merely to re-ingest it.

## Tests

- Two concurrent deliveries of the same RFC Message-ID produce one canonical message, mappings,
  task, and fan-out set.
- A repeated Message-ID with changed canonical content is rejected as a collision.
- Inject failures before and after each statement group and assert the whole logical commit rolls
  back.
- A committed message is never observable without its source mapping and task/delivery rows.
- Multi-channel email routing associates one canonical message with each permitted thread without
  duplicating its payload.
- SMTP hard/soft responses remain correct for unknown address, ACL denial, auth failure, database
  outage, duplicate, and accepted message.
- Existing hop-limit, third-party, outreach-reply, context-only, and quote-stripping cases pass
  through the new contract.
- Every auth, ACL, unknown-recipient, spam, hop, loop, attachment-limit, and validation rejection
  leaves principal, identity, thread, message, mapping, task, delivery, and attachment-object counts
  unchanged.

## Acceptance criteria

- No accepted email requires a post-commit mapping or task insert.
- No canonical function accepts or returns `ParsedEmail`.
- Email-only syntax and authentication remain in the adapter while business policy remains in the
  domain/application layers.

The earlier unexplained stock-stack exit 101 did not reproduce in the isolated 918-test rerun. The
2 MiB `STACK_BUDGET_KIB` threshold remains unchanged and continues to be the early failure signal.
