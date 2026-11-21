# Attribution p09 — operations, retention and independent deployment

- Workflow implement-astra-reviewed-2-3. Root/A /root user-confirmed Sol6.1/xhigh;
  retained independent B /root/attribution_reviewer requested Astra/high, no inherited
  author context. Actual model/context telemetry unavailable. No new workers required.
- Original scope: all09A/B in 09-operations-retention-and-separate-deployment.md;
  accepted01–08 contracts and original10 combined acceptance constrain implementation.
  Whole plan authorized; API-first native adapters/real-store pilot and production
  region/retention/SLO decisions remain deferred. No commit/deployment authorized.
- Active worktree /private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree,
  detached HEAD/start/reviewbase6e55258cbbdc7633940805f366fb17fe240f02aa. Main delivery
  /Users/mac02/orca/workspaces/openbot/attribution-service. Preserve test-fix commit
  and uncommitted acceptedp01–08. Both worktrees verified at prerequisitep08 source
  99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e;
  B AS-P08-B1-N2, root AS-P08-ACCEPT-R1. No p09 implementation yet.
- Authoritative coordination is main absolute plan/attribution-service/COORDINATION.md,
  writer/root, revision24 checked at startup, C001–C007/r1 acknowledged. Proposed
  C008/r1 below will carry retention/financial retirement and operational semantics.
- Exact10 snapshot exclusions: plan/attribution-service/phases/p01/docs through
  p09/docs individually, plus plan/attribution-service/COORDINATION.md. No source,
  migration, operational docs, scripts or shared application changes excluded.
- Owned paths: service operations/retention domain+adapters, config/startup wiring,
  migration and financial read/filter owners, safe tracing, own deployment/recovery
  scripts/docs/CI. Openbot changes only if actual topology integration requires them;
  trace/record shared changes before editing. No unrelated test-fix work.
- Resources: UUID-owned PostgreSQL DBs/ports, Rust1.95.0 cache/tmp/as-attribution-target,
  Python/tmp/as-p02-contract-clean/bin/python, Playwright/tmp/as-p03-browser. Stock2MiB.
  Colima as-attribution-integration 4CPU/6GiB RAM/32GiBdata/8GiBroot/nohostmounts;
  no default context or SSH config edits. Docker socket under this namedprofile,
  DOCKER_CONFIG=/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-docker-90iaewwo.
  Root Openbot Dockerfile imagebuild as-openbot-integration:local in progress
  exec68642; p08 logs/AS-P08-openbot-image-preparation-2.txt. Current root image
  contains accepted Openbot identity changes and user test fixes. Clean only owned
  resources at finalcompletion; preserve source/review records. No runningPGfixtures.
- Status: discovery/architecture; AS-P09-D1/r1 gate required before dependent edits.
  One cohesive09A/B implementation batch after gate, one wholebatch review.

## AS-P09-D1/r1 — proposed retention and recovery ownership

1. Explicit required service retention keys (integer days1..10000):
   ATTRIBUTION_CLICK_RETENTION_DAYS, ATTRIBUTION_OBSERVATION_RETENTION_DAYS,
   ATTRIBUTION_FINANCIAL_RETENTION_DAYS, ATTRIBUTION_EVENT_RETENTION_DAYS and
   ATTRIBUTION_AUDIT_RETENTION_DAYS. Validate click>=30+eventdelivery+1,
   observations>=2, financial>=refundwindow+eventdelivery+1, event>=delivery+1.
   One-day margins cover future skew and exact cutoffs; no implicit production
   defaults. Test/example values38/30/400/8/90 for existing delivery7/refund365.
   Management receipts retain fixed48h protocolminimum; checkout receipts use click
   retention with at least48h. Tenant terminal tombstones persist indefinitely,
   a fixed safety invariant rather than a configurable ability to resurrect data.
2. Existing shared tenant row lock serializes cleanup with setup, public ingestion,
   merchant writes, settlement and reports. Cleanup does not require upstream
   freshness: retention/deletion remains safe while authority is unavailable.
   Freshness still gates serving operations. Lock order is tenant then order/event.
   SQL deadline2s and whole batch5s; row deletion batches<=100, including children.
   Never raise runtime/request/stack/lease budgets to make cleanup pass.
3. Add tenant maintenance_due_at, maintenance_failure_at and purged_at scheduling
   metadata. A short claim transaction selects<=8 due tenants ordered by due/id with
   FOR UPDATE SKIP LOCKED and advances due by60s before commit. Each owned worker
   processes each claimed tenant once periteration under its tenant lock. No external
   effects or persistent execution lease needed: the transaction owns effects, repeat
   cleanup is idempotent, and the advance-before-attempt schedule survives crash and
   prevents one poison tenant monopolizing subsequent claims. Lock timeout/exception
   records safe failure and leaves scheduled retry; cancellation rolls back in-flight
   work and preserves the committed retry schedule. Loop interval1s, successful full
   batch schedules remaining work1s later, otherwise60s. Join/cancel with existing
   startup worker owner; no detached tasks.
4. Prune observations by received_at only after retention (preserves original-event
   retry horizon). Prune checkout receipts only when old and not referenced by any
   pending/unresolved/leased merchant event; keep their referenced raw clicks while
   those checkout rows exist. Prune raw clicks by clicked_at; paid-order dimensions
   already frozen in acceptedp07 remain immutable. Terminal event envelopes may prune
   after received retention only in accepted/conflict states without execution. Pending,
   unresolved or leased envelopes and their order/refund identities cannot prune.
   Audit uses created_at; management receipts use48h created_at, whose original request
   horizon is24h. Existing ingress rejects original old timestamps before receipt lookup,
   so removal never reaccepts an old delivery. New business identities after contractual
   retention are distinct requests; no permanent business-history promise.
5. Financial retirement must not expose partial order/line/refund cleanup or perform
   unbounded FK cascades. Add nullable attribution_orders.retired_at. Under tenant/order
   ownership, mark an order retired only when greatest(received_at,paid_at) is older
   than financial retention and no pending/unresolved/executing event references it.
   All financial report/detail/list consumers exclude retired orders atomically.
   A subsequent mutation/settlement encountering a retired order fails closed. Delete
   expired order children in bounded chunks (events, refundlines, refunds, orderlines)
   and finally the empty parent; resume marked retirement after restart. Each marking
   and deletion transaction is atomic; the shared lock and statement snapshot mean a
   report sees the full retained fact or no fact, never partial money. Raw observations
   follow their own retention and need not disappear with financial retirement.
   Refund identity lasts with its parent until the validated refund+delivery horizon;
   late admitted durable work is protected regardless of wall-clock age.
6. Deleted tenant purging overrides data retention but never deletes its terminal
   fence. Existing deleted state immediately denies every read/write; bounded purge
   visits child tables in FK-safe order, deleting<=100 rows perbatch and continuing
   only once a prior child table is empty. Credential rotation self-references are
   removed in dependency order, not by an unbounded whole-tenant update. Cover events,
   refund/orderlines/facts,checkouts,observations,clicks,links,ads,campaigns,credentials,
   installations,domains,receipts,audit,stores. No server-side persisted exports exist.
   Purged tombstone keeps companyid/version/deleted state/time and minimal scheduling
   marker; clear names/freshness/reconciliation errors/claims and invalidate local
   freshness. Company-independent user sessions cannot be deleted for unrelated
   companies, but can no longer authorize the tombstoned company. Delayed provision/
   lifecycle attempts cannot recreate any purged tenant data even after restart.
7. Operational signals extend existing structured tracing, using fixed operation
   labels and internal request/event UUIDs, never URL/query/header/body/IP/customer
   values or secret-bearing debug errors. Instrument redirect success/storage failure
   and duration; event acceptance/rejection, lag, duplicate/conflicting outcomes;
   safe attribution-selection reason; pending/unresolved age/worker failures; report
   latency; admission/identity failures; lifecycle freshness; retention progress/failure.
   Signals distinguish attempted selections from committed delivery outcomes. Use
   existing shared request correlation/span and safe error mapping. No unauthenticated
   admin/metrics API or high-cardinality tenant labels. Existing scoped event diagnostics
   and owner/admin retry remain the operator repair surface. Tests capture actual logs
   across success/failure and assert seeded secret/request data absent.
8. Independent backup/restore uses standard pg_dump custom format and pg_restore into
   a fresh isolated owned role/database (no-owner/no-acl); preserve encryption keyring
   separately via the operator secret store. Never restore over a configured existing
   database or reset automatically. Real fixture must preserve paid/refund sums, key
   verification, encrypted session material, pending work and terminal fences, with
   cross-role denial and actual pending settlement on restored service. Record elapsed
   backup/restore/recovery; these are local measurements, not production RPO/RTO approval.
9. Deployment examples use the existing non-root image and actual configkeys, private
   per-app databases/roles, TLS termination, no trusted forwarded identity/IP headers,
   documented callback/authority origins and fixed timeouts. Stable public origin
   survives compute relocation. Use standard trust store verification for HTTPS;
   never add disabled certificate verification to production. Test two topologies
   with identical app artifacts: separatecontainers and separate isolated networks
   linked only by documented HTTPS identity/lifecycle/SSO routes. Deny cross-database,
   filesystem and private-port access; browser/merchant traffic uses public service
   API. This simulates separate hosts/network boundaries locally, not a production
   rollout. Actual Openbot bridge must be exercised, not only a fake authority.
10. Reuse extracted-build.py, container-smoke.py and p03 browser setup where fit.
    Extend real fixtures for full setup->merchant purchase/refund->report across the
    topology; cold restart/outage must fail user actions immediately and machine/
    public use after5minfreshness, then reconcile before recovery. Exact boundary,
    backup and TLS failure checks travel with the extracted service where possible;
    cross-application orchestration may live at root and must not become a runtime
    service dependency. Phase10 repeats the final combined acceptance/fullCI gates.

## Planned acceptance mapping — NOT EXERCISED

| ID/original | Actual production path/case | Decisive assertion / focused check |
| --- | --- | --- |
| AC1/09A.1 | shared request/business/worker tracing over real HTTP+PG success/failure | fixed operation/duration/status/lag/reason/pending signals present; seeded secrets/URL/body/IP absent; attribution_limits_ |
| AC2/09A.2–3 | config validation and actual cleanup at exact retention boundaries; ingress after dedup/receipt prune | minimum contracts reject invalid config; original delivery410 not recounted; frozen financial dimensions/report unchanged after clicks prune; attribution_retention_ |
| AC3/09A.3–4 | retirement/delete with competing real ingestion/settlement/report reads and100+ children | no partial retained report; pending/leased/unresolved protected; terminal tenant purge complete; no delayed recreation; bounded chunks and actual competing claims; attribution_retention_ |
| AC4/09A.4 | two tenants including SQL poison plus cancel/restart | second tenant progresses; cancelled batch rolls back, scheduling survives, fresh worker finishes; attribution_recovery_ |
| AC5/09A.5 | real pg_dump/pg_restore and restored production auth/commerce/report owners | independent exact ledger/key/session/pending/tombstone assertions, otherDBdenied, measured restore time; attribution_recovery_ |
| AC6/09B.1–4 | nonroot production image and extracted directory; actual Openbot plus service in2network topologies | immutable image identities, health/assets/readiness, full authorized bridge+merchant/report, deniedprivate access/TLS failures, no parent sources; deployment script/extracted gates |
| AC7/09B.5 | real authority outage/reconnect and restart | user/login fail, machine/public only within5minfreshness then503/deny; reconciled state required; attribution_deployment_ |
| AC8/shared | every retired fact reader, config/startup/script consumer, boundary and contract owners | commerce/analytics regressions, standaloneconfig/metadata/migration/offline/fmt/contract/browser; full suite onlyp10 |

## Contract impact / initial trace

Accepted p08 analytics query and API/SSR/export share one financial query; retirement
predicate goes on every o measure source, keeping raw measures independent. Commerce
reads::read_order/list are shared by API/dashboard, and settle::apply->order_lock owns
mutation checks. Raw checkout selection revalidates against raw clicks at first paid
fact; cleanup protects referenced checkout evidence and documents retention-limited
late payment attribution. Tenant lock used by setup/catalog/commerce/reports; graph
missing crossmodule edges requires literal caller verification before edits.
Existing claim worker orders event-only short claims then tenant/order settlement;
cleanup never takes event locks before tenant. Identity observes deleted terminal
state and requests purge; it must not clear completed purge state or revive fences.
Startup run owns allworker JoinSet/shutdown and configused by testfixtures/scripts,
container-smoke, browser-SSO and realOpenbot fixture. Explicitretentionkeys must update
all owned startup producers, not merely configunit tests. No tests removed/planned.

Next verify original sources/callers for all consequential decisions, publish/pin
C008proposal and AS-P09-D1/r1 source/decision, then independent B architecture gate.


### Gate assignment checkpoint

Coordination authority revision25, C001–C007/r1 active acknowledged, C008/r1 proposed.
Decision immutable AS-P09-D1-r1.md SHA256 `281e4b7cf4ac8ad7e8f7bdccd33a1887b49b6b638bebaae1e0178d2a9fa0a2de`; source remains acceptedp08.
Consumer traces saved inlogs AS-P09-read-callers/startup-callers/order-consumers/
tenant-consumers/config-producers. Literal audit adds actual tenant entry owners:
setup.user_transaction,begin_merchant,public_installation,catalog.redirect,commerce
worker.process. Orders/refunds detail/list use shared commerce reads with parent
validation under the tenant transaction; all order source queries must exclude retired.
Startup config producers include Config tests, Database::config, browser-sso.py,
container-smoke.py, config.env and README; migrate also reads Config, so update fresh
fixture scripts consistently. Existing reqwest0.13.4 rustls feature already uses
rustls-platform-verifier in both applications; no dependency/TLS bypass change is
needed to use container trust stores. Exact runtime HTTPS trust test still required.
No tests deleted/renamed, no p09source edits, all AC1–AC8 NOT EXERCISED.


### Implementation checkpoint

AS-P09-G1-N1 PASS of D1/r1 accepted by root; preimplementation snapshot equals
acceptedp08 99d5627b. Coordination revision26, C001–C008/r1 active acknowledged.
Dependent p09 implementation starts; allruntime AC1–AC8 still unexercised.
Root release image failed OOM/SIGKILL; debug actualOpenbot fixture build session72652
is separate evidence and does not prove release build. No limits raised.

### P09 B1 implementation checkpoint (2026-11-16)

Root accepted D1/r1 after B AS-P09-G1-N1 PASS; main coordination revision26 activates
C008/r1, allC001–C008 acknowledged. Authoritative source stays isolated; main delivery
remains acceptedp08. No p09 batch freeze/review/acceptance or sync yet.

Implemented retention policy/config and every known producer; bounded tenant scheduler,
owned cancellation/retry, terminal purge, financial retirement/read filters; operation
and duration correlation, committed merchant lag/reason, duplicate/conflict/backlog
signals and redirect durability outcomes; operations/deployment docs and Compose sample.
Added real PostgreSQL cutoff/race/purge/cancellation tests and real pg_dump/pg_restore.
No tests removed. Production interface retention keys require explicit input. Exact
cutoffs use transaction_timestamp and strict `<`, preserving boundary facts.
Terminal envelopes additionally protect financial retirement for their own configured
lifetime, preserving both independent retention promises. Successful nonempty cleanup
schedules another pass after1s (including short credential-chain leaf batches); idle60s.
Both refinements preserve the accepted D1 safety constraints.

Evidence (logs immutable; not yet complete acceptance manifest):
- AS-P09-retention-4.txt: six retention/config tests PASS. Earlier retained failed
  attempts were fixture timestamp ordering/private helper access corrections.
- AS-P09-retirement-race-1.txt: real report and cleanup both wait on tenant ownership;
  competing report sees full refund ledger or whole retired absence; >250 childrows
  remain bounded at100 pertransaction; PASS. This strengthens earlier sequential check.
- AS-P09-recovery-1/2: cancellation/poison/competingclaims PASS. Restore initially failed
  because SQLx URL contained libpq-unsupported statement-cache-capacity; removed that
  client-only parameter. Next restore fixture created payment after pending refund,
  correctly causing temporal conflict; corrected chronology without weakening assertion.
- AS-P09-restore-3.txt: actual independent dump191ms,restore104ms, recovered auth/ledger/
  pending331ms PASS. Exact totals2400 refunded across2 orders, restored encrypted session,
  merchant verifier, checkout, pending settlement, terminal fence, two-waycrossrole denial.
  These are local fixture measurements, not approved production objectives.
- AS-P09-signals-1.txt focusedPASS. Wider commerce regression exposed process-global
  tracing callsite interest interference; capture now executes actual journey in its
  own testprocess. AS-P09-commerce-analytics-regression-2.txt36PASS/1explicitbrowserignore.
- AS-P09-foundation-1 failed formatting only after newtestwrapper; cargo fmt applied;
  AS-P09-foundation-2 running. Metadata/migration before fmt passed. Fullsuite remainsp10.
- AS-P09-attribution-image-1.txt productionrelease/nonroot buildPASS, image
  sha256:76154a6f0260239afe4e2b987d8f83f4b0ab1d9b3310258bd1a7fd1605557d29.
  This image predates last logging changes: rebuild finalsource before finalimageproof.
- Actual Openbot debug fixtureimage buildPASS (AS-P09-openbot-fixture-build-1), image
  sha256:23fd1df4f0edbfe3a8205513523d466605ac0da2dc474844f254a2522ab46b4e.
  Rootproductionrelease build OOM remains separate failedcheck; no limitsraised.

Topology integration now authored in scripts/attribution/topology.py and servicebrowser/
topology.mjs, CI job and rootOpenbot.fixture.Dockerfile. Ephemeral trusted CA adds a
truststore-only layer to identical binaryartifacts across both layouts; browserpins
that fixturekey, standardNode HTTPSverifiesCA, productionverification remains enabled.
Modecontainers shares appnetwork with perapp privateDBs; modeseparate-networks apps use
isolated internalnetworks connected only through allowlisted HTTPSgateway. Browser runs
realOpenbot login/PKCE and merchant paid/refund/report/outage/restart. PublicDNSproof is
explicitly seeded approved state here; accepted service DNSfixture owns actualDNSproof.
Durablefreshness expiry forcedviafixture SQL; precise monotonic5min is serviceclockproof.
Current topology attempts1–3 FAIL setup; retained logs. Attempt3 actualOpenbot failed
migration because nonprivilegedrole did not own precreatedpg_stat_statements extension.
Fixture now creates that extension under its own role inside a transaction temporarily
allowingSUPERUSER and revokes it before appstart; no productionprivilegechange.
Attempt4running exec18097, logAS-P09-topologies-4.txt. Allresources invocationUUID and
finallycleaned; only taskownedColimaprofile/buildcaches survive. Need inspect failures,
complete true boundary tests, exact artifacts, servicecontainer smoke/extractedcopy,
contracts/browser regressions and rootindependent compile. No acceptance implied.


## AS-P09-B1-R1 — completed implementation, frozen whole-batch review

Resume reconciliation: all previously active commands finished. HEAD remains user test-fix
commit6e55258c; main delivery is acceptedp08 and no source from another task was overwritten.
Coordination authority revision26, C001–C008/r1 acknowledged; immutable pin inlogs.
Current source868a58789f2d34aa43c62256bd6a480c8096613ba7f2dd70b496b6d2561d2339;
exact10 exclusions unchanged (p01–p09/docs individually and COORDINATION.md).
Architecture D1/r1 remains approved. B must review actual p09 delta vs immutable p08
baseline archive; earlier phase artifacts are accepted dependencies. Full repository
and service final gates remain p10. Source frozen; no processes currently active.

### Actual acceptance mapping

| Criterion | Exercised production path and decisive assertion | Evidence / status |
| --- | --- | --- |
| AC1 | Real HTTP/PG tracing journey, isolated subprocess captures operational selection/redirect/ingress/report signals while seeded secrets/body/URL/IP remain absent | signals-2, PASS1 |
| AC2 | Exact cutoff SQL; raw evidence prunes only beyond policy; paid snapshots survive; original old deliveries fail after receipt removal | cutoffs-1, retention-4, PASS |
| AC3 | Actual maintenance vs settlement/report competitors; >100-child retirement atomically hides all financial surfaces before chunk deletion; deleted tenant remains fenced after complete purge | retention-4, retirement-race-1, PASS |
| AC4 | Competing maintenance claims, SQL-poison tenant fairness and cancelled batch rollback with durable retry schedule | recovery-2, PASS |
| AC5 | Real pg_dump/pg_restore, restored merchant verifier/encrypted session/ledger/checkout/pending settlement/tombstone, two-way role denial | restore-3, PASS1; dump191ms restore104ms recovery331ms fixture measurements |
| AC6 | Production attribution image non-root/read-only smoke; clean service copy offline/migration/assets/contracts; actual Openbot HTTPS in two network layouts with denied DB/filesystem/private ports and untrusted TLS | attribution-image-3, container-smoke-1, extracted-2, topologies-11, PASS |
| AC7 | Warm authority outage then forced durable expiry, cold restart denial and reconciled recovery in both real layouts; precise monotonic freshness separate service regressions | topologies-11, lifecycle-regression-1 PASS12 |
| AC8 | Shared config/startup, financial query/read/settlement consumers, safe identity logging, source ownership boundary, API contracts and actual dashboard | focused-remaining-1 PASS13 startup/2storage/36commerce+analytics; browser1 PASS, boundary11 PASS, contracts15 PASS, redirect regressions PASS |

No existing tests removed: retention-inventory-before/after contain identical six names.
New cutoff and restore tests are separately discovered; explicit dashboard ignore was
executed in dashboard-browser-1. Boundary false positives were resolved by extending
standard SQL syntax/catalog recognition or placing real production tests in the
correct test modules; retained 11 boundary regressions include foreign/nested SQL.
The extracted foundation run includes successful metadata/migration, formatting and
offline compilation after earlier foundation failure. It is not a full-suite claim.

### Failed attempts and material limitations

All failed fixture attempts remain retained. Image/extraction disk exhaustion was
resolved only by deleting this task's disposable caches. Final service production
image digest sha256:f4471b5d97c52e317347a25761aa4a76c4da3e27ca27550dc6f236a0a0ce4544;
Openbot fixture uses actual debug binary image sha256:23fd1df4f0edbfe3a8205513523d466605ac0da2dc474844f254a2522ab46b4e.
The earlier Openbot production Docker build OOM/SIGKILL in6GiB Colima remains FAILED
(p08 AS-P08-openbot-image-preparation-2.txt), not proven by debug artifact. No resource
bound raised. Root native independent build and full checks remain p10.

Final topology11 passes both layouts with identical base binaries. Ephemeral CA root
signs a separate server certificate with proper constraints/SAN; standard production
TLS validation remains enabled. DNS ownership is preapproved in this crossapplication
fixture (actual DNS proof in acceptedp04); freshness time is forced in SQL (exact5min
monotonic boundaries in lifecycle tests). This proves local network isolation, not
production deployment or native store support. No production retention/region/SLO
approved. Production composition and operational docs ship in extracted service.

Trail check reports lexical conflicts on pre-existing p01/p03 historical failure
records: those records explicitly retain failed build evidence, and do not mask exit
status with a later successful command. This is a false-positive conflict, not a reason
to rewrite historical evidence; carry disposition into any later commit/PR text.
Review/evidence file identities validated with check-evidence.py; no staging/commit.


## AS-P09-B1-R2 — all actionable findings corrected

B AS-P09-B1-N1 requested F1 matching current gateway-denial proof and F2 fixture
commerce chronology. The fixture control now returns database time after each preceding
click/checkout/payment effect; Node uses that for order_created_at, paid_at and refund
occurred_at. No production logic or eligibility changed. Same entrypoint/route isolation
and secret control token protect this fixture-only helper. Shared control callers still
receive ok and ignore the additional parsed result. Three changed paths: topology.py,
service browser/topology.mjs, and a retained Trail learning about fixture clock ordering.

Topology14 PASS both complete actual HTTPS layouts, including bidirectional non-bridge
HTTP403 assertions and plaintext/DB/filesystem/TLS denial. Same base app image identities
as R1. Source542b32ae822314eb5a16050b153fa4e7a625cfc7380d2b66a9298d79fbdb5d9d,
exact10 exclusions unchanged, coordination26 C001–C008/r1 still acknowledged. R2 evidence
manifest retains topology12 (daemon unavailable during restart),13 (clock skew failure),
14(PASS). All unaffected production evidence from R1 remains applicable. Recheck pending.

Final-gate preparation used only ignored dependencies and isolated resources. Task-owned
Colima build machine increased6to12GiB on32GiB host for Openbot release compilation; this
is build-environment capacity, not an application/runtime/test limit. Existing stock-stack,
lease and production-image CI gates remain. Restart revealed corruption in disposable
BuildKit history after prior disk exhaustion; only history_c8d.db was renamed aside;
images retained and same identities rechecked. Root releasebuild session77453 remains
active in logs/AS-P09-openbot-production-12g.txt. Invocation-owned PG18+pgvector/query-stat
container as-final-postgres-fb07042291b6444983511ac71f79fe92 port32768 prepared forp10;
its safe resource record AS-P09-final-postgres.json. Host PG remains untouched.


## AS-P09-ACCEPT-R1

Root accepts p09 source542b32ae822314eb5a16050b153fa4e7a625cfc7380d2b66a9298d79fbdb5d9d
after B AS-P09-B1-N2 PASS, F1/F2closed and matching root digest. C001–C008/r1
acknowledged/satisfied in owningp09 scope, coordination26 current. R1/R2 evidence
validated; historical failures retained. Guarded39path source transfer to main
verified preimages before writing; preserved user6e55258c test fixes. P09 integrated
in sequential author worktree and main delivery. P10fullgate remains; no production
deployment/commit/push. Isolatedworktree remains authoritative for p06+records.
