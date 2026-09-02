# Step 11 — Delete the Email-Shaped Spine and Enforce the Abstraction Gate

## Outcome

Finish the direct canonical replacement before Slack code is allowed into the repository. The
database-reset premise means the final migration set and runtime contain no transitional tables,
fields, payloads, or compatibility adapters.

## Final schema definition

Rewrite/consolidate the migration set so a clean run defines the final model directly:

- canonical `thread_messages.message_id` and task source UUID constraints are required;
- `task_outreach_targets.response_message_id` and other FKs point to the intended canonical or
  association ID explicitly;
- no `thread_messages.email_message_id`, `threads.external_thread_key`, or email-key task columns;
- no email-keyed `thread_participants`/`channel_participants`;
- no `email_outbox` or email-shaped canonical `email_messages` table; and
- `email_message_metadata` exists only as a protocol extension of canonical `messages`.

Define canonical message notifications directly, preserving transaction-bound wake semantics. Do
not create a contract/cleanup migration whose only purpose is converting the pre-reset schema.

Do not add mismatch assertions, row-copy logic, queue draining, or populated-database upgrade
support. The deployment procedure resets the database before this migration set runs. The final
dependency graph must still be explicit and reproducible.

## Code removal and naming cleanup

- Delete `NormalizedInboundMessage`, `NormalizedOutboundMessage`, `ParticipantIdentity`,
  `ChannelType`, `parsed_email_from_normalized`, adapter compatibility constructors, and broad old
  task writers/readers in the same implementation series.
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
- Fresh migration from an empty database and bootstrap through final-model creation paths.
- Schema assertions prove the removed email-shaped tables/columns do not exist.
- Offline SQLx compilation from committed metadata.

## Acceptance criteria

- Email works exclusively as an adapter over transport-neutral application/domain contracts.
- The build fails if later work leaks transport types back into the canonical spine.
- Only after this commit is green may step 12 add Slack-specific modules.
