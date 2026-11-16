# 04RU integration-main — canonical authority after main merge

Workflow implement-astra-reviewed-2-3; root/A /root user-confirmed Sol6.1/xhigh;
retained independent B /root/p04_reviewer requested Astra/high, actual metadata unexposed.
Root owns TASK/HANDOFF/coordination, B owns REVIEW/notes/findings. Worktree
/Users/mac02/orca/workspaces/openbot/plan03-04, branch feature/plan03_04;
start/review-base/HEAD60b9a28b9f5b39981d804d0fdaeab5c75916c65d. Clean source baseline.
Exact snapshot exclusions: this integration docs directory and exact Plan04 COORDINATION.md.
Authority /Users/mac02/orca/workspaces/openbot/plan03-04/plan/04-root-unit/COORDINATION.md,
revision2, notice04RU-C001/r1 ACKNOWLEDGED; writerroot. No pending outside request.

## Prerequisites and authorized scope

p01 accepted in commitd4bbde66, source2a5714f52927de4b4b7d261ba970d95d2ed526d768f80ced4009c8b2dd7ab793,
independent04RU-P01-B1-R2/N1 PASS. Main03f0bda2 brings reviewed attribution service;
merge60b9a28b retains complete reviewed root init schema plus complete append-only
attribution block (resolved byte-for-byte ours + remote append). Imported947paths
are existing main, not all integration authorship. User authorizes main integration,
fixes/tests/commits/push and continuing Plan04, stop before05. Doop C008/r1 deferred
under fix-doop; do not claim whole CI green. Final full gate after p02/p03 remains.

One cohesive small integration batch B1 ports new attribution membership readers,
keeps locks/permissions, updates the fixture, and validates terminal retained evidence
in existing populated root teardown tests. No new schema, rollout or compatibility.
Then accept integration before p02. No new architecture gate: use p01's accepted
canonical membership and lock pattern and imported unchanged lifecycle ownership.
Any new costly contract/timing change reopens a recorded gate before dependent edits.

## Traced behavior and decision

Attribution COMPANY_JOIN currently references removed company_members in list/detail;
set_status locks verified account+session, company FOR UPDATE, then old member row.
List starts from the authenticated user's live person principal, joins its company,
and reads role from membership_of(user,company). Status preserves account/session→
company→underlying root membership row locks, reading its role only through canonical
membership. Company FOR UPDATE already serializes all root-role/membership writers.
No role cache, fallback, new permissions, lock-order inversion or new effect protocol.
Root seed fixtures reuse insert_root_member at PUBLIC level for admin/member.

Imported lifecycle and outbox rows deliberately survive owning company/account deletion.
Extend root teardown capture/assert to preserve prior outbox rows exactly, append
exactly one versioned deleted event matching terminal lifecycle, and keep no-live-row
scan strict for every other company-scoped table. Global installation comparisons
remain exact. No deleting tombstones or broadly excluding unexplained tables.

## Acceptance mapping / focused checks

| ID | Production path / decisive assertion | Check |
| --- | --- | --- |
| I1 canonical access | real SSO bearer company list/detail owner/admin/member/foreign, live downgrade/removal and pagination reflect root facts | attribution_ focused suites, zero active exact old-table inventory |
| I2 mutation fences | real authenticated status writes preserve existing company/member locks and optimistic version; competing requests yield one winner and one conflict | existing auth tests + new competing status case |
| I3 owning teardown | direct/workflow-native/refusal/protected-action histories and tree/docs × company/account gone; exact installation reservations and versioned terminal attribution evidence retained | root_department_company_and_account_teardown_ |
| I4 inherited interfaces | all21required HTTP auth cases, real attribution contract/worker tests; fresh combined migration/SQLx/offline typecheck and reachability | authorization_tests, fresh owned schema, all-target prepare, source_modules |

Retain failed commands, inventory before/after if moving tests, exact manifests/digests
and one whole-B1 independent code/proof review. Reuse p01 proofs only for unchanged
contracts; the changed attribution readers and new tombstone effects require execution.
Full suite/stack/lease gates occur once after whole Plan04, not after this integration.

## Resources / next

External base env pa03-p04-env.sh under cache thr_jt43h4qc8d (Node24/GNU/Python3.13,
PG18 localhost51167/task target/en_US.UTF-8). Allocate fresh pa04_m01_schema1 and
pa04_m01_core1_test for combined init; preserve all prior schemas/binaries/evidence.
Task-only Colima openbot-plan04/Docker29.5.2, selected by pa04-docker-env.sh, retained
for final checks; default context/other workers untouched. No active test worker.
Context controls unexposed. Next implement bounded reader/fixture/tombstone adaptation,
run focused proof, snapshot+manifest, independent B review, then p02.

## B1 implemented / runtime proof in progress

Ported COMPANY_JOIN through the authenticated user's live principal/company and
canonical membership_of, preserving list bounds/pagination. set_status retains
verified account/session→company FOR UPDATE→underlying root member FOR UPDATE,
reading role canonically. Existing permissions/refusals unchanged. Seed fixtures
reuse insert_root_member PUBLIC. Added actual concurrent admin status HTTP requests
with one200/one409 and exactly one lifecycle version transition.

Root teardown now snapshots prior attribution outbox rows and version, verifies
exact old-row retention, one terminal deletion event matching terminal lifecycle,
and keeps all other scoped-table absence checks plus exact installation tombstones.
No schema/production lifecycle change. Fresh combined pa04_m01_schema1 migration
PASS0.695s and locked/offline all-target SQLx prepare PASS2m28s.
CURRENT exec11985 builds/runs network-isolated attribution_, populated root
company/account teardown and authorization_tests (logs/B1-focused-tests.txt).
Source held stable. Exec95291 runs unchanged source-module checker then exact old
table inventory. Preserve all earlier p01 schemas/accepted artifacts.

## Completed integration proof and learning-check disposition

Fresh combined focused56:55PASS/0FAIL/1ignored in58.46s after4m17s build.
Required21HTTP authorization cases allPASS, four populated root teardown tests
(all company/account counterparts), real role downgrade/removal/pagination, current
session/security-revision/refusal/lifecycle rollback/outbox proofs and new competing
admin status one-winner allPASS. Ignored real browser SSO is explicitly NOT EXERCISED
and remains a required separate final integration gate; no Chrome proof claimed.
Reachability8PASS9.43s. Active exact old-table inventory empty. Current runner4738
vs acceptedp014724:0removed/14added (13imported main tests,1new competing test).
Current binary open_bots-17137d9956c1beec retained in task target.

Default Trail check after merge reports one lexical false positive and400KB branch
diff cap. Learning l-2cfeeacd (shared browser cleanup preserving writes) rules out
per-click localStorage keys as an atomic-cleanup solution. It wrongly matches the
unchanged accepted SQL comment at init schema14090 about native/workflow peer FK
owners. This integration changes no browser SDK cleanup/localStorage key protocol,
and keeps those already-reviewed database ownership/FK invariants. Retain this
reason in the integration commit message as required; do not weaken native FKs or
edit a teammate's learning. Scoped Trail check against integration base60b9a28b
covers this small current delta without that unrelated earlier SQL comment.

Frozen staged source09f2972f563e400b3553eb64d1a834d9cf942ab944d130180463f2410d22c4a8,
HEAD/base60b9a28b9f5b39981d804d0fdaeab5c75916c65d. Exact exclusions current
integration docs and exact Plan04 COORDINATION.md. Coordination2/04RU-C001r1
pinned in logs/coordination-r2.md. Prepare immutable manifest and whole-B1 review.

## Integration accepted

Root ACCEPTS integration source09f2972f563e400b3553eb64d1a834d9cf942ab944d130180463f2410d22c4a8
after independent04RU-MAIN-B1-R1/N1 PASS, no findings, matching final source/HEAD/
coordination2/C001r1. Manifest and all required exercised affected checks retained.
Real browser SSO/final repository gates remain for final Plan04; not claimed here.
Root will publish coordination3 with C001 integration response satisfied (same
requirement r1) and commit this reviewed small integration with documented Trail
false-positive disposition, then start p02. No Plan04 push or full completion yet.
