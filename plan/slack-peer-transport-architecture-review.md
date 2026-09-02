# Slack peer transport architecture review

## Verdict

The proposed plan is a strong discovery draft and a reasonable minimum-change prototype, but it
is not the best production architecture for a true peer transport, especially when backward
compatibility is unnecessary.

Do not implement it as written. Its central choice—keeping email as the identity and message
spine—is precisely the compromise that a clean break allows the project to avoid.

## Blocking architectural problems

### 1. Slack ingress is not durable before acknowledgement

The plan says “ack in <3s, then hand off” but defines no durable inbound-event queue. Slack
explicitly recommends acknowledging within three seconds and processing through a queue afterward.
See the [Slack Events API documentation](https://docs.slack.dev/apis/events-api/).

A process crash after the 2xx but before ingestion permanently loses the message. Add a
`slack_inbound_events` inbox keyed by globally unique `event_id`, with status, attempts, backoff,
execution fence, correlation ID, and a bounded payload. Persist it before returning 2xx.

`event_id` must be the delivery deduplication mechanism, not merely a “cheap pre-filter.” Slack
`(binding_id, ts)` is a separate semantic-message uniqueness key.

### 2. Loop suppression cannot be atomic using the existing ingest path

The inbound `thread_message` currently commits inside `create_message` in
[`src/adapters/persistence/thread.rs`](../src/adapters/persistence/thread.rs). Only afterward could
Slack code insert `slack_message_map`. The commit emits `MessageCommitted`, so the mirror can
observe and repost the Slack-originated message before its mapping exists.

The inbound event, canonical message, external-message mapping, and background task need one
purpose-specific transactional persistence operation, as required by
[`src/application/AGENTS.md`](../src/application/AGENTS.md).

### 3. The claimed authorization model leaks channel data

The plan says Slack workspace membership grants nothing, but then mirrors all email traffic into a
Slack conversation. Every member of that conversation can read it, including people who fail
`Channel::viewer_access` in
[`src/domain/entities/channel.rs`](../src/domain/entities/channel.rs).

Choose one security model explicitly:

- Linking a Slack conversation is an audited channel-level read grant to all its members; or
- Only private conversations may be linked, and membership is continuously reconciled against
  channel viewers, disabling delivery on drift.

The current plan claims neither while effectively implementing the first.

### 4. Email-shaped Slack messages are transport leakage, not peer-protocol architecture

`NormalizedInboundMessage` appears protocol-neutral, but it is immediately projected into
`ParsedEmail`, discarding identity protocol semantics in
[`src/application/use_cases/thread/support.rs`](../src/application/use_cases/thread/support.rs).
Persisted messages still require RFC Message-IDs and email senders and recipients in
[`src/domain/entities/message.rs`](../src/domain/entities/message.rs).

Synthetic `.invalid` senders and fabricated RFC Message-IDs make Slack work by lying to the domain
model. This also loses immutable Slack attribution: the canonical record identifies a mutable email
address, not `(installation, Slack user ID)`.

With no compatibility requirement, introduce:

- A canonical participant/principal ID.
- Protocol-specific identities such as `Email(EmailAddress)` and
  `Slack { installation_id, user_id }`.
- ACLs and thread participants keyed by principals or qualified identities.
- Protocol-neutral messages.
- Email- and Slack-specific metadata in separate transport tables.

Email can help link a Slack identity to an existing person, but it should not become the Slack
identity itself.

### 5. The outbound mirror is not crash-safe

`slack_message_map` inserts a claim, calls Slack, and then records `ts`. A crash after Slack accepts
the post but before confirmation leaves a permanent `NULL ts`. Retrying may duplicate; refusing to
retry loses progress. A cursor does not solve this uncertainty.

The proposed lease also lacks the execution-generation fence required by
[`src/adapters/persistence/AGENTS.md`](../src/adapters/persistence/AGENTS.md), and the row has no
retry state, backoff, terminal state, or error classification.

Use a real `message_deliveries` outbox with:

- `pending / sending / delivered / outcome_unknown / retryable / dead_letter`
- `attempt_count`, `available_at`, and `last_error`
- `execution_id`, owner, expiry, and fenced transitions
- Slack `Retry-After` handling
- Channel and workspace rate limiting
- An explicit policy for ambiguous HTTP outcomes

Slack’s current official SDK does not expose a `client_msg_id` argument for `chat.postMessage`, so
exactly-once delivery cannot simply be assumed. See the
[Slack SDK request type](https://github.com/slackapi/node-slack-sdk/blob/main/packages/web-api/src/types/request/chat.ts).

One `slack_ts` per internal message also contradicts 12,000-character chunking. Delivery parts need
`(delivery_id, part_index, slack_ts)`.

### 6. Threading is not correct under delay or reordering

`In-Reply-To` works only if the Slack root has already been ingested. Events can be delayed or
processed out of order; a reply arriving first will create a separate thread. The unused
`threads.external_thread_key` does not fix that, and a single field cannot represent one thread
bound simultaneously to email and Slack.

Use:

- `external_threads(binding_id, external_thread_key, thread_id)`
- `external_messages(binding_id, external_message_key, message_id)`

For Slack, the thread key is `thread_ts.unwrap_or(ts)`. Upsert the external-thread binding before
materializing a message, and make reply-before-root a tested case.

### 7. OAuth and Slack permissions are incomplete

The callback needs a signed or stored, single-use OAuth `state` binding the initiating user,
company, expiry, and nonce. Slack explicitly requires checking it to prevent forged authorization
callbacks. See the [Slack OAuth guide](https://docs.slack.dev/authentication/installing-with-oauth/).

The listed scopes omit those needed for the planned inbound events:

- Public-channel messages require `channels:history`.
- Private-channel messages require `groups:history`.

See the [public history scope](https://docs.slack.dev/reference/scopes/channels.history/) and
[private message event](https://docs.slack.dev/reference/events/message.groups).

Conversely, `files:read` and DM scopes should not be requested before those phases ship. The link
flow must also verify that the bot can receive events and post in the selected conversation.

`users.info` exposes an email field, but no `email_verified` proof matching the proposed column.
Treat it as a Slack-profile-derived link with recorded provenance, not as a cryptographically
verified identity. See [`users.info`](https://docs.slack.dev/reference/methods/users.info).

### 8. The migration plan directly contradicts repository rules

The plan says to edit `20260817000000_init_schema.sql` in place. Repository guidance explicitly
declares that baseline immutable and requires a new timestamped migration in
[`src/adapters/persistence/AGENTS.md`](../src/adapters/persistence/AGENTS.md).

“No backward compatibility” allows a clean domain cutover; it does not make applied migration
checksums mutable.

## Recommended architecture

```text
Slack HTTP request
    -> authenticated durable inbound_event
    -> ACK
    -> fenced ingress worker
    -> canonical thread + message + external mapping + agent task (one transaction)

Canonical message
    -> durable delivery rows selected by reply/delivery policy
    -> email worker / Slack worker
    -> protocol-specific external message mappings
```

The core model should contain:

- `integration_installations`
- `channel_bindings` for Slack conversations and future transports
- `participants` plus protocol-qualified `participant_identities`
- `external_threads` and `external_messages`
- `inbound_events`
- `message_deliveries` and `message_delivery_parts`

Slack recipient syntax should use a transport-neutral `ChannelSelector` parser shared with the
email address parser. Do not call an email-address parser from Slack. Customized bot names and
icons do not create independently mentionable Slack agents; `@nova` would be plain text, not native
Slack presence. Use a documented command/app-mention grammar or evaluate Slack’s
[native agent-session surface](https://docs.slack.dev/ai/agent-sessions/).

## Good parts worth retaining

- Linking a Slack conversation to the existing business channel.
- Raw-body HMAC verification, timestamp replay window, and constant-time comparison.
- LISTEN/NOTIFY as a wake-up only, with durable reconciliation.
- Narrow secret reads and encrypted token storage.
- Application-owned ports, validated newtypes, pure formatting functions, supervised workers, and
  bounded HTTP clients.
- Phased delivery and local-stub end-to-end tests.

The newtypes should have validating constructors and private fields; the generic
`string_newtype!` accepts arbitrary untrusted strings.

## Required verification additions

Before implementation is approved, the plan should include database-backed tests for:

- Two concurrent inbox/outbox claimants.
- Lease loss during an active Slack request.
- Crash before and after every transaction/API boundary.
- Ambiguous provider outcomes and poison batches.
- Reply arriving before its Slack root.
- Cross-tenant installation/link/map attempts.
- Unauthorized Slack conversation members reading mirrored traffic.
- Partial multi-part delivery.
- `429` plus `Retry-After` and workspace-wide limits.
- Shutdown during active processing with no orphaned work.

Use a new migration and run SQLx preparation, offline compilation, migrations, the database-backed
suite, formatting/clippy, and the repository’s stack-budget check.
