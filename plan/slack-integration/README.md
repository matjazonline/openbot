# Transport-Neutral Messaging and Slack Integration

This directory turns `plan/slack-peer-transport-architecture-review.md` into an implementation
sequence. Each numbered file is one independently reviewable step. Complete the files in order;
do not begin the Slack phase until the abstraction gate in step 11 passes.

## Target architecture

```text
authenticated provider request
  -> durable inbound_events row
  -> fenced ingress worker
  -> one transaction:
       principal/identity resolution
       + external thread resolution
       + canonical message/thread association
       + external message mapping
       + background task
       + delivery fan-out

canonical message
  -> message_deliveries + message_delivery_parts
  -> transport worker (email, Slack, future provider)
  -> external message/thread mappings
```

A business `Channel` is not an email inbox or a Slack conversation. It owns agents, policy,
threads, memory settings, and zero or more `ChannelBinding`s. A binding is one protocol-facing
interface. A canonical message is stored once and can be associated with more than one thread or
delivered through more than one binding without fabricating an email address or RFC Message-ID.

## Current seams this plan replaces

- `src/domain/entities/message.rs::Message` requires RFC message/reference IDs, email sender, and
  To/Cc arrays for every message.
- `src/domain/entities/thread.rs::Thread` and `Channel` authorization are keyed by email addresses.
- `NormalizedInboundMessage` looks generic but
  `thread/support.rs::parsed_email_from_normalized` immediately turns it back into `ParsedEmail`.
- `ProtocolEgressAdapter` lives under `src/adapters` even though the application consumes it, and
  `EmailEgressAdapter` reconstructs placeholder channel/company names.
- `src/adapters/persistence/thread.rs::create_message` commits the message before a future Slack
  mapping could be inserted, while the database trigger publishes the committed message.
- `threads.external_thread_key` can represent only one external transport, and
  `email_messages`/`email_outbox` are the canonical message and delivery stores.
- `background_tasks.payload` serializes broad email-shaped/domain entities, making queued work
  sensitive to model changes and keeping transport data inside the task protocol.

The numbered steps remove these seams rather than placing Slack branches beside them.

## Sequence

### Phase A — replace the email-shaped spine

1. [Record the architecture contract](01-architecture-contract.md).
2. [Introduce validated transport vocabulary](02-transport-domain-types.md).
3. [Add principals, qualified identities, and principal ACLs](03-principals-identities-and-acls.md).
4. [Add installations and channel bindings](04-installations-and-channel-bindings.md).
5. [Create canonical messages and external correlation maps](05-canonical-messages-and-external-maps.md).
6. [Move ingress and egress ports into the application layer](06-application-transport-ports.md).
7. [Refactor email ingress and commit it atomically](07-email-ingress-cutover.md).
8. [Migrate readers and non-ingress message producers](08-canonical-readers-and-producers.md).
9. [Replace the email outbox with generic deliveries](09-generic-delivery-outbox-and-email-egress.md).
10. [Add the generic durable inbound inbox](10-generic-inbound-inbox.md).
11. [Remove the legacy email spine and enforce the abstraction gate](11-email-spine-contract-and-gate.md).

### Phase B — add Slack as a peer transport

12. [Add Slack configuration, client, installation persistence, and OAuth](12-slack-installation-and-oauth.md).
13. [Link private Slack conversations with an explicit access grant](13-slack-channel-binding-and-access.md).
14. [Verify and durably acknowledge Slack Events API requests](14-slack-events-http-ingress.md).
15. [Normalize and atomically ingest Slack messages](15-slack-ingress-worker.md).
16. [Plan and format Slack delivery parts](16-slack-delivery-planning.md).
17. [Deliver Slack parts with fenced recovery and rate limiting](17-slack-delivery-worker.md).
18. [Finish lifecycle, observability, failure testing, and rollout](18-operations-verification-and-rollout.md).

## Non-negotiable invariants

- The domain and application layers never import Axum, SQLx, Lettre, or Slack HTTP types.
- Protocol identifiers are qualified by their namespace. A Slack user ID without an installation
  ID, or a message timestamp without a binding ID, is not an identity.
- A Slack profile email is a claim with provenance, not proof of identity and never an automatic
  authorization grant.
- A provider event is acknowledged only after its bounded raw payload is durable.
- An inbound event, canonical message, external maps, task, and immediate delivery fan-out become
  visible together or not at all.
- Every claim, renewal, completion, failure, and recovery transition is fenced by a fresh execution
  UUID and covered by a competing-claimants database test.
- `LISTEN/NOTIFY` is a wake-up. Workers always reconcile durable rows after startup, lag, and wake.
- Slack v1 links only private, non-shared, threadable conversations. Linking is an explicit,
  audited read grant to every current or future member of that conversation.
- Provider delivery is not described as exactly once. Definitive failures, ambiguous outcomes,
  rate limits, and poison payloads have distinct durable states.
- The immutable baseline migration is never edited. Every schema change uses a new timestamped
  migration and refreshes `.sqlx` metadata.

## Planned module ownership

The exact split may be adjusted to respect file-size thresholds, but dependency ownership may not:

```text
src/domain/entities/{participant,transport,message,thread}.rs
src/application/transport/{mod,ingress,delivery,ports}.rs
src/application/use_cases/integration.rs
src/application/services/{inbound_event_worker,delivery_worker}.rs
src/adapters/persistence/{participant,integration,inbound_event,delivery}/
src/adapters/protocols/email/{ingress,egress}.rs
src/adapters/protocols/slack/{client,ingress,egress}.rs
src/adapters/http/routes/{slack_oauth,ui_integrations}.rs
src/adapters/http/routes/webhooks/slack.rs
src/adapters/http/pages/integrations.rs
```

Protocol adapters parse/render/call providers; application ports and workers coordinate; SQLx
adapters persist; the domain owns only transport-neutral policy and validated concepts.

## Standard verification for every SQL-bearing step

Run the narrow unit/database tests named by that step, then run:

```sh
cargo fmt --check
git diff --check
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_test" cargo sqlx migrate run
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents" cargo sqlx prepare --check -- --all-targets
SQLX_OFFLINE=true cargo check --all-targets
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_test" cargo test --locked --all-targets
cargo clippy --all-targets -- -D warnings
DATABASE_URL="postgres://$(whoami)@localhost:5432/mail_agents_test" scripts/stack-budget.sh
```

When a step changes SQL, run `cargo sqlx prepare -- --all-targets` first and commit the resulting
`.sqlx/` files. The stack-budget threshold is not raised to make this project pass; shrink new
async chains or document a genuine platform calibration.

## Slack references used by the Slack phase

- [Events API acknowledgement and retry behavior](https://docs.slack.dev/apis/events-api/)
- [Request signature verification](https://docs.slack.dev/authentication/verifying-requests-from-slack/)
- [OAuth installation flow](https://docs.slack.dev/authentication/installing-with-oauth/)
- [`chat.postMessage` limits and rate behavior](https://docs.slack.dev/reference/methods/chat.postmessage)
- [Web API rate limits](https://docs.slack.dev/apis/web-api/rate-limits)
- [Message metadata and history reconciliation](https://docs.slack.dev/messaging/message-metadata/)
- [Message event variants](https://docs.slack.dev/reference/events/message/)
- [Private-channel history scope](https://docs.slack.dev/reference/scopes/groups.history/)
