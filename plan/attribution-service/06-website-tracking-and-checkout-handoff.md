# 06 — Website tracking and checkout handoff

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [05](05-links-and-durable-clicks.md). Participating websites record observable loads and
carry eligible click evidence into a merchant-controlled checkout. Browser observations never become
trusted sales, and a tracking identifier never becomes a visitor's authenticated identity.

## Batch 06A — Collect distinct, bounded website observations

1. Add `GET /pixel.gif` returning a transparent image and `POST /v1/events` for a small enum of
   observation types: image load, landing page load, and visible placement. Require an active
   public installation and validate related click/link IDs within its company/store.
2. Disable image/event response caching and document cache-busting for pixel embeds. A plain
   pixel measures an observed image request, not viewability or a verified ad impression. A
   visibility script records the chosen viewport threshold/duration and labels that separately.
3. Validate body size, field lengths, timestamps, event type, payload shape and permitted origin
   where available. Enforce exact origin CORS for approved sites; CORS/referrer checks are not
   authentication and cannot make an observation trustworthy. Pixel requests may omit Origin.
4. Deduplicate browser retries by installation plus opaque event ID with a bounded uniqueness
   horizon at least as long as the documented retry window. A retry keeps its original ID; a new
   page observation gets a new one. Pixel embeds should provide an observation ID; without one,
   each request counts separately and the API/reporting contract must say so.
5. Return success only after durable insertion/deduplication. On storage failure return a
   retryable failure rather than a successful image/204 that pretends tracking was recorded.

Focused checks: `attribution_website_event_` with permitted/forbidden kinds, forged sales, foreign
IDs, duplicate/conflicting events, concurrent inserts, storage failure, correct GIF/no-store headers,
origin handling, and public endpoint overload. Verify event-type labels against API and UI fixtures.

## Batch 06B — Ship the small browser integration and merchant handoff

1. Serve a versioned, pinned script from attribution with installation examples. It captures the
   reserved click parameter, validates shape, records permitted observations, and exposes a small
   explicit handoff to the participating shop. Do not embed merchant/service API keys in JavaScript.
2. Implement the consent behavior agreed in phase 01. Do not persist click IDs across visits or
   send optional tracking before the appropriate consent signal. Define withdrawal and expiry;
   avoid fingerprinting and collecting customer email/name in event bodies or tracking URLs.
3. The merchant backend binds click evidence to its own server-controlled cart/checkout session.
   It validates the click's company/store and records the latest eligible consented click before
   order creation. Keep enough candidate history to avoid a later ineligible/bot click blindly
   replacing the last eligible click. Freeze checkout attribution evidence at order creation.
4. Document that last-click means the merchant's observed, consented checkout history; attribution
   cannot infer an unseen earlier/later click or cross-device identity. Attribution revalidates
   the supplied chosen click when accepting the paid order. Browser form fields alone cannot
   establish order identity or prove payment.
5. Exercise navigation, return visits within policy, cart persistence, query removal after capture,
   consent withdrawal, and checkout on the selected merchant platform. A cross-origin checkout
   handoff must use that platform's supported server-side association, not third-party cookies.

Focused checks: `attribution_checkout_handoff_` and real browser/script tests for Ad A then Ad B,
expired/foreign/bot candidates, multiple tabs, blocked storage, no consent, and server-side checkout
freeze. Assert emitted URLs/payloads contain no customer PII or privileged credentials. Record
limitations caused by blocking or platform placement restrictions as product behavior.

Completion: clicks and observations are independently labeled, click evidence survives the chosen
checkout flow where permitted, and no pixel can create a sale. Record exact batch checks here;
final full-suite gate is in [10](10-integration-verification-and-pilot.md).
