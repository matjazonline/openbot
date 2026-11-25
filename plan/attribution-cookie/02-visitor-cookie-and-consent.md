# 02 — Visitor cookie and consent

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

## Outcome and dependency

Depends on [01](01-tracking-host-and-consent-settings.md) and on decisions D4 and D5 in the
[README](README.md). On a store's tracking host, a browser receives a visitor cookie after the
page reports acceptance, or on the first redirect when the requirement rule says consent is not
needed. Clicks carry the visitor, and withdrawal removes it.

## Batch 02A — Visitor records and the redirect

1. Add `attribution_visitors` to the one migration, scoped by company and store: id, created
   time, `basis` (`consent` or `region_exempt`), the notice version for `consent`, the country
   for `region_exempt`. Add a nullable `visitor_id` to `attribution_clicks` with a composite
   foreign key to the visitor in the same company and store, and an index that serves counting
   distinct visitors by link and time.
2. Trace the existing tenant lock and transaction in the redirect before adding anything. Visitor
   handling joins that transaction; it must not add a second commit or a new failure exit.
3. On `GET /r/{code}` arriving on the link's own tracking host:
   - Read `ob_visitor`. Accept it only when it is one well-formed UUID that names a visitor of
     this company and store. More than one cookie of that name, a malformed value or an unknown
     id is treated as absent, and the response expires the cookie.
   - With an accepted visitor, store the click with that visitor.
   - Without one, call the requirement rule. If consent is not required, create a
     `region_exempt` visitor, store the click with it and set the cookie. Otherwise store the
     click with no visitor and set nothing.
   - When `ob_optout` is present, refresh it and never issue a visitor.
4. Clicks classified as bot or prefetch never create a visitor and never receive a cookie.
   `HEAD` creates nothing, as today.
5. On any host that is not a tracking host, and so for every store without one, the redirect is
   byte-for-byte today's behavior: no cookie is read or set.
6. If any visitor step fails, store the click without a visitor and redirect. A cookie is set
   only in a response whose transaction committed the visitor row.
7. Set cookies with the attributes in the README. Responses that set or clear a cookie keep
   `Cache-Control: no-store`.
8. If the prospect links of [flexible-tracking-crm](../flexible-tracking-crm/README.md) exist, a
   prospect link's redirect neither reads nor sets `ob_visitor`, and its click never gets a
   visitor.

Focused checks: `attribution_visitor_redirect_` for each row of the README's behavior table, the
exact `Set-Cookie` attributes, foreign-store and forged ids, duplicate cookies, bot and prefetch
requests, `HEAD`, the opt-out refresh, an injected visitor-insert failure that still redirects,
unchanged responses on any other host, and, where prospect links exist, no cookie on their
redirects. With competing claimants: many simultaneous
first redirects from one cookie-less browser, asserting every click has exactly one visitor or
none, no error, and no duplicate id; and a redirect racing a withdrawal of the same visitor,
asserting the click ends attached to a live visitor or to none.

## Batch 02B — Requirement, grant and withdrawal

1. Add two routes, served only on a tracking host, for an installation of that host's store:
   - `GET /v1/visitor-consent` answers whether consent is required for this request and whether
     a valid visitor cookie is present. It stores nothing.
   - `POST /v1/visitor-consent` takes the installation id, a decision (`granted` or
     `withdrawn`), a notice version of 1–64 characters, and up to 32 candidate click ids.
2. Both require an `Origin` that is an active approved origin of the store. Answer with that
   exact origin, `Access-Control-Allow-Credentials: true` and `Vary: Origin`; never a wildcard.
   Requests without an approved origin are refused. The existing `/v1/events` and `/pixel.gif`
   stay credential-free and unchanged.
3. Grant: with an accepted visitor cookie, keep that visitor. Otherwise create one and set the
   cookie. Its basis is `consent` with the supplied notice version when the rule says consent is
   required, in which case the notice version is mandatory; otherwise `region_exempt`. A grant
   clears `ob_optout`. A grant is the store's statement that the visitor accepted that notice;
   say so in the contract.
4. Attach candidates in the same transaction: each click that belongs to this store, has no
   visitor, is not bot or prefetch, and was clicked within the last 30 days gets this visitor,
   with a compare-and-set on the empty reference. Clicks that already have a visitor are left
   alone. The response does not reveal which candidates were attached.
5. Withdrawal: clear the visitor reference on the visitor's clicks, delete the visitor row,
   expire `ob_visitor` and set `ob_optout`, in one transaction. Without a visitor cookie it only
   sets `ob_optout`. It is idempotent.
6. Bound both routes like the other public routes: the same admission, body limit and deadlines.
   Neither returns a visitor id.

Focused checks: `attribution_visitor_consent_` for origin and installation checks, the mandatory
notice version, basis selection from the rule, foreign, expired, bot and already-attached
candidates, idempotent withdrawal, the cleared and set cookies, and absence of any id in
responses and logs. With competing claimants: two grants with overlapping candidates and no
cookie, asserting each click is attached to exactly one visitor; and a grant racing a withdrawal
for one visitor, asserting the end state is either a live visitor with its clicks or no visitor
and no click pointing at one.

## Batch 02C — Browser script 1.1.0

1. Publish `sdk-1.1.0.js` in place of 1.0.0, with its route, a new integrity value, and updated
   entries in `scripts/verify.py` and the install snippet.
2. Add `consentRequirement()`, which calls the requirement route with credentials and resolves
   to required or not required. A network error, a timeout or an unexpected answer resolves to
   required.
3. `setConsent(true, { noticeVersion })` keeps its current local behavior and also sends a grant
   with the click ids it holds. `setConsent(false)` keeps its current local clearing and also
   sends a withdrawal. Both retry within the script's existing bounded retry rules and stop on
   page hide.
4. When the script was not loaded from a tracking host, the consent routes are absent. The
   script must detect that once, skip the visitor calls and keep every existing behavior.
5. The script still creates no cookie itself and reads none; the visitor cookie is `HttpOnly`.
   Update the header comment, which today says "no cookies".
6. Add `showBanner(options)` for sites with no consent tool of their own, such as preview sites.
   It first calls `consentRequirement()`; when consent is not required it calls
   `setConsent(true)` and draws nothing. Otherwise it draws a small banner with the store's own
   `title`, `text`, `acceptLabel`, `declineLabel`, `policyUrl` and `noticeVersion`; there is no
   default wording. Accept and decline are equally prominent and reachable by keyboard, the
   banner is labelled for screen readers, and nothing is sent before a choice. It keeps the
   choice and its notice version in the site's local storage, the one thing stored before
   consent, as the record of the choice; a new notice version asks again. `openConsentSettings()`
   shows the banner again so a visitor can change their mind. Styles are set through the DOM
   style API, so a strict `style-src` policy does not block them.

Focused checks: `node --check` on the asset; `attribution_visitor_browser_`, an ignored browser
journey over real HTTPS with a shop page and a tracking host on one test domain, covering:
decline leaves no cookie; accept sets one and attaches the arriving click; a second click
reuses it; withdrawal removes it and a later redirect does not bring it back; a listed country
gets the cookie on the first redirect with no banner; an unlisted country does not; Global
Privacy Control suppresses the automatic cookie; the built-in banner shown only when required,
with equal keyboard access to both buttons, its choice kept across reloads, a new notice version
asking again and reopening from a settings link; and a script loaded from the shared address
behaves as 1.0.0 did. Add the prefix to `scripts/verify.py`, the CI workflow and the root runner.

Completion: the visitor cookie works end to end on a tracking host and is absent everywhere
else. Record commands, counts, results and the redirect latency before and after here;
regenerate SQLx metadata; update the contract. Full-suite gate:
[03](03-reports-retention-and-final-verification.md).
