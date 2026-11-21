# Attribution p03 — Openbot SSO, current company authority and lifecycle

- Workflow: implement-astra-reviewed-2-3, installed at
  `/Users/mac02/orca/workspaces/openbot/attribution-service/.agents/skills/implement-astra-reviewed-2-3`.
- Root/A `/root`: user-confirmed Sol6.1/xhigh; B `/root/attribution_reviewer`:
  independent Astra/high requested, reused after p02 completion. Actual worker/context
  telemetry unavailable; retain host compaction defaults with checkpoints.
- Worktree `/Users/mac02/orca/workspaces/openbot/attribution-service`, branch
  attribution-service, starting/review-base/HEAD 2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee.
- Original entry tree clean; preserve sequential uncommitted p01/p02 deliverables.
  Prerequisites p01 f61d9149b78e8f3cbf8876ac1362089561e0d76f2d2e67b5373a7ec801dd6a02
  (AS-P01-B1-N2, D1/r2 +D2/r1); p02
  f4421ac9eb35f50acf92d710948bf4e3ca777104821000a4c0e296d1ac53c10d (AS-P02-B1-N2).
- Authoritative coordination absolute plan-root COORDINATION.md, writer root,
  revision4 checked at startup. AS-C001/r1 (API-first/Openbot bridge), AS-C002/r1
  (approved permissions/consent/freshness/503) acknowledged; no pending requests.
- Record root `/Users/mac02/orca/workspaces/openbot/attribution-service/plan/attribution-service/phases/p03/docs`.
- Exact snapshot exclusions: `plan/attribution-service/phases/p01/docs`,
  `plan/attribution-service/phases/p02/docs`, `plan/attribution-service/phases/p03/docs`,
  `plan/attribution-service/COORDINATION.md`. Include all implementation/contracts/docs.
- Scope/originals: phase03 plan plus accepted network contracts in
  services/attribution/contract/README.md, both OpenAPI documents and root canonical
  docs/contracts/attribution-identity.openapi.json. All three plan batches comprise
  one cohesive SSO/authority/lifecycle milestone, with an architecture gate before
  dependent schema/protocol implementation. Do not silently weaken runtime criteria.
- Implementation is complete pending the remaining verification and independent batch review.
  No p03 digest/review/acceptance/integration yet. p04–p10 wait for accepted predecessor.

## Acceptance and planned verification

| Criterion | Production path/invariant | Decisive assertion / focused check |
| --- | --- | --- |
| AC1 / 03A | Code+PKCE with verified Openbot actor/session/client/callback and browser transaction | attribution_sso_: concurrent redemption has one winner; state/correlation/verifier/callback/audience/authority failures; expired/replayed code; actual browser redirects for already/newly authenticated user |
| AC2 / 03A | Opaque attribution sessions, encrypted grant, separate cookies, local logout and live upstream authority | attribution_session_: fixation rotation, idle/absolute/provider expiry, no parent cookie domain, CSRF denial, grant encryption AAD/key handling, upstream revoke/security-change/account deletion, authority outage distinct503 |
| AC3 / 03B | Every protected user operation checks current company permissions, company-scoped selector/lookup | attribution_company_access_: foreign tenant IDs, multi-company, lost membership, role downgrade, forged actor, concurrent provisioning, timeout, separate DB roles and session cookies |
| AC4 / 03C | Monotonic lifecycle/tombstone, status-request-start freshness <=5min, shared tenant write lock | attribution_company_lifecycle_: competing deliveries/equal-version conflict, delayed provisioning versus delete, stale delivery cannot refresh, expiry fails closed, missing delivery repaired by bounded reconciliation |
| AC5 / 03C | Openbot lifecycle+outbox commit with direct company and owner-account deletion; durable supervised retries | real owning deletion port, rollback/no message, post-commit notification, preserve integration/memory cleanup, competing claimants/lease loss/shutdown, poison-batch bound and persistent unresolved state |
| AC6 / impacted interfaces | Both applications remain independently buildable and portable over published HTTP | focused original Openbot session/revocation/company/account-deletion regressions; service boundary/extracted checks; fmt/offline all-target compilation, fresh migrations/SQLx metadata; retained browser/HTTP evidence |

No tests may silently skip DB setup. Record discovered nonzero inventories, actual
names/assertions/results and retained evidence before review. No existing tests
removed without named replacements/dispositions. Required full repository/service
verification is still final p10; use focused checks during this phase.

## Ownership and discovery so far

- Openbot SessionAuthority in src/adapters/http/session.rs verifies signed claims
  and stored session; user_from_headers is the actor boundary. SessionRecord has
  id/user/auth_revision/issued/expiry; VerifiedSession only id/user. New bridge
  must reuse this authority, never accept submitted user IDs or the browser token
  at attribution. Grant verification binds a live originating user_sessions row
  and users.auth_revision (src/adapters/persistence/session.rs).
- Root normal login/confirmation and Google callback land at SIGNED_IN_LANDING=/;
  user::index then routes to invitations/onboarding/ui. A bounded server continuation
  after existing login needs one validated continuation path, not arbitrary return URLs.
  Explore index as the common continuation seam; no decision accepted yet.
- Root company membership owner is CompanyPersistence/company_access and
  CompanyUseCases.readable_company/managed_company. Owner/admin manages; member reads.
  Existing list_accessible_companies is unpaginated; new identity list must preserve
  membership semantics with bounded keyset pagination, not load-all-and-truncate.
- Contract status POST explicitly suspends/resumes **attribution access** for a
  company; do not invent a global Openbot company suspension feature. Deletion
  remains owned by existing company/account use cases.
- Direct deletion src/adapters/persistence/company.rs::delete_company_with_cleanup
  locks company, retires integration/memory lifecycle, then deletes in one tx.
  Account deletion user_lifecycle::lock_account_on locks user then sorted companies/
  memberships; finish_account_deletion_on independently retires/deletes all owned
  companies. Both must write durable attribution tombstone/outbox before deletion.
  Trail caller/exhaustive searches identify vault/memory account-deletion regressions.
- Root src guides apply to root edits, service AGENTS.md to independent service.
  Root init schema must be edited in place; only newly owned test DBs may be reset.
  Never run blanket reset scripts on this shared PostgreSQL server.

## Resources

- Service cache /tmp/as-attribution-target, contract environments
  /tmp/as-p02-contract-clean (hash-verified Python3.13) and prior
  /tmp/attribution-service-contract-env remain available.
- Plan to allocate distinct as_p03_* root test/migration DBs and root cargo cache
  /tmp/as-openbot-target, preserving root fixture isolation requirements. Service
  existing UUID as_test_/as_verify_ fixtures remain invocation-owned. No p03 DB yet.
- Root maintenance Postgres localhost5432/mac02, PostgreSQL18.7. Do not expose URLs
  with secrets in commands/logs. Global sweeps need owned DB fixtures.
- Homebrew Docker/Colima now installed. p02 owned VM/data removed after proof to
  reclaim disk; create a new named task profile only if required. No active app/DB
  container. Leave unrelated machine processes/caches/data alone.

## Next

Complete source/caller trace for root config/state/supervision and company lifecycle,
record AS-P03-D1/r1 architecture/schema/transaction/authorization/failure decisions,
then obtain B's explicit gate pass before dependent implementation. Routine fixtures
and baseline environment preparation may proceed independently.


## Architecture assignment boundary

Proposed decision AS-P03-D1/r1 in logs/AS-P03-D1-r1.md (immutable revision, SHA256
retained alongside). Covers schema/authorization/lock order/continuation/rotation,
DB-trigger lifecycle ownership and durable supervised retries, verification mapping.
No implementation until B explicitly passes the decision. Coordination revision4
and notices pinned in logs/AS-P03-G1.coordination.md.

Completed trace: AppState literals exist only in http_state + struct definition;
init_app_state composes Foundation then ApplicationWorkers+HttpComponents. The root
main serve function retains worker handles and join_background; use that owner.
Root central security accepts strict Origin/Referer for cookie mutations, but skips
requests carrying Authorization. The new cookie-only status route must explicitly
reject irrelevant Authorization headers or independently enforce its browser boundary.
New machine ingress authenticates its own header class before bounded form/JSON parsing.

Read all454lines of root persistence AGENTS.md. Its compatibility/backfill and shared
reset suggestions are superseded by no-backward-compatibility and invocation-owned
DB isolation; do not reset existing databases or stash user work.

Baseline command (before any root source edits): CARGO_TARGET_DIR=/tmp/as-openbot-target
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 SQLX_OFFLINE=true
cargo check --locked --offline --all-targets. FAIL with61 E0659 ambiguous invalid()
imports in existing workflow code, rustc1.99.1 (root CI selects stable). No root src,
Cargo or migration diff existed. Log AS-P03-root-baseline-check.txt retained.
Traced cause: actions/reconciliation/contracts.rs:307 exports invalid() pub(crate),
which glob-exports through reconciliation/actions and conflicts with preexisting
invalid helpers. Its consumers are within the reconciliation module; narrowing to
pub(super) is the proposed one-line correction. User authorization is pending because
selected skill says leave unrelated failures unchanged. Do not apply without answer.
This is a baseline verification limitation, not evidence against new SSO code.

Dependency lookup found oauth2 5.0.0, chacha20poly1305 0.11.0 and hmac0.13.0 current;
choose compatible maintained versions and lock them during implementation. No new
root/service dependency or source has been added for p03 yet.

## Gate revision r2

AS-P03-G1-N1 found F1 (P2): DB clock rollback extends timestamp-only freshness.
AS-P03-D1/r2 adds mandatory process-local monotonic tickets, cold refresh after
restart/other instance/eviction, durable sequence ordering, locked deadline recheck,
and deterministic rollback/late/lock-wait tests. Pending B recheck; no implementation.
Coordination revision4 and both r1 notices rechecked unchanged. Owned root DB was
created/migrated (see baseline-migration log); earlier no-DB status is stale.

## Implementation start

AS-P03-G1-N2 PASS for AS-P03-D1/r2 (SHA6968a1aa prefix), source f4421ac9.
F1 resolved at architecture level only; runtime assertions remain required.
Coordination revision4 and both notices unchanged. Root begins p03 implementation.
Browser tooling /tmp/as-p03-browser pinned Playwright1.65.1; installation in progress.

## Implementation checkpoint 2026-11-16

Service identity/session/tenant ports, PostgreSQL storage, encrypted secret envelope,
HTTP authority and browser adapters, local monotonic freshness, reconciliation and
startup composition are implemented (not reviewed). Eight focused tests passed on
real isolated UUID databases in AS-P03-service-focused-1.txt; tests cover transaction
consume races, encryption AAD/rotation, session rotation/idle/revoke, terminal lifecycle
races, equal-version conflict, backward-clock rejection/cold ticket, stale sequence,
and reconciliation competing claims/fenced completion/poison backoff. More AC1–AC6
runtime/browser/upstream assertions remain. Service pinned1.95 offline all-target
check passed in service-check-3.txt (two unused imports subsequently removed).

Root init schema now adds bound codes/grants/continuations and lifecycle triggers/
outbox. Fresh reset/migrate on the exact owned DB succeeded (root-migration-1.txt).
Root application ports and PostgreSQL identity/outbox adapters added; HTTP/config/
supervised delivery/root tests are next. Existing helper visibility fix unapproved,
unchanged. No p03 batch freeze or acceptance yet. No native store adapters added.


Focused HTTP tests initially failed because oauth2 CsrfToken::new_random() defaults
16 bytes, below the accepted 32-byte token bound. Corrected both application token
factories to maintained new_random_len(32), with explicit decoded-size regression.
AS-P03-service-http-1.txt retains the failure. AS-P03-service-focused-2.txt proves
11 tests passing (all eight persistence/crypto tests plus HTTP live role downgrade,
membership loss, foreign company IDs, upstream revoke vs timeout503, CSRF, local
logout during revoke outage, separate cookie scope, escaping and credential classes).
AS-P03-service-startup-1.txt proves actual startup/readiness/assets/joined shutdown.

Root bridge HTTP/config/cursor authentication, post-login continuation and worker
composition are now implemented, not accepted. Root-check-1 still reports baseline
61 E0659 errors (no new errors reported by the lib check); root runtime tests remain
unverified. Main binary compilation can expose further errors after the baseline fix.
Playwright1.65.1/Chromium installed at /tmp/as-p03-browser; browser journey not run.


## Authorization update

User replied “continue” after the explicit narrow build-fix question and status that
root checks were blocked. Root stated it would include that proposed fix and proceed.
Applied exactly contracts.rs invalid() visibility pub(crate)->pub(super); direct
callers traced and remain within reconciliation. This supersedes the pending approval
status. Include the change and focused workflow reconciliation checks in p03 review.


Root offline all-target compilation PASS after authorized helper fix, 1m48s,
AS-P03-root-check-2.txt. Existing unused import warning in reconciliation_fixture.rs
left unchanged. Root focused attribution test run now compiling/linking (log
AS-P03-root-focused-1.txt). Service binary build PASS under pinned1.95.
Service lease mutation fence and 413 body error now implemented and seven lifecycle
checks PASS in AS-P03-service-lifecycle-4.txt (includes two new tests). Source hardening
no longer pending for those cases. Browser script/fixture runner and npm lock now
under service browser/ and scripts/browser-sso.py; root real-login test composition
still being added. No browser result claimed.


## Resume and verification closure 2026-11-16

Coordination revision4 and AS-C001/r1 + AS-C002/r1 rechecked unchanged on resume.
Same root/A and independent B handles retained. All prior approved defaults and the
authorized one-line unrelated helper fix remain in effect; no questions pending.

Recovered browser-1 PASS (real password and existing-login journeys, seven decisive
checks) and root-focused-1 PASS17. Browser-1 predates the database-isolation assertion
and a subsequent Python fixture typo; corrected the tuple environment lookup and
will rerun current browser source before relying on it. Root-http-1 retained the
new fixture's ambiguous get_by_id compiler failure; explicitly selected UserPersistence.
root-focused-2 now PASS7 identity/lifecycle DB+HTTP tests.

Additional focused proofs now cover expiry/suspension after an actual competing
tenant lock (pg_blocking_pids verifies the wait), cold instance and cache eviction,
concurrent provisioning, bounded callback/login admission, and expired local sessions.
Service-focused-4 retained two fixture failures (302 expected as303; invalid expired
row violating schema). Corrected the fixtures without weakening intended assertions.
Service-focused-5 PASS33 (19 identity/config cases plus14 affected foundation cases).
New root worker fixture exercises actual HTTP cancellation/join, lost acknowledgment,
lease recovery with identical payload, and stopped worker admission; execution pending.

Configuration now refuses reuse of current/previous client and lifecycle secrets
across credential classes. Fixtures use separate secrets; both configuration owners
have regression checks. This implements the reviewed dedicated-credential boundary.

Extracted-1 exposed the source guard mistaking FOR UPDATE SKIP LOCKED for a table
reference and joining the final UPDATE of one query to the next query literal.
Guard now scans literals separately and recognizes row-lock/upsert UPDATE clauses.
Ten checker regressions pass, including foreign tables under locks and CTE scope
across separate queries. Extracted-2 rerun is in progress. Earlier service-gate-1
Python3.9/tomllib failure is environment-only; use installed Python3.13 as documented.

Current commands: root pipeline33046 runs identity/config/session/company/deletion/
authorized-helper/source-modules/offline checks sequentially; extracted pipeline23764
runs isolated foundation/metadata/boundaries/contracts. All source remains uncommitted.
Remaining: current browser run, root SQLx prepare, finish pipelines, freeze/evidence
manifest, B whole p03 review and corrections. Final root full suite remains p10.


## AS-P03-B1-R1 verification and contract impact

| Criterion | Exercised production mechanism and decisive evidence | Current result |
| --- | --- | --- |
| AC1 | Root `attribution_sso_code_binding_competing_redeemers_and_parent_revocation`, two HTTP SSO tests, service `attribution_sso_login_binding_and_competing_consumers` + callback binding/admission; browser-2 uses actual password form, continuation, both origins and one-use exchange | PASS; one winner, wrong binding rejected, no upstream calls on unbound callback, expiry/replay rejected, seven browser checks |
| AC2 | Service session rotation/expiry/encryption/CSRF/logout/credential-class tests; root current revision/parent revoke; root-session-regressions-1; browser-2 | PASS33 service total, PASS25 existing security_session cases; idle/absolute expiry, AAD/wrong keys, local-first logout, retryable outage, separate cookies and upstream revoke |
| AC3 | Root current membership/access tests plus company regressions; service live-role/foreign-ID tests; browser fixture attempts CONNECT as the service-owned role to the real Openbot fixture DB | PASS3 existing company-access cases; foreign IDs denied, live downgrade/lost membership, one tenant row under competing first-use, database connection refused |
| AC4 | Service lifecycle concurrent provision/delete/version/observation tests, monotonic rollback/late expiry, actual pg_blocking_pids-confirmed lock wait with no subsequent write, cold/expired/evicted instance and reconciliation missed-delete repair | PASS; persisted freshness cannot supply a process ticket and notifications cannot refresh it |
| AC5 | Root owning deletion transaction/rollback + outbox competitors/exhaustion; actual worker HTTP shutdown/lost-ack recovery; service reconciliation ownership/shutdown/poison tests; vault and memory account deletion regressions | PASS8 root identity/lifecycle cases; PASS1 vault + PASS1 memory; identical immutable retry payload, joined cancellation, stale lease rejected, terminal outbox failures retained |
| AC6 | Extracted-2 includes fresh restricted-role DB migration, SQLx metadata check, format/offline all-target check, source boundary, ten boundary checker tests, fourteen foundation tests and twelve contracts; root source_modules-1 and helper regressions | PASS extracted checks; PASS8 module checks; PASS4 authorized-helper regressions; final root compilation/SQLx metadata pending below |

Browser-2 executed the fresh compiled root lib-test binary directly while Cargo
compiled the separate non-test library for module checks, with the same root source
and standalone service-binary-2. Binary path is retained in the evidence manifest;
this avoids build-cache contention without replacing the actual test. No tests
removed or renamed. New tests are discovered by the Rust module roots.

| Changed contract | Producers/consumers/wrappers and disposition | Proof |
| --- | --- | --- |
| Openbot code/grant authority and current membership | New bridge consumes SessionAuthority and account auth_revision; API consumer is independent HttpAuthority. Root normal login only adds common index continuation; original password/security handlers preserved | Browser +25 security regressions + both HTTP sides; dedicated credential-class tests |
| Account/company deletion | Init-schema triggers own lifecycle+outbox transaction; CompanyPersistence::delete and HostAccountDeletion retain integration/memory cleanup and terminal tombstones | Owning deletion failure/rollback, vault/memory regressions, outbox concurrency |
| Service tenant freshness/mutation guard | IdentityService company/tenant/refresh and lifecycle ingress use same Postgres row; future merchant/resource owners must use lock_tenant before mutation | Actual waiting-writer tests, durable sequence/version tests, cold-process/eviction/outage |
| Worker startup/shutdown | Openbot Foundation→ApplicationWorkers→main workflow handle joins; service startup owns reconciliation; delivery and reconciliation use execution-fenced durable state | Actual HTTP cancellation and recoverable lease plus production service startup/shutdown |
| Optional Openbot / required attribution identity config | Both config owners reject malformed/incomplete origins, credentials and rotation; separate test/image/browser fixtures supply keys. No upstream readiness claim | Config tests, current browser, extracted checks; p10 will rerun current container |
| Independent-source guard | CI/verify/extracted-build consume boundary.py; lexical locking fix preserves unknown-table rejection and query-local CTE scope | Ten checker tests + extracted source boundary |
| Reconciliation helper visibility | Authorized narrow pub(crate)→pub(super), consumers within reconciliation; no business logic change | PASS4 focused reconciliation-attribution tests + all-target compile |

Limits: HTTP authority fixtures prove the consumer with contract-faithful responses,
not a whole Openbot deployment. Browser uses real login/session/persistence/bridge
handlers with real PostgreSQL and actual independent attribution process; unrelated
Openbot mail/agent runtimes are intentionally absent. Full two-process deployment,
commerce lifecycle/purge and full-suite gates belong to later phases. This phase
provides the locked tenant boundary; public merchant ingestion does not exist yet.
No p03 container result is claimed from p02's accepted image evidence.


## AS-P03-B1-R1 freeze

Root all-target offline check PASS; root SQLx prepare PASS against owned migrated DB;
metadata regeneration produces no .sqlx diff (new queries are runtime SQL, exercised
by DB tests). Root format and diff whitespace checks PASS. Trail graph rebuilt;
trail check PASS with three unrelated advisory keyword matches, no conflicts.

Frozen artifact `45fc0ad49012ab1f6f58a619c06508430011b0c3a127683196c79ea906c71897`, phase start/review-base/current HEAD all
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`. Exact exclusions unchanged at top.
Pinned coordination revision4 and both r1 notices: logs/AS-P03-B1-R1.coordination.md.
Evidence manifest: logs/AS-P03-B1-R1.evidence.json (retains historical failures and
current replacement successes; none of those superseded failures is a current gap).
Mac snapshot needs PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:$PATH; initial
BSD realpath -m failure corrected using the installed GNU tool, no script change.
Whole source worktree frozen for independent B review; only phase records may change.
All author check commands finished; no task-owned servers remain running.
No implementation acceptance yet. Next: B whole batch AS-P03-B1-R1, then corrections.


## AS-P03-B1-R1 review disposition / correction start

AS-P03-B1-N1 requests changes for five P2 findings F1–F5 (all in reviewer findings).
Frozen R1 identity retained; author unfreezes source for one cohesive correction round.
F1: use a targeted retained-session lookup for logout-only CSRF validation, including
already revoked records; keep authentication's live-only lookup. Return contract302
and clear cookie on same-target retries. F2: HTML shell recovers Unauthorized to
sign-in/clearing cookie; API401 and authority503 remain distinct. F3: bound authority
responses to128KiB and test schema-derived maximum serialized pages with an80%
early-failure guard plus over-limit rejection. F4: add bounded operator log probes
backed by persistent outbox/status evidence; no public/global tenant endpoint or
new credential. F5: add actual producer membership/pagination/foreign-ID/status-write
regressions with the same live grant across mutations. These finish reviewed
contracts; no ownership/schema/recovery/authorization model change is proposed.

Earlier AC3/AC5 summaries are superseded by these explicit producer/operational gaps
until correction checks and B recheck pass. Prior passing tests remain historical
evidence for their actual exercised paths, not acceptance of missing behavior.


## AS-P03-B1-R2 correction evidence (in progress)

All five findings have authored corrections. AS-P03-B1-R2-service-1 PASS21 identity
tests includes F1 retry with valid/invalid CSRF, contract302/cookie removal; F2 HTML
expiry/revocation versus API401/outage503; F3 actual HttpAuthority maximum Unicode
and escaped-control-character pages with schema-derived limits,80% guard and valid
over-limit JSON rejection; F4 captured operator warning/healthy events after fresh
service recreation and actual reconciliation repair. Service metadata regeneration,
extracted foundation gate and current standalone binary PASS (named logs).

AS-P03-B1-R2-root-1 PASS11 includes F5 real bridge grant + production membership
use cases, owner/admin/member status permissions, valid foreign company, downgrade,
removal and multi-company pagination. F4 root actual worker probe reads persisted
retry/exhaustion state through fresh persistence/bridge instances and emits captured
warning/healthy events. These now prove producer SQL independently of consumer
authority stubs. Root worker health probes and service probes run every30s at startup,
with2s deadlines and cancellation; global state appears only in operator logs.

AS-P03-B1-R2-browser PASS1/8 checks against fresh service binary and current root
test binary, adding a usable sign-in page and absent cookie after upstream revocation.
No existing tests removed/renamed; new cases extend coverage. Existing R1 security/
company/deletion/helper/configuration results are unchanged-scope evidence.

Root pipeline80178 runs all-target offline compilation, source_modules, SQLx prepare,
and formatting. Browser61891 is complete PASS. No new migration changes; all new SQL
is runtime SQL exercised against real isolated PostgreSQL; regenerate metadata anyway.
No p03 acceptance; B recheck only after these gates and a new frozen evidence manifest.


## AS-P03-B1-R2 freeze / correction recheck assignment

All correction checks PASS: service identity21 + configuration1; root identity11;
browser1/eight checks; independent extraction foundation14, boundary10, contracts12;
root modules8; both apps formatting/offline all-target compilation and fresh owned
service migrations/SQLx preparation, root SQLx preparation on owned migrated DB.
No .sqlx diff; runtime SQL proofs remain the decisive checks for new queries.
No test names removed. Preserved unchanged R1 configuration/security/company/deletion/
helper evidence is enumerated in the R2 manifest; changed logout behavior uses new
evidence only. Initial R2 formatting parse failure retained with its correction.

F1–F5 corrections are mapped above and are ready for B's independent recheck, not
yet accepted. Operator-log access is the monitoring scope, with no global tenant
HTTP endpoint; README names all fields, deadlines and external alert-routing limit.
A scoped learning l-20f04c5f records the response-size cause/guard; included as a
deliverable, uncommitted. Trail build/check PASS;7 advisory keyword matches refer
to unrelated SDK locking/mail-ID/Git-push/personal test-role guidance, no conflicts.

Frozen digest `fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c`; start/review-base/current HEAD remain
`2df562d6c0880dbfc7498e4fe8ecb19ef3fe34ee`; exact four exclusions unchanged.
Coordination revision4 and AS-C001/r1/AS-C002/r1 compared unchanged and copied to
logs/AS-P03-B1-R2.coordination.md. Manifest logs/AS-P03-B1-R2.evidence.json.
Whole implementation worktree frozen. No author commands remain active.
Next: B recheck F1–F5 and affected integration against this artifact.


## Root acceptance — AS-P03-ACCEPT-R1

Accepted in phase at fd40f4807955c2e19c00ab9d864c2a0ce84566088e99743b84f9338a7ce7d30c.
B AS-P03-B1-N2 explicitly PASS; F1–F5 resolutions saved. Root recomputed the exact
matching digest, checked coordination revision4/notices unchanged, and validated
AS-P03-ACCEPT-R1.evidence.json (17 PASS, no current FAIL or NOT EXERCISED). Historical
failed checks remain in original manifests/logs with correction evidence.

Phase03 accepted scope: SSO/local session recovery, live company authorization,
monotonic lifecycle/freshness, durable supervised delivery/reconciliation and operator
signals, independent application/DB boundary and focused real-browser journey.
This is not whole-plan/container/deployment acceptance. p04–p10 remain authorized
and required. Source changes uncommitted; no deployment or database reset authorized
except invocation-owned fixtures. B idle, reusable for next phase.
