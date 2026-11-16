# Attribution p03 independent review notes

## AS-P03-G1-N1 — 2026-11-16 — Architecture gate

Workflow implement-astra-reviewed-2-3; phase p03; batch covers phase03 A/B/C as one
SSO/current-authority/lifecycle milestone. Task [TASK.md](TASK.md), result
[REVIEW.md](REVIEW.md). Reviewer `/root/attribution_reviewer`, Astra/high requested;
actual model/effort and context telemetry unavailable. Independent initial context,
retained reviewer across sequential phases. No reviewer source/design edits.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
attribution-service. Starting/review-base/HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Source digest unchanged:
`f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`.
Exact exclusions are the p01/p02/p03 phase docs directories and plan-root
COORDINATION.md, as spelled out in REVIEW. Decision AS-P03-D1/r1 SHA256
`d139936c19eb01c044b924d7a3971b41777f93f87b68c82c99bbba67cf7c8b0f`.
Authoritative coordination absolute path
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision4; notices AS-C001/r1 and AS-C002/r1 reviewed. Retained copy
[AS-P03-G1.coordination.md](logs/AS-P03-G1.coordination.md) matched at review end;
decision hash and source [end snapshot](logs/AS-P03-G1-reviewer-end.snapshot.txt)
rechecked. No applicable notice changes.

### Coverage and evidence

Independently reviewed original phase03 requirements, accepted p01 network contract
and decision prerequisites, all of r1, and TASK's AC1–AC6 verification mapping.
Applied repository/source/application/HTTP/infra/persistence guides; retained the
root overrides of obsolete compatibility/shared-reset suggestions. Trail discovery
preceded bounded source checks. Exact crux/source evidence included:

- `src/adapters/persistence/user_lifecycle.rs:31–70,74–118`: account-first and
  sorted company/membership locks, real owner-account company deletion and cleanup.
- `src/adapters/persistence/company.rs:104–138`: direct company lock/cleanup/delete
  transaction. A company-delete trigger covers both owners while leaving existing
  cleanup in place; outbox/tombstones must survive deletion and rollback together.
- `src/adapters/persistence/session.rs:38–68` and existing session-security tests:
  current revision, session revocation, real password/email changes; proposed
  code lock ordering and grants are tied to that authority rather than a second
  account-status cache. Existing admitted-request semantics were considered.
- `src/adapters/http/routes/user.rs:39,71–93`: common signed-in landing and current
  invitation/onboarding/UI routing support the single continuation seam. Recovered
  the complete index body after tool compression; did not treat elision as source.
- `src/infra/setup/workers.rs:3–9`, existing session cleanup supervision and
  TASK's composition/caller inventory: retained-worker ownership is available.
  Full production composition remains an implementation/final integration check.

Challenged request and browser binding, one-use code/continuation transactions,
live actor/session/client identity, separate credential classes, local logout during
outage, encryption/AAD/rotation, pagination, role/membership checks, tenant scoping,
idempotent provisioning versus deletion, retained terminal versions, trigger/outbox
ownership, fencing/retry exhaustion, locked tenant writes, and lifecycle freshness.
The concrete remaining design defect is F1. Shared CSRF middleware's Authorization
bypass is already explicitly addressed in TASK's cookie-status-route requirement;
implementation review must prove that boundary rather than assuming middleware
alone enforces it.

The verification plan includes real competitors and actual Openbot login handlers,
use cases and session persistence across two origins, alongside service contract
fixtures; fixture-only claims must not replace those browser paths. AC4's elapsed
clock/recovery mapping is incomplete as detailed in F1. No implementation or runtime
tests were run by B for this architecture-only gate; no evidence manifest is due
yet. The root's existing offline baseline compile failure (61 E0659 errors, retained
AS-P03-root-baseline-check.txt) remains an explicit verification limitation, not a
new regression or architecture finding. Its unrelated visibility fix is not
authorized by this review. No migrations, DBs, ports, or processes were created.

### Outcome and process feedback

**Changes required for AS-P03-D1/r1**: [AS-P03-G1-F1](agent-review-findings.md),
medium/P2. No code, runtime or phase acceptance. Recheck the corrected decision and
elapsed-age/recovery test mapping, reusing unchanged covered architecture evidence.

The cohesive architecture gate is warranted: identity authority, schema ownership,
transaction boundaries and lifecycle recovery are expensive to change afterward.
The batch is reviewable; splitting by layer would obscure those boundaries. The
immutable decision and pinned coordination made requirements traceable. No skill or
AGENTS change is supported at this stage; retain F1 for later instruction analysis.

## AS-P03-G1-N2 — 2026-11-16 — Architecture correction recheck

Workflow implement-astra-reviewed-2-3; phase p03; [TASK](TASK.md),
[REVIEW](REVIEW.md). Reviewer `/root/attribution_reviewer`, Astra/high requested,
actual model/effort/context telemetry unavailable; same independent reviewer and
worktree as N1. Starting/review-base/HEAD
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`; source unchanged at
`f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d`.
Exact exclusions remain p01/p02/p03 phase docs and plan-root COORDINATION.md,
spelled out in REVIEW. [Retained source check](logs/AS-P03-G1-r2-reviewer.snapshot.txt).

Decision [AS-P03-D1/r2](logs/AS-P03-D1-r2.md), SHA256 independently matched to its
retained hash: `6968a1aa538ec5817939a92198ff2f111da8a581ac7bd17a1d2529645430b722`.
Authoritative coordination remains
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision4, notices AS-C001/r1 and AS-C002/r1; pinned G1 copy matched
byte-for-byte at the recheck. No requirement changes.

Read full r2 and compared it directly with immutable r1: only revision metadata and
the freshness mechanism/test criteria changed. Reused all unaffected N1 architecture
evidence. Rechecked F1 against the accepted README's five-minute elapsed-age rule,
including response ordering and lifecycle/ticket concurrency implications.

The mandatory conjunction of process-local monotonic tickets and persisted current
active state/version/deadline removes wall-clock rollback as an extension mechanism.
Cold processes cannot reconstruct authorization from stored wall timestamps. The
lock-acquisition deadline recheck prevents a request from waiting past expiry and
then writing under a pre-wait ticket. Durable sequence ordering does not confer
freshness by itself; only a successful trusted observation may mint a local ticket.
Bounded eviction and cold refresh trade availability for a fresh authority read,
preserving the fail-closed requirement. Live authority on user operations and
terminal lifecycle semantics remain unchanged.

TASK's r2 correction entry and r2's deterministic criteria explicitly require
rollback/elapsed expiry/no write, restart/second process, stale sequence, delayed
response, eviction and lock-wait expiry coverage. No new implementation exists;
no runtime tests or DB checks executed in this recheck, and no evidence manifest
is due for the architecture-only gate. Root baseline compilation limitation remains
unchanged and is not waived. Root's newly owned baseline DB is recorded in TASK;
B allocated or changed no resources.

**PASS for AS-P03-D1/r2**, resolving AS-P03-G1-F1 at design level. No remaining
architecture findings; no implementation/phase/runtime acceptance. The correction
was cohesive and limited to the material finding, so a whole-gate re-review was
unnecessary. No additional instruction change is supported; preserve the linked
failure and resolution as later retrospective evidence.

### Gate follow-up — 2026-11-16

AS-P03-B1-F4 later identified that durable lifecycle/freshness state has no alert
consumer. G1-N1/N2 examined persistence/recovery but did not require a concrete
surface for the original phase03 alert obligation. This does not rewrite the
original architecture outcome; implementation acceptance now requires F4 resolution.

## AS-P03-B1-N1 — 2026-11-16 — Whole implementation review AS-P03-B1-R1

Workflow implement-astra-reviewed-2-3; phase p03, full A/B/C batch. Reviewer
`/root/attribution_reviewer`, Astra/high requested, actual model/effort/context
telemetry unavailable; retained independent reviewer context. Author `/root`,
user-confirmed Sol6.1/xhigh. [TASK](TASK.md), [result](REVIEW.md),
[findings](agent-review-findings.md). No agents spawned and no implementation edits.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
attribution-service. Phase starting/review-base/current HEAD:
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Frozen digest:
`45fc0ad49012ab1f6f58a619c06508430011b0c3a127683196c79ea906c71897`.
Exact exclusions: `plan/attribution-service/phases/p01/docs`,
`plan/attribution-service/phases/p02/docs`, `plan/attribution-service/phases/p03/docs`,
`plan/attribution-service/COORDINATION.md`. Start/end snapshot logs retained.
Prerequisites p01 f61d9149 and p02 f4421ac9 are unchanged accepted work; reviewed
integration effects without repeating their unchanged whole-phase review.
Decision AS-P03-D1/r2 remains the gated design; F1 from G1 is resolved and its
monotonic/locked-writer behavior now has exercised regression evidence.

Authoritative coordination path
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision4, notices AS-C001/r1 and AS-C002/r1. Pinned
`logs/AS-P03-B1-R1.coordination.md` matched byte-for-byte; final identity/freshness
checks are retained in the end-identity log. No applicable notice changed.

### Coverage

Read original phase03 requirements, accepted contract README and both HTTP schemas,
TASK criteria/impact mappings and manifest; reused the previously read applicable
root/src/application/persistence/HTTP/infra instructions and read the independent
service guide. Used Trail discovery, skeletons and exhaustive literal searches
before bounded source reads; reviewed actual sources rather than author summaries.

- Root code/continuation/grant flow: live account/session locks, one-use code and
  continuation transaction, PKCE/callback/client binding, immutable session actor
  composite FK, token expiry, revoke/security-revision joins, HTTP authentication
  before parsing, strict request types, origin/CSRF and distinct credential classes.
- Root membership/status producer and pagination/cursor signing, company/account
  deletion triggers and immutable monotonic outbox, retry exhaustion, live execution
  fences, cancellation and task ownership. Existing cleanup owners remain in place.
- Service PKCE/login consumption and encrypted session rotation, purpose/record/key
  AAD, return paths, cookies/CSRF, upstream response validation and deadlines,
  role/scoped access, HTML consumer and logout/browser recovery.
- Local lifecycle/provisioning races, terminal tombstones, sequence observations,
  process-local monotonic freshness, cold/evicted instances, actual post-lock expiry
  checks, reconciliation fences and missed-deletion repair. Later commerce writes
  must use the provided locked boundary; merchant ingestion is not implemented here.
- Service/root startup composition and shutdown ownership; optional root vs required
  service configuration, key separation/rotation, SQL schema constraints, source
  boundary scanner delta, browser/CI wiring and deployment documentation integration.
  Root authorized helper visibility delta has no logic change and retained focused
  reconciliation regression evidence. No unrelated code correction was made by B.

### Executed and inspected evidence

The author's [manifest](logs/AS-P03-B1-R1.evidence.json) passed check-evidence.py
against the frozen digest: 15 PASS, five retained historical FAIL, zero declared
NOT EXERCISED. The baseline compiler failure is superseded after the specifically
authorized helper visibility correction; recorded fixture/environment failures have
current passing replacements. This structural validation is not behavioral acceptance.

B independently executed:

- Service `cargo test --locked --offline --lib attribution_`: **33 passed**, real
  UUID-owned databases, [log](logs/AS-P03-B1-R1-reviewer-service-tests.txt).
- Root `cargo test --locked --offline --lib adapters::persistence::attribution::`:
  **eight passed**, owned p03 test configuration and per-test isolated databases,
  [log](logs/AS-P03-B1-R1-reviewer-root-tests.txt). Existing unrelated unused import
  warning retained. Used the assigned root cache/profile/offline environment.
- Boundary checker `python3.13 services/attribution/scripts/test_boundary.py`:
  **ten passed**, [log](logs/AS-P03-B1-R1-reviewer-boundary-tests.txt).
- Real standalone-server session probe with HTTP authority stand-in: confirmed
  non-idempotent logout retry and unusable expired-cookie shell, F1/F2. A second
  probe confirmed schema-valid maximum CompanyPage rejected as503, F3. Sources and
  output retained in logs as reviewer-session-probe and reviewer-page-probe; schema
  validation result separately retained. These are diagnostic confirmations of
  defects, not successful acceptance tests. URLs/tokens/credentials were not logged.
- Canonical vs service identity schema comparison and `git diff --check`: pass.

Inspected actual assertions and retained author results for the existing 25 session
security tests, company access, vault/memory account cleanup, four helper regressions,
eight source-module checks, root offline/SQLx metadata checks and standalone extracted
migration/format/offline/contract checks. Reviewed actual real-browser fixture and
seven checks: real password login, continuation, existing-login path, cookies,
company navigation, local logout/rotation and upstream revocation. It uses real
Openbot handlers/use cases/session PostgreSQL plus the actual standalone service;
unrelated Openbot runtimes are omitted. B did not repeat the browser run, full root
suite, container build or production deployment. Full integration/full-suite scope
remains p10; no p03 container proof is borrowed from p02.

The existing competing claim/redemption tests exercise real SQL, and the waiting
tenant-writer test checks pg_blocking_pids before expiry/suspension and asserts no
mutation. The root access test and stub consumer coverage do not fulfill real
producer membership-change/foreign-existing-company criteria (F5). Persistent failed
rows are tested but alert visibility is not implemented/exercised (F4). These are
explicit gaps despite the author's broad AC3/AC5 PASS mapping.

All B-owned probe servers/processes stopped and UUID fixture roles/databases were
dropped by their context managers. Test commands completed. Shared author caches
and p03 root database were preserved; no other worktree resources touched.

### Outcome and process feedback

**Changes required**, AS-P03-B1-F1 through F5, all P2/medium. No implementation or
phase acceptance. One correction batch should address all five and retain evidence
showing each original failure no longer occurs. Recheck affected contracts/consumers
and required tests without rerunning unrelated full suites by default.

The cohesive batch was reviewable and necessary to follow both HTTP applications
and lifecycle recovery. Material misses came from acceptance assertions stopping
before their consumer or substituting upstream responses for producer mutations:
logout lost-response behavior, HTML expiry recovery, maximum wire page and operational
alert reader. The current verification-contract guidance already covers these;
retain this evidence for retrospective analysis rather than adding broad new rules.

## AS-P03-B1-N2 — 2026-11-16 — Correction recheck AS-P03-B1-R2

Workflow implement-astra-reviewed-2-3; phase p03. [TASK](TASK.md),
[REVIEW](REVIEW.md), [original findings/resolutions](agent-review-findings.md).
Reviewer `/root/attribution_reviewer`, Astra/high requested, actual model/effort/
context telemetry unavailable, same independent reviewer retained. Author `/root`,
user-confirmed Sol6.1/xhigh. No source edits or subagents by B.

Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
attribution-service. Phase start/review-base/current HEAD remain
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Frozen artifact
`fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`;
exact exclusions remain `plan/attribution-service/phases/p01/docs`,
`plan/attribution-service/phases/p02/docs`, `plan/attribution-service/phases/p03/docs`,
`plan/attribution-service/COORDINATION.md`. Start/end snapshots retained under
`logs/AS-P03-B1-R2-reviewer-*.snapshot.txt`.
Authoritative coordination is
`/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/COORDINATION.md`,
writer `/root`, revision4; AS-C001/r1 and AS-C002/r1 unchanged, pinned R2 copy matched.
Gated AS-P03-D1/r2 unchanged. Unaffected R1 scope/evidence remains applicable.

### Correction coverage

Read the corrected production paths and decisive test assertions after Trail
discovery; did not treat the author's resolution summary as proof.

- **F1 fixed:** logout-specific retained lookup preserves encrypted CSRF checking
  on replay while authentication still requires a live session; both first/repeat
  successful logout return contract302 with cookie removal, locally before bounded
  upstream revoke. Wrong CSRF remains403. Tested consumer JS follows the redirect.
- **F2 fixed:** only definite Unauthorized from shell loading becomes sign-in HTML
  with local-cookie removal. Expired/revoked session API behavior remains401;
  authority outage remains503 without cookie removal. Root/service real-browser
  fixture now reloads after upstream logout and asserts sign-in plus absent cookie.
- **F3 fixed:**128KiB hard cap paired with schema-derived maximum names/page/cursor
  payloads using Unicode and worst-case JSON control escapes;80% budget guard and
  valid-JSON over-limit rejection remain in CI's service test selection. Original
  schema-valid73,530-byte response succeeds. Reviewed scoped learning l-20f04c5f.
- **F4 fixed:** persisted outbox/tenant health queries feed supervised bounded
  startup/30-second operator-log probes; warning, unavailable and recovery states
  are documented. Tests rebuild the bridge/service owner over persisted failures,
  capture actual log fields and prove recovery. No global tenant HTTP surface.
- **F5 fixed:** tests use actual new bridge SQL with retained real grant/session,
  owner/admin/member/foreign companies, production role-change/removal use cases,
  status-write authorization and multi-company cursor traversal before/after removal.
  Consumer stubs remain separately identified. Original AC3/AC5 overclaims now have
  explicit historical/correction dispositions in TASK.

No new persistent ownership, transaction or authorization-model change requires
reopening D1/r2. Logout's retained read is scoped solely to revocation/CSRF and is
not a substitute for normal session authentication.

### Verification and limits

Independently executed:

- Service `cargo test --locked --offline --lib adapters::identity::`:21 passed,
  real invocation-owned PostgreSQL fixtures; [log](logs/AS-P03-B1-R2-reviewer-service.txt).
- Root `cargo test --locked --offline --lib adapters::persistence::attribution::`:
  11 passed, assigned cache/profile/offline environment and owned p03 test setup;
  [log](logs/AS-P03-B1-R2-reviewer-root.txt). Existing unused-import/linker warnings
  retained; no test failure or skip.
- Replayed the original diagnostic cases against the current standalone service
  and real isolated DB using the bounded authority stand-in:
  [source](logs/AS-P03-B1-R2-reviewer-runtime-probe.py),
  [output](logs/AS-P03-B1-R2-reviewer-runtime-probe.txt). Valid maximum page200;
  first/repeat logout302, clearing cookie; expired-cookie shell200/sign-in/clearing.
- `git diff --check`, snapshot identity and pinned coordination comparison: pass.

Validated [author manifest](logs/AS-P03-B1-R2.evidence.json):17 PASS and one retained
superseded formatting FAIL. Inspected actual current service/root assertion logs,
eight-check real-browser result, standalone extraction/foundation/boundary/contracts,
both metadata and offline compilation records, module/config/format records, and
justified reuse of unchanged R1 session/company/deletion/helper evidence. No test
was removed. B did not rerun unchanged full suites or the browser/container gates;
the current browser result was inspected, while current container/full-plan
verification remains later-phase scope. Hosted paging configuration is explicitly
a deployment responsibility; this change supplies durable operator signals.

All independent reruns/probes completed. Probe HTTP servers, service processes and
UUID-owned DB roles/databases were cleaned up; assigned caches/root database and
other worktrees' resources were preserved. End identity/log hashes retained.

### Outcome and process feedback

**PASS for `fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`.**
AS-P03-B1-F1–F5 fixed, with no remaining actionable findings. Root owns acceptance
and integration; this is not full-plan/deployment completion.

One cohesive correction round addressed all findings without narrowing accepted
contracts. Consumer recovery tests, actual producer mutations and early wire-budget
guard now discriminate the original failures. The scoped learning is supported by
F3; no additional broad AGENTS/skill change is justified by this recheck.
