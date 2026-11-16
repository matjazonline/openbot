# P08 independent review notes

## AS-P08-G1-N1 — architecture gate — 2026-11-16

**PASS AS-P08-D1/r1**, immutable SHA256
`69dcc8f4b01b904e3b89352ba1c9d2b498baef6740e7b145ebda6c56651b6a2f`.
No concrete actionable findings or blocking architecture questions established.

### Identity and inputs

Workflow `implement-astra-reviewed-2-3`, phase p08, whole08A/B architecture gate.
Retained independent B `/root/attribution_reviewer`, requested Astra/high without
inherited author reasoning; actual model/effort/context telemetry unavailable.
No implementation/design edits or delegation. Worktree/records authority:
`/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
Detached HEAD/start/review-base `6e55258cbbdc7633940805f366fb17fe240f02aa`.
Accepted p07 prerequisite/source baseline
`2712efab34ac16d2a2d368b4d83909d04b57b6e93eaf0740ef8262d6fbfd091d`,
B AS-P07-B1-N1 and root AS-P07-ACCEPT-R1. Preserved earlier attribution work and
user test-runner commit remain unchanged. Exact nine exclusions appear in REVIEW.

Read original08A/B, relevant original09/10, service AGENTS, contract README reporting/
privacy and pagination/export rules, Analytics/ReportGroup/FinancialTotals and both
analytics route schemas, analytics example, TASK/HANDOFF and the complete immutable
[decision](logs/AS-P08-D1-r1.md). Reviewed planned AC1–AC6 and shared-owner impact
mapping in [TASK](TASK.md) against original obligations and accepted producers.

Main coordination authority:
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer/root revision22. Read active C001–C006/r1 and proposed C007/r1; main bytes match
[immutable pin](logs/AS-P08-D1-coordination-r22.md) at start/end. TASK/HANDOFF still
record their discovery check at revision21; this gate uses the newer pinned22 and
C007 proposal explicitly, without treating it as an already active implementation.

### Coverage and evidence

Reviewed bounded strict query normalization, UTC half-open periods/IANA grouping,
user/key authorization and current UUID filter ownership; merchant-derived store
scope and public-ID exclusion. Challenged the shared Authorized move against its
actual consumers: company/filter/actor/begin are reused, while catalog-specific
resource and permission checks remain catalog-owned. Report Read/analytics:read is
an additional precise check, not a widening of existing link permissions. User and
merchant transaction entry retains live tenant/session/key/store checks and atomic
merchant last-use. Shared catalog/commerce regression checks are explicitly due.

Reviewed independent aggregation of clicks/observations/orders/reasons/lines/refund
facts/refund lines before joins; distinct business identities and exclusion of
unsettled/conflicting facts; frozen dimensions rather than current catalog edits.
Separate paid/refund clocks permit refund-only negative periods without restricting
refunds to orders paid inside the range. Currency aggregates remain separate and
click denominators occur once per group. Undefined rates are null and rates above1
are not clamped. Sold-product filtering/grouping selects matching line measures,
counts each matching order/refund once and omits the rate. Contextual click counts
and order-level tax/shipping can repeat across product groups, with explicit
nonadditive labels and an independently aggregated overview. No unsupported line
allocation or sum of overlapping distinct counts is introduced.

Reviewed bounded keyset semantics, null-first C collation, normalized query/company/
derived-store binding and size-only page variation. Analytics cursor1024 is an
explicit public contract correction; other cursors stay512. The maximum128 four-byte
Unicode-key round trip and1025 rejection are concrete regression tripwires required
in implementation/CI, not a silent raised bound. SQL exact aggregate overflow must
reject rather than round/clamp. Report single-statement snapshots, shared tenant
ownership,2s statement limit, request/admission limits, measured representative
EXPLAIN/latency and10,000 total CSV-row cap address bounded execution. CSV rejects
oversized results before successful output; escaping/formula safety is distinct from
numeric negative amounts and per-group fields are not repeated per currency row.

Reviewed service-owned SSR dashboard reuse of the same report owner, bounded commerce
read owners and existing setup/key/link UI; service-only assets, account/company links,
role/CSRF controls, safe EventResult diagnostics, escaping, accessibility, labels,
empty/error states and navigation semantics. Date conversion explicitly defines
ambiguous/nonexistent midnight behavior and displays actual UTC interval. Ordinary
GET navigation avoids a separate asynchronous report state engine; AC5 still must
prove superseded reads cannot overwrite current navigation and accepted retry writes
remain durable. No browser-only selection or detail-page summation becomes authority.

Used Trail before source discovery and inspected these actual spans/edges:

- `services/attribution/src/adapters/catalog/postgres.rs:24–86` Authorized;
  read112–123, list131–149 and mutate191–218 show authorization/filter/transaction/
  receipt obligations. Exhaustive scoped Authorized search identifies catalog HTTP,
  persistence methods/apply and one real competing-disable test.
- `adapters/catalog/http.rs:44–114` current user/key selection, scopes, admission,
  CSRF and read entry; `adapters/management.rs` current Actor/Receipt owner surface.
- `adapters/identity/http.rs:439–475` full shell rendering and shell caller: current
  active-company setup, CSRF, account/company navigation and escaped content.
- `adapters/commerce/reads.rs:107–116` scoped read_order, and commerce manage62–137
  plus exact reads::list callers; widening visibility can reuse the accepted owner.
- `adapters/website/postgres.rs:62–112` complete observation producer freezes click
  or link dimensions under scoped installation/tenant ownership and observation clock;
  `adapters/catalog/postgres.rs:377–394` scoped dimension producer.
- Accepted p07 commerce/schema/frozen paid/refund facts and p05 click producers were
  reused from immediately preceding independent review; no unchanged whole-phase rerun.

Paths beginning adapters above are under services/attribution/src. No new report SQL,
UI or runtime exists yet; source inspection establishes producer/consumer feasibility,
not successful future aggregation, performance or UI behavior. Original09/10 and C007
retain actual cleanup/report atomicity, retained financial dimensions and full combined
verification obligations. Raw counts are not promised beyond their retention lifetime.

Executed only static identity checks: decision hash, HEAD, start/end source snapshots,
main coordination pin and git diff --check, all PASS. Evidence:
[identity](logs/AS-P08-G1-reviewer-identity.txt),
[start snapshot](logs/AS-P08-G1-reviewer-start.snapshot.txt),
[end snapshot](logs/AS-P08-G1-reviewer-end.snapshot.txt).
No implementation evidence manifest or runtime tests are due for this decision gate.
AC1–AC6 are explicitly NOT EXERCISED, including representative latency, Unicode cursor,
CSV cap/formulas, DST, real dashboard and shared-owner regression tests.

### Outcome and process feedback

PASS applies only to AS-P08-D1/r1 at the exact hash above. Root may accept the gate
before dependent implementation; later implementation needs whole-batch review and
actual criterion assertions. P07's unrelated session-fixture failure remains p10-owned;
no green full-suite claim or p09/p10 waiver is made. Native-store pilot, production
SLO/retention policy and deployment remain outside this gate. No owned process exists.

The cohesive gate is warranted for shared authorization ownership and durable report
meaning across SQL/API/CSV/UI/retention. Explicit nonadditive product groups and a
separate overview eliminate an otherwise ambiguous consumer contract. Planned bounded
Unicode cursor proof preserves a concrete early failure after the limit change.
No new instruction change is supported by this review. No findings file required.


## AS-P08-B1-N1 — whole implementation review — 2026-11-16

**CHANGES REQUIRED AS-P08-B1-R1**. Open **AS-P08-B1-F1/P2** (duplicate CSV metric
header loses group numerator) and **AS-P08-B1-F2/P2** (purchased-product CSV lacks
promised nonadditivity/basis labels). All concrete findings saved together in
[findings](agent-review-findings.md). No architecture-decision defect established;
corrections fit accepted D1/r1.

### Identity and coverage

Workflow implement-astra-reviewed-2-3, p08 entire08A/B implementation. Retained
independent B /root/attribution_reviewer, requested Astra/high; actual model/effort/
context telemetry unavailable. Worktree
`/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`;
phase start/review base/current HEAD6e55258cbbdc7633940805f366fb17fe240f02aa.
Reviewed frozen artifact
`e31e15080831480b5ad749be84bb980250149b98732741d01b43533caae40e91`
against accepted p07 source2712efab. Exact nine exclusions in REVIEW unchanged.
No source/design edits or agents; reviewer probes/records only under p08 logs.

Main coordination authority remains
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer/root revision23, active C001–C007/r1. Main pin byte equality checked at start/end.
Original08, service contracts and original09/10 obligations from G1 were reused;
new TASK actual AC1–AC6 mapping and changed OpenAPI/REPORTING documentation checked
against actual producers/consumers/assertions. D1/r1 immutable69dcc8f4 and G1-N1 remain
architecture prerequisites. No p09/p10 implementation is implicitly accepted.

Independently compared the complete tracked/nonignored file inventories to the
accepted p07 main workspace under the exact exclusions, not only the author list:
exactly35 changed/new paths, matching AS-P08-delta.json. All19 predecessor tar files
match accepted p07 bytes; all current35 hashes match frozen inputs. See
[delta check](logs/AS-P08-B1-R1-reviewer-delta-check.txt). Read all new production
application/analytics.rs and analytics/{query,postgres,http,export,dashboard,views},
HTML helper, report assets/browser and all nine analytics test definitions; inspected
all changed shared-owner/router/fixture/CI/boundary/contract/doc diffs. Unchanged
p01–p07 code retains prior acceptance; no whole prerequisite re-review claimed.

Trail preceded discovery; followed shared Authorized consumers, commerce scoped read
owners, shell/router and error boundary. Application query validates strict bounds,
canonical instants/zone, typed filters and1024 cursor. Query SQL was checked branch
by branch: each UNION owns a measure, distinct product projections avoid order/refund
fan-out, refund clocks do not constrain payment period, null groups and C ordering
match Rust assembly, contextual sold-product counts repeat deliberately. Exact numeric
sums reject overflow, independent currency totals and nullable/uncapped rates retain
contract meanings. Scoped current objects, merchant-derived store, cursor bindings,
live transaction entry and atomic last-use remain intact. No SQL aggregation defect
was established in this review.

Checked API/CSV/body/credential routing, export cap before success, SQL2s deadline,
nullable key pagination, maximum Unicode cursor, fixed-plan performance evidence,
SSR local date conversion, frozen historical identifiers versus current suggestions,
HTML escaping, responsive/accessible labels, safe retry metadata and role/CSRF checks.
Shared Authorized move preserves catalog restriction to link resources and original
permission/key scopes; receipt ownership unchanged. Commerce changes only widen
read visibility; fixture constructors preserve original discovery/test names. Shared
HTML SafeError is minted only by the escaped constructor; arbitrary errors still
redact and common boundary applies security/correlation/retry headers. Boundary
parser change is limited to CTE lists/materialization/LATERAL and has nested-foreign
and separate-literal negative assertions. CI explicitly runs dashboard Chromium.

### Checks and findings evidence

Independently executed:

- `cargo +1.95.0 test --locked --offline --lib analytics_tests -- --test-threads=2 --nocapture`:
  **8 passed,1 ignored** browser; actual PostgreSQL; representative exact query plan
  SQL10.246ms. [Output](logs/AS-P08-B1-R1-reviewer-analytics.txt).
- `cargo +1.95.0 test --locked --offline --lib attribution_dashboard_browser_reports_filters_details_and_retry -- --ignored --nocapture`:
  **1 passed**, real Chromium/dashboard/HTTP/PG, controlled upstream authority.
  [Output](logs/AS-P08-B1-R1-reviewer-browser.txt).
- `cargo +1.95.0 test --locked --offline --lib catalog_tests -- --test-threads=2`:
  **12 passed** shared authorization regressions.
  [Output](logs/AS-P08-B1-R1-reviewer-catalog.txt).
- `PYTHONDONTWRITEBYTECODE=1 /tmp/as-p02-contract-clean/bin/python scripts/test_boundary.py`:
  **11 passed**. [Output](logs/AS-P08-B1-R1-reviewer-boundary.txt).
- Reviewer-only probe compiled actual production export.rs with production types:
  reproduced both CSV defects; parsed output using csv.reader/DictReader.
  [Probe](logs/AS-P08-B1-reviewer-csv-probe/main.rs),
  [CSV](logs/AS-P08-B1-R1-reviewer-csv.csv),
  [analysis](logs/AS-P08-B1-R1-reviewer-csv-analysis.txt).
  This is a representation probe with constructed report values, not a merchant/SQL
  fixture. Existing SQL/browser checks separately exercise actual production data.

Rust1.95.0, offline SQLx, stock2MiB libtest stack, cache /tmp/as-attribution-target;
UUID-owned local PostgreSQL fixtures; Node24/pinned Playwright module
/tmp/as-p03-browser/node_modules/playwright/index.mjs. Commands executed service-local
except reviewer probe/static identity. All owned processes completed. Successful
fixtures close their own servers/databases; probe artifacts retained as evidence.

Inspected author foundation PASS (fresh migration, SQLx metadata, formatting,
offline all-target check,11 boundary,13 startup and2 storage), commerce28 PASS
(includes report tests), wire contracts15 PASS and historical corrected failures.
[Manifest](logs/AS-P08-B1-R1.evidence.json) validates **7 PASS/2 FAIL entries**;
[validation](logs/AS-P08-B1-R1-reviewer-evidence.txt). Preserved failures include export
limit timeout corrected without raising2s bound, HTML boundary/label/CTE parsing
corrections and previously reviewed unrelated Trail lexical matches. Manifest validity
is not acceptance; export semantic failures remain despite the passing original checks.
Known unchanged p03 session timestamp fixture remains reserved forp10 as recorded inp07.

Start/end frozen snapshot, HEAD, main coordination23 pin and diff check PASS:
[identity](logs/AS-P08-B1-R1-reviewer-identity.txt),
[end snapshot](logs/AS-P08-B1-R1-reviewer-end.snapshot.txt).
No complete plan, full-suite, native-provider pilot or deployment claim. Actual retention
and combined production effects remain p09/p10; full repository gates still required.

### Verification limits and process feedback

The multi-product/two-currency ledger tests discriminate join multiplication and
scope well. Export tests cover formula escaping, byte production and maximum rows,
but their substring/download assertions miss named-column semantics and required
artifact labels. F1/F2 document this concrete gap; they are implementation/verification
misses, not design-gate errors. Candidate narrow improvement: parse flattened exports
and reconcile uniquely named scoped measures and required semantic caveats, especially
where report metrics are intentionally nonadditive. No skill/AGENTS edit performed.

Dashboard browser proves ordinary navigation and completed lost-response retry
persistence. It does not simulate a slow superseded report request; server2s/query
bounds and ordinary document navigation were inspected, without claiming a concurrent
browser cancellation schedule. Bulk load uses schema-valid persisted facts based on
accepted real ingress, not merchant stress throughput. Only the overview's detailed
EXPLAIN plan is retained; grouped maximum export executes its own meaningful10000-row
boundary. Thin module/visibility/CI changes reviewed by diff rather than new bespoke
tests. The batch remains cohesive; fix both findings together and recheck the export
contract/consumers and affected tests at a new frozen digest.


## AS-P08-B1-N2 — correction recheck — 2026-11-16

**PASS AS-P08-B1-R2**, exact artifact
`99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e`.
**AS-P08-B1-F1/F2 closed** by independent recheck; no remaining actionable findings.
See appended [finding dispositions](agent-review-findings.md). R1's changes-required
outcome and reproduction are preserved; this follow-up supplies correction evidence.

### Identity, delta and scope

Workflow implement-astra-reviewed-2-3; p08 whole-batch correction stage. Retained
independent B /root/attribution_reviewer requested Astra/high; actual model/effort/
context telemetry unavailable. Worktree/records authority and nine exclusions are
unchanged from REVIEW. HEAD/start/review-base6e55258cbbdc7633940805f366fb17fe240f02aa.
Main coordination authority is the original absolute path in REVIEW, writer/root,
revision23, active C001–C007/r1; main bytes equal the retained immutable R1 revision23
pin at start/end. No new decision revision, source authorship or delegation.

Checked R1 hashes and the entire tracked/nonignored source inventory versus accepted
p07 main. Exactly six R1 files changed: exporter, its API/dashboard callers, bounds
regression, REPORTING and OpenAPI. Only new deliverable is scripts/check-report-csv.py.
All other R1 source is byte-unchanged, justifying reuse of its completed whole-phase
review and independent SQL/catalog/browser/boundary checks. See
[delta audit](logs/AS-P08-B1-R2-reviewer-delta-check.txt).

Read the complete corrected serializer, both actual callers, full new real-HTTP
regression/parser and documentation changes. Traced both export::csv callers through
Trail; no sibling export path was omitted. Unique currency-prefixed headers preserve
group and currency measures without positional ambiguity. Validated grouping travels
from each normalized request into export labels; product-filter rate metadata drives
the contextual/matching-line basis, and sold-product grouping explicitly marks all
required overlapping measures. Existing quoting/formula escaping, first-currency-row
group fields, and10000 data-row cap remain intact. New parser independently interprets
actual export bytes; it does not call production helpers to calculate expected values.

### Verification and result

Independently executed service-local:
`cargo +1.95.0 test --locked --offline --lib analytics_tests -- --test-threads=2 --nocapture`.
**9 passed,1 ignored** browser, real UUID-owned PostgreSQL, stock2MiB libtest stack,
offline SQLx and /tmp/as-attribution-target. New parsed currencies/products cases both
PASS and original Unicode/pagination/overflow/exactCSVcap/timeout/ledger/DST tests
remain passing. [Output](logs/AS-P08-B1-R2-reviewer-analytics.txt).

Inspected current author R2 Chromium1 PASS (including dashboard download), offline
all-target compile/format/diff PASS and wire contracts15 PASS. Did not repeat unaffected
full catalog/commerce/foundation or Chromium checks independently; R1 independent
results and unchanged source plus author R2 browser proof remain valid. The inherited
manifest entries are historical R1 evidence, not misreported newly executed R2 checks.
R2 manifest validates **10 PASS/2 FAIL entries**, retaining earlier failure dispositions:
[manifest](logs/AS-P08-B1-R2.evidence.json),
[validation](logs/AS-P08-B1-R2-reviewer-evidence.txt).

Start/end snapshot, HEAD, coordination23 pin and diff check PASS:
[identity](logs/AS-P08-B1-R2-reviewer-identity.txt),
[end snapshot](logs/AS-P08-B1-R2-reviewer-end.snapshot.txt).
No reviewer-owned process remains. Root's separate task-owned Colima preparation is
outside this review and was neither used nor touched. Reviewer probe artifacts from
R1 remain historical evidence; they were not silently updated to describe R2.

PASS covers the whole p08 artifact by R1 full review plus this correction delta.
No new architecture gate is required. Actual retention/combined operational checks,
p10 full verification and the known unrelated p03 session-fixture correction remain
outstanding; no deployment/native pilot or full-plan completion claim. The original
batch coverage/limitations in AS-P08-B1-N1 still apply. Correction regressions now
exercise the concrete export consumer that exposed F1/F2. Candidate instruction
feedback from R1 remains supported; no additional instruction change is needed.
