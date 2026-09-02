# Step 8 — Migrate Canonical Readers and Non-Ingress Producers

## Outcome

Move every message reader and every schedule/approval/agent/system writer to the canonical model.
At the end of this step, only the transitional email egress path may still read legacy email
fields.

## Persistence/read-model work

- Rewrite the selects in `src/adapters/persistence/thread/` to join `messages`, principals,
  identities, and optional protocol extensions. Do not reconstruct one giant entity containing
  fields that do not apply to all transports.
- Introduce purpose-specific projections:
  - `ThreadMessageView` for mailbox/task/simulation pages;
  - `AgentHistoryMessage` for prompt history;
  - `EmailReplyContext` for the email renderer;
  - `MessageAuditView` for operational detail.
- Bound every history projection and select only fields it consumes. Preserve cursor ordering by
  `(created_at, association_id)` and the existing newest-200 behavior where product policy requires
  it.
- Replace `find_thread_by_message_ids` with binding-qualified external correlation methods. A raw
  RFC Message-ID is never a company-wide generic thread key.
- Replace `find_outbound_reply(thread_id, in_reply_to)` with a canonical source-message/reply
  relation or task/delivery idempotency key; outreach-specific exclusions must not depend on an SMTP
  provider ID.

## Producer changes

- `src/application/use_cases/thread/dispatch.rs`: create one canonical agent reply with an agent
  principal/identity and associate it with every answered thread; produce delivery intents instead
  of email-shaped message copies.
- `src/application/services/task_worker.rs` scheduled replies: author with the agent principal and
  refer to the canonical triggering message. No synthetic `<scheduled-...>` ID is required by the
  message entity; the email renderer may derive a stable RFC ID from the delivery key.
- `src/application/use_cases/schedule.rs`, `approval.rs`, and system-note paths: use explicit system
  or agent principals and canonical source relationships.
- `src/application/services/outreach_tool.rs`: split `ExternalDestination::Email` from
  `ChannelSelector`; queue canonical outreach intents and keep the target allowlist decision on
  pre-model trusted identities.
- `src/application/services/agent_channel_tool.rs` and directory tool output: return canonical
  channel selector/ID plus available interfaces. An email address can remain display data, not the
  internal routing key.
- `src/application/services/memory_coordinator.rs` and agent prompt construction: scope by
  principal ID. Render a safe display identity as data inside prompt fences; do not use provider
  identity strings as authorization keys.

## HTTP/UI changes

Update mailbox, task, simulation, dashboard, attachment, and outbox pages/routes to use the new
read projections. Expected visible changes:

- Sender labels prefer principal display name and show a small transport badge from the authored
  identity.
- Recipient rows are optional because a conversation message may target a binding, not To/Cc.
- Reply/thread links use canonical IDs; provider IDs appear only in an authorized diagnostic pane.
- Attachment authorization scopes the canonical message through company/channel/thread before
  returning bytes.

Touch the current user-edited files (`agent_settings.rs`, `channel_settings.rs`, their routes, and
page tests) only after rebasing around their existing changes; do not overwrite unrelated work.

## Durable payload transition

- Teach `TaskWorker` to decode both the current payload and `InboundTaskPayloadV2` fallibly.
- New writers produce v2 only. Add an operational count/query for remaining v1 queued/parked tasks.
- V2 processing reloads message, thread, channel, company, principal, and agent with tenant-scoped
  queries and returns a classified task failure for missing/incoherent state.

## Tests

- Snapshot/HTML tests render email, Slack-shaped (no To/Cc), agent, schedule, and system messages.
- Authorization tests attempt a valid foreign-company message/attachment/thread ID.
- Agent history retains role, body ordering, and bounded size without needing email metadata.
- Schedules, approvals, outreach replies, and multi-channel agent dispatch produce one canonical
  message and the expected associations/intents.
- Old task payload fixtures still decode during expansion; malformed and unknown versions fail
  observably rather than stopping silently.

## Acceptance criteria

- `rg 'MessageId|EmailAddress|ParsedEmail'` in canonical message readers/producers finds only
  adapter/projection code with documented reasons.
- Every UI and agent-history read works for a message that has no email extension.
- New background tasks contain stable IDs only and no serialized broad domain entities.

