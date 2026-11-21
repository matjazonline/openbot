# Attribution p06 — website observations and checkout handoff

- Workflow implement-astra-reviewed-2-3; skill directory
  /Users/mac02/orca/workspaces/openbot/attribution-service/.agents/skills/implement-astra-reviewed-2-3.
  Root/A /root user-confirmed Sol6.1/xhigh; retained independent B
  /root/attribution_reviewer Astra/high, idle after p05 PASS. Host compaction/context
  telemetry unavailable; durable records, no claimed configuration change.
- Phase p06, whole06A–B cohesive batch, all ten sequential phases authorized.
  Worktree /Users/mac02/orca/workspaces/openbot/attribution-service, branch attribution-service.
  Start/review-base/current HEAD2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee.
  Preserve all uncommitted p01–p05/root changes and three Trail learnings.
- Prerequisite p05 accepted a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d,
  B AS-P05-B1-N1 and root AS-P05-ACCEPT-R1; exact accepted source remains current.
- Authoritative coordination /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md,
  writer/root revision12 checked. Acknowledge C001/r1 API-first and Openbot SSO,
  C002/r1 explicit consent/no-IP/30-day/freshness/503, C003/r1 approved destination/
  installation/merchant transaction ownership, C004/r1 click snapshot semantics.
- Record root /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p06/docs.
  Exact snapshot exclusions: plan/attribution-service/phases/p01/docs,
  plan/attribution-service/phases/p02/docs, plan/attribution-service/phases/p03/docs,
  plan/attribution-service/phases/p04/docs, plan/attribution-service/phases/p05/docs,
  plan/attribution-service/phases/p06/docs, plan/attribution-service/COORDINATION.md.
  GNU snapshot tools: PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:$PATH.
- Originals: plan/attribution-service/06-website-tracking-and-checkout-handoff.md,
  services/attribution/contract/README.md, attribution.openapi.json Observation,
  CheckoutWrite/Checkout/AttributionReason and pixel/events/checkouts operations.
- Owned: service website domain/SQL/HTTP, schema, SDK asset/docs/examples and real
  PG/browser merchant fixture; shared module/router/test-helper owners if needed.
  No native store adapter, production deployment or real-store pilot authorized.
- Resources: service Rust1.95.0, /tmp/as-attribution-target; UUID PG/DNS/HTTP/HTTPS
  fixtures; local maintenance PostgreSQL; Python3.13; contract env
  /tmp/as-p02-contract-clean; Playwright /tmp/as-p03-browser/node_modules/playwright/index.mjs;
  openssl ephemeral browser certificate. No currently active fixtures. Preserve
  existing root p03 DB/cache for later combined/full checks.
- Status: discovery and architecture, no p06 source edits. Next: gate AS-P06-D1/r1
  before dependent schema/ingestion/SDK work. p07–p10 await accepted predecessors.

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

Gate AS-P06-G1 assigned to retained independent B for decision AS-P06-D1/r1,
immutable hash62324766f8ea1a6140bc8b448e510b5f75d57a414b859963124e34a3db9f8364.
Coordination revision13 pinned, C001–C004 active/acknowledged, C005/r1 proposed.
Source remains accepted p05 a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d
under exact seven exclusions; logs/AS-P06-G1.snapshot.txt. No dependent source edit
or runtime claim; original source and decision frozen for B. Exhaustive click-table
references retained in logs/AS-P06-click-consumers.txt confirm only catalog producer
and p05 tests; future phases have no existing readers. No owned process active.

Gate clarification revisionr2 hash4dbef90e5d7e2534198e4cb61d2f8b4f8f2f2e18f9d3433c460bad28f689492a: in response to B, explicitly skip landing events with no captured/consented retained click (wire schema unchanged); clarify no browser page URL collection versus existing approved link-destination dimensions. No source or consequential contract changes. r1 retained; B reviews currentr2.

AS-P06-G1-N1 PASS received for AS-P06-D1/r2 hash4dbef90e5d7e2534198e4cb61d2f8b4f8f2f2e18f9d3433c460bad28f689492a. Root checked source still acceptedp05 and coordination14; revision15 activatesC005/r1. P06 acknowledges all five notices. Dependent implementation begins; no runtime/phase acceptance.

## Implementation checkpoint — observation/checkout paths and SDK

Gated AS-P06-D1/r2 implementation now exists in application/website.rs,
adapters/website/{http,postgres}.rs, initial schema and embedded sdk-1.0.0.js.
Website routes compose in the existing service router. Catalog snapshot projection
and serialization now have one shared owner for redirects and link-only observations;
existing14 p05 tests pass unchanged in AS-P06-shared-catalog-1.txt (fresh migration).
OpenAPI pixel observed_at conditional requirement and24h protocol horizon descriptions
are amended. No p06 implementation review/digest/acceptance yet.

Initial offline all-target compilation PASS in AS-P06-initial-check-1.txt. Eight new
real PG/HTTP tests PASS in AS-P06-website-tests-2.txt: strict kind/scope/origin/consent,
competing observation retries, pixel/GIF/HEAD/storage failure, current revocation,
public admission, checkout exact30d/nanosecond precision/ties/ineligible candidates,
concurrent immutable checkout and real postcommit acknowledgement loss for both
pixel and checkout. A test-only unused Row import warning was removed afterward.
Website tests reuse p05 Catalog fixture/protocol relay through pub(super) test-only
visibility, without moving/deleting tests. Shared setup/identity/foundation/contracts
and final required p06 checks still pending after complete batch.

The actual browser merchant fixture is in browser/website.mjs, invoked by ignored
attribution_checkout_handoff_browser_sdk_cart_and_freeze. It owns an HTTPS merchant
server, cart cookie and server-only API key; backend generates checkout identity/time.
First run AS-P06-browser-1.txt is in progress (unified exec session31999), with a
120s fixture deadline. Do not claim it passes until output is observed. SDK/browser
coverage and docs/contract regressions/CI wiring remain unfinished. No reviewer
currently running; B idle after architecture pass.


## AS-P06-B1 implementation complete, pending independent review

Coordination authority rechecked at revision15; notices C001–C005/r1 acknowledged
and implemented for p06. Revision16 records review readiness without contract changes.
Full original06A–B batch is implemented. No native platform or production-policy claim.
WEBSITE.md contains installation, SRI, consent/withdrawal, observed-load meanings,
retry horizons, server-controlled cart/checkout and stand-in limitations. README
links it; contract tests pin SDK integrity; CI explicitly runs syntax/browser journey.
No tests moved/deleted; existing catalog helpers only gained test-module visibility.

| Criterion | Executed production check and decisive assertion | Retained output |
| --- | --- | --- |
| AC1 | strict_types_scope_origins_and_cors rejects sale/unknown/PII/duplicate/invalid fields; exact approved preflight/CORS, mismatched link and foreign references rejected. existing_cross_store_evidence_rejected uses real second store click and returns no foreign detail. current_installation_domain_and_company_before_replay denies disabled/suspended scope. | AS-P06-website-tests-4.txt: 11 PASS, browser separately |
| AC2 | competing_retries_conflict_and_pruned_horizon proves one row under concurrent identical posts, changed409, old410 after pruning; pixel_durable_gif_head_and_retry_timestamp verifies GIF bytes, no-store, no-ID distinct rows, required original timestamp and constraint fault503; unknown_commit_outcomes_replay_once loses real PG COMMIT acknowledgement and verifies one committed row/exact replay for pixel and checkout. | AS-P06-website-tests-4.txt |
| AC3 | public_admission_competitors_release_and_health holds pool and competes actual handlers, observes429 then successful recovery/cheap health; HEAD no row and explicit204; competing_installation_disable_serializes_both_orders observes actual PG blocking for both orders and only observation-first commits. | AS-P06-website-tests-4.txt |
| AC4 | headers_authenticate_before_body_and_limits sends incomplete TCP body with invalid key and receives401 before bytes; 33 IDs/129-character ID400 and oversized413; no last-use committed. latest_eligible_immutable_concurrent_and_scope proves exact frozen concurrent result, conflicting409, key revocation blocks replay, old creation410 after pruning. | AS-P06-website-tests-4.txt |
| AC5 | exact_window_precision_reasons_and_ties exercises inclusive30d/nanosecond originals, future/expired/bot/prefetch/no-consent/missing/foreign and deterministic tie. Immutable checkout retains original dimensions despite changed click evidence; eligible history survives later invalid candidates in actual browser journey. | AS-P06-website-tests-4.txt, AS-P06-browser-4.txt |
| AC6 | Real Chromium: zero storage/events before explicit consent; only ob_click removed; original event ID/body across committed-response-loss retry; less50%/less1s/hidden branch no visibility row, continuous50%/1s produces1; two tabs merge/withdraw, storage blocked uses memory,31d expiry and32 bound, withdrawal cancels503 retries. Complete event headers prove no auth/cookie/referrer information; synthetic email in page URL never emitted. | AS-P06-browser-4.txt: 1 PASS; 11 emitted attempts |
| AC7 | Actual local HTTPS merchant owns HttpOnly cart cookie and server-only key; two simultaneous checkout calls freeze one server identity/time; forged browser checkout identity rejected400; AdA thenB selectsB, late invalids preserve eligible result. Fixture uses simulated identity authority; real Openbot SSO was acceptedp03, native shops unproved/deferred. | AS-P06-browser-4.txt |
| AC8 | Existing catalog14 PASS unchanged; identity/setup35 PASS (setup browser ignored, unchanged); foundation14 PASS, fresh owned migration, SQLx metadata, fmt/offline compilation, source boundary plus10 checker tests;15 contracts PASS including SRI and pixel clock. Full service/root/extraction/container gates remain p10. | AS-P06-shared-catalog-1.txt; AS-P06-shared-identity-setup-1.txt; AS-P06-foundation-1.txt; AS-P06-contract-1.txt |

Commands run in service cwd with Rust1.95.0, CARGO_TARGET_DIR=/tmp/as-attribution-target,
stock2MiB stacks, local maintenance PostgreSQL and invocation-owned UUID resources.
Browser uses /tmp/as-p03-browser/node_modules/playwright/index.mjs, Node/Chromium,
openssl local certificate; all owned fixture processes/databases cleaned on completion.
Exact check commands and hashes are in round manifest. No production credentials/logs.

Retained failures: AS-P06-website-tests-3.txt was test-only wrong type name
InstallationUpdate (existing mutation uses EnabledUpdate), corrected before -4 PASS.
AS-P06-browser-3.txt failed after all functional stages because browser request metadata
used an empty referrer string for no-referrer; -4 checks absence of information and
uses allHeaders() so credentials/cookies are not hidden by Playwright headers().
Earlier -1/-2 browser and -1/-2 website outputs are intermediate passing coverage,
not substitutes for the completed11+1 proof. No assertion about privacy was removed.
Syntax and diff checks PASS, trail check has no conflicts (lexical advisory matches
are unrelated prior-phase/other-branch learnings); graph refreshed. No new source
edit after snapshot freeze is allowed until B returns. Review/acceptance outstanding.

Frozen AS-P06-B1-R1 digest 0f0023424964d9a62ee5a6439de67d1193a7eba606b19d2efd7c500cac914747 at HEAD2df562d6; snapshot and evidence manifest under logs/. Seven exclusions unchanged. B review to follow.

## Isolated continuation after AS-P06-B1-N1

B findings F1/F2 OPEN. Current author/reviewer record authority is this worktree /var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree, detached HEAD2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee. Original main coordination authority unchanged, revision17, C001–C005/r1 acknowledged. User confirmed separate test-fixing task in main; preserve it. Transfer inventory/hashes logs/AS-P06-author-transfer.json. No commits or acceptance; source is unfrozen for F1/F2 correction. Same seven relative snapshot exclusions. Main original p06 records are historical pointers; all future p06 evidence writes here. Shared service cache /tmp/as-attribution-target, isolated UUID PG fixtures unchanged.


## AS-P06-B1-R2 corrections and isolation

F1: historyEntries now prunes only valid keys sampled by that read, not a fresh
storage enumeration. A newly written candidate absent from the read survives;
returned handoffs remain <=32, and each writer's following read enforces storage
pruning as concurrent operations finish. No async API/lock/cache or policy change.
WEBSITE clarifies concurrent pruning and updates pinned SDK SRI. Actual production
browser regression pauses tabA by CDP after reading/before cleanup, grants tabB's
new production click, resumesA, then navigatesB and verifies both persisted IDs.
This assertion is after navigation and cannot be satisfied byB's old in-memory map.
Full browser journey PASS AS-P06-browser-5.txt, still one discoverable browser test.

F2: observe/preflight retain the public permit acquisition result while validating
supplied Origin. The result is enforced before body reads/ingestion, then its429
uses the same exact-origin CORS wrapper. Preflight method/header rejections for a
verified origin are wrapped too. The response-policy read is still subject to the
existing global32 request admission/deadline; public ingestion remains16 and tenant8.
Unknown/duplicate origins and unavailable validation fail closed without ACAO.
The inherited global admission rejection occurs before any origin is established;
this correction does not add unbounded lookups outside that common admission layer.
New competing HTTP regression holds16 real requests at body polling, uses actual
HTTP100-continue to prove admission, then checks known-origin POST/HEAD/OPTIONS429,
readable Retry-After, exactACAO/Vary/no credentials, unknown-origin403/noACAO,
zero observations during overload, and successful recovery. Earlier poll-based
fixture temporarily stole a slot while competitors arrived; replaced by protocol
readiness, not relaxed limits.12 PG/HTTP PASS AS-P06-website-tests-7.txt.

Retained corrective failures: website-tests-5 compile error from async closure
borrowing non-Sync request fixed with synchronous preflight validation; website-tests-6
had11 PASS/new overload readiness assertion FAIL; cors-overload-1 and website-tests-7
prove corrected fixture. No production limit raised. Format/offline all-targets PASS
AS-P06-check-2.txt; contracts15 including SRI PASS AS-P06-contract-2.txt; syntax/diff
PASS. Previous catalog14/identity-setup35/foundation14 evidence unchanged and retained.

User confirmed external test-fixing task. Main source/runner work preserved. Active
isolated worktree and record authority are identified in revision17/transfer manifest.
Five owned correction files synced coherently to main under byte-hash comparison;
no external runner edit copied or overwritten. Recheck artifact uses whole isolated
worktree under same seven exclusions; final combined integration will reconcile
other task's test fixes. A new Trail learning records F1's otherwise hidden cause.

Frozen correction artifact AS-P06-B1-R2 c5f2ac256934858ba3b656f9158657d3160436b2188ee2027c555a52cdd438e6. Both findings corrected, new12 HTTP/PG and1 browser PASS,15 contracts and offline all-target/fmt PASS. B correction review pending, no acceptance. Source frozen in isolated worktree; no active checks/processes. Same seven exclusions; main coordination authority revision17 pinned.


## AS-P06-B1-R3 — complete F1 read/validation/eviction correction

B AS-P06-B1-N2 closedF2 and independently reproduced a secondF1 interleaving:
reader sampled time before another tab wrote, then classified the new entry as
future during validation/selection. Now each stored item is validated against a
clock read AFTER reading that item, and retention samples its clock AFTER completing
the storage read. Thus all legitimate concurrently read writes precede the clocks
used to validate/select them; expiry/future-input rejection remains intact. Sampled
keys still bound eviction to entries considered by the read. No protocol/gate change.

Actual production service/PG/Chromium journey now runs TWO isolated competing-tab
cases: pause before enumeration/validation (after the old clock sample), writeB,
resumeA, then navigateB; and pause after read/before cleanup, writeB, resumeA, then
navigateB. Both prove persistedA+B with only2 valid candidates. The existing expiry
and32-bound assertions remain; an additional one-day-future entry is rejected and
removed. Browser AS-P06-browser-6.txt PASS; contracts15/SRI AS-P06-contract-3.txt PASS.
No Rust source/HTTP/config/schema changes afterR2; reuse R2 website12, offline/fmt
and unaffected earlier shared checks. F1 remains OPEN until B rechecks.

User now reports test fixes committed/pushed. Main HEAD6e55258c (9 runner/CI/docs/
memory-container files) confirmed. Isolated p06 remains atHEAD2df562d6 for this frozen
review; after p06 acceptance advance isolated base to that existing commit with
our uncommitted source preserved, and include its combined effects at finalp10.
No claim that p06 has accepted that independent test-fix commit. Main coordination
revision17 unchanged; no active p06 tests/processes after checks.

Frozen AS-P06-B1-R3 fd2a235453db086980456eaf541657997df46111ddd476632bba09c8cd3a2991. F2 closed byB/N2; F1 complete read-clock/eviction correction now awaits recheck. Expanded two-schedule production browser PASS (browser-6),15 contracts/SRI PASS (contract-3); unaffectedR2 checks retained. Source unchanged except SDK/browser/WEBSITE afterR2. Same isolated authority, maincoord17/seven exclusions; no running fixtures.

## Root acceptance AS-P06-ACCEPT-R1

Accepted p06 at fd2a235453db086980456eaf541657997df46111ddd476632bba09c8cd3a2991 after independent AS-P06-B1-N3 PASS, F1/F2 fixed. Root recomputed exact digest and byte-compared current coordination17 pin, validates retained evidence. No required p06 gap remains. All original06 scope implemented; complete-plan/full-suite/native-production claims remain excluded. P07 may begin after registration and its own architecture gate. User test-fix commit6e55258c is next base integration; no additional source/phase acceptance implied by p06.
