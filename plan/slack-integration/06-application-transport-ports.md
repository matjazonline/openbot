# Step 6 — Move Transport Contracts into the Application Layer

## Outcome

Define the protocol-neutral commands and ports where they are consumed. Remove the current
dependency inversion in which application use cases import `adapters::protocols`, and stop
projecting `NormalizedInboundMessage` back into `ParsedEmail`.

## New application contracts

Create a focused module such as `src/application/transport/` with:

- `InboundEnvelope`: source binding, qualified author identity, optional addressed identities,
  canonical content, attachment metadata, external event/message/thread keys, candidate reply
  message keys, typed ingress policy facts, correlation ID, and protocol extension.
- `IngressPolicyFacts`: an enum (`Email(EmailIngressFacts)`, `TrustedApplication`,
  `InstalledConversation`) so DMARC/spam fields never appear as meaningless defaults on Slack.
- `ProtocolExtension`: bounded/versioned email metadata or provider-neutral opaque reference to
  data already stored at ingress. Do not place raw webhook JSON in a background task payload.
- `InboundCommitRequest` and `InboundCommitOutcome`: named structs carrying all rows that must agree
  transactionally, including optional claimed-event fence, thread resolution, task materialization,
  and delivery intents.
- `DeliveryIntent`: canonical message ID, source binding, destination binding or explicit external
  destination, semantic purpose (`reply`, `mirror`, `outreach`, `notification`), and stable key.
- `DeliveryEnvelope`: versioned protocol-neutral content plus a typed adapter payload produced only
  after destination resolution.
- `ProviderSendOutcome`: `Delivered`, `RetryAfter`, `Retryable`, `OutcomeUnknown`, or `Terminal`.

Define cohesive ports with no correctness defaults:

- `InboundMessageCommitter::commit_inbound` for the single transactional operation.
- `ExternalCorrelationStore` for read-only resolution outside that commit where required.
- `DeliveryPlanner` for pure binding/policy fan-out.
- `TransportRenderer` for deterministic bounded part creation.
- `TransportSender` for one external request and typed outcome.
- `TransportRegistry`, keyed by `TransportKind`, in the application/service layer.

Persistence queue claim/transition traits are defined with the workers that consume them, not in
the SQL adapter. Use `ExecutionLease` containing row ID, execution UUID, owner, and expiry; never
pass a row ID plus worker ID as adjacent UUIDs.

## Refactor existing code

- Move `ProtocolEgressAdapter` and `EgressRegistry` out of `src/adapters/protocols/mod.rs`; adapters
  implement inner-layer traits.
- Delete `parsed_email_from_normalized` from `src/application/use_cases/thread/support.rs` after its
  callers accept `InboundEnvelope` directly.
- Break email-specific decisions out of `thread/ingest.rs`: RFC reference extraction, DMARC, spam,
  auto-reply, and quote stripping are supplied by the email adapter/policy. Canonical ACL, thread
  limits, principal resolution, task creation, and agent selection remain application behavior.
- Replace `send_email: bool` and related flag matrices in `thread/dispatch.rs` with an outcome or
  enum that makes simulated, direct, and durable delivery modes explicit.
- Remove direct construction of `OutboundEmail` from thread use cases, approval, schedules, and
  outreach. Those callers create delivery intents; only the email renderer creates `OutboundEmail`
  at the outer boundary.
- Avoid adding async forwarding layers. Extract pure synchronous decisions and `Box::pin` only at
  measured provider/agent seams, with a comment explaining stack impact.

## Durable task payload

Stop serializing `Company`, `Channel`, parsed email, and normalized protocol messages into
`background_tasks.payload`. Add a versioned payload containing stable IDs only:

```text
InboundTaskPayloadV1 {
  version, company_id, channel_id, thread_id, source_message_id, correlation_id
}
```

The worker reloads current entities with tenant-scoped queries and accepts only this canonical
version. Do not add a decoder for the pre-reset broad payload. Versioning remains so future schema
changes can be handled deliberately, and raw provider content stays out of task JSON.

## Contract tests

- Compile-time dependency check (or `rg` CI assertion) proves `src/application` imports no adapter,
  SQLx, Axum, Lettre, or Slack client types.
- Table-driven tests cover delivery fan-out: source excluded, disabled binding excluded, explicit
  destination retained, and multiple different bindings allowed.
- Serialization tests reject unknown payload versions and over-limit strings without panicking.
- A fake sender must classify ambiguous and rate-limited outcomes; it cannot return a bare `Err`
  that loses provider semantics.

## Acceptance criteria

- The application pipeline has no `ParsedEmail` parameter and no `.identity: String` extraction.
- Adding a future chat adapter requires implementing ports and registering it, not changing the
  canonical message or thread use case.
- Task payloads reference canonical IDs and remain bounded, versioned, and transport-neutral.
