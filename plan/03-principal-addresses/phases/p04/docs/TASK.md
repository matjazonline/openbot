# 03PA-P04 — Combined main integration, documentation and final verification

## Resumed here — 2026-11-15

User lifted stop and reselected implement-astra-reviewed-2-3; starting HEAD c4671e72, clean tree.
Root/A /root is Sol6.1/xhigh (user confirmation retained; model also exposed). Context threshold
controls unavailable: keep durable records and host defaults. Sole coordination revision26 read;
I001–I004 (Plan05 portion superseded), C001–C007/r1 and revision10 acknowledged. Terminal scope
is Plan03 complete/review/verify/normal main push, then Plan04 complete/review/verify/normal main
push, then stop before Plan05. No force push/deployment. Reuse accepted D1/r2 architecture and
all unchanged historical evidence; it is not runtime acceptance. No new reviewer needed until
whole-batch review; check existing handles then, otherwise start independent Astra/high B.

D1/r2 §§8–11 implementation mapping: application NativeBrokerIdentity/source and compiler tests;
registry exact current company/account/person capture, incarnation-keyed reservation/lookup/insert;
init-schema immutable broker historical principal insertion validation plus exact run_live check;
provenance FrozenOrigin reconstructs from stored principal. Public private-broker filters and
contract-only recovery reads remain unchanged because they do not reconstruct reviewer identity.
Focused assertions: exact literal principal distinct from run_as account, foreign/wrong-account
INSERT rejected, broker immutable and idempotent, new person incarnation selects a distinct broker
without renewing old effect authority, original-principal deletion preserves historical unknown
recovery, real direct-native company teardown commits. Existing native_/workflow-role/address/API
suites cover coupled interfaces. Fresh owned schema reset, SQLx regeneration, strict clippy then
final full repository/CI gates remain outstanding. PostgreSQL51167 restarted from owned retained
data; no other cluster touched. Pin C locale for snapshots; UTF8 for runtime tests.


## Current resume location — transfer completed 2026-11-15

This entry supersedes older worktree, branch, merge-state and next-action statements below.
- Worktree: `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`
- Branch: `room/check-last-handoff-md-under-plan-02-work-qc8d`
- Sole authoritative coordination: `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot/plan/03-principal-addresses/COORDINATION.md`, revision25.
- Reused checkpoint: `a8d220041b0a90e001815cf8e29fab87f6ab149a`; includes the completed
  merge of main `d543787f` and all additional integration fixes/tests/evidence. Current HEAD
  after this transfer commit is the resume baseline; original review bases remain historical.
- The old integration worktree is no longer required. Its remaining local changes are backed up at
  `/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-retirement-backup-20261115-161325`: binary tracked-settings patch, all4081 untracked/ignored files in a verified archive,
  and SHA256 manifest. This backup is outside the removable worktree. It includes generated caches
  for preservation, not as accepted build evidence. Keep the branch and backup until no longer needed.
- Owned PostgreSQL data, environment script and Cargo cache remain at the external paths recorded
  below and survive removal. On resume, use this checkout as cwd, source the existing environment
  script, restart only the owned database when required, and reinstall npm dependencies here as
  needed. Do not use the older `cd` instruction targeting the removable worktree.
- Transfer checks compare all material tracked files and retained p04 review evidence with the
  checkpoint, except root-owned TASK/HANDOFF updates; no production source changed in this transfer.
- Status: **unfinished implementation, safe transfer checkpoint**. No build/test rerun was needed
  for this source-preserving transfer, and no new runtime or independent-review pass is claimed.
  Existing native broker failures remain. Next implementation is accepted D1/r2 §§8–11, followed
  by focused proof, SQLx regeneration, final repository/CI gates and independent integration review.
- Current `origin/main` was last fetched as `2df562d6`; its subsequent tooling-only commit is not
  merged by this transfer. Recheck current main before final integration/publication.
- Nothing pushed. This transfer does not complete Plan03 or begin Plan04/Plan05.

> No backward compatibility. Reset owned disposable databases; no backfills, legacy paths or staged rollout.

## Authority, roles and accepted inputs

Workflow implement-astra-reviewed-2-3, installed skill at
/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot/.agents/skills/implement-astra-reviewed-2-3.
Root/A /root is user-confirmed Sol6.1/xhigh. Independent Astra/high B /root/p02_reviewer completed
p03 without authorship and will be reused under these p04-only records when a review is ready.
Actual worker/context controls unexposed. Root owns TASK/HANDOFF/coordination; B owns REVIEW/notes/
findings/logs. No other active phase worker. Existing authorizations cover Plan03 full-gate/main
push, then Plan04/root-unit/review/full-gate/main push, then Plan05/scopes. No force push/deployment.

Authoritative coordination is this worktree plan/03-principal-addresses/COORDINATION.md revision22,
root sole writer, adopted from original worktree revision21 handoff. I001–I004/C001–C007/r1 and
revision10 acknowledged unchanged; recheck at batch/shared-contract/review/acceptance boundaries.

Worktree /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-integration-20261115; branch codex/principal-addresses-integration-20261115.
Starting/review-base/HEAD3fceb7d3435f3b821c3b37820200e1c5400206d9, reviewed Plan03 source commit.
Plan02 accepted cb46c6fd and p01/p02/p03 accepted artifacts remain ancestry/prerequisites. p03 exact
accepted digest3247c1f394f32bb86dea1a31f3f1856c0f805ed0c68329149217f9c72fa9230d; export
phases/p03/docs/logs/p03-accepted-source.json/.tar.gz (SHA2566891f662b5c05529e4e569b2a35d4be3870b82497640870ae99f523e5a08cacb,
413 material files, omissions retain830561e2 HEAD bytes). Earlier accepted identities/evidence in
those phase records are retained, not re-reviewed unchanged. Original checkout user changes under
.claude/.mcp/AGENTS/.trail and Plan02 handoff are preserved outside this clean integration checkout.

## Original requirements and batch

Original plan README and04-integration-and-final-verification.md, accepted p03 D1/r3/D2/r1 and
active coordination contracts. One cohesive integration batch: merge newer main preserving both
accepted implementations, update all six named deliverable docs, fix integrated regressions and
required-gate failures without weakening assertions/limits/guards, then one final independent
integration review. Architecture gate reused for unchanged accepted contracts; reopen only if a
new costly ownership/auth/concurrency decision becomes necessary. Do not start Plan04/root-unit
until Plan03 complete/accepted/pushed normally to main.

Snapshot exclusions for p04: this p04/docs record root, p03/docs, p02/docs, p01/docs,
plan/03-principal-addresses/COORDINATION.md, plan/02-workflow-roles/COORDINATION.md,
plan/02-workflow-roles/phases/p05/docs. No implementation or deliverable docs excluded.

## Acceptance and focused/final verification mapping

| Criterion | Production path/decisive proof | Checks/evidence | Status |
| --- | --- | --- | --- |
| FC1 six deliverable docs | system_addresses, transport_architecture, inter_channel_agent_communication, agent-authoring, custom_tools, departments name actual principal/direct/binding behavior, public desk vs agent independence, routed-domain external prerequisites and actual configuration | trace code/producers/consumers; docs/boundary scripts and independent doc review | NOT EXERCISED |
| FC2 combined implementation | accepted principal/roles/origin/retirement/lease/native ownership contracts coexist with newer Cloudroom/skill bundles/main changes | trace conflict/caller effects; focused principal/agent/relay/notice/role/authorization/local-agent/import tests before final gate | NOT EXERCISED |
| FC3 final repository gate | fresh migration/checksum/SQLx metadata, format/whitespace/offline/clippy/reachability, database-backed suite/network isolation, stack/lease budgets, generated CSS/browser protocols/CI extras all pass | scripts/check.sh once after implementation; compare .github/workflows/ci.yml and run missing gates; retain actual commands/status | NOT EXERCISED |
| FC4 independent integration/retention | current artifact, requirements and retained evidence independently pass; no unresolved affected findings; normal main integration/push retains concurrent work | B notes/findings/PASS, validator --require-tracked, snapshot/export identities; normal push/remote SHA | NOT EXERCISED |

Implementation focused checks: locked/offline all-targets after conflict resolution, fresh schema
queries and affected suites selected from actual caller/contract trace. Final full suite once
implementation/docs/assets are complete; after fixes repeat affected checks or broaden when shared
impact warrants. Inherited7 strict lint findings and previous CSS/CSP failures remain failed until
fixed/dispositioned with evidence; no full-green claim yet. Raw historical terminal/patch logs stay
byte-exact; staged material source/record Markdown whitespace passed, raw-record advisory retained.

## Resources and next action

Prepared owned PostgreSQL18.7 maintenance cluster51167/mac02, data
/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-final-pg-gle6llrk/data, PID36486;
en_US.UTF-8, pg_stat_statements top/utilityoff, parameter logs0/0, max_connections100. No app DB yet.
Do not reset shared52471/5432/55437 clusters. Use new pa03_p04_ disposable DBs and task-local outputs.
Compiler may reuse the serialized task cache /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/target
now p03 workers are complete; record all commands with explicit cwd. New worktree has no npm deps.
No local Docker/gh; public GitHub Actions reads and SSH git access work. Use temporary integration
branch CI for exact Docker/Linux/Node24.21.0 gates before normal main push, rather than claim local
unavailable gates pass. Local Node26.13.1; prepare pinned task-local runtime as needed. No push yet.

NEXT: trail map/context in new worktree, inspect applicable guides/current main, fetch/recheck and
merge main. Preflight d543787f/common103004a8 had73 overlap/17 conflict paths; actual merge may
change. Trace preserved contracts before edits, resolve coherent integration, then docs/focused proof.

## Main merge started

Incoming main remains d543787fd0ca82ca0976a7e0ea1d9d73f325423b, common103004a8; local reviewed
producer3fceb7d3. git merge --no-commit --no-ff preserves both accepted ancestries; current unmerged
paths/log will be inventoried before edits. Source is not frozen/review-ready. User-supplied Trail
discovery instructions supersede the checked-in Graft paragraph; trail map refreshed source context.
Generated .trail/cache is ignored via an appended local Git info/exclude entry (no tracked file,
implementation, evidence, or CI change). Original user config remains untouched. Homebrew Python3.13
is available; default python3 is Apple3.9. Use3.13 for final validator/model tooling as applicable.

## Combined authority architecture gate

D1/r1 in logs/D1-r1.md defines the shared NOWAIT/principal/role integration and decisive competing-lock proof. Dependent edits wait for independent PASS; mechanical conflicts/docs may proceed. Initial source remains the unmerged producer/main snapshot.

Root accepted D1/r1 after independent 03PA-P04-D1-R1/N1 PASS, decision25ad97ff…19d68 and recomputed source2cfa7782…d6d76 with unchanged seven exclusions/coordination22. No actionable findings. Outer-entry account-prefix obligation explicitly retained. Source freeze released; implementation begins.

## B1 integration progress

- Resolved all17 conflicted material files, preserving both role/principal behavior and incoming native MCP/skill-bundle/main behavior. Reconstructed both complete SQL tails around their shared function terminator; fresh SQLx migration PASS against ownedpa03_p04_dev/51167 (merge-fresh-migration1.txt).
- Extended existing RowLocks through exact association/role-selection/account readers. Native outer workflow entries authorize the frozen operation account union before company ownership; nested checks revalidate the same immutable source. Direct native entries acquire their actual actor account first. Full shared authorization/claim/lease and publication guards retained.
- Native ledger now records principal_id alongside desk/thread history. Shared AssociationColumns owns conversion; source facts/live checks and workflow sessions preserve it. Generic ledger immutability already includes the added field; deferred witness also compares it with the run.
- Updated all six deliverable docs, including independent public desk/direct-agent controls, lifetime bindings, conversation grouping, relay identity and external routing prerequisites. No future Plan04/05 behavior claimed.
- Added production native direct-source publication/replay/immutable identity, competing principal/thread auth/admission/no-write, foreign direct-conversation refusal cases. Added actual executed role capture + production native-selector competing ownership/role/revision reader proof; this layer test is not a claim that an external native CLI was exercised.
- Initial merge-offline-check1 failed: native broker lacked explicit escalation None and an incoming continuation fixture used removed user reviewer references; corrected to current principal type/shape. Initial new focused-test compilation also exposed response field/Debug mistakes; retain merge-native-focused1 failures and correction rerun. Full-suite status remains NOT EXERCISED.
- Environment: task-local Node24.21.0 checksum verified; GNUcoreutils and Python3.13; task-owned PostgreSQL51167, pa03_p04_dev/test, serialized existing targetcache. Frontend npm ci and Chromium preparation PASS. Plugin133tests,14guard tests, typecheck and packaging PASS with pinned Node; logs retained. Incoming main CI failure jobs retained separately (main-baseline-actions-jobs.json), not an unrelated-failure disposition or green gate.
- Last checked coordination23; architecture/requirements unchanged. Working tree not frozen for batch review.

## D1/r2 broker reviewer integration gate

Native-focused2 compiled4690 tests;34 matched,10PASS/23FAIL/1ignored. Every23 failure shares NativeMcpNotAdmitted. Existing application native broker units run directly (same compiled source identity; no Rust rebuild) isolate registry.role on removed kind:user reviewer source;2negative unitsPASS and positiveFAIL. Retained native-broker-unit-diagnostic1.txt. New D1/r2 proposes typed current principal capture + immutable historical broker identity, incarnation-specific broker key, exact live binding and historical restore without live lookup. Dependent schema/producer edits gated. Shared outer native source prefix consolidated into existing workflow/local_agents/delivery owner, boxed at both callers to bound poll frames; no invariant change. Source is now frozen for architecture review.

Additional isolated diagnostics from the merge-native-focused2 binary (same unchanged exercised source paths; later shared-prefix consolidation not compiled): workflow_role_52PASS including new exact captured-role NOWAIT test; address_tests52PASS; relay_identity_8PASS; current-source CSP scanner1PASS. Retained merge-existing-binary-focused-{1,2,4,5}.txt. First multipart filter collected0; explicitly NOT EXERCISED, not a passing check. Retain its0 output and correct the module filter using merged-binary-inventory1.txt. This is affected-path evidence only, not full verification or current combined runtime acceptance.

D1/r2 source freeze reconciliation: original/root repeated en_US.UTF-8 snapshots d36ece50…e1583a and reviewer C.UTF-8/explicit root C snapshot0ea4417d…d4604 are byte-identical source/file sets (1537paths); GNU sort's untracked hash stream uses locale-sensitive order. Retained both snapshots and D1-r2-snapshot-locale-disposition.json. No material file changed after original freeze. Future snapshot commands pin LC_ALL=C LANG=C; DB/runtime tests keep en_US.UTF-8. Candidate workflow improvement: snapshot script must pin collation internally (evidence in these logs); no skill/AGENTS edit during this run. Corrected multipart diagnostic actually collected1PASS in merge-existing-binary-multipart-corrected.txt.

## Root architecture acceptance and safe stop

2026-11-15: root accepted D1/r2 on completed independent03PA-P04-D1-R2/N1 PASS, exact decision
f0de7324…0a388 and STOP-source-C.snapshot.txt canonical0ea4417d…d4604 (equivalent assigned UTF8
d36ece50…e1583a). Seven exclusions and HEAD3fceb7d3 unchanged; notes/findings/coordination23 pin
retained. No actionable architecture findings. Dependent broker principal capture is authorized by
the gate but not implemented. User then explicitly requested handoff and stop; no further
implementation started. Coordination24 records status only, with unchanged accepted requirements.

Stopped owned PG51167 cleanly (STOP-owned-postgres.txt), retaining pa03_p04_dev/test and cluster
files. B completed, no root-owned cargo/test/Node worker remains. Uncommitted merge and all current
changes preserved; STOP-{git-status,staged-paths,unstaged-paths,untracked-paths}.txt retain inventory.
HANDOFF rewritten as authoritative resumption guide. No main/temporary push or new source commit
occurred. Plan03 p04 and full gate remain incomplete; Plan04/05 remain queued. Stop until user resumes.

## Resume implementation/check progress

D1/r2 §§8–11 implemented in the owning application/registry/schema/provenance seams. New
identity tests cover wrong-account/foreign insertion, immutable updates and principal replacement;
direct-native populated company cascade added and busy-owner test includes actor principals.
Fresh owned init migration PASS (resume-fresh-migration.txt). Seven boundary checks PASS
(resume-boundaries.json); plugin typecheck/build/tests/guards/package and CSS PASS
(resume-plugin-gates.json). Initial focused compile FAIL from request.clone() in new test; fixed
by constructing a fresh fixture request (no production API change); failed log retained.
Latest main tooling-only commit2df562d6 merged as e5a13246; sole conflict was identical .trail
ignore entries in different order, resolved by retaining both. No runtime source changed there.

Additional browser protocols25PASS; run-count browser protocolPASS; scoped macOS network isolation
preflightPASS; knowledge artifact testsPASS and Doop probe boundariesPASS. Doop runner Python3.13
unit gate45PASS/1ERROR: strict TLS reports Missing Authority Key Identifier. Its unchanged baseline
input identity is retained; full helper inventory and platform disposition are still being checked.
Do not weaken TLS or claim this check passed. Current main's Actions runs also fail browser install,
memory integration and external Doop gate; recorded jobs are baseline evidence, not waivers.

The first compile ended naturally with one E0599; its owned PID had already exited when a
checked attempt to stop failed compilation inspected it, so no signal was sent. Corrected
compile is resume-native-focused2.txt. All17 Doop runner/helper/static fixture inputs are
byte-identical to origin/main (resume-doop-baseline-identity.json). Tests remain at original
assertions; no source fix or TLS relaxation is introduced for this unrelated platform failure.

Doop runner alternative platform proof: system Python3.9 ran all46 tests successfully
(resume-doop-python39.txt). Pinned Node production package installationPASS
(resume-package-runtime-install.txt). The local Python3.13 strict TLS failure remains retained
as confirmed unchanged-input platform behavior, not a source regression or relaxed check.

Focused build2 succeeded in7m51s. All encountered native MCP production-path tests pass, including
new identity/incarnation/direct-cascade cases and original-actor-deleted unknown recovery.
Broad native_ also exposed deterministic fixture integration failure in incoming native_tests.rs:
raw cfg(test) AgentPersistence::create creates a principal but omits its address; the concurrent
settings update correctly requires the canonical direct address. Isolated exact test reproduced
Internal(Company agent has no direct address) in owned pa03_p04_diag_test.
Correction completes only NativeSetup's fixture with the existing production address-registration
helper (visibility narrowed to persistence siblings); no guard is relaxed and no model/publication
behavior changes. Native-only low-level publication remains tested without a hosted model;
production company-agent creation's keyed-model prerequisite is unchanged from main.

Build2 collected4693 total library tests versus4690 retained pre-resume inventory: three new tests
(two broker-identity and one direct-company-cascade); no deletion/rename planned. Completed
native_mcp_ subset36PASS/1existing native-CLI ignore; native_broker_ compiler3PASS. Broad native_
includes many costly calibration/100001-member tests; it is still running and is not an overall
pass. It also reported cloudroom_e2e_cut_points failure; exact same binary on isolated diagnostic
database reran all8 production cut-points successfully in31.88s. Preserve original failure and
investigate its final captured cause; do not infer unrelated flake from one successful rerun.
SQLx regeneration now running against freshly migrated owned dev DB; no reset of test fixtures.

Broad native_ completed191PASS/2FAIL/4existing ignores in525.17s;100001-member retentionPASS.
Failure1 canonical-address fixture fixed as recorded. Failure2 captured ready()15-second timeout
at local_agent_e2e_tests.rs:282; same code/fixture8-cut isolated rerunPASS, so this remains an
unresolved readiness/load gap until the final combined suite (no limit/assertion changes).
Initial SQLx prepare failed on new fixture helper's wrong Agent path; corrected exact module
type and reran as resume-sqlx-prepare2.txt. Historical failures retained; no metadata success
claimed yet. Runner inventory now retained with explicit no-deletion delta.

SQLx prepare2 PASS against fresh schema; all-target dev compilationPASS in3m01s. Regenerated
.sqlx is byte-unchanged (all new native SQL uses runtime queries). Exact compiled-binary filters
now independently report native_broker_3PASS and native_mcp_36PASS/1existing ignore; logs
resume-exact-native_broker.txt/resume-exact-native_mcp.txt and binary SHA retained. These exact
checks use the previously built native-correction binary; only later NativeSetup fixture repair
and helper visibility differ from that binary. Rebuilt fixture/full-suite proof still required.
Strict all-target offline clippy is now running. No threshold or guard has been relaxed.

Fixture audit found the existing test_support/native_agent.rs::register_native_agent_address
helper already supplies this exact unpublished-fixture namespace contract, with explicit limits.
Use it in incoming NativeSetup rather than the temporary local wrapper/new production-helper
visibility. No global AgentPersistence fixture behavior changes are needed: the existing
postgres_agent_persistence_works already calls the shared helper and independently passed.
This supersedes the earlier proposed local address helper/visibility change; final source will
retain the existing production visibility and reuse the test-support owner.

## B1-R1 whole integration review freeze

HEADe5a13246f054325ac97ad5c2355de3b927ac1002; review base3fceb7d3435f3b821c3b37820200e1c5400206d9;
canonical C-locale digest `42490f46b0a9225c43501d04fef6db942d506a876769c43b3865823754530ed1`, exact same seven exclusions as above.
Material source frozen while B reviews the whole integration batch, not just this resume delta.
Author may continue checks and excluded records only. Preflight evidence manifest records actual
passes/failures and pending final gates. Strict clippy preflightPASS3m53s before fixture simplification;
final full gate must rerun it on the frozen source. NativeSetup now uses existing test_support
register_native_agent_address; production helper visibility restored and temporary wrapper removed.
Current fixture check compiling; final verification and B acceptance still pending.

Current rebuilt fixture checkPASS8/1existing ignore (resume-native-fixture-check.txt), including
the previously failing concurrent publication/admission case; existing shared native-agent
address helper used, no added production visibility. Source remains frozen42490f46…530ed1.
Temporary CI commit ef1cf778ed64ff6d2e6977ec3d0f020928e2719e created with an external alternate
Git index; exact material-file equality verified, source snapshot unchanged, current HEAD/index
unchanged. HTTPS push unavailable (no username prompt); existing SSH authentication succeeded.
Only codex/plan03-p04-ci-20261115-ef1cf778 was pushed; no main publication. CI identity/push/run
logs retained. Final local scripts/check.sh running now in final-check1.txt on frozen source.
B /root/p02_reviewer acknowledged whole integration review and source/coordination identities.

## B1-R1 corrections — all findings together

Independent B returned CHANGES REQUIRED03PA-P04-B1-R1/N1 on unchanged42490f46…530ed1:
F1medium verification gap (actual role-origin native continuation/actor+requester contention),
F2low automatic-desk UI creation help. Source freeze ends for this correction batch; decisions
D1/r1/r2 remain accepted. Root will implement both, run focused proofs and final full gate, then
request one combined correction/integration recheck. B did no runtime tests; no runtime acceptance.

Owned final-check1 was deliberately stopped at Compile tests before any full runtime suite,
exit143, to incorporate all review corrections. Its earlier CSS/JS/deploy/format/whitespace/
boundary/evidence/offline/clippy/migration/SQLx-check stages passed on42490f46; no full-suite
claim. Only exact-owned scripts/check.sh and3descendants stopped; EXIT cleanup handled its own
check DB, other processes/clusters untouched. Retain log as aborted preflight, restart full gate
after corrections. CI candidate1 continues on its immutable earlier source as comparison evidence.

F1 trace contradicted D1's original native Agent-origin assumption: FrozenAction::local_agent
explicitly rejects it, and the sole producer emits Workflow origin. Proposed correction is immutable
logs/D1-r3.md SHA2563ef997db3f23aa25babd827570f8edc2fdfcf0f69c51bdca15f76f0bc1d050cd, coordination27. Obtain B architecture/criterion acceptance before
dependent new proof. Preserve strict origin guard and actual actor/requester native boundary
proof; no new agent-local tool or forged producer. F2 wording corrected independently.

Root matched/accepted03PA-P04-D1-R3/N1 on exact decision/source, coordination28. B amended F1:
unsupported native Agent-origin fixture withdrawn; genuine Workflow-origin actor/requester
contention, rollback/retry and strict-origin rejection remain required. F2 reviewer-resolved.
New test fixture will use a real application-authored stored message and actual intent admission
to distinguish the privileged run owner from the original member requester, then execute normal
local-agent park/native observations and native MCP pre-body/admission/live guards. No new
native producer or schema/authorization behavior. Helper-level role test stays explicitly narrow.

D1/r3 dependent source test implemented (local_agent_mcp_source_tests.rs, two registered tests).
Fixture writes an actual internal application message by an existing admin, admits its intent
through MessageAdmissions/WorkflowService under owner run-as, and verifies production requester_on
resolves a distinct requester. It then parks a normal local-agent worker, accepts native protocol
observations and contends real actor/requester/native-revision locks at native authentication,
admission and live preparation, with full effect-state comparison, release/retry and exact
captured revision checks. A separate guard test starts from a real saved local action, proves its
valid base, changes only origin to Agent, and asserts constructor rejection without state changes.
No special native producer, role fixture forgery or authorization/schema change. Current all-target
compile check running; runtime proof not claimed yet. Existing helper role test remains narrow.

CI candidate1 results: main verify stops at Install browser for protocol tests (also failed on
main); doop-integrationPASS; memory-integrationFAIL before any step; separate Memory service
imagePASS/verifyFAIL at Extracted-copy full gate. Retained job metadata; root collecting logs
before any failure disposition. These checks concern earlier-source CI candidate1, not corrected
current source. No main push, final acceptance or Plan04 start.
