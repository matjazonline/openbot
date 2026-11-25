# 02 — Activity types and activities

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-profiles-identifiers-and-tags.md). A store declares the kinds of happening it
tracks, and any permitted writer records them against a profile. A new channel or measure is a
new declared type, with no schema change.

## Batch 02A — Declare activity types

1. Add `attribution_activity_types`, scoped by company and store: `name`, `channel`, `direction`
   (`outbound`, `inbound` or `internal`), a property declaration, an optional `stage` token used
   by [03](03-stages-and-enrollments.md), `enabled` and `revision`. Unique on company, store and
   name.
2. Names match `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,3}$`, for example `email.sent`,
   `call.completed`, `chat.replied`, `site.reviewed`, `appointment.booked`. Channels match
   `^[a-z][a-z0-9_]{0,31}$`; there is no fixed channel list. The name `link.clicked` is reserved
   for the service, which creates it for prospect links
   ([04](04-prospect-links-and-unsubscribe.md)).
3. The property declaration lists at most 32 properties. Each has a name matching the attribute
   key pattern, a type (`string`, `number`, `boolean` or `timestamp`), a `required` flag and, for
   strings, a maximum length of at most 2,000 bytes.
4. `POST`, `GET`, `PATCH /v1/activity-types` need `definitions:write` or `links:manage`. A change
   bumps `revision`. A type is disabled, never deleted while activities reference it. Changing a
   property's type or removing a property is refused once an activity of that type exists; add a
   new property or a new type instead.
5. Ship no built-in types. Put a starter set (email, call, chat, review, appointment) in the
   contract examples so a store can post it.

Focused checks: `attribution_activity_types_` for name and channel validation, bounds, scope and
permission denial, refused incompatible changes, and two competing creations of one name ending
with one type.

## Batch 02B — Record activities

1. Add `attribution_activities`, scoped by company and store: a server id, the profile, the type
   name and the type revision it was validated against, the frozen channel and direction,
   `occurred_at`, `received_at`, optional `campaign_id` and `ad_id`, `properties`, the actor kind
   and id, a caller-supplied `activity_key`, a fingerprint of the normalized request, and
   `suppressed_at_write`.
2. `POST /v1/activities` needs `activities:write` or `links:manage`. Refuse an unknown or disabled
   type, an undeclared property, a missing required property and a wrong-typed value with a
   specific error. Properties are at most 8 KiB encoded.
3. `activity_key` is 1–128 visible ASCII characters, unique per store. The same key with the same
   normalized request returns the stored activity; the same key with a different request is 409.
   Competing deliveries of one key store one row.
4. `occurred_at` may be at most 30 seconds in the future and no older than the activity retention
   period. `received_at` is the database clock.
5. A supplied campaign must belong to the store; a supplied ad must belong to that campaign.
   Freeze the campaign and ad names with the activity the way a click freezes its dimensions, so
   later renames do not rewrite history.
6. An `outbound` activity for a profile whose contact status is `suppressed` is stored with
   `suppressed_at_write = true` and counted as a violation in reports and diagnostics. The message
   was already sent, so refusing the record would only hide it. Senders must call contactability
   before sending; state this in the contract.
7. Activities have no update or delete route.

Focused checks: `attribution_activities_` for every refusal, type-revision recording, frozen
names after a rename, foreign campaign and ad, time bounds, the suppression flag, and key scope
denial; `attribution_activities_idempotency_` with identical and conflicting competing deliveries
of one key, and with a transaction failure and a lost commit acknowledgment, ending with one row.

## Batch 02C — Read a timeline

1. `GET /v1/profiles/{id}/activities` returns the profile's activities newest first, with bounded
   pagination, optional type, channel, campaign and time filters.
2. `GET /v1/activities` lists across the store with the same filters, for diagnostics and export.
   Both need `profiles:read` or the `read` permission.
3. Add an index that serves the per-profile timeline and one that serves type-and-time scans.
   Inspect the query plans on the pilot data volume before adding any other index.

Focused checks: `attribution_activities_read_` for ordering under equal timestamps, cursor
stability while new activities arrive, filter combinations, foreign profile ids and scope denial.
Capture the two query plans in this file.

Completion: a store can declare types and writers can record and read activities with safe
retries. Record commands, counts and results here; regenerate SQLx metadata; update the contract.
Full-suite gate: [06](06-retention-erasure-and-final-verification.md).
