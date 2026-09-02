# Step 1 — Record the Architecture Contract

## Outcome

Add `docs/transport_architecture.md` as the decision record that every later schema, port, and
adapter must satisfy. This is a coding prerequisite: it settles identity, authorization,
threading, routing, and delivery semantics before names harden into migrations.

## Decisions to record

### Business channels and protocol interfaces

- `Channel` remains the business object that owns agents, policy, memory configuration, threads,
  and access rules.
- A `ChannelBinding` is one addressable interface on a channel. One channel may have its email
  binding, several Slack bindings, and future chat-provider bindings simultaneously.
- A binding never owns a second copy of channel policy or message history. Disabling a binding
  stops ingress and egress through that interface without deleting the channel or its threads.
- `ChannelType` is renamed to `TransportKind`; “channel” is reserved for the business entity.

### Principals and identities

- A `Principal` is the stable actor inside one company: app user, agent, system actor, or external
  person.
- A `QualifiedIdentity` is how one transport names that principal. Its uniqueness scope is
  `(transport, namespace, subject)`: email uses a deployment/email namespace plus a normalized
  address; Slack uses an installation ID plus immutable Slack user ID.
- An identity can be observed before it is linked. Observation creates or reuses a company-scoped
  external principal; it does not confer team membership or channel access.
- Slack profile email is optional enrichment with `slack_profile_claim` provenance. Never merge it
  automatically with an email principal. An authenticated app user or company manager must make
  the link explicitly, and the audit row must identify the actor.

### Access model

- Existing app UI access continues to require company membership plus the channel's principal ACL.
- A provider conversation binding is a separate disclosure boundary. Slack v1 chooses the review's
  explicit-grant model: linking a private conversation grants all its current and future members
  read access through Slack. The confirmation screen states this and records the manager, member
  count, conversation identity, and time.
- V1 rejects public, Slack Connect/shared, archived, non-threadable, DM, and MPIM conversations.
  This bounds the grant; support for any rejected kind requires a later threat-model decision.
- Receiving an event from an installed workspace is not authorization by itself. The event must
  name an active binding and an eligible human sender in that bound conversation.

### Canonical messages and correlation

- `Message` contains protocol-neutral author, body, subject, attachments, role, direction,
  correlation ID, and timestamps. It never requires an email address or RFC Message-ID.
- Protocol headers and raw representations live in protocol extension tables. Provider keys live
  in `external_threads` and `external_messages`, qualified by binding.
- `external_thread_key` and `external_message_key` are opaque provider keys. For Slack they are
  `thread_ts.unwrap_or(ts)` and `ts`. For email, adapters expose RFC/Thread-Index keys and reference
  candidates; the application does not parse email syntax.
- Reply-before-root is valid. Resolving/upserting the external thread binding precedes canonical
  message creation so a Slack reply may create the internal thread that its later root joins.

### Delivery semantics

- Ingress explicitly computes delivery intents in the same transaction as the message. A database
  notification never decides whether to mirror a message.
- Fan-out targets every eligible active binding except the source binding, plus direct destinations
  required by the command. This prevents echoes while still allowing one Slack binding to mirror
  to another when policy explicitly permits it.
- One logical delivery has one or more parts. Every part has a stable ID and index; Slack chunk
  timestamps are stored per part, never in a single message-map field.
- A stable application idempotency key deduplicates queue creation. It does not turn a provider API
  without idempotency support into exactly-once delivery.
- Ambiguous Slack HTTP outcomes become `outcome_unknown`; an echo or bounded history lookup using
  non-secret message metadata may reconcile them. Automatic blind retry is forbidden.

### Slack interaction behavior

- The Slack bot represents the bound business channel, not each agent. Custom bot names/icons do
  not make separately mentionable Slack agents.
- V1 routes eligible human messages in a bound conversation to the channel's position-0 active
  agent. Bot/app messages, edits, deletes, join notices, and other subtypes are ignored or audited
  according to a closed allowlist.
- Cross-channel agent delegation uses a transport-neutral `ChannelSelector` resolved to a canonical
  channel ID. Tools no longer manufacture an email address to call another channel.

## Code/document map

- Create `docs/transport_architecture.md` with the decisions, state diagrams, table ownership, and
  trust boundaries above.
- Update `docs/inter_channel_agent_communication.md` and `docs/system_addresses.md` terminology:
  email addresses are one adapter syntax for a channel selector, not the identity of a channel.
- Add a compact glossary to `README.md` and link the decision record.
- Add a “deferred” section: Slack files, edits/deletes/reactions, public/shared channels, native
  Slack agent sessions, identity auto-linking, and provider-specific backfills are not v1.

## Review checklist

- Trace four sequences in the ADR: email inbound/reply, Slack inbound/reply, email-to-Slack mirror,
  and Slack-to-email mirror.
- For each sequence identify the authentication boundary, principal, binding, external thread key,
  canonical message, delivery rows, and failure recovery point.
- Include a tenancy diagram showing `company_id` on every relationship and the required composite
  foreign keys.
- Include the inbound-event and delivery state machines used in steps 9, 10, 14, and 17.

## Acceptance criteria

- No unresolved identity or access decision is deferred to a migration author.
- The ADR states the unavoidable at-least-once/ambiguous provider boundary without claiming exact
  once.
- The private-conversation access grant and unsupported Slack conversation kinds are explicit.
- Reviewers can reject any later change by pointing to one violated invariant in this document.

