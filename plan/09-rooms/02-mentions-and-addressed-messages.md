# 02 Mentions, replies and notifications

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome

Each accepted room message records exactly whom it addresses. The same facts drive human notices
and agent turns, without forwarding to direct inboxes. Continues in [03](03-agent-turns-and-ordering.md).

## Batch 02A Normalize and store addressing

1. Introduce a bounded typed addressed-member list on the message/thread association: principal
   id, first-occurrence order and reason (`mention` or `reply`). Persist it with the canonical
   message, thread association and durable admission intent. Pin company, room and thread with
   composite foreign keys. Association-level storage matters: one canonical email can appear in
   several threads, whose rosters differ. Do not overload email `To`/`Cc` projections.
2. The product composer supplies structured mention spans and principal ids. Validate offsets,
   UTF-8 boundaries, displayed handles and membership server-side; reject forged or stale explicit
   targets before accepting a product/API write. Store resolved identities immutably, so a later
   rename or reuse of a slug never retargets the saved message. Deduplicate by principal in first
   mention order. Maximum 16 distinct addressees per message; reject overflow, never truncate.
3. Existing email ingress maps bounded, case-insensitive complete `@slug` tokens from the newly
   authored body to current members. Reuse the token-boundary rules of `body_mentions_slug` in
   `src/application/use_cases/thread/support.rs`; extract candidates once rather than scanning
   the body separately for every member. Exclude quoted/forwarded sections, signatures, Markdown
   code, HTML code/quotes, attachments and fetched content using the adapter's normalized body
   representation. When authored content cannot be separated safely, accept it without automatic
   mentions and record that outcome. Do not strip quote information before mention extraction.
4. Text tokens not resolving to a member stay literal text and cause no notification or work.
   The UI can show unresolved handles without revealing non-member directory entries. Explicit
   structured targets are stricter: invalid targets refuse the write rather than silently dropping
   an intended request. A rename between text acceptance and execution does not trigger re-parsing.

Focused checks: `room_mention_parse_` covers token boundaries, punctuation, case, Unicode offsets,
duplicates, overflows, ambiguous/unknown names and all excluded content; `room_addressed_write_`
covers tenant/thread constraints, retries, rename/rekey races and all-or-nothing structured writes.

## Batch 02B Reply semantics and authenticated origins

1. Resolve an explicit reply to a canonical message in this same room/thread that the sender may
   read. If a human message has no explicit resolved mentions, address that message's author when
   they are still a room member. With explicit mentions, use only those recipients; the UI shows
   this choice. A plain message has no implicit audience beyond room readership.
2. Distinguish conversational `in_reply_to` from a request for another turn. An agent's response
   naturally replies to its triggering message; this never implicitly wakes the prior author.
   Agents address members through structured recipients in the existing response/publication
   contract. Render those as mentions. Raw model prose, recalled history and tool output cannot
   mint addressed recipients. Self addressing is visible text but starts no turn or self notice.
3. Derive human, agent and system origin from authenticated ingress or the owning workflow run,
   never a request field or mail header. System/notification origins start no discussion. Normal
   agent publication and inbound adapters converge on one addressed-message commit operation.
4. Preserve reply-wait correlation from `MessageAdmissions`: a message that answers a parked
   workflow question is consumed by that wait and does not also admit new room turns. Duplicate
   and competing human answers retain the existing one-winner contract. The composer identifies
   replies to waiting questions so users understand which interaction they are completing.

Focused checks: `room_reply_addressing_` proves explicit precedence, deleted/foreign parent
refusal, human implicit reply and agent non-recursion; `room_reply_wait_` proves no double
admission and competing-answer behavior; `room_origin_` refuses forged human/system provenance.

## Batch 02C Human notifications

1. Addressed person members get an in-product event through the existing durable notification
   path, with a stable key from message association and principal. A retry, repeated mention or
   simultaneous reply/mention yields one notice. Reuse preferences and live view/level rechecks;
   remove payload access when the principal or membership is revoked.
2. Notification generation commits with the accepted message's addressing facts. Notifications
   do not re-enter room ingress, create direct threads or wake agent bindings. Email transport
   delivery remains separate from notification policy; add no new email-alert preference.

Focused checks: `room_notification_` covers exact recipients, retries, membership/level revocation,
no direct-inbox copy, and notification-origin recursion suppression. Use the README's focused
commands and record selected/executed counts for each batch.
