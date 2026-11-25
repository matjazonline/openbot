# 04 — Prospect links and unsubscribe

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [03](03-stages-and-enrollments.md) and, for prospect links, on decisions D5 and D6 in
the [README](README.md). Every outbound message can carry an unsubscribe link that stops its
address for every agent at once. A store that has turned prospect links on can give each
organization its own tracked link, and a click is recorded on that organization's profile.

Both kinds of link are public. They are served wherever the store's tracked links are served,
set no cookies, and show no address or profile data.

## Batch 04A — Unsubscribe links

1. Add `attribution_unsubscribe_tokens`, scoped by company and store: identifier kind (`email` or
   `phone`), normalized value, a random 32-byte token in unpadded base64url, created time. Unique
   on company, store, kind and value, and on the token. It has no foreign key to a profile, so it
   can outlive one, like a do-not-contact entry.
2. `POST /v1/unsubscribe-links` needs `activities:write` or `profiles:write`. It takes a profile
   and one of that profile's email or phone identifiers, and returns the link plus the two email
   header values: `List-Unsubscribe: <link>` and `List-Unsubscribe-Post: List-Unsubscribe=One-Click`.
   A repeat returns the same link. An identifier that is already suppressed is refused with 409
   `suppressed`, a second stop for a sender that skipped the contactability check.
3. `GET /u/{token}` returns a small page with one Unsubscribe button. It changes nothing: mail
   security filters open links on delivery, and a GET that unsubscribed would stop people who
   never asked. An unknown or malformed token gets a page saying the link is not valid and
   suggesting a reply to the message instead.
4. `POST /u/{token}`, from that button or from a mailbox provider's one-click request with the
   body `List-Unsubscribe=One-Click`, adds a do-not-contact entry with reason `asked` and stops
   enrollments as [03](03-stages-and-enrollments.md) describes. It needs no cookie or session, is
   idempotent, and answers with a plain confirmation. Record an audit row with a new actor kind,
   `recipient`, added to the audit table's constraint.
5. Bound both routes like the other public routes. Neither shows the address, any profile data,
   or whether a profile exists.

Focused checks: `attribution_unsubscribe_` for a GET that changes nothing, the form and one-click
POSTs, idempotent repeats, unknown and malformed tokens, the entry and the stopped enrollments,
refusal for a suppressed identifier at creation, scope denial, and no cookie and no address in any
response. With competing claimants: simultaneous one-click POSTs for one token ending with one
entry, and an unsubscribe racing an activity for the same enrollment ending at `stopped` unless
the activity reached a final stage first.

## Batch 04B — The store's prospect-link setting

1. Add `prospect_links` to `attribution_stores`, off by default. Turning it on needs
   `integrations:manage` from a signed-in user and an explicit acknowledgement in the request that
   the store has a legal basis for per-recipient tracking (decision D6). It writes an audit row
   naming the actor. A server key cannot change it.
2. The activity type `link.clicked` is reserved ([02](02-activity-types-and-activities.md)). The
   service creates it for the store the first time the setting is turned on: channel `link`,
   direction `inbound`, properties `link_id` (string), `scanner_suspected` (boolean) and
   `scanner_rule` (number). A store may give it a stage and enable or disable it, nothing else.

Focused checks: `attribution_prospect_settings_` for the acknowledgement, the audit row, key
denial, the reserved type created exactly once under competing first enables, and refusal to
change its properties.

## Batch 04C — Prospect links and their clicks

1. Add a nullable `profile_id` to `attribution_links`, with a composite foreign key to a profile in
   the same company and store and an index. A link with a profile is a prospect link; every other
   link behaves exactly as today.
2. `POST /v1/prospect-links` needs `links:write` and the setting on. It takes an organization
   profile, an ad, a destination and a placement, and validates the destination exactly as other
   links. If the profile is not enrolled in the ad's campaign, it is enrolled with that ad as its
   variant in the same transaction; if it is enrolled with another variant, the request is
   refused, so an A/B comparison stays clean. A repeat for the same profile, ad and destination
   returns the existing link. Refuse a person profile (D5), a suppressed profile, and a foreign
   profile or ad, each with its own error.
3. While the setting is off, `GET /r/{code}` for a prospect link redirects to the destination
   with no `ob_click` and stores nothing. Links in messages already sent keep working without
   tracking anyone.
4. While it is on, store the click exactly as today and, in the same transaction, record a
   `link.clicked` activity on the profile with the link's campaign and ad, an `activity_key`
   derived from the click id, and the scanner verdict. Write the activity inside a savepoint: if
   it fails, roll back to the savepoint, keep the click, redirect, and log a fixed outcome label.
   Clicks classified as bot or prefetch get no activity.
5. The scanner rule is fixed in code and versioned like the bot classifier. A click is suspected
   when it arrives within 120 seconds of the link's creation, or within 5 seconds of a click on
   another prospect link of the same profile. The first condition relies on senders creating the
   link just before sending; say so in the contract. Change the rule only on measured evidence and
   record why. A suspected click never moves a stage.
6. If the visitor-cookie plan ([attribution-cookie](../attribution-cookie/README.md)) is
   implemented, a prospect link's redirect neither reads nor sets `ob_visitor`, and its click never
   gets a visitor. Orders attributed to a prospect-link click are not shown on the profile.

Focused checks: `attribution_prospect_links_` for every creation refusal, the variant conflict,
implicit enrollment, idempotent creation, the setting-off redirect storing nothing and adding no
`ob_click`, the setting-on redirect storing the click and the activity together, bot and prefetch
clicks, an injected activity failure that still redirects with the click stored, both scanner
boundaries, stage movement only for unsuspected clicks, and no visitor cookie on prospect
redirects. With competing claimants: simultaneous clicks on one link each producing their own
click and activity, and the setting turned off while clicks are in flight, each click ending
either fully tracked or untracked.

Completion: every message can be unsubscribed from, and a store that has made the legal decision
can see which organizations clicked. Record commands, counts and results here; regenerate SQLx
metadata; update the contract and its examples. Full-suite gate:
[06](06-retention-erasure-and-final-verification.md).
