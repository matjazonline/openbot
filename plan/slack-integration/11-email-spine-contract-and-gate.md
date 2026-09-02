# Step 11 — Remove the Legacy Email Spine and Enforce the Abstraction Gate

## Outcome

Finish the canonical cutover before Slack code is allowed into the repository. Remove transitional
tables, fields, payloads, and compatibility adapters only after data/runtime evidence proves they
are unused.

## Preconditions

- New email ingress writes only canonical messages, identities, external maps, tasks v2, and generic
  deliveries.
- Every message reader and non-ingress producer uses the canonical model.
- Generic delivery workers have drained or imported every `email_outbox` row.
- Operational queries show zero old task payloads and zero canonical/backfill mismatches for at
  least one full maximum task/approval/outreach lifetime, or an explicit offline migration handles
  the remaining rows.
- A backup and restore rehearsal has been completed before destructive schema changes.

## Contract migration

Create `migrations/20260902050000_contract_legacy_email_spine.sql` and, in dependency order:

- make canonical `thread_messages.message_id` and task source UUID constraints required;
- repoint `task_outreach_targets.response_message_id` and other FKs to the intended canonical or
  association ID explicitly;
- remove `thread_messages.email_message_id`, `threads.external_thread_key`, and old email-key task
  columns only after unmatched-row assertions;
- drop legacy `thread_participants`/`channel_participants` after principal equivalents are complete;
- drop `email_outbox` only after every row is represented in generic deliveries;
- drop or rename `email_messages` only after `email_message_metadata` contains all required
  protocol fields and no code references the old table;
- replace old triggers/functions with canonical message notifications, preserving transaction-bound
  wake semantics.

Use guarded precondition blocks that abort on non-zero mismatch counts instead of silently deleting
data. Do not use `CASCADE` to discover dependencies.

## Code removal and naming cleanup

- Delete `NormalizedInboundMessage`, `NormalizedOutboundMessage`, `ParticipantIdentity`,
  `ChannelType`, `parsed_email_from_normalized`, adapter compatibility constructors, and v1 task
  writers/readers once the row census is zero.
- Rename `email_outbox` modules, `OutboxEmail`, `OutboundSend`, and UI labels to delivery-neutral
  names. Keep `OutboundEmail` only inside the email renderer/sender boundary.
- Move the email parser fully under the email adapter and remove application imports of MIME,
  Lettre, RFC-header, or app-domain address types.
- Search documentation and examples for promises that channels are email addresses; describe
  email as the initial/default binding.

## Abstraction gate

Add a CI script such as `scripts/transport-boundary-check.sh` that fails on:

- imports from `crate::adapters` inside `src/application` or `src/domain` (maintain a narrow,
  reviewed allowlist only while other unrelated debt exists);
- Slack/email provider types in canonical message/thread/participant entities;
- SQL table names `email_messages` or `email_outbox` after contract;
- construction of synthetic `.invalid` email addresses or RFC Message-IDs outside the email adapter;
- direct `OutboundEmail` construction outside `adapters/protocols/email`.

Run it in CI beside formatting, offline compilation, migrations, the database suite, Clippy, and
stack-budget. This is the early failure signal that replaces the removed structural limitation.

## Regression suite

- SMTP, SendGrid, simulation, mailbox compose, agent reply, schedule delivery, approval, bounce,
  outreach/quorum, inter-channel delegation, attachment access, live mailbox updates, and outbox UI.
- Fresh migration and upgrade migration from a populated pre-refactor fixture.
- Database count/hash/ordering equivalence before and after contract.
- Offline SQLx compilation from committed metadata.

## Acceptance criteria

- Email works exclusively as an adapter over transport-neutral application/domain contracts.
- The build fails if later work leaks transport types back into the canonical spine.
- Only after this commit is green may step 12 add Slack-specific modules.

