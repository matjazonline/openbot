# Attribution p04 — merchant setup, domains and credentials

- Workflow implement-astra-reviewed-2-3, skill absolute
  /Users/mac02/orca/workspaces/openbot/attribution-service/.agents/skills/implement-astra-reviewed-2-3.
- Phase p04, all ten phases authorized sequentially. Root/A /root user-confirmed
  Sol6.1/xhigh; B /root/attribution_reviewer independent Astra/high, idle after p03,
  reused when gate ready. Context/model telemetry unavailable; no context/compaction
  controls exposed by current tools; host defaults and durable checkpoints retained.
- Worktree /Users/mac02/orca/workspaces/openbot/attribution-service, branch attribution-service.
  Phase start/review-base/current HEAD2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee.
  Preserve all sequential uncommitted p01–p03 deliverables, including authorized
  reconciliation helper visibility correction and scoped Trail learning l-20f04c5f.
- Prerequisite p03 accepted fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c,
  B AS-P03-B1-N2, root AS-P03-ACCEPT-R1. p01/p02 identities in coordination.
- Authoritative coordination /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md,
  writer/root revision5 checked. AS-C001/r1 API-first/Openbot login and AS-C002/r1
  approved permissions/consent/no raw IP/30-day attribution/five-minute freshness/503
  acknowledged. Production region/retention/load targets not approved deployment policy.
- Record root /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p04/docs.
  Exact snapshot exclusions: plan/attribution-service/phases/p01/docs,
  plan/attribution-service/phases/p02/docs, plan/attribution-service/phases/p03/docs,
  plan/attribution-service/phases/p04/docs, plan/attribution-service/COORDINATION.md.
- Scope originals: plan/attribution-service/04-merchant-setup-and-api-credentials.md,
  accepted contract/README.md and attribution.openapi.json under services/attribution.
  Cohesive whole phase04A–04B batch; schema/domain proof/credential/idempotency/lock
  decisions require one architecture gate before dependent implementation.
- Owned source: services/attribution only (domain/application/setup adapters, migration,
  own HTTP/UI/config/fixtures/docs); root consumer edits only if traced integration
  requires them. No store-native adapters or signing providers. No implementation yet.
- Resources: service cache /tmp/as-attribution-target, invocation-owned as_test_*/as_verify_*
  PostgreSQL fixture resources; Python3.13, contract env /tmp/as-p02-contract-clean.
  Root cache/owned p03 database retained for affected/final checks; not reset for p04.
- Status: discovery/architecture. No p04 source digest/review/acceptance. p05–p10 wait.
  Next: trace accepted session/tenant/HTTP boundaries, map acceptance and consumer
  contracts, record AS-P04-D1/r1 then obtain B gate before implementation.

## AS-P04-D1 / r1 — proposed architecture gate

One cohesive p04A–B batch; no dependent source edits before B PASS. Original
requirements are phase04 and the accepted network/OpenAPI contracts, not this design.

1. Extend attribution's initial migration (reset-only) with stores, exact domains,
   installations, merchant credentials, management receipts and an audit log. Each
   child has company/store predicates and composite foreign keys; domain-to-installation
   ownership is enforced in SQL. Store/installation disablement, domain revocation,
   credential revocation and lifecycle all serialize on the existing tenant row.
   No schema touches Openbot. Audit records contain trusted actor/company, action,
   resource UUID and time only, never bearer/verifier/challenge/payload values.
2. Typed setup commands perform bounded normalization in application code. The SQL
   adapter owns transactions. After live session company/permission and CSRF checks,
   every read/write obtains `lock_tenant` with the freshness ticket. Writes recheck
   local session expiry/revocation under a session-row lock (tenant then session),
   so a completed local logout cannot authorize a later commit. Membership authority
   is the live Openbot observation at request entry; there is no distributed lock.
   No network request occurs while a tenant transaction is held. Management
   mutations, audit row and receipt commit together; uncertainty returns 503.
3. Receipt identity is (company, user, method + concrete path, UUID key), bound to
   original timestamp and SHA-256 of normalized typed JSON. Validate the timestamp
   both before work and after acquiring the tenant lock using database wall time;
   >30s future is 400 and >24h old is 410. Compare an existing receipt first for
   changed timestamp/payload (409), within the accepted horizon. Receipt JSON is
   metadata only; creation/rotation return the new secret only to the first committed
   invocation. Replays always return 200 with the declared CredentialReplay recovery
   instructions; they never mint a key or extend overlap. Keep receipts >=48h;
   pruning is deferred to p09 and cannot relax the original-time bound.
4. DNS TXT proof is `_attribution.<canonical-host>` with 256 random bits in a
   public challenge. Use a maintained DNS resolver with fixed public recursive
   servers, no OS search suffixes and one bounded attempt inside a 2s total timeout.
   It queries DNS only; never fetch an HTTP destination or use customer-supplied
   resolvers. Proof also requires nonempty A/AAAA answers containing only globally
   routable addresses. No custom DNS packet implementation. Unit/local-DNS fixture
   adapters prove transport and validation without a public-network dependency.
   Pending/expired domains can verify or reverify against their stable challenge;
   approval expires after 30 days. Revoked records are terminal; re-add produces a
   new domain/challenge and requires a new installation. Unique live host per store.
   Verification obtains proof outside the transaction then locks and rechecks row
   state/challenge before approval. Replay of an accepted verification needs no DNS.
   Proof failures are safe 400, resolver failure/timeout 503; no failed receipt.
5. Canonical host validation owns IDNA and exact match. Accept DNS names only, with
   public-looking labels; reject IP literals, single labels, localhost and reserved
   local/special suffixes, empty labels, trailing dot and percent/userinfo confusion.
   Limit canonical host to 240 bytes so the TXT record name fits 253. Destination
   parsing requires HTTPS, no credentials, no explicit non-443 port, a canonical
   host and a currently approved exact domain for that company/store. DNS proof
   screens local addresses at approval; redirects never fetch the destination, so
   DNS changes cannot induce service-side SSRF. Domain expiry/revocation is checked
   at every dependent use. Common destination policy is consumed by p05/p06.
6. Keys use existing OS-random 32-byte and SHA-256 helpers: `ak_` +43 base64url
   characters; only verifier stored. Five typed scopes from the contract. Credentials
   belong to exactly one enabled store/company. Rotation clones label/scopes and
   original expiry into a new key and clips old expiry to min(original expiry,
   DB-now + overlap[0..86400]); only an unrevoked/unexpired/unrotated credential can
   rotate. Keep rotated-to ID internally; an exact retry is a receipt replay.
   Revoking the new key never changes old expiry. Creation expiry must be future.
7. A merchant authentication adapter accepts exactly one syntactically valid bearer
   header, rejects session cookies and conflicting company/store selectors, and
   verifies scope and local credential before body consumption. It refreshes company
   authority only when freshness requires. The trusted transaction entry re-locks
   tenant then credential and rechecks expiry/revocation/store/scope and ticket;
   last-use update and downstream mutation commit together. Revocation uses the same
   lock: an operation already holding it may commit first; after revocation commits,
   no later use commits. No commerce/report endpoints are prematurely added in p04;
   tests exercise the actual transaction entry with an isolated write assertion.
8. Bounded per-tenant concurrent admission (8 requests; bounded registry tied to
   active permits, 1024 tenants) follows authentication and precedes payload read;
   global 64-request/10s boundary stays in force. The production merchant pre-body
   adapter is exercised by a test-only route. Subsequent phases must use it, not
   duplicate credential logic. Public IDs expose only installation selection,
   allowed observation types (landing/viewport/image) and exact domain; they never
   satisfy merchant authentication or session authorization.
9. Lists use UUID keyset order, default25/max100, optional store filter, and encrypted
   opaque cursors bound to company/list-kind/filter; reuse the attribution cipher
   with distinct purpose. UI uses the existing session shell and external JS/CSS,
   explicit labels/status/error regions and text nodes; no privileged secret in
   markup/public snippets. API-key secret appears once after explicit creation in a
   private response panel and is cleared on navigation. UI exposes pagination,
   store create/edit/enable, domain add/proof/verify/revoke, installation create/enable,
   credential create/list/rotate/revoke. Members get read-only controls plus server
   enforcement; all mutations use one stable key/time per submission/retry.

### Acceptance mapping (planned; execution pending)

| ID / source | Production path and discriminating case | Decisive assertion / focused proof |
| --- | --- | --- |
| AC1 /04A.1,3 | Store/domain/install reads and writes, actual PostgreSQL | Foreign company/store IDs are 404; SQL composite FK cannot bypass ownership; public IDs fail key/session auth |
| AC2 /04A.1–2 | DNS verifier + destination validator | Real local DNS transport fixture proves TXT, all-address policy and timeout; Unicode exact host succeeds; suffix, unsafe URL/local IP, expired/revoked domain fail; no HTTP fetch |
| AC3 /04A.4 | Browser setup through HTTP with live-authority fixture | Keyboard labels/status, escaped names, all setup actions, read-only member, CSRF rejection, one-time secret absent from listing/public snippet |
| AC4 /04B.1–3 | Key SQL/HTTP create/list/rotate/revoke | Hash-only DB plus receipts/audit/log safe; secret syntax/entropy; scopes, expiry, overlap and last use; replay metadata only, no overlap extension |
| AC5 /04B.4–5 | Actual merchant auth/transaction, before body | Missing/invalid/expired/revoked/public/session/service values rejected before invalid/oversized body; wrong scope403; foreign selectors404; current suspension/staleness blocks |
| AC6 / retry contract | Concurrent production setup commands against PG | One resource/audit/receipt/secret response; identical replay200; changed data/time409; future400/old410 even after receipt removal; authority rechecked before replay |
| AC7 / concurrency | Competing key use/revoke, verify/revoke and store disable/use | Lock ordering determines one valid serial outcome; waiter rechecks state after lock and no post-revocation acceptance |
| AC8 / boundaries | Existing identity/foundation/boundary/contract suites | P03 shared failure/router/shell behavior retained; max-page Unicode JSON remains bounded; offline compile, fmt, fresh migration and SQLx metadata |

Run focused `attribution_installation_`, `attribution_destination_`,
`attribution_credentials_` tests plus setup retry/HTTP tests and existing identity
tests affected by router/error/shell changes. Browser proof uses fixture-owned
authority and actual local service/PG; it does not establish live external DNS or
production Openbot deployment. Final whole-plan suites/container gate remain p10.

### Contract impact and traced consumers

| Value/interface | Producers, consumers, wrappers | Disposition and proof |
| --- | --- | --- |
| `IdentityService::company`, `verify_csrf`, `TenantPrincipal` | identity HTTP shell/session; new setup HTTP | Reuse live authority; no permission changes. Session/local commit recheck added only in setup transaction; existing identity HTTP suite retained |
| `lock_tenant`, status freshness | lifecycle SQL, reconciliation, setup and future merchant writes | Reuse same row and lock order; real competing writers and p03 lifecycle tests |
| HTTP router/state/error mapper | startup, identity fixture, foundation shutdown tests | Add setup composition and Gone410; existing global limits/security/errors retained; focused foundation tests |
| Cipher/random/verifier | identity sessions, setup cursors and keys | Reuse maintained primitives with separate purpose; no key material/session schema changes |
| New persisted stores/domains/keys/receipts | setup SQL/UI; p05–p09 future consumers | Composite scope and transaction entry published to later phases after gate via coordination; no Openbot source changes |
| Initial migration | migrate, fresh DB fixtures, SQLx prepare | Reset only owned fixtures; fresh migration and metadata check; deployment remains unapproved |
| Setup wire contract | OpenAPI, API handlers, JS | Preserve declared routes/status/schema, 16KiB, bounded lists and original-time retries; runtime response validation and browser proof |

Discovery: Trail symbol/caller/literal passes inspected actual identity HTTP,
tenant SQL and startup composition. Ambiguous caller edges were supplemented by
literal references; no existing setup/merchant owner exists. No tests removed.

## Implementation checkpoint

AS-P04-D1/r1 PASS by B AS-P04-G1-N1 (decision hash unchanged). Source edits began
after PASS. Initial offline compilation found three adapter type errors (Lookup
iteration API, Arc<Cipher> ownership); corrections in progress. No runtime proof or
phase acceptance yet. Merchant/setup source, migration and UI now authored, tests
next. Existing p03 snapshot remains historical acceptance, not current source.


Coordination revision6 checked: AS-C001/r1, AS-C002/r1 unchanged; new AS-C003/r1
publishes the gated setup boundary to p05–p10, effective only after accepted p04
implementation. The G1 pinned revision5 remains historical evidence. SQL timestamp
storage uses canonical UTC text in receipts to retain original sub-microsecond
precision rather than rounding it in PostgreSQL's timestamptz type; expiry checks
still use database time. A/AAAA lookups are independent so a partial DNS timeout
cannot pass approval. These preserve the gated invariants without changing contracts.

Current focused setup proof: AS-P04-setup-2.txt, 14 PASS. Includes DNS transport,
competing receipts, competing revoke/use, verification/revoke while DNS awaits,
strict HTTP/CSRF/roles, metadata-only key replay and no per-event authority call.
Browser R1 failed fixture cookie shape before navigation (fixed domain/path).
R2 found a real fast-click race during post-save refresh: UI now disables action
buttons through refresh, including dynamically rendered actions. Explicit [hidden]
CSS keeps read-only forms hidden despite form display styles. R3 pending.


## AS-P04-B1 — implementation complete, preparing independent review

Current coordination revision6; original applicable AS-C001/r1 and AS-C002/r1
satisfied within p04 scope; AS-C003/r1 publishes the gated boundary for future phases.
Root/A authored all source, fixtures and corrections. B remains independent and idle
until freeze. No source accepted yet. No user approval or production action pending.

Final proof mapping replaces the planned "execution pending" status above:

| Criterion | Actual decisive production checks | Evidence/status |
| --- | --- | --- |
| AC1 | scope_keysets_and_foreign_constraints (valid foreign company/store and SQL FK); destination_verification_expiry_revoke_and_exact_scope; credentials_auth_classes_scope_expiry_store_and_company | AS-P04-setup-4.txt PASS |
| AC2 | exact_idna_and_unsafe_urls (includes 253-byte challenge boundary); dns_transport_all_addresses_and_timeout; special_ranges_and_public_addresses; verification_expiry_revoke_and_exact_scope; competing_verification_and_revocation | AS-P04-setup-4.txt PASS |
| AC3 | attribution_setup_browser_real_forms_and_postgres (8 actual browser checks), installation_http_roles_csrf_strict_json_and_limits | AS-P04-browser-5.txt + setup-4 PASS |
| AC4 | once_replay_rotation_revocation_and_hash_only; expiry_overlap_and_expired_metadata_replay; browser secret/rotation controls | setup-4 + browser-5 PASS |
| AC5 | header_auth_precedes_body_and_freshness_fails_closed (real auth/transaction, fixture side effect), auth_classes_scope_expiry_store_and_company; no authority calls for fresh merchant request | setup-4 PASS; fixture does not prove future commerce route wiring |
| AC6 | concurrent_receipts_conflicts_and_pruned_horizon; once_replay_rotation_revocation_and_hash_only; logout_and_authority_recheck_before_replay | setup-4 PASS; one row/audit/receipt and one secret response |
| AC7 | competing_revoke_and_use_serializes_both_orders; competing_verification_and_revocation; existing lifecycle waiting-writer tests | setup-4 + AS-P04-identity-1.txt PASS |
| AC8 | runtime_shapes_and_rejected_nullable_payloads; 21 p03 identity cases; 14 foundation; fresh migrate/SQLx prepare/check; format/offline all targets; boundaries10; contracts12 | logs AS-P04-{setup-4,identity-1,foundation-1,prepare-1,format-1,offline-1,boundary-1,boundary-tests-1,contract-1}.txt PASS |

16 focused setup tests pass, plus one separately run browser test. No tests removed;
new tests are discoverable through the existing identity fixture child and setup
module roots. Test-only ingestion side effect uses actual merchant authentication,
last-use/transaction boundaries, but is not a commerce implementation. Future real
routes must satisfy AS-C003 and owning p05–p08 tests. Browser fixture owns loopback
HTTP/DNS processes and isolated PG; all stopped/dropped after each invocation.

Failure dispositions: initial type/fixture compile failures retained in build-2 and
compile-tests-1; superseded by offline-1 and executed tests. Browser1 cookie fixture
was invalid before navigation, fixed by explicit cookie domain/path. Browser2–4
failed at exact-label domain selection. The earlier R2 note identifying a refresh
race as the failure cause was premature: action disabling did not resolve the
failure. Browser4 located the exact select-label mismatch; explicit label for/id
fixed it without weakening the locator (browser5 PASS). Action disabling remains
because send() otherwise ignored another action while a mutation was busy. CSS
[hidden] now wins over form/label display rules. New Trail learning l-73b4ae1a retains
this discovery; existing l-20f04c5f maximum-JSON response guard remains unchanged.

Reviewed r1 described "64-request" global admission; actual pre-existing boundary
is 64 connections and 32 concurrent non-health requests. Implementation retains
those original tighter limits unchanged, plus the gated tenant bound. This is a
record correction, not a changed runtime budget. Root source is unchanged except
CI's explicit service-browser check; no Openbot identity contract or code changed.
Whole-plan final suites, container and real-store pilot disposition remain p10.

Frozen AS-P04-B1: HEAD/review base2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee, digest 6837fba3f8f66fb0160bf308ecc0668d25f33be5a70ad3b65a2895fbedea0c0f.
Exact exclusions unchanged from TASK header. Manifest logs/AS-P04-B1.evidence.json.
Source frozen for B; applicable coordination pinned logs/AS-P04-B1-coordination-r6.md.


## AS-P04-B1-R2 — corrections for F1 and F2

B AS-P04-B1-N1 required two verification corrections. No production implementation
changed; no architecture revision needed. New actual tests in setup/http_tests.rs:

- F1: attribution_credentials_competing_store_disable_and_use_recheck_both_orders.
  PostgreSQL pg_blocking_pids establishes actual waiting in both orders. When use
  holds the tenant transaction, production UpdateStore waits and commits afterward.
  For disable-first, a session-row fixture lock pauses production UpdateStore only
  after it holds the tenant row; actual begin_merchant then demonstrably waits behind
  it. After release, disable commits, the waiting use is Forbidden and last_used_at
  stays unchanged. AS-P04-R2-F1.txt:1 PASS; replaces AC7's missing store competitor.
- F2: attribution_credentials_replay_after_original_payload_expiry. Issue with the
  original expiry two seconds in the future, retain key/time/body/receipt unchanged,
  wait for the database clock to cross that exact time (five-second fixture bound),
  then exact replay returns only metadata and the original credential ID. A new
  operation using the expired payload returns Invalid; one key and one credential
  receipt remain. No row/clock manipulation. AS-P04-R2-F2.txt:1 PASS; establishes
  the original-payload boundary for AC4/AC6. The earlier manual-row-expiry test remains
  coverage of its separate persisted-state/rotation case, not proof of this boundary.

Both new tests pass individually; aggregate setup run pending. No tests removed.
All prior production/browser/identity/foundation evidence remains unchanged and
eligible for reuse. Formatting and offline all-target compilation pass with the new
fixtures. Original phase04 execution count will be18 after aggregate confirmation.
Coordination revision6 and applicable notices unchanged, no new source contract.

Aggregate AS-P04-R2-setup.txt:18 PASS plus explicit browser ignore (browser5
previously PASS, unchanged). Freeze R2 digest a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275;
base/HEAD/exclusions unchanged. Manifest logs/AS-P04-B1-R2.evidence.json.


## Root acceptance AS-P04-ACCEPT-R1

Accepted p04 in phase at a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275.
B explicit PASS AS-P04-B1-N2, F1/F2 resolved in B-owned findings; root independently
recomputed matching digest and validated13 current retained PASS entries in
logs/AS-P04-ACCEPT-R1.evidence.json. Coordination revision6 matched pinned requirements
before acceptance. AS-C001/r1+AS-C002/r1 satisfied for p04; AS-C003/r1 now backed by
this accepted producer artifact. Exact base/HEAD/exclusions unchanged. All disposable
fixtures are stopped; no production actions/commits/deployments. Source remains
uncommitted sequential work. No current whole-plan/container/full-suite acceptance.
Next authorized work is p05 registration, architecture review and implementation.
