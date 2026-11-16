# Attribution p08 — analytics and independent dashboard

- Workflow implement-astra-reviewed-2-3. Root/A /root user-confirmed Sol6.1/xhigh;
  retained independent B /root/attribution_reviewer requested Astra/high, independent
  initial context. Actual model/context telemetry unavailable; no setting claims.
- Authorized scope: complete original08A/B in 08-analytics-and-attribution-dashboard.md,
  contract README/OpenAPI ReportGroup/FinancialTotals/Analytics and analytics.json.
  Original09/10 downstream semantics and C001–C006 remain constraints. No phase9/10
  implementation until dependencies accepted. No production deployment authorized.
- Active isolated worktree /private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree, detachedHEAD/start/review-base6e55258cbbdc7633940805f366fb17fe240f02aa.
  Main delivery workspace /Users/mac02/orca/workspaces/openbot/attribution-service.
  Preserve user test-fix commit6e55258c and all uncommitted p01–p07 source/records.
- Prerequisite p07 accepted2712efab34ac16d2a2d368b4d83909d04b57b6e93eaf0740ef8262d6fbfd091d,
  B AS-P07-B1-N1/root AS-P07-ACCEPT-R1,20 PG commerce tests + browser + contracts;
  earlier phase identities in coordination. Not yet transferred to main workspace.
- Coordination authority /Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md,
  writer/root revision21 checked, C001–C006/r1 acknowledged: API-first/Openbot SSO,
  roles/consent/noIP/window/freshness/503, shared setup authorization/receipts,
  immutable click and checkout evidence, commerce horizons/facts and safe diagnostics.
- Owned: service application/analytics.rs, adapters/analytics, report assets and
  navigation; shared management authorization owner, commerce read visibility,
  router and contract/docs/fixtures. Index changes only if representative plans need.
- Exact nine snapshot exclusions: p01/docs through p08/docs individually under
  plan/attribution-service/phases, plus plan/attribution-service/COORDINATION.md.
  No source/deliverable exclusions. Skill location original main .agents/skills/
  implement-astra-reviewed-2-3. Resource/cache/PG/Playwright identity same as p07,
  UUID-owned databases/ports, /tmp/as-attribution-target Rust1.95.0, Python3.13 venv
  /tmp/as-p02-contract-clean/bin/python; PYTHONDONTWRITEBYTECODE=1. No active process.
- Status: discovery/architecture only; no p08 source edits. Gate AS-P08-D1/r1 needed
  for shared report semantics, cursor contract and shared authorization ownership.
  Retain p07 known unrelated session fixture failure for p10 owning correction.

## AS-P08-D1/r1 — proposed cohesive reporting decision

1. Application query validates explicit RFC3339 start/end, start<end and elapsed
   duration<=93 days; IANA zone via installed chrono-tz, optional store/ad/campaign/
   advertised-product/sold-product/currency, enum grouping (none/day/ad/campaign/
   advertised_product/sold_product), default25/max100 pages. Strict unknown/duplicate
   query rejection, distinct IDs, normalized UTC/timezone/filters before cursor hash.
   Scope every current UUID filter through company/merchant-store ownership; existing
   foreign objects404 even for actors legitimately in both companies. External product
   references are scoped strings, never global object lookups.
2. Reuse current tenant/user/merchant entry/locks. Move real shared Authorized enum
   and company/filter/actor/begin methods from catalog persistence to management owner;
   keep catalog resource permission rules in catalog, add a reusable exact permission/
   key-scope check for report reads. No new service/interface. User requires liveRead,
   merchant requires analytics:read; public IDs cannot read. Merchant query store can
   only equal key's store and last use commits with successful report. Auth precedes
   query processing where current adapters do it. Recheck in transaction.
3. One PostgreSQL statement with independently aggregated clicks, observations,
   paid-order counts/reasons, purchased lines, refund facts and refund lines joined
   only after aggregation by group/currency. Each SQL statement sees one MVCC snapshot.
   Existing tenant lock orders it against accepted producers; no cross-database data.
   Raw click=every redirect, eligible excludes bot OR prefetch. Observations counted by
   accepted kind. Use frozen click/observation/order dimensions, never live catalog
   names/product values for historical grouping. Paid order distinct before lines;
   refund distinct before refund lines; cancellation/pending/conflict facts excluded.
4. Clocks: click.clicked_at, observation.observed_at, order.paid_at, refund.occurred_at;
   all half-open[start,end). Refund joins order dimensions without restricting order
   paid time to the report interval, so period net merchandise may be negative.
   Day grouping uses UTC instant AT TIME ZONE validated zone. Never sum currencies.
   Conversion numerator is attributed paid orders across selected currency filter;
   denominator is filtered eligible clicks once per group (not per currency).
   Rate may exceed1; zero denominator=>null; no clamping or fake zero.
5. Sold-product filter/grouping uses only matching line money/units and counts each
   matching order/refund once in that group. Advertised-product filtering remains
   independent against frozen ad dimensions. With sold-product filtering/grouping,
   omit rate (null plus omitted_sold_product_filter). Click/observation counts reflect
   selected company/store/ad/campaign/advertised-product/time scope, not a sold product;
   for sold-product groups the same contextual counts can repeat and UI/export labels
   explicitly forbid adding them. Tax/shipping stay order/refund-level totals counted
   once per matching fact, because merchant contract does not allocate these to lines.
   Product groups can overlap in order/refund counts and tax/shipping; never sum them
   to overall totals. Overview is its own ungrouped backend aggregation, not detail sums.
6. Group key uses deterministic C collation with null first. Opaque AEAD keyset cursor
   binds normalized query hash including derived company/store and last group key;
   limit changes may vary page size only. Max128-character Unicode product key cannot
   always fit old512-byte cursor; set analytics request/response cursor bound1024 only,
   leave other cursors512. Worst-case128 four-byte Unicode key test must produce<=1024
   and resume exactly; reject1025. This is a public contract correction, not an excuse
   to remove the bound. Query canonical hash avoids carrying every filter in the cursor.
7. SET LOCAL statement_timeout='2s' inside report transaction plus existing request/
   tenant admission/time limits. SQL sums use exact PostgreSQL numeric/bigint conversion;
   reject any aggregate outside published ±9e12 report amount/count contract, never
   clamp/wrap/round. User can narrow filters/range. No preaggregation or new database.
   Examine EXPLAIN ANALYZE on fixed test data (10k clicks,1k orders,1k observations,
   representative refunds); test command/latencies/query plans retained. Two-second
   report budget is implementation regression target, not production SLO approval.
8. Add GET /v1/analytics/export with same session/key scope and normalized filters,
   no pagination bypass. Produce CSV with total10000 data-row cap, same2s query bound;
   stop with explicit invalid-request when too large, no partial successful export.
   Quote/escape CSV and neutralize untrusted text formula prefixes; numeric negative
   amounts remain numeric. Group click/order/reason/rate fields emitted once per group,
   not repeated per currency row. No persisted export files or new queue. Export's
   contract/limits travel with service. Existing README already promises export bounds.
9. Independent service-rendered /reports pages reuse the same report owner as API.
   Use ordinary GET form navigation to cancel superseded reads through browser document
   navigation; avoid a second asynchronous report-state engine. Local date fields convert
   selected zone's midnight to UTC server-side; earliest instant for ambiguous midnight,
   nonexistent midnight=>explicit error. Display actual UTC half-open range and localzone.
   Overview, group selection/table pagination, links to paid-order/refund detail and
   pending/conflict/unresolved events; own setup/link/key UI remains existing shell.
   Reuse commerce scoped read owners, widening crate visibility only, no duplicated SQL.
   Reports link from active-company shell; company selection/account management remain
   existing links. Escape every dynamic text/attribute; responsive tables, captions,
   focus and labels, empty/error states. Detail lists are bounded; never derive totals
   from visible rows. Minimal JS only for existing retry API, CSRF/idempotency metadata;
   no cancellation of writes by report navigation. Retry state remains server durable.
10. UI labels estimated image loads, continuous visible observations, heuristic click
    filtering, missing impressions/blocked tracking, distinct clocks, currency, discount
    merchandise and separate tax/shipping, negative period net, nullable/uncapped rate
    formula, advertised versus sold products and nonadditive product groups. Diagnostics
    expose accepted safe EventResult only; no raw envelope, credentials or customer PII.
    Installation instructions via current setup/WEBSITE docs with key server-only.
11. Publish C007/r1 for p08/p09/p10: report query clock/filter/group semantics, bounded
    analytics cursor/export, preserved financial/dimension consumers. P09 cleanup must
    not expose partially pruned facts to reports and must keep frozen report dimensions;
    no claim that raw counts survive their approved retention. P10 verifies combined UI,
    merchant scoping, export and retention effects.

## Acceptance mapping (planned; NOT EXERCISED)

| ID/source | Production case and decisive assertion | Focused check |
| --- | --- | --- |
| AC1/08A.1,shared | live user and analytics key, exact store derivation, foreign ad/store/campaign and cursor/filter attacks with actual multi-company member; validation/range/zone; no public credential authority | attribution_analytics_scope_ HTTP+PG |
| AC2/08A.2–5 | fixed two-company/two-store/two-ad, multi-product paid/unattributed/four reasons/two currency/refund ledger; independent arithmetic on gross/refund/net/count/units; no join amplification | attribution_analytics_ledger_ |
| AC3/08A.3–5 | exact half-open/DST23h25h, refund-only period negative; zero clicks null and rate>1; product sold vs advertised independent; once-per-group denominators | attribution_analytics_clocks_ and product tests |
| AC4/08A.1,6 | keyset all groups including null/128 four-byte key, filter replay rejection, page size100; bounded export10000 vs10001; query timeout, numeric overflow reject; EXPLAIN/index/representative latency | attribution_analytics_bounds_ and export/plan fixtures |
| AC5/08B.1–4 | actual browser overview/group/form, order/refund/event diagnostics, management-role controls/retry CSRF, HTML/CSV escaping, same company totals match API, tenant switching, navigation cancels stale read not writes; setup/account links | attribution_dashboard_ Chromium/HTTP/PG |
| AC6/shared | shared Authorized catalog/user/key dispositions preserved, immutable ledger unchanged, standalone embedded routes/assets, contracts and offline/fmt/migrations | catalog/commerce regression families, foundation/contracts; full suitep10 |

## Contract impact / traced owners

render_shell<-shell owns current company/setup navigation; only add Reports link.
HTTP router merges setup/catalog/website and optionalcommerce; report router can reuse
SetupHttp state without another config container, included whenever setup is present.
Authorized exhaustive occurrences: catalog postgres read/list/mutate/apply, catalog HTTP,
and one actual competing-disable fixture. Move shared owner atomically; preserve exact
resource/permission/key side effects. Commerce read_order<-manage, list<-manage (graph
ambiguous list; literal references verified) widen only for own dashboard consumer.
Observation dimensions already frozen by website observe; raw clicks and paid order
snapshots from accepted p05–p07. Existing time indexes cover scoped snapshots/financial
clocks; measured plan determines any additional index. Domain/report schema, CSV and
SSR are new consumers of same persisted meanings. No identity grant schema changes.
No tests to remove/rename. Repo source graph used before source reads/caller changes.

AS-P08-G1-N1 PASS of D1/r1 SHA25669dcc8f4b01b904e3b89352ba1c9d2b498baef6740e7b145ebda6c56651b6a2f received. Root checked frozen source2712efab/HEAD and coordination22. Coordination23 activatesC007/r1; p08 acknowledgesC001–C007/r1. Architecture accepted; dependent implementation begins. All runtime evidence outstanding.


## Implementation checkpoint 2026-11-16

D1/r1 accepted AS-P08-G1-N1, coordination23/C007 active. Implemented domain
analytics Query/Filter, clocks/midnight, bounded aggregates and typed response;
shared Authorized moved from catalog postgres to adapters/management (actual two
consumers; catalog keeps own resource restrictions). New adapters/analytics query
uses UNION ALL of independently owned measures then aggregation/page join, equivalent
to independently grouped measures without any entity fan-out. One statement MVCC,
exact numeric sums, nullable/uncapped rate, contextual sold-product counts and
scope-bound1024-byte AEAD cursor. API /v1/analytics and CSV export wired via setup
state; report deadline2s, CSV10000 rows and formula-safe text, no repeated currency
row denominator. Compile/check all-targets passed AS-P08-query-check-1. First real
PG ledger test attribution_analytics_ledger_independent_counts_money_and_product_bases
passed AS-P08-ledger-tests-1; verifies two currencies, three paid orders/multi-lines,
one partial refund, rate2.0, sold-product filtering/grouping, exact totals. Not full
phase acceptance; extensive scope/time/pagination/export/load/browser proof remains.

Next implement own service-rendered /reports overview and /reports/orders/{id},
/reports/orders, /reports/events pages using current commerce read functions; add
Reports link to identity shell and shared HTML escaping owner (two actual consumers).
GET forms convert local date midnights via application midnight helper. Minimal
separate report.js only handles unresolved event retry through existing API; no
async report-read state. Then remaining focused tests/contracts/docs and one batch
review. No need another gate unless D1 invariants materially change. B idle after
architecture PASS. Main delivery workspace safely contains accepted p07 source,
confirmed identical2712efab digest; latest sync guard p07/logs/AS-P07-main-sync-1.json.
Do not sync partialp08 yet. All source remains uncommitted.


## AS-P08-B1-R1 — implemented, frozen; independent review pending

Current base/HEAD6e55258c; source `e31e15080831480b5ad749be84bb980250149b98732741d01b43533caae40e91`. Exact nine exclusions unchanged:
p01/docs through p08/docs individually plus coordination. Main workspace rechecked
at acceptedp07 source2712efab; immutable `logs/AS-P08-p07-baseline.tar` contains
changed predecessor files, `AS-P08-delta.json` hashes all35 p08 changed/new paths.
All p01–p07 code and user test-fix commit preserved. Coordination authority main
revision23, C001–C007/r1 acknowledged; immutable pin AS-P08-B1-R1-coordination-r23.md.
No schema/gated-decision change. No tests removed or renamed. Fixture constructors
now accept an actual configured browser origin, all original test names retained.

Implemented report API/CSV, dashboard forms/details/events, embedded assets and
shared authorization/HTML owners. Financial facts aggregate before paginated groups;
nullable group join uses hashable equality on null flag/value. Initial10000 export
was503 under the2s budget; fixed query now passes exact10000 and rejects10001 without
raising limits. Representative10000 clicks/1000 observations/1000 orders/1refund
HTTP overview ~17ms and SQL ~11ms locally; exact plan and timing in analytics-3.
No measured need for extra indexes or preaggregation. Product context is explicitly
nonadditive; distinct amounts/currencies/clocks use immutable snapshots.

Boundary guard now understands CTE column lists/materialization and LATERAL, retains
nested foreign-table and separate-literal denial. Shared HTML error boundary
preserves escaped accessible errors with current request ID/security/redaction;
startup regression13 passed. Browser found wrapping select label ambiguity; applied
existing explicit-label learning and preserved exact accessible locators.

### Actual acceptance mapping

| ID | Production path / decisive assertion | Evidence | Status |
| --- | --- | --- | --- |
| AC1 | analytics scope HTTP: user/member/key, populated sibling store excluded by key, existing foreign store/ad/campaign404 for legitimate multi-company user, revoked key401; strict ranges/query/cursor replay | analytics-3 scope/bounds | PASS |
| AC2 | real ingress ledger EUR/USD multi-lines and partial refund; fixed persisted four-reason fixture with nine observations and real full refund; exact independent sums, units, tax/shipping and rate2 | analytics-3 ledger/reasons | PASS |
| AC3 | explicit23h/25h Berlin days, half-open paid boundary, refund-only negative1200 and undefined rate; advertised P1 history survives live edit, sold-product amount basis and omitted rate | analytics-3 clocks/ledger/reasons | PASS |
| AC4 | real API128four-byte key cursor>512<=1024 and1025 rejects; null-group cursor; page100; CSV formula safety;10000 accepts/10001 rejects; numeric overflow400; blockedquery503 then recovery; exactproduction EXPLAIN | analytics-3 bounds + retained bounds-2 failure | PASS |
| AC5 | actualChromium390px, exact keyboard labels, productXSS, API/visible47.00 net equality,CSVdownload,DST/empty/error,order/refund details,live roles/CSRF,stable response-loss retry survives navigation,company isolation | browser-3 + HTTP dashboard | PASS |
| AC6 | shared Authorized consumer regressions:12catalog/28commerce; migrations,SQLxmetadata,fmt/offline alltargets,11boundary fixtures,15DB/startup,15wire contracts | catalog-1,commerce-1,foundation-3,contracts-1 | PASS |

Browser uses fixture upstream identity; actual Openbot login already acceptedp03,
p10 repeats combined gate. Bulk reports use schema-valid persisted facts, not a new
merchant stress test; production ingress separately exercised by ledger/commerce.
Production volume/SLO unapproved; phase09 cleanup and phase10 full integration still
NOT EXERCISED here. No native adapter/pilot or deployment claim.

Retained failed checks: HTTP1/2 accessible error header lost by shared boundary, now
startup+dashboard pass; bounds1 private test visibility; bounds2 export timeout;
browser1 wrapping select name; foundation1/2 staticSQL syntax parsing. Current
passing evidence above supersedes each failure, without discarding it. Trail check
still reports two false lexical matches in unchangedp01:170/p03:105 documentation
against a shell-runner learning. Both document failure propagation; they neither
change nor weaken the committed test runner. Same p07 disposition; no commit/PR
requested. Known unchangedp03 clock_timestamp expiry fixture is reserved forp10.

Evidence manifest AS-P08-B1-R1.evidence.json. Review artifact frozen for B; no source
writes until reply. Next B whole08 review, resolve all findings in one correction,
accept and transfer guarded35-path delta to main, then beginp09 architecture.


## AS-P08-B1-R2 — all findings corrected; recheck pending

B AS-P08-B1-N1 requested F1 duplicateCSVcolumn and F2 missing delivered CSV basis labels.
Corrected both together: unique currency_ metric columns preserve group numerator,
and each CSV row carries group_by, click_observation_basis, financial_basis and
nonadditive_measures. API/dashboard pass their validated grouping to shared exporter.
Both routes trace to the same serializer (logs/AS-P08-F1-callers.txt). REPORTING and
OpenAPI describe exact headers/labels. No aggregation/authorization/gate change.

Added attribution_analytics_export_parsed_currency_and_product_semantics: actual
HTTP export of2 attributed paid orders (EUR/USD) and2 products is parsed using
Python stdlib csv.DictReader. It asserts unique headers, group total2 vs currency1,
blank later group fields, per-product line2400/3500, repeated wholeorder tax1180 and
shipping500 plus explicit nonadditivity labels. Existing10000-row export remains
within bound with labels and10001 rejects. Focused9 PG tests pass R2-analytics-1;
Chromium1 pass R2-browser-1; offline/fmt/contracts15/diff pass. No tests removed.

New frozen source `99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e`, HEAD6e55258c and same9 exclusions. Coordination23
C001–C007/r1 unchanged/checked; R2 evidence validates inherited unaffected checks plus
all correction consumers. B must close F1/F2, root has not accepted p08 yet.

Independent future-gate environment preparation (no source changes): task-owned
Colima as-attribution-integration running (4CPU/6GiB/32GiBdata/8GiBroot/nohostmounts,
no SSH config/default Docker context change). Installed docker-buildx dependency.
Task-local DOCKER_CONFIG=/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-docker-90iaewwo
with cliPluginsExtraDirs; Docker socket /Users/mac02/.colima/as-attribution-integration/docker.sock.
Openbot unchangedimage build as-openbot-integration:local in progress, execsession68642,
log AS-P08-openbot-image-preparation-2.txt. Preserve until p09/p10 topology proof;
clean only this profile/cache at final completion. Initial legacybuilder --progress
failure retained in preparation.txt. These are environment preparations, notp09acceptance.


## P08 accepted — AS-P08-ACCEPT-R1

B AS-P08-B1-N2 explicit PASS 99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e; F1/F2 closed. Root recomputed same artifact, checked coordination23 pin and retained evidence validator. Phase08 accepted/integrated in isolated sequential worktree; final plan verification remains09/10. Guarded main transfer follows; no commit/deployment.
