# P09 independent review notes

## AS-P09-G1-N1 — architecture gate — 2026-11-16

**PASS AS-P09-D1/r1**, immutable SHA256
`281e4b7cf4ac8ad7e8f7bdccd33a1887b49b6b638bebaae1e0178d2a9fa0a2de`.
Review round AS-P09-G1-R1. No concrete actionable findings or blocking design questions.

### Identity and requirements

Workflow implement-astra-reviewed-2-3, p09 whole09A/B architecture. Retained independent
B /root/attribution_reviewer, requested Astra/high without inherited author reasoning;
actual model/effort/context telemetry unavailable. No source/design authoring or agents.
Worktree and p09 records authority:
`/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
Detached HEAD/start/review-base6e55258cbbdc7633940805f366fb17fe240f02aa. Prerequisite
accepted p08 AS-P08-B1-N2/rootAS-P08-ACCEPT-R1, source
`99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e` unchanged.
Exact ten exclusions in REVIEW. User test-fix commit and accepted p01–p08 retained.

Reviewed original09A/B and downstream10 requirements, root/service rules and accepted
commerce/reporting/identity/retention contracts, full TASK/HANDOFF proposal and planned
AC1–AC8 against actual accepted producers/consumers. Original09/10 and contracts were
already read in preceding gates; their unchanged obligations are reused, not replaced
by the author's mapping. Read/verified complete immutable
[decision](logs/AS-P09-D1-r1.md) against TASK text/hash.

Coordination authority:
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer/root revision25. C001–C007/r1 active acknowledged and C008/r1 proposed reviewed.
Main authority matches [immutable25 pin](logs/AS-P09-D1-coordination-r25.md) at start/end.
TASK/HANDOFF startup revision24 is superseded by their explicit25 assignment checkpoint;
this review uses25 and does not treat proposedC008 as runtime implementation.

### Decisions and failure paths examined

Required retention configuration separates raw clicks, received observations,
financial evidence, terminal merchant envelopes and audit. Validated click30d plus
delivery/skew, observation48h minimum, financial refund-window plus delivery/skew,
event retry horizon, fixed48h management receipts and permanent terminal tenant fences
preserve accepted protocol assumptions. Example38/30/400/8/90 values are test choices,
not production approval. Existing ingress original-time checks remain essential after
pruning; future identity reuse outside declared contractual retention is distinguished
from replay of acknowledged existing work.

Reviewed cleanup's shared tenant-first lock ordering with merchant/public/user/report
owners, separate short scheduling claims, advance-before-attempt retry persistence,
per-iteration tenant bounds, SQL2s and wholebatch5s deadlines, child-inclusive100-row
cap, poison-tenant progress, cancellation and startup join ownership. No cleanup lease
is required for this proposed transaction-only effect model: claims schedule work;
actual tenant transaction serializes and rolls back effects, with bounded execution
well below the60s reschedule interval. Actual competing claimants, interrupted effects
and fairness remain mandatory tests, not inferred from this design statement.

Reviewed unresolved/pending/leased envelope and associated order/refund protection,
checkout-reference/raw-evidence protection, received-time observation/event horizons,
and frozen financial dimensions after rawclick pruning. Financial retirement is a
logical marker before bounded physical child deletion; every shared financial reader
and mutation must respect it so reports expose the full fact or no fact. Marking uses
order age and excludes outstanding work; child deletion is explicitly of expired
children before empty-parent deletion, not permission to bypass a child's own retry
retention. No unbounded parent FK cascade is an acceptable implementation of100 rows.
Retired parents fail closed for new effects while resumable child removal proceeds.

Reviewed deleted-tenant override: deleted state denies serving immediately; FK-safe
bounded physical purge includes the credential self-reference chain and all service
owned tenant tables, clears sensitive tenant metadata, leaves a permanent minimal
terminal fence and cannot invalidate another company's company-independent user
session. Local freshness invalidation and identity observation must preserve terminal
state and completed purge against delayed/higher-version provisioning and restart.
No server-persisted export/caches are currently present to leave behind.

Reviewed structured operational signals and redaction boundaries: fixed operation
labels/internal UUIDs, lag/duration/reason/outcome/pending age and maintenance failures,
without URL/body/header/IP/customer/secret/debug payloads or tenant metric labels.
Attempted selection versus committed result must stay distinguishable. Existing scoped
EventResult/admin retry remains repair authority; no new public management surface.

Reviewed standard custom-format pg_dump/pg_restore into a fresh owned isolated DB/role,
separate keyring recovery, preserved keys/encrypted sessions/ledger/pending/fences and
actual restored production operations. Cross-role denial and local elapsed measurements
are explicit; no automatic reset/restore-over-existing or invented production RPO/RTO.
Deployment proof requires attribution's non-root production image, exact real config,
verified HTTPS trust, private independent databases, identical artifacts in the two
local topology fixtures, actual Openbot bridge plus complete merchant/report journey,
filesystem/private-port/database denial, and extracted-source independence. Outage/
coldrestart/reconnect proof retains immediate user authority failures and five-minute
machine/public freshness bound. Local networks simulate host boundaries; they are not
proof of a real production relocation or native-provider pilot.

### Independent producer/consumer trace

Trail preceded discovery. Exhaustive service-scoped order/refund and lock references
were used because several bare graph names are ambiguous; no conclusion relies on
missing call edges. Inspected relevant complete spans and reused immediately accepted
unchanged p07/p08 mechanisms:

- `adapters/commerce/postgres.rs:72–83,107–152`: scoped order lock, event-result/replay,
  ingress original-time gate, existing/new order and event insertion. Retirement
  requires auditing these bypass/diagnostic paths as well as settle::apply.
- `adapters/commerce/reads.rs:61–219`: shared detail/order list/refund parent validation;
  exact read_order callers are commerce HTTP and analytics dashboard. Analytics SQL
  has four financial order-source/join branches (query.rs:72,81,87,94) shared by API,
  SSR and CSV; all need retired filtering, without hiding independent raw measures.
- `adapters/identity/tenants.rs:10–29,47–95,101–140`: current tenantlock/freshness,
  terminal observation/provision fence, reconciliation scheduling/fencing and existing
  bounded company-independent session cleanup. Proposed cleanup needs lock ownership
  without making maintenance depend on upstream freshness; serving still gates on it.
- Exact tenant entry references include setup.user_transaction, begin_merchant,
  public_installation, catalog.redirect and commerce.worker.process. Existing short
  event-only claims precede tenant/order settlement; cleanup must not invert that order.
- `infra/startup.rs:31–115`: owned JoinSet, HTTP drain, worker cancellation/join/abort
  and pool close. New maintenance must use this owner, not detach a background task.
- Migration FK/index audit includes store/domain/installation/catalog/click links,
  credential rotated_to self-reference, order/refund/event cascades and refund-line
  constraints. Logical retirement plus dependency-first bounded deletion is feasible.
- Current config/test_support constructors, browser-sso.py/container-smoke.py live
  launch env and Dockerfile were checked as configuration/deployment consumers. The
  service image already runs UID10001 with ca-certificates; new fixture settings
  must reach all startup/migrate/extracted/container paths. Root browser fixture
  forwards service environment through browser-sso.py rather than owning these keys.

Source paths above are under services/attribution/src unless migration/scripts or
Dockerfile specified. Deployment orchestration may use root-owned integration fixtures
without becoming an attribution runtime dependency; standalone required assets and
contracts must still travel with the service.

### Verification status, environment and outcome

Executed decision/source/HEAD/pin/diff identity checks only, all PASS:
[identity](logs/AS-P09-G1-reviewer-identity.txt),
[start snapshot](logs/AS-P09-G1-reviewer-start.snapshot.txt),
[end snapshot](logs/AS-P09-G1-reviewer-end.snapshot.txt).
No p09 source changes exist; AC1–AC8 are **NOT EXERCISED**, including real cleanup
races, cutoff replay, log redaction, backup restore, TLS/network topology and extracted
production-image checks. No implementation evidence manifest is due at this gate.
No owned reviewer process/fixture exists.

Root reported the existing Openbot release-image build was SIGKILL/OOM in the owned
6GiB Colima environment; no memory/runtime budget raised or source change. Historical
output remains p08/logs/AS-P08-openbot-image-preparation-2.txt. The proposed alternative
uses the actual Openbot app in a debug integration image, identical across both local
topologies. That can prove real bridge/network behavior, but cannot establish a
successful Openbot production release-image build; distinguish the proof explicitly.
Attribution's own production release/non-root image remains required, as do final
p10 gates. No success is claimed for the not-yet-completed alternative fixture.
This environment disposition does not alter the retention/ownership decision.

**PASS** only for AS-P09-D1/r1 at the exact hash above. Root may accept the gate before
dependent implementation. Whole implementation review must verify actual retirement
predicates and all shared consumers, child-inclusive row limits, genuine competing
transactions and restoration/topology assertions. Native pilot, production policy,
deployment approval and full-plan/full-suite completion remain outside this gate.
Known unrelated p03 expiry fixture remains p10-owned. No findings file required.

### Process/instruction feedback

The cohesive architecture gate is warranted for permanent tenant fences, staged
financial visibility, resumable deletion and cross-subsystem recovery/deployment
contracts. It explicitly replaces the prior p07 cleanup stand-in with the real owner
and required competing-runtime proof. Atomic retirement reconciles bounded physical
cleanup with complete report facts; implementation should be judged through actual
all-consumer assertions, not marker existence alone. No new instruction change is
supported from this gate. Preserve unavailable environmental checks distinctly from
fixture alternatives and never infer a successful release build from a debug image.


## AS-P09-B1-N1 — whole-batch implementation review — 2026-11-16

**CHANGES REQUIRED: AS-P09-B1-F1 and AS-P09-B1-F2 (P2 verification/fixture defects).**
No production implementation defect identified in the reviewed batch. All source was
frozen through this review; reviewer wrote only p09 review records/logs. See
[findings](agent-review-findings.md), [TASK mapping](TASK.md),
[R1 evidence manifest](logs/AS-P09-B1-R1.evidence.json).

### Identity and requirements

Workflow implement-astra-reviewed-2-3; p09 AS-P09-B1-R1; reviewer
/root/attribution_reviewer retained independent B, requested Astra/high. Actual
model/effort/context telemetry unavailable; no replacement agent or source authoring.
Worktree `/private/var/folders/y1/v99y__1j741fh8z9fqd8rvlc0000gn/T/as-attribution-author-7fo_yda4/worktree`.
HEAD/start/reviewbase `6e55258cbbdc7633940805f366fb17fe240f02aa`;
artifact `868a58789f2d34aa43c62256bd6a480c8096613ba7f2dd70b496b6d2561d2339`.
Exact ten exclusions remain p01 throughp09/docs individually and COORDINATION.md,
as enumerated in REVIEW. [Start](logs/AS-P09-B1-reviewer-start.snapshot.txt) and
[end](logs/AS-P09-B1-reviewer-end.snapshot.txt) match;
[identity log](logs/AS-P09-B1-reviewer-identity.txt) verifies HEAD, diff check and
main coordination revision26 byte-equal to the immutable R1 pin, C001–C008/r1 active.
Authoritative coordination remains the main workspace absolute path, writer/root.

Independently read original09 operations/retention/deployment requirements and
current C001–C008; reused independently reviewed original10 and accepted p01–p08
contracts. Rechecked D1/r1 implications including no resurrection, protocol horizons,
financial atomicity, bounded cleanup and task ownership, separate restore, actual
HTTPS bridge, runtime isolation and extraction. Architecture gate AS-P09-G1-N1 remains
valid; these findings concern implementation verification and fixture chronology.

### Coverage and observed evidence

Reviewed the complete38-path p09 delta, not merely the changed Rust adapters.
[Inventory audit](logs/AS-P09-B1-reviewer-inventory.txt) accounts for23 edited preimages
and15 new paths; all144 unaffected acceptedp08 paths byte-equal to main delivery.
Archive preimage/current delta hashes independently verified. Original retained
source baseline is acceptedp08 `99d5627b7bc25cb34d828005e9539a07cf5e40eb76c35fb7b1276feeddaa225e`.

- Read retention domain/config, claim/batch/prune/worker in full. Checked strict
  fixed cutoffs, global100-row budget including retirement marks, child-before-parent
  deletion, credential-rotation dependency order, pending/unresolved/leased checkout
  and financial protection, advance-before-attempt scheduling, poison fairness,
  per-statement and transaction deadlines, cancellation and startup JoinSet ownership.
- Exhaustively traced order readers/mutators through Trail and exact spans. All four
  analytics financial sources exclude retired orders; detail/list/refund-parent
  consumers and shared settlement order lock exclude them; transaction entry owners
  share the tenant lock. Refund details cannot bypass parent validation. Frozen paid
  dimensions have no raw-click FK, so raw cleanup does not rewrite retained money.
  Audited migration FK graph for unintended cascade work and tombstone mutation.
- Read all new cutoff/retention/signals/restore tests, including actual concurrency
  assertions rather than their names. Read logging producer changes and unchanged
  lifecycle-health owner. Secrets are deliberately absent from emitted fields;
  duplicate/conflict/committed outcomes retain distinct meanings.
- Read operations documentation, configuration examples, all deployment assets,
  production and Openbot fixture Dockerfiles, CI integration, topology orchestrator
  and browser assertions in full. Reviewed exact config producers, extraction and
  existing smoke consumers. Production app is nonroot, TLS trust remains verified,
  private DB roles/networks and no shared application mounts are explicit.
- Independently executed `cargo +1.95.0 test --locked --offline --lib retention --
  --nocapture`: **8 PASS**, including config, cutoff, logging subprocess, deletion,
  poison/cancellation, report race and settlement competitor. PostgreSQL UUID-owned
  fixtures, local Rust1.95, stock2MiB stack. Output
  [reviewer retention](logs/AS-P09-B1-reviewer-retention.txt).
- Independently executed the real dump/restore test: **1 PASS**. Restored merchant
  verifier, encrypted session, paid/refund ledger, checkout, pending settlement,
  terminal fence and bidirectional cross-role denial. Local measurements dump75ms,
  restore81ms, total185ms; not production objectives. Output
  [reviewer restore](logs/AS-P09-B1-reviewer-restore.txt). No reviewer process remains.
- Inspected author logs/assertions for release image build, nonroot/read-only smoke,
  clean service extraction with formatting/offline-all-target compilation, migration
  and metadata checks,13 startup +2 storage tests,15 contracts,11 boundary regressions,
  36 commerce/analytics tests, dashboard browser, redirect and12 lifecycle regressions.
  Reused unchanged acceptedp08 browser/report/contract review where applicable. The
  extracted foundation log proves actual isolated paths; it is not a full suite.
- Manifest validator independently passes13 declared PASS/3 historical FAIL entries;
  that only establishes retained file identity. Inspected relevant failed fixture
  attempts, TLS leaf/CA correction, disk-exhaustion and SQL-boundary failure records.
  Found current gateway assertions absent from topology11 output, yielding F1.
  Root's subsequent topology13 run failed at checkout, exposing F2. No separate-network
  allowlist proof from that run exists, and historical topology11 cannot prove the
  current final fixture. AC6/AC7 remain unverified for the current artifact pending
  correction/recheck; no claim of whole p09 PASS.

### Limits and next review

No full repository/service suite run in B1; final combined gates belong to p10,
including inherited p03 session-fixture clock correction. Openbot release-image
OOM/SIGKILL remains a retained failure: actual debug integration image is not a
release-build proof. Attribution production release image is proven. Topologies
simulate separate networks locally; DNS approval and forced durable freshness use
explicit fixture controls, with actual DNS and exact monotonic boundaries covered
by their accepted owning tests. Production rollout, region/retention/SLO approval,
native adapters and real-store pilot remain outside these claims. Current topology
negative trust probe uses OpenSSL; positive application HTTPS and earlier invalid
leaf callback failure provide distinct transport evidence, never equated with it.
Root increased only task-owned VM RAM6GiB→12GiB during attempted rerun; no application
bound or stock stack/lease budget changed. Reviewer did not operate that VM.

Batch size was cohesive and reviewable. The schema/ownership gate was warranted;
no new architecture decision is needed for these findings. Concrete process lesson:
record an execution after the final fixture assertion edit and use one clock for
causally ordered integration timestamps. Skill already requires current behavioral
evidence; no broad instruction rewrite is supported. Recheck should review the
fixture-only delta and current topology evidence, reuse unchanged production/test
coverage above, and verify the new complete snapshot identity. Preserve failed runs.


## AS-P09-B1-N2 — correction recheck — 2026-11-16

**PASS AS-P09-B1-R2** for source
`542b32ae822314eb5a16050b153fa4e7a625cfc7380d2b66a9298d79fbdb5d9d`.
**AS-P09-B1-F1/F2 CLOSED**, no new actionable findings.

Identity: implement-astra-reviewed-2-3; p09, retained independent reviewer
/root/attribution_reviewer requested Astra/high, actual telemetry unavailable.
Same assigned isolated worktree, HEAD/start/base6e55258cbbdc7633940805f366fb17fe240f02aa,
D1/r1, exact10 record exclusions and main coordination authority revision26
C001–C008/r1 as N1. [Identity](logs/AS-P09-B1-R2-reviewer-identity.txt) verifies
start/end snapshot, unchanged HEAD, byte-equal main coordination pin and diff check.
[Inventory](logs/AS-P09-B1-R2-reviewer-inventory.txt) independently reconstructs both
R1 fixture preimages from the exact reversed correction, verifies their R1 hashes,
checks all other R1 source hashes and accounts for the single new learning.

Rechecked the changed fixture action, response parser and every timestamp consumer;
read the new Trail learning. A fixed SQL read returns an RFC3339 UTC timestamp from
the same DB clock that records clicks, after each prior effect. No secret, tenant or
production time endpoint was introduced. Existing fixture-only control isolation
and bounds remain intact. Host-clock management timestamps/report windows have
existing generous horizons and do not carry the strict click/payment/refund ordering
at issue. No production runtime, migration, contract or budget changed.

Read the current network assertion paths and the entire topology14 output against
original09B/AC6–7. Both layouts complete the actual Openbot HTTPS bridge, independent
sessions, scoped setup, durable redirect, consent checkout, payment replay, refund,
API/dashboard ledger, warm outage, forced expiry, cold restart and reconciled
recovery. Separate-network layout proves non-bridge HTTPS403 in both directions
in addition to private DB/plaintext isolation and absent application mounts.
The log's exact success text now matches current assertions. Same base Openbot debug
and attribution release image identities as R1; trust-only layers shared by both
layouts. [R2 manifest](logs/AS-P09-B1-R2.evidence.json) independently validates1 PASS
and2 historical FAIL; [topology14](logs/AS-P09-topologies-14.txt) hash verified.
Reviewer inspected this execution; did not independently rerun the expensive
container journey or operate root-owned VM/build resources.

Reuse N1's whole38-path production review, independent8 retention/config/signals/race
passes and1 real restore pass, plus inspected foundation/migration/offline/build/
smoke/contracts/browser/lifecycle evidence: all affected runtime and fixtures are
unchanged. No redundant Rust suite run for this fixture-only correction. Original
review findings and failed attempts remain preserved. All p09 focused criteria are
now supported within the explicit fixture limits; root acceptance/integration and
p10 final full-suite gate remain separate. The Openbot production release-image
build is still not established by the debug fixture; root's new build is in progress
and not relied upon here. No production rollout/native pilot or policy approval.

The correction is appropriately scoped; no gate reopening needed. Candidate lesson
from F2 is accurately retained in Trail. F1 reinforces that exact current assertions
need matching execution evidence; manifest retention validation is insufficient by
itself. No additional skill/AGENTS change supported by this single recheck.
