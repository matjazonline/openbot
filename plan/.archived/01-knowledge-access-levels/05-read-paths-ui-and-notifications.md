# 05 — Read paths: mailbox, previews, attachments, notifications, run views

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

A stamped message or run is readable only by a viewer whose level is at least its stamp.
Everywhere else in the app it shows as a placeholder (P7). Channel access still decides who may
open a thread; the level decides what they see inside it.

## Batch 05A — One visibility rule

- One pure rule, `visible_to(viewer: AccessLevel, stamp: AccessLevel) -> bool`, lives next to
  `may_release`.
- Read queries return the stamp with each message, and the application decides. Do not filter
  bodies out in SQL and leave callers guessing why a row is missing: the thread view needs the
  placeholder in the right position.
- The placeholder says "Restricted message". It shows the author (the channel address) and the
  time. It shows no subject, body, excerpt or attachment names, and attachment downloads for that
  message are refused.

## Batch 05B — Sweep every body reader

Run `graft grep "clean_text_body"` and the same for subject and attachment readers. Each
production hit must do one of two things:

- apply `visible_to` for the current viewer; or
- carry a comment explaining why it is not a viewer path. For example, the delivery worker sends
  to recipients that the release check already approved.

Known production readers today:

- `src/adapters/persistence/thread/message.rs`
- `src/adapters/persistence/thread/views.rs` (thread list previews and the latest-message excerpt)
- `src/adapters/http/routes/ui_attachments.rs`
- `src/adapters/http/routes/channel.rs`
- `src/adapters/persistence/workflow/message_intents.rs` (trigger input, already covered by 03A)
- `src/adapters/persistence/workflow/history_reader.rs` (covered by 03C)

Re-run the sweep at the end of the phase. New readers added in the meantime count too.

## Batch 05C — Notifications and attention

- Notification emails and in-app notifications that quote a message use the recipient member's
  level. Above it, they say "New restricted message in <thread subject>", with no excerpt. Review
  `src/application/services/notification_worker.rs` and `src/application/notification.rs`.
- Attention items for `knowledge.audience_below_content` (03D) never include content. They name
  the thread, the run and the two levels.

## Batch 05D — Run views

- A run's step inputs, outputs, tool calls and observations are shown only to viewers whose
  level is at least the run's `content_level`. Lower viewers see the run's status, timing and
  step names, with payloads redacted. Apply this in the run read models
  (`src/application/workflow/run_views.rs`) and the run board.
- The knowledge library page already filters by level (02B).

## Focused checks

Prefix `knowledge_view_`:

- a level-0 member sees the placeholder in the thread view and list preview for a level-5 reply;
- a level-5 member sees the full message;
- an attachment download on a stamped message is refused below its level;
- a notification excerpt is withheld below its level;
- run payloads are redacted below the content level;
- the owner sees everything;
- rendered HTML for the placeholder contains no subject, body or attachment name (assert on the
  markup, as the existing page tests do).

Extend the HTML-injection browser fixtures if the placeholder adds new markup.
