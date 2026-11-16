# Attribution p05 — links and durable clicks

- Workflow implement-astra-reviewed-2-3 at absolute skill path
  /Users/mac02/orca/workspaces/openbot/attribution-service/.agents/skills/implement-astra-reviewed-2-3.
  Phase p05, all ten sequential phases authorized. Root/A /root user-confirmed
  Sol6.1/xhigh. B /root/attribution_reviewer retained independent Astra/high, idle.
  No context telemetry or enforced compaction controls exposed; durable checkpoints.
- Worktree /Users/mac02/orca/workspaces/openbot/attribution-service, branch attribution-service.
  Phase start/review-base/current HEAD2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee.
  Preserve all prior uncommitted p01–p04 work and authorized root helper correction;
  Trail learnings l-20f04c5f and l-73b4ae1a remain uncommitted deliverables.
- Prerequisite p04 accepted a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275,
  B AS-P04-B1-N2 and root AS-P04-ACCEPT-R1. Earlier accepted identities in coordination.
- Authoritative coordination /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md,
  writer/root revision8 checked. Acknowledge AS-C001/r1 API-first/Openbot login,
  AS-C002/r1 approved permission/consent/no-IP/30-day/freshness/503 defaults, and
  AS-C003/r1 accepted setup/domain/credential transaction boundaries. No questions.
- Record root /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p05/docs.
  Exact snapshot exclusions: plan/attribution-service/phases/p01/docs,
  plan/attribution-service/phases/p02/docs, plan/attribution-service/phases/p03/docs,
  plan/attribution-service/phases/p04/docs, plan/attribution-service/phases/p05/docs,
  plan/attribution-service/COORDINATION.md. No deliverable/source exclusions.
- Originals: plan/attribution-service/05-links-and-durable-clicks.md and service
  contract README/OpenAPI. One cohesive phase05A–B batch. Schema/history, shared
  receipts (user versus merchant principal), commit-before-redirect and concurrency
  decisions need an architecture gate before dependent implementation.
- Scope: services/attribution domain/application/catalog, persistence/HTTP/UI,
  migration, focused real-PostgreSQL/browser tests; own CI/docs where needed.
  Openbot no click ownership/routes. Native adapters and production deployment deferred.
- Resources: /tmp/as-attribution-target, invocation-owned UUID PG/DNS/browser fixtures;
  Python3.13, /tmp/as-p02-contract-clean, /tmp/as-p03-browser Playwright. No running
  fixture processes. Preserve root p03 owned database/cache for later final checks.
- Status discovery/architecture; no p05 source changes, source digest or gate yet.
  Next: trace accepted merchant/user/receipt/domain and global HTTP boundaries,
  record criterion/assertion and consumer impact maps, propose AS-P05-D1/r1 for B.
  Only original05/06 text and p05 OpenAPI shapes have been read so far; p06–p10 wait.

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

Coordination revision9 checked for gate: C001/r1 C002/r1 C003/r1 acknowledged;
C004/r1 proposed, no dependent source edits.


Gate AS-P05-G1 assigned to retained independent B. Decision AS-P05-D1/r1 hash
c65c8f5653783df03d83a871adaf8ecb163e1f44fd1a07880df93523b49529b5;
source remains accepted p04 a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275.
Start/review-base/HEAD and exact exclusions are unchanged from the p05 header.
Coordination revision9 pinned in logs/AS-P05-D1-coordination-r9.md. C004/r1 remains
proposed until gate acceptance; no dependent source edits. All owned fixtures stopped.

AS-P05-D1/r1 PASSED independent gate AS-P05-G1-N1, decision hash unchanged.
Coordination revision10 activates AS-C004/r1; p05 acknowledges all four active
notices. Source implementation may now proceed; no runtime/phase acceptance yet.


## Implementation checkpoint — shared receipt owner

AS-P05-D1/r1 gate PASS received before edits. Coordination revision10 active;
AS-C004/r1 acknowledged. Added application/ids.rs nonnil typed-ID macro; setup uses
it unchanged. Added adapters/management.rs ActorKind(user/merchant), Actor and
Receipt replay/record owner. Setup mutations now call that owner; original-time,
secret metadata, authority/transaction order unchanged. Initial migration receipts
and audits now require principal_kind/actor_kind; no backfill. Shared transaction
alias/error mapping lives in adapters/postgres.rs. Test-only merchant audit insert
was updated with the explicit merchant actor kind. Existing p04 exact-replay,
expiry, secret and competitor tests all pass against freshly migrated owned DBs:
AS-P05-shared-setup-1.txt,18 PASS; browser explicitly ignored in that focused command.
This is focused intermediate implementation proof, not independent batch acceptance.

No catalog schema/models/routes/UI, AdUpdate wire amendment, public redirect,
classifier, commit-loss proxy or phase05-specific tests implemented yet. Next: build
these on the accepted gate, retain p04 tests, then one whole-phase independent review.
No currently running fixtures/commands. Entire implementation remains uncommitted.


Added catalog application models/typed campaign/ad/link/click IDs, normalized
commands, distinct absent-versus-clear advertised product representation and
classifier flags/version. Initial schema now has scoped campaigns/ads/links and
immutable click dimensions/time/classification. Shared destination owner now
checks canonical serialized2048 bytes. Two pure catalog domain tests pass in
AS-P05-catalog-domain-1.txt. The new schema has not yet been freshly migrated/tested
since this addition; SQL/HTTP/UI/proxy/runtime work still pending.


AdUpdate OpenAPI PATCH schema extension is now applied, with a contract regression
for omitted/set/clear product reference and invalid null/name/bounds. First test
harness invocation used the validator helper with a schema object rather than its
expected schema name (AS-P05-contract-1.txt FAIL); corrected helper call and explicit
route-ref assertion now pass all13 contract tests (AS-P05-contract-2.txt). Runtime
AdUpdate handler still pending. No production effect is claimed by schema tests.

## AS-P05-B1 / implementation and review freeze

Coordination revision11 checked and pinned in logs/AS-P05-B1-coordination-r11.md;
AS-C001/r1–AS-C004/r1 remain active and acknowledged. All p05 source work is now
implemented; independent batch acceptance remains pending. Source is frozen for B
at a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d,
start/review-base/HEAD2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee; exact exclusions
remain the six paths in the header. Snapshot retained in logs/AS-P05-B1-R1.snapshot.txt.
Snapshot on this macOS host needs PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:$PATH;
the initial invocation with BSD realpath failed before hashing. No source change.

Implemented the gated schema/models and shared receipt/actor owner, catalog SQL,
user/merchant HTTP routes, exact domain and parent eligibility, immutable database
click snapshots, explicit HEAD, bounded classifier/admission and commit-before302.
The existing setup JS owns catalog forms, paginated lists, edits/disable and retry
identity. AdUpdate's nullable/omitted product semantics are exercised through real
HTTP edits. The existing p04 browser test retains its name and extends the journey;
no test was removed or moved. Test-only PostgreSQL protocol relay observes real
CommandComplete(COMMIT) before dropping/withholding it, with no production failpoint.

### Executed acceptance mapping

| IDs | Actual tests / decisive assertions | Current evidence/status |
| --- | --- | --- |
| AC1, AC2 | attribution_links_http_roles_scope_retry_and_competing_claimants: competing same key yields201+200, same response,2 total links and exactly1 new audit/receipt; changed time/payload409; member write403/read succeeds; encrypted cursor cannot change resource/filter. attribution_links_existing_foreign_dimensions_never_cross_scope: existing valid foreign IDs404; composite FK rejected. attribution_links_merchant_scope_principal_collision_and_revoke_before_replay: equal user/key UUIDs have distinct effects/receipt kinds, key cannot cross store/company or use campaign routes; real TCP headers401 before body; revoked-key replay401. | AS-P05-catalog-tests-5.txt PASS |
| AC3, AC4 | attribution_links_destination_and_parent_eligibility plus attribution_redirect_headers_head_classification_and_immutable_history: percent-decoded reserved key400, wrong host404, nonHTTPS400; active domain/parents rechecked; production campaign/ad/product/link PATCH cannot change original snapshot; explicit null clears product; newer click stores revision2 and changed names/product; final disable410 retains rows. | AS-P05-catalog-tests-5.txt PASS |
| AC5 | headers_head_classification_and_immutable_history: HEAD204/no Location/no row, unknown404,302/no-store/no-referrer, duplicate query values/fragment preserved; raw/bot/prefetch flags/version separate; concurrent_distinct_ids_and_latency_budget:101 committed rows/101 IDs/one scoped company. No raw IP/UA/referrer columns or writes exist (inspection). | AS-P05-catalog-tests-5.txt PASS |
| AC6 | storage_rejection_and_freshness_fail_closed: actual CHECK rejection503/no Location/no row; unknown_commit_outcome_retains_row_without_location: observed real COMMIT, independent SQL sees1 row, HTTP503/no Location/Retry-After, new GET becomes second row; client_disconnect_after_real_commit_is_durable: real TCP consumer drops after observed commit, row remains and next GET is distinct. | AS-P05-catalog-tests-5.txt PASS |
| AC7 | competing_disable_serializes_both_orders: pg_blocking_pids proves production contenders are waiting in both orders, only redirect-first commits a click. public_admission_competing_requests_and_recovery:24 real handler contenders against occupied pool prove16 public slots,429 and recovery/health. tenant_admission_competing_requests_and_recovery:8 actual tenant writes wait on lock, ninth429, release gives8 committed clicks then recovery. Durable stale company503; authoritative refresh of deleted state403. | AS-P05-catalog-tests-5.txt PASS |
| AC8 | attribution_setup_browser_real_forms_and_postgres:12 browser assertions retain setup behavior and add catalog forms/escaping, lost-response retry with exact key/time/body, product edit, actual302 navigation to local HTTPS merchant, member read-only. SQL confirms one click and product P2. Real TCP100 warm redirects at concurrency8: p50=6ms,p95=29ms,p99=44ms; p95<1000ms assertion. | AS-P05-browser-3.txt and catalog-tests-5 PASS |
| AC9 | p04 setup18 (14 HTTP+3 DNS/admission+1 normalization), p03 identity21, foundation14; contracts13; fresh migrations, SQLx prepare --check, format/offline all-targets, boundary checker and its tests. | setup-2/setup-domain-1/setup-normalization-1, identity-1, foundation-2, contract-3 PASS; exact filenames in manifest |

All commands run in services/attribution unless noted; CARGO_TARGET_DIR=/tmp/as-attribution-target,
Rust1.95.0, RUST_MIN_STACK=2097152 from service config. Fixture admin is local
maintenance PostgreSQL; only invocation-created UUID as_test_/as_verify_ resources
are cleaned. Browser uses installed Playwright via ATTRIBUTION_PLAYWRIGHT_MODULE
and ephemeral local certificate/server; requires openssl. Use PYTHON=python3.13 for
verify.sh on this host; default system Python3.9 lacks tomllib. Contract interpreter
/tmp/as-p02-contract-clean/bin/python. Source/fixture obligations for p06–p10 remain
unimplemented, including checkout/commerce/reports/retention and full-plan verification.
SQLx macro SQL did not change; new dynamic SQL is exercised against fresh migrations.

Retained intermediate failures: catalog-tests-1 (test Origin incorrectly had a
trailing slash); catalog-tests-2 (test expected expired durable company status to
refresh despite a still-live monotonic ticket, corrected to invoke real refresh);
tests-build-2 (test helper referenced an undeclared stream crate, replaced with
real TCP header-before-body proof); browser-1/2 (Playwright destination routing did
not intercept redirect navigation, replaced by actual local HTTPS target);
foundation-1 (system Python3.9/tomllib environment; corrected using documented3.13).
None was skipped or weakened. Final decisive assertions retain the original criteria.
The browser failure and chosen fixture are saved as learning l-70a31dc0. Existing
stable-select-label learning was reused for dynamic campaign/ad selects.
Trail graph refreshed; AS-P05-trail-check-1.txt reports no conflicts. No worker or
fixture is running before B assignment. No user approval is pending. Root and
service full-suite/standalone/container combined gates remain p10, not claimed here.

## AS-P05-ACCEPT-R1 — accepted in phase

B returned explicit AS-P05-B1-N1 PASS with no actionable findings for frozen
artifact a03abfeaf183952cbbf5c178939823e441b9355fcc2e151618527ee2d18d1a4d.
Root independently recomputed the same digest (logs/AS-P05-ACCEPT-R1.snapshot.txt)
and checked coordination revision11 with all four notice revisions unchanged.
Acceptance manifest logs/AS-P05-ACCEPT-R1.evidence.json validates12 PASS entries,
including B's independent14 catalog,18 shared setup and browser12-check journey.
No acceptance-blocking p05 finding or verification gap remains. Accepted in p05;
not full-plan or production deployment acceptance. No source commit performed.
All owned fixtures stopped; B idle and retained. Proceed to p06 using this accepted
producer; p06–p10 must satisfy their own gates and final integration verification.
