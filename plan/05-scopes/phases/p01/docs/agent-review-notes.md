# Plan05 p01 review notes

## 05SC-P01-D1-R1-N1

- Date/workflow/stage: 2026-11-17, implement-astra-reviewed-2-3, B1 architecture
  gate D1 revision 1. Reviewer `/root/p05_reviewer`, requested Astra/high with
  independent context; additional actual-setting telemetry unavailable.
- Identity: worktree `/Users/mac02/orca/workspaces/openbot/plan03-04`, branch
  `feature/plan05-scopes`; start/base/HEAD d748c2ff64309903449a3d860377a5e0385377a4;
  digest 3774130c5113a407f3af1c3c37f8c288742f2365b02c1d5a741a34901719f33d,
  exclusions exactly `plan/05-scopes/phases/p01/docs` and
  `plan/05-scopes/COORDINATION.md`.
- Inputs: original README and 01–04 documents, applicable root/source/application/
  persistence AGENTS, [retained TASK/D1 and AC/impact mappings](logs/05SC-P01-D1-R1-TASK.md),
  [review](REVIEW.md), and [pinned coordination r1](logs/D1-R1-coordination.md).
  Authority is `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`;
  05SC-C001/r1 and C002/r1 plus inherited contracts read and acknowledged. Current
  authority matched pinned copy; source digest independently matched at review end.
- Coverage: Trail map, namespace ensure/retire, principal create/restoration callers,
  agent/member/account/department retirement, root bootstrap/join, schema namespace/
  lifecycle retained records and uniqueness, save freezing and actual service request
  types. Read real exact source spans for truncated relevant graph results. Reviewed
  dependent 02/03 contracts for snapshot/namespace-ID lookup and provenance boundaries;
  no future implementation acceptance implied.
- Findings: [05SC-P01-D1-R1-F1 and F2](agent-review-findings.md). The changed call
  into provisioning needs explicit prefix acquisition before principal writes; current
  upsert callers cannot all inherit the lock-order claim. Service opaque provenance
  is not present in current request types. A independently notified B of the service
  gap; B verified both client and service source before recording F2.
- Sound decisions inspected: retained owner UUIDs plus insert-time tenant/kind validation
  accommodate deleted owners without blocking remote cleanup; lifetime person rejoin
  requires a fresh namespace; disabled owners use the explicit later-enable lazy
  backstop; reparent/child membership do not change ownership; subject cannot grant
  scope. Namespace read consumers must switch together; tenant lifecycle owner kind
  still represents the company independently of the namespace enum.
- Verification: inspection only. No behavioral tests, database migrations, or Doop
  checks executed by B. Proposed AC1–5 tests remain NOT EXERCISED at this gate. No
  implementation evidence manifest is required yet. The proof plan is broadly
  appropriate once F1's actual upsert race and F2's deferred cross-service obligation
  are explicitly mapped. Tests of creation/provisioning do not alone cover an existing
  principal upsert that runs on message production.
- Outcome: CHANGES REQUIRED for D1/r1. Correct both findings in one revised decision;
  recheck the delta. No unrelated prior-phase repair requested.
- Process feedback: batch size and architecture gate are appropriate for this schema
  and ownership change. Candidate narrowly scoped lesson: when a creation helper is
  also an existing-owner upsert, adding lifecycle effects requires inspecting its
  first existing-row lock and non-creation callers. Existing workflow instructions
  already demand this; no instruction edit is justified by this round alone.

## 05SC-P01-D1-R2-N1

- Date/workflow/stage: 2026-11-17, implement-astra-reviewed-2-3, B1 architecture
  correction recheck D1 revision 2. Same independent reviewer, settings, worktree,
  branch, start/base/HEAD and exact snapshot exclusions as R1-N1.
- Artifact: 3774130c5113a407f3af1c3c37f8c288742f2365b02c1d5a741a34901719f33d,
  independently recomputed unchanged before return. [D1/r2 TASK and mappings](logs/05SC-P01-D1-R2-TASK.md)
  retained. No project source edits exist; R1 source-inspection evidence remains valid.
- Delta coverage: D1/r2 explicitly takes company then agent before agent principal
  upsert, and account then company before person resolution; existing held prefixes
  are reused. It centralizes ensure/trigger owner locking and exact metadata
  revalidation; includes competing actual existing-principal upsert/delete in both
  orders with no deadlock/timeouts; preserves the independent deferred-FK proof.
  P01's local nullable provenance fields are separated from p02's not-yet-existing
  remote subject contract, query and purge implementation.
- Coordination: authoritative `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`
  revision 3 read, with 05SC-C001/r1, C002/r1, C003/r1 and C004/r1 acknowledged.
  [Pinned r3](logs/D1-R2-coordination-r3.md) matches current authority. C003 adjusts
  required final source verification to precede phase03 publication; C004 explicitly
  carries the p02 gate and real service proof obligation. Neither broadens p01 code.
- Outcome: **PASS D1 revision 2**, resolving R1-F1/F2 at decision level.
  Implementation/behavioral verification remains NOT EXERCISED by B. No tests or
  Doop execution; no implementation manifest asserted. This pass does not accept
  future p02 subject/purge behavior or certify planned tests as executed.
- Process feedback: one cohesive correction resolved both issues; no repeated
  unchanged-source review was needed. No additional instruction change supported.

## 05SC-P01-B1-R1-N1

- Identity: 2026-11-17, implement-astra-reviewed-2-3, p01/B1 whole implementation
  review. Independent B `/root/p05_reviewer`, requested Astra/high; additional
  actual-settings/context telemetry unavailable. Worktree
  `/Users/mac02/orca/workspaces/openbot/plan03-04`, branch `feature/plan05-scopes`;
  start/review base/HEAD d748c2ff64309903449a3d860377a5e0385377a4. Artifact
  0713361f3809b86cfbde192b30267041ea50f862c3183afcb4fe38896890dabf, exclusions exactly
  `plan/05-scopes/phases/p01/docs` and `plan/05-scopes/COORDINATION.md`. All source
  changes since base included; no preserved source changes or separate committed
  delta. D1/r2 is the governing accepted decision.
- Authority: original README/01 requirements, bounded dependent02/03 obligations,
  applicable root/src/application/persistence AGENTS and skill references; authoritative
  `/Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md`
  revision 4, C001–C004/r1 plus inherited contracts read/ACK. [Pinned notices](logs/B1-R1-coordination.md)
  matched current authority. [Retained TASK](logs/05SC-P01-B1-R1-TASK.md) contains
  the checked criterion/impact mapping. Source digest matched again at review end.
- Coverage: complete production diff and affected test conversions; Trail namespace
  owner inventory and create/retire callers, including repeated message-author
  upsert, root bootstrap/join, company-agent creation and department lifecycle.
  Inspected direct SQL owner validation, immutable namespace identity, one-live-owner
  uniqueness, missing/foreign/external/system refusal and retained cleanup IDs.
  Inspected account/company/agent/principal prefix and exact locked metadata
  revalidation, person lifetime rejoin, creation rollback, retirement cancellation
  and generation supersession. Callers preserving tenant/credential/watermark/fence
  boundaries include save/capture freezing, ready lookup, reconciler owner liveness,
  checkpoint tools, agent-history recall/capture and workflow memory steps.
- AC1/AC2 proof: new shape test uses an existing foreign department in the same DB,
  separates shape failures from uniqueness, and checks immutable ownership. Lifecycle
  tests exercise real owner writers and rollback; department reparent/member removal
  preserve IDs; delete retains removal/lifecycle state; recreated slug differs.
  Actual existing-principal upsert versus actual deletion competes in both orders.
  The updated deferred-FK test retains its independent raw-write commit refusal.
  Existing member-removal/provisioning, disable/enable, company/account deletion,
  claim fencing and rejoin assertions remain. Rejoin preserves lifetime principal
  while selecting a new namespace.
- AC3/AC4 proof: shared namespace owner columns replace the duplicate save enum;
  principal/unit exactness retains the previous personal/shared truth table. Tools
  still enforce the authenticated actor's own manual-session restriction. The final
  internal `MemoryStepScope` is Unit(Own|Root)|Agent; Own reads frozen manifest
  department context, with an admitted-run/desk-move regression; agent resolution
  still refuses uncaptured bundle keys. Old wire selectors are an explicit p02
  dependency, not a legacy row decoder. Service/client/fake production contract is
  unchanged; affected HTTP-client/fake and access-level tests provide current scope
  evidence. Real-service suites were not executed in this phase and remain a final
  gate obligation after p02's planned extension.
- Verification executed by B: nine focused real database tests PASS in
  [focused output](logs/B1-R1-reviewer-focused.txt), command and fixture identity in
  [reviewer manifest](logs/B1-R1-reviewer.evidence.json). All-target discovery PASS;
  [comparison](logs/B1-R1-reviewer-collection.json) shows six added and zero removed
  names (4,762 → 4,768). Discovery does not execute tests. No Doop checks run by B.
  An intermediate comparison made before the list command completed was discarded;
  only the completed list supplies the retained comparison and acceptance evidence.
- Verification inspected: [author manifest](logs/B1-R1.evidence.json), fresh init,
  SQLx prepare, final offline all-target compilation, formatting, whitespace and
  eight source-module checks. Retained initial focused run has 363 passed/5 failed/
  8 ignored, not a clean PASS. Each of the five actual failures maps to its exact
  successful correction name; source changes repair owner fixtures or catalog/kind
  expectations without removing behavioral assertions. Final affected step/Own
  run has 18 passed/1 ignored. Two incidental Doop tests in the author's initial
  broad filter remain excluded from accepted scope, as recorded by A. Both manifests
  validated; validator success is retention proof, not behavioral proof.
- Bounds: delivery subject-column SQL roundtrip is local schema proof only; it does
  not prove validated subject selection, remote persistence or purge. C004 carries
  those requirements to a separate p02 architecture gate and real cross-service
  proof. Full repository, Clippy, stack/lease/network and affected real-service gates
  remain scheduled before phase03 publication. No full-suite/whole-plan acceptance
  inferred from this p01 pass. Unchanged independent services were not reviewed or
  retested; routine test ID substitutions received lighter review than owner/lock
  and authorization paths.
- Outcome: **PASS** for the frozen artifact; no findings. R1 architecture F1 is
  fulfilled by implementation plus exercised contention/deferred-FK assertions;
  F2 remains correctly resolved as a p02 contract dependency, not a completed feature.
- Process feedback: the cohesive schema/owner/consumer batch was reviewable; D1
  correctly gated the costly ownership decisions. Retaining original failures and
  exact recheck names prevented green-result ambiguity. No new instruction change
  is supported beyond the existing actual-caller and wire-contract rules.
