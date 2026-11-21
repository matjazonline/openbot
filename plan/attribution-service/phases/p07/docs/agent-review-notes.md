# p07 reviewer notes

## AS-P07-G1-N1 — architecture gate — 2026-11-16

Identity: AS-P07-G1, p07 whole07A/B, architecture stage,
`implement-astra-reviewed-2-3`. Independent B `/root/attribution_reviewer`, requested
Astra/high without inherited author reasoning; actual model/effort/context occupancy
unavailable. No implementation/design edits or delegation. Active worktree/records
`/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`,
detached HEAD/start/base `6e55258cbbdc7633940805f366fb17fe240f02aa`.
Source `87a2d3e588289d3e35d7023cd0b42ac21b42ac69293e46aa362797c6dccb1aa9`;
exact eight exclusions listed in REVIEW. Accepted p06 fd2a2354/AS-P06-B1-N3 remains
the producer prerequisite; separate user test-runner commit6e55258c is preserved.
Decision AS-P07-D1/r1 SHA256
`2760673dc2838a177c22686a1502ec4aa62f28c2ae66c6dff6a35793512e16cf`.

### Coverage and evidence

Read original07A/B, service contract README and OpenAPI paid/refund/cancellation,
Order/Refund/EventResult, list/retry/auth/pagination shapes and ledger example.
Read original08 report clocks/dimensions/diagnostics and09 retention/cleanup/recovery
requirements, applicable root/service/source/HTTP rules, TASK/HANDOFF and all12
immutable decision items plus planned AC1–AC8 and contract-impact mapping. Safe
EventResult diagnostic extension and required event/refund configuration are explicit
proposals under C006, not assumed existing wire/startup behavior.

Reviewed distinct event/order/refund identity and normalized fingerprint ownership;
integer/checked money/quantity/currency validation; original-time horizons before
replay and reserved refund identity before payment; tenant-first atomic event/effect
transactions and commit uncertainty; immutable paid facts and first-paid checkout/
raw-evidence revalidation; explicit unattributed reasons; cancellation/refund
arrival-order semantics; line money/quantity/tax/shipping balances without inferred
quantity-derived money. Immutable envelopes and diagnostic state are separate, and
business conflicts do not silently overwrite paid/refund facts.

Reviewed proposed worker ownership: short committed claims before tenant work,
SKIP LOCKED bounded batch/per-company fairness, attempts consumed before processing,
leases/tokens checked after waits and at effect completion, transaction rollback on
lost ownership, retry/deadline unresolved state, supervised bounded shutdown and
manual permission/CSRF/receipt replay. Existing accepted work need not retain the
old merchant credential, but current tenant/store authority is enforced. Cleanup
and retention obligations are explicitly assigned to future actual p09 consumers;
p07's protocol fixture cannot substitute for those production retention checks.

Independently traced through Trail before exact source inspection:

- `adapters/setup/merchant.rs:109–137` begin_merchant current tenant/key/store/scope and atomic
  last-use; callers catalog, checkout and accepted revoke/disable tests.
- `adapters/setup/postgres.rs:91–116` user_transaction live scoped tenant/session lock;
  existing management/catalog callers.
- `adapters/management.rs:44–98` Receipt normalized hash, original-time validation,
  principal-kind identity, replay metadata and atomic audit/receipt.
- `adapters/website/postgres.rs:113–153` accepted immutable checkout selected evidence; indexed
  checkout references currently producer/tests, no existing commerce consumer.
- `infra/startup.rs:31–105` owned identity worker, router construction and bounded
  shutdown; `infra/config.rs:13–59` current startup configuration ownership.
- `with_setup` callers include startup, identity fixture, catalog rebuild and setup
  browser composition. Exhaustive indexed ATTRIBUTION_PUBLIC_ORIGIN references
  confirm service config/test_support, container-smoke, forwarded browser-sso env
  and root `user_attribution_tests.rs:170` fixture environment. Historical reviewer
  probes are evidence, not live configuration consumers to rewrite.

Paths above without a leading service prefix are under `services/attribution/src`
unless explicitly root. Existing lease/reconciliation symbols and tests were
consulted as patterns, not accepted as proof of the new commerce worker. No p07
source implementation exists. Original08/09 consumers remain future phases and
must require accepted p07 implementation, not this gate.

Coordination authority is the main absolute plan-root path in REVIEW, writer/root,
revision19. Fully read active AS-C001–C005/r1 and proposed AS-C006/r1. Checked main
bytes against immutable `logs/AS-P07-D1-coordination-r19.md` at start/end. C001/API-first,
C002 consent/window/freshness/503, C003 shared authorization/receipt owners, C004
frozen click snapshots and C005 first-paid evidence revalidation are preserved.
C006 records proposed diagnostics, ingress/refund horizons and downstream retention
relationships without claiming deployment-policy approval.

Executed only identity/static checks: immutable decision hash, HEAD, snapshot under
all8 exact exclusions, main coordination pin comparison and `git diff --check`.
All final checks PASS. Initial reviewer snapshot included two author-generated
untracked .pyc outputs; root acknowledged/remediated only those disposable files.
Final reviewer snapshot byte-matches original assigned snapshot. Both initial and
restored evidence retained; no broadened exclusions. See
`logs/AS-P07-G1-reviewer-identity.txt` and reviewer start/end snapshots.

### Outcome and limitations

**PASS AS-P07-D1/r1**, exact decision hash above; no concrete actionable findings or
blocking questions. No findings file required for a clean gate. Planned AC1–AC8
provide appropriate discriminating checks, including simultaneous event identities,
different delivery IDs for one refund with enough balance for two decrements,
competing distinct refunds, leased-worker/crash/cleanup races and actual merchant
browser checkout-to-paid/refund flow. They remain NOT EXERCISED: no runtime tests,
production fixtures or implementation manifest were due/run for this gate.

Implementation review must inspect those actual production branches/assertions,
configuration consumer updates, startup supervision and downstream contract changes;
this pass does not certify future code or waive the repository final suite. Native
provider sandbox/pilot, production policy/deployment and combined full-plan checks
remain outside this architecture acceptance. No owned fixture/process remains.

### Process and instruction feedback

A cohesive architecture gate is warranted for persistent financial identity,
transaction/lease ownership, attribution evidence and shared retention/reporting
contracts. The criterion mapping makes the dangerous duplicate-refund case explicit
rather than relying on an accidental over-refund rejection as dedup proof. No new
instruction change is supported from this round. The disposable-bytecode incident
was resolved by restoring exact identity, not by hiding deliverable paths; avoid
bytecode generation in read-only imported checks during a freeze.


## AS-P07-B1-N1 — whole implementation review — 2026-11-16

**PASS AS-P07-B1-R1** for frozen artifact
`2712efab34ac16d2a2d368b4d83909d04b57b6e93eaf0740ef8262d6fbfd091d`.
No concrete actionable findings or unresolved p07 acceptance defects established.

### Identity and scope

Workflow `implement-astra-reviewed-2-3`; phase p07, whole07A/B implementation.
Retained independent B `/root/attribution_reviewer`, requested Astra/high with no
inherited author reasoning; actual model/effort/context telemetry unavailable.
No implementation/design edits or delegation. Worktree and records authority:
`/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
Detached HEAD, phase start and review base:
`6e55258cbbdc7633940805f366fb17fe240f02aa`. Reviewed p07 additions over accepted p06
`fd2a235453db086980456eaf541657997df46111ddd476632bba09c8cd3a2991`;
preexisting p01–p06 dirty implementation and user test-runner commit are preserved
prerequisites. Eight snapshot exclusions are exactly those listed in REVIEW;
no implementation/deliverable exclusion was added. Gated AS-P07-D1/r1 and its
SHA256 `2760673dc2838a177c22686a1502ec4aa62f28c2ae66c6dff6a35793512e16cf`
remain the decision input.

Main coordination authority:
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer/root revision20, active AS-C001–C006/r1. Read on review/resume and verified
byte equality at completion with [immutable pin](logs/AS-P07-B1-R1-coordination-r20.md).
Original07 requirements, original08 diagnostics/reporting implications, original09
retention ownership, service contract/OpenAPI/ledger examples, applicable AGENTS,
[TASK acceptance and impact mappings](TASK.md), and
[author manifest](logs/AS-P07-B1-R1.evidence.json) were independently assessed.

### Coverage and challenged assumptions

Read the new application commerce types/validation, all commerce HTTP/persistence/
settlement/read/retry/worker implementation, complete commerce reset-schema addition,
and all20 commerce tests. Checked composition through startup/router/shared merchant
and user transaction owners, configuration and active fixtures, EventResult contract,
and the extended merchant browser journey. Trail preceded source discovery; inspected
needed complete spans rather than trusting ranked summaries. Existing unrelated
p01–p06 implementations were reused as accepted prerequisites, not wholly re-reviewed.

- AC1/AC2: header/key/scope authorization before bounded bodies; strict typed identity,
  time/currency/integer/aggregate/line validation; current merchant/tenant/store lock
  before replay; normalized immutable event identity; concurrent equal deliveries,
  changed-envelope409, transaction rollback and genuine post-COMMIT response loss.
  Original occurrence and paid horizons are checked even when dedup rows are gone.
- AC3: order identity scopes company/store; immutable paid business hash excludes
  delivery identity/time but keeps money/checkout/payment facts. Exact checkout
  creation instant, frozen policy and selected raw click/dimensions/classification/
  inclusive30d are revalidated at first payment; missing/pruned evidence produces
  explicit unattributed sales. Later ad edits/clicks do not rewrite historical facts.
- AC4: refund business reservation precedes payment, binds order and facts, and
  distinct delivery IDs decrement once with enough balance for two decrements.
  Distinct competing refunds cannot exceed line money/units or tax/shipping balances.
  Currency/time/unknown-line conflicts leave financial effects atomic; money-only,
  units-only, partial and complete refunds are independently asserted.
- AC5: cancellation creates no sale/reversal; pending refund-before-paid converges.
  Claims are bounded and per-company; attempt/deadline accounting, short claims,
  token/expiry validation after lock waits, and final fence protect all financial
  writes. Tests include competing claims, stale token, forced final-fence lease loss
  after a real balance write with rollback, crash recovery, exhausted/manual retry,
  current tenant/store changes, shutdown during blocked work, backoff and fairness.
  Already durable facts settle despite later ingress age expiry or key revocation.
- AC6: live user/company/session and ManageIntegrations/CSRF boundaries; list/detail/
  filter/parent/retry IDs remain scoped even for an actor with real second-company
  access. Authenticated cursors bind company/resource/filter/parent. Only unresolved
  events rearm; shared receipts/audit are atomic. Merchant replay returns current
  state while management replay returns original response metadata. Diagnostics
  expose bounded identities/times/retry fields without envelope/key/line payloads.
- AC7: real Chromium, HTTPS merchant fixture, actual service HTTP and PostgreSQL
  execute Ad A then B, frozen server checkout, two-line payment, partial/full refunds
  and exact retries. Browser submits empty payment/refund requests; merchant server
  owns credentials and immutable commerce facts. Rust independently asserts one
  order/two refunds/two lines, exact5900/1180/500 paid and refunded totals and Ad B.
- AC8: checked both required1..3650 startup keys, fixture propagation, router/module
  wiring, owned worker supervision/join/shutdown, reset migration constraints and
  indexes, contract diagnostic extension and documentation against code. Thin module/
  environment additions were reviewed by focused diff; existing deployment/container
  and real Openbot SSO paths were not rerun in this phase.

### Executed and inspected verification

Independently executed, all PASS:

1. `cargo +1.95.0 test --locked --offline --lib commerce_tests -- --test-threads=2`:
   **20 passed**, real UUID-owned PostgreSQL databases, stock2MiB libtest stacks.
   [Output](logs/AS-P07-B1-R1-reviewer-commerce-tests.txt).
2. `cargo +1.95.0 test --locked --offline --lib attribution_checkout_handoff_browser_sdk_cart_and_freeze -- --ignored --nocapture`:
   **1 passed**, pinned Playwright1.55.1 Chromium, Node24, local HTTPS fixture,
   PostgreSQL and ephemeral HTTP origin. [Output](logs/AS-P07-B1-R1-reviewer-browser-tests.txt).
3. `PYTHONDONTWRITEBYTECODE=1 ATTRIBUTION_CONTRACT_PYTHON=/tmp/as-p02-contract-clean/bin/python scripts/check-contracts.sh`:
   **15 passed**. [Output](logs/AS-P07-B1-R1-reviewer-contract-tests.txt).
4. Start/end frozen snapshot, HEAD, coordination pin, evidence validator and
   `git diff --check`: PASS. [Final identity](logs/AS-P07-B1-R1-reviewer-identity.txt),
   [snapshot](logs/AS-P07-B1-R1-reviewer-end.snapshot.txt).

Service commands ran in `services/attribution`, Rust1.95.0, offline SQLx/cache
`/tmp/as-attribution-target`, local maintenance PostgreSQL and RUST_MIN_STACK2097152.
Browser used `/tmp/as-p03-browser/node_modules/playwright/index.mjs`. No owned
process remains; successful fixtures close/drop their owned servers/databases.

Inspected author foundation evidence: fresh migration and SQLx metadata check,
formatting, offline all-target compilation,10 boundary checks,12 startup and2 storage
checks PASS. Manifest validates **5 PASS and4 FAIL entries**; failures were retained,
not converted to success. Historical compilation/test fixture corrections are followed
by successful20-test run; host Python3.9 failure is followed by Python3.13 foundation
PASS. Inherited identity run remains **78 passed,1 failed,2 ignored**, not a passing
suite. Independently checked failing log at identity/tests.rs132, unchanged source
SHA256 `bcc008c23e0cf06c890be187aad3b8bf221fcadac5eb113dfdad36696ea54363`
against accepted p06 mirror, and fixture SQL: two volatile clock_timestamp calls make
idle expiry1us after absolute expiry, violating existing schema before session lookup.
This confirmed unrelated preexisting fixture remains unchanged; p10 owns its correction
and final suite. Source/pin/hash evidence is retained in reviewer identity log.

Trail check remains FAIL for two lexical matches in unchanged original01/03 prose.
Inspected matched lines: five-minute fail-closed requirement and historical extracted
build verification are neither shell conditionals nor swallowed failures. These are
confirmed unrelated lexical matches, not suppressed checks. No learning was edited.

### Limits, outcome and process feedback

**PASS** applies to this exact p07 implementation and verified affected scope only.
The cleanup test exercises the agreed tenant-lock/protected-row protocol fixture;
actual retention/purge owner and its combined races belong to p09/p10. The final
lease-fence test expires the lease through a database trigger after a real financial
write; separate tests cover actual competing claimants. Fixture upstream authority
is controlled; this browser check does not replace p03 actual Openbot SSO proof or
final p10 combined integration. No native-provider pilot, production retention/load
approval, deployment or full-plan/full-suite completion is implied. Original p10
formatting/offline compilation/migration/database suite gates remain required.

The whole phase was cohesive and reviewable; the architecture gate was warranted
for durable financial identities/lease ownership. Coverage mapping usefully separated
real production mutation tests from future-owner cleanup stand-ins. No new instruction
change is supported by this round. Keep historical failures and exact artifact identity
visible through downstream acceptance. No findings file was created for this clean pass.
