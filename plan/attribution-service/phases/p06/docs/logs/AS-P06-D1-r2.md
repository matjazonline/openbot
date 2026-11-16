## AS-P06-D1 / r2 — proposed architecture

1. Add local observations and checkouts to the reset-only initial schema. Observations
   have installation+opaque event ID uniqueness, scoped company/store, received DB
   time, claimed observed time, enum kind, related IDs, normalized fingerprint and
   immutable scoped dimensions. The accepted frozen link destination may remain in
   click dimensions; it is not a collected visitor page URL. No visitor/customer identity, IP, browser page URL, UA/referrer or
   merchant key payload. Click and link references are checked under the accepted
   tenant lock. Both present must identify the same link. Click observations reuse
   its frozen dimensions; link-only observations snapshot current scoped dimensions.
   Historical disabled links may receive observations through an active installation;
   disabled link status still prevents new tracking clicks. No raw-click foreign key
   prevents later bounded pruning of evidence whose snapshot is already retained.
2. Observation body uses strict typed fields/unknown/duplicate rejection and8KiB;
   timestamps explicitly offset RFC3339 strings <=40 bytes, retaining precision in
   canonical fingerprint. Kind is image_load/landing_page_load/visible_placement;
   consent=true and at least one scoped click/link required. Public installation
   lookup gives only company scope; existing identity.tenant, tenant admission and
   SetupPostgres.public_installation perform active company/store/domain/installation
   recheck in the insertion transaction. Deduplication happens only after current
   eligibility and origin checks. Same identity/content replays; changed content409.
   Database time bounds original observed_at to at most24h old and30s future; receipt
   uniqueness persists >=48h. These are protocol retry bounds/minimum retention,
   not approval of production retention/region/load policy; p09 config must honor them.
3. GET /pixel.gif stores image_load and returns a fixed transparent GIF only after
   COMMIT; failure/uncertain commit503, exact replay succeeds. No supplied event ID
   means each GET has a fresh event ID and server timestamp, with no retry identity.
   Explicit HEAD validates without insertion or successful tracking acknowledgement
   (204/no image). With an event ID, require original observed_at too. This small
   pixel-query extension is necessary to reject old replays after pruning: the
   accepted query shape currently lacks any timestamp. Add the parameter/conditional
   requirement to OpenAPI, docs and examples (AS-C005/r1); no compatibility shim.
   Without event ID, reject a supplied observed_at to keep semantics unambiguous.
4. POST /v1/events requires an exact approved installation origin when supplied;
   absent Origin is permitted for non-browser public clients/pixels and never grants
   trust. Match serialized HTTPS origin to the active exact domain; reject null,
   credentials, path/query/fragment and nondefault port. Successful/failed known-origin
   responses carry exact Access-Control-Allow-Origin and Vary:Origin; no credentials
   allowance or wildcard. OPTIONS checks that the origin has some currently approved
   domain and enabled store, allows POST/Content-Type only; actual POST rechecks its
   exact installation/tenant and never relies on preflight for authorization. Unknown
   origins get no ACAO. CORS and referrer cannot authenticate facts. Public observation
   admission16 plus shared per-tenant8/global32 and existing deadlines; no spawned
   ingestion workers. Redirect retains its accepted separate16-slot budget.
5. POST /v1/checkouts authenticates checkouts:write headers before its <=16KiB body,
   rejects user/public credentials and never accepts body tenant/store overrides.
   begin_merchant owns the transaction and last-use commit. Normalize distinct
   candidate IDs as a sorted set <=32, checkout ID <=128 nonwhitespace characters,
   explicit bool consent and full-precision original order_created_at. Bound original
   creation to24h old/30s future, including replays, so eventual pruning cannot make
   an old checkout newly writable. Scope immutable identity to company/store/external
   checkout ID; bind normalized fingerprint. Concurrent exact requests have one
   frozen record/result; changed facts409; revoked keys cannot replay it.
6. Evaluate at most32 scoped click rows in the authorized transaction. Select latest
   server clicked_at, UUID as stable tie breaker, in inclusive [order_created_at-30d,
   order_created_at], excluding either bot/prefetch flag. A later invalid candidate
   cannot overwrite earlier eligible evidence. No consent=>no_consent; empty=>missing.
   With no eligible candidate, fixed precedence foreign_or_missing, future, expired,
   bot yields a deterministic reason without foreign details. Save selected click
   identity/time/classification/dimensions and policy last-eligible-click-30d-v1, or
   explicit reason, once. Keep full-precision original creation as canonical text
   for equality/eligibility rather than silently rounding nanoseconds in PG. Do not
   retain raw candidates; the fingerprint binds them and checkout evidence retains
   only the selection. p07 must revalidate eligibility at first paid fact, including
   availability of required evidence, and never mutate a frozen checkout/order.
7. Publish a fixed-version browser asset /sdk/1.0.0.js with SRI/pinned installation
   example and a small API: create(installation_id), setConsent(boolean), handoff(),
   observeVisible(element,link_id). Construction captures/validates ob_click only in
   memory and removes that parameter via URL/history while preserving others/fragment.
   It reads/writes no optional persistent storage and sends no observation until
   explicit consent. Granted consent records one landing event only when a captured or consented
   retained click supplies the required scoped reference; otherwise it sends no
   landing observation. It merges at most32
   candidate IDs using installation-scoped per-click localStorage keys, expires local
   evidence30 days after capture, and exposes only consent/candidate IDs to the shop.
   Per-click keys avoid lost updates between tabs; re-read on handoff. Storage failures
   fall back to bounded memory. Withdrawal clears candidates, aborts pending fetches,
   observers/retry timers, and signals other tabs (no persistent visitor ID). Choice
   signals contain no click/customer data; a later grant must again be explicit.
8. Visible placement means >=50% intersecting continuously >=1s while document visible;
   record these fixed parameters/classifier version with that separate observation
   kind. SDK never calls a paid/order endpoint. Each observation has one event ID/time
   across bounded retries (max3 attempts, Retry-After-aware bounded backoff); new page
   events get new IDs. Bound in-flight/pending events32, clear on withdrawal/pagehide;
   no delivery promise when blocked/closed. Fetch credentials:omit, only bounded IDs,
   kind/time/consent; never collect a full URL, email, name, cart contents or API key.
   Scripts/pixels require merchant placement/consent integration; blockers may reduce
   observable loads and do not imply zero real impressions.
9. A representative merchant fixture, not a native adapter, owns a server cart cookie
   and checkout identity/time, keeps consented candidate history and calls the scoped
   API using its server-only key. Browser input cannot choose an order identity or
   establish payment. Browser journeys cover AdA thenAdB, navigation/URL cleanup,
   return visit, multiple tabs, blocked storage, withdrawal, no consent, ineligible
   latest evidence and immutable checkout. Use real browser/service/PG and local
   HTTPS merchant, retain explicit stand-in limits. Shared p05 commit wire relay may
   move to test_support with unchanged tests/imports; compare discovered names if any
   test module moves (none planned). No generic tracker framework or broker.

### Acceptance mapping (planned)

| ID/source | Actual production path and decisive assertion | Focused check plan |
| --- | --- | --- |
| AC1/06A.1–3 | Typed observation ingress never accepts sales/PII/foreign IDs, current installation/domain/company enforced, exact origin/CORS | attribution_website_event_ real HTTP+PG cases and FK constraints; event-kind labels/static contract |
| AC2/06A.4–5 | Competing exact event retries one row, conflicting409; old24h replay410 after receipt pruning; actual storage/COMMIT-loss failure503 and no GIF/204 | PostgreSQL contenders/constraint fault/wire relay; pixel with/without event ID and timestamps; GIF byte decode/header assertions |
| AC3/06A.2–3,5 | Explicit HEAD no observation; no-store GIF/POST, approved preflight actual POST cross-check, public/pertenant overload recovers | HTTP routes with real database/known/foreign/origin-less clients and competing admission |
| AC4/06B.3–4 | Store-key-only checkout, original precision/retry hash, concurrent freeze and current revocation | attribution_checkout_handoff_ real before-body auth and PG write/replay; cross-store/company/key constraints |
| AC5/06B.3–4 | Latest eligible server-time selection, inclusive30d boundary, future/expired/bot/prefetch/foreign and no consent; late invalid cannot replace eligible; immutable dimensions | Exact timestamps and distinct IDs, real production click fixtures, multiple candidates/ties; subsequent edits/GETs leave frozen result unchanged |
| AC6/06B.1–2,5 | Script no optional storage/network before consent; original query removed; same event on retry, pinned version; bounded history/expiry/withdrawal | Actual browser SDK journey, inspect emitted network requests and persisted keys; race tabs, blocked storage, viewport duration and hidden page |
| AC7/06B.3–5 | Merchant server cart holds candidates; backend generates checkout identity/time; private key absent from page/requests; frozen checkout follows latest eligible | Representative real HTTPS merchant/browser/service/PG fixture, no platform-support claim |
| AC8/shared | Accepted setup/merchant/redirect/startup boundaries retained; SQLx/migrations/offline/source boundary/contracts | p05 catalog14 and affected setup/identity/foundation cases, contract tests, focused browser; final full-suite remains p10 |

### Contract impact / traced consumers

Trail public_installation callers: only accepted p04 test currently, new observation
producer must use the same lock/approval owner. begin_merchant callers: catalog
begin/read/list/mutate and p04 revoke/store-disable/last-use tests; checkout adds one
consumer without altering auth. Click dimensions/time/flags are produced solely by
accepted catalog.redirect and consumed first by observation snapshots/checkout
selection; no current ledger/report reader. HTTP router/startup/browser service
fixtures will compose website routes and SDK once. Raw schema literals also need
exhaustive check before edits; future p07/p08/p09 consumers get AS-C005 and accepted
p06 producer requirements. Pixel timestamp amendment impacts OpenAPI/client examples,
SDK image examples and contract tests. No Openbot source/shared DB change is planned.
