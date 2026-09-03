# Step 5 — Create Canonical Messages and External Correlation Maps

## Outcome

Replace `email_messages` as the canonical payload store. Preserve the useful many-thread
association in `thread_messages`, but make protocol metadata and provider keys optional extensions
instead of mandatory message fields.

## Final schema

Define canonical-message storage directly in the rewritten clean-reset migration set. It must
produce only the final canonical relationships; it does not copy or preserve rows from
`email_messages`.

### Canonical storage

- `messages`: `id`, `company_id`, `author_principal_id`, optional `authored_identity_id`, subject,
  clean text, attachments, direction, role, correlation ID, content hash, and creation time.
- `message_participants`: `(message_id, participant_identity_id, kind, position)` for sender/to/cc
  projections where a transport has them. Positions preserve deterministic email formatting.
- `thread_messages.message_id` is a required FK to `messages`; keep `thread_messages.id` as the
  association ID used by outreach and UI rows. Do not retain `email_message_id`.
- Add `background_tasks.source_message_uuid` with a tenant-scoped FK and unique constraint. The old
  text `source_message_id` is not present in the final schema.

All canonical relationships carry `company_id` even when derivable, with composite FKs to messages,
threads, bindings, principals, and identities. Database checks constrain role/direction and JSON
shape. Decode attachments fallibly as versioned untrusted JSON.

### Protocol extension and correlation tables

- `email_message_metadata`: canonical message ID, RFC Message-ID, In-Reply-To, References,
  Thread-Index, raw text/html, and email authentication/envelope fields that must be retained.
  RFC Message-ID is deliberately non-unique: the same wire ID can be observed through more than
  one binding (including an internal relay's outbound and inbound sides). Deduplication is
  exclusively the binding-scoped `(binding_id, external_message_key)` relationship below.
- `external_threads`: company, binding, opaque external thread key, canonical thread ID, timestamps;
  unique on `(binding_id, external_thread_key)` and tenant-scoped to both sides.
- `external_messages`: company, binding, opaque external message key, canonical message ID,
  optional delivery-part ID, timestamps; unique on `(binding_id, external_message_key)`.
- Do not put a single `external_thread_key` back on `threads`: one canonical thread may be bound to
  email and multiple Slack conversations concurrently.

## Entity/read-model changes

- Rewrite `src/domain/entities/message.rs` so `Message` holds `PrincipalId` and optional
  `ParticipantIdentityId`, not `EmailAddress`, `MessageId`, recipient arrays, or email threading
  headers.
- Add explicit `EmailMessageMetadata` and `MessageParticipant` types under the email adapter/read
  projection boundary.
- Rewrite `src/domain/entities/thread.rs` around principal participation; keep subject/cursor logic
  protocol-neutral.
- Split `src/adapters/persistence/thread.rs` before it grows: canonical message writes and reads,
  external correlation, and email metadata get focused sibling modules with inline tests.
- Keep canonical content hashing in one pure, protocol-aware function: email hashes the retained
  raw representation, while stored/non-email events hash canonical body content. Include complete
  qualified participant identities (transport, namespace, and subject), or their resolved identity
  IDs. Dedup first by qualified external-message mapping; if a repeated provider key has a
  different hash, return a typed collision error rather than silently updating content.

## Reset-only schema rules

- New ingress and producer paths write canonical messages, email metadata, thread associations,
  external maps, and task source UUIDs directly.
- Do not add copy SQL, temporary nullable foreign keys, dual-write triggers, or ID-preservation
  logic for the pre-reset email tables.
- Step 11 ensures the migration set and code contain no email-shaped canonical tables or fields
  before Slack work begins.

## Database tests

- One message can associate with multiple same-company threads but never a foreign thread.
- One thread can map to several provider bindings; one provider thread key maps to only one thread.
- Identical provider redelivery returns the existing canonical message; changed content under the
  same key raises a collision.
- Reply-before-root upserts one external thread and both later messages join it.
- External message keys collide only inside the same binding, not across bindings.
- Invalid attachments/metadata return an application error rather than panic.

## Acceptance criteria

- A valid canonical `Message` can represent an email, Slack message, schedule prompt, system note,
  or agent answer without fabricated email fields.
- Thread lookup uses external maps, not columns embedded in the canonical message.
- A fresh migration exposes no email-shaped canonical message or thread key.
