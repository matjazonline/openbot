# Attribution p07 — durable commerce and reconciliation

- Workflow implement-astra-reviewed-2-3. Root/A /root user-confirmed Sol6.1/xhigh;
  independent retained B /root/attribution_reviewer requested Astra/high without
  inherited conversation. Actual context/model telemetry unavailable; no claimed
  setting changes. Skill absolute path remains original main .agents/skills/
  implement-astra-reviewed-2-3.
- Scope: complete original07A/B, plan/attribution-service/07-orders-refunds-and-attribution.md;
  contract README, OpenAPI Conversion/Order/Refund/EventResult and ledger examples.
  All ten phases authorized sequentially. P08–P10 wait for accepted predecessors.
- Worktree /var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree,
  detached HEAD/start/review-base6e55258cbbdc7633940805f366fb17fe240f02aa. Main user
  workspace /Users/mac02/orca/workspaces/openbot/attribution-service has same HEAD.
  User's independent committed/pushed test fixes in6e55258c are preserved, not ours.
- Prerequisite p06 accepted fd2a235453db086980456eaf541657997df46111ddd476632bba09c8cd3a2991
  at priorHEAD2df562d6; independent AS-P06-B1-N3, root AS-P06-ACCEPT-R1. Advancing to
  user test-fix commit changed9 non-attribution runtime files, preserving all own
  dirty source; current baseline digest will be recorded before architecture gate.
- Preserve allp01–p06 uncommitted source/records and four own Trail learnings.
  Owned new paths: service application/commerce.rs or cohesive directory, adapters/
  commerce/, reset schema, router/startup/config/fixtures, contracts/docs/browser.
  Root source touch only service fixture configuration in user_attribution_tests.rs.
- Record authority: phase-local docs in this isolated worktree. Main copies are
  mirrors only. Coordination authority /Users/mac02/orca/workspaces/openbot/attribution-service/
  plan/attribution-service/COORDINATION.md, writer/root, revision18 checked; C001–C005/r1
  acknowledged. C001 API-first/OpenbotSSO; C002 permissions/consent/noIP/window/freshness/
  503; C003 merchant/setup transaction and receipts; C004 frozen click snapshots;
  C005 original timestamps/checkout evidence and paid revalidation.
- Exact snapshot exclusions: plan/attribution-service/phases/p01/docs through p07/docs
  as seven separate arguments, plus plan/attribution-service/COORDINATION.md. No source,
  deliverable docs, sibling test fixes or learning exclusions. GNU coreutils PATH.
- Resources: service Rust1.95.0/cache /tmp/as-attribution-target, root cache
  /tmp/as-openbot-target and owned rootDB fromp03 resources.json; local maintenance PG,
  invocation-owned UUID roles/DBs/ports; Python3.13, /tmp/as-p02-contract-clean/bin/python,
  Playwright /tmp/as-p03-browser/node_modules/playwright/index.mjs, localHTTPS/openssl.
  No active fixture/process. Shared host PG may be busy with other tasks; never alter
  their DBs or misclassify timing interference as a regression without evidence.
- Status: whole-phase implementation present; focused verification in progress. Architecture accepted AS-P07-G1-N1.
  User has not authorized production deployment/reset or production policy; required
  explicit configuration and fixture values implement the contract without claiming it.

## AS-P07-D1/r1 — proposed architecture

1. Add a commerce policy value owned by application code and required explicit startup
   keys ATTRIBUTION_EVENT_MAX_AGE_DAYS and ATTRIBUTION_REFUND_WINDOW_DAYS, each1..3650.
   Tests/examples use7/365 and clearly label them fixture choices, not approved policy.
   DB time enforces occurred_at age and <=30s future skew on all external deliveries,
   including replays; paid_at is also within that delivery horizon, created<=paid<=occurred.
   Refund occurrence must be >=paid_at and <=paid_at+refund window when paid fact exists.
   Old originals410 cannot become new after dedup pruning. Already acknowledged pending
   events are reconciled from their immutable envelopes without reapplying ingress age.
   P09 retention configuration must cover these horizons before pruning is enabled.
2. Strict tagged paid/refund/cancelled DTOs and distinct internal/external ID types.
   External IDs <=128 characters, nonempty/no whitespace/control;1..100 distinct line
   IDs. Sort line sets and normalize UTC time before hashing. Preserve original full
   timestamp precision in canonical envelopes/fingerprints and checkout-created equality;
   indexed UTC SQL time columns are for clocks/reporting, not equality fingerprints.
   Amounts and checked sums0..9e12; paid quantities1..1e6, refund returned quantities
   0..1e6; merchandise sum equals lines; total merchandise+tax+shipping <=9e12.
   EUR/USD/GBP exponent2, JPY0; no floats/unsupported currencies. No body company/store
   selection, PII fields or browser observations in this trusted path. Authenticated
   merchant conversions:write headers precede262144-byte body read; begin_merchant
   rechecks key/store/tenant and commits last-use with envelope/effect atomically.
3. Reset-only schema: orders unique company/store/external order ID, immutable paid
   fingerprint/time/lines/money/attribution once present; unpaid placeholders may bind
   pending refunds/cancellations but never appear as paid orders. Order lines carry
   stable external line/product IDs, quantities, money and refunded/returned balances.
   Events unique company/store/external event ID store canonical immutable envelope,
   fingerprint, DB received time, mutable safe state/retry metadata and optional paid
   order association. Refund facts unique company/store/external refund ID bind one
   order, canonical fact fingerprint/time/currency/allocations and applied state;
   refund lines reference the same scoped order lines once payable. Scoped FKs/checks
   enforce ownership/nonnegative bounds; financial snapshots have no raw-click FK.
   Existing tenant lock precedes scoped order/event/refund row locks. Unique order
   identity plus this lock serializes concurrent first creation; no new advisory lock.
4. Exact same event identity/content returns CURRENT event state after current key/
   tenant validation; changed content409, no overwritten envelope. COMMIT precedes all
   success, uncertain outcome503.200 EventResult means durably accepted or a durably
   recorded business conflict;202 means durable pending prerequisites, never a sale.
   Distinct delivery IDs with identical paid business facts create one paid order.
   Paid fact hash excludes delivery event_id/occurred_at but includes external order,
   checkout, creation/paid instants, currency, sorted lines and all money. Different
   paid facts under the same order become visible conflict envelopes, never overwrite.
5. At first paid fact, null checkout=>missing; supplied absent/foreign/pruned checkout
   =>evidence_unavailable with no foreign detail. Found checkout creation must exactly
   equal order creation or the paid envelope is a reconciliation conflict. Revalidate
   its frozen selected evidence/policy, IDs and classification against scoped raw click
   availability plus inclusive[created-30d,created]; snapshot mismatch/unavailable=>
   evidence_unavailable. Frozen explicit no-consent/expired/future/bot/foreign reason
   remains explicit when no selection exists. Freeze selected click/time/dimensions,
   policy and reason on the paid order; later clicks/catalog edits/refunds/replays or
   click pruning cannot change it. Sales with unavailable attribution still count.
6. Reserve refund business identity before payment is available; otherwise E1/E2/E3
   competing for one refund could acquire conflicting identities while pending.
   Refund fingerprint excludes delivery event ID but includes its order/refund IDs,
   occurrence time, currency, sorted line allocations/quantities and tax/shipping.
   Same refund under another delivery applies balances once; changed facts/order
   become durable conflict. Under the order lock, check each remaining line money/
   quantity and order tax/shipping; validate all before writing any balance. Zero
   returned units with money and zero money with returned units are valid. No quantity-
   derived money. Unknown line/currency/over-refund/invalid refund time becomes visible
   conflict, with no partial ledger writes. Reserved identities are retained even when
   not applied; no correction under the same acknowledged business identity.
7. Cancellation envelopes record facts/earliest cancellation time on the scoped order
   placeholder, never create paid revenue or reverse it. A later paid event may settle
   regardless of cancellation arrival. Refund-before-payment remains pending; valid
   delivery orders converge after reconciliation. HTTP never drains an unbounded set
   of pending events inline. Revenue reversals require applied refund facts.
8. Supervised in-process worker:5s interval; claims at most8 due events, at most1 per
   company per batch, earliest due ordering. SKIP LOCKED claims commit random execution
   token/30s lease and increment attempts before work so crashes consume an attempt.
   Max20 attempts or7d pending deadline yields durable unresolved state. Exponential
   backoff capped1h, fixed documented implementation budgets. Claiming holds only event
   rows briefly, commits before tenant/status work. Processing obtains current tenant
   ticket then locks tenant, event, order/refund in that order; exact execution token
   and lease must still be valid after waits. Financial effects and state transition
   share one transaction, final token/expiry predicate fences completion; lost ownership
   rolls the entire effect back. Lease expiry recovers crashes. Persist safe retry
   reason/deadline/attempts without payload logs. Same-tenant heavy/poison work cannot
   claim the whole batch. Bound each task10s and keep tasks in an owned JoinSet.
   Already acknowledged valid facts do not require their old merchant key to remain
   active, but tenant/store must be active. Startup supervises identity+commerce workers,
   cancels/joins during bounded shutdown and fails if either exits unexpectedly.
9. User read/list order/refund/event APIs require live read authority and shared
   user_transaction. Every object/filter binds active company and store/parent where
   applicable, including users legitimately in both companies. Default25/max100 keyset
   pages use existing AEAD purpose binding with resource/company/store/state/parent.
   Return paid orders/applied refunds only; pending envelopes separately inspectable.
   EventResult adds safe diagnostics: kind, external_order_id, nullable refund_id,
   occurred_at, received_at, attempts, next_attempt_at, deadline_at; never full payload,
   credential or customer data. Update existing schemas/examples and C006/p08 consumer
   mapping. Exact request metadata is bounded; no financial fact editing API.
10. Manual retry endpoint requires ManageIntegrations, CSRF and shared management
   Receipt/Actor key/original time. Only unresolved events are re-armed with fresh
   attempt/deadline state; immutable facts remain unchanged. Accepted/conflict/pending
   states return current metadata without restarting/stealing an active lease.
   Concurrent exact manual retries have one effect/audit receipt; current authority
   always rechecked. Receipt replay returns its original response metadata, while
   merchant body-identity replay returns current event state as already contracted.
11. Publish C006 for downstream ownership: p08 consumes immutable facts and safe
   EventResult diagnostics, clocks and distinct refund identities; p09 validates raw
   click>=30d+delivery, financial facts>=refund+delivery horizons, dedup>=delivery plus
   skew margin, protects pending/leased/unresolved event and refund identity, serializes
   cleanup with tenant/order owner, retains snapshots and tombstones. P07 proves the
   DB protocol with competing settlement and scoped cleanup/deletion transactions;
   p09 must exercise its actual production retention/purge implementation. No claim
   p07 itself implements retention or approves production values. Paid events arriving
   after raw evidence was pruned remain explicitly unattributed, never recreated clicks.
12. Extend representative merchant fixture through paid multi-product order/refund,
   using server-controlled checkout and key. No native provider adapters or signatures.
   Existing two-tab/consent/checkout tests stay discoverable; if test modules split,
   inventory names before/after and account for each. Use focused checks below and
   one whole-phase review; final full repository/service/container/extraction suitep10.

## Acceptance mapping (planned; NOT EXERCISED)

| ID/source | Production case / decisive assertion | Focused proof |
| --- | --- | --- |
| AC1/07A.1–3,5 | Header auth401 before incomplete body; scope/class/revocation/tenant checks; no tenant override; floats/overflow/bad sums/currencies/duplicate lines/times reject; body/admission controlled | attribution_merchant_ingress_ HTTP+PG/TCP |
| AC2/07A.4–5 | Competing exact delivery one envelope/effect, changed409, constraint/actual post-COMMIT acknowledgement loss503 and exact retry once; expired originals410 even after pruning | merchant_ingress contenders and reused PG wire relay |
| AC3/07B.1–3 | Distinct identical paid IDs one paid fact; changed fact visible conflict; concurrent first creation; checkout time equality/full precision, inclusive30d and explicit all unattributed reasons, later edits/late clicks do not mutate | attribution_order_, attribution_policy_ real HTTP/PG |
| AC4/07B.4 | E1/E2 sameR with enough balance for both decrements once; E3 changedR/order conflicts; distinct refunds race cannot over-refund; money-only/unit-only, partial/full, tax/shipping/currencies exact | attribution_refund_ competing transactions and independent ledger totals |
| AC5/07B.5–6 | Refund-before-paid/cancel reordering converges; crashed claims recover; stale lease cannot commit money; attempts/deadline unresolved; admin retry/receipt/CSRF; tenant/store disable stops settlement; poison-company fairness; cleanup lock prevents lost acknowledged effect | attribution_reconciliation_ actual competing claims/writes, current cleanup protocol fixture; p09 actual owner later |
| AC6/07B.6/shared | All detail/list/filter/retry objects scoped with real foreign IDs and multi-company actor; keyset filters cannot cross scopes; diagnostics safe/bounded; exact current event state vs receipt metadata semantics | commerce API scoped fixtures and role/receipt regressions |
| AC7/completion | Actual tracking AdA thenB -> server checkout -> paid two products -> partial/full refund; one order, exact amounts/B attribution after replays; no browser commerce key/payload authority | production merchant HTTPS/Chromium/PG fixture, no native-platform claim |
| AC8/shared/final | Changed schema freshly migrates, SQLx metadata/offline/all-targets/fmt/contracts pass; inherited website/catalog/setup/identity/startup boundaries preserved, new required env consumers all updated | focused existing families + foundation; complete root/service/extraction/container/CI gatep10 |

## Contract impact / traced owners

Trail with_setup callers: startup.run, identity Fixture::with_origin, catalog.rebuild
(including commit-loss/tenant-admission tests), setup browser router. Add separate
CommerceHttp/HttpState policy ownership rather than attaching commerce to SetupHttp.
startup.run caller serve; supervised worker change affects normal/forced shutdown.
Config.from_lookup production/env and test config plus test_support.config. Exhaustive
ATTRIBUTION_PUBLIC_ORIGIN consumers include container-smoke.py and root actualSSO
browser fixture env; browser-sso.py forwards env. Update live examples/docs; never
rewrite historical reviewer probes. Root src/AGENTS and HTTP guide read before
fixture env edit. Shared merchant.begin_merchant, lock_tenant, Receipt.replay/record
remain responsible owners; no copied auth/receipt policy. Checkout evidence is the
acceptedp06 producer, raw click snapshot acceptedp05; no existing financial SQL
readers/workers to preserve. New report/cleanup consumers get C006. Independent
user test-runner commit now runs grouped gates outside Bash conditions; preserve it.

## Verification/resource disposition

Only focused tests during implementation; no full suite yet. Actual clocks/headers/
rows and competing claimants required, no sequential mock substitute. Browser identity
fixture is simulated authority unless actualSSO launched; explicit stand-in limits
retained. P07 needs an architecture PASS before dependent schema/protocol edits.
Native adapters/pilot, production region/retention/SLO deployment decisions remain
outside release implementation. No pending user permission blocks this phase.

Architecture input pinned AS-P07-D1/r1 SHA256 2760673dc2838a177c22686a1502ec4aa62f28c2ae66c6dff6a35793512e16cf. Main coordination19 checked, C001–C005 active/acknowledged, C006/r1 proposed. No p07 source edit; architecture gate pending.

Gate source baseline 87a2d3e588289d3e35d7023cd0b42ac21b42ac69293e46aa362797c6dccb1aa9 at HEAD6e55258c, eight exact exclusions. Source/decision frozen for B.

Gate identity correction: author ran imported runner focused tests after freeze without PYTHONDONTWRITEBYTECODE, creating exactly two disposable untracked bytecode files (scripts/check-doop and services/memory container-smoke pyc). Reviewer detected them; author removed only those owned files, restoring assigned source baseline. No implementation change. Future Python fixture commands use PYTHONDONTWRITEBYTECODE=1. Runner check output retained AS-P07-imported-runner-check.txt.

AS-P07-G1-N1 PASS received for D1/r1 hash2760673d; root rechecked source87a2d3e5 and coordination19. Coordination20 activatesC006/r1. P07 acknowledges activeC001–C006/r1. Architecture accepted; dependent implementation begins, all runtime/review acceptance outstanding. No processes active.


## Implementation and focused evidence — 2026-11-16

Author resumed on isolated HEAD6e55258c; main coordination20 and active C001–C006/r1
rechecked. User test fixes already included. No test-runner files changed here.
New commerce domain owns bounded typed money/facts, adapters own ingress, scoped
reads, ledger SQL and fenced worker. Existing begin_merchant/user_transaction,
lock_tenant, receipts, AEAD cursors and accepted checkout/click producers reused.
Schema is reset-only; service module/router/startup/config and all active fixtures
are wired. EventResult diagnostics and horizon docs updated per C006. Browser
fixture extends the existing discoverable p06 journey without deleting/renaming it.
Identity HTTP fixture mode7 now models a legitimate multi-company member so object
scoping is proved independently from upstream membership denial.

AS-P07-commerce-tests-1 compile failure: missing to_bytes import; fixed, no runtime
assertion executed. -2 passed original4 tests. -3 13/14 pass; manual retry fixture
used wrong header x-requested-at, corrected to existing Idempotency-Created-At.
-4 17/18 pass; disabled store returns existing Forbidden, not NotFound; corrected
assertion after tracing responsible enabled_store owner. -5 all20 tests pass with
real UUID-owned PG databases,2 test threads, stock2MiB stacks. No implementation
assertions weakened; tests now match the established auth boundary.

Command (service cwd): PATH includes libpq; CARGO_TARGET_DIR=/tmp/as-attribution-target,
SQLX_OFFLINE=true RUST_MIN_STACK=2097152; ATTRIBUTION_TEST_ADMIN_URL names local
maintenance DB; cargo +1.95.0 test --locked --offline --lib commerce_tests -- --test-threads=2.
Logs retain every failed and passing attempt. No test names removed.

AC1: ingress_tests::attribution_merchant_ingress_auth_before_body_and_strict_money
proves live TCP401 before incomplete body, scope/credential classes, malformed
integer/quantity/sum/time/tenant overrides, body bound, revocation before replay.
AC2: original competing_event_retries plus atomic_failure_and_real_commit_ack_loss
assert one envelope and one effect after actual PG COMMIT reply loss503; constraint
failure leaves zero orders/events/lines. original_horizon_survives_pruning proves410.
AC3: order_paid_business_identity, order_competing_business_facts and policy test
prove one paid fact, frozen attribution, exact30d/1us boundary, 1ns checkout mismatch,
explicit no-consent/future/expired/bot/missing/pruned and surviving paid snapshot.
AC4: independent_business_identity, competing_balances and currencies tests prove
oneR decrement despite distinct E1/E2 and sufficient balance for both; changedR
and changedorder conflicts; distinct refunds compete; money-only/unit-only and full
balances exact; four currencies, unknown line and time/tax/shipping failures atomic.
AC5: recovery tests prove actual concurrent claims, recovered expired lease/stale
execution rejection, final lease fence rolls back real balance writes, pending
identity reservations and age-independent replay, attempt/deadline unresolved,
manual CSRF/live-role/concurrent receipt semantics, cleanup/disable competes with
settlement on real tenant lock, revoking old key preserves acknowledged fact,
current tenant/refund window rechecks, cancellation shutdown joins tasks while
leaves recoverable lease, poison company cannot fill one batch. Cleanup test is a
protocol fixture; actual production cleanup remains p09 as originally scoped.
AC6: scope_tests uses one live user authorized in both companies, real foreign paid
order/event/store, scopes every route/filter/retry; cursor purpose/company/filter/
parent binding and no payload/secret metadata. Same-company lists page distinct IDs.
AC7: actual Chromium merchant fixture extended to payment and partial/full refunds;
server owns immutable payloads and key, browser submits empty requests; Rust asserts
one order/two refunds/two lines, gross5900/1180/500 equals refunds and AdB frozen.
Runtime browser result pending. Upstream identity remains controlled fixture here;
p03 real Openbot browser proof and final p10 integration cover actual authority.
AC8: contract schema check passed AS-P07-contract-1; foundation/offline/migration and
inherited owner regressions pending below. Final whole-repository gates remain p10.


Verification continuation: AS-P07-browser-1 PASS,1 real Chromium/HTTP/PG test,
including payment/refund ledger assertions described AC7. AS-P07-foundation-1
failed because host default Python3.9 lacks tomllib; -2 passed with declared
Python3.13 venv: fresh migration, SQLx metadata check, formatting, offline all-target
build, boundary checker +10 tests,12 startup tests and2 storage-boundary tests.
AS-P07-inherited-1:78 PASS,2 ignored (separately browser-covered where affected),
1 confirmed unrelated preexisting identity fixture failure. Exact evidence and
unchanged source/schema comparison AS-P07-unrelated-session-fixture.txt; volatile
clock_timestamp calls made idle expiry1us later than absolute expiry. No changed
production path ran in that failure. Keep unchanged forp07; p10 final integration
owns correcting that existing fixture to one timestamp then all final gates.
Do not claim the broad inherited invocation passed. Other inherited owner checks
passed. Root full build/test/container/extraction gates still deferredp10.

Trail check AS-P07-trail-check-1 exits1 for two lexical false positives against
user's new runner learning: original01 line170 says fail closed after5min without
successful status; original03 line105 records previous extracted-build proof. Neither
is Bash command grouping or conditional gate execution; scripts/check.sh and
check-support.sh are byte-preserved at user commit6e55258c. Do not change correct
contract/proof text to placate lexical matches. Retain this disposition for future
commit/PR description if user authorizes one. Other informational matches are
unchanged previous-phase requirements. No learning edited or ignored.

Frozen whole p07 artifact: HEAD6e55258c, digest 2712efab34ac16d2a2d368b4d83909d04b57b6e93eaf0740ef8262d6fbfd091d; eight exact exclusions unchanged. Whole-phase review requested AS-P07-B1-R1; no source edits until B finishes. Main coordination20/C001–C006/r1 pinned.

## P07 accepted in phase — 2026-11-16

B AS-P07-B1-N1 explicitly passed complete source 2712efab34ac16d2a2d368b4d83909d04b57b6e93eaf0740ef8262d6fbfd091d, no actionable findings. Root independently recomputed same digest/HEAD, compared authoritative coordination20 pin and retained evidence.20 commerce PG +1 actual browser +15 contracts independently rerun by B. All affected proof satisfied; unchanged identity-fixture clock failure remains recorded and p10-owned, no full-suite claim. P07 accepted/integrated into sequential isolated worktree; not yet synced to delivery workspace. Next p08. No active processes.

Accepted p07 source safely synchronized to main workspace with own-file hash guards; logs/AS-P07-main-sync-1.json records updated identities and changed files. User committed check/check-support files byte-equal HEAD. No commit/reset/stash. Isolated worktree still active for p08; coordination authority unchanged.
