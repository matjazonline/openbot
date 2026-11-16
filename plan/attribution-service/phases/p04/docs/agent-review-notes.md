# Attribution p04 persistent independent review notes

## AS-P04-G1-N1 — 2026-11-16 — Architecture gate AS-P04-G1

Workflow implement-astra-reviewed-2-3; phase p04, cohesive 04A–04B architecture.
[TASK](TASK.md), [REVIEW](REVIEW.md),
[immutable AS-P04-D1/r1](logs/AS-P04-D1-r1.md).
Reviewer `/root/attribution_reviewer`, Astra/high requested, same independent B
retained across phases. Actual model/effort/context telemetry unavailable.
Author `/root`, user-confirmed Sol6.1/xhigh. No source edits or subagents by B.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
`attribution-service`; phase start/review-base/current HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`.
Decision SHA256
`ab5267ed4b857447ccae0fdf1a6b83dffa0fce2ffd646dd44790cdea01b89d85`.
Frozen source is unchanged accepted p03
`fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`.
Exact exclusions: `plan/attribution-service/phases/p01/docs`,
`plan/attribution-service/phases/p02/docs`,
`plan/attribution-service/phases/p03/docs`,
`plan/attribution-service/phases/p04/docs`,
`plan/attribution-service/COORDINATION.md`.
Start/end snapshots and end identity check are linked from REVIEW.

Authoritative coordination:
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision5. AS-C001/r1 and AS-C002/r1 apply unchanged; pinned
[revision5](logs/AS-P04-D1-coordination-r5.md) matched byte-for-byte at review end.
Prior p03 acceptance AS-P03-ACCEPT-R1 and independent AS-P03-B1-N2 are reused for
unchanged identity/session/lifecycle foundations, not as proof of future setup code.

### Requirements and architecture coverage

Read original phase04, accepted service contract README/OpenAPI setup operations
and schemas, repository/service instructions, and the complete proposed decision,
AC1–AC8 mapping and traced-consumer table. Reviewed the gate independently of the
author's proposed outcome. All nine decision items were assessed; no section was
accepted solely from its summary.

- Company/store/domain ownership, composite SQL constraints, public installation
  identifier separation, trusted audit identities, reset-only migration and service
  ownership meet 04A.1/3 and 04B.2/5 at the design level.
- DNS TXT control proof uses a fixed resolver boundary and no submitted HTTP fetch.
  Challenged arbitrary resolver/address access, exact IDNA host matching, address
  screening, proof expiry/reverification, terminal revoke, proof obtained outside
  transactions and a later locked state/challenge recheck. The proposed local DNS
  transport fixture and competing verify/revoke assertion are necessary runtime
  evidence; this gate does not establish resolver behavior.
- Live request-entry membership/permission and CSRF remain authoritative for setup.
  Existing `IdentityService::authenticate/company/tenant` at
  `services/attribution/src/application/identity.rs:248–301`,
  `lock_tenant` at `adapters/identity/tenants.rs:10–18`, and session/logout SQL at
  `adapters/identity/postgres.rs:65–78` support the stated ownership boundaries.
  Trail discovery preceded exact source reads; ambiguous caller results were
  supplemented with scoped literal references. No current retained session-then-
  tenant transaction creates a reverse lock order against proposed tenant-then-
  session writes. Membership remains an upstream observation, not a distributed
  lock; no stronger atomic membership guarantee is claimed.
- Receipt identity, normalized body/timestamp conflict, original-time horizon,
  current authority before replay, atomic mutation/audit/receipt and uncertain
  commit503 were checked against published management retries. Credential replay
  means the contract's CredentialReplay; other setup replays retain their declared
  original metadata schemas. Creation's future-expiry check applies to a new
  mutation after receipt lookup, as the author clarified; an otherwise valid replay
  after credential expiry returns the original metadata. These are readings of the
  original contracts, not new exceptions or runtime proof.
- Verifier-only random keys, five typed scopes, nullable expiry, one-store binding,
  rotation overlap/original expiry, one-time secret and metadata-only listing meet
  04B.1–3. Rotation source liveness and receipt replay prevent repeated overlap
  extensions; revoking the replacement does not restore the source.
- Pre-body merchant authentication and trusted tenant/credential transaction entry
  separate admission from the final state recheck. Actual production entry via a
  test-only route is an appropriate p04 test surface before commerce routes exist.
  Real competing revoke/use and disable/use tests must prove serial outcomes;
  sequential mocks would not suffice. The bounded tenant registry and admission
  permits retain existing global64/10s HTTP limits (`adapters/http.rs:103–151`).
- Bound encrypted cursors, setup management/member permissions, accessible forms,
  stable browser retry identity, private one-time secret handling and disjoint
  public/session/merchant/service credentials cover the remaining UI/wire boundary.
  Browser assertions must reach actual setup SQL through HTTP, with fixture
  authority explicitly distinguished from deployed Openbot.

### Verification and limits

Executed architecture artifact identity checks: source snapshot, immutable decision
hash, branch/HEAD, pinned coordination comparison and `git diff --check`. Start/end
evidence is retained in logs. Independently inspected contracts and existing source
boundaries; did not execute p04 runtime tests because no p04 implementation exists.
The author's dependency inspection reports hickory-resolver0.26.3 supports fixed
resolver configuration, bounded attempts/timeouts and disabled system configuration;
this feasibility report is not substituted for later production adapter inspection.

AC1–AC8 are planned, not passing checks: SQL ownership; DNS/URL policy; browser
forms/roles/CSRF; keys and replay; pre-body authentication; concurrent receipts;
revocation competitors; affected foundation/identity/contracts, formatting, offline
compilation, fresh migrations and SQLx metadata. Focused checks and the final p10
whole-plan/container gate are explicitly retained. No implementation evidence
manifest is required for this architecture-only review. External DNS, production
Openbot, deployment, current full suites and p04 runtime behavior remain unverified.

### Outcome and process feedback

**PASS for AS-P04-D1/r1**, SHA256
`ab5267ed4b857447ccae0fdf1a6b83dffa0fce2ffd646dd44790cdea01b89d85`.
No actionable findings or unresolved gate questions; no findings file is needed.
Implementation may proceed under the original requirements and contracts. Root
retains implementation/phase acceptance ownership.

One cohesive gate is appropriate because scoped ownership, receipts and revocation
share transaction boundaries. Its criterion/consumer mappings expose the required
test surfaces without prematurely adding commerce APIs. No instruction change is
supported by this architecture inspection. No reviewer-owned processes, fixtures
or resource cleanup remain.

## AS-P04-B1-N1 — 2026-11-16 — Whole-phase implementation review

Workflow implement-astra-reviewed-2-3, p04 batch04A–04B. [TASK](TASK.md),
[REVIEW](REVIEW.md), [findings](agent-review-findings.md).
Reviewer `/root/attribution_reviewer`, Astra/high requested, retained independent B;
actual model/effort/context telemetry unavailable. Author `/root`, user-confirmed
Sol6.1/xhigh. B wrote only assigned review artifacts; no source edits or subagents.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
attribution-service; phase start/review-base/HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Frozen artifact
`6837fba3f8f66fb0160bf308ecc0668d25f33be5a70ad3b65a2895fbedea0c0f`.
Exact exclusions remain the five literal paths recorded in G1. Start/end snapshots
and identity check: `logs/AS-P04-B1-reviewer-{start,end}.snapshot.txt` and
`logs/AS-P04-B1-reviewer-identity.txt`.
AS-P04-D1/r1 remains the accepted architecture, unchanged SHA256
`ab5267ed4b857447ccae0fdf1a6b83dffa0fce2ffd646dd44790cdea01b89d85`.

Authoritative coordination is
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision6; pinned `logs/AS-P04-B1-coordination-r6.md` compared equal.
AS-C001/r1 and AS-C002/r1 unchanged. AS-C003/r1 publishes reusable setup boundaries
to future phases only after p04 acceptance, which this review does not grant.
Unchanged p01–p03 evidence is reused for established foundations, including p03
AS-P03-B1-N2 and accepted `fd40f480...`; no root producer check was warranted by
the service-scoped changes and CI browser-step addition.

### Coverage and challenged invariants

Independently followed original04A/04B and accepted README/OpenAPI through the
application models, schema, PostgreSQL adapter, HTTP parsing/router, merchant entry,
DNS resolver, admission limits, rendered shell/assets, tests and browser consumer.
Trail discovery/skeletons preceded exact source reads; exhaustive scoped references
checked StoreUpdate test coverage. Inspected the complete setup production paths,
all16 setup test cases, browser fixture/journey and integration composition. Reused
unchanged identity/crypto internals; did not uniformly rereview unrelated earlier
phases or the entire dependency implementation.

- **AC1:** company predicates, composite store/domain/install/key ownership,
  cursor company/kind/filter binding and foreign-resource404; public identifiers
  cannot satisfy credential/session syntax or checks. SQL constraints and real
  foreign-company/store fixtures reinforce application checks.
- **AC2:** maintained fixed recursive DNS configuration, disabled hosts/cache,
  qualified names, two-second total bound, independent A/AAAA failure handling,
  exact TXT/public-address policy, IDNA URL policy and no submitted HTTP fetch.
  Approval/reverification/revoke use shared tenant locking and recheck proof/state
  after network work. Reviewed local DNS packet fixture, mixed public/local answer,
  partial-family timeout, expired/revoked installation and verify/revoke competitor.
- **AC3:** session/company live authority, permissions/CSRF, strict bounded typed
  JSON, output escaping/text nodes, stable retry key/time/body, secret clearing and
  member controls. Browser reaches real setup HTTP/SQL/DNS through fixture authority;
  its eight checks do not establish all possible error/retry/pagination journeys.
  Store-edit and pagination wiring inspected. Public proof retrieval after navigation
  is not a separately exercised browser recovery feature.
- **AC4/AC6:**256-bit secret/verifier storage, five typed scopes, one-store ownership,
  metadata-only lists/receipts/audits, original expiry/overlap cap, rotated-source
  guard, same receipt replay and no overlap restoration. Tenant serialization makes
  concurrent exact receipts one mutation/audit/receipt and one secret response.
  Timestamp precision retained as normalized UTC text; changed payload/time conflict,
  old/pruned horizon and current authority/logout enforcement inspected/tested.
  The expired-original-payload case is not established: F2.
- **AC5/AC7:** merchant headers and local scope checked before adapter body parsing,
  current monotonic company freshness reused, tenant→credential recheck, enabled
  store, last-use plus effect commit, bounded per-tenant permits and cleanup.
  Test-only ingestion route uses the actual adapter and transaction with a fixture
  audit effect; it is not a future commerce route and omits the outer production
  router. The outer body-size layer remains unchanged and can reject declared
  oversized input before inner authentication without consuming/parsing it. Future
  phases must verify their full route composition under AS-C003. Revoke/use and
  verify/revoke competitors are exercised; store-disable/use is only sequential: F1.
- **AC8:** setup composition in standalone startup/global boundary, safe failure
  mapping, reset-only initial migration, service Cargo dependencies, existing UI
  shell and CI browser execution. Actual existing limits are32 non-health requests
  and64 connections; TASK correctly records the historical gate wording typo.
  Read the new label learning l-73b4ae1a and its failure/correction disposition;
  explicit select label and [hidden] CSS match the passing actual browser consumer.

### Executed, inspected and unavailable evidence

Independently executed from the service cwd with Rust1.95.0, offline mode,
`/tmp/as-attribution-target` and invocation-owned PostgreSQL fixture resources:

- `cargo test --locked --offline --lib setup`:16 PASS, one explicitly ignored
  browser test; [output](logs/AS-P04-B1-reviewer-setup.txt).
- `cargo test --locked --offline --lib attribution_setup_browser_ -- --ignored --nocapture`:
  one PASS/eight browser checks using pinned Playwright;
  [output](logs/AS-P04-B1-reviewer-browser.txt). Browser processes and fixture DB
  resources are closed by the completed harness; no reviewer-owned server remains.
- Manifest validation with expected artifact:11 retained PASS entries;
  [output](logs/AS-P04-B1-reviewer-manifest.txt). Identity/diff checks and retained
  source excerpts support the review; a valid manifest does not resolve F1/F2.

Inspected [author manifest](logs/AS-P04-B1.evidence.json) and actual successful logs:
21 identity tests,14 foundation tests, fresh migration/SQLx preparation and check,
formatting/offline all-target compilation,10 boundary checker tests and12 contract
tests. Earlier compilation/browser failures and their dispositions remain in TASK
and immutable logs. No tests were removed. B did not rerun unchanged full/root
suites or current extraction/container gates. Public recursive DNS deployment,
production Openbot, full-plan p10 and real-store/native adapter behavior remain
unverified/deferred. SQL response shape tests deserialize service models; OpenAPI
runtime compatibility was independently inspected rather than inferred from that
deserialization alone.

### Outcome and process feedback

**Changes required** for `6837fba3f8f66fb0160bf308ecc0668d25f33be5a70ad3b65a2895fbedea0c0f`:
AS-P04-B1-F1 and F2, both verification defects. No unsafe production behavior was
demonstrated; intended locking and receipt ordering look correct on inspection.
Recheck the missing discriminating cases together, preserving all current evidence.
Root retains phase acceptance; no runtime/full-plan/deployment acceptance follows.

The cohesive phase was reviewable. Its shared transaction owner supports reusing
unaffected evidence, but an adjacent green case must not erase a specifically
planned concurrency case or a time-dependent payload boundary. Existing skill
verification-contract guidance already states this; no broader instruction change
is supported. The architecture gate correctly required the cases, so no historical
gate correction is warranted.

## AS-P04-B1-N2 — 2026-11-16 — Correction recheck AS-P04-B1-R2

Workflow implement-astra-reviewed-2-3, p04. [TASK](TASK.md), [REVIEW](REVIEW.md),
[findings and dated resolutions](agent-review-findings.md).
Same independent reviewer `/root/attribution_reviewer`, Astra/high requested;
actual model/effort/context telemetry unavailable. Author `/root`, user-confirmed
Sol6.1/xhigh. No source edits or subagents by B.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
attribution-service; phase start/review-base/HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Corrected source
`a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`.
Exact five exclusions remain those recorded in G1/B1. Start/end snapshots and
identity output retained under `logs/AS-P04-B1-R2-reviewer-*`.
Architecture AS-P04-D1/r1 and its SHA256 are unchanged.

Authoritative coordination remains plan-root COORDINATION.md, writer `/root`,
revision6; pinned B1 revision6 matched byte-for-byte. AS-C001/r1, AS-C002/r1 and
future-phase AS-C003/r1 remain unchanged. No dependent phase implementation is
established by this recheck.

Correction scope: two appended setup tests, their bounded PostgreSQL blocker
observer, and phase04 evidence/count records. Reused unaffected B1 production,
contracts, browser, foundation and identity coverage; no production change or new
architecture gate was necessary. Trail discovery/skeletons preceded complete reads
of `setup/http_tests.rs:1420–1623`. Both corrections use the actual production
mutation/transaction entry, not substitutes for their behavior.

- **F1 fixed:** both store-disable/use orderings are actual competing transactions.
  `pg_blocking_pids` observes the blocker/contender chain in the invocation-owned
  database; production disable holds tenant and waits on a fixture-held session row
  before merchant is admitted to the same lock queue. After release the waiter
  rejects disabled state and cannot advance last_used_at. The reverse ordering
  allows held use to commit before production disable, then denies later use.
- **F2 fixed:** original two-second expiry is derived from database time; the test
  waits until the database crosses it without changing payload, receipt or key row.
  Exact replay is metadata-only/same ID/no new key or receipt. New identity with
  that expired payload is Invalid, proving the time-dependent branch is reached.

Independently executed `cargo test --locked --offline --lib setup` in the service
cwd with the same Rust1.95.0/offline/cache/invocation-owned PG configuration:
**18 PASS**, one browser test explicitly ignored in this selection;
[output](logs/AS-P04-B1-R2-reviewer-setup.txt). Browser evidence is reused from the
unchanged B1 journey, including B's independent eight-check pass. Inspected current
individual F1/F2 and aggregate author output, current formatting/offline logs, and
validated the [R2 manifest](logs/AS-P04-B1-R2.evidence.json):13 retained PASS entries.
Prior metadata/migration/identity/foundation/contract/boundary evidence is reused
because its production inputs are unchanged. No tests were removed. No full/root,
container, public DNS or production deployment rerun was required for this test-only
correction; those later-phase limitations from B1 remain.

**PASS for `a5e729a610b120f50fb43acddf1c3bd7cda8c7b8a3cc5ae8d394e971607c7275`.**
F1/F2 fixed, no remaining actionable findings. Root owns phase acceptance and
integration. Completed fixture harness cleaned its resources; no reviewer process
or server remains. The focused correction provides discriminating coverage without
changing protocol or expanding scope. Existing verification guidance suffices; no
new instruction is supported by this recheck.
