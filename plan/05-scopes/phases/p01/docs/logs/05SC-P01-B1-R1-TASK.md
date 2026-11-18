# Plan05 p01 — unit/principal namespaces and owner lifecycle

Status: implementation complete; final focused verification in progress, then independent batch review.
Workflow implement-astra-reviewed-2-3. Root/A /root, user-confirmed Sol6.1/xhigh;
B /root/p05_reviewer, requested Astra/high with independent context. Actual worker/context controls unexposed. Record root this
directory; root owns TASK/HANDOFF, B owns REVIEW/notes/findings when assigned.

Worktree /Users/mac02/orca/workspaces/openbot/plan03-04, branch
feature/plan05-scopes. Start/base/HEAD d748c2ff64309903449a3d860377a5e0385377a4.
Clean at startup, no preexisting dirty work. Exact snapshot exclusions planned:
plan/05-scopes/phases/p01/docs and exact plan/05-scopes/COORDINATION.md.

## Requirements and authority

Original plan/05-scopes/README.md and01-unit-and-principal-namespaces.md; retain
02scope snapshot/tool and03knowledge consumer obligations while establishing the
foundation. Final full gate once after all phases per04 document and root AGENTS.
No backward compatibility: edit init schema, use fresh owned DBs, no old-row shims.

Authoritative /Users/mac02/orca/workspaces/openbot/plan03-04/plan/05-scopes/COORDINATION.md,
root writer revision1 checked at startup. ACK05SC-C001r1 (only05/push/STOP, no06,
Doop separate) andC002r1 (accepted trust boundaries). Inherited01DEP-C002r1,
04RU-C003r2/C004r1/C005r1 acknowledged. Plan04 prerequisite implementation86d225cc,
publicationd748c2ff, source9b394b31…e2f0e, review04RU-P03-B1-R1-S1/N1 PASS.

## Cohesive B1 and architecture gate

Trace NamespaceOwner and namespace/owner creation, retirement, capture/delivery,
reconciler, memory step/tool consumers, manual-person versus agent identity and
admission facts before writing D1. D1 must resolve schema shape and deleted-owner
retention, disabled/enabled provisioning semantics, retirement/rejoin/tombstone
identity, transaction/lock ordering and affected consumers together. Preserve
current classification/credential/incarnation guards and service opaque contract.
Architecture PASS is required before dependent implementation.

| Criterion | Production path / decisive proof | Focused evidence plan | Status |
| --- | --- | --- | --- |
| AC1 namespace shape | only unit/principal; one live owner; tenant/type refusal; retired owner retained | real fresh-schema shape/tenant/live uniqueness tests | tracing |
| AC2 owner lifecycle | actual create/retire/reparent/membership paths, rejoin new namespace, no external/system namespace | production writers plus competing create/delete/enable cases | tracing |
| AC3 consumers | steps/tools/capture/delivery/reconciler decode only new owner schema; provenance subject fields | affected memory/protocol/compiler/checkpoint families; caller dispositions | tracing |
| AC4 independent service contract | opaque namespace IDs/subject, unchanged credential and watermark/egress contracts | production client/fake contract and levels/fencing/replay tests | tracing |
| AC5 phase verification | fresh init, SQLx regeneration, fmt/offline all-targets/Clippy/module reachability and focused DB tests | phase-owned DB/cache, retained logs/manifest/review | pending |

No tests moved/deleted yet. Save discovered names before any extraction/removal.
Phase02/03/04 are authorized only in original dependency order after accepted
prerequisites; one final full repository gate after all implementation, excluding
separately deferred Doop and unaffected independent-service reruns as authorized.

## Resources / next

Previous p03 disposable DBs/images were cleaned; preserve all p01/m01/p02 and shared
PG18localhost51167/Colima services. Existing task Cargo/Node/GNU/dependency/model
caches may be reused; allocate new05-owned schemas and private env before tests.
No05 DB or process allocated yet. Next trace actual owners/admission semantics,
record D1/proof/consumer mapping, assign independent Astra architecture review.

## D1 / revision 1 — architecture gate (2026-11-17)

1. Namespace ownership is `Unit(DepartmentId)` or `Principal(PrincipalId)`;
   namespace columns are `unit_id`/`principal_id`, exactly one matching kind.
   Tenant and historical owner IDs remain plain UUIDs on retained remote lifecycle
   records. A BEFORE INSERT/owner-change trigger validates owner existence, tenant
   and principal kind under `FOR SHARE`; a foreign key to a deletable owner would
   prevent the specified retained-removal state, so the trigger enforces the same
   composite ownership invariant without a permanent owner FK. Existing namespace
   identity and owner columns cannot be reassigned after insert.
2. Owners created while memory is enabled call ensure_on in their creation
   transaction. Without an enabled tenant there is no remote resource to provision;
   owner creation succeeds without namespaces. The existing enabled-after-creation
   lazy backstop provisions only live, same-company units/persons/agents. Enabling
   does not bulk provision old owners. This follows01B2's explicit backstop.
3. Person rejoin keeps the lifetime principal (Plan03) but gets a new namespace ID:
   demotion retires every live namespace, cancels its intents and supersedes its
   lifecycle. Agent and department deletion do likewise in the existing transaction;
   reparent/member/role edits do not touch namespace ownership. Company delete keeps
   durable detached absent lifecycles as before. Tombstone cooldown/redrive unchanged.
4. Provisioning lock order stays account (for person) -> company -> agent (when
   applicable) -> principal/unit -> tenant -> namespace -> lifecycle. Owner metadata
   may be probed to choose locks, then must be revalidated under those locks; a
   changed kind/account/agent is unavailable, not a different owner. Creation paths
   already holding these locks call the same ensure helper. Preserve all independent
   tenant/epoch/credential/incarnation/fence checks at runtime.
5. Phase01 changes every namespace SQL reader and lifecycle owner decoder together.
   Existing tool/step selectors are adapted to new owners in this phase, then
   replaced by02's vocabulary/snapshot in dependency order before publication.
   NamespaceOwner has no legacy variants or decoders. Save/capture use this same
   owner type (remove duplicate SaveScope owner model); exactness targets become
   UnitScope/PrincipalScope with the existing company/personal truth table unchanged.
   No service ownership semantics change: opaque namespace and tenant IDs only.
6. Deliveries retain nullable unit_id/principal_id/subject_principal_id provenance
   as plain IDs (not live-owner FKs); owner columns identify the target namespace.
   Phase02 supplies authenticated actor and optional validated visible subject;
   subject is propagated through the service's existing opaque provenance metadata.
   A subject never authorizes a scope. Frozen payload bounds/idempotency remain.

### Contract impact / focused evidence

- NamespaceOwner: settings, namespace persistence, lifecycle reconciler, memory
  steps, checkpoint tools, agent-history recall/capture, delivery saves and tests.
  Caller/exhaustive inventories: trail NamespaceOwner, namespace.{agent,user}_id,
  SaveScope and owner_kind. Types have no reference edges; exhaustive uses traced.
- Owners: create_agent_principal_on -> agent/company-agent/onboarding/publication/
  message-author; create_person_principal_on -> root join/address/adoption; department
  lifecycle/root bootstrap. Retirement: agent/member/account/company/department.
- Proof AC1: production namespace ensure plus direct SQL rejection of foreign unit,
  external/system principal, invalid matching columns and duplicate live owner;
  nullable subject survives delivery persistence. Retiring does not lose history.
- Proof AC2: production create/delete/rejoin; competing owner/provisioning transactions
  in both orders, existing disable races and lifecycle claimant tests. Reparent and
  child membership changes preserve IDs; recreated slug gets distinct owner/namespace.
- Proof AC3/4: existing memory scope/lifecycle/delivery/capture/access-level families,
  workflow memory steps/tools/recall/history and compiler families against fresh schema.
  Contract fake unchanged unless opaque metadata requires a narrow fidelity fix;
  retain real-service integration and new behavior proof at final combined gate.
- Proof AC5: fmt, locked offline all-target compilation, fresh init, SQLx prepare,
  reachability and focused DB tests now; root full gate once at04. Preserve test
  collection before any relocation/removal. No tests removed yet.

Architecture acceptance pending independent B. Source unedited. Coordination1,
C001r1/C002r1 rechecked and ACK. Exact exclusions unchanged. Root/A actual session
settings are user-confirmed; context threshold remains host default/unexposed.

## D1 / revision 2 — corrections to architecture review

Supersedes only D1r1 items4/6; other decisions remain. No source edit yet.

- All actual principal upsert paths take the ownership prefix BEFORE the first
  principal write: create_agent_principal_on takes company FOR SHARE then agent
  FOR SHARE, before its INSERT/ON CONFLICT. This includes repeated message-author
  upserts, not just new-agent creation. A missing/foreign agent fails before the
  principal write; the existing deferred-FK regression must be updated to assert
  this earlier refusal without weakening its independent commit-constraint proof.
  create_person_principal_on takes account FOR SHARE then company FOR SHARE before
  identity/principal resolution. Existing callers that hold the prefix reuse locks.
- Namespace owner locking is centralized in a database function shared by ensure_on
  and the namespace guard trigger: probe person/agent metadata only to choose locks,
  then account -> company -> agent -> principal/unit, revalidate the exact owner
  kind and metadata, refuse changed/missing/foreign owners. This avoids maintaining
  subtly different ownership lock protocols for direct SQL and application ensure.
  Retired namespaces bypass live-owner revalidation only for non-owner state edits;
  identity/owner columns are immutable. No permanent FK blocks owner deletion.
- Add competing REAL production create_agent_principal_on upsert vs agent delete
  in both orderings, including existing principal, plus same-company/person retire
  and ensure competition; demonstrate ordinary results without deadlock/timeouts.
- D1r1item6's service assumption is contradicted by source: service SaveItem67..79,
  SaveRequest83..97 in services/memory/src/application/contract.rs carry no subject
  or generic opaque provenance, and deny_unknown_fields. Phase01 only adds nullable
  retained delivery unit/principal/subject columns. Subject-bearing service request,
  query and purge support require a narrow opaque provenance contract extension,
  production client/fake changes and affected service tests under a separate reviewed
  p02 architecture gate BEFORE those dependent edits. No existing service capability
  is claimed and phase01 cannot satisfy p02 subject/purge criteria.

Baseline `cargo test --locked --offline --all-targets -- --list` PASS, retained
logs/B1-before-tests.txt. No tests moved/deleted. Fresh owned DBs pa05_p01_schema
and pa05_p01_core_test on borrowedPG51167; private env in external session cache
pa05-p01-env.sh. Never run a second libtest against this core DB concurrently.

Coordination2/05SC-C003r1 ACK: latest user requires main publication after accepted
phase03. Run final source gate before that publication;04 can finish docs/records
and reuse unchanged evidence. Stop after05, no06/Doop scope expansion.

## Implementation in progress after architecture PASS

Independent05SC-P01-D1-R2/N1 PASS for D1r2/source3774130c…f33d; r1F1/F2 closed
at decision level, execution proof pending. Coordination3/C001–C004r1 ACK.
Root/A implementing B1 in the existing worktree. No sibling source changes.

Namespace shape, shared SQL owner locks/guard, typed owner API, enabled-owner
creation/retirement, lifecycle decoder and every production memory consumer now
use unit/principal. SaveScope duplicated model removed. Frozen delivery target
columns added; optional subject production propagation remains p02 dependency.
Legacy step/tool selectors temporarily map to new IDs within p01 and are replaced
in p02 before any user checkpoint publication. No legacy row decoder added.

Fresh init migration PASS (logs/B1-migrate.txt) on pa05_p01_schema. Baseline test
collection PASS. Compile attempts1/2 exposed mechanical test-owner conversion
errors (logs retained); no passing compile claim yet. Focused1 command active
(session48027) but likely compiles older conversion snapshot; inspect actual
outcome before rerun. Compiler sessions50091/48637 finished101. No test has run
against pa05_p01_core_test yet. Baseline list session83651 finished0.

New enduring scope_owner_tests exercises actual creation/rollback and both-order
production existing-principal upsert/deletion, direct owner/shape/tenant/uniqueness
refusals, unit reparent/membership identity stability and retained retirement/
recreated slug, and delivery subject-column roundtrip. Subject schema test is a
SQL stand-in, not remote subject/purge proof. Existing tests retain their names;
removed duplicate enum truth-table assertion is now one PrincipalScope assertion
because AgentScope/UserScope were identical domains.

### Future02 tracing facts (not decisions/implementation)

- Real run creation consumers of admission_write::insert_run include external
  admission_write::create, runtime children::insert_child_on and native_mcp::admit_on;
  autonomous_agent_storage_native_runs is a test helper. Trail call edges miss
  qualified consumers; exhaustive literal inventory supplies them. Ensure snapshots
  cover every production path, not just external create.
- A workflow can select multiple agents. Principal personal identity and lead reports
  differ per acting agent, so02's original singular RunScopes/table proposal needs
  an explicit per-acting-principal disposition; blindly unioning all captured agents'
  reports into one shared run set widens access. Manual-person own-session scope is
  an inherited constraint, not simply the binding author's principal.
- Frozen topology must distinguish chain and reports for new selectors/labels and
  namespace identity in consumption/revalidation; category alone was sufficient
  for old three-scopes but cannot identify several unit namespaces.
- Source: agent_manifest/mod.rs54..124 captures dependency closure and role context;
  agent_manifest/roles.rs62..136 loads chains boundedMAX_DEPTH+1; role_binding.rs132..
  uses workflow_target_departments, whose direct target must be evaluated against
  the actual acting agent where plans require it. Check child association inheritance
  and native MCP captures before choosing snapshot ownership/seal design.
- Another concurrent worktree /Users/mac02/orca/workspaces/openbot/plan06 has its
  own cargo target/processes; preserve them. Our instructions still stop this
  workstream before06. Do not kill other cargo processes or share their databases.

Additional02 investigation questions (not accepted design): service subject needs
actual opaque SaveItem storage, retrieval/list filtering and deletion via existing
operations; actual OpenBot operator CLI/client path is not established. Inspect
service timeline/correction/observation retention before deciding scope. A frozen
per-run seal/header may be needed to prevent late INSERT widening, beyond merely
rejecting UPDATE of each row; distinguish incomplete snapshot from an empty allowed
readset. Labels and recall refs must identify individual namespaces within category.

Focused2 compiled current batch and launched376 tests (compilation6m21s, retained
nonfatal inherited Mach-O unwind-size linker warning; no Rust compiler warning).
All5new scope_owner_tests PASS, including production both-order upsert/delete.
First observed5failures are fixture conversions:3multi-company delivery fixtures
changed f.company without changing f.root; exact memory catalog allowlist still
expects old columns/functions; child-capture assertion still expects namespacekind
user. Corrections applied while the already-loaded test binary finishes. Preserve
focused2 actual final summary/details before assigning dispositions. No production
relaxation or assertion weakening. SQLxprepare/all-target command active75251,
logs/B1-sqlx-prepare.txt; sources now frozen for that build.

The broad agent_history_ FILTER also matched one incidental Doop test through its
ancestor module path (PASS). No Doop investigation/change done. Future focused and
final library commands explicitly --skip doop --skip Doop under user deferral;
record skipped scope and avoid any whole-CI-green claim.

All5focused2failures nowPASS in logs/B1-focused-corrections1.txt. Exact summary
focused2=363PASS/5FAIL/8ignored; correction5PASS. Two incidental Doop names were
matched via ancestor paths (not1); in-scope unique non-Doop passing names366.
SQLxprepare1PASS (.sqlxunchanged), offline-all-targetsPASS, reachability8PASS,
fmt/whitespacePASS. These remain retained as pre-final-type evidence.

Final original01B1 audit completes MemoryStepScope's internal type now:
Unit(UnitSelector::Own|Root)|Agent(Uuid). Legacy wire selection grammar remains
02's original responsibility, but the runtime now resolves Own from the already
admitted immutable manifest role_context_department_id, never live tree. Root uses
company's immutable root identity. Added real admitted desk-move regression in
role_snapshot_tests; existing Root and Agent path tests remain discoverable.
Wire Own activation and shared read/write snapshots still require02. No expensive
architecture contract changed beyond accepted original01/D1 ownership decision.
SQLxprepare2active39848; then rerun affected memory step tests/new own regression,
check current all-targets and source reachability; no full suite needed until03.

## B1 final proof mapping — ready for independent batch review

Source complete, all focused corrections done. Final step-type run18PASS/1ignored,
including `memory_scope_admission_step_own_uses_the_captured_department`: actual
admission, later desk move, Own still original department and Root still root.
SQLxprepare2PASS/.sqlxunchanged; finalfmtPASS; final locked/offline alltargetsPASS
29.58s; reachability8PASS (module edges unchanged by final type/test addition).
367unique non-Doop test names passed across retained unaffected/correction/final
runs. Initial376selection also executed2incidental Doop names through ancestor
paths; no Doop claim/work or further check, subsequent explicit skip filters used.
Full repository/real-service integration gate remains scheduled03, not claimed now.

| Criterion | Actual production path / decisive assertion | Retained evidence / disposition |
| --- | --- | --- |
| AC1 | scope_owner_tests shape: foreign unit exists in sameDB, external/system principals, mismatched fields and oldkind refused23514; duplicate live owner23505; reassignment refused; delivery target/subjectUUID roundtrip | B1-focused2 newshape2PASS; delivery-column test is SQL-schema proof only, remote subject proof pending02C004 |
| AC2 | enabled production agent/person/department create; rollback leaves noNS; real unit delete retains removingNS/absentlife; recreate slug differs; reparent/roster keepsIDs; actual existing-principal upsert vs delete in BOTH orders | B1-focused2 newlifecycle3PASS plus existing memory_scope_, lifecycle_, capture/delivery claimant/disable/rejoin testsPASS; no concurrency mock substitute |
| AC3 | all namespace SQL readers use new columns; stepUnitOwn/Root and bundleAgent -> namespaceowner; tool checkpoint/recall/capture/consumption keep current authority/levels; expired namespace never remapped to wider owner | B1-focused2 memory/agent_history/retirement/address/department families; exact5fixture rechecksPASS; B1-step-type-focused18PASS; final Root labelunit assertion and real frozenOwn test |
| AC4 | memory service unchanged: random opaque tenant/namespace IDs and exact wire save/generation shapes; per-namespace credential/open/fence/epoch/watermark proofs retained | production HTTP client and ContractFake unchanged, memory adapter/integration/access-level familiesPASS in focused2; only internal ownership mapping changed; real service gates inherited unchanged and scheduled final after02extension |
| AC5 | freshinit; macros regenerated; alltargets offline currentcode; fmt/patch/module connectivity | B1-migrate.txt, B1-sqlx-prepare2.txt, B1-final-offline-all-targets.txt, B1-final-format.txt, B1-source-modules.txt (8PASS), staged whitespace pending below |

The5initial failures were asserted failures, not ignored or declared unrelated:
3stale fixture rootIDs afterchangingcompany; old exactcatalog allowlist; child
capture expected old namespacekind. Logs B1-focused2/B1-focused-corrections1 and
B1-failure-dispositions.json preserve matching full names. Compiler conversion
failures likewise retained separately; no assertion removed to turn them green.
No tests moved/deleted; 5new owner tests +1new admittedOwn regression registered.
Exactness test keeps all remaining domain variants after identicalAgent/User targets
consolidated asPrincipalScope. No service source/fake edits in this phase.

TrailcheckPASS (B1-trail-check.txt), no conflicts,10nonscoped advisory hits reviewed.
They concern different native lock/cascade mechanisms; our own production lock
prefix tests and deferredFK forced-immediate/commit refusal proofs remain. No
threshold, native-lock policy or deferred integrity constraint changed. Final
full-suite Clippy/stack/lease/network/realservice gates run once after03source.
Coordination4/C001–C004r1 rechecked/ACK; no pending crossphase request. C004subject
contract explicitly unresolved for02, not an accepted feature in this phase.

### Frozen B1 / review round05SC-P01-B1-R1

Start/reviewbase/HEADd748c2ff64309903449a3d860377a5e0385377a4. Source digest
0713361f3809b86cfbde192b30267041ea50f862c3183afcb4fe38896890dabf,
exact exclusions plan/05-scopes/phases/p01/docs and plan/05-scopes/COORDINATION.md.
Source staged and frozen for whole-batch review. No preserved preexisting changes.
Coordination4/C001–C004r1 pinned logs/B1-R1-coordination.md; source/snapshot manifest
logs/B1-R1.snapshot.txt. Evidence logs/B1-R1.evidence.json validated successfully
9PASS/1retained-resolvedFAIL (checker is retention, not behavioral acceptance).
Staged whitespacePASS. B requested to independently review all original01criteria,
actualsource/callers and proof, with02subject/grammar dependency explicit. Root
will not edit source until actual B result; only excluded workflow records maywrite.
