# Plan03 p04 — safe checkpoint; stopped at the user's request

## Active B1-R1 review/final verification checkpoint

This entry supersedes all older live statuses below. User resumed: finish Plan03, then Plan04,
normal main push after each accepted/full-verified plan, and STOP BEFORE PLAN05. No force push or
deployment. Sole coordination in this checkout revision26; root/A user-confirmed Sol6.1/xhigh.

Current HEAD e5a13246f054325ac97ad5c2355de3b927ac1002, branch
room/check-last-handoff-md-under-plan-02-work-qc8d. Material source is FROZEN for independent
B /root/p02_reviewer (Astra/high, existing independent context), whole integration review03PA-P04-B1-R1.
Artifact canonical-C digest42490f46b0a9225c43501d04fef6db942d506a876769c43b3865823754530ed1,
review base/phase start3fceb7d3. Exact seven exclusions unchanged (see TASK and source snapshot).
Only excluded records/check output may change until B returns or coordinated correction boundary.
B acknowledged/pinned HEAD/digest/coord26; no new architecture gate needed. B has a provisional
proof concern: added role-selector test covers helper, not actual native role-origin continuation.
No known retained exact full-branch test; B is continuing whole-batch review and will record all
findings together. Do not make material edits while review is active. No runtime PASS yet.

Accepted D1/r2 native broker correction implemented. SQLx prepare2 PASS/current schema, .sqlx
unchanged; all-target dev checkPASS. Strict clippy preflightPASS3m53s before fixture simplification;
final gate reruns it. Native feature binary exact filters3brokerPASS and36nativeMcpPASS/1existing
CLI ignore. Broad native_191PASS/2FAIL/4ignore (525s), including100001-retentionPASS. Missing-address
NativeSetup fixture repaired by existing test_support::register_native_agent_address only; temporary
local helper/production visibility removed. Current rebuilt cloudroom_native_ checkPASS8/1existing
ignore, including former concurrency failure. Broad cut-point failure was ready()15s timeout; exact
isolated8-cut rerunPASS31.88s, cause/disposition pending final full suite. Test inventory4690→4693:
three additions, no removals (resume-test-inventory-delta.json). Failed type checks/logs retained.

Owned full local gate is RUNNING: unified exec session61370, logs/final-check1.txt. Current stage
offline all-target compilation. Use external env script, UTF8 runtime, C locale snapshots.
Do not start other Cargo checks on shared target or DB fixture checks until this gate finishes.
Rebuilt focused session72195 already finishedPASS; SQLx/Clippy sessions finished. PG51167 running
owned external data, pa03_p04_dev/test/diag_test; full check creates its own *_check_* DB. Other
clusters untouched. Follow root/B resource ownership; B currently source/read-only checks only.

Immutable CI candidate ef1cf778ed64ff6d2e6977ec3d0f020928e2719e (treebac02faf…) created via separate
external Git index: material bytes equal frozen working tree; root HEAD/index/source unchanged.
Seven workflow-record exclusions retain HEAD versions in CI tree. Candidate pushed via existing
SSH to codex/plan03-p04-ci-20261115-ef1cf778, not main. JSON/source equality/snapshot/push logs under
B1-R1-ci-*. Main CI run43732748013 and Memory service43732748071 currently running; public API
works. Separate agent-history43732746803 failed before any jobs; workflow identical to main,
retained as inherited workflow failure, do not fix unrelated YAML. HTTPS push failed for missing
username prompt; SSH succeeded. No credentials logged. B knows CI mapping and unchanged source.

Completed ancillary proof: plugin133tests/14guards/typecheck/build/package/locked runtime install,
CSS, JS25protocols/run-count browser, seven boundaries, deployment and evidence validatorsPASS.
Doop runner systemPython3.9 all46PASS; Python3.13 strict TLS failure retained with all17inputs
identical to main. Scoped macOS network isolationPASS. Full local suite + omitted browser/CI gates,
B findings/corrections, final manifests/acceptance and Plan03 main push remain; then all Plan04.
Preflight evidence manifest is immutable B1-R1-preflight.evidence.json (includes FAIL/pending,
not final acceptance). Save new final manifest when all checks finish; do not overwrite it.


## Live resume checkpoint (supersedes older statuses)

User has resumed; finish Plan03 then Plan04 and stop before Plan05 (coordination26).
Current worktree/branch remain this checkout; HEAD e5a13246 merges origin/main2df562d6.
Uncommitted native broker correction implements accepted D1/r2, with three added tests.
Compiled binary8b1fa2d2de168230 collected4693;36 native_mcp_ PASS/1existing CLI ignore and
3 native_broker_ PASS, including identity/incarnation/historical recovery. Broad native_
session22903 (log resume-native-focused2.txt) is at196/197, last100001-member retention
test still running. It has two failures: incomplete NativeSetup transport fixture (fixed
in source via shared register_agent_address_on, helper visibility pub(super)), and cut-point
test (same binary rerun isolatedPASS31.88s; original failure cause remains to inspect at end).
SQLx prepare session81199 is checking; it emitted a wrong Agent type path in the new test helper.
Corrected to crate::entities::agent::Agent; retain first failure and rerun prepare afterward.
No source change since that correction. No B runtime/implementation review requested yet; reuse
completed /root/p02_reviewer when whole-batch ready. Earlier independent architecture PASS stands.

Owned PostgreSQL51167 running; pa03_p04_dev/test reset to new init schema and fresh migrationPASS;
extra owned pa03_p04_diag_test used for isolated exact compiled-binary diagnostics. No other
cluster touched. Env script remains external and valid. Use serialized external Cargo cache.
Plugin133tests/14guards/typecheck/build/package/runtime-installPASS; CSSPASS; JS25protocols,
run-count browser, seven boundaries, deployment checks and tracked workflow-evidence checksPASS.
Doop runner46PASS with systemPython3.9; Python3.13strictTLS1failure retained, all17 runner inputs
identical to main. Scoped macOS network guard works. Full suite/SQLx/clippy/CI/B review still
pending; no completion or push. TASK has full chronological commands/results and log pointers.


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

Status: **stopped, 2026-11-15**. User: "at safe checkpoint write handoff and stop".
No background implementation should continue until the user resumes. TASK and this HANDOFF are
authoritative; REVIEW/agent-review-notes/agent-review-findings retain independent evidence.

## Packaging checkpoint — 2026-11-15

The user authorized committing this worktree so its existing implementation can be reused in
`room/check-last-handoff-md-under-plan-02-work-qc8d` without repeating the integration work.
This checkpoint completes the pending merge of `d543787fd0ca82ca0976a7e0ea1d9d73f325423b`
and includes the additional integration fixes, direct-native tests and p04 evidence below.
It is an **unfinished checkpoint**, not implementation acceptance: the known native broker
failure, remaining implementation, verification and independent review are unchanged.
No push or further implementation is part of this packaging action. The older git-state
statements below describe the pre-checkpoint state; use this branch's new HEAD for recovery.
Unrelated local settings, Trail files and Python cache files remain outside the checkpoint.

## Resume here

- Worktree: `/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-integration-20261115`
- Branch: `codex/principal-addresses-integration-20261115`
- HEAD/start/review base: `3fceb7d3435f3b821c3b37820200e1c5400206d9` (accepted Plan03 producer).
- Pending uncommitted merge: main `d543787fd0ca82ca0976a7e0ea1d9d73f325423b`.
- **All17 conflicts resolved and staged.** Further integration changes and p04 records are partly
  unstaged/untracked. Preserve them. No new commit or push in this continuation. Material source/doc
  whitespace passed. STOP git/path inventories are retained under logs/.
- Original user worktree is `/Users/mac02/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jt43h4qc8d-1/openbot`.
  Its unrelated `.claude`, `.mcp`, AGENTS, `.trail` and Plan02 handoff changes remain preserved.
  Its coordination copy is a historical pointer; do not make it a second authority.

Workflow: implement-astra-reviewed-2-3. Installed skill remains at original worktree
`.agents/skills/implement-astra-reviewed-2-3`; do not switch to main's incoming Opus variants.
Root/A `/root` is user-confirmed Sol6.1/xhigh. B `/root/p02_reviewer` is independent Astra/high,
no inherited author conversation, completed and reusable. No active child or owned build/test worker.
Context/automatic-compaction controls are unexposed; durable checkpoints retained.

Authoritative coordination: this worktree `plan/03-principal-addresses/COORDINATION.md` revision24.
I001–I004/C001–C007/r1 and revision10 unchanged/acknowledged. B's latest requirement pin is23;
24 records root acceptance and explicit stop only. User already authorized: finish/review/verify Plan03,
push normally to main, then Plan04/root-unit/review/full verification/main push, then Plan05/scopes.
No force push or deployment. Those authorizations persist, but the current stop takes precedence.

## Accepted inputs and current design gate

Plan02 and Plan03 p01/p02/p03 were accepted before p04. p03 accepted digest
`3247c1f394f32bb86dea1a31f3f1856c0f805ed0c68329149217f9c72fa9230d`, B1-R2/N1, committed3fceb7d3.
Their exact earlier records/exports remain retained. p04 implementation and whole Plan03 are not accepted.

Independent architecture PASS/root acceptance:

- `03PA-P04-D1-R1/N1`: combined native MCP NOWAIT, exact account prefix, principal association and
  sealed-role selection. Decision `logs/D1-r1.md`.
- `03PA-P04-D1-R2/N1`: native broker reviewer principal capture, immutable historical identity,
  incarnation-specific selection/live authority and historical restore without current lookup.
  **Decision SHA256 `f0de7324072fe8dbdb761897936bbbe7b362093ba5fbba0b4f34b3ad65f0a388`.**
  Decision `logs/D1-r2.md`. **Sections8–11 have NOT been implemented.** No actionable findings.

Current frozen material identity, same seven exclusions from TASK:

- Canonical snapshot with `LC_ALL=C LANG=C`: **`0ea4417d9fa23af60da326f0838d95cdbe867d7f9f4c0918e805d848299d4604`**;
  `logs/STOP-source-C.snapshot.txt`.
- Assigned/root UTF8 snapshot: `d36ece5064ce6cb57c89af50aa4a5231b7ac857aeda6ec7220708e8eb6e1583a`.
  Same files/bytes; GNU sort collation changes the untracked hash stream. Reviewer independently
  reproduced both. Keep historical snapshots unchanged. Future snapshots explicitly use C locale;
  DB/runtime tests use en_US.UTF-8. Equivalence in `D1-r2-snapshot-locale-disposition.json`.
- Exclude only p04/docs, p03/docs, p02/docs, p01/docs (all under plan03 phases), exact Plan03
  COORDINATION, exact Plan02 COORDINATION and Plan02 p05/docs. No material deliverables excluded.

## Implemented in this continuation

1. Reconstructed both complete SQL append tails around a shared function terminator and resolved
   all17 merge conflicts, retaining Plan02/03 and newer main/native MCP/skill-bundle changes.
2. Preserved `AssociationColumns` through native direct/workflow source lookup, live authorization
   and ledger publication. Native calls now carry historical target `principal_id`; deferred witness
   compares it to its run. The existing generic immutable guard includes it automatically.
3. Extended existing RowLocks through account, association, captured-role/agent selection helpers.
   Ordinary calls keep Wait; native calls keep NoWait. Exact workflow actor/requester account union
   precedes outer company ownership and is revalidated afterward. The shared prefix now lives in
   `workflow/local_agents/delivery.rs::authorize_native_source_on`, boxed at both native entry
   callers to bound poll frames. **This final consolidation has not yet been rebuilt.**
4. Updated all six required docs (system addresses, transport architecture, internal communication,
   agent authoring, tools, departments). Regenerated CSS; current check passed.
5. Added registered `management/native_mcp_direct_tests.rs`: real principal conversation publication,
   immutable witness/replay, competing principal/thread auth/admission and foreign conversation
   refusal. Added production captured-role selector contention test in role_model_runtime_tests.
6. Corrected missing native broker `escalation: None`, old continuation reviewer fixture shape, and
   direct-public-settings fixtures for main's required native_permissions field.
7. Addressed inherited seven clippy diagnostics without allowances/limit changes: wildcard patterns,
   boxed knowledge upload error responses with callers adapted, single match, price parser unwrap_or,
   fixed-pair reply hash decoding. Existing multipart regression passed. Strict clippy rerun pending.
   CSP source-scan false positive now constructs the exact hostile fixture via concat!; scanner and
   escape assertions unchanged. Current-source CSP scan passed.

## Known failure and exact next implementation

`merge-native-focused2.txt`: build succeeded,4690 tests collected;34 native_mcp_ matches:
**10passed,23failed,1existing native-CLI ignore**. Every23 failure reports NativeMcpNotAdmitted.
Direct execution of the existing broker unit tests isolated the root cause:
`native-broker-unit-diagnostic1.txt`,2negativePASS/1positiveFAIL, registry.role refuses `kind:user`.
`src/application/workflow/native_mcp.rs::source` still emits user reviewer references, removed by Plan02.
Do not weaken the compiler, sanitize boundary or tests.

After resume, implement accepted D1/r2 §§8–11 coherently:

- Add typed actor_principal to NativeBrokerIdentity; protected broker source emits literal
  `{kind:principal,id:actor_principal}`. Run-as remains the authenticated account.
- Resolve current exact person principal under existing company/member/principal authority. Persist
  immutable historical actor_principal_id on native_mcp_brokers, with exact initial tenant/account
  validation and no live FK/deletion pin. Include it in uniqueness/lookup/advisory key.
- Bind native_mcp_run_live to that exact current principal. Old immutable invocations do not acquire
  new authority under another principal incarnation; new explicit calls may select a new broker.
- Pass stored broker principal through FrozenOrigin/provenance restoration; never consult current
  principals for historical structural proof after retirement/removal.
- Extend existing compiler assertion to distinguish principal ID from account UUID. Add wrong-account/
  foreign native INSERT negatives, lifetime/replay/incarnation proof, and populated principal-native
  company cascade proof. Existing private_tests has a desk-native company cascade to extend/reuse.
- Re-read D1/r2 and trace actual callers before editing. Four NativeBrokerIdentity construction paths
  are in application type/tests, native registry and native provenance. Inspect other affected SQL
  readers/keys and live checks exhaustively. Schema change resets ONLY owned disposable DBs.

A read-only follow-up noted that native admit manually inserts runs/jobs/admission without ordinary
role capture. Its broker uses literal principal reviewers, not roles, so no role manifest is implied;
verify actual integrated guards in focused runtime tests rather than assume. No extra decision made.

## Evidence already retained

- Fresh combined SQLx migration PASS: `merge-fresh-migration1.txt`, ownedpa03_p04_dev/51167.
- Initial offline all-target check failed on broker escalation/old reviewer fixture, corrected.
  Initial new native compilation failed on response field/Debug mistakes, corrected. Keep failed logs.
- Same compiled native-focused2 binary diagnostics on unchanged exercised paths:
  workflow_role_52PASS (includes new captured-role NOWAIT case), address_tests52PASS,
  relay_identity_8PASS, CSP1PASS, corrected multipart1PASS. Logs `merge-existing-binary-focused-*`,
  `merge-existing-binary-multipart-corrected.txt`; full inventory `merged-binary-inventory1.txt`.
  Initial multipart filter collected0 and is explicitly NOT EXERCISED. These are not final current
  source acceptance: prefix consolidation/last fixture changes remain to rebuild.
- All seven transport/sample/integration boundary scripts/tests PASS: integrated-boundaries.json.
- Verified official Node24.21.0 checksum, frontend npm ci, plugin typecheck133tests14guard tests,
  reproducible package, CSS build/current check and Chromium setup PASS.
- Incoming main's failed Actions jobs retained in main-baseline-actions-jobs.json. No final-gate
  pass or unrelated-failure disposition inferred from those failures.
- No p04 runtime evidence manifest/final review yet; do not claim full verification.

## Resources and restart

Only phase-owned PostgreSQL51167 was stopped cleanly; data retained. Other clusters untouched.

```sh
cd /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-integration-20261115
/opt/homebrew/opt/postgresql@18/bin/pg_ctl   -D /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-final-pg-gle6llrk/data   -l /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-final-pg-gle6llrk/resume-server.log -w start
. /Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/pa03-p04-env.sh
```

The env script sets pinned Node, GNUcoreutils, Python3.13, target cache, UTF8 and disposable
DATABASE_URL/TEST_DATABASE_URL (no credentials): pa03_p04_dev/test on127.0.0.1:51167/mac02.
Cluster has pg_stat_statements top/utilityoff, parameter logs0/0, en_US.UTF-8. After the gated init
schema edit, reset/recreate these OWNED databases before migration/tests; preserve other clusters.
CARGO_TARGET_DIR is `/Users/mac02/Library/Caches/openbot/thr_jt43h4qc8d/target`, serialized to this phase.
Lib-test full rebuild took about8minutes; avoid unnecessary repeated builds. Node/browser deps are
installed. No local Docker/gh; SSH git and public GitHub Actions reads work. Intended final gate uses
local checks plus a temporary integration branch's CI for Linux/Docker prerequisites before main.

## Remaining sequence

Implement D1/r2; focused native/compiler/approval/lifecycle/history/incarnation and direct/role/API
proof; SQLx metadata regeneration; strict clippy and all affected checks. Then one complete
scripts/check.sh gate plus missing CI/browser/Docker/memory/Doop gates, graph refresh, test inventory
accounting, retained evidence manifests, independent B whole integration review, corrections and
exact artifact acceptance. Recheck current main and integrate any new changes safely. Only after
Plan03 completion push normally to main. Then Plan04/root-unit with its own phased records/full gate/
main push, then Plan05/scopes. **None of that should run until the user resumes.**
