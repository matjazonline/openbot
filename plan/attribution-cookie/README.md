# Attribution visitor cookie: recognise a returning browser, with consent decided per visitor

> **No backward compatibility.** We don't need backward compatibility and will reset the database
> when needed. Do not add data backfills, dual reads/writes, legacy decoders, compatibility shims,
> or staged rollouts for existing data.

Status: proposed, 2026-11-25. This is an implementation plan; nothing in it is implemented.
File anchors are on `main` at `a1cdb2d5`. Re-trace every anchor with Trail before implementation.

All work is inside the independent attribution service, [services/attribution](../../services/attribution/README.md),
and follows its [AGENTS.md](../../services/attribution/AGENTS.md). Openbot's source, schema and
routes are not changed. The companion plan [flexible-tracking-crm](../flexible-tracking-crm/README.md)
is independent; neither needs the other. Both edit the service's one migration and its README, so
implement them one after the other.

## What exists today

- `GET /r/{code}` stores a click and redirects with a one-off `ob_click` id in the destination
  URL. It sets no cookie and stores no IP address or user agent.
- The browser script keeps nothing and sends nothing until the shop's consent manager calls
  `setConsent(true)`. After that it keeps up to 32 click ids in local storage for 30 days and
  hands them to the shop's server at checkout.
- There is no browser identity on the server. A click that never reaches a checkout is an
  isolated row, and reports count clicks, not visitors.

## Intended behavior

A store that sets up a tracking host on its own domain gets a visitor cookie: a random id that
lets the service see that several clicks came from one browser. The cookie identifies a browser,
never a person. It carries no email, name or profile, and nothing in this plan links it to one.

| Situation | First click through the redirect | When the page reports acceptance | On decline or withdrawal |
| --- | --- | --- | --- |
| Store has no tracking host | No cookie, exactly as today | As today | As today |
| Tracking host, consent `required` (the default) | No cookie | Cookie issued; the click that brought the visitor is attached | No cookie; the choice is remembered |
| Tracking host, consent `by_region`, visitor's country is on the store's exempt list, no opt-out signal | Cookie issued on the redirect; the click is attached | Nothing more to do | Cookie removed; the choice is remembered |
| `by_region`, but the country is not listed, is unknown, or the browser sends Global Privacy Control | Same as `required` | Same as `required` | Same as `required` |

Decided with the owner on 2026-11-24 and 2026-11-25:

- **Gradual and consent-first.** The banner comes first; the cookie is created only after it,
  except where the next point applies.
- **No prior consent where the law does not ask for it.** A store may list countries, such as
  the US, where the cookie is set on the first redirect.
- **Mixed audiences are decided per visitor from the IP address.** The lookup happens at the
  reverse proxy, which passes only a country code. The service never receives or stores the IP.
- **Anything uncertain means consent.** No tracking host, no country, an unlisted country, a
  malformed header or an opt-out signal all fall back to the consent-required flow.

What the cookie adds: unique and returning visitors per ad and campaign, and a server-side view
of several clicks by one browser. What it does not add: identity, cross-device matching, or
better order attribution, which the existing click history already covers.

## What the store needs to do

A store that does nothing keeps today's behavior. To use the visitor cookie:

1. **Pick a tracking host on the shop's own domain**, for example `go.shop.example` for a shop
   on `shop.example` or `www.shop.example`. It must be one label below an approved shop domain.
   A host on any other domain cannot work, because browsers would treat the cookie as
   third-party.
2. **Create one DNS record**: a CNAME from that host to the address shown in attribution's
   settings. Then press verify in settings. No second verification record is needed, because the
   shop's domain is already verified for the browser script.
3. **Nothing to do for the certificate.** It is obtained automatically the first time the host is
   visited after verification; that first request takes a few seconds longer.
4. **Use tracked links on the tracking host.** Settings shows each link's address on that host.
   Links placed earlier on the shared attribution address keep working but never carry the
   visitor cookie, so replace them in ads and messages.
5. **Load the browser script from the tracking host**, with the new address and integrity value
   from settings. If the site sends a Content-Security-Policy, allow the host for scripts,
   connections and images.
6. **Ask the service whether this visitor needs a banner**, then act on the answer:
   `consentRequirement()` resolves to required or not required. If it is required, or the call
   fails, show the banner. If it is not required, call `setConsent(true)` without one.
   A site with no consent tool of its own, such as a preview site, can call `showBanner({...})`
   instead: the script asks, draws a banner with the store's own wording when one is needed,
   remembers the choice and does step 7 itself; for step 9, link to `openConsentSettings()`.
   Never use it beside another consent tool.
7. **Wire the banner to the script.** Accept calls `setConsent(true, { noticeVersion })`;
   decline and any later withdrawal call `setConsent(false)`. On every later page load, call
   `setConsent(true, { noticeVersion })` again once the consent tool has restored an earlier
   acceptance. The script does not remember the choice between page loads.
8. **Write the banner and policy text.** Name the cookie `ob_visitor` and its purpose
   (recognising a returning browser to measure advertising), its lifetime (390 days), and who
   operates the service. Mention `ob_optout`, which only remembers a refusal. Change
   `noticeVersion` whenever the wording changes what a visitor agrees to.
9. **Offer a permanent way to refuse**, such as a "cookie settings" link that calls
   `setConsent(false)`. The Global Privacy Control signal is honored automatically.
10. **Choose the consent setting deliberately.** The default is `required` for everyone. Turning
    on `by_region` and listing countries is the store's legal decision; settings records who made
    it and when.
11. **Keep the checkout hand-off as it is.** Nothing changes on the shop's server.

Outside the software: if the attribution operator and the shop are different companies, they
need a data-processing agreement, and the shop answers its visitors' privacy requests.

## Rules that do not bend

- **Issuance.** The service issues a visitor cookie only on an explicit grant from the page, or
  on a redirect when the store is `by_region`, the trusted country is on the store's list, the
  request has no `Sec-GPC: 1` header and no `ob_optout` cookie.
- **Server-issued ids only.** A presented id is accepted only when the service issued it for the
  same company and store as the tracking host. Anything else is ignored and cleared. The service
  never creates a visitor row from an id the client chose.
- **Store isolation.** Visitor cookies exist only on a store's own tracking host and are
  host-only. A link of another store requested on a tracking host is not found. The shared
  attribution address never reads or sets a visitor cookie.
- **A click's visitor is set once.** It is set when the click is stored, or by the grant that
  follows, and afterwards only cleared by withdrawal or retention. It never moves to another
  visitor.
- **Withdrawal removes the link.** It deletes the visitor row, clears its clicks' visitor
  reference, expires the cookie and remembers the refusal. The clicks remain as anonymous clicks.
- **The redirect never gets worse.** Any failure in visitor handling degrades to today's
  behavior: the click is stored without a visitor and the redirect succeeds.
- **Prospect links stay out.** If the prospect links of
  [flexible-tracking-crm](../flexible-tracking-crm/README.md) exist, their redirects neither read
  nor set `ob_visitor`, and their clicks never get a visitor. A visitor cookie never connects a
  browser to a profile.
- **No new personal data.** No IP address, user agent, email or name is stored. The visitor id
  never appears in a URL, a log, a response body or a script-readable cookie.

## The cookie

`ob_visitor` holds a random UUID. It is `Secure`, `HttpOnly`, `SameSite=Lax`, `Path=/`, host-only
on the tracking host, with a fixed 390-day lifetime from issue that later visits do not extend.
`ob_optout` has the same attributes, holds no identifier, and is refreshed whenever it is seen.

## Known limits, to state in the store guide

- **Safari** is expected to cut the lifetime to about seven days when the tracking host resolves
  to a different network than the shop. Measure it in a real Safari where possible and document
  what is observed; do not promise 390 days.
- **Country from IP is approximate.** A visitor on a VPN can appear to be in a listed country.
  Unknown is safe; wrong is a residual risk the store accepts when it turns on `by_region`.
- **Two first clicks at the same instant** can create two visitors for one browser. The browser
  keeps one cookie; the other visitor keeps one click. Unique-visitor counts can be high by one.
- **Cleared cookies and other devices** start a new visitor.
- **The first request to a new tracking host** is a few seconds slower while its certificate is
  obtained. Certificate authorities limit how many certificates one domain can get per week.

## Decisions to confirm before the dependent phase

| # | Decision | Default in this plan | Needed by |
| --- | --- | --- | --- |
| D1 | Legal sign-off for `by_region`, including whether an EU-established operator may skip prior consent for visitors elsewhere | Owner's legal decision; `by_region` stays off in production until recorded | Before production use of `by_region` |
| D2 | Where the country comes from | The reverse proxy or CDN sets one trusted header; the service does no IP lookup | 01 |
| D3 | How tracking-host certificates are provisioned | Automatically, by the proxy, on the first request for a verified host | 01 |
| D4 | Cookie lifetime | 390 days, fixed from issue | 02 |
| D5 | Replace browser script 1.0.0 with 1.1.0 instead of serving both | Replace; no store is in production | 02 |

Record each answer, its date and who gave it in this file. An unanswered item stays open.

## Out of scope, with the reason

- **Linking a visitor to an email or profile.** That turns pseudonymous records into profiles of
  named people and needs its own consent wording and its own plan.
- **Sticky page variants for A/B tests.** No page-variant feature exists to make sticky.
- **An IP database inside the service.** It would bring raw IP addresses into the application.

## Phases

Implement in order. Each phase depends on the ones before it.

| Phase | Deliverable |
| --- | --- |
| [01 — Tracking host and consent settings](01-tracking-host-and-consent-settings.md) | Per-store host verified by CNAME, automatic certificates, host-bound routing, store consent setting, trusted country, the requirement rule |
| [02 — Visitor cookie and consent](02-visitor-cookie-and-consent.md) | Visitor records, redirect issuance, grant and withdrawal, browser script 1.1.0 with an optional built-in banner |
| [03 — Reports, retention and final verification](03-reports-retention-and-final-verification.md) | Visitor measures, lifetimes, store guide, deployment proof, full gate |

## Verification discipline

Each batch names focused checks; run only those while implementing it. List a filtered test set
before running it and confirm a nonzero count:

```sh
cd services/attribution
cargo test --locked --offline --all-targets PREFIX -- --list
cargo test --locked --offline --all-targets PREFIX
```

Database tests use the disposable fixture and fail when it is unavailable. After SQL changes run
`scripts/verify.sh prepare`. Record commands, selected and executed counts, results and any check
that could not run in the phase file. The one full-suite gate is in
[03](03-reports-retention-and-final-verification.md).

No resource limit is raised: the redirect's admission, connection, body and SQL budgets and the
2 MiB stack budgets stay as they are. The redirect gains at most one indexed read and one insert;
measure its latency before and after and record both.

## Anchors

- Redirect: `redirect` in [adapters/catalog/http.rs](../../services/attribution/src/adapters/catalog/http.rs)
  and [adapters/catalog/postgres.rs](../../services/attribution/src/adapters/catalog/postgres.rs),
  where the click is inserted and `ob_click` is appended.
- Browser script: [assets/sdk-1.0.0.js](../../services/attribution/assets/sdk-1.0.0.js). It takes
  the service address from its own script URL, so loading it from the tracking host sends its
  calls there. Routes and CORS: [adapters/website/http.rs](../../services/attribution/src/adapters/website/http.rs).
- Domain verification by DNS TXT: [adapters/setup/dns.rs](../../services/attribution/src/adapters/setup/dns.rs).
  Tracking hosts reuse its resolver and time limits for their CNAME check.
- Stores and settings: [application/setup.rs](../../services/attribution/src/application/setup.rs),
  `attribution_stores` in [the migration](../../services/attribution/migrations/202611150001_foundation.sql).
- Configuration: [infra/config.rs](../../services/attribution/src/infra/config.rs). The only
  cookies today are the dashboard's sign-in cookies in
  [adapters/identity/http.rs](../../services/attribution/src/adapters/identity/http.rs).
- Sample proxy: [deploy/nginx.conf](../../services/attribution/deploy/nginx.conf). It clears
  `X-Forwarded-For` and turns access logs off; keep both.
- Store guide and reports: [WEBSITE.md](../../services/attribution/WEBSITE.md),
  [REPORTING.md](../../services/attribution/REPORTING.md), [operations.md](../../services/attribution/operations.md).
