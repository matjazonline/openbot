# Plan04 p03 — final documentation, integration and publication

Status: accepted in phase under C005/r1; normal main publication pending.
Workflow implement-astra-reviewed-2-3; phasep03, record root this docs directory.
Root/A /root user-confirmedSol6.1/xhigh; retained B /root/p04_reviewer requested
Astra/high, actual settings unexposed, completed p02 review and idle. Root owns
TASK/HANDOFF/coordination; B owns REVIEW/notes/findings. Context controls unexposed;
use durable checkpoints. Worktree /Users/mac02/orca/workspaces/openbot/plan03-04,
branch feature/plan03_04. Start/review-base/current HEAD4dad1e971e1791f75731da50a7e73e6776177057. Clean at startup;
no preserved dirty changes. Exact snapshot exclusions for this phase: this p03/docs
and exact Plan04 COORDINATION.md. Do not exclude implementation/deliverable docs.

## Authority, dependencies and original requirements

Authoritative /Users/mac02/orca/workspaces/openbot/plan03-04/plan/04-root-unit/COORDINATION.md,
root writer, revision12 checked at startup. ACK C001r1 canonical root/attribution
teardown contract; C002r2 explicit user "stop after 04 push" -> finish04/required
checks/pushmain/STOP, NO05/06; C003r2 explicit require receiving Participate grants
for actual authenticated agents on EVERY Members root/child desk; C004r1 real
UUID membership grants survive unverified email. Inherited01DEP-C002r1: own members/
all own roles,parent Lead,direct child Leads, one step BOTH WAYS.

Original plan/04-root-unit/03-integration-and-final-verification.md plus README,
01-root-department-and-membership and02-access-modes. Accepted dependencies:
p01d4bbde66/source2a5714f5…7ab793/review04RU-P01-B1-R2/N1; main integration334e79c7/
source09f2972f…d22c4a8/review04RU-MAIN-B1-R1/N1; p024dad1e971e1791f75731da50a7e73e6776177057/
source6391c0ba46d382417a618bdc9238b32c1b8e1b976a457ee42f60b2382971e660/
review04RU-P02-B1-R2/N1. P02 whole-B1 R1 F1 fixed and explicitly closed by R2.

Plan03 and plan/fix-doop.md already published on origin/main03f0bda2. Fresh fetch
started before this phase; confirm result before integration. Plan03C008/r1
explicitly defers existing Doop execution; keep its failures/declarations visible,
do not claim whole CI green or label a new affected failure deferred blindly.

## Cohesive B1 / contract and verification mapping

No new schema/ownership/authorization/concurrency decision is proposed: reuse
accepted p01/p02 contracts. Documentation and final integration form one B1;
no architecture gate unless a necessary correction changes a costly contract.
Read root/src/subsystem guides and selected skill verification/coordination
references already retained from this session; use Trail before source discovery.

| Criterion | Production scope / assertion | Verification/evidence plan | Status |
| --- | --- | --- | --- |
| AC1 integrated root/membership/roles | canonical root row and membership, mirrored owner/admin, protected root, tenant fences and actual competing mutations | accepted prerequisite proofs + final current DB suite and HTTP authorization cases | Pending final gate |
| AC2 integrated required desk/grants | default/explicit owner, Members/Public, both-way audience, explicit authenticated receiving grant, provenance/email/incarnation/classification and revision fences | p02 accepted206/14/170/HTTP21/browser/scale evidence + current full suite, source reachability and integration | Pending final gate |
| AC3 docs match current behavior | departments/transport/authentication/archived memory mapping; current Company/Agent/User namespace vocabulary, levels from root; no promised future scopes/delegated administration | direct source/accepted-contract inspection, doc/plan/startup-path checks and final independent review | In progress |
| AC4 required repository gate | formatting/whitespace/offline all-target compilation/clippy/fresh migrations/SQLx/reachability/full DB/network/stock-stack/lease | scripts/check.sh once after docs; retain every result, compare ci.yml and run omissions including Linux-only control-plane scope through owned Docker when available | Not yet exercised |
| AC5 publication and stop | complete accepted04 source on normal main, accepted Doop follow-up explicit | final snapshot/manifest/review, normal commit/fetch/merge if needed/push, remote SHA confirmation; stop without05/06 | Pending |

Source impact: p02 already supplies accurate docs/departments.md and
transport_architecture.md; reuse where they satisfy03. Authentication docs need
canonical-root membership context. Archived memory integration still uses actual
NamespaceOwner::Company/Agent(Uuid)/User(Uuid); add dated organization mapping
without claiming proposed unit/principal namespace implementation. Align active
plan wording with already-accepted both-direction and receiving-grant notices.
Only scoped evidence-byte .gitattributes metadata is additionally needed.

Run the full verification script ONCE initially; do not run full suites per small
fix. Diagnose actual failures and correct affected regressions; rerun affected
checks and repeat broader gates only when impact warrants it. Preserve failures,
exact commands/results, fixture identity and unavailable prerequisites. Full
stock-stack and lease checks are distinct from p02 focused2MiB tests.

## Resources and next

Private env /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa04-p03-env.sh will
source p02 and task Docker env; owned PG18localhost51167, Node24/GNU/Python3.13,
locale en_US.UTF-8 and task Cargo target. Preserve p01/m01/p02 databases. The
check runner creates/drops a fresh owned database; its source DATABASE_URL is
maintenance access, not permission to reset borrowed schemas. Docker profile
openbot-plan04, default context unchanged. No owned Cargo/browser/DB-check process
active at phase startup. No other workspace resources may be stopped/reset.

Next finish precise docs/current contract mapping, start retained full gate, run
CI omissions, resolve affected failures and obtain one final integration review.
Then publish04to main and STOP as explicitly requested.

## Resume / first full-run diagnoses

The original full run remains exec9115, owned `openbot_check_*` DB, log
`logs/B1-full-check.txt`. It is still in its first database-backed suite; do not
start another complete check runner. The separate Linux control-plane gate
completed successfully: `logs/B1-linux-scope-test.txt` proves real competing
entrypoints, descendant isolation, supervisor survival, signal/error/timeout
cleanup and preservation of unrelated firewall/group objects. Pinned build/pull
inputs and outputs are retained alongside it.

Early failures were reproduced against a separate phase-owned diagnostic DB
`pa04_p03_diag_77d937a839e9` on PG18:51167 using the existing compiled libtest
binary. Logs `B1-early-failure-diagnostics.txt` and
`B1-more-failure-diagnostics.txt` retain the failures. They are not waived:

- Inbound fixtures assumed settings revision0. Default root Members grants now
  advance the actual revision. Fixture intents must capture the current desk
  revision, including every multi-desk association; production's supersession
  guard remains unchanged.
- Five workflow/publication fixtures deleted a company owner's principal.
  Canonical root membership now correctly refuses this via the owner FK. Reuse
  the established test revocation helper, which transfers ownership first, then
  demotes the original principal and retires its membership in the same transaction.
  Preserve competing-claimant checks and exact refusal/no-write assertions.
- An email-adoption test counted every grant as an allowlist grant. Check the
  exact two configured grants (View/Participate) and their new principal; preserve
  inherited root grants and the existing orphan/author/thread assertions.
- Rejected onboarding must leave exactly the preexisting root department.
- Agent-principal FK checks are now deferred for tenant teardown. The race must
  exercise that check before releasing its writer and assert a late writer is
  refused at COMMIT. Keep the real competing deletion and no-dangling-row proof.
- Member-removal barriers must recognize the current `SELECT user_id` company
  lock statement. The reviewer account discovery race may return its documented
  retry Conflict; assert unchanged durable counts, retry, and require the
  original terminal no-park outcome. No timeout/budget is raised.

These corrections change tests/helpers only, no production protocol/schema or
authorization decision. They extend accepted contracts and need no architecture
gate. Focused check plan: all inbound tests (including settings races/multi-desk),
account identity, onboarding, agent retirement/publication, workflow action/control/
harness/reconciliation/decision authority families; HTTP authorization21 and
SQLx regeneration after fixture SQL edits. The original full run's later stock
stack gate will compile and exercise current source; retain every failure and
any affected recheck separately.

The Doop runner-boundary failure under local Python3.13 is TLS strict-validation
of its existing generated CA (missing Authority Key Identifier), not a Plan04
policy/runtime change. Run the unchanged gate with CI's Ubuntu24/Python3.12 in the
owned Linux image, retaining both results. This does not exercise or waive the
separately deferred live Doop integration in C008.

### Correction and final-gate checkpoint

- First full libtest result: 4689PASS/31FAIL/29ignored in1950.64s. Full output retains
  all31 traces. Thirty failures belong to the fixture diagnoses above; the last
  provisioning receipt test used now-valid root role `owner` as an invalid role.
  It now accepts owner/admin/lead explicitly and rejects `unknown_role`.
- Focused correction run:139PASS/2FAIL (141selected), both failures were residual
  fixed absolute settings-revision assertions. The two competing settings tests
  now assert current revision unchanged while blocked and exactly+1 after edit.
  Recheck `B1-correction-revisions-http.txt`:27PASS, including both races and all
  exact21HTTP authorization cases (plus4other authorization tests).
- `B1-correction-provisioning.txt`:1PASS. `B1-final-other-targets.txt`: binary5PASS
  plus source reachability8PASS. `B1-correction-sqlx-prepare.txt`: all-target SQLx
  regeneration PASS; metadata unchanged. No tests renamed, moved or removed.
- Full initial Clippy failed8Plan04 shorthand/autoderef lints; correction added one
  clone-on-Copy lint. All9 mechanically corrected. Two production call sites only
  drop redundant explicit autoderef (member/account retirement); types, ownership,
  SQL, locks and failure semantics unchanged. Focused compile/Clippy recheck and
  current full stock-stack proof remain required. No budgets or lints suppressed.
- Grouped browser gate stalled in unchanged knowledge upload test at page.evaluate
  during a pending native navigation (line78), with no timeout. Stopped only the
  verified owned stuck child after6m22s; original output17PASS/1FAIL retained. An
  external instrumented probe using the identical generated fixture passed. All
  unchanged18protocols then passed with file concurrency1 and explicit90s/test
  deadline in `B1-browser-bounded-recheck.txt` (16.4s); internal competing-claimant
  assertions unchanged. No application/browser test source changed for this.
- Unchanged Doop runner boundaries: all46PASS/0skipped with Ubuntu24/Python3.12
  plus git/OpenSSL/Node in `B1-doop-boundaries-ci-python-exec.txt`. Prior container
  attempts document absent tools and Docker's default noexec /tmp; successful run
  explicitly mounts an executable disposable /tmp. Python3.13 strict-CA failure
  retained. This is independent of deferred live Doop execution.
- First full runner now executes stock2048KiB suite on corrected source. It will
  continue lease/image/service gates. Do not launch a second full runner. Remote
  fetch completed: origin/main remains03f0bda2, no new commits to integrate.

Additional owned fixture: Docker container `openbot-plan04-p03-memory-pg`, label
04RU-P03, `--rm`, pinned CI pgvector0.8.7/PG18 digestc29f3068…42e47, bound only
127.0.0.1:32768, no shared volume. Private pa04-p03-memory-env.sh names its memory
admin URLs. Core PG18:51167 lacks vector; original runner memory URLs default to
it. Use the owned vector fixture for affected memory gate rechecks if that missing
prerequisite fails; preserve original results. Container/pull identity retained in
B1-memory-postgres-{pull,container}.txt. Stop only this container at completion.

### Owned CI-fixture correction and completed memory gates

The first extracted service run failed only its20k-nearer-ineligible-parts search
case: PostgreSQL error53100 while resizing a DSM segment to63999520bytes, exhausting
Docker's default64MiB /dev/shm with overhead. This is an inadequate new test fixture,
not a Plan04 or memory-service code failure (services/memory has no diff from main).
Recreated only the verified04RU-P03-labelled owned container with1GiB /dev/shm at
the same loopback port, migrated its fresh core DB, and reran the extracted gate.
No application/CI bound or assertion changed; the same large index stress test
remains exercised as the failure signal. Old container/core test data were disposable.

- `B1-memory-extracted2.txt`: ALL extracted service gates PASS,175tests PASS,
  2ignored (20.02s). Boundary probe, locked/offline check, Clippy, owner-role fresh
  migrations, SQLx, pinned model identity and network-isolated suite all exercised.
- `B1-memory-backup.txt`: actual pg_dump/pg_restore content/no-replay test1PASS.
- `B1-memory-standalone.txt`:13-step standalone journey PASS with stand-in Messages
  provider and actual service/model/database; extracted source/build exercised.
- `B1-memory-integration.txt`:6real-service OpenBot live/e2e cases PASS, including
  competing delivery workers, provisioning/capture/disable, revoked credential
  replacement, content-level ceilings and tampered-label refusal. Both-direction
  database role isolation asserted. Core and service DBs correctly share only the
  owned PGvector server, not credentials or roles.
- `B1-memory-image-buildkit.txt` and `B1-memory-image-smoke.txt`: production image
  PASS, non-root/offline/read-only pinned model/RSS budget/database refusal.

Initial production image attempt failed its prerequisite: Docker29's CLI did not
discover the already installed Buildx0.39 plugin and used the legacy builder,
which cannot execute Dockerfile RUN--mount. Use private task Docker config
pa04-p03-docker-config (cliPluginsExtraDirs points to installed Homebrew plugins),
DOCKER_BUILDKIT=1 and existing own Docker host, all in pa04-p03-image-env.sh.
No global Docker config/context or Dockerfile changed. Buildx version evidence in
B1-buildkit-prerequisite.txt; preserve original prerequisite failure. Root full
runner inherited legacy config, so expected image prerequisite failures need the
successful current BuildKit image/smoke rechecks, not a second full runner.

Current core stock full suite is still executing. Knowledge production BuildKit
build/smoke is exec92845. Exact initial31failure dispositions are retained in
B1-core-initial-failure-dispositions.json (all31 have exact passing focused names;
this does not yet claim the current full suite passed). Graph refresh completed
successfully in B1-trail-build.txt. No05/06 work.

## B1 final execution and review candidate (coordination13)

All checks have now executed. Original `scripts/check.sh` exited1; its complete
44-gate summary/failures remain in B1-full-check.txt. Never relabel that invocation
PASS. Current replacement proofs for its fixture/prerequisite failures are retained:

| Gate/result | Current evidence/disposition |
| --- | --- |
| Core normal full:4689PASS/31FAIL/29ignored | All31 have exact passing focused corrections; B1-core-initial-failure-dispositions.json |
| Current full stock2MiB:4719PASS/1FAIL/29ignored,2068.72s, no overflow | One Doop mock notify timeout remains an explicit review gap below; all31 corrected cases passed in this full run |
| Lease | 139PASS; unchanged50% work threshold, lease.claim max49ms, lease.validate max1ms; full log |
| Plugin reproducible package and locked runtime install | PASS in canonical fresh external directory, B1-plugin-{package-recheck,runtime-install-recheck}.txt. Same package/lock source unchanged; original npm missing-plugin entry failure retained. Do not assert its path-dependent cause proved |
| Doop boundary protocols |46PASS/0skip on CI Python3.12; original Python3.13 strict CA failure retained; no CA/security/test edit |
| Native browser protocols |18PASS with bounded file concurrency1; original pending-navigation hang retained; no browser/test source edit |
| Memory complete service gates |175PASS/2ignored plus owner migrations/SQLx/model/network/boundary, backup1PASS, standalone13stepsPASS, live integration6PASS; properly sized isolated CI-PGvector fixture |
| Attribution complete extracted service/contracts | PASS, including120native tests, locked Python contract dependencies and actual dashboard/browser contracts; B1-attribution-native-contracts-recheck.txt |
| Attribution browser integration | PASS: service2browser cases and real OpenBot SSO1case/all8assertions; B1-attribution-browser-recheck.txt. Initial root Playwright1.55 fallback failed cookie filtering; installed owning browser package and matching Chromium (actual CI dependencies), no cookie assertion or source edit |
| Production images | All3BuildKit builds/smokes PASS: B1-{memory,knowledge,attribution}-image-{buildkit,smoke}.txt |
| Static/schema/module proof | Final fmt/offline-all-targets/Clippy2/SQLx/current migrations/binary5/source_modules8 PASS; exact evidence paths above |
| Linux control-plane network scope | All8real supervisor/descendant/mutation/signal/cleanup categories PASS; B1-linux-scope-test.txt |
| Full private Doop | Original runner stops at missing explicit system Chrome prerequisite; private assertions NOT EXERCISED. Existing C008/r1 deferral to fix-doop applies; never PASS/whole-CI-green |

### New stock Doop mock timeout: explicit reassessment, not a clean-suite claim

`doop_agent_observe_policy_keeps_error_observation_available_to_the_model` passed
the integrated normal full run, then timed out in the integrated stock run with
all MCP canvas calls completed and execution at notify. The unchanged isolated
network-denied stock recheck PASSED in48.30s; B1-stock-doop-observe-recheck.txt.
No Doop production/fixture/assertion/deadline was edited. The driver still polls
the complete authenticated RunView every20ms with its original5+3*steps deadline.

Historical CH-P01-B3-F7 had an identical notify symptom, but was FIXED by a native
eligibility planning correction; that history is NOT evidence this current failure
is unrelated. Current lease claim/validate metrics are healthy, but they do not
measure all RunView provenance work. Existing fix-doop batch1 separately identifies
expensive full-view test polling as a candidate for private lifecycle progress.
Neither a focused pass nor that candidate alone establishes the cause here.

Final B must assess this new full-context result against original Plan04 criteria,
actual affected paths and C008/r1's instruction to reassess new affected failures.
Root has NOT accepted the phase, classified this unrelated, or silently extended
the private Doop deferral. Preserve it as a verification gap until independent
scope/evidence review establishes the proper disposition or a correction/proof
resolves it. No timeout increase, retry-to-hide-failure or assertion removal.

Source candidate includes deliverable docs, fixture/provisioning corrections,
mechanical Clippy cleanups, evidence byte attributes and one new Trail learning.
All source changes are owned; no preexisting dirty work or05/06 edits. Freeze
exclusions remain exact p03/docs and Plan04COORDINATION. Root/A and retained B
roles unchanged. Latest authorization: finish04/pushmain/STOP. Publication and
final acceptance remain pending; current origin/main03f0bda2 at last confirmed fetch.


## Documentation batch / full gate entry

Current source NamespaceOwner settings.rs302–306 is Company/Agent(Uuid)/User(Uuid),
so dated archived mapping names those exact variants and root-derived clearance.
Authentication operations now explain canonical root membership, current principal/
email/desk authority and owner-only root administration. Existing accepted
Departments/transport guides already describe current root/both-way/receiving-grant
behavior and require no duplicate rewrite. Active README/02 prose now matches
those accepted clarifications. No runtime/schema/query change or new config key.
No startup plan path is new; existing no-backward-compatibility notes retained.

Run scripts/check.sh with private p03 env (p02 PG/Node/GNU plus owned Docker profile).
The runner owns its fresh disposable DB and keeps going after failures. Retain all
output in logs/B1-full-check.txt; this is the first full Plan04 gate. Linux-only
control-plane test is omitted on macOS by the runner; compare CI and exercise it
separately in the owned Linux Docker environment. No existing test thresholds moved.

## Frozen whole-B1 candidate R1

Base/review-base/HEAD4dad1e971e1791f75731da50a7e73e6776177057; artifact9b394b31e98b0ede9d281ea7e977a5966ce683692500dc0ba2642369b43e2f0e.
Exact exclusions: plan/04-root-unit/phases/p03/docs and
plan/04-root-unit/COORDINATION.md. Source staged; no further source edits during
review. Immutable authority pins B1-R1-coordination.md (rev13) and plan01 copy.
Evidence manifest B1-R1.evidence.json retains original failures and successful
rechecks; validator runs before assignment. New Doop mock gap above needs B
disposition; root has not accepted/published. All other required gates have
current successful evidence. Remote fetch reconfirmed03f0bda2/no new main.

Trail check exit1 is retained, not reported PASS: migration14087 contains a
comment about separate native-resource/workflow tenant cascade owners. The
triggering 2026-11-16 browser localStorage learning forbids per-click-key cleanup
that loses another tab's writes. This SQL FK/comment changes no browser key or
cleanup protocol; SDK source is unchanged. Treat as a scope false positive and
state this exact reason in the eventual commit message, per Trail instructions.
The tool also reports its400KB cap and nonblocking suggestions; no full semantic
coverage is claimed for that automatic check.

## Latest user scope update: C005/r1, coordination14

User explicitly instructed "we'll check doop separately" after the named mock
full-stock timeout was disclosed. ACK C005/r1: defer Doop investigation/checks,
including this mock gap, to separate follow-up; retain failed evidence, no PASS/
unrelatedness/clean-full-CI claim. All non-Doop Plan04 gates remain required and
verified. Current source stays frozen at9b394b31e98b0ede9d281ea7e977a5966ce683692500dc0ba2642369b43e2f0e.
Immutable scope addendum/pin/manifest are logs/B1-R1-scope-addendum*; original R1
manifest/pins and reviewer history remain unchanged. B must acknowledge14/C005r1
and disposition any Doop finding as user-deferred, not fixed. Then normal root
acceptance/main push/remote confirmation and STOP; no05/06 or further Doop work.

## Root acceptance / publication pending

Independent04RU-P03-B1-R1-S1/N1 PASS for unchanged source9b394b31…e2f0e under
coordination14/C005r1, with no remaining in-scope finding. Original R1/F1 remains
user-deferred (not fixed/unrelated); current acceptance does not claim whole CI
or Doop PASS. Root recomputed exact source digest after B and validated both
manifests; final patch whitespace PASS. Applicable active notices unchanged;
coordination15 records acceptance only, introducing no requirement change.

AC1/AC2/AC3 accepted with source/integration proofs; AC4 accepted for verified
non-Doop scope under explicit C005r1; AC5 pending normal main commit/push/remote
confirmation. All checks/processes finished, reviewer has no resources. Next clean
only owned disposable fixtures, stage final evidence, validate tracked manifests,
commit accepted source/records with Trail scope-conflict explanation, normally
push main, retain publication identity and STOP. No05/06 or further Doop work.
