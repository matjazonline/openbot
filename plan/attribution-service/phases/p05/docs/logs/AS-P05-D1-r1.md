## AS-P05-D1 / r1 — proposed architecture gate

One cohesive phase05A–B. No dependent source edits before B PASS. Original05 and
accepted network contract remain the requirements; changes below are scoped to
implementing their required behavior. Proposed notice AS-C004/r1 records the one
wire extension and shared receipt adjustment before dependent edits.

1. Extend the initial reset-only schema with campaigns, ads, links and immutable
   clicks. All belong to company/store with composite parent foreign keys. Campaign
   and ad enabled flags participate in link eligibility, as do store, domain,
   company and link status. Link IDs, ad IDs, campaign IDs and per-request click IDs
   are distinct UUID wrappers. Share the existing nonnil ID macro through a small
   application IDs module; existing identity authority IDs keep their behavior.
   Link code is random18 bytes/base64url24 characters, unique and stable. Do not
   derive it from ad IDs, names or counters. Updates increment checked bigint link
   revision; exact receipt replay does not increment it again.
2. Reuse p04 tenant locking, user_transaction (live user authority at request entry
   and local session lock), merchant pre-body auth and begin_merchant. User reads
   need Read; writes need ManageLinks; campaign/ad routes accept sessions only.
   Link read/write keys require links:read/links:write and use only their own store.
   Conflicting body/query/company IDs fail404 without foreign details. Shared tenant
   admission is reused; no new company/credential authorization model or port.
3. Extract the existing management receipt/audit SQL into one adapter helper used
   by setup and catalog. Add a principal-kind discriminator (user/merchant) to the
   receipt primary key and audit identity; UUID equality across credential/user
   classes cannot alias a receipt. The caller supplies an authorized transaction,
   company, typed principal, canonical operation, original retry identity and typed
   normalized fingerprint. Preserve UTC timestamp precision, 24h/30s horizon,
   >=48h retention, authority-before-replay and receipt/effect/audit atomicity.
   All existing setup consumers and tests move to this same owner; no dual paths.
   Secret receipts retain p04's metadata-only behavior. No backfill/compatibility.
4. Campaign/ads retain current names; a click snapshots company/store/campaign/ad/
   link IDs, campaign/ad names, advertised product reference, placement, canonical
   destination and link revision while holding the shared tenant lock. Later edits
   or disablement cannot alter prior click evidence. Required product metadata
   editing is absent from the accepted Ad PATCH schema, which currently reuses
   DimensionUpdate(name,enabled). Add AdUpdate(name,enabled,advertised_product_id),
   all optional with minProperties1, product nullable and bounded as in AdWrite;
   omitted product means unchanged, explicit null clears it. Campaign update stays
   DimensionUpdate. This implements original05A.4 with a real production mutation;
   a direct-SQL stand-in would not prove its history obligation. No new endpoint,
   permission, integration vendor or product-management ownership is added.
5. Normalize and validate destinations before fingerprinting/persistence through
   the p04 owner. Add a canonical serialized2048-byte guard (Unicode URL encoding
   can expand the input) and reject any decoded query key exactly ob_click. Domain
   check is same-company/store and exact-host, active at create/update and redirect.
   Existing URL syntax/IDNA/HTTPS/userinfo/port/local policy is unchanged. Append
   ob_click via url's query serializer, preserving existing unrelated keys, duplicate
   values and fragments. Validate final Location before COMMIT so a formatting/header
   error cannot create an undisclosed click. No destination fetch.
6. GET /r/{code}: bounded syntax and public admission, preliminary lookup only to
   find local company/store, then freshness via existing identity.tenant, tenant
   admission, begin PostgreSQL transaction, lock_tenant, re-read all active scoped
   parents/domain/link and prepare canonical Location. Generate a fresh ClickId,
   assign database clock_timestamp and frozen dimensions/classification, insert,
   COMMIT, then return302/no-store/Location with existing security headers. No302 on
   insert failure or commit uncertainty; all such failures503/Retry-After. Ordinary
   repeated GETs always create separate raw clicks; no fingerprint/IP dedupe.
   HEAD uses the same validity checks but produces204 and no row/Location; explicitly
   register it so Axum's implicit GET-to-HEAD cannot insert. Unknown code404, disabled
   link/parent410 (company permission/freshness retains403/503), invalid destination
   or domain nonredirect error. GET has no mutation retry identity.
7. Store only is_bot/is_prefetch booleans and classifier_version1; no raw IP,
   user-agent or referrer. Inspect at most512 User-Agent bytes for a small documented
   bot/scanner/preview token set and bounded Purpose/Sec-Purpose prefetch signals.
   Missing/unknown signals remain raw/heuristic-eligible; no assertion of human
   identity. Both exclusion flags remain separately observable for p06–p08. Ignore
   forwarded client IP headers rather than introducing a proxy trust configuration.
8. Existing64-connection/32-request/10s HTTP and pool/statement/lock bounds remain.
   Add a16-slot public redirect admission semaphore before database work, plus the
   existing8-per-tenant admission after scope resolution. The semaphore is held
   through response construction; saturation429/Retry-After. No unbounded queued
   tasks or spawned click workers; cancelled precommit work rolls back, committed
   work remains durable. Health retains its existing cheap independent path.
9. Extend attribution's own company/store UI with accessible campaign/ad/link
   create/list/edit/disable forms, opaque tracking URL display and pagination. Use
   current session/CSRF boundary and stable key/time/body across a retry. Share the
   existing setup JS mutation/error/selection ownership where it fits instead of
   a second competing request coordinator; no privileged key in public markup.
   Product-reference edits use AdUpdate; click snapshots remain read-only persistence
   evidence until reporting's p08 UI. Browser proof reaches real forms/API/PG/DNS,
   then follows a tracking redirect to a fixture destination without claiming a
   real-store adapter or an external production deployment.

### Acceptance and decisive proof mapping (planned, not yet exercised)

| ID / source | Production behavior / case | Decisive focused check |
| --- | --- | --- |
| AC1 /05A.1–2 | Full campaign/ad/link HTTP+PG paths, current user/key permissions, parent scope | attribution_links_ cases with valid foreign company/store/ad, key scopes/public IDs, composite FK violations and cursor scope/filter binding |
| AC2 /05A.2 + retries | Concurrent setup/catalog retries, user versus merchant identity | One link/revision/audit/receipt effect; same UUID in different principal classes cannot alias; payload/time conflict; expired/pruned horizon; revoked key denied before replay; p04 receipt/secret tests retained |
| AC3 /05A.3 | Active domain and canonical destination at create/update/redirect | Exact host, revoked/expired approval, Unicode expansion bound, percent-encoded ob_click rejection, duplicate query values and fragment preservation through actual302 |
| AC4 /05A.4 | Production campaign/ad/product/link edits racing click | Earlier click retains original dimensions/product/destination/revision; later click captures current values; competing disable and redirect serialize; no direct-SQL history substitute |
| AC5 /05B.1–3 | Actual public GET and explicit HEAD | One committed unique UUID row for each concurrent GET;302 only after commit; HEAD204/no Location/no rows; no-store/security; bot/prefetch flags/version and no raw private headers persisted |
| AC6 /05B.4 | Real PG/HTTP failure paths | Storage rejection503/no Location/no row; wire proxy suppresses actual successful COMMIT acknowledgement:503/no Location while DB retains one row; separate GET produces another ID. Client disconnect after observed database commit leaves that row |
| AC7 /05B.5 | Public/global/tenant admission and deadlines | Actual competing requests saturate bounded slots with429 and recover; blocked scoped write cannot use a completed disable; current freshness blocks public use503; no forwarded-IP authority |
| AC8 /UI + performance | Browser create/edit/click + isolated redirect baseline | Accessible labels/keyboard/text escaping/read-only permissions/stable retries;100 warm-cache redirects at concurrency8 report p50/p95/p99, fixture p95<1000ms. This is an implementation regression budget, not an approved production SLO |
| AC9 / shared contracts | p04 receipt helper + startup/router + schema changes | p04 setup18 and browser, p03 identity21, affected foundation, contracts incl AdUpdate, format/offline, fresh migrations/SQLx; required whole-plan gates remain p10 |

Failure fixtures use only invocation-owned databases/ports/processes. For commit
uncertainty, a test-only PostgreSQL wire proxy observes a completed COMMIT and drops
or withholds its acknowledgement; no production failpoint is added. The disconnect
fixture drops the HTTP consumer only after observing that database commit, then
checks durable rows through an independent fixture connection. A proxy returning
an error before COMMIT would not prove these cases. Tests have explicit deadlines,
join owned tasks and do not reset configured/shared databases.

### Contract-impact and traced consumer dispositions

| Changed value/owner | Producers/consumers/wrappers | Disposition and checks |
| --- | --- | --- |
| Receipt/audit principal discriminator and extraction | setup.mutate/receipt, new catalog mutations, p04 receipt/secret tests and test-only merchant audit effect | Move all existing consumers together; preserve normalized payload/time/secret rules; test cross-class UUID collision and old p04 contract |
| Shared IDs and destination serialized bound | setup typed IDs/destination, approved_destination, new catalog create/update/redirect | Keep nonnil/wire semantics; add expansion/reserved-key assertions; existing destination tests retained |
| Merchant/user authorization transactions | setup HTTP, merchant auth/begin_merchant/user_transaction, catalog keys/sessions | Extend consumers without changing permissions/freshness/session/revocation implementation; real full-route pre-body and replay tests |
| AdUpdate public schema | attribution.openapi.json, application model, SQL update, UI, contract tests; p06–p10 dimensions | Proposed AS-C004; real product edit plus immutable click snapshot test; no change to Openbot wire contract |
| HTTP state/router/startup and shell assets | standalone serve, identity/setup fixtures, shutdown/foundation, browser | Reuse SetupHttp's accepted state/resources; compose catalog routes once, explicit HEAD; focused foundation and both service browsers |
| Immutable click schema | redirect producer; p06 checkout/observations, p07 ledger and p08 reports future readers | Publish producer ID/time/snapshot/classifier meanings through coordination after gate; later phases need accepted p05 implementation |

Discovery used Trail callers for receipt, user_transaction and approved_destination,
then exhaustive stored-table/SQL literal references (including test-only audits).
Current repository has no catalog/click owner to reuse; the p04 lock/domain/credential
and receipt implementations are the responsible shared owners. No tests removed.
